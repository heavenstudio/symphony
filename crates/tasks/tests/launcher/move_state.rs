//! `tools/move-state.sh`, the one command that moves a repository's Symphony state in its git
//! common dir into `symphony/`. Each case is a repository of its own under the temp folder, its old
//! folders holding marker files, and a machine folder of its own (`SYMPHONY_MACHINE_DIR`): nothing
//! here reads the machine's real one or reaches a real repository's state, board or server.

use super::{git, scratch};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
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
        self.run_with(cwd, args, &[])
    }

    /// The script run as [`State::run_in`] runs it, with `vars` set too.
    fn run_with(&self, cwd: &Path, args: &[&str], vars: &[(&str, String)]) -> Done {
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
            .envs(vars.iter().map(|(name, value)| (*name, value.as_str())))
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

    /// What the script says moving each of `moves`, in order, a fenced folder's fence right after
    /// its move, then what it says after.
    fn said(&self, moves: &[&str], after: &str) -> String {
        let mut said = format!("in {}\n", self.common.display());
        for line in moves {
            said.push_str(&format!("moved {line}\n"));
            if let Some((old, _)) = FENCED
                .iter()
                .find(|(old, _)| line.starts_with(&format!("{old} to ")))
            {
                said.push_str(&fenced(old));
            }
        }
        said + after + NEXT
    }

    /// The script run in the main checkout with `mv` a stand-in that moves as `mv` does and then,
    /// at the `stop`-th move, fails: the run ends there, as one stopped between a move and what
    /// follows it.
    fn run_stopped(&self, stop: usize) -> Done {
        // Beside the repository, out of its common dir.
        let bin = self.main.parent().unwrap().join("bin");
        fs::create_dir_all(&bin).unwrap();
        super::fixture_script::write(
            &bin.join("mv"),
            "#!/bin/bash\ncommand -p mv \"$@\" || exit\ncount=\"${0%/*}/count\"\n\
             moves=$(( $(cat \"$count\" 2>/dev/null || echo 0) + 1 ))\necho \"$moves\" >\"$count\"\n\
             [ \"$moves\" -lt \"$MOVE_STOP\" ]\n",
            false,
        );
        let _ = fs::remove_file(bin.join("count"));
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        self.run_with(
            &self.main,
            &[],
            &[("PATH", path), ("MOVE_STOP", stop.to_string())],
        )
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

/// Wait until `ps` says process `pid` runs `named`, as a process that renamed itself comes to.
fn wait_named(pid: u32, named: &str) {
    let start = Instant::now();
    while command_of(pid) != named {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the process never took its name: {}",
            command_of(pid)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `sleep 600` run as the program `name` (its first word, which ps names it by) in `cwd`, and the
/// command line ps says it runs, once it does.
fn program(name: &str, cwd: &Path) -> (Started, String) {
    let started = Started(
        Command::new("bash")
            .args(["-c", "exec -a \"$0\" sleep 600", name])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let line = format!("{name} 600");
    wait_named(started.0.id(), &line);
    (started, line)
}

/// The folders a run leaves a fence in once they move, and where each went.
const FENCED: [(&str, &str); 4] = [
    ("sf-tasks", "symphony/tasks"),
    ("sf-surreal", "symphony/surreal"),
    ("sf-symphony", "symphony/server"),
    ("sf-task-models", "symphony/models"),
];

/// The one line a fence holds, naming where its folder went.
fn fence(new: &str) -> String {
    format!(
        "moved by tools/move-state.sh to {new}: run only builds of the new code on this repository\n"
    )
}

/// What a run says fencing `old`.
fn fenced(old: &str) -> String {
    format!("fenced {old}: a build of the old code fails there instead of making it again\n")
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
    // The project's own and the older install's files stay, the fences stand where the folders an
    // older build would make again were, and nothing else of the old names is left.
    for kept in KEPT {
        assert_eq!(
            fs::read_to_string(s.common.join(kept)).unwrap(),
            format!("{kept}\n")
        );
    }
    for (old, new) in FENCED {
        assert_eq!(
            fs::read_to_string(s.common.join(old)).unwrap(),
            fence(new),
            "{old}"
        );
    }
    assert_eq!(
        s.files_under("sf-"),
        KEPT.into_iter()
            .chain(FENCED.map(|(old, _)| old))
            .map(String::from)
            .collect::<BTreeSet<_>>()
    );
    let new: BTreeSet<String> = MOVED
        .iter()
        .map(|(_, new)| new.to_string())
        .filter(|new| new.starts_with("symphony/"))
        .collect();
    assert_eq!(s.files_under("symphony/"), new);
    assert!(!s.common.join("sf-runs").exists());

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
    s.put("worktrees/wt/.sf-sweep-lock-7");
    s.put("worktrees/wt/.symphony-sweep-lock-7");
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
            "both worktrees/wt/.sf-sweep-lock-7 and worktrees/wt/.symphony-sweep-lock-7 exist: \
             keep one, move the other aside, and run this again",
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
fn an_old_copy_the_new_path_already_replaces_is_removed_and_a_different_one_refused() {
    let s = State::new("move-replaced");
    s.write("sf-surreal/3.3.0/surreal", "the pinned binary\n");
    s.write("symphony/surreal/3.3.0/surreal", "the pinned binary\n");
    for (old, new) in [
        ("sf-sweep-remove.lock", "symphony-sweep-remove.lock"),
        ("sf-sweep-verdict.json", "symphony-sweep-verdict.json"),
    ] {
        s.put(&format!("worktrees/wt/{old}"));
        s.put(&format!("worktrees/wt/{new}"));
    }
    s.put("sf-runs/2026-11-11.jsonl");
    let before = s.tree();
    let removed = [
        "sf-surreal: symphony/surreal holds the same pinned SurrealDB",
        "worktrees/wt/sf-sweep-remove.lock: the sweep's \
         worktrees/wt/symphony-sweep-remove.lock replaces it",
        "worktrees/wt/sf-sweep-verdict.json: the sweep's \
         worktrees/wt/symphony-sweep-verdict.json replaces it",
    ];
    let dry = s.run(&["--dry-run"]);
    assert_eq!((dry.code, dry.err.as_str()), (0, ""), "{}", dry.out);
    assert_eq!(
        dry.out,
        format!(
            "in {} (a dry run: nothing changes)\nwould move sf-runs to symphony/runs\n{}would \
             fence sf-surreal\n",
            s.common.display(),
            removed
                .map(|line| format!("would remove {line}\n"))
                .concat()
        )
    );
    assert_eq!(s.tree(), before);

    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    // Each removal is said in turn, the pinned SurrealDB's fence right after it.
    let [surreal, worktree @ ..] = removed.map(|line| format!("removed {line}\n"));
    assert_eq!(
        done.out,
        s.said(
            &["sf-runs to symphony/runs"],
            &(surreal + &fenced("sf-surreal") + &worktree.concat())
        )
    );
    // The old copy went, and its fence stands in its place.
    assert_eq!(
        fs::read_to_string(s.common.join("sf-surreal")).unwrap(),
        fence("symphony/surreal")
    );
    assert_eq!(
        s.files_under("sf-"),
        BTreeSet::from(["sf-surreal".to_string()])
    );
    let again = s.run(&[]);
    assert_eq!(
        (again.code, again.err.as_str()),
        (0, ""),
        "a fence is passed over: {}",
        again.out
    );
    assert!(
        again.out.starts_with("nothing to move in "),
        "{}",
        again.out
    );
    assert_eq!(
        fs::read_to_string(s.common.join("symphony/surreal/3.3.0/surreal")).unwrap(),
        "the pinned binary\n"
    );
    assert_eq!(
        fs::read_to_string(s.common.join("worktrees/wt/symphony-sweep-verdict.json")).unwrap(),
        "worktrees/wt/symphony-sweep-verdict.json\n"
    );
    assert!(!s.common.join("worktrees/wt/sf-sweep-remove.lock").exists());

    // A pinned binary that differs, or an old folder holding more, is a state to settle by hand.
    let s = State::new("move-replaced-differs");
    s.write("sf-surreal/3.3.0/surreal", "one binary\n");
    s.write("symphony/surreal/3.3.0/surreal", "another\n");
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(
                "both sf-surreal and symphony/surreal exist: keep the one that holds the state, \
                 move the other aside, and run this again"
            )
        )
    );
    assert_eq!(s.tree(), before);
    s.write("sf-surreal/3.3.0/surreal", "another\n");
    s.write("sf-surreal/3.2.0/surreal", "an older pin\n");
    assert_eq!(s.run(&[]).code, 1);
    assert_eq!(s.files_under("sf-surreal/").len(), 2);
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
    wait_named(pid, &named);
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
fn a_land_at_work_refuses_and_the_record_of_a_group_gone_moves_along() {
    let s = State::new("move-land");
    s.fill();
    // The lock two lands take in turn, held as a land holds it, naming the land.
    s.write("sf-tasks/land.lock", "the land of tools/x (pid 41)");
    let lock = fs::File::open(s.common.join("sf-tasks/land.lock")).unwrap();
    lock.try_lock().unwrap();
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(
                "the land of tools/x (pid 41) holds sf-tasks/land.lock: let it end and run this \
                 again"
            )
        )
    );
    assert_eq!(s.tree(), before);
    drop(lock);

    // A land killed outright left its record, and the process group it names still runs.
    let group = Started(
        Command::new("sleep")
            .arg("600")
            .process_group(0)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let id = group.0.id();
    s.write("sf-tasks/land/running", &format!("{id}\tjust verify\n"));
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "the process group {id} a land started (just verify) still runs: end it (kill \
                 -TERM -{id}) and run this again"
            ))
        )
    );
    assert_eq!(s.tree(), before);

    // With the group gone the record says nothing running: it moves with the board's folder,
    // where the next land forgets it, and so does the free lock.
    drop(group);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    s.moved();
    assert_eq!(
        fs::read_to_string(s.common.join("symphony/tasks/land/running")).unwrap(),
        format!("{id}\tjust verify\n")
    );
    assert!(s.common.join("symphony/tasks/land.lock").is_file());
}

