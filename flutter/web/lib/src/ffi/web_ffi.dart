import 'dart:async';
import 'dart:js_interop';

// ---------------------------------------------------------------------------
// JS interop declarations for wasm-bindgen generated functions
// ---------------------------------------------------------------------------

// DartSignal senders (one per type)
@JS('wasm_bindgen.send_monero_test_request')
external void _sendMoneroTestRequest(JSString json);
@JS('wasm_bindgen.send_create_wallet_request')
external void _sendCreateWalletRequest(JSString json);
@JS('wasm_bindgen.send_start_sync_request')
external void _sendStartSyncRequest(JSString json);
@JS('wasm_bindgen.send_get_balance_request')
external void _sendGetBalanceRequest(JSString json);
@JS('wasm_bindgen.send_create_transaction_request')
external void _sendCreateTransactionRequest(JSString json);
@JS('wasm_bindgen.send_sweep_all_request')
external void _sendSweepAllRequest(JSString json);
@JS('wasm_bindgen.send_generate_seed_request')
external void _sendGenerateSeedRequest(JSString json);
@JS('wasm_bindgen.send_get_seed_birthday_request')
external void _sendGetSeedBirthdayRequest(JSString json);
@JS('wasm_bindgen.send_get_block_height_from_timestamp_request')
external void _sendGetBlockHeightFromTimestampRequest(JSString json);
@JS('wasm_bindgen.send_derive_address_request')
external void _sendDeriveAddressRequest(JSString json);
@JS('wasm_bindgen.send_derive_subaddress_request')
external void _sendDeriveSubaddressRequest(JSString json);
@JS('wasm_bindgen.send_derive_keys_request')
external void _sendDeriveKeysRequest(JSString json);
@JS('wasm_bindgen.send_scan_block_request')
external void _sendScanBlockRequest(JSString json);
@JS('wasm_bindgen.send_broadcast_transaction_request')
external void _sendBroadcastTransactionRequest(JSString json);
@JS('wasm_bindgen.send_query_daemon_height_request')
external void _sendQueryDaemonHeightRequest(JSString json);
@JS('wasm_bindgen.send_start_continuous_scan_request')
external void _sendStartContinuousScanRequest(JSString json);
@JS('wasm_bindgen.send_stop_scan_request')
external void _sendStopScanRequest(JSString json);
@JS('wasm_bindgen.send_mempool_scan_request')
external void _sendMempoolScanRequest(JSString json);
@JS('wasm_bindgen.send_generate_out_proof_request')
external void _sendGenerateOutProofRequest(JSString json);
@JS('wasm_bindgen.send_save_wallet_data_request')
external void _sendSaveWalletDataRequest(JSString json);
@JS('wasm_bindgen.send_load_wallet_data_request')
external void _sendLoadWalletDataRequest(JSString json);
@JS('wasm_bindgen.send_derive_encryption_key_request')
external void _sendDeriveEncryptionKeyRequest(JSString json);
@JS('wasm_bindgen.send_save_with_derived_key_request')
external void _sendSaveWithDerivedKeyRequest(JSString json);
@JS('wasm_bindgen.send_scan_block_multi_wallet_request')
external void _sendScanBlockMultiWalletRequest(JSString json);
@JS('wasm_bindgen.send_start_multi_wallet_scan_request')
external void _sendStartMultiWalletScanRequest(JSString json);
@JS('wasm_bindgen.send_restore_wallet_data_request')
external void _sendRestoreWalletDataRequest(JSString json);
@JS('wasm_bindgen.send_get_block_hashes_request')
external void _sendGetBlockHashesRequest(JSString json);
@JS('wasm_bindgen.send_get_pending_state_request')
external void _sendGetPendingStateRequest(JSString json);
@JS('wasm_bindgen.send_convert_bip39_to_legacy_request')
external void _sendConvertBip39ToLegacyRequest(JSString json);
@JS('wasm_bindgen.send_freeze_output_request')
external void _sendFreezeOutputRequest(JSString json);
@JS('wasm_bindgen.send_thaw_output_request')
external void _sendThawOutputRequest(JSString json);
@JS('wasm_bindgen.send_create_unsigned_transaction_request')
external void _sendCreateUnsignedTransactionRequest(JSString json);
@JS('wasm_bindgen.send_sign_unsigned_transaction_request')
external void _sendSignUnsignedTransactionRequest(JSString json);
@JS('wasm_bindgen.send_export_key_images_request')
external void _sendExportKeyImagesRequest(JSString json);
@JS('wasm_bindgen.send_import_key_images_request')
external void _sendImportKeyImagesRequest(JSString json);

