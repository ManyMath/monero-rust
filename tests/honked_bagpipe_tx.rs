//! Transaction building tests with mocked RPC responses.

mod test_helpers;

use test_helpers::test_vector_path;
use monero_seed::{Language, Seed};
use monero_wallet::{
    address::{Network, MoneroAddress},
    ringct::RctType,
    rpc::FeeRate,
    send::{Change, SignableTransaction},
    OutputWithDecoys,
};
use zeroize::Zeroizing;

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const BLOCK_HEIGHT: u64 = 1384526;
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

#[derive(serde::Deserialize)]
struct TxBuildingVectors {
    #[serde(with = "hex_serde_single")]
    fee_rate: Vec<u8>,
    #[allow(dead_code)]
    daemon_height: u64,
    #[serde(with = "hex_serde")]
    outputs_with_decoys: Vec<Vec<u8>>,
}

mod hex_serde_single {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

mod hex_serde {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_strings: Vec<String> = Vec::deserialize(deserializer)?;
        hex_strings
            .iter()
            .map(|s| hex::decode(s).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[tokio::test]
async fn test_build_transaction_deterministic() {
    use curve25519_dalek::scalar::Scalar;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    let vectors_path = test_vector_path("honked_bagpipe_tx_vectors.json");
    assert!(
        vectors_path.exists(),
        "Transaction vectors not found. Run: cargo run --example record_tx_building_vectors -- 127.0.0.1:38081 tests/vectors/honked_bagpipe_tx_vectors.json"
    );

    let vectors_json = std::fs::read_to_string(&vectors_path).expect("read vectors");
    let vectors: TxBuildingVectors = serde_json::from_str(&vectors_json).expect("parse vectors");

    assert_eq!(vectors.outputs_with_decoys.len(), 1);
    let output_with_decoys = OutputWithDecoys::read(&mut &vectors.outputs_with_decoys[0][..])
        .expect("deserialize output");
    let fee_rate = FeeRate::read(&mut &vectors.fee_rate[..]).expect("deserialize fee");

    assert_eq!(output_with_decoys.commitment().amount, EXPECTED_AMOUNT);

    let seed = Seed::from_string(
        Language::English,
        Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    ).expect("valid mnemonic");

    let temp_dir = tempfile::TempDir::new().unwrap();
    let wallet = monero_rust::WalletState::new(
        seed.clone(),
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        temp_dir.path().join("deterministic_tx.mw"),
        0,
    ).expect("wallet creation");

    assert_eq!(wallet.get_address(), EXPECTED_ADDRESS);

    let destination = MoneroAddress::from_str(Network::Stagenet, EXPECTED_ADDRESS)
        .expect("valid address");
    let send_amount = 1_000_000_000_000u64;

    let outgoing_view_key = Zeroizing::new([
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ]);

    let change_addr = MoneroAddress::new(
        Network::Stagenet,
        monero_wallet::address::AddressType::Legacy,
        wallet.view_pair.spend(),
        wallet.view_pair.view()
    );
    let change = Change::fingerprintable(Some(change_addr));

    let signable = SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key,
        vec![output_with_decoys],
        vec![(destination, send_amount)],
        change,
        vec![],
        fee_rate,
    ).expect("create signable");

    let spend_bytes: [u8; 32] = *seed.entropy();
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_bytes));
    let mut deterministic_rng = ChaCha20Rng::from_seed([42u8; 32]);

    let signed_tx = signable.sign(&mut deterministic_rng, &spend_key).expect("signing");

    let tx_hash = signed_tx.hash();

    match &signed_tx {
        monero_wallet::transaction::Transaction::V2 { proofs: Some(proofs), .. } => {
            let fee = proofs.base.fee;
            assert!(fee > 0);
            assert!(fee < send_amount);
        }
        _ => panic!("Invalid transaction format"),
    }

    let output_with_decoys2 = OutputWithDecoys::read(&mut &vectors.outputs_with_decoys[0][..])
        .expect("deserialize output 2");
    let fee_rate2 = FeeRate::read(&mut &vectors.fee_rate[..]).expect("deserialize fee 2");

