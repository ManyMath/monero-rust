//! Transaction building.

pub mod native {
    use std::collections::HashSet;

    use crate::fee_estimate::{fee_for_weight, transaction_weight_upper_bound};
    use crate::monero_rpc::{FeePriority, Rpc, RpcConnection, DEFAULT_MAX_FEE_PER_BYTE};
    use crate::seed_compat::Seed;
    use crate::unsigned_tx::{
        check_input_ownership, check_payment_id_recipient, output_with_decoys, ring_len,
        sign_offline, UnsignedChange, UnsignedTransaction, MAX_CHANGE_ACCOUNT,
    };
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

    const MAX_UNSIGNED_TX_HEX_LEN: usize = 8 * 1024 * 1024;

    fn decode_unsigned_tx_hex(unsigned_tx_hex: &str) -> Result<Vec<u8>, String> {
        if unsigned_tx_hex.len() > MAX_UNSIGNED_TX_HEX_LEN {
            return Err(format!(
                "Unsigned transaction exceeds {MAX_UNSIGNED_TX_HEX_LEN} hex characters"
            ));
        }
        hex::decode(unsigned_tx_hex).map_err(|e| format!("Invalid unsigned tx hex: {e:?}"))
    }

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

    fn spendable_from_stored_data(
        stored: &StoredOutputData,
    ) -> Result<(WalletOutput, u64), String> {
        let output = parse_stored_output(&stored.received_output_bytes)?;
        let global_index = stored.global_index.ok_or_else(|| {
            "Output has no authoritative global index; refresh/rescan it before spending"
                .to_string()
        })?;
        Ok((output, global_index))
    }

    fn persisted_rings(stored_outputs: &[StoredOutputData]) -> Result<Vec<Option<Decoys>>, String> {
        stored_outputs
            .iter()
            .map(|stored| {
                stored
                    .ring
                    .as_ref()
                    .map(DecoySelection::to_decoys)
                    .transpose()
            })
            .collect()
    }

    fn source_account(stored_outputs: &[StoredOutputData]) -> Result<u32, String> {
        let account = stored_outputs
            .first()
            .and_then(|stored| stored.subaddress)
            .map_or(0, |(account, _)| account);
        if account > MAX_CHANGE_ACCOUNT {
            return Err(format!(
                "Source account {account} exceeds signer lookahead {MAX_CHANGE_ACCOUNT}"
            ));
        }
        if stored_outputs.iter().any(|stored| {
            stored
                .subaddress
                .map_or(0, |(stored_account, _)| stored_account)
                != account
        }) {
            return Err("Cannot combine outputs from different accounts".to_string());
        }
        Ok(account)
    }

    fn wallet2_change(
        view_pair: &ViewPair,
        network: Network,
        source_account: u32,
    ) -> Result<UnsignedChange, String> {
        if source_account == 0 {
            return Ok(UnsignedChange::Own {
                address: view_pair.legacy_address(network),
                subaddress: None,
            });
        }
        let index = SubaddressIndex::new(source_account, 0)
            .ok_or_else(|| format!("Invalid source account {source_account}"))?;
        Ok(UnsignedChange::Own {
            address: view_pair.subaddress(network, index),
            subaddress: Some(index),
        })
    }

    fn output_source_account(outputs: &[(WalletOutput, u64)]) -> Result<u32, String> {
        let account = outputs
            .first()
            .and_then(|(output, _)| output.subaddress())
            .map_or(0, |index| index.account());
        if outputs
            .iter()
            .any(|(output, _)| output.subaddress().map_or(0, |index| index.account()) != account)
        {
            return Err("Cannot combine outputs from different accounts".to_string());
        }
        Ok(account)
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
        rct_type: RctType,
        height: usize,
        outputs: &[(WalletOutput, u64)],
        existing_rings: &[Option<Decoys>],
        blackballed: &HashSet<u64>,
    ) -> Result<Vec<Decoys>, String> {
        let ring_len = ring_len(rct_type).ok_or_else(|| "Unsupported RingCT type".to_string())?;
        let mut spent = Vec::with_capacity(outputs.len());
        for (output, global_index) in outputs {
            spent.push(spent_output(output, *global_index));
        }
        crate::decoy_select::select_decoys_for_outputs(
            rng,
            rpc,
            ring_len,
            height,
            &spent,
            existing_rings,
            blackballed,
        )
        .await
        .map_err(|e| format!("Decoy selection failed: {}", e))
    }

    /// Select decoys for every output and assemble the unsigned transaction.
    pub async fn build_unsigned_transaction<G: RngCore + CryptoRng, R: RpcConnection>(
        rng: &mut G,
        rpc: &Rpc<R>,
        rct_type: RctType,
        fee_rate: FeeRate,
        outputs: &[(WalletOutput, u64)],
        payments: Vec<(MoneroAddress, u64)>,
        change: Option<UnsignedChange>,
    ) -> Result<UnsignedTransaction, String> {
        build_unsigned_transaction_with_blackballs(
            rng,
            rpc,
            rct_type,
            fee_rate,
            outputs,
            payments,
            change,
            &HashSet::new(),
        )
        .await
    }

    /// `build_unsigned_transaction`, using blackballed outputs as decoys only
    /// when a full ring needs them.
    pub async fn build_unsigned_transaction_with_blackballs<
        G: RngCore + CryptoRng,
        R: RpcConnection,
    >(
        rng: &mut G,
        rpc: &Rpc<R>,
        rct_type: RctType,
        fee_rate: FeeRate,
        outputs: &[(WalletOutput, u64)],
        payments: Vec<(MoneroAddress, u64)>,
        change: Option<UnsignedChange>,
        blackballed: &HashSet<u64>,
    ) -> Result<UnsignedTransaction, String> {
        let existing_rings = vec![None; outputs.len()];
        let source_account = output_source_account(outputs)?;
        build_unsigned_transaction_with_rings(
            rng,
            rpc,
            rct_type,
            fee_rate,
            outputs,
            &existing_rings,
            source_account,
            payments,
            change,
            blackballed,
        )
        .await
    }

