//! What the agents API and the monitor ask of the machine: the test-slot locks, hc's terminals, the
//! branch checked out in a folder, and whether a task's branch landed.

use serde_json::{Value, json};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::cli::agents::Pane;
use symphony_board::cli::probes::{branch_of, hc_terminals, landed, machine, slot_runs, split};
use symphony_board::sys::Command;

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
    let mut child = Command::new("true").spawn().unwrap();
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
    let common = scratch("holding");
    let slots = common.join("sf-test-slots");
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
    let runs = slot_runs(&common).unwrap();
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
    let common = scratch("free");
    let slots = common.join("sf-test-slots");
    fs::create_dir_all(&slots).unwrap();
    let path = slots.join("slot-0.lock");
    let me = std::process::id();
    holder(&path, me, "/wt/a", r#", "handle": "amber-heron""#);
    let last_holder = fs::read_to_string(&path).unwrap();
    let held = locked(&path);
    // Keep a second descriptor alive, as another thread or child may do. Dropping
    // just the first descriptor would not release the shared lock description.
    let inherited = held.try_clone().unwrap();
    let runs = slot_runs(&common).unwrap();
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
            slot_runs(&common).unwrap().is_empty(),
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
    let common = scratch("dead");
    let slots = common.join("sf-test-slots");
    fs::create_dir_all(&slots).unwrap();
    holder(&slots.join("slot-0.lock"), dead_pid(), "/wt/a", "");
    let _held = locked(&slots.join("slot-0.lock"));
    assert!(slot_runs(&common).unwrap().is_empty());
}

#[test]
fn a_slot_file_that_cannot_be_opened_to_lock_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let common = scratch("readonly");
    let slots = common.join("sf-test-slots");
    fs::create_dir_all(&slots).unwrap();
    let path = slots.join("slot-0.lock");
    holder(&path, std::process::id(), "/wt/a", "");
    let _held = locked(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(slot_runs(&common).unwrap().is_empty());
}

#[test]
fn a_slot_file_that_names_no_pid_is_skipped() {
    let common = scratch("junk");
    let slots = common.join("sf-test-slots");
    fs::create_dir_all(&slots).unwrap();
    fs::write(slots.join("slot-0.lock"), "").unwrap();
    fs::write(slots.join("slot-1.lock"), "not json").unwrap();
    fs::write(slots.join("slot-2.lock"), r#"{"pid": "12"}"#).unwrap();
    fs::write(slots.join("slot-3.lock"), "[1]").unwrap();
    let _held: Vec<File> = (0..4)
        .map(|i| locked(&slots.join(format!("slot-{i}.lock"))))
        .collect();
    assert!(slot_runs(&common).unwrap().is_empty());
}

#[test]
fn a_standing_place_in_the_line_is_a_run_waiting_in_arrival_order() {
    let common = scratch("waiting");
    let slots = common.join("sf-test-slots");
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
    let runs = slot_runs(&common).unwrap();
    let seen: Vec<(&str, &str)> = runs
        .iter()
        .map(|r| (r.worktree.as_str(), r.phase.as_str()))
        .collect();
    assert_eq!(seen, [("/wt/early", "waiting"), ("/wt/late", "waiting")]);
}

#[test]
fn no_slot_folder_is_no_run() {
    assert!(slot_runs(&scratch("none")).unwrap().is_empty());
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
        Command::new("git")
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
