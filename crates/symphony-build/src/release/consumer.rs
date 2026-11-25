//! What a release tells the agent that takes it in a consuming project (docs/workflow/RELEASES.md, "Its
//! notes"): what the project does to move from the release before, by section. Two sources, and no
//! file every branch edits:
//!
//! - **Derived**, read from git at the two releases' commits and never written by hand
//!   ([`derived`]): the line that names the release in a project's manifest and the package
//!   versions its lock takes ([`LOCK`]), the board schema and the pin, the settings keys
//!   ([`SETTINGS`]), the tools the repository pins ([`TOOLS`]), the
//!   files `symphony init` rewrites ([`SHIPPED`]) and the consumer-API guards that changed
//!   ([`GUARDS`]).
//! - **Entries**, said by the landing that changes what a consumer must do: a trailer of one of
//!   its branch's commits, `Consumer-Note: SECTION: TEXT` ([`parse`]), which the `--no-ff` land
//!   keeps on main, so the release reads them between two releases' commits ([`entries`]); a land
//!   a pull request delivered keeps them on the head its receipt names, where the release reads
//!   them for the squash ([`delivered`]). One
//!   that names a release already cut (`Consumer-Note: release-NAME SECTION: TEXT`) is read as
//!   that release's, and said again in the release whose commits hold it, for the project already
//!   past the one it names: what a landing left unsaid is said by a later commit.
//!
//! A landing that changes the board schema, or takes a call out of a guard, and says nothing is
//! red at its own tip ([`check`], a step of `just lint-rs`), not at the release. The notes of every
//! release compose ([`section`]): a replacement shipped in one and removed in a later one reads
//! "move before R". A release's section goes in its tag's message, and every release's, oldest
//! first, in `notes.md` of the release installed on the machine ([`document`]), which
//! `symphony release-notes` prints with no clone of this repository.

use std::path::Path;

use super::git::git;
use super::notes::{PIN_FILE, SCHEMA_FILE, constant};
use super::receipts::{Proof, Receipts};
use super::tags::Release;

/// The trailer a commit says an entry by.
pub(crate) const TRAILER: &str = "Consumer-Note";
/// The files `symphony init` writes into a project, in this repository.
const SHIPPED: &str = "crates/symphony/shipped/";
/// The consumer-API guards, wherever a package keeps one.
const GUARDS: &str = ":(glob)**/tests/consumer_api.rs";
/// The text of an entry that says its section owes a consumer nothing: it answers the check and
/// is left out of the notes.
const NONE: &str = "none";
/// What a check's refusal and the notes' reader are pointed at.
const WHERE: &str = "docs/workflow/RELEASES.md, \"Its notes\"";

/// A tool the repository pins: what the notes call it, the file that pins it and how that file
/// says the version.
type Tool = (&'static str, &'static str, fn(&str) -> Option<String>);

/// The tools a release pins, each read at a release's commit.
const TOOLS: [Tool; 3] = [
    ("Rust toolchain", "rust-toolchain.toml", toolchain),
    ("Node", "apps/symphony-web/.node-version", node),
    (
        "PostgreSQL, the oldest major the board's server runs on,",
        "crates/tasks/src/server/postgres.rs",
        postgres,
    ),
];

/// `channel = "1.99.0"` in `rust-toolchain.toml`.
fn toolchain(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("channel")?.trim_start();
        let value = value.strip_prefix('=')?.trim();
        Some(value.strip_prefix('"')?.strip_suffix('"')?.to_string())
    })
}

/// The whole of `.node-version`.
fn node(text: &str) -> Option<String> {
    Some(text.trim().to_string()).filter(|value| !value.is_empty())
}

/// `pub const OLDEST_MAJOR: u32 = 18;` in the board's PostgreSQL module.
fn postgres(text: &str) -> Option<String> {
    constant(text, "OLDEST_MAJOR").map(|major| major.to_string())
}

/// The settings reference, whose tables list every key of `symphony.toml` the Symphony reads.
const SETTINGS: &str = "docs/operations/PROJECT_FILE.md";

/// Every key the settings reference `text` lists, each as `[table] key`: a row of a table under
/// a `## `[table]`` heading, its first cell the key in backticks.
fn settings(text: &str) -> std::collections::BTreeSet<String> {
    let mut keys = std::collections::BTreeSet::new();
    let mut table = None;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            let heading = heading.trim().trim_matches('`');
            table = (heading.starts_with('[') && heading.ends_with(']')).then_some(heading);
        } else if let (Some(table), Some(row)) = (table, line.strip_prefix("| `"))
            && let Some((key, _)) = row.split_once('`')
        {
            keys.insert(format!("{table} {key}"));
        }
    }
    keys
}

/// The lock file, which names each workspace package with its version and no source.
const LOCK: &str = "Cargo.lock";

/// The version of each package the lock file `text` names with no source: the workspace's own.
fn packages(text: &str) -> std::collections::BTreeMap<String, String> {
    let value = |line: &str, key: &str| {
        let value = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
        Some(value.trim().trim_matches('"').to_string())
    };
    let mut found = std::collections::BTreeMap::new();
    for block in text.split("[[package]]").skip(1) {
        let field = |key: &str| block.lines().find_map(|line| value(line, key));
        if let (Some(name), Some(version), None) =
            (field("name"), field("version"), field("source"))
        {
            found.insert(name, version);
        }
    }
    found
}

