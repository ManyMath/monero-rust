#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::sync::{Arc, Mutex};

use hostile::epee::{self, Obj, Val};
use hostile::{HostileNode, SnapshotNode, LOOKAHEAD, NETWORK, NETWORK_STR, SEED_W, SEED_W2};
use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
use monero_rust::native::{create_transaction_with_rpc, StoredOutputData};
use monero_rust::scanner::{derive_address, scan_pool_update, MempoolView, Refresh};
use monero_rust::WalletState;
use serde_json::{json, Value};

type Node = HostileNode<SnapshotNode>;

#[derive(Clone)]
struct PoolTx {
    id: [u8; 32],
    pruned: Vec<u8>,
    prunable_hash: [u8; 32],
}

async fn pool_tx(wallet: &hostile::Wallet) -> PoolTx {
    let mut state = WalletState::new();
    state.replace_outputs(wallet.state.outputs().to_vec());
    state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    let input = wallet
        .unspent()
        .into_iter()
        .filter(|o| o.global_index.is_some() && !o.is_coinbase)
        .max_by_key(|o| o.amount)
        .expect("a spendable output")
        .clone();
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let built = create_transaction_with_rpc(
        &Node::honest(SnapshotNode::new()).rpc(),
        SEED_W,
        NETWORK_STR,
        vec![StoredOutputData::from(&input)],
        &[(w2, 100_000_000_000)],
        "",
        &mut state,
    )
    .await
    .expect("honest send builds");
    let full = hex::decode(&built.tx_blob).unwrap();
    let tx = Transaction::<NotPruned>::read(&mut full.as_slice()).unwrap();
    let pruned = Transaction::<Pruned>::from(tx.clone()).serialize();
    let prunable_hash = monero_oxide::primitives::keccak256(&full[pruned.len()..]);
    PoolTx {
        id: tx.hash(),
        pruned,
        prunable_hash,
    }
}

fn entry(tx: &PoolTx) -> Val {
    Val::Obj(vec![
        ("double_spend_seen".into(), Val::Bool(false)),
        ("tx_blob".into(), Val::Str(tx.pruned.clone())),
        ("tx_hash".into(), Val::Str(tx.id.to_vec())),
    ])
}

async fn synced_state() -> WalletState {
    let synced = hostile::synced_wallet().await;
    let mut state = WalletState::new();
    state.replace_outputs(synced.state.outputs().to_vec());
    state.block_hashes = synced.state.block_hashes.clone();
    state.current_height = synced.state.current_height;
    state
}

fn pool_node(
    pool: impl Fn(&Obj) -> Obj + Send + Sync + 'static,
    txs: Vec<PoolTx>,
    gettransactions: impl Fn(&Value, &[PoolTx]) -> Value + Send + Sync + 'static,
) -> Node {
    Node::with(SnapshotNode::new(), "pool", move |call, reply| {
        match call.route.as_str() {
            "getblocks.bin" => {
                let request = epee::decode(&call.body).unwrap();
                let mut body = epee::decode(&reply?).unwrap();
                if epee::get(&request, "requested_info").is_some() {
                    for (name, value) in pool(&request) {
                        epee::set(&mut body, &name, value);
                    }
                }
                Ok(epee::encode(&body))
            }
            "get_transaction_pool_hashes.bin" => Ok(hostile::pool_hashes_reply(
                &txs.iter().map(|tx| tx.id).collect::<Vec<_>>(),
            )),
            "gettransactions" => {
                let request: Value = serde_json::from_slice(&call.body).unwrap();
                Ok(serde_json::to_vec(&gettransactions(&request, &txs)).unwrap())
            }
            _ => reply,
        }
    })
}

fn honest_gettransactions(request: &Value, txs: &[PoolTx]) -> Value {
    let entries: Vec<Value> = request["txs_hashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hash| {
            let tx = txs
                .iter()
                .find(|tx| hex::encode(tx.id) == hash.as_str().unwrap())
                .unwrap();
            json!({
                "tx_hash": hex::encode(tx.id),
                "as_hex": "",
                "pruned_as_hex": hex::encode(&tx.pruned),
                "prunable_hash": hex::encode(tx.prunable_hash),
                "in_pool": true,
                "double_spend_seen": false,
            })
        })
        .collect();
    json!({ "status": "OK", "txs": entries })
}

