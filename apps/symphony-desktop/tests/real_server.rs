//! The follower against a real server: the machine's one server (`serve --home`) on a private
//! port, its machine folder and its one project in a temporary folder, its board a server of the
//! test's own in that folder, which ends with it. A card added to a document raises the count and
//! gives one notice; a question an agent asks the owner does the same; a reviewer's verdict gives
//! one notice and leaves the count as it was.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "the fixtures beside the test assert as it does: an unwrap or a panic is the failure"
)]

#[expect(
    clippy::new_without_default,
    reason = "a tree is made fresh with new(), never by default"
)]
#[path = "../../../crates/symphony/tests/common/tree.rs"]
pub mod tree;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use symphony_api::ProjectList;
use symphony_board::rpc::Client;
use symphony_board::server::{Home, Postgres};
use symphony_board::{NewSession, NewTask, Store};
use symphony_desktop::Server;
use symphony_desktop::follow::{Follow, Kind, Notice};
use symphony_desktop::read::{self, Answer};
use tree::Tree;

/// The board's database the server reads.
const DATABASE: &str = "desktop_follow";

/// The register the project starts with: one card waiting on the owner.
const REGISTER: &str = "# Decisions\n\n- [ ] **first** — The first question.\n";
/// The register once a second card is added to it.
const TWO_CARDS: &str =
    "# Decisions\n\n- [ ] **first** — The first question.\n- [ ] **second** — The second one.\n";
/// The wall-clock backstop of each wait on the server: a quiet machine has each within this
/// allowance, and only a stall reaches six times it (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));
/// How long one read of the list waits at each step.
const READ_WAIT: Duration = Duration::from_secs(5);

/// A board of the test's own: its PostgreSQL, the machine's own programs as the workspace finds
/// them, kept in a scratch folder by the board's own keeper ([`Home`]), which ends it with this
/// process however that ends, and the store on it.
struct Board {
    postgres: Postgres,
    address: String,
    store: Store,
}

impl Board {
    fn start(tree: &Tree) -> Board {
        let folder = tree.path("board");
        std::fs::create_dir_all(&folder).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let programs = symphony_board::server::postgres::discover_programs(&workspace)
            .unwrap_or_else(|why| panic!("no PostgreSQL for the board: {why}"));
        let postgres = Home::at(&folder).supervised(true).postgresql(&programs);
        let serving = postgres
            .ensure()
            .unwrap_or_else(|why| panic!("the board's server did not start: {why}"));
        let address = format!("postgresql:{}", serving.socket.display());
        let store = Store::with(
            Client::new(&address, "sf", DATABASE),
            Box::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0.0, |since| since.as_secs_f64())
            }),
            // No process of this machine is an agent of this board's.
            Box::new(|_| false),
            "desktop-fixture",
        )
        .unwrap();
        Board {
            postgres,
            address,
            store,
        }
    }

    /// A worker's session, by its handle.
    fn session(&self) -> String {
        self.store
            .register(&NewSession::new("worker"))
            .unwrap()
            .row
            .handle
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let _ = self.postgres.stop();
    }
}

/// The machine's one server, started on a scratch machine folder over the project `desk`.
struct OneServer {
    child: Child,
    base: String,
    log: PathBuf,
}

