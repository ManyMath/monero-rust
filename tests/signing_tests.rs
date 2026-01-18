use monero_rust::signing::{
    generate_key_image, sign_clsag, verify_clsag, prove_key_image, verify_key_image,
    SigningParameters,
};
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    scalar::Scalar,
};
use rand_core::OsRng;
use zeroize::Zeroizing;

#[test]
fn test_key_image_deterministic() {
    let secret = Scalar::random(&mut OsRng);
    let public = &secret * ED25519_BASEPOINT_TABLE;

    let ki1 = generate_key_image(&secret, &public);
    let ki2 = generate_key_image(&secret, &public);
    let ki3 = generate_key_image(&secret, &public);

    assert_eq!(ki1, ki2);
    assert_eq!(ki2, ki3);
}

#[test]
fn test_key_image_unique_per_key() {
    let secret1 = Scalar::random(&mut OsRng);
    let secret2 = Scalar::random(&mut OsRng);

    let public1 = &secret1 * ED25519_BASEPOINT_TABLE;
    let public2 = &secret2 * ED25519_BASEPOINT_TABLE;

    let ki1 = generate_key_image(&secret1, &public1);
    let ki2 = generate_key_image(&secret2, &public2);

    assert_ne!(ki1, ki2);
}

#[test]
fn test_key_image_not_identity() {
    for _ in 0..10 {
        let secret = Scalar::random(&mut OsRng);
        let public = &secret * ED25519_BASEPOINT_TABLE;
        let _ki = generate_key_image(&secret, &public);
    }
}

#[test]
fn test_key_image_proof_valid() {
    let secret = Scalar::random(&mut OsRng);
    let public = &secret * ED25519_BASEPOINT_TABLE;
    let key_image = generate_key_image(&secret, &public);

    let proof = prove_key_image(&secret, &public, &key_image);
    assert!(verify_key_image(&key_image, &public, &proof));
}

#[test]
fn test_key_image_proof_wrong_public_key() {
    let secret = Scalar::random(&mut OsRng);
    let public = &secret * ED25519_BASEPOINT_TABLE;
    let key_image = generate_key_image(&secret, &public);

    let proof = prove_key_image(&secret, &public, &key_image);

    let wrong_secret = Scalar::random(&mut OsRng);
    let wrong_public = &wrong_secret * ED25519_BASEPOINT_TABLE;

    assert!(!verify_key_image(&key_image, &wrong_public, &proof));
}

#[test]
fn test_key_image_proof_wrong_key_image() {
    let secret = Scalar::random(&mut OsRng);
    let public = &secret * ED25519_BASEPOINT_TABLE;
    let key_image = generate_key_image(&secret, &public);

    let proof = prove_key_image(&secret, &public, &key_image);

    let wrong_secret = Scalar::random(&mut OsRng);
    let wrong_public = &wrong_secret * ED25519_BASEPOINT_TABLE;
    let wrong_ki = generate_key_image(&wrong_secret, &wrong_public);

    assert!(!verify_key_image(&wrong_ki, &public, &proof));
}

#[test]
fn test_clsag_sign_and_verify_ring_size_4() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 2;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"test transaction message";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    assert!(verify_clsag(
        &signed.signature,
        &ring_members,
        message,
        &commitment,
        None,
    ));
}

#[test]
fn test_clsag_sign_and_verify_ring_size_16() {
    let mut rng = OsRng;
    let ring_size = 16;
    let real_idx = 7;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"monero transaction";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    assert!(verify_clsag(
        &signed.signature,
        &ring_members,
        message,
        &commitment,
        None,
    ));
}

#[test]
fn test_clsag_verify_wrong_message() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 1;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"original message";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    let wrong_message = b"tampered message";
    assert!(!verify_clsag(
        &signed.signature,
        &ring_members,
        wrong_message,
        &commitment,
        None,
    ));
}

#[test]
fn test_clsag_verify_wrong_ring() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 0;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"test message";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    let mut wrong_ring = Vec::with_capacity(ring_size);
    for _ in 0..ring_size {
        wrong_ring.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
    }

    assert!(!verify_clsag(
        &signed.signature,
        &wrong_ring,
        message,
        &commitment,
        None,
    ));
}

#[test]
fn test_clsag_verify_wrong_commitment() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 3;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"test message";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    let wrong_commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    assert!(!verify_clsag(
        &signed.signature,
        &ring_members,
        message,
        &wrong_commitment,
        None,
    ));
}

