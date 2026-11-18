//! A green release's refresh of the machine: the release installed by a stand-in for cargo in a
//! temporary machine folder, each registered project's board moved to the release's schema when
//! every writer seen there in the last week is a release's own or at that schema, its store
//! snapshotted first, `current` pointed once nothing holds the release, and the machine's server
//! waited for while it follows `current`. A writer behind the release holds it, the project's to
//! finish; a project naming a linked plugin holds the switch, and the release is installed and
//! nothing more. No project's checkout is written. Every project is a scratch repository, its board
//! a scratch database of the test's own, and the machine's server a stand-in on a private port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use symphony_board::rpc::Client;
use symphony_board::server::{Home, Running};
use symphony_board::{NewTask, Program, Raw};

use super::common::older_board;
use super::consumers::{checkout, project, symphony_crate};
use super::{CHECK, DAY, Fixture, NOW, git};
use symphony::machine::{Entry, Registry};

/// The release a run of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// The release a run a day later tags.
const NEXT: &str = "release-2026-11-13";
/// What names the stand-in for cargo.
const BUILDER: &str = "SYMPHONY_RELEASE_BUILDER";
/// The refresh's wait for the machine's server, in seconds, where the server has followed by the
/// time it looks: what a stall alone reaches, the backstop of a minute's allowance
/// ([`test_time::backstop`]).
const FOLLOWED: &str = "360";

/// The fixture with the refresh on and its Symphony a workspace pinning the board's SurrealDB, with
/// a client's distribution, pushed: the commit.
pub(super) fn refreshing() -> (Fixture, String) {
    let fixture = Fixture::new();
    fixture.write("symphony.toml", &Fixture::project_file("refresh = true\n"));
    symphony_crate(&fixture, 42);
    fixture.write(
        "apps/symphony-server/client/public/assets/app.js",
        "app()\n",
    );
    fixture.write(
        "apps/symphony-server/client/public/index.html",
        "<html></html>\n",
    );
    fixture.write(
        "crates/tasks/src/server.rs",
        &format!(
            "pub const SURREAL_VERSION: &str = \"{}\";\n",
            symphony_board::server::SURREAL_VERSION
        ),
    );
    let commit = fixture.land(
        "crates/core/notes.txt",
        Some("a\n"),
        "the Symphony's crates",
    );
    (fixture, commit)
}

/// `text` at `path`, executable.
fn executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in for cargo in the fixture's folder, building releases at board `schema` until the
/// fixture's `schema` file says another: it counts its runs in `builds`, exits 101 while
/// `build-red` is there, and otherwise writes the four binaries into the build folder it is given.
/// Its server and its board answer what the install asks at that schema, and its `tasks migrate`
/// logs to `migrated.log` where it ran and how many snapshots every project there held then,
/// fails while `migrate-red` is there, or `migrate-red-ID` for the project in the folder `ID`, and
/// otherwise moves the board its checkout reaches to that schema: its path.
pub(super) fn builder(fixture: &Fixture, schema: i64) -> String {
    let root = fixture.root.display().to_string();
    fs::write(fixture.root.join("schema"), schema.to_string()).unwrap();
    let stand_in = fixture.root.join("stand-in");
    executable(
        &stand_in.join("symphony-server"),
        "#!/bin/sh\n[ \"$1\" = --serves ] || exit 9\n\
         echo '{\"protocol\":{\"major\":0,\"minor\":3},\"schema\":@SCHEMA@,\"pin\":1}'\n",
    );
    executable(
        &stand_in.join("tasks"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n\
             api) cat > /dev/null\n\
             echo '{{\"ok\": {{\"api\": {{\"major\": 1, \"minor\": 14}}, \"schema\": @SCHEMA@}}}}' ;;\n\
             migrate) kept=$(ls '{root}'/*/.git/symphony/tasks/snapshots/*.surql 2>/dev/null | wc -l)\n\
             echo \"tasks $* in $PWD, $(echo $kept) snapshots\" >> '{root}/migrated.log'\n\
             if [ -e '{root}/migrate-red' ] || [ -e \"{root}/migrate-red-${{PWD##*/}}\" ]; then \
             echo 'the migrate is red' >&2; exit 1; fi\n\
             addr=$(sed -n 's/.*\"addr\": *\"\\([^\"]*\\)\".*/\\1/p' .git/symphony/tasks/server.json)\n\
             curl -sf -X POST \"http://$addr/sql\" -H 'Accept: application/json' \
             --data 'USE NS sf; USE DB tasks; UPSERT meta:schema SET version = @SCHEMA@;' \
             > /dev/null ;;\n\
             *) exit 9 ;;\nesac\n"
        ),
    );
    executable(&stand_in.join("agent-send"), "#!/bin/sh\n");
    executable(&stand_in.join("symphony"), "#!/bin/sh\n");
    let path = fixture.root.join("cargo-stand-in");
    executable(
        &path,
        &format!(
            "#!/bin/sh\necho \"$PWD $*\" >> '{root}/builds'\n\
             [ ! -e '{root}/build-red' ] || {{ echo 'error: could not compile'; exit 101; }}\n\
             out=\"$CARGO_TARGET_DIR/release\"\nmkdir -p \"$out\"\n\
             schema=$(cat '{root}/schema')\n\
             for b in symphony-server tasks agent-send symphony; do\n\
             sed \"s/@SCHEMA@/$schema/g\" '{root}/stand-in/'\"$b\" > \"$out/$b\"; chmod +x \"$out/$b\"\n\
             done\n"
        ),
    );
    path.to_string_lossy().into_owned()
}

