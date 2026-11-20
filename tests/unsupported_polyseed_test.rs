//! Reduced builds reject Polyseed before deriving any wallet keys.

use monero_rust::{
    derive_address, derive_keys, derive_subaddress, resolve_seed, resolve_seed_bip39,
    seed_birthday,
    seed_compat::{Seed, SeedError},
    validate_seed, Network,
};
use zeroize::Zeroizing;

#[test]
fn rejects_plain_and_encrypted_polyseed_in_wallet_entry_points() {
    // The second phrase is the first encrypted with the password "CakeWallet".
    // Both are fixed synthetic compatibility vectors, not operational wallets.
    for words in [
        "unaware yard donate shallow slot sing oil oxygen loyal bench near hill surround forum execute lamp",
        "arm dutch crystal reduce elephant mix squeeze garlic slam brand tent seed rubber fame summer sample",
    ] {
        assert_eq!(
            Seed::from_string(Zeroizing::new(words.into())),
            Err(SeedError::UnsupportedPolyseed)
        );
        assert!(validate_seed(words).is_err());
        assert!(resolve_seed(words).is_err());
        assert_eq!(seed_birthday(words), None);
        for passphrase in ["", "CakeWallet", "wrong"] {
            assert!(resolve_seed_bip39(words, passphrase, 3).is_err());
            assert!(derive_keys(words, Network::Stagenet, passphrase).is_err());
            assert!(derive_address(words, Network::Stagenet, passphrase).is_err());
            assert!(derive_subaddress(words, Network::Stagenet, 0, 1, passphrase).is_err());
        }
    }
}
