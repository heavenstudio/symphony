//! A repository whose state an older build kept in its old folders, before `tools/move-state.sh`
//! moved it ([`symphony_board::unmoved`]): this build's commands start no board and make nothing
//! of `symphony/` in its common dir, each refusing with the line that names the move, and once the
//! move left its one-line files they run as on any repository.

use crate::common::{NOW, surreal};
use crate::{Ran, git, repository, scratch};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use symphony_board::server::Home;

const TASKS: &str = env!("CARGO_BIN_EXE_tasks");

/// A project's repository as an older build left it: its main checkout, a linked worktree, and
/// its board, server, models and runs in the old folders, each holding a file.
struct Unmoved {
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
    common: PathBuf,
}

impl Unmoved {
    fn new(name: &str) -> Unmoved {
        let base = scratch(name);
        let main = repository(&base.join("main"));
        fs::write(main.join("symphony.toml"), "[project]\nid = \"unmoved\"\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-qm", "project"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let common = main.join(".git");
        for (_, old) in symphony_board::unmoved::OLD {
            fs::create_dir_all(common.join(old)).unwrap();
            fs::write(common.join(old).join("kept"), format!("{old}\n")).unwrap();
        }
        fs::create_dir_all(base.join("home")).unwrap();
        fs::create_dir_all(base.join("machine")).unwrap();
        Unmoved {
            wt: base.join("wt"),
            base,
            main,
            common,
        }
    }

    /// `tasks ARGS` in `cwd`, `input` on stdin, as a session runs it: no board named, no Cargo
    /// around it, the machine folder its own.
    fn tasks(&self, cwd: &Path, args: &[&str], input: &str) -> Ran {
        let file = self.base.join("input");
        fs::write(&file, input).unwrap();
        let done = Command::new(TASKS)
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:/usr/sbin:/sbin",
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", self.base.join("home"))
            .env("SYMPHONY_SURREAL", surreal())
            .env("SYMPHONY_MACHINE_DIR", self.base.join("machine"))
            .env("TASKS_NOW", NOW.to_string())
            .env("TASKS_SWEEP", "off")
            .env("TASKS_DISPATCH", "inline")
            .args(args)
            .current_dir(cwd)
            .stdin(fs::File::open(&file).unwrap())
            .output()
            .unwrap();
        Ran {
            code: done.status.code().unwrap_or(-1),
            out: String::from_utf8(done.stdout).unwrap(),
            err: String::from_utf8(done.stderr).unwrap(),
        }
    }

    /// The line a build refuses with here, for the folder `new`.
    fn refusal(&self, new: &str) -> String {
        symphony_board::unmoved::refusal(&self.common, new).expect("unmoved")
    }

    /// That nothing of `symphony/` was made in the common dir.
    fn untouched(&self, after: &str) {
        assert!(
            !self.common.join("symphony").exists(),
            "{after} made {}",
            self.common.join("symphony").display()
        );
    }

    fn home(&self) -> Home {
        Home::in_common(&self.common, Some(&surreal()))
    }
}

impl Drop for Unmoved {
    fn drop(&mut self) {
        let _ = self.home().stop();
    }
}

#[test]
fn a_board_command_from_either_checkout_refuses_naming_the_move_and_starts_no_board() {
    let u = Unmoved::new("unmoved-commands");
    let refusal = u.refusal(symphony_board::server::FOLDER);
    let (_, board) = symphony_board::unmoved::OLD[0];
    assert!(
        refusal.contains("tools/move-state.sh") && refusal.contains(&format!("/{board},")),
        "{refusal}"
    );
    for cwd in [&u.main, &u.wt] {
        for args in [&["list"][..], &["pull", "--as", "someone"], &["sweep"]] {
            let ran = u.tasks(cwd, args, "");
            assert_eq!(
                (ran.code, ran.err.as_str()),
                (2, format!("✖ {refusal}\n").as_str()),
                "{args:?} in {}: {}",
                cwd.display(),
                ran.out
            );
            u.untouched(&format!("{args:?}"));
        }
    }
}

#[test]
fn the_database_and_snapshot_commands_which_open_no_board_refuse_naming_the_move() {
    let u = Unmoved::new("unmoved-database");
    let refusal = u.refusal(symphony_board::server::FOLDER);
    for cwd in [&u.main, &u.wt] {
        for args in [
            &["database"][..],
            &["database", "hold", "a swap", "--as", "someone"],
            &["database", "release", "--as", "someone"],
            &["snapshot"],
            &["snapshot", "--list"],
        ] {
            let ran = u.tasks(cwd, args, "");
            assert_eq!(
                (ran.code, ran.out.as_str(), ran.err.as_str()),
                (2, "", format!("✖ {refusal}\n").as_str()),
                "{args:?} in {}",
                cwd.display()
            );
            u.untouched(&format!("{args:?}"));
        }
    }
    let (_, old) = symphony_board::unmoved::OLD[0];
    assert_eq!(
        fs::read_to_string(u.common.join(old).join("kept")).unwrap(),
        format!("{old}\n"),
        "the older build's board as it was"
    );
}

#[test]
fn the_heartbeat_and_the_recorder_make_nothing_and_the_recorded_command_still_runs() {
    let u = Unmoved::new("unmoved-quiet");
    let event = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": "just tasks pull --as someone"}, "session_id": "s-1",
        "cwd": u.wt});
    let ran = u.tasks(&u.wt, &["beat"], &event.to_string());
    assert_eq!((ran.code, ran.out.as_str()), (0, ""), "{}", ran.err);
    u.untouched("a beat");

    let ran = u.tasks(
        &u.wt,
        &[
            "run", "--phase", "test", "--label", "t", "--", "sh", "-c", "echo ran",
        ],
        "",
    );
    assert_eq!((ran.code, ran.out.as_str()), (0, "ran\n"), "{}", ran.err);
    u.untouched("a recorded run");
}

