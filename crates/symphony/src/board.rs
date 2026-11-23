//! The board over every document of the project, the Markdown its entrypoints show outside their
//! history folders: `just symphony-status`, its `--json`, and `--tidy`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use symphony_board::lock::Recovered as _;

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
    has_documents(root)?;
    documents(root)
}

/// [`scan`], each document a save still waits on read as its page shows it
/// ([`documents_shown`]): what `symphony status` reads, so that in a project that delivers by
/// pull request it says what the page says, an agent's kept reply among it.
pub fn scan_shown(root: &Path) -> Result<Vec<Doc>, ReviewError> {
    has_documents(root)?;
    documents_shown(root)
}

/// The refusal of a checkout with no documents folder.
fn has_documents(root: &Path) -> Result<(), ReviewError> {
    match Project::of(root).missing_documents(root) {
        Some(folder) => Err(ReviewError(format!(
            "no {folder}/ directory under {}",
            root.display()
        ))),
        None => Ok(()),
    }
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
            let parsed = PARSED.lock().recover();
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
            let mut parsed = PARSED.lock().recover();
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
///
/// The whole tidy is worked out before a file is written, so a card it cannot tidy leaves every
/// file as it was, and the ledger is written before the pages it takes comments from, so a ledger
/// that cannot be written leaves them on their pages. In a checkout on the default branch of a
/// project that delivers by pull request ([`crate::commit::delivers`]) it writes no file, as an
/// owner's save writes none there: a tracked edit left in that checkout stops every delivered
/// land. It reads each document there as the page shows it, the saves still waiting laid on it,
/// and keeps each file it changes as a save of its own, all of them together
/// ([`crate::commit::pending::keep_together`]), which the page shows at once and one pull request
/// delivers, the ledger never without the pages or the pages without it. A ledger that branch does
/// not hold yet could not be delivered, so there the answered comments stay on their pages, and
/// the tidy says so.
pub fn tidy(root: &Path, now: Option<&str>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    tidy_with(root, now, crate::register::sort_register, out, err)
}

/// [`tidy`] by the session `by`, which the saves it keeps for delivery name (`agent` when blank).
pub fn tidy_as(
    root: &Path,
    now: Option<&str>,
    by: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    tidied(root, now, crate::register::sort_register, by, out, err)
}

/// How a tidy regroups a register, its cards read by the project's card kinds.
pub type Sort = fn(&str, &Plugins) -> Result<String, ReviewError>;

/// One register tidied from its text `original`: its new text, the slugs resolved and those
/// superseded.
fn tidy_text(
    original: &str,
    stamp: &str,
    sort: Sort,
    plugins: &Plugins,
) -> Result<(String, Vec<String>, Vec<String>), ReviewError> {
    let (text, superseded) = supersede(original, stamp);
    let (text, changed) = resolve(&text, stamp, plugins);
    let text = sort(&text, plugins)?;
    check_nothing_lost(original, &text, "tidying")?;
    check_nothing_repeated(original, &text, "tidying")?;
    Ok((text, changed, superseded))
}

/// What a tidy says of one file: the cards it resolved, and those it superseded when any.
fn said(changed: &[String], superseded: &[String]) -> String {
    let resolved = if changed.is_empty() {
        "(none)".to_string()
    } else {
        changed.join(", ")
    };
    let superseded = if superseded.is_empty() {
        String::new()
    } else {
        format!("superseded: {}\n", superseded.join(", "))
    };
    format!("resolved: {resolved}\n{superseded}")
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

/// One write of a tidy: the file, its new text, what the tidy says once it is written, and the
/// words a failure to write it opens with.
struct Step {
    path: PathBuf,
    text: String,
    said: String,
    failed: String,
}

impl Step {
    /// The step's text written over its file whole, its folder made first.
    fn write(&self) -> Result<(), String> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| os_error(&e, folder))?;
        }
        write_whole(&self.path, &self.text)
            .map_err(|e| format!("{}{}", self.failed, os_error(&e, &self.path)))
    }
}

/// A tidy of the checkout at `root` worked out, nothing written yet: its writes in the order it
/// reaches them, a file's later write over its earlier one. `kept` when the tidy keeps its
/// changes for delivery rather than writing them (a checkout on the default branch of a project
/// that delivers by pull request): each file is then read as the page shows it, the saves still
/// waiting laid on it ([`crate::commit::pending::shown`]).
struct Plan {
    root: PathBuf,
    kept: bool,
    steps: Vec<Step>,
    /// Said once a kept tidy is done: what it left as it was, and why.
    left: Vec<String>,
}

/// A file a tidy changes: its text before the tidy, none for a file not there yet, and after.
struct Change {
    path: PathBuf,
    before: Option<String>,
    after: String,
}

