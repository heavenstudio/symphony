//! Exact recipient consequences on an isolated board, using real native envelopes.
use super::callbacks::Listener;
use super::common::Fixture;
use crate::children::Child as _;
use serde_json::json;
use std::os::unix::fs::MetadataExt;
use symphony_board::NewTask;
use symphony_board::codex_queue::PATIENCE_VAR;
use symphony_board::sys::{Command, Spawning as _};
use symphony_board::{Health, NewLaunch, cli, sys};

const EMPTY_PULL: &str = "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";
const BUSY_PULL: &str = "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";

struct CommandDirectory(std::path::PathBuf);

impl CommandDirectory {
    fn stop_inherited_child(&self) {
        let file = self.0.join("held-child.pid");
        let pid = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let _ = std::fs::remove_file(file);
        if let Some(pid) = pid.filter(|pid| *pid > 0) {
            let _ = std::process::Command::of("/bin/kill")
                .args(["-TERM", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status_locked();
        }
    }
}

impl Drop for CommandDirectory {
    fn drop(&mut self) {
        self.stop_inherited_child();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn listen(f: &Fixture, handle: &str) -> Listener {
    let listener = Listener::new(&format!("{}-{handle}", f.store.database().database()));
    f.store
        .callback(
            handle,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    listener
}

#[path = "pull_expectations.rs"]
mod expectations;
pub(super) use expectations::with_volume;
use expectations::{callback_state, listen_orchestrator};

#[path = "quiet_pull.rs"]
mod pull_updates;

#[test]
fn a_stale_child_running_a_test_leaves_idle_callback_rows_unchanged() {
    // Run in a private repository so the real machine probe sees only this test's slot.
    const CHILD: &str = "SYMPHONY_STALE_CHILD_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let dir = std::env::temp_dir().join(format!(
            "symphony-stale-child-{}-{backend:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _directory = CommandDirectory(dir.clone());
        assert!(
            std::process::Command::of("git")
                .args(["init", "-q"])
                .arg(&dir)
                .status_locked()
                .unwrap()
                .success()
        );
        let output = sys::output(Command::of(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("quiet_callbacks::a_stale_child_running_a_test_leaves_idle_callback_rows_unchanged::{}", if backend == Backend::PostgreSql { "postgresql" } else { "surrealdb" }),
                "--nocapture",
            ])
            .current_dir(&dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env(CHILD, "1")
            .env("HC_BIN", "/usr/bin/false")
            )
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    use crate::common::{Seed, session};
    let mut parent = session("parent");
    parent.kind = "orchestrator".into();
    let mut child = session("child");
    child.parent = Some("parent".into());
    let f = Fixture::new(
        Seed::default()
            .session_row(parent)
            .session_row(child)
            .task("tools/child", "in_progress")
            .role("tools/child", "child", "executor"),
    );
    std::fs::write(
        "symphony.toml",
        "[board]\nslot_locks = \".git/test-slots\"\n",
    )
    .unwrap();
    let slots = std::path::Path::new(".git/test-slots");
    std::fs::create_dir_all(slots).unwrap();
    let path = slots.join("slot-0.lock");
    std::fs::write(
        &path,
        json!({"pid":std::process::id(),"handle":"child","recipe":"verify"}).to_string(),
    )
    .unwrap();
    let slot = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    slot.lock().unwrap();
    let o = listen_orchestrator(&f, "parent", BUSY_PULL);
    f.advance(91.0);
    assert_eq!(f.health("child"), Health::Stale);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        callback_state(&f, "parent")["seen"]["children"]["child"],
        "Live"
    );
    let rows = || {
        let rows = |table: &str| f.store.raw().table(table).get().unwrap();
        json!({
            "states": rows("callback_state"), "counters": rows("counter"),
            "leases": rows("callback_lease"),
        })
    };
    let before = rows();
    for _ in 0..4 {
        f.advance(1.0);
        f.store.dispatch_callbacks().unwrap();
        assert_eq!(
            rows(),
            before,
            "an idle tick takes no lease and writes no rows"
        );
    }
    // Past the lost line the run still holds it: its parent is told no LOST, so no takeover.
    f.advance(600.0);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        callback_state(&f, "parent")["seen"]["children"]["child"],
        "Live"
    );
    slot.unlock().unwrap();
    f.store.dispatch_callbacks().unwrap();
    // Ending the test restores raw health: lost, said once the run no longer holds it, and its
    // task orphaned.
    let sender = f.session("worker");
    assert_eq!(
        o.drain(&f, &sender, "parent"),
        [
            "LOST child — its work is claimable: tools/child",
            "PULL executing=0 (was 1) in_progress_orphaned=1 (was 0)",
            "in_progress_orphaned p2 tools/child — Task tools/child",
        ],
        "ending the test restores raw health"
    );
}

#[test]
fn an_armed_idle_gate_delivers_a_reservation_lapse_without_a_marker_change() {
    use crate::common::{NOW, Seed, session};
    let mut orch = session("orchestrator");
    orch.kind = "orchestrator".into();
    let f = Fixture::new(
        Seed::default()
            .session_row(orch)
            .session("worker")
            .task("tools/reserved", "ready")
            .role("tools/reserved", "orchestrator", "orchestrator")
            .reserved("tools/reserved", "worker", Some(NOW + 30.0)),
    );
    let o = listen_orchestrator(
        &f,
        "orchestrator",
        "PULL executing=0 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 tools/reserved — Task tools/reserved (reserved worker)",
    );
    f.store.dispatch_callbacks().unwrap();
    let requests = f.store.database().requests();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        f.store.database().requests() - requests,
        5,
        "the idle gate is armed after task-block reconciliation; one canonical holdings read still detects hold-only changes"
    );
    let marker = || f.store.marker().unwrap();
    let before = marker();
    let checkpoint = || {
        f.store
            .raw()
            .row("callback_lease", "orchestrator")
            .field("pull")
            .unwrap()
    };
    assert_eq!(checkpoint(), json!(NOW));
    f.advance(31.0);
    assert_eq!(marker(), before);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(o.text(), "ROLE tools/reserved worker none, ready");
    assert_eq!(
        checkpoint(),
        json!(f.now()),
        "the lapse schedules the digest before its periodic deadline"
    );
}

fn old_seen_write(f: &Fixture, handle: &str) {
    let before = callback_state(f, handle);
    let mut legacy = before["seen"].clone();
    legacy
        .as_object_mut()
        .unwrap()
        .retain(|field, _| matches!(field.as_str(), "roles" | "statuses" | "ready" | "children"));
    f.store
        .raw()
        .row("callback_state", handle)
        .set(json!({"seen": legacy}))
        .unwrap();
    let after = callback_state(f, handle);
    assert!(after["seen"].get("latest_message").is_none());
    assert_eq!(after["message_cursor"], before["message_cursor"]);
    assert_eq!(after["delivery_version"], before["delivery_version"]);
}

#[test]
fn an_undelivered_first_empty_pull_keeps_its_key_until_it_is_received() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    // Persist the registration before its first dispatch, then lose the listener. This is the
    // same durable state a restart can find after registration committed but before delivery.
    f.store
        .raw()
        .row("callback_state", orch.as_str())
        .put(json!({"lease": f.now() + 60.0}))
        .unwrap();
    let o = listen(&f, &orch);
    drop(o);
    f.store
        .raw()
        .row("callback_state", orch.as_str())
        .set(json!({"lease": 0}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    let pending = f.store.callback_inbox(&orch).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].1, with_volume(&orch, 0, EMPTY_PULL));
    assert!(callback_state(&f, &orch).get("pull_entries").is_none());
    assert_eq!(
        callback_state(&f, &orch)["pull_queued"],
        json!([["low_volume", orch]])
    );
    for _ in 0..4 {
        f.advance(601.0);
        f.store.touch(&orch).unwrap();
        f.store.dispatch_callbacks().unwrap();
        assert_eq!(f.store.callback_inbox(&orch).unwrap(), pending);
    }
    let recovered = Listener::new(&format!("{}-first-retry", f.store.database().database()));
    f.store
        .callback(
            &orch,
            Some(&format!("uds:{}", recovered.path.display())),
            false,
            false,
        )
        .unwrap();
    assert_eq!(recovered.text(), with_volume(&orch, 0, EMPTY_PULL));
    assert_eq!(
        callback_state(&f, &orch)["pull_entries"],
        json!([["low_volume", orch]])
    );
    assert!(recovered.drain(&f, &sender, &orch).is_empty());
    f.advance(601.0);
    f.store.dispatch_callbacks().unwrap();
    assert!(recovered.drain(&f, &sender, &orch).is_empty());
}

#[test]
fn addressed_messages_verdicts_owner_messages_and_plain_bytes() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    f.task("tools/quiet");
    f.store
        .take_role("tools/quiet", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tools/quiet", &exe).unwrap();
    f.store.take_role("tools/quiet", &rev, "reviewer").unwrap();
    let o = listen_orchestrator(&f, &orch, BUSY_PULL);
    let e = listen(&f, &exe);
    let r = listen(&f, &rev);
    assert!(o.drain(&f, &exe, &orch).is_empty());
    for n in 0..5 {
        f.store
            .say("tools/quiet", &exe, &format!("progress {n}"), None)
            .unwrap();
    }
    let question = f
        .store
        .say("tools/quiet", &exe, "which scope?", Some("@orchestrator"))
        .unwrap();
    assert_eq!(
        f.store
            .thread("tools/quiet")
            .unwrap()
            .iter()
            .filter(|m| m.text.starts_with("progress "))
            .count(),
        5
    );

    f.store.move_to("tools/quiet", "review", &exe).unwrap();
    let launched = f.session("reviewer");
    f.store
        .take_role_beside("tools/quiet", &launched, "reviewer", Some("a second stamp"))
        .unwrap();
    f.store
        .record_launch(&NewLaunch::new("tools/quiet", &launched, "reviewer"))
        .unwrap();
    f.store
        .verdict("tools/quiet", &rev, "approve", "all clear")
        .unwrap();
    let verdict = f.store.thread("tools/quiet").unwrap().pop().unwrap();
    let line = format!(
        "VERDICT #{} tools/quiet from {rev} — approve: all clear",
        verdict.id
    );

    let mut unrelated = NewTask::new("tools/unrelated", "Unrelated", &orch);
    unrelated.status = "backlog".into();
    f.task_with(unrelated);
    for status in ["concept", "design", "backlog"] {
        f.store.move_to("tools/unrelated", status, &orch).unwrap();
    }
    let role = format!("ROLE tools/quiet {launched} reviewer, review");
    assert_eq!(
        o.drain(&f, &exe, &orch),
        [
            format!("MSG #{} tools/quiet from {exe} — which scope?", question.id),
            "STATUS tools/quiet in_progress → review".into(),
            role.clone(),
            line.clone(),
        ],
        "only the question, move, launch and verdict; no count-only PULL"
    );
    assert_eq!(
        r.drain(&f, &orch, &rev),
        [
            "STATUS tools/quiet in_progress → review".to_string(),
            role.clone()
        ]
    );
    assert_eq!(e.drain(&f, &orch, &exe), [role, line]);
    let owner = f
        .store
        .say("tools/quiet", "owner", "keep the result", Some(&rev))
        .unwrap();
    let line = format!("MSG #{} tools/quiet from owner — keep the result", owner.id);
    assert_eq!(o.text(), line);
    assert_eq!(e.text(), line);
    assert_eq!(r.text(), line);
}

#[test]
fn ready_again_is_the_other_actors_action_even_when_i_created_the_task() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let other = f.session("worker");
    let o = listen_orchestrator(&f, &orch, EMPTY_PULL);
    f.task_with(NewTask::new("tools/mine", "Mine", &orch));
    let initial = o.drain(&f, &other, &orch);
    let entry = "ready_unstaffed p2 tools/mine — Mine";
    assert_eq!(initial, ["PULL ready_unstaffed=1 (was 0)", entry]);
    f.store.move_to("tools/mine", "backlog", &other).unwrap();
    assert!(o.drain(&f, &other, &orch).is_empty());
    f.store.move_to("tools/mine", "ready", &other).unwrap();
    assert_eq!(
        o.drain(&f, &other, &orch),
        // Its count is as the last PULL said it: only the row it gained back.
        [
            "NEW tools/mine ready — Mine",
            "PULL counts unchanged",
            entry
        ]
    );
}

