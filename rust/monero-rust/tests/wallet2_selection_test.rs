//! Coin selection and transaction construction compared with wallet2 at 22578c3f7.

mod common;

use monero_rust::coin_selection::{estimate_fee_with_rate, select_inputs_with_fee_and_rng};
use monero_rust::tx_prepare::{
    prepare_sweep_inputs_at_with_fee, prepare_sweep_transactions, SweepSubaddresses,
};
use monero_rust::WalletOutput;
use monero_wallet::{interface::FeeRate, ringct::RctType};
use rand::SeedableRng;

const XMR: u64 = 1_000_000_000_000;
const RCT: RctType = RctType::ClsagBulletproofPlus;
const SEEDS: u64 = 32;

fn rate() -> FeeRate {
    FeeRate::new(20_000, 10_000).unwrap()
}

/// wallet2's `estimate_rct_tx_size` for CLSAG, Bulletproofs+, view tags and ring size 16.
fn wallet2_estimate_size(inputs: u64, outputs: u64) -> u64 {
    let ring = 16;
    let mut size = 1 + 6;
    size += inputs * (1 + 6 + ring * 2 + 32);
    size += outputs * (6 + 32);
    size += 1;
    let mut log_padded = 0;
    while (1u64 << log_padded) < outputs {
        log_padded += 1;
    }
    size += (2 * (6 + log_padded) + 6) * 32 + 3;
    size += inputs * (32 * ring + 64);
    size += outputs; // view tags
    size += 32 * inputs; // pseudoOuts
    size += 8 * outputs; // ecdhInfo
    size += 32 * outputs; // outPk
    size += 4; // txnFee
    size
}

/// wallet2's `estimate_tx_weight`, for two outputs or fewer.
fn wallet2_estimate_weight(inputs: u64, outputs: u64) -> u64 {
    assert!(outputs <= 2, "clawback is not modelled here");
    wallet2_estimate_size(inputs, outputs)
}

/// wallet2's `calculate_fee_from_weight`.
fn wallet2_fee(weight: u64) -> u64 {
    let (per_weight, mask) = (20_000u64, 10_000u64);
    (weight * per_weight + mask - 1) / mask * mask
}

/// wallet2's `fractional_threshold`: the fee one more ring costs.
fn wallet2_fractional_threshold() -> u64 {
    20_000 * (wallet2_estimate_weight(2, 2) - wallet2_estimate_weight(1, 2))
}

fn output(amount: u64, tx: &str, index: u8, height: u64, subaddress: (u32, u32)) -> WalletOutput {
    WalletOutput {
        tx_hash: tx.to_string(),
        output_index: index.into(),
        amount,
        key: "k".into(),
        key_offset: "ko".into(),
        commitment_mask: "cm".into(),
        subaddress_index: Some(subaddress),
        payment_id: None,
        received_output_bytes: String::new(),
        global_index: Some(1),
        ring: None,
        block_height: height,
        spent: false,
        spent_height: None,
        key_image: format!("ki_{tx}_{index}"),
        is_coinbase: false,
        frozen: false,
    }
}

fn select(seed: u64, outputs: &[WalletOutput], amount: u64) -> Result<Vec<WalletOutput>, String> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    select_inputs_with_fee_and_rng(&mut rng, outputs, amount, 1, None, 0, RCT, rate())
        .map(|selection| selection.selected)
}

fn names(selected: &[WalletOutput]) -> Vec<String> {
    let mut names: Vec<String> = selected.iter().map(WalletOutput::output_key).collect();
    names.sort();
    names
}

