//! A preview server of its own (`just symphony --port 0`) in a linked worktree whose build's board
//! schema is newer than the main checkout's: it reads the main checkout's board, the one its record
//! in the common dir names, and never moves it forward. Its pages say why they read nothing, naming
//! what does, and the main checkout's build reads and writes the board as it left it.

use super::*;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Schema, Store};

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

    /// The board of the checkout at `main`, made by a build one schema version ahead of this one
    /// (main's, while the release this build stands for has not caught up), holding one task.
    fn newer(main: &Path) -> Board {
        let board = Board {
            home: Home::in_common(&main.join(".git"), Some(&surreal())),
        };
        board.home.ensure().unwrap();
        board
            .opened(
                Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;"),
            )
            .create(&NewTask::new("main/one", "Made by main's build", "owner"))
            .unwrap();
        board
    }

    /// The board as the main checkout's build opens it to write.
    fn older_build(&self) -> Store {
        self.opened(Schema::this_build().before())
    }

    /// The board as a build of `schema` opens it to write.
    fn opened(&self, schema: Schema) -> Store {
        Store::opening(
            self.client(),
            Box::new(|| 1_000_000.0),
            Box::new(|_| false),
            "this-host",
            &Opening::write().at(schema),
        )
        .unwrap()
    }

    /// The schema version the board records.
    fn version(&self) -> Option<i64> {
        symphony_board::Raw::on(&self.client())
            .row("meta", "schema")
            .field("version")
            .unwrap()
            .as_i64()
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
        .env("SYMPHONY_SURREAL", surreal())
        .env("PATH", path)
        .args(["--port", "0", "--host", "127.0.0.1"]);
    let mut server = repo.run(command);
    let actual = server.ready();
    let behind = format!(
        "board is at schema {before}, behind this build's {this}, and a reader never moves a \
         board forward: nothing was read or written; `just tasks migrate` in the main checkout \
         of "
    );
    // The board's page, read from the hub (which says first that the board is starting), and the
    // agents' page, which opens the board itself: each unavailable, saying why. A server of its
    // own serves its one checkout at `/`.
    assert_eq!(actual.projects, Vec::<String>::new());
    for page in ["/api/tasks", "/api/agents"] {
        let mut last = None;
        let said = test_time::until(
            format!("{page} to say why it reads no board"),
            fixture::BACKSTOP,
            || {
                let answer = get(actual.port, page);
                let said = answer.clone().filter(|(_, body)| body.contains(&behind));
                last = answer;
                said
            },
        );
        let (status, body) =
            said.unwrap_or_else(|stall| panic!("{stall}; its last answer: {last:?}"));
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

/// The server of an installed release, one board schema behind main's: it reads the board main's
/// build moved on, writes nothing to it, and every page says so.
#[test]
fn a_release_reads_a_board_one_schema_ahead_writes_nothing_and_its_pages_say_so() {
    let repo = Repo::new("board-ahead");
    let board = Board::newer(&repo.main);
    let ahead = Schema::this_build().version() + 1;
    let server = super::release_tests::installed(&repo, "release-1", "first", None);
    let main = board
        .opened(Schema::this_build().then("DEFINE TABLE IF NOT EXISTS fixture_next SCHEMALESS;"));
    let lines = main.thread("main/one").unwrap().len();
    let (_, path) = repo
        .environment()
        .into_iter()
        .find(|(name, _)| *name == "PATH")
        .unwrap();
    let mut path = path.into_string().unwrap();
    path.push_str(":/usr/sbin:/sbin");
    let mut command = Command::new(&server);
    command
        .current_dir(&repo.main)
        .env_clear()
        .envs(repo.environment())
        .env_remove("TASKS_DATABASE")
        .env("SYMPHONY_SURREAL", surreal())
        .env("PATH", path)
        .stdin(Stdio::null())
        .args(["--root", repo.main.to_str().unwrap()])
        .args(["--port", "0", "--host", "127.0.0.1"]);
    let mut served = repo.run(command);
    let actual = served.ready();
    assert_eq!(actual.release.as_deref(), Some("release-1"));
    let mut last = None;
    let read = test_time::until(
        "the release to list the board's tasks",
        fixture::BACKSTOP,
        || {
            let answer = get(actual.port, "/api/tasks");
            let read = match &answer {
                Some((200, body)) => serde_json::from_str::<symphony_api::Tasks>(body).ok(),
                _ => None,
            };
            last = answer;
            read
        },
    );
    let tasks = read.unwrap_or_else(|stall| panic!("{stall}; its last answer: {last:?}"));
    let slugs: Vec<String> = tasks.tasks.into_iter().map(|task| task.slug).collect();
    assert_eq!(slugs, ["main/one"]);
    super::release_tests::told(
        actual.port,
        "",
        &format!(
            "The task board is at schema {ahead}, ahead of this Symphony's {}: it is read, and \
             nothing is written to it from here, until a Symphony at schema {ahead} serves it.",
            ahead - 1
        ),
    );
    served.signal(libc::SIGTERM);
    served.success();
    // Nothing was written: the board is where main's build left it.
    assert_eq!(board.version(), Some(ahead));
    assert_eq!(main.thread("main/one").unwrap().len(), lines);
}
