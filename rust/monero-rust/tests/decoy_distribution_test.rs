//! Tests for decoy distribution and output validation using captured vectors.
//!
//! These tests verify invariants of the output distribution and `get_outs`
//! responses captured from a real stagenet node. They run purely offline
//! with no network access required.

use curve25519_dalek::edwards::CompressedEdwardsY;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// JSON structures matching the monerod JSON-RPC responses
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct DistributionResult {
    distributions: Vec<Distribution>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct Distribution {
    amount: u64,
    base: u64,
    distribution: Vec<u64>,
    start_height: u64,
}

#[derive(Debug, Deserialize)]
struct GetOutsResult {
    outs: Vec<OutEntry>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct OutEntry {
    height: u64,
    key: String,
    mask: String,
    txid: String,
    unlocked: bool,
}

// ---------------------------------------------------------------------------
// Distribution invariants
// ---------------------------------------------------------------------------

#[test]
fn distribution_is_monotonically_increasing() {
    let json = include_str!("vectors/get_output_distribution_recent_1000.json");
    let envelope: RpcEnvelope<DistributionResult> =
        serde_json::from_str(json).expect("should parse distribution JSON");

    assert_eq!(envelope.result.status, "OK");
    assert!(!envelope.result.distributions.is_empty());

    let dist = &envelope.result.distributions[0];
    assert!(
        !dist.distribution.is_empty(),
        "distribution array should not be empty"
    );

    // Cumulative distribution must be monotonically non-decreasing.
    for window in dist.distribution.windows(2) {
        assert!(
            window[1] >= window[0],
            "distribution must be monotonically non-decreasing: {} < {} (violation)",
            window[1],
            window[0]
        );
    }
}

#[test]
fn distribution_start_height_and_base_are_consistent() {
    let json = include_str!("vectors/get_output_distribution_recent_1000.json");
    let envelope: RpcEnvelope<DistributionResult> =
        serde_json::from_str(json).expect("should parse distribution JSON");

    let dist = &envelope.result.distributions[0];

    // The `base` field is the cumulative count at `start_height - 1`.
    // The first element of `distribution` should be >= base (cumulative at start_height).
    assert!(
        dist.distribution[0] >= dist.base,
        "first distribution entry ({}) should be >= base ({})",
        dist.distribution[0],
        dist.base
    );

    // The last element should be the largest (cumulative at the end of the range).
    let last = *dist.distribution.last().unwrap();
    let first = dist.distribution[0];
    assert!(
        last >= first,
        "last distribution entry ({}) should be >= first ({})",
        last,
        first
    );

    // start_height should be a reasonable block height (non-zero for recent-1000).
    assert!(
        dist.start_height > 0,
        "start_height should be non-zero for a recent block range"
    );

    // amount == 0 means the RingCT output distribution (post-RingCT, all amounts are 0).
    assert_eq!(dist.amount, 0, "amount should be 0 for RingCT distribution");
}

#[test]
fn distribution_length_matches_block_range() {
    let json = include_str!("vectors/get_output_distribution_recent_1000.json");
    let envelope: RpcEnvelope<DistributionResult> =
        serde_json::from_str(json).expect("should parse distribution JSON");

    let dist = &envelope.result.distributions[0];

    // The recent-1000 request should yield approximately 1000 entries.
    // Allow some flexibility since the exact count depends on the daemon's
    // interpretation of the range.
    assert!(
        dist.distribution.len() >= 900 && dist.distribution.len() <= 1100,
        "expected ~1000 distribution entries, got {}",
        dist.distribution.len()
    );
}

// ---------------------------------------------------------------------------
// Output key validation (get_outs)
// ---------------------------------------------------------------------------

#[test]
fn output_keys_are_valid_curve_points() {
    let json = include_str!("vectors/get_outs_sample_0_15.json");
    let result: GetOutsResult =
        serde_json::from_str(json).expect("should parse get_outs JSON");

    assert_eq!(result.status, "OK");
    assert_eq!(result.outs.len(), 16, "sample should have 16 output entries");

    for (i, out) in result.outs.iter().enumerate() {
        // Each key should be 64 hex chars = 32 bytes
        assert_eq!(
            out.key.len(),
            64,
            "output key {} should be 64 hex chars, got {}",
            i,
            out.key.len()
        );

        let key_bytes = hex::decode(&out.key)
            .unwrap_or_else(|e| panic!("output key {} should be valid hex: {}", i, e));
        assert_eq!(key_bytes.len(), 32);

        // The key must be a valid compressed Edwards Y point.
        let compressed = CompressedEdwardsY::from_slice(&key_bytes);
        let point = compressed.decompress();
        assert!(
            point.is_some(),
            "output key {} ({}) should decompress to a valid curve point",
            i,
            out.key
        );
    }
}

#[test]
fn output_masks_are_valid_32_byte_keys() {
    let json = include_str!("vectors/get_outs_sample_0_15.json");
    let result: GetOutsResult =
        serde_json::from_str(json).expect("should parse get_outs JSON");

    for (i, out) in result.outs.iter().enumerate() {
        assert_eq!(
            out.mask.len(),
            64,
            "output mask {} should be 64 hex chars",
            i
        );
        let mask_bytes = hex::decode(&out.mask)
            .unwrap_or_else(|e| panic!("output mask {} should be valid hex: {}", i, e));
        assert_eq!(mask_bytes.len(), 32);
    }
}

#[test]
fn output_txids_are_valid_32_byte_hashes() {
    let json = include_str!("vectors/get_outs_sample_0_15.json");
    let result: GetOutsResult =
        serde_json::from_str(json).expect("should parse get_outs JSON");

    for (i, out) in result.outs.iter().enumerate() {
        assert_eq!(
            out.txid.len(),
            64,
            "output txid {} should be 64 hex chars",
            i
        );
        let txid_bytes = hex::decode(&out.txid)
            .unwrap_or_else(|e| panic!("output txid {} should be valid hex: {}", i, e));
        assert_eq!(txid_bytes.len(), 32);
    }
}

#[test]
fn output_heights_are_sequential_in_sample() {
    let json = include_str!("vectors/get_outs_sample_0_15.json");
    let result: GetOutsResult =
        serde_json::from_str(json).expect("should parse get_outs JSON");

    // The sample was captured for global output indexes 0..15, which correspond
    // to consecutive blocks in the early chain. Heights should be increasing.
    for window in result.outs.windows(2) {
        assert!(
            window[1].height >= window[0].height,
            "output heights should be non-decreasing: {} < {}",
            window[1].height,
            window[0].height
        );
    }

    // All outputs in this early range should be unlocked.
    for out in &result.outs {
        assert!(out.unlocked, "early outputs should be unlocked");
    }
}
