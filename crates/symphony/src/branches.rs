//! The Symphony's view of every worktree: the cards an agent posted on a branch, before the branch
//! lands ([`branch_cards`]).
//!
//! The owner reviews in ONE app, the primary checkout's. A card posted on a branch (committed to a
//! worktree's register) is not in main's file until the branch merges, so this finds it: the
//! worktrees whose branch is live (not merged into main, its task not finished), and in each the
//! register cards that the branch's commits added or changed and that differ from main's. A card is
//! read from the branch's HEAD commit, never from the worktree's files, so a card an agent is still
//! writing reaches nobody; the owner's own lines the worktree holds uncommitted are laid on top of
//! it ([`one`]). A card is listed when it differs from main's AND from the version the branch forked
//! from, so a branch that is only behind main never shows main's newer cards as its own. A finished
//! branch keeps its posted revisions of main's proposal text until it lands, as a live branch does.
//! For other cards it keeps only threads the owner started: the owner's lines and the answers under
//! them, committed or not, laid on main's copy or the branch page's card when main has none.
//! On a live branch, what the owner said on a draft counts
//! until an agent answers it. A file a merge stopped on is read as the branch's committed copy with
//! what was saved on the branch's side since ([`file_cards`]).

mod one;

pub use one::{markdown, one_card, same, stale, with_owner_lines};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::plugins::Plugins;
use crate::project::Project;
use crate::register::{Decision, REGION_START, parse_decisions};
use crate::text::{rstrip, splitlines};
use sha2::{Digest, Sha256};

/// Git processes at once: a scan is mostly waiting on them.
const WORKERS: usize = 8;

/// A worktree on a branch other than the project's default branch (main).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub branch: String,
    pub path: PathBuf,
}

impl Tree {
    /// The branch as one URL segment: `~` cannot be in a branch name, so `/` maps back from it.
    pub fn id(&self) -> String {
        self.branch.replace('/', "~")
    }
}

/// A register card a branch's worktree holds that the owner is to read.
#[derive(Clone, Debug)]
pub struct BranchCard {
    pub tree: Tree,
    /// The page's document name, `docs/DECISIONS.md`, whatever the project's documents folder is
    /// called: [`Project::file`] is the file under the worktree.
    pub doc: String,
    /// On a live branch, the card as the branch's HEAD commit holds it, with the owner's lines its
    /// worktree holds uncommitted laid on top. A card the owner spoke on that only a draft or a
    /// finished branch holds is the worktree's copy. A card of main's carries only the owner's
    /// lines (on a finished branch, with the answers under them), under the card's own line.
    pub card: Decision,
    /// The card is main's (the branch did not post it): it adds the lines its
    /// worktree holds to main's copy, which stays main's, and is read on main's page.
    pub on_main: bool,
}

impl BranchCard {
    /// The page path through the app: the document behind the branch's prefix, or main's document
    /// for the owner's lines on main's card.
    pub fn path(&self) -> String {
        if self.on_main {
            self.doc.clone()
        } else {
            format!("@{}/{}", self.tree.id(), self.doc)
        }
    }
}

const FINISHED: [&str; 2] = ["done", "cancelled"];

/// Each branch name the board ties to a task, with the tasks it names: the task whose slug is the
/// branch, and the tasks that record it as their executor's branch or whose executor registered
/// on it, in the board's order.
fn branch_tasks(board: &symphony_board::Board) -> BTreeMap<&str, Vec<&symphony_board::Task>> {
    let mut named: BTreeMap<&str, Vec<&symphony_board::Task>> = BTreeMap::new();
    for task in &board.tasks {
        let mut branches: Vec<&str> = vec![task.slug.as_str()];
        branches.extend(task.branch.as_deref());
        for role in task.roles.iter().filter(|r| r.role == "executor") {
            if let Some(session) = board.sessions.iter().find(|s| s.row.handle == role.handle)
                && !session.row.branch.is_empty()
            {
                branches.push(session.row.branch.as_str());
            }
        }
        for branch in branches {
            let tasks = named.entry(branch).or_default();
            if !tasks.iter().any(|t| t.slug == task.slug) {
                tasks.push(task);
            }
        }
    }
    named
}

/// Whether `task` is done or cancelled.
fn is_finished(task: &symphony_board::Task) -> bool {
    FINISHED.contains(&task.status.as_str())
}

/// The branches whose work is finished on `board`: every task the branch names
/// ([`branch_tasks`]) is done or cancelled, so a branch named for a done task that a live task
/// records as its own is not. Such a branch lists no card of its own.
pub fn finished(board: &symphony_board::Board) -> BTreeSet<String> {
    branch_tasks(board)
        .into_iter()
        .filter(|(_, tasks)| tasks.iter().copied().all(is_finished))
        .map(|(branch, _)| branch.to_string())
        .collect()
}

/// The task each branch's cards are tagged with: the first live task the branch names
/// ([`branch_tasks`]), else the first it names.
pub fn names(board: &symphony_board::Board) -> BTreeMap<String, String> {
    branch_tasks(board)
        .into_iter()
        .filter_map(|(branch, tasks)| {
            let task = tasks
                .iter()
                .find(|task| !is_finished(task))
                .or(tasks.first())?;
            Some((branch.to_string(), task.slug.clone()))
        })
        .collect()
}

