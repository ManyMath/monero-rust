/// Numeric signal IDs shared between Rust and Dart.
/// DartSignal = Dart->Rust (requests), RustSignal = Rust->Dart (responses).
///
/// IMPORTANT: These IDs must stay in sync with `hub_signal_ids.dart` on the Dart side.

// -- DartSignal IDs (Dart -> Rust) --
pub const MONERO_TEST_REQUEST: u32 = 1;
pub const CREATE_WALLET_REQUEST: u32 = 2;
pub const START_SYNC_REQUEST: u32 = 3;
pub const GET_BALANCE_REQUEST: u32 = 4;
pub const CREATE_TRANSACTION_REQUEST: u32 = 5;
pub const SWEEP_ALL_REQUEST: u32 = 6;
pub const GENERATE_SEED_REQUEST: u32 = 7;
pub const GET_SEED_BIRTHDAY_REQUEST: u32 = 8;
pub const GET_BLOCK_HEIGHT_FROM_TIMESTAMP_REQUEST: u32 = 9;
pub const DERIVE_ADDRESS_REQUEST: u32 = 10;
pub const DERIVE_SUBADDRESS_REQUEST: u32 = 11;
pub const DERIVE_KEYS_REQUEST: u32 = 12;
pub const SCAN_BLOCK_REQUEST: u32 = 13;
pub const BROADCAST_TRANSACTION_REQUEST: u32 = 14;
pub const QUERY_DAEMON_HEIGHT_REQUEST: u32 = 15;
pub const START_CONTINUOUS_SCAN_REQUEST: u32 = 16;
pub const STOP_SCAN_REQUEST: u32 = 17;
pub const MEMPOOL_SCAN_REQUEST: u32 = 18;
pub const GENERATE_OUT_PROOF_REQUEST: u32 = 19;
pub const SAVE_WALLET_DATA_REQUEST: u32 = 20;
pub const LOAD_WALLET_DATA_REQUEST: u32 = 21;
pub const DERIVE_ENCRYPTION_KEY_REQUEST: u32 = 22;
pub const SAVE_WITH_DERIVED_KEY_REQUEST: u32 = 23;
pub const SCAN_BLOCK_MULTI_WALLET_REQUEST: u32 = 24;
pub const START_MULTI_WALLET_SCAN_REQUEST: u32 = 25;
pub const RESTORE_WALLET_DATA_REQUEST: u32 = 26;
pub const GET_BLOCK_HASHES_REQUEST: u32 = 27;
pub const GET_PENDING_STATE_REQUEST: u32 = 28;
pub const CONVERT_BIP39_TO_LEGACY_REQUEST: u32 = 29;
pub const FREEZE_OUTPUT_REQUEST: u32 = 30;
pub const THAW_OUTPUT_REQUEST: u32 = 31;
pub const CREATE_UNSIGNED_TRANSACTION_REQUEST: u32 = 32;
pub const SIGN_UNSIGNED_TRANSACTION_REQUEST: u32 = 33;
pub const EXPORT_KEY_IMAGES_REQUEST: u32 = 34;
pub const IMPORT_KEY_IMAGES_REQUEST: u32 = 35;
pub const IMPORT_KEYS_FILE_REQUEST: u32 = 36;
pub const EXPORT_KEYS_FILE_REQUEST: u32 = 37;
pub const START_UR_ENCODER_REQUEST: u32 = 38;
pub const STOP_UR_ENCODER_REQUEST: u32 = 39;
pub const UR_DECODE_FRAME_REQUEST: u32 = 40;
pub const RESET_UR_DECODER_REQUEST: u32 = 41;
pub const EXTRACT_SIGNED_TXSET_REQUEST: u32 = 42;
pub const INSPECT_UNSIGNED_TXSET_REQUEST: u32 = 43;

