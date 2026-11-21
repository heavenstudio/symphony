//! The machine's server as the installed release's, in the case's own machine folder: a start
//! that finds none running starts the release `current` names, which serves every project whose
//! plugins answer from a process and is what the record names; its reload follows `current` to
//! the next release by the socket hand-off, and a release that does not start leaves it serving,
//! saying why; a project naming a linked plugin holds the machine on a project's build, which its
//! pages and the release's status say.

use super::fixture::{BACKSTOP, Process};
use super::*;
use std::io::{BufRead, BufReader};
use std::time::Instant;

/// The process plugin both projects name.
const PLUGIN: &str = "fixture-process";

/// A release installed in `repo`'s machine folder as an install leaves it, and `current` pointed
/// at it: `server`'s text as its server when given (a stand-in), else this build of the server; a
/// stand-in for each other binary; and a client of a page and one script, each saying `mark`. Its
/// server, by its own folder's path.
pub(super) fn installed(repo: &Repo, name: &str, mark: &str, server: Option<&str>) -> PathBuf {
    use symphony::machine::release::{self, BINARIES, Built, Serves, Version};
    let build = repo.base.join(format!("{name}-build"));
    fs::create_dir_all(build.join("bin")).unwrap();
    for binary in BINARIES {
        let to = build.join("bin").join(binary);
        match (binary, server) {
            ("symphony-server", None) => {
                fs::copy(fixture::APP, &to).unwrap();
            }
            ("symphony-server", Some(text)) => write(&to, text),
            _ => write(&to, "#!/bin/sh\n"),
        }
        fs::set_permissions(&to, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    write(
        &build.join("client/index.html"),
        &format!(
            "<html><head><script type=\"module\" src=\"/assets/app-{mark}.js\"></script></head>\
             <body>{mark}</body></html>"
        ),
    );
    write(
        &build.join(format!("client/assets/app-{mark}.js")),
        &format!("console.log('{mark}')"),
    );
    let version = Version { major: 0, minor: 0 };
    let built = Built {
        release: name,
        commit: "0000000",
        serves: Serves {
            schema: symphony_board::SCHEMA,
            additions: symphony_board::schema::ADDITIONS,
            protocol: version,
            tasks_api: version,
            pin: symphony::machine::PIN,
        },
        binaries: &build.join("bin"),
        client: &build.join("client"),
    };
    release::install(&repo.home, &built).unwrap();
    release::point(&repo.home, name).unwrap();
    release::folder(&repo.home, name).join("bin/symphony-server")
}

/// A plugin that answers from its own process: a script `/bin/sh` reads, which declares
/// [`PLUGIN`] to the host's first request and then reads its stdin until the host ends.
fn plugin(repo: &Repo) -> PathBuf {
    struct Declared;
    impl symphony_plugin::Plugin for Declared {
        fn name(&self) -> &str {
            PLUGIN
        }
    }
    let declared = symphony_plugin::protocol::Declaration::of(&Declared);
    let answer =
        serde_json::to_string(&symphony_plugin::protocol::Answered::Answer(&declared)).unwrap();
    assert!(!answer.contains('\''), "{answer}");
    let script = repo.base.join("plugin.sh");
    write(
        &script,
        &format!(
            "IFS= read -r line\n\
             id=$(printf '%s' \"$line\" | sed -n 's/.*\"id\":\\([0-9][0-9]*\\).*/\\1/p')\n\
             printf '{{\"id\":%s,\"answered\":%s}}\\n' \"$id\" '{answer}'\n\
             exec cat >/dev/null\n"
        ),
    );
    script
}

/// The project file of `id`, naming [`PLUGIN`] and the command that starts its process.
fn naming_the_process(id: &str, script: &Path) -> String {
    format!(
        "[project]\nid = \"{id}\"\n[plugins]\ncrates = [\"{PLUGIN}\"]\n\n[plugins.process]\n\
         {PLUGIN} = [\"/bin/sh\", \"{}\"]\n",
        script.display()
    )
}

/// A start of the one server from `dir` by this build, in the fixture's environment: what it said.
fn started(repo: &Repo, dir: &Path) -> String {
    let mut start: Process = repo.run(repo.command(dir));
    start.success();
    start.output()
}

/// What the server on `port` tells the pages under `base` (`/<id>`), once it says `wanted`: the
/// feed's line that says it. Only a stall reaches [`BACKSTOP`].
pub(super) fn told(port: u16, base: &str, wanted: &str) -> String {
    let (_, board) = answer(port, &format!("{base}/api/board"));
    let version = serde_json::from_str::<serde_json::Value>(&board).unwrap()["version"].clone();
    let mut feed = TcpStream::connect(("127.0.0.1", port)).unwrap();
    feed.set_read_timeout(Some(BACKSTOP)).unwrap();
    write!(
        feed,
        "GET {base}/api/feed?since={version} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .unwrap();
    let began = Instant::now();
    let mut feed = BufReader::new(feed);
    let mut heard = Vec::new();
    loop {
        let mut line = String::new();
        let read = feed.read_line(&mut line).unwrap_or(0);
        if line.contains(wanted) {
            return line;
        }
        assert!(
            read > 0 && began.elapsed() < BACKSTOP,
            "the feed of {base} did not say {wanted:?}: {heard:?}"
        );
        heard.push(line);
    }
}

/// What the server log says, once it says `wanted`.
fn logged(repo: &Repo, wanted: &str) -> String {
    until(&format!("the server's log to say {wanted:?}"), || {
        let log = fs::read_to_string(repo.home.join("server.log")).unwrap_or_default();
        log.contains(wanted).then_some(log)
    })
}

/// The server's record as the machine folder keeps it, whole.
fn record(repo: &Repo) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(repo.home.join("server.json")).unwrap()).unwrap()
}

#[test]
fn a_start_runs_the_installed_release_which_serves_two_projects_whose_plugins_are_processes() {
    let repo = Repo::new("release serves");
    let server = installed(&repo, "release-1", "first", None);
    let script = plugin(&repo);
    write(
        &repo.main.join("symphony.toml"),
        &naming_the_process("alpha", &script),
    );
    let pantry = repo.external("pantry");
    write(
        &pantry.join("symphony.toml"),
        &naming_the_process("pantry", &script),
    );
    // No server runs: the start runs the release's server in place of its own build.
    let said = started(&repo, &repo.main);
    assert!(
        said.contains(
            "the machine's Symphony starts as the release release-1, which serves every \
             registered project"
        ),
        "{said}"
    );
    repo.remember();
    let port = repo.port();
    let front = repo.check_server();
    assert_eq!(front.release.as_deref(), Some("release-1"));
    assert!(front.plugins.is_empty(), "{:?}", front.plugins);
    // The record names the release, and the process is its own folder's server.
    let recorded = record(&repo);
    assert_eq!(recorded["release"], "release-1", "{recorded}");
    let command = symphony_board::sys::output(Command::new("ps").args([
        "-ww",
        "-o",
        "command=",
        "-p",
        &front.pid.to_string(),
    ]))
    .unwrap();
    let command = String::from_utf8_lossy(&command.stdout).into_owned();
    assert!(
        command.starts_with(&format!("{} serve ", server.display())),
        "{command}"
    );
    // A second project whose plugin answers from a process is served by it, registered beside.
    let said = started(&repo, &pantry);
    assert!(said.contains("/pantry/"), "{said}");
    assert_eq!(status(port).pid, front.pid, "no start replaces a release");
    until("both projects to be served", || {
        let mut projects = status(port).projects;
        projects.sort();
        (projects == ["alpha", "pantry"]).then_some(())
    });
    for (id, root) in [("alpha", &repo.main), ("pantry", &pantry)] {
        let (code, page) = answer(port, &format!("/{id}/"));
        assert_eq!(code, 200, "{id}: {page}");
        assert!(page.ends_with("first</body></html>"), "{id}: {page}");
        // Each project's plugin was started from its own checkout, and declared.
        let log = root.join(format!("artifacts/plugins/{PLUGIN}.log"));
        until(&format!("{id}'s plugin to start"), || {
            let said = fs::read_to_string(&log).unwrap_or_default();
            said.contains("started ").then_some(())
        });
    }
}

#[test]
fn the_release_server_follows_current_and_one_that_does_not_start_leaves_it_serving() {
    let repo = Repo::new("release follows");
    installed(&repo, "release-1", "first", None);
    write(
        &repo.main.join("symphony.toml"),
        "[project]\nid = \"alpha\"\n",
    );
    started(&repo, &repo.main);
    repo.remember();
    let port = repo.port();
    let first = repo.check_server();
    assert_eq!(first.release.as_deref(), Some("release-1"));
    // A follow that has nothing to hand over to says so in the log, once.
    logged(
        &repo,
        "names release-1, the release this server is: it follows the link",
    );
    // A release whose server ends before it is ready: the running one keeps serving, and says why.
    installed(&repo, "release-2", "second", Some("#!/bin/sh\nexit 3\n"));
    let log = logged(&repo, "The release release-2 did not take over");
    assert!(
        log.contains(
            "· Symphony reload: Symphony server update failed; the previous server is still \
             serving. The release release-2 did not take over: replacement did not become ready: \
             it ended. It is tried again at about "
        ),
        "{log}"
    );
    told(
        port,
        "/alpha",
        "The release release-2 did not take over: replacement did not become ready: it ended.",
    );
    let still = status(port);
    assert_eq!(
        (still.pid, still.release.as_deref()),
        (first.pid, Some("release-1"))
    );
    assert_eq!(record(&repo)["pid"], first.pid);
    // The next release is handed the sockets and answers from them, its record its own.
    installed(&repo, "release-3", "third", None);
    let next = until("the release release-3 to answer", || {
        front(port).filter(|front| front.release.as_deref() == Some("release-3") && front.ready)
    });
    assert_ne!(next.pid, first.pid);
    // The replaced server records its successor once that one said it is ready, which it answers
    // from before: the record is waited for, then the successor owned for the case's cleanup.
    let recorded = until("the record to name the release release-3", || {
        Some(record(&repo)).filter(|recorded| recorded["release"] == "release-3")
    });
    assert_eq!(recorded["pid"], next.pid, "{recorded}");
    repo.adopt();
    let (_, page) = answer(port, "/alpha/");
    assert!(page.ends_with("third</body></html>"), "{page}");
    until("the replaced release's server to end", || {
        (!symphony_server::sys::pid_alive(first.pid)).then_some(())
    });
}

#[test]
fn a_start_by_the_release_while_a_project_holds_the_switch_starts_nothing_and_names_the_holder() {
    let repo = Repo::new("release refused");
    installed(&repo, "release-1", "first", None);
    write(
        &repo.main.join("symphony.toml"),
        "[project]\nid = \"alpha\"\n",
    );
    let pantry = repo.external("pantry");
    write(
        &pantry.join("symphony.toml"),
        "[project]\nid = \"pantry\"\n[plugins]\ncrates = [\"fixture-shelf\"]\n",
    );
    symphony::machine::Registry::change(&repo.home, |registry| {
        registry.add(symphony::machine::Entry::of(&pantry)?)
    })
    .unwrap();
    // What `just symphony` runs in a project with no build of its own: the release, through
    // `current`, with no server running.
    let mut start = repo.run({
        let mut command = Command::new(repo.home.join("current/bin/symphony-server"));
        command
            .current_dir(&repo.main)
            .env_clear()
            .envs(repo.environment())
            .stdin(Stdio::null());
        command
    });
    assert_eq!(start.wait().code(), Some(2));
    let registered = symphony::machine::Registry::read(&repo.home).unwrap();
    let held = (registered.projects.iter())
        .find(|entry| entry.id == "pantry")
        .unwrap();
    let said = start.output();
    assert!(
        said.contains(&format!(
            "✖ Symphony: the machine's Symphony is not started from the release release-1, as the \
             switch to it is held: pantry names the linked plugin fixture-shelf, and a release's \
             server serves a plugin only from its process: a [plugins.process] command for it in \
             {} lets the release serve every project (docs/PLUGIN_PROTOCOL.md); `just symphony` \
             in {}, by pantry's own build, starts the machine's server\n",
            held.root.join("symphony.toml").display(),
            held.root.display()
        )),
        "{said}"
    );
    // Nothing started: no record, and no server answers for the machine.
    assert!(repo.record().is_none());
    assert!(!repo.home.join("server.log").exists());
    // The starting project is registered beside the holder, for the holder's build to serve.
    let mut ids: Vec<&str> = (registered.projects.iter())
        .map(|entry| entry.id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["alpha", "pantry"]);
}

#[test]
fn a_project_naming_a_linked_plugin_holds_the_switch_on_its_pages_and_in_the_status() {
    const SHELF: &str = env!("CARGO_BIN_EXE_symphony-shelf");
    let repo = Repo::new("release held");
    installed(&repo, "release-1", "first", None);
    let pantry = repo.external("pantry");
    write(
        &pantry.join("symphony.toml"),
        "[project]\nid = \"pantry\"\n[plugins]\ncrates = [\"fixture-shelf\"]\n",
    );
    write(
        &pantry.join(".git/symphony/server/client/public/index.html"),
        "<html><body>shelf client</body></html>",
    );
    // The release cannot serve the linked shelf: the shelf's own build is the machine's server.
    let mut start = repo.run({
        let mut command = Command::new(SHELF);
        command
            .current_dir(&pantry)
            .env_clear()
            .envs(repo.environment())
            .stdin(Stdio::null());
        command
    });
    start.success();
    repo.adopt();
    let port = repo.port();
    let host = status(port);
    assert_eq!(host.release, None);
    assert_eq!(host.plugins, ["fixture-shelf"]);
    told(
        port,
        "/pantry",
        "The machine's Symphony stays a project's build, not the release release-1: pantry names \
         the linked plugin fixture-shelf, and a release's server serves a plugin only from its \
         process: a [plugins.process] command for it in its symphony.toml lets the release serve \
         every project (docs/PLUGIN_PROTOCOL.md).",
    );
    let standing = symphony::machine::switch::standing(&repo.home);
    let registered = symphony::machine::Registry::read(&repo.home).unwrap();
    let root = &registered.projects[0].root;
    assert_eq!(
        standing,
        format!(
            "machine: current names release-1; the machine's server (port {port}, pid {}) is the \
             build of {}\nmachine: the switch to release-1 is held: pantry names the linked \
             plugin fixture-shelf, and a release's server serves a plugin only from its process: \
             a [plugins.process] command for it in {} lets the release serve every project \
             (docs/PLUGIN_PROTOCOL.md)\n",
            host.pid,
            host.root,
            root.join("symphony.toml").display()
        )
    );
}
