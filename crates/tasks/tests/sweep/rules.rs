//! The sweep's rules: each keep rule on its own (the worktree would go but for that one thing), what
//! goes, the agent branches, the window of recent activity, the races the re-checks guard against,
//! and the processes behind a lock.

use crate::children::Child as _;
use crate::common::{GONE, Seed, session};
use crate::{HeldCwd, IDLE_HOURS, NOT_UTC, Repo, dead_pid, git, set_time, started, text, write};
use std::cell::Cell;
use std::cell::RefCell;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;
use symphony_board::health::LIVE_SECONDS;
use symphony_board::held::{MESSAGE, Place};
use symphony_board::people::Power;
use symphony_board::rows::SessionRow;
use symphony_board::schema::{ADDED, HELD};
use symphony_board::sweep::process;
use symphony_board::{NewLaunch, NewSession, sweep, sys};

const LIVE: [&str; 6] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "blocked",
];

fn me() -> i64 {
    i64::from(std::process::id())
}

// -- each keep rule on its own -----------------------------------------------------------

#[test]
fn tracked_changes_keep_it() {
    let r = Repo::new();
    let wt = r.detached("dirty");
    write(&wt, "sim/world.gd", "extends RefCounted\n# unsaved\n");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_staged_change_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("staged");
    write(&wt, "sim/new.gd", "extends Node\n");
    git(&wt, &["add", "sim/new.gd"]);
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn an_untracked_file_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("untracked");
    write(&wt, "notes.txt", "a scratch note\n");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn an_ignored_file_that_is_not_output_keeps_it_and_is_named() {
    // Local settings survive, even when no task is bound here. The probes do not keep it: with no
    // task, its work is finished, and they would be set aside in the main checkout were it to go.
    let r = Repo::new();
    let wt = r.detached("probes");
    write(&wt, "artifacts/solar-swift/probe.json", "x\n");
    write(&wt, ".claude/settings.local.json", "x\n");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    let kept: Vec<&str> = said
        .lines()
        .filter(|l| l.starts_with("kept ") && l.contains(&*wt.to_string_lossy()))
        .collect();
    assert_eq!(kept.len(), 1, "{said}");
    assert!(kept[0].contains(".claude/"), "{said}");
    assert!(!kept[0].contains("artifacts/"), "{said}");
    assert!(wt.join("artifacts/solar-swift/probe.json").is_file());
}

#[test]
fn a_head_not_merged_into_main_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("unmerged");
    r.commit(&wt, "extends RefCounted\n# more\n");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_live_claude_lock_keeps_it() {
    // Written in UTC, as Claude Code writes it, while the sweep runs in a zone three hours off.
    let r = Repo::new();
    let wt = r.detached("live-lock");
    r.claude_lock(&wt, &me().to_string(), &started(me()));
    let mut vars = r.vars();
    vars.insert("TZ".into(), NOT_UTC.into());
    let result = r.sweep_with(|o| {
        o.started = Box::new(move |pid| process::process_start(pid, &vars));
    });
    r.assert_kept(&wt, &text(&result));
}

#[test]
fn a_live_claude_lock_with_a_padded_day_keeps_it() {
    // `ps` pads a one-digit day with a space ("Sep  3"), strftime with a zero ("Sep 03"): one start.
    let r = Repo::new();
    let wt = r.detached("live-lock-spaced");
    let start = started(me());
    let words: Vec<String> = start
        .split(' ')
        .map(|w| match w.strip_prefix('0') {
            Some(digit) if w.len() == 2 => format!(" {digit}"),
            _ => w.to_string(),
        })
        .collect();
    r.claude_lock(&wt, &me().to_string(), &words.join(" "));
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn ps_is_read_in_utc_whatever_zone_the_caller_is_in() {
    // On the owner's machine (-03) a lock said 13:10:22 and a local `ps` said 10:10:22 for one process.
    let mut vars: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    vars.insert("TZ".into(), NOT_UTC.into());
    let start = process::process_start(me(), &vars).unwrap();
    assert_eq!(
        process::parse_start(&start),
        process::parse_start(&started(me()))
    );
}

#[test]
fn a_foreign_lock_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("foreign-lock");
    r.lock(&wt, "on a USB disk that is not mounted");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_lock_with_no_reason_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("bare-lock");
    git(&r.root, &["worktree", "lock", wt.to_str().unwrap()]);
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_claude_lock_with_an_unreadable_start_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("garbled-lock");
    r.claude_lock(&wt, &dead_pid().to_string(), "sometime last week");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_fresh_branch_with_no_commits_of_its_own_goes_once_idle() {
    // Recent activity, live sessions and working processes protect this checkout. Its branch, at a
    // commit of main's, goes with it: it holds nothing main has not.
    let r = Repo::new();
    let wt = r.add("fresh", Some("sim/fresh"), "main");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(!said.contains("no commits of its own"), "{said}");
    assert_eq!(r.branch("sim/fresh"), "", "{said}");
}

#[test]
fn a_fresh_branch_behind_main_goes_once_idle() {
    let r = Repo::new();
    let old = git(&r.root, &["rev-parse", "main"]);
    r.commit(&r.root, "extends RefCounted\n# more\n");
    let wt = r.add("fresh-behind", Some("sim/fresh-behind"), &old);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_fresh_branch_that_fast_forwarded_main_before_any_commit_goes_once_idle() {
    let r = Repo::new();
    let wt = r.add("synced", Some("sim/synced"), "main");
    r.commit(&r.root, "extends RefCounted\n# more\n");
    git(&wt, &["merge", "-q", "--ff-only", "main"]);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_branch_a_live_task_holds_keeps_it() {
    let mut seed = Seed::default();
    for status in LIVE {
        seed = seed.task(&format!("sim/held-{}", status.replace('_', "-")), status);
    }
    let r = Repo::with(seed);
    let trees = LIVE.map(|status| {
        let slug = format!("sim/held-{}", status.replace('_', "-"));
        (r.merged(&format!("held-{status}"), &slug), slug, status)
    });
    let said = r.sweep();
    for (wt, slug, status) in &trees {
        r.assert_kept(wt, &said);
        assert!(
            said.contains(&format!("kept {}: task {slug} is {status}", wt.display())),
            "{said}"
        );
    }
}

#[test]
fn a_cherry_pick_only_branch_goes_once_idle() {
    // A branch with committed work is eligible once the other protections expire.
    let r = Repo::new();
    let side = r.add("side", Some("sim/side"), "main");
    r.commit(&side, "extends RefCounted\n# more\n");
    let sha = git(&side, &["rev-parse", "HEAD"]);
    let wt = r.add("picked", Some("sim/picked"), "main");
    git(&wt, &["cherry-pick", &sha]);
    git(&r.root, &["merge", "-q", "--ff-only", "sim/picked"]);
    r.assert_removed(&wt, &r.sweep());
}

/// A branch that once had commits (merged), then `worktree add -B` resets it to main for a new
/// executor.
fn reused_with_dash_b(r: &Repo, slug: &str) -> PathBuf {
    let old = r.merged("old", slug);
    git(&r.root, &["worktree", "remove", old.to_str().unwrap()]);
    r.commit(&r.root, "extends RefCounted\n# main moved on\n"); // so -B really moves it
    let wt = r.dir.join("reused");
    git(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "-B",
            slug,
            wt.to_str().unwrap(),
            "main",
        ],
    );
    wt
}

#[test]
fn a_branch_reset_with_dash_b_is_swept_when_idle_with_no_task() {
    let r = Repo::new();
    let wt = reused_with_dash_b(&r, "sim/reused");
    let reflog = git(
        &r.root,
        &["reflog", "show", "--format=%gs", "refs/heads/sim/reused"],
    );
    assert!(reflog.contains("branch: Reset"), "{reflog}");
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_branch_reset_with_dash_b_is_kept_by_its_task_or_its_process() {
    let r = Repo::with(Seed::default().task("sim/reused", "ready"));
    let wt = reused_with_dash_b(&r, "sim/reused");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(said.contains("task sim/reused is ready"), "{said}");
    let r = Repo::with(Seed::default().task("sim/reused", "done"));
    let wt = reused_with_dash_b(&r, "sim/reused");
    let inner = wt.join("sim");
    let result = r.sweep_with(|o| o.cwds = Box::new(move || Some(vec![inner.clone()])));
    r.assert_kept(&wt, &text(&result));
    assert!(text(&result).contains("a process is working inside it"));
}

#[test]
fn the_summary_counts_what_was_kept_in_use() {
    let r = Repo::new();
    let wt = r.add("fresh-count", Some("sim/fresh-count"), "main");
    let result = r.sweep_with(|o| o.cwds = Box::new(|| Some(Vec::new())));
    assert!(
        result.summary().contains("kept 1 in use"),
        "{}",
        text(&result)
    );
    assert!(wt.is_dir());
}

#[test]
fn a_process_working_inside_it_keeps_it() {
    let r = Repo::new();
    let wt = r.merged("occupied", "sim/occupied");
    let inner = wt.join("sim");
    let result = r.sweep_with(|o| o.cwds = Box::new(move || Some(vec![inner.clone()])));
    r.assert_kept(&wt, &text(&result));
    assert!(text(&result).contains("a process is working inside it"));
}

#[test]
fn a_process_working_elsewhere_does_not_keep_it() {
    let r = Repo::new();
    let wt = r.merged("elsewhere", "sim/elsewhere");
    let root = r.root.clone();
    let result = r.sweep_with(|o| o.cwds = Box::new(move || Some(vec![root.clone()])));
    r.assert_removed(&wt, &text(&result));
}

#[test]
fn a_cwd_scan_that_cannot_be_read_keeps_it() {
    for (name, unread) in [("none", None), ("empty", Some(Vec::new()))] {
        let r = Repo::new();
        let wt = r.merged(&format!("unread-{name}"), &format!("sim/unread-{name}"));
        let result = r.sweep_with(|o| o.cwds = Box::new(move || unread.clone()));
        r.assert_kept(&wt, &text(&result));
        assert!(
            text(&result).contains("cannot tell whether a process is working inside it"),
            "{name}"
        );
    }
}

#[test]
fn a_live_session_bound_to_it_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("bound");
    r.bind(&wt);
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_stale_session_bound_to_it_keeps_it() {
    // Missed beats for a few minutes is not gone: its agent may still be at work there.
    let r = Repo::new();
    let wt = r.detached("stale");
    let handle = r.bind(&wt);
    r.board.advance(LIVE_SECONDS + 60.0);
    assert_eq!(r.board.health(&handle).as_str(), "stale");
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_live_session_bound_through_a_symlinked_path_keeps_it() {
    let r = Repo::new();
    let wt = r.detached("bound-link");
    let link = r.dir.join("link-to-bound");
    std::os::unix::fs::symlink(&wt, &link).unwrap();
    r.bind(&link);
    r.assert_kept(&wt, &r.sweep());
}

// -- a subagent's own worktree, which Claude Code names agent-ID: its session is registered at the
// main checkout, and a resumed agent's worktree has no lock (swift-kestrel-809's probe) ----------

#[test]
fn a_live_subagents_own_worktree_is_kept_by_its_agent_id_with_no_lock() {
    let r = Repo::new();
    let mine = r.detached("agent-a5a6262f496f70d23");
    let other = r.detached("agent-ac953cb051c3dd046"); // no session is bound to that agent
    let handle = r.bind(&r.root); // as an orchestrator registers its subagent
    assert!(
        r.board
            .store
            .bind_agent("a5a6262f496f70d23", &handle, false)
            .unwrap()
    );
    let said = r.sweep();
    r.assert_kept(&mine, &said);
    r.assert_removed(&other, &said);
}

#[test]
fn a_lost_subagents_live_task_keeps_its_own_worktree_and_a_finished_one_lets_it_go() {
    for status in ["in_progress", "done"] {
        let slug = "tools/python-zero/merge-risk";
        let r = Repo::with(
            Seed::default()
                .session_row(SessionRow {
                    agent: Some("a5a6262f496f70d23".into()),
                    ..session("amber-heron")
                })
                .task(slug, status)
                .role(slug, "amber-heron", "executor"),
        );
        r.board.advance(GONE);
        let gone = if status == "done" { "left" } else { "lost" }; // nothing left to claim: left
        assert_eq!(r.board.health("amber-heron").as_str(), gone);
        let wt = r.detached("agent-a5a6262f496f70d23");
        let said = r.sweep();
        if status == "done" {
            r.assert_removed(&wt, &said);
        } else {
            r.assert_kept(&wt, &said);
            let why = format!(
                "kept {}: task {slug} is {status}, held by amber-heron, whose session is bound here",
                wt.display()
            );
            assert!(said.contains(&why), "{said}");
        }
    }
}

#[test]
fn a_branch_spelling_a_live_tasks_slug_with_its_last_slash_as_a_dash_keeps_it() {
    let r = Repo::with(
        Seed::default()
            .task("tools/python-zero/merge-risk", "in_progress")
            .task("sim/jump", "review")
            .task("sim/landed", "done"),
    );
    let deep = r.merged("merge-risk", "tools/python-zero-merge-risk");
    let shallow = r.merged("jump", "sim-jump");
    let finished = r.merged("landed", "sim-landed");
    let first = r.merged("first", "tools-python-zero/merge-risk"); // its first / as -: no task's
    let said = r.sweep();
    r.assert_kept(&deep, &said);
    let why = format!(
        "kept {}: task tools/python-zero/merge-risk is in_progress, whose slug branch tools/python-zero-merge-risk spells with its last / as -",
        deep.display()
    );
    assert!(said.contains(&why), "{said}");
    r.assert_kept(&shallow, &said);
    assert!(
        said.contains("task sim/jump is review, whose slug branch sim-jump spells"),
        "{said}"
    );
    r.assert_removed(&finished, &said);
    r.assert_removed(&first, &said);
}

#[test]
fn the_main_checkout_is_never_removed() {
    // git itself refuses to remove it; the dry run shows the sweep never even picks it. Detached, as
    // the real root often is while main lives elsewhere, so the rule for main's own worktree does not
    // cover it; idle too, so the window does not either; swept from another worktree.
    let r = Repo::new();
    git(&r.root, &["checkout", "-q", "--detach"]);
    let wt = r.detached("elsewhere");
    r.idle(&[]);
    Repo::age(&r.root, IDLE_HOURS);
    let dry = text(&r.sweep_from(&wt, |o| o.dry_run = true));
    assert!(
        !dry.contains(&format!("would remove {} ", r.root.display())),
        "{dry}"
    );
    r.sweep_from(&wt, |_| {});
    assert!(r.worktrees().contains(&r.root));
    assert!(r.root.join(".git").is_dir());
}

#[test]
fn the_worktree_main_is_checked_out_in_is_never_removed() {
    // `land` moves main through that worktree's files; someone works on main there.
    let r = Repo::new();
    git(&r.root, &["checkout", "-q", "--detach"]);
    let wt = r.detached("on-main");
    git(&wt, &["checkout", "-q", "main"]);
    r.assert_kept(&wt, &r.sweep());
}

#[test]
fn a_worktree_holding_another_worktree_keeps_it() {
    // Removing it would take the inner one's files with it, whatever state they are in. Under
    // build/, which land lets go with a worktree, so nothing but this rule stands in the way.
    let r = Repo::new();
    let outer = r.detached("outer");
    let inner = outer.join("build/inner");
    git(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            inner.to_str().unwrap(),
            "main",
        ],
    );
    write(&inner, "sim/world.gd", "extends RefCounted\n# unsaved\n");
    r.assert_kept(&outer, &r.sweep());
    assert!(inner.join("sim/world.gd").is_file());
}

#[test]
fn an_owner_verdict_no_commit_has_keeps_a_merged_worktree_and_says_why() {
    // The owner's answer to a card lives in the worktree's file until the agent commits it.
    let r = Repo::new();
    let wt = r.add("tools-o", Some("tools/o"), "main");
    let register = "# Decisions\n\n<!-- register:start -->\n### Topic\n\n- [ ] **beta** — b.\n  [2026-10-06 09:00] agent — Posted.\n<!-- register:end -->\n";
    write(&wt, "docs/DECISIONS.md", register);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "card"]);
    git(&r.root, &["merge", "-q", "--no-edit", "tools/o"]);
    let answered = register.replace(
        "<!-- register:end",
        "  [2026-10-06 10:00] owner · approved — yes\n<!-- register:end",
    );
    write(&wt, "docs/DECISIONS.md", &answered);
    let result = r.sweep_with(|_| {});
    assert!(wt.exists());
    assert!(result.removed.is_empty());
    assert!(
        result
            .spared
            .iter()
            .any(|(_, why)| why.contains("owner") && why.contains("docs/DECISIONS.md")),
        "{:?}",
        result.spared
    );
    git(&wt, &["commit", "-qam", "verdict"]); // committed, the line no longer keeps it here
    assert!(sweep::git::answers(&wt, &docs(), None).unwrap().is_empty());
}

/// The convention's document folder, where the Symphony writes the owner's answers.
fn docs() -> Vec<String> {
    vec!["docs".into()]
}

/// A merged worktree whose documents git cannot read for the owner's answers is kept, saying so,
/// and read again on the next sweep, never remembered as holding none.
#[test]
fn a_worktree_whose_owner_answers_cannot_be_read_is_kept_and_read_again() {
    let r = Repo::new();
    let wt = r.add("tools-u", Some("tools/u"), "main");
    let register = "# Decisions\n\n- [ ] **beta** — b.\n  [2026-10-06 09:00] agent — Posted.\n";
    write(&wt, "docs/DECISIONS.md", register);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "card"]);
    git(&r.root, &["merge", "-q", "--no-edit", "tools/u"]);
    write(
        &wt,
        "docs/DECISIONS.md",
        &format!("{register}  [2026-10-06 10:00] owner · approved — yes\n"),
    );
    // A file the read cannot open where the document's folder holds an untracked one.
    let blocked = wt.join("docs/NEW.md");
    write(&wt, "docs/NEW.md", "# New\n");
    std::fs::set_permissions(
        &blocked,
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let unread = sweep::git::answers(&wt, &docs(), None).unwrap_err();
    assert!(
        unread.starts_with("git grep could not read in ") && unread.contains("docs/NEW.md"),
        "{unread}"
    );
    let result = r.sweep_with(|_| {});
    assert!(wt.exists());
    assert!(
        result.kept.iter().any(|(_, left)| {
            left.iter().any(|why| {
                why.starts_with("(its documents could not be read for the owner's answers")
            })
        }),
        "{:?}",
        result.kept
    );
    // Readable again, the same tree is read anew: the owner's answer keeps it, said as such.
    std::fs::set_permissions(
        &blocked,
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    let result = r.sweep_with(|_| {});
    assert!(wt.exists());
    assert!(
        result
            .spared
            .iter()
            .any(|(_, why)| why == "holds an owner verdict no commit has: docs/DECISIONS.md"),
        "{:?}",
        result.spared
    );
}

#[test]
fn foreign_worktree_reads_preserve_stale_merged_and_unmerged_indexes() {
    use std::fs;
    use std::io::Write;
    use std::time::Duration;
    use symphony_board::sys::Command;

    let r = Repo::new();
    write(&r.root, "docs/OWNER.md", "# Owner\n");
    write(&r.root, "docs/OTHER.md", "# Other\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "document"]);
    let wt = r.add("writer", Some("tools/writer"), "main");
    git(&wt, &["config", "diff.autoRefreshIndex", "true"]);
    let index = PathBuf::from(git(
        &wt,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    ));
    for unmerged in [false, true] {
        if unmerged {
            // A real stage-2 entry also checks the read with a conflicted index.
            let blob = git(&wt, &["rev-parse", "HEAD:docs/OWNER.md"]);
            let mut child = crate::common::spawn_with_input(
                Command::of("git")
                    .args(["update-index", "--index-info"])
                    .current_dir(&wt),
            )
            .unwrap();
            writeln!(
                child.stdin.take().unwrap(),
                "100644 {blob} 2\tdocs/CONFLICT.md"
            )
            .unwrap();
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(git(&wt, &["ls-files", "-u"]).is_empty(), !unmerged);
        write(
            &wt,
            "docs/OWNER.md",
            "# Owner\n\n- [x] **keep** — K.\n  [2026-10-27 10:00] owner · approved — keep this \
             answer\n",
        );
        // Touch without changing content, and age the index so even a same-tick rewrite is visible.
        let tracked = wt.join("docs/OTHER.md");
        let touched = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + u64::from(unmerged));
        assert_ne!(fs::metadata(&tracked).unwrap().modified().unwrap(), touched);
        set_time(&tracked, touched);
        assert_eq!(fs::metadata(&tracked).unwrap().modified().unwrap(), touched);
        set_time(
            &index,
            SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000),
        );
        let before = fs::read(&index).unwrap();
        let modified = fs::metadata(&index).unwrap().modified().unwrap();

        assert_eq!(
            sweep::git::answers(&wt, &docs(), None).unwrap(),
            [sweep::git::Answer {
                file: "docs/OWNER.md".into(),
                card: "keep".into()
            }]
        );
        assert_eq!(
            fs::read(&index).unwrap(),
            before,
            "diff: unmerged={unmerged}"
        );
        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), modified);
        let mut expected = vec!["docs/OWNER.md"];
        if unmerged {
            expected.insert(0, "docs/CONFLICT.md");
        }
        assert_eq!(
            sweep::git::left_behind(&wt, false, Some("artifacts")),
            expected
        );
        assert_eq!(
            fs::read(&index).unwrap(),
            before,
            "status: unmerged={unmerged}"
        );
        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), modified);
    }
}

