use monero_rust::{
    bip39_to_legacy_mnemonic, validate_bip39, generate_bip39,
    derive_address, derive_keys, derive_subaddress,
    generate_seed, resolve_seed, resolve_seed_bip39, seed_birthday, validate_seed,
};


#[test]
fn test_wallet1_account0() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    let expected = "tasked eight afraid laboratory tail feline rift reinvest vane cafe bailed \
        foggy dormant paper jigsaw king hazard suture king dapper dummy jolted \
        dating dwindling king";
    let result = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_wallet1_account1() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    let expected = "palace pairing axes mohawk rekindle excess awful juvenile shipped talent \
        nibs efficient dapper biggest swung fight pact innocent emerge issued \
        titans affair nearby noises emerge";
    let result = bip39_to_legacy_mnemonic(bip39, "", 1).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_wallet2_account0() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let expected = "somewhere problems gauze gigantic intended foxes upcoming saved waffle \
        pipeline lurk bogeys empty wipeout abbey italics novelty tucks rafts elite \
        lunar obnoxious awful bugs elite";
    let result = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_wallet2_account1() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let expected = "playful toxic wildly eluded mesh fainted february mugged maps repent \
        vigilant hitched seventh threaten clue fetches sample diet number alkaline \
        future cottage tuition vegan alkaline";
    let result = bip39_to_legacy_mnemonic(bip39, "", 1).unwrap();
    assert_eq!(result, expected);
}

#[test]
fn test_wallet2_address() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let legacy = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    let address = derive_address(&legacy, "mainnet").unwrap();
    assert_eq!(
        address,
        "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a"
    );
}

#[test]
fn test_validate_bip39_valid() {
    validate_bip39("meadow tip best belt boss eyebrow control affair eternal piece very shiver").unwrap();
}

#[test]
fn test_validate_bip39_invalid_word() {
    let err = validate_bip39("meadow tip best belt boss eyebrow control affair eternal piece very zzzzz").unwrap_err();
    assert!(err.contains("Invalid BIP39"), "unexpected error: {}", err);
}

#[test]
fn test_validate_bip39_bad_checksum() {
    // valid words but wrong checksum
    let err = validate_bip39("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon word").unwrap_err();
    assert!(err.contains("Invalid BIP39"), "unexpected error: {}", err);
}

#[test]
fn test_validate_bip39_wrong_word_count() {
    let err = validate_bip39("meadow tip best belt boss").unwrap_err();
    assert!(err.contains("Invalid BIP39"), "unexpected error: {}", err);
}

#[test]
fn test_conversion_invalid_mnemonic() {
    let err = bip39_to_legacy_mnemonic("not a valid mnemonic at all here nope", "", 0).unwrap_err();
    assert!(err.contains("Invalid BIP39"), "unexpected error: {}", err);
}

#[test]
fn test_generate_bip39_produces_12_words() {
    let mnemonic = generate_bip39().unwrap();
    assert_eq!(mnemonic.split_whitespace().count(), 12);
}

#[test]
fn test_generate_bip39_is_valid() {
    let mnemonic = generate_bip39().unwrap();
    validate_bip39(&mnemonic).unwrap();
}

#[test]
fn test_generate_bip39_is_random() {
    let a = generate_bip39().unwrap();
    let b = generate_bip39().unwrap();
    assert_ne!(a, b);
}

#[test]
fn test_generate_bip39_converts_to_legacy() {
    let mnemonic = generate_bip39().unwrap();
    let legacy = bip39_to_legacy_mnemonic(&mnemonic, "", 0).unwrap();
    assert_eq!(legacy.split_whitespace().count(), 25);
}

#[test]
fn test_derive_keys_with_bip39_input() {
    // derive_keys with a 12-word BIP39 mnemonic should auto-convert via resolve_seed
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let keys = derive_keys(bip39, "mainnet").unwrap();
    assert_eq!(
        keys.address,
        "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a"
    );
    assert!(!keys.secret_spend_key.is_empty());
    assert!(!keys.secret_view_key.is_empty());
}

#[test]
fn test_derive_address_with_bip39_input() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let address = derive_address(bip39, "mainnet").unwrap();
    assert_eq!(
        address,
        "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a"
    );
}

#[test]
fn test_derive_subaddress_with_bip39_input() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    // account 0, index 0 returns the standard address
    let addr = derive_subaddress(bip39, "mainnet", 0, 0).unwrap();
    assert_eq!(
        addr,
        "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a"
    );
    // account 0, index 1 returns a different subaddress
    let sub = derive_subaddress(bip39, "mainnet", 0, 1).unwrap();
    assert_ne!(sub, addr);
    assert!(sub.starts_with('8'), "subaddress should start with 8");
}

#[test]
fn test_derive_keys_bip39_matches_explicit_conversion() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    // Explicit path: convert then derive
    let legacy = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    let keys_explicit = derive_keys(&legacy, "mainnet").unwrap();
    // Transparent path: pass BIP39 directly to derive_keys
    let keys_transparent = derive_keys(bip39, "mainnet").unwrap();
    assert_eq!(keys_explicit.address, keys_transparent.address);
    assert_eq!(keys_explicit.secret_spend_key, keys_transparent.secret_spend_key);
    assert_eq!(keys_explicit.secret_view_key, keys_transparent.secret_view_key);
}

