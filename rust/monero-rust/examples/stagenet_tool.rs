//! Harness comparing the crate with wallet2 on a live network, run by
//! `scripts/stagenet_compat.py` as `stagenet_tool <request.json> <response.json>`.
//! Only `relay` and `relay-signed` publish anything.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE, edwards::CompressedEdwardsY, scalar::Scalar,
};
use monero_rust::crypto::{generate_key_image, hash_to_scalar};
use monero_rust::epee_compat::{self, ExportedTransferDetails, SignedTxSetKeyImagePair};
use monero_rust::key_image_import::{parse_rpc_export, SignedKeyImageImport};
use monero_rust::key_image_signing::{
    export_key_images_from_outputs, import_key_images_from_seed, import_signed_key_images_from_seed,
};
use monero_rust::monero_rpc::{
    FeePriority, HttpRpc, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
};
use monero_rust::native::{
    broadcast_raw_transaction_with_rpc, broadcast_transaction_with_rpc,
    create_transaction_with_rpc_and_policy, create_unsigned_transaction_from_prepared_inputs,
    preview_unsigned_transaction_from_seed, sign_unsigned_transaction,
    sweep_all_with_rpc_and_policy, DecoyPolicy, DecoySelection, FeePolicy, InputRing,
    OfflineSignResult, RingMember, TransactionResult, UnsignedTransactionResult,
};
use monero_rust::scanner::{
    fetch_blocks_batch, fetch_blocks_batch_with_history, genesis_block_hash,
    process_fetched_batch_cached, scan_pool_update, CachedScanner, FetchedBlocks, Lookahead,
    MempoolView, Refresh, SeedFormat, DEFAULT_LOOKAHEAD,
};
use monero_rust::wallet_state::is_spendable;
use monero_rust::{
    block_id_from_hex, derive_address, derive_keys, derive_subaddress, generate_seed,
    key_image_spent_status, parse_view_only_keys, prepare_send_inputs_with_rpc,
    prepare_sweep_inputs_with_rpc, process_batch_with_reorg_detection, scan_mempool,
    BlockHashChain, Network, PendingSpend, ScanBatchOutcome, TrackedTransaction, TxStatus,
    WalletOutput, WalletState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroizing;

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
    #[serde(default)]
    mempool: MempoolView,
    #[serde(default)]
    duplicate_keys: Vec<Value>,
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
    state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    state.replace_outputs(file.outputs.clone());
    state.block_hashes = file.block_hashes.clone();
    state.current_height = file.next_height.saturating_sub(1);
    state.daemon_height = file.daemon_height;
    state.restore_pending_spends(file.pending_spends.clone(), now_secs());
    state.restore_tracked_transactions(file.tracked.clone());
    state
}

struct FileRingPersister {
    path: std::path::PathBuf,
}

#[async_trait]
impl monero_rust::RingPersister for FileRingPersister {
    async fn persist(&mut self, rings: &monero_rust::RingDatabase) -> Result<(), String> {
        use std::io::Write;
        let bytes = serde_json::to_vec(rings).map_err(|error| error.to_string())?;
        let tmp = self.path.with_extension("tmp");
        let mut file = std::fs::File::create(&tmp).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|error| error.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::File::open(dir)
                .and_then(|dir| dir.sync_all())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn record_with_ring_db(request: &Value) -> bool {
    request["record"].as_bool().unwrap_or(false) && request["ring_db"].is_string()
}

fn wallet_state_with_rings(file: &StateFile, request: &Value) -> Result<WalletState, String> {
    let mut state = wallet_state_from(file);
    if let Some(path) = request["ring_db"].as_str() {
        let path = std::path::PathBuf::from(path);
        let stored: monero_rust::RingDatabase = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
        } else {
            monero_rust::RingDatabase::default()
        };
        state.restore_ring_database(stored)?;
        state.set_ring_persister(Box::new(FileRingPersister { path }));
    }
    Ok(state)
}

async fn save_wallet(
    path: &str,
    file: &mut StateFile,
    state: &mut WalletState,
) -> Result<(), String> {
    state.persist_rings().await?;
    store_wallet_state(file, state);
    save_state(path, file);
    Ok(())
}

fn ring_db_on_disk(path: &str) -> Result<monero_rust::RingDatabase, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|error| format!("{path}: {error}"))?)
        .map_err(|error| format!("{path}: {error}"))
}

