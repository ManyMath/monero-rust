use monero_rust::WalletState;
use monero_seed::{Language, Seed};
use monero_wallet::address::Network;
use std::path::PathBuf;
use tempfile::TempDir;

const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

#[test]
fn test_import_monero_wallet_cli_key_images() {
    let test_vector_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/stagenet_key_images.monero-wallet-cli");

    if !test_vector_path.exists() {
        panic!("test vector not found: {:?}", test_vector_path);
    }

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    wallet.save("").unwrap();

    let (newly_spent, already_spent) = wallet.import_key_images(&test_vector_path).unwrap();
    let _ = (newly_spent, already_spent); // counts can be 0 if wallet has no matching outputs
}

#[test]
fn test_export_format_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("our_export.bin");
    wallet.export_key_images(&export_path, true).unwrap();

    let exported_data = std::fs::read(&export_path).unwrap();
    assert!(exported_data.starts_with(b"Monero key image export\x03"));
}

#[test]
fn test_round_trip_with_cli_format() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "test_password",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("export.bin");
    let count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(count, 0);

    let file_data = std::fs::read(&export_path).unwrap();
    assert!(file_data.len() >= 24);
    assert_eq!(&file_data[0..23], b"Monero key image export");
    assert_eq!(file_data[23], 0x03);
    assert!(file_data.len() >= 100); // magic(24) + IV(8) + encrypted(offset(4) + keys(64))

    let (newly_spent, already_spent) = wallet.import_key_images(&export_path).unwrap();
    assert_eq!(newly_spent, 0);
    assert_eq!(already_spent, 0);
}

#[test]
fn test_export_structure_matches_monero_format() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("export.bin");
    wallet.export_key_images(&export_path, true).unwrap();

    let file_data = std::fs::read(&export_path).unwrap();
    assert_eq!(&file_data[0..24], b"Monero key image export\x03");

    let iv = &file_data[24..32];
    let encrypted = &file_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(encrypted, &chacha_key, &iv_array);

    // decrypted: offset(4) + spend_public(32) + view_public(32) + key_images...
    assert!(decrypted.len() >= 68);

    let offset = u32::from_le_bytes([decrypted[0], decrypted[1], decrypted[2], decrypted[3]]);
    assert_eq!(offset, 0);

    let spend_public = &decrypted[4..36];
    let view_public = &decrypted[36..68];

    assert_eq!(spend_public, &wallet.view_pair.spend().compress().to_bytes()[..]);
    assert_eq!(view_public, &wallet.view_pair.view().compress().to_bytes()[..]);
}

#[test]
fn test_signature_generation_and_verification() {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use curve25519_dalek::scalar::Scalar;
    use monero_generators::biased_hash_to_point;
    use sha3::{Digest, Keccak256};
    use std::ops::Deref;
    use zeroize::Zeroizing;

    const OUTPUT_PUBKEY: &str = "cc5eaba178da7120abf3baef8ba8c015a06be428b0a9fcf802fb0e1d3e99d0ab";
    const TX_SECRET_KEY: &str = "25af62811cc2b11052e6c33886b1449cf628f4c15c1d2a8fd8bde2ca0617a400";
    const OUTPUT_INDEX: usize = 0;

    let seed = Seed::from_string(Language::English, zeroize::Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let spend_key_bytes: [u8; 32] = *seed.entropy();
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_key_bytes));
    let keccak_output: [u8; 32] = Keccak256::digest(spend_key_bytes).into();
    let view_key = Zeroizing::new(Scalar::from_bytes_mod_order(keccak_output));

    let tx_secret_bytes = hex::decode(TX_SECRET_KEY).unwrap();
    let mut tx_secret_array = [0u8; 32];
    tx_secret_array.copy_from_slice(&tx_secret_bytes);
    let tx_secret = Scalar::from_bytes_mod_order(tx_secret_array);
    let tx_pubkey = &tx_secret * ED25519_BASEPOINT_TABLE;
    let ecdh = view_key.deref() * tx_pubkey;

    let output_pubkey_bytes = hex::decode(OUTPUT_PUBKEY).unwrap();
    let output_pubkey = CompressedEdwardsY::from_slice(&output_pubkey_bytes).unwrap().decompress().unwrap();

    fn compute_key_offset(ecdh_point: &curve25519_dalek::edwards::EdwardsPoint, output_index: usize) -> Scalar {
        let cofactor_mul = ecdh_point.mul_by_cofactor();
        let mut derivation = cofactor_mul.compress().to_bytes().to_vec();
        derivation.push(output_index as u8);
        Scalar::from_bytes_mod_order(Keccak256::digest(&derivation).into())
    }

    let key_offset = compute_key_offset(&ecdh, OUTPUT_INDEX);
    let effective_spend_key = spend_key.deref() + key_offset;

    let hash_point = biased_hash_to_point(output_pubkey.compress().to_bytes());
    let key_image_point = effective_spend_key * hash_point;

    let signature = monero_rust::crypto::generate_key_image_signature(&effective_spend_key, &output_pubkey, &key_image_point);
    assert_ne!(signature, [0u8; 64]);
    assert!(monero_rust::crypto::verify_key_image_signature(&signature, &output_pubkey, &key_image_point));
}

