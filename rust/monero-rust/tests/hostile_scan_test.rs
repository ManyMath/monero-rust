//! Scan path against an adversarial node.

#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use std::collections::BTreeSet;

use hostile::epee::{self, Obj, Val};
use hostile::{edit_bin, Chain, HostileNode, SnapshotNode, Wallet, LOOKAHEAD, NETWORK, SEED_W};
use monero_rust::scanner::{scan_blocks_batch, BlockScanResult};

type Node = HostileNode<SnapshotNode>;

async fn scan_block(node: &Node, height: u64) -> Result<BlockScanResult, String> {
    scan_blocks_batch(&node.rpc(), height, SEED_W, NETWORK, LOOKAHEAD, false, "")
        .await?
        .into_iter()
        .find(|result| result.block_height == height)
        .ok_or_else(|| format!("node did not return block {height}"))
}

fn first_block_node(
    name: &'static str,
    edit: impl Fn(&mut Obj, &mut Obj) + Send + Sync + 'static,
) -> Node {
    Node::with(SnapshotNode::new(), name, move |call, reply| {
        if !call.is("getblocks.bin") {
            return reply;
        }
        edit_bin(reply, |obj| {
            let mut block = epee::get(obj, "blocks").unwrap().arr()[0].obj().clone();
            let mut indices = epee::get(obj, "output_indices").unwrap().arr()[0]
                .obj()
                .clone();
            edit(&mut block, &mut indices);
            epee::get_mut(obj, "blocks").unwrap().arr_mut()[0] = Val::Obj(block);
            epee::get_mut(obj, "output_indices").unwrap().arr_mut()[0] = Val::Obj(indices);
        })
    })
}

fn tx_indices(indices: &mut Obj, tx: usize) -> &mut Vec<Val> {
    epee::get_mut(
        epee::get_mut(indices, "indices").unwrap().arr_mut()[tx].obj_mut(),
        "indices",
    )
    .unwrap()
    .arr_mut()
}

async fn honest_block(height: u64) -> BlockScanResult {
    scan_block(&Node::honest(SnapshotNode::new()), height)
        .await
        .expect("honest scan")
}

fn fingerprint(wallet: &Wallet) -> BTreeSet<(String, u64, bool, Option<u64>)> {
    wallet
        .outputs()
        .iter()
        .map(|o| (o.output_key(), o.block_height, o.spent, o.global_index))
        .collect()
}

async fn fresh_sync(chain: Chain) -> Wallet {
    let honest = SnapshotNode::new();
    honest.switch_chain(chain);
    let mut wallet = Wallet::new(SEED_W);
    wallet
        .refresh(&Node::honest(honest).rpc(), 64)
        .await
        .expect("honest refresh");
    wallet
}

#[tokio::test]
async fn pruned_flag_set_by_the_node_never_hides_received_outputs() {
    let honest = honest_block(701).await;
    assert_eq!(honest.outputs.len(), 1);
    let node = first_block_node("block-marked-pruned", |block, _| {
        epee::set(block, "pruned", Val::Bool(true));
    });
    match scan_block(&node, 701).await {
        Err(_) => {}
        Ok(scanned) => assert_eq!(
            scanned.outputs.len(),
            honest.outputs.len(),
            "block 701 was accepted as scanned with the wallet's output missing"
        ),
    }
}

#[tokio::test]
async fn block_with_a_low_version_byte_is_never_skipped_unvalidated() {
    let honest = honest_block(701).await;
    for (name, blob) in [
        ("block-version-byte-1", None),
        ("block-blob-empty", Some(vec![])),
        ("block-blob-garbage", Some(vec![3u8; 40])),
    ] {
        let node = first_block_node(name, move |block, _| {
            let replacement = blob.clone().unwrap_or_else(|| {
                let mut real = epee::get(block, "block").unwrap().bytes().to_vec();
                real[0] = 1;
                real
            });
            epee::set(block, "block", Val::Str(replacement));
        });
        match scan_block(&node, 701).await {
            Err(_) => {}
            Ok(scanned) => {
                assert!(
                    scanned.outputs.len() == honest.outputs.len()
                        && scanned.spent_key_images == honest.spent_key_images,
                    "{name}: block 701 was accepted as scanned with {} outputs and {} key images \
                     (honest: {} and {})",
                    scanned.outputs.len(),
                    scanned.spent_key_images.len(),
                    honest.outputs.len(),
                    honest.spent_key_images.len()
                );
            }
        }
    }
}

