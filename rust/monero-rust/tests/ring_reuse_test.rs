//! A key image never gets a newly drawn ring once its ring or candidate set has left the process.

#![cfg(not(target_arch = "wasm32"))]

mod ring_reuse;

use std::collections::BTreeSet;

use monero_rust::monero_rpc::FeePriority;
use monero_rust::native::{InputRing, StoredOutputData};
use monero_rust::{PendingSpend, WalletState, PENDING_SPEND_TTL_SECS};
use ring_reuse::*;

const NORMAL: FeePriority = FeePriority::Normal;

#[tokio::test]
#[ignore = "known defect: after a crash the hot builder draws new candidates, and the daemon can intersect the two requests to find the real output"]
async fn crash_after_selection_shows_the_daemon_the_same_candidates_hot() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let first_ring = signed_ring(&first.tx_blob, a);
    drop(first);
    drop(state);

    let mut state = crash_and_reload(&disk);
    let _second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();

    assert!(node.seen_txs().is_empty(), "nothing was relayed");
    let learned = candidate_intersection(&node.get_outs_log(), a.global_index());
    assert!(
        first_ring.iter().all(|index| learned.contains(index)),
        "the daemon intersected two candidate requests down to {learned:?}; the real output is \
         {} and the first ring had 16 members",
        a.global_index()
    );
}

#[tokio::test]
#[ignore = "known defect: after a crash the cold builder draws new candidates, and the daemon can intersect the two requests to find the real output"]
async fn crash_after_selection_shows_the_daemon_the_same_candidates_cold() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = cold_build(&node, &mut state, &[a.key_image()], &destination(0), XMR)
        .await
        .unwrap();
    let first_ring = unsigned_ring(&first.unsigned_tx_hex, a);
    drop(first);
    drop(state);

    let mut state = crash_and_reload(&disk);
    let _second = cold_build(&node, &mut state, &[a.key_image()], &destination(0), XMR)
        .await
        .unwrap();

    let learned = candidate_intersection(&node.get_outs_log(), a.global_index());
    assert!(
        first_ring.iter().all(|index| learned.contains(index)),
        "the daemon intersected two candidate requests down to {learned:?}; the real output is \
         {}",
        a.global_index()
    );
}

#[tokio::test]
async fn rings_drawn_up_front_and_saved_are_fixed_before_any_build() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let drawn = draw_rings(&node, &mut state, &[a.key_image()])
        .await
        .unwrap();
    state
        .record_input_rings(&[InputRing {
            key_image: a.key_image().to_string(),
            ring: drawn[0].clone(),
        }])
        .unwrap();
    let ring = ring_indexes(&drawn[0]);
    disk.save(&state);

    for amount in [XMR, 2 * XMR] {
        let mut state = crash_and_reload(&disk);
        let built = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(0),
            amount,
            NORMAL,
        )
        .await
        .unwrap();
        assert_same_ring(
            "ring drawn up front and a later build",
            &ring,
            &signed_ring(&built.tx_blob, a),
        );
    }
    let learned = candidate_intersection(&node.get_outs_log(), a.global_index());
    assert!(ring.iter().all(|index| learned.contains(index)));
    assert!(learned.len() >= 16);
}

#[tokio::test]
#[ignore = "known defect: the hot builders record nothing, so two builds of one output return two different rings"]
async fn two_hot_builds_of_one_output_carry_one_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();

    assert_same_ring(
        "signed bytes returned by two builds",
        &signed_ring(&first.tx_blob, a),
        &signed_ring(&second.tx_blob, a),
    );
}

#[tokio::test]
async fn cold_ring_survives_a_crash_when_state_is_saved_before_export() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = cold_build(&node, &mut state, &[a.key_image()], &destination(0), XMR)
        .await
        .unwrap();
    disk.save(&state);
    let exported = first.unsigned_tx_hex.clone();
    drop(state);

    let mut state = crash_and_reload(&disk);
    let rebuilt = cold_build(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        2 * XMR,
    )
    .await
    .unwrap();

    assert_same_ring(
        "unsigned envelopes exported before and after a crash",
        &unsigned_ring(&exported, a),
        &unsigned_ring(&rebuilt.unsigned_tx_hex, a),
    );
}

#[tokio::test]
#[ignore = "known defect: the cold builder returns the envelope with its ring recorded in memory only, so a crash before the caller saves loses the exported ring"]
async fn unsigned_envelope_ring_is_durable_before_the_envelope_is_returned() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = cold_build(&node, &mut state, &[a.key_image()], &destination(0), XMR)
        .await
        .unwrap();
    let exported = first.unsigned_tx_hex.clone();
    drop(state);

    let mut state = crash_and_reload(&disk);
    let rebuilt = cold_build(&node, &mut state, &[a.key_image()], &destination(0), XMR)
        .await
        .unwrap();

    assert_same_ring(
        "unsigned envelopes exported before and after a crash",
        &unsigned_ring(&exported, a),
        &unsigned_ring(&rebuilt.unsigned_tx_hex, a),
    );
}

