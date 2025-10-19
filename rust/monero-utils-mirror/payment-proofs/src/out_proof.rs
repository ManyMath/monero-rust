use core::fmt;
use std::io::{self, Read};
use std_shims::{string::String, vec::Vec, vec};

use zeroize::Zeroizing;
use rand_core::{RngCore, CryptoRng};

use sha3::{Digest, Keccak256};
use curve25519_dalek::{
  constants::{ED25519_BASEPOINT_POINT, ED25519_BASEPOINT_TABLE},
  scalar::Scalar,
  edwards::{EdwardsPoint, CompressedEdwardsY},
};

use monero_serai::{
  transaction::Transaction,
  wallet::address::{AddressBytes, Address},
  wallet::extra::{PaymentId, Extra},
};

use crate::{base58, shared_key_derivations::SharedKeyDerivations};

// --- inlined helpers ---

fn keccak256(data: impl AsRef<[u8]>) -> [u8; 32] {
  Keccak256::digest(data.as_ref()).into()
}

fn read_scalar<R: Read>(r: &mut R) -> io::Result<Scalar> {
  let mut bytes = [0u8; 32];
  r.read_exact(&mut bytes)?;
  Scalar::from_canonical_bytes(bytes)
    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "unreduced scalar"))
}

fn read_point<R: Read>(r: &mut R) -> io::Result<EdwardsPoint> {
  let mut bytes = [0u8; 32];
  r.read_exact(&mut bytes)?;
  CompressedEdwardsY(bytes)
    .decompress()
    .filter(|point| point.compress().to_bytes() == bytes)
    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid point"))
}

// --- OutProof ---

/// A Monero OutProof.
///
/// This is specifically a v2 OutProof.
/// [v1 OutProofs were insecure](https://github.com/monero-project/research-lab/issues/60) and are
/// unsupported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutProof {
  ecdh: EdwardsPoint,
  c: Scalar,
  s: Scalar,
}

impl OutProof {
  fn challenge<B: AddressBytes>(
    address: &Address<B>,
    nonce_commitment_generator: EdwardsPoint,
    nonce_commitment_view_key: EdwardsPoint,
    ephemeral_key_commitment: EdwardsPoint,
    ecdh: EdwardsPoint,
    message: &[u8],
  ) -> Scalar {
    let mut keccak = Keccak256::new();
    keccak.update(keccak256(message));
    keccak.update(ecdh.compress().to_bytes());
    keccak.update(nonce_commitment_generator.compress().to_bytes());
    keccak.update(nonce_commitment_view_key.compress().to_bytes());
    if !address.is_guaranteed() {
      keccak.update(keccak256(b"TXPROOF_V2"));
    } else {
      keccak.update(keccak256(b"TXPROOF_V2_GUARANTEED"));
    }
    keccak.update(ephemeral_key_commitment.compress().to_bytes());
    keccak.update(address.view.compress().to_bytes());
    if address.is_subaddress() {
      keccak.update(address.spend.compress().to_bytes());
    } else {
      keccak.update([0; 32]);
    }
    Scalar::from_bytes_mod_order(keccak.finalize().into())
  }

  /// Prove an OutProof v2.
  pub fn prove<B: AddressBytes>(
    rng: &mut (impl RngCore + CryptoRng),
    address: &Address<B>,
    ephemeral_key: &Zeroizing<Scalar>,
    message: &[u8],
  ) -> Self {
    let nonce = {
      let mut wide = Zeroizing::new([0u8; 64]);
      rng.fill_bytes(wide.as_mut());
      Zeroizing::new(Scalar::from_bytes_mod_order_wide(&*wide))
    };
    let commitment_generator =
      if address.is_subaddress() { address.spend } else { ED25519_BASEPOINT_POINT };
    let commit = |value: &Scalar| (commitment_generator * value, address.view * value);
    let (nonce_commitment_generator, nonce_commitment_view_key) = commit(&*nonce);
    let (ephemeral_key_commitment, ecdh) = commit(&**ephemeral_key);
    let c = Self::challenge(
      address,
      nonce_commitment_generator,
      nonce_commitment_view_key,
      ephemeral_key_commitment,
      ecdh,
      message,
    );
    let s = &*nonce - (c * &**ephemeral_key);
    OutProof { ecdh, c, s }
  }