// -- RustSignal IDs (Rust -> Dart) --
pub const MONERO_TEST_RESPONSE: u32 = 101;
pub const WALLET_CREATED_RESPONSE: u32 = 102;
pub const SYNC_PROGRESS_RESPONSE: u32 = 103;
pub const BALANCE_RESPONSE: u32 = 104;
pub const TRANSACTION_CREATED_RESPONSE: u32 = 105;
pub const SEED_GENERATED_RESPONSE: u32 = 106;
pub const SEED_BIRTHDAY_RESPONSE: u32 = 107;
pub const BLOCK_HEIGHT_FROM_TIMESTAMP_RESPONSE: u32 = 108;
pub const ADDRESS_DERIVED_RESPONSE: u32 = 109;
pub const SUBADDRESS_DERIVED_RESPONSE: u32 = 110;
pub const KEYS_DERIVED_RESPONSE: u32 = 111;
pub const BLOCK_SCAN_RESPONSE: u32 = 112;
pub const TRANSACTION_BROADCAST_RESPONSE: u32 = 113;
pub const DAEMON_HEIGHT_RESPONSE: u32 = 114;
pub const SPENT_STATUS_UPDATED_RESPONSE: u32 = 115;
pub const MEMPOOL_SCAN_RESPONSE: u32 = 116;
pub const OUT_PROOF_GENERATED_RESPONSE: u32 = 117;
pub const WALLET_DATA_SAVED_RESPONSE: u32 = 118;
pub const WALLET_DATA_LOADED_RESPONSE: u32 = 119;
pub const ENCRYPTION_KEY_DERIVED_RESPONSE: u32 = 120;
pub const MULTI_WALLET_SCAN_RESPONSE: u32 = 121;
pub const BLOCK_HASHES_RESPONSE: u32 = 122;
pub const PENDING_STATE_RESPONSE: u32 = 123;
pub const REORG_DETECTED_RESPONSE: u32 = 124;
pub const DOUBLE_SPEND_DETECTED_RESPONSE: u32 = 125;
pub const BIP39_LEGACY_SEED_RESPONSE: u32 = 126;
pub const FREEZE_THAW_RESPONSE: u32 = 127;
pub const TRANSACTION_STATUS_UPDATE: u32 = 128;
pub const UNSIGNED_TRANSACTION_CREATED_RESPONSE: u32 = 129;
pub const TRANSACTION_SIGNED_OFFLINE_RESPONSE: u32 = 130;
pub const KEY_IMAGES_EXPORTED_RESPONSE: u32 = 131;
pub const KEY_IMAGES_IMPORTED_RESPONSE: u32 = 132;
pub const IMPORT_KEYS_FILE_RESPONSE: u32 = 133;
pub const EXPORT_KEYS_FILE_RESPONSE: u32 = 134;
pub const QR_FRAME_RESPONSE: u32 = 135;
pub const UR_DECODE_PROGRESS_RESPONSE: u32 = 136;
pub const UR_DECODE_COMPLETE_RESPONSE: u32 = 137;
pub const SIGNED_TXSET_EXTRACTED_RESPONSE: u32 = 138;
pub const UNSIGNED_TXSET_INSPECTED_RESPONSE: u32 = 139;

