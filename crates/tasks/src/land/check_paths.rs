//! What of a project's tests and checks a branch changes (`[land] check_paths`). The land reads it
//! for a red filed as a fast fix ([`super::independent`]): its orchestrator's approve lands only a
//! fix that changes none of them, since a loosened test or check is how a red turns green unfixed,
//! and a fresh reviewer's lands any other.
//!
//! An entry names paths as a `.gitignore` line does: a final `/` names a folder and what is inside
//! it, a `/` at the start or in the middle anchors it at the project's root, and one with neither
//! names a path's last part or any folder on the way to it; `*` and `?` stand for any run of
//! characters and any one character within a part. `PATH [TABLE]` names a table of a TOML file
//! (`/symphony.toml [verdicts]`, dotted for one inside another), which counts only where its value
//! differs. Besides the entries, a Rust file counts where the diff changes:
//!
//! - a test-only module's file: one whose file says `#![cfg(test)]`, one a file declares under
//!   `#[cfg(test)]` (`#[cfg(test)] mod fixture;`, through `#[path]` too), or one a test-only module
//!   declares in turn;
//! - an item under `#[cfg(test)]` inline in a source file, read by rustfmt's layout: from the
//!   attribute, past the attributes after it, to the `}` closing at its indentation, or to the
//!   line ending a short item with `;`, `,` or `}`;
//! - a lint allowed or expected (`#[allow(`, `#[expect(`, and their `#![` forms).
//!
//! What this cannot tell counts as product, and is said here rather than guessed: a module declared
//! inside an inline module (`mod a { mod b; }`) is looked for beside its file, a `}` at an item's
//! indentation inside a string ends the item early, and a test-only file named only by a macro or
//! a build script, or a lint set in a file no entry names, is product.

use super::run_git;
use crate::sweep::git::git;
use std::path::Path;

#[cfg(test)]
mod tests;

/// The entries a project that names none has: its tests (a `tests/` folder anywhere, a module file
/// named `tests.rs` or `NAME_tests.rs`), its recipes, the Symphony's size budget and the tables of
/// its project file that say what checks, lands and releases.
pub const DEFAULT: &[&str] = &[
    "tests/",
    "*_tests.rs",
    "tests.rs",
    "/justfile",
    "/tools/size-budget.toml",
    "/symphony.toml [verdicts]",
    "/symphony.toml [land]",
    "/symphony.toml [releases]",
];

/// How many files up a module's declarations are followed.
const DEPTH: usize = 8;

/// One `[land] check_paths` entry: the paths it names, and the table of them it narrows to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// As written, trimmed: how a refusal names it.
    pub written: String,
    anchored: bool,
    folder: bool,
    parts: Vec<String>,
    table: Option<Vec<String>>,
}

impl Entry {
    /// `written` as an entry, or the line saying why it is none: a path leaving the project or
    /// naming nothing, or a table of a folder or with no name.
    pub fn parse(written: &str) -> Result<Entry, String> {
        let text = written.trim();
        let refused = |why: &str| format!("[land] check_paths names {written:?}, {why}");
        let (path, table) = match text.strip_suffix(']').and_then(|t| t.rsplit_once(" [")) {
            Some((path, table)) => {
                let keys: Vec<String> = table.split('.').map(|k| k.trim().to_string()).collect();
                if keys.iter().any(|k| k.is_empty() || k.contains(['[', ']'])) {
                    return Err(refused("whose table has no TOML table's name"));
                }
                (path.trim_end(), Some(keys))
            }
            None => (text, None),
        };
        let folder = path.ends_with('/');
        let inner = path.trim_end_matches('/');
        let anchored = inner.starts_with('/') || inner.contains('/');
        let inner = inner.trim_start_matches('/');
        let parts: Vec<String> = inner.split('/').map(str::to_string).collect();
        if inner.is_empty() || parts.iter().any(|p| p.is_empty() || p == "." || p == "..") {
            return Err(refused("which is no path inside the project"));
        }
        if folder && table.is_some() {
            return Err(refused("a table of a folder, which is no TOML file"));
        }
        Ok(Entry {
            written: text.to_string(),
            anchored,
            folder,
            parts,
            table,
        })
    }

