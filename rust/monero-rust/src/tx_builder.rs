//! Transaction building.

pub mod native {
    use crate::monero_rpc::{Rpc, RpcConnection, DEFAULT_MAX_FEE_PER_BYTE};
    use crate::seed_compat::Seed;
    use crate::unsigned_tx::{
        check_input_ownership, output_with_decoys, sign_offline, UnsignedChange,
        UnsignedTransaction,
    };
    use crate::wallet_compat::{extra_fee_weight, transaction_fee_weight, Fee, Protocol};
    use curve25519_dalek::{
        constants::ED25519_BASEPOINT_TABLE,
        edwards::{CompressedEdwardsY, EdwardsPoint},
        scalar::Scalar,
    };
    use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
    use monero_wallet::address::{AddressType, MoneroAddress, Network, SubaddressIndex};
    use monero_wallet::interface::FeeRate;
    use monero_wallet::ringct::{clsag::Decoys, RctType};
    use monero_wallet::{OutputWithDecoys, ViewPair, WalletOutput};
    use rand_core::{CryptoRng, RngCore};

    /// Compute the key image `x * H_p(xG)` for an owned one-time key.
    fn generate_key_image(secret: &Zeroizing<Scalar>) -> EdwardsPoint {
        let public = &**secret * ED25519_BASEPOINT_TABLE;
        let hash_point: EdwardsPoint =
            monero_wallet::ed25519::Point::biased_hash(public.compress().to_bytes()).into();
        hash_point * **secret
    }

    fn parse_stored_output(received_output_bytes_hex: &str) -> Result<WalletOutput, String> {
        let bytes = hex::decode(received_output_bytes_hex)
            .map_err(|e| format!("Invalid output bytes: {:?}", e))?;
        WalletOutput::read(&mut bytes.as_slice())
            .map_err(|e| format!("Failed to parse output: {}", e))
    }

    /// Ask the daemon for an output's global RingCT index.
    pub async fn resolve_global_index<R: RpcConnection>(
        rpc: &Rpc<R>,
        output: &WalletOutput,
    ) -> Result<u64, String> {
        let indexes = rpc
            .get_o_indexes(output.transaction())
            .await
            .map_err(|e| format!("Failed to resolve the output's global index: {:?}", e))?;
        usize::try_from(output.index_in_transaction())
            .ok()
            .and_then(|position| indexes.get(position).copied())
            .ok_or_else(|| "Daemon returned too few output indexes".to_string())
    }

    async fn spendable_from_stored_bytes<R: RpcConnection>(
        rpc: &Rpc<R>,
        received_output_bytes_hex: &str,
    ) -> Result<(WalletOutput, u64), String> {
        let output = parse_stored_output(received_output_bytes_hex)?;
        let global_index = resolve_global_index(rpc, &output).await?;
        Ok((output, global_index))
    }

    fn spent_output(output: &WalletOutput, global_index: u64) -> crate::decoy_select::SpentOutput {
        crate::decoy_select::SpentOutput {
            index_on_blockchain: global_index,
            key: output.key().into(),
            commitment: output.commitment().commit().into(),
        }
    }

    async fn select_output_decoys<G: RngCore + CryptoRng, R: RpcConnection>(
        rng: &mut G,
        rpc: &Rpc<R>,
        protocol: Protocol,
        height: usize,
        outputs: &[(WalletOutput, u64)],
    ) -> Result<Vec<Decoys>, String> {
        let ring_len = u8::try_from(protocol.ring_len())
            .map_err(|_| "Ring length exceeds supported range".to_string())?;
        let mut decoys = Vec::with_capacity(outputs.len());
        for (output, global_index) in outputs {
            let spent = spent_output(output, *global_index);
            decoys.push(
                crate::decoy_select::select_decoys(rng, rpc, ring_len, height, &spent)
                    .await
                    .map_err(|e| format!("Decoy selection failed: {}", e))?,
            );
        }
        Ok(decoys)
    }

    /// Select decoys for every output and assemble the unsigned transaction.
    pub async fn build_unsigned_transaction<G: RngCore + CryptoRng, R: RpcConnection>(
        rng: &mut G,
        rpc: &Rpc<R>,
        protocol: Protocol,
        fee_rate: Fee,
        outputs: &[(WalletOutput, u64)],
        payments: Vec<(MoneroAddress, u64)>,
        change: Option<UnsignedChange>,
    ) -> Result<UnsignedTransaction, String> {
        let fee_rate = FeeRate::new(fee_rate.per_weight, fee_rate.mask)
            .ok_or_else(|| "Fee rate and quantization mask must be nonzero".to_string())?;
        let height = rpc
            .get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;

        let decoys =
            select_output_decoys(rng, rpc, protocol, height.saturating_sub(1), outputs).await?;
        let mut inputs = Vec::with_capacity(outputs.len());
        for ((output, _), decoys) in outputs.iter().zip(&decoys) {
            inputs.push(output_with_decoys(
                output.key(),
                output.key_offset(),
                output.commitment(),
                decoys,
            )?);
        }

        let mut seed = Zeroizing::new([0u8; 32]);
        rng.fill_bytes(seed.as_mut());

        Ok(UnsignedTransaction {
            rct_type: protocol.rct_type(),
            seed,
            fee_rate,
            inputs,
            payments,
            change,
        })
    }

