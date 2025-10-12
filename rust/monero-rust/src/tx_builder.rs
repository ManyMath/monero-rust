//! Transaction building.

pub mod native {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use monero_serai::{
        rpc::{Rpc, RpcConnection, DEFAULT_MAX_FEE_PER_BYTE},
        transaction::Transaction,
        wallet::{
            address::{MoneroAddress, Network},
            seed::Seed,
            Change, Decoys, ReceivedOutput, Scanner, SignableTransactionBuilder, SpendableOutput,
            ViewPair, Fee,
            UnsignedTransaction, sign_offline,
        },
    };
    use rand_core::RngCore;

    #[cfg(not(target_arch = "wasm32"))]
    use monero_serai::rpc::HttpRpc;

    #[cfg(target_arch = "wasm32")]
    use crate::rpc_serai::WasmRpcConnection;

    use crate::scanner::{resolve_seed, register_subaddresses, Lookahead, DEFAULT_LOOKAHEAD};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use sha3::{Digest, Keccak256};
    use std::collections::HashSet;
    use zeroize::Zeroizing;

    #[cfg(not(target_arch = "wasm32"))]
    fn raw_tx_endpoint(node_url: &str) -> String {
        let trimmed = node_url.trim_end_matches('/');
        let base = trimmed.strip_suffix("/json_rpc").unwrap_or(trimmed);
        format!("{}/send_raw_transaction", base)
    }

