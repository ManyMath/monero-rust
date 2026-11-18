//! The send path against an adversarial node. `parity_*` tests pin
//! behaviour shared with wallet2.

#![cfg(not(target_arch = "wasm32"))]

mod hostile;

use curve25519_dalek::{
    constants::{ED25519_BASEPOINT_POINT, EIGHT_TORSION},
    edwards::{CompressedEdwardsY, EdwardsPoint},
    traits::Identity,
    Scalar,
};
use hostile::epee::{self, Val};
use hostile::{
    distribution_reply, edit_bin, edit_json, edit_outs, outs_reply, Chain, HostileNode,
    SnapshotNode, Wallet, NETWORK, NETWORK_STR, SEED_W, SEED_W2,
};
use monero_rust::monero_rpc::RpcError;
use monero_rust::native::{
    create_transaction_with_rpc, DecoySelection, StoredOutputData, TransactionResult,
};
use monero_rust::scanner::derive_address;
use monero_rust::{WalletOutput, WalletState};

type Node = HostileNode<SnapshotNode>;

fn absolute(ring: &DecoySelection) -> Vec<u64> {
    let mut index = 0;
    ring.offsets
        .iter()
        .map(|offset| {
            index += offset;
            index
        })
        .collect()
}

fn decoys_of(tx: &TransactionResult, real: u64) -> Vec<u64> {
    absolute(&tx.input_rings[0].ring)
        .into_iter()
        .filter(|index| *index != real)
        .collect()
}

fn change_output(wallet: &Wallet) -> WalletOutput {
    wallet
        .unspent()
        .into_iter()
        .find(|o| !o.is_coinbase && o.block_height == 700)
        .unwrap()
        .clone()
}

fn coinbase_at(wallet: &Wallet, height: u64) -> WalletOutput {
    wallet
        .unspent()
        .into_iter()
        .find(|o| o.is_coinbase && o.block_height == height)
        .unwrap()
        .clone()
}

async fn send(node: &Node, input: &WalletOutput) -> Result<TransactionResult, String> {
    let w2 = derive_address(SEED_W2, NETWORK, "").unwrap().to_string();
    let mut wallet_state = WalletState::new();
    wallet_state.set_ring_persister(Box::new(monero_rust::RingsNotPersisted));
    wallet_state.add_outputs(vec![input.clone()]);
    create_transaction_with_rpc(
        &node.rpc(),
        SEED_W,
        NETWORK_STR,
        vec![StoredOutputData::from(input)],
        &[(w2, 100_000_000_000)],
        "",
        &mut wallet_state,
    )
    .await
}

fn point_bytes(point: EdwardsPoint) -> Vec<u8> {
    point.compress().to_bytes().to_vec()
}

fn decompress(bytes: &[u8]) -> EdwardsPoint {
    CompressedEdwardsY(bytes.try_into().unwrap())
        .decompress()
        .unwrap()
}

#[tokio::test]
async fn distribution_with_no_blocks_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let node = Node::with(SnapshotNode::new(), "distribution-empty", |call, reply| {
        if call.is("get_output_distribution.bin") {
            return distribution_reply(&[]);
        }
        reply
    });
    let err = send(&node, &change_output(&wallet)).await.unwrap_err();
    assert!(err.contains("not enough blocks"), "{err}");
    assert_eq!(node.count("get_outs.bin"), 0);
}

#[tokio::test]
async fn distribution_of_only_empty_blocks_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let node = Node::with(SnapshotNode::new(), "distribution-zeros", |call, reply| {
        if call.is("get_output_distribution.bin") {
            return distribution_reply(&[0; 850]);
        }
        reply
    });
    let err = send(&node, &change_output(&wallet)).await.unwrap_err();
    assert!(err.contains("not enough decoy candidates"), "{err}");
    assert_eq!(node.count("get_outs.bin"), 0);
}

#[tokio::test]
async fn uncompressed_or_non_binary_distribution_is_refused() {
    let wallet = hostile::synced_wallet().await;
    for field in ["compress", "binary"] {
        let node = Node::with(
            SnapshotNode::new(),
            "distribution-flags",
            move |call, reply| {
                if call.is("get_output_distribution.bin") {
                    return edit_bin(reply, |obj| {
                        let first =
                            epee::get_mut(obj, "distributions").unwrap().arr_mut()[0].obj_mut();
                        epee::set(first, field, Val::Bool(false));
                    });
                }
                reply
            },
        );
        send(&node, &change_output(&wallet)).await.unwrap_err();
        assert_eq!(node.count("get_outs.bin"), 0);
    }
}

