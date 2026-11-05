//! Worktree and version coverage uses native boundaries, owned fixtures and
//! command counts rather than host wall-clock speed.

mod common;
#[path = "after_worktrees/owner_save.rs"]
mod owner_save;
#[path = "after_worktrees/routes.rs"]
mod routes;

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};
use symphony_board::sys::{Command, Spawning as _};

use common::Tree;
use serde_json::json;
use symphony::{branches, docview, render::Render, versions};
use symphony_api::{Document, DocumentChange};

const DOC: &str = "docs/DECISIONS.md";
const CHILD: &str = "SF_AFTER_WORKTREES_CHILD";

fn command(root: &Path, args: &[&str], day: u8) -> String {
    let date = format!("2026-09-{day:02}T10:00:00+0000");
    let done = symphony_board::sys::output(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_NAME", "Test fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", date),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

fn commit(root: &Path, subject: &str, day: u8) -> String {
    command(root, &["add", "-A"], day);
    command(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", subject],
        day,
    );
    command(root, &["rev-parse", "--short", "HEAD"], day)
}

fn repository() -> Tree {
    let tree = Tree::new();
    command(&tree.root, &["init", "-q", "-b", "main"], 1);
    command(&tree.root, &["config", "core.abbrev", "12"], 1);
    tree
}

fn register(body: &str) -> String {
    format!("# Decisions\n\n<!-- register:start -->\n{body}<!-- register:end -->\n")
}

fn card(slug: &str) -> String {
    format!("- [ ] **{slug}** — Proposal {slug}.\n  [2026-09-01 09:00] agent — Posted.\n")
}

fn branch(tree: &Tree, name: &str, folder: &str) -> std::path::PathBuf {
    let path = tree.path(folder);
    let exclude = tree.path(".git/info/exclude");
    let mut ignored = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(exclude)
        .unwrap();
    writeln!(ignored, "/{folder}/").unwrap();
    command(
        &tree.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            name,
            path.to_str().unwrap(),
            "main",
        ],
        2,
    );
    path
}

#[test]
fn owner_verdict_read_preserves_stale_merged_and_unmerged_indexes() {
    use std::fs::{self, File, FileTimes};
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    let tree = repository();
    tree.write(DOC, "# Decisions\n");
    tree.write("docs/OTHER.md", "# Other\n");
    commit(&tree.root, "base", 1);
    let wt = branch(&tree, "tools/writer", "writer");
    command(&wt, &["config", "diff.autoRefreshIndex", "true"], 1);
    let index = PathBuf::from(command(
        &wt,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        1,
    ));
    for unmerged in [false, true] {
        if unmerged {
            let blob = command(&wt, &["rev-parse", "HEAD:docs/OTHER.md"], 1);
            let mut child = symphony_board::sys::spawn_piped(
                Command::new("git")
                    .args(["update-index", "--index-info"])
                    .current_dir(&wt),
            )
            .unwrap();
            writeln!(
                child.stdin.take().unwrap(),
                "100644 {blob} 2\tdocs/CONFLICT.md"
            )
            .unwrap();
            let done = symphony_board::sys::wait_output(child).unwrap();
            assert!(done.status.success(), "{done:?}");
        }
        assert_eq!(command(&wt, &["ls-files", "-u"], 1).is_empty(), !unmerged);
        fs::write(
            wt.join(DOC),
            "# Decisions\n[2026-10-27 10:00] owner · approved — keep this answer\n",
        )
        .unwrap();
        let tracked = wt.join("docs/OTHER.md");
        let touched = UNIX_EPOCH + Duration::from_secs(1_000_000 + u64::from(unmerged));
        assert_ne!(fs::metadata(&tracked).unwrap().modified().unwrap(), touched);
        File::open(&tracked)
            .unwrap()
            .set_times(FileTimes::new().set_modified(touched))
            .unwrap();
        assert_eq!(fs::metadata(&tracked).unwrap().modified().unwrap(), touched);
        File::open(&index)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(2_000_000)))
            .unwrap();
        let before = fs::read(&index).unwrap();
        let modified = fs::metadata(&index).unwrap().modified().unwrap();

        assert_eq!(symphony::landing::owner_lines_left(&wt), [DOC]);
        assert_eq!(fs::read(&index).unwrap(), before, "unmerged={unmerged}");
        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), modified);
    }
    assert!(symphony::landing::owner_lines_left(&tree.path("gone")).is_empty());
}

fn executable(tree: &Tree, rel: &str, text: &str) {
    let path = tree.write(rel, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Environment-dependent public APIs run in an owned copy of this test process. No test changes
/// the suite's PATH/board configuration, and no read or background filer can reach the shared board.
fn isolated(name: &str, body: impl FnOnce(&Tree)) {
    if std::env::var(CHILD).as_deref() == Ok(name) {
        body(&Tree::new());
        return;
    }
    let start = Instant::now();
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, name)
            .env("TASKS_SERVER", "127.0.0.1:1")
            .env("SF_TASKS_CLI", "/usr/bin/false")
            .env_remove("TASKS_SESSION")
            .env_remove("TASKS_DATABASE")
            .env_remove("TASKS_NOW")
            .env_remove("ASKS_DB"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    eprintln!(
        "{name}: {:.3}s\n{}",
        start.elapsed().as_secs_f64(),
        String::from_utf8_lossy(&done.stdout)
    );
}

fn post(
    saves: &symphony::save::Saves,
    scope: &str,
    route: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    saves
        .post(scope, route, &payload)
        .unwrap_or_else(|failure| panic!("save failed with {}: {}", failure.status, failure.error))
}

#[test]
fn branch_decisions_share_ranking_counts_and_navigation_with_main() {
    isolated(
        "branch_decisions_share_ranking_counts_and_navigation_with_main",
        |_| {
            let tree = repository();
            // the candidates a card shows are drawn by the project's card kind
            tree.write("symphony.toml", &common::candidates::candidates(""));
            tree.write(DOC, &register(&(card("alpha") + &card("behind"))));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "design/policy", "policy");
            let beta = card("beta")
                .replace("Proposal beta.", "Proposal beta. *Blocks:* alpha")
                .replace(
                    "Posted.",
                    "Posted. ![Candidate A](/artifacts/proposal.png) ![Candidate B](proposal.png)",
                );
            std::fs::write(
                wt.join(DOC),
                register(&(card("alpha") + &card("behind") + &beta)),
            )
            .unwrap();
            tree.write(
                DOC,
                &register(&(card("alpha") + &card("behind").replace("[ ]", "[x]"))),
            );
            commit(&tree.root, "main decided inherited card", 3);
            let cards = branches::branch_cards(&tree.root);
            assert_eq!(
                cards
                    .iter()
                    .map(|c| c.card.slug.as_str())
                    .collect::<Vec<_>>(),
                ["beta"]
            );
            let snapshot = symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            assert_eq!(snapshot.board.counts.waiting, 2);
            assert_eq!(snapshot.board.counts.docs_waiting, 1);
            assert_eq!(snapshot.board.progress.decisions_waiting, 2);
            assert_eq!(
                snapshot
                    .queue
                    .waiting
                    .iter()
                    .map(|c| c.slug.as_str())
                    .collect::<Vec<_>>(),
                ["beta", "alpha"]
            );
            assert_eq!(snapshot.board.next, snapshot.queue.waiting);
            assert!(snapshot.queue.branches.is_empty());
            let proposed = &snapshot.queue.waiting[0];
            assert_eq!(proposed.href(), "/@design~policy/docs/DECISIONS.md#beta");
            assert_eq!(proposed.task.as_deref(), Some("design/policy"));
            assert_eq!(
                proposed.pictures[0].src.as_deref(),
                Some("/@design~policy/artifacts/proposal.png")
            );
            assert_eq!(
                proposed.pictures[1].src.as_deref(),
                Some("/@design~policy/docs/proposal.png")
            );
            tree.write("policy/docs/data/items.json", "{unfinished");
            let with_unfinished_data =
                symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            assert_eq!(with_unfinished_data.queue.waiting, snapshot.queue.waiting);
            assert_eq!(snapshot.pending.items.len(), 2);
            assert!(
                snapshot
                    .pending
                    .items
                    .iter()
                    .any(|c| c.href == proposed.href())
            );
            let saves = symphony::save::Saves::start(&tree.root);
            let before = std::fs::read_to_string(wt.join(DOC)).unwrap();
            let later = post(
                &saves,
                "design~policy",
                "/api/postpone",
                json!({"path":DOC, "slug":"beta", "action":"later"}),
            );
            assert_eq!(later["next"], "/docs/DECISIONS.md#alpha");
            let postponed = symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            assert_eq!(postponed.board.counts.waiting, 2);
            assert_eq!(
                postponed
                    .queue
                    .waiting
                    .iter()
                    .map(|c| (c.slug.as_str(), c.postponed))
                    .collect::<Vec<_>>(),
                [("alpha", false), ("beta", true)]
            );
            assert_eq!(postponed.board.next, postponed.queue.waiting);
            assert!(!wt.join(symphony::postpone::FILE).exists());
            assert_eq!(std::fs::read_to_string(wt.join(DOC)).unwrap(), before);
            let undone = post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":"@design~policy/docs/DECISIONS.md", "slug":"beta", "action":"undo"}),
            );
            assert_eq!(undone["changed"], true);
            assert_eq!(
                symphony::dashboard::snapshot(&tree.root, &cards, "test")
                    .unwrap()
                    .queue
                    .waiting,
                snapshot.queue.waiting
            );
            let later = post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":DOC, "slug":"alpha", "action":"later"}),
            );
            assert_eq!(later["next"], "/@design~policy/docs/DECISIONS.md#beta");
            post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":DOC, "slug":"alpha", "action":"undo"}),
            );
            post(
                &saves,
                "design~policy",
                "/api/postpone",
                json!({"path":DOC, "slug":"beta", "action":"later"}),
            );
            post(
                &saves,
                "design~policy",
                "/api/decision",
                json!({"path":DOC, "slug":"beta", "state":"approved", "note":"Use this policy"}),
            );
            saves.close();
            let order: serde_json::Value = serde_json::from_slice(
                &std::fs::read(tree.path(symphony::postpone::FILE)).unwrap(),
            )
            .unwrap();
            assert!(order["owners"]["owner"]["@design~policy/docs/DECISIONS.md#beta"].is_null());
            assert!(
                std::fs::read_to_string(wt.join(DOC))
                    .unwrap()
                    .contains("owner · approved — Use this policy")
            );
            assert!(!tree.read(DOC).contains("Use this policy"));
            let updated = symphony::dashboard::snapshot(
                &tree.root,
                &branches::branch_cards(&tree.root),
                "test",
            )
            .unwrap();
            assert_eq!(updated.board.counts.waiting, 1);
            assert_eq!(updated.board.progress.decisions_waiting, 1);
            assert_eq!(updated.queue.waiting[0].slug, "alpha");
        },
    );
}

