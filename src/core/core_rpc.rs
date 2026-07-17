//! JSON-RPC 2.0 server for Gateway native Core binding.
//! Newline-delimited JSON over TCP or Unix domain socket.

use crate::core::asset::Asset;
use crate::core::block_proposal_cli::{
    block_proposal_status_json, mempool_admit_json, min_fee_from_load_cli, select_block_txs_json,
};
use crate::core::consensus_cli::{
    assemble_block_json, l1_process_votes_json, l1_verify_txs_json, l2_process_votes_json,
};
use crate::core::state_file::{
    init_state_file, state_apply_tx_json, state_credit_json, state_query_json, state_root_json,
    state_validate_tx_json,
};
use crate::core::transaction::Transaction;
use crate::core::validator_selection::{
    committee_count, select_n_by_weight, selection_percent_from_load_pct,
};
use crate::error::{PlatariumError, Result};
use crate::signature::normalize_signature_hex;
use crate::signer::sign_with_both_keys;
use crate::{
    generate_alphanumeric_part, generate_mnemonic, validate_mnemonic, verify_signature, KeyGenerator,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;

fn param_str(params: &Value, key: &str) -> Result<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| PlatariumError::State(format!("missing param {}", key)))
}

fn param_u64(params: &Value, key: &str) -> Result<u64> {
    if let Some(n) = params.get(key).and_then(|v| v.as_u64()) {
        return Ok(n);
    }
    if let Some(n) = params.get(key).and_then(|v| v.as_i64()) {
        if n >= 0 {
            return Ok(n as u64);
        }
    }
    if let Some(s) = params.get(key).and_then(|v| v.as_str()) {
        return s
            .parse()
            .map_err(|_| PlatariumError::State(format!("invalid param {}", key)));
    }
    Err(PlatariumError::State(format!("missing param {}", key)))
}

fn param_i64(params: &Value, key: &str) -> Result<i64> {
    if let Some(n) = params.get(key).and_then(|v| v.as_i64()) {
        return Ok(n);
    }
    if let Some(n) = params.get(key).and_then(|v| v.as_u64()) {
        return Ok(n as i64);
    }
    if let Some(s) = params.get(key).and_then(|v| v.as_str()) {
        return s
            .parse()
            .map_err(|_| PlatariumError::State(format!("invalid param {}", key)));
    }
    Err(PlatariumError::State(format!("missing param {}", key)))
}

fn param_usize(params: &Value, key: &str) -> Result<usize> {
    Ok(param_u64(params, key)? as usize)
}