fn full_pool(txs: Vec<PoolTx>, time: u64) -> impl Fn(&Obj) -> Obj + Send + Sync {
    move |_| {
        vec![
            ("daemon_time".into(), Val::U64(time)),
            ("pool_info_extent".into(), Val::U8(2)),
            (
                "added_pool_txs".into(),
                Val::Arr(12, txs.iter().map(entry).collect()),
            ),
            ("remaining_added_pool_txids".into(), Val::Str(vec![])),
        ]
    }
}

async fn refresh_pool(
    node: &Node,
    state: &WalletState,
    view: &mut MempoolView,
) -> Result<monero_rust::scanner::MempoolScanResult, String> {
    let mut refresh = Refresh::new(state, NETWORK, 0).unwrap().with_pool(view);
    let mut batch = refresh.next_batch(&node.rpc()).await?.expect("one request");
    let update = batch.take_pool().expect("the first batch carries the pool");
    assert!(batch.is_empty(), "the wallet is at the tip");
    scan_pool_update(update, view, SEED_W, NETWORK, LOOKAHEAD, None, "")
}

fn pool_request_fields(node: &Node, nth: usize) -> (Option<Val>, Option<Val>, bool) {
    let request = node.requests("getblocks.bin")[nth].epee();
    (
        epee::get(&request, "requested_info").cloned(),
        epee::get(&request, "pool_info_since").cloned(),
        epee::get(&request, "prune").unwrap().bool(),
    )
}

#[tokio::test]
async fn refresh_reads_the_pool_like_wallet2_then_works_incrementally() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let time = Arc::new(Mutex::new(0u64));
    let seen = time.clone();
    let id = tx.id;
    let node = pool_node(
        move |request| {
            *seen.lock().unwrap() += 1;
            if epee::get(request, "pool_info_since").is_none() {
                full_pool(vec![tx.clone()], 1000)(request)
            } else {
                vec![
                    ("daemon_time".into(), Val::U64(1060)),
                    ("pool_info_extent".into(), Val::U8(1)),
                    ("added_pool_txs".into(), Val::Arr(12, vec![])),
                    ("remaining_added_pool_txids".into(), Val::Str(vec![])),
                    ("removed_pool_txids".into(), Val::Str(id.to_vec())),
                ]
            }
        },
        vec![],
        honest_gettransactions,
    );

    let mut view = MempoolView::new();
    let first = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(first.tx_count, 1);
    assert_eq!(view.query_time(), 1000);
    assert!(view.contains(&hex::encode(id)));
    assert!(first
        .outputs
        .iter()
        .any(|o| o.tx_hash == hex::encode(id) && o.block_height == 0));
    assert_eq!(first.spent_key_images.len(), 1);
    assert_eq!(
        pool_request_fields(&node, 0),
        (Some(Val::U8(1)), None, true)
    );

    let second = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(second.tx_count, 0);
    assert!(second.outputs.is_empty());
    assert_eq!(view.query_time(), 1060);
    assert_eq!(
        pool_request_fields(&node, 1),
        (Some(Val::U8(1)), Some(Val::U64(1000)), true)
    );
    assert_eq!(node.names(), vec!["getblocks.bin", "getblocks.bin"]);
}

