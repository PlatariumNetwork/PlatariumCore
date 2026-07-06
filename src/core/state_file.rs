//! Persistent JSON state file for gateway CLI integration.
//!
//! Atomic write via temp file + rename. Same transaction sequence yields the same file contents.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::asset::Asset;
use crate::core::execution::{ExecutionContext, ExecutionLogic};
use crate::core::state::{State, TREASURY_ADDRESS};
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};

pub const STATE_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateFileData {
    pub version: u32,
    /// (address, asset_canonical, balance as decimal string)
    pub asset_balances: Vec<(String, String, String)>,
    pub uplp_balances: Vec<(String, String)>,
    pub nonces: Vec<(String, u64)>,
}

impl StateFileData {
    pub fn empty() -> Self {
        Self {
            version: STATE_FILE_VERSION,
            asset_balances: Vec::new(),
            uplp_balances: Vec::new(),
            nonces: Vec::new(),
        }
    }

    pub fn from_state(state: &State) -> Self {
        let snap = state.create_snapshot();
        let mut asset_balances: Vec<(String, String, String)> = snap
            .asset_balances_arc()
            .iter()
            .map(|((addr, asset), bal)| (addr.clone(), asset.clone(), bal.to_string()))
            .collect();
        asset_balances.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut uplp_balances: Vec<(String, String)> = snap
            .uplp_balances_arc()
            .iter()
            .map(|(addr, bal)| (addr.clone(), bal.to_string()))
            .collect();
        uplp_balances.sort_by(|a, b| a.0.cmp(&b.0));

        let mut nonces: Vec<(String, u64)> = snap
            .nonces_arc()
            .iter()
            .map(|(addr, n)| (addr.clone(), *n))
            .collect();
        nonces.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            version: STATE_FILE_VERSION,
            asset_balances,
            uplp_balances,
            nonces,
        }
    }

    pub fn into_state(self) -> Result<State> {
        if self.version != STATE_FILE_VERSION {
            return Err(PlatariumError::State(format!(
                "unsupported state file version {}",
                self.version
            )));
        }
        let state = State::new();
        for (addr, asset, bal_str) in self.asset_balances {
            let bal: u128 = bal_str
                .parse()
                .map_err(|e| PlatariumError::State(format!("invalid balance for {}: {}", addr, e)))?;
            let asset_enum = if asset == Asset::PLP.as_canonical() {
                Asset::PLP
            } else if asset.starts_with("Token:") {
                Asset::Token(asset["Token:".len()..].to_string())
            } else {
                Asset::Token(asset)
            };
            state.set_asset_balance(&addr, &asset_enum, bal);
        }
        for (addr, bal_str) in self.uplp_balances {
            let bal: u128 = bal_str.parse().map_err(|e| {
                PlatariumError::State(format!("invalid uplp balance for {}: {}", addr, e))
            })?;
            state.set_uplp_balance(&addr, bal);
        }
        for (addr, nonce) in self.nonces {
            state.set_nonce(&addr, nonce);
        }
        Ok(state)
    }
}

pub fn init_state_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            PlatariumError::State(format!("create state dir {}: {}", parent.display(), e))
        })?;
    }
    let state = State::new();
    save_state_file(path, &state)
}

pub fn load_state_file(path: &Path) -> Result<State> {
    let data = fs::read_to_string(path).map_err(|e| {
        PlatariumError::State(format!("read state file {}: {}", path.display(), e))
    })?;
    let file: StateFileData = serde_json::from_str(&data).map_err(|e| {
        PlatariumError::State(format!("parse state file {}: {}", path.display(), e))
    })?;
    file.into_state()
}

pub fn save_state_file(path: &Path, state: &State) -> Result<()> {
    let file = StateFileData::from_state(state);
    let json = serde_json::to_string_pretty(&file).map_err(|e| {
        PlatariumError::State(format!("serialize state file: {}", e))
    })?;
    atomic_write(path, json.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            PlatariumError::State(format!("create state dir {}: {}", parent.display(), e))
        })?;
    }
    let tmp = temp_path(path);
    fs::write(&tmp, bytes).map_err(|e| {
        PlatariumError::State(format!("write temp state {}: {}", tmp.display(), e))
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        PlatariumError::State(format!(
            "rename state {} -> {}: {}",
            tmp.display(),
            path.display(),
            e
        ))
    })?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("state.json");
    tmp.set_file_name(format!("{}.tmp", name));
    tmp
}

