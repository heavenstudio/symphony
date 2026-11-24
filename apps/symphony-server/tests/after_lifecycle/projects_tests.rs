//! Two projects on the one server, started in either order: the fixture's main checkout, a plain
//! project with a theme of its own in its file, from the plain build; and a project naming the
//! shelf plugin and its theme package, from the build holding it (`symphony-shelf`). Each has its
//! own board, its server in the project's own common dir as every project's is, holding a task of
//! its own: whichever build serves them reads each project's tasks from its own board.

use super::fixture::{APP, BACKSTOP, Process};
use super::*;
use std::ffi::OsString;
use std::path::PathBuf;
use symphony_board::NewTask;
use symphony_board::server::Home as Board;

/// The build holding the shelf fixture plugin.
const SHELF: &str = env!("CARGO_BIN_EXE_symphony-shelf");
/// The client each install folder holds: a head for the theme, a body for the project.
const SHELL: &str = "<!doctype html><html><head><title>Symphony</title>\
                     <link rel=\"icon\" href=\"data:,\"></head><body>projects client</body></html>\n";
const ALPHA: &str = "[project]\nname = \"Alpha\"\nid = \"alpha\"\n\
                     [theme.tokens]\nprimary = \"#7a2e8f\"\n[theme.layouts]\nhome = \"focus\"\n";
const SHELF_PROJECT: &str = "[project]\nname = \"Shelf\"\nid = \"pantry\"\n\
                             [plugins]\ncrates = [\"fixture-shelf\"]\n\
                             [theme]\npackage = \"fixture-shelf\"\n";
/// The task each project's own board holds.
const ALPHA_TASK: &str = "alpha/own";
const PANTRY_TASK: &str = "pantry/own";

/// The fixture's main checkout as the project `alpha`, and a second repository as `pantry`, each
/// install folder holding [`SHELL`]: the shelf project's root.
fn projects(repo: &Repo) -> PathBuf {
    write(&repo.main.join("symphony.toml"), ALPHA);
    write(&repo.install.join("client/public/index.html"), SHELL);
    let shelf = repo.external("shelf project");
    write(&shelf.join("symphony.toml"), SHELF_PROJECT);
    write(
        &shelf.join(".git/symphony/server/client/public/index.html"),
        SHELL,
    );
    shelf
}

/// Each project's own board, made by its first writer here, as a project's first `just tasks`
/// makes it, and holding one task of its own. The one server reads each but never ends one, so it
/// ends first, then each board, before the case's scratch goes.
struct Boards<'a> {
    repo: &'a Repo,
    boards: Vec<Board>,
}

impl<'a> Boards<'a> {
    /// The boards of `alpha`, the fixture's main checkout, and of `pantry` at `shelf`.
    fn made(repo: &'a Repo, shelf: &Path) -> Self {
        let boards = [(repo.main.as_path(), ALPHA_TASK), (shelf, PANTRY_TASK)]
            .into_iter()
            .map(|(root, task)| {
                let env = symphony_board::cli::Env {
                    vars: [(
                        symphony::machine::DIR_ENV.to_string(),
                        repo.home.display().to_string(),
                    )]
                    .into(),
                    cwd: root.to_path_buf(),
                };
                symphony_board::cli::connect(&env)
                    .unwrap_or_else(|why| panic!("{}'s board: {why}", root.display()))
                    .create(&NewTask::new(task, "Its own project's task", "owner"))
                    .unwrap();
                Board::in_common(&root.join(".git"))
            })
            .collect();
        Boards { repo, boards }
    }
}

impl Drop for Boards<'_> {
    fn drop(&mut self) {
        self.repo.cleanup();
        for board in &self.boards {
            if let Err(why) = board.stop_board() {
                eprintln!("{}: {why}", board.dir().display());
            }
        }
    }
}

/// The fixture's `PATH` and `/usr/sbin`, where macOS keeps `lsof`, which the board's sweep finds
/// the folders processes run in with.
fn path(repo: &Repo) -> OsString {
    let (_, mut path) = repo
        .environment()
        .into_iter()
        .find(|(name, _)| *name == "PATH")
        .expect("the fixture's PATH");
    path.push(":/usr/sbin:/sbin");
    path
}

/// A start of the one server from `dir` by `program`, in the fixture's environment.
fn start_with(repo: &Repo, program: &str, dir: &Path) -> Process {
    start_with_args(repo, program, dir, &[])
}

/// [`start_with`], with `args`.
fn start_with_args(repo: &Repo, program: &str, dir: &Path, args: &[&str]) -> Process {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(repo.environment())
        .env("PATH", path(repo))
        .stdin(Stdio::null());
    repo.run(command)
}

