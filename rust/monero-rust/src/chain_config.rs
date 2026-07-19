// Findings from codebase investigation:
//
// RPC URL: passed by caller at scan time (e.g., StartContinuousScan.node_url).
//   Not hardcoded anywhere in monero-rust; WalletActor stores it as scan_node_url: String.
//
// Genesis: not stored as a hash in monero-rust. Scanner functions take start_height: u64
//   and call rpc.get_block_hash(start_height) at runtime to obtain the anchor from the daemon.
//   ChainConfig.genesis_hash is the known-correct hash for height 0, enabling future
//   anchor verification independent of the daemon.
//
// Network: backend wallet address network, variants Mainnet / Testnet / Stagenet.
//   Used in scanner.rs via parse_network() for address derivation.
//
// monero-serai Scanner: does not take a network parameter directly; network is
//   embedded in the ViewPair / address derivation step.

use monero_wallet::address::Network;

/// Configuration for a specific Monero-compatible chain.
/// Passed to WalletActor at startup to support both Monero and child chains.
#[derive(Clone, Debug)]
pub struct ChainConfig {
    /// JSON-RPC endpoint URL, e.g. "http://127.0.0.1:38081"
    pub rpc_url: String,
    /// Network type (mainnet / stagenet / testnet)
    pub network: Network,
    /// Genesis block hash (used as the scanner's starting anchor)
    pub genesis_hash: [u8; 32],
    /// Human-readable label for logging
    pub label: &'static str,
}

impl ChainConfig {
    /// Monero stagenet configuration.
    ///
    /// Genesis block hash for Monero stagenet (height 0):
    /// 76ee3cc98646292206cd3e86f74d88b4dcc1d937088645e9b0cbca84b7ce74eb
    pub fn monero_stagenet(rpc_url: String) -> Self {
        ChainConfig {
            rpc_url,
            network: Network::Stagenet,
            genesis_hash: [
                0x76, 0xee, 0x3c, 0xc9, 0x86, 0x46, 0x29, 0x22, 0x06, 0xcd, 0x3e, 0x86, 0xf7, 0x4d,
                0x88, 0xb4, 0xdc, 0xc1, 0xd9, 0x37, 0x08, 0x86, 0x45, 0xe9, 0xb0, 0xcb, 0xca, 0x84,
                0xb7, 0xce, 0x74, 0xeb,
            ],
            label: "monero-stagenet",
        }
    }

    /// Child-chain stagenet configuration.
    ///
    /// Genesis block hash computed from cuprate-child's generate_child_genesis_block():
    /// nonce = 0xDEAD_BEEF, same miner_tx blob as mainnet.
    /// Hash: 5730ff0a9d8f619691f2c2a1f2eba1083cc895fe951fd887201028510e1d66d7
    pub fn child_stagenet(rpc_url: String) -> Self {
        ChainConfig {
            rpc_url,
            network: Network::Stagenet,
            genesis_hash: [
                0x57, 0x30, 0xFF, 0x0A, 0x9D, 0x8F, 0x61, 0x96, 0x91, 0xF2, 0xC2, 0xA1, 0xF2, 0xEB,
                0xA1, 0x08, 0x3C, 0xC8, 0x95, 0xFE, 0x95, 0x1F, 0xD8, 0x87, 0x20, 0x10, 0x28, 0x51,
                0x0E, 0x1D, 0x66, 0xD7,
            ],
            label: "child-stagenet",
        }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn chain_config_stagenet_genesis_wasm() {
        let cfg = ChainConfig::monero_stagenet("http://localhost:38081".into());
        // Monero stagenet genesis: 76ee3cc9...
        assert_eq!(cfg.genesis_hash[0], 0x76);
        assert_eq!(cfg.label, "monero-stagenet");
    }

    #[wasm_bindgen_test]
    fn chain_config_child_differs_wasm() {
        let monero = ChainConfig::monero_stagenet("http://a".into());
        let child = ChainConfig::child_stagenet("http://b".into());
        assert_ne!(monero.genesis_hash, child.genesis_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_configs_have_different_genesis_hashes() {
        let monero = ChainConfig::monero_stagenet("http://localhost:38081".into());
        let child = ChainConfig::child_stagenet("http://localhost:38091".into());
        assert_ne!(monero.genesis_hash, child.genesis_hash);
        assert_ne!(monero.rpc_url, child.rpc_url);
    }
}
