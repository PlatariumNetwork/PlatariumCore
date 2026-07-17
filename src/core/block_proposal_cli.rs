//! JSON CLI/RPC wrappers for gas-triggered block proposal.

use crate::core::block_proposal::{
    block_proposal_status, mempool_admit, min_fee_from_load_json, parse_mempool_snapshot,
    select_block_txs,
};
use crate::core::state_file::load_state_file;
use crate::error::Result;
use std::path::Path;

pub fn min_fee_from_load_cli(pending_count: usize) -> Result<String> {
    min_fee_from_load_json(pending_count)
}

pub fn mempool_admit_json(path: &Path, tx_json: &str, mempool_json: &str) -> Result<String> {
    let state = load_state_file(path)?;
    let mempool = parse_mempool_snapshot(mempool_json)?;
    let result = mempool_admit(&state, tx_json, &mempool);
    Ok(serde_json::to_string(&result).unwrap())
}

pub fn block_proposal_status_json(mempool_json: &str, now_unix: i64) -> Result<String> {
    let mempool = parse_mempool_snapshot(mempool_json)?;
    let status = block_proposal_status(&mempool, now_unix);
    Ok(serde_json::to_string(&status).unwrap())
}

pub fn select_block_txs_json(path: &Path, mempool_json: &str) -> Result<String> {
    let state = load_state_file(path)?;
    let mempool = parse_mempool_snapshot(mempool_json)?;
    let result = select_block_txs(&state, &mempool);
    Ok(serde_json::to_string(&result).unwrap())
}
