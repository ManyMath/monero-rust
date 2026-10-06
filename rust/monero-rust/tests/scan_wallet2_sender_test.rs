//! What a sender or miner can make the wallet record, against wallet2.
#![cfg(not(target_arch = "wasm32"))]

mod scan_wallet2;

use curve25519_dalek::scalar::Scalar;
use monero_oxide::transaction::Timelock;
use monero_rust::scanner::{
    process_batch_multi_wallet_response, process_batch_response, BlockScanResult, Lookahead,
    WalletScanConfig,
};
use monero_rust::{is_spendable_at, Network, WalletOutput, WalletState};
use scan_wallet2::*;

const LOOKAHEAD: Lookahead = Lookahead {
    account: 2,
    subaddress: 5,
};

async fn scan(chain: &FakeChain, seed: &str) -> Vec<BlockScanResult> {
    let (results, _) = process_batch_response(
        chain.response_from(chain.start_height),
        chain.start_height,
        seed,
        Network::Mainnet,
        LOOKAHEAD,
        None,
        "",
    )
    .await
    .expect("scan");
    results
}

fn all_outputs(results: &[BlockScanResult]) -> Vec<WalletOutput> {
    results.iter().flat_map(|r| r.outputs.clone()).collect()
}

fn state_from(results: &[BlockScanResult]) -> WalletState {
    let mut state = WalletState::new();
    apply_results(&mut state, results);
    state
}

#[tokio::test]
async fn burning_bug_coinbase_dust_does_not_hide_the_real_payment() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(1001u64);
    let payment = send_output(&recipient, r, 0, 100 * XMR);
    let dust = CoinbaseOutput {
        key: payment.key,
        view_tag: payment.view_tag,
        amount: 1,
    };

    let mut chain = FakeChain::new(100, 5000);
    let real_tx = ringct_tx(
        &[payment.clone()],
        extra_with_pubkeys(&[payment.tx_pubkey]),
        Timelock::None,
        1,
    );
    chain.push(
        miner_tx(
            100,
            &[dust, burn_coinbase_output(600_000_000_000)],
            extra_with_pubkeys(&[payment.tx_pubkey]),
        ),
        vec![real_tx.clone()],
    );
    chain.push_empty(70);

    let state = state_from(&scan(&chain, SEED).await);
    let height = chain.next_height();
    let unspent: Vec<_> = state.outputs().iter().filter(|o| !o.spent).collect();
    assert_eq!(unspent.len(), 1, "one spendable record for one key image");
    assert_eq!(
        unspent[0].amount,
        100 * XMR,
        "wallet2 keeps the larger of two outputs sharing a key; got the record from tx {}",
        unspent[0].tx_hash
    );
    assert_eq!(unspent[0].tx_hash, hex::encode(real_tx.hash()));
    assert_eq!(total_balance(&state, height), 100 * XMR);
}

#[tokio::test]
async fn burning_bug_full_wallet_reports_the_larger_duplicate() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(1002u64);
    let small = send_output(&recipient, r, 0, XMR);
    let large = send_output(&recipient, r, 0, 5 * XMR);
    assert_eq!(small.key, large.key);
    let extra = extra_with_pubkeys(&[small.tx_pubkey]);

    let mut chain = FakeChain::new(100, 5000);
    chain.push_empty(1);
    chain.push(
        miner_tx(101, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(&[small], extra.clone(), Timelock::None, 1)],
    );
    chain.push(
        miner_tx(102, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(&[large], extra, Timelock::None, 2)],
    );
    chain.push_empty(12);

    let state = state_from(&scan(&chain, SEED).await);
    assert_eq!(state.outputs().len(), 1);
    assert_eq!(total_balance(&state, chain.next_height()), 5 * XMR);
}

#[tokio::test]
async fn burning_bug_full_wallet_ignores_a_smaller_later_duplicate() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(1003u64);
    let large = send_output(&recipient, r, 0, 5 * XMR);
    let small = send_output(&recipient, r, 0, XMR);
    let extra = extra_with_pubkeys(&[small.tx_pubkey]);

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(&[large], extra.clone(), Timelock::None, 1)],
    );
    chain.push(
        miner_tx(101, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(&[small], extra, Timelock::None, 2)],
    );
    chain.push_empty(12);

    let state = state_from(&scan(&chain, SEED).await);
    assert_eq!(state.outputs().len(), 1);
    assert_eq!(total_balance(&state, chain.next_height()), 5 * XMR);
}