#[test]
fn a_new_root_subscription_has_no_history_or_subtask_role_pushes_but_keeps_future_verdicts() {
    for register_first in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let orch = f.session("orchestrator");
        let exe = f.session("worker");
        let rev = f.session("reviewer");
        f.task("tools/root");
        f.store.claim("tools/root", &exe).unwrap();
        for slug in ["tools/root/old", "tools/root/current"] {
            let mut task = NewTask::new(slug, "Child", "owner");
            task.parent = Some("tools/root".into());
            f.task_with(task);
            f.store.claim(slug, &exe).unwrap();
        }
        f.store
            .take_role("tools/root/old", &rev, "reviewer")
            .unwrap();
        f.historical_review("tools/root/old");
        f.store
            .verdict("tools/root/old", &rev, "approve", "old result")
            .unwrap();
        f.task("tools/old-gap");
        let o = if register_first {
            let o = listen_orchestrator(
                &f,
                &orch,
                "PULL executing=2 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 tools/old-gap — Jump departure",
            );
            f.store
                .take_role("tools/root", &orch, "orchestrator")
                .unwrap();
            o
        } else {
            f.store
                .take_role("tools/root", &orch, "orchestrator")
                .unwrap();
            listen_orchestrator(
                &f,
                &orch,
                "PULL executing=2 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 tools/old-gap — Jump departure",
            )
        };
        assert!(
            o.drain(&f, &exe, &orch).is_empty(),
            "no old ROLE or VERDICT, and no repeated initial PULL"
        );
        assert!(
            f.store
                .inbox(&orch)
                .unwrap()
                .iter()
                .any(|m| m.kind == "verdict")
        );
        f.store
            .take_role("tools/root/current", &rev, "reviewer")
            .unwrap();
        assert!(
            o.drain(&f, &exe, &orch).is_empty(),
            "subtask role changes are not root role changes"
        );
        f.historical_review("tools/root/current");
        f.store
            .verdict("tools/root/current", &rev, "approve", "new result")
            .unwrap();
        let verdict = f.store.thread("tools/root/current").unwrap().pop().unwrap();
        assert_eq!(
            o.drain(&f, &exe, &orch),
            [format!(
                "VERDICT #{} tools/root/current from {rev} — approve: new result",
                verdict.id
            )]
        );
    }
}

