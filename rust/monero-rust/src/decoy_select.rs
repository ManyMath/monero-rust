//! Decoy selection for the send path, ported from `monero-wallet`.
//!
//! `monero-wallet` only selects decoys through its `ProvidesDecoys` interface,
//! whose futures must be `Send`. This prevents its use with single-threaded wasm RPC.
//! This implementation follows `monero-wallet/src/decoys.rs` at revision 946ec5f
//! bit-for-bit in its sampling logic while sourcing chain data from our RPC
//! client, so one code path serves native and wasm targets.

// Consumed by the send-path port over the next commits.
#![allow(dead_code)]

use std::collections::HashSet;

use curve25519_dalek::traits::IsIdentity as _;
use monero_oxide::{
    ringct::clsag::Decoys, BLOCK_TIME, COINBASE_LOCK_WINDOW, DEFAULT_LOCK_WINDOW,
};
use monero_wallet::{ed25519::CompressedPoint, WalletOutput};
use rand_core::{CryptoRng, RngCore};
use rand_distr::{Distribution as _, Gamma};

use crate::monero_backend::rpc::{Rpc, RpcConnection};

const RECENT_WINDOW: u64 = 15;
const BLOCKS_PER_YEAR: usize = (365 * 24 * 60 * 60) / BLOCK_TIME;
const TIP_APPLICATION: f64 = (DEFAULT_LOCK_WINDOW * BLOCK_TIME) as f64;

async fn select_n<R: RngCore + CryptoRng, C: RpcConnection>(
    rng: &mut R,
    rpc: &Rpc<C>,
    block_number: usize,
    output_being_spent: &WalletOutput,
    ring_len: u8,
) -> Result<Vec<(u64, [CompressedPoint; 2])>, String> {
    if block_number <= DEFAULT_LOCK_WINDOW {
        return Err("not enough blocks to select decoys".to_string());
    }
    let latest_block_number = rpc
        .get_height()
        .await
        .map_err(|e| format!("Failed to fetch height: {:?}", e))?
        .saturating_sub(1);
    if block_number > latest_block_number {
        return Err("decoys being requested from blocks this node doesn't have".to_string());
    }

    // Get the cumulative RingCT output distribution
    let distribution = rpc
        .get_output_distribution(0, block_number)
        .await
        .map_err(|e| format!("Failed to fetch output distribution: {:?}", e))?;
    if distribution.len() < DEFAULT_LOCK_WINDOW {
        return Err("not enough blocks to select decoys".to_string());
    }
    let highest_output_exclusive_bound = distribution[distribution.len() - DEFAULT_LOCK_WINDOW];
    // This assumes that each miner TX had one output (as sane) and checks we
    // have sufficient outputs even when excluding them (due to their own
    // timelock requirements)
    if highest_output_exclusive_bound.saturating_sub(COINBASE_LOCK_WINDOW as u64)
        < u64::from(ring_len)
    {
        return Err("not enough decoy candidates".to_string());
    }

    // Determine the outputs per second
    let per_second = {
        let blocks = distribution.len().min(BLOCKS_PER_YEAR);
        let initial = distribution[distribution.len().saturating_sub(blocks + 1)];
        let outputs = distribution[distribution.len() - 1].saturating_sub(initial);
        (outputs as f64) / ((blocks * BLOCK_TIME) as f64)
    };

    let output_being_spent_index = output_being_spent.index_on_blockchain();

    // Don't select the real output
    let mut do_not_select = HashSet::new();
    do_not_select.insert(output_being_spent_index);

    let decoy_count = usize::from(ring_len - 1);
    let mut res = Vec::with_capacity(decoy_count);

    let mut first_iter = true;
    let mut iters = 0;
    // Iterates until we have enough decoys
    while res.len() != decoy_count {
        let remaining = decoy_count - res.len();
        let mut candidates = Vec::with_capacity(remaining);
        while candidates.len() != remaining {
            iters += 1;
            // Ensure this isn't infinitely looping
            if (iters == 10 * usize::from(ring_len))
                || ((highest_output_exclusive_bound - do_not_select.len() as u64)
                    < (remaining - candidates.len()) as u64)
            {
                return Err("hit decoy selection round limit".to_string());
            }

            // Use a gamma distribution, as Monero does
            let mut age = Gamma::<f64>::new(19.28, 1.0 / 1.61)
                .expect("constant Gamma distribution could no longer be created")
                .sample(rng)
                .exp();
            if age > TIP_APPLICATION {
                age -= TIP_APPLICATION;
            } else {
                age = (rng.next_u64() % (RECENT_WINDOW * (BLOCK_TIME as u64))) as f64;
            }

            let o = (age * per_second) as u64;
            if o < highest_output_exclusive_bound {
                // Find which block this points to
                let i = distribution
                    .partition_point(|s| *s < (highest_output_exclusive_bound - 1 - o));
                let prev_block = i.checked_sub(1);
                let prev_outputs = prev_block.map(|prev_block| distribution[prev_block]).unwrap_or(0);
                let n = distribution[i]
                    .checked_sub(prev_outputs)
                    .ok_or_else(|| "RPC returned non-monotonic distribution".to_string())?;
                if n != 0 {
                    // Select an output from within this block
                    let o = prev_outputs + (rng.next_u64() % n);
                    if !do_not_select.contains(&o) {
                        candidates.push(o);
                        // This output will either be used or is unusable; in
                        // either case, we should not try it again
                        do_not_select.insert(o);
                    }
                }
            }
        }

        // If this is the first time we're requesting these outputs, include
        // the real one as well. Prevents the node we're connected to from
        // having a list of known decoys and then seeing a TX which uses all
        // of them, with one additional output (the true spend).
        let real_index = first_iter.then(|| {
            first_iter = false;

            candidates.push(output_being_spent_index);
            // Sort candidates so the real spends aren't the ones at the end
            candidates.sort_unstable();
            candidates
                .binary_search(&output_being_spent_index)
                .expect("selected a ring which didn't include the real spend")
        });

        let unlocked = rpc
            .get_unlocked_outputs(&candidates, block_number)
            .await
            .map_err(|e| format!("Failed to fetch ring members: {:?}", e))?;

        for (i, output) in unlocked.into_iter().enumerate() {
            if real_index == Some(i) {
                let matches = output.map_or(false, |[key, commitment]| {
                    let expected_key: curve25519_dalek::EdwardsPoint =
                        output_being_spent.key().into();
                    let expected_commitment: curve25519_dalek::EdwardsPoint =
                        output_being_spent.commitment().commit().into();
                    (key == expected_key) && (commitment == expected_commitment)
                });
                if !matches {
                    return Err(
                        "node presented different view of output we're trying to spend"
                            .to_string(),
                    );
                }
                continue;
            }

            // If this is an unlocked output, push it to the result
            if let Some([key, commitment]) = output {
                // Unless torsion is present
                if !(key.is_torsion_free() && commitment.is_torsion_free()) {
                    continue;
                }
                // Or the key is the identity, which cannot be signed for as a
                // real ring member
                if key.is_identity() {
                    continue;
                }
                res.push((
                    candidates[i],
                    [
                        CompressedPoint::from(key.compress().to_bytes()),
                        CompressedPoint::from(commitment.compress().to_bytes()),
                    ],
                ));
            }
        }
    }

    Ok(res)
}