#[test]
fn a_program_of_the_board_or_the_server_in_or_naming_the_repository_refuses() {
    let s = State::new("move-programs");
    s.fill();
    // A board command working in the linked worktree: lsof names its folder resolved, as the
    // scratch folder is.
    let (tasks, line) = program("tasks", &s.wt);
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {} ({line}), a program of the board or the server, works in {}, a folder of \
                 this repository: stop it and run this again",
                tasks.0.id(),
                s.wt.display()
            ))
        )
    );
    assert_eq!(s.tree(), before);
    drop(tasks);

    // A server run by its path in the main checkout, from a folder outside the repository, named
    // as a command line names it: on macOS without the /private its links resolve to.
    let main = s.main.to_str().unwrap();
    let main = main.strip_prefix("/private").unwrap_or(main);
    let (server, line) = program(&format!("{main}/.target/bin/symphony"), &s.machine);
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {} ({line}), a program of the board or the server, names {main}, a folder of \
                 this repository: stop it and run this again",
                server.0.id()
            ))
        )
    );
    assert_eq!(s.tree(), before);
    drop(server);

    // The same program outside the repository, or another program inside it, is no reason.
    let _outside = program("tasks", &s.machine);
    let _other = program("sleep", &s.wt);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    s.moved();
}

#[test]
fn a_lock_no_fetch_holds_is_no_state_and_a_fetch_under_way_refuses() {
    let s = State::new("move-surreal-lock");
    s.write("sf-surreal/3.3.0/surreal", "the pinned binary\n");
    s.write("symphony/surreal/3.3.0/surreal", "the pinned binary\n");
    // The empty lock an older fetch script took, and the lock tools/surreal.sh takes now, its
    // holder alive.
    s.write("sf-surreal/lock", "");
    let fetch = sleeper();
    let pid = fetch.0.id();
    s.write("sf-surreal/lock.d/pid", &format!("{pid}\n"));
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            refusal(&format!(
                "pid {pid} fetches SurrealDB into sf-surreal (its lock.d names it): let it end and \
                 run this again"
            ))
        )
    );
    assert_eq!(s.tree(), before);

    // Its holder gone, that lock is the next fetch's to take over: the old copy goes, its locks
    // with it, and its fence stands in its place.
    drop(fetch);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(
        done.out,
        s.said(
            &[],
            &("removed sf-surreal: symphony/surreal holds the same pinned SurrealDB\n".to_string()
                + &fenced("sf-surreal"))
        )
    );
    assert_eq!(
        fs::read_to_string(s.common.join("sf-surreal")).unwrap(),
        fence("symphony/surreal")
    );
    assert_eq!(
        s.files_under("symphony/"),
        BTreeSet::from(["symphony/surreal/3.3.0/surreal".to_string()])
    );

    // A lock file holding anything is no lock an older fetch left: a state to settle by hand.
    let s = State::new("move-surreal-lock-held");
    s.write("sf-surreal/3.3.0/surreal", "the pinned binary\n");
    s.write("symphony/surreal/3.3.0/surreal", "the pinned binary\n");
    s.write("sf-surreal/lock", "41\n");
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.err),
        (
            1,
            refusal(
                "both sf-surreal and symphony/surreal exist: keep the one that holds the state, \
                 move the other aside, and run this again"
            )
        )
    );
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

    // The common dir itself names its repository, as a new build refusing to start before the move
    // names it (`bash tools/move-state.sh COMMON`).
    s.put("sf-runs/2026-11-11.jsonl");
    let dry = s.run_in(&s.machine, &["--dry-run", s.common.to_str().unwrap()]);
    assert_eq!(
        (dry.code, dry.out, dry.err.as_str()),
        (
            0,
            format!(
                "in {} (a dry run: nothing changes)\nwould move sf-runs to symphony/runs\n",
                s.common.display()
            ),
            ""
        )
    );
}

