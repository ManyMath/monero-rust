use zeroize::Zeroizing;
use rand_core::{OsRng, RngCore};

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

use crate::monero_backend::wallet::{
  ViewPair,
  ed25519::{Point, Scalar as OxideScalar},
  address::Network,
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
  let view_key = Zeroizing::new(OxideScalar::from(random_scalar(&mut OsRng)));
  let view_pair =
    ViewPair::new(Point::from(ED25519_BASEPOINT_TABLE * &*spend_key), view_key).unwrap();

  let ephemeral_key = Zeroizing::new(random_scalar(&mut OsRng));

  let address = view_pair.legacy_address(Network::Mainnet);
  let proof = OutProof::prove(&mut OsRng, &address, &ephemeral_key, &[]);

  let mut proofs = vec![];
  for _ in 0 .. 5 {
    assert_eq!(&OutProof::read(&OutProof::serialize(&proofs)).unwrap(), &proofs);
    proofs.push(proof);
  }
}