fn machine(fixture: &Fixture) -> PathBuf {
    fixture.root.join("machine")
}

/// The release `current` names in the fixture's machine folder.
fn current(fixture: &Fixture) -> Option<String> {
    fs::read_link(machine(fixture).join("current"))
        .ok()
        .and_then(|link| Some(link.file_name()?.to_string_lossy().into_owned()))
}

/// The machine folder's registry: `projects`, each with the plugins it names, as its own file
/// names them.
fn registered(fixture: &Fixture, projects: &[(&str, &Path, &[&str])]) {
    let entry = |(id, root, plugins): &(&str, &Path, &[&str])| Entry {
        id: (*id).into(),
        name: (*id).into(),
        root: root.to_path_buf(),
        manifest: None,
        plugins: plugins.iter().map(|name| (*name).to_string()).collect(),
        pin: 1,
        schema: 18,
    };
    Registry {
        projects: projects.iter().map(entry).collect(),
    }
    .write(&machine(fixture))
    .unwrap();
}

/// A project `id` in the fixture's folder, a git checkout whose lock pins the Symphony, its file
/// naming `plugins`: its checkout.
fn a_project(fixture: &Fixture, id: &str, plugins: &[&str]) -> PathBuf {
    let crates: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
    let file = format!(
        "[project]\nid = \"{id}\"\n\n[plugins]\ncrates = [{}]\n",
        crates.join(", ")
    );
    let lock = "[[package]]\nname = \"fixture-core\"\nversion = \"0.1.0\"\nsource = \
                \"git+file:///symphony?tag=release-2026-11-01#0123456789abcdef\"\n";
    project(
        fixture,
        id,
        &[("symphony.toml", file.as_str()), ("Cargo.lock", lock)],
    )
}

/// What a project's checkout holds that a refresh must leave alone: its commits, branches,
/// worktrees and files.
fn untouched(root: &Path) -> (String, String, String, Vec<(PathBuf, Vec<u8>)>) {
    (
        git(root, &["rev-parse", "HEAD"]),
        git(root, &["branch", "--list"]),
        git(root, &["worktree", "list", "--porcelain"]),
        checkout(root),
    )
}

/// A project's board whose SurrealDB server its test started, stopped when the test ends, however
/// it ends: cleanly, through [`Home::stop`], so no later run's `lsof` reads it and none lives until
/// the binary exits. A stop that fails fails the test, unless it is failing already.
struct Board(Home);

impl Board {
    /// The board in the project `project`'s common dir, its server started.
    fn started(project: &Path) -> (Board, Running) {
        let home = Home::in_common(&project.join(".git"), Some(&super::board::surreal()));
        let board = Board(home.supervised(true));
        let running = board.ensure().unwrap();
        (board, running)
    }
}

impl std::ops::Deref for Board {
    type Target = Home;

    fn deref(&self) -> &Home {
        &self.0
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        if let Err(problem) = self.0.stop()
            && !std::thread::panicking()
        {
            panic!(
                "the board's server in {}: {problem}",
                self.0.dir().display()
            );
        }
    }
}

/// The project's board, its server started on the data in its common dir, made by the build of
/// schema `schema` with a task on it, its one writer this test on the host `this-host`: its board,
/// which stops that server when it is dropped, the server that runs it and a client of it.
fn board_at(project: &Path, schema: i64) -> (Board, Running, Client) {
    let (board, running) = Board::started(project);
    let client = Client::new(&running.addr, "sf", "tasks");
    older_board(Client::new(&running.addr, "sf", "tasks"), schema, NOW)
        .create(&NewTask::new(
            "sim/kept",
            "What the snapshot keeps",
            "owner",
        ))
        .unwrap();
    (board, running, client)
}