/// Recorded task names ([`names`]), with the branch slug used as the fallback when the board is
/// unavailable.
pub(crate) fn task_names(root: &Path) -> BTreeMap<String, String> {
    crate::taskview::connect(root)
        .and_then(|store| store.board().map_err(|e| e.to_string()))
        .map(|board| names(&board))
        .unwrap_or_default()
}

/// What a card says, for telling one version of it from another: its state, text and lines.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Shape {
    state: String,
    text: String,
    lines: Vec<String>,
}

type Shapes = HashMap<String, HashSet<Shape>>;

/// Read through [`symphony_board::sys::output`]: a child another thread spawns meanwhile never holds
/// the answer open.
pub(crate) fn git(at: &Path, args: &[&str]) -> Option<String> {
    let done = git_run(at, args)?;
    done.status
        .success()
        .then(|| String::from_utf8_lossy(&done.stdout).to_string())
}

/// [`git`] for `git grep`, whose exit status 1 with nothing on stderr says that nothing matched:
/// that is an empty answer, and only a grep that could not run is none.
fn git_grep(at: &Path, args: &[&str]) -> Option<String> {
    let done = git_run(at, args)?;
    match done.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&done.stdout).to_string()),
        Some(1) if done.stderr.is_empty() => Some(String::new()),
        _ => None,
    }
}

fn git_run(at: &Path, args: &[&str]) -> Option<std::process::Output> {
    symphony_board::sys::output(
        crate::children::git()
            .args(["-c", "diff.autoRefreshIndex=false"])
            .args(args)
            // A live scan can inspect a verdict worktree while its worker stages/commits it.
            // Read-only discovery must not refresh that index or contend with its writer.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(at),
    )
    .ok()
}

/// The worktrees of `root`'s repository that hold a branch other than main's, none when git could
/// not list them ([`listed`]).
pub fn trees(root: &Path) -> Vec<Tree> {
    listed(root).unwrap_or_default()
}

/// [`trees`], or none when `git worktree list` failed: an empty list is a listing that found no
/// worktree, never one that could not be made.
pub fn listed(root: &Path) -> Option<Vec<Tree>> {
    let main = Project::of(root).default_branch.clone();
    let listing = git(root, &["worktree", "list", "--porcelain"])?;
    let here = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut found = Vec::new();
    for (number, block) in listing.trim().split("\n\n").enumerate() {
        let entry: HashMap<&str, &str> = block
            .lines()
            .map(|line| line.split_once(' ').unwrap_or((line, "")))
            .collect();
        let branch = entry.get("branch").copied().unwrap_or("");
        if number == 0 || !branch.starts_with("refs/heads/") || entry.contains_key("prunable") {
            continue;
        }
        let name = &branch["refs/heads/".len()..];
        let path = PathBuf::from(entry.get("worktree").copied().unwrap_or(""));
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        if name != main && path.is_dir() && resolved != here {
            found.push(Tree {
                branch: name.to_string(),
                path,
            });
        }
    }
    Some(found)
}