/// What an entry is about, in the order the notes say them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Manifest,
    Board,
    Api,
    Settings,
    Init,
    Tools,
    Once,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::Manifest,
        Section::Board,
        Section::Api,
        Section::Settings,
        Section::Init,
        Section::Tools,
        Section::Once,
    ];

    /// The word a trailer names it by.
    fn word(self) -> &'static str {
        match self {
            Section::Manifest => "manifest",
            Section::Board => "board",
            Section::Api => "api",
            Section::Settings => "settings",
            Section::Init => "init",
            Section::Tools => "tools",
            Section::Once => "once",
        }
    }

    /// Its heading in the notes.
    fn title(self) -> &'static str {
        match self {
            Section::Manifest => "Cargo.toml and Cargo.lock",
            Section::Board => "Board",
            Section::Api => "Public Rust items a consumer calls",
            Section::Settings => "symphony.toml",
            Section::Init => "Installed recipes, skills, hooks and rules",
            Section::Tools => "Tools",
            Section::Once => "Run once",
        }
    }

    fn parse(word: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|section| section.word() == word)
    }

    /// Every word, as a refusal lists them.
    fn words() -> String {
        let words: Vec<&str> = Section::ALL.into_iter().map(Section::word).collect();
        words.join(", ")
    }
}

/// A public item and what a consumer calls in its place: `OLD => NEW` while both work, `removed
/// OLD => NEW` once the old one is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Moved {
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) removed: bool,
}

/// One entry: a trailer's value, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The release already cut it is said of, when it names one.
    pub(crate) release: Option<String>,
    pub(crate) section: Section,
    /// What it says; for a [`Moved`] entry what follows its `;`, which may be nothing.
    pub(crate) text: String,
    pub(crate) moved: Option<Moved>,
    /// For an entry said of a release already cut: the commit that said it and the release whose
    /// commits hold that one, which [`history`] fills in.
    pub(crate) after: Option<(String, String)>,
}

impl Entry {
    /// Whether it says its section owes a consumer nothing ([`NONE`]).
    fn nothing(&self) -> bool {
        self.moved.is_none()
            && (self.text == NONE
                || self.text.starts_with("none:")
                || self.text.starts_with("none;"))
    }
}

/// An item's name as an entry wrote it, without the backticks it may be quoted in.
fn item(name: &str) -> String {
    name.trim().trim_matches('`').trim().to_string()
}

/// The entry the trailer value `value` says: `[release-NAME] SECTION: TEXT`, and for `api` the
/// forms `OLD => NEW[; TEXT]` and `removed OLD => NEW[; TEXT]`.
///
/// # Errors
///
/// When it names no section, says nothing, or a moved item or its replacement is left out.
pub(crate) fn parse(value: &str) -> Result<Entry, String> {
    let value = value.trim();
    let (release, rest) = match value.split_once(char::is_whitespace) {
        Some((first, rest)) if first.starts_with(super::tags::PREFIX) => {
            let named = first.trim_end_matches(':');
            if Release::parse(named).is_none() {
                return Err(format!("{named:?} names no release"));
            }
            (Some(named.to_string()), rest.trim())
        }
        _ => (None, value),
    };
    let Some((word, text)) = rest.split_once(':') else {
        return Err(format!(
            "{value:?} names no section: write `{TRAILER}: SECTION: TEXT`, SECTION one of {}",
            Section::words()
        ));
    };
    let Some(section) = Section::parse(word.trim()) else {
        return Err(format!(
            "{:?} is no section: one of {}",
            word.trim(),
            Section::words()
        ));
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("the {} entry says nothing", section.word()));
    }
    let (moved, text) = match text.split_once("=>") {
        Some((old, rest)) if section == Section::Api => {
            let old = old.trim();
            let (removed, old) = match old.strip_prefix("removed") {
                Some(item) if item.is_empty() || item.starts_with(char::is_whitespace) => {
                    (true, item)
                }
                _ => (false, old),
            };
            let (new, more) = rest.split_once(';').unwrap_or((rest, ""));
            let (old, new) = (item(old), item(new));
            if old.is_empty() || new.is_empty() {
                return Err(format!(
                    "{text:?} leaves out an item: write `OLD => NEW` or `removed OLD => NEW`"
                ));
            }
            (Some(Moved { old, new, removed }), more.trim())
        }
        _ => (None, text),
    };
    Ok(Entry {
        release,
        section,
        text: text.to_string(),
        moved,
        after: None,
    })
}

/// Every [`TRAILER`] value of the commits `range` names, oldest first, each with its commit's
/// short name. Git reads the trailers, so a value folded over several lines is one.
pub(crate) fn entries(root: &Path, range: &str) -> Result<Vec<(String, String)>, String> {
    entries_of(root, &[range])
}

