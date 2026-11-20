//! Scan and spend against a private regtest chain.
//!
//! Mine the required blocks locally so these tests need no synced stagenet node.
//!
//!     eval "$(scripts/regtest-node.sh start)"
//!     cargo test --test regtest_e2e_test
//!     scripts/regtest-node.sh stop
//!
//! Set `MONERO_REGTEST_RPC` to enable these tests; see `common::regtest_node`.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
use monero_rust::monero_rpc::{HttpRpc, Rpc};
use monero_rust::native::{
    build_unsigned_transaction, create_transaction_with_rpc, sweep_all_with_rpc, StoredOutputData,
};
use monero_rust::scanner::{
    derive_address, derive_keys, scan_block_for_outputs_with_lookahead, scan_blocks_batch,
    Lookahead,
};
use monero_rust::seed_compat::Seed;
use monero_rust::unsigned_tx::{sign_offline, UnsignedChange};
use zeroize::Zeroizing;

/// Monero's deterministic test seed, also used for cold-signing vectors.
const SEED: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";

const SWEEP_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel \
    orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills \
    cottage daytime daytime";

const MATRIX_SEED: &str = SWEEP_SEED;

/// Use mainnet address encoding on regtest.
const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;

/// Mine past coinbase's 60-confirmation maturity requirement.
const BLOCKS_TO_MINE: usize = 80;

const MIN_HEIGHT_TO_SPEND: usize = 750;

const LOOKAHEAD: Lookahead = Lookahead {
    account: 0,
    subaddress: 5,
};

