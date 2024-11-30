use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;
use wasm_bindgen_futures;

pub struct WalletActor {
    state: WalletState,
    rpc_actor: Option<Address<super::rpc::RpcActor>>,
    _owned_tasks: JoinSet<()>,
    // Continuous scan state
    is_scanning: bool,
    scan_start_height: u64,
    scan_current_height: u64,
    scan_target_height: u64,
    scan_node_url: String,
    scan_seed: String,
    scan_network: String,
    self_addr: Option<Address<Self>>,
}

impl Actor for WalletActor {}

impl WalletActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_create_wallet(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_balance_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_test(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_generate_seed(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_derive_address(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_derive_keys(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_scan_block(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_query_daemon_height(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_start_continuous_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_stop_scan(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_mempool_scan());

        WalletActor {
            state: WalletState {
                address: String::new(),
                current_height: 0,
                daemon_height: 0,
                confirmed_balance: 0,
                unconfirmed_balance: 0,
                seed: None,
                network: None,
                outputs: Vec::new(),
            },
            rpc_actor: None,
            _owned_tasks,
            is_scanning: false,
            scan_start_height: 0,
            scan_current_height: 0,
            scan_target_height: 0,
            scan_node_url: String::new(),
            scan_seed: String::new(),
            scan_network: String::new(),
            self_addr: Some(self_addr),
        }
    }

    pub fn set_rpc_actor(&mut self, addr: Address<super::rpc::RpcActor>) {
        self.rpc_actor = Some(addr);
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

    async fn listen_to_test(mut self_addr: Address<Self>) {
        let receiver = MoneroTestRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let result = monero_wasm::test_integration();
            MoneroTestResponse { result }.send_signal_to_dart();
        }
    }

    async fn listen_to_generate_seed(mut self_addr: Address<Self>) {
        let receiver = GenerateSeedRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            match monero_wasm::generate_seed() {
                Ok(seed) => {
                    SeedGeneratedResponse {
                        seed,
                        success: true,
                        error: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    SeedGeneratedResponse {
                        seed: String::new(),
                        success: false,
                        error: Some(e),
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_derive_address(mut self_addr: Address<Self>) {
        let receiver = DeriveAddressRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match monero_wasm::derive_address(&request.seed, &request.network) {
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

    async fn listen_to_derive_keys(mut self_addr: Address<Self>) {
        let receiver = DeriveKeysRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            match monero_wasm::derive_keys(&request.seed, &request.network) {
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

            match monero_wasm::scan_block_for_outputs_with_url(
                &request.node_url,
                request.block_height,
                &request.seed,
                &request.network,
            )
            .await
            {
                Ok(result) => {
                    let outputs = result
                        .outputs
                        .iter()
                        .map(|o| OwnedOutput {
                            tx_hash: o.tx_hash.clone(),
                            output_index: o.output_index,
                            amount: o.amount,
                            amount_xmr: o.amount_xmr.clone(),
                            key: o.key.clone(),
                            key_offset: o.key_offset.clone(),
                            commitment_mask: o.commitment_mask.clone(),
                            subaddress_index: o.subaddress_index,
                            payment_id: o.payment_id.clone(),
                            received_output_bytes: o.received_output_bytes.clone(),
                            block_height: o.block_height,
                            spent: o.spent,
                            key_image: o.key_image.clone(),
                        })
                        .collect();

                    let stored_outputs: Vec<StoredOutput> = result
                        .outputs
                        .iter()
                        .map(|o| StoredOutput {
                            tx_hash: o.tx_hash.clone(),
                            output_index: o.output_index,
                            amount: o.amount,
                            key: o.key.clone(),
                            key_offset: o.key_offset.clone(),
                            commitment_mask: o.commitment_mask.clone(),
                            subaddress: o.subaddress_index,
                            payment_id: o.payment_id.clone(),
                            received_output_bytes: o.received_output_bytes.clone(),
                            block_height: o.block_height,
                            spent: o.spent,
                            key_image: o.key_image.clone(),
                        })
                        .collect();

                    let _ = self_addr
                        .notify(StoreOutputs {
                            seed,
                            network,
                            outputs: stored_outputs,
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

            match monero_wasm::get_daemon_height(&request.node_url).await {
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
                })
                .await;
        }
    }

    async fn listen_to_stop_scan(mut self_addr: Address<Self>) {
        let receiver = StopScanRequest::get_dart_signal_receiver();
        while let Some(_signal_pack) = receiver.recv().await {
            let _ = self_addr.notify(messages::StopScan).await;
        }
    }

    async fn listen_to_mempool_scan() {
        let receiver = MempoolScanRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            wasm_bindgen_futures::spawn_local(async move {
                match monero_wasm::scan_mempool_for_outputs(
                    &request.node_url,
                    &request.seed,
                    &request.network,
                )
                .await
                {
                    Ok(result) => {
                        let outputs = result
                            .outputs
                            .iter()
                            .map(|o| OwnedOutput {
                                tx_hash: o.tx_hash.clone(),
                                output_index: o.output_index,
                                amount: o.amount,
                                amount_xmr: o.amount_xmr.clone(),
                                key: o.key.clone(),
                                key_offset: o.key_offset.clone(),
                                commitment_mask: o.commitment_mask.clone(),
                                subaddress_index: o.subaddress_index,
                                payment_id: o.payment_id.clone(),
                                received_output_bytes: o.received_output_bytes.clone(),
                                block_height: 0, // Unconfirmed - in mempool
                                spent: o.spent,
                                key_image: o.key_image.clone(),
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

}

#[async_trait]
impl Notifiable<CreateWalletRequest> for WalletActor {
    async fn notify(&mut self, msg: CreateWalletRequest, _ctx: &Context<Self>) {
        self.state.address = format!("4{}_placeholder", msg.network);
        WalletCreatedResponse {
            address: self.state.address.clone(),
        }
        .send_signal_to_dart();
    }
}

#[async_trait]
impl Notifiable<UpdateBalance> for WalletActor {
    async fn notify(&mut self, msg: UpdateBalance, _ctx: &Context<Self>) {
        self.state.confirmed_balance = msg.confirmed;
        self.state.unconfirmed_balance = msg.unconfirmed;
    }
}

#[async_trait]
impl Notifiable<GetBalanceRequest> for WalletActor {
    async fn notify(&mut self, _msg: GetBalanceRequest, _ctx: &Context<Self>) {
        BalanceResponse {
            confirmed: self.state.confirmed_balance,
            unconfirmed: self.state.unconfirmed_balance,
        }
        .send_signal_to_dart();
    }
}
#[async_trait]
impl Notifiable<StoreOutputs> for WalletActor {
    async fn notify(&mut self, msg: StoreOutputs, _ctx: &Context<Self>) {
        self.state.seed = Some(msg.seed);
        self.state.network = Some(msg.network);
        self.state.daemon_height = msg.daemon_height;

        // Update current_height to the highest block_height among new outputs
        for output in &msg.outputs {
            if output.block_height > self.state.current_height {
                self.state.current_height = output.block_height;
            }
        }

        self.state.outputs.extend(msg.outputs);

        // Recalculate balances after adding outputs
        self.recalculate_balances();
    }
}

#[async_trait]
impl Handler<GetWalletData> for WalletActor {
    type Result = WalletData;

    async fn handle(&mut self, _msg: GetWalletData, _ctx: &Context<Self>) -> Self::Result {
        WalletData {
            seed: self.state.seed.clone(),
            network: self.state.network.clone(),
            outputs: self.state.outputs.clone(),
        }
    }
}

#[async_trait]
impl Notifiable<MarkOutputsSpent> for WalletActor {
    async fn notify(&mut self, msg: MarkOutputsSpent, _ctx: &Context<Self>) {
        for output in &mut self.state.outputs {
            let output_key = format!("{}:{}", output.tx_hash, output.output_index);
            if msg.output_keys.contains(&output_key) {
                output.spent = true;
            }
        }

        // Recalculate balances after marking outputs as spent
        self.recalculate_balances();
    }
}

#[async_trait]
impl Handler<GetWalletHeight> for WalletActor {
    type Result = WalletHeight;

    async fn handle(&mut self, _msg: GetWalletHeight, _ctx: &Context<Self>) -> Self::Result {
        WalletHeight {
            current_height: self.state.current_height,
            daemon_height: self.state.daemon_height,
        }
    }
}

impl WalletActor {
    fn recalculate_balances(&mut self) {
        const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;

        let mut confirmed = 0u64;
        let mut unconfirmed = 0u64;

        for output in &self.state.outputs {
            if output.spent {
                continue;
            }

            let confirmations = if self.state.current_height > output.block_height {
                self.state.current_height - output.block_height
            } else {
                0
            };

            if confirmations >= CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE {
                confirmed += output.amount;
            } else {
                unconfirmed += output.amount;
            }
        }

        self.state.confirmed_balance = confirmed;
        self.state.unconfirmed_balance = unconfirmed;
    }
}

#[async_trait]
impl Notifiable<UpdateScanState> for WalletActor {
    async fn notify(&mut self, msg: UpdateScanState, _ctx: &Context<Self>) {
        self.is_scanning = msg.is_scanning;
        self.scan_current_height = msg.current_height;
        self.scan_target_height = msg.target_height;
        self.scan_node_url = msg.node_url;
        self.scan_seed = msg.seed;
        self.scan_network = msg.network;
    }
}

#[async_trait]
impl Notifiable<StartContinuousScan> for WalletActor {
    async fn notify(&mut self, msg: StartContinuousScan, ctx: &Context<Self>) {
        let node_url = msg.node_url.clone();
        let start_height = msg.start_height;
        let seed = msg.seed.clone();
        let network = msg.network.clone();
        let mut self_addr = ctx.address();

        wasm_bindgen_futures::spawn_local(async move {
            match monero_wasm::get_daemon_height(&node_url).await {
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
        SyncProgressResponse {
            current_height: self.scan_current_height,
            daemon_height: self.scan_target_height,
            is_synced: self.scan_current_height >= self.scan_target_height,
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
        let block_height = self.scan_current_height;
        let seed = self.scan_seed.clone();
        let network = self.scan_network.clone();
        let target_height = self.scan_target_height;
        let mut self_addr = ctx.address();

        // Increment current height
        self.scan_current_height += 1;

        wasm_bindgen_futures::spawn_local(async move {
            match monero_wasm::scan_block_for_outputs_with_url(&node_url, block_height, &seed, &network).await {
                Ok(result) => {
                    let outputs = result
                        .outputs
                        .iter()
                        .map(|o| OwnedOutput {
                            tx_hash: o.tx_hash.clone(),
                            output_index: o.output_index,
                            amount: o.amount,
                            amount_xmr: o.amount_xmr.clone(),
                            key: o.key.clone(),
                            key_offset: o.key_offset.clone(),
                            commitment_mask: o.commitment_mask.clone(),
                            subaddress_index: o.subaddress_index,
                            payment_id: o.payment_id.clone(),
                            received_output_bytes: o.received_output_bytes.clone(),
                            block_height: o.block_height,
                            spent: o.spent,
                            key_image: o.key_image.clone(),
                        })
                        .collect();

                    let stored_outputs: Vec<StoredOutput> = result
                        .outputs
                        .iter()
                        .map(|o| StoredOutput {
                            tx_hash: o.tx_hash.clone(),
                            output_index: o.output_index,
                            amount: o.amount,
                            key: o.key.clone(),
                            key_offset: o.key_offset.clone(),
                            commitment_mask: o.commitment_mask.clone(),
                            subaddress: o.subaddress_index,
                            payment_id: o.payment_id.clone(),
                            received_output_bytes: o.received_output_bytes.clone(),
                            block_height: o.block_height,
                            spent: o.spent,
                            key_image: o.key_image.clone(),
                        })
                        .collect();

                    let _ = self_addr
                        .notify(StoreOutputs {
                            seed,
                            network,
                            outputs: stored_outputs,
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

                    // Send progress update
                    SyncProgressResponse {
                        current_height: block_height + 1,
                        daemon_height: target_height,
                        is_synced: (block_height + 1) >= target_height,
                        is_scanning: (block_height + 1) < target_height,
                    }
                    .send_signal_to_dart();

                    // Continue scanning if not done, otherwise start polling
                    if block_height + 1 < target_height {
                        let _ = self_addr.notify(ContinueScan).await;
                    } else {
                        // Scanning complete.  Dart handles starting polling timers
                        #[cfg(target_arch = "wasm32")]
                        web_sys::console::log_1(&"[ContinueScan] Scan complete!".into());

                        let _ = self_addr.notify(StopScan).await;
                    }
                }
                Err(e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("[ContinueScan] Scan error at height {}: {}", block_height, e).into());

                    BlockScanResponse {
                        success: false,
                        error: Some(e),
                        block_height,
                        block_hash: String::new(),
                        block_timestamp: 0,
                        tx_count: 0,
                        outputs: Vec::new(),
                        daemon_height: 0,
                        spent_key_images: Vec::new(),
                    }
                    .send_signal_to_dart();

                    // Stop scanning on error
                    SyncProgressResponse {
                        current_height: block_height,
                        daemon_height: target_height,
                        is_synced: false,
                        is_scanning: false,
                    }
                    .send_signal_to_dart();

                    // Notify actor to stop scanning
                    let _ = self_addr.notify(StopScan).await;
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<UpdateSpentStatus> for WalletActor {
    async fn notify(&mut self, msg: UpdateSpentStatus, _ctx: &Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("[UpdateSpentStatus] Received {} key images to mark as spent", msg.key_images.len()).into());
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("[UpdateSpentStatus] Current outputs in wallet: {}", self.state.outputs.len()).into());

        let mut updated_count = 0;
        for output in &mut self.state.outputs {
            if !output.spent && msg.key_images.contains(&output.key_image) {
                #[cfg(target_arch = "wasm32")]
                web_sys::console::log_1(&format!("[UpdateSpentStatus] Marking output as spent: {}...", &output.key_image[..16.min(output.key_image.len())]).into());
                output.spent = true;
                updated_count += 1;
            }
        }

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("[UpdateSpentStatus] Updated {} outputs as spent", updated_count).into());

        if updated_count > 0 {
            self.recalculate_balances();
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&format!("[UpdateSpentStatus] Recalculated balance: confirmed={}, unconfirmed={}", self.state.confirmed_balance, self.state.unconfirmed_balance).into());

            BalanceResponse {
                confirmed: self.state.confirmed_balance,
                unconfirmed: self.state.unconfirmed_balance,
            }
            .send_signal_to_dart();

            SpentStatusUpdatedResponse {
                spent_key_images: msg.key_images.clone(),
            }
            .send_signal_to_dart();
        }
    }
}