#[test]
fn an_unreviewed_task_returning_is_new_even_when_its_status_never_changes() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    let o = listen_orchestrator(&f, &orch, EMPTY_PULL);
    f.task("tools/review-gap");
    o.drain(&f, &exe, &orch);
    f.store.claim("tools/review-gap", &exe).unwrap();
    assert!(o.drain(&f, &exe, &orch).is_empty());
    f.store.move_to("tools/review-gap", "review", &exe).unwrap();
    let row = "review_unreviewed p2 tools/review-gap — Jump departure";
    assert_eq!(
        o.drain(&f, &exe, &orch),
        [
            "PULL ready_unstaffed=0 (was 1) review_unreviewed=1 (was 0)",
            row,
            "resolved ready_unstaffed tools/review-gap",
        ]
    );
    f.store
        .take_role("tools/review-gap", &rev, "reviewer")
        .unwrap();
    assert!(
        o.drain(&f, &exe, &orch).is_empty(),
        "staffing a gap only removes an entry"
    );
    f.store
        .release("tools/review-gap", &rev, Some("reviewer"))
        .unwrap();
    assert_eq!(f.store.get("tools/review-gap").unwrap().status, "review");
    // The counts are as the last PULL said them: only the row it gained back.
    assert_eq!(o.drain(&f, &exe, &orch), ["PULL counts unchanged", row]);
    f.store.dispatch_callbacks().unwrap();
    assert!(
        o.drain(&f, &exe, &orch).is_empty(),
        "the return pushes once"
    );
}

#[test]
fn a_reviewer_receives_the_verdict_or_its_status_without_duplicates() {
    for by_owner in [false, true] {
        // Imported boards can carry named senders too. A verdict still needs a registered
        // reviewer, even when its sender is the owner whose messages reach every participant.
        let f = Fixture::new(crate::common::Seed::default().session("owner"));
        let exe = f.session("worker");
        let first = f.session("reviewer");
        let second = f.session("reviewer");
        // Filed by its executor: the owner reviews only work it did not file.
        f.task_with(NewTask::new("tools/two-reviewers", "Two reviewers", &exe));
        f.store.claim("tools/two-reviewers", &exe).unwrap();
        // The owner first, whose words reach every participant; each other beside those before it,
        // saying why.
        if by_owner {
            f.store
                .take_role("tools/two-reviewers", "owner", "reviewer")
                .unwrap();
        }
        for who in [&first, &second] {
            f.store
                .take_role_beside(
                    "tools/two-reviewers",
                    who,
                    "reviewer",
                    Some("another stamp"),
                )
                .unwrap();
        }
        f.store
            .move_to("tools/two-reviewers", "review", &exe)
            .unwrap();
        let listener = listen(&f, &second);
        let author = if by_owner { "owner" } else { &first };
        f.store
            .verdict("tools/two-reviewers", author, "approve", "approved")
            .unwrap();
        let verdict = f
            .store
            .thread("tools/two-reviewers")
            .unwrap()
            .pop()
            .unwrap();
        let expected = if by_owner {
            format!(
                "VERDICT #{} tools/two-reviewers from owner — approve: approved",
                verdict.id
            )
        } else {
            "STATUS tools/two-reviewers review → done".to_string()
        };
        assert_eq!(listener.drain(&f, &exe, &second), [expected], "{by_owner}");
    }
}