    /// Whether the entry names the project's `path`, its parts joined by `/`, its table aside: a
    /// table's entry names its file alone.
    pub fn names(&self, path: &str) -> bool {
        let path: Vec<&str> = path.split('/').collect();
        let last = path.len() - 1;
        if !self.anchored {
            let name = &self.parts[0];
            let at = |i: usize| match (&self.table, self.folder) {
                (Some(_), _) => i == last,
                (None, true) => i < last,
                (None, false) => true,
            };
            return path
                .iter()
                .enumerate()
                .any(|(i, part)| at(i) && glob(name, part));
        }
        let n = self.parts.len();
        let fits = if self.table.is_some() {
            path.len() == n
        } else {
            path.len() >= n + usize::from(self.folder)
        };
        fits && self.parts.iter().zip(&path).all(|(p, part)| glob(p, part))
    }
}

/// The entries `written`, or the line saying which is none.
pub fn parse(written: &[String]) -> Result<Vec<Entry>, String> {
    written.iter().map(|w| Entry::parse(w)).collect()
}

/// [`DEFAULT`]'s entries.
pub fn defaults() -> Vec<Entry> {
    DEFAULT
        .iter()
        .map(|w| Entry::parse(w).expect("the default entries are paths inside the project"))
        .collect()
}

/// Whether `text`, one part of a path, fits `pattern`: `*` any run of characters, `?` any one.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut from) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            from = ti;
            pi += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            from += 1;
            ti = from;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// What of `entries`, and of the Rust files' tests and lints, the commits from where `tip` left
/// `base` to `tip` change in the checkout `dir`: each path, table or Rust change, as a refusal
/// names it, in the diff's order; else the line saying why git cannot tell.
pub fn touched(
    dir: &Path,
    base: &str,
    tip: &str,
    entries: &[Entry],
) -> Result<Vec<String>, String> {
    let from = run_git(dir, &["merge-base", base, tip])?;
    let listed = raw(
        dir,
        &["diff", "--name-only", "-z", "--no-renames", &from, tip],
    )?;
    let (old, new) = (Tree { dir, commit: &from }, Tree { dir, commit: tip });
    let mut found = Vec::new();
    for path in listed.split('\0').filter(|p| !p.is_empty()) {
        let named: Vec<&Entry> = entries.iter().filter(|e| e.names(path)).collect();
        if named.iter().any(|e| e.table.is_none()) {
            found.push(path.to_string());
            continue;
        }
        for entry in &named {
            let keys = entry.table.as_deref().unwrap_or_default();
            if old.table(path, keys)? != new.table(path, keys)? {
                found.push(entry.written.clone());
            }
        }
        if named.is_empty() && path.ends_with(".rs") {
            found.extend(rust(&old, &new, path, entries)?);
        }
    }
    Ok(found)
}

/// What a change to the Rust file `path` from `old` to `new` changes of the tests and lints: the
/// file itself when its module is test-only on either side, else each hunk reaching a
/// `#[cfg(test)]` item and each lint it allows.
fn rust(
    old: &Tree<'_>,
    new: &Tree<'_>,
    path: &str,
    entries: &[Entry],
) -> Result<Vec<String>, String> {
    if old.test_only(path, entries, 0)? || new.test_only(path, entries, 0)? {
        return Ok(vec![format!("{path} (a test module)")]);
    }
    let diff = raw(
        old.dir,
        &[
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            old.commit,
            new.commit,
            "--",
            path,
        ],
    )?;
    let was = regions(&old.read(path)?.unwrap_or_default());
    let is = regions(&new.read(path)?.unwrap_or_default());
    let mut found = Vec::new();
    for hunk in hunks(&diff) {
        let line = within(hunk.old, &was)
            .map(|_| hunk.new.0)
            .or_else(|| within(hunk.new, &is));
        if let Some(line) = line {
            found.push(format!("{path} (#[cfg(test)] code at line {line})"));
        }
        for (line, text) in &hunk.added {
            if allows(text) {
                found.push(format!("{path} (a lint allowed at line {line})"));
            }
        }
    }
    Ok(found)
}