/// [`entries`] of the commits `revs` name together, as `git log` reads several.
fn entries_of(root: &Path, revs: &[&str]) -> Result<Vec<(String, String)>, String> {
    let format =
        format!("--format=%h%x01%(trailers:key={TRAILER},valueonly,unfold,separator=%x01)%x02");
    let listed = git(root, &[&["log", "--reverse", &format][..], revs].concat())?;
    let mut found = Vec::new();
    for record in listed.split('\u{2}') {
        let mut fields = record.trim_matches('\n').split('\u{1}');
        let commit = fields.next().unwrap_or_default().trim().to_string();
        for value in fields {
            if !value.trim().is_empty() {
                found.push((commit.clone(), value.trim().to_string()));
            }
        }
    }
    Ok(found)
}

/// The entries the lands a pull request delivered between `before` and `commit` said: a squash
/// keeps no trailer of the commits it squashed, so each squash among the commits of
/// `before..commit` that one of `receipts` names, and git bears out, is read from the head that
/// receipt kept, the commits that head holds beyond the squash's parent, beyond `before` (which
/// a release on a delivered head said already) and beyond each head delivered onto the line
/// before it ([`Receipts::landed_before`]): a head started at another delivered head holds that
/// one's commits, which its own squash said, and no entry is said twice. Oldest squash first. What
/// git bears out is `proof`'s, asked of `receipts` once ([`Receipts::proof`]).
fn delivered(
    root: &Path,
    receipts: &Receipts,
    proof: &Proof,
    before: &str,
    commit: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut found = Vec::new();
    if receipts.is_empty() {
        return Ok(found);
    }
    let listed = git(
        root,
        &["rev-list", "--reverse", &format!("{before}..{commit}")],
    )?;
    for squash in listed.lines() {
        let Some(receipt) = receipts.squashed(squash) else {
            continue;
        };
        if !proof.proved(receipt) {
            continue;
        }
        let mut revs = vec![
            receipt.head.clone(),
            format!("^{}", receipt.parent),
            format!("^{before}"),
        ];
        revs.extend((receipts.landed_before(proof, receipt).iter()).map(|head| format!("^{head}")));
        let revs: Vec<&str> = revs.iter().map(String::as_str).collect();
        found.extend(entries_of(root, &revs)?);
    }
    Ok(found)
}

/// What git says changed between two releases' commits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Derived {
    /// The board schema before and now, each none where its file names none.
    pub(crate) schema: (Option<i64>, Option<i64>),
    pub(crate) pin: (Option<i64>, Option<i64>),
    /// Each tool whose pin changed: its name, the version before and now.
    pub(crate) tools: Vec<(String, String, String)>,
    /// Each package of the workspace whose version changed: its name, the version before and now.
    pub(crate) packages: Vec<(String, String, String)>,
    /// The keys the settings reference lists anew, and those it lists no more, each `[table] key`.
    pub(crate) settings: (Vec<String>, Vec<String>),
    /// The files `symphony init` rewrites that changed, by their path among the shipped ones.
    pub(crate) shipped: Vec<String>,
    /// The consumer-API guards that changed, by their path here.
    pub(crate) guards: Vec<String>,
}

/// `file` as the commit `commit` holds it, or none.
fn held(root: &Path, commit: &str, file: &str) -> Option<String> {
    git(root, &["show", &format!("{commit}:{file}")]).ok()
}

/// The paths `pathspec` names that differ between `before` and `commit`.
fn differing(root: &Path, before: &str, commit: &str, pathspec: &str) -> Vec<String> {
    git(
        root,
        &["diff", "--name-only", before, commit, "--", pathspec],
    )
    .map(|listed| listed.lines().map(str::to_string).collect())
    .unwrap_or_default()
}

/// What changed from the commit `before` (none for the first release) to `commit`.
pub(crate) fn derived(root: &Path, before: Option<&str>, commit: &str) -> Derived {
    let number = |at: &str, file: &str, name: &str| {
        held(root, at, file).and_then(|text| constant(&text, name))
    };
    let pair = |file: &str, name: &str| {
        (
            before.and_then(|before| number(before, file, name)),
            number(commit, file, name),
        )
    };
    let mut derived = Derived {
        schema: pair(SCHEMA_FILE, "VERSION"),
        pin: pair(PIN_FILE, "PIN"),
        ..Derived::default()
    };
    let Some(before) = before else {
        return derived;
    };
    for (name, file, read) in TOOLS {
        let at = |commit: &str| held(root, commit, file).and_then(|text| read(&text));
        if let (Some(was), Some(now)) = (at(before), at(commit))
            && was != now
        {
            derived.tools.push((name.to_string(), was, now));
        }
    }
    let locked = |at: &str| held(root, at, LOCK).map(|text| packages(&text));
    if let (Some(was), Some(now)) = (locked(before), locked(commit)) {
        derived.packages = (now.into_iter())
            .filter_map(|(name, now)| {
                let was = was.get(&name).filter(|was| **was != now)?.clone();
                Some((name, was, now))
            })
            .collect();
    }
    let listed = |at: &str| held(root, at, SETTINGS).map(|text| settings(&text));
    if let (Some(was), Some(now)) = (listed(before), listed(commit)) {
        derived.settings = (
            now.difference(&was).cloned().collect(),
            was.difference(&now).cloned().collect(),
        );
    }
    derived.shipped = differing(root, before, commit, SHIPPED)
        .into_iter()
        .map(|path| path.strip_prefix(SHIPPED).unwrap_or(&path).to_string())
        .collect();
    derived.guards = differing(root, before, commit, GUARDS);
    derived
}