#[test]
fn a_branch_card_in_a_projects_own_folder_is_linked_read_and_saved_by_its_route() {
    routes::branch_card_in_project_folder();
}

#[test]
fn branch_documents_mark_source_changes_in_place_and_clear_when_main_catches_up() {
    isolated(
        "branch_documents_mark_source_changes_in_place_and_clear_when_main_catches_up",
        |_| {
            use symphony_api::Difference;
            let tree = repository();
            let rel = "docs/POLICY.md";
            let before = "# Policy\n\nUnchanged.\n\n## Changes\n\nOld policy.\n\n## Gone\n\nRemoved paragraph.\n\n## Stable\n\nKeep this.\n";
            let after = "# Policy\n\nInserted before unchanged.\n\nUnchanged.\n\n## Changes\n\nNew policy.\n\n## Stable\n\nKeep this.\n\n## Added\n\nNew paragraph. See [other](OTHER.md#details).\n";
            tree.write(rel, before);
            tree.write("docs/OTHER.md", "# Other\n\n## Details\n\nDetails.\n");
            tree.write("docs/TAIL.md", "# Tail\n\nRetained.\n\nRemove last.\n");
            tree.write(
                "docs/CARDS.md",
                &format!(
                    "# Cards\n\n{}\n{}",
                    card("kept").replace("Posted.", "Old agent detail."),
                    card("removed")
                ),
            );
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "design/policy", "policy");
            std::fs::write(wt.join(rel), after).unwrap();
            let render = Render::new(&wt);
            std::fs::write(
                wt.join("docs/CARDS.md"),
                format!(
                    "# Cards\n\n{}\n{}",
                    card("inserted"),
                    card("kept").replace("Proposal kept.", "Revised proposal.")
                ),
            )
            .unwrap();
            let cards = docview::document(&render, "docs/CARDS.md", "design~policy").unwrap();
            let kept = cards
                .sections
                .iter()
                .find(|s| s.key == "card:kept")
                .unwrap();
            assert_eq!(kept.comparison.as_ref().unwrap().kind, Difference::Changed);
            let previous = serde_json::to_string(&kept.comparison).unwrap();
            assert!(previous.contains("Old agent detail."));
            assert!(previous.contains("Proposal kept."));
            assert!(cards.sections.iter().any(|s| s.key == "card:inserted"
                && s.comparison.as_ref().unwrap().kind == Difference::Added));
            assert!(
                cards
                    .sections
                    .iter()
                    .any(|s| s.key == "removed:card:removed"
                        && s.comparison.as_ref().unwrap().kind == Difference::Removed)
            );
            std::fs::write(wt.join("docs/TAIL.md"), "# Tail\n\nRetained.\n").unwrap();
            let tail = docview::document(&render, "docs/TAIL.md", "design~policy").unwrap();
            assert_eq!(
                tail.sections
                    .last()
                    .unwrap()
                    .comparison
                    .as_ref()
                    .unwrap()
                    .kind,
                Difference::Removed
            );
            std::fs::write(wt.join("docs/TAIL.md"), "").unwrap();
            let empty = docview::document(&render, "docs/TAIL.md", "design~policy").unwrap();
            assert_eq!(empty.sections.len(), 3);
            assert!(empty.sections.iter().all(
                |s| s.start == 0 && s.comparison.as_ref().unwrap().kind == Difference::Removed
            ));
            assert!(docview::outline(empty).anchors.values().all(Vec::is_empty));
            let doc = docview::document(&render, rel, "design~policy").unwrap();
            assert_eq!(doc.comparison.as_ref().unwrap().task, "design/policy");
            let changed = doc
                .sections
                .iter()
                .filter_map(|s| s.comparison.as_ref().map(|c| (s, c)))
                .collect::<Vec<_>>();
            for kind in [Difference::Added, Difference::Changed, Difference::Removed] {
                assert!(changed.iter().any(|(_, c)| c.kind == kind), "{kind:?}");
            }
            for words in ["Unchanged.", "Keep this."] {
                let section = doc
                    .sections
                    .iter()
                    .find(|s| s.html.contains(words))
                    .unwrap();
                assert_eq!(
                    section.comparison, None,
                    "insertion must not cascade into {words}"
                );
            }
            let replacement = doc
                .sections
                .iter()
                .find(|s| s.html.contains("New policy."))
                .unwrap();
            assert_eq!(
                replacement.comparison.as_ref().unwrap().kind,
                Difference::Changed
            );
            assert!(
                serde_json::to_string(&replacement.comparison)
                    .unwrap()
                    .contains("Old policy.")
            );
            let removed = doc
                .sections
                .iter()
                .position(|s| s.html.contains("Removed paragraph."))
                .unwrap();
            let stable = doc
                .sections
                .iter()
                .position(|s| s.html.contains("Keep this."))
                .unwrap();
            assert!(removed < stable);
            assert_eq!(
                doc.sections[removed].comparison.as_ref().unwrap().kind,
                Difference::Removed
            );
            assert!(
                doc.sections
                    .iter()
                    .any(|s| s.html.contains("/@design~policy/docs/OTHER.md#details"))
            );
            let outline = docview::outline(doc.clone());
            assert!(outline.document.sections.iter().all(|s| s.blocks.is_empty()
                && s.comparison.as_ref().is_none_or(|c| c.before.is_empty())));
            let loaded = docview::selected(&doc, std::slice::from_ref(&replacement.key), 7);
            assert!(
                !loaded.change.sections[0]
                    .comparison
                    .as_ref()
                    .unwrap()
                    .before
                    .is_empty()
            );
            let identical = docview::document(&render, "docs/OTHER.md", "design~policy").unwrap();
            assert_eq!(identical.comparison, None);
            assert!(identical.sections.iter().all(|s| s.comparison.is_none()));
            std::fs::write(wt.join("docs/NEW.md"), "# New\n\nA new document.\n").unwrap();
            let added = docview::document(&render, "docs/NEW.md", "design~policy").unwrap();
            assert!(!added.sections.is_empty());
            assert!(
                added
                    .sections
                    .iter()
                    .all(|s| s.comparison.as_ref().unwrap().kind == Difference::Added)
            );
            tree.write(rel, after);
            commit(&tree.root, "main catches up", 3);
            let updated = docview::document(&render, rel, "design~policy").unwrap();
            assert_eq!(updated.comparison, None);
            assert!(updated.sections.iter().all(|s| s.comparison.is_none()));
            let delta = delta_roundtrip(&doc, &updated);
            assert_eq!(delta.comparison, Some(Vec::new()));
            assert!(delta.deleted.iter().any(|key| key.starts_with("removed:")));
        },
    );
}

