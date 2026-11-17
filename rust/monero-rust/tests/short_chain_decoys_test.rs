//! Decoy selection on a chain with at most 75 outputs older than the lock
//! window, where wallet2 asks for all of them (wallet2.cpp:9789-9798).

mod ring_reuse;

use monero_rust::monero_rpc::FeePriority;
use ring_reuse::*;

const NORMAL: FeePriority = FeePriority::Normal;

fn short_chain() -> FakeNode {
    FakeNode::with_blocks(vec![1; 80])
}

#[tokio::test]
async fn a_pre_ringct_output_never_reaches_a_candidate_request() {
    let node = short_chain();
    let mut owned = owned_output(1, 20, 10, XMR);
    node.set_output(owned.global_index(), owned.key, owned.commitment);
    owned.output.pre_ringct = true;
    let (mut state, _) = wallet_holding(vec![owned.output.clone()]);

    let raw = build_unsigned_from_raw_output(&node, &mut state, &owned, &destination(0), XMR / 2)
        .await
        .unwrap_err();
    assert!(raw.contains("no decoys are drawn"), "{raw}");
    let hot = hot_send(
        &node,
        &mut state,
        &[owned.key_image()],
        &destination(0),
        XMR / 2,
        NORMAL,
    )
    .await
    .unwrap_err();
    let cold = cold_build(
        &node,
        &mut state,
        &[owned.key_image()],
        &destination(0),
        XMR / 2,
    )
    .await
    .unwrap_err();
    for error in [hot, cold] {
        assert!(error.contains("pre-RingCT"), "{error}");
    }
    assert!(node.get_outs_log().is_empty());
    assert!(state
        .ring_database()
        .candidates(owned.key_image())
        .is_none());
    assert!(state.ring_database().ring(owned.key_image()).is_none());
}

#[tokio::test]
async fn a_ring_drawn_from_every_output_is_fixed_on_every_builder() {
    let node = short_chain();
    let owned = owned_output(1, 60, 10, 5 * XMR);
    node.set_output(owned.global_index(), owned.key, owned.commitment);
    let (mut state, _disk) = wallet_holding(vec![owned.output.clone()]);

    let hot = hot_send(
        &node,
        &mut state,
        &[owned.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let ring = signed_ring(&hot.tx_blob, &owned);
    let log = node.get_outs_log();
    assert_eq!(log.len(), 1);
    assert_eq!(&log[0][..70], (0..70).collect::<Vec<_>>().as_slice());
    assert!(state
        .ring_database()
        .candidates(owned.key_image())
        .is_none());
    assert_eq!(
        ring_indexes(state.ring_database().ring(owned.key_image()).unwrap()),
        ring
    );

    let cold = cold_build(
        &node,
        &mut state,
        &[owned.key_image()],
        &destination(1),
        XMR,
    )
    .await
    .unwrap();
    assert_same_ring(
        "a hot and a cold build on a short chain",
        &ring,
        &unsigned_ring(&cold.unsigned_tx_hex, &owned),
    );
}

#[tokio::test]
async fn a_request_for_every_output_is_not_kept_for_a_longer_chain() {
    let short = short_chain();
    let owned = owned_output(1, 60, 10, 5 * XMR);
    short.set_output(owned.global_index(), owned.key, owned.commitment);
    let (mut state, _disk) = wallet_holding(vec![owned.output.clone()]);
    short.set_lock_script(vec![Lock::AtOrAbove(0)]);
    let error = hot_send(
        &short,
        &mut state,
        &[owned.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap_err();
    assert!(error.contains("too few usable"), "{error}");
    assert_eq!(short.get_outs_log().len(), 1);
    assert!(state
        .ring_database()
        .candidates(owned.key_image())
        .is_none());

    let mut blocks = vec![1; 80];
    blocks.extend(std::iter::repeat_n(2, 2_000));
    let grown = FakeNode::with_blocks(blocks);
    grown.set_output(owned.global_index(), owned.key, owned.commitment);
    let built = hot_send(
        &grown,
        &mut state,
        &[owned.key_image()],
        &destination(0),
        XMR,
        NORMAL,
    )
    .await
    .unwrap();
    let ring = signed_ring(&built.tx_blob, &owned);
    let oldest: Vec<u64> = (0..15).chain([60]).collect();
    assert_ne!(ring, oldest, "the request for every output was reused");
    let request = &grown.get_outs_log()[0];
    let distinct: std::collections::BTreeSet<_> = request.iter().collect();
    assert_eq!(distinct.len(), CANDIDATES_PER_INPUT);
    assert!(request.iter().any(|index| *index >= 70));
}