/// One commit's files in a checkout.
struct Tree<'a> {
    dir: &'a Path,
    commit: &'a str,
}

impl Tree<'_> {
    /// The file `path`'s text, none where the commit has no such file.
    fn read(&self, path: &str) -> Result<Option<String>, String> {
        let at = format!("{}:{path}", self.commit);
        if git(self.dir, &["cat-file", "-e", &at]).is_none_or(|done| !done.status.success()) {
            return Ok(None);
        }
        raw(self.dir, &["show", &at]).map(Some)
    }

    /// The value of the table `keys` of the TOML file `path`: none where the file or the table is
    /// not, and the file's whole text where it is no TOML, so any change to it counts.
    fn table(&self, path: &str, keys: &[String]) -> Result<Option<toml::Value>, String> {
        let Some(text) = self.read(path)? else {
            return Ok(None);
        };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else {
            return Ok(Some(toml::Value::String(text)));
        };
        let mut value = toml::Value::Table(table);
        for key in keys {
            match value.get(key.as_str()) {
                Some(inner) => value = inner.clone(),
                None => return Ok(None),
            }
        }
        Ok(Some(value))
    }

    /// The Rust files of the commit holding any of `needles`.
    fn holding(&self, needles: &[&str]) -> Result<Vec<String>, String> {
        let mut args = vec!["grep", "-l", "-F"];
        for needle in needles {
            args.extend(["-e", needle]);
        }
        args.extend([self.commit, "--", "*.rs"]);
        let done = git(self.dir, &args)
            .ok_or_else(|| format!("git cannot run in {}", self.dir.display()))?;
        // `git grep` says it found nothing with 1.
        if !done.status.success() && done.status.code() != Some(1) {
            return Err(format!(
                "git grep: {}",
                String::from_utf8_lossy(&done.stderr).trim()
            ));
        }
        let prefix = format!("{}:", self.commit);
        Ok(String::from_utf8_lossy(&done.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix).map(str::to_string))
            .collect())
    }

    /// Whether the module of the Rust file `path` is test-only here: an entry names it (past the
    /// file asked about), its file says `#![cfg(test)]`, or a file declares it under
    /// `#[cfg(test)]` or is test-only itself, followed [`DEPTH`] files up.
    fn test_only(&self, path: &str, entries: &[Entry], depth: usize) -> Result<bool, String> {
        if depth > DEPTH {
            return Ok(false);
        }
        if depth > 0 && entries.iter().any(|e| e.table.is_none() && e.names(path)) {
            return Ok(true);
        }
        if self
            .read(path)?
            .is_some_and(|text| text.lines().any(|l| l.trim() == "#![cfg(test)]"))
        {
            return Ok(true);
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        let name = match file.strip_suffix(".rs") {
            Some("mod") => parent(path).rsplit('/').next().unwrap_or_default(),
            // A crate's root, which no module declares.
            Some("lib" | "main" | "build") | None => return Ok(false),
            Some(stem) => stem,
        };
        if name.is_empty() {
            return Ok(false);
        }
        let declaration = format!("mod {name}");
        for declarer in self.holding(&[&declaration, file])? {
            if declarer == path {
                continue;
            }
            let Some(text) = self.read(&declarer)? else {
                continue;
            };
            let gated = regions(&text);
            for (line, module, at) in declarations(&text) {
                if !declared(&declarer, &module, at.as_deref())
                    .iter()
                    .any(|p| p == path)
                {
                    continue;
                }
                if within((line, 1), &gated).is_some()
                    || self.test_only(&declarer, entries, depth + 1)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

/// The folder of the project's `path`, empty at the root.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `path` under the folder `dir`, its `.` and `..` parts folded.
fn joined(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// The files a module `module` declared in `declarer` may be: the one its `#[path]` (`at`) names
/// beside the declarer, else `module.rs` or `module/mod.rs` in the declarer's folder of modules
/// (its own folder for `mod.rs`, `lib.rs` and `main.rs`, else the folder named as it), and beside
/// it too for a declarer that may be a crate root of another name (a binary's, a test's, a bench's,
/// an example's, `build.rs`).
fn declared(declarer: &str, module: &str, at: Option<&str>) -> Vec<String> {
    let dir = parent(declarer);
    if let Some(at) = at {
        return vec![joined(dir, at)];
    }
    let file = declarer.rsplit('/').next().unwrap_or(declarer);
    let stem = file.strip_suffix(".rs").unwrap_or(file);
    let mut folders = Vec::new();
    if matches!(stem, "mod" | "lib" | "main") {
        folders.push(dir.to_string());
    } else {
        folders.push(joined(dir, stem));
        let above = dir.rsplit('/').next().unwrap_or(dir);
        if stem == "build" || matches!(above, "bin" | "tests" | "benches" | "examples") {
            folders.push(dir.to_string());
        }
    }
    folders
        .iter()
        .flat_map(|folder| {
            [
                joined(folder, &format!("{module}.rs")),
                joined(folder, &format!("{module}/mod.rs")),
            ]
        })
        .collect()
}

/// The modules a Rust file's `text` declares by file (`mod NAME;`, `pub mod NAME;`): the line of
/// each (from 1), its name and the path its `#[path = "…"]` gives it, if any.
fn declarations(text: &str) -> Vec<(usize, String, Option<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(module) = declares(line.trim()) else {
            continue;
        };
        let at = lines[..i]
            .iter()
            .rev()
            .map(|l| l.trim())
            .take_while(|l| l.starts_with("#["))
            .find_map(|l| {
                let value = l.strip_prefix("#[path")?.trim_start().strip_prefix('=')?;
                let value = value.trim().strip_suffix(']')?.trim();
                Some(value.strip_prefix('"')?.strip_suffix('"')?.to_string())
            });
        found.push((i + 1, module, at));
    }
    found
}

/// The module a line declares by file, `mod NAME;` after any visibility.
fn declares(line: &str) -> Option<String> {
    let rest = match line.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')')?.1.trim_start(),
        Some(rest) if rest.starts_with(' ') => rest.trim_start(),
        _ => line,
    };
    let name = rest.strip_prefix("mod ")?.trim().strip_suffix(';')?.trim();
    let name = name.strip_prefix("r#").unwrap_or(name);
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| name.to_string())
}

/// Whether an attribute line opens what only tests compile: `#[cfg(test)]` or
/// `#[cfg(all(test, …))]`.
fn testing(line: &str) -> bool {
    let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    line.starts_with("#[cfg(test)]") || line.starts_with("#[cfg(all(test,")
}

/// Whether a line allows or expects a lint.
fn allows(line: &str) -> bool {
    let line = line.trim_start();
    ["#[allow(", "#[expect(", "#![allow(", "#![expect("]
        .iter()
        .any(|start| line.starts_with(start))
}

/// The lines (from 1, both ends in) of each item under `#[cfg(test)]` in a Rust file's `text`
/// ([`item_end`]).
fn regions(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if !testing(trimmed) {
            i += 1;
            continue;
        }
        let end = item_end(&lines, i, lines[i].len() - trimmed.len());
        found.push((i + 1, end + 1));
        i = end + 1;
    }
    found
}

