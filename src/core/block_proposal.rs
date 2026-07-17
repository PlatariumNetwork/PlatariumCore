//! Gas-triggered mempool admission and block tx selection (authoritative consensus rules).

use crate::core::asset::Asset;
use crate::core::consensus_params::{
    BLOCK_GAS_CAP_UPLP, BLOCK_MAX_TX_COUNT, BLOCK_MAX_WAIT_SEC, BLOCK_MIN_GAS_UPLP,
    BLOCK_MIN_TX_COUNT, FAUCET_ADDRESS,
};
use crate::core::execution::ExecutionLogic;
use crate::core::fee::calculate_fee_from_load;
use crate::core::state::State;
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One mempool entry from gateway snapshot (FIFO fairness).
#[derive(Debug, Clone)]
pub struct MempoolSnapshotEntry {
    pub tx: SnapshotTxFields,
    pub arrival_index: u64,
    pub timestamp: i64,
}

/// Transaction fields in gateway mempool JSON (flattened).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTxFields {
    pub hash: String,
    pub from: String,
    pub to: String,
    #[serde(default = "default_asset")]
    pub asset: String,
    #[serde(default)]
    pub amount: serde_json::Value,
    #[serde(default)]
    pub fee_uplp: serde_json::Value,
    #[serde(default)]
    pub fee: Option<String>,
    pub nonce: u64,
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub writes: Vec<String>,
    pub sig_main: String,
    pub sig_derived: String,
    #[serde(default)]
    pub pub_main: Option<String>,
    #[serde(default)]
    pub pub_derived: Option<String>,
    #[serde(default)]
    pub timestamp: i64,
}

fn default_asset() -> String {
    "PLP".to_string()
}

impl SnapshotTxFields {
    pub fn to_transaction(&self) -> Result<Transaction> {
        let json = serde_json::to_string(self)
            .map_err(|e| PlatariumError::State(format!("snapshot tx encode: {}", e)))?;
        Transaction::from_gateway_json(&json)
    }

