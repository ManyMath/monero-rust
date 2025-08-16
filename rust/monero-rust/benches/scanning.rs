use criterion::{black_box, criterion_group, criterion_main, Criterion};
use monero_rust::scanner::{extract_key_images_from_raw_tx, BlockScanResult};
use monero_rust::WalletOutput;

/// Build a synthetic RingCT-style transaction blob with `num_inputs` ToKey
/// inputs, each containing a 32-byte key image and `ring_size` key offsets.
fn make_synthetic_tx_blob(num_inputs: usize, ring_size: usize) -> Vec<u8> {
    let mut blob = Vec::new();

    // version (varint 2)
    blob.push(2u8);
    // timelock (varint 0)
    blob.push(0u8);
    // num_inputs (varint)
    push_varint(&mut blob, num_inputs as u64);

    for i in 0..num_inputs {
        // Input type: ToKey = 0x02
        blob.push(0x02);
        // amount (varint 0 for RingCT)
        blob.push(0u8);
        // number of key offsets (varint)
        push_varint(&mut blob, ring_size as u64);
        // key offsets (varint each)
        for j in 0..ring_size {
            push_varint(&mut blob, (j as u64 + 1) * 1000);
        }
        // 32-byte key image (deterministic filler)
        let mut ki = [0u8; 32];
        ki[0] = i as u8;
        ki[31] = 0xff;
        blob.extend_from_slice(&ki);
    }

    blob
}

fn push_varint(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let byte = (val & 0x7f) as u8;
        val >>= 7;
        if val == 0 {
            buf.push(byte);
            break;
        } else {
            buf.push(byte | 0x80);
        }
    }
}

fn make_sample_wallet_output(index: u64) -> WalletOutput {
    WalletOutput {
        tx_hash: format!("{:064x}", index),
        output_index: 0,
        amount: 1_000_000_000_000,
        amount_xmr: "1.000000000000".to_string(),
        key: "a".repeat(64),
        key_offset: "b".repeat(64),
        commitment_mask: "c".repeat(64),
        subaddress_index: Some((0, 1)),
        payment_id: None,
        received_output_bytes: "d".repeat(128),
        block_height: 100_000 + index,
        spent: false,
        spent_height: None,
        key_image: "e".repeat(64),
        is_coinbase: false,
        frozen: false,
    }
}

fn make_sample_block_scan_result(num_outputs: usize, num_key_images: usize) -> BlockScanResult {
    BlockScanResult {
        block_height: 500_000,
        block_hash: "f".repeat(64),
        block_timestamp: 1700000000,
        tx_count: num_outputs.max(1),
        outputs: (0..num_outputs)
            .map(|i| make_sample_wallet_output(i as u64))
            .collect(),
        daemon_height: 600_000,
        spent_key_images: (0..num_key_images)
            .map(|i| format!("{:064x}", i))
            .collect(),
        spent_key_image_tx_hashes: (0..num_key_images)
            .map(|i| format!("{:064x}", i + 10000))
            .collect(),
    }
}

fn bench_extract_key_images(c: &mut Criterion) {
    let blob_2_16 = make_synthetic_tx_blob(2, 16);
    let blob_4_16 = make_synthetic_tx_blob(4, 16);
    let blob_16_16 = make_synthetic_tx_blob(16, 16);

    c.bench_function("extract_key_images/2_inputs_ring16", |b| {
        b.iter(|| extract_key_images_from_raw_tx(black_box(&blob_2_16)))
    });

    c.bench_function("extract_key_images/4_inputs_ring16", |b| {
        b.iter(|| extract_key_images_from_raw_tx(black_box(&blob_4_16)))
    });

    c.bench_function("extract_key_images/16_inputs_ring16", |b| {
        b.iter(|| extract_key_images_from_raw_tx(black_box(&blob_16_16)))
    });
}

fn bench_block_scan_result_json(c: &mut Criterion) {
    let result_empty = make_sample_block_scan_result(0, 10);
    let result_small = make_sample_block_scan_result(3, 50);
    let result_large = make_sample_block_scan_result(20, 200);

    c.bench_function("block_scan_result_json/serialize_empty", |b| {
        b.iter(|| serde_json::to_string(black_box(&result_empty)).unwrap())
    });

    c.bench_function("block_scan_result_json/serialize_3_outputs", |b| {
        b.iter(|| serde_json::to_string(black_box(&result_small)).unwrap())
    });

    c.bench_function("block_scan_result_json/serialize_20_outputs", |b| {
        b.iter(|| serde_json::to_string(black_box(&result_large)).unwrap())
    });

    let json_small = serde_json::to_string(&result_small).unwrap();
    let json_large = serde_json::to_string(&result_large).unwrap();

    c.bench_function("block_scan_result_json/deserialize_3_outputs", |b| {
        b.iter(|| {
            serde_json::from_str::<BlockScanResult>(black_box(&json_small)).unwrap()
        })
    });

    c.bench_function("block_scan_result_json/deserialize_20_outputs", |b| {
        b.iter(|| {
            serde_json::from_str::<BlockScanResult>(black_box(&json_large)).unwrap()
        })
    });
}

criterion_group!(benches, bench_extract_key_images, bench_block_scan_result_json);
criterion_main!(benches);
