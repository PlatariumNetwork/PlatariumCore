//! Medium (M1–M6) security/correctness coverage.

use platarium_core::core::core_rpc::{dispatch_rpc, handle_rpc_line};
use platarium_core::core::rpc_security::{dag_reset_allowed, MAX_RPC_LINE_BYTES};
use serde_json::json;
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
