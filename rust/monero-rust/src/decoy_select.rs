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

pub(crate) struct RecordedRing {
    /// The members' global indexes, ascending.
    pub indexes: Vec<u64>,
    pub members: Option<Vec<[curve25519_dalek::EdwardsPoint; 2]>>,
}

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
    use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, Scalar};

    #[test]
    fn wallet2_requests_seventy_five_candidates_for_a_sixteen_member_ring() {
        assert_eq!(requested_outputs_count(16), 75);
    }

    fn blackball_fixture() -> (
        SpentOutput,
        Vec<u64>,
        Vec<Option<[curve25519_dalek::EdwardsPoint; 2]>>,
    ) {
        let indexes = vec![1, 2, 3, 4];
        let fetched = (1u64..=4)
            .map(|n| {
                Some([
                    Scalar::from(n) * ED25519_BASEPOINT_POINT,
                    Scalar::from(n + 10) * ED25519_BASEPOINT_POINT,
                ])
            })
            .collect::<Vec<_>>();
        let [key, commitment] = fetched[0].unwrap();
        (
            SpentOutput {
                index_on_blockchain: 1,
                key,
                commitment,
            },
            indexes,
            fetched,
        )
    }

    #[test]
    fn blackballed_candidates_are_skipped_when_alternatives_exist() {
        let (input, indexes, fetched) = blackball_fixture();
        let ring = ring_from_candidates(
            &input,
            3,
            &indexes,
            &indexes,
            &fetched,
            None,
            &HashSet::from([2]),
        )
        .unwrap();
        assert_eq!(absolute_indexes(&ring).unwrap(), vec![1, 3, 4]);
    }

    #[test]
    fn blackballed_candidates_are_a_last_resort_for_ring_size() {
        let (input, indexes, fetched) = blackball_fixture();
        let ring = ring_from_candidates(
            &input,
            3,
            &indexes,
            &indexes,
            &fetched,
            None,
            &HashSet::from([2, 3, 4]),
        )
        .unwrap();
        assert_eq!(absolute_indexes(&ring).unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn recorded_ring_without_member_keys_takes_them_from_the_reply() {
        let (input, indexes, fetched) = blackball_fixture();
        let recorded = |members| RecordedRing {
            indexes: vec![1, 2, 4],
            members,
        };
        let ring = ring_from_candidates(
            &input,
            3,
            &indexes,
            &indexes,
            &fetched,
            Some(&recorded(None)),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(absolute_indexes(&ring).unwrap(), vec![1, 2, 4]);
        assert_eq!(ring.signer_index(), 0);
        let reply_key: curve25519_dalek::EdwardsPoint = ring.ring()[2][0].into();
        assert_eq!(reply_key, fetched[3].unwrap()[0]);

        let mut members: Vec<_> = [0, 1, 3].iter().map(|i| fetched[*i].unwrap()).collect();
        members[1][0] = fetched[2].unwrap()[0];
        let error = ring_from_candidates(
            &input,
            3,
            &indexes,
            &indexes,
            &fetched,
            Some(&recorded(Some(members))),
            &HashSet::new(),
        )
        .unwrap_err();
        assert!(error.contains("different persisted ring member"), "{error}");

        let error = ring_from_candidates(
            &input,
            3,
            &indexes,
            &indexes,
            &fetched,
            Some(&RecordedRing {
                indexes: vec![2, 3, 4],
                members: None,
            }),
            &HashSet::new(),
        )
        .unwrap_err();
        assert!(
            error.contains("does not contain the real output"),
            "{error}"
        );
    }

    fn ring_at(indexes: &[u64]) -> Decoys {
        let mut previous = 0;
        let offsets = indexes
            .iter()
            .map(|index| {
                let offset = index - previous;
                previous = *index;
                offset
            })
            .collect::<Vec<_>>();
        let point = CompressedPoint::from(ED25519_BASEPOINT_POINT.compress().to_bytes())
            .decompress()
            .unwrap();
        Decoys::new(offsets, 0, vec![[point; 2]; indexes.len()]).unwrap()
    }

    #[test]
    fn wallet2_sanity_rejects_excessive_ring_overlap() {
        let ring = ring_at(&(7000..7016).collect::<Vec<_>>());
        assert!(!rings_are_sane(&[ring.clone(), ring], 10_000).unwrap());
    }

    #[test]
    fn wallet2_sanity_rejects_an_implausibly_old_median() {
        let ring = ring_at(&(1..=16).collect::<Vec<_>>());
        assert!(!rings_are_sane(&[ring], 10_000).unwrap());
    }

    fn sanity_fixture(
        old: u64,
    ) -> (
        CandidatePlan,
        SpentOutput,
        Vec<Option<[curve25519_dalek::EdwardsPoint; 2]>>,
    ) {
        let real = 15_000u64;
        let mut candidates = vec![real];
        candidates.extend(100..100 + old);
        candidates.extend(13_000..13_000 + (74 - old));
        let point = |n: u64| Scalar::from(n) * ED25519_BASEPOINT_POINT;
        let member = |index: u64| [point(index), point(index + 50_000)];
        let mut sorted = candidates.clone();
        sorted.sort_unstable();
        let fetched = sorted.iter().map(|index| Some(member(*index))).collect();
        let [key, commitment] = member(real);
        (
            CandidatePlan {
                ring_len: 16,
                block_number: 1_000,
                rct_outputs_available: 20_000,
                candidates: vec![candidates],
            },
            SpentOutput {
                index_on_blockchain: real,
                key,
                commitment,
            },
            fetched,
        )
    }

    #[test]
    fn sanity_failure_is_retried_from_the_same_reply() {
        use rand_core::SeedableRng;

        let (plan, input, fetched) = sanity_fixture(15);
        let first = ring_from_candidates(
            &input,
            16,
            &plan.candidates[0],
            &{
                let mut sorted = plan.candidates[0].clone();
                sorted.sort_unstable();
                sorted
            },
            &fetched,
            None,
            &HashSet::new(),
        )
        .unwrap();
        assert!(!rings_are_sane(&[first], plan.rct_outputs_available).unwrap());

        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(1);
        let rings = rings_from_reply(
            &mut rng,
            &plan,
            std::slice::from_ref(&input),
            &[None],
            &fetched,
            &HashSet::new(),
        )
        .unwrap();
        assert!(rings_are_sane(&rings, plan.rct_outputs_available).unwrap());
        let chosen = absolute_indexes(&rings[0]).unwrap();
        assert!(chosen.contains(&15_000));
        assert!(chosen
            .iter()
            .all(|index| plan.candidates[0].contains(index)));
    }

    #[test]
    fn sanity_failure_with_no_sane_choice_is_an_error_not_a_new_request() {
        use rand_core::SeedableRng;

        let (plan, input, fetched) = sanity_fixture(74);
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(1);
        let error = rings_from_reply(
            &mut rng,
            &plan,
            std::slice::from_ref(&input),
            &[None],
            &fetched,
            &HashSet::new(),
        )
        .unwrap_err();
        assert!(error.contains("sanity check"), "{error}");

        let recorded = RecordedRing {
            indexes: {
                let mut ring: Vec<u64> = (100..115).collect();
                ring.push(15_000);
                ring
            },
            members: None,
        };
        let error = rings_from_reply(
            &mut rng,
            &plan,
            std::slice::from_ref(&input),
            &[Some(recorded)],
            &fetched,
            &HashSet::new(),
        )
        .unwrap_err();
        assert!(error.contains("persisted rings were kept"), "{error}");
    }

    #[test]
    fn persisted_candidates_are_reused_whole_or_refused() {
        let (plan, input, _) = sanity_fixture(15);
        let candidates = &plan.candidates[0];
        check_persisted_candidates(candidates, 75, &input, None).unwrap();

        let error = check_persisted_candidates(&candidates[1..], 74, &input, None).unwrap_err();
        assert!(
            error.contains("does not contain the real output"),
            "{error}"
        );
        let error = check_persisted_candidates(candidates, 76, &input, None).unwrap_err();
        assert!(error.contains("wrong size"), "{error}");
        let outside = RecordedRing {
            indexes: vec![7, 15_000],
            members: None,
        };
        let error = check_persisted_candidates(candidates, 75, &input, Some(&outside)).unwrap_err();
        assert!(
            error.contains("does not contain the persisted ring"),
            "{error}"
        );
    }

    #[test]
    fn wallet2_sanity_accepts_distinct_recent_outputs() {
        let first = ring_at(&(7000..7016).collect::<Vec<_>>());
        let second = ring_at(&(7100..7116).collect::<Vec<_>>());
        assert!(rings_are_sane(&[first, second], 10_000).unwrap());
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
    initial: &[u64],
    count: usize,
    blackballed: &HashSet<u64>,
) -> Result<Vec<u64>, String> {
    let mut selected = Vec::with_capacity(count);
    let mut do_not_select = HashSet::new();
    for index in initial {
        if do_not_select.insert(*index) {
            selected.push(*index);
        }
    }
    if selected.len() > count {
        return Err("persisted ring is larger than the candidate request".to_string());
    }
    let mut iters = 0usize;
    let mut allow_blackballed = false;
    while selected.len() < count {
        iters += 1;
        if (iters == 10 * count)
            || (highest_output_exclusive_bound.saturating_sub(do_not_select.len() as u64)
                < (count - selected.len()) as u64)
        {
            if allow_blackballed {
                return Err("hit decoy selection round limit".to_string());
            }
            allow_blackballed = true;
            iters = 0;
            do_not_select.clear();
            do_not_select.extend(selected.iter().copied());
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
        if do_not_select.insert(index) && (allow_blackballed || !blackballed.contains(&index)) {
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
    existing: Option<&RecordedRing>,
    blackballed: &HashSet<u64>,
) -> Result<Decoys, String> {
    if let Some(existing) = existing {
        let indexes = &existing.indexes;
        if indexes.len() != usize::from(ring_len) {
            return Err("persisted ring has the wrong size for this protocol".to_string());
        }
        if !indexes.windows(2).all(|pair| pair[0] < pair[1])
            || existing
                .members
                .as_ref()
                .is_some_and(|members| members.len() != indexes.len())
        {
            return Err("persisted ring is syntactically invalid".to_string());
        }
        let Ok(signer) = indexes.binary_search(&input.index_on_blockchain) else {
            return Err("persisted ring does not contain the real output".to_string());
        };
        let convert = |point: &curve25519_dalek::EdwardsPoint| {
            CompressedPoint::from(point.compress().to_bytes())
                .decompress()
                .ok_or_else(|| "persisted ring member failed to decompress".to_string())
        };
        let mut ring = Vec::with_capacity(indexes.len());
        for (member, index) in indexes.iter().enumerate() {
            let position = request_order
                .binary_search(index)
                .map_err(|_| "persisted ring member was not requested".to_string())?;
            let Some([key, commitment]) = fetched.get(position).copied().flatten() else {
                return Err("persisted ring member is no longer available".to_string());
            };
            if member == signer {
                if key != input.key || commitment != input.commitment {
                    return Err(
                        "node presented different view of output we're trying to spend".into(),
                    );
                }
            } else if let Some(members) = &existing.members {
                if [key, commitment] != members[member] {
                    return Err("node presented a different persisted ring member".to_string());
                }
            }
            ring.push([convert(&key)?, convert(&commitment)?]);
        }
        let mut offsets = Vec::with_capacity(indexes.len());
        offsets.push(indexes[0]);
        for member in 1..indexes.len() {
            offsets.push(indexes[member] - indexes[member - 1]);
        }
        let signer_index =
            u8::try_from(signer).map_err(|_| "ring signer index exceeded u8::MAX".to_string())?;
        return Decoys::new(offsets, signer_index, ring)
            .ok_or_else(|| "persisted ring is syntactically invalid".to_string());
    }

    let mut ring = Vec::with_capacity(usize::from(ring_len));
    for allow_blackballed in [false, true] {
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
                    return Err(
                        "node presented different view of output we're trying to spend".into(),
                    );
                }
                continue;
            }
            if blackballed.contains(index) != allow_blackballed {
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

fn absolute_indexes(decoys: &Decoys) -> Result<Vec<u64>, String> {
    let mut absolute = Vec::with_capacity(decoys.offsets().len());
    let mut index = 0u64;
    for offset in decoys.offsets() {
        index = index
            .checked_add(*offset)
            .ok_or_else(|| "persisted ring offsets overflow".to_string())?;
        absolute.push(index);
    }
    Ok(absolute)
}

fn rings_are_sane(rings: &[Decoys], rct_outputs_available: u64) -> Result<bool, String> {
    let mut unique = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for ring in rings {
        let indexes = absolute_indexes(ring)?;
        total = total
            .checked_add(indexes.len())
            .ok_or_else(|| "ring member count overflowed".to_string())?;
        unique.extend(indexes);
    }
    if total <= 10 || rct_outputs_available < 10_000 {
        return Ok(true);
    }
    if unique.len() < total.saturating_mul(8) / 10 {
        return Ok(false);
    }

    let indexes = unique.into_iter().collect::<Vec<_>>();
    let middle = indexes.len() / 2;
    let median = if indexes.len() % 2 == 0 {
        let lower = indexes[middle - 1];
        let upper = indexes[middle];
        (lower / 2) + (upper / 2) + ((lower % 2) + (upper % 2)) / 2
    } else {
        indexes[middle]
    };
    Ok(median >= rct_outputs_available.saturating_mul(6) / 10)
}

pub(crate) struct CandidatePlan {
    ring_len: u8,
    block_number: usize,
    rct_outputs_available: u64,
    pub candidates: Vec<Vec<u64>>,
}

/// Decide every input's candidates, keeping recorded sets. Only chain-wide
/// data is fetched, so the plan can be recorded before [`choose_rings`]
/// sends it.
pub(crate) async fn plan_candidates<R: RngCore + CryptoRng, C: RpcConnection>(
    rng: &mut R,
    rpc: &Rpc<C>,
    ring_len: u8,
    block_number: usize,
    inputs: &[SpentOutput],
    existing_rings: &[Option<RecordedRing>],
    existing_candidates: &[Option<Vec<u64>>],
    blackballed: &HashSet<u64>,
) -> Result<CandidatePlan, String> {
    if ring_len == 0 {
        return Err("requesting a ring of length 0".to_string());
    }
    if inputs.len() != existing_rings.len() || inputs.len() != existing_candidates.len() {
        return Err("persisted ring count does not match input count".to_string());
    }
    if inputs.iter().zip(existing_rings).any(|(input, ring)| {
        ring.as_ref()
            .is_some_and(|ring| !ring.indexes.contains(&input.index_on_blockchain))
    }) {
        return Err("persisted ring does not contain the real output".to_string());
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
    // wallet2 checks against the total. The unlocked bound also keeps outputs
    // parked in the locked tip from satisfying the check.
    if inputs
        .iter()
        .any(|input| input.index_on_blockchain >= highest_output_exclusive_bound)
    {
        return Err(
            "node reports fewer unlocked RingCT outputs than the index of an output being spent"
                .to_string(),
        );
    }

    // Determine the outputs per second
    let per_second = {
        let blocks = distribution.len().min(BLOCKS_PER_YEAR);
        let initial = distribution[distribution.len().saturating_sub(blocks + 1)];
        let outputs = distribution[distribution.len() - 1].saturating_sub(initial);
        (outputs as f64) / ((blocks * BLOCK_TIME) as f64)
    };

    let count = requested_outputs_count(ring_len);
    let rct_outputs_available = *distribution
        .last()
        .ok_or_else(|| "RPC returned an empty output distribution".to_string())?;

    let mut candidates = Vec::with_capacity(inputs.len());
    for ((input, ring), persisted) in inputs.iter().zip(existing_rings).zip(existing_candidates) {
        if let Some(persisted) = persisted {
            check_persisted_candidates(persisted, count, input, ring.as_ref())?;
            candidates.push(persisted.clone());
            continue;
        }
        let initial = match ring {
            Some(ring) => ring.indexes.clone(),
            None => vec![input.index_on_blockchain],
        };
        candidates.push(sample_candidates(
            rng,
            &distribution,
            highest_output_exclusive_bound,
            per_second,
            &initial,
            count,
            blackballed,
        )?);
    }
    Ok(CandidatePlan {
        ring_len,
        block_number,
        rct_outputs_available,
        candidates,
    })
}

fn check_persisted_candidates(
    persisted: &[u64],
    count: usize,
    input: &SpentOutput,
    ring: Option<&RecordedRing>,
) -> Result<(), String> {
    if persisted.len() != count {
        return Err("persisted candidate set has the wrong size for this protocol".to_string());
    }
    let distinct: HashSet<u64> = persisted.iter().copied().collect();
    if distinct.len() != persisted.len() {
        return Err("persisted candidate set repeats an output".to_string());
    }
    if !distinct.contains(&input.index_on_blockchain) {
        return Err("persisted candidate set does not contain the real output".to_string());
    }
    if ring.is_some_and(|ring| !ring.indexes.iter().all(|index| distinct.contains(index))) {
        return Err("persisted candidate set does not contain the persisted ring".to_string());
    }
    Ok(())
}

/// Request the planned candidates, once, with the same batched, sorted
/// binary request shape as wallet2, and choose every input's ring from the
/// reply.
pub(crate) async fn choose_rings<R: RngCore + CryptoRng, C: RpcConnection>(
    rng: &mut R,
    rpc: &Rpc<C>,
    plan: &CandidatePlan,
    inputs: &[SpentOutput],
    existing_rings: &[Option<RecordedRing>],
    blackballed: &HashSet<u64>,
) -> Result<Vec<Decoys>, String> {
    let mut flattened = Vec::with_capacity(plan.candidates.iter().map(Vec::len).sum());
    for candidates in &plan.candidates {
        let mut requested = candidates.clone();
        requested.sort_unstable();
        flattened.extend_from_slice(&requested);
    }
    let fetched = rpc
        .get_unlocked_outputs(&flattened, plan.block_number)
        .await
        .map_err(|e| format!("Failed to fetch ring members: {:?}", e))?;
    rings_from_reply(rng, plan, inputs, existing_rings, &fetched, blackballed)
}

/// Choose every input's ring from one reply, retrying a failed sanity check
/// on that reply. wallet2 requests new candidates instead
/// (wallet2.cpp:9436-9457), which the daemon can intersect.
fn rings_from_reply<R: RngCore + CryptoRng>(
    rng: &mut R,
    plan: &CandidatePlan,
    inputs: &[SpentOutput],
    existing_rings: &[Option<RecordedRing>],
    fetched: &[Option<[curve25519_dalek::EdwardsPoint; 2]>],
    blackballed: &HashSet<u64>,
) -> Result<Vec<Decoys>, String> {
    let mut request_orders = Vec::with_capacity(inputs.len());
    let mut starts = Vec::with_capacity(inputs.len());
    let mut start = 0;
    for candidates in &plan.candidates {
        let mut requested = candidates.clone();
        requested.sort_unstable();
        request_orders.push(requested);
        starts.push(start);
        start += candidates.len();
    }
    if fetched.len() != start {
        return Err("node returned too few ring candidates".to_string());
    }

    let mut secret_orders = plan.candidates.clone();
    for attempt in 0..3 {
        if attempt > 0 {
            let mut reordered = false;
            for (order, existing) in secret_orders.iter_mut().zip(existing_rings) {
                if existing.is_none() {
                    for last in (1..order.len()).rev() {
                        order.swap(last, (rng.next_u64() % (last as u64 + 1)) as usize);
                    }
                    reordered = true;
                }
            }
            if !reordered {
                break;
            }
        }
        let mut rings = Vec::with_capacity(inputs.len());
        for (position, input) in inputs.iter().enumerate() {
            let chunk = starts[position]..starts[position] + request_orders[position].len();
            rings.push(ring_from_candidates(
                input,
                plan.ring_len,
                &secret_orders[position],
                &request_orders[position],
                &fetched[chunk],
                existing_rings[position].as_ref(),
                blackballed,
            )?);
        }
        if rings_are_sane(&rings, plan.rct_outputs_available)? {
            return Ok(rings);
        }
    }
    let detail = if existing_rings.iter().any(Option::is_some) {
        "; persisted rings were kept"
    } else {
        ""
    };
    Err(format!(
        "decoy rings failed wallet2's transaction sanity check with every choice tried from the \
         candidates already requested; no further candidates were requested{detail}"
    ))
}
