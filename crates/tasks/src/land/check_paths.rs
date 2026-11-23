//! What of a project's tests and checks a branch changes (`[land] check_paths`), reported to its
//! assessor: a change to one keeps review, since a loosened test or check is how a red turns green
//! unfixed.
//!
//! An entry names paths as a `.gitignore` line does: a final `/` names a folder and what is inside
//! it, a `/` at the start or in the middle anchors it at the project's root, and one with neither
//! names a path's last part or any folder on the way to it; `*` and `?` stand for any run of
//! characters and any one character within a part. `PATH [TABLE]` names a table of a TOML file
//! (`/symphony.toml [verdicts]`, dotted for one inside another), which counts only where its value
//! differs. Besides the entries, a Rust file counts where the diff changes:
//!
//! - a test-only module's file: one whose file says `#![cfg(test)]`, one a file declares under a
//!   test's attribute (`#[cfg(test)] mod fixture;`, through `#[path]` too), or one a test-only
//!   module declares in turn;
//! - an item a test's attribute marks inline in a source file: `#[test]`, `#[bench]`, one whose
//!   last name says `test` (`#[tokio::test]`), or a `cfg` naming `test` but as `not(test)`
//!   (`#[cfg(any(test, …))]` among them), from the first attribute of its run (an `#[ignore]` above
//!   it) to what ends the item, read from the file's code, its comments, strings and characters
//!   (raw strings among them) left out;
//! - a line added or removed that sets a lint's level (`allow`, `expect`, `warn`, `deny`,
//!   `forbid`, in an attribute or a `cfg_attr`).
//!
//! What it cannot tell counts as a check, never as product: a file whose code does not lex, or
//! where an item a test marks runs to the end unclosed, counts whole, and so does a module declared
//! by such a file; a path git fails to read, or a branch with more than one merge base, refuses.
//! What it does not see is product: a module declared inside an inline module (`mod a { mod b; }`)
//! is looked for beside its file, and a test-only file named only by a macro or a build script, or
//! a lint set in a file no entry names, is product.

use crate::sweep::git::{blobs, git};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

#[cfg(test)]
mod tests;

/// The entries a project that names none has: its tests and what they compare against (a `tests/`,
/// `fixtures/`, `testdata/` or `snapshots/` folder anywhere, a module file named `tests.rs` or
/// `NAME_tests.rs`, a `.snap`), the switches that turn a package's tests off (its `[lib]`'s `test`,
/// `doctest` and `harness`, its `[[test]]`, `[[bench]]`, `[[bin]]` and `[[example]]` targets whole,
/// each with its `test`, `harness` and `required-features`, `autotests` and `autobenches`, and its
/// `[features]`, which decide what `#[cfg(feature = …)]` and `required-features` leave out of a
/// test run), the `[dev-dependencies]` its tests alone build with, its recipes (the justfile and
/// the `symphony.just` it imports) and the `tools/` folder of what they run (verify's steps and the
/// Symphony's size budget among it), its lint and format settings (Clippy's file, rustfmt's,
/// Cargo's configuration, the workspace's and each package's `[lints]`), the tables of its project
/// file that say what checks, lands and releases, and the Symphony's own checkers, its run's list
/// of them and this gate, which name nothing in another project.
pub const DEFAULT: &[&str] = &[
    "tests/",
    "fixtures/",
    "testdata/",
    "snapshots/",
    "*_tests.rs",
    "tests.rs",
    "*.snap",
    "Cargo.toml [lib.test]",
    "Cargo.toml [lib.doctest]",
    "Cargo.toml [lib.harness]",
    "Cargo.toml [test]",
    "Cargo.toml [bench]",
    "Cargo.toml [bin]",
    "Cargo.toml [example]",
    "Cargo.toml [package.autotests]",
    "Cargo.toml [package.autobenches]",
    "Cargo.toml [features]",
    "Cargo.toml [dev-dependencies]",
    "/justfile",
    "/symphony.just",
    "/tools/",
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    "/.cargo/",
    "/Cargo.toml [workspace.lints]",
    "Cargo.toml [lints]",
    "/symphony.toml [verdicts]",
    "/symphony.toml [land]",
    "/symphony.toml [releases]",
    "/crates/symphony-build/src/commands/size_budget.rs",
    "/crates/symphony-build/src/commands/crates_check.rs",
    "/crates/symphony-build/src/release/crates.rs",
    "/crates/symphony/src/docs_audit.rs",
    "/crates/symphony/src/docs_audit/",
    "/crates/symphony-build/src/release.rs",
    "/crates/symphony-build/src/release/checks.rs",
    "/crates/symphony-build/src/release/steps.rs",
    // Their paths before the move to a package of their own, which a branch made before it still
    // names.
    "/crates/symphony/src/cli/commands/size_budget.rs",
    "/crates/symphony/src/cli/commands/crates_check.rs",
    "/crates/symphony/src/release/crates.rs",
    "/crates/symphony/src/release.rs",
    "/crates/symphony/src/release/checks.rs",
    "/crates/symphony/src/release/steps.rs",
    // And before the rename to `just release`.
    "/crates/symphony/src/nightly.rs",
    "/crates/symphony/src/nightly/checks.rs",
    "/crates/symphony/src/nightly/steps.rs",
    "/crates/tasks/src/land.rs",
    "/crates/tasks/src/land/",
    "/crates/tasks/src/stamps.rs",
    "/crates/tasks/src/stamps/",
    "/crates/tasks/src/red.rs",
    "/crates/tasks/src/store/work/red.rs",
];