#[tokio::test]
async fn cold_signed_transaction_carries_the_saved_ring_and_a_rebuild_reuses_it() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 2);
    let (a, b) = (&owned[0], &owned[1]);
    let both = [a.key_image(), b.key_image()];

    let unsigned = cold_build(&node, &mut state, &both, &destination(0), 7 * XMR)
        .await
        .unwrap();
    disk.save(&state);
    let signed = cold_sign(&unsigned.unsigned_tx_hex);
    for owned in [a, b] {
        assert_same_ring(
            "exported envelope and the transaction signed from it",
            &unsigned_ring(&unsigned.unsigned_tx_hex, owned),
            &signed_ring(&signed.tx_blob, owned),
        );
    }
    drop(state);

    let mut state = crash_and_reload(&disk);
    let rebuilt = cold_build(&node, &mut state, &both, &destination(1), 6 * XMR)
        .await
        .unwrap();
    let resigned = cold_sign(&rebuilt.unsigned_tx_hex);
    for owned in [a, b] {
        assert_same_ring(
            "transactions signed before and after a crash of the online wallet",
            &signed_ring(&signed.tx_blob, owned),
            &signed_ring(&resigned.tx_blob, owned),
        );
    }
}

#[tokio::test]
#[ignore = "known defect: the hot builders return signed bytes with no ring recorded anywhere, so a crash before the caller records and saves loses the ring"]
async fn signed_bytes_ring_is_durable_before_the_bytes_are_returned() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let first_bytes = first.tx_blob.clone();
    drop(first);
    drop(state);

    let mut state = crash_and_reload(&disk);
    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();

    assert_same_ring(
        "signed bytes returned before and after a crash",
        &signed_ring(&first_bytes, a),
        &signed_ring(&second.tx_blob, a),
    );
}

#[tokio::test]
async fn hot_ring_survives_crashes_when_the_caller_saves_before_relay() {
    for outcome in [Relay::Accept, Relay::AcceptButLoseReply, Relay::Unreachable] {
        let node = FakeNode::uniform();
        let (mut state, owned, disk) = funded(&node, 1);
        let a = &owned[0];

        let first = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(0),
            XMR,
            NORMAL,
        )
        .await
        .unwrap();
        first.persist_input_rings(&mut state).unwrap();
        disk.save(&state);

        node.set_relay(outcome);
        let relayed = relay(&node, &mut state, &first).await;
        assert_eq!(relayed.is_ok(), outcome == Relay::Accept);
        drop(state);

        let mut state = crash_and_reload(&disk);
        let second = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(1),
            2 * XMR,
            NORMAL,
        )
        .await
        .unwrap();
        assert_same_ring(
            "transactions built before and after a crash around relay",
            &signed_ring(&first.tx_blob, a),
            &signed_ring(&second.tx_blob, a),
        );
    }
}

async fn relay_then_crash_before_the_next_save(outcome: Relay) {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    node.set_relay(outcome);
    let relayed = relay(&node, &mut state, &first).await;
    assert_eq!(relayed.is_ok(), outcome == Relay::Accept);
    assert_eq!(node.seen_txs().len(), 1, "the daemon has the transaction");
    drop(state);

    let mut state = crash_and_reload(&disk);
    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    node.set_relay(Relay::Reject);
    let _ = relay(&node, &mut state, &second).await;

    assert_eq!(node.seen_txs().len(), 2);
    assert_daemon_saw_one_ring(&node, a);
}

#[tokio::test]
#[ignore = "known defect: the relay helper records the ring in memory and relays in one call, so a crash before the caller's next save loses the ring of a relayed transaction"]
async fn crash_after_relay_keeps_the_relayed_ring() {
    relay_then_crash_before_the_next_save(Relay::Accept).await;
}

#[tokio::test]
#[ignore = "known defect: the relay helper records the ring in memory and relays in one call, so a crash before the caller's next save loses the ring of a relayed transaction"]
async fn crash_after_a_relay_whose_reply_was_lost_keeps_the_ring() {
    relay_then_crash_before_the_next_save(Relay::AcceptButLoseReply).await;
}

#[tokio::test]
async fn ring_stays_recorded_when_relay_fails_or_its_reply_is_lost() {
    for outcome in [Relay::Unreachable, Relay::Reject, Relay::AcceptButLoseReply] {
        let node = FakeNode::uniform();
        let (mut state, owned, _disk) = funded(&node, 1);
        let a = &owned[0];

        let first = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(0),
            XMR,
            NORMAL,
        )
        .await
        .unwrap();
        node.set_relay(outcome);
        relay(&node, &mut state, &first)
            .await
            .expect_err("relay does not report success");

        let second = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(1),
            XMR,
            FeePriority::Priority,
        )
        .await
        .unwrap();
        assert_same_ring(
            "transactions built before and after a failed relay",
            &signed_ring(&first.tx_blob, a),
            &signed_ring(&second.tx_blob, a),
        );
    }
}