/// The branches already in main: all they can hold of their own is what is uncommitted.
fn merged(root: &Path, main: &str) -> HashSet<String> {
    git(
        root,
        &[
            "for-each-ref",
            "--merged",
            main,
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .unwrap_or_default()
    .split_whitespace()
    .map(str::to_string)
    .collect()
}

/// The document pages of the project's entrypoints at `rev` that hold a register (cards): where a
/// branch's cards can be. None when git could not say.
fn registers(at: &Path, rev: &str, project: &Project) -> Option<Vec<String>> {
    let folders = project.pathspecs();
    let mut args = vec!["grep", "-l", "-e", REGION_START, rev, "--"];
    args.extend(folders.iter().map(String::as_str));
    Some(
        git_grep(at, &args)?
            .replace(&format!("{rev}:"), "")
            .split_whitespace()
            .filter(|name| project.is_page(name))
            .map(str::to_string)
            .collect(),
    )
}

/// The text at each `<rev>:<path>`, "" where there is none: one `git cat-file --batch` for all of
/// them, none when it could not run. Its input and output go through
/// [`symphony_board::sys::spawn_piped`]: it runs before readiness, and a child another thread
/// spawns meanwhile must hold neither end open.
pub(crate) fn blobs(at: &Path, specs: &[String]) -> Option<Vec<String>> {
    if specs.is_empty() {
        return Some(Vec::new());
    }
    let mut child = symphony_board::sys::spawn_piped(
        crate::children::git()
            .args(["cat-file", "--batch"])
            .current_dir(at),
    )
    .ok()?;
    let input: String = specs.iter().map(|s| format!("{s}\n")).collect();
    let mut stdin = child.stdin.take().expect("a piped stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let done = symphony_board::sys::wait_output(child).ok()?;
    let _ = writer.join();
    if !done.status.success() {
        return None;
    }
    let data = done.stdout;
    let mut out = Vec::new();
    let mut at_byte = 0;
    for _ in specs {
        let Some(end) = data[at_byte..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| at_byte + p)
        else {
            out.push(String::new());
            continue;
        };
        let head: Vec<&[u8]> = data[at_byte..end].split(|b| *b == b' ').collect();
        let size = (head.len() == 3 && head[1] == b"blob")
            .then(|| String::from_utf8_lossy(head[2]).parse::<usize>().ok())
            .flatten();
        match size {
            Some(size) if end + 1 + size <= data.len() => {
                out.push(String::from_utf8_lossy(&data[end + 1..end + 1 + size]).to_string());
                at_byte = end + 1 + size + 1;
            }
            _ => {
                out.push(String::new());
                at_byte = end + 1;
            }
        }
    }
    Some(out)
}

/// The document pages of the project's entrypoints that the git command `args` names, one per line,
/// over the project's folders; none when git could not say.
fn pages(tree: &Tree, project: &Project, args: &[&str]) -> Option<BTreeSet<String>> {
    let folders = project.pathspecs();
    let mut all = args.to_vec();
    all.extend(folders.iter().map(String::as_str));
    Some(
        git(&tree.path, &all)?
            .lines()
            .filter(|name| project.is_page(name))
            .map(str::to_string)
            .collect(),
    )
}

/// The pages whose worktree file differs from the branch's HEAD commit: where the owner may have
/// written a line no commit has.
fn unsaved(tree: &Tree, project: &Project) -> Option<BTreeSet<String>> {
    pages(tree, project, &["diff", "--name-only", "HEAD", "--"])
}

/// The cards of the worktree's file `name`, none when it cannot be read ([`file_cards`]).
fn worktree_cards(tree: &Tree, name: &str) -> Vec<Decision> {
    crate::text::read_text(&tree.path.join(name))
        .map(|text| file_cards(&tree.path, name, &text))
        .unwrap_or_default()
}

/// The cards of `text`, the file `file` of the worktree at `root`. While a merge stopped on it
/// ([`crate::merging::conflict`]), each is the branch's committed copy with what was saved on the
/// branch's side since ([`one::since_merge`]): a line of the other side's that git left outside a
/// conflict never counts as the branch card's.
pub(crate) fn file_cards(root: &Path, file: &str, text: &str) -> Vec<Decision> {
    let Some(conflict) = crate::merging::conflict(root, file, text) else {
        return cards(text);
    };
    let head = crate::merging::head_text(root, file).unwrap_or_default();
    let theirs = crate::merging::theirs_text(root, file).unwrap_or_default();
    one::since_merge(cards(&conflict.ours()), &cards(&head), &cards(&theirs))
}

/// Whether `worktree`, a card's copy in a worktree's file, says anything its branch's posted copy
/// `posted` does not besides the owner's dated lines ([`one::changed_since`]).
pub(crate) fn changed_since(worktree: &Decision, posted: &Decision) -> bool {
    one::changed_since(worktree, posted)
}

fn shape(card: &Decision) -> Shape {
    Shape {
        state: card.state.as_str().to_string(),
        text: card.text.clone(),
        lines: card.raw.iter().map(|l| rstrip(l).to_string()).collect(),
    }
}

fn cards(text: &str) -> Vec<Decision> {
    parse_decisions(&splitlines(text))
}

/// Every card of every register in `texts`, by slug: wherever a card sits, a moved card is the same card.
fn shapes(texts: &[String]) -> Arc<Shapes> {
    // Cache parsed immutable register blobs shared by many worktrees.
    static PARSED: LazyLock<Mutex<HashMap<Vec<u8>, Arc<Shapes>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut hash = Sha256::new();
    for text in texts {
        hash.update(text.len().to_le_bytes());
        hash.update(text.as_bytes());
    }
    let key = hash.finalize().to_vec();
    let mut parsed = PARSED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(shapes) = parsed.get(&key) {
        return shapes.clone();
    }
    let mut found: Shapes = HashMap::new();
    for text in texts {
        for card in cards(text) {
            found
                .entry(card.slug.clone())
                .or_default()
                .insert(shape(&card));
        }
    }
    let found = Arc::new(found);
    if parsed.len() >= 32 {
        parsed.clear();
    }
    parsed.insert(key, found.clone());
    found
}

/// What the registers held where the checkout at `root` forked from main (its merge base with
/// main): a card the fork did not hold as it is now is one its branch wrote.
pub(crate) struct Fork(Arc<Shapes>);

/// Where the checkout at `root` forked from main: its merge base with the project's default
/// branch, none when it has none.
pub(crate) fn fork_base(root: &Path) -> Option<String> {
    let project = Project::of(root);
    let base = git(root, &["merge-base", &project.default_branch, "HEAD"])?
        .trim()
        .to_string();
    (!base.is_empty()).then_some(base)
}

impl Fork {
    /// The fork of the checkout at `root`, none when it has no merge base with main.
    pub(crate) fn of(root: &Path) -> Option<Fork> {
        let project = Project::of(root);
        let base = fork_base(root)?;
        let specs: Vec<String> = registers(root, &base, &project)
            .unwrap_or_default()
            .iter()
            .map(|n| format!("{base}:{n}"))
            .collect();
        Some(Fork(shapes(&blobs(root, &specs).unwrap_or_default())))
    }

    /// What `card` says that the fork's copies of it do not ([`one::written`]): what this
    /// checkout's branch wrote on it.
    pub(crate) fn written(&self, card: &Decision) -> Decision {
        written_beside(card, [self.0.get(&card.slug)])
    }
}

/// What `card` says that none of the copies `held` does ([`one::written`]).
fn written_beside<'a>(
    card: &Decision,
    held: impl IntoIterator<Item = Option<&'a HashSet<Shape>>>,
) -> Decision {
    one::written(
        card,
        held.into_iter()
            .flatten()
            .flatten()
            .map(|copy| (copy.text.as_str(), copy.lines.as_slice())),
    )
}

/// Whether `card` asks nothing more of the owner: resolved, or rejected.
pub(crate) fn closed(plugins: &Plugins, card: &Decision) -> bool {
    matches!(
        card.group(plugins),
        crate::Group::Resolved | crate::Group::Rejected
    )
}

/// Whether `card` waits on the owner's word, and so would put its pictures before the owner: not
/// closed, nor waiting on an agent to act on what the owner said.
fn asks_owner(plugins: &Plugins, card: &Decision) -> bool {
    !matches!(
        card.group(plugins),
        crate::Group::Resolved | crate::Group::Rejected | crate::Group::Agent
    )
}

/// How a worktree's branch stands for the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    /// Its posted cards are the owner's to read.
    Live,
    /// Its task is done or cancelled: revisions of main's proposal text and threads the owner
    /// started there still count until it lands.
    Finished,
    /// In main already: all it can hold of its own is uncommitted.
    Merged,
}

/// What a branch's page draws otherwise than its worktree's file holds it ([`page_cards`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageMarks {
    /// The cards only the worktree's file holds, not the branch's HEAD commit: drafts, marked as
    /// such on the page.
    pub drafts: BTreeSet<String>,
    /// The cards the branch posted (its HEAD commit's copy is neither its fork's nor main's): its
    /// page draws its own copy of them.
    pub own: BTreeSet<String>,
}

