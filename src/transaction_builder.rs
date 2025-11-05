use crate::{
    WalletError, WalletState,
    input_selection::{InputSelectionConfig, InputSelectionError, select_inputs},
    decoy_selection::{DecoySelectionConfig, select_decoys_for_outputs},
    fee_calculation,
    types::{KeyImage, TxKey},
};

use monero_wallet::{
    rpc::{FeePriority, Rpc},
    send::Change,
    address::MoneroAddress,
    transaction::Transaction,
    ringct::RctType,
    WalletOutput,
};

use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionPriority {
    Default = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Unimportant = 4,
}

impl TransactionPriority {
    pub fn to_fee_priority(self) -> FeePriority {
        match self {
            TransactionPriority::Unimportant => FeePriority::Unimportant,
            TransactionPriority::Low | TransactionPriority::Default | TransactionPriority::Medium => FeePriority::Normal,
            TransactionPriority::High => FeePriority::Elevated,
        }
    }

    pub fn from_u8(value: u8) -> Result<Self, WalletError> {
        match value {
            0 => Ok(TransactionPriority::Default),
            1 => Ok(TransactionPriority::Low),
            2 => Ok(TransactionPriority::Medium),
            3 => Ok(TransactionPriority::High),
            4 => Ok(TransactionPriority::Unimportant),
            _ => Err(WalletError::Other(format!("invalid priority: {}", value))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PendingTransaction {
    pub tx: Transaction,
    pub tx_key: TxKey,
    pub fee: u64,
    pub amount: u64,
    pub destinations: Vec<String>,
    pub selected_inputs: Vec<KeyImage>,
}

impl PendingTransaction {
    pub fn txid(&self) -> [u8; 32] {
        self.tx_key.txid
    }

    pub fn fee(&self) -> u64 {
        self.fee
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }

    pub fn destinations(&self) -> &[String] {
        &self.destinations
    }

    pub fn num_inputs(&self) -> usize {
        self.selected_inputs.len()
    }

    pub fn transaction_bytes(&self) -> Vec<u8> {
        self.tx.serialize()
    }
}

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub priority: TransactionPriority,
    pub account_index: u32,
    pub preferred_inputs: Option<Vec<KeyImage>>,
    pub payment_id: Option<Vec<u8>>,
    pub sweep_all: bool,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            priority: TransactionPriority::Default,
            account_index: 0,
            preferred_inputs: None,
            payment_id: None,
            sweep_all: false,
        }
    }
}

impl WalletState {
    pub async fn create_tx(
        &mut self,
        destination: &str,
        amount: u64,
        config: TransactionConfig,
    ) -> Result<PendingTransaction, WalletError> {
        self.create_tx_multi_dest(&[(destination, amount)], config).await
    }

    pub async fn create_tx_multi_dest(
        &mut self,
        destinations: &[(&str, u64)],
        config: TransactionConfig,
    ) -> Result<PendingTransaction, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.is_connected {
            return Err(WalletError::NotConnected);
        }
        if destinations.is_empty() {
            return Err(WalletError::Other("no destinations".to_string()));
        }
        if destinations.len() > 16 {
            return Err(WalletError::Other(format!(
                "too many destinations: {} (max 16)", destinations.len()
            )));
        }

        let mut parsed_destinations = Vec::with_capacity(destinations.len());
        let mut total_amount = 0u64;

        for (addr_str, amt) in destinations {
            if *amt == 0 {
                return Err(WalletError::Other("cannot send zero".to_string()));
            }
            total_amount = total_amount.saturating_add(*amt);

            let addr = MoneroAddress::from_str(self.network, addr_str)
                .map_err(|e| WalletError::Other(format!("bad address {}: {}", addr_str, e)))?;
            parsed_destinations.push((addr, *amt));
        }

        let estimated_fee = self.estimate_tx_fee(destinations.len(), config.priority).await?;

        let target_amount = if config.sweep_all {
            0
        } else {
            total_amount.saturating_add(estimated_fee)
        };

        let selection_config = InputSelectionConfig {
            target_amount,
            fee_per_byte: None,
            preferred_inputs: config.preferred_inputs.clone(),
            sweep_all: config.sweep_all,
        };

        let selected = select_inputs(self, selection_config)?;

        let rpc = self.rpc_client.read().await;
        let rpc_ref = rpc.as_ref().ok_or(WalletError::NotConnected)?;

        let mut wallet_outputs = Vec::with_capacity(selected.inputs.len());
        for serializable_output in &selected.inputs {
            let wallet_output = self.reconstruct_wallet_output(rpc_ref, serializable_output).await?;
            wallet_outputs.push(wallet_output);
        }

        let decoy_config = DecoySelectionConfig {
            ring_size: 16,
            height: self.daemon_height as usize,
            deterministic: false,
        };

        let outputs_with_decoys = select_decoys_for_outputs(
            &mut OsRng,
            rpc_ref,
            wallet_outputs,
            &decoy_config,
        ).await?;

        let mut outgoing_view_key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut outgoing_view_key_bytes);
        let outgoing_view_key = Zeroizing::new(outgoing_view_key_bytes);
        let outgoing_view_key_copy = Zeroizing::new(outgoing_view_key_bytes);

