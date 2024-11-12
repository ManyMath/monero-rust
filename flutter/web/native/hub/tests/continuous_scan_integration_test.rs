mod mock_scanner;

use mock_scanner::{MockRpc, MockOutput};

/// Simulates a full continuous scan workflow
#[tokio::test]
async fn test_continuous_scan_complete_workflow() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = rpc.get_height().unwrap();

    let mut current_height = start_height;
    let mut all_outputs = Vec::new();
    let mut scan_count = 0;

    // Simulate continuous scanning
    while current_height <= target_height {
        let result = rpc.scan_block(current_height, "test_seed").unwrap();

        all_outputs.extend(result.outputs);
        scan_count += 1;
        current_height += 1;

        // Simulate progress tracking
        let progress = (current_height - start_height) as f64 / (target_height - start_height) as f64;
        if scan_count % 20 == 0 {
            println!("Progress: {:.1}% ({}/{})", progress * 100.0, current_height, target_height);
        }
    }

    assert_eq!(current_height, target_height + 1);
    assert_eq!(scan_count, 101); // 1000 to 1100 inclusive

    // Should have found outputs at heights divisible by 15
    // In range 1000-1100: 1005, 1020, 1035, 1050, 1065, 1080, 1095
    assert!(all_outputs.len() >= 6);
}

#[tokio::test]
async fn test_continuous_scan_with_interruption() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = 1100;

    let mut current_height = start_height;
    let mut scan_stopped = false;
    let stop_at_height = 1050;

    // Simulate continuous scanning with stop
    while current_height <= target_height && !scan_stopped {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;

        if current_height >= stop_at_height {
            scan_stopped = true;
        }
    }

    assert_eq!(current_height, stop_at_height);
    assert!(scan_stopped);
    assert!(current_height < target_height);
}

#[tokio::test]
async fn test_continuous_scan_with_network_failure() {
    let rpc = MockRpc::new(1100).with_failure(1050);
    let start_height = 1000;
    let target_height = 1100;

    let mut current_height = start_height;
    let mut error_occurred = false;
    let mut error_message = String::new();

    // Simulate continuous scanning with error handling
    while current_height <= target_height && !error_occurred {
        match rpc.scan_block(current_height, "test_seed") {
            Ok(_result) => {
                current_height += 1;
            }
            Err(e) => {
                error_occurred = true;
                error_message = e;
                break;
            }
        }
    }

    assert!(error_occurred);
    assert_eq!(current_height, 1050);
    assert!(error_message.contains("Failed to get block at height 1050"));
}

#[tokio::test]
async fn test_continuous_scan_progress_updates() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = 1100;

    struct ProgressUpdate {
        current: u64,
        target: u64,
        is_synced: bool,
    }

    let mut progress_updates = Vec::new();
    let mut current_height = start_height;

    while current_height <= target_height {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;

        // Record progress every 10 blocks
        if current_height % 10 == 0 || current_height == target_height {
            progress_updates.push(ProgressUpdate {
                current: current_height,
                target: target_height,
                is_synced: current_height >= target_height,
            });
        }
    }

    // Should have multiple progress updates
    assert!(progress_updates.len() >= 10);

    // Last update should indicate sync complete
    let last_update = progress_updates.last().unwrap();
    assert!(last_update.is_synced);
    assert_eq!(last_update.current, target_height);
}

#[tokio::test]
async fn test_continuous_scan_output_accumulation() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = 1100;

    let mut current_height = start_height;
    let mut all_outputs = Vec::new();
    let mut outputs_by_height: std::collections::HashMap<u64, Vec<MockOutput>> =
        std::collections::HashMap::new();

    while current_height <= target_height {
        let result = rpc.scan_block(current_height, "test_seed").unwrap();

        if !result.outputs.is_empty() {
            outputs_by_height.insert(current_height, result.outputs.clone());
            all_outputs.extend(result.outputs);
        }

        current_height += 1;
    }

    // Verify outputs were found at expected heights
    for (height, outputs) in &outputs_by_height {
        assert_eq!(height % 15, 0, "Outputs should only be at heights divisible by 15");
        assert!(!outputs.is_empty());
    }

    // All outputs should sum correctly
    let total_amount: u64 = all_outputs.iter().map(|o| o.amount).sum();
    assert_eq!(total_amount, all_outputs.len() as u64 * 1000000000000);
}

#[tokio::test]
async fn test_continuous_scan_daemon_height_update() {
    let mut rpc = MockRpc::new(1100);
    let start_height = 1000;

    let mut current_height = start_height;
    let mut initial_target = rpc.get_height().unwrap();

    // Scan first 50 blocks
    for _ in 0..50 {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;
    }

    assert_eq!(current_height, 1050);

    // Simulate daemon height increasing
    rpc.daemon_height = 1200;
    let new_target = rpc.get_height().unwrap();

    assert_eq!(initial_target, 1100);
    assert_eq!(new_target, 1200);
    assert!(current_height < new_target);
}

#[tokio::test]
async fn test_scan_synced_state_detection() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = 1100;

    let mut current_height = start_height;
    let mut is_synced = false;

    while current_height <= target_height {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;

        // Check if synced
        if current_height >= target_height {
            is_synced = true;
        }
    }

    assert!(is_synced);
    assert_eq!(current_height, target_height + 1);
}

#[tokio::test]
async fn test_empty_scan_range() {
    let rpc = MockRpc::new(1100);
    let start_height = 1100;
    let target_height = 1100;

    let mut current_height = start_height;
    let mut scan_count = 0;

    while current_height <= target_height {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;
        scan_count += 1;
    }

    // Should scan exactly 1 block (height 1100)
    assert_eq!(scan_count, 1);
    assert_eq!(current_height, target_height + 1);
}

#[tokio::test]
async fn test_scan_performance_timing() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = 1020; // Small range for timing test

    let start_time = std::time::Instant::now();
    let mut current_height = start_height;

    while current_height <= target_height {
        let _result = rpc.scan_block(current_height, "test_seed").unwrap();
        current_height += 1;
    }

    let duration = start_time.elapsed();

    // Mock scanning should be very fast (< 100ms for 21 blocks)
    assert!(duration.as_millis() < 100, "Scanning took too long: {:?}", duration);
    assert_eq!(current_height, target_height + 1);
}
