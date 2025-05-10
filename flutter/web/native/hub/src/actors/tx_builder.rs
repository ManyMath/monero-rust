use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use rinf::{DartSignal, RustSignal};
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

pub struct TxBuilderActor {
    wallet_actor: Option<Address<super::wallet::WalletActor>>,
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
            _owned_tasks,
        }
    }

    pub fn set_wallet_actor(&mut self, addr: Address<super::wallet::WalletActor>) {
        self.wallet_actor = Some(addr);
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
                    let stored_daemon_height = wallet_height.daemon_height;
                    let node_url = msg.node_url;
                    let seed = msg.seed;
                    let network = msg.network;
                    let recipients = msg.recipients;
                    let selected_outputs = msg.selected_outputs;

                    wasm_bindgen_futures::spawn_local(async move {
                        // Query fresh daemon height to avoid stale core_state
                        let daemon_height = match monero_rust::get_daemon_height(&node_url).await {
                            Ok(h) => {
                                #[cfg(target_arch = "wasm32")]
                                web_sys::console::log_1(&format!(
                                    "[BuildTx] Fresh daemon height: {}, stored: {}",
                                    h, stored_daemon_height
                                ).into());
                                h.max(stored_daemon_height)
                            }
                            Err(e) => {
                                #[cfg(target_arch = "wasm32")]
                                web_sys::console::warn_1(&format!(
                                    "[BuildTx] Failed to get daemon height: {}, using stored: {}",
                                    e, stored_daemon_height
                                ).into());
                                stored_daemon_height
                            }
                        };

                        let num_outputs = wallet_data.outputs.len();
                        let num_spent = wallet_data.outputs.iter().filter(|o| o.spent).count();
                        let num_spendable = wallet_data.outputs.iter()
                            .filter(|o| monero_rust::is_spendable(o, daemon_height))
                            .count();
                        #[cfg(target_arch = "wasm32")]
                        web_sys::console::log_1(&format!(
                            "[BuildTx] outputs={}, spent={}, spendable={}, daemon_height={}",
                            num_outputs, num_spent, num_spendable, daemon_height
                        ).into());

                        let total_send_amount: u64 = recipients.iter().map(|(_, amt)| amt).sum();

                        let prepared = match monero_rust::prepare_send_inputs(
                            &wallet_data.outputs,
                            daemon_height,
                            total_send_amount,
                            selected_outputs.as_deref(),
                        ) {
                            Ok(p) => p,
                            Err(error_msg) => {
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
                        };

                        let spent_hashes = prepared.spent_output_keys;

                        match monero_rust::native::create_transaction(
                            &node_url,
                            &seed,
                            &network,
                            prepared.stored_outputs,
                            &recipients,
                        )
                        .await
                        {
                            Ok(result) => {
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

                                TransactionCreatedResponse {
                                    success: true,
                                    error: None,
                                    tx_id: result.tx_id,
                                    fee: result.fee,
                                    tx_blob: Some(result.tx_blob),
                                    tx_key: Some(result.tx_key),
                                    tx_key_additional: result.tx_key_additional,
                                    spent_output_hashes: spent_hashes,
                                    change_outputs,
                                }
                                .send_signal_to_dart();
                            }
                            Err(e) => {
                                TransactionCreatedResponse {
                                    success: false,
                                    error: Some(format!("Transaction building failed: {}", e)),
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
                    let stored_daemon_height = wallet_height.daemon_height;
                    let node_url = msg.node_url;
                    let seed = msg.seed;
                    let network = msg.network;
                    let destination = msg.destination_address;
                    let selected_outputs = msg.selected_outputs;

                    wasm_bindgen_futures::spawn_local(async move {
                        // Query fresh daemon height to avoid stale core_state
                        let daemon_height = match monero_rust::get_daemon_height(&node_url).await {
                            Ok(h) => h.max(stored_daemon_height),
                            Err(_) => stored_daemon_height,
                        };

                        let prepared = match monero_rust::prepare_sweep_inputs(
                            &wallet_data.outputs,
                            daemon_height,
                            selected_outputs.as_deref(),
                        ) {
                            Ok(p) => p,
                            Err(error_msg) => {
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
                        };

                        let spent_output_hashes = prepared.spent_output_keys;

                        match monero_rust::tx_builder::native::sweep_all(
                            &node_url,
                            &seed,
                            &network,
                            prepared.stored_outputs,
                            &destination,
                        )
                        .await
                        {
                            Ok(result) => {
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
    fn create_output(amount: u64, tx_hash: &str, output_index: u8, block_height: u64) -> monero_rust::WalletOutput {
        monero_rust::WalletOutput {
            tx_hash: tx_hash.to_string(),
            output_index,
            amount,
            amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
            key: "test_key".to_string(),
            key_offset: "test_offset".to_string(),
            commitment_mask: "test_mask".to_string(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height,
            spent: false,
            key_image: format!("key_image_{}", output_index),
            is_coinbase: false,
        }
    }

    // Helper function to simulate the input selection logic using core
    fn select_outputs(
        available_outputs: Vec<monero_rust::WalletOutput>,
        total_send_amount: u64,
    ) -> Vec<monero_rust::WalletOutput> {
        monero_rust::select_inputs(&available_outputs, total_send_amount, None)
            .map(|r| r.selected)
            .unwrap_or(available_outputs)
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
