//! The unsigned transaction envelope and its offline signer. The envelope
//! carries no change or dummy-output key: the signer uses its own.

use std::io::{self, Read, Write};

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

use monero_wallet::{
    address::{MoneroAddress, Network, SubaddressIndex},
    ed25519::{Commitment, Point as OxidePoint, Scalar as OxideScalar},
    extra::Extra,
    interface::FeeRate,
    io::{read_byte, read_bytes, read_u32, read_u64, read_vec, write_vec},
    ringct::{clsag::Decoys, RctType},
    send::{Change, SignableTransaction, TransactionKeys},
    transaction::Transaction,
    OutputWithDecoys, ViewPair,
};

use crate::scanner::{DEFAULT_LOOKAHEAD, WALLET_CLI_SOFTWARE_LOOKAHEAD};
use crate::tx_utils::checked_amount_sum;

/// Leading bytes of every unsigned transaction.
pub const MAGIC: [u8; 4] = *b"MRUT";

/// The envelope version this build reads and writes.
pub const VERSION: u8 = 3;

const MAX_INPUTS: usize = 1000;

const MAX_PAYMENTS: usize = 16;

/// Highest account the signer accepts change in.
pub const MAX_CHANGE_ACCOUNT: u32 = WALLET_CLI_SOFTWARE_LOOKAHEAD.account;

/// Highest subaddress index the signer accepts change at.
pub const MAX_CHANGE_SUBADDRESS: u32 = DEFAULT_LOOKAHEAD.subaddress;

/// The ring size of the RingCT types an unsigned transaction can have.
pub fn ring_len(rct_type: RctType) -> Option<u8> {
    match rct_type {
        RctType::ClsagBulletproof => Some(11),
        RctType::ClsagBulletproofPlus => Some(16),
        _ => None,
    }
}

/// The output an unsigned transaction has besides its payments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnsignedChange {
    /// Change back to the wallet. The signer refuses to sign unless `address` is
    /// what it derives at `subaddress`.
    Own {
        address: MoneroAddress,
        subaddress: Option<SubaddressIndex>,
    },
    /// A zero-amount output to a throwaway key, as wallet2 adds to a single
    /// payment with no change. The signer draws the key.
    Dummy,
}

impl UnsignedChange {
    /// The change subaddress; `None` for the primary address or a dummy output.
    pub fn subaddress(&self) -> Option<SubaddressIndex> {
        match self {
            UnsignedChange::Own { subaddress, .. } => *subaddress,
            UnsignedChange::Dummy => None,
        }
    }
}

/// A transaction prepared for signing.
#[derive(Clone, PartialEq, Eq)]
pub struct UnsignedTransaction {
    pub rct_type: RctType,
    /// Seeds the transaction keys: anyone who knows it can recompute them.
    pub seed: Zeroizing<[u8; 32]>,
    pub fee_rate: FeeRate,
    pub inputs: Vec<OutputWithDecoys>,
    pub payments: Vec<(MoneroAddress, u64)>,
    /// `None`: the payments are the only outputs, and whatever they leave over is
    /// the fee.
    pub change: Option<UnsignedChange>,
}

impl std::fmt::Debug for UnsignedTransaction {
    // Inputs carry key offsets and commitment masks, so only count them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnsignedTransaction")
            .field("rct_type", &self.rct_type)
            .field("fee_rate", &self.fee_rate)
            .field("inputs", &self.inputs.len())
            .field("payments", &self.payments)
            .field("change", &self.change)
            .finish_non_exhaustive()
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn write_address<W: Write>(address: &MoneroAddress, w: &mut W) -> io::Result<()> {
    write_vec(
        |byte: &u8, w: &mut W| w.write_all(&[*byte]),
        address.to_string().as_bytes(),
        w,
    )
}

fn read_address<R: Read>(r: &mut R) -> io::Result<MoneroAddress> {
    let bytes = read_vec(read_byte, Some(MoneroAddress::SIZE_UPPER_BOUND.0), r)?;
    String::from_utf8(bytes)
        .ok()
        .and_then(|address| MoneroAddress::from_str_with_unchecked_network(&address).ok())
        .ok_or_else(|| invalid("invalid address"))
}

impl UnsignedTransaction {
    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&MAGIC)?;
        w.write_all(&[VERSION, u8::from(self.rct_type)])?;
        w.write_all(self.seed.as_ref())?;
        self.fee_rate.write(w)?;
        write_vec(OutputWithDecoys::write, &self.inputs, w)?;
        write_vec(
            |(address, amount): &(MoneroAddress, u64), w: &mut W| {
                write_address(address, w)?;
                w.write_all(&amount.to_le_bytes())
            },
            &self.payments,
            w,
        )?;
        match &self.change {
            None => w.write_all(&[0]),
            Some(UnsignedChange::Own {
                address,
                subaddress,
            }) => {
                w.write_all(&[1])?;
                write_address(address, w)?;
                match subaddress {
                    None => w.write_all(&[0]),
                    Some(index) => {
                        w.write_all(&[1])?;
                        w.write_all(&index.account().to_le_bytes())?;
                        w.write_all(&index.address().to_le_bytes())
                    }
                }
            }
            Some(UnsignedChange::Dummy) => w.write_all(&[2]),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.write(&mut bytes)
            .expect("writing into a Vec never fails");
        bytes
    }

