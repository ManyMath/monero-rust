//! Compatibility facade for the current Monero protocol backend.
//!
//! This module intentionally re-exports the small backend surface used by
//! `monero-rust` so a future `monero-oxide` migration can happen behind one
//! boundary instead of through scattered direct imports.

pub mod block {
    pub use monero_serai::block::*;
}

pub mod ringct {
    pub use monero_serai::ringct::*;
}

pub mod rpc {
    pub use monero_serai::rpc::*;
}

pub mod transaction {
    pub use monero_serai::transaction::*;
}

pub mod wallet {
    pub use monero_serai::wallet::*;

    pub mod address {
        pub use monero_serai::wallet::address::*;
    }

    pub mod seed {
        pub use monero_serai::wallet::seed::*;
    }
}

pub use monero_serai::*;
