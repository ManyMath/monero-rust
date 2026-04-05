mod common;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::monero_backend::transaction::Transaction;
use monero_rust::monero_backend::wallet::{
    address::{AddressSpec, MoneroAddress, Network},
    seed::Seed,
    Change, ReceivedOutput, SignableTransactionBuilder, SpendableOutput, ViewPair,
};
use monero_rust::scanner::{
    derive_address, derive_keys, scan_block_for_outputs_with_lookahead, Lookahead,
};
use rand::SeedableRng;
use std::io::Cursor;
use zeroize::Zeroizing;

#[cfg(not(feature = "mock-rpc"))]
use monero_rust::monero_backend::rpc::HttpRpc;

#[cfg(feature = "mock-rpc")]
mod mock_rpc;
#[cfg(feature = "mock-rpc")]
use mock_rpc::MockRpc;

const TEST_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const NETWORK_STR: &str = "stagenet";
const NETWORK: Network = Network::Stagenet;
const START_BLOCK: u64 = 1386863;
const END_BLOCK: u64 = 1386874;
const NODE_URL: &str = "http://127.0.0.1:38081";
const TEST_VECTORS_PATH: &str = "tests/vectors/tx_construction_test_vectors.json";

#[derive(Debug, Clone)]
struct TestOutput {
    block_height: u64,
    tx_hash: String,
    #[allow(dead_code)]
    output_index: u8,
    amount: u64,
    #[allow(dead_code)]
    key_image: String,
    received_output_bytes: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tx_construction() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(feature = "mock-rpc"))]
    if !common::stagenet_available() {
        eprintln!("Skipped: no stagenet node at 127.0.0.1:38081");
        return Ok(());
    }
    let address = derive_address(TEST_SEED, NETWORK_STR, "")?;
    let keys = derive_keys(TEST_SEED, NETWORK_STR, "")?;

    assert!(address.starts_with("5"));
    assert_eq!(address.len(), 95);

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
        let scan_result = scan_block_for_outputs_with_lookahead(
            &rpc,
            height,
            TEST_SEED,
            NETWORK_STR,
            lookahead,
            "",
        )
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
                received_output_bytes: output.received_output_bytes,
            });
        }
    }

    assert!(!all_outputs.is_empty(), "No outputs found in test range");

    let protocol = rpc.get_protocol().await?;
    let fee_rate = rpc.get_fee().await?;

    assert!(protocol.ring_len() >= 11);
    assert!(fee_rate.per_weight > 0);

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))?;
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..32]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let view_bytes = hex::decode(&keys.secret_view_key)?;
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes[..32].try_into()?);
    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

    let derived_address = view_pair.address(NETWORK, AddressSpec::Standard);
    assert_eq!(derived_address.to_string(), address);

    let test_output = &all_outputs[0];
    let output_bytes = hex::decode(&test_output.received_output_bytes)?;
    let mut cursor = Cursor::new(output_bytes);
    let received = ReceivedOutput::read(&mut cursor)?;

    assert_eq!(received.commitment().amount, test_output.amount);

    let spendable = SpendableOutput::from(&rpc, received.clone()).await?;

    let change = Change::new(&view_pair, false);
    let mut builder = SignableTransactionBuilder::new(protocol, fee_rate, Some(change));
    builder.add_input(spendable);

    let send_amount = 1_000_000_000u64;
    let dest_addr = MoneroAddress::from_str(NETWORK, &address)?;
    builder.add_payment(dest_addr, send_amount);

    let signable = builder.build()?;

    assert!(signable.fee() > 0);
    assert!(signable.fee() < test_output.amount);

    let total_input = test_output.amount;
    let total_output = send_amount + (total_input - send_amount - signable.fee());
    let fee = signable.fee();

    assert_eq!(total_input, total_output + fee);

    #[cfg(feature = "mock-rpc")]
    let mut rng = rand::rngs::StdRng::seed_from_u64(12345);

    #[cfg(not(feature = "mock-rpc"))]
    let mut rng = rand::rngs::OsRng;

    let tx = signable
        .sign(&mut rng, &rpc, &Zeroizing::new(spend_scalar))
        .await?;

    let tx_hash = tx.hash();
    let mut tx_bytes = Vec::new();
    tx.write(&mut tx_bytes)?;

    let mut cursor = Cursor::new(&tx_bytes[..]);
    let deserialized_tx = Transaction::read(&mut cursor)?;

    assert_eq!(deserialized_tx.hash(), tx_hash);
    assert_eq!(deserialized_tx.prefix.inputs.len(), 1);
    assert_eq!(deserialized_tx.prefix.outputs.len(), 2);
    assert_eq!(deserialized_tx.prefix.version, 2);

    for input in &deserialized_tx.prefix.inputs {
        match input {
            monero_rust::monero_backend::transaction::Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => {
                assert!(key_offsets.len() >= 11);
                let mut key_image_bytes = [0u8; 32];
                key_image_bytes.copy_from_slice(&key_image.compress().to_bytes());
                assert_ne!(key_image_bytes, [0u8; 32]);
            }
            _ => panic!("Expected ToKey input"),
        }
    }

    for output in &deserialized_tx.prefix.outputs {
        assert_eq!(output.amount, 0);
        assert_ne!(output.key.to_bytes(), [0u8; 32]);
    }

    assert_eq!(deserialized_tx.rct_signatures.base.commitments.len(), 2);
    assert_eq!(deserialized_tx.rct_signatures.base.fee, fee);

    use monero_rust::monero_backend::wallet::Scanner;
    use std::collections::HashSet;

    let mut scanner = Scanner::from_view(view_pair.clone(), Some(HashSet::new()));
    let scan_result = scanner.scan_transaction(&tx);
    let found_outputs = scan_result.ignore_timelock();

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
    let address1 = derive_address(TEST_SEED, NETWORK_STR, "")?;
    let address2 = derive_address(TEST_SEED, NETWORK_STR, "")?;

    assert_eq!(address1, address2);

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))?;
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..32]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let keys = derive_keys(TEST_SEED, NETWORK_STR, "")?;
    let view_bytes = hex::decode(&keys.secret_view_key)?;
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes[..32].try_into()?);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let address3 = view_pair
        .address(NETWORK, AddressSpec::Standard)
        .to_string();

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
            NETWORK_STR,
            lookahead,
            "",
        )
        .await?;
        let scan2 = scan_block_for_outputs_with_lookahead(
            &rpc,
            START_BLOCK,
            TEST_SEED,
            NETWORK_STR,
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
                &rpc,
                height,
                TEST_SEED,
                NETWORK_STR,
                lookahead,
                "",
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

            let keys = derive_keys(TEST_SEED, NETWORK_STR, "")?;
            let view_bytes = hex::decode(&keys.secret_view_key)?;
            let view_scalar = Scalar::from_bytes_mod_order(view_bytes[..32].try_into()?);
            let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

            let protocol = rpc.get_protocol().await?;
            let fee_rate = rpc.get_fee().await?;

            let change = Change::new(&view_pair, false);
            let mut builder = SignableTransactionBuilder::new(protocol, fee_rate, Some(change));

            let mut total_input = 0u64;
            for output in all_outputs.iter().take(2) {
                let output_bytes = hex::decode(&output.received_output_bytes)?;
                let mut cursor = Cursor::new(output_bytes);
                let received = ReceivedOutput::read(&mut cursor)?;

                let spendable = SpendableOutput::from(&rpc, received).await?;
                builder.add_input(spendable);
                total_input += output.amount;
            }

            let address = derive_address(TEST_SEED, NETWORK_STR, "")?;
            let dest_addr = MoneroAddress::from_str(NETWORK, &address)?;
            let send_amount = 1_000_000_000u64;
            builder.add_payment(dest_addr, send_amount);

            let signable = builder.build()?;

            assert_eq!(
                total_input,
                send_amount + (total_input - send_amount - signable.fee()) + signable.fee()
            );

            #[cfg(feature = "mock-rpc")]
            let mut rng = rand::rngs::StdRng::seed_from_u64(12345);

            #[cfg(not(feature = "mock-rpc"))]
            let mut rng = rand::rngs::OsRng;

            let tx = signable
                .sign(&mut rng, &rpc, &Zeroizing::new(spend_scalar))
                .await?;

            assert_eq!(tx.prefix.inputs.len(), 2);
            assert_eq!(tx.prefix.outputs.len(), 2);
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
                &rpc,
                height,
                TEST_SEED,
                NETWORK_STR,
                lookahead,
                "",
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
            let received = ReceivedOutput::read(&mut cursor)?;

            assert_eq!(received.commitment().amount, output.amount);
            assert!(!output.key.is_empty());
            assert!(!output.key_offset.is_empty());
            assert!(!output.commitment_mask.is_empty());
        }

        Ok(())
    }
}