#[test]
fn test_signature_verification_rejects_invalid() {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::scalar::Scalar;
    use monero_generators::biased_hash_to_point;
    use rand_core::OsRng;

    let secret_key = Scalar::random(&mut OsRng);
    let public_key = &secret_key * ED25519_BASEPOINT_TABLE;
    let hash_point = biased_hash_to_point(public_key.compress().to_bytes());
    let key_image = secret_key * hash_point;

    let valid_sig = monero_rust::crypto::generate_key_image_signature(&secret_key, &public_key, &key_image);
    assert!(monero_rust::crypto::verify_key_image_signature(&valid_sig, &public_key, &key_image));

    let mut corrupted = valid_sig;
    corrupted[0] ^= 0xFF;
    assert!(!monero_rust::crypto::verify_key_image_signature(&corrupted, &public_key, &key_image));

    let wrong_secret = Scalar::random(&mut OsRng);
    let wrong_public = &wrong_secret * ED25519_BASEPOINT_TABLE;
    assert!(!monero_rust::crypto::verify_key_image_signature(&valid_sig, &wrong_public, &key_image));

    assert!(!monero_rust::crypto::verify_key_image_signature(&[0u8; 64], &public_key, &key_image));
}

#[test]
fn test_export_with_valid_signatures() {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use curve25519_dalek::scalar::Scalar;
    use monero_generators::biased_hash_to_point;
    use sha3::{Digest, Keccak256};
    use std::ops::Deref;
    use zeroize::Zeroizing;

    const OUTPUT_PUBKEY: &str = "cc5eaba178da7120abf3baef8ba8c015a06be428b0a9fcf802fb0e1d3e99d0ab";
    const TX_SECRET_KEY: &str = "25af62811cc2b11052e6c33886b1449cf628f4c15c1d2a8fd8bde2ca0617a400";
    const EXPECTED_KEY_IMAGE: &str = "6bfc252ca5f153655fc99b3627d1bd0b62d06947b6a89c77c202d43352098549";
    const OUTPUT_INDEX: usize = 0;

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(Language::English, zeroize::Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let mut wallet = WalletState::new(seed.clone(), "English".to_string(), Network::Testnet, "", wallet_path, 0).unwrap();

    let spend_key_bytes: [u8; 32] = *seed.entropy();
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_key_bytes));
    let keccak_output: [u8; 32] = Keccak256::digest(spend_key_bytes).into();
    let view_key = Zeroizing::new(Scalar::from_bytes_mod_order(keccak_output));

    let tx_secret_bytes = hex::decode(TX_SECRET_KEY).unwrap();
    let mut tx_secret_array = [0u8; 32];
    tx_secret_array.copy_from_slice(&tx_secret_bytes);
    let tx_secret = Scalar::from_bytes_mod_order(tx_secret_array);
    let tx_pubkey = &tx_secret * ED25519_BASEPOINT_TABLE;
    let ecdh = view_key.deref() * tx_pubkey;

    fn compute_key_offset(ecdh_point: &curve25519_dalek::edwards::EdwardsPoint, output_index: usize) -> Scalar {
        let cofactor_mul = ecdh_point.mul_by_cofactor();
        let mut derivation = cofactor_mul.compress().to_bytes().to_vec();
        derivation.push(output_index as u8);
        Scalar::from_bytes_mod_order(Keccak256::digest(&derivation).into())
    }

    let key_offset = compute_key_offset(&ecdh, OUTPUT_INDEX);
    let output_pubkey_bytes = hex::decode(OUTPUT_PUBKEY).unwrap();
    let output_pubkey = CompressedEdwardsY::from_slice(&output_pubkey_bytes).unwrap().decompress().unwrap();
    let effective_spend_key = spend_key.deref() + key_offset;
    let hash_point = biased_hash_to_point(output_pubkey.compress().to_bytes());
    let key_image_point = effective_spend_key * hash_point;
    let key_image_bytes = key_image_point.compress().to_bytes();

    assert_eq!(hex::encode(&key_image_bytes), EXPECTED_KEY_IMAGE);

    use monero_rust::types::SerializableOutput;
    let output = SerializableOutput {
        tx_hash: [0u8; 32],
        output_index: OUTPUT_INDEX as u64,
        amount: 5_000_000_000,
        key_image: key_image_bytes,
        subaddress_indices: (0, 0),
        height: 1000,
        unlocked: true,
        spent: false,
        frozen: false,
        payment_id: None,
        key_offset: Some(key_offset.to_bytes()),
        output_public_key: Some(output_pubkey.compress().to_bytes()),
    };
    wallet.outputs.insert(key_image_bytes, output);

    let export_path = temp_dir.path().join("export_with_sigs.bin");
    let count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(count, 1);

    let file_data = std::fs::read(&export_path).unwrap();
    let iv = &file_data[24..32];
    let encrypted = &file_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes_dec = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes_dec);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(encrypted, &chacha_key, &iv_array);
    let key_images_data = &decrypted[68..];
    let exported_sig: [u8; 64] = key_images_data[32..96].try_into().unwrap();

    assert_ne!(exported_sig, [0u8; 64]);
    assert!(monero_rust::crypto::verify_key_image_signature(&exported_sig, &output_pubkey, &key_image_point));
}

