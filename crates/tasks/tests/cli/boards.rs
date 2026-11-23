//! Two projects' boards side by side, each its own SurrealDB server in its repository's common dir
//! in a temporary folder, named SurrealDB's and started by its project's first command (no
//! `TASKS_SERVER`): each command
//! line reaches its own project's board, and its sessions, heartbeat keys and sweep state stay with
//! that project; an older command line is refused a board a newer build migrated before any write,
//! naming the project, while the other project's board goes on as it was.

use crate::common::{NOW, surreal};
use crate::{Ran, repository, run, scratch};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use symphony_board::cli::{Env, connect};
use symphony_board::launch::reap::state_path;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Program, Schema, Store};

/// Two fixture projects, `alpha` and `beta`, each a repository of its own outside this one: their
/// boards' servers are stopped when the case ends, however it ends.
struct Two {
    alpha: PathBuf,
    beta: PathBuf,
}

impl Two {
    fn new(name: &str) -> Two {
        let dir = scratch(name);
        let project = |id: &str| {
            let root = repository(&dir.join(id));
            std::fs::write(
                root.join("symphony.toml"),
                format!("[project]\nid = \"{id}\"\n"),
            )
            .unwrap();
            // On SurrealDB, whose recorded servers and SurrealQL these cases read: named before
            // the first command, which would otherwise make the board on PostgreSQL.
            Two::home(&root)
                .initialize_backend(symphony_board::server::Engine::SurrealDb)
                .unwrap();
            root
        };
        Two {
            alpha: project("alpha"),
            beta: project("beta"),
        }
    }

    /// A run in `root`: no board named, so the project's own is reached, started if it is down.
    fn env(root: &Path) -> Env {
        let vars: BTreeMap<String, String> = [
            ("SYMPHONY_SURREAL", surreal().display().to_string()),
            ("TASKS_NOW", NOW.to_string()),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        Env {
            vars,
            cwd: root.to_path_buf(),
        }
    }

    fn home(root: &Path) -> Home {
        Home::in_common(&root.join(".git"), Some(&surreal()))
    }

    fn ok(root: &Path, args: &[&str]) -> String {
        let ran = run(&Two::env(root), args, "");
        assert_eq!(ran.code, 0, "{args:?} in {}: {}", root.display(), ran.err);
        ran.out.trim().to_string()
    }
}

impl Drop for Two {
    fn drop(&mut self) {
        for root in [&self.alpha, &self.beta] {
            let _ = Two::home(root).stop();
        }
    }
}

/// The handles of the sessions on the board of `root`'s project.
fn sessions(root: &Path) -> Vec<String> {
    connect(&Two::env(root))
        .unwrap()
        .board()
        .unwrap()
        .sessions
        .into_iter()
        .map(|session| session.row.handle)
        .collect()
}

#[test]
fn each_projects_command_line_reaches_its_own_board_with_its_own_sessions_keys_and_sweep_state() {
    let two = Two::new("boards-own");
    let a = Two::ok(&two.alpha, &["register", "--kind", "worker"]);
    let b = Two::ok(&two.beta, &["register", "--kind", "worker"]);
    let alpha_server = Two::home(&two.alpha).recorded().expect("alpha's server");
    let beta_server = Two::home(&two.beta).recorded().expect("beta's server");
    assert_ne!(alpha_server.addr, beta_server.addr);
    assert_eq!(sessions(&two.alpha), std::slice::from_ref(&a));
    assert_eq!(sessions(&two.beta), std::slice::from_ref(&b));

    Two::ok(
        &two.alpha,
        &["--as", &a, "create", "alpha/one", "Alpha's own"],
    );
    let beta_board = connect(&Two::env(&two.beta)).unwrap();
    assert!(beta_board.get("alpha/one").is_err());
    let docs = two.alpha.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    assert_eq!(
        connect(&Two::env(&docs))
            .unwrap()
            .get("alpha/one")
            .unwrap()
            .title,
        "Alpha's own"
    );

    // A beat from a folder of alpha's binds its key in alpha's common dir alone.
    let event = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": format!("TASKS_SESSION={a} just tasks inbox")},
        "session_id": "s-alpha", "cwd": docs});
    let ran: Ran = run(&Two::env(&two.beta), &["beat"], &event.to_string());
    assert_eq!((ran.code, ran.err.as_str()), (0, ""));
    assert!(two.alpha.join(".git/symphony/tasks/keys/s-alpha").is_file());
    assert!(!two.beta.join(".git/symphony/tasks/keys/s-alpha").exists());

    for root in [&two.alpha, &two.beta] {
        assert_eq!(
            state_path(&Two::env(root), &root.join(".")).unwrap(),
            root.join(".git/symphony/tasks/reap.json")
        );
    }
}

#[test]
fn an_older_command_line_is_refused_a_board_a_newer_build_migrated_before_any_write() {
    let two = Two::new("boards-skew");
    let a = Two::ok(&two.alpha, &["register", "--kind", "worker"]);
    let b = Two::ok(&two.beta, &["register", "--kind", "worker"]);
    let addr = Two::home(&two.alpha).recorded().unwrap().addr;
    let newer = Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;");
    // As the newer build's `tasks migrate --leave-behind` in alpha's main checkout moves alpha's
    // live board, which its server names whoever opens it: where the main checkout holds no board
    // source, nothing else does. This build's `tasks`, which wrote it, is left behind.
    let migrated = Store::opening(
        Client::new(&addr, "sf", "tasks"),
        Box::new(|| NOW),
        Box::new(|_| false),
        "this-host",
        &Opening::write()
            .at(newer.clone())
            .migrating()
            .leave_behind(),
    )
    .unwrap();

    let ran = run(
        &Two::env(&two.alpha),
        &["--as", &a, "create", "alpha/late", "After the migration"],
        "",
    );
    let this = Schema::this_build().version();
    // A board this build cannot open is a run that cannot start, as an unreachable one is.
    assert_eq!(ran.code, 2, "{}", ran.err);
    let program = Program::this();
    assert_eq!(
        ran.err,
        format!(
            "✖ the alpha board is at schema {}, and this build writes only a board at its own \
             schema, {this}: nothing was written; rebuild {} ({}) at the board's schema, and \
             `just symphony` in alpha brings its Symphony to it\n",
            newer.version(),
            program.name,
            program.path
        )
    );
    assert!(migrated.get("alpha/late").is_err());
    // The newer build writes there, and beta's board is still this build's to write.
    migrated
        .create(&NewTask::new("alpha/newer", "By the newer build", "owner"))
        .unwrap();
    Two::ok(&two.beta, &["--as", &b, "create", "beta/one", "Beta's own"]);
    assert_eq!(sessions(&two.beta), [b]);
}