impl OneServer {
    /// The server over `root`, its machine folder in `tree`, its board `board`.
    fn start(tree: &Tree, root: &Path, board: &Board) -> OneServer {
        let binary = built("symphony-server");
        let machine = tree.path("machine");
        // The tailnet worker of a server with a home asks `tailscale`: here it answers nothing.
        let bin = tree.path("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink("/usr/bin/false", bin.join("tailscale")).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let added = cleared(Command::new(&binary).args(["project", "add"]).arg(root))
            .env("SYMPHONY_MACHINE_DIR", &machine)
            .env("PATH", &path)
            .output()
            .unwrap();
        assert!(added.status.success(), "project add: {added:?}");
        let log = tree.path("server.log");
        let mut child = cleared(Command::new(&binary).args(["serve", "--home"]))
            .arg(&machine)
            .arg("--root")
            .arg(root)
            .args(["--port", "0", "--host", "127.0.0.1"])
            .env("PATH", &path)
            .env("SYMPHONY_MACHINE_DIR", &machine)
            .env("TASKS_SERVER", &board.address)
            .env("TASKS_DATABASE", DATABASE)
            .env("TASKS_SWEEP", "off")
            .env("ASKS_DB", tree.path("asks.db"))
            .env("SYMPHONY_TEST_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, announced) = mpsc::channel();
        // A server with a home stamps each line of its output with the time.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = line
                    .split_once("→ Symphony at ")
                    .and_then(|(_, said)| said.split_whitespace().next())
                {
                    let _ = send.send(url.split('/').take(3).collect::<Vec<_>>().join("/"));
                }
            }
        });
        let mut server = OneServer {
            child,
            base: String::new(),
            log,
        };
        server.base = announced.recv_timeout(BACKSTOP).unwrap_or_else(|why| {
            panic!(
                "the server never said its address ({why}); {}; {}",
                test_time::Load::now(),
                server.said()
            )
        });
        server
    }

    fn said(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for OneServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `command` with none of the caller's git or board session in its environment.
fn cleared(command: &mut Command) -> &mut Command {
    for name in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "TASKS_SESSION",
        "SYMPHONY_UI_DIST",
    ] {
        command.env_remove(name);
    }
    command
}

/// The workspace's binary `name`: the one in the folder `SYMPHONY_TEST_BINARIES` names, when a
/// runner built them before any test started, else built through `tools/rs-bin.sh --build`.
fn built(name: &str) -> PathBuf {
    if let Some(folder) = std::env::var_os("SYMPHONY_TEST_BINARIES").filter(|f| !f.is_empty()) {
        let named = Path::new(&folder).join(name);
        assert!(named.is_file(), "{} holds no {name}", named.display());
        return named;
    }
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/rs-bin.sh");
    let done = Command::new(wrapper)
        .args(["--build", name])
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "tools/rs-bin.sh --build {name}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    PathBuf::from(String::from_utf8(done.stdout).unwrap().trim())
}

/// The project `desk` in `tree`: its file, its register and its first commit.
fn project(tree: &Tree) -> PathBuf {
    tree.write(
        "desk/symphony.toml",
        "[project]\nname = \"Desk\"\nid = \"desk\"\n",
    );
    tree.write("desk/docs/DECISIONS.md", REGISTER);
    // The server names its checkout canonically: macOS's /private/var, never /var.
    let root = tree.path("desk").canonicalize().unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Desktop fixture"],
        &["config", "user.email", "desktop@example.invalid"],
        &["config", "core.hooksPath", "/dev/null"],
        &["add", "."],
        &["-c", "commit.gpgsign=false", "commit", "-qm", "Desk"],
    ] {
        let done = cleared(Command::new("git").args(args).current_dir(&root))
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
    }
    root
}

/// The follower of the one server, and the notices it gave.
struct Following {
    server: Server,
    follow: Follow,
    told: Vec<Notice>,
}

impl Following {
    fn new(base: &str) -> Following {
        let server = Server::parse(base).unwrap();
        Following {
            follow: Follow::new([(server.clone(), server.default_name())]),
            server,
            told: Vec::new(),
        }
    }

    /// Read the server's list, telling each answer to the follower, until `holds` holds of one;
    /// `what` names it. The notices it gave meanwhile, and the list.
    fn until(&mut self, what: &str, holds: impl Fn(&ProjectList) -> bool) -> ProjectList {
        let mut last = None;
        let found = test_time::until(what, BACKSTOP, || {
            let answer = read::projects(&self.server, READ_WAIT);
            last = Some(format!("{answer:?}"));
            let listed = match &answer {
                Answer::Listed(list) => Some(list.clone()),
                _ => None,
            };
            let (notices, _) = self.follow.heard(0, answer, now());
            self.told.extend(notices);
            listed.filter(|list| holds(list))
        });
        found.unwrap_or_else(|stall| panic!("{stall}; the last answer: {last:?}"))
    }

    /// The notices given since the last call.
    fn told(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.told)
    }
}

/// Seconds since the epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// What the one project waits on the owner with: its cards and its questions.
fn waiting(list: &ProjectList) -> (Option<u32>, Option<u32>) {
    let project = &list.projects[0];
    (
        project.waiting,
        project
            .asks
            .as_ref()
            .and_then(|asks| asks.questions.get("owner").copied()),
    )
}

