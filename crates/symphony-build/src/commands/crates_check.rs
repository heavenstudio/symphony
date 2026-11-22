//! `symphony-build crates-check [--branch] [ROOT]` (`just crates-check`, a step of `just verify`):
//! each package the workspace at ROOT publishes, against crates.io (docs/workflow/RELEASES.md, "Packages on
//! crates.io").
//! A version crates.io lacks has nothing to compare with; one it holds is packaged here and
//! compared with that version's sources there, and one whose sources moved while its version did
//! not is red, since a version is published once (exit 1). It only reads crates.io. A registry it
//! cannot read, or a package it cannot make, is exit 2: it fails closed. A read of crates.io that
//! got no answer, or a server's error, is tried again first, three tries in all.
//!
//! `--branch` is a task's own, a step of `just lint-rs`: the same check of the published packages
//! the checkout changes since it left the default branch, and of no other, so a branch that
//! changes none reads nothing, of crates.io or of a package, and one that changes a published
//! package's sources without its version is red at its own tip, not at the next release run. A
//! crates.io that gives it no answer is said and no red (exit 0): only the whole check fails
//! closed on that.
#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::release::crates::{self, Branch, Checked, Registry};

const USAGE: &str = "usage: symphony-build crates-check [--branch] [ROOT]";
/// How long the check may take: a read of crates.io for each package, then the packaging and the
/// comparison of the ones it holds.
const BOUND: Duration = Duration::from_secs(30 * 60);
/// What a green check of every published package ends with, and one of a branch's.
const EVERY: &str = "every published package's version on crates.io holds its sources";
const BRANCH: &str = "each published package this branch changes holds the sources of its \
                      version on crates.io, or is at a version crates.io lacks";

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (branch, rest) = match args {
        [flag, rest @ ..] if flag == "--branch" => (true, rest),
        rest => (false, rest),
    };
    let root = match rest {
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
    if branch {
        let default = symphony::project::Project::load(root).map_or_else(
            |_| "main".to_string(),
            |project| project.default_branch.clone(),
        );
        let registry = Registry::of_env();
        checked_branch(
            root,
            &default,
            &registry,
            &target,
            crates::BRANCH_READS,
            out,
            err,
        )
    } else {
        checked(root, &Registry::of_env(), &target, out, err)
    }
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
    let found = crates::check(root, registry, target, Instant::now() + BOUND);
    said(found, EVERY, out, err)
}

/// The check of the published packages the checkout at `root` changes since it left `default`,
/// said: its exit code, 0 with one line where there is no `default` to compare with or the branch
/// changes no published package, and 0 with a line on `err` naming the packages it did not check
/// where the registry gave no answer within `reads`: that is the network's, never the branch's
/// red, and the whole check, which the release run makes, is what still fails closed on it.
fn checked_branch(
    root: &Path,
    default: &str,
    registry: &Registry,
    target: &Path,
    reads: Duration,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let until = Instant::now() + BOUND;
    match crates::check_branch(root, default, registry, target, reads, until) {
        Ok(Branch::NoBase) => {
            let _ = writeln!(
                out,
                "crates-check: this checkout has no {default} to compare with: nothing checked"
            );
            0
        }
        Ok(Branch::Checked(found)) if found == Checked::default() => {
            let _ = writeln!(
                out,
                "crates-check: this branch changes no published package: nothing checked"
            );
            0
        }
        Ok(Branch::Checked(found)) => said(Ok(found), BRANCH, out, err),
        Ok(Branch::Unread { packages, why }) => {
            let _ = writeln!(
                err,
                "crates-check: not checked: crates.io gave no answer ({why}), so nothing is known \
                 of {}, which this branch changes: `just crates-check` reads them, and the \
                 release run does",
                packages.join(", ")
            );
            0
        }
        Err(problem) => said(Err(problem), BRANCH, out, err),
    }
}

/// What a check `found`, said, ending a green one with `green`: its exit code.
fn said(
    found: Result<Checked, String>,
    green: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match found {
        Ok(checked) => {
            for line in &checked.lines {
                let _ = writeln!(out, "crates-check: {line}");
            }
            for changed in &checked.changed {
                let _ = writeln!(err, "crates-check: {}", crates::changed_line(changed));
            }
            if checked.changed.is_empty() {
                let _ = writeln!(out, "crates-check: {green}");
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