#[tokio::test]
async fn rebuild_with_other_recipient_amount_fee_or_shape_reuses_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let only = [a.key_image()];

    let first = hot_send(&node, &mut state, &only, &destination(0), XMR, NORMAL)
        .await
        .unwrap();
    relay(&node, &mut state, &first).await.unwrap();
    let ring = signed_ring(&first.tx_blob, a);

    let recipient = hot_send(&node, &mut state, &only, &destination(2), XMR, NORMAL)
        .await
        .unwrap();
    assert_same_ring(
        "other recipient",
        &ring,
        &signed_ring(&recipient.tx_blob, a),
    );

    let amount = hot_send(&node, &mut state, &only, &destination(0), 3 * XMR, NORMAL)
        .await
        .unwrap();
    assert_same_ring("other amount", &ring, &signed_ring(&amount.tx_blob, a));

    let fee = hot_send(
        &node,
        &mut state,
        &only,
        &destination(0),
        XMR,
        FeePriority::Priority,
    )
    .await
    .unwrap();
    assert!(fee.fee > first.fee);
    assert_same_ring("other fee", &ring, &signed_ring(&fee.tx_blob, a));

    let sweep = hot_sweep(&node, &mut state, &only, &destination(0))
        .await
        .unwrap();
    assert_same_ring("sweep", &ring, &signed_ring(&sweep.tx_blob, a));

    let unsigned = cold_build(&node, &mut state, &only, &destination(0), XMR)
        .await
        .unwrap();
    assert_same_ring(
        "unsigned envelope",
        &ring,
        &unsigned_ring(&unsigned.unsigned_tx_hex, a),
    );

    let learned = candidate_intersection(&node.get_outs_log(), a.global_index());
    assert!(ring.iter().all(|index| learned.contains(index)));
}

#[tokio::test]
async fn partially_recorded_inputs_keep_the_old_ring_and_draw_only_the_new_one() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 2);
    let (a, b) = (&owned[0], &owned[1]);
    let both = [a.key_image(), b.key_image()];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    first.persist_input_rings(&mut state).unwrap();

    let second = hot_send(&node, &mut state, &both, &destination(0), 7 * XMR, NORMAL)
        .await
        .unwrap();
    assert_same_ring(
        "recorded input beside a new one",
        &signed_ring(&first.tx_blob, a),
        &signed_ring(&second.tx_blob, a),
    );
    second.persist_input_rings(&mut state).unwrap();

    disk.save(&state);
    let mut state = crash_and_reload(&disk);
    let third = hot_send(&node, &mut state, &both, &destination(1), 6 * XMR, NORMAL)
        .await
        .unwrap();
    for owned in [a, b] {
        assert_same_ring(
            "rebuild once both rings are recorded",
            &signed_ring(&second.tx_blob, owned),
            &signed_ring(&third.tx_blob, owned),
        );
    }
}

#[tokio::test]
#[ignore = "known defect: a failed ring sanity check makes the builder draw and request new candidates, up to three sets per build, which the daemon can intersect to find the real outputs"]
async fn sanity_failure_sends_one_candidate_request_and_the_next_build_repeats_it() {
    let node = FakeNode::tail_heavy();
    let threshold = node.total_outputs() * 6 / 10;
    let (mut state, owned, disk) = funded(&node, 3);
    let (a, b, c) = (&owned[0], &owned[1], &owned[2]);
    let all = [a.key_image(), b.key_image(), c.key_image()];

    node.set_lock_script(vec![Lock::Below(threshold)]);
    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &first).await.unwrap();
    let ring_a = signed_ring(&first.tx_blob, a);
    node.never_lock(&ring_a);
    let calls_before = node.get_outs_log().len();

    node.set_lock_script(vec![Lock::AtOrAbove(threshold)]);
    let error = hot_send(&node, &mut state, &all, &destination(0), 11 * XMR, NORMAL)
        .await
        .expect_err("no sane set of rings exists in this reply");
    assert!(error.contains("sanity check"), "{error}");
    assert_eq!(
        node.get_outs_log().len() - calls_before,
        1,
        "a failed sanity check must not send further candidate requests"
    );

    let second = hot_send(&node, &mut state, &all, &destination(0), 11 * XMR, NORMAL)
        .await
        .unwrap();
    assert_same_ring(
        "recorded ring across a sanity failure",
        &ring_a,
        &signed_ring(&second.tx_blob, a),
    );
    let log = node.get_outs_log();
    for owned in [b, c] {
        let learned = candidate_intersection(&log[calls_before..], owned.global_index());
        assert_eq!(
            learned.len(),
            CANDIDATES_PER_INPUT,
            "the two requests for {} must name the same candidates",
            owned.global_index()
        );
    }

    disk.save(&state);
    let mut state = crash_and_reload(&disk);
    let third = hot_send(
        &node,
        &mut state,
        &all,
        &destination(1),
        9 * XMR,
        FeePriority::Elevated,
    )
    .await
    .unwrap();
    for owned in [a, b, c] {
        assert_same_ring(
            "rebuild after a sanity failure",
            &signed_ring(&second.tx_blob, owned),
            &signed_ring(&third.tx_blob, owned),
        );
    }
}

/// wallet2 unsets these rings and redraws (wallet2.cpp:9449-9454); this crate refuses.
#[tokio::test]
async fn recorded_rings_that_fail_sanity_together_are_never_redrawn() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 2);
    let (a, b) = (&owned[0], &owned[1]);
    let top = node.total_outputs() - 5_000;
    let shared: Vec<u64> = (0..8).map(|n| top - 10 * n).collect();
    let decoys = |base: u64| -> Vec<u64> {
        shared
            .iter()
            .copied()
            .chain((0..7).map(|n| base - 10 * n))
            .collect()
    };
    let ring_a = crafted_ring(a, &decoys(top - 1_000));
    let ring_b = crafted_ring(b, &decoys(top - 2_000));
    state
        .record_input_rings(&[ring_a.clone(), ring_b.clone()])
        .unwrap();

    let error = hot_send(
        &node,
        &mut state,
        &[a.key_image(), b.key_image()],
        &destination(0),
        7 * XMR,
        NORMAL,
    )
    .await
    .expect_err("eight shared members of 32 fail wallet2's 80% uniqueness rule");
    assert!(error.contains("sanity check"), "{error}");
    assert_eq!(state.outputs()[0].ring, Some(ring_a.ring));
    assert_eq!(state.outputs()[1].ring, Some(ring_b.ring));
}

