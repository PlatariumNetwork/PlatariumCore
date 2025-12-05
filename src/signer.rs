use bip39::{Language, Mnemonic};
use secp256k1::SecretKey;
use sha2::Sha256;
use hkdf::Hkdf;
use crate::error::{PlatariumError, Result};
use crate::signature::{sign_message, hash_message};

/// Generates master seed from mnemonic with alphanumeric part
fn generate_master_seed(mnemonic: &str, alphanumeric_part: &str) -> Result<Vec<u8>> {
    let mnemonic_obj = Mnemonic::parse_in_normalized(Language::English, mnemonic)?;
    Ok(mnemonic_obj.to_seed(alphanumeric_part).to_vec())
}

/// Derives HKDF key from seed with info
fn derive_hkdf_key(seed: &[u8], info: &[u8]) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, seed);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .map_err(|e| PlatariumError::KeyDerivation(format!("HKDF expansion failed: {}", e)))?;
    Ok(okm)
}

/// Signs a message with both keys (main key and HKDF-derived key)
pub fn sign_with_both_keys<T: serde::Serialize>(
    message: &T,
    mnemonic: &str,
    alphanumeric_part: &str,
) -> Result<DualSignature> {
    // Generate master seed
    let seed = generate_master_seed(mnemonic, alphanumeric_part)?;
    
    // Hash the message
    let hash = hash_message(message)?;
    let hash_hex = hex::encode(hash);
    
    // Derive keys using HKDF with different info strings
    let main_key_info = format!("mainKey-{}", alphanumeric_part);
    let hkdf_key_info = format!("hkdfKey-{}", alphanumeric_part);
    
    let main_private_key_bytes = derive_hkdf_key(&seed, main_key_info.as_bytes())?;
    let hkdf_private_key_bytes = derive_hkdf_key(&seed, hkdf_key_info.as_bytes())?;
    
    let main_private_key = SecretKey::from_slice(&main_private_key_bytes)
        .map_err(|e| PlatariumError::Crypto(format!("Invalid main private key: {}", e)))?;
    
    let hkdf_private_key = SecretKey::from_slice(&hkdf_private_key_bytes)
        .map_err(|e| PlatariumError::Crypto(format!("Invalid HKDF private key: {}", e)))?;
    
    // Sign with both keys
    let main_signature = sign_message(&main_private_key, message)?;
    let hkdf_signature = sign_message(&hkdf_private_key, message)?;
    
    Ok(DualSignature {
        hash: hash_hex,
        signatures: vec![
            SignatureWithType {
                sig_type: "main".to_string(),
                r: main_signature.r,
                s: main_signature.s,
                pub_key: main_signature.pub_key,
                der: hex::encode(&main_signature.der),
                signature_compact: main_signature.signature_compact,
            },
            SignatureWithType {
                sig_type: "hkdf".to_string(),
                r: hkdf_signature.r,
                s: hkdf_signature.s,
                pub_key: hkdf_signature.pub_key,
                der: hex::encode(&hkdf_signature.der),
                signature_compact: hkdf_signature.signature_compact,
            },
        ],
    })
}

#[derive(Debug, Clone)]
pub struct DualSignature {
    pub hash: String,
    pub signatures: Vec<SignatureWithType>,
}

#[derive(Debug, Clone)]
pub struct SignatureWithType {
    pub sig_type: String,
    pub r: String,
    pub s: String,
    pub pub_key: String,
    pub der: String,
    pub signature_compact: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_with_both_keys() {
        use crate::mnemonic::generate_mnemonic;
        
        let (mnemonic, alphanumeric) = generate_mnemonic().unwrap();
        let message = serde_json::json!({"test": "message"});
        
        let result = sign_with_both_keys(&message, &mnemonic, &alphanumeric).unwrap();
        
        assert_eq!(result.signatures.len(), 2);
        assert_eq!(result.signatures[0].sig_type, "main");
        assert_eq!(result.signatures[1].sig_type, "hkdf");
    }
}

