//! Process-wide RocksDB handle cache for the long-lived Core RPC daemon.
//! Opening the same path once avoids reopen cost and exclusive-lock conflicts.

use crate::error::{PlatariumError, Result};
use crate::storage::rocks::RocksStore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

static STORE_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<RocksStore>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<RocksStore>>> {
    STORE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn canonicalize_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Return a shared RocksStore for `path`, opening it on first use.
pub fn open_cached(path: impl AsRef<Path>) -> Result<Arc<RocksStore>> {
    let path = path.as_ref();
    let key = canonicalize_key(path);
    {
        let guard = cache()
            .lock()
            .map_err(|_| PlatariumError::State("rocks store cache poisoned".into()))?;
        if let Some(existing) = guard.get(&key) {
            return Ok(Arc::clone(existing));
        }
    }
    let store = Arc::new(RocksStore::open(path)?);
    let key = canonicalize_key(store.path());
    let mut guard = cache()
        .lock()
        .map_err(|_| PlatariumError::State("rocks store cache poisoned".into()))?;
    Ok(Arc::clone(guard.entry(key).or_insert_with(|| Arc::clone(&store))))
}

/// Drop a cached handle (tests / explicit reopen). Next open_cached recreates it.
pub fn evict_cached(path: impl AsRef<Path>) {
    let key = canonicalize_key(path.as_ref());
    if let Ok(mut guard) = cache().lock() {
        guard.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_cached_reuses_same_arc() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("db");
        let a = open_cached(&path).unwrap();
        let b = open_cached(&path).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