/// One release's notes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Notes {
    pub(crate) release: String,
    /// The commit it tags.
    pub(crate) commit: String,
    /// The release before it, none for the first.
    pub(crate) previous: Option<String>,
    pub(crate) derived: Derived,
    pub(crate) entries: Vec<Entry>,
    /// The entries its commits say of a release cut before it: said here too, for the project
    /// already at or past that release, which reads this one's notes and not that one's again.
    pub(crate) late: Vec<Entry>,
    /// The trailers that read as no entry, each as written: said in the notes all the same.
    pub(crate) unread: Vec<String>,
}

/// The notes of each of `releases` (a name and its commit, oldest first) in the repository at
/// `root`: what git derives between each and the one before, and the entries its commits say, an
/// entry naming another of them moved to that one, with the commit that said it and the release
/// that commit came in, and kept as that release's late entry as well, so a project behind the
/// named release and one already past it each read it.
pub(crate) fn history(root: &Path, releases: &[(String, String)]) -> Result<Vec<Notes>, String> {
    let mut all: Vec<Notes> = Vec::new();
    let mut said: Vec<(usize, String, Entry)> = Vec::new();
    let receipts = Receipts::read(root);
    let proof = receipts.proof(root);
    for (at, (name, commit)) in releases.iter().enumerate() {
        let before = at.checked_sub(1).map(|before| &releases[before]);
        let mut notes = Notes {
            release: name.clone(),
            commit: commit.clone(),
            previous: before.map(|(name, _)| name.clone()),
            derived: derived(root, before.map(|(_, commit)| commit.as_str()), commit),
            ..Notes::default()
        };
        // The first release's entries would be every commit's before it: it says none.
        if let Some((_, before)) = before {
            let mut read = entries(root, &format!("{before}..{commit}"))?;
            read.extend(delivered(root, &receipts, &proof, before, commit)?);
            for (by, value) in read {
                match parse(&value) {
                    Ok(entry) => said.push((at, by, entry)),
                    Err(_) => notes.unread.push(value),
                }
            }
        }
        all.push(notes);
    }
    for (found, by, mut entry) in said {
        let named = (entry.release.as_ref())
            .and_then(|release| all.iter().position(|notes| notes.release == *release))
            .filter(|named| *named != found);
        if named.is_some() {
            entry.after = Some((by, all[found].release.clone()));
            all[found].late.push(entry.clone());
        }
        all[named.unwrap_or(found)].entries.push(entry);
    }
    Ok(all)
}

/// The first of `releases` that says the item `old` is `removed` (or, not `removed`, replaced).
fn says<'a>(releases: &'a [Notes], old: &str, removed: bool) -> Option<&'a Notes> {
    releases.iter().find(|notes| {
        (notes.entries.iter())
            .filter_map(|entry| entry.moved.as_ref())
            .any(|moved| moved.removed == removed && moved.old == old)
    })
}

/// What the entry `entry` of the release at `at` of `history` says, on one line, composed with the
/// releases around it.
fn line(history: &[Notes], at: usize, entry: &Entry) -> String {
    let said = said(history, at, entry);
    match &entry.after {
        Some((commit, release)) => format!(
            "{said} (added after this release was cut, by the commit {commit} that {release} \
             holds: not in its tag's message)"
        ),
        None => said,
    }
}

/// [`line`] without where it was said.
fn said(history: &[Notes], at: usize, entry: &Entry) -> String {
    let more = |text: &str| {
        if text.is_empty() {
            String::new()
        } else {
            format!(" {text}")
        }
    };
    let Some(moved) = &entry.moved else {
        return entry.text.clone();
    };
    let (old, new) = (&moved.old, &moved.new);
    if moved.removed {
        let since = says(&history[..at], old, false)
            .map(|notes| format!(" `{new}` has been there since {}.", notes.release))
            .unwrap_or_default();
        format!(
            "`{old}` is removed: call `{new}`.{since}{}",
            more(&entry.text)
        )
    } else {
        let before = says(&history[at + 1..], old, true)
            .map(|notes| {
                format!(
                    " Move to `{new}` before {}, which removes `{old}`.",
                    notes.release
                )
            })
            .unwrap_or_default();
        format!(
            "`{old}` is replaced by `{new}`; both work in this release.{before}{}",
            more(&entry.text)
        )
    }
}

