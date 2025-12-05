use secp256k1::{Secp256k1, SecretKey, PublicKey, Message};
use secp256k1::ecdsa::Signature;
use sha2::{Sha256, Digest};
use serde_json;
use crate::error::{PlatariumError, Result};

const DOMAIN_SEPARATOR: &str = "PlatariumSignature:";

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
    
    // Sign
    let signature = secp.sign_ecdsa(&msg, private_key);
    
    // Get public key
    let public_key = PublicKey::from_secret_key(&secp, private_key);
    
    // Extract r and s values
    let sig_bytes = signature.serialize_compact();
    let r_hex = hex::encode(&sig_bytes[..32]);
    let s_hex = hex::encode(&sig_bytes[32..]);
    
    // Ensure low-S (canonical form)
    let signature_low_s = ensure_low_s(&signature)?;
    
    Ok(SignatureComponents {
        r: format!("{:0>64}", r_hex),
        s: format!("{:0>64}", s_hex),
        pub_key: hex::encode(public_key.serialize()),
        der: signature_low_s.serialize_der().to_vec(),
        signature_compact: format!("{}{}", hex::encode(signature_low_s.serialize_compact()), "01"),
    })
}

/// Verifies a signature
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
    
    // Parse signature
    let sig_bytes = hex::decode(signature_hex)
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
    
    // Parse public key
    let pub_key_bytes = hex::decode(pub_key_hex)
        .map_err(|e| PlatariumError::Signature(format!("Invalid public key hex: {}", e)))?;
    
    let pub_key = PublicKey::from_slice(&pub_key_bytes)
        .map_err(|e| PlatariumError::Signature(format!("Invalid public key: {}", e)))?;
    
    // Verify
    Ok(secp.verify_ecdsa(&msg, &signature, &pub_key).is_ok())
}

/// Ensures signature is in low-S (canonical) form
fn ensure_low_s(signature: &Signature) -> Result<Signature> {
    // secp256k1 library's sign_ecdsa already ensures low-S
    // Return as-is
    Ok(*signature)
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
}

