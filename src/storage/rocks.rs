//! RocksDB open/close helpers.

use crate::error::{PlatariumError, Result};
use crate::storage::migrations::ensure_schema;
use crate::storage::schema::{KEY_META_HEAD, decode_u64};
use rocksdb::{Options, DB, WriteBatch};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Thin wrapper around RocksDB for Platarium chain storage.
pub struct RocksStore {
    db: Arc<DB>,
    path: PathBuf,
}

impl RocksStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PlatariumError::State(format!("create rocksdb dir: {}", e)))?;
        }
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let db = DB::open(&opts, &path)
            .map_err(|e| PlatariumError::State(format!("open rocksdb: {}", e)))?;
        ensure_schema(&db)?;
        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn db(&self) -> &DB {
        &self.db
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db
            .get(key)
            .map_err(|e| PlatariumError::State(format!("rocksdb get: {}", e)))
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db
            .put(key, value)
            .map_err(|e| PlatariumError::State(format!("rocksdb put: {}", e)))
    }

    pub fn write_batch(&self, batch: WriteBatch) -> Result<()> {
        self.db
            .write(batch)
            .map_err(|e| PlatariumError::State(format!("rocksdb write_batch: {}", e)))
    }

    pub fn head_height(&self) -> Result<u64> {
        match self.get(KEY_META_HEAD)? {
            Some(bytes) => Ok(decode_u64(&bytes).unwrap_or(0)),
            None => Ok(0),
        }
    }

    /// Drop the DB handle and reopen (for durability/restart tests).
    pub fn reopen(self) -> Result<Self> {
        let path = self.path.clone();
        drop(self);
        Self::open(path)
    }
}

/// Open store at `PLATARIUM_ROCKSDB_PATH` or `{data_dir}/rocksdb`.
pub fn open_store(data_dir: Option<&Path>) -> Result<RocksStore> {
    if let Ok(p) = std::env::var("PLATARIUM_ROCKSDB_PATH") {
        if !p.trim().is_empty() {
            return RocksStore::open(p);
        }
    }
    let dir = data_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("data"));
    RocksStore::open(dir.join("rocksdb"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_put_get_reopen() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        store.put(b"k", b"v").unwrap();
        assert_eq!(store.get(b"k").unwrap().unwrap(), b"v");
        let store = store.reopen().unwrap();
        assert_eq!(store.get(b"k").unwrap().unwrap(), b"v");
    }
}
