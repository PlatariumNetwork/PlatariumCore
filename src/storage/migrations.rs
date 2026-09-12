//! SCHEMA_VERSION migration harness (issue #74).
//!
//! Real entrypoints: read/write schema marker on open, and stepwise
//! `migrate(from → to)` (including `vN-1 → vN`) — not a one-off hack.
//!
//! # Plugging future XP / assets / staking / account metadata (issue #77)
//!
//! Register new durable account fields through this harness — do not invent a
//! one-off open-path rewrite. Extension steps:
//!
//! 1. Bump [`SCHEMA_VERSION`] in [`crate::storage::schema`] (N → N+1).
//! 2. Add the field on [`AccountRecord`](crate::storage::commit::AccountRecord)
//!    with `#[serde(default)]` (and safe defaults) so old JSON still loads.
//! 3. Add a `migrate_one_step(N → N+1)` arm below: marker-only when serde
//!    defaults suffice, or a rewrite pass when values must be backfilled
//!    (XP maps, asset catalogs, staking balances, account metadata).
//! 4. Wire StateDiff / query / RPC helpers so new fields survive RMW and
//!    always surface (including empty) without panic.
//! 5. Add an old-schema fixture test: open vN DB → [`ensure_schema`] /
//!    [`migrate`] → current [`SCHEMA_VERSION`].
//!
//! Current marker: see [`SCHEMA_VERSION`]. v1→v2 covered Tokens·Xp; future
//! staking / extra XP / asset metadata fields follow the same steps.

use crate::error::{PlatariumError, Result};
use crate::storage::schema::{KEY_META_SCHEMA, SCHEMA_VERSION};
use rocksdb::DB;

/// Discoverable note that future XP/assets/staking fields register via
/// [`SCHEMA_VERSION`] + `migrate_one_step` (issue #77).
pub const MIGRATION_EXTENSION_DOC: &str = concat!(
    "bump SCHEMA_VERSION; serde defaults on AccountRecord; ",
    "migrate_one_step N→N+1; StateDiff/query/RPC surface; old-schema fixture"
);

/// Read on-disk schema version from `meta/schema`. `None` if unset (fresh DB).
pub fn read_schema_version(db: &DB) -> Result<Option<u32>> {
    match db.get(KEY_META_SCHEMA) {
        Ok(Some(bytes)) => {
            if bytes.len() < 4 {
                return Err(PlatariumError::State("invalid schema version bytes".into()));
            }
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&bytes[..4]);
            Ok(Some(u32::from_be_bytes(arr)))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(PlatariumError::State(format!("read schema: {}", e))),
    }
}

/// Persist schema version marker (`meta/schema`).
pub fn write_schema_version(db: &DB, version: u32) -> Result<()> {
    db.put(KEY_META_SCHEMA, version.to_be_bytes())
        .map_err(|e| PlatariumError::State(format!("write schema: {}", e)))
}

/// Ensure DB schema matches [`SCHEMA_VERSION`]: write on fresh open, migrate when older.
///
/// Called from [`RocksStore::open`](crate::storage::rocks::RocksStore::open).
pub fn ensure_schema(db: &DB) -> Result<()> {
    match read_schema_version(db)? {
        Some(ver) => {
            if ver > SCHEMA_VERSION {
                return Err(PlatariumError::State(format!(
                    "DB schema {} newer than binary {}",
                    ver, SCHEMA_VERSION
                )));
            }
            if ver < SCHEMA_VERSION {
                migrate(db, ver, SCHEMA_VERSION)?;
            }
            Ok(())
        }
        None => {
            write_schema_version(db, SCHEMA_VERSION)?;
            Ok(())
        }
    }
}

/// Migrate on-disk schema from `from` to `to` (inclusive steps `v → v+1`).
///
/// Public harness entrypoint for `migrate(vN-1 → vN)` and multi-step upgrades.
pub fn migrate(db: &DB, from: u32, to: u32) -> Result<()> {
    if from > to {
        return Err(PlatariumError::State(format!(
            "unsupported schema migration {} -> {} (from > to)",
            from, to
        )));
    }
    if from == to {
        write_schema_version(db, to)?;
        return Ok(());
    }
    let mut cur = from;
    while cur < to {
        let next = cur + 1;
        migrate_one_step(db, cur, next)?;
        write_schema_version(db, next)?;
        cur = next;
    }
    Ok(())
}