/// Each notice's kind and title.
fn titles(told: &[Notice]) -> Vec<(Kind, &str)> {
    told.iter()
        .map(|notice| (notice.kind, notice.title.as_str()))
        .collect()
}

/// The board's two tasks, both claimed by an executor: `desk/asked`, in progress, where it asks
/// the owner, and `desk/judged`, in review under a reviewer. The executor's handle and the
/// reviewer's.
fn staffed(board: &Board) -> (String, String) {
    let (executor, reviewer) = (board.session(), board.session());
    for slug in ["desk/asked", "desk/judged"] {
        board
            .store
            .create(&NewTask::new(slug, "Desk work", "owner"))
            .unwrap();
        board.store.claim(slug, &executor).unwrap();
    }
    board
        .store
        .move_from("desk/judged", &["in_progress"], "review", &executor)
        .unwrap();
    board
        .store
        .take_role("desk/judged", &reviewer, "reviewer")
        .unwrap();
    (executor, reviewer)
}

#[test]
fn a_card_and_a_question_raise_the_count_with_a_notice_each_and_a_verdict_gives_one_alone() {
    let tree = Tree::new();
    let root = project(&tree);
    // Dropped in the reverse order: the server before its board, both before their folder.
    let board = Board::start(&tree);
    let (executor, reviewer) = staffed(&board);
    let server = OneServer::start(&tree, &root, &board);
    let mut following = Following::new(&server.base);

    // First sight: what waits is counted, and nothing is told.
    let first = following.until(
        "the list counts the register's card and the board's asks",
        |list| waiting(list) == (Some(1), Some(0)),
    );
    assert_eq!(
        (first.projects.len(), first.projects[0].id.as_str()),
        (1, "desk"),
        "{first:?}; {}",
        server.said()
    );
    assert!(first.installation.is_some(), "{first:?}");
    assert_eq!(following.follow.total(), 1);
    assert_eq!(following.told(), []);

    // A card added to a document.
    tree.write("desk/docs/DECISIONS.md", TWO_CARDS);
    following.until("the list counts the second card", |list| {
        waiting(list).0 == Some(2)
    });
    assert_eq!(following.follow.total(), 2);
    let told = following.told();
    assert_eq!(
        titles(&told),
        [(Kind::Card, "2 cards wait on you")],
        "{told:?}"
    );
    assert_eq!(
        told[0].place, None,
        "one server and one project name neither"
    );
    assert!(
        told[0]
            .page
            .starts_with(&format!("{}/desk/docs/DECISIONS.md#", server.base)),
        "the notice opens the next card: {told:?}"
    );

    // A question an agent asks the owner on a task.
    board
        .store
        .say(
            "desk/asked",
            &executor,
            "Which of the two first?",
            Some("owner"),
        )
        .unwrap();
    following.until("the list counts the question", |list| {
        waiting(list).1 == Some(1)
    });
    assert_eq!(following.follow.total(), 3);
    let told = following.told();
    assert_eq!(
        titles(&told),
        [(Kind::Question, "1 question waits on you")],
        "{told:?}"
    );
    assert_eq!(told[0].page, format!("{}/desk/", server.base));

    // A reviewer's verdict: news, and nothing more waits.
    board
        .store
        .verdict("desk/judged", &reviewer, "approve", "it holds")
        .unwrap();
    let judged = following.until("the list names the verdict", |list| {
        list.projects[0]
            .asks
            .as_ref()
            .is_some_and(|asks| asks.verdicts.contains_key("owner"))
    });
    assert_eq!(waiting(&judged), (Some(2), Some(1)));
    assert_eq!(following.follow.total(), 3, "a verdict waits on no one");
    let told = following.told();
    let approved = format!("{reviewer} approved desk/judged");
    assert_eq!(
        titles(&told),
        [(Kind::Verdict, approved.as_str())],
        "{told:?}"
    );
    assert_eq!(
        told[0].page,
        format!("{}/desk/task/desk/judged", server.base)
    );

    // Read again with nothing new, the follower tells nothing more.
    following.until("one more list", |_| true);
    assert_eq!(following.told(), []);
}