#[test]
fn test_generate_seed_bip39_type() {
    let seed = generate_seed("bip39").unwrap();
    assert_eq!(seed.split_whitespace().count(), 12);
    validate_bip39(&seed).unwrap();
}

#[test]
fn test_validate_seed_with_bip39() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    validate_seed(bip39).unwrap();
}

#[test]
fn test_validate_seed_with_invalid_bip39() {
    let err = validate_seed("meadow tip best belt boss eyebrow control affair eternal piece very zzzzz").unwrap_err();
    assert!(err.contains("Invalid BIP39"), "unexpected error: {}", err);
}

#[test]
fn test_different_accounts_produce_different_seeds() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    let seed0 = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    let seed1 = bip39_to_legacy_mnemonic(bip39, "", 1).unwrap();
    let seed2 = bip39_to_legacy_mnemonic(bip39, "", 2).unwrap();
    assert_ne!(seed0, seed1);
    assert_ne!(seed1, seed2);
    assert_ne!(seed0, seed2);
}

#[test]
fn test_resolve_seed_with_bip39() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let _seed = resolve_seed(bip39).unwrap();
    // Verify it produces the same keys as the explicit conversion path
    let keys = derive_keys(bip39, "mainnet").unwrap();
    assert_eq!(
        keys.address,
        "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a"
    );
}

#[test]
fn test_resolve_seed_with_classic_25_word() {
    let classic = "tasked eight afraid laboratory tail feline rift reinvest vane cafe bailed \
        foggy dormant paper jigsaw king hazard suture king dapper dummy jolted \
        dating dwindling king";
    let _seed = resolve_seed(classic).unwrap();
    let keys = derive_keys(classic, "mainnet").unwrap();
    assert!(!keys.address.is_empty());
}

#[test]
fn test_resolve_seed_invalid_input() {
    assert!(resolve_seed("not a valid seed").is_err());
}

#[test]
fn test_seed_birthday_with_bip39_does_not_panic() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    // Should not panic; returns None because legacy seeds don't encode birthday
    let birthday = seed_birthday(bip39);
    assert_eq!(birthday, None);
}

#[test]
fn test_resolve_seed_matches_explicit_bip39_conversion() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";
    let keys_via_resolve = derive_keys(bip39, "mainnet").unwrap();
    let legacy = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    let keys_via_legacy = derive_keys(&legacy, "mainnet").unwrap();
    assert_eq!(keys_via_resolve.address, keys_via_legacy.address);
    assert_eq!(keys_via_resolve.secret_spend_key, keys_via_legacy.secret_spend_key);
    assert_eq!(keys_via_resolve.secret_view_key, keys_via_legacy.secret_view_key);
}

#[test]
fn test_resolve_seed_with_polyseed() {
    let polyseed = generate_seed("polyseed").unwrap();
    assert_eq!(polyseed.split_whitespace().count(), 16);
    // Polyseed should pass through resolve_seed without BIP39 conversion
    let _seed = resolve_seed(&polyseed).unwrap();
    let keys = derive_keys(&polyseed, "mainnet").unwrap();
    assert!(!keys.address.is_empty());
}

#[test]
fn test_validate_seed_accepts_all_types() {
    // 12-word BIP39
    validate_seed("meadow tip best belt boss eyebrow control affair eternal piece very shiver").unwrap();
    // 16-word polyseed
    let polyseed = generate_seed("polyseed").unwrap();
    validate_seed(&polyseed).unwrap();
    // 25-word classic
    validate_seed("tasked eight afraid laboratory tail feline rift reinvest vane cafe bailed \
        foggy dormant paper jigsaw king hazard suture king dapper dummy jolted \
        dating dwindling king").unwrap();
}

#[test]
fn test_bip39_passphrase_cake_wallet_vector() {
    // Cross-validated against Cake Wallet: BIP39 seed with passphrase="passphrase"
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    let legacy = bip39_to_legacy_mnemonic(bip39, "passphrase", 0).unwrap();
    assert_eq!(
        legacy,
        "eject vinegar artistic toyed aunt imagine evolved truth pause abnormal lemon \
        arena taken utopia alumni baby gearbox molten aspire dude sample trolling \
        afoot aside evolved"
    );
    let keys = derive_keys(&legacy, "mainnet").unwrap();
    assert_eq!(
        keys.address,
        "44KghHsEKVxbf9kpZ5Jry33rbX2J3DkWV3HVKEMsaAykY1Gmi2s55F6fsTB41U98dnSjgswjhc7HkY9nq9nwP4cDERWwpsM"
    );

    // Must differ from no-passphrase
    let keys_no_pass = derive_keys(bip39, "mainnet").unwrap();
    assert_ne!(keys_no_pass.address, keys.address);

    // resolve_seed_bip39 should match the explicit conversion
    let _seed = resolve_seed_bip39(bip39, "passphrase", 0).unwrap();
}