    fn check_funds(
        protocol: Protocol,
        fee_rate: Fee,
        total_input: u64,
        input_count: usize,
        payments: &[(MoneroAddress, u64)],
        with_change: bool,
    ) -> Result<(), String> {
        let outputs = payments.len() + usize::from(with_change);
        let has_payment_id = outputs == 2
            || payments
                .iter()
                .any(|(address, _)| address.payment_id().is_some());
        let extra = extra_fee_weight(outputs, true, has_payment_id, &[]);
        let fee = fee_rate.calculate(transaction_fee_weight(
            protocol,
            input_count,
            outputs,
            extra,
        ));
        let total_send = crate::tx_utils::checked_amount_sum(payments.iter().map(|p| p.1))
            .ok_or_else(|| "requested amounts overflow".to_string())?;
        let needed = total_send
            .checked_add(fee)
            .ok_or_else(|| "requested amount plus fee overflows".to_string())?;
        if total_input < needed {
            return Err(format!(
                "Insufficient funds: have {} piconero, need {} (amount {} + fee up to {})",
                total_input, needed, total_send, fee
            ));
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    use crate::monero_rpc::HttpRpc;

    use crate::scanner::{resolve_seed, Lookahead, DEFAULT_LOOKAHEAD};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use sha3::{Digest, Keccak256};
    use zeroize::Zeroizing;

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
                real_index: d.signer_index(),
                offsets: d.offsets().to_vec(),
                ring: d
                    .ring()
                    .iter()
                    .map(|[key, commitment]| RingMember {
                        key: hex::encode(key.compress().to_bytes()),
                        commitment: hex::encode(commitment.compress().to_bytes()),
                    })
                    .collect(),
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
        let data: usize = data_sizes
            .iter()
            .map(|len| {
                // nonce tag (1) + varint length + marker (1) + data
                1 + varint_len(1 + len) + 1 + len
            })
            .sum();
        base + additional + payment_id + data
    }

    fn varint_len(val: usize) -> usize {
        if val < 0x80 {
            1
        } else if val < 0x4000 {
            2
        } else if val < 0x200000 {
            3
        } else if val < 0x10000000 {
            4
        } else {
            5
        }
    }

    fn scan_transaction_outputs(
        tx: &Transaction,
        tx_id: &str,
        view_pair: ViewPair,
        spend_key: Zeroizing<Scalar>,
        lookahead: Lookahead,
        change_subaddress: Option<SubaddressIndex>,
    ) -> Vec<ChangeOutputInfo> {
        let mut scanner = monero_wallet::Scanner::new(view_pair);
        crate::scanner::register_subaddresses(&mut scanner, lookahead);
        // Change may go to a subaddress outside the lookahead.
        if let Some(index) = change_subaddress {
            scanner.register_subaddress(index);
        }

        let Ok(tx_hash) = hex::decode(tx_id) else {
            return vec![];
        };
        let Ok(tx_hash) = <[u8; 32]>::try_from(tx_hash) else {
            return vec![];
        };
        let pruned = Transaction::<Pruned>::from(tx.clone());
        let our_outputs = crate::scanner::scan_single_transaction(&mut scanner, tx_hash, &pruned)
            .unwrap_or_default();

        our_outputs
            .into_iter()
            .map(|output| {
                let amount = output.commitment().amount;
                let key = hex::encode(output.key().compress().to_bytes());
                let key_offset_scalar =
                    Scalar::from_bytes_mod_order(<[u8; 32]>::from(output.key_offset()));
                let key_offset = hex::encode(key_offset_scalar.to_bytes());
                let commitment_mask = hex::encode(<[u8; 32]>::from(output.commitment().mask));
                let subaddress_index = output
                    .subaddress()
                    .map(|idx| (idx.account(), idx.address()));
                let received_output_bytes = hex::encode(output.serialize());

                let one_time_key_scalar = Zeroizing::new(*spend_key + key_offset_scalar);
                let key_image_point = generate_key_image(&one_time_key_scalar);
                let key_image = hex::encode(key_image_point.compress().to_bytes());

                ChangeOutputInfo {
                    tx_hash: tx_id.to_string(),
                    output_index: u8::try_from(output.index_in_transaction()).unwrap_or(u8::MAX),
                    amount,
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

    fn spend_key_from_seed(seed: &Seed, passphrase: &str) -> Zeroizing<Scalar> {
        // Apply the seed offset passphrase and derive the spend key. For polyseed,
        // this includes PBKDF2; the seed entropy alone is not the spend key.
        let entropy = seed.key_bytes_with_passphrase(passphrase);
        let mut spend_bytes = [0u8; 32];
        spend_bytes.copy_from_slice(&entropy[..]);
        Zeroizing::new(Scalar::from_bytes_mod_order(spend_bytes))
    }

    fn view_key_from_spend_key(spend_key: &Zeroizing<Scalar>) -> Zeroizing<Scalar> {
        let view: [u8; 32] = Keccak256::digest(spend_key.to_bytes()).into();
        Zeroizing::new(Scalar::from_bytes_mod_order(view))
    }

    fn view_pair_from_seed(seed: &Seed, passphrase: &str) -> ViewPair {
        let spend_key = spend_key_from_seed(seed, passphrase);
        let view_key = view_key_from_spend_key(&spend_key);
        signer_view_pair(&spend_key, &view_key).expect("valid wallet keys")
    }

    fn signer_view_pair(
        spend_key: &Zeroizing<Scalar>,
        view_key: &Zeroizing<Scalar>,
    ) -> Result<ViewPair, String> {
        ViewPair::new(
            monero_wallet::ed25519::Point::from(&**spend_key * ED25519_BASEPOINT_TABLE),
            Zeroizing::new(monero_wallet::ed25519::Scalar::from(**view_key)),
        )
        .map_err(|e| format!("Invalid wallet keys: {}", e))
    }

    /// Derive the private spend and view keys for the offline signing APIs.
    ///
    /// Signing, preview, and key-offset derivation share this helper so they use
    /// the same wallet. Apply the passphrase and reduce the spend scalar before
    /// hashing it to derive the view key, which must also be a canonical scalar.
    /// Wallet2 encryption and subaddress derivation hash these exact key bytes.
    fn private_keys_from_seed(
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<([u8; 32], [u8; 32]), String> {
        let seed = resolve_seed(seed_phrase)?;
        let spend_secret_key = spend_key_from_seed(&seed, passphrase).to_bytes();
        let view_secret_key: [u8; 32] = Keccak256::digest(spend_secret_key).into();
        Ok((
            spend_secret_key,
            Scalar::from_bytes_mod_order(view_secret_key).to_bytes(),
        ))
    }

    /// Fetch decoys for a set of outputs to be spent.
    /// Returns decoy ring data that can be cached and used later for transaction signing.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn fetch_decoys(
        node_url: &str,
        stored_outputs: Vec<StoredOutputData>,
    ) -> Result<DecoyResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        fetch_decoys_with_rpc(&rpc, stored_outputs).await
    }

    pub async fn fetch_decoys_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        stored_outputs: Vec<StoredOutputData>,
    ) -> Result<DecoyResult, String> {
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let height = rpc
            .get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;

        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let spendable = spendable_from_stored_bytes(rpc, &stored.received_output_bytes).await?;
            spendable_outputs.push(spendable);
        }

        let decoys = select_output_decoys(
            &mut rand::rngs::OsRng,
            rpc,
            protocol,
            height.saturating_sub(1),
            &spendable_outputs,
        )
        .await?;

        Ok(DecoyResult {
            height: height.saturating_sub(1),
            decoys: decoys.iter().map(DecoySelection::from_decoys).collect(),
        })
    }

    /// Estimate transaction fee without building the transaction.
    /// `num_outputs` should include the change output (typically num_destinations + 1).
    /// Maximum 16 outputs due to bulletproofs limit (15 destinations + 1 change, or 16 if no change).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn estimate_fee(
        node_url: &str,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<FeeEstimate, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        estimate_fee_with_rpc(&rpc, num_inputs, num_outputs).await
    }

    pub async fn estimate_fee_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
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
        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee_rate: Fee = rpc
            .get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE)
            .await
            .map_err(|e| format!("Failed to get fee rate: {:?}", e))?;

        // Worst-case extra: assume payment ID and additional keys
        let extra = extra_weight(num_outputs, true, &[]);
        let weight = transaction_fee_weight(protocol, num_inputs, num_outputs, extra);
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
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn prepare_transaction(
        node_url: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
    ) -> Result<PreparedTransaction, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        crate::validation::validate_network(network_str).map_err(|e| e.to_string())?;
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

        let total_input =
            crate::tx_utils::checked_amount_sum(stored_outputs.iter().map(|o| o.amount))
                .ok_or_else(|| "input amounts overflow".to_string())?;
        let total_amount =
            crate::tx_utils::checked_amount_sum(recipients.iter().map(|(_, amt)| *amt))
                .ok_or_else(|| "requested amounts overflow".to_string())?;

        // num_outputs = recipients + 1 change
        let num_outputs = recipients.len() + 1;
        let fee_est = estimate_fee(node_url, stored_outputs.len(), num_outputs).await?;

        // Check the sum before comparing funds or calculating change. Overflow
        // would make an excessive payment appear affordable.
        let total_out = total_amount
            .checked_add(fee_est.fee)
            .ok_or_else(|| "requested amount plus fee overflows".to_string())?;
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
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn sign_prepared_transaction(
        prepared: PreparedTransaction,
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<TransactionResult, String> {
        create_transaction(
            &prepared.node_url,
            seed_phrase,
            &prepared.network,
            prepared.stored_outputs,
            &prepared.recipients,
            passphrase,
        )
        .await
    }

    /// Decision on whether to include a change output.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ChangeDecision {
        /// Include change output (normal case, or single-recipient with any change).
        IncludeChange,
        /// Absorb small change into miner fee (multi-recipient only).
        AbsorbDust,
        /// Change is dust and only 1 recipient; caller should use sweep_all.
        DustError,
    }

