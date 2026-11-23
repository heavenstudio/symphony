//! A tidy and a reply where the project delivers by pull request (`[land] delivery`): in its
//! checkout on the default branch they write no file, and keep each change as a save for
//! delivery (`symphony::commit::pending::keep`), read as the page shows the saves still waiting
//! and shown on it at once; a task's worktree, and a project that does not deliver, are written
//! as before.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use serde_json::Value;
use symphony::commit::pending;

use crate::common::{Tree, cli_in, status, unmarked};

const DELIVERS: &str = "[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n";
const REGISTER: &str =
    "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
const NOTES: &str = "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n\nrest\n";
/// A ledger as a delivered tidy left one on the default branch.
const LEDGER: &str = "# Review ledger\n\nNewest last.\n";
/// A card the owner asked a question on, and a comment nobody answered yet.
const ASKED: &str = "# D\n\n<!-- register:start -->\n### T\n\n- [?] **ask** — proposal.\n  \
                     [2026-11-22 20:00] owner · question — which one?\n<!-- register:end -->\n";
const COMMENTED: &str = "# N\n\n> [!comment] Odd\n> why this?\n\nrest\n";

/// `git ARGS…` in `root`, by no one's own git settings, asserting it succeeds: what it printed.
fn git(root: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim_end().into()
}

/// A checkout on `main` whose one commit holds `manifest` as its `symphony.toml`, a register
/// with a plain approval and a page with an answered comment: a tidy changes both and starts the
/// ledger.
fn checkout(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write("symphony.toml", manifest);
    tree.write("docs/DECISIONS.md", REGISTER);
    tree.write("docs/NOTES.md", NOTES);
    git(&tree.root, &["init", "-q", "-b", "main"]);
    commit(&tree.root, "the project");
    tree
}