        let change_addr = MoneroAddress::new(
            self.network,
            monero_wallet::address::AddressType::Legacy,
            self.view_pair.spend(),
            self.view_pair.view()
        );
        let change = Change::fingerprintable(Some(change_addr));

        let fee_rate = rpc_ref.get_fee_rate(config.priority.to_fee_priority()).await
            .map_err(WalletError::RpcError)?;

        let rct_type = RctType::ClsagBulletproofPlus;

        let signable = monero_wallet::send::SignableTransaction::new(
            rct_type,
            outgoing_view_key,
            outputs_with_decoys,
            parsed_destinations.clone(),
            change,
            vec![],
            fee_rate,
        ).map_err(|e| WalletError::Other(format!("build error: {}", e)))?;

        let spend_key = self.spend_key.as_ref()
            .ok_or(WalletError::Other("view-only wallet cannot sign".to_string()))?;

        let signed_tx = signable.sign(&mut OsRng, spend_key)
            .map_err(|e| WalletError::Other(format!("sign error: {}", e)))?;

        let txid = signed_tx.hash();

        let fee = match &signed_tx {
            Transaction::V2 { proofs: Some(proofs), .. } => proofs.base.fee,
            _ => return Err(WalletError::Other("invalid tx format".to_string())),
        };

        let tx_key = TxKey {
            txid,
            tx_private_key: outgoing_view_key_copy,
            additional_tx_keys: vec![],
        };

        let dest_strings: Vec<String> = destinations.iter()
            .map(|(addr, _)| addr.to_string())
            .collect();

        Ok(PendingTransaction {
            tx: signed_tx,
            tx_key,
            fee,
            amount: total_amount,
            destinations: dest_strings,
            selected_inputs: selected.inputs.iter().map(|o| o.key_image).collect(),
        })
    }

    pub async fn estimate_tx_fee(
        &self,
        num_destinations: usize,
        priority: TransactionPriority,
    ) -> Result<u64, WalletError> {
        let rpc = self.rpc_client.read().await;
        let rpc_ref = rpc.as_ref().ok_or(WalletError::NotConnected)?;

        let fee_rate = fee_calculation::get_fee_rate_for_priority(rpc_ref, priority).await?;
        Ok(fee_calculation::estimate_fee(2, num_destinations, &fee_rate, false))
    }

    async fn reconstruct_wallet_output(
        &self,
        rpc: &impl Rpc,
        serializable: &crate::types::SerializableOutput,
    ) -> Result<WalletOutput, WalletError> {
        let cache_key = (serializable.tx_hash, serializable.output_index);

        // check cache first
        if let Some(cached) = self.commitment_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let block = rpc.get_block_by_number(serializable.height as usize).await
            .map_err(WalletError::RpcError)?;

        let scannable_block = rpc.get_scannable_block(block).await
            .map_err(WalletError::RpcError)?;

        let mut scanner = self.scanner.clone();
        let scanned_outputs = scanner.scan(scannable_block)
            .map_err(|e| WalletError::Other(format!("scan error: {}", e)))?
            .ignore_additional_timelock();

        for output in scanned_outputs {
            if output.transaction() == serializable.tx_hash
                && output.index_in_transaction() == serializable.output_index
            {
                return Ok(output);
            }
        }

        Err(WalletError::Other(format!(
            "output not found: tx {} index {} height {}",
            hex::encode(serializable.tx_hash),
            serializable.output_index,
            serializable.height
        )))
    }

    pub async fn commit_tx(&mut self, pending: &PendingTransaction) -> Result<[u8; 32], WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.is_connected {
            return Err(WalletError::NotConnected);
        }

        for key_image in &pending.selected_inputs {
            if !self.outputs.contains_key(key_image) {
                return Err(WalletError::Other(format!(
                    "input {} not in wallet", hex::encode(key_image)
                )));
            }
            if self.spent_outputs.contains(key_image) {
                return Err(WalletError::Other(format!(
                    "input {} already spent", hex::encode(key_image)
                )));
            }
        }

        let total_input: u64 = pending.selected_inputs
            .iter()
            .filter_map(|ki| self.outputs.get(ki))
            .map(|o| o.amount)
            .sum();

        let required = pending.amount.saturating_add(pending.fee);
        if total_input < required {
            return Err(WalletError::Other(format!(
                "inputs {} < required {}", total_input, required
            )));
        }

        let txid = pending.txid();

        {
            let rpc = self.rpc_client.read().await;
            let rpc_ref = rpc.as_ref().ok_or(WalletError::NotConnected)?;
            rpc_ref.publish_transaction(&pending.tx).await
                .map_err(|e| WalletError::Other(format!("broadcast failed: {}", e)))?;
        }

        for key_image in &pending.selected_inputs {
            self.spent_outputs.insert(*key_image);
        }

        use crate::types::Transaction as TxRecord;
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let tx = TxRecord::new_outgoing(
            txid,
            None,
            timestamp,
            pending.amount,
            pending.fee,
            pending.destinations.clone(),
        );

        self.transactions.insert(txid, tx);
        self.store_tx_key(txid, pending.tx_key.clone())?;

        Ok(txid)
    }
}

impl From<InputSelectionError> for WalletError {
    fn from(err: InputSelectionError) -> Self {
        WalletError::Other(format!("{}", err))
    }
}
