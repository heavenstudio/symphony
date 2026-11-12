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
//! branch lists only the owner's lines no commit has, laid on main's copy of the card.

mod one;

pub use one::{markdown, one_card, same, stale, with_owner_lines};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::children::command;
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
    /// worktree holds uncommitted laid on top. On a finished branch, only those owner lines, under
    /// the card's own line.
    pub card: Decision,
    /// The card is main's (the branch is finished, or did not post this card): it adds the owner's
    /// lines its worktree holds to main's copy, which stays main's, and is read on main's page.
    pub on_main: bool,
}

impl BranchCard {
    /// The page path through the app: the document behind the branch's prefix, or main's document
    /// for a finished branch's owner lines.
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
/// branch, else the tasks that record it as their executor's branch or whose executor registered
/// on it.
fn branch_tasks(board: &symphony_board::Board) -> BTreeMap<&str, Vec<&symphony_board::Task>> {
    let mut named: BTreeMap<&str, Vec<&symphony_board::Task>> = BTreeMap::new();
    let mut recorded: BTreeMap<&str, Vec<&symphony_board::Task>> = BTreeMap::new();
    for task in &board.tasks {
        named.insert(task.slug.as_str(), vec![task]);
        let mut branches: Vec<&str> = task.branch.as_deref().into_iter().collect();
        for role in task.roles.iter().filter(|r| r.role == "executor") {
            if let Some(session) = board.sessions.iter().find(|s| s.row.handle == role.handle)
                && !session.row.branch.is_empty()
            {
                branches.push(session.row.branch.as_str());
            }
        }
        for branch in branches {
            let tasks = recorded.entry(branch).or_default();
            if !tasks.iter().any(|t| t.slug == task.slug) {
                tasks.push(task);
            }
        }
    }
    for (branch, tasks) in recorded {
        named.entry(branch).or_insert(tasks);
    }
    named
}

/// The branches whose work is finished on `board`: every task the branch names
/// ([`branch_tasks`]) is done or cancelled. Such a branch lists no card of its own.
pub fn finished(board: &symphony_board::Board) -> BTreeSet<String> {
    branch_tasks(board)
        .into_iter()
        .filter(|(_, tasks)| {
            tasks
                .iter()
                .all(|task| FINISHED.contains(&task.status.as_str()))
        })
        .map(|(branch, _)| branch.to_string())
        .collect()
}

/// Recorded task names, with the branch slug used as the fallback when the board is unavailable.
pub(crate) fn task_names(root: &Path) -> BTreeMap<String, String> {
    crate::taskview::connect(root)
        .and_then(|store| store.board().map_err(|e| e.to_string()))
        .map(|board| {
            let mut names: BTreeMap<_, _> = board
                .tasks
                .iter()
                .map(|task| (task.slug.clone(), task.slug.clone()))
                .collect();
            for task in &board.tasks {
                for role in task.roles.iter().filter(|r| r.role == "executor") {
                    if let Some(session) =
                        board.sessions.iter().find(|s| s.row.handle == role.handle)
                        && !session.row.branch.is_empty()
                    {
                        names
                            .entry(session.row.branch.clone())
                            .or_insert_with(|| task.slug.clone());
                    }
                }
            }
            names
        })
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

/// Read through [`symphony_board::sys::output`]: a child another thread spawns meanwhile (the server's
/// census) never holds the answer open.
fn git(at: &Path, args: &[&str]) -> Option<String> {
    let done = symphony_board::sys::output(
        command("git")
            .args(["-c", "diff.autoRefreshIndex=false"])
            .args(args)
            // A live scan can inspect a verdict worktree while its worker stages/commits it.
            // Read-only discovery must not refresh that index or contend with its writer.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(at),
    )
    .ok()?;
    done.status
        .success()
        .then(|| String::from_utf8_lossy(&done.stdout).to_string())
}

pub fn trees(root: &Path) -> Vec<Tree> {
    let main = Project::of(root).default_branch.clone();
    let listing = git(root, &["worktree", "list", "--porcelain"]).unwrap_or_default();
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
    found
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
/// branch's cards can be.
fn registers(at: &Path, rev: &str, project: &Project) -> Vec<String> {
    let folders = project.pathspecs();
    let mut args = vec!["grep", "-l", "-e", REGION_START, rev, "--"];
    args.extend(folders.iter().map(String::as_str));
    git(at, &args)
        .unwrap_or_default()
        .replace(&format!("{rev}:"), "")
        .split_whitespace()
        .filter(|name| project.is_page(name))
        .map(str::to_string)
        .collect()
}

/// The text at each `<rev>:<path>`, "" where there is none: one `git cat-file --batch` for all of them.
/// Its input and output go through [`symphony_board::sys::spawn_piped`]: it runs before readiness, and a
/// child another thread spawns meanwhile (the census) must hold neither end open.
fn blobs(at: &Path, specs: &[String]) -> Vec<String> {
    let none = || vec![String::new(); specs.len()];
    if specs.is_empty() {
        return Vec::new();
    }
    let Ok(mut child) = symphony_board::sys::spawn_piped(
        command("git").args(["cat-file", "--batch"]).current_dir(at),
    ) else {
        return none();
    };
    let input: String = specs.iter().map(|s| format!("{s}\n")).collect();
    let mut stdin = child.stdin.take().expect("a piped stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let Ok(done) = symphony_board::sys::wait_output(child) else {
        return none();
    };
    let _ = writer.join();
    if !done.status.success() {
        return none();
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
    out
}

/// The document pages of the project's entrypoints that the git command `args` names, one per line,
/// over the project's folders.
fn pages(tree: &Tree, project: &Project, args: &[&str]) -> BTreeSet<String> {
    let folders = project.pathspecs();
    let mut all = args.to_vec();
    all.extend(folders.iter().map(String::as_str));
    git(&tree.path, &all)
        .unwrap_or_default()
        .lines()
        .filter(|name| project.is_page(name))
        .map(str::to_string)
        .collect()
}

/// The pages whose worktree file differs from the branch's HEAD commit: where the owner may have
/// written a line no commit has.
fn unsaved(tree: &Tree, project: &Project) -> BTreeSet<String> {
    pages(tree, project, &["diff", "--name-only", "HEAD", "--"])
}

/// The worktree's text of `name`, none when it cannot be read.
fn worktree_cards(tree: &Tree, name: &str) -> Vec<Decision> {
    crate::text::read_text(&tree.path.join(name))
        .map(|text| cards(&text))
        .unwrap_or_default()
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

impl Fork {
    /// The fork of the checkout at `root`, none when it has no merge base with main.
    pub(crate) fn of(root: &Path) -> Option<Fork> {
        let project = Project::of(root);
        let base = git(root, &["merge-base", &project.default_branch, "HEAD"])?
            .trim()
            .to_string();
        if base.is_empty() {
            return None;
        }
        let specs: Vec<String> = registers(root, &base, &project)
            .iter()
            .map(|n| format!("{base}:{n}"))
            .collect();
        Some(Fork(shapes(&blobs(root, &specs))))
    }

    /// Whether the fork held `card` as it is.
    pub(crate) fn holds(&self, card: &Decision) -> bool {
        self.0
            .get(&card.slug)
            .is_some_and(|copies| copies.contains(&shape(card)))
    }
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

/// A live branch's cards main does not have: each card of a page its commits changed since it forked
/// from main, as its HEAD commit holds it, with the owner's lines its worktree holds uncommitted on
/// top. A card only the worktree's file holds is a draft and is not read; nor is a card waiting on
/// the owner whose pictures are not in the worktree yet. The owner's lines on a card the branch did
/// not post go to main's copy ([`owner_card`]). A tree that cannot be read lists nothing: one bad
/// worktree never takes the app down.
fn live_cards(tree: &Tree, main_shapes: &Shapes, project: &Project) -> Vec<BranchCard> {
    let base = git(&tree.path, &["merge-base", &project.default_branch, "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string();
    if base.is_empty() {
        return Vec::new();
    }
    let posted = pages(tree, project, &["diff", "--name-only", &base, "HEAD", "--"]);
    let unsaved = unsaved(tree, project);
    if posted.is_empty() && unsaved.is_empty() {
        return Vec::new();
    }
    let forked = registers(&tree.path, &base, project);
    let docs: Vec<&String> = posted.union(&unsaved).collect();
    let specs: Vec<String> = forked
        .iter()
        .map(|n| format!("{base}:{n}"))
        .chain(docs.iter().map(|n| format!("HEAD:{n}")))
        .collect();
    let texts = blobs(&tree.path, &specs);
    let (forked_texts, head_texts) = texts.split_at(forked.len().min(texts.len()));
    let forked = shapes(forked_texts);
    let plugins = std::cell::OnceCell::new();
    let mut found = Vec::new();
    for (name, head) in docs.into_iter().zip(head_texts) {
        let doc = project.route(name).unwrap_or_else(|| name.clone());
        let saved = if unsaved.contains(name) {
            worktree_cards(tree, name)
        } else {
            Vec::new()
        };
        for posted in cards(head) {
            let owner = saved
                .iter()
                .find(|w| w.slug == posted.slug)
                .map(|w| one::owner_lines(w, Some(&posted)))
                .unwrap_or_default();
            let copy = shape(&posted);
            // The branch did not post this card: its copy is the fork's (it may have moved it to
            // another file) or main's as it is. It is main's card, and the owner's lines on the
            // branch's copy join main's.
            if forked.get(&posted.slug).is_some_and(|s| s.contains(&copy))
                || main_shapes
                    .get(&posted.slug)
                    .is_some_and(|s| s.contains(&copy))
            {
                if !owner.is_empty() {
                    found.push(owner_card(tree, &doc, posted, &owner));
                }
                continue;
            }
            let card = if owner.is_empty() {
                posted
            } else {
                one::with_lines(&posted, &owner)
            };
            if main_shapes
                .get(&card.slug)
                .is_some_and(|s| s.contains(&shape(&card)))
            {
                continue; // main holds it as it is now, in whichever file
            }
            if !crate::docs_audit::missing_pictures(&tree.path, &doc, &card).is_empty()
                && asks_owner(plugins.get_or_init(|| Plugins::of(&tree.path)), &card)
            {
                continue; // posted before its pictures: it waits until they load
            }
            // Main's copy, answered or changed since the fork or not, is one card with this one
            // ([`crate::dashboard::merge_decisions`]): what main's says still counts.
            found.push(BranchCard {
                tree: tree.clone(),
                doc: doc.clone(),
                card,
                on_main: false,
            });
        }
    }
    found
}

/// The owner's lines `owner` that the worktree of `tree` holds on its copy of `card`, in the page
/// `doc`, to be laid on main's copy of the card: the card's line, slug and text with those lines
/// alone under it.
fn owner_card(tree: &Tree, doc: &str, card: Decision, owner: &[one::Chunk]) -> BranchCard {
    let bare = Decision {
        raw: Vec::new(),
        note: Vec::new(),
        ..card
    };
    BranchCard {
        tree: tree.clone(),
        doc: doc.to_string(),
        card: one::with_lines(&bare, owner),
        on_main: true,
    }
}

/// A finished branch's part: only the owner's lines its worktree holds that its HEAD commit does
/// not, each card's under its own line, to be laid on main's copy of the card.
fn owner_cards(tree: &Tree, project: &Project) -> Vec<BranchCard> {
    let unsaved: Vec<String> = unsaved(tree, project).into_iter().collect();
    if unsaved.is_empty() {
        return Vec::new();
    }
    let specs: Vec<String> = unsaved.iter().map(|n| format!("HEAD:{n}")).collect();
    let heads = blobs(&tree.path, &specs);
    let mut found = Vec::new();
    for (name, head) in unsaved.iter().zip(&heads) {
        let posted = cards(head);
        let doc = project.route(name).unwrap_or_else(|| name.clone());
        for saved in worktree_cards(tree, name) {
            let owner = one::owner_lines(&saved, posted.iter().find(|p| p.slug == saved.slug));
            if !owner.is_empty() {
                found.push(owner_card(tree, &doc, saved, &owner));
            }
        }
    }
    found
}

/// Whether `other` is `held` with more history under it: the same card, answered further. A card
/// asked in two worktrees (stacked branches) is answered in one; the other copy must not ask the
/// owner again.
fn extends(other: &Shape, held: &Shape) -> bool {
    other.text == held.text
        && other.lines.len() > held.lines.len()
        && other.lines[..held.lines.len()] == held.lines[..]
}

/// Every card the live branches posted that main does not hold, and the owner's lines finished
/// branches' worktrees hold uncommitted, in branch then page order. A branch is finished when it is
/// merged into main or the board says its task is done or cancelled; the board is read for that
/// when its server runs (none is started), and a board that cannot be read finishes nothing.
pub fn branch_cards(root: &Path) -> Vec<BranchCard> {
    let all = trees(root);
    if all.is_empty() {
        return Vec::new();
    }
    let finished = crate::taskview::connect_running(root)
        .and_then(|store| store.board().map_err(|e| e.to_string()))
        .map(|board| finished(&board))
        .unwrap_or_default();
    scan_trees(root, all, &finished)
}

/// [`branch_cards`] with the finished branches given: `finished` names the branches whose task is
/// done, as [`finished`] reads them from the board.
pub fn scan(root: &Path, finished: &BTreeSet<String>) -> Vec<BranchCard> {
    scan_trees(root, trees(root), finished)
}

fn scan_trees(root: &Path, mut all: Vec<Tree>, finished: &BTreeSet<String>) -> Vec<BranchCard> {
    all.sort_by(|a, b| a.branch.cmp(&b.branch));
    if all.is_empty() {
        return Vec::new();
    }
    let project = Project::of(root);
    let main = &project.default_branch;
    let in_main = merged(root, main);
    let names = registers(root, main, &project);
    let specs: Vec<String> = {
        let mut sorted = names;
        sorted.sort();
        sorted.iter().map(|n| format!("{main}:{n}")).collect()
    };
    let main_shapes = shapes(&blobs(root, &specs));
    let next = AtomicUsize::new(0);
    let done: Vec<Mutex<Vec<BranchCard>>> = all.iter().map(|_| Mutex::new(Vec::new())).collect();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(all.len()) {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::SeqCst);
                    let Some(tree) = all.get(at) else { break };
                    let found = if in_main.contains(&tree.branch) || finished.contains(&tree.branch)
                    {
                        owner_cards(tree, &project)
                    } else {
                        live_cards(tree, &main_shapes, &project)
                    };
                    *done[at].lock().unwrap_or_else(|e| e.into_inner()) = found;
                }
            });
        }
    });
    let listed: Vec<BranchCard> = done
        .into_iter()
        .flat_map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect();
    let mut every: Shapes = (*main_shapes).clone();
    for card in listed.iter().filter(|c| !c.on_main) {
        every
            .entry(card.card.slug.clone())
            .or_default()
            .insert(shape(&card.card));
    }
    let mut out = Vec::new();
    let mut seen: HashSet<(String, Shape)> = HashSet::new();
    for card in listed {
        if card.on_main {
            out.push(card);
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
            out.push(card);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::shapes;

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
