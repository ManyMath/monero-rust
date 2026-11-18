//! Harness comparing the crate with wallet2 on a live network, run by
//! `scripts/stagenet_compat.py` as `stagenet_tool <request.json> <response.json>`.
//! Only `relay` publishes anything.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use monero_rust::monero_rpc::{
    FeePriority, HttpRpc, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
};
use monero_rust::native::{
    broadcast_transaction_with_rpc, create_transaction_with_rpc_and_policy,
    sweep_all_with_rpc_and_policy, FeePolicy, TransactionResult,
};
use monero_rust::scanner::{
    fetch_blocks_batch, fetch_blocks_batch_with_history, process_fetched_batch_cached,
    CachedScanner, FetchedBlocks, Lookahead, SeedFormat,
};
use monero_rust::wallet_state::is_spendable;
use monero_rust::{
    derive_address, derive_subaddress, generate_seed, prepare_send_inputs_with_rpc,
    prepare_sweep_inputs_with_rpc, process_batch_with_reorg_detection, scan_mempool,
    BlockHashChain, Network, PendingSpend, ScanBatchOutcome, TrackedTransaction, TxStatus,
    WalletOutput, WalletState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, Clone, Default)]
struct SpentBy {
    tx: String,
    height: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct StateFile {
    restore_height: u64,
    lookahead_account: u32,
    lookahead_subaddress: u32,
    next_height: u64,
    daemon_height: u64,
    outputs: Vec<WalletOutput>,
    spent_by: HashMap<String, SpentBy>,
    block_hashes: BlockHashChain,
    pending_spends: HashMap<String, PendingSpend>,
    tracked: HashMap<String, TrackedTransaction>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    reorgs: Vec<String>,
}

type SharedReply = Arc<tokio::sync::OnceCell<Result<Vec<u8>, String>>>;

#[derive(Clone, Debug)]
struct SharedHttp {
    client: reqwest::Client,
    url: String,
    replies: Arc<Mutex<VecDeque<(Vec<u8>, SharedReply)>>>,
    keep: usize,
    downloaded: Arc<std::sync::atomic::AtomicU64>,
}

impl SharedHttp {
    fn new(url: String, keep: usize) -> Self {
        SharedHttp {
            client: reqwest::Client::new(),
            url,
            replies: Arc::new(Mutex::new(VecDeque::new())),
            keep,
            downloaded: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    async fn fetch(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, String> {
        let reply = self
            .client
            .post(self.url.clone() + "/" + route)
            .body(body)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .bytes()
            .await
            .map_err(|error| error.to_string())?
            .to_vec();
        self.downloaded
            .fetch_add(reply.len() as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(reply)
    }
}

#[async_trait]
impl RpcConnection for SharedHttp {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        if route != "getblocks.bin" {
            return self
                .fetch(route, body)
                .await
                .map_err(|_| RpcError::ConnectionError);
        }
        let cell = {
            let mut replies = self.replies.lock().unwrap();
            match replies.iter().find(|(request, _)| *request == body) {
                Some((_, cell)) => cell.clone(),
                None => {
                    let cell: SharedReply = Arc::new(tokio::sync::OnceCell::new());
                    replies.push_back((body.clone(), cell.clone()));
                    while replies.len() > self.keep {
                        replies.pop_front();
                    }
                    cell
                }
            }
        };
        cell.get_or_init(|| self.fetch(route, body))
            .await
            .clone()
            .map_err(|_| RpcError::ConnectionError)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn network_of(request: &Value) -> (Network, String) {
    let name = request["network"]
        .as_str()
        .map(str::to_string)
        .or_else(|| std::env::var("MONERO_STAGENET_NETWORK").ok())
        .unwrap_or_else(|| "stagenet".to_string());
    let network = match name.as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        other => panic!("unknown network {other}"),
    };
    (network, name)
}

fn rpc_of(request: &Value) -> Rpc<HttpRpc> {
    let url = request["rpc"]
        .as_str()
        .map(str::to_string)
        .or_else(|| std::env::var("MONERO_STAGENET_RPC").ok())
        .expect("rpc URL in request or MONERO_STAGENET_RPC");
    HttpRpc::new(url).expect("RPC client")
}

fn seed_of(request: &Value) -> String {
    let path = request["seed_file"].as_str().expect("seed_file");
    std::fs::read_to_string(path)
        .expect("read seed file")
        .trim()
        .to_string()
}

fn load_state(path: &str) -> StateFile {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read state file"))
        .expect("parse state file")
}

fn save_state(path: &str, state: &StateFile) {
    use std::io::Write;
    let tmp = format!("{path}.tmp");
    let mut file = std::fs::File::create(&tmp).expect("create state tmp");
    file.write_all(&serde_json::to_vec(state).expect("serialize state"))
        .expect("write state");
    file.sync_all().expect("sync state");
    std::fs::rename(&tmp, path).expect("rename state");
}

fn wallet_state_from(file: &StateFile) -> WalletState {
    let mut state = WalletState::new();
    state.replace_outputs(file.outputs.clone());
    state.block_hashes = file.block_hashes.clone();
    state.current_height = file.next_height.saturating_sub(1);
    state.daemon_height = file.daemon_height;
    state.restore_pending_spends(file.pending_spends.clone(), now_secs());
    state.restore_tracked_transactions(file.tracked.clone());
    state
}

fn store_wallet_state(file: &mut StateFile, state: &WalletState) {
    file.outputs = state.outputs().to_vec();
    file.block_hashes = state.block_hashes.clone();
    file.pending_spends = state.pending_spends().clone();
    file.tracked = state.tracked_transactions().clone();
}

fn priority_of(name: &str) -> FeePriority {
    match name {
        "automatic" => FeePriority::Automatic,
        "low" => FeePriority::Low,
        "normal" => FeePriority::Normal,
        "elevated" => FeePriority::Elevated,
        "priority" => FeePriority::Priority,
        other => panic!("unknown priority {other}"),
    }
}

fn op_addr(request: &Value) -> Value {
    let (network, _) = network_of(request);
    let seed = seed_of(request);
    let primary = derive_address(&seed, network, "").expect("address");
    let mut subaddresses = Vec::new();
    for pair in request["subaddresses"].as_array().unwrap_or(&Vec::new()) {
        let account = pair[0].as_u64().unwrap() as u32;
        let index = pair[1].as_u64().unwrap() as u32;
        let address = derive_subaddress(&seed, network, account, index, "").expect("subaddress");
        subaddresses.push(json!([account, index, address.to_string()]));
    }
    json!({ "primary": primary.to_string(), "subaddresses": subaddresses })
}

fn op_seed(request: &Value) -> Value {
    let path = request["seed_file"].as_str().expect("seed_file");
    assert!(
        !std::path::Path::new(path).exists(),
        "refusing to overwrite an existing seed file"
    );
    let seed = generate_seed(SeedFormat::Classic).expect("seed");
    std::fs::write(path, format!("{seed}\n")).expect("write seed");
    let (network, _) = network_of(request);
    let primary = derive_address(&seed, network, "").expect("address");
    json!({ "primary": primary.to_string() })
}

async fn op_scan(request: &Value) -> Value {
    let Some(wallets) = request["wallets"].as_array() else {
        return scan_one(rpc_of(request), request.clone()).await;
    };
    let url = request["rpc"]
        .as_str()
        .map(str::to_string)
        .or_else(|| std::env::var("MONERO_STAGENET_RPC").ok())
        .expect("rpc URL");
    let shared = SharedHttp::new(url, request["keep_batches"].as_u64().unwrap_or(12) as usize);
    let downloaded = shared.downloaded.clone();
    let rpc = Rpc::new_with_connection(shared);
    let mut tasks = Vec::new();
    for wallet in wallets {
        let mut one = request.clone();
        let fields = one.as_object_mut().expect("request object");
        fields.remove("wallets");
        for (key, value) in wallet.as_object().expect("wallet object") {
            fields.insert(key.clone(), value.clone());
        }
        let rpc = rpc.clone();
        let label = wallet["label"].as_str().expect("label").to_string();
        let response_path = wallet["response"].as_str().expect("response").to_string();
        tasks.push((
            label,
            tokio::spawn(async move {
                let response = scan_one(rpc, one).await;
                std::fs::write(
                    &response_path,
                    serde_json::to_string(&response).expect("serialize response"),
                )
                .expect("write response");
                json!({
                    "height": response["height"],
                    "seconds": response["seconds"],
                    "outputs": response["outputs"].as_array().map(Vec::len),
                })
            }),
        ));
    }
    let mut summary = serde_json::Map::new();
    for (label, task) in tasks {
        summary.insert(label, task.await.expect("scan task"));
    }
    json!({
        "wallets": summary,
        "bytes_downloaded": downloaded.load(std::sync::atomic::Ordering::Relaxed),
    })
}

async fn scan_one<R: RpcConnection + 'static>(rpc: Rpc<R>, request: Value) -> Value {
    let request = &request;
    let (network, _) = network_of(request);
    let seed = seed_of(request);
    let state_path = request["state"].as_str().expect("state");
    let mut file = if std::path::Path::new(state_path).exists() {
        load_state(state_path)
    } else {
        let restore_height = request["restore_height"].as_u64().unwrap_or(0);
        StateFile {
            restore_height,
            lookahead_account: request["lookahead_account"].as_u64().unwrap_or(0) as u32,
            lookahead_subaddress: request["lookahead_subaddress"].as_u64().unwrap_or(20) as u32,
            next_height: restore_height,
            ..StateFile::default()
        }
    };
    let lookahead = Lookahead {
        account: file.lookahead_account,
        subaddress: file.lookahead_subaddress,
    };
    let stop_height = request["stop_height"].as_u64();
    let save_every = request["save_every_blocks"].as_u64().unwrap_or(200_000);

    let mut state = wallet_state_from(&file);
    let genesis = hex::encode(rpc.get_block_hash(0).await.expect("genesis hash"));
    let genesis_first = request["genesis_first"].as_bool().unwrap_or(false);
    if file.next_height > 0 || genesis_first {
        state.record_block_hash(0, genesis.clone());
    }
    let mut own: HashSet<String> = state
        .outputs()
        .iter()
        .map(|output| output.key_image.clone())
        .collect();
    let mut cached: Option<CachedScanner> = None;

    let started = Instant::now();
    let start_height = file.next_height;
    let mut next = file.next_height;
    let mut batches = 0u64;
    let mut blocks = 0u64;
    let mut txs = 0u64;
    let mut fetch_secs = 0f64;
    let mut process_secs = 0f64;
    let mut new_outputs = 0usize;
    let mut newly_spent = 0usize;
    let mut last_save = next;
    let mut prefetch: Option<tokio::task::JoinHandle<Result<FetchedBlocks, String>>> = None;

    loop {
        let target = rpc.get_height().await.expect("get_height") as u64;
        let target = stop_height.map_or(target, |stop| stop.min(target));
        if next >= target {
            break;
        }
        while next < target {
            let knows_tip = next > 0 && state.block_hashes.get_hash(next - 1).is_some();
            let history_always = request["history_always"].as_bool().unwrap_or(false);
            let far_behind = !history_always && (target - next > 2_000 || !knows_tip);
            let fetch_started = Instant::now();
            let (fetched, batch_start) = if far_behind {
                let fetched = match prefetch.take() {
                    Some(handle) => handle.await.expect("join prefetch"),
                    None => fetch_blocks_batch(&rpc, next, false).await,
                }
                .expect("fetch batch");
                (fetched, next)
            } else {
                if let Some(handle) = prefetch.take() {
                    handle.abort();
                }
                let history = state.get_short_chain_history();
                let (fetched, actual_start) =
                    fetch_blocks_batch_with_history(&rpc, 0, &history, false)
                        .await
                        .expect("fetch batch with history");
                (fetched, actual_start)
            };
            fetch_secs += fetch_started.elapsed().as_secs_f64();
            let count = fetched.block_count() as u64;
            if count == 0 {
                break;
            }
            if far_behind && batch_start + count + 2_000 < target {
                let rpc = rpc.clone();
                let following = batch_start + count;
                prefetch = Some(tokio::spawn(async move {
                    fetch_blocks_batch(&rpc, following, false).await
                }));
            }

            let process_started = Instant::now();
            let (results, scanner) =
                process_fetched_batch_cached(fetched, &seed, network, lookahead, cached.take(), "")
                    .await
                    .expect("process batch");
            cached = Some(scanner);
            if let Some(first) = results.first() {
                assert!(
                    first.block_height <= next,
                    "gap in scan: batch starts at {} but the next unscanned height is {next}",
                    first.block_height
                );
            }

            match process_batch_with_reorg_detection(
                &results,
                &mut state,
                None,
                target,
                batch_start,
            )
            .expect("reorg detection")
            {
                ScanBatchOutcome::Reorg(info) => {
                    if let Some(handle) = prefetch.take() {
                        handle.abort();
                    }
                    file.reorgs.push(format!("{info:?}"));
                    if genesis_first && file.reorgs.len() >= 3 {
                        return json!({ "reorg_loop": true, "reorgs": file.reorgs });
                    }
                    file.spent_by
                        .retain(|_, spent| spent.height < info.split_height);
                    own = state
                        .outputs()
                        .iter()
                        .map(|output| output.key_image.clone())
                        .collect();
                    next = info.split_height;
                    continue;
                }
                ScanBatchOutcome::Normal(batch) => {
                    new_outputs += batch
                        .outputs_to_store
                        .iter()
                        .filter(|output| !own.contains(&output.key_image))
                        .count();
                    for output in &batch.outputs_to_store {
                        own.insert(output.key_image.clone());
                    }
                    state.add_outputs(batch.outputs_to_store);
                    for result in &results {
                        txs += result.tx_count as u64;
                        let (count, conflicts) = state.mark_spent_detecting_conflicts(
                            &result.spent_key_images,
                            &result.spent_key_image_tx_hashes,
                            result.block_height,
                        );
                        newly_spent += count;
                        for conflict in conflicts {
                            file.conflicts.push(format!("{conflict:?}"));
                        }
                        for (key_image, tx) in result
                            .spent_key_images
                            .iter()
                            .zip(&result.spent_key_image_tx_hashes)
                        {
                            if own.contains(key_image) {
                                state.confirm_pending_spend(key_image, result.block_height);
                                file.spent_by.insert(
                                    key_image.clone(),
                                    SpentBy {
                                        tx: tx.clone(),
                                        height: result.block_height,
                                    },
                                );
                            }
                        }
                        state.record_block_hash(result.block_height, result.block_hash.clone());
                    }
                    state.record_block_hash(0, genesis.clone());
                    state.block_hashes.compact();
                    let end = batch.batch_end_height;
                    blocks += end.saturating_sub(next);
                    next = next.max(end);
                    state.current_height = next.saturating_sub(1);
                    state.daemon_height = batch.daemon_height;
                    file.daemon_height = batch.daemon_height;
                }
            }
            process_secs += process_started.elapsed().as_secs_f64();
            batches += 1;
            if batches % 50 == 0 {
                eprintln!(
                    "scan {next}/{target} outputs={} elapsed={:.0}s",
                    state.outputs().len(),
                    started.elapsed().as_secs_f64()
                );
            }
            if next - last_save >= save_every {
                file.next_height = next;
                store_wallet_state(&mut file, &state);
                save_state(state_path, &file);
                last_save = next;
            }
        }
        if stop_height.is_some() {
            break;
        }
    }
    if let Some(handle) = prefetch.take() {
        handle.abort();
    }

    file.next_height = next;
    store_wallet_state(&mut file, &state);
    save_state(state_path, &file);

    let elapsed = started.elapsed().as_secs_f64();
    let height = next;
    let balance = state.balance_at_height(height);
    let outputs: Vec<Value> = state
        .outputs()
        .iter()
        .map(|output| {
            let spent_by = file.spent_by.get(&output.key_image);
            json!({
                "tx_hash": output.tx_hash,
                "output_index": output.output_index,
                "amount": output.amount,
                "key": output.key,
                "subaddress_index": output.subaddress_index,
                "global_index": output.global_index,
                "block_height": output.block_height,
                "spent": output.spent,
                "spent_height": output.spent_height,
                "spent_by_tx": spent_by.map(|spent| spent.tx.clone()),
                "key_image": output.key_image,
                "is_coinbase": output.is_coinbase,
                "payment_id": output.payment_id,
                "has_ring": output.ring.is_some(),
                "pending_spend": state.is_pending_spent(&output.key_image),
                "spendable": !output.spent && is_spendable(output, height),
            })
        })
        .collect();
    json!({
        "start_height": start_height,
        "height": height,
        "daemon_height": file.daemon_height,
        "batches": batches,
        "blocks": blocks,
        "transactions": txs,
        "seconds": elapsed,
        "fetch_seconds": fetch_secs,
        "process_seconds": process_secs,
        "blocks_per_second": if elapsed > 0.0 { blocks as f64 / elapsed } else { 0.0 },
        "new_outputs": new_outputs,
        "newly_spent": newly_spent,
        "balance": {
            "confirmed": balance.confirmed,
            "unconfirmed": balance.unconfirmed,
            "pending_spend": balance.pending_spend,
        },
        "conflicts": file.conflicts,
        "reorgs": file.reorgs,
        "outputs": outputs,
    })
}

async fn op_mempool(request: &Value) -> Value {
    let (network, _) = network_of(request);
    let rpc = rpc_of(request);
    let seed = seed_of(request);
    let lookahead = Lookahead {
        account: request["lookahead_account"].as_u64().unwrap_or(0) as u32,
        subaddress: request["lookahead_subaddress"].as_u64().unwrap_or(20) as u32,
    };
    match scan_mempool(&rpc, &seed, network, lookahead, None, "").await {
        Ok(result) => json!({
            "tx_count": result.tx_count,
            "outputs": result.outputs.iter().map(|output| json!({
                "tx_hash": output.tx_hash,
                "output_index": output.output_index,
                "amount": output.amount,
                "subaddress_index": output.subaddress_index,
                "global_index": output.global_index,
            })).collect::<Vec<_>>(),
            "spent_key_images": result.spent_key_images.len(),
        }),
        Err(error) => json!({ "error": error }),
    }
}

async fn op_fee(request: &Value) -> Value {
    let rpc = rpc_of(request);
    let wallet_height = request["wallet_height"].as_u64();
    let mut rows = Vec::new();
    for name in ["automatic", "low", "normal", "elevated", "priority"] {
        let unchecked = rpc
            .get_fee_for_priority_at_height(priority_of(name), wallet_height)
            .await
            .map(|fee| json!({ "per_weight": fee.per_weight, "mask": fee.mask }))
            .map_err(|error| format!("{error:?}"));
        let checked = rpc
            .get_fee_checked_for_priority_at_height(
                priority_of(name),
                wallet_height,
                DEFAULT_MAX_FEE_PER_BYTE,
            )
            .await
            .map(|fee| json!({ "per_weight": fee.per_weight, "mask": fee.mask }))
            .map_err(|error| format!("{error:?}"));
        rows.push(json!({ "priority": name, "unchecked": unchecked, "checked": checked }));
    }
    json!({ "default_max_fee_per_byte": DEFAULT_MAX_FEE_PER_BYTE, "priorities": rows })
}

fn recipients_of(request: &Value) -> Vec<(String, u64)> {
    request["recipients"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|pair| {
                    (
                        pair[0].as_str().expect("address").to_string(),
                        pair[1].as_u64().expect("amount"),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn op_build(request: &Value) -> Value {
    let (_, network_name) = network_of(request);
    let rpc = rpc_of(request);
    let seed = seed_of(request);
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let mut state = wallet_state_from(&file);
    let kind = request["kind"].as_str().unwrap_or("send");
    let source_account = request["source_account"].as_u64().unwrap_or(0) as u32;
    let recipients = recipients_of(request);
    let inputs: Option<Vec<String>> = request["inputs"].as_array().map(|list| {
        list.iter()
            .map(|key| key.as_str().expect("input key").to_string())
            .collect()
    });
    let repeat = request["repeat"].as_u64().unwrap_or(1) as usize;
    let record = request["record"].as_bool().unwrap_or(false);
    let fresh_rings = request["fresh_rings"].as_bool().unwrap_or(false);
    let keep_blobs = request["keep_blobs"].as_bool().unwrap_or(repeat <= 4);
    assert!(
        !(record && (repeat != 1 || fresh_rings)),
        "record needs one build with stored rings"
    );

    let height = file.next_height;
    let policy = FeePolicy {
        priority: priority_of(request["priority"].as_str().unwrap_or("automatic")),
        wallet_height: Some(height),
        max_fee_per_weight: request["max_fee_per_weight"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_FEE_PER_BYTE),
    };
    let ignore_pending = request["ignore_pending"].as_bool().unwrap_or(false);
    assert!(
        !(record && ignore_pending),
        "record cannot ignore pending spends"
    );
    let pending = if ignore_pending {
        HashSet::new()
    } else {
        state.pending_key_images()
    };
    let mut wallet_outputs: Vec<WalletOutput> = state.outputs().to_vec();
    if fresh_rings {
        for output in &mut wallet_outputs {
            output.ring = None;
        }
    }

    let prepared = match kind {
        "send" => {
            let total: u64 = recipients.iter().map(|recipient| recipient.1).sum();
            prepare_send_inputs_with_rpc(
                &rpc,
                &wallet_outputs,
                height,
                total,
                recipients.len(),
                inputs.as_deref(),
                Some(&pending),
                source_account,
                policy,
            )
            .await
        }
        "sweep" => {
            prepare_sweep_inputs_with_rpc(
                &rpc,
                &wallet_outputs,
                height,
                inputs.as_deref(),
                Some(&pending),
                source_account,
                policy,
            )
            .await
        }
        other => panic!("unknown kind {other}"),
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return json!({ "prepare_error": error }),
    };

    let mut builds = Vec::new();
    let mut errors = Vec::new();
    let mut first: Option<TransactionResult> = None;
    let build_started = Instant::now();
    for _ in 0..repeat {
        let built = match kind {
            "send" => {
                create_transaction_with_rpc_and_policy(
                    &rpc,
                    &seed,
                    &network_name,
                    prepared.stored_outputs.clone(),
                    &recipients,
                    "",
                    policy,
                )
                .await
            }
            _ => {
                sweep_all_with_rpc_and_policy(
                    &rpc,
                    &seed,
                    &network_name,
                    prepared.stored_outputs.clone(),
                    &recipients[0].0,
                    "",
                    policy,
                )
                .await
            }
        };
        match built {
            Ok(result) => {
                let rings: Vec<Value> = result
                    .input_rings
                    .iter()
                    .map(|ring| {
                        let mut absolute = Vec::with_capacity(ring.ring.offsets.len());
                        let mut index = 0u64;
                        for offset in &ring.ring.offsets {
                            index += offset;
                            absolute.push(index);
                        }
                        json!({
                            "key_image": ring.key_image,
                            "real_index": ring.ring.real_index,
                            "indexes": absolute,
                        })
                    })
                    .collect();
                builds.push(json!({
                    "tx_id": result.tx_id,
                    "fee": result.fee,
                    "blob_bytes": result.tx_blob.len() / 2,
                    "tx_blob": if keep_blobs { Value::String(result.tx_blob.clone()) } else { Value::Null },
                    "rings": rings,
                    "change_outputs": result.change_outputs.iter().map(|change| json!({
                        "output_index": change.output_index,
                        "amount": change.amount,
                        "subaddress_index": change.subaddress_index,
                    })).collect::<Vec<_>>(),
                }));
                if first.is_none() {
                    first = Some(result);
                }
            }
            Err(error) => errors.push(error),
        }
    }
    let build_seconds = build_started.elapsed().as_secs_f64();

    let mut recorded = false;
    if record {
        let result = first.as_ref().expect("a build to record");
        result
            .persist_input_rings(&mut state)
            .expect("record input rings");
        state.track_transaction(TrackedTransaction {
            tx_id: result.tx_id.clone(),
            status: TxStatus::Created,
            spent_key_images: result
                .input_rings
                .iter()
                .map(|ring| ring.key_image.clone())
                .collect(),
            spent_output_keys: prepared.spent_output_keys.clone(),
            change_outputs: Vec::new(),
            fee: result.fee,
            created_at_secs: now_secs(),
        });
        store_wallet_state(&mut file, &state);
        save_state(state_path, &file);
        recorded = true;
        let tx_path = request["tx_out"].as_str().expect("tx_out");
        std::fs::write(
            tx_path,
            serde_json::to_string_pretty(result).expect("serialize tx"),
        )
        .expect("write tx");
    }

    json!({
        "wallet_height": height,
        "selected_inputs": prepared.spent_output_keys,
        "input_total": prepared.total_input,
        "estimated_fee": prepared.estimated_fee,
        "builds": builds,
        "build_errors": errors,
        "build_seconds": build_seconds,
        "recorded": recorded,
    })
}

async fn op_relay(request: &Value) -> Value {
    let rpc = rpc_of(request);
    let state_path = request["state"].as_str().expect("state");
    let tx_path = request["tx"].as_str().expect("tx");
    let mut file = load_state(state_path);
    let mut state = wallet_state_from(&file);
    let result: TransactionResult =
        serde_json::from_str(&std::fs::read_to_string(tx_path).expect("read tx")).expect("tx");

    for ring in &result.input_rings {
        let stored = state
            .outputs()
            .iter()
            .find(|output| output.key_image == ring.key_image)
            .expect("input is in wallet state");
        assert!(
            stored.ring.as_ref() == Some(&ring.ring),
            "saved state does not hold the ring of {}",
            ring.key_image
        );
        assert!(!stored.spent, "input {} is already spent", ring.key_image);
        assert!(
            !state.is_pending_spent(&ring.key_image),
            "input {} is already pending in another relayed transaction",
            ring.key_image
        );
    }

    let outcome = broadcast_transaction_with_rpc(&rpc, &result, &mut state, false).await;
    let status = match &outcome {
        Ok(()) => "OK".to_string(),
        Err(error) => error.clone(),
    };
    if outcome.is_ok() {
        let spends = result
            .input_rings
            .iter()
            .map(|ring| {
                let output = state
                    .outputs()
                    .iter()
                    .find(|output| output.key_image == ring.key_image)
                    .expect("input");
                PendingSpend {
                    tx_id: result.tx_id.clone(),
                    key_image: ring.key_image.clone(),
                    output_key: output.output_key(),
                    amount: output.amount,
                    created_at_secs: now_secs(),
                }
            })
            .collect();
        state.add_pending_spends(&result.tx_id, spends);
        store_wallet_state(&mut file, &state);
        save_state(state_path, &file);
    }
    json!({ "tx_id": result.tx_id, "relay": status })
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let mut args = std::env::args().skip(1);
    let request_path = args.next().expect("request path");
    let response_path = args.next().expect("response path");
    let request: Value =
        serde_json::from_str(&std::fs::read_to_string(&request_path).expect("read request"))
            .expect("parse request");
    let response = match request["op"].as_str().expect("op") {
        "addr" => op_addr(&request),
        "seed" => op_seed(&request),
        "scan" => op_scan(&request).await,
        "fee" => op_fee(&request).await,
        "mempool" => op_mempool(&request).await,
        "build" => op_build(&request).await,
        "relay" => op_relay(&request).await,
        other => panic!("unknown op {other}"),
    };
    std::fs::write(
        &response_path,
        serde_json::to_string(&response).expect("serialize response"),
    )
    .expect("write response");
}
