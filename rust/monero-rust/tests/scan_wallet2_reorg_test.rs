#![cfg(not(target_arch = "wasm32"))]

mod scan_wallet2;

use curve25519_dalek::scalar::Scalar;
use monero_oxide::transaction::Timelock;
use monero_rust::monero_rpc::GetBlocksFastResponse;
use monero_rust::scanner::{process_batch_response, BlockScanResult, Lookahead};
use monero_rust::{process_batch_with_reorg_detection, Network, ScanBatchOutcome, WalletState};
use scan_wallet2::*;

const LOOKAHEAD: Lookahead = Lookahead {
    account: 1,
    subaddress: 5,
};

async fn scan_response(response: GetBlocksFastResponse) -> Vec<BlockScanResult> {
    let start = response.start_height;
    process_batch_response(response, start, SEED, Network::Mainnet, LOOKAHEAD, None, "")
        .await
        .expect("scan")
        .0
}

fn pay(amount: u64, salt: u64) -> monero_oxide::transaction::Transaction {
    let out = send_output(&Recipient::of(SEED, 0, 0), Scalar::from(salt), 0, amount);
    ringct_tx(
        &[out.clone()],
        extra_with_pubkeys(&[out.tx_pubkey]),
        Timelock::None,
        salt,
    )
}

fn block_with(chain: &mut FakeChain, txs: Vec<monero_oxide::transaction::Transaction>) {
    let height = chain.next_height();
    chain.push(
        miner_tx(height, &[burn_coinbase_output(XMR + height)], vec![1; 33]),
        txs,
    );
}

#[derive(Clone, Copy)]
enum Composition {
    PerBlock,
    AggregatedBatch,
}

async fn sync_with_history(state: &mut WalletState, node: &FakeChain, composition: Composition) {
    for _ in 0..4 {
        let history = state.get_short_chain_history();
        let start = node.common_start(&history);
        let results = scan_response(node.response_from(start)).await;
        match process_batch_with_reorg_detection(&results, state, None, node.next_height(), start)
            .unwrap()
        {
            ScanBatchOutcome::Reorg(_) => continue,
            ScanBatchOutcome::Normal(batch) => {
                match composition {
                    Composition::PerBlock => apply_results(state, &results),
                    Composition::AggregatedBatch => {
                        state.add_outputs(batch.outputs_to_store.clone());
                        state.mark_spent_by_key_images_at_height(
                            &batch.spent_key_images,
                            batch.batch_end_height,
                        );
                        for (height, hash) in &batch.block_hashes {
                            state.record_block_hash(*height, hash.clone());
                        }
                        state.current_height = batch.batch_end_height;
                        state.daemon_height = batch.daemon_height;
                    }
                }
                return;
            }
        }
    }
    panic!("sync did not settle");
}

fn summary(state: &WalletState) -> Vec<(String, u64, u64, bool)> {
    let mut rows: Vec<_> = state
        .outputs()
        .iter()
        .map(|o| (o.tx_hash.clone(), o.output_index, o.amount, o.spent))
        .collect();
    rows.sort();
    rows
}

async fn fresh_scan(node: &FakeChain) -> WalletState {
    let mut state = WalletState::new();
    apply_results(
        &mut state,
        &scan_response(node.response_from(node.start_height)).await,
    );
    state
}

/// Chain A pays X at 100, spends it at 112, pays Y at 114 and Z at 115; chain
/// B forks at 114 and pays W. Returns (A up to 111, A, B).
async fn fork_scenario() -> (FakeChain, FakeChain, FakeChain) {
    let mut a = FakeChain::new(100, 5000);
    block_with(&mut a, vec![pay(5 * XMR, 1)]);
    a.push_empty(11);
    let before_spend = a.fork_at(112);

    let x = scan_response(a.response_from(100)).await[0].outputs[0].clone();
    let key_image: [u8; 32] = hex::decode(&x.key_image).unwrap().try_into().unwrap();
    let elsewhere = send_output(
        &Recipient::of(OTHER_SEED, 0, 0),
        Scalar::from(50u64),
        0,
        XMR,
    );
    let spend = ringct_tx_spending(
        key_image,
        &[elsewhere.clone()],
        extra_with_pubkeys(&[elsewhere.tx_pubkey]),
        Timelock::None,
    );
    block_with(&mut a, vec![spend]);
    a.push_empty(1);
    let mut b = a.fork_at(114);
    block_with(&mut a, vec![pay(2 * XMR, 2)]);
    block_with(&mut a, vec![pay(3 * XMR, 3)]);

    block_with(&mut b, vec![pay(7 * XMR, 4)]);
    b.push_empty(2);
    (before_spend, a, b)
}

