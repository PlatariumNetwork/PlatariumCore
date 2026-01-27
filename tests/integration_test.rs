use platarium_core::*;

#[test]
fn test_mnemonic_generation() {
    let (mnemonic, alphanumeric) = generate_mnemonic().unwrap();
    
    assert!(!mnemonic.is_empty());
    assert_eq!(alphanumeric.len(), 12);
    assert!(validate_mnemonic(&mnemonic));
    
    println!("OK: Mnemonic generation test passed");
    println!("  Mnemonic: {}", mnemonic);
    println!("  Alphanumeric: {}", alphanumeric);
}

#[test]
fn test_mnemonic_validation() {
    let (mnemonic, _) = generate_mnemonic().unwrap();
    assert!(validate_mnemonic(&mnemonic));
    assert!(!validate_mnemonic("invalid mnemonic phrase here"));
    
    println!("OK: Mnemonic validation test passed");
}

#[test]
fn test_generate_alphanumeric_part() {
    let part = generate_alphanumeric_part(10).unwrap();
    assert_eq!(part.len(), 10);
    assert!(part.chars().all(|c| c.is_ascii_alphanumeric()));
    
    let part15 = generate_alphanumeric_part(15).unwrap();
    assert_eq!(part15.len(), 15);
    
    println!("OK: Alphanumeric part generation test passed");
}

#[test]
fn test_key_generator_generate_keys() {
    let key_gen = KeyGenerator::new(0, None, None, None).unwrap();
    let keys = key_gen.generate_keys().unwrap();
    
    assert!(!keys.mnemonic.is_empty());
    assert_eq!(keys.alphanumeric_part.len(), 12);
    assert!(keys.public_key.starts_with("Px"));
    assert!(keys.private_key.starts_with("PSx"));
    assert!(keys.signature_key.starts_with("Sx"));
    assert_eq!(keys.private_key.len(), 67); // PSx + 64 hex chars
    assert_eq!(keys.signature_key.len(), 66); // Sx + 64 hex chars
    
    println!("OK: Key generation test passed");
    println!("  Public Key: {}", keys.public_key);
    println!("  Private Key: {}...", &keys.private_key[..10]);
    println!("  Signature Key: {}...", &keys.signature_key[..10]);
}

#[test]
fn test_key_generator_restore_keys() {
    let key_gen = KeyGenerator::new(0, None, None, None).unwrap();
    let generated = key_gen.generate_keys().unwrap();
    
    let restored = key_gen.restore_keys(
        &generated.mnemonic,
        &generated.alphanumeric_part,
        0,
        None,
    ).unwrap();
    
    assert_eq!(restored.public_key, generated.public_key);
    assert_eq!(restored.private_key, generated.private_key);
    assert_eq!(restored.signature_key, generated.signature_key);
    
    println!("OK: Key restoration test passed");
}

#[test]
fn test_key_correlation_verification() {
    let key_gen = KeyGenerator::new(0, None, None, None).unwrap();
    let keys = key_gen.generate_keys().unwrap();
    
    // Extract hex values without prefixes
    let private_key_hex = keys.private_key.strip_prefix("PSx").unwrap();
    let signature_key_hex = keys.signature_key.strip_prefix("Sx").unwrap();
    
    // Generate master seed for verification
    use bip39::{Language, Mnemonic};
    let mnemonic_obj = Mnemonic::parse_in_normalized(Language::English, &keys.mnemonic).unwrap();
    let master_seed = mnemonic_obj.to_seed(&keys.alphanumeric_part);
    
    let is_valid = verify_correlation(
        private_key_hex,
        signature_key_hex,
        &master_seed,
        None,
        None,
    ).unwrap();
    
    assert!(is_valid, "Key correlation verification failed");
    
    println!("OK: Key correlation verification test passed");
}

#[test]
fn test_custom_derivation_path() {
    let custom_path = "m/44'/60'/1'/0/0";
    let key_gen = KeyGenerator::new(0, None, None, Some(custom_path.to_string())).unwrap();
    let keys = key_gen.generate_keys().unwrap();
    
    assert_eq!(keys.derivation_paths.main_path, custom_path);
    
    println!("OK: Custom derivation path test passed");
}