/// The slugs of the tasks the server on `port` lists for project `id`, once it has read the
/// project's board: until its first read it answers that it has none yet.
fn board_tasks(port: u16, id: &str) -> Vec<String> {
    let route = format!("/{id}/api/tasks");
    let mut last = None;
    let read = test_time::until(
        format!("{route} to list its board's tasks"),
        BACKSTOP,
        || {
            let answer = get(port, &route);
            let read = match &answer {
                Some((200, body)) => serde_json::from_str::<symphony_api::Tasks>(body).ok(),
                _ => None,
            };
            last = answer;
            read
        },
    );
    read.unwrap_or_else(|stall| panic!("{stall}; its last answer: {last:?}"))
        .tasks
        .into_iter()
        .map(|task| task.slug)
        .collect()
}

/// The status, headers and body of a browser's navigation to `path`.
fn navigate(port: u16, path: &str, cookie: Option<&str>) -> (u16, String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(BACKSTOP)).unwrap();
    let cookie = cookie.map_or_else(String::new, |cookie| format!("Cookie: {cookie}\r\n"));
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nSec-Fetch-Mode: navigate\r\n\
         Accept: text/html\r\n{cookie}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (head, body) = reply.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_ascii_lowercase(), body.to_string())
}

/// Both projects served by the one server holding the shelf, each its own page and theme, and a
/// bookmark of today's paths sent to the starter's project.
fn both_served(repo: &Repo, starter: &str) {
    let port = repo.port();
    let front = status(port);
    assert!(
        front.plugins.iter().any(|name| name == "fixture-shelf"),
        "{:?}",
        front.plugins
    );
    let mut projects = front.projects.clone();
    projects.sort();
    assert_eq!(projects, ["alpha", "pantry"]);
    let registry = fs::read_to_string(repo.home.join("projects.json")).unwrap();
    assert!(
        registry.contains("\"alpha\"") && registry.contains("\"pantry\""),
        "{registry}"
    );
    // Each project's tasks are its own board's, and neither lists the other's.
    for (id, task) in [("alpha", ALPHA_TASK), ("pantry", PANTRY_TASK)] {
        assert_eq!(board_tasks(port, id), [task], "{id}'s board");
    }
    let (code, alpha) = answer(port, "/alpha/");
    assert_eq!(code, 200, "{alpha}");
    assert!(alpha.contains("<title>Alpha</title>"), "{alpha}");
    assert!(alpha.contains(":root{--primary:#7a2e8f;}"), "{alpha}");
    assert!(alpha.contains("data-layout-home=\"focus\""), "{alpha}");
    let (code, shelf) = answer(port, "/pantry/");
    assert_eq!(code, 200, "{shelf}");
    assert!(shelf.contains("<title>The Shelf</title>"), "{shelf}");
    assert!(shelf.contains(":root{--primary:#2a6f4e;}"), "{shelf}");
    assert!(shelf.contains("data-layout-header=\"compact\""), "{shelf}");
    for (id, token) in [("alpha", "#7a2e8f"), ("pantry", "#2a6f4e")] {
        let (code, css) = answer(port, &format!("/{id}/theme.css"));
        assert_eq!((code, css), (200, format!(":root{{--primary:{token};}}\n")));
    }
    // The shelf's own page answers under its project alone.
    assert_eq!(answer(port, "/pantry/x/shelf/none.item").0, 404);
    let (status, head, _) = navigate(port, "/docs/A.md", None);
    assert_eq!(status, 307, "{head}");
    assert!(
        head.contains(&format!("location: /{starter}/docs/a.md")),
        "{head}"
    );
    for cookie in [
        None,
        Some("symphony-project=alpha"),
        Some("symphony-project=pantry"),
        Some("symphony-project=gone"),
    ] {
        let (status, head, list) = navigate(port, "/", cookie);
        assert_eq!(status, 200, "{cookie:?}: {head}\n{list}");
        assert!(
            !head.lines().any(|line| line.starts_with("location:")),
            "{cookie:?}: {head}"
        );
        assert_eq!(
            head.lines()
                .find_map(|line| line.strip_prefix("cache-control: ")),
            Some("no-store"),
            "{cookie:?}: {head}"
        );
        assert!(list.contains("<h1>Projects</h1>"), "{cookie:?}: {list}");
        for id in ["alpha", "pantry"] {
            assert!(
                list.contains(&format!("href=\"/{id}/\"")),
                "{cookie:?}, {id}: {list}"
            );
        }
    }
}