    /// Decide whether to include a change output based on the expected change
    /// amount and number of recipients.
    ///
    /// Rules:
    /// - Single-recipient: always include change (monero-serai requires >=2 outputs,
    ///   so we need either change or a second payment). If the change is dust,
    ///   return DustError so the caller can redirect to sweep_all.
    /// - Multi-recipient (>=2 payments): if change is dust, absorb into miner fee
    ///   by omitting the change output. Otherwise include change.
    pub fn decide_change(expected_change: u64, num_recipients: usize) -> ChangeDecision {
        let is_dust =
            expected_change > 0 && expected_change < crate::coin_selection::DUST_THRESHOLD;

        if is_dust {
            if num_recipients < 2 {
                ChangeDecision::DustError
            } else {
                ChangeDecision::AbsorbDust
            }
        } else {
            // Always include change for single-recipient (monero-serai needs >=2
            // outputs). For expected_change == 0, monero-serai computes the real
            // fee and handles any residual automatically.
            ChangeDecision::IncludeChange
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_transaction(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        passphrase: &str,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        create_transaction_with_rpc(
            &rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            recipients,
            passphrase,
        )
        .await
    }

    pub async fn create_transaction_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        passphrase: &str,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_network(network_str).map_err(|e| e.to_string())?;
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

        let spend_key = spend_key_from_seed(&seed, passphrase);
        let view_key = view_key_from_spend_key(&spend_key);
        let view_pair = signer_view_pair(&spend_key, &view_key)?;

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

        for stored in &stored_outputs {
            let spendable =
                spendable_from_stored_bytes(&rpc, &stored.received_output_bytes).await?;
            spendable_outputs.push(spendable);
        }

        // Compute expected change to detect dust
        let total_input = crate::tx_utils::checked_amount_sum(
            spendable_outputs
                .iter()
                .map(|(output, _)| output.commitment().amount),
        )
        .ok_or_else(|| "input amounts overflow".to_string())?;
        let total_send =
            crate::tx_utils::checked_amount_sum(recipients.iter().map(|(_, amt)| *amt))
                .ok_or_else(|| "requested amounts overflow".to_string())?;
        let num_out_with_change = recipients.len() + 1;
        let extra_wc = extra_weight(num_out_with_change, true, &[]);
        let weight_wc = transaction_fee_weight(
            protocol,
            spendable_outputs.len(),
            num_out_with_change,
            extra_wc,
        );
        let fee_wc = fee.calculate(weight_wc);
        // Reject overflow before subtracting the send amount and fee from the inputs.
        let expected_change = total_input.saturating_sub(
            total_send
                .checked_add(fee_wc)
                .ok_or_else(|| "requested amount plus fee overflows".to_string())?,
        );

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

        let with_change = match change_decision {
            ChangeDecision::IncludeChange => true,
            ChangeDecision::AbsorbDust => false,
            ChangeDecision::DustError => unreachable!(),
        };

        let payments: Vec<(MoneroAddress, u64)> = dest_addrs
            .into_iter()
            .zip(recipients.iter().map(|(_, amount)| *amount))
            .collect();
        check_funds(
            protocol,
            fee,
            total_input,
            spendable_outputs.len(),
            &payments,
            with_change,
        )?;

        let change = with_change.then(|| UnsignedChange {
            address: view_pair.legacy_address(network),
            subaddress: None,
        });

        // Select decoys online, then sign; the signer returns the tx keys
        let mut rng = rand::rngs::OsRng;
        let unsigned = build_unsigned_transaction(
            &mut rng,
            rpc,
            protocol,
            fee,
            &spendable_outputs,
            payments,
            change,
        )
        .await
        .map_err(|e| format!("Failed to prepare transaction: {}", e))?;
        let signed = sign_offline(&mut rng, &spend_key, &view_key, &unsigned)
            .map_err(|e| format!("Failed to sign transaction: {}", e))?;
        let fee_amount = signed.fee;
        let tx = signed.tx;
        let tx_key = hex::encode(signed.tx_key.to_bytes());
        let tx_key_additional: Vec<String> = signed
            .additional_tx_keys
            .iter()
            .map(|k| hex::encode(k.to_bytes()))
            .collect();

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let max_account = stored_outputs
            .iter()
            .filter_map(|o| o.subaddress.map(|(a, _)| a))
            .max()
            .unwrap_or(0);
        let lookahead = Lookahead {
            account: max_account,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        };
        let change_outputs =
            scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead, None);

        Ok(TransactionResult {
            tx_id,
            fee: fee_amount,
            tx_blob,
            tx_key,
            tx_key_additional,
            change_outputs,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn sweep_all(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
        passphrase: &str,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        sweep_all_with_rpc(
            &rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            destination_address,
            passphrase,
        )
        .await
    }

    pub async fn sweep_all_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
        passphrase: &str,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_network(network_str).map_err(|e| e.to_string())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        let network = parse_network(network_str)?;

        let seed = resolve_seed(seed_phrase)?;

        let spend_key = spend_key_from_seed(&seed, passphrase);
        let view_key = view_key_from_spend_key(&spend_key);
        let view_pair = signer_view_pair(&spend_key, &view_key)?;

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let fee = rpc
            .get_fee_checked(DEFAULT_MAX_FEE_PER_BYTE)
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        // Parse destination address
        let dest_addr = MoneroAddress::from_str(network, destination_address).map_err(|e| {
            format!(
                "Invalid destination address '{}': {:?}",
                destination_address, e
            )
        })?;

        // Convert stored outputs to spendable outputs
        let mut spendable_outputs = Vec::new();

        for stored in &stored_outputs {
            let spendable =
                spendable_from_stored_bytes(&rpc, &stored.received_output_bytes).await?;
            spendable_outputs.push(spendable);
        }

        // Calculate total input amount
        let total_in = crate::tx_utils::checked_amount_sum(
            spendable_outputs
                .iter()
                .map(|(output, _)| output.commitment().amount),
        )
        .ok_or_else(|| "input amounts overflow".to_string())?;

        // Estimate transaction size for fee calculation
        // Sweep always creates 2 outputs (no change)
        let num_outputs = 2;
        // Worst-case extra: assume payment ID and additional keys
        let extra = extra_weight(num_outputs, true, &[]);
        let estimated_tx_size =
            transaction_fee_weight(protocol, spendable_outputs.len(), num_outputs, extra);

        let fee_amount = fee.calculate(estimated_tx_size);

        // Calculate sweep amount (total - fee)
        let sweep_amount = total_in.checked_sub(fee_amount).ok_or_else(|| {
            format!(
                "Insufficient funds: have {} atomic units, need {} for fee",
                total_in, fee_amount
            )
        })?;

        let payments = vec![(dest_addr, sweep_amount), (dest_addr, 0)];

        // Select decoys online, then sign; the signer returns the tx keys
        let mut rng = rand::rngs::OsRng;
        let unsigned = build_unsigned_transaction(
            &mut rng,
            rpc,
            protocol,
            fee,
            &spendable_outputs,
            payments,
            None,
        )
        .await
        .map_err(|e| format!("Failed to prepare sweep transaction: {}", e))?;
        let signed = sign_offline(&mut rng, &spend_key, &view_key, &unsigned)
            .map_err(|e| format!("Failed to sign sweep transaction: {}", e))?;
        let actual_fee = signed.fee;
        let tx = signed.tx;
        let tx_key = hex::encode(signed.tx_key.to_bytes());
        let tx_key_additional: Vec<String> = signed
            .additional_tx_keys
            .iter()
            .map(|k| hex::encode(k.to_bytes()))
            .collect();

        let tx_id = hex::encode(tx.hash());
        let tx_blob = hex::encode(tx.serialize());

        let max_account = stored_outputs
            .iter()
            .filter_map(|o| o.subaddress.map(|(a, _)| a))
            .max()
            .unwrap_or(0);
        let lookahead = Lookahead {
            account: max_account,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        };
        let change_outputs =
            scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead, None);

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
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        broadcast_transaction_with_rpc(&rpc, tx_blob_hex, do_not_relay).await
    }

    pub async fn broadcast_transaction_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        tx_blob_hex: &str,
        do_not_relay: bool,
    ) -> Result<(), String> {
        let tx_bytes = hex::decode(tx_blob_hex).map_err(|e| format!("Invalid hex: {:?}", e))?;
        let tx = Transaction::<NotPruned>::read::<&[u8]>(&mut tx_bytes.as_ref())
            .map_err(|e| format!("Invalid transaction: {:?}", e))?;
        let response: Value = rpc
            .rpc_call(
                "send_raw_transaction",
                Some(serde_json::json!({
                    "tx_as_hex": hex::encode(tx.serialize()),
                    "do_not_relay": do_not_relay,
                })),
            )
            .await
            .map_err(|e| format!("Failed to broadcast: {:?}", e))?;
        check_send_raw_transaction_response(&response.to_string())
    }

