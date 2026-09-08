//! Synthetic wallets and unsigned transactions for the offline signing tests.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::native::OfflineSignResult;
use monero_rust::unsigned_tx::{output_with_decoys, UnsignedChange, UnsignedTransaction};
use monero_wallet::{
    address::{MoneroAddress, Network, SubaddressIndex},
    ed25519::{Commitment, Point, Scalar as OxideScalar},
    interface::FeeRate,
    ringct::{clsag::Decoys, RctType},
    OutputWithDecoys, ViewPair,
};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

pub const INPUT_AMOUNT: u64 = 1_000_000_000_000;
pub const PAYMENT_AMOUNT: u64 = 800_000_000_000;
pub const FEE: u64 = 100_000_000;
pub const CHANGE_AMOUNT: u64 = INPUT_AMOUNT - PAYMENT_AMOUNT - FEE;

pub struct Wallet {
    pub spend: Scalar,
    pub view: Scalar,
}

impl Wallet {
    pub fn from_spend(spend: Scalar) -> Wallet {
        let view = Scalar::from_bytes_mod_order(Keccak256::digest(spend.to_bytes()).into());
        Wallet { spend, view }
    }

    pub fn view_pair(&self) -> ViewPair {
        ViewPair::new(
            Point::from(&self.spend * ED25519_BASEPOINT_TABLE),
            Zeroizing::new(OxideScalar::from(self.view)),
        )
        .expect("view pair")
    }

    pub fn address(&self, network: Network) -> MoneroAddress {
        self.view_pair().legacy_address(network)
    }

    pub fn subaddress(&self, network: Network, account: u32, index: u32) -> MoneroAddress {
        self.view_pair().subaddress(
            network,
            SubaddressIndex::new(account, index).expect("subaddress index"),
        )
    }
}

/// Ring offsets whose first member is the real output at `global_index`.
pub fn ring_offsets(global_index: u64) -> Vec<u64> {
    let mut offsets = vec![global_index];
    offsets.extend(std::iter::repeat(100).take(15));
    offsets
}

pub fn owned_input(
    owner: &Wallet,
    key_offset: Scalar,
    mask: Scalar,
    amount: u64,
    offsets: Vec<u64>,
) -> OutputWithDecoys {
    let key = Point::from(&(owner.spend + key_offset) * ED25519_BASEPOINT_TABLE);
    let commitment = Commitment::new(OxideScalar::from(mask), amount);

    let mut ring = vec![[key, commitment.commit()]];
    for i in 1..16u8 {
        ring.push([
            Point::from(&Scalar::from_bytes_mod_order([i + 0x10; 32]) * ED25519_BASEPOINT_TABLE),
            Point::from(&Scalar::from_bytes_mod_order([i + 0x80; 32]) * ED25519_BASEPOINT_TABLE),
        ]);
    }
    let decoys = Decoys::new(offsets, 0, ring).expect("decoys");
    output_with_decoys(key, OxideScalar::from(key_offset), &commitment, &decoys)
        .expect("synthetic input")
}

pub fn default_input(owner: &Wallet) -> OutputWithDecoys {
    owned_input(
        owner,
        Scalar::from_bytes_mod_order([0x01; 32]),
        Scalar::from_bytes_mod_order([0x05; 32]),
        INPUT_AMOUNT,
        ring_offsets(50_000),
    )
}

pub fn pinned_fee(fee: u64) -> FeeRate {
    FeeRate::new(1, fee).expect("fee rate")
}

pub fn unsigned_for(
    owner: &Wallet,
    network: Network,
    payments: Vec<(MoneroAddress, u64)>,
) -> UnsignedTransaction {
    UnsignedTransaction {
        rct_type: RctType::ClsagBulletproofPlus,
        seed: Zeroizing::new([0x42; 32]),
        fee_rate: pinned_fee(FEE),
        inputs: vec![default_input(owner)],
        payments,
        change: Some(UnsignedChange {
            address: owner.address(network),
            subaddress: None,
        }),
    }
}

pub fn encode(unsigned: &UnsignedTransaction) -> String {
    hex::encode(unsigned.serialize())
}

// Explicit test policy, independent of amounts supplied by the fixture.
pub fn sign_reviewed_seed(
    seed: &str,
    encoded: &str,
    network: &str,
    passphrase: &str,
) -> Result<OfflineSignResult, String> {
    let preview = monero_rust::native::preview_unsigned_transaction_from_seed(
        seed, encoded, network, passphrase,
    )?;
    let approval = preview.approve(10_000_000_000, 100_000_000_000_000)?;
    monero_rust::native::sign_unsigned_transaction(seed, encoded, network, passphrase, &approval)
}

pub fn sign_reviewed_keys(
    wallet: &Wallet,
    encoded: &str,
    network: &str,
) -> Result<OfflineSignResult, String> {
    let (spend, view) = (wallet.spend.to_bytes(), wallet.view.to_bytes());
    let preview = monero_rust::native::preview_unsigned_transaction(spend, view, encoded, network)?;
    let approval = preview.approve(10_000_000_000, 100_000_000_000_000)?;
    monero_rust::native::sign_unsigned_transaction_with_private_keys(
        spend, view, encoded, network, &approval,
    )
}