#[tokio::test]
async fn burning_bug_view_only_wallet_credits_one_output_per_key() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(1004u64);
    let paid = send_output(&recipient, r, 0, 5 * XMR);
    let extra = extra_with_pubkeys(&[paid.tx_pubkey]);

    let mut chain = FakeChain::new(100, 5000);
    for salt in 0..3u64 {
        let height = chain.next_height();
        chain.push(
            miner_tx(height, &[burn_coinbase_output(XMR)], vec![1; 33]),
            vec![ringct_tx(
                &[paid.clone()],
                extra.clone(),
                Timelock::None,
                salt,
            )],
        );
    }
    chain.push_empty(12);

    let state = state_from(&scan(&chain, &view_only_string(SEED)).await);
    assert_eq!(
        total_balance(&state, chain.next_height()),
        5 * XMR,
        "only one of the {} recorded outputs can ever be spent",
        state.outputs().len()
    );
}

#[tokio::test]
async fn coinbase_output_beyond_position_255_is_credited() {
    let recipient = Recipient::of(SEED, 0, 0);
    let r = Scalar::from(1005u64);
    let position = 299usize;
    let ours = send_output(&recipient, r, position, 3 * XMR);

    let mut outputs: Vec<CoinbaseOutput> = (0..position)
        .map(|_| burn_coinbase_output(1_000_000))
        .collect();
    outputs.push((&ours).into());

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &outputs, extra_with_pubkeys(&[ours.tx_pubkey])),
        vec![],
    );
    chain.push_empty(2);

    let result = process_batch_response(
        chain.response_from(100),
        100,
        SEED,
        Network::Mainnet,
        LOOKAHEAD,
        None,
        "",
    )
    .await;
    let (results, _) = result.expect("a payout at output 299 must not stop the scan");
    let found = all_outputs(&results);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].amount, 3 * XMR);
}

#[tokio::test]
async fn output_derived_from_a_second_tx_pubkey_is_found() {
    let recipient = Recipient::of(SEED, 0, 0);
    let ours = send_output(&recipient, Scalar::from(1006u64), 0, 2 * XMR);
    let decoy_key = send_output(&recipient, Scalar::from(77u64), 5, 1).tx_pubkey;

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[ours.clone()],
            extra_with_pubkeys(&[decoy_key, ours.tx_pubkey]),
            Timelock::None,
            1,
        )],
    );
    let found = all_outputs(&scan(&chain, SEED).await);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].amount, 2 * XMR);
}

#[tokio::test]
async fn output_is_found_when_extra_has_trailing_garbage() {
    let recipient = Recipient::of(SEED, 0, 0);
    let ours = send_output(&recipient, Scalar::from(1007u64), 0, 2 * XMR);
    let mut extra = extra_with_pubkeys(&[ours.tx_pubkey]);
    extra.extend_from_slice(&[0x99, 0xff, 0xff, 0x03]);

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(&[ours], extra, Timelock::None, 1)],
    );
    assert_eq!(all_outputs(&scan(&chain, SEED).await).len(), 1);
}

#[tokio::test]
async fn subaddress_outputs_inside_the_lookahead_are_labelled() {
    let mut chain = FakeChain::new(100, 5000);
    let mut txs = Vec::new();
    for (salt, (account, index)) in [(0u32, 3u32), (1, 0), (2, 5)].into_iter().enumerate() {
        let out = send_output(
            &Recipient::of(SEED, account, index),
            Scalar::from(2000 + salt as u64),
            0,
            XMR,
        );
        txs.push(ringct_tx(
            &[out.clone()],
            extra_with_pubkeys(&[out.tx_pubkey]),
            Timelock::None,
            salt as u64,
        ));
    }
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        txs,
    );
    let mut found: Vec<_> = all_outputs(&scan(&chain, SEED).await)
        .iter()
        .map(|o| o.subaddress_index)
        .collect();
    found.sort();
    assert_eq!(found, vec![Some((0, 3)), Some((1, 0)), Some((2, 5))]);
}

