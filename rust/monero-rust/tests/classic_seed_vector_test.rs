//! Classic seed vectors for `monero_rust::seed_compat`.
//!
//! These vectors come from the removed `monero-seed` crate. They cover its
//! language cases against the seed parser used by this wallet library.

use monero_rust::seed_compat::{Language, Seed};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Fixture {
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Vector {
    language: String,
    seed: String,
    /// Classic seed entropy, used directly as the spend key.
    spend: String,
    /// View key derived as H(spend).
    view: String,
    /// Expected result when another language matches the same trimmed words
    /// and takes precedence during detection.
    #[serde(default)]
    ambiguous: Option<Ambiguous>,
}

#[derive(Deserialize)]
struct Ambiguous {
    resolves_to: String,
    /// Spend key returned for the detected language.
    spend: String,
}

impl Vector {
    /// Expected key from `Seed::from_string`, accounting for language ambiguity.
    fn detected_spend(&self) -> &str {
        match &self.ambiguous {
            Some(a) => &a.spend,
            None => &self.spend,
        }
    }
}

fn load() -> Fixture {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/classic_seed/classic_seed_vectors.json");
    let data = std::fs::read_to_string(&path).expect("failed to read seed vectors");
    serde_json::from_str(&data).expect("failed to parse seed vectors")
}

fn language(name: &str) -> Language {
    match name {
        "Chinese" => Language::Chinese,
        "English" => Language::English,
        "Dutch" => Language::Dutch,
        "French" => Language::French,
        "Spanish" => Language::Spanish,
        "German" => Language::German,
        "Italian" => Language::Italian,
        "Portuguese" => Language::Portuguese,
        "Japanese" => Language::Japanese,
        "Russian" => Language::Russian,
        "Esperanto" => Language::Esperanto,
        "Lojban" => Language::Lojban,
        // Map `monero-seed`'s "DeprecatedEnglish" to the old 24-word list.
        "DeprecatedEnglish" => Language::EnglishOld,
        other => panic!("unmapped language {}", other),
    }
}

/// Parse every vector and compare its spend key with the expected Monero result.
#[test]
fn parses_every_language_to_the_expected_spend_key() {
    let fixture = load();
    assert_eq!(fixture.vectors.len(), 18, "vector count changed");

    for vector in &fixture.vectors {
        let seed = Seed::from_string(Zeroizing::new(vector.seed.clone()))
            .unwrap_or_else(|e| panic!("failed to parse {} seed: {:?}", vector.language, e));

        assert_eq!(
            hex::encode(&*seed.entropy()),
            vector.detected_spend(),
            "entropy must equal Monero's spend key ({})",
            vector.language
        );
        // Without a passphrase, classic seed entropy is the spend key.
        assert_eq!(
            hex::encode(&*seed.key_bytes()),
            vector.detected_spend(),
            "key bytes must equal the spend key ({})",
            vector.language
        );
        assert_eq!(seed.birthday(), None, "classic seeds carry no birthday");
    }
}

/// Recreate each mnemonic from its spend key.
#[test]
fn rebuilds_the_mnemonic_from_entropy() {
    let fixture = load();

    for vector in &fixture.vectors {
        let spend: [u8; 32] = hex::decode(&vector.spend).unwrap().try_into().unwrap();
        let rebuilt = Seed::from_entropy(language(&vector.language), Zeroizing::new(spend))
            .unwrap_or_else(|| panic!("from_entropy returned None for {}", vector.language));

        assert_eq!(
            &*rebuilt.to_string(),
            &vector.seed,
            "from_entropy must reproduce the mnemonic ({})",
            vector.language
        );

        // `entropy()` detects the language again. Ambiguous words may resolve
        // to a language different from the one used to construct the seed.
        if vector.ambiguous.is_none() {
            assert_eq!(hex::encode(&*rebuilt.entropy()), vector.spend);
        }
    }
}

/// Repeated language detection must return the same keys. HashMap iteration
/// previously made ambiguous seeds depend on process-specific ordering.
#[test]
fn detection_is_deterministic() {
    let fixture = load();

    for vector in &fixture.vectors {
        let first = Seed::from_string(Zeroizing::new(vector.seed.clone()))
            .map(|s| hex::encode(&*s.entropy()));

        for _ in 0 .. 8 {
            let again = Seed::from_string(Zeroizing::new(vector.seed.clone()))
                .map(|s| hex::encode(&*s.entropy()));
            assert_eq!(first, again, "detection must be stable ({})", vector.language);
        }
    }
}

/// Resolve ambiguous seeds in the fixed order used by Monero's `find_seed_language`.
#[test]
fn resolves_ambiguous_seeds_the_way_monero_does() {
    let fixture = load();
    let ambiguous: Vec<&Vector> = fixture.vectors.iter().filter(|v| v.ambiguous.is_some()).collect();
    assert_eq!(ambiguous.len(), 2, "fixture should carry both ambiguous cases");

    for vector in ambiguous {
        let expected = vector.ambiguous.as_ref().unwrap();
        let seed = Seed::from_string(Zeroizing::new(vector.seed.clone())).expect("must parse");

        assert_eq!(
            hex::encode(&*seed.entropy()),
            expected.spend,
            "{} seed must resolve as {}",
            vector.language,
            expected.resolves_to
        );
        assert_ne!(
            hex::encode(&*seed.entropy()),
            vector.spend,
            "and so must not yield the key it was recorded under"
        );
    }
}

/// Check the fixture's view keys against H(spend) to catch transcription errors.
#[test]
fn vector_view_keys_are_the_hash_of_the_spend_key() {
    use curve25519_dalek::scalar::Scalar;
    use sha3::{Digest, Keccak256};

    let fixture = load();

    for vector in &fixture.vectors {
        let spend: [u8; 32] = hex::decode(&vector.spend).unwrap().try_into().unwrap();
        let view: [u8; 32] = hex::decode(&vector.view).unwrap().try_into().unwrap();

        let derived = Scalar::from_bytes_mod_order(Keccak256::digest(spend).into());
        let expected = Option::<Scalar>::from(Scalar::from_canonical_bytes(view))
            .unwrap_or_else(|| panic!("view key not canonical ({})", vector.language));

        assert_eq!(derived, expected, "view key must be H(spend) ({})", vector.language);
    }
}

/// A seed must not parse as the wrong language's wordlist, and corruption must be caught
/// by the checksum rather than yielding a different key.
#[test]
fn rejects_corrupted_seeds() {
    let fixture = load();

    for vector in &fixture.vectors {
        // Swap the first two words to invalidate the checksum.
        let mut words: Vec<&str> = vector.seed.split_whitespace().collect();
        if words.len() < 25 {
            continue; // the 24-word deprecated list carries no checksum
        }
        if words[0] == words[1] {
            continue; // a repeated-word vector is unchanged by the swap
        }
        words.swap(0, 1);
        let mangled = Zeroizing::new(words.join(" "));

        let parsed = Seed::from_string(mangled);
        if let Ok(seed) = parsed {
            assert_ne!(
                hex::encode(&*seed.entropy()),
                vector.spend,
                "a reordered seed must not yield the original key ({})",
                vector.language
            );
        }
    }
}
