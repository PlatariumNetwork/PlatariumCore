use secp256k1::{Secp256k1, SecretKey, PublicKey, Message};
use secp256k1::ecdsa::Signature;
use sha2::{Sha256, Digest};
use serde_json;
use crate::error::{PlatariumError, Result};

const DOMAIN_SEPARATOR: &str = "PlatariumSignature:";

/// Normalizes CLI compact signatures (128 hex + optional recovery suffix) to 64-byte compact hex.
pub fn normalize_signature_hex(signature_hex: &str) -> String {
    let hex: String = signature_hex
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.len() >= 128 {
        hex[..128].to_string()
    } else {
        hex
    }
}

/// Hashes a message with domain separator
pub fn hash_message<T: serde::Serialize>(message: &T) -> Result<[u8; 32]> {
    let json = serde_json::to_string(message)
        .map_err(|e| PlatariumError::Validation(format!("Failed to serialize message: {}", e)))?;
    
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_SEPARATOR.as_bytes());
    hasher.update(json.as_bytes());
    let hash = hasher.finalize();
    
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    Ok(result)
}

/// Signs a message and returns signature components
pub fn sign_message(private_key: &SecretKey, message: &impl serde::Serialize) -> Result<SignatureComponents> {
    let secp = Secp256k1::new();
    
    // Hash the message
    let hash = hash_message(message)?;
    let msg = Message::from_digest_slice(&hash)
        .map_err(|e| PlatariumError::Signature(format!("Invalid message hash: {}", e)))?;
    
    // Sign (libsecp256k1 already produces low-S; ensure_low_s keeps the contract explicit)
    let signature = ensure_low_s(&secp.sign_ecdsa(&msg, private_key))?;
    
    // Get public key
    let public_key = PublicKey::from_secret_key(&secp, private_key);
    
    // Extract r and s values from canonical low-S signature
    let sig_bytes = signature.serialize_compact();
    let r_hex = hex::encode(&sig_bytes[..32]);
    let s_hex = hex::encode(&sig_bytes[32..]);
    
    Ok(SignatureComponents {
        r: format!("{:0>64}", r_hex),
        s: format!("{:0>64}", s_hex),
        pub_key: hex::encode(public_key.serialize()),
        der: signature.serialize_der().to_vec(),
        signature_compact: format!("{}{}", hex::encode(sig_bytes), "01"),
    })
}

/// Verifies a signature. Rejects non-canonical high-S signatures (L1 malleability hygiene).
pub fn verify_signature(
    message: &impl serde::Serialize,
    signature_hex: &str,
    pub_key_hex: &str,
) -> Result<bool> {
    let secp = Secp256k1::new();
    
    // Hash the message
    let hash = hash_message(message)?;
    let msg = Message::from_digest_slice(&hash)
        .map_err(|e| PlatariumError::Signature(format!("Invalid message hash: {}", e)))?;
    
    // Parse signature (compact may include trailing recovery byte from CLI)
    let sig_bytes = hex::decode(normalize_signature_hex(signature_hex))
        .map_err(|e| PlatariumError::Signature(format!("Invalid signature hex: {}", e)))?;
    
    let signature = if sig_bytes.len() == 64 {
        // Compact format
        let compact: [u8; 64] = sig_bytes.try_into()
            .map_err(|_| PlatariumError::Signature("Invalid signature length".to_string()))?;
        Signature::from_compact(&compact)
            .map_err(|e| PlatariumError::Signature(format!("Invalid compact signature: {}", e)))?
    } else {
        // DER format
        Signature::from_der(&sig_bytes)
            .map_err(|e| PlatariumError::Signature(format!("Invalid DER signature: {}", e)))?
    };

    if !is_low_s(&signature) {
        return Ok(false);
    }
    
    // Parse public key
    let pub_key_bytes = hex::decode(pub_key_hex)
        .map_err(|e| PlatariumError::Signature(format!("Invalid public key hex: {}", e)))?;
    
    let pub_key = PublicKey::from_slice(&pub_key_bytes)
        .map_err(|e| PlatariumError::Signature(format!("Invalid public key: {}", e)))?;
    
    // Verify
    Ok(secp.verify_ecdsa(&msg, &signature, &pub_key).is_ok())
}

