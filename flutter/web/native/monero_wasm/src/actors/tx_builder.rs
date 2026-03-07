use crate::ffi_web::SendToDart;
use crate::messages::*;
use crate::signals::*;
use async_trait::async_trait;
use messages::prelude::{Actor, Address, Context, Notifiable};
use monero_rust::error_codes::ErrorResponse;
use tokio::task::JoinSet;
use tokio_with_wasm::alias as tokio;

#[cfg(target_arch = "wasm32")]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    tokio::task::spawn_local(f);
}

fn tx_error_response(msg: String) -> TransactionCreatedResponse {
    let err = ErrorResponse::from_string(&msg);
    TransactionCreatedResponse {
        success: false,
        error: Some(msg),
        error_code: Some(err.code),
        error_hint: err.hint,
        error_transient: Some(err.transient),
        tx_id: String::new(),
        fee: 0,
        tx_blob: None,
        tx_key: None,
        tx_key_additional: Vec::new(),
        spent_output_hashes: Vec::new(),
        change_outputs: Vec::new(),
    }
}

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
        _owned_tasks.spawn(Self::listen_to_proof_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_unsigned_tx_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_sign_unsigned_requests(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_inspect_unsigned_txsets(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_extract_signed_txsets(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_build_signed_txsets(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_export_key_images(self_addr.clone()));
        _owned_tasks.spawn(Self::listen_to_import_key_images(self_addr));

        TxBuilderActor {
            wallet_actor: None,
            _owned_tasks,
        }
    }

    pub fn set_wallet_actor(&mut self, addr: Address<super::wallet::WalletActor>) {
        self.wallet_actor = Some(addr);
    }

    async fn listen_to_tx_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_create_transaction_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved_seed = match super::wallet::pre_resolve_bip39(
                &request.seed,
                &request.passphrase,
                request.bip39_account_index,
            ) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    TransactionCreatedResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        tx_id: String::new(),
                        fee: 0,
                        tx_blob: None,
                        tx_key: None,
                        tx_key_additional: Vec::new(),
                        spent_output_hashes: Vec::new(),
                        change_outputs: Vec::new(),
                    }
                    .send_signal_to_dart();
                    continue;
                }
            };
            // Convert Vec<Recipient> to Vec<(String, u64)>
            let recipients: Vec<(String, u64)> = request
                .recipients
                .into_iter()
                .map(|r| (r.address, r.amount))
                .collect();
            let _ = self_addr
                .notify(BuildTransaction {
                    node_url: request.node_url,
                    seed: resolved_seed,
                    network: request.network,
                    recipients,
                    selected_outputs: request.selected_outputs,
                    subtract_fee: request.subtract_fee,
                })
                .await;
        }
    }

    async fn listen_to_sweep_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_sweep_all_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved_seed = match super::wallet::pre_resolve_bip39(
                &request.seed,
                &request.passphrase,
                request.bip39_account_index,
            ) {
                Ok(s) => s,
                Err(e) => {
                    let err = ErrorResponse::from_string(&e);
                    TransactionCreatedResponse {
                        success: false,
                        error: Some(e),
                        error_code: Some(err.code),
                        error_hint: err.hint,
                        error_transient: Some(err.transient),
                        tx_id: String::new(),
                        fee: 0,
                        tx_blob: None,
                        tx_key: None,
                        tx_key_additional: Vec::new(),
                        spent_output_hashes: Vec::new(),
                        change_outputs: Vec::new(),
                    }
                    .send_signal_to_dart();
                    continue;
                }
            };
            let _ = self_addr
                .notify(SweepAll {
                    node_url: request.node_url,
                    seed: resolved_seed,
                    network: request.network,
                    destination_address: request.destination_address,
                    selected_outputs: request.selected_outputs,
                })
                .await;
        }
    }

    async fn listen_to_broadcast_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_broadcast_transaction_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(BroadcastTransaction {
                    node_url: request.node_url,
                    tx_blob: request.tx_blob,
                    spent_output_hashes: request.spent_output_hashes,
                    tx_id: request.tx_id,
                    spent_key_images: request.spent_key_images,
                    do_not_relay: request.do_not_relay,
                })
                .await;
        }
    }

    async fn listen_to_proof_requests(_self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_generate_out_proof_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;

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
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        signature: Some(result.signature),
                        formatted: Some(result.formatted),
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    OutProofGeneratedResponse {
                        success: false,
                        error: Some(e),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        signature: None,
                        formatted: None,
                    }
                    .send_signal_to_dart();
                }
            }
        }
    }

    async fn listen_to_unsigned_tx_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_create_unsigned_transaction_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let recipients: Vec<(String, u64)> = request
                .recipients
                .iter()
                .map(|r| (r.address.clone(), r.amount))
                .collect();
            let _ = self_addr
                .notify(CreateUnsignedTx {
                    node_url: request.node_url,
                    view_key_hex: request.view_key_hex,
                    pub_spend_key_hex: request.pub_spend_key_hex,
                    network: request.network,
                    recipients,
                    selected_outputs: request.selected_outputs,
                    max_fee_per_weight: request.max_fee_per_weight,
                })
                .await;
        }
    }

    async fn listen_to_sign_unsigned_requests(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_sign_unsigned_transaction_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let has_private_keys = request
                .spend_secret_key_hex
                .as_deref()
                .is_some_and(|key| !key.is_empty())
                || request
                    .view_secret_key_hex
                    .as_deref()
                    .is_some_and(|key| !key.is_empty());
            let resolved_seed = if has_private_keys {
                request.seed.clone()
            } else {
                match super::wallet::pre_resolve_bip39(
                    &request.seed,
                    &request.passphrase,
                    request.bip39_account_index,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        TransactionSignedOfflineResponse {
                            success: false,
                            error: Some(e),
                            tx_id: None,
                            fee: 0,
                            error_code: None,
                            error_hint: None,
                            error_transient: None,
                            tx_blob: None,
                            signed_txset_hex: None,
                            tx_key: None,
                            tx_key_additional: vec![],
                            change_outputs: vec![],
                            spent_key_images: vec![],
                        }
                        .send_signal_to_dart();
                        continue;
                    }
                }
            };
            let _ = self_addr
                .notify(SignUnsignedTx {
                    seed: resolved_seed,
                    unsigned_tx_hex: request.unsigned_tx_hex,
                    network: request.network,
                    spend_secret_key_hex: request.spend_secret_key_hex,
                    view_secret_key_hex: request.view_secret_key_hex,
                })
                .await;
        }
    }

    async fn listen_to_extract_signed_txsets(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_extract_signed_txset_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(ExtractSignedTxSet {
                    data_hex: request.data_hex,
                    view_key_hex: request.view_key_hex,
                })
                .await;
        }
    }

    async fn listen_to_build_signed_txsets(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_build_signed_txset_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(BuildSignedTxSet {
                    unsigned_txset_hex: request.unsigned_txset_hex,
                    view_key_hex: request.view_key_hex,
                    tx_blob_hex: request.tx_blob_hex,
                    key_images: request.key_images,
                    tx_key_images: request.tx_key_images,
                })
                .await;
        }
    }

    async fn listen_to_inspect_unsigned_txsets(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_inspect_unsigned_txset_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let _ = self_addr
                .notify(InspectUnsignedTxSet {
                    data_hex: request.data_hex,
                    view_key_hex: request.view_key_hex,
                })
                .await;
        }
    }

    async fn listen_to_export_key_images(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_export_key_images_request_receiver();
        while let Some(dart_msg) = receiver.recv().await {
            let request = dart_msg;
            let resolved_seed = match super::wallet::pre_resolve_bip39(
                &request.seed,
                &request.passphrase,
                request.bip39_account_index,
            ) {
                Ok(s) => s,
                Err(e) => {
                    KeyImagesExportedResponse {
                        success: false,
                        error: Some(e),
                        key_images_hex: None,
                        count: 0,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                    }
                    .send_signal_to_dart();
                    continue;
                }
            };
            let _ = self_addr
                .notify(ExportKeyImages {
                    seed: resolved_seed,
                    network: request.network,
                    passphrase: request.passphrase.clone(),
                })
                .await;
        }
    }

    async fn listen_to_import_key_images(mut self_addr: Address<Self>) {
        let mut receiver = crate::ffi_web::get_import_key_images_request_receiver();
        while let Some(request) = receiver.recv().await {
            let resolved_seed = match super::wallet::pre_resolve_bip39(
                &request.seed,
                &request.passphrase,
                request.bip39_account_index,
            ) {
                Ok(s) => s,
                Err(e) => {
                    KeyImagesImportedResponse {
                        success: false,
                        error: Some(e),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        imported_count: 0,
                        spent_count: 0,
                        key_images: vec![],
                        spent_key_images: vec![],
                    }
                    .send_signal_to_dart();
                    continue;
                }
            };
            let _ = self_addr
                .notify(ImportKeyImages {
                    data_hex: request.data_hex,
                    node_url: request.node_url,
                    seed: resolved_seed,
                    passphrase: request.passphrase.clone(),
                })
                .await;
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
                    let subtract_fee = msg.subtract_fee;

                    spawn_local(async move {
                        // Query fresh daemon height to avoid stale core_state
                        let daemon_height = match monero_rust::get_daemon_height(&node_url).await {
                            Ok(h) => {
                                log::info!(
                                    "[BuildTx] Fresh daemon height: {}, stored: {}",
                                    h,
                                    stored_daemon_height
                                );
                                h.max(stored_daemon_height)
                            }
                            Err(e) => {
                                log::warn!(
                                    "[BuildTx] Failed to get daemon height: {}, using stored: {}",
                                    e,
                                    stored_daemon_height
                                );
                                stored_daemon_height
                            }
                        };

                        let num_outputs = wallet_data.outputs.len();
                        let num_spent = wallet_data.outputs.iter().filter(|o| o.spent).count();
                        let num_spendable = wallet_data
                            .outputs
                            .iter()
                            .filter(|o| monero_rust::is_spendable(o, daemon_height))
                            .count();
                        log::info!(
                            "[BuildTx] outputs={}, spent={}, spendable={}, daemon_height={}",
                            num_outputs,
                            num_spent,
                            num_spendable,
                            daemon_height
                        );

                        let total_send_amount: u64 = recipients.iter().map(|(_, amt)| amt).sum();

                        let excluded = if wallet_data.pending_key_images.is_empty() {
                            None
                        } else {
                            Some(&wallet_data.pending_key_images)
                        };

                        let num_recipients = recipients.len();

                        // Single-recipient subtract_fee: always use sweep_all.
                        // sweep_all computes the exact fee from the RPC fee rate and
                        // transaction weight, avoiding any constant-vs-RPC fee mismatch.
                        if subtract_fee && recipients.len() == 1 {
                            let prepared = match monero_rust::prepare_sweep_inputs(
                                &wallet_data.outputs,
                                daemon_height,
                                selected_outputs.as_deref(),
                                excluded,
                            ) {
                                Ok(p) => p,
                                Err(error_msg) => {
                                    tx_error_response(error_msg).send_signal_to_dart();
                                    return;
                                }
                            };

                            let spent_hashes = prepared.spent_output_keys;

                            match monero_rust::tx_builder::native::sweep_all(
                                &node_url,
                                &seed,
                                &network,
                                prepared.stored_outputs,
                                &recipients[0].0,
                            )
                            .await
                            {
                                Ok(result) => {
                                    let change_outputs: Vec<ChangeOutput> = result
                                        .change_outputs
                                        .into_iter()
                                        .map(|c| ChangeOutput {
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
                                        error_code: None,
                                        error_hint: None,
                                        error_transient: None,
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
                                    tx_error_response(format!(
                                        "Transaction building failed: {}",
                                        e
                                    ))
                                    .send_signal_to_dart();
                                }
                            }
                            return;
                        }

                        // Multi-recipient or non-subtract_fee: normal coin selection
                        let prepared = if subtract_fee {
                            // Multi-recipient subtract_fee: try normal selection, fall back to sweep
                            match monero_rust::prepare_send_inputs(
                                &wallet_data.outputs,
                                daemon_height,
                                total_send_amount,
                                num_recipients,
                                selected_outputs.as_deref(),
                                excluded,
                            ) {
                                Ok(p) => p,
                                Err(_) => {
                                    match monero_rust::prepare_sweep_inputs(
                                        &wallet_data.outputs,
                                        daemon_height,
                                        selected_outputs.as_deref(),
                                        excluded,
                                    ) {
                                        Ok(p) => p,
                                        Err(error_msg) => {
                                            tx_error_response(error_msg).send_signal_to_dart();
                                            return;
                                        }
                                    }
                                }
                            }
                        } else {
                            match monero_rust::prepare_send_inputs(
                                &wallet_data.outputs,
                                daemon_height,
                                total_send_amount,
                                num_recipients,
                                selected_outputs.as_deref(),
                                excluded,
                            ) {
                                Ok(p) => p,
                                Err(error_msg) => {
                                    tx_error_response(error_msg).send_signal_to_dart();
                                    return;
                                }
                            }
                        };

                        let spent_hashes = prepared.spent_output_keys;

                        // Adjust amounts if subtract_fee (multi-recipient only at this point)
                        let final_recipients = if subtract_fee {
                            let adjusted = monero_rust::adjust_recipients_for_fee(
                                &recipients,
                                prepared.estimated_fee,
                            );
                            if adjusted.iter().any(|(_, amt)| *amt == 0) {
                                tx_error_response(
                                    "Recipient amount(s) too small to cover the fee after subtraction".to_string()
                                ).send_signal_to_dart();
                                return;
                            }
                            adjusted
                        } else {
                            recipients
                        };

                        match monero_rust::native::create_transaction(
                            &node_url,
                            &seed,
                            &network,
                            prepared.stored_outputs.clone(),
                            &final_recipients,
                        )
                        .await
                        {
                            Ok(result) => {
                                let change_outputs: Vec<ChangeOutput> = result
                                    .change_outputs
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
                                    error_code: None,
                                    error_hint: None,
                                    error_transient: None,
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
                            Err(e)
                                if subtract_fee
                                    && e.contains("dust")
                                    && final_recipients.len() == 1 =>
                            {
                                // Redirect single-recipient dust change to sweep_all
                                match monero_rust::tx_builder::native::sweep_all(
                                    &node_url,
                                    &seed,
                                    &network,
                                    prepared.stored_outputs,
                                    &final_recipients[0].0,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        let change_outputs: Vec<ChangeOutput> = result
                                            .change_outputs
                                            .into_iter()
                                            .map(|c| ChangeOutput {
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
                                            error_code: None,
                                            error_hint: None,
                                            error_transient: None,
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
                                    Err(e2) => {
                                        tx_error_response(format!(
                                            "Transaction building failed: {}",
                                            e2
                                        ))
                                        .send_signal_to_dart();
                                    }
                                }
                            }
                            Err(e) => {
                                tx_error_response(format!("Transaction building failed: {}", e))
                                    .send_signal_to_dart();
                            }
                        }
                    });
                }
                (Err(e), _) | (_, Err(e)) => {
                    tx_error_response(format!("Failed to get wallet data or height: {:?}", e))
                        .send_signal_to_dart();
                }
            }
        } else {
            tx_error_response("Wallet actor not initialized".to_string()).send_signal_to_dart();
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

                    spawn_local(async move {
                        // Query fresh daemon height to avoid stale core_state
                        let daemon_height = match monero_rust::get_daemon_height(&node_url).await {
                            Ok(h) => h.max(stored_daemon_height),
                            Err(_) => stored_daemon_height,
                        };

                        let excluded = if wallet_data.pending_key_images.is_empty() {
                            None
                        } else {
                            Some(&wallet_data.pending_key_images)
                        };
                        let prepared = match monero_rust::prepare_sweep_inputs(
                            &wallet_data.outputs,
                            daemon_height,
                            selected_outputs.as_deref(),
                            excluded,
                        ) {
                            Ok(p) => p,
                            Err(error_msg) => {
                                tx_error_response(error_msg).send_signal_to_dart();
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
                                    error_code: None,
                                    error_hint: None,
                                    error_transient: None,
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
                                tx_error_response(e).send_signal_to_dart();
                            }
                        }
                    });
                }
                (Err(e), _) | (_, Err(e)) => {
                    tx_error_response(format!("Failed to get wallet data: {:?}", e))
                        .send_signal_to_dart();
                }
            }
        } else {
            tx_error_response("Wallet actor not initialized".to_string()).send_signal_to_dart();
        }
    }
}