/// What a board's move from schema `was` to `now` needs of its project
/// (`docs/operations/BOARD_OPERATIONS.md`, "Who writes a board").
fn schema_steps(was: i64, now: i64) -> String {
    format!(
        "Board schema {was} to {now}. A board is moved only by a build at schema {now}: take this \
         release first, so the project's own `tasks` is at {now}. Rebuild at this release every \
         program of the project that writes its board (a `tasks` or `agent-send` it links, a \
         program of its own that opens the board through the board's crate) and run each once, \
         which records it. Then `just tasks migrate` in the project's main checkout moves the \
         board. It first names each writer seen in the last week that is still behind, with its \
         path, and moves nothing (exit 2): that refusal is the one list of the board's writers. \
         Rebuild and run each it names; one that cannot be rebuilt where it was (a build folder \
         that is gone, a path named by a hash) is left behind with `just tasks migrate \
         --leave-behind`, which moves the board all the same, and a writer left behind is refused \
         the board until it is rebuilt. Until the move, a build at schema {now} is refused the \
         board at {was}; after it, a server still on a build at {was} reads the board and its \
         writes are refused until it runs a build at {now}."
    )
}

/// The notes of the release at `at` of `history`, a heading and its lines for each section that
/// says something: what a project does to move to it from the release before.
pub(crate) fn section(history: &[Notes], at: usize) -> Vec<String> {
    let notes = &history[at];
    let derived = &notes.derived;
    let mut lines = Vec::new();
    for section in Section::ALL {
        let mut said: Vec<String> = Vec::new();
        match section {
            Section::Manifest if notes.previous.is_some() => {
                said.push(format!(
                    "Name this release in each of the project's Symphony dependency lines, `tag \
                     = \"{}\"`, or `rev = \"{}\"` where the project pins a commit, and let \
                     Cargo.lock follow: a build run with `--locked` refuses until the lock is \
                     updated.",
                    notes.release, notes.commit
                ));
                if !derived.packages.is_empty() {
                    let moved: Vec<String> = (derived.packages.iter())
                        .map(|(name, was, now)| format!("{name} {was} to {now}"))
                        .collect();
                    said.push(format!(
                        "Versions that move in Cargo.lock: {}.",
                        moved.join(", ")
                    ));
                }
            }
            Section::Board => {
                if let (Some(was), Some(now)) = derived.schema
                    && was != now
                {
                    said.push(schema_steps(was, now));
                }
                if let (Some(was), Some(now)) = derived.pin
                    && was != now
                {
                    said.push(format!(
                        "Pin {was} to {now}: a host serves a project registered at its pin or an \
                         older one and refuses a newer one, so the machine's server takes this \
                         release before a project registers at it."
                    ));
                }
            }
            Section::Settings => {
                let (added, removed) = &derived.settings;
                let keys = |keys: &[String]| {
                    let keys: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
                    keys.join(", ")
                };
                if !added.is_empty() {
                    said.push(format!(
                        "Keys read anew (docs/operations/PROJECT_FILE.md says each): {}.",
                        keys(added)
                    ));
                }
                if !removed.is_empty() {
                    said.push(format!(
                        "Keys no longer read, which `just symphony-cli settings-check` refuses \
                         as unknown: {}.",
                        keys(removed)
                    ));
                }
            }
            Section::Init if !derived.shipped.is_empty() => said.push(format!(
                "`just symphony-cli init --root .` in the project rewrites: {}.",
                derived.shipped.join(", ")
            )),
            Section::Tools => {
                for (name, was, now) in &derived.tools {
                    said.push(format!("{name} {was} to {now}."));
                }
            }
            _ => {}
        }
        said.extend(
            (notes.entries.iter())
                .filter(|entry| entry.section == section && !entry.nothing())
                .map(|entry| line(history, at, entry)),
        );
        if section == Section::Api && !derived.guards.is_empty() {
            said.push(format!(
                "The calls a consumer's code is held to changed in: {}.",
                derived.guards.join(", ")
            ));
        }
        if !said.is_empty() {
            lines.push(format!("{}:", section.title()));
            lines.extend(said.into_iter().map(|said| format!("- {said}")));
        }
    }
    let late: Vec<String> = (notes.late.iter())
        .filter(|entry| !entry.nothing())
        .filter_map(|entry| {
            let of = entry.release.as_ref()?;
            let named = history.iter().position(|notes| notes.release == *of)?;
            Some(format!(
                "- Of {of}, {}: {}",
                entry.section.title(),
                said(history, named, entry)
            ))
        })
        .collect();
    if !late.is_empty() {
        lines.push(
            "Said with this release of an earlier one, for a project already at or past it:".into(),
        );
        lines.extend(late);
    }
    if !notes.unread.is_empty() {
        lines.push("Said without a section:".into());
        lines.extend(notes.unread.iter().map(|said| format!("- {said}")));
    }
    // The line that names the release is every release's: alone, it is the whole move.
    match (notes.previous.is_some(), lines.len()) {
        (false, 0) => lines.push("Nothing a project changes to take it.".into()),
        (true, 2) => lines.push("Nothing else changes for a project.".into()),
        _ => {}
    }
    lines
}

/// The line a tag's message says before a release's [`section`].
pub(crate) const TAKING: &str = "For a project taking this release (`just symphony-cli \
                                 release-notes` prints every release's since the project's own):";

/// What the line under a release's heading says its commit after, which the notes' reader maps a
/// project's pinned commit by (`symphony::machine::release::notes_between`).
const COMMIT: &str = "Commit: ";

