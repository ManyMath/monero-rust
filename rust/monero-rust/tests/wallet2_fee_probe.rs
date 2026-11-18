//! Driver for `scripts/wallet2-fees/`: ignored probes configured by `PROBE_*` variables.

#![cfg(not(target_arch = "wasm32"))]

use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::monero_rpc::{FeePriority, HttpRpc, DEFAULT_MAX_FEE_PER_BYTE};
use monero_rust::native::{
    create_transaction_with_rpc_and_policy, estimate_fee_with_rpc_and_policy, FeePolicy,
    StoredOutputData,
};
use monero_rust::scanner::{scan_block_for_outputs_with_lookahead, Lookahead};
use monero_rust::tx_prepare::prepare_send_inputs_with_rpc;
use monero_rust::WalletOutput;
use serde_json::json;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn priority() -> FeePriority {
    match env("PROBE_PRIORITY").as_deref().unwrap_or("automatic") {
        "automatic" | "0" => FeePriority::Automatic,
        "low" | "1" => FeePriority::Low,
        "normal" | "2" => FeePriority::Normal,
        "elevated" | "3" => FeePriority::Elevated,
        "priority" | "4" => FeePriority::Priority,
        other => panic!("unknown PROBE_PRIORITY {other}"),
    }
}

fn policy() -> FeePolicy {
    FeePolicy {
        priority: priority(),
        wallet_height: env("PROBE_WALLET_HEIGHT").map(|h| h.parse().expect("wallet height")),
        max_fee_per_weight: env("PROBE_MAX_FEE")
            .map(|h| h.parse().expect("max fee"))
            .unwrap_or(DEFAULT_MAX_FEE_PER_BYTE),
    }
}

fn emit(value: serde_json::Value) {
    println!("PROBE_RESULT {value}");
}

#[tokio::test]
#[ignore = "probe for scripts/wallet2-fees, run by name with PROBE_* set"]
async fn probe_rate() {
    let Some(node) = env("PROBE_NODE") else {
        return;
    };
    let rpc = HttpRpc::new(node).expect("rpc");
    let policy = policy();
    let started = std::time::Instant::now();
    let rate = rpc
        .get_fee_checked_for_priority_at_height(
            policy.priority,
            policy.wallet_height,
            policy.max_fee_per_weight,
        )
        .await;
    let elapsed = started.elapsed().as_secs_f64();
    match rate {
        Ok(fee) => emit(json!({
            "ok": true, "per_weight": fee.per_weight, "mask": fee.mask, "secs": elapsed
        })),
        Err(e) => emit(json!({ "ok": false, "error": format!("{e:?}"), "secs": elapsed })),
    }
}