/// Spent and pool status of `key_images`. Sends them to the node: ask only a node you run.
async fn node_guard<R: RpcConnection>(
    rpc: &Rpc<R>,
    key_images: &[String],
) -> Result<Value, String> {
    let status = key_image_spent_status(rpc, key_images).await?;
    let (_, pool_images) = rpc
        .get_transaction_pool()
        .await
        .map_err(|error| format!("pool: {error:?}"))?;
    let pool: HashSet<String> = pool_images.iter().map(hex::encode).collect();
    let rows: Vec<Value> = key_images
        .iter()
        .zip(&status)
        .map(|(key_image, status)| {
            json!({ "key_image": key_image, "spent_status": status, "in_pool": pool.contains(key_image) })
        })
        .collect();
    let clear = status.iter().all(|status| *status == 0)
        && key_images.iter().all(|key_image| !pool.contains(key_image));
    Ok(json!({ "clear": clear, "pool_key_images": pool.len(), "inputs": rows }))
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
        let default_window = request["lookahead_default"].as_bool().unwrap_or(false);
        StateFile {
            restore_height,
            lookahead_account: if default_window {
                DEFAULT_LOOKAHEAD.account
            } else {
                request["lookahead_account"].as_u64().unwrap_or(0) as u32
            },
            lookahead_subaddress: if default_window {
                DEFAULT_LOOKAHEAD.subaddress
            } else {
                request["lookahead_subaddress"].as_u64().unwrap_or(20) as u32
            },
            next_height: restore_height,
            ..StateFile::default()
        }
    };
    let lookahead = Lookahead {
        account: file.lookahead_account,
        subaddress: file.lookahead_subaddress,
    };
    let stop_height = request["stop_height"].as_u64();
    let prune = request["prune"].as_bool().unwrap_or(false);
    let save_every = request["save_every_blocks"].as_u64().unwrap_or(200_000);

    let mut state = match wallet_state_with_rings(&file, request) {
        Ok(state) => state,
        Err(error) => return json!({ "error": format!("ring database: {error}") }),
    };
    let refresh_mode = !request["legacy_fetch"].as_bool().unwrap_or(false);
    let genesis = if refresh_mode {
        genesis_block_hash(network)
    } else {
        rpc.get_block_hash(0).await.expect("genesis hash")
    };
    let genesis_first = request["genesis_first"].as_bool().unwrap_or(false);
    if file.next_height > 0 || genesis_first {
        state.record_block_hash(0, genesis);
    }
    let mut own: HashSet<String> = state
        .outputs()
        .iter()
        .filter(|output| !output.key_image.is_empty())
        .map(|output| output.key_image.clone())
        .collect();
    let mut seen_keys: HashMap<String, (String, u64, u64)> = state
        .outputs()
        .iter()
        .map(|output| {
            (
                output.key.clone(),
                (output.output_key(), output.amount, output.block_height),
            )
        })
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

    if refresh_mode {
        let mut view = std::mem::take(&mut file.mempool);
        let mut pool = None;
        let mut refresh = Refresh::new(&state, network, file.restore_height)
            .expect("refresh")
            .with_pool(&view);
        refresh
            .fetch_block_ids(&rpc, &mut state)
            .await
            .expect("fetch block ids");
        loop {
            let fetch_started = Instant::now();
            let Some(mut fetched) = (match refresh.next_batch(&rpc).await {
                Ok(fetched) => fetched,
                Err(error) => {
                    file.next_height = next;
                    save_wallet(state_path, &mut file, &mut state)
                        .await
                        .expect("save wallet");
                    return json!({
                        "error": format!("{}: {error}", "fetch batch"),
                        "start_height": start_height,
                        "height": next,
                        "balance_at_error": state.balance_at_height(next).confirmed,
                        "outputs_at_error": state.outputs().len(),
                        "seconds": started.elapsed().as_secs_f64(),
                    });
                }
            }) else {
                break;
            };
            fetch_secs += fetch_started.elapsed().as_secs_f64();
            if let Some(update) = fetched.take_pool() {
                pool = Some(update);
            }
            if fetched.is_empty() {
                continue;
            }
            let batch_start = fetched.start_height().expect("start height");
            let daemon_height = fetched.daemon_height();
            let process_started = Instant::now();
            let (mut results, scanner) = match process_fetched_batch_cached(
                fetched,
                &seed,
                network,
                lookahead,
                cached.take(),
                "",
            )
            .await
            {
                Ok(processed) => processed,
                Err(error) => {
                    file.next_height = next;
                    save_wallet(state_path, &mut file, &mut state)
                        .await
                        .expect("save wallet");
                    return json!({
                        "error": format!("{}: {error}", "process batch"),
                        "start_height": start_height,
                        "height": next,
                        "balance_at_error": state.balance_at_height(next).confirmed,
                        "outputs_at_error": state.outputs().len(),
                        "seconds": started.elapsed().as_secs_f64(),
                    });
                }
            };
            cached = Some(scanner);
            if let Some(stop) = stop_height {
                results.retain(|result| result.block_height < stop);
                if results.is_empty() {
                    break;
                }
            }
            if let Some(first) = results.first() {
                assert!(
                    first.block_height <= next,
                    "gap in scan: batch starts at {} but the next unscanned height is {next}",
                    first.block_height
                );
            }
            match match process_batch_with_reorg_detection(
                &results,
                &mut state,
                None,
                daemon_height,
                batch_start,
            ) {
                Ok(outcome) => outcome,
                Err(error) => {
                    file.next_height = next;
                    save_wallet(state_path, &mut file, &mut state)
                        .await
                        .expect("save wallet");
                    return json!({
                        "error": format!("{}: {error}", "reorg detection"),
                        "start_height": start_height,
                        "height": next,
                        "balance_at_error": state.balance_at_height(next).confirmed,
                        "outputs_at_error": state.outputs().len(),
                        "seconds": started.elapsed().as_secs_f64(),
                    });
                }
            } {
                ScanBatchOutcome::Reorg(info) => {
                    file.reorgs.push(format!("{info:?}"));
                    file.spent_by
                        .retain(|_, spent| spent.height < info.split_height);
                    own = state
                        .outputs()
                        .iter()
                        .filter(|output| !output.key_image.is_empty())
                        .map(|output| output.key_image.clone())
                        .collect();
                    next = info.split_height;
                    refresh = Refresh::new(&state, network, file.restore_height).expect("refresh");
                    refresh
                        .fetch_block_ids(&rpc, &mut state)
                        .await
                        .expect("fetch block ids");
                }
                ScanBatchOutcome::Normal(batch) => {
                    for output in &batch.outputs_to_store {
                        if !output.key_image.is_empty() {
                            own.insert(output.key_image.clone());
                        }
                        let id = output.output_key();
                        match seen_keys.get(&output.key) {
                            Some(first) if first.0 != id => file.duplicate_keys.push(json!({
                                "key": output.key,
                                "first": first.0,
                                "first_amount": first.1,
                                "first_height": first.2,
                                "second": id,
                                "second_amount": output.amount,
                                "second_height": output.block_height,
                            })),
                            Some(_) => {}
                            None => {
                                seen_keys.insert(
                                    output.key.clone(),
                                    (id, output.amount, output.block_height),
                                );
                            }
                        }
                    }
                    let held = state.outputs().len();
                    let spent_before = state.outputs().iter().filter(|o| o.spent).count();
                    for conflict in monero_rust::apply_processed_batch(&mut state, &batch) {
                        file.conflicts.push(format!("{conflict:?}"));
                    }
                    new_outputs += state.outputs().len().saturating_sub(held);
                    newly_spent += state
                        .outputs()
                        .iter()
                        .filter(|o| o.spent)
                        .count()
                        .saturating_sub(spent_before);
                    for result in &results {
                        txs += result.tx_count as u64;
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
                        state.record_block_hash(
                            result.block_height,
                            block_id_from_hex(&result.block_hash).expect("block id"),
                        );
                    }
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
        }
        if let Some(update) = pool {
            let scanned = scan_pool_update(update, &mut view, &seed, network, lookahead, None, "")
                .expect("pool update");
            state.add_pool_spends(&scanned, now_secs());
            state.add_outputs(scanned.outputs);
            state.retain_pool_outputs(|tx| view.contains(tx));
        }
        file.mempool = view;
    } else {
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
                        None => fetch_blocks_batch(&rpc, next, prune).await,
                    }
                    .expect("fetch batch");
                    (fetched, next)
                } else {
                    if let Some(handle) = prefetch.take() {
                        handle.abort();
                    }
                    let history = state.get_short_chain_history();
                    let (fetched, actual_start) =
                        fetch_blocks_batch_with_history(&rpc, 0, &history, prune)
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
                        fetch_blocks_batch(&rpc, following, prune).await
                    }));
                }

                let process_started = Instant::now();
                let (mut results, scanner) = process_fetched_batch_cached(
                    fetched,
                    &seed,
                    network,
                    lookahead,
                    cached.take(),
                    "",
                )
                .await
                .expect("process batch");
                cached = Some(scanner);
                if let Some(stop) = stop_height {
                    results.retain(|result| result.block_height < stop);
                    if results.is_empty() {
                        break;
                    }
                }
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
                            .filter(|output| !output.key_image.is_empty())
                            .map(|output| output.key_image.clone())
                            .collect();
                        next = info.split_height;
                        continue;
                    }
                    ScanBatchOutcome::Normal(batch) => {
                        for output in &batch.outputs_to_store {
                            if !output.key_image.is_empty() {
                                own.insert(output.key_image.clone());
                            }
                            let id = output.output_key();
                            match seen_keys.get(&output.key) {
                                Some(first) if first.0 != id => file.duplicate_keys.push(json!({
                                    "key": output.key,
                                    "first": first.0,
                                    "first_amount": first.1,
                                    "first_height": first.2,
                                    "second": id,
                                    "second_amount": output.amount,
                                    "second_height": output.block_height,
                                })),
                                Some(_) => {}
                                None => {
                                    seen_keys.insert(
                                        output.key.clone(),
                                        (id, output.amount, output.block_height),
                                    );
                                }
                            }
                        }
                        let held = state.outputs().len();
                        state.add_outputs(batch.outputs_to_store);
                        new_outputs += state.outputs().len().saturating_sub(held);
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
                            state.record_block_hash(
                                result.block_height,
                                block_id_from_hex(&result.block_hash).expect("block id"),
                            );
                        }
                        state.record_block_hash(0, genesis);
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
                    save_wallet(state_path, &mut file, &mut state)
                        .await
                        .expect("save wallet");
                    last_save = next;
                }
            }
            if stop_height.is_some() {
                break;
            }
        }
    }
    if let Some(handle) = prefetch.take() {
        handle.abort();
    }

    if request["compact"].as_bool().unwrap_or(false) {
        state.block_hashes.compact();
    }
    file.next_height = next;
    save_wallet(state_path, &mut file, &mut state)
        .await
        .expect("save wallet");

    let elapsed = started.elapsed().as_secs_f64();
    let height = next;
    let balance = state.balance_at_height(height);
    let outputs = outputs_json(&state, &file, height);
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
            "pre_ringct": balance.pre_ringct,
        },
        "conflicts": file.conflicts,
        "reorgs": file.reorgs,
        "duplicate_one_time_keys": file.duplicate_keys,
        "rings": state
            .ring_database()
            .rings()
            .map(|(key_image, ring)| (key_image.to_string(), json!(ring.indexes().ok())))
            .collect::<serde_json::Map<_, _>>(),
        "stored_block_hashes": state.block_hashes.len(),
        "outputs": outputs,
    })
}

