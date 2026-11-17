//! Decoy and input selection compared statistically with wallet2. The samplers
//! feed `scripts/stat-parity/` (see its README.md).

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::coin_selection::select_inputs_with_fee_and_rng;
use monero_rust::monero_rpc::{
    FeePriority, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
};
use monero_rust::native::build_unsigned_transaction;
use monero_rust::scanner::{derive_address, scan_blocks_batch, Lookahead};
use monero_rust::wallet_state::is_spendable_at;
use monero_rust::{
    prepare_sweep_transactions, RingsNotPersisted, SweepSubaddresses, WalletOutput, WalletState,
};
use monero_wallet::{interface::FeeRate, ringct::RctType};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};

const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;

const WALLET2_AGE_SHARES: [(u64, f64); 7] = [
    (10, 0.008028),
    (13, 0.012505),
    (23, 0.039662),
    (99, 0.123518),
    (999, 0.339389),
    (9_999, 0.314248),
    (u64::MAX, 0.162649),
];

const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

#[derive(Deserialize)]
struct GetOutsRequest {
    outputs: Vec<GetOut>,
}

#[derive(Deserialize)]
struct GetOut {
    index: u64,
}

#[derive(Serialize, Deserialize)]
struct GetOutsResponse {
    outs: Vec<OutKey>,
    status: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct OutKey {
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
struct DistributionResponse {
    distributions: Vec<DistributionEntry>,
    status: String,
}

#[derive(Serialize)]
struct DistributionEntry {
    amount: u64,
    start_height: u64,
    binary: bool,
    compress: bool,
    #[serde(with = "serde_bytes")]
    compressed_data: Vec<u8>,
    base: u64,
}

#[derive(Clone)]
struct DistributionNode {
    height: usize,
    distribution_reply: Arc<Vec<u8>>,
    real: Arc<Mutex<HashMap<u64, ([u8; 32], [u8; 32])>>>,
    decoy_point: [u8; 32],
    calls: Arc<Mutex<Vec<Vec<u64>>>>,
}

impl std::fmt::Debug for DistributionNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DistributionNode")
    }
}

#[async_trait]
impl RpcConnection for DistributionNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        match route {
            "get_height" => Ok(format!("{{\"height\":{},\"status\":\"OK\"}}", self.height).into()),
            "get_output_distribution.bin" => Ok(self.distribution_reply.as_ref().clone()),
            "get_outs.bin" => {
                let request: GetOutsRequest = monero_epee_bin_serde::from_bytes(&body)
                    .map_err(|_| RpcError::InternalError("harness: bad get_outs request"))?;
                let indexes: Vec<u64> = request.outputs.iter().map(|o| o.index).collect();
                let real = self.real.lock().unwrap().clone();
                let outs = indexes
                    .iter()
                    .map(|index| {
                        let (key, mask) = real
                            .get(index)
                            .copied()
                            .unwrap_or((self.decoy_point, self.decoy_point));
                        OutKey {
                            key: key.to_vec(),
                            mask: mask.to_vec(),
                            unlocked: true,
                            height: 1,
                            txid: vec![0; 32],
                        }
                    })
                    .collect::<Vec<_>>();
                self.calls.lock().unwrap().push(indexes);
                monero_epee_bin_serde::to_bytes(&GetOutsResponse {
                    outs,
                    status: "OK".into(),
                })
                .map_err(|_| RpcError::InternalError("harness: get_outs reply"))
            }
            _ => Err(RpcError::InternalError("harness: unexpected route")),
        }
    }
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn synthetic_output(
    global_index: u64,
    n: u64,
) -> (monero_wallet::WalletOutput, [u8; 32], [u8; 32]) {
    let key = (&Scalar::from(1_000_003u64 + n) * ED25519_BASEPOINT_TABLE).compress();
    let mask = Scalar::from(77u64 + n);
    let amount = 1_000_000_000_000u64;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[n as u8; 32]); // transaction hash
    bytes.extend_from_slice(&0u64.to_le_bytes()); // index in transaction
    bytes.extend_from_slice(&global_index.to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(&Scalar::from(5u64).to_bytes()); // key offset
    bytes.extend_from_slice(&mask.to_bytes());
    bytes.extend_from_slice(&amount.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]); // no timelock, subaddress, payment id, data
    let output =
        monero_wallet::WalletOutput::read(&mut bytes.as_slice()).expect("synthetic output");
    let commitment = output.commitment().commit().compress().to_bytes();
    (output, key.to_bytes(), commitment)
}

