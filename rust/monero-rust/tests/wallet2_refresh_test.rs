#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::collections::{BTreeSet, HashMap};

use hostile::epee::{self, Val};
use hostile::{Chain, HostileNode, SnapshotNode, LOOKAHEAD, NETWORK, SEED_W};
use monero_rust::scanner::{genesis_block_hash, process_fetched_batch_cached, Refresh};
use monero_rust::{
    apply_processed_batch, process_batch_with_reorg_detection, ReorgInfo, ScanBatchOutcome,
    WalletState,
};

type Node = HostileNode<SnapshotNode>;

fn asked(
    request: &hostile::Logged,
    heights: &HashMap<[u8; 32], u64>,
) -> (Vec<Option<u64>>, u64, bool, Option<u64>, Option<u64>) {
    let body = request.epee();
    let ids = epee::get(&body, "block_ids").unwrap().bytes();
    let ids = ids
        .chunks(32)
        .map(|id| heights.get(<&[u8; 32]>::try_from(id).unwrap()).copied())
        .collect();
    let number = |name: &str| {
        epee::get(&body, name).map(|value| match value {
            Val::U8(v) => u64::from(*v),
            other => other.u64(),
        })
    };
    (
        ids,
        number("start_height").unwrap(),
        epee::get(&body, "prune").unwrap().bool(),
        number("requested_info"),
        number("pool_info_since"),
    )
}

async fn refresh(node: &Node, state: &mut WalletState, restore_height: u64) {
    let reorgs = refresh_following_reorgs(node, state, restore_height).await;
    assert!(reorgs.is_empty(), "unexpected reorg {reorgs:?}");
}

async fn refresh_following_reorgs(
    node: &Node,
    state: &mut WalletState,
    restore_height: u64,
) -> Vec<ReorgInfo> {
    let rpc = node.rpc();
    let mut reorgs = Vec::new();
    let mut session = Refresh::new(state, NETWORK, restore_height).unwrap();
    session.fetch_block_ids(&rpc, state).await.unwrap();
    let mut cached = None;
    let mut requests = 0;
    while let Some(batch) = session.next_batch(&rpc).await.unwrap() {
        requests += 1;
        assert!(requests < 20, "refresh does not stop");
        if batch.is_empty() {
            continue;
        }
        let start = batch.start_height().unwrap();
        let daemon_height = batch.daemon_height();
        let (results, scanner) =
            process_fetched_batch_cached(batch, SEED_W, NETWORK, LOOKAHEAD, cached.take(), "")
                .await
                .unwrap();
        cached = Some(scanner);
        match process_batch_with_reorg_detection(&results, state, None, daemon_height, start)
            .unwrap()
        {
            ScanBatchOutcome::Normal(processed) => {
                apply_processed_batch(state, &processed);
            }
            ScanBatchOutcome::Reorg(info) => {
                assert!(reorgs.len() < 3, "reorgs do not stop: {reorgs:?}");
                reorgs.push(info);
                session = Refresh::new(state, NETWORK, restore_height).unwrap();
                session.fetch_block_ids(&rpc, state).await.unwrap();
            }
        }
    }
    assert!(session.is_done());
    for request in node.requests("getblocks.bin") {
        assert_eq!(epee::get(&request.epee(), "start_height").unwrap().u64(), 0);
    }
    reorgs
}

fn outputs(state: &WalletState) -> BTreeSet<(String, u64, u64, bool)> {
    outputs_in(state, 0..u64::MAX)
}

fn outputs_in(
    state: &WalletState,
    heights: std::ops::Range<u64>,
) -> BTreeSet<(String, u64, u64, bool)> {
    state
        .outputs()
        .iter()
        .filter(|o| heights.contains(&o.block_height))
        .map(|o| (o.tx_hash.clone(), o.output_index, o.amount, o.spent))
        .collect()
}

async fn scanned_from(node: &Node, restore_height: u64) -> WalletState {
    let mut state = WalletState::new();
    refresh(node, &mut state, restore_height).await;
    state
}