/// Every release's notes, oldest first, each under a `## NAME` heading with its commit: the `notes.md` an
/// installed release holds, which `symphony release-notes` prints a part of.
pub(crate) fn document(history: &[Notes]) -> String {
    let mut text = String::from(
        "# What a project changes to take each Symphony release\n\nOldest first. Each release \
         says what a project does to move to it from the release before; a project several \
         releases behind does each in order.\n",
    );
    for (at, notes) in history.iter().enumerate() {
        text.push_str(&format!("\n## {}\n\n", notes.release));
        text.push_str(&format!("{COMMIT}{}\n", notes.commit));
        text.push_str(&match &notes.previous {
            Some(previous) => format!("From {previous}.\n\n"),
            None => "The first release.\n\n".into(),
        });
        for line in section(history, at) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    text
}

/// The releases the repository at `root` tags that `commit` holds, oldest first, each with its
/// commit: those that tag it or an ancestor of it, and those tagged on a head a pull request
/// delivered whose squash it holds ([`Receipts::holds`]), a release candidate's release, which
/// no history of the default branch holds and every release after it is still after.
pub(crate) fn releases(root: &Path, commit: &str) -> Result<Vec<(String, String)>, String> {
    let pattern = format!("{}*", super::tags::PREFIX);
    let tagged = |merged: &[&str]| -> Result<Vec<(Release, String)>, String> {
        let format = "--format=%(refname:short) %(*objectname)%(if)%(*objectname)%(then)%(else)%(objectname)%(end)";
        let listed = git(
            root,
            &[&["tag", "-l", &pattern][..], merged, &[format]].concat(),
        )?;
        Ok(listed
            .lines()
            .filter_map(|line| {
                let (name, commit) = line.split_once(' ')?;
                Some((Release::parse(name)?, commit.trim().to_string()))
            })
            .collect())
    };
    let mut found = tagged(&["--merged", commit])?;
    let receipts = Receipts::read(root);
    if !receipts.is_empty() {
        for (release, at) in tagged(&[])? {
            if found.iter().all(|(known, _)| *known != release)
                && receipts.holds(root, &at, commit).unwrap_or(false)
            {
                found.push((release, at));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found
        .into_iter()
        .map(|(release, commit)| (release.name(), commit))
        .collect())
}

/// [`document`] for the release `tag` of `commit` in the repository at `root`, with every release
/// before it: what its install writes. `tag` need not be a tag yet.
pub(crate) fn document_at(root: &Path, tag: &str, commit: &str) -> Result<String, String> {
    Ok(document(&history(root, &through(root, tag, commit)?)?))
}

/// The releases before `tag` in the repository at `root`, then `tag` at `commit`.
fn through(root: &Path, tag: &str, commit: &str) -> Result<Vec<(String, String)>, String> {
    let mut all = releases(root, commit)?;
    all.retain(|(name, _)| name != tag);
    all.push((tag.to_string(), commit.to_string()));
    Ok(all)
}

/// The lines the message of the tag `tag` on `commit` says for a project taking it: [`TAKING`],
/// then its [`section`], composed with the releases before it. None for the first release.
pub(crate) fn taking(root: &Path, tag: &str, commit: &str) -> Result<Vec<String>, String> {
    let history = history(root, &through(root, tag, commit)?)?;
    // The first release is moved to from none.
    if history.len() < 2 {
        return Ok(Vec::new());
    }
    let mut lines = vec![TAKING.to_string()];
    lines.extend(section(&history, history.len() - 1));
    Ok(lines)
}

/// What a branch changed that owes a consumer an entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Changed {
    /// The board schema at the branch's base and at its tip, when they differ.
    pub(crate) schema: Option<(i64, i64)>,
    /// Each call a guard lost ([`lost`]): the guard's path and the line.
    pub(crate) lost: Vec<(String, String)>,
    /// Each key the settings reference lists no more, `[table] key`.
    pub(crate) keys: Vec<String>,
}

/// What `changed` owes that none of `entries` (each trailer of the branch, read) says, and each
/// trailer that reads as no entry: one line each, none when the branch owes nothing. An entry said
/// of a release already cut answers for that release, never for what this branch changed.
pub(crate) fn owed(changed: &Changed, entries: &[Result<Entry, String>]) -> Vec<String> {
    let mut owed: Vec<String> = (entries.iter())
        .filter_map(|entry| entry.as_ref().err())
        .map(|why| format!("a {TRAILER} trailer of this branch reads as no entry: {why}"))
        .collect();
    let says = |section: Section| {
        (entries.iter().flatten()).any(|entry| entry.section == section && entry.release.is_none())
    };
    let add = "and no commit of this branch says so: add a trailer to a commit's message,";
    if let Some((was, now)) = changed.schema
        && !says(Section::Board)
    {
        owed.push(format!(
            "the board schema moves from {was} to {now}, which a project's board must follow, \
             {add} `{TRAILER}: board: TEXT`, what the move changes for a project"
        ));
    }
    if !says(Section::Api) {
        for (file, lost) in &changed.lost {
            owed.push(format!(
                "{file} no longer holds a call a consumer makes (`{lost}`), so a consumer calls \
                 something else, {add} `{TRAILER}: api: removed OLD => NEW`, or `{TRAILER}: api: \
                 {NONE}: WHY` where no consumer changes"
            ));
        }
    }
    if !changed.keys.is_empty() && !says(Section::Settings) {
        owed.push(format!(
            "{SETTINGS} no longer lists {}, which a project's symphony.toml may hold, {add} \
             `{TRAILER}: settings: TEXT`, what a project writes instead",
            changed.keys.join(", ")
        ));
    }
    owed
}

/// A guard's line without the comment that ends it, trimmed. None for a comment or a blank.
fn uncommented(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("//") {
        return None;
    }
    Some(
        line.split_once(" //")
            .map_or(line, |(code, _)| code.trim_end()),
    )
}

/// A guard's line as the check compares it: without the comment that ends it, its spaces and its
/// commas, so a call that `rustfmt` wraps anew, or that moves, is the same call. None for a
/// comment or a blank.
fn code(line: &str) -> Option<String> {
    uncommented(line).map(|code| {
        (code.chars())
            .filter(|c| !c.is_whitespace() && *c != ',')
            .collect()
    })
}

/// What a guard holds, in its order: each path a `use` declaration names, and each other line.
#[derive(Debug, PartialEq, Eq)]
enum Held<'a> {
    Path(String),
    Line(&'a str),
}

/// A guard read as [`Held`]: a `use` declaration, over as many lines as `rustfmt` wraps it on, is
/// the full path of each name its tree imports ([`paths`]), not a line. One that never closes (no
/// line of it, to the end of the guard, ends in `;` once its comment is cut) is its lines as
/// written, each a [`Held::Line`], so a call among them is still compared.
fn holds(guard: &str) -> Vec<Held<'_>> {
    let mut held = Vec::new();
    // The tree read so far, and its lines as written.
    let mut tree: Option<(String, Vec<&str>)> = None;
    for line in guard.lines() {
        let code = uncommented(line);
        if let Some((tree, lines)) = tree.as_mut() {
            lines.push(line);
            if let Some(code) = code {
                tree.push(' ');
                tree.push_str(code);
            }
        } else if let Some(rest) = code.and_then(|code| code.strip_prefix("use ")) {
            tree = Some((rest.to_string(), vec![line]));
        } else {
            held.push(Held::Line(line));
        }
        if let Some(whole) = tree.as_ref().and_then(|(tree, _)| tree.strip_suffix(';')) {
            let mut named = Vec::new();
            paths("", whole, &mut named);
            held.extend(named.into_iter().map(Held::Path));
            tree = None;
        }
    }
    if let Some((_, lines)) = tree {
        held.extend(lines.into_iter().map(Held::Line));
    }
    held
}

/// Each full path the `use` tree `tree` names under `prefix`, into `named`: `a::{b, c::{d}}` is
/// `a::b` and `a::c::d`, `a::{self}` is `a`, and a rename keeps its `as`.
fn paths(prefix: &str, tree: &str, named: &mut Vec<String>) {
    let tree = tree.trim();
    let Some((head, list)) = tree
        .split_once('{')
        .and_then(|(head, rest)| Some((head.trim(), rest.strip_suffix('}')?)))
    else {
        let leaf = tree.split_whitespace().collect::<Vec<_>>().join(" ");
        if leaf == "self" {
            named.push(prefix.trim_end_matches("::").to_string());
        } else if !leaf.is_empty() {
            named.push(format!("{prefix}{leaf}"));
        }
        return;
    };
    let prefix = format!("{prefix}{head}");
    let mut depth = 0_u32;
    let mut start = 0;
    for (at, c) in list.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                paths(&prefix, &list[start..at], named);
                start = at + 1;
            }
            _ => {}
        }
    }
    paths(&prefix, &list[start..], named);
}

