//! `symphony-build size-budget ROOT RELEASE`: each shipped binary, as `cargo install` builds
//! it into RELEASE, and the client distribution the server embeds, against its line in the budget
//! file under ROOT ([`BUDGET`], docs/CHECKS.md "Size budgets"). Within every line it prints each
//! size beside its budget, and where the rule ([`line_for`]) brings down a binary's line that
//! stands above it (exit 0); past one it says the size, the budget and how to see what grew
//! (exit 1). A budget it cannot read, one with no line for a shipped binary ([`BUILDS`]) or a line
//! for none, a binary it cannot find, or a server built from another client than the one beside
//! it, is exit 2.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::SystemTime;

use serde::Deserialize;

use symphony::init::BUILDS;

/// The budget, under the checkout.
const BUDGET: &str = "tools/size-budget.toml";
/// The client distribution the server's build embeds, under the checkout.
const CLIENT: &str = "apps/symphony-server/client/public";
/// The binary whose build embeds that distribution (apps/symphony-server/build.rs): its line is its
/// file less the client's bytes, which have the client's line, so each line grows for one reason.
const EMBEDDER: &str = "symphony-server";
/// Where the shipped set is stated, as a refusal names it.
const SHIPPED: &str = "`BUILDS` in crates/symphony/src/init.rs";
const USAGE: &str = "usage: symphony-build size-budget ROOT RELEASE";
/// How far above a binary's size its line is set, in percent, and the bytes the line is rounded
/// down to (docs/CHECKS.md, "Where a line stands").
const MARGIN_PERCENT: u64 = 4;
const ROUNDED_TO: u64 = 10_000;

/// The budget file: the embedded client's line and each shipped binary's, in bytes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    /// The most the client distribution's files may add up to.
    client: u64,
    /// The most each shipped binary's file may weigh, by its name ([`EMBEDDER`]'s less the client).
    binaries: BTreeMap<String, u64>,
}