impl Plan {
    fn new(root: &Path, kept: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            kept,
            steps: Vec::new(),
            left: Vec::new(),
        }
    }

    /// The text of `path` before the tidy: the file's own, with the saves still waiting laid on
    /// it where the tidy keeps its changes.
    fn original(&self, path: &Path) -> std::io::Result<String> {
        let text = read_text(path)?;
        if !self.kept {
            return Ok(text);
        }
        let file = relative(path, &self.root);
        Ok(crate::commit::pending::shown(&self.root, &file, text))
    }

    /// The text of `path` as the tidy leaves it so far: its last write here, else its original.
    fn read(&self, path: &Path) -> std::io::Result<String> {
        match self.steps.iter().rev().find(|step| step.path == path) {
            Some(step) => Ok(step.text.clone()),
            None => self.original(path),
        }
    }

    /// The files the tidy changes, each once in the order it reaches them: a file written with
    /// the text it already holds is not changed.
    fn changes(&self) -> Vec<Change> {
        let mut reached: Vec<&Path> = Vec::new();
        for step in &self.steps {
            if !reached.contains(&step.path.as_path()) {
                reached.push(&step.path);
            }
        }
        reached
            .into_iter()
            .filter_map(|path| {
                let before = self.original(path).ok();
                let after = self.read(path).ok()?;
                (before.as_ref() != Some(&after)).then(|| Change {
                    path: path.to_path_buf(),
                    before,
                    after,
                })
            })
            .collect()
    }
}

/// The tidy of the project at `root`, worked out and nothing written; the first file it cannot
/// tidy, as the line it is refused with.
fn plan(
    root: &Path,
    project: &Project,
    stamp: &str,
    sort: Sort,
    plugins: &Plugins,
    kept: bool,
) -> Result<Plan, String> {
    let mut plan = Plan::new(root, kept);
    let regions = register_files(root);
    for path in &regions {
        let rel = relative(path, root);
        let failed = format!("{rel} not tidied: ");
        let (text, changed, superseded) = plan
            .read(path)
            .map_err(|e| ReviewError(os_error(&e, path)))
            .and_then(|original| tidy_text(&original, stamp, sort, plugins))
            .map_err(|problem| format!("{failed}{problem}"))?;
        let said = format!("{}regrouped {rel}\n", said(&changed, &superseded));
        plan.steps.push(Step {
            path: path.clone(),
            text,
            said,
            failed,
        });
    }
    // A document whose cards all sit inline has nothing to regroup: its cards are resolved and
    // superseded where they are, and it is named only when one was.
    for path in inline_files(root, &regions) {
        let rel = relative(&path, root);
        let failed = format!("{rel} not tidied: ");
        let (text, changed, superseded) = plan
            .read(&path)
            .map_err(|e| ReviewError(os_error(&e, &path)))
            .and_then(|original| tidy_text(&original, stamp, sort, plugins))
            .map_err(|problem| format!("{failed}{problem}"))?;
        if changed.is_empty() && superseded.is_empty() {
            continue;
        }
        let said = format!("{}tidied {rel}\n", said(&changed, &superseded));
        plan.steps.push(Step {
            path,
            text,
            said,
            failed,
        });
    }
    archive(root, project, stamp, &mut plan)?;
    Ok(plan)
}

/// What a tidy kept for delivery says once it is done, `kept` the files it kept, each with its
/// save's number.
fn kept_said(project: &Project, kept: &[(String, u64)]) -> String {
    let each: Vec<String> = kept
        .iter()
        .map(|(file, number)| format!("{file} (save {number})"))
        .collect();
    format!(
        "kept for delivery: {}. This project delivers by pull request ([land] delivery), so a \
         tidy writes no file in a checkout on {}: the pages show it now, and a pull request \
         delivers it.\n",
        each.join(", "),
        project.default_branch
    )
}

/// [`tidy`] with the regroup given, so a test can hand it one that drops a line.
pub fn tidy_with(
    root: &Path,
    now: Option<&str>,
    sort: Sort,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    tidied(root, now, sort, "", out, err)
}

