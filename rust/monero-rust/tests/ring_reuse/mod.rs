#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_oxide::transaction::{Input, NotPruned, Transaction};
use monero_rust::monero_rpc::{
    FeePriority, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
};
use monero_rust::native::{
    DecoySelection, FeePolicy, InputRing, RingMember, StoredOutputData, TransactionResult,
    UnsignedTransactionResult,
};
use monero_rust::{
    process_batch_with_reorg_detection, BlockScanResult, ScanBatchOutcome, WalletOutput,
    WalletState,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders \
    cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful \
    layout lower eden";
const OTHER_SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";
pub const NETWORK: &str = "mainnet";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relay {
    Accept,
    Reject,
    Unreachable,
    AcceptButLoseReply,
}

#[derive(Clone, Copy, Debug)]
pub enum Lock {
    Nothing,
    AtOrAbove(u64),
    Below(u64),
}

#[derive(Debug)]
struct NodeState {
    per_block: Vec<u64>,
    overrides: HashMap<u64, ([u8; 32], [u8; 32])>,
    never_locked: HashSet<u64>,
    lock_script: Vec<Lock>,
    always_locked: HashSet<u64>,
    get_outs_log: Vec<Vec<u64>>,
    relay: Relay,
    seen_txs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FakeNode(Arc<Mutex<NodeState>>);

fn default_point(index: u64, salt: u64) -> [u8; 32] {
    (&Scalar::from(index + salt) * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes()
}

pub fn default_member(index: u64) -> ([u8; 32], [u8; 32]) {
    (
        default_point(index, 1_000_003),
        default_point(index, 9_000_011),
    )
}

#[derive(Deserialize)]
struct GetOutsRequest {
    outputs: Vec<GetOut>,
    #[allow(dead_code)]
    get_txid: bool,
}

#[derive(Deserialize)]
struct GetOut {
    #[allow(dead_code)]
    amount: u64,
    index: u64,
}

#[derive(Serialize)]
struct Out {
    #[serde(with = "serde_bytes")]
    key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    mask: Vec<u8>,
    unlocked: bool,
    height: u64,
    #[serde(with = "serde_bytes")]
    txid: Vec<u8>,
}

#[derive(Serialize)]
struct Outs {
    outs: Vec<Out>,
    status: String,
}

#[derive(Serialize)]
struct Distributions {
    distributions: Vec<Distribution>,
    status: String,
}

#[derive(Serialize)]
struct Distribution {
    amount: u64,
    start_height: u64,
    binary: bool,
    compress: bool,
    #[serde(with = "serde_bytes")]
    compressed_data: Vec<u8>,
    base: u64,
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

impl FakeNode {
    pub fn uniform() -> Self {
        Self::with_blocks(vec![2; 100_000])
    }

    pub fn tail_heavy() -> Self {
        let mut blocks = vec![2; 99_991];
        blocks.extend(std::iter::repeat(14_162).take(9));
        Self::with_blocks(blocks)
    }

    pub fn small() -> Self {
        Self::with_blocks(vec![2; 2_000])
    }

    fn with_blocks(per_block: Vec<u64>) -> Self {
        FakeNode(Arc::new(Mutex::new(NodeState {
            per_block,
            overrides: HashMap::new(),
            never_locked: HashSet::new(),
            lock_script: Vec::new(),
            always_locked: HashSet::new(),
            get_outs_log: Vec::new(),
            relay: Relay::Accept,
            seen_txs: Vec::new(),
        })))
    }

    pub fn rpc(&self) -> Rpc<FakeNode> {
        Rpc::new_with_connection(self.clone())
    }

    pub fn total_outputs(&self) -> u64 {
        self.0.lock().unwrap().per_block.iter().sum()
    }

    pub fn selectable_outputs(&self) -> u64 {
        let state = self.0.lock().unwrap();
        state.per_block[..state.per_block.len() - 9].iter().sum()
    }

    pub fn set_output(&self, index: u64, key: [u8; 32], commitment: [u8; 32]) {
        let mut state = self.0.lock().unwrap();
        state.overrides.insert(index, (key, commitment));
        state.never_locked.insert(index);
    }

    pub fn clear_output(&self, index: u64) {
        let mut state = self.0.lock().unwrap();
        state.overrides.remove(&index);
        state.never_locked.remove(&index);
    }

    pub fn never_lock(&self, indexes: &[u64]) {
        self.0.lock().unwrap().never_locked.extend(indexes);
    }

    pub fn always_lock(&self, indexes: &[u64]) {
        self.0.lock().unwrap().always_locked.extend(indexes);
    }

    pub fn set_lock_script(&self, script: Vec<Lock>) {
        let mut state = self.0.lock().unwrap();
        let done = state.get_outs_log.len();
        let mut padded = vec![Lock::Nothing; done];
        padded.extend(script);
        state.lock_script = padded;
    }

    pub fn set_relay(&self, relay: Relay) {
        self.0.lock().unwrap().relay = relay;
    }

    pub fn get_outs_log(&self) -> Vec<Vec<u64>> {
        self.0.lock().unwrap().get_outs_log.clone()
    }

    pub fn seen_txs(&self) -> Vec<String> {
        self.0.lock().unwrap().seen_txs.clone()
    }

    fn json(&self, body: &[u8]) -> Result<Vec<u8>, RpcError> {
        let request: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| RpcError::InvalidNode)?;
        let height = self.0.lock().unwrap().per_block.len() as u64;
        let result = match request["method"].as_str().unwrap_or_default() {
            "get_last_block_header" => serde_json::json!({
                "block_header": { "major_version": 16 },
                "status": "OK"
            }),
            "get_fee_estimate" => serde_json::json!({
                "fee": 20_000,
                "fees": [20_000, 80_000, 320_000, 4_000_000],
                "quantization_mask": 10_000,
                "status": "OK"
            }),
            "get_txpool_backlog" => serde_json::json!({ "backlog": [], "status": "OK" }),
            "get_info" => serde_json::json!({
                "height": height,
                "block_weight_limit": 600_000,
                "status": "OK"
            }),
            "getblockheadersrange" => serde_json::json!({
                "headers": (0..10)
                    .map(|_| serde_json::json!({ "block_weight": 0 }))
                    .collect::<Vec<_>>(),
                "status": "OK"
            }),
            method => panic!("the fake daemon was asked for JSON-RPC method {method}"),
        };
        Ok(serde_json::to_vec(&serde_json::json!({ "result": result })).unwrap())
    }
}

#[async_trait::async_trait]
impl RpcConnection for FakeNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        match route {
            "json_rpc" => self.json(&body),
            "get_height" => {
                let height = self.0.lock().unwrap().per_block.len();
                Ok(
                    serde_json::to_vec(&serde_json::json!({ "height": height, "status": "OK" }))
                        .unwrap(),
                )
            }
            "get_output_distribution.bin" => {
                let state = self.0.lock().unwrap();
                let mut compressed_data = Vec::with_capacity(state.per_block.len());
                for count in &state.per_block {
                    varint(*count, &mut compressed_data);
                }
                Ok(monero_epee_bin_serde::to_bytes(&Distributions {
                    distributions: vec![Distribution {
                        amount: 0,
                        start_height: 0,
                        binary: true,
                        compress: true,
                        compressed_data,
                        base: 0,
                    }],
                    status: "OK".to_string(),
                })
                .unwrap())
            }
            "get_outs.bin" => {
                let request: GetOutsRequest =
                    monero_epee_bin_serde::from_bytes(&body).expect("get_outs request");
                let mut state = self.0.lock().unwrap();
                let call = state.get_outs_log.len();
                let rule = state
                    .lock_script
                    .get(call)
                    .copied()
                    .unwrap_or(Lock::Nothing);
                let indexes: Vec<u64> = request.outputs.iter().map(|out| out.index).collect();
                let total: u64 = state.per_block.iter().sum();
                let outs = indexes
                    .iter()
                    .map(|index| {
                        assert!(*index < total, "request for nonexistent output {index}");
                        let (key, mask) = state
                            .overrides
                            .get(index)
                            .copied()
                            .unwrap_or_else(|| default_member(*index));
                        let scripted = match rule {
                            Lock::Nothing => false,
                            Lock::AtOrAbove(bound) => *index >= bound,
                            Lock::Below(bound) => *index < bound,
                        };
                        let locked = state.always_locked.contains(index)
                            || (scripted && !state.never_locked.contains(index));
                        Out {
                            key: key.to_vec(),
                            mask: mask.to_vec(),
                            unlocked: !locked,
                            height: 0,
                            txid: vec![0; 32],
                        }
                    })
                    .collect();
                state.get_outs_log.push(indexes);
                Ok(monero_epee_bin_serde::to_bytes(&Outs {
                    outs,
                    status: "OK".to_string(),
                })
                .unwrap())
            }
            "send_raw_transaction" => {
                let request: serde_json::Value =
                    serde_json::from_slice(&body).map_err(|_| RpcError::InvalidNode)?;
                let mut state = self.0.lock().unwrap();
                if state.relay == Relay::Unreachable {
                    return Err(RpcError::ConnectionError);
                }
                state
                    .seen_txs
                    .push(request["tx_as_hex"].as_str().unwrap().to_string());
                match state.relay {
                    Relay::Accept => Ok(br#"{"status":"OK","reason":""}"#.to_vec()),
                    Relay::Reject => Ok(br#"{"status":"Failed","reason":"double spend"}"#.to_vec()),
                    Relay::AcceptButLoseReply => Err(RpcError::ConnectionError),
                    Relay::Unreachable => unreachable!(),
                }
            }
            route => panic!("the fake daemon was asked for route {route}"),
        }
    }
}

pub struct Keys {
    pub spend: Scalar,
    pub view: Scalar,
    pub view_hex: String,
    pub public_spend_hex: String,
}

pub fn keys() -> Keys {
    let derived = monero_rust::derive_keys(SEED, monero_rust::Network::Mainnet, "").unwrap();
    let spend = Scalar::from_bytes_mod_order(**derived.secret_spend_key.as_ref().unwrap());
    let view = Scalar::from_bytes_mod_order(*derived.secret_view_key);
    Keys {
        spend,
        view,
        view_hex: hex::encode(view.to_bytes()),
        public_spend_hex: hex::encode(derived.public_spend_key),
    }
}

pub fn destination(n: u32) -> String {
    if n == 0 {
        monero_rust::derive_address(OTHER_SEED, monero_rust::Network::Mainnet, "")
            .unwrap()
            .to_string()
    } else {
        monero_rust::derive_subaddress(OTHER_SEED, monero_rust::Network::Mainnet, 0, n, "")
            .unwrap()
            .to_string()
    }
}

pub struct Owned {
    pub output: WalletOutput,
    pub key: [u8; 32],
    pub commitment: [u8; 32],
}

impl Owned {
    pub fn key_image(&self) -> &str {
        &self.output.key_image
    }
    pub fn global_index(&self) -> u64 {
        self.output.global_index.unwrap()
    }
}

pub fn owned_output(n: u8, global_index: u64, block_height: u64, amount: u64) -> Owned {
    let keys = keys();
    let key_offset = Scalar::from(1_000u64 + u64::from(n));
    let mask = Scalar::from(77u64 + u64::from(n));
    let one_time = Zeroizing::new(keys.spend + key_offset);
    let key = (&*one_time * ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let commitment: EdwardsPoint =
        monero_wallet::ed25519::Commitment::new(monero_wallet::ed25519::Scalar::from(mask), amount)
            .commit()
            .into();
    let commitment = commitment.compress().to_bytes();
    let key_image = monero_rust::crypto::generate_key_image(&one_time)
        .compress()
        .to_bytes();

    let tx_hash = [n; 32];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&tx_hash);
    bytes.extend_from_slice(&0u64.to_le_bytes()); // index in transaction
    bytes.extend_from_slice(&0u64.to_le_bytes()); // placeholder chain index
    bytes.extend_from_slice(&key);
    bytes.extend_from_slice(&key_offset.to_bytes());
    bytes.extend_from_slice(&mask.to_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.push(0); // no additional timelock
    bytes.push(0); // no subaddress
    bytes.push(0); // no payment id
    bytes.push(0); // no arbitrary data
    monero_wallet::WalletOutput::read(&mut bytes.as_slice()).expect("fixture output parses");

    Owned {
        output: WalletOutput {
            tx_hash: hex::encode(tx_hash),
            output_index: 0,
            amount,
            key: hex::encode(key),
            key_offset: hex::encode(key_offset.to_bytes()),
            commitment_mask: hex::encode(mask.to_bytes()),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: hex::encode(bytes),
            global_index: Some(global_index),
            ring: None,
            block_height,
            spent: false,
            spent_height: None,
            key_image: hex::encode(key_image),
            is_coinbase: false,
            frozen: false,
            pre_ringct: false,
        },
        key,
        commitment,
    }
}

pub const XMR: u64 = 1_000_000_000_000;

#[derive(Clone, Default)]
pub struct Disk(Arc<Mutex<Stored>>);

#[derive(Default)]
struct Stored {
    outputs: String,
    rings: String,
}

struct DiskRings(Disk);

#[async_trait::async_trait]
impl monero_rust::RingPersister for DiskRings {
    async fn persist(&mut self, rings: &monero_rust::RingDatabase) -> Result<(), String> {
        self.0 .0.lock().unwrap().rings = serde_json::to_string(rings).unwrap();
        Ok(())
    }
}

impl Disk {
    pub fn save(&self, state: &WalletState) {
        let mut stored = self.0.lock().unwrap();
        stored.outputs = serde_json::to_string(state.outputs()).unwrap();
        stored.rings = serde_json::to_string(state.ring_database()).unwrap();
    }
}

fn wallet_on(disk: &Disk, outputs: Vec<WalletOutput>) -> WalletState {
    let mut state = WalletState::new();
    state.set_ring_persister(Box::new(DiskRings(disk.clone())));
    state
        .restore_ring_database(monero_rust::RingDatabase::default())
        .unwrap();
    state.replace_outputs(outputs);
    disk.save(&state);
    state
}

pub fn funded(node: &FakeNode, count: u8) -> (WalletState, Vec<Owned>, Disk) {
    let mut owned = Vec::new();
    for n in 0..count {
        let global_index = node.selectable_outputs() - 500 - 97 * u64::from(n);
        let output = owned_output(n + 1, global_index, 50_000 + u64::from(n), 5 * XMR);
        node.set_output(global_index, output.key, output.commitment);
        owned.push(output);
    }
    let disk = Disk::default();
    let state = wallet_on(&disk, owned.iter().map(|o| o.output.clone()).collect());
    (state, owned, disk)
}

pub fn wallet_holding(outputs: Vec<WalletOutput>) -> (WalletState, Disk) {
    let disk = Disk::default();
    let state = wallet_on(&disk, outputs);
    (state, disk)
}

pub fn second_instance(owned: &[Owned]) -> (WalletState, Disk) {
    wallet_holding(owned.iter().map(|o| o.output.clone()).collect())
}

pub fn crash_and_reload(disk: &Disk) -> WalletState {
    let stored = disk.0.lock().unwrap();
    let mut state = WalletState::new();
    state.set_ring_persister(Box::new(DiskRings(disk.clone())));
    state
        .restore_ring_database(serde_json::from_str(&stored.rings).unwrap())
        .unwrap();
    state.replace_outputs(serde_json::from_str(&stored.outputs).unwrap());
    state
}

pub fn reload_without_ring_database(disk: &Disk) -> WalletState {
    let stored = disk.0.lock().unwrap();
    let mut state = WalletState::new();
    state.set_ring_persister(Box::new(DiskRings(disk.clone())));
    state.replace_outputs(serde_json::from_str(&stored.outputs).unwrap());
    state
}

pub fn inputs(state: &WalletState, key_images: &[&str]) -> Vec<StoredOutputData> {
    key_images
        .iter()
        .map(|key_image| {
            state
                .outputs()
                .iter()
                .find(|output| output.key_image == *key_image)
                .map(StoredOutputData::from)
                .unwrap_or_else(|| panic!("no output with key image {key_image}"))
        })
        .collect()
}

pub fn policy(priority: FeePriority) -> FeePolicy {
    FeePolicy {
        priority,
        wallet_height: None,
        max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
    }
}

pub async fn hot_send(
    node: &FakeNode,
    state: &mut WalletState,
    key_images: &[&str],
    destination: &str,
    amount: u64,
    priority: FeePriority,
) -> Result<TransactionResult, String> {
    let inputs = inputs(state, key_images);
    hot_send_with(node, state, inputs, destination, amount, priority).await
}

pub async fn hot_send_with(
    node: &FakeNode,
    state: &mut WalletState,
    inputs: Vec<StoredOutputData>,
    destination: &str,
    amount: u64,
    priority: FeePriority,
) -> Result<TransactionResult, String> {
    hot_send_as(node, state, SEED, inputs, destination, amount, priority).await
}

pub async fn hot_send_as(
    node: &FakeNode,
    state: &mut WalletState,
    seed: &str,
    inputs: Vec<StoredOutputData>,
    destination: &str,
    amount: u64,
    priority: FeePriority,
) -> Result<TransactionResult, String> {
    monero_rust::native::create_transaction_with_rpc_and_policy(
        &node.rpc(),
        seed,
        NETWORK,
        inputs,
        &[(destination.to_string(), amount)],
        "",
        policy(priority),
        state,
    )
    .await
}

pub async fn hot_sweep(
    node: &FakeNode,
    state: &mut WalletState,
    key_images: &[&str],
    destination: &str,
) -> Result<TransactionResult, String> {
    let inputs = inputs(state, key_images);
    hot_sweep_with(node, state, inputs, destination).await
}

pub async fn prepare_sweep(node: &FakeNode, state: &WalletState) -> Vec<Vec<StoredOutputData>> {
    let rpc = node.rpc();
    let height = rpc.get_height().await.unwrap() as u64;
    monero_rust::prepare_sweep_transactions_with_rpc(
        &rpc,
        state.outputs(),
        height,
        monero_rust::SweepSubaddresses::All,
        None,
        0,
        policy(FeePriority::Normal),
    )
    .await
    .unwrap()
    .into_iter()
    .map(|transaction| transaction.stored_outputs)
    .collect()
}

pub async fn hot_sweep_with(
    node: &FakeNode,
    state: &mut WalletState,
    inputs: Vec<StoredOutputData>,
    destination: &str,
) -> Result<TransactionResult, String> {
    monero_rust::native::sweep_all_with_rpc_and_policy(
        &node.rpc(),
        SEED,
        NETWORK,
        inputs,
        destination,
        "",
        policy(FeePriority::Normal),
        state,
    )
    .await
}

pub async fn cold_build(
    node: &FakeNode,
    state: &mut WalletState,
    key_images: &[&str],
    destination: &str,
    amount: u64,
) -> Result<UnsignedTransactionResult, String> {
    let inputs = inputs(state, key_images);
    cold_build_with(node, state, inputs, destination, amount).await
}

pub async fn cold_build_with(
    node: &FakeNode,
    state: &mut WalletState,
    inputs: Vec<StoredOutputData>,
    destination: &str,
    amount: u64,
) -> Result<UnsignedTransactionResult, String> {
    let keys = keys();
    monero_rust::native::create_unsigned_transaction_with_rpc_and_policy(
        &node.rpc(),
        &keys.view_hex,
        &keys.public_spend_hex,
        NETWORK,
        inputs,
        &[(destination.to_string(), amount)],
        policy(FeePriority::Normal),
        state,
    )
    .await
}

pub async fn draw_rings(
    node: &FakeNode,
    state: &mut WalletState,
    key_images: &[&str],
) -> Result<Vec<DecoySelection>, String> {
    let inputs = inputs(state, key_images);
    monero_rust::native::fetch_decoys_with_rpc(&node.rpc(), inputs, state)
        .await
        .map(|drawn| drawn.decoys)
}

pub async fn build_unsigned_from_raw_output(
    node: &FakeNode,
    state: &mut WalletState,
    owned: &Owned,
    destination: &str,
    amount: u64,
) -> Result<String, String> {
    let rpc = node.rpc();
    let record = state
        .outputs()
        .iter()
        .find(|output| output.key_image == owned.key_image())
        .expect("the output is in wallet state");
    let bytes = hex::decode(&record.received_output_bytes).unwrap();
    let output = monero_wallet::WalletOutput::read(&mut bytes.as_slice()).unwrap();
    let global_index = record.global_index.unwrap();
    let own = monero_rust::derive_address(SEED, monero_rust::Network::Mainnet, "").unwrap();
    let unsigned = monero_rust::native::build_unsigned_transaction(
        &mut rand::rngs::OsRng,
        &rpc,
        rpc.get_rct_type().await.unwrap(),
        rpc.get_fee().await.unwrap().rate().unwrap(),
        &[(output, global_index)],
        vec![(
            monero_rust::MoneroAddress::from_str(monero_rust::Network::Mainnet, destination)
                .unwrap(),
            amount,
        )],
        Some(monero_rust::unsigned_tx::UnsignedChange::Own {
            address: own,
            subaddress: None,
        }),
        state,
    )
    .await?;
    Ok(hex::encode(unsigned.serialize()))
}

pub async fn relay(
    node: &FakeNode,
    state: &mut WalletState,
    transaction: &TransactionResult,
) -> Result<(), String> {
    monero_rust::native::broadcast_transaction_with_rpc(&node.rpc(), transaction, state, false)
        .await
}

pub async fn relay_raw(
    node: &FakeNode,
    state: &mut WalletState,
    tx_blob_hex: &str,
) -> Result<(), String> {
    monero_rust::native::broadcast_raw_transaction_with_rpc(&node.rpc(), tx_blob_hex, state, false)
        .await
}

pub fn scanned_block(height: u64, hash: &str, tx_blobs: &[&str]) -> BlockScanResult {
    let mut spent_key_images = Vec::new();
    let mut spent_key_image_tx_hashes = Vec::new();
    let mut spent_key_image_rings = Vec::new();
    for blob in tx_blobs {
        let bytes = hex::decode(blob).unwrap();
        let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
        for (key_image, ring) in monero_rust::scanner::extract_spent_rings_from_raw_tx(&bytes) {
            spent_key_images.push(key_image);
            spent_key_image_tx_hashes.push(hex::encode(tx.hash()));
            spent_key_image_rings.push(ring);
        }
    }
    BlockScanResult {
        block_height: height,
        block_hash: hash.to_string(),
        previous_block_hash: String::new(),
        block_timestamp: height * 120,
        tx_count: tx_blobs.len(),
        outputs: vec![],
        daemon_height: height + 1,
        spent_key_images,
        spent_key_image_tx_hashes,
        spent_key_image_rings,
    }
}

pub fn refresh_with(state: &mut WalletState, results: &[BlockScanResult]) -> bool {
    let from = results[0].block_height;
    let target = results[results.len() - 1].block_height + 1;
    match process_batch_with_reorg_detection(results, state, None, target, from).unwrap() {
        ScanBatchOutcome::Reorg(_) => true,
        ScanBatchOutcome::Normal(batch) => {
            state.add_outputs(batch.outputs_to_store);
            for block in results {
                state.mark_spent_detecting_conflicts(
                    &block.spent_key_images,
                    &block.spent_key_image_tx_hashes,
                    block.block_height,
                );
            }
            for (height, hash) in &batch.block_hashes {
                state.record_block_hash(*height, hash.clone());
            }
            false
        }
    }
}

pub fn cold_sign(unsigned_tx_hex: &str) -> monero_rust::native::OfflineSignResult {
    let preview = monero_rust::native::preview_unsigned_transaction_from_seed(
        SEED,
        unsigned_tx_hex,
        NETWORK,
        "",
    )
    .unwrap();
    let approval = preview.approve(u64::MAX, u64::MAX).unwrap();
    monero_rust::native::sign_unsigned_transaction(SEED, unsigned_tx_hex, NETWORK, "", &approval)
        .unwrap()
}

fn absolute(offsets: &[u64]) -> Vec<u64> {
    let mut index = 0;
    offsets
        .iter()
        .map(|offset| {
            index += offset;
            index
        })
        .collect()
}

pub fn rings_in_signed(tx_blob_hex: &str) -> BTreeMap<String, Vec<u64>> {
    let bytes = hex::decode(tx_blob_hex).unwrap();
    let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
    tx.prefix()
        .inputs
        .iter()
        .map(|input| match input {
            Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => (hex::encode(key_image.to_bytes()), absolute(key_offsets)),
            Input::Gen(_) => panic!("unexpected miner input"),
        })
        .collect()
}

pub fn rings_in_unsigned(unsigned_tx_hex: &str) -> BTreeMap<String, Vec<u64>> {
    let bytes = hex::decode(unsigned_tx_hex).unwrap();
    let unsigned = monero_rust::unsigned_tx::UnsignedTransaction::from_bytes(&bytes).unwrap();
    unsigned
        .inputs
        .iter()
        .map(|input| {
            (
                hex::encode(input.key().compress().to_bytes()),
                absolute(input.decoys().offsets()),
            )
        })
        .collect()
}

pub fn unsigned_ring(unsigned_tx_hex: &str, owned: &Owned) -> Vec<u64> {
    rings_in_unsigned(unsigned_tx_hex)
        .remove(&owned.output.key)
        .expect("envelope spends the output")
}

pub fn signed_ring(tx_blob_hex: &str, owned: &Owned) -> Vec<u64> {
    rings_in_signed(tx_blob_hex)
        .remove(owned.key_image())
        .expect("transaction spends the output")
}

pub fn ring_indexes(selection: &DecoySelection) -> Vec<u64> {
    absolute(&selection.offsets)
}

pub fn crafted_ring(owned: &Owned, decoys: &[u64]) -> InputRing {
    let mut members: Vec<(u64, [u8; 32], [u8; 32])> = decoys
        .iter()
        .map(|index| {
            let (key, commitment) = default_member(*index);
            (*index, key, commitment)
        })
        .collect();
    members.push((owned.global_index(), owned.key, owned.commitment));
    members.sort_by_key(|member| member.0);
    let real_index = members
        .iter()
        .position(|member| member.0 == owned.global_index())
        .unwrap() as u8;
    let mut previous = 0;
    let offsets = members
        .iter()
        .map(|member| {
            let offset = member.0 - previous;
            previous = member.0;
            offset
        })
        .collect();
    InputRing {
        key_image: owned.key_image().to_string(),
        ring: DecoySelection {
            real_index,
            offsets,
            ring: members
                .iter()
                .map(|member| RingMember {
                    key: hex::encode(member.1),
                    commitment: hex::encode(member.2),
                })
                .collect(),
        },
    }
}

pub const CANDIDATES_PER_INPUT: usize = 75;

pub fn candidate_intersection(log: &[Vec<u64>], real: u64) -> BTreeSet<u64> {
    let mut sets = log.iter().filter_map(|call| {
        call.chunks(CANDIDATES_PER_INPUT)
            .find(|chunk| chunk.contains(&real))
            .map(|chunk| chunk.iter().copied().collect::<BTreeSet<u64>>())
    });
    let first = sets.next().expect("the real output was requested");
    sets.fold(first, |acc, set| acc.intersection(&set).copied().collect())
}

#[track_caller]
pub fn assert_same_ring(what: &str, first: &[u64], second: &[u64]) {
    let shared = first.iter().filter(|index| second.contains(index)).count();
    assert_eq!(
        first,
        second,
        "{what}: the same key image was given two different rings, sharing {shared} of {} \
         members. An observer of both learns the real output.",
        first.len()
    );
}

#[track_caller]
pub fn assert_daemon_saw_one_ring(node: &FakeNode, owned: &Owned) {
    let rings: Vec<Vec<u64>> = node
        .seen_txs()
        .iter()
        .filter_map(|tx| rings_in_signed(tx).remove(owned.key_image()))
        .collect();
    assert!(!rings.is_empty(), "the daemon saw no spend of the output");
    for ring in &rings[1..] {
        assert_same_ring("two transactions handed to the daemon", &rings[0], ring);
    }
}
