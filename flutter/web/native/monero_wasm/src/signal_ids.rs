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
        "KeyImagesExportedResponse" => KEY_IMAGES_EXPORTED_RESPONSE,
        "KeyImagesImportedResponse" => KEY_IMAGES_IMPORTED_RESPONSE,
        _ => {
            eprintln!("unknown RustSignal type name: {name}");
            0
        }
    }
}

/// Route an incoming DartSignal to the appropriate channel.
///
/// This is the native-FFI equivalent of the wasm-bindgen signal routing.
/// For now this is a stub — full routing is implemented in commit 2.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn route_dart_signal(_signal_id: u32, _data: Vec<u8>) {
    // TODO: match on signal_id, JSON-deserialize, and send to actors.
}