#[test]
fn revised_cards_count_once_and_answered_branch_questions_stay_in_navigation() {
    isolated(
        "revised_cards_count_once_and_answered_branch_questions_stay_in_navigation",
        |_| {
            let tree = repository();
            tree.write(DOC, &register(&card("alpha")));
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "design/policy", "policy");
            let question = "- [?] **answered** — A policy question.\n  [2026-09-01 09:00] agent — Posted.\n  [2026-09-01 10:00] owner · question — Why?\n  [2026-09-01 11:00] agent — Here is the explanation.\n";
            std::fs::write(
                wt.join(DOC),
                register(&(card("alpha").replace("Proposal alpha.", "Revised alpha.") + question)),
            )
            .unwrap();
            tree.write(DOC, &register(&(card("alpha") + &card("main-only"))));
            commit(&tree.root, "new main card", 3);
            let cards = branches::branch_cards(&tree.root);
            assert_eq!(cards.len(), 2);
            assert_eq!(
                cards
                    .iter()
                    .find(|c| c.card.slug == "answered")
                    .unwrap()
                    .card
                    .group(&common::plain()),
                symphony::Group::Confirm
            );
            let snapshot = symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            assert_eq!(snapshot.board.counts.waiting, 3);
            assert_eq!(snapshot.board.counts.docs_waiting, 1);
            assert_eq!(snapshot.board.progress.decisions_waiting, 3);
            let revised = snapshot
                .queue
                .waiting
                .iter()
                .filter(|c| c.slug == "alpha")
                .collect::<Vec<_>>();
            assert_eq!(revised.len(), 1);
            assert_eq!(revised[0].doc, "@design~policy/docs/DECISIONS.md");
            assert!(revised[0].title.contains("Revised alpha"));
            let answered = snapshot
                .queue
                .waiting
                .iter()
                .find(|c| c.slug == "answered")
                .unwrap();
            assert_eq!(answered.why, "your question, answered by an agent");
            let pending = snapshot
                .pending
                .items
                .iter()
                .map(|c| c.href.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                pending,
                [
                    "/docs/DECISIONS.md#main-only",
                    "/@design~policy/docs/DECISIONS.md#alpha",
                    "/@design~policy/docs/DECISIONS.md#answered"
                ]
            );
        },
    );
}

#[test]
fn distinct_branch_revisions_keep_their_counts_actions_and_navigation_after_one_is_approved() {
    isolated(
        "distinct_branch_revisions_keep_their_counts_actions_and_navigation_after_one_is_approved",
        |_| {
            let tree = repository();
            let base = register(&(card("alpha") + &card("other")));
            tree.write(DOC, &base);
            commit(&tree.root, "base", 1);
            for (name, folder, text) in [
                ("design/a", "a", "Revision A."),
                ("design/b", "b", "Revision B."),
            ] {
                let wt = branch(&tree, name, folder);
                std::fs::write(wt.join(DOC), base.replace("Proposal alpha.", text)).unwrap();
            }
            let a = "@design~a/docs/DECISIONS.md";
            let b = "@design~b/docs/DECISIONS.md";
            let snapshot = || {
                symphony::dashboard::snapshot(
                    &tree.root,
                    &branches::branch_cards(&tree.root),
                    "test",
                )
                .unwrap()
            };
            assert_eq!(branches::branch_cards(&tree.root).len(), 2);
            let initial = snapshot();
            assert_eq!(initial.board.counts.waiting, 3);
            assert_eq!(initial.board.counts.docs_waiting, 1);
            assert_eq!(initial.board.progress.decisions_waiting, 3);
            assert_eq!(
                initial
                    .queue
                    .waiting
                    .iter()
                    .filter(|c| c.slug == "alpha")
                    .map(|c| c.doc.as_str())
                    .collect::<Vec<_>>(),
                [a, b]
            );
            assert_eq!(
                initial
                    .pending
                    .items
                    .iter()
                    .map(|c| c.href.as_str())
                    .collect::<Vec<_>>(),
                [
                    "/docs/DECISIONS.md#other",
                    "/@design~a/docs/DECISIONS.md#alpha",
                    "/@design~b/docs/DECISIONS.md#alpha"
                ]
            );
            let saves = symphony::save::Saves::start(&tree.root);
            post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":DOC, "slug":"other", "action":"later"}),
            );
            let next = post(
                &saves,
                "design~a",
                "/api/postpone",
                json!({"path":DOC, "slug":"alpha", "action":"later"}),
            );
            assert_eq!(next["next"], "/@design~b/docs/DECISIONS.md#alpha");
            let postponed = snapshot();
            assert!(
                postponed
                    .queue
                    .waiting
                    .iter()
                    .find(|c| c.doc == a)
                    .unwrap()
                    .postponed
            );
            assert!(
                !postponed
                    .queue
                    .waiting
                    .iter()
                    .find(|c| c.doc == b)
                    .unwrap()
                    .postponed
            );
            post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":a, "slug":"alpha", "action":"undo"}),
            );
            post(
                &saves,
                "",
                "/api/postpone",
                json!({"path":DOC, "slug":"other", "action":"undo"}),
            );
            post(
                &saves,
                "design~b",
                "/api/postpone",
                json!({"path":DOC, "slug":"alpha", "action":"later"}),
            );
            // Opening B is not a visit to A merely because both proposals share a slug.
            post(
                &saves,
                "design~b",
                "/api/postpone",
                json!({"path":DOC, "slug":"alpha", "action":"seen"}),
            );
            assert!(
                snapshot()
                    .queue
                    .waiting
                    .iter()
                    .find(|c| c.doc == b)
                    .unwrap()
                    .postponed
            );
            post(
                &saves,
                "design~a",
                "/api/decision",
                json!({"path":DOC, "slug":"alpha", "state":"approved", "note":"Choose A"}),
            );
            saves.close();
            let after = snapshot();
            assert_eq!(after.board.counts.waiting, 2);
            assert_eq!(after.board.counts.docs_waiting, 1);
            assert_eq!(after.board.progress.decisions_waiting, 2);
            assert_eq!(
                after
                    .queue
                    .waiting
                    .iter()
                    .filter(|c| c.slug == "alpha")
                    .map(|c| (c.doc.as_str(), c.postponed))
                    .collect::<Vec<_>>(),
                [(b, true)]
            );
            assert_eq!(
                after
                    .pending
                    .items
                    .iter()
                    .map(|c| c.href.as_str())
                    .collect::<Vec<_>>(),
                [
                    "/docs/DECISIONS.md#other",
                    "/@design~b/docs/DECISIONS.md#alpha"
                ]
            );
            assert!(
                tree.read("a/docs/DECISIONS.md")
                    .contains("owner · approved — Choose A")
            );
            assert!(!tree.read("b/docs/DECISIONS.md").contains("Choose A"));
            assert_eq!(tree.read(DOC), base);
        },
    );
}

