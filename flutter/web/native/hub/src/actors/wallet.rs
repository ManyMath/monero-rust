use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use std::collections::HashSet;
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;
use wasm_bindgen_futures;

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
    scan_network: String,
    scan_account_lookahead: u32,
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
        _owned_tasks.spawn(Self::listen_to_mempool_scan());
        _owned_tasks.spawn(Self::listen_to_scan_block_multi_wallet());
        _owned_tasks.spawn(Self::listen_to_start_multi_wallet_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_restore_wallet_data(self_addr.clone()));

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
            scan_network: String::new(),
            scan_account_lookahead: 0,
            scan_accounts_to_scan: None,
            multi_wallet_scan_current_height: 0,
            multi_wallet_scan_target_height: 0,
            multi_wallet_scan_node_url: String::new(),
            multi_wallet_scan_wallets: Vec::new(),
        }
    }

    async fn listen_to_create_wallet(mut self_addr: Address<Self>) {
        let receiver = CreateWalletRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr.notify(request).await;
        }
    }

    async fn listen_to_balance_requests(mut self_addr: Address<Self>) {
        let receiver = GetBalanceRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let _ = self_addr.notify(GetBalanceRequest {}).await;
        }
    }

    async fn listen_to_test(_self_addr: Address<Self>) {
        let receiver = MoneroTestRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let result = monero_rust::test_integration();
            MoneroTestResponse { result }.send_signal_to_dart();
        }
    }

    async fn listen_to_generate_seed(_self_addr: Address<Self>) {
        let receiver = GenerateSeedRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
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
                        restore_height,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    SeedGeneratedResponse {
                        seed: String::new(),
                        success: false,
                        error: Some(e),
                        restore_height: None,
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_get_seed_birthday(_self_addr: Address<Self>) {
        let receiver = GetSeedBirthdayRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let birthday = monero_rust::seed_birthday(&request.seed);
            SeedBirthdayResponse {
                birthday,
                success: true,
                error: None,
            }
            .send_signal_to_dart();
        }
    }

    async fn listen_to_get_block_height_from_timestamp(_self_addr: Address<Self>) {
        let receiver = GetBlockHeightFromTimestampRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match Self::find_block_height_for_timestamp(&request.node_url, request.timestamp).await {
                Ok(block_height) => {
                    BlockHeightFromTimestampResponse {
                        block_height,
                        success: true,
                        error: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    BlockHeightFromTimestampResponse {
                        block_height: 0,
                        success: false,
                        error: Some(e),
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

    /// Make a JSON-RPC call to a Monero daemon.
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
        let receiver = DeriveAddressRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match monero_rust::derive_address(&request.seed, &request.network) {
                Ok(address) => {
                    AddressDerivedResponse {
                        address,
                        success: true,
                        error: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    AddressDerivedResponse {
                        address: String::new(),
                        success: false,
                        error: Some(e),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_derive_subaddress(_self_addr: Address<Self>) {
        let receiver = DeriveSubaddressRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match monero_rust::derive_subaddress(
                &request.seed,
                &request.network,
                request.account,
                request.address_index,
            ) {
                Ok(address) => {
                    SubaddressDerivedResponse {
                        address,
                        success: true,
                        error: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    SubaddressDerivedResponse {
                        address: String::new(),
                        success: false,
                        error: Some(e),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_derive_keys(_self_addr: Address<Self>) {
        let receiver = DeriveKeysRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match monero_rust::derive_keys(&request.seed, &request.network) {
                Ok(keys) => {
                    KeysDerivedResponse {
                        address: keys.address,
                        secret_spend_key: keys.secret_spend_key,
                        secret_view_key: keys.secret_view_key,
                        public_spend_key: keys.public_spend_key,
                        public_view_key: keys.public_view_key,
                        success: true,
                        error: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    KeysDerivedResponse {
                        address: String::new(),
                        secret_spend_key: String::new(),
                        secret_view_key: String::new(),
                        public_spend_key: String::new(),
                        public_view_key: String::new(),
                        success: false,
                        error: Some(e),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_scan_block(mut self_addr: Address<Self>) {
        let receiver = ScanBlockRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            let seed = request.seed.clone();
            let network = request.network.clone();

            match monero_rust::scan_block_for_outputs_with_url(
                &request.node_url,
                request.block_height,
                &request.seed,
                &request.network,
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
                        })
                        .await;

                    if !result.spent_key_images.is_empty() {
                        let _ = self_addr.notify(UpdateSpentStatus {
                            key_images: result.spent_key_images.clone(),
                        }).await;
                    }

                    BlockScanResponse {
                        success: true,
                        error: None,
                        block_height: result.block_height,
                        block_hash: result.block_hash,
                        block_timestamp: result.block_timestamp,
                        tx_count: result.tx_count as u32,
                        outputs,
                        daemon_height: result.daemon_height,
                        spent_key_images: result.spent_key_images.clone(),
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    BlockScanResponse {
                        success: false,
                        error: Some(e),
                        block_height: request.block_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_query_daemon_height(_self_addr: Address<Self>) {
        let receiver = QueryDaemonHeightRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            match monero_rust::get_daemon_height(&request.node_url).await {
                Ok(height) => {
                    DaemonHeightResponse {
                        success: true,
                        error: None,
                        daemon_height: height,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    DaemonHeightResponse {
                        success: false,
                        error: Some(e),
                        daemon_height: 0,
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_start_continuous_scan(mut self_addr: Address<Self>) {
        let receiver = StartContinuousScanRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr
                .notify(StartContinuousScan {
                    node_url: request.node_url,
                    start_height: request.start_height,
                    seed: request.seed,
                    network: request.network,
                    account_lookahead: request.account_lookahead,
                    accounts_to_scan: request.accounts_to_scan,
                })
                .await;
        }
    }

    async fn listen_to_stop_scan(mut self_addr: Address<Self>) {
        let receiver = StopScanRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let _ = self_addr.notify(StopScan).await;
        }
    }

    async fn listen_to_mempool_scan() {
        let receiver = MempoolScanRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            wasm_bindgen_futures::spawn_local(async move {
                match monero_rust::scan_mempool_for_outputs_with_account_lookahead(
                    &request.node_url,
                    &request.seed,
                    &request.network,
                    request.account_lookahead,
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

                        MempoolScanResponse {
                            success: true,
                            error: None,
                            tx_count: result.tx_count as u32,
                            outputs,
                            spent_key_images: result.spent_key_images,
                        }
                        .send_signal_to_dart();
                    }
                    Err(e) => {
                        MempoolScanResponse {
                            success: false,
                            error: Some(e),
                            tx_count: 0,
                            outputs: Vec::new(),
                            spent_key_images: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_scan_block_multi_wallet() {
        let receiver = ScanBlockMultiWalletRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            wasm_bindgen_futures::spawn_local(async move {
                // Convert wallet configs to the format expected by the scanner
                let wallet_configs: Vec<monero_rust::WalletScanConfig> = request
                    .wallets
                    .iter()
                    .map(|w| monero_rust::WalletScanConfig {
                        mnemonic: w.seed.clone(),
                        network: w.network.clone(),
                        lookahead: monero_rust::Lookahead {
                            account: w.account_lookahead,
                            subaddress: 20,
                        },
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
                            block_height: result.block_height,
                            block_hash: result.block_hash,
                            block_timestamp: result.block_timestamp,
                            tx_count: result.tx_count as u32,
                            daemon_height: result.daemon_height,
                            spent_key_images: result.spent_key_images,
                            wallet_results,
                        }
                        .send_signal_to_dart();
                    }
                    Err(e) => {
                        MultiWalletScanResponse {
                            success: false,
                            error: Some(e),
                            block_height: request.block_height,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            wallet_results: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_start_multi_wallet_scan(self_addr: Address<Self>) {
        let receiver = StartMultiWalletScanRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let node_url = request.node_url.clone();
            let start_height = request.start_height;
            let wallets = request.wallets.clone();
            let mut self_addr_clone = self_addr.clone();

            // Spawn task to get daemon height and start scanning
            wasm_bindgen_futures::spawn_local(async move {
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
                        MultiWalletScanResponse {
                            success: false,
                            error: Some(format!("Failed to get daemon height: {}", e)),
                            block_height: 0,
                            block_hash: String::new(),
                            block_timestamp: 0,
                            tx_count: 0,
                            daemon_height: 0,
                            spent_key_images: Vec::new(),
                            wallet_results: Vec::new(),
                        }
                        .send_signal_to_dart();
                    }
                }
            });
        }
    }

    async fn listen_to_restore_wallet_data(mut self_addr: Address<Self>) {
        let receiver = RestoreWalletDataRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let msg = signal_pack.message;
            let outputs: Vec<monero_rust::WalletOutput> = msg.outputs.into_iter().map(|o| o.into()).collect();
            let _ = self_addr.notify(RestoreOutputs {
                seed: msg.seed,
                network: msg.network,
                outputs,
                daemon_height: msg.daemon_height,
                current_height: msg.current_height,
            }).await;
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
}

#[async_trait]
impl Notifiable<RestoreOutputs> for WalletActor {
    async fn notify(&mut self, msg: RestoreOutputs, _ctx: &Context<Self>) {
        self.seed = Some(msg.seed);
        self.network = Some(msg.network);
        self.core_state.daemon_height = msg.daemon_height;
        self.core_state.current_height = msg.current_height;
        self.core_state.replace_outputs(msg.outputs);
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
        }
        .send_signal_to_dart();
    }
}
#[async_trait]
impl Notifiable<StoreOutputs> for WalletActor {
    async fn notify(&mut self, msg: StoreOutputs, _ctx: &Context<Self>) {
        self.seed = Some(msg.seed);
        self.network = Some(msg.network);
        self.core_state.daemon_height = msg.daemon_height;
        self.core_state.add_outputs(msg.outputs);
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
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&"Stopping multi-wallet scan to transition to single-wallet scan".into());

            // Stop the multi-wallet scan
            self.notify(StopScan {}, ctx).await;
        }

        self.is_scanning = msg.is_scanning;
        self.active_scan_type = if msg.is_scanning { ScanType::SingleWallet } else { ScanType::None };
        self.scan_current_height = msg.current_height;
        self.scan_target_height = msg.target_height;
        self.scan_node_url = msg.node_url;
        self.scan_seed = msg.seed;
        self.scan_network = msg.network;
        self.scan_account_lookahead = msg.account_lookahead;
        self.scan_accounts_to_scan = msg.accounts_to_scan;
    }
}

#[async_trait]
impl Notifiable<UpdateMultiWalletScanState> for WalletActor {
    async fn notify(&mut self, msg: UpdateMultiWalletScanState, ctx: &Context<Self>) {
        // If single-wallet scan is active, stop it first to allow transition
        if self.is_scanning && self.active_scan_type == ScanType::SingleWallet {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&"Stopping single-wallet scan to transition to multi-wallet scan".into());

            // Stop the single-wallet scan
            self.notify(StopScan {}, ctx).await;
        }

        self.is_scanning = msg.is_scanning;
        self.active_scan_type = if msg.is_scanning { ScanType::MultiWallet } else { ScanType::None };
        self.multi_wallet_scan_current_height = msg.current_height;
        self.multi_wallet_scan_target_height = msg.target_height;
        self.multi_wallet_scan_node_url = msg.node_url;
        self.multi_wallet_scan_wallets = msg.wallets;
    }
}

#[async_trait]
impl Notifiable<StartContinuousScan> for WalletActor {
    async fn notify(&mut self, msg: StartContinuousScan, ctx: &Context<Self>) {
        let node_url = msg.node_url.clone();
        let start_height = msg.start_height;
        let seed = msg.seed.clone();
        let network = msg.network.clone();
        let account_lookahead = msg.account_lookahead;
        let mut self_addr = ctx.address();

        wasm_bindgen_futures::spawn_local(async move {
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
                            network: network.clone(),
                            account_lookahead,
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
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("[StartContinuousScan] Failed to get daemon height: {}", e).into());

                    BlockScanResponse {
                        success: false,
                        error: Some(format!("Failed to get daemon height: {}", e)),
                        block_height: 0,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
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
        let network = self.scan_network.clone();
        let account_lookahead = self.scan_account_lookahead;
        let accounts_to_scan = self.scan_accounts_to_scan.clone();
        let target_height = self.scan_target_height;
        let mut self_addr = ctx.address();

        // For batch scanning, we don't increment by 1 here.
        // The batch result will tell us how many blocks were fetched.
        // Set current_height to target to prevent re-entry; the async task
        // will update it via UpdateScanState or direct notify.
        self.scan_current_height = self.scan_target_height;

        wasm_bindgen_futures::spawn_local(async move {
            let max_account = if let Some(ref accounts) = accounts_to_scan {
                accounts.iter().max().copied().unwrap_or(0)
            } else {
                account_lookahead
            };

            let lookahead = monero_rust::Lookahead {
                account: max_account,
                subaddress: monero_rust::DEFAULT_LOOKAHEAD.subaddress,
            };

            // Fetch and scan a batch of blocks (~1000) in one RPC call
            match monero_rust::scan_blocks_batch_with_url(
                &node_url,
                batch_start_height,
                &seed,
                &network,
                lookahead,
            )
            .await
            {
                Ok(batch_results) => {
                    if batch_results.is_empty() {
                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }

                    let batch_end_height = batch_results
                        .last()
                        .map(|r| r.block_height + 1)
                        .unwrap_or(batch_start_height);

                    // Accumulate spent key images across the batch for a single update
                    let mut all_spent_key_images = Vec::new();
                    let mut all_stored_outputs = Vec::new();
                    let mut last_daemon_height = 0u64;

                    let accounts_set: Option<HashSet<u32>> = accounts_to_scan.as_ref().map(|a| a.iter().copied().collect());

                    for result in &batch_results {
                        last_daemon_height = result.daemon_height;

                        // Collect spent key images
                        all_spent_key_images.extend(result.spent_key_images.iter().cloned());

                        // Filter and collect outputs
                        let filtered_outputs: Vec<&monero_rust::WalletOutput> = result
                            .outputs
                            .iter()
                            .filter(|o| {
                                if let Some(ref accounts) = accounts_set {
                                    if let Some((account, _)) = o.subaddress_index {
                                        accounts.contains(&account)
                                    } else {
                                        accounts.contains(&0)
                                    }
                                } else {
                                    true
                                }
                            })
                            .collect();

                        // Only send BlockScanResponse for blocks that have outputs
                        if !filtered_outputs.is_empty() {
                            let mut outputs: Vec<OwnedOutput> = Vec::with_capacity(filtered_outputs.len());
                            for o in &filtered_outputs {
                                outputs.push((*o).into());
                                all_stored_outputs.push((*o).clone());
                            }

                            BlockScanResponse {
                                success: true,
                                error: None,
                                block_height: result.block_height,
                                block_hash: result.block_hash.clone(),
                                block_timestamp: result.block_timestamp,
                                tx_count: result.tx_count as u32,
                                outputs,
                                daemon_height: result.daemon_height,
                                spent_key_images: result.spent_key_images.clone(),
                            }
                            .send_signal_to_dart();
                        }
                    }

                    // Store all outputs from the batch in one message
                    if !all_stored_outputs.is_empty() {
                        let _ = self_addr
                            .notify(StoreOutputs {
                                seed: seed.clone(),
                                network: network.clone(),
                                outputs: all_stored_outputs,
                                daemon_height: last_daemon_height,
                            })
                            .await;
                    }

                    // Update spent status once for the whole batch
                    if !all_spent_key_images.is_empty() {
                        let _ = self_addr
                            .notify(UpdateSpentStatus {
                                key_images: all_spent_key_images,
                            })
                            .await;
                    }

                    // Send progress update for the whole batch
                    SyncProgressResponse {
                        current_height: batch_end_height,
                        daemon_height: target_height,
                        is_synced: batch_end_height >= target_height,
                        is_scanning: batch_end_height < target_height,
                    }
                    .send_signal_to_dart();

                    // Update the actor's scan height and continue or stop
                    let _ = self_addr
                        .notify(UpdateScanState {
                            is_scanning: batch_end_height < target_height,
                            current_height: batch_end_height,
                            target_height,
                            node_url,
                            seed,
                            network,
                            account_lookahead,
                            accounts_to_scan,
                        })
                        .await;

                    if batch_end_height < target_height {
                        let _ = self_addr.notify(ContinueScan).await;
                    } else {
                        let _ = self_addr.notify(StopScan).await;
                    }
                }
                Err(e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(
                        &format!(
                            "[ContinueScan] Batch scan error at height {}: {}",
                            batch_start_height, e
                        )
                        .into(),
                    );

                    BlockScanResponse {
                        success: false,
                        error: Some(e),
                        block_height: batch_start_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
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

        // Prevent re-entry while batch is in flight
        self.multi_wallet_scan_current_height = self.multi_wallet_scan_target_height;

        wasm_bindgen_futures::spawn_local(async move {
            let wallet_configs: Vec<monero_rust::WalletScanConfig> = wallets
                .iter()
                .map(|w| {
                    let max_account = if let Some(ref accounts) = w.accounts_to_scan {
                        accounts.iter().max().copied().unwrap_or(0)
                    } else {
                        w.account_lookahead
                    };
                    monero_rust::WalletScanConfig {
                        mnemonic: w.seed.clone(),
                        network: w.network.clone(),
                        lookahead: monero_rust::Lookahead {
                            account: max_account,
                            subaddress: 20,
                        },
                    }
                })
                .collect();

            // Batch-fetch and scan ~1000 blocks for all wallets at once
            match monero_rust::scan_blocks_batch_multi_wallet_with_url(
                &node_url,
                batch_start_height,
                wallet_configs,
            )
            .await
            {
                Ok(batch_results) => {
                    if batch_results.is_empty() {
                        let _ = self_addr.notify(StopScan).await;
                        return;
                    }

                    let batch_end_height = batch_results
                        .last()
                        .map(|r| r.block_height + 1)
                        .unwrap_or(batch_start_height);

                    let wallet_accounts: Vec<Option<HashSet<u32>>> = wallets
                        .iter()
                        .map(|w| w.accounts_to_scan.as_ref().map(|a| a.iter().copied().collect()))
                        .collect();

                    for result in &batch_results {
                        // Filter outputs per wallet and send signal for blocks with outputs
                        let wallet_results: Vec<WalletScanResult> = result
                            .wallet_results
                            .iter()
                            .enumerate()
                            .map(|(idx, (address, wallet_data))| {
                                let accounts_filter =
                                    wallet_accounts.get(idx).and_then(|a| a.as_ref());

                                let outputs = wallet_data
                                    .outputs
                                    .iter()
                                    .filter(|o| {
                                        if let Some(accounts) = accounts_filter {
                                            if let Some((account, _)) = o.subaddress_index {
                                                accounts.contains(&account)
                                            } else {
                                                accounts.contains(&0)
                                            }
                                        } else {
                                            true
                                        }
                                    })
                                    .map(|o| o.into())
                                    .collect();

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
                                block_height: result.block_height,
                                block_hash: result.block_hash.clone(),
                                block_timestamp: result.block_timestamp,
                                tx_count: result.tx_count as u32,
                                daemon_height: result.daemon_height,
                                spent_key_images: result.spent_key_images.clone(),
                                wallet_results,
                            }
                            .send_signal_to_dart();
                        }
                    }

                    // Send progress for the whole batch
                    SyncProgressResponse {
                        current_height: batch_end_height,
                        daemon_height: target_height,
                        is_synced: batch_end_height >= target_height,
                        is_scanning: batch_end_height < target_height,
                    }
                    .send_signal_to_dart();

                    // Update scan state and continue
                    let _ = self_addr
                        .notify(UpdateMultiWalletScanState {
                            is_scanning: batch_end_height < target_height,
                            current_height: batch_end_height,
                            target_height,
                            node_url,
                            wallets,
                        })
                        .await;

                    if batch_end_height < target_height {
                        let _ = self_addr.notify(ContinueMultiWalletScan).await;
                    } else {
                        let _ = self_addr.notify(StopScan).await;
                    }
                }
                Err(e) => {
                    MultiWalletScanResponse {
                        success: false,
                        error: Some(e),
                        block_height: batch_start_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
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
impl Notifiable<UpdateSpentStatus> for WalletActor {
    async fn notify(&mut self, msg: UpdateSpentStatus, _ctx: &Context<Self>) {
        let updated_count = self.core_state.mark_spent_by_key_images(&msg.key_images);

        if updated_count > 0 {
            let balance = self.core_state.balance();

            BalanceResponse {
                confirmed: balance.confirmed,
                unconfirmed: balance.unconfirmed,
            }
            .send_signal_to_dart();

            SpentStatusUpdatedResponse {
                spent_key_images: msg.key_images.clone(),
            }
            .send_signal_to_dart();
        }
    }
}
