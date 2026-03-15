use clap::{Parser, Subcommand};
use platarium_core::*;
use std::process;

#[derive(Parser)]
#[command(name = "platarium-cli")]
#[command(about = "Platarium Core CLI - Cryptographic operations and consensus (Gateway calls Core)")]
#[command(version = "1.0.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Consensus: selection percent from load (Gateway uses this instead of duplicating logic). Output: JSON {"percent": 10|15|20|25|30}
    SelectionPercentFromLoad {
        /// Load percentage 0..100 (e.g. LoadScore×100/SCORE_SCALE)
        #[arg(long)]
        load_pct: u64,
    },
    /// Consensus: committee size from candidate count and load (all logic in Core). Output: JSON {"count": N}
    CommitteeCount {
        /// Number of candidates (e.g. 1 + peer count)
        #[arg(long)]
        candidates: usize,
        /// Load percentage 0..100
        #[arg(long)]
        load_pct: u64,
    },
    /// Consensus: select committee by weight (Gateway uses Core for deterministic selection). Input: JSON [{"id":"...","weight":N},...]. Output: JSON ["id1","id2",...]
    SelectCommittee {
        /// JSON array of {id, weight}
        #[arg(long)]
        candidates: String,
        /// Seed hex (64 chars = 32 bytes)
        #[arg(long)]
        seed_hex: String,
        /// Number of nodes to select
        #[arg(long)]
        count: usize,
    },
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

    /// Validate a transaction (basic: amount, fee, signatures). Input: JSON tx (Gateway format). Output: JSON {"valid": true} or {"valid": false, "error": "..."}
    ValidateTx {
        /// Transaction JSON (hash, from, to, asset, amount, fee_uplp, nonce, reads, writes, sig_main, sig_derived)
        #[arg(long)]
        tx: String,
    },

    /// Sign a transaction with both keys; outputs full signed tx JSON (Gateway adds to mempool).
    SignTransaction {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// Asset: "PLP" or "Token:XXX"
        #[arg(long, default_value = "PLP")]
        asset: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        fee_uplp: u64,
        #[arg(long)]
        nonce: u64,
        /// JSON array of read addresses, e.g. []
        #[arg(long, default_value = "[]")]
        reads: String,
        /// JSON array of write addresses, e.g. []
        #[arg(long, default_value = "[]")]
        writes: String,
        #[arg(short, long)]
        mnemonic: String,
        #[arg(short, long)]
        alphanumeric: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::SelectionPercentFromLoad { load_pct } => handle_selection_percent_from_load(load_pct),
        Commands::CommitteeCount { candidates, load_pct } => handle_committee_count(candidates, load_pct),
        Commands::SelectCommittee {
            candidates,
            seed_hex,
            count,
        } => handle_select_committee(candidates, seed_hex, count),
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
        Commands::ValidateTx { tx } => handle_validate_tx(tx),
        Commands::SignTransaction {
            from,
            to,
            asset,
            amount,
            fee_uplp,
            nonce,
            reads,
            writes,
            mnemonic,
            alphanumeric,
        } => handle_sign_transaction(from, to, asset, amount, fee_uplp, nonce, reads, writes, mnemonic, alphanumeric),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn handle_selection_percent_from_load(load_pct: u64) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let percent = selection_percent_from_load_pct(load_pct).map_err(|e| e.to_string())?;
    let out = serde_json::json!({ "percent": percent });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn handle_committee_count(candidates: usize, load_pct: u64) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let count = committee_count(candidates, load_pct);
    let out = serde_json::json!({ "count": count });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn handle_select_committee(
    candidates: String,
    seed_hex: String,
    count: usize,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct Candidate {
        id: String,
        weight: u64,
    }
    let list: Vec<Candidate> = serde_json::from_str(&candidates).map_err(|e| format!("invalid candidates JSON: {}", e))?;
    let pairs: Vec<(String, u64)> = list.into_iter().map(|c| (c.id, c.weight)).collect();
    let seed_bytes: Vec<u8> = hex::decode(seed_hex.trim()).map_err(|e| format!("invalid seed_hex: {}", e))?;
    let mut seed = [0u8; 32];
    if seed_bytes.len() != 32 {
        return Err("seed_hex must be 64 hex chars (32 bytes)".into());
    }
    seed.copy_from_slice(&seed_bytes[..32]);
    let selected = select_n_by_weight(pairs, &seed, count);
    println!("{}", serde_json::to_string(&selected)?);
    Ok(())
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
        println!("Signature is valid.");
    } else {
        println!("Verified: false");
        println!("Signature is invalid.");
        process::exit(1);
    }
    
    Ok(())
}

