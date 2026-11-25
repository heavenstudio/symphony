//! Worktree and version coverage uses native boundaries, owned fixtures and
//! command counts rather than host wall-clock speed.

#[path = "after_worktrees/audit.rs"]
mod audit;
mod common;
#[path = "after_worktrees/in_flight.rs"]
mod in_flight;
#[path = "after_worktrees/landed.rs"]
mod landed;
#[path = "after_worktrees/owner.rs"]
mod owner;
#[path = "after_worktrees/owner_save.rs"]
mod owner_save;
#[path = "after_worktrees/posted.rs"]
mod posted;
#[path = "after_worktrees/routes.rs"]
mod routes;
#[path = "after_worktrees/saving.rs"]
mod saving;
// How a test makes a channel ([`test_channel::channel`]).
#[path = "common/test_channel.rs"]
mod test_channel;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};
use symphony_board::sys::{Command, Spawning as _};

use common::Tree;
use in_flight::{Scan, ended_early, in_flight, scan_end};
use serde_json::json;
use symphony::{branches, docview, render::Render, versions};
use symphony_api::{Document, DocumentChange};

const DOC: &str = "docs/DECISIONS.md";
const CHILD: &str = "SYMPHONY_AFTER_WORKTREES_CHILD";

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
            "# Decisions\n- [x] **answer** — Answer.\n  [2026-10-27 10:00] owner · approved — keep this answer\n",
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

#[test]
fn the_consumer_owner_reader_keeps_untracked_answers_and_git_failures() {
    let tree = repository();
    tree.write("spec/D.md", "# D\n");
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"spec\"\nfolder = \"spec\"\n",
    );
    commit(&tree.root, "base", 1);
    let answer = "- [x] **x** — X.\n  [2026-11-14 01:52] owner · approved — go\n";
    tree.write("spec/NEW.md", answer);
    tree.write(DOC, answer);
    assert_eq!(
        symphony::landing::owner_lines_left(&tree.root),
        ["spec/NEW.md"]
    );
    // An unreadable project reads the whole checkout, preserving the answers it cannot scope.
    tree.write("symphony.toml", "[broken\n");
    assert_eq!(
        symphony::landing::owner_lines_left(&tree.root),
        [DOC, "spec/NEW.md"]
    );
    let ordinary = Tree::new();
    let kept = symphony::landing::owner_lines_left(&ordinary.root);
    assert_eq!(kept.len(), 1);
    assert!(
        kept[0].starts_with("owner answers could not be read: git diff failed"),
        "{kept:?}"
    );
}