fn owned_record(output: &monero_wallet::WalletOutput, global_index: u64, n: u64) -> WalletOutput {
    serde_json::from_value(serde_json::json!({
        "tx_hash": hex::encode([n as u8; 32]),
        "output_index": 0,
        "amount": output.commitment().amount,
        "key": hex::encode(output.key().compress().to_bytes()),
        "key_offset": hex::encode(Scalar::from(5u64).to_bytes()),
        "commitment_mask": hex::encode(Scalar::from(77u64 + n).to_bytes()),
        "subaddress_index": null,
        "payment_id": null,
        "received_output_bytes": hex::encode(output.serialize()),
        "global_index": global_index,
        "block_height": 1,
        "spent": false,
        "key_image": hex::encode((&Scalar::from(3_000_017u64 + n) * ED25519_BASEPOINT_TABLE).compress().to_bytes()),
        "is_coinbase": false,
    }))
    .expect("wallet output record")
}

fn fresh_state(outputs: Vec<WalletOutput>) -> WalletState {
    let mut wallet_state = WalletState::new();
    wallet_state.set_ring_persister(Box::new(RingsNotPersisted));
    wallet_state.add_outputs(outputs);
    wallet_state
}

struct Offline {
    rpc: Rpc<DistributionNode>,
    real: Arc<Mutex<HashMap<u64, ([u8; 32], [u8; 32])>>>,
    calls: Arc<Mutex<Vec<Vec<u64>>>>,
    owned: Mutex<Vec<WalletOutput>>,
    bound: u64,
}

