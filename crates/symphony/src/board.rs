//! The board over every document under `docs/`: `just symphony-status`, its `--json`, and `--tidy`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;

use crate::callouts::{Comment, archive_callouts, ledger_entry, parse_comments};
use crate::clock::now_stamp;
use crate::register::{
    DOCS_DIR, Decision, Group, REGION_START, ReviewError, SKIP_DIRS, State, check_nothing_lost,
    check_nothing_repeated, parse_decisions, resolve, supersede,
};
use crate::text::{FileStamp, os_error, path_str, read_text, splitlines, strip, write_whole};

/// Where `--tidy` moves the answered callouts, under `docs/`.
pub const LEDGER: &str = "history/review-ledger.md";
const LEDGER_HEAD: &str = "# Review ledger\n\n\
    Comments left on documents in the Symphony, moved here word for word by\n\
    `just symphony-status --tidy` once an agent answered them, so the pages keep only\n\
    what is still open. Newest last.\n";

/// One document's callouts and cards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    /// Relative to the root, `docs/…`.
    pub path: String,
    pub comments: Vec<Comment>,
    pub decisions: Vec<Decision>,
}

/// The markdown files under `dir` in the order Python's `os.walk` gives them with its folders sorted:
/// a folder's own files (sorted), then each folder under it. Dot folders and `history/` are skipped,
/// and so is a link to a folder; a folder that cannot be listed is passed over.
pub(crate) fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            let linked = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            if !linked && !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                dirs.push((name, path));
            }
        } else if name.ends_with(".md") {
            files.push((name, path));
        }
    }
    files.sort();
    dirs.sort();
    let mut found: Vec<PathBuf> = files.into_iter().map(|(_, path)| path).collect();
    for (_, path) in dirs {
        found.extend(markdown_files(&path));
    }
    found
}

/// `path` relative to `root`, with forward slashes.
pub(crate) fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every markdown file under `<root>/docs`, sorted by path, history excluded.
pub fn scan(root: &Path) -> Result<Vec<Doc>, ReviewError> {
    static PARSED: LazyLock<Mutex<BTreeMap<PathBuf, (FileStamp, Doc)>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    let docs_root = root.join(DOCS_DIR);
    if !docs_root.is_dir() {
        return Err(ReviewError(format!(
            "no {DOCS_DIR}/ directory under {}",
            root.display()
        )));
    }
    let mut docs = Vec::new();
    for path in markdown_files(&docs_root) {
        let key = path
            .canonicalize()
            .map_err(|e| ReviewError(os_error(&e, &path)))?;
        let stamp = FileStamp::read(&path).map_err(|e| ReviewError(os_error(&e, &path)))?;
        {
            let parsed = PARSED.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((before, doc)) = parsed.get(&key)
                && *before == stamp
            {
                let mut doc = doc.clone();
                doc.path = relative(&path, root);
                docs.push(doc);
                continue;
            }
        }
        let text = read_text(&path).map_err(|e| ReviewError(os_error(&e, &path)))?;
        let lines = splitlines(&text);
        let doc = Doc {
            path: relative(&path, root),
            comments: parse_comments(&lines),
            decisions: parse_decisions(&lines),
        };
        if FileStamp::read(&path).ok().as_ref() == Some(&stamp) {
            let mut parsed = PARSED.lock().unwrap_or_else(|e| e.into_inner());
            if parsed.len() >= 2048 && !parsed.contains_key(&key) {
                parsed.clear();
            }
            parsed.insert(key, (stamp, doc.clone()));
        }
        docs.push(doc);
    }
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(docs)
}