/// Select decoys for spending `input`, mirroring `monero-wallet`'s
/// `select_decoys`.
///
/// `input.index_on_blockchain()` must be the output's true global RingCT
/// index (see `oxide_output_bytes::wallet_output_with_index_on_blockchain`).
pub(crate) async fn select_decoys<R: RngCore + CryptoRng, C: RpcConnection>(
    rng: &mut R,
    rpc: &Rpc<C>,
    ring_len: u8,
    block_number: usize,
    input: &WalletOutput,
) -> Result<Decoys, String> {
    if ring_len == 0 {
        return Err("requesting a ring of length 0".to_string());
    }

    // Select all decoys for this transaction, assuming we generate a sane
    // transaction
    let decoys = select_n(rng, rpc, block_number, input, ring_len).await?;

    // Form the complete ring
    let mut ring = decoys
        .into_iter()
        .map(|(index, [key, commitment])| {
            let key = key
                .decompress()
                .ok_or_else(|| "ring member key failed to decompress".to_string())?;
            let commitment = commitment
                .decompress()
                .ok_or_else(|| "ring member commitment failed to decompress".to_string())?;
            Ok((index, [key, commitment]))
        })
        .collect::<Result<Vec<_>, String>>()?;
    ring.push((
        input.index_on_blockchain(),
        [input.key(), input.commitment().commit()],
    ));
    ring.sort_by_key(|(index_on_blockchain, _value)| *index_on_blockchain);

    // We need to convert our positional indexes to offset indexes
    let mut offsets = Vec::with_capacity(ring.len());
    offsets.push(ring[0].0);
    for m in 1..ring.len() {
        offsets.push(ring[m].0 - ring[m - 1].0);
    }

    let signer_index = u8::try_from(
        ring.partition_point(|x| x.0 < input.index_on_blockchain()),
    )
    .map_err(|_| "ring of size <= u8::MAX had an index exceeding u8::MAX".to_string())?;

    Decoys::new(
        offsets,
        signer_index,
        ring.into_iter().map(|output| output.1).collect(),
    )
    .ok_or_else(|| "selected a syntactically-invalid set of decoys".to_string())
}