/// What a scan of the worktrees found.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    /// The cards the owner is to read ([`branch_cards`]).
    pub cards: Vec<BranchCard>,
    /// By branch page (`@<tree>/<doc>`), what it draws otherwise than its file ([`PageMarks`]).
    pub pages: BTreeMap<String, PageMarks>,
    /// The missing picture files of the cards held back for them: a card is posted when they land,
    /// which no other event need follow, so a server checks them on a timer.
    pub awaited: Vec<PathBuf>,
    /// The branches whose worktree git could not read, and so listed nothing: what the scan lacks
    /// of them says nothing of where their cards are ([`crate::owner_asks`]). Every branch is one
    /// when main's registers could not be read.
    pub failed: BTreeSet<String>,
}

/// One worktree's part of a [`Scan`].
///
/// A live branch's cards are those of each page its commits changed since it forked from main, as
/// its HEAD commit holds them, with the owner's lines its worktree holds uncommitted on top. A card
/// only the worktree's file holds is a draft and is not read; nor is a posted card waiting on the
/// owner whose pictures, in what the branch wrote on it, are not in the worktree yet. The owner's
/// lines on a card the branch did not post go to main's copy ([`owner_card`]). What the owner said
/// on a live branch's draft counts until an agent answers it ([`waiting_owner`]); on a finished or
/// merged branch, the owner's lines and the answers under them count ([`finished_thread`]). A tree
/// that cannot be read lists nothing and says so ([`Scan::failed`]): one bad worktree never takes
/// the app down.
fn tree_scan(tree: &Tree, main_shapes: &Shapes, project: &Project, standing: Standing) -> Scan {
    let failed = || Scan {
        failed: BTreeSet::from([tree.branch.clone()]),
        ..Scan::default()
    };
    let mut out = Scan::default();
    // Where the branch forked from main: a merged branch has no fork, and posts nothing.
    let base = if standing == Standing::Merged {
        None
    } else {
        let base = git(&tree.path, &["merge-base", &project.default_branch, "HEAD"])
            .unwrap_or_default()
            .trim()
            .to_string();
        if base.is_empty() {
            return failed();
        }
        Some(base)
    };
    let posted = match &base {
        None => Some(BTreeSet::new()),
        Some(base) => pages(tree, project, &["diff", "--name-only", base, "HEAD", "--"]),
    };
    let (Some(posted), Some(unsaved)) = (posted, unsaved(tree, project)) else {
        return failed();
    };
    if posted.is_empty() && unsaved.is_empty() {
        return out;
    }
    let forked = match &base {
        None => Some(Vec::new()),
        Some(base) => registers(&tree.path, base, project),
    };
    let Some(forked) = forked else {
        return failed();
    };
    let docs: Vec<&String> = posted.union(&unsaved).collect();
    let specs: Vec<String> = base
        .iter()
        .flat_map(|base| forked.iter().map(move |n| format!("{base}:{n}")))
        .chain(docs.iter().map(|n| format!("HEAD:{n}")))
        .collect();
    let Some(texts) = blobs(&tree.path, &specs) else {
        return failed();
    };
    let (forked_texts, head_texts) = texts.split_at(forked.len().min(texts.len()));
    let forked = shapes(forked_texts);
    let plugins = std::cell::OnceCell::new();
    for (name, head) in docs.into_iter().zip(head_texts) {
        let doc = project.route(name).unwrap_or_else(|| name.clone());
        let saved = if unsaved.contains(name) {
            worktree_cards(tree, name)
        } else {
            Vec::new()
        };
        let heads = cards(head);
        let mut marks = PageMarks::default();
        for posted in &heads {
            let worktree = saved.iter().find(|w| w.slug == posted.slug);
            let owner = worktree
                .map(|w| match standing {
                    Standing::Finished => one::thread_beside(w, [posted.raw.as_slice()]),
                    _ => one::owner_lines(w, Some(posted)),
                })
                .unwrap_or_default();
            let copy = shape(posted);
            let main = main_shapes.get(&posted.slug);
            // The branch did not post this card: its copy is main's (a merged branch's all are),
            // the fork's (it may have moved it to another file), or main's as it is now. It is
            // main's card, and the owner's lines on the branch's copy join main's: on a finished
            // branch, with the answers its worktree holds under them.
            if standing == Standing::Merged
                || forked.get(&posted.slug).is_some_and(|s| s.contains(&copy))
                || main.is_some_and(|s| s.contains(&copy))
            {
                let lines = match (standing, worktree) {
                    (Standing::Live, _) => owner,
                    (_, Some(worktree)) => one::thread_beside(
                        worktree,
                        std::iter::once(posted.raw.as_slice()).chain(lines_of(main)),
                    ),
                    (_, None) => Vec::new(),
                };
                if !lines.is_empty() {
                    out.cards
                        .push(owner_card(tree, &doc, posted.clone(), &lines));
                }
                continue;
            }
            marks.own.insert(posted.slug.clone());
            if standing == Standing::Finished {
                let revises_main = main.is_some_and(|copies| rewrites_all(posted, copies))
                    && forked
                        .get(&posted.slug)
                        .is_none_or(|copies| rewrites_all(posted, copies));
                if !revises_main {
                    // New cards and replies on the same proposal keep only the owner's thread.
                    // Rewritten headings or body content stay the branch's until it lands.
                    out.cards.extend(finished_thread(
                        tree,
                        &doc,
                        worktree.unwrap_or(posted),
                        main_shapes,
                        &forked,
                    ));
                    continue;
                }
            }
            let card = if owner.is_empty() {
                posted.clone()
            } else {
                one::with_lines(posted, &owner)
            };
            if main_shapes
                .get(&card.slug)
                .is_some_and(|s| s.contains(&shape(&card)))
            {
                continue; // main holds it as it is now, in whichever file
            }
            // Posted before its pictures: it waits until they load, judged on what the branch wrote
            // (main's pictures may live in main's `/artifacts/`, which no new worktree holds), and
            // unless the owner spoke last on it.
            if !one::awaits_agent(&card)
                && asks_owner(plugins.get_or_init(|| Plugins::of(&tree.path)), &card)
            {
                let written = written_beside(
                    posted,
                    [forked.get(&posted.slug), main_shapes.get(&posted.slug)],
                );
                let missing = crate::docs_audit::missing_files(&tree.path, &doc, &written);
                if !missing.is_empty() {
                    out.awaited.extend(missing);
                    continue;
                }
            }
            // Main's copy, answered or changed since the fork or not, is one card with this one
            // ([`crate::dashboard::merge_decisions`]): what main's says still counts.
            out.cards.push(BranchCard {
                tree: tree.clone(),
                doc: doc.clone(),
                card,
                on_main: false,
            });
        }
        for draft in saved
            .iter()
            .filter(|w| !heads.iter().any(|p| p.slug == w.slug))
        {
            marks.drafts.insert(draft.slug.clone());
            out.cards.extend(if standing == Standing::Live {
                waiting_owner(tree, &doc, draft, main_shapes)
            } else {
                finished_thread(tree, &doc, draft, main_shapes, &forked)
            });
        }
        if marks != PageMarks::default() {
            out.pages.insert(format!("@{}/{doc}", tree.id()), marks);
        }
    }
    out
}

