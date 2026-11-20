//! Records the chain snapshot the hostile node harness replays, from a fresh
//! private regtest daemon, in two phases:
//!
//!     eval "$(MONEROD=... scripts/regtest-node.sh start)"
//!     HOSTILE_RPC_RECORD=main  cargo test -p monero-rust --test hostile_rpc_record -- --ignored --nocapture
//!     HOSTILE_RPC_RECORD=forks cargo test -p monero-rust --test hostile_rpc_record -- --ignored --nocapture

#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use hostile::epee::{self, Obj, Val};
use hostile::{
    BlockRec, Fork, LiveNode, OutRec, Snapshot, Wallet, NETWORK, NETWORK_STR, SEED_W, SEED_W2,
};
use monero_oxide::block::Block;
use monero_rust::monero_rpc::{Rpc, RpcConnection};
use monero_rust::native::{create_transaction_with_rpc, StoredOutputData};
use monero_rust::scanner::{derive_address, derive_subaddress};
use serde_json::{json, Value};

const PRE_TX_HEIGHT: u64 = 700;
const MAIN_TIP: u64 = 850;
const SHALLOW_DEPTH: u64 = 20;

async fn json_rpc(node: &LiveNode, method: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0", "id": "0", "method": method, "params": params
    }))
    .unwrap();
    let reply: Value = serde_json::from_slice(&node.post("json_rpc", body).await.unwrap()).unwrap();
    assert!(reply.get("error").is_none(), "{method} failed: {reply}");
    reply["result"].clone()
}

async fn other(node: &LiveNode, route: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&params).unwrap();
    serde_json::from_slice(&node.post(route, body).await.unwrap()).unwrap()
}

async fn height(node: &LiveNode) -> u64 {
    other(node, "get_height", json!({})).await["height"]
        .as_u64()
        .unwrap()
}

async fn mine(node: &LiveNode, address: &str, mut count: u64) {
    while count > 0 {
        let step = count.min(100);
        json_rpc(
            node,
            "generateblocks",
            json!({ "wallet_address": address, "amount_of_blocks": step }),
        )
        .await;
        count -= step;
    }
}

fn getblocks_request(ids: &[Vec<u8>], start_height: u64) -> Vec<u8> {
    epee::encode(&vec![
        ("block_ids".into(), Val::Str(ids.concat())),
        ("start_height".into(), Val::U64(start_height)),
        ("prune".into(), Val::Bool(false)),
        ("no_miner_tx".into(), Val::Bool(false)),
    ])
}

fn header_subset(header: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in [
        "block_weight",
        "hash",
        "height",
        "major_version",
        "minor_version",
        "miner_tx_hash",
        "num_txes",
        "prev_hash",
        "reward",
        "timestamp",
    ] {
        out.insert(key.to_string(), header[key].clone());
    }
    Value::Object(out)
}

async fn record_blocks(
    node: &LiveNode,
    genesis: &[u8],
    from: u64,
    to: u64,
) -> (Vec<BlockRec>, Obj) {
    let mut records = Vec::new();
    let mut template = Obj::new();
    let mut next = from;
    while next < to {
        let raw = node
            .post(
                "getblocks.bin",
                getblocks_request(&[genesis.to_vec()], next),
            )
            .await
            .unwrap();
        let mut reply = epee::decode(&raw).expect("getblocks reply decodes");
        assert_eq!(epee::get(&reply, "status").unwrap().bytes(), b"OK");
        assert_eq!(epee::get(&reply, "start_height").unwrap().u64(), next);
        let blocks = epee::remove(&mut reply, "blocks").expect("blocks");
        let indices = epee::remove(&mut reply, "output_indices").expect("output_indices");
        assert_eq!(blocks.arr().len(), indices.arr().len());
        let headers = json_rpc(
            node,
            "getblockheadersrange",
            json!({ "start_height": next, "end_height": (next + blocks.arr().len() as u64 - 1).min(to - 1) }),
        )
        .await;
        for (offset, (entry, index)) in blocks.arr().iter().zip(indices.arr()).enumerate() {
            let h = next + offset as u64;
            if h >= to {
                break;
            }
            let header = &headers["headers"][offset];
            assert_eq!(header["height"].as_u64(), Some(h));
            let blob = epee::get(entry.obj(), "block").unwrap().bytes().to_vec();
            let block = Block::read(&mut blob.as_slice()).expect("block parses");
            assert_eq!(hex::encode(block.hash()), header["hash"].as_str().unwrap());
            records.push(BlockRec {
                hash: hex::encode(block.hash()),
                entry: hex::encode(epee::encode(entry.obj())),
                indices: hex::encode(epee::encode(index.obj())),
                header: header_subset(header),
            });
        }
        next += blocks.arr().len() as u64;
        template = reply;
    }
    (records, template)
}