#[test]
fn single_preferred_input_is_the_first_in_wallet_order() {
    let outputs = [
        output(5 * XMR, "a", 0, 100, (0, 0)),
        output(2 * XMR, "b", 0, 300, (0, 0)),
        output(9 * XMR, "c", 0, 500, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(selected[0].tx_hash, "a");
    }
}

#[test]
fn preferred_pair_is_the_first_unrelated_pair_in_one_subaddress() {
    let outputs = [
        output(XMR, "a", 0, 100, (0, 0)),
        output(XMR, "b", 0, 101, (0, 0)), // adjacent block: related
        output(XMR, "c", 0, 300, (0, 1)), // other subaddress
        output(XMR, "d", 0, 500, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, 3 * XMR / 2).unwrap();
        assert_eq!(names(&selected), ["a:0", "d:0"]);
    }
}

#[test]
fn other_accounts_are_never_candidates() {
    let outputs = [
        output(XMR / 2, "a", 0, 100, (0, 0)),
        output(9 * XMR, "b", 0, 300, (1, 0)),
    ];
    assert!(select(0, &outputs, XMR).is_err());
}

#[test]
fn unneeded_second_input_from_the_same_transaction_is_not_added() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(XMR / 2, "a", 1, 100, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(
            names(&selected),
            ["a:0"],
            "wallet2 does not merge two outputs of one transaction when one suffices"
        );
    }
}

#[test]
fn unneeded_second_input_from_an_adjacent_block_is_not_added() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(XMR / 2, "b", 0, 101, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(names(&selected), ["a:0"]);
    }
}

#[test]
fn unneeded_second_input_is_not_taken_from_another_subaddress() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(XMR / 10, "b", 0, 500, (0, 5)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(
            names(&selected),
            ["a:0"],
            "the cleanup input links subaddress (0,5) to (0,0)"
        );
    }
}

#[test]
fn unneeded_second_input_is_the_smallest_unrelated_output() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(XMR / 2, "b", 0, 300, (0, 0)),
        output(XMR / 100, "c", 0, 500, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(names(&selected), ["a:0", "c:0"], "seed {seed}");
    }
}

#[test]
fn unneeded_second_input_respects_the_min_output_reservation() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(5 * XMR / 2, "b", 0, 300, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(names(&selected), ["a:0"]);
    }
}

#[test]
fn dust_below_the_fee_of_one_ring_is_never_selected() {
    let threshold = wallet2_fractional_threshold();
    for dust in [0, 5, threshold - 1] {
        let outputs = [
            output(3 * XMR, "a", 0, 100, (0, 0)),
            output(dust, "d", 0, 500, (0, 0)),
        ];
        for seed in 0..SEEDS {
            let selected = select(seed, &outputs, XMR).unwrap();
            assert!(
                selected.iter().all(|output| output.amount >= threshold),
                "an output of {dust} piconero was selected; one more ring costs {threshold}"
            );
        }
    }
}

#[test]
fn zero_amount_output_is_never_a_send_input() {
    let outputs = [
        output(3 * XMR, "a", 0, 100, (0, 0)),
        output(0, "z", 0, 500, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(names(&selected), ["a:0"], "seed {seed}");
    }
}

#[test]
fn zero_amount_outputs_do_not_flood_a_multi_input_send() {
    let mut outputs: Vec<WalletOutput> = (0..3u64)
        .map(|i| output(XMR, &format!("real{i}"), 0, 100 + 1000 * i, (0, 0)))
        .collect();
    for i in 0..60u64 {
        outputs.push(output(0, &format!("zero{i}"), 0, 5_000 + 20 * i, (0, 0)));
    }
    let minimal_fee = estimate_fee_with_rate(3, 2, RCT, rate()).unwrap();
    let mut counts = Vec::new();
    let mut fees = Vec::new();
    for seed in 0..SEEDS {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let selection = select_inputs_with_fee_and_rng(
            &mut rng,
            &outputs,
            5 * XMR / 2,
            1,
            None,
            0,
            RCT,
            rate(),
        )
        .unwrap();
        counts.push(selection.selected.len());
        fees.push(selection.estimated_fee);
    }
    let worst = *counts.iter().max().unwrap();
    let mean = counts.iter().sum::<usize>() / counts.len();
    assert!(
        counts.iter().all(|count| *count == 3),
        "inputs per send over {SEEDS} seeds: mean {mean}, worst {worst}; fee bound up to {} \
         against {minimal_fee} for the three real inputs",
        fees.iter().max().unwrap()
    );
}

#[test]
fn zero_amount_outputs_do_not_make_a_send_unbuildable() {
    let mut outputs: Vec<WalletOutput> = (0..3u64)
        .map(|i| output(XMR, &format!("real{i}"), 0, 100 + 1000 * i, (0, 0)))
        .collect();
    for i in 0..600u64 {
        outputs.push(output(0, &format!("zero{i}"), 0, 5_000 + 20 * i, (0, 0)));
    }
    let smallest_input_weight = 1 + 1 + 1 + 16 + 32 + (16 * 32 + 64) + 32;
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, 5 * XMR / 2).unwrap();
        assert!(
            selected.len() * smallest_input_weight < 149_400,
            "seed {seed}: {} inputs selected, at least {} weight",
            selected.len(),
            selected.len() * smallest_input_weight
        );
    }
}