    async fn received_outputs_from_scan_inner<R: RpcConnection>(
        rpc: &Rpc<R>,
        block_height: u64,
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<Vec<WalletOutput>, String> {
        let seed = resolve_seed(seed_phrase)?;
        let spend_key = spend_key_from_seed(&seed, passphrase);
        let view_pair = signer_view_pair(&spend_key, &view_key_from_spend_key(&spend_key))?;
        let mut scanner = monero_wallet::Scanner::new(view_pair);

        let block = rpc
            .get_block_by_number(block_height as usize)
            .await
            .map_err(|e| format!("Failed to get block: {:?}", e))?;

        let mut txs_with_hashes = Vec::new();
        let miner_tx = block.miner_transaction();
        txs_with_hashes.push((
            miner_tx.hash(),
            Transaction::<Pruned>::from(miner_tx.clone()),
        ));
        if !block.transactions.is_empty() {
            let fetched_txs = rpc
                .get_transactions(&block.transactions)
                .await
                .map_err(|e| format!("Failed to get transactions: {:?}", e))?;
            for tx in fetched_txs {
                txs_with_hashes.push((tx.hash(), Transaction::<Pruned>::from(tx)));
            }
        }

        let mut received_outputs = Vec::new();
        for (tx_hash, tx) in &txs_with_hashes {
            received_outputs.extend(crate::scanner::scan_single_transaction(
                &mut scanner,
                *tx_hash,
                tx,
            )?);
        }
        Ok(received_outputs)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_received_outputs_from_scan(
        node_url: &str,
        block_height: u64,
        seed_phrase: &str,
        _network_str: &str,
        passphrase: &str,
    ) -> Result<Vec<WalletOutput>, String> {
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        received_outputs_from_scan_inner(&rpc, block_height, seed_phrase, passphrase).await
    }

    pub async fn get_received_outputs_from_scan_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        block_height: u64,
        seed_phrase: &str,
        _network_str: &str,
        passphrase: &str,
    ) -> Result<Vec<WalletOutput>, String> {
        received_outputs_from_scan_inner(rpc, block_height, seed_phrase, passphrase).await
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UnsignedTransactionResult {
        pub unsigned_tx_hex: String,
        pub fee: u64,
        pub recipients: Vec<(String, u64)>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OfflineSignedTransaction {
        pub tx_id: String,
        pub fee: u64,
        pub tx_blob: String,
        pub tx_key: String,
        pub tx_key_additional: Vec<String>,
        pub spent_key_images: Vec<String>,
        pub change_outputs: Vec<ChangeOutputInfo>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OfflineSignResult {
        pub tx_id: String,
        pub fee: u64,
        pub tx_blob: String,
        pub tx_key: String,
        pub tx_key_additional: Vec<String>,
        pub spent_key_images: Vec<String>,
        pub change_outputs: Vec<ChangeOutputInfo>,
        pub transactions: Vec<OfflineSignedTransaction>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Wallet2SourceKeyOffset {
        pub construction_index: usize,
        pub source_index: usize,
        pub real_output: u64,
        pub real_global_output_index: u64,
        pub output_public_key: String,
        pub tx_public_key: String,
        pub key_offset: String,
        pub key_image: String,
        pub subaddress_index: Option<(u32, u32)>,
        pub amount: u64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Wallet2SourceKeyOffsetsResult {
        pub archive_version: u64,
        pub transaction_count: usize,
        pub source_key_offsets: Vec<Wallet2SourceKeyOffset>,
    }

    fn write_varint_u64(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    fn hash_to_scalar(data: &[u8]) -> Scalar {
        let hash: [u8; 32] = Keccak256::digest(data).into();
        Scalar::from_bytes_mod_order(hash)
    }

    fn output_key_offset(
        view_secret: [u8; 32],
        tx_public_key: [u8; 32],
        output_index: u64,
    ) -> Result<Scalar, String> {
        let view_scalar = Scalar::from_bytes_mod_order(view_secret);
        let tx_public_point = CompressedEdwardsY(tx_public_key)
            .decompress()
            .ok_or_else(|| "Invalid source tx public key point".to_string())?;
        let derivation = (view_scalar * tx_public_point).mul_by_cofactor();
        let mut bytes = derivation.compress().to_bytes().to_vec();
        write_varint_u64(output_index, &mut bytes);
        Ok(hash_to_scalar(&bytes))
    }

    fn subaddress_key_offset(view_secret: [u8; 32], account: u32, minor: u32) -> Scalar {
        if account == 0 && minor == 0 {
            return Scalar::ZERO;
        }

        let mut bytes = Vec::with_capacity(8 + 32 + 4 + 4);
        bytes.extend_from_slice(b"SubAddr\0");
        bytes.extend_from_slice(&view_secret);
        bytes.extend_from_slice(&account.to_le_bytes());
        bytes.extend_from_slice(&minor.to_le_bytes());
        hash_to_scalar(&bytes)
    }

    fn derive_source_key_offset(
        source: &crate::epee_compat::TxSourceEntrySummary,
        spend_secret: Scalar,
        view_secret: [u8; 32],
        subaddr_account: u32,
        subaddr_indices: &[u32],
    ) -> Result<(Scalar, [u8; 32], Option<(u32, u32)>), String> {
        let real_output_index: usize = source
            .real_output
            .try_into()
            .map_err(|_| "Wallet2 source real output index exceeds usize".to_string())?;
        let real = source
            .ring
            .get(real_output_index)
            .ok_or_else(|| "Wallet2 source real output index is out of range".to_string())?;

        let mut tx_public_keys = Vec::with_capacity(source.real_out_additional_tx_keys.len() + 1);
        tx_public_keys.push(source.real_out_tx_key);
        tx_public_keys.extend(source.real_out_additional_tx_keys.iter().copied());

        let mut candidate_subaddresses = vec![0u32];
        for index in subaddr_indices {
            if !candidate_subaddresses.contains(index) {
                candidate_subaddresses.push(*index);
            }
        }

        for tx_public_key in tx_public_keys {
            let base_offset =
                output_key_offset(view_secret, tx_public_key, source.real_output_in_tx_index)?;
            for minor in &candidate_subaddresses {
                let subaddress_offset = subaddress_key_offset(view_secret, subaddr_account, *minor);
                let key_offset = base_offset + subaddress_offset;
                let output_secret = spend_secret + key_offset;
                let output_public_key = (&output_secret * ED25519_BASEPOINT_TABLE)
                    .compress()
                    .to_bytes();
                if output_public_key == real.output_public_key {
                    let subaddress_index = if subaddr_account == 0 && *minor == 0 {
                        Some((0, 0))
                    } else {
                        Some((subaddr_account, *minor))
                    };
                    return Ok((key_offset, tx_public_key, subaddress_index));
                }
            }
        }

        Err("Wallet2 source does not belong to the supplied seed".to_string())
    }

    pub fn derive_wallet2_unsigned_source_key_offsets_from_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
    ) -> Result<Wallet2SourceKeyOffsetsResult, String> {
        let spend_secret = Scalar::from_bytes_mod_order(spend_secret_key);
        let summary = crate::epee_compat::parse_unsigned_monero_txset_summary(
            unsigned_txset,
            &view_secret_key,
        )?;
        let mut source_key_offsets = Vec::new();
        for (construction_index, construction) in summary.txes.iter().enumerate() {
            for (source_index, source) in construction.sources.iter().enumerate() {
                let real_output_index: usize = source
                    .real_output
                    .try_into()
                    .map_err(|_| "Wallet2 source real output index exceeds usize".to_string())?;
                let real = source.ring.get(real_output_index).ok_or_else(|| {
                    "Wallet2 source real output index is out of range".to_string()
                })?;
                let (key_offset, tx_public_key, subaddress_index) = derive_source_key_offset(
                    source,
                    spend_secret,
                    view_secret_key,
                    construction.subaddr_account,
                    &construction.subaddr_indices,
                )?;
                let output_secret = Zeroizing::new(spend_secret + key_offset);
                let key_image = generate_key_image(&output_secret).compress().to_bytes();
                source_key_offsets.push(Wallet2SourceKeyOffset {
                    construction_index,
                    source_index,
                    real_output: source.real_output,
                    real_global_output_index: real.global_output_index,
                    output_public_key: hex::encode(real.output_public_key),
                    tx_public_key: hex::encode(tx_public_key),
                    key_offset: hex::encode(key_offset.to_bytes()),
                    key_image: hex::encode(key_image),
                    subaddress_index,
                    amount: source.amount,
                });
            }
        }

        Ok(Wallet2SourceKeyOffsetsResult {
            archive_version: summary.archive_version,
            transaction_count: summary.txes.len(),
            source_key_offsets,
        })
    }

    pub fn derive_wallet2_unsigned_source_key_offsets(
        seed_phrase: &str,
        unsigned_txset: &[u8],
        passphrase: &str,
    ) -> Result<Wallet2SourceKeyOffsetsResult, String> {
        let (spend_secret_key, view_secret_key) = private_keys_from_seed(seed_phrase, passphrase)?;
        derive_wallet2_unsigned_source_key_offsets_from_keys(
            spend_secret_key,
            view_secret_key,
            unsigned_txset,
        )
    }

    fn parse_wallet2_destination_address(
        destination: &crate::epee_compat::TxDestinationEntrySummary,
        network: Network,
    ) -> Result<MoneroAddress, String> {
        if !destination.original.is_empty() {
            let address = std::str::from_utf8(&destination.original)
                .map_err(|e| format!("Wallet2 destination original address is not UTF-8: {e}"))?;
            return MoneroAddress::from_str_with_unchecked_network(address)
                .map_err(|e| format!("Invalid wallet2 destination address: {e:?}"));
        }
        if destination.is_integrated {
            return Err(
                "Wallet2 integrated destination without original address is unsupported"
                    .to_string(),
            );
        }
        let spend = monero_wallet::ed25519::CompressedPoint::from(destination.spend_public_key)
            .decompress()
            .ok_or_else(|| "Invalid wallet2 destination spend public key".to_string())?;
        let view = monero_wallet::ed25519::CompressedPoint::from(destination.view_public_key)
            .decompress()
            .ok_or_else(|| "Invalid wallet2 destination view public key".to_string())?;
        let address_type = if destination.is_subaddress {
            AddressType::Subaddress
        } else {
            AddressType::Legacy
        };
        Ok(MoneroAddress::new(network, address_type, spend, view))
    }

    fn wallet2_input(
        source: &crate::epee_compat::TxSourceEntrySummary,
        key_offset: Scalar,
    ) -> Result<OutputWithDecoys, String> {
        use monero_wallet::ed25519::{
            Commitment, CompressedPoint, Point as OxidePoint, Scalar as OxideScalar,
        };

        let real_output: u8 = source
            .real_output
            .try_into()
            .map_err(|_| "Wallet2 source real output index exceeds u8".to_string())?;
        let real = source
            .ring
            .get(usize::from(real_output))
            .ok_or_else(|| "Wallet2 source real output index is out of range".to_string())?;

        let mut previous = 0u64;
        let mut offsets = Vec::with_capacity(source.ring.len());
        let mut ring = Vec::with_capacity(source.ring.len());
        for member in &source.ring {
            let offset = member
                .global_output_index
                .checked_sub(previous)
                .ok_or_else(|| "Wallet2 source ring global indices are not sorted".to_string())?;
            previous = member.global_output_index;
            offsets.push(offset);
            let output_public_key = CompressedPoint::from(member.output_public_key)
                .decompress()
                .ok_or_else(|| "Invalid wallet2 source ring output public key".to_string())?;
            let commitment = CompressedPoint::from(member.commitment)
                .decompress()
                .ok_or_else(|| "Invalid wallet2 source ring commitment".to_string())?;
            ring.push([output_public_key, commitment]);
        }
        let decoys = monero_wallet::ringct::clsag::Decoys::new(offsets, real_output, ring)
            .ok_or_else(|| "Invalid wallet2 source ring".to_string())?;

        let key: OxidePoint = CompressedPoint::from(real.output_public_key)
            .decompress()
            .ok_or_else(|| "Invalid wallet2 source output public key".to_string())?;
        let mask = Option::<Scalar>::from(Scalar::from_canonical_bytes(source.mask))
            .ok_or_else(|| "Invalid wallet2 source commitment mask".to_string())?;
        output_with_decoys(
            key,
            OxideScalar::from(key_offset),
            &Commitment::new(OxideScalar::from(mask), source.amount),
            &decoys,
        )
    }

    fn wallet2_r_seed(
        unsigned_txset: &[u8],
        tx_count: usize,
        construction_index: usize,
    ) -> Zeroizing<[u8; 32]> {
        if tx_count == 1 && construction_index == 0 {
            return Zeroizing::new(Keccak256::digest(unsigned_txset).into());
        }

        let mut material = Vec::with_capacity(unsigned_txset.len() + 8);
        material.extend_from_slice(unsigned_txset);
        material.extend_from_slice(&(construction_index as u64).to_le_bytes());
        Zeroizing::new(Keccak256::digest(material).into())
    }

    fn create_unsigned_transaction_from_wallet2_construction(
        spend_secret: Scalar,
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
        tx_count: usize,
        construction_index: usize,
        construction: &crate::epee_compat::TxConstructionDataSummary,
        network: Network,
    ) -> Result<UnsignedTransactionResult, String> {
        let mut inputs = Vec::with_capacity(construction.sources.len());
        for source in &construction.sources {
            let (key_offset, _, _) = derive_source_key_offset(
                source,
                spend_secret,
                view_secret_key,
                construction.subaddr_account,
                &construction.subaddr_indices,
            )?;
            inputs.push(wallet2_input(source, key_offset)?);
        }

        let mut payments = Vec::with_capacity(construction.destinations.len());
        let mut recipients = Vec::with_capacity(construction.destinations.len());
        for destination in &construction.destinations {
            let address = parse_wallet2_destination_address(destination, network)?;
            recipients.push((address.to_string(), destination.amount));
            payments.push((address, destination.amount));
        }
        let change = if construction.change_amount > 0 {
            Some(UnsignedChange {
                address: parse_wallet2_destination_address(&construction.change, network)?,
                subaddress: SubaddressIndex::new(construction.subaddr_account, 0),
            })
        } else {
            None
        };

        let total_input = construction
            .sources
            .iter()
            .try_fold(0u64, |acc, source| acc.checked_add(source.amount))
            .ok_or_else(|| "Wallet2 source amount overflow".to_string())?;
        let total_output = construction
            .destination_total_amount
            .checked_add(construction.change_amount)
            .ok_or_else(|| "Wallet2 output amount overflow".to_string())?;
        let fee = total_input
            .checked_sub(total_output)
            .ok_or_else(|| "Wallet2 outputs exceed inputs".to_string())?;

        // A rate of 1 with the fee as mask charges exactly `fee` for any weight
        // up to `fee`.
        let fee_rate =
            FeeRate::new(1, fee).ok_or_else(|| "Wallet2 transaction pays no fee".to_string())?;
        let unsigned = UnsignedTransaction {
            rct_type: RctType::ClsagBulletproofPlus,
            seed: wallet2_r_seed(unsigned_txset, tx_count, construction_index),
            fee_rate,
            inputs,
            payments,
            change,
        };

        let view_pair = signer_view_pair(
            &Zeroizing::new(spend_secret),
            &Zeroizing::new(Scalar::from_bytes_mod_order(view_secret_key)),
        )?;
        let resolved = unsigned.resolve(&view_pair)?;
        let resolved_change = resolved.change.map_or(0, |change| change.amount);
        if resolved.fee != fee || resolved_change != construction.change_amount {
            return Err(format!(
                "Cannot reproduce the wallet2 transaction exactly: wallet2 has fee {} and change \
                 {}, the rebuilt transaction has fee {} and change {}",
                fee, construction.change_amount, resolved.fee, resolved_change
            ));
        }

        Ok(UnsignedTransactionResult {
            unsigned_tx_hex: hex::encode(unsigned.serialize()),
            fee,
            recipients,
        })
    }

    pub fn create_unsigned_transactions_from_wallet2_txset_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
        network_str: &str,
    ) -> Result<Vec<UnsignedTransactionResult>, String> {
        let network = parse_network(network_str)?;
        let spend_secret = Scalar::from_bytes_mod_order(spend_secret_key);
        let summary = crate::epee_compat::parse_unsigned_monero_txset_summary(
            unsigned_txset,
            &view_secret_key,
        )?;
        let tx_count = summary.txes.len();
        summary
            .txes
            .iter()
            .enumerate()
            .map(|(construction_index, construction)| {
                create_unsigned_transaction_from_wallet2_construction(
                    spend_secret,
                    view_secret_key,
                    unsigned_txset,
                    tx_count,
                    construction_index,
                    construction,
                    network,
                )
            })
            .collect()
    }

    pub fn create_unsigned_transaction_from_wallet2_txset_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
        network_str: &str,
    ) -> Result<UnsignedTransactionResult, String> {
        let mut unsigned = create_unsigned_transactions_from_wallet2_txset_keys(
            spend_secret_key,
            view_secret_key,
            unsigned_txset,
            network_str,
        )?;
        if unsigned.len() != 1 {
            return Err(format!(
                "Only single-transaction wallet2 unsigned txsets are supported, got {}",
                unsigned.len()
            ));
        }
        Ok(unsigned.remove(0))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_unsigned_transaction(
        node_url: &str,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        max_fee_per_weight: Option<u64>,
    ) -> Result<UnsignedTransactionResult, String> {
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        create_unsigned_transaction_with_rpc(
            &rpc,
            view_key_hex,
            pub_spend_key_hex,
            network_str,
            stored_outputs,
            recipients,
            max_fee_per_weight,
        )
        .await
    }

    pub async fn create_unsigned_transaction_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        max_fee_per_weight: Option<u64>,
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

        let view_bytes =
            hex::decode(view_key_hex).map_err(|e| format!("Invalid view key hex: {:?}", e))?;
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
        let spend_point = crate::scanner::decompress_public_spend_key(spend_arr)
            .ok_or("Invalid public spend key point")?;

        let view_pair = ViewPair::new(
            monero_wallet::ed25519::Point::from(spend_point),
            Zeroizing::new(monero_wallet::ed25519::Scalar::from(view_scalar)),
        )
        .map_err(|e| format!("Invalid view keys: {}", e))?;

        let protocol = rpc
            .get_protocol()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;
        let fee_rate: Fee = rpc
            .get_fee_checked(max_fee_per_weight.unwrap_or(DEFAULT_MAX_FEE_PER_BYTE))
            .await
            .map_err(|e| format!("Failed to get fee: {:?}", e))?;

        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let spendable = spendable_from_stored_bytes(rpc, &stored.received_output_bytes).await?;
            spendable_outputs.push(spendable);
        }

        let total_input = crate::tx_utils::checked_amount_sum(
            spendable_outputs
                .iter()
                .map(|(output, _)| output.commitment().amount),
        )
        .ok_or_else(|| "input amounts overflow".to_string())?;
        let total_send =
            crate::tx_utils::checked_amount_sum(recipients.iter().map(|(_, amt)| *amt))
                .ok_or_else(|| "requested amounts overflow".to_string())?;
        let num_out_with_change = recipients.len() + 1;
        let extra_wc = extra_weight(num_out_with_change, true, &[]);
        let weight_wc = transaction_fee_weight(
            protocol,
            spendable_outputs.len(),
            num_out_with_change,
            extra_wc,
        );
        let fee_wc = fee_rate.calculate(weight_wc);
        // Reject overflow before subtracting the send amount and fee from the inputs.
        let expected_change = total_input.saturating_sub(
            total_send
                .checked_add(fee_wc)
                .ok_or_else(|| "requested amount plus fee overflows".to_string())?,
        );

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

        let with_change = match change_decision {
            ChangeDecision::IncludeChange => true,
            ChangeDecision::AbsorbDust => false,
            ChangeDecision::DustError => unreachable!(),
        };

        let payments: Vec<(MoneroAddress, u64)> = dest_addrs
            .into_iter()
            .zip(recipients.iter().map(|(_, amount)| *amount))
            .collect();
        check_funds(
            protocol,
            fee_rate,
            total_input,
            spendable_outputs.len(),
            &payments,
            with_change,
        )?;

        let change = with_change.then(|| UnsignedChange {
            address: view_pair.legacy_address(network),
            subaddress: None,
        });

        let mut rng = rand::rngs::OsRng;
        let unsigned = build_unsigned_transaction(
            &mut rng,
            rpc,
            protocol,
            fee_rate,
            &spendable_outputs,
            payments,
            change,
        )
        .await
        .map_err(|e| format!("Failed to prepare unsigned tx: {}", e))?;

        let fee = unsigned.resolve(&view_pair)?.fee;

        Ok(UnsignedTransactionResult {
            unsigned_tx_hex: hex::encode(unsigned.serialize()),
            fee,
            recipients: recipients.to_vec(),
        })
    }

    pub fn sign_unsigned_transaction(
        seed_phrase: &str,
        unsigned_tx_hex: &str,
        network_str: &str,
        passphrase: &str,
        approval: &SigningApproval,
    ) -> Result<OfflineSignResult, String> {
        let (spend_secret_key, view_secret_key) = private_keys_from_seed(seed_phrase, passphrase)?;
        sign_unsigned_transaction_with_private_keys(
            spend_secret_key,
            view_secret_key,
            unsigned_tx_hex,
            network_str,
            approval,
        )
    }

    pub fn sign_unsigned_transaction_with_private_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_tx_hex: &str,
        network_str: &str,
        approval: &SigningApproval,
    ) -> Result<OfflineSignResult, String> {
        let reviewed = preview_unsigned_transaction(
            spend_secret_key,
            view_secret_key,
            unsigned_tx_hex,
            network_str,
        )?;
        if reviewed != approval.reviewed {
            return Err(
                "Refusing to sign: transaction or wallet differs from the approved review".into(),
            );
        }
        reviewed.check_limits(approval.max_total_fee, approval.max_wallet_debit)?;
        let unsigned_bytes = hex::decode(unsigned_tx_hex)
            .map_err(|e| format!("Invalid unsigned tx hex: {:?}", e))?;
        if let Some(kind) = crate::epee_compat::detect_monero_txset(&unsigned_bytes) {
            match kind {
                crate::epee_compat::MoneroTxSetKind::Unsigned => {
                    let converted = create_unsigned_transactions_from_wallet2_txset_keys(
                        spend_secret_key,
                        view_secret_key,
                        &unsigned_bytes,
                        network_str,
                    )
                    .map_err(|e| format!("Failed to convert wallet2 unsigned txset: {e}"))?;
                    let mut signed = Vec::with_capacity(converted.len());
                    for unsigned in converted {
                        let unsigned_bytes = hex::decode(unsigned.unsigned_tx_hex)
                            .map_err(|e| format!("Invalid converted unsigned tx hex: {:?}", e))?;
                        signed.push(sign_app_unsigned_transaction_with_private_keys(
                            spend_secret_key,
                            view_secret_key,
                            &unsigned_bytes,
                            network_str,
                        )?);
                    }
                    return offline_sign_result_from_transactions(signed);
                }
                _ => {
                    return Err(format!(
                        "{} wallet2 containers are signed/import artifacts, not unsigned signer inputs",
                        kind.label()
                    ));
                }
            }
        }
        let signed = sign_app_unsigned_transaction_with_private_keys(
            spend_secret_key,
            view_secret_key,
            &unsigned_bytes,
            network_str,
        )?;
        offline_sign_result_from_transactions(vec![signed])
    }

    /// Convert an oxide `Point` to a dalek `EdwardsPoint`.
    fn addr_point(p: monero_wallet::ed25519::Point) -> EdwardsPoint {
        p.into()
    }

    /// Extract the public keys and derivation mode used to determine ownership.
    ///
    /// The unsigned transaction supplies the network prefix too, but ownership
    /// depends on the keys, subaddress flag, and guaranteed flag. These flags change the sender's
    /// key derivation, so relabeling owned keys can make funds unreceivable.
    /// Comparing encoded addresses would reject our own
    /// change address if it carried a different network prefix.
    pub(crate) fn address_identity(address: &MoneroAddress) -> ([u8; 32], [u8; 32], bool, bool) {
        (
            addr_point(address.spend()).compress().to_bytes(),
            addr_point(address.view()).compress().to_bytes(),
            address.is_subaddress(),
            address.is_guaranteed(),
        )
    }

    /// Choose the offline signer's lookahead without stored account metadata.
    pub(crate) fn signer_lookahead() -> Lookahead {
        Lookahead {
            account: 0,
            subaddress: DEFAULT_LOOKAHEAD.subaddress,
        }
    }

    /// Collect the address identities this wallet owns within `lookahead`.
    ///
    /// Signing and preview use the same set to determine address ownership.
    pub(crate) fn own_address_identities(
        view_pair: &ViewPair,
        network: Network,
        lookahead: Lookahead,
    ) -> Vec<([u8; 32], [u8; 32], bool, bool)> {
        let mut keys = vec![address_identity(&view_pair.legacy_address(network))];
        for index in crate::scanner::build_subaddress_indices(lookahead) {
            keys.push(address_identity(&view_pair.subaddress(network, index)));
        }
        keys
    }

    /// Check whether this wallet owns `address` within its lookahead.
    pub(crate) fn address_is_ours(
        address: &MoneroAddress,
        view_pair: &ViewPair,
        network: Network,
        lookahead: Lookahead,
    ) -> bool {
        let target = address_identity(address);
        own_address_identities(view_pair, network, lookahead)
            .iter()
            .any(|candidate| *candidate == target)
    }

    /// A destination and amount for the user to review before signing.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DestinationPreview {
        pub address: String,
        pub amount: u64,
        /// Whether the unsigned transaction labels this output as change.
        pub is_change: bool,
        /// Whether this wallet owns `address` within the lookahead.
        /// Signing rejects change sent to an address outside this set.
        pub is_ours: bool,
    }

    /// One transaction boundary in the reviewed container. Amounts are piconero.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TransactionSummary {
        pub destinations: Vec<DestinationPreview>,
        /// Total of inputs whose one-time keys were verified against this wallet.
        pub input_total: u64,
        pub fee: u64,
        /// Input total minus all outputs recoverable by this wallet (including self-payments).
        pub wallet_debit: u64,
    }