#[tokio::test]
async fn ids_a_restricted_node_only_lists_are_fetched_like_wallet2() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let node = pool_node(
        move |_| {
            vec![
                ("daemon_time".into(), Val::U64(5)),
                ("pool_info_extent".into(), Val::U8(2)),
                ("added_pool_txs".into(), Val::Arr(12, vec![])),
                ("remaining_added_pool_txids".into(), Val::Str(id.to_vec())),
            ]
        },
        vec![tx.clone()],
        honest_gettransactions,
    );
    let mut view = MempoolView::new();
    let result = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(result.tx_count, 1);
    assert!(result.outputs.iter().any(|o| o.tx_hash == hex::encode(id)));
    assert_eq!(node.names(), vec!["getblocks.bin", "gettransactions"]);
    let request = node.requests("gettransactions")[0].json();
    assert_eq!(request["txs_hashes"], json!([hex::encode(id)]));
    assert_eq!(request["prune"], json!(true));
    assert_eq!(request["decode_as_json"], json!(false));
    assert_eq!(request["client"].as_str().map(str::len), Some(208));
}

fn pool_outputs(result: &monero_rust::scanner::MempoolScanResult) -> Vec<(String, u64)> {
    result
        .outputs
        .iter()
        .map(|o| (o.tx_hash.clone(), o.amount))
        .collect()
}

#[tokio::test]
async fn a_transaction_reported_removed_and_added_again_stays_in_the_pool() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let node = pool_node(
        move |request| {
            if epee::get(request, "pool_info_since").is_none() {
                full_pool(vec![tx.clone()], 1000)(request)
            } else {
                vec![
                    ("daemon_time".into(), Val::U64(1060)),
                    ("pool_info_extent".into(), Val::U8(1)),
                    ("added_pool_txs".into(), Val::Arr(12, vec![entry(&tx)])),
                    ("remaining_added_pool_txids".into(), Val::Str(vec![])),
                    ("removed_pool_txids".into(), Val::Str(id.to_vec())),
                ]
            }
        },
        vec![],
        honest_gettransactions,
    );
    let mut view = MempoolView::new();
    let first = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(first.tx_count, 1);
    let second = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(second.tx_count, 1);
    assert!(view.contains(&hex::encode(id)));
    assert_eq!(pool_outputs(&second), pool_outputs(&first));
}

#[tokio::test]
async fn the_first_refresh_after_loading_the_view_asks_for_the_whole_pool() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let in_pool = tx.clone();
    let node_a = pool_node(
        move |request| full_pool(vec![in_pool.clone()], 1000)(request),
        vec![],
        honest_gettransactions,
    );
    let mut view = MempoolView::new();
    let first = refresh_pool(&node_a, &state, &mut view).await.unwrap();
    assert_eq!(first.tx_count, 1);
    let stored = serde_json::to_string(&view).unwrap();

    let mut loaded: MempoolView = serde_json::from_str(&stored).unwrap();
    assert!(loaded.contains(&hex::encode(id)));
    assert_eq!(loaded.query_time(), 0);
    let again = refresh_pool(&node_a, &state, &mut loaded).await.unwrap();
    assert_eq!(
        pool_request_fields(&node_a, 1),
        (Some(Val::U8(1)), None, true)
    );
    assert_eq!(pool_outputs(&again), pool_outputs(&first));

    let node_b = pool_node(
        |request| {
            if epee::get(request, "pool_info_since").is_none() {
                full_pool(vec![], 5000)(request)
            } else {
                vec![
                    ("daemon_time".into(), Val::U64(5000)),
                    ("pool_info_extent".into(), Val::U8(1)),
                    ("added_pool_txs".into(), Val::Arr(12, vec![])),
                    ("remaining_added_pool_txids".into(), Val::Str(vec![])),
                ]
            }
        },
        vec![],
        honest_gettransactions,
    );
    let mut loaded: MempoolView = serde_json::from_str(&stored).unwrap();
    let result = refresh_pool(&node_b, &state, &mut loaded).await.unwrap();
    assert_eq!(
        pool_request_fields(&node_b, 0),
        (Some(Val::U8(1)), None, true)
    );
    assert!(!loaded.contains(&hex::encode(id)));
    assert_eq!(result.tx_count, 0);
    assert!(result.outputs.is_empty());
}

