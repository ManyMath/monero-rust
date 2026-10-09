//! The txset checks wallet2 runs that need no wallet state
//! (`src/wallet/pending_tx_validation.cpp`).

use std::collections::{HashMap, HashSet};

use curve25519_dalek::{edwards::CompressedEdwardsY, scalar::Scalar, traits::IsIdentity};
use monero_oxide::transaction::{Input, NotPruned, Timelock, Transaction};
use monero_wallet::address::{AddressType, MoneroAddress};
use monero_wallet::ed25519::{Commitment, Scalar as OxideScalar};

use crate::epee_compat::{
    wallet2_key_images_text, ParsedSignedTxSetSummary, ParsedUnsignedTxSetSummary,
    PendingTxSummary, TxConstructionDataSummary, TxDestinationEntrySummary,
};

/// The Bulletproofs+ version wallet2 writes for the current protocol.
const BULLETPROOF_PLUS_VERSION: u64 = 4;
/// `use_rct` and `use_view_tags`, the only construction flags wallet2 defines.
const CONSTRUCTION_FLAGS: u8 = 0x03;

/// wallet2's `check_address`: both keys are points of the prime-order
/// subgroup other than the identity.
fn address_keys_are_valid(destination: &TxDestinationEntrySummary) -> bool {
    [destination.spend_public_key, destination.view_public_key]
        .iter()
        .all(|key| {
            CompressedEdwardsY(*key)
                .decompress()
                .is_some_and(|point| !point.is_identity() && point.is_torsion_free())
        })
}

fn same_address(a: &TxDestinationEntrySummary, b: &TxDestinationEntrySummary) -> bool {
    a.spend_public_key == b.spend_public_key && a.view_public_key == b.view_public_key
}

/// A destination's `original` string as an address; `None` when it is not one.
pub(crate) fn original_as_address(original: &[u8]) -> Result<Option<MoneroAddress>, String> {
    let Some(address) = std::str::from_utf8(original)
        .ok()
        .and_then(|text| MoneroAddress::from_str_with_unchecked_network(text).ok())
    else {
        return Ok(None);
    };
    match address.kind() {
        AddressType::Legacy | AddressType::LegacyIntegrated(_) | AddressType::Subaddress => {
            Ok(Some(address))
        }
        AddressType::Featured { .. } => {
            Err("destination addr string is not an address wallet2 pays".into())
        }
    }
}

fn check_original(destination: &TxDestinationEntrySummary) -> Result<(), String> {
    if destination.is_integrated && destination.original.is_empty() {
        return Err("integrated destination missing original address string".into());
    }
    let Some(address) = original_as_address(&destination.original)? else {
        return Ok(());
    };
    let spend: curve25519_dalek::EdwardsPoint = address.spend().into();
    let view: curve25519_dalek::EdwardsPoint = address.view().into();
    if address.is_subaddress() != destination.is_subaddress {
        return Err("destination addr string mismatch - is_subaddress".into());
    }
    if address.payment_id().is_some() != destination.is_integrated {
        return Err("destination addr string mismatch - is_integrated".into());
    }
    if spend.compress().to_bytes() != destination.spend_public_key
        || view.compress().to_bytes() != destination.view_public_key
    {
        return Err("destination addr string mismatch - address".into());
    }
    Ok(())
}