/// Whether `posted` rewrites every one of `copies`, the copies of its card somewhere else
/// ([`one::rewrites`]): true of no copies at all.
fn rewrites_all(posted: &Decision, copies: &HashSet<Shape>) -> bool {
    copies
        .iter()
        .all(|copy| one::rewrites(posted, &copy.text, &copy.lines))
}

/// The lines of the copies `copies` of a card.
fn lines_of(copies: Option<&HashSet<Shape>>) -> impl Iterator<Item = &[String]> {
    copies
        .into_iter()
        .flatten()
        .map(|copy| copy.lines.as_slice())
}

/// What the owner said on a live branch's draft `card`, in the page `doc` of `tree`, that waits on
/// an agent ([`one::awaits_agent`]) and that main's copies do not hold ([`said`]). None once an
/// agent answered, the answer being a draft too until its branch posts it.
fn waiting_owner(
    tree: &Tree,
    doc: &str,
    card: &Decision,
    main_shapes: &Shapes,
) -> Option<BranchCard> {
    if !one::awaits_agent(card) {
        return None;
    }
    let main = main_shapes.get(&card.slug);
    let owner = one::owner_lines_beside(card, lines_of(main));
    said(tree, doc, card, main.is_some(), &owner)
}

/// The thread the owner started on `card` in a finished branch's worktree, committed or not, that
/// neither main's copies nor the ones it forked with (`forked`) hold ([`one::thread_beside`]): the
/// owner's lines and the agents' answers under them, never lost to an answer ([`said`]). It stays
/// until main holds those lines or closes the card after them
/// ([`crate::dashboard::merge_decisions`]).
fn finished_thread(
    tree: &Tree,
    doc: &str,
    card: &Decision,
    main_shapes: &Shapes,
    forked: &Shapes,
) -> Option<BranchCard> {
    let main = main_shapes.get(&card.slug);
    let thread = one::thread_beside(card, lines_of(main).chain(lines_of(forked.get(&card.slug))));
    said(tree, doc, card, main.is_some(), &thread)
}

/// The lines `lines` a worktree holds on `card`, in the page `doc` of `tree`: laid on main's copy
/// when main has the card (`on_main`, [`owner_card`]), else the card itself as its branch page's.
/// None when there are none.
fn said(
    tree: &Tree,
    doc: &str,
    card: &Decision,
    on_main: bool,
    lines: &[one::Chunk],
) -> Option<BranchCard> {
    if lines.is_empty() {
        return None;
    }
    Some(if on_main {
        owner_card(tree, doc, card.clone(), lines)
    } else {
        BranchCard {
            tree: tree.clone(),
            doc: doc.to_string(),
            card: card.clone(),
            on_main: false,
        }
    })
}

