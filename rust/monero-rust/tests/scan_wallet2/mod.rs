#![allow(dead_code)]

use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE as G, edwards::EdwardsPoint, scalar::Scalar,
};
use monero_oxide::{
    block::{Block, BlockHeader},
    ed25519::{Commitment, CompressedPoint, Scalar as OxideScalar},
    ringct::{
        bulletproofs::Bulletproof, clsag::Clsag, EncryptedAmount, RctBase, RctProofs, RctPrunable,
    },
    transaction::{Input, NotPruned, Output, Timelock, Transaction, TransactionPrefix},
};
use monero_rust::monero_rpc::{
    BlockCompleteEntry, BlockOutputIndices, GetBlocksFastResponse, TxOutputIndices,
};
use monero_rust::scanner::BlockScanResult;
use monero_rust::{derive_keys, derive_subaddress, Network, WalletState};
use sha3::{Digest, Keccak256};

/// Wallet under test; regtest uses mainnet prefixes.
pub const SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
pub const OTHER_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";

pub const XMR: u64 = 1_000_000_000_000;

fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub fn view_only_string(seed: &str) -> String {
    let keys = derive_keys(seed, Network::Mainnet, "").unwrap();
    format!(
        "viewonly:{}:{}",
        hex::encode(*keys.secret_view_key),
        hex::encode(keys.public_spend_key)
    )
}

#[derive(Clone, Copy)]
pub struct Recipient {
    pub spend: EdwardsPoint,
    pub view: EdwardsPoint,
    pub is_subaddress: bool,
}

impl Recipient {
    pub fn of(seed: &str, account: u32, index: u32) -> Self {
        let address = derive_subaddress(seed, Network::Mainnet, account, index, "").unwrap();
        Recipient {
            spend: address.spend().into(),
            view: address.view().into(),
            is_subaddress: address.is_subaddress(),
        }
    }
}

#[derive(Clone)]
pub struct SentOutput {
    pub key: [u8; 32],
    pub view_tag: u8,
    pub encrypted_amount: [u8; 8],
    pub commitment: Commitment,
    pub tx_pubkey: [u8; 32],
    pub amount: u64,
}

pub fn send_output(recipient: &Recipient, r: Scalar, o: usize, amount: u64) -> SentOutput {
    let derivation = (r * recipient.view).mul_by_cofactor().compress().to_bytes();
    let mut indexed = derivation.to_vec();
    write_varint(o as u64, &mut indexed);

    let view_tag = keccak(&[b"view_tag".as_slice(), &indexed].concat())[0];
    let shared = Scalar::from_bytes_mod_order(keccak(&indexed));
    let key = (&shared * G + recipient.spend).compress().to_bytes();

    let amount_mask = keccak(&[b"amount".as_slice(), &shared.to_bytes()].concat());
    let mut encrypted_amount = amount.to_le_bytes();
    for (byte, mask) in encrypted_amount.iter_mut().zip(amount_mask) {
        *byte ^= mask;
    }
    let mask = Scalar::from_bytes_mod_order(keccak(
        &[b"commitment_mask".as_slice(), &shared.to_bytes()].concat(),
    ));

    let tx_pubkey = if recipient.is_subaddress {
        (r * recipient.spend).compress().to_bytes()
    } else {
        (&r * G).compress().to_bytes()
    };

    SentOutput {
        key,
        view_tag,
        encrypted_amount,
        commitment: Commitment::new(OxideScalar::from(mask), amount),
        tx_pubkey,
        amount,
    }
}

pub fn extra_with_pubkeys(pubkeys: &[[u8; 32]]) -> Vec<u8> {
    let mut extra = Vec::new();
    for key in pubkeys {
        extra.push(1);
        extra.extend_from_slice(key);
    }
    extra
}

/// A RingCT transaction with proofs the scanner does not verify.
pub fn ringct_tx(
    outputs: &[SentOutput],
    extra: Vec<u8>,
    timelock: Timelock,
    salt: u64,
) -> Transaction<NotPruned> {
    let key_image = (&Scalar::from(salt + 7) * G).compress().to_bytes();
    ringct_tx_spending(key_image, outputs, extra, timelock)
}

