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
        use polyseed::{Polyseed, Language as PolyseedLanguage};
        Polyseed::from_string(PolyseedLanguage::English, words)
          .map(Seed::Polyseed)
          .map_err(|_| SeedError::InvalidSeed)
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
      Seed::Polyseed(seed) => Zeroizing::new(seed.to_string().to_string()),
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
      Seed::Polyseed(seed) => seed.key(),
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
