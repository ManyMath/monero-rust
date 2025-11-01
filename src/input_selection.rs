use crate::types::{KeyImage, SerializableOutput};
use crate::WalletState;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::HashSet;

const LOCK_BLOCKS: u64 = 10;

#[derive(Debug, Clone)]
pub struct InputSelectionConfig {
    pub target_amount: u64,
    pub fee_per_byte: Option<u64>,
    pub preferred_inputs: Option<Vec<KeyImage>>,
    pub sweep_all: bool,
}

#[derive(Debug, Clone)]
pub struct SelectedInputs {
    pub inputs: Vec<SerializableOutput>,
    pub total_amount: u64,
    pub num_inputs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSelectionError {
    InsufficientFunds { available: u64, required: u64 },
    AllOutputsFrozen,
    AllOutputsLocked,
    PreferredInputNotFound { key_image: KeyImage },
    PreferredInputSpent { key_image: KeyImage },
    PreferredInputFrozen { key_image: KeyImage },
    PreferredInputLocked { key_image: KeyImage },
    WalletClosed,
    NoOutputsAvailable,
}

impl std::fmt::Display for InputSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientFunds { available, required } => {
                write!(f, "insufficient funds: need {} piconeros, have {}", required, available)
            }
            Self::AllOutputsFrozen => write!(f, "all outputs are frozen"),
            Self::AllOutputsLocked => write!(f, "all outputs are locked"),
            Self::PreferredInputNotFound { key_image } => {
                write!(f, "preferred input not found: {}", hex::encode(key_image))
            }
            Self::PreferredInputSpent { key_image } => {
                write!(f, "preferred input already spent: {}", hex::encode(key_image))
            }
            Self::PreferredInputFrozen { key_image } => {
                write!(f, "preferred input is frozen: {}", hex::encode(key_image))
            }
            Self::PreferredInputLocked { key_image } => {
                write!(f, "preferred input is locked: {}", hex::encode(key_image))
            }
            Self::WalletClosed => write!(f, "wallet is closed"),
            Self::NoOutputsAvailable => write!(f, "no outputs available"),
        }
    }
}

impl std::error::Error for InputSelectionError {}

fn is_unlocked(output: &SerializableOutput, daemon_height: u64) -> bool {
    daemon_height >= output.height.saturating_add(LOCK_BLOCKS)
}

fn is_available(output: &SerializableOutput, key_image: &KeyImage, wallet: &WalletState) -> bool {
    !wallet.spent_outputs.contains(key_image)
        && !wallet.frozen_outputs.contains(key_image)
        && is_unlocked(output, wallet.daemon_height)
}