/// The index of the line ending the item whose first attribute is on the line `start`, indented
/// `indent`: past its attributes (their brackets balanced) and comments, the head ends on the first
/// line ending with `;`, `,` or `}`, the item with it, or with `{`, the item at the next `}`
/// indented as the attribute; the last line when nothing ends it.
fn item_end(lines: &[&str], start: usize, indent: usize) -> usize {
    let last = lines.len() - 1;
    let mut depth = 0;
    for at in start..lines.len() {
        let (after, rest) = past_attributes(lines[at], depth);
        depth = after;
        if depth > 0 || rest.is_empty() || rest.starts_with("//") {
            continue;
        }
        let mut head = rest.trim_end().to_string();
        for line in at..lines.len() {
            if line > at {
                head = lines[line].trim_end().to_string();
            }
            if head.ends_with('{') {
                return (line + 1..lines.len())
                    .find(|&i| {
                        let inner = lines[i].trim_start();
                        lines[i].len() - inner.len() == indent && inner.starts_with('}')
                    })
                    .unwrap_or(last);
            }
            if head.ends_with([';', ',', '}']) {
                return line;
            }
        }
        return last;
    }
    last
}

/// What of `line` is left past the attributes it opens or goes on with, `depth` brackets into one
/// before it: the depth after them, and the rest, trimmed at its start.
fn past_attributes(line: &str, mut depth: usize) -> (usize, &str) {
    let mut rest = line.trim_start();
    loop {
        if depth == 0 {
            match rest.strip_prefix("#[").or_else(|| rest.strip_prefix("#![")) {
                Some(after) => {
                    depth = 1;
                    rest = after;
                }
                None => return (0, rest),
            }
        }
        let mut closed = None;
        for (i, c) in rest.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        closed = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match closed {
            Some(i) => rest = rest.get(i..).unwrap_or_default().trim_start(),
            None => return (depth, ""),
        }
    }
}