#[test]
fn a_run_stopped_after_a_move_fenced_what_it_moved_before_and_the_next_fences_the_rest() {
    let s = State::new("move-stopped");
    s.fill();
    // Stopped at its second move, right after sf-surreal moved and before its fence.
    let stopped = s.run_stopped(2);
    assert_eq!(stopped.code, 1, "{}{}", stopped.out, stopped.err);
    assert_eq!(
        stopped.out,
        format!(
            "in {}\nmoved sf-tasks to symphony/tasks\n{}",
            s.common.display(),
            fenced("sf-tasks")
        )
    );
    // The folder that moved first is fenced already; the second moved, unfenced; nothing else.
    assert_eq!(
        fs::read_to_string(s.common.join("sf-tasks")).unwrap(),
        fence("symphony/tasks")
    );
    assert!(!s.common.join("sf-surreal").exists());
    assert!(s.common.join("symphony/surreal/3.3.0/surreal").is_file());
    assert!(s.common.join("sf-symphony").is_dir());

    // A board a build of the new code started meanwhile, or a land of its, is read where the
    // board's folder went: the next run refuses while either works.
    let server = sleeper();
    let pid = server.0.id();
    s.write(
        "symphony/tasks/server.json",
        &format!("{{\"addr\":\"127.0.0.1:4321\",\"pid\":{pid},\"version\":\"3.3.0\"}}"),
    );
    s.write("symphony/tasks/land.lock", "the land of tools/y (pid 42)");
    let lock = fs::File::open(s.common.join("symphony/tasks/land.lock")).unwrap();
    lock.try_lock().unwrap();
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            [
                format!(
                    "pid {pid} (sleep 600), the board's server symphony/tasks/server.json \
                     records, runs: stop it and run this again"
                ),
                "the land of tools/y (pid 42) holds symphony/tasks/land.lock: let it end and run \
                 this again"
                    .to_string(),
            ]
            .map(|why| refusal(&why))
            .concat()
        )
    );
    assert_eq!(s.tree(), before);
    drop((server, lock));
    for file in ["symphony/tasks/server.json", "symphony/tasks/land.lock"] {
        fs::remove_file(s.common.join(file)).unwrap();
    }

    // The next run says the fence it owes, writes it first and finishes the move.
    let before = s.tree();
    let dry = s.run(&["--dry-run"]);
    assert_eq!((dry.code, dry.err.as_str()), (0, ""), "{}", dry.out);
    assert!(
        dry.out.contains(
            "would fence sf-symphony\nwould fence sf-task-models\nwould fence sf-surreal, which a \
             stopped run left unfenced\n"
        ),
        "{}",
        dry.out
    );
    assert_eq!(s.tree(), before);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    let said = s.said(
        &[
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
        &s.older_install(),
    );
    let head = format!("in {}\n", s.common.display());
    assert_eq!(
        done.out,
        format!(
            "{head}{}{}",
            fenced("sf-surreal"),
            said.strip_prefix(&head).unwrap()
        )
    );
    s.moved();
    for (old, new) in FENCED {
        assert_eq!(
            fs::read_to_string(s.common.join(old)).unwrap(),
            fence(new),
            "{old}"
        );
    }
    let again = s.run(&[]);
    assert!(
        again.code == 0 && again.out.starts_with("nothing to move in "),
        "{}{}",
        again.out,
        again.err
    );

    // Stopped right after its first move: what an older build would make an empty board in
    // again is fenced by the next run, which finds nothing else to move.
    let s = State::new("move-stopped-first");
    s.put("sf-tasks/data/000001.sst");
    let stopped = s.run_stopped(1);
    assert_eq!(stopped.code, 1, "{}{}", stopped.out, stopped.err);
    assert!(!s.common.join("sf-tasks").exists());
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(done.out, s.said(&[], &fenced("sf-tasks")));
    assert_eq!(
        fs::read_to_string(s.common.join("sf-tasks")).unwrap(),
        fence("symphony/tasks")
    );
    assert_eq!(
        fs::read_to_string(s.common.join("symphony/tasks/data/000001.sst")).unwrap(),
        "sf-tasks/data/000001.sst\n"
    );
}