/// What one line of the budget measured.
#[derive(Debug, PartialEq, Eq)]
struct Measured {
    what: What,
    /// What the line counts.
    bytes: u64,
    budget: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum What {
    /// A binary, by its name, and its whole file when the line counts it less the client.
    Binary { name: String, whole: Option<u64> },
    /// The client distribution, with its file count.
    Client { files: usize },
}

/// The client distribution as the server's build embeds it: its bytes, its files, and when the
/// newest of them last changed (none for no file).
struct Distribution {
    bytes: u64,
    files: usize,
    newest: Option<SystemTime>,
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let [root, release] = args else {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    };
    match measure(Path::new(root), Path::new(release)) {
        Ok(lines) => report(&lines, out, err),
        Err(why) => {
            let _ = writeln!(err, "symphony-build size-budget: {why}");
            2
        }
    }
}

/// Every line of the budget under `root`, measured: the binaries in `release`, then the client.
fn measure(root: &Path, release: &Path) -> Result<Vec<Measured>, String> {
    let text = fs::read_to_string(root.join(BUDGET)).map_err(|e| format!("{BUDGET}: {e}"))?;
    let budget: Budget =
        toml::from_str(&text).map_err(|e| format!("{BUDGET}: {}", e.to_string().trim_end()))?;
    shipped(&budget)?;
    let client = folder(&root.join(CLIENT)).map_err(|e| format!("{CLIENT}: {e}"))?;
    let mut lines = Vec::new();
    for (name, most) in budget.binaries {
        let path = release.join(&name);
        let meta = fs::metadata(&path).map_err(|e| {
            format!(
                "{name}: {e} at {} (`just size-check` builds every binary first)",
                path.display()
            )
        })?;
        if !meta.is_file() {
            return Err(format!("{name}: {} is no file", path.display()));
        }
        let (bytes, whole) = if name == EMBEDDER {
            (embedder(&path, &meta, &client)?, Some(meta.len()))
        } else {
            (meta.len(), None)
        };
        lines.push(Measured {
            what: What::Binary { name, whole },
            bytes,
            budget: most,
        });
    }
    lines.push(Measured {
        what: What::Client {
            files: client.files,
        },
        bytes: client.bytes,
        budget: budget.client,
    });
    Ok(lines)
}

/// Refuses a budget that leaves a shipped binary out, or holds a line for a binary the workspace
/// does not deliver: the check is of every shipped binary, so a line gone or a binary added
/// without one is an error, never a pass.
fn shipped(budget: &Budget) -> Result<(), String> {
    let unshipped: Vec<&str> = (budget.binaries.keys())
        .map(String::as_str)
        .filter(|name| !BUILDS.contains(name))
        .collect();
    if !unshipped.is_empty() {
        return Err(format!(
            "{BUDGET}: [binaries] names {}, which this workspace does not deliver ({SHIPPED}): a \
             line is for a shipped binary",
            unshipped.join(", ")
        ));
    }
    let unbudgeted: Vec<&str> = (BUILDS.into_iter())
        .filter(|name| !budget.binaries.contains_key(*name))
        .collect();
    if !unbudgeted.is_empty() {
        return Err(format!(
            "{BUDGET}: [binaries] has no line for {}, which this workspace delivers ({SHIPPED}): \
             every shipped binary has a budget",
            unbudgeted.join(", ")
        ));
    }
    Ok(())
}

/// The embedder's own bytes: its file less the client it embeds. A server built before the client
/// beside it last changed, or one smaller than that client, embeds another client, and its own
/// bytes cannot be told: an error naming the rebuild, never a count of nothing.
fn embedder(path: &Path, meta: &fs::Metadata, client: &Distribution) -> Result<u64, String> {
    let rebuild = "rebuild it (`just size-check`)";
    let built = meta
        .modified()
        .map_err(|e| format!("{EMBEDDER}: {e} at {}", path.display()))?;
    if client.newest.is_some_and(|newest| newest > built) {
        return Err(format!(
            "{EMBEDDER}: {} was built before {CLIENT} last changed, so it embeds another client; \
             {rebuild}",
            path.display()
        ));
    }
    meta.len().checked_sub(client.bytes).ok_or_else(|| {
        format!(
            "{EMBEDDER}: {} is {} bytes, less than the {} of {CLIENT} it embeds, so it embeds \
             another client; {rebuild}",
            path.display(),
            grouped(meta.len()),
            grouped(client.bytes)
        )
    })
}

/// The regular files under `folder`, as the server's build script embeds them; no folder is none.
/// A link or any other kind of entry is an error, as it is there.
fn folder(folder: &Path) -> io::Result<Distribution> {
    let mut found = Distribution {
        bytes: 0,
        files: 0,
        newest: None,
    };
    match fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(io::Error::other("the client distribution is no folder")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(error),
    }
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                found.bytes += meta.len();
                found.files += 1;
                let changed = meta.modified()?;
                found.newest = Some(found.newest.map_or(changed, |newest| newest.max(changed)));
            } else {
                return Err(io::Error::other(format!(
                    "{}: client assets must be regular files",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(found)
}

/// Where the rule sets a binary's line at `bytes`: its size plus [`MARGIN_PERCENT`], rounded down
/// to [`ROUNDED_TO`], and never below the size itself.
fn line_for(bytes: u64) -> u64 {
    let raised = u128::from(bytes) * u128::from(100 + MARGIN_PERCENT) / 100;
    let rounded = raised / u128::from(ROUNDED_TO) * u128::from(ROUNDED_TO);
    u64::try_from(rounded).unwrap_or(u64::MAX).max(bytes)
}

/// Each line within its budget on `out`, and where the rule would bring down a binary's line that
/// stands above it; each one past it on `err` with how to see what grew. Exit 1 when one is past.
fn report(lines: &[Measured], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let width = lines.iter().map(|line| name(line).len()).max().unwrap_or(0);
    let mut past = 0;
    for line in lines {
        if line.bytes <= line.budget {
            let lower = match line.what {
                What::Binary { .. } if line_for(line.bytes) < line.budget => format!(
                    "; its line comes down to {} ({} bytes plus {MARGIN_PERCENT} %)",
                    grouped(line_for(line.bytes)),
                    grouped(line.bytes)
                ),
                _ => String::new(),
            };
            let _ = writeln!(
                out,
                "size-budget: {:<width$}  {} of {}{}{lower}",
                name(line),
                mib(line.bytes),
                mib(line.budget),
                match line.what {
                    What::Binary { whole: None, .. } => String::new(),
                    What::Binary {
                        whole: Some(whole), ..
                    } => format!(" ({} with the embedded client)", mib(whole)),
                    What::Client { files } => format!(" ({files} files)"),
                }
            );
            continue;
        }
        past += 1;
        let _ = writeln!(err, "{}", refusal(line));
    }
    if past > 0 {
        return 1;
    }
    let _ = writeln!(
        out,
        "size-budget: every shipped binary and the embedded client within {BUDGET}"
    );
    0
}

/// What a line's report calls it.
fn name(line: &Measured) -> &str {
    match &line.what {
        What::Binary { name, .. } => name,
        What::Client { .. } => "embedded client",
    }
}

/// Why `line` is past its budget: the size, the budget and how to see what grew.
fn refusal(line: &Measured) -> String {
    let over = format!(
        "is {} ({} bytes), {} past its budget of {} ({} bytes) in {BUDGET}",
        mib(line.bytes),
        grouped(line.bytes),
        mib(line.bytes - line.budget),
        mib(line.budget),
        grouped(line.budget)
    );
    match &line.what {
        What::Binary { name, whole } => format!(
            "size-budget: {name}{} {over}. See what grew: `cargo bloat --release --crates --bin \
             {name}` (`cargo install cargo-bloat --root DIR`) beside the same at a commit within \
             it: the last release (`git tag -l 'release-*'`), or before the first one, the commit \
             that last set the budget (`git log -1 -- {BUDGET}`). A reduction takes the bytes \
             back first; a line is raised only in a commit of its own that names the growth and \
             why no reduction will do, to the size plus 4 % and never higher, on a fresh \
             reviewer's approve (docs/CHECKS.md, \"A red at the size check\").",
            match whole {
                Some(whole) => format!(
                    ", less its embedded client (its file is {} bytes),",
                    grouped(*whole)
                ),
                None => String::new(),
            }
        ),
        What::Client { files } => format!(
            "size-budget: the embedded client ({CLIENT}, {files} files) {over}. See what it \
             holds: `ls -lS {CLIENT}/assets`; every generation `symphony-build publish-client` keeps \
             there is embedded in the server."
        ),
    }
}

/// `bytes` in MiB, to a tenth.
fn mib(bytes: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + (1 << 19)) >> 20;
    format!("{}.{} MiB", tenths / 10, tenths % 10)
}

/// `bytes` with its thousands apart: `31,234,567`.
fn grouped(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut text = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests;