fn wallet2_list(tip: u64) -> Vec<Option<u64>> {
    let mut expected: Vec<Option<u64>> = (0..11).map(|back| Some(tip - back)).collect();
    let length = tip + 1;
    let mut offset = 13;
    let mut power = 4;
    while offset < length {
        expected.push(Some(length - offset));
        power *= 2;
        offset = 9 + power;
    }
    expected.truncate(expected.len() - 3);
    expected.push(Some(0));
    expected
}

fn id_heights(state: &WalletState) -> HashMap<[u8; 32], u64> {
    let mut heights: HashMap<[u8; 32], u64> = (0..=state.current_height)
        .filter_map(|h| {
            let hash = state.block_hashes.get_hash(h)?;
            Some((hex::decode(hash).unwrap().try_into().unwrap(), h))
        })
        .collect();
    heights.insert(genesis_block_hash(NETWORK), 0);
    heights
}

#[tokio::test]
async fn refresh_from_genesis_asks_like_wallet2_and_ends_where_the_scan_ends() {
    let node = Node::with(SnapshotNode::new(), "short-batches", |call, reply| {
        if !call.is("getblocks.bin") {
            return reply;
        }
        let mut body = epee::decode(&reply?).unwrap();
        for name in ["blocks", "output_indices"] {
            if let Some(Val::Arr(_, items)) = epee::get_mut(&mut body, name) {
                items.truncate(300);
            }
        }
        Ok(epee::encode(&body))
    });
    let mut state = WalletState::new();
    refresh(&node, &mut state, 0).await;

    let synced = hostile::synced_wallet().await;
    assert_eq!(state.current_height, synced.state.current_height);
    assert_eq!(outputs(&state), outputs(&synced.state));

    let heights = id_heights(&state);
    let requests: Vec<_> = node
        .requests("getblocks.bin")
        .iter()
        .map(|request| asked(request, &heights))
        .collect();
    assert!(requests.len() >= 3);
    assert_eq!(requests[0].0, vec![Some(0)]);
    for (ids, start, prune, _, _) in &requests {
        assert_eq!(*start, 0);
        assert!(*prune);
        assert_eq!(ids.last(), Some(&Some(0)));
    }
    for pair in requests.windows(2) {
        let previous_end = pair[1].0[0].unwrap();
        assert_eq!(
            pair[1].0[..3],
            [
                Some(previous_end),
                Some(previous_end - 1),
                Some(previous_end - 2)
            ]
        );
    }
}

#[tokio::test]
async fn refresh_at_the_tip_sends_wallet2s_history_once() {
    let synced = hostile::synced_wallet().await;
    let mut state = WalletState::new();
    state.replace_outputs(synced.state.outputs().to_vec());
    state.block_hashes = synced.state.block_hashes.clone();
    state.current_height = synced.state.current_height;
    let tip = state.current_height;

    let node = Node::honest(SnapshotNode::new());
    let rpc = node.rpc();
    let mut session = Refresh::new(&state, NETWORK, 0).unwrap();
    let batch = session.next_batch(&rpc).await.unwrap().unwrap();
    assert!(batch.is_empty(), "the node has nothing newer");
    assert_eq!(batch.start_height().unwrap(), tip + 1);
    assert!(session.next_batch(&rpc).await.unwrap().is_none());

    let heights = id_heights(&state);
    let requests = node.requests("getblocks.bin");
    assert_eq!(requests.len(), 1);
    let (ids, start, prune, _, _) = asked(&requests[0], &heights);
    assert_eq!(ids, wallet2_list(tip));
    assert_eq!(start, 0);
    assert!(prune);
    let body = requests[0].epee();
    let client = epee::get(&body, "client").unwrap().bytes();
    assert_eq!(
        &client[..64],
        hex::encode(state.rpc_client_key().public_key()).as_bytes()
    );
    assert_ne!(state.rpc_client_key(), rpc.client_key());
}