pub fn ringct_tx_spending(
    key_image: [u8; 32],
    outputs: &[SentOutput],
    extra: Vec<u8>,
    timelock: Timelock,
) -> Transaction<NotPruned> {
    let filler = CompressedPoint::from(key_image);
    let mut rng = rand::thread_rng();
    let commitments: Vec<Commitment> = outputs.iter().map(|o| o.commitment.clone()).collect();
    Transaction::V2 {
        prefix: TransactionPrefix {
            additional_timelock: timelock,
            inputs: vec![Input::ToKey {
                amount: None,
                key_offsets: vec![1; 16],
                key_image: filler,
            }],
            outputs: outputs
                .iter()
                .map(|o| Output {
                    amount: None,
                    key: CompressedPoint::from(o.key),
                    view_tag: Some(o.view_tag),
                })
                .collect(),
            extra,
        },
        proofs: Some(RctProofs {
            base: RctBase {
                fee: 30_000_000,
                pseudo_outs: vec![],
                encrypted_amounts: outputs
                    .iter()
                    .map(|o| EncryptedAmount::Compact {
                        amount: o.encrypted_amount,
                    })
                    .collect(),
                commitments: outputs
                    .iter()
                    .map(|o| o.commitment.commit().compress())
                    .collect(),
            },
            prunable: RctPrunable::Clsag {
                clsags: vec![Clsag {
                    D: filler,
                    s: vec![OxideScalar::from(Scalar::ONE); 16],
                    c1: OxideScalar::from(Scalar::ONE),
                }],
                pseudo_outs: vec![filler],
                bulletproof: Bulletproof::prove_plus(&mut rng, commitments).unwrap(),
            },
        }),
    }
}

#[derive(Clone)]
pub struct CoinbaseOutput {
    pub key: [u8; 32],
    pub view_tag: u8,
    pub amount: u64,
}

impl From<&SentOutput> for CoinbaseOutput {
    fn from(o: &SentOutput) -> Self {
        CoinbaseOutput {
            key: o.key,
            view_tag: o.view_tag,
            amount: o.amount,
        }
    }
}

pub fn burn_coinbase_output(amount: u64) -> CoinbaseOutput {
    CoinbaseOutput {
        key: (&Scalar::from(424_242u64) * G).compress().to_bytes(),
        view_tag: 0,
        amount,
    }
}

pub fn miner_tx(height: u64, outputs: &[CoinbaseOutput], extra: Vec<u8>) -> Transaction<NotPruned> {
    Transaction::V2 {
        prefix: TransactionPrefix {
            additional_timelock: Timelock::Block((height + 60) as usize),
            inputs: vec![Input::Gen(height as usize)],
            outputs: outputs
                .iter()
                .map(|o| Output {
                    amount: Some(o.amount),
                    key: CompressedPoint::from(o.key),
                    view_tag: Some(o.view_tag),
                })
                .collect(),
            extra,
        },
        proofs: None,
    }
}

pub struct FakeChain {
    pub start_height: u64,
    entries: Vec<BlockCompleteEntry>,
    indices: Vec<BlockOutputIndices>,
    previous: [u8; 32],
    next_ringct_index: u64,
    hashes: Vec<[u8; 32]>,
    ringct_after: Vec<u64>,
    first_ringct_index: u64,
    filler_amount: u64,
}

impl FakeChain {
    pub fn new(start_height: u64, first_ringct_index: u64) -> Self {
        FakeChain {
            start_height,
            entries: Vec::new(),
            indices: Vec::new(),
            previous: [0x11; 32],
            next_ringct_index: first_ringct_index,
            hashes: Vec::new(),
            ringct_after: Vec::new(),
            first_ringct_index,
            filler_amount: 600_000_000_000,
        }
    }

    pub fn fork_at(&self, height: u64) -> FakeChain {
        let keep = (height - self.start_height) as usize;
        assert!(keep >= 1 && keep <= self.entries.len());
        FakeChain {
            start_height: self.start_height,
            entries: self.entries[..keep].to_vec(),
            indices: self.indices[..keep].to_vec(),
            previous: self.hashes[keep - 1],
            next_ringct_index: self.ringct_after[keep - 1],
            hashes: self.hashes[..keep].to_vec(),
            ringct_after: self.ringct_after[..keep].to_vec(),
            first_ringct_index: self.first_ringct_index,
            filler_amount: self.filler_amount + 1,
        }
    }

    pub fn hash_hex(&self, height: u64) -> String {
        hex::encode(self.hashes[(height - self.start_height) as usize])
    }