impl Offline {
    fn new(cumulative: &[u64]) -> Offline {
        let mut compressed = Vec::with_capacity(cumulative.len() * 2);
        let mut previous = 0u64;
        for value in cumulative {
            varint(value - previous, &mut compressed);
            previous = *value;
        }
        let distribution_reply = monero_epee_bin_serde::to_bytes(&DistributionResponse {
            distributions: vec![DistributionEntry {
                amount: 0,
                start_height: 0,
                binary: true,
                compress: true,
                compressed_data: compressed,
                base: 0,
            }],
            status: "OK".into(),
        })
        .unwrap();
        let real = Arc::new(Mutex::new(HashMap::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let node = DistributionNode {
            height: cumulative.len(),
            distribution_reply: Arc::new(distribution_reply),
            real: real.clone(),
            decoy_point: (&Scalar::from(9u64) * ED25519_BASEPOINT_TABLE)
                .compress()
                .to_bytes(),
            calls: calls.clone(),
        };
        Offline {
            rpc: Rpc::new_with_connection(node),
            real,
            calls,
            owned: Mutex::new(Vec::new()),
            bound: cumulative[cumulative.len() - 10],
        }
    }

    fn own(&self, indexes: &[u64]) -> Vec<(monero_wallet::WalletOutput, u64)> {
        let mut map = self.real.lock().unwrap();
        map.clear();
        let mut owned = self.owned.lock().unwrap();
        owned.clear();
        let mut spent = Vec::with_capacity(indexes.len());
        for (n, global_index) in indexes.iter().enumerate() {
            let (output, key, commitment) = synthetic_output(*global_index, n as u64);
            map.insert(*global_index, (key, commitment));
            owned.push(owned_record(&output, *global_index, n as u64));
            spent.push((output, *global_index));
        }
        spent
    }

    async fn build(
        &self,
        rng: &mut ChaCha20Rng,
        spent: &[(monero_wallet::WalletOutput, u64)],
    ) -> (
        Result<monero_rust::unsigned_tx::UnsignedTransaction, String>,
        Vec<Vec<u64>>,
    ) {
        self.calls.lock().unwrap().clear();
        let mut wallet_state = fresh_state(self.owned.lock().unwrap().clone());
        let built = build_unsigned_transaction(
            rng,
            &self.rpc,
            RctType::ClsagBulletproofPlus,
            FeeRate::new(20_000, 10_000).unwrap(),
            spent,
            vec![(derive_address(SEED, NETWORK, "").unwrap(), 1_000_000_000)],
            None,
            &mut wallet_state,
        )
        .await;
        (built, std::mem::take(&mut *self.calls.lock().unwrap()))
    }
}

fn ring_members(input: &monero_wallet::OutputWithDecoys) -> Vec<u64> {
    let mut index = 0u64;
    input
        .decoys()
        .offsets()
        .iter()
        .map(|offset| {
            index += offset;
            index
        })
        .collect()
}

fn cumulative_of(per_block: &[u64]) -> Vec<u64> {
    let mut total = 0u64;
    per_block
        .iter()
        .map(|count| {
            total += count;
            total
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn rings_and_candidate_requests_have_wallet2s_shape() {
    let mut per_block = vec![8u64; 3000];
    for block in (2900..3000).step_by(3) {
        per_block[block] = 0;
    }
    let cumulative = cumulative_of(&per_block);
    let node = Offline::new(&cumulative);
    let reals = [node.bound / 3, node.bound - 1];
    let spent = node.own(&reals);
    let mut rng = ChaCha20Rng::seed_from_u64(1);
    for _ in 0..12 {
        let (built, calls) = node.build(&mut rng, &spent).await;
        let unsigned = built.expect("a ring for each input");
        assert!(
            (1..=3).contains(&calls.len()),
            "one request per sanity attempt"
        );
        for call in &calls {
            assert_eq!(call.len(), 150, "75 candidates per input");
            for (segment, real) in call.chunks(75).zip(reals) {
                assert!(
                    segment.windows(2).all(|pair| pair[0] < pair[1]),
                    "sorted and distinct"
                );
                assert!(
                    segment.contains(&real),
                    "the real output is requested with its decoys"
                );
                for index in segment {
                    assert!(
                        *index < node.bound,
                        "candidate {index} is too young to spend"
                    );
                    let block = cumulative.partition_point(|total| total <= index);
                    assert_ne!(
                        per_block[block], 0,
                        "candidate from a block without outputs"
                    );
                }
            }
        }
        let last = calls.last().unwrap();
        for ((input, real), segment) in unsigned.inputs.iter().zip(reals).zip(last.chunks(75)) {
            let ring = ring_members(input);
            assert_eq!(ring.len(), 16);
            assert!(
                ring.windows(2).all(|pair| pair[0] < pair[1]),
                "sorted and distinct"
            );
            assert_eq!(ring[usize::from(input.decoys().signer_index())], real);
            assert!(ring.iter().all(|member| segment.contains(member)));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn candidate_ages_follow_wallet2s_picker() {
    const CLASSES: [(u64, f64); 7] = WALLET2_AGE_SHARES;
    const BUILDS: u64 = 20;
    let per_block = vec![10u64; 100_000];
    let cumulative = cumulative_of(&per_block);
    let node = Offline::new(&cumulative);
    let reals: Vec<u64> = (0..8u64).map(|n| node.bound / 2 + n * 1000).collect();
    let spent = node.own(&reals);
    let mut rng = ChaCha20Rng::seed_from_u64(20261118);
    let mut counts = [0u64; 7];
    let mut total = 0u64;
    for _ in 0..BUILDS {
        let (built, calls) = node.build(&mut rng, &spent).await;
        built.expect("rings");
        for (segment, real) in calls[0].chunks(75).zip(&reals) {
            for index in segment.iter().filter(|index| *index != real) {
                let age = 99_999 - index / 10;
                let class = CLASSES.iter().position(|(upper, _)| age <= *upper).unwrap();
                counts[class] += 1;
                total += 1;
            }
        }
    }
    assert_eq!(total, BUILDS * 8 * 74);
    for ((upper, expected), count) in CLASSES.iter().zip(counts) {
        let share = count as f64 / total as f64;
        let tolerance = 5.0 * (expected * (1.0 - expected) / total as f64).sqrt();
        assert!(
            (share - expected).abs() <= tolerance,
            "share of candidates up to age {upper}: {share:.5}, wallet2 {expected:.5} +/- {tolerance:.5}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known defect: no ring is built while fewer than 76 outputs are older than the coinbase lock, where wallet2 requests every output"]
async fn ring_is_built_on_a_chain_of_eighty_blocks() {
    let cumulative = cumulative_of(&[1u64; 80]);
    let node = Offline::new(&cumulative);
    let spent = node.own(&[5]);
    let mut rng = ChaCha20Rng::seed_from_u64(1);
    let mut every: Vec<u64> = (0..71).collect();
    every.extend([70; 4]);
    for _ in 0..5 {
        let (built, calls) = node.build(&mut rng, &spent).await;
        let unsigned = built.expect("wallet2 builds a ring from the 71 spendable outputs");
        assert_eq!(ring_members(&unsigned.inputs[0]).len(), 16);
        assert_eq!(calls, vec![every.clone()]);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known defect: candidate picking gives up after 1500 draws on a chain of about 90 blocks, where wallet2 keeps drawing"]
async fn ring_is_built_when_most_draws_miss_the_chain() {
    let cumulative = cumulative_of(&[1u64; 90]);
    let node = Offline::new(&cumulative);
    let spent = node.own(&[5]);
    let mut rng = ChaCha20Rng::seed_from_u64(1);
    for _ in 0..10 {
        let (built, _) = node.build(&mut rng, &spent).await;
        let unsigned = built.expect("wallet2 finds 74 candidates among the 81 spendable outputs");
        assert_eq!(ring_members(&unsigned.inputs[0]).len(), 16);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn picker_offline() {
    let Ok(distribution) = std::env::var("STATP_DIST") else {
        eprintln!("Skipping picker_offline: STATP_DIST is unset");
        return;
    };
    let raw = std::fs::read(distribution).expect("distribution file");
    let cumulative: Vec<u64> = raw
        .chunks_exact(8)
        .map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    let prefix = env("STATP_OUT");
    let seed: u64 = env("STATP_SEED").parse().unwrap();
    let builds: usize = env("STATP_BUILDS").parse().unwrap();
    let inputs: usize = env_or("STATP_INPUTS", "8").parse().unwrap();

    let node = Offline::new(&cumulative);
    let bound = node.bound;
    let mut supplied_reals: Option<std::vec::IntoIter<u64>> =
        std::env::var("STATP_REALS").ok().map(|path| {
            std::fs::read(path)
                .expect("reals file")
                .chunks_exact(8)
                .map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()))
                .filter(|index| *index < bound)
                .collect::<Vec<_>>()
                .into_iter()
        });
    let mut spent = Vec::new();
    let mut real_indexes: Vec<u64> = Vec::new();
    if supplied_reals.is_none() {
        let spacing = if bound / 2 + 1000 * inputs as u64 <= bound {
            1000
        } else {
            (bound / 64).max(1)
        };
        real_indexes = (0..inputs as u64)
            .map(|n| bound / 2 + n * spacing)
            .collect();
        spent = node.own(&real_indexes);
    }

    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut cand =
        std::io::BufWriter::new(std::fs::File::create(format!("{prefix}.cand.u64")).unwrap());
    let mut ring =
        std::io::BufWriter::new(std::fs::File::create(format!("{prefix}.ring.u64")).unwrap());
    let mut attempts_histogram = [0u64; 4];
    let mut errors: HashMap<String, u64> = HashMap::new();
    let mut signer_positions = [0u64; 16];
    for _ in 0..builds {
        if let Some(reals) = supplied_reals.as_mut() {
            real_indexes.clear();
            while real_indexes.len() < inputs {
                let index = reals.next().expect("STATP_REALS ran out of indexes");
                if !real_indexes.contains(&index) {
                    real_indexes.push(index);
                }
            }
            spent = node.own(&real_indexes);
        }
        let (built, recorded) = node.build(&mut rng, &spent).await;
        attempts_histogram[recorded.len().min(3)] += 1;
        for call in &recorded {
            for (input, segment) in call.chunks(75).enumerate() {
                for index in segment {
                    if *index != real_indexes[input % inputs] {
                        cand.write_all(&index.to_le_bytes()).unwrap();
                    }
                }
            }
        }
        match built {
            Ok(unsigned) => {
                for input in &unsigned.inputs {
                    let signer = usize::from(input.decoys().signer_index());
                    signer_positions[signer] += 1;
                    for (position, index) in ring_members(input).iter().enumerate() {
                        if position != signer {
                            ring.write_all(&index.to_le_bytes()).unwrap();
                        }
                    }
                }
            }
            Err(error) => *errors.entry(error).or_default() += 1,
        }
    }
    cand.flush().unwrap();
    ring.flush().unwrap();
    std::fs::write(
        format!("{prefix}.meta.json"),
        serde_json::to_string(&serde_json::json!({
            "seed": seed, "builds": builds, "inputs": inputs, "bound": bound,
            "real_indexes": if supplied_reals.is_some() { Vec::new() } else { real_indexes.clone() },
            "get_outs_calls_per_build_histogram": attempts_histogram,
            "signer_positions": signer_positions,
            "errors": errors,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[derive(Clone, Debug)]
struct RecordingNode {
    client: reqwest::Client,
    url: String,
    calls: Arc<Mutex<Vec<(Vec<u64>, Vec<bool>)>>>,
    transport_retries: Arc<std::sync::atomic::AtomicU64>,
}

#[async_trait]
impl RpcConnection for RecordingNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let request = (route == "get_outs.bin").then(|| body.clone());
        let mut tries = 0;
        let reply = loop {
            let attempt = async {
                self.client
                    .post(format!("{}/{}", self.url, route))
                    .body(body.clone())
                    .send()
                    .await?
                    .bytes()
                    .await
            };
            match attempt.await {
                Ok(bytes) => break bytes.to_vec(),
                Err(_) if tries < 200 => {
                    tries += 1;
                    self.transport_retries
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(_) => return Err(RpcError::ConnectionError),
            }
        };
        if let Some(request) = request {
            let request: GetOutsRequest = monero_epee_bin_serde::from_bytes(&request)
                .map_err(|_| RpcError::InternalError("harness: bad get_outs request"))?;
            let response: GetOutsResponse = monero_epee_bin_serde::from_bytes(&reply)
                .map_err(|_| RpcError::InternalError("harness: bad get_outs reply"))?;
            self.calls.lock().unwrap().push((
                request.outputs.iter().map(|o| o.index).collect(),
                response.outs.iter().map(|o| o.unlocked).collect(),
            ));
        }
        Ok(reply)
    }
}

#[derive(Deserialize)]
struct Spec {
    node: String,
    mnemonic: String,
    lookahead: (u32, u32),
    seed: u64,
    out: String,
    cells: Vec<Cell>,
}

#[derive(Deserialize)]
struct Cell {
    kind: String,
    tag: String,
    n: usize,
    #[serde(default)]
    priority: u32,
    #[serde(default)]
    account: u32,
    #[serde(default)]
    amounts: Vec<u64>,
    #[serde(default)]
    transfers: Vec<u64>,
    #[serde(default)]
    subaddresses: Option<String>,
}

fn priority(value: u32) -> FeePriority {
    match value {
        0 => FeePriority::Automatic,
        1 => FeePriority::Low,
        2 => FeePriority::Normal,
        3 => FeePriority::Elevated,
        _ => FeePriority::Priority,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn regtest_samples() {
    let Ok(spec_path) = std::env::var("STATP_SPEC") else {
        eprintln!("Skipping regtest_samples: STATP_SPEC is unset");
        return;
    };
    let spec: Spec = serde_json::from_str(&std::fs::read_to_string(spec_path).unwrap()).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let transport_retries = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let rpc = Rpc::new_with_connection(RecordingNode {
        client: reqwest::Client::new(),
        url: spec.node.trim_end_matches('/').to_string(),
        calls: calls.clone(),
        transport_retries: transport_retries.clone(),
    });
    let mut out = std::io::BufWriter::new(std::fs::File::create(&spec.out).unwrap());
    let lookahead = Lookahead {
        account: spec.lookahead.0,
        subaddress: spec.lookahead.1,
    };

    let height = rpc.get_height().await.expect("get_height") as u64;
    let mut outputs: Vec<WalletOutput> = Vec::new();
    let mut next = 0u64;
    while next < height {
        let batch = scan_blocks_batch(&rpc, next, &spec.mnemonic, NETWORK, lookahead, false, "")
            .await
            .expect("scan");
        let Some(last) = batch.last() else { break };
        next = last.block_height + 1;
        for block in batch {
            outputs.extend(block.outputs);
        }
    }
    let address = derive_address(&spec.mnemonic, NETWORK, "").unwrap();
    writeln!(
        out,
        "{}",
        serde_json::json!({"ev": "restored", "height": height, "n_transfers": outputs.len(),
                           "address": address.to_string(), "seed": spec.seed})
    )
    .unwrap();
    for (i, output) in outputs.iter().enumerate() {
        let (major, minor) = output.subaddress_index.unwrap_or((0, 0));
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "ev": "transfer", "i": i, "amount": output.amount, "height": output.block_height,
                "gidx": output.global_index, "major": major, "minor": minor,
                "txid": output.tx_hash, "out": output.output_index, "ki": output.key_image,
                "spent": output.spent as u8, "frozen": output.frozen as u8,
                "unlocked": is_spendable_at(output, height, None) as u8,
                "coinbase": output.is_coinbase as u8,
            })
        )
        .unwrap();
    }
    writeln!(out, "{}", serde_json::json!({"ev": "transfers_end"})).unwrap();

    let position: HashMap<String, usize> = outputs
        .iter()
        .enumerate()
        .map(|(i, output)| (output.output_key(), i))
        .collect();
    let spendable: Vec<WalletOutput> = outputs
        .iter()
        .filter(|o| !o.spent && !o.frozen && is_spendable_at(o, height, None))
        .cloned()
        .collect();
    let rct_type = rpc.get_rct_type().await.expect("rct type");
    let to_spend = |output: &WalletOutput| {
        let bytes = hex::decode(&output.received_output_bytes).expect("hex");
        (
            monero_wallet::WalletOutput::read(&mut bytes.as_slice()).expect("stored output"),
            output.global_index.expect("global index"),
        )
    };

    let mut rng = ChaCha20Rng::seed_from_u64(spec.seed);
    let mut sample_no = 0u64;
    for cell in &spec.cells {
        let fee = rpc
            .get_fee_checked_for_priority_at_height(
                priority(cell.priority),
                Some(height),
                DEFAULT_MAX_FEE_PER_BYTE,
            )
            .await;
        let fee_rate = match fee {
            Ok(fee) => fee.rate().expect("fee rate"),
            Err(error) => {
                for k in 0..cell.n {
                    writeln!(
                        out,
                        "{}",
                        serde_json::json!({"ev": "sample", "tag": cell.tag, "k": k, "ok": false,
                                           "err": format!("Failed to get fee rate: {error:?}"), "txs": []})
                    )
                    .unwrap();
                }
                writeln!(
                    out,
                    "{}",
                    serde_json::json!({"ev": "cell_end", "tag": cell.tag})
                )
                .unwrap();
                continue;
            }
        };
        let total: u64 = cell.amounts.iter().sum();
        let payments: Vec<_> = cell
            .amounts
            .iter()
            .map(|amount| (address, *amount))
            .collect();
        let plans: Vec<Option<usize>> = if cell.kind == "ring" {
            cell.transfers
                .iter()
                .map(|gidx| {
                    Some(
                        outputs
                            .iter()
                            .position(|o| o.global_index == Some(*gidx))
                            .expect("ring cell names an output this wallet did not scan"),
                    )
                })
                .collect()
        } else {
            vec![None]
        };
        for plan in plans {
            for k in 0..cell.n {
                let mut row =
                    serde_json::json!({"ev": "sample", "tag": cell.tag, "k": k, "n": sample_no});
                sample_no += 1;
                if let Some(i) = plan {
                    row["gidx"] = outputs[i].global_index.into();
                }
                if cell.kind == "sweep" {
                    let subaddresses = match cell.subaddresses.as_deref() {
                        Some("all") => SweepSubaddresses::All,
                        _ => SweepSubaddresses::OneAtRandom,
                    };
                    match prepare_sweep_transactions(
                        &mut rng,
                        &outputs,
                        height,
                        None,
                        subaddresses,
                        None,
                        cell.account,
                        rct_type,
                        fee_rate,
                    ) {
                        Ok(prepared) => {
                            let mut txs = Vec::new();
                            for transaction in prepared {
                                let sel: Vec<usize> = transaction
                                    .spent_output_keys
                                    .iter()
                                    .map(|key| position[key])
                                    .collect();
                                let mut tx = serde_json::json!({"sel": sel});
                                if k == 0 {
                                    let mut wallet_state = fresh_state(
                                        transaction
                                            .spent_output_keys
                                            .iter()
                                            .map(|key| outputs[position[key]].clone())
                                            .collect(),
                                    );
                                    match monero_rust::native::sweep_all_with_rpc(
                                        &rpc,
                                        &spec.mnemonic,
                                        "mainnet",
                                        transaction.stored_outputs,
                                        &address.to_string(),
                                        "",
                                        &mut wallet_state,
                                    )
                                    .await
                                    {
                                        Ok(result) => {
                                            tx["tx_bytes"] = (result.tx_blob.len() / 2).into();
                                            tx["fee"] = result.fee.into();
                                        }
                                        Err(error) => tx["build_error"] = error.into(),
                                    }
                                }
                                txs.push(tx);
                            }
                            row["ok"] = true.into();
                            row["err"] = "".into();
                            row["txs"] = txs.into();
                        }
                        Err(error) => {
                            row["ok"] = false.into();
                            row["err"] = error.into();
                            row["txs"] = serde_json::json!([]);
                        }
                    }
                    writeln!(out, "{row}").unwrap();
                    continue;
                }
                let selected: Result<Vec<WalletOutput>, String> = match (cell.kind.as_str(), plan) {
                    ("ring", Some(i)) => Ok(vec![outputs[i].clone()]),
                    _ => select_inputs_with_fee_and_rng(
                        &mut rng,
                        &spendable,
                        total,
                        cell.amounts.len(),
                        false,
                        None,
                        cell.account,
                        rct_type,
                        fee_rate,
                    )
                    .map(|result| result.selected),
                };
                let selected = match selected {
                    Ok(selected) => selected,
                    Err(error) => {
                        row["ok"] = false.into();
                        row["err"] = error.into();
                        row["txs"] = serde_json::json!([]);
                        writeln!(out, "{row}").unwrap();
                        continue;
                    }
                };
                let sel: Vec<usize> = selected.iter().map(|o| position[&o.output_key()]).collect();
                let mut tx = serde_json::json!({"sel": sel, "fee_rate": fee_rate.calculate_fee_from_weight(1u64)});
                row["ok"] = true.into();
                row["err"] = "".into();
                if cell.kind == "select" {
                    row["txs"] = serde_json::json!([tx]);
                    writeln!(out, "{row}").unwrap();
                    continue;
                }
                calls.lock().unwrap().clear();
                let spend: Vec<_> = selected.iter().map(|o| to_spend(o)).collect();
                let mut wallet_state = fresh_state(selected.clone());
                let built = build_unsigned_transaction(
                    &mut rng,
                    &rpc,
                    rct_type,
                    fee_rate,
                    &spend,
                    payments.clone(),
                    None,
                    &mut wallet_state,
                )
                .await;
                let recorded = std::mem::take(&mut *calls.lock().unwrap());
                row["calls"] = recorded
                    .iter()
                    .map(|(indexes, unlocked)| {
                        serde_json::json!({
                            "idx": indexes,
                            "unl": unlocked.iter().map(|u| if *u { '1' } else { '0' }).collect::<String>(),
                        })
                    })
                    .collect::<Vec<_>>()
                    .into();
                match built {
                    Ok(unsigned) => {
                        tx["inputs"] = unsigned
                            .inputs
                            .iter()
                            .zip(&sel)
                            .map(|(input, t)| serde_json::json!({"t": t, "ring": ring_members(input)}))
                            .collect::<Vec<_>>()
                            .into();
                        row["txs"] = serde_json::json!([tx]);
                    }
                    Err(error) => {
                        row["ok"] = false.into();
                        row["err"] = error.into();
                        row["txs"] = serde_json::json!([]);
                    }
                }
                writeln!(out, "{row}").unwrap();
            }
        }
        writeln!(
            out,
            "{}",
            serde_json::json!({"ev": "cell_end", "tag": cell.tag})
        )
        .unwrap();
        out.flush().unwrap();
    }
    writeln!(
        out,
        "{}",
        serde_json::json!({"ev": "end", "transport_retries": transport_retries.load(std::sync::atomic::Ordering::Relaxed)})
    )
    .unwrap();
}