    fn check_send_raw_transaction_response(body: &str) -> Result<(), String> {
        let value: Value = serde_json::from_str(body)
            .map_err(|e| format!("Invalid send_raw_transaction response: {:?}", e))?;
        match value.get("status").and_then(Value::as_str) {
            Some("OK") => Ok(()),
            Some(status) => {
                let reason = value
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or(body);
                Err(format!("{status}: {reason}"))
            }
            None => Err(format!("Unexpected send_raw_transaction response: {body}")),
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChangeOutputInfo {
        pub tx_hash: String,
        pub output_index: u8,
        pub amount: u64,
        pub amount_xmr: String,
        pub key: String,
        pub key_offset: String,
        pub commitment_mask: String,
        pub subaddress_index: Option<(u32, u32)>,
        pub received_output_bytes: String,
        pub key_image: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TransactionResult {
        pub tx_id: String,
        pub fee: u64,
        pub tx_blob: String,
        /// The private transaction key (r scalar), hex-encoded.
        /// This key is required to prove payments to recipients.
        pub tx_key: String,
        /// Additional private keys for subaddress outputs, hex-encoded.
        pub tx_key_additional: Vec<String>,
        pub change_outputs: Vec<ChangeOutputInfo>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StoredOutputData {
        pub tx_hash: String,
        pub output_index: u8,
        pub amount: u64,
        pub key: String,
        pub key_offset: String,
        pub commitment_mask: String,
        pub subaddress: Option<(u32, u32)>,
        pub payment_id: Option<String>,
        pub received_output_bytes: String,
    }

    /// Ring member data (key + commitment as hex)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RingMember {
        pub key: String,
        pub commitment: String,
    }

    /// Decoy selection result for a single input
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DecoySelection {
        pub real_index: u8,
        pub offsets: Vec<u64>,
        pub ring: Vec<RingMember>,
    }

    /// Result of decoy fetching operation
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DecoyResult {
        pub height: usize,
        pub decoys: Vec<DecoySelection>,
    }

    impl DecoySelection {
        fn from_decoys(d: &Decoys) -> Self {
            DecoySelection {
                real_index: d.i,
                offsets: d.offsets.clone(),
                ring: d.ring.iter().map(|[key, commitment]| {
                    RingMember {
                        key: hex::encode(key.compress().as_bytes()),
                        commitment: hex::encode(commitment.compress().as_bytes()),
                    }
                }).collect(),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FeeEstimate {
        pub fee: u64,
        pub weight: usize,
        pub per_weight: u64,
        pub mask: u64,
        pub inputs: usize,
        pub outputs: usize,
    }

    /// Prepared transaction state, ready for signing.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PreparedTransaction {
        pub node_url: String,
        pub network: String,
        pub recipients: Vec<(String, u64)>, // (address, amount) pairs
        pub fee: u64,
        pub total_input: u64,
        pub change: u64,
        pub stored_outputs: Vec<StoredOutputData>,
    }

    fn extra_weight(outputs: usize, has_payment_id: bool, data_sizes: &[usize]) -> usize {
        // tx public key: tag (1) + key (32)
        let base = 1 + 32;
        // assume additional keys needed (worst case for subaddresses)
        let additional = 1 + 1 + (outputs * 32);
        // payment id: nonce tag (1) + length (1) + encrypted tag (1) + id (8)
        let payment_id = if has_payment_id { 11 } else { 0 };
        // arbitrary data
        let data: usize = data_sizes.iter().map(|len| {
            // nonce tag (1) + varint length + marker (1) + data
            1 + varint_len(1 + len) + 1 + len
        }).sum();
        base + additional + payment_id + data
    }

    fn varint_len(val: usize) -> usize {
        if val < 0x80 { 1 }
        else if val < 0x4000 { 2 }
        else if val < 0x200000 { 3 }
        else if val < 0x10000000 { 4 }
        else { 5 }
    }

    fn scan_transaction_outputs(
        tx: &Transaction,
        tx_id: &str,
        view_pair: ViewPair,
        spend_key: Zeroizing<Scalar>,
        lookahead: Lookahead,
    ) -> Vec<ChangeOutputInfo> {
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
        register_subaddresses(&mut scanner, lookahead);
        let scan_result = scanner.scan_transaction(tx);
        let our_outputs = scan_result.ignore_timelock();

        use monero_serai::ringct::generate_key_image;

        our_outputs
            .into_iter()
            .map(|output| {
                let amount = output.data.commitment.amount;
                let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
                let key = hex::encode(output.data.key.compress().to_bytes());
                let key_offset_scalar = output.data.key_offset;
                let key_offset = hex::encode(key_offset_scalar.to_bytes());
                let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
                let subaddress_index = output.metadata.subaddress.map(|idx| (idx.account(), idx.address()));
                let received_output_bytes = hex::encode(output.serialize());

                let one_time_key_scalar = Zeroizing::new(*spend_key + key_offset_scalar);
                let key_image_point = generate_key_image(&one_time_key_scalar);
                let key_image = hex::encode(key_image_point.compress().to_bytes());

                ChangeOutputInfo {
                    tx_hash: tx_id.to_string(),
                    output_index: output.absolute.o,
                    amount,
                    amount_xmr,
                    key,
                    key_offset,
                    commitment_mask,
                    subaddress_index,
                    received_output_bytes,
                    key_image,
                }
            })
            .collect()
    }

    fn parse_network(network_str: &str) -> Result<Network, String> {
        match network_str.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "stagenet" => Ok(Network::Stagenet),
            _ => Err(format!("Invalid network: {}", network_str)),
        }
    }

    fn spend_key_from_seed(seed: &Seed) -> Zeroizing<Scalar> {
        let entropy = seed.entropy();
        let mut spend_bytes = [0u8; 32];
        spend_bytes.copy_from_slice(&entropy[..]);
        Zeroizing::new(Scalar::from_bytes_mod_order(spend_bytes))
    }

    fn view_pair_from_seed(seed: &Seed) -> ViewPair {
        let entropy = seed.entropy();
        let mut spend_bytes = [0u8; 32];
        spend_bytes.copy_from_slice(&entropy[..]);

        let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
        let spend_point = &spend_scalar * &ED25519_BASEPOINT_TABLE;

        let view: [u8; 32] = Keccak256::digest(spend_bytes).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);

        ViewPair::new(spend_point, Zeroizing::new(view_scalar))
    }

    pub async fn create_spendable_output<R: RpcConnection>(
        rpc: &Rpc<R>,
        received_output: ReceivedOutput,
    ) -> Result<SpendableOutput, String> {
        SpendableOutput::from(rpc, received_output)
            .await
            .map_err(|e| format!("Failed to create spendable output: {:?}", e))
    }

    /// Fetch decoys for a set of outputs to be spent.
    /// Returns decoy ring data that can be cached and used later for transaction signing.
    pub async fn fetch_decoys(
        node_url: &str,
        stored_outputs: Vec<StoredOutputData>,
    ) -> Result<DecoyResult, String> {
        use std::io::Cursor;

        crate::error_codes::validate_node_url(node_url).map_err(|e| e.message.clone())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("RPC error: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let protocol = rpc.get_protocol().await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let height = rpc.get_height().await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;

        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let output_bytes = hex::decode(&stored.received_output_bytes)
                .map_err(|e| format!("Invalid output bytes: {:?}", e))?;
            let mut cursor = Cursor::new(output_bytes);
            let received = ReceivedOutput::read(&mut cursor)
                .map_err(|e| format!("Failed to parse output: {:?}", e))?;
            let spendable = create_spendable_output(&rpc, received).await?;
            spendable_outputs.push(spendable);
        }

        let mut rng = rand::rngs::OsRng;
        let decoys = Decoys::select(
            &mut rng,
            &rpc,
            protocol.ring_len(),
            height.saturating_sub(1),
            &spendable_outputs,
        ).await.map_err(|e| format!("Decoy selection failed: {:?}", e))?;

        Ok(DecoyResult {
            height: height.saturating_sub(1),
            decoys: decoys.iter().map(DecoySelection::from_decoys).collect(),
        })
    }

    /// Estimate transaction fee without building the transaction.
    /// `num_outputs` should include the change output (typically num_destinations + 1).
    /// Maximum 16 outputs due to bulletproofs limit (15 destinations + 1 change, or 16 if no change).
    pub async fn estimate_fee(
        node_url: &str,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<FeeEstimate, String> {
        crate::error_codes::validate_node_url(node_url).map_err(|e| e.message.clone())?;
        if num_inputs == 0 {
            return Err("Must have at least one input".to_string());
        }
        if num_outputs < 2 {
            return Err("Must have at least 2 outputs (destination + change)".to_string());
        }
        if num_outputs > 16 {
            return Err("Maximum 16 outputs allowed (bulletproofs limit)".to_string());
        }

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("RPC error: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let protocol = rpc.get_protocol().await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee_rate: Fee = rpc.get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE).await
            .map_err(|e| format!("Failed to get fee rate: {:?}", e))?;

        // Worst-case extra: assume payment ID and additional keys
        let extra = extra_weight(num_outputs, true, &[]);
        let weight = Transaction::fee_weight(protocol, num_inputs, num_outputs, extra);
        let fee = fee_rate.calculate(weight);

        Ok(FeeEstimate {
            fee,
            weight,
            per_weight: fee_rate.per_weight,
            mask: fee_rate.mask,
            inputs: num_inputs,
            outputs: num_outputs,
        })
    }

    /// Prepare a transaction: validate inputs, calculate fee, check balance.
    /// Returns a PreparedTransaction that can be inspected before signing.
    pub async fn prepare_transaction(
        node_url: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
    ) -> Result<PreparedTransaction, String> {
        crate::error_codes::validate_node_url(node_url).map_err(|e| e.message.clone())?;
        crate::error_codes::validate_network(network_str).map_err(|e| e.message.clone())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }
        if recipients.is_empty() {
            return Err("No recipients provided".to_string());
        }
        if recipients.len() > 15 {
            return Err("Maximum 15 recipients allowed (16 outputs - 1 change)".to_string());
        }

        let network = parse_network(network_str)?;

        // Validate all destination addresses
        for (addr, _) in recipients {
            MoneroAddress::from_str(network, addr)
                .map_err(|e| format!("Invalid destination '{}': {:?}", addr, e))?;
        }

        let total_input: u64 = stored_outputs.iter().map(|o| o.amount).sum();
        let total_amount: u64 = recipients.iter().map(|(_, amt)| *amt).sum();

        // num_outputs = recipients + 1 change
        let num_outputs = recipients.len() + 1;
        let fee_est = estimate_fee(node_url, stored_outputs.len(), num_outputs).await?;

        let total_out = total_amount + fee_est.fee;
        if total_input < total_out {
            return Err(format!(
                "Insufficient funds: have {} piconero, need {} (amount {} + fee {})",
                total_input, total_out, total_amount, fee_est.fee
            ));
        }

        Ok(PreparedTransaction {
            node_url: node_url.to_string(),
            network: network_str.to_string(),
            recipients: recipients.to_vec(),
            fee: fee_est.fee,
            total_input,
            change: total_input - total_out,
            stored_outputs,
        })
    }

    /// Sign a prepared transaction.
    pub async fn sign_prepared_transaction(
        prepared: PreparedTransaction,
        seed_phrase: &str,
    ) -> Result<TransactionResult, String> {
        create_transaction(
            &prepared.node_url,
            seed_phrase,
            &prepared.network,
            prepared.stored_outputs,
            &prepared.recipients,
        ).await
    }

    /// Decision on whether to include a change output.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ChangeDecision {
        /// Include change output (normal case, or single-recipient with any change).
        IncludeChange,
        /// Absorb small change into miner fee (multi-recipient only).
        AbsorbDust,
        /// Change is dust and only 1 recipient — caller should use sweep_all.
        DustError,
    }

    /// Decide whether to include a change output based on the expected change
    /// amount and number of recipients.
    ///
    /// Rules:
    /// - Single-recipient: always include change (monero-serai requires ≥2 outputs,
    ///   so we need either change or a second payment). If the change is dust,
    ///   return DustError so the caller can redirect to sweep_all.
    /// - Multi-recipient (≥2 payments): if change is dust, absorb into miner fee
    ///   by omitting the change output. Otherwise include change.
    pub fn decide_change(expected_change: u64, num_recipients: usize) -> ChangeDecision {
        let is_dust = expected_change > 0
            && expected_change < crate::coin_selection::DUST_THRESHOLD;

        if is_dust {
            if num_recipients < 2 {
                ChangeDecision::DustError
            } else {
                ChangeDecision::AbsorbDust
            }
        } else {
            // Always include change for single-recipient (monero-serai needs ≥2
            // outputs). For expected_change == 0, monero-serai computes the real
            // fee and handles any residual automatically.
            ChangeDecision::IncludeChange
        }
    }

    pub async fn create_transaction(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
    ) -> Result<TransactionResult, String> {
        crate::error_codes::validate_node_url(node_url).map_err(|e| e.message.clone())?;
        crate::error_codes::validate_network(network_str).map_err(|e| e.message.clone())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }
        if recipients.is_empty() {
            return Err("No recipients provided".to_string());
        }
        if recipients.len() > 15 {
            return Err("Maximum 15 recipients allowed (16 outputs - 1 change)".to_string());
        }

        let network = parse_network(network_str)?;

        let seed = resolve_seed(seed_phrase)?;

        let spend_key = spend_key_from_seed(&seed);
        let view_pair = view_pair_from_seed(&seed);

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee = rpc
            .get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE)
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        // Parse and validate all destination addresses
        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination address '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        let mut spendable_outputs = Vec::new();

        use std::io::Cursor;

        for stored in &stored_outputs {
            let output_bytes = hex::decode(&stored.received_output_bytes)
                .map_err(|e| format!("Invalid received_output_bytes: {:?}", e))?;

            let mut cursor = Cursor::new(output_bytes);
            let received_output = ReceivedOutput::read(&mut cursor)
                .map_err(|e| format!("Failed to deserialize ReceivedOutput: {:?}", e))?;

            let spendable = create_spendable_output(&rpc, received_output).await?;
            spendable_outputs.push(spendable);
        }

        // Compute expected change to detect dust
        let total_input: u64 = spendable_outputs.iter()
            .map(|o| o.commitment().amount).sum();
        let total_send: u64 = recipients.iter().map(|(_, amt)| *amt).sum();
        let num_out_with_change = recipients.len() + 1;
        let extra_wc = extra_weight(num_out_with_change, true, &[]);
        let weight_wc = Transaction::fee_weight(
            protocol, spendable_outputs.len(), num_out_with_change, extra_wc,
        );
        let fee_wc = fee.calculate(weight_wc);
        let expected_change = total_input.saturating_sub(total_send + fee_wc);

        let change_decision = decide_change(
            expected_change, recipients.len(),
        );

        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "[create_tx] total_input={} total_send={} fee_wc={} expected_change={} decision={:?} recipients={}",
            total_input, total_send, fee_wc, expected_change, change_decision, recipients.len(),
        );
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!(
            "[create_tx] total_input={} total_send={} fee_wc={} expected_change={} decision={:?} recipients={}",
            total_input, total_send, fee_wc, expected_change, change_decision, recipients.len(),
        ).into());

