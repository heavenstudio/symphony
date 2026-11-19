//! The server clock's terminal watch (`dispatch_callbacks_snapshot`), hc the crate's `fake-hc`
//! found on PATH as `hc`, as the shared server finds hc: each case runs again in a child process of
//! its own, in a repository of its own, its terminals a file the case rewrites between ticks.
use crate::common::Backend;

use super::common::Fixture;
use super::quiet_callbacks::listen;
use crate::children::Child as _;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use symphony_board::NewLaunch;
use symphony_board::sys::{Command, Spawning as _};

#[path = "approval_freshness.rs"]
mod freshness;

const IN_CHILD: &str = "SYMPHONY_BLOCKED_PANES_CHILD";
const EMPTY_PULL: &str = "PULL executing=0 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";
const BUSY_PULL: &str = "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";

/// A folder removed when the case ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// In the parent, run the case `name` again in a child process whose PATH finds `fake-hc` as `hc`
/// (each call it answers one line of `FAKE_HC_LOG`), in a repository of its own so no
/// slot or branch of this one is probed; false once it passed. In that child, true: the case goes
/// on.
fn in_child_with_hc(name: &str) -> bool {
    if std::env::var_os(IN_CHILD).is_some() {
        return true;
    }
    let dir = std::env::temp_dir().join(format!(
        "symphony-blocked-panes-{}-{}",
        std::process::id(),
        name.replace(':', "-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let scratch = Scratch(dir.clone());
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-hc"), bin.join("hc")).unwrap();
    let state = dir.join("hc.json");
    std::fs::write(&state, json!({"terminals": []}).to_string()).unwrap();
    assert!(
        std::process::Command::of("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status_locked()
            .unwrap()
            .success()
    );
    let output = symphony_board::sys::output(
        Command::of(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .current_dir(&dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("HC_BIN")
            .env_remove("HC_TERMINAL_ID")
            .env_remove("FAKE_HC_FAIL")
            .env(IN_CHILD, "1")
            .env("FAKE_HC_STATE", &state)
            .env("FAKE_HC_LOG", dir.join("hc.log"))
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            ),
    )
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    drop(scratch);
    false
}

fn paired_child(name: &str, backend: Backend) -> bool {
    let suffix = match backend {
        Backend::PostgreSql => "postgresql",
        Backend::SurrealDb => "surrealdb",
    };
    in_child_with_hc(&format!("{name}::{suffix}"))
}

/// What hc answers from now on: these terminals, or, given None, a state it cannot read, which it
/// refuses as hc does when it cannot be asked.
fn panes(terminals: Option<Value>) {
    let state = std::env::var("FAKE_HC_STATE").unwrap();
    let text = match terminals {
        Some(terminals) => json!({"terminals": terminals}).to_string(),
        None => "not json".to_string(),
    };
    std::fs::write(Path::new(&state), text).unwrap();
}

fn pane(id: &str, status: &str) -> Value {
    json!({"address": id, "agent": "codex", "status": status, "cwd": "/wt", "state": "live"})
}

/// A tick of the server clock.
fn tick(f: &Fixture) {
    f.store.dispatch_callbacks_snapshot().unwrap();
}

/// hc failing at two ticks of the server clock the outage's stretch apart, which says its WATCH.
fn hc_down_for_the_stretch(f: &Fixture) {
    panes(None);
    tick(f);
    f.advance(symphony_board::cli::attach::OUTAGE_SECONDS);
    tick(f);
}

/// The server clock's ticks for `seconds` of the fixture's clock: one now and one a second after
/// it, `seconds` times.
fn ticks(f: &Fixture, seconds: u32) {
    tick(f);
    for _ in 0..seconds {
        f.advance(1.0);
        tick(f);
    }
}

/// Launched into `pane`, on `slug`.
fn launch(f: &Fixture, slug: &str, handle: &str, pane: &str) {
    f.store
        .record_launch(&NewLaunch {
            pane: pane.into(),
            ..NewLaunch::new(slug, handle, "executor")
        })
        .unwrap();
}

on_both! {
fn a_blocked_pane_reaches_the_orchestrator_of_its_work_once_an_episode(backend: Backend) {
    if !paired_child("blocked_panes::a_blocked_pane_reaches_the_orchestrator_of_its_work_once_an_episode", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let parent = f.session("orchestrator");
    let other = f.session("orchestrator");
    let mut child = symphony_board::NewSession::new("worker");
    child.parent = Some(parent.clone());
    let worker = f.session_with(child);
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    // The orchestrator's own pane and a pane no session holds wait too, and are never said.
    launch(&f, "tests/pane", &orch, "t1");
    launch(&f, "tests/pane", &worker, "t2");
    panes(Some(json!([
        pane("t1", "blocked"),
        pane("t2", "working"),
        pane("t9", "blocked")
    ])));
    let (o, p, x) = (listen(&f, &orch), listen(&f, &parent), listen(&f, &other));
    let w = listen(&f, &worker);
    // The work is orch's: only its digest counts it.
    assert_eq!(
        o.text(),
        format!(
            "{BUSY_PULL}\nlow_volume {orch} open=1 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
        )
    );
    for (listener, handle) in [(&p, &parent), (&x, &other)] {
        assert_eq!(
            listener.text(),
            crate::quiet_callbacks::with_volume(handle, 0, EMPTY_PULL)
        );
    }
    tick(&f);
    let quiet: [&[String]; 4] = [&[], &[], &[], &[]];
    // Each stream read to a barrier another session sends.
    let heard = || {
        [
            o.drain(&f, &worker, &orch),
            p.drain(&f, &worker, &parent),
            x.drain(&f, &worker, &other),
            w.drain(&f, &other, &worker),
        ]
    };
    assert_eq!(heard(), quiet, "nothing they oversee waits");

    panes(Some(json!([pane("t1", "blocked"), pane("t2", "blocked")])));
    let blocked = format!(
        "BLOCKED {worker} tests/pane t2 — observed waiting on an approval or a question; inspect its terminal before acting"
    );
    tick(&f);
    tick(&f);
    let blocked = std::slice::from_ref(&blocked);
    // Not its parent, another orchestrator: the work is orch's, as LOST would say.
    let once: [&[String]; 4] = [blocked, &[], &[], &[]];
    assert_eq!(heard(), once, "the orchestrator of its work, once");

    panes(Some(json!([pane("t2", "working")])));
    tick(&f);
    assert_eq!(heard(), quiet, "an answered prompt says nothing");
    panes(Some(json!([pane("t2", "blocked")])));
    tick(&f);
    tick(&f);
    assert_eq!(heard(), once, "blocking again is a new episode");
}
}

on_both! {
fn hc_failing_is_said_once_and_a_known_block_is_not_repeated(backend: Backend) {
    if !paired_child("blocked_panes::hc_failing_is_said_once_and_a_known_block_is_not_repeated", backend)
    {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    launch(&f, "tests/pane", &worker, "t2");
    panes(Some(json!([pane("t2", "blocked")])));
    let o = listen(&f, &orch);
    assert_eq!(
        o.text(),
        crate::quiet_callbacks::with_volume(&orch, 1, BUSY_PULL)
    );
    tick(&f);
    assert_eq!(
        o.drain(&f, &worker, &orch),
        [format!(
            "BLOCKED {worker} tests/pane t2 — observed waiting on an approval or a question; inspect its terminal before acting"
        )]
    );

    panes(None);
    ticks(&f, 32);
    let said = o.drain(&f, &worker, &orch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("WATCH hc unanswered at observation: hc ls: fake_state: ")
            && said[0].ends_with(
                " — blocked terminals may have gone unreported; inspect hc before acting"
            ),
        "{said:?}"
    );

    // hc answers again with the same prompt still waiting: nothing new to say.
    panes(Some(json!([pane("t2", "blocked")])));
    f.advance(1.0);
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
    // A second outage is a second WATCH.
    panes(None);
    f.advance(1.0);
    ticks(&f, 30);
    assert_eq!(o.drain(&f, &worker, &orch).len(), 1);
}
}

on_both! {
fn with_no_session_launched_into_a_pane_there_is_nothing_to_watch(backend: Backend) {
    if !paired_child("blocked_panes::with_no_session_launched_into_a_pane_there_is_nothing_to_watch", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    let o = listen(&f, &orch);
    assert_eq!(
        o.text(),
        crate::quiet_callbacks::with_volume(&orch, 1, BUSY_PULL)
    );
    // hc cannot answer, but no session it oversees was launched into a terminal: no WATCH.
    panes(None);
    tick(&f);
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
}
}

/// An orchestrator, and a worker it oversees launched into `t2`, which waits: both handles, and
/// the orchestrator's stream past its first PULL.
fn waiting_worker(f: &Fixture) -> (String, String, super::callbacks::Listener) {
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    launch(f, "tests/pane", &worker, "t2");
    panes(Some(json!([pane("t2", "blocked")])));
    let o = listen(f, &orch);
    assert_eq!(
        o.text(),
        super::quiet_callbacks::with_volume(&orch, 1, BUSY_PULL)
    );
    (orch, worker, o)
}

on_both! {
fn two_clocks_on_one_board_say_an_episode_once(backend: Backend) {
    if !paired_child("blocked_panes::two_clocks_on_one_board_say_an_episode_once", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let (orch, worker, o) = waiting_worker(&f);
    // The slow clock has read what its recipients were told when the other clock's whole tick runs
    // (the shared server's beside a `just symphony --port N` one).
    let other = f.other();
    let slow = f.slow(move || {
        other.dispatch_callbacks_snapshot().unwrap();
    });
    slow.dispatch_callbacks_snapshot().unwrap();
    assert_eq!(
        o.drain(&f, &worker, &orch),
        [format!(
            "BLOCKED {worker} tests/pane t2 — observed waiting on an approval or a question; inspect its terminal before acting"
        )]
    );
}
}

on_both! {
fn a_tick_asks_hc_once_for_its_watch_its_pull_and_its_agents_view(backend: Backend) {
    if !paired_child("blocked_panes::a_tick_asks_hc_once_for_its_watch_its_pull_and_its_agents_view", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let (orch, worker, o) = waiting_worker(&f);
    let log = Path::new(&std::env::var("FAKE_HC_LOG").unwrap()).to_path_buf();
    let asked = || std::fs::read_to_string(&log).map_or(0, |text| text.lines().count());
    let env = symphony_board::cli::Env::of_process();
    for wait in [0.0, 601.0] {
        // The second tick also owes the orchestrator its periodic PULL, whose view reads the panes.
        f.advance(wait);
        let before = asked();
        // A tick as the server runs it: its callbacks, then its agents view.
        let board = f.store.dispatch_callbacks_snapshot().unwrap().unwrap();
        let probes = symphony_board::cli::probes::machine(&env);
        symphony_board::cli::agents::describe_board(&f.store, &probes, f.now(), false, &board)
            .unwrap();
        assert_eq!(asked() - before, 1, "after {wait} s");
    }
    assert!(o.drain(&f, &worker, &orch).contains(&format!(
        "BLOCKED {worker} tests/pane t2 — observed waiting on an approval or a question; inspect its terminal before acting"
    )));
}
}

on_both! {
fn a_block_waiting_out_an_outage_is_not_said_once_a_hand_over_took_its_task_away(backend: Backend) {
    if !paired_child("blocked_panes::a_block_waiting_out_an_outage_is_not_said_once_a_hand_over_took_its_task_away", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let successor = f.session("orchestrator");
    let (orch, worker, o) = waiting_worker(&f);
    // The orchestrator cannot be reached when the watch sees the pane wait.
    drop(o);
    tick(&f);
    let waiting = f.store.callback_inbox(&orch).unwrap();
    assert!(
        (waiting.iter()).any(|(_, text)| text.starts_with(&format!("BLOCKED {worker} "))),
        "{waiting:?}"
    );
    f.store.hand_over("tests/pane", &orch, &successor).unwrap();
    let back = crate::callbacks::Listener::new(&format!("{}-back", f.store.database().database()));
    let address = format!("uds:{}", back.path.display());
    (f.store.callback(&orch, Some(&address), false, false)).unwrap();
    // Nor is its own hand-over said to it.
    assert!(back.drain(&f, &worker, &orch).is_empty());
}
}

on_both! {
fn a_reviewers_delayed_notice_follows_the_current_owner_instead_of_its_parent(backend: Backend) {
    if !paired_child("blocked_panes::a_reviewers_delayed_notice_follows_the_current_owner_instead_of_its_parent", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let former = f.session("orchestrator");
    let current = f.session("orchestrator");
    let unrelated = f.session("orchestrator");
    let executor = f.session("worker");
    let mut child = symphony_board::NewSession::new("reviewer");
    child.parent = Some(former.clone());
    let reviewer = f.session_with(child);
    f.task("tests/reviewer");
    f.store.take_role("tests/reviewer", &former, "orchestrator").unwrap();
    f.store.claim("tests/reviewer", &executor).unwrap();
    f.store.move_to("tests/reviewer", "review", &executor).unwrap();
    f.store.take_role("tests/reviewer", &reviewer, "reviewer").unwrap();
    f.store.record_launch(&NewLaunch {
        pane: "t3".into(),
        claude_session: "codex:review-conversation".into(),
        ..NewLaunch::new("tests/reviewer", &reviewer, "reviewer")
    }).unwrap();
    panes(Some(json!([pane("t3", "working")])));
    let (old, next, other) = (
        listen(&f, &former),
        listen(&f, &current),
        listen(&f, &unrelated),
    );
    // Queue an observation while its original owner cannot be reached.
    drop(old);
    panes(Some(json!([pane("t3", "blocked")])));
    tick(&f);
    let notice = format!(
        "BLOCKED {reviewer} tests/reviewer t3 — observed waiting on an approval or a question; inspect its terminal before acting"
    );
    assert!(f.store.callback_inbox(&former).unwrap().iter().any(|(_, text)| text == &notice));
    f.store.hand_over("tests/reviewer", &former, &current).unwrap();
    tick(&f);
    let back = crate::callbacks::Listener::new(&format!("{}-back", f.store.database().database()));
    let address = format!("uds:{}", back.path.display());
    f.store.callback(&former, Some(&address), false, false).unwrap();
    let blocked = |lines: Vec<String>| lines.into_iter().filter(|line| line.starts_with("BLOCKED ")).collect::<Vec<_>>();
    assert_eq!(blocked(next.drain(&f, &executor, &current)), [notice]);
    assert!(blocked(back.drain(&f, &executor, &former)).is_empty());
    assert!(blocked(other.drain(&f, &executor, &unrelated)).is_empty());
    tick(&f);
    assert!(blocked(next.drain(&f, &executor, &current)).is_empty(), "one episode is said once");
}
}

on_both! {
fn a_quiet_child_at_its_prompt_whose_listener_answers_is_said_idle_once_across_wakes(backend: Backend) {
    if !paired_child("blocked_panes::a_quiet_child_at_its_prompt_whose_listener_answers_is_said_idle_once_across_wakes", backend) {
        return;
    }
    // The night of the incident: no tool call since before the sleep, its turn retrying its
    // model at each wake, then ended on an error at its prompt, its listener answering throughout.
    let f = Fixture::on(backend, crate::common::Seed::default());
    let orch = f.session("orchestrator");
    let mut child = symphony_board::NewSession::new("worker");
    child.parent = Some(orch.clone());
    let worker = f.session_with(child);
    f.task("tests/quiet");
    f.store.take_role("tests/quiet", &orch, "orchestrator").unwrap();
    f.store.claim("tests/quiet", &worker).unwrap();
    launch(&f, "tests/quiet", &worker, "t2");
    // Its terminal, named for it: the agents view finds a terminal of no conversation by name.
    let terminal = |status: &str| {
        let mut terminal = pane("t2", status);
        terminal["name"] = json!(worker);
        terminal
    };
    panes(Some(json!([terminal("working")])));
    let o = listen(&f, &orch);
    assert!(o.text().starts_with(BUSY_PULL));
    let w = listen(&f, &worker);
    // The barrier's sender, whose own write is no sign of the child's.
    let sender = f.session("worker");
    let state = std::env::temp_dir().join(format!(
        "quiet-at-prompt-{}-{}.json",
        std::process::id(),
        f.store.database().database()
    ));
    // Each wake as the server takes it: the sweep's round first, then the clock's tick.
    let wake = || {
        f.advance(1800.0);
        crate::common::sweep_presence(&f.store, state.clone());
        tick(&f);
        o.drain(&f, &sender, &orch)
    };
    for _ in 0..3 {
        assert_eq!(wake(), Vec::<String>::new(), "a turn under way is not idle");
    }
    panes(Some(json!([terminal("idle")])));
    assert_eq!(
        wake(),
        ["PULL idle=1 (was 0)".to_string(), format!("idle {worker}")],
        "at its prompt, idle: said once, neither LOST nor takeover"
    );
    for _ in 0..3 {
        assert_eq!(wake(), Vec::<String>::new(), "not again at each wake");
    }
    drop(w);
}
}

on_both! {
fn hc_unanswered_is_said_for_an_outage_of_the_clocks_awake_time_never_a_slow_answer(backend: Backend) {
    if !paired_child("blocked_panes::hc_unanswered_is_said_for_an_outage_of_the_clocks_awake_time_never_a_slow_answer", backend) {
        return;
    }
    let f = Fixture::on(backend, crate::common::Seed::default());
    let (orch, worker, o) = waiting_worker(&f);
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch).len(), 1, "its block, once");
    // The WATCH lines said since the last read; a sleep's gap may say the worker is quiet too.
    let watching = || {
        let lines = o.drain(&f, &worker, &orch);
        assert!(!lines.iter().any(|l| l.starts_with("BLOCKED ")), "{lines:?}");
        (lines.iter())
            .filter(|l| l.starts_with("WATCH hc unanswered at observation: "))
            .count()
    };
    let answering = || panes(Some(json!([pane("t2", "blocked")])));
    // The incident: the clock ticks once a second, and hc fails at one sample in four under load.
    for second in 0..40 {
        f.advance(1.0);
        panes((second % 4 != 3).then(|| json!([pane("t2", "blocked")])));
        tick(&f);
    }
    assert_eq!(watching(), 0, "a failed sample between answers is a slow answer");
    // Failing at every tick for the stretch: one WATCH, at its end and not before.
    answering();
    f.advance(1.0);
    tick(&f);
    panes(None);
    f.advance(1.0);
    ticks(&f, 29);
    assert_eq!(watching(), 0, "short of the stretch");
    f.advance(1.0);
    tick(&f);
    assert_eq!(watching(), 1, "the stretch");
    ticks(&f, 60);
    assert_eq!(watching(), 0, "still failing, no second");
    // Answered, then failing for the stretch again: a second.
    answering();
    f.advance(1.0);
    tick(&f);
    panes(None);
    f.advance(1.0);
    ticks(&f, 30);
    assert_eq!(watching(), 1, "a second outage");
    // A sleep spanning the stretch, failing at the clock's last tick before it and its first
    // after it: nothing, and the stretch starts again at the wake.
    answering();
    f.advance(1.0);
    tick(&f);
    panes(None);
    f.advance(1.0);
    ticks(&f, 20);
    f.advance(1_800.0);
    ticks(&f, 29);
    assert_eq!(watching(), 0, "the sleep is no part of the stretch");
    f.advance(1.0);
    tick(&f);
    assert_eq!(watching(), 1, "a stretch awake after the wake");
}
}
