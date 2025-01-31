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
        _owned_tasks.spawn(Self::listen_to_broadcast_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_proof_requests(self_addr));

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

    async fn listen_to_proof_requests(_self_addr: Address<Self>) {
        let receiver = GenerateOutProofRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;

            // Generate the proof (network is passed as string)
            match monero_rust::tx_proof::generate_out_proof_v2(
                &request.tx_id,
                &request.tx_key,
                &request.recipient_address,
                &request.message,
                &request.network,
            ) {
                Ok(result) => {
                    OutProofGeneratedResponse {
                        success: true,
                        error: None,
                        signature: Some(result.signature),
                        formatted: Some(result.formatted),
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    OutProofGeneratedResponse {
                        success: false,
                        error: Some(e),
                        signature: None,
                        formatted: None,
                    }
                    .send_signal_to_dart();
                }
            }
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
                    let mut spendable_outputs: Vec<_> = wallet_data
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
                            confirmations >= CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE
                        })
                        .cloned()
                        .collect();

                    // If specific outputs are manually selected, use only those
                    if let Some(ref selected) = msg.selected_outputs {
                        spendable_outputs.retain(|o| {
                            let output_key = format!("{}:{}", o.tx_hash, o.output_index);
                            selected.contains(&output_key)
                        });
                    } else {
                        // No manual selection: use smart input selection to avoid linking all outputs
                        // Strategy: Use the smallest single output that can cover the amount,
                        // or if no single output suffices, combine minimum number of outputs

                        let total_send_amount: u64 = msg.recipients.iter().map(|(_, amt)| amt).sum();

                        // Fee estimates (conservative)
                        const FEE_PER_INPUT_ESTIMATE: u64 = 15_000_000; // ~0.015 XMR per input
                        const BASE_FEE_ESTIMATE: u64 = 20_000_000; // ~0.02 XMR base fee

                        // First, try to find the smallest single output that can cover the transaction
                        let single_input_fee = BASE_FEE_ESTIMATE + FEE_PER_INPUT_ESTIMATE;
                        let needed_for_single = total_send_amount + single_input_fee;

                        // Sort by amount to find candidates
                        spendable_outputs.sort_by_key(|o| o.amount);

                        // Find the smallest output that's >= needed amount
                        let single_output = spendable_outputs.iter()
                            .find(|o| o.amount >= needed_for_single);

                        if let Some(output) = single_output {
                            // Found a single output that can cover it: only use it
                            spendable_outputs = vec![output.clone()];
                        } else {
                            // No single output works: find optimal combination
                            // Strategy: Find combinations with minimum number of inputs,
                            // then among those, pick the one with smallest total value

                            // Sort by amount descending for efficient searching
                            spendable_outputs.sort_by_key(|o| std::cmp::Reverse(o.amount));

                            let mut best_selection: Option<Vec<StoredOutput>> = None;
                            let mut best_count = usize::MAX;
                            let mut best_total = u64::MAX;

                            // Try different numbers of inputs, starting from 2
                            for target_count in 2..=spendable_outputs.len() {
                                let estimated_fee = BASE_FEE_ESTIMATE + (target_count as u64 * FEE_PER_INPUT_ESTIMATE);
                                let needed_total = total_send_amount + estimated_fee;

                                // Try to find a combination of exactly target_count outputs
                                if let Some((selection, total)) = Self::find_best_combination(
                                    &spendable_outputs,
                                    needed_total,
                                    target_count,
                                ) {
                                    // Found a valid combination with this many inputs
                                    if target_count < best_count || (target_count == best_count && total < best_total) {
                                        best_selection = Some(selection);
                                        best_count = target_count;
                                        best_total = total;
                                        // Found minimum number of inputs, no need to try more
                                        break;
                                    }
                                }
                            }

                            if let Some(selection) = best_selection {
                                spendable_outputs = selection;
                            }
                            // else: keep all outputs, will fail later with proper error
                        }
                    }

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
                            tx_key: None,
                            tx_key_additional: Vec::new(),
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
                            Ok((tx_id, fee, tx_blob, tx_key, tx_key_additional, change_outputs)) => {
                                #[cfg(target_arch = "wasm32")]
                                web_sys::console::log_1(&format!("Transaction created successfully! TX ID: {}, Fee: {}, Change outputs: {}", tx_id, fee, change_outputs.len()).into());

                                TransactionCreatedResponse {
                                    success: true,
                                    error: None,
                                    tx_id,
                                    fee,
                                    tx_blob: Some(tx_blob),
                                    tx_key: Some(tx_key),
                                    tx_key_additional,
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
                                    tx_key: None,
                                    tx_key_additional: Vec::new(),
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
                        tx_key: None,
                        tx_key_additional: Vec::new(),
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
                tx_key: None,
                tx_key_additional: Vec::new(),
                spent_output_hashes: Vec::new(),
                change_outputs: Vec::new(),
            }
            .send_signal_to_dart();
        }
    }
}

impl TxBuilderActor {
    /// Find the best combination of exactly `target_count` outputs that sum to >= `needed_total`.
    /// Returns the combination closest to needed_total (minimizes excess) among valid combinations.
    fn find_best_combination(
        outputs: &[StoredOutput],
        needed_total: u64,
        target_count: usize,
    ) -> Option<(Vec<StoredOutput>, u64)> {
        if target_count == 0 || target_count > outputs.len() {
            return None;
        }

        // Exhaustive search to find combination closest to needed amount (minimizes waste)
        fn search(
            outputs: &[StoredOutput],
            needed: u64,
            target_count: usize,
            start_idx: usize,
            current: &mut Vec<StoredOutput>,
            current_sum: u64,
            best: &mut Option<(Vec<StoredOutput>, u64)>,
        ) {
            // Found a combination of the right size
            if current.len() == target_count {
                if current_sum >= needed {
                    // Update best if this is closer to needed amount (less excess)
                    let current_excess = current_sum - needed;
                    let is_better = match best {
                        None => true,
                        Some((_, best_sum)) => {
                            let best_excess = *best_sum - needed;
                            current_excess < best_excess
                        }
                    };
                    if is_better {
                        *best = Some((current.clone(), current_sum));
                    }
                }
                return;
            }

            // Try each remaining output
            let remaining_needed = target_count - current.len();
            for i in start_idx..outputs.len() {
                // Pruning: not enough outputs left to reach target_count
                if outputs.len() - i < remaining_needed {
                    break;
                }

                current.push(outputs[i].clone());
                search(
                    outputs,
                    needed,
                    target_count,
                    i + 1,
                    current,
                    current_sum + outputs[i].amount,
                    best,
                );
                current.pop();
            }
        }

        let mut best: Option<(Vec<StoredOutput>, u64)> = None;
        let mut current = Vec::new();
        search(outputs, needed_total, target_count, 0, &mut current, 0, &mut best);
        best
    }

    fn build_transaction_impl_inner(
        &self,
        msg: BuildTransaction,
        wallet_data: WalletData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(String, u64, String, String, Vec<String>, Vec<ChangeOutput>), String>>>> {
        use monero_rust::native::{create_transaction, TransactionResult};

        let outputs_vec: Vec<monero_rust::native::StoredOutputData> = wallet_data
            .outputs
            .iter()
            .map(|o| monero_rust::native::StoredOutputData {
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

            Ok((result.tx_id, result.fee, result.tx_blob, result.tx_key, result.tx_key_additional, change_outputs))
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
            match monero_rust::native::broadcast_transaction(&msg.node_url, &msg.tx_blob).await {
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