/// The lines `lines` that the worktree of `tree` holds on its copy of `card`, in the page `doc`,
/// to be laid on main's copy of the card: the card's line, slug and text with those lines alone
/// under it.
fn owner_card(tree: &Tree, doc: &str, card: Decision, lines: &[one::Chunk]) -> BranchCard {
    let bare = Decision {
        raw: Vec::new(),
        note: Vec::new(),
        ..card
    };
    BranchCard {
        tree: tree.clone(),
        doc: doc.to_string(),
        card: one::with_lines(&bare, lines),
        on_main: true,
    }
}

/// Whether `other` is `held` with more history under it: the same card, answered further. A card
/// asked in two worktrees (stacked branches) is answered in one; the other copy must not ask the
/// owner again.
fn extends(other: &Shape, held: &Shape) -> bool {
    other.text == held.text
        && other.lines.len() > held.lines.len()
        && other.lines[..held.lines.len()] == held.lines[..]
}

/// Every card the live branches posted that main does not hold, and what the owner said in any
/// worktree that main's copies lack, in branch then page order ([`tree_scan`]). A branch is
/// finished when it is merged into main or the board says its task is done or cancelled; the board
/// is read for that when its server runs (none is started), and a board that cannot be read
/// finishes nothing.
pub fn branch_cards(root: &Path) -> Vec<BranchCard> {
    let all = trees(root);
    if all.is_empty() {
        return Vec::new();
    }
    let finished = crate::taskview::connect_running(root)
        .and_then(|store| store.board().map_err(|e| e.to_string()))
        .map(|board| finished(&board))
        .unwrap_or_default();
    scan_trees(root, all, &finished).cards
}

/// [`branch_cards`] with the finished branches given: `finished` names the branches whose task is
/// done, as [`finished`] reads them from the board.
pub fn scan(root: &Path, finished: &BTreeSet<String>) -> Vec<BranchCard> {
    scan_pages(root, finished).cards
}

/// [`scan`], with what each branch page draws otherwise than its file and the pictures held cards
/// wait on.
pub fn scan_pages(root: &Path, finished: &BTreeSet<String>) -> Scan {
    scan_trees(root, trees(root), finished)
}

/// [`scan_pages`] over the worktrees `trees` a caller already listed ([`listed`]).
pub fn scan_listed(root: &Path, trees: Vec<Tree>, finished: &BTreeSet<String>) -> Scan {
    scan_trees(root, trees, finished)
}

fn scan_trees(root: &Path, mut all: Vec<Tree>, finished: &BTreeSet<String>) -> Scan {
    symphony_board::order::sort_by(&mut all, &|a, b| a.branch.cmp(&b.branch));
    if all.is_empty() {
        return Scan::default();
    }
    let project = Project::of(root);
    let main = &project.default_branch;
    let in_main = merged(root, main);
    let specs: Option<Vec<String>> = registers(root, main, &project).map(|mut sorted| {
        sorted.sort();
        sorted.iter().map(|n| format!("{main}:{n}")).collect()
    });
    let main_texts = specs.and_then(|specs| blobs(root, &specs));
    let main_shapes = shapes(main_texts.as_deref().unwrap_or_default());
    let next = AtomicUsize::new(0);
    let done: Vec<Mutex<Scan>> = all.iter().map(|_| Mutex::new(Scan::default())).collect();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(all.len()) {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::SeqCst);
                    let Some(tree) = all.get(at) else { break };
                    let standing = if in_main.contains(&tree.branch) {
                        Standing::Merged
                    } else if finished.contains(&tree.branch) {
                        Standing::Finished
                    } else {
                        Standing::Live
                    };
                    *done[at].lock().unwrap_or_else(|e| e.into_inner()) =
                        tree_scan(tree, &main_shapes, &project, standing);
                }
            });
        }
    });
    let mut out = Scan::default();
    let mut listed = Vec::new();
    for found in done {
        let found = found.into_inner().unwrap_or_else(|e| e.into_inner());
        listed.extend(found.cards);
        out.pages.extend(found.pages);
        out.awaited.extend(found.awaited);
        out.failed.extend(found.failed);
    }
    if main_texts.is_none() {
        // Without main's copies no branch's card can be told from main's.
        out.failed
            .extend(all.iter().map(|tree| tree.branch.clone()));
    }
    let mut every: Shapes = (*main_shapes).clone();
    for card in listed.iter().filter(|c| !c.on_main) {
        every
            .entry(card.card.slug.clone())
            .or_default()
            .insert(shape(&card.card));
    }
    let mut seen: HashSet<(String, Shape)> = HashSet::new();
    for card in listed {
        if card.on_main {
            out.cards.push(card);
            continue;
        }
        let held = shape(&card.card);
        if every[&card.card.slug]
            .iter()
            .any(|other| extends(other, &held))
        {
            continue; // another copy has the owner's answer on top of this one
        }
        if seen.insert((card.card.slug.clone(), held)) {
            // stacked branches carry the same card: it is listed once
            out.cards.push(card);
        }
    }
    out
}