pub fn select_inputs(
    wallet: &WalletState,
    config: InputSelectionConfig,
) -> Result<SelectedInputs, InputSelectionError> {
    if wallet.is_closed {
        return Err(InputSelectionError::WalletClosed);
    }
    if wallet.outputs.is_empty() {
        return Err(InputSelectionError::NoOutputsAvailable);
    }

    let mut selected: Vec<SerializableOutput> = Vec::new();
    let mut selected_keys: HashSet<KeyImage> = HashSet::new();
    let mut total: u64 = 0;

    // Process preferred inputs first
    if let Some(ref preferred) = config.preferred_inputs {
        for ki in preferred {
            let output = wallet.outputs.get(ki)
                .ok_or(InputSelectionError::PreferredInputNotFound { key_image: *ki })?;

            if wallet.spent_outputs.contains(ki) {
                return Err(InputSelectionError::PreferredInputSpent { key_image: *ki });
            }
            if wallet.frozen_outputs.contains(ki) {
                return Err(InputSelectionError::PreferredInputFrozen { key_image: *ki });
            }
            if !is_unlocked(output, wallet.daemon_height) {
                return Err(InputSelectionError::PreferredInputLocked { key_image: *ki });
            }

            selected.push(output.clone());
            selected_keys.insert(*ki);
            total = total.saturating_add(output.amount);
        }
    }

    // Sweep: grab all available outputs
    if config.sweep_all {
        for (ki, output) in &wallet.outputs {
            if selected_keys.contains(ki) {
                continue;
            }
            if is_available(output, ki, wallet) {
                selected.push(output.clone());
                total = total.saturating_add(output.amount);
            }
        }

        if selected.is_empty() {
            return Err(InputSelectionError::NoOutputsAvailable);
        }

        return Ok(SelectedInputs {
            num_inputs: selected.len(),
            inputs: selected,
            total_amount: total,
        });
    }

    // Check if preferred inputs already satisfy the target
    if total >= config.target_amount {
        return Ok(SelectedInputs {
            num_inputs: selected.len(),
            inputs: selected,
            total_amount: total,
        });
    }

    // Gather remaining available outputs
    let mut available: Vec<(KeyImage, SerializableOutput)> = wallet.outputs
        .iter()
        .filter(|(ki, out)| !selected_keys.contains(*ki) && is_available(out, ki, wallet))
        .map(|(ki, out)| (*ki, out.clone()))
        .collect();

    if available.is_empty() && selected.is_empty() {
        let all_frozen = wallet.outputs.keys().all(|ki| wallet.frozen_outputs.contains(ki));
        let all_locked = wallet.outputs.values().all(|o| !is_unlocked(o, wallet.daemon_height));

        if all_frozen {
            return Err(InputSelectionError::AllOutputsFrozen);
        } else if all_locked {
            return Err(InputSelectionError::AllOutputsLocked);
        } else {
            return Err(InputSelectionError::InsufficientFunds {
                available: total,
                required: config.target_amount,
            });
        }
    }

    // Shuffle for privacy
    available.shuffle(&mut thread_rng());

    for (_, output) in available {
        selected.push(output.clone());
        total = total.saturating_add(output.amount);

        if total >= config.target_amount {
            return Ok(SelectedInputs {
                num_inputs: selected.len(),
                inputs: selected,
                total_amount: total,
            });
        }
    }

    Err(InputSelectionError::InsufficientFunds {
        available: total,
        required: config.target_amount,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(amount: u64, height: u64, key_image: KeyImage) -> SerializableOutput {
        SerializableOutput {
            tx_hash: [0u8; 32],
            output_index: 0,
            amount,
            key_image,
            subaddress_indices: (0, 0),
            height,
            unlocked: true,
            spent: false,
            frozen: false,
            payment_id: None,
            key_offset: None,
            output_public_key: None,
        }
    }

    fn make_wallet(daemon_height: u64) -> WalletState {
        use std::collections::HashMap;
        use std::path::PathBuf;
        use monero_wallet::{ViewPair, Scanner, address::Network};
        use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, scalar::Scalar};
        use zeroize::Zeroizing;

        let spend = ED25519_BASEPOINT_POINT;
        let view = Zeroizing::new(Scalar::from_bytes_mod_order([2u8; 32]));
        let view_pair = ViewPair::new(spend, view).unwrap();

        WalletState {
            magic: 0x4D4F4E45524F5758,
            version: 1,
            seed: None,
            view_pair: view_pair.clone(),
            view_only_spend_public: None,
            view_only_view_private: None,
            spend_key: None,
            view_key: None,
            network: Network::Mainnet,
            seed_language: "English".to_string(),
            outputs: HashMap::new(),
            frozen_outputs: HashSet::new(),
            spent_outputs: HashSet::new(),
            transactions: HashMap::new(),
            tx_keys: HashMap::new(),
            refresh_from_height: 0,
            current_scanned_height: 0,
            daemon_height,
            is_syncing: false,
            block_hash_cache: HashMap::new(),
            daemon_address: None,
            is_connected: false,
            password_salt: [0u8; 32],
            password_hash: [0u8; 32],
            wallet_path: PathBuf::from("/tmp/test"),
            is_closed: false,
            keys_checksum: [0u8; 32],
            auto_save_enabled: false,
            rpc_client: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            health_check_handle: None,
            reconnection_policy: crate::rpc::ReconnectionPolicy {
                max_attempts: 0,
                initial_delay: std::time::Duration::from_secs(1),
                max_delay: std::time::Duration::from_secs(1),
                backoff_multiplier: 1.0,
                health_check_interval: std::time::Duration::from_secs(60),
            },
            reconnection_attempts: 0,
            connection_config: None,
            scanner: Scanner::new(view_pair),
            registered_subaddresses: Vec::new(),
            sync_handle: None,
            sync_interval: std::time::Duration::from_secs(60),
            sync_progress_callback: None,
        }
    }

    #[test]
    fn test_unlock_requires_10_confirms() {
        let output = make_output(100, 100, [1u8; 32]);
        assert!(!is_unlocked(&output, 109));
        assert!(is_unlocked(&output, 110));
    }

    #[test]
    fn test_basic_selection() {
        let mut wallet = make_wallet(100);
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 50, [1u8; 32]));
        wallet.outputs.insert([2u8; 32], make_output(2_000_000, 60, [2u8; 32]));
        wallet.outputs.insert([3u8; 32], make_output(3_000_000, 70, [3u8; 32]));

        let config = InputSelectionConfig {
            target_amount: 2_500_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };

        let result = select_inputs(&wallet, config).unwrap();
        assert!(result.total_amount >= 2_500_000);
        assert!(result.num_inputs > 0);
    }

    #[test]
    fn test_empty_wallet() {
        let wallet = make_wallet(100);
        let config = InputSelectionConfig {
            target_amount: 1_000_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::NoOutputsAvailable)));
    }

    #[test]
    fn test_all_frozen() {
        let mut wallet = make_wallet(100);
        let ki = [1u8; 32];
        wallet.outputs.insert(ki, make_output(1_000_000, 50, ki));
        wallet.frozen_outputs.insert(ki);

        let config = InputSelectionConfig {
            target_amount: 500_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::AllOutputsFrozen)));
    }

    #[test]
    fn test_all_locked() {
        let mut wallet = make_wallet(100);
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 95, [1u8; 32]));

        let config = InputSelectionConfig {
            target_amount: 500_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::AllOutputsLocked)));
    }

    #[test]
    fn test_insufficient_funds() {
        let mut wallet = make_wallet(100);
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 50, [1u8; 32]));

        let config = InputSelectionConfig {
            target_amount: 2_000_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };

        let err = select_inputs(&wallet, config).unwrap_err();
        assert!(matches!(err, InputSelectionError::InsufficientFunds { available: 1_000_000, required: 2_000_000 }));
    }

    #[test]
    fn test_sweep_all() {
        let mut wallet = make_wallet(100);
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 50, [1u8; 32]));
        wallet.outputs.insert([2u8; 32], make_output(2_000_000, 60, [2u8; 32]));

        let config = InputSelectionConfig {
            target_amount: 0,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: true,
        };

        let result = select_inputs(&wallet, config).unwrap();
        assert_eq!(result.total_amount, 3_000_000);
        assert_eq!(result.num_inputs, 2);
    }

    #[test]
    fn test_preferred_inputs() {
        let mut wallet = make_wallet(100);
        let ki2 = [2u8; 32];
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 50, [1u8; 32]));
        wallet.outputs.insert(ki2, make_output(2_000_000, 60, ki2));

        let config = InputSelectionConfig {
            target_amount: 1_500_000,
            fee_per_byte: None,
            preferred_inputs: Some(vec![ki2]),
            sweep_all: false,
        };

        let result = select_inputs(&wallet, config).unwrap();
        assert!(result.inputs.iter().any(|o| o.key_image == ki2));
    }

    #[test]
    fn test_preferred_not_found() {
        let mut wallet = make_wallet(100);
        wallet.outputs.insert([1u8; 32], make_output(1_000_000, 50, [1u8; 32]));

        let config = InputSelectionConfig {
            target_amount: 500_000,
            fee_per_byte: None,
            preferred_inputs: Some(vec![[99u8; 32]]),
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::PreferredInputNotFound { .. })));
    }

    #[test]
    fn test_preferred_spent() {
        let mut wallet = make_wallet(100);
        let ki = [1u8; 32];
        wallet.outputs.insert(ki, make_output(1_000_000, 50, ki));
        wallet.spent_outputs.insert(ki);

        let config = InputSelectionConfig {
            target_amount: 500_000,
            fee_per_byte: None,
            preferred_inputs: Some(vec![ki]),
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::PreferredInputSpent { .. })));
    }

    #[test]
    fn test_wallet_closed() {
        let mut wallet = make_wallet(100);
        wallet.is_closed = true;

        let config = InputSelectionConfig {
            target_amount: 1_000_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };
        assert!(matches!(select_inputs(&wallet, config), Err(InputSelectionError::WalletClosed)));
    }

    #[test]
    fn test_selection_is_randomized() {
        let mut wallet = make_wallet(200);
        for i in 0..20u8 {
            let mut ki = [0u8; 32];
            ki[0] = i;
            wallet.outputs.insert(ki, make_output(1_000_000, 100 + i as u64, ki));
        }

        let config = InputSelectionConfig {
            target_amount: 5_000_000,
            fee_per_byte: None,
            preferred_inputs: None,
            sweep_all: false,
        };

        let mut first_selected: Vec<KeyImage> = Vec::new();
        for _ in 0..10 {
            let result = select_inputs(&wallet, config.clone()).unwrap();
            first_selected.push(result.inputs[0].key_image);
        }

        let unique: HashSet<_> = first_selected.iter().collect();
        assert!(unique.len() > 1, "selection should vary across runs");
    }
}