/// Single-step migration `from → from+1`. Extend this match when bumping SCHEMA_VERSION.
fn migrate_one_step(db: &DB, from: u32, to: u32) -> Result<()> {
    debug_assert_eq!(to, from + 1);
    match (from, to) {
        // Fresh / unset treated as 0 → 1: marker only.
        (0, 1) => Ok(()),
        // v1 → v2: AccountRecord gained `tokens`/`xp`. Stored JSON is left as-is; missing
        // fields deserialize with safe defaults. Only the schema marker is updated by caller.
        (1, 2) => {
            let _ = db; // no account rewrite pass
            Ok(())
        }
        _ => Err(PlatariumError::State(format!(
            "unsupported schema migration step {} -> {}",
            from, to
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::commit::AccountRecord;
    use crate::storage::query::get_account;
    use crate::storage::rocks::RocksStore;
    use crate::storage::schema::key_account;
    use rocksdb::Options;
    use tempfile::TempDir;

    #[test]
    fn migrate_v1_to_v2_bumps_meta_schema() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("db");
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, &path).unwrap();
        write_schema_version(&db, 1).unwrap();
        ensure_schema(&db).unwrap();
        assert_eq!(read_schema_version(&db).unwrap(), Some(SCHEMA_VERSION));
        assert_eq!(SCHEMA_VERSION, 2);
    }

    /// Issue #74: public migrate(vN-1 → vN) entrypoint + version read/write on open.
    #[test]
    fn migrate_entrypoint_step_and_open_writes_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("db");
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, &path).unwrap();
        assert_eq!(read_schema_version(&db).unwrap(), None);
        // Fresh open path via ensure_schema writes current version.
        ensure_schema(&db).unwrap();
        assert_eq!(read_schema_version(&db).unwrap(), Some(SCHEMA_VERSION));

        // Explicit vN-1 → vN harness (rewrite marker to 1, migrate to 2).
        write_schema_version(&db, 1).unwrap();
        migrate(&db, 1, SCHEMA_VERSION).unwrap();
        assert_eq!(read_schema_version(&db).unwrap(), Some(SCHEMA_VERSION));
    }

    /// Issue #75: old-schema fixture DB → migration on open → current schema; no panic.
    #[test]
    fn old_fixture_migrates_to_current_schema() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old_fixture_db");

        // Build a v1-shaped fixture without going through RocksStore::open (which
        // would immediately bump schema). Legacy account JSON omits tokens/xp.
        const LEGACY_ACCOUNT: &str = concat!(
            r#"{"address":"PxOldFixture","balance":"12345","uplp_balance":"9","nonce":3}"#
        );
        {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            let db = DB::open(&opts, &path).unwrap();
            write_schema_version(&db, 1).unwrap();
            db.put(key_account("PxOldFixture"), LEGACY_ACCOUNT.as_bytes())
                .unwrap();
            assert_eq!(read_schema_version(&db).unwrap(), Some(1));
        }

        // Reopen via RocksStore — ensure_schema must migrate without panic.
        let store = RocksStore::open(&path).expect("old fixture must open after migration");
        assert_eq!(
            read_schema_version(store.db()).unwrap(),
            Some(SCHEMA_VERSION),
            "migrated fixture must report current SCHEMA_VERSION"
        );

        let loaded = get_account(&store, "PxOldFixture")
            .expect("get_account must not fail")
            .expect("legacy account must still be present");
        assert_eq!(loaded.address, "PxOldFixture");
        assert_eq!(loaded.balance, "12345");
        assert_eq!(loaded.uplp_balance, "9");
        assert_eq!(loaded.nonce, 3);
        assert!(loaded.tokens.is_empty());
        assert_eq!(loaded.xp, "0");

        // Serde path also stays panic-free on the raw fixture bytes.
        let parsed: AccountRecord = serde_json::from_str(LEGACY_ACCOUNT).unwrap();
        assert_eq!(parsed.xp, "0");
    }

    /// Issue #77: extension doc lists steps and references SCHEMA_VERSION.
    #[test]
    fn migration_extension_doc_references_schema_version() {
        assert!(MIGRATION_EXTENSION_DOC.contains("SCHEMA_VERSION"));
        assert!(MIGRATION_EXTENSION_DOC.contains("migrate_one_step"));
        assert!(SCHEMA_VERSION >= 2);
        // Module rustdoc is the canonical short doc; constant mirrors the steps.
        assert!(MIGRATION_EXTENSION_DOC.contains("serde defaults"));
        assert!(MIGRATION_EXTENSION_DOC.contains("old-schema fixture"));
    }

    /// Issue #76: after migration, read → write → read preserves tokens/xp and
    /// prior balance/nonce (round-trip green).
    #[test]
    fn post_migration_read_write_read_preserves_tokens_xp() {
        use crate::core::asset::Asset;
        use crate::core::kernel::commit_engine::commit_state_diff;
        use crate::core::kernel::state_diff::{AccountPostImage, StateDiff, STATE_DIFF_SCHEMA_VERSION};
        use crate::storage::cache::evict_cached;
        use crate::storage::engine::RocksAccountStorageEngine;
        use std::collections::BTreeMap;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("post_mig_rwr_db");

        // v1 fixture with tokens/xp already present (serde-forward) + balance/nonce.
        let mut tokens = BTreeMap::new();
        tokens.insert("Token:USDT".to_string(), "77".to_string());
        tokens.insert(Asset::xp().as_canonical(), "250".to_string());
        let seeded = AccountRecord {
            address: "PxRwr".into(),
            balance: "5000".into(),
            uplp_balance: "12".into(),
            nonce: 7,
            tokens: tokens.clone(),
            xp: "250".into(),
        };
        {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            let db = DB::open(&opts, &path).unwrap();
            write_schema_version(&db, 1).unwrap();
            db.put(
                key_account("PxRwr"),
                serde_json::to_vec(&seeded).expect("serialize seeded account"),
            )
            .unwrap();
            assert_eq!(read_schema_version(&db).unwrap(), Some(1));
        }

        // Migrate on open, then READ — capture prior balance/nonce + tokens/xp.
        let (prior_balance, prior_uplp, prior_nonce, prior_tokens, prior_xp) = {
            let store = RocksStore::open(&path).expect("migrate on open");
            assert_eq!(
                read_schema_version(store.db()).unwrap(),
                Some(SCHEMA_VERSION)
            );
            let got = get_account(&store, "PxRwr")
                .expect("read")
                .expect("account present after migration");
            assert_eq!(got.balance, "5000");
            assert_eq!(got.uplp_balance, "12");
            assert_eq!(got.nonce, 7);
            assert_eq!(got.xp, "250");
            assert_eq!(
                got.tokens.get("Token:USDT").map(String::as_str),
                Some("77")
            );
            (
                got.balance.clone(),
                got.uplp_balance.clone(),
                got.nonce,
                got.tokens.clone(),
                got.xp.clone(),
            )
        };
        evict_cached(&path);

        // WRITE: balance-only post-image (empty token_balances) must RMW-preserve
        // durable tokens/xp while updating balance/nonce.
        {
            let mut eng = RocksAccountStorageEngine::open(&path).unwrap();
            let bal_only = StateDiff {
                schema_version: STATE_DIFF_SCHEMA_VERSION,
                batch_id: "post-mig-rwr".into(),
                receipts: vec![],
                accounts: vec![AccountPostImage {
                    address: "PxRwr".into(),
                    plp_balance: "4900".into(),
                    uplp_balance: "11".into(),
                    nonce: 8,
                    token_balances: BTreeMap::new(),
                }],
                pre_state_root: None,
                post_state_root: "rwr-root".into(),
                escrows_json: None,
            };
            let res = commit_state_diff(&mut eng, &bal_only).unwrap();
            assert!(res.ok, "{:?}", res.error);
        }
        evict_cached(&path);

        // READ again — tokens/xp intact; prior values were correct before write.
        let store = RocksStore::open(&path).unwrap();
        let got = get_account(&store, "PxRwr")
            .expect("second read")
            .expect("account still present");
        assert_eq!(prior_balance, "5000");
        assert_eq!(prior_uplp, "12");
        assert_eq!(prior_nonce, 7);
        assert_eq!(prior_xp, "250");
        assert_eq!(prior_tokens.get("Token:USDT").map(String::as_str), Some("77"));
        assert_eq!(got.balance, "4900");
        assert_eq!(got.uplp_balance, "11");
        assert_eq!(got.nonce, 8);
        assert_eq!(got.xp, "250", "xp must survive post-migration RWR");
        assert_eq!(
            got.tokens.get("Token:USDT").map(String::as_str),
            Some("77"),
            "tokens must survive post-migration RWR"
        );
        assert_eq!(
            got.tokens.get(&Asset::xp().as_canonical()).map(String::as_str),
            Some("250")
        );
        evict_cached(&path);
    }
}