/// Map a RustSignal type name to its numeric ID for native FFI dispatch.
///
/// Used by `SendToDart::send_signal_to_dart()` on native to convert the
/// compile-time type name into the numeric signal_id expected by the
/// C FFI callback.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn rust_signal_id_for_name(name: &str) -> u32 {
    match name {
        "MoneroTestResponse" => MONERO_TEST_RESPONSE,
        "WalletCreatedResponse" => WALLET_CREATED_RESPONSE,
        "SyncProgressResponse" => SYNC_PROGRESS_RESPONSE,
        "BalanceResponse" => BALANCE_RESPONSE,
        "TransactionCreatedResponse" => TRANSACTION_CREATED_RESPONSE,
        "SeedGeneratedResponse" => SEED_GENERATED_RESPONSE,
        "SeedBirthdayResponse" => SEED_BIRTHDAY_RESPONSE,
        "BlockHeightFromTimestampResponse" => BLOCK_HEIGHT_FROM_TIMESTAMP_RESPONSE,
        "AddressDerivedResponse" => ADDRESS_DERIVED_RESPONSE,
        "SubaddressDerivedResponse" => SUBADDRESS_DERIVED_RESPONSE,
        "KeysDerivedResponse" => KEYS_DERIVED_RESPONSE,
        "BlockScanResponse" => BLOCK_SCAN_RESPONSE,
        "TransactionBroadcastResponse" => TRANSACTION_BROADCAST_RESPONSE,
        "DaemonHeightResponse" => DAEMON_HEIGHT_RESPONSE,
        "SpentStatusUpdatedResponse" => SPENT_STATUS_UPDATED_RESPONSE,
        "MempoolScanResponse" => MEMPOOL_SCAN_RESPONSE,
        "OutProofGeneratedResponse" => OUT_PROOF_GENERATED_RESPONSE,
        "WalletDataSavedResponse" => WALLET_DATA_SAVED_RESPONSE,
        "WalletDataLoadedResponse" => WALLET_DATA_LOADED_RESPONSE,
        "EncryptionKeyDerivedResponse" => ENCRYPTION_KEY_DERIVED_RESPONSE,
        "MultiWalletScanResponse" => MULTI_WALLET_SCAN_RESPONSE,
        "BlockHashesResponse" => BLOCK_HASHES_RESPONSE,
        "PendingStateResponse" => PENDING_STATE_RESPONSE,
        "ReorgDetectedResponse" => REORG_DETECTED_RESPONSE,
        "DoubleSpendDetectedResponse" => DOUBLE_SPEND_DETECTED_RESPONSE,
        "Bip39LegacySeedResponse" => BIP39_LEGACY_SEED_RESPONSE,
        "FreezeThawResponse" => FREEZE_THAW_RESPONSE,
        "TransactionStatusUpdate" => TRANSACTION_STATUS_UPDATE,
        "UnsignedTransactionCreatedResponse" => UNSIGNED_TRANSACTION_CREATED_RESPONSE,
        "TransactionSignedOfflineResponse" => TRANSACTION_SIGNED_OFFLINE_RESPONSE,
        "SignedTxSetExtractedResponse" => SIGNED_TXSET_EXTRACTED_RESPONSE,
        "UnsignedTxSetInspectedResponse" => UNSIGNED_TXSET_INSPECTED_RESPONSE,
        "KeyImagesExportedResponse" => KEY_IMAGES_EXPORTED_RESPONSE,
        "KeyImagesImportedResponse" => KEY_IMAGES_IMPORTED_RESPONSE,
        "ImportKeysFileResponse" => IMPORT_KEYS_FILE_RESPONSE,
        "ExportKeysFileResponse" => EXPORT_KEYS_FILE_RESPONSE,
        "QrFrameResponse" => QR_FRAME_RESPONSE,
        "UrDecodeProgressResponse" => UR_DECODE_PROGRESS_RESPONSE,
        "UrDecodeCompleteResponse" => UR_DECODE_COMPLETE_RESPONSE,
        _ => {
            eprintln!("unknown RustSignal type name: {name}");
            0
        }
    }
}

