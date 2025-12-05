use sha2::Sha256;
use hkdf::Hkdf;
use crate::error::{PlatariumError, Result};

/// Default HKDF salt for signature key derivation
pub const HKDF_SALT: &[u8] = b"PlatariumSignatureKeySalt2050";
/// Default HKDF info for signature key derivation
pub const HKDF_INFO: &[u8] = b"Signature Key Derivation";

/// Derives a 32-byte signature seed from master seed using HKDF
pub fn derive_signature_seed_from_master_seed(
    master_seed: &[u8],
    salt: Option<&[u8]>,
    info: Option<&[u8]>,
) -> Result<[u8; 32]> {
    if master_seed.is_empty() {
        return Err(PlatariumError::Validation("masterSeed must be non-empty".to_string()));
    }

    let salt = salt.unwrap_or(HKDF_SALT);
    let info = info.unwrap_or(HKDF_INFO);

    let hk = Hkdf::<Sha256>::new(Some(salt), master_seed);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .map_err(|e| PlatariumError::KeyDerivation(format!("HKDF expansion failed: {}", e)))?;

    Ok(okm)
}

/// Verifies correlation between private key and signature key
/// Both should be derivable from the same master seed
pub fn verify_correlation(
    private_key_hex: &str,
    signature_key_hex: &str,
    master_seed: &[u8],
    hkdf_salt: Option<&[u8]>,
    hkdf_info: Option<&[u8]>,
) -> Result<bool> {
    if private_key_hex.len() != 64 || signature_key_hex.len() != 64 {
        return Err(PlatariumError::Validation(
            "Invalid format of private keys for verification".to_string(),
        ));
    }

    // Derive signature seed from master seed
    let signature_seed = derive_signature_seed_from_master_seed(master_seed, hkdf_salt, hkdf_info)?;
    
    // Convert signature seed to hex
    let derived_sig_hex = hex::encode(signature_seed);
    
    // Compare (case-insensitive)
    let is_match = derived_sig_hex.to_lowercase() == signature_key_hex.to_lowercase();
    
    Ok(is_match)
}

/// Converts a big number to a 64-character hex string with padding
pub fn bn_to_hex32(value: &[u8]) -> Result<String> {
    if value.len() > 32 {
        return Err(PlatariumError::Validation(
            "Value length is greater than 32 bytes".to_string(),
        ));
    }
    
    let hex = hex::encode(value);
    Ok(format!("{:0>64}", hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_signature_seed() {
        let master_seed = b"test master seed";
        let result = derive_signature_seed_from_master_seed(master_seed, None, None).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_bn_to_hex32() {
        let value = b"test";
        let hex = bn_to_hex32(value).unwrap();
        assert_eq!(hex.len(), 64);
        assert!(hex.starts_with("0"));
    }
}

