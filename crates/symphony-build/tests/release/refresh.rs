//! A green release's refresh of the machine: the release installed by a stand-in for cargo in a
//! temporary machine folder, each registered project's board moved to the release's schema when
//! every writer counted there in the last week is a release's own or at that schema, its store
//! snapshotted just before its own move, `current` pointed whatever a board did, and the machine's
//! server waited for while it follows `current`. A writer behind the release holds its own board
//! alone, the project's to finish, and never another board or `current`; a project naming a linked
//! plugin holds the switch, and the release is installed and nothing more. No project's checkout is
//! written. Every project is a scratch repository, its board on a PostgreSQL of its own that its
//! test starts on the data in its common dir, and the machine's server a stand-in on a private
//! port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use symphony_board::server::{Engine, FOLDER, GONE, Home, Postgres, Running, postgres};
use symphony_board::{DATABASE, Database, NewTask, Program, Raw, pg};

use super::common::older_board;
use super::consumers::{checkout, project, symphony_crate};
use super::{CHECK, DAY, Fixture, NOW, git};
use symphony::machine::{Entry, Registry};

/// The release a run of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// The release a run a day later tags.
const NEXT: &str = "release-2026-11-13";
/// The release a run two days later tags.
const LATER: &str = "release-2026-11-14";
/// What names the stand-in for cargo.
const BUILDER: &str = "SYMPHONY_RELEASE_BUILDER";
/// The refresh's wait for the machine's server, in seconds, where the server has followed by the
/// time it looks: what a stall alone reaches, the backstop of a minute's allowance
/// ([`test_time::backstop`]).
const FOLLOWED: &str = "360";

/// The fixture with the refresh on and its Symphony a workspace with a client's distribution,
/// pushed: the commit.
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
    let commit = fixture.land(
        "crates/core/notes.txt",
        Some("a\n"),
        "the Symphony's crates",
    );
    (fixture, commit)
}

/// `text` at `path`, executable.
pub(super) fn executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in for cargo in the fixture's folder, building releases at board `schema` until the
/// fixture's `schema` file says another: it counts its runs in `builds`, exits 101 while
/// `build-red` is there, and otherwise writes the five binaries into the build folder it is given.
/// Its server and its board answer what the install asks at that schema, and its `tasks migrate`
/// logs to `migrated.log` where it ran and how many snapshots every project there held then,
/// fails while `migrate-red` is there, or `migrate-red-ID` for the project in the folder `ID`, and
/// otherwise moves the board its checkout reaches to that schema, as the store keeps it (the
/// `meta` row `"schema"`), by `psql` on the socket its PostgreSQL's record names: its path. Any
/// other `tasks api` request goes to the board's own binary, on the scratch database `filed` of
/// the PostgreSQL of the board its checkout reaches: what a refresh files on a project's board
/// lands there.
pub(super) fn builder(fixture: &Fixture, schema: i64) -> String {
    let root = fixture.root.display().to_string();
    let real = super::common::tasks_binary();
    let psql = programs().join("psql").display().to_string();
    // The running server's record, line 5 its socket's folder, as the board reads it.
    let socket = format!(
        "socket=$(sed -n 5p .git/{FOLDER}/{}/postmaster.pid)",
        Engine::PostgreSql.data_folder()
    );
    let user = postgres::USER;
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
             api) req=$(cat)\n\
             case \"$req\" in *'\"method\": \"version\"'*|*'\"method\":\"version\"'*)\n\
             echo '{{\"ok\": {{\"api\": {{\"major\": 1, \"minor\": 14}}, \"schema\": @SCHEMA@}}}}' ;;\n\
             *) {socket}\n\
             printf '%s' \"$req\" | TASKS_SERVER=\"postgresql:$socket\" TASKS_DATABASE=filed '{real}' api ;;\n\
             esac ;;\n\
             migrate) kept=$(ls '{root}'/*/.git/{FOLDER}/snapshots/*.sql 2>/dev/null | wc -l)\n\
             echo \"tasks $* in $PWD, $(echo $kept) snapshots\" >> '{root}/migrated.log'\n\
             if [ -e '{root}/migrate-red' ] || [ -e \"{root}/migrate-red-${{PWD##*/}}\" ]; then \
             echo 'the migrate is red' >&2; exit 1; fi\n\
             {socket}\n\
             '{psql}' -X -q -v ON_ERROR_STOP=1 -h \"$socket\" -U {user} -d {DATABASE} \
             -c \"UPDATE meta SET version = @SCHEMA@ WHERE id = '\\\"schema\\\"'\" > /dev/null ;;\n\
             *) exit 9 ;;\nesac\n"
        ),
    );
    executable(&stand_in.join("agent-send"), "#!/bin/sh\n");
    executable(&stand_in.join("symphony"), "#!/bin/sh\n");
    executable(&stand_in.join("tasks-move"), "#!/bin/sh\n");
    let path = fixture.root.join("cargo-stand-in");
    executable(
        &path,
        &format!(
            "#!/bin/sh\necho \"$PWD $*\" >> '{root}/builds'\n\
             [ ! -e '{root}/build-red' ] || {{ echo 'error: could not compile'; exit 101; }}\n\
             out=\"$CARGO_TARGET_DIR/release\"\nmkdir -p \"$out\"\n\
             schema=$(cat '{root}/schema')\n\
             for b in symphony-server tasks agent-send symphony tasks-move; do\n\
             sed \"s/@SCHEMA@/$schema/g\" '{root}/stand-in/'\"$b\" > \"$out/$b\"; chmod +x \"$out/$b\"\n\
             done\n"
        ),
    );
    path.to_string_lossy().into_owned()
}

