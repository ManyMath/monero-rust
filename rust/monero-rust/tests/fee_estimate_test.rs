//! Fee estimate validation test using a captured vector from a real stagenet node.
//!
//! Verifies structure and sanity of the `get_fee_estimate` JSON-RPC response.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// JSON structures matching the monerod JSON-RPC response
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcEnvelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct FeeEstimateResult {
    credits: u64,
    fee: u64,
    fees: Vec<u64>,
    quantization_mask: u64,
    status: String,
    top_hash: String,
    untrusted: bool,
}

/// Sanity-check upper bound for per-weight fee (in piconero).
/// A fee above this per byte would be unreasonably expensive and likely
/// indicates a manipulated node response.  100,000 pico/byte is generous
/// — typical mainnet fees are around 20,000-40,000 pico/byte.
const MAX_FEE_PER_BYTE: u64 = 100_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn fee_estimate_has_expected_structure() {
    let json = include_str!("vectors/get_fee_estimate.json");
    let envelope: RpcEnvelope<FeeEstimateResult> =
        serde_json::from_str(json).expect("should parse fee estimate JSON");

    let result = &envelope.result;

    assert_eq!(result.status, "OK");
    assert_eq!(result.credits, 0);
    assert!(!result.untrusted);
    assert_eq!(result.top_hash, "");
}

#[test]
fn fee_is_nonzero_and_within_sanity_range() {
    let json = include_str!("vectors/get_fee_estimate.json");
    let envelope: RpcEnvelope<FeeEstimateResult> =
        serde_json::from_str(json).expect("should parse fee estimate JSON");

    let fee = envelope.result.fee;

    assert!(fee > 0, "fee must be non-zero");
    assert!(
        fee <= MAX_FEE_PER_BYTE,
        "fee {} exceeds sanity limit of {} piconero per weight unit",
        fee,
        MAX_FEE_PER_BYTE
    );
}

#[test]
fn fee_tiers_are_present_and_increasing() {
    let json = include_str!("vectors/get_fee_estimate.json");
    let envelope: RpcEnvelope<FeeEstimateResult> =
        serde_json::from_str(json).expect("should parse fee estimate JSON");

    let fees = &envelope.result.fees;

    // Monero returns 4 fee priority tiers (default, x4, x20, x166).
    assert_eq!(fees.len(), 4, "expected 4 fee priority tiers");

    // The base fee (tier 0) should match the top-level `fee` field.
    assert_eq!(
        fees[0], envelope.result.fee,
        "first fee tier should equal the top-level fee field"
    );

    // Each tier should be strictly greater than the previous.
    for window in fees.windows(2) {
        assert!(
            window[1] > window[0],
            "fee tiers must be strictly increasing: {} <= {}",
            window[1],
            window[0]
        );
    }

    // All tiers should be within a reasonable range.
    for (i, &tier_fee) in fees.iter().enumerate() {
        assert!(tier_fee > 0, "fee tier {} must be non-zero", i);
    }
}

#[test]
fn quantization_mask_is_positive_power_of_ten() {
    let json = include_str!("vectors/get_fee_estimate.json");
    let envelope: RpcEnvelope<FeeEstimateResult> =
        serde_json::from_str(json).expect("should parse fee estimate JSON");

    let mask = envelope.result.quantization_mask;

    assert!(mask > 0, "quantization mask must be positive");

    // The mask is typically a power of 10 (e.g. 10000).
    // Verify it is indeed a power of 10.
    let mut val = mask;
    while val > 1 {
        assert_eq!(
            val % 10,
            0,
            "quantization mask {} should be a power of 10",
            mask
        );
        val /= 10;
    }
    assert_eq!(val, 1);
}

#[test]
fn fee_calculation_matches_expected_rounding() {
    let json = include_str!("vectors/get_fee_estimate.json");
    let envelope: RpcEnvelope<FeeEstimateResult> =
        serde_json::from_str(json).expect("should parse fee estimate JSON");

    let per_weight = envelope.result.fee;
    let mask = envelope.result.quantization_mask;

    // Simulate the fee calculation from monero-serai Fee::calculate
    // for a hypothetical 2000-byte transaction.
    let weight: u64 = 2000;
    let raw = per_weight * weight;
    let fee = ((raw - 1) / mask + 1) * mask;

    // Fee must be a multiple of the mask.
    assert_eq!(
        fee % mask,
        0,
        "calculated fee {} should be a multiple of mask {}",
        fee,
        mask
    );

    // Fee should be >= the raw amount (rounding up).
    assert!(
        fee >= raw,
        "quantized fee {} should be >= raw fee {}",
        fee,
        raw
    );

    // Fee should be < raw + mask (at most one mask above).
    assert!(
        fee < raw + mask,
        "quantized fee {} should be < raw + mask ({})",
        fee,
        raw + mask
    );
}