#[tokio::test]
async fn lookahead_expansion_carries_over_to_the_next_batch() {
    let first = send_output(&Recipient::of(SEED, 0, 4), Scalar::from(3001u64), 0, XMR);
    let second = send_output(
        &Recipient::of(SEED, 0, 8),
        Scalar::from(3002u64),
        0,
        2 * XMR,
    );

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[first.clone()],
            extra_with_pubkeys(&[first.tx_pubkey]),
            Timelock::None,
            1,
        )],
    );
    chain.push(
        miner_tx(101, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[second.clone()],
            extra_with_pubkeys(&[second.tx_pubkey]),
            Timelock::None,
            2,
        )],
    );

    let mut state = WalletState::new();
    let mut found = Vec::new();
    for height in [100u64, 101] {
        let scanner =
            monero_rust::cached_scanner_for_wallet(SEED, LOOKAHEAD, "", &state.used_subaddresses())
                .await
                .unwrap();
        let (results, _) = process_batch_response(
            chain.response_range(height, height + 1),
            height,
            SEED,
            Network::Mainnet,
            LOOKAHEAD,
            Some(scanner),
            "",
        )
        .await
        .unwrap();
        apply_results(&mut state, &results);
        found.extend(all_outputs(&results));
    }
    let total: u64 = found.iter().map(|o| o.amount).sum();
    assert_eq!(
        total,
        3 * XMR,
        "the payment to subaddress (0,8) was missed after (0,4) had been used"
    );
}

#[tokio::test]
async fn lookahead_expansion_works_while_the_scanner_cache_is_kept() {
    let first = send_output(&Recipient::of(SEED, 0, 4), Scalar::from(3001u64), 0, XMR);
    let second = send_output(
        &Recipient::of(SEED, 0, 8),
        Scalar::from(3002u64),
        0,
        2 * XMR,
    );

    let mut chain = FakeChain::new(100, 5000);
    for (height, out) in [(100u64, first), (101, second)] {
        chain.push(
            miner_tx(height, &[burn_coinbase_output(XMR)], vec![1; 33]),
            vec![ringct_tx(
                &[out.clone()],
                extra_with_pubkeys(&[out.tx_pubkey]),
                Timelock::None,
                height,
            )],
        );
    }

    let mut cache = None;
    let mut total = 0;
    for height in [100u64, 101] {
        let (results, cached) = process_batch_response(
            chain.response_range(height, height + 1),
            height,
            SEED,
            Network::Mainnet,
            LOOKAHEAD,
            cache.take(),
            "",
        )
        .await
        .unwrap();
        cache = Some(cached);
        total += all_outputs(&results).iter().map(|o| o.amount).sum::<u64>();
    }
    assert_eq!(total, 3 * XMR);
}

#[tokio::test]
async fn zero_amount_output_is_not_recorded() {
    let recipient = Recipient::of(SEED, 0, 0);
    let zero = send_output(&recipient, Scalar::from(4001u64), 0, 0);

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[zero.clone()],
            extra_with_pubkeys(&[zero.tx_pubkey]),
            Timelock::None,
            1,
        )],
    );
    let found = all_outputs(&scan(&chain, SEED).await);
    assert!(
        found.is_empty(),
        "wallet2 records nothing for a zero-amount output; got {} record(s)",
        found.len()
    );
}

#[tokio::test]
async fn amount_that_does_not_match_the_commitment_is_rejected() {
    let recipient = Recipient::of(SEED, 0, 0);
    let mut forged = send_output(&recipient, Scalar::from(4002u64), 0, XMR);
    let honest = forged.encrypted_amount;
    let claimed = send_output(&recipient, Scalar::from(4002u64), 0, 1000 * XMR);
    forged.encrypted_amount = claimed.encrypted_amount;
    assert_ne!(honest, forged.encrypted_amount);

    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[forged.clone()],
            extra_with_pubkeys(&[forged.tx_pubkey]),
            Timelock::None,
            1,
        )],
    );
    assert!(all_outputs(&scan(&chain, SEED).await).is_empty());
}