// -- what goes ---------------------------------------------------------------------------

#[test]
fn a_merged_clean_detached_worktree_with_no_lock_goes() {
    let r = Repo::new();
    let wt = r.detached("reviewer");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(said.contains("no lock"), "{said}");
}

#[test]
fn a_head_behind_main_is_merged_and_goes() {
    let r = Repo::new();
    let old = git(&r.root, &["rev-parse", "main"]);
    r.commit(&r.root, "extends RefCounted\n# more\n");
    let wt = r.add("behind", None, &old);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn caches_and_verify_output_do_not_protect_it() {
    // What land lets go with a landed worktree goes with this one: its DISPOSABLE files.
    let r = Repo::new();
    let wt = r.detached("caches");
    for path in [
        ".venv/lib/x.py",
        "artifacts/slynx-verify.log",
        ".godot/cache",
        "sim/__pycache__/x.pyc",
    ] {
        write(&wt, path, "x\n");
    }
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn what_the_project_names_as_disposable_goes_and_is_kept_where_it_names_none() {
    // Another stack's dependency and build folders: git ignores them and the board's own list
    // does not know them, so only the project can say a checkout makes them again.
    let r = Repo::new();
    let ignore = std::fs::read_to_string(r.root.join(".gitignore")).unwrap();
    write(
        &r.root,
        ".gitignore",
        &format!("{ignore}node_modules/\ndist/\n*.tsbuildinfo\n"),
    );
    git(&r.root, &["commit", "-qam", "another stack's ignore rules"]);
    let wt = r.detached("node");
    for path in [
        "node_modules/left-pad/index.js",
        "web/node_modules/left-pad/index.js",
        "web/dist/app.js",
        "tsconfig.tsbuildinfo",
    ] {
        write(&wt, path, "x\n");
    }
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(said.contains("node_modules/"), "{said}");
    // Named in the main checkout's project file, they go, though the verdict that kept the
    // worktree was saved: it was read with other patterns.
    write(
        &r.root,
        "symphony.toml",
        "[board]\nsweep_disposable = [\"node_modules/\", \"*/node_modules/\", \"*/dist/\", \"*.tsbuildinfo\"]\n",
    );
    git(&r.root, &["add", "symphony.toml"]);
    git(
        &r.root,
        &["commit", "-qm", "the project's disposable paths"],
    );
    // What it does not name still keeps a worktree: a local setting beside them.
    let settings = r.detached("node-settings");
    write(&settings, "node_modules/left-pad/index.js", "x\n");
    write(&settings, ".claude/settings.local.json", "{}\n");
    // Not aged again, so the worktree's stamps are the ones its saved verdict was read with: it is
    // read again only because the patterns differ.
    let said = text(&r.sweep_now(|o| o.recent_hours = 0.0));
    r.assert_removed(&wt, &said);
    r.assert_kept(&settings, &said);
    assert!(!said.contains("node_modules/"), "{said}");
}

#[test]
fn a_dead_claude_lock_goes() {
    let r = Repo::new();
    let wt = r.detached("dead-lock");
    let pid = dead_pid();
    r.claude_lock(&wt, &pid.to_string(), &started(me()));
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(said.contains(&format!("pid {pid}")), "{said}");
}

#[test]
fn a_claude_lock_on_a_reused_pid_goes() {
    let r = Repo::new();
    let wt = r.detached("reused-pid");
    r.claude_lock(&wt, &me().to_string(), "Thu Jan  1 00:00:00 1970");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(said.contains(&format!("pid {}", me())), "{said}");
}

#[test]
fn a_session_that_left_does_not_protect_it() {
    let r = Repo::new();
    let wt = r.detached("left");
    let handle = r.bind(&wt);
    r.board.store.leave(&handle).unwrap();
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_merged_branch_worktree_goes_and_its_branch_with_it() {
    let r = Repo::new();
    let wt = r.merged("named", "sim/done");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert_eq!(r.branch("sim/done"), "", "{said}");
}

/// Ana's comment on a card of docs/a.md held and passed on into each checkout of `scopes`: their
/// ids, in order.
fn passed_into<const N: usize>(r: &Repo, scopes: [&str; N]) -> [i64; N] {
    let store = &r.board.store;
    (store.add_person("ana", &[], &[Power::Speak], "owner")).unwrap();
    let ana = r.board.other().for_person("ana");
    scopes.map(|scope| {
        let place = Place {
            path: "docs/a.md".into(),
            scope: Some(scope.into()),
            kind: MESSAGE.into(),
            card: Some("one".into()),
            block: None,
        };
        let held = ana.hold_on_document("ana", "Look again", &place).unwrap();
        let passed = store.pass_on_document_into(held.id, "owner", Some(scope));
        passed.unwrap().id
    })
}

/// The checkout the comment `id` is kept under: none for main's.
fn kept_under(r: &Repo, id: i64) -> Option<String> {
    r.board.store.held_comment(id).unwrap().unwrap().document.scope
}

/// The comments a pass wrote into a removed worktree's documents, all in main now, are kept under
/// main's from then on (`Store::held_landed`); a dry run moves none, and a worktree that stays
/// keeps its own.
#[test]
fn a_removed_worktrees_passed_comments_are_kept_under_mains() {
    let r = Repo::new();
    let [landed, open] = passed_into(&r, ["sim~done", "sim~open"]);
    let wt = r.merged("named", "sim/done");
    let stays = r.add("open", Some("sim/open"), "main");
    r.commit(&stays, "extends RefCounted\n# open\n");
    let scope = |id: i64| kept_under(&r, id);
    let dry = text(&r.sweep_with(|o| o.dry_run = true));
    assert_eq!(scope(landed).as_deref(), Some("sim~done"), "{dry}");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    r.assert_kept(&stays, &said);
    assert_eq!(scope(landed), None, "{said}");
    assert_eq!(scope(open).as_deref(), Some("sim~open"), "{said}");
}

/// Two worktrees on one branch (`git worktree add --force`) share its id, so a pass may have
/// written into the one that stays: removing the other moves none of the branch's passed comments,
/// whose line main would then lack. The last of them to go moves them, in a later sweep, or in the
/// same one where both go.
#[test]
fn a_removed_worktrees_passed_comments_stay_while_another_worktree_holds_its_branch() {
    for later in [true, false] {
        let r = Repo::new();
        let [passed] = passed_into(&r, ["sim~done"]);
        let wt = r.merged("named", "sim/done");
        let other = r.dir.join("again");
        let at = other.to_str().unwrap();
        git(&r.root, &["worktree", "add", "-q", "--force", at, "sim/done"]);
        if later {
            // The pass's line, not landed, in the one that stays.
            write(&other, "docs/a.md", "Look again\n");
            let said = r.sweep();
            r.assert_removed(&wt, &said);
            r.assert_kept(&other, &said);
            assert_eq!(kept_under(&r, passed).as_deref(), Some("sim~done"), "{said}");
            std::fs::remove_file(other.join("docs/a.md")).unwrap();
        }
        let said = r.sweep();
        r.assert_removed(&other, &said);
        if !later {
            r.assert_removed(&wt, &said);
        }
        assert_eq!(kept_under(&r, passed), None, "later={later}: {said}");
    }
}

/// A move the board refuses (one whose record lacks the held step) is said, its refusal and not
/// the comment's words, and the sweep goes on: the worktree is removed, the row where it was.
#[test]
fn a_move_the_board_refuses_is_said_and_the_sweep_goes_on() {
    let r = Repo::new();
    let store = &r.board.store;
    (store.add_person("ana", &[], &[Power::Speak], "owner")).unwrap();
    let place = Place {
        path: "docs/a.md".into(),
        scope: Some("sim~done".into()),
        kind: MESSAGE.into(),
        card: Some("one".into()),
        block: None,
    };
    let ana = r.board.other().for_person("ana");
    let held = ana.hold_on_document("ana", "Quartz lamps", &place).unwrap();
    (store.pass_on_document_into(held.id, "owner", Some("sim~done"))).unwrap();
    let step = ADDED.iter().position(|step| *step == HELD).unwrap();
    (store.raw().row("meta", "additions"))
        .set(serde_json::json!({"version": step}))
        .unwrap();
    let wt = r.merged("named", "sim/done");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    let line = said
        .lines()
        .find(|line| line.starts_with("the comments passed on into sim/done stay"))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(line.contains("this write names `held_message`"), "{line}");
    assert!(!said.contains("Quartz"), "{said}");
    let kept = store.held_comment(held.id).unwrap().unwrap();
    assert_eq!(kept.document.scope.as_deref(), Some("sim~done"));
}

#[test]
fn a_merged_branch_another_worktree_holds_stays() {
    // Two worktrees on one branch (git allows it with --force): the one removed does not take the
    // branch the other is on.
    let r = Repo::new();
    let wt = r.merged("named", "sim/done");
    let other = r.dir.join("again");
    let at = other.to_str().unwrap();
    git(
        &r.root,
        &["worktree", "add", "-q", "--force", at, "sim/done"],
    );
    write(&other, "notes.txt", "keeps this one\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    r.assert_kept(&other, &said);
    assert_eq!(r.branch("sim/done"), "+ sim/done", "{said}");
}

#[test]
fn prune_forgets_a_worktree_whose_folder_is_gone() {
    let r = Repo::new();
    let wt = r.detached("vanished");
    std::fs::remove_dir_all(&wt).unwrap();
    r.sweep();
    assert!(!r.worktrees().contains(&wt));
}

#[test]
fn one_line_per_removal_and_a_summary() {
    let r = Repo::new();
    let (first, second) = (r.detached("one"), r.detached("two"));
    let kept = r.detached("three");
    r.commit(&kept, "extends RefCounted\n# more\n");
    let said = r.sweep();
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.len(), 3, "{said}");
    let both = format!("{}{}", lines[0], lines[1]);
    assert!(both.contains(&*first.to_string_lossy()), "{said}");
    assert!(both.contains(&*second.to_string_lossy()), "{said}");
    assert!(lines[2].contains("removed 2 of 4 worktrees"), "{said}");
}

#[test]
fn a_dry_run_removes_nothing_and_says_what_it_would() {
    let r = Repo::new();
    let wt = r.merged("candidate", "worktree-agent-dry");
    let result = r.sweep_with(|o| o.dry_run = true);
    let said = text(&result);
    assert!(
        said.contains(&format!("would remove {}", wt.display())),
        "{said}"
    );
    assert!(
        said.contains("would delete branch worktree-agent-dry"),
        "{said}"
    );
    assert!(
        said.contains("would delete 1 merged worktree-agent-* branch"),
        "{said}"
    );
    assert!(wt.is_dir());
    assert_eq!(r.branch("worktree-agent-dry"), "+ worktree-agent-dry"); // still checked out there
}

#[test]
fn a_sweep_with_nothing_to_do_says_so() {
    let r = Repo::new();
    assert_eq!(
        r.sweep(),
        "worktree sweep: removed 0 of 1 worktrees, deleted 0 merged worktree-agent-* branches"
    );
}

// -- the agent branches ------------------------------------------------------------------

#[test]
fn a_merged_agent_branch_no_worktree_holds_is_deleted() {
    let r = Repo::new();
    git(&r.root, &["branch", "worktree-agent-old", "main"]);
    let said = r.sweep();
    assert_eq!(r.branch("worktree-agent-old"), "");
    assert!(said.contains("deleted branch worktree-agent-old"), "{said}");
}

#[test]
fn an_agent_branch_freed_by_the_sweep_is_deleted() {
    let r = Repo::new();
    let wt = r.merged("agent", "worktree-agent-freed");
    r.sweep();
    assert!(!wt.exists());
    assert_eq!(r.branch("worktree-agent-freed"), "");
}

#[test]
fn a_fresh_agent_branch_and_its_worktree_go_once_idle() {
    // A review checkout with no author commits may still be eligible for cleanup.
    let r = Repo::new();
    let wt = r.add("agent", Some("worktree-agent-fresh"), "main");
    r.assert_removed(&wt, &r.sweep());
    assert_eq!(r.branch("worktree-agent-fresh"), "");
}

#[test]
fn an_unmerged_agent_branch_is_kept() {
    let r = Repo::new();
    let wt = r.add("agent", Some("worktree-agent-work"), "main");
    r.commit(&wt, "extends RefCounted\n# more\n");
    git(&r.root, &["worktree", "remove", wt.to_str().unwrap()]);
    r.sweep();
    assert_eq!(r.branch("worktree-agent-work"), "worktree-agent-work");
}

#[test]
fn an_agent_branch_a_kept_worktree_holds_is_kept() {
    let r = Repo::new();
    let wt = r.add("agent", Some("worktree-agent-held"), "main");
    write(&wt, "notes.txt", "x\n");
    r.sweep();
    assert_eq!(r.branch("worktree-agent-held"), "+ worktree-agent-held");
}

#[test]
fn a_detached_agents_ref_stays_while_its_session_holds_the_worktree() {
    let r = Repo::new();
    let mut cases = Vec::new();
    // The stale sessions first, registered before the board's clock moves on: every session after
    // it is live, and one that leaves has left whenever it registered.
    for health in ["stale", "live", "left"] {
        for binding in ["agent-id", "direct", "direct-symlink"] {
            let agent = format!("{binding}-{health}");
            let branch = format!("worktree-agent-{agent}");
            let wt = r.add(&format!("agent-{agent}"), Some(&branch), "main");
            git(&wt, &["checkout", "-q", "--detach"]);
            let handle = match binding {
                "agent-id" => {
                    let handle = r.bind(&r.root);
                    assert!(r.board.store.bind_agent(&agent, &handle, false).unwrap());
                    handle
                }
                "direct" => r.bind(&wt),
                _ => {
                    let link = r.dir.join(format!("bound-link-{health}"));
                    std::os::unix::fs::symlink(&wt, &link).unwrap();
                    r.bind(&link)
                }
            };
            if health == "left" {
                r.board.store.leave(&handle).unwrap();
            }
            cases.push((wt, branch, handle, health));
        }
        if health == "stale" {
            r.board.advance(LIVE_SECONDS + 60.0);
        }
    }
    for (_, _, handle, health) in &cases {
        assert_eq!(r.board.health(handle).as_str(), *health, "{handle}");
    }
    let said = r.sweep();
    for (wt, branch, _, health) in &cases {
        if *health == "left" {
            r.assert_removed(wt, &said);
            assert_eq!(r.branch(branch), "", "{said}");
        } else {
            r.assert_kept(wt, &said);
            assert_eq!(r.branch(branch), *branch, "{said}");
        }
    }
}

#[test]
fn a_bound_agent_with_no_own_worktree_does_not_keep_its_old_ref() {
    let r = Repo::new();
    git(&r.root, &["branch", "worktree-agent-gone", "main"]);
    let handle = r.bind(&r.root);
    assert!(r.board.store.bind_agent("gone", &handle, false).unwrap());
    r.sweep();
    assert_eq!(r.branch("worktree-agent-gone"), "");
}

#[test]
fn other_merged_branches_are_not_deleted() {
    let r = Repo::new();
    git(&r.root, &["branch", "sim/landed-long-ago", "main"]);
    r.sweep();
    assert_eq!(r.branch("sim/landed-long-ago"), "sim/landed-long-ago");
}

// -- the window of recent activity: what sees a live agent no lock or binding reaches ----------

#[test]
fn a_worktree_touched_within_two_hours_is_kept() {
    let r = Repo::new();
    let wt = r.add("fresh-agent", Some("worktree-agent-fresh"), "main");
    r.idle(&[&wt]);
    let said = text(&r.sweep_now(|_| {}));
    r.assert_kept(&wt, &said);
    assert_eq!(r.branch("worktree-agent-fresh"), "+ worktree-agent-fresh");
}

#[test]
fn each_thing_it_reads_alone_keeps_it() {
    let r = Repo::new();
    let trees = [0, 1, 2, 3].map(|index| r.detached(&format!("touched-{index}")));
    // One with nothing touched, which goes: each of the others is kept by its own marker alone.
    let untouched = r.detached("untouched");
    r.idle(&[]);
    for (index, wt) in trees.iter().enumerate() {
        set_time(&Repo::activity(wt)[index], SystemTime::now());
    }
    let said = text(&r.sweep_now(|_| {}));
    for wt in &trees {
        r.assert_kept(wt, &said);
    }
    r.assert_removed(&untouched, &said);
}

#[test]
fn activity_just_past_the_window_does_not_keep_it() {
    let r = Repo::new();
    let wt = r.detached("idle");
    Repo::age(&wt, 2.1);
    r.assert_removed(&wt, &text(&r.sweep_now(|_| {})));
}

#[test]
fn a_window_of_half_an_hour_lets_an_hour_idle_go() {
    let r = Repo::new();
    let wt = r.detached("idle-an-hour");
    Repo::age(&wt, 1.0);
    r.assert_kept(&wt, &text(&r.sweep_now(|_| {})));
    r.assert_removed(&wt, &text(&r.sweep_now(|o| o.recent_hours = 0.5)));
}

// -- races: work done after the checks and before the removal survives it --------------------

type Pause = Box<dyn Fn(&str, &str)>;

/// A pause that runs `act` when the sweep reaches `stage` for `wt`.
fn at(stage: &'static str, wt: &Path, act: impl Fn() + 'static) -> Pause {
    let wt = wt.canonicalize().unwrap();
    Box::new(move |reached, what| {
        if reached == stage && Path::new(what).canonicalize().ok().as_ref() == Some(&wt) {
            act();
        }
    })
}

#[test]
fn a_commit_made_after_the_checks_survives() {
    let r = Repo::new();
    let wt = r.detached("racing");
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("remove", &wt, move || {
            write(&there, "sim/world.gd", "extends RefCounted\n# more\n");
            git(&there, &["commit", "-qam", "work"]);
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert_ne!(
        git(&wt, &["rev-parse", "HEAD"]),
        git(&r.root, &["rev-parse", "main"])
    );
}

#[test]
fn an_untracked_file_made_after_the_checks_survives() {
    let r = Repo::new();
    let wt = r.detached("racing");
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("remove", &wt, move || {
            write(&there, "notes.txt", "written just now\n")
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert!(wt.join("notes.txt").is_file());
}

#[test]
fn a_commit_made_during_the_last_status_survives() {
    // HEAD is read again after the last look at what is left behind, not only before it.
    let r = Repo::new();
    let wt = r.detached("racing-status");
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("checked", &wt, move || {
            write(&there, "sim/world.gd", "extends RefCounted\n# more\n");
            git(&there, &["commit", "-qam", "work"]);
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert_ne!(
        git(&wt, &["rev-parse", "HEAD"]),
        git(&r.root, &["rev-parse", "main"])
    );
}

#[test]
fn an_ignored_file_that_is_not_output_made_after_the_checks_survives() {
    // git's own remove lets ignored files go; the sweep's measure keeps local settings, checked again.
    let r = Repo::new();
    let wt = r.detached("racing-settings");
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("remove", &wt, move || {
            write(&there, ".claude/settings.local.json", "{}\n");
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert!(wt.join(".claude/settings.local.json").is_file());
}

#[test]
fn a_probe_made_after_the_checks_in_a_worktree_whose_task_is_not_finished_survives() {
    let r = Repo::with(Seed::default().task("sim/parked", "backlog"));
    let wt = r.merged("racing-probe", "sim/parked");
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("remove", &wt, move || {
            write(&there, "artifacts/hazel-swift/probe.json", "{}\n");
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert!(wt.join("artifacts/hazel-swift/probe.json").is_file());
}

#[test]
fn a_dead_lock_whose_removal_is_refused_is_locked_again() {
    let r = Repo::new();
    let wt = r.detached("racing-locked");
    let reason = format!(
        "claude agent agent-x (pid {} start {})",
        dead_pid(),
        started(me())
    );
    r.lock(&wt, &reason);
    let stages: Rc<RefCell<Vec<String>>> = Rc::default();
    let (seen, there) = (Rc::clone(&stages), wt.canonicalize().unwrap());
    let result = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            seen.borrow_mut().push(stage.to_string());
            if stage == "unlocked" && Path::new(what).canonicalize().unwrap() == there {
                // After the lock is lifted, before the final guard: the new file must keep it.
                write(&there, "notes.txt", "written just now\n");
            }
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert!(stages.borrow().iter().any(|s| s == "unlocked"));
    assert!(wt.join("notes.txt").is_file());
    let listing = git(&r.root, &["worktree", "list", "--porcelain"]);
    assert!(listing.contains(&format!("locked {reason}")), "{listing}");
}

#[test]
fn a_branch_moved_before_its_deletion_survives() {
    let r = Repo::new();
    let first = git(&r.root, &["rev-parse", "main"]);
    r.commit(&r.root, "extends RefCounted\n# more\n");
    git(&r.root, &["branch", "worktree-agent-moving", &first]);
    let root = r.root.clone();
    let result = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            if stage == "delete" && what == "worktree-agent-moving" {
                git(&root, &["branch", "-f", "worktree-agent-moving", "main"]);
            }
        });
    });
    assert_eq!(
        git(&r.root, &["rev-parse", "worktree-agent-moving"]),
        git(&r.root, &["rev-parse", "main"])
    );
    assert!(
        !result
            .branches
            .contains(&"worktree-agent-moving".to_string())
    );
}

/// A stage of the sweep, the tree it is reached for and what to do there.
type Act = (&'static str, PathBuf, Box<dyn Fn()>);

/// A pause that runs each tree's `act` when the sweep reaches its stage for that tree, a tree and
/// its stage named once.
fn at_each(acts: Vec<Act>) -> Pause {
    let acts: Vec<_> = acts
        .into_iter()
        .map(|(stage, wt, act)| (stage, wt.canonicalize().unwrap(), act))
        .collect();
    Box::new(move |reached, what| {
        let what = Path::new(what).canonicalize().ok();
        for (stage, wt, act) in &acts {
            if reached == *stage && what.as_ref() == Some(wt) {
                act();
            }
        }
    })
}

#[test]
fn a_session_or_launch_registered_during_the_sweep_keeps_its_tree() {
    let r = Repo::with(Seed::default().task("sim/starting", "ready"));
    let mut trees = Vec::new();
    let mut acts: Vec<Act> = Vec::new();
    for (binding, stage) in [
        ("session", "remove"),
        ("launch", "remove"),
        ("session", "checked"),
        ("launch", "checked"),
    ] {
        let wt = r.detached(&format!("starting-{binding}-{stage}"));
        let (store, there, root) = (r.board.other(), wt.clone(), r.root.clone());
        let act = move || {
            let place = if binding == "session" { &there } else { &root };
            let handle = store
                .register(&NewSession {
                    worktree: place.to_string_lossy().into_owned(),
                    ..NewSession::new("worker")
                })
                .unwrap()
                .row
                .handle;
            if binding == "launch" {
                store
                    .record_launch(&NewLaunch {
                        worktree: there.to_string_lossy().into_owned(),
                        ..NewLaunch::new("sim/starting", &handle, "executor")
                    })
                    .unwrap();
            }
        };
        acts.push((stage, wt.clone(), Box::new(act)));
        trees.push(wt);
    }
    // One nothing starts in, which goes: each of the others is kept by its own registration.
    let nobody = r.detached("nobody-starting");
    let result = r.sweep_with(|o| o.pause = at_each(acts));
    let said = text(&result);
    for wt in &trees {
        r.assert_kept(wt, &said);
    }
    r.assert_removed(&nobody, &said);
}

#[test]
fn a_new_or_replaced_lock_is_never_unlocked_by_the_old_snapshot() {
    let r = Repo::new();
    let mut trees = Vec::new();
    let mut acts: Vec<Act> = Vec::new();
    for (name, stage, old) in [
        ("relocked-new", "remove", None),
        (
            "relocked-at-remove",
            "remove",
            Some("claude agent agent-x (pid 41 start Wed Sep 30 13:10:22 2026)"),
        ),
        (
            "relocked-at-checked",
            "checked",
            Some("claude agent agent-x (pid 41 start Wed Sep 30 13:10:22 2026)"),
        ),
    ] {
        let wt = r.detached(name);
        if let Some(reason) = old {
            r.lock(&wt, reason);
        }
        let (root, there) = (r.root.clone(), wt.clone());
        let act = move || {
            if old.is_some() {
                git(&root, &["worktree", "unlock", there.to_str().unwrap()]);
            }
            git(
                &root,
                &[
                    "worktree",
                    "lock",
                    "--reason",
                    "new owner",
                    there.to_str().unwrap(),
                ],
            );
        };
        acts.push((stage, wt.clone(), Box::new(act)));
        trees.push(wt);
    }
    let result = r.sweep_with(|o| {
        o.alive = Box::new(|_| false);
        o.pause = at_each(acts);
    });
    let said = text(&result);
    let listing = git(&r.root, &["worktree", "list", "--porcelain"]);
    for wt in &trees {
        r.assert_kept(wt, &said);
        let listed = listing
            .split("\n\n")
            .find(|entry| entry.lines().next() == Some(&format!("worktree {}", wt.display())))
            .unwrap_or_else(|| panic!("{listing}"));
        assert!(listed.contains("locked new owner"), "{listing}");
    }
}

#[test]
fn activity_after_the_first_check_keeps_each_activity_marker() {
    let r = Repo::new();
    let mut trees = Vec::new();
    let mut acts: Vec<Act> = Vec::new();
    for marker in 0..4 {
        let wt = r.detached(&format!("active-again-{marker}"));
        let touched = Repo::activity(&wt)[marker].clone();
        let act = move || set_time(&touched, SystemTime::now());
        acts.push(("checked", wt.clone(), Box::new(act)));
        trees.push(wt);
    }
    // One nothing touches, which goes: each of the others is kept by its own marker alone.
    let untouched = r.detached("idle-still");
    let result = r.sweep_with(|o| o.pause = at_each(acts));
    let said = text(&result);
    for wt in &trees {
        r.assert_kept(wt, &said);
    }
    r.assert_removed(&untouched, &said);
}

#[test]
fn the_final_scoped_process_search_can_keep_a_tree_the_shared_scan_missed() {
    for occupied in [Some(true), None] {
        let r = Repo::new();
        let wt = r.detached("new-process");
        let target = wt.clone();
        let called = Rc::new(RefCell::new(false));
        let seen = Rc::clone(&called);
        let result = r.sweep_with(|o| {
            o.occupied = Box::new(move |path| {
                assert_eq!(path, target);
                *seen.borrow_mut() = true;
                occupied
            });
        });
        assert!(*called.borrow());
        r.assert_kept(&wt, &text(&result));
    }
}

#[test]
fn a_tree_that_vanishes_during_the_sweep_does_not_stop_the_next_removal() {
    let r = Repo::new();
    let first = r.detached("a-vanished");
    let next = r.detached("b-next");
    let there = first.clone();
    let result = r.sweep_with(|o| {
        o.pause = at("remove", &first, move || {
            std::fs::remove_dir_all(&there).unwrap()
        });
    });
    r.assert_removed(&next, &text(&result));
    assert!(!r.worktrees().contains(&first));
    assert_eq!(result.removed.len(), 1);
}

#[test]
fn a_detached_agent_bound_just_before_ref_deletion_keeps_its_ref() {
    let r = Repo::new();
    let wt = r.detached("agent-starting");
    write(
        &wt,
        "notes.txt",
        "keep the candidate until a session binds\n",
    );
    git(&r.root, &["branch", "worktree-agent-starting", "main"]);
    let (store, root) = (r.board.other(), r.root.clone());
    let result = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, name| {
            if stage == "delete" && name == "worktree-agent-starting" {
                let handle = store
                    .register(&NewSession {
                        worktree: root.to_string_lossy().into_owned(),
                        ..NewSession::new("reviewer")
                    })
                    .unwrap()
                    .row
                    .handle;
                assert!(store.bind_agent("starting", &handle, false).unwrap());
            }
        });
    });
    assert!(result.branches.is_empty());
    assert_eq!(
        r.branch("worktree-agent-starting"),
        "worktree-agent-starting"
    );
}

#[test]
fn a_ref_checked_out_after_the_listing_is_kept() {
    let r = Repo::new();
    git(&r.root, &["branch", "worktree-agent-checkout", "main"]);
    let (root, wt) = (r.root.clone(), r.dir.join("new-checkout"));
    let result = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, name| {
            if stage == "delete" && name == "worktree-agent-checkout" {
                git(
                    &root,
                    &["worktree", "add", "-q", wt.to_str().unwrap(), name],
                );
            }
        });
    });
    assert!(result.branches.is_empty());
    assert_eq!(
        r.branch("worktree-agent-checkout"),
        "+ worktree-agent-checkout"
    );
}

#[test]
fn a_checkout_to_main_during_the_last_status_survives_even_without_a_window() {
    let r = Repo::new();
    let wt = r.detached("new-main");
    git(&r.root, &["checkout", "-q", "--detach"]);
    let there = wt.clone();
    let result = r.sweep_with(|o| {
        o.recent_hours = 0.0;
        o.pause = at("checked", &wt, move || {
            git(&there, &["checkout", "-q", "main"]);
        });
    });
    r.assert_kept(&wt, &text(&result));
    assert_eq!(git(&wt, &["branch", "--show-current"]), "main");
}

#[test]
fn an_agents_tree_recreated_after_removal_keeps_its_ref() {
    let r = Repo::new();
    let wt = r.add("agent-reused", Some("worktree-agent-reused"), "main");
    let (root, there, store) = (r.root.clone(), wt.clone(), r.board.other());
    let result = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, name| {
            if stage == "delete" && name == "worktree-agent-reused" {
                assert!(!there.exists());
                git(
                    &root,
                    &[
                        "worktree",
                        "add",
                        "-q",
                        "--detach",
                        there.to_str().unwrap(),
                        "main",
                    ],
                );
                let handle = store
                    .register(&NewSession {
                        worktree: root.to_string_lossy().into_owned(),
                        ..NewSession::new("reviewer")
                    })
                    .unwrap()
                    .row
                    .handle;
                assert!(store.bind_agent("reused", &handle, false).unwrap());
            }
        });
    });
    assert_eq!(result.removed.len(), 1);
    assert!(wt.is_dir());
    assert!(result.branches.is_empty());
    assert_eq!(r.branch("worktree-agent-reused"), "worktree-agent-reused");
}

#[test]
fn a_process_entering_after_status_or_unlock_keeps_its_tree() {
    for (stage, locked) in [("checked", false), ("unlocked", false), ("unlocked", true)] {
        let r = Repo::new();
        let wt = r.detached("late-process");
        let reason = format!(
            "claude agent agent-x (pid {} start {})",
            dead_pid(),
            started(me())
        );
        if locked {
            r.lock(&wt, &reason);
        }
        let child: Rc<RefCell<Option<HeldCwd>>> = Rc::default();
        let (running, there) = (Rc::clone(&child), wt.clone());
        let result = r.sweep_with(|o| {
            let occupied = |tree: &Path| {
                process_cwd::occupied_with(symphony_board::sys::command("lsof"), tree)
            };
            o.occupied = Box::new(occupied);
            o.pause = at(stage, &wt, move || {
                *running.borrow_mut() = Some(HeldCwd::at(&there.join("sim")));
                assert_eq!(occupied(&there), Some(true));
            });
        });
        assert!(child.borrow().is_some());
        r.assert_kept(&wt, &text(&result));
        if locked {
            assert!(
                git(&r.root, &["worktree", "list", "--porcelain"])
                    .contains(&format!("locked {reason}"))
            );
        }
    }
}

#[test]
fn a_clean_unmerged_commit_after_unlock_survives() {
    for branch in [None, Some("sim/late-commit")] {
        let r = Repo::new();
        let wt = r.add("late-commit", branch, "main");
        let there = wt.clone();
        let result = r.sweep_with(|o| {
            o.pause = at("unlocked", &wt, move || {
                write(
                    &there,
                    "sim/world.gd",
                    "extends RefCounted\n# late unmerged work\n",
                );
                git(&there, &["commit", "-qam", "late work"]);
            });
        });
        r.assert_kept(&wt, &text(&result));
        assert_ne!(
            git(&wt, &["rev-parse", "HEAD"]),
            git(&r.root, &["rev-parse", "main"])
        );
        assert!(git(&wt, &["status", "--porcelain"]).is_empty());
        assert!(git(&wt, &["reflog", "--format=%gs"]).contains("late work"));
    }
}

#[test]
fn head_and_its_branch_stay_locked_during_the_final_process_probe_and_release_after_refusal() {
    for branch in [None, Some("sim/guarded")] {
        let r = Repo::new();
        let wt = r.add("guarded", branch, "main");
        let old = git(&wt, &["rev-parse", "HEAD"]);
        let next = git(
            &wt,
            &["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "next"],
        );
        if branch.is_some() {
            git(&r.root, &["pack-refs", "--all", "--prune"]);
            let loose = PathBuf::from(git(
                &wt,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "refs/heads/sim/guarded",
                ],
            ));
            assert!(!loose.exists());
            let parent = loose.parent().unwrap();
            if parent.exists() {
                std::fs::remove_dir(parent).unwrap();
            }
            assert!(!parent.exists());
        }
        let (there, before, attempted) = (wt.clone(), old.clone(), next.clone());
        let called = Rc::new(RefCell::new(false));
        let seen = Rc::clone(&called);
        let result = r.sweep_with(|o| {
            o.occupied = Box::new(move |path| {
                assert_eq!(path, there);
                for args in [
                    vec!["update-ref", "HEAD", &attempted],
                    vec!["symbolic-ref", "HEAD", "refs/heads/other"],
                ] {
                    let done =
                        sys::output(sys::Command::of("git").args(&args).current_dir(path)).unwrap();
                    assert!(
                        !done.status.success(),
                        "writer succeeded under removal guard: {args:?}"
                    );
                    assert!(
                        String::from_utf8_lossy(&done.stderr).contains(".lock"),
                        "{}",
                        String::from_utf8_lossy(&done.stderr)
                    );
                    assert_eq!(git(path, &["rev-parse", "HEAD"]), before);
                }
                *seen.borrow_mut() = true;
                Some(true)
            });
        });
        assert!(*called.borrow());
        r.assert_kept(&wt, &text(&result));
        // The same writer succeeds after the guard ends: the refusal above was the lock.
        git(&wt, &["update-ref", "HEAD", &next]);
        assert_eq!(git(&wt, &["rev-parse", "HEAD"]), next);
        git(&wt, &["symbolic-ref", "HEAD", "refs/heads/other"]);
        assert_eq!(git(&wt, &["symbolic-ref", "HEAD"]), "refs/heads/other");
    }
}

#[test]
fn an_existing_head_or_branch_writer_lock_keeps_the_tree_and_is_never_removed() {
    for reference in ["HEAD", "refs/heads/sim/writer"] {
        let r = Repo::new();
        let wt = r.add("writer", Some("sim/writer"), "main");
        let lock = PathBuf::from(format!(
            "{}.lock",
            git(
                &wt,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    reference
                ]
            )
        ));
        std::fs::write(&lock, "another writer").unwrap();
        let result = r.sweep_with(|_| {});
        r.assert_kept(&wt, &text(&result));
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "another writer");
        if reference != "HEAD" {
            let head = PathBuf::from(format!(
                "{}.lock",
                git(
                    &wt,
                    &["rev-parse", "--path-format=absolute", "--git-path", "HEAD"]
                )
            ));
            assert!(!head.exists(), "partially acquired guard leaked HEAD.lock");
        }
    }
}