/// The writers' lines on the board `client` reaches moved back `by` seconds.
fn aged(client: &Client, by: f64) {
    Raw::on(client)
        .table("writer")
        .matching(json!({"host": "this-host"}))
        .add("seen", -by)
        .unwrap();
}

/// A line on the board `client` reaches of `program` at `path` on this host at `schema`, seen now.
fn writes(client: &Client, program: &str, path: &Path, schema: i64) {
    let host = symphony_board::sys::hostname();
    let path = path.to_string_lossy().into_owned();
    let seen = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    Raw::on(client)
        .row("writer", json!([host, path]))
        .put(
            json!({"program": program, "path": path, "host": host, "schema": schema, "seen": seen}),
        )
        .unwrap();
}

/// What the test's own line on a board says of it, as a refresh names a writer behind.
fn this_writer() -> String {
    let program = Program::this();
    format!(
        "{} at schema 18 ({} on this-host, last seen ",
        program.name, program.path
    )
}

/// A stand-in for the machine's server on a private port, recorded in the machine folder: each
/// `/api/front` is answered as `answer()` says then (a pid, the root it serves, and the release it
/// is, if any), or as a server not yet serving (503) while that says none.
fn server_as(
    fixture: &Fixture,
    answer: impl Fn() -> Option<(i64, String, Option<String>)> + Send + 'static,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // The whole request first: one closed on bytes unread is reset, and the answer lost.
            let mut asked = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !asked.windows(4).any(|end| end == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => asked.extend_from_slice(&chunk[..read]),
                }
            }
            let Some((pid, root, release)) = answer() else {
                let _ = write!(stream, "HTTP/1.0 503 Service Unavailable\r\n\r\n");
                continue;
            };
            let front = json!({
                "pid": pid,
                "port": port,
                "root": root,
                "ready": true,
                "app": null,
                "tailnet": null,
                "release": release,
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{front}",
                front.len()
            );
        }
    });
    fs::write(
        machine(fixture).join("server.json"),
        format!("{{\"port\": {port}}}\n"),
    )
    .unwrap();
    port
}

/// A release's server, `before` as process 4242 while `current` names no other release, and once
/// it does, that release as process 4343, as its reload follows `current` by its hand-off; or,
/// with `follows` false, `before` whatever `current` names.
fn release_server(fixture: &Fixture, before: &str, follows: bool) -> u16 {
    let link = machine(fixture).join("current");
    let before = before.to_string();
    server_as(fixture, move || {
        let named = (fs::read_link(&link).ok())
            .and_then(|link| Some(link.file_name()?.to_string_lossy().into_owned()));
        let release = named.filter(|_| follows).unwrap_or_else(|| before.clone());
        let pid = if release == before { 4242 } else { 4343 };
        Some((pid, "/machine".into(), Some(release)))
    })
}

/// A project's build serving the machine: process 4242, built from `root`, which never follows
/// `current`.
fn project_server(fixture: &Fixture, root: &Path) -> u16 {
    let root = root.to_string_lossy().into_owned();
    server_as(fixture, move || Some((4242, root.clone(), None)))
}

/// The snapshot a move of `project`'s board to `schema` by `release` keeps.
fn snapshot(project: &Path, schema: i64, release: &str) -> PathBuf {
    project.join(format!(
        ".git/symphony/tasks/snapshots/before-schema-{schema}-{release}.surql"
    ))
}

fn migrated(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("migrated.log")).unwrap_or_default()
}