#[test]
fn prose_rewrites_follow_the_base_rule_past_main_and_branch_insertions() {
    isolated(
        "prose_rewrites_follow_the_base_rule_past_main_and_branch_insertions",
        |_| {
            for main_first in [true, false] {
                for branch_first in [true, false] {
                    let tree = repository();
                    let rel = "docs/RULES.md";
                    tree.write(rel, "# Rules\n\nOriginal rule.\n");
                    commit(&tree.root, "base", 1);
                    let wt = branch(&tree, "design/policy", "proposal");
                    let paragraphs = |first, note, rule| {
                        if first {
                            format!("# Rules\n\n{note}\n\n{rule}\n")
                        } else {
                            format!("# Rules\n\n{rule}\n\n{note}\n")
                        }
                    };
                    std::fs::write(
                        wt.join(rel),
                        paragraphs(
                            branch_first,
                            "A new branch-only note.",
                            "Branch revised the rule.",
                        ),
                    )
                    .unwrap();
                    tree.write(
                        rel,
                        &paragraphs(
                            main_first,
                            "Main inserted this note later.",
                            "Main revised the rule.",
                        ),
                    );
                    commit(&tree.root, "main edits and inserts", 3);
                    let doc = docview::document(&Render::new(&wt), rel, "design~policy").unwrap();
                    let rule = doc
                        .sections
                        .iter()
                        .find(|s| s.html.contains("Branch revised the rule."))
                        .unwrap();
                    assert_eq!(
                        rule.comparison.as_ref().unwrap().kind,
                        symphony_api::Difference::Changed
                    );
                    let previous =
                        serde_json::to_string(&rule.comparison.as_ref().unwrap().before).unwrap();
                    assert!(
                        previous.contains("Main revised the rule."),
                        "{main_first}/{branch_first}: {previous}"
                    );
                    assert!(!previous.contains("Main inserted this note later."));
                    let note = doc
                        .sections
                        .iter()
                        .find(|s| s.html.contains("A new branch-only note."))
                        .unwrap();
                    assert_eq!(
                        note.comparison.as_ref().unwrap().kind,
                        symphony_api::Difference::Added
                    );
                    assert!(note.comparison.as_ref().unwrap().before.is_empty());
                    assert!(!doc.sections.iter().any(|s| s.comparison.is_some()
                        && s.html.contains("Main inserted this note later.")));
                }
            }
        },
    );
}

#[test]
fn branch_comparisons_exclude_main_only_edits_verdicts_and_additions_after_fork() {
    isolated(
        "branch_comparisons_exclude_main_only_edits_verdicts_and_additions_after_fork",
        |_| {
            use symphony::{docview, render::Render};
            use symphony_api::Difference;
            let tree = repository();
            let rel = "docs/MIXED.md";
            let base = format!(
                "# Mixed\n\n## Main\n\nOriginal main paragraph.\n\n## Task\n\nOriginal task paragraph.\n\n## Removal\n\nDelete this.\n\n## Addition\n\n{}",
                card("alpha")
            );
            tree.write(rel, &base);
            tree.write("docs/UNTOUCHED.md", "# Untouched\n\nOld policy.\n");
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "design/policy", "policy");
            let proposed = base
                .replace("Original task paragraph.", "Revised task paragraph.")
                .replace("Delete this.\n\n", "")
                .replace("## Addition\n\n", "## Addition\n\nBranch addition.\n\n")
                + &card("beta");
            std::fs::write(wt.join(rel), &proposed).unwrap();
            commit(&wt, "proposal", 2);
            tree.write(
                rel,
                &(base
                    .replace("Original main paragraph.", "Main edited this later.")
                    .replace(
                        "Original task paragraph.",
                        "Main also revised the task paragraph.",
                    )
                    .replace("## Addition\n\n", "## Addition\n\nMain addition.\n\n")
                    .replace("[ ] **alpha**", "[x] **alpha**")
                    + &card("gamma")),
            );
            tree.write(
                "docs/UNTOUCHED.md",
                "# Untouched\n\nMain changed this after the fork.\n",
            );
            commit(&tree.root, "main moves", 3);
            let render = Render::new(&wt);
            let doc = docview::document(&render, rel, "design~policy").unwrap();
            assert!(doc.comparison.is_some());
            for section in doc
                .sections
                .iter()
                .filter(|s| s.key == "card:alpha" || s.html.contains("Original main paragraph."))
            {
                assert_eq!(
                    section.comparison, None,
                    "{} is unchanged from the fork",
                    section.key
                );
            }
            assert!(doc.sections.iter().any(|s| s.key == "card:alpha"));
            assert!(
                !doc.sections
                    .iter()
                    .any(|s| s.key == "removed:card:gamma" || s.html.contains("Main addition."))
            );
            let changed = doc
                .sections
                .iter()
                .find(|s| s.html.contains("Revised task paragraph."))
                .unwrap();
            assert_eq!(
                changed.comparison.as_ref().unwrap().kind,
                Difference::Changed
            );
            assert!(
                serde_json::to_string(&changed.comparison)
                    .unwrap()
                    .contains("Main also revised the task paragraph.")
            );
            let added = doc
                .sections
                .iter()
                .find(|s| s.html.contains("Branch addition."))
                .unwrap();
            assert_eq!(added.comparison.as_ref().unwrap().kind, Difference::Added);
            assert!(doc.sections.iter().any(|s| s.html.contains("Delete this.")
                && s.comparison.as_ref().unwrap().kind == Difference::Removed));
            let untouched =
                docview::document(&render, "docs/UNTOUCHED.md", "design~policy").unwrap();
            assert_eq!(untouched.comparison, None);
            assert!(untouched.sections.iter().all(|s| s.comparison.is_none()));
        },
    );
}

#[test]
fn scan_225_worktrees_has_bounded_commands_and_parallel_waits() {
    // Re-enter with a process-local PATH, so interception stays at the real subprocess boundary.
    if std::env::var(CHILD).as_deref() == Ok("scan") {
        let root = std::env::var("SF_SCAN_ROOT").unwrap();
        let found = branches::branch_cards(Path::new(&root));
        assert_eq!(found.len(), 10);
        assert_eq!(
            found
                .iter()
                .map(|c| c.card.slug.as_str())
                .collect::<Vec<_>>(),
            (0..10).map(|n| format!("beta{n}")).collect::<Vec<_>>()
        );
        return;
    }
    let tree = repository();
    tree.write(DOC, &register(&card("alpha")));
    // Reading every document instead of just registers incurs 300 extra blobs in this workload.
    for n in 0..300 {
        tree.write(
            &format!("docs/plain/page{n}.md"),
            &format!("# Plain {n}\n\n{}", "Ordinary prose.\n\n".repeat(350)),
        );
    }
    commit(&tree.root, "base", 1);
    let base = command(&tree.root, &["rev-parse", "HEAD"], 1);
    for n in 0..10 {
        let path = branch(&tree, &format!("tools/live{n}"), &format!("live{n}"));
        std::fs::write(
            path.join(DOC),
            register(&(card("alpha") + &card(&format!("beta{n}")))),
        )
        .unwrap();
        commit(&path, "branch card", 2);
    }
    let mut listing = command(&tree.root, &["worktree", "list", "--porcelain"], 2);
    listing.push_str("\n\n");
    // Detached checkouts need no copied files: any command/read in one is already a regression.
    for n in 0..215 {
        let path = tree.path(&format!("detached{n}"));
        std::fs::create_dir_all(&path).unwrap();
        listing.push_str(&format!(
            "worktree {}\nHEAD {base}\ndetached\n\n",
            path.display()
        ));
    }
    assert_eq!(listing.matches("worktree ").count(), 226); // 225 secondary trees plus main
    tree.write("listing", &listing);
    for fifo in ["arrived", "release"] {
        let done = std::process::Command::new("mkfifo")
            .arg(tree.path(fifo))
            .status_locked()
            .unwrap();
        assert!(done.success());
    }
    let open_fifo = |name| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(tree.path(name))
            .unwrap()
    };
    let arrivals = open_fifo("arrived");
    let mut arrivals_guard = arrivals.try_clone().unwrap();
    let mut release = open_fifo("release");
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(arrivals);
        for _ in 0..8 {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            send.send(line).unwrap();
        }
    });
    executable(
        &tree,
        "bin/git",
        r#"#!/bin/sh
set -eu
# Configuration is a global option, before the subcommand. Keep it on the real execution while
# counting/gating the operation itself, including readers that disable automatic index refresh.
operation="$*"
case "$operation" in
    '-c diff.autoRefreshIndex=false '*) operation="${operation#-c diff.autoRefreshIndex=false }" ;;