/// Check one transaction's construction data against itself, as wallet2's
/// `sign_tx` and `sanity_check_tx_construction_data` do.
pub(crate) fn check_construction_data(tx: &TxConstructionDataSummary) -> Result<(), String> {
    if tx.sources.is_empty() {
        return Err("transaction has no inputs".into());
    }
    if tx.unlock_time != 0 {
        return Err("transaction cannot have non-zero unlock time".into());
    }
    if tx.construction_flags & !CONSTRUCTION_FLAGS != 0 {
        return Err(format!(
            "unknown construction flags {:#04x}",
            tx.construction_flags
        ));
    }
    if !tx.use_rct {
        return Err("only RingCT transactions are signed".into());
    }
    if !tx.use_view_tags {
        return Err("must be view-tag-enabled (hardfork >= 15)".into());
    }
    // The range proof type is not checked; the protocol accepts one shape anyway.
    if tx.rct_bp_version != BULLETPROOF_PLUS_VERSION {
        return Err(format!(
            "unsupported RingCT config: bulletproof version {}, only Bulletproofs+ ({}) is built",
            tx.rct_bp_version, BULLETPROOF_PLUS_VERSION
        ));
    }

    let mut input_amount = 0u128;
    for source in &tx.sources {
        let real = usize::try_from(source.real_output)
            .ok()
            .and_then(|index| source.ring.get(index))
            .ok_or("ring sig index out of input set size")?;
        let mask = Option::<Scalar>::from(Scalar::from_canonical_bytes(source.mask))
            .ok_or("input commitment mask is not a canonical scalar")?;
        if !source.rct && mask != Scalar::ONE {
            return Err("pre-rct input in > v1 tx has non-identity mask recorded".into());
        }
        let commitment = Commitment::new(OxideScalar::from(mask), source.amount).commit();
        if commitment.compress().to_bytes() != real.commitment {
            return Err("failed reproducing real input's amount commitment".into());
        }
        input_amount += u128::from(source.amount);
    }

    let mut output_amount = 0u128;
    let mut amount_to_change_address = 0u128;
    for destination in &tx.split_destinations {
        output_amount += u128::from(destination.amount);
        if same_address(destination, &tx.change) {
            amount_to_change_address += u128::from(destination.amount);
        }
    }
    if output_amount > input_amount {
        return Err("output amount exceeds input amount".into());
    }
    if u128::from(tx.change.amount) > amount_to_change_address {
        return Err("change exceeds payment to change address".into());
    }

    let mut expected: Vec<&TxDestinationEntrySummary> = tx.destinations.iter().collect();
    if tx.change.amount > 0 || tx.destinations.len() == 1 {
        expected.push(&tx.change);
    }
    if tx.split_destinations.len() != expected.len() {
        return Err("failed checking splitted_dsts size".into());
    }

    let mut integrated = 0usize;
    let mut by_spend_key: HashMap<[u8; 32], &TxDestinationEntrySummary> = HashMap::new();
    for destination in &tx.split_destinations {
        check_original(destination)?;
        if destination.is_integrated {
            integrated += 1;
            if integrated > 1 {
                return Err("more than one integrated address detected".into());
            }
        }
        let position = expected
            .iter()
            .position(|candidate| *candidate == destination)
            .ok_or("failed checking splitted_dsts consistency")?;
        expected.swap_remove(position);
        if !address_keys_are_valid(destination) {
            return Err("invalid destination address keys".into());
        }
        match by_spend_key.get(&destination.spend_public_key) {
            Some(seen) if !same_address(seen, destination) => {
                return Err(
                    "destinations have duplicate spend keys but not duplicate view keys".into(),
                );
            }
            Some(seen) if seen.is_subaddress != destination.is_subaddress => {
                return Err(
                    "duplicate destinations do not have matching subaddress designations".into(),
                );
            }
            Some(_) => {}
            None => {
                by_spend_key.insert(destination.spend_public_key, destination);
            }
        }
    }
    Ok(())
}

/// Check the transactions of one set against each other (wallet2's
/// `check_consistent_ins_outs`, pending_tx_validation.cpp:87-122).
pub(crate) fn check_construction_set<'a>(
    txes: impl IntoIterator<Item = &'a TxConstructionDataSummary>,
) -> Result<(), String> {
    let mut seen_inputs = HashSet::new();
    let mut destination_kinds: HashMap<([u8; 32], [u8; 32]), bool> = HashMap::new();
    let mut total_input = 0u128;
    for tx in txes {
        for source in &tx.sources {
            let real = usize::try_from(source.real_output)
                .ok()
                .and_then(|index| source.ring.get(index))
                .ok_or("ring sig index out of input set size")?;
            if !seen_inputs.insert(real.output_public_key) {
                return Err("duplicate input pubkey".into());
            }
            total_input += u128::from(source.amount);
        }
        for destination in &tx.split_destinations {
            let kind = destination_kinds
                .entry((destination.spend_public_key, destination.view_public_key))
                .or_insert(destination.is_subaddress);
            if *kind != destination.is_subaddress {
                return Err(
                    "duplicate destinations do not have matching subaddress designations".into(),
                );
            }
        }
    }
    if total_input > u128::from(u64::MAX) {
        return Err("tx set input amount > 2^64 - 1".into());
    }
    Ok(())
}

/// Check an unsigned txset as wallet2's signer does. The change address is not
/// checked: that needs the signer's keys.
pub fn check_unsigned_monero_txset(set: &ParsedUnsignedTxSetSummary) -> Result<(), String> {
    check_construction_set(&set.txes).map_err(|e| format!("check_consistent_ins_outs: {e}"))?;
    for (index, tx) in set.txes.iter().enumerate() {
        check_construction_data(tx).map_err(|e| format!("unsigned transaction #{index}: {e}"))?;
    }
    Ok(())
}

