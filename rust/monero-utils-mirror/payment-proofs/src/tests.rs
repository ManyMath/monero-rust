use zeroize::Zeroizing;
use rand_core::{OsRng, RngCore};

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

use monero_serai::wallet::{
  ViewPair,
  address::{Network, AddressSpec},
};

use crate::OutProof;

fn random_scalar(rng: &mut impl RngCore) -> Scalar {
  let mut wide = [0u8; 64];
  rng.fill_bytes(&mut wide);
  Scalar::from_bytes_mod_order_wide(&wide)
}

#[test]
fn out_proof_serialization() {
  let spend_key = Zeroizing::new(random_scalar(&mut OsRng));
  let view_key = Zeroizing::new(random_scalar(&mut OsRng));
  let view_pair = ViewPair::new(ED25519_BASEPOINT_TABLE * &*spend_key, view_key);

  let ephemeral_key = Zeroizing::new(random_scalar(&mut OsRng));

  let address = view_pair.address(Network::Mainnet, AddressSpec::Standard);
  let proof = OutProof::prove(&mut OsRng, &address, &ephemeral_key, &[]);

  let mut proofs = vec![];
  for _ in 0 .. 5 {
    assert_eq!(&OutProof::read(&OutProof::serialize(&proofs)).unwrap(), &proofs);
    proofs.push(proof);
  }
}
