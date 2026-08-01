//! Process-global last successful DAG batch commit (for L2 apply ordering).

use crate::core::dag::commit::CommitOutcome;
use std::sync::{Mutex, OnceLock};

pub fn global_last_commit() -> &'static Mutex<Option<CommitOutcome>> {
    static LAST: OnceLock<Mutex<Option<CommitOutcome>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

pub fn set_last_commit(outcome: CommitOutcome) {
    if let Ok(mut g) = global_last_commit().lock() {
        *g = Some(outcome);
    }
}

pub fn get_last_commit() -> Option<CommitOutcome> {
    global_last_commit().lock().ok().and_then(|g| g.clone())
}

pub fn clear_last_commit() {
    if let Ok(mut g) = global_last_commit().lock() {
        *g = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear() {
        clear_last_commit();
        assert!(get_last_commit().is_none());
        set_last_commit(CommitOutcome {
            round: 1,
            anchor: "a".into(),
            digests: vec!["d1".into(), "d2".into()],
            vertex_order: vec!["g".into(), "a".into()],
        });
        let got = get_last_commit().unwrap();
        assert_eq!(got.digests, vec!["d1", "d2"]);
        clear_last_commit();
        assert!(get_last_commit().is_none());
    }
}
