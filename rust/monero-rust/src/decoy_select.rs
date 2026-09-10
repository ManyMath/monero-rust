//! Decoy selection for the send path, ported from `monero-wallet`.
//!
//! `monero-wallet` only selects decoys through its `ProvidesDecoys` interface,
//! whose futures must be `Send`. This prevents its use with single-threaded wasm RPC.
//! This implementation follows `monero-wallet/src/decoys.rs` at revision 946ec5f
//! bit-for-bit in its sampling logic while sourcing chain data from our RPC
//! client, so one code path serves native and wasm targets.

use std::collections::HashSet;

use curve25519_dalek::traits::IsIdentity as _;
use monero_oxide::{ringct::clsag::Decoys, BLOCK_TIME, COINBASE_LOCK_WINDOW, DEFAULT_LOCK_WINDOW};
use monero_wallet::ed25519::CompressedPoint;

/// The facts about the output being spent that decoy selection needs.
pub(crate) struct SpentOutput {
    /// The output's global RingCT index.
    pub index_on_blockchain: u64,
    /// The one-time output key.
    pub key: curve25519_dalek::EdwardsPoint,
    /// The output's commitment.
    pub commitment: curve25519_dalek::EdwardsPoint,
}
use rand_core::{CryptoRng, RngCore};
use rand_distr::{Distribution as _, Gamma};

use crate::monero_rpc::{Rpc, RpcConnection};

const RECENT_WINDOW: u64 = 15;
const BLOCKS_PER_YEAR: usize = (365 * 24 * 60 * 60) / BLOCK_TIME;
const TIP_APPLICATION: f64 = (DEFAULT_LOCK_WINDOW * BLOCK_TIME) as f64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet2_requests_seventy_five_candidates_for_a_sixteen_member_ring() {
        assert_eq!(requested_outputs_count(16), 75);
    }
}

fn requested_outputs_count(ring_len: u8) -> usize {
    ((usize::from(ring_len) * 3) / 2 + 1)
        + (COINBASE_LOCK_WINDOW.saturating_sub(DEFAULT_LOCK_WINDOW))
}

fn sample_candidates<R: RngCore + CryptoRng>(
    rng: &mut R,
    distribution: &[u64],
    highest_output_exclusive_bound: u64,
    per_second: f64,
    output_being_spent_index: u64,
    count: usize,
) -> Result<Vec<u64>, String> {
    let mut selected = Vec::with_capacity(count);
    selected.push(output_being_spent_index);
    let mut do_not_select = HashSet::from([output_being_spent_index]);
    let mut iters = 0usize;
    while selected.len() < count {
        iters += 1;
        if (iters == 10 * count)
            || (highest_output_exclusive_bound.saturating_sub(do_not_select.len() as u64)
                < (count - selected.len()) as u64)
        {
            return Err("hit decoy selection round limit".to_string());
        }

        let mut age = Gamma::<f64>::new(19.28, 1.0 / 1.61)
            .expect("constant Gamma distribution could no longer be created")
            .sample(rng)
            .exp();
        if age > TIP_APPLICATION {
            age -= TIP_APPLICATION;
        } else {
            age = (rng.next_u64() % (RECENT_WINDOW * (BLOCK_TIME as u64))) as f64;
        }

        let output_age = (age * per_second) as u64;
        if output_age >= highest_output_exclusive_bound {
            continue;
        }
        let target = highest_output_exclusive_bound - 1 - output_age;
        let block = distribution.partition_point(|cumulative| *cumulative < target);
        let previous = block
            .checked_sub(1)
            .map(|previous| distribution[previous])
            .unwrap_or(0);
        let outputs_in_block = distribution[block]
            .checked_sub(previous)
            .ok_or_else(|| "RPC returned non-monotonic distribution".to_string())?;
        if outputs_in_block == 0 {
            continue;
        }
        let index = previous + (rng.next_u64() % outputs_in_block);
        if do_not_select.insert(index) {
            selected.push(index);
        }
    }
    Ok(selected)
}

