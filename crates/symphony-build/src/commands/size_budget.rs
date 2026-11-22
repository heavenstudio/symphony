//! `symphony-build size-budget ROOT RELEASE`: each shipped binary, as `cargo install` builds
//! it into RELEASE, and the builds of the client distribution the server embeds, each counted
//! whole ([`Distribution::kept`]), against its line in the budget file under ROOT ([`BUDGET`],
//! docs/workflow/CHECKS.md "Size budgets"). Within every line it prints each size beside its budget, and
//! where the rule ([`line_for`]) brings down a binary's line that stands above it (exit 0); past
//! one it says the size, the budget and how to see what grew (exit 1). A budget it cannot read, one
//! with no line for a shipped binary ([`BUILDS`]) or a line for none, a binary it cannot find, a
//! server built from another client than the one beside it, or a client file no kept build names,
//! is exit 2.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
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
/// The distribution's file that names the builds `symphony-build publish-client` keeps in its
/// `assets/`, newest first (crates/symphony/src/cli/publish.rs).
const KEPT: &str = "builds.json";
/// The binary whose build embeds that distribution (apps/symphony-server/build.rs): its line is its
/// file less the client's bytes, which have the client's line, so each line grows for one reason.
const EMBEDDER: &str = "symphony-server";
/// Where the shipped set is stated, as a refusal names it.
const SHIPPED: &str = "`BUILDS` in crates/symphony/src/init.rs";
const USAGE: &str = "usage: symphony-build size-budget ROOT RELEASE";
/// How far above a binary's size its line is set, in percent, and the bytes the line is rounded
/// down to (docs/workflow/CHECKS.md, "Where a line stands").
const MARGIN_PERCENT: u64 = 4;
const ROUNDED_TO: u64 = 10_000;
/// What a line past its budget asks for, after how to see what grew.
const RAISED: &str = "A reduction takes the bytes back first; a line is raised only in a commit \
                      of its own that names the growth and why no reduction will do, to the size \
                      plus 4 % and never higher, on a fresh reviewer's approve (docs/workflow/CHECKS.md, \
                      \"A red at the size check\").";

/// The budget file: the embedded client's line and each shipped binary's, in bytes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    /// The most the client distribution's kept builds may weigh, each counted whole
    /// ([`Distribution::kept`]).
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
    /// The client distribution: each kept build's own weight, newest first, and the bytes and
    /// the files the server embeds, a file the builds share once.
    Client {
        builds: Vec<u64>,
        embedded: u64,
        files: usize,
    },
}

/// The client distribution as the server's build embeds it: its bytes and its files, when the
/// newest of them last changed (none for no file), and the builds [`KEPT`] names in it.
struct Distribution {
    /// Every regular file's bytes, each once: what the server embeds.
    embedded: u64,
    files: usize,
    newest: Option<SystemTime>,
    /// Each kept build's own weight, the bytes of the files [`KEPT`] names for it, newest first.
    builds: Vec<u64>,
    /// The files outside `assets/` (the page, [`KEPT`]), which every build is served with.
    page: u64,
}

impl Distribution {
    /// What the client's line counts: each kept build whole, a file builds share once for each
    /// build that names it, and the files outside `assets/` once. It is the most the distribution
    /// weighs with these builds, as it does once they share no file: a change to the chunk every
    /// page imports renames every chunk that imports it, so a build after one shares almost
    /// nothing with the builds before it. What the builds share moves the bytes embedded, never
    /// this, which moves only with what a build weighs and how many are kept.
    fn kept(&self) -> u64 {
        self.builds.iter().sum::<u64>() + self.page
    }
}

/// [`KEPT`] as `symphony-build publish-client` writes it, read for the files each build names.
#[derive(Deserialize)]
struct Kept {
    builds: Vec<KeptBuild>,
}

#[derive(Deserialize)]
struct KeptBuild {
    shell: String,
    files: Vec<String>,
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
        bytes: client.kept(),
        what: What::Client {
            builds: client.builds,
            embedded: client.embedded,
            files: client.files,
        },
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
    meta.len().checked_sub(client.embedded).ok_or_else(|| {
        format!(
            "{EMBEDDER}: {} is {} bytes, less than the {} of {CLIENT} it embeds, so it embeds \
             another client; {rebuild}",
            path.display(),
            grouped(meta.len()),
            grouped(client.embedded)
        )
    })
}

