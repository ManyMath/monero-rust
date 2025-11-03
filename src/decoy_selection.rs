use crate::WalletError;
use monero_wallet::{OutputWithDecoys, WalletOutput, rpc::DecoyRpc};
use rand_core::{CryptoRng, RngCore};

#[derive(Debug, Clone)]
pub struct DecoySelectionConfig {
    pub ring_size: u8,
    pub height: usize,
    pub deterministic: bool,
}

impl Default for DecoySelectionConfig {
    fn default() -> Self {
        Self {
            ring_size: 16,
            height: 0,
            deterministic: false,
        }
    }
}

pub async fn select_decoys_for_output<R: RngCore + CryptoRng + Send + Sync>(
    rng: &mut R,
    rpc: &impl DecoyRpc,
    output: WalletOutput,
    config: &DecoySelectionConfig,
) -> Result<OutputWithDecoys, WalletError> {
    if config.ring_size != 16 {
        return Err(WalletError::Other(format!(
            "ring size must be 16, got {}",
            config.ring_size
        )));
    }
    if config.height == 0 {
        return Err(WalletError::Other("height not set".into()));
    }

    const MAX_RETRIES: u32 = 3;

    for attempt in 0..MAX_RETRIES {
        let result = if config.deterministic {
            OutputWithDecoys::fingerprintable_deterministic_new(
                rng,
                rpc,
                config.ring_size,
                config.height,
                output.clone(),
            )
            .await
        } else {
            OutputWithDecoys::new(rng, rpc, config.ring_size, config.height, output.clone()).await
        };

        match result {
            Ok(decoys) => return Ok(decoys),
            Err(_) if attempt + 1 < MAX_RETRIES => {
                let delay = 100 * (1 << attempt);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            Err(e) => {
                return Err(WalletError::Other(format!("decoy selection failed: {}", e)));
            }
        }
    }

    unreachable!()
}

pub async fn select_decoys_for_outputs<R: RngCore + CryptoRng + Send + Sync>(
    rng: &mut R,
    rpc: &impl DecoyRpc,
    outputs: Vec<WalletOutput>,
    config: &DecoySelectionConfig,
) -> Result<Vec<OutputWithDecoys>, WalletError> {
    let mut results = Vec::with_capacity(outputs.len());
    for output in outputs {
        results.push(select_decoys_for_output(rng, rpc, output, config).await?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = DecoySelectionConfig::default();
        assert_eq!(config.ring_size, 16);
        assert!(!config.deterministic);
    }
}
