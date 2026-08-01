//! Canonical StateDiff encoding (deterministic JSON).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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
        };
        let j1 = diff.to_canonical_json().unwrap();
        let j2 = diff.to_canonical_json().unwrap();
        assert_eq!(j1, j2);
        assert!(j1.find("\"address\":\"a\"").unwrap() < j1.find("\"address\":\"b\"").unwrap());
        let round = StateDiff::from_canonical_json(&j1).unwrap();
        assert_eq!(diff.content_fingerprint(), round.content_fingerprint());
    }
}