#[test]
fn due_ticks_probe_once_and_idle_cursor_changes_do_not_probe() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tools/probes");
    f.store.claim("tools/probes", &worker).unwrap();
    f.store
        .take_role("tools/probes", &orch, "orchestrator")
        .unwrap();
    let o = listen_orchestrator(&f, &orch, BUSY_PULL);
    let dir = std::env::temp_dir().join(format!(
        "symphony-callback-probes-{}-{}",
        std::process::id(),
        f.store.database().database()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let directory = CommandDirectory(dir.clone());
    let hc = dir.join("hc");
    let log = dir.join("probes");
    let git = dir.join("git");
    let git_log = dir.join("repository-probes");
    crate::fixture_script::write(
        &git,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$PROBE_GIT_LOG\"\nexec /usr/bin/git \"$@\"\n",
        false,
    );
    crate::fixture_script::write(
        &hc,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$PROBE_LOG\"\nprintf '%s\\n' '{\"terminals\":[]}'\n",
        false,
    );
    let git_image = std::fs::metadata(&git).unwrap();
    let hc_image = std::fs::metadata(&hc).unwrap();
    assert_eq!(
        (git_image.dev(), git_image.ino()),
        (hc_image.dev(), hc_image.ino())
    );
    f.store
        .raw()
        .row("callback_state", orch.as_str())
        .set(json!({"host": sys::hostname(), "pull": 0}))
        .unwrap();
    f.store
        .raw()
        .row("callback_lease", orch.as_str())
        .set(json!({"pull": 0}))
        .unwrap();
    let dispatch = || {
        let output = sys::output(
            Command::of(env!("CARGO_BIN_EXE_tasks"))
                .args([
                    "callback",
                    &format!("uds:{}", o.path.display()),
                    "--as",
                    &orch,
                ])
                .current_dir(&dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env("TASKS_SERVER", f.store.database().addr())
                .env("TASKS_DATABASE", f.store.database().database())
                .env("TASKS_NOW", f.now().to_string())
                .env("TASKS_SWEEP", "off")
                // The command's own dispatch is the one counted: it ends before the count is read.
                .env("TASKS_DISPATCH", "inline")
                .env("HC_BIN", &hc)
                .env("CODEX_THREAD_ID", "")
                .env("CLAUDE_SESSION_ID", "")
                .env("PROBE_LOG", &log)
                .env("PROBE_GIT_LOG", &git_log)
                .env(
                    "PATH",
                    format!("{}:{}", dir.display(), std::env::var("PATH").unwrap()),
                ),
        )
        .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    let probes = || std::fs::read_to_string(&log).unwrap().lines().count();
    dispatch();
    let initial = probes();
    assert_eq!(initial, 1, "a scheduled digest samples the machine once");
    let repository_probes = || std::fs::read_to_string(&git_log).unwrap().lines().count();
    let initial_repository_probes = repository_probes();
    assert!(
        initial_repository_probes > 0,
        "the repository stand-in is reached"
    );
    for n in 0..5 {
        f.store
            .say("tools/probes", &worker, &format!("progress {n}"), None)
            .unwrap();
        dispatch();
        assert_eq!(
            probes(),
            initial,
            "a message cursor change is not a staffing change"
        );
        assert_eq!(
            repository_probes(),
            initial_repository_probes,
            "idle cursor changes do not construct the machine probe"
        );
    }
    f.advance(601.0);
    dispatch();
    assert_eq!(
        probes(),
        initial + 1,
        "a later due tick takes a fresh sample"
    );
    assert!(o.text().contains("in_progress_orphaned p2 tools/probes"));
    f.store
        .release("tools/probes", &worker, Some("executor"))
        .unwrap();
    dispatch();
    assert_eq!(
        probes(),
        initial + 2,
        "a staffing change also probes once before its periodic deadline"
    );
    assert_eq!(
        o.text(),
        format!(
            "ROLE tools/probes {worker} none, ready\nPULL ready_unstaffed=1 (was 0) in_progress_orphaned=0 (was 1)\nready_unstaffed p2 tools/probes — Jump departure\nresolved in_progress_orphaned tools/probes"
        )
    );
    dispatch();
    assert_eq!(
        probes(),
        initial + 2,
        "an unchanged idle tick does not probe"
    );
    f.task("tools/probe-gap");
    dispatch();
    assert_eq!(
        probes(),
        initial + 3,
        "new work refreshes the digest with one sample"
    );
    assert!(o.text().contains("NEW tools/probe-gap"));
    drop(directory);
    assert!(!dir.exists());
}

#[test]
fn one_dispatch_coalesces_launch_roles_and_suppresses_self_events() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    f.task("tools/launch");
    f.store
        .take_role("tools/launch", &orch, "orchestrator")
        .unwrap();
    let o = listen_orchestrator(
        &f,
        &orch,
        "PULL executing=0 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 tools/launch — Jump departure",
    );
    let r = listen(&f, &rev);
    o.drain(&f, &exe, &orch);
    for handle in [&orch, &rev] {
        f.store
            .raw()
            .table("callback_state")
            .matching(json!({"handle": handle}))
            .set(json!({"lease": f.now() + 60.0}))
            .unwrap();
    }
    f.store.reserve("tools/launch", &rev, 3600.0).unwrap();
    f.store.take_role("tools/launch", &rev, "reviewer").unwrap();
    f.store
        .release("tools/launch", &rev, Some("reserved"))
        .unwrap();
    f.store
        .raw()
        .table("callback_state")
        .set(json!({"lease": 0}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        o.drain(&f, &exe, &orch),
        [format!("ROLE tools/launch {rev} reviewer, ready")]
    );
    assert!(r.drain(&f, &exe, &rev).is_empty());
}

#[test]
fn a_later_blocked_move_is_not_hidden_by_a_verdict_in_the_same_dispatch() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    f.task("tools/batched");
    f.store
        .take_role("tools/batched", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tools/batched", &exe).unwrap();
    f.store
        .take_role("tools/batched", &rev, "reviewer")
        .unwrap();
    f.store.move_to("tools/batched", "review", &exe).unwrap();
    let o = listen_orchestrator(&f, &orch, EMPTY_PULL);
    assert!(o.drain(&f, &exe, &orch).is_empty());
    f.store
        .raw()
        .row("callback_state", orch.as_str())
        .set(json!({"lease": f.now() + 60.0}))
        .unwrap();
    f.store
        .verdict("tools/batched", &rev, "changes", "fix the issue")
        .unwrap();
    let verdict = f.store.thread("tools/batched").unwrap().pop().unwrap();
    f.store.move_to("tools/batched", "blocked", &exe).unwrap();
    f.store
        .raw()
        .row("callback_state", orch.as_str())
        .set(json!({"lease": 0}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        o.drain(&f, &exe, &orch),
        [
            "STATUS tools/batched review → blocked".to_string(),
            format!(
                "VERDICT #{} tools/batched from {rev} — changes: fix the issue",
                verdict.id
            ),
        ]
    );
}

#[test]
fn changed_digest_is_plain_and_delivered_once_even_after_any_age() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    let o = listen_orchestrator(&f, &orch, EMPTY_PULL);
    assert!(o.drain(&f, &sender, &orch).is_empty());
    f.task_with(NewTask::new("tools/new", "Jump\ndeparture", "owner"));
    assert_eq!(
        o.text(),
        "NEW tools/new ready — Jump departure\nPULL ready_unstaffed=1 (was 0)\nready_unstaffed p2 tools/new — Jump departure"
    );
    for age in [601.0, 86_400.0] {
        f.advance(age);
        f.store.touch(&orch).unwrap();
        f.store.dispatch_callbacks().unwrap();
        assert!(
            o.drain(&f, &sender, &orch).is_empty(),
            "an entry that stays present is not re-sent"
        );
    }
    f.store.move_to("tools/new", "backlog", &sender).unwrap();
    assert!(
        o.drain(&f, &sender, &orch).is_empty(),
        "departures never trigger a digest"
    );
    f.store.move_to("tools/new", "ready", &sender).unwrap();
    assert_eq!(
        o.text(),
        "NEW tools/new ready — Jump departure\nPULL counts unchanged\nready_unstaffed p2 tools/new — Jump departure",
        "an entry absent from a computed digest becomes actionable again"
    );
    f.store.dispatch_callbacks().unwrap();
    assert!(
        o.drain(&f, &sender, &orch).is_empty(),
        "a return pushes once"
    );
    f.store.move_to("tools/new", "backlog", &sender).unwrap();
    assert!(o.drain(&f, &sender, &orch).is_empty());
    for age in [601.0, 86_400.0, 10_000_000.0] {
        f.advance(age);
        f.store.touch(&orch).unwrap();
        f.store.dispatch_callbacks().unwrap();
        assert!(o.drain(&f, &sender, &orch).is_empty());
    }
    f.task("tools/other");
    assert_eq!(
        o.text(),
        "NEW tools/other ready — Jump departure\nPULL counts unchanged\nready_unstaffed p2 tools/other — Jump departure\nresolved ready_unstaffed tools/new"
    );
    f.store.move_to("tools/new", "ready", &sender).unwrap();
    assert_eq!(
        o.text(),
        "NEW tools/new ready — Jump departure\nPULL ready_unstaffed=2 (was 1)\nready_unstaffed p2 tools/new — Jump departure",
        "a return is actionable when the last delivered digest lacked it"
    );
    f.store.send_agent(&sender, &orch, "two\nlines").unwrap();
    assert_eq!(
        o.text(),
        format!("AGENT from {sender} to {orch} — two lines")
    );
    assert!(f.store.send_agent(&orch, &orch, "self").unwrap());
    assert!(o.drain(&f, &sender, &orch).is_empty());
}

#[test]
fn a_pending_digest_keeps_its_retry_budget_until_a_new_action_replaces_it() {
    let f = Fixture::new(crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let sender = f.session("worker");
    let o = listen_orchestrator(&f, &orch, EMPTY_PULL);
    let path = o.path.clone();
    drop(o);
    f.task("tools/pending");
    for _ in 0..2 {
        f.advance(6.0);
        f.store.dispatch_callbacks().unwrap();
    }
    let state = || {
        f.store
            .raw()
            .row("callback_state", orch.as_str())
            .get()
            .unwrap()
    };
    assert_eq!(state()["pull_entries"], json!([["low_volume", orch]]));
    let pending = f.store.callback_inbox(&orch).unwrap();
    assert_eq!(
        pending
            .iter()
            .filter(|(_, text)| text.starts_with("PULL "))
            .count(),
        1
    );
    f.advance(601.0);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        f.store.callback_inbox(&orch).unwrap(),
        pending,
        "an unchanged pending digest keeps its key and retry budget at any age"
    );
    let original_key = pending
        .iter()
        .find(|(_, text)| text.starts_with("PULL "))
        .unwrap()
        .0
        .clone();
    f.store
        .move_to("tools/pending", "backlog", &sender)
        .unwrap();
    // A pending digest is said as the digest is when it goes out: with its one entry departed,
    // nothing is left in it that was not delivered, so it is taken back and pushes nothing.
    let departed = f.store.callback_inbox(&orch).unwrap();
    let pending_pull = (departed.iter()).find(|(_, text)| text.starts_with("PULL "));
    assert_eq!(pending_pull, None, "{departed:?}");
    f.store.move_to("tools/pending", "ready", &sender).unwrap();
    let returned = f.store.callback_inbox(&orch).unwrap();
    let pulls: Vec<_> = returned
        .iter()
        .filter(|(_, text)| text.starts_with("PULL "))
        .collect();
    assert_eq!(pulls.len(), 1);
    assert_ne!(
        pulls[0].0, original_key,
        "a returned action is a digest of its own, with a retry budget of its own"
    );
    f.task("tools/later");
    assert_eq!(
        f.store
            .callback_inbox(&orch)
            .unwrap()
            .iter()
            .filter(|(_, text)| text.starts_with("PULL "))
            .count(),
        1,
        "a new action replaces the pending digest instead of duplicating it"
    );
    // Re-register at a new listener after the retry budget; the latest durable digest is reused.
    let o = Listener::new(&format!("{}-recovered", f.store.database().database()));
    assert_ne!(o.path, path);
    f.store
        .callback(
            &orch,
            Some(&format!("uds:{}", o.path.display())),
            false,
            false,
        )
        .unwrap();
    let lines = o.drain(&f, &sender, &orch);
    assert_eq!(
        lines,
        [
            "NEW tools/pending ready — Jump departure".to_string(),
            "NEW tools/later ready — Jump departure".to_string(),
            "PULL ready_unstaffed=2 (was 0)".to_string(),
            "ready_unstaffed p2 tools/later — Jump departure".to_string(),
            "ready_unstaffed p2 tools/pending — Jump departure".to_string(),
        ]
    );
    assert_eq!(
        state()["pull_entries"],
        json!([
            ["low_volume", orch],
            ["ready_unstaffed", "tools/later"],
            ["ready_unstaffed", "tools/pending"]
        ])
    );
    f.advance(601.0);
    f.store.dispatch_callbacks().unwrap();
    assert!(o.drain(&f, &sender, &orch).is_empty());
    drop(o);
    f.store.move_to("tools/later", "backlog", &sender).unwrap();
    f.store
        .move_to("tools/pending", "backlog", &sender)
        .unwrap();
    f.task("tools/manual");
    let keys: Vec<String> = f
        .store
        .callback_inbox(&orch)
        .unwrap()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    f.store.ack_callbacks(&orch, &keys).unwrap();
    assert_eq!(
        state()["pull_entries"],
        json!([["low_volume", orch], ["ready_unstaffed", "tools/manual"]])
    );
    f.store.move_to("tools/manual", "backlog", &sender).unwrap();
    f.store.move_to("tools/later", "ready", &sender).unwrap();
    let retained = f.store.callback_inbox(&orch).unwrap();
    assert_eq!(retained.into_iter().filter(|(_,text)| text.starts_with("PULL ")).map(|(_,text)| text).collect::<Vec<_>>(), [
        // What changed since the PULL the receipt took, which said tools/manual.
        "PULL counts unchanged\nready_unstaffed p2 tools/later — Jump departure\nresolved ready_unstaffed tools/manual".to_string()
    ], "a manual inbox receipt also establishes the last delivered lists");
}

#[test]
fn batching_retry_and_chat_recipient_gating_keep_their_delivery_contracts() {
    let f = Fixture::new(crate::common::Seed::default());
    let sender = f.session("worker");
    let worker = f.session("worker");
    let chat = f.session("worker");
    f.task("tools/batch");
    f.store.claim("tools/batch", &worker).unwrap();
    let listener = listen(&f, &worker);
    f.store
        .raw()
        .row("callback_state", worker.as_str())
        .set(json!({"lease": f.now() + 60.0}))
        .unwrap();
    let first = f
        .store
        .say("tools/batch", &sender, "first", Some(&worker))
        .unwrap();
    let second = f
        .store
        .say("tools/batch", &sender, "second", Some(&worker))
        .unwrap();
    f.store
        .raw()
        .row("callback_state", worker.as_str())
        .set(json!({"lease": 0}))
        .unwrap();
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        listener.text(),
        format!(
            "MSG #{} tools/batch from {sender} — first\nMSG #{} tools/batch from {sender} — second",
            first.id, second.id
        )
    );
    drop(listener);
    let retry = f
        .store
        .say("tools/batch", &sender, "retry", Some(&worker))
        .unwrap();
    assert_eq!(
        f.store.inbox(&worker).unwrap().as_slice(),
        std::slice::from_ref(&retry)
    );
    let listener = Listener::new(&format!("{}-{worker}", f.store.database().database()));
    f.store.dispatch_callbacks().unwrap();
    f.advance(6.0);
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(
        listener.text(),
        format!("MSG #{} tools/batch from {sender} — retry", retry.id)
    );
    assert!(f.store.inbox(&worker).unwrap().is_empty());

    let owner_text = "CHAT docs/topic.md — owner question\nAnswer: symphony ask answer 12 TEXT";
    f.store
        .owner_callback("owner-question", owner_text)
        .unwrap();
    assert!(listener.drain(&f, &sender, &worker).is_empty());
    let c = listen(&f, &chat);
    f.store
        .callback(
            &chat,
            Some(&format!("uds:{}", c.path.display())),
            true,
            false,
        )
        .unwrap();
    assert_eq!(
        c.text(),
        owner_text,
        "owner chat retains its own reply information"
    );
    f.store
        .owner_callback("owner-question", owner_text)
        .unwrap();
    assert!(c.drain(&f, &sender, &chat).is_empty());
    assert!(listener.drain(&f, &sender, &worker).is_empty());
}

#[test]
fn a_legacy_upgrade_baselines_once_across_older_seen_writes_and_keeps_retries() {
    for pending in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let orch = f.session("orchestrator");
        let exe = f.session("worker");
        let rev = f.session("reviewer");
        f.task("tools/root");
        f.store
            .take_role("tools/root", &orch, "orchestrator")
            .unwrap();
        f.store.claim("tools/root", &exe).unwrap();
        f.store.take_role("tools/root", &rev, "reviewer").unwrap();
        let o = listen_orchestrator(&f, &orch, BUSY_PULL);
        assert!(o.drain(&f, &exe, &orch).is_empty());
        let delivered = f
            .store
            .say("tools/root", &exe, "already delivered", Some(&orch))
            .unwrap();
        assert_eq!(
            o.text(),
            format!(
                "MSG #{} tools/root from {exe} — already delivered",
                delivered.id
            )
        );
        assert!(f.store.inbox(&orch).unwrap().is_empty());

        // A compatibility dispatcher snapshot contains these tasks but only
        // direct-role watches. Preserve the delivered question acknowledgement.
        f.store
            .raw()
            .row("callback_state", orch.as_str())
            .set(json!({"lease": f.now() + 60.0}))
            .unwrap();
        for (slug, status) in [
            ("tools/root/finished", "done"),
            ("tools/root/cancelled", "cancelled"),
        ] {
            let mut task = NewTask::new(slug, "Historical work", "owner");
            task.parent = Some("tools/root".into());
            f.task_with(task);
            f.store.claim(slug, &exe).unwrap();
            f.store.take_role(slug, &rev, "reviewer").unwrap();
            if status == "done" {
                f.historical_review(slug);
                f.store
                    .verdict(slug, &rev, "approve", "historical verdict")
                    .unwrap();
                let verdict = f.store.thread(slug).unwrap().pop().unwrap();
                f.store.ack(&exe, &[verdict.id]).unwrap();
            } else {
                f.store.move_to(slug, status, &exe).unwrap();
            }
        }
        f.task("tools/old-gap");
        assert!(
            f.store
                .inbox(&orch)
                .unwrap()
                .iter()
                .any(|m| m.kind == "verdict")
        );
        if pending {
            assert!(!f.store.send_agent(&exe, &orch, "still pending").unwrap());
        }
        let mut legacy =
            serde_json::to_value(cli::attach::observe(&f.store, &orch, "orchestrator").unwrap())
                .unwrap();
        let object = legacy.as_object_mut().unwrap();
        object.remove("latest_message");
        object.remove("self_created");
        object.remove("verdict_tasks");
        object.remove("staffing");
        object
            .get_mut("roles")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .retain(|role| role[0] == "tools/root");
        let state = || f.store.raw().row("callback_state", orch.as_str());
        state()
            .set(json!({
                "pull_entries": null, "pull_queued": null, "message_floor": null,
                "message_cursor": null, "delivery_version": null,
            }))
            .unwrap();
        state().set(json!({"seen": legacy, "lease": 0})).unwrap();
        f.store.dispatch_callbacks().unwrap();
        let mut expected = if pending {
            vec![format!("AGENT from {exe} to {orch} — still pending")]
        } else {
            Vec::new()
        };
        expected.extend(with_volume(&orch, 1, "PULL executing=1 ready_unstaffed=1 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0\nready_unstaffed p2 tools/old-gap — Jump departure").lines().map(str::to_owned));
        assert_eq!(
            o.drain(&f, &exe, &orch),
            expected,
            "only the pending message and first warning; no historical ROLE or VERDICT"
        );
        assert_eq!(callback_state(&f, &orch)["delivery_version"], 1);
        for round in 0..4 {
            old_seen_write(&f, &orch);
            state().set(json!({"lease": f.now() + 60.0})).unwrap();
            let text = format!("new question {round}");
            let future = f.store.say("tools/root", &exe, &text, Some(&orch)).unwrap();
            // Older writers also advance their floor and inbox cursor after losing Seen's cursor.
            // Only the durable cursor owned by this stream may decide what is already observed.
            state()
                .set(json!({"message_floor": future.id, "inbox_cursor": future.id, "lease": 0}))
                .unwrap();
            assert!(
                callback_state(&f, &orch)["message_cursor"]
                    .as_i64()
                    .unwrap()
                    < future.id
            );
            f.store.dispatch_callbacks().unwrap();
            assert_eq!(
                o.text(),
                format!("MSG #{} tools/root from {exe} — {text}", future.id)
            );
            assert_eq!(callback_state(&f, &orch)["message_cursor"], future.id);
            assert_eq!(callback_state(&f, &orch)["delivery_version"], 1);
        }
        old_seen_write(&f, &orch);
        f.store.move_to("tools/root", "review", &exe).unwrap();
        assert_eq!(o.text(), "STATUS tools/root in_progress → review");
        old_seen_write(&f, &orch);
        f.store
            .verdict("tools/root", &rev, "approve", "future verdict")
            .unwrap();
        let verdict = f.store.thread("tools/root").unwrap().pop().unwrap();
        assert_eq!(
            o.text(),
            format!(
                "VERDICT #{} tools/root from {rev} — approve: future verdict",
                verdict.id
            )
        );
        f.store.dispatch_callbacks().unwrap();
        assert!(o.drain(&f, &exe, &orch).is_empty());
        assert!(
            f.store
                .inbox(&orch)
                .unwrap()
                .iter()
                .any(|m| m.kind == "verdict"),
            "historical inbox stays manually readable"
        );
        old_seen_write(&f, &orch);
        f.task("tools/future-gap");
        let lines = o.drain(&f, &exe, &orch);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let pulls: Vec<&String> = (lines.iter())
            .filter(|line| line.starts_with("PULL "))
            .collect();
        // What changed since the PULL it received: one count and one row, not tools/old-gap again.
        assert_eq!(
            pulls,
            ["PULL executing=0 (was 1) ready_unstaffed=2 (was 1)"]
        );
        assert!(lines.contains(&"NEW tools/future-gap ready — Jump departure".into()));
        assert!(!lines.contains(&"ready_unstaffed p2 tools/old-gap — Jump departure".into()));
        assert!(lines.contains(&"ready_unstaffed p2 tools/future-gap — Jump departure".into()));
        old_seen_write(&f, &orch);
        f.store.dispatch_callbacks().unwrap();
        assert!(o.drain(&f, &exe, &orch).is_empty());
    }
}

#[test]
fn codex_steering_requires_opt_in_and_preserves_queue_and_retry_without_a_live_daemon() {
    let f = Fixture::new(crate::common::Seed::default());
    let worker = f.session("worker");
    let sender = f.session("worker");
    let dir = std::env::temp_dir().join(format!(
        "symphony-codex-{}-{}",
        std::process::id(),
        f.store.database().database()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let directory = CommandDirectory(dir.clone());
    let config = |args: &[&str]| {
        let output = sys::output(
            Command::of("git")
                .args(args)
                .current_dir(&dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_CONFIG_COUNT")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "true"),
        )
        .unwrap();
        assert!(output.status.success(), "{output:?}");
    };
    config(&["init", "-q"]);
    let fake = dir.join("codex");
    let log = dir.join("calls.jsonl");
    let argv_log = dir.join("argv");
    crate::fixture_script::write(
        &fake,
        r#"#!/bin/bash
if [ "$1" = --fixture-ready ]; then
  jq -cn '"ready"'
  exit $?
fi
printf '%s\0' "$#" "$@" >> "$CODEX_ARGV"
if [ "$1" = queue ]; then
  [ "$CODEX_STATE" != fail ]
  exit $?
fi
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$CODEX_LOG"
  method=$(printf '%s\n' "$line" | jq -r .method)
  case "$method" in
    initialize)
      if [ "$CODEX_STATE" = stdout-held ]; then /bin/sleep 600 & printf '%s\n' "$!" > "$CODEX_CHILD_PID"; fi
      if [ "$CODEX_STATE" = unavailable ]; then exit 1; fi
      if [ "$CODEX_STATE" = legacy ]; then printf '%s\n' '{"id":1,"error":{"code":-32601}}'; else printf '%s\n' '{"id":1,"result":{}}'; fi ;;
    thread/read)
      case "$CODEX_STATE" in active|refused|ambiguous|stall|blocked|no-turn) state=active ;; *) state=idle ;; esac
      jq -cn --arg state "$state" --arg mode "$CODEX_STATE" '{id:2,result:{thread:{status:{type:$state},canAcceptDirectInput:($mode != "blocked")}}}' ;;
    thread/turns/list)
      if [ "$CODEX_STATE" = no-turn ]; then printf '%s\n' '{"id":3,"result":{"data":[]}}'; else printf '%s\n' '{"id":3,"result":{"data":[{"id":"turn-current","status":"inProgress"}]}}'; fi ;;
    turn/steer)
      if [ "$CODEX_STATE" = ambiguous ]; then exit 1; fi
      if [ "$CODEX_STATE" = stall ]; then IFS= read -r ignored; exit 0; fi
      if [ "$CODEX_STATE" = refused ]; then
        printf '%s\n' '{"id":4,"error":{"code":-32000,"message":"turn already ended"}}'
      else
        printf '%s\n' '{"method":"unrelated/notification"}' '{"id":4,"result":{"turnId":"turn-current"}}'
      fi ;;
  esac
