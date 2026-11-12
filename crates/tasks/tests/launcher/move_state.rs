//! `tools/move-state.sh`, the one command that moves a repository's Symphony state in its git
//! common dir into `symphony/`. Each case is a repository of its own under the temp folder, its old
//! folders holding marker files, and a machine folder of its own (`SYMPHONY_MACHINE_DIR`): nothing
//! here reads the machine's real one or reaches a real repository's state, board or server.

use super::{git, scratch};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Each old file of the common dir that moves, and where it comes to be. The board's server's
/// folder, the verdict journals of an older install and the client's build tools included.
const MOVED: [(&str, &str); 13] = [
    ("sf-tasks/data/000001.sst", "symphony/tasks/data/000001.sst"),
    ("sf-tasks/health.json", "symphony/tasks/health.json"),
    ("sf-surreal/3.3.0/surreal", "symphony/surreal/3.3.0/surreal"),
    (
        "sf-symphony/bin/symphony-server",
        "symphony/server/bin/symphony-server",
    ),
    (
        "sf-task-models/bge-small/model.onnx",
        "symphony/models/bge-small/model.onnx",
    ),
    ("sf-runs/2026-11-11.jsonl", "symphony/runs/2026-11-11.jsonl"),
    (
        "sf-cargo/dx-0.7.10/bin/dx",
        "symphony/tools/dx-0.7.10/bin/dx",
    ),
    (
        "sf-cargo/dx-home/tools/esbuild/esbuild",
        "symphony/tools/dx-home/tools/esbuild/esbuild",
    ),
    (
        "sf-docs/verdict-backlog/next",
        "symphony/server/verdict-backlog/next",
    ),
    (
        "sf-docs/verdict-publications/00000000000000000041.json",
        "symphony/server/verdict-publications/00000000000000000041.json",
    ),
    (
        "sf-docs/verdict-commits.lock",
        "symphony/server/verdict-commits.lock",
    ),
    (
        "sf-docs/verdict-writes.lock",
        "symphony/server/verdict-writes.lock",
    ),
    (
        "worktrees/wt/sf-sweep-remove.lock",
        "worktrees/wt/symphony-sweep-remove.lock",
    ),
];

/// What stays where it is: the project's own build folder and builds, and an older install's files.
const KEPT: [&str; 3] = [
    "sf-cargo/target/debug/keep",
    "sf-bin/tasks-0123456789abcdef",
    "sf-docs/index.html",
];

/// What every run that moves something ends with.
const NEXT: &str = "next: start the board and the shared server from builds of the new code (the \
                    next `just tasks` starts the board; `just symphony` the shared server); no \
                    older build may run on this repository again\n";

/// A repository whose main checkout is `main/`, with a linked worktree `wt/`, and a machine folder
/// of its own holding no server's record.
struct State {
    main: PathBuf,
    wt: PathBuf,
    common: PathBuf,
    machine: PathBuf,
}

struct Done {
    code: i32,
    out: String,
    err: String,
}