// One daemon for all tests: serialize chain work.
static REGTEST_CHAIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool_hashes(rpc: &Rpc<HttpRpc>) -> Vec<String> {
    let pool: serde_json::Value = rpc
        .rpc_call::<Option<()>, serde_json::Value>("get_transaction_pool_hashes", None)
        .await
        .expect("get_transaction_pool_hashes");
    pool["tx_hashes"]
        .as_array()
        .map(|hashes| {
            hashes
                .iter()
                .map(|hash| hash.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Mine until `tx_hash` leaves the pool (Dandelion++ may delay it); returns
/// the block height.
async fn mine_until_mined(rpc: &Rpc<HttpRpc>, address: &str, tx_hash: [u8; 32]) -> usize {
    let tx_hash = hex::encode(tx_hash);
    assert!(
        pool_hashes(rpc).await.contains(&tx_hash),
        "the daemon accepted {tx_hash} into its pool"
    );
    for _ in 0..20 {
        let height = rpc.get_height().await.expect("get_height");
        rpc.generate_blocks(address, 1)
            .await
            .expect("generateblocks");
        if !pool_hashes(rpc).await.contains(&tx_hash) {
            return height;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("{tx_hash} is still in the pool after 20 blocks");
}

/// Mine to the test wallet, then scan for its coinbase outputs.
/// Keep mining and scanning together because tests share the chain.
#[tokio::test(flavor = "multi_thread")]
async fn mines_then_scans_its_own_outputs() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, monero_rust::Network::Mainnet, "").expect("derive address");
    assert!(
        address.to_string().starts_with('4'),
        "regtest uses mainnet encoding, got {}",
        address
    );

    let start_height = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&address.to_string(), BLOCKS_TO_MINE)
        .await
        .expect("generateblocks must succeed on a regtest daemon");

    let height = rpc.get_height().await.expect("get_height");
    assert!(
        height >= start_height + BLOCKS_TO_MINE,
        "chain must have grown by the blocks we mined ({} -> {})",
        start_height,
        height
    );

    // Every block just mined pays our address and should yield one output.
    let mut found = 0usize;
    let mut total = 0u64;
    for h in start_height..(start_height + BLOCKS_TO_MINE) {
        let scanned =
            scan_block_for_outputs_with_lookahead(&rpc, h as u64, SEED, NETWORK, LOOKAHEAD, "")
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
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, monero_rust::Network::Mainnet, "").expect("derive address");

    // Ensure some blocks exist regardless of which test runs first.
    rpc.generate_blocks(&address.to_string(), 5)
        .await
        .expect("generateblocks");
    let height = rpc.get_height().await.expect("get_height");

    let mut checked = 0usize;
    for h in (height.saturating_sub(5))..height {
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

    assert!(
        checked > 0,
        "expected to check at least one freshly scanned output"
    );
}

/// Build, sign, and broadcast a transaction spending a mined output.
/// Use live decoys and require the daemon to accept the result.
#[tokio::test(flavor = "multi_thread")]
async fn builds_signs_and_broadcasts_a_transaction() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, monero_rust::Network::Mainnet, "").expect("derive address");

    // Mine up to the required height, accounting for blocks from other tests.
    let height = rpc.get_height().await.expect("get_height");
    if height < MIN_HEIGHT_TO_SPEND {
        rpc.generate_blocks(&address.to_string(), MIN_HEIGHT_TO_SPEND - height)
            .await
            .expect("generateblocks");
    }
    let height = rpc.get_height().await.expect("get_height");
    assert!(height >= MIN_HEIGHT_TO_SPEND);

    // Select the first mature output.
    let mature_before = height.saturating_sub(60);
    let mut spendable = None;
    let mut wallet_state = monero_rust::WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    for h in 1..mature_before {
        let scanned =
            scan_block_for_outputs_with_lookahead(&rpc, h as u64, SEED, NETWORK, LOOKAHEAD, "")
                .await
                .expect("scan");
        if let Some(output) = scanned.outputs.first() {
            let bytes = hex::decode(&output.received_output_bytes).expect("hex");
            let wallet_output =
                monero_wallet::WalletOutput::read(&mut bytes.as_slice()).expect("stored output");
            let global_index = output
                .global_index
                .expect("refresh must persist the authoritative global index");
            spendable = Some((wallet_output, global_index));
            wallet_state.add_outputs(vec![output.clone()]);
            break;
        }
    }
    let spendable = spendable.expect("no mature output to spend");
    let input_amount = spendable.0.commitment().amount;

    let keys = derive_keys(SEED, monero_rust::Network::Mainnet, "").expect("derive keys");
    let seed = Seed::from_string(Zeroizing::new(SEED.to_string())).expect("seed");
    let spend_scalar = Scalar::from_bytes_mod_order(*seed.entropy());
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view_scalar = Scalar::from_bytes_mod_order(*keys.secret_view_key);
    let view_pair = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(spend_point),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(view_scalar)),
    )
    .expect("view pair");

    let rct_type = rpc.get_rct_type().await.expect("get_rct_type");
    let fee_rate = rpc
        .get_fee()
        .await
        .expect("get_fee")
        .rate()
        .expect("nonzero fee rate");

    let send_amount = 1_000_000_000u64;
    let mut rng = rand::rngs::OsRng;
    let unsigned = build_unsigned_transaction(
        &mut rng,
        &rpc,
        rct_type,
        fee_rate,
        &[spendable],
        vec![(address, send_amount)],
        Some(UnsignedChange::Own {
            address: view_pair.legacy_address(monero_rust::Network::Mainnet),
            subaddress: None,
        }),
        &mut wallet_state,
    )
    .await
    .expect("select live decoys and assemble the unsigned transaction");

    let resolved = unsigned
        .resolve(&view_pair)
        .expect("resolve fee and change");
    assert!(resolved.fee > 0, "a real transaction must carry a fee");
    assert!(
        resolved.fee + send_amount < input_amount,
        "the input must cover the payment and the fee"
    );

    let signed = sign_offline(
        &mut rng,
        &Zeroizing::new(spend_scalar),
        &Zeroizing::new(view_scalar),
        &unsigned,
    )
    .expect("signing against live decoys must succeed");
    assert_eq!(signed.fee, resolved.fee);
    let tx = signed.tx;

    assert_eq!(tx.prefix().inputs.len(), 1);
    assert_eq!(tx.prefix().outputs.len(), 2, "payment plus change");
    assert_eq!(tx.prefix().extra.len(), 44);
    assert_eq!(
        u128::from(resolved.fee),
        fee_rate.calculate_fee_from_weight(tx.weight() as u64),
        "the fee is the node's rate applied to the signed transaction's weight"
    );

    // Require daemon acceptance, including ring-signature and range-proof checks.
    rpc.publish_transaction(&tx)
        .await
        .expect("the daemon must accept the transaction we built");

    mine_until_mined(&rpc, &address.to_string(), tx.hash()).await;
    println!("daemon accepted transaction {}", hex::encode(tx.hash()));
}

