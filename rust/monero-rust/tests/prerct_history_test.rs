//! wallet2 parity on a chain that starts before RingCT. Skipped unless
//! MONERO_REGTEST_RPC and PRERCT_FIXTURES are set:
//!
//!     scripts/prerct_chain.py all snapshot    # prints both variables
#![cfg(not(target_arch = "wasm32"))]

mod common;

use std::collections::{HashMap, HashSet};

use monero_rust::monero_rpc::HttpRpc;
use monero_rust::scanner::{scan_blocks_batch, scan_blocks_batch_with_history, Lookahead};
use monero_rust::{
    process_batch_with_reorg_detection, BlockScanResult, Network, ScanBatchOutcome, WalletOutput,
    WalletState,
};
use serde_json::Value;

const LOOKAHEAD: Lookahead = Lookahead {
    account: 3,
    subaddress: 10,
};

const WALLETS: [&str; 4] = ["alice", "bob", "carol", "miner"];

struct Fixture {
    url: String,
    chain: Value,
    dir: String,
}

async fn fixture() -> Option<Fixture> {
    let Ok(dir) = std::env::var("PRERCT_FIXTURES") else {
        eprintln!("Skipping pre-RingCT tests: PRERCT_FIXTURES is unset");
        return None;
    };
    let url = common::regtest_node()?;
    let chain: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{dir}/chain.json")).expect("read chain.json"),
    )
    .expect("parse chain.json");
    let height = chain["anchor"]["height"].as_u64().expect("anchor height");
    let rpc = HttpRpc::new(url.clone()).expect("regtest RPC");
    let served = rpc
        .get_block_hash(height as usize)
        .await
        .ok()
        .map(hex::encode);
    assert_eq!(
        served.as_deref(),
        chain["anchor"]["hash"].as_str(),
        "MONERO_REGTEST_RPC does not serve the chain that {dir} was dumped from"
    );
    Some(Fixture { url, chain, dir })
}

impl Fixture {
    fn seed(&self, wallet: &str) -> String {
        self.chain["wallets"][wallet]["seed"]
            .as_str()
            .expect("wallet seed")
            .to_string()
    }

    fn forks(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.chain["forks"]
            .as_array()
            .expect("fork table")
            .iter()
            .map(|entry| (entry[0].as_u64().unwrap(), entry[1].as_u64().unwrap()))
    }

    fn fork(&self, version: u64) -> u64 {
        self.forks()
            .find(|(fork, _)| *fork == version)
            .map(|(_, height)| height)
            .expect("fork height")
    }

    fn block_version(&self, height: u64) -> u64 {
        self.forks()
            .filter(|(_, start)| *start <= height)
            .map(|(version, _)| version)
            .max()
            .expect("block version")
    }

    fn wallet2(&self, wallet: &str) -> Wallet2 {
        let dump: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/w2-{wallet}.json", self.dir))
                .expect("read wallet2 dump"),
        )
        .expect("parse wallet2 dump");
        Wallet2 { dump }
    }

    async fn scan(&self, wallet: &str, start: u64) -> Scan {
        let end = self.wallet2(wallet).height();
        scan(&self.url, &self.seed(wallet), start, end)
            .await
            .unwrap_or_else(|error| panic!("{wallet}: scan failed: {error}"))
    }
}

struct Wallet2 {
    dump: Value,
}

impl Wallet2 {
    fn height(&self) -> u64 {
        self.dump["daemon_height"].as_u64().expect("dump height")
    }