#[tokio::test]
async fn rollback_then_rescan_equals_a_fresh_scan() {
    let (_, a, b) = fork_scenario().await;
    let mut state = WalletState::new();
    sync_with_history(&mut state, &a, Composition::PerBlock).await;
    assert_eq!(total_balance(&state, a.next_height()), 5 * XMR);

    sync_with_history(&mut state, &b, Composition::PerBlock).await;
    let fresh = fresh_scan(&b).await;
    assert_eq!(summary(&state), summary(&fresh));
    assert_eq!(
        total_balance(&state, b.next_height()),
        total_balance(&fresh, b.next_height())
    );
    assert_eq!(total_balance(&state, b.next_height()), 7 * XMR);
}

#[tokio::test]
#[ignore = "known defect: aggregated batches lose the height of each spend"]
async fn rollback_keeps_a_spend_that_is_below_the_fork_when_batches_are_aggregated() {
    let (before_spend, a, b) = fork_scenario().await;
    let mut state = WalletState::new();
    sync_with_history(&mut state, &before_spend, Composition::AggregatedBatch).await;
    sync_with_history(&mut state, &a, Composition::AggregatedBatch).await;
    assert_eq!(total_balance(&state, a.next_height()), 5 * XMR);

    sync_with_history(&mut state, &b, Composition::AggregatedBatch).await;
    let fresh = fresh_scan(&b).await;
    assert_eq!(
        summary(&state),
        summary(&fresh),
        "X was spent in block 112, which both chains share"
    );
}

/// monerod ignores `block_ids` when `start_height > 0`.
#[tokio::test]
async fn fork_below_the_requested_height_is_noticed() {
    let (_, a, mut b) = fork_scenario().await;
    b.push_empty(3);
    let mut state = WalletState::new();
    sync_with_history(&mut state, &a, Composition::PerBlock).await;

    let next = a.next_height();
    let results = scan_response(b.response_from(next)).await;
    match process_batch_with_reorg_detection(&results, &mut state, None, b.next_height(), next) {
        Ok(ScanBatchOutcome::Normal(_)) => apply_results(&mut state, &results),
        Ok(ScanBatchOutcome::Reorg(_)) | Err(_) => {
            sync_with_history(&mut state, &b, Composition::PerBlock).await
        }
    }

    let fresh = fresh_scan(&b).await;
    assert_eq!(
        summary(&state),
        summary(&fresh),
        "the wallet kept outputs from orphaned blocks 114 and 115 and never saw block 114 of the new chain"
    );
}

#[tokio::test]
#[ignore = "known defect: an output marked spent at send time is never released"]
async fn output_of_a_dropped_send_becomes_spendable_again() {
    let mut a = FakeChain::new(100, 5000);
    block_with(&mut a, vec![pay(5 * XMR, 1)]);
    a.push_empty(20);
    let mut state = WalletState::new();
    sync_with_history(&mut state, &a, Composition::PerBlock).await;
    let identity = state.outputs()[0].output_key();

    assert_eq!(state.mark_spent_by_output_keys(&[identity]), 1);
    assert_eq!(total_balance(&state, a.next_height()), 0);

    a.push_empty(30);
    sync_with_history(&mut state, &a, Composition::PerBlock).await;
    state.cleanup_expired_pending_spends(u64::MAX);

    assert_eq!(
        total_balance(&state, a.next_height()),
        5 * XMR,
        "the output is unspent on chain but the wallet still reports it as spent"
    );
}

#[tokio::test]
async fn rollback_credits_the_output_a_removed_duplicate_had_displaced() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(9001u64);
    let small = send_output(&recipient, r, 0, XMR);
    let large = send_output(&recipient, r, 0, 5 * XMR);
    let extra = extra_with_pubkeys(&[small.tx_pubkey]);

    let mut a = FakeChain::new(100, 5000);
    a.push_empty(1);
    block_with(
        &mut a,
        vec![ringct_tx(&[small], extra.clone(), Timelock::None, 1)],
    );
    a.push_empty(11);
    let mut b = a.fork_at(113);
    block_with(&mut a, vec![ringct_tx(&[large], extra, Timelock::None, 2)]);
    b.push_empty(3);

    let mut state = WalletState::new();
    sync_with_history(&mut state, &a, Composition::PerBlock).await;
    assert_eq!(total_balance(&state, a.next_height()), 5 * XMR);

    sync_with_history(&mut state, &b, Composition::PerBlock).await;
    assert_eq!(summary(&state), summary(&fresh_scan(&b).await));
    assert_eq!(total_balance(&state, b.next_height()), XMR);
}