#[tokio::test]
async fn distribution_with_a_count_near_u64_max_fails_without_panicking() {
    let wallet = hostile::synced_wallet().await;
    let node = Node::with(
        SnapshotNode::new(),
        "distribution-huge-count",
        |call, reply| {
            if call.is("get_output_distribution.bin") {
                let mut per_block = vec![1u64; 850];
                per_block[400] = u64::MAX - 10_000;
                return distribution_reply(&per_block);
            }
            reply
        },
    );
    send(&node, &change_output(&wallet)).await.unwrap_err();
}

#[tokio::test]
async fn distribution_claiming_fewer_outputs_than_the_real_index_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let real = coinbase_at(&wallet, 780);
    let real_index = real.global_index.unwrap();
    let node = Node::with(
        SnapshotNode::new(),
        "distribution-truncated",
        |call, reply| {
            if call.is("get_output_distribution.bin") {
                let mut per_block = vec![1u64; 350];
                per_block[0] = 0;
                return distribution_reply(&per_block);
            }
            reply
        },
    );
    match send(&node, &real).await {
        Err(_) => {}
        Ok(tx) => {
            let decoys = decoys_of(&tx, real_index);
            if let Ok(url) = std::env::var("MONERO_REGTEST_RPC") {
                let live = hostile::LiveNode::new(&url);
                let body = serde_json::to_vec(
                    &serde_json::json!({ "tx_as_hex": tx.tx_blob, "do_not_relay": true }),
                )
                .unwrap();
                let verdict = monero_rust::monero_rpc::RpcConnection::post(
                    &live,
                    "send_raw_transaction",
                    body,
                )
                .await
                .unwrap();
                println!("live daemon verdict: {}", String::from_utf8_lossy(&verdict));
            }
            panic!(
                "built a transaction on a distribution that ends at output 349 while spending \
                 output {real_index}: decoys span {}..={}, the real spend is the outlier",
                decoys.iter().min().unwrap(),
                decoys.iter().max().unwrap()
            );
        }
    }
}

#[tokio::test]
async fn parity_crafted_distribution_pins_decoys_to_a_node_chosen_window() {
    let wallet = hostile::synced_wallet().await;
    let real = coinbase_at(&wallet, 100);
    let real_index = real.global_index.unwrap();
    let window = 600u64..700;
    let node = Node::with(SnapshotNode::new(), "distribution-window", |call, reply| {
        if call.is("get_output_distribution.bin") {
            let mut per_block = vec![0u64; 262_800];
            per_block[0] = 600;
            per_block[1] = 100;
            *per_block.last_mut().unwrap() = 7;
            return distribution_reply(&per_block);
        }
        reply
    });
    let tx = send(&node, &real)
        .await
        .expect("both wallets accept this distribution");
    let decoys = decoys_of(&tx, real_index);
    let pinned = decoys.iter().filter(|i| window.contains(i)).count();
    assert!(
        pinned >= 10,
        "expected most decoys inside the node's window, got {decoys:?}"
    );
}

#[tokio::test]
async fn parity_distribution_longer_than_the_reported_height_is_accepted() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    let honest = SnapshotNode::new();
    let total = *honest.cumulative_distribution().last().unwrap();
    let inner = honest.clone();
    let node = Node::with(
        honest,
        "distribution-longer-than-chain",
        move |call, reply| {
            if call.is("get_output_distribution.bin") {
                let mut per_block = inner.outputs_per_block();
                per_block.extend(std::iter::repeat(50).take(2_000));
                return distribution_reply(&per_block);
            }
            if call.is("get_outs.bin") {
                let request = epee::decode(&call.body).unwrap();
                let outs = epee::items(&request, "outputs")
                    .iter()
                    .map(|out| {
                        let index = epee::get(out.obj(), "index").unwrap().u64();
                        Val::Obj(inner.honest_out(index).unwrap_or_else(|| {
                            let fake =
                                point_bytes(Scalar::from(index + 7) * ED25519_BASEPOINT_POINT);
                            vec![
                                ("height".into(), Val::U64(849)),
                                ("key".into(), Val::Str(fake.clone())),
                                ("mask".into(), Val::Str(fake)),
                                ("txid".into(), Val::Str(vec![0; 32])),
                                ("unlocked".into(), Val::Bool(true)),
                            ]
                        }))
                    })
                    .collect();
                return Ok(outs_reply(outs));
            }
            reply
        },
    );
    let tx = send(&node, &real).await.expect("accepted");
    let decoys = decoys_of(&tx, real_index);
    let nonexistent = decoys.iter().filter(|i| **i >= total).count();
    assert!(
        nonexistent >= 12,
        "expected nearly every decoy to be an output that does not exist: {decoys:?}"
    );
}