/// Everything in the checkout at `root` committed on its branch.
fn commit(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Tidy test",
            "-c",
            "user.email=tidy-test@example.invalid",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// The file at `path` by its inode: a file replaced whole is another.
fn inode(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().ino()
}

/// What `git status` says of the checkout at `root`, untracked files and all.
fn dirt(root: &Path) -> String {
    git(root, &["status", "--porcelain", "--untracked-files=all"])
}

/// `symphony status --tidy` of the checkout at `root` at `now`, run by the session `session`
/// (none when empty): its exit code, what it printed and what it said on stderr.
fn tidy(root: &Path, now: &str, session: &str) -> (i32, String, String) {
    let mut env: BTreeMap<String, String> = unmarked();
    env.remove("TASKS_SESSION");
    if !session.is_empty() {
        env.insert("TASKS_SESSION".into(), session.into());
    }
    let root = root.display().to_string();
    cli_in(&["status", "--root", &root, "--tidy", "--now", now], &env)
}

/// The saves the journal of the checkout at `root` keeps, in the order kept: each entry whole.
fn journal(root: &Path) -> Vec<Value> {
    let folder = root.join(".git/symphony/server/verdict-publications");
    let mut names: Vec<_> = std::fs::read_dir(folder)
        .map(|read| read.map(|entry| entry.unwrap().path()).collect())
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

/// What each kept save edits, and who it names as saving it.
fn kept(root: &Path) -> Vec<(String, String)> {
    journal(root)
        .iter()
        .map(|entry| {
            let rel = entry["edit"]["rel"].as_str().unwrap().to_string();
            let by = entry["delivery"]["by"].as_str().unwrap_or("").to_string();
            (rel, by)
        })
        .collect()
}

/// The checkout as delivering, a ledger on its default branch: what a tidy keeps all of.
fn ledgered() -> Tree {
    let tree = checkout(DELIVERS);
    tree.write("docs/history/review-ledger.md", LEDGER);
    commit(&tree.root, "the ledger");
    tree
}

#[test]
fn a_tidy_on_the_default_branch_of_a_project_that_delivers_keeps_each_change_and_writes_no_file() {
    let tree = ledgered();
    let register = inode(&tree.path("docs/DECISIONS.md"));
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        "resolved: done\nregrouped docs/DECISIONS.md\narchived: docs/NOTES.md (1)\nkept for \
         delivery: docs/DECISIONS.md (save 1), docs/history/review-ledger.md (save 2), \
         docs/NOTES.md (save 3). This project delivers by pull request ([land] delivery), so a \
         tidy writes no file in a checkout on main: the pages show it now, and a pull request \
         delivers it.\n"
    );
    // Nothing of the checkout written, not even replaced by the same text.
    assert_eq!(tree.read("docs/DECISIONS.md"), REGISTER);
    assert_eq!(tree.read("docs/NOTES.md"), NOTES);
    assert_eq!(tree.read("docs/history/review-ledger.md"), LEDGER);
    let after = inode(&tree.path("docs/DECISIONS.md"));
    assert_eq!(after, register, "the register was replaced");
    assert_eq!(dirt(&tree.root), "");
    // Kept, the ledger before the page its comment leaves, each against the file it edits.
    let names = |rel: &str| (rel.to_string(), "agent".to_string());
    assert_eq!(
        kept(&tree.root),
        [
            names("docs/DECISIONS.md"),
            names("docs/history/review-ledger.md"),
            names("docs/NOTES.md")
        ]
    );
    let entries = journal(&tree.root);
    assert_eq!(entries[0]["edit"]["base"], REGISTER);
    let resolved = entries[0]["edit"]["new"].as_str().unwrap();
    assert!(
        resolved.contains("- [r] **done** — plain.\n  [2026-11-22 21:00] agent · resolved"),
        "{resolved}"
    );
    assert!(
        entries[1]["edit"]["new"]
            .as_str()
            .unwrap()
            .contains("Fixed.")
    );
    assert_eq!(entries[2]["edit"]["new"], "# N\n\nrest\n");
    // Kept together: delivered in one commit or not at all, the ledger never without its pages.
    let first = entries[0]["delivery"]["together"]["first"].clone();
    for entry in &entries {
        assert_eq!(entry["delivery"]["together"]["count"], 3, "{entry}");
        assert_eq!(entry["delivery"]["together"]["first"], first, "{entry}");
    }
    // The page shows it at once, and so does the board the CLI reads.
    let shown = pending::shown(&tree.root, "docs/DECISIONS.md", REGISTER.into());
    assert_eq!(shown, resolved);
    let (code, json, _) = status(&tree.root, &["--json"]);
    assert_eq!(code, 0);
    assert!(json.contains("\"resolved\""), "{json}");
    assert!(
        !json.contains("Odd"),
        "the answered comment is still on its page: {json}"
    );
    // A tidy again reads the pages as those saves leave them: nothing more to keep.
    let (code, out, err) = tidy(&tree.root, "2026-11-22 22:00", "");
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "resolved: (none)\nregrouped docs/DECISIONS.md\n");
    assert_eq!(journal(&tree.root).len(), 3);
}

#[test]
fn a_kept_tidy_leaves_the_answered_comments_on_their_pages_while_main_holds_no_ledger() {
    // A save of a file the default branch does not hold could never be delivered.
    let tree = checkout(DELIVERS);
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.starts_with(
            "resolved: done\nregrouped docs/DECISIONS.md\narchived: none of 1 answered comments, \
             as main does not hold docs/history/review-ledger.md yet: they stay on their pages \
             until a tidy in a task's worktree moves them\nkept for delivery: docs/DECISIONS.md \
             (save 1). "
        ),
        "{out}"
    );
    assert_eq!(
        kept(&tree.root),
        [("docs/DECISIONS.md".into(), "agent".into())]
    );
    assert_eq!(tree.names("docs"), ["DECISIONS.md", "NOTES.md"]);
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn a_ledger_that_cannot_be_written_leaves_every_comment_on_its_page() {
    use std::os::unix::fs::PermissionsExt as _;
    // Written before the pages it takes them from: a ledger refused leaves them where they were.
    let tree = checkout("");
    let history = tree.path("docs/history");
    std::fs::create_dir_all(&history).unwrap();
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(err.contains("review-ledger.md"), "{err}");
    assert_eq!(tree.read("docs/NOTES.md"), NOTES);
    assert!(!out.contains("archived"), "{out}");
}