fn outputs_json(state: &WalletState, file: &StateFile, height: u64) -> Vec<Value> {
    use monero_wallet::transaction::Timelock;
    state
        .outputs()
        .iter()
        .map(|output| {
            let spent_by = file.spent_by.get(&output.key_image);
            let unlock_time = match output.additional_timelock() {
                Timelock::None => 0,
                Timelock::Block(block) => block as u64,
                Timelock::Time(time) => time,
            };
            json!({
                "tx_hash": output.tx_hash,
                "output_index": output.output_index,
                "amount": output.amount,
                "key": output.key,
                "subaddress_index": output.subaddress_index,
                "global_index": output.global_index,
                "block_height": output.block_height,
                "unlock_time": unlock_time,
                "spent": output.spent,
                "spent_height": output.spent_height,
                "spent_by_tx": spent_by.map(|spent| spent.tx.clone()),
                "key_image": output.key_image,
                "is_coinbase": output.is_coinbase,
                "pre_ringct": output.pre_ringct,
                "payment_id": output.payment_id,
                "has_ring": output.ring.is_some(),
                "ring": output.ring.as_ref().and_then(|ring| ring.indexes().ok()),
                "pending_spend": !output.key_image.is_empty()
                    && state.is_pending_spent(&output.key_image),
                "spendable": !output.spent && is_spendable(output, height),
            })
        })
        .collect()
}

fn export_order(state: &WalletState) -> Vec<WalletOutput> {
    let mut outputs: Vec<WalletOutput> = state
        .outputs()
        .iter()
        .filter(|output| output.block_height != 0)
        .cloned()
        .collect();
    outputs.sort_by_key(|output| output.block_height);
    outputs
}