#[test]
fn replacement_worktree_locks_survive_every_dead_lock_release_interval() {
    for stage in ["clock", "probe", "unlock", "lock-claimed", "unlocked"] {
        for kind in ["foreign", "live", "same-reason"] {
            let r = Repo::new();
            let wt = r.detached("replacement-lock");
            let dead = dead_pid();
            let old = format!("claude agent agent-x (pid {dead} start Wed Sep 30 13:10:22 2026)");
            let reason = match kind {
                "live" => format!(
                    "claude agent agent-y (pid {} start {})",
                    me(),
                    started(me())
                ),
                "same-reason" => old.clone(),
                _ => "new foreign owner".to_string(),
            };
            r.lock(&wt, &old);
            let lock = Repo::activity(&wt)[0].parent().unwrap().join("locked");
            let identity = Rc::new(RefCell::new(None));
            let (recorded, root, there, locked, replacement) = (
                Rc::clone(&identity),
                r.root.clone(),
                wt.clone(),
                lock.clone(),
                reason.clone(),
            );
            let replace: Rc<dyn Fn()> = Rc::new(move || {
                assert!(recorded.borrow().is_none());
                if !matches!(stage, "lock-claimed" | "unlocked") {
                    git(&root, &["worktree", "unlock", there.to_str().unwrap()]);
                }
                git(
                    &root,
                    &[
                        "worktree",
                        "lock",
                        "--reason",
                        &replacement,
                        there.to_str().unwrap(),
                    ],
                );
                let m = std::fs::metadata(&locked).unwrap();
                *recorded.borrow_mut() = Some((m.dev(), m.ino(), std::fs::read(&locked).unwrap()));
            });
            let result = r.sweep_with(|o| {
                let replace_clock = Rc::clone(&replace);
                let times = Cell::new(0);
                o.now = Box::new(move || {
                    times.set(times.get() + 1);
                    if stage == "clock" && times.get() == 2 {
                        replace_clock();
                    }
                    sweep::trigger::now()
                });
                let replace_probe = Rc::clone(&replace);
                let probes = Cell::new(0);
                o.alive = Box::new(move |pid| {
                    probes.set(probes.get() + 1);
                    if stage == "probe" && probes.get() == 2 {
                        replace_probe();
                    }
                    pid != dead && process::pid_alive(pid)
                });
                let replace_pause = Rc::clone(&replace);
                o.pause = at(stage, &wt, move || replace_pause());
            });
            let expected = identity.borrow().clone().expect("replacement interval ran");
            r.assert_kept(&wt, &text(&result));
            let actual = std::fs::metadata(&lock).unwrap();
            assert_eq!(
                (actual.dev(), actual.ino()),
                (expected.0, expected.1),
                "{stage}/{kind}"
            );
            assert_eq!(std::fs::read(&lock).unwrap(), expected.2, "{stage}/{kind}");
            assert!(
                git(&r.root, &["worktree", "list", "--porcelain"])
                    .contains(&format!("locked {reason}"))
            );
        }
    }
}

