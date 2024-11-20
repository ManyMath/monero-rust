use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Handler, Notifiable};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

pub struct TxBuilderActor {
    wallet_actor: Option<Address<super::wallet::WalletActor>>,
    rpc_actor: Option<Address<super::rpc::RpcActor>>,
    _owned_tasks: JoinSet<()>,
}

impl Actor for TxBuilderActor {}

impl TxBuilderActor {
    pub fn new(self_addr: Address<Self>) -> Self {
        let mut _owned_tasks = JoinSet::new();
        _owned_tasks.spawn(Self::listen_to_tx_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_broadcast_requests(self_addr));

        TxBuilderActor {
            wallet_actor: None,
            rpc_actor: None,
            _owned_tasks,
        }
    }

    pub fn set_wallet_actor(&mut self, addr: Address<super::wallet::WalletActor>) {
        self.wallet_actor = Some(addr);
    }

    pub fn set_rpc_actor(&mut self, addr: Address<super::rpc::RpcActor>) {
        self.rpc_actor = Some(addr);
    }

    async fn listen_to_tx_requests(mut self_addr: Address<Self>) {
        let receiver = CreateTransactionRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            // Convert Vec<Recipient> to Vec<(String, u64)>
            let recipients: Vec<(String, u64)> = request
                .recipients
                .into_iter()
                .map(|r| (r.address, r.amount))
                .collect();
            let _ = self_addr
                .notify(BuildTransaction {
                    node_url: request.node_url,
                    seed: request.seed,
                    network: request.network,
                    recipients,
                    selected_outputs: request.selected_outputs,
                })
                .await;
        }
    }

    async fn listen_to_broadcast_requests(mut self_addr: Address<Self>) {
        let receiver = BroadcastTransactionRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr.notify(BroadcastTransaction {
                node_url: request.node_url,
                tx_blob: request.tx_blob,
                spent_output_hashes: request.spent_output_hashes,
            }).await;
        }
    }
}

#[async_trait]
impl Notifiable<BuildTransaction> for TxBuilderActor {
    async fn notify(&mut self, msg: BuildTransaction, _ctx: &Context<Self>) {
        if let Some(wallet_addr) = &mut self.wallet_actor {
            // Get wallet data and height
            let wallet_data_result = wallet_addr.send(GetWalletData).await;
            let wallet_height_result = wallet_addr.send(GetWalletHeight).await;

            match (wallet_data_result, wallet_height_result) {
                (Ok(wallet_data), Ok(wallet_height)) => {
                    const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;

                    // Filter outputs: only use unspent outputs with >= 10 confirmations
                    let spendable_outputs: Vec<_> = wallet_data
                        .outputs
                        .iter()
                        .filter(|o| {
                            if o.spent {
                                return false;
                            }
                            let confirmations = if wallet_height.daemon_height > o.block_height {
                                wallet_height.daemon_height - o.block_height
                            } else {
                                0
                            };
                            if confirmations < CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE {
                                return false;
                            }
                            // If specific outputs are selected, only include those
                            if let Some(ref selected) = msg.selected_outputs {
                                let output_key = format!("{}:{}", o.tx_hash, o.output_index);
                                return selected.contains(&output_key);
                            }
                            true
                        })
                        .cloned()
                        .collect();

                    if spendable_outputs.is_empty() {
                        let error_msg = if msg.selected_outputs.is_some() {
                            "No selected outputs available to spend".to_string()
                        } else {
                            "No confirmed outputs available to spend (outputs need 10 confirmations)".to_string()
                        };
                        TransactionCreatedResponse {
                            success: false,
                            error: Some(error_msg),
                            tx_id: String::new(),
                            fee: 0,
                            tx_blob: None,
                            spent_output_hashes: Vec::new(),
                            change_outputs: Vec::new(),
                        }
                        .send_signal_to_dart();
                        return;
                    }

                    // Collect output keys (txHash:outputIndex) of outputs that will be spent
                    let spent_hashes: Vec<String> = spendable_outputs
                        .iter()
                        .map(|o| format!("{}:{}", o.tx_hash, o.output_index))
                        .collect();

                    // Spawn transaction building in local task to avoid Send requirements
                    let wallet_data_filtered = WalletData {
                        seed: wallet_data.seed,
                        network: wallet_data.network,
                        outputs: spendable_outputs,
                    };
                    let build_fut = self.build_transaction_impl_inner(msg, wallet_data_filtered);
                    wasm_bindgen_futures::spawn_local(async move {
                        match build_fut.await {
                            Ok((tx_id, fee, tx_blob, change_outputs)) => {
                                #[cfg(target_arch = "wasm32")]
                                web_sys::console::log_1(&format!("Transaction created successfully! TX ID: {}, Fee: {}, Change outputs: {}", tx_id, fee, change_outputs.len()).into());

                                TransactionCreatedResponse {
                                    success: true,
                                    error: None,
                                    tx_id,
                                    fee,
                                    tx_blob: Some(tx_blob),
                                    spent_output_hashes: spent_hashes,
                                    change_outputs,
                                }
                                .send_signal_to_dart();
                            }
                            Err(e) => {
                                #[cfg(target_arch = "wasm32")]
                                web_sys::console::error_1(&format!("Transaction creation failed: {}", e).into());

                                TransactionCreatedResponse {
                                    success: false,
                                    error: Some(e),
                                    tx_id: String::new(),
                                    fee: 0,
                                    tx_blob: None,
                                    spent_output_hashes: Vec::new(),
                                    change_outputs: Vec::new(),
                                }
                                .send_signal_to_dart();
                            }
                        }
                    });
                }
                _ => {
                    TransactionCreatedResponse {
                        success: false,
                        error: Some("Failed to get wallet data or height".to_string()),
                        tx_id: String::new(),
                        fee: 0,
                        tx_blob: None,
                        spent_output_hashes: Vec::new(),
                        change_outputs: Vec::new(),
                    }
                    .send_signal_to_dart();
                }
            }
        } else {
            TransactionCreatedResponse {
                success: false,
                error: Some("Wallet actor not initialized".to_string()),
                tx_id: String::new(),
                fee: 0,
                tx_blob: None,
                spent_output_hashes: Vec::new(),
                change_outputs: Vec::new(),
            }
            .send_signal_to_dart();
        }
    }
}

