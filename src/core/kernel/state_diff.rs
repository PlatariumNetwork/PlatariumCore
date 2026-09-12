//! Canonical StateDiff encoding (deterministic JSON).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const STATE_DIFF_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxReceipt {
    pub tx_hash: String,
    pub index: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub fee_uplp: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountPostImage {
    pub address: String,
    /// Absolute PLP balance (minimal units) as decimal string.
    pub plp_balance: String,
    pub uplp_balance: String,
    pub nonce: u64,
    /// Touched token symbol → absolute balance string. Sorted keys in JSON via BTreeMap.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub token_balances: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateDiff {
    pub schema_version: u32,
    pub batch_id: String,
    pub receipts: Vec<TxReceipt>,
    pub accounts: Vec<AccountPostImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_state_root: Option<String>,
    pub post_state_root: String,
    /// Full escrow records after batch execution (sorted JSON). None = do not touch escrows on commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrows_json: Option<Vec<String>>,
}

impl StateDiff {
    /// Canonical JSON: accounts sorted by address (caller should already sort).
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if let Some(obj) = value.as_object_mut() {
            if let Some(accounts) = obj.get_mut("accounts") {
                if let Some(arr) = accounts.as_array_mut() {
                    arr.sort_by(|a, b| {
                        let aa = a.get("address").and_then(|v| v.as_str()).unwrap_or("");
                        let bb = b.get("address").and_then(|v| v.as_str()).unwrap_or("");
                        aa.cmp(bb)
                    });
                }
            }
        }
        serde_json::to_string(&value)
    }

    pub fn from_canonical_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Stable content hash helper for tests (SHA-256 of canonical JSON).
    pub fn content_fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let json = self.to_canonical_json().unwrap_or_default();
        hex::encode(Sha256::digest(json.as_bytes()))
    }
}

/// Actionable diagnostics when two StateDiffs disagree (issue #69).
///
/// Returns `Ok(())` when equal. On mismatch, `Err` message includes the
/// diverged address and field (not only an opaque bool).
pub fn diagnose_state_diff_mismatch(expected: &StateDiff, actual: &StateDiff) -> Result<(), String> {
    if expected == actual {
        return Ok(());
    }
    let mut msg = String::from("StateDiff mismatch:");
    if expected.schema_version != actual.schema_version {
        let _ = write!(
            msg,
            " field=schema_version expected={} actual={}",
            expected.schema_version, actual.schema_version
        );
        return Err(msg);
    }
    if expected.batch_id != actual.batch_id {
        let _ = write!(
            msg,
            " field=batch_id expected={} actual={}",
            expected.batch_id, actual.batch_id
        );
        return Err(msg);
    }
    if expected.post_state_root != actual.post_state_root {
        let _ = write!(
            msg,
            " field=post_state_root expected={} actual={}",
            expected.post_state_root, actual.post_state_root
        );
        return Err(msg);
    }
    if expected.pre_state_root != actual.pre_state_root {
        let _ = write!(
            msg,
            " field=pre_state_root expected={:?} actual={:?}",
            expected.pre_state_root, actual.pre_state_root
        );
        return Err(msg);
    }
    if expected.receipts != actual.receipts {
        let _ = write!(msg, " field=receipts (count expected={} actual={})", expected.receipts.len(), actual.receipts.len());
        return Err(msg);
    }
    if expected.escrows_json != actual.escrows_json {
        let _ = write!(msg, " field=escrows_json");
        return Err(msg);
    }

    let mut exp_map: BTreeMap<&str, &AccountPostImage> = BTreeMap::new();
    for a in &expected.accounts {
        exp_map.insert(a.address.as_str(), a);
    }
    let mut act_map: BTreeMap<&str, &AccountPostImage> = BTreeMap::new();
    for a in &actual.accounts {
        act_map.insert(a.address.as_str(), a);
    }

    let mut addrs: BTreeMap<&str, ()> = BTreeMap::new();
    for k in exp_map.keys().chain(act_map.keys()) {
        addrs.insert(k, ());
    }
    for addr in addrs.keys() {
        match (exp_map.get(addr), act_map.get(addr)) {
            (None, Some(_)) => {
                return Err(format!(
                    "StateDiff mismatch: address={addr} field=presence expected=missing actual=present"
                ));
            }
            (Some(_), None) => {
                return Err(format!(
                    "StateDiff mismatch: address={addr} field=presence expected=present actual=missing"
                ));
            }
            (Some(e), Some(a)) => {
                if e.plp_balance != a.plp_balance {
                    return Err(format!(
                        "StateDiff mismatch: address={addr} field=plp_balance expected={} actual={}",
                        e.plp_balance, a.plp_balance
                    ));
                }
                if e.uplp_balance != a.uplp_balance {
                    return Err(format!(
                        "StateDiff mismatch: address={addr} field=uplp_balance expected={} actual={}",
                        e.uplp_balance, a.uplp_balance
                    ));
                }
                if e.nonce != a.nonce {
                    return Err(format!(
                        "StateDiff mismatch: address={addr} field=nonce expected={} actual={}",
                        e.nonce, a.nonce
                    ));
                }
                if e.token_balances != a.token_balances {
                    return Err(format!(
                        "StateDiff mismatch: address={addr} field=token_balances expected={:?} actual={:?}",
                        e.token_balances, a.token_balances
                    ));
                }
            }
            (None, None) => {}
        }
    }
    Err(format!(
        "StateDiff mismatch: address=<unknown> field=<opaque> fingerprints expected={} actual={}",
        expected.content_fingerprint(),
        actual.content_fingerprint()
    ))
}