#[test]
fn test_verify_monero_wallet_cli_signatures() {
    use curve25519_dalek::edwards::CompressedEdwardsY;

    const TEST_VECTORS: &[(&str, &str)] = &[
        ("e0e85f0a080c2fa2baaddb4397499da77000372f04e11b82bc6fcfa3ce2f7108", "cec4da739aa000035decc6e2bc0e4ad385016b8fbe6a2ceb5f45b4b9c7694629"),
        ("450025e7b0fe9fd1ee96df0d33294eead3446ad7e1fe1cd9eb84dd92c9cff51d", "ae289cd2191ce674d83e5b6cdd378c733dc627f4a2e81c36d190f4c7010852ed"),
        ("c0335180f8552682c82df62742c26499ecaedcc586deb282da6c774ae3871132", "52c11ce9b6b8da54658992828cc1c2c8fc6e5b4d49c025576f3551de63fad31b"),
        ("592b5e2d1ece24ddeaf4803ed4ade6ba77be337ac9c089d049de855186ac59d5", "2af18893293451750177b18ec1a977a260ffb4d46fa4ded4aa7a4fcb70296a49"),
        ("6bfc252ca5f153655fc99b3627d1bd0b62d06947b6a89c77c202d43352098549", "cc5eaba178da7120abf3baef8ba8c015a06be428b0a9fcf802fb0e1d3e99d0ab"),
        ("52cb6b59208a166be2cde80ea2f4c239515b9b070d3d8c0040e5c342e7d36786", "c2c2a2d1cab531e18d81f2609407bfbacf61c73b9ec83e86b841596dee196a1e"),
        ("150841953413906cb2080a3f193a4b784d85904451e3603cd6b05c3a7896394b", "738cbc0ab0ea21c0b1f5ea5bcf12fc6615f6b917901b87983ea40a7a0b2c3422"),
    ];

    let pubkey_map: std::collections::HashMap<Vec<u8>, Vec<u8>> = TEST_VECTORS
        .iter()
        .map(|(ki, pk)| (hex::decode(ki).unwrap(), hex::decode(pk).unwrap()))
        .collect();

    let test_vector_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/stagenet_key_images.monero-wallet-cli");

    if !test_vector_path.exists() {
        panic!("test vector not found: {:?}", test_vector_path);
    }

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(Language::English, zeroize::Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let wallet = WalletState::new(seed, "English".to_string(), Network::Testnet, "", wallet_path, 0).unwrap();

    let cli_data = std::fs::read(&test_vector_path).unwrap();
    let cli_iv = &cli_data[24..32];
    let cli_encrypted = &cli_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(cli_iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(cli_encrypted, &chacha_key, &iv_array);
    let key_images_data = &decrypted[68..];
    let num_key_images = key_images_data.len() / 96;

    assert_eq!(num_key_images, 7);

    for i in 0..num_key_images {
        let offset = i * 96;
        let ki_bytes = &key_images_data[offset..offset + 32];
        let sig_bytes: [u8; 64] = key_images_data[offset + 32..offset + 96].try_into().unwrap();

        let output_pubkey_bytes = pubkey_map.get(ki_bytes).expect("Key image not found in test vectors");
        let output_pubkey = CompressedEdwardsY::from_slice(output_pubkey_bytes).unwrap().decompress().unwrap();
        let key_image_point = CompressedEdwardsY::from_slice(ki_bytes).unwrap().decompress().unwrap();

        assert_ne!(sig_bytes, [0u8; 64]);
        assert!(monero_rust::crypto::verify_key_image_signature(&sig_bytes, &output_pubkey, &key_image_point),
            "CLI signature {} verification failed", i);
    }
}

#[test]
fn test_verify_subaddress_cli_signatures() {
    use curve25519_dalek::edwards::CompressedEdwardsY;

    const SUBADDRESS_KEY_IMAGE: &str = "3d62a9570c06d94829c2291c6f3f3f9debafcff4cecd58fc05b5ca00365e32b4";
    const SUBADDRESS_OUTPUT_PK: &str = "9c2cd583a3971612b035d33586dad0affc37efabc026e3e95199d90f9f62f97e";

    let test_vector_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/subaddress_key_images.monero-wallet-cli");

    if !test_vector_path.exists() {
        panic!("Subaddress test vector not found: {:?}", test_vector_path);
    }

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(Language::English, zeroize::Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let wallet = WalletState::new(seed, "English".to_string(), Network::Testnet, "", wallet_path, 0).unwrap();

    let cli_data = std::fs::read(&test_vector_path).unwrap();
    let cli_iv = &cli_data[24..32];
    let cli_encrypted = &cli_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(cli_iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(cli_encrypted, &chacha_key, &iv_array);
    let key_images_data = &decrypted[68..];
    let num_key_images = key_images_data.len() / 96;

    let expected_ki = hex::decode(SUBADDRESS_KEY_IMAGE).unwrap();
    let mut found = false;

    for i in 0..num_key_images {
        let offset = i * 96;
        let ki_bytes = &key_images_data[offset..offset + 32];

        if ki_bytes == expected_ki.as_slice() {
            let sig_bytes: [u8; 64] = key_images_data[offset + 32..offset + 96].try_into().unwrap();
            let output_pk_bytes = hex::decode(SUBADDRESS_OUTPUT_PK).unwrap();
            let output_pubkey = CompressedEdwardsY::from_slice(&output_pk_bytes).unwrap().decompress().unwrap();
            let key_image_point = CompressedEdwardsY::from_slice(ki_bytes).unwrap().decompress().unwrap();

            assert_ne!(sig_bytes, [0u8; 64]);
            assert!(monero_rust::crypto::verify_key_image_signature(&sig_bytes, &output_pubkey, &key_image_point));
            found = true;
            break;
        }
    }

    assert!(found, "Subaddress key image not found in export");
}

/// Comprehensive test: compute key_offset from tx data, verify key images, export with signatures.
#[test]
fn test_export_all_outputs_with_real_signatures() {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use curve25519_dalek::scalar::Scalar;
    use monero_generators::biased_hash_to_point;
    use sha3::{Digest, Keccak256};
    use std::ops::Deref;
    use zeroize::Zeroizing;

    // (key_image, output_public_key, tx_pub_key, output_index, amount, subaddress_indices)
    const TEST_VECTORS: &[(&str, &str, &str, usize, u64, (u32, u32))] = &[
        ("e0e85f0a080c2fa2baaddb4397499da77000372f04e11b82bc6fcfa3ce2f7108",
         "cec4da739aa000035decc6e2bc0e4ad385016b8fbe6a2ceb5f45b4b9c7694629",
         "d7898f0f50688aa7fae5467ad55e01af040b1368faefa3e2bf6390eb73f899a1",
         1, 10_000_000_000, (0, 0)),
        ("450025e7b0fe9fd1ee96df0d33294eead3446ad7e1fe1cd9eb84dd92c9cff51d",
         "ae289cd2191ce674d83e5b6cdd378c733dc627f4a2e81c36d190f4c7010852ed",
         "a260e23070a9b26614b5debadd23cd8f982dbc120c702944eb442cd4fe2e12c8",
         0, 752_554_768_712, (0, 0)),
        ("c0335180f8552682c82df62742c26499ecaedcc586deb282da6c774ae3871132",
         "52c11ce9b6b8da54658992828cc1c2c8fc6e5b4d49c025576f3551de63fad31b",
         "534033f0414730a06c9162007554f7a03c78fe264cf580d89544e8d1e0e1357d",
         0, 752_553_333_328, (0, 0)),
        ("592b5e2d1ece24ddeaf4803ed4ade6ba77be337ac9c089d049de855186ac59d5",
         "2af18893293451750177b18ec1a977a260ffb4d46fa4ded4aa7a4fcb70296a49",
         "4f1950a4e8647e6de9c7e27a8e051e42a5a2f909137b60ab236d02ac8736e8cf",
         0, 752_550_462_567, (0, 0)),
        ("6bfc252ca5f153655fc99b3627d1bd0b62d06947b6a89c77c202d43352098549",
         "cc5eaba178da7120abf3baef8ba8c015a06be428b0a9fcf802fb0e1d3e99d0ab",
         "TX_SECRET:25af62811cc2b11052e6c33886b1449cf628f4c15c1d2a8fd8bde2ca0617a400",
         0, 5_000_000_000, (0, 0)),
        ("52cb6b59208a166be2cde80ea2f4c239515b9b070d3d8c0040e5c342e7d36786",
         "c2c2a2d1cab531e18d81f2609407bfbacf61c73b9ec83e86b841596dee196a1e",
         "TX_SECRET:25af62811cc2b11052e6c33886b1449cf628f4c15c1d2a8fd8bde2ca0617a400",
         1, 4_960_370_000, (0, 0)),
        ("150841953413906cb2080a3f193a4b784d85904451e3603cd6b05c3a7896394b",
         "738cbc0ab0ea21c0b1f5ea5bcf12fc6615f6b917901b87983ea40a7a0b2c3422",
         "396bbab66b7c266ff033f3813204ae7475a2c0322c6da1375f9e722bfaff9387",
         0, 5_048_231_279, (0, 0)),
        // Subaddress (0, 1) output
        ("3d62a9570c06d94829c2291c6f3f3f9debafcff4cecd58fc05b5ca00365e32b4",
         "9c2cd583a3971612b035d33586dad0affc37efabc026e3e95199d90f9f62f97e",
         "c1da8b69f8c050151bf27d1af551e0a99279c749fc94150e5fbe161f22ea578b",
         0, 133_700_000_000, (0, 1)),
    ];

    fn compute_key_offset(view_key: &Scalar, tx_pub_key: &curve25519_dalek::edwards::EdwardsPoint, output_index: usize) -> Scalar {
        let ecdh = view_key * tx_pub_key;
        let cofactor_mul = ecdh.mul_by_cofactor();
        let mut derivation = cofactor_mul.compress().to_bytes().to_vec();
        derivation.push(output_index as u8);
        Scalar::from_bytes_mod_order(Keccak256::digest(&derivation).into())
    }

    fn compute_subaddress_derivation(view_key: &Scalar, account: u32, address: u32) -> Scalar {
        let mut data = b"SubAddr\0".to_vec();
        data.extend_from_slice(&view_key.to_bytes());
        data.extend_from_slice(&account.to_le_bytes());
        data.extend_from_slice(&address.to_le_bytes());
        Scalar::from_bytes_mod_order(Keccak256::digest(&data).into())
    }

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(Language::English, zeroize::Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let mut wallet = WalletState::new(seed.clone(), "English".to_string(), Network::Testnet, "", wallet_path, 0).unwrap();

    let spend_key_bytes: [u8; 32] = *seed.entropy();
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_key_bytes));
    let keccak_output: [u8; 32] = Keccak256::digest(spend_key_bytes).into();
    let view_key = Zeroizing::new(Scalar::from_bytes_mod_order(keccak_output));

    let mut verified_key_images = 0;

    for (i, (expected_ki_hex, output_pk_hex, tx_key_str, output_index, amount, subaddr_indices)) in TEST_VECTORS.iter().enumerate() {
        let expected_ki = hex::decode(expected_ki_hex).unwrap();
        let output_pk_bytes = hex::decode(output_pk_hex).unwrap();
        let output_pubkey = CompressedEdwardsY::from_slice(&output_pk_bytes).unwrap().decompress().unwrap();

        let tx_pubkey = if tx_key_str.starts_with("TX_SECRET:") {
            let tx_secret_hex = &tx_key_str[10..];
            let tx_secret_bytes = hex::decode(tx_secret_hex).unwrap();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&tx_secret_bytes);
            &Scalar::from_bytes_mod_order(arr) * ED25519_BASEPOINT_TABLE
        } else {
            let tx_pk_bytes = hex::decode(tx_key_str).unwrap();
            CompressedEdwardsY::from_slice(&tx_pk_bytes).unwrap().decompress().unwrap()
        };

        let mut key_offset = compute_key_offset(view_key.deref(), &tx_pubkey, *output_index);
        if *subaddr_indices != (0, 0) {
            key_offset += compute_subaddress_derivation(view_key.deref(), subaddr_indices.0, subaddr_indices.1);
        }

        let effective_spend_key = spend_key.deref() + key_offset;
        let reconstructed_pk = &effective_spend_key * ED25519_BASEPOINT_TABLE;
        assert_eq!(reconstructed_pk.compress().to_bytes(), output_pubkey.compress().to_bytes(), "Output {} pubkey mismatch", i);

        let hash_point = biased_hash_to_point(output_pubkey.compress().to_bytes());
        let key_image_point = effective_spend_key * hash_point;
        let computed_ki = key_image_point.compress().to_bytes();
        assert_eq!(computed_ki.as_slice(), expected_ki.as_slice(), "Output {} key image mismatch", i);

        verified_key_images += 1;

        use monero_rust::types::SerializableOutput;
        let output = SerializableOutput {
            tx_hash: [i as u8; 32],
            output_index: *output_index as u64,
            amount: *amount,
            key_image: computed_ki,
            subaddress_indices: *subaddr_indices,
            height: 2032114 + i as u64,
            unlocked: true,
            spent: false,
            frozen: false,
            payment_id: None,
            key_offset: Some(key_offset.to_bytes()),
            output_public_key: Some(output_pubkey.compress().to_bytes()),
        };
        wallet.outputs.insert(computed_ki, output);
    }

    assert_eq!(verified_key_images, 8);

    let export_path = temp_dir.path().join("real_signatures_export.bin");
    let count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(count, 8);

    let file_data = std::fs::read(&export_path).unwrap();
    let iv = &file_data[24..32];
    let encrypted = &file_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes_export = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes_export);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(encrypted, &chacha_key, &iv_array);
    let key_images_data = &decrypted[68..];

    let mut verified_signatures = 0;
    for (_, (expected_ki_hex, output_pk_hex, _, _, _, _)) in TEST_VECTORS.iter().enumerate() {
        let expected_ki = hex::decode(expected_ki_hex).unwrap();
        let output_pk_bytes = hex::decode(output_pk_hex).unwrap();

        for j in 0..8 {
            let offset = j * 96;
            let ki_bytes = &key_images_data[offset..offset + 32];

            if ki_bytes == expected_ki.as_slice() {
                let sig_bytes: [u8; 64] = key_images_data[offset + 32..offset + 96].try_into().unwrap();
                assert_ne!(sig_bytes, [0u8; 64]);

                let output_pubkey = CompressedEdwardsY::from_slice(&output_pk_bytes).unwrap().decompress().unwrap();
                let key_image_point = CompressedEdwardsY::from_slice(ki_bytes).unwrap().decompress().unwrap();
                assert!(monero_rust::crypto::verify_key_image_signature(&sig_bytes, &output_pubkey, &key_image_point));

                verified_signatures += 1;
                break;
            }
        }
    }

    assert_eq!(verified_signatures, 8);
}