fn tidied(
    root: &Path,
    now: Option<&str>,
    sort: Sort,
    by: &str,
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
    // Where the host alone writes the default branch, the checkout on it is written by nothing
    // here, as an owner's save writes nothing there: each change is kept as a save
    // (`commit::pending`), read and kept under the lock the server's saves take.
    let kept = crate::commit::on_main(root) && crate::commit::delivers(root);
    let _held = if kept {
        match crate::commit::write_lock(root) {
            Ok(held) => held.map(crate::commit::Held),
            Err(problem) => {
                let _ = writeln!(err, "✖ not tidied: {problem}");
                return 1;
            }
        }
    } else {
        None
    };
    let plan = match plan(root, &project, &stamp, sort, &plugins, kept) {
        Ok(plan) => plan,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
    };
    if kept {
        return keep_for_delivery(root, &project, &plan, by, out, err);
    }
    for step in &plan.steps {
        if let Err(problem) = step.write() {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
        let _ = out.write_all(step.said.as_bytes());
    }
    0
}

/// The tidy `plan` of the checkout at `root` on the default branch of a project that delivers by
/// pull request, kept and not written: each file it changes kept as a save of its own, in the
/// order it reaches them (a ledger before its pages), all of them kept together
/// ([`crate::commit::pending::keep_together`]), so no page reaches the default branch without the
/// ledger that holds the comments it gave. `by` is the session tidying. Its exit code.
fn keep_for_delivery(
    root: &Path,
    project: &Project,
    plan: &Plan,
    by: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    for step in &plan.steps {
        let _ = out.write_all(step.said.as_bytes());
    }
    for left in &plan.left {
        let _ = writeln!(out, "{left}");
    }
    let (files, edits): (Vec<String>, Vec<crate::commit::Edit>) = (plan.changes().into_iter())
        .map(|change| {
            let file = relative(&change.path, root);
            let edit = crate::commit::Edit {
                rel: file.clone(),
                base: change.before.unwrap_or_default(),
                new: change.after,
            };
            (file, edit)
        })
        .unzip();
    match crate::commit::pending::keep_together(root, edits, by) {
        Ok(numbers) => {
            if !numbers.is_empty() {
                let kept: Vec<(String, u64)> = files.into_iter().zip(numbers).collect();
                let _ = out.write_all(kept_said(project, &kept).as_bytes());
            }
            0
        }
        Err(unkept) => {
            let file = &files[unkept.at];
            let _ = writeln!(
                err,
                "✖ {file} not kept for delivery: {}\n  nothing of this tidy is kept: the pages and \
                 {} stay as they were",
                unkept.problem, project.default_branch
            );
            if unkept.left > 0 {
                let _ = writeln!(
                    err,
                    "  ({} of its saves written before it stay in the journal ({}), never shown or \
                     delivered, and hold back no other save)",
                    unkept.left,
                    crate::commit::pending::journal_below()
                );
            }
            1
        }
    }
}

/// Move every answered callout of the project's documents, as the tidy leaves them so far, to
/// the ledger in its first history folder: the writes that does, the ledger's first and then each
/// document's, saying how many it gave, so a ledger that cannot be written leaves every comment
/// on its page. A project without a history folder keeps them on their pages, and so does a tidy
/// kept for delivery while the default branch holds no ledger yet: a save of a file that branch
/// does not hold cannot be delivered ([`crate::commit::pending::holds_file`]).
fn archive(root: &Path, project: &Project, stamp: &str, plan: &mut Plan) -> Result<(), String> {
    let Some(history) = project.history().into_iter().next() else {
        return Ok(());
    };
    let fail = |e: std::io::Error, path: &Path| os_error(&e, path);
    let mut entries: Vec<String> = Vec::new();
    let mut pages: Vec<Step> = Vec::new();
    for path in document_files(root, project) {
        let (text, taken) = archive_callouts(&plan.read(&path).map_err(|e| fail(e, &path))?);
        if taken.is_empty() {
            continue;
        }
        let rel = relative(&path, root);
        entries.extend(
            taken
                .iter()
                .map(|comment| ledger_entry(&rel, comment, stamp)),
        );
        let said = format!("archived: {rel} ({})\n", taken.len());
        pages.push(Step {
            path,
            text,
            said,
            failed: String::new(),
        });
    }
    if entries.is_empty() {
        return Ok(());
    }
    let ledger = root.join(history).join(LEDGER);
    let named = relative(&ledger, root);
    if plan.kept && !crate::commit::pending::holds_file(root, &named) {
        plan.left.push(format!(
            "archived: none of {} answered comments, as {} does not hold {named} yet: they stay \
             on their pages until a tidy in a task's worktree moves them",
            entries.len(),
            project.default_branch
        ));
        return Ok(());
    }
    let old = if ledger.exists() {
        plan.read(&ledger).map_err(|e| fail(e, &ledger))?
    } else {
        LEDGER_HEAD.to_string()
    };
    plan.steps.push(Step {
        text: format!("{}\n\n{}", old.trim_end_matches('\n'), entries.join("\n")),
        path: ledger,
        said: String::new(),
        failed: String::new(),
    });
    plan.steps.extend(pages);
    Ok(())
}