#[tokio::test]
async fn unparseable_transaction_blob_is_never_dropped_silently() {
    let honest = honest_block(701).await;
    assert_eq!(honest.outputs.len(), 1);
    let node = first_block_node("tx-blob-unparseable", |block, _| {
        let txs = epee::get_mut(block, "txs").unwrap().arr_mut();
        txs[0] = Val::Str(vec![0xff; 64]);
    });
    match scan_block(&node, 701).await {
        Err(_) => {}
        Ok(scanned) => {
            assert_eq!(scanned.block_hash, honest.block_hash);
            assert!(
                scanned.outputs.len() == honest.outputs.len()
                    && scanned.spent_key_images == honest.spent_key_images,
                "block 701 was accepted as scanned with {} outputs and {} key images (honest: {} \
                 and {})",
                scanned.outputs.len(),
                scanned.spent_key_images.len(),
                honest.outputs.len(),
                honest.spent_key_images.len()
            );
        }
    }
}

#[tokio::test]
async fn pre_ringct_blocks_are_parsed_and_keep_their_real_hash() {
    let node = Node::honest(SnapshotNode::new());
    let scanned = scan_blocks_batch(&node.rpc(), 0, SEED_W, NETWORK, LOOKAHEAD, false, "")
        .await
        .unwrap();
    assert_eq!(scanned[0].block_height, 0);
    assert_eq!(scanned[0].block_hash, hostile::snapshot().chain[0].hash);
    assert_eq!(scanned[0].tx_count, 1);
    assert!(scanned[0].outputs.is_empty());
}

#[tokio::test]
async fn malformed_blocks_fail_the_batch_without_panicking() {
    type Edit = fn(&mut Obj);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("block-truncated", |block| {
            let mut blob = epee::get(block, "block").unwrap().bytes().to_vec();
            blob.truncate(blob.len() / 2);
            epee::set(block, "block", Val::Str(blob));
        }),
        ("block-claims-2^62-transactions", |block| {
            let mut blob = epee::get(block, "block").unwrap().bytes().to_vec();
            blob.truncate(blob.len() - 33);
            blob.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40]);
            epee::set(block, "block", Val::Str(blob));
        }),
        ("block-trailing-transaction-missing", |block| {
            epee::remove(block, "txs");
        }),
        ("block-extra-transaction", |block| {
            let txs = epee::get_mut(block, "txs").unwrap().arr_mut();
            txs.push(txs[0].clone());
        }),
        ("block-field-wrong-type", |block| {
            epee::set(block, "block", Val::U64(7));
        }),
    ];
    for (name, edit) in cases {
        let node = first_block_node(name, move |block, _| edit(block));
        scan_block(&node, 701).await.expect_err(name);
    }
}

#[tokio::test]
async fn transactions_out_of_order_within_a_block_are_refused() {
    let node = first_block_node("txs-swapped", |block, _| {
        epee::get_mut(block, "txs").unwrap().arr_mut().swap(0, 1);
    });
    let err = scan_block(&node, 700).await.unwrap_err();
    assert!(err.contains("lists transaction"), "{err}");
}

#[tokio::test]
async fn blocks_out_of_order_are_refused() {
    let node = Node::with(SnapshotNode::new(), "blocks-swapped", |call, reply| {
        if call.is("getblocks.bin") {
            return edit_bin(reply, |obj| {
                epee::get_mut(obj, "blocks").unwrap().arr_mut().swap(0, 1);
                epee::get_mut(obj, "output_indices")
                    .unwrap()
                    .arr_mut()
                    .swap(0, 1);
            });
        }
        reply
    });
    let err = scan_block(&node, 700).await.unwrap_err();
    assert!(err.contains("height mismatch"), "{err}");
}

#[tokio::test]
async fn index_arrays_that_do_not_fit_the_block_are_refused() {
    type Edit = fn(&mut Obj);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("indices-block-entry-empty", |indices| {
            epee::remove(indices, "indices");
        }),
        ("indices-extra-transaction-entry", |indices| {
            let entries = epee::get_mut(indices, "indices").unwrap().arr_mut();
            entries.push(entries[0].clone());
        }),
        ("indices-near-u64-max", |indices| {
            tx_indices(indices, 0)[0] = Val::U64(u64::MAX);
        }),
    ];
    for (name, edit) in cases {
        let node = first_block_node(name, move |_, indices| edit(indices));
        scan_block(&node, 701).await.expect_err(name);
    }
}

