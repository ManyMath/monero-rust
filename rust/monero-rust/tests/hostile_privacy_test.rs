//! What the wallet's requests tell the node.

#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::collections::HashSet;

use hostile::epee::{self, Val};
use hostile::{
    distribution_reply, edit_outs, HostileNode, SnapshotNode, Wallet, LOOKAHEAD, NETWORK,
    NETWORK_STR, SEED_W, SEED_W2,
};
use monero_rust::native::{
    broadcast_transaction_with_rpc, create_transaction_with_rpc, sweep_all_with_rpc,
    StoredOutputData, TransactionResult,
};
use monero_rust::scanner::{derive_address, scan_mempool};
use monero_rust::{WalletOutput, WalletState};

type Node = HostileNode<SnapshotNode>;

const REVEALING: &[&str] = &[
    "get_transactions",
    "gettransactions",
    "get_o_indexes.bin",
    "is_key_image_spent",
    "get_outs",
];

fn change_output(wallet: &Wallet) -> WalletOutput {
    wallet
        .unspent()
        .into_iter()
        .find(|o| !o.is_coinbase && o.block_height == 700)
        .unwrap()
        .clone()
}

fn coinbase_at(wallet: &Wallet, height: u64) -> WalletOutput {
    wallet
        .unspent()
        .into_iter()
        .find(|o| o.is_coinbase && o.block_height == height)
        .unwrap()
        .clone()
}

async fn send(
    node: &Node,
    inputs: &[WalletOutput],
    wallet_state: &mut WalletState,
) -> Result<TransactionResult, String> {
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    create_transaction_with_rpc(
        &node.rpc(),
        SEED_W,
        NETWORK_STR,
        inputs.iter().map(StoredOutputData::from).collect(),
        &[(w2, 100_000_000_000)],
        "",
        wallet_state,
    )
    .await
}

fn assert_no_revealing_requests(node: &Node) {
    for name in node.names() {
        assert!(
            !REVEALING.contains(&name.as_str()),
            "{}: wallet sent {name}",
            node.mutation
        );
    }
}

#[tokio::test]
async fn honest_send_hides_each_input_in_one_batched_get_outs_request() {
    let mut wallet = hostile::synced_wallet().await;
    let node = Node::honest(SnapshotNode::new());
    let inputs = [change_output(&wallet), coinbase_at(&wallet, 500)];
    let tx = send(&node, &inputs, &mut wallet.state)
        .await
        .expect("honest send");
    broadcast_transaction_with_rpc(&node.rpc(), &tx, &mut wallet.state, false)
        .await
        .unwrap();

    assert_eq!(
        node.names(),
        vec![
            "json_rpc:hard_fork_info",
            "json_rpc:get_fee_estimate",
            "json_rpc:get_txpool_backlog",
            "json_rpc:get_info",
            "json_rpc:getblockheadersrange",
            "get_height",
            "get_height",
            "get_output_distribution.bin",
            "get_outs.bin",
            "sendrawtransaction",
        ]
    );
    assert_no_revealing_requests(&node);

    let request = &node.requests("get_outs.bin")[0];
    assert_eq!(
        epee::get(&request.epee(), "get_txid"),
        Some(&Val::Bool(false))
    );
    let indices = request.get_outs_indices();
    assert_eq!(indices.len(), 150, "75 candidates per input, like wallet2");
    for (chunk, input) in indices.chunks(75).zip(&inputs) {
        assert!(
            chunk.windows(2).all(|pair| pair[0] < pair[1]),
            "sorted, distinct"
        );
        assert!(chunk.contains(&input.global_index.unwrap()));
    }
    let distribution = node.requests("get_output_distribution.bin")[0].epee();
    assert_eq!(epee::items(&distribution, "amounts"), &[Val::U64(0)]);
}

