use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use crate::ffi_web::SendToDart;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

/// Cross-platform spawn_local: uses wasm_bindgen_futures on WASM,
/// tokio::task::spawn_local on native (requires a LocalSet or
/// current_thread runtime).
#[cfg(target_arch = "wasm32")]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    tokio::task::spawn_local(f);
}

/// Get current time in seconds (WASM-safe).
pub(crate) fn current_time_secs() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Pre-convert BIP39 12-word seeds to legacy format using the given passphrase
/// and account index. Non-BIP39 seeds pass through unchanged.
pub(crate) fn pre_resolve_bip39(seed: &str, passphrase: &str, account_index: u32) -> Result<String, String> {
    if seed.split_whitespace().count() == 12 {
        monero_rust::bip39_to_legacy_mnemonic(seed, passphrase, account_index)
    } else {
        Ok(seed.to_string())
    }
}

// ---------------------------------------------------------------------------
// Double-buffered prefetch infrastructure
// ---------------------------------------------------------------------------

struct PrefetchSlot {
    generation: u64,
    height: u64,
    data: monero_rust::FetchedBlocks,
}

thread_local! {
    static PREFETCH_SLOT: RefCell<Option<PrefetchSlot>> = RefCell::new(None);
    static PREFETCH_GENERATION: Cell<u64> = Cell::new(0);
    static SCANNER_CACHE: RefCell<Option<(u64, monero_rust::CachedScanner)>> = RefCell::new(None);
    static MULTI_SCANNER_CACHE: RefCell<Option<(u64, monero_rust::CachedScanners)>> = RefCell::new(None);
}

/// Increment the generation counter and clear any buffered prefetch.
/// Returns the new generation value.
fn bump_generation() -> u64 {
    PREFETCH_GENERATION.with(|g| {
        let next = g.get() + 1;
        g.set(next);
        next
    });
    PREFETCH_SLOT.with(|s| { s.borrow_mut().take(); });
    SCANNER_CACHE.with(|s| { s.borrow_mut().take(); });
    MULTI_SCANNER_CACHE.with(|s| { s.borrow_mut().take(); });
    PREFETCH_GENERATION.with(|g| g.get())
}

/// Take prefetched data if it matches the current generation and expected height.
fn take_prefetch(generation: u64, height: u64) -> Option<monero_rust::FetchedBlocks> {
    PREFETCH_SLOT.with(|s| {
        let matches = {
            let slot = s.borrow();
            matches!(&*slot, Some(ps) if ps.generation == generation && ps.height == height)
        };
        if matches {
            s.borrow_mut().take().map(|ps| ps.data)
        } else {
            None
        }
    })
}