fn handle_validate_tx(tx_json: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = match Transaction::from_gateway_json(&tx_json).and_then(|tx| tx.validate_basic().map_err(Into::into)) {
        Ok(()) => serde_json::json!({ "valid": true }),
        Err(e) => serde_json::json!({ "valid": false, "error": e.to_string() }),
    };
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn handle_sign_transaction(
    from: String,
    to: String,
    asset: String,
    amount: u64,
    fee_uplp: u64,
    nonce: u64,
    reads: String,
    writes: String,
    mnemonic: String,
    alphanumeric: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashSet;

    if !validate_mnemonic(&mnemonic) {
        return Err("Invalid mnemonic phrase".into());
    }
    let reads_vec: Vec<String> = serde_json::from_str(&reads).map_err(|e| format!("invalid reads JSON: {}", e))?;
    let writes_vec: Vec<String> = serde_json::from_str(&writes).map_err(|e| format!("invalid writes JSON: {}", e))?;
    let reads_set: HashSet<String> = reads_vec.into_iter().collect();
    let writes_set: HashSet<String> = writes_vec.into_iter().collect();
    let asset_enum = if asset == "PLP" {
        Asset::PLP
    } else if asset.starts_with("Token:") {
        Asset::Token(asset["Token:".len()..].to_string())
    } else {
        Asset::Token(asset.clone())
    };
    let canonical_asset = asset_enum.as_canonical();
    let mut reads_sorted: Vec<String> = reads_set.iter().cloned().collect();
    reads_sorted.sort();
    let mut writes_sorted: Vec<String> = writes_set.iter().cloned().collect();
    writes_sorted.sort();
    #[derive(serde::Serialize)]
    struct TxHashData {
        from: String,
        to: String,
        asset: String,
        amount: u128,
        fee_uplp: u128,
        nonce: u64,
        reads: Vec<String>,
        writes: Vec<String>,
    }
    let amount_u128 = amount as u128;
    let fee_uplp_u128 = fee_uplp as u128;
    let message = TxHashData {
        from: from.clone(),
        to: to.clone(),
        asset: canonical_asset,
        amount: amount_u128,
        fee_uplp: fee_uplp_u128,
        nonce,
        reads: reads_sorted,
        writes: writes_sorted,
    };
    let sig_result = sign_with_both_keys(&message, &mnemonic, &alphanumeric)?;
    let sig_main = sig_result.signatures[0].signature_compact.clone();
    let sig_derived = sig_result.signatures[1].signature_compact.clone();
    // Output Gateway-compatible JSON (asset as string "PLP" or "Token:X")
    let reads_out: Vec<String> = reads_set.iter().cloned().collect();
    let writes_out: Vec<String> = writes_set.iter().cloned().collect();
    let out = serde_json::json!({
        "hash": sig_result.hash,
        "from": from,
        "to": to,
        "asset": asset_enum.as_canonical(),
        "amount": amount_u128,
        "fee_uplp": fee_uplp_u128,
        "nonce": nonce,
        "reads": reads_out,
        "writes": writes_out,
        "sig_main": sig_main,
        "sig_derived": sig_derived,
    });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}