pub fn with_state_file<F>(path: &Path, f: F) -> Result<()>
where
    F: FnOnce(&State) -> Result<()>,
{
    let state = if path.exists() {
        load_state_file(path)?
    } else {
        State::new()
    };
    f(&state)?;
    save_state_file(path, &state)
}

pub fn with_state_file_mut<F, T>(path: &Path, f: F) -> Result<T>
where
    F: FnOnce(&State) -> Result<T>,
{
    let state = if path.exists() {
        load_state_file(path)?
    } else {
        State::new()
    };
    let out = f(&state)?;
    save_state_file(path, &state)?;
    Ok(out)
}

pub fn state_query_json(path: &Path, address: &str, asset: &str) -> Result<String> {
    let state = load_state_file(path)?;
    let asset_enum = parse_asset(asset)?;
    let balance = if asset_enum == Asset::PLP {
        state.get_balance(&address.to_string())
    } else {
        state.get_asset_balance(&address.to_string(), &asset_enum)
    };
    let uplp = state.get_uplp_balance(&address.to_string());
    let nonce = state.get_nonce(&address.to_string());
    let fee_spendable = state.fee_spendable_uplp(&address.to_string());
    let out = serde_json::json!({
        "address": address,
        "asset": asset_enum.as_canonical(),
        "balance": balance.to_string(),
        "uplp_balance": uplp.to_string(),
        "fee_spendable_uplp": fee_spendable.to_string(),
        "nonce": nonce,
    });
    Ok(serde_json::to_string(&out).map_err(|e| PlatariumError::State(e.to_string()))?)
}

pub fn state_validate_tx_json(path: &Path, tx_json: &str) -> Result<String> {
    let state = load_state_file(path)?;
    let tx = Transaction::from_gateway_json(tx_json)?;
    match ExecutionLogic::validate_transaction(&tx)
        .and_then(|_| ExecutionLogic::check_transaction_applicability(&state, &tx))
    {
        Ok(()) => Ok(r#"{"valid":true}"#.to_string()),
        Err(e) => Ok(format!(
            r#"{{"valid":false,"error":{}}}"#,
            serde_json::to_string(&e.to_string())
                .map_err(|je| PlatariumError::State(je.to_string()))?
        )),
    }
}

pub fn state_apply_tx_json(path: &Path, tx_json: &str) -> Result<String> {
    with_state_file_mut(path, |state| {
        let tx = Transaction::from_gateway_json(tx_json)?;
        ExecutionLogic::execute_transaction(state, &tx, ExecutionContext::Production)?;
        let root = state.create_snapshot().compute_state_root();
        Ok(serde_json::json!({
            "ok": true,
            "hash": tx.hash,
            "state_root": root,
        }))
    })
    .map(|v| serde_json::to_string(&v).unwrap())
}

pub fn state_credit_json(path: &Path, address: &str, plp: u128, uplp: u128, testnet: bool) -> Result<String> {
    if !testnet {
        return Err(PlatariumError::State(
            "state-credit requires --testnet flag".into(),
        ));
    }
    with_state_file_mut(path, |state| {
        let addr = address.to_string();
        if plp > 0 {
            let current = state.get_balance(&addr);
            state.set_balance(&addr, current.saturating_add(plp));
        }
        if uplp > 0 {
            let current = state.get_uplp_balance(&addr);
            state.set_uplp_balance(&addr, current.saturating_add(uplp));
        }
        let root = state.create_snapshot().compute_state_root();
        Ok(serde_json::json!({
            "ok": true,
            "state_root": root,
            "treasury": TREASURY_ADDRESS,
        }))
    })
    .map(|v| serde_json::to_string(&v).unwrap())
}

pub fn state_root_json(path: &Path) -> Result<String> {
    let state = load_state_file(path)?;
    let root = state.create_snapshot().compute_state_root();
    Ok(serde_json::to_string(&serde_json::json!({ "state_root": root }))
        .map_err(|e| PlatariumError::State(e.to_string()))?)
}

fn parse_asset(asset: &str) -> Result<Asset> {
    if asset == "PLP" || asset.is_empty() {
        Ok(Asset::PLP)
    } else if asset.starts_with("Token:") {
        Ok(Asset::Token(asset["Token:".len()..].to_string()))
    } else {
        Ok(Asset::Token(asset.to_string()))
    }
}