#[test]
fn test_clsag_with_pseudo_commitment() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 1;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let pseudo_commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"test with pseudo commitment";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: Some(pseudo_commitment),
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");

    assert!(verify_clsag(
        &signed.signature,
        &ring_members,
        message,
        &commitment,
        Some(&pseudo_commitment),
    ));

    assert!(!verify_clsag(
        &signed.signature,
        &ring_members,
        message,
        &commitment,
        None,
    ));
}

#[test]
fn test_clsag_different_real_indices() {
    let mut rng = OsRng;
    let ring_size = 8;

    for real_idx in 0..ring_size {
        let spend_key = Zeroizing::new(Scalar::random(&mut rng));
        let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

        let mut ring_members = Vec::with_capacity(ring_size);
        for i in 0..ring_size {
            if i == real_idx {
                ring_members.push(real_pubkey);
            } else {
                ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
            }
        }

        let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
        let message = format!("test real_idx={}", real_idx);

        let params = SigningParameters {
            spend_key,
            real_output_index: real_idx,
            ring_members: ring_members.clone(),
            commitment,
            pseudo_commitment: None,
            message: message.as_bytes().to_vec(),
        };

        let signed = sign_clsag(&mut rng, params)
            .unwrap_or_else(|_| panic!("signing failed for real_idx={}", real_idx));

        assert!(
            verify_clsag(
                &signed.signature,
                &ring_members,
                message.as_bytes(),
                &commitment,
                None,
            ),
            "verification failed for real_idx={}",
            real_idx
        );
    }
}

#[test]
fn test_clsag_error_ring_size_too_small() {
    let mut rng = OsRng;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let ring_members = vec![real_pubkey];
    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;

    let params = SigningParameters {
        spend_key: spend_key.clone(),
        real_output_index: 0,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: b"test".to_vec(),
    };

    assert!(sign_clsag(&mut rng, params).is_err());

    let params = SigningParameters {
        spend_key,
        real_output_index: 0,
        ring_members: vec![],
        commitment,
        pseudo_commitment: None,
        message: b"test".to_vec(),
    };

    assert!(sign_clsag(&mut rng, params).is_err());
}

#[test]
fn test_clsag_error_real_index_out_of_bounds() {
    let mut rng = OsRng;
    let ring_size = 4;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));

    let mut ring_members = Vec::with_capacity(ring_size);
    for _ in 0..ring_size {
        ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;

    let params = SigningParameters {
        spend_key: spend_key.clone(),
        real_output_index: ring_size,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: b"test".to_vec(),
    };

    assert!(sign_clsag(&mut rng, params).is_err());

    let params = SigningParameters {
        spend_key,
        real_output_index: ring_size + 10,
        ring_members,
        commitment,
        pseudo_commitment: None,
        message: b"test".to_vec(),
    };

    assert!(sign_clsag(&mut rng, params).is_err());
}

#[test]
fn test_clsag_multiple_signatures_same_key() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 2;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"test message";

    let mut signatures = Vec::new();
    for _ in 0..3 {
        let params = SigningParameters {
            spend_key: spend_key.clone(),
            real_output_index: real_idx,
            ring_members: ring_members.clone(),
            commitment,
            pseudo_commitment: None,
            message: message.to_vec(),
        };

        let signed = sign_clsag(&mut rng, params).expect("signing failed");
        signatures.push(signed);
    }

    for sig in &signatures {
        assert!(verify_clsag(
            &sig.signature,
            &ring_members,
            message,
            &commitment,
            None,
        ));
    }

    assert_eq!(signatures[0].key_image, signatures[1].key_image);
    assert_eq!(signatures[1].key_image, signatures[2].key_image);
}

#[test]
fn test_clsag_signature_clone() {
    let mut rng = OsRng;
    let ring_size = 4;
    let real_idx = 0;

    let spend_key = Zeroizing::new(Scalar::random(&mut rng));
    let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

    let mut ring_members = Vec::with_capacity(ring_size);
    for i in 0..ring_size {
        if i == real_idx {
            ring_members.push(real_pubkey);
        } else {
            ring_members.push(&Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE);
        }
    }

    let commitment = &Scalar::random(&mut rng) * ED25519_BASEPOINT_TABLE;
    let message = b"clone test";

    let params = SigningParameters {
        spend_key,
        real_output_index: real_idx,
        ring_members: ring_members.clone(),
        commitment,
        pseudo_commitment: None,
        message: message.to_vec(),
    };

    let signed = sign_clsag(&mut rng, params).expect("signing failed");
    let cloned_sig = signed.signature.clone();

    assert!(verify_clsag(
        &cloned_sig,
        &ring_members,
        message,
        &commitment,
        None,
    ));
}