async fn locked_output(timelock: Timelock) -> (WalletState, WalletOutput, u64) {
    let recipient = Recipient::of(SEED, 0, 0);
    let out = send_output(&recipient, Scalar::from(5001u64), 0, 7 * XMR);
    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(100, &[burn_coinbase_output(XMR)], vec![1; 33]),
        vec![ringct_tx(
            &[out.clone()],
            extra_with_pubkeys(&[out.tx_pubkey]),
            timelock,
            1,
        )],
    );
    chain.push_empty(99);
    let state = state_from(&scan(&chain, SEED).await);
    let output = state.outputs()[0].clone();
    (state, output, chain.next_height())
}

#[tokio::test]
async fn far_future_block_lock_is_never_spendable() {
    let (state, output, chain_length) = locked_output(Timelock::Block(400_000_000)).await;
    assert_eq!(chain_length, 200);
    assert!(!is_spendable_at(&output, chain_length, Some(u64::MAX / 2)));
    assert!(state.spendable_outputs().is_empty());
    let balance = state.balance_at_height(chain_length);
    assert_eq!(balance.confirmed, 0);
    assert_eq!(balance.unconfirmed, 7 * XMR);
}

#[tokio::test]
async fn far_future_time_lock_is_never_spendable() {
    let (state, output, chain_length) = locked_output(Timelock::Time(4_000_000_000)).await;
    assert!(!is_spendable_at(&output, chain_length, Some(1_800_000_000)));
    assert!(state.spendable_outputs().is_empty());
    assert_eq!(state.balance_at_height(chain_length).confirmed, 0);
}

/// wallet2: `get_blockchain_current_height() - 1 + 1 >= unlock_time`, with the
/// height being the wallet's chain length (wallet2.cpp:7454).
#[tokio::test]
async fn block_lock_boundary_matches_wallet2() {
    let (_, output, _) = locked_output(Timelock::Block(150)).await;
    assert!(!is_spendable_at(&output, 149, Some(0)));
    assert!(is_spendable_at(&output, 150, Some(0)));
}

/// wallet2: `adjusted_time + 120 >= unlock_time` after v2 (wallet2.cpp:7466-7468).
#[tokio::test]
async fn time_lock_boundary_matches_wallet2() {
    let unlock = 1_900_000_000u64;
    let (_, output, chain_length) = locked_output(Timelock::Time(unlock)).await;
    assert!(!is_spendable_at(&output, chain_length, Some(unlock - 121)));
    assert!(is_spendable_at(&output, chain_length, Some(unlock - 120)));
}

/// wallet2: `block_height + 10 > chain length` is locked (wallet2.cpp:7443),
/// and coinbase unlocks at chain length >= height + 60 through unlock_time.
#[tokio::test]
async fn standard_and_coinbase_age_match_wallet2_at_chain_length() {
    let (_, output, _) = locked_output(Timelock::None).await;
    assert_eq!(output.block_height, 100);
    assert!(!is_spendable_at(&output, 109, Some(0)));
    assert!(is_spendable_at(&output, 110, Some(0)));

    let recipient = Recipient::of(SEED, 0, 0);
    let reward = send_output(&recipient, Scalar::from(5002u64), 0, 6 * XMR);
    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(
            100,
            &[(&reward).into()],
            extra_with_pubkeys(&[reward.tx_pubkey]),
        ),
        vec![],
    );
    let coinbase = all_outputs(&scan(&chain, SEED).await).remove(0);
    assert!(coinbase.is_coinbase);
    assert!(!is_spendable_at(&coinbase, 159, Some(0)));
    assert!(is_spendable_at(&coinbase, 160, Some(0)));
}

#[tokio::test]
async fn stored_global_index_equals_the_daemon_index_for_that_output() {
    let recipient = Recipient::of(SEED, 0, 0);
    let stranger = Recipient::of(OTHER_SEED, 0, 0);
    let reward = send_output(&recipient, Scalar::from(6001u64), 1, 6 * XMR);
    let ours = send_output(&recipient, Scalar::from(6002u64), 1, 2 * XMR);
    let theirs = |salt: u64, o: usize| send_output(&stranger, Scalar::from(salt), o, XMR);

    let mut chain = FakeChain::new(100, 5000);
    chain.push_empty(1); // index 5000
    chain.push(
        // 5001, 5002
        miner_tx(
            101,
            &[burn_coinbase_output(XMR), (&reward).into()],
            extra_with_pubkeys(&[reward.tx_pubkey]),
        ),
        vec![
            // 5003, 5004
            ringct_tx(
                &[theirs(1, 0), theirs(1, 1)],
                extra_with_pubkeys(&[theirs(1, 0).tx_pubkey]),
                Timelock::None,
                1,
            ),
            // 5005, 5006
            ringct_tx(
                &[theirs(2, 0), ours.clone()],
                extra_with_pubkeys(&[ours.tx_pubkey]),
                Timelock::None,
                2,
            ),
        ],
    );
    let found = all_outputs(&scan(&chain, SEED).await);
    let index_of = |amount: u64| {
        found
            .iter()
            .find(|o| o.amount == amount)
            .unwrap()
            .global_index
    };
    assert_eq!(index_of(6 * XMR), Some(5002));
    assert_eq!(index_of(2 * XMR), Some(5006));
}

