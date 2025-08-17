//! Structured error codes and ErrorResponse type for the Monero wallet.

use monero_serai::{
    rpc::RpcError,
    wallet::{address::AddressError, seed::SeedError},
};
use serde::{Deserialize, Serialize};

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