#[test]
fn a_refresh_installs_moves_the_boards_its_writers_allow_points_current_and_waits_for_the_server() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    let gamma = a_project(&fixture, "gamma", &[]);
    registered(
        &fixture,
        &[
            ("alpha", &alpha, &[]),
            ("beta", &beta, &[]),
            ("gamma", &gamma, &[]),
        ],
    );
    // alpha's writers are a release's own: an older release's tasks on this machine, at 18.
    let (alpha_board, alpha_running, alpha_client) = board_at(&alpha, 18);
    Raw::on(&alpha_client).table("writer").delete().unwrap();
    let older = machine(&fixture).join("releases/release-2026-11-01/bin/tasks");
    writes(&alpha_client, "tasks", &older, 18);
    // beta's last writer behind was seen over a week ago; gamma keeps no board.
    let (beta_board, _, beta_client) = board_at(&beta, 18);
    aged(&beta_client, 8.0 * DAY);
    let before = [untouched(&alpha), untouched(&beta), untouched(&gamma)];
    let port = release_server(&fixture, "release-2026-11-01", true);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", FOLLOWED),
    ];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let releases = machine(&fixture).join("releases");
    assert!(
        out.contains(&format!(
            "release: refresh: installed {RELEASE} in {}, built in ",
            releases.join(RELEASE).display()
        )),
        "{out}"
    );
    let tasks = releases.join(RELEASE).join("bin/tasks");
    let line = |id: &str, project: &Path, command: &str| {
        format!(
            "release: refresh: {id}: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks {command}`, its store snapshotted first to {}\n",
            snapshot(project, 19, RELEASE).display()
        )
    };
    // alpha's only writer behind is a release's own, which current takes on: left behind by name.
    assert!(
        out.contains(&line("alpha", &alpha, "migrate --leave-behind")),
        "{out}"
    );
    assert!(out.contains(&line("beta", &beta, "migrate")), "{out}");
    assert!(
        out.contains(&format!(
            "release: refresh: gamma: it keeps no board ({}/.git/symphony/tasks is not there): \
             nothing to move\n",
            gamma.display()
        )),
        "{out}"
    );
    // Then current, and the server that follows it.
    assert!(
        out.contains(&format!(
            "release: refresh: {} names {RELEASE} (it named no release): a project with no build \
             of its own runs it\n",
            machine(&fixture).join("current").display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: the machine's server (process 4242, port {port}), the release \
             release-2026-11-01, followed current by its own reload \
             (apps/symphony-server/src/reload.rs): process 4343 on port {port} answers as {RELEASE}\n"
        )),
        "{out}"
    );
    for step in [
        "refresh-install",
        "refresh-alpha",
        "refresh-beta",
        "refresh-gamma",
    ] {
        assert!(
            out.contains(&format!("release: {step} green (")),
            "{step}\n{out}"
        );
    }
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    // Both boards snapshotted before the first move, each moved by the release's own tasks in its
    // main checkout, on the same database, never restarted.
    assert_eq!(
        migrated(&fixture),
        format!(
            "tasks migrate --leave-behind in {}, 2 snapshots\ntasks migrate in {}, 2 snapshots\n",
            alpha.display(),
            beta.display()
        )
    );
    assert!(tasks.is_file());
    for (board, project) in [(&alpha_board, &alpha), (&beta_board, &beta)] {
        assert_eq!(board.schema("tasks").unwrap(), Some(19));
        let kept = fs::read_to_string(snapshot(project, 19, RELEASE)).unwrap();
        assert!(kept.contains("sim/kept"), "{kept}");
    }
    assert_eq!(alpha_board.recorded(), Some(alpha_running));
    // No project's checkout gains a branch, a commit, a worktree or a moved pin.
    let after = [untouched(&alpha), untouched(&beta), untouched(&gamma)];
    assert_eq!(after, before);
    assert!(!fixture.root.join("wt/release-refresh").exists());

    // The record and the status say it, and the machine's own line names the release.
    let run = fixture.runs().pop().unwrap();
    let steps: Vec<&str> = (run["refreshed"]["steps"].as_array().unwrap().iter())
        .map(|step| step["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        steps,
        [
            "refresh-install",
            "refresh-alpha",
            "refresh-beta",
            "refresh-gamma"
        ]
    );
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("last refresh: green, of release-2026-11-12 by the run ")
            && status.contains("\n  alpha: its board moved from schema 18 to 19 by ")
            && status.contains(&format!("\nmachine: current names {RELEASE}; ")),
        "{status}"
    );

    // A release that keeps the schema current names moves nothing.
    fixture.land("crates/core/notes.txt", Some("b\n"), "a landing after it");
    let (code, out, err) = fixture.release_with(NOW + DAY, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: {NEXT} serves board schema 19, as the release current names serves \
             19: no board moves\n"
        )),
        "{out}"
    );
    assert!(out.contains(&format!(
        "names {NEXT} (it named {RELEASE}, which stays installed)"
    )));
    assert!(!out.contains("refresh-alpha"), "no board is judged: {out}");
    assert_eq!(migrated(&fixture).lines().count(), 2, "no migrate ran");
    assert_eq!(current(&fixture).as_deref(), Some(NEXT));
}