        match change_decision {
            ChangeDecision::DustError => {
                return Err(
                    "Change amount is dust; use sweep_all for single-recipient send-max"
                        .to_string(),
                );
            }
            _ => {}
        }

        let change_opt = match change_decision {
            ChangeDecision::IncludeChange => Some(Change::new(&view_pair, true)),
            ChangeDecision::AbsorbDust => None,
            ChangeDecision::DustError => unreachable!(),
        };

        let mut builder = SignableTransactionBuilder::new(protocol, fee, change_opt);

        // Generate and set r_seed so we can access the eventuality (and tx_key)
        let mut rng = rand::rngs::OsRng;
        let mut r_seed = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(r_seed.as_mut());
        builder.set_r_seed(r_seed);

        for output in spendable_outputs {
            builder.add_input(output);
        }

        // Add all payments
        for (dest_addr, (_, amount)) in dest_addrs.into_iter().zip(recipients.iter()) {
            builder.add_payment(dest_addr, *amount);
        }

        let signable = builder
            .build()
            .map_err(|e| format!("Failed to build transaction: {:?}", e))?;

        let fee_amount = signable.fee();

        // Get the eventuality to extract the private tx_key before signing
        let eventuality = signable.eventuality()
            .ok_or_else(|| "Failed to get eventuality (r_seed not set)".to_string())?;
        let tx_key = hex::encode(eventuality.tx_key().to_bytes());
        let tx_key_additional: Vec<String> = eventuality.tx_key_additional()
            .iter()
            .map(|k| hex::encode(k.to_bytes()))
            .collect();