/// Route an incoming DartSignal to the appropriate channel.
///
/// Converts the raw byte payload to a UTF-8 JSON string, then dispatches
/// to the matching `send_<signal>()` function generated by the `dart_signal!`
/// macro. Each send function deserializes the JSON into the signal struct
/// and pushes it into the corresponding `tokio::sync::mpsc` channel where
/// the actor's `listen_to_*` task is waiting.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn route_dart_signal(signal_id: u32, data: Vec<u8>) {
    let json_str = match std::str::from_utf8(&data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("route_dart_signal: invalid UTF-8 for signal_id {signal_id}: {e}");
            return;
        }
    };

    let result: Result<(), String> = match signal_id {
        MONERO_TEST_REQUEST => crate::ffi_web::send_monero_test_request(json_str),
        CREATE_WALLET_REQUEST => crate::ffi_web::send_create_wallet_request(json_str),
        START_SYNC_REQUEST => crate::ffi_web::send_start_sync_request(json_str),
        GET_BALANCE_REQUEST => crate::ffi_web::send_get_balance_request(json_str),
        CREATE_TRANSACTION_REQUEST => crate::ffi_web::send_create_transaction_request(json_str),
        SWEEP_ALL_REQUEST => crate::ffi_web::send_sweep_all_request(json_str),
        GENERATE_SEED_REQUEST => crate::ffi_web::send_generate_seed_request(json_str),
        GET_SEED_BIRTHDAY_REQUEST => crate::ffi_web::send_get_seed_birthday_request(json_str),
        GET_BLOCK_HEIGHT_FROM_TIMESTAMP_REQUEST => {
            crate::ffi_web::send_get_block_height_from_timestamp_request(json_str)
        }
        DERIVE_ADDRESS_REQUEST => crate::ffi_web::send_derive_address_request(json_str),
        DERIVE_SUBADDRESS_REQUEST => crate::ffi_web::send_derive_subaddress_request(json_str),
        DERIVE_KEYS_REQUEST => crate::ffi_web::send_derive_keys_request(json_str),
        SCAN_BLOCK_REQUEST => crate::ffi_web::send_scan_block_request(json_str),
        BROADCAST_TRANSACTION_REQUEST => {
            crate::ffi_web::send_broadcast_transaction_request(json_str)
        }
        QUERY_DAEMON_HEIGHT_REQUEST => crate::ffi_web::send_query_daemon_height_request(json_str),
        START_CONTINUOUS_SCAN_REQUEST => {
            crate::ffi_web::send_start_continuous_scan_request(json_str)
        }
        STOP_SCAN_REQUEST => crate::ffi_web::send_stop_scan_request(json_str),
        MEMPOOL_SCAN_REQUEST => crate::ffi_web::send_mempool_scan_request(json_str),
        GENERATE_OUT_PROOF_REQUEST => crate::ffi_web::send_generate_out_proof_request(json_str),
        SAVE_WALLET_DATA_REQUEST => crate::ffi_web::send_save_wallet_data_request(json_str),
        LOAD_WALLET_DATA_REQUEST => crate::ffi_web::send_load_wallet_data_request(json_str),
        DERIVE_ENCRYPTION_KEY_REQUEST => {
            crate::ffi_web::send_derive_encryption_key_request(json_str)
        }
        SAVE_WITH_DERIVED_KEY_REQUEST => {
            crate::ffi_web::send_save_with_derived_key_request(json_str)
        }
        SCAN_BLOCK_MULTI_WALLET_REQUEST => {
            crate::ffi_web::send_scan_block_multi_wallet_request(json_str)
        }
        START_MULTI_WALLET_SCAN_REQUEST => {
            crate::ffi_web::send_start_multi_wallet_scan_request(json_str)
        }
        RESTORE_WALLET_DATA_REQUEST => crate::ffi_web::send_restore_wallet_data_request(json_str),
        GET_BLOCK_HASHES_REQUEST => crate::ffi_web::send_get_block_hashes_request(json_str),
        GET_PENDING_STATE_REQUEST => crate::ffi_web::send_get_pending_state_request(json_str),
        CONVERT_BIP39_TO_LEGACY_REQUEST => {
            crate::ffi_web::send_convert_bip39_to_legacy_request(json_str)
        }
        FREEZE_OUTPUT_REQUEST => crate::ffi_web::send_freeze_output_request(json_str),
        THAW_OUTPUT_REQUEST => crate::ffi_web::send_thaw_output_request(json_str),
        CREATE_UNSIGNED_TRANSACTION_REQUEST => {
            crate::ffi_web::send_create_unsigned_transaction_request(json_str)
        }
        SIGN_UNSIGNED_TRANSACTION_REQUEST => {
            crate::ffi_web::send_sign_unsigned_transaction_request(json_str)
        }
        INSPECT_UNSIGNED_TXSET_REQUEST => {
            crate::ffi_web::send_inspect_unsigned_txset_request(json_str)
        }
        EXTRACT_SIGNED_TXSET_REQUEST => crate::ffi_web::send_extract_signed_txset_request(json_str),
        EXPORT_KEY_IMAGES_REQUEST => crate::ffi_web::send_export_key_images_request(json_str),
        IMPORT_KEY_IMAGES_REQUEST => crate::ffi_web::send_import_key_images_request(json_str),
        IMPORT_KEYS_FILE_REQUEST => crate::ffi_web::send_import_keys_file_request(json_str),
        EXPORT_KEYS_FILE_REQUEST => crate::ffi_web::send_export_keys_file_request(json_str),
        START_UR_ENCODER_REQUEST => crate::ffi_web::send_start_ur_encoder_request(json_str),
        STOP_UR_ENCODER_REQUEST => crate::ffi_web::send_stop_ur_encoder_request(json_str),
        UR_DECODE_FRAME_REQUEST => crate::ffi_web::send_ur_decode_frame_request(json_str),
        RESET_UR_DECODER_REQUEST => crate::ffi_web::send_reset_ur_decoder_request(json_str),
        _ => {
            eprintln!("route_dart_signal: unknown signal_id: {signal_id}");
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("route_dart_signal: failed to dispatch signal_id {signal_id}: {e}");
    }
}
