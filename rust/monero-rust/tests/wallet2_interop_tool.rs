//! Send harness driven by `scripts/wallet2_interop.py`.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashSet;

use monero_rust::monero_rpc::{FeePriority, HttpRpc, Rpc, DEFAULT_MAX_FEE_PER_BYTE};
use monero_rust::native::{
    broadcast_raw_transaction_with_rpc, create_transaction_with_rpc_and_policy,
    sweep_all_with_rpc_and_policy, FeePolicy, InputRing, TransactionResult,
};
use monero_rust::scanner::{scan_blocks_batch, Lookahead};
use monero_rust::{prepare_send_inputs_with_rpc, prepare_sweep_inputs_with_rpc, WalletOutput};
use serde::{Deserialize, Serialize};

/// Regtest uses mainnet address encoding.
const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;

#[derive(Deserialize)]
struct Request {
    rpc: String,
    seed: String,
    #[serde(default)]
    lookahead_account: u32,
    #[serde(default = "default_subaddress_lookahead")]
    lookahead_subaddress: u32,
    op: String,
    #[serde(default)]
    source_account: u32,
    #[serde(default)]
    recipients: Vec<(String, u64)>,
    #[serde(default)]
    inputs: Option<Vec<String>>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    rings: Vec<InputRing>,
    #[serde(default = "one")]
    repeat: usize,
    #[serde(default)]
    broadcast: bool,
}

fn default_subaddress_lookahead() -> u32 {
    5
}

fn one() -> usize {
    1
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
    height: usize,
    outputs: Vec<WalletOutput>,
    builds: Vec<Build>,
    build_errors: Vec<String>,
    broadcast: Option<String>,
}

fn priority_of(name: Option<&str>) -> FeePriority {
    match name.unwrap_or("automatic") {
        "automatic" => FeePriority::Automatic,
        "low" => FeePriority::Low,
        "normal" => FeePriority::Normal,
        "elevated" => FeePriority::Elevated,
        "priority" => FeePriority::Priority,
        other => panic!("unknown priority {other}"),
    }
}

async fn scan_wallet(rpc: &Rpc<HttpRpc>, request: &Request) -> (usize, Vec<WalletOutput>) {
    let lookahead = Lookahead {
        account: request.lookahead_account,
        subaddress: request.lookahead_subaddress,
    };
    let height = rpc.get_height().await.expect("get_height");
    let mut outputs: Vec<WalletOutput> = Vec::new();
    let mut spent: HashSet<String> = HashSet::new();
    let mut next = 1u64;
    while (next as usize) < height {
        let results = scan_blocks_batch(rpc, next, &request.seed, NETWORK, lookahead, false, "")
            .await
            .expect("batch scan");
        let Some(last) = results.last() else {
            break;
        };
        next = last.block_height + 1;
        for block in results {
            spent.extend(block.spent_key_images);
            outputs.extend(block.outputs);
        }
    }
    for output in &mut outputs {
        if spent.contains(&output.key_image) {
            output.spent = true;
        }
    }
    (height, outputs)
}

async fn build(rpc: &Rpc<HttpRpc>, request: &Request, response: &mut Response) {
    let height = response.height as u64;
    let policy = FeePolicy {
        priority: priority_of(request.priority.as_deref()),
        wallet_height: Some(height),
        max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
    };
    let prepared = match request.op.as_str() {
        "send" => {
            let total: u64 = request.recipients.iter().map(|recipient| recipient.1).sum();
            prepare_send_inputs_with_rpc(
                rpc,
                &response.outputs,
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
                rpc,
                &response.outputs,
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
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            response.build_errors.push(error);
            return;
        }
    };
    let mut stored_outputs = prepared.stored_outputs.clone();
    for stored in &mut stored_outputs {
        if let Some(known) = request
            .rings
            .iter()
            .find(|known| known.key_image == stored.key_image)
        {
            stored.ring = Some(known.ring.clone());
        }
    }

    for _ in 0..request.repeat {
        let mut build_state = monero_rust::WalletState::new();
        build_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
        build_state.add_outputs(response.outputs.clone());
        let built = match request.op.as_str() {
            "send" => {
                create_transaction_with_rpc_and_policy(
                    rpc,
                    &request.seed,
                    "mainnet",
                    stored_outputs.clone(),
                    &request.recipients,
                    "",
                    policy,
                    &mut build_state,
                )
                .await
            }
            _ => {
                sweep_all_with_rpc_and_policy(
                    rpc,
                    &request.seed,
                    "mainnet",
                    stored_outputs.clone(),
                    &request.recipients[0].0,
                    "",
                    policy,
                    &mut build_state,
                )
                .await
            }
        };
        match built {
            Ok(result) => response.builds.push(Build {
                inputs: prepared.spent_output_keys.clone(),
                input_total: prepared.total_input,
                estimated_fee: prepared.estimated_fee,
                result,
            }),
            Err(error) => response.build_errors.push(error),
        }
    }

    if request.broadcast {
        assert_eq!(request.repeat, 1, "broadcast one build at a time");
        if let Some(build) = response.builds.first() {
            response.broadcast = Some(
                match broadcast_raw_transaction_with_rpc(rpc, &build.result.tx_blob, false).await {
                    Ok(()) => "OK".to_string(),
                    Err(error) => error,
                },
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn interop_tool() {
    let Ok(request_path) = std::env::var("INTEROP_REQUEST") else {
        eprintln!("Skipping wallet2 interop tool: INTEROP_REQUEST is unset");
        return;
    };
    let response_path = std::env::var("INTEROP_RESPONSE").expect("INTEROP_RESPONSE");
    let request: Request =
        serde_json::from_str(&std::fs::read_to_string(&request_path).expect("read request"))
            .expect("parse request");

    let rpc = HttpRpc::new(request.rpc.clone()).expect("regtest RPC");
    let (height, outputs) = scan_wallet(&rpc, &request).await;
    let mut response = Response {
        height,
        outputs,
        ..Response::default()
    };

    if request.op != "scan" {
        build(&rpc, &request, &mut response).await;
    }

    std::fs::write(
        &response_path,
        serde_json::to_string_pretty(&response).expect("serialize response"),
    )
    .expect("write response");
}
