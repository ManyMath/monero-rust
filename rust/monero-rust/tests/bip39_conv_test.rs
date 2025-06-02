use monero_rust::{bip39_to_legacy_mnemonic, derive_address};

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
