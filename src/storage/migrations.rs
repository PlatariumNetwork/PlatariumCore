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
    // v1 is the first schema — no intermediate migrations yet.
    if from == 0 || from == to {
        db.put(KEY_META_SCHEMA, to.to_be_bytes())
            .map_err(|e| PlatariumError::State(format!("write schema: {}", e)))?;
        return Ok(());
    }
    Err(PlatariumError::State(format!(
        "unsupported schema migration {} -> {}",
        from, to
    )))
}