pub(super) fn machine(fixture: &Fixture) -> PathBuf {
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
pub(super) fn registered(fixture: &Fixture, projects: &[(&str, &Path, &[&str])]) {
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
pub(super) fn a_project(fixture: &Fixture, id: &str, plugins: &[&str]) -> PathBuf {
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

/// The folder of the PostgreSQL programs the tests' boards run: the ones the store's test server
/// runs, which a project naming none in its file finds on PATH as well.
fn programs() -> PathBuf {
    postgres::discover_programs(&super::board::project_root()).unwrap()
}

/// A project's board on the PostgreSQL its test started on the data in its common dir, stopped
/// when the test ends, however it ends: cleanly, through [`Home::stop_board`], so none lives until
/// the binary exits. A stop that fails fails the test, unless it is failing already.
pub(super) struct Board {
    home: Home,
    /// The project's main checkout, whose file names its PostgreSQL programs.
    root: PathBuf,
}

impl Board {
    /// The board in the project `project`'s common dir, made on PostgreSQL and its server started,
    /// as the project's first board command makes it.
    fn started(project: &Path) -> (Board, Running) {
        let board = Board {
            home: Home::in_common(&project.join(".git")).supervised(true),
            root: project.to_path_buf(),
        };
        let running = board.home.board_server(project, true).unwrap();
        (board, running)
    }

    /// Its PostgreSQL, by the programs its project names.
    fn postgres(&self) -> Postgres {
        self.home.project_postgresql(&self.root).unwrap()
    }

    /// The server that serves it now, started by nothing here.
    fn running(&self) -> Running {
        self.home.board_server(&self.root, false).unwrap()
    }
}

impl std::ops::Deref for Board {
    type Target = Home;

    fn deref(&self) -> &Home {
        &self.home
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        if let Err(problem) = self.home.stop_board()
            && !std::thread::panicking()
        {
            panic!(
                "the board's server in {}: {problem}",
                self.home.dir().display()
            );
        }
    }
}

/// The database `name` on the board's server `running`, at the socket its address names.
fn database(running: &Running, name: &str) -> Database {
    let socket = running.addr.strip_prefix("postgresql:").unwrap();
    pg::Client::new(Path::new(socket), name).into()
}

/// The project's board, its server started on the data in its common dir, made by the build of
/// schema `schema` with a task on it, its one writer this test on the host `this-host`: its board,
/// which stops that server when it is dropped, the server that runs it and its database.
pub(super) fn board_at(project: &Path, schema: i64) -> (Board, Running, Database) {
    let (board, running) = Board::started(project);
    let address = symphony_board::rpc::Client::new(&running.addr, "sf", DATABASE);
    older_board(address, schema, NOW)
        .create(&NewTask::new(
            "sim/kept",
            "What the snapshot keeps",
            "owner",
        ))
        .unwrap();
    let db = database(&running, DATABASE);
    (board, running, db)
}

/// The writers' lines on the board in `db` moved back `by` seconds.
fn aged(db: &Database, by: f64) {
    Raw::of(db)
        .table("writer")
        .matching(json!({"host": "this-host"}))
        .add("seen", -by)
        .unwrap();
}

/// A line on the board in `db` of `program` at `path` on this host at `schema`, seen now.
fn writes(db: &Database, program: &str, path: &Path, schema: i64) {
    wrote(db, program, path, schema, 0.0);
}

/// [`writes`], its line seen `ago` seconds before now.
pub(super) fn wrote(db: &Database, program: &str, path: &Path, schema: i64, ago: f64) {
    let host = symphony_board::sys::hostname();
    let path = path.to_string_lossy().into_owned();
    let seen = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - ago;
    Raw::of(db)
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
/// is, if any, at the board schema that release's manifest names), or as a server not yet serving
/// (503) while that says none. Its project list (`/api/projects`) is the machine folder's
/// `listed.json` while there is one, else the front's answer, which lists no project.
fn server_as(
    fixture: &Fixture,
    answer: impl Fn() -> Option<(i64, String, Option<String>)> + Send + 'static,
) -> u16 {
    let folder = machine(fixture);
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
            let listed = fs::read_to_string(folder.join("listed.json")).ok();
            let schema = (release.as_ref())
                .and_then(|name| {
                    fs::read_to_string(folder.join("releases").join(name).join("manifest.json"))
                        .ok()
                })
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|manifest| manifest["schema"].as_i64())
                .unwrap_or(0);
            let front = match listed.filter(|_| asked.starts_with(b"GET /api/projects")) {
                Some(listed) => listed,
                None => json!({
                    "pid": pid,
                    "port": port,
                    "root": root,
                    "ready": true,
                    "app": null,
                    "tailnet": null,
                    "schema": schema,
                    "release": release,
                })
                .to_string(),
            };
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
        ".git/{FOLDER}/snapshots/before-schema-{schema}-{release}.sql"
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
    let (alpha_board, alpha_running, alpha_db) = board_at(&alpha, 18);
    Raw::of(&alpha_db).table("writer").delete().unwrap();
    let older = machine(&fixture).join("releases/release-2026-11-01/bin/tasks");
    writes(&alpha_db, "tasks", &older, 18);
    // beta's last writer behind was seen over a week ago; gamma keeps no board.
    let (beta_board, _, beta_db) = board_at(&beta, 18);
    aged(&beta_db, 8.0 * DAY);
    let before = [untouched(&alpha), untouched(&beta), untouched(&gamma)];
    let port = release_server(&fixture, "release-2026-11-01", true);
    let builder = builder(&fixture, 19);
    let extra = [
        (BUILDER, builder.as_str()),
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
    // Each board snapshotted just before its own move, each moved by the release's own tasks in
    // its main checkout, on the same database, never restarted.
    assert_eq!(
        migrated(&fixture),
        format!(
            "tasks migrate --leave-behind in {}, 1 snapshots\ntasks migrate in {}, 2 snapshots\n",
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
    assert_eq!(alpha_board.running(), alpha_running, "never restarted");
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
             19: only a board left behind it moves\n"
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
fn a_lagging_writer_holds_its_own_board_alone_and_never_another_or_current() {
    let (fixture, _) = refreshing();
    // Main commits no schema source here, so any build makes its board.
    fixture.land("crates/tasks/src/schema.rs", None, "no schema source");
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    let main = fixture.main.clone();
    registered(
        &fixture,
        &[
            ("symphony", &main, &[]),
            ("alpha", &alpha, &[]),
            ("beta", &beta, &[]),
        ],
    );
    // This repository's board is already past the release, as main moved on while its run ran: a
    // project behind holds it back no more than one at the release's schema would.
    let (ours_board, _, _) = board_at(&main, 20);
    // alpha's writer, this test's build at 18, was seen just now; beta's over a week ago.
    let (alpha_board, _, alpha_db) = board_at(&alpha, 18);
    let (beta_board, _, beta_db) = board_at(&beta, 18);
    aged(&beta_db, 8.0 * DAY);
    let port = project_server(&fixture, &alpha);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];
    let before = [untouched(&alpha), untouched(&beta)];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "the project's to finish is green here: {out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: the project's to finish: its board stays at schema 18: a \
             writer seen there in the last week is behind {RELEASE}'s schema 19: {}",
            this_writer()
        )) && out.contains(
            " ago); rebuild each at schema 19 and run it once, which records it, then the \
             Symphony moves it (`just symphony-cli release-notes` in the project says what \
             taking the release asks of it)\n"
        ),
        "{out}"
    );
    // beta moves all the same, and current is pointed: alpha holds its own board alone.
    assert!(
        out.contains(&format!(
            "release: refresh: beta: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate`, its store snapshotted first to {}\n",
            snapshot(&beta, 19, RELEASE).display()
        )),
        "{out}"
    );
    assert!(!out.contains("holds the release"), "{out}");
    assert!(
        out.contains(&format!(
            "release: refresh: {} names {RELEASE} (it named no release): a project with no build \
             of its own runs it\n",
            machine(&fixture).join("current").display()
        )),
        "{out}"
    );
    assert!(
        out.contains("release: refresh-alpha the project's to finish (")
            && out.contains("release: refresh-beta green ("),
        "{out}"
    );
    // The machine's server is a project's build: its switch is a stop and a start, the owner's
    // word.
    assert!(
        out.contains(&format!(
            "release: refresh: the machine's server (process 4242, port {port}) is the build of \
             {}, whose reload follows that checkout and never current",
            alpha.display()
        )),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(18));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(
        ours_board.schema("tasks").unwrap(),
        Some(20),
        "never judged here"
    );
    assert_eq!(
        migrated(&fixture),
        format!("tasks migrate in {}, 1 snapshots\n", beta.display())
    );
    assert!(!alpha.join(".git/symphony/tasks/snapshots").exists());
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("last refresh: green, 1 step left to its project, of ")
            && status.contains(
                "  refresh-alpha the project's to finish, its schema update's task filed (below); \
                 log "
            )
            && status.contains(
                "  by hand: each writer named behind the release is rebuilt at its schema and run \
                 once, which records it;"
            )
            && status.contains(&format!("\nmachine: current names {RELEASE}; ")),
        "{status}"
    );

    // The writer rebuilt at the release's schema and run once: alpha's board moves too.
    Raw::of(&alpha_db).table("writer").delete().unwrap();
    writes(
        &alpha_db,
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
    // current serves 19 already, so only alpha's board, still behind it, is judged and said.
    assert!(
        out.contains("release: refresh: alpha: its board moved from schema 18 to 19 by ")
            && !out.contains("release: refresh: beta: "),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
    assert_eq!([untouched(&alpha), untouched(&beta)], before);
}

/// A build its project no longer runs, still at its path but unseen there for longer than the
/// project's bound, holds no board: said, and left behind by name. The build a link in the
/// project's main checkout names holds it however long ago it last wrote, and the judgement says
/// how the project takes the release.
#[test]
fn a_build_its_project_no_longer_runs_holds_no_board_and_the_one_it_runs_does() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    registered(&fixture, &[("alpha", &alpha, &[]), ("beta", &beta, &[])]);
    let host = symphony_board::sys::hostname();
    // alpha's writers behind: a superseded build named by its hash, still in its cache, unseen for
    // two days; and the build its link names now, which wrote three days ago.
    let (alpha_board, _, alpha_db) = board_at(&alpha, 18);
    Raw::of(&alpha_db).table("writer").delete().unwrap();
    let superseded = fixture.root.join("builds-of/alpha/bin/tasks-0a1b2c");
    executable(&superseded, "#!/bin/sh\nexit 9\n");
    wrote(&alpha_db, "tasks-0a1b2c", &superseded, 18, 2.0 * DAY);
    // beta's: the same, its link naming the newer build.
    let (beta_board, _, beta_db) = board_at(&beta, 18);
    Raw::of(&beta_db).table("writer").delete().unwrap();
    let linked = fixture.root.join("builds-of/beta/bin/tasks-3d4e5f");
    executable(&linked, "#!/bin/sh\nexit 9\n");
    wrote(&beta_db, "tasks-3d4e5f", &linked, 18, 3.0 * DAY);
    fs::create_dir_all(beta.join(".target/bin")).unwrap();
    std::os::unix::fs::symlink(&linked, beta.join(".target/bin/tasks")).unwrap();
    // Built before they wrote, so neither is gone from its path.
    for build in [&superseded, &linked] {
        let file = fs::File::options().write(true).open(build).unwrap();
        file.set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(4 * 86_400),
        )
        .unwrap();
    }
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate --leave-behind`, its store snapshotted first to {}; not counted, as \
             its project runs another build now and it has not run here for over 24 h ([board] \
             stale_writer_hours): tasks-0a1b2c at schema 18 ({} on {host}, last seen 48 h ago)\n",
            snapshot(&alpha, 19, RELEASE).display(),
            superseded.display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: beta: the project's to finish: its board stays at schema 18: a \
             writer seen there in the last week is behind {RELEASE}'s schema 19: tasks-3d4e5f at \
             schema 18 ({} on {host}, last seen 72 h ago); rebuild each at schema 19 and run it \
             once, which records it, then the Symphony moves it; the project's own build of the \
             board's binaries is among them (a link in its `.target/bin`): it takes the release \
             by moving its pin to it and rebuilding them, or, where those binaries are the \
             board's own, by removing those links, so its recipes run the release's \
             (`current/bin` in the machine folder), as a project with no build of its own does, \
             and its board moves with each release (",
            linked.display()
        )),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(18));
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));

    // beta's project takes the release by removing its link: its build, last run three days ago,
    // holds its board no more, and the retry moves it.
    fs::remove_file(beta.join(".target/bin/tasks")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: beta: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate --leave-behind`, its store snapshotted first to {}; not counted, as \
             its project runs another build now",
            snapshot(&beta, 19, RELEASE).display()
        )),
        "{out}"
    );
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
}

