//! Daemon connection integration tests.
//!
//! All tests in this file require network access to live stagenet nodes.
//! They are marked as #[ignore] by default. Run with --ignored to execute them.

use monero_rust::{rpc::ConnectionConfig, wallet_state::WalletState, Network};
use monero_seed::Seed;
use rand_core::OsRng;
use std::path::PathBuf;
use std::time::Duration;

const STAGENET_NODES: &[&str] = &[
    "https://stagenet.xmr.ditatompel.com:443",
    "http://node2.monerodevs.org:38089",
    "http://stagenet.xmr-tw.org:38081",
    "http://20.168.147.29:18081",
    "http://xmr-lux.boldsuck.org:38081",
    "http://node.monerodevs.org:38089",
];

fn create_test_wallet(name: &str) -> WalletState {
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    WalletState::new(
        seed,
        String::from("English"),
        Network::Stagenet,
        "test_password",
        PathBuf::from(format!("test_wallet_{}.bin", name)),
        0,
    )
    .expect("Failed to create test wallet")
}

async fn try_connect_any_node(wallet: &mut WalletState) -> Result<String, String> {
    let mut last_error = String::new();

    for node in STAGENET_NODES {
        println!("Trying {}...", node);

        let config = ConnectionConfig::new(node.to_string())
            .with_trusted(false)
            .with_timeout(Duration::from_secs(10));

        match wallet.connect(config).await {
            Ok(()) => {
                println!("Connected to {}", node);
                return Ok(node.to_string());
            }
            Err(e) => {
                println!("Failed: {}", e);
                last_error = e.to_string();
            }
        }
    }

    Err(last_error)
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
#[ignore] // Requires network access to live stagenet nodes
async fn test_connect_to_stagenet() {
    let mut wallet = create_test_wallet("connect");

    assert!(!wallet.is_connected_to_daemon());

    let connected_node = try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");

    assert!(wallet.is_connected_to_daemon());
    assert!(wallet.daemon_address.is_some());
    assert!(wallet.daemon_height > 0);

    println!("Node: {}, height: {}", connected_node, wallet.daemon_height);

    wallet.disconnect().await;
    assert!(!wallet.is_connected_to_daemon());
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_connection_health_check() {
    let mut wallet = create_test_wallet("health_check");

    try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");

    let initial_height = wallet.daemon_height;

    wallet.check_connection().await.expect("Health check failed");
    assert!(wallet.daemon_height > 0);

    println!("Height before: {}, after: {}", initial_height, wallet.daemon_height);

    wallet.disconnect().await;

    assert!(wallet.check_connection().await.is_err());
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_reconnect_after_disconnect() {
    let mut wallet = create_test_wallet("reconnect");

    let first = try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");
    println!("First: {}", first);

    wallet.disconnect().await;
    assert!(!wallet.is_connected_to_daemon());

    tokio::time::sleep(Duration::from_secs(1)).await;

    let second = try_connect_any_node(&mut wallet)
        .await
        .expect("Reconnection failed");
    println!("Second: {}", second);

    assert!(wallet.is_connected_to_daemon());
    assert!(wallet.daemon_height > 0);

    wallet.disconnect().await;
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_switch_nodes() {
    let mut wallet = create_test_wallet("switch");

    let first = try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");
    let first_height = wallet.daemon_height;
    println!("First: {} (height {})", first, first_height);

    // Try connecting to a different node
    for node in STAGENET_NODES {
        if *node != first {
            let config = ConnectionConfig::new(node.to_string())
                .with_timeout(Duration::from_secs(10));

            if wallet.connect(config).await.is_ok() {
                println!("Switched to: {} (height {})", node, wallet.daemon_height);
                assert!(wallet.is_connected_to_daemon());
                break;
            }
        }
    }

    wallet.disconnect().await;
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_connection_timeout() {
    let mut wallet = create_test_wallet("timeout");

    let config = ConnectionConfig::new("http://invalid.node.local:18081".to_string())
        .with_timeout(Duration::from_secs(2));

    let result = wallet.connect(config).await;

    assert!(result.is_err());
    assert!(!wallet.is_connected_to_daemon());
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_is_synced() {
    let mut wallet = create_test_wallet("synced");

    try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");

    // Fresh wallet is not synced
    assert!(!wallet.is_synced());

    // Simulate being fully scanned
    wallet.current_scanned_height = wallet.daemon_height;
    assert!(wallet.is_synced());

    wallet.disconnect().await;
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_multiple_wallets() {
    let mut wallets = vec![
        create_test_wallet("multi_1"),
        create_test_wallet("multi_2"),
        create_test_wallet("multi_3"),
    ];

    let mut connected = 0;

    for (i, wallet) in wallets.iter_mut().enumerate() {
        let node = STAGENET_NODES[i % STAGENET_NODES.len()];
        let config = ConnectionConfig::new(node.to_string())
            .with_timeout(Duration::from_secs(10));

        if wallet.connect(config).await.is_ok() {
            println!("Wallet {} connected to {}", i, node);
            connected += 1;
        }
    }

    assert!(connected > 0, "At least one wallet should connect");

    for wallet in &mut wallets {
        if wallet.is_connected_to_daemon() {
            wallet.disconnect().await;
        }
    }
}

#[tokio::test]
#[ignore] // Requires network access to live stagenet nodes
async fn test_daemon_height_updates() {
    let mut wallet = create_test_wallet("height");

    try_connect_any_node(&mut wallet)
        .await
        .expect("All stagenet nodes unreachable");

    let height1 = wallet.daemon_height;
    assert!(height1 > 0);

    tokio::time::sleep(Duration::from_secs(3)).await;

    wallet.check_connection().await.expect("Health check failed");
    let height2 = wallet.daemon_height;

    println!("Height: {} -> {}", height1, height2);

    // Height shouldn't go backwards
    assert!(height2 >= height1);

    wallet.disconnect().await;
}