#[tokio::test]
async fn per_transaction_arrays_cannot_redirect_the_stored_index() {
    let honest = honest_block(701).await;
    let honest_index = honest.outputs[0].global_index;
    assert!(matches!(honest_index, Some(706) | Some(707)));
    type Edit = fn(&mut Obj);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("indices-tx-array-short", |indices| {
            tx_indices(indices, 1).truncate(1);
        }),
        ("indices-tx-array-long", |indices| {
            tx_indices(indices, 1).extend([Val::U64(1), Val::U64(2)]);
        }),
        ("indices-tx-array-duplicated", |indices| {
            let first = tx_indices(indices, 1)[0].clone();
            tx_indices(indices, 1)[1] = first;
        }),
        ("indices-tx-array-from-another-transaction", |indices| {
            *tx_indices(indices, 1) = vec![Val::U64(13), Val::U64(14)];
        }),
    ];
    for (name, edit) in cases {
        let node = first_block_node(name, move |_, indices| edit(indices));
        if let Ok(scanned) = scan_block(&node, 701).await {
            assert_eq!(scanned.outputs[0].global_index, honest_index, "{name}");
        }
    }
}

#[tokio::test]
async fn missing_index_arrays_are_refused_whether_or_not_the_block_pays_the_wallet() {
    let strip = |name| {
        Node::with(SnapshotNode::new(), name, |call, reply| {
            if call.is("getblocks.bin") {
                return edit_bin(reply, |obj| {
                    epee::remove(obj, "output_indices");
                });
            }
            reply
        })
    };
    let stranger = SnapshotNode::new();
    stranger.switch_chain(Chain::Shallow);
    let unowned = Node::with(stranger, "indices-missing-unowned", |call, reply| {
        if call.is("getblocks.bin") {
            return edit_bin(reply, |obj| {
                epee::remove(obj, "output_indices");
            });
        }
        reply
    });
    let owned = scan_block(&strip("indices-missing-owned"), 701).await;
    let not_owned = scan_block(&unowned, 835).await;
    assert!(
        owned.is_err(),
        "a block paying the wallet must not be stored without indices"
    );
    assert!(
        not_owned.is_err(),
        "the same malformed reply was accepted for a block that does not pay the wallet, so the \
         outcome tells the node which blocks do"
    );
}

#[tokio::test]
async fn emptied_miner_index_entry_never_shifts_the_stored_index() {
    let honest = honest_block(701).await;
    let node = first_block_node("indices-miner-entry-empty", |_, indices| {
        tx_indices(indices, 0).clear();
    });
    match scan_block(&node, 701).await {
        Err(_) => {}
        Ok(scanned) => assert_eq!(
            scanned.outputs[0].global_index, honest.outputs[0].global_index,
            "the wallet stored a global index the node never sent for this output"
        ),
    }
}

#[tokio::test]
async fn parity_consistently_shifted_indices_are_stored() {
    let honest = honest_block(701).await;
    let node = first_block_node("indices-shifted", |_, indices| {
        for tx in 0..2 {
            for index in tx_indices(indices, tx).iter_mut() {
                *index = Val::U64(index.u64() + 1_000_000);
            }
        }
    });
    let scanned = scan_block(&node, 701).await.expect("accepted");
    assert_eq!(
        scanned.outputs[0].global_index,
        honest.outputs[0].global_index.map(|i| i + 1_000_000)
    );
}

#[tokio::test]
async fn honest_rescan_repairs_a_poisoned_global_index() {
    let honest = honest_block(701).await;
    let poisoner = first_block_node("indices-shifted", |_, indices| {
        for tx in 0..2 {
            for index in tx_indices(indices, tx).iter_mut() {
                *index = Val::U64(index.u64() + 1_000_000);
            }
        }
    });
    let poisoned = scan_block(&poisoner, 701).await.unwrap();
    let mut wallet = Wallet::new(SEED_W);
    wallet.state.add_outputs(poisoned.outputs);
    wallet.state.add_outputs(honest.outputs.clone());
    assert_eq!(
        wallet.outputs()[0].global_index,
        honest.outputs[0].global_index,
        "the index stored from the hostile node is still in the wallet after an honest rescan"
    );
}