#[tokio::test]
#[ignore = "known defect: build_unsigned_transaction always draws a fresh ring, whatever ring the output already has"]
async fn build_unsigned_transaction_reuses_a_recorded_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &first).await.unwrap();

    let unsigned = build_unsigned_from_raw_output(&node, &mut state, a, &destination(0), XMR)
        .await
        .unwrap();

    assert_same_ring(
        "relayed transaction and a later build_unsigned_transaction envelope",
        &signed_ring(&first.tx_blob, a),
        &unsigned_ring(&unsigned, a),
    );
}

#[tokio::test]
#[ignore = "known defect: the hot builders trust the ring field of the inputs they are handed, so inputs captured before a ring was recorded produce a second ring"]
async fn hot_rebuild_from_stale_inputs_reuses_the_recorded_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let stale: Vec<StoredOutputData> = inputs(&state, &[a.key_image()]);

    let first = hot_send_with(
        &node,
        &mut state,
        stale.clone(),
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &first).await.unwrap();

    let second = hot_send_with(&node, &mut state, stale, &destination(1), XMR, NORMAL)
        .await
        .unwrap();
    assert_same_ring(
        "relayed transaction and signed bytes rebuilt from stale inputs",
        &signed_ring(&first.tx_blob, a),
        &signed_ring(&second.tx_blob, a),
    );
}

#[tokio::test]
#[ignore = "known defect: the cold builder reads rings from the inputs it is handed, so with stale inputs it requests fresh candidates before it refuses to record the second ring"]
async fn cold_rebuild_from_stale_inputs_sends_no_fresh_candidates() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let stale: Vec<StoredOutputData> = inputs(&state, &[a.key_image()]);

    let first = cold_build_with(&node, &mut state, stale.clone(), &destination(0), XMR)
        .await
        .unwrap();
    let ring = unsigned_ring(&first.unsigned_tx_hex, a);

    if let Ok(second) = cold_build_with(&node, &mut state, stale, &destination(1), XMR).await {
        assert_same_ring(
            "envelopes built from the same stale inputs",
            &ring,
            &unsigned_ring(&second.unsigned_tx_hex, a),
        );
    }

    let learned = candidate_intersection(&node.get_outs_log(), a.global_index());
    assert!(
        ring.iter().all(|index| learned.contains(index)),
        "the rebuild let the daemon narrow the candidates to {learned:?}; the real output is {}",
        a.global_index()
    );
}

#[tokio::test]
#[ignore = "known defect: an input without a key image is refused only after its candidate request has been sent"]
async fn input_without_key_image_is_refused_before_any_candidate_request() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let mut without_key_image = inputs(&state, &[a.key_image()]);
    without_key_image[0].key_image.clear();

    let error = cold_build_with(
        &node,
        &mut state,
        without_key_image.clone(),
        &destination(0),
        XMR,
    )
    .await
    .expect_err("refused");
    assert!(error.contains("key image"), "{error}");
    let error = hot_send_with(
        &node,
        &mut state,
        without_key_image,
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .expect_err("refused");
    assert!(error.contains("key image"), "{error}");

    assert_eq!(
        node.get_outs_log().len(),
        0,
        "a build that can never be recorded must be refused before candidates are requested"
    );
}

async fn conflicting_spends(
    node: &FakeNode,
) -> (
    WalletState,
    Vec<Owned>,
    monero_rust::native::TransactionResult,
) {
    let (mut state, owned, _disk) = funded(node, 3);
    let (mut other_device, _other_disk) = second_instance(&owned);
    let (a, b, c) = (&owned[0], &owned[1], &owned[2]);

    let first = hot_send(
        node,
        &mut state,
        &[a.key_image(), b.key_image()],
        &destination(0),
        7 * XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let second = hot_send(
        node,
        &mut other_device,
        &[a.key_image(), c.key_image()],
        &destination(1),
        7 * XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_ne!(
        signed_ring(&first.tx_blob, a),
        signed_ring(&second.tx_blob, a)
    );

    relay(node, &mut state, &first).await.unwrap();
    (state, owned, second)
}

#[tokio::test]
async fn relaying_a_second_ring_for_a_relayed_output_is_refused() {
    let node = FakeNode::uniform();
    let (mut state, owned, second) = conflicting_spends(&node).await;

    let error = relay(&node, &mut state, &second)
        .await
        .expect_err("the second ring for the shared input must not be relayed");
    assert!(
        error.contains("Refusing to replace the persisted ring"),
        "{error}"
    );
    assert_eq!(node.seen_txs().len(), 1, "the daemon saw only the first");
    let c_state = state
        .outputs()
        .iter()
        .find(|output| output.key_image == owned[2].key_image())
        .unwrap();
    assert!(
        c_state.ring.is_none(),
        "the refused record is all or nothing"
    );
}

#[tokio::test]
#[ignore = "known defect: broadcast_raw_transaction relays any bytes without consulting wallet state, so it hands the daemon a second ring for an output this wallet already spent"]
async fn raw_relay_never_hands_the_daemon_a_second_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, second) = conflicting_spends(&node).await;

    let _ = relay_raw(&node, &mut state, &second.tx_blob).await;

    assert_daemon_saw_one_ring(&node, &owned[0]);
}

