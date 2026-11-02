//! The wire fixtures crates/symphony-api pins, read for the client's tests.
#![allow(dead_code)] // each test binary uses its own share of these

use symphony_api::{Board, Delta};

pub fn fixture_path(name: &str) -> String {
    format!(
        "{}/../../crates/symphony-api/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub fn fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

pub fn board(name: &str) -> Board {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}

pub fn delta(name: &str) -> Delta {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}
