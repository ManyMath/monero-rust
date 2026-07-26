//! Resolve missing global output indices before spending.
//!
//! When `getblocks.bin` omits `output_indices`, the scanner stores zero as a
//! placeholder. The send path must discard it and fetch the actual global index
//! through `get_o_indexes`; using the placeholder would construct the wrong ring.

#![cfg(not(target_arch = "wasm32"))]

mod mock_rpc;

use monero_rust::monero_backend::rpc::Rpc;
use monero_rust::tx_builder::native::create_spendable_output;
use monero_rust::wallet_compat::received_from_wallet_output;
use mock_rpc::MockRpc;
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

fn mock_daemon() -> Rpc<MockRpc> {
    // indexes[1] is the one that matters - the stored output is index_in_transaction 1.
    let rpc = MockRpc::from_json("[]")
        .expect("mock")
        .with_binary_response("get_o_indexes.bin", get_o_indexes_response(vec![7, DAEMON_INDEX, 9]));
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

    let received = received_from_wallet_output(&wallet_output).expect("convert to compat output");
    let spendable = create_spendable_output(&mock_daemon(), received)
        .await
        .expect("resolving the global index must succeed");

    assert_eq!(
        spendable.global_index, DAEMON_INDEX,
        "the daemon's index must win over the stored placeholder"
    );
    assert_ne!(spendable.global_index, 0, "the placeholder must not survive");
}

/// The conversion out of stored bytes carries no global index at all, which is what makes
/// re-resolution unavoidable rather than just customary.
#[test]
fn the_compat_output_has_nowhere_to_carry_a_stale_index() {
    let stored = stored_output_with_zero_index();
    let wallet_output = monero_wallet::WalletOutput::read(&mut stored.as_slice()).unwrap();
    let received = received_from_wallet_output(&wallet_output).unwrap();

    // `ReceivedOutput` retains only the index within the transaction.
    assert_eq!(received.absolute.tx, [0x11; 32]);
    assert_eq!(received.absolute.o, 1);
}