// Rust→Dart callback registration
@JS('wasm_bindgen.register_rust_signal_callback')
external void _registerRustSignalCallback(JSFunction callback);

// WASM runtime
@JS('wasm_bindgen.start_rust_runtime')
external JSPromise _startRustRuntime();

// WASM module loader
@JS('wasm_bindgen.default')
external JSPromise _initWasm(JSString wasmUrl);

// ---------------------------------------------------------------------------
// Dispatch map: snake_name → JS interop function
// ---------------------------------------------------------------------------

final _senders = <String, void Function(JSString)>{
  'monero_test_request': (j) => _sendMoneroTestRequest(j),
  'create_wallet_request': (j) => _sendCreateWalletRequest(j),
  'start_sync_request': (j) => _sendStartSyncRequest(j),
  'get_balance_request': (j) => _sendGetBalanceRequest(j),
  'create_transaction_request': (j) => _sendCreateTransactionRequest(j),
  'sweep_all_request': (j) => _sendSweepAllRequest(j),
  'generate_seed_request': (j) => _sendGenerateSeedRequest(j),
  'get_seed_birthday_request': (j) => _sendGetSeedBirthdayRequest(j),
  'get_block_height_from_timestamp_request': (j) => _sendGetBlockHeightFromTimestampRequest(j),
  'derive_address_request': (j) => _sendDeriveAddressRequest(j),
  'derive_subaddress_request': (j) => _sendDeriveSubaddressRequest(j),
  'derive_keys_request': (j) => _sendDeriveKeysRequest(j),
  'scan_block_request': (j) => _sendScanBlockRequest(j),
  'broadcast_transaction_request': (j) => _sendBroadcastTransactionRequest(j),
  'query_daemon_height_request': (j) => _sendQueryDaemonHeightRequest(j),
  'start_continuous_scan_request': (j) => _sendStartContinuousScanRequest(j),
  'stop_scan_request': (j) => _sendStopScanRequest(j),
  'mempool_scan_request': (j) => _sendMempoolScanRequest(j),
  'generate_out_proof_request': (j) => _sendGenerateOutProofRequest(j),
  'save_wallet_data_request': (j) => _sendSaveWalletDataRequest(j),
  'load_wallet_data_request': (j) => _sendLoadWalletDataRequest(j),
  'derive_encryption_key_request': (j) => _sendDeriveEncryptionKeyRequest(j),
  'save_with_derived_key_request': (j) => _sendSaveWithDerivedKeyRequest(j),
  'scan_block_multi_wallet_request': (j) => _sendScanBlockMultiWalletRequest(j),
  'start_multi_wallet_scan_request': (j) => _sendStartMultiWalletScanRequest(j),
  'restore_wallet_data_request': (j) => _sendRestoreWalletDataRequest(j),
  'get_block_hashes_request': (j) => _sendGetBlockHashesRequest(j),
  'get_pending_state_request': (j) => _sendGetPendingStateRequest(j),
  'convert_bip39_to_legacy_request': (j) => _sendConvertBip39ToLegacyRequest(j),
  'freeze_output_request': (j) => _sendFreezeOutputRequest(j),
  'thaw_output_request': (j) => _sendThawOutputRequest(j),
  'create_unsigned_transaction_request': (j) => _sendCreateUnsignedTransactionRequest(j),
  'sign_unsigned_transaction_request': (j) => _sendSignUnsignedTransactionRequest(j),
  'export_key_images_request': (j) => _sendExportKeyImagesRequest(j),
  'import_key_images_request': (j) => _sendImportKeyImagesRequest(j),
};

/// Send a DartSignal to Rust as JSON via the named wasm-bindgen function.
void sendDartSignalJson(String snakeName, String json) {
  final sender = _senders[snakeName];
  if (sender != null) {
    sender(json.toJS);
  } else {
    throw StateError('Unknown DartSignal: $snakeName');
  }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the WASM module and start the Rust runtime.
/// [handlers] maps RustSignal type names to their JSON handler functions.
Future<void> initializeBareFfi(
    Map<String, void Function(String json)> handlers) async {
  // 1. Load the WASM module
  await _initWasm('pkg/hub_bg.wasm'.toJS).toDart;

  // 2. Register the Rust → Dart callback
  void onRustSignal(JSString type, JSString json) {
    final typeName = type.toDart;
    final jsonStr = json.toDart;
    final handler = handlers[typeName];
    if (handler != null) {
      handler(jsonStr);
    }
  }

  _registerRustSignalCallback(onRustSignal.toJS);

  // 3. Start the Rust async runtime
  await _startRustRuntime().toDart;
}