#[test]
fn a_lagging_writer_holds_every_board_and_current_until_it_is_rebuilt() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    registered(&fixture, &[("alpha", &alpha, &[]), ("beta", &beta, &[])]);
    // alpha's writer, this test's build at 18, was seen just now; beta's over a week ago.
    let (alpha_board, _, alpha_client) = board_at(&alpha, 18);
    let (beta_board, _, beta_client) = board_at(&beta, 18);
    aged(&beta_client, 8.0 * DAY);
    let port = project_server(&fixture, &alpha);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let before = [untouched(&alpha), untouched(&beta)];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "the project's to finish is green here: {out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: the project's to finish: its board stays at schema 18: a \
             writer seen there in the last week is behind {RELEASE}'s schema 19: {}",
            this_writer()
        )) && out.contains(
            " ago); rebuild each at schema 19 and run it once, which records it, then an \
             orchestrator's `just release --refresh` moves it\n"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "release: refresh: beta: its board can move from schema 18 to 19, and stays at 18 \
             while alpha holds the release\n"
        ),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: current is not pointed at {RELEASE}: it names no release, until \
             each board above can move\n"
        )),
        "{out}"
    );
    assert!(
        out.contains("release: refresh-alpha the project's to finish (")
            && out.contains("release: refresh-beta green ("),
        "{out}"
    );
    assert!(
        !out.contains("machine's server"),
        "nothing is waited for: {out}"
    );
    assert!(!out.contains("release: filed"), "{out}");
    // Nothing moved, nothing snapshotted, current not made, and the release installed for later.
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(18));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(18));
    assert!(migrated(&fixture).is_empty());
    assert!(!alpha.join(".git/symphony/tasks/snapshots").exists());
    assert!(!beta.join(".git/symphony/tasks/snapshots").exists());
    assert_eq!(current(&fixture), None);
    assert!(
        machine(&fixture)
            .join("releases")
            .join(RELEASE)
            .join("manifest.json")
            .is_file()
    );
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("last refresh: green, 1 step left to its project, of ")
            && status.contains("  refresh-alpha the project's to finish, filed nowhere here; log ")
            && status.contains(
                "  by hand: each writer named behind the release is rebuilt at its schema and run \
                 once, which records it;"
            )
            && status.contains(&format!(
                "\nmachine: current names no release, and {RELEASE} is installed in "
            )),
        "{status}"
    );

    // The writer rebuilt at the release's schema and run once: both boards move, alpha's first,
    // and current is pointed. The machine's server is a project's build: its switch is a stop and
    // a start, the owner's word.
    Raw::on(&alpha_client).table("writer").delete().unwrap();
    writes(
        &alpha_client,
        "tasks",
        Path::new("/work/alpha/.target/release/tasks"),
        19,
    );
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!("{RELEASE} is installed already in ")),
        "{out}"
    );
    assert!(
        out.contains("release: refresh: alpha: its board moved from schema 18 to 19 by ")
            && out.contains("release: refresh: beta: its board moved from schema 18 to 19 by "),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert!(
        out.contains(&format!(
            "release: refresh: the machine's server (process 4242, port {port}) is the build of {}, \
             whose reload follows that checkout and never current: a stop and a start (`just \
             symphony --stop`, then `just symphony`), the owner's word, make {RELEASE} the \
             machine's server\n",
            alpha.display()
        )),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
    assert_eq!([untouched(&alpha), untouched(&beta)], before);
}

#[test]
fn every_snapshot_comes_first_and_a_move_that_fails_partway_is_finished_by_the_retry() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    let gamma = a_project(&fixture, "gamma", &[]);
    registered(
        &fixture,
        &[
            ("alpha", &alpha, &[]),
            ("beta", &beta, &[]),
            ("gamma", &gamma, &[]),
        ],
    );
    let boards: Vec<Board> = [&alpha, &beta, &gamma]
        .into_iter()
        .map(|project| {
            let (board, _, client) = board_at(project, 18);
            aged(&client, 8.0 * DAY);
            board
        })
        .collect();
    let schemas = || -> Vec<Option<i64>> {
        (boards.iter())
            .map(|board| board.schema("tasks").unwrap())
            .collect()
    };
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let waits = |id: &str, why: &str| {
        format!(
            "release: refresh: {id}: its board can move from schema 18 to 19, and stays at 18 \
             while {why}\n"
        )
    };

    // gamma's snapshot cannot be written: taken before any move, it moves nothing.
    let blocked = gamma.join(".git/symphony/tasks/snapshots");
    fs::write(&blocked, "").unwrap();
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(
            "release: refresh: gamma: red: the snapshot of its store failed, so no board was \
             moved, this one or another: "
        ) && out.contains(&waits("alpha", "the snapshot of gamma's store failed"))
            && out.contains(&waits("beta", "the snapshot of gamma's store failed")),
        "{out}"
    );
    assert!(migrated(&fixture).is_empty(), "no migrate ran");
    assert_eq!(schemas(), [Some(18), Some(18), Some(18)]);
    assert_eq!(current(&fixture), None);
    assert!(fixture.reds()["tests/release-red-refresh-gamma"].red());

    // Every snapshot taken, then beta's migrate fails: alpha's board stays moved, gamma's waits,
    // and current is not pointed.
    fs::remove_file(&blocked).unwrap();
    fs::write(fixture.root.join("migrate-red-beta"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate`, its store snapshotted first to {}\n",
            snapshot(&alpha, 19, RELEASE).display()
        )) && out.contains(&format!(
            "release: refresh: beta: red: {RELEASE}'s `tasks migrate` in {} failed (",
            beta.display()
        )) && out.contains(&format!(
            "); its store as it was before is in {}, and an orchestrator's `just release \
             --refresh` retries\n",
            snapshot(&beta, 19, RELEASE).display()
        )) && out.contains(&waits("gamma", "the move of beta's board failed")),
        "{out}"
    );
    assert_eq!(
        migrated(&fixture),
        format!(
            "tasks migrate in {}, 3 snapshots\ntasks migrate in {}, 3 snapshots\n",
            alpha.display(),
            beta.display()
        ),
        "all three snapshots before the first move"
    );
    assert_eq!(schemas(), [Some(19), Some(18), Some(18)]);
    assert_eq!(current(&fixture), None);
    assert!(
        out.contains("release: filed tests/release-red-refresh-beta")
            && out.contains("release: closed tests/release-red-refresh-gamma"),
        "{out}"
    );

    // The retry takes alpha's board as moved, moves the other two and points current.
    fs::remove_file(fixture.root.join("migrate-red-beta")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 240.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board is at schema 19, {RELEASE}'s or past it: nothing \
             to move\n"
        )) && out.contains("release: refresh: beta: its board moved from schema 18 to 19 by ")
            && out.contains("release: refresh: gamma: its board moved from schema 18 to 19 by ")
            && out.contains("release: closed tests/release-red-refresh-beta"),
        "{out}"
    );
    assert_eq!(
        migrated(&fixture).lines().count(),
        4,
        "{}",
        migrated(&fixture)
    );
    assert_eq!(schemas(), [Some(19), Some(19), Some(19)]);
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
}

#[test]
fn what_cannot_be_judged_is_left_out_and_every_board_that_holds_the_release_is_named() {
    let (fixture, _) = refreshing();
    let plain = fixture.root.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let gone = fixture.root.join("gone");
    let fresh = a_project(&fixture, "fresh", &[]);
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    let gamma = a_project(&fixture, "gamma", &[]);
    registered(
        &fixture,
        &[
            ("a/b", &plain, &[]),
            ("gone", &gone, &[]),
            ("plain", &plain, &[]),
            ("fresh", &fresh, &[]),
            ("alpha", &alpha, &[]),
            ("beta", &beta, &[]),
            ("gamma", &gamma, &[]),
        ],
    );
    // fresh's database runs and holds no board yet; alpha has two writers behind, beta one, and
    // gamma's last was seen over a week ago.
    let _fresh_board = Board::started(&fresh);
    let (_alpha_board, _, alpha_client) = board_at(&alpha, 18);
    let gate = Path::new("/work/alpha/.target/debug/gate-review");
    writes(&alpha_client, "gate-review", gate, 18);
    let _beta_board = board_at(&beta, 18);
    let (gamma_board, _, gamma_client) = board_at(&gamma, 18);
    aged(&gamma_client, 8.0 * DAY);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    for line in [
        "left out: a/b: its id names no folder of its own".to_string(),
        format!("left out: gone: its checkout {} is gone", gone.display()),
        format!("left out: plain: {} is not a git checkout", plain.display()),
        "fresh: its board is not made yet: nothing to move".to_string(),
        "gamma: its board can move from schema 18 to 19, and stays at 18 while alpha, beta hold \
         the release"
            .to_string(),
    ] {
        assert!(
            out.contains(&format!("release: refresh: {line}\n")),
            "{line}\n{out}"
        );
    }
    let host = symphony_board::sys::hostname();
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: the project's to finish: its board stays at schema 18: 2 \
             writers seen there in the last week are behind {RELEASE}'s schema 19: "
        )) && out.contains(&format!(
            "gate-review at schema 18 ({} on {host}, last seen ",
            gate.display()
        )) && out.contains(&format!(
            "release: refresh: beta: the project's to finish: its board stays at schema 18: a \
             writer seen there in the last week is behind {RELEASE}'s schema 19: {}",
            this_writer()
        )),
        "{out}"
    );
    assert!(
        !out.contains("refresh-gone") && !out.contains("refresh-plain"),
        "{out}"
    );
    assert_eq!(gamma_board.schema("tasks").unwrap(), Some(18));
    assert_eq!(current(&fixture), None);
    assert!(migrated(&fixture).is_empty());

    // A registry that cannot be read is the refresh's own red: nothing judged, current as it was.
    fs::write(machine(&fixture).join("projects.json"), "not a registry\n").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(
            "release: refresh red (0 s): the machine's registry cannot be read, so no board moved \
             and current is as it was (it names no release): "
        ),
        "{out}"
    );
    assert!(
        out.contains("release: filed tests/release-red-refresh"),
        "{out}"
    );
    assert!(fixture.reds()["tests/release-red-refresh"].red());
    assert_eq!(current(&fixture), None);
}

#[test]
fn a_project_naming_a_linked_plugin_holds_the_switch_the_release_installed_and_current_unpointed() {
    let (fixture, _) = refreshing();
    let shelf = a_project(&fixture, "shelf", &["shelf-plugin"]);
    registered(&fixture, &[("shelf", &shelf, &["shelf-plugin"])]);
    let (board, _, client) = board_at(&shelf, 18);
    aged(&client, 8.0 * DAY);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let before = untouched(&shelf);
    let held = format!(
        "shelf names the linked plugin shelf-plugin, and a release's server serves a plugin only \
         from its process: a [plugins.process] command for it in {} lets the release serve every \
         project (docs/PLUGIN_PROTOCOL.md)",
        shelf.join("symphony.toml").display()
    );

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let said = format!(
        "the switch to {RELEASE} is held: {held}; {RELEASE} is installed, no board moves, and \
         current is not pointed at it (it names no release)"
    );
    assert!(
        out.contains(&format!("release: refresh: {said}\n")),
        "{out}"
    );
    assert!(out.contains("release: refresh-install green ("), "{out}");
    assert!(!out.contains("refresh-shelf"), "no board is judged: {out}");
    assert_eq!(current(&fixture), None);
    assert!(
        machine(&fixture)
            .join("releases")
            .join(RELEASE)
            .join("bin/tasks")
            .is_file()
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(18));
    assert!(migrated(&fixture).is_empty());
    assert_eq!(untouched(&shelf), before);
    // The status names the project that holds it, in the refresh's lines and the machine's own.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!("\n  {said}\n"))
            && status.contains(&format!(
                "\nmachine: current names no release, and {RELEASE} is installed in {}: a \
                 project's build is the machine's server\nmachine: the switch is held: {held}\n",
                machine(&fixture).join("releases").display()
            )),
        "{status}"
    );

    // Its plugin answering from a process lets the switch go: the next refresh moves its board and
    // points current, with no server to wait for.
    fs::write(
        shelf.join("symphony.toml"),
        "[project]\nid = \"shelf\"\n\n[plugins]\ncrates = [\"shelf-plugin\"]\n\n\
         [plugins.process]\nshelf-plugin = [\"bin/shelf-plugin\"]\n",
    )
    .unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("release: refresh: shelf: its board moved from schema 18 to 19 by "),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert!(
        out.contains(&format!(
            "release: refresh: the machine's server: none answers on this machine, so none follows \
             current; the next start of the one server (`just symphony`) runs {RELEASE} when it \
             serves every registered project\n"
        )),
        "{out}"
    );
}

#[test]
fn a_failed_move_is_red_and_leaves_current_where_it_was_until_a_refresh_gets_through() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    registered(&fixture, &[("alpha", &alpha, &[])]);
    let (board, running, client) = board_at(&alpha, 18);
    aged(&client, 8.0 * DAY);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    // The first release, at the board's schema: nothing to move, and current names it.
    let at_18 = builder(&fixture, 18);
    let extra = [
        (BUILDER, at_18.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board is at schema 18, {RELEASE}'s or past it: nothing \
             to move\n"
        )),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    // A server that never takes the next: the wait ends at its bound and says so.
    let port = release_server(&fixture, RELEASE, false);

    // The next raises the schema, and its migrate fails: red, current where it was.
    fixture.land("crates/core/notes.txt", Some("b\n"), "a landing after it");
    let at_19 = builder(&fixture, 19);
    fs::write(fixture.root.join("migrate-red"), "").unwrap();
    let extra = [
        (BUILDER, at_19.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", "1"),
    ];
    let (code, out, err) = fixture.release_with(NOW + DAY, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let kept = snapshot(&alpha, 19, NEXT);
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: red: {NEXT}'s `tasks migrate` in {} failed (`",
            alpha.display()
        )) && out.contains("the migrate is red")
            && out.contains(&format!(
                "); its store as it was before is in {}, and an orchestrator's `just release \
                 --refresh` retries\n",
                kept.display()
            )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: current is not pointed at {NEXT}: it names {RELEASE}, until each \
             board above can move\n"
        )),
        "{out}"
    );
    assert!(
        out.contains("release: filed tests/release-red-refresh-alpha"),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert_eq!(board.schema("tasks").unwrap(), Some(18));
    assert!(fs::read_to_string(&kept).unwrap().contains("sim/kept"));
    assert!(fixture.reds()["tests/release-red-refresh-alpha"].red());

    // Green again: moved, current pointed, the server waited for, and the red closed.
    fs::remove_file(fixture.root.join("migrate-red")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + DAY + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board moved from schema 18 to 19 by {NEXT}'s own `tasks \
             migrate`, its store snapshotted first to {}\n",
            kept.display()
        )),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(NEXT));
    assert!(
        out.contains(&format!(
            "release: refresh: the machine's server (process 4242, port {port}) still answers as \
             the release {RELEASE} after 1 s: its own reload"
        )),
        "{out}"
    );
    assert!(
        out.contains("release: closed tests/release-red-refresh-alpha"),
        "{out}"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(19));
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
}

