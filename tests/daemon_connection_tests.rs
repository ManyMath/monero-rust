//! Integration tests for daemon connection management.
//!
//! These tests verify that the wallet can successfully connect to Monero
//! stagenet nodes, handle connection failures gracefully, and perform
//! automatic reconnection when necessary.
//!
//! ## Stagenet Nodes
//!
//! Tests use the following stagenet nodes:
//! - stagenet.xmr.ditatompel.com:443
//! - node2.monerodevs.org:38089
//! - stagenet.xmr-tw.org:38081
//! - 20.168.147.29:18081
//! - xmr-lux.boldsuck.org:38081
//! - node.monerodevs.org:38089
//!
//! Tests are designed to be resilient: connection tests only fail if
//! ALL stagenet nodes are unreachable, not just individual nodes.

use monero_rust::{ConnectionConfig, Network, WalletState};
use monero_seed::Seed;
use rand_core::OsRng;
use std::path::PathBuf;
use std::time::Duration;

/// List of stagenet node addresses for testing.
///
/// Tests will try each node in sequence and succeed if ANY node is reachable.
const STAGENET_NODES: &[&str] = &[
    "https://stagenet.xmr.ditatompel.com:443",
    "http://node2.monerodevs.org:38089",
    "http://stagenet.xmr-tw.org:38081",
    "http://20.168.147.29:18081",
    "http://xmr-lux.boldsuck.org:38081",
    "http://node.monerodevs.org:38089",
];

/// Helper function to create a test wallet.
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

/// Attempts to connect to any available stagenet node.
///
/// Tries each node in sequence and returns the first successful connection.
/// If all nodes fail, returns the last error encountered.
async fn try_connect_any_node(
    wallet: &mut WalletState,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut last_error = None;

    for node in STAGENET_NODES {
        println!("Attempting to connect to {}...", node);

        let config = ConnectionConfig::new(node.to_string())
            .with_trusted(false)
            .with_timeout(Duration::from_secs(10));

        match wallet.connect(config).await {
            Ok(()) => {
                println!("✓ Successfully connected to {}", node);
                return Ok(node.to_string());
            }
            Err(e) => {
                println!("✗ Failed to connect to {}: {}", node, e);
                last_error = Some(e);
            }
        }
    }

    Err(Box::new(
        last_error.unwrap_or_else(|| "No nodes available".to_string().into()),
    ))
}

#[tokio::test]
async fn test_connect_to_stagenet() {
    let mut wallet = create_test_wallet("connect");

    // Verify wallet starts disconnected
    assert!(
        !wallet.is_connected_to_daemon(),
        "Wallet should start disconnected"
    );

    // Try to connect to any available node
    let connected_node = try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node. All nodes may be down.");

    // Verify connection succeeded
    assert!(
        wallet.is_connected_to_daemon(),
        "Wallet should be connected after successful connect()"
    );
    assert!(
        wallet.daemon_address.is_some(),
        "Daemon address should be set"
    );
    assert!(
        wallet.daemon_height > 0,
        "Daemon height should be greater than 0"
    );

    println!(
        "Connected to {} with daemon height: {}",
        connected_node, wallet.daemon_height
    );

    // Disconnect
    wallet.disconnect().await;

    assert!(
        !wallet.is_connected_to_daemon(),
        "Wallet should be disconnected after disconnect()"
    );
}

#[tokio::test]
async fn test_connection_health_check() {
    let mut wallet = create_test_wallet("health_check");

    // Connect to any available node
    try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node");

    let initial_height = wallet.daemon_height;

    // Perform manual health check
    wallet
        .check_connection()
        .await
        .expect("Health check should succeed for connected wallet");

    // Daemon height should be updated (or at least still valid)
    assert!(
        wallet.daemon_height > 0,
        "Daemon height should remain valid after health check"
    );

    println!(
        "Health check passed. Initial height: {}, Current height: {}",
        initial_height, wallet.daemon_height
    );

    // Disconnect
    wallet.disconnect().await;

    // Health check should fail when disconnected
    let result = wallet.check_connection().await;
    assert!(
        result.is_err(),
        "Health check should fail when disconnected"
    );
}

#[tokio::test]
async fn test_reconnect_after_disconnect() {
    let mut wallet = create_test_wallet("reconnect");

    // First connection
    let first_node = try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node");

    println!("First connection to: {}", first_node);

    // Disconnect
    wallet.disconnect().await;
    assert!(!wallet.is_connected_to_daemon());

    // Wait a moment
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Reconnect
    let second_node = try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to reconnect to any stagenet node");

    println!("Second connection to: {}", second_node);

    assert!(wallet.is_connected_to_daemon());
    assert!(wallet.daemon_height > 0);

    // Cleanup
    wallet.disconnect().await;
}