#[tokio::test(flavor = "multi_thread")]
async fn sweep_with_dummy_output_is_accepted_by_the_daemon() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let decoy_address = derive_address(SEED, NETWORK, "").expect("derive decoy-mining address");
    let height = rpc.get_height().await.expect("get_height");
    if height < MIN_HEIGHT_TO_SPEND {
        rpc.generate_blocks(&decoy_address.to_string(), MIN_HEIGHT_TO_SPEND - height)
            .await
            .expect("mine enough decoys");
    }

    let sweep_address = derive_address(SWEEP_SEED, NETWORK, "").expect("derive sweep wallet");
    let start = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&sweep_address.to_string(), BLOCKS_TO_MINE)
        .await
        .expect("mine sweep wallet outputs");

    let mut owned = None;
    for height in start..(start + BLOCKS_TO_MINE - 60) {
        let scanned = scan_block_for_outputs_with_lookahead(
            &rpc,
            height as u64,
            SWEEP_SEED,
            NETWORK,
            LOOKAHEAD,
            "",
        )
        .await
        .expect("scan sweep output");
        if let Some(output) = scanned.outputs.into_iter().next() {
            owned = Some(output);
            break;
        }
    }
    let owned = owned.expect("a mature sweep output");
    let input_amount = owned.amount;
    let mut wallet_state = monero_rust::WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    wallet_state.add_outputs(vec![owned.clone()]);
    let stored = StoredOutputData::from(owned);

    let recipient_spend = Scalar::from(777u64);
    let recipient_view = Scalar::from(888u64);
    let recipient_view_pair = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(&recipient_spend * ED25519_BASEPOINT_TABLE),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(recipient_view)),
    )
    .expect("recipient view pair");
    let recipient_index = monero_wallet::address::SubaddressIndex::new(0, 1).unwrap();
    let recipient = recipient_view_pair.subaddress(NETWORK, recipient_index);

    let result = sweep_all_with_rpc(
        &rpc,
        SWEEP_SEED,
        "mainnet",
        vec![stored],
        &recipient.to_string(),
        "",
        &mut wallet_state,
    )
    .await
    .expect("build and sign sweep");
    let bytes = hex::decode(&result.tx_blob).expect("signed transaction hex");
    let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).expect("signed transaction");
    assert_eq!(tx.prefix().outputs.len(), 2);
    assert_eq!(tx.prefix().extra.len(), 44);
    assert!(result.tx_key_additional.is_empty());
    assert!(result.change_outputs.is_empty());
    assert!(result.fee < input_amount);

    let mut scanner = monero_wallet::Scanner::new(recipient_view_pair);
    scanner.register_subaddress(recipient_index);
    let received: Vec<_> = monero_rust::scanner::scan_single_transaction(
        &mut scanner,
        tx.hash(),
        &Transaction::<Pruned>::from(tx.clone()),
    )
    .expect("scan sweep recipient")
    .into_iter()
    .map(|output| output.commitment().amount)
    .collect();
    assert_eq!(received, vec![input_amount - result.fee]);

    rpc.publish_transaction(&tx)
        .await
        .expect("daemon must accept wallet2-style sweep");
    mine_until_mined(&rpc, &decoy_address.to_string(), tx.hash()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn daemon_accepts_multi_input_mixed_recipient_account_change() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let decoy_address = derive_address(SEED, NETWORK, "").expect("derive decoy-mining address");
    let height = rpc.get_height().await.expect("get_height");
    if height < MIN_HEIGHT_TO_SPEND {
        rpc.generate_blocks(&decoy_address.to_string(), MIN_HEIGHT_TO_SPEND - height)
            .await
            .expect("mine enough decoys");
    }

    let keys = derive_keys(MATRIX_SEED, NETWORK, "").expect("derive matrix wallet keys");
    let seed = Seed::from_string(Zeroizing::new(MATRIX_SEED.to_string())).expect("matrix seed");
    let spend = Scalar::from_bytes_mod_order(*seed.entropy());
    let view = Scalar::from_bytes_mod_order(*keys.secret_view_key);
    let sender = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(&spend * ED25519_BASEPOINT_TABLE),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(view)),
    )
    .expect("matrix sender view pair");
    let source_index = monero_wallet::address::SubaddressIndex::new(1, 0).unwrap();
    let source_address = sender.subaddress(NETWORK, source_index);
    let primary_address = sender.legacy_address(NETWORK);
    let start = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&primary_address.to_string(), BLOCKS_TO_MINE)
        .await
        .expect("mine primary-address outputs");

    let lookahead = Lookahead {
        account: 1,
        subaddress: 5,
    };
    let mut funding_input = None;
    let mut wallet_state = monero_rust::WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    for height in start..(start + BLOCKS_TO_MINE - 60) {
        let scanned = scan_block_for_outputs_with_lookahead(
            &rpc,
            height as u64,
            MATRIX_SEED,
            NETWORK,
            lookahead,
            "",
        )
        .await
        .expect("scan matrix output");
        if let Some(output) = scanned.outputs.into_iter().next() {
            wallet_state.add_outputs(vec![output.clone()]);
            funding_input = Some(StoredOutputData::from(output));
            break;
        }
    }
    let funding_input = funding_input.expect("a mature primary-address output");

    // `generateblocks` does not mine to subaddresses reliably, so fund by a send.
    let funding = create_transaction_with_rpc(
        &rpc,
        MATRIX_SEED,
        "mainnet",
        vec![funding_input],
        &[
            (source_address.to_string(), 1_000_000_000_000),
            (source_address.to_string(), 1_000_000_000_000),
        ],
        "",
        &mut wallet_state,
    )
    .await
    .expect("fund account-subaddress outputs");
    let funding_bytes = hex::decode(&funding.tx_blob).expect("funding transaction hex");
    let funding_tx =
        Transaction::<NotPruned>::read(&mut funding_bytes.as_slice()).expect("funding transaction");
    rpc.publish_transaction(&funding_tx)
        .await
        .expect("daemon must accept account funding");
    let funding_height =
        mine_until_mined(&rpc, &decoy_address.to_string(), funding_tx.hash()).await;
    rpc.generate_blocks(
        &decoy_address.to_string(),
        monero_oxide::DEFAULT_LOCK_WINDOW,
    )
    .await
    .expect("unlock account funding");
    let funded = scan_block_for_outputs_with_lookahead(
        &rpc,
        funding_height as u64,
        MATRIX_SEED,
        NETWORK,
        lookahead,
        "",
    )
    .await
    .expect("scan account funding");
    wallet_state.add_outputs(funded.outputs.clone());
    let stored = funded
        .outputs
        .into_iter()
        .filter(|output| output.subaddress_index == Some((1, 0)))
        .map(StoredOutputData::from)
        .collect::<Vec<_>>();
    assert_eq!(stored.len(), 2, "funding must create two account inputs");
    assert!(stored.iter().all(|output| !output.key_image.is_empty()));

    let recipient_spend = Scalar::from(1777u64);
    let recipient_view = Scalar::from(1888u64);
    let recipient_pair = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(&recipient_spend * ED25519_BASEPOINT_TABLE),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(recipient_view)),
    )
    .expect("recipient view pair");
    let standard = recipient_pair.legacy_address(NETWORK);
    let recipient_index = monero_wallet::address::SubaddressIndex::new(0, 1).unwrap();
    let subaddress = recipient_pair.subaddress(NETWORK, recipient_index);
    let recipients = vec![
        (standard.to_string(), 1_000_000_000),
        (subaddress.to_string(), 2_000_000_000),
    ];

    let result = create_transaction_with_rpc(
        &rpc,
        MATRIX_SEED,
        "mainnet",
        stored,
        &recipients,
        "",
        &mut wallet_state,
    )
    .await
    .expect("build mixed-recipient transaction");
    let bytes = hex::decode(&result.tx_blob).expect("signed transaction hex");
    let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).expect("signed transaction");
    assert_eq!(tx.prefix().inputs.len(), 2);
    assert_eq!(tx.prefix().outputs.len(), 3);
    assert_eq!(result.tx_key_additional.len(), 3);
    assert!(result
        .change_outputs
        .iter()
        .any(|output| output.subaddress_index == Some((1, 0))));

    let mut recipient_scanner = monero_wallet::Scanner::new(recipient_pair);
    recipient_scanner.register_subaddress(recipient_index);
    let mut received = monero_rust::scanner::scan_single_transaction(
        &mut recipient_scanner,
        tx.hash(),
        &Transaction::<Pruned>::from(tx.clone()),
    )
    .expect("scan both recipients")
    .into_iter()
    .map(|output| output.commitment().amount)
    .collect::<Vec<_>>();
    received.sort_unstable();
    assert_eq!(received, vec![1_000_000_000, 2_000_000_000]);

    rpc.publish_transaction(&tx)
        .await
        .expect("daemon must accept mixed-recipient transaction");
    mine_until_mined(&rpc, &decoy_address.to_string(), tx.hash()).await;
}