    /// Read an unsigned transaction. Every count is bounded before anything is
    /// allocated for it, since the bytes come from the online wallet.
    pub fn read<R: Read>(r: &mut R) -> io::Result<UnsignedTransaction> {
        if read_bytes::<_, 4>(r)? != MAGIC {
            return Err(invalid(
                "not an unsigned transaction in the current format; one made by an older \
                 version has to be built again by the online wallet",
            ));
        }
        let version = read_byte(r)?;
        if version != VERSION {
            return Err(invalid(format!(
                "unsupported unsigned transaction version {version}, this build reads version \
                 {VERSION}"
            )));
        }
        let rct_type = RctType::try_from(read_byte(r)?)
            .ok()
            .filter(|rct_type| ring_len(*rct_type).is_some())
            .ok_or_else(|| invalid("unsupported RingCT type"))?;
        let seed = Zeroizing::new(read_bytes(r)?);
        let fee_rate = FeeRate::read(r)?;
        let inputs = read_vec(OutputWithDecoys::read, Some(MAX_INPUTS), r)?;
        let payments = read_vec(
            |r: &mut R| Ok((read_address(r)?, read_u64(r)?)),
            Some(MAX_PAYMENTS),
            r,
        )?;
        let change = match read_byte(r)? {
            0 => None,
            1 => Some(UnsignedChange::Own {
                address: read_address(r)?,
                subaddress: match read_byte(r)? {
                    0 => None,
                    1 => Some(
                        SubaddressIndex::new(read_u32(r)?, read_u32(r)?)
                            .ok_or_else(|| invalid("invalid change subaddress"))?,
                    ),
                    _ => return Err(invalid("invalid change subaddress")),
                },
            }),
            2 => Some(UnsignedChange::Dummy),
            _ => return Err(invalid("invalid change")),
        };
        Ok(UnsignedTransaction {
            rct_type,
            seed,
            fee_rate,
            inputs,
            payments,
            change,
        })
    }

    /// Read an unsigned transaction that must fill `bytes` exactly.
    pub fn from_bytes(bytes: &[u8]) -> Result<UnsignedTransaction, String> {
        let mut reader = bytes;
        let unsigned = UnsignedTransaction::read(&mut reader)
            .map_err(|e| format!("Failed to parse unsigned tx: {e}"))?;
        if !reader.is_empty() {
            return Err("unsigned transaction has trailing bytes".to_string());
        }
        Ok(unsigned)
    }

    /// The fee and change once signed by the wallet of `view_pair`. Foreign change
    /// and a nonzero `dummy_amount` are reported; `sign_offline` refuses them.
    pub fn resolve(&self, view_pair: &ViewPair) -> Result<ResolvedTransaction, String> {
        // A dummy is the same size whatever its key, so the wallet's keys stand in.
        self.build(view_pair, view_pair)
            .map(|(_, resolved)| resolved)
    }

    fn build(
        &self,
        view_pair: &ViewPair,
        dummy: &ViewPair,
    ) -> Result<(SignableTransaction, ResolvedTransaction), String> {
        if self.inputs.is_empty() {
            return Err("unsigned transaction has no inputs".to_string());
        }
        let input_total =
            checked_amount_sum(self.inputs.iter().map(|input| input.commitment().amount))
                .ok_or_else(|| "unsigned transaction input amounts overflow".to_string())?;
        let payment_total = checked_amount_sum(self.payments.iter().map(|payment| payment.1))
            .ok_or_else(|| "unsigned transaction payment amounts overflow".to_string())?;

        let (change, declared) = match &self.change {
            None => (Change::fingerprintable(None), None),
            Some(UnsignedChange::Own {
                address,
                subaddress,
            }) => {
                let derived = change_address(view_pair, *subaddress);
                let is_ours =
                    change_index_allowed(*subaddress) && same_destination(&derived, address);
                (
                    Change::new(view_pair.clone(), *subaddress),
                    Some((*address, is_ours)),
                )
            }
            Some(UnsignedChange::Dummy) => {
                if self.payments.len() != 1 {
                    return Err(format!(
                        "unsigned transaction asks for a dummy output with {} payments; one is \
                         only added to a single payment",
                        self.payments.len()
                    ));
                }
                (Change::new(dummy.clone(), None), None)
            }
        };
        let has_dummy = matches!(self.change, Some(UnsignedChange::Dummy));

        let signable = SignableTransaction::new(
            self.rct_type,
            self.seed.clone(),
            self.inputs.clone(),
            self.payments.clone(),
            change,
            vec![],
            self.fee_rate,
        )
        .map_err(|e| format!("Failed to build signable transaction: {e}"))?;

        let left_over = input_total
            .checked_sub(payment_total)
            .ok_or_else(|| "unsigned transaction pays out more than its inputs".to_string())?;
        let after_fee = || {
            let fee = signable.necessary_fee();
            left_over
                .checked_sub(fee)
                .map(|rest| (fee, rest))
                .ok_or_else(|| {
                    "unsigned transaction cannot pay its fee from its inputs".to_string()
                })
        };
        let (fee, change, dummy_amount) = match declared {
            Some((address, is_ours)) => {
                let (fee, amount) = after_fee()?;
                (
                    fee,
                    Some(ResolvedChange {
                        address,
                        amount,
                        is_ours,
                    }),
                    None,
                )
            }
            None if has_dummy => {
                let (fee, amount) = after_fee()?;
                (fee, None, Some(amount))
            }
            None => (left_over, None, None),
        };

        Ok((
            signable,
            ResolvedTransaction {
                input_total,
                fee,
                change,
                dummy_amount,
            },
        ))
    }
}

/// The change output of a resolved transaction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolvedChange {
    /// The change address the unsigned transaction declares.
    pub address: MoneroAddress,
    pub amount: u64,
    /// Whether the declared address is the one the wallet derives for itself.
    pub is_ours: bool,
}

/// An unsigned transaction with its fee and change worked out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolvedTransaction {
    pub input_total: u64,
    pub fee: u64,
    pub change: Option<ResolvedChange>,
    /// What a dummy output would carry. It goes to a key nobody keeps, so signing
    /// refuses anything but zero.
    pub dummy_amount: Option<u64>,
}