#[test]
fn zero_amount_outputs_do_not_make_a_sweep_unbuildable() {
    let mut outputs = vec![output(XMR, "real", 0, 100, (0, 0))];
    for i in 0..300u64 {
        outputs.push(output(0, &format!("zero{i}"), 0, 5_000 + 20 * i, (0, 0)));
    }
    let prepared =
        prepare_sweep_inputs_at_with_fee(&outputs, 100_000, Some(0), None, None, 0, RCT, rate())
            .unwrap();
    assert_eq!(
        prepared.stored_outputs.len(),
        1,
        "fee bound {} against {} for the one real input",
        prepared.estimated_fee,
        estimate_fee_with_rate(1, 2, RCT, rate()).unwrap()
    );
}

#[test]
fn dust_does_not_turn_an_affordable_send_into_insufficient_funds() {
    let fee_one_input = estimate_fee_with_rate(1, 2, RCT, rate()).unwrap();
    let outputs = [
        output(XMR + fee_one_input + 1_000, "a", 0, 100, (0, 0)),
        output(5, "d", 0, 500, (0, 0)),
    ];
    assert_eq!(names(&select(0, &outputs[..1], XMR).unwrap()), ["a:0"]);
    for seed in 0..SEEDS {
        let selected =
            select(seed, &outputs, XMR).unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        assert_eq!(names(&selected), ["a:0"]);
    }
}

#[test]
fn output_below_the_threshold_does_not_complete_a_preferred_pair_unlike_wallet2() {
    let amount = XMR / 2;
    let preferred_fee = wallet2_fee(wallet2_estimate_weight(2, 2));
    let outputs = [
        output(amount + preferred_fee - 1_000, "a", 0, 100, (0, 0)),
        output(wallet2_fractional_threshold() - 1, "b", 0, 200, (0, 0)),
    ];
    for seed in 0..SEEDS {
        assert_eq!(names(&select(seed, &outputs, amount).unwrap()), ["a:0"]);
    }
}

#[test]
fn single_preferred_input_must_cover_the_two_input_fee() {
    let fee_one_input = estimate_fee_with_rate(1, 2, RCT, rate()).unwrap();
    let wallet2_needed = XMR + wallet2_fee(wallet2_estimate_weight(2, 2));
    let a = XMR + fee_one_input + 1_000_000;
    assert!(
        a < wallet2_needed,
        "\"a\" must not be a wallet2 preferred input"
    );
    let outputs = [
        output(a, "a", 0, 100, (0, 0)),
        output(10 * XMR, "b", 0, 100, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, XMR).unwrap();
        assert_eq!(names(&selected), ["b:0"]);
    }
}

