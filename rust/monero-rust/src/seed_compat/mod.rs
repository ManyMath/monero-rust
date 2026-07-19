//! Monero seed handling, ported from the vendored serai mirror.
//!
//! Wraps classic (25-word) seeds and 16-word polyseeds behind one type,
//! including Monero's seed-offset passphrase mechanism.

// The facade cutover repoints monero_backend::wallet::seed here.
#![allow(dead_code)]

use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};
use rand_core::{RngCore, CryptoRng};

use thiserror::Error;

pub(crate) mod classic;
use classic::{CLASSIC_SEED_LENGTH, CLASSIC_SEED_LENGTH_WITH_CHECKSUM, ClassicSeed};

/// Error when decoding a seed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Error)]
pub enum SeedError {
  #[error("invalid number of words in seed")]
  InvalidSeedLength,
  #[error("unknown language")]
  UnknownLanguage,
  #[error("invalid checksum")]
  InvalidChecksum,
  #[error("english old seeds don't support checksums")]
  EnglishOldWithChecksum,
  #[error("invalid seed")]
  InvalidSeed,
  #[error("encrypted Polyseed is not supported; decrypt it with a Polyseed-compatible tool before importing")]
  EncryptedPolyseed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Language {
  Chinese,
  English,
  Dutch,
  French,
  Spanish,
  German,
  Italian,
  Portuguese,
  Japanese,
  Russian,
  Esperanto,
  Lojban,
  EnglishOld,
}

/// A Monero seed.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub enum Seed {
  Classic(ClassicSeed),
  Polyseed(polyseed::Polyseed),
}

impl fmt::Debug for Seed {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Seed::Classic(_) => f.debug_struct("Seed::Classic").finish_non_exhaustive(),
      Seed::Polyseed(_) => f.debug_struct("Seed::Polyseed").finish_non_exhaustive(),
    }
  }
}

impl Seed {
  /// Create a new seed.
  pub fn new<R: RngCore + CryptoRng>(rng: &mut R, lang: Language) -> Seed {
    Seed::Classic(ClassicSeed::new(rng, lang))
  }

  /// Create a new polyseed.
  pub fn new_polyseed<R: RngCore + CryptoRng>(rng: &mut R) -> Seed {
    use polyseed::{Polyseed, Language};
    Seed::Polyseed(Polyseed::new(rng, Language::English))
  }

  /// Parse a seed from a String.
  pub fn from_string(words: Zeroizing<String>) -> Result<Seed, SeedError> {
    let word_count = words.split_whitespace().count();
    match word_count {
      16 => {
        use polyseed::{Coin, Polyseed, Language as PolyseedLanguage};
        let seed = Polyseed::from_string(PolyseedLanguage::English, words, Coin::Monero, 0)
            .map_err(|_| SeedError::InvalidSeed)?;
        // A classic seed-offset passphrase does not decrypt Polyseed.
        // Never silently derive a different wallet from encrypted entropy.
        if seed.is_encrypted() {
            return Err(SeedError::EncryptedPolyseed);
        }
        Ok(Seed::Polyseed(seed))
      }
      CLASSIC_SEED_LENGTH | CLASSIC_SEED_LENGTH_WITH_CHECKSUM => {
        ClassicSeed::from_string(words).map(Seed::Classic)
      }
      _ => Err(SeedError::InvalidSeedLength)?,
    }
  }

  /// Create a Seed from entropy.
  pub fn from_entropy(lang: Language, entropy: Zeroizing<[u8; 32]>) -> Option<Seed> {
    ClassicSeed::from_entropy(lang, entropy).map(Seed::Classic)
  }

  /// Convert a seed to a String.
  pub fn to_string(&self) -> Zeroizing<String> {
    match self {
      Seed::Classic(seed) => seed.to_string(),
      Seed::Polyseed(seed) => Zeroizing::new(seed.to_string(polyseed::Coin::Monero).to_string()),
    }
  }

  /// Return the entropy for this seed.
  pub fn entropy(&self) -> Zeroizing<[u8; 32]> {
    match self {
      Seed::Classic(seed) => seed.entropy(),
      Seed::Polyseed(seed) => seed.entropy().clone(),
    }
  }

  /// Return the key bytes for this seed (entropy for Classic, PBKDF2-derived for Polyseed).
  pub fn key_bytes(&self) -> Zeroizing<[u8; 32]> {
    match self {
      Seed::Classic(seed) => seed.entropy(),
      Seed::Polyseed(seed) => seed.key(polyseed::Coin::Monero),
    }
  }

