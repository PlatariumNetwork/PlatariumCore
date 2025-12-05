use bip39::{Language, Mnemonic};
use rand::Rng;
use crate::error::{PlatariumError, Result};

pub const CHARACTER_SET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Generates a BIP39 mnemonic phrase and an alphanumeric part
/// 
/// Returns a tuple of (mnemonic_phrase, alphanumeric_part)
pub fn generate_mnemonic() -> Result<(String, String)> {
    // Generate 24-word mnemonic (256 bits of entropy = 32 bytes)
    let mut entropy = [0u8; 32];
    rand::thread_rng().fill(&mut entropy);
    
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)?;
    let mnemonic_phrase = mnemonic.to_string();
    
    // Generate 12-character alphanumeric part
    let alphanumeric_part = generate_alphanumeric_string(12)?;
    
    Ok((mnemonic_phrase, alphanumeric_part))
}

/// Generates a random alphanumeric string of given length
fn generate_alphanumeric_string(length: usize) -> Result<String> {
    if length == 0 {
        return Err(PlatariumError::Validation("Length must be greater than 0".to_string()));
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

/// Validates a BIP39 mnemonic phrase
pub fn validate_mnemonic(mnemonic: &str) -> bool {
    Mnemonic::parse_in_normalized(Language::English, mnemonic).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mnemonic() {
        let (mnemonic, alphanumeric) = generate_mnemonic().unwrap();
        assert!(!mnemonic.is_empty());
        assert_eq!(alphanumeric.len(), 12);
        assert!(validate_mnemonic(&mnemonic));
    }

    #[test]
    fn test_validate_mnemonic() {
        let (mnemonic, _) = generate_mnemonic().unwrap();
        assert!(validate_mnemonic(&mnemonic));
        assert!(!validate_mnemonic("invalid mnemonic phrase here"));
    }
}