    pub fn fee_uplp_u64(&self) -> u64 {
        parse_u64_value(&self.fee_uplp)
            .or_else(|| self.fee.as_ref().and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    }
}

fn parse_u64_value(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Parse gateway mempool snapshot JSON array.
pub fn parse_mempool_snapshot(json: &str) -> Result<Vec<MempoolSnapshotEntry>> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json)
        .map_err(|e| PlatariumError::State(format!("invalid mempool JSON array: {}", e)))?;
    let mut out = Vec::with_capacity(raw.len());
    for (i, v) in raw.into_iter().enumerate() {
        let arrival_index = v
            .get("arrival_index")
            .and_then(|x| x.as_u64())
            .unwrap_or(i as u64);
        let timestamp = v
            .get("timestamp")
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let tx: SnapshotTxFields = serde_json::from_value(v)
            .map_err(|e| PlatariumError::State(format!("invalid mempool entry: {}", e)))?;
        out.push(MempoolSnapshotEntry {
            tx,
            arrival_index,
            timestamp,
        });
    }
    out.sort_by(|a, b| {
        (a.arrival_index, a.tx.hash.as_str()).cmp(&(b.arrival_index, b.tx.hash.as_str()))
    });
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct MempoolAdmitResult {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub min_fee_uplp: u64,
    pub expected_nonce: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockProposalStatus {
    pub should_propose: bool,
    pub mempool_count: usize,
    pub mempool_gas_uplp: u64,
    pub block_gas_cap_uplp: u64,
    pub min_fee_uplp: u64,
    pub oldest_mempool_wait_sec: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectBlockTxsResult {
    pub hashes: Vec<String>,
    pub gas_used: u64,
    pub gas_cap: u64,
    pub tx_count: usize,
}

pub fn min_fee_from_load_json(pending_count: usize) -> Result<String> {
    let fee = calculate_fee_from_load(pending_count);
    Ok(serde_json::json!({ "min_fee_uplp": fee }).to_string())
}

pub fn mempool_admit(
    state: &State,
    tx_json: &str,
    mempool: &[MempoolSnapshotEntry],
) -> MempoolAdmitResult {
    let min_fee = calculate_fee_from_load(mempool.len());
    let tx = match Transaction::from_gateway_json(tx_json) {
        Ok(t) => t,
        Err(e) => {
            return MempoolAdmitResult {
                accepted: false,
                error: Some(e.to_string()),
                min_fee_uplp: min_fee,
                expected_nonce: 0,
            };
        }
    };

    if tx.from == FAUCET_ADDRESS {
        return MempoolAdmitResult {
            accepted: true,
            error: None,
            min_fee_uplp: min_fee,
            expected_nonce: tx.nonce,
        };
    }

    let chain_nonce = state.get_nonce(&tx.from);
    let expected = chain_nonce + pending_count_from(mempool, &tx.from);
    if tx.nonce != expected {
        return MempoolAdmitResult {
            accepted: false,
            error: Some(format!(
                "invalid nonce: expected {}, got {}",
                expected, tx.nonce
            )),
            min_fee_uplp: min_fee,
            expected_nonce: expected,
        };
    }

    if tx.fee_uplp < min_fee as u128 {
        return MempoolAdmitResult {
            accepted: false,
            error: Some(format!(
                "fee below minimum: need at least {} μPLP (load-based)",
                min_fee
            )),
            min_fee_uplp: min_fee,
            expected_nonce: expected,
        };
    }

    let (reserved_plp, reserved_fee) = pending_reserve(mempool, &tx.from);
    let fee_spendable = state.fee_spendable_uplp(&tx.from);
    let fee_need = reserved_fee + tx.fee_uplp;
    if fee_spendable < fee_need {
        return MempoolAdmitResult {
            accepted: false,
            error: Some(format!(
                "insufficient fee budget: need {} μPLP, available {} μPLP",
                fee_need, fee_spendable
            )),
            min_fee_uplp: min_fee,
            expected_nonce: expected,
        };
    }

    if tx.asset == Asset::PLP {
        let plp_bal = state.get_asset_balance(&tx.from, &Asset::PLP);
        let need = reserved_plp + tx.amount;
        if plp_bal < need {
            return MempoolAdmitResult {
                accepted: false,
                error: Some(format!(
                    "insufficient PLP balance: need {}, available {}",
                    need, plp_bal
                )),
                min_fee_uplp: min_fee,
                expected_nonce: expected,
            };
        }
    }

    if tx.nonce == chain_nonce {
        if let Err(e) = ExecutionLogic::validate_transaction(&tx)
            .and_then(|_| ExecutionLogic::check_transaction_applicability(state, &tx))
        {
            return MempoolAdmitResult {
                accepted: false,
                error: Some(e.to_string()),
                min_fee_uplp: min_fee,
                expected_nonce: expected,
            };
        }
    } else if let Err(e) = ExecutionLogic::validate_transaction(&tx) {
        return MempoolAdmitResult {
            accepted: false,
            error: Some(e.to_string()),
            min_fee_uplp: min_fee,
            expected_nonce: expected,
        };
    }

    MempoolAdmitResult {
        accepted: true,
        error: None,
        min_fee_uplp: min_fee,
        expected_nonce: expected,
    }
}

pub fn block_proposal_status(mempool: &[MempoolSnapshotEntry], now_unix: i64) -> BlockProposalStatus {
    let count = mempool.len();
    let gas = sum_fee_uplp(mempool);
    let min_fee = calculate_fee_from_load(count);
    let oldest_wait = oldest_wait_sec(mempool, now_unix);
    let should = if count == 0 {
        false
    } else if count >= BLOCK_MIN_TX_COUNT && gas >= BLOCK_MIN_GAS_UPLP {
        true
    } else {
        oldest_wait >= BLOCK_MAX_WAIT_SEC
    };
    BlockProposalStatus {
        should_propose: should,
        mempool_count: count,
        mempool_gas_uplp: gas,
        block_gas_cap_uplp: BLOCK_GAS_CAP_UPLP,
        min_fee_uplp: min_fee,
        oldest_mempool_wait_sec: oldest_wait,
    }
}

pub fn select_block_txs(state: &State, mempool: &[MempoolSnapshotEntry]) -> SelectBlockTxsResult {
    let mut chain_nonce: HashMap<String, u64> = HashMap::new();
    for e in mempool {
        if e.tx.from.is_empty() || e.tx.from == FAUCET_ADDRESS {
            continue;
        }
        chain_nonce
            .entry(e.tx.from.clone())
            .or_insert_with(|| state.get_nonce(&e.tx.from));
    }

    let mut next_nonce = chain_nonce.clone();
    let mut gas_used: u64 = 0;
    let mut hashes = Vec::new();

    for entry in mempool {
        if hashes.len() >= BLOCK_MAX_TX_COUNT {
            break;
        }
        let from = &entry.tx.from;
        if !from.is_empty() && from != FAUCET_ADDRESS {
            let want = *next_nonce.get(from).unwrap_or(&0);
            if entry.tx.nonce != want {
                continue;
            }
        }
        let fee = entry.tx.fee_uplp_u64();
        if gas_used.saturating_add(fee) > BLOCK_GAS_CAP_UPLP {
            break;
        }
        hashes.push(entry.tx.hash.clone());
        gas_used += fee;
        if !from.is_empty() {
            next_nonce.insert(from.clone(), entry.tx.nonce + 1);
        }
    }

    SelectBlockTxsResult {
        tx_count: hashes.len(),
        hashes,
        gas_used,
        gas_cap: BLOCK_GAS_CAP_UPLP,
    }
}

fn pending_count_from(mempool: &[MempoolSnapshotEntry], from: &str) -> u64 {
    mempool
        .iter()
        .filter(|e| e.tx.from == from)
        .count() as u64
}

fn pending_reserve(mempool: &[MempoolSnapshotEntry], from: &str) -> (u128, u128) {
    let mut plp: u128 = 0;
    let mut fee: u128 = 0;
    for e in mempool {
        if e.tx.from != from {
            continue;
        }
        if e.tx.asset == "PLP" || e.tx.asset.is_empty() {
            plp += parse_u128_value(&e.tx.amount).unwrap_or(0);
        }
        fee += e.tx.fee_uplp_u64() as u128;
    }
    (plp, fee)
}

fn parse_u128_value(v: &serde_json::Value) -> Option<u128> {
    v.as_u64()
        .map(|n| n as u128)
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn sum_fee_uplp(mempool: &[MempoolSnapshotEntry]) -> u64 {
    mempool.iter().map(|e| e.tx.fee_uplp_u64()).sum()
}

fn oldest_wait_sec(mempool: &[MempoolSnapshotEntry], now_unix: i64) -> i64 {
    let mut oldest = 0i64;
    for e in mempool {
        let ts = if e.timestamp > 0 {
            e.timestamp
        } else {
            e.tx.timestamp
        };
        if ts <= 0 {
            continue;
        }
        if oldest == 0 || ts < oldest {
            oldest = ts;
        }
    }
    if oldest <= 0 || now_unix <= oldest {
        0
    } else {
        now_unix - oldest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, from: &str, nonce: u64, fee: u64, idx: u64) -> MempoolSnapshotEntry {
        MempoolSnapshotEntry {
            tx: SnapshotTxFields {
                hash: hash.to_string(),
                from: from.to_string(),
                to: "PxB".to_string(),
                asset: "PLP".to_string(),
                amount: serde_json::json!(1),
                fee_uplp: serde_json::json!(fee),
                fee: None,
                nonce,
                reads: vec![],
                writes: vec![],
                sig_main: "aa".repeat(64),
                sig_derived: "bb".repeat(64),
                pub_main: None,
                pub_derived: None,
                timestamp: 0,
            },
            arrival_index: idx,
            timestamp: 0,
        }
    }

    #[test]
    fn select_respects_gas_cap() {
        let state = State::new();
        let mempool = vec![
            entry("a", "PxA", 0, 2000, 0),
            entry("b", "PxB", 0, 2000, 1),
            entry("c", "PxC", 0, 2000, 2),
        ];
        let r = select_block_txs(&state, &mempool);
        assert_eq!(r.tx_count, 2);
        assert_eq!(r.gas_used, 4000);
        assert_eq!(r.hashes, vec!["a", "b"]);
    }

    #[test]
    fn select_skips_nonce_gap_in_fifo() {
        let state = State::new();
        let mempool = vec![
            entry("late", "PxA", 1, 1, 0),
            entry("first", "PxA", 0, 1, 1),
        ];
        let r = select_block_txs(&state, &mempool);
        assert_eq!(r.hashes, vec!["first"]);
    }

    #[test]
    fn should_propose_with_one_tx() {
        let mempool = vec![entry("x", "PxA", 0, 1, 0)];
        let s = block_proposal_status(&mempool, 100);
        assert!(s.should_propose);
        assert_eq!(s.mempool_gas_uplp, 1);
    }

    #[test]
    fn should_propose_max_wait() {
        let mut e = entry("w", "PxA", 0, 1, 0);
        e.timestamp = 1;
        e.tx.timestamp = 1;
        let s = block_proposal_status(&[e], 1 + BLOCK_MAX_WAIT_SEC);
        assert!(s.should_propose);
        assert!(s.oldest_mempool_wait_sec >= BLOCK_MAX_WAIT_SEC);
    }

    #[test]
    fn select_skips_gap_then_includes() {
        let state = State::new();
        let mut mempool = vec![entry("n1", "PxA", 1, 1, 0)];
        let r = select_block_txs(&state, &mempool);
        assert!(r.hashes.is_empty());
        mempool.push(entry("n0", "PxA", 0, 1, 1));
        let r = select_block_txs(&state, &mempool);
        assert_eq!(r.hashes, vec!["n0"]);
        // After n0 would be applied, n1 becomes selectable — simulate with nonce bump.
        state.set_nonce(&"PxA".to_string(), 1);
        let mempool = vec![entry("n1", "PxA", 1, 1, 0)];
        let r = select_block_txs(&state, &mempool);
        assert_eq!(r.hashes, vec!["n1"]);
    }

    #[test]
    fn mempool_admit_rejects_bad_nonce() {
        let state = State::new();
        state.set_balance(&"PxA".to_string(), 1000);
        let tx = r#"{"hash":"t1","from":"PxA","to":"PxB","asset":"PLP","amount":1,"fee_uplp":1,"nonce":5,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#;
        let r = mempool_admit(&state, tx, &[]);
        assert!(!r.accepted);
        assert_eq!(r.expected_nonce, 0);
    }

    #[test]
    fn mempool_admit_rejects_low_fee() {
        let state = State::new();
        state.set_balance(&"PxA".to_string(), 1000);
        // High mempool load → min fee 5; tx fee 1 must be rejected.
        let pending: Vec<_> = (0..810)
            .map(|i| entry(&format!("p{i}"), "PxOther", 0, 5, i))
            .collect();
        let tx = r#"{"hash":"low","from":"PxA","to":"PxB","asset":"PLP","amount":1,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#;
        let r = mempool_admit(&state, tx, &pending);
        assert!(!r.accepted);
    }

    #[test]
    fn mempool_admit_reserves_balance() {
        let state = State::new();
        state.set_balance(&"PxA".to_string(), 10);
        let pending = vec![entry("p0", "PxA", 0, 1, 0)];
        // amount 10 + fee 1 > remaining after pending reserve
        let tx = r#"{"hash":"t2","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":1,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#;
        let r = mempool_admit(&state, tx, &pending);
        assert!(!r.accepted);
    }
}
