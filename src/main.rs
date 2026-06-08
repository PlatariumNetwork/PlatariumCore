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

    /// Validate a transaction (basic or full state check with --state-file). Output: JSON {"valid": true} or {"valid": false, "error": "..."}
    ValidateTx {
        /// Transaction JSON (hash, from, to, asset, amount, fee_uplp, nonce, reads, writes, sig_main, sig_derived)
        #[arg(long)]
        tx: String,
        /// Optional state file for nonce/balance applicability check
        #[arg(long)]
        state_file: Option<String>,
    },

    /// Initialize empty persistent Core state file
    StateInit {
        #[arg(long)]
        state_file: String,
    },

    /// Query balance, μPLP, and nonce from state file
    StateQuery {
        #[arg(long)]
        state_file: String,
        #[arg(long)]
        address: String,
        #[arg(long, default_value = "PLP")]
        asset: String,
    },

    /// Dry-run validate transaction against state file (no apply)
    StateValidateTx {
        #[arg(long)]
        state_file: String,
        #[arg(long)]
        tx: String,
    },

    /// Apply transaction to state file (production commit)
    StateApplyTx {
        #[arg(long)]
        state_file: String,
        #[arg(long)]
        tx: String,
    },

    /// Credit PLP and μPLP to address (testnet only)
    StateCredit {
        #[arg(long)]
        state_file: String,
        #[arg(long)]
        address: String,
        #[arg(long, default_value = "0")]
        plp: u128,
        #[arg(long, default_value = "0")]
        uplp: u128,
        /// Required flag: only allowed on testnet flows
        #[arg(long)]
        testnet: bool,
    },

    /// Compute deterministic state root from state file
    StateRoot {
        #[arg(long)]
        state_file: String,
    },

    /// L1: verify all transactions against state (balance, nonce, signature, fee)
    L1VerifyTxs {
        #[arg(long)]
        state_file: String,
        /// JSON array of transaction JSON strings
        #[arg(long)]
        txs: String,
    },

    /// L1: aggregate validator votes (JSON array of {node_id, yes})
    L1ProcessVotes {
        #[arg(long)]
        votes: String,
    },

    /// L2: aggregate block votes (JSON array of {node_id, yes})
    L2ProcessVotes {
        #[arg(long)]
        votes: String,
    },

    /// Assemble Core block header (merkle_root, state_root, block_hash)
    AssembleBlock {
        #[arg(long)]
        state_file: String,
        #[arg(long)]
        block_number: u64,
        #[arg(long)]
        previous_hash: String,
        #[arg(long)]
        timestamp: i64,
        /// JSON array of transaction hash strings
        #[arg(long)]
        tx_hashes: String,
        #[arg(long)]
        producer_id: String,
    },

    /// Start JSON-RPC server for Gateway native binding. TCP host:port or unix:/path
    Serve {
        /// Listen address, e.g. 127.0.0.1:19500 or unix:/tmp/platarium-core.sock
        #[arg(long)]
        listen: String,
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
        Commands::ValidateTx { tx, state_file } => handle_validate_tx(tx, state_file),
        Commands::StateInit { state_file } => handle_state_init(state_file),
        Commands::StateQuery {
            state_file,
            address,
            asset,
        } => handle_state_query(state_file, address, asset),
        Commands::StateValidateTx { state_file, tx } => handle_state_validate_tx(state_file, tx),
        Commands::StateApplyTx { state_file, tx } => handle_state_apply_tx(state_file, tx),
        Commands::StateCredit {
            state_file,
            address,
            plp,
            uplp,
            testnet,
        } => handle_state_credit(state_file, address, plp, uplp, testnet),
        Commands::StateRoot { state_file } => handle_state_root(state_file),
        Commands::L1VerifyTxs { state_file, txs } => handle_l1_verify_txs(state_file, txs),
        Commands::L1ProcessVotes { votes } => handle_l1_process_votes(votes),
        Commands::L2ProcessVotes { votes } => handle_l2_process_votes(votes),
        Commands::AssembleBlock {
            state_file,
            block_number,
            previous_hash,
            timestamp,
            tx_hashes,
            producer_id,
        } => handle_assemble_block(state_file, block_number, previous_hash, timestamp, tx_hashes, producer_id),
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
        Commands::Serve { listen } => handle_serve(listen),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn handle_serve(listen: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    platarium_core::core::core_rpc::run_serve(&listen)?;
    Ok(())
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

fn handle_validate_tx(
    tx_json: String,
    state_file: Option<String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = if let Some(path) = state_file {
        state_validate_tx_json(std::path::Path::new(&path), &tx_json)?
    } else {
        match Transaction::from_gateway_json(&tx_json)
            .and_then(|tx| tx.validate_basic().map_err(Into::into))
        {
            Ok(()) => serde_json::json!({ "valid": true }).to_string(),
            Err(e) => serde_json::json!({ "valid": false, "error": e.to_string() }).to_string(),
        }
    };
    println!("{}", out);
    Ok(())
}

fn handle_state_init(state_file: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(&state_file);
    init_state_file(path)?;
    println!(
        r#"{{"ok":true,"path":{}}}"#,
        serde_json::to_string(&state_file)?
    );
    Ok(())
}

fn handle_state_query(
    state_file: String,
    address: String,
    asset: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = state_query_json(std::path::Path::new(&state_file), &address, &asset)?;
    println!("{}", out);
    Ok(())
}

fn handle_state_validate_tx(
    state_file: String,
    tx: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = state_validate_tx_json(std::path::Path::new(&state_file), &tx)?;
    println!("{}", out);
    Ok(())
}

fn handle_state_apply_tx(
    state_file: String,
    tx: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = state_apply_tx_json(std::path::Path::new(&state_file), &tx)?;
    println!("{}", out);
    Ok(())
}

fn handle_state_credit(
    state_file: String,
    address: String,
    plp: u128,
    uplp: u128,
    testnet: bool,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = state_credit_json(std::path::Path::new(&state_file), &address, plp, uplp, testnet)?;
    println!("{}", out);
    Ok(())
}

fn handle_state_root(state_file: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = state_root_json(std::path::Path::new(&state_file))?;
    println!("{}", out);
    Ok(())
}

fn handle_l1_verify_txs(state_file: String, txs: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = l1_verify_txs_json(std::path::Path::new(&state_file), &txs)?;
    println!("{}", out);
    Ok(())
}

fn handle_l1_process_votes(votes: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = l1_process_votes_json(&votes)?;
    println!("{}", out);
    Ok(())
}

fn handle_l2_process_votes(votes: String) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = l2_process_votes_json(&votes)?;
    println!("{}", out);
    Ok(())
}

fn handle_assemble_block(
    state_file: String,
    block_number: u64,
    previous_hash: String,
    timestamp: i64,
    tx_hashes: String,
    producer_id: String,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out = assemble_block_json(
        std::path::Path::new(&state_file),
        block_number,
        &previous_hash,
        timestamp,
        &tx_hashes,
        &producer_id,
    )?;
    println!("{}", out);
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
    let sig_main = normalize_signature_hex(&sig_result.signatures[0].signature_compact);
    let sig_derived = normalize_signature_hex(&sig_result.signatures[1].signature_compact);
    let pub_main = sig_result.signatures[0].pub_key.clone();
    let pub_derived = sig_result.signatures[1].pub_key.clone();
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
        "pub_main": pub_main,
        "pub_derived": pub_derived,
    });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}