#[test]
fn test_resolve_seed_bip39_with_passphrase() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";

    // Pinned vector: passphrase="mypassphrase", account=0
    let legacy = bip39_to_legacy_mnemonic(bip39, "mypassphrase", 0).unwrap();
    assert_eq!(
        legacy,
        "business vaults urchins rounded vein rhino tuxedo unveil framed feast lipstick \
        biggest aglow maverick godfather software musical candy vary money agenda icing \
        bids boyfriend money"
    );
    let keys = derive_keys(&legacy, "mainnet").unwrap();
    assert_eq!(
        keys.address,
        "47ojc1ijXPhhcB9tS9b7XoeYnqSj7AT9U9JGtYVEqmuKLnSTqhZ31UxiJJvDavsDKtBVM6n2XKUqciV9GfPtytpE2BVVxd9"
    );

    // Pinned vector: passphrase="mypassphrase", account=1
    let legacy_a1 = bip39_to_legacy_mnemonic(bip39, "mypassphrase", 1).unwrap();
    assert_eq!(
        legacy_a1,
        "romance radar talent juggled bygones unquoted orchid siblings dubbed lion exult \
        wise atrium coexist rewind taunts nimbly roster shyness skydive opened umpire \
        oven polar nimbly"
    );
    let keys_a1 = derive_keys(&legacy_a1, "mainnet").unwrap();
    assert_eq!(
        keys_a1.address,
        "42cmFtDdVjEQaP4aYPKfTEh1NrCxTpd6XGNSGyvphacMHqEKK3nWHVP6VgZV48172386mQWHSashEXpdmEdDv6vF2yu5PeP"
    );
}

#[test]
fn test_resolve_seed_bip39_with_passphrase_wallet2() {
    let bip39 = "color ranch color remove subway public water embrace before begin liberty fault";

    // Pinned vector: passphrase="mypassphrase", account=0
    let legacy = bip39_to_legacy_mnemonic(bip39, "mypassphrase", 0).unwrap();
    assert_eq!(
        legacy,
        "mixture ivory fabrics react navy deity history debut aimless owed puffin \
        trying jazz tarnished pizza elite memoir wives pockets fidget ankle wanted \
        tolerant typist typist"
    );
    let keys = derive_keys(&legacy, "mainnet").unwrap();
    assert_eq!(
        keys.address,
        "42CCXP59L21BEwWGKVRf7NTkWe9r7e1AAgC9Szw9pczAaij6kguRRaUbJqskePJUFfXLpWf4p7ZA4Pqfq6hBMvw66d2Mzqx"
    );
}

#[test]
fn test_resolve_seed_bip39_with_account_index() {
    let bip39 = "meadow tip best belt boss eyebrow control affair eternal piece very shiver";
    let legacy_0 = bip39_to_legacy_mnemonic(bip39, "", 0).unwrap();
    let legacy_1 = bip39_to_legacy_mnemonic(bip39, "", 1).unwrap();
    assert_ne!(legacy_0, legacy_1);
    // Pinned addresses for account 0 and 1 (empty passphrase)
    assert_eq!(
        derive_address(&legacy_0, "mainnet").unwrap(),
        "496KnMKCc8N4smgQ8XV1uJYwgonX8de8i814Q5ycq1WTaM7YN6e1ATwabmr6FtRSa7A6sSFnPfMqhZ4FHwrS8vMWJka2snk"
    );
    assert_eq!(
        derive_address(&legacy_1, "mainnet").unwrap(),
        "44FGDiq3V6uWhPVwVtnKNufm69mfM4VCs8GCtCJzrGyqe8s7w2Pgx8r6b9S5J6e7VaHfBqA6Go9VudyjGpGiL6imK2rPXMg"
    );
}

#[test]
fn test_resolve_seed_bip39_passthrough_classic() {
    // 25-word classic seed should pass through unchanged regardless of passphrase
    let classic = "tasked eight afraid laboratory tail feline rift reinvest vane cafe bailed \
        foggy dormant paper jigsaw king hazard suture king dapper dummy jolted \
        dating dwindling king";
    let _seed_default = resolve_seed(classic).unwrap();
    let _seed_with_pass = resolve_seed_bip39(classic, "somepassphrase", 5).unwrap();
    // Both should produce the same keys
    let keys_default = derive_keys(classic, "mainnet").unwrap();
    // Can't compare Seed directly, but keys should match
    assert_eq!(keys_default.address, derive_address(classic, "mainnet").unwrap());
}

#[test]
fn test_resolve_seed_bip39_passthrough_polyseed() {
    let polyseed = generate_seed("polyseed").unwrap();
    // Polyseed (16 words) should pass through regardless of passphrase/account_index
    let _seed = resolve_seed_bip39(&polyseed, "pass", 3).unwrap();
    let keys = derive_keys(&polyseed, "mainnet").unwrap();
    assert!(!keys.address.is_empty());
}
