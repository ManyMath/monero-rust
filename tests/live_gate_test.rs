//! Test the network check used to enable live stagenet tests.
//!
//! Parsing failures cause those tests to skip. Check against recorded daemon
//! responses so a broken parser cannot silently disable them.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::json_string_field;

/// A truncated HTTP response from monerod's `/get_info` endpoint.
const RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\
    \"alt_blocks_count\": 0,\"block_size_limit\": 600000,\"height\": 1384600,\
    \"mainnet\": false,\"nettype\": \"stagenet\",\"offline\": false,\
    \"status\": \"OK\",\"synchronized\": true}";

#[test]
fn reads_nettype_from_a_real_response() {
    assert_eq!(
        json_string_field(RESPONSE, "nettype").as_deref(),
        Some("stagenet")
    );
    assert_eq!(json_string_field(RESPONSE, "status").as_deref(), Some("OK"));
}

/// Reject mainnet, testnet, and fakechain responses at the stagenet test port.
#[test]
fn distinguishes_the_chains_the_gate_must_reject() {
    for nettype in ["fakechain", "mainnet", "testnet"] {
        let body = RESPONSE.replace("\"stagenet\"", &format!("\"{}\"", nettype));
        assert_eq!(
            json_string_field(&body, "nettype").as_deref(),
            Some(nettype)
        );
    }
}

#[test]
fn tolerates_spacing_and_missing_fields() {
    assert_eq!(
        json_string_field("{\"nettype\"   :   \"stagenet\"}", "nettype").as_deref(),
        Some("stagenet")
    );
    assert_eq!(json_string_field("{\"height\": 803}", "nettype"), None);
    assert_eq!(json_string_field("", "nettype"), None);
}
