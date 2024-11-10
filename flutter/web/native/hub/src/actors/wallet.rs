use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

pub struct WalletActor {
    state: WalletState,
    sync_actor: Option<Address<super::sync::SyncActor>>,
    rpc_actor: Option<Address<super::rpc::RpcActor>>,
    _owned_tasks: JoinSet<()>,
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
        _owned_tasks.spawn(Self::listen_to_query_daemon_height(self_addr));

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
            sync_actor: None,
            rpc_actor: None,
            _owned_tasks,
        }
    }

    pub fn set_sync_actor(&mut self, addr: Address<super::sync::SyncActor>) {
        self.sync_actor = Some(addr);
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

                    BlockScanResponse {
                        success: true,
                        error: None,
                        block_height: result.block_height,
                        block_hash: result.block_hash,
                        block_timestamp: result.block_timestamp,
                        tx_count: result.tx_count as u32,
                        outputs,
                        daemon_height: result.daemon_height,
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
            if msg.tx_hashes.contains(&output.tx_hash) {
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