#[tokio::test]
async fn sweep_request_pattern_matches_a_send() {
    let mut wallet = hostile::synced_wallet().await;
    let node = Node::honest(SnapshotNode::new());
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let inputs = vec![StoredOutputData::from(&change_output(&wallet))];
    let _ = sweep_all_with_rpc(
        &node.rpc(),
        SEED_W,
        NETWORK_STR,
        inputs,
        &w2,
        "",
        &mut wallet.state,
    )
    .await
    .expect("honest sweep");
    assert_eq!(node.count("get_outs.bin"), 1);
    assert_eq!(
        node.requests("get_outs.bin")[0].get_outs_indices().len(),
        75
    );
    assert_no_revealing_requests(&node);
}

fn without_client(request: &hostile::Logged) -> String {
    if request.route.ends_with(".bin") {
        let mut members = request.epee();
        members.retain(|(name, _)| name != "client");
        format!("{members:?}")
    } else {
        let mut json = request.json();
        if let Some(params) = json["params"].as_object_mut() {
            params.remove("client");
        }
        json.to_string()
    }
}

#[tokio::test]
async fn refresh_requests_do_not_depend_on_the_wallet() {
    let mut logs = Vec::new();
    for seed in [SEED_W, SEED_W2] {
        let node = Node::honest(SnapshotNode::new());
        let mut wallet = Wallet::new(seed);
        wallet.refresh(&node.rpc(), 64).await.unwrap();
        assert!(!wallet.outputs().is_empty());
        assert_no_revealing_requests(&node);
        let allowed: HashSet<&str> =
            ["json_rpc:get_block_header_by_height", "getblocks.bin"].into();
        assert!(node
            .names()
            .iter()
            .all(|name| allowed.contains(name.as_str())));
        logs.push(
            node.log()
                .into_iter()
                .map(|l| (l.name(), without_client(&l)))
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(logs[0], logs[1]);
}

#[tokio::test]
async fn mempool_scan_fetches_the_whole_pool_only() {
    let node = Node::honest(SnapshotNode::new());
    scan_mempool(&node.rpc(), SEED_W, NETWORK, LOOKAHEAD, None, "")
        .await
        .unwrap();
    assert_eq!(node.names(), vec!["get_transaction_pool_hashes.bin"]);
}

#[tokio::test]
async fn persisted_ring_is_reused_and_never_silently_replaced() {
    let mut wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let node = Node::honest(SnapshotNode::new());
    let first = send(&node, &[real.clone()], &mut wallet.state)
        .await
        .unwrap();
    first.persist_input_rings(&mut wallet.state).unwrap();
    let persisted = change_output(&wallet);
    assert!(persisted.ring.is_some());

    let second = send(&node, &[persisted.clone()], &mut wallet.state)
        .await
        .unwrap();
    assert_eq!(first.input_rings[0].ring, second.input_rings[0].ring);
    let ring: HashSet<u64> = {
        let mut index = 0;
        first.input_rings[0]
            .ring
            .offsets
            .iter()
            .map(|offset| {
                index += offset;
                index
            })
            .collect()
    };
    let requested: HashSet<u64> = node.requests("get_outs.bin")[1]
        .get_outs_indices()
        .into_iter()
        .collect();
    assert!(ring.is_subset(&requested));
    assert_eq!(
        requested.len(),
        75,
        "the rebuild request is padded like a fresh one"
    );

    let real_index = real.global_index.unwrap();
    let victim = *ring.iter().find(|i| **i != real_index).unwrap();
    let hostile = Node::with(
        SnapshotNode::new(),
        "persisted-decoy-withdrawn",
        move |call, reply| {
            if call.is("get_outs.bin") {
                return edit_outs(call, reply, |index, out| {
                    if index == victim {
                        epee::set(out, "unlocked", Val::Bool(false));
                    }
                });
            }
            reply
        },
    );
    let err = send(&hostile, &[persisted], &mut wallet.state)
        .await
        .unwrap_err();
    assert!(err.contains("persisted ring member"), "{err}");
    assert_eq!(hostile.count("get_outs.bin"), 1);
}

#[tokio::test]
async fn sanity_check_failure_sends_one_candidate_request() {
    let mut wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    let honest = SnapshotNode::new();
    let inner = honest.clone();
    let node = Node::with(honest, "distribution-fails-sanity", move |call, reply| {
        if call.is("get_output_distribution.bin") {
            let honest = inner.outputs_per_block();
            let mut per_block = vec![0u64; 262_800 - honest.len()];
            per_block.extend(honest);
            *per_block.last_mut().unwrap() += 10_000;
            return distribution_reply(&per_block);
        }
        reply
    });
    let err = send(&node, &[real], &mut wallet.state).await.unwrap_err();
    assert!(err.contains("sanity check"), "{err}");

    let requests: Vec<HashSet<u64>> = node
        .requests("get_outs.bin")
        .iter()
        .map(|r| r.get_outs_indices().into_iter().collect())
        .collect();
    let mut common = requests[0].clone();
    for request in &requests[1..] {
        common = common.intersection(request).copied().collect();
    }
    assert!(common.contains(&real_index));
    assert!(
        requests.len() <= 1,
        "the node was sent {} get_outs requests for one input; their intersection is {} of 75 \
         candidates and contains the real output {real_index}",
        requests.len(),
        common.len()
    );
}

#[tokio::test]
async fn failed_get_outs_keeps_the_candidates_so_a_user_retry_repeats_them() {
    let mut wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    let node = Node::with(
        SnapshotNode::new(),
        "get-outs-spoiled-once",
        move |call, reply| {
            if call.is("get_outs.bin") && call.nth == 0 {
                return edit_outs(call, reply, |index, out| {
                    if index != real_index {
                        epee::set(out, "unlocked", Val::Bool(false));
                    }
                });
            }
            reply
        },
    );
    send(&node, &[real.clone()], &mut wallet.state)
        .await
        .unwrap_err();
    let _ = send(&node, &[real], &mut wallet.state)
        .await
        .expect("second attempt succeeds");
    let requests: Vec<HashSet<u64>> = node
        .requests("get_outs.bin")
        .iter()
        .map(|r| r.get_outs_indices().into_iter().collect())
        .collect();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains(&real_index));
    assert_eq!(
        requests[0], requests[1],
        "the retry asks for the same candidates"
    );
}

#[tokio::test]
async fn automatic_priority_is_settled_once_per_transaction() {
    use monero_rust::monero_rpc::{FeePriority, DEFAULT_MAX_FEE_PER_BYTE};
    use monero_rust::native::{create_transaction_with_rpc_and_policy, FeePolicy};
    let mut wallet = hostile::synced_wallet().await;
    let node = Node::honest(SnapshotNode::new());
    let rpc = node.rpc().for_wallet(&wallet.state);
    let height = wallet.state.chain_height();
    let policy = FeePolicy {
        priority: FeePriority::Automatic,
        wallet_height: Some(height),
        max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
    };
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let prepared = monero_rust::prepare_send_inputs_with_rpc(
        &rpc,
        wallet.outputs(),
        height,
        100_000_000_000,
        1,
        None,
        None,
        0,
        policy,
    )
    .await
    .unwrap();
    let settled = prepared.fee_policy.expect("the settled policy");
    assert_ne!(settled.priority, FeePriority::Automatic);
    let tier = rpc
        .get_fee_checked_for_priority_at_height(
            settled.priority,
            settled.wallet_height,
            settled.max_fee_per_weight,
        )
        .await
        .unwrap();
    assert_eq!(prepared.fee_rate, Some(tier));
    let _built = create_transaction_with_rpc_and_policy(
        &rpc,
        SEED_W,
        NETWORK_STR,
        prepared.stored_outputs.clone(),
        &[(w2, 100_000_000_000)],
        "",
        settled,
        &mut wallet.state,
    )
    .await
    .unwrap();
    assert_eq!(
        node.count("json_rpc:get_txpool_backlog"),
        1,
        "{:?}",
        node.names()
    );
    assert_eq!(
        node.count("json_rpc:hard_fork_info"),
        1,
        "{:?}",
        node.names()
    );
    assert_eq!(node.count("json_rpc:get_last_block_header"), 0);
}
