use clap::{Parser, Subcommand};
use platarium_core::*;
use std::process;

#[derive(Parser)]
#[command(name = "platarium-cli")]
#[command(about = "Platarium Core CLI - Cryptographic operations for Platarium Network")]
#[command(version = "1.0.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new BIP39 mnemonic phrase and alphanumeric code
    GenerateMnemonic,
    
    /// Generate cryptographic keys from a mnemonic phrase
    GenerateKeys {
        /// BIP39 mnemonic phrase
        #[arg(short, long)]
        mnemonic: String,
        
        /// Alphanumeric code (optional, will be generated if not provided)
        #[arg(short, long)]
        alphanumeric: Option<String>,
        
        /// Seed index for key derivation (default: 0)
        #[arg(short, long, default_value = "0")]
        seed_index: u32,
        
        /// Custom derivation path (optional)
        #[arg(short, long)]
        path: Option<String>,
    },
    
    /// Sign a message with both keys (main + HKDF)
    SignMessage {
        /// JSON message to sign
        #[arg(long)]
        message: String,
        
        /// BIP39 mnemonic phrase
        #[arg(short, long)]
        mnemonic: String,
        
        /// Alphanumeric code
        #[arg(short, long)]
        alphanumeric: String,
    },
    
    /// Verify a message signature
    VerifySignature {
        /// JSON message that was signed
        #[arg(long)]
        message: String,
        
        /// Signature in hex format (compact or DER)
        #[arg(short, long)]
        signature: String,
        
        /// Public key in hex format
        #[arg(short, long)]
        pubkey: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::GenerateMnemonic => handle_generate_mnemonic(),
        Commands::GenerateKeys {
            mnemonic,
            alphanumeric,
            seed_index,
            path,
        } => handle_generate_keys(mnemonic, alphanumeric, seed_index, path),
        Commands::SignMessage {
            message,
            mnemonic,
            alphanumeric,
        } => handle_sign_message(message, mnemonic, alphanumeric),
        Commands::VerifySignature {
            message,
            signature,
            pubkey,
        } => handle_verify_signature(message, signature, pubkey),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn handle_generate_mnemonic() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (mnemonic, alphanumeric) = generate_mnemonic()?;
    
    println!("Mnemonic: {}", mnemonic);
    println!("Alphanumeric: {}", alphanumeric);
    
    Ok(())
}

fn handle_generate_keys(
    mnemonic: String,
    alphanumeric: Option<String>,
    seed_index: u32,
    path: Option<String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Validate mnemonic
    if !validate_mnemonic(&mnemonic) {
        return Err("Invalid mnemonic phrase".into());
    }

    // Use provided alphanumeric or generate new one
    let alphanumeric_part = alphanumeric.unwrap_or_else(|| {
        generate_alphanumeric_part(12).unwrap_or_else(|_| {
            eprintln!("Warning: Failed to generate alphanumeric, using empty string");
            String::new()
        })
    });

    // Create key generator
    let key_gen = KeyGenerator::new(seed_index, None, None, path.clone())?;
    
    // Restore keys from mnemonic
    let keys = key_gen.restore_keys(&mnemonic, &alphanumeric_part, seed_index, path)?;
    
    println!("Public Key: {}", keys.public_key);
    println!("Private Key: {}", keys.private_key);
    println!("Signature Key: {}", keys.signature_key);
    println!("Derivation Path: {}", keys.derivation_paths.main_path);
    println!("Alphanumeric: {}", keys.alphanumeric_part);
    
    Ok(())
}

fn handle_sign_message(
    message_str: String,
    mnemonic: String,
    alphanumeric: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Validate mnemonic
    if !validate_mnemonic(&mnemonic) {
        return Err("Invalid mnemonic phrase".into());
    }

    // Parse JSON message
    let message: serde_json::Value = serde_json::from_str(&message_str)
        .map_err(|e| format!("Invalid JSON message: {}", e))?;

    // Sign the message
    let signature_result = sign_with_both_keys(&message, &mnemonic, &alphanumeric)?;
    
    println!("Message Hash: {}", signature_result.hash);
    println!("\nMain Signature:");
    println!("  Type: {}", signature_result.signatures[0].sig_type);
    println!("  R: {}", signature_result.signatures[0].r);
    println!("  S: {}", signature_result.signatures[0].s);
    println!("  Public Key: {}", signature_result.signatures[0].pub_key);
    println!("  DER: {}", signature_result.signatures[0].der);
    println!("  Compact: {}", signature_result.signatures[0].signature_compact);
    
    println!("\nHKDF Signature:");
    println!("  Type: {}", signature_result.signatures[1].sig_type);
    println!("  R: {}", signature_result.signatures[1].r);
    println!("  S: {}", signature_result.signatures[1].s);
    println!("  Public Key: {}", signature_result.signatures[1].pub_key);
    println!("  DER: {}", signature_result.signatures[1].der);
    println!("  Compact: {}", signature_result.signatures[1].signature_compact);
    
    Ok(())
}

fn handle_verify_signature(
    message_str: String,
    signature: String,
    pubkey: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Parse JSON message
    let message: serde_json::Value = serde_json::from_str(&message_str)
        .map_err(|e| format!("Invalid JSON message: {}", e))?;

    // Verify signature
    let verified = verify_signature(&message, &signature, &pubkey)?;
    
    if verified {
        println!("Verified: true");
        println!("✓ Signature is valid");
    } else {
        println!("Verified: false");
        println!("✗ Signature is invalid");
        process::exit(1);
    }
    
    Ok(())
}

