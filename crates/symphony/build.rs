//! The `symphony` binary links on macOS with `-no_exported_symbols`: no other code loads it, so the
//! export trie a link writes by default, a name for each global symbol, is bytes nothing reads
//! (tests/links.rs; docs/CHECKS.md, "Size budgets"). A consuming project's own binaries link as
//! their own packages say.
#![forbid(unsafe_code)]

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols");
    }
}