fn check_pending_tx(ptx: &PendingTxSummary, key_images: &[[u8; 32]]) -> Result<(), String> {
    let construction = &ptx.construction;
    let tx = Transaction::<NotPruned>::read(&mut ptx.tx_blob.as_slice())
        .map_err(|e| format!("signed transaction does not parse: {e}"))?;
    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &tx
    else {
        return Err("signed transaction is not a RingCT transaction".into());
    };
    let prefix = tx.prefix();

    if prefix
        .outputs
        .iter()
        .any(|output| output.view_tag.is_none())
    {
        return Err("all outputs should have view tags".into());
    }
    if construction.split_destinations.len() != prefix.outputs.len() {
        return Err("destination vecs are inconsistent".into());
    }
    if prefix.additional_timelock != Timelock::None {
        return Err("unlock_time must be zero".into());
    }
    check_construction_data(construction)?;

    let mut input_key_images = Vec::with_capacity(prefix.inputs.len());
    let mut input_rings = Vec::with_capacity(prefix.inputs.len());
    for input in &prefix.inputs {
        let Input::ToKey {
            amount,
            key_offsets,
            key_image,
        } = input
        else {
            return Err("all inputs are not txin_to_key".into());
        };
        let mut position = 0u64;
        let mut ring = Vec::with_capacity(key_offsets.len());
        for offset in key_offsets {
            position = position
                .checked_add(*offset)
                .ok_or("key offsets overflow")?;
            ring.push(position);
        }
        input_key_images.push(key_image.to_bytes());
        input_rings.push((ring, amount.unwrap_or(0)));
    }
    if ptx.key_images_blob != wallet2_key_images_text(&input_key_images).as_bytes() {
        return Err("failed reconstructing key_images field".into());
    }
    if input_key_images.iter().collect::<HashSet<_>>().len() != input_key_images.len() {
        return Err("duplicate key images".into());
    }
    if construction.sources.len() != prefix.inputs.len() {
        return Err("source/vin size mismatch".into());
    }
    if construction.sources.len() != ptx.selected_transfer_indices.len() {
        return Err("selected_transfers invalid size".into());
    }
    if construction.selected_transfer_indices != ptx.selected_transfer_indices {
        return Err("selected_transfers inconsistent".into());
    }
    let mut unmatched_images: HashSet<&[u8; 32]> = input_key_images.iter().collect();
    for index in &ptx.selected_transfer_indices {
        let image = usize::try_from(*index)
            .ok()
            .and_then(|index| key_images.get(index))
            .ok_or("ptx selected transfer is outside the bounds of imported key images")?;
        if !unmatched_images.remove(image) {
            return Err("ki mismatch between selected transfers and tx".into());
        }
    }

    // Sources are not always in input order, so match rings instead of positions.
    let mut unmatched_sources: Vec<_> = construction.sources.iter().collect();
    for (ring, amount) in &input_rings {
        let position = unmatched_sources
            .iter()
            .position(|source| {
                source.ring.len() == ring.len()
                    && source
                        .ring
                        .iter()
                        .zip(ring)
                        .all(|(member, position)| member.global_output_index == *position)
            })
            .ok_or("key offsets mismatch")?;
        let source = unmatched_sources.swap_remove(position);
        let expected_amount = if source.rct { 0 } else { source.amount };
        if *amount != expected_amount {
            return Err("amount extracted from inputs is wrong".into());
        }
    }

    if ptx.change != construction.change {
        return Err("change_dts inconsistent".into());
    }
    if ptx.destinations != construction.destinations {
        return Err("destination vecs are inconsistent".into());
    }

    if proofs.base.fee != ptx.fee {
        return Err("inconsistent fee for tx.v > 1 transaction".into());
    }
    if ptx.dust_added_to_fee && ptx.dust > ptx.fee {
        return Err("invalid dust amount when dust added to fee".into());
    }
    let input_amount: u128 = construction
        .sources
        .iter()
        .map(|source| u128::from(source.amount))
        .sum();
    let output_amount: u128 = construction
        .split_destinations
        .iter()
        .map(|destination| u128::from(destination.amount))
        .sum::<u128>()
        + u128::from(ptx.fee);
    if output_amount != input_amount {
        return Err("output amount != input amount".into());
    }
    Ok(())
}

/// Check a signed txset as wallet2 does before relay (wallet2.cpp:8266-8284).
/// Outputs are not tied to destinations: the transaction key is withheld.
pub fn check_signed_monero_txset(set: &ParsedSignedTxSetSummary) -> Result<(), String> {
    check_construction_set(set.ptxes.iter().map(|ptx| &ptx.construction))
        .map_err(|e| format!("check_consistent_ins_outs: {e}"))?;
    for (index, ptx) in set.ptxes.iter().enumerate() {
        check_pending_tx(ptx, &set.key_images)
            .map_err(|e| format!("signed transaction #{index}: sanity_check_pending_tx: {e}"))?;
    }
    Ok(())
}