#[test]
fn two_replacement_locks_are_both_preserved_when_restore_would_overwrite_the_newest() {
    let r = Repo::new();
    let wt = r.detached("two-replacements");
    let dead = dead_pid();
    let reason = format!("claude agent agent-x (pid {dead} start Wed Sep 30 13:10:22 2026)");
    r.lock(&wt, &reason);
    let (root, there) = (r.root.clone(), wt.clone());
    let result = r.sweep_with(|o| {
        o.alive = Box::new(move |pid| pid != dead && process::pid_alive(pid));
        o.pause = Box::new(move |stage, path| {
            if Path::new(path) != there {
                return;
            }
            if stage == "unlock" {
                git(&root, &["worktree", "unlock", there.to_str().unwrap()]);
                git(
                    &root,
                    &[
                        "worktree",
                        "lock",
                        "--reason",
                        "first replacement owner",
                        there.to_str().unwrap(),
                    ],
                );
            } else if stage == "lock-claimed" {
                git(
                    &root,
                    &[
                        "worktree",
                        "lock",
                        "--reason",
                        "second replacement owner",
                        there.to_str().unwrap(),
                    ],
                );
            }
        });
    });
    r.assert_kept(&wt, &text(&result));
    let admin = Repo::activity(&wt)[0].parent().unwrap().to_path_buf();
    assert_eq!(
        std::fs::read_to_string(admin.join("locked"))
            .unwrap()
            .trim(),
        "second replacement owner"
    );
    let saved: Vec<_> = std::fs::read_dir(&admin)
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".symphony-sweep-lock-")
        })
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        std::fs::read_to_string(saved[0].path()).unwrap().trim(),
        "first replacement owner"
    );
    // Even after the newer owner unlocks, the displaced owner still protects this tree.
    git(&r.root, &["worktree", "unlock", wt.to_str().unwrap()]);
    let later = r.sweep();
    r.assert_kept(&wt, &later);
    assert!(
        later.contains("saved worktree lock needs recovery"),
        "{later}"
    );
    assert!(saved[0].path().exists());
}