fn ring_from_candidates(
    input: &SpentOutput,
    ring_len: u8,
    secret_order: &[u64],
    request_order: &[u64],
    fetched: &[Option<[curve25519_dalek::EdwardsPoint; 2]>],
) -> Result<Decoys, String> {
    let mut ring = Vec::with_capacity(usize::from(ring_len));
    for index in secret_order {
        let position = request_order
            .binary_search(index)
            .map_err(|_| "requested decoy candidate was lost while sorting".to_string())?;
        let output = fetched
            .get(position)
            .ok_or_else(|| "node returned too few ring candidates".to_string())?;
        if *index == input.index_on_blockchain {
            let matches = output.map_or(false, |[key, commitment]| {
                (key == input.key) && (commitment == input.commitment)
            });
            if !matches {
                return Err("node presented different view of output we're trying to spend".into());
            }
            continue;
        }
        let Some([key, commitment]) = output else {
            continue;
        };
        if !(key.is_torsion_free() && commitment.is_torsion_free()) || key.is_identity() {
            continue;
        }
        let convert = |point: &curve25519_dalek::EdwardsPoint| {
            CompressedPoint::from(point.compress().to_bytes())
                .decompress()
                .ok_or_else(|| "ring member failed to decompress".to_string())
        };
        ring.push((*index, [convert(key)?, convert(commitment)?]));
        if ring.len() == usize::from(ring_len - 1) {
            break;
        }
    }
    if ring.len() != usize::from(ring_len - 1) {
        return Err("node returned too few usable ring candidates".to_string());
    }

    let convert = |point: &curve25519_dalek::EdwardsPoint| {
        CompressedPoint::from(point.compress().to_bytes())
            .decompress()
            .ok_or_else(|| "real spend failed to decompress".to_string())
    };
    ring.push((
        input.index_on_blockchain,
        [convert(&input.key)?, convert(&input.commitment)?],
    ));
    ring.sort_by_key(|(index, _)| *index);

    let mut offsets = Vec::with_capacity(ring.len());
    offsets.push(ring[0].0);
    for member in 1..ring.len() {
        offsets.push(ring[member].0 - ring[member - 1].0);
    }
    let signer_index = u8::try_from(ring.partition_point(|x| x.0 < input.index_on_blockchain))
        .map_err(|_| "ring signer index exceeded u8::MAX".to_string())?;
    Decoys::new(
        offsets,
        signer_index,
        ring.into_iter().map(|(_, points)| points).collect(),
    )
    .ok_or_else(|| "selected a syntactically-invalid set of decoys".to_string())
}

pub(crate) async fn select_decoys_for_outputs<R: RngCore + CryptoRng, C: RpcConnection>(
    rng: &mut R,
    rpc: &Rpc<C>,
    ring_len: u8,
    block_number: usize,
    inputs: &[SpentOutput],
) -> Result<Vec<Decoys>, String> {
    if ring_len == 0 {
        return Err("requesting a ring of length 0".to_string());
    }
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

    let count = requested_outputs_count(ring_len);
    let mut secret_orders = Vec::with_capacity(inputs.len());
    let mut request_orders = Vec::with_capacity(inputs.len());
    let mut flattened = Vec::with_capacity(inputs.len() * count);
    for input in inputs {
        let secret = sample_candidates(
            rng,
            &distribution,
            highest_output_exclusive_bound,
            per_second,
            input.index_on_blockchain,
            count,
        )?;
        let mut requested = secret.clone();
        requested.sort_unstable();
        flattened.extend_from_slice(&requested);
        secret_orders.push(secret);
        request_orders.push(requested);
    }

    let fetched = rpc
        .get_unlocked_outputs(&flattened, block_number)
        .await
        .map_err(|e| format!("Failed to fetch ring members: {:?}", e))?;
    let mut rings = Vec::with_capacity(inputs.len());
    for (position, input) in inputs.iter().enumerate() {
        let start = position * count;
        let end = start + count;
        rings.push(ring_from_candidates(
            input,
            ring_len,
            &secret_orders[position],
            &request_orders[position],
            fetched
                .get(start..end)
                .ok_or_else(|| "node returned too few ring candidates".to_string())?,
        )?);
    }
    Ok(rings)
}