/// Returns true if `signature` is already in canonical low-S form.
fn is_low_s(signature: &Signature) -> bool {
    let mut normalized = *signature;
    normalized.normalize_s();
    normalized.serialize_compact() == signature.serialize_compact()
}

/// Ensures signature is in low-S (canonical) form.
fn ensure_low_s(signature: &Signature) -> Result<Signature> {
    let mut sig = *signature;
    sig.normalize_s();
    Ok(sig)
}

#[derive(Debug, Clone)]
pub struct SignatureComponents {
    pub r: String,
    pub s: String,
    pub pub_key: String,
    pub der: Vec<u8>,
    pub signature_compact: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::SecretKey;

    #[test]
    fn test_hash_message() {
        let message = serde_json::json!({"test": "data"});
        let hash = hash_message(&message).unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sign_and_verify() {
        let _secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        
        let message = serde_json::json!({"test": "message"});
        let sig_components = sign_message(&secret_key, &message).unwrap();
        
        let verified = verify_signature(&message, &sig_components.signature_compact[..128], &sig_components.pub_key).unwrap();
        assert!(verified);
    }

    #[test]
    fn l1_reject_high_s_signature() {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[2; 32]).unwrap();
        let message = serde_json::json!({"l1": "high-s"});
        let signed = sign_message(&secret_key, &message).unwrap();

        let compact = hex::decode(&signed.signature_compact[..128]).unwrap();
        let compact_arr: [u8; 64] = compact.as_slice().try_into().unwrap();
        let sig = Signature::from_compact(&compact_arr).unwrap();
        assert!(is_low_s(&sig));

        // Construct high-S twin: S' = n - S (same R).
        let mut high = sig;
        let mut bytes = high.serialize_compact();
        const N: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFE, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C,
            0xD0, 0x36, 0x41, 0x41,
        ];
        let s = bytes[32..].to_vec();
        let mut s_high = [0u8; 32];
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let diff = N[i] as i16 - s[i] as i16 - borrow;
            if diff < 0 {
                s_high[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                s_high[i] = diff as u8;
                borrow = 0;
            }
        }
        bytes[32..].copy_from_slice(&s_high);
        high = Signature::from_compact(&bytes).unwrap();
        assert!(!is_low_s(&high), "constructed signature must be high-S");

        let high_hex = hex::encode(high.serialize_compact());
        let pub_key = signed.pub_key;
        // Explicit policy: reject high-S even if some verifiers would accept malleable form.
        assert!(
            !verify_signature(&message, &high_hex, &pub_key).unwrap(),
            "verify_signature must reject high-S"
        );
        assert!(verify_signature(&message, &signed.signature_compact[..128], &pub_key).unwrap());
        let _ = secp;
    }

    #[test]
    fn l1_ensure_low_s_normalizes() {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[3; 32]).unwrap();
        let message = serde_json::json!({"l1": "normalize"});
        let signed = sign_message(&secret_key, &message).unwrap();
        let compact = hex::decode(&signed.signature_compact[..128]).unwrap();
        let mut bytes: [u8; 64] = compact.try_into().unwrap();
        const N: [u8; 32] = [
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFE, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C,
            0xD0, 0x36, 0x41, 0x41,
        ];
        let s = bytes[32..].to_vec();
        let mut s_high = [0u8; 32];
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let diff = N[i] as i16 - s[i] as i16 - borrow;
            if diff < 0 {
                s_high[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                s_high[i] = diff as u8;
                borrow = 0;
            }
        }
        bytes[32..].copy_from_slice(&s_high);
        let high = Signature::from_compact(&bytes).unwrap();
        let low = ensure_low_s(&high).unwrap();
        assert!(is_low_s(&low));
        let _ = secp; // silence if unused in future edits
    }
}