/// Store prefetched data, but only if the generation hasn't been bumped since
/// the fetch was launched.
fn store_prefetch(generation: u64, height: u64, data: monero_rust::FetchedBlocks) {
    PREFETCH_GENERATION.with(|g| {
        if g.get() == generation {
            PREFETCH_SLOT.with(|s| {
                *s.borrow_mut() = Some(PrefetchSlot {
                    generation,
                    height,
                    data,
                });
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Scanner cache infrastructure (keyed by generation)
// ---------------------------------------------------------------------------

fn take_scanner_cache(generation: u64) -> Option<monero_rust::CachedScanner> {
    SCANNER_CACHE.with(|s| {
        let matches = {
            let slot = s.borrow();
            matches!(&*slot, Some((g, _)) if *g == generation)
        };
        if matches {
            s.borrow_mut().take().map(|(_, cached)| cached)
        } else {
            None
        }
    })
}

fn store_scanner_cache(generation: u64, cached: monero_rust::CachedScanner) {
    PREFETCH_GENERATION.with(|g| {
        if g.get() == generation {
            SCANNER_CACHE.with(|s| {
                *s.borrow_mut() = Some((generation, cached));
            });
        }
    });
}

fn take_multi_scanner_cache(generation: u64) -> Option<monero_rust::CachedScanners> {
    MULTI_SCANNER_CACHE.with(|s| {
        let matches = {
            let slot = s.borrow();
            matches!(&*slot, Some((g, _)) if *g == generation)
        };
        if matches {
            s.borrow_mut().take().map(|(_, cached)| cached)
        } else {
            None
        }
    })
}

fn store_multi_scanner_cache(generation: u64, cached: monero_rust::CachedScanners) {
    PREFETCH_GENERATION.with(|g| {
        if g.get() == generation {
            MULTI_SCANNER_CACHE.with(|s| {
                *s.borrow_mut() = Some((generation, cached));
            });
        }
    });
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanType {
    None,
    SingleWallet,
    MultiWallet,
}

pub struct WalletActor {
    core_state: monero_rust::WalletState,
    address: String,
    seed: Option<String>,
    network: Option<String>,
    _owned_tasks: JoinSet<()>,
    // Shared scan state
    is_scanning: bool,
    active_scan_type: ScanType,
    // Single-wallet scan state
    scan_current_height: u64,
    scan_target_height: u64,
    scan_node_url: String,
    scan_seed: String,
    scan_passphrase: String,
    scan_network: String,
    scan_account_lookahead: u32,
    scan_subaddress_lookahead: u32,
    scan_accounts_to_scan: Option<Vec<u32>>,
    // Multi-wallet scan state
    multi_wallet_scan_current_height: u64,
    multi_wallet_scan_target_height: u64,
    multi_wallet_scan_node_url: String,
    multi_wallet_scan_wallets: Vec<WalletConfig>,
}

impl Actor for WalletActor {}

impl WalletActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_create_wallet(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_balance_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_test(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_generate_seed(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_get_seed_birthday(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_get_block_height_from_timestamp(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_derive_address(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_derive_subaddress(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_derive_keys(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_scan_block(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_query_daemon_height(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_start_continuous_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_stop_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_mempool_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_scan_block_multi_wallet());
        _owned_tasks.spawn(Self::listen_to_start_multi_wallet_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_restore_wallet_data(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_get_block_hashes(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_get_pending_state(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_convert_bip39_to_legacy(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_freeze_output(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_thaw_output(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_import_keys_file());

        WalletActor {
            core_state: monero_rust::WalletState::new(),
            address: String::new(),
            seed: None,
            network: None,
            _owned_tasks,
            is_scanning: false,
            active_scan_type: ScanType::None,
            scan_current_height: 0,
            scan_target_height: 0,
            scan_node_url: String::new(),
            scan_seed: String::new(),
            scan_passphrase: String::new(),
            scan_network: String::new(),
            scan_account_lookahead: 0,
            scan_subaddress_lookahead: 0,
            scan_accounts_to_scan: None,
            multi_wallet_scan_current_height: 0,
            multi_wallet_scan_target_height: 0,
            multi_wallet_scan_node_url: String::new(),
            multi_wallet_scan_wallets: Vec::new(),
        }
    }

    async fn listen_to_create_wallet(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_create_wallet_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr.notify(request).await;
        }
    }

    async fn listen_to_balance_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_get_balance_request_receiver();
        while let Some(_dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(GetBalanceRequest {}).await;
        }
    }

    async fn listen_to_test(_self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_monero_test_request_receiver();
        while let Some(_dart_msg) = receiver.recv().await {
            let result = monero_rust::test_integration();
            MoneroTestResponse { result }.send_signal_to_dart();
        }
    }

    async fn listen_to_generate_seed(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_generate_seed_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            match monero_rust::generate_seed(&request.seed_type) {
                Ok(seed) => {
                    let restore_height = if request.seed_type == "polyseed" {
                        monero_rust::seed_birthday(&seed)
                    } else {
                        None
                    };
                    SeedGeneratedResponse {
                        seed,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        restore_height,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    SeedGeneratedResponse {
                        seed: String::new(),
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        restore_height: None,
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_get_seed_birthday(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_get_seed_birthday_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    SeedBirthdayResponse {
                        birthday: None, success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            let birthday = monero_rust::seed_birthday(&resolved);
            SeedBirthdayResponse {
                birthday,
                success: true,
                error: None,
                error_code: None,
                error_hint: None,
                error_transient: None,
            }
            .send_signal_to_dart();
        }
    }

    async fn listen_to_get_block_height_from_timestamp(_self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_get_block_height_from_timestamp_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            match Self::find_block_height_for_timestamp(&request.node_url, request.timestamp).await {
                Ok(block_height) => {
                    BlockHeightFromTimestampResponse {
                        block_height,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                    BlockHeightFromTimestampResponse {
                        block_height: 0,
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    /// Binary search to find the block height closest to a given timestamp.
    /// Uses JSON-RPC methods (get_block_count, get_block_header_by_height)
    /// which are supported on the /json_rpc endpoint.
    async fn find_block_height_for_timestamp(node_url: &str, target_timestamp: u64) -> Result<u64, String> {
        // Ensure we're hitting the /json_rpc endpoint
        let rpc_url = if node_url.ends_with("/json_rpc") {
            node_url.to_string()
        } else {
            format!("{}/json_rpc", node_url.trim_end_matches('/'))
        };

        // Get current chain height via get_block_count (proper JSON-RPC method)
        let height_resp = Self::json_rpc_call(&rpc_url, "get_block_count", serde_json::json!({})).await?;
        let max_height = height_resp["count"].as_u64()
            .ok_or_else(|| "Invalid get_block_count response".to_string())?;

        let mut low = 1u64; // skip genesis
        let mut high = max_height.saturating_sub(1);
        let mut best_height = 0u64;
        let mut best_diff = u64::MAX;

        while low <= high {
            let mid = low + (high - low) / 2;

            let header_resp = Self::json_rpc_call(
                &rpc_url,
                "get_block_header_by_height",
                serde_json::json!({"height": mid}),
            ).await?;

            let block_timestamp = header_resp["block_header"]["timestamp"].as_u64()
                .ok_or_else(|| format!("No timestamp in block header at height {}", mid))?;

            let diff = block_timestamp.abs_diff(target_timestamp);
            if diff < best_diff {
                best_diff = diff;
                best_height = mid;
            }

            if block_timestamp < target_timestamp {
                low = mid + 1;
            } else if block_timestamp > target_timestamp {
                if mid == 0 { break; }
                high = mid - 1;
            } else {
                return Ok(mid);
            }
        }

        Ok(best_height)
    }

    /// Make a JSON-RPC call to a Monero daemon (WASM only; native uses reqwest).
    #[cfg(target_arch = "wasm32")]
    async fn json_rpc_call(url: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode, RequestCredentials, Response};

        let window = web_sys::window().ok_or("No window object")?;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": method,
            "params": params,
        });
        let body_str = serde_json::to_string(&body).map_err(|e| e.to_string())?;

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);
        opts.set_credentials(RequestCredentials::SameOrigin);
        opts.set_body(&wasm_bindgen::JsValue::from_str(&body_str));

        let request = Request::new_with_str_and_init(url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;
        request.headers().set("Content-Type", "application/json")
            .map_err(|e| format!("Header set failed: {:?}", e))?;

        let resp_val = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;
        let resp: Response = resp_val.dyn_into()
            .map_err(|e| format!("Invalid response: {:?}", e))?;

        let text_val = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
            .await
            .map_err(|e| format!("Read failed: {:?}", e))?;
        let text = text_val.as_string().ok_or("Response not a string")?;

        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

        if let Some(error) = json.get("error") {
            return Err(format!("RPC error: {}", error));
        }

        json.get("result").cloned().ok_or_else(|| "No 'result' in JSON-RPC response".to_string())
    }

    async fn listen_to_derive_address(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_derive_address_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    AddressDerivedResponse {
                        address: String::new(), success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            match monero_rust::derive_address(&resolved, &request.network, &request.passphrase) {
                Ok(address) => {
                    AddressDerivedResponse {
                        address,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    AddressDerivedResponse {
                        address: String::new(),
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_derive_subaddress(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_derive_subaddress_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    SubaddressDerivedResponse {
                        address: String::new(), success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            match monero_rust::derive_subaddress(
                &resolved,
                &request.network,
                request.account,
                request.address_index,
                &request.passphrase,
            ) {
                Ok(address) => {
                    SubaddressDerivedResponse {
                        address,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    SubaddressDerivedResponse {
                        address: String::new(),
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_derive_keys(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_derive_keys_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    KeysDerivedResponse {
                        address: String::new(), secret_spend_key: String::new(),
                        secret_view_key: String::new(), public_spend_key: String::new(),
                        public_view_key: String::new(), success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            match monero_rust::derive_keys(&resolved, &request.network, &request.passphrase) {
                Ok(keys) => {
                    KeysDerivedResponse {
                        address: keys.address,
                        secret_spend_key: keys.secret_spend_key,
                        secret_view_key: keys.secret_view_key,
                        public_spend_key: keys.public_spend_key,
                        public_view_key: keys.public_view_key,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    KeysDerivedResponse {
                        address: String::new(),
                        secret_spend_key: String::new(),
                        secret_view_key: String::new(),
                        public_spend_key: String::new(),
                        public_view_key: String::new(),
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_scan_block(mut self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_scan_block_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            if let Err(err) = monero_rust::error_codes::Network::parse(&request.network) {
                BlockScanResponse {
                    success: false, error: Some(err.message.clone()),
                    error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    block_height: request.block_height,
                    block_hash: String::new(), block_timestamp: 0, tx_count: 0,
                    outputs: Vec::new(), daemon_height: 0, spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(),
                }.send_signal_to_dart();
                continue;
            }

            let seed = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    BlockScanResponse {
                        success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                        block_height: request.block_height,
                        block_hash: String::new(), block_timestamp: 0, tx_count: 0,
                        outputs: Vec::new(), daemon_height: 0, spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            let network = request.network.clone();

            match monero_rust::scan_block_for_outputs_with_url(
                &request.node_url,
                request.block_height,
                &seed,
                &request.network,
                &request.passphrase,
            )
            .await
            {
                Ok(result) => {
                    let outputs: Vec<OwnedOutput> = result
                        .outputs
                        .iter()
                        .map(|o| o.into())
                        .collect();

                    let _ = self_addr
                        .notify(StoreOutputs {
                            seed,
                            network,
                            outputs: result.outputs.clone(),
                            daemon_height: result.daemon_height,
                            block_hashes: vec![(result.block_height, result.block_hash.clone())],
                        })
                        .await;

                    if !result.spent_key_images.is_empty() {
                        let _ = self_addr.notify(UpdateSpentStatus {
                            key_images: result.spent_key_images.clone(),
                            tx_hashes: result.spent_key_image_tx_hashes.clone(),
                            height: result.block_height,
                        }).await;
                    }

                    BlockScanResponse {
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        block_height: result.block_height,
                        block_hash: result.block_hash,
                        block_timestamp: result.block_timestamp,
                        tx_count: result.tx_count as u32,
                        outputs,
                        daemon_height: result.daemon_height,
                        spent_key_images: result.spent_key_images.clone(),
                        spent_key_image_tx_hashes: result.spent_key_image_tx_hashes.clone(),
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    BlockScanResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        block_height: request.block_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_query_daemon_height(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_query_daemon_height_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            match monero_rust::get_daemon_height(&request.node_url).await {
                Ok(height) => {
                    let _ = self_addr.notify(SetDaemonHeight { height }).await;

                    DaemonHeightResponse {
                        success: true,
                        error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                        daemon_height: height,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                    DaemonHeightResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        daemon_height: 0,
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_start_continuous_scan(mut self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_start_continuous_scan_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            if let Err(err) = monero_rust::error_codes::Network::parse(&request.network) {
                BlockScanResponse {
                    success: false, error: Some(err.message.clone()),
                    error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    block_height: request.start_height,
                    block_hash: String::new(), block_timestamp: 0, tx_count: 0,
                    outputs: Vec::new(), daemon_height: 0, spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(),
                }.send_signal_to_dart();
                continue;
            }

            let resolved_seed = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    BlockScanResponse {
                        success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                        block_height: request.start_height,
                        block_hash: String::new(), block_timestamp: 0, tx_count: 0,
                        outputs: Vec::new(), daemon_height: 0, spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            let _ = self_addr
                .notify(StartContinuousScan {
                    node_url: request.node_url,
                    start_height: request.start_height,
                    seed: resolved_seed,
                    passphrase: request.passphrase,
                    network: request.network,
                    account_lookahead: request.account_lookahead,
                    subaddress_lookahead: request.subaddress_lookahead,
                    accounts_to_scan: request.accounts_to_scan,
                })
                .await;
        }
    }

    async fn listen_to_stop_scan(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_stop_scan_request_receiver();
        while let Some(_dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(StopScan).await;
        }
    }

    async fn listen_to_mempool_scan(self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_mempool_scan_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            if let Err(err) = monero_rust::error_codes::Network::parse(&request.network) {
                MempoolScanResponse {
                    success: false, error: Some(err.message.clone()),
                    error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    tx_count: 0,
                    outputs: Vec::new(), spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(),
                }.send_signal_to_dart();
                continue;
            }

            let resolved_seed = match pre_resolve_bip39(&request.seed, &request.passphrase, request.bip39_account_index) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    MempoolScanResponse {
                        success: false, error: Some(e),
                        error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                        tx_count: 0,
                        outputs: Vec::new(), spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }.send_signal_to_dart();
                    continue;
                }
            };
            let mut addr = self_addr.clone();

            spawn_local(async move {
                match monero_rust::scan_mempool_for_outputs_with_account_lookahead(
                    &request.node_url,
                    &resolved_seed,
                    &request.network,
                    request.account_lookahead,
                    request.subaddress_lookahead,
                    &request.passphrase,
                )
                .await
                {
                    Ok(result) => {
                        let outputs: Vec<OwnedOutput> = result
                            .outputs
                            .iter()
                            .map(|o| {
                                let mut out: OwnedOutput = o.into();
                                out.block_height = 0; // Unconfirmed - in mempool
                                out
                            })
                            .collect();

                        let spent_key_images = result.spent_key_images.clone();

                        MempoolScanResponse {
                            success: true,
                            error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                            tx_count: result.tx_count as u32,
                            outputs,
                            spent_key_images: result.spent_key_images,
                            spent_key_image_tx_hashes: result.spent_key_image_tx_hashes,
                        }
                        .send_signal_to_dart();

                        if !spent_key_images.is_empty() {
                            // Track mempool-detected spends as pending
                            let _ = addr.notify(AddMempoolPendingSpends {
                                key_images: spent_key_images.clone(),
                            }).await;
                            let _ = addr.notify(CheckMempoolConflicts {
                                key_images: spent_key_images,
                            }).await;
                        }
                    }
                    Err(e) => {
                        let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                        MempoolScanResponse {
                            success: false,
                            error: Some(e),
                            error_code: Some(err.code),
                            error_hint: err.hint,
                            error_transient: Some(err.transient),
                            tx_count: 0,
                            outputs: Vec::new(),
                            spent_key_images: Vec::new(),
                            spent_key_image_tx_hashes: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_scan_block_multi_wallet() {
        let mut receiver = crate::ffi_web::get_scan_block_multi_wallet_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            // Pre-resolve all wallet seeds before spawning async task
            let mut resolved_seeds = Vec::new();
            let mut resolve_err = None;
            for w in &request.wallets {
                match pre_resolve_bip39(&w.seed, &w.passphrase, w.bip39_account_index) {
                    Ok(s) => resolved_seeds.push(s),
                    Err(e) => { resolve_err = Some(e); break; }
                }
            }
            if let Some(e) = resolve_err {
                let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                MultiWalletScanResponse {
                    success: false, error: Some(e),
                    error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    block_height: request.block_height,
                    block_hash: String::new(), block_timestamp: 0, tx_count: 0,
                    daemon_height: 0, spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(), wallet_results: Vec::new(),
                }.send_signal_to_dart();
                continue;
            }

            spawn_local(async move {
                // Convert wallet configs to the format expected by the scanner
                let wallet_configs: Vec<monero_rust::WalletScanConfig> = request
                    .wallets
                    .iter()
                    .enumerate()
                    .map(|(i, w)| monero_rust::WalletScanConfig {
                        mnemonic: resolved_seeds[i].clone(),
                        network: w.network.clone(),
                        lookahead: monero_rust::compute_lookahead(
                            w.account_lookahead,
                            w.subaddress_lookahead,
                            w.accounts_to_scan.as_deref(),
                        ),
                        passphrase: w.passphrase.clone(),
                    })
                    .collect();

                match monero_rust::scan_block_multi_wallet_with_url(
                    &request.node_url,
                    request.block_height,
                    wallet_configs,
                )
                .await
                {
                    Ok(result) => {
                        // Convert each wallet's results
                        let wallet_results: Vec<WalletScanResult> = result
                            .wallet_results
                            .into_iter()
                            .map(|(address, wallet_data)| {
                                let outputs = wallet_data
                                    .outputs
                                    .iter()
                                    .map(|o| o.into())
                                    .collect();

                                WalletScanResult { address, outputs }
                            })
                            .collect();

                        MultiWalletScanResponse {
                            success: true,
                            error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                            block_height: result.block_height,
                            block_hash: result.block_hash,
                            block_timestamp: result.block_timestamp,
                            tx_count: result.tx_count as u32,
                            daemon_height: result.daemon_height,
                            spent_key_images: result.spent_key_images,
                            spent_key_image_tx_hashes: result.spent_key_image_tx_hashes,
                            wallet_results,
                        }
                        .send_signal_to_dart();
                    }
                    Err(e) => {
                        let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                        MultiWalletScanResponse {
                            success: false,
                            error: Some(e),
                            error_code: Some(err.code),
                            error_hint: err.hint,
                            error_transient: Some(err.transient),
                            block_height: request.block_height,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            spent_key_image_tx_hashes: Vec::new(),
                            wallet_results: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_start_multi_wallet_scan(self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_start_multi_wallet_scan_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

            // Pre-resolve all wallet seeds
            let mut resolved_wallets = Vec::new();
            let mut resolve_err = None;
            for w in &request.wallets {
                match pre_resolve_bip39(&w.seed, &w.passphrase, w.bip39_account_index) {
                    Ok(s) => resolved_wallets.push(WalletConfig {
                        seed: s,
                        network: w.network.clone(),
                        account_lookahead: w.account_lookahead,
                        subaddress_lookahead: w.subaddress_lookahead,
                        accounts_to_scan: w.accounts_to_scan.clone(),
                        passphrase: w.passphrase.clone(),
                        bip39_account_index: 0,
                    }),
                    Err(e) => { resolve_err = Some(e); break; }
                }
            }
            if let Some(e) = resolve_err {
                let msg = format!("Failed to resolve BIP39 seed: {}", e);
                let err = monero_rust::error_codes::ErrorResponse::from_string(&msg);
                MultiWalletScanResponse {
                    success: false, error: Some(msg),
                    error_code: Some(err.code), error_hint: err.hint, error_transient: Some(err.transient),
                    block_height: 0, block_hash: String::new(), block_timestamp: 0,
                    tx_count: 0, daemon_height: 0, spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(), wallet_results: Vec::new(),
                }.send_signal_to_dart();
                continue;
            }

            let node_url = request.node_url.clone();
            let start_height = request.start_height;
            let wallets = resolved_wallets;
            let mut self_addr_clone = self_addr.clone();

            // Spawn task to get daemon height and start scanning
            spawn_local(async move {
                bump_generation();

                match monero_rust::get_daemon_height(&node_url).await {
                    Ok(daemon_height) => {
                        // Initialize multi-wallet scanning state
                        let _ = self_addr_clone
                            .notify(UpdateMultiWalletScanState {
                                is_scanning: true,
                                current_height: start_height,
                                target_height: daemon_height,
                                node_url: node_url.clone(),
                                wallets,
                            })
                            .await;

                        // Send initial progress
                        SyncProgressResponse {
                            current_height: start_height,
                            daemon_height,
                            is_synced: start_height >= daemon_height,
                            is_scanning: true,
                        }
                        .send_signal_to_dart();

                        // Start scanning
                        let _ = self_addr_clone.notify(ContinueMultiWalletScan).await;
                    }
                    Err(e) => {
                        let msg = format!("Failed to get daemon height: {}", e);
                        let err = monero_rust::error_codes::ErrorResponse::from_string(&msg);
                        MultiWalletScanResponse {
                            success: false,
                            error: Some(msg),
                            error_code: Some(err.code),
                            error_hint: err.hint,
                            error_transient: Some(err.transient),
                            block_height: 0,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            spent_key_image_tx_hashes: Vec::new(),
                            wallet_results: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_restore_wallet_data(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_restore_wallet_data_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let msg = dart_msg;
            let resolved_seed = match pre_resolve_bip39(&msg.seed, &msg.passphrase, msg.bip39_account_index) {
                Ok(s) => s,
                Err(_e) => msg.seed.clone(), // Fall through; RestoreOutputs just stores seed
            };
            let outputs: Vec<monero_rust::WalletOutput> = msg.outputs.into_iter().map(|o| o.into()).collect();
            let _ = self_addr.notify(RestoreOutputs {
                seed: resolved_seed,
                network: msg.network,
                outputs,
                daemon_height: msg.daemon_height,
                current_height: msg.current_height,
                block_hashes_json: msg.block_hashes_json,
                pending_state_json: msg.pending_state_json,
            }).await;
        }
    }

    async fn listen_to_get_block_hashes(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_get_block_hashes_request_receiver();
        while let Some(_dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(GetBlockHashesMsg).await;
        }
    }

    async fn listen_to_get_pending_state(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_get_pending_state_request_receiver();
        while let Some(_dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(GetPendingStateMsg).await;
        }
    }

    async fn listen_to_convert_bip39_to_legacy(_self_addr: Address<Self>) {
        use monero_rust::error_codes::ErrorResponse;

        let mut receiver = crate::ffi_web::get_convert_bip39_to_legacy_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            match monero_rust::bip39_to_legacy_mnemonic(
                &request.bip39_mnemonic,
                &request.passphrase,
                request.account_index,
            ) {
                Ok(legacy) => {
                    Bip39LegacySeedResponse {
                        legacy_seed: legacy,
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    Bip39LegacySeedResponse {
                        legacy_seed: String::new(),
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RestoreOutputs {
    seed: String,
    network: String,
    outputs: Vec<monero_rust::WalletOutput>,
    daemon_height: u64,
    current_height: u64,
    block_hashes_json: Option<String>,
    pending_state_json: Option<String>,
}

#[derive(Debug, Clone)]
struct GetBlockHashesMsg;

#[derive(Debug, Clone)]
struct GetPendingStateMsg;

#[async_trait]
impl Notifiable<RestoreOutputs> for WalletActor {
    async fn notify(&mut self, msg: RestoreOutputs, _ctx: &Context<Self>) {
        log::info!(
            "[RestoreOutputs] outputs={}, daemon_height={}, current_height={}, has_block_hashes={}",
            msg.outputs.len(), msg.daemon_height, msg.current_height, msg.block_hashes_json.is_some()
        );
        self.seed = Some(msg.seed);
        self.network = Some(msg.network);
        self.core_state.daemon_height = msg.daemon_height;
        self.core_state.current_height = msg.current_height;
        self.core_state.replace_outputs(msg.outputs);

        if let Some(json) = msg.block_hashes_json {
            match serde_json::from_str::<monero_rust::BlockHashChain>(&json) {
                Ok(chain) => {
                    self.core_state.block_hashes = chain;
                    log::info!("[RestoreOutputs] Block hash chain restored");
                }
                Err(e) => {
                    log::error!("[RestoreOutputs] Failed to deserialize block hashes: {}", e);
                }
            }
        }

        if let Some(json) = msg.pending_state_json {
            #[derive(serde::Deserialize)]
            struct PendingStateBlob {
                #[serde(default)]
                pending_spends: std::collections::HashMap<String, monero_rust::PendingSpend>,
                #[serde(default)]
                tracked_transactions: std::collections::HashMap<String, monero_rust::TrackedTransaction>,
            }
            match serde_json::from_str::<PendingStateBlob>(&json) {
                Ok(blob) => {
                    let now = current_time_secs();
                    self.core_state.restore_pending_spends(blob.pending_spends, now);
                    self.core_state.restore_tracked_transactions(blob.tracked_transactions);
                    log::info!("[RestoreOutputs] Pending state restored");
                }
                Err(e) => {
                    log::error!("[RestoreOutputs] Failed to deserialize pending state: {}", e);
                }
            }
        }
    }
}

#[async_trait]
impl Notifiable<GetPendingStateMsg> for WalletActor {
    async fn notify(&mut self, _msg: GetPendingStateMsg, _ctx: &Context<Self>) {
        #[derive(serde::Serialize)]
        struct PendingStateBlob<'a> {
            pending_spends: &'a std::collections::HashMap<String, monero_rust::PendingSpend>,
            tracked_transactions: &'a std::collections::HashMap<String, monero_rust::TrackedTransaction>,
        }
        let blob = PendingStateBlob {
            pending_spends: self.core_state.pending_spends(),
            tracked_transactions: self.core_state.tracked_transactions(),
        };
        match serde_json::to_string(&blob) {
            Ok(json) => {
                PendingStateResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    pending_state_json: Some(json),
                }
                .send_signal_to_dart();
            }
            Err(e) => {
                PendingStateResponse {
                    success: false,
                    error: Some(format!("Failed to serialize pending state: {}", e)),
                    error_code: Some(monero_rust::error_codes::ERR_SERIALIZATION),
                    error_hint: None,
                    error_transient: Some(false),
                    pending_state_json: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[async_trait]
impl Notifiable<GetBlockHashesMsg> for WalletActor {
    async fn notify(&mut self, _msg: GetBlockHashesMsg, _ctx: &Context<Self>) {
        let json_result = serde_json::to_string(&self.core_state.block_hashes);
        match json_result {
            Ok(json) => {
                BlockHashesResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    block_hashes_json: Some(json),
                }
                .send_signal_to_dart();
            }
            Err(e) => {
                BlockHashesResponse {
                    success: false,
                    error: Some(format!("Failed to serialize block hashes: {}", e)),
                    error_code: Some(monero_rust::error_codes::ERR_SERIALIZATION),
                    error_hint: None,
                    error_transient: Some(false),
                    block_hashes_json: None,
                }
                .send_signal_to_dart();
            }
        }
    }
}

#[async_trait]
impl Notifiable<CreateWalletRequest> for WalletActor {
    async fn notify(&mut self, msg: CreateWalletRequest, _ctx: &Context<Self>) {
        self.address = format!("4{}_placeholder", msg.network);
        WalletCreatedResponse {
            address: self.address.clone(),
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<UpdateBalance> for WalletActor {
    async fn notify(&mut self, _msg: UpdateBalance, _ctx: &Context<Self>) {
        // Balance is now computed from core_state, no-op
    }
}

#[async_trait]
impl Notifiable<GetBalanceRequest> for WalletActor {
    async fn notify(&mut self, _msg: GetBalanceRequest, _ctx: &Context<Self>) {
        let bal = self.core_state.balance();
        BalanceResponse {
            confirmed: bal.confirmed,
            unconfirmed: bal.unconfirmed,
            pending_spend: bal.pending_spend,
        }
        .send_signal_to_dart();
    }
}
#[async_trait]
impl Notifiable<StoreOutputs> for WalletActor {
    async fn notify(&mut self, msg: StoreOutputs, _ctx: &Context<Self>) {
        log::info!(
            "[StoreOutputs] new_outputs={}, daemon_height={}, total_after={}, block_hashes={}",
            msg.outputs.len(), msg.daemon_height,
            self.core_state.outputs().len() + msg.outputs.len(),
            msg.block_hashes.len()
        );
        self.seed = Some(msg.seed);
        self.network = Some(msg.network);
        self.core_state.daemon_height = msg.daemon_height;
        self.core_state.add_outputs(msg.outputs);
        for (height, hash) in msg.block_hashes {
            self.core_state.record_block_hash(height, hash);
        }
        self.core_state.block_hashes.compact();
    }
}

#[async_trait]
impl Notifiable<RecordBlockHashes> for WalletActor {
    async fn notify(&mut self, msg: RecordBlockHashes, _ctx: &Context<Self>) {
        for (height, hash) in msg.block_hashes {
            self.core_state.record_block_hash(height, hash);
        }
        self.core_state.block_hashes.compact();
        if msg.daemon_height > self.core_state.daemon_height {
            self.core_state.daemon_height = msg.daemon_height;
        }
    }
}

#[async_trait]
impl Handler<GetWalletData> for WalletActor {
    type Result = WalletData;

    async fn handle(&mut self, _msg: GetWalletData, _ctx: &Context<Self>) -> Self::Result {
        WalletData {
            seed: self.seed.clone(),
            network: self.network.clone(),
            outputs: self.core_state.outputs().to_vec(),
            pending_key_images: self.core_state.pending_key_images(),
        }
    }
}

#[async_trait]
impl Notifiable<MarkOutputsSpent> for WalletActor {
    async fn notify(&mut self, msg: MarkOutputsSpent, _ctx: &Context<Self>) {
        self.core_state.mark_spent_by_output_keys(&msg.output_keys);
    }
}

#[async_trait]
impl Notifiable<UpdateOutputKeyImages> for WalletActor {
    async fn notify(&mut self, msg: UpdateOutputKeyImages, _ctx: &Context<Self>) {
        let outputs = self.core_state.outputs_mut();
        // Sort by (block_height, output_index) to match Monero's positional
        // key image export ordering.
        outputs.sort_by(|a, b| {
            a.block_height.cmp(&b.block_height)
                .then(a.output_index.cmp(&b.output_index))
        });
        let ki_count = msg.key_images.len();
        let out_count = outputs.len();
        if ki_count != out_count {
            log::warn!(
                "[UpdateOutputKeyImages] count mismatch: {} key images vs {} outputs — \
                 positional assignment may pair key images with wrong outputs",
                ki_count, out_count
            );
        }
        let count = ki_count.min(out_count);
        for i in 0..count {
            outputs[i].key_image = msg.key_images[i].clone();
        }
        log::info!(
            "[UpdateOutputKeyImages] assigned {} key images to {} outputs",
            count, out_count
        );
    }
}

#[async_trait]
impl Handler<GetWalletHeight> for WalletActor {
    type Result = WalletHeight;

    async fn handle(&mut self, _msg: GetWalletHeight, _ctx: &Context<Self>) -> Self::Result {
        WalletHeight {
            current_height: self.core_state.current_height,
            daemon_height: self.core_state.daemon_height,
        }
    }
}

#[async_trait]
impl Notifiable<UpdateScanState> for WalletActor {
    async fn notify(&mut self, msg: UpdateScanState, ctx: &Context<Self>) {
        // If multi-wallet scan is active, stop it first to allow transition
        if self.is_scanning && self.active_scan_type == ScanType::MultiWallet {
            log::info!("Stopping multi-wallet scan to transition to single-wallet scan");

            // Stop the multi-wallet scan
            self.notify(StopScan {}, ctx).await;
        }

        self.is_scanning = msg.is_scanning;
        self.active_scan_type = if msg.is_scanning { ScanType::SingleWallet } else { ScanType::None };
        self.scan_current_height = msg.current_height;
        self.scan_target_height = msg.target_height;
        self.scan_node_url = msg.node_url;
        self.scan_seed = msg.seed;
        self.scan_passphrase = msg.passphrase;
        self.scan_network = msg.network;
        self.scan_account_lookahead = msg.account_lookahead;
        self.scan_subaddress_lookahead = msg.subaddress_lookahead;
        self.scan_accounts_to_scan = msg.accounts_to_scan;
    }
}

#[async_trait]
impl Notifiable<UpdateMultiWalletScanState> for WalletActor {
    async fn notify(&mut self, msg: UpdateMultiWalletScanState, ctx: &Context<Self>) {
        // If single-wallet scan is active, stop it first to allow transition
        if self.is_scanning && self.active_scan_type == ScanType::SingleWallet {
            log::info!("Stopping single-wallet scan to transition to multi-wallet scan");

            // Stop the single-wallet scan
            self.notify(StopScan {}, ctx).await;
        }

        self.is_scanning = msg.is_scanning;
        self.active_scan_type = if msg.is_scanning { ScanType::MultiWallet } else { ScanType::None };
        self.multi_wallet_scan_current_height = msg.current_height;
        self.multi_wallet_scan_target_height = msg.target_height;
        self.multi_wallet_scan_node_url = msg.node_url;
        self.multi_wallet_scan_wallets = msg.wallets;
        if msg.target_height > self.core_state.daemon_height {
            self.core_state.daemon_height = msg.target_height;
        }
    }
}

#[async_trait]
impl Notifiable<StartContinuousScan> for WalletActor {
    async fn notify(&mut self, msg: StartContinuousScan, ctx: &Context<Self>) {
        let node_url = msg.node_url.clone();
        let start_height = msg.start_height;
        let seed = msg.seed.clone();
        let passphrase = msg.passphrase.clone();
        let network = msg.network.clone();
        let account_lookahead = msg.account_lookahead;
        let subaddress_lookahead = msg.subaddress_lookahead;
        let mut self_addr = ctx.address();

        spawn_local(async move {
            bump_generation();

            match monero_rust::get_daemon_height(&node_url).await {
                Ok(daemon_height) => {
                    // Initialize scanning state
                    let _ = self_addr
                        .notify(UpdateScanState {
                            is_scanning: true,
                            current_height: start_height,
                            target_height: daemon_height,
                            node_url: node_url.clone(),
                            seed: seed.clone(),
                            passphrase: passphrase.clone(),
                            network: network.clone(),
                            account_lookahead,
                            subaddress_lookahead,
                            accounts_to_scan: msg.accounts_to_scan.clone(),
                        })
                        .await;

                    // Send initial progress
                    SyncProgressResponse {
                        current_height: start_height,
                        daemon_height,
                        is_synced: start_height >= daemon_height,
                        is_scanning: true,
                    }
                    .send_signal_to_dart();

                    // Start scanning
                    let _ = self_addr.notify(ContinueScan).await;
                }
                Err(e) => {
                    let msg = format!("Failed to get daemon height: {}", e);
                    log::error!("[StartContinuousScan] {}", msg);

                    let err = monero_rust::error_codes::ErrorResponse::from_string(&msg);
                    BlockScanResponse {
                        success: false,
                        error: Some(msg),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        block_height: 0,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }
                    .send_signal_to_dart();
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<StopScan> for WalletActor {
    async fn notify(&mut self, _msg: StopScan, _ctx: &Context<Self>) {
        bump_generation();
        self.is_scanning = false;

        if self.active_scan_type == ScanType::None {
            return;
        }

        // Use correct state fields based on which scan was active
        let (current_height, target_height) = match self.active_scan_type {
            ScanType::SingleWallet => (self.scan_current_height, self.scan_target_height),
            ScanType::MultiWallet => (self.multi_wallet_scan_current_height, self.multi_wallet_scan_target_height),
            ScanType::None => unreachable!(),
        };

        self.active_scan_type = ScanType::None;

        SyncProgressResponse {
            current_height,
            daemon_height: target_height,
            is_synced: current_height >= target_height,
            is_scanning: false,
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<ContinueScan> for WalletActor {
    async fn notify(&mut self, _msg: ContinueScan, ctx: &Context<Self>) {
        if !self.is_scanning || self.scan_current_height >= self.scan_target_height {
            if self.scan_current_height >= self.scan_target_height {
                self.is_scanning = false;
                SyncProgressResponse {
                    current_height: self.scan_current_height,
                    daemon_height: self.scan_target_height,
                    is_synced: true,
                    is_scanning: false,
                }
                .send_signal_to_dart();
            }
            return;
        }

        let node_url = self.scan_node_url.clone();
        let batch_start_height = self.scan_current_height;
        let seed = self.scan_seed.clone();
        let passphrase = self.scan_passphrase.clone();
        let network = self.scan_network.clone();
        let account_lookahead = self.scan_account_lookahead;
        let subaddress_lookahead = self.scan_subaddress_lookahead;
        let accounts_to_scan = self.scan_accounts_to_scan.clone();
        let target_height = self.scan_target_height;
        let mut self_addr = ctx.address();
        let block_hash_history = self.core_state.get_short_chain_history();

        // For batch scanning, we don't increment by 1 here.
        // The batch result will tell us how many blocks were fetched.
        // Set current_height to target to prevent re-entry; the async task
        // will update it via UpdateScanState or direct notify.
        self.scan_current_height = self.scan_target_height;

        let scan_gen = PREFETCH_GENERATION.with(|g| g.get());

        spawn_local(async move {
            let lookahead = monero_rust::compute_lookahead(
                account_lookahead,
                subaddress_lookahead,
                accounts_to_scan.as_deref(),
            );

            // 1. Take from prefetch slot or fetch fresh (with history for reorg detection)
            let fetched = match take_prefetch(scan_gen, batch_start_height) {
                Some(data) => data,
                None => match monero_rust::fetch_blocks_batch_with_history_url(
                    &node_url,
                    batch_start_height,
                    &block_hash_history,
                    false,
                ).await {
                    Ok((data, _actual_start)) => data,
                    Err(e) => {
                        log::error!(
                            "[ContinueScan] Batch fetch error at height {}: {}",
                            batch_start_height, e
                        );

                        let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                        BlockScanResponse {
                            success: false,
                            error: Some(e),
                            error_code: Some(err.code),
                            error_hint: err.hint,
                            error_transient: Some(err.transient),
                            block_height: batch_start_height,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            outputs: Vec::new(),
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            spent_key_image_tx_hashes: Vec::new(),
                        }
                        .send_signal_to_dart();

                        SyncProgressResponse {
                            current_height: batch_start_height,
                            daemon_height: target_height,
                            is_synced: false,
                            is_scanning: false,
                        }
                        .send_signal_to_dart();

                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }
                },
            };

            if fetched.is_empty() {
                let _ = self_addr.notify(StopScan).await;
                return;
            }

            // 2. Spawn prefetch for next batch while we process this one
            let next_height = batch_start_height + fetched.block_count() as u64;
            if next_height < target_height {
                let prefetch_url = node_url.clone();
                spawn_local(async move {
                    if let Ok(data) = monero_rust::fetch_blocks_batch_with_url(
                        &prefetch_url,
                        next_height,
                        false,
                    ).await {
                        store_prefetch(scan_gen, next_height, data);
                    }
                });
            }

            // 3. Process current batch (reuse cached scanner if available)
            let cached_scanner = take_scanner_cache(scan_gen);
            match monero_rust::process_fetched_batch_cached(
                fetched,
                &seed,
                &network,
                lookahead,
                cached_scanner,
                &passphrase,
            ).await {
                Ok((batch_results, returned_scanner)) => {
                    if batch_results.is_empty() {
                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }

                    // Check for reorg: compare block hashes from batch against known hashes
                    let mut reorg_detected = false;
                    for result in &batch_results {
                        for (known_height, known_hash) in &block_hash_history {
                            if result.block_height == *known_height
                                && result.block_hash != *known_hash
                            {
                                reorg_detected = true;
                                break;
                            }
                        }
                        if reorg_detected { break; }
                    }

                    if reorg_detected {
                        log::info!(
                            "[ContinueScan] Reorg detected at batch starting {}",
                            batch_start_height
                        );
                        let _ = self_addr.notify(HandleReorg {
                            batch_results,
                            accounts_to_scan,
                            target_height,
                            batch_start_height,
                            node_url,
                            seed,
                            passphrase,
                            network,
                            account_lookahead,
                            subaddress_lookahead,
                        }).await;
                        return;
                    }

                    // Cache the scanner for the next batch
                    store_scanner_cache(scan_gen, returned_scanner);

                    let processed = monero_rust::process_single_wallet_batch(
                        &batch_results,
                        accounts_to_scan.as_deref(),
                        target_height,
                        batch_start_height,
                    );

                    // Send BlockScanResponse for each block with outputs
                    for block in &processed.blocks_with_outputs {
                        let outputs: Vec<OwnedOutput> = block.outputs.iter().map(|o| o.into()).collect();
                        BlockScanResponse {
                            success: true,
                            error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                            block_height: block.block_height,
                            block_hash: block.block_hash.clone(),
                            block_timestamp: block.block_timestamp,
                            tx_count: block.tx_count as u32,
                            outputs,
                            daemon_height: block.daemon_height,
                            spent_key_images: block.spent_key_images.clone(),
                            spent_key_image_tx_hashes: block.spent_key_image_tx_hashes.clone(),
                        }
                        .send_signal_to_dart();
                    }

                    // Always update daemon_height so spendability checks
                    // stay current even when a batch has no new outputs.
                    let _ = self_addr
                        .notify(StoreOutputs {
                            seed: seed.clone(),
                            network: network.clone(),
                            outputs: processed.outputs_to_store,
                            daemon_height: processed.daemon_height,
                            block_hashes: processed.block_hashes.clone(),
                        })
                        .await;

                    if !processed.spent_key_images.is_empty() {
                        let _ = self_addr
                            .notify(UpdateSpentStatus {
                                key_images: processed.spent_key_images,
                                tx_hashes: processed.spent_key_image_tx_hashes,
                                height: processed.batch_end_height,
                            })
                            .await;
                    }

                    // If scan was cancelled (e.g. user paused) while we were
                    // processing, don't re-enable scanning or continue the loop.
                    // Outputs above were still stored so nothing is lost.
                    if PREFETCH_GENERATION.with(|g| g.get()) != scan_gen {
                        return;
                    }

                    let progress = monero_rust::sync_progress(processed.batch_end_height, target_height);
                    SyncProgressResponse {
                        current_height: progress.current_height,
                        daemon_height: progress.target_height,
                        is_synced: progress.is_synced,
                        is_scanning: progress.is_scanning,
                    }
                    .send_signal_to_dart();

                    let _ = self_addr
                        .notify(UpdateScanState {
                            is_scanning: processed.should_continue,
                            current_height: processed.batch_end_height,
                            target_height,
                            node_url,
                            seed,
                            passphrase,
                            network,
                            account_lookahead,
                            subaddress_lookahead,
                            accounts_to_scan,
                        })
                        .await;

                    if processed.should_continue {
                        let _ = self_addr.notify(ContinueScan).await;
                    } else {
                        let _ = self_addr.notify(StopScan).await;
                    }
                }
                Err(e) => {
                    log::error!(
                        "[ContinueScan] Batch scan error at height {}: {}",
                        batch_start_height, e
                    );

                    let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                    BlockScanResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        block_height: batch_start_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                    }
                    .send_signal_to_dart();

                    SyncProgressResponse {
                        current_height: batch_start_height,
                        daemon_height: target_height,
                        is_synced: false,
                        is_scanning: false,
                    }
                    .send_signal_to_dart();

                    let _ = self_addr.notify(StopScan).await;
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<ContinueMultiWalletScan> for WalletActor {
    async fn notify(&mut self, _msg: ContinueMultiWalletScan, ctx: &Context<Self>) {
        if !self.is_scanning || self.multi_wallet_scan_current_height >= self.multi_wallet_scan_target_height {
            if self.multi_wallet_scan_current_height >= self.multi_wallet_scan_target_height {
                self.is_scanning = false;
                SyncProgressResponse {
                    current_height: self.multi_wallet_scan_current_height,
                    daemon_height: self.multi_wallet_scan_target_height,
                    is_synced: true,
                    is_scanning: false,
                }
                .send_signal_to_dart();
            }
            return;
        }

        let node_url = self.multi_wallet_scan_node_url.clone();
        let batch_start_height = self.multi_wallet_scan_current_height;
        let wallets = self.multi_wallet_scan_wallets.clone();
        let target_height = self.multi_wallet_scan_target_height;
        let mut self_addr = ctx.address();
        let block_hash_history = self.core_state.get_short_chain_history();

        // Prevent re-entry while batch is in flight
        self.multi_wallet_scan_current_height = self.multi_wallet_scan_target_height;

        let scan_gen = PREFETCH_GENERATION.with(|g| g.get());

        spawn_local(async move {
            let wallet_configs: Vec<monero_rust::WalletScanConfig> = wallets
                .iter()
                .map(|w| monero_rust::WalletScanConfig {
                    mnemonic: w.seed.clone(),
                    network: w.network.clone(),
                    lookahead: monero_rust::compute_lookahead(
                        w.account_lookahead,
                        w.subaddress_lookahead,
                        w.accounts_to_scan.as_deref(),
                    ),
                    passphrase: w.passphrase.clone(),
                })
                .collect();

            // 1. Take from prefetch slot or fetch fresh (with history for reorg detection)
            let fetched = match take_prefetch(scan_gen, batch_start_height) {
                Some(data) => data,
                None => match monero_rust::fetch_blocks_batch_with_history_url(
                    &node_url,
                    batch_start_height,
                    &block_hash_history,
                    false,
                ).await {
                    Ok((data, _actual_start)) => data,
                    Err(e) => {
                        let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                        MultiWalletScanResponse {
                            success: false,
                            error: Some(e),
                            error_code: Some(err.code),
                            error_hint: err.hint,
                            error_transient: Some(err.transient),
                            block_height: batch_start_height,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            spent_key_image_tx_hashes: Vec::new(),
                            wallet_results: Vec::new(),
                        }
                        .send_signal_to_dart();

                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }
                },
            };

            if fetched.is_empty() {
                let _ = self_addr.notify(StopScan).await;
                return;
            }

            // 2. Spawn prefetch for next batch while we process this one
            let next_height = batch_start_height + fetched.block_count() as u64;
            if next_height < target_height {
                let prefetch_url = node_url.clone();
                spawn_local(async move {
                    if let Ok(data) = monero_rust::fetch_blocks_batch_with_url(
                        &prefetch_url,
                        next_height,
                        false,
                    ).await {
                        store_prefetch(scan_gen, next_height, data);
                    }
                });
            }

            // 3. Process current batch (reuse cached scanners if available)
            let cached_scanners = take_multi_scanner_cache(scan_gen);
            match monero_rust::process_fetched_batch_multi_wallet_cached(
                fetched,
                wallet_configs,
                cached_scanners,
            ).await {
                Ok((batch_results, returned_scanners)) => {
                    if batch_results.is_empty() {
                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }

                    // Check for reorg: compare block hashes against known history
                    let mut reorg_detected = false;
                    for result in &batch_results {
                        for (known_height, known_hash) in &block_hash_history {
                            if result.block_height == *known_height && result.block_hash != *known_hash {
                                reorg_detected = true;
                                break;
                            }
                        }
                        if reorg_detected { break; }
                    }

                    if reorg_detected {
                        log::warn!(
                            "[ContinueMultiWalletScan] Reorg detected at batch starting {}",
                            batch_start_height
                        );
                        // Convert MultiWalletScanResult to BlockScanResult for HandleReorg
                        // Use first wallet's data as representative (all wallets see same blocks)
                        let block_scan_results: Vec<monero_rust::BlockScanResult> = batch_results.iter().map(|r| {
                            monero_rust::BlockScanResult {
                                block_height: r.block_height,
                                block_hash: r.block_hash.clone(),
                                block_timestamp: r.block_timestamp,
                                tx_count: r.tx_count,
                                outputs: Vec::new(),
                                daemon_height: r.daemon_height,
                                spent_key_images: r.spent_key_images.clone(),
                                spent_key_image_tx_hashes: r.spent_key_image_tx_hashes.clone(),
                            }
                        }).collect();
                        // Use first wallet's config for reorg handling
                        let first_wallet = &wallets[0];
                        let _ = self_addr.notify(HandleReorg {
                            batch_results: block_scan_results,
                            accounts_to_scan: first_wallet.accounts_to_scan.clone(),
                            target_height,
                            batch_start_height,
                            node_url,
                            seed: first_wallet.seed.clone(),
                            passphrase: first_wallet.passphrase.clone(),
                            network: first_wallet.network.clone(),
                            account_lookahead: first_wallet.account_lookahead,
                            subaddress_lookahead: first_wallet.subaddress_lookahead,
                        }).await;
                        return;
                    }

                    // Cache the scanners for the next batch
                    store_multi_scanner_cache(scan_gen, returned_scanners);

                    let batch_end_height = batch_results
                        .last()
                        .map(|r| r.block_height + 1)
                        .unwrap_or(batch_start_height);

                    // Collect block hashes and spent key images from this batch
                    let mut block_hashes = Vec::new();
                    let mut all_spent_key_images = Vec::new();
                    let mut all_spent_tx_hashes = Vec::new();
                    let mut last_daemon_height = 0u64;
                    for result in &batch_results {
                        block_hashes.push((result.block_height, result.block_hash.clone()));
                        all_spent_key_images.extend(result.spent_key_images.iter().cloned());
                        all_spent_tx_hashes.extend(result.spent_key_image_tx_hashes.iter().cloned());
                        if result.daemon_height > last_daemon_height {
                            last_daemon_height = result.daemon_height;
                        }
                    }

                    // Record block hashes into core_state
                    let _ = self_addr.notify(RecordBlockHashes {
                        block_hashes,
                        daemon_height: last_daemon_height,
                    }).await;

                    // Record spent key images into core_state
                    if !all_spent_key_images.is_empty() {
                        let _ = self_addr.notify(UpdateSpentStatus {
                            key_images: all_spent_key_images,
                            tx_hashes: all_spent_tx_hashes,
                            height: batch_end_height,
                        }).await;
                    }

                    // If scan was cancelled while we were processing, stop here.
                    if PREFETCH_GENERATION.with(|g| g.get()) != scan_gen {
                        return;
                    }

                    for result in &batch_results {
                        let wallet_results: Vec<WalletScanResult> = result
                            .wallet_results
                            .iter()
                            .map(|(address, wallet_data)| {
                                let outputs: Vec<OwnedOutput> = wallet_data.outputs.iter().map(|o| o.into()).collect();

                                WalletScanResult {
                                    address: address.clone(),
                                    outputs,
                                }
                            })
                            .collect();

                        let has_outputs = wallet_results.iter().any(|wr| !wr.outputs.is_empty());

                        if has_outputs {
                            MultiWalletScanResponse {
                                success: true,
                                error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                                block_height: result.block_height,
                                block_hash: result.block_hash.clone(),
                                block_timestamp: result.block_timestamp,
                                tx_count: result.tx_count as u32,
                                daemon_height: result.daemon_height,
                                spent_key_images: result.spent_key_images.clone(),
                                spent_key_image_tx_hashes: result.spent_key_image_tx_hashes.clone(),
                                wallet_results,
                            }
                            .send_signal_to_dart();
                        }
                    }

                    let progress = monero_rust::sync_progress(batch_end_height, target_height);
                    SyncProgressResponse {
                        current_height: progress.current_height,
                        daemon_height: progress.target_height,
                        is_synced: progress.is_synced,
                        is_scanning: progress.is_scanning,
                    }
                    .send_signal_to_dart();

                    let should_continue = batch_end_height < target_height;
                    let _ = self_addr
                        .notify(UpdateMultiWalletScanState {
                            is_scanning: should_continue,
                            current_height: batch_end_height,
                            target_height,
                            node_url,
                            wallets,
                        })
                        .await;

                    if should_continue {
                        let _ = self_addr.notify(ContinueMultiWalletScan).await;
                    } else {
                        let _ = self_addr.notify(StopScan).await;
                    }
                }
                Err(e) => {
                    let err = monero_rust::error_codes::ErrorResponse::from_string(&e);
                    MultiWalletScanResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        block_height: batch_start_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                        spent_key_image_tx_hashes: Vec::new(),
                        wallet_results: Vec::new(),
                    }
                    .send_signal_to_dart();

                    let _ = self_addr.notify(StopScan).await;
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<SetDaemonHeight> for WalletActor {
    async fn notify(&mut self, msg: SetDaemonHeight, _ctx: &Context<Self>) {
        if msg.height > self.core_state.daemon_height {
            self.core_state.daemon_height = msg.height;
        }
    }
}

#[async_trait]
impl Notifiable<UpdateSpentStatus> for WalletActor {
    async fn notify(&mut self, msg: UpdateSpentStatus, _ctx: &Context<Self>) {
        // Confirm any pending spends that are now on-chain
        let mut confirmed_tx_ids: HashSet<String> = HashSet::new();
        for ki in &msg.key_images {
            if let Some(ps) = self.core_state.pending_spends().get(ki).cloned() {
                confirmed_tx_ids.insert(ps.tx_id.clone());
            }
            self.core_state.confirm_pending_spend(ki, msg.height);
        }

        let (updated_count, conflicts) = self.core_state.mark_spent_detecting_conflicts(
            &msg.key_images, &msg.tx_hashes, msg.height
        );

        if !conflicts.is_empty() {
            DoubleSpendDetectedResponse {
                conflicts: conflicts.iter().map(|c| DoubleSpendConflict {
                    key_image: c.key_image.clone(),
                    previous_spent_height: c.previous_spent_height.unwrap_or(0),
                    new_height: c.new_height,
                }).collect(),
            }
            .send_signal_to_dart();
        }

        // Check if any tracked transaction is now fully confirmed
        for tx_id in &confirmed_tx_ids {
            let all_confirmed = self
                .core_state
                .pending_spends_for_tx(tx_id)
                .is_empty();
            if all_confirmed {
                if let Some(new_status) = self.core_state.advance_tx_status(
                    tx_id,
                    monero_rust::TxStatus::Confirmed { height: msg.height },
                ) {
                    let height = match new_status {
                        monero_rust::TxStatus::Confirmed { height } => Some(*height),
                        _ => None,
                    };
                    TransactionStatusUpdate {
                        tx_id: tx_id.clone(),
                        status: "confirmed".to_string(),
                        confirmed_height: height,
                    }
                    .send_signal_to_dart();
                }
            }
        }

        if updated_count > 0 || !confirmed_tx_ids.is_empty() {
            let balance = self.core_state.balance();

            BalanceResponse {
                confirmed: balance.confirmed,
                unconfirmed: balance.unconfirmed,
                pending_spend: balance.pending_spend,
            }
            .send_signal_to_dart();

            SpentStatusUpdatedResponse {
                spent_key_images: msg.key_images.clone(),
            }
            .send_signal_to_dart();
        }
    }
}

#[async_trait]
impl Notifiable<CheckMempoolConflicts> for WalletActor {
    async fn notify(&mut self, msg: CheckMempoolConflicts, _ctx: &Context<Self>) {
        let conflicts = self.core_state.check_spent_conflicts(&msg.key_images);
        if !conflicts.is_empty() {
            DoubleSpendDetectedResponse {
                conflicts: conflicts.iter().map(|c| DoubleSpendConflict {
                    key_image: c.key_image.clone(),
                    previous_spent_height: c.previous_spent_height.unwrap_or(0),
                    new_height: c.new_height,
                }).collect(),
            }
            .send_signal_to_dart();
        }
    }
}

#[async_trait]
impl Notifiable<HandleReorg> for WalletActor {
    async fn notify(&mut self, msg: HandleReorg, ctx: &Context<Self>) {
        let outcome = monero_rust::process_batch_with_reorg_detection(
            &msg.batch_results,
            &mut self.core_state,
            msg.accounts_to_scan.as_deref(),
            msg.target_height,
            msg.batch_start_height,
        );

        match outcome {
            Ok(monero_rust::ScanBatchOutcome::Reorg(info)) => {
                log::info!(
                    "[HandleReorg] Reorg at height {}: {} blocks detached, {} outputs removed, {} outputs unspent",
                    info.split_height, info.blocks_detached,
                    info.outputs_removed, info.outputs_unspent
                );

                ReorgDetectedResponse {
                    split_height: info.split_height,
                    blocks_detached: info.blocks_detached,
                    outputs_removed: info.outputs_removed as u64,
                    outputs_unspent: info.outputs_unspent as u64,
                    removed_key_images: info.removed_key_images.clone(),
                    unspent_key_images: info.unspent_key_images.clone(),
                }.send_signal_to_dart();

                // Process new-chain blocks from same batch
                let new_blocks: Vec<_> = msg.batch_results.iter()
                    .filter(|r| r.block_height >= info.split_height)
                    .cloned().collect();
                let processed = monero_rust::process_single_wallet_batch(
                    &new_blocks,
                    msg.accounts_to_scan.as_deref(),
                    msg.target_height,
                    info.split_height,
                );

                self.core_state.add_outputs(processed.outputs_to_store);
                for (h, hash) in &processed.block_hashes {
                    self.core_state.record_block_hash(*h, hash.clone());
                }
                self.core_state.block_hashes.compact();
                if !processed.spent_key_images.is_empty() {
                    self.core_state.mark_spent_detecting_conflicts(
                        &processed.spent_key_images,
                        &processed.spent_key_image_tx_hashes,
                        processed.batch_end_height,
                    );
                }

                let balance = self.core_state.balance();
                BalanceResponse {
                    confirmed: balance.confirmed,
                    unconfirmed: balance.unconfirmed,
                    pending_spend: balance.pending_spend,
                }.send_signal_to_dart();

                // Continue scanning from where the batch left off
                let mut self_addr = ctx.address();
                let _ = self_addr.notify(UpdateScanState {
                    is_scanning: processed.should_continue,
                    current_height: processed.batch_end_height,
                    target_height: msg.target_height,
                    node_url: msg.node_url,
                    seed: msg.seed,
                    passphrase: msg.passphrase,
                    network: msg.network,
                    account_lookahead: msg.account_lookahead,
                    subaddress_lookahead: msg.subaddress_lookahead,
                    accounts_to_scan: msg.accounts_to_scan,
                }).await;

                if processed.should_continue {
                    let _ = self_addr.notify(ContinueScan).await;
                }
            }
            Ok(monero_rust::ScanBatchOutcome::Normal(_)) => {
                // False alarm — hash comparison in spawn_local was wrong.
                // Continue scanning normally.
                let mut self_addr = ctx.address();
                let _ = self_addr.notify(ContinueScan).await;
            }
            Err(e) => {
                log::error!("[HandleReorg] Error: {}", e);
                let mut self_addr = ctx.address();
                let _ = self_addr.notify(StopScan).await;
            }
        }
    }
}

// --- Pending spends ---

#[async_trait]
impl Notifiable<AddPendingSpends> for WalletActor {
    async fn notify(&mut self, msg: AddPendingSpends, _ctx: &Context<Self>) {
        let now = current_time_secs();
        let spends: Vec<monero_rust::PendingSpend> = msg
            .spends
            .into_iter()
            .map(|s| monero_rust::PendingSpend {
                tx_id: msg.tx_id.clone(),
                key_image: s.key_image,
                output_key: s.output_key,
                amount: s.amount,
                created_at_secs: now,
            })
            .collect();

        self.core_state.add_pending_spends(&msg.tx_id, spends);

        // Advance tracked tx to Broadcast status
        self.core_state
            .advance_tx_status(&msg.tx_id, monero_rust::TxStatus::Broadcast);

        let balance = self.core_state.balance();
        BalanceResponse {
            confirmed: balance.confirmed,
            unconfirmed: balance.unconfirmed,
            pending_spend: balance.pending_spend,
        }
        .send_signal_to_dart();

        TransactionStatusUpdate {
            tx_id: msg.tx_id,
            status: "broadcast".to_string(),
            confirmed_height: None,
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<TrackTransaction> for WalletActor {
    async fn notify(&mut self, msg: TrackTransaction, _ctx: &Context<Self>) {
        let now = current_time_secs();
        let change_outputs: Vec<monero_rust::ChangeOutputRef> = msg.change_outputs;

        self.core_state
            .track_transaction(monero_rust::TrackedTransaction {
                tx_id: msg.tx_id.clone(),
                status: monero_rust::TxStatus::Created,
                spent_key_images: msg.spent_key_images,
                spent_output_keys: msg.spent_output_keys,
                change_outputs,
                fee: msg.fee,
                created_at_secs: now,
            });

        TransactionStatusUpdate {
            tx_id: msg.tx_id,
            status: "created".to_string(),
            confirmed_height: None,
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<AddMempoolPendingSpends> for WalletActor {
    async fn notify(&mut self, msg: AddMempoolPendingSpends, _ctx: &Context<Self>) {
        // For mempool-detected spends, create pending spend entries
        // for any key images that match our outputs and aren't already pending/spent
        let now = current_time_secs();
        let mut added = false;
        for ki in &msg.key_images {
            if self.core_state.is_pending_spent(ki) {
                continue;
            }
            // Check if this key image belongs to one of our outputs
            if let Some(output) = self.core_state.outputs().iter().find(|o| &o.key_image == ki && !o.spent) {
                let tx_id = format!("mempool_{}", ki);
                let spend = monero_rust::PendingSpend {
                    tx_id: tx_id.clone(),
                    key_image: ki.clone(),
                    output_key: output.output_key(),
                    amount: output.amount,
                    created_at_secs: now,
                };
                self.core_state.add_pending_spends(&tx_id, vec![spend]);
                added = true;
            }
        }

        if added {
            let balance = self.core_state.balance();
            BalanceResponse {
                confirmed: balance.confirmed,
                unconfirmed: balance.unconfirmed,
                pending_spend: balance.pending_spend,
            }
            .send_signal_to_dart();
        }
    }
}

// --- .keys file import ---

impl WalletActor {
    async fn listen_to_import_keys_file() {
        let mut receiver = crate::ffi_web::get_import_keys_file_request_receiver();
        while let Some(request) = receiver.recv().await {
            let file_bytes = match hex::decode(&request.file_bytes_hex) {
                Ok(b) => b,
                Err(e) => {
                    ImportKeysFileResponse {
                        success: false,
                        error: Some(format!("hex decode: {e}")),
                        error_code: None, error_hint: None, error_transient: None,
                        spend_secret_key: None, view_secret_key: None,
                        spend_public_key: None, view_public_key: None,
                        creation_timestamp: 0, watch_only: false,
                        seed_language: None, mnemonic: None,
                    }.send_signal_to_dart();
                    continue;
                }
            };

            let result = monero_rust::decrypt_keys_data(&file_bytes, &request.password)
                .and_then(|(plaintext, key, iv)| monero_rust::parse_decrypted_keys(&plaintext, &key, iv));

            match result {
                Ok(imported) => {
                    ImportKeysFileResponse {
                        success: true,
                        error: None, error_code: None, error_hint: None, error_transient: None,
                        spend_secret_key: Some(hex::encode(imported.spend_secret_key)),
                        view_secret_key: Some(hex::encode(imported.view_secret_key)),
                        spend_public_key: Some(hex::encode(imported.spend_public_key)),
                        view_public_key: Some(hex::encode(imported.view_public_key)),
                        creation_timestamp: imported.creation_timestamp,
                        watch_only: imported.watch_only,
                        seed_language: imported.seed_language,
                        mnemonic: imported.mnemonic,
                    }.send_signal_to_dart();
                }
                Err(e) => {
                    ImportKeysFileResponse {
                        success: false,
                        error: Some(e),
                        error_code: None, error_hint: None, error_transient: None,
                        spend_secret_key: None, view_secret_key: None,
                        spend_public_key: None, view_public_key: None,
                        creation_timestamp: 0, watch_only: false,
                        seed_language: None, mnemonic: None,
                    }.send_signal_to_dart();
                }
            }
        }
    }
}

// --- Freeze/Thaw ---

impl WalletActor {
    async fn listen_to_freeze_output(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_freeze_output_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(dart_msg).await;
        }
    }

    async fn listen_to_thaw_output(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_thaw_output_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let _ = self_addr.notify(dart_msg).await;
        }
    }
}

#[async_trait]
impl Notifiable<FreezeOutputRequest> for WalletActor {
    async fn notify(&mut self, msg: FreezeOutputRequest, _ctx: &Context<Self>) {
        let success = self.core_state.freeze_output(&msg.key_image);
        FreezeThawResponse {
            success,
            key_image: msg.key_image,
            frozen: true,
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<ThawOutputRequest> for WalletActor {
    async fn notify(&mut self, msg: ThawOutputRequest, _ctx: &Context<Self>) {
        let success = self.core_state.thaw_output(&msg.key_image);
        FreezeThawResponse {
            success,
            key_image: msg.key_image,
            frozen: false,
        }
        .send_signal_to_dart();
    }
}
