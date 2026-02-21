use core::ops::Deref;
use std::io::{self, Read, Write};

use rand_core::{RngCore, CryptoRng};

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use curve25519_dalek::{
  constants::ED25519_BASEPOINT_TABLE,
  scalar::Scalar,
  edwards::EdwardsPoint,
};

use crate::{
  Protocol,
  serialize::{
    write_byte, write_varint, write_vec,
    read_byte, read_varint, read_bytes, read_vec,
  },
  ringct::{
    generate_key_image,
    clsag::{ClsagInput, Clsag},
    RctPrunable,
  },
  transaction::{Input, Transaction},
  wallet::{
    SpendableOutput, Decoys, key_image_sort, uniqueness,
  },
};

use super::{SignableTransaction, InternalPayment, TransactionError};

#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct UnsignedInput {
  pub output: SpendableOutput,
  pub decoys: Decoys,
}

#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct UnsignedTransaction {
  pub protocol: Protocol,
  pub r_seed: Zeroizing<[u8; 32]>,
  pub fee: u64,
  pub payments: Vec<InternalPayment>,
  pub data: Vec<Vec<u8>>,
  pub inputs: Vec<UnsignedInput>,
}

impl UnsignedTransaction {
  pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
    self.protocol.write(w)?;
    w.write_all(self.r_seed.as_ref())?;
    w.write_all(&self.fee.to_le_bytes())?;

    fn write_payment<W: Write>(payment: &InternalPayment, w: &mut W) -> io::Result<()> {
      match payment {
        InternalPayment::Payment(payment) => {
          w.write_all(&[0])?;
          write_vec(write_byte, payment.0.to_string().as_bytes(), w)?;
          w.write_all(&payment.1.to_le_bytes())
        }
        InternalPayment::Change(change, amount) => {
          w.write_all(&[1])?;
          write_vec(write_byte, change.address().to_string().as_bytes(), w)?;
          if let Some(view) = change.view() {
            w.write_all(&[1])?;
            crate::serialize::write_scalar(view, w)?;
          } else {
            w.write_all(&[0])?;
          }
          w.write_all(&amount.to_le_bytes())
        }
      }
    }
    write_vec(write_payment, &self.payments, w)?;

    write_varint(&(self.data.len() as u64), w)?;
    for chunk in &self.data {
      write_vec(write_byte, chunk, w)?;
    }

    write_varint(&(self.inputs.len() as u64), w)?;
    for input in &self.inputs {
      input.output.write(w)?;
      input.decoys.write(w)?;
    }

    Ok(())
  }

  pub fn serialize(&self) -> Vec<u8> {
    let mut buf = Vec::new();
    self.write(&mut buf).unwrap();
    buf
  }

  pub fn read<R: Read>(r: &mut R) -> io::Result<UnsignedTransaction> {
    use crate::serialize::{read_u64, read_scalar};
    use crate::wallet::address::MoneroAddress;
    use super::Change;

    let protocol = Protocol::read(r)?;
    let r_seed = Zeroizing::new(read_bytes::<_, 32>(r)?);
    let fee = read_u64(r)?;

    fn read_payment<R: Read>(r: &mut R) -> io::Result<InternalPayment> {
      fn read_address<R: Read>(r: &mut R) -> io::Result<MoneroAddress> {
        String::from_utf8(read_vec(read_byte, r)?)
          .ok()
          .and_then(|str| MoneroAddress::from_str_raw(&str).ok())
          .ok_or(io::Error::new(io::ErrorKind::Other, "invalid address"))
      }

      Ok(match read_byte(r)? {
        0 => InternalPayment::Payment((read_address(r)?, read_u64(r)?)),
        1 => InternalPayment::Change(
          Change::from_raw(
            read_address(r)?,
            match read_byte(r)? {
              0 => None,
              1 => Some(Zeroizing::new(read_scalar(r)?)),
              _ => Err(io::Error::new(io::ErrorKind::Other, "invalid change payment"))?,
            },
          ),
          read_u64(r)?,
        ),
        _ => Err(io::Error::new(io::ErrorKind::Other, "invalid payment"))?,
      })
    }

    let payments = read_vec(read_payment, r)?;

    let data_len: usize = read_varint(r)?
      .try_into()
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "data length overflow"))?;
    if data_len > 1_000_000 {
      return Err(io::Error::new(io::ErrorKind::InvalidData, "data length exceeds limit"));
    }
    let mut data = Vec::with_capacity(data_len);
    for _ in 0 .. data_len {
      data.push(read_vec(read_byte, r)?);
    }

    let inputs_len: usize = read_varint(r)?
      .try_into()
      .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "inputs length overflow"))?;
    if inputs_len > 1000 {
      return Err(io::Error::new(io::ErrorKind::InvalidData, "inputs count exceeds limit"));
    }
    let mut inputs = Vec::with_capacity(inputs_len);
    for _ in 0 .. inputs_len {
      inputs.push(UnsignedInput {
        output: SpendableOutput::read(r)?,
        decoys: Decoys::read(r)?,
      });
    }

    Ok(UnsignedTransaction { protocol, r_seed, fee, payments, data, inputs })
  }
}