#[tokio::test]
async fn unusable_decoys_are_never_put_in_a_ring() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    type Edit = fn(&mut epee::Obj);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("decoys-locked", |out| {
            epee::set(out, "unlocked", Val::Bool(false))
        }),
        ("decoys-height-in-the-future", |out| {
            epee::set(out, "height", Val::U64(u64::MAX))
        }),
        ("decoys-torsioned-key", |out| {
            let key = decompress(epee::get(out, "key").unwrap().bytes()) + EIGHT_TORSION[1];
            epee::set(out, "key", Val::Str(point_bytes(key)));
        }),
        ("decoys-torsioned-commitment", |out| {
            let mask = decompress(epee::get(out, "mask").unwrap().bytes()) + EIGHT_TORSION[1];
            epee::set(out, "mask", Val::Str(point_bytes(mask)));
        }),
        ("decoys-small-order-key", |out| {
            epee::set(out, "key", Val::Str(point_bytes(EIGHT_TORSION[3])));
        }),
        ("decoys-identity-key", |out| {
            epee::set(out, "key", Val::Str(point_bytes(EdwardsPoint::identity())));
        }),
        ("decoys-not-on-curve", |out| {
            let mut bytes = vec![0u8; 32];
            bytes[0] = 2;
            epee::set(out, "key", Val::Str(bytes));
        }),
        ("decoys-short-key", |out| {
            epee::set(out, "key", Val::Str(vec![1; 31]))
        }),
    ];
    for (name, edit) in cases {
        let node = Node::with(SnapshotNode::new(), name, move |call, reply| {
            if call.is("get_outs.bin") {
                return edit_outs(call, reply, |index, out| {
                    if index != real_index {
                        edit(out);
                    }
                });
            }
            reply
        });
        let result = send(&node, &real).await;
        assert!(
            result.is_err(),
            "{name}: built a transaction from unusable decoys"
        );
        assert_eq!(node.count("get_outs.bin"), 1, "{name}: no retry");
        assert_eq!(node.count("sendrawtransaction"), 0);
    }
}

#[tokio::test]
async fn a_different_view_of_the_real_output_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    type Edit = fn(&mut epee::Obj);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("real-locked", |out| {
            epee::set(out, "unlocked", Val::Bool(false))
        }),
        ("real-height-in-the-future", |out| {
            epee::set(out, "height", Val::U64(1_000_000))
        }),
        ("real-key-altered", |out| {
            let key = decompress(epee::get(out, "key").unwrap().bytes()) + ED25519_BASEPOINT_POINT;
            epee::set(out, "key", Val::Str(point_bytes(key)));
        }),
        ("real-key-torsioned", |out| {
            let key = decompress(epee::get(out, "key").unwrap().bytes()) + EIGHT_TORSION[1];
            epee::set(out, "key", Val::Str(point_bytes(key)));
        }),
        ("real-commitment-altered", |out| {
            let mask =
                decompress(epee::get(out, "mask").unwrap().bytes()) + ED25519_BASEPOINT_POINT;
            epee::set(out, "mask", Val::Str(point_bytes(mask)));
        }),
    ];
    for (name, edit) in cases {
        let node = Node::with(SnapshotNode::new(), name, move |call, reply| {
            if call.is("get_outs.bin") {
                return edit_outs(call, reply, |index, out| {
                    if index == real_index {
                        edit(out);
                    }
                });
            }
            reply
        });
        let err = send(&node, &real).await.expect_err(name);
        assert!(err.contains("different view of output"), "{name}: {err}");
        assert_eq!(node.count("get_outs.bin"), 1, "{name}: no retry");
    }
}

#[tokio::test]
async fn wrong_entry_counts_and_rotated_replies_are_refused() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    type Edit = fn(&mut Vec<Val>);
    let cases: Vec<(&'static str, Edit)> = vec![
        ("outs-one-missing", |outs| {
            outs.pop();
        }),
        ("outs-one-extra", |outs| outs.push(outs[0].clone())),
        ("outs-empty", |outs| outs.clear()),
        ("outs-rotated", |outs| outs.rotate_left(1)),
    ];
    for (name, edit) in cases {
        let node = Node::with(SnapshotNode::new(), name, move |call, reply| {
            if call.is("get_outs.bin") {
                return edit_bin(reply, |obj| {
                    edit(epee::get_mut(obj, "outs").unwrap().arr_mut())
                });
            }
            reply
        });
        send(&node, &real).await.expect_err(name);
        assert_eq!(node.count("get_outs.bin"), 1, "{name}: no retry");
    }
}