#[test]
fn the_board_library_refuses_its_start_and_stop_naming_the_move() {
    let u = Unmoved::new("unmoved-library");
    let refusal = u.refusal(symphony_board::server::FOLDER);
    assert_eq!(u.home().ensure().unwrap_err().to_string(), refusal);
    assert_eq!(u.home().stop().unwrap_err().to_string(), refusal);
    // The hold, its release and a snapshot each take the starters' lock first, as a start does.
    assert_eq!(
        u.home().hold("someone", "a swap").unwrap_err().to_string(),
        refusal
    );
    assert_eq!(
        u.home().release("someone", None).unwrap_err().to_string(),
        refusal
    );
    assert_eq!(u.home().snapshot(NOW).unwrap_err(), refusal);
    u.untouched("the library");
    // A board folder named by hand (a reclaim's rehearsal on a copy) is no repository's.
    let elsewhere = u.base.join("copy");
    assert!(Home::new(&elsewhere, &surreal()).stop().unwrap().is_none());
}

#[test]
fn once_the_move_left_its_fences_the_board_starts_in_its_new_folder() {
    let u = Unmoved::new("unmoved-moved");
    for (new, old) in symphony_board::unmoved::OLD {
        let from = u.common.join(old);
        let to = u.common.join(new);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::rename(&from, &to).unwrap();
        fs::write(
            &from,
            format!(
                "moved by tools/move-state.sh to {new}: run only builds of the new code on this \
                 repository\n"
            ),
        )
        .unwrap();
    }
    // Not refused for the move (what the board then says of a board not made yet is its own).
    let ran = u.tasks(&u.main, &["list"], "");
    assert!(!ran.err.contains("tools/move-state.sh"), "{}", ran.err);
    assert!(u.home().recorded().is_some(), "the board's server runs");
    let (new, old) = symphony_board::unmoved::OLD[0];
    assert_eq!(new, symphony_board::server::FOLDER);
    assert_eq!(
        fs::read_to_string(u.common.join(new).join("kept")).unwrap(),
        format!("{old}\n")
    );
}
