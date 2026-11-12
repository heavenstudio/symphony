//! The Symphony's view of every worktree, as far as the owner's review needs it: the cards an agent
//! wrote on a branch, before the branch lands ([`branch_cards`]).
//!
//! The owner reviews in ONE app, the primary checkout's. A card written on a branch (a worktree's
//! register) is not in main's file until the branch merges, so this finds it: the worktrees whose
//! branch is not merged into main, and in each the register cards that the branch itself added or
//! changed and that differ from main's. A card is listed when it differs from main's AND from the
//! version the branch forked from, so a branch that is only behind main never shows main's newer
//! cards as its own. The native server refreshes these on filesystem events; the cache, stat
//! signature and `owner_lines_left` beside that refresh are tools/python-zero/review-retire's to
//! remove.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::children::command;
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

/// A register card only a branch's worktree holds.
#[derive(Clone, Debug)]
pub struct BranchCard {
    pub tree: Tree,
    /// The page's document name, `docs/DECISIONS.md`, whatever the project's documents folder is
    /// called: [`Project::file`] is the file under the worktree.
    pub doc: String,
    pub card: Decision,
    /// Main's own copy of the card changed after the branch forked: the branch's copy is its own
    /// proposal beside main's, never in its place, so what main's register says still counts.
    pub beside_main: bool,
}

impl BranchCard {
    /// The page path through the app: the document behind the branch's prefix.
    pub fn path(&self) -> String {
        format!("@{}/{}", self.tree.id(), self.doc)
    }
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
pub(crate) fn git(at: &Path, args: &[&str]) -> Option<String> {
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

/// (path, the worktree's text) for each document page of an entrypoint the branch changed since it
/// forked from main, uncommitted or not, or added. A branch already in main has only its
/// uncommitted files.
fn changed_docs(tree: &Tree, is_merged: bool, project: &Project) -> Vec<(String, String)> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    let folders = project.pathspecs();
    let over_folders = |args: &[&str]| {
        let mut all = args.to_vec();
        all.extend(folders.iter().map(String::as_str));
        git(&tree.path, &all).unwrap_or_default()
    };
    if is_merged {
        let status = over_folders(&["status", "--porcelain", "--untracked-files=all", "--"]);
        for line in status.lines() {
            let name = line.get(3..).unwrap_or("");
            names.insert(
                name.rsplit(" -> ")
                    .next()
                    .unwrap_or(name)
                    .trim_matches('"')
                    .to_string(),
            );
        }
    } else {
        let base =
            git(&tree.path, &["merge-base", &project.default_branch, "HEAD"]).unwrap_or_default();
        if base.trim().is_empty() {
            return Vec::new();
        }
        let diff = over_folders(&["diff", "--name-only", base.trim(), "--"]);
        names.extend(diff.lines().map(str::to_string));
        let others = over_folders(&["ls-files", "--others", "--exclude-standard", "--"]);
        names.extend(others.lines().map(str::to_string));
    }
    names
        .into_iter()
        .filter(|n| project.is_page(n) && tree.path.join(n).is_file())
        .filter_map(|n| {
            let text = crate::text::read_text(&tree.path.join(&n)).ok()?;
            Some((n, text))
        })
        .collect()
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

/// The cards of every register as the branch found them when it forked from main.
fn fork_shapes(tree: &Tree, is_merged: bool, project: &Project) -> Arc<Shapes> {
    let base = if is_merged {
        "HEAD".to_string()
    } else {
        git(&tree.path, &["merge-base", &project.default_branch, "HEAD"])
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if base.is_empty() {
        return Arc::new(HashMap::new());
    }
    let specs: Vec<String> = registers(&tree.path, &base, project)
        .iter()
        .map(|n| format!("{base}:{n}"))
        .collect();
    shapes(&blobs(&tree.path, &specs))
}

/// The tree's cards main does not have. A tree that cannot be read lists nothing: one bad worktree
/// never takes the app down.
fn tree_cards(
    tree: &Tree,
    is_merged: bool,
    main_shapes: &Shapes,
    project: &Project,
) -> Vec<BranchCard> {
    let changed = changed_docs(tree, is_merged, project);
    if changed.is_empty() {
        return Vec::new();
    }
    let forked = fork_shapes(tree, is_merged, project);
    let mut found = Vec::new();
    for (name, text) in changed {
        for card in cards(&text) {
            let held = shape(&card);
            if forked.get(&card.slug).is_some_and(|s| s.contains(&held)) {
                continue; // the branch did not touch it (it may have moved it to another file)
            }
            let main = main_shapes.get(&card.slug);
            if main.is_some_and(|s| s.contains(&held)) {
                continue; // identical to main's, in whichever file
            }
            // main answered or changed its copy since the fork, so main's is not the older one
            let beside_main = main.is_some_and(|main| {
                !forked
                    .get(&card.slug)
                    .is_some_and(|fork| main.iter().any(|s| fork.contains(s)))
            });
            found.push(BranchCard {
                tree: tree.clone(),
                doc: project.route(&name).unwrap_or_else(|| name.clone()),
                card,
                beside_main,
            });
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

/// Every card the unmerged branches' worktrees hold that main does not, in branch then page order.
pub fn branch_cards(root: &Path) -> Vec<BranchCard> {
    let mut all = trees(root);
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
                    let found =
                        tree_cards(tree, in_main.contains(&tree.branch), &main_shapes, &project);
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
    for card in &listed {
        every
            .entry(card.card.slug.clone())
            .or_default()
            .insert(shape(&card.card));
    }
    let mut out = Vec::new();
    let mut seen: HashSet<(String, Shape)> = HashSet::new();
    for card in listed {
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