#[tokio::test]
async fn shallow_reorg_converges_on_the_new_chain() {
    let honest = SnapshotNode::new();
    let node = Node::honest(honest.clone());
    let mut wallet = hostile::synced_wallet().await;
    honest.switch_chain(Chain::Shallow);
    wallet.refresh(&node.rpc(), 10).await.unwrap();
    assert_eq!(wallet.reorgs.len(), 1);
    assert_eq!(wallet.reorgs[0].split_height, 830);
    assert_eq!(
        fingerprint(&wallet),
        fingerprint(&fresh_sync(Chain::Shallow).await)
    );
    assert_eq!(wallet.state.current_height, 854);
}

#[tokio::test]
async fn deep_reorg_with_full_hash_history_is_refused_beyond_wallet2s_limit() {
    let honest = SnapshotNode::new();
    let node = Node::honest(honest.clone());
    let mut wallet = hostile::synced_wallet().await;
    let before = fingerprint(&wallet);
    honest.switch_chain(Chain::Deep);
    let refreshed = wallet.refresh(&node.rpc(), 10).await;
    assert!(
        refreshed
            .as_ref()
            .is_err_and(|e| e.contains("exceeds maximum 100")),
        "{refreshed:?}"
    );
    assert!(wallet.reorgs.is_empty());
    assert_eq!(fingerprint(&wallet), before);

    wallet.state.set_max_reorg_depth(200);
    wallet.refresh(&node.rpc(), 10).await.unwrap();
    assert_eq!(wallet.reorgs.len(), 1);
    assert_eq!(
        fingerprint(&wallet),
        fingerprint(&fresh_sync(Chain::Deep).await)
    );
}

#[tokio::test]
async fn deep_reorg_after_compaction_leaves_no_orphaned_outputs_or_spends() {
    let honest = SnapshotNode::new();
    let node = Node::honest(honest.clone());
    let mut wallet = hostile::synced_wallet().await;
    wallet.state.block_hashes.compact();
    honest.switch_chain(Chain::Deep);
    let refreshed = wallet.refresh(&node.rpc(), 10).await;
    let expected = fingerprint(&fresh_sync(Chain::Deep).await);
    let actual = fingerprint(&wallet);
    let orphaned: Vec<_> = actual.difference(&expected).collect();
    assert!(
        refreshed.is_err() || orphaned.is_empty(),
        "refresh reported success (split heights {:?}) but the wallet keeps {} records that do \
         not exist on the node's chain, e.g. {:?}",
        wallet
            .reorgs
            .iter()
            .map(|r| r.split_height)
            .collect::<Vec<_>>(),
        orphaned.len(),
        orphaned.first()
    );
}

#[tokio::test]
async fn flapping_between_two_chains_settles_without_corruption() {
    let honest = SnapshotNode::new();
    let inner = honest.clone();
    let flapping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = flapping.clone();
    let node = Node::with(honest.clone(), "flapping", move |call, reply| {
        if call.is("getblocks.bin") && flag.load(std::sync::atomic::Ordering::SeqCst) {
            inner.switch_chain(if call.nth % 2 == 0 {
                Chain::Shallow
            } else {
                Chain::Main
            });
        }
        reply
    });
    let mut wallet = hostile::synced_wallet().await;
    for _ in 0..4 {
        let _ = wallet.refresh(&node.rpc(), 6).await;
    }
    flapping.store(false, std::sync::atomic::Ordering::SeqCst);
    honest.switch_chain(Chain::Main);
    wallet.refresh(&node.rpc(), 10).await.unwrap();
    assert!(wallet.reorgs.len() >= 2);
    assert_eq!(
        fingerprint(&wallet),
        fingerprint(&hostile::synced_wallet().await)
    );
}