fn param_bool(params: &Value, key: &str) -> bool {
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn param_opt_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Dispatch one JSON-RPC method to Core logic. Returns JSON result string.
pub fn dispatch_rpc(method: &str, params: &Value) -> Result<String> {
    match method {
        "ping" => Ok(json!({"ok": true, "service": "platarium-core-rpc", "version": "1.0.0"}).to_string()),

        "state_init" => {
            let path = param_str(params, "state_file")?;
            init_state_file(Path::new(&path))?;
            Ok(json!({"ok": true, "path": path}).to_string())
        }
        "state_query" => {
            let path = param_str(params, "state_file")?;
            let address = param_str(params, "address")?;
            let asset = param_opt_str(params, "asset").unwrap_or_else(|| "PLP".to_string());
            state_query_json(Path::new(&path), &address, &asset)
        }
        "state_validate_tx" => {
            let path = param_str(params, "state_file")?;
            let tx = param_str(params, "tx")?;
            state_validate_tx_json(Path::new(&path), &tx)
        }
        "state_apply_tx" => {
            let path = param_str(params, "state_file")?;
            let tx = param_str(params, "tx")?;
            state_apply_tx_json(Path::new(&path), &tx)
        }
        "state_credit" => {
            let path = param_str(params, "state_file")?;
            let address = param_str(params, "address")?;
            let plp = param_u64(params, "plp")? as u128;
            let uplp = param_u64(params, "uplp")? as u128;
            let testnet = param_bool(params, "testnet");
            state_credit_json(Path::new(&path), &address, plp, uplp, testnet)
        }
        "state_root" => {
            let path = param_str(params, "state_file")?;
            state_root_json(Path::new(&path))
        }

        "validate_tx" => {
            let tx = param_str(params, "tx")?;
            if let Some(path) = param_opt_str(params, "state_file") {
                state_validate_tx_json(Path::new(&path), &tx)
            } else {
                match Transaction::from_gateway_json(&tx)
                    .and_then(|tx| tx.validate_basic().map_err(Into::into))
                {
                    Ok(()) => Ok(json!({"valid": true}).to_string()),
                    Err(e) => Ok(json!({"valid": false, "error": e.to_string()}).to_string()),
                }
            }
        }

        "l1_verify_txs" => {
            let path = param_str(params, "state_file")?;
            let txs = param_str(params, "txs")?;
            l1_verify_txs_json(Path::new(&path), &txs)
        }
        "l1_process_votes" => {
            let votes = param_str(params, "votes")?;
            l1_process_votes_json(&votes)
        }
        "l2_process_votes" => {
            let votes = param_str(params, "votes")?;
            l2_process_votes_json(&votes)
        }
        "assemble_block" => {
            let path = param_str(params, "state_file")?;
            let block_number = param_u64(params, "block_number")?;
            let previous_hash = param_str(params, "previous_hash")?;
            let timestamp = param_i64(params, "timestamp")?;
            let tx_hashes = param_str(params, "tx_hashes")?;
            let producer_id = param_str(params, "producer_id")?;
            assemble_block_json(
                Path::new(&path),
                block_number,
                &previous_hash,
                timestamp,
                &tx_hashes,
                &producer_id,
            )
        }

        "min_fee_from_load" => {
            let pending = param_usize(params, "pending_count")?;
            min_fee_from_load_cli(pending)
        }
        "mempool_admit" => {
            let path = param_str(params, "state_file")?;
            let tx = param_str(params, "tx")?;
            let mempool_txs = param_str(params, "mempool_txs")?;
            mempool_admit_json(Path::new(&path), &tx, &mempool_txs)
        }
        "block_proposal_status" => {
            let mempool_txs = param_str(params, "mempool_txs")?;
            let now_unix = param_i64(params, "now_unix")?;
            block_proposal_status_json(&mempool_txs, now_unix)
        }
        "select_block_txs" => {
            let path = param_str(params, "state_file")?;
            let mempool_txs = param_str(params, "mempool_txs")?;
            select_block_txs_json(Path::new(&path), &mempool_txs)
        }

        "rocks_get_head" => {
            let db_path = param_str(params, "db_path")?;
            crate::storage::rpc::rocks_get_head_json(&db_path)
        }
        "rocks_get_tx" => {
            let db_path = param_str(params, "db_path")?;
            let tx_hash = param_str(params, "tx_hash")?;
            crate::storage::rpc::rocks_get_tx_json(&db_path, &tx_hash)
        }
        "rocks_get_block" => {
            let db_path = param_str(params, "db_path")?;
            let height = param_u64(params, "height")?;
            crate::storage::rpc::rocks_get_block_json(&db_path, height)
        }
        "rocks_get_account" => {
            let db_path = param_str(params, "db_path")?;
            let address = param_str(params, "address")?;
            crate::storage::rpc::rocks_get_account_json(&db_path, &address)
        }
        "rocks_list_address_txs" => {
            let db_path = param_str(params, "db_path")?;
            let address = param_str(params, "address")?;
            crate::storage::rpc::rocks_list_address_txs_json(&db_path, &address)
        }
        "rocks_commit_block" => {
            let db_path = param_str(params, "db_path")?;
            let commit = param_str(params, "commit")?;
            crate::storage::rpc::rocks_commit_block_json(&db_path, &commit)
        }
        "rocks_list_snapshots" => {
            let db_path = param_str(params, "db_path")?;
            crate::storage::rpc::rocks_list_snapshots_json(&db_path)
        }
        "rocks_bootstrap_snapshot" => {
            let db_path = param_str(params, "db_path")?;
            let snapshot = param_str(params, "snapshot")?;
            crate::storage::rpc::rocks_bootstrap_snapshot_json(&db_path, &snapshot)
        }
        "migrate_json_to_rocks" => {
            let db_path = param_str(params, "db_path")?;
            let chain = param_str(params, "chain_json")?;
            let accounts = param_opt_str(params, "accounts_json");
            crate::storage::rpc::migrate_json_to_rocks(&db_path, &chain, accounts.as_deref())
        }

        "selection_percent_from_load" => {
            let load_pct = param_u64(params, "load_pct")?;
            let percent = selection_percent_from_load_pct(load_pct)
                .map_err(|e| PlatariumError::State(e.to_string()))?;
            Ok(json!({"percent": percent}).to_string())
        }
        "committee_count" => {
            let candidates = param_usize(params, "candidates")?;
            let load_pct = param_u64(params, "load_pct")?;
            let count = committee_count(candidates, load_pct);
            Ok(json!({"count": count}).to_string())
        }
        "select_committee" => {
            #[derive(serde::Deserialize)]
            struct Candidate {
                id: String,
                weight: u64,
            }
            let candidates_raw = param_str(params, "candidates")?;
            let seed_hex = param_str(params, "seed_hex")?;
            let count = param_usize(params, "count")?;
            let list: Vec<Candidate> = serde_json::from_str(&candidates_raw)
                .map_err(|e| PlatariumError::State(format!("invalid candidates JSON: {}", e)))?;
            let pairs: Vec<(String, u64)> = list.into_iter().map(|c| (c.id, c.weight)).collect();
            let seed_bytes: Vec<u8> = hex::decode(seed_hex.trim())
                .map_err(|e| PlatariumError::State(format!("invalid seed_hex: {}", e)))?;
            let mut seed = [0u8; 32];
            if seed_bytes.len() != 32 {
                return Err(PlatariumError::State(
                    "seed_hex must be 64 hex chars (32 bytes)".into(),
                ));
            }
            seed.copy_from_slice(&seed_bytes[..32]);
            let selected = select_n_by_weight(pairs, &seed, count);
            Ok(serde_json::to_string(&selected).map_err(|e| PlatariumError::State(e.to_string()))?)
        }

        "generate_mnemonic" => {
            let (mnemonic, alphanumeric) = generate_mnemonic()?;
            Ok(json!({"mnemonic": mnemonic, "alphanumeric": alphanumeric}).to_string())
        }

        "generate_keys" => {
            let mnemonic = param_str(params, "mnemonic")?;
            if !validate_mnemonic(&mnemonic) {
                return Err(PlatariumError::State("Invalid mnemonic phrase".into()));
            }
            let alphanumeric_part = param_opt_str(params, "alphanumeric").unwrap_or_else(|| {
                generate_alphanumeric_part(12).unwrap_or_default()
            });
            let seed_index = param_u64(params, "seed_index").unwrap_or(0) as u32;
            let path = param_opt_str(params, "path");
            let key_gen = KeyGenerator::new(seed_index, None, None, path.clone())?;
            let keys = key_gen.restore_keys(&mnemonic, &alphanumeric_part, seed_index, path)?;
            Ok(json!({
                "publicKey": keys.public_key,
                "privateKey": keys.private_key,
                "signatureKey": keys.signature_key,
                "derivationPath": keys.derivation_paths.main_path,
                "alphanumeric": keys.alphanumeric_part,
            })
            .to_string())
        }

        "verify_signature" => {
            let message_str = param_str(params, "message")?;
            let signature = param_str(params, "signature")?;
            let pubkey = param_str(params, "pubkey")?;
            let message: Value = serde_json::from_str(&message_str)
                .map_err(|e| PlatariumError::State(format!("Invalid JSON message: {}", e)))?;
            let verified = verify_signature(&message, &signature, &pubkey)?;
            Ok(json!({"verified": verified}).to_string())
        }

        "sign_message" => {
            let message_str = param_str(params, "message")?;
            let mnemonic = param_str(params, "mnemonic")?;
            let alphanumeric = param_str(params, "alphanumeric")?;
            if !validate_mnemonic(&mnemonic) {
                return Err(PlatariumError::State("Invalid mnemonic phrase".into()));
            }
            let message: Value = serde_json::from_str(&message_str)
                .map_err(|e| PlatariumError::State(format!("Invalid JSON message: {}", e)))?;
            let signature_result = sign_with_both_keys(&message, &mnemonic, &alphanumeric)?;
            Ok(json!({
                "hash": signature_result.hash,
                "signatures": signature_result.signatures.iter().map(|s| json!({
                    "sig_type": s.sig_type,
                    "r": s.r,
                    "s": s.s,
                    "pub_key": s.pub_key,
                    "der": s.der,
                    "signature_compact": s.signature_compact,
                })).collect::<Vec<_>>(),
            })
            .to_string())
        }

        "sign_transaction" => {
            let from = param_str(params, "from")?;
            let to = param_str(params, "to")?;
            let asset = param_opt_str(params, "asset").unwrap_or_else(|| "PLP".to_string());
            let amount = param_u64(params, "amount")?;
            let fee_uplp = param_u64(params, "fee_uplp")?;
            let nonce = param_u64(params, "nonce")?;
            let reads = params
                .get("reads")
                .and_then(|v| v.as_str())
                .unwrap_or("[]")
                .to_string();
            let writes = params
                .get("writes")
                .and_then(|v| v.as_str())
                .unwrap_or("[]")
                .to_string();
            let mnemonic = param_str(params, "mnemonic")?;
            let alphanumeric = param_str(params, "alphanumeric")?;
            if !validate_mnemonic(&mnemonic) {
                return Err(PlatariumError::State("Invalid mnemonic phrase".into()));
            }
            let reads_vec: Vec<String> = serde_json::from_str(&reads)
                .map_err(|e| PlatariumError::State(format!("invalid reads JSON: {}", e)))?;
            let writes_vec: Vec<String> = serde_json::from_str(&writes)
                .map_err(|e| PlatariumError::State(format!("invalid writes JSON: {}", e)))?;
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
            let reads_out: Vec<String> = reads_set.iter().cloned().collect();
            let writes_out: Vec<String> = writes_set.iter().cloned().collect();
            Ok(json!({
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
            })
            .to_string())
        }

        other => Err(PlatariumError::State(format!("unknown method: {}", other))),
    }
}

pub fn handle_rpc_line(line: &str) -> String {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("parse error: {}", e)}
            })
            .to_string();
        }
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    if method.is_empty() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32600, "message": "missing method"}
        })
        .to_string();
    }

    match dispatch_rpc(method, &params) {
        Ok(result_str) => {
            let result: Value =
                serde_json::from_str(&result_str).unwrap_or(Value::String(result_str));
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            })
            .to_string()
        }
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": e.to_string()}
        })
        .to_string(),
    }
}

