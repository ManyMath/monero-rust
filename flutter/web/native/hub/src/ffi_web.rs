use std::cell::RefCell;
use wasm_bindgen::prelude::*;

use crate::signals::*;

// ---------------------------------------------------------------------------
// Macro: generate DartSignal channel infrastructure
// ---------------------------------------------------------------------------
//
// For each DartSignal type, generates:
//   - A thread_local UnboundedSender
//   - A #[wasm_bindgen] export `send_<snake_name>(json)` that deserializes and sends
//   - A `get_<snake_name>_receiver()` function for actors to call during init

macro_rules! dart_signal {
    ($type:ty, $snake:ident) => {
        paste::paste! {
            thread_local! {
                static [<$snake:upper _SENDER>]: RefCell<Option<tokio::sync::mpsc::UnboundedSender<$type>>>
                    = RefCell::new(None);
            }

            #[wasm_bindgen]
            pub fn [<send_ $snake>](json: &str) -> Result<(), JsValue> {
                let msg: $type = serde_json::from_str(json)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                [<$snake:upper _SENDER>].with(|s| {
                    if let Some(tx) = s.borrow().as_ref() {
                        tx.send(msg).map_err(|_| JsValue::from_str("channel closed"))
                    } else {
                        Err(JsValue::from_str("not initialized"))
                    }
                })
            }

            pub fn [<get_ $snake _receiver>]() -> tokio::sync::mpsc::UnboundedReceiver<$type> {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                [<$snake:upper _SENDER>].with(|s| { *s.borrow_mut() = Some(tx); });
                rx
            }
        }
    };
}

// All 35 DartSignal types
dart_signal!(MoneroTestRequest, monero_test_request);
dart_signal!(CreateWalletRequest, create_wallet_request);
dart_signal!(StartSyncRequest, start_sync_request);
dart_signal!(GetBalanceRequest, get_balance_request);
dart_signal!(CreateTransactionRequest, create_transaction_request);
dart_signal!(SweepAllRequest, sweep_all_request);
dart_signal!(GenerateSeedRequest, generate_seed_request);
dart_signal!(GetSeedBirthdayRequest, get_seed_birthday_request);
dart_signal!(GetBlockHeightFromTimestampRequest, get_block_height_from_timestamp_request);
dart_signal!(DeriveAddressRequest, derive_address_request);
dart_signal!(DeriveSubaddressRequest, derive_subaddress_request);
dart_signal!(DeriveKeysRequest, derive_keys_request);
dart_signal!(ScanBlockRequest, scan_block_request);
dart_signal!(BroadcastTransactionRequest, broadcast_transaction_request);
dart_signal!(QueryDaemonHeightRequest, query_daemon_height_request);
dart_signal!(StartContinuousScanRequest, start_continuous_scan_request);
dart_signal!(StopScanRequest, stop_scan_request);
dart_signal!(MempoolScanRequest, mempool_scan_request);
dart_signal!(GenerateOutProofRequest, generate_out_proof_request);
dart_signal!(SaveWalletDataRequest, save_wallet_data_request);
dart_signal!(LoadWalletDataRequest, load_wallet_data_request);
dart_signal!(DeriveEncryptionKeyRequest, derive_encryption_key_request);
dart_signal!(SaveWithDerivedKeyRequest, save_with_derived_key_request);
dart_signal!(ScanBlockMultiWalletRequest, scan_block_multi_wallet_request);
dart_signal!(StartMultiWalletScanRequest, start_multi_wallet_scan_request);
dart_signal!(RestoreWalletDataRequest, restore_wallet_data_request);
dart_signal!(GetBlockHashesRequest, get_block_hashes_request);
dart_signal!(GetPendingStateRequest, get_pending_state_request);
dart_signal!(ConvertBip39ToLegacyRequest, convert_bip39_to_legacy_request);
dart_signal!(FreezeOutputRequest, freeze_output_request);
dart_signal!(ThawOutputRequest, thaw_output_request);

// ---------------------------------------------------------------------------
// Rust -> Dart callback
// ---------------------------------------------------------------------------

thread_local! {
    static DART_CALLBACK: RefCell<Option<js_sys::Function>> = RefCell::new(None);
}

#[wasm_bindgen]
pub fn register_rust_signal_callback(callback: js_sys::Function) {
    DART_CALLBACK.with(|c| { *c.borrow_mut() = Some(callback); });
}

fn send_to_dart_raw<T: serde::Serialize + ?Sized>(type_name: &str, msg: &T) {
    DART_CALLBACK.with(|c| {
        if let Some(cb) = c.borrow().as_ref() {
            match serde_json::to_string(msg) {
                Ok(json) => {
                    let _ = cb.call2(
                        &JsValue::NULL,
                        &JsValue::from_str(type_name),
                        &JsValue::from_str(&json),
                    );
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &JsValue::from_str(&format!("serialize error for {}: {}", type_name, e))
                    );
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// SendToDart trait — replaces rinf's .send_signal_to_dart()
// ---------------------------------------------------------------------------

pub trait SendToDart: serde::Serialize {
    const TYPE_NAME: &'static str;

    fn send_signal_to_dart(&self) {
        send_to_dart_raw(Self::TYPE_NAME, self);
    }
}

macro_rules! impl_send_to_dart {
    ($($type:ty => $name:expr),* $(,)?) => {
        $(
            impl SendToDart for $type {
                const TYPE_NAME: &'static str = $name;
            }
        )*
    };
}

impl_send_to_dart! {
    MoneroTestResponse => "MoneroTestResponse",
    WalletCreatedResponse => "WalletCreatedResponse",
    SyncProgressResponse => "SyncProgressResponse",
    BalanceResponse => "BalanceResponse",
    TransactionCreatedResponse => "TransactionCreatedResponse",
    SeedGeneratedResponse => "SeedGeneratedResponse",
    SeedBirthdayResponse => "SeedBirthdayResponse",
    BlockHeightFromTimestampResponse => "BlockHeightFromTimestampResponse",
    AddressDerivedResponse => "AddressDerivedResponse",
    SubaddressDerivedResponse => "SubaddressDerivedResponse",
    KeysDerivedResponse => "KeysDerivedResponse",
    BlockScanResponse => "BlockScanResponse",
    TransactionBroadcastResponse => "TransactionBroadcastResponse",
    DaemonHeightResponse => "DaemonHeightResponse",
    SpentStatusUpdatedResponse => "SpentStatusUpdatedResponse",
    MempoolScanResponse => "MempoolScanResponse",
    OutProofGeneratedResponse => "OutProofGeneratedResponse",
    WalletDataSavedResponse => "WalletDataSavedResponse",
    WalletDataLoadedResponse => "WalletDataLoadedResponse",
    EncryptionKeyDerivedResponse => "EncryptionKeyDerivedResponse",
    MultiWalletScanResponse => "MultiWalletScanResponse",
    BlockHashesResponse => "BlockHashesResponse",
    PendingStateResponse => "PendingStateResponse",
    ReorgDetectedResponse => "ReorgDetectedResponse",
    DoubleSpendDetectedResponse => "DoubleSpendDetectedResponse",
    Bip39LegacySeedResponse => "Bip39LegacySeedResponse",
    FreezeThawResponse => "FreezeThawResponse",
    TransactionStatusUpdate => "TransactionStatusUpdate",
}

// ---------------------------------------------------------------------------
// WASM entry point
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub async fn start_rust_runtime() {
    tokio_with_wasm::alias::spawn(crate::actors::create_actors());
}