/// How many files up a module's declarations are followed.
const DEPTH: usize = 8;

/// One `[land] check_paths` entry: the paths it names, and the table of them it narrows to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// As the project file writes it, trimmed.
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

    /// Whether the entry names its paths whole, with no table to narrow them to: so any change to
    /// one counts, as a raise of a limit in it must ([`crate::gates`]).
    pub fn whole(&self) -> bool {
        self.table.is_none()
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
/// names it, in the diff's order; else the line saying why git cannot tell, as where the two have
/// more than one merge base, which makes no one diff of the branch.
pub fn touched(
    dir: &Path,
    base: &str,
    tip: &str,
    entries: &[Entry],
) -> Result<Vec<String>, String> {
    let bases = raw(dir, &["merge-base", "--all", base, tip])?;
    let bases: Vec<&str> = bases.split_whitespace().collect();
    let [from] = bases[..] else {
        return Err(format!(
            "{base} and {tip} have {} merge bases, so no one diff says what the branch changes",
            bases.len()
        ));
    };
    let listed = raw(
        dir,
        &["diff", "--name-only", "-z", "--no-renames", from, tip],
    )?;
    let paths: Vec<(&str, Vec<&Entry>)> = (listed.split('\0'))
        .filter(|p| !p.is_empty())
        .map(|path| (path, entries.iter().filter(|e| e.names(path)).collect()))
        .collect();
    // What is read of the files: a Rust file no entry names, its tests and lints, and a file only
    // tables' entries name, those tables. Each side of them is read at once, and the Rust files'
    // diffs in one diff.
    let rust_file = |path: &str, named: &[&Entry]| named.is_empty() && path.ends_with(".rs");
    let tables = |named: &[&Entry]| !named.is_empty() && named.iter().all(|e| !e.whole());
    let read: Vec<&str> = (paths.iter())
        .filter(|(path, named)| rust_file(path, named) || tables(named))
        .map(|(path, _)| *path)
        .collect();
    let (old, new) = (Tree::new(dir, from), Tree::new(dir, tip));
    old.load(&read)?;
    new.load(&read)?;
    let diffs = if paths.iter().any(|(path, named)| rust_file(path, named)) {
        diffs(dir, from, tip)?
    } else {
        HashMap::new()
    };
    let mut found = Vec::new();
    for (path, named) in &paths {
        if named.iter().any(|e| e.whole()) {
            found.push(path.to_string());
            continue;
        }
        for entry in named {
            let keys = entry.table.as_deref().unwrap_or_default();
            if old.table(path, keys)? != new.table(path, keys)? {
                found.push(format!("{path} [{}]", keys.join(".")));
            }
        }
        if rust_file(path, named) {
            found.extend(rust(&old, &new, path, entries, &diffs)?);
        }
    }
    Ok(found)
}

/// Each Rust file's diff from `from` to `tip` with no context, by its path: one `git diff` of the
/// commits' Rust files ([`split`]).
fn diffs(dir: &Path, from: &str, tip: &str) -> Result<HashMap<String, String>, String> {
    let diff = raw(
        dir,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            from,
            tip,
            "--",
            "*.rs",
        ],
    )?;
    Ok(split(&diff))
}