#[tokio::test]
async fn refresh_of_a_restored_wallet_fetches_the_block_ids_below_it_first() {
    let node = Node::honest(SnapshotNode::new());
    let mut state = WalletState::new();
    refresh(&node, &mut state, 800).await;

    let heights = id_heights(&state);
    let hashes = node.requests("gethashes.bin");
    assert_eq!(hashes.len(), 1);
    let body = hashes[0].epee();
    let listed: Vec<Option<u64>> = epee::get(&body, "block_ids")
        .unwrap()
        .bytes()
        .chunks(32)
        .map(|id| heights.get(<&[u8; 32]>::try_from(id).unwrap()).copied())
        .collect();
    assert_eq!(listed, vec![Some(0)]);
    assert_eq!(epee::get(&body, "start_height").unwrap().u64(), 0);
    assert_eq!(state.block_hashes.earliest_height(), Some(0));

    let requests = node.requests("getblocks.bin");
    let (ids, start, prune, _, _) = asked(&requests[0], &heights);
    assert_eq!((ids, start, prune), (wallet2_list(799), 0, true));

    let synced = hostile::synced_wallet().await;
    assert_eq!(state.current_height, synced.state.current_height);
    assert_eq!(outputs(&state), outputs_in(&synced.state, 800..u64::MAX));
}

#[tokio::test]
async fn a_new_wallet_refreshed_at_every_block_keeps_refreshing() {
    let node = Node::honest(SnapshotNode::new());
    let synced = hostile::synced_wallet().await;
    let first = 820;
    let mut state = WalletState::new();
    for height in first..first + 8 {
        node.inner.set_height(height);
        refresh(&node, &mut state, first - 1).await;
        assert_eq!(state.current_height, height - 1);
        assert_eq!(
            outputs(&state),
            outputs_in(&synced.state, first - 1..height),
            "at height {height}"
        );
    }
}

#[tokio::test]
async fn a_restored_wallet_keeps_refreshing_with_any_number_of_blocks_recorded() {
    let node = Node::honest(SnapshotNode::new());
    for back in 1..=5u64 {
        node.inner.set_height(830);
        let restore_height = 830 - back;
        let mut state = WalletState::new();
        refresh(&node, &mut state, restore_height).await;
        refresh(&node, &mut state, restore_height).await;
        node.inner.set_height(831);
        refresh(&node, &mut state, restore_height).await;
        assert_eq!(state.current_height, 830, "restored {back} below the tip");
    }
}

#[tokio::test]
async fn a_restore_height_at_or_past_the_nodes_chain_is_reached_without_a_refused_request() {
    let synced = hostile::synced_wallet().await;
    for restore_height in [799, 800, 805, 2_000] {
        let node = Node::honest(SnapshotNode::new());
        node.inner.set_height(800);
        let mut state = WalletState::new();
        refresh(&node, &mut state, restore_height).await;
        refresh(&node, &mut state, restore_height).await;
        assert_eq!(state.current_height, 799, "restore height {restore_height}");
        node.inner.set_height(850);
        refresh(&node, &mut state, restore_height).await;
        assert_eq!(state.current_height, 849);
        assert_eq!(
            outputs(&state),
            outputs_in(&synced.state, restore_height..u64::MAX),
            "restore height {restore_height}"
        );
    }
}

#[tokio::test]
async fn a_fork_below_the_restore_height_is_rolled_back() {
    let node = Node::honest(SnapshotNode::new());
    let restore_height = 835;
    let mut state = WalletState::new();
    refresh(&node, &mut state, restore_height).await;
    let before = outputs(&state);

    node.inner.switch_chain(Chain::Shallow);
    let reorgs = refresh_following_reorgs(&node, &mut state, restore_height).await;
    assert_eq!(reorgs.len(), 1);
    assert_eq!(reorgs[0].split_height, 830);
    assert_eq!(reorgs[0].outputs_removed, before.len());

    let reference = scanned_from(&Node::honest(node.inner.clone()), restore_height).await;
    assert_eq!(state.current_height, reference.current_height);
    assert_eq!(outputs(&state), outputs(&reference));
}
