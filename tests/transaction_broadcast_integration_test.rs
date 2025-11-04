use monero_rust::{
    WalletState, Network, ConnectionConfig, TransactionConfig, TransactionPriority,
};

const STAGENET_RPC: &str = "http://stagenet.xmr-tw.org:38081";
const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

#[tokio::test]
#[ignore]
async fn test_create_and_broadcast_tx() {
    let mut wallet = WalletState::from_mnemonic(TEST_SEED, None, Network::Stagenet)
        .expect("failed to create wallet");

    let config = ConnectionConfig::new(STAGENET_RPC.to_string());
    wallet.connect(config).await.expect("failed to connect");

    let blocks: Vec<u64> = vec![2032114, 2032323, 2032324, 2032326, 2032338, 2032598];
    wallet.scan_specific_blocks(&blocks).await.expect("scan failed");

    let balance = wallet.get_unlocked_balance();
    if balance == 0 {
        println!("No unlocked balance, skipping broadcast test");
        return;
    }

    let dest = wallet.get_address();
    let amount = 1_000_000; // 0.000001 XMR

    let config = TransactionConfig::default();
    let pending = wallet.create_tx(&dest, amount, config).await
        .expect("failed to create tx");

    assert!(pending.fee() > 0);
    assert!(!pending.selected_inputs.is_empty());

    // We don't actually broadcast in tests
    println!("TX created: {}", hex::encode(pending.txid()));
}

#[tokio::test]
#[ignore]
async fn test_estimate_fee() {
    let mut wallet = WalletState::from_mnemonic(TEST_SEED, None, Network::Stagenet)
        .expect("failed to create wallet");

    let config = ConnectionConfig::new(STAGENET_RPC.to_string());
    wallet.connect(config).await.expect("failed to connect");

    let fee = wallet.estimate_tx_fee(1, TransactionPriority::Default).await
        .expect("failed to estimate fee");

    assert!(fee > 0, "fee should be non-zero");
    println!("Estimated fee for 1 output: {} piconeros", fee);

    let fee_2 = wallet.estimate_tx_fee(2, TransactionPriority::High).await
        .expect("failed to estimate fee");

    assert!(fee_2 >= fee, "more outputs or higher priority should not reduce fee");
}

#[tokio::test]
#[ignore]
async fn test_sweep_all() {
    let mut wallet = WalletState::from_mnemonic(TEST_SEED, None, Network::Stagenet)
        .expect("failed to create wallet");

    let config = ConnectionConfig::new(STAGENET_RPC.to_string());
    wallet.connect(config).await.expect("failed to connect");

    let blocks: Vec<u64> = vec![2032114, 2032323, 2032324, 2032326, 2032338, 2032598];
    wallet.scan_specific_blocks(&blocks).await.expect("scan failed");

    let balance = wallet.get_unlocked_balance();
    if balance == 0 {
        println!("No unlocked balance, skipping sweep test");
        return;
    }

    let dest = wallet.get_address();
    let config = TransactionConfig {
        sweep_all: true,
        ..Default::default()
    };

    let pending = wallet.create_tx(&dest, 0, config).await
        .expect("failed to create sweep tx");

    let all_outputs = wallet.get_outputs(false).expect("get outputs failed");
    let expected_inputs: usize = all_outputs.iter()
        .filter(|o| !o.frozen && o.unlocked)
        .count();

    assert_eq!(pending.num_inputs(), expected_inputs);
    println!("Sweep tx uses {} inputs", pending.num_inputs());
}