/// A diff's text by the file each part is of, a part starting at each `diff --git a/PATH b/PATH`
/// header, and two parts of one file (a file made a link, which git shows removed and added) one
/// text. A part whose header git quotes (a path holding a `"`, a `\` or a control character) is
/// left out, so its file's own diff is read.
fn split(diff: &str) -> HashMap<String, String> {
    let mut found: HashMap<String, String> = HashMap::new();
    let mut of: Option<String> = None;
    for line in diff.split_inclusive('\n') {
        if let Some(names) = line.strip_prefix("diff --git ") {
            let names = names.trim_end_matches('\n');
            of = (names.len().checked_sub(5))
                .and_then(|both| names.get(2..2 + both / 2))
                .filter(|path| names == format!("a/{path} b/{path}"))
                .map(str::to_string);
        }
        if let Some(path) = &of {
            found.entry(path.clone()).or_default().push_str(line);
        }
    }
    found
}

/// What a change to the Rust file `path` from `old` to `new` changes of the tests and lints: the
/// file itself when its module is test-only on either side or where its tests are cannot be read
/// on either ([`regions`]), else each hunk of its diff (from `diffs`, else its own) reaching a
/// test's item and each line it adds or removes that sets a lint's level.
fn rust(
    old: &Tree<'_>,
    new: &Tree<'_>,
    path: &str,
    entries: &[Entry],
    diffs: &HashMap<String, String>,
) -> Result<Vec<String>, String> {
    if old.test_only(path, entries, 0)? || new.test_only(path, entries, 0)? {
        return Ok(vec![format!("{path} (a test module)")]);
    }
    let own;
    let diff = match diffs.get(path) {
        Some(diff) => diff,
        None => {
            own = raw(
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
            &own
        }
    };
    let (Some(was), Some(is)) = (old.regions(path)?, new.regions(path)?) else {
        return Ok(vec![format!("{path} (where its tests end cannot be read)")]);
    };
    let mut found = Vec::new();
    for hunk in hunks(diff) {
        let line = within(hunk.old, &was)
            .map(|_| hunk.new.0)
            .or_else(|| within(hunk.new, &is));
        if let Some(line) = line {
            found.push(format!("{path} (test code at line {line})"));
        }
        // An added line by its own line; a removed one where the hunk stands now (git says 0 for
        // the place before the first line).
        let set = (hunk.added.iter())
            .find(|(_, text)| sets_level(text))
            .map(|(line, _)| *line)
            .or_else(|| {
                (hunk.removed.iter())
                    .any(|text| sets_level(text))
                    .then_some(hunk.new.0.max(1))
            });
        if let Some(line) = set {
            found.push(format!("{path} (a lint's level set at line {line})"));
        }
    }
    Ok(found)
}

/// One commit's files in a checkout, each listed and read from git once, which of its Rust files
/// declares which module, searched for once, and what the walk up a module reads of each file, read
/// once: so what is asked of it costs a few runs of git however many files a diff changes, and no
/// file is read twice however many of its modules it does.
struct Tree<'a> {
    dir: &'a Path,
    commit: &'a str,
    /// The commit's files, listed at the first read.
    files: RefCell<Option<HashSet<String>>>,
    /// The text of each file read so far, none for one the commit does not have.
    texts: RefCell<HashMap<String, Option<String>>>,
    /// Each file a module declared by file may be, with the files declaring it ([`Tree::declaring`]),
    /// found at the first question.
    declarers: RefCell<Option<HashMap<String, Vec<String>>>>,
    /// Each Rust file's [`Module`] read so far, none for one the commit does not have.
    modules: RefCell<HashMap<String, Option<Rc<Module>>>>,
}

/// What the walk up a Rust file's modules reads of it.
struct Module {
    /// Whether its file says its module is test-only ([`test_only_file`]).
    test_only: bool,
    /// Where its tests are ([`regions`]).
    regions: Option<Vec<(usize, usize)>>,
    /// The modules it declares by file ([`declarations`]).
    declarations: Vec<(usize, String, Option<String>)>,
}