#[test]
fn a_tidy_there_with_nothing_to_change_says_what_it_always_says_and_replaces_no_file() {
    // The checkout as a tidy delivered leaves it: tidied, committed, and then delivering.
    let tree = checkout("");
    let (code, _, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""));
    tree.write("symphony.toml", DELIVERS);
    commit(&tree.root, "tidied, and delivering");
    let register = inode(&tree.path("docs/DECISIONS.md"));
    let (code, out, err) = tidy(&tree.root, "2026-11-22 22:00", "");
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "resolved: (none)\nregrouped docs/DECISIONS.md\n");
    let after = inode(&tree.path("docs/DECISIONS.md"));
    assert_eq!(after, register, "the register was replaced");
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn the_tidy_in_a_tasks_worktree_writes_it_and_leaves_the_checkout_on_main_as_it_was() {
    let tree = checkout(DELIVERS);
    let worktree = tree.path("../").join(format!(
        "{}-task",
        tree.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&worktree);
    git(
        &tree.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "docs/tidy",
            worktree.to_str().unwrap(),
        ],
    );
    let _gone = Removed(&worktree);
    let (code, out, err) = tidy(&worktree, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let register = std::fs::read_to_string(worktree.join("docs/DECISIONS.md")).unwrap();
    assert!(
        register.contains("- [r] **done** — plain.\n  [2026-11-22 21:00] agent · resolved"),
        "{register}"
    );
    assert!(worktree.join("docs/history/review-ledger.md").is_file());
    assert_eq!(
        dirt(&worktree),
        " M docs/DECISIONS.md\n M docs/NOTES.md\n?? docs/history/review-ledger.md"
    );
    assert_eq!(tree.read("docs/DECISIONS.md"), REGISTER);
    assert_eq!(dirt(&tree.root), "");
}

/// A linked worktree outside the tree, removed with its folder once the test is done.
struct Removed<'a>(&'a Path);

impl Drop for Removed<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

#[test]
fn a_tidy_of_a_project_that_does_not_deliver_writes_its_checkout_on_main_as_before() {
    let tree = checkout("[land]\ncheck = false\n");
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "resolved: done\nregrouped docs/DECISIONS.md\narchived: docs/NOTES.md (1)\n"
    );
    assert!(
        tree.read("docs/DECISIONS.md")
            .contains("- [r] **done** — plain.\n  [2026-11-22 21:00] agent · resolved")
    );
    assert_eq!(tree.read("docs/NOTES.md"), "# N\n\nrest\n");
    assert_eq!(
        dirt(&tree.root),
        " M docs/DECISIONS.md\n M docs/NOTES.md\n?? docs/history/review-ledger.md"
    );
}

