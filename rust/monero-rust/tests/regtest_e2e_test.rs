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

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::monero_backend::rpc::HttpRpc;
use monero_rust::monero_backend::wallet::{
    address::{MoneroAddress, Network},
    seed::Seed,
    Change, SignableTransactionBuilder, SpendableOutput, ViewPair,
};
use monero_rust::scanner::{
    derive_address, derive_keys, scan_block_for_outputs_with_lookahead, scan_blocks_batch,
    Lookahead,
};
use monero_rust::wallet_compat::received_from_wallet_output;
use zeroize::Zeroizing;

/// Monero's own deterministic test seed, as used by the cold-signing vector generator.
const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";

/// regtest uses mainnet address encoding.
const NETWORK: &str = "mainnet";

/// Coinbase needs 60 confirmations, so mine past that to end up with spendable outputs.
const BLOCKS_TO_MINE: usize = 80;

/// Minimum chain height for the spending test.
///
/// Decoy selection samples ages from a gamma distribution and limits retries.
/// A short coinbase-only chain has too few unlocked candidates. Earlier runs
/// failed around 165 blocks and succeeded around 650; allow additional depth
/// to reduce sensitivity to random sampling.
const MIN_HEIGHT_TO_SPEND: usize = 750;

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

/// Build and sign a real transaction spending what we mined, then have the daemon accept it.
///
/// This is the half the fixture-driven tests can only replay: decoys come from the live
/// output distribution, and the daemon is the one judging whether the result is valid.
#[tokio::test(flavor = "multi_thread")]
async fn builds_signs_and_broadcasts_a_transaction() {
    let Some(node) = common::regtest_node() else {
        return;
    };

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, NETWORK, "").expect("derive address");

    // Top the chain up rather than mining a fixed amount: the other tests in this binary
    // share it, so its starting height depends on what already ran.
    let height = rpc.get_height().await.expect("get_height");
    if height < MIN_HEIGHT_TO_SPEND {
        rpc.generate_blocks(&address, MIN_HEIGHT_TO_SPEND - height)
            .await
            .expect("generateblocks");
    }
    let height = rpc.get_height().await.expect("get_height");
    assert!(height >= MIN_HEIGHT_TO_SPEND);

    // Take the first mature output we can find.
    let mature_before = height.saturating_sub(60);
    let mut spendable = None;
    for h in 1 .. mature_before {
        let scanned =
            scan_block_for_outputs_with_lookahead(&rpc, h as u64, SEED, NETWORK, LOOKAHEAD, "")
                .await
                .expect("scan");
        if let Some(output) = scanned.outputs.first() {
            let bytes = hex::decode(&output.received_output_bytes).expect("hex");
            let wallet_output =
                monero_wallet::WalletOutput::read(&mut bytes.as_slice()).expect("stored output");
            let received = received_from_wallet_output(&wallet_output).expect("compat output");
            spendable = Some(
                SpendableOutput::from(&rpc, received)
                    .await
                    .expect("resolve the global index from the daemon"),
            );
            break;
        }
    }
    let spendable = spendable.expect("no mature output to spend");
    let input_amount = spendable.commitment().amount;

    let keys = derive_keys(SEED, NETWORK, "").expect("derive keys");
    let seed = Seed::from_string(Zeroizing::new(SEED.to_string())).expect("seed");
    let spend_scalar = Scalar::from_bytes_mod_order(*seed.entropy());
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view_scalar = Scalar::from_bytes_mod_order(
        hex::decode(&keys.secret_view_key).expect("view hex")[.. 32]
            .try_into()
            .expect("32 bytes"),
    );
    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

    let protocol = rpc.get_protocol().await.expect("get_protocol");
    let fee_rate = rpc.get_fee().await.expect("get_fee");

    let mut builder = SignableTransactionBuilder::new(
        protocol,
        fee_rate,
        Some(Change::new(&view_pair, false)),
    );
    builder.add_input(spendable);
    let send_amount = 1_000_000_000u64;
    builder.add_payment(
        MoneroAddress::from_str(Network::Mainnet, &address).expect("address"),
        send_amount,
    );

    let signable = builder.build().expect("build a signable transaction");
    assert!(signable.fee() > 0, "a real transaction must carry a fee");
    assert!(
        signable.fee() + send_amount < input_amount,
        "the input must cover the payment and the fee"
    );

    let tx = signable
        .sign(&mut rand::rngs::OsRng, &rpc, &Zeroizing::new(spend_scalar))
        .await
        .expect("signing against live decoys must succeed");

    assert_eq!(tx.prefix().inputs.len(), 1);
    assert_eq!(tx.prefix().outputs.len(), 2, "payment plus change");

    // Require daemon acceptance, including ring-signature and range-proof checks.
    rpc.publish_transaction(&tx)
        .await
        .expect("the daemon must accept the transaction we built");

    let mined_into = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&address, 1).await.expect("mine it in");
    assert!(
        rpc.get_height().await.expect("get_height") > mined_into,
        "the chain must advance once our transaction is mined"
    );
    println!("daemon accepted transaction {}", hex::encode(tx.hash()));
}

/// Scan locally mined blocks through the `getblocks.bin` batch sync path.
#[tokio::test(flavor = "multi_thread")]
async fn batch_scans_the_chain_it_mined() {
    let Some(node) = common::regtest_node() else {
        return;
    };

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, NETWORK, "").expect("derive address");

    let start = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&address, BLOCKS_TO_MINE)
        .await
        .expect("generateblocks");

    let results = scan_blocks_batch(
        &rpc,
        start as u64,
        SEED,
        NETWORK,
        LOOKAHEAD,
        false,
        "",
    )
    .await
    .expect("batch scan must succeed");

    assert!(!results.is_empty(), "a batch scan must return blocks");

    let total_outputs: usize = results.iter().map(|r| r.outputs.len()).sum();
    assert!(
        total_outputs > 0,
        "every block in the range paid our address, so the batch must find outputs"
    );

    // Heights must come back ascending and start where we asked, since the sync loop
    // resumes from the last one it saw.
    let heights: Vec<u64> = results.iter().map(|r| r.block_height).collect();
    assert_eq!(heights[0], start as u64, "batch must start at the requested height");
    assert!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "batch heights must be strictly ascending, got {:?}",
        &heights[.. heights.len().min(8)]
    );

    // A batch scan and a per-block scan of the same range must agree.
    let mut per_block = 0usize;
    for height in &heights {
        per_block += scan_block_for_outputs_with_lookahead(
            &rpc, *height, SEED, NETWORK, LOOKAHEAD, "",
        )
        .await
        .expect("scan")
        .outputs
        .len();
    }
    assert_eq!(
        total_outputs, per_block,
        "batch scanning must find exactly what block-by-block scanning finds"
    );
}
