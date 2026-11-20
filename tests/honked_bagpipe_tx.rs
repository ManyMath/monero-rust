//! Transaction building tests using stagenet data.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_oxide::{
    ringct::{bulletproofs::Bulletproof, RctBase, RctProofs, RctPrunable},
    transaction::{Transaction, TransactionPrefix},
};
use monero_rust::seed_compat::Seed;
use monero_rust::unsigned_tx::{
    output_with_decoys, sign_offline, UnsignedChange, UnsignedTransaction,
};
use monero_wallet::address::Network;
use monero_wallet::ed25519::{Point, Scalar as OxideScalar};
use monero_wallet::{Scanner, ViewPair};
use std::io::Cursor;
use zeroize::Zeroizing;

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;
const TEST_OUTPUT_GLOBAL_INDEX: u64 = 6693930;

fn spend_key_from_seed(seed: &Seed) -> EdwardsPoint {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> Scalar {
    use monero_rust::crypto::hash_to_scalar;
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    hash_to_scalar(&spend_bytes)
}

fn spend_key_scalar_from_seed(seed: &Seed) -> Scalar {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    Scalar::from_bytes_mod_order(spend_bytes)
}

fn parse_pruned_transaction<R: std::io::Read>(
    r: &mut R,
    _prunable_hash: [u8; 32],
) -> std::io::Result<Transaction> {
    let version: u64 = monero_oxide::io::VarInt::read(r)?;
    let prefix = TransactionPrefix::read(r, version)?;
    if version != 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid version",
        ));
    }
    let Some((_rct_type, rct_base)) = RctBase::read(prefix.inputs.len(), prefix.outputs.len(), r)?
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "pruned RCT transaction had no RCT base",
        ));
    };
    // RctPrunable::Null no longer exists in the oxide backend, so stand in a
    // placeholder prunable. It is discarded (only the RCT base is kept) when
    // the transaction is converted to its pruned form for scanning.
    Ok(Transaction::V2 {
        prefix,
        proofs: Some(RctProofs {
            base: rct_base,
            prunable: placeholder_bp_plus_prunable(),
        }),
    })
}

/// A structurally-valid, all-zero Bulletproof+ prunable placeholder, filling
/// the role of the removed `RctPrunable::Null`.
fn placeholder_bp_plus_prunable() -> RctPrunable {
    // Three compressed points, three scalars, and two empty point vectors.
    let zero_bp_plus = [0u8; (6 * 32) + 2];
    RctPrunable::Clsag {
        clsags: vec![],
        pseudo_outs: vec![],
        bulletproof: Bulletproof::read_plus(&mut zero_bp_plus.as_slice())
            .expect("all-zero Bulletproof+ placeholder should parse"),
    }
}

#[tokio::test]
async fn test_transaction_building() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);

    let pair = ViewPair::new(
        Point::from(spend_point),
        Zeroizing::new(OxideScalar::from(view_scalar)),
    )
    .expect("view pair");
    let address = pair.legacy_address(Network::Stagenet);

    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let tx_hex = "0200020200108baef301e2c69a01a8fb0289ad07cb11c503599e05de083acf0137e201a601039c013cf142d01c72522aeaa9118a3415f7aaf5a70564ab84451c65609988acc8f15702001088bd8d03b3bd049f6a84da03e17dc444880ea831a0023b8c01f20212321802002bcb6380fcc2c7e31d3f65039347338eebe022da1e7eedb16f49fa867cc025020003e2a6399605c1f69d6035b4cb7a4b6f42d19c991404436303acc5b799aa5af7ef560003fae8f70fc43bebe819e07e8381370372533a53e1576f3f59720be14b2078a3c0f52c016509e5ac8d7c85d828e890bfcd1b7fe31c1b4ca8cb63c667af53c44634d5c10f020901d907907175d1d5c206b0e0975b72b4160691ec9a6acb07fe38be143d3bf545e0f100a7087720c41a21e83315e712dcd826e1a6520e5ec4ed290990f00cdcf95707df73ddcfe15a24350cb53055e0c687ef7f699e97bf5979d4147aa5c1";
    let prunable_hash_hex = "18ecf92af45242c451790c0c7084790d48300dc6474b3ef2bea2dd752b47feac";

    let tx_bytes = hex::decode(tx_hex).expect("valid hex");
    let prunable_hash = hex::decode(prunable_hash_hex).expect("valid hex");
    let mut prunable_hash_array = [0u8; 32];
    prunable_hash_array.copy_from_slice(&prunable_hash);

    let mut cursor = Cursor::new(&tx_bytes);
    let transaction =
        parse_pruned_transaction(&mut cursor, prunable_hash_array).expect("parse failed");

    let mut scanner = Scanner::new(pair.clone());
    let outputs = monero_rust::scanner::scan_single_transaction(
        &mut scanner,
        transaction.hash(),
        &Transaction::<monero_oxide::transaction::Pruned>::from(transaction),
    )
    .expect("scan failed");

    assert_eq!(outputs.len(), 1);
    let received_output = &outputs[0];
    assert_eq!(received_output.commitment().amount, EXPECTED_AMOUNT);

    let key = received_output.key();
    let commitment = received_output.commitment().clone();
    let mut ring = vec![[key, commitment.commit()]];
    for i in 1..16u64 {
        let point = Point::from(&Scalar::from(9000 + i) * ED25519_BASEPOINT_TABLE);
        ring.push([point, point]);
    }
    let mut offsets = vec![TEST_OUTPUT_GLOBAL_INDEX];
    offsets.extend(std::iter::repeat(7).take(15));
    let decoys = monero_wallet::ringct::clsag::Decoys::new(offsets, 0, ring).expect("decoys");
    let input =
        output_with_decoys(key, received_output.key_offset(), &commitment, &decoys).expect("input");

    let send_amount = 1_000_000_000_000u64;
    let unsigned = UnsignedTransaction {
        rct_type: monero_oxide::ringct::RctType::ClsagBulletproofPlus,
        seed: Zeroizing::new([7; 32]),
        fee_rate: monero_wallet::interface::FeeRate::new(6000, 10000).expect("fee rate"),
        source_account: 0,
        inputs: vec![input],
        payments: vec![(address, send_amount)],
        change: Some(UnsignedChange::Own {
            address,
            subaddress: None,
        }),
    };

    let resolved = unsigned.resolve(&pair).expect("resolve failed");
    assert!(resolved.fee > 0);
    assert_eq!(resolved.fee % 10000, 0, "fee is quantized by the mask");
    let change = resolved.change.expect("change");
    assert!(change.is_ours);
    assert_eq!(send_amount + change.amount + resolved.fee, EXPECTED_AMOUNT);

    let signed = sign_offline(
        &mut rand::rngs::OsRng,
        &Zeroizing::new(spend_key_scalar_from_seed(&seed)),
        &Zeroizing::new(view_scalar),
        &unsigned,
    )
    .expect("sign failed");
    assert_eq!(signed.fee, resolved.fee);
    assert_eq!(signed.tx.prefix().outputs.len(), 2);
}

#[test]
fn test_spend_key_derivation() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend_point = spend_key_from_seed(&seed);
    let spend_scalar = spend_key_scalar_from_seed(&seed);

    let derived_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    assert_eq!(derived_point, spend_point);
}
