//! Ledger consistency diagnostics (issues #61–#65).
//!
//! Read-only checks — never mutates ledgers (no auto-repair). Compares execution
//! tip / accounts against Rocks canonical tip when an execution view is supplied.

use crate::core::asset::Asset;
use crate::core::crash_failpoints::{assert_height_hash_invariant, read_canonical_tip};
use crate::core::state::State;
use crate::error::{PlatariumError, Result};
use crate::storage::cache::open_cached;
use crate::storage::commit::AccountRecord;
use crate::storage::query::{get_account, get_block, get_head, get_state_root, list_accounts};
use crate::storage::rocks::RocksStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// Machine-readable consistency status (issues #62/#63; fixtures #58/#59).
pub const STATUS_CONSISTENT: &str = "consistent";
pub const STATUS_DIVERGED: &str = "diverged";

/// Reason code when account fields disagree (issue #63).
pub const REASON_ACCOUNT_STATE_MISMATCH: &str = "account_state_mismatch";
pub const REASON_HEIGHT_MISMATCH: &str = "height_mismatch";
pub const REASON_HASH_MISMATCH: &str = "hash_mismatch";

/// Result of [`check_consistency`] / [`check_consistency_store`] (Rocks-internal).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsistencyReport {
    pub ok: bool,
    pub head: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl ConsistencyReport {
    fn ok_report(head: u64) -> Self {
        Self {
            ok: true,
            head,
            errors: Vec::new(),
        }
    }
}

/// Execution-side tip view for comparison against Rocks (issue #62).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionTipView {
    pub height: u64,
    pub block_hash: String,
}

/// Verdict of execution-vs-Rocks consistency (issues #62/#63).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsistencyVerdict {
    /// `"consistent"` or `"diverged"`.
    pub status: String,
    pub height: u64,
    pub rocks_head: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ConsistencyVerdict {
    fn consistent(height: u64, rocks_head: u64, state_root: Option<String>) -> Self {
        Self {
            status: STATUS_CONSISTENT.into(),
            height,
            rocks_head,
            state_root,
            reason: None,
        }
    }

    fn diverged(
        height: u64,
        rocks_head: u64,
        state_root: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            status: STATUS_DIVERGED.into(),
            height,
            rocks_head,
            state_root,
            reason: Some(reason.into()),
        }
    }

    pub fn is_consistent(&self) -> bool {
        self.status == STATUS_CONSISTENT
    }
}

/// Diagnostic consistency check for a RocksDB path (does not mutate).
pub fn check_consistency(db_path: impl AsRef<Path>) -> Result<ConsistencyReport> {
    let store = open_cached(db_path.as_ref())?;
    check_consistency_store(store.as_ref())
}