esac
printf '%s\n' "$operation" >> "$PWD/scan-runs"
printf '50\n' >> "$PWD/scan-costs"
if [ "${operation%% *}" = worktree ]; then
    exec /bin/cat "$SF_SCAN_ROOT/listing"
fi
if [ "${operation%% *}" = merge-base ] && [ ! -f "$PWD/scan-started" ]; then
    : > "$PWD/scan-started"
    printf '%s\n' "$PWD" > "$SF_SCAN_ROOT/arrived"
    read -r ready < "$SF_SCAN_ROOT/release"
fi
if [ "${operation%% *}" = cat-file ]; then
    /bin/cat > "$PWD/scan-blobs"
    while IFS= read -r spec; do
        printf '5\n' >> "$PWD/scan-costs"
    done < "$PWD/scan-blobs"
    exec /usr/bin/git "$@" < "$PWD/scan-blobs"
fi
exec /usr/bin/git "$@"
"#,
    );
    let path = std::env::join_paths(
        std::iter::once(tree.path("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut child = symphony_board::sys::spawn_output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "scan_225_worktrees_has_bounded_commands_and_parallel_waits",
                "--nocapture",
            ])
            .env(CHILD, "scan")
            .env("SF_SCAN_ROOT", &tree.root)
            .env("PATH", path),
    )
    .unwrap();
    let mut active = std::collections::BTreeSet::new();
    for _ in 0..8 {
        match receive.recv_timeout(Duration::from_secs(30)) {
            Ok(path) => {
                active.insert(path);
            }
            Err(error) => {
                // Unblock our intercepted children and the reader even when concurrency regresses.
                release
                    .write_all(b"go\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\n")
                    .unwrap();
                child.kill().unwrap();
                let _ = child.wait();
                arrivals_guard
                    .write_all(b"abort\nabort\nabort\nabort\nabort\nabort\nabort\nabort\n")
                    .unwrap();
                reader.join().unwrap();
                panic!("scan must start eight independent worktrees before awaiting any: {error}");
            }
        }
    }
    // Release the first eight and the two remaining workers, with no timing sleeps.
    release
        .write_all(b"go\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\n")
        .unwrap();
    reader.join().unwrap();
    let done = symphony_board::sys::wait_output(child).unwrap();
    assert_eq!(active.len(), 8);
    assert!(
        done.status.success(),
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    let root_runs = tree.read("scan-runs");
    assert_eq!(
        root_runs.lines().collect::<Vec<_>>(),
        [
            "worktree list --porcelain",
            "for-each-ref --merged main --format=%(refname:short) refs/heads",
            "grep -l -e <!-- register:start --> main -- docs",
            "cat-file --batch",
        ]
    );
    assert_eq!(tree.read("scan-blobs"), "main:docs/DECISIONS.md\n");
    let mut calls = root_runs.lines().count();
    let mut blobs = tree.read("scan-blobs").lines().count();
    let cost = |text: String| {
        text.lines()
            .map(|line| line.parse::<usize>().unwrap())
            .sum::<usize>()
    };
    let mut charged_ms = cost(tree.read("scan-costs"));
    for n in 0..10 {
        let runs = tree.read(&format!("live{n}/scan-runs"));
        assert_eq!(
            runs.lines().collect::<Vec<_>>(),
            [
                "merge-base main HEAD".into(),
                format!("diff --name-only {base} -- docs"),
                "ls-files --others --exclude-standard -- docs".into(),
                "merge-base main HEAD".into(),
                format!("grep -l -e <!-- register:start --> {base} -- docs"),
                "cat-file --batch".into(),
            ]
        );
        assert_eq!(
            tree.read(&format!("live{n}/scan-blobs")),
            format!("{base}:docs/DECISIONS.md\n")
        );
        calls += runs.lines().count();
        blobs += tree.read(&format!("live{n}/scan-blobs")).lines().count();
        charged_ms += cost(tree.read(&format!("live{n}/scan-costs")));
    }
    for n in 0..215 {
        assert!(
            std::fs::read_dir(tree.path(&format!("detached{n}")))
                .unwrap()
                .next()
                .is_none()
        );
    }
    assert_eq!(calls, 64);
    assert_eq!(blobs, 11);
    // Inject a 50 ms process-start cost and 5 ms/blob read cost. Enumerating plain pages or
    // launching per-file subprocesses exceeds this same five-second budget under any host load.
    assert!(charged_ms < 5_000, "scan cost: {charged_ms}ms");
    eprintln!(
        "225 secondary trees, 300 plain pages: {calls} commands, 11 blobs, injected cost {charged_ms}ms; eight concurrent worktrees handshook"
    );
}

#[test]
fn vanished_worktree_is_skipped_while_surviving_tree_cards_still_list() {
    let tree = repository();
    tree.write(DOC, &register(&card("alpha")));
    commit(&tree.root, "base", 1);
    let gone = branch(&tree, "tools/gone", "gone");
    let kept = branch(&tree, "tools/kept", "kept");
    for (path, slug) in [(&gone, "gone-card"), (&kept, "kept-card")] {
        std::fs::write(path.join(DOC), register(&(card("alpha") + &card(slug)))).unwrap();
        commit(path, "branch proposal", 2);
    }
    assert_eq!(
        branches::branch_cards(&tree.root)
            .iter()
            .map(|found| found.card.slug.as_str())
            .collect::<Vec<_>>(),
        ["gone-card", "kept-card"]
    );
    let missing_path = gone.canonicalize().unwrap();
    std::fs::remove_dir_all(&gone).unwrap();
    assert!(!gone.exists());
    let listing = command(&tree.root, &["worktree", "list", "--porcelain"], 2);
    assert!(
        listing.contains(&format!("worktree {}\n", missing_path.display())),
        "the missing checkout must still be registered: {listing}"
    );
    assert_eq!(
        branches::trees(&tree.root)
            .iter()
            .map(|found| found.branch.as_str())
            .collect::<Vec<_>>(),
        ["tools/kept"]
    );
    assert_eq!(
        branches::branch_cards(&tree.root)
            .iter()
            .map(|found| (
                found.tree.branch.as_str(),
                found.doc.as_str(),
                found.card.slug.as_str()
            ))
            .collect::<Vec<_>>(),
        [("tools/kept", DOC, "kept-card")]
    );
}

#[derive(Default)]
struct Filing {
    rows: RefCell<Vec<symphony::verdicts::BoardTask>>,
    links: RefCell<Vec<String>>,
}

impl symphony::verdicts::Board for Filing {
    fn now(&self) -> f64 {
        1_800_000_000.0
    }
    fn tasks(
        &self,
    ) -> Result<Vec<symphony::verdicts::BoardTask>, symphony::verdicts::VerdictError> {
        Ok(self.rows.borrow().clone())
    }
    fn app_session(&self) -> Result<Option<String>, symphony::verdicts::VerdictError> {
        Ok(Some("test-app".into()))
    }
    fn register_app(&self) -> Result<String, symphony::verdicts::VerdictError> {
        panic!("session already registered")
    }
    fn say(&self, _: &str, _: &str, _: &str) -> Result<(), symphony::verdicts::VerdictError> {
        panic!("a covered card must not be filed twice")
    }
    fn create(
        &self,
        slug: &str,
        title: &str,
        _: &str,
        body: &str,
        links: &[String],
    ) -> Result<(), symphony::verdicts::VerdictError> {
        *self.links.borrow_mut() = links.to_vec();
        self.rows.borrow_mut().push(symphony::verdicts::BoardTask {
            slug: slug.into(),
            title: title.into(),
            body: body.into(),
            status: "ready".into(),
            created: self.now(),
            executors: 0,
            reserved: 0,
            thread: Vec::new(),
        });
        Ok(())
    }
}

