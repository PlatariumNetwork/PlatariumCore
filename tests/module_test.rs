// Comprehensive module tests
use platarium_core::*;

#[test]
fn test_all_modules_loaded() {
    // Test that all modules can be imported and used
    let _key_gen = KeyGenerator::default();
    let (_, _) = generate_mnemonic().unwrap();
    let _part = generate_alphanumeric_part(10).unwrap();
    
    println!("OK: All modules loaded successfully");
}

#[test]
fn test_error_handling() {
    // Test validation errors
    assert!(generate_alphanumeric_part(0).is_err());
    
    // Test invalid mnemonic
    assert!(!validate_mnemonic("invalid mnemonic phrase"));
    
    // Test invalid seed index
    assert!(KeyGenerator::new(2u32.pow(31), None, None, None).is_err());
    
    println!("OK: Error handling test passed");
}

#[test]
fn test_key_generator_different_indices() {
    let key_gen0 = KeyGenerator::new(0, None, None, None).unwrap();
    let key_gen1 = KeyGenerator::new(1, None, None, None).unwrap();
    
    let keys0 = key_gen0.generate_keys().unwrap();
    let keys1 = key_gen1.generate_keys().unwrap();
    
    // Different indices should produce different keys
    assert_ne!(keys0.public_key, keys1.public_key);
    assert_ne!(keys0.private_key, keys1.private_key);
    
    println!("OK: Different key indices test passed");
}

#[test]
fn test_signature_components() {
    use secp256k1::SecretKey;
    use platarium_core::signature::sign_message;
    
    let secret_key = SecretKey::from_slice(&[2; 32]).unwrap();
    let message = serde_json::json!({"test": "data"});
    
    let sig = sign_message(&secret_key, &message).unwrap();
    
    assert_eq!(sig.r.len(), 64);
    assert_eq!(sig.s.len(), 64);
    assert!(!sig.pub_key.is_empty());
    assert!(!sig.der.is_empty());
    assert!(!sig.signature_compact.is_empty());
    
    println!("OK: Signature components test passed");
}

#[test]
fn test_multiple_signatures_same_message() {
    let (mnemonic, alphanumeric) = generate_mnemonic().unwrap();
    let message = serde_json::json!({"test": "message"});
    
    let sig1 = sign_with_both_keys(&message, &mnemonic, &alphanumeric).unwrap();
    let sig2 = sign_with_both_keys(&message, &mnemonic, &alphanumeric).unwrap();
    
    // Same mnemonic and message should produce same signatures
    assert_eq!(sig1.hash, sig2.hash);
    assert_eq!(sig1.signatures[0].r, sig2.signatures[0].r);
    assert_eq!(sig1.signatures[1].r, sig2.signatures[1].r);
    
    println!("OK: Multiple signatures consistency test passed");
}

#[test]
fn test_different_messages_different_hashes() {
    use platarium_core::signature::hash_message;
    
    let msg1 = serde_json::json!({"value": 1});
    let msg2 = serde_json::json!({"value": 2});
    
    let hash1 = hash_message(&msg1).unwrap();
    let hash2 = hash_message(&msg2).unwrap();
    
    assert_ne!(hash1, hash2);
    
    println!("OK: Different messages produce different hashes test passed");
}

