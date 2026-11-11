//! A preview server of its own (`just symphony --port 0`) in a linked worktree whose build's board
//! schema is newer than the main checkout's: it reads the main checkout's board, the one its record
//! in the common dir names, and never moves it forward. Its pages say why they read nothing, naming
//! what does, and the main checkout's build reads and writes the board as it left it.

use super::*;
use std::path::PathBuf;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Schema, Store};

/// The pinned SurrealDB: `SF_SURREAL`, else what `tools/surreal.sh` fetches.
fn surreal() -> PathBuf {
    std::env::var_os("SF_SURREAL").map_or_else(
        || {
            let found = symphony_board::sys::output(
                Command::new("bash")
                    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/surreal.sh"))
                    .env_remove("BASH_ENV"),
            )
            .unwrap();
            assert!(found.status.success(), "SurrealDB lookup: {found:?}");
            PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
        },
        PathBuf::from,
    )
}

/// The main checkout's board: its server in the common dir, stopped when the test ends.
struct Board {
    home: Home,
}

impl Board {
    /// The board of the checkout at `main`, made by a build one schema version behind this one
    /// (the main checkout's, while this build's schema has not landed), holding one task.
    fn older(main: &Path) -> Board {
        let board = Board {
            home: Home::in_common(&main.join(".git"), Some(&surreal())),
        };
        board.home.ensure().unwrap();
        board
            .older_build()
            .create(&NewTask::new("main/one", "Made by main's build", "owner"))
            .unwrap();
        board
    }

    fn client(&self) -> Client {
        Client::new(&self.home.recorded().unwrap().addr, "sf", "tasks")
    }

    /// The board as the main checkout's build opens it to write.
    fn older_build(&self) -> Store {
        Store::opening(
            self.client(),
            Box::new(|| 1_000_000.0),
            Box::new(|_| false),
            "this-host",
            &Opening::write().at(Schema::this_build().before()),
        )
        .unwrap()
    }

    /// The schema version the board records.
    fn version(&self) -> Option<i64> {
        self.client()
            .query(
                "RETURN (SELECT VALUE version FROM ONLY meta:schema);",
                serde_json::json!({}),
            )
            .unwrap()
            .last()
            .and_then(serde_json::Value::as_i64)
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let _ = self.home.stop();
    }
}

#[test]
fn a_preview_in_a_worktree_of_a_newer_build_never_moves_the_main_checkouts_board_forward() {
    let repo = Repo::new("board-schema");
    let board = Board::older(&repo.main);
    let (before, this) = (
        Schema::this_build().before().version(),
        Schema::this_build().version(),
    );
    let mut command = repo.command(&repo.tree);
    // The preview reaches the board its repository's common dir records, as a worktree's does,
    // and checks with `lsof` (in /usr/sbin on macOS) that the recorded server listens there.
    let (_, path) = repo
        .environment()
        .into_iter()
        .find(|(name, _)| *name == "PATH")
        .unwrap();
    let mut path = path.into_string().unwrap();
    path.push_str(":/usr/sbin:/sbin");
    command
        .env_remove("TASKS_DATABASE")
        .env("SF_SURREAL", surreal())
        .env("PATH", path)
        .args(["--port", "0", "--host", "127.0.0.1"]);
    let mut server = repo.run(command);
    let actual = server.ready();
    let behind = format!(
        "board is at schema {before}, behind this build's {this}, and a reader never moves a \
         board forward: nothing was read or written; `just tasks list` in "
    );
    // The board's page, read from the hub (which says first that the board is starting), and the
    // agents' page, which opens the board itself: each unavailable, saying why. A server of its
    // own serves its one checkout at `/`.
    assert_eq!(actual.projects, Vec::<String>::new());
    for page in ["/api/tasks", "/api/agents"] {
        let deadline = Instant::now() + fixture::PATIENCE;
        let (status, body) = loop {
            let answer = get(actual.port, page);
            match answer {
                Some((status, body)) if body.contains(&behind) => break (status, body),
                answer => assert!(
                    Instant::now() < deadline,
                    "{page} never said why it reads no board: {answer:?}"
                ),
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(status, 503, "{page}: {body}");
        assert_eq!(board.version(), Some(before), "{page}");
    }
    server.signal(libc::SIGTERM);
    server.success();
    // The main checkout's build reads and writes its board still, at its own schema.
    assert_eq!(board.version(), Some(before));
    let main = board.older_build();
    main.say("main/one", "owner", "after the preview", None)
        .unwrap();
    assert_eq!(main.thread("main/one").unwrap().len(), 2);
}