#[test]
fn branch_verdict_saves_its_file_and_files_its_card_not_mains_answered_copy() {
    isolated(
        "branch_verdict_saves_its_file_and_files_its_card_not_mains_answered_copy",
        |_| {
            let tree = repository();
            let gamma = "- [-] **gamma** — Old proposal.\n  [2026-09-01 10:00] owner · rejected — Turrets inward.\n";
            tree.write(DOC, &register(&(card("alpha") + gamma)));
            commit(&tree.root, "base", 1);
            let path = branch(&tree, "pictures/fid", "fid");
            std::fs::write(
                path.join(DOC),
                register(&(card("alpha") + gamma + &card("beta"))),
            )
            .unwrap();
            tree.write(
                DOC,
                &register(
                    &(card("alpha")
                        + gamma
                        + "  [2026-09-02 10:00] agent — Redrawn on **pictures/gamma**.\n"),
                ),
            );
            commit(&tree.root, "main answered gamma", 3);
            let before = tree.read(DOC);
            let main_tip = command(&tree.root, &["rev-parse", "main"], 3);
            let saves = symphony::save::Saves::start(&tree.root);
            assert_eq!(
                post(
                    &saves,
                    "",
                    "/api/decision",
                    json!({"path":"@pictures~fid/docs/DECISIONS.md", "slug":"beta", "state":"rejected", "note":"too dark"})
                ),
                json!({"ok":true})
            );
            saves.close();
            let written = std::fs::read_to_string(path.join(DOC)).unwrap();
            assert!(written.contains("owner · rejected — too dark"));
            assert!(written.contains("- [-] **beta**"));
            assert_eq!(tree.read(DOC), before);
            assert_eq!(command(&tree.root, &["rev-parse", "main"], 3), main_tip);
            let board = Filing::default();
            let open = symphony::verdicts::untasked(&tree.root, &board).unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(
                (open[0].slug.as_str(), open[0].path.as_str()),
                ("beta", "@pictures~fid/docs/DECISIONS.md")
            );
            assert_eq!(open[0].quotes, ["too dark"]);
            assert!(
                symphony::verdicts::file(&tree.root, &board, "test-app")
                    .unwrap()
                    .is_some()
            );
            assert_eq!(board.rows.borrow().len(), 1);
            let rows = board.rows.borrow();
            assert!(rows[0].body.contains("**beta**"));
            assert!(rows[0].body.contains("too dark"));
            assert!(!rows[0].body.contains("gamma"));
            assert_eq!(*board.links.borrow(), ["@pictures~fid/docs/DECISIONS.md"]);
            drop(rows);
            assert_eq!(
                symphony::verdicts::file(&tree.root, &board, "test-app").unwrap(),
                None
            );
        },
    );
}

#[test]
fn versions_and_patches_cover_first_commit_owner_verdict_and_invalid_revisions() {
    let tree = repository();
    tree.write(DOC, &register(&card("alpha")));
    let first = commit(&tree.root, "First proposal", 1);
    let initial = versions::diff(&tree.root, DOC, &first).unwrap();
    assert!(initial.diff.contains("--- /dev/null\n"));
    assert!(initial.diff.contains("+- [ ] **alpha**"));
    tree.write("docs/OTHER.md", "# Unrelated secret\n");
    let unrelated = commit(&tree.root, "Other document", 2);
    let answered = symphony::edit::set_decision(
        &tree.read(DOC),
        "alpha",
        "approved",
        "yes, shorter",
        "2026-09-03 10:00",
        &common::plain(),
    )
    .unwrap();
    tree.write(DOC, &answered);
    tree.write("docs/OTHER.md", "# Unrelated changed secret\n");
    let verdict = commit(&tree.root, "Owner verdict", 3);
    let rows = versions::versions(&tree.root, DOC, 0);
    assert_eq!(
        rows.iter()
            .map(|r| (r.sha.as_str(), r.date.as_str(), r.subject.as_str()))
            .collect::<Vec<_>>(),
        [
            (verdict.as_str(), "2026-09-03", "Owner verdict"),
            (first.as_str(), "2026-09-01", "First proposal")
        ]
    );
    assert_eq!(versions::versions(&tree.root, DOC, 1), rows[..1]);
    let patch = versions::diff(&tree.root, DOC, &verdict).unwrap();
    assert_eq!(patch.subject, "Owner verdict");
    assert!(patch.diff.contains("-- [ ] **alpha**"));
    assert!(patch.diff.contains("+- [x] **alpha**"));
    assert!(
        patch
            .diff
            .contains("+  [2026-09-03 10:00] owner · approved — yes, shorter")
    );
    assert!(!patch.diff.contains("Unrelated"));
    assert!(!patch.diff.contains("OTHER.md"));
    assert_eq!(versions::subject(&tree.root, &verdict), "Owner verdict");
    assert_eq!(versions::diff(&tree.root, DOC, &unrelated), None);
    for bad in [
        "--output=x",
        "HEAD; ls",
        "HEAD",
        "",
        "zz",
        "../x",
        "abc",
        "ABCD",
        "deadbeef",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert_eq!(versions::diff(&tree.root, DOC, bad), None, "{bad}");
        assert_eq!(versions::subject(&tree.root, bad), "", "{bad}");
    }
    let blob = command(&tree.root, &["rev-parse", "HEAD:docs/DECISIONS.md"], 3);
    assert_eq!(versions::subject(&tree.root, &blob), "");
    assert_eq!(versions::diff(&tree.root, DOC, &blob), None);
    tree.write("docs/NEW.md", "# Untracked\n");
    assert!(versions::versions(&tree.root, "docs/NEW.md", 0).is_empty());
    assert!(versions::versions(&Tree::new().root, DOC, 0).is_empty());
    tree.write("docs/RAW.md", "# Raw\nNo trailing newline");
    let raw = commit(&tree.root, "Raw document", 4);
    let patch = versions::diff(&tree.root, "docs/RAW.md", &raw)
        .unwrap()
        .diff;
    assert!(patch.contains("+No trailing newline\n\\ No newline at end of file\n"));
    command(&tree.root, &["rm", DOC], 5);
    let deleted = commit(&tree.root, "Remove proposal", 5);
    let patch = versions::diff(&tree.root, DOC, &deleted).unwrap().diff;
    assert!(patch.contains("+++ /dev/null\n"));
    assert!(patch.contains("-- [x] **alpha**"));
    assert_eq!(versions::versions(&tree.root, DOC, 1)[0].sha, deleted);
}

