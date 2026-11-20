//! Pre-relay checks on a transaction signed offline from this crate's format.
//! The signer is not trusted to have signed what it was asked.

use std::collections::HashSet;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use monero_wallet::{
    address::MoneroAddress,
    ed25519::{Commitment, Point as OxidePoint, Scalar as OxideScalar},
    extra::{Extra, ExtraField, PaymentId},
    io::VarInt,
    ringct::EncryptedAmount,
    transaction::{Input, NotPruned, Timelock, Transaction},
    ViewPair,
};

use crate::tx_builder::native::{ChangeOutputInfo, OfflineSignResult};
use crate::unsigned_tx::UnsignedTransaction;

fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

/// Keccak of the parts in order, without copying them into one buffer: some
/// derive from the view key.
fn keccak_of(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn point(address_key: monero_wallet::ed25519::Point) -> EdwardsPoint {
    address_key.into()
}

fn same_keys(a: &MoneroAddress, b: &MoneroAddress) -> bool {
    a.spend() == b.spend() && a.view() == b.view()
}

struct Derivation {
    view_tag: u8,
    shared_key: Zeroizing<Scalar>,
}

fn derive(ecdh: &EdwardsPoint, index: usize, uniqueness: Option<[u8; 32]>) -> Derivation {
    // Sized so the buffer never grows and leaves a copy of the secret behind.
    let mut material = Zeroizing::new(Vec::with_capacity(32 + 10));
    material.extend_from_slice(Zeroizing::new(ecdh.mul_by_cofactor().compress()).as_bytes());
    VarInt::write(&index, &mut *material).expect("writing to a Vec does not fail");
    let view_tag = keccak_of(&[b"view_tag", &material])[0];
    let shared_key = Zeroizing::new(Scalar::from_bytes_mod_order(match uniqueness {
        Some(uniqueness) => keccak_of(&[&uniqueness, &material]),
        None => keccak_of(&[&material]),
    }));
    Derivation {
        view_tag,
        shared_key,
    }
}

/// The mask wallet2 encrypts a payment ID with for the output whose shared
/// secret is `ecdh` (`encrypt_payment_id`, device_default.cpp).
fn payment_id_xor(ecdh: &EdwardsPoint) -> [u8; 8] {
    let derivation = Zeroizing::new(ecdh.mul_by_cofactor().compress());
    let mask = Zeroizing::new(keccak_of(&[derivation.as_bytes(), &[0x8d]]));
    let mut xor = [0u8; 8];
    xor.copy_from_slice(&mask[..8]);
    xor
}

fn output_matches(
    tx: &SignedParts<'_>,
    index: usize,
    derivation: &Derivation,
    spend: Option<&EdwardsPoint>,
    amount: Option<u64>,
) -> Option<u64> {
    let output = &tx.outputs[index];
    if output.view_tag != Some(derivation.view_tag) {
        return None;
    }
    if let Some(spend) = spend {
        let expected = &*derivation.shared_key * ED25519_BASEPOINT_TABLE + spend;
        if output.key.to_bytes() != expected.compress().to_bytes() {
            return None;
        }
    }
    let EncryptedAmount::Compact { amount: encrypted } = tx.encrypted_amounts[index] else {
        return None;
    };
    let mask_key = Zeroizing::new(keccak_of(&[b"amount", derivation.shared_key.as_bytes()]));
    let mut xor = [0u8; 8];
    xor.copy_from_slice(&mask_key[..8]);
    let decrypted = u64::from_le_bytes(encrypted) ^ u64::from_le_bytes(xor);
    if amount.is_some_and(|amount| amount != decrypted) {
        return None;
    }
    let mask = Zeroizing::new(Scalar::from_bytes_mod_order(keccak_of(&[
        b"commitment_mask",
        derivation.shared_key.as_bytes(),
    ])));
    let commitment = Commitment::new(OxideScalar::from(*mask), decrypted).commit();
    let commitment: EdwardsPoint = commitment.into();
    (tx.commitments[index].to_bytes() == commitment.compress().to_bytes()).then_some(decrypted)
}

struct SignedParts<'a> {
    outputs: &'a [monero_wallet::transaction::Output],
    encrypted_amounts: &'a [EncryptedAmount],
    commitments: &'a [monero_wallet::ed25519::CompressedPoint],
}