        let tx = signable
            .sign(&mut rng, &rpc, &spend_key)
            .await
            .map_err(|e| format!("Failed to sign transaction: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let max_account = stored_outputs.iter()
            .filter_map(|o| o.subaddress.map(|(a, _)| a))
            .max()
            .unwrap_or(0);
        let lookahead = Lookahead {
            account: max_account,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        };
        let change_outputs = scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead);

        Ok(TransactionResult {
            tx_id,
            fee: fee_amount,
            tx_blob,
            tx_key,
            tx_key_additional,
            change_outputs,
        })
    }

    pub async fn sweep_all(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
    ) -> Result<TransactionResult, String> {
        crate::error_codes::validate_node_url(node_url).map_err(|e| e.message.clone())?;
        crate::error_codes::validate_network(network_str).map_err(|e| e.message.clone())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        let network = parse_network(network_str)?;

        let seed = resolve_seed(seed_phrase)?;

        let spend_key = spend_key_from_seed(&seed);
        let view_pair = view_pair_from_seed(&seed);

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee = rpc
            .get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE)
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        // Parse destination address
        let dest_addr = MoneroAddress::from_str(network, destination_address)
            .map_err(|e| format!("Invalid destination address '{}': {:?}", destination_address, e))?;

        // Convert stored outputs to spendable outputs
        let mut spendable_outputs = Vec::new();
        use std::io::Cursor;

        for stored in &stored_outputs {
            let output_bytes = hex::decode(&stored.received_output_bytes)
                .map_err(|e| format!("Invalid received_output_bytes: {:?}", e))?;

            let mut cursor = Cursor::new(output_bytes);
            let received_output = ReceivedOutput::read(&mut cursor)
                .map_err(|e| format!("Failed to deserialize ReceivedOutput: {:?}", e))?;

            let spendable = create_spendable_output(&rpc, received_output).await?;
            spendable_outputs.push(spendable);
        }