#[test]
fn two_outputs_of_one_transaction_are_not_a_preferred_pair() {
    let outputs = [
        output(XMR, "a", 0, 100, (0, 0)),
        output(XMR, "a", 1, 100, (0, 0)),
        output(XMR / 10, "c", 0, 500, (0, 0)),
    ];
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, 3 * XMR / 2).unwrap();
        assert_eq!(names(&selected), ["a:0", "a:1", "c:0"], "seed {seed}");
    }
}

#[test]
fn fallback_uses_up_one_subaddress_before_touching_another() {
    let mut outputs = Vec::new();
    for i in 0..4u64 {
        outputs.push(output(XMR, &format!("p{i}"), 0, 100 + 100 * i, (0, 1)));
        outputs.push(output(
            9 * XMR / 10,
            &format!("q{i}"),
            0,
            150 + 100 * i,
            (0, 2),
        ));
    }
    for seed in 0..SEEDS {
        let selected = select(seed, &outputs, 5 * XMR / 2).unwrap();
        assert!(
            selected
                .iter()
                .all(|output| output.subaddress_index == Some((0, 1))),
            "seed {seed}: inputs from {:?}",
            selected
                .iter()
                .map(|output| output.subaddress_index.unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(selected.len(), 3);
    }
}

#[test]
fn selection_stays_under_the_wallet2_weight_target() {
    let limit = 300_000 / 2 - 600; // wallet2.cpp:12477-12486
    let target = limit * 2 / 3;
    let outputs: Vec<WalletOutput> = (0..200u64)
        .map(|i| output(XMR, &format!("t{i}"), 0, 100 + 20 * i, (0, 0)))
        .collect();
    match select(0, &outputs, 180 * XMR) {
        // Refusing is acceptable: monero-rust does not split payments.
        Err(_) => {}
        Ok(selected) => {
            let weight = wallet2_estimate_weight(selected.len() as u64, 2);
            assert!(
                weight < target,
                "{} inputs, wallet2 estimated weight {weight}, wallet2 splits at {target}",
                selected.len()
            );
        }
    }
}

#[test]
fn sweep_of_many_outputs_is_split_like_wallet2() {
    let outputs: Vec<WalletOutput> = (0..165u64)
        .map(|i| output(XMR / 10, &format!("t{i}"), 0, 100 + i / 15, (0, 0)))
        .collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let transactions = prepare_sweep_transactions(
        &mut rng,
        &outputs,
        1_000,
        Some(0),
        SweepSubaddresses::OneAtRandom,
        None,
        0,
        RCT,
        rate(),
    )
    .unwrap();
    let inputs: Vec<usize> = transactions
        .iter()
        .map(|transaction| transaction.stored_outputs.len())
        .collect();
    assert_eq!(inputs, [146, 19]);

    let one =
        prepare_sweep_inputs_at_with_fee(&outputs, 1_000, Some(0), None, None, 0, RCT, rate());
    assert!(one.is_err());
}

#[tokio::test]
async fn construction_refuses_more_than_one_transaction_before_asking_the_node() {
    use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
    use monero_rust::native::{create_transaction_with_rpc, sweep_all_with_rpc, StoredOutputData};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Debug, Default)]
    struct Node(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl RpcConnection for Node {
        async fn post(&self, _: &str, _: Vec<u8>) -> Result<Vec<u8>, RpcError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(RpcError::ConnectionError)
        }
    }

    const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
        sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
        fences goat unquoted";
    let address = monero_rust::scanner::derive_address(SEED, monero_rust::Network::Mainnet, "")
        .unwrap()
        .to_string();
    let output = StoredOutputData {
        tx_hash: String::new(),
        output_index: 0,
        amount: XMR,
        key: String::new(),
        key_offset: String::new(),
        commitment_mask: String::new(),
        subaddress: None,
        payment_id: None,
        received_output_bytes: String::new(),
        global_index: None,
        key_image: String::new(),
        ring: None,
    };
    let node = Node::default();
    let rpc = Rpc::new_with_connection(node.clone());
    let pay = |inputs: usize| {
        let recipients = [(address.clone(), XMR)];
        let outputs = vec![output.clone(); inputs];
        let rpc = rpc.clone();
        async move {
            let mut wallet_state = monero_rust::WalletState::new();
            create_transaction_with_rpc(
                &rpc,
                SEED,
                "mainnet",
                outputs,
                &recipients,
                "",
                &mut wallet_state,
            )
            .await
            .map(|_| ())
            .unwrap_err()
        }
    };
    let sweep = |inputs: usize| {
        let outputs = vec![output.clone(); inputs];
        let (rpc, address) = (rpc.clone(), address.clone());
        async move {
            let mut wallet_state = monero_rust::WalletState::new();
            sweep_all_with_rpc(
                &rpc,
                SEED,
                "mainnet",
                outputs,
                &address,
                "",
                &mut wallet_state,
            )
            .await
            .map(|_| ())
            .unwrap_err()
        }
    };

    let error = pay(146).await;
    assert!(error.contains("146 inputs"), "{error}");
    let error = sweep(147).await;
    assert!(error.contains("147 inputs"), "{error}");
    let error = sweep(1_000).await;
    assert!(error.contains("do not fit"), "{error}");
    assert_eq!(node.0.load(Ordering::SeqCst), 0);

    pay(145).await;
    assert_eq!(node.0.load(Ordering::SeqCst), 1);
    sweep(146).await;
    assert_eq!(node.0.load(Ordering::SeqCst), 2);
}