fn fingerprint(outputs: &[WalletOutput]) -> Vec<(String, u64, u64, String, Option<u64>)> {
    let mut rows: Vec<_> = outputs
        .iter()
        .map(|o| {
            (
                o.tx_hash.clone(),
                o.output_index,
                o.amount,
                o.key_image.clone(),
                o.global_index,
            )
        })
        .collect();
    rows.sort();
    rows
}

#[tokio::test]
async fn single_multi_and_split_batch_scans_agree_without_cross_credit() {
    let mut chain = FakeChain::new(100, 5000);
    for i in 0..6u64 {
        let height = chain.next_height();
        let a = send_output(
            &Recipient::of(SEED, 0, (i % 3) as u32),
            Scalar::from(7000 + i),
            0,
            XMR + i,
        );
        let b = send_output(
            &Recipient::of(OTHER_SEED, 0, 0),
            Scalar::from(7100 + i),
            1,
            2 * XMR + i,
        );
        chain.push(
            miner_tx(height, &[burn_coinbase_output(XMR)], vec![1; 33]),
            vec![
                ringct_tx(
                    &[a.clone()],
                    extra_with_pubkeys(&[a.tx_pubkey]),
                    Timelock::None,
                    i,
                ),
                ringct_tx(
                    &[
                        send_output(&Recipient::of(SEED, 1, 1), Scalar::from(7200 + i), 0, 5),
                        b.clone(),
                    ],
                    extra_with_pubkeys(&[b.tx_pubkey]),
                    Timelock::None,
                    100 + i,
                ),
            ],
        );
    }

    let single_a = all_outputs(&scan(&chain, SEED).await);
    let single_b = all_outputs(&scan(&chain, OTHER_SEED).await);
    assert_eq!(single_a.len(), 6);
    assert_eq!(single_b.len(), 6);

    let configs = || {
        [SEED, OTHER_SEED]
            .iter()
            .map(|seed| WalletScanConfig {
                mnemonic: seed.to_string(),
                network: Network::Mainnet,
                lookahead: LOOKAHEAD,
                passphrase: String::new(),
            })
            .collect::<Vec<_>>()
    };
    let (multi, _) =
        process_batch_multi_wallet_response(chain.response_from(100), 100, configs(), None)
            .await
            .unwrap();
    let address = |seed: &str| {
        monero_rust::derive_address(seed, Network::Mainnet, "")
            .unwrap()
            .to_string()
    };
    let multi_for = |seed: &str| -> Vec<WalletOutput> {
        multi
            .iter()
            .flat_map(|block| block.wallet_results[&address(seed)].outputs.clone())
            .collect()
    };
    assert_eq!(fingerprint(&multi_for(SEED)), fingerprint(&single_a));
    assert_eq!(fingerprint(&multi_for(OTHER_SEED)), fingerprint(&single_b));

    let mut cache = None;
    let mut split = Vec::new();
    for (from, to) in [(100u64, 103u64), (103, 106)] {
        let (results, cached) = process_batch_response(
            chain.response_range(from, to),
            from,
            SEED,
            Network::Mainnet,
            LOOKAHEAD,
            cache.take(),
            "",
        )
        .await
        .unwrap();
        cache = Some(cached);
        split.extend(all_outputs(&results));
    }
    assert_eq!(fingerprint(&split), fingerprint(&single_a));
}