  /// Verify an OutProof. Returns the amount if valid.
  pub fn verify<B: AddressBytes>(
    self,
    tx: &Transaction,
    output_index: usize,
    address: &Address<B>,
    message: &[u8],
  ) -> Option<u64> {
    let commitment_generator =
      if address.is_subaddress() { address.spend } else { ED25519_BASEPOINT_POINT };

    let OutProof { ecdh, c, s } = self;
    let s_commitment_generator = commitment_generator * s;
    let s_commitment_view_key = address.view * s;

    let extra = Extra::read(&mut tx.prefix.extra.as_slice()).ok()?;
    let (tx_pub_key, additional_keys) = extra.keys()?;
    let mut keys = core::iter::once(Some(tx_pub_key))
      .chain(core::iter::once(additional_keys.and_then(|keys| keys.get(output_index).copied())));

    while let Some(Some(key)) = keys.next() {
      if c ==
        Self::challenge(
          address,
          s_commitment_generator + (c * key),
          s_commitment_view_key + (c * ecdh),
          key,
          ecdh,
          message,
        )
      {
        let output = tx.prefix.outputs.get(output_index)?;

        let shared_key_derivations = SharedKeyDerivations::output_derivations(
          address.is_guaranteed().then(|| SharedKeyDerivations::uniqueness(&tx.prefix.inputs)),
          ecdh,
          output_index,
        );

        if let Some(actual_view_tag) = output.view_tag {
          if actual_view_tag != shared_key_derivations.view_tag {
            None?;
          }
        }

        if output.key !=
          (address.spend + (&shared_key_derivations.shared_key * &ED25519_BASEPOINT_TABLE))
            .compress()
        {
          None?;
        }

        if let Some(payment_id) = address.payment_id() {
          if PaymentId::Encrypted(payment_id) !=
            (extra.payment_id()? ^ SharedKeyDerivations::payment_id_xor(ecdh))
          {
            None?;
          }
        }

        if tx.prefix.version == 1 || output.amount != 0 {
          return Some(output.amount);
        }

        let encrypted = tx.rct_signatures.base.ecdh_info.get(output_index)?;
        let commitment = shared_key_derivations.decrypt_compact(encrypted);
        let expected = tx.rct_signatures.base.commitments.get(output_index)?;
        if commitment.calculate() != *expected {
          None?;
        }

        return Some(commitment.amount);
      }
    }

    None
  }

  /// Serialize a series of OutProofs to a single string.
  pub fn serialize(proofs: &[Self]) -> String {
    let mut res = String::with_capacity(10 + (proofs.len() * 96));
    res.push_str("OutProofV2");
    for proof in proofs {
      let OutProof { ecdh, c, s } = proof;
      let mut signature = c.to_bytes().to_vec();
      signature.extend(&s.to_bytes());
      res.push_str(&base58::encode(&ecdh.compress().to_bytes()));
      res.push_str(&base58::encode(&signature));
    }
    res
  }

  /// Read a Vec of OutProofs from a str.
  pub fn read(proofs: &str) -> Option<Vec<Self>> {
    let ecdh_len = base58::encoded_len_for_bytes(32);
    let signature_len = base58::encoded_len_for_bytes(64);

    let mut res = vec![];
    if let Some(mut proofs) = proofs.strip_prefix("OutProofV2") {
      while !proofs.is_empty() {
        if proofs.len() < (ecdh_len + signature_len) {
          None?;
        }
        let ecdh = base58::decode(proofs.get(.. ecdh_len)?)?;
        #[allow(clippy::string_slice)]
        {
          proofs = &proofs[ecdh_len ..];
        }

        let signature = base58::decode(proofs.get(.. signature_len)?)?;
        #[allow(clippy::string_slice)]
        {
          proofs = &proofs[signature_len ..];
        }

        res.push(OutProof {
          ecdh: read_point(&mut ecdh.as_slice()).ok()?,
          c: read_scalar(&mut &signature[.. 32]).ok()?,
          s: read_scalar(&mut &signature[32 ..]).ok()?,
        });
      }
    }
    Some(res)
  }
}

impl fmt::Display for OutProof {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let OutProof { ecdh, c, s } = self;
    let mut signature = c.to_bytes().to_vec();
    signature.extend(&s.to_bytes());
    write!(
      f,
      "OutProofV2{}{}",
      base58::encode(&ecdh.compress().to_bytes()),
      base58::encode(&signature)
    )
  }
}