/// Scan locally mined blocks through the `getblocks.bin` batch sync path.
#[tokio::test(flavor = "multi_thread")]
async fn batch_scans_the_chain_it_mined() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = REGTEST_CHAIN.lock().await;

    let rpc = HttpRpc::new(node).expect("regtest RPC");
    let address = derive_address(SEED, monero_rust::Network::Mainnet, "").expect("derive address");

    let start = rpc.get_height().await.expect("get_height");
    rpc.generate_blocks(&address.to_string(), BLOCKS_TO_MINE)
        .await
        .expect("generateblocks");

    let results = scan_blocks_batch(&rpc, start as u64, SEED, NETWORK, LOOKAHEAD, false, "")
        .await
        .expect("batch scan must succeed");

    assert!(!results.is_empty(), "a batch scan must return blocks");

    let total_outputs: usize = results.iter().map(|r| r.outputs.len()).sum();
    assert!(
        total_outputs > 0,
        "every block in the range paid our address, so the batch must find outputs"
    );

    // Check the requested start and ascending heights before the sync loop
    // would resume from the last returned block.
    let heights: Vec<u64> = results.iter().map(|r| r.block_height).collect();
    assert_eq!(
        heights[0], start as u64,
        "batch must start at the requested height"
    );
    assert!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "batch heights must be strictly ascending, got {:?}",
        &heights[..heights.len().min(8)]
    );

    // Compare batch and per-block scanning over the same range.
    let mut per_block = 0usize;
    for height in &heights {
        per_block +=
            scan_block_for_outputs_with_lookahead(&rpc, *height, SEED, NETWORK, LOOKAHEAD, "")
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
