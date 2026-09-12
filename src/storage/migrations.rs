//! Schema version bumps for RocksDB.

use crate::error::{PlatariumError, Result};
use crate::storage::schema::{KEY_META_SCHEMA, SCHEMA_VERSION};
use rocksdb::DB;

pub fn ensure_schema(db: &DB) -> Result<()> {
    match db.get(KEY_META_SCHEMA) {
        Ok(Some(bytes)) => {
            if bytes.len() < 4 {
                return Err(PlatariumError::State("invalid schema version bytes".into()));
            }
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&bytes[..4]);
            let ver = u32::from_be_bytes(arr);
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
        Ok(None) => {
            db.put(KEY_META_SCHEMA, SCHEMA_VERSION.to_be_bytes())
                .map_err(|e| PlatariumError::State(format!("write schema: {}", e)))?;
            Ok(())
        }
        Err(e) => Err(PlatariumError::State(format!("read schema: {}", e))),
    }
}

fn migrate(db: &DB, from: u32, to: u32) -> Result<()> {
    // v1 → v2: AccountRecord gained `tokens`/`xp`. Stored JSON is left as-is; missing
    // fields deserialize with safe defaults. Only the schema marker is updated.
    if from == 0 || from == to || (from == 1 && to == 2) {
        db.put(KEY_META_SCHEMA, to.to_be_bytes())
            .map_err(|e| PlatariumError::State(format!("write schema: {}", e)))?;
        return Ok(());
    }
    Err(PlatariumError::State(format!(
        "unsupported schema migration {} -> {}",
        from, to
    )))
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
        db.put(KEY_META_SCHEMA, 1u32.to_be_bytes()).unwrap();
        ensure_schema(&db).unwrap();
        let bytes = db.get(KEY_META_SCHEMA).unwrap().unwrap();
        let mut arr = [0u8; 4];
        arr.copy_from_slice(&bytes[..4]);
        assert_eq!(u32::from_be_bytes(arr), SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 2);
    }
}
