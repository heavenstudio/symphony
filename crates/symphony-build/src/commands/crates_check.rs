//! `symphony-build crates-check [ROOT]` (`just crates-check`, a step of `just verify`): each
//! package the workspace at ROOT publishes, against crates.io (docs/RELEASES.md, "Packages on
//! crates.io").
//! A version crates.io lacks has nothing to compare with; one it holds is packaged here and
//! compared with that version's sources there, and one whose sources moved while its version did
//! not is red, since a version is published once (exit 1). It only reads crates.io. A registry it
//! cannot read, or a package it cannot make, is exit 2: it fails closed. A read of crates.io that
//! got no answer, or a server's error, is tried again first, three tries in all.
#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::release::crates::{self, Registry};

const USAGE: &str = "usage: symphony-build crates-check [ROOT]";
/// How long the check may take: a read of crates.io for each package, then the packaging and the
/// comparison of the ones it holds.
const BOUND: Duration = Duration::from_secs(30 * 60);

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let root = match args {
        [] => Path::new("."),
        [root] if !root.starts_with('-') => Path::new(root),
        _ => {
            let _ = writeln!(err, "{USAGE}");
            return 2;
        }
    };
    // The build folder its recipe's cargo uses, where the packages are made and unpacked.
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join(".target"), PathBuf::from);
    checked(root, &Registry::of_env(), &target, out, err)
}

/// The check of the workspace at `root` against `registry`, in the build folder `target`, said:
/// its exit code.
fn checked(
    root: &Path,
    registry: &Registry,
    target: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match crates::check(root, registry, target, Instant::now() + BOUND) {
        Ok(checked) => {
            for line in &checked.lines {
                let _ = writeln!(out, "crates-check: {line}");
            }
            for changed in &checked.changed {
                let _ = writeln!(err, "crates-check: {}", crates::changed_line(changed));
            }
            if checked.changed.is_empty() {
                let _ = writeln!(
                    out,
                    "crates-check: every published package's version on crates.io holds its \
                     sources"
                );
                0
            } else {
                1
            }
        }
        Err(problem) => {
            let _ = writeln!(err, "symphony-build crates-check: {problem}");
            2
        }
    }
}

#[cfg(test)]
mod tests;