impl<'a> Tree<'a> {
    fn new(dir: &'a Path, commit: &'a str) -> Tree<'a> {
        Tree {
            dir,
            commit,
            files: RefCell::default(),
            texts: RefCell::default(),
            declarers: RefCell::default(),
            modules: RefCell::default(),
        }
    }

    /// The Rust file `path`'s [`Module`], none where the commit has no such file.
    fn module(&self, path: &str) -> Result<Option<Rc<Module>>, String> {
        if let Some(module) = self.modules.borrow().get(path) {
            return Ok(module.clone());
        }
        let module = self.read(path)?.map(|text| {
            Rc::new(Module {
                test_only: test_only_file(&text),
                regions: regions(&text),
                declarations: declarations(&text),
            })
        });
        (self.modules.borrow_mut()).insert(path.to_string(), module.clone());
        Ok(module)
    }

    /// Where the tests of the Rust file `path` are ([`regions`]): none where that cannot be told,
    /// and nowhere in a file the commit does not have.
    fn regions(&self, path: &str) -> Result<Option<Vec<(usize, usize)>>, String> {
        Ok(match self.module(path)? {
            Some(module) => module.regions.clone(),
            None => Some(Vec::new()),
        })
    }

    /// The file `path`'s text, none where the commit has no such file; the line saying why where git
    /// cannot list or read it, so a file git fails on never reads as one that is not there.
    fn read(&self, path: &str) -> Result<Option<String>, String> {
        self.load(&[path])?;
        Ok(self.texts.borrow().get(path).cloned().flatten())
    }

    /// Each of `paths` no read has read yet read, those the commit has in one `git cat-file
    /// --batch`; the line saying why where git cannot list or read one.
    fn load(&self, paths: &[&str]) -> Result<(), String> {
        let mut wanted = Vec::new();
        for path in paths {
            if self.texts.borrow().contains_key(*path) {
                continue;
            }
            if self.has(path)? {
                wanted.push(*path);
            } else {
                (self.texts.borrow_mut()).insert(path.to_string(), None);
            }
        }
        if wanted.is_empty() {
            return Ok(());
        }
        let specs: Vec<String> = (wanted.iter())
            .map(|path| format!("{}:{path}", self.commit))
            .collect();
        ran();
        for (path, text) in wanted.iter().zip(blobs(self.dir, &specs)?) {
            // Listed, so a file git does not answer with is one it failed on.
            let text = text.ok_or_else(|| {
                format!("git cat-file --batch: {}:{path} gives no file", self.commit)
            })?;
            (self.texts.borrow_mut()).insert(path.to_string(), Some(text));
        }
        Ok(())
    }

    /// Whether the commit has the file `path`, listing them all the first time it is asked.
    fn has(&self, path: &str) -> Result<bool, String> {
        let mut files = self.files.borrow_mut();
        if files.is_none() {
            let listed = raw(
                self.dir,
                &[
                    "ls-tree",
                    "-r",
                    "--full-tree",
                    "--name-only",
                    "-z",
                    self.commit,
                ],
            )?;
            let listed = listed.split('\0').filter(|p| !p.is_empty());
            *files = Some(listed.map(str::to_string).collect());
        }
        Ok(files.as_ref().is_some_and(|files| files.contains(path)))
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

    /// The files declaring a module by file that may be the Rust file `path`, in the order git
    /// lists them: the commit's Rust files holding `mod ` searched for, read and their
    /// [`declarations`] noted at the first question, so a question costs no run of git.
    fn declaring(&self, path: &str) -> Result<Vec<String>, String> {
        if self.declarers.borrow().is_none() {
            let holding = self.holding(&["mod "])?;
            self.load(&holding.iter().map(String::as_str).collect::<Vec<_>>())?;
            let mut declarers: HashMap<String, Vec<String>> = HashMap::new();
            let texts = self.texts.borrow();
            for file in &holding {
                let Some(Some(text)) = texts.get(file) else {
                    continue;
                };
                for (_, module, at) in declarations(text) {
                    for may_be in declared(file, &module, at.as_deref()) {
                        let by = declarers.entry(may_be).or_default();
                        if by.last() != Some(file) {
                            by.push(file.clone());
                        }
                    }
                }
            }
            *self.declarers.borrow_mut() = Some(declarers);
        }
        let declarers = self.declarers.borrow();
        Ok((declarers.as_ref().and_then(|d| d.get(path)))
            .cloned()
            .unwrap_or_default())
    }

    /// The Rust files of the commit holding any of `needles`.
    fn holding(&self, needles: &[&str]) -> Result<Vec<String>, String> {
        let mut args = vec!["grep", "-l", "-F"];
        for needle in needles {
            args.extend(["-e", needle]);
        }
        args.extend([self.commit, "--", "*.rs"]);
        ran();
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
    /// file asked about), its file says `#![cfg(test)]` ([`test_only_file`]), or a file declares it
    /// under a test's attribute, or where that file's tests cannot be read, or is test-only itself,
    /// followed [`DEPTH`] files up.
    fn test_only(&self, path: &str, entries: &[Entry], depth: usize) -> Result<bool, String> {
        if depth > DEPTH {
            return Ok(false);
        }
        if depth > 0 && entries.iter().any(|e| e.table.is_none() && e.names(path)) {
            return Ok(true);
        }
        if self.module(path)?.is_some_and(|module| module.test_only) {
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
        for declarer in self.declaring(path)? {
            if declarer == path {
                continue;
            }
            // Only one holding the declaration or the file's name, as a search for those found
            // them: one writing `mod  NAME;` or `mod r#NAME;` was never followed.
            let holds = (self.texts.borrow().get(&declarer))
                .and_then(|text| text.as_deref())
                .is_some_and(|text| text.contains(&declaration) || text.contains(file));
            if !holds {
                continue;
            }
            let Some(module) = self.module(&declarer)? else {
                continue;
            };
            for (line, named, at) in &module.declarations {
                if !declared(&declarer, named, at.as_deref())
                    .iter()
                    .any(|p| p == path)
                {
                    continue;
                }
                if (module.regions.as_ref()).is_none_or(|gated| within((*line, 1), gated).is_some())
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

/// The lint levels an attribute sets, by the name it is set with.
const LEVELS: [&str; 5] = ["allow", "expect", "warn", "deny", "forbid"];

/// Whether a line, added or removed, sets a lint's level: an attribute line naming one
/// (`#[allow(`, `#![deny(`, `#[cfg_attr(test, allow(` …), or a line of an attribute going on that
/// starts with one.
fn sets_level(line: &str) -> bool {
    let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    let attribute = line.starts_with("#[") || line.starts_with("#![");
    LEVELS.iter().any(|level| {
        let call = format!("{level}(");
        line.starts_with(&call)
            || (attribute
                && line.match_indices(&call).any(|(at, _)| {
                    line[..at]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '.')
                }))
    })
}

/// A Rust file's code: its comments left out and each string or character literal made one empty
/// pair of `"`, each character with its line (from 1); none where the text does not lex, ending
/// inside a string, a character or a block comment.
fn code(text: &str) -> Option<Vec<(char, usize)>> {
    let chars: Vec<char> = text.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::with_capacity(chars.len());
    let (mut i, mut line) = (0, 1);
    while let Some(c) = at(i) {
        match c {
            '\n' => {
                out.push(('\n', line));
                line += 1;
                i += 1;
            }
            '/' if at(i + 1) == Some('/') => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            '/' if at(i + 1) == Some('*') => {
                let mut depth = 0;
                loop {
                    match (at(i)?, at(i + 1)) {
                        ('/', Some('*')) => (depth, i) = (depth + 1, i + 2),
                        ('*', Some('/')) => (depth, i) = (depth - 1, i + 2),
                        (c, _) => {
                            line += usize::from(c == '\n');
                            i += 1;
                        }
                    }
                    if depth == 0 {
                        break;
                    }
                }
                out.push((' ', line));
            }
            '"' => {
                let from = line;
                i += 1;
                loop {
                    match at(i)? {
                        '\\' => {
                            line += usize::from(at(i + 1) == Some('\n'));
                            i += 2;
                        }
                        '"' => break,
                        c => {
                            line += usize::from(c == '\n');
                            i += 1;
                        }
                    }
                }
                i += 1;
                out.extend([('"', from), ('"', line)]);
            }
            // A character: an escape, or one character between quotes; else a lifetime or label.
            '\'' if at(i + 1) == Some('\\') => {
                let mut j = i + 3;
                while at(j)? != '\'' {
                    if at(j)? == '\n' {
                        return None;
                    }
                    j += 1;
                }
                out.extend([('"', line), ('"', line)]);
                i = j + 1;
            }
            '\'' if at(i + 2) == Some('\'') && at(i + 1) != Some('\n') => {
                out.extend([('"', line), ('"', line)]);
                i += 3;
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while at(i).is_some_and(ident) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let hashes = chars[i..].iter().take_while(|c| **c == '#').count();
                if matches!(word.as_str(), "r" | "br" | "cr") && at(i + hashes) == Some('"') {
                    let from = line;
                    let mut j = i + hashes + 1;
                    loop {
                        let c = at(j)?;
                        if c == '"' && (1..=hashes).all(|k| at(j + k) == Some('#')) {
                            break;
                        }
                        line += usize::from(c == '\n');
                        j += 1;
                    }
                    i = j + 1 + hashes;
                    out.extend([('"', from), ('"', line)]);
                } else {
                    out.extend(word.chars().map(|c| (c, line)));
                }
            }
            c => {
                out.push((c, line));
                i += 1;
            }
        }
    }
    Some(out)
}

/// Whether an outer attribute's text (between `#[` and `]`, its strings emptied) marks what tests
/// alone run or compile: a test or bench (`test`, `bench`, `tokio::test`, `rstest`, any whose
/// last name says `test`), or a `cfg` naming `test` other than as `not(test)`.
fn testing(attribute: &str) -> bool {
    let text: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
    let path = text.split(['(', '=']).next().unwrap_or_default();
    let last = path.rsplit("::").next().unwrap_or_default();
    if last.contains("test") || last == "bench" {
        return true;
    }
    path == "cfg" && names_test(&text.replace("not(test)", ""))
}

/// Whether a `cfg` predicate's text names `test` as a word.
fn names_test(text: &str) -> bool {
    text.match_indices("test").any(|(at, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        !text[..at].chars().next_back().is_some_and(word)
            && !text[at + 4..].chars().next().is_some_and(word)
    })
}

/// Whether a Rust file's `text` says its module is test-only: an inner `#![cfg(…)]` naming `test`
/// ([`testing`]); where the text does not lex, whether it says `#![cfg(` at all.
fn test_only_file(text: &str) -> bool {
    let Some(code) = code(text) else {
        return text.contains("#![cfg(");
    };
    attributes(&code)
        .iter()
        .any(|a| a.inner && a.text.starts_with("cfg") && testing(&a.text))
}

/// An attribute of a file's [`code`]: whether it is inner (`#![`), its text between the brackets,
/// its line, and the index in the code just past its `]`.
struct Attribute {
    inner: bool,
    text: String,
    line: usize,
    end: usize,
}

/// The attributes of a file's [`code`], in order, the attributes inside one's text left in it.
fn attributes(code: &[(char, usize)]) -> Vec<Attribute> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < code.len() {
        if let Some(found_one) = attribute_at(code, i) {
            i = found_one.end;
            found.push(found_one);
        } else {
            i += 1;
        }
    }
    found
}

/// The attribute that opens at the index `i` of a file's [`code`], if one does; none for one never
/// closed.
fn attribute_at(code: &[(char, usize)], i: usize) -> Option<Attribute> {
    let (hash, line) = *code.get(i)?;
    if hash != '#' {
        return None;
    }
    let mut j = i + 1;
    let inner = code.get(j).map(|c| c.0) == Some('!');
    j += usize::from(inner);
    if code.get(j).map(|c| c.0) != Some('[') {
        return None;
    }
    let (mut depth, mut text) = (0, String::new());
    for (k, (c, _)) in code.iter().enumerate().skip(j) {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(Attribute {
                inner,
                text: text.get(1..).unwrap_or_default().to_string(),
                line,
                end: k + 1,
            });
        }
        text.push(*c);
    }
    None
}

/// The words that open an item that a `,` at its own depth does not end (a function's generics or
/// a `where` clause hold one).
const ITEMS: [&str; 15] = [
    "fn",
    "mod",
    "impl",
    "struct",
    "enum",
    "union",
    "trait",
    "type",
    "static",
    "const",
    "use",
    "extern",
    "let",
    "macro_rules",
    "async",
];

/// The lines (from 1, both ends in) of each item a test attribute ([`testing`]) marks in a Rust
/// file's `text`, from the first attribute of the run it is in (an `#[ignore]` above a `#[test]`
/// among them) to what ends the item, read from the file's [`code`]: past the attributes after it,
/// the `;` at its own depth, a `,` there (but in an item one of [`ITEMS`] opens), the `}` closing
/// what it opened, or a bracket closing what holds it. None where that cannot be told: the text
/// does not lex, or an item runs to its end unclosed.
fn regions(text: &str) -> Option<Vec<(usize, usize)>> {
    let code = code(text)?;
    let mut found = Vec::new();
    // The line of the first attribute of the run the one read is in, and where that run got to.
    let (mut first, mut after) = (0, usize::MAX);
    let mut i = 0;
    while i < code.len() {
        let Some(attribute) = attribute_at(&code, i) else {
            i += 1;
            continue;
        };
        let joined = after <= i && code[after..i].iter().all(|c| c.0.is_whitespace());
        if !joined {
            first = attribute.line;
        }
        (i, after) = (attribute.end, attribute.end);
        if attribute.inner || !testing(&attribute.text) {
            continue;
        }
        let end = item_end(&code, i)?;
        found.push((first, code[end].1));
        (i, after) = (end + 1, usize::MAX);
    }
    Some(found)
}

/// The index in a file's [`code`] of what ends the item whose head starts at `from`, past any
/// attributes ([`regions`]); none when nothing does.
fn item_end(code: &[(char, usize)], from: usize) -> Option<usize> {
    let (mut i, mut depth, mut item) = (from, 0_usize, false);
    while i < code.len() {
        if let Some(attribute) = attribute_at(code, i) {
            i = attribute.end;
            continue;
        }
        let c = code[i].0;
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while code
                .get(i)
                .is_some_and(|c| c.0.is_alphanumeric() || c.0 == '_')
            {
                i += 1;
            }
            let word: String = code[start..i].iter().map(|c| c.0).collect();
            item |= depth == 0 && ITEMS.contains(&word.as_str());
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => return Some(i),
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 && c == '}' {
                    return Some(i);
                }
            }
            ';' if depth == 0 => return Some(i),
            ',' if depth == 0 && !item => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// The first of the `count` lines from `start` that any of `regions` holds; a count of none is a
/// place between lines, which none holds.
fn within((start, count): (usize, usize), regions: &[(usize, usize)]) -> Option<usize> {
    (start..start + count).find(|line| regions.iter().any(|(s, e)| (s..=e).contains(&line)))
}

/// One hunk of a diff with no context: its old and new lines, `(start, count)` each, the lines it
/// adds with their new line numbers, and the lines it removes.
#[derive(Debug, PartialEq, Eq)]
struct Hunk {
    old: (usize, usize),
    new: (usize, usize),
    added: Vec<(usize, String)>,
    removed: Vec<String>,
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
                removed: Vec::new(),
            });
        } else if let (Some(hunk), Some(text)) = (found.last_mut(), line.strip_prefix('+')) {
            hunk.added.push((next, text.to_string()));
            next += 1;
        } else if let (Some(hunk), Some(text)) = (found.last_mut(), line.strip_prefix('-')) {
            hunk.removed.push(text.to_string());
        }
    }
    found
}

#[cfg(test)]
thread_local! {
    /// The runs of git this thread's judgements made ([`ran`]), which a test counts.
    static RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts a run of git on the thread for a test ([`RUNS`]): each this module makes calls it first.
fn ran() {
    #[cfg(test)]
    RUNS.set(RUNS.get() + 1);
}

/// What git printed for `args` in `dir`, as it printed it; else the line saying why it failed.
fn raw(dir: &Path, args: &[&str]) -> Result<String, String> {
    ran();
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
