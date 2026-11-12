//! `tasks database`: the board's database said, held stopped with every board command refused
//! naming the hold, and released, on the board of a fixture repository of its own outside this one
//! (no `TASKS_SERVER`), its server started by its first command and stopped when the case ends.

use crate::common::{NOW, surreal};
use crate::{repository, run, scratch};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::Home;
use symphony_board::sys::pid_alive;

/// A fixture project, `alpha`, its machine folder its own: its board's hold released and its server
/// stopped when the case ends, however it ends.
struct Alpha {
    root: PathBuf,
    machine: PathBuf,
}

impl Alpha {
    fn new(name: &str) -> Alpha {
        let dir = scratch(name);
        let root = repository(&dir.join("alpha"));
        std::fs::write(root.join("symphony.toml"), "[project]\nid = \"alpha\"\n").unwrap();
        let machine = dir.join("machine");
        std::fs::create_dir_all(&machine).unwrap();
        Alpha { root, machine }
    }

    fn env(&self) -> Env {
        let vars: BTreeMap<String, String> = [
            ("SF_SURREAL", surreal().display().to_string()),
            ("SYMPHONY_MACHINE_DIR", self.machine.display().to_string()),
            ("TASKS_NOW", NOW.to_string()),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        Env {
            vars,
            cwd: self.root.clone(),
        }
    }

    fn home(&self) -> Home {
        Home::in_common(&self.root.join(".git"), Some(&surreal()))
    }

    /// `tasks ARGS` here: its exit code, stdout and stderr.
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let ran = run(&self.env(), args, "");
        (ran.code, ran.out, ran.err)
    }
}

impl Drop for Alpha {
    fn drop(&mut self) {
        let home = self.home();
        let _ = home.release();
        let _ = home.stop();
    }
}

fn data(root: &Path) -> String {
    root.join(".git/sf-tasks/data").display().to_string()
}

/// A board whose server is not running is said so, and held and released without one ever
/// starting.
#[test]
fn a_board_with_no_server_is_held_and_released_and_none_starts() {
    let alpha = Alpha::new("database-idle");
    let home = alpha.home();
    let data = data(&alpha.root);
    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. No server runs on its data: the \
                 board's next command starts one\n"
            )
        )
    );
    let (code, out, err) = alpha.run(&["database", "hold", "a clone", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.ends_with(&format!(
            ": a clone. No server ran on its data. Nothing serves {data} now, and nothing starts \
             a server on it until `just tasks database release --as russet-lynx`\n"
        )),
        "{out}"
    );
    let (code, _, err) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(home.recorded(), None, "nothing started the board");
    assert_eq!(home.serving(), None);
}

#[test]
fn a_held_board_refuses_every_command_naming_the_hold_until_its_release() {
    let alpha = Alpha::new("database-hold");
    let home = alpha.home();
    let (code, handle, err) = alpha.run(&["register", "--kind", "worker"]);
    assert_eq!(code, 0, "{err}");
    let running = home
        .recorded()
        .expect("the first command started the board");
    let data = data(&alpha.root);

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(
        (code, out),
        (
            0,
            format!(
                "the board's database at {data} is not held. Its server is pid {} at {}\n",
                running.pid, running.addr
            )
        )
    );

    let (code, _, err) = alpha.run(&["database", "hold", "a snapshot"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("✖ no session: pass --as HANDLE"), "{err}");

    let (code, out, err) = alpha.run(&["database", "hold", "a snapshot", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ the board's database is held by russet-lynx since "),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(
            ": a snapshot. Stopped pid {} at {} cleanly. Nothing serves {data} now, and nothing \
             starts a server on it until `just tasks database release --as russet-lynx`\n",
            running.pid, running.addr
        )),
        "{out}"
    );
    assert!(!pid_alive(running.pid));
    let hold = home.held().unwrap();

    // Every command is refused, naming the hold, and starts nothing: one that writes, one that
    // reads, and the session's own.
    let refused = format!(
        "✖ the board's database is {hold}; nothing starts it until the hold is released (`just \
         tasks database release --as HANDLE`)\n"
    );
    for args in [
        &["list"][..],
        &["show", "sim/jump"],
        &["create", "sim/jump", "Jump", "--as", handle.trim()],
    ] {
        let (code, out, err) = alpha.run(args);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", refused.as_str()),
            "{args:?}"
        );
    }
    assert_eq!(home.recorded(), None, "a refused command started the board");
    assert_eq!(home.serving(), None);

    let (code, out, _) = alpha.run(&["database"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!(
            "the board's database at {data} is {hold}; nothing starts it until `just tasks \
             database release --as HANDLE`. No server runs on its data\n"
        )
    );
    let (code, _, err) = alpha.run(&["database", "hold", "a swap", "--as", "vivid-swift"]);
    assert_eq!((code, err), (1, refused.clone()));

    let (code, out, err) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ released the hold russet-lynx set at "),
        "{out}"
    );
    assert!(
        out.ends_with(
            " (a snapshot). Nothing was started: the board's next command starts the database\n"
        ),
        "{out}"
    );
    assert_eq!(home.recorded(), None, "the release started the board");
    let (code, out, _) = alpha.run(&["database", "release", "--as", "russet-lynx"]);
    assert_eq!(
        (code, out.as_str()),
        (
            0,
            "✔ the board's database was not held: nothing to release\n"
        )
    );
    let (code, _, err) = alpha.run(&["list"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        home.recorded().is_some(),
        "the next command started the board"
    );
    let (code, out, _) = alpha.run(&["help", "database"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("usage: just tasks database [-h] [hold WHY | release] [--as ACTOR]\n"),
        "{out}"
    );
}