fn change_address(view_pair: &ViewPair, subaddress: Option<SubaddressIndex>) -> MoneroAddress {
    // The network only sets the prefix; destinations are compared by keys and kind.
    match subaddress {
        None => view_pair.legacy_address(Network::Mainnet),
        Some(index) => view_pair.subaddress(Network::Mainnet, index),
    }
}

fn change_index_allowed(subaddress: Option<SubaddressIndex>) -> bool {
    subaddress.map_or(true, |index| {
        index.account() <= MAX_CHANGE_ACCOUNT && index.address() <= MAX_CHANGE_SUBADDRESS
    })
}

/// Whether two addresses have the same keys and kind. The network is ignored.
fn same_destination(a: &MoneroAddress, b: &MoneroAddress) -> bool {
    a.spend() == b.spend() && a.view() == b.view() && a.kind() == b.kind()
}

/// Pair an output's spending data with its decoys, checking both survive
/// `monero-wallet`'s serialization.
pub fn output_with_decoys(
    key: OxidePoint,
    key_offset: OxideScalar,
    commitment: &Commitment,
    decoys: &Decoys,
) -> Result<OutputWithDecoys, String> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(128));
    key.compress()
        .write(&mut *bytes)
        .and_then(|()| key_offset.write(&mut *bytes))
        .and_then(|()| commitment.write(&mut *bytes))
        .and_then(|()| decoys.write(&mut *bytes))
        .map_err(|e| format!("Failed to serialize input: {e}"))?;
    let input = OutputWithDecoys::read(&mut bytes.as_slice())
        .map_err(|e| format!("Invalid transaction input: {e}"))?;
    let same = input.key() == key
        && input.key_offset() == key_offset
        && input.commitment().amount == commitment.amount
        && input.commitment().mask == commitment.mask
        && input.decoys().offsets() == decoys.offsets()
        && input.decoys().signer_index() == decoys.signer_index()
        && input.decoys().ring() == decoys.ring();
    if !same {
        return Err("transaction input did not survive serialization".to_string());
    }
    Ok(input)
}

/// A signed transaction and the keys needed to prove its payments.
pub struct SignedTransaction {
    pub tx: Transaction,
    /// The transaction private key.
    pub tx_key: Zeroizing<Scalar>,
    /// One key per output when the transaction uses additional keys, else none.
    pub additional_tx_keys: Vec<Zeroizing<Scalar>>,
    /// The fee the signed transaction pays.
    pub fee: u64,
}

/// Sign an unsigned transaction; nothing here asks a user to approve it. For bytes
/// from another machine use `native::sign_unsigned_transaction_with_private_keys`.
pub fn sign_offline<R: RngCore + CryptoRng>(
    rng: &mut R,
    spend: &Zeroizing<Scalar>,
    view: &Zeroizing<Scalar>,
    unsigned: &UnsignedTransaction,
) -> Result<SignedTransaction, String> {
    if unsigned.inputs.is_empty() {
        return Err("unsigned transaction has no inputs".to_string());
    }
    check_input_ownership(spend, unsigned)?;

    let view_pair = ViewPair::new(
        OxidePoint::from(&**spend * ED25519_BASEPOINT_TABLE),
        Zeroizing::new(OxideScalar::from(**view)),
    )
    .map_err(|e| format!("Invalid wallet keys: {e}"))?;

    // The online wallet holds the view key, so a dummy sent to a spend key it
    // chose would be its to spend. Draw the key here.
    let dummy = match unsigned.change {
        Some(UnsignedChange::Dummy) => Some(throwaway_view_pair(rng, view)?),
        _ => None,
    };
    let (signable, resolved) = unsigned.build(&view_pair, dummy.as_ref().unwrap_or(&view_pair))?;
    if dummy.is_some() && unsigned.resolve(&view_pair)? != resolved {
        return Err(
            "the transaction to sign does not have the fee and amounts it resolved to".to_string(),
        );
    }

    if let Some(amount) = resolved.dummy_amount {
        if amount != 0 {
            return Err(format!(
                "Refusing to sign: this transaction asks for a dummy output, which must carry \
                 nothing, but its payment and fee leave {amount} piconero over. A dummy output \
                 goes to a key nobody keeps, so that amount would be lost. An unsigned \
                 transaction is built by the online, view-only wallet, and one built correctly \
                 sends what is left over back as change."
            ));
        }
    }

    if let Some(change) = &resolved.change {
        if !change.is_ours {
            return Err(format!(
                "Refusing to sign: this transaction sends its change to {}, which is not the \
                 address this wallet derives for its own change. An unsigned transaction is \
                 built by the online, view-only wallet, and a tampered one redirects the change \
                 while still balancing its amounts. (Change to a subaddress beyond account \
                 {MAX_CHANGE_ACCOUNT} or index {MAX_CHANGE_SUBADDRESS} also lands here: it fails \
                 closed.)",
                change.address
            ));
        }
    }

    let spend_oxide = Zeroizing::new(OxideScalar::from(**spend));
    let tx = signable
        .sign(rng, &spend_oxide)
        .map_err(|e| format!("Failed to sign transaction: {e}"))?;

    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &tx
    else {
        return Err("signed transaction is not a RingCT transaction".to_string());
    };
    if proofs.base.fee != resolved.fee {
        return Err("signed transaction fee differs from the resolved fee".to_string());
    }

    let (tx_key, additional_tx_keys) = transaction_keys(unsigned, &view_pair, &tx)?;
    Ok(SignedTransaction {
        tx,
        tx_key,
        additional_tx_keys,
        fee: resolved.fee,
    })
}

fn throwaway_view_pair<R: RngCore + CryptoRng>(
    rng: &mut R,
    view: &Zeroizing<Scalar>,
) -> Result<ViewPair, String> {
    let mut wide = Zeroizing::new([0u8; 64]);
    rng.fill_bytes(wide.as_mut());
    let mut spend = Scalar::from_bytes_mod_order_wide(&wide);
    let spend_public = &spend * ED25519_BASEPOINT_TABLE;
    spend.zeroize();
    ViewPair::new(
        OxidePoint::from(spend_public),
        Zeroizing::new(OxideScalar::from(**view)),
    )
    .map_err(|e| format!("Invalid dummy output key: {e}"))
}

