//! The board over every document of the project, the Markdown its entrypoints show outside their
//! history folders: `just symphony-status`, its `--json`, and `--tidy`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;

use crate::callouts::{Comment, archive_callouts, ledger_entry, parse_comments};
use crate::clock::now_stamp;
use crate::plugins::Plugins;
use crate::project::Project;
use crate::register::{
    Decision, Group, REGION_START, ReviewError, State, check_nothing_lost, check_nothing_repeated,
    parse_decisions, resolve, supersede,
};
use crate::text::{FileStamp, os_error, path_str, read_text, splitlines, strip, write_whole};

/// The ledger `--tidy` moves the answered callouts to, in the project's first history folder.
pub const LEDGER: &str = "review-ledger.md";
const LEDGER_HEAD: &str = "# Review ledger\n\n\
    Comments left on documents in the Symphony, moved here word for word by\n\
    `just symphony-status --tidy` once an agent answered them, so the pages keep only\n\
    what is still open. Newest last.\n";

/// One document's callouts and cards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc {
    /// The document's route (`docs/…` by the convention).
    pub path: String,
    pub comments: Vec<Comment>,
    pub decisions: Vec<Decision>,
}

/// The project's documents in the checkout at `root`: the Markdown each entrypoint shows outside
/// its history folders, entrypoint by entrypoint in the order [`Project::walk`] gives them.
pub(crate) fn document_files(root: &Path, project: &Project) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in &project.entrypoints {
        project.walk(root, entry, "", &mut |inside| {
            if entry.document(inside) {
                found.push(root.join(entry.file(inside)));
            }
        });
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

/// Every document of the project at `root`, named by its route (`docs/…`) and sorted by it; a
/// checkout that holds none of its entrypoints' folders is refused, naming the first.
pub fn scan(root: &Path) -> Result<Vec<Doc>, ReviewError> {
    if let Some(folder) = Project::of(root).missing_documents(root) {
        return Err(ReviewError(format!(
            "no {folder}/ directory under {}",
            root.display()
        )));
    }
    documents(root)
}

/// Every document of the project at `root`, as [`scan`] reads them, and none in a checkout that
/// holds none of its entrypoints' folders: the server's read, which serves a project's board and
/// agents before it has documents.
pub fn documents(root: &Path) -> Result<Vec<Doc>, ReviewError> {
    static PARSED: LazyLock<Mutex<BTreeMap<PathBuf, (FileStamp, Doc)>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    let project = Project::of(root);
    let mut docs = Vec::new();
    // A document is named by its route, its file read where the project keeps it.
    let named = |path: &Path| {
        let file = relative(path, root);
        project.route(&file).unwrap_or(file)
    };
    for path in document_files(root, &project) {
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
                doc.path = named(&path);
                docs.push(doc);
                continue;
            }
        }
        let text = read_text(&path).map_err(|e| ReviewError(os_error(&e, &path)))?;
        let lines = splitlines(&text);
        let doc = Doc {
            path: named(&path),
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
    symphony_board::order::sort_by(&mut docs, &|a, b| a.path.cmp(&b.path));
    Ok(docs)
}

/// [`documents`], each document an owner's save still waits on read as that save leaves it
/// ([`crate::commit::pending::shown`]): what the server's dashboard counts, so a card the owner
/// answered in a project that delivers by pull request does not wait on the owner again until
/// its pull request lands.
pub fn documents_shown(root: &Path) -> Result<Vec<Doc>, ReviewError> {
    let mut docs = documents(root)?;
    let saves = crate::commit::pending::saves(root);
    if saves.is_empty() {
        return Ok(docs);
    }
    let project = Project::of(root);
    let mut read = std::collections::BTreeSet::new();
    for save in &saves {
        if !read.insert(&save.file) {
            continue;
        }
        let route = project.route(&save.file);
        let Some(doc) = (docs.iter_mut()).find(|doc| Some(&doc.path) == route.as_ref()) else {
            continue;
        };
        let Ok(text) = read_text(&root.join(&save.file)) else {
            continue;
        };
        let text = crate::commit::pending::shown(root, &save.file, text);
        let lines = splitlines(&text);
        doc.comments = parse_comments(&lines);
        doc.decisions = parse_decisions(&lines);
    }
    Ok(docs)
}

/// Open comments and open decisions, in file order, one block each.
fn open_comment_lines(plugins: &Plugins, docs: &[Doc]) -> Vec<String> {
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
        for decision in doc.decisions.iter().filter(|d| d.open(plugins)) {
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

/// How many comments and cards wait on an agent, the cards read by the project's card kinds.
pub fn open_count(plugins: &Plugins, docs: &[Doc]) -> usize {
    docs.iter()
        .map(|doc| {
            doc.comments.iter().filter(|c| c.open()).count()
                + doc.decisions.iter().filter(|d| d.open(plugins)).count()
        })
        .sum()
}

/// The board as text: the decision counts and every open comment, or only the open ones.
pub fn render(plugins: &Plugins, docs: &[Doc], open_only: bool) -> String {
    let open_lines = open_comment_lines(plugins, docs);
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
        let groups: Vec<Group> = decisions.iter().map(|d| d.group(plugins)).collect();
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
    fn of(d: &'a Decision, plugins: &Plugins) -> JsonDecision<'a> {
        let thread = d.thread(plugins);
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
pub fn to_json(plugins: &Plugins, docs: &[Doc]) -> String {
    let board = JsonBoard {
        docs: docs
            .iter()
            .map(|doc| JsonDoc {
                path: &doc.path,
                decisions: doc
                    .decisions
                    .iter()
                    .map(|d| JsonDecision::of(d, plugins))
                    .collect(),
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
        open: open_count(plugins, docs),
    };
    serde_json::to_string_pretty(&board).expect("the board serialises")
}

/// Every document of the project at `root` holding a register region, the project's register
/// first.
pub fn register_files(root: &Path) -> Vec<PathBuf> {
    let project = Project::of(root);
    let register = root.join(&project.register);
    let mut found: Vec<PathBuf> = document_files(root, &project)
        .into_iter()
        .filter(|path| read_text(path).is_ok_and(|text| splitlines(&text).contains(&REGION_START)))
        .collect();
    symphony_board::order::sort_by_key(&mut found, &|path| {
        (*path != register, path.to_string_lossy().to_string())
    });
    found
}

/// `--tidy`: supersede and resolve the cards of every document, a register's and those inline
/// alike, regroup every register, then move answered callouts to the ledger. Its exit code.
pub fn tidy(root: &Path, now: Option<&str>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    tidy_with(root, now, crate::register::sort_register, out, err)
}

/// How a tidy regroups a register, its cards read by the project's card kinds.
pub type Sort = fn(&str, &Plugins) -> Result<String, ReviewError>;

/// One register tidied: its new text, the slugs resolved and those superseded.
fn tidy_text(
    path: &Path,
    stamp: &str,
    sort: Sort,
    plugins: &Plugins,
) -> Result<(String, Vec<String>, Vec<String>), ReviewError> {
    let original = read_text(path).map_err(|e| ReviewError(os_error(&e, path)))?;
    let (text, superseded) = supersede(&original, stamp);
    let (text, changed) = resolve(&text, stamp, plugins);
    let text = sort(&text, plugins)?;
    check_nothing_lost(&original, &text, "tidying")?;
    check_nothing_repeated(&original, &text, "tidying")?;
    Ok((text, changed, superseded))
}

/// One register tidied and written.
fn tidy_file(
    path: &Path,
    stamp: &str,
    sort: Sort,
    plugins: &Plugins,
) -> Result<(String, Vec<String>, Vec<String>), ReviewError> {
    let tidied = tidy_text(path, stamp, sort, plugins)?;
    write_whole(path, &tidied.0).map_err(|e| ReviewError(os_error(&e, path)))?;
    Ok(tidied)
}

/// What a tidy says of one file: the cards it resolved, and those it superseded when any.
fn said(out: &mut dyn Write, changed: &[String], superseded: &[String]) {
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
}

/// Every document of the project at `root` that holds a card and no register region, `regions`
/// being those that hold one ([`register_files`]): its cards sit inline, each at the place the
/// document says what it decides.
pub fn inline_files(root: &Path, regions: &[PathBuf]) -> Vec<PathBuf> {
    let project = Project::of(root);
    let mut found: Vec<PathBuf> = document_files(root, &project)
        .into_iter()
        .filter(|path| !regions.contains(path))
        .filter(|path| {
            read_text(path).is_ok_and(|text| !parse_decisions(&splitlines(&text)).is_empty())
        })
        .collect();
    symphony_board::order::sort_by_key(&mut found, &|path| path.to_string_lossy().to_string());
    found
}

/// [`tidy`] with the regroup given, so a test can hand it one that drops a line.
pub fn tidy_with(
    root: &Path,
    now: Option<&str>,
    sort: Sort,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let project = Project::of(root);
    if let Some(folder) = project.missing_documents(root) {
        let _ = writeln!(
            err,
            "✖ no {folder}/ directory under {}",
            path_str(&root.to_string_lossy())
        );
        return 2;
    }
    let plugins = Plugins::at(&project, root);
    let stamp = now
        .filter(|now| !now.is_empty())
        .map_or_else(now_stamp, str::to_string);
    let regions = register_files(root);
    for path in &regions {
        let rel = relative(path, root);
        let (_, changed, superseded) = match tidy_file(path, &stamp, sort, &plugins) {
            Ok(tidied) => tidied,
            Err(problem) => {
                let _ = writeln!(err, "✖ {rel} not tidied: {problem}");
                return 1;
            }
        };
        said(out, &changed, &superseded);
        let _ = writeln!(out, "regrouped {rel}");
    }
    // A document whose cards all sit inline has nothing to regroup: its cards are resolved and
    // superseded where they are, and it is named only when one was.
    for path in inline_files(root, &regions) {
        let rel = relative(&path, root);
        match tidy_text(&path, &stamp, sort, &plugins) {
            Ok((_, changed, superseded)) if changed.is_empty() && superseded.is_empty() => {}
            Ok((text, changed, superseded)) => {
                if let Err(e) = write_whole(&path, &text) {
                    let _ = writeln!(err, "✖ {rel} not tidied: {}", os_error(&e, &path));
                    return 1;
                }
                said(out, &changed, &superseded);
                let _ = writeln!(out, "tidied {rel}");
            }
            Err(problem) => {
                let _ = writeln!(err, "✖ {rel} not tidied: {problem}");
                return 1;
            }
        }
    }
    match archive(root, &stamp, out) {
        Ok(()) => 0,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            1
        }
    }
}

/// Move every answered callout of the project's documents to the ledger in its first history
/// folder, saying which files gave some. A project without a history folder keeps them on their
/// pages.
fn archive(root: &Path, stamp: &str, out: &mut dyn Write) -> Result<(), ReviewError> {
    let project = Project::of(root);
    let Some(history) = project.history().into_iter().next() else {
        return Ok(());
    };
    let fail = |e: std::io::Error, path: &Path| ReviewError(os_error(&e, path));
    let mut entries: Vec<String> = Vec::new();
    for path in document_files(root, &project) {
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
    let ledger = root.join(history).join(LEDGER);
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