#[test]
fn a_board_not_running_holds_the_release_and_one_from_before_the_writer_record_is_its_own() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let old = a_project(&fixture, "old", &[]);
    registered(&fixture, &[("alpha", &alpha, &[]), ("old", &old, &[])]);
    // alpha's board is there, its database not running; old's from schema 17, before writers.
    let common = alpha.join(".git");
    fs::create_dir_all(common.join("symphony/tasks")).unwrap();
    let (old_board, _, _) = board_at(&old, 17);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: red: the board's database in {} is not running (no address \
             is recorded): nothing is read or exported, and nothing is started: its schema and \
             writers were not read, and nothing was moved; once it runs (any `just tasks` in {} \
             starts it), an orchestrator's `just release --refresh` moves it\n",
            common.join("symphony/tasks").display(),
            alpha.display()
        )),
        "{out}"
    );
    assert!(
        !common.join("symphony/tasks/server.json").exists(),
        "nothing started it"
    );
    let own = format!(
        "release: refresh: old: the project's to finish: its board is at schema 17, from before \
         the board recorded who writes it (schema 18), so it is never moved here: `cd {} && just \
         tasks migrate`, its own, brings it up\n",
        old.display()
    );
    assert!(out.contains(&own), "{out}");
    assert!(out.contains("current is not pointed at"), "{out}");
    assert_eq!(current(&fixture), None);
    assert!(fixture.reds()["tests/release-red-refresh-alpha"].red());

    // alpha gone from the registry: old's board, behind every release, holds nothing.
    registered(&fixture, &[("old", &old, &[])]);
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(&own), "{out}");
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert_eq!(old_board.schema("tasks").unwrap(), Some(17));
    assert!(migrated(&fixture).is_empty());
}

