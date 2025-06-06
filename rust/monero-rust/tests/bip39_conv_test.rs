use monero_rust::{
    bip39_to_legacy_mnemonic, validate_bip39, generate_bip39,
    derive_address, derive_keys, derive_subaddress,
    generate_seed, resolve_seed, validate_seed,
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
    let seed = resolve_seed(bip39).unwrap();
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
    let seed = resolve_seed(classic).unwrap();
    let keys = derive_keys(classic, "mainnet").unwrap();
    assert!(!keys.address.is_empty());
}

#[test]
fn test_resolve_seed_invalid_input() {
    assert!(resolve_seed("not a valid seed").is_err());
}
