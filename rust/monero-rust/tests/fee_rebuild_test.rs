mod ring_reuse;

use std::collections::BTreeMap;

use monero_rust::monero_rpc::FeePriority;
use monero_rust::native::DecoyPolicy;
use monero_rust::{PreparedInputs, WalletState};
use ring_reuse::*;

const TWO_INPUT_ESTIMATE: u64 = 2171 * 20_000;
const AMOUNT: u64 = 3 * XMR;

fn wallet(node: &FakeNode) -> (WalletState, [Owned; 3]) {
    let base = node.selectable_outputs() - 500;
    let owned = [
        owned_output(1, base, 50_000, 2 * XMR),
        owned_output(2, base - 97, 50_020, XMR + TWO_INPUT_ESTIMATE),
        owned_output(3, base - 194, 50_040, XMR),
    ];
    for output in &owned {
        node.set_output(output.global_index(), output.key, output.commitment);
    }
    let (state, _) = wallet_holding(owned.iter().map(|o| o.output.clone()).collect());
    (state, owned)
}

async fn prepare(node: &FakeNode, state: &WalletState) -> PreparedInputs {
    let rpc = node.rpc();
    let height = rpc.get_height().await.unwrap() as u64;
    monero_rust::prepare_send_inputs_with_rpc(
        &rpc,
        state.outputs(),
        height,
        AMOUNT,
        1,
        false,
        None,
        None,
        0,
        policy(FeePriority::Automatic),
    )
    .await
    .unwrap()
}

fn candidate_sets(node: &FakeNode, owned: &Owned) -> Vec<Vec<u64>> {
    node.get_outs_log()
        .iter()
        .flat_map(|call| call.chunks(75).map(<[u64]>::to_vec).collect::<Vec<_>>())
        .filter(|set| set.contains(&owned.global_index()))
        .collect()
}

fn recorded_rings(state: &WalletState, owned: &[Owned]) -> BTreeMap<String, Vec<u64>> {
    owned
        .iter()
        .filter_map(|o| {
            state
                .ring_database()
                .ring(o.key_image())
                .map(|ring| (o.key_image().to_string(), ring_indexes(ring)))
        })
        .collect()
}

async fn pair_falls_short(
    node: &FakeNode,
    state: &mut WalletState,
    owned: &[Owned; 3],
) -> PreparedInputs {
    let prepared = prepare(node, state).await;
    assert_eq!(
        prepared.fee_policy.map(|p| p.priority),
        Some(FeePriority::Low)
    );
    let chosen: Vec<_> = prepared
        .stored_outputs
        .iter()
        .map(|s| s.key_image.clone())
        .collect();
    assert_eq!(chosen, [owned[1].key_image(), owned[0].key_image()]);
    let further: Vec<_> = prepared
        .further_inputs
        .iter()
        .map(|s| s.key_image.clone())
        .collect();
    assert_eq!(further, [owned[2].key_image()]);

    let error = hot_send_with(
        node,
        state,
        prepared.stored_outputs.clone(),
        &destination(0),
        AMOUNT,
        FeePriority::Low,
    )
    .await
    .unwrap_err();
    assert!(error.contains("Insufficient funds"), "{error}");
    assert_eq!(node.get_outs_log().len(), 1);
    prepared
}

fn assert_rebuild_kept_what_was_recorded(
    node: &FakeNode,
    owned: &[Owned; 3],
    before: &BTreeMap<String, Vec<u64>>,
    built: &BTreeMap<String, Vec<u64>>,
) {
    assert_eq!(
        before.len(),
        2,
        "the pair's rings were recorded by the first build"
    );
    assert_eq!(built.len(), 3, "the rebuild spends the added input too");
    for (key_image, ring) in before {
        assert_same_ring("an input built again", ring, &built[key_image]);
    }
    for o in &owned[..2] {
        let sets = candidate_sets(node, o);
        assert_eq!(
            sets.len(),
            3,
            "requested by the failed build, the retry and the rebuild"
        );
        assert!(
            sets.iter().all(|set| *set == sets[0]),
            "one candidate set per input"
        );
    }
    assert_eq!(
        candidate_sets(node, &owned[2]).len(),
        1,
        "the added input is requested once"
    );
}

#[tokio::test]
async fn hot_send_adds_an_input_when_the_real_fee_is_not_covered() {
    let node = FakeNode::uniform();
    let (mut state, owned) = wallet(&node);
    let prepared = pair_falls_short(&node, &mut state, &owned).await;
    let before = recorded_rings(&state, &owned);

    let result = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        policy(FeePriority::Normal),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap();
    let built = rings_in_signed(&result.tx_blob);
    assert_rebuild_kept_what_was_recorded(&node, &owned, &before, &built);
    assert_eq!(recorded_rings(&state, &owned), built);
    let spent: Vec<_> = result
        .input_rings
        .iter()
        .map(|r| r.key_image.clone())
        .collect();
    assert_eq!(spent.len(), 3);
    assert!(owned
        .iter()
        .all(|o| spent.iter().any(|k| k == o.key_image())));
    assert!(
        result.fee < 3_500 * 20_000,
        "a three-input transaction at the Low tier, not Normal: fee {}",
        result.fee
    );
    assert_eq!(result.change_outputs.len(), 1);
    assert_eq!(
        result.change_outputs[0].amount + result.fee + AMOUNT,
        4 * XMR + TWO_INPUT_ESTIMATE
    );
}