#[tokio::test]
async fn a_view_moved_to_another_node_asks_it_for_the_whole_pool() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let node_a = pool_node(
        move |request| full_pool(vec![tx.clone()], 1000)(request),
        vec![],
        honest_gettransactions,
    );
    let node_b = pool_node(
        |request| {
            if epee::get(request, "pool_info_since").is_none() {
                full_pool(vec![], 5000)(request)
            } else {
                vec![
                    ("daemon_time".into(), Val::U64(5000)),
                    ("pool_info_extent".into(), Val::U8(1)),
                    ("added_pool_txs".into(), Val::Arr(12, vec![])),
                    ("remaining_added_pool_txids".into(), Val::Str(vec![])),
                ]
            }
        },
        vec![],
        honest_gettransactions,
    );
    let mut view = MempoolView::new();
    refresh_pool(&node_a, &state, &mut view).await.unwrap();
    view.node_changed();
    let result = refresh_pool(&node_b, &state, &mut view).await.unwrap();
    assert_eq!(
        pool_request_fields(&node_b, 0),
        (Some(Val::U8(1)), None, true)
    );
    assert!(!view.contains(&hex::encode(id)));
    assert!(result.outputs.is_empty());
    assert_eq!(view.query_time(), 5000);
}

#[tokio::test]
async fn a_node_without_pool_data_is_asked_the_old_way() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let node = pool_node(|_| vec![], vec![tx.clone()], honest_gettransactions);
    let mut view = MempoolView::new();
    let result = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(result.tx_count, 1);
    assert_eq!(view.query_time(), 0, "no daemon time to ask from");
    assert_eq!(
        node.names(),
        vec![
            "getblocks.bin",
            "get_transaction_pool_hashes.bin",
            "gettransactions"
        ]
    );
}

#[tokio::test]
async fn malformed_pool_data_is_refused() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    type Shape = Box<dyn Fn(&Obj) -> Obj + Send + Sync>;
    let mut cases: Vec<(&str, Shape)> = Vec::new();
    let t = tx.clone();
    cases.push((
        "an unknown extent",
        Box::new(move |r| {
            let mut fields = full_pool(vec![t.clone()], 1)(r);
            fields[1].1 = Val::U8(3);
            fields
        }),
    ));
    let t = tx.clone();
    cases.push((
        "removed ids in a full answer",
        Box::new(move |r| {
            let mut fields = full_pool(vec![t.clone()], 1)(r);
            fields.push(("removed_pool_txids".into(), Val::Str(id.to_vec())));
            fields
        }),
    ));
    let t = tx.clone();
    cases.push((
        "an incremental answer to a request for the whole pool",
        Box::new(move |r| {
            let mut fields = full_pool(vec![t.clone()], 1)(r);
            fields[1].1 = Val::U8(1);
            fields
        }),
    ));
    let t = tx.clone();
    cases.push((
        "one transaction twice",
        Box::new(move |r| full_pool(vec![t.clone(), t.clone()], 1)(r)),
    ));
    let t = tx.clone();
    cases.push((
        "an id listed both inline and by id",
        Box::new(move |r| {
            let mut fields = full_pool(vec![t.clone()], 1)(r);
            fields[3].1 = Val::Str(id.to_vec());
            fields
        }),
    ));
    cases.push((
        "a remaining-id list that is not whole ids",
        Box::new(move |r| {
            let mut fields = full_pool(vec![], 1)(r);
            fields[3].1 = Val::Str(vec![7; 33]);
            fields
        }),
    ));
    let mut broken = tx.clone();
    broken.pruned.truncate(broken.pruned.len() - 5);
    cases.push((
        "a transaction that does not parse",
        Box::new(move |r| full_pool(vec![broken.clone()], 1)(r)),
    ));
    let t = tx.clone();
    cases.push((
        "a short transaction id",
        Box::new(move |r| {
            let mut fields = full_pool(vec![t.clone()], 1)(r);
            if let Val::Arr(_, items) = &mut fields[2].1 {
                epee::set(items[0].obj_mut(), "tx_hash", Val::Str(vec![1; 31]));
            }
            fields
        }),
    ));
    for (name, shape) in cases {
        let node = pool_node(shape, vec![], honest_gettransactions);
        let mut view = MempoolView::new();
        let result = refresh_pool(&node, &state, &mut view).await;
        assert!(result.is_err(), "{name}: accepted");
        assert!(
            view.is_empty() && view.query_time() == 0,
            "{name}: view changed"
        );
    }
}

