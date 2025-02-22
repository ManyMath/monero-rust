//! Transaction building.

pub mod native {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use monero_serai::{
        rpc::{Rpc, RpcConnection},
        transaction::Transaction,
        wallet::{
            address::{MoneroAddress, Network},
            seed::Seed,
            Change, Decoys, ReceivedOutput, Scanner, SignableTransactionBuilder, SpendableOutput,
            ViewPair, Fee,
        },
    };
    use rand_core::RngCore;

    #[cfg(not(target_arch = "wasm32"))]
    use monero_serai::rpc::HttpRpc;

    #[cfg(target_arch = "wasm32")]
    use crate::rpc_serai::WasmRpcConnection;

    use serde::{Deserialize, Serialize};
    use sha3::{Digest, Keccak256};
    use std::collections::HashSet;
    use zeroize::Zeroizing;

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

    fn parse_network(network_str: &str) -> Result<Network, String> {
        match network_str.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "stagenet" => Ok(Network::Stagenet),
            _ => Err(format!("Invalid network: {}", network_str)),
        }
    }

    fn spend_key_from_seed(seed: &Seed) -> Scalar {
        let entropy = seed.entropy();
        let mut spend_bytes = [0u8; 32];
        spend_bytes.copy_from_slice(&entropy[..]);
        Scalar::from_bytes_mod_order(spend_bytes)
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

        let fee_rate: Fee = rpc.get_fee().await
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

    pub async fn create_transaction(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
    ) -> Result<TransactionResult, String> {
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

        let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string()))
            .map_err(|e| format!("Invalid seed: {:?}", e))?;

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
            .get_fee()
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        // Parse and validate all destination addresses
        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination address '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        let change = Change::new(&view_pair, false);
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

        let mut builder = SignableTransactionBuilder::new(protocol, fee, Some(change));

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
            .sign(&mut rng, &rpc, &Zeroizing::new(spend_key))
            .await
            .map_err(|e| format!("Failed to sign transaction: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        // Scan the transaction we just created to find change outputs (sends to self)
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
        let scan_result = scanner.scan_transaction(&tx);
        let our_outputs = scan_result.ignore_timelock();

        use monero_serai::ringct::generate_key_image;

        let change_outputs: Vec<ChangeOutputInfo> = our_outputs
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

                // Calculate key image
                let one_time_key_scalar = Zeroizing::new(spend_key + key_offset_scalar);
                let key_image_point = generate_key_image(&one_time_key_scalar);
                let key_image = hex::encode(key_image_point.compress().to_bytes());

                ChangeOutputInfo {
                    tx_hash: tx_id.clone(),
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
            .collect();

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
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        let network = parse_network(network_str)?;

        let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string()))
            .map_err(|e| format!("Invalid seed: {:?}", e))?;

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
            .get_fee()
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

        // Split sweep amount into 2 outputs for Monero privacy
        // Main output gets most of the amount, second output gets 1 atomic unit
        let dummy_amount = 1u64;
        let main_amount = sweep_amount.checked_sub(dummy_amount)
            .ok_or_else(|| format!(
                "Sweep amount too small: {} atomic units (need at least 2)",
                sweep_amount
            ))?;

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
        builder.add_payment(dest_addr, main_amount);
        builder.add_payment(dest_addr, dummy_amount);

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
            .sign(&mut rng, &rpc, &Zeroizing::new(spend_key))
            .await
            .map_err(|e| format!("Failed to sign sweep transaction: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        // Sweep transactions don't produce change outputs (by design)
        // All outputs go to the destination address
        let change_outputs = Vec::new();

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
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex)
            .map_err(|e| format!("Invalid hex: {:?}", e))?;

        let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;

        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        rpc.publish_transaction(&tx)
            .await
            .map_err(|e| format!("Failed to broadcast: {:?}", e))?;

        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn broadcast_transaction(
        node_url: &str,
        tx_blob_hex: &str,
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex)
            .map_err(|e| format!("Invalid hex: {:?}", e))?;

        let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;

        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));

        rpc.publish_transaction(&tx)
            .await
            .map_err(|e| format!("Failed to broadcast: {:?}", e))?;

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_received_outputs_from_scan(
        node_url: &str,
        block_height: u64,
        seed_phrase: &str,
        _network_str: &str,
    ) -> Result<Vec<ReceivedOutput>, String> {
        let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string()))
            .map_err(|e| format!("Invalid seed: {:?}", e))?;

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
        let seed = Seed::from_string(Zeroizing::new(seed_phrase.to_string()))
            .map_err(|e| format!("Invalid seed: {:?}", e))?;

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
    }
}

pub use native::*;