#[test]
fn edited_rename_through_merge_ignores_unrelated_rename_noise() {
    let tree = repository();
    let body = (0..40)
        .map(|n| format!("Stable deck procedure line {n}.\n"))
        .collect::<String>();
    tree.write("docs/DECK.md", &format!("# Deck\n{body}Old menu entry.\n"));
    for n in 0..180 {
        tree.write(
            &format!("noise/old{n}.md"),
            &format!(
                "# Noise {n}\n{}Old ending.\n",
                format!("Unrelated file {n}.\n").repeat(40)
            ),
        );
    }
    let first = commit(&tree.root, "Deck added", 1);
    command(&tree.root, &["switch", "-qc", "menu"], 1);
    tree.write(
        "docs/DECK.md",
        &format!("# Deck\n{body}NEW GAME menu entry.\n"),
    );
    let edit = commit(&tree.root, "Deck menu edit", 2);
    command(&tree.root, &["switch", "-q", "main"], 2);
    std::fs::create_dir_all(tree.path("docs/engineering")).unwrap();
    command(
        &tree.root,
        &["mv", "docs/DECK.md", "docs/engineering/DECK.md"],
        3,
    );
    tree.write(
        "docs/engineering/DECK.md",
        &format!("# Deck\nNew directory note.\n{body}Old menu entry.\n"),
    );
    for n in 0..180 {
        let old = format!("noise/old{n}.md");
        let new = format!("noise/new{n}.md");
        std::fs::rename(tree.path(&old), tree.path(&new)).unwrap();
        tree.write(
            &new,
            &format!(
                "# Noise {n}\n{}New ending.\n",
                format!("Unrelated file {n}.\n").repeat(40)
            ),
        );
    }
    let moved = commit(&tree.root, "Large edited rename", 3);
    command(
        &tree.root,
        &[
            "-c",
            "commit.gpgsign=false",
            "merge",
            "-q",
            "--no-ff",
            "menu",
            "-m",
            "Merge menu",
        ],
        4,
    );
    let merge = command(&tree.root, &["rev-parse", "--short", "HEAD"], 4);
    let rel = "docs/engineering/DECK.md";
    let rows = versions::versions(&tree.root, rel, 0);
    assert_eq!(
        rows.iter().map(|v| v.sha.as_str()).collect::<Vec<_>>(),
        [moved.as_str(), edit.as_str(), first.as_str()]
    );
    let patch = versions::diff(&tree.root, rel, &edit).unwrap().diff;
    assert!(patch.contains("-Old menu entry.\n+NEW GAME menu entry."));
    assert!(!patch.contains("Noise"));
    let renamed = versions::diff(&tree.root, rel, &moved).unwrap().diff;
    assert!(renamed.contains("rename from docs/DECK.md\nrename to docs/engineering/DECK.md"));
    assert!(renamed.contains("+New directory note."));
    assert_eq!(versions::diff(&tree.root, rel, &merge), None);
}

#[test]
fn historical_deck_edit_7a0e58cd_survives_merge_a915f81c() {
    // Borrow immutable objects, but pin HEAD in our own repo. This neither walks today's moving
    // main nor depends on a checked-out copy of the historical source tree.
    let tree = repository();
    let common = std::env::var("SF_GIT_COMMON_DIR").unwrap_or_else(|_| {
        command(
            &common::repo(),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            1,
        )
    });
    tree.write(
        ".git/objects/info/alternates",
        &format!("{common}/objects\n"),
    );
    command(
        &tree.root,
        &[
            "update-ref",
            "refs/heads/main",
            "a915f81cc5745219ebcfec849daae88a5cb2f192",
        ],
        1,
    );
    let rel = "docs/engineering/DECK.md";
    let rows = versions::versions(&tree.root, rel, 0);
    assert!(
        rows.iter().any(|v| v.sha.len() >= 7
            && "7a0e58cd74db704e183b9fbcf707158cdb6cad3b".starts_with(&v.sha)
            && v.subject.starts_with("view/main-menu:")),
        "{rows:?}"
    );
    let patch = versions::diff(&tree.root, rel, "7a0e58cd").unwrap();
    assert!(patch.diff.contains("NEW GAME"));
    assert!(patch.diff.contains("NEW VOYAGE"));
    assert!(!patch.diff.contains("view/ui_title.gd"));
}

#[test]
fn scoped_reads_without_cards_preserve_links_sections_and_thumbnail_fallback() {
    isolated(
        "scoped_reads_without_cards_preserve_links_sections_and_thumbnail_fallback",
        |tree| {
            tree.write("docs/README.md", "# Docs\n");
            tree.write("docs/guide/README.md", "# Guide\n");
            let rel = "docs/guide/PLAIN.md";
            tree.write(rel, "# Plain *guide*\n\nSee [local](OTHER.md#part), [heading](#details), [outside](https://example.com), and ![figure](picture.png).\n\n## Details\n\nPlain words.\n");
            tree.write("docs/guide/OTHER.md", "# Other\n\n## Part\n\nElsewhere.\n");
            let mut png = vec![0; 130_000];
            png[..24].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x04\0\0\0\x02\0");
            std::fs::write(tree.path("docs/guide/picture.png"), png).unwrap();
            let render = Render::new(&tree.root);
            let doc = docview::document(&render, rel, "tools~plain").unwrap();
            assert_eq!(doc.path, "@tools~plain/docs/guide/PLAIN.md");
            assert_eq!(doc.title, "Plain guide");
            assert!(doc.cards.is_empty());
            assert!(doc.waiting.is_empty());
            assert!(doc.threads.is_empty());
            assert_eq!(
                doc.crumbs[1].href.as_deref(),
                Some("/@tools~plain/docs/guide/")
            );
            let fragment = docview::section(&render, rel, "details", "tools~plain").unwrap();
            assert!(fragment.html.contains("Plain words."));
            assert!(!fragment.blocks.is_empty());
            assert!(docview::section(&render, rel, "missing", "tools~plain").is_err());
            let html = doc
                .sections
                .iter()
                .map(|s| s.html.as_str())
                .collect::<String>();
            assert!(
                html.contains("href=\"/@tools~plain/docs/guide/OTHER.md#part\""),
                "{html}"
            );
            assert!(html.contains("href=\"/@tools~plain/docs/guide/PLAIN.md#details\""));
            assert!(html.contains("href=\"https://example.com\""));
            // The fallback is explicitly injected, independent of whether this host has sips.
            let fallback = symphony::images::reserve(
                "<img src=\"/docs/guide/picture.png\">",
                &tree.root,
                false,
                false,
            );
            assert!(fallback.contains("src=\"/docs/guide/picture.png\""));
            assert!(fallback.contains("width=\"1024\" height=\"512\""));
            assert!(!fallback.contains("/thumb/"));
            let convert = |_: &Path, _: &Path, _: u32| panic!("unavailable converter must not run");
            assert!(
                symphony::images::thumbnail(
                    &tree.root,
                    540,
                    "stale",
                    "docs/guide/picture.png",
                    &convert,
                    false
                )
                .is_none()
            );
            let folder = docview::folder(&render, "docs/guide", "tools~plain").unwrap();
            assert_eq!(folder.docs, 2);
            assert!(
                folder
                    .entries
                    .iter()
                    .all(|e| e.path.starts_with("@tools~plain/docs/guide/"))
            );
            for invalid in [
                "docs/../README.md",
                "docs/missing.md",
                "/docs/guide/PLAIN.md",
                "docs/guide/picture.png",
            ] {
                assert!(
                    docview::document(&render, invalid, "").is_err(),
                    "{invalid}"
                );
            }
        },
    );
}

fn apply(mut old: Document, delta: &DocumentChange) -> Document {
    old.sections.retain(|s| !delta.deleted.contains(&s.key));
    for section in &delta.sections {
        if let Some(held) = old.sections.iter_mut().find(|s| s.key == section.key) {
            *held = section.clone();
        } else {
            old.sections.push(section.clone());
        }
    }
    for pos in &delta.positions {
        let section = old.sections.iter_mut().find(|s| s.key == pos.key).unwrap();
        (section.start, section.end, section.part) = (pos.start, pos.end, pos.part);
    }
    if let Some(order) = &delta.order {
        old.sections
            .sort_by_key(|s| order.iter().position(|k| k == &s.key).unwrap());
    }
    for (key, value) in &delta.threads {
        if let Some(value) = value {
            old.threads.insert(key.clone(), value.clone());
        } else {
            old.threads.remove(key);
        }
    }
    for card in &delta.cards {
        old.cards.retain(|c| c.slug != card.key);
        if let Some(value) = &card.value {
            old.cards.push(value.clone());
        }
    }
    macro_rules! field { ($($name:ident),*) => { $(if let Some(value) = &delta.$name { old.$name = value.clone(); })* }; }
    field!(title, waiting, crumbs, headings, tasks);
    if let Some(comparison) = &delta.comparison {
        old.comparison = comparison.first().cloned();
    }
    old
}

fn gzip_size(bytes: &[u8]) -> usize {
    let mut child = symphony_board::sys::spawn_piped(Command::new("gzip").arg("-c")).unwrap();
    let mut input = child.stdin.take().unwrap();
    // Written beside the read: gzip answers while it reads, and a pipe holds only so much.
    let done = std::thread::scope(|scope| {
        let written = scope.spawn(move || input.write_all(bytes));
        let done = symphony_board::sys::wait_output(child).unwrap();
        written.join().unwrap().unwrap();
        done
    });
    assert!(done.status.success());
    done.stdout.len()
}