#[async_trait]
impl Notifiable<BroadcastTransaction> for TxBuilderActor {
    async fn notify(&mut self, msg: BroadcastTransaction, _ctx: &Context<Self>) {
        let wallet_actor = self.wallet_actor.clone();
        let tx_id = msg.tx_id.clone();
        let spent_key_images = msg.spent_key_images.clone();
        let spent_output_hashes = msg.spent_output_hashes.clone();
        let do_not_relay = msg.do_not_relay;

        log::info!("Broadcasting transaction...");

        // Spawn in local task to avoid Send requirements
        spawn_local(async move {
            match monero_rust::native::broadcast_transaction(
                &msg.node_url,
                &msg.tx_blob,
                do_not_relay,
            )
            .await
            {
                Ok(()) => {
                    log::info!("Transaction broadcast successful!");

                    // Only mutate wallet pending state when the transaction was actually relayed.
                    if !do_not_relay {
                        if let Some(mut wallet) = wallet_actor {
                            // Build PendingSpendInfo by zipping key images with output hashes
                            let spends: Vec<PendingSpendInfo> = spent_key_images
                                .iter()
                                .zip(spent_output_hashes.iter())
                                .map(|(ki, oh)| PendingSpendInfo {
                                    key_image: ki.clone(),
                                    output_key: oh.clone(),
                                    amount: 0, // amount will be resolved from wallet state
                                })
                                .collect();

                            let _ = wallet
                                .notify(AddPendingSpends {
                                    tx_id: tx_id.clone(),
                                    spends,
                                })
                                .await;
                        }
                    }

                    TransactionBroadcastResponse {
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        tx_id: Some(tx_id),
                        is_retryable: false,
                        is_double_spend: false,
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    let error_str = format!("Broadcast failed: {}", e);
                    log::error!("{}", error_str);
                    let (is_double_spend, is_retryable) =
                        monero_rust::classify_broadcast_error(&error_str);

                    let err = ErrorResponse::from_string(&error_str);
                    let error_transient = Some(is_retryable || err.transient);
                    let error_code = if is_double_spend {
                        Some(monero_rust::error_codes::ERR_TX_DOUBLE_SPEND)
                    } else if is_retryable {
                        Some(monero_rust::error_codes::ERR_RPC_CONNECTION)
                    } else {
                        Some(err.code)
                    };

                    TransactionBroadcastResponse {
                        success: false,
                        error: Some(error_str),
                        error_code,
                        error_hint: err.hint,
                        error_transient,
                        tx_id: None,
                        is_retryable,
                        is_double_spend,
                    }
                    .send_signal_to_dart();
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<CreateUnsignedTx> for TxBuilderActor {
    async fn notify(&mut self, msg: CreateUnsignedTx, _ctx: &Context<Self>) {
        if let Some(wallet_addr) = &mut self.wallet_actor {
            let wallet_data_result = wallet_addr.send(GetWalletData).await;
            let wallet_height_result = wallet_addr.send(GetWalletHeight).await;

            match (wallet_data_result, wallet_height_result) {
                (Ok(wallet_data), Ok(wallet_height)) => {
                    let node_url = msg.node_url;
                    let view_key_hex = msg.view_key_hex;
                    let pub_spend_key_hex = msg.pub_spend_key_hex;
                    let network = msg.network;
                    let recipients = msg.recipients;
                    let selected_outputs = msg.selected_outputs;
                    let max_fee_per_weight = msg.max_fee_per_weight;
                    let daemon_height = wallet_height.daemon_height;

                    spawn_local(async move {
                        let fresh_daemon_height =
                            match monero_rust::get_daemon_height(&node_url).await {
                                Ok(h) => h.max(daemon_height),
                                Err(_) => daemon_height,
                            };

                        let stored_outputs: Vec<monero_rust::tx_builder::StoredOutputData> =
                            wallet_data
                                .outputs
                                .iter()
                                .filter(|o| {
                                    if o.spent {
                                        return false;
                                    }
                                    if o.frozen {
                                        return false;
                                    }
                                    if !monero_rust::is_spendable(o, fresh_daemon_height) {
                                        return false;
                                    }
                                    if wallet_data.pending_key_images.contains(&o.key_image) {
                                        return false;
                                    }
                                    if let Some(ref sel) = selected_outputs {
                                        let key = format!("{}:{}", o.tx_hash, o.output_index);
                                        return sel.contains(&key);
                                    }
                                    true
                                })
                                .map(|o| monero_rust::tx_builder::StoredOutputData {
                                    tx_hash: o.tx_hash.clone(),
                                    output_index: o.output_index,
                                    amount: o.amount,
                                    key: o.key.clone(),
                                    key_offset: o.key_offset.clone(),
                                    commitment_mask: o.commitment_mask.clone(),
                                    subaddress: o.subaddress_index,
                                    payment_id: o.payment_id.clone(),
                                    received_output_bytes: o.received_output_bytes.clone(),
                                })
                                .collect();

                        if stored_outputs.is_empty() {
                            UnsignedTransactionCreatedResponse {
                                success: false,
                                error: Some("No spendable outputs available".to_string()),
                                error_code: None,
                                error_hint: None,
                                error_transient: None,
                                unsigned_tx_hex: None,
                                fee: 0,
                                recipients: vec![],
                            }
                            .send_signal_to_dart();
                            return;
                        }

                        match monero_rust::tx_builder::create_unsigned_transaction(
                            &node_url,
                            &view_key_hex,
                            &pub_spend_key_hex,
                            &network,
                            stored_outputs,
                            &recipients,
                            max_fee_per_weight,
                        )
                        .await
                        {
                            Ok(result) => {
                                UnsignedTransactionCreatedResponse {
                                    success: true,
                                    error: None,
                                    error_code: None,
                                    error_hint: None,
                                    error_transient: None,
                                    unsigned_tx_hex: Some(result.unsigned_tx_hex),
                                    fee: result.fee,
                                    recipients: result
                                        .recipients
                                        .iter()
                                        .map(|(addr, amt)| Recipient {
                                            address: addr.clone(),
                                            amount: *amt,
                                        })
                                        .collect(),
                                }
                                .send_signal_to_dart();
                            }
                            Err(e) => {
                                UnsignedTransactionCreatedResponse {
                                    success: false,
                                    error: Some(e),
                                    error_code: None,
                                    error_hint: None,
                                    error_transient: None,
                                    unsigned_tx_hex: None,
                                    fee: 0,
                                    recipients: vec![],
                                }
                                .send_signal_to_dart();
                            }
                        }
                    });
                }
                _ => {
                    UnsignedTransactionCreatedResponse {
                        success: false,
                        error: Some("Failed to get wallet data".to_string()),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        unsigned_tx_hex: None,
                        fee: 0,
                        recipients: vec![],
                    }
                    .send_signal_to_dart();
                }
            }
        } else {
            UnsignedTransactionCreatedResponse {
                success: false,
                error: Some("Wallet not initialized".to_string()),
                error_code: None,
                error_hint: None,
                error_transient: None,
                unsigned_tx_hex: None,
                fee: 0,
                recipients: vec![],
            }
            .send_signal_to_dart();
        }
    }
}

#[async_trait]
impl Notifiable<InspectUnsignedTxSet> for TxBuilderActor {
    async fn notify(&mut self, msg: InspectUnsignedTxSet, _ctx: &Context<Self>) {
        spawn_local(async move {
            let response =
                match decode_txset_request(&msg.data_hex, &msg.view_key_hex, "unsigned txset") {
                    Ok((data, view_key)) => {
                        match monero_rust::epee_compat::parse_unsigned_monero_txset_summary(
                            &data, &view_key,
                        ) {
                            Ok(summary) => match unsigned_txset_response(summary) {
                                Ok(response) => response,
                                Err(e) => unsigned_txset_error_response(e),
                            },
                            Err(e) => unsigned_txset_error_response(e),
                        }
                    }
                    Err(e) => unsigned_txset_error_response(e),
                };

            response.send_signal_to_dart();
        });
    }
}

#[async_trait]
impl Notifiable<ExtractSignedTxSet> for TxBuilderActor {
    async fn notify(&mut self, msg: ExtractSignedTxSet, _ctx: &Context<Self>) {
        spawn_local(async move {
            let response =
                match decode_txset_request(&msg.data_hex, &msg.view_key_hex, "signed txset") {
                    Ok((data, view_key)) => {
                        match monero_rust::epee_compat::parse_signed_monero_txset_summary(
                            &data, &view_key,
                        ) {
                            Ok(summary) => SignedTxSetExtractedResponse {
                                success: true,
                                error: None,
                                error_code: None,
                                error_hint: None,
                                error_transient: None,
                                transactions: summary
                                    .ptxes
                                    .into_iter()
                                    .map(|tx| ExtractedSignedTransaction {
                                        tx_id: hex::encode(tx.tx_hash),
                                        tx_blob: hex::encode(tx.tx_blob),
                                        tx_version: tx.tx_version,
                                        tx_unlock_time: tx.tx_unlock_time,
                                        tx_input_count: tx.tx_input_count,
                                        tx_input_ring_sizes: tx.tx_input_ring_sizes,
                                        tx_output_count: tx.tx_output_count,
                                        tx_extra_len: tx.tx_extra_len,
                                        rct_type: tx.rct_type,
                                        rct_fee: tx.rct_fee,
                                        dust: tx.dust,
                                        fee: tx.fee,
                                        dust_added_to_fee: tx.dust_added_to_fee,
                                        change_amount: tx.change_amount,
                                        selected_transfer_count: tx.selected_transfer_count,
                                        selected_transfer_indices: tx.selected_transfer_indices,
                                        key_images_len: tx.key_images_len,
                                        key_images_blob_hex: hex::encode(tx.key_images_blob),
                                        tx_key_is_zero: tx.tx_key_is_zero,
                                        tx_key: if tx.tx_key_is_zero {
                                            None
                                        } else {
                                            Some(hex::encode(tx.tx_key))
                                        },
                                        additional_tx_key_count: tx.additional_tx_key_count,
                                        tx_key_additional: tx
                                            .additional_tx_keys
                                            .into_iter()
                                            .map(hex::encode)
                                            .collect(),
                                        destination_count: tx.destination_count,
                                        destination_total_amount: tx.destination_total_amount,
                                        multisig_sig_count: tx.multisig_sig_count,
                                    })
                                    .collect(),
                                key_images: summary.key_images.iter().map(hex::encode).collect(),
                                tx_key_images: summary
                                    .tx_key_images
                                    .iter()
                                    .map(|entry| SignedTxSetKeyImageEntry {
                                        public_key: hex::encode(entry.public_key),
                                        key_image: hex::encode(entry.key_image),
                                    })
                                    .collect(),
                            },
                            Err(e) => signed_txset_error_response(e),
                        }
                    }
                    Err(e) => signed_txset_error_response(e),
                };

            response.send_signal_to_dart();
        });
    }
}

#[async_trait]
impl Notifiable<BuildSignedTxSet> for TxBuilderActor {
    async fn notify(&mut self, msg: BuildSignedTxSet, _ctx: &Context<Self>) {
        spawn_local(async move {
            let response = match build_signed_txset_response(msg) {
                Ok(response) => response,
                Err(e) => signed_txset_built_error_response(e),
            };

            response.send_signal_to_dart();
        });
    }
}

fn decode_txset_request(
    data_hex: &str,
    view_key_hex: &str,
    label: &str,
) -> Result<(Vec<u8>, [u8; 32]), String> {
    let data = hex::decode(data_hex.trim()).map_err(|e| format!("Invalid {label} hex: {e}"))?;
    let view_key_bytes =
        hex::decode(view_key_hex.trim()).map_err(|e| format!("Invalid view key hex: {e}"))?;
    let view_key: [u8; 32] = view_key_bytes
        .try_into()
        .map_err(|_| "View key must be exactly 32 bytes".to_string())?;
    Ok((data, view_key))
}

fn build_signed_txset_response(msg: BuildSignedTxSet) -> Result<SignedTxSetBuiltResponse, String> {
    let (unsigned_txset, view_key) =
        decode_txset_request(&msg.unsigned_txset_hex, &msg.view_key_hex, "unsigned txset")?;
    let tx_blob = hex::decode(msg.tx_blob_hex.trim())
        .map_err(|e| format!("Invalid signed transaction blob hex: {e}"))?;
    let key_images = decode_fixed_hex_list(msg.key_images, "key image")?;
    let tx_key_images = msg
        .tx_key_images
        .into_iter()
        .map(|entry| {
            Ok(monero_rust::epee_compat::SignedTxSetKeyImagePair {
                public_key: decode_fixed_hex(&entry.public_key, "tx key image public key")?,
                key_image: decode_fixed_hex(&entry.key_image, "tx key image key image")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let signed_txset = monero_rust::epee_compat::build_signed_monero_txset(
        monero_rust::epee_compat::BuildSignedTxSetRequest {
            unsigned_txset: &unsigned_txset,
            view_secret_key: &view_key,
            tx_blob: &tx_blob,
            key_images: &key_images,
            tx_key_images: &tx_key_images,
        },
    )?;

    Ok(SignedTxSetBuiltResponse {
        success: true,
        error: None,
        error_code: None,
        error_hint: None,
        error_transient: None,
        signed_txset_hex: Some(hex::encode(signed_txset)),
    })
}

fn decode_fixed_hex_list(values: Vec<String>, label: &str) -> Result<Vec<[u8; 32]>, String> {
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| decode_fixed_hex(&value, &format!("{label} {index}")))
        .collect()
}

fn decode_fixed_hex(hex_value: &str, label: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_value.trim()).map_err(|e| format!("Invalid {label} hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("{label} must be 32 bytes, got {}", bytes.len()))
}

fn unsigned_txset_response(
    summary: monero_rust::epee_compat::ParsedUnsignedTxSetSummary,
) -> Result<UnsignedTxSetInspectedResponse, String> {
    let mut constructions = Vec::with_capacity(summary.txes.len());
    for construction in summary.txes {
        constructions.push(unsigned_construction_summary(construction)?);
    }
    let new_transfer_count = summary.new_transfers.len() as u64;
    Ok(UnsignedTxSetInspectedResponse {
        success: true,
        error: None,
        error_code: None,
        error_hint: None,
        error_transient: None,
        archive_version: summary.archive_version,
        transaction_count: constructions.len() as u64,
        new_transfer_first: summary.new_transfer_first,
        new_transfer_second: summary.new_transfer_second,
        new_transfer_count,
        new_transfers: summary
            .new_transfers
            .into_iter()
            .map(unsigned_transfer_summary)
            .collect(),
        constructions,
    })
}

fn unsigned_construction_summary(
    construction: monero_rust::epee_compat::TxConstructionDataSummary,
) -> Result<UnsignedTxSetConstructionSummary, String> {
    let mut sources = Vec::with_capacity(construction.sources.len());
    for source in construction.sources {
        sources.push(unsigned_source_summary(source)?);
    }

    Ok(UnsignedTxSetConstructionSummary {
        source_count: construction.source_count,
        source_ring_sizes: construction.source_ring_sizes,
        sources,
        change_amount: construction.change_amount,
        change: unsigned_destination_summary(construction.change),
        split_destination_count: construction.split_destination_count,
        split_destination_total_amount: construction.split_destination_total_amount,
        split_destinations: construction
            .split_destinations
            .into_iter()
            .map(unsigned_destination_summary)
            .collect(),
        selected_transfer_indices: construction.selected_transfer_indices,
        extra_hex: hex::encode(construction.extra),
        unlock_time: construction.unlock_time,
        construction_flags: construction.construction_flags,
        use_rct: construction.use_rct,
        use_view_tags: construction.use_view_tags,
        rct_range_proof_type: construction.rct_range_proof_type,
        rct_bp_version: construction.rct_bp_version,
        destination_count: construction.destination_count,
        destination_total_amount: construction.destination_total_amount,
        destinations: construction
            .destinations
            .into_iter()
            .map(unsigned_destination_summary)
            .collect(),
        subaddr_account: construction.subaddr_account,
        subaddr_indices: construction.subaddr_indices,
    })
}

fn unsigned_source_summary(
    source: monero_rust::epee_compat::TxSourceEntrySummary,
) -> Result<UnsignedTxSetSourceSummary, String> {
    let real_index: usize = source
        .real_output
        .try_into()
        .map_err(|_| "Source real output index exceeds usize".to_string())?;
    let real_output = source
        .ring
        .get(real_index)
        .ok_or_else(|| "Source real output index is outside ring".to_string())?;
    let ring = source
        .ring
        .iter()
        .map(|entry| UnsignedTxSetSourceRingEntry {
            global_output_index: entry.global_output_index,
            output_public_key: hex::encode(entry.output_public_key),
            commitment: hex::encode(entry.commitment),
        })
        .collect();
    Ok(UnsignedTxSetSourceSummary {
        ring_size: source.ring.len() as u64,
        ring,
        real_output: source.real_output,
        real_global_output_index: real_output.global_output_index,
        real_output_public_key: hex::encode(real_output.output_public_key),
        real_tx_public_key: hex::encode(source.real_out_tx_key),
        real_out_additional_tx_keys: source
            .real_out_additional_tx_keys
            .iter()
            .map(hex::encode)
            .collect(),
        real_output_in_tx_index: source.real_output_in_tx_index,
        amount: source.amount,
        rct: source.rct,
        mask: hex::encode(source.mask),
    })
}

fn unsigned_destination_summary(
    destination: monero_rust::epee_compat::TxDestinationEntrySummary,
) -> UnsignedTxSetDestinationSummary {
    UnsignedTxSetDestinationSummary {
        original_address_hex: hex::encode(destination.original),
        amount: destination.amount,
        spend_public_key: hex::encode(destination.spend_public_key),
        view_public_key: hex::encode(destination.view_public_key),
        is_subaddress: destination.is_subaddress,
        is_integrated: destination.is_integrated,
    }
}

fn unsigned_transfer_summary(
    transfer: monero_rust::epee_compat::ExportedTransferDetails,
) -> UnsignedTxSetTransferSummary {
    UnsignedTxSetTransferSummary {
        output_public_key: hex::encode(transfer.output_public_key),
        internal_output_index: transfer.internal_output_index,
        global_output_index: transfer.global_output_index,
        tx_public_key: hex::encode(transfer.tx_public_key),
        flags_raw: transfer.flags.raw,
        spent: transfer.flags.spent,
        frozen: transfer.flags.frozen,
        rct: transfer.flags.rct,
        key_image_known: transfer.flags.key_image_known,
        key_image_request: transfer.flags.key_image_request,
        key_image_partial: transfer.flags.key_image_partial,
        amount: transfer.amount,
        additional_tx_keys: transfer
            .additional_tx_keys
            .iter()
            .map(hex::encode)
            .collect(),
        subaddress_major: transfer.subaddress_major,
        subaddress_minor: transfer.subaddress_minor,
    }
}

fn unsigned_txset_error_response(msg: String) -> UnsignedTxSetInspectedResponse {
    let err = ErrorResponse::from_string(&msg);
    UnsignedTxSetInspectedResponse {
        success: false,
        error: Some(msg),
        error_code: Some(err.code),
        error_hint: err.hint,
        error_transient: Some(err.transient),
        archive_version: 0,
        transaction_count: 0,
        new_transfer_first: 0,
        new_transfer_second: 0,
        new_transfer_count: 0,
        new_transfers: Vec::new(),
        constructions: Vec::new(),
    }
}

fn decode_private_key_hex(hex_value: &str, label: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_value).map_err(|e| format!("Invalid {label} hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("{label} must be 32 bytes, got {}", bytes.len()))
}

fn signed_txset_error_response(msg: String) -> SignedTxSetExtractedResponse {
    let err = ErrorResponse::from_string(&msg);
    SignedTxSetExtractedResponse {
        success: false,
        error: Some(msg),
        error_code: Some(err.code),
        error_hint: err.hint,
        error_transient: Some(err.transient),
        transactions: Vec::new(),
        key_images: Vec::new(),
        tx_key_images: Vec::new(),
    }
}

fn signed_txset_built_error_response(msg: String) -> SignedTxSetBuiltResponse {
    let err = ErrorResponse::from_string(&msg);
    SignedTxSetBuiltResponse {
        success: false,
        error: Some(msg),
        error_code: Some(err.code),
        error_hint: err.hint,
        error_transient: Some(err.transient),
        signed_txset_hex: None,
    }
}

#[async_trait]
impl Notifiable<SignUnsignedTx> for TxBuilderActor {
    async fn notify(&mut self, msg: SignUnsignedTx, _ctx: &Context<Self>) {
        let seed = msg.seed;
        let unsigned_tx_hex = msg.unsigned_tx_hex;
        let network = msg.network;
        let spend_secret_key_hex = msg.spend_secret_key_hex;
        let view_secret_key_hex = msg.view_secret_key_hex;

        spawn_local(async move {
            let spend_secret_key_hex = spend_secret_key_hex.filter(|key| !key.is_empty());
            let view_secret_key_hex = view_secret_key_hex.filter(|key| !key.is_empty());
            let result = match (spend_secret_key_hex, view_secret_key_hex) {
                (Some(spend_hex), Some(view_hex)) => {
                    decode_private_key_hex(&spend_hex, "spend secret key").and_then(|spend_key| {
                        decode_private_key_hex(&view_hex, "view secret key").and_then(|view_key| {
                            monero_rust::tx_builder::sign_unsigned_transaction_with_private_keys(
                                spend_key,
                                view_key,
                                &unsigned_tx_hex,
                                &network,
                            )
                        })
                    })
                }
                (None, None) => monero_rust::tx_builder::sign_unsigned_transaction(
                    &seed,
                    &unsigned_tx_hex,
                    &network,
                ),
                _ => Err(
                    "Both spend and view secret keys are required for key-based signing"
                        .to_string(),
                ),
            };

            match result {
                Ok(result) => {
                    TransactionSignedOfflineResponse {
                        success: true,
                        error: None,
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        tx_id: Some(result.tx_id),
                        fee: result.fee,
                        tx_blob: Some(result.tx_blob),
                        signed_txset_hex: None,
                        tx_key: Some(result.tx_key),
                        tx_key_additional: result.tx_key_additional,
                        spent_key_images: result.spent_key_images,
                        change_outputs: result
                            .change_outputs
                            .into_iter()
                            .map(|co| ChangeOutput {
                                tx_hash: co.tx_hash,
                                output_index: co.output_index,
                                amount: co.amount,
                                amount_xmr: co.amount_xmr,
                                key: co.key,
                                key_offset: co.key_offset,
                                commitment_mask: co.commitment_mask,
                                subaddress_index: co.subaddress_index,
                                received_output_bytes: co.received_output_bytes,
                                key_image: co.key_image,
                            })
                            .collect(),
                    }
                    .send_signal_to_dart();
                }
                Err(e) => {
                    TransactionSignedOfflineResponse {
                        success: false,
                        error: Some(e),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        tx_id: None,
                        fee: 0,
                        tx_blob: None,
                        signed_txset_hex: None,
                        tx_key: None,
                        tx_key_additional: vec![],
                        change_outputs: vec![],
                        spent_key_images: vec![],
                    }
                    .send_signal_to_dart();
                }
            }
        });
    }
}

#[async_trait]
impl Notifiable<ExportKeyImages> for TxBuilderActor {
    async fn notify(&mut self, _msg: ExportKeyImages, _ctx: &Context<Self>) {
        if let Some(wallet_addr) = &mut self.wallet_actor {
            let wallet_data_result = wallet_addr.send(GetWalletData).await;
            match wallet_data_result {
                Ok(wallet_data) => {
                    // Sort outputs to match import ordering
                    let mut sorted_outputs: Vec<monero_rust::WalletOutput> = wallet_data
                        .outputs
                        .iter()
                        .filter(|o| !o.key_image.is_empty())
                        .cloned()
                        .collect();
                    sorted_outputs.sort_by(|a, b| {
                        a.block_height
                            .cmp(&b.block_height)
                            .then(a.output_index.cmp(&b.output_index))
                    });

                    let count = sorted_outputs.len() as u64;

                    // Use v3 format with per-key-image ring signatures and view-key encryption
                    match monero_rust::key_image_signing::export_key_images_from_outputs(
                        &_msg.seed,
                        &_msg.passphrase,
                        &sorted_outputs,
                    ) {
                        Ok(data) => {
                            KeyImagesExportedResponse {
                                success: true,
                                error: None,
                                error_code: None,
                                error_hint: None,
                                error_transient: None,
                                key_images_hex: Some(hex::encode(data)),
                                count,
                            }
                            .send_signal_to_dart();
                        }
                        Err(e) => {
                            KeyImagesExportedResponse {
                                success: false,
                                error: Some(e),
                                error_code: None,
                                error_hint: None,
                                error_transient: None,
                                key_images_hex: None,
                                count: 0,
                            }
                            .send_signal_to_dart();
                        }
                    }
                }
                Err(_) => {
                    KeyImagesExportedResponse {
                        success: false,
                        error: Some("Failed to get wallet data".to_string()),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        key_images_hex: None,
                        count: 0,
                    }
                    .send_signal_to_dart();
                }
            }
        } else {
            KeyImagesExportedResponse {
                success: false,
                error: Some("Wallet not initialized".to_string()),
                error_code: None,
                error_hint: None,
                error_transient: None,
                key_images_hex: None,
                count: 0,
            }
            .send_signal_to_dart();
        }
    }
}

#[async_trait]
impl Notifiable<ImportKeyImages> for TxBuilderActor {
    async fn notify(&mut self, msg: ImportKeyImages, _ctx: &Context<Self>) {
        if let Some(wallet_addr) = &mut self.wallet_actor {
            let data = match hex::decode(&msg.data_hex) {
                Ok(d) => d,
                Err(e) => {
                    KeyImagesImportedResponse {
                        success: false,
                        error: Some(format!("Invalid hex data: {:?}", e)),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        imported_count: 0,
                        spent_count: 0,
                        key_images: vec![],
                        spent_key_images: vec![],
                    }
                    .send_signal_to_dart();
                    return;
                }
            };

            let key_images = match monero_rust::key_image_signing::import_key_images_from_seed(
                &msg.seed,
                &msg.passphrase,
                &data,
            ) {
                Ok(kis) => kis,
                Err(e) => {
                    KeyImagesImportedResponse {
                        success: false,
                        error: Some(format!("Failed to parse key images: {}", e)),
                        error_code: None,
                        error_hint: None,
                        error_transient: None,
                        imported_count: 0,
                        spent_count: 0,
                        key_images: vec![],
                        spent_key_images: vec![],
                    }
                    .send_signal_to_dart();
                    return;
                }
            };

            let imported_count = key_images.len() as u64;

            // Assign key images positionally to wallet outputs so the view-only
            // wallet can later detect spends during blockchain scanning. The
            // wallet actor sorts outputs by (block_height, output_index) to match
            // Monero's key image export ordering.
            let _ = wallet_addr
                .notify(UpdateOutputKeyImages {
                    key_images: key_images.clone(),
                })
                .await;

            let node_url = msg.node_url;
            let response_key_images = key_images.clone();
            let non_empty_kis: Vec<String> =
                key_images.into_iter().filter(|ki| !ki.is_empty()).collect();

            if non_empty_kis.is_empty() || node_url.is_empty() {
                KeyImagesImportedResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    imported_count,
                    spent_count: 0,
                    key_images: response_key_images,
                    spent_key_images: vec![],
                }
                .send_signal_to_dart();
                return;
            }

            spawn_local(async move {
                let spent_key_images =
                    match monero_rust::is_key_image_spent(&node_url, &non_empty_kis).await {
                        Ok(statuses) => non_empty_kis
                            .iter()
                            .zip(statuses.iter())
                            .filter_map(
                                |(ki, &status)| {
                                    if status > 0 {
                                        Some(ki.clone())
                                    } else {
                                        None
                                    }
                                },
                            )
                            .collect::<Vec<_>>(),
                        Err(_) => vec![],
                    };
                let spent_count = spent_key_images.len() as u64;

                KeyImagesImportedResponse {
                    success: true,
                    error: None,
                    error_code: None,
                    error_hint: None,
                    error_transient: None,
                    imported_count,
                    spent_count,
                    key_images: response_key_images,
                    spent_key_images,
                }
                .send_signal_to_dart();
            });
        } else {
            KeyImagesImportedResponse {
                success: false,
                error: Some("Wallet not initialized".to_string()),
                error_code: None,
                error_hint: None,
                error_transient: None,
                imported_count: 0,
                spent_count: 0,
                key_images: vec![],
                spent_key_images: vec![],
            }
            .send_signal_to_dart();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test output
    fn create_output(
        amount: u64,
        tx_hash: &str,
        output_index: u8,
        block_height: u64,
    ) -> monero_rust::WalletOutput {
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
            spent_height: None,
            key_image: format!("key_image_{}", output_index),
            is_coinbase: false,
            frozen: false,
        }
    }

    // Helper function to simulate the input selection logic using core
    fn select_outputs(
        available_outputs: Vec<monero_rust::WalletOutput>,
        total_send_amount: u64,
    ) -> Vec<monero_rust::WalletOutput> {
        monero_rust::select_inputs(&available_outputs, total_send_amount, 1, None)
            .map(|r| r.selected)
            .unwrap_or(available_outputs)
    }

    #[test]
    fn test_scenario_1_single_output_sufficient() {
        // Scenario 1: Send 1 XMR with outputs [5 XMR, 2 XMR, 0.5 XMR, 0.3 XMR]
        // Should use only the 2 XMR output (smallest that covers 1 XMR + fee)
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100), // 5 XMR
            create_output(2_000_000_000_000, "tx2", 0, 101), // 2 XMR
            create_output(500_000_000_000, "tx3", 0, 102),   // 0.5 XMR
            create_output(300_000_000_000, "tx4", 0, 103),   // 0.3 XMR
        ];

        let send_amount = 1_000_000_000_000; // 1 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 1 output
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");

        // Should be the 2 XMR output (smallest sufficient)
        assert_eq!(
            selected[0].amount, 2_000_000_000_000,
            "Should select 2 XMR output"
        );
        assert_eq!(selected[0].tx_hash, "tx2", "Should select the 2 XMR output");
    }

    #[test]
    fn test_scenario_2_multiple_outputs_needed() {
        // Scenario 2: Send 3 XMR with outputs [2 XMR, 1.5 XMR, 0.8 XMR, 0.5 XMR]
        // Should use 2 XMR + 1.5 XMR (largest first to minimize inputs)
        let outputs = vec![
            create_output(2_000_000_000_000, "tx1", 0, 100), // 2 XMR
            create_output(1_500_000_000_000, "tx2", 0, 101), // 1.5 XMR
            create_output(800_000_000_000, "tx3", 0, 102),   // 0.8 XMR
            create_output(500_000_000_000, "tx4", 0, 103),   // 0.5 XMR
        ];

        let send_amount = 3_000_000_000_000; // 3 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 2 outputs (2 XMR + 1.5 XMR = 3.5 XMR covers 3 XMR + fee)
        assert_eq!(
            selected.len(),
            2,
            "Should select exactly 2 outputs (largest first)"
        );

        // Should select the two largest outputs
        assert_eq!(
            selected[0].amount, 2_000_000_000_000,
            "First should be 2 XMR"
        );
        assert_eq!(
            selected[1].amount, 1_500_000_000_000,
            "Second should be 1.5 XMR"
        );

        // Total should be enough to cover amount + fee
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        let estimated_fee = 20_000_000 + (selected.len() as u64 * 15_000_000);
        assert!(
            total >= send_amount + estimated_fee,
            "Total selected ({}) should cover amount + fee ({})",
            total,
            send_amount + estimated_fee
        );
    }

    #[test]
    fn test_scenario_3_small_send_small_output() {
        // Scenario 3: Send 0.5 XMR with outputs [5 XMR, 2 XMR, 0.8 XMR, 0.3 XMR]
        // Should use only the 0.8 XMR output (smallest sufficient)
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100), // 5 XMR
            create_output(2_000_000_000_000, "tx2", 0, 101), // 2 XMR
            create_output(800_000_000_000, "tx3", 0, 102),   // 0.8 XMR
            create_output(300_000_000_000, "tx4", 0, 103),   // 0.3 XMR
        ];

        let send_amount = 500_000_000_000; // 0.5 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 1 output
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");

        // Should be the 0.8 XMR output (smallest sufficient)
        assert_eq!(
            selected[0].amount, 800_000_000_000,
            "Should select 0.8 XMR output"
        );
        assert_eq!(
            selected[0].tx_hash, "tx3",
            "Should select the 0.8 XMR output"
        );
    }

    #[test]
    fn test_avoids_using_all_outputs_unnecessarily() {
        // Verify that we don't use all outputs when only some are needed
        let outputs = vec![
            create_output(1_000_000_000_000, "tx1", 0, 100), // 1 XMR
            create_output(1_000_000_000_000, "tx2", 0, 101), // 1 XMR
            create_output(1_000_000_000_000, "tx3", 0, 102), // 1 XMR
            create_output(1_000_000_000_000, "tx4", 0, 103), // 1 XMR
            create_output(1_000_000_000_000, "tx5", 0, 104), // 1 XMR
        ];

        let send_amount = 500_000_000_000; // 0.5 XMR
        let selected = select_outputs(outputs.clone(), send_amount);

        // Should use only 1 output, not all 5
        assert_eq!(
            selected.len(),
            1,
            "Should use only 1 output, not all available outputs"
        );

        // Verify for a larger amount that still doesn't need all
        let send_amount_2 = 1_500_000_000_000; // 1.5 XMR
        let selected_2 = select_outputs(outputs, send_amount_2);

        // Should use minimal outputs, not all 5
        assert!(
            selected_2.len() <= 3,
            "Should use minimal outputs, not all available"
        );
        assert!(
            selected_2.len() >= 2,
            "Should use at least 2 outputs for 1.5 XMR"
        );
    }

    #[test]
    fn test_exact_amount_match() {
        // Test when we have an output that exactly matches (or very close to) the needed amount
        let outputs = vec![
            create_output(5_000_000_000_000, "tx1", 0, 100), // 5 XMR
            create_output(1_035_000_000_000, "tx2", 0, 101), // 1.035 XMR (~1 XMR + fee)
            create_output(500_000_000_000, "tx3", 0, 102),   // 0.5 XMR
        ];

        let send_amount = 1_000_000_000_000; // 1 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select the output closest to needed amount
        assert_eq!(selected.len(), 1, "Should select exactly 1 output");
        assert_eq!(
            selected[0].amount, 1_035_000_000_000,
            "Should select output close to needed amount + fee"
        );
    }

    #[test]
    fn test_prefers_single_large_over_multiple_small() {
        // Verify we use 1 large output rather than combining many small ones
        let outputs = vec![
            create_output(3_000_000_000_000, "tx_large", 0, 100), // 3 XMR
            create_output(100_000_000_000, "tx1", 0, 101),        // 0.1 XMR
            create_output(100_000_000_000, "tx2", 0, 102),        // 0.1 XMR
            create_output(100_000_000_000, "tx3", 0, 103),        // 0.1 XMR
            create_output(100_000_000_000, "tx4", 0, 104),        // 0.1 XMR
            create_output(100_000_000_000, "tx5", 0, 105),        // 0.1 XMR
        ];

        let send_amount = 400_000_000_000; // 0.4 XMR
        let selected = select_outputs(outputs, send_amount);

        // Even though we have many small outputs, should use the single large one
        assert_eq!(selected.len(), 1, "Should use single large output");
        assert_eq!(
            selected[0].tx_hash, "tx_large",
            "Should select the 3 XMR output"
        );
    }

    #[test]
    fn test_minimize_inputs_and_change() {
        // Send 3 XMR with outputs [0.75, 1, 1.5, 2.5 XMR]
        // Should use 2.5 + 0.75 = 3.25 XMR (2 inputs, locks only ~0.25 XMR as change)
        // NOT 2.5 + 1.0 = 3.5 XMR (2 inputs, locks ~0.5 XMR as change)
        // NOT 2.5 + 1.5 = 4.0 XMR (2 inputs, locks ~1.0 XMR as change)
        // NOT 0.75 + 1 + 1.5 = 3.25 XMR (3 inputs - more inputs is worse)
        let outputs = vec![
            create_output(750_000_000_000, "tx1", 0, 100), // 0.75 XMR
            create_output(1_000_000_000_000, "tx2", 0, 101), // 1 XMR
            create_output(1_500_000_000_000, "tx3", 0, 102), // 1.5 XMR
            create_output(2_500_000_000_000, "tx4", 0, 103), // 2.5 XMR
        ];

        let send_amount = 3_000_000_000_000; // 3 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select exactly 2 outputs (minimizing inputs)
        assert_eq!(
            selected.len(),
            2,
            "Should select 2 outputs to minimize inputs"
        );

        // Should select 2.5 + 0.75 = 3.25 XMR (minimizes locked change)
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(
            total, 3_250_000_000_000,
            "Total should be 3.25 XMR (2.5 + 0.75) to minimize locked change"
        );

        // Verify we have the right outputs
        assert!(
            selected.iter().any(|o| o.amount == 2_500_000_000_000),
            "Should include 2.5 XMR"
        );
        assert!(
            selected.iter().any(|o| o.amount == 750_000_000_000),
            "Should include 0.75 XMR"
        );

        let estimated_fee = 20_000_000 + (2 * 15_000_000); // 50M atomic units
        assert!(
            total >= send_amount + estimated_fee,
            "Should have enough to cover 3 XMR + fee"
        );

        // Change locked = 3.25 - 3.0 - 0.05 = ~0.20 XMR (minimized!)
        let change_locked = total - send_amount - estimated_fee;
        assert!(
            change_locked < 250_000_000_000,
            "Should lock < 0.25 XMR as change"
        );
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
            create_output(500_000_000_000, "tx1", 0, 100), // 0.5 XMR
            create_output(600_000_000_000, "tx2", 0, 101), // 0.6 XMR
            create_output(700_000_000_000, "tx3", 0, 102), // 0.7 XMR
            create_output(800_000_000_000, "tx4", 0, 103), // 0.8 XMR
        ];

        let send_amount = 1_200_000_000_000; // 1.2 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should select 2 outputs
        assert_eq!(selected.len(), 2, "Should select 2 outputs");

        // Should select 0.8 + 0.5 to minimize locked change
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(
            total, 1_300_000_000_000,
            "Total should be 1.3 XMR (0.8 + 0.5)"
        );

        assert!(
            selected.iter().any(|o| o.amount == 800_000_000_000),
            "Should include 0.8 XMR"
        );
        assert!(
            selected.iter().any(|o| o.amount == 500_000_000_000),
            "Should include 0.5 XMR"
        );
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
        assert_eq!(
            selected.len(),
            2,
            "Should use 2 outputs, not many small ones"
        );
        assert_eq!(
            selected[0].amount, 1_500_000_000_000,
            "First should be 1.5 XMR"
        );
        assert_eq!(
            selected[1].amount, 1_000_000_000_000,
            "Second should be 1 XMR"
        );
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
        assert_eq!(
            selected[0].amount, 1_050_000_000_000,
            "Should use 1.05 XMR output"
        );
        assert_eq!(selected[0].tx_hash, "tx_single");
    }

    #[test]
    fn test_three_outputs_minimized_to_two() {
        // Send 5 XMR with outputs [3, 2.5, 1, 0.5, 0.3]
        // Should use 3 + 2.5 = 5.5 XMR (2 inputs)
        // NOT 2.5 + 1 + 0.5 + 0.3 + ... (3+ inputs)
        let outputs = vec![
            create_output(3_000_000_000_000, "tx1", 0, 100), // 3 XMR
            create_output(2_500_000_000_000, "tx2", 0, 101), // 2.5 XMR
            create_output(1_000_000_000_000, "tx3", 0, 102), // 1 XMR
            create_output(500_000_000_000, "tx4", 0, 103),   // 0.5 XMR
            create_output(300_000_000_000, "tx5", 0, 104),   // 0.3 XMR
        ];

        let send_amount = 5_000_000_000_000; // 5 XMR
        let selected = select_outputs(outputs, send_amount);

        // Should use exactly 2 outputs (the two largest)
        assert_eq!(selected.len(), 2, "Should use 2 outputs, not more");
        assert_eq!(
            selected[0].amount, 3_000_000_000_000,
            "First should be 3 XMR"
        );
        assert_eq!(
            selected[1].amount, 2_500_000_000_000,
            "Second should be 2.5 XMR"
        );

        let total: u64 = selected.iter().map(|o| o.amount).sum();
        assert_eq!(total, 5_500_000_000_000, "Total should be 5.5 XMR");
    }
}