fn parse_scalar(hex_key: &str, what: &str) -> Result<Zeroizing<Scalar>, String> {
    let bytes = Zeroizing::new(
        <[u8; 32]>::try_from(
            hex::decode(hex_key)
                .map_err(|_| format!("{what} is not hex"))?
                .as_slice(),
        )
        .map_err(|_| format!("{what} is not 32 bytes"))?,
    );
    Option::<Scalar>::from(Scalar::from_canonical_bytes(*bytes))
        .map(Zeroizing::new)
        .ok_or_else(|| format!("{what} is not a canonical scalar"))
}

/// Check a transaction signed offline against the unsigned transaction it came
/// from: its inputs, fee, outputs under the returned hex keys, and `extra`.
pub fn check_signed_transaction(
    unsigned: &UnsignedTransaction,
    view_secret_key: &[u8; 32],
    public_spend_key: &[u8; 32],
    tx_bytes: &[u8],
    tx_key: &str,
    additional_tx_keys: &[String],
) -> Result<Transaction<NotPruned>, String> {
    let view_secret = Zeroizing::new(Scalar::from_bytes_mod_order(*view_secret_key));
    let spend_public = crate::scanner::decompress_public_spend_key(*public_spend_key)
        .ok_or("invalid public spend key")?;
    let view_pair = ViewPair::new(
        OxidePoint::from(spend_public),
        Zeroizing::new(OxideScalar::from(*view_secret)),
    )
    .map_err(|e| format!("invalid wallet keys: {e}"))?;
    let tx = Transaction::<NotPruned>::read(&mut &tx_bytes[..])
        .map_err(|e| format!("signed transaction does not parse: {e}"))?;
    if tx.serialize() != tx_bytes {
        return Err("signed transaction is not canonically encoded".into());
    }
    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &tx
    else {
        return Err("signed transaction is not a RingCT transaction".into());
    };
    let prefix = tx.prefix();
    if proofs.rct_type() != unsigned.rct_type {
        return Err("signed transaction has another RingCT type than requested".into());
    }
    if prefix.additional_timelock != Timelock::None {
        return Err("signed transaction is time-locked".into());
    }
    if prefix
        .outputs
        .iter()
        .any(|output| output.view_tag.is_none() || output.amount.is_some())
    {
        return Err("signed transaction has an output without a view tag".into());
    }

    if prefix.inputs.len() != unsigned.inputs.len() {
        return Err(format!(
            "signed transaction spends {} inputs, {} were requested",
            prefix.inputs.len(),
            unsigned.inputs.len()
        ));
    }
    let mut unmatched: Vec<_> = unsigned.inputs.iter().collect();
    let mut key_images = HashSet::new();
    for input in &prefix.inputs {
        let Input::ToKey {
            amount: None,
            key_offsets,
            key_image,
        } = input
        else {
            return Err("signed transaction has an input that is not a RingCT key input".into());
        };
        if !key_images.insert(key_image.to_bytes()) {
            return Err("signed transaction spends one key image twice".into());
        }
        let position = unmatched
            .iter()
            .position(|requested| requested.decoys().offsets() == key_offsets.as_slice())
            .ok_or("signed transaction spends a ring that was not requested")?;
        unmatched.swap_remove(position);
    }

    let resolved = unsigned.resolve(&view_pair)?;
    if proofs.base.fee != resolved.fee {
        return Err(format!(
            "signed transaction pays a fee of {}, the unsigned transaction {}",
            proofs.base.fee, resolved.fee
        ));
    }
    let change_address = resolved.change.map(|change| change.address);
    let expected_outputs = unsigned.payments.len()
        + usize::from(resolved.change.is_some())
        + usize::from(resolved.dummy_amount.is_some());
    if prefix.outputs.len() != expected_outputs {
        return Err(format!(
            "signed transaction has {} outputs, {expected_outputs} were requested",
            prefix.outputs.len()
        ));
    }
    if proofs.base.encrypted_amounts.len() != expected_outputs
        || proofs.base.commitments.len() != expected_outputs
    {
        return Err("signed transaction's amounts do not match its outputs".into());
    }

    let extra = Extra::read(&mut prefix.extra.as_slice())
        .map_err(|e| format!("signed transaction extra does not parse: {e}"))?;
    let (main_keys, additional_published) = extra
        .keys()
        .ok_or("signed transaction has no transaction public key")?;
    let [main_published] = main_keys.as_slice() else {
        return Err("signed transaction has more than one transaction public key".into());
    };
    let additional_published = additional_published.unwrap_or_default();
    let main_public = point(*main_published);
    let additional_public: Vec<EdwardsPoint> =
        additional_published.iter().copied().map(point).collect();
    let tx_key = parse_scalar(tx_key, "transaction key")?;
    let additional_keys = additional_tx_keys
        .iter()
        .map(|key| parse_scalar(key, "additional transaction key"))
        .collect::<Result<Vec<_>, _>>()?;

    // wallet2's key layout (cryptonote_tx_utils.cpp:76-100, 413-430): a sole
    // subaddress destination puts the main key on its spend key; a subaddress
    // beside any other destination gets per-output keys.
    let to_change = |address: &MoneroAddress| {
        change_address
            .as_ref()
            .is_some_and(|change| same_keys(change, address))
    };
    let mut destinations: Vec<&MoneroAddress> = Vec::new();
    for (address, _) in &unsigned.payments {
        if !to_change(address) && !destinations.iter().any(|seen| same_keys(seen, address)) {
            destinations.push(address);
        }
    }
    let subaddresses = destinations
        .iter()
        .filter(|address| address.is_subaddress())
        .count();
    let standard = destinations.len() - subaddresses;
    let per_output_keys = subaddresses != 0 && (standard != 0 || subaddresses > 1);
    let generator = |address: &MoneroAddress| {
        if address.is_subaddress() {
            point(address.spend())
        } else {
            curve25519_dalek::constants::ED25519_BASEPOINT_POINT
        }
    };
    let main_generator = match destinations.as_slice() {
        [sole] if standard == 0 => generator(sole),
        _ => curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
    };
    if &*tx_key * main_generator != main_public {
        return Err("the returned transaction keys are not the transaction's".into());
    }
    let expected_additional = if per_output_keys { expected_outputs } else { 0 };
    if additional_keys.len() != additional_public.len()
        || additional_public.len() != expected_additional
    {
        return Err("signed transaction's additional keys do not match its outputs".into());
    }

    let uniqueness = {
        let mut material = b"uniqueness".to_vec();
        for input in &prefix.inputs {
            if let Input::ToKey { key_image, .. } = input {
                material.extend(key_image.to_bytes());
            }
        }
        keccak(&material)
    };
    let own_ecdh = Zeroizing::new(*view_secret * main_public);
    let parts = SignedParts {
        outputs: &prefix.outputs,
        encrypted_amounts: &proofs.base.encrypted_amounts,
        commitments: &proofs.base.commitments,
    };

    enum Paid<'a> {
        Change,
        Dummy,
        Payment(&'a MoneroAddress),
    }
    let mut found: Vec<(Paid<'_>, Zeroizing<EdwardsPoint>)> = Vec::with_capacity(expected_outputs);
    let mut payments_left: Vec<(usize, &(MoneroAddress, u64))> =
        unsigned.payments.iter().enumerate().collect();
    let mut change_found = change_address.is_none();
    let mut dummy_found = resolved.dummy_amount.is_none();
    for index in 0..expected_outputs {
        let own = derive(&own_ecdh, index, None);
        if !change_found {
            if let Some(change) = &change_address {
                let amount = resolved.change.map(|change| change.amount);
                if output_matches(&parts, index, &own, Some(&point(change.spend())), amount)
                    .is_some()
                {
                    change_found = true;
                    found.push((Paid::Change, own_ecdh.clone()));
                    continue;
                }
            }
        }
        if !dummy_found && output_matches(&parts, index, &own, None, Some(0)).is_some() {
            dummy_found = true;
            found.push((Paid::Dummy, own_ecdh.clone()));
            continue;
        }
        let mut matched = None;
        for (position, (_, (address, amount))) in payments_left.iter().enumerate() {
            let (ecdh, publishes) = if to_change(address) {
                (own_ecdh.clone(), None)
            } else {
                let (secret, public) = if address.is_subaddress() && !additional_keys.is_empty() {
                    (&*additional_keys[index], additional_public[index])
                } else {
                    (&*tx_key, main_public)
                };
                (
                    Zeroizing::new(secret * point(address.view())),
                    Some((secret * generator(address), public)),
                )
            };
            if let Some((expected, published)) = publishes {
                if expected != published {
                    continue;
                }
            }
            let derivation = derive(&ecdh, index, address.is_guaranteed().then_some(uniqueness));
            if output_matches(
                &parts,
                index,
                &derivation,
                Some(&point(address.spend())),
                Some(*amount),
            )
            .is_some()
            {
                matched = Some((position, ecdh));
                break;
            }
        }
        match matched {
            Some((position, ecdh)) => {
                let (_, (address, _)) = payments_left.swap_remove(position);
                found.push((Paid::Payment(address), ecdh));
            }
            None => {
                return Err(format!(
                "signed transaction output {index} is not one the unsigned transaction requested"
            ))
            }
        }
    }
    if !payments_left.is_empty() || !change_found || !dummy_found {
        return Err("signed transaction leaves out a requested output".into());
    }

    for (index, (paid, _)) in found.iter().enumerate() {
        let (Some(secret), Some(public)) =
            (additional_keys.get(index), additional_public.get(index))
        else {
            break;
        };
        let generator = match paid {
            Paid::Payment(address) => generator(address),
            Paid::Change => generator(change_address.as_ref().expect("change was found")),
            Paid::Dummy => curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
        };
        if &**secret * generator != *public {
            return Err("the returned transaction keys are not the transaction's".into());
        }
    }

    // The payment ID, real or dummy, is encrypted to the one non-change
    // destination (cryptonote_tx_utils.cpp:208-228); a dummy only with two outputs.
    let mut recipient: Option<(usize, &MoneroAddress)> = None;
    let mut several = false;
    let mut first_to_change = None;
    for (index, (paid, _)) in found.iter().enumerate() {
        let Paid::Payment(address) = paid else {
            continue;
        };
        if to_change(address) {
            first_to_change = first_to_change.or(Some(index));
        } else {
            match recipient {
                None => recipient = Some((index, address)),
                Some((_, first)) if !same_keys(first, address) => several = true,
                Some(_) => {}
            }
        }
    }
    let recipient = if several {
        None
    } else {
        recipient.map(|(index, _)| index).or(first_to_change)
    };
    let integrated = found
        .iter()
        .enumerate()
        .find_map(|(index, (paid, _))| match paid {
            Paid::Payment(address) => address.payment_id().map(|id| (index, id)),
            Paid::Change | Paid::Dummy => None,
        });
    let payment_id = match integrated {
        Some((own_index, id)) => {
            let xor = payment_id_xor(&found[recipient.unwrap_or(own_index)].1);
            Some((u64::from_le_bytes(id) ^ u64::from_le_bytes(xor)).to_le_bytes())
        }
        None if expected_outputs == 2 => recipient.map(|index| payment_id_xor(&found[index].1)),
        None => None,
    };

    // Exactly the keys and the payment ID, in wallet2's order (sort_tx_extra):
    // anything else is room for the signer to write into the chain.
    let mut expected_extra = Vec::with_capacity(prefix.extra.len());
    let mut fields = vec![ExtraField::PublicKey(main_published.compress())];
    if !additional_published.is_empty() {
        fields.push(ExtraField::PublicKeys(
            additional_published
                .iter()
                .map(|key| key.compress())
                .collect(),
        ));
    }
    if let Some(id) = payment_id {
        fields.push(ExtraField::Nonce(PaymentId::Encrypted(id).serialize()));
    }
    for field in &fields {
        field
            .write(&mut expected_extra)
            .expect("writing to a Vec does not fail");
    }
    if prefix.extra != expected_extra {
        return Err(match (integrated, extra.payment_id()) {
            (Some(_), found) if found != payment_id.map(PaymentId::Encrypted) => {
                "signed transaction does not carry the requested payment ID".into()
            }
            _ => "signed transaction's extra holds something other than its keys and payment ID"
                .into(),
        });
    }
    Ok(tx)
}

/// `check_signed_transaction` on an offline signer's result, plus its id, fee, key
/// images and change outputs. A change output's `key_image` is the signer's word.
pub fn check_offline_signed_transaction(
    unsigned_tx_hex: &str,
    signed: &OfflineSignResult,
    view_secret_key: &[u8; 32],
    public_spend_key: &[u8; 32],
) -> Result<(), String> {
    let unsigned_bytes =
        hex::decode(unsigned_tx_hex.trim()).map_err(|_| "unsigned transaction is not hex")?;
    let unsigned = UnsignedTransaction::from_bytes(&unsigned_bytes)?;
    let [transaction] = signed.transactions.as_slice() else {
        return Err(format!(
            "the signer returned {} transactions for one unsigned transaction",
            signed.transactions.len()
        ));
    };
    if signed.tx_blob != transaction.tx_blob
        || signed.tx_id != transaction.tx_id
        || signed.tx_key != transaction.tx_key
        || signed.tx_key_additional != transaction.tx_key_additional
        || signed.fee != transaction.fee
        || signed.spent_key_images != transaction.spent_key_images
        || signed.change_outputs != transaction.change_outputs
    {
        return Err("the sign result describes another transaction than it holds".into());
    }
    let tx_bytes =
        hex::decode(&transaction.tx_blob).map_err(|_| "signed transaction is not hex")?;
    let tx = check_signed_transaction(
        &unsigned,
        view_secret_key,
        public_spend_key,
        &tx_bytes,
        &transaction.tx_key,
        &transaction.tx_key_additional,
    )?;
    if hex::encode(tx.hash()) != transaction.tx_id {
        return Err("signed transaction id is not the transaction's hash".into());
    }
    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &tx
    else {
        unreachable!("checked above");
    };
    if transaction.fee != proofs.base.fee {
        return Err("the sign result states another fee than the transaction pays".into());
    }
    let spent: HashSet<String> = tx
        .prefix()
        .inputs
        .iter()
        .filter_map(|input| match input {
            Input::ToKey { key_image, .. } => Some(hex::encode(key_image.to_bytes())),
            Input::Gen(_) => None,
        })
        .collect();
    let reported: HashSet<String> = transaction.spent_key_images.iter().cloned().collect();
    if reported != spent || transaction.spent_key_images.len() != spent.len() {
        return Err("the sign result reports other key images than the transaction spends".into());
    }

    let view_pair = ViewPair::new(
        OxidePoint::from(
            crate::scanner::decompress_public_spend_key(*public_spend_key)
                .ok_or("invalid public spend key")?,
        ),
        Zeroizing::new(OxideScalar::from(Scalar::from_bytes_mod_order(
            *view_secret_key,
        ))),
    )
    .map_err(|e| format!("invalid wallet keys: {e}"))?;
    let found = crate::tx_builder::native::own_transaction_outputs(
        &tx,
        &transaction.tx_id,
        view_pair,
        crate::tx_builder::native::signer_lookahead(),
        unsigned.change.and_then(|change| change.subaddress()),
    )?;
    let described = transaction.change_outputs.len() == found.len()
        && transaction
            .change_outputs
            .iter()
            .zip(&found)
            .all(|(reported, (found, _))| {
                ChangeOutputInfo {
                    key_image: String::new(),
                    ..reported.clone()
                } == *found
            });
    if !described {
        return Err(
            "the sign result describes other outputs coming back to the wallet than the \
             transaction has"
                .into(),
        );
    }
    Ok(())
}
