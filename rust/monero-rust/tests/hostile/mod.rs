//! Adversarial daemon harness: `SnapshotNode` replays a recorded regtest
//! chain, and `HostileNode` logs requests and applies one named mutation.

#![allow(dead_code)]

pub mod epee;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use epee::{Obj, Val};
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use monero_rust::scanner::{scan_blocks_batch_with_history, BlockScanResult, Lookahead};
use monero_rust::{
    process_batch_with_reorg_detection, ScanBatchOutcome, WalletOutput, WalletState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SEED_W: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";

pub const SEED_W2: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel \
    orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills \
    cottage daytime daytime";

/// Regtest uses mainnet address encoding.
pub const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;
pub const NETWORK_STR: &str = "mainnet";

pub const LOOKAHEAD: Lookahead = Lookahead {
    account: 0,
    subaddress: 5,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockRec {
    pub hash: String,
    pub entry: String,
    pub indices: String,
    pub header: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutRec {
    pub key: String,
    pub mask: String,
    pub height: u64,
    pub txid: String,
    pub coinbase: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fork {
    pub fork_height: u64,
    pub blocks: Vec<BlockRec>,
    pub first_out: u64,
    pub outs: Vec<OutRec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub monerod_version: String,
    pub chain: Vec<BlockRec>,
    pub outs: Vec<OutRec>,
    pub fork_shallow: Fork,
    pub fork_deep: Fork,
    pub tx_heights: Vec<u64>,
    pub get_info: Value,
    pub fee_estimate: Value,
    pub txpool_backlog: Value,
    pub transaction_pool: Value,
    pub send_raw_ok: Value,
    pub getblocks_template: String,
    pub getblocks_at_tip: String,
    pub distribution_raw: String,
    pub get_outs_sample: String,
}

pub fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostile_rpc/snapshot.json")
}

pub fn snapshot() -> Arc<Snapshot> {
    static SNAPSHOT: OnceLock<Arc<Snapshot>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| {
            let text = std::fs::read_to_string(fixture_path())
                .expect("tests/fixtures/hostile_rpc/snapshot.json (run hostile_rpc_record)");
            Arc::new(serde_json::from_str(&text).expect("snapshot parses"))
        })
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    Main,
    Shallow,
    Deep,
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub chain: Chain,
    pub height: u64,
}

#[derive(Clone)]
pub struct SnapshotNode {
    pub snapshot: Arc<Snapshot>,
    view: Arc<Mutex<View>>,
    pub relayed: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for SnapshotNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SnapshotNode({:?})", self.view.lock().unwrap())
    }
}

pub fn ringct_outputs_in(record: &BlockRec) -> u64 {
    if record.header["major_version"].as_u64().unwrap_or(0) < 4 {
        return 0;
    }
    let indices = epee::decode(&hex::decode(&record.indices).unwrap()).expect("indices decode");
    epee::items(&indices, "indices")
        .iter()
        .map(|tx| epee::items(tx.obj(), "indices").len() as u64)
        .sum()
}

pub fn outs_reply(outs: Vec<Val>) -> Vec<u8> {
    epee::encode(&vec![
        ("credits".into(), Val::U64(0)),
        ("outs".into(), Val::Arr(12, outs)),
        ("status".into(), epee::str_val("OK")),
        ("top_hash".into(), epee::str_val("")),
        ("untrusted".into(), Val::Bool(false)),
    ])
}

fn hex32(text: &str) -> Vec<u8> {
    let bytes = hex::decode(text).expect("hex");
    assert_eq!(bytes.len(), 32);
    bytes
}

fn json_bytes(value: Value) -> Result<Vec<u8>, RpcError> {
    Ok(serde_json::to_vec(&value).unwrap())
}

fn json_rpc_result(result: Value) -> Result<Vec<u8>, RpcError> {
    json_bytes(json!({ "id": "0", "jsonrpc": "2.0", "result": result }))
}

impl SnapshotNode {
    pub fn new() -> Self {
        let snapshot = snapshot();
        let height = snapshot.chain.len() as u64;
        SnapshotNode {
            snapshot,
            view: Arc::new(Mutex::new(View {
                chain: Chain::Main,
                height,
            })),
            relayed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn view(&self) -> View {
        *self.view.lock().unwrap()
    }

    pub fn switch_chain(&self, chain: Chain) {
        let height = self.chain_len(chain);
        *self.view.lock().unwrap() = View { chain, height };
    }

    pub fn set_height(&self, height: u64) {
        let mut view = self.view.lock().unwrap();
        assert!(height <= self.chain_len(view.chain));
        view.height = height;
    }

    fn fork(&self, chain: Chain) -> Option<&Fork> {
        match chain {
            Chain::Main => None,
            Chain::Shallow => Some(&self.snapshot.fork_shallow),
            Chain::Deep => Some(&self.snapshot.fork_deep),
        }
    }

    pub fn chain_len(&self, chain: Chain) -> u64 {
        match self.fork(chain) {
            None => self.snapshot.chain.len() as u64,
            Some(fork) => fork.fork_height + fork.blocks.len() as u64,
        }
    }

    pub fn height(&self) -> u64 {
        self.view().height
    }

    pub fn block_on(&self, chain: Chain, height: u64) -> &BlockRec {
        match self.fork(chain) {
            Some(fork) if height >= fork.fork_height => {
                &fork.blocks[(height - fork.fork_height) as usize]
            }
            _ => &self.snapshot.chain[height as usize],
        }
    }

    pub fn block(&self, height: u64) -> &BlockRec {
        self.block_on(self.view().chain, height)
    }

    pub fn out_on(&self, chain: Chain, index: u64) -> Option<&OutRec> {
        match self.fork(chain) {
            Some(fork) if index >= fork.first_out => {
                fork.outs.get((index - fork.first_out) as usize)
            }
            _ => self.snapshot.outs.get(index as usize),
        }
    }

    pub fn outputs_per_block(&self) -> Vec<u64> {
        (0..self.height())
            .map(|height| ringct_outputs_in(self.block(height)))
            .collect()
    }

    pub fn cumulative_distribution(&self) -> Vec<u64> {
        let mut total = 0;
        self.outputs_per_block()
            .into_iter()
            .map(|count| {
                total += count;
                total
            })
            .collect()
    }

    fn header(&self, height: u64) -> Value {
        self.block(height).header.clone()
    }

    pub fn honest_out(&self, index: u64) -> Option<Obj> {
        let view = self.view();
        let total = *self.cumulative_distribution().last().unwrap_or(&0);
        let out = self.out_on(view.chain, index).filter(|_| index < total)?;
        let unlocked = !out.coinbase || view.height >= out.height + 60;
        Some(vec![
            ("height".into(), Val::U64(out.height)),
            ("key".into(), Val::Str(hex32(&out.key))),
            ("mask".into(), Val::Str(hex32(&out.mask))),
            ("txid".into(), Val::Str(vec![0; 32])),
            ("unlocked".into(), Val::Bool(unlocked)),
        ])
    }

    fn get_outs(&self, body: &[u8]) -> Result<Vec<u8>, RpcError> {
        let request = epee::decode(body).map_err(|_| RpcError::InvalidNode)?;
        let mut outs = Vec::new();
        for requested in epee::items(&request, "outputs") {
            let index = epee::get(requested.obj(), "index").unwrap().u64();
            let Some(out) = self.honest_out(index) else {
                return Ok(epee::encode(&vec![
                    ("status".into(), epee::str_val("Failed")),
                    ("untrusted".into(), Val::Bool(false)),
                ]));
            };
            outs.push(Val::Obj(out));
        }
        Ok(outs_reply(outs))
    }

    fn get_distribution(&self) -> Result<Vec<u8>, RpcError> {
        let mut compressed = Vec::new();
        for count in self.outputs_per_block() {
            let mut value = count;
            while value >= 0x80 {
                compressed.push((value as u8) | 0x80);
                value >>= 7;
            }
            compressed.push(value as u8);
        }
        let distribution: Obj = vec![
            ("amount".into(), Val::U64(0)),
            ("base".into(), Val::U64(0)),
            ("binary".into(), Val::Bool(true)),
            ("compress".into(), Val::Bool(true)),
            ("compressed_data".into(), Val::Str(compressed)),
            ("start_height".into(), Val::U64(0)),
        ];
        Ok(epee::encode(&vec![
            ("credits".into(), Val::U64(0)),
            (
                "distributions".into(),
                Val::Arr(12, vec![Val::Obj(distribution)]),
            ),
            ("status".into(), epee::str_val("OK")),
            ("top_hash".into(), epee::str_val("")),
            ("untrusted".into(), Val::Bool(false)),
        ]))
    }

    fn get_blocks(&self, body: &[u8]) -> Result<Vec<u8>, RpcError> {
        let request = epee::decode(body).map_err(|_| RpcError::InvalidNode)?;
        let requested_start = epee::get(&request, "start_height").map_or(0, Val::u64);
        let ids = epee::get(&request, "block_ids").map_or(&[][..], Val::bytes);
        let height = self.height();
        let mut reply =
            epee::decode(&hex::decode(&self.snapshot.getblocks_template).unwrap()).unwrap();
        epee::set(&mut reply, "current_height", Val::U64(height));

        let failed = |mut reply: Obj| {
            epee::set(&mut reply, "status", epee::str_val("Failed"));
            epee::set(&mut reply, "start_height", Val::U64(0));
            Ok(epee::encode(&reply))
        };

        if ids.len() >= 32 && ids[..32] == hex32(&self.block(height - 1).hash)[..] {
            epee::set(&mut reply, "start_height", Val::U64(0));
            epee::set(&mut reply, "status", epee::str_val("OK"));
            return Ok(epee::encode(&reply));
        }

        let start = if requested_start > 0 {
            if requested_start >= height {
                return failed(reply);
            }
            requested_start
        } else {
            let known: HashMap<Vec<u8>, u64> = (0..height)
                .map(|h| (hex32(&self.block(h).hash), h))
                .collect();
            let chunks: Vec<&[u8]> = ids.chunks(32).collect();
            match chunks.last() {
                Some(last) if known.get(*last) == Some(&0) => {}
                _ => return failed(reply),
            }
            match chunks.iter().find_map(|id| known.get(*id)) {
                Some(found) => *found,
                None => return failed(reply),
            }
        };

        let end = (start + 1000).min(height);
        let mut blocks = Vec::new();
        let mut indices = Vec::new();
        for h in start..end {
            let record = self.block(h);
            blocks.push(Val::Obj(
                epee::decode(&hex::decode(&record.entry).unwrap()).unwrap(),
            ));
            indices.push(Val::Obj(
                epee::decode(&hex::decode(&record.indices).unwrap()).unwrap(),
            ));
        }
        epee::set(&mut reply, "start_height", Val::U64(start));
        epee::set(&mut reply, "blocks", Val::Arr(12, blocks));
        epee::set(&mut reply, "output_indices", Val::Arr(12, indices));
        epee::set(&mut reply, "status", epee::str_val("OK"));
        Ok(epee::encode(&reply))
    }

    fn json_rpc(&self, body: &[u8]) -> Result<Vec<u8>, RpcError> {
        let request: Value = serde_json::from_slice(body).map_err(|_| RpcError::InvalidNode)?;
        let method = request["method"].as_str().unwrap_or_default();
        let params = &request["params"];
        let height = self.height();
        match method {
            "get_last_block_header" => json_rpc_result(json!({
                "block_header": self.header(height - 1),
                "status": "OK",
                "untrusted": false,
            })),
            "get_block_header_by_height" => {
                let wanted = params["height"].as_u64().unwrap_or(u64::MAX);
                if wanted >= height {
                    return json_bytes(json!({
                        "id": "0", "jsonrpc": "2.0",
                        "error": { "code": -2, "message": "Requested block height is too big" }
                    }));
                }
                json_rpc_result(json!({
                    "block_header": self.header(wanted),
                    "status": "OK",
                    "untrusted": false,
                }))
            }
            "getblockheadersrange" => {
                let start = params["start_height"].as_u64().unwrap_or(0);
                let end = params["end_height"].as_u64().unwrap_or(0);
                if end >= height || start > end {
                    return json_bytes(json!({
                        "id": "0", "jsonrpc": "2.0",
                        "error": { "code": -4, "message": "Invalid start/end heights." }
                    }));
                }
                let headers: Vec<Value> = (start..=end).map(|h| self.header(h)).collect();
                json_rpc_result(json!({ "headers": headers, "status": "OK", "untrusted": false }))
            }
            "get_fee_estimate" => json_rpc_result(self.snapshot.fee_estimate.clone()),
            "get_txpool_backlog" => json_rpc_result(self.snapshot.txpool_backlog.clone()),
            "get_info" => {
                let mut info = self.snapshot.get_info.clone();
                info["height"] = json!(height);
                info["top_block_hash"] = json!(self.block(height - 1).hash);
                json_rpc_result(info)
            }
            other => panic!("SnapshotNode: unsupported json_rpc method {other}"),
        }
    }
}

#[async_trait::async_trait]
impl RpcConnection for SnapshotNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let height = self.height();
        match route {
            "get_height" => json_bytes(json!({
                "hash": self.block(height - 1).hash,
                "height": height,
                "status": "OK",
                "untrusted": false,
            })),
            "json_rpc" => self.json_rpc(&body),
            "get_outs.bin" => self.get_outs(&body),
            "get_output_distribution.bin" => self.get_distribution(),
            "getblocks.bin" => self.get_blocks(&body),
            "get_transaction_pool" => json_bytes(self.snapshot.transaction_pool.clone()),
            "send_raw_transaction" => {
                let request: Value =
                    serde_json::from_slice(&body).map_err(|_| RpcError::InvalidNode)?;
                self.relayed.lock().unwrap().push(
                    request["tx_as_hex"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                );
                json_bytes(self.snapshot.send_raw_ok.clone())
            }
            "is_key_image_spent" => {
                let request: Value =
                    serde_json::from_slice(&body).map_err(|_| RpcError::InvalidNode)?;
                let count = request["key_images"].as_array().map_or(0, Vec::len);
                json_bytes(json!({ "spent_status": vec![0; count], "status": "OK" }))
            }
            "get_transactions" => json_bytes(json!({ "status": "OK", "txs": [], "missed_tx": [] })),
            "get_o_indexes.bin" => Ok(epee::encode(&vec![
                ("status".into(), epee::str_val("Failed")),
                ("untrusted".into(), Val::Bool(false)),
            ])),
            other => panic!("SnapshotNode: unsupported route {other}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveNode {
    pub url: String,
    client: reqwest::Client,
}

impl LiveNode {
    pub fn new(url: &str) -> Self {
        LiveNode {
            url: url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl RpcConnection for LiveNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let response = self
            .client
            .post(format!("{}/{}", self.url, route))
            .body(body)
            .send()
            .await
            .map_err(|_| RpcError::ConnectionError)?;
        Ok(response
            .bytes()
            .await
            .map_err(|_| RpcError::ConnectionError)?
            .to_vec())
    }
}

#[derive(Clone, Debug)]
pub struct Logged {
    pub route: String,
    pub method: String,
    pub body: Vec<u8>,
}

impl Logged {
    pub fn name(&self) -> String {
        if self.method.is_empty() {
            self.route.clone()
        } else {
            format!("{}:{}", self.route, self.method)
        }
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn epee(&self) -> Obj {
        epee::decode(&self.body).expect("binary request decodes")
    }

    pub fn get_outs_indices(&self) -> Vec<u64> {
        epee::items(&self.epee(), "outputs")
            .iter()
            .map(|out| epee::get(out.obj(), "index").unwrap().u64())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Call {
    pub route: String,
    pub method: String,
    pub body: Vec<u8>,
    pub nth: usize,
    pub seq: usize,
}

impl Call {
    pub fn is(&self, name: &str) -> bool {
        if self.method.is_empty() {
            self.route == name
        } else {
            self.method == name || format!("{}:{}", self.route, self.method) == name
        }
    }
}

pub type Reply = Result<Vec<u8>, RpcError>;
type Hook = dyn Fn(&Call, Reply) -> Reply + Send + Sync;

#[derive(Clone)]
pub struct HostileNode<I: RpcConnection> {
    pub inner: I,
    pub mutation: &'static str,
    hook: Arc<Hook>,
    log: Arc<Mutex<Vec<Logged>>>,
}

impl<I: RpcConnection> std::fmt::Debug for HostileNode<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HostileNode({})", self.mutation)
    }
}

impl<I: RpcConnection + 'static> HostileNode<I> {
    pub fn honest(inner: I) -> Self {
        Self::with(inner, "honest", |_, reply| reply)
    }

    pub fn with(
        inner: I,
        mutation: &'static str,
        hook: impl Fn(&Call, Reply) -> Reply + Send + Sync + 'static,
    ) -> Self {
        HostileNode {
            inner,
            mutation,
            hook: Arc::new(hook),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn rpc(&self) -> Rpc<Self> {
        Rpc::new_with_connection(self.clone())
    }

    pub fn log(&self) -> Vec<Logged> {
        self.log.lock().unwrap().clone()
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    pub fn names(&self) -> Vec<String> {
        self.log().iter().map(Logged::name).collect()
    }

    pub fn count(&self, name: &str) -> usize {
        self.names().iter().filter(|n| n.as_str() == name).count()
    }

    pub fn requests(&self, name: &str) -> Vec<Logged> {
        self.log()
            .into_iter()
            .filter(|logged| logged.name() == name)
            .collect()
    }
}

#[async_trait::async_trait]
impl<I: RpcConnection + 'static> RpcConnection for HostileNode<I> {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let method = if route == "json_rpc" {
            serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v["method"].as_str().map(str::to_string))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let call = {
            let mut log = self.log.lock().unwrap();
            let nth = log
                .iter()
                .filter(|l| l.route == route && l.method == method)
                .count();
            let seq = log.len();
            log.push(Logged {
                route: route.to_string(),
                method: method.clone(),
                body: body.clone(),
            });
            Call {
                route: route.to_string(),
                method,
                body: body.clone(),
                nth,
                seq,
            }
        };
        let honest = self.inner.post(route, body).await;
        (self.hook)(&call, honest)
    }
}

pub fn edit_bin(reply: Reply, edit: impl FnOnce(&mut Obj)) -> Reply {
    let mut obj = epee::decode(&reply?).expect("honest binary reply decodes");
    edit(&mut obj);
    Ok(epee::encode(&obj))
}

pub fn edit_json(reply: Reply, edit: impl FnOnce(&mut Value)) -> Reply {
    let mut value: Value = serde_json::from_slice(&reply?).expect("honest JSON reply parses");
    edit(&mut value);
    Ok(serde_json::to_vec(&value).unwrap())
}

pub fn edit_outs(call: &Call, reply: Reply, mut edit: impl FnMut(u64, &mut Obj)) -> Reply {
    let request = epee::decode(&call.body).expect("get_outs request decodes");
    let indices: Vec<u64> = epee::items(&request, "outputs")
        .iter()
        .map(|out| epee::get(out.obj(), "index").unwrap().u64())
        .collect();
    edit_bin(reply, |obj| {
        let outs = epee::get_mut(obj, "outs").expect("outs").arr_mut();
        for (index, out) in indices.iter().zip(outs.iter_mut()) {
            edit(*index, out.obj_mut());
        }
    })
}

pub fn distribution_reply(per_block: &[u64]) -> Reply {
    let mut obj: Obj = vec![
        (
            "distributions".into(),
            Val::Arr(
                12,
                vec![Val::Obj(vec![
                    ("amount".into(), Val::U64(0)),
                    ("binary".into(), Val::Bool(true)),
                    ("compress".into(), Val::Bool(true)),
                ])],
            ),
        ),
        ("status".into(), epee::str_val("OK")),
    ];
    set_distribution(&mut obj, per_block, 0, 0);
    Ok(epee::encode(&obj))
}

pub fn set_distribution(obj: &mut Obj, per_block: &[u64], start_height: u64, base: u64) {
    let mut compressed = Vec::new();
    for count in per_block {
        let mut value = *count;
        while value >= 0x80 {
            compressed.push((value as u8) | 0x80);
            value >>= 7;
        }
        compressed.push(value as u8);
    }
    let distribution = epee::get_mut(obj, "distributions").unwrap().arr_mut()[0].obj_mut();
    epee::set(distribution, "compressed_data", Val::Str(compressed));
    epee::set(distribution, "start_height", Val::U64(start_height));
    epee::set(distribution, "base", Val::U64(base));
}

pub const AT_TIP: &str = "History-negotiated batch is missing its common starting block";

pub struct Wallet {
    pub seed: &'static str,
    pub state: WalletState,
    pub restore_height: u64,
    pub reorgs: Vec<monero_rust::ReorgInfo>,
    genesis: String,
    started: bool,
}

impl Wallet {
    pub fn new(seed: &'static str) -> Self {
        Wallet {
            seed,
            state: WalletState::new(),
            restore_height: 0,
            reorgs: Vec::new(),
            genesis: snapshot().chain[0].hash.clone(),
            started: false,
        }
    }

    fn apply(&mut self, results: &[BlockScanResult], batch: monero_rust::ProcessedBatch) {
        self.state.add_outputs(batch.outputs_to_store.clone());
        for result in results {
            self.state.mark_spent_detecting_conflicts(
                &result.spent_key_images,
                &result.spent_key_image_tx_hashes,
                result.block_height,
            );
        }
        for (height, hash) in &batch.block_hashes {
            self.state.record_block_hash(*height, hash.clone());
        }
        self.state.record_block_hash(0, self.genesis.clone());
        self.state.current_height = batch.batch_end_height - 1;
        self.state.daemon_height = batch.daemon_height;
        self.started = true;
    }

    pub async fn refresh<C: RpcConnection>(
        &mut self,
        rpc: &Rpc<C>,
        max_batches: usize,
    ) -> Result<usize, String> {
        let mut batches = 0;
        while batches < max_batches {
            let history = if self.started {
                self.state.get_short_chain_history()
            } else {
                Vec::new()
            };
            let start = if self.started { 0 } else { self.restore_height };
            let scanned = scan_blocks_batch_with_history(
                rpc, start, &history, self.seed, NETWORK, LOOKAHEAD, false, "",
            )
            .await;
            batches += 1;
            let (mut results, actual_start) = match scanned {
                Ok(scanned) => scanned,
                // A client at monerod's tip gets an empty OK reply.
                Err(e) if !history.is_empty() && e.contains(AT_TIP) => return Ok(batches),
                Err(e) => return Err(e),
            };
            for result in results.iter_mut().filter(|r| r.block_height == 0) {
                result.block_hash = self.genesis.clone();
            }
            let fresh = results
                .iter()
                .any(|r| self.state.block_hashes.get_hash(r.block_height) != Some(&r.block_hash));
            if results.is_empty() || !fresh {
                return Ok(batches);
            }
            match process_batch_with_reorg_detection(
                &results,
                &mut self.state,
                None,
                u64::MAX,
                actual_start,
            )? {
                ScanBatchOutcome::Normal(batch) => self.apply(&results, batch),
                ScanBatchOutcome::Reorg(info) => self.reorgs.push(info),
            }
        }
        Err(format!("refresh did not converge in {max_batches} batches"))
    }

    pub fn outputs(&self) -> &[WalletOutput] {
        self.state.outputs()
    }

    pub fn unspent(&self) -> Vec<&WalletOutput> {
        self.state.outputs().iter().filter(|o| !o.spent).collect()
    }
}

pub async fn synced_wallet() -> Wallet {
    static OUTPUTS: OnceLock<String> = OnceLock::new();
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = LOCK.lock().await;
    let node = SnapshotNode::new();
    let mut wallet = Wallet::new(SEED_W);
    if let Some(cached) = OUTPUTS.get() {
        let (outputs, hashes, height): (Vec<WalletOutput>, Vec<(u64, String)>, u64) =
            serde_json::from_str(cached).unwrap();
        wallet.state.add_outputs(outputs);
        for (h, hash) in hashes {
            wallet.state.record_block_hash(h, hash);
        }
        wallet.state.current_height = height;
        wallet.state.daemon_height = height + 1;
        wallet.started = true;
        return wallet;
    }
    let rpc = Rpc::new_with_connection(node.clone());
    wallet.refresh(&rpc, 64).await.expect("honest refresh");
    let hashes: Vec<(u64, String)> = (0..node.height())
        .filter_map(|h| {
            wallet
                .state
                .block_hashes
                .get_hash(h)
                .map(|hash| (h, hash.to_string()))
        })
        .collect();
    let cached = serde_json::to_string(&(
        wallet.state.outputs().to_vec(),
        hashes,
        wallet.state.current_height,
    ))
    .unwrap();
    let _ = OUTPUTS.set(cached);
    wallet
}