#[tokio::test]
async fn fabricated_reorg_below_the_restore_height_is_refused() {
    let honest = SnapshotNode::new();
    let mut wallet = Wallet::new(SEED_W);
    wallet.restore_height = 800;
    wallet
        .refresh(&Node::honest(honest.clone()).rpc(), 64)
        .await
        .unwrap();
    let before = fingerprint(&wallet);
    assert!(!before.is_empty());

    let deep = SnapshotNode::new();
    deep.switch_chain(Chain::Deep);
    let node = Node::with(honest, "reorg-below-restore-height", move |call, reply| {
        if !call.is("getblocks.bin") {
            return reply;
        }
        let request = epee::encode(&vec![
            ("block_ids".into(), Val::Str(vec![])),
            ("start_height".into(), Val::U64(650)),
            ("prune".into(), Val::Bool(false)),
            ("no_miner_tx".into(), Val::Bool(false)),
        ]);
        futures::executor::block_on(monero_rust::monero_rpc::RpcConnection::post(
            &deep,
            "getblocks.bin",
            request,
        ))
    });
    let err = wallet.refresh(&node.rpc(), 8).await.unwrap_err();
    assert!(err.contains("not in the supplied wallet history"), "{err}");
    assert_eq!(fingerprint(&wallet), before);
}

#[tokio::test]
async fn real_reorg_below_the_restore_height_leaves_a_consistent_wallet() {
    let honest = SnapshotNode::new();
    let node = Node::honest(honest.clone());
    let mut wallet = Wallet::new(SEED_W);
    wallet.restore_height = 800;
    wallet.refresh(&node.rpc(), 10).await.unwrap();
    let before = fingerprint(&wallet);
    honest.switch_chain(Chain::Deep);
    let outcome = wallet.refresh(&node.rpc(), 10).await;
    println!("refresh across a reorg below the restore height: {outcome:?}");
    match outcome {
        Err(_) => assert_eq!(fingerprint(&wallet), before),
        Ok(_) => {
            assert_eq!(
                fingerprint(&wallet),
                fingerprint(&fresh_sync(Chain::Deep).await)
            );
            assert!(node
                .names()
                .iter()
                .all(|n| n == "getblocks.bin" || n == "json_rpc:get_block_header_by_height"));
        }
    }
}

#[tokio::test]
async fn history_of_a_restored_wallet_is_completed_with_the_genesis_id() {
    let honest = SnapshotNode::new();
    honest.set_height(820);
    let node = Node::honest(honest.clone());
    let mut wallet = Wallet::new(SEED_W);
    wallet.restore_height = 800;
    wallet.refresh(&node.rpc(), 8).await.unwrap();
    assert_eq!(wallet.state.current_height, 819);
    assert_eq!(wallet.state.block_hashes.get_hash(0), None);

    honest.set_height(850);
    node.clear_log();
    wallet.refresh(&node.rpc(), 8).await.unwrap();
    assert_eq!(wallet.state.current_height, 849);
    let genesis_requests: Vec<_> = node
        .requests("json_rpc:get_block_header_by_height")
        .iter()
        .map(|request| request.json()["params"]["height"].as_u64())
        .collect();
    assert_eq!(
        genesis_requests,
        vec![Some(0)],
        "asked for once per connection"
    );
    assert!(node.count("getblocks.bin") >= 2);
    assert_eq!(fingerprint(&wallet).len(), 50);
    assert_history_requests_end_in_genesis(&node);
}

fn assert_history_requests_end_in_genesis(node: &Node) {
    let genesis = hex::decode(&hostile::snapshot().chain[0].hash).unwrap();
    for request in node.requests("getblocks.bin") {
        let request = request.epee();
        if epee::get(&request, "start_height").map_or(0, Val::u64) != 0 {
            continue;
        }
        let ids = epee::get(&request, "block_ids").unwrap().bytes();
        assert_eq!(&ids[ids.len() - 32..], &genesis[..]);
    }
}

#[tokio::test]
async fn stale_height_zero_hash_is_never_sent_to_the_daemon() {
    let honest = SnapshotNode::new();
    let node = Node::honest(honest.clone());
    let mut wallet = hostile::synced_wallet().await;
    wallet.state.record_block_hash(0, [0x9c; 32]);
    honest.switch_chain(Chain::Shallow);
    wallet.refresh(&node.rpc(), 10).await.unwrap();
    assert_eq!(wallet.state.current_height, 854);
    assert_history_requests_end_in_genesis(&node);
    for request in node.requests("getblocks.bin") {
        let reply = futures::executor::block_on(monero_rust::monero_rpc::RpcConnection::post(
            &honest,
            "getblocks.bin",
            request.body.clone(),
        ))
        .unwrap();
        assert_eq!(
            epee::get(&epee::decode(&reply).unwrap(), "status")
                .unwrap()
                .bytes(),
            b"OK"
        );
    }
}