/// What of the guard `before` the guard `now` holds nowhere, each as written: a call a consumer
/// makes that the guard stopped holding it to. A line that only moved, was wrapped anew or gained
/// a comment is not one, neither is a call added beside the old one, and a line of brackets alone
/// names no call. A `use` declaration holds no call, only paths: each path it named that no
/// declaration of `now` names is one, said `use PATH;`, and a name added to its list is not.
pub(crate) fn lost(before: &str, now: &str) -> Vec<String> {
    let now = holds(now);
    let imported: std::collections::BTreeSet<&str> = (now.iter())
        .filter_map(|held| match held {
            Held::Path(path) => Some(path.as_str()),
            Held::Line(_) => None,
        })
        .collect();
    let calls: String = (now.iter())
        .filter_map(|held| match held {
            Held::Line(line) => code(line),
            Held::Path(_) => None,
        })
        .collect();
    (holds(before).into_iter())
        .filter_map(|held| match held {
            Held::Path(path) => (!imported.contains(path.as_str())).then(|| format!("use {path};")),
            Held::Line(line) => code(line)
                .is_some_and(|call| {
                    call.chars().any(char::is_alphanumeric) && !calls.contains(&call)
                })
                .then(|| line.trim().to_string()),
        })
        .collect()
}

/// What the branch checked out at `root` owes a consumer and does not say, against the default
/// branch `default`: [`owed`] of what it changed since it left `default` (the commits of
/// `merge-base..HEAD`, which are the branch's own however often it merged `default`, and the
/// difference between those two commits). None on `default`'s own line, or where the checkout has
/// no such branch, which is said.
///
/// # Errors
///
/// When git cannot read the checkout.
pub(crate) fn check(root: &Path, default: &str) -> Result<Checked, String> {
    let Ok(base) = git(root, &["merge-base", "HEAD", default]) else {
        return Ok(Checked::NoBase);
    };
    let schema = |at: &str| held(root, at, SCHEMA_FILE).and_then(|text| constant(&text, "VERSION"));
    let listed = |at: &str| held(root, at, SETTINGS).map(|text| settings(&text));
    let mut changed = Changed {
        schema: match (schema(&base), schema("HEAD")) {
            (Some(was), Some(now)) if was != now => Some((was, now)),
            _ => None,
        },
        keys: match (listed(&base), listed("HEAD")) {
            (Some(was), Some(now)) => was.difference(&now).cloned().collect(),
            _ => Vec::new(),
        },
        ..Changed::default()
    };
    for file in differing(root, &base, "HEAD", GUARDS) {
        if let Some(before) = held(root, &base, &file) {
            let now = held(root, "HEAD", &file).unwrap_or_default();
            (changed.lost).extend(
                lost(&before, &now)
                    .into_iter()
                    .map(|line| (file.clone(), line)),
            );
        }
    }
    let tagged = |release: &str| {
        git(root, &["tag", "-l", release]).is_ok_and(|listed| listed.trim() == release)
    };
    let entries: Vec<Result<Entry, String>> = entries(root, &format!("{base}..HEAD"))?
        .iter()
        .map(|(commit, value)| {
            let entry = parse(value).map_err(|why| format!("{commit}: {why}"))?;
            match &entry.release {
                Some(release) if !tagged(release) => Err(format!(
                    "{commit}: it is said of {release}, which is no release tagged here"
                )),
                _ => Ok(entry),
            }
        })
        .collect();
    Ok(Checked::Owed(owed(&changed, &entries)))
}