#[test]
fn a_delivery_the_default_branch_does_not_commit_holds_nothing_back() {
    // What delivers is the setting the default branch holds, as for an owner's save: one only
    // written in the checkout is not the project's yet.
    let tree = checkout("");
    tree.write("symphony.toml", DELIVERS);
    let (code, _, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(tree.read("docs/DECISIONS.md").contains("- [r] **done**"));
}

/// `symphony reply ARGS…` in process, its session `by` credited in an ask store of its own
/// outside the checkout: its exit code, what it printed and what it said on stderr.
fn reply(asks: &Tree, args: &[&str]) -> (i32, String, String) {
    let mut env: BTreeMap<String, String> = unmarked();
    env.remove("TASKS_SESSION");
    env.insert("ASKS_DB".into(), asks.path("asks.db").display().to_string());
    let mut argv = vec!["reply"];
    argv.extend(args);
    cli_in(&argv, &env)
}

/// A delivering checkout whose register holds a card the owner asked on, and a page with an
/// unanswered comment.
fn asked(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write("symphony.toml", manifest);
    tree.write("docs/DECISIONS.md", ASKED);
    tree.write("docs/NOTES.md", COMMENTED);
    git(&tree.root, &["init", "-q", "-b", "main"]);
    commit(&tree.root, "the project");
    tree
}

/// The agent's line the reply appends under the card `ask`.
const ANSWER: &str = "  [2026-11-22 21:05] agent — The first.\n";

#[test]
fn a_reply_on_the_default_branch_of_a_project_that_delivers_is_kept_and_writes_no_file() {
    let tree = asked(DELIVERS);
    let asks = Tree::new();
    let page = tree.path("docs/DECISIONS.md");
    let before = inode(&page);
    let path = page.to_str().unwrap();
    let (code, out, err) = reply(
        &asks,
        &[
            path,
            "ask",
            "The first.",
            "--now",
            "2026-11-22 21:05",
            "--as",
            "fox-1",
        ],
    );
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(
        out,
        format!(
            "replied on ask in {path}, kept as save 1: this project delivers by pull request, so \
             the page shows it now and a pull request delivers it\n"
        )
    );
    // Main's checkout is left as it was, the file not even replaced.
    assert_eq!(tree.read("docs/DECISIONS.md"), ASKED);
    assert_eq!(inode(&page), before, "the register was replaced");
    assert_eq!(dirt(&tree.root), "");
    // The reply is on its way: kept as a save, credited to its session, whose page shows it.
    assert_eq!(
        kept(&tree.root),
        [("docs/DECISIONS.md".into(), "fox-1".into())]
    );
    let entry = &journal(&tree.root)[0];
    assert_eq!(entry["edit"]["base"], ASKED);
    let answered = ASKED.replace("<!-- register:end", &format!("{ANSWER}<!-- register:end"));
    assert_eq!(entry["edit"]["new"], answered.as_str());
    assert_eq!(
        pending::shown(&tree.root, "docs/DECISIONS.md", ASKED.into()),
        answered
    );
    let saves = pending::saves(&tree.root);
    assert_eq!(saves.len(), 1, "{saves:?}");
    assert_eq!(saves[0].state, pending::State::Queued);
    assert_eq!(saves[0].by, "fox-1");
    let note = pending::note(&tree.root, "docs/DECISIONS.md").unwrap();
    assert!(note.contains("This page shows an agent's save"), "{note}");
    // What waits on an agent, read as the page shows it: the owner's question is answered.
    let (code, open, _) = status(&tree.root, &["--open"]);
    assert_eq!(code, 0);
    assert!(!open.contains("which one?"), "{open}");
    // Who replied is in the ask store, as off main.
    assert!(asks.path("asks.db").is_file());
}

#[test]
fn a_second_reply_there_is_written_after_the_first_as_the_page_shows_it() {
    let tree = asked(DELIVERS);
    let asks = Tree::new();
    let path = tree.path("docs/DECISIONS.md");
    let path = path.to_str().unwrap();
    for (text, now) in [
        ("The first.", "2026-11-22 21:05"),
        ("Done.", "2026-11-22 21:06"),
    ] {
        let (code, _, err) = reply(&asks, &[path, "ask", text, "--now", now]);
        assert_eq!((code, err.as_str()), (0, ""));
    }
    let entries = journal(&tree.root);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["edit"]["base"], entries[0]["edit"]["new"]);
    let both = format!("{ANSWER}  [2026-11-22 21:06] agent — Done.\n");
    assert_eq!(
        pending::shown(&tree.root, "docs/DECISIONS.md", ASKED.into()),
        ASKED.replace("<!-- register:end", &format!("{both}<!-- register:end"))
    );
    // With no session named, an agent's all the same.
    assert_eq!(kept(&tree.root)[1].1, "agent");
    assert_eq!(tree.read("docs/DECISIONS.md"), ASKED);
}

