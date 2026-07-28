//! Scan and spend against a private regtest chain.
//!
//! Mine the required blocks locally so these tests need no synced stagenet node.
//!
//!     eval "$(scripts/regtest-node.sh start)"
//!     cargo test -p monero-rust --test regtest_e2e_test
//!     scripts/regtest-node.sh stop
//!
//! Set `MONERO_REGTEST_RPC` to enable these tests; see `common::regtest_node`.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use monero_rust::monero_backend::rpc::HttpRpc;
use monero_rust::scanner::{derive_address, scan_block_for_outputs_with_lookahead, Lookahead};

/// Monero's own deterministic test seed, as used by the cold-signing vector generator.
const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";

/// regtest uses mainnet address encoding.
const NETWORK: &str = "mainnet";

/// Coinbase needs 60 confirmations, so mine past that to end up with spendable outputs.
const BLOCKS_TO_MINE: usize = 80;

const LOOKAHEAD: Lookahead = Lookahead {
    account: 0,
    subaddress: 5,
};

/// Mine to the test wallet, then scan for its coinbase outputs.
/// Keep mining and scanning together because tests share the chain.
#[tokio::test(flavor = "multi_thread")]
async fn mines_then_scans_its_own_outputs() {
    let Some(node) = common::regtest_node() else {
        return;
    };

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, NETWORK, "").expect("derive address");
    assert!(address.starts_with('4'), "regtest uses mainnet encoding, got {}", address);

    let start_height = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&address, BLOCKS_TO_MINE)
        .await
        .expect("generateblocks must succeed on a regtest daemon");

    let height = rpc.get_height().await.expect("get_height");
    assert!(
        height >= start_height + BLOCKS_TO_MINE,
        "chain must have grown by the blocks we mined ({} -> {})",
        start_height,
        height
    );

    // Scan what we just mined. Every block's coinbase paid our address, so every block in
    // the range should yield an output.
    let mut found = 0usize;
    let mut total = 0u64;
    for h in start_height .. (start_height + BLOCKS_TO_MINE) {
        let scanned = scan_block_for_outputs_with_lookahead(
            &rpc,
            h as u64,
            SEED,
            NETWORK,
            LOOKAHEAD,
            "",
        )
        .await
        .unwrap_or_else(|e| panic!("scan of block {} failed: {}", h, e));

        for output in &scanned.outputs {
            assert!(output.amount > 0, "a mined output must carry an amount");
            assert!(
                !output.received_output_bytes.is_empty(),
                "the scanner must emit the stored form of every output it finds"
            );
            total += output.amount;
        }
        found += scanned.outputs.len();
    }

    assert!(
        found > 0,
        "scanning {} blocks we mined to our own address found nothing",
        BLOCKS_TO_MINE
    );
    assert!(total > 0, "mined outputs must sum to something");
    println!("scanned {} outputs worth {} atomic units", found, total);
}

/// Read a scanned output back from storage and convert it for spending,
/// following the wallet reload path.
#[tokio::test(flavor = "multi_thread")]
async fn scanned_outputs_parse_back_in_the_stored_format() {
    let Some(node) = common::regtest_node() else {
        return;
    };

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, NETWORK, "").expect("derive address");

    // Mine a little in case this runs before the other test; the chain is shared.
    rpc.generate_blocks(&address, 5).await.expect("generateblocks");
    let height = rpc.get_height().await.expect("get_height");

    let mut checked = 0usize;
    for h in (height.saturating_sub(5)) .. height {
        let scanned =
            scan_block_for_outputs_with_lookahead(&rpc, h as u64, SEED, NETWORK, LOOKAHEAD, "")
                .await
                .expect("scan");

        for output in &scanned.outputs {
            let bytes = hex::decode(&output.received_output_bytes).expect("stored bytes are hex");
            let parsed = monero_wallet::WalletOutput::read(&mut bytes.as_slice())
                .expect("stored bytes must parse as the monero-wallet output they claim to be");

            assert_eq!(
                parsed.commitment().amount,
                output.amount,
                "the parsed amount must match what the scanner reported"
            );
            checked += 1;
        }
    }

    assert!(checked > 0, "expected to check at least one freshly scanned output");
}