#[tokio::test]
#[ignore = "probe for scripts/wallet2-fees, run by name with PROBE_* set"]
async fn probe_estimate() {
    let Some(node) = env("PROBE_NODE") else {
        return;
    };
    let rpc = HttpRpc::new(node).expect("rpc");
    let inputs = env("PROBE_INPUTS").map_or(1, |v| v.parse().unwrap());
    let outputs = env("PROBE_OUTPUTS").map_or(2, |v| v.parse().unwrap());
    match estimate_fee_with_rpc_and_policy(&rpc, inputs, outputs, policy()).await {
        Ok(est) => emit(json!({
            "ok": true, "fee": est.fee, "per_weight": est.per_weight, "mask": est.mask,
            "estimate": serde_json::to_value(&est).unwrap()
        })),
        Err(e) => emit(json!({ "ok": false, "error": e })),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "probe for scripts/wallet2-fees, run by name with PROBE_* set"]
async fn probe_scan() {
    let Some(node) = env("PROBE_NODE") else {
        return;
    };
    let rpc = HttpRpc::new(node).expect("rpc");
    let seed = env("PROBE_SEED").expect("PROBE_SEED");
    let from: u64 = env("PROBE_SCAN_FROM").expect("from").parse().unwrap();
    let to: u64 = env("PROBE_SCAN_TO").expect("to").parse().unwrap();
    let want: usize = env("PROBE_SCAN_WANT").map_or(4, |v| v.parse().unwrap());
    let mut found: Vec<WalletOutput> = Vec::new();
    for height in from..to {
        let scanned = scan_block_for_outputs_with_lookahead(
            &rpc,
            height,
            &seed,
            monero_rust::Network::Mainnet,
            Lookahead {
                account: 0,
                subaddress: 2,
            },
            "",
        )
        .await
        .expect("scan");
        found.extend(scanned.outputs);
        if found.len() >= want {
            break;
        }
    }
    let path = env("PROBE_OUTPUTS_FILE").expect("PROBE_OUTPUTS_FILE");
    std::fs::write(&path, serde_json::to_vec(&found).unwrap()).unwrap();
    emit(json!({ "ok": true, "outputs": found.len(), "file": path }));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "probe for scripts/wallet2-fees, run by name with PROBE_* set"]
async fn probe_send() {
    let Some(node) = env("PROBE_NODE") else {
        return;
    };
    let rpc = HttpRpc::new(node).expect("rpc");
    let seed = env("PROBE_SEED").expect("PROBE_SEED");
    let dest = env("PROBE_DEST").expect("PROBE_DEST");
    let amount: u64 = env("PROBE_AMOUNT").expect("amount").parse().unwrap();
    let daemon_height: u64 = env("PROBE_DAEMON_HEIGHT").expect("height").parse().unwrap();
    let outputs: Vec<WalletOutput> = serde_json::from_slice(
        &std::fs::read(env("PROBE_OUTPUTS_FILE").expect("PROBE_OUTPUTS_FILE")).unwrap(),
    )
    .unwrap();
    let policy = policy();

    let stored: Vec<StoredOutputData> = if env("PROBE_PREPARE").is_some() {
        match prepare_send_inputs_with_rpc(
            &rpc,
            &outputs,
            daemon_height,
            amount,
            1,
            None,
            None,
            0,
            policy,
        )
        .await
        {
            Ok(prepared) => prepared.stored_outputs,
            Err(e) => {
                emit(json!({ "ok": false, "stage": "prepare", "error": e }));
                return;
            }
        }
    } else {
        let take: usize = env("PROBE_INPUTS").map_or(1, |v| v.parse().unwrap());
        outputs
            .iter()
            .take(take)
            .map(StoredOutputData::from)
            .collect()
    };
    let inputs = stored.len();

    match create_transaction_with_rpc_and_policy(
        &rpc,
        &seed,
        "mainnet",
        stored,
        &[(dest, amount)],
        "",
        policy,
    )
    .await
    {
        Ok(result) => {
            let bytes = hex::decode(&result.tx_blob).unwrap();
            let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
            emit(json!({
                "ok": true, "fee": result.fee, "weight": tx.weight(), "size": bytes.len(),
                "inputs": inputs, "outputs": tx.prefix().outputs.len(),
                "extra_len": tx.prefix().extra.len(), "tx_id": result.tx_id,
                "tx_blob": result.tx_blob,
            }));
        }
        Err(e) => emit(json!({ "ok": false, "stage": "create", "error": e })),
    }
}

#[test]
#[ignore = "probe for scripts/wallet2-fees, run by name with PROBE_* set"]
fn probe_weigh() {
    let Some(blob) = env("PROBE_TX_HEX") else {
        return;
    };
    let bytes = hex::decode(blob.trim()).unwrap();
    let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
    let fee = match &tx {
        Transaction::V2 {
            proofs: Some(proofs),
            ..
        } => proofs.base.fee,
        _ => 0,
    };
    emit(json!({
        "ok": true, "weight": tx.weight(), "size": bytes.len(), "fee": fee,
        "inputs": tx.prefix().inputs.len(), "outputs": tx.prefix().outputs.len(),
        "extra_len": tx.prefix().extra.len(),
    }));
}