/// Open comments and open decisions, in file order, one block each.
fn open_comment_lines(docs: &[Doc]) -> Vec<String> {
    let mut lines = Vec::new();
    for doc in docs {
        let mut items: Vec<(usize, String, String, Vec<String>)> = Vec::new();
        for comment in doc.comments.iter().filter(|c| c.open()) {
            items.push((
                comment.line,
                comment.kind.clone(),
                comment.title.clone(),
                comment.body.clone(),
            ));
        }
        for decision in doc.decisions.iter().filter(|d| d.open()) {
            let unanswered: Vec<String> = decision
                .unanswered()
                .iter()
                .flat_map(|text| {
                    splitlines(text)
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect();
            let body = if unanswered.is_empty() {
                decision.note.clone()
            } else {
                unanswered
            };
            let title = format!("**{}** {}", decision.slug, decision.text);
            items.push((
                decision.line,
                decision.state.as_str().to_string(),
                title,
                body,
            ));
        }
        items.sort();
        for (line, kind, title, body) in items {
            let head = format!("{}:{line}  {kind:<8} {title}", doc.path);
            lines.push(crate::text::rstrip(&head).to_string());
            lines.extend(
                body.iter()
                    .filter(|text| !strip(text).is_empty())
                    .map(|text| format!("    {text}")),
            );
        }
    }
    lines
}

/// How many comments and cards wait on an agent.
pub fn open_count(docs: &[Doc]) -> usize {
    docs.iter()
        .map(|doc| {
            doc.comments.iter().filter(|c| c.open()).count()
                + doc.decisions.iter().filter(|d| d.open()).count()
        })
        .sum()
}

/// The board as text: the decision counts and every open comment, or only the open ones.
pub fn render(docs: &[Doc], open_only: bool) -> String {
    let open_lines = open_comment_lines(docs);
    if open_only {
        return if open_lines.is_empty() {
            "no open comments".to_string()
        } else {
            open_lines.join("\n")
        };
    }
    let mut out: Vec<String> = Vec::new();
    let decisions: Vec<&Decision> = docs.iter().flat_map(|doc| &doc.decisions).collect();
    if !decisions.is_empty() {
        out.push(String::new());
        out.push(format!("DECISIONS ({})", decisions.len()));
        for state in State::ORDER {
            let count = decisions.iter().filter(|d| d.state == state).count();
            out.push(format!("  {:<11} {count:>3}", state.as_str()));
        }
        out.push("  by where they sit:".to_string());
        let groups: Vec<Group> = decisions.iter().map(|d| d.group()).collect();
        for group in Group::ALL {
            let count = groups.iter().filter(|g| **g == group).count();
            out.push(format!("  {:<21} {count:>3}", group.title().to_lowercase()));
        }
    }
    let comments = docs.iter().flat_map(|doc| &doc.comments);
    let likes = comments.clone().filter(|c| c.kind == "like").count();
    let closed = comments.filter(|c| c.kind != "like" && !c.open()).count();
    out.push(String::new());
    out.push(format!(
        "OPEN COMMENTS ({}; {closed} resolved, {likes} likes)",
        open_lines.len()
    ));
    out.extend(open_lines.iter().map(|line| format!("  {line}")));
    out.join("\n").trim_start_matches('\n').to_string()
}

#[derive(Serialize)]
struct JsonEntry {
    ts: Option<String>,
    who: &'static str,
    verdict: Option<String>,
    text: String,
}

#[derive(Serialize)]
struct JsonDecision<'a> {
    line: usize,
    slug: &'a str,
    state: &'static str,
    text: &'a str,
    note: &'a [String],
    open: bool,
    group: &'static str,
    topic: &'a str,
    decided_at: Option<String>,
    entries: Vec<JsonEntry>,
}

impl<'a> JsonDecision<'a> {
    fn of(d: &'a Decision) -> JsonDecision<'a> {
        let thread = d.thread();
        JsonDecision {
            line: d.line,
            slug: &d.slug,
            state: d.state.as_str(),
            text: &d.text,
            note: &d.note,
            open: thread.open,
            group: thread.group.key(),
            topic: &d.topic,
            decided_at: thread.decided_at,
            entries: thread
                .entries
                .into_iter()
                .map(|e| JsonEntry {
                    ts: e.ts,
                    who: e.who.as_str(),
                    verdict: e.verdict,
                    text: e.text,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonComment<'a> {
    line: usize,
    kind: &'a str,
    title: &'a str,
    body: &'a [String],
    resolution: Option<&'a str>,
    open: bool,
}

#[derive(Serialize)]
struct JsonDoc<'a> {
    path: &'a str,
    decisions: Vec<JsonDecision<'a>>,
    comments: Vec<JsonComment<'a>>,
}

#[derive(Serialize)]
struct JsonBoard<'a> {
    docs: Vec<JsonDoc<'a>>,
    open: usize,
}

/// The board as `--json` prints it: `json.dumps(…, indent=2, ensure_ascii=False)`, byte for byte.
pub fn to_json(docs: &[Doc]) -> String {
    let board = JsonBoard {
        docs: docs
            .iter()
            .map(|doc| JsonDoc {
                path: &doc.path,
                decisions: doc.decisions.iter().map(JsonDecision::of).collect(),
                comments: doc
                    .comments
                    .iter()
                    .map(|c| JsonComment {
                        line: c.line,
                        kind: &c.kind,
                        title: &c.title,
                        body: &c.body,
                        resolution: c.resolution.as_deref(),
                        open: c.open(),
                    })
                    .collect(),
            })
            .collect(),
        open: open_count(docs),
    };
    serde_json::to_string_pretty(&board).expect("the board serialises")
}

/// Every markdown file under `<root>/docs` holding a register region, DECISIONS.md first.
pub fn register_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = markdown_files(&root.join(DOCS_DIR))
        .into_iter()
        .filter(|path| read_text(path).is_ok_and(|text| splitlines(&text).contains(&REGION_START)))
        .collect();
    found.sort_by_key(|path| {
        (
            path.file_name().is_none_or(|name| name != "DECISIONS.md"),
            path.to_string_lossy().to_string(),
        )
    });
    found
}

/// `--tidy`: supersede, resolve and regroup every register, then move answered callouts to the
/// ledger. Its exit code.
pub fn tidy(root: &Path, now: Option<&str>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    tidy_with(root, now, crate::register::sort_register, out, err)
}

/// One register tidied: its new text, the slugs resolved and those superseded.
fn tidy_text(
    path: &Path,
    stamp: &str,
    sort: fn(&str) -> Result<String, ReviewError>,
) -> Result<(String, Vec<String>, Vec<String>), ReviewError> {
    let original = read_text(path).map_err(|e| ReviewError(os_error(&e, path)))?;
    let (text, superseded) = supersede(&original, stamp);
    let (text, changed) = resolve(&text, stamp);
    let text = sort(&text)?;
    check_nothing_lost(&original, &text, "tidying")?;
    check_nothing_repeated(&original, &text, "tidying")?;
    Ok((text, changed, superseded))
}

/// [`tidy`] with the regroup given, so a test can hand it one that drops a line.
pub fn tidy_with(
    root: &Path,
    now: Option<&str>,
    sort: fn(&str) -> Result<String, ReviewError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if !root.join(DOCS_DIR).is_dir() {
        let _ = writeln!(
            err,
            "✖ no {DOCS_DIR}/ directory under {}",
            path_str(&root.to_string_lossy())
        );
        return 2;
    }
    let stamp = now
        .filter(|now| !now.is_empty())
        .map_or_else(now_stamp, str::to_string);
    for path in register_files(root) {
        let rel = relative(&path, root);
        let written = tidy_text(&path, &stamp, sort).and_then(|tidied| {
            write_whole(&path, &tidied.0)
                .map(|()| tidied)
                .map_err(|e| ReviewError(os_error(&e, &path)))
        });
        let (_, changed, superseded) = match written {
            Ok(tidied) => tidied,
            Err(problem) => {
                let _ = writeln!(err, "✖ {rel} not tidied: {problem}");
                return 1;
            }
        };
        let _ = writeln!(
            out,
            "resolved: {}",
            if changed.is_empty() {
                "(none)".to_string()
            } else {
                changed.join(", ")
            }
        );
        if !superseded.is_empty() {
            let _ = writeln!(out, "superseded: {}", superseded.join(", "));
        }
        let _ = writeln!(out, "regrouped {rel}");
    }
    match archive(root, &stamp, out) {
        Ok(()) => 0,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            1
        }
    }
}

/// Move every answered callout under `docs/` to the ledger, saying which files gave some.
fn archive(root: &Path, stamp: &str, out: &mut dyn Write) -> Result<(), ReviewError> {
    let docs_root = root.join(DOCS_DIR);
    let fail = |e: std::io::Error, path: &Path| ReviewError(os_error(&e, path));
    let mut entries: Vec<String> = Vec::new();
    for path in markdown_files(&docs_root) {
        let (text, taken) = archive_callouts(&read_text(&path).map_err(|e| fail(e, &path))?);
        if taken.is_empty() {
            continue;
        }
        let rel = relative(&path, root);
        entries.extend(
            taken
                .iter()
                .map(|comment| ledger_entry(&rel, comment, stamp)),
        );
        write_whole(&path, &text).map_err(|e| fail(e, &path))?;
        let _ = writeln!(out, "archived: {rel} ({})", taken.len());
    }
    if entries.is_empty() {
        return Ok(());
    }
    let ledger = docs_root.join(LEDGER);
    if let Some(folder) = ledger.parent() {
        std::fs::create_dir_all(folder).map_err(|e| fail(e, folder))?;
    }
    let old = if ledger.exists() {
        read_text(&ledger).map_err(|e| fail(e, &ledger))?
    } else {
        LEDGER_HEAD.to_string()
    };
    let new = format!("{}\n\n{}", old.trim_end_matches('\n'), entries.join("\n"));
    write_whole(&ledger, &new).map_err(|e| fail(e, &ledger))
}