#[test]
fn test_sign_with_both_keys() {
    let (mnemonic, alphanumeric) = generate_mnemonic().unwrap();
    let message = serde_json::json!({
        "test": "message",
        "timestamp": 1234567890
    });
    
    let result = sign_with_both_keys(&message, &mnemonic, &alphanumeric).unwrap();
    
    assert_eq!(result.signatures.len(), 2);
    assert_eq!(result.signatures[0].sig_type, "main");
    assert_eq!(result.signatures[1].sig_type, "hkdf");
    assert!(!result.hash.is_empty());
    
    println!("OK: Sign with both keys test passed");
    println!("  Hash: {}", result.hash);
    println!("  Main signature R: {}...", &result.signatures[0].r[..16]);
    println!("  HKDF signature R: {}...", &result.signatures[1].r[..16]);
}

#[test]
fn test_sign_and_verify_signature() {
    use secp256k1::SecretKey;
    use platarium_core::signature::{sign_message, verify_signature};
    
    // Generate a random private key
    let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
    
    let message = serde_json::json!({
        "from": "Px1234",
        "to": "Px5678",
        "value": "100",
        "timestamp": 1234567890
    });
    
    let sig_components = sign_message(&secret_key, &message).unwrap();
    
    // Verify using compact format
    let verified = verify_signature(
        &message,
        &sig_components.signature_compact[..128], // Remove "01" suffix
        &sig_components.pub_key,
    ).unwrap();
    
    assert!(verified, "Signature verification failed");
    
    println!("OK: Sign and verify test passed");
}

#[test]
fn test_hash_message() {
    use platarium_core::signature::hash_message;
    
    let message = serde_json::json!({
        "test": "data",
        "number": 42
    });
    
    let hash1 = hash_message(&message).unwrap();
    let hash2 = hash_message(&message).unwrap();
    
    // Same message should produce same hash
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 32);
    
    println!("OK: Hash message test passed");
    println!("  Hash: {}", hex::encode(hash1));
}

#[test]
fn test_bn_to_hex32() {
    let value = b"test";
    let hex = bn_to_hex32(value).unwrap();
    
    assert_eq!(hex.len(), 64);
    assert!(hex.starts_with("0"));
    
    println!("OK: BN to hex32 test passed");
    println!("  Hex: {}", hex);
}

#[test]
fn test_derive_signature_seed() {
    let master_seed = b"test master seed for hkdf derivation";
    let signature_seed = derive_signature_seed_from_master_seed(master_seed, None, None).unwrap();
    
    assert_eq!(signature_seed.len(), 32);
    
    // Same input should produce same output
    let signature_seed2 = derive_signature_seed_from_master_seed(master_seed, None, None).unwrap();
    assert_eq!(signature_seed, signature_seed2);
    
    println!("OK: Derive signature seed test passed");
}

#[test]
fn test_full_workflow() {
    // 1. Generate mnemonic
    let (_mnemonic, _alphanumeric) = generate_mnemonic().unwrap();
    println!("Step 1: Generated mnemonic");
    
    // 2. Generate keys
    let key_gen = KeyGenerator::new(0, None, None, None).unwrap();
    let keys = key_gen.generate_keys().unwrap();
    println!("Step 2: Generated keys");
    
    // 3. Sign a transaction
    let transaction = serde_json::json!({
        "from": keys.public_key.clone(),
        "to": "Px1234567890abcdef",
        "value": "100",
        "timestamp": 1234567890,
        "nonce": 1
    });
    
    let signature_result = sign_with_both_keys(&transaction, &keys.mnemonic, &keys.alphanumeric_part).unwrap();
    println!("Step 3: Signed transaction");
    
    // 4. Verify signatures
    use platarium_core::signature::verify_signature;
    
    let main_sig_verified = verify_signature(
        &transaction,
        &signature_result.signatures[0].signature_compact[..128],
        &signature_result.signatures[0].pub_key,
    ).unwrap();
    
    let hkdf_sig_verified = verify_signature(
        &transaction,
        &signature_result.signatures[1].signature_compact[..128],
        &signature_result.signatures[1].pub_key,
    ).unwrap();
    
    assert!(main_sig_verified, "Main signature verification failed");
    assert!(hkdf_sig_verified, "HKDF signature verification failed");
    
    println!("Step 4: Verified both signatures");
    println!("OK: Full workflow test passed!");
}

