//! Tool for `scripts/prerct_chain.py`; does nothing unless `PRERCT_REQUEST`
//! (and `PRERCT_RESPONSE`) name the request and response files.

#![cfg(not(target_arch = "wasm32"))]

use monero_rust::monero_rpc::{FeePriority, HttpRpc, Rpc, DEFAULT_MAX_FEE_PER_BYTE};
use monero_rust::native::{
    broadcast_raw_transaction_with_rpc, create_transaction_with_rpc_and_policy,
    sweep_all_with_rpc_and_policy, FeePolicy, TransactionResult,
};
use monero_rust::scanner::{scan_blocks_batch, scan_blocks_batch_with_history, Lookahead};
use monero_rust::{
    prepare_send_inputs_with_rpc, prepare_sweep_inputs_with_rpc,
    process_batch_with_reorg_detection, BlockScanResult, ScanBatchOutcome, WalletOutput,
    WalletState,
};
use serde::{Deserialize, Serialize};

/// Regtest uses mainnet address encoding.
const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;

#[derive(Deserialize)]
struct Request {
    rpc: String,
    seed: String,
    #[serde(default)]
    start_height: u64,
    #[serde(default)]
    stop_height: Option<u64>,
    #[serde(default)]
    resume: bool,
    /// `batch` (by height) or `history` (from the wallet's chain history).
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_account_lookahead")]
    lookahead_account: u32,
    #[serde(default = "default_subaddress_lookahead")]
    lookahead_subaddress: u32,
    #[serde(default)]
    seed_genesis: bool,
    op: String,
    #[serde(default)]
    source_account: u32,
    #[serde(default)]
    recipients: Vec<(String, u64)>,
    #[serde(default)]
    inputs: Option<Vec<String>>,
    #[serde(default)]
    broadcast: bool,
}

fn default_mode() -> String {
    "batch".to_string()
}

fn default_account_lookahead() -> u32 {
    3
}

fn default_subaddress_lookahead() -> u32 {
    10
}

#[derive(Serialize)]
struct BlockSeen {
    height: u64,
    hash: String,
    timestamp: u64,
    tx_count: usize,
    outputs: usize,
    key_images: usize,
}

#[derive(Serialize)]
struct Build {
    inputs: Vec<String>,
    input_total: u64,
    estimated_fee: u64,
    result: TransactionResult,
}

#[derive(Serialize, Default)]
struct Response {
    daemon_height: u64,
    rounds: usize,
    passes: Vec<u64>,
    scanned_to: u64,
    scan_error: Option<String>,
    reorgs: Vec<String>,
    blocks: Vec<BlockSeen>,
    key_images: Vec<(u64, String, String)>,
    outputs: Vec<WalletOutput>,
    balance_confirmed: u64,
    balance_unconfirmed: u64,
    spendable: Vec<String>,
    short_chain_history: Vec<(u64, String)>,
    prepare_error: Option<String>,
    build: Option<Build>,
    build_error: Option<String>,
    broadcast: Option<String>,
}

fn fold(
    state: &mut WalletState,
    response: &mut Response,
    results: &[BlockScanResult],
    target: u64,
) {
    let Some(first) = results.first() else {
        return;
    };
    match process_batch_with_reorg_detection(results, state, None, target, first.block_height) {
        Ok(ScanBatchOutcome::Normal(batch)) => {
            state.add_outputs(batch.outputs_to_store.clone());
            for result in results {
                state.mark_spent_by_key_images_at_height(
                    &result.spent_key_images,
                    result.block_height,
                );
                state.record_block_hash(result.block_height, result.block_hash.clone());
                response.blocks.push(BlockSeen {
                    height: result.block_height,
                    hash: result.block_hash.clone(),
                    timestamp: result.block_timestamp,
                    tx_count: result.tx_count,
                    outputs: result.outputs.len(),
                    key_images: result.spent_key_images.len(),
                });
                for (image, tx) in result
                    .spent_key_images
                    .iter()
                    .zip(&result.spent_key_image_tx_hashes)
                {
                    response
                        .key_images
                        .push((result.block_height, image.clone(), tx.clone()));
                }
            }
            state.current_height = batch.batch_end_height.saturating_sub(1);
            state.daemon_height = batch.daemon_height;
        }
        Ok(ScanBatchOutcome::Reorg(info)) => {
            response.reorgs.push(format!("{info:?}"));
        }
        Err(error) => response.scan_error = Some(format!("reorg detection: {error}")),
    }
}