#[tokio::test]
async fn respend_after_pending_spend_expiry_or_pool_drop_reuses_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];
    let height = 100_000;

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    first.persist_input_rings(&mut state).unwrap();
    disk.save(&state);
    relay(&node, &mut state, &first).await.unwrap();
    let pending = PendingSpend {
        tx_id: first.tx_id.clone(),
        key_image: a.key_image().to_string(),
        output_key: a.output.output_key(),
        amount: a.output.amount,
        created_at_secs: 1_000,
    };
    state.add_pending_spends(&first.tx_id, vec![pending.clone()]);
    assert!(state.spendable_outputs_at(height, Some(2_000)).is_empty());

    let expired = state.cleanup_expired_pending_spends(1_000 + PENDING_SPEND_TTL_SECS);
    assert_eq!(expired, vec![first.tx_id.clone()]);
    assert_eq!(state.spendable_outputs_at(height, Some(2_000)).len(), 1);
    let respend = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        2 * XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "respend after pending-spend expiry",
        &signed_ring(&first.tx_blob, a),
        &signed_ring(&respend.tx_blob, a),
    );
    relay(&node, &mut state, &respend)
        .await
        .expect("the same ring is accepted for the respend");

    let mut reloaded = crash_and_reload(&disk);
    reloaded.restore_pending_spends(
        [(a.key_image().to_string(), pending)].into_iter().collect(),
        1_000 + PENDING_SPEND_TTL_SECS,
    );
    assert!(reloaded.pending_spends().is_empty());
    let respend = hot_send(
        &node,
        &mut reloaded,
        &[a.key_image()],
        &destination(2),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "respend after restart and pending-spend expiry",
        &signed_ring(&first.tx_blob, a),
        &signed_ring(&respend.tx_blob, a),
    );
}

#[tokio::test]
async fn respend_after_releasing_an_unmined_spend_reuses_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];
    let height = 100_000;

    let first = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &first).await.unwrap();
    assert_eq!(
        state.mark_spent_by_key_images(&[a.key_image().to_string()]),
        1
    );
    assert!(state.spendable_outputs_at(height, Some(2_000)).is_empty());
    disk.save(&state);

    let mut state = crash_and_reload(&disk);
    assert_eq!(state.release_unmined_spends(&[a.output.output_key()]), 1);
    assert_eq!(state.spendable_outputs_at(height, Some(2_000)).len(), 1);
    let respend = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        2 * XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &respend).await.unwrap();

    assert_eq!(node.seen_txs().len(), 2);
    assert_daemon_saw_one_ring(&node, a);
}

#[tokio::test]
#[ignore = "known defect: the hot builders record nothing, so a prepared sweep that is built twice gets two rings for every input"]
async fn prepared_sweep_built_twice_carries_one_ring_per_input() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 3);

    let transactions = prepare_sweep(&node, &state).await;
    assert_eq!(
        transactions.iter().map(Vec::len).sum::<usize>(),
        owned.len(),
        "every output is swept"
    );
    for inputs in transactions {
        let first = hot_sweep_with(&node, &mut state, inputs.clone(), &destination(0))
            .await
            .unwrap();
        let second = hot_sweep_with(&node, &mut state, inputs, &destination(1))
            .await
            .unwrap();
        let first_rings = rings_in_signed(&first.tx_blob);
        for (key_image, ring) in rings_in_signed(&second.tx_blob) {
            assert_same_ring(
                "a prepared sweep transaction built twice",
                &first_rings[&key_image],
                &ring,
            );
        }
    }
}

async fn relay_first_spend(
    node: &FakeNode,
    state: &mut WalletState,
    a: &Owned,
) -> (Vec<u64>, String) {
    let first = hot_send(node, state, &[a.key_image()], &destination(0), XMR, NORMAL)
        .await
        .unwrap();
    relay(node, state, &first).await.unwrap();
    (signed_ring(&first.tx_blob, a), first.tx_blob)
}

#[tokio::test]
async fn reorg_of_the_receiving_block_keeps_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;
    disk.save(&state);
    let mut state = crash_and_reload(&disk);

    let rollback = state.rollback_to_height(a.output.block_height);
    assert_eq!(rollback.removed_outputs.len(), 1);
    let mut rescanned = a.output.clone();
    rescanned.block_height += 1;
    state.add_outputs(vec![rescanned]);

    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut state, &second).await.unwrap();

    assert_eq!(node.seen_txs().len(), 2);
    assert_same_ring(
        "transactions relayed before and after a reorg of the receiving block",
        &first_ring,
        &signed_ring(&second.tx_blob, a),
    );
}

