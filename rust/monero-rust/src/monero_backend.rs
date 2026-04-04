//! Compatibility facade for the current Monero protocol backend.
//!
//! This module intentionally re-exports the small backend surface used by
//! `monero-rust` so a future `monero-oxide` migration can happen behind one
//! boundary instead of through scattered direct imports.

pub mod block {
    pub use monero_serai::block::Block;
}

pub mod ringct {
    pub use monero_serai::ringct::{generate_key_image, hash_to_point};
}

pub mod rpc {
    pub use monero_serai::rpc::{
        GetBlocksFastResponse, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
    };

    #[cfg(not(target_arch = "wasm32"))]
    pub use monero_serai::rpc::HttpRpc;
}

pub mod transaction {
    pub use monero_serai::transaction::{Input, Timelock, Transaction};
}

pub mod wallet {
    pub use monero_serai::wallet::{
        sign_offline, Change, Decoys, Fee, InternalPayment, ReceivedOutput, Scanner,
        SignableTransactionBuilder, SpendableOutput, UnsignedInput, UnsignedTransaction, ViewPair,
    };

    pub mod address {
        pub use monero_serai::wallet::address::{
            AddressError, AddressMeta, AddressSpec, AddressType, MoneroAddress, Network,
            SubaddressIndex,
        };
    }

    pub mod seed {
        pub use monero_serai::wallet::seed::{Language, Seed, SeedError};
    }
}

pub use monero_serai::{hash_to_scalar, Commitment, Protocol};