#[test]
fn weight_bound_refuses_counts_that_overflow() {
    let result = std::panic::catch_unwind(|| {
        monero_rust::fee_estimate::transaction_weight_upper_bound(RCT, usize::MAX / 4, 2, true)
    });
    assert!(
        matches!(result, Ok(Err(_))),
        "expected an error, got {:?}",
        result.map_err(|_| "a panic")
    );
}

#[test]
fn sweep_without_indices_spends_one_subaddress() {
    let outputs = [
        output(XMR, "a", 0, 100, (0, 0)),
        output(XMR, "b", 0, 300, (0, 3)),
        output(XMR, "c", 0, 500, (0, 7)),
    ];
    let prepared =
        prepare_sweep_inputs_at_with_fee(&outputs, 1_000, Some(0), None, None, 0, RCT, rate())
            .unwrap();
    let mut subaddresses: Vec<_> = prepared
        .stored_outputs
        .iter()
        .map(|stored| stored.subaddress.unwrap())
        .collect();
    subaddresses.sort();
    subaddresses.dedup();
    assert_eq!(
        subaddresses.len(),
        1,
        "one sweep transaction links subaddresses {subaddresses:?}"
    );
}

#[test]
fn sweep_leaves_out_dust_below_the_fee_of_one_ring() {
    let threshold = wallet2_fractional_threshold();
    let outputs = [
        output(XMR, "a", 0, 100, (0, 0)),
        output(0, "z", 0, 300, (0, 0)),
        output(threshold - 1, "d", 0, 500, (0, 0)),
    ];
    let prepared =
        prepare_sweep_inputs_at_with_fee(&outputs, 1_000, Some(0), None, None, 0, RCT, rate())
            .unwrap();
    let amounts: Vec<u64> = prepared
        .stored_outputs
        .iter()
        .map(|stored| stored.amount)
        .collect();
    assert_eq!(amounts, [XMR]);
}

#[test]
fn fee_subtraction_spreads_the_remainder_like_wallet2() {
    let recipients = vec![
        ("a".to_string(), 100u64),
        ("b".to_string(), 100),
        ("c".to_string(), 100),
    ];
    let adjusted = monero_rust::adjust_recipients_for_fee(&recipients, 11);
    let amounts: Vec<u64> = adjusted.iter().map(|recipient| recipient.1).collect();
    assert_eq!(amounts, [96, 96, 97]);
}

