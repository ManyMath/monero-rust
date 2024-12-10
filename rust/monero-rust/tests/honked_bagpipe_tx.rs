//! Transaction building tests using stagenet data.

use std::collections::HashSet;
use std::io::Cursor;
use zeroize::Zeroizing;
use monero_serai::{
    Protocol,
    transaction::{Transaction, TransactionPrefix},
    ringct::{RctBase, RctSignatures, RctPrunable},
    wallet::{
        seed::Seed,
        address::{Network, AddressSpec},
        ViewPair, Scanner, SpendableOutput,
        Fee, Change, SignableTransactionBuilder,
    },
};
use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar, edwards::EdwardsPoint};

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;
const TEST_OUTPUT_GLOBAL_INDEX: u64 = 6693930;

fn spend_key_from_seed(seed: &Seed) -> EdwardsPoint {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> Scalar {
    use monero_serai::hash_to_scalar;
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

fn parse_pruned_transaction<R: std::io::Read>(r: &mut R, _prunable_hash: [u8; 32]) -> std::io::Result<Transaction> {
    let prefix = TransactionPrefix::read(r)?;
    if prefix.version != 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid version"));
    }
    let (rct_base, _rct_type) = RctBase::read(prefix.outputs.len(), r)?;
    Ok(Transaction {
        prefix,
        signatures: vec![],
        rct_signatures: RctSignatures { base: rct_base, prunable: RctPrunable::Null },
    })
}

#[tokio::test]
async fn test_transaction_building() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let address = pair.address(Network::Stagenet, AddressSpec::Standard);

    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let tx_hex = "0200020200108baef301e2c69a01a8fb0289ad07cb11c503599e05de083acf0137e201a601039c013cf142d01c72522aeaa9118a3415f7aaf5a70564ab84451c65609988acc8f15702001088bd8d03b3bd049f6a84da03e17dc444880ea831a0023b8c01f20212321802002bcb6380fcc2c7e31d3f65039347338eebe022da1e7eedb16f49fa867cc025020003e2a6399605c1f69d6035b4cb7a4b6f42d19c991404436303acc5b799aa5af7ef560003fae8f70fc43bebe819e07e8381370372533a53e1576f3f59720be14b2078a3c0f52c016509e5ac8d7c85d828e890bfcd1b7fe31c1b4ca8cb63c667af53c44634d5c10f020901d907907175d1d5c206b0e0975b72b4160691ec9a6acb07fe38be143d3bf545e0f100a7087720c41a21e83315e712dcd826e1a6520e5ec4ed290990f00cdcf95707df73ddcfe15a24350cb53055e0c687ef7f699e97bf5979d4147aa5c1";
    let prunable_hash_hex = "18ecf92af45242c451790c0c7084790d48300dc6474b3ef2bea2dd752b47feac";

    let tx_bytes = hex::decode(tx_hex).expect("valid hex");
    let prunable_hash = hex::decode(prunable_hash_hex).expect("valid hex");
    let mut prunable_hash_array = [0u8; 32];
    prunable_hash_array.copy_from_slice(&prunable_hash);

    let mut cursor = Cursor::new(&tx_bytes);
    let transaction = parse_pruned_transaction(&mut cursor, prunable_hash_array)
        .expect("parse failed");

    let mut scanner = Scanner::from_view(pair.clone(), Some(HashSet::new()));
    let scan_result = scanner.scan_transaction(&transaction);
    let outputs = scan_result.ignore_timelock();

    assert_eq!(outputs.len(), 1);
    let received_output = &outputs[0];
    assert_eq!(received_output.data.commitment.amount, EXPECTED_AMOUNT);

    let spendable = SpendableOutput::test_new(received_output.clone(), TEST_OUTPUT_GLOBAL_INDEX);

    let protocol = Protocol::v16;
    let fee = Fee { per_weight: 6000, mask: 10000 };
    let change = Change::new(&pair, false);
    let send_amount = 1_000_000_000_000u64;

    let mut builder = SignableTransactionBuilder::new(protocol, fee, Some(change));
    builder.add_input(spendable);
    builder.add_payment(address, send_amount);

    let signable_tx = builder.build().expect("build failed");

    let total_output = send_amount + (EXPECTED_AMOUNT - send_amount - signable_tx.fee()) + signable_tx.fee();
    assert_eq!(total_output, EXPECTED_AMOUNT);
}

#[test]
fn test_spend_key_derivation() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend_point = spend_key_from_seed(&seed);
    let spend_scalar = spend_key_scalar_from_seed(&seed);

    let derived_point = &spend_scalar * &ED25519_BASEPOINT_TABLE;
    assert_eq!(derived_point, spend_point);
}