fn executable(tree: &Tree, rel: &str, text: &str) {
    let path = tree.write(rel, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Environment-dependent public APIs run in an owned copy of this test process. No test changes
/// the suite's PATH/board configuration, and no read or background filer can reach the shared board.
fn isolated(name: &str, body: impl FnOnce(&Tree)) {
    isolated_with(name, &[], body);
}

/// [`isolated`], the child given `vars` as well (a `PATH` whose `git` the test wrote).
fn isolated_with(name: &str, vars: &[(&str, std::ffi::OsString)], body: impl FnOnce(&Tree)) {
    if std::env::var(CHILD).as_deref() == Ok(name) {
        body(&Tree::new());
        return;
    }
    let start = Instant::now();
    let done = symphony_board::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .envs(vars.iter().map(|(name, value)| (*name, value)))
            .env(CHILD, name)
            .env("TASKS_SERVER", "127.0.0.1:1")
            .env("SYMPHONY_TASKS_CLI", "/usr/bin/false")
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
            // A card reaches the owner once it is posted, its pictures on disk.
            tree.write("policy/artifacts/proposal.png", "png");
            tree.write("policy/docs/proposal.png", "png");
            commit(&wt, "post beta", 2);
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
            assert!(!symphony::postpone::order_file(&wt).exists());
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
                &std::fs::read(symphony::postpone::order_file(&tree.root)).unwrap(),
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
fn a_verdict_on_a_landed_branchs_page_is_saved_on_main_and_status_agrees_with_the_page() {
    landed::a_verdict_on_a_landed_branchs_page_is_mains();
}

#[test]
fn a_line_on_mains_card_from_a_live_branchs_page_is_saved_on_main_and_its_own_card_stays_its() {
    landed::a_line_on_mains_card_from_a_live_branchs_page_is_mains();
}

#[test]
fn a_verdict_from_a_gone_worktrees_page_is_saved_on_main_when_main_holds_the_card() {
    landed::a_verdict_from_a_gone_worktrees_page_is_mains();
}

#[test]
fn a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages_and_main_takes_nothing() {
    landed::a_verdict_when_the_worktrees_cannot_be_listed_stays_the_pages();
}

#[test]
fn a_line_on_a_card_the_branch_posted_and_main_closed_since_is_saved_on_main_as_its_page_draws_it()
{
    landed::a_line_on_a_card_main_closed_since_the_branch_posted_it_is_mains();
}

#[test]
fn save_routing_and_page_agree_on_which_cards_main_closed() {
    landed::save_routing_and_page_use_the_registers_closure();
}

#[test]
fn a_finished_revision_and_its_answer_stay_one_card_until_landed() {
    landed::a_finished_revision_stays_one_card_until_its_answer_lands(false);
}

#[test]
fn a_finished_option_revision_and_its_answer_stay_one_card_until_landed() {
    landed::a_finished_revision_stays_one_card_until_its_answer_lands(true);
}

#[test]
fn a_verdict_from_a_gone_worktrees_page_is_judged_on_its_branch_while_it_is_kept() {
    landed::a_verdict_from_a_gone_worktrees_page_reads_its_kept_branch();
}

#[test]
fn a_branch_card_in_a_projects_own_folder_is_linked_read_and_saved_by_its_route() {
    routes::branch_card_in_project_folder();
}

#[test]
fn a_card_reaches_the_owner_once_posted_and_never_from_a_draft_or_a_finished_branch() {
    posted::cards_reach_the_owner_once_posted();
}

#[test]
fn an_inline_card_is_read_queued_answered_from_its_page_and_resolved_where_it_sits() {
    posted::an_inline_card_lives_as_a_registers_does();
}

#[test]
fn an_inline_card_a_branch_posted_is_its_own_and_mains_inline_cards_stay_mains() {
    posted::inline_cards_are_the_branchs_only_when_it_posted_them();
}

#[test]
fn the_owners_unsaved_line_in_a_finished_worktree_shows_on_mains_card_until_main_closes_it_later() {
    posted::owner_lines_reach_main_from_a_finished_worktree();
}

#[test]
fn a_branch_is_finished_when_every_task_it_names_is_done_or_cancelled() {
    posted::finished_branches_follow_the_board();
}

#[test]
fn the_docs_audit_refuses_a_card_whose_pictures_do_not_load() {
    audit::a_card_whose_pictures_do_not_load_is_refused_at_posting();
}

#[test]
fn the_docs_audit_refuses_a_card_picture_this_checkout_wrote_that_git_does_not_track() {
    audit::a_cards_picture_is_a_file_git_tracks();
}

#[test]
fn with_no_fork_the_docs_audit_warns_of_an_untracked_card_picture() {
    audit::with_no_fork_an_untracked_picture_is_warned_of();
}

#[test]
fn the_docs_audit_refuses_a_documents_own_picture_this_checkout_wrote_that_git_does_not_track() {
    audit::a_documents_own_picture_is_a_file_git_tracks();
}

#[test]
fn the_docs_audit_reads_every_form_of_a_picture() {
    audit::every_form_of_a_picture_is_read();
}

#[test]
fn the_docs_audit_of_a_clone_warns_of_a_picture_git_never_tracked() {
    audit::a_clone_is_warned_of_a_picture_git_never_tracked();
}

#[test]
fn an_owner_line_counts_wherever_it_was_saved_until_an_agent_answers() {
    owner::owner_lines_count_until_an_agent_answers();
}

#[test]
fn an_answer_in_a_finished_worktree_shows_under_the_owners_line_on_mains_card_and_its_own() {
    owner::answers_in_a_finished_worktree_stay_under_the_owners_line();
}

#[test]
fn a_verdict_that_closes_a_draft_is_refused_until_its_branch_posts_it() {
    saving::a_closing_verdict_on_a_draft_waits_for_its_post();
}

#[test]
fn a_verdict_that_closes_a_posted_card_its_agent_is_changing_is_refused_until_committed() {
    saving::a_closing_verdict_waits_while_its_agent_changes_a_posted_card();
}

#[test]
fn a_branch_page_a_merge_stopped_on_shows_its_last_committed_copy_and_takes_an_answer() {
    saving::a_merging_worktree_shows_its_last_committed_copy();
}

#[test]
fn while_merging_a_line_git_left_after_a_line_two_cards_share_is_not_the_branch_cards() {
    saving::a_line_git_left_by_a_shared_line_is_not_the_branch_cards();
}

#[test]
fn a_comment_on_a_register_heading_is_refused_and_every_later_verdict_on_its_cards_saves() {
    saving::a_comment_on_a_register_heading_leaves_its_cards_answerable();
}

#[test]
fn a_branch_answer_to_a_main_card_with_a_picture_only_main_holds_reaches_the_owner() {
    owner::branch_answer_reaches_the_owner_whatever_mains_pictures();
}

#[test]
fn an_owners_approval_saved_in_the_minute_of_the_answer_it_follows_closes_the_card() {
    owner::a_verdict_in_the_minute_of_the_answer_decides();
}

#[test]
fn a_branch_page_marks_its_drafts_and_draws_mains_card_for_every_card_it_did_not_post() {
    owner::branch_page_marks_drafts_and_draws_mains_cards();
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
            let cards = docview::document(&render, "docs/CARDS.md", "design~policy", &[]).unwrap();
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
            let tail = docview::document(&render, "docs/TAIL.md", "design~policy", &[]).unwrap();
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
            let empty = docview::document(&render, "docs/TAIL.md", "design~policy", &[]).unwrap();
            assert_eq!(empty.sections.len(), 3);
            assert!(empty.sections.iter().all(
                |s| s.start == 0 && s.comparison.as_ref().unwrap().kind == Difference::Removed
            ));
            assert!(docview::outline(empty).anchors.values().all(Vec::is_empty));
            let doc = docview::document(&render, rel, "design~policy", &[]).unwrap();
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
            let identical =
                docview::document(&render, "docs/OTHER.md", "design~policy", &[]).unwrap();
            assert_eq!(identical.comparison, None);
            assert!(identical.sections.iter().all(|s| s.comparison.is_none()));
            std::fs::write(wt.join("docs/NEW.md"), "# New\n\nA new document.\n").unwrap();
            let added = docview::document(&render, "docs/NEW.md", "design~policy", &[]).unwrap();
            assert!(!added.sections.is_empty());
            assert!(
                added
                    .sections
                    .iter()
                    .all(|s| s.comparison.as_ref().unwrap().kind == Difference::Added)
            );
            tree.write(rel, after);
            commit(&tree.root, "main catches up", 3);
            let updated = docview::document(&render, rel, "design~policy", &[]).unwrap();
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
            commit(&wt, "post the revision and the answer", 2);
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

/// The owner's register on 2026-11-11 16:2x, as main held it: `reports-small-choices` never answered
/// and `desktop-client` asked at 15:26 and answered at 15:44 by an agent ending in a question, while
/// the card's branch kept an older copy with a question the owner left there at 14:34. The branch
/// never posted the card, so it is main's: one card, with the owner's line from the branch's copy.
#[test]
fn a_card_main_answered_since_the_fork_counts_as_mains_with_the_owners_line_from_its_branch_copy() {
    isolated(
        "a_card_main_answered_since_the_fork_counts_as_mains_with_the_owners_line_from_its_branch_copy",
        |_| {
            let desktop = "### Desktop client\n\n- [ ] **desktop-client** — A Symphony app for the Mac.\n  [2026-11-11 14:07] agent — Posted for the board task `product/desktop-client`.\n\n    **A, a thin shell.** Each server's window loads that server's own pages.\n  [2026-11-11 14:33] agent — Corrections to the entry above, after a review of its clarity.\n";
            let tree = repository();
            tree.write(
                DOC,
                &register(&format!("## Needs your decision\n\n{desktop}\n")),
            );
            commit(&tree.root, "base", 1);
            let wt = branch(&tree, "product/desktop-client", "desktop");
            std::fs::write(wt.join("docs/analysis.md"), "# Desktop client\n").unwrap();
            commit(&wt, "analysis", 2);
            let asked_there = desktop.replace("[ ]", "[?]")
                + "  [2026-11-11 14:34] owner · question — Consider that a phone app would come next.\n";
            std::fs::write(
                wt.join(DOC),
                register(&format!("## Waiting on an agent\n\n{asked_there}\n")),
            )
            .unwrap();
            let answered = desktop.replace("[ ]", "[?]")
                + "  [2026-11-11 15:26] owner · question — 1. Should the badge also cover questions agents ask you on tasks?\n    Yes.\n    But considering there will be an iOS app, how would this change the decided upon solution here?\n  [2026-11-11 15:44] agent — Your five answers, each with what it changes, then the iPhone app.\n\n    So the phone strengthens A.\n  [2026-11-11 15:44] agent · question — How is the app built? A, B or C. Recommended: A.\n";
            let small = "### Reports, small choices\n\n- [ ] **reports-small-choices** — Six smaller choices for the Runs and Reports pages.\n  [2026-11-11 15:45] agent — Posted for the board task `product/reports-and-monitoring`.\n";
            let main = register(&format!("## Needs your decision\n\n{small}\n{answered}\n"));
            tree.write(DOC, &main);
            commit(&tree.root, "docs: owner verdicts", 3);

            // The branch posted no card: only the owner's line on its copy is read, for main's card.
            let cards = branches::branch_cards(&tree.root);
            let listed: Vec<_> = cards
                .iter()
                .map(|c| (c.card.slug.as_str(), c.on_main, c.path()))
                .collect();
            assert_eq!(listed, [("desktop-client", true, DOC.to_string())]);

            // The tidy puts both of main's cards under "Needs your decision" ...
            let tidied = symphony::register::sort_register(&main, &common::plain()).unwrap();
            let needs = tidied
                .split("\n## ")
                .find(|group| group.starts_with("Needs your decision"))
                .unwrap();
            let mut tidy_slugs: Vec<&str> = ["desktop-client", "reports-small-choices"]
                .into_iter()
                .filter(|slug| needs.contains(&format!("**{slug}**")))
                .collect();
            tidy_slugs.sort_unstable();
            assert_eq!(tidy_slugs, ["desktop-client", "reports-small-choices"]);
            // ... and so do the status, the board's header and progress, its queue and its pages.
            let status = common::status_ok(&tree.root, &[]);
            assert!(
                common::matches(r"(?m)^  needs your decision +2$", &status),
                "{status}"
            );
            let snapshot = symphony::dashboard::snapshot(&tree.root, &cards, "test").unwrap();
            assert_eq!(snapshot.board.counts.waiting, 2);
            assert_eq!(snapshot.board.counts.docs_waiting, 1);
            assert_eq!(snapshot.board.progress.decisions_waiting, 2);
            let mut waiting: Vec<_> = snapshot
                .queue
                .waiting
                .iter()
                .map(|c| (c.doc.as_str(), c.slug.as_str(), c.why.as_str()))
                .collect();
            waiting.sort_unstable();
            assert_eq!(
                waiting,
                [
                    (DOC, "desktop-client", "design · an agent asks you"),
                    (DOC, "reports-small-choices", "design call"),
                ]
            );
            let pending: Vec<_> = snapshot
                .pending
                .items
                .iter()
                .map(|c| c.href.as_str())
                .collect();
            assert_eq!(
                pending,
                [
                    "/docs/DECISIONS.md#reports-small-choices",
                    "/docs/DECISIONS.md#desktop-client"
                ]
            );
            // The branch's copy is no second card: the owner's line on it shows on main's, in time
            // order, and main's later answer still decides what the card waits on.
            assert!(
                snapshot.queue.agent.is_empty(),
                "{:?}",
                snapshot.queue.agent
            );
            let one = &snapshot.pages[DOC];
            assert_eq!(one.len(), 1);
            let said: Vec<&str> = one[0]
                .raw
                .iter()
                .filter_map(|line| line.trim().strip_prefix('['))
                .map(|line| &line[..16])
                .collect();
            assert_eq!(
                said,
                [
                    "2026-11-11 14:07",
                    "2026-11-11 14:33",
                    "2026-11-11 14:34",
                    "2026-11-11 15:26",
                    "2026-11-11 15:44",
                    "2026-11-11 15:44"
                ]
            );
            assert!(
                one[0]
                    .raw
                    .iter()
                    .any(|l| l.contains("a phone app would come next"))
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
                commit(&wt, "post a revision", 2);
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
                    let doc =
                        docview::document(&Render::new(&wt), rel, "design~policy", &[]).unwrap();
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
            let doc = docview::document(&render, rel, "design~policy", &[]).unwrap();
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
                docview::document(&render, "docs/UNTOUCHED.md", "design~policy", &[]).unwrap();
            assert_eq!(untouched.comparison, None);
            assert!(untouched.sections.iter().all(|s| s.comparison.is_none()));
        },
    );
}

#[test]
fn scan_225_worktrees_has_bounded_commands_and_parallel_waits() {
    // Re-enter with a process-local PATH, so interception stays at the real subprocess boundary.
    // The scan is given its finished branches, so the child never reads a board.
    if std::env::var(CHILD).as_deref() == Ok("scan") {
        let root = std::env::var("SYMPHONY_SCAN_ROOT").unwrap();
        let found = branches::scan(Path::new(&root), &Default::default());
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
    let (send, receive) = crate::test_channel::channel();
    let send_end = send.clone();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(arrivals);
        for _ in 0..8 {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            send.send(Scan::Arrived(line)).unwrap();
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
    exec /bin/cat "$SYMPHONY_SCAN_ROOT/listing"
fi
if [ "${operation%% *}" = merge-base ] && [ ! -f "$PWD/scan-started" ]; then
    : > "$PWD/scan-started"
    printf '%s\n' "$PWD" > "$SYMPHONY_SCAN_ROOT/arrived"
    read -r ready < "$SYMPHONY_SCAN_ROOT/release"
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
    let child = symphony_board::sys::spawn_output(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "scan_225_worktrees_has_bounded_commands_and_parallel_waits",
                "--nocapture",
            ])
            .env(CHILD, "scan")
            .env("SYMPHONY_SCAN_ROOT", &tree.root)
            .env("PATH", path),
    )
    .unwrap();
    let waiter = std::thread::spawn(move || {
        let _ = send_end.send(Scan::Ended(symphony_board::sys::wait_output(child)));
    });
    // No deadline: nothing is released before eight distinct worktrees arrive (`in_flight`). A
    // serial scan never brings a second, so it shows as the suite's budget timeout, its last
    // stderr line naming the held worktree ("scan: 1 of 8 worktrees in flight ...: <path>").
    let active = in_flight(&receive, 8);
    // Release the first eight and the two remaining workers, with no timing sleeps.
    release
        .write_all(b"go\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\ngo\n")
        .unwrap();
    if active.is_err() {
        // Unblock the reader, which still waits for the arrivals that never came.
        arrivals_guard
            .write_all(b"abort\nabort\nabort\nabort\nabort\nabort\nabort\nabort\n")
            .unwrap();
    }
    reader.join().unwrap();
    let active = active.unwrap_or_else(ended_early);
    let done = scan_end(&receive);
    waiter.join().unwrap();
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
            r"grep -l -E -e <!-- register:start --> -e ^-[[:space:]]+\[[ xX?r~a-]\][[:space:]]+\*\* main -- docs",
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
                // what the branch posted, then what its worktree holds unsaved
                format!("diff --name-only {base} HEAD -- docs"),
                "diff --name-only HEAD -- docs".into(),
                format!(
                    r"grep -l -E -e <!-- register:start --> -e ^-[[:space:]]+\[[ xX?r~a-]\][[:space:]]+\*\* {base} -- docs"
                ),
                "cat-file --batch".into(),
            ]
        );
        assert_eq!(
            tree.read(&format!("live{n}/scan-blobs")),
            format!("{base}:docs/DECISIONS.md\nHEAD:docs/DECISIONS.md\n")
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
    assert_eq!(calls, 54);
    assert_eq!(blobs, 21);
    // Inject a 50 ms process-start cost and 5 ms/blob read cost. Enumerating plain pages or
    // launching per-file subprocesses exceeds this same five-second budget under any host load.
    assert!(charged_ms < 5_000, "scan cost: {charged_ms}ms");
    eprintln!(
        "225 secondary trees, 300 plain pages: {calls} commands, {blobs} blobs, injected cost {charged_ms}ms; eight concurrent worktrees handshook"
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
    let none = Default::default();
    assert_eq!(
        branches::scan(&tree.root, &none)
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
        branches::scan(&tree.root, &none)
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

/// The journal records the saves left for the shared server: each one's chat text and the ask it
/// opens, as written.
fn journalled(root: &Path) -> Vec<(String, Option<serde_json::Value>)> {
    let db = symphony::asks::default_path(root, None);
    let done = symphony_board::sys::output(
        Command::new("sqlite3")
            .arg("-json")
            .arg(&db)
            .arg("SELECT text, ask FROM callbacks ORDER BY id;"),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    if done.stdout.is_empty() {
        return Vec::new();
    }
    serde_json::from_slice::<Vec<serde_json::Value>>(&done.stdout)
        .unwrap()
        .into_iter()
        .map(|row| {
            let ask = row["ask"]
                .as_str()
                .map(|ask| serde_json::from_str(ask).unwrap());
            (row["text"].as_str().unwrap().to_string(), ask)
        })
        .collect()
}

#[test]
fn branch_verdict_saves_its_file_and_asks_about_its_card_not_mains_answered_copy() {
    isolated(
        "branch_verdict_saves_its_file_and_asks_about_its_card_not_mains_answered_copy",
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
            commit(&path, "post beta", 2);
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
            let first = symphony::clock::now_stamp();
            assert_eq!(
                post(
                    &saves,
                    "",
                    "/api/decision",
                    json!({"path":"@pictures~fid/docs/DECISIONS.md", "slug":"beta", "state":"rejected", "note":"too dark"})
                ),
                json!({"ok":true})
            );
            let last = symphony::clock::now_stamp();
            saves.close();
            let written = std::fs::read_to_string(path.join(DOC)).unwrap();
            assert!(written.contains("owner · rejected — too dark"));
            assert!(written.contains("- [-] **beta**"));
            assert_eq!(tree.read(DOC), before);
            assert_eq!(command(&tree.root, &["rev-parse", "main"], 3), main_tip);
            let open = symphony::verdicts::open_cards(&tree.root).unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(
                (open[0].slug.as_str(), open[0].path.as_str()),
                ("beta", "@pictures~fid/docs/DECISIONS.md")
            );
            assert_eq!(open[0].quotes, ["too dark"]);
            // The save's journal record opens the ask about the branch's card, in main's journal.
            let journal = journalled(&tree.root);
            assert_eq!(journal.len(), 1, "{journal:?}");
            let (text, ask) = &journal[0];
            // The line says the minute the owner said it, as a card's stamp does.
            let said = text
                .strip_prefix("OWNER /api/decision at ")
                .and_then(|rest| rest.split_once(" scope="))
                .map_or("", |(stamp, _)| stamp);
            assert!(
                (first.as_str()..=last.as_str()).contains(&said),
                "{first}..{last}: {text}"
            );
            let mut ask = ask.clone().expect("the verdict waits on an agent");
            assert!(ask["at"].as_f64().is_some(), "{ask}");
            ask["at"] = serde_json::Value::Null;
            assert_eq!(
                ask,
                json!({"kind": "verdict", "path": "docs/DECISIONS.md", "card": "beta",
                       "block": null, "tree": "pictures/fid", "task": null, "thread": null,
                       "words": "too dark", "at": null})
            );
        },
    );
}

/// Each save that leaves the owner waiting on an agent journals the ask it opens, beside its chat;
/// one that waits on nobody journals no ask. A checkout with no repository commits nothing.
#[test]
fn each_save_waiting_on_an_agent_journals_its_ask_and_one_waiting_on_nobody_none() {
    isolated(
        "each_save_waiting_on_an_agent_journals_its_ask_and_one_waiting_on_nobody_none",
        |tree| {
            tree.write(
                DOC,
                &register(&(card("alpha") + &card("beta") + &card("gamma") + &card("delta"))),
            );
            let saves = symphony::save::Saves::start(&tree.root);
            for (route, payload) in [
                // a rejection with no note: nothing for the chat, the ask alone
                (
                    "/api/decision",
                    json!({"path": DOC, "slug": "alpha", "state": "rejected", "note": ""}),
                ),
                // an approval with no note waits on nobody
                (
                    "/api/decision",
                    json!({"path": DOC, "slug": "beta", "state": "approved", "note": ""}),
                ),
                (
                    "/api/decision",
                    json!({"path": DOC, "slug": "delta", "state": "rejected", "note": "Too dark."}),
                ),
                (
                    "/api/message",
                    json!({"path": DOC, "slug": "gamma", "text": "Which one?"}),
                ),
                (
                    "/api/comment",
                    json!({"path": DOC, "kind": "dislike", "title": "Too long", "body": "Cut it.", "after_line": 1}),
                ),
            ] {
                post(&saves, "", route, payload);
            }
            saves.close();
            let journal: Vec<_> = journalled(&tree.root)
                .into_iter()
                .map(|(text, ask)| {
                    let ask = ask.expect("each record here opens an ask");
                    assert!(ask["tree"].is_null(), "{ask}");
                    let about = ask["card"].as_str().or(ask["block"].as_str());
                    (
                        text.split(' ').take(2).collect::<Vec<_>>().join(" "),
                        ask["kind"].as_str().unwrap().to_string(),
                        about.unwrap().to_string(),
                        ask["words"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            let row = |text: &str,
                       kind: &str,
                       about: &str,
                       words: &str|
             -> (String, String, String, String) {
                (text.into(), kind.into(), about.into(), words.into())
            };
            assert_eq!(
                journal,
                [
                    row("", "verdict", "alpha", "rejected"),
                    row("OWNER /api/decision", "verdict", "delta", "Too dark."),
                    row("OWNER /api/message", "message", "gamma", "Which one?"),
                    row(
                        "OWNER /api/comment",
                        "comment",
                        "Too long",
                        "Too long Cut it."
                    ),
                ]
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

/// The deck document of the branch-edit case: its link into the history folder, its menu entry and
/// whether it still holds the evidence table main trims.
fn deck(history: &str, entry: &str, evidence: bool) -> String {
    let facts = (0..40)
        .map(|n| format!("- Host fact {n}: the device keeps its setting {n} between sessions.\n"))
        .collect::<String>();
    let steps = (3..9)
        .map(|n| format!("{n}. Step {n} of the procedure is read on the device.\n"))
        .collect::<String>();
    let table = if evidence {
        (0..8)
            .map(|n| format!("| run {n} | median {} fps |\n", 50 + n))
            .collect()
    } else {
        String::new()
    };
    format!(
        "# Deck\n\nThe passes behind these numbers are in [history/deck.md]({history}).\n\n\
         ## Host facts\n{facts}\n## Procedure\n\n\
         1. Deploy and play. The title screen opens with **{entry}** lit.\n\
         2. **A** ({entry}), then **A** again on a career.\n{steps}\n## Performance\n{table}"
    )
}

/// A document edited on a feature branch under its old name, while main moved every document into
/// a folder for its topic and then edited it there: the branch's edit stays in the moved document's
/// history through the branch's merge of main and through main's merge of the branch, and its patch
/// holds that document alone, not the source files the same commit changed.
#[test]
fn a_branch_edit_under_the_old_name_survives_merging_into_a_main_that_moved_the_document() {
    let tree = repository();
    let topics = ["widgets", "stations", "workflow", "economy"];
    let other = |n: usize| format!("docs/TOPIC_{n}.md");
    tree.write("docs/DECK.md", &deck("history/deck.md", "NEW VOYAGE", true));
    for n in 0..60 {
        tree.write(
            &other(n),
            &format!(
                "# Topic {n}\n{}",
                format!("The {n}th topic keeps its own procedure.\n").repeat(20)
            ),
        );
    }
    tree.write("view/ui_title.gd", "extends Control\nvar entries = []\n");
    tree.write("view/ui_menu.gd", "extends Control\n");
    let first = commit(&tree.root, "Deck procedure", 1);

    // The feature branch: a commit beside the document, then the edit of the document together
    // with the source it describes.
    command(&tree.root, &["switch", "-qc", "view/main-menu"], 2);
    tree.write("view/ui_kit.gd", "extends Node\nvar screens = {}\n");
    commit(
        &tree.root,
        "view/main-menu: a screen registry on the kit",
        2,
    );
    tree.write("docs/DECK.md", &deck("history/deck.md", "NEW GAME", true));
    tree.write(
        "view/ui_title.gd",
        "extends Control\nvar entries = [\"NEW GAME\", \"CONTINUE\"]\n",
    );
    tree.write("view/ui_new_game.gd", "extends Control\n");
    let edit = commit(
        &tree.root,
        "view/main-menu: the title is a plain list, and each entry opens its own screen",
        4,
    );

    // Main moves on beside the document; the branch takes it in, then goes on without the document.
    command(&tree.root, &["switch", "-q", "main"], 3);
    tree.write("view/ui_menu.gd", "extends Control\nvar opened = false\n");
    commit(&tree.root, "view/menu: the overlay remembers it opened", 3);
    command(&tree.root, &["switch", "-q", "view/main-menu"], 5);
    command(
        &tree.root,
        &[
            "-c",
            "commit.gpgsign=false",
            "merge",
            "-q",
            "--no-ff",
            "main",
            "-m",
            "Merge branch 'main' into view/main-menu",
        ],
        5,
    );
    tree.write(
        "view/ui_title.gd",
        "extends Control\nvar entries = [\"NEW GAME\", \"CONTINUE\", \"LOAD GAME\"]\n",
    );
    commit(
        &tree.root,
        "view/main-menu: CONTINUE and LOAD GAME read the save slots",
        6,
    );

    // Main moves every document into a folder for its topic, unchanged, then edits the deck there.
    command(&tree.root, &["switch", "-q", "main"], 6);
    for topic in topics.iter().chain(["engineering"].iter()) {
        std::fs::create_dir_all(tree.path(&format!("docs/{topic}"))).unwrap();
    }
    command(
        &tree.root,
        &["mv", "docs/DECK.md", "docs/engineering/DECK.md"],
        6,
    );
    for n in 0..60 {
        let to = format!("docs/{}/TOPIC_{n}.md", topics[n % topics.len()]);
        command(&tree.root, &["mv", &other(n), &to], 6);
    }
    let moved = commit(
        &tree.root,
        "docs/restructure: move the docs into a folder per topic",
        6,
    );
    tree.write(
        "docs/engineering/DECK.md",
        &deck("../history/deck.md", "NEW VOYAGE", false),
    );
    tree.write("docs/engineering/README.md", "# Engineering\n");
    let trimmed = commit(
        &tree.root,
        "docs/restructure: a README per folder, and the merged and trimmed docs",
        7,
    );
    command(
        &tree.root,
        &[
            "-c",
            "commit.gpgsign=false",
            "merge",
            "-q",
            "--no-ff",
            "view/main-menu",
            "-m",
            "Merge view/main-menu: the title is a list of acts",
        ],
        8,
    );
    let merge = command(&tree.root, &["rev-parse", "--short", "HEAD"], 8);
    let rel = "docs/engineering/DECK.md";
    assert_eq!(
        tree.read(rel),
        deck("../history/deck.md", "NEW GAME", false),
        "the fixture's merge carries both sides' edits"
    );

    let rows = versions::versions(&tree.root, rel, 0);
    assert_eq!(
        rows.iter()
            .map(|v| (v.sha.as_str(), v.subject.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                trimmed.as_str(),
                "docs/restructure: a README per folder, and the merged and trimmed docs"
            ),
            (
                moved.as_str(),
                "docs/restructure: move the docs into a folder per topic"
            ),
            (
                edit.as_str(),
                "view/main-menu: the title is a plain list, and each entry opens its own screen"
            ),
            (first.as_str(), "Deck procedure"),
        ]
    );
    let patch = versions::diff(&tree.root, rel, &edit).unwrap();
    assert_eq!(patch.sha, edit);
    assert!(
        patch
            .diff
            .starts_with("diff --git a/docs/DECK.md b/docs/DECK.md\n--- a/docs/DECK.md\n"),
        "{}",
        patch.diff
    );
    assert!(
        patch.diff.contains(
            "-1. Deploy and play. The title screen opens with **NEW VOYAGE** lit.\n\
             -2. **A** (NEW VOYAGE), then **A** again on a career.\n\
             +1. Deploy and play. The title screen opens with **NEW GAME** lit.\n\
             +2. **A** (NEW GAME), then **A** again on a career.\n"
        ),
        "{}",
        patch.diff
    );
    for source in ["view/ui_title.gd", "view/ui_new_game.gd", "extends Control"] {
        assert!(!patch.diff.contains(source), "{source}: {}", patch.diff);
    }
    assert_eq!(versions::diff(&tree.root, rel, &merge), None);
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
            let doc = docview::document(&render, rel, "tools~plain", &[]).unwrap();
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
            let folder = docview::folder(&render, "docs/guide", "tools~plain", &[]).unwrap();
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
                    docview::document(&render, invalid, "", &[]).is_err(),
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
            let before = docview::document(&render, rel, "", &[]).unwrap();
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
            let after = docview::document(&render, rel, "", &[]).unwrap();
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
            let decided = docview::document(&render, rel, "", &[]).unwrap();
            let delta = delta_roundtrip(&after, &decided);
            assert_eq!(delta.cards.len(), 1);
            assert_eq!(delta.cards[0].key, "proposal-5");
            assert_eq!(delta.cards[0].value.as_ref().unwrap().state, "approved");
            assert_eq!(delta.waiting.as_ref().unwrap().len(), 159);
            assert!(delta.sections.iter().all(|s| s.key == "card:proposal-5"));
            let replacement = format!("# Long\n\n{}{}", card("replacement"), card("proposal-5"));
            tree.write(rel, &replacement);
            let removed = docview::document(&render, rel, "", &[]).unwrap();
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

/// A card's thread is no part of its section's hash, so the outline names each thread by a hash of
/// its own: a reply moves its card's alone, and the section a client kept on the device with its
/// thread is known current by the two together (docs/engineering/analysis/client-cache.md).
#[test]
fn an_outline_names_each_cards_thread_by_a_hash_that_moves_with_that_thread_alone() {
    isolated(
        "an_outline_names_each_cards_thread_by_a_hash_that_moves_with_that_thread_alone",
        |tree| {
            let rel = "docs/THREADS.md";
            let text = format!("# Threads\n\n{}{}", card("first"), card("second"));
            tree.write(rel, &text);
            let render = Render::new(&tree.root);
            let before = docview::document(&render, rel, "", &[]).unwrap();
            let outline = docview::outline(before.clone());
            assert_eq!(
                outline
                    .thread_hashes
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["first", "second"]
            );
            assert!(
                outline
                    .thread_hashes
                    .values()
                    .all(|sum| sum.len() == 16 && sum.chars().all(|c| c.is_ascii_hexdigit())),
                "{:?}",
                outline.thread_hashes
            );
            // A hash is of the thread alone: two cards' like threads hash alike, unlike ones not.
            assert_eq!(
                outline.thread_hashes["first"],
                outline.thread_hashes["second"]
            );
            tree.write(
                "docs/UNLIKE.md",
                &format!("# Unlike\n\n{}{}", card("first"), card("second")).replacen(
                    "Posted.",
                    "Posted, first.",
                    1,
                ),
            );
            let unlike =
                docview::outline(docview::document(&render, "docs/UNLIKE.md", "", &[]).unwrap());
            assert_ne!(
                unlike.thread_hashes["first"],
                unlike.thread_hashes["second"]
            );
            // The hashes are the outline's alone: the threads stay out of it, as before.
            assert!(outline.document.threads.is_empty());
            assert_eq!(
                docview::outline(before.clone()).thread_hashes,
                outline.thread_hashes
            );
            let replied = symphony::edit::add_message(
                &text,
                "first",
                "Please clarify.",
                "2026-09-03 10:00",
                symphony::Who::Owner,
                None,
            )
            .unwrap();
            tree.write(rel, &replied);
            let after = docview::outline(docview::document(&render, rel, "", &[]).unwrap());
            assert_ne!(after.thread_hashes["first"], outline.thread_hashes["first"]);
            assert_eq!(
                after.thread_hashes["second"],
                outline.thread_hashes["second"]
            );
            let hash = |outline: &symphony_api::DocumentOutline, key: &str| {
                outline
                    .document
                    .sections
                    .iter()
                    .find(|s| s.key == key)
                    .map(|s| s.hash.clone())
                    .unwrap()
            };
            assert_eq!(
                hash(&after, "card:second"),
                hash(&outline, "card:second"),
                "the other card is unchanged"
            );
            // The outline's text, its threads' hashes among it, round-trips; one without them
            // (a server before them) reads as none.
            let wire: symphony_api::DocumentOutline =
                serde_json::from_slice(&serde_json::to_vec(&after).unwrap()).unwrap();
            assert_eq!(wire, after);
            let mut older = serde_json::to_value(&after).unwrap();
            older.as_object_mut().unwrap().remove("thread_hashes");
            let older: symphony_api::DocumentOutline = serde_json::from_value(older).unwrap();
            assert!(older.thread_hashes.is_empty());
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
            let before = docview::document(&render, rel, "branch", &[]).unwrap();
            tree.write(rel, "# Renamed\n\nIntro.\n\n## Second\n\nTwo.\n\n## First\n\nOne changed.\nExtra line.\n");
            let after = docview::document(&render, rel, "branch", &[]).unwrap();
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
