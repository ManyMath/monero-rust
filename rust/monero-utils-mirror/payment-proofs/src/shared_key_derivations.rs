// Adapted from monero-wallet's internal shared key derivation routines.
// Helpers (keccak256, write_varint, Commitment) are inlined here since
// monero-serai-mirror keeps them pub(crate).

use std_shims::vec::Vec;

use zeroize::{Zeroize, Zeroizing};

use sha3::{Digest, Keccak256};
use curve25519_dalek::{
  constants::ED25519_BASEPOINT_TABLE,
  scalar::Scalar,
  edwards::EdwardsPoint,
};

use crate::monero_backend::{transaction::Input, H};

// --- inlined helpers ---

fn keccak256(data: impl AsRef<[u8]>) -> [u8; 32] {
  Keccak256::digest(data.as_ref()).into()
}

fn keccak256_to_scalar(data: impl AsRef<[u8]>) -> Scalar {
  Scalar::from_bytes_mod_order(keccak256(data))
}

fn write_varint<W: std::io::Write>(varint: &u64, w: &mut W) -> std::io::Result<()> {
  let mut varint = *varint;
  loop {
    let mut b = (varint & 0x7f) as u8;
    varint >>= 7;
    if varint != 0 {
      b |= 0x80;
    }
    w.write_all(&[b])?;
    if varint == 0 {
      break;
    }
  }
  Ok(())
}

// --- Commitment ---

/// A Pedersen commitment: `mask * G + amount * H`.
pub(crate) struct Commitment {
  pub(crate) amount: u64,
  mask: Scalar,
}

impl Commitment {
  pub(crate) fn new(mask: Scalar, amount: u64) -> Self {
    Commitment { mask, amount }
  }

  pub(crate) fn calculate(&self) -> EdwardsPoint {
    &self.mask * ED25519_BASEPOINT_TABLE + Scalar::from(self.amount) * *H
  }
}

// --- SharedKeyDerivations ---

#[derive(Clone, PartialEq, Eq, Zeroize)]
pub(crate) struct SharedKeyDerivations {
  pub(crate) view_tag: u8,
  pub(crate) shared_key: Scalar,
}

impl SharedKeyDerivations {
  // https://gist.github.com/kayabaNerve/8066c13f1fe1573286ba7a2fd79f6100
  pub(crate) fn uniqueness(inputs: &[Input]) -> [u8; 32] {
    let mut u = b"uniqueness".to_vec();
    for input in inputs {
      match input {
        Input::Gen(height) => {
          write_varint(height, &mut u).unwrap();
        }
        Input::ToKey { key_image, .. } => u.extend(key_image.compress().to_bytes()),
      }
    }
    keccak256(u)
  }

  pub(crate) fn output_derivations(
    uniqueness: Option<[u8; 32]>,
    ecdh: EdwardsPoint,
    o: usize,
  ) -> Zeroizing<SharedKeyDerivations> {
    // 8Ra (cofactor multiply then compress)
    let cofactored = ecdh.mul_by_cofactor();
    let mut output_derivation = Zeroizing::new(cofactored.compress().to_bytes().to_vec());

    // || o
    {
      let od: &mut Vec<u8> = output_derivation.as_mut();
      write_varint(&(o as u64), od).unwrap();
    }

    let view_tag = keccak256([b"view_tag".as_ref(), &output_derivation].concat())[0];

    let output_derivation = if let Some(uniqueness) = uniqueness {
      Zeroizing::new([uniqueness.as_ref(), &output_derivation].concat())
    } else {
      output_derivation
    };

    Zeroizing::new(SharedKeyDerivations {
      view_tag,
      shared_key: keccak256_to_scalar(&output_derivation),
    })
  }

  pub(crate) fn payment_id_xor(ecdh: EdwardsPoint) -> [u8; 8] {
    let cofactored = ecdh.mul_by_cofactor();
    let output_derivation = Zeroizing::new(cofactored.compress().to_bytes().to_vec());

    let mut payment_id_xor = [0; 8];
    payment_id_xor
      .copy_from_slice(&keccak256([output_derivation.as_ref(), [0x8d].as_ref()].concat())[.. 8]);
    payment_id_xor
  }

  pub(crate) fn commitment_mask(&self) -> Scalar {
    let mut mask = b"commitment_mask".to_vec();
    mask.extend(self.shared_key.as_bytes());
    let res = keccak256_to_scalar(&mask);
    mask.zeroize();
    res
  }

  pub(crate) fn compact_amount_encryption(&self, amount: u64) -> [u8; 8] {
    let mut amount_mask = Zeroizing::new(b"amount".to_vec());
    amount_mask.extend(self.shared_key.to_bytes());
    let mut amount_mask = keccak256(&amount_mask);

    let mut amount_mask_8 = [0; 8];
    amount_mask_8.copy_from_slice(&amount_mask[.. 8]);
    amount_mask.zeroize();

    (amount ^ u64::from_le_bytes(amount_mask_8)).to_le_bytes()
  }

  /// Decrypt a compact encrypted amount (our fork's RctBase.ecdh_info stores `[u8; 8]`).
  pub(crate) fn decrypt_compact(&self, encrypted: &[u8; 8]) -> Commitment {
    Commitment::new(
      self.commitment_mask(),
      u64::from_le_bytes(self.compact_amount_encryption(u64::from_le_bytes(*encrypted))),
    )
  }
}