#[tokio::test]
async fn unsigned_send_adds_an_input_when_the_real_fee_is_not_covered() {
    let node = FakeNode::uniform();
    let (mut state, owned) = wallet(&node);
    let prepared = pair_falls_short(&node, &mut state, &owned).await;
    let before = recorded_rings(&state, &owned);

    let keys = keys();
    let result = monero_rust::native::create_unsigned_transaction_from_prepared_inputs(
        &node.rpc(),
        &keys.view_hex,
        &keys.public_spend_hex,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        policy(FeePriority::Low),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap();
    assert_eq!(rings_in_unsigned(&result.unsigned_tx_hex).len(), 3);
    let built = owned
        .iter()
        .map(|o| {
            let ring = unsigned_ring(&result.unsigned_tx_hex, o);
            (o.key_image().to_string(), ring)
        })
        .collect();
    assert_rebuild_kept_what_was_recorded(&node, &owned, &before, &built);
    assert_eq!(recorded_rings(&state, &owned), built);
    assert_eq!(result.input_rings.len(), 3);
}

#[tokio::test]
async fn prepared_send_without_a_reserve_fails_after_one_build() {
    let node = FakeNode::uniform();
    let (mut state, owned) = wallet(&node);
    let mut prepared = prepare(&node, &state).await;
    prepared.further_inputs.clear();
    let error = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        policy(FeePriority::Low),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap_err();
    assert!(error.contains("Insufficient funds"), "{error}");
    assert_eq!(node.get_outs_log().len(), 1);
    assert!(candidate_sets(&node, &owned[2]).is_empty());
}

#[tokio::test]
async fn prepared_send_short_of_the_estimate_draws_on_its_reserve() {
    let node = FakeNode::uniform();
    let (mut state, owned) = wallet(&node);
    let mut prepared = prepare(&node, &state).await;
    let mut all = prepared.stored_outputs.clone();
    all.extend(prepared.further_inputs.iter().cloned());
    assert_eq!(all[0].key_image, owned[1].key_image());
    prepared.stored_outputs = vec![all[0].clone()];
    prepared.further_inputs = all[1..].to_vec();

    let result = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        policy(FeePriority::Low),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap();
    assert_eq!(result.input_rings.len(), 3);
    let first = &node.get_outs_log()[0];
    assert!(first.contains(&owned[0].global_index()));
    assert!(first.contains(&owned[1].global_index()));
}

#[tokio::test]
async fn prepared_send_no_reserve_can_cover_asks_for_no_decoys() {
    let node = FakeNode::uniform();
    let (mut state, owned) = wallet(&node);
    let mut prepared = prepare(&node, &state).await;
    let mut all = prepared.stored_outputs.clone();
    all.extend(prepared.further_inputs.iter().cloned());
    assert_eq!(all[2].key_image, owned[2].key_image());
    prepared.stored_outputs = vec![all[2].clone()];
    prepared.further_inputs = vec![all[0].clone()];

    let error = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        policy(FeePriority::Low),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap_err();
    assert!(error.contains("Insufficient funds"), "{error}");
    assert!(node.get_outs_log().is_empty());
}

#[tokio::test]
async fn a_prepared_send_is_built_at_the_rate_it_was_prepared_at() {
    use monero_oxide::transaction::{NotPruned, Transaction};
    use monero_rust::monero_rpc::Fee;
    let node = FakeNode::uniform();
    let (mut state, _) = wallet(&node);
    let mut prepared = prepare(&node, &state).await;
    assert_eq!(
        prepared.fee_rate,
        Some(Fee {
            per_weight: 20_000,
            mask: 10_000
        })
    );
    prepared.fee_rate = Some(Fee {
        per_weight: 30_000,
        mask: 10_000,
    });

    let mut capped = policy(FeePriority::Low);
    capped.max_fee_per_weight = 25_000;
    let error = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        capped,
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap_err();
    assert!(error.contains("FeeExceedsLimit"), "{error}");
    assert!(node.get_outs_log().is_empty(), "refused before any decoys");

    let result = monero_rust::native::create_transaction_from_prepared_inputs(
        &node.rpc(),
        SEED,
        NETWORK,
        &prepared,
        &[(destination(0), AMOUNT)],
        "",
        policy(FeePriority::Low),
        &DecoyPolicy::default(),
        &mut state,
    )
    .await
    .unwrap();
    let bytes = hex::decode(&result.tx_blob).unwrap();
    let weight = Transaction::<NotPruned>::read(&mut bytes.as_slice())
        .unwrap()
        .weight() as u64;
    assert_eq!(result.fee, (weight * 30_000).div_ceil(10_000) * 10_000);
}