/// Same as [`check_consistency`] against an already-open store.
pub fn check_consistency_store(store: &RocksStore) -> Result<ConsistencyReport> {
    let head = get_head(store)?;
    if head == 0 {
        return Ok(ConsistencyReport::ok_report(0));
    }

    let mut errors = Vec::new();
    for h in 1..=head {
        match get_block(store, h)? {
            None => errors.push(format!("missing block at height {h}")),
            Some(block) => {
                if block.height != h {
                    errors.push(format!(
                        "block height mismatch at key {h}: record says {}",
                        block.height
                    ));
                }
                match get_state_root(store, h)? {
                    None => errors.push(format!("missing state_root at height {h}")),
                    Some(root) => {
                        if root != block.state_root {
                            errors.push(format!(
                                "state_root mismatch at height {h}: meta={root} block={}",
                                block.state_root
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(ConsistencyReport {
        ok: errors.is_empty(),
        head,
        errors,
    })
}

/// JSON wrapper for RPC/CLI (issue #61).
pub fn check_consistency_json(db_path: &str) -> Result<String> {
    let report = check_consistency(Path::new(db_path))?;
    serde_json::to_string(&report).map_err(|e| {
        PlatariumError::State(format!("encode consistency report: {e}"))
    })
}

/// Compare execution tip height/hash to Rocks tip (issue #62).
///
/// On mismatch → `status=diverged` with reason. On match → contributes to
/// `status=consistent` (account checks may still diverge when `state` is given).
pub fn compare_height_hash(
    exec: &ExecutionTipView,
    store: &RocksStore,
) -> Result<ConsistencyVerdict> {
    let rocks = read_canonical_tip(store)?;
    let state_root = if rocks.height == 0 {
        None
    } else {
        Some(rocks.state_root.clone())
    };
    if exec.height != rocks.height {
        return Ok(ConsistencyVerdict::diverged(
            exec.height,
            rocks.height,
            state_root,
            format!(
                "{REASON_HEIGHT_MISMATCH}: execution_height={} rocks_head={}",
                exec.height, rocks.height
            ),
        ));
    }
    if exec.block_hash != rocks.block_hash {
        return Ok(ConsistencyVerdict::diverged(
            exec.height,
            rocks.height,
            state_root,
            format!(
                "{REASON_HASH_MISMATCH}: execution_hash={} rocks_hash={}",
                exec.block_hash, rocks.block_hash
            ),
        ));
    }
    // Shared height/hash pairing on Rocks tip (issue #60 helper).
    let _ = assert_height_hash_invariant(store)?;
    Ok(ConsistencyVerdict::consistent(
        exec.height,
        rocks.height,
        state_root,
    ))
}

/// Compare execution state vs Rocks for tip height/hash and account fields (issues #62/#63).
///
/// - Height/hash mismatch → diverged with `height_mismatch` / `hash_mismatch`.
/// - Account field mismatch → diverged with `account_state_mismatch` (includes address+field).
/// - `sample_addresses`: when `Some`, only those addresses; when `None`, all known addresses
///   from execution state ∪ Rocks accounts.
pub fn check_execution_vs_rocks(
    state: &State,
    exec: &ExecutionTipView,
    store: &RocksStore,
    sample_addresses: Option<&[String]>,
) -> Result<ConsistencyVerdict> {
    let tip_check = compare_height_hash(exec, store)?;
    if !tip_check.is_consistent() {
        return Ok(tip_check);
    }

    let rocks_head = tip_check.rocks_head;
    let state_root = tip_check.state_root.clone();

    let addresses = resolve_addresses(state, store, sample_addresses)?;
    for addr in addresses {
        if let Some(reason) = compare_one_account(state, store, &addr)? {
            return Ok(ConsistencyVerdict::diverged(
                exec.height,
                rocks_head,
                state_root,
                reason,
            ));
        }
    }

    Ok(ConsistencyVerdict::consistent(
        exec.height,
        rocks_head,
        state_root,
    ))
}

/// Machine-readable JSON for a [`ConsistencyVerdict`] (issues #64 / #65 fixtures).
///
/// Consistent shape: `{status, height, rocks_head, state_root?}`.
/// Diverged shape: adds `reason`. Never mutates ledgers.
pub fn consistency_verdict_json(verdict: &ConsistencyVerdict) -> Result<String> {
    serde_json::to_string(verdict).map_err(|e| {
        PlatariumError::State(format!("encode consistency verdict: {e}"))
    })
}

fn resolve_addresses(
    state: &State,
    store: &RocksStore,
    sample: Option<&[String]>,
) -> Result<Vec<String>> {
    if let Some(addrs) = sample {
        let mut v: Vec<String> = addrs.to_vec();
        v.sort();
        v.dedup();
        return Ok(v);
    }
    let mut set = BTreeSet::new();
    for (addr, _) in state.get_all_balances() {
        set.insert(addr);
    }
    for (addr, _) in state.get_all_nonces() {
        set.insert(addr);
    }
    for rec in list_accounts(store)? {
        set.insert(rec.address);
    }
    Ok(set.into_iter().collect())
}

fn compare_one_account(
    state: &State,
    store: &RocksStore,
    addr: &str,
) -> Result<Option<String>> {
    let exec_balance = state.get_balance(&addr.to_string()).to_string();
    let exec_nonce = state.get_nonce(&addr.to_string());
    let exec_tokens = state.token_balances_of(&addr.to_string());
    let exec_xp = exec_tokens
        .get(&Asset::xp().as_canonical())
        .cloned()
        .unwrap_or_else(|| "0".into());

    let rocks = get_account(store, addr)?;
    let (rocks_balance, rocks_nonce, rocks_tokens, rocks_xp) = match rocks {
        Some(AccountRecord {
            balance,
            nonce,
            tokens,
            xp,
            ..
        }) => (balance, nonce, tokens, if xp.is_empty() { "0".into() } else { xp }),
        None => {
            // Absent in Rocks is OK only if execution also has zeroed defaults.
            if exec_balance == "0" && exec_nonce == 0 && exec_tokens.is_empty() && exec_xp == "0" {
                return Ok(None);
            }
            return Ok(Some(format!(
                "{REASON_ACCOUNT_STATE_MISMATCH}: address={addr} field=presence execution=present rocks=missing"
            )));
        }
    };

    if exec_balance != rocks_balance {
        return Ok(Some(format!(
            "{REASON_ACCOUNT_STATE_MISMATCH}: address={addr} field=balance execution={exec_balance} rocks={rocks_balance}"
        )));
    }
    if exec_nonce != rocks_nonce {
        return Ok(Some(format!(
            "{REASON_ACCOUNT_STATE_MISMATCH}: address={addr} field=nonce execution={exec_nonce} rocks={rocks_nonce}"
        )));
    }
    if exec_tokens != rocks_tokens {
        return Ok(Some(format!(
            "{REASON_ACCOUNT_STATE_MISMATCH}: address={addr} field=tokens execution={exec_tokens:?} rocks={rocks_tokens:?}"
        )));
    }
    if exec_xp != rocks_xp {
        return Ok(Some(format!(
            "{REASON_ACCOUNT_STATE_MISMATCH}: address={addr} field=xp execution={exec_xp} rocks={rocks_xp}"
        )));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::storage::cache::evict_cached;
    use crate::storage::commit::{
        commit_block, AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord,
    };
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn sample_commit(height: u64, state_root: &str, block_hash: &str) -> BlockCommit {
        let mut tokens = BTreeMap::new();
        tokens.insert(Asset::xp().as_canonical(), "40".into());
        BlockCommit {
            block: BlockRecordStored {
                height,
                previous_hash: "0".into(),
                timestamp: 1,
                tx_hashes: vec!["aabb".into()],
                merkle_root: "m".into(),
                state_root: state_root.into(),
                block_hash: block_hash.into(),
                producer_id: "n1".into(),
            },
            tx_jsons: vec![
                r#"{"hash":"aabb","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
            ],
            accounts: vec![AccountRecord {
                address: "PxA".into(),
                balance: "90".into(),
                uplp_balance: "0".into(),
                nonce: 1,
                tokens: tokens.clone(),
                xp: "40".into(),
            }],
            receipts: vec![ReceiptRecord {
                tx_hash: "aabb".into(),
                status: "ok".into(),
                fee_uplp: 1,
                block_height: height,
            }],
            state_root: state_root.into(),
        }
    }

    #[test]
    fn empty_db_is_consistent_and_does_not_mutate() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let before = check_consistency(&db_path).unwrap();
        assert!(before.ok);
        assert_eq!(before.head, 0);
        let after = check_consistency(&db_path).unwrap();
        assert_eq!(before, after);
        evict_cached(&db_path);
    }

    #[test]
    fn committed_chain_passes_consistency() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        {
            let store = crate::storage::rocks::RocksStore::open(&db_path).unwrap();
            commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();
        }
        let report = check_consistency(&db_path).unwrap();
        assert!(report.ok, "{:?}", report.errors);
        assert_eq!(report.head, 1);
        evict_cached(&db_path);
    }

    /// Issue #62: height/hash match → consistent; mismatch → diverged with reason.
    #[test]
    fn height_hash_match_and_mismatch() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();

        let ok = compare_height_hash(
            &ExecutionTipView {
                height: 1,
                block_hash: "bh1".into(),
            },
            &store,
        )
        .unwrap();
        assert!(ok.is_consistent(), "{ok:?}");
        assert_eq!(ok.status, STATUS_CONSISTENT);

        let bad_h = compare_height_hash(
            &ExecutionTipView {
                height: 2,
                block_hash: "bh1".into(),
            },
            &store,
        )
        .unwrap();
        assert!(!bad_h.is_consistent());
        assert_eq!(bad_h.status, STATUS_DIVERGED);
        assert!(
            bad_h.reason.as_ref().unwrap().contains(REASON_HEIGHT_MISMATCH),
            "{bad_h:?}"
        );

        let bad_hash = compare_height_hash(
            &ExecutionTipView {
                height: 1,
                block_hash: "other".into(),
            },
            &store,
        )
        .unwrap();
        assert!(!bad_hash.is_consistent());
        assert!(
            bad_hash
                .reason
                .as_ref()
                .unwrap()
                .contains(REASON_HASH_MISMATCH),
            "{bad_hash:?}"
        );
        evict_cached(&db_path);
    }

    /// Issue #63: nonce/balance/tokens/xp compared; mismatch → account_state_mismatch.
    #[test]
    fn account_fields_match_and_tokens_xp_mismatch() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();

        let state = State::new();
        state.set_balance(&"PxA".into(), 90);
        state.set_nonce(&"PxA".into(), 1);
        state.set_asset_balance(&"PxA".into(), &Asset::xp(), 40);

        let exec = ExecutionTipView {
            height: 1,
            block_hash: "bh1".into(),
        };
        let ok = check_execution_vs_rocks(&state, &exec, &store, Some(&["PxA".into()])).unwrap();
        assert!(ok.is_consistent(), "{ok:?}");

        state.set_asset_balance(&"PxA".into(), &Asset::xp(), 99);
        let bad = check_execution_vs_rocks(&state, &exec, &store, Some(&["PxA".into()])).unwrap();
        assert!(!bad.is_consistent());
        let reason = bad.reason.as_ref().unwrap();
        assert!(reason.contains(REASON_ACCOUNT_STATE_MISMATCH), "{reason}");
        assert!(reason.contains("address=PxA"), "{reason}");
        assert!(reason.contains("field=xp") || reason.contains("field=tokens"), "{reason}");
        evict_cached(&db_path);
    }

    /// Issue #64: matching fixture → machine-readable consistent JSON shape.
    #[test]
    fn consistent_result_json_fixture() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();

        let state = State::new();
        state.set_balance(&"PxA".into(), 90);
        state.set_nonce(&"PxA".into(), 1);
        state.set_asset_balance(&"PxA".into(), &Asset::xp(), 40);

        let exec = ExecutionTipView {
            height: 1,
            block_hash: "bh1".into(),
        };
        let verdict =
            check_execution_vs_rocks(&state, &exec, &store, Some(&["PxA".into()])).unwrap();
        assert_eq!(verdict.status, STATUS_CONSISTENT);
        assert!(verdict.is_consistent());

        let raw = consistency_verdict_json(&verdict).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["status"], "consistent");
        assert_eq!(v["height"], 1);
        assert_eq!(v["rocks_head"], 1);
        assert_eq!(v["state_root"], "root1");
        assert!(v.get("reason").is_none() || v["reason"].is_null());
        evict_cached(&db_path);
    }

    /// Issue #65: forced mismatch → diverged + reason; no auto-repair (state unchanged).
    #[test]
    fn diverged_result_json_fixture_no_repair() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();

        let state = State::new();
        state.set_balance(&"PxA".into(), 90);
        state.set_nonce(&"PxA".into(), 1);
        state.set_asset_balance(&"PxA".into(), &Asset::xp(), 40);

        let tip_before = read_canonical_tip_accounts(&store);
        let bal_before = state.get_balance(&"PxA".into());
        let xp_before = state.get_asset_balance(&"PxA".into(), &Asset::xp());
        let nonce_before = state.get_nonce(&"PxA".into());

        // Forced height mismatch.
        let exec = ExecutionTipView {
            height: 9,
            block_hash: "bh1".into(),
        };
        let verdict =
            check_execution_vs_rocks(&state, &exec, &store, Some(&["PxA".into()])).unwrap();
        assert_eq!(verdict.status, STATUS_DIVERGED);
        assert!(!verdict.is_consistent());
        assert!(verdict.reason.is_some());

        let raw = consistency_verdict_json(&verdict).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["status"], "diverged");
        assert!(v["reason"].as_str().unwrap().contains(REASON_HEIGHT_MISMATCH));

        // No auto-repair: Rocks tip and execution state unchanged after call.
        let tip_after = read_canonical_tip_accounts(&store);
        assert_eq!(tip_before, tip_after, "rocks must be unchanged (no repair)");
        assert_eq!(state.get_balance(&"PxA".into()), bal_before);
        assert_eq!(state.get_asset_balance(&"PxA".into(), &Asset::xp()), xp_before);
        assert_eq!(state.get_nonce(&"PxA".into()), nonce_before);
        evict_cached(&db_path);
    }

    fn read_canonical_tip_accounts(
        store: &RocksStore,
    ) -> (u64, Option<String>, Vec<(String, String, u64, String)>) {
        use crate::storage::query::{get_block, get_head, list_accounts};
        let head = get_head(store).unwrap();
        let hash = get_block(store, head)
            .unwrap()
            .map(|b| b.block_hash);
        let accounts: Vec<_> = list_accounts(store)
            .unwrap()
            .into_iter()
            .map(|a| (a.address, a.balance, a.nonce, a.xp))
            .collect();
        (head, hash, accounts)
    }
}
