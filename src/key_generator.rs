use bip39::{Language, Mnemonic};
use bip32::{DerivationPath, XPrv};
use secp256k1::{Secp256k1, SecretKey, PublicKey};
use rand::Rng;
use crate::error::{PlatariumError, Result};
use crate::mnemonic::{generate_mnemonic, validate_mnemonic, CHARACTER_SET};
use crate::utils::{bn_to_hex32, verify_correlation, HKDF_SALT, HKDF_INFO};

/// Generates a random alphanumeric string of given length
pub fn generate_alphanumeric_part(length: usize) -> Result<String> {
    if length == 0 {
        return Err(PlatariumError::Validation("length must be a positive integer".to_string()));
    }

    let mut rng = rand::thread_rng();
    let chars: Vec<char> = CHARACTER_SET.chars().collect();
    
    let result: String = (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..chars.len());
            chars[idx]
        })
        .collect();
    
    Ok(result)
}

/// Re-export derive_signature_seed_from_master_seed from utils
pub use crate::utils::derive_signature_seed_from_master_seed;


/// Key generation result
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub mnemonic: String,
    pub alphanumeric_part: String,
    pub derivation_paths: DerivationPaths,
    pub public_key: String,
    pub private_key: String,
    pub signature_key: String,
}

#[derive(Debug, Clone)]
pub struct DerivationPaths {
    pub main_path: String,
    pub signature_path: String,
}

/// Key generator using BIP32 + HKDF
pub struct KeyGenerator {
    seed_index: u32,
    hkdf_salt: Vec<u8>,
    hkdf_info: Vec<u8>,
    custom_path: Option<String>,
}

impl KeyGenerator {
    /// Creates a new KeyGenerator
    /// 
    /// # Arguments
    /// * `seed_index` - BIP44 index (0 <= seed_index < 2^31 - 1)
    /// * `hkdf_salt` - Optional HKDF salt (defaults to HKDF_SALT)
    /// * `hkdf_info` - Optional HKDF info (defaults to HKDF_INFO)
    /// * `custom_path` - Optional custom derivation path
    pub fn new(
        seed_index: u32,
        hkdf_salt: Option<&[u8]>,
        hkdf_info: Option<&[u8]>,
        custom_path: Option<String>,
    ) -> Result<Self> {
        const MAX_INDEX: u32 = 2u32.pow(31) - 1;
        
        if seed_index >= MAX_INDEX {
            return Err(PlatariumError::Validation(
                format!("seedIndex must be in range [0, {}]", MAX_INDEX - 1),
            ));
        }

        Ok(KeyGenerator {
            seed_index,
            hkdf_salt: hkdf_salt.unwrap_or(HKDF_SALT).to_vec(),
            hkdf_info: hkdf_info.unwrap_or(HKDF_INFO).to_vec(),
            custom_path,
        })
    }

    /// Generates new keys from a random mnemonic
    pub fn generate_keys(&self) -> Result<KeyPair> {
        let (mnemonic, alphanumeric_part) = generate_mnemonic()?;
        
        if !validate_mnemonic(&mnemonic) {
            return Err(PlatariumError::Validation(
                "Generated mnemonic is not valid according to BIP39".to_string(),
            ));
        }

        self.restore_keys(&mnemonic, &alphanumeric_part, self.seed_index, self.custom_path.clone())
    }

    /// Restores keys from mnemonic and alphanumeric part
    pub fn restore_keys(
        &self,
        mnemonic: &str,
        alphanumeric_part: &str,
        seed_index: u32,
        custom_path: Option<String>,
    ) -> Result<KeyPair> {
        if !validate_mnemonic(mnemonic) {
            return Err(PlatariumError::Validation(
                "Provided mnemonic is not valid according to BIP39".to_string(),
            ));
        }

        self.build_keys_from_seed(mnemonic, alphanumeric_part, seed_index, custom_path)
    }

    fn build_keys_from_seed(
        &self,
        mnemonic: &str,
        alphanumeric_part: &str,
        seed_index: u32,
        custom_path: Option<String>,
    ) -> Result<KeyPair> {
        let secp = Secp256k1::new();
        
        // Generate master seed from mnemonic
        let mnemonic_obj = Mnemonic::parse_in_normalized(Language::English, mnemonic)?;
        let master_seed = mnemonic_obj.to_seed(alphanumeric_part);

        // Derive main key using BIP32
        let root_xprv = XPrv::new(&master_seed)?;
        
        let main_path = custom_path.clone().unwrap_or_else(|| {
            format!("m/44'/60'/0'/0/{}", seed_index)
        });
        
        let derivation_path: DerivationPath = main_path.parse()
            .map_err(|e| PlatariumError::Bip32(format!("Invalid derivation path: {}", e)))?;
        
        // Derive path by iterating through components
        let main_node = derivation_path.iter().fold(Ok(root_xprv), |acc, child_num| {
            acc?.derive_child(child_num)
        })?;
        let main_private_key = SecretKey::from_slice(&main_node.private_key().to_bytes())
            .map_err(|e| PlatariumError::Crypto(format!("Invalid private key: {}", e)))?;

        // Derive signature key using HKDF
        let signature_seed = crate::utils::derive_signature_seed_from_master_seed(
            &master_seed,
            Some(&self.hkdf_salt),
            Some(&self.hkdf_info),
        )?;
        
        let signature_private_key = SecretKey::from_slice(&signature_seed)
            .map_err(|e| PlatariumError::Crypto(format!("Invalid signature key: {}", e)))?;

        // Get public keys
        let main_public_key = PublicKey::from_secret_key(&secp, &main_private_key);
        let main_public_key_hex = hex::encode(main_public_key.serialize());

        // Format keys
        let private_key_hex = bn_to_hex32(&main_private_key.secret_bytes())?;
        let signature_key_hex = bn_to_hex32(&signature_private_key.secret_bytes())?;

        // Verify correlation
        let is_valid = verify_correlation(
            &private_key_hex,
            &signature_key_hex,
            &master_seed,
            Some(&self.hkdf_salt),
            Some(&self.hkdf_info),
        )?;

        if !is_valid {
            return Err(PlatariumError::Validation(
                "Key correlation verification failed".to_string(),
            ));
        }

        Ok(KeyPair {
            mnemonic: mnemonic.to_string(),
            alphanumeric_part: alphanumeric_part.to_string(),
            derivation_paths: DerivationPaths {
                main_path: main_path.clone(),
                signature_path: "HKDF-derived".to_string(),
            },
            public_key: format!("Px{}", main_public_key_hex),
            private_key: format!("PSx{}", private_key_hex),
            signature_key: format!("Sx{}", signature_key_hex),
        })
    }
}

impl Default for KeyGenerator {
    fn default() -> Self {
        KeyGenerator::new(0, None, None, None).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keys() {
        let key_gen = KeyGenerator::default();
        let keys = key_gen.generate_keys().unwrap();
        
        assert!(!keys.mnemonic.is_empty());
        assert_eq!(keys.alphanumeric_part.len(), 12);
        assert!(keys.public_key.starts_with("Px"));
        assert!(keys.private_key.starts_with("PSx"));
        assert!(keys.signature_key.starts_with("Sx"));
    }

    #[test]
    fn test_restore_keys() {
        let key_gen = KeyGenerator::default();
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
    }
}