/// Where a branch's page was drawn from: its worktree, or, that gone, its branch by name.
#[derive(Clone, Copy, Debug)]
pub enum Page<'a> {
    Tree(&'a Path),
    Gone(&'a str),
}

/// The commit the branch `name` points at in the checkout at `root`: `Some(None)` when there is no
/// such branch, none when git could not say. The branch is read by its ref's full name alone, so a
/// name holding a revision's syntax (`main@{1}`) is no branch, never the commit it would name.
fn branch_tip(root: &Path, name: &str) -> Option<Option<String>> {
    let named = format!("refs/heads/{name}");
    let listed = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)",
            &named,
        ],
    )?;
    Some(listed.lines().find_map(|line| {
        let (refname, commit) = line.split_once('\0')?;
        (refname == named).then(|| commit.to_string())
    }))
}

/// Whether the owner's line on the card `slug` of the document `rel`, saved from a branch's page
/// (`page`), is main's to keep: exactly when that page draws main's card ([`page_cards`]). Main's
/// copy of the document (in the checkout at `main_root`) holds the card, and the branch is in main
/// already (landed, or with no commit of its own), or its copy of the card is the one it forked
/// with or main's (it did not post the card, as [`tree_scan`] reads it), or it posted the card and
/// main closed its copy after every line of the page's ([`one::stale`], main's copy as main's file
/// holds it). The branch is read at its worktree's HEAD, or, the worktree gone, at the branch's
/// tip: its page was drawn from it; with neither left, the card is main's. A card the branch posted
/// that main has not closed since, a draft only its worktree holds and a card main's copy lacks
/// are the branch's, and so is the line whenever git cannot say: the page's save then stands or
/// fails where the owner sees it, never laid on a card the owner did not read.
pub fn mains_card(main_root: &Path, page: Page, rel: &str, slug: &str) -> bool {
    let project = Project::of(main_root);
    let card = |text: &str| cards(text).into_iter().find(|card| card.slug == slug);
    let main_file = project.file(rel);
    let Some(main_card) = crate::text::read_text(&main_root.join(&main_file))
        .ok()
        .and_then(|text| card(&text))
    else {
        return false;
    };
    let main = &project.default_branch;
    let (at, head, file) = match page {
        Page::Tree(tree) => match git(tree, &["rev-parse", "HEAD"]) {
            Some(head) => (tree, head.trim().to_string(), Project::of(tree).file(rel)),
            None => return false,
        },
        Page::Gone(branch) => match branch_tip(main_root, branch) {
            Some(Some(tip)) => (main_root, tip, main_file.clone()),
            Some(None) => return true,
            None => return false,
        },
    };
    let Some(base) = git(at, &["merge-base", main, &head]) else {
        return false;
    };
    let base = base.trim();
    let specs = [
        format!("{head}:{file}"),
        format!("{base}:{file}"),
        format!("{main}:{main_file}"),
    ];
    let Some(texts) = blobs(at, &specs) else {
        return false;
    };
    let Some(posted) = card(&texts[0]) else {
        return false; // a draft
    };
    if head == base {
        return true;
    }
    let copy = shape(&posted);
    if texts[1..]
        .iter()
        .any(|text| card(text).is_some_and(|held| shape(&held) == copy))
    {
        return true;
    }
    let drawn = match page {
        Page::Tree(tree) => page_file(tree, rel, true)
            .into_iter()
            .find(|card| card.slug == slug),
        Page::Gone(_) => Some(posted),
    };
    drawn.is_some_and(|drawn| {
        one::stale(
            &main_card,
            closed(&Plugins::of(main_root), &main_card),
            &drawn,
        )
    })
}

/// The cards of the page `rel` of the checkout at `root` as its file holds them: in a worktree
/// (`worktree`), its last committed copy while a merge stopped on it ([`crate::merging::committed`]).
fn page_file(root: &Path, rel: &str, worktree: bool) -> Vec<Decision> {
    let file = Project::of(root).file(rel);
    crate::text::read_text(&root.join(&file))
        .map(|text| {
            let shown = worktree
                .then(|| crate::merging::committed(root, &file, &text))
                .flatten();
            cards(shown.as_deref().unwrap_or(&text))
        })
        .unwrap_or_default()
}