#[test]
fn a_plain_host_waits_for_the_plugins_project_whose_build_then_takes_over_by_hand_off() {
    let repo = Repo::new("projects plain first");
    let shelf = projects(&repo);
    let _boards = Boards::made(&repo, &shelf);
    start_with(&repo, APP, &repo.main).success();
    repo.remember();
    let plain = repo.check_server();
    assert!(plain.plugins.is_empty(), "{:?}", plain.plugins);
    assert_eq!(board_tasks(plain.port, "alpha"), [ALPHA_TASK]);
    // The plain build cannot serve the shelf: it registers the project and says whose build can.
    let mut waits = start_with(&repo, APP, &shelf);
    let said = waits.wait();
    assert!(!said.success(), "{said}");
    let output = waits.output();
    assert!(
        output.contains("lacks the plugin fixture-shelf"),
        "{output}"
    );
    assert!(output.contains(&shelf.display().to_string()), "{output}");
    assert_eq!(answer(repo.port(), "/pantry/").0, 503);
    // The shelf's build serves every registered project, so it replaces the plain host.
    let mut takes = start_with(&repo, SHELF, &shelf);
    takes.success();
    let output = takes.output();
    assert!(output.contains("replaces it"), "{output}");
    assert!(output.contains("/pantry/"), "{output}");
    let host = status(repo.port());
    assert_ne!(host.pid, plain.pid);
    assert_eq!(host.root, shelf.to_str().unwrap());
    repo.adopt();
    until("the replaced plain host to end", || {
        (!symphony_server::sys::pid_alive(plain.pid)).then_some(())
    });
    both_served(&repo, "pantry");
}

#[test]
fn a_branchs_build_holding_the_plugin_never_replaces_the_plain_host() {
    let repo = Repo::new("projects branch build");
    let shelf = projects(&repo);
    // A branch of the shelf project: a linked worktree of its repository.
    repository(&shelf, &["config", "core.hooksPath", "/dev/null"]);
    repository(&shelf, &["add", "."]);
    repository(&shelf, &["commit", "-qm", "The shelf"]);
    let branch = repo.base.join("shelf branch");
    repository(
        &shelf,
        &[
            "worktree",
            "add",
            "-qb",
            "feature/shelf",
            branch.to_str().unwrap(),
        ],
    );
    start_with(&repo, APP, &repo.main).success();
    repo.remember();
    let plain = repo.check_server();
    let registry = || fs::read_to_string(repo.home.join("projects.json")).unwrap();
    let before = registry();
    // Started there, the build holding the shelf only reads the host, which does not serve it.
    let mut reads = start_with(&repo, SHELF, &branch);
    assert_eq!(reads.wait().code(), Some(2), "{}", reads.output());
    let said = reads.output();
    assert!(
        said.contains(&format!(
            "✖ Symphony: the running Symphony does not serve {}, and a start in a linked worktree \
             ({}) never starts the machine's Symphony",
            shelf.display(),
            branch.display()
        )),
        "{said}"
    );
    assert_eq!(registry(), before, "the shelf project was registered");
    // With the owner's `--shared` it registers the shelf project, which waits for the start in the
    // shelf's main checkout: the host is not replaced.
    let mut waits = start_with_args(&repo, SHELF, &branch, &["--shared"]);
    assert_eq!(waits.wait().code(), Some(2), "{}", waits.output());
    let said = waits.output();
    assert!(
        said.contains(&format!(
            "✖ Symphony: the running Symphony lacks the plugin fixture-shelf, which pantry names: \
             this start runs in the linked worktree {}, and a start in a linked worktree never \
             replaces the running Symphony: run `just symphony` in {}\n",
            branch.display(),
            shelf.display()
        )),
        "{said}"
    );
    assert!(registry().contains("\"pantry\""), "{}", registry());
    assert!(
        !repo.home.join("handover.json").exists(),
        "a hand-off was asked"
    );
    let host = status(repo.port());
    assert_eq!(host.pid, plain.pid);
    assert!(host.plugins.is_empty(), "{:?}", host.plugins);
}

#[test]
fn a_host_holding_the_plugin_serves_a_plain_project_started_after_it() {
    let repo = Repo::new("projects shelf first");
    let shelf = projects(&repo);
    let _boards = Boards::made(&repo, &shelf);
    let mut first = start_with(&repo, SHELF, &shelf);
    first.success();
    repo.adopt();
    let host = status(repo.port());
    assert_eq!(board_tasks(host.port, "pantry"), [PANTRY_TASK]);
    // The plain build serves no plugin the host lacks: it registers and is served as it is.
    let mut second = start_with(&repo, APP, &repo.main);
    second.success();
    assert!(second.output().contains("/alpha/"), "{}", second.output());
    assert_eq!(status(repo.port()).pid, host.pid);
    both_served(&repo, "pantry");
}