#[tokio::test]
async fn parity_one_key_served_for_every_decoy_is_accepted() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    let node = Node::with(
        SnapshotNode::new(),
        "decoys-same-key",
        move |call, reply| {
            if call.is("get_outs.bin") {
                let same = point_bytes(Scalar::from(42u64) * ED25519_BASEPOINT_POINT);
                return edit_outs(call, reply, |index, out| {
                    if index != real_index {
                        epee::set(out, "key", Val::Str(same.clone()));
                    }
                });
            }
            reply
        },
    );
    let tx = send(&node, &real).await.expect("accepted");
    let ring = &tx.input_rings[0].ring;
    let distinct: std::collections::HashSet<_> = ring.ring.iter().map(|m| &m.key).collect();
    assert_eq!(
        distinct.len(),
        2,
        "fifteen identical decoy keys plus the real key"
    );
}

#[tokio::test]
async fn parity_noncanonical_commitment_encoding_is_canonicalised_not_refused() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let real_index = real.global_index.unwrap();
    let mut noncanonical = vec![0xffu8; 32];
    noncanonical[0] = 0xee;
    noncanonical[31] = 0x7f;
    let served = noncanonical.clone();
    let node = Node::with(
        SnapshotNode::new(),
        "decoys-noncanonical",
        move |call, reply| {
            if call.is("get_outs.bin") {
                return edit_outs(call, reply, |index, out| {
                    if index != real_index {
                        epee::set(out, "mask", Val::Str(served.clone()));
                    }
                });
            }
            reply
        },
    );
    let tx = send(&node, &real).await.expect("accepted");
    let identity = hex::encode(point_bytes(EdwardsPoint::identity()));
    let ring = &tx.input_rings[0].ring;
    assert_eq!(
        ring.ring
            .iter()
            .filter(|m| m.commitment == identity)
            .count(),
        15
    );
    assert!(ring
        .ring
        .iter()
        .all(|m| m.commitment != hex::encode(&noncanonical)));
}

#[tokio::test]
async fn height_that_goes_backwards_between_calls_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let node = Node::with(SnapshotNode::new(), "height-backwards", |call, reply| {
        if call.is("get_height") && call.nth == 1 {
            return edit_json(reply, |v| v["height"] = 600.into());
        }
        reply
    });
    let err = send(&node, &change_output(&wallet)).await.unwrap_err();
    assert!(err.contains("blocks this node doesn't have"), "{err}");
    assert_eq!(node.count("get_outs.bin"), 0);
}

#[tokio::test]
async fn unparseable_or_absurd_heights_are_refused() {
    let wallet = hostile::synced_wallet().await;
    for height in [
        serde_json::json!(-1),
        serde_json::json!("850"),
        serde_json::json!(5),
    ] {
        let node = Node::with(SnapshotNode::new(), "height-bad", move |call, reply| {
            if call.is("get_height") {
                let height = height.clone();
                return edit_json(reply, move |v| v["height"] = height);
            }
            reply
        });
        send(&node, &change_output(&wallet)).await.unwrap_err();
        assert_eq!(node.count("get_outs.bin"), 0);
    }
}

#[tokio::test]
async fn parity_height_far_in_the_future_does_not_change_the_ring_source() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let node = Node::with(SnapshotNode::new(), "height-future", |call, reply| {
        if call.is("get_height") {
            return edit_json(reply, |v| v["height"] = 4_000_000_000u64.into());
        }
        reply
    });
    let tx = send(&node, &real).await.expect("accepted");
    assert!(absolute(&tx.input_rings[0].ring).iter().all(|i| *i < 856));
}

#[tokio::test]
async fn reorg_that_removes_the_real_output_before_get_outs_is_refused() {
    let wallet = hostile::synced_wallet().await;
    let real = change_output(&wallet);
    let honest = SnapshotNode::new();
    let inner = honest.clone();
    let node = Node::with(honest, "reorg-before-get-outs", move |call, reply| {
        if call.is("get_output_distribution.bin") {
            inner.switch_chain(Chain::Deep);
        }
        reply
    });
    let err = send(&node, &real).await.unwrap_err();
    assert!(err.contains("different view of output"), "{err}");
}

#[tokio::test]
async fn rpc_errors_are_not_retried_with_a_different_request() {
    let wallet = hostile::synced_wallet().await;
    let node = Node::with(
        SnapshotNode::new(),
        "get-outs-connection-error",
        |call, reply| {
            if call.is("get_outs.bin") {
                return Err(RpcError::ConnectionError);
            }
            reply
        },
    );
    send(&node, &change_output(&wallet)).await.unwrap_err();
    assert_eq!(node.count("get_outs.bin"), 1);
}