#[tokio::test]
async fn node_reported_height_never_unlocks_immature_outputs() {
    let node = Node::with(
        SnapshotNode::new(),
        "current-height-far-future",
        |call, reply| {
            if call.is("getblocks.bin") {
                return edit_bin(reply, |obj| {
                    epee::set(obj, "current_height", Val::U64(4_000_000_000));
                });
            }
            reply
        },
    );
    let mut wallet = Wallet::new(SEED_W);
    wallet.restore_height = 780;
    wallet.refresh(&node.rpc(), 8).await.unwrap();
    let tip = wallet.state.current_height;
    let immature: Vec<u64> = wallet
        .state
        .spendable_outputs()
        .iter()
        .map(|o| o.block_height)
        .filter(|height| height + 60 > tip + 1)
        .collect();
    assert!(
        immature.is_empty(),
        "spendable_outputs() offers {} coinbase outputs with fewer than 60 confirmations at the \
         wallet's own tip {tip}, the youngest from block {}",
        immature.len(),
        immature.iter().max().unwrap()
    );
}

#[tokio::test]
async fn batch_that_starts_after_the_requested_height_is_refused() {
    let node = Node::with(
        SnapshotNode::new(),
        "start-height-skips-ahead",
        |call, reply| {
            if call.is("getblocks.bin") {
                return edit_bin(reply, |obj| {
                    epee::get_mut(obj, "blocks").unwrap().arr_mut().remove(0);
                    epee::get_mut(obj, "output_indices")
                        .unwrap()
                        .arr_mut()
                        .remove(0);
                    epee::set(obj, "start_height", Val::U64(702));
                });
            }
            reply
        },
    );
    let err = scan_block(&node, 701).await.unwrap_err();
    assert!(err.contains("never skip forward"), "{err}");
}

#[tokio::test]
async fn selection_spends_only_outputs_unlocked_on_the_wallets_chain() {
    use monero_rust::native::FeePolicy;

    let node = Node::with(
        SnapshotNode::new(),
        "claimed-height-and-unlocked",
        |call, reply| {
            if call.is("getblocks.bin") {
                return edit_bin(reply, |obj| {
                    epee::set(obj, "current_height", Val::U64(4_000_000_000));
                });
            }
            if call.is("get_outs.bin") {
                return hostile::edit_outs(call, reply, |_, out| {
                    epee::set(out, "unlocked", Val::Bool(true));
                });
            }
            reply
        },
    );
    let mut wallet = Wallet::new(SEED_W);
    wallet.restore_height = 760;
    wallet.refresh(&node.rpc(), 8).await.unwrap();
    assert_eq!(wallet.state.daemon_height, 4_000_000_000);
    let height = wallet.state.chain_height();
    assert_eq!(height, wallet.state.current_height + 1);
    assert!(wallet.outputs().iter().all(|o| o.is_coinbase && !o.spent));
    let mature = |o: &&monero_rust::WalletOutput| o.block_height + 60 <= height;
    let (old, young): (Vec<_>, Vec<_>) = wallet.outputs().iter().partition(mature);
    assert!(
        !old.is_empty() && !young.is_empty(),
        "{} old, {} young",
        old.len(),
        young.len()
    );
    let old_keys: BTreeSet<_> = old.iter().map(|o| o.key_image.clone()).collect();
    let old_total: u64 = old.iter().map(|o| o.amount).sum();

    let spendable: BTreeSet<_> = wallet
        .state
        .spendable_outputs()
        .iter()
        .map(|o| o.key_image.clone())
        .collect();
    assert_eq!(spendable, old_keys);

    let rpc = node.rpc();
    let prepare = |amount| {
        monero_rust::prepare_send_inputs_with_rpc(
            &rpc,
            wallet.outputs(),
            height,
            amount,
            1,
            false,
            None,
            None,
            0,
            FeePolicy::default(),
        )
    };
    let prepared = prepare(old_total / 2).await.expect("the old outputs pay");
    assert!(!prepared.stored_outputs.is_empty());
    assert!(prepared
        .stored_outputs
        .iter()
        .all(|o| old_keys.contains(&o.key_image)));
    assert!(
        prepare(old_total + 1).await.is_err(),
        "only outputs locked at the wallet's tip could make up the amount"
    );
}