#[tokio::test]
async fn reorg_that_moves_the_global_index_never_draws_a_fresh_ring_silently() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;

    state.rollback_to_height(a.output.block_height);
    let moved = a.global_index() + 3;
    node.clear_output(a.global_index());
    node.set_output(moved, a.key, a.commitment);
    let mut rescanned = a.output.clone();
    rescanned.block_height += 1;
    rescanned.global_index = Some(moved);
    state.add_outputs(vec![rescanned]);

    let rebuilt = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await;
    let Ok(second) = rebuilt else {
        return; // Refusing is a safe outcome.
    };
    let second_ring = signed_ring(&second.tx_blob, a);
    let old_decoys: BTreeSet<u64> = first_ring
        .iter()
        .copied()
        .filter(|index| *index != a.global_index())
        .collect();
    let kept = old_decoys
        .iter()
        .filter(|index| second_ring.contains(index))
        .count();
    assert_eq!(
        kept,
        old_decoys.len(),
        "the rebuilt ring keeps {kept} of the 15 decoys published with the first spend"
    );
}

#[tokio::test]
async fn rescan_merged_into_existing_state_keeps_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;

    state.add_outputs(vec![a.output.clone()]);
    assert_eq!(state.outputs().len(), 1);
    assert_eq!(
        ring_indexes(state.outputs()[0].ring.as_ref().unwrap()),
        first_ring
    );
}

#[tokio::test]
async fn reorg_of_the_spending_transaction_keeps_the_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, first_bytes) = relay_first_spend(&node, &mut state, a).await;

    let mined_at = 99_990;
    assert!(!refresh_with(
        &mut state,
        &[scanned_block(mined_at, "old chain", &[&first_bytes])]
    ));
    assert!(state.outputs()[0].spent);
    disk.save(&state);
    let mut state = crash_and_reload(&disk);

    let rollback = state.rollback_to_height(mined_at);
    assert_eq!(rollback.unspent_key_images, vec![a.key_image().to_string()]);
    assert!(rollback.removed_outputs.is_empty());

    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        2 * XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "transactions built before and after a reorg of the spend",
        &first_ring,
        &signed_ring(&second.tx_blob, a),
    );
}

#[tokio::test]
async fn recorded_ring_with_a_vanished_or_changed_member_fails_closed() {
    for changed in [true, false] {
        let node = FakeNode::uniform();
        let (mut state, owned, _disk) = funded(&node, 1);
        let a = &owned[0];
        let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;
        let decoy = *first_ring
            .iter()
            .find(|index| **index != a.global_index())
            .unwrap();

        if changed {
            let (key, commitment) = default_member(decoy + 1);
            node.set_output(decoy, key, commitment);
        } else {
            node.always_lock(&[decoy]);
        }
        let error = hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(1),
            XMR,
            NORMAL,
        )
        .await
        .expect_err("a recorded ring that no longer matches the chain is not replaced");
        let expected = if changed {
            "different persisted ring member"
        } else {
            "persisted ring member is no longer available"
        };
        assert!(error.contains(expected), "{error}");
        assert_eq!(node.seen_txs().len(), 1);
        assert_eq!(
            ring_indexes(state.outputs()[0].ring.as_ref().unwrap()),
            first_ring
        );
    }
}

#[tokio::test]
async fn clearing_a_recorded_ring_is_the_only_way_to_draw_another() {
    let node = FakeNode::uniform();
    let (mut state, owned, disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;
    let decoy = *first_ring
        .iter()
        .find(|index| **index != a.global_index())
        .unwrap();
    let (key, commitment) = default_member(decoy + 1);
    node.set_output(decoy, key, commitment);
    for _ in 0..2 {
        hot_send(
            &node,
            &mut state,
            &[a.key_image()],
            &destination(1),
            XMR,
            NORMAL,
        )
        .await
        .expect_err("refused for as long as the ring is recorded");
    }

    let cleared = state
        .clear_ring(a.key_image())
        .expect("a ring was recorded");
    assert_eq!(ring_indexes(&cleared), first_ring);
    assert!(state.outputs()[0].ring.is_none());
    assert!(state.clear_ring(a.key_image()).is_none());

    let second = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let second_ring = signed_ring(&second.tx_blob, a);
    assert_ne!(second_ring, first_ring);
    relay(&node, &mut state, &second).await.unwrap();

    disk.save(&state);
    let mut state = crash_and_reload(&disk);
    let third = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(2),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "the ring drawn after clearing and a later rebuild",
        &second_ring,
        &signed_ring(&third.tx_blob, a),
    );
}