async fn record_outs(
    node: &LiveNode,
    blocks: &dyn Fn(u64) -> BlockRec,
    from: u64,
    to: u64,
) -> Vec<OutRec> {
    let mut outs = Vec::new();
    let tip = height(node).await;
    let mut next = from;
    while next < to {
        let end = (next + 500).min(to);
        let request = epee::encode(&vec![
            (
                "outputs".into(),
                Val::Arr(
                    12,
                    (next..end)
                        .map(|index| {
                            Val::Obj(vec![
                                ("amount".into(), Val::U64(0)),
                                ("index".into(), Val::U64(index)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("get_txid".into(), Val::Bool(true)),
        ]);
        let reply = epee::decode(&node.post("get_outs.bin", request).await.unwrap()).unwrap();
        assert_eq!(epee::get(&reply, "status").unwrap().bytes(), b"OK");
        for out in epee::items(&reply, "outs") {
            let out = out.obj();
            let out_height = epee::get(out, "height").unwrap().u64();
            let txid = hex::encode(epee::get(out, "txid").unwrap().bytes());
            let record = blocks(out_height);
            let coinbase = record.header["miner_tx_hash"].as_str() == Some(txid.as_str());
            let unlocked = epee::get(out, "unlocked").unwrap().bool();
            assert_eq!(
                unlocked,
                !coinbase || tip >= out_height + 60,
                "unlocked model mismatch at height {out_height} (tip {tip}, coinbase {coinbase})"
            );
            outs.push(OutRec {
                key: hex::encode(epee::get(out, "key").unwrap().bytes()),
                mask: hex::encode(epee::get(out, "mask").unwrap().bytes()),
                height: out_height,
                txid,
                coinbase,
            });
        }
        next = end;
    }
    outs
}

fn first_out_of(record: &BlockRec) -> u64 {
    let indices = epee::decode(&hex::decode(&record.indices).unwrap()).unwrap();
    epee::items(epee::items(&indices, "indices")[0].obj(), "indices")[0].u64()
}

fn total_outs(records: &[BlockRec]) -> u64 {
    records.iter().map(hostile::ringct_outputs_in).sum()
}

fn partial_path() -> std::path::PathBuf {
    hostile::fixture_path().with_file_name("snapshot.json")
}

async fn record_main(node: &LiveNode) {
    assert_eq!(
        height(node).await,
        1,
        "phase main needs a fresh regtest chain"
    );
    let rpc = Rpc::new_with_connection(node.clone());
    let w = derive_address(SEED_W, NETWORK, "").unwrap().to_string();
    let w_sub = derive_subaddress(SEED_W, NETWORK, 0, 1, "")
        .unwrap()
        .to_string();
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();

    mine(node, &w, PRE_TX_HEIGHT - 1).await;
    assert_eq!(height(node).await, PRE_TX_HEIGHT);

    let mut wallet = Wallet::new(SEED_W);
    wallet.refresh(&rpc, 64).await.expect("refresh W");
    let mature: Vec<StoredOutputData> = wallet
        .outputs()
        .iter()
        .filter(|o| o.block_height + 61 <= PRE_TX_HEIGHT)
        .map(StoredOutputData::from)
        .collect();
    assert!(mature.len() > 600);

    let xmr = 1_000_000_000_000u64;
    let plans: Vec<(Vec<StoredOutputData>, Vec<(String, u64)>)> = vec![
        (vec![mature[100].clone()], vec![(w2.clone(), 5 * xmr)]),
        (
            vec![mature[200].clone(), mature[201].clone()],
            vec![(w2.clone(), xmr), (w_sub.clone(), 2 * xmr)],
        ),
        (vec![mature[300].clone()], vec![(w2.clone(), 3 * xmr)]),
    ];
    let mut send_raw_ok = Value::Null;
    let mut tx_heights = Vec::new();
    for (position, (inputs, recipients)) in plans.into_iter().enumerate() {
        let tx = create_transaction_with_rpc(
            &rpc,
            SEED_W,
            NETWORK_STR,
            inputs,
            &recipients,
            "",
            &mut wallet.state,
        )
        .await
        .expect("honest send builds");
        let reply = other(
            node,
            "send_raw_transaction",
            json!({ "tx_as_hex": tx.tx_blob, "do_not_relay": false }),
        )
        .await;
        assert_eq!(
            reply["status"], "OK",
            "daemon rejected an honest tx: {reply}"
        );
        send_raw_ok = reply;
        if position != 0 {
            tx_heights.push(height(node).await);
            mine(node, &w2, 1).await;
        }
    }
    let mined = height(node).await;
    mine(node, &w, MAIN_TIP - mined).await;
    assert_eq!(height(node).await, MAIN_TIP);

    let genesis = hex::decode(
        json_rpc(node, "get_block_header_by_height", json!({ "height": 0 })).await["block_header"]
            ["hash"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let (chain, template) = record_blocks(node, &genesis, 0, MAIN_TIP).await;
    assert_eq!(chain.len() as u64, MAIN_TIP);
    let total = total_outs(&chain);
    let chain_ref = chain.clone();
    let outs = record_outs(node, &move |h| chain_ref[h as usize].clone(), 0, total).await;

    let at_tip = node
        .post(
            "getblocks.bin",
            getblocks_request(&[genesis.clone()], MAIN_TIP),
        )
        .await
        .unwrap();
    println!("getblocks at tip: {:?}", epee::decode(&at_tip));
    let tip_hash = hex::decode(&chain.last().unwrap().hash).unwrap();
    let with_history = node
        .post(
            "getblocks.bin",
            getblocks_request(&[tip_hash, genesis.clone()], 0),
        )
        .await
        .unwrap();
    let with_history = epee::decode(&with_history).unwrap();
    println!(
        "getblocks with tip in history: status {:?} start {:?} blocks {}",
        epee::get(&with_history, "status"),
        epee::get(&with_history, "start_height"),
        epee::items(&with_history, "blocks").len()
    );

    let distribution_raw = node
        .post(
            "get_output_distribution.bin",
            epee::encode(&vec![
                ("amounts".into(), Val::Arr(5, vec![Val::U64(0)])),
                ("compress".into(), Val::Bool(true)),
                ("binary".into(), Val::Bool(true)),
            ]),
        )
        .await
        .unwrap();
    println!("distribution reply: {:?}", {
        let mut decoded = epee::decode(&distribution_raw).unwrap();
        let first = epee::get_mut(&mut decoded, "distributions")
            .unwrap()
            .arr_mut()[0]
            .obj_mut();
        epee::remove(first, "compressed_data");
        decoded
    });
    let sample_request = epee::encode(&vec![
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
    let get_outs_sample = node.post("get_outs.bin", sample_request).await.unwrap();

    let mut wallet = Wallet::new(SEED_W);
    wallet.refresh(&rpc, 64).await.expect("refresh W at tip");
    let spent = wallet.outputs().iter().filter(|o| o.spent).count();
    let received: Vec<_> = wallet.outputs().iter().filter(|o| !o.is_coinbase).collect();
    println!(
        "W at tip: {} outputs, {} spent, {} non-coinbase",
        wallet.outputs().len(),
        spent,
        received.len()
    );
    assert_eq!(spent, 4);
    assert_eq!(received.len(), 4);

    let empty = Fork {
        fork_height: 0,
        blocks: vec![],
        first_out: 0,
        outs: vec![],
    };
    let snapshot = Snapshot {
        monerod_version: json_rpc(node, "get_info", json!({})).await["version"]
            .as_str()
            .unwrap()
            .to_string(),
        chain,
        outs,
        fork_shallow: empty.clone(),
        fork_deep: empty,
        tx_heights,
        get_info: json_rpc(node, "get_info", json!({})).await,
        fee_estimate: json_rpc(node, "get_fee_estimate", json!({ "grace_blocks": 10 })).await,
        txpool_backlog: json_rpc(node, "get_txpool_backlog", json!({})).await,
        transaction_pool: other(node, "get_transaction_pool", json!({})).await,
        send_raw_ok,
        getblocks_template: hex::encode(epee::encode(&template)),
        getblocks_at_tip: hex::encode(at_tip),
        distribution_raw: hex::encode(distribution_raw),
        get_outs_sample: hex::encode(get_outs_sample),
    };
    std::fs::create_dir_all(partial_path().parent().unwrap()).unwrap();
    std::fs::write(partial_path(), serde_json::to_string(&snapshot).unwrap()).unwrap();
    println!(
        "recorded main chain: {} blocks, {} outputs",
        MAIN_TIP, total
    );
}

async fn record_fork(
    node: &LiveNode,
    snapshot: &Snapshot,
    fork_height: u64,
    extra: u64,
    flush: bool,
) -> Fork {
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let current = height(node).await;
    let popped = other(
        node,
        "pop_blocks",
        json!({ "nblocks": current - fork_height }),
    )
    .await;
    assert_eq!(popped["status"], "OK", "{popped}");
    if flush {
        json_rpc(node, "flush_txpool", json!({})).await;
    }
    assert_eq!(height(node).await, fork_height);
    let pool = other(node, "get_transaction_pool", json!({})).await;
    assert!(
        pool.get("transactions")
            .map_or(true, |t| t.as_array().map_or(true, Vec::is_empty)),
        "pool must be empty before mining a fork: {pool}"
    );
    let new_tip = snapshot.chain.len() as u64 + extra;
    mine(node, &w2, new_tip - fork_height).await;

    let genesis = hex::decode(&snapshot.chain[0].hash).unwrap();
    let (blocks, _) = record_blocks(node, &genesis, fork_height, new_tip).await;
    assert_ne!(blocks[0].hash, snapshot.chain[fork_height as usize].hash);
    let first_out = first_out_of(&blocks[0]);
    assert_eq!(
        first_out,
        first_out_of(&snapshot.chain[fork_height as usize])
    );
    let total = first_out + total_outs(&blocks);
    let main = snapshot.chain.clone();
    let forked = blocks.clone();
    let outs = record_outs(
        node,
        &move |h| {
            if h >= fork_height {
                forked[(h - fork_height) as usize].clone()
            } else {
                main[h as usize].clone()
            }
        },
        first_out,
        total,
    )
    .await;
    Fork {
        fork_height,
        blocks,
        first_out,
        outs,
    }
}

async fn record_forks(node: &LiveNode) {
    let mut snapshot: Snapshot =
        serde_json::from_str(&std::fs::read_to_string(partial_path()).unwrap()).unwrap();
    assert_eq!(
        height(node).await,
        MAIN_TIP,
        "phase forks continues from phase main"
    );
    snapshot.fork_shallow = record_fork(node, &snapshot, MAIN_TIP - SHALLOW_DEPTH, 5, false).await;
    snapshot.fork_deep = record_fork(node, &snapshot, snapshot.tx_heights[0], 15, true).await;
    std::fs::write(partial_path(), serde_json::to_string(&snapshot).unwrap()).unwrap();
    println!(
        "recorded forks: shallow from {} ({} blocks), deep from {} ({} blocks)",
        snapshot.fork_shallow.fork_height,
        snapshot.fork_shallow.blocks.len(),
        snapshot.fork_deep.fork_height,
        snapshot.fork_deep.blocks.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "records tests/fixtures/hostile_rpc/snapshot.json from a live regtest daemon"]
async fn record_snapshot() {
    let phase = std::env::var("HOSTILE_RPC_RECORD").expect("HOSTILE_RPC_RECORD=main|forks");
    let url = std::env::var("MONERO_REGTEST_RPC").expect("MONERO_REGTEST_RPC");
    let node = LiveNode::new(&url);
    let info = json_rpc(&node, "get_info", json!({})).await;
    assert_eq!(
        info["nettype"], "fakechain",
        "refusing to record from a non-regtest daemon"
    );
    match phase.as_str() {
        "main" => record_main(&node).await,
        "forks" => record_forks(&node).await,
        other => panic!("unknown phase {other}"),
    }
}
