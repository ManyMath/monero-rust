#![cfg_attr(docsrs, feature(doc_auto_cfg))]
//! Payment proofs for the Monero protocol.
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

mod base58;
mod monero_backend;
mod out_proof;
mod shared_key_derivations;
pub use out_proof::OutProof;

#[cfg(test)]
mod tests;
