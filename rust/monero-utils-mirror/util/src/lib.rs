#![cfg_attr(docsrs, feature(doc_auto_cfg))]
//! Additional utility functions for monero-wallet.
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

mod monero_backend;
pub use monero_backend::*;

/// Seed creation and parsing functionality.
pub mod seed;
/// Payment proofs.
pub use monero_payment_proofs::*;