impl TxBuilderActor {
    fn build_transaction_impl_inner(
        &self,
        msg: BuildTransaction,
        wallet_data: WalletData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(String, u64, String, Vec<ChangeOutput>), String>>>> {
        use monero_wasm::native::{create_transaction, TransactionResult};

        let outputs_vec: Vec<monero_wasm::native::StoredOutputData> = wallet_data
            .outputs
            .iter()
            .map(|o| monero_wasm::native::StoredOutputData {
                tx_hash: o.tx_hash.clone(),
                output_index: o.output_index,
                amount: o.amount,
                key: o.key.clone(),
                key_offset: o.key_offset.clone(),
                commitment_mask: o.commitment_mask.clone(),
                subaddress: o.subaddress,
                payment_id: o.payment_id.clone(),
                received_output_bytes: o.received_output_bytes.clone(),
            })
            .collect();

        Box::pin(async move {
            let result: TransactionResult = create_transaction(
                &msg.node_url,
                &msg.seed,
                &msg.network,
                outputs_vec,
                &msg.recipients,
            )
            .await
            .map_err(|e| format!("Transaction building failed: {}", e))?;

            let change_outputs: Vec<ChangeOutput> = result.change_outputs
                .into_iter()
                .map(|c| ChangeOutput {
                    tx_hash: c.tx_hash,
                    output_index: c.output_index,
                    amount: c.amount,
                    amount_xmr: c.amount_xmr,
                    key: c.key,
                    key_offset: c.key_offset,
                    commitment_mask: c.commitment_mask,
                    subaddress_index: c.subaddress_index,
                    received_output_bytes: c.received_output_bytes,
                    key_image: c.key_image,
                })
                .collect();

            Ok((result.tx_id, result.fee, result.tx_blob, change_outputs))
        })
    }
}

#[async_trait]
impl Notifiable<BroadcastTransaction> for TxBuilderActor {
    async fn notify(&mut self, msg: BroadcastTransaction, _ctx: &Context<Self>) {
        let wallet_actor = self.wallet_actor.clone();
        let spent_hashes = msg.spent_output_hashes.clone();

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"Broadcasting transaction...".into());

        // Spawn in local task to avoid Send requirements
        wasm_bindgen_futures::spawn_local(async move {
            match monero_wasm::native::broadcast_transaction(&msg.node_url, &msg.tx_blob).await {
                Ok(()) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::log_1(&"Transaction broadcast successful!".into());

                    // Mark outputs as spent
                    if let Some(mut wallet) = wallet_actor {
                        let _ = wallet.notify(MarkOutputsSpent {
                            output_keys: spent_hashes,
                        }).await;
                    }

                    TransactionBroadcastResponse {
                        success: true,
                        error: None,
                        tx_id: None,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("Broadcast failed: {}", e).into());

                    TransactionBroadcastResponse {
                        success: false,
                        error: Some(format!("Broadcast failed: {}", e)),
                        tx_id: None,
                    }
                    .send_signal_to_dart();
                }
            }
        });
    }
}
