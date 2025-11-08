use zeroize::Zeroizing;
use rand_core::{RngCore, CryptoRng, Error};

use monero_wallet_util::seed::{Seed, SeedType, SeedError, original, polyseed};

/// Deterministic RNG so tests are reproducible without pulling in a rand dev-dependency.
struct TestRng(u64);
impl RngCore for TestRng {
  fn next_u32(&mut self) -> u32 {
    self.next_u64() as u32
  }
  fn next_u64(&mut self) -> u64 {
    // xorshift64
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
  fn fill_bytes(&mut self, dest: &mut [u8]) {
    for chunk in dest.chunks_mut(8) {
      let bytes = self.next_u64().to_le_bytes();
      chunk.copy_from_slice(&bytes[.. chunk.len()]);
    }
  }
  fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
    self.fill_bytes(dest);
    Ok(())
  }
}
impl CryptoRng for TestRng {}

const ORIGINAL: SeedType = SeedType::Original(original::Language::English);
const POLYSEED: SeedType = SeedType::Polyseed(polyseed::Language::English);

#[test]
fn original_seed_string_roundtrip() {
  let seed = Seed::new(&mut TestRng(1), ORIGINAL);
  let words = seed.to_string();
  assert_eq!(words.split_whitespace().count(), 25);

  let parsed = Seed::from_string(ORIGINAL, words).unwrap();
  assert_eq!(seed, parsed);
  assert_eq!(seed.entropy(), parsed.entropy());
  // The original seed format doesn't differentiate between entropy and key
  assert_eq!(seed.key(), seed.entropy());
  assert_eq!(seed.birthday(), 0);
}

#[test]
fn polyseed_string_roundtrip() {
  let seed = Seed::new(&mut TestRng(2), POLYSEED);
  let words = seed.to_string();
  assert_eq!(words.split_whitespace().count(), 16);

  let parsed = Seed::from_string(POLYSEED, words).unwrap();
  assert_eq!(seed, parsed);
  assert_eq!(seed.entropy(), parsed.entropy());
  assert_eq!(seed.key(), parsed.key());
  // Polyseed derives its key from its entropy
  assert!(seed.key() != seed.entropy());
}

#[test]
fn original_seed_from_entropy() {
  // A canonical scalar (high byte clear) is valid entropy
  let mut entropy = [1; 32];
  entropy[31] = 0;
  let entropy = Zeroizing::new(entropy);

  let seed = Seed::from_entropy(ORIGINAL, entropy.clone(), None).unwrap();
  assert_eq!(seed.entropy(), entropy);

  // The birthday is ignored for the original seed format
  let with_birthday = Seed::from_entropy(ORIGINAL, entropy, Some(1700000000)).unwrap();
  assert_eq!(seed, with_birthday);
  assert_eq!(with_birthday.birthday(), 0);
}

#[test]
fn original_seed_from_invalid_entropy() {
  // A non-canonical scalar is invalid entropy
  let entropy = Zeroizing::new([0xff; 32]);
  assert_eq!(Seed::from_entropy(ORIGINAL, entropy, None), Err(SeedError::InvalidEntropy));
}

#[test]
fn polyseed_from_entropy() {
  // Polyseed entropy is 19 bytes, with the last 13 bytes of the 32-byte array zero
  let mut entropy = [0; 32];
  entropy[.. 19].copy_from_slice(&[3; 19]);
  let entropy = Zeroizing::new(entropy);

  const BIRTHDAY: u64 = 1700000000;
  let seed = Seed::from_entropy(POLYSEED, entropy.clone(), Some(BIRTHDAY)).unwrap();
  assert_eq!(seed.entropy(), entropy);

  // The birthday is stored at month granularity, rounded down
  assert!(seed.birthday() <= BIRTHDAY);
  assert!((BIRTHDAY - seed.birthday()) < 2_700_000);

  // The seed string preserves entropy and birthday
  let parsed = Seed::from_string(POLYSEED, seed.to_string()).unwrap();
  assert_eq!(seed, parsed);
  assert_eq!(parsed.birthday(), seed.birthday());
}

#[test]
fn polyseed_from_invalid_entropy() {
  // Entropy whose last 13 bytes aren't zero is invalid for Polyseed
  let entropy = Zeroizing::new([3; 32]);
  assert_eq!(
    Seed::from_entropy(POLYSEED, entropy, None),
    Err(SeedError::InvalidEntropy),
  );
}

#[test]
fn debug_does_not_leak_seed_material() {
  let seed = Seed::new(&mut TestRng(4), POLYSEED);
  let debug = format!("{:?}", seed);
  assert!(debug.contains("Seed::Polyseed"));
  let first_word = seed.to_string().split_whitespace().next().unwrap().to_string();
  assert!(!debug.contains(&first_word));
}
