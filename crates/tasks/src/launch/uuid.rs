//! The existing launch session UUID generator, kept independent from launch orchestration.

use std::hash::{BuildHasher, RandomState};
use std::io::Read;

/// A random (version 4) UUID: the id a new Claude session is started with.
pub fn new_uuid() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        let state = RandomState::new();
        let (a, b) = (
            state.hash_one(std::process::id()),
            state.hash_one(std::time::SystemTime::now()),
        );
        bytes[..8].copy_from_slice(&a.to_le_bytes());
        bytes[8..].copy_from_slice(&b.to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
