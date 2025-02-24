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
        _owned_tasks.spawn(Self::listen_to_sweep_requests(self_addr.clone()));
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

    async fn listen_to_sweep_requests(mut self_addr: Address<Self>) {
        let receiver = SweepAllRequest::get_dart_signal_receiver();
        while let Some(signal_pack) = receiver.recv().await {
            let request = signal_pack.message;
            let _ = self_addr
                .notify(SweepAll {
                    node_url: request.node_url,
                    seed: request.seed,
                    network: request.network,
                    destination_address: request.destination_address,
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

#[async_trait]
impl Notifiable<SweepAll> for TxBuilderActor {
    async fn notify(&mut self, msg: SweepAll, _ctx: &Context<Self>) {
        if let Some(wallet_addr) = &mut self.wallet_actor {
            // Get wallet data and height
            let wallet_data_result = wallet_addr.send(GetWalletData).await;
            let wallet_height_result = wallet_addr.send(GetWalletHeight).await;

            match (wallet_data_result, wallet_height_result) {
                (Ok(wallet_data), Ok(wallet_height)) => {
                    const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;

                    // Filter to spendable outputs
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

                    // Apply manual output selection if provided (for account filtering)
                    if let Some(ref selected) = msg.selected_outputs {
                        spendable_outputs.retain(|o| {
                            let output_key = format!("{}:{}", o.tx_hash, o.output_index);
                            selected.contains(&output_key)
                        });
                    }

                    if spendable_outputs.is_empty() {
                        TransactionCreatedResponse {
                            success: false,
                            error: Some("No spendable outputs available for sweep".to_string()),
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

                    // Collect spent output hashes
                    let spent_output_hashes: Vec<String> = spendable_outputs
                        .iter()
                        .map(|o| format!("{}:{}", o.tx_hash, o.output_index))
                        .collect();

                    // Convert StoredOutput to StoredOutputData
                    let stored_outputs: Vec<monero_rust::tx_builder::native::StoredOutputData> =
                        spendable_outputs
                            .into_iter()
                            .map(|o| monero_rust::tx_builder::native::StoredOutputData {
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

                    // Spawn sweep operation in local task to avoid Send requirements
                    let node_url = msg.node_url.clone();
                    let seed = msg.seed.clone();
                    let network = msg.network.clone();
                    let destination = msg.destination_address.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        match monero_rust::tx_builder::native::sweep_all(
                            &node_url,
                            &seed,
                            &network,
                            stored_outputs,
                            &destination,
                        )
                        .await
                        {
                            Ok(result) => {
                                // Convert change outputs (should be empty for sweep_all)
                                let change_outputs: Vec<crate::signals::ChangeOutput> = result
                                    .change_outputs
                                    .into_iter()
                                    .map(|c| crate::signals::ChangeOutput {
                                        tx_hash: c.tx_hash,
                                        output_index: c.output_index.into(),
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

                                TransactionCreatedResponse {
                                    success: true,
                                    error: None,
                                    tx_id: result.tx_id,
                                    fee: result.fee,
                                    tx_blob: Some(result.tx_blob),
                                    tx_key: Some(result.tx_key),
                                    tx_key_additional: result.tx_key_additional,
                                    spent_output_hashes,
                                    change_outputs,
                                }
                                .send_signal_to_dart();
                            }
                            Err(e) => {
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
                (Err(e), _) | (_, Err(e)) => {
                    TransactionCreatedResponse {
                        success: false,
                        error: Some(format!("Failed to get wallet data: {:?}", e)),
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test output
    fn create_output(amount: u64, tx_hash: &str, output_index: u8, block_height: u64) -> StoredOutput {
        StoredOutput {
            tx_hash: tx_hash.to_string(),
            output_index,
            amount,
            key: "test_key".to_string(),
            key_offset: "test_offset".to_string(),
            commitment_mask: "test_mask".to_string(),
            subaddress: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height,
            spent: false,
            key_image: format!("key_image_{}", output_index),
            is_coinbase: false,
        }
    }

    // Helper function to simulate the input selection logic
    fn select_outputs(
        mut available_outputs: Vec<StoredOutput>,
        total_send_amount: u64,
    ) -> Vec<StoredOutput> {
        // Fee estimates (same as in the actual code)
        const FEE_PER_INPUT_ESTIMATE: u64 = 15_000_000;
        const BASE_FEE_ESTIMATE: u64 = 20_000_000;

        // First, try to find the smallest single output that can cover the transaction
        let single_input_fee = BASE_FEE_ESTIMATE + FEE_PER_INPUT_ESTIMATE;
        let needed_for_single = total_send_amount + single_input_fee;

        // Sort by amount to find candidates
        available_outputs.sort_by_key(|o| o.amount);

        // Find the smallest output that's >= needed amount
        let single_output = available_outputs.iter()
            .find(|o| o.amount >= needed_for_single);

        if let Some(output) = single_output {
            // Found a single output that can cover it - use only that
            vec![output.clone()]
        } else {
            // No single output works - find optimal combination
            // Helper function matching the actor's implementation
            fn find_best_combination(
                outputs: &[StoredOutput],
                needed_total: u64,
                target_count: usize,
            ) -> Option<(Vec<StoredOutput>, u64)> {
                if target_count == 0 || target_count > outputs.len() {
                    return None;
                }

                fn search(
                    outputs: &[StoredOutput],
                    needed: u64,
                    target_count: usize,
                    start_idx: usize,
                    current: &mut Vec<StoredOutput>,
                    current_sum: u64,
                    best: &mut Option<(Vec<StoredOutput>, u64)>,
                ) {
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

                    let remaining_needed = target_count - current.len();
                    for i in start_idx..outputs.len() {
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

            // Sort by amount descending for efficient searching
            available_outputs.sort_by_key(|o| std::cmp::Reverse(o.amount));

            let mut best_selection: Option<Vec<StoredOutput>> = None;
            let mut best_count = usize::MAX;
            let mut best_total = u64::MAX;

            for target_count in 2..=available_outputs.len() {
                let estimated_fee = BASE_FEE_ESTIMATE + (target_count as u64 * FEE_PER_INPUT_ESTIMATE);
                let needed_total = total_send_amount + estimated_fee;

                if let Some((selection, total)) = find_best_combination(
                    &available_outputs,
                    needed_total,
                    target_count,
                ) {
                    if target_count < best_count || (target_count == best_count && total < best_total) {
                        best_selection = Some(selection);
                        best_count = target_count;
                        best_total = total;
                        break;
                    }
                }
            }

            best_selection.unwrap_or(available_outputs)
        }
    }

    #[test]
    fn test_scenario_1_single_output_sufficient() {
        // Scenario 1: Send 1 XMR with outputs [5 XMR, 2 XMR, 0.5 XMR, 0.3 XMR]
        // Should use only the 2 XMR output (smallest that covers 1 XMR + fee)
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100),  // 5 XMR
            create_output(2_000_000_000_000, "tx2", 0, 101),  // 2 XMR
            create_output(500_000_000_000, "tx3", 0, 102),    // 0.5 XMR
            create_output(300_000_000_000, "tx4", 0, 103),    // 0.3 XMR
        ];

        let send_amount = 1_000_000_000_000; // 1 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 1 output
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");

        // Should be the 2 XMR output (smallest sufficient)
        assert_eq!(selected[0].amount, 2_000_000_000_000, "Should select 2 XMR output");
        assert_eq!(selected[0].tx_hash, "tx2", "Should select the 2 XMR output");
    }

    #[test]
    fn test_scenario_2_multiple_outputs_needed() {
        // Scenario 2: Send 3 XMR with outputs [2 XMR, 1.5 XMR, 0.8 XMR, 0.5 XMR]
        // Should use 2 XMR + 1.5 XMR (largest first to minimize inputs)
        let outputs = vec![
            create_output(2_000_000_000_000, "tx1", 0, 100),   // 2 XMR
            create_output(1_500_000_000_000, "tx2", 0, 101),   // 1.5 XMR
            create_output(800_000_000_000, "tx3", 0, 102),     // 0.8 XMR
            create_output(500_000_000_000, "tx4", 0, 103),     // 0.5 XMR
        ];

        let send_amount = 3_000_000_000_000; // 3 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 2 outputs (2 XMR + 1.5 XMR = 3.5 XMR covers 3 XMR + fee)
        assert_eq!(selected.len(), 2, "Should select exactly 2 outputs (largest first)");

        // Should select the two largest outputs
        assert_eq!(selected[0].amount, 2_000_000_000_000, "First should be 2 XMR");
        assert_eq!(selected[1].amount, 1_500_000_000_000, "Second should be 1.5 XMR");

        // Total should be enough to cover amount + fee
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        let estimated_fee = 20_000_000 + (selected.len() as u64 * 15_000_000);
        assert!(total >= send_amount + estimated_fee,
            "Total selected ({}) should cover amount + fee ({})", total, send_amount + estimated_fee);
    }

    #[test]
    fn test_scenario_3_small_send_small_output() {
        // Scenario 3: Send 0.5 XMR with outputs [5 XMR, 2 XMR, 0.8 XMR, 0.3 XMR]
        // Should use only the 0.8 XMR output (smallest sufficient)
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100),  // 5 XMR
            create_output(2_000_000_000_000, "tx2", 0, 101),  // 2 XMR
            create_output(800_000_000_000, "tx3", 0, 102),    // 0.8 XMR
            create_output(300_000_000_000, "tx4", 0, 103),    // 0.3 XMR
        ];

        let send_amount = 500_000_000_000; // 0.5 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 1 output
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");

        // Should be the 0.8 XMR output (smallest sufficient)
        assert_eq!(selected[0].amount, 800_000_000_000, "Should select 0.8 XMR output");
        assert_eq!(selected[0].tx_hash, "tx3", "Should select the 0.8 XMR output");
    }

    #[test]
    fn test_avoids_using_all_outputs_unnecessarily() {
        // Verify that we don't use all outputs when only some are needed
        let outputs = vec![
            create_output(1_000_000_000_000, "tx1", 0, 100),  // 1 XMR
            create_output(1_000_000_000_000, "tx2", 0, 101),  // 1 XMR
            create_output(1_000_000_000_000, "tx3", 0, 102),  // 1 XMR
            create_output(1_000_000_000_000, "tx4", 0, 103),  // 1 XMR
            create_output(1_000_000_000_000, "tx5", 0, 104),  // 1 XMR
        ];

        let send_amount = 500_000_000_000; // 0.5 XMR
        let selected = select_outputs(outputs.clone(), send_amount);

        // Should use only 1 output, not all 5
        assert_eq!(selected.len(), 1, "Should use only 1 output, not all available outputs");

        // Verify for a larger amount that still doesn't need all
        let send_amount_2 = 1_500_000_000_000; // 1.5 XMR
        let selected_2 = select_outputs(outputs, send_amount_2);

        // Should use minimal outputs, not all 5
        assert!(selected_2.len() <= 3, "Should use minimal outputs, not all available");
        assert!(selected_2.len() >= 2, "Should use at least 2 outputs for 1.5 XMR");
    }

    #[test]
    fn test_exact_amount_match() {
        // Test when we have an output that exactly matches (or very close to) the needed amount
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100),      // 5 XMR
            create_output(1_035_000_000_000, "tx2", 0, 101),      // 1.035 XMR (≈ 1 XMR + fee)
            create_output(500_000_000_000, "tx3", 0, 102),        // 0.5 XMR
        ];

        let send_amount = 1_000_000_000_000; // 1 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select the output closest to needed amount
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");
        assert_eq!(selected[0].amount, 1_035_000_000_000,
            "Should select output close to needed amount + fee");
    }

    #[test]
    fn test_prefers_single_large_over_multiple_small() {
        // Verify we use 1 large output rather than combining many small ones
        let outputs = vec![
            create_output(3_000_000_000_000, "tx_large", 0, 100),  // 3 XMR
            create_output(100_000_000_000, "tx1", 0, 101),          // 0.1 XMR
            create_output(100_000_000_000, "tx2", 0, 102),          // 0.1 XMR
            create_output(100_000_000_000, "tx3", 0, 103),          // 0.1 XMR
            create_output(100_000_000_000, "tx4", 0, 104),          // 0.1 XMR
            create_output(100_000_000_000, "tx5", 0, 105),          // 0.1 XMR
        ];

        let send_amount = 400_000_000_000; // 0.4 XMR
        let selected = select_outputs(outputs, send_amount);

        // Even though we have many small outputs, should use the single large one
        assert_eq!(selected.len(), 1, "Should use single large output");
        assert_eq!(selected[0].tx_hash, "tx_large", "Should select the 3 XMR output");
    }

    #[test]
    fn test_minimize_inputs_and_change() {
        // Send 3 XMR with outputs [0.75, 1, 1.5, 2.5 XMR]
        // Should use 2.5 + 0.75 = 3.25 XMR (2 inputs, locks only ~0.25 XMR as change)
        // NOT 2.5 + 1.0 = 3.5 XMR (2 inputs, locks ~0.5 XMR as change)
        // NOT 2.5 + 1.5 = 4.0 XMR (2 inputs, locks ~1.0 XMR as change)
        // NOT 0.75 + 1 + 1.5 = 3.25 XMR (3 inputs - more inputs is worse)
        let outputs = vec![
            create_output(750_000_000_000, "tx1", 0, 100),    // 0.75 XMR
            create_output(1_000_000_000_000, "tx2", 0, 101),  // 1 XMR
            create_output(1_500_000_000_000, "tx3", 0, 102),  // 1.5 XMR
            create_output(2_500_000_000_000, "tx4", 0, 103),  // 2.5 XMR
        ];

        let send_amount = 3_000_000_000_000; // 3 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 2 outputs (minimizing inputs)
        assert_eq!(selected.len(), 2, "Should select 2 outputs to minimize inputs");

        // Should select 2.5 + 0.75 = 3.25 XMR (minimizes locked change)
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(total, 3_250_000_000_000, "Total should be 3.25 XMR (2.5 + 0.75) to minimize locked change");

        // Verify we have the right outputs
        assert!(selected.iter().any(|o| o.amount == 2_500_000_000_000), "Should include 2.5 XMR");
        assert!(selected.iter().any(|o| o.amount == 750_000_000_000), "Should include 0.75 XMR");

        let estimated_fee = 20_000_000 + (2 * 15_000_000); // 50M atomic units
        assert!(total >= send_amount + estimated_fee,
            "Should have enough to cover 3 XMR + fee");

        // Change locked = 3.25 - 3.0 - 0.05 = ~0.20 XMR (minimized!)
        let change_locked = total - send_amount - estimated_fee;
        assert!(change_locked < 250_000_000_000, "Should lock < 0.25 XMR as change");
    }

    #[test]
    fn test_edge_case_no_single_output_works() {
        // Edge case: No single output is sufficient, must combine
        // Outputs: [0.5, 0.6, 0.7, 0.8 XMR], send 1.2 XMR
        // Need: 1.2 + 0.05 fee = 1.25 XMR
        // Should use 0.8 + 0.5 = 1.3 XMR (minimizes locked change: ~0.05 XMR)
        // NOT 0.8 + 0.6 = 1.4 XMR (locks ~0.15 XMR)
        // NOT 0.8 + 0.7 = 1.5 XMR (locks ~0.25 XMR)
        let outputs = vec![
            create_output(500_000_000_000, "tx1", 0, 100),  // 0.5 XMR
            create_output(600_000_000_000, "tx2", 0, 101),  // 0.6 XMR
            create_output(700_000_000_000, "tx3", 0, 102),  // 0.7 XMR
            create_output(800_000_000_000, "tx4", 0, 103),  // 0.8 XMR
        ];

        let send_amount = 1_200_000_000_000; // 1.2 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select 2 outputs
        assert_eq!(selected.len(), 2, "Should select 2 outputs");

        // Should select 0.8 + 0.5 to minimize locked change
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(total, 1_300_000_000_000, "Total should be 1.3 XMR (0.8 + 0.5)");

        assert!(selected.iter().any(|o| o.amount == 800_000_000_000), "Should include 0.8 XMR");
        assert!(selected.iter().any(|o| o.amount == 500_000_000_000), "Should include 0.5 XMR");
    }

    #[test]
    fn test_prefers_fewer_large_over_many_small() {
        // Send 2 XMR with outputs [1.5, 1, 0.4, 0.4, 0.4, 0.4, 0.4]
        // Should use 1.5 + 1 = 2.5 XMR (2 inputs)
        // NOT 0.4 + 0.4 + 0.4 + 0.4 + 0.4 = 2.0 XMR (5 inputs)
        let outputs = vec![
            create_output(1_500_000_000_000, "tx_large1", 0, 100), // 1.5 XMR
            create_output(1_000_000_000_000, "tx_large2", 0, 101), // 1 XMR
            create_output(400_000_000_000, "tx1", 0, 102),         // 0.4 XMR
            create_output(400_000_000_000, "tx2", 0, 103),         // 0.4 XMR
            create_output(400_000_000_000, "tx3", 0, 104),         // 0.4 XMR
            create_output(400_000_000_000, "tx4", 0, 105),         // 0.4 XMR
            create_output(400_000_000_000, "tx5", 0, 106),         // 0.4 XMR
        ];

        let send_amount = 2_000_000_000_000; // 2 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should use only 2 large outputs, not 5+ small ones
        assert_eq!(selected.len(), 2, "Should use 2 outputs, not many small ones");
        assert_eq!(selected[0].amount, 1_500_000_000_000, "First should be 1.5 XMR");
        assert_eq!(selected[1].amount, 1_000_000_000_000, "Second should be 1 XMR");
    }

    #[test]
    fn test_single_output_preference_over_combination() {
        // Send 1 XMR with outputs [1.05, 0.6, 0.5]
        // Should use single 1.05 XMR output (covers 1 XMR + ~0.035 fee)
        // NOT combine 0.6 + 0.5 = 1.1 XMR
        let outputs = vec![
            create_output(1_050_000_000_000, "tx_single", 0, 100), // 1.05 XMR
            create_output(600_000_000_000, "tx1", 0, 101),         // 0.6 XMR
            create_output(500_000_000_000, "tx2", 0, 102),         // 0.5 XMR
        ];

        let send_amount = 1_000_000_000_000; // 1 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should prefer single output
        assert_eq!(selected.len(), 1, "Should use single output");
        assert_eq!(selected[0].amount, 1_050_000_000_000, "Should use 1.05 XMR output");
        assert_eq!(selected[0].tx_hash, "tx_single");
    }

    #[test]
    fn test_three_outputs_minimized_to_two() {
        // Send 5 XMR with outputs [3, 2.5, 1, 0.5, 0.3]
        // Should use 3 + 2.5 = 5.5 XMR (2 inputs)
        // NOT 2.5 + 1 + 0.5 + 0.3 + ... (3+ inputs)
        let outputs = vec![
            create_output(3_000_000_000_000, "tx1", 0, 100),   // 3 XMR
            create_output(2_500_000_000_000, "tx2", 0, 101),   // 2.5 XMR
            create_output(1_000_000_000_000, "tx3", 0, 102),   // 1 XMR
            create_output(500_000_000_000, "tx4", 0, 103),     // 0.5 XMR
            create_output(300_000_000_000, "tx5", 0, 104),     // 0.3 XMR
        ];

        let send_amount = 5_000_000_000_000; // 5 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should use exactly 2 outputs (the two largest)
        assert_eq!(selected.len(), 2, "Should use 2 outputs, not more");
        assert_eq!(selected[0].amount, 3_000_000_000_000, "First should be 3 XMR");
        assert_eq!(selected[1].amount, 2_500_000_000_000, "Second should be 2.5 XMR");

        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(total, 5_500_000_000_000, "Total should be 5.5 XMR");
    }
}
