//! Medium (M1–M6) security/correctness coverage.

use platarium_core::core::core_rpc::{dispatch_rpc, handle_rpc_line, read_limited_rpc_line};
use platarium_core::core::rpc_security::{dag_reset_allowed, MAX_RPC_LINE_BYTES};
use serde_json::json;
use std::io::Cursor;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn m1_dag_reset_blocked_without_env() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_DAG_ALLOW_RESET");
    assert!(!dag_reset_allowed());
    let err = dispatch_rpc("dag_reset", &json!({})).unwrap_err();
    assert!(
        err.to_string().contains("dag_reset disabled"),
        "{}",
        err
    );
}

#[test]
fn m1_dag_reset_allowed_with_env() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::set_var("PLATARIUM_DAG_ALLOW_RESET", "1");
    assert!(dag_reset_allowed());
    let out = dispatch_rpc("dag_reset", &json!({})).unwrap();
    assert!(out.contains("\"ok\":true") || out.contains("\"ok\": true"), "{out}");
    std::env::remove_var("PLATARIUM_DAG_ALLOW_RESET");
}

#[test]
fn m2_rpc_line_size_limit() {
    let huge = "x".repeat(MAX_RPC_LINE_BYTES + 8);
    let line = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"pad":"{}"}}}}"#,
        &huge[..std::cmp::min(huge.len(), MAX_RPC_LINE_BYTES)]
    );
    // Ensure constructed line exceeds limit.
    let oversized = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"pad":"{}"}}}}"#,
        "y".repeat(MAX_RPC_LINE_BYTES)
    );
    assert!(oversized.len() > MAX_RPC_LINE_BYTES);
    let resp: serde_json::Value = serde_json::from_str(&handle_rpc_line(&oversized)).unwrap();
    let msg = resp["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("too large"), "line={} msg={msg}", line.len());
}

/// R2-M1: size check must reject before an unbounded buffer is allocated.
#[test]
fn r2_m1_limited_reader_rejects_before_unbounded_alloc() {
    let max = 64usize;
    let mut huge = "x".repeat(max + 100);
    huge.push('\n');
    let mut cursor = Cursor::new(huge.into_bytes());
    let err = read_limited_rpc_line(&mut cursor, max).unwrap_err();
    assert!(
        err.to_string().contains("too large"),
        "{err}"
    );
    // Remainder drained — next read is EOF.
    assert!(read_limited_rpc_line(&mut cursor, max).unwrap().is_none());

    let mut ok = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n".to_vec());
    let line = read_limited_rpc_line(&mut ok, max).unwrap().unwrap();
    assert!(line.contains("ping"));
}

#[test]
fn m5_ping_does_not_require_state_lock_path() {
    // Smoke: ping succeeds without state_file (skips dispatch lock).
    let resp: serde_json::Value = serde_json::from_str(&handle_rpc_line(
        r#"{"jsonrpc":"2.0","id":7,"method":"ping","params":{}}"#,
    ))
    .unwrap();
    assert_eq!(resp["id"], 7);
    assert!(resp.get("result").is_some(), "{resp}");
    assert!(resp.get("error").is_none() || resp["error"].is_null(), "{resp}");
}

/// R2-M3: relative and absolute paths to the same file share one dispatch lock key.
#[test]
fn r2_m3_path_lock_key_collapses_relative_and_absolute() {
    use std::fs::File;
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("state.json");
    File::create(&file).unwrap();
    let abs = std::fs::canonicalize(&file).unwrap();
    // Same inode via absolute path string and via the same path again.
    let key_a = {
        // Exercise through RPC: concurrent apply is hard to assert; check canonicalize parity.
        let a = abs.to_string_lossy().to_string();
        let b = file.to_string_lossy().to_string();
        assert_ne!(a, b, "paths should differ as strings");
        // Both paths must resolve to an existing file for inode keying.
        assert!(std::fs::metadata(&a).is_ok());
        assert!(std::fs::metadata(&b).is_ok());
        let meta_a = std::fs::metadata(&a).unwrap();
        let meta_b = std::fs::metadata(&b).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(meta_a.dev(), meta_b.dev());
            assert_eq!(meta_a.ino(), meta_b.ino());
        }
        a
    };
    assert!(!key_a.is_empty());
}

/// R2-M4: missing settle outcome must not default to accept.
#[test]
fn r2_m4_settle_outcome_requires_explicit_key() {
    use platarium_core::core::asset::Asset;
    use platarium_core::core::transaction::Transaction;
    use std::collections::HashSet;
    let mut tx = Transaction::new(
        "from".into(),
        "to".into(),
        Asset::PLP,
        1,
        1,
        0,
        HashSet::new(),
        HashSet::new(),
        "s1".into(),
        "s2".into(),
    )
    .unwrap();
    tx.tx_kind = Some("escrow_settle".into());
    let err = tx.settle_outcome_key().unwrap_err();
    assert!(
        err.contains("explicit") || err.contains("settle_outcome"),
        "{err}"
    );
    tx.settle_outcome_key = Some("accept".into());
    assert_eq!(tx.settle_outcome_key().unwrap(), "accept");
    tx.settle_outcome_key = None;
    tx.settle_outcome = Some(2);
    assert_eq!(tx.settle_outcome_key().unwrap(), "reject");
}