/// `path` under the common dir made a plain executable file, as an install leaves its program.
fn installed(s: &State, path: &str) {
    s.put(path);
    fs::set_permissions(s.common.join(path), fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn client_build_tools_a_whole_new_install_replaces_go_and_a_partial_one_is_refused() {
    let s = State::new("move-tools");
    s.put("sf-cargo/dx-0.7.10/bin/dx");
    s.put("sf-cargo/dx-home/tools/esbuild/esbuild");
    installed(&s, "symphony/tools/dx-0.7.10/bin/dx");
    installed(
        &s,
        "symphony/tools/dx-home/tools/wasm-bindgen-0.2.129/wasm-bindgen",
    );
    let removed = [
        "sf-cargo/dx-0.7.10: symphony/tools/dx-0.7.10 holds the client's build tools whole, which a \
         build of the new code installed: the old folder is a cache",
        "sf-cargo/dx-home: symphony/tools/dx-home holds the client's build tools whole, which a \
         build of the new code installed: the old folder is a cache",
    ];
    let before = s.tree();
    let dry = s.run(&["--dry-run"]);
    assert_eq!((dry.code, dry.err.as_str()), (0, ""), "{}", dry.out);
    assert_eq!(
        dry.out,
        format!(
            "in {} (a dry run: nothing changes)\n{}would remove sf-cargo, left empty\n",
            s.common.display(),
            removed
                .map(|line| format!("would remove {line}\n"))
                .concat()
        )
    );
    assert_eq!(s.tree(), before);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(
        done.out,
        s.said(
            &[],
            &(removed.map(|line| format!("removed {line}\n")).concat()
                + "removed sf-cargo, left empty\n")
        )
    );
    assert!(!s.common.join("sf-cargo").exists());
    assert_eq!(
        s.files_under("symphony/"),
        BTreeSet::from([
            "symphony/tools/dx-0.7.10/bin/dx".to_string(),
            "symphony/tools/dx-home/tools/wasm-bindgen-0.2.129/wasm-bindgen".to_string(),
        ])
    );

    // A new folder without its program (an install stopped part way, or one not executable) may
    // need the old one: both stay, refused, and the project's own beside them too.
    let s = State::new("move-tools-partial");
    s.put("sf-cargo/dx-0.7.10/bin/dx");
    s.put("sf-cargo/dx-home/tools/wasm-bindgen-0.2.129/wasm-bindgen");
    s.put("sf-cargo/target/debug/keep");
    s.put("symphony/tools/dx-0.7.10/bin/dx");
    s.put("symphony/tools/dx-home/tools/esbuild/esbuild");
    let before = s.tree();
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err),
        (
            1,
            "",
            [
                "both sf-cargo/dx-0.7.10 and symphony/tools/dx-0.7.10 exist: keep the one that \
                 holds the state, move the other aside, and run this again",
                "both sf-cargo/dx-home and symphony/tools/dx-home exist: keep the one that holds \
                 the state, move the other aside, and run this again",
            ]
            .map(refusal)
            .concat()
        )
    );
    assert_eq!(s.tree(), before);
}