        // Calculate total input amount
        let total_in: u64 = spendable_outputs.iter()
            .map(|o| o.commitment().amount)
            .sum();

        // Estimate transaction size for fee calculation
        // Sweep always creates 2 outputs (no change)
        let num_outputs = 2;
        // Worst-case extra: assume payment ID and additional keys
        let extra = extra_weight(num_outputs, true, &[]);
        let estimated_tx_size = Transaction::fee_weight(
            protocol,
            spendable_outputs.len(),
            num_outputs,
            extra,
        );

        let fee_amount = fee.calculate(estimated_tx_size);

        // Calculate sweep amount (total - fee)
        let sweep_amount = total_in.checked_sub(fee_amount)
            .ok_or_else(|| format!(
                "Insufficient funds: have {} atomic units, need {} for fee",
                total_in, fee_amount
            ))?;

        // Monero requires ≥2 outputs for RingCT. Create the real output
        // with the full sweep amount and a 0-value dummy, matching wallet2.

        // Build transaction with NO change address (None)
        let mut builder = SignableTransactionBuilder::new(protocol, fee, None);

        // Generate and set r_seed so we can access the eventuality (and tx_key)
        let mut rng = rand::rngs::OsRng;
        let mut r_seed = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(r_seed.as_mut());
        builder.set_r_seed(r_seed);

        // Add all inputs
        for output in spendable_outputs {
            builder.add_input(output);
        }

        // Add 2 payments to satisfy Monero's privacy requirement
        builder.add_payment(dest_addr, sweep_amount);
        builder.add_payment(dest_addr, 0);

        let signable = builder
            .build()
            .map_err(|e| format!("Failed to build sweep transaction: {:?}", e))?;

        let actual_fee = signable.fee();

        // Get the eventuality to extract the private tx_key before signing
        let eventuality = signable.eventuality()
            .ok_or_else(|| "Failed to get eventuality (r_seed not set)".to_string())?;
        let tx_key = hex::encode(eventuality.tx_key().to_bytes());
        let tx_key_additional: Vec<String> = eventuality.tx_key_additional()
            .iter()
            .map(|k| hex::encode(k.to_bytes()))
            .collect();