/// The first of the `count` lines from `start` that any of `regions` holds; a count of none is a
/// place between lines, which none holds.
fn within((start, count): (usize, usize), regions: &[(usize, usize)]) -> Option<usize> {
    (start..start + count).find(|line| regions.iter().any(|(s, e)| (s..=e).contains(&line)))
}

/// One hunk of a diff with no context: its old and new lines, `(start, count)` each, and the lines
/// it adds with their new line numbers.
#[derive(Debug, PartialEq, Eq)]
struct Hunk {
    old: (usize, usize),
    new: (usize, usize),
    added: Vec<(usize, String)>,
}

/// The hunks of a unified diff's text; what comes before the first is its header.
fn hunks(diff: &str) -> Vec<Hunk> {
    let mut found: Vec<Hunk> = Vec::new();
    let mut next = 0;
    for line in diff.lines() {
        if let Some(head) = line.strip_prefix("@@ ") {
            let mut sides = head.split_whitespace();
            let side = |word: Option<&str>, sign: char| -> Option<(usize, usize)> {
                let word = word?.strip_prefix(sign)?;
                Some(match word.split_once(',') {
                    Some((start, count)) => (start.parse().ok()?, count.parse().ok()?),
                    None => (word.parse().ok()?, 1),
                })
            };
            let (Some(old), Some(new)) = (side(sides.next(), '-'), side(sides.next(), '+')) else {
                continue;
            };
            next = new.0;
            found.push(Hunk {
                old,
                new,
                added: Vec::new(),
            });
        } else if let (Some(hunk), Some(text)) = (found.last_mut(), line.strip_prefix('+')) {
            hunk.added.push((next, text.to_string()));
            next += 1;
        }
    }
    found
}

/// What git printed for `args` in `dir`, as it printed it; else the line saying why it failed.
fn raw(dir: &Path, args: &[&str]) -> Result<String, String> {
    let done = git(dir, args).ok_or_else(|| format!("git cannot run in {}", dir.display()))?;
    if done.status.success() {
        return Ok(String::from_utf8_lossy(&done.stdout).into_owned());
    }
    let said = String::from_utf8_lossy(&done.stderr);
    let said = said
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed");
    Err(format!("git {}: {}", args.join(" "), said.trim()))
}