#[test]
fn a_process_with_a_file_open_under_the_old_client_tools_or_working_there_refuses_naming_it() {
    let s = State::new("move-tools-open");
    s.put("sf-cargo/dx-0.7.10/bin/dx");
    s.put("sf-cargo/dx-home/tools/esbuild/esbuild");
    // A build working in dx's folder, and one reading a tool from dx's home: neither command line
    // names the folder, as a build's compilers and tools need not.
    let (working, line) = program("sleep", &s.common.join("sf-cargo/dx-0.7.10"));
    let tool = fs::File::open(s.common.join("sf-cargo/dx-home/tools/esbuild/esbuild")).unwrap();
    let reading = Started(
        Command::new("sleep")
            .arg("600")
            .stdin(tool)
            .spawn()
            .unwrap(),
    );
    wait_named(reading.0.id(), "sleep 600");
    let refused = [
        format!(
            "pid {} ({line}) works in sf-cargo/dx-0.7.10, an older build's copy of the client's \
             build tools: let it end and run this again",
            working.0.id()
        ),
        format!(
            "pid {} (sleep 600) has sf-cargo/dx-home/tools/esbuild/esbuild open, an older \
             build's copy of the client's build tools: let it end and run this again",
            reading.0.id()
        ),
    ]
    .map(|why| refusal(&why))
    .concat();
    let moves = [
        "sf-cargo/dx-0.7.10 to symphony/tools/dx-0.7.10",
        "sf-cargo/dx-home to symphony/tools/dx-home",
    ];
    let before = s.tree();
    let dry = s.run(&["--dry-run"]);
    assert_eq!(
        (dry.code, dry.out, dry.err.as_str()),
        (
            1,
            format!(
                "in {} (a dry run: nothing changes)\n{}would remove sf-cargo, left empty\n",
                s.common.display(),
                moves.map(|line| format!("would move {line}\n")).concat()
            ),
            refused.as_str()
        )
    );
    let done = s.run(&[]);
    assert_eq!(
        (done.code, done.out.as_str(), done.err.as_str()),
        (1, "", refused.as_str())
    );
    assert_eq!(s.tree(), before);

    drop(working);
    drop(reading);
    let done = s.run(&[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""), "{}", done.out);
    assert_eq!(done.out, s.said(&moves, "removed sf-cargo, left empty\n"));
    assert_eq!(
        s.files_under("symphony/"),
        BTreeSet::from([
            "symphony/tools/dx-0.7.10/bin/dx".to_string(),
            "symphony/tools/dx-home/tools/esbuild/esbuild".to_string(),
        ])
    );
}
