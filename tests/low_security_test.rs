//! Low (L1–L2) security/ops coverage.
//! L1: high-S reject lives in `signature` unit tests.
//! L2: daemon flock / socket ownership lives in Gateway `daemon_test.go`.

use platarium_core::signature::{sign_message, verify_signature};
use secp256k1::SecretKey;

#[test]
fn l1_low_s_roundtrip_accepted() {
    let sk = SecretKey::from_slice(&[7u8; 32]).unwrap();
    let msg = serde_json::json!({"low": true});
    let sig = sign_message(&sk, &msg).unwrap();
    assert!(verify_signature(&msg, &sig.signature_compact[..128], &sig.pub_key).unwrap());
}