fn serve_connection<S: std::io::Read + Write + Send + 'static>(stream: S) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }
                let response = handle_rpc_line(&line);
                if writeln!(reader.get_mut(), "{}", response).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Run JSON-RPC server on TCP `host:port` or Unix socket `unix:/path` (Unix only).
pub fn run_serve(listen: &str) -> Result<()> {
    if let Some(path) = listen.strip_prefix("unix:") {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixListener;
            let _ = std::fs::remove_file(path);
            let listener = UnixListener::bind(path)
                .map_err(|e| PlatariumError::State(format!("unix bind {}: {}", path, e)))?;
            eprintln!("[core-rpc] listening on unix:{}", path);
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        std::thread::spawn(move || serve_connection(s));
                    }
                    Err(e) => eprintln!("[core-rpc] accept error: {}", e),
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            return Err(PlatariumError::State("unix sockets not supported on this platform".into()));
        }
    } else {
        let listener = TcpListener::bind(listen)
            .map_err(|e| PlatariumError::State(format!("tcp bind {}: {}", listen, e)))?;
        eprintln!("[core-rpc] listening on {}", listen);
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    std::thread::spawn(move || serve_connection(s));
                }
                Err(e) => eprintln!("[core-rpc] accept error: {}", e),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_ping() {
        let out = dispatch_rpc("ping", &json!({})).unwrap();
        assert!(out.contains("\"ok\":true"));
    }

    #[test]
    fn test_handle_rpc_line() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}"#;
        let resp = handle_rpc_line(line);
        assert!(resp.contains("\"result\""));
        assert!(resp.contains("\"id\":1"));
    }
}