fn op_ki_export(request: &Value) -> Value {
    let seed = seed_of(request);
    let file = load_state(request["state"].as_str().expect("state"));
    let state = wallet_state_from(&file);
    let mut outputs = export_order(&state);
    let swap = request["swap"].as_array().map(|pair| {
        (
            pair[0].as_u64().expect("swap index") as usize,
            pair[1].as_u64().expect("swap index") as usize,
        )
    });
    let started = Instant::now();
    let bytes = if request["legacy"].as_bool().unwrap_or(false) {
        let mut list: Vec<epee_compat::ExportedKeyImage> = outputs
            .iter()
            .map(|output| epee_compat::ExportedKeyImage {
                key_image: output.key_image.clone(),
                tx_hash: output.tx_hash.clone(),
                output_index: u8::try_from(output.output_index).expect("output index fits a byte"),
            })
            .collect();
        if let Some((a, b)) = swap {
            list.swap(a, b);
        }
        #[allow(deprecated)]
        epee_compat::export_key_images(&list)
    } else {
        if let Some((a, b)) = swap {
            outputs.swap(a, b);
        }
        export_key_images_from_outputs(&seed, "", &outputs)
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    std::fs::write(request["out"].as_str().expect("out"), &bytes).expect("write export");
    json!({
        "ok": true,
        "count": outputs.len(),
        "bytes": bytes.len(),
        "seconds": started.elapsed().as_secs_f64(),
        "swap": request["swap"],
    })
}

fn op_ki_read(request: &Value) -> Value {
    let seed = seed_of(request);
    let data = std::fs::read(request["file"].as_str().expect("file")).expect("read export");
    let parsed = match request["format"].as_str().unwrap_or("signed") {
        "signed" => import_signed_key_images_from_seed(&seed, "", &data),
        "rpc-json" => std::str::from_utf8(&data)
            .map_err(|error| error.to_string())
            .and_then(parse_rpc_export)
            .map(|(offset, key_images)| SignedKeyImageImport {
                offset: offset as u32,
                key_images,
            }),
        other => panic!("unknown key image format {other}"),
    };
    match parsed {
        Ok(import) => json!({
            "ok": true,
            "offset": import.offset,
            "count": import.key_images.len(),
            "key_images": import.key_images_hex(),
            "signatures": import
                .key_images
                .iter()
                .map(|entry| hex::encode(entry.signature))
                .collect::<Vec<_>>(),
        }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

async fn op_ki_import(request: &Value) -> Value {
    let seed = seed_of(request);
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let mut state = wallet_state_from(&file);
    let data = std::fs::read(request["file"].as_str().expect("file")).expect("read export");
    let started = Instant::now();
    let assigned = match request["format"].as_str().unwrap_or("signed") {
        "signed" => import_signed_key_images_from_seed(&seed, "", &data)
            .and_then(|import| state.assign_verified_key_images_in_export_order(&import)),
        "rpc-json" => std::str::from_utf8(&data)
            .map_err(|error| error.to_string())
            .and_then(parse_rpc_export)
            .and_then(|(offset, key_images)| {
                state.assign_verified_key_images_in_export_order(&SignedKeyImageImport {
                    offset: offset as u32,
                    key_images,
                })
            }),
        "unsigned" => import_key_images_from_seed(&seed, "", &data)
            .and_then(|list| state.assign_key_images_in_export_order(&list)),
        other => panic!("unknown key image format {other}"),
    };
    let import_seconds = started.elapsed().as_secs_f64();
    let assigned = match assigned {
        Ok(assigned) => assigned,
        Err(error) => {
            return json!({ "ok": false, "error": error, "import_seconds": import_seconds })
        }
    };

    let mut newly_spent = 0;
    let mut status_seconds = 0.0;
    let mut pool_spent = 0;
    let mut status_requests = 0;
    if request["mark_spent"].as_bool().unwrap_or(true) {
        let rpc = rpc_of(request).for_wallet(&state);
        let key_images: Vec<String> = state
            .outputs()
            .iter()
            .filter(|output| !output.key_image.is_empty())
            .map(|output| output.key_image.clone())
            .collect();
        let status_started = Instant::now();
        let chunk = request["status_chunk"].as_u64().unwrap_or(0) as usize;
        let chunk = if chunk == 0 {
            key_images.len().max(1)
        } else {
            chunk
        };
        let mut statuses = Vec::with_capacity(key_images.len());
        for part in key_images.chunks(chunk) {
            status_requests += 1;
            match key_image_spent_status(&rpc, part).await {
                Ok(part_statuses) => statuses.extend(part_statuses),
                Err(error) => {
                    return json!({
                        "ok": false,
                        "stage": "spent status",
                        "error": error,
                        "key_images": key_images.len(),
                        "status_requests": status_requests,
                    })
                }
            }
        }
        status_seconds = status_started.elapsed().as_secs_f64();
        pool_spent = statuses.iter().filter(|status| **status == 2).count();
        let spent: Vec<String> = key_images
            .into_iter()
            .zip(&statuses)
            .filter(|(_, status)| **status != 0)
            .map(|(key_image, _)| key_image)
            .collect();
        newly_spent = state.mark_spent_by_key_images(&spent);
    }
    if request["save"].as_bool().unwrap_or(true) {
        store_wallet_state(&mut file, &state);
        save_state(state_path, &file);
    }
    let height = file.next_height;
    let balance = state.balance_at_height(height);
    let (mut spent_amount, mut unspent_amount, mut spent_count, mut unspent_count) =
        (0u128, 0u128, 0usize, 0usize);
    for output in state.outputs() {
        if output.spent {
            spent_amount += u128::from(output.amount);
            spent_count += 1;
        } else {
            unspent_amount += u128::from(output.amount);
            unspent_count += 1;
        }
    }
    json!({
        "ok": true,
        "assigned": assigned,
        "import_seconds": import_seconds,
        "status_seconds": status_seconds,
        "status_requests": status_requests,
        "newly_spent": newly_spent,
        "spent_in_pool": pool_spent,
        "height": height,
        "spent_amount": spent_amount.to_string(),
        "unspent_amount": unspent_amount.to_string(),
        "spent_count": spent_count,
        "unspent_count": unspent_count,
        "balance": {
            "confirmed": balance.confirmed,
            "unconfirmed": balance.unconfirmed,
            "pending_spend": balance.pending_spend,
            "pre_ringct": balance.pre_ringct,
        },
        "outputs": outputs_json(&state, &file, height),
    })
}

async fn op_unsigned_build(request: &Value) -> Value {
    let (network, network_name) = network_of(request);
    let rpc = rpc_of(request);
    let seed = seed_of(request);
    let (view_hex, spend_hex) = seed
        .strip_prefix("viewonly:")
        .and_then(|rest| rest.split_once(':'))
        .expect("a view-only seed file");
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let mut state = match wallet_state_with_rings(&file, request) {
        Ok(state) => state,
        Err(error) => return json!({ "build_error": error }),
    };
    let rpc = rpc.for_wallet(&state);
    let source_account = request["source_account"].as_u64().unwrap_or(0) as u32;
    let recipients = recipients_of(request);
    let inputs: Option<Vec<String>> = request["inputs"].as_array().map(|list| {
        list.iter()
            .map(|key| key.as_str().expect("input key").to_string())
            .collect()
    });
    let height = file.next_height;
    let policy = FeePolicy {
        priority: priority_of(request["priority"].as_str().unwrap_or("automatic")),
        wallet_height: Some(height),
        max_fee_per_weight: request["max_fee_per_weight"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_FEE_PER_BYTE),
    };
    let pending = state.pending_key_images();
    let wallet_outputs: Vec<WalletOutput> = state.outputs().to_vec();
    let total: u64 = recipients.iter().map(|recipient| recipient.1).sum();
    let prepared = match prepare_send_inputs_with_rpc(
        &rpc,
        &wallet_outputs,
        height,
        total,
        recipients.len(),
        pays_integrated(network, &recipients),
        inputs.as_deref(),
        Some(&pending),
        source_account,
        policy,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(error) => return json!({ "prepare_error": error }),
    };
    let result: UnsignedTransactionResult = match create_unsigned_transaction_from_prepared_inputs(
        &rpc,
        view_hex,
        spend_hex,
        &network_name,
        &prepared,
        &recipients,
        policy,
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            return json!({ "build_error": error, "selected_inputs": prepared.spent_output_keys })
        }
    };
    store_wallet_state(&mut file, &state);
    save_state(state_path, &file);
    std::fs::write(
        request["out"].as_str().expect("out"),
        serde_json::to_string_pretty(&result).expect("serialize unsigned"),
    )
    .expect("write unsigned");
    json!({
        "wallet_height": height,
        "selected_inputs": prepared.spent_output_keys,
        "further_inputs": prepared.further_inputs.iter().map(|input| format!("{}:{}", input.tx_hash, input.output_index)).collect::<Vec<_>>(),
        "input_total": prepared.total_input,
        "estimated_fee": prepared.estimated_fee,
        "fee": result.fee,
        "unsigned_bytes": result.unsigned_tx_hex.len() / 2,
        "rings": rings_json(&result.input_rings),
    })
}

fn rings_json(rings: &[InputRing]) -> Vec<Value> {
    rings
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
        .collect()
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn exported_key_image(
    transfer: &ExportedTransferDetails,
    spend: &Scalar,
    view: &[u8; 32],
) -> Option<[u8; 32]> {
    let view_scalar = Scalar::from_bytes_mod_order(*view);
    let mut subaddress = Scalar::ZERO;
    if transfer.subaddress_major != 0 || transfer.subaddress_minor != 0 {
        let mut material = b"SubAddr\0".to_vec();
        material.extend_from_slice(view);
        material.extend_from_slice(&(transfer.subaddress_major as u32).to_le_bytes());
        material.extend_from_slice(&(transfer.subaddress_minor as u32).to_le_bytes());
        subaddress = hash_to_scalar(&material);
    }
    let mut candidates = vec![transfer.tx_public_key];
    candidates.extend(transfer.additional_tx_keys.iter().copied());
    for tx_key in candidates {
        let Some(point) = CompressedEdwardsY(tx_key).decompress() else {
            continue;
        };
        let mut material = (view_scalar * point)
            .mul_by_cofactor()
            .compress()
            .to_bytes()
            .to_vec();
        varint(transfer.internal_output_index, &mut material);
        let secret = Zeroizing::new(spend + hash_to_scalar(&material) + subaddress);
        let public = (&*secret * ED25519_BASEPOINT_TABLE).compress().to_bytes();
        if public == transfer.output_public_key {
            return Some(generate_key_image(&secret).compress().to_bytes());
        }
    }
    None
}

fn op_sign(request: &Value) -> Value {
    let (network, network_name) = network_of(request);
    let seed = seed_of(request);
    let raw = std::fs::read(request["unsigned"].as_str().expect("unsigned")).expect("read");
    let is_wallet2 = raw.starts_with(epee_compat::UNSIGNED_TX_MAGIC);
    let unsigned_hex = if is_wallet2 {
        hex::encode(&raw)
    } else {
        let result: UnsignedTransactionResult =
            serde_json::from_slice(&raw).expect("unsigned transaction file");
        result.unsigned_tx_hex
    };
    let preview =
        match preview_unsigned_transaction_from_seed(&seed, &unsigned_hex, &network_name, "") {
            Ok(preview) => preview,
            Err(error) => return json!({ "ok": false, "stage": "preview", "error": error }),
        };
    let approval = match preview.approve(
        request["max_fee"].as_u64().expect("max_fee"),
        request["max_wallet_debit"]
            .as_u64()
            .expect("max_wallet_debit"),
    ) {
        Ok(approval) => approval,
        Err(error) => {
            return json!({ "ok": false, "stage": "approve", "error": error, "preview": preview })
        }
    };
    let signed: OfflineSignResult =
        match sign_unsigned_transaction(&seed, &unsigned_hex, &network_name, "", &approval) {
            Ok(signed) => signed,
            Err(error) => {
                return json!({ "ok": false, "stage": "sign", "error": error, "preview": preview })
            }
        };
    std::fs::write(
        request["out"].as_str().expect("out"),
        serde_json::to_string_pretty(&signed).expect("serialize signed"),
    )
    .expect("write signed");

    let mut txset = Value::Null;
    if is_wallet2 {
        let keys = derive_keys(&seed, network, "").expect("keys");
        let spend = Scalar::from_bytes_mod_order(*keys.secret_spend_key.expect("spend key"));
        let view: [u8; 32] = *keys.secret_view_key;
        let export = std::fs::read(
            request["wallet2_outputs"]
                .as_str()
                .expect("wallet2_outputs"),
        )
        .expect("read output export");
        let parsed = match epee_compat::parse_output_export(&export, &view) {
            Ok(parsed) => parsed,
            Err(error) => return json!({ "ok": false, "stage": "output export", "error": error }),
        };
        let key_images: Vec<[u8; 32]> = parsed
            .outputs
            .iter()
            .enumerate()
            .map(|(index, transfer)| {
                exported_key_image(transfer, &spend, &view)
                    .unwrap_or_else(|| panic!("exported output {index} is not this wallet's"))
            })
            .collect();
        let mut tx_key_images = Vec::new();
        for tx in &signed.transactions {
            for output in &tx.change_outputs {
                tx_key_images.push(SignedTxSetKeyImagePair {
                    public_key: hex::decode(&output.key).unwrap().try_into().unwrap(),
                    key_image: hex::decode(&output.key_image).unwrap().try_into().unwrap(),
                });
            }
        }
        let blobs: Vec<Vec<u8>> = signed
            .transactions
            .iter()
            .map(|tx| hex::decode(&tx.tx_blob).unwrap())
            .collect();
        match epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
            unsigned_txset: &raw,
            view_secret_key: &view,
            tx_blobs: blobs.iter().map(Vec::as_slice).collect(),
            key_images: &key_images,
            tx_key_images: &tx_key_images,
        }) {
            Ok(bytes) => {
                std::fs::write(request["txset_out"].as_str().expect("txset_out"), &bytes)
                    .expect("write signed txset");
                txset = json!({ "bytes": bytes.len(), "key_images": key_images.len() });
            }
            Err(error) => return json!({ "ok": false, "stage": "signed txset", "error": error }),
        }
    }
    json!({
        "ok": true,
        "wallet2_txset": is_wallet2,
        "preview": preview,
        "tx_ids": signed.transactions.iter().map(|tx| tx.tx_id.clone()).collect::<Vec<_>>(),
        "fees": signed.transactions.iter().map(|tx| tx.fee).collect::<Vec<_>>(),
        "tx_blobs": signed.transactions.iter().map(|tx| tx.tx_blob.clone()).collect::<Vec<_>>(),
        "spent_key_images": signed.transactions.iter().map(|tx| tx.spent_key_images.clone()).collect::<Vec<_>>(),
        "change_outputs": signed.transactions.iter().map(|tx| tx.change_outputs.iter().map(|change| json!({
            "output_index": change.output_index,
            "amount": change.amount,
            "subaddress_index": change.subaddress_index,
        })).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "signed_txset": txset,
    })
}

fn op_txset_check(request: &Value) -> Value {
    let (network, _) = network_of(request);
    let seed = seed_of(request);
    let view: [u8; 32] = match parse_view_only_keys(&seed) {
        Some((view, _)) => view.to_bytes(),
        None => {
            *derive_keys(&seed, network, "")
                .expect("keys")
                .secret_view_key
        }
    };
    let signed = std::fs::read(request["signed"].as_str().expect("signed")).expect("read signed");
    let unsigned = std::fs::read(request["unsigned"].as_str().expect("unsigned")).expect("read");
    match epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
        &signed, &view, &unsigned,
    ) {
        Ok(txs) => json!({
            "ok": true,
            "tx_ids": txs.iter().map(|tx| hex::encode(tx.tx_hash)).collect::<Vec<_>>(),
        }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

async fn op_record_rings(request: &Value) -> Value {
    let (network, _) = network_of(request);
    let seed = seed_of(request);
    let view: [u8; 32] = match parse_view_only_keys(&seed) {
        Some((view, _)) => view.to_bytes(),
        None => {
            *derive_keys(&seed, network, "")
                .expect("keys")
                .secret_view_key
        }
    };
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let mut state = match wallet_state_with_rings(&file, request) {
        Ok(state) => state,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let data = std::fs::read(request["unsigned"].as_str().expect("unsigned")).expect("read");
    let summary = match epee_compat::parse_unsigned_monero_txset_summary(&data, &view) {
        Ok(summary) => summary,
        Err(error) => return json!({ "ok": false, "error": error }),
    };
    let mut rings = Vec::new();
    for tx in &summary.txes {
        for source in &tx.sources {
            let real = &source.ring[source.real_output as usize];
            let key = hex::encode(real.output_public_key);
            let Some(output) = state.outputs().iter().find(|output| {
                output.global_index == Some(real.global_output_index) && output.key == key
            }) else {
                return json!({ "ok": false, "error": format!("source {key} is not in the wallet state") });
            };
            if output.key_image.is_empty() {
                return json!({ "ok": false, "error": format!("source {key} has no key image yet") });
            }
            let mut offsets = Vec::with_capacity(source.ring.len());
            let mut previous = 0u64;
            for member in &source.ring {
                offsets.push(member.global_output_index - previous);
                previous = member.global_output_index;
            }
            rings.push(InputRing {
                key_image: output.key_image.clone(),
                ring: DecoySelection {
                    real_index: source.real_output as u8,
                    offsets,
                    ring: source
                        .ring
                        .iter()
                        .map(|member| RingMember {
                            key: hex::encode(member.output_public_key),
                            commitment: hex::encode(member.commitment),
                        })
                        .collect(),
                },
            });
        }
    }
    if let Err(error) = state.record_input_rings(&rings) {
        return json!({ "ok": false, "error": error });
    }
    if let Err(error) = state.persist_rings().await {
        return json!({ "ok": false, "error": error });
    }
    store_wallet_state(&mut file, &state);
    save_state(state_path, &file);
    json!({ "ok": true, "transactions": summary.txes.len(), "rings": rings_json(&rings) })
}

async fn op_relay_signed(request: &Value) -> Value {
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let mut state = match wallet_state_with_rings(&file, request) {
        Ok(state) => state,
        Err(error) => return json!({ "relay": error }),
    };
    let signed: OfflineSignResult = serde_json::from_str(
        &std::fs::read_to_string(request["signed"].as_str().expect("signed")).expect("read"),
    )
    .expect("signed transaction file");
    let tx = &signed.transactions[request["index"].as_u64().unwrap_or(0) as usize];
    let mut spends = Vec::new();
    for key_image in &tx.spent_key_images {
        let Some(output) = state
            .outputs()
            .iter()
            .find(|output| &output.key_image == key_image)
        else {
            return json!({ "relay": format!("input {key_image} is not in the wallet state") });
        };
        if state.ring_database().ring(key_image).is_none() {
            return json!({ "relay": format!("no ring is recorded for input {key_image}") });
        }
        if output.spent || state.is_pending_spent(key_image) {
            return json!({ "relay": format!("input {key_image} is already spent or pending") });
        }
        spends.push(PendingSpend {
            tx_id: tx.tx_id.clone(),
            key_image: key_image.clone(),
            output_key: output.output_key(),
            amount: output.amount,
            created_at_secs: now_secs(),
        });
    }
    let status = if request["broadcast"].as_bool().unwrap_or(true) {
        let rpc = rpc_of(request);
        match broadcast_raw_transaction_with_rpc(&rpc, &tx.tx_blob, &mut state, false).await {
            Ok(()) => "OK".to_string(),
            Err(error) => error,
        }
    } else {
        let blob = hex::decode(&tx.tx_blob).expect("signed transaction hex");
        match state.record_transaction_rings(&blob) {
            Ok(_) => match state.persist_rings().await {
                Ok(()) => "not broadcast here".to_string(),
                Err(error) => error,
            },
            Err(error) => error,
        }
    };
    if status == "OK" || status == "not broadcast here" {
        state.add_pending_spends(&tx.tx_id, spends);
        store_wallet_state(&mut file, &state);
        save_state(state_path, &file);
    }
    json!({ "tx_id": tx.tx_id, "relay": status })
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

fn pays_integrated(network: Network, recipients: &[(String, u64)]) -> bool {
    recipients.iter().any(|(address, _)| {
        monero_rust::MoneroAddress::from_str(network, address)
            .is_ok_and(|address| address.payment_id().is_some())
    })
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
    let (network, network_name) = network_of(request);
    let rpc = rpc_of(request);
    let seed = seed_of(request);
    let state_path = request["state"].as_str().expect("state");
    let mut file = load_state(state_path);
    let ring_db = request["ring_db"].as_str();
    let mut state = if record_with_ring_db(request) {
        match wallet_state_with_rings(&file, request) {
            Ok(state) => state,
            Err(error) => return json!({ "prepare_error": format!("ring database: {error}") }),
        }
    } else {
        wallet_state_from(&file)
    };
    let rpc = rpc.for_wallet(&state);
    let stored_rings = match ring_db {
        Some(path) if std::path::Path::new(path).exists() => match ring_db_on_disk(path) {
            Ok(rings) => Some(rings),
            Err(error) => return json!({ "prepare_error": error }),
        },
        _ => None,
    };
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
                pays_integrated(network, &recipients),
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
    let policy = prepared.fee_policy.unwrap_or(policy);

    let mut builds = Vec::new();
    let mut errors = Vec::new();
    let mut first: Option<TransactionResult> = None;
    let build_started = Instant::now();
    let own_state = record && ring_db.is_some();
    for _ in 0..repeat {
        let mut fresh_state = WalletState::new();
        fresh_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
        fresh_state.restore_rpc_client_key(state.rpc_client_key().clone());
        fresh_state.add_outputs(wallet_outputs.clone());
        if let Some(rings) = stored_rings.clone().filter(|_| !fresh_rings) {
            if let Err(error) = fresh_state.restore_ring_database(rings) {
                return json!({ "prepare_error": format!("ring database: {error}") });
            }
        }
        let build_state: &mut WalletState = if own_state {
            &mut state
        } else {
            &mut fresh_state
        };
        let built = match kind {
            "send" if inputs.is_none() => {
                monero_rust::native::create_transaction_from_prepared_inputs(
                    &rpc,
                    &seed,
                    &network_name,
                    &prepared,
                    &recipients,
                    "",
                    policy,
                    &monero_rust::native::DecoyPolicy::default(),
                    build_state,
                )
                .await
            }
            "send" => {
                create_transaction_with_rpc_and_policy(
                    &rpc,
                    &seed,
                    &network_name,
                    prepared.stored_outputs.clone(),
                    &recipients,
                    "",
                    policy,
                    build_state,
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
                    build_state,
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
                let spent: Vec<&WalletOutput> = result
                    .input_rings
                    .iter()
                    .filter_map(|ring| {
                        wallet_outputs
                            .iter()
                            .find(|output| output.key_image == ring.key_image)
                    })
                    .collect();
                builds.push(json!({
                    "tx_id": result.tx_id,
                    "inputs": spent.iter().map(|output| output.output_key()).collect::<Vec<_>>(),
                    "input_total": spent.iter().map(|output| output.amount).sum::<u64>(),
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
        if !own_state {
            result
                .persist_input_rings(&mut state)
                .expect("record input rings");
        }
        state.track_transaction(TrackedTransaction {
            tx_id: result.tx_id.clone(),
            status: TxStatus::Created,
            spent_key_images: result
                .input_rings
                .iter()
                .map(|ring| ring.key_image.clone())
                .collect(),
            spent_output_keys: result
                .input_rings
                .iter()
                .filter_map(|ring| {
                    state
                        .outputs()
                        .iter()
                        .find(|output| output.key_image == ring.key_image)
                        .map(|output| output.output_key())
                })
                .collect(),
            change_outputs: Vec::new(),
            fee: result.fee,
            created_at_secs: now_secs(),
        });
        save_wallet(state_path, &mut file, &mut state)
            .await
            .expect("save wallet");
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
        "further_inputs": prepared
            .further_inputs
            .iter()
            .map(|input| format!("{}:{}", input.tx_hash, input.output_index))
            .collect::<Vec<_>>(),
        "input_total": prepared.total_input,
        "estimated_fee": prepared.estimated_fee,
        "builds": builds,
        "build_errors": errors,
        "build_seconds": build_seconds,
        "recorded": recorded,
        "ring_db": ring_db,
        "rings_stored_by_the_builder": own_state,
    })
}

async fn op_relay(request: &Value) -> Value {
    let rpc = rpc_of(request);
    let state_path = request["state"].as_str().expect("state");
    let tx_path = request["tx"].as_str().expect("tx");
    let mut file = load_state(state_path);
    let mut state = match wallet_state_with_rings(&file, request) {
        Ok(state) => state,
        Err(error) => return json!({ "relay": format!("not relayed: ring database: {error}") }),
    };
    let result: TransactionResult =
        serde_json::from_str(&std::fs::read_to_string(tx_path).expect("read tx")).expect("tx");
    let on_disk = match request["ring_db"].as_str().map(ring_db_on_disk) {
        Some(Ok(rings)) => Some(rings),
        Some(Err(error)) => return json!({ "relay": format!("not relayed: {error}") }),
        None => None,
    };

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
        if let Some(rings) = &on_disk {
            let recorded = rings.ring(&ring.key_image);
            assert!(
                recorded.is_some_and(|recorded| recorded.offsets == ring.ring.offsets),
                "the ring database file does not hold the ring of {}",
                ring.key_image
            );
        }
        assert!(!stored.spent, "input {} is already spent", ring.key_image);
        assert!(
            !state.is_pending_spent(&ring.key_image),
            "input {} is already pending in another relayed transaction",
            ring.key_image
        );
    }

    let key_images: Vec<String> = result
        .input_rings
        .iter()
        .map(|ring| ring.key_image.clone())
        .collect();
    let guard = if request["guard"].as_bool().unwrap_or(false) {
        match node_guard(&rpc, &key_images).await {
            Ok(guard) if guard["clear"] == true => guard,
            Ok(guard) => {
                return json!({ "tx_id": result.tx_id, "relay": "not relayed: an input is spent or in the pool", "guard": guard })
            }
            Err(error) => {
                return json!({ "tx_id": result.tx_id, "relay": format!("not relayed: guard failed: {error}") })
            }
        }
    } else {
        Value::Null
    };
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
        save_wallet(state_path, &mut file, &mut state)
            .await
            .expect("save wallet");
    }
    json!({ "tx_id": result.tx_id, "relay": status, "guard": guard })
}

async fn op_guard(request: &Value) -> Value {
    let rpc = rpc_of(request);
    let mut key_images: Vec<String> = request["key_images"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|key_image| key_image.as_str().expect("key image").to_string())
                .collect()
        })
        .unwrap_or_default();
    if let (Some(keys), Some(path)) = (request["outputs"].as_array(), request["state"].as_str()) {
        let file = load_state(path);
        for key in keys {
            let key = key.as_str().expect("output key");
            match file
                .outputs
                .iter()
                .find(|output| output.output_key() == key)
            {
                Some(output) => key_images.push(output.key_image.clone()),
                None => return json!({ "error": format!("{key} is not an output of this wallet") }),
            }
        }
    }
    match node_guard(&rpc, &key_images).await {
        Ok(guard) => guard,
        Err(error) => json!({ "error": error }),
    }
}

async fn op_estimate(request: &Value) -> Value {
    use monero_rust::fee_estimate::{fee_for_weight, wallet2_weight_estimate};
    let rpc = rpc_of(request);
    let priority = priority_of(request["priority"].as_str().unwrap_or("low"));
    let rct_type = match rpc.get_rct_type().await {
        Ok(rct_type) => rct_type,
        Err(error) => return json!({ "error": format!("{error:?}") }),
    };
    let fee = match rpc
        .get_fee_for_priority_at_height(priority, request["wallet_height"].as_u64())
        .await
    {
        Ok(fee) => fee,
        Err(error) => return json!({ "error": format!("{error:?}") }),
    };
    let rate = monero_wallet::interface::FeeRate::new(fee.per_weight, fee.mask).expect("fee rate");
    let mut rows = Vec::new();
    for shape in request["shapes"].as_array().expect("shapes") {
        let inputs = shape[0].as_u64().expect("inputs") as usize;
        let outputs = shape[1].as_u64().expect("outputs") as usize;
        let weight = wallet2_weight_estimate(rct_type, inputs, outputs).expect("estimate");
        rows.push(json!({
            "inputs": inputs,
            "outputs": outputs,
            "wallet2_weight": weight,
            "wallet2_fee": fee_for_weight(rate, weight).expect("fee"),
            "upper_bound_fee": monero_rust::coin_selection::estimate_fee_with_rate(inputs, outputs, rct_type, rate).ok(),
        }));
    }
    json!({ "per_weight": fee.per_weight, "mask": fee.mask, "rct_type": format!("{rct_type:?}"), "shapes": rows })
}

async fn rpc_probe_one(rpc: &Rpc<HttpRpc>, probe: &Value) -> Value {
    let kind = probe["kind"].as_str().expect("probe kind");
    let started = Instant::now();
    let hash_of = |value: &Value| -> [u8; 32] {
        hex::decode(value.as_str().expect("hash hex"))
            .expect("hash hex")
            .try_into()
            .expect("32-byte hash")
    };
    let outcome: Result<Value, String> = match kind {
        "height" => rpc
            .get_height()
            .await
            .map(|height| json!({ "height": height }))
            .map_err(|error| format!("{error:?}")),
        "rct_type" => rpc
            .get_rct_type()
            .await
            .map(|rct| json!({ "rct_type": format!("{rct:?}") }))
            .map_err(|error| format!("{error:?}")),
        "fee" => rpc
            .get_fee_for_priority_at_height(
                priority_of(probe["priority"].as_str().unwrap_or("automatic")),
                probe["wallet_height"].as_u64(),
            )
            .await
            .map(|fee| json!({ "per_weight": fee.per_weight, "mask": fee.mask }))
            .map_err(|error| format!("{error:?}")),
        "key_images" => {
            let count = probe["count"].as_u64().expect("count");
            let images: Vec<String> = match probe["key_images"].as_array() {
                Some(list) => list
                    .iter()
                    .map(|image| image.as_str().expect("key image").to_string())
                    .collect(),
                None => (1..=count).map(|i| format!("{i:064x}")).collect(),
            };
            key_image_spent_status(rpc, &images).await.map(|statuses| {
                json!({
                    "asked": images.len(),
                    "statuses": statuses.len(),
                    "spent": statuses.iter().filter(|status| **status != 0).count(),
                })
            })
        }
        "transactions" => {
            let hashes: Vec<[u8; 32]> = match probe["hashes"].as_array() {
                Some(list) => list.iter().map(hash_of).collect(),
                None => {
                    let height = probe["block"].as_u64().expect("hashes or block") as usize;
                    match rpc.get_block_by_number(height).await {
                        Ok(block) => block.transactions.clone(),
                        Err(error) => {
                            return json!({ "kind": kind, "error": format!("get_block: {error:?}") })
                        }
                    }
                }
            };
            rpc.get_transactions(&hashes)
                .await
                .map(|txs| json!({ "asked": hashes.len(), "returned": txs.len() }))
                .map_err(|error| format!("{error:?}"))
        }
        "block_txs" => rpc
            .get_block_transactions_by_number(probe["height"].as_u64().expect("height") as usize)
            .await
            .map(|txs| json!({ "returned": txs.len() }))
            .map_err(|error| format!("{error:?}")),
        "o_indexes" => rpc
            .get_o_indexes(hash_of(&probe["tx"]))
            .await
            .map(|indexes| json!({ "indexes": indexes }))
            .map_err(|error| format!("{error:?}")),
        "distribution" => {
            let height = match probe["height"].as_u64() {
                Some(height) => height as usize,
                None => rpc.get_height().await.unwrap_or(0),
            };
            rpc.get_output_distribution(0, height)
                .await
                .map(|distribution| {
                    json!({
                        "blocks": distribution.len(),
                        "outputs": distribution.last().copied().unwrap_or(0),
                    })
                })
                .map_err(|error| format!("{error:?}"))
        }
        "outs" => {
            let start = probe["start"].as_u64().expect("start");
            let count = probe["count"].as_u64().expect("count");
            let height = match probe["height"].as_u64() {
                Some(height) => height as usize,
                None => rpc.get_height().await.unwrap_or(0),
            };
            let indexes: Vec<u64> = (start..start + count).collect();
            rpc.get_unlocked_outputs(&indexes, height)
                .await
                .map(|outs| {
                    json!({
                        "asked": indexes.len(),
                        "returned": outs.len(),
                        "unlocked": outs.iter().filter(|out| out.is_some()).count(),
                    })
                })
                .map_err(|error| format!("{error:?}"))
        }
        "pool" => rpc
            .get_transaction_pool()
            .await
            .map(|(txs, images)| json!({ "transactions": txs.len(), "key_images": images.len() }))
            .map_err(|error| format!("{error:?}")),
        "getblocks" => fetch_blocks_batch(
            rpc,
            probe["start"].as_u64().expect("start"),
            probe["prune"].as_bool().unwrap_or(false),
        )
        .await
        .map(|fetched| json!({ "blocks": fetched.block_count() })),
        other => panic!("unknown probe {other}"),
    };
    let seconds = started.elapsed().as_secs_f64();
    match outcome {
        Ok(result) => json!({ "kind": kind, "ok": result, "seconds": seconds }),
        Err(error) => json!({ "kind": kind, "error": error, "seconds": seconds }),
    }
}

async fn op_rpc_probe(request: &Value) -> Value {
    let rpc = rpc_of(request);
    let mut results = Vec::new();
    for probe in request["probes"].as_array().expect("probes") {
        results.push(rpc_probe_one(&rpc, probe).await);
    }
    json!({ "probes": results })
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
        "ki-export" => op_ki_export(&request),
        "ki-import" => op_ki_import(&request).await,
        "ki-read" => op_ki_read(&request),
        "unsigned-build" => op_unsigned_build(&request).await,
        "sign" => op_sign(&request),
        "record-rings" => op_record_rings(&request).await,
        "txset-check" => op_txset_check(&request),
        "relay-signed" => op_relay_signed(&request).await,
        "rpc-probe" => op_rpc_probe(&request).await,
        "guard" => op_guard(&request).await,
        "estimate" => op_estimate(&request).await,
        other => panic!("unknown op {other}"),
    };
    std::fs::write(
        &response_path,
        serde_json::to_string(&response).expect("serialize response"),
    )
    .expect("write response");
}
