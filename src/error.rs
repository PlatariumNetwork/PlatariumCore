use thiserror::Error;

#[derive(Error, Debug)]
pub enum PlatariumError {
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Cryptographic error: {0}")]
    Crypto(String),
    
    #[error("BIP39 error: {0}")]
    Bip39(String),
    
    #[error("BIP32 error: {0}")]
    Bip32(String),
    
    #[error("Signature error: {0}")]
    Signature(String),
    
    #[error("Key derivation error: {0}")]
    KeyDerivation(String),
    
    #[error("State error: {0}")]
    State(String),
}

pub type Result<T> = std::result::Result<T, PlatariumError>;

impl From<bip39::Error> for PlatariumError {
    fn from(err: bip39::Error) -> Self {
        PlatariumError::Bip39(err.to_string())
    }
}

impl From<secp256k1::Error> for PlatariumError {
    fn from(err: secp256k1::Error) -> Self {
        PlatariumError::Crypto(err.to_string())
    }
}

impl From<bip32::Error> for PlatariumError {
    fn from(err: bip32::Error) -> Self {
        PlatariumError::Bip32(err.to_string())
    }
}