/// The cards the page `rel` of the worktree at `tree_root` draws in place of its file's copies:
/// the page's one cards (`shown`, [`crate::dashboard::Snapshot::pages`]), and for each card the
/// branch did not post, main's card as main's page draws it (`main_shown` over the file of the
/// checkout at `main_root`), so a stale copy shows main's resolution and every answer main's card
/// has. A draft, and a card the branch posted that main has not closed since, stay the file's. The
/// file is the one the page shows: its last committed copy while a merge stopped on it
/// ([`crate::merging::committed`]).
pub fn page_cards(
    main_root: &Path,
    tree_root: &Path,
    rel: &str,
    shown: &[Decision],
    main_shown: &[Decision],
    marks: &PageMarks,
) -> Vec<Decision> {
    let read = |root: &Path| page_file(root, rel, root != main_root);
    let file = read(tree_root);
    let main: Vec<Decision> = read(main_root)
        .into_iter()
        .map(|card| {
            main_shown
                .iter()
                .find(|one| one.slug == card.slug)
                .cloned()
                .unwrap_or(card)
        })
        .collect();
    let plugins = std::cell::OnceCell::new();
    let mut out = Vec::new();
    for card in &file {
        if let Some(one) = shown.iter().find(|one| one.slug == card.slug) {
            out.push(one.clone());
            continue;
        }
        if marks.drafts.contains(&card.slug) {
            continue;
        }
        let Some(main) = main.iter().find(|m| m.slug == card.slug) else {
            continue;
        };
        let plugins = plugins.get_or_init(|| Plugins::of(main_root));
        if !one::same(main, card)
            && (!marks.own.contains(&card.slug) || one::stale(main, closed(plugins, main), card))
        {
            out.push(main.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{cards, listed, rewrites_all, scan_listed, shape, shapes};
    use crate::test_tree::Tree;
    use std::collections::{BTreeSet, HashSet};
    use std::path::Path;

    /// `git` in `at`, which must succeed.
    fn git(at: &Path, args: &[&str]) {
        let done = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(at)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            done.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&done.stderr)
        );
    }

    const REGISTER: &str = "# A\n\n<!-- register:start -->\n\n- [ ] **lamp-a** — Lamp A.\n  \
                            [2026-11-11 09:00] agent — Posted.\n<!-- register:end -->\n";

    #[test]
    fn a_card_rewrites_all_its_copies_only_when_it_rewrites_each() {
        let posted = &cards(REGISTER)[0];
        let same = shape(posted);
        let reworded = shape(&cards(&REGISTER.replace("Lamp A.", "Lamp B."))[0]);
        // No copy anywhere: nothing it leaves as it was.
        assert!(rewrites_all(posted, &HashSet::new()));
        assert!(rewrites_all(posted, &HashSet::from([reworded.clone()])));
        assert!(!rewrites_all(posted, &HashSet::from([same.clone()])));
        // One copy it only repeats is enough: it does not rewrite them all.
        assert!(!rewrites_all(posted, &HashSet::from([same, reworded])));
    }

    #[test]
    fn a_listed_worktree_git_cannot_read_lists_no_card_and_says_it_failed() {
        let main = Tree::new();
        main.write("docs/A.md", "# A\n");
        git(&main.root, &["init", "-q"]);
        git(&main.root, &["add", "."]);
        git(&main.root, &["commit", "-qm", "a"]);
        let linked = main.path("wt");
        let linked_arg = linked.to_str().unwrap();
        git(
            &main.root,
            &["worktree", "add", "-q", "-b", "pic/x", linked_arg],
        );
        let idle = main.path("idle");
        git(
            &main.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "pic/idle",
                idle.to_str().unwrap(),
            ],
        );
        std::fs::write(linked.join("docs/A.md"), REGISTER).unwrap();
        git(&linked, &["commit", "-qam", "post lamp-a"]);
        let none = BTreeSet::new();
        let read = scan_listed(&main.root, listed(&main.root).unwrap(), &none);
        let slugs: Vec<&str> = read.cards.iter().map(|c| c.card.slug.as_str()).collect();
        assert_eq!(slugs, ["lamp-a"]);
        assert!(read.failed.is_empty(), "{:?}", read.failed);

        // Its `.git` names no repository: git lists it from main's, and cannot read it.
        let link = std::fs::read_to_string(linked.join(".git")).unwrap();
        std::fs::write(linked.join(".git"), "gitdir: /nowhere/at/all\n").unwrap();
        let trees = listed(&main.root).unwrap();
        assert_eq!(trees.len(), 2, "still listed: {trees:?}");
        let failed = scan_listed(&main.root, trees, &none);
        assert!(failed.cards.is_empty() && failed.pages.is_empty());
        assert_eq!(failed.failed, BTreeSet::from(["pic/x".to_string()]));
        std::fs::write(linked.join(".git"), link).unwrap();
        let again = scan_listed(&main.root, listed(&main.root).unwrap(), &none);
        assert_eq!(again.cards.len(), 1);
        assert!(
            again.failed.is_empty(),
            "the idle worktree reads as nothing to list"
        );
    }

    #[test]
    fn idle_register_cache_invalidates_history_and_verdict_changes() {
        let before = shapes(&[
            "- [ ] **idle-cache** — A card.\n  [2026-10-26] agent — old history\n".into(),
        ]);
        assert!(
            before["idle-cache"]
                .iter()
                .any(|card| card.lines.iter().any(|line| line.contains("old history")))
        );
        let after = shapes(&[
            "- [ ] **idle-cache** — A card.\n  [2026-10-26] agent — new history\n".into(),
        ]);
        assert!(
            after["idle-cache"]
                .iter()
                .all(|card| card.state == "pending")
        );
        assert!(
            after["idle-cache"]
                .iter()
                .any(|card| card.lines.iter().any(|line| line.contains("new history")))
        );
        assert!(
            !after["idle-cache"]
                .iter()
                .any(|card| card.lines.iter().any(|line| line.contains("old history")))
        );
        let decided = shapes(&[
            "- [x] **idle-cache** — A card.\n  [2026-10-26] agent — new history\n".into(),
        ]);
        assert_eq!(decided["idle-cache"].len(), 1);
        assert!(
            decided["idle-cache"]
                .iter()
                .all(|card| card.state == "approved")
        );
        let restored = shapes(&[
            "- [ ] **idle-cache** — A card.\n  [2026-10-26] agent — old history\n".into(),
        ]);
        assert_eq!(restored["idle-cache"].len(), 1);
        assert!(
            restored["idle-cache"]
                .iter()
                .all(|card| card.state == "pending")
        );
        assert!(
            restored["idle-cache"]
                .iter()
                .any(|card| card.lines.iter().any(|line| line.contains("old history")))
        );
    }
}
