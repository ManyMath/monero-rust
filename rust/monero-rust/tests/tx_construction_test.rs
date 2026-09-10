mod common;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::native::build_unsigned_transaction;
use monero_rust::scanner::{
    derive_address, derive_keys, scan_block_for_outputs_with_lookahead, Lookahead,
};
use monero_rust::seed_compat::Seed;
use monero_rust::unsigned_tx::{sign_offline, UnsignedChange};
use monero_wallet::address::Network;
use monero_wallet::WalletOutput;
#[cfg(feature = "mock-rpc")]
use rand::SeedableRng;
use std::io::Cursor;
use zeroize::Zeroizing;

#[cfg(not(feature = "mock-rpc"))]
use monero_rust::monero_rpc::HttpRpc;

#[cfg(feature = "mock-rpc")]
mod mock_rpc;
#[cfg(feature = "mock-rpc")]
use mock_rpc::MockRpc;

const TEST_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const NETWORK: Network = Network::Stagenet;
const START_BLOCK: u64 = 1386863;
const END_BLOCK: u64 = 1386874;
#[cfg(not(feature = "mock-rpc"))]
const NODE_URL: &str = "http://127.0.0.1:38081";
#[cfg(feature = "mock-rpc")]
const TEST_VECTORS_PATH: &str = "tests/vectors/tx_construction_test_vectors.json";

#[derive(Debug, Clone)]
struct TestOutput {
    // Retain this fixture field for debug output.
    #[allow(dead_code)]
    block_height: u64,
    #[allow(dead_code)]
    tx_hash: String,
    #[allow(dead_code)]
    output_index: u8,
    amount: u64,
    #[allow(dead_code)]
    key_image: String,
    global_index: u64,
    received_output_bytes: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tx_construction() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(feature = "mock-rpc"))]
    if !common::stagenet_available() {
        eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
        return Ok(());
    }
    let address = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "")?;
    let keys = derive_keys(TEST_SEED, monero_rust::Network::Stagenet, "")?;

    assert!(address.to_string().starts_with("5"));
    assert_eq!(address.to_string().len(), 95);

    #[cfg(not(feature = "mock-rpc"))]
    let rpc = HttpRpc::new(NODE_URL.to_string())?;

    #[cfg(feature = "mock-rpc")]
    let rpc = {
        let mock = MockRpc::from_file(TEST_VECTORS_PATH)?;
        mock_rpc::create_rpc(mock)
    };

    let current_height = rpc.get_height().await?;

    if current_height <= (END_BLOCK + 60) as usize {
        println!("Warning: outputs may not be mature (< 60 confirmations)");
    }

    let mut all_outputs = Vec::new();
    for height in START_BLOCK..=END_BLOCK {
        let lookahead = Lookahead {
            account: 0,
            subaddress: 10,
        };
        let scan_result =
            scan_block_for_outputs_with_lookahead(&rpc, height, TEST_SEED, NETWORK, lookahead, "")
                .await?;

        for output in scan_result.outputs {
            assert!(!output.tx_hash.is_empty());
            if !output.key_image.is_empty() {
                assert_eq!(output.key_image.len(), 64);
            }
            assert!(output.amount > 0);
            assert!(!output.received_output_bytes.is_empty());

            all_outputs.push(TestOutput {
                block_height: height,
                tx_hash: output.tx_hash,
                output_index: output.output_index,
                amount: output.amount,
                key_image: output.key_image,
                global_index: output
                    .global_index
                    .ok_or("indexed block scan did not return a global output index")?,
                received_output_bytes: output.received_output_bytes,
            });
        }
    }

    assert!(!all_outputs.is_empty(), "No outputs found in test range");

    let rct_type = rpc.get_rct_type().await?;
    let node_fee = rpc.get_fee().await?;

    assert!(monero_rust::unsigned_tx::ring_len(rct_type).is_some_and(|len| len >= 11));
    assert!(node_fee.per_weight > 0);
    let fee_rate = node_fee.rate().ok_or("node reported a zero fee rate")?;

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))?;
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..32]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let view_scalar = Scalar::from_bytes_mod_order(*keys.secret_view_key);
    let wallet_view_pair = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(spend_point),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(view_scalar)),
    )?;

    let derived_address = wallet_view_pair.legacy_address(NETWORK);
    assert_eq!(derived_address, address);

    let test_output = &all_outputs[0];
    let output_bytes = hex::decode(&test_output.received_output_bytes)?;
    let mut cursor = Cursor::new(output_bytes);
    let wallet_output = WalletOutput::read(&mut cursor)?;

    assert_eq!(wallet_output.commitment().amount, test_output.amount);

    let spendable = (wallet_output, test_output.global_index);

    let send_amount = 1_000_000_000u64;
    let change = UnsignedChange {
        address,
        subaddress: None,
    };

    #[cfg(feature = "mock-rpc")]
    let mut rng = rand::rngs::StdRng::seed_from_u64(12345);

    #[cfg(not(feature = "mock-rpc"))]
    let mut rng = rand::rngs::OsRng;

    let unsigned = build_unsigned_transaction(
        &mut rng,
        &rpc,
        rct_type,
        fee_rate,
        &[spendable],
        vec![(address, send_amount)],
        Some(change),
    )
    .await?;

    let resolved = unsigned.resolve(&wallet_view_pair)?;
    let fee = resolved.fee;
    assert!(fee > 0);
    assert!(fee < test_output.amount);

    let total_input = test_output.amount;
    assert_eq!(resolved.input_total, total_input);
    let resolved_change = resolved.change.expect("transaction has change");
    assert!(resolved_change.is_ours);
    assert_eq!(total_input, send_amount + resolved_change.amount + fee);

    let tx = sign_offline(
        &mut rng,
        &Zeroizing::new(spend_scalar),
        &Zeroizing::new(view_scalar),
        &unsigned,
    )?
    .tx;

    let tx_hash = tx.hash();
    let mut tx_bytes = Vec::new();
    tx.write(&mut tx_bytes)?;

    let mut cursor = Cursor::new(&tx_bytes[..]);
    let deserialized_tx = Transaction::<NotPruned>::read(&mut cursor)?;

    assert_eq!(deserialized_tx.hash(), tx_hash);
    assert_eq!(deserialized_tx.prefix().inputs.len(), 1);
    assert_eq!(deserialized_tx.prefix().outputs.len(), 2);
    assert_eq!(u64::from(deserialized_tx.version()), 2);

    for input in &deserialized_tx.prefix().inputs {
        match input {
            monero_oxide::transaction::Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => {
                assert!(key_offsets.len() >= 11);
                let mut key_image_bytes = [0u8; 32];
                key_image_bytes.copy_from_slice(&key_image.to_bytes());
                assert_ne!(key_image_bytes, [0u8; 32]);
            }
            _ => panic!("Expected ToKey input"),
        }
    }

    for output in &deserialized_tx.prefix().outputs {
        assert_eq!(output.amount.unwrap_or(0), 0);
        assert_ne!(output.key.to_bytes(), [0u8; 32]);
    }

    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &deserialized_tx
    else {
        panic!("signed TX must be a v2 transaction with RCT proofs");
    };
    assert_eq!(proofs.base.commitments.len(), 2);
    assert_eq!(proofs.base.fee, fee);

    use monero_oxide::transaction::Pruned;
    use monero_rust::scanner::scan_single_transaction;

    let mut scanner = monero_wallet::Scanner::new(wallet_view_pair.clone());
    let found_outputs =
        scan_single_transaction(&mut scanner, tx_hash, &Transaction::<Pruned>::from(tx))?;

    assert!(!found_outputs.is_empty());

    let expected_change = total_input - send_amount - fee;
    let has_change = found_outputs
        .iter()
        .any(|o| o.commitment().amount == expected_change);
    assert!(has_change);

    Ok(())
}