    /// Where monerod starts for a history and `start_height == 0`: the highest
    /// history block on this chain.
    pub fn common_start(&self, history: &[(u64, String)]) -> u64 {
        history
            .iter()
            .filter(|(height, hash)| {
                *height >= self.start_height
                    && *height < self.next_height()
                    && self.hash_hex(*height) == *hash
            })
            .map(|(height, _)| *height)
            .max()
            .unwrap_or(self.start_height)
    }

    pub fn tip_height(&self) -> u64 {
        self.start_height + self.entries.len() as u64 - 1
    }

    pub fn next_height(&self) -> u64 {
        self.start_height + self.entries.len() as u64
    }

    pub fn push(&mut self, miner: Transaction<NotPruned>, txs: Vec<Transaction<NotPruned>>) {
        let height = self.next_height();
        let mut tx_indices = Vec::new();
        for tx in std::iter::once(&miner).chain(txs.iter()) {
            let count = tx.prefix().outputs.len() as u64;
            tx_indices.push(TxOutputIndices {
                indices: (self.next_ringct_index..self.next_ringct_index + count).collect(),
            });
            self.next_ringct_index += count;
        }
        let block = Block::new(
            BlockHeader {
                hardfork_version: 16,
                hardfork_signal: 16,
                timestamp: 1_700_000_000 + height * 120,
                previous: self.previous,
                nonce: 0,
            },
            miner,
            txs.iter().map(|tx| tx.hash()).collect(),
        )
        .expect("synthetic block");
        assert_eq!(block.number() as u64, height);
        self.previous = block.hash();
        self.hashes.push(self.previous);
        self.ringct_after.push(self.next_ringct_index);
        self.entries.push(BlockCompleteEntry {
            block: block.serialize(),
            txs: txs.iter().map(|tx| tx.serialize()).collect(),
            pruned: false,
            block_weight: 0,
        });
        self.indices.push(BlockOutputIndices {
            indices: tx_indices,
        });
    }

    pub fn push_empty(&mut self, count: usize) {
        for _ in 0..count {
            let height = self.next_height();
            self.push(
                miner_tx(
                    height,
                    &[burn_coinbase_output(self.filler_amount)],
                    vec![1; 33],
                ),
                vec![],
            );
        }
    }

    pub fn response_from(&self, from: u64) -> GetBlocksFastResponse {
        self.response_range(from, self.next_height())
    }

    /// Pruned blocks, with a real prunable hash or the zero one monerod sends.
    pub fn pruned_response_from(
        &self,
        from: u64,
        with_prunable_hash: bool,
    ) -> GetBlocksFastResponse {
        let mut response = self.response_from(from);
        for entry in &mut response.blocks {
            entry.pruned = true;
            for blob in &mut entry.txs {
                let tx = Transaction::<NotPruned>::read(&mut blob.as_slice()).unwrap();
                let prunable_hash = if with_prunable_hash {
                    tx.prunable_hash().unwrap()
                } else {
                    [0u8; 32]
                };
                let (pruned, _) = tx.pruned_with_prunable();
                *blob = pruned.serialize();
                blob.extend_from_slice(&prunable_hash);
            }
        }
        response
    }

    pub fn response_range(&self, from: u64, to: u64) -> GetBlocksFastResponse {
        let a = (from - self.start_height) as usize;
        let b = (to - self.start_height) as usize;
        GetBlocksFastResponse {
            status: "OK".to_string(),
            blocks: self.entries[a..b].to_vec(),
            start_height: from,
            current_height: self.next_height(),
            output_indices: self.indices[a..b].to_vec(),
            untrusted: false,
            credits: 0,
            top_hash: String::new(),
            daemon_time: 0,
        }
    }
}

/// Reference composition: outputs, spends at the block height, block hash.
pub fn apply_results(state: &mut WalletState, results: &[BlockScanResult]) {
    for result in results {
        state.add_outputs(result.outputs.clone());
        state.mark_spent_detecting_conflicts(
            &result.spent_key_images,
            &result.spent_key_image_tx_hashes,
            result.block_height,
        );
        state.record_block_hash(result.block_height, result.block_hash.clone());
        state.current_height = result.block_height + 1;
        state.daemon_height = result.daemon_height;
    }
}

pub fn total_balance(state: &WalletState, height: u64) -> u64 {
    let balance = state.balance_at_height(height);
    balance.confirmed + balance.unconfirmed + balance.pending_spend
}
