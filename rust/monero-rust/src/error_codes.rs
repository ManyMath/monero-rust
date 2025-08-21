//! Structured error codes and ErrorResponse type for the Monero wallet.

use monero_serai::{
    rpc::RpcError,
    wallet::{
        address::{AddressError, Network as SeraiNetwork},
        seed::SeedError,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Stagenet,
}

impl Network {
    pub fn from_str(s: &str) -> Result<Self, ErrorResponse> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "stagenet" => Ok(Network::Stagenet),
            _ => Err(ErrorResponse::new(
                ERR_INVALID_NETWORK,
                format!("Invalid network: '{}'. Expected mainnet, testnet, or stagenet", s),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Stagenet => "stagenet",
        }
    }

    pub fn to_serai(self) -> SeraiNetwork {
        match self {
            Network::Mainnet => SeraiNetwork::Mainnet,
            Network::Testnet => SeraiNetwork::Testnet,
            Network::Stagenet => SeraiNetwork::Stagenet,
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn validate_network(network_str: &str) -> Result<(), ErrorResponse> {
    Network::from_str(network_str)?;
    Ok(())
}

pub fn validate_node_url(url: &str) -> Result<(), ErrorResponse> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(ErrorResponse::new(
            ERR_INVALID_URL,
            "Node URL is empty",
        ).with_hint("Provide a URL like http://127.0.0.1:18081"));
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(ErrorResponse::new(
            ERR_INVALID_URL,
            format!("Node URL has invalid scheme: '{}'", trimmed),
        ).with_hint("URL must start with http:// or https://"));
    }
    Ok(())
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: u32,
    pub message: String,
    pub hint: Option<String>,
    pub transient: bool,
}

impl ErrorResponse {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            transient: false,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn transient(mut self) -> Self {
        self.transient = true;
        self
    }

    /// Classify a bare `String` error from the core library into a structured
    /// `ErrorResponse` by pattern-matching on the message text.
    ///
    /// This is the preferred way to convert legacy `Result<_, String>` errors
    /// that originate from `monero-rust` into structured codes when the
    /// original typed error (e.g. `SeedError`, `RpcError`) is no longer
    /// available.
    pub fn from_string(msg: &str) -> Self {
        let lower = msg.to_lowercase();

        // Seed / mnemonic errors
        if lower.contains("invalid seed") || lower.contains("failed to parse seed")
            || lower.contains("invalid bip39") || lower.contains("failed to parse derived legacy seed")
        {
            return ErrorResponse::new(ERR_SEED_INVALID, msg)
                .with_hint("Double-check the seed phrase for typos");
        }
        if lower.contains("seed phrase is empty") {
            return ErrorResponse::new(ERR_EMPTY_FIELD, msg)
                .with_hint("Provide a non-empty seed phrase");
        }
        if lower.contains("unknown language") {
            return ErrorResponse::new(ERR_SEED_UNKNOWN_LANGUAGE, msg);
        }
        if lower.contains("invalid checksum") {
            return ErrorResponse::new(ERR_SEED_INVALID_CHECKSUM, msg)
                .with_hint("Double-check the seed phrase for typos");
        }
        if lower.contains("invalid seed length") || lower.contains("invalid word count") {
            return ErrorResponse::new(ERR_SEED_INVALID_LENGTH, msg)
                .with_hint("A classic seed is 25 words; a polyseed is 16 words");
        }

        // Network errors
        if lower.contains("invalid network") {
            return ErrorResponse::new(ERR_INVALID_NETWORK, msg)
                .with_hint("Expected mainnet, testnet, or stagenet");
        }

        // Address errors
        if lower.contains("invalid address") || lower.contains("address") && lower.contains("invalid") {
            return ErrorResponse::new(ERR_INVALID_ADDRESS, msg);
        }
        if lower.contains("different network") {
            return ErrorResponse::new(ERR_ADDRESS_WRONG_NETWORK, msg);
        }

        // RPC / connection errors
        if lower.contains("connection") || lower.contains("fetch failed")
            || lower.contains("failed to get height") || lower.contains("failed to create rpc")
        {
            return ErrorResponse::new(ERR_RPC_CONNECTION, msg)
                .with_hint("Check your network connection and node URL")
                .transient();
        }

        // No outputs
        if lower.contains("no spendable outputs") || lower.contains("no selected outputs")
            || lower.contains("no confirmed outputs")
        {
            return ErrorResponse::new(ERR_NO_OUTPUTS, msg);
        }

        // Insufficient funds
        if lower.contains("insufficient") {
            return ErrorResponse::new(ERR_INSUFFICIENT_FUNDS, msg);
        }

        // Transaction build failures
        if lower.contains("transaction") && (lower.contains("failed") || lower.contains("error")) {
            return ErrorResponse::new(ERR_TX_BUILD_FAILED, msg);
        }

        // Fallback: internal error
        ErrorResponse::new(ERR_INTERNAL, msg)
    }
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorResponse {}

// Validation (1000–1099)
pub const ERR_EMPTY_FIELD: u32 = 1000;
pub const ERR_INVALID_NETWORK: u32 = 1001;
pub const ERR_INVALID_ADDRESS: u32 = 1002;
pub const ERR_ADDRESS_WRONG_NETWORK: u32 = 1003;

// Wallet state (1100–1199)
pub const ERR_WALLET_NOT_READY: u32 = 1100;
pub const ERR_NO_OUTPUTS: u32 = 1101;
pub const ERR_INSUFFICIENT_FUNDS: u32 = 1102;

// Transaction (1200–1299)
pub const ERR_TX_BUILD_FAILED: u32 = 1200;
pub const ERR_TX_BROADCAST_REJECTED: u32 = 1201;
pub const ERR_TX_DOUBLE_SPEND: u32 = 1202;

// RPC / network (2000–2099)
pub const ERR_RPC_CONNECTION: u32 = 2000;
pub const ERR_RPC_INVALID_NODE: u32 = 2001;
pub const ERR_RPC_UNSUPPORTED_PROTOCOL: u32 = 2002;
pub const ERR_RPC_TX_NOT_FOUND: u32 = 2003;
pub const ERR_RPC_PRUNED_TX: u32 = 2004;
pub const ERR_RPC_INVALID_TX: u32 = 2005;
pub const ERR_RPC_INTERNAL: u32 = 2006;
pub const ERR_RPC_INVALID_POINT: u32 = 2007;

// Cryptographic (3000–3099)
pub const ERR_SEED_INVALID_LENGTH: u32 = 3000;
pub const ERR_SEED_INVALID_CHECKSUM: u32 = 3001;
pub const ERR_SEED_UNKNOWN_LANGUAGE: u32 = 3002;
pub const ERR_SEED_INVALID: u32 = 3003;
pub const ERR_SEED_ENGLISH_OLD_CHECKSUM: u32 = 3004;
pub const ERR_INVALID_KEY: u32 = 3005;

// Internal (9000–9099)
pub const ERR_INTERNAL: u32 = 9000;
pub const ERR_SERIALIZATION: u32 = 9001;

impl From<RpcError> for ErrorResponse {
    fn from(e: RpcError) -> Self {
        match &e {
            RpcError::ConnectionError => {
                ErrorResponse::new(ERR_RPC_CONNECTION, e.to_string())
                    .with_hint("Check your network connection and node URL")
                    .transient()
            }
            RpcError::InvalidNode => {
                ErrorResponse::new(ERR_RPC_INVALID_NODE, e.to_string())
                    .with_hint("The node returned an invalid response; try a different node")
            }
            RpcError::InternalError(_) => {
                ErrorResponse::new(ERR_RPC_INTERNAL, e.to_string())
            }
            RpcError::UnsupportedProtocol(_) => {
                ErrorResponse::new(ERR_RPC_UNSUPPORTED_PROTOCOL, e.to_string())
                    .with_hint("The node may be running an outdated version of Monero")
            }
            RpcError::TransactionsNotFound(_) => {
                ErrorResponse::new(ERR_RPC_TX_NOT_FOUND, e.to_string())
                    .transient()
            }
            RpcError::InvalidPoint(_) => {
                ErrorResponse::new(ERR_RPC_INVALID_POINT, e.to_string())
            }
            RpcError::PrunedTransaction => {
                ErrorResponse::new(ERR_RPC_PRUNED_TX, e.to_string())
                    .with_hint("Try using a full (non-pruned) node")
            }
            RpcError::InvalidTransaction(_) => {
                ErrorResponse::new(ERR_RPC_INVALID_TX, e.to_string())
            }
            RpcError::FeeExceedsLimit { .. } => {
                ErrorResponse::new(ERR_RPC_INVALID_NODE, e.to_string())
                    .with_hint("The node returned an unreasonable fee; try a different node")
            }
            RpcError::ZeroFee => {
                ErrorResponse::new(ERR_RPC_INVALID_NODE, e.to_string())
                    .with_hint("The node returned a zero fee; try a different node")
            }
            RpcError::ZeroMask => {
                ErrorResponse::new(ERR_RPC_INVALID_NODE, e.to_string())
                    .with_hint("The node returned a zero fee mask; try a different node")
            }
        }
    }
}

impl From<SeedError> for ErrorResponse {
    fn from(e: SeedError) -> Self {
        match e {
            SeedError::InvalidSeedLength => {
                ErrorResponse::new(ERR_SEED_INVALID_LENGTH, e.to_string())
                    .with_hint("A classic seed is 25 words; a polyseed is 16 words")
            }
            SeedError::UnknownLanguage => {
                ErrorResponse::new(ERR_SEED_UNKNOWN_LANGUAGE, e.to_string())
            }
            SeedError::InvalidChecksum => {
                ErrorResponse::new(ERR_SEED_INVALID_CHECKSUM, e.to_string())
                    .with_hint("Double-check the seed phrase for typos")
            }
            SeedError::EnglishOldWithChecksum => {
                ErrorResponse::new(ERR_SEED_ENGLISH_OLD_CHECKSUM, e.to_string())
            }
            SeedError::InvalidSeed => {
                ErrorResponse::new(ERR_SEED_INVALID, e.to_string())
                    .with_hint("The seed phrase could not be decoded")
            }
        }
    }
}

impl From<AddressError> for ErrorResponse {
    fn from(e: AddressError) -> Self {
        match e {
            AddressError::InvalidByte => {
                ErrorResponse::new(ERR_INVALID_ADDRESS, e.to_string())
                    .with_hint("The address prefix byte is not recognized")
            }
            AddressError::InvalidEncoding => {
                ErrorResponse::new(ERR_INVALID_ADDRESS, e.to_string())
                    .with_hint("The address is not valid base58")
            }
            AddressError::InvalidLength => {
                ErrorResponse::new(ERR_INVALID_ADDRESS, e.to_string())
                    .with_hint("The address has an unexpected length")
            }
            AddressError::InvalidKey => {
                ErrorResponse::new(ERR_INVALID_KEY, e.to_string())
                    .with_hint("The address contains an invalid cryptographic key")
            }
            AddressError::UnknownFeatures => {
                ErrorResponse::new(ERR_INVALID_ADDRESS, e.to_string())
            }
            AddressError::DifferentNetwork => {
                ErrorResponse::new(ERR_ADDRESS_WRONG_NETWORK, e.to_string())
                    .with_hint("The address belongs to a different Monero network")
            }
        }
    }
}