/// What [`check`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Checked {
    /// The checkout has no default branch to compare with.
    NoBase,
    /// What the branch owes and does not say, none when it is in order.
    Owed(Vec<String>),
}

/// What `symphony-build consumer-notes` heads the commits since the newest release with: no
/// release's name, so nothing reads it as one.
const UNRELEASED: &str = "unreleased";

/// The usage of `symphony-build consumer-notes`.
pub(crate) const USAGE: &str = "usage: symphony-build consumer-notes [--check] [ROOT]";

/// `symphony-build consumer-notes [--check] [ROOT]`: the notes of every release ROOT's repository
/// tags, as the next install would write them ([`document`]), the commits since the newest under
/// [`UNRELEASED`]; with `--check`, whether the branch
/// checked out there says what it owes a consumer ([`check`]), exit 1 when it does not. Its exit
/// code.
pub(crate) fn main(
    args: &[String],
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let mut checking = false;
    let mut root = None;
    for arg in args {
        match arg.as_str() {
            "--check" => checking = true,
            flag if flag.starts_with('-') => {
                let _ = writeln!(
                    err,
                    "{USAGE}\nsymphony-build consumer-notes: unknown {flag}"
                );
                return 2;
            }
            path if root.is_none() => root = Some(std::path::PathBuf::from(path)),
            more => {
                let _ = writeln!(
                    err,
                    "{USAGE}\nsymphony-build consumer-notes: unexpected {more}"
                );
                return 2;
            }
        }
    }
    let root = root.unwrap_or_else(|| std::path::PathBuf::from("."));
    if !checking {
        let made = git(&root, &["rev-parse", "HEAD"]).and_then(|head| {
            let mut releases = releases(&root, &head)?;
            // What the commits since the newest release would say in the next one's notes.
            if releases.last().is_none_or(|(_, commit)| *commit != head) {
                releases.push((UNRELEASED.to_string(), head));
            }
            Ok(document(&history(&root, &releases)?))
        });
        return match made {
            Ok(text) => {
                let _ = write!(out, "{text}");
                0
            }
            Err(why) => {
                let _ = writeln!(err, "symphony-build consumer-notes: {why}");
                2
            }
        };
    }
    let default = symphony::project::Project::load(&root).map_or_else(
        |_| "main".to_string(),
        |project| project.default_branch.clone(),
    );
    match check(&root, &default) {
        Ok(Checked::NoBase) => {
            let _ = writeln!(
                out,
                "consumer-notes: this checkout has no {default} to compare with: nothing checked"
            );
            0
        }
        Ok(Checked::Owed(owed)) if owed.is_empty() => 0,
        Ok(Checked::Owed(owed)) => {
            for line in &owed {
                let _ = writeln!(err, "consumer-notes: {line} ({WHERE})");
            }
            1
        }
        Err(why) => {
            let _ = writeln!(err, "symphony-build consumer-notes: {why}");
            2
        }
    }
}

#[cfg(test)]
#[path = "consumer_tests.rs"]
mod tests;