impl State {
    fn new(name: &str) -> State {
        let base = scratch(name);
        let main = base.join("main");
        git(&base, &["init", "-q", "-b", "main", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "main"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let machine = base.join("machine");
        fs::create_dir(&machine).unwrap();
        State {
            common: main.join(".git"),
            wt: base.join("wt"),
            main,
            machine,
        }
    }

    /// A file under the common dir holding its own path, its folders made.
    fn put(&self, path: &str) {
        self.write(path, &format!("{path}\n"));
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.common.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    /// Every old file of [`MOVED`] and [`KEPT`], and an empty verdict folder the server's folder
    /// holds where an older install's comes in.
    fn fill(&self) {
        for (old, _) in MOVED {
            self.put(old);
        }
        for kept in KEPT {
            self.put(kept);
        }
        fs::create_dir(self.common.join("sf-symphony/verdict-publications")).unwrap();
    }

    /// `bash tools/move-state.sh ARGS` run in `cwd`, on this repository's machine folder.
    fn run_in(&self, cwd: &Path, args: &[&str]) -> Done {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/move-state.sh");
        let mut bash = Command::new("bash");
        for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"] {
            bash.env_remove(name);
        }
        let done = bash
            .arg(script)
            .args(args)
            .current_dir(cwd)
            .env("SYMPHONY_MACHINE_DIR", &self.machine)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Done {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8(done.stdout).unwrap(),
            err: String::from_utf8(done.stderr).unwrap(),
        }
    }

    /// The script run in the main checkout, naming no checkout.
    fn run(&self, args: &[&str]) -> Done {
        self.run_in(&self.main, args)
    }

    /// Every entry under the common dir, a folder as `PATH/`, a file with what it holds.
    fn tree(&self) -> BTreeMap<String, String> {
        fn walk(top: &Path, dir: &Path, found: &mut BTreeMap<String, String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path
                    .strip_prefix(top)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                if fs::symlink_metadata(&path).unwrap().is_dir() {
                    found.insert(format!("{relative}/"), String::new());
                    walk(top, &path, found);
                } else {
                    found.insert(relative, fs::read_to_string(&path).unwrap_or_default());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.common, &self.common, &mut found);
        found
    }

    /// The files under the common dir whose path starts with `prefix`.
    fn files_under(&self, prefix: &str) -> BTreeSet<String> {
        self.tree()
            .into_keys()
            .filter(|path| path.starts_with(prefix) && !path.ends_with('/'))
            .collect()
    }

    /// What the script says moving each of `moves`, in order, then what it says after.
    fn said(&self, moves: &[&str], after: &str) -> String {
        let mut said = format!("in {}\n", self.common.display());
        for line in moves {
            said.push_str(&format!("moved {line}\n"));
        }
        said + after + NEXT
    }

    /// The line naming an older install's folder left behind.
    fn older_install(&self) -> String {
        format!(
            "sf-docs holds an older install and nothing Symphony reads: it is safe to delete ({})\n",
            self.common.join("sf-docs").display()
        )
    }

    /// That every file of [`MOVED`] is at its new place, holding what it held, and none at its old.
    fn moved(&self) {
        for (old, new) in MOVED {
            let at = self.common.join(new);
            assert_eq!(
                fs::read_to_string(&at).unwrap_or_else(|_| panic!("{new} is not there")),
                format!("{old}\n")
            );
            assert!(!self.common.join(old).exists(), "{old} is still there");
        }
    }
}

/// A process started for a case, ended and reaped when the case ends, however it ends.
struct Started(Child);

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `sleep 600`, a live pid no record really names.
fn sleeper() -> Started {
    Started(
        Command::new("sleep")
            .arg("600")
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

/// The pid of a process that has exited and been reaped.
fn dead_pid() -> u32 {
    let mut done = Command::new("true").spawn().unwrap();
    done.wait().unwrap();
    done.id()
}

/// What `ps` says process `pid` runs.
fn command_of(pid: u32) -> String {
    let listed = Command::new("ps")
        .args(["-ww", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&listed.stdout).trim().to_string()
}

/// One refusal, as the script writes it on stderr.
fn refusal(why: &str) -> String {
    format!("symphony move-state: {why}\n")
}

#[test]
fn the_whole_state_moves_into_one_folder_and_a_second_run_finds_nothing_to_move() {
    let s = State::new("move-whole");
    s.fill();
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(
        done.out,
        s.said(
            &[
                "sf-tasks to symphony/tasks",
                "sf-surreal to symphony/surreal",
                "sf-symphony to symphony/server",
                "sf-task-models to symphony/models",
                "sf-runs to symphony/runs",
                "sf-cargo/dx-0.7.10 to symphony/tools/dx-0.7.10",
                "sf-cargo/dx-home to symphony/tools/dx-home",
                "sf-docs/verdict-backlog to symphony/server/verdict-backlog",
                "sf-docs/verdict-publications to symphony/server/verdict-publications",
                "sf-docs/verdict-commits.lock to symphony/server/verdict-commits.lock",
                "sf-docs/verdict-writes.lock to symphony/server/verdict-writes.lock",
                "worktrees/wt/sf-sweep-remove.lock to worktrees/wt/symphony-sweep-remove.lock",
            ],
            &s.older_install()
        )
    );
    s.moved();
    // The project's own and the older install's files stay, and nothing else of the old names.
    for kept in KEPT {
        assert_eq!(
            fs::read_to_string(s.common.join(kept)).unwrap(),
            format!("{kept}\n")
        );
    }
    assert_eq!(
        s.files_under("sf-"),
        KEPT.into_iter().map(String::from).collect::<BTreeSet<_>>()
    );
    let new: BTreeSet<String> = MOVED
        .iter()
        .map(|(_, new)| new.to_string())
        .filter(|new| new.starts_with("symphony/"))
        .collect();
    assert_eq!(s.files_under("symphony/"), new);
    for gone in [
        "sf-tasks",
        "sf-surreal",
        "sf-symphony",
        "sf-task-models",
        "sf-runs",
    ] {
        assert!(!s.common.join(gone).exists(), "{gone}");
    }

    // A second run finds nothing to move and changes nothing.
    let before = s.tree();
    let again = s.run(&[]);
    assert_eq!(
        (again.code, again.out.as_str(), again.err.as_str()),
        (
            0,
            format!(
                "nothing to move in {}\n{}",
                s.common.display(),
                s.older_install()
            )
            .as_str(),
            ""
        )
    );
    assert_eq!(s.tree(), before);
}

#[test]
fn a_linked_worktrees_sweep_files_take_their_new_names_from_a_run_there() {
    let s = State::new("move-worktree");
    s.put("worktrees/wt/sf-sweep-remove.lock");
    s.write(
        "worktrees/wt/sf-sweep-verdict.json",
        "{\"verdict\":\"kept\"}",
    );
    s.put("worktrees/wt/.sf-sweep-lock-41");
    // Git's own files there stay as they are.
    let head = fs::read_to_string(s.common.join("worktrees/wt/HEAD")).unwrap();

    let done = s.run_in(&s.wt, &[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(
        done.out,
        s.said(
            &[
                "worktrees/wt/sf-sweep-remove.lock to worktrees/wt/symphony-sweep-remove.lock",
                "worktrees/wt/sf-sweep-verdict.json to worktrees/wt/symphony-sweep-verdict.json",
                "worktrees/wt/.sf-sweep-lock-41 to worktrees/wt/.symphony-sweep-lock-41",
            ],
            ""
        )
    );
    let admin = s.common.join("worktrees/wt");
    assert_eq!(
        fs::read_to_string(admin.join("symphony-sweep-remove.lock")).unwrap(),
        "worktrees/wt/sf-sweep-remove.lock\n"
    );
    assert_eq!(
        fs::read_to_string(admin.join("symphony-sweep-verdict.json")).unwrap(),
        "{\"verdict\":\"kept\"}"
    );
    assert_eq!(
        fs::read_to_string(admin.join(".symphony-sweep-lock-41")).unwrap(),
        "worktrees/wt/.sf-sweep-lock-41\n"
    );
    assert_eq!(s.files_under("worktrees/wt/sf-"), BTreeSet::new());
    assert_eq!(s.files_under("worktrees/wt/.sf-"), BTreeSet::new());
    assert_eq!(fs::read_to_string(admin.join("HEAD")).unwrap(), head);
    assert!(
        !s.common.join("symphony").exists(),
        "no folder is made for nothing"
    );
}

#[test]
fn a_dry_run_says_what_it_would_move_and_changes_nothing() {
    let s = State::new("move-dry");
    // Nothing of the project's own beside the build tools, nor of an older install beside the
    // verdicts: both folders are left empty, and go.
    for (old, _) in MOVED.iter().filter(|(old, _)| {
        old.starts_with("sf-cargo/") || old.starts_with("sf-docs/") || old.starts_with("sf-runs/")
    }) {
        s.put(old);
    }
    let before = s.tree();
    let dry = s.run(&["--dry-run"]);
    assert_eq!((dry.code, dry.err.as_str()), (0, ""), "{}", dry.out);
    assert_eq!(
        dry.out,
        format!(
            "in {} (a dry run: nothing changes)\n\
             would move sf-runs to symphony/runs\n\
             would move sf-cargo/dx-0.7.10 to symphony/tools/dx-0.7.10\n\
             would move sf-cargo/dx-home to symphony/tools/dx-home\n\
             would move sf-docs/verdict-backlog to symphony/server/verdict-backlog\n\
             would move sf-docs/verdict-publications to symphony/server/verdict-publications\n\
             would move sf-docs/verdict-commits.lock to symphony/server/verdict-commits.lock\n\
             would move sf-docs/verdict-writes.lock to symphony/server/verdict-writes.lock\n\
             would remove sf-cargo, left empty\n\
             would remove sf-docs, left empty\n",
            s.common.display()
        )
    );
    assert_eq!(s.tree(), before);

    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(
        done.out,
        s.said(
            &[
                "sf-runs to symphony/runs",
                "sf-cargo/dx-0.7.10 to symphony/tools/dx-0.7.10",
                "sf-cargo/dx-home to symphony/tools/dx-home",
                "sf-docs/verdict-backlog to symphony/server/verdict-backlog",
                "sf-docs/verdict-publications to symphony/server/verdict-publications",
                "sf-docs/verdict-commits.lock to symphony/server/verdict-commits.lock",
                "sf-docs/verdict-writes.lock to symphony/server/verdict-writes.lock",
            ],
            "removed sf-cargo, left empty\nremoved sf-docs, left empty\n"
        )
    );
    assert!(!s.common.join("sf-cargo").exists() && !s.common.join("sf-docs").exists());
}

#[test]
fn a_new_path_beside_an_old_one_refuses_and_moves_nothing() {
    let s = State::new("move-conflict");
    s.fill();
    s.put("symphony/tasks/data/000002.sst");
    s.put("sf-symphony/verdict-backlog/next");
    s.put("worktrees/wt/symphony-sweep-remove.lock");
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!((done.code, done.out.as_str()), (1, ""), "{}", done.err);
    assert_eq!(
        done.err,
        [
            "both sf-tasks and symphony/tasks exist: keep the one that holds the state, move the \
             other aside, and run this again",
            "both sf-docs/verdict-backlog and sf-symphony/verdict-backlog exist: keep the one that \
             holds the verdicts, move the other aside, and run this again",
            "both worktrees/wt/sf-sweep-remove.lock and worktrees/wt/symphony-sweep-remove.lock \
             exist: keep one, move the other aside, and run this again",
        ]
        .map(refusal)
        .concat()
    );
    assert_eq!(s.tree(), before);

    // A dry run says the same, and what it would move meanwhile.
    let dry = s.run(&["--dry-run"]);
    assert_eq!((dry.code, dry.err), (1, done.err));
    assert!(
        dry.out
            .contains("would move sf-surreal to symphony/surreal\n"),
        "{}",
        dry.out
    );
    assert_eq!(s.tree(), before);
}

#[test]
fn a_file_where_the_new_folder_goes_refuses_and_moves_nothing() {
    let s = State::new("move-file");
    s.put("sf-runs/2026-11-11.jsonl");
    s.put("sf-task-models/bge-small/model.onnx");
    s.write("symphony", "a file\n");
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            [
                "symphony is a file or a link, not a folder, so sf-task-models cannot move to \
                 symphony/models: move it aside and run this again",
                "symphony is a file or a link, not a folder, so sf-runs cannot move to \
                 symphony/runs: move it aside and run this again",
            ]
            .map(refusal)
            .concat()
        )
    );
    assert_eq!(s.tree(), before);
}

#[test]
fn a_process_naming_the_old_folders_refuses_with_its_pid() {
    let s = State::new("move-process");
    s.fill();
    // The board's SurrealDB as it is started, named by the common dir as given: on macOS the temp
    // folder's path without the /private its links resolve to.
    let common = s.common.to_str().unwrap();
    let given = common.strip_prefix("/private").unwrap_or(common);
    let surreal = Started(
        Command::new("bash")
            .args([
                "-c",
                "exec -a \"surreal start surrealkv://$common/sf-tasks/data\" sleep 600",
            ])
            .env("common", given)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = surreal.0.id();
    let named = format!("surreal start surrealkv://{given}/sf-tasks/data 600");
    let start = Instant::now();
    while command_of(pid) != named {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the process never took its name: {}",
            command_of(pid)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {pid} ({named}) runs on this repository's state: stop it and run this again"
            ))
        )
    );
    assert_eq!(s.tree(), before);

    drop(surreal);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    s.moved();
}

#[test]
fn a_live_server_in_either_record_refuses_and_a_dead_one_does_not() {
    let s = State::new("move-records");
    s.fill();
    let live = sleeper();
    let pid = live.0.id();
    let dead = dead_pid();
    let board = |pid: u32| {
        s.write(
            "sf-tasks/server.json",
            &format!("{{\"addr\":\"127.0.0.1:4321\",\"pid\":{pid},\"version\":\"3.3.0\"}}"),
        );
    };
    let machine = |pid: u32| {
        fs::write(
            s.machine.join("server.json"),
            format!("{{\"port\":8765,\"pid\":{pid},\"root\":\"/nowhere\"}}"),
        )
        .unwrap();
    };

    board(pid);
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {pid} (sleep 600), the board's server sf-tasks/server.json records, runs: \
                 stop it and run this again"
            ))
        )
    );
    assert_eq!(s.tree(), before);

    board(dead);
    machine(pid);
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {pid} (sleep 600), the machine's Symphony server ({}/server.json), runs and \
                 serves every registered project: stop it (just symphony --stop) and run this again",
                s.machine.display()
            ))
        )
    );
    assert_eq!(s.tree(), before);

    machine(dead);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    s.moved();
    assert!(s.common.join("symphony/tasks/server.json").is_file());
}

#[test]
fn a_wrong_argument_or_a_folder_outside_a_repository_is_refused() {
    let s = State::new("move-arguments");
    let usage = "usage: tools/move-state.sh [--dry-run] [CHECKOUT]\n";
    for args in [&["--now"][..], &["a", "b"]] {
        let done = s.run(args);
        assert_eq!(
            (done.code, done.out.as_str(), done.err.as_str()),
            (2, "", usage)
        );
    }
    let help = s.run(&["--help"]);
    assert_eq!((help.code, help.out.as_str()), (0, usage));

    let outside = s.machine.to_str().unwrap();
    let done = s.run(&[outside]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!("{outside} is not in a git repository"))
        )
    );
}