/// Check the spend key opens every input's one-time key.
pub fn check_input_ownership(
    spend: &Zeroizing<Scalar>,
    unsigned: &UnsignedTransaction,
) -> Result<(), String> {
    for input in &unsigned.inputs {
        let one_time = Zeroizing::new(**spend + input.key_offset().into());
        let expected: EdwardsPoint = input.key().into();
        if (&*one_time * ED25519_BASEPOINT_TABLE) != expected {
            return Err("wrong private key for unsigned transaction".to_string());
        }
    }
    Ok(())
}

/// Recompute the transaction keys, accepting each only if it produces the public
/// key the signed transaction carries.
fn transaction_keys(
    unsigned: &UnsignedTransaction,
    view_pair: &ViewPair,
    tx: &Transaction,
) -> Result<(Zeroizing<Scalar>, Vec<Zeroizing<Scalar>>), String> {
    let extra = Extra::read(&mut tx.prefix().extra.as_slice())
        .map_err(|e| format!("Failed to read signed transaction extra: {e}"))?;
    let (tx_public_keys, additional_public_keys) = extra
        .keys()
        .ok_or_else(|| "signed transaction has no transaction public key".to_string())?;
    let [tx_public_key] = tx_public_keys.as_slice() else {
        return Err("signed transaction has more than one transaction public key".to_string());
    };
    let additional_public_keys = additional_public_keys.unwrap_or_default();

    // A transaction key is a multiple of the basepoint or of a paid subaddress's
    // spend key.
    let mut bases = vec![curve25519_dalek::constants::ED25519_BASEPOINT_POINT];
    let change = match &unsigned.change {
        Some(UnsignedChange::Own { subaddress, .. }) => {
            Some(change_address(view_pair, *subaddress))
        }
        Some(UnsignedChange::Dummy) | None => None,
    };
    for address in unsigned
        .payments
        .iter()
        .map(|payment| &payment.0)
        .chain(&change)
    {
        if address.is_subaddress() {
            bases.push(address.spend().into());
        }
    }
    let matches = |key: &Zeroizing<Scalar>, public: &OxidePoint| {
        let public: EdwardsPoint = (*public).into();
        bases.iter().any(|base| (**key * base) == public)
    };

    let mut keys = TransactionKeys::new(
        &unsigned.seed,
        unsigned
            .inputs
            .iter()
            .map(|input| (input.key(), input.commitment().commit()))
            .collect(),
    );
    let mut next_key = || -> Zeroizing<Scalar> {
        let key = keys
            .next()
            .expect("TransactionKeys (never-ending) was exhausted");
        Zeroizing::new((*key).into())
    };

    let tx_key = next_key();
    if !matches(&tx_key, tx_public_key) {
        return Err("transaction key does not match the signed transaction".to_string());
    }
    let mut additional_tx_keys = Vec::with_capacity(additional_public_keys.len());
    for public in &additional_public_keys {
        let key = next_key();
        if !matches(&key, public) {
            return Err(
                "additional transaction key does not match the signed transaction".to_string(),
            );
        }
        additional_tx_keys.push(key);
    }
    Ok((tx_key, additional_tx_keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_wallet::address::AddressType;
    use monero_wallet::io::VarInt;
    use rand::SeedableRng;

    const INPUT_AMOUNT: u64 = 2_000_000_000;

    struct Wallet {
        spend: Zeroizing<Scalar>,
        view: Zeroizing<Scalar>,
    }

    fn wallet(spend: u64, view: u64) -> Wallet {
        Wallet {
            spend: Zeroizing::new(Scalar::from(spend)),
            view: Zeroizing::new(Scalar::from(view)),
        }
    }

    impl Wallet {
        fn view_pair(&self) -> ViewPair {
            ViewPair::new(
                OxidePoint::from(&*self.spend * ED25519_BASEPOINT_TABLE),
                Zeroizing::new(OxideScalar::from(*self.view)),
            )
            .unwrap()
        }

        fn address(&self) -> MoneroAddress {
            self.view_pair().legacy_address(Network::Mainnet)
        }

        fn subaddress(&self, account: u32, index: u32) -> MoneroAddress {
            self.view_pair().subaddress(
                Network::Mainnet,
                SubaddressIndex::new(account, index).unwrap(),
            )
        }
    }

    fn sender() -> Wallet {
        wallet(987_654_321, 1_234_567)
    }

    fn recipient() -> Wallet {
        wallet(777, 888)
    }

    const SIGNER_INDEX: u8 = 5;

    fn ring_offsets() -> Vec<u64> {
        let mut offsets = vec![1000u64];
        offsets.extend(std::iter::repeat(3).take(15));
        offsets
    }

    fn input(owner: &Wallet, tag: u64, amount: u64) -> OutputWithDecoys {
        let key_offset = Scalar::from(1_122_334_455u64 + tag);
        let key = OxidePoint::from(&(*owner.spend + key_offset) * ED25519_BASEPOINT_TABLE);
        let commitment = Commitment::new(OxideScalar::from(Scalar::from(31_337u64 + tag)), amount);
        let ring = (0..16u64)
            .map(|i| {
                if i == u64::from(SIGNER_INDEX) {
                    [key, commitment.commit()]
                } else {
                    let point = OxidePoint::from(
                        &Scalar::from(9000 + (100 * tag) + i) * ED25519_BASEPOINT_TABLE,
                    );
                    [point, point]
                }
            })
            .collect();
        let decoys = Decoys::new(ring_offsets(), SIGNER_INDEX, ring).unwrap();
        output_with_decoys(key, OxideScalar::from(key_offset), &commitment, &decoys).unwrap()
    }

    fn fee_rate() -> FeeRate {
        FeeRate::new(20_000, 10_000).unwrap()
    }

    fn unsigned(payments: Vec<(MoneroAddress, u64)>) -> UnsignedTransaction {
        UnsignedTransaction {
            rct_type: RctType::ClsagBulletproofPlus,
            seed: Zeroizing::new([42; 32]),
            fee_rate: fee_rate(),
            inputs: vec![input(&sender(), 0, INPUT_AMOUNT)],
            payments,
            change: Some(UnsignedChange::Own {
                address: sender().address(),
                subaddress: None,
            }),
        }
    }

    fn sign(unsigned: &UnsignedTransaction) -> Result<SignedTransaction, String> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
        let sender = sender();
        sign_offline(&mut rng, &sender.spend, &sender.view, unsigned)
    }

    fn signed_fee(tx: &Transaction) -> u64 {
        let Transaction::V2 {
            proofs: Some(proofs),
            ..
        } = tx
        else {
            panic!("transaction wasn't RingCT");
        };
        proofs.base.fee
    }

    fn extra_keys(tx: &Transaction) -> (Vec<EdwardsPoint>, Vec<EdwardsPoint>) {
        let extra = Extra::read(&mut tx.prefix().extra.as_slice()).unwrap();
        let (keys, additional) = extra.keys().unwrap();
        (
            keys.into_iter().map(|key| key.into()).collect(),
            additional
                .unwrap_or_default()
                .into_iter()
                .map(|key| key.into())
                .collect(),
        )
    }

    fn received_amounts(
        owner: &Wallet,
        subaddress: Option<SubaddressIndex>,
        tx: &Transaction,
    ) -> Vec<u64> {
        use monero_oxide::transaction::Pruned;
        let mut scanner = monero_wallet::Scanner::new(owner.view_pair());
        if let Some(index) = subaddress {
            scanner.register_subaddress(index);
        }
        let pruned = monero_oxide::transaction::Transaction::<Pruned>::from(tx.clone());
        crate::scanner::scan_single_transaction(&mut scanner, tx.hash(), &pruned)
            .unwrap()
            .iter()
            .map(|output| output.commitment().amount)
            .collect()
    }

    #[test]
    fn output_with_decoys_keeps_the_output_and_its_ring() {
        let key_offset = OxideScalar::from(Scalar::from(13u64));
        let key = OxidePoint::from(&Scalar::from(11u64) * ED25519_BASEPOINT_TABLE);
        let commitment = Commitment::new(OxideScalar::from(Scalar::from(17u64)), 1_000_000);
        let ring: Vec<[OxidePoint; 2]> = (0..16u64)
            .map(|i| {
                let point = OxidePoint::from(&Scalar::from(500 + i) * ED25519_BASEPOINT_TABLE);
                [point, point]
            })
            .collect();
        let decoys = Decoys::new(ring_offsets(), SIGNER_INDEX, ring.clone()).unwrap();

        let input = output_with_decoys(key, key_offset, &commitment, &decoys).unwrap();
        assert_eq!(input.key(), key);
        assert_eq!(input.key_offset(), key_offset);
        assert_eq!(input.commitment().amount, 1_000_000);
        assert_eq!(input.commitment().mask, commitment.mask);
        assert_eq!(input.decoys().offsets(), ring_offsets().as_slice());
        assert_eq!(input.decoys().signer_index(), SIGNER_INDEX);
        assert_eq!(input.decoys().ring(), ring.as_slice());
    }

    #[test]
    fn envelope_roundtrips() {
        let mut tx = unsigned(vec![
            (recipient().address(), 1_000_000_000),
            (recipient().subaddress(0, 1), 5),
        ]);
        for change in [
            None,
            Some(UnsignedChange::Own {
                address: sender().address(),
                subaddress: None,
            }),
            Some(UnsignedChange::Own {
                address: sender().subaddress(3, 0),
                subaddress: SubaddressIndex::new(3, 0),
            }),
            Some(UnsignedChange::Dummy),
        ] {
            tx.change = change;
            let bytes = tx.serialize();
            assert_eq!(&bytes[..4], b"MRUT");
            assert_eq!(bytes[4], VERSION);
            assert_eq!(UnsignedTransaction::from_bytes(&bytes).unwrap(), tx);

            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(UnsignedTransaction::from_bytes(&trailing)
                .unwrap_err()
                .contains("trailing bytes"));
            assert!(UnsignedTransaction::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        }
    }

    #[test]
    fn envelope_carries_no_wallet_secret() {
        let bytes = unsigned(vec![(recipient().address(), 1_000_000_000)]).serialize();
        for secret in [sender().view.to_bytes(), sender().spend.to_bytes()] {
            assert!(!bytes.windows(32).any(|window| window == secret));
        }
    }

    #[test]
    fn refuses_other_formats_and_versions() {
        let mut old = vec![0u8, 16];
        old.extend_from_slice(&[7; 200]);
        let error = UnsignedTransaction::from_bytes(&old).unwrap_err();
        assert!(error.contains("older version"), "{error}");

        let mut bytes = unsigned(vec![(recipient().address(), 1)]).serialize();
        bytes[4] = VERSION + 1;
        let error = UnsignedTransaction::from_bytes(&bytes).unwrap_err();
        assert!(
            error.contains("unsupported unsigned transaction version 4"),
            "{error}"
        );

        bytes[4] = VERSION;
        bytes[5] = 4;
        let error = UnsignedTransaction::from_bytes(&bytes).unwrap_err();
        assert!(error.contains("unsupported RingCT type"), "{error}");
    }

    #[test]
    fn refuses_counts_beyond_the_bounds() {
        let header = || {
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(&[VERSION, u8::from(RctType::ClsagBulletproofPlus)]);
            bytes.extend_from_slice(&[42; 32]);
            fee_rate().write(&mut bytes).unwrap();
            bytes
        };

        let mut bytes = header();
        VarInt::write(&1_000_000u64, &mut bytes).unwrap();
        let error = UnsignedTransaction::from_bytes(&bytes).unwrap_err();
        assert!(error.contains("exceeds bound"), "{error}");

        let mut bytes = header();
        bytes.push(0);
        bytes.push(17);
        let error = UnsignedTransaction::from_bytes(&bytes).unwrap_err();
        assert!(error.contains("exceeds bound"), "{error}");

        let mut bytes = header();
        bytes.push(0);
        bytes.push(1);
        VarInt::write(&100_000u64, &mut bytes).unwrap();
        let error = UnsignedTransaction::from_bytes(&bytes).unwrap_err();
        assert!(error.contains("exceeds bound"), "{error}");
    }

    #[test]
    fn sign_offline_produces_valid_transaction() {
        let sender = sender();
        let pay = 1_000_000_000u64;
        let unsigned = unsigned(vec![(recipient().address(), pay)]);
        let resolved = unsigned.resolve(&sender.view_pair()).unwrap();
        let change = resolved.change.unwrap();
        assert!(change.is_ours);
        assert_eq!(resolved.input_total, INPUT_AMOUNT);
        assert_eq!(pay + change.amount + resolved.fee, INPUT_AMOUNT);

        let signed = sign(&unsigned).unwrap();
        let tx = &signed.tx;
        assert!(signed.additional_tx_keys.is_empty());

        // Input structure: our ring offsets, our key image
        assert_eq!(tx.version(), 2);
        let inputs = &tx.prefix().inputs;
        assert_eq!(inputs.len(), 1);
        let monero_oxide::transaction::Input::ToKey {
            key_offsets,
            key_image,
            ..
        } = &inputs[0]
        else {
            panic!("input wasn't ToKey");
        };
        assert_eq!(key_offsets, &ring_offsets());
        let one_time = *sender.spend + Scalar::from(1_122_334_455u64);
        let one_time_key = &one_time * ED25519_BASEPOINT_TABLE;
        let hashed: EdwardsPoint =
            OxidePoint::biased_hash(one_time_key.compress().to_bytes()).into();
        assert_eq!(
            key_image.to_bytes(),
            (hashed * one_time).compress().to_bytes()
        );

        assert_eq!(tx.prefix().outputs.len(), 2);
        assert_eq!(signed.fee, resolved.fee);
        assert_eq!(signed_fee(tx), resolved.fee);
        assert_eq!(
            received_amounts(&sender, None, tx),
            vec![change.amount],
            "the change output must come back with the resolved amount"
        );
        assert_eq!(received_amounts(&recipient(), None, tx), vec![pay]);

        assert_eq!(
            u128::from(resolved.fee),
            fee_rate().calculate_fee_from_weight(tx.weight() as u64)
        );

        // The returned tx key matches the pubkey embedded in extra
        let (tx_keys, additional) = extra_keys(tx);
        assert_eq!(tx_keys, vec![&*signed.tx_key * ED25519_BASEPOINT_TABLE]);
        assert!(additional.is_empty());

        // Deterministic given the same seed material
        let mut rng = rand::rngs::StdRng::seed_from_u64(999);
        let again = sign_offline(&mut rng, &sender.spend, &sender.view, &unsigned).unwrap();
        assert_eq!(again.tx_key, signed.tx_key);
        assert_eq!(again.tx.prefix().extra, tx.prefix().extra);
    }

    #[test]
    fn subaddress_payment_with_change_takes_the_wallet2_two_output_form() {
        let destination = recipient().subaddress(0, 1);
        let signed = sign(&unsigned(vec![(destination, 1_000_000_000)])).unwrap();
        assert!(signed.additional_tx_keys.is_empty());

        let (tx_keys, additional) = extra_keys(&signed.tx);
        assert!(additional.is_empty());
        let spend: EdwardsPoint = destination.spend().into();
        assert_eq!(tx_keys, vec![*signed.tx_key * spend]);
        assert_eq!(signed.tx.prefix().extra.len(), 44);
    }

    #[test]
    fn synthetic_change_view_pair_has_the_wallet2_shape() {
        let sender = sender();
        let destination = recipient().subaddress(0, 1);
        let dummy_change = ViewPair::new(
            OxidePoint::from(&Scalar::from(123_456_789u64) * ED25519_BASEPOINT_TABLE),
            Zeroizing::new(OxideScalar::from(*sender.view)),
        )
        .unwrap();
        let build = |amount| {
            SignableTransaction::new(
                RctType::ClsagBulletproofPlus,
                Zeroizing::new([42; 32]),
                vec![input(&sender, 0, INPUT_AMOUNT)],
                vec![(destination, amount)],
                Change::new(dummy_change.clone(), None),
                vec![],
                fee_rate(),
            )
            .unwrap()
        };

        let fee = build(1).necessary_fee();
        let amount = INPUT_AMOUNT - fee;
        let signable = build(amount);
        assert_eq!(signable.necessary_fee(), fee);

        let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
        let signed = signable
            .sign(&mut rng, &Zeroizing::new(OxideScalar::from(*sender.spend)))
            .unwrap();
        assert_eq!(signed.prefix().outputs.len(), 2);
        assert_eq!(signed.prefix().extra.len(), 44);
        assert!(extra_keys(&signed).1.is_empty());
        assert_eq!(signed_fee(&signed), fee);
        assert_eq!(
            received_amounts(
                &recipient(),
                Some(SubaddressIndex::new(0, 1).unwrap()),
                &signed
            ),
            vec![amount]
        );
    }

    #[test]
    fn dummy_output_has_the_wallet2_shape_and_must_be_zero() {
        let sender = sender();
        let recipient = recipient();
        let destination = recipient.subaddress(0, 1);
        let mut unsigned = unsigned(vec![(destination, 1)]);
        unsigned.change = Some(UnsignedChange::Dummy);

        let fee = unsigned.resolve(&sender.view_pair()).unwrap().fee;
        unsigned.payments[0].1 = INPUT_AMOUNT - fee;
        let resolved = unsigned.resolve(&sender.view_pair()).unwrap();
        assert_eq!(resolved.dummy_amount, Some(0));
        assert!(resolved.change.is_none());

        let signed = sign(&unsigned).unwrap();
        let (tx_keys, additional) = extra_keys(&signed.tx);
        let destination_spend: EdwardsPoint = destination.spend().into();
        assert_eq!(signed.tx.prefix().outputs.len(), 2);
        assert_eq!(tx_keys, vec![*signed.tx_key * destination_spend]);
        assert!(additional.is_empty());
        assert_eq!(signed.tx.prefix().extra.len(), 44);
        assert_eq!(
            received_amounts(
                &recipient,
                Some(SubaddressIndex::new(0, 1).unwrap()),
                &signed.tx,
            ),
            vec![INPUT_AMOUNT - fee]
        );
        assert!(
            received_amounts(&sender, None, &signed.tx).is_empty(),
            "the sender must not recognize the throwaway output"
        );

        let mut hostile = unsigned;
        hostile.payments[0].1 -= 1;
        assert_eq!(
            hostile.resolve(&sender.view_pair()).unwrap().dummy_amount,
            Some(1)
        );
        let error = sign(&hostile)
            .err()
            .expect("a nonzero dummy output must be refused");
        assert!(error.contains("dummy output"), "{error}");
        assert!(error.contains("must carry nothing"), "{error}");
    }

    #[test]
    fn additional_keys_are_returned_when_the_transaction_has_them() {
        let unsigned = unsigned(vec![
            (recipient().subaddress(0, 1), 400_000_000),
            (recipient().address(), 300_000_000),
        ]);
        let signed = sign(&unsigned).unwrap();
        assert_eq!(signed.tx.prefix().outputs.len(), 3);

        let (tx_keys, additional) = extra_keys(&signed.tx);
        assert_eq!(tx_keys, vec![&*signed.tx_key * ED25519_BASEPOINT_TABLE]);
        assert_eq!(additional.len(), 3);
        assert_eq!(signed.additional_tx_keys.len(), 3);

        let spend: EdwardsPoint = recipient().subaddress(0, 1).spend().into();
        let mut with_subaddress_spend = 0;
        for (key, public) in signed.additional_tx_keys.iter().zip(&additional) {
            if **key * spend == *public {
                with_subaddress_spend += 1;
            } else {
                assert_eq!(&**key * ED25519_BASEPOINT_TABLE, *public);
            }
        }
        assert_eq!(with_subaddress_spend, 1);
    }

    #[test]
    fn no_change_transaction_pays_the_residual_as_fee() {
        let sender = sender();
        let mut unsigned = unsigned(vec![
            (recipient().address(), 1_000_000_000),
            (recipient().subaddress(0, 1), 999_000_000),
        ]);
        unsigned.change = None;
        unsigned.fee_rate = FeeRate::new(1, 1).unwrap();

        let resolved = unsigned.resolve(&sender.view_pair()).unwrap();
        assert_eq!(resolved.fee, 1_000_000);
        assert!(resolved.change.is_none());

        let bytes = unsigned.serialize();
        let unsigned = UnsignedTransaction::from_bytes(&bytes).unwrap();
        let signed = sign(&unsigned).unwrap();
        assert_eq!(signed.fee, 1_000_000);
        assert_eq!(signed_fee(&signed.tx), 1_000_000);
        assert!(received_amounts(&sender, None, &signed.tx).is_empty());
    }

    #[test]
    fn a_fee_rate_can_pin_an_exact_fee() {
        let sender = sender();
        let fee = 123_456_789u64;
        let pay = 1_000_000_000u64;
        let mut unsigned = unsigned(vec![(recipient().address(), pay)]);
        unsigned.fee_rate = FeeRate::new(1, fee).unwrap();

        let resolved = unsigned.resolve(&sender.view_pair()).unwrap();
        assert_eq!(resolved.fee, fee);
        assert_eq!(resolved.change.unwrap().amount, INPUT_AMOUNT - pay - fee);

        let signed = sign(&unsigned).unwrap();
        assert_eq!(signed_fee(&signed.tx), fee);
        assert_eq!(
            received_amounts(&sender, None, &signed.tx),
            vec![INPUT_AMOUNT - pay - fee]
        );
    }

    #[test]
    fn single_payment_without_change_is_refused() {
        let mut unsigned = unsigned(vec![(recipient().address(), 1)]);
        unsigned.change = None;
        let error = unsigned.resolve(&sender().view_pair()).err().unwrap();
        assert!(error.contains("no change address"), "{error}");
    }

    #[test]
    fn sign_offline_rejects_a_key_that_does_not_own_the_inputs() {
        let unsigned = unsigned(vec![(recipient().address(), 1_000_000_000)]);
        let other = wallet(4, 5);
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let error = sign_offline(&mut rng, &other.spend, &other.view, &unsigned)
            .err()
            .unwrap();
        assert!(error.contains("wrong private key"), "{error}");
    }

    #[test]
    fn refuses_payment_amounts_that_wrap() {
        let (a, b) = (u64::MAX, INPUT_AMOUNT + 1);
        assert_eq!(
            a.wrapping_add(b),
            INPUT_AMOUNT,
            "the two payments must wrap to the input total"
        );
        let attacker = recipient().address();
        let mut unsigned = unsigned(vec![(attacker, a), (attacker, b)]);
        unsigned.change = None;

        let error = sign(&unsigned)
            .err()
            .expect("payment amounts that wrap must be refused");
        assert!(
            error.contains("payment amounts overflow"),
            "the refusal must name the overflow rather than blame the fee: {error}"
        );
        let error = unsigned.resolve(&sender().view_pair()).err().unwrap();
        assert!(error.contains("payment amounts overflow"), "{error}");
    }

    /// Reject overflow in the input total too, even when the wallet owns both inputs.
    #[test]
    fn refuses_input_amounts_that_wrap() {
        assert_eq!(
            u64::MAX.wrapping_add(2),
            1,
            "the two inputs must wrap to something a payment can match"
        );
        let mut unsigned = unsigned(vec![(recipient().address(), 1)]);
        unsigned.inputs = vec![input(&sender(), 0, u64::MAX), input(&sender(), 1, 2)];
        unsigned.change = None;

        let error = sign(&unsigned)
            .err()
            .expect("input amounts that wrap must be refused");
        assert!(error.contains("input amounts overflow"), "{error}");
    }

    #[test]
    fn refuses_an_unsigned_transaction_without_inputs() {
        let mut unsigned = unsigned(vec![(recipient().address(), 1)]);
        unsigned.inputs.clear();
        assert!(sign(&unsigned).err().unwrap().contains("no inputs"));
        assert!(unsigned
            .resolve(&sender().view_pair())
            .err()
            .unwrap()
            .contains("no inputs"));
    }

    #[test]
    fn the_weight_estimate_bounds_signed_transactions() {
        use crate::fee_estimate::transaction_weight_upper_bound;

        let standard = recipient().address();
        let subaddress = recipient().subaddress(0, 1);
        let two_inputs = vec![
            input(&sender(), 0, INPUT_AMOUNT),
            input(&sender(), 1, INPUT_AMOUNT),
        ];
        let mut shapes = Vec::new();
        for payments in [
            vec![(standard, 1_000_000_000)],
            vec![(subaddress, 1_000_000_000)],
            vec![(subaddress, 400_000_000), (standard, 300_000_000)],
            vec![(subaddress, 400_000_000), (subaddress, 300_000_000)],
        ] {
            shapes.push(unsigned(payments.clone()));
            let mut more_inputs = unsigned(payments);
            more_inputs.inputs = two_inputs.clone();
            shapes.push(more_inputs);
        }
        let mut sweep = unsigned(vec![(subaddress, 1_000_000_000), (subaddress, 0)]);
        sweep.change = None;
        shapes.push(sweep);

        for tx in shapes {
            let outputs = tx.payments.len() + usize::from(tx.change.is_some());
            let payment_id = outputs == 2;
            let bound =
                transaction_weight_upper_bound(tx.rct_type, tx.inputs.len(), outputs, payment_id)
                    .unwrap();
            let signed = sign(&tx).unwrap();
            assert!(
                signed.tx.weight() <= bound,
                "{} inputs, {} outputs: weight {} is above the estimate {}",
                tx.inputs.len(),
                outputs,
                signed.tx.weight(),
                bound
            );
        }
    }

    #[test]
    fn change_must_be_the_address_the_signer_derives() {
        let sender = sender();
        let payments = vec![(recipient().address(), 1_000_000_000)];
        let refused = |change: UnsignedChange| {
            let mut tx = unsigned(payments.clone());
            tx.change = Some(change);
            let resolved = tx.resolve(&sender.view_pair()).unwrap();
            assert!(!resolved.change.unwrap().is_ours);
            let error = sign(&tx).err().expect("redirected change must be refused");
            assert!(error.contains("Refusing to sign"), "{error}");
        };

        refused(UnsignedChange::Own {
            address: recipient().address(),
            subaddress: None,
        });
        refused(UnsignedChange::Own {
            address: sender.subaddress(0, 2),
            subaddress: SubaddressIndex::new(0, 1),
        });
        refused(UnsignedChange::Own {
            address: sender.subaddress(0, 1),
            subaddress: None,
        });
        let primary = sender.address();
        for kind in [
            AddressType::Subaddress,
            AddressType::LegacyIntegrated([7; 8]),
            AddressType::Featured {
                subaddress: false,
                payment_id: None,
                guaranteed: true,
            },
        ] {
            refused(UnsignedChange::Own {
                address: MoneroAddress::new(
                    Network::Mainnet,
                    kind,
                    primary.spend(),
                    primary.view(),
                ),
                subaddress: None,
            });
        }
        for (account, index) in [(MAX_CHANGE_ACCOUNT + 1, 0), (0, MAX_CHANGE_SUBADDRESS + 1)] {
            refused(UnsignedChange::Own {
                address: sender.subaddress(account, index),
                subaddress: SubaddressIndex::new(account, index),
            });
        }

        let mut tx = unsigned(payments.clone());
        tx.change = Some(UnsignedChange::Own {
            address: sender.view_pair().legacy_address(Network::Stagenet),
            subaddress: None,
        });
        assert!(
            tx.resolve(&sender.view_pair())
                .unwrap()
                .change
                .unwrap()
                .is_ours
        );
        sign(&tx).unwrap();
    }

    #[test]
    fn change_to_an_account_subaddress_comes_back() {
        let sender = sender();
        let index = SubaddressIndex::new(3, 0).unwrap();
        let mut tx = unsigned(vec![(recipient().address(), 1_000_000_000)]);
        tx.change = Some(UnsignedChange::Own {
            address: sender.subaddress(3, 0),
            subaddress: Some(index),
        });
        let resolved = tx.resolve(&sender.view_pair()).unwrap();
        let change = resolved.change.unwrap();
        assert!(change.is_ours);

        let signed = sign(&tx).unwrap();
        assert!(signed.additional_tx_keys.is_empty());
        assert_eq!(signed.tx.prefix().extra.len(), 44);
        assert_eq!(
            received_amounts(&sender, Some(index), &signed.tx),
            vec![change.amount]
        );
    }
}