    async fn build_unsigned_transaction_with_rings<G: RngCore + CryptoRng, R: RpcConnection>(
        rng: &mut G,
        rpc: &Rpc<R>,
        rct_type: RctType,
        fee_rate: FeeRate,
        outputs: &[(WalletOutput, u64)],
        existing_rings: &[Option<Decoys>],
        source_account: u32,
        payments: Vec<(MoneroAddress, u64)>,
        change: Option<UnsignedChange>,
        blackballed: &HashSet<u64>,
    ) -> Result<UnsignedTransaction, String> {
        if output_source_account(outputs)? != source_account {
            return Err("Stored source account does not match the selected outputs".to_string());
        }
        let height = rpc
            .get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;

        let decoys = select_output_decoys(
            rng,
            rpc,
            rct_type,
            height.saturating_sub(1),
            outputs,
            existing_rings,
            blackballed,
        )
        .await?;
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
            rct_type,
            seed,
            fee_rate,
            source_account,
            inputs,
            payments,
            change,
        })
    }

    fn check_payment_id_destinations(
        payments: &[(MoneroAddress, u64)],
        change: Option<&UnsignedChange>,
    ) -> Result<(), String> {
        let change = match change {
            Some(UnsignedChange::Own { address, .. }) => Some(address),
            Some(UnsignedChange::Dummy) | None => None,
        };
        check_payment_id_recipient(payments, change)
    }

    /// Refuse a zero amount, or inputs short of the payments plus wallet2's fee
    /// estimate, before decoys are requested (wallet2.cpp:11764-11776).
    fn check_funds(
        rct_type: RctType,
        fee_rate: FeeRate,
        total_input: u64,
        input_count: usize,
        payments: &[(MoneroAddress, u64)],
    ) -> Result<(), String> {
        if payments.iter().any(|(_, amount)| *amount == 0) {
            return Err("Destination amount is zero".to_string());
        }
        let total_send = crate::tx_utils::checked_amount_sum(payments.iter().map(|p| p.1))
            .ok_or_else(|| "requested amounts overflow".to_string())?;
        // Tag, length, and the encrypted ID with its type byte.
        let extra = if payments
            .iter()
            .any(|(address, _)| address.payment_id().is_some())
        {
            1 + 1 + 1 + 8
        } else {
            0
        };
        let weight = crate::fee_estimate::wallet2_weight_estimate(
            rct_type,
            input_count,
            payments.len() + 1,
        )?
        .checked_add(extra)
        .ok_or_else(|| "weight exceeds usize".to_string())?;
        let fee = fee_for_weight(fee_rate, weight)?;
        let needed = total_send
            .checked_add(fee)
            .ok_or_else(|| "requested amount plus fee overflows".to_string())?;
        if total_input < needed {
            return Err(format!(
                "Insufficient funds: have {} piconero, need at least {} (amount {} + estimated \
                 fee {})",
                total_input, needed, total_send, fee
            ));
        }
        Ok(())
    }

    /// Settle a zero change as wallet2 does: a zero dummy output for one
    /// destination, none for several, at the same fee (wallet2.cpp:10444-10466).
    pub fn settle_change(
        unsigned: &mut UnsignedTransaction,
        view_pair: &ViewPair,
    ) -> Result<(), String> {
        let with_change = unsigned.resolve(view_pair)?;
        match with_change.change {
            Some(change) if change.amount == 0 => {}
            _ => return Ok(()),
        }
        unsigned.change = (unsigned.payments.len() == 1).then_some(UnsignedChange::Dummy);

        let settled = unsigned.resolve(view_pair)?;
        if settled.fee != with_change.fee
            || settled.change.is_some()
            || settled.dummy_amount.is_some_and(|amount| amount != 0)
        {
            return Err(format!(
                "the fee moved from {} to {} without a change output, leaving {:?} for the dummy \
                 output",
                with_change.fee, settled.fee, settled.dummy_amount
            ));
        }
        Ok(())
    }

    fn pay_everything(
        unsigned: &mut UnsignedTransaction,
        view_pair: &ViewPair,
    ) -> Result<u64, String> {
        if unsigned.payments.len() != 1 || unsigned.change != Some(UnsignedChange::Dummy) {
            return Err("only one payment with a dummy output can take everything".to_string());
        }
        let provisional = unsigned.resolve(view_pair)?;
        let amount = provisional
            .input_total
            .checked_sub(provisional.fee)
            .ok_or_else(|| "inputs cannot pay the fee".to_string())?;
        unsigned.payments[0].1 = amount;

        let resolved = unsigned.resolve(view_pair)?;
        if resolved.fee != provisional.fee
            || resolved.dummy_amount != Some(0)
            || resolved.change.is_some()
        {
            return Err(format!(
                "the fee moved from {} to {} when the amount was set, leaving {:?} for the dummy \
                 output",
                provisional.fee, resolved.fee, resolved.dummy_amount
            ));
        }
        Ok(amount)
    }

    fn fee_rate_of(fee: crate::monero_rpc::Fee) -> Result<FeeRate, String> {
        fee.rate()
            .ok_or_else(|| "Fee rate and quantization mask must be nonzero".to_string())
    }

    /// Fee selection policy shared by estimation and transaction construction.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FeePolicy {
        pub priority: FeePriority,
        /// wallet2's `m_blockchain.size()`, the newest scanned height plus one.
        /// `None` uses the node's height.
        pub wallet_height: Option<u64>,
        pub max_fee_per_weight: u64,
    }

    impl Default for FeePolicy {
        fn default() -> Self {
            Self {
                priority: FeePriority::Automatic,
                wallet_height: None,
                max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
            }
        }
    }

    impl FeePolicy {
        /// The policy for a wallet whose newest scanned block is `last_scanned_block`.
        pub fn for_scanned_wallet(priority: FeePriority, last_scanned_block: u64) -> Self {
            Self {
                priority,
                wallet_height: Some(last_scanned_block.saturating_add(1)),
                ..Self::default()
            }
        }

        /// Settle Automatic priority once, as wallet2's `adjust_priority`; pass the
        /// result to every step of a send.
        pub async fn resolve<R: RpcConnection>(self, rpc: &Rpc<R>) -> Result<Self, String> {
            let priority = rpc
                .adjust_priority(self.priority, self.wallet_height)
                .await
                .map_err(|e| format!("Failed to get fee rate: {e:?}"))?;
            Ok(Self { priority, ..self })
        }
    }

    /// Blackballed RingCT global output indexes from a trusted local list,
    /// never from the node.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct DecoyPolicy {
        pub blackballed_outputs: HashSet<u64>,
    }

    async fn fee_for_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        policy: FeePolicy,
    ) -> Result<(crate::monero_rpc::Fee, FeeRate), String> {
        let fee = rpc
            .get_fee_checked_for_priority_at_height(
                policy.priority,
                policy.wallet_height,
                policy.max_fee_per_weight,
            )
            .await
            .map_err(|e| format!("Failed to get fee rate: {e:?}"))?;
        Ok((fee, fee_rate_of(fee)?))
    }

    fn fee_upper_bound(
        rct_type: RctType,
        fee_rate: FeeRate,
        inputs: usize,
        outputs: usize,
    ) -> Result<(usize, u64), String> {
        let weight = transaction_weight_upper_bound(rct_type, inputs, outputs, true)?;
        Ok((weight, fee_for_weight(fee_rate, weight)?))
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

    #[must_use = "persist input_rings in WalletState before a transaction may be rebuilt"]
    #[derive(Clone, Serialize, Deserialize)]
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
        /// The ring of each input, by key image.
        #[serde(default)]
        pub input_rings: Vec<InputRing>,
    }

    const REDACTED: &str = "<redacted>";

    impl std::fmt::Debug for TransactionResult {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TransactionResult")
                .field("tx_id", &self.tx_id)
                .field("fee", &self.fee)
                .field("tx_blob", &self.tx_blob)
                .field("tx_key", &REDACTED)
                .field("tx_key_additional", &self.tx_key_additional.len())
                .field("change_outputs", &self.change_outputs)
                .field("input_rings", &self.input_rings)
                .finish()
        }
    }

    impl TransactionResult {
        /// Record every ring in `wallet_state`, all or none.
        pub fn persist_input_rings(
            &self,
            wallet_state: &mut crate::wallet_state::WalletState,
        ) -> Result<(), String> {
            wallet_state.record_input_rings(&self.input_rings)
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StoredOutputData {
        pub tx_hash: String,
        pub output_index: u64,
        pub amount: u64,
        pub key: String,
        pub key_offset: String,
        pub commitment_mask: String,
        pub subaddress: Option<(u32, u32)>,
        pub payment_id: Option<String>,
        pub received_output_bytes: String,
        /// Global RingCT index learned during refresh.
        #[serde(default)]
        pub global_index: Option<u64>,
        /// Key image, which keys the wallet's ring database.
        #[serde(default)]
        pub key_image: String,
        /// The ring a build reuses for this output.
        #[serde(default)]
        pub ring: Option<DecoySelection>,
    }

    /// Ring member data (key + commitment as hex)
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct RingMember {
        pub key: String,
        pub commitment: String,
    }

    /// Decoy selection result for a single input
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DecoySelection {
        pub real_index: u8,
        pub offsets: Vec<u64>,
        pub ring: Vec<RingMember>,
    }

    /// A selected ring keyed the same way as wallet2's ring database.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct InputRing {
        pub key_image: String,
        pub ring: DecoySelection,
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

        fn to_decoys(&self) -> Result<Decoys, String> {
            let ring = self
                .ring
                .iter()
                .map(|member| {
                    let point = |encoded: &str| {
                        let bytes: [u8; 32] = hex::decode(encoded)
                            .map_err(|_| "persisted ring member is not hex".to_string())?
                            .try_into()
                            .map_err(|_| {
                                "persisted ring member is not a 32-byte point".to_string()
                            })?;
                        monero_wallet::ed25519::CompressedPoint::from(bytes)
                            .decompress()
                            .ok_or_else(|| "persisted ring member is not a point".to_string())
                    };
                    Ok([point(&member.key)?, point(&member.commitment)?])
                })
                .collect::<Result<Vec<_>, String>>()?;
            Decoys::new(self.offsets.clone(), self.real_index, ring)
                .ok_or_else(|| "persisted ring is syntactically invalid".to_string())
        }
    }

    fn selected_input_rings(
        stored_outputs: &[StoredOutputData],
        unsigned: &UnsignedTransaction,
    ) -> Result<Vec<InputRing>, String> {
        if stored_outputs.len() != unsigned.inputs.len() {
            return Err("selected ring count does not match stored input count".to_string());
        }
        if stored_outputs
            .iter()
            .any(|stored| stored.key_image.is_empty())
        {
            return Err(
                "Cannot build a transaction until every selected output has a key image"
                    .to_string(),
            );
        }
        Ok(stored_outputs
            .iter()
            .zip(&unsigned.inputs)
            .map(|(stored, input)| InputRing {
                key_image: stored.key_image.clone(),
                ring: DecoySelection::from_decoys(input.decoys()),
            })
            .collect())
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

    /// Validated inputs and recipients with pre-decoy fee and change bounds.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PreparedTransaction {
        pub node_url: String,
        pub network: String,
        pub recipients: Vec<(String, u64)>, // (address, amount) pairs
        #[serde(alias = "fee")]
        pub fee_upper_bound: u64,
        pub total_input: u64,
        #[serde(alias = "change")]
        pub change_lower_bound: u64,
        pub stored_outputs: Vec<StoredOutputData>,
    }

    fn scan_transaction_outputs(
        tx: &Transaction,
        tx_id: &str,
        view_pair: ViewPair,
        spend_key: Zeroizing<Scalar>,
        lookahead: Lookahead,
        change_subaddress: Option<SubaddressIndex>,
    ) -> Result<Vec<ChangeOutputInfo>, String> {
        let mut scanner = monero_wallet::Scanner::new(view_pair);
        crate::scanner::register_subaddresses(&mut scanner, lookahead);
        // Change may go to a subaddress outside the lookahead.
        if let Some(index) = change_subaddress {
            scanner.register_subaddress(index);
        }

        let tx_hash: [u8; 32] = hex::decode(tx_id)
            .map_err(|e| format!("Signed transaction ID is not hex: {e}"))?
            .try_into()
            .map_err(|_| "Signed transaction ID is not 32 bytes".to_string())?;
        let pruned = Transaction::<Pruned>::from(tx.clone());
        let our_outputs = crate::scanner::scan_single_transaction(&mut scanner, tx_hash, &pruned)?;

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

                Ok(ChangeOutputInfo {
                    tx_hash: tx_id.to_string(),
                    output_index: u8::try_from(output.index_in_transaction())
                        .map_err(|_| "Signed transaction output index exceeds u8".to_string())?,
                    amount,
                    key,
                    key_offset,
                    commitment_mask,
                    subaddress_index,
                    received_output_bytes,
                    key_image,
                })
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
    ) -> Result<(Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>), String> {
        let seed = resolve_seed(seed_phrase)?;
        let spend_secret_key = Zeroizing::new(spend_key_from_seed(&seed, passphrase).to_bytes());
        let view_secret_key = view_key_from_spend_key(&Zeroizing::new(
            Scalar::from_bytes_mod_order(*spend_secret_key),
        ));
        Ok((spend_secret_key, Zeroizing::new(view_secret_key.to_bytes())))
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

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn fetch_decoys_with_policy(
        node_url: &str,
        stored_outputs: Vec<StoredOutputData>,
        policy: &DecoyPolicy,
    ) -> Result<DecoyResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        fetch_decoys_with_rpc_and_policy(&rpc, stored_outputs, policy).await
    }

    pub async fn fetch_decoys_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        stored_outputs: Vec<StoredOutputData>,
    ) -> Result<DecoyResult, String> {
        fetch_decoys_with_rpc_and_policy(rpc, stored_outputs, &DecoyPolicy::default()).await
    }

    pub async fn fetch_decoys_with_rpc_and_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        stored_outputs: Vec<StoredOutputData>,
        policy: &DecoyPolicy,
    ) -> Result<DecoyResult, String> {
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }

        let rct_type = rpc
            .get_rct_type()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let height = rpc
            .get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;

        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let spendable = spendable_from_stored_data(stored)?;
            spendable_outputs.push(spendable);
        }

        let decoys = select_output_decoys(
            &mut rand::rngs::OsRng,
            rpc,
            rct_type,
            height.saturating_sub(1),
            &spendable_outputs,
            &persisted_rings(&stored_outputs)?,
            &policy.blackballed_outputs,
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
        estimate_fee_with_policy(node_url, num_inputs, num_outputs, FeePolicy::default()).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn estimate_fee_with_policy(
        node_url: &str,
        num_inputs: usize,
        num_outputs: usize,
        policy: FeePolicy,
    ) -> Result<FeeEstimate, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        estimate_fee_with_rpc_and_policy(&rpc, num_inputs, num_outputs, policy).await
    }

    pub async fn estimate_fee_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<FeeEstimate, String> {
        estimate_fee_with_rpc_and_policy(rpc, num_inputs, num_outputs, FeePolicy::default()).await
    }

    pub async fn estimate_fee_with_rpc_and_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        num_inputs: usize,
        num_outputs: usize,
        policy: FeePolicy,
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
        let rct_type = rpc
            .get_rct_type()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let (node_fee, fee_rate) = fee_for_policy(rpc, policy).await?;

        // Worst-case extra: assume payment ID and additional keys
        let (weight, fee) = fee_upper_bound(rct_type, fee_rate, num_inputs, num_outputs)?;

        Ok(FeeEstimate {
            fee,
            weight,
            per_weight: node_fee.per_weight,
            mask: node_fee.mask,
            inputs: num_inputs,
            outputs: num_outputs,
        })
    }

    /// Check recipients and balance against a fee upper bound, before decoy
    /// selection.
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
        source_account(&stored_outputs)?;

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
            fee_upper_bound: fee_est.fee,
            total_input,
            change_lower_bound: total_input - total_out,
            stored_outputs,
        })
    }

    /// Sign a prepared transaction, rebuilding it with decoys and its exact fee.
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
        /// Include a change output.
        IncludeChange,
        /// Absorb small change into miner fee (multi-recipient only).
        AbsorbDust,
        /// Change is dust and only 1 recipient; caller should use sweep_all.
        DustError,
    }

    /// Always `IncludeChange`: wallet2 keeps every nonzero RingCT change.
    pub fn decide_change(_expected_change: u64, _num_recipients: usize) -> ChangeDecision {
        ChangeDecision::IncludeChange
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
        create_transaction_with_policy(
            node_url,
            seed_phrase,
            network_str,
            stored_outputs,
            recipients,
            passphrase,
            FeePolicy::default(),
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_transaction_with_policy(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        passphrase: &str,
        policy: FeePolicy,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        create_transaction_with_rpc_and_policy(
            &rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            recipients,
            passphrase,
            policy,
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
        create_transaction_with_rpc_and_policy(
            rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            recipients,
            passphrase,
            FeePolicy::default(),
        )
        .await
    }

    pub async fn create_transaction_with_rpc_and_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        passphrase: &str,
        policy: FeePolicy,
    ) -> Result<TransactionResult, String> {
        create_transaction_with_rpc_and_policies(
            rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            recipients,
            passphrase,
            policy,
            &DecoyPolicy::default(),
        )
        .await
    }

    pub async fn create_transaction_with_rpc_and_policies<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        passphrase: &str,
        policy: FeePolicy,
        decoy_policy: &DecoyPolicy,
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
        crate::fee_estimate::check_one_transaction(
            stored_outputs.len(),
            recipients.len() + 1,
            false,
        )?;

        let source_account = source_account(&stored_outputs)?;

        let network = parse_network(network_str)?;

        let seed = resolve_seed(seed_phrase)?;

        let spend_key = spend_key_from_seed(&seed, passphrase);
        let view_key = view_key_from_spend_key(&spend_key);
        let view_pair = signer_view_pair(&spend_key, &view_key)?;

        let rct_type = rpc
            .get_rct_type()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let (_, fee) = fee_for_policy(rpc, policy).await?;

        // Parse and validate all destination addresses
        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination address '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        let mut spendable_outputs = Vec::new();

        for stored in &stored_outputs {
            let spendable = spendable_from_stored_data(stored)?;
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
        let (_, fee_wc) =
            fee_upper_bound(rct_type, fee, spendable_outputs.len(), num_out_with_change)?;
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
            rct_type,
            fee,
            total_input,
            spendable_outputs.len(),
            &payments,
        )?;

        let change = with_change
            .then(|| wallet2_change(&view_pair, network, source_account))
            .transpose()?;
        check_payment_id_destinations(&payments, change.as_ref())?;

        // Select decoys online, then sign; the signer returns the tx keys
        let mut rng = rand::rngs::OsRng;
        let mut unsigned = build_unsigned_transaction_with_rings(
            &mut rng,
            rpc,
            rct_type,
            fee,
            &spendable_outputs,
            &persisted_rings(&stored_outputs)?,
            source_account,
            payments,
            change,
            &decoy_policy.blackballed_outputs,
        )
        .await
        .map_err(|e| format!("Failed to prepare transaction: {}", e))?;
        settle_change(&mut unsigned, &view_pair)?;
        let input_rings = selected_input_rings(&stored_outputs, &unsigned)?;
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
        let change_outputs = scan_transaction_outputs(
            &tx,
            &tx_id,
            view_pair,
            spend_key,
            lookahead,
            unsigned.change.and_then(|change| change.subaddress()),
        )?;

        Ok(TransactionResult {
            tx_id,
            fee: fee_amount,
            tx_blob,
            tx_key,
            tx_key_additional,
            change_outputs,
            input_rings,
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
        sweep_all_with_policy(
            node_url,
            seed_phrase,
            network_str,
            stored_outputs,
            destination_address,
            passphrase,
            FeePolicy::default(),
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn sweep_all_with_policy(
        node_url: &str,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
        passphrase: &str,
        policy: FeePolicy,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        sweep_all_with_rpc_and_policy(
            &rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            destination_address,
            passphrase,
            policy,
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
        sweep_all_with_rpc_and_policy(
            rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            destination_address,
            passphrase,
            FeePolicy::default(),
        )
        .await
    }

    pub async fn sweep_all_with_rpc_and_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
        passphrase: &str,
        policy: FeePolicy,
    ) -> Result<TransactionResult, String> {
        sweep_all_with_rpc_and_policies(
            rpc,
            seed_phrase,
            network_str,
            stored_outputs,
            destination_address,
            passphrase,
            policy,
            &DecoyPolicy::default(),
        )
        .await
    }

    pub async fn sweep_all_with_rpc_and_policies<R: RpcConnection>(
        rpc: &Rpc<R>,
        seed_phrase: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        destination_address: &str,
        passphrase: &str,
        policy: FeePolicy,
        decoy_policy: &DecoyPolicy,
    ) -> Result<TransactionResult, String> {
        crate::validation::validate_network(network_str).map_err(|e| e.to_string())?;
        if stored_outputs.is_empty() {
            return Err("No outputs provided".to_string());
        }
        crate::fee_estimate::check_one_transaction(stored_outputs.len(), 2, true)?;

        let source_account = source_account(&stored_outputs)?;

        let network = parse_network(network_str)?;

        let seed = resolve_seed(seed_phrase)?;

        let spend_key = spend_key_from_seed(&seed, passphrase);
        let view_key = view_key_from_spend_key(&spend_key);
        let view_pair = signer_view_pair(&spend_key, &view_key)?;

        let rct_type = rpc
            .get_rct_type()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;

        let (_, fee) = fee_for_policy(rpc, policy).await?;

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
            let spendable = spendable_from_stored_data(stored)?;
            spendable_outputs.push(spendable);
        }

        // Calculate total input amount
        let total_in = crate::tx_utils::checked_amount_sum(
            spendable_outputs
                .iter()
                .map(|(output, _)| output.commitment().amount),
        )
        .ok_or_else(|| "input amounts overflow".to_string())?;

        let num_outputs = 2;
        let (_, fee_bound) = fee_upper_bound(rct_type, fee, spendable_outputs.len(), num_outputs)?;
        let provisional_amount = total_in
            .checked_sub(fee_bound)
            .filter(|amount| *amount > 0)
            .ok_or_else(|| {
                format!(
                    "Insufficient funds: have {} atomic units, need more than {} for fee",
                    total_in, fee_bound
                )
            })?;

        let mut rng = rand::rngs::OsRng;
        let mut unsigned = build_unsigned_transaction_with_rings(
            &mut rng,
            rpc,
            rct_type,
            fee,
            &spendable_outputs,
            &persisted_rings(&stored_outputs)?,
            source_account,
            vec![(dest_addr, provisional_amount)],
            Some(UnsignedChange::Dummy),
            &decoy_policy.blackballed_outputs,
        )
        .await
        .map_err(|e| format!("Failed to prepare sweep transaction: {}", e))?;
        pay_everything(&mut unsigned, &view_pair)
            .map_err(|e| format!("Failed to prepare sweep transaction: {}", e))?;

        let input_rings = selected_input_rings(&stored_outputs, &unsigned)?;
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
            scan_transaction_outputs(&tx, &tx_id, view_pair, spend_key, lookahead, None)?;

        Ok(TransactionResult {
            tx_id,
            fee: actual_fee,
            tx_blob,
            tx_key,
            tx_key_additional,
            change_outputs,
            input_rings,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn broadcast_transaction(
        node_url: &str,
        transaction: &TransactionResult,
        wallet_state: &mut crate::wallet_state::WalletState,
        do_not_relay: bool,
    ) -> Result<(), String> {
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        broadcast_transaction_with_rpc(&rpc, transaction, wallet_state, do_not_relay).await
    }

    /// Persist the rings selected for `transaction`, then relay it.
    pub async fn broadcast_transaction_with_rpc<R: RpcConnection>(
        rpc: &Rpc<R>,
        transaction: &TransactionResult,
        wallet_state: &mut crate::wallet_state::WalletState,
        do_not_relay: bool,
    ) -> Result<(), String> {
        transaction.persist_input_rings(wallet_state)?;
        broadcast_raw_transaction_with_rpc(rpc, &transaction.tx_blob, do_not_relay).await
    }

    /// Relay signed transaction bytes whose rings were managed elsewhere.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn broadcast_raw_transaction(
        node_url: &str,
        tx_blob_hex: &str,
        do_not_relay: bool,
    ) -> Result<(), String> {
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        broadcast_raw_transaction_with_rpc(&rpc, tx_blob_hex, do_not_relay).await
    }

    pub async fn broadcast_raw_transaction_with_rpc<R: RpcConnection>(
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

    #[must_use = "persist input_rings in WalletState before an unsigned transaction may be rebuilt"]
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UnsignedTransactionResult {
        pub unsigned_tx_hex: String,
        pub fee: u64,
        pub recipients: Vec<(String, u64)>,
        /// The ring of each input, by key image.
        #[serde(default)]
        pub input_rings: Vec<InputRing>,
    }

    impl UnsignedTransactionResult {
        /// Record every ring in `wallet_state`, all or none.
        pub fn persist_input_rings(
            &self,
            wallet_state: &mut crate::wallet_state::WalletState,
        ) -> Result<(), String> {
            wallet_state.record_input_rings(&self.input_rings)
        }
    }

    fn persist_unsigned_transaction_result(
        result: UnsignedTransactionResult,
        wallet_state: &mut crate::wallet_state::WalletState,
    ) -> Result<UnsignedTransactionResult, String> {
        result.persist_input_rings(wallet_state)?;
        Ok(result)
    }

    #[derive(Clone, Serialize, Deserialize)]
    pub struct OfflineSignedTransaction {
        pub tx_id: String,
        pub fee: u64,
        pub tx_blob: String,
        pub tx_key: String,
        pub tx_key_additional: Vec<String>,
        pub spent_key_images: Vec<String>,
        pub change_outputs: Vec<ChangeOutputInfo>,
    }

    impl std::fmt::Debug for OfflineSignedTransaction {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("OfflineSignedTransaction")
                .field("tx_id", &self.tx_id)
                .field("fee", &self.fee)
                .field("tx_blob", &self.tx_blob)
                .field("tx_key", &REDACTED)
                .field("tx_key_additional", &self.tx_key_additional.len())
                .field("spent_key_images", &self.spent_key_images)
                .field("change_outputs", &self.change_outputs)
                .finish()
        }
    }

    /// What signing an unsigned transaction or a wallet2 unsigned txset gives.
    /// Every one of `transactions` has to be relayed.
    #[derive(Clone, Serialize, Deserialize)]
    pub struct OfflineSignResult {
        /// ID of the first transaction.
        pub tx_id: String,
        /// Total fee of all transactions.
        pub fee: u64,
        /// The first transaction.
        pub tx_blob: String,
        /// Private key of the first transaction.
        pub tx_key: String,
        /// Additional private keys of the first transaction.
        pub tx_key_additional: Vec<String>,
        /// Key images spent by all transactions.
        pub spent_key_images: Vec<String>,
        /// Outputs of all transactions that return to the wallet.
        pub change_outputs: Vec<ChangeOutputInfo>,
        /// Every signed transaction, in the order of the unsigned set.
        pub transactions: Vec<OfflineSignedTransaction>,
    }

    impl std::fmt::Debug for OfflineSignResult {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("OfflineSignResult")
                .field("tx_id", &self.tx_id)
                .field("fee", &self.fee)
                .field("tx_blob", &self.tx_blob)
                .field("tx_key", &REDACTED)
                .field("tx_key_additional", &self.tx_key_additional.len())
                .field("spent_key_images", &self.spent_key_images)
                .field("change_outputs", &self.change_outputs)
                .field("transactions", &self.transactions)
                .finish()
        }
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
        let view_scalar = Zeroizing::new(Scalar::from_bytes_mod_order(view_secret));
        let tx_public_point = CompressedEdwardsY(tx_public_key)
            .decompress()
            .ok_or_else(|| "Invalid source tx public key point".to_string())?;
        // Room for the varint, so appending does not reallocate and leave a copy of the secret.
        let derivation = Zeroizing::new((*view_scalar * tx_public_point).mul_by_cofactor());
        let compressed = Zeroizing::new(derivation.compress());
        let mut bytes = Zeroizing::new(Vec::with_capacity(32 + 10));
        bytes.extend_from_slice(compressed.as_bytes());
        write_varint_u64(output_index, &mut bytes);
        Ok(hash_to_scalar(&bytes))
    }

    fn subaddress_key_offset(view_secret: [u8; 32], account: u32, minor: u32) -> Scalar {
        if account == 0 && minor == 0 {
            return Scalar::ZERO;
        }

        let mut bytes = Zeroizing::new(Vec::with_capacity(8 + 32 + 4 + 4));
        bytes.extend_from_slice(b"SubAddr\0");
        bytes.extend_from_slice(&view_secret);
        bytes.extend_from_slice(&account.to_le_bytes());
        bytes.extend_from_slice(&minor.to_le_bytes());
        hash_to_scalar(&bytes)
    }

    struct SourceSubaddresses {
        account: u32,
        by_spend_key: std::collections::HashMap<[u8; 32], (u32, Scalar)>,
    }

    impl SourceSubaddresses {
        fn new(
            spend_secret: &Scalar,
            view_secret: [u8; 32],
            account: u32,
            subaddr_indices: &[u32],
        ) -> Self {
            let spend_public = spend_secret * ED25519_BASEPOINT_TABLE;
            let mut by_spend_key = std::collections::HashMap::new();
            let mut seen = HashSet::new();
            for minor in std::iter::once(0).chain(subaddr_indices.iter().copied()) {
                if !seen.insert(minor) {
                    continue;
                }
                let offset = subaddress_key_offset(view_secret, account, minor);
                let key = (spend_public + &offset * ED25519_BASEPOINT_TABLE)
                    .compress()
                    .to_bytes();
                by_spend_key.insert(key, (minor, offset));
            }
            Self {
                account,
                by_spend_key,
            }
        }
    }

    fn derive_source_key_offset(
        source: &crate::epee_compat::TxSourceEntrySummary,
        spend_secret: &Scalar,
        view_secret: [u8; 32],
        subaddresses: &SourceSubaddresses,
    ) -> Result<(Scalar, [u8; 32], Option<(u32, u32)>), String> {
        let real_output_index: usize = source
            .real_output
            .try_into()
            .map_err(|_| "Wallet2 source real output index exceeds usize".to_string())?;
        let real = source
            .ring
            .get(real_output_index)
            .ok_or_else(|| "Wallet2 source real output index is out of range".to_string())?;
        let output_public_key = CompressedEdwardsY(real.output_public_key)
            .decompress()
            .ok_or_else(|| "Invalid wallet2 source output public key".to_string())?;

        let additional = usize::try_from(source.real_output_in_tx_index)
            .ok()
            .and_then(|index| source.real_out_additional_tx_keys.get(index));
        for tx_public_key in std::iter::once(&source.real_out_tx_key).chain(additional) {
            let Ok(base_offset) =
                output_key_offset(view_secret, *tx_public_key, source.real_output_in_tx_index)
            else {
                continue;
            };
            let subaddress_spend_key = (output_public_key - &base_offset * ED25519_BASEPOINT_TABLE)
                .compress()
                .to_bytes();
            let Some((minor, subaddress_offset)) =
                subaddresses.by_spend_key.get(&subaddress_spend_key)
            else {
                continue;
            };
            let key_offset = base_offset + subaddress_offset;
            let output_secret = Zeroizing::new(spend_secret + key_offset);
            if &*output_secret * ED25519_BASEPOINT_TABLE != output_public_key {
                continue;
            }
            return Ok((
                key_offset,
                *tx_public_key,
                Some((subaddresses.account, *minor)),
            ));
        }

        Err("Wallet2 source does not belong to the supplied seed".to_string())
    }

    pub fn derive_wallet2_unsigned_source_key_offsets_from_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
    ) -> Result<Wallet2SourceKeyOffsetsResult, String> {
        let spend_secret = Zeroizing::new(Scalar::from_bytes_mod_order(spend_secret_key));
        let summary = crate::epee_compat::parse_unsigned_monero_txset_summary(
            unsigned_txset,
            &view_secret_key,
        )?;
        let mut source_key_offsets = Vec::new();
        for (construction_index, construction) in summary.txes.iter().enumerate() {
            let subaddresses = SourceSubaddresses::new(
                &spend_secret,
                view_secret_key,
                construction.subaddr_account,
                &construction.subaddr_indices,
            );
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
                    &spend_secret,
                    view_secret_key,
                    &subaddresses,
                )?;
                let output_secret = Zeroizing::new(*spend_secret + key_offset);
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
            *spend_secret_key,
            *view_secret_key,
            unsigned_txset,
        )
    }

    fn parse_wallet2_destination_address(
        destination: &crate::epee_compat::TxDestinationEntrySummary,
        network: Network,
    ) -> Result<MoneroAddress, String> {
        if let Some(address) = crate::txset_checks::original_as_address(&destination.original)? {
            return Ok(address);
        }
        if destination.is_integrated {
            return Err(
                "Wallet2 integrated destination without a readable address string is unsupported"
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
        spend_secret: &Zeroizing<Scalar>,
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
        tx_count: usize,
        construction_index: usize,
        construction: &crate::epee_compat::TxConstructionDataSummary,
        network: Network,
    ) -> Result<UnsignedTransactionResult, String> {
        let subaddresses = SourceSubaddresses::new(
            spend_secret,
            view_secret_key,
            construction.subaddr_account,
            &construction.subaddr_indices,
        );
        let mut inputs = Vec::with_capacity(construction.sources.len());
        let mut input_rings = Vec::with_capacity(construction.sources.len());
        for source in &construction.sources {
            let (key_offset, _, _) =
                derive_source_key_offset(source, spend_secret, view_secret_key, &subaddresses)?;
            let input = wallet2_input(source, key_offset)?;
            let output_secret = Zeroizing::new(**spend_secret + key_offset);
            input_rings.push(InputRing {
                key_image: hex::encode(generate_key_image(&output_secret).compress().to_bytes()),
                ring: DecoySelection::from_decoys(input.decoys()),
            });
            inputs.push(input);
        }

        let mut payments = Vec::with_capacity(construction.destinations.len());
        let mut recipients = Vec::with_capacity(construction.destinations.len());
        for destination in &construction.destinations {
            let address = parse_wallet2_destination_address(destination, network)?;
            recipients.push((address.to_string(), destination.amount));
            payments.push((address, destination.amount));
        }
        let change = if construction.change_amount > 0 {
            Some(UnsignedChange::Own {
                address: parse_wallet2_destination_address(&construction.change, network)?,
                subaddress: SubaddressIndex::new(construction.subaddr_account, 0),
            })
        } else if payments.len() == 1 {
            Some(UnsignedChange::Dummy)
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
            source_account: construction.subaddr_account,
            inputs,
            payments,
            change,
        };

        let view_pair = signer_view_pair(
            spend_secret,
            &Zeroizing::new(Scalar::from_bytes_mod_order(view_secret_key)),
        )?;
        let resolved = unsigned.resolve(&view_pair)?;
        let resolved_change = resolved.change.map_or(0, |change| change.amount);
        let dummy_amount = resolved.dummy_amount.unwrap_or(0);
        if resolved.fee != fee || resolved_change != construction.change_amount || dummy_amount != 0
        {
            return Err(format!(
                "Cannot reproduce the wallet2 transaction exactly: wallet2 has fee {} and change \
                 {}, the rebuilt transaction has fee {}, change {} and {} in a dummy output",
                fee, construction.change_amount, resolved.fee, resolved_change, dummy_amount
            ));
        }

        Ok(UnsignedTransactionResult {
            unsigned_tx_hex: hex::encode(unsigned.serialize()),
            fee,
            recipients,
            input_rings,
        })
    }

    /// Convert every transaction of a wallet2 unsigned txset. Record each
    /// result's `input_rings` before this wallet spends the same outputs.
    pub fn create_unsigned_transactions_from_wallet2_txset_keys(
        spend_secret_key: [u8; 32],
        view_secret_key: [u8; 32],
        unsigned_txset: &[u8],
        network_str: &str,
    ) -> Result<Vec<UnsignedTransactionResult>, String> {
        let network = parse_network(network_str)?;
        let spend_secret = Zeroizing::new(Scalar::from_bytes_mod_order(spend_secret_key));
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
                    &spend_secret,
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

    /// Build an unsigned transaction, persisting its rings before returning it.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_unsigned_transaction(
        node_url: &str,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        max_fee_per_weight: Option<u64>,
        wallet_state: &mut crate::wallet_state::WalletState,
    ) -> Result<UnsignedTransactionResult, String> {
        let policy = FeePolicy {
            max_fee_per_weight: max_fee_per_weight.unwrap_or(DEFAULT_MAX_FEE_PER_BYTE),
            ..FeePolicy::default()
        };
        create_unsigned_transaction_with_policy(
            node_url,
            view_key_hex,
            pub_spend_key_hex,
            network_str,
            stored_outputs,
            recipients,
            policy,
            wallet_state,
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_unsigned_transaction_with_policy(
        node_url: &str,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        policy: FeePolicy,
        wallet_state: &mut crate::wallet_state::WalletState,
    ) -> Result<UnsignedTransactionResult, String> {
        let rpc = HttpRpc::new(node_url.to_string()).map_err(|e| format!("RPC error: {:?}", e))?;
        create_unsigned_transaction_with_rpc_and_policy(
            &rpc,
            view_key_hex,
            pub_spend_key_hex,
            network_str,
            stored_outputs,
            recipients,
            policy,
            wallet_state,
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
        wallet_state: &mut crate::wallet_state::WalletState,
    ) -> Result<UnsignedTransactionResult, String> {
        let policy = FeePolicy {
            max_fee_per_weight: max_fee_per_weight.unwrap_or(DEFAULT_MAX_FEE_PER_BYTE),
            ..FeePolicy::default()
        };
        create_unsigned_transaction_with_rpc_and_policy(
            rpc,
            view_key_hex,
            pub_spend_key_hex,
            network_str,
            stored_outputs,
            recipients,
            policy,
            wallet_state,
        )
        .await
    }

    pub async fn create_unsigned_transaction_with_rpc_and_policy<R: RpcConnection>(
        rpc: &Rpc<R>,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        policy: FeePolicy,
        wallet_state: &mut crate::wallet_state::WalletState,
    ) -> Result<UnsignedTransactionResult, String> {
        create_unsigned_transaction_with_rpc_and_policies(
            rpc,
            view_key_hex,
            pub_spend_key_hex,
            network_str,
            stored_outputs,
            recipients,
            policy,
            &DecoyPolicy::default(),
            wallet_state,
        )
        .await
    }

    pub async fn create_unsigned_transaction_with_rpc_and_policies<R: RpcConnection>(
        rpc: &Rpc<R>,
        view_key_hex: &str,
        pub_spend_key_hex: &str,
        network_str: &str,
        stored_outputs: Vec<StoredOutputData>,
        recipients: &[(String, u64)],
        policy: FeePolicy,
        decoy_policy: &DecoyPolicy,
        wallet_state: &mut crate::wallet_state::WalletState,
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
        crate::fee_estimate::check_one_transaction(
            stored_outputs.len(),
            recipients.len() + 1,
            false,
        )?;

        let source_account = source_account(&stored_outputs)?;

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

        let rct_type = rpc
            .get_rct_type()
            .await
            .map_err(|e| format!("Failed to get protocol: {:?}", e))?;
        let (_, fee_rate) = fee_for_policy(rpc, policy).await?;

        let mut dest_addrs = Vec::with_capacity(recipients.len());
        for (addr_str, _) in recipients {
            let dest_addr = MoneroAddress::from_str(network, addr_str)
                .map_err(|e| format!("Invalid destination '{}': {:?}", addr_str, e))?;
            dest_addrs.push(dest_addr);
        }

        let mut spendable_outputs = Vec::with_capacity(stored_outputs.len());
        for stored in &stored_outputs {
            let spendable = spendable_from_stored_data(stored)?;
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
        let (_, fee_wc) = fee_upper_bound(
            rct_type,
            fee_rate,
            spendable_outputs.len(),
            num_out_with_change,
        )?;
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
            rct_type,
            fee_rate,
            total_input,
            spendable_outputs.len(),
            &payments,
        )?;

        let change = with_change
            .then(|| wallet2_change(&view_pair, network, source_account))
            .transpose()?;
        check_payment_id_destinations(&payments, change.as_ref())?;

        let mut rng = rand::rngs::OsRng;
        let mut unsigned = build_unsigned_transaction_with_rings(
            &mut rng,
            rpc,
            rct_type,
            fee_rate,
            &spendable_outputs,
            &persisted_rings(&stored_outputs)?,
            source_account,
            payments,
            change,
            &decoy_policy.blackballed_outputs,
        )
        .await
        .map_err(|e| format!("Failed to prepare unsigned tx: {}", e))?;
        settle_change(&mut unsigned, &view_pair)?;

        let fee = unsigned.resolve(&view_pair)?.fee;
        let input_rings = selected_input_rings(&stored_outputs, &unsigned)?;

        persist_unsigned_transaction_result(
            UnsignedTransactionResult {
                unsigned_tx_hex: hex::encode(unsigned.serialize()),
                fee,
                recipients: recipients.to_vec(),
                input_rings,
            },
            wallet_state,
        )
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
            *spend_secret_key,
            *view_secret_key,
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
        let unsigned_bytes = decode_unsigned_tx_hex(unsigned_tx_hex)?;
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
        /// The dummy output of a one-payment transaction without change. Its
        /// `address` is empty.
        pub is_dummy: bool,
    }

    /// One transaction boundary in the reviewed container. Amounts are piconero.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TransactionSummary {
        /// The account every input was selected from; change returns to its
        /// minor index zero.
        pub source_account: u32,
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
        digest.update(b"monero-rust offline transaction approval v4");
        digest.update((network_str.len() as u64).to_le_bytes());
        digest.update(network_str.as_bytes());
        // Bind public identities, never retain secret key bytes in the review.
        for secret in [&spend_secret_key, &view_secret_key] {
            let secret = Zeroizing::new(Scalar::from_bytes_mod_order(*secret));
            digest.update((&*secret * ED25519_BASEPOINT_TABLE).compress().to_bytes());
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
            *spend_secret_key,
            *view_secret_key,
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
        let unsigned_bytes = decode_unsigned_tx_hex(unsigned_tx_hex)?;

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
                is_dummy: false,
            })
            .collect();
        if let Some(change) = &resolved.change {
            destinations.push(DestinationPreview {
                address: change.address.to_string(),
                amount: change.amount,
                is_change: true,
                is_ours: change.is_ours,
                is_dummy: false,
            });
        }
        if let Some(amount) = resolved.dummy_amount {
            destinations.push(DestinationPreview {
                address: String::new(),
                amount,
                is_change: false,
                is_ours: false,
                is_dummy: true,
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
            source_account: unsigned.source_account,
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
            unsigned.change.and_then(|change| change.subaddress()),
        )?;

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
                *spend_secret_key,
                **expected.secret_spend_key.as_ref().unwrap(),
                "signer's spend key must be the wallet's spend key"
            );
            assert_eq!(
                *view_secret_key, *expected.secret_view_key,
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
        fn input_selection_never_crosses_account_boundaries() {
            let output = |subaddress| StoredOutputData {
                tx_hash: String::new(),
                output_index: 0,
                amount: 1,
                key: String::new(),
                key_offset: String::new(),
                commitment_mask: String::new(),
                subaddress,
                payment_id: None,
                received_output_bytes: String::new(),
                global_index: None,
                key_image: String::new(),
                ring: None,
            };

            assert_eq!(source_account(&[output(None), output(Some((0, 7)))]), Ok(0));
            assert_eq!(
                source_account(&[output(Some((3, 2))), output(Some((3, 9)))]),
                Ok(3)
            );
            let error = source_account(&[output(Some((3, 2))), output(Some((4, 0)))])
                .expect_err("wallet2 does not combine accounts");
            assert!(error.contains("different accounts"), "{error}");
        }

        #[test]
        fn unsigned_envelope_is_returned_only_after_its_ring_is_recorded() {
            let selection = DecoySelection {
                real_index: 0,
                offsets: vec![1; 16],
                ring: vec![
                    RingMember {
                        key: "00".repeat(32),
                        commitment: "00".repeat(32),
                    };
                    16
                ],
            };
            let mut state = crate::wallet_state::WalletState::new();
            state.add_outputs(vec![crate::wallet_output::WalletOutput {
                tx_hash: "tx".into(),
                output_index: 0,
                amount: 1,
                key: String::new(),
                key_offset: String::new(),
                commitment_mask: String::new(),
                subaddress_index: Some((0, 0)),
                payment_id: None,
                received_output_bytes: String::new(),
                global_index: Some(1),
                ring: None,
                block_height: 1,
                spent: false,
                spent_height: None,
                key_image: "ki".into(),
                is_coinbase: false,
                frozen: false,
            }]);
            let result = UnsignedTransactionResult {
                unsigned_tx_hex: "envelope".into(),
                fee: 1,
                recipients: vec![],
                input_rings: vec![InputRing {
                    key_image: "ki".into(),
                    ring: selection.clone(),
                }],
            };

            let exported = persist_unsigned_transaction_result(result, &mut state).unwrap();
            assert_eq!(exported.unsigned_tx_hex, "envelope");
            assert_eq!(state.outputs()[0].ring, Some(selection));
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
            let restored = selection.to_decoys().unwrap();
            assert_eq!(restored.offsets(), decoys.offsets());
            assert_eq!(restored.signer_index(), decoys.signer_index());
            assert_eq!(restored.ring(), decoys.ring());
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
        fn spend_uses_the_persisted_global_index() {
            let output = parse_stored_output(&hex::encode(stored_output_bytes(0))).unwrap();
            assert_eq!(output.index_on_blockchain(), 0);

            let stored = StoredOutputData {
                tx_hash: hex::encode(output.transaction()),
                output_index: 1,
                amount: 1234,
                key: String::new(),
                key_offset: String::new(),
                commitment_mask: String::new(),
                subaddress: None,
                payment_id: None,
                received_output_bytes: hex::encode(stored_output_bytes(0)),
                global_index: Some(424242),
                key_image: String::new(),
                ring: None,
            };
            let (output, global_index) = spendable_from_stored_data(&stored).unwrap();
            let spent = spent_output(&output, global_index);
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
                global_index: None,
                key_image: String::new(),
                ring: None,
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
                input_rings: vec![],
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

        #[test]
        fn scanned_wallet_policy_counts_blocks_like_wallet2() {
            let policy = FeePolicy::for_scanned_wallet(FeePriority::Automatic, 499);
            assert_eq!(
                policy,
                FeePolicy {
                    wallet_height: Some(500),
                    ..FeePolicy::default()
                }
            );
            assert_eq!(
                FeePolicy::for_scanned_wallet(FeePriority::Low, u64::MAX).wallet_height,
                Some(u64::MAX)
            );
        }

        #[tokio::test]
        async fn fee_estimate_uses_the_selected_priority() {
            #[derive(Clone, Debug)]
            struct Connection;
            #[async_trait::async_trait]
            impl RpcConnection for Connection {
                async fn post(
                    &self,
                    route: &str,
                    body: Vec<u8>,
                ) -> Result<Vec<u8>, crate::monero_rpc::RpcError> {
                    assert_eq!(route, "json_rpc");
                    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    let result = match request["method"].as_str().unwrap() {
                        "get_last_block_header" => serde_json::json!({
                            "block_header": { "major_version": 16 }
                        }),
                        "get_fee_estimate" => serde_json::json!({
                            "fee": 32_000,
                            "fees": [32_000, 130_000, 510_000, 6_400_000],
                            "quantization_mask": 10_000,
                            "status": "OK"
                        }),
                        method => panic!("unexpected method {method}"),
                    };
                    Ok(serde_json::to_vec(&serde_json::json!({ "result": result })).unwrap())
                }
            }

            let policy = FeePolicy {
                priority: FeePriority::Normal,
                wallet_height: Some(123),
                max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
            };
            let estimate = estimate_fee_with_rpc_and_policy(
                &Rpc::new_with_connection(Connection),
                1,
                2,
                policy,
            )
            .await
            .unwrap();
            assert_eq!(estimate.per_weight, 130_000);
            assert_eq!(estimate.mask, 10_000);
        }

        #[tokio::test]
        async fn a_send_short_of_wallet2s_fee_estimate_asks_for_no_decoys() {
            use std::sync::{Arc, Mutex};

            #[derive(Clone, Debug, Default)]
            struct Connection(Arc<Mutex<Vec<String>>>);
            #[async_trait::async_trait]
            impl RpcConnection for Connection {
                async fn post(
                    &self,
                    route: &str,
                    body: Vec<u8>,
                ) -> Result<Vec<u8>, crate::monero_rpc::RpcError> {
                    let method = if route == "json_rpc" {
                        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        request["method"].as_str().unwrap().to_string()
                    } else {
                        route.to_string()
                    };
                    self.0.lock().unwrap().push(method.clone());
                    let result = match method.as_str() {
                        "get_last_block_header" => serde_json::json!({
                            "block_header": { "major_version": 16 }
                        }),
                        "get_fee_estimate" => serde_json::json!({
                            "fee": 32_000,
                            "fees": [32_000, 130_000, 510_000, 6_400_000],
                            "quantization_mask": 10_000,
                            "status": "OK"
                        }),
                        _ => return Err(crate::monero_rpc::RpcError::ConnectionError),
                    };
                    Ok(serde_json::to_vec(&serde_json::json!({ "result": result })).unwrap())
                }
            }

            const SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
            const INPUT: u64 = 1_000_000_000_000;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&[0x11; 32]); // transaction hash
            bytes.extend_from_slice(&1u64.to_le_bytes()); // index in transaction
            bytes.extend_from_slice(&0u64.to_le_bytes()); // index on blockchain
            bytes.extend_from_slice(
                (&Scalar::from(9u64) * ED25519_BASEPOINT_TABLE)
                    .compress()
                    .as_bytes(),
            );
            bytes.extend_from_slice(&Scalar::from(5u64).to_bytes()); // key offset
            bytes.extend_from_slice(&Scalar::from(3u64).to_bytes()); // commitment mask
            bytes.extend_from_slice(&INPUT.to_le_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]); // timelock, subaddress, payment ID, data
            let stored = StoredOutputData {
                tx_hash: "11".repeat(32),
                output_index: 1,
                amount: INPUT,
                key: String::new(),
                key_offset: String::new(),
                commitment_mask: String::new(),
                subaddress: None,
                payment_id: None,
                received_output_bytes: hex::encode(bytes),
                global_index: Some(424_242),
                key_image: String::new(),
                ring: None,
            };
            let address = crate::scanner::derive_address(SEED, Network::Mainnet, "")
                .unwrap()
                .to_string();
            let policy = FeePolicy {
                priority: FeePriority::Low,
                wallet_height: None,
                max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
            };
            let estimated_fee = (1_492u64 * 32_000).div_ceil(10_000) * 10_000;
            let send = |amount: u64| {
                let connection = Connection::default();
                let log = connection.0.clone();
                let stored = stored.clone();
                let recipients = vec![(address.clone(), amount)];
                async move {
                    let result = create_transaction_with_rpc_and_policy(
                        &Rpc::new_with_connection(connection),
                        SEED,
                        "mainnet",
                        vec![stored],
                        &recipients,
                        "",
                        policy,
                    )
                    .await;
                    let log = log.lock().unwrap().clone();
                    (result.map(|_| ()).unwrap_err(), log)
                }
            };

            let (error, log) = send(INPUT - estimated_fee + 1).await;
            assert!(error.starts_with("Insufficient funds"), "{error}");
            assert_eq!(log, ["get_last_block_header", "get_fee_estimate"]);

            let (error, log) = send(INPUT - estimated_fee).await;
            assert!(
                error.starts_with("Failed to prepare transaction"),
                "{error}"
            );
            assert_eq!(log[2..], ["get_height"]);
        }

        #[test]
        fn prepared_transaction_labels_pre_decoy_values_as_bounds() {
            let prepared = PreparedTransaction {
                node_url: "http://127.0.0.1:18081".to_string(),
                network: "mainnet".to_string(),
                recipients: vec![],
                fee_upper_bound: 20,
                total_input: 100,
                change_lower_bound: 80,
                stored_outputs: vec![],
            };
            let json = serde_json::to_value(&prepared).unwrap();
            assert_eq!(json["fee_upper_bound"], 20);
            assert_eq!(json["change_lower_bound"], 80);
            assert!(json.get("fee").is_none());
            assert!(json.get("change").is_none());

            let old = serde_json::json!({
                "node_url": "http://127.0.0.1:18081",
                "network": "mainnet",
                "recipients": [],
                "fee": 20,
                "total_input": 100,
                "change": 80,
                "stored_outputs": []
            });
            let restored: PreparedTransaction = serde_json::from_value(old).unwrap();
            assert_eq!(restored.fee_upper_bound, 20);
            assert_eq!(restored.change_lower_bound, 80);
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
            let result = decide_change(1_000_000, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_dust_multi_recipient() {
            let result = decide_change(1_000_000, 2);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_at_dust_threshold_boundary() {
            use crate::coin_selection::DUST_THRESHOLD;

            let result = decide_change(DUST_THRESHOLD - 1, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);

            let result = decide_change(DUST_THRESHOLD - 1, 2);
            assert_eq!(result, ChangeDecision::IncludeChange);

            // At threshold -> not dust, include change
            let result = decide_change(DUST_THRESHOLD, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);

            let result = decide_change(DUST_THRESHOLD, 2);
            assert_eq!(result, ChangeDecision::IncludeChange);
        }

        #[test]
        fn test_decide_change_one_piconero() {
            let result = decide_change(1, 1);
            assert_eq!(result, ChangeDecision::IncludeChange);

            let result = decide_change(1, 3);
            assert_eq!(result, ChangeDecision::IncludeChange);
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
