//! What the agents API and the monitor ask of the machine: the test-slot locks, hc's terminals, the
//! branch checked out in a folder, whether a task's branch landed, and its tip ([`tips`]).

mod tips;

use crate::children::Child as _;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::cli::agents::{Began, Pane};
use symphony_board::cli::probes::{branch_of, hc_terminals, landed, machine, slot_runs, split};
use symphony_board::git::{checkout, linked};
use symphony_board::sweep::trigger::now as wall_clock;
use symphony_board::sys::{Command, boot_clock};

mod completeness;

/// A run's fields, in `Run`'s order, for comparing several at once.
type RunFields<'a> = (
    i64,
    &'a str,
    &'a str,
    &'a str,
    Option<f64>,
    &'a str,
    Option<&'a str>,
);

/// A folder of its own under the test target's scratch space, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("probes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A folder of its own outside any repository (the target dir may be inside this one).
fn outside(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("probes-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A pid no process has: a child that has been waited for.
fn dead_pid() -> u32 {
    let mut child = Command::of("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

fn holder(path: &Path, pid: u32, worktree: &str, extra: &str) {
    fs::write(
        path,
        format!(r#"{{"pid": {pid}, "worktree": "{worktree}", "recipe": "verify", "since": 1800000000.5{extra}}}"#),
    )
    .unwrap();
}

/// The file at `path` locked as its holder locks it, until the result is dropped.
fn locked(path: &Path) -> File {
    let file = File::options().read(true).append(true).open(path).unwrap();
    file.lock().unwrap();
    file
}

// -- the slot locks --------------------------------------------------------------------

#[test]
fn a_locked_slot_of_a_live_holder_is_a_run_holding_it() {
    let slots = scratch("holding").join("slots");
    fs::create_dir_all(&slots).unwrap();
    let me = std::process::id();
    holder(
        &slots.join("slot-0.lock"),
        me,
        "/wt/a",
        r#", "handle": "amber-heron""#,
    );
    holder(&slots.join("render-1.lock"), me, "/wt/b", "");
    let _held = (
        locked(&slots.join("slot-0.lock")),
        locked(&slots.join("render-1.lock")),
    );
    let runs = slot_runs(&slots).unwrap();
    let seen: Vec<RunFields> = runs
        .iter()
        .map(|r| {
            (
                r.pid,
                r.worktree.as_str(),
                r.recipe.as_str(),
                r.phase.as_str(),
                r.since,
                r.pool.as_str(),
                r.handle.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                me as i64,
                "/wt/a",
                "verify",
                "holding",
                Some(1_800_000_000.5),
                "run",
                Some("amber-heron")
            ),
            (
                me as i64,
                "/wt/b",
                "verify",
                "holding",
                Some(1_800_000_000.5),
                "render",
                None
            ),
        ]
    );
}

#[test]
fn a_slot_nobody_holds_is_no_run_though_it_keeps_its_last_holders_name() {
    let slots = scratch("free").join("slots");
    fs::create_dir_all(&slots).unwrap();
    let path = slots.join("slot-0.lock");
    let me = std::process::id();
    holder(&path, me, "/wt/a", r#", "handle": "amber-heron""#);
    let last_holder = fs::read_to_string(&path).unwrap();
    let held = locked(&path);
    // Keep a second descriptor alive, as another thread or child may do. Dropping
    // just the first descriptor would not release the shared lock description.
    let inherited = held.try_clone().unwrap();
    let runs = slot_runs(&slots).unwrap();
    assert_eq!(
        runs.len(),
        1,
        "the live holder must be visible before unlock"
    );
    assert_eq!(runs[0].pid, i64::from(me));
    assert_eq!(runs[0].phase, "holding");
    assert_eq!(runs[0].handle.as_deref(), Some("amber-heron"));

    held.unlock().unwrap();
    for _ in 0..2 {
        assert!(
            slot_runs(&slots).unwrap().is_empty(),
            "the unlocked slot is free despite its live last holder"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), last_holder);
        let after = File::options().read(true).append(true).open(&path).unwrap();
        after.try_lock().expect("looking left the slot free");
        after.unlock().unwrap();
    }
    drop(inherited);
}

#[test]
fn a_slot_whose_holder_is_dead_is_not_even_tried() {
    let slots = scratch("dead").join("slots");
    fs::create_dir_all(&slots).unwrap();
    holder(&slots.join("slot-0.lock"), dead_pid(), "/wt/a", "");
    let _held = locked(&slots.join("slot-0.lock"));
    assert!(slot_runs(&slots).unwrap().is_empty());
}

#[test]
fn a_slot_file_that_cannot_be_opened_to_lock_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let slots = scratch("readonly").join("slots");
    fs::create_dir_all(&slots).unwrap();
    let path = slots.join("slot-0.lock");
    holder(&path, std::process::id(), "/wt/a", "");
    let _held = locked(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(slot_runs(&slots).unwrap().is_empty());
}

#[test]
fn a_slot_file_that_names_no_pid_is_skipped() {
    let slots = scratch("junk").join("slots");
    fs::create_dir_all(&slots).unwrap();
    fs::write(slots.join("slot-0.lock"), "").unwrap();
    fs::write(slots.join("slot-1.lock"), "not json").unwrap();
    fs::write(slots.join("slot-2.lock"), r#"{"pid": "12"}"#).unwrap();
    fs::write(slots.join("slot-3.lock"), "[1]").unwrap();
    let _held: Vec<File> = (0..4)
        .map(|i| locked(&slots.join(format!("slot-{i}.lock"))))
        .collect();
    assert!(slot_runs(&slots).unwrap().is_empty());
}

#[test]
fn a_standing_place_in_the_line_is_a_run_waiting_in_arrival_order() {
    let slots = scratch("waiting").join("slots");
    fs::create_dir_all(&slots).unwrap();
    let me = std::process::id();
    for (seq, worktree) in [(12, "/wt/late"), (3, "/wt/early"), (7, "/wt/left")] {
        holder(
            &slots.join(format!("slot-{seq:012}.place")),
            me,
            worktree,
            "",
        );
    }
    holder(&slots.join("slot-x.place"), me, "/wt/not-a-number", "");
    holder(&slots.join("slot-²².place"), me, "/wt/not-ascii", "");
    holder(&slots.join("slot.place-new"), me, "/wt/unnamed", "");
    let _standing: Vec<File> = [12, 3]
        .iter()
        .map(|seq| locked(&slots.join(format!("slot-{seq:012}.place"))))
        .chain(["slot-x.place", "slot-²².place", "slot.place-new"].map(|n| locked(&slots.join(n))))
        .collect();
    let runs = slot_runs(&slots).unwrap();
    let seen: Vec<(&str, &str)> = runs
        .iter()
        .map(|r| (r.worktree.as_str(), r.phase.as_str()))
        .collect();
    assert_eq!(seen, [("/wt/early", "waiting"), ("/wt/late", "waiting")]);
}

#[test]
fn no_slot_folder_is_no_run() {
    assert!(
        slot_runs(&scratch("none").join("slots"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_machines_probe_reads_the_slot_locks_the_main_checkouts_project_names_and_none_unnamed() {
    let dir = repo("named-slots");
    let me = std::process::id();
    let mut held = Vec::new();
    for (folder, worktree) in [(".git/test-slots", "/wt/common"), ("slots", "/wt/project")] {
        let slots = dir.join(folder);
        fs::create_dir_all(&slots).unwrap();
        holder(&slots.join("slot-0.lock"), me, worktree, "");
        held.push(locked(&slots.join("slot-0.lock")));
    }
    let tree = dir.with_file_name(format!("probes-{me}-named-slots-wt"));
    let _ = fs::remove_dir_all(&tree);
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "tools/w",
            tree.to_str().unwrap(),
        ],
    );
    let mut env = hc_env("/nonexistent/hc", &[]);
    let worktrees = |env: &Env| -> Vec<String> {
        (machine(env).runs)()
            .unwrap()
            .into_iter()
            .map(|run| run.worktree)
            .collect()
    };
    for cwd in [&dir, &tree] {
        env.cwd = cwd.clone();
        // A project that names no folder has no slots to probe, whatever its common dir holds.
        let _ = fs::remove_file(dir.join("symphony.toml"));
        assert!(worktrees(&env).is_empty(), "{cwd:?}");
        // `.git/` names the common dir, any other path the main checkout's own folder; a linked
        // worktree reads its main checkout's setting.
        for (named, worktree) in [(".git/test-slots", "/wt/common"), ("slots", "/wt/project")] {
            fs::write(
                dir.join("symphony.toml"),
                format!("[board]\nslot_locks = \"{named}\"\n"),
            )
            .unwrap();
            assert_eq!(worktrees(&env), [worktree], "{cwd:?} {named}");
        }
    }
    fs::remove_dir_all(&tree).unwrap();
}

// -- hc --------------------------------------------------------------------------------

fn hc_env(bin: &str, extra: &[(&str, &str)]) -> Env {
    let mut vars: std::collections::BTreeMap<String, String> = [
        ("PATH", std::env::var("PATH").unwrap()),
        ("HC_BIN", bin.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    vars.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Env {
        vars,
        cwd: std::env::temp_dir(),
    }
}

#[test]
fn hc_s_terminals_are_its_live_list_and_one_without_an_address_is_none() {
    let dir = scratch("hc");
    let answer = dir.join("answer.json");
    fs::write(
        &answer,
        r#"{"terminals": [{"address": "t5", "agent": "claude", "status": "blocked",
             "name": "amber-heron", "cwd": "/wt/x", "state": "live"}, {"address": "t6"},
             {"address": ""}, {"name": "no-address"}]}"#,
    )
    .unwrap();
    let bin = format!("sh -c 'cat \"$0\"' '{}'", answer.display());
    let panes = hc_terminals(&hc_env(&bin, &[])).unwrap();
    let ids: Vec<&str> = panes.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["t5", "t6"]);
    let first = &panes[0];
    assert_eq!(
        (
            first.agent.as_deref(),
            first.status.as_deref(),
            first.name.as_deref(),
            first.cwd.as_str()
        ),
        (
            Some("claude"),
            Some("blocked"),
            Some("amber-heron"),
            "/wt/x"
        )
    );
}

#[test]
fn hc_is_asked_for_its_live_terminals_whatever_space_the_caller_runs_in() {
    let bin = r#"sh -c 'printf "{\"terminals\": [{\"address\": \"%s\"}]}" "$*"' hc"#;
    let ids = |extra: &[(&str, &str)]| -> Vec<String> {
        hc_terminals(&hc_env(bin, extra))
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect()
    };
    assert_eq!(ids(&[]), ["ls --json"]);
    assert_eq!(ids(&[("HC_TERMINAL_ID", "t9")]), ["ls --json"]);
}

#[test]
fn an_hc_that_cannot_answer_is_an_error() {
    for bin in [
        "/nonexistent/hc",
        "sh -c 'echo no socket >&2; exit 1'",
        "sh -c 'echo not json'",
        "true",
        "sh -c 'echo \"[1]\"'",
        r#"sh -c 'echo "{\"terminals\": 3}"'"#,
        r#"sh -c 'echo "{\"terminals\": [\"junk\"]}"'"#,
        "'unclosed",
        "  ",
    ] {
        assert!(hc_terminals(&hc_env(bin, &[])).is_err(), "{bin}");
    }
}

#[test]
fn an_hc_with_no_live_terminal_has_no_panes() {
    let bin = r#"sh -c 'echo "{\"terminals\": []}"'"#;
    assert!(hc_terminals(&hc_env(bin, &[])).unwrap().is_empty());
}

#[test]
fn a_terminal_is_read_whatever_else_hc_says_of_it() {
    let read = |terminal: Value| Pane::from_hc(terminal.as_object().unwrap());
    let pane = read(json!({
        "address": "t2", "agent": null, "status": 3, "argv": "not a list",
        "foreground": {"cwd": ""}, "cwd": "/wt/x", "name": "amber-heron", "keep": true,
    }))
    .unwrap();
    assert_eq!(
        (
            pane.id.as_str(),
            pane.agent,
            pane.status,
            pane.session,
            pane.cwd.as_str(),
            pane.name.as_deref()
        ),
        ("t2", None, None, None, "/wt/x", Some("amber-heron"))
    );
    assert!(read(json!({"name": "amber-heron"})).is_none());
    assert_eq!(pane.turn, None, "a missing counter is unknown");
    for (value, expected) in [
        (json!(0), Some(0)),
        (json!(7), Some(7)),
        (json!(u64::MAX), Some(u64::MAX)),
        (Value::Null, None),
        (json!(-1), None),
        (json!("7"), None),
        (json!(7.5), None),
        (json!(true), None),
    ] {
        assert_eq!(
            read(json!({"address": "t2", "turn": value})).unwrap().turn,
            expected
        );
    }
    // As `hc get` shows it: the status an object, the conversation in the foreground's argv.
    let got = read(json!({
        "address": "t4", "status": {"app": "codex", "state": "idle"},
        "foreground": {"argv": ["codex", "resume", "0abc"], "cwd": "/wt/fg", "name": "codex"},
        "cwd": "/wt/start",
    }))
    .unwrap();
    assert_eq!(
        (
            got.agent.as_deref(),
            got.status.as_deref(),
            got.session.as_deref(),
            got.cwd.as_str()
        ),
        (Some("codex"), Some("idle"), Some("0abc"), "/wt/fg")
    );
}

#[test]
fn a_binary_is_split_as_a_posix_shell_splits_it() {
    let cases: [(&str, &[&str]); 7] = [
        ("hc", &["hc"]),
        ("  bash   fake.sh ", &["bash", "fake.sh"]),
        (
            r#"sh -c 'cat "$0"' /a\ b"#,
            &["sh", "-c", "cat \"$0\"", "/a b"],
        ),
        (r#""a \"b\" \c" d"#, &["a \"b\" \\c", "d"]),
        ("a'b'\"c\"d", &["abcd"]),
        ("''", &[""]),
        ("a # b", &["a", "#", "b"]),
    ];
    for (text, words) in cases {
        assert_eq!(split(text).unwrap(), words, "{text}");
    }
    for bad in ["'a", "\"a", "a\\", "\"a\\"] {
        assert!(split(bad).is_err(), "{bad}");
    }
}

// -- git -------------------------------------------------------------------------------

/// A repository with one commit on `main`.
fn repo(name: &str) -> PathBuf {
    let dir = scratch(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "base"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        Command::of("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

#[test]
fn the_branch_of_a_folder_is_the_one_checked_out_there() {
    let dir = repo("branch");
    git(&dir, &["checkout", "-q", "-b", "tools/x"]);
    assert_eq!(branch_of(dir.to_str().unwrap()).unwrap(), "tools/x");
    git(&dir, &["checkout", "-q", "--detach"]);
    assert_eq!(branch_of(dir.to_str().unwrap()).unwrap(), "");
    let nowhere = outside("no-repo");
    assert_eq!(branch_of(nowhere.to_str().unwrap()).unwrap(), "");
    fs::remove_dir_all(nowhere).unwrap();
}

#[test]
fn a_merged_branch_with_commits_of_its_own_is_landed() {
    let dir = repo("merged");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "work"]);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["merge", "-q", "--no-ff", "-m", "land", "sim/a"]);
    assert!(landed(&dir, "sim/a"));
}

#[test]
fn an_unmerged_branch_is_not() {
    let dir = repo("unmerged");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "work"]);
    git(&dir, &["checkout", "-q", "main"]);
    assert!(!landed(&dir, "sim/a"));
}

#[test]
fn a_branch_cut_at_main_and_not_yet_used_is_not() {
    let dir = repo("unused");
    git(&dir, &["branch", "sim/a"]);
    assert!(!landed(&dir, "sim/a"));
}

#[test]
fn a_task_with_no_branch_is_not() {
    let dir = repo("nobranch");
    assert!(!landed(&dir, "sim/none"));
    let nowhere = outside("no-repo-landed");
    assert!(!landed(&nowhere, "sim/none"));
    fs::remove_dir_all(nowhere).unwrap();
}

#[test]
fn the_machines_probes_outside_a_repository_read_no_slots_and_ask_git_once_a_folder() {
    let nowhere = outside("machine");
    let mut env = hc_env("/nonexistent/hc", &[]);
    env.cwd = nowhere.clone();
    let probes = machine(&env);
    assert!((probes.runs)().is_err());
    assert!((probes.panes)().is_err());
    let dir = repo("machine-branch");
    git(&dir, &["checkout", "-q", "-b", "tools/y"]);
    let folder = dir.to_str().unwrap();
    assert_eq!((probes.branch_of)(folder).unwrap(), "tools/y");
    git(&dir, &["checkout", "-q", "-b", "tools/z"]);
    assert_eq!((probes.branch_of)(folder).unwrap(), "tools/y"); // asked once a view
    fs::remove_dir_all(nowhere).unwrap();
}

#[test]
fn a_folders_checkout_is_its_top_folder_and_branch_read_from_the_git_files() {
    let main = repo("checkout").canonicalize().unwrap();
    git(&main, &["checkout", "-q", "-b", "tools/x"]);
    fs::create_dir_all(main.join("deep/er")).unwrap();
    assert_eq!(
        checkout(&main.join("deep/er")),
        Some((main.clone(), "tools/x".to_string()))
    );
    assert!(!linked(&main), "the main checkout's .git is the common dir");
    let tree = main.with_file_name(format!("probes-{}-checkout-wt", std::process::id()));
    let _ = fs::remove_dir_all(&tree);
    let at = tree.to_str().unwrap();
    git(&main, &["worktree", "add", "-q", "-b", "tools/y", at]);
    assert_eq!(checkout(&tree), Some((tree.clone(), "tools/y".to_string())));
    assert!(linked(&tree));
    git(&tree, &["checkout", "-q", "--detach"]);
    assert_eq!(checkout(&tree), Some((tree.clone(), String::new())));
    let nowhere = outside("no-checkout");
    assert_eq!(checkout(&nowhere), None);
    assert!(!linked(&nowhere));
    assert_eq!(checkout(&nowhere.join("not-there")), None);
    fs::remove_dir_all(nowhere).unwrap();
    fs::remove_dir_all(tree).unwrap();
}

/// A process start and the readings bounding its spawn, all in the kernel's clock domain.
#[derive(Debug)]
struct SpawnSpan {
    from: f64,
    started: f64,
    to: f64,
}

#[test]
fn the_machine_lists_each_process_with_its_working_directory_and_when_it_started() {
    let folder = scratch("working").canonicalize().unwrap();
    let before = (wall_clock(), boot_clock());
    let mut cat = Command::of("cat");
    cat.current_dir(&folder)
        .stdin(symphony_board::sys::Stdio::piped())
        .stdout(symphony_board::sys::Stdio::null());
    let mut working = cat.spawn().unwrap();
    let after = (wall_clock(), boot_clock());
    let env = hc_env("/nonexistent/hc", &[]);
    let listed = (machine(&env).processes)();
    let _ = working.kill();
    let _ = working.wait();
    let listed = listed.expect("lsof lists the working directories");
    let there: Vec<_> = listed.listed.iter().filter(|p| p.cwd == folder).collect();
    assert_eq!(there.len(), 1, "{} in {listed:?}", folder.display());
    // Stamped by the wall clock to the microsecond on macOS; on Linux in clock ticks since boot, in
    // this boot, the clock `/proc/uptime` reads to the hundredth.
    let started = there[0].started.as_ref().expect("its start is read");
    let span = match (started, &before.1, &after.1) {
        (Began::At(at), None, None) => SpawnSpan {
            from: before.0,
            started: *at,
            to: after.0,
        },
        (Began::AfterBoot(at), Some(from), Some(to))
            if at.boot == from.boot && at.boot == to.boot =>
        {
            SpawnSpan {
                from: from.at,
                started: at.at,
                to: to.at,
            }
        }
        _ => panic!("{started:?} on a machine whose clock since boot reads {before:?}"),
    };
    assert!(
        (span.from - 0.01..=span.to + 0.01).contains(&span.started),
        "{span:?}; listed={listed:?}; before={before:?}; after={after:?}"
    );
    // This process, still running, is listed too: its working directory shared with the others.
    let here = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert!(
        listed
            .listed
            .iter()
            .any(|p| p.cwd == here && p.started.is_some()),
        "{} not among {listed:?}",
        here.display()
    );
}