  /// Return the key bytes with an optional seed offset passphrase.
  ///
  /// This implements Monero's seed offset mechanism (`cryptonote::decrypt_key`):
  /// `final_key = base_key - cn_slow_hash(passphrase)` (ed25519 scalar subtraction).
  /// The offset applies to both Classic (25-word) and Polyseed (16-word) seeds.
  /// When the passphrase is empty, this returns the same result as `key_bytes()`.
  pub fn key_bytes_with_passphrase(&self, passphrase: &str) -> Zeroizing<[u8; 32]> {
    let base_key = self.key_bytes();
    if passphrase.is_empty() {
      return base_key;
    }

    // Monero's decrypt_key: sc_sub(key, key, cn_slow_hash(passphrase))
    use curve25519_dalek::scalar::Scalar;
    let hash = cuprate_cryptonight::cryptonight_hash_v0(passphrase.as_bytes());
    let key_scalar = Scalar::from_bytes_mod_order(*base_key);
    let hash_scalar = Scalar::from_bytes_mod_order(hash);
    let result = key_scalar - hash_scalar;
    Zeroizing::new(result.to_bytes())
  }

  /// Return the birthday (creation timestamp) for this seed.
  pub fn birthday(&self) -> Option<u64> {
    match self {
      Seed::Classic(_) => None,
      Seed::Polyseed(seed) => Some(seed.birthday()),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Transitional parity check while the serai mirror is still vendored: the
  /// ported seed handling must match the previous backend exactly.
  #[test]
  fn matches_mirror_seed_handling() {
    use crate::monero_backend::wallet::seed::Seed as MirrorSeed;

    for mnemonic in [
      "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden",
      "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow",
      "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime",
    ] {
      let ported = Seed::from_string(Zeroizing::new(mnemonic.to_string())).unwrap();
      let mirror = MirrorSeed::from_string(Zeroizing::new(mnemonic.to_string())).unwrap();
      assert_eq!(ported.entropy(), mirror.entropy());
      assert_eq!(ported.key_bytes(), mirror.key_bytes());
      assert_eq!(
        ported.key_bytes_with_passphrase("hunter2"),
        mirror.key_bytes_with_passphrase("hunter2")
      );
      assert_eq!(ported.to_string(), mirror.to_string());
      assert_eq!(ported.birthday(), mirror.birthday());
    }
  }

  #[test]
  fn polyseed_roundtrip_matches_mirror() {
    use crate::monero_backend::wallet::seed::Seed as MirrorSeed;
    use rand_core::SeedableRng;

    let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(7);
    let ported = Seed::new_polyseed(&mut rng);
    let words = ported.to_string();

    let mirror = MirrorSeed::from_string(words.clone()).unwrap();
    assert_eq!(ported.entropy(), mirror.entropy());
    assert_eq!(ported.key_bytes(), mirror.key_bytes());
    assert_eq!(ported.birthday(), mirror.birthday());

    let reparsed = Seed::from_string(words).unwrap();
    assert_eq!(reparsed.entropy(), ported.entropy());
  }

  #[test]
  fn classic_generation_roundtrips() {
    use rand_core::SeedableRng;
    let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(11);
    let seed = Seed::new(&mut rng, Language::English);
    let words = seed.to_string();
    assert_eq!(words.split_whitespace().count(), 25);
    let reparsed = Seed::from_string(words).unwrap();
    assert_eq!(reparsed.entropy(), seed.entropy());
  }

    #[test]
    fn encrypted_polyseed_is_rejected_before_wallet_derivation() {
        use polyseed::{Coin, Language as PolyseedLanguage, Polyseed};
        let mut entropy = [0; 32];
        entropy[0] = 7;
        let mut seed = Polyseed::from(PolyseedLanguage::English, 0, 1_700_000_000, Zeroizing::new(entropy)).unwrap();
        let words = seed.to_string(Coin::Monero);
        let expected = Seed::from_string(words.clone()).unwrap().key_bytes();
        seed.crypt("password");
        let encrypted = seed.to_string(Coin::Monero);
        assert_eq!(Seed::from_string(encrypted.clone()).unwrap_err(), SeedError::EncryptedPolyseed);
        assert!(Seed::from_string(encrypted).is_err());
        seed.crypt("password");
        assert_eq!(Seed::from_string(seed.to_string(Coin::Monero)).unwrap().key_bytes(), expected);
    }

}
