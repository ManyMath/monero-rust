//! Transaction building.

pub mod native {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use monero_serai::{
        rpc::{Rpc, RpcConnection},
        transaction::Transaction,
        wallet::{
            address::{MoneroAddress, Network},
            seed::Seed,
            Change, Decoys, Fee, ReceivedOutput, Scanner, SignableTransactionBuilder, SpendableOutput,
            ViewPair,
        },
    };

    #[cfg(not(target_arch = "wasm32"))]
    use monero_serai::rpc::HttpRpc;

    #[cfg(target_arch = "wasm32")]
    use crate::rpc_adapter::WasmRpcAdapter;

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
        pub tx_key: String,
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

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RingMember {
        pub key: String,
        pub commitment: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DecoySelection {
        pub real_index: u8,
        pub offsets: Vec<u64>,
        pub ring: Vec<RingMember>,
    }

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

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PreparedTransaction {
        pub node_url: String,
        pub network: String,
        pub destination: String,
        pub amount: u64,
        pub fee: u64,
        pub total_input: u64,
        pub change: u64,
        pub stored_outputs: Vec<StoredOutputData>,
    }

    fn extra_weight(outputs: usize, has_payment_id: bool, data_sizes: &[usize]) -> usize {
        let base = 1 + 32;
        let additional = 1 + 1 + (outputs * 32);
        let payment_id = if has_payment_id { 11 } else { 0 };
        let data: usize = data_sizes.iter().map(|len| {
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

        let view: [u8; 32] = Keccak256::digest(&spend_bytes).into();
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
        let rpc = Rpc::new_with_connection(WasmRpcAdapter::new(node_url.to_string()));

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

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("RPC error: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcAdapter::new(node_url.to_string()));

        let protocol = rpc.get_protocol().await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee_rate: Fee = rpc.get_fee().await
            .map_err(|e| format!("Failed to get fee rate: {:?}", e))?;

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

    pub async fn prepare_transaction(
        node_url: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination: &str,
        amount: u64,
    ) -> Result<PreparedTransaction, String> {
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }
        let network = parse_network(network_str)?;

        MoneroAddress::from_str(network, destination)
            .map_err(|e| format!("Invalid destination: {:?}", e))?;

        let total_input: u64 = stored_outputs.iter().map(|o| o.amount).sum();
        let fee_est = estimate_fee(node_url, stored_outputs.len(), 2).await?;

        let total_out = amount + fee_est.fee;
        if total_input < total_out {
            return Err(format!(
                "Insufficient funds: have {} piconero, need {} (amount {} + fee {})",
                total_input, total_out, amount, fee_est.fee
            ));
        }

        Ok(PreparedTransaction {
            node_url: node_url.to_string(),
            network: network_str.to_string(),
            destination: destination.to_string(),
            amount,
            fee: fee_est.fee,
            total_input,
            change: total_input - total_out,
            stored_outputs,
        })
    }

    pub async fn sign_prepared_transaction(
        prepared: PreparedTransaction,
        seed_phrase: &str,
    ) -> Result<TransactionResult, String> {
        create_transaction(
            &prepared.node_url,
            seed_phrase,
            &prepared.network,
            prepared.stored_outputs,
            &prepared.destination,
            prepared.amount,
        ).await
    }

    pub async fn create_transaction(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination: &str,
        amount: u64,
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
        let rpc = Rpc::new_with_connection(WasmRpcAdapter::new(node_url.to_string()));

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee = rpc
            .get_fee()
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        let dest_addr = MoneroAddress::from_str(network, destination)
            .map_err(|e| format!("Invalid destination address: {:?}", e))?;

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

        for output in spendable_outputs {
            builder.add_input(output);
        }

        builder.add_payment(dest_addr, amount);

        let signable = builder
            .build()
            .map_err(|e| format!("Failed to build transaction: {:?}", e))?;

        let fee_amount = signable.fee();

        let mut rng = rand::rngs::OsRng;
        let tx = signable
            .sign(&mut rng, &rpc, &Zeroizing::new(spend_key))
            .await
            .map_err(|e| format!("Failed to sign transaction: {:?}", e))?;

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let tx_key = "0".repeat(64);

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
            change_outputs,
        })
    }

    pub async fn broadcast_transaction(
        node_url: &str,
        tx_blob_hex: &str,
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex)
            .map_err(|e| format!("Invalid hex: {:?}", e))?;

        let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;

        #[cfg(not(target_arch = "wasm32"))]
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;

        #[cfg(target_arch = "wasm32")]
        let rpc = Rpc::new_with_connection(WasmRpcAdapter::new(node_url.to_string()));

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
}

pub use native::*;