// -- the processes behind a lock: asked of the kernel; `ps` only dates them --------------------

#[test]
fn a_start_time_that_cannot_be_parsed_keeps_a_live_lock() {
    let r = Repo::new();
    let wt = r.detached("localized");
    r.claude_lock(&wt, &me().to_string(), &started(me()));
    let result = r.sweep_with(|o| {
        o.started = Box::new(|_| Some("mer. 30 sept. 10:50:28 2026".into())); // ps under fr_FR
    });
    r.assert_kept(&wt, &text(&result));
}

#[test]
fn a_ps_that_fails_keeps_a_live_lock() {
    let r = Repo::new();
    let wt = r.detached("no-ps");
    r.claude_lock(&wt, &me().to_string(), &started(me()));
    r.assert_kept(
        &wt,
        &text(&r.sweep_with(|o| o.started = Box::new(|_| None))),
    );
}

#[test]
fn a_process_we_may_not_signal_keeps_its_lock() {
    // pid 1 answers kill(0) with EPERM for a normal user: it is running.
    let r = Repo::new();
    let wt = r.detached("init");
    r.claude_lock(&wt, "1", "Thu Jan  1 00:00:00 1970");
    r.assert_kept(
        &wt,
        &text(&r.sweep_with(|o| o.started = Box::new(|_| None))),
    );
}

#[test]
fn a_pid_too_large_for_the_kernel_is_a_foreign_lock() {
    let r = Repo::new();
    let wt = r.detached("absurd-pid");
    r.claude_lock(&wt, "99999999999999999999", &started(me()));
    r.assert_kept(&wt, &r.sweep());
}
