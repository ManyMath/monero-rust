//! Resolve missing global output indices before spending.
//!
//! When `getblocks.bin` omits `output_indices`, the scanner stores zero as a
//! placeholder. The send path must discard it and fetch the actual global index
//! through `get_o_indexes`; using the placeholder would construct the wrong ring.

#![cfg(not(target_arch = "wasm32"))]

mod mock_rpc;

use mock_rpc::MockRpc;
use monero_rust::monero_rpc::Rpc;
use monero_rust::tx_builder::native::resolve_global_index;
use serde::Serialize;

/// Global index returned by the daemon for transaction output 1.
const DAEMON_INDEX: u64 = 424242;

/// Serialize a `WalletOutput` with a placeholder global index of zero.
fn stored_output_with_zero_index() -> Vec<u8> {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, Scalar};

    let key = &Scalar::from(9u64) * ED25519_BASEPOINT_TABLE;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x11; 32]); // AbsoluteId: transaction hash
    bytes.extend_from_slice(&1u64.to_le_bytes()); // index_in_transaction
    bytes.extend_from_slice(&0u64.to_le_bytes()); // index_on_blockchain: the placeholder
    bytes.extend_from_slice(key.compress().as_bytes()); // OutputData: key
    bytes.extend_from_slice(&Scalar::from(5u64).to_bytes()); // key offset
    bytes.extend_from_slice(&Scalar::from(3u64).to_bytes()); // commitment mask
    bytes.extend_from_slice(&1234u64.to_le_bytes()); // amount
    bytes.push(0); // Metadata: Timelock::None
    bytes.push(0); // no subaddress
    bytes.push(0); // no payment id
    bytes.push(0); // no arbitrary data
    bytes
}

/// Encode a `get_o_indexes.bin` response using the RPC client's Epee crate.
fn get_o_indexes_response(indexes: Vec<u64>) -> Vec<u8> {
    #[derive(Serialize)]
    struct OIndexes {
        o_indexes: Vec<u64>,
        status: String,
        untrusted: bool,
        credits: usize,
        top_hash: String,
    }

    monero_epee_bin_serde::to_bytes(&OIndexes {
        o_indexes: indexes,
        status: "OK".to_string(),
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
    })
    .expect("epee encode")
}

fn mock_daemon(indexes: Vec<u64>) -> Rpc<MockRpc> {
    let rpc = MockRpc::from_json("[]")
        .expect("mock")
        .with_binary_response("get_o_indexes.bin", get_o_indexes_response(indexes));
    Rpc::new_with_connection(rpc)
}

/// Replace the stored placeholder with the daemon's global index before spending.
#[tokio::test]
async fn spending_re_resolves_the_stored_index_from_the_daemon() {
    let stored = stored_output_with_zero_index();
    let wallet_output =
        monero_wallet::WalletOutput::read(&mut stored.as_slice()).expect("stored output parses");
    assert_eq!(
        wallet_output.index_on_blockchain(),
        0,
        "fixture must carry the placeholder, or this proves nothing"
    );

    let global_index = resolve_global_index(&mock_daemon(vec![7, DAEMON_INDEX, 9]), &wallet_output)
        .await
        .expect("resolving the global index must succeed");

    assert_eq!(
        global_index, DAEMON_INDEX,
        "the daemon's index must win over the stored placeholder"
    );
    assert_ne!(global_index, 0, "the placeholder must not survive");
}

#[tokio::test]
async fn a_short_daemon_answer_does_not_fall_back_to_the_stored_index() {
    let stored = stored_output_with_zero_index();
    let wallet_output = monero_wallet::WalletOutput::read(&mut stored.as_slice()).unwrap();

    let error = resolve_global_index(&mock_daemon(vec![7]), &wallet_output)
        .await
        .expect_err("one index cannot answer for the output at position 1");
    assert!(error.contains("too few output indexes"), "{error}");
}