#[test]
fn fee_subtraction_never_yields_a_zero_or_short_adjustment() {
    let recipients = vec![("a".to_string(), 10u64), ("b".to_string(), 1_000_000)];
    assert!(monero_rust::adjust_recipients_for_fee(&recipients, 1_000).is_empty());
}

#[test]
fn zero_amount_destination_is_refused() {
    use common::offline::{unsigned_for, Wallet};
    use curve25519_dalek::scalar::Scalar;
    use monero_rust::unsigned_tx::sign_offline;
    use monero_wallet::address::Network;
    use zeroize::Zeroizing;

    let sender = Wallet::from_spend(Scalar::from(2_345_678u64));
    let recipient = Wallet::from_spend(Scalar::from(777u64)).address(Network::Stagenet);
    let unsigned = unsigned_for(&sender, Network::Stagenet, vec![(recipient, 0)]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let signed = sign_offline(
        &mut rng,
        &Zeroizing::new(sender.spend),
        &Zeroizing::new(sender.view),
        &unsigned,
    );
    assert!(signed.is_err(), "a payment of zero was signed");
}

fn sign_shape(
    unsigned: &monero_rust::unsigned_tx::UnsignedTransaction,
    sender: &common::offline::Wallet,
) -> monero_wallet::extra::Extra {
    use monero_rust::unsigned_tx::sign_offline;
    use zeroize::Zeroizing;

    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let signed = sign_offline(
        &mut rng,
        &Zeroizing::new(sender.spend),
        &Zeroizing::new(sender.view),
        unsigned,
    )
    .expect("sign");
    monero_wallet::extra::Extra::read(&mut signed.tx.prefix().extra.as_slice()).expect("extra")
}

#[test]
fn paying_the_change_subaddress_and_a_standard_address_uses_no_additional_keys() {
    use common::offline::{unsigned_for, Wallet, PAYMENT_AMOUNT};
    use curve25519_dalek::scalar::Scalar;
    use monero_rust::unsigned_tx::UnsignedChange;
    use monero_wallet::address::{Network, SubaddressIndex};

    let sender = Wallet::from_spend(Scalar::from(2_345_678u64));
    let own_change = sender.subaddress(Network::Stagenet, 1, 0);
    let standard = Wallet::from_spend(Scalar::from(777u64)).address(Network::Stagenet);
    let mut unsigned = unsigned_for(
        &sender,
        Network::Stagenet,
        vec![
            (own_change, PAYMENT_AMOUNT / 2),
            (standard, PAYMENT_AMOUNT / 2),
        ],
    );
    unsigned.source_account = 1;
    unsigned.change = Some(UnsignedChange::Own {
        address: own_change,
        subaddress: SubaddressIndex::new(1, 0),
    });
    let extra = sign_shape(&unsigned, &sender);
    let (_, additional) = extra.keys().expect("transaction key");
    assert_eq!(additional.map_or(0, |keys| keys.len()), 0);
}

#[test]
fn two_destinations_without_change_carry_no_dummy_payment_id() {
    use common::offline::{unsigned_for, Wallet, FEE, INPUT_AMOUNT};
    use curve25519_dalek::scalar::Scalar;
    use monero_wallet::address::Network;

    let sender = Wallet::from_spend(Scalar::from(2_345_678u64));
    let first = Wallet::from_spend(Scalar::from(777u64)).address(Network::Stagenet);
    let second = Wallet::from_spend(Scalar::from(778u64)).address(Network::Stagenet);
    let half = (INPUT_AMOUNT - FEE) / 2;
    let mut unsigned = unsigned_for(
        &sender,
        Network::Stagenet,
        vec![(first, half), (second, INPUT_AMOUNT - FEE - half)],
    );
    unsigned.change = None;
    let extra = sign_shape(&unsigned, &sender);
    assert!(
        extra.payment_id().is_none(),
        "wallet2 writes only the transaction public key here"
    );
}