    fn outputs(&self) -> Vec<Value> {
        self.dump["transfers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn balance(&self) -> u64 {
        self.dump["accounts"]["total_balance"]
            .as_u64()
            .expect("total balance")
    }

    fn transfers(&self, kind: &str) -> Vec<Value> {
        self.dump["history"][kind]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// wallet2 prints a missing payment ID as zeroes.
fn wallet2_payment_id(transfer: &Value) -> Option<String> {
    let id = transfer["payment_id"].as_str().unwrap_or_default();
    (!id.bytes().all(|byte| byte == b'0')).then(|| id.to_string())
}

struct Scan {
    state: WalletState,
    blocks: Vec<BlockScanResult>,
    height: u64,
}

impl Scan {
    fn by_key(&self) -> HashMap<String, WalletOutput> {
        self.state
            .outputs()
            .iter()
            .map(|output| (output.key.clone(), output.clone()))
            .collect()
    }
}

fn apply(state: &mut WalletState, results: &[BlockScanResult], target: u64) -> Result<(), String> {
    let Some(first) = results.first() else {
        return Ok(());
    };
    match process_batch_with_reorg_detection(results, state, None, target, first.block_height)? {
        ScanBatchOutcome::Normal(batch) => {
            state.add_outputs(batch.outputs_to_store.clone());
            for result in results {
                state.mark_spent_by_key_images_at_height(
                    &result.spent_key_images,
                    result.block_height,
                );
                state.record_block_hash(result.block_height, result.block_hash.clone());
            }
            state.current_height = batch.batch_end_height.saturating_sub(1);
            state.daemon_height = batch.daemon_height;
            Ok(())
        }
        ScanBatchOutcome::Reorg(info) => Err(format!("unexpected reorg: {info:?}")),
    }
}

async fn scan(url: &str, seed: &str, start: u64, end: u64) -> Result<Scan, String> {
    let rpc = HttpRpc::new(url.to_string()).map_err(|e| format!("{e:?}"))?;
    let mut state = WalletState::new();
    let mut blocks = Vec::new();
    let mut next = start;
    while next < end {
        let mut results =
            scan_blocks_batch(&rpc, next, seed, Network::Mainnet, LOOKAHEAD, false, "").await?;
        results.retain(|result| result.block_height < end);
        let Some(last) = results.last() else {
            break;
        };
        next = last.block_height + 1;
        apply(&mut state, &results, end)?;
        blocks.extend(results);
    }
    Ok(Scan {
        state,
        blocks,
        height: end,
    })
}

fn differences(scan: &Scan, wallet2: &Value, rust: &WalletOutput) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |name: &str, expected: String, actual: String| {
        if expected != actual {
            out.push(format!("{name}: wallet2 {expected}, monero-rust {actual}"));
        }
    };
    check(
        "amount",
        wallet2["amount"].to_string(),
        rust.amount.to_string(),
    );
    check(
        "key image",
        wallet2["key_image"].as_str().unwrap().to_string(),
        rust.key_image.clone(),
    );
    check(
        "global index",
        wallet2["global_index"].to_string(),
        rust.global_index.map(|i| i.to_string()).unwrap_or_default(),
    );
    check(
        "spent",
        wallet2["spent"].to_string(),
        rust.spent.to_string(),
    );
    check(
        "block height",
        wallet2["block_height"].to_string(),
        rust.block_height.to_string(),
    );
    check(
        "transaction",
        wallet2["tx_hash"].as_str().unwrap().to_string(),
        rust.tx_hash.clone(),
    );
    let (major, minor) = rust.subaddress_index.unwrap_or((0, 0));
    check(
        "subaddress",
        format!(
            "{}/{}",
            wallet2["subaddr_index"]["major"], wallet2["subaddr_index"]["minor"]
        ),
        format!("{major}/{minor}"),
    );
    if !wallet2["spent"].as_bool().unwrap() {
        let unlocked = monero_rust::is_spendable(rust, scan.height);
        check(
            "unlocked",
            wallet2["unlocked"].to_string(),
            unlocked.to_string(),
        );
    }
    out
}

#[tokio::test]
async fn every_wallet2_output_is_reported() {
    let Some(fx) = fixture().await else { return };
    for wallet in WALLETS {
        let rust = fx.scan(wallet, 0).await.by_key();
        let mut missing: HashMap<String, (usize, u64)> = HashMap::new();
        for output in fx.wallet2(wallet).outputs() {
            if !rust.contains_key(output["pubkey"].as_str().unwrap()) {
                let class = format!(
                    "block version {} / tx version {}",
                    fx.block_version(output["block_height"].as_u64().unwrap()),
                    output["tx_version"]
                );
                let entry = missing.entry(class).or_default();
                entry.0 += 1;
                entry.1 += output["amount"].as_u64().unwrap();
            }
        }
        assert!(
            missing.is_empty(),
            "{wallet}: outputs wallet2 reports and monero-rust does not, as class -> (count, atomic amount): {missing:?}"
        );
    }
}

#[tokio::test]
async fn balance_matches_wallet2() {
    let Some(fx) = fixture().await else { return };
    let mut old_funds = 0;
    for wallet in WALLETS {
        let scan = fx.scan(wallet, 0).await;
        let balance = scan.state.balance_at_height(scan.height);
        let expected = fx.wallet2(wallet).balance();
        let total = balance.confirmed + balance.unconfirmed + balance.pre_ringct;
        assert_eq!(
            total, expected,
            "{wallet}: wallet2 balance {expected}, monero-rust {total} with no error"
        );
        let pre_ringct: u64 = scan
            .state
            .outputs()
            .iter()
            .filter(|output| output.pre_ringct && !output.spent)
            .map(|output| output.amount)
            .sum();
        assert_eq!(balance.pre_ringct, pre_ringct, "{wallet}");
        old_funds += pre_ringct;
    }
    assert!(
        old_funds > 0,
        "the fixture has no unspent pre-RingCT output"
    );
}

#[tokio::test]
async fn version_1_outputs_in_ringct_era_blocks_are_reported() {
    let Some(fx) = fixture().await else { return };
    let fork = fx.fork(4);
    let rust = fx.scan("alice", fork).await.by_key();
    let expected: Vec<Value> = fx
        .wallet2("alice")
        .outputs()
        .into_iter()
        .filter(|output| {
            output["block_height"].as_u64().unwrap() >= fork && output["tx_version"] == 1
        })
        .collect();
    assert!(
        expected
            .iter()
            .any(|output| fx.block_version(output["block_height"].as_u64().unwrap()) == 16),
        "the fixture has no version 1 output in a block of version 16"
    );
    let missing: Vec<String> = expected
        .iter()
        .filter(|output| !rust.contains_key(output["pubkey"].as_str().unwrap()))
        .map(|output| {
            format!(
                "block {} amount {}",
                output["block_height"], output["amount"]
            )
        })
        .collect();
    assert!(
        missing.is_empty(),
        "version 1 outputs in blocks of version 4 and later that monero-rust skipped: {missing:?}"
    );
}

#[tokio::test]
async fn pre_ringct_outputs_are_flagged_and_never_spent() {
    use monero_rust::native::{create_unsigned_transaction_with_rpc, StoredOutputData};
    use monero_wallet::{interface::FeeRate, ringct::RctType};

    let Some(fx) = fixture().await else { return };
    let mut refused = 0;
    for wallet in WALLETS {
        let scan = fx.scan(wallet, 0).await;
        let rust = scan.by_key();
        for output in fx.wallet2(wallet).outputs() {
            let key = output["pubkey"].as_str().unwrap();
            assert_eq!(
                rust[key].pre_ringct,
                output["tx_version"] == 1,
                "{wallet}: output {key} in block {}",
                output["block_height"]
            );
        }
        let state = &scan.state;
        let height = state.chain_height();
        assert!(state.spendable_outputs().iter().all(|o| !o.pre_ringct));
        let balance = state.balance();
        if balance.pre_ringct == 0 {
            continue;
        }

        let error = monero_rust::prepare_send_inputs_at_with_fee(
            state.outputs(),
            height,
            None,
            balance.confirmed + balance.unconfirmed + 1,
            1,
            false,
            None,
            None,
            0,
            RctType::ClsagBulletproofPlus,
            FeeRate::new(20_000, 10_000).unwrap(),
        )
        .expect_err("pre-RingCT funds must not be selected");
        assert!(
            error.contains("pre-RingCT") && error.contains("sweep_unmixable"),
            "{wallet}: {error}"
        );

        let old = state
            .outputs()
            .iter()
            .find(|o| o.pre_ringct && !o.spent)
            .expect("an unspent pre-RingCT output");
        let error = monero_rust::prepare_send_inputs_at_with_fee(
            state.outputs(),
            height,
            None,
            1,
            1,
            false,
            Some(&[old.output_key()]),
            None,
            old.subaddress_index.map_or(0, |index| index.0),
            RctType::ClsagBulletproofPlus,
            FeeRate::new(20_000, 10_000).unwrap(),
        )
        .expect_err("a named pre-RingCT output must be refused");
        assert!(error.contains("pre-RingCT"), "{wallet}: {error}");

        let keys = monero_rust::scanner::derive_keys(&fx.seed(wallet), Network::Mainnet, "")
            .expect("wallet keys");
        let rpc = HttpRpc::new(fx.url.clone()).unwrap();
        let mut ring_state = WalletState::new();
        let error = create_unsigned_transaction_with_rpc(
            &rpc,
            &hex::encode(*keys.secret_view_key),
            &hex::encode(keys.public_spend_key),
            "mainnet",
            vec![StoredOutputData::from(old)],
            &[(keys.address.to_string(), 1)],
            None,
            &mut ring_state,
        )
        .await
        .err()
        .expect("a builder handed a pre-RingCT output must refuse it");
        assert!(error.contains("pre-RingCT"), "{wallet}: {error}");
        refused += 1;
    }
    assert!(
        refused > 0,
        "no wallet of the fixture holds pre-RingCT funds"
    );
}

#[tokio::test]
async fn outputs_found_in_one_block_say_whether_they_are_pre_ringct() {
    let Some(fx) = fixture().await else { return };
    let rpc = HttpRpc::new(fx.url.clone()).expect("regtest RPC");
    let outputs = fx.wallet2("alice").outputs();
    for version in [1, 2] {
        let wallet2 = outputs
            .iter()
            .find(|output| output["tx_version"] == version)
            .expect("an output of each transaction version");
        let height = wallet2["block_height"].as_u64().unwrap();
        let found = monero_rust::native::get_received_outputs_from_scan_with_rpc(
            &rpc,
            height,
            &fx.seed("alice"),
            "mainnet",
            "",
        )
        .await
        .unwrap();
        let in_tx: Vec<_> = found
            .iter()
            .filter(|received| {
                hex::encode(received.output.transaction()) == wallet2["tx_hash"].as_str().unwrap()
            })
            .collect();
        assert!(!in_tx.is_empty(), "block {height}: output not found");
        for received in in_tx {
            assert_eq!(received.pre_ringct, version == 1, "block {height}");
        }
    }
}

#[tokio::test]
async fn outgoing_transfers_are_seen_as_spends() {
    let Some(fx) = fixture().await else { return };
    for wallet in WALLETS {
        let dump = fx.wallet2(wallet);
        let scan = fx.scan(wallet, 0).await;
        let spent_heights: HashSet<u64> = scan
            .state
            .outputs()
            .iter()
            .filter(|output| output.spent)
            .filter_map(|output| output.spent_height)
            .collect();
        let unseen: Vec<String> = dump
            .transfers("out")
            .iter()
            .filter(|transfer| {
                let height = transfer["height"].as_u64().unwrap();
                height < dump.height() && !spent_heights.contains(&height)
            })
            .map(|transfer| format!("{} at height {}", transfer["txid"], transfer["height"]))
            .collect();
        assert!(
            unseen.is_empty(),
            "{wallet}: outgoing transfers with no spend in monero-rust: {unseen:?}"
        );
    }
}

#[tokio::test]
async fn change_outputs_carry_no_payment_id() {
    let Some(fx) = fixture().await else { return };
    let (mut checked, mut checked_with_id) = (0, 0);
    for wallet in WALLETS {
        let sent: HashMap<String, bool> = fx
            .wallet2(wallet)
            .transfers("out")
            .iter()
            .map(|transfer| {
                (
                    transfer["txid"].as_str().unwrap().to_string(),
                    wallet2_payment_id(transfer).is_some(),
                )
            })
            .collect();
        let scan = fx.scan(wallet, 0).await;
        let wrong: Vec<String> = scan
            .state
            .outputs()
            .iter()
            .filter(|output| sent.contains_key(&output.tx_hash))
            .inspect(|output| {
                checked += 1;
                checked_with_id += usize::from(sent[&output.tx_hash]);
            })
            .filter_map(|output| {
                output
                    .payment_id
                    .as_ref()
                    .map(|id| format!("block {} payment ID {id}", output.block_height))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "{wallet}: change outputs with a payment ID: {wrong:?}"
        );
    }
    assert!(checked > 0, "the fixture has no change output to check");
    assert!(
        checked_with_id > 0,
        "the fixture has no change from a transfer with a payment ID"
    );
}

#[tokio::test]
async fn old_blocks_are_recorded_under_their_block_id() {
    let Some(fx) = fixture().await else { return };
    let scan = scan(&fx.url, &fx.seed("alice"), 0, 12).await.unwrap();
    let rpc = HttpRpc::new(fx.url.clone()).unwrap();
    assert_eq!(scan.blocks.len(), 12);
    for block in &scan.blocks {
        let real = hex::encode(
            rpc.get_block_hash(block.block_height as usize)
                .await
                .unwrap(),
        );
        assert_eq!(
            block.block_hash, real,
            "block {} recorded under a hash the daemon does not know",
            block.block_height
        );
    }
}

/// One request only: the daemon blocks an address after four refusals.
#[tokio::test]
async fn history_resume_with_tip_in_old_blocks() {
    let Some(fx) = fixture().await else { return };
    let seed = fx.seed("alice");
    let stop = fx.fork(2);
    let first = scan(&fx.url, &seed, 0, stop).await.unwrap();
    let history = first.state.get_short_chain_history();
    assert!(!history.is_empty());
    let rpc = HttpRpc::new(fx.url.clone()).unwrap();
    let (results, start) = scan_blocks_batch_with_history(
        &rpc,
        0,
        &history,
        &seed,
        Network::Mainnet,
        LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("resume from the wallet's own history");
    assert!(
        start + 1 >= stop && results.iter().any(|result| result.block_height >= stop),
        "resume restarted at {start} instead of the wallet tip {stop}"
    );
}

#[tokio::test]
async fn history_resume_with_known_genesis_is_not_a_reorg() {
    let Some(fx) = fixture().await else { return };
    let seed = fx.seed("alice");
    let stop = fx.fork(2);
    let rpc = HttpRpc::new(fx.url.clone()).unwrap();
    let mut first = scan(&fx.url, &seed, 1, stop).await.unwrap();
    let genesis = hex::encode(rpc.get_block_hash(0).await.unwrap());
    first.state.record_block_hash(0, genesis);
    let history = first.state.get_short_chain_history();
    let (results, _start) = scan_blocks_batch_with_history(
        &rpc,
        0,
        &history,
        &seed,
        Network::Mainnet,
        LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("the daemon accepts a history that ends in the real genesis hash");
    let outcome = process_batch_with_reorg_detection(
        &results,
        &mut first.state,
        None,
        fx.wallet2("alice").height(),
        results[0].block_height,
    )
    .unwrap();
    assert!(
        matches!(outcome, ScanBatchOutcome::Normal(_)),
        "no block changed, yet the batch was treated as: {outcome:?}"
    );
}

#[tokio::test]
async fn reported_outputs_agree_with_wallet2() {
    let Some(fx) = fixture().await else { return };
    for wallet in WALLETS {
        let scan = fx.scan(wallet, 0).await;
        let wallet2: HashMap<String, Value> = fx
            .wallet2(wallet)
            .outputs()
            .into_iter()
            .map(|output| (output["pubkey"].as_str().unwrap().to_string(), output))
            .collect();
        let mut problems = Vec::new();
        for (key, output) in scan.by_key() {
            match wallet2.get(&key) {
                None => problems.push(format!("{key}: not known to wallet2")),
                Some(reference) => {
                    for difference in differences(&scan, reference, &output) {
                        problems.push(format!(
                            "{key} (block {}): {difference}",
                            output.block_height
                        ));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{wallet}: {problems:#?}");
    }
}

#[tokio::test]
async fn payment_ids_of_received_transfers_agree_with_wallet2() {
    let Some(fx) = fixture().await else { return };
    let received: HashMap<String, Option<String>> = fx
        .wallet2("alice")
        .transfers("in")
        .iter()
        .map(|transfer| {
            (
                transfer["txid"].as_str().unwrap().to_string(),
                wallet2_payment_id(transfer),
            )
        })
        .collect();
    let sent_late: HashSet<&str> = fx.chain["events"]
        .as_array()
        .expect("event log")
        .iter()
        .filter(|event| event["payment_id"].is_string() && event["era"] == 12)
        .filter_map(|event| event["txid"].as_str())
        .collect();
    let scan = fx.scan("alice", 0).await;
    let (mut long, mut short, mut dropped) = (0, 0, 0);
    for output in scan.state.outputs() {
        let Some(expected) = received.get(&output.tx_hash) else {
            continue;
        };
        assert_eq!(
            &output.payment_id, expected,
            "payment ID of the transfer in block {}",
            output.block_height
        );
        match expected.as_ref().map(String::len) {
            Some(64) => long += 1,
            Some(16) => short += 1,
            _ if sent_late.contains(output.tx_hash.as_str()) => dropped += 1,
            _ => {}
        }
    }
    assert!(
        long > 0 && short > 0 && dropped > 0,
        "the fixture lacks a case: {long} long, {short} short, {dropped} in block version 12"
    );
}

#[tokio::test]
async fn restore_height_at_ringct_fork_matches_full_scan() {
    let Some(fx) = fixture().await else { return };
    let fork = fx.fork(4);
    let full = fx.scan("alice", 0).await;
    let late = fx.scan("alice", fork).await;
    let full_keys: HashSet<String> = full
        .by_key()
        .into_iter()
        .filter(|(_, output)| output.block_height >= fork)
        .map(|(key, _)| key)
        .collect();
    let late_keys: HashSet<String> = late.by_key().into_keys().collect();
    assert!(!late_keys.is_empty());
    assert_eq!(full_keys, late_keys);
    assert_eq!(
        full.state.balance_at_height(full.height).confirmed,
        late.state.balance_at_height(late.height).confirmed
    );
}

#[tokio::test]
async fn key_images_of_pre_ringct_inputs_are_listed() {
    let Some(fx) = fixture().await else { return };
    let fork = fx.fork(4);
    let scan = fx.scan("alice", fork).await;
    let listed: HashSet<&String> = scan
        .blocks
        .iter()
        .flat_map(|block| block.spent_key_images.iter())
        .collect();
    let mut expected = 0;
    for output in fx.wallet2("alice").outputs() {
        let spent_in_ringct_era = output["spent"].as_bool().unwrap()
            && output["tx_version"] == 1
            && output["spent_height"].as_u64().unwrap_or(0) >= fork;
        if spent_in_ringct_era {
            expected += 1;
            let image = output["key_image"].as_str().unwrap().to_string();
            assert!(listed.contains(&image), "key image {image} not listed");
        }
    }
    assert!(
        expected > 0,
        "the fixture has no pre-RingCT output spent in the RingCT era"
    );
}

#[tokio::test]
async fn old_blocks_parse_and_hash() {
    let Some(fx) = fixture().await else { return };
    let rpc = HttpRpc::new(fx.url.clone()).unwrap();
    let genesis = rpc.get_block_hash(0).await.unwrap();
    let response = rpc.get_blocks_fast(&[genesis], 0, false).await.unwrap();
    let (mut blocks, mut transactions) = (0, 0);
    for (index, entry) in response.blocks.iter().enumerate() {
        let height = response.start_height + index as u64;
        if fx.block_version(height) >= 4 {
            break;
        }
        let block = monero_oxide::block::Block::read(&mut entry.block.as_slice())
            .unwrap_or_else(|e| panic!("block {height} does not parse: {e:?}"));
        assert_eq!(block.number() as u64, height);
        assert_eq!(
            block.hash(),
            rpc.get_block_hash(height as usize).await.unwrap(),
            "block {height}"
        );
        for blob in &entry.txs {
            let tx = monero_oxide::transaction::Transaction::<
                monero_oxide::transaction::NotPruned,
            >::read(&mut blob.as_slice())
            .unwrap_or_else(|e| panic!("transaction in block {height} does not parse: {e:?}"));
            assert_eq!(tx.version(), 1);
            transactions += 1;
        }
        blocks += 1;
    }
    assert!(
        blocks >= fx.fork(4) as usize && transactions > 0,
        "only {blocks} old blocks and {transactions} transactions were returned"
    );
}