    let outgoing_view_key2 = Zeroizing::new([
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ]);

    let signable2 = SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key2,
        vec![output_with_decoys2],
        vec![(destination, send_amount)],
        Change::fingerprintable(Some(change_addr)),
        vec![],
        fee_rate2,
    ).expect("create signable 2");

    let mut deterministic_rng2 = ChaCha20Rng::from_seed([42u8; 32]);
    let signed_tx2 = signable2.sign(&mut deterministic_rng2, &spend_key).expect("signing 2");

    assert_eq!(tx_hash, signed_tx2.hash());
}

#[tokio::test]
#[ignore]
async fn test_build_transaction_live() {
    use monero_simple_request_rpc::SimpleRequestRpc;
    use monero_wallet::rpc::Rpc;
    use std::time::Duration;
    use rand_core::{OsRng, RngCore};
    use curve25519_dalek::scalar::Scalar;

    let node = std::env::var("STAGENET_NODE")
        .unwrap_or_else(|_| "127.0.0.1:38081".to_string());

    let url = format!("http://{}", node);
    let rpc_client = SimpleRequestRpc::with_custom_timeout(url, Duration::from_secs(60))
        .await
        .expect("connect to stagenet");

    let daemon_height = rpc_client.get_height().await.expect("get height");

    let seed = Seed::from_string(
        Language::English,
        Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    ).expect("valid mnemonic");

    let temp_dir = tempfile::TempDir::new().unwrap();
    let wallet = monero_rust::WalletState::new(
        seed.clone(),
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        temp_dir.path().join("tx_live.mw"),
        BLOCK_HEIGHT.saturating_sub(10),
    ).expect("wallet creation");

    assert_eq!(wallet.get_address(), EXPECTED_ADDRESS);

    let block = rpc_client.get_block_by_number(BLOCK_HEIGHT as usize).await.expect("get block");
    let scannable_block = rpc_client.get_scannable_block(block).await.expect("get scannable");

    let mut scanner = wallet.scanner.clone();
    let scanned = scanner.scan(scannable_block)
        .expect("scan")
        .ignore_additional_timelock();

    assert_eq!(scanned.len(), 1);
    let wallet_output = scanned.into_iter().next().expect("output");
    assert_eq!(wallet_output.commitment().amount, EXPECTED_AMOUNT);

    let output_with_decoys = OutputWithDecoys::fingerprintable_deterministic_new(
        &mut OsRng,
        &rpc_client,
        16,
        daemon_height,
        wallet_output.clone(),
    ).await.expect("decoy selection");

    let destination = MoneroAddress::from_str(Network::Stagenet, EXPECTED_ADDRESS)
        .expect("valid address");
    let send_amount = 1_000_000_000_000u64;

    let mut outgoing_view_key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(outgoing_view_key.as_mut());

    let change_addr = MoneroAddress::new(
        Network::Stagenet,
        monero_wallet::address::AddressType::Legacy,
        wallet.view_pair.spend(),
        wallet.view_pair.view()
    );
    let change = Change::fingerprintable(Some(change_addr));

    let fee_rate = rpc_client.get_fee_rate(monero_wallet::rpc::FeePriority::Normal)
        .await.expect("fee rate");

    let signable = SignableTransaction::new(
        RctType::ClsagBulletproofPlus,
        outgoing_view_key,
        vec![output_with_decoys],
        vec![(destination, send_amount)],
        change,
        vec![],
        fee_rate,
    ).expect("create signable");

    let spend_bytes: [u8; 32] = *seed.entropy();
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_bytes));

    let signed_tx = signable.sign(&mut OsRng, &spend_key).expect("signing");

    match &signed_tx {
        monero_wallet::transaction::Transaction::V2 { proofs: Some(proofs), .. } => {
            assert!(proofs.base.fee > 0);
        }
        _ => panic!("Invalid transaction format"),
    }
}