#[tokio::test]
#[ignore = "known defect: frozen outputs are counted in the balance"]
async fn frozen_output_is_not_part_of_the_balance() {
    let (mut state, output, chain_length) = locked_output(Timelock::None).await;
    assert!(state.freeze_output(&output.key_image));
    let balance = state.balance_at_height(chain_length);
    assert_eq!(
        balance.confirmed + balance.unconfirmed,
        0,
        "wallet2 reports 0 balance and 0 unlocked balance for a wallet whose only output is frozen"
    );
}

#[tokio::test]
#[ignore = "known defect: balance() measures age from the top block height, one block late"]
async fn default_balance_unlocks_at_the_same_block_as_wallet2() {
    let recipient = Recipient::of(SEED, 0, 0);
    let mut chain = FakeChain::new(100, 5000);
    for (height, salt) in [(100u64, 1u64), (109, 2)] {
        while chain.next_height() < height {
            chain.push_empty(1);
        }
        let out = send_output(&recipient, Scalar::from(8000 + salt), 0, XMR);
        chain.push(
            miner_tx(height, &[burn_coinbase_output(XMR)], vec![1; 33]),
            vec![ringct_tx(
                &[out.clone()],
                extra_with_pubkeys(&[out.tx_pubkey]),
                Timelock::None,
                salt,
            )],
        );
    }
    let mut state = WalletState::new();
    state.add_outputs(all_outputs(&scan(&chain, SEED).await));
    assert_eq!(state.current_height, 109);
    assert_eq!(
        state.balance().confirmed,
        XMR,
        "the output in block 100 has ten blocks on the chain (100..=109)"
    );
}

fn chain_with_a_receipt_and_its_spend() -> FakeChain {
    let recipient = Recipient::of(SEED, 0, 1);
    let out = send_output(&recipient, Scalar::from(9101u64), 1, 4 * XMR);
    let other = send_output(
        &Recipient::of(OTHER_SEED, 0, 0),
        Scalar::from(9102u64),
        0,
        XMR,
    );
    let reward = send_output(
        &Recipient::of(SEED, 0, 0),
        Scalar::from(9103u64),
        0,
        6 * XMR,
    );
    let mut chain = FakeChain::new(100, 5000);
    chain.push(
        miner_tx(
            100,
            &[(&reward).into()],
            extra_with_pubkeys(&[reward.tx_pubkey]),
        ),
        vec![ringct_tx(
            &[other.clone(), out.clone()],
            extra_with_pubkeys(&[out.tx_pubkey]),
            Timelock::Block(150),
            1,
        )],
    );
    chain.push_empty(2);
    chain
}

#[tokio::test]
async fn pruned_blocks_report_the_same_outputs_and_spends_as_full_blocks() {
    let chain = chain_with_a_receipt_and_its_spend();
    let full = scan(&chain, SEED).await;
    assert_eq!(all_outputs(&full).len(), 2);
    let timelocks = |results: &[BlockScanResult]| -> Vec<_> {
        all_outputs(results)
            .iter()
            .map(|o| o.additional_timelock())
            .collect()
    };
    for with_prunable_hash in [false, true] {
        let (pruned, _) = process_batch_response(
            chain.pruned_response_from(100, with_prunable_hash),
            100,
            SEED,
            Network::Mainnet,
            LOOKAHEAD,
            None,
            "",
        )
        .await
        .expect("pruned scan");
        assert_eq!(
            fingerprint(&all_outputs(&pruned)),
            fingerprint(&all_outputs(&full))
        );
        for (a, b) in pruned.iter().zip(&full) {
            assert_eq!(a.block_hash, b.block_hash);
            assert_eq!(a.spent_key_images, b.spent_key_images);
            assert_eq!(a.spent_key_image_tx_hashes, b.spent_key_image_tx_hashes);
        }
        assert_eq!(timelocks(&pruned), timelocks(&full));
    }
}

#[tokio::test]
async fn pruned_transaction_with_the_wrong_prunable_hash_is_refused() {
    let chain = chain_with_a_receipt_and_its_spend();
    let mut response = chain.pruned_response_from(100, true);
    let blob = &mut response.blocks[0].txs[0];
    let last = blob.len() - 1;
    blob[last] ^= 1;
    let result =
        process_batch_response(response, 100, SEED, Network::Mainnet, LOOKAHEAD, None, "").await;
    assert!(result.is_err());
}
