//! Every binary of the board links on macOS with `-no_exported_symbols`: no other code loads one,
//! so the export trie a link writes by default, a name for each global symbol, would serve only a
//! debugger reading a stripped build, which a build to debug replaces with one that keeps every
//! name (`CARGO_PROFILE_RELEASE_STRIP=debuginfo`; tests/links.rs; docs/workflow/CHECKS.md, "Size budgets").
//! A consuming project's own binaries link as their own packages say.
//!
//! It also gives the build the SHA-256 of its common and PostgreSQL schema sources as
//! `BOARD_SCHEMA_SHA256` and `BOARD_POSTGRES_SCHEMA_SHA256`, by which `live.rs` recognizes a
//! default branch's committed definitions without the build carrying the source.
#![forbid(unsafe_code)]

use sha2::{Digest as _, Sha256};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/schema.rs");
    println!("cargo:rerun-if-changed=src/store/postgres/schema.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols");
    }
    let package = std::env::var("CARGO_MANIFEST_DIR").expect("Cargo names the package's folder");
    let source = std::fs::read(Path::new(&package).join("src/schema.rs"))
        .expect("the board's schema source is in the package");
    let digest: String = (Sha256::digest(source).iter())
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=BOARD_SCHEMA_SHA256={digest}");
    let source = std::fs::read(Path::new(&package).join("src/store/postgres/schema.rs"))
        .expect("the board's PostgreSQL schema source is in the package");
    let digest: String = (Sha256::digest(source).iter())
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=BOARD_POSTGRES_SCHEMA_SHA256={digest}");
}