#[test]
fn a_board_whose_move_could_replace_its_database_is_left_running_and_unmoved() {
    let (fixture, _) = refreshing();
    // The release pins a SurrealDB newer than the one the board runs.
    fixture.land(
        "crates/tasks/src/server.rs",
        Some("pub const SURREAL_VERSION: &str = \"99.0.0\";\n"),
        "a newer SurrealDB",
    );
    let alpha = a_project(&fixture, "alpha", &[]);
    registered(&fixture, &[("alpha", &alpha, &[])]);
    let (board, running, client) = board_at(&alpha, 18);
    aged(&client, 8.0 * DAY);
    let builder = builder(&fixture, 19);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: red: its board's database runs SurrealDB {} and {RELEASE} \
             pins 99.0.0: moving its schema would replace that server, and the refresh stops no \
             board's database, so nothing was moved\n",
            running.version
        )),
        "{out}"
    );
    // One whose record names no version fails closed the same way.
    fixture.land("crates/tasks/src/server.rs", None, "no pinned SurrealDB");
    let (code, out, err) = fixture.release_with(NOW + DAY, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: red: the SurrealDB {NEXT} pins cannot be read \
             (crates/tasks/src/server.rs there names no SURREAL_VERSION)"
        )),
        "{out}"
    );
    assert!(migrated(&fixture).is_empty(), "no migrate ran");
    assert!(!alpha.join(".git/symphony/tasks/snapshots").exists());
    assert_eq!(board.schema("tasks").unwrap(), Some(18));
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
    assert_eq!(current(&fixture), None);
}