/// The regular files under `folder`, as the server's build script embeds them, and the builds
/// [`KEPT`] names among them; no folder is none, and a folder without [`KEPT`] keeps no build. A
/// link or any other kind of entry is an error, as it is there, and so is a file under `assets/`
/// no build names, or a build naming one that is not there: the line counts the builds, so a file
/// outside them would be embedded uncounted.
fn folder(folder: &Path) -> io::Result<Distribution> {
    let mut found = Distribution {
        embedded: 0,
        files: 0,
        newest: None,
        builds: Vec::new(),
        page: 0,
    };
    match fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(io::Error::other("the client distribution is no folder")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(error),
    }
    // Every file by its path under `folder`, `/` apart.
    let mut files = BTreeMap::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                found.embedded += meta.len();
                found.files += 1;
                let changed = meta.modified()?;
                found.newest = Some(found.newest.map_or(changed, |newest| newest.max(changed)));
                files.insert(under(folder, &entry.path())?, meta.len());
            } else {
                return Err(io::Error::other(format!(
                    "{}: client assets must be regular files",
                    entry.path().display()
                )));
            }
        }
    }
    let kept: Kept = match fs::read_to_string(folder.join(KEPT)) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| io::Error::other(format!("{KEPT}: {error}")))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Kept { builds: Vec::new() },
        Err(error) => return Err(io::Error::other(format!("{KEPT}: {error}"))),
    };
    let mut named = BTreeSet::new();
    for build in &kept.builds {
        let mut weight = 0;
        for name in build.files.iter().collect::<BTreeSet<_>>() {
            // A file directly in `assets/`, as `publish-client` names them: a name with a `/`
            // would reach below it.
            let path = format!("assets/{name}");
            let bytes = files
                .get(&path)
                .filter(|_| !name.contains('/'))
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "{KEPT}: the build {} names {name:?}, which is no file of assets/",
                        build.shell
                    ))
                })?;
            weight += bytes;
            named.insert(path);
        }
        found.builds.push(weight);
    }
    let unnamed: Vec<&str> = (files.keys())
        .filter(|path| path.starts_with("assets/") && !named.contains(*path))
        .map(String::as_str)
        .collect();
    if !unnamed.is_empty() {
        return Err(io::Error::other(format!(
            "{KEPT} names no build that holds {}: the line counts the builds, so the server \
             would embed what it does not count; `symphony-build publish-client` keeps no file \
             outside its builds (`just client-update` publishes the client again)",
            unnamed.join(", ")
        )));
    }
    found.page = (files.iter())
        .filter(|(path, _)| !path.starts_with("assets/"))
        .map(|(_, bytes)| bytes)
        .sum();
    Ok(found)
}

/// `path`'s place under `folder`, its parts `/` apart.
fn under(folder: &Path, path: &Path) -> io::Result<String> {
    let parts: Option<Vec<&str>> = (path.strip_prefix(folder).ok())
        .and_then(|relative| relative.iter().map(|part| part.to_str()).collect());
    parts
        .map(|parts| parts.join("/"))
        .ok_or_else(|| io::Error::other(format!("{}: no UTF-8 name", path.display())))
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
                    What::Client {
                        ref builds,
                        embedded,
                        files,
                    } => format!(
                        " ({}; {} bytes embedded in {files} files)",
                        weighing(builds),
                        grouped(embedded)
                    ),
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
             that last set the budget (`git log -1 -- {BUDGET}`). {RAISED}",
            match whole {
                Some(whole) => format!(
                    ", less its embedded client (its file is {} bytes),",
                    grouped(*whole)
                ),
                None => String::new(),
            }
        ),
        What::Client {
            builds,
            embedded,
            files,
        } => format!(
            "size-budget: the embedded client ({CLIENT}: {}) {over}. Its line counts each build \
             `symphony-build publish-client` keeps whole, a file builds share once for each, and \
             the files beside assets/ once; the server embeds {} bytes in {files} files of them. \
             See what the newest build grew by beside the one before it: the files `jq -r \
             '.builds[0].files[]' {CLIENT}/{KEPT}` names in {CLIENT}/assets, beside \
             `.builds[1]`'s. {RAISED}",
            weighing(builds),
            grouped(*embedded)
        ),
    }
}

/// The kept builds' weights as a line says them: `3 builds of 756,886, 754,713 and 755,142 bytes,
/// newest first`.
fn weighing(builds: &[u64]) -> String {
    let each: Vec<String> = builds.iter().map(|bytes| grouped(*bytes)).collect();
    match each.as_slice() {
        [] => "no build".to_string(),
        [one] => format!("1 build of {one} bytes"),
        [rest @ .., last] => format!(
            "{} builds of {} and {last} bytes, newest first",
            each.len(),
            rest.join(", ")
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