#[tokio::test]
async fn bad_gettransactions_answers_are_refused() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let listed = move |_: &Obj| {
        vec![
            ("daemon_time".into(), Val::U64(5)),
            ("pool_info_extent".into(), Val::U8(2)),
            ("added_pool_txs".into(), Val::Arr(12, vec![])),
            ("remaining_added_pool_txids".into(), Val::Str(id.to_vec())),
        ]
    };
    type Answer = Box<dyn Fn(&Value, &[PoolTx]) -> Value + Send + Sync>;
    let cases: Vec<(&str, Answer)> = vec![
        (
            "no entries",
            Box::new(|_, _| json!({ "status": "OK", "txs": [] })),
        ),
        (
            "a transaction that does not hash to its id",
            Box::new(|request, txs| {
                let mut answer = honest_gettransactions(request, txs);
                answer["txs"][0]["prunable_hash"] = json!(hex::encode([9u8; 32]));
                answer
            }),
        ),
        (
            "an id that was not asked for",
            Box::new(|request, txs| {
                let mut answer = honest_gettransactions(request, txs);
                answer["txs"][0]["tx_hash"] = json!(hex::encode([9u8; 32]));
                answer
            }),
        ),
        (
            "a failed status",
            Box::new(|request, txs| {
                let mut answer = honest_gettransactions(request, txs);
                answer["status"] = json!("Failed");
                answer
            }),
        ),
    ];
    for (name, answer) in cases {
        let node = pool_node(listed, vec![tx.clone()], answer);
        let mut view = MempoolView::new();
        assert!(
            refresh_pool(&node, &state, &mut view).await.is_err(),
            "{name}: accepted"
        );
        assert!(view.is_empty(), "{name}: view changed");
    }
    let node = pool_node(
        listed,
        vec![tx.clone()],
        |request: &Value, txs: &[PoolTx]| {
            let mut answer = honest_gettransactions(request, txs);
            answer["txs"][0]["in_pool"] = json!(false);
            answer
        },
    );
    let mut view = MempoolView::new();
    assert_eq!(
        refresh_pool(&node, &state, &mut view)
            .await
            .unwrap()
            .tx_count,
        0
    );
}

#[tokio::test]
async fn pool_fields_nobody_asked_for_are_refused() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let node = Node::with(SnapshotNode::new(), "unasked-pool", move |call, reply| {
        if !call.is("getblocks.bin") {
            return reply;
        }
        let mut body = epee::decode(&reply?).unwrap();
        for (name, value) in full_pool(vec![tx.clone()], 1)(&vec![]) {
            epee::set(&mut body, &name, value);
        }
        Ok(epee::encode(&body))
    });
    let mut refresh = Refresh::new(&state, NETWORK, 0).unwrap();
    assert!(refresh.next_batch(&node.rpc()).await.is_err());
}

#[tokio::test]
async fn a_whole_pool_answer_drops_what_left_the_pool() {
    let state = synced_state().await;
    let tx = pool_tx(&hostile::synced_wallet().await).await;
    let id = tx.id;
    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let node = pool_node(
        move |r| {
            let mut count = counter.lock().unwrap();
            *count += 1;
            full_pool(
                if *count == 1 {
                    vec![tx.clone()]
                } else {
                    vec![]
                },
                7,
            )(r)
        },
        vec![],
        honest_gettransactions,
    );
    let mut view = MempoolView::new();
    assert_eq!(
        refresh_pool(&node, &state, &mut view)
            .await
            .unwrap()
            .tx_count,
        1
    );
    let second = refresh_pool(&node, &state, &mut view).await.unwrap();
    assert_eq!(second.tx_count, 0);
    assert!(!view.contains(&hex::encode(id)));
}
