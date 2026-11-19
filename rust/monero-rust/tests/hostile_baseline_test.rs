#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use hostile::epee::{self, Val};
use hostile::{HostileNode, SnapshotNode, Wallet, NETWORK, NETWORK_STR, SEED_W, SEED_W2};
use monero_rust::monero_rpc::RpcConnection;
use monero_rust::native::{
    broadcast_transaction_with_rpc, create_transaction_with_rpc, StoredOutputData,
};
use monero_rust::scanner::derive_address;

#[tokio::test]
async fn snapshot_distribution_matches_the_recorded_daemon_reply() {
    let node = SnapshotNode::new();
    let ours = epee::decode(
        &node
            .post("get_output_distribution.bin", vec![])
            .await
            .unwrap(),
    )
    .unwrap();
    let recorded = epee::decode(&hex::decode(&node.snapshot.distribution_raw).unwrap()).unwrap();
    assert_eq!(ours, recorded);
}

#[tokio::test]
async fn snapshot_get_outs_matches_the_recorded_daemon_reply() {
    let node = SnapshotNode::new();
    let request = epee::encode(&vec![
        (
            "outputs".into(),
            Val::Arr(
                12,
                (0..8u64)
                    .map(|index| {
                        Val::Obj(vec![
                            ("amount".into(), Val::U64(0)),
                            ("index".into(), Val::U64(index)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("get_txid".into(), Val::Bool(false)),
    ]);
    let ours = epee::decode(&node.post("get_outs.bin", request).await.unwrap()).unwrap();
    let recorded = epee::decode(&hex::decode(&node.snapshot.get_outs_sample).unwrap()).unwrap();
    assert_eq!(ours, recorded);
}

#[tokio::test]
async fn honest_refresh_finds_the_recorded_wallet() {
    let node = HostileNode::honest(SnapshotNode::new());
    let mut wallet = Wallet::new(SEED_W);
    wallet
        .refresh(&node.rpc(), 64)
        .await
        .expect("honest refresh");
    assert_eq!(wallet.state.current_height, 849);
    assert_eq!(wallet.outputs().len(), 851);
    assert_eq!(wallet.outputs().iter().filter(|o| o.spent).count(), 4);
    assert_eq!(
        wallet.outputs().iter().filter(|o| !o.is_coinbase).count(),
        4
    );
    assert!(wallet.outputs().iter().all(|o| o.global_index.is_some()));
    for output in wallet.outputs() {
        let recorded = &node.inner.snapshot.outs[output.global_index.unwrap() as usize];
        assert_eq!(recorded.key, output.key);
    }
    println!("refresh requests: {:?}", node.names());
}

#[tokio::test]
async fn honest_send_builds_and_relays() {
    let mut wallet = hostile::synced_wallet().await;
    let node = HostileNode::honest(SnapshotNode::new());
    let input = wallet
        .unspent()
        .into_iter()
        .find(|o| !o.is_coinbase)
        .map(StoredOutputData::from)
        .unwrap();
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let tx = create_transaction_with_rpc(
        &node.rpc(),
        SEED_W,
        NETWORK_STR,
        vec![input],
        &[(w2, 100_000_000_000)],
        "",
    )
    .await
    .expect("honest send");
    broadcast_transaction_with_rpc(&node.rpc(), &tx, &mut wallet.state, false)
        .await
        .expect("honest relay");
    assert_eq!(node.inner.relayed.lock().unwrap().len(), 1);
    println!("send requests: {:?}", node.names());
}

/// Needs a regtest daemon serving the recorded main chain:
///
///     MONERO_REGTEST_RPC=http://127.0.0.1:PORT cargo test -p monero-rust \
///         --test hostile_baseline_test live_ -- --ignored --nocapture
#[tokio::test]
#[ignore = "needs MONERO_REGTEST_RPC serving the recorded chain"]
async fn live_history_negotiation_matches_the_snapshot_node() {
    use monero_rust::scanner::scan_blocks_batch_with_history;
    let url = std::env::var("MONERO_REGTEST_RPC").expect("MONERO_REGTEST_RPC");
    let live = HostileNode::honest(hostile::LiveNode::new(&url));
    let snapshot = hostile::snapshot();
    let genesis = snapshot.chain[0].hash.clone();
    let known = |h: usize| (h as u64, snapshot.chain[h].hash.clone());

    let wrong_last_id = scan_blocks_batch_with_history(
        &live.rpc(),
        0,
        &[known(800), (0, "11".repeat(32))],
        SEED_W,
        NETWORK,
        hostile::LOOKAHEAD,
        false,
        "",
    )
    .await;
    assert!(wrong_last_id.is_err());

    let with_genesis = scan_blocks_batch_with_history(
        &live.rpc(),
        0,
        &[known(800), (0, genesis)],
        SEED_W,
        NETWORK,
        hostile::LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("history ending in the real genesis hash");
    assert_eq!(with_genesis.1, 800);

    let without_genesis = scan_blocks_batch_with_history(
        &live.rpc(),
        0,
        &[known(800)],
        SEED_W,
        NETWORK,
        hostile::LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("history without a genesis entry");
    assert_eq!(without_genesis.1, 800);
}