#[tokio::test]
async fn test_multiple_connections() {
    let mut wallet = create_test_wallet("multiple");

    // Connect to first available node
    let first_node = try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node");

    let first_height = wallet.daemon_height;

    println!("First node: {}, height: {}", first_node, first_height);

    // Try to connect to a different node (if available)
    // The connect() method should automatically disconnect the first connection
    if STAGENET_NODES.len() > 1 {
        for node in STAGENET_NODES {
            if node != &first_node {
                let config = ConnectionConfig::new(node.to_string())
                    .with_timeout(Duration::from_secs(10));

                if let Ok(()) = wallet.connect(config).await {
                    println!("Second node: {}, height: {}", node, wallet.daemon_height);
                    assert!(wallet.is_connected_to_daemon());
                    break;
                }
            }
        }
    }

    // Cleanup
    wallet.disconnect().await;
}

#[tokio::test]
async fn test_connection_timeout() {
    let mut wallet = create_test_wallet("timeout");

    // Try to connect to an invalid address with a short timeout
    let config = ConnectionConfig::new("http://invalid.node.local:18081".to_string())
        .with_timeout(Duration::from_secs(2));

    let result = wallet.connect(config).await;

    // Should fail
    assert!(
        result.is_err(),
        "Connection to invalid node should fail"
    );
    assert!(
        !wallet.is_connected_to_daemon(),
        "Wallet should not be connected after failed connection attempt"
    );

    println!("Expected connection failure: {:?}", result);
}

#[tokio::test]
async fn test_is_synced() {
    let mut wallet = create_test_wallet("synced");

    // Connect to any available node
    try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node");

    // Wallet should not be synced initially (current_scanned_height is 0)
    assert!(
        !wallet.is_synced(),
        "Wallet should not be synced with height 0"
    );

    // Simulate scanning by setting current_scanned_height to daemon_height
    wallet.current_scanned_height = wallet.daemon_height;

    // Now it should be considered synced
    assert!(
        wallet.is_synced(),
        "Wallet should be synced when current_scanned_height equals daemon_height"
    );

    // Cleanup
    wallet.disconnect().await;
}

#[tokio::test]
async fn test_concurrent_connections() {
    // Create multiple wallets and try to connect them concurrently
    let mut wallets = vec![
        create_test_wallet("concurrent_1"),
        create_test_wallet("concurrent_2"),
        create_test_wallet("concurrent_3"),
    ];

    let mut handles = vec![];

    for (i, wallet) in wallets.iter_mut().enumerate() {
        // Each wallet tries a different node (cycling through the list)
        let node = STAGENET_NODES[i % STAGENET_NODES.len()];
        let config = ConnectionConfig::new(node.to_string()).with_timeout(Duration::from_secs(10));

        // Note: We can't actually spawn concurrent tasks that mutate wallet
        // because wallet needs &mut self. This test verifies that connections
        // don't interfere with each other when done sequentially.

        let result = wallet.connect(config).await;

        if let Ok(()) = result {
            println!("Wallet {} connected to {}", i, node);
            handles.push(i);
        } else {
            println!("Wallet {} failed to connect to {}: {:?}", i, node, result);
        }
    }

    // At least one wallet should have connected successfully
    assert!(
        !handles.is_empty(),
        "At least one wallet should connect successfully"
    );

    // Cleanup
    for wallet in &mut wallets {
        if wallet.is_connected_to_daemon() {
            wallet.disconnect().await;
        }
    }
}

#[tokio::test]
async fn test_daemon_height_query() {
    let mut wallet = create_test_wallet("height_query");

    // Connect to any available node
    try_connect_any_node(&mut wallet)
        .await
        .expect("Failed to connect to any stagenet node");

    let height1 = wallet.daemon_height;
    assert!(height1 > 0, "Initial daemon height should be > 0");

    // Wait a few seconds and check again
    tokio::time::sleep(Duration::from_secs(3)).await;

    wallet
        .check_connection()
        .await
        .expect("Connection check should succeed");

    let height2 = wallet.daemon_height;

    println!("Height before: {}, Height after: {}", height1, height2);

    // Height should be >= initial height (may have new blocks)
    assert!(
        height2 >= height1,
        "Daemon height should not decrease (unless reorg, but unlikely in 3 seconds)"
    );

    // Cleanup
    wallet.disconnect().await;
}

// NOTE: Testing automatic reconnection from background health check task
// is difficult in a unit test because it requires simulating a connection
// failure and waiting for the background task to detect it. This would
// make tests slow and flaky. The reconnection logic itself is tested
// indirectly through the manual reconnection test above.

// TODO: Add integration test for proxy connections when proxy support is implemented.
// This test should verify:
// - Connection through HTTP proxy
// - Connection through SOCKS5 proxy
// - Proxy authentication
// - Proxy connection timeout handling
