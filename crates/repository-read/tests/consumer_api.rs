//! The public names of `repository_read` that consuming projects' code calls, each in the shape it
//! calls it: a signature pinned by coercing the function to its pointer type. Nothing here runs; it
//! is compiled by the test build and by `just lint-rs` (clippy over every target), so a change that
//! would break a consumer fails a task's gate rather than a night later. What it calls that is not
//! named here the nightly's conciliation (`crates/symphony/src/nightly/conciliate.rs`) alone
//! catches. The other packages' guards are named in `crates/symphony/tests/consumer_api.rs`.
//!
//! A release's refresh moves a consumer's pin in a commit that touches only its `Cargo.toml` and
//! `Cargo.lock`, so a consumer's code never changes with a release: a name or signature here changes
//! only with a consumer release plan (the consumer's code moved first, against a release that still
//! has the old name). Names were read from the consuming projects' code on 2026-11-12; add one when
//! a consumer starts calling it.

use std::fs::{Metadata, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

use repository_read::{HEADER, REPORT_ENV, ROOT_ENV, Reader};

#[test]
fn the_names_consumers_call_keep_their_shapes() {
    let _: [&str; 3] = [HEADER, REPORT_ENV, ROOT_ENV];
    let _: fn(PathBuf, Option<PathBuf>) -> io::Result<Reader> = Reader::new;
    let _: fn(PathBuf) -> io::Result<Reader> = Reader::process;
    let _: fn(&Reader, &Path) -> io::Result<String> = Reader::read_to_string;
    let _: fn(&Reader, &Path) -> io::Result<ReadDir> = Reader::read_dir;
    let _: fn(&Reader, &Path) -> io::Result<Metadata> = Reader::metadata;
    let _: fn(&Reader, &Path, &Path) -> io::Result<Vec<(PathBuf, String)>> = Reader::json_files;
}