done
"#,
        false,
    );
    // Only subprocesses see this PATH. Parallel tests and the parent can never reach this stub.
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    // Initialize the prepared interpreter and the fixture's jq before the transport starts;
    // every delivery below still has the production deadline and exact argument assertions.
    let ready = sys::output(Command::of(&fake).arg("--fixture-ready").env("PATH", &path)).unwrap();
    assert!(ready.status.success(), "{ready:?}");
    assert_eq!(ready.stdout, b"\"ready\"\n");
    let command = |state: &str, args: &[&str]| {
        let (binary, args) = if args.first() == Some(&"agent-send") {
            (env!("CARGO_BIN_EXE_agent-send"), &args[1..])
        } else {
            (env!("CARGO_BIN_EXE_tasks"), args)
        };
        let mut command = Command::of(binary);
        command
            .args(args)
            .current_dir(&dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "true")
            .env("TASKS_SERVER", f.store.database().addr())
            .env("TASKS_DATABASE", f.store.database().database())
            .env("TASKS_NOW", f.now().to_string())
            .env("TASKS_SWEEP", "off")
            // Each command's deliveries end with it, so the stand-in's log is read after them all.
            .env("TASKS_DISPATCH", "inline")
            .env("CODEX_THREAD_ID", "")
            .env("CLAUDE_SESSION_ID", "")
            .env("PATH", &path)
            .env("CODEX_LOG", &log)
            .env("CODEX_ARGV", &argv_log)
            .env("CODEX_STATE", state)
            .env("CODEX_CHILD_PID", dir.join("held-child.pid"));
        // A stand-in that answers is given the backstop of the delivery's 5 s, so a loaded
        // machine's slow jq never reads as a Codex that did not answer. The stall, which never
        // answers its steer, is the deadline's own case: it keeps the delivery's 5 s.
        if state != "stall" {
            let patience = test_time::backstop(std::time::Duration::from_secs(5));
            command.env(PATIENCE_VAR, patience.as_secs().to_string());
        }
        command
    };
    let run = |state: &str, args: &[&str]| {
        let output = sys::output(&mut command(state, args)).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run("idle", &["callback", "codex:abcdef01", "--as", &worker]);
    let read = || -> Vec<serde_json::Value> {
        let bytes = std::fs::read(&argv_log).unwrap_or_default();
        let mut fields = bytes.split(|byte| *byte == 0);
        let mut calls = Vec::new();
        while let Some(count) = fields.next().filter(|field| !field.is_empty()) {
            let count = std::str::from_utf8(count)
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let args: Vec<String> = (0..count)
                .map(|_| {
                    String::from_utf8(fields.next().expect("complete argv record").to_vec())
                        .unwrap()
                })
                .collect();
            calls.push(json!(args));
        }
        calls.extend(
            std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()),
        );
        calls
    };
    let send = |state: &str, text: &str| {
        std::fs::write(&log, "").unwrap();
        std::fs::write(&argv_log, "").unwrap();
        let output = run(state, &["agent-send", &worker, text, "--as", &sender]);
        let calls = read();
        assert!(
            !calls.is_empty(),
            "stand-in not reached: {output:?}; state {:?}; inbox {:?}",
            f.store.raw().row("callback_state", worker.as_str()).get(),
            f.store.callback_inbox(&worker)
        );
        calls
    };
    let plain = format!("AGENT from {sender} to {worker} — idle message");
    for value in [None, Some("false"), Some("invalid")] {
        if let Some(value) = value {
            config(&["config", "--local", "symphony.codexSteer", value]);
        }
        let calls = send("active", "mid-turn with steering disabled");
        assert_eq!(
            calls,
            [json!([
                "queue",
                "--thread",
                "abcdef01",
                "--message",
                format!("AGENT from {sender} to {worker} — mid-turn with steering disabled")
            ])],
            "{value:?}: disabled steering never starts a proxy"
        );
        assert!(f.store.callback_inbox(&worker).unwrap().is_empty());
    }
    config(&["config", "--local", "symphony.codexSteer", "true"]);
    let calls = send("idle", "idle message");
    assert_eq!(
        calls
            .iter()
            .filter(|v| v.is_array())
            .cloned()
            .collect::<Vec<_>>(),
        [
            json!(["app-server", "proxy"]),
            json!(["queue", "--thread", "abcdef01", "--message", plain])
        ]
    );
    assert!(!calls.iter().any(|v| v["method"] == "turn/steer"));
    std::fs::write(&log, "").unwrap();
    std::fs::write(&argv_log, "").unwrap();
    let mut held = command(
        "stdout-held",
        &["agent-send", &worker, "inherited stdout", "--as", &sender],
    )
    .stdout(sys::Stdio::null())
    .stderr(sys::Stdio::null())
    .spawn()
    .unwrap();
    let status = test_time::until(
        "delivery ends without waiting for a descendant to release proxy stdout",
        test_time::backstop(std::time::Duration::from_secs(15)),
        || held.try_wait().unwrap(),
    )
    .unwrap_or_else(|stall| {
        directory.stop_inherited_child();
        let _ = held.kill();
        let _ = held.wait();
        panic!("{stall}");
    });
    assert!(status.success());
    let pid = std::fs::read_to_string(dir.join("held-child.pid")).unwrap();
    assert!(
        std::process::Command::of("/bin/kill")
            .args(["-0", pid.trim()])
            .status_locked()
            .unwrap()
            .success(),
        "the inherited stdout holder must still be alive when delivery exits"
    );
    assert!(read().iter().any(|v| v[0] == "queue"));
    directory.stop_inherited_child();
    for state in ["legacy", "unavailable", "blocked", "no-turn"] {
        let calls = send(state, "fallback");
        assert_eq!(
            calls.iter().filter(|v| v[0] == "queue").count(),
            1,
            "{state}: {calls:?}"
        );
        assert!(
            !calls.iter().any(|v| v["method"] == "turn/steer"),
            "{state}: {calls:?}"
        );
        assert!(f.store.callback_inbox(&worker).unwrap().is_empty());
    }
    let calls = send("active", "mid-turn message");
    assert_eq!(
        calls
            .iter()
            .filter(|v| v.is_array())
            .cloned()
            .collect::<Vec<_>>(),
        [json!(["app-server", "proxy"])]
    );
    assert_eq!(
        calls.iter().find(|v| v["method"] == "turn/steer").unwrap()["params"],
        json!({"threadId":"abcdef01","expectedTurnId":"turn-current","input":[{"type":"text","text":format!("AGENT from {sender} to {worker} — mid-turn message")}]})
    );
    let calls = send("refused", "turn ended");
    assert!(calls.iter().any(|v| v["method"] == "turn/steer"));
    assert!(calls.iter().any(|v| v[0] == "queue"));
    let calls = send("fail", "retained");
    assert!(calls.iter().any(|v| v[0] == "queue"));
    assert_eq!(f.store.callback_inbox(&worker).unwrap().len(), 1);
    f.advance(6.0);
    std::fs::write(&log, "").unwrap();
    std::fs::write(&argv_log, "").unwrap();
    run(
        "active",
        &["agent-send", &worker, "recovered", "--as", &sender],
    );
    let calls = read();
    assert_eq!(
        calls.iter().find(|v| v["method"] == "turn/steer").unwrap()["params"]["input"][0]["text"],
        format!(
            "AGENT from {sender} to {worker} — retained\nAGENT from {sender} to {worker} — recovered"
        )
    );
    assert!(f.store.callback_inbox(&worker).unwrap().is_empty());
    let calls = send("ambiguous", "uncertain delivery");
    assert!(calls.iter().any(|v| v["method"] == "turn/steer"));
    assert!(
        !calls.iter().any(|v| v[0] == "queue"),
        "an ambiguous steer must not also enqueue a duplicate"
    );
    assert_eq!(f.store.callback_inbox(&worker).unwrap().len(), 1);
    f.advance(6.0);
    let calls = send("stall", "timed out delivery");
    assert!(calls.iter().any(|v| v["method"] == "turn/steer"));
    assert!(!calls.iter().any(|v| v[0] == "queue"));
    assert_eq!(f.store.callback_inbox(&worker).unwrap().len(), 2);
    drop(directory);
    assert!(!dir.exists(), "the private stand-in repository is removed");
}