fn delta_roundtrip(before: &Document, after: &Document) -> DocumentChange {
    let delta = docview::changes(before, after).expect("the fixture made an observable change");
    // Check the actual wire format, not just in-process compatibility HTML skipped by serde.
    let bytes = serde_json::to_vec(&delta).unwrap();
    let zipped = gzip_size(&bytes);
    assert!(zipped <= 4096, "delta is {zipped} bytes gzip");
    let wire: DocumentChange = serde_json::from_slice(&bytes).unwrap();
    let mut applied = apply(before.clone(), &wire);
    applied.cards.sort_by(|a, b| a.slug.cmp(&b.slug));
    let mut wanted = after.clone();
    wanted.cards.sort_by(|a, b| a.slug.cmp(&b.slug));
    assert_eq!(
        serde_json::to_value(applied).unwrap(),
        serde_json::to_value(wanted).unwrap()
    );
    eprintln!(
        "document delta: {} bytes JSON, {zipped} bytes gzip",
        bytes.len()
    );
    delta
}

#[test]
fn keyed_document_deltas_send_only_changed_fragments_within_four_kib_gzip() {
    isolated(
        "keyed_document_deltas_send_only_changed_fragments_within_four_kib_gzip",
        |tree| {
            let rel = "docs/LONG.md";
            let text = format!(
                "# Long\n\n{}",
                (0..160)
                    .map(|n| card(&format!("proposal-{n}")))
                    .collect::<String>()
            );
            tree.write(rel, &text);
            let render = Render::new(&tree.root);
            let before = docview::document(&render, rel, "").unwrap();
            assert_eq!(before.cards.len(), 160);
            assert!(before.sections.len() >= 160);
            assert_eq!(docview::changes(&before, &before), None);
            let replied = symphony::edit::add_message(
                &text,
                "proposal-5",
                "Please clarify.",
                "2026-09-03 10:00",
                symphony::Who::Owner,
                None,
            )
            .unwrap();
            tree.write(rel, &replied);
            let after = docview::document(&render, rel, "").unwrap();
            let delta = delta_roundtrip(&before, &after);
            assert_eq!(
                delta.threads.keys().map(String::as_str).collect::<Vec<_>>(),
                ["proposal-5"]
            );
            assert!(delta.positions.iter().any(|p| p.key == "card:proposal-159"));
            assert!(delta.sections.iter().all(|s| s.key == "card:proposal-5"));
            assert_eq!(delta.order, None);
            let answered = symphony::edit::set_decision(
                &replied,
                "proposal-5",
                "approved",
                "Use the smaller layout.",
                "2026-09-04 10:00",
                &common::plain(),
            )
            .unwrap();
            tree.write(rel, &answered);
            let decided = docview::document(&render, rel, "").unwrap();
            let delta = delta_roundtrip(&after, &decided);
            assert_eq!(delta.cards.len(), 1);
            assert_eq!(delta.cards[0].key, "proposal-5");
            assert_eq!(delta.cards[0].value.as_ref().unwrap().state, "approved");
            assert_eq!(delta.waiting.as_ref().unwrap().len(), 159);
            assert!(delta.sections.iter().all(|s| s.key == "card:proposal-5"));
            let replacement = format!("# Long\n\n{}{}", card("replacement"), card("proposal-5"));
            tree.write(rel, &replacement);
            let removed = docview::document(&render, rel, "").unwrap();
            // Large deletions carry keys/tombstones, not the deleted cards' bodies or threads.
            let delta = delta_roundtrip(&decided, &removed);
            assert_eq!(delta.deleted.len(), 159);
            assert_eq!(delta.cards.iter().filter(|c| c.removed).count(), 159);
            assert_eq!(delta.threads.values().filter(|v| v.is_none()).count(), 159);
            assert!(
                delta
                    .order
                    .as_ref()
                    .unwrap()
                    .contains(&"card:replacement".into())
            );
            let outline = docview::outline(removed.clone());
            assert!(
                outline
                    .document
                    .sections
                    .iter()
                    .all(|s| s.blocks.is_empty() && s.html.is_empty())
            );
            assert!(outline.document.threads.is_empty());
            assert!(
                outline
                    .anchors
                    .values()
                    .flatten()
                    .any(|a| a == "replacement")
            );
            let selected = docview::selected(&removed, &["card:replacement".into()], 42);
            assert_eq!(selected.version, 42);
            assert_eq!(selected.change.sections.len(), 1);
            assert_eq!(selected.change.cards[0].key, "replacement");
            assert_eq!(
                selected
                    .change
                    .threads
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["replacement"]
            );
        },
    );
}

#[test]
fn prose_delta_distinguishes_content_positions_order_and_metadata() {
    isolated(
        "prose_delta_distinguishes_content_positions_order_and_metadata",
        |tree| {
            let rel = "docs/PLAIN.md";
            tree.write(
                rel,
                "# Plain\n\nIntro.\n\n## First\n\nOne.\n\n## Second\n\nTwo.\n",
            );
            let render = Render::new(&tree.root);
            let before = docview::document(&render, rel, "branch").unwrap();
            tree.write(rel, "# Renamed\n\nIntro.\n\n## Second\n\nTwo.\n\n## First\n\nOne changed.\nExtra line.\n");
            let after = docview::document(&render, rel, "branch").unwrap();
            let delta = delta_roundtrip(&before, &after);
            assert_eq!(delta.path, "@branch/docs/PLAIN.md");
            assert_eq!(delta.title.as_deref(), Some("Renamed"));
            assert!(delta.headings.is_some());
            assert!(delta.order.is_some());
            assert!(
                delta
                    .sections
                    .iter()
                    .any(|s| s.html.contains("One changed."))
            );
            assert!(!delta.sections.iter().any(|s| s.html.contains("Two.")));
            assert!(delta.positions.iter().any(|p| p.key.contains("second")));
            assert!(delta.cards.is_empty());
            assert!(delta.threads.is_empty());
        },
    );
}

#[test]
fn branch_asks_keep_document_anchor_quote_and_source_then_reopen_on_reply() {
    isolated(
        "branch_asks_keep_document_anchor_quote_and_source_then_reopen_on_reply",
        |_| {
            let tree = repository();
            tree.write(
                "docs/PLAIN.md",
                "# Plain\n\n## Details\n\nThe selected words.\n",
            );
            commit(&tree.root, "base", 1);
            branch(&tree, "tools/plain", "plain");
            let saves = symphony::save::Saves::start(&tree.root);
            let answer = post(
                &saves,
                "tools~plain",
                "/api/ask",
                json!({"path":"docs/PLAIN.md", "question":"Why these words?", "anchor":"details", "quote":"selected words"}),
            );
            let id = answer["id"].as_i64().unwrap();
            let db = symphony::asks::default_path(&tree.root, None);
            let store = symphony::asks::AskStore::open(&db).unwrap();
            let thread = store.thread(id).unwrap();
            assert_eq!(thread.path, "docs/PLAIN.md");
            assert_eq!(thread.anchor, "details");
            assert_eq!(thread.quote, "selected words");
            assert!(thread.messages[0].text.contains("Why these words?"));
            assert!(
                thread.messages[0]
                    .text
                    .contains("sent from /@tools~plain/docs/PLAIN.md#details")
            );
            store
                .answer(id, "Because of the layout.", "test-worker")
                .unwrap();
            assert!(store.open_asks().unwrap().is_empty());
            assert_eq!(
                post(
                    &saves,
                    "",
                    "/api/ask-reply",
                    json!({"id":id,"text":"Which layout?"})
                )["ok"],
                true
            );
            let opened = store.open_asks().unwrap();
            assert_eq!(opened.len(), 1);
            assert_eq!(opened[0].id, id);
            assert_eq!(opened[0].messages.last().unwrap().text, "Which layout?");
            assert_eq!(store.for_doc("docs/OTHER.md").unwrap().len(), 0);
            saves.close();
        },
    );
}
