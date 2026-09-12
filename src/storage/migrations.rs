//! SCHEMA_VERSION migration harness (issue #74).
//!
//! Real entrypoints: read/write schema marker on open, and stepwise
//! `migrate(from → to)` (including `vN-1 → vN`) — not a one-off hack.

use crate::error::{PlatariumError, Result};
use crate::storage::schema::{KEY_META_SCHEMA, SCHEMA_VERSION};
use rocksdb::DB;

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
}