#[tokio::test]
async fn ring_of_a_spend_made_by_another_instance_is_learned_from_the_chain() {
    let node = FakeNode::uniform();
    let (mut instance_one, owned, _disk) = funded(&node, 1);
    let (mut instance_two, _other_disk) = second_instance(&owned);
    let a = &owned[0];

    let (first_ring, first_bytes) = relay_first_spend(&node, &mut instance_one, a).await;

    let mined_at = 99_990;
    assert!(!refresh_with(
        &mut instance_two,
        &[scanned_block(mined_at, "old chain", &[&first_bytes])]
    ));
    assert!(instance_two.outputs()[0].spent);
    assert!(refresh_with(
        &mut instance_two,
        &[scanned_block(mined_at, "new chain", &[])]
    ));
    assert!(!instance_two.outputs()[0].spent);

    let second = hot_send(
        &node,
        &mut instance_two,
        &[a.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    relay(&node, &mut instance_two, &second).await.unwrap();

    assert_eq!(node.seen_txs().len(), 2);
    assert_same_ring(
        "a spend seen on chain and this instance's respend after its reorg",
        &first_ring,
        &signed_ring(&second.tx_blob, a),
    );
}

fn block_spending(
    height: u64,
    hash: &str,
    key_image: &str,
    indexes: &[u64],
) -> monero_rust::BlockScanResult {
    let mut block = scanned_block(height, hash, &[]);
    let mut previous = 0;
    block.tx_count = 1;
    block.spent_key_images = vec![key_image.to_string()];
    block.spent_key_image_tx_hashes = vec!["ab".repeat(32)];
    block.spent_key_image_rings = vec![indexes
        .iter()
        .map(|index| {
            let offset = index - previous;
            previous = *index;
            offset
        })
        .collect()];
    block
}

#[tokio::test]
async fn a_ring_served_in_a_block_never_replaces_the_relayed_ring() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;

    let mut planted: Vec<u64> = (199_000..199_015).collect();
    planted.push(a.global_index());
    planted.sort_unstable();
    assert!(!refresh_with(
        &mut state,
        &[block_spending(99_990, "planted", a.key_image(), &planted)]
    ));
    assert!(refresh_with(
        &mut state,
        &[scanned_block(99_990, "honest", &[])]
    ));
    let kept = state.ring_database().ring(a.key_image()).unwrap();
    assert_eq!(ring_indexes(kept), first_ring);
    assert_eq!(
        state.ring_conflicts(),
        &[monero_rust::ring_db::ChainRingConflict {
            key_image: a.key_image().to_string(),
            recorded: first_ring.clone(),
            seen: planted,
        }]
    );

    let respend = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "the relayed ring and the respend after a planted block",
        &first_ring,
        &signed_ring(&respend.tx_blob, a),
    );
}

#[tokio::test]
async fn a_ring_without_the_real_output_is_never_learned() {
    let node = FakeNode::uniform();
    let (mut state, owned, _disk) = funded(&node, 1);
    let (mut fresh, _other_disk) = second_instance(&owned);
    let a = &owned[0];
    let (first_ring, _) = relay_first_spend(&node, &mut state, a).await;

    let poisoned: Vec<u64> = (1_000..1_016).collect();
    for wallet in [&mut state, &mut fresh] {
        assert!(!refresh_with(
            wallet,
            &[block_spending(99_990, "poisoned", a.key_image(), &poisoned)]
        ));
        assert!(refresh_with(
            wallet,
            &[scanned_block(99_990, "honest", &[])]
        ));
        assert!(wallet.ring_conflicts().is_empty());
    }
    assert_eq!(
        ring_indexes(state.ring_database().ring(a.key_image()).unwrap()),
        first_ring
    );
    assert!(fresh.ring_database().ring(a.key_image()).is_none());

    let respend = hot_send(
        &node,
        &mut state,
        &[a.key_image()],
        &destination(1),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    assert_same_ring(
        "the relayed ring and the respend after a poisoned block",
        &first_ring,
        &signed_ring(&respend.tx_blob, a),
    );
}

mod wallet2_txset {
    use super::*;
    use monero_rust::epee_compat;
    use monero_rust::native::{DecoySelection, RingMember};
    use monero_rust::wallet_keys_file::{read_keys_file, ImportedKeysFile};
    use monero_rust::WalletOutput;
    use std::path::{Path, PathBuf};

    fn vector(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/cold_signing_regtest_v0_18_5_0")
            .join(name)
    }

    pub struct Fixture {
        pub cold: ImportedKeysFile,
        pub unsigned: Vec<u8>,
        pub sources: Vec<(String, Vec<u64>, WalletOutput, InputRing)>,
    }

    pub fn fixture(node: &FakeNode) -> Fixture {
        let cold = read_keys_file(&vector("cold_full.keys"), "").unwrap();
        let unsigned = std::fs::read(vector("unsigned_monero_tx")).unwrap();
        let summary =
            epee_compat::parse_unsigned_monero_txset_summary(&unsigned, &cold.view_secret_key)
                .unwrap();
        let offsets = monero_rust::native::derive_wallet2_unsigned_source_key_offsets_from_keys(
            cold.spend_secret_key,
            cold.view_secret_key,
            &unsigned,
        )
        .unwrap();
        assert_eq!(summary.txes.len(), 1);

        let mut sources = Vec::new();
        for (n, (source, derived)) in summary.txes[0]
            .sources
            .iter()
            .zip(&offsets.source_key_offsets)
            .enumerate()
        {
            let real = &source.ring[source.real_output as usize];
            let ring: Vec<u64> = source.ring.iter().map(|m| m.global_output_index).collect();
            for member in &source.ring {
                node.set_output(
                    member.global_output_index,
                    member.output_public_key,
                    member.commitment,
                );
            }

            let tx_hash = [0xa0 + n as u8; 32];
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&tx_hash);
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&real.output_public_key);
            bytes.extend_from_slice(&hex::decode(&derived.key_offset).unwrap());
            bytes.extend_from_slice(&source.mask);
            bytes.extend_from_slice(&source.amount.to_le_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            monero_wallet::WalletOutput::read(&mut bytes.as_slice()).unwrap();

            let output = WalletOutput {
                tx_hash: hex::encode(tx_hash),
                output_index: 0,
                amount: source.amount,
                key: hex::encode(real.output_public_key),
                key_offset: derived.key_offset.clone(),
                commitment_mask: hex::encode(source.mask),
                subaddress_index: None,
                payment_id: None,
                received_output_bytes: hex::encode(bytes),
                global_index: Some(real.global_output_index),
                ring: None,
                block_height: 10 + n as u64,
                spent: false,
                spent_height: None,
                key_image: derived.key_image.clone(),
                is_coinbase: false,
                frozen: false,
            };
            let mut previous = 0;
            let persisted = InputRing {
                key_image: derived.key_image.clone(),
                ring: DecoySelection {
                    real_index: source.real_output as u8,
                    offsets: ring
                        .iter()
                        .map(|index| {
                            let offset = index - previous;
                            previous = *index;
                            offset
                        })
                        .collect(),
                    ring: source
                        .ring
                        .iter()
                        .map(|member| RingMember {
                            key: hex::encode(member.output_public_key),
                            commitment: hex::encode(member.commitment),
                        })
                        .collect(),
                },
            };
            sources.push((derived.key_image.clone(), ring, output, persisted));
        }
        Fixture {
            cold,
            unsigned,
            sources,
        }
    }

    pub fn sign(fixture: &Fixture) -> monero_rust::native::OfflineSignResult {
        let encoded = hex::encode(&fixture.unsigned);
        let preview = monero_rust::native::preview_unsigned_transaction(
            fixture.cold.spend_secret_key,
            fixture.cold.view_secret_key,
            &encoded,
            NETWORK,
        )
        .unwrap();
        let approval = preview.approve(u64::MAX, u64::MAX).unwrap();
        monero_rust::native::sign_unsigned_transaction_with_private_keys(
            fixture.cold.spend_secret_key,
            fixture.cold.view_secret_key,
            &encoded,
            NETWORK,
            &approval,
        )
        .unwrap()
    }

    pub async fn native_rebuild(
        node: &FakeNode,
        fixture: &Fixture,
        state: &mut WalletState,
    ) -> Result<monero_rust::native::TransactionResult, String> {
        let key_images: Vec<&str> = fixture.sources.iter().map(|s| s.0.as_str()).collect();
        let inputs = inputs(state, &key_images);
        hot_send_as(
            node,
            state,
            fixture.cold.mnemonic.as_ref().unwrap(),
            inputs,
            &destination(0),
            XMR,
            NORMAL,
        )
        .await
    }
}