pub fn sign_offline<R: RngCore + CryptoRng>(
  rng: &mut R,
  spend: &Zeroizing<Scalar>,
  unsigned: UnsignedTransaction,
) -> Result<(Transaction, Zeroizing<Scalar>, Vec<Zeroizing<Scalar>>), TransactionError> {
  for input in &unsigned.inputs {
    let offset = Zeroizing::new(spend.deref() + input.output.key_offset());
    if (offset.deref() * ED25519_BASEPOINT_TABLE) != input.output.key() {
      return Err(TransactionError::WrongPrivateKey);
    }
  }

  let mut images = Vec::with_capacity(unsigned.inputs.len());
  for input in &unsigned.inputs {
    let offset = Zeroizing::new(spend.deref() + input.output.key_offset());
    images.push(generate_key_image(&offset));
  }
  images.sort_by(key_image_sort);

  let uniqueness = uniqueness(
    &images
      .iter()
      .map(|image| Input::ToKey { amount: 0, key_offsets: vec![], key_image: *image })
      .collect::<Vec<_>>(),
  );

  let spendable_outputs: Vec<SpendableOutput> =
    unsigned.inputs.iter().map(|i| i.output.clone()).collect();

  let mut signable = SignableTransaction::from_parts(
    unsigned.protocol,
    Some(unsigned.r_seed.clone()),
    spendable_outputs,
    unsigned.payments.clone(),
    unsigned.data.clone(),
    unsigned.fee,
  );

  // Extract the eventuality before prepare_transaction mutates payments
  let eventuality = signable.eventuality();
  let (tx_key, tx_key_additional) = match &eventuality {
    Some(ev) => (Zeroizing::new(*ev.tx_key()), ev.tx_key_additional().to_vec()),
    None => return Err(TransactionError::NoInputs),
  };

  let (mut tx, mask_sum) = signable.prepare_transaction(rng, uniqueness);

  let mut clsag_inputs: Vec<(Zeroizing<Scalar>, EdwardsPoint, ClsagInput)> =
    Vec::with_capacity(unsigned.inputs.len());

  for (i, unsigned_input) in unsigned.inputs.iter().enumerate() {
    let input_spend = Zeroizing::new(unsigned_input.output.key_offset() + spend.deref());
    let image = generate_key_image(&input_spend);
    let clsag_input = ClsagInput::new(
      unsigned_input.output.commitment().clone(),
      unsigned_input.decoys.clone(),
    )
    .map_err(TransactionError::ClsagError)?;

    clsag_inputs.push((input_spend, image, clsag_input));

    tx.prefix.inputs.push(Input::ToKey {
      amount: 0,
      key_offsets: unsigned_input.decoys.offsets.clone(),
      key_image: clsag_inputs[i].1,
    });
  }

  // Sort inputs by key image (descending)
  clsag_inputs
    .sort_by(|x, y| x.1.compress().to_bytes().cmp(&y.1.compress().to_bytes()).reverse());
  tx.prefix.inputs.sort_by(|x, y| {
    if let (Input::ToKey { key_image: x, .. }, Input::ToKey { key_image: y, .. }) = (x, y) {
      x.compress().to_bytes().cmp(&y.compress().to_bytes()).reverse()
    } else {
      // unreachable: all inputs are ToKey
      std::cmp::Ordering::Equal
    }
  });

  let clsag_pairs = Clsag::sign(rng, clsag_inputs, mask_sum, tx.signature_hash());
  match tx.rct_signatures.prunable {
    RctPrunable::Null => panic!("Signing for RctPrunable::Null"),
    RctPrunable::Clsag { ref mut clsags, ref mut pseudo_outs, .. } => {
      clsags.append(&mut clsag_pairs.iter().map(|clsag| clsag.0.clone()).collect::<Vec<_>>());
      pseudo_outs.append(&mut clsag_pairs.iter().map(|clsag| clsag.1).collect::<Vec<_>>());
    }
  }

  Ok((tx, tx_key, tx_key_additional))
}