#[test]
fn an_answer_on_a_comment_there_is_kept_the_same_way() {
    let tree = asked(DELIVERS);
    let asks = Tree::new();
    let path = tree.path("docs/NOTES.md");
    let (code, out, err) = reply(&asks, &[path.to_str().unwrap(), "L3", "Because."]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(out.contains("kept as save 1"), "{out}");
    assert_eq!(tree.read("docs/NOTES.md"), COMMENTED);
    let shown = pending::shown(&tree.root, "docs/NOTES.md", COMMENTED.into());
    assert!(shown.contains("> > [!resolved] Because."), "{shown}");
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn a_reply_with_delivery_off_writes_the_file_as_before() {
    let tree = asked("[land]\ncheck = false\n");
    let asks = Tree::new();
    let path = tree.path("docs/DECISIONS.md");
    let path = path.to_str().unwrap();
    let (code, out, err) = reply(
        &asks,
        &[path, "ask", "The first.", "--now", "2026-11-22 21:05"],
    );
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("replied on ask in {path}\n"));
    assert_eq!(
        tree.read("docs/DECISIONS.md"),
        ASKED.replace("<!-- register:end", &format!("{ANSWER}<!-- register:end"))
    );
    assert!(journal(&tree.root).is_empty());
    assert_eq!(dirt(&tree.root), " M docs/DECISIONS.md");
}

#[test]
fn a_reply_in_a_tasks_worktree_of_a_project_that_delivers_writes_the_worktree() {
    let tree = asked(DELIVERS);
    let asks = Tree::new();
    let worktree = tree.path("../").join(format!(
        "{}-reply",
        tree.root.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&worktree);
    git(
        &tree.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "docs/reply",
            worktree.to_str().unwrap(),
        ],
    );
    let _gone = Removed(&worktree);
    let page = worktree.join("docs/DECISIONS.md");
    let (code, out, err) = reply(
        &asks,
        &[
            page.to_str().unwrap(),
            "ask",
            "The first.",
            "--now",
            "2026-11-22 21:05",
        ],
    );
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(!out.contains("kept"), "{out}");
    assert!(std::fs::read_to_string(&page).unwrap().contains(ANSWER));
    assert!(journal(&tree.root).is_empty());
    assert_eq!(dirt(&tree.root), "");
}

/// The checkout's journal folder made unwritable while `run` runs: what a disk that refuses the
/// save leaves.
fn unwritable_journal<T>(root: &Path, run: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt as _;
    let folder = root.join(".git/symphony/server/verdict-publications");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let done = run();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    done
}

#[test]
fn a_reply_that_cannot_be_kept_says_so_and_writes_no_file_either() {
    let tree = asked(DELIVERS);
    let asks = Tree::new();
    let path = tree.path("docs/DECISIONS.md");
    let (code, out, err) = unwritable_journal(&tree.root, || {
        reply(&asks, &[path.to_str().unwrap(), "ask", "The first."])
    });
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("✖ the reply is not kept: "), "{err}");
    assert_eq!(tree.read("docs/DECISIONS.md"), ASKED);
    assert!(journal(&tree.root).is_empty());
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn a_tidy_that_cannot_keep_a_change_says_which_and_writes_no_file_either() {
    let tree = ledgered();
    let (code, out, err) =
        unwritable_journal(&tree.root, || tidy(&tree.root, "2026-11-22 21:00", ""));
    assert_eq!(code, 1, "{out}");
    // Its first entry is written last, so the first write tried is the last file's, and with
    // it refused nothing of the tidy is kept.
    assert!(
        err.starts_with("✖ docs/NOTES.md not kept for delivery: "),
        "{err}"
    );
    assert!(
        err.contains("nothing of this tidy is kept: the pages and main stay as they were"),
        "{err}"
    );
    assert!(!out.contains("kept for delivery"), "{out}");
    assert!(journal(&tree.root).is_empty());
    assert_eq!(tree.read("docs/DECISIONS.md"), REGISTER);
    assert_eq!(tree.read("docs/NOTES.md"), NOTES);
    assert_eq!(dirt(&tree.root), "");
}

#[test]
fn a_tidy_keeps_its_saves_in_the_name_of_the_session_tidying() {
    let tree = ledgered();
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "fox-1");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let by: Vec<String> = kept(&tree.root).into_iter().map(|(_, by)| by).collect();
    assert_eq!(by, ["fox-1", "fox-1", "fox-1"]);
}

#[test]
fn a_tidy_of_one_file_keeps_it_alone() {
    // The register alone changes: nothing goes with it.
    let tree = checkout(DELIVERS);
    let (code, out, err) = tidy(&tree.root, "2026-11-22 21:00", "");
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let entries = journal(&tree.root);
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]["delivery"].get("together").is_none(),
        "{}",
        entries[0]
    );
}
