// Test verifying key image calculation and spent detection.
// Requires local stagenet node at 127.0.0.1:38081.
// Run with: cargo test --test spent_detection_test -- --nocapture --ignored

use monero_serai::rpc::HttpRpc;
use monero_serai::transaction::Input;

const NODE_URL: &str = "http://127.0.0.1:38081";
const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

const RECEIVE_BLOCKS: &[u64] = &[2032114, 2032323, 2032324, 2032326];
const SPEND_BLOCK: u64 = 2032338;

#[tokio::test]
#[ignore] // requires local stagenet node
async fn test_key_image_calculation() {
    let rpc = HttpRpc::new(NODE_URL.to_string()).expect("Failed to create RPC");

    // scan blocks to find our outputs
    let mut our_key_images = Vec::new();
    for &block_height in RECEIVE_BLOCKS {
        let result = monero_wasm::scanner_native::scan_block_for_outputs_with_url(
            NODE_URL,
            block_height,
            TEST_SEED,
            "stagenet",
        )
        .await
        .expect("scan failed");

        for output in &result.outputs {
            our_key_images.push(output.key_image.clone());
        }
    }

    assert!(!our_key_images.is_empty(), "should find outputs in receive blocks");

    // extract key images from spend block
    let block = rpc
        .get_block_by_number(SPEND_BLOCK as usize)
        .await
        .expect("failed to get spend block");

    let tx_hashes = block.txs.clone();
    assert!(!tx_hashes.is_empty(), "spend block should have transactions");

    let transactions = rpc
        .get_transactions(&tx_hashes)
        .await
        .expect("failed to get transactions");

    let mut blockchain_key_images = Vec::new();
    for tx in &transactions {
        for input in &tx.prefix.inputs {
            if let Input::ToKey { key_image, .. } = input {
                blockchain_key_images.push(hex::encode(key_image.compress().to_bytes()));
            }
        }
    }

    // verify our calculated key images match blockchain
    let matches: Vec<_> = blockchain_key_images
        .iter()
        .filter(|ki| our_key_images.contains(ki))
        .collect();

    assert!(
        !matches.is_empty(),
        "at least one of our key images should appear in spend block"
    );
}

#[tokio::test]
#[ignore] // requires local stagenet node
async fn test_spent_key_images_extracted_during_scan() {
    // verify that scanning the spend block extracts the spent key images
    let result = monero_wasm::scanner_native::scan_block_for_outputs_with_url(
        NODE_URL,
        SPEND_BLOCK,
        TEST_SEED,
        "stagenet",
    )
    .await
    .expect("scan failed");

    assert!(
        !result.spent_key_images.is_empty(),
        "spend block should contain spent key images"
    );

    // scan receive blocks to get our key images
    let mut our_key_images = Vec::new();
    for &block_height in RECEIVE_BLOCKS {
        let recv_result = monero_wasm::scanner_native::scan_block_for_outputs_with_url(
            NODE_URL,
            block_height,
            TEST_SEED,
            "stagenet",
        )
        .await
        .expect("scan failed");

        for output in &recv_result.outputs {
            our_key_images.push(output.key_image.clone());
        }
    }

    // verify at least one of our key images is in the spent list
    let spent_matches: Vec<_> = result
        .spent_key_images
        .iter()
        .filter(|ki| our_key_images.contains(ki))
        .collect();

    assert!(
        !spent_matches.is_empty(),
        "spent_key_images should include our spent output"
    );
}