        let tx = signable
            .sign(&mut rng, &rpc, &spend_key)
            .await
            .map_err(|e| format!("Failed to sign sweep transaction: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let max_account = stored_outputs.iter()
            .filter_map(|o| o.subaddress.map(|(a, _)| a))
            .max()
            .unwrap_or(0);
        let lookahead = Lookahead {
            account: max_account,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        };
        let change_outputs = scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead);

        Ok(TransactionResult {
            tx_id,
            fee: actual_fee,
            tx_blob,
            tx_key,
            tx_key_additional,
            change_outputs,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn broadcast_transaction(
        node_url: &str,
        tx_blob_hex: &str,
        do_not_relay: bool,
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex)
            .map_err(|e| format!("Invalid hex: {:?}", e))?;

        let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;

        let _rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        let response = reqwest::Client::new()
            .post(raw_tx_endpoint(node_url))
            .json(&serde_json::json!({
                "tx_as_hex": hex::encode(tx.serialize()),
                "do_not_relay": do_not_relay,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to broadcast: {:?}", e))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read daemon response: {:?}", e))?;
        if !status.is_success() {
            return Err(format!("HTTP {}: {}", status, body));
        }
        check_send_raw_transaction_response(&body)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn broadcast_transaction(
        node_url: &str,
        tx_blob_hex: &str,
        do_not_relay: bool,
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex)
            .map_err(|e| format!("Invalid hex: {:?}", e))?;

        let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;

        let conn = WasmRpcConnection::new(node_url.to_string());
        let body = serde_json::to_vec(&serde_json::json!({
            "tx_as_hex": hex::encode(tx.serialize()),
            "do_not_relay": do_not_relay,
        }))
        .map_err(|e| format!("Failed to serialize broadcast request: {:?}", e))?;

        let response = conn
            .post("send_raw_transaction", body)
            .await
            .map_err(|e| format!("Failed to broadcast: {:?}", e))?;
        let response = String::from_utf8(response)
            .map_err(|e| format!("Invalid UTF-8 daemon response: {:?}", e))?;
        check_send_raw_transaction_response(&response)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_received_outputs_from_scan(
        node_url: &str,
        block_height: u64,
        seed_phrase: &str,
        _network_str: &str,
    ) -> Result<Vec<ReceivedOutput>, String> {
        let seed = resolve_seed(seed_phrase)?;

        let view_pair = view_pair_from_seed(&seed);
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));

        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        let block = rpc
            .get_block_by_number(block_height as usize)
            .await
            .map_err(|e| format!("Failed to get block: {:?}", e))?;

        let mut all_transactions = vec![block.miner_tx];

        if !block.txs.is_empty() {
            let fetched_txs = rpc
                .get_transactions(&block.txs)
                .await
                .map_err(|e| format!("Failed to get transactions: {:?}", e))?;
            all_transactions.extend(fetched_txs);
        }

        let mut received_outputs = Vec::new();
        for tx in all_transactions.iter() {
            let scan_result = scanner.scan_transaction(tx);
            let outputs = scan_result.ignore_timelock();
            received_outputs.extend(outputs);
        }

        Ok(received_outputs)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn get_received_outputs_from_scan(
        node_url: &str,
        block_height: u64,
        seed_phrase: &str,
        _network_str: &str,
    ) -> Result<Vec<ReceivedOutput>, String> {
        let seed = resolve_seed(seed_phrase)?;

        let view_pair = view_pair_from_seed(&seed);
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));

        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let block = rpc
            .get_block_by_number(block_height as usize)
            .await
            .map_err(|e| format!("Failed to get block: {:?}", e))?;

        let mut all_transactions = vec![block.miner_tx];

        if !block.txs.is_empty() {
            let fetched_txs = rpc
                .get_transactions(&block.txs)
                .await
                .map_err(|e| format!("Failed to get transactions: {:?}", e))?;
            all_transactions.extend(fetched_txs);
        }

        let mut received_outputs = Vec::new();
        for tx in all_transactions.iter() {
            let scan_result = scanner.scan_transaction(tx);
            let outputs = scan_result.ignore_timelock();
            received_outputs.extend(outputs);
        }

        Ok(received_outputs)
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UnsignedTransactionResult {
        pub unsigned_tx_hex: String,
        pub fee: u64,
        pub recipients: Vec<(String, u64)>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OfflineSignResult {
        pub tx_id: String,
        pub fee: u64,
        pub tx_blob: String,
        pub tx_key: String,
        pub tx_key_additional: Vec<String>,
        pub change_outputs: Vec<ChangeOutputInfo>,
    }

    pub async fn create_unsigned_transaction(
        node_url: &str,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
    ) -> Result<UnsignedTransactionResult, String> {
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }
        if recipients.is_empty() {
            return Err("No recipients provided".to_string());
        }
        if recipients.len() > 15 {
            return Err("Maximum 15 recipients allowed (16 outputs - 1 change)".to_string());
        }

        let network = parse_network(network_str)?;

        let view_bytes = hex::decode(view_key_hex)
            .map_err(|e| format!("Invalid view key hex: {:?}", e))?;
        if view_bytes.len() != 32 {
            return Err("View key must be 32 bytes".to_string());
        }
        let mut view_arr = [0u8; 32];
        view_arr.copy_from_slice(&view_bytes);
        let view_scalar = Scalar::from_bytes_mod_order(view_arr);

        let spend_bytes = hex::decode(pub_spend_key_hex)
            .map_err(|e| format!("Invalid public spend key hex: {:?}", e))?;
        if spend_bytes.len() != 32 {
            return Err("Public spend key must be 32 bytes".to_string());
        }
        let mut spend_arr = [0u8; 32];
        spend_arr.copy_from_slice(&spend_bytes);
        let spend_point = curve25519_dalek::edwards::CompressedEdwardsY(spend_arr)
            .decompress()
            .ok_or("Invalid public spend key point")?;

        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("RPC error: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        let protocol = rpc.get_protocol().await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;
        let fee_rate: Fee = rpc.get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE).await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        use std::io::Cursor;
        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let output_bytes = hex::decode(&stored.received_output_bytes)
                .map_err(|e| format!("Invalid output bytes: {:?}", e))?;
            let mut cursor = Cursor::new(output_bytes);
            let received = ReceivedOutput::read(&mut cursor)
                .map_err(|e| format!("Failed to parse output: {:?}", e))?;
            let spendable = create_spendable_output(&rpc, received).await?;
            spendable_outputs.push(spendable);
        }

        let total_input: u64 = spendable_outputs.iter()
            .map(|o| o.commitment().amount).sum();
        let total_send: u64 = recipients.iter().map(|(_, amt)| *amt).sum();
        let num_out_with_change = recipients.len() + 1;
        let extra_wc = extra_weight(num_out_with_change, true, &[]);
        let weight_wc = Transaction::fee_weight(
            protocol, spendable_outputs.len(), num_out_with_change, extra_wc,
        );
        let fee_wc = fee_rate.calculate(weight_wc);
        let expected_change = total_input.saturating_sub(total_send + fee_wc);

        let change_decision = decide_change(expected_change, recipients.len());
        match change_decision {
            ChangeDecision::DustError => {
                return Err(
                    "Change amount is dust; use sweep_all for single-recipient send-max"
                        .to_string(),
                );
            }
            _ => {}
        }

        let change_opt = match change_decision {
            ChangeDecision::IncludeChange => Some(Change::new(&view_pair, true)),
            ChangeDecision::AbsorbDust => None,
            ChangeDecision::DustError => unreachable!(),
        };

        let mut builder = SignableTransactionBuilder::new(protocol, fee_rate, change_opt);

        let mut rng = rand::rngs::OsRng;
        let mut r_seed = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(r_seed.as_mut());
        builder.set_r_seed(r_seed);

        for output in spendable_outputs {
            builder.add_input(output);
        }
        for (dest_addr, (_, amount)) in dest_addrs.into_iter().zip(recipients.iter()) {
            builder.add_payment(dest_addr, *amount);
        }

        let signable = builder.build()
            .map_err(|e| format!("Failed to build transaction: {:?}", e))?;
        let fee = signable.fee();

        let unsigned = signable.prepare_unsigned(&mut rng, &rpc).await
            .map_err(|e| format!("Failed to prepare unsigned tx: {:?}", e))?;

        let unsigned_bytes = unsigned.serialize();

        Ok(UnsignedTransactionResult {
            unsigned_tx_hex: hex::encode(unsigned_bytes),
            fee,
            recipients: recipients.to_vec(),
        })
    }

    pub fn sign_unsigned_transaction(
        seed_phrase: &str,
        unsigned_tx_hex: &str,
        network_str: &str,
    ) -> Result<OfflineSignResult, String> {
        let seed = resolve_seed(seed_phrase)?;
        let spend_key = spend_key_from_seed(&seed);
        let view_pair = view_pair_from_seed(&seed);

        let unsigned_bytes = hex::decode(unsigned_tx_hex)
            .map_err(|e| format!("Invalid unsigned tx hex: {:?}", e))?;
        let unsigned = UnsignedTransaction::read(&mut std::io::Cursor::new(unsigned_bytes))
            .map_err(|e| format!("Failed to parse unsigned tx: {:?}", e))?;

        let fee = unsigned.fee;

        let mut rng = rand::rngs::OsRng;
        let (tx, tx_key, tx_key_additional) =
            sign_offline(&mut rng, &spend_key, unsigned)
                .map_err(|e| format!("Failed to sign offline: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let max_account = 0u32;  // offline signer doesn't have stored output metadata
        let lookahead = Lookahead {
            account: max_account,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        };
        let change_outputs = scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead);

        let _ = parse_network(network_str)?;

        Ok(OfflineSignResult {
            tx_id,
            fee,
            tx_blob,
            tx_key: hex::encode(tx_key.to_bytes()),
            tx_key_additional: tx_key_additional
                .iter()
                .map(|k| hex::encode(k.to_bytes()))
                .collect(),
            change_outputs,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_varint_len() {
            assert_eq!(varint_len(0), 1);
            assert_eq!(varint_len(0x7f), 1);
            assert_eq!(varint_len(0x80), 2);
            assert_eq!(varint_len(0x3fff), 2);
            assert_eq!(varint_len(0x4000), 3);
            assert_eq!(varint_len(0x1fffff), 3);
            assert_eq!(varint_len(0x200000), 4);
            assert_eq!(varint_len(0xfffffff), 4);
            assert_eq!(varint_len(0x10000000), 5);
            assert_eq!(varint_len(usize::MAX), 5);
        }

        #[test]
        fn test_extra_weight_no_payment_id() {
            // 2 outputs, no payment ID, no data
            let weight = extra_weight(2, false, &[]);
            // base (33) + additional (1 + 1 + 2*32 = 66) = 99
            assert_eq!(weight, 99);
        }

        #[test]
        fn test_extra_weight_with_payment_id() {
            // 2 outputs, with payment ID, no data
            let weight = extra_weight(2, true, &[]);
            // base (33) + additional (66) + payment_id (11) = 110
            assert_eq!(weight, 110);
        }

        #[test]
        fn test_extra_weight_with_data() {
            // 2 outputs, no payment ID, with 10 bytes of data
            let weight = extra_weight(2, false, &[10]);
            // base (33) + additional (66) + data (1 + 1 + 1 + 10 = 13) = 112
            assert_eq!(weight, 112);
        }

        #[test]
        fn test_parse_network_mainnet() {
            assert!(matches!(parse_network("mainnet"), Ok(Network::Mainnet)));
            assert!(matches!(parse_network("Mainnet"), Ok(Network::Mainnet)));
            assert!(matches!(parse_network("MAINNET"), Ok(Network::Mainnet)));
        }

        #[test]
        fn test_parse_network_testnet() {
            assert!(matches!(parse_network("testnet"), Ok(Network::Testnet)));
            assert!(matches!(parse_network("Testnet"), Ok(Network::Testnet)));
        }

        #[test]
        fn test_parse_network_stagenet() {
            assert!(matches!(parse_network("stagenet"), Ok(Network::Stagenet)));
            assert!(matches!(parse_network("Stagenet"), Ok(Network::Stagenet)));
        }

        #[test]
        fn test_parse_network_invalid() {
            assert!(parse_network("invalid").is_err());
            assert!(parse_network("").is_err());
            assert!(parse_network("main").is_err());
        }

        #[test]
        fn test_spend_key_from_seed() {
            // Use test vector from scanner tests
            let seed_phrase = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
            let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string())).unwrap();

            let spend_key = spend_key_from_seed(&seed);

            // The spend key should be deterministic
            let spend_key2 = spend_key_from_seed(&seed);
            assert_eq!(spend_key.to_bytes(), spend_key2.to_bytes());

            // Should match the expected spend key from test vector
            let expected = "29adefc8f67515b4b4bf48031780ab9d071d24f8a674b879ce7f245c37523807";
            let entropy = seed.entropy();
            assert_eq!(hex::encode(&entropy[..]), expected);
        }

        #[test]
        fn test_view_pair_from_seed() {
            let seed_phrase = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
            let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string())).unwrap();

            let view_pair = view_pair_from_seed(&seed);

            // View pair should be deterministic - test by generating the same spend point
            let view_pair2 = view_pair_from_seed(&seed);
            assert_eq!(view_pair.spend().compress().to_bytes(), view_pair2.spend().compress().to_bytes());

            // The view pair should successfully be created for valid seed
            // (detailed field comparisons not possible due to privacy, but we verified determinism)
        }

        #[test]
        fn test_decoy_selection_from_decoys() {
            // Test the conversion from Decoys to DecoySelection
            use curve25519_dalek::edwards::EdwardsPoint;

            // Create mock ring data using a known scalar
            let one_bytes = [1u8; 32];
            let scalar_one = Scalar::from_bytes_mod_order(one_bytes);
            let g: EdwardsPoint = &scalar_one * &ED25519_BASEPOINT_TABLE;
            let ring = vec![
                [g, g],
                [g, g],
            ];

            let decoys = Decoys {
                i: 1,
                offsets: vec![0, 5, 10],
                ring,
            };

            let selection = DecoySelection::from_decoys(&decoys);
            assert_eq!(selection.real_index, 1);
            assert_eq!(selection.offsets, vec![0, 5, 10]);
            assert_eq!(selection.ring.len(), 2);
        }

        #[test]
        fn test_stored_output_data_serialization() {
            let output = StoredOutputData {
                tx_hash: "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806".to_string(),
                output_index: 0,
                amount: 1000000000000,
                key: "abc123".to_string(),
                key_offset: "def456".to_string(),
                commitment_mask: "789ghi".to_string(),
                subaddress: None,
                payment_id: None,
                received_output_bytes: "".to_string(),
            };

            // Test that it can be serialized and deserialized
            let json = serde_json::to_string(&output).unwrap();
            let deserialized: StoredOutputData = serde_json::from_str(&json).unwrap();
            assert_eq!(output.tx_hash, deserialized.tx_hash);
            assert_eq!(output.amount, deserialized.amount);
        }

        #[test]
        fn test_transaction_result_serialization() {
            let result = TransactionResult {
                tx_id: "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806".to_string(),
                fee: 10000000,
                tx_blob: "deadbeef".to_string(),
                tx_key: "abc123".to_string(),
                tx_key_additional: vec!["def456".to_string()],
                change_outputs: vec![],
            };

            let json = serde_json::to_string(&result).unwrap();
            let deserialized: TransactionResult = serde_json::from_str(&json).unwrap();
            assert_eq!(result.tx_id, deserialized.tx_id);
            assert_eq!(result.fee, deserialized.fee);
        }

        #[test]
        fn test_fee_estimate_structure() {
            let estimate = FeeEstimate {
                fee: 10000000,
                weight: 1000,
                per_weight: 10000,
                mask: 10000,
                inputs: 2,
                outputs: 2,
            };

            assert_eq!(estimate.inputs, 2);
            assert_eq!(estimate.outputs, 2);
            assert!(estimate.fee > 0);
        }

        // -- decide_change tests --

        #[test]
        fn test_decide_change_normal_change_single_recipient() {
            // Change well above dust → include change
            let result = decide_change(1_000_000_000, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_normal_change_multi_recipient() {
            let result = decide_change(1_000_000_000, 3);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_zero_change_single_recipient() {
            // Zero change, single recipient → must include change
            // (monero-serai requires ≥2 outputs for RingCT)
            let result = decide_change(0, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_zero_change_multi_recipient() {
            let result = decide_change(0, 2);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_dust_single_recipient() {
            // Dust change, single recipient → error (use sweep_all)
            let result = decide_change(1_000_000, 1);
            assert_eq!(result, ChangeDecision::DustError);
        }

        #[test]
        fn test_decide_change_dust_multi_recipient() {
            // Dust change, multi-recipient → absorb into miner fee
            let result = decide_change(1_000_000, 2);
            assert_eq!(result, ChangeDecision::AbsorbDust);
        }

        #[test]
        fn test_decide_change_at_dust_threshold_boundary() {
            use crate::coin_selection::DUST_THRESHOLD;

            // Just below threshold → dust
            let result = decide_change(DUST_THRESHOLD - 1, 1);
            assert_eq!(result, ChangeDecision::DustError);

            let result = decide_change(DUST_THRESHOLD - 1, 2);
            assert_eq!(result, ChangeDecision::AbsorbDust);

            // At threshold → not dust, include change
            let result = decide_change(DUST_THRESHOLD, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);

            let result = decide_change(DUST_THRESHOLD, 2);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_one_piconero() {
            // 1 piconero is dust
            let result = decide_change(1, 1);
            assert_eq!(result, ChangeDecision::DustError);

            let result = decide_change(1, 3);
            assert_eq!(result, ChangeDecision::AbsorbDust);
        }

        #[test]
        fn test_decide_change_large_change() {
            // 1 XMR change → always include
            let result = decide_change(1_000_000_000_000, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }
    }
}

pub use native::*;
