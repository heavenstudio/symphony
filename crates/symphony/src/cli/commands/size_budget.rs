//! `symphony size-budget ROOT RELEASE`: each binary this workspace ships, as `cargo install` builds
//! it into RELEASE, and the client distribution the server embeds, against its line in the budget
//! file under ROOT ([`BUDGET`], docs/CHECKS.md "Size budgets"). Within every line it prints each
//! size beside its budget (exit 0); past one it says the size, the budget and how to see what grew
//! (exit 1); a budget it cannot read, or a binary it cannot find, is exit 2.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::Deserialize;

/// The budget, under the checkout.
const BUDGET: &str = "tools/size-budget.toml";
/// The client distribution the server's build embeds, under the checkout.
const CLIENT: &str = "apps/symphony-server/client/public";
/// The binary whose build embeds that distribution (apps/symphony-server/build.rs): its line is its
/// file less the client's bytes, which have the client's line, so each line grows for one reason.
const EMBEDDER: &str = "symphony-server";
const USAGE: &str = "usage: symphony size-budget ROOT RELEASE";

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

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let [root, release] = args else {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    };
    match measure(Path::new(root), Path::new(release)) {
        Ok(lines) => report(&lines, out, err),
        Err(why) => {
            let _ = writeln!(err, "symphony size-budget: {why}");
            2
        }
    }
}

/// Every line of the budget under `root`, measured: the binaries in `release`, then the client.
fn measure(root: &Path, release: &Path) -> Result<Vec<Measured>, String> {
    let text = fs::read_to_string(root.join(BUDGET)).map_err(|e| format!("{BUDGET}: {e}"))?;
    let budget: Budget =
        toml::from_str(&text).map_err(|e| format!("{BUDGET}: {}", e.to_string().trim_end()))?;
    if budget.binaries.is_empty() {
        return Err(format!("{BUDGET}: [binaries] names no binary"));
    }
    let (client, files) = folder(&root.join(CLIENT)).map_err(|e| format!("{CLIENT}: {e}"))?;
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
            (meta.len().saturating_sub(client), Some(meta.len()))
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
        what: What::Client { files },
        bytes: client,
        budget: budget.client,
    });
    Ok(lines)
}

/// The bytes and the number of the regular files under `folder`, as the server's build script
/// embeds them; no folder is none. A link or any other kind of entry is an error, as it is there.
fn folder(folder: &Path) -> io::Result<(u64, usize)> {
    match fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(io::Error::other("the client distribution is no folder")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(error),
    }
    let (mut bytes, mut files) = (0, 0);
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                bytes += meta.len();
                files += 1;
            } else {
                return Err(io::Error::other(format!(
                    "{}: client assets must be regular files",
                    entry.path().display()
                )));
            }
        }
    }
    Ok((bytes, files))
}

/// Each line within its budget on `out`, each one past it on `err` with how to see what grew. Exit
/// 1 when one is past.
fn report(lines: &[Measured], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let width = lines.iter().map(|line| name(line).len()).max().unwrap_or(0);
    let mut past = 0;
    for line in lines {
        if line.bytes <= line.budget {
            let _ = writeln!(
                out,
                "size-budget: {:<width$}  {} of {}{}",
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
             {name}` (`cargo install cargo-bloat --root DIR`), beside the same at the last \
             release (`git tag -l 'release-*'`); a budget is raised only in the commit that \
             needs it, saying why.",
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
             holds: `ls -lS {CLIENT}/assets`; every generation `symphony publish-client` keeps \
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