async fn scan_wallet(
    rpc: &Rpc<HttpRpc>,
    request: &Request,
    response: &mut Response,
) -> WalletState {
    let lookahead = Lookahead {
        account: request.lookahead_account,
        subaddress: request.lookahead_subaddress,
    };
    let mut state = WalletState::new();
    let daemon_height = match rpc.get_height().await {
        Ok(height) => height as u64,
        Err(error) => {
            response.scan_error = Some(format!("get_height: {error:?}"));
            return state;
        }
    };
    response.daemon_height = daemon_height;
    if request.seed_genesis {
        match rpc.get_block_hash(0).await {
            Ok(hash) => state.record_block_hash(0, hex::encode(hash)),
            Err(error) => {
                response.scan_error = Some(format!("genesis hash: {error:?}"));
                return state;
            }
        }
    }
    let first_target = request
        .stop_height
        .unwrap_or(daemon_height)
        .min(daemon_height);
    let mut targets = vec![first_target];
    if request.resume && first_target < daemon_height {
        targets.push(daemon_height);
    }
    let mut next = request.start_height;
    let mut reorgs_seen = 0;

    for target in targets {
        while next < target && response.rounds < 10_000 {
            response.rounds += 1;
            let history = state.get_short_chain_history();
            let results = if request.mode == "history" && response.rounds > 1 {
                scan_blocks_batch_with_history(
                    rpc,
                    0,
                    &history,
                    &request.seed,
                    NETWORK,
                    lookahead,
                    false,
                    "",
                )
                .await
                .map(|(results, _start)| results)
            } else if request.mode == "history" {
                scan_blocks_batch_with_history(
                    rpc,
                    next,
                    &[],
                    &request.seed,
                    NETWORK,
                    lookahead,
                    false,
                    "",
                )
                .await
                .map(|(results, _start)| results)
            } else {
                scan_blocks_batch(rpc, next, &request.seed, NETWORK, lookahead, false, "").await
            };
            let mut results = match results {
                Ok(results) => results,
                Err(error) => {
                    response.scan_error =
                        Some(format!("round {} from {next}: {error}", response.rounds));
                    break;
                }
            };
            results.retain(|result| result.block_height < target);
            let Some(last) = results.last() else {
                break;
            };
            let last_height = last.block_height;
            fold(&mut state, response, &results, target);
            if response.scan_error.is_some() {
                break;
            }
            if response.reorgs.len() > reorgs_seen {
                reorgs_seen = response.reorgs.len();
                if reorgs_seen > 5 {
                    response.scan_error = Some("reorg loop".to_string());
                    break;
                }
                next = state.current_height;
                continue;
            }
            if last_height < next {
                response.scan_error = Some(format!("no progress at {next}"));
                break;
            }
            next = last_height + 1;
        }
        if response.scan_error.is_some() {
            break;
        }
        response.passes.push(next);
    }

    response.scanned_to = next;
    response.short_chain_history = state.get_short_chain_history();
    response.outputs = state.outputs().to_vec();
    let balance = state.balance_at_height(daemon_height);
    response.balance_confirmed = balance.confirmed;
    response.balance_unconfirmed = balance.unconfirmed;
    response.spendable = state
        .spendable_outputs_at_height(daemon_height)
        .iter()
        .map(|output| output.output_key())
        .collect();
    state
}

#[tokio::test(flavor = "multi_thread")]
async fn prerct_scan_tool() {
    let Ok(request_path) = std::env::var("PRERCT_REQUEST") else {
        eprintln!("Skipping the pre-RingCT scan tool: PRERCT_REQUEST is unset");
        return;
    };
    let response_path = std::env::var("PRERCT_RESPONSE").expect("PRERCT_RESPONSE");
    let request: Request =
        serde_json::from_str(&std::fs::read_to_string(&request_path).expect("read request"))
            .expect("parse request");

    let rpc = HttpRpc::new(request.rpc.clone()).expect("regtest RPC");
    let mut response = Response::default();
    let state = scan_wallet(&rpc, &request, &mut response).await;
    let outputs = state.outputs().to_vec();
    let height = response.daemon_height;

    if request.op != "scan" {
        let policy = FeePolicy {
            priority: FeePriority::Normal,
            wallet_height: Some(height),
            max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
        };
        let prepared = match request.op.as_str() {
            "send" => {
                let total: u64 = request.recipients.iter().map(|recipient| recipient.1).sum();
                prepare_send_inputs_with_rpc(
                    &rpc,
                    &outputs,
                    height,
                    total,
                    request.recipients.len(),
                    request.inputs.as_deref(),
                    None,
                    request.source_account,
                    policy,
                )
                .await
            }
            "sweep" => {
                prepare_sweep_inputs_with_rpc(
                    &rpc,
                    &outputs,
                    height,
                    request.inputs.as_deref(),
                    None,
                    request.source_account,
                    policy,
                )
                .await
            }
            other => panic!("unknown op {other}"),
        };
        match prepared {
            Err(error) => response.prepare_error = Some(error),
            Ok(prepared) => {
                let built = match request.op.as_str() {
                    "send" => {
                        create_transaction_with_rpc_and_policy(
                            &rpc,
                            &request.seed,
                            "mainnet",
                            prepared.stored_outputs.clone(),
                            &request.recipients,
                            "",
                            policy,
                        )
                        .await
                    }
                    _ => {
                        sweep_all_with_rpc_and_policy(
                            &rpc,
                            &request.seed,
                            "mainnet",
                            prepared.stored_outputs.clone(),
                            &request.recipients[0].0,
                            "",
                            policy,
                        )
                        .await
                    }
                };
                match built {
                    Ok(result) => {
                        if request.broadcast {
                            response.broadcast = Some(
                                match broadcast_raw_transaction_with_rpc(
                                    &rpc,
                                    &result.tx_blob,
                                    false,
                                )
                                .await
                                {
                                    Ok(()) => "OK".to_string(),
                                    Err(error) => error,
                                },
                            );
                        }
                        response.build = Some(Build {
                            inputs: prepared.spent_output_keys.clone(),
                            input_total: prepared.total_input,
                            estimated_fee: prepared.estimated_fee,
                            result,
                        });
                    }
                    Err(error) => response.build_error = Some(error),
                }
            }
        }
    }

    std::fs::write(
        &response_path,
        serde_json::to_string_pretty(&response).expect("serialize response"),
    )
    .expect("write response");
}