/// Pretty-stable Value for golden comparisons (sort account arrays).
pub fn normalize_diff_value(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        if let Some(accounts) = obj.get_mut("accounts") {
            if let Some(arr) = accounts.as_array_mut() {
                arr.sort_by(|a, b| {
                    let aa = a.get("address").and_then(|x| x.as_str()).unwrap_or("");
                    let bb = b.get("address").and_then(|x| x.as_str()).unwrap_or("");
                    aa.cmp(bb)
                });
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_encoding_stable() {
        let diff = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "b1".into(),
            receipts: vec![TxReceipt {
                tx_hash: "h1".into(),
                index: 0,
                status: "ok".into(),
                error: None,
                fee_uplp: 1,
            }],
            accounts: vec![
                AccountPostImage {
                    address: "b".into(),
                    plp_balance: "1".into(),
                    uplp_balance: "0".into(),
                    nonce: 0,
                    token_balances: BTreeMap::new(),
                },
                AccountPostImage {
                    address: "a".into(),
                    plp_balance: "2".into(),
                    uplp_balance: "3".into(),
                    nonce: 1,
                    token_balances: BTreeMap::new(),
                },
            ],
            pre_state_root: Some("pre".into()),
            post_state_root: "post".into(),
            escrows_json: None,
        };
        let j1 = diff.to_canonical_json().unwrap();
        let j2 = diff.to_canonical_json().unwrap();
        assert_eq!(j1, j2);
        assert!(j1.find("\"address\":\"a\"").unwrap() < j1.find("\"address\":\"b\"").unwrap());
        let round = StateDiff::from_canonical_json(&j1).unwrap();
        assert_eq!(diff.content_fingerprint(), round.content_fingerprint());
    }

    /// Issue #69: mismatch diagnostics include address and field.
    #[test]
    fn diagnose_reports_address_and_field() {
        let mut a = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "b1".into(),
            receipts: vec![],
            accounts: vec![AccountPostImage {
                address: "PxAlice".into(),
                plp_balance: "100".into(),
                uplp_balance: "0".into(),
                nonce: 1,
                token_balances: BTreeMap::new(),
            }],
            pre_state_root: None,
            post_state_root: "post".into(),
            escrows_json: None,
        };
        let mut b = a.clone();
        b.accounts[0].nonce = 2;
        let err = diagnose_state_diff_mismatch(&a, &b).unwrap_err();
        assert!(err.contains("address=PxAlice"), "{err}");
        assert!(err.contains("field=nonce"), "{err}");
        assert!(diagnose_state_diff_mismatch(&a, &a).is_ok());

        a.accounts[0].plp_balance = "50".into();
        let err2 = diagnose_state_diff_mismatch(&a, &b).unwrap_err();
        assert!(err2.contains("address=PxAlice"), "{err2}");
        assert!(err2.contains("field=plp_balance"), "{err2}");
    }
}