    /// Complete review of exact unsigned bytes for one wallet and network.
    /// Display all transaction summaries and aggregate totals before approval.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TransactionPreview {
        pub transactions: Vec<TransactionSummary>,
        pub transaction_count: usize,
        pub input_total: u64,
        pub fee: u64,
        pub wallet_debit: u64,
        binding: [u8; 32],
    }

    /// Explicit approval of an exact review and independently selected spending limits.
    /// Construct with `TransactionPreview::approve` only after the user reviews it.
    #[derive(Debug, Clone)]
    pub struct SigningApproval {
        reviewed: TransactionPreview,
        max_total_fee: u64,
        max_wallet_debit: u64,
    }

    impl TransactionPreview {
        fn check_limits(&self, max_total_fee: u64, max_wallet_debit: u64) -> Result<(), String> {
            if self.fee > max_total_fee {
                return Err("Refusing to sign: total fee exceeds the approved limit".into());
            }
            if self.wallet_debit > max_wallet_debit {
                return Err("Refusing to sign: wallet debit exceeds the approved limit".into());
            }
            Ok(())
        }

        /// Record approval after review. Limits must come from the user's policy,
        /// not an unchecked fee or amount supplied by the online wallet.
        pub fn approve(
            &self,
            max_total_fee: u64,
            max_wallet_debit: u64,
        ) -> Result<SigningApproval, String> {
            self.check_limits(max_total_fee, max_wallet_debit)?;
            Ok(SigningApproval {
                reviewed: self.clone(),
                max_total_fee,
                max_wallet_debit,
            })
        }
    }

    fn finish_preview(
        transactions: Vec<TransactionSummary>,
        unsigned_bytes: &[u8],
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        network_str: &str,
    ) -> Result<TransactionPreview, String> {
        use sha3::{Digest, Keccak256};
        parse_network(network_str)?;
        let mut digest = Keccak256::new();
        digest.update(b"monero-rust offline transaction approval v2");
        digest.update((network_str.len() as u64).to_le_bytes());
        digest.update(network_str.as_bytes());
        // Bind public identities, never retain secret key bytes in the review.
        for secret in [spend_secret_key, view_secret_key] {
            digest.update(
                (&Scalar::from_bytes_mod_order(secret) * ED25519_BASEPOINT_TABLE)
                    .compress()
                    .to_bytes(),
            );
        }
        digest.update(unsigned_bytes);
        let sum = |values: Vec<u64>| {
            crate::tx_utils::checked_amount_sum(values)
                .ok_or_else(|| "unsigned transaction summary amounts overflow".to_string())
        };
        Ok(TransactionPreview {
            transaction_count: transactions.len(),
            input_total: sum(transactions.iter().map(|t| t.input_total).collect())?,
            fee: sum(transactions.iter().map(|t| t.fee).collect())?,
            wallet_debit: sum(transactions.iter().map(|t| t.wallet_debit).collect())?,
            transactions,
            binding: digest.finalize().into(),
        })
    }

    /// Preview an unsigned transaction using a seed phrase.
    ///
    /// Derive keys as in `sign_unsigned_transaction` so preview and signing use
    /// the same wallet.
    pub fn preview_unsigned_transaction_from_seed(
        seed_phrase: &str,
        unsigned_tx_hex: &str,
        network_str: &str,
        passphrase: &str,
    ) -> Result<TransactionPreview, String> {
        let (spend_secret_key, view_secret_key) = private_keys_from_seed(seed_phrase, passphrase)?;
        preview_unsigned_transaction(
            spend_secret_key,
            view_secret_key,
            unsigned_tx_hex,
            network_str,
        )
    }

    /// Review destinations, fees, owned inputs, wallet debit, and transaction boundaries.
    ///
    /// The online wallet supplies the transaction and is untrusted. Before signing,
    /// show the user every destination and whether the claimed change belongs to
    /// this wallet. Ownership checks use the same helper as signing.
    ///
    /// Accepts wallet2 unsigned txsets and the app's unsigned transaction format.
    pub fn preview_unsigned_transaction(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_tx_hex: &str,
        network_str: &str,
    ) -> Result<TransactionPreview, String> {
        let unsigned_bytes = hex::decode(unsigned_tx_hex)
            .map_err(|e| format!("Invalid unsigned tx hex: {:?}", e))?;

        if let Some(kind) = crate::epee_compat::detect_monero_txset(&unsigned_bytes) {
            match kind {
                crate::epee_compat::MoneroTxSetKind::Unsigned => {
                    let converted = create_unsigned_transactions_from_wallet2_txset_keys(
                        spend_secret_key,
                        view_secret_key,
                        &unsigned_bytes,
                        network_str,
                    )
                    .map_err(|e| format!("Failed to convert wallet2 unsigned txset: {e}"))?;
                    let mut all = Vec::new();
                    for unsigned in converted {
                        let converted_bytes = hex::decode(unsigned.unsigned_tx_hex)
                            .map_err(|e| format!("Invalid converted unsigned tx hex: {:?}", e))?;
                        all.push(preview_app_unsigned_transaction(
                            spend_secret_key,
                            view_secret_key,
                            &converted_bytes,
                            network_str,
                        )?);
                    }
                    return finish_preview(
                        all,
                        &unsigned_bytes,
                        spend_secret_key,
                        view_secret_key,
                        network_str,
                    );
                }
                _ => {
                    return Err(format!(
                        "{} wallet2 containers are signed/import artifacts, not unsigned signer inputs",
                        kind.label()
                    ));
                }
            }
        }

        let transaction = preview_app_unsigned_transaction(
            spend_secret_key,
            view_secret_key,
            &unsigned_bytes,
            network_str,
        )?;
        finish_preview(
            vec![transaction],
            &unsigned_bytes,
            spend_secret_key,
            view_secret_key,
            network_str,
        )
    }

    fn preview_app_unsigned_transaction(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_bytes: &[u8],
        network_str: &str,
    ) -> Result<TransactionSummary, String> {
        let unsigned = UnsignedTransaction::from_bytes(unsigned_bytes)?;
        let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_secret_key));
        let view_key = Zeroizing::new(Scalar::from_bytes_mod_order(view_secret_key));
        let view_pair = signer_view_pair(&spend_key, &view_key)?;
        let network = parse_network(network_str)?;
        let lookahead = signer_lookahead();
        if unsigned.inputs.is_empty() {
            return Err("unsigned transaction has no inputs".into());
        }
        check_input_ownership(&spend_key, &unsigned).map_err(|_| {
            "Refusing to sign: unsigned input does not belong to this wallet".to_string()
        })?;

        let resolved = unsigned.resolve(&view_pair)?;

        let mut destinations: Vec<_> = unsigned
            .payments
            .iter()
            .map(|(address, amount)| DestinationPreview {
                address: address.to_string(),
                amount: *amount,
                is_change: false,
                is_ours: address_is_ours(address, &view_pair, network, lookahead),
            })
            .collect();
        if let Some(change) = &resolved.change {
            destinations.push(DestinationPreview {
                address: change.address.to_string(),
                amount: change.amount,
                is_change: true,
                is_ours: change.is_ours,
            });
        }

        let input_total = resolved.input_total;
        let output_total =
            crate::tx_utils::checked_amount_sum(destinations.iter().map(|d| d.amount))
                .ok_or_else(|| "unsigned output amounts overflow".to_string())?;
        if output_total.checked_add(resolved.fee) != Some(input_total) {
            return Err("unsigned transaction fee doesn't match its amounts".into());
        }
        let returned = crate::tx_utils::checked_amount_sum(
            destinations.iter().filter(|d| d.is_ours).map(|d| d.amount),
        )
        .ok_or_else(|| "unsigned owned output amounts overflow".to_string())?;
        Ok(TransactionSummary {
            destinations,
            input_total,
            fee: resolved.fee,
            wallet_debit: input_total
                .checked_sub(returned)
                .ok_or_else(|| "unsigned wallet debit underflow".to_string())?,
        })
    }

    fn sign_app_unsigned_transaction_with_private_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_bytes: &[u8],
        network_str: &str,
    ) -> Result<OfflineSignedTransaction, String> {
        let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_secret_key));
        let view_key = Zeroizing::new(Scalar::from_bytes_mod_order(view_secret_key));
        let view_pair = signer_view_pair(&spend_key, &view_key)?;
        parse_network(network_str)?;
        let unsigned = UnsignedTransaction::from_bytes(unsigned_bytes)?;

        let mut rng = rand::rngs::OsRng;
        let signed = sign_offline(&mut rng, &spend_key, &view_key, &unsigned)?;

        let tx = signed.tx;
        let tx_id = hex::encode(tx.hash());
        let tx_bytes = tx.serialize();
        let spent_key_images = crate::scanner::extract_key_images_from_raw_tx(&tx_bytes);
        let tx_blob = hex::encode(tx_bytes);

        let change_outputs = scan_transaction_outputs(
            &tx,
            &tx_id,
            view_pair,
            spend_key,
            signer_lookahead(),
            unsigned.change.and_then(|change| change.subaddress),
        );

        Ok(OfflineSignedTransaction {
            tx_id,
            fee: signed.fee,
            tx_blob,
            tx_key: hex::encode(signed.tx_key.to_bytes()),
            tx_key_additional: signed
                .additional_tx_keys
                .iter()
                .map(|k| hex::encode(k.to_bytes()))
                .collect(),
            spent_key_images,
            change_outputs,
        })
    }

    fn offline_sign_result_from_transactions(
        transactions: Vec<OfflineSignedTransaction>,
    ) -> Result<OfflineSignResult, String> {
        let first = transactions
            .first()
            .ok_or_else(|| "No signed transactions were produced".to_string())?;
        let fee = transactions
            .iter()
            .try_fold(0u64, |acc, tx| acc.checked_add(tx.fee))
            .ok_or_else(|| "Signed transaction fee overflow".to_string())?;
        let spent_key_images = transactions
            .iter()
            .flat_map(|tx| tx.spent_key_images.iter().cloned())
            .collect();
        let change_outputs = transactions
            .iter()
            .flat_map(|tx| tx.change_outputs.iter().cloned())
            .collect();

        Ok(OfflineSignResult {
            tx_id: first.tx_id.clone(),
            fee,
            tx_blob: first.tx_blob.clone(),
            tx_key: first.tx_key.clone(),
            tx_key_additional: first.tx_key_additional.clone(),
            spent_key_images,
            change_outputs,
            transactions,
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

        /// Derive the same polyseed wallet as the scanner and key-image signer.
        ///
        /// Polyseed derives its spend key from entropy through PBKDF2. Using the
        /// entropy directly would select a different wallet; classic seeds do not
        /// expose that mistake because their entropy is the spend key.
        #[test]
        fn seed_derivation_matches_derive_keys_for_a_polyseed() {
            use rand_core::SeedableRng;

            let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(3);
            let seed = Seed::new_polyseed(&mut rng);
            assert_ne!(
                seed.entropy(),
                seed.key_bytes(),
                "premise: a polyseed's key is not its entropy"
            );

            let words = seed.to_string();
            let expected =
                crate::scanner::derive_keys(&words, monero_wallet::address::Network::Stagenet, "")
                    .expect("derive_keys must accept a polyseed");

            let spend = spend_key_from_seed(&seed, "");
            assert_eq!(
                (&*spend * ED25519_BASEPOINT_TABLE).compress().to_bytes(),
                expected.public_spend_key,
                "signer's spend key must be the wallet's spend key"
            );

            let view_pair = view_pair_from_seed(&seed, "");
            assert_eq!(
                view_pair.legacy_address(Network::Stagenet).to_string(),
                expected.address.to_string(),
                "signer's view pair must be the wallet's view pair"
            );
        }

        /// Apply the seed offset passphrase in every seed-based offline signing API.
        ///
        /// The passphrase changes the wallet by subtracting `cn_slow_hash(passphrase)`
        /// from the spend key. All three signing entry points use
        /// `private_keys_from_seed`, so check its result against scanner derivation.
        #[test]
        fn private_keys_from_seed_matches_derive_keys_for_a_passphrase() {
            let words = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
            let passphrase = "hunter2";

            let seed = Seed::from_string(Zeroizing::new(words.to_string())).unwrap();
            assert_ne!(
                seed.key_bytes_with_passphrase(passphrase),
                seed.key_bytes(),
                "premise: the passphrase must change which wallet this mnemonic opens"
            );

            let expected = crate::scanner::derive_keys(
                words,
                monero_wallet::address::Network::Stagenet,
                passphrase,
            )
            .expect("derive_keys must accept a passphrase");
            let (spend_secret_key, view_secret_key) = private_keys_from_seed(words, passphrase)
                .expect("the signer must accept the same passphrase");

            assert_eq!(
                spend_secret_key,
                **expected.secret_spend_key.as_ref().unwrap(),
                "signer's spend key must be the wallet's spend key"
            );
            assert_eq!(
                view_secret_key, *expected.secret_view_key,
                "signer's view key must be the wallet's view key"
            );
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

            let spend_key = spend_key_from_seed(&seed, "");

            // The spend key should be deterministic
            let spend_key2 = spend_key_from_seed(&seed, "");
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

            let spend_key = spend_key_from_seed(&seed, "");
            let view_key = view_key_from_spend_key(&spend_key);
            let view_pair = signer_view_pair(&spend_key, &view_key).unwrap();

            let expected = crate::scanner::derive_keys(seed_phrase, Network::Stagenet, "").unwrap();
            assert_eq!(view_key.to_bytes(), *expected.secret_view_key);
            assert_eq!(
                view_pair.legacy_address(Network::Stagenet),
                expected.address
            );
        }

        #[test]
        fn test_decoy_selection_from_decoys() {
            // Test the conversion from Decoys to DecoySelection
            let g = monero_wallet::ed25519::Point::from(
                &Scalar::from_bytes_mod_order([1u8; 32]) * ED25519_BASEPOINT_TABLE,
            );
            let decoys = Decoys::new(vec![7, 5, 10], 1, vec![[g, g], [g, g], [g, g]]).unwrap();

            let selection = DecoySelection::from_decoys(&decoys);
            assert_eq!(selection.real_index, 1);
            assert_eq!(selection.offsets, vec![7, 5, 10]);
            assert_eq!(selection.ring.len(), 3);
            assert_eq!(selection.ring[0].key, hex::encode(g.compress().to_bytes()));
        }

        fn stored_output_bytes(chunks: u64) -> Vec<u8> {
            let key = &Scalar::from(9u64) * ED25519_BASEPOINT_TABLE;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&[0x11; 32]); // transaction hash
            bytes.extend_from_slice(&1u64.to_le_bytes()); // index in transaction
            bytes.extend_from_slice(&0u64.to_le_bytes()); // index on blockchain
            bytes.extend_from_slice(key.compress().as_bytes());
            bytes.extend_from_slice(&Scalar::from(5u64).to_bytes()); // key offset
            bytes.extend_from_slice(&Scalar::from(3u64).to_bytes()); // commitment mask
            bytes.extend_from_slice(&1234u64.to_le_bytes()); // amount
            bytes.push(0); // no additional timelock
            bytes.push(0); // no subaddress
            bytes.push(0); // no payment id
            monero_oxide::io::VarInt::write(&chunks, &mut bytes).unwrap();
            bytes
        }

        #[test]
        fn decoy_selection_ignores_the_stored_global_index() {
            let output = parse_stored_output(&hex::encode(stored_output_bytes(0))).unwrap();
            assert_eq!(output.index_on_blockchain(), 0);

            let spent = spent_output(&output, 424242);
            assert_eq!(spent.index_on_blockchain, 424242);
            let key: EdwardsPoint = output.key().into();
            assert_eq!(spent.key, key);
            let commitment: EdwardsPoint = output.commitment().commit().into();
            assert_eq!(spent.commitment, commitment);
        }

        #[test]
        fn stored_output_refuses_an_absurd_arbitrary_data_count() {
            // Accept a valid count before testing the limit.
            let mut within = stored_output_bytes(2);
            within.extend_from_slice(&[3, 1, 2, 3]);
            within.extend_from_slice(&[1, 9]);
            let output =
                parse_stored_output(&hex::encode(within)).expect("a real chunk count must read");
            assert_eq!(output.arbitrary_data(), &[vec![1, 2, 3], vec![9]]);

            let error = parse_stored_output(&hex::encode(stored_output_bytes(1_000_000)))
                .expect_err("a million declared chunks must be refused");
            assert!(error.contains("arbitrary data chunks"), "{error}");
        }

        #[test]
        fn test_stored_output_data_serialization() {
            let output = StoredOutputData {
                tx_hash: "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806"
                    .to_string(),
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
                tx_id: "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806"
                    .to_string(),
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
            // Change well above dust -> include change
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
            // Zero change, single recipient -> must include change
            // (monero-serai requires >=2 outputs for RingCT)
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
            // Dust change, single recipient -> error (use sweep_all)
            let result = decide_change(1_000_000, 1);
            assert_eq!(result, ChangeDecision::DustError);
        }

        #[test]
        fn test_decide_change_dust_multi_recipient() {
            // Dust change, multi-recipient -> absorb into miner fee
            let result = decide_change(1_000_000, 2);
            assert_eq!(result, ChangeDecision::AbsorbDust);
        }

        #[test]
        fn test_decide_change_at_dust_threshold_boundary() {
            use crate::coin_selection::DUST_THRESHOLD;

            // Just below threshold -> dust
            let result = decide_change(DUST_THRESHOLD - 1, 1);
            assert_eq!(result, ChangeDecision::DustError);

            let result = decide_change(DUST_THRESHOLD - 1, 2);
            assert_eq!(result, ChangeDecision::AbsorbDust);

            // At threshold -> not dust, include change
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
            // 1 XMR change -> always include
            let result = decide_change(1_000_000_000_000, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }
    }
}

pub use native::*;
