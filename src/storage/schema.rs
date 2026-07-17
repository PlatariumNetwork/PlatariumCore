//! RocksDB key encoding (v1). Big-endian heights for lexicographic order.

pub const SCHEMA_VERSION: u32 = 1;

pub const PREFIX_ACCOUNT: &[u8] = b"a/";
pub const PREFIX_TX: &[u8] = b"t/";
pub const PREFIX_BLOCK: &[u8] = b"b/";
pub const PREFIX_RECEIPT: &[u8] = b"r/";
pub const PREFIX_STATE_ROOT: &[u8] = b"s/";
pub const PREFIX_IDX_ADDR: &[u8] = b"i/a/";
pub const PREFIX_IDX_BLOCK: &[u8] = b"i/b/";
pub const PREFIX_SNAPSHOT: &[u8] = b"snap/";
pub const KEY_META_HEAD: &[u8] = b"meta/head";
pub const KEY_META_SCHEMA: &[u8] = b"meta/schema";

pub fn key_account(address: &str) -> Vec<u8> {
    let mut k = PREFIX_ACCOUNT.to_vec();
    k.extend_from_slice(address.as_bytes());
    k
}

pub fn key_tx(tx_hash: &str) -> Vec<u8> {
    let mut k = PREFIX_TX.to_vec();
    k.extend_from_slice(tx_hash.as_bytes());
    k
}

pub fn key_block(height: u64) -> Vec<u8> {
    let mut k = PREFIX_BLOCK.to_vec();
    k.extend_from_slice(&height.to_be_bytes());
    k
}

pub fn key_receipt(tx_hash: &str) -> Vec<u8> {
    let mut k = PREFIX_RECEIPT.to_vec();
    k.extend_from_slice(tx_hash.as_bytes());
    k
}

pub fn key_state_root(height: u64) -> Vec<u8> {
    let mut k = PREFIX_STATE_ROOT.to_vec();
    k.extend_from_slice(&height.to_be_bytes());
    k
}

pub fn key_idx_addr(address: &str, height: u64, tx_hash: &str) -> Vec<u8> {
    let mut k = PREFIX_IDX_ADDR.to_vec();
    k.extend_from_slice(address.as_bytes());
    k.push(b'/');
    k.extend_from_slice(&height.to_be_bytes());
    k.push(b'/');
    k.extend_from_slice(tx_hash.as_bytes());
    k
}

pub fn key_idx_block(height: u64, idx: u32) -> Vec<u8> {
    let mut k = PREFIX_IDX_BLOCK.to_vec();
    k.extend_from_slice(&height.to_be_bytes());
    k.push(b'/');
    k.extend_from_slice(&idx.to_be_bytes());
    k
}

pub fn key_snapshot(height: u64) -> Vec<u8> {
    let mut k = PREFIX_SNAPSHOT.to_vec();
    k.extend_from_slice(&height.to_be_bytes());
    k
}

pub fn encode_u64(n: u64) -> [u8; 8] {
    n.to_be_bytes()
}

pub fn decode_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(bytes);
    Some(u64::from_be_bytes(arr))
}