/// A writer's line keeps its program's path alone, so the refresh looks at what is there now, on
/// this host, and runs none of it: nothing at the path, or a file written after the line's last
/// possible write, is a program that can write no more, said and left behind by name. The build
/// that wrote, still at its path, holds its own board as before, and no other.
#[test]
fn a_writer_gone_from_its_path_is_said_and_holds_no_board_and_one_still_there_does() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    registered(&fixture, &[("alpha", &alpha, &[]), ("beta", &beta, &[])]);
    let host = symphony_board::sys::hostname();
    // alpha's writers behind: a build named by its hash, superseded and removed, and a build
    // rebuilt at its path since it last wrote there, two hours ago.
    let (alpha_board, _, alpha_db) = board_at(&alpha, 18);
    Raw::of(&alpha_db).table("writer").delete().unwrap();
    let removed = fixture.root.join("builds-of/alpha/bin/tasks-0a1b2c");
    let rebuilt = fixture.root.join("builds-of/alpha/rs-bin/agent-send");
    executable(&rebuilt, "#!/bin/sh\nexit 9\n");
    wrote(&alpha_db, "tasks-0a1b2c", &removed, 18, DAY);
    wrote(&alpha_db, "agent-send", &rebuilt, 18, 2.0 * 3600.0);
    // beta's writer behind is the build that wrote just now, where it was.
    let (beta_board, _, beta_db) = board_at(&beta, 18);
    Raw::of(&beta_db).table("writer").delete().unwrap();
    let standing = fixture.root.join("builds-of/beta/bin/tasks");
    executable(&standing, "#!/bin/sh\nexit 9\n");
    writes(&beta_db, "tasks", &standing, 18);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let not_counted = format!(
        "; not counted, as its program is gone: agent-send at schema 18 ({} on {host}, last seen \
         2 h ago): the file at its path was written after it last ran there, a build that has not \
         opened this board; tasks-0a1b2c at schema 18 ({} on {host}, last seen 24 h ago): nothing \
         is at its path now\n",
        rebuilt.display(),
        removed.display()
    );
    // alpha moves while beta's build holds beta's board alone, leaving its gone writers behind by
    // name, as the release's migrate reads the lines again.
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate --leave-behind`, its store snapshotted first to {}{not_counted}",
            snapshot(&alpha, 19, RELEASE).display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: beta: the project's to finish: its board stays at schema 18: a \
             writer seen there in the last week is behind {RELEASE}'s schema 19: tasks at schema \
             18 ({} on {host}, last seen ",
            standing.display()
        )),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(18));
    assert_eq!(migrated(&fixture).lines().count(), 1);
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));

    // beta's build removed too: its board moves, leaving it behind by name.
    fs::remove_file(&standing).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains("release: refresh: alpha: "), "{out}");
    assert!(
        out.contains(&format!(
            "`tasks migrate --leave-behind`, its store snapshotted first to {}; not counted, as \
             its program is gone: tasks at schema 18 ({} on {host}, last seen ",
            snapshot(&beta, 19, RELEASE).display(),
            standing.display()
        )) && out.contains(" ago): nothing is at its path now\n"),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(migrated(&fixture).lines().count(), 2);
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
}

#[test]
fn each_board_is_snapshotted_before_its_own_move_and_one_that_fails_holds_no_other() {
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
            let (board, _, db) = board_at(project, 18);
            aged(&db, 8.0 * DAY);
            board
        })
        .collect();
    let schemas = || -> Vec<Option<i64>> {
        (boards.iter())
            .map(|board| board.schema("tasks").unwrap())
            .collect()
    };
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    // gamma's snapshot cannot be written: its board alone is not moved, and current is pointed.
    let blocked = gamma.join(".git/symphony/tasks/snapshots");
    fs::write(&blocked, "").unwrap();
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let moved = |id: &str, project: &Path| {
        format!(
            "release: refresh: {id}: its board moved from schema 18 to 19 by {RELEASE}'s own \
             `tasks migrate`, its store snapshotted first to {}\n",
            snapshot(project, 19, RELEASE).display()
        )
    };
    assert!(
        out.contains(
            "release: refresh: gamma: red: the snapshot of its store failed, so its board was not \
             moved: "
        ) && out.contains(&moved("alpha", &alpha))
            && out.contains(&moved("beta", &beta)),
        "{out}"
    );
    assert_eq!(
        migrated(&fixture),
        format!(
            "tasks migrate in {}, 1 snapshots\ntasks migrate in {}, 2 snapshots\n",
            alpha.display(),
            beta.display()
        ),
        "each snapshot just before its own move"
    );
    assert_eq!(schemas(), [Some(19), Some(19), Some(18)]);
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    // gamma's failed move is its update's one task; alpha's and beta's wait for a server.
    assert!(
        out.contains(
            "release: refresh: updates: gamma@19 failed: the snapshot of its store failed"
        ) && out.contains("; filed tests/release-red-update-gamma-19 on this repository's board\n")
            && out.contains(
                "release: refresh: updates: alpha@19 waits: no server answers on this machine to \
                 write it\n"
            )
            && out.contains("release: refresh: updates: beta@19 waits: "),
        "{out}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-update-gamma-19"]
    );

    // Its snapshot taken, gamma's migrate fails: its snapshot kept, the boards moved stay so, and
    // its update fails again on the same task.
    fs::remove_file(&blocked).unwrap();
    fs::write(fixture.root.join("migrate-red-gamma"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    // current serves 19 already: the boards moved are not judged again, gamma's alone is.
    assert!(
        !out.contains("release: refresh: alpha: ")
            && out.contains(&format!(
                "release: refresh: gamma: red: {RELEASE}'s `tasks migrate` in {} failed (",
                gamma.display()
            ))
            && out.contains(&format!(
                "); its store as it was before is in {}, and an orchestrator's `just release \
             --refresh` retries\n",
                snapshot(&gamma, 19, RELEASE).display()
            ))
            && out.contains("release: refresh: updates: gamma@19 failed: "),
        "{out}"
    );
    assert_eq!(migrated(&fixture).lines().count(), 3);
    assert!(snapshot(&gamma, 19, RELEASE).is_file());
    assert_eq!(schemas(), [Some(19), Some(19), Some(18)]);
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/release-red-update-gamma-19"],
        "one task for the update, whatever its failures"
    );
    assert!(
        reds.values()
            .all(|red| symphony_board::red::open(&red.status))
    );

    // The retry moves gamma's board too.
    fs::remove_file(fixture.root.join("migrate-red-gamma")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 240.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("release: refresh: gamma: its board moved from schema 18 to 19 by "),
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

    // The release's server answers: every update served, and gamma's task closed, none filed again.
    release_server(&fixture, RELEASE, true);
    let (code, out, err) = fixture.release_with(NOW + 360.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let slug = "tests/release-red-update-gamma-19";
    assert!(
        out.contains(&format!(
            "release: refresh: updates: closed {slug} on this repository's board: "
        )),
        "{out}"
    );
    assert_eq!(fixture.reds()[slug].status, "cancelled");
    assert_eq!(fixture.reds().len(), 1, "no task filed again");
    let ledger = ledger(&fixture);
    let served: Vec<(&str, usize)> = (ledger.updates.iter())
        .filter(|update| update.served.is_some())
        .map(|update| (update.id.as_str(), update.failures.len()))
        .collect();
    assert_eq!(served, [("alpha@19", 0), ("beta@19", 0), ("gamma@19", 2)]);
}

#[test]
fn what_cannot_be_judged_is_left_out_and_each_board_that_lags_is_named_and_holds_no_other() {
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
    let (_alpha_board, _, alpha_db) = board_at(&alpha, 18);
    // A program of the project's own, where it wrote from: a path with nothing at it holds none.
    let gate = &fixture.root.join("builds-of/alpha/debug/gate-review");
    executable(gate, "#!/bin/sh\nexit 9\n");
    writes(&alpha_db, "gate-review", gate, 18);
    let _beta_board = board_at(&beta, 18);
    let (gamma_board, _, gamma_db) = board_at(&gamma, 18);
    aged(&gamma_db, 8.0 * DAY);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    for line in [
        "left out: a/b: its id names no folder of its own".to_string(),
        format!("left out: gone: its checkout {} is gone", gone.display()),
        format!("left out: plain: {} is not a git checkout", plain.display()),
        "fresh: its board is not made yet: nothing to move".to_string(),
        format!(
            "gamma: its board moved from schema 18 to 19 by {RELEASE}'s own `tasks migrate`, its \
             store snapshotted first to {}",
            snapshot(&gamma, 19, RELEASE).display()
        ),
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
    assert_eq!(gamma_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert_eq!(migrated(&fixture).lines().count(), 1);

    // A registry that cannot be read is the refresh's own red: nothing judged, current as it was.
    fs::write(machine(&fixture).join("projects.json"), "not a registry\n").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh red (0 s): the machine's registry cannot be read, so no board \
                 moved and current is as it was (it names {RELEASE}): "
        )),
        "{out}"
    );
    assert!(
        out.contains("release: filed tests/release-red-refresh"),
        "{out}"
    );
    assert!(fixture.reds()["tests/release-red-refresh"].red());
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
}

#[test]
fn a_project_naming_a_linked_plugin_holds_the_switch_the_release_installed_and_current_unpointed() {
    let (fixture, _) = refreshing();
    let shelf = a_project(&fixture, "shelf", &["shelf-plugin"]);
    registered(&fixture, &[("shelf", &shelf, &["shelf-plugin"])]);
    let (board, _, db) = board_at(&shelf, 18);
    aged(&db, 8.0 * DAY);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];
    let before = untouched(&shelf);
    let held = format!(
        "shelf names the linked plugin shelf-plugin, and a release's server serves a plugin only \
         from its process: a [plugins.process] command for it in {} lets the release serve every \
         project (docs/engineering/PLUGIN_PROTOCOL.md)",
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
fn a_failed_move_is_its_boards_red_alone_and_current_is_pointed_all_the_same() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    registered(&fixture, &[("alpha", &alpha, &[])]);
    let (board, running, db) = board_at(&alpha, 18);
    aged(&db, 8.0 * DAY);
    // The first release, at the board's schema: nothing to move, and current names it.
    let at_18 = builder(&fixture, 18);
    let extra = [(BUILDER, at_18.as_str())];
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

    // The next raises the schema, and its migrate fails: red, and current pointed all the same.
    fixture.land("crates/core/notes.txt", Some("b\n"), "a landing after it");
    let at_19 = builder(&fixture, 19);
    fs::write(fixture.root.join("migrate-red"), "").unwrap();
    let extra = [
        (BUILDER, at_19.as_str()),
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
            "release: refresh: {} names {NEXT} (it named {RELEASE}, which stays installed): a \
             project with no build of its own runs it\n",
            machine(&fixture).join("current").display()
        )),
        "{out}"
    );
    // The failed move is its update's one task, on this repository's board, and no red of the step.
    let slug = "tests/release-red-update-alpha-19";
    assert!(
        out.contains(&format!(
            "release: refresh: updates: alpha@19 failed: {NEXT}'s `tasks migrate` in {}",
            alpha.display()
        )) && out.contains(&format!("; filed {slug} on this repository's board\n")),
        "{out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(NEXT));
    assert_eq!(board.schema("tasks").unwrap(), Some(18));
    assert!(fs::read_to_string(&kept).unwrap().contains("sim/kept"));
    let reds = fixture.reds();
    assert_eq!(reds.keys().collect::<Vec<_>>(), [slug], "{out}");
    assert!(reds[slug].red() && reds[slug].priority == 0);
    let failures = |fixture: &Fixture| -> Vec<String> {
        (fixture.thread(slug).into_iter())
            .filter(|post| post.starts_with("Failure "))
            .collect()
    };
    assert_eq!(failures(&fixture).len(), 1);

    // Green again: moved, current as the failed run pointed it, the server waited for. What the
    // server's reload said before this run is quoted as said before it.
    fs::remove_file(fixture.root.join("migrate-red")).unwrap();
    let log = machine(&fixture).join("server.log");
    fs::write(
        &log,
        "[2026-06-01T00:00:00.000Z] · Symphony reload: current names release-0; handing over to \
         its server\n",
    )
    .unwrap();
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
             the release {RELEASE} after 1 s; its reload wrote no line in its log ({}) since \
             current moved, as one that waits to try a release again writes none; its last line \
             before: [2026-06-01T00:00:00.000Z] · Symphony reload: current names release-0; \
             handing over to its server. Its own reload",
            log.display()
        )),
        "{out}"
    );
    // Moved and pointed, but the server never came to write the board: the same update failed
    // again, a post on its task, which stays open.
    assert!(
        out.contains(&format!(
            "release: refresh: updates: alpha@19 failed: the machine's server, the release {RELEASE} \
             at schema 18, does not write its board at schema 19: "
        )) && out.contains(&format!("; heard on {slug}, this repository's board\n")),
        "{out}"
    );
    assert_eq!(failures(&fixture).len(), 2);
    assert!(symphony_board::red::open(&fixture.reds()[slug].status));
    assert_eq!(board.schema("tasks").unwrap(), Some(19));
    assert_eq!(
        board.running(),
        running,
        "the same database, never restarted"
    );

    // The server follows by its hand-off: the update is served, timed from that line, and closed.
    release_server(&fixture, RELEASE, true);
    let moved = (ledger(&fixture)
        .newest("alpha", 19)
        .unwrap()
        .reached
        .clone())
    .unwrap();
    assert!(moved.source.starts_with("this refresh's move"), "{moved:?}");
    fs::write(
        &log,
        format!(
            "[{}] · Symphony reload: replacement pid 4343 ready; draining pid 4242\n",
            utc(moved.at + 1.0)
        ),
    )
    .unwrap();
    let (code, out, err) = fixture.release_with(NOW + DAY + 240.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(
            "release: refresh: updates: alpha@19 served: the machine's server writes \
             alpha's board at schema 19 (at the board's schema; its project list says nothing of \
             writes)"
        ) && out.contains(&format!(
            "release: refresh: updates: closed {slug} on this repository's board: alpha@19\n"
        )),
        "{out}"
    );
    assert_eq!(fixture.reds()[slug].status, "cancelled");
    let ledger = ledger(&fixture);
    let update = ledger.newest("alpha", 19).unwrap();
    assert_eq!(update.failures.len(), 2);
    assert!(
        (update.served.as_ref().unwrap().source).starts_with("the server's hand-off"),
        "{update:?}"
    );
    assert_eq!(
        update.served_by.as_deref(),
        Some(NEXT),
        "the release whose server wrote it"
    );
    assert_eq!(
        update.release.as_deref(),
        Some(NEXT),
        "the release serving it, installed"
    );
}

#[test]
fn a_board_not_running_is_red_alone_and_one_from_before_the_writer_record_is_its_own() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    let old = a_project(&fixture, "old", &[]);
    registered(&fixture, &[("alpha", &alpha, &[]), ("old", &old, &[])]);
    // alpha's board is made, its database stopped; old's from schema 17, before writers.
    let common = alpha.join(".git");
    let (alpha_board, _, _) = board_at(&alpha, 18);
    alpha_board.postgres().stop().unwrap();
    let (old_board, _, _) = board_at(&old, 17);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: alpha: red: the board's PostgreSQL in {} is not running: nothing \
             is read or exported, and nothing is started: its schema and writers were not read, \
             and nothing was moved; once it runs (any `just tasks` in {} starts it), an \
             orchestrator's `just release --refresh` moves it\n",
            common.join(FOLDER).display(),
            alpha.display()
        )),
        "{out}"
    );
    assert_eq!(
        alpha_board.postgres().current(),
        Ok(None),
        "nothing started it"
    );
    assert!(!common.join(FOLDER).join("snapshots").exists());
    let own = format!(
        "release: refresh: old: the project's to finish: its board is at schema 17, from before \
         the board recorded who writes it (schema 18), so it is never moved here: `cd {} && just \
         tasks migrate`, its own, brings it up (`just symphony-cli release-notes` in the project \
         says what taking the release asks of it)\n",
        old.display()
    );
    assert!(out.contains(&own), "{out}");
    assert!(!out.contains("not pointed"), "{out}");
    assert_eq!(
        current(&fixture).as_deref(),
        Some(RELEASE),
        "neither holds current"
    );
    // Each failed, whether a server answers or not: alpha's database down and old's board its
    // project's, neither of which any release here writes, so both on this repository's board.
    let (alpha_red, old_red) = (
        "tests/release-red-update-alpha-19",
        "tests/release-red-update-old-19",
    );
    assert!(
        out.contains("release: refresh: updates: alpha@19 failed: the board's PostgreSQL in ")
            && out.contains(&format!("; filed {alpha_red} on this repository's board\n"))
            && out.contains("release: refresh: updates: old@19 failed: its board is at schema 17")
            && out.contains(&format!("; filed {old_red} on this repository's board\n")),
        "{out}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        [alpha_red, old_red]
    );
    assert!(
        fixture.reds()[alpha_red]
            .body
            .contains("its board's database is down"),
        "{}",
        fixture.reds()[alpha_red].body
    );

    // alpha gone from the registry: old's board, behind every release, holds nothing; alpha's
    // update ends with its task, and old's fails again on its own.
    registered(&fixture, &[("old", &old, &[])]);
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(&own), "{out}");
    assert!(
        out.contains(
            "release: refresh: updates: alpha@19 ended unserved: its project is no longer \
             registered on this machine\n"
        ) && out.contains(&format!(
            "updates: closed {alpha_red} on this repository's board"
        )) && out.contains(&format!("; heard on {old_red}, this repository's board\n")),
        "{out}"
    );
    assert_eq!(fixture.reds()[alpha_red].status, "cancelled");
    assert!(symphony_board::red::open(&fixture.reds()[old_red].status));
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE));
    assert_eq!(old_board.schema("tasks").unwrap(), Some(17));
    assert!(migrated(&fixture).is_empty());
}

/// A registered project whose board is still on SurrealDB, as an earlier release kept it, is red,
/// naming how it moves, and its home is left as it was: nothing is started, made, moved or
/// snapshotted for it, and `current` is pointed all the same.
#[test]
fn a_board_still_on_surrealdb_is_red_naming_how_it_moves_and_nothing_of_it_is_touched() {
    let (fixture, _) = refreshing();
    let alpha = a_project(&fixture, "alpha", &[]);
    registered(&fixture, &[("alpha", &alpha, &[])]);
    // Its home holds a SurrealDB store and names no engine, as a board from before the move does.
    let folder = alpha.join(".git").join(FOLDER);
    fs::create_dir_all(folder.join("data")).unwrap();
    fs::write(folder.join("data/CURRENT"), "MANIFEST-000001\n").unwrap();
    let before = Fixture::files(&folder);
    let builder = builder(&fixture, 19);
    let extra = [(BUILDER, builder.as_str())];

    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let said: Vec<&str> = (out.lines())
        .filter(|line| line.starts_with("release: refresh: alpha: "))
        .collect();
    assert!(
        said.len() == 1
            && said[0].starts_with("release: refresh: alpha: red: ")
            && said[0].contains(GONE),
        "{out}"
    );
    assert!(out.contains("release: refresh-alpha red ("), "{out}");
    // Read and nothing more: no PostgreSQL made or named for it, nothing moved or snapshotted.
    assert_eq!(Fixture::files(&folder), before, "{out}");
    assert!(migrated(&fixture).is_empty(), "no migrate ran");
    assert_eq!(
        current(&fixture).as_deref(),
        Some(RELEASE),
        "a board's red holds no current"
    );
    // Its update failed, filed on this repository's board: no release here writes that board.
    let slug = "tests/release-red-update-alpha-19";
    assert!(
        out.contains("release: refresh: updates: alpha@19 failed: ")
            && out.contains(&format!("; filed {slug} on this repository's board\n")),
        "{out}"
    );
    assert_eq!(fixture.reds().keys().collect::<Vec<_>>(), [slug], "{out}");
}

/// The scratch database `filed` on the board's server `running`, where the stand-in's own board
/// binary files what a refresh files on that project's board.
fn filed(running: &Running) -> symphony_board::Store {
    symphony_board::Store::opening_on(
        database(running, "filed"),
        Box::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64()
        }),
        Box::new(|_| false),
        "this-host",
        &symphony_board::Opening::write(),
    )
    .unwrap()
}

/// The texts on `slug`'s thread on `store`, oldest first.
fn posts(store: &symphony_board::Store, slug: &str) -> Vec<String> {
    (store.thread(slug).unwrap().into_iter())
        .map(|message| message.text)
        .collect()
}

/// `seconds` since the epoch as the server's log writes a time: `YYYY-MM-DDTHH:MM:SS.mmmZ`, in UTC.
fn utc(seconds: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a time since the epoch, in milliseconds"
    )]
    let millis = (seconds * 1000.0).floor() as i64;
    let (days, of_day) = (millis.div_euclid(86_400_000), millis.rem_euclid(86_400_000));
    // The civil date of a day since 1970-01-01 (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let of_era = z - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1000 % 60,
        of_day % 1000
    )
}

#[test]
fn a_time_in_the_servers_log_is_written_in_utc_to_the_millisecond() {
    assert_eq!(utc(0.0), "1970-01-01T00:00:00.000Z");
    assert_eq!(utc(951_825_600.25), "2000-02-29T12:00:00.250Z");
    assert_eq!(utc(1_795_089_079.778), "2026-11-19T11:51:19.778Z");
}

/// The machine's ledger of schema updates.
fn ledger(fixture: &Fixture) -> symphony::machine::updates::Ledger {
    symphony::machine::updates::Ledger::read(&machine(fixture)).unwrap()
}

/// Each update in the ledger, as `ID: served|open|left, failures N[, task SLUG on ID[ (closed)]]`.
fn updates(fixture: &Fixture) -> Vec<String> {
    (ledger(fixture).updates.iter())
        .map(|update| {
            let state = if update.served.is_some() {
                "served"
            } else if update.left.is_some() {
                "left"
            } else {
                "open"
            };
            let task = (update.task.as_ref()).map_or_else(String::new, |task| {
                let closed = if task.closed { " (closed)" } else { "" };
                format!(", task {} on {}{closed}", task.slug, task.project)
            });
            format!(
                "{}: {state}, failures {}{task}",
                update.id,
                update.failures.len()
            )
        })
        .collect()
}

/// This repository's board at schema 19, moved by main's own build ahead of every release: its
/// update waits while no release serves 19, and the release that does is pointed at though
/// alpha's writer is behind, beta moving and alpha left at 18. This repository's update is served,
/// timed from its board reaching 19 and from that release's install; alpha's fails: one red task
/// on alpha's own board, filed through the installed release of its schema; a retry posts its
/// second failure on that same task, and a later release at the same schema is pointed at all the
/// same, its third; the refresh that moves alpha once its writer is rebuilt closes it. Every update
/// served the first time files nothing, the ledger keeps each, and a refresh with nothing to do
/// changes nothing.
#[test]
fn a_board_left_behind_holds_no_current_and_its_failed_update_is_one_task_until_it_is_served() {
    let (fixture, _) = refreshing();
    // Main commits no schema source here, so any build makes its board, at 19 as main's would.
    fixture.land("crates/tasks/src/schema.rs", None, "no schema source");
    let alpha = a_project(&fixture, "alpha", &[]);
    let beta = a_project(&fixture, "beta", &[]);
    let main = fixture.main.clone();
    registered(
        &fixture,
        &[
            ("symphony", &main, &[]),
            ("alpha", &alpha, &[]),
            ("beta", &beta, &[]),
        ],
    );
    let (ours_board, _, _) = board_at(&main, 19);
    let (alpha_board, alpha_running, alpha_db) = board_at(&alpha, 18);
    let (beta_board, _, beta_db) = board_at(&beta, 18);
    aged(&beta_db, 8.0 * DAY);
    aged(&alpha_db, 8.0 * DAY);
    release_server(&fixture, "release-0", true);
    let builder = builder(&fixture, 18);
    let extra = [
        (BUILDER, builder.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", FOLLOWED),
    ];

    // The first release, at the projects' schema: theirs served at once, no task anywhere, and
    // this repository's waiting for a release serving 19.
    let (code, out, err) = fixture.release_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(current(&fixture).as_deref(), Some(RELEASE), "{out}");
    assert_eq!(
        updates(&fixture),
        [
            "symphony@19: open, failures 0",
            "alpha@18: served, failures 0",
            "beta@18: served, failures 0"
        ],
        "{out}"
    );
    assert!(
        out.contains(
            "release: refresh: updates: symphony@19 waits: no release serving schema 19 is \
             installed yet\n"
        ),
        "{out}"
    );
    assert!(!out.contains("release-red-update-"), "{out}");

    // Alpha's own build at 18 has just run, and is where it ran from.
    let own_build = fixture.root.join("builds-of/alpha/release/tasks");
    executable(&own_build, "#!/bin/sh\nexit 9\n");
    writes(&alpha_db, "tasks", &own_build, 18);
    fs::write(fixture.root.join("schema"), "19").unwrap();
    fixture.land("crates/core/next.txt", Some("b\n"), "the next release");
    let (code, out, err) = fixture.release_with(NOW + DAY, &["--check", CHECK], &extra);
    assert_eq!(
        code, 0,
        "a project left behind is its update's task, not a red: {out}{err}"
    );
    assert!(!out.contains("holds the release"), "{out}");
    assert_eq!(
        current(&fixture).as_deref(),
        Some(NEXT),
        "forward despite alpha: {out}"
    );
    assert_eq!(ours_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(beta_board.schema("tasks").unwrap(), Some(19));
    assert_eq!(
        alpha_board.schema("tasks").unwrap(),
        Some(18),
        "its writer holds alpha's board"
    );
    let slug = "tests/release-red-update-alpha-19";
    assert!(
        out.contains("release: refresh: updates: alpha@19 failed: its board stays at schema 18: ")
            && out.contains(&format!("; filed {slug} on alpha's board\n")),
        "{out}"
    );
    assert!(out.contains("updates: symphony@19 served"), "{out}");
    assert!(out.contains("updates: beta@19 served"), "{out}");
    let ours = ledger(&fixture).newest("symphony", 19).unwrap().clone();
    assert_eq!(ours.release.as_deref(), Some(NEXT), "{ours:?}");
    assert_eq!(ours.served_by.as_deref(), Some(NEXT), "{ours:?}");
    assert!(
        ours.total().is_some() && ours.after_available().is_some(),
        "{ours:?}"
    );
    assert!(ours.after_available() <= ours.total(), "{ours:?}");
    assert!(
        fixture.reds().is_empty(),
        "nothing on this repository's board: {out}"
    );
    let theirs = filed(&alpha_running);
    let task = theirs.get(slug).unwrap();
    assert_eq!(task.priority, 0);
    assert!(task.red(), "red work, staffed as a red is");
    assert!(symphony_board::red::open(&task.status), "{}", task.status);
    let failures: Vec<String> = (posts(&theirs, slug).into_iter())
        .filter(|post| post.starts_with("Failure "))
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].starts_with("Failure 1 of alpha@19 (run "),
        "{failures:?}"
    );

    // A retry finds it again: the same task, a second failure posted on it, and no other; current
    // stays where the refresh pointed it, and the retry says so.
    let (code, out, err) = fixture.release_with(NOW + DAY + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!("; heard on {slug}, alpha's board\n")),
        "{out}"
    );
    assert!(
        out.contains(&format!("names {NEXT} (as it did)"))
            && !out.contains("current is not pointed"),
        "{out}"
    );
    let failures: Vec<String> = (posts(&theirs, slug).into_iter())
        .filter(|post| post.starts_with("Failure "))
        .collect();
    assert_eq!(failures.len(), 2, "{failures:?}");
    assert!(
        failures[1].starts_with("Failure 2 of alpha@19 (run "),
        "{failures:?}"
    );
    let filed_there: Vec<String> = (theirs.tasks(None).unwrap().into_iter())
        .map(|task| task.slug)
        .collect();
    assert_eq!(filed_there, [slug], "never a second task for one update");

    // A later release at the same schema, alpha still behind: a board left behind holds neither
    // current nor another, so current is pointed at it, and alpha's one task hears a third failure.
    fixture.land(
        "crates/core/later.txt",
        Some("c\n"),
        "a later release at the same schema",
    );
    let (code, out, err) = fixture.release_with(NOW + 2.0 * DAY, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        current(&fixture).as_deref(),
        Some(LATER),
        "alpha left behind holds nothing: {out}"
    );
    assert!(
        out.contains(&format!(
            "names {LATER} (it named {NEXT}, which stays installed)"
        )) && !out.contains("current is not pointed"),
        "{out}"
    );
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(18), "{out}");
    assert!(
        out.contains(&format!("; heard on {slug}, alpha's board\n")),
        "{out}"
    );
    let failures: Vec<String> = (posts(&theirs, slug).into_iter())
        .filter(|post| post.starts_with("Failure "))
        .collect();
    assert_eq!(failures.len(), 3, "{failures:?}");
    let filed_there: Vec<String> = (theirs.tasks(None).unwrap().into_iter())
        .map(|task| task.slug)
        .collect();
    assert_eq!(filed_there, [slug], "nothing else filed there");
    assert!(fixture.reds().is_empty(), "nor here: {out}");

    // Alpha's writer rebuilt at 19: the next refresh moves its board, the update is served and its
    // task closed; the ledger keeps every update with its failures.
    Raw::of(&alpha_db).table("writer").delete().unwrap();
    writes(
        &alpha_db,
        "tasks",
        Path::new("/work/alpha/.target/release/tasks"),
        19,
    );
    let (code, out, err) = fixture.release_with(NOW + 2.0 * DAY + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(alpha_board.schema("tasks").unwrap(), Some(19), "{out}");
    assert!(
        snapshot(&alpha, 19, LATER).is_file(),
        "snapshotted just before its own move by the release current serves: {out}"
    );
    assert_eq!(current(&fixture).as_deref(), Some(LATER), "{out}");
    assert!(
        out.contains("updates: alpha@19 served")
            && out.contains(&format!(
                "updates: closed {slug} on alpha's board: alpha@19"
            )),
        "{out}"
    );
    assert_eq!(theirs.get(slug).unwrap().status, "cancelled");
    assert_eq!(
        updates(&fixture),
        [
            "symphony@19: served, failures 0",
            "alpha@18: served, failures 0",
            "beta@18: served, failures 0",
            &format!("alpha@19: served, failures 3, task {slug} on alpha (closed)"),
            "beta@19: served, failures 0",
        ]
    );
    // A refresh with nothing to do changes nothing.
    let before = ledger(&fixture);
    let (code, out, err) = fixture.release_with(NOW + 2.0 * DAY + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(ledger(&fixture), before, "{out}");
    // The status says them, the newest served first, each with where its times come from.
    let (code, status, err) = fixture.release_with(NOW + 2.0 * DAY + 180.0, &["--status"], &extra);
    assert_eq!(code, 0, "{status}{err}");
    assert!(
        status.contains("\nschema updates: 0 open, 5 served (ledger ")
            && status.contains("\n  served beta@19 ")
            && status.contains("\n  served alpha@19 ")
            && status.contains(&format!(
                "{NEXT} by {NEXT}'s manifest, written by its install"
            )),
        "{status}"
    );
}
