//! Regtest tools for `tests/scan_wallet2/harness.py`.
#![cfg(not(target_arch = "wasm32"))]

mod common;
mod scan_wallet2;

use monero_rust::scanner::{
    fetch_blocks_batch_with_url, get_daemon_height, process_fetched_batch_cached,
    scan_blocks_batch_with_url, scan_mempool_for_outputs_with_lookahead, Lookahead,
};
use monero_rust::{is_spendable_at, Network, WalletState};
use scan_wallet2::*;
use serde_json::json;

const WALLET2_LOOKAHEAD: Lookahead = Lookahead {
    account: 50,
    subaddress: 200,
};

fn mnemonic() -> String {
    let seed = std::env::var("SCAN_WALLET2_SEED").unwrap_or_else(|_| SEED.to_string());
    if std::env::var("SCAN_WALLET2_VIEW_ONLY").is_ok() {
        view_only_string(&seed)
    } else {
        seed
    }
}

async fn scan_to_tip(url: &str, mnemonic: &str) -> (WalletState, u64) {
    let mut state = WalletState::new();
    let mut cache = None;
    let mut next = 0u64;
    loop {
        let chain_length = get_daemon_height(url).await.unwrap();
        if next >= chain_length {
            return (state, chain_length);
        }
        let fetched = fetch_blocks_batch_with_url(url, next, false).await.unwrap();
        let (results, cached) = process_fetched_batch_cached(
            fetched,
            mnemonic,
            Network::Mainnet,
            WALLET2_LOOKAHEAD,
            cache.take(),
            "",
        )
        .await
        .unwrap();
        cache = Some(cached);
        let last = results
            .last()
            .expect("node returned no blocks")
            .block_height;
        apply_results(&mut state, &results);
        next = last + 1;
    }
}

#[tokio::test]
async fn dump_state() {
    let (Some(url), Ok(path)) = (common::regtest_node(), std::env::var("SCAN_WALLET2_DUMP")) else {
        return;
    };
    let mnemonic = mnemonic();
    let (state, chain_length) = scan_to_tip(&url, &mnemonic).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let outputs: Vec<_> = state
        .outputs()
        .iter()
        .map(|o| {
            let mut row = serde_json::to_value(o).unwrap();
            row.as_object_mut().unwrap().remove("received_output_bytes");
            row["unlocked_at_chain_length"] = json!(is_spendable_at(o, chain_length, Some(now)));
            row["unlocked_at_tip_height"] = json!(is_spendable_at(o, chain_length - 1, Some(now)));
            row
        })
        .collect();

    let pool = scan_mempool_for_outputs_with_lookahead(
        &url,
        &mnemonic,
        Network::Mainnet,
        WALLET2_LOOKAHEAD,
        "",
    )
    .await
    .unwrap();
    let pool_outputs: Vec<_> = pool
        .outputs
        .iter()
        .map(|o| {
            let mut row = serde_json::to_value(o).unwrap();
            row.as_object_mut().unwrap().remove("received_output_bytes");
            row
        })
        .collect();
    let own_images: std::collections::HashSet<&str> = state
        .outputs()
        .iter()
        .map(|o| o.key_image.as_str())
        .collect();
    let pool_spends: Vec<&String> = pool
        .spent_key_images
        .iter()
        .filter(|ki| own_images.contains(ki.as_str()))
        .collect();

    let balance = state.balance_at_height(chain_length);
    let dump = json!({
        "chain_length": chain_length,
        "state_current_height": state.current_height,
        "balance_at_chain_length": {
            "confirmed": balance.confirmed,
            "unconfirmed": balance.unconfirmed,
        },
        "balance_default": {
            "confirmed": state.balance().confirmed,
            "unconfirmed": state.balance().unconfirmed,
        },
        "outputs": outputs,
        "pool_outputs": pool_outputs,
        "pool_spent_own_key_images": pool_spends,
    });
    std::fs::write(path, serde_json::to_string_pretty(&dump).unwrap()).unwrap();
}

/// Needs SCAN_WALLET2_PRUNE_FROM, a height whose batch pays the wallet.
#[tokio::test]
#[ignore = "known defect: pruned getblocks.bin replies cannot be read"]
async fn pruned_scan_finds_the_same_outputs() {
    let (Some(url), Ok(from)) = (
        common::regtest_node(),
        std::env::var("SCAN_WALLET2_PRUNE_FROM"),
    ) else {
        return;
    };
    let from: u64 = from.parse().unwrap();
    let mnemonic = mnemonic();
    let count = |results: &[monero_rust::BlockScanResult]| -> usize {
        results.iter().map(|r| r.outputs.len()).sum()
    };
    let full = scan_blocks_batch_with_url(
        &url,
        from,
        &mnemonic,
        Network::Mainnet,
        WALLET2_LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("unpruned scan");
    assert!(count(&full) > 0, "choose a height range with a payment");
    let pruned = scan_blocks_batch_with_url(
        &url,
        from,
        &mnemonic,
        Network::Mainnet,
        WALLET2_LOOKAHEAD,
        true,
        "",
    )
    .await
    .expect("pruned scan must succeed against monerod");
    assert_eq!(
        count(&pruned),
        count(&full),
        "a pruned scan reported fewer outputs than the full scan of the same blocks"
    );
}