#[tokio::test]
async fn wallet2_unsigned_txset_is_signed_with_the_rings_wallet2_chose() {
    let node = FakeNode::small();
    let fixture = wallet2_txset::fixture(&node);
    let signed = wallet2_txset::sign(&fixture);
    let rings = rings_in_signed(&signed.tx_blob);
    assert_eq!(rings.len(), fixture.sources.len());
    for (key_image, ring, _, _) in &fixture.sources {
        assert_same_ring(
            "wallet2 txset and the signed transaction",
            ring,
            &rings[key_image],
        );
    }
}

#[tokio::test]
async fn wallet2_txset_rings_are_recorded_for_a_later_native_rebuild() {
    let node = FakeNode::small();
    let fixture = wallet2_txset::fixture(&node);
    let (mut state, _disk) = wallet_holding(fixture.sources.iter().map(|s| s.2.clone()).collect());

    let converted = monero_rust::native::create_unsigned_transactions_from_wallet2_txset_keys(
        fixture.cold.spend_secret_key,
        fixture.cold.view_secret_key,
        &fixture.unsigned,
        NETWORK,
    )
    .unwrap();
    for result in &converted {
        result.persist_input_rings(&mut state).unwrap();
    }
    let signed = wallet2_txset::sign(&fixture);
    relay_raw(&node, &mut state, &signed.tx_blob).await.unwrap();

    let rebuilt = wallet2_txset::native_rebuild(&node, &fixture, &mut state)
        .await
        .unwrap();
    let rings = rings_in_signed(&rebuilt.tx_blob);
    for (key_image, ring, _, _) in &fixture.sources {
        assert_same_ring(
            "wallet2's ring, relayed, and a later native rebuild",
            ring,
            &rings[key_image],
        );
    }
}

#[tokio::test]
async fn wallet2_rings_recorded_by_hand_from_the_txset_summary_are_reused() {
    let node = FakeNode::small();
    let fixture = wallet2_txset::fixture(&node);
    let (mut state, _disk) = wallet_holding(fixture.sources.iter().map(|s| s.2.clone()).collect());
    let by_hand: Vec<InputRing> = fixture.sources.iter().map(|s| s.3.clone()).collect();
    state.record_input_rings(&by_hand).unwrap();

    let rebuilt = wallet2_txset::native_rebuild(&node, &fixture, &mut state)
        .await
        .unwrap();
    let rings = rings_in_signed(&rebuilt.tx_blob);
    for (key_image, ring, _, _) in &fixture.sources {
        assert_same_ring(
            "wallet2's ring and a native rebuild",
            ring,
            &rings[key_image],
        );
    }
}