#[tokio::test]
async fn test_address_derivation_consistency() -> Result<(), Box<dyn std::error::Error>> {
    if !common::stagenet_available() {
        eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
        return Ok(());
    }
    let address1 = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "")?;
    let address2 = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "")?;

    assert_eq!(address1, address2);

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))?;
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..32]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let keys = derive_keys(TEST_SEED, monero_rust::Network::Stagenet, "")?;
    let view_scalar = Scalar::from_bytes_mod_order(*keys.secret_view_key);

    let view_pair = monero_wallet::ViewPair::new(
        monero_wallet::ed25519::Point::from(spend_point),
        Zeroizing::new(monero_wallet::ed25519::Scalar::from(view_scalar)),
    )?;
    let address3 = view_pair.legacy_address(NETWORK);

    assert_eq!(address1, address3);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_key_image_determinism() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "mock-rpc")]
    {
        return Ok(());
    }

    #[cfg(not(feature = "mock-rpc"))]
    {
        if !common::stagenet_available() {
            eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
            return Ok(());
        }
        let rpc = HttpRpc::new(NODE_URL.to_string())?;
        let lookahead = Lookahead {
            account: 0,
            subaddress: 10,
        };

        let scan1 = scan_block_for_outputs_with_lookahead(
            &rpc,
            START_BLOCK,
            TEST_SEED,
            NETWORK,
            lookahead,
            "",
        )
        .await?;
        let scan2 = scan_block_for_outputs_with_lookahead(
            &rpc,
            START_BLOCK,
            TEST_SEED,
            NETWORK,
            lookahead,
            "",
        )
        .await?;

        if !scan1.outputs.is_empty() && !scan2.outputs.is_empty() {
            assert_eq!(scan1.outputs.len(), scan2.outputs.len());

            for (out1, out2) in scan1.outputs.iter().zip(scan2.outputs.iter()) {
                assert_eq!(out1.tx_hash, out2.tx_hash);
                assert_eq!(out1.amount, out2.amount);

                if !out1.key_image.is_empty() && !out2.key_image.is_empty() {
                    assert_eq!(out1.key_image, out2.key_image);
                }
            }
        }

        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_transaction_with_multiple_inputs() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "mock-rpc")]
    {
        return Ok(());
    }

    #[cfg(not(feature = "mock-rpc"))]
    {
        if !common::stagenet_available() {
            eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
            return Ok(());
        }
        let rpc = HttpRpc::new(NODE_URL.to_string())?;
        let mut all_outputs = Vec::new();
        let lookahead = Lookahead {
            account: 0,
            subaddress: 10,
        };

        for height in START_BLOCK..=END_BLOCK {
            let scan_result = scan_block_for_outputs_with_lookahead(
                &rpc, height, TEST_SEED, NETWORK, lookahead, "",
            )
            .await?;

            for output in scan_result.outputs {
                all_outputs.push(output);
            }
        }

        if all_outputs.len() >= 2 {
            let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))?;
            let entropy = seed.entropy();
            let mut spend_bytes = [0u8; 32];
            spend_bytes.copy_from_slice(&entropy[..32]);
            let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
            let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

            let keys = derive_keys(TEST_SEED, monero_rust::Network::Stagenet, "")?;
            let view_scalar = Scalar::from_bytes_mod_order(*keys.secret_view_key);
            let wallet_view_pair = monero_wallet::ViewPair::new(
                monero_wallet::ed25519::Point::from(spend_point),
                Zeroizing::new(monero_wallet::ed25519::Scalar::from(view_scalar)),
            )?;

            let rct_type = rpc.get_rct_type().await?;
            let fee_rate = rpc
                .get_fee()
                .await?
                .rate()
                .ok_or("node reported a zero fee rate")?;

            let mut spendables = Vec::new();
            let mut total_input = 0u64;
            for output in all_outputs.iter().take(2) {
                let output_bytes = hex::decode(&output.received_output_bytes)?;
                let mut cursor = Cursor::new(output_bytes);
                let wallet_output = WalletOutput::read(&mut cursor)?;

                let global_index = resolve_global_index(&rpc, &wallet_output).await?;
                spendables.push((wallet_output, global_index));
                total_input += output.amount;
            }

            let address = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "")?;
            let send_amount = 1_000_000_000u64;

            #[cfg(feature = "mock-rpc")]
            let mut rng = rand::rngs::StdRng::seed_from_u64(12345);

            #[cfg(not(feature = "mock-rpc"))]
            let mut rng = rand::rngs::OsRng;

            let unsigned = build_unsigned_transaction(
                &mut rng,
                &rpc,
                rct_type,
                fee_rate,
                &spendables,
                vec![(address, send_amount)],
                Some(UnsignedChange {
                    address: wallet_view_pair.legacy_address(NETWORK),
                    subaddress: None,
                }),
            )
            .await?;

            let resolved = unsigned.resolve(&wallet_view_pair)?;
            assert_eq!(resolved.input_total, total_input);
            assert_eq!(
                total_input,
                send_amount + resolved.change.expect("change").amount + resolved.fee
            );

            let tx = sign_offline(
                &mut rng,
                &Zeroizing::new(spend_scalar),
                &Zeroizing::new(view_scalar),
                &unsigned,
            )?
            .tx;

            assert_eq!(tx.prefix().inputs.len(), 2);
            assert_eq!(tx.prefix().outputs.len(), 2);
        }

        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_transaction_parsing_and_validation() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "mock-rpc")]
    {
        return Ok(());
    }

    #[cfg(not(feature = "mock-rpc"))]
    {
        if !common::stagenet_available() {
            eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
            return Ok(());
        }
        let rpc = HttpRpc::new(NODE_URL.to_string())?;
        let lookahead = Lookahead {
            account: 0,
            subaddress: 10,
        };

        let mut test_output = None;
        for height in START_BLOCK..=END_BLOCK {
            let scan_result = scan_block_for_outputs_with_lookahead(
                &rpc, height, TEST_SEED, NETWORK, lookahead, "",
            )
            .await?;

            if !scan_result.outputs.is_empty() {
                test_output = Some(scan_result.outputs[0].clone());
                break;
            }
        }

        if let Some(output) = test_output {
            let output_bytes = hex::decode(&output.received_output_bytes)?;
            let mut cursor = Cursor::new(&output_bytes);
            let wallet_output = WalletOutput::read(&mut cursor)?;

            assert_eq!(wallet_output.commitment().amount, output.amount);
            assert!(!output.key.is_empty());
            assert!(!output.key_offset.is_empty());
            assert!(!output.commitment_mask.is_empty());
        }

        Ok(())
    }
}
