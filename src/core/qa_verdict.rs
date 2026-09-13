//! Orchestrator QA artifact contract (issues #97 / #115).
//!
//! QA workers must leave `qa-verdict.json` in the task worktree for the
//! orchestrator to consume. A QA run that exits ok without that file is marked
//! `qa_failed` ("worker did not write qa-verdict.json"). This module is the
//! PlatariumCore path that reliably produces the artifact — prefer writing a
//! `failed` verdict over silent absence.

use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Worktree-relative filename expected by the orchestrator.
pub const QA_VERDICT_FILENAME: &str = "qa-verdict.json";

pub const STATUS_PASSED: &str = "passed";
pub const STATUS_FAILED: &str = "failed";

/// Machine-readable QA verdict consumed by the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QaVerdict {
    pub status: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub reproduction: String,
    #[serde(default)]
    pub expected: String,
    #[serde(default)]
    pub actual: String,
}

impl QaVerdict {
    pub fn passed(
        reproduction: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            status: STATUS_PASSED.into(),
            problem: String::new(),
            reproduction: reproduction.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    pub fn failed(
        problem: impl Into<String>,
        reproduction: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            status: STATUS_FAILED.into(),
            problem: problem.into(),
            reproduction: reproduction.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Fallback when a QA worker exits without writing the artifact (#97 / #115).
    pub fn missing_worker_artifact() -> Self {
        Self::failed(
            "worker did not write qa-verdict.json",
            "QA worker exited without qa-verdict.json in the worktree",
            "Worktree contains qa-verdict.json with status passed|failed",
            "qa-verdict.json was missing; Core wrote this failed verdict instead",
        )
    }

    pub fn is_passed(&self) -> bool {
        self.status == STATUS_PASSED
    }

    pub fn is_failed(&self) -> bool {
        self.status == STATUS_FAILED || self.status.is_empty()
    }
}

/// Absolute path to `qa-verdict.json` under `worktree`.
pub fn qa_verdict_path(worktree: impl AsRef<Path>) -> PathBuf {
    worktree.as_ref().join(QA_VERDICT_FILENAME)
}

/// Encode a verdict as JSON (pretty for agent readability).
pub fn qa_verdict_json(verdict: &QaVerdict) -> Result<String> {
    serde_json::to_string_pretty(verdict).map_err(|e| {
        PlatariumError::State(format!("encode qa-verdict.json: {e}"))
    })
}

/// Atomically write `qa-verdict.json` into `worktree`. Returns the file path.
pub fn write_qa_verdict(worktree: impl AsRef<Path>, verdict: &QaVerdict) -> Result<PathBuf> {
    let path = qa_verdict_path(worktree);
    let json = qa_verdict_json(verdict)?;
    atomic_write(&path, json.as_bytes())?;
    Ok(path)
}

/// Read and parse an existing `qa-verdict.json`.
pub fn read_qa_verdict(worktree: impl AsRef<Path>) -> Result<QaVerdict> {
    let path = qa_verdict_path(worktree);
    let raw = fs::read_to_string(&path).map_err(|e| {
        PlatariumError::State(format!("read {}: {e}", path.display()))
    })?;
    let mut v: QaVerdict = serde_json::from_str(&raw).map_err(|e| {
        PlatariumError::State(format!("parse {}: {e}", path.display()))
    })?;
    if v.status.is_empty() {
        v.status = STATUS_FAILED.into();
    }
    Ok(v)
}

/// Ensure `qa-verdict.json` exists in `worktree`.
///
/// If the file is already present and parseable, it is left unchanged.
/// If missing (or unreadable), a failed verdict is written so a QA-028-class
/// run cannot exit ok yet leave the orchestrator without an artifact.
pub fn ensure_qa_verdict(worktree: impl AsRef<Path>) -> Result<PathBuf> {
    let worktree = worktree.as_ref();
    let path = qa_verdict_path(worktree);
    match read_qa_verdict(worktree) {
        Ok(_) => Ok(path),
        Err(_) => write_qa_verdict(worktree, &QaVerdict::missing_worker_artifact()),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                PlatariumError::State(format!("create dir {}: {e}", parent.display()))
            })?;
        }
    }
    let tmp = {
        let mut t = path.to_path_buf();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(QA_VERDICT_FILENAME);
        t.set_file_name(format!(".{name}.tmp"));
        t
    };
    fs::write(&tmp, bytes).map_err(|e| {
        PlatariumError::State(format!("write temp {}: {e}", tmp.display()))
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        PlatariumError::State(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_qa_verdict_creates_worktree_artifact() {
        let dir = tempdir().unwrap();
        let path = write_qa_verdict(
            dir.path(),
            &QaVerdict::passed(
                "cargo test --lib qa_verdict",
                "qa-verdict.json present",
                "written by write_qa_verdict",
            ),
        )
        .unwrap();
        assert_eq!(path, dir.path().join(QA_VERDICT_FILENAME));
        assert!(path.is_file());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("qa-verdict") || path.ends_with("qa-verdict.json"));
        let v = read_qa_verdict(dir.path()).unwrap();
        assert!(v.is_passed());
        assert_eq!(v.status, STATUS_PASSED);
    }

    #[test]
    fn ensure_qa_verdict_writes_failed_when_missing() {
        let dir = tempdir().unwrap();
        assert!(!qa_verdict_path(dir.path()).exists());
        let path = ensure_qa_verdict(dir.path()).unwrap();
        assert!(path.is_file());
        let v = read_qa_verdict(dir.path()).unwrap();
        assert!(v.is_failed());
        assert!(v.problem.contains("qa-verdict.json"));
    }

    #[test]
    fn ensure_qa_verdict_preserves_existing_passed() {
        let dir = tempdir().unwrap();
        write_qa_verdict(
            dir.path(),
            &QaVerdict::passed("repro", "expected", "actual"),
        )
        .unwrap();
        ensure_qa_verdict(dir.path()).unwrap();
        let v = read_qa_verdict(dir.path()).unwrap();
        assert!(v.is_passed());
        assert_eq!(v.actual, "actual");
    }

    #[test]
    fn qa_verdict_json_roundtrip_schema() {
        let v = QaVerdict::failed("p", "r", "e", "a");
        let raw = qa_verdict_json(&v).unwrap();
        let back: QaVerdict = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, v);
        assert!(raw.contains("\"status\""));
        assert!(raw.contains("\"problem\""));
        assert!(raw.contains("\"reproduction\""));
        assert!(raw.contains("\"expected\""));
        assert!(raw.contains("\"actual\""));
    }
}
