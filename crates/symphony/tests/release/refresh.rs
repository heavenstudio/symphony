//! A green release's refresh of the machine: each project the machine serves that builds on the
//! fixture's Symphony from git has its pin moved to the release on a branch of its own, through
//! its own commit gate and never pushed, its main fast-forwarded to it in its main checkout when
//! that checkout is ready for it, and its board's schema moved to the pin its main locks, its store
//! snapshotted first. A gate that refuses the move, a main checkout not ready or hooks that refuse
//! the fast-forward move nothing more and are the project's to finish, filed nowhere; the
//! refresh's own work that fails (a worktree it cannot make, a migrate that fails, a board whose
//! database is not running) is a red task until a refresh gets through.
//! Every project is a scratch repository, its board a scratch database of the test's own, and the
//! shared server a stand-in on a private port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use symphony_board::NewTask;
use symphony_board::live::seen;
use symphony_board::rpc::Client;
use symphony_board::server::{Home, Running};
use symphony_board::sys::{Command, output};

use super::common::older_board;
use super::consumers::{checkout, consumer, register, symphony_crate};
use super::{DAY, Fixture, NOW, git};

/// The release a run of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// The branch a project's pin moves on.
const BRANCH: &str = "symphony/release-2026-11-12";
/// How long a refresh waits for the shared server's reload, in seconds, in a test whose server has
/// reloaded by the time it looks: what a stall alone reaches, the backstop of a minute
/// ([`test_time::backstop`]).
const RELOADED: &str = "360";

/// The fixture with the refresh on and its Symphony a workspace of two packages pinning the
/// board's SurrealDB, pushed: the commit.
fn refreshing() -> (Fixture, String) {
    let fixture = Fixture::new();
    fixture.write("symphony.toml", &Fixture::project_file("refresh = true\n"));
    symphony_crate(&fixture, 42);
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/core\", \"crates/extra\"]\nresolver = \"2\"\n",
    );
    fixture.write(
        "crates/extra/Cargo.toml",
        "[package]\nname = \"fixture-extra\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    );
    fixture.write(
        "crates/extra/src/lib.rs",
        "pub fn extra() -> u32 {\n    7\n}\n",
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

/// Run `program ARGS` in `dir` with the fixture's Cargo home, which must succeed.
fn run_in(fixture: &Fixture, dir: &Path, program: &str, args: &[&str]) {
    let done = output(
        Command::new(program)
            .args(args)
            .current_dir(dir)
            .env("CARGO_HOME", fixture.root.join("cargo-home"))
            .env_remove("RUSTUP_TOOLCHAIN")
            .env_remove("CARGO_TARGET_DIR"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{program} {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The registered [`consumer`] taking both packages, its lock committed at the fixture's main as
/// pushed now, and a commit gate of its own (`hooks/pre-commit`, red while the fixture holds
/// `gate-red`, and staging a file of its own into the commit while it holds `gate-stages`)
/// logging each commit it gates to `gate.log`: its checkout.
fn locked_consumer(fixture: &Fixture) -> PathBuf {
    let root = consumer(fixture);
    let manifest = fs::read_to_string(root.join("host/Cargo.toml")).unwrap();
    let extra = format!(
        "fixture-extra = {{ git = \"file://{}\", branch = \"main\" }}\n",
        fixture.origin.display()
    );
    fs::write(root.join("host/Cargo.toml"), manifest + &extra).unwrap();
    let gate = format!(
        "#!/bin/sh\n\
         top=$(git rev-parse --show-toplevel)\n\
         echo \"$top: $(git diff --cached --name-only | tr '\\n' ' ')\" >> '{log}'\n\
         if [ -e '{red}' ]; then echo 'the gate is red' >&2; exit 1; fi\n\
         if [ -e '{stages}' ]; then echo 'the hook its own' > hooked.txt; git add hooked.txt; fi\n",
        log = fixture.root.join("gate.log").display(),
        red = fixture.root.join("gate-red").display(),
        stages = fixture.root.join("gate-stages").display(),
    );
    executable(&root.join("hooks/pre-commit"), &gate);
    run_in(fixture, &root, "cargo", &["generate-lockfile"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "its lock and its gate"]);
    git(&root, &["config", "core.hooksPath", "hooks"]);
    root
}

/// `text` at `path`, executable.
fn executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The commit the lock of `rev` in the checkout at `root` pins `package` at.
fn pinned(root: &Path, rev: &str, package: &str) -> String {
    let lock = git(root, &["show", &format!("{rev}:Cargo.lock")]);
    let at = lock
        .split("[[package]]")
        .find(|entry| entry.contains(&format!("name = \"{package}\"")))
        .and_then(|entry| entry.split_once('#'))
        .map(|(_, rest)| rest.split('"').next().unwrap().to_string());
    at.unwrap_or_else(|| panic!("{package} is not taken from git in {rev}:\n{lock}"))
}

/// The worktrees of the repository at `root`, by path.
fn worktrees(root: &Path) -> Vec<String> {
    (git(root, &["worktree", "list", "--porcelain"]).lines())
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(str::to_string)
        .collect()
}

/// The commits each gate run gated: one line each, the checkout it ran in and what it staged.
fn gated(fixture: &Fixture) -> Vec<String> {
    (fs::read_to_string(fixture.root.join("gate.log"))
        .unwrap_or_default()
        .lines())
    .map(str::to_string)
    .collect()
}

/// A stand-in for the machine's shared server, built from `root`, on a private port: the
/// machine folder records it, and it answers every `/api/front` until the test ends as process
/// `pid()` says then, or as a server not yet serving (503) while that says none.
fn shared_server_as(
    fixture: &Fixture,
    root: &Path,
    pid: impl Fn() -> Option<i64> + Send + 'static,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let root = root.to_string_lossy().into_owned();
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
            let Some(pid) = pid() else {
                let _ = write!(stream, "HTTP/1.0 503 Service Unavailable\r\n\r\n");
                continue;
            };
            let front = serde_json::json!({
                "pid": pid,
                "port": port,
                "root": root,
                "ready": true,
                "app": null,
                "tailnet": null,
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
        fixture.root.join("machine/server.json"),
        format!("{{\"port\": {port}}}\n"),
    )
    .unwrap();
    port
}

/// The same, always process 4242: a server whose reload never replaces it.
fn shared_server(fixture: &Fixture, root: &Path) -> u16 {
    shared_server_as(fixture, root, || Some(4242))
}

/// The same, built from the checkout `root`: process `before` (none: not serving) until that
/// checkout's `Cargo.lock` pins `release`, then 4343, as its reload's new build.
fn reloading_server(fixture: &Fixture, root: &Path, release: &str, before: Option<i64>) -> u16 {
    let lock = root.join("Cargo.lock");
    let release = release.to_string();
    shared_server_as(fixture, root, move || {
        let pins = fs::read_to_string(&lock).unwrap_or_default();
        if pins.contains(&format!("#{release}\"")) {
            Some(4343)
        } else {
            before
        }
    })
}

/// The fixture's Cargo home.
fn cargo_home(fixture: &Fixture) -> String {
    fixture
        .root
        .join("cargo-home")
        .to_string_lossy()
        .into_owned()
}

/// The consumer's board, its server started on the data in its common dir, made by the build of
/// schema `schema` with a task on it: a board an older build left, which its record says it was
/// seen at, never one put back from a newer schema. Its home and the server that runs it.
fn board_at(consumer: &Path, schema: i64) -> (Home, Running) {
    let common = consumer.join(".git");
    let board =
        Home::new(&common.join("symphony/tasks"), &super::board::surreal()).supervised(true);
    let running = board.ensure().unwrap();
    older_board(Client::new(&running.addr, "sf", "tasks"), schema, NOW)
        .create(&NewTask::new(
            "sim/kept",
            "What the snapshot keeps",
            "owner",
        ))
        .unwrap();
    assert_eq!(
        seen::read(&common).map(|seen| seen.version),
        Some(schema),
        "the consumer's live board, seen at the schema its build made it at"
    );
    (board, running)
}

#[test]
fn a_green_release_moves_each_projects_pin_and_main_through_its_own_gate_and_hooks() {
    let (fixture, first) = refreshing();
    let consumer = locked_consumer(&fixture);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), first);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let port = reloading_server(&fixture, &consumer, &release, Some(4242));
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", RELOADED),
    ];

    let (code, out, err) = fixture.release_with(NOW, &["--check", super::CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(fixture.remote_tags(), [RELEASE]);
    let made = git(&consumer, &["rev-parse", BRANCH]);
    let shown = consumer.display();
    for said in [
        format!(
            "consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}), made through its own \
             commit gate and not pushed\n",
            &made[..12]
        ),
        format!(
            "consumer: its main is fast-forwarded to {BRANCH} ({}) in its main checkout {shown}, \
             through its own hooks and not pushed: its own next push carries it\n",
            &made[..12]
        ),
        format!(
            "consumer: it keeps no board ({shown}/.git/symphony/tasks is not there): nothing to \
             move\n"
        ),
        format!(
            "the shared server (port {port}) is built from {shown}, whose main this refresh moved \
             to {RELEASE}: its own reload (apps/symphony-server/src/reload.rs) built it again, \
             and process 4343 answers in place of 4242\n"
        ),
    ] {
        assert!(
            out.contains(&format!("release: refresh: {said}")),
            "{said}\n{out}"
        );
    }
    assert!(out.contains("release: refresh-consumer green ("), "{out}");
    // The release was judged by its own check alone: the project was neither copied nor built,
    // and none of its tests ran.
    assert_eq!(
        super::consumers::checked(&fixture.runs().pop().unwrap()),
        ["verify"]
    );
    assert!(!out.contains("conciliat"), "{out}");
    assert!(!fixture.root.join("wt/release-conciliate").exists());
    assert!(!super::consumers::tested(&fixture).exists());

    // The branch is one commit on its old main: the manifest naming the release's tag in place of
    // main, and the lock binding it, both packages at the release.
    assert_eq!(git(&consumer, &["rev-parse", &format!("{BRANCH}^")]), head);
    assert_eq!(
        git(&consumer, &["diff", "--name-only", &head, BRANCH]),
        "Cargo.lock\nhost/Cargo.toml"
    );
    let origin = fixture.origin.display();
    let manifest = git(&consumer, &["show", &format!("{BRANCH}:host/Cargo.toml")]);
    for package in ["fixture-core", "fixture-extra"] {
        assert!(
            manifest.contains(&format!(
                "{package} = {{ git = \"file://{origin}\", tag = \"{RELEASE}\" }}"
            )),
            "{manifest}"
        );
    }
    assert!(!manifest.contains("branch"), "{manifest}");
    assert_eq!(pinned(&consumer, BRANCH, "fixture-core"), release);
    assert_eq!(pinned(&consumer, BRANCH, "fixture-extra"), release);
    let lock = git(&consumer, &["show", &format!("{BRANCH}:Cargo.lock")]);
    assert!(
        lock.contains(&format!("?tag={RELEASE}#{release}")),
        "{lock}"
    );
    let message = git(&consumer, &["log", "-1", "--format=%B", BRANCH]);
    assert!(
        message.starts_with(&format!("Take Symphony {RELEASE}\n\n")),
        "{message}"
    );
    assert!(message.contains("and nothing else."), "{message}");
    // Its gate ran once, in the refresh's worktree, on those two; that worktree is gone.
    let tree = fixture.root.join("wt/release-refresh/consumer");
    assert_eq!(
        gated(&fixture),
        [format!("{}: Cargo.lock host/Cargo.toml ", tree.display())]
    );
    assert_eq!(
        worktrees(&consumer),
        [consumer.to_string_lossy().into_owned()]
    );
    assert!(!tree.exists());
    // Its main is that commit, fast-forwarded: checked out clean, no merge made, nothing pushed.
    assert_eq!(git(&consumer, &["rev-parse", "main"]), made);
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), made);
    assert_eq!(git(&consumer, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert!(
        git(
            &consumer,
            &["status", "--porcelain", "--untracked-files=no"]
        )
        .is_empty()
    );
    assert_eq!(
        fs::read_to_string(consumer.join("Cargo.lock")).unwrap(),
        lock + "\n"
    );
    assert_eq!(
        git(
            &consumer,
            &["for-each-ref", "--format=%(refname)", "refs/heads"]
        ),
        format!("refs/heads/main\nrefs/heads/{BRANCH}")
    );
    assert!(git(&consumer, &["remote"]).is_empty());

    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["refreshed"]["release"], RELEASE, "{run}");
    assert_eq!(run["refreshed"]["steps"][0]["name"], "refresh-consumer");
    assert_eq!(run["refreshed"]["steps"][0]["exit_code"], 0);
    // Its hooks were asked first by a transaction aborted, then its main moved to that very
    // commit, never to a name.
    let log = fs::read_to_string(run["refreshed"]["steps"][0]["log"].as_str().unwrap()).unwrap();
    let asked = log.find("exec git update-ref --stdin").expect(&log);
    let merged = log
        .find(&format!(
            "$ git merge --ff-only --quiet {made}   (in {shown})"
        ))
        .expect(&log);
    assert!(asked < merged, "{log}");

    // Asked again, its main takes the release: nothing is made or moved twice.
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its main takes {RELEASE} already (its lock pins {})\n",
            &release[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: the shared server (process 4343, port {port}) is built from \
             {shown}, whose main took {RELEASE} before this refresh, as its own reload \
             (apps/symphony-server/src/reload.rs) follows that checkout's Cargo.lock\n"
        )),
        "{out}"
    );
    assert_eq!(gated(&fixture).len(), 1);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), made);
    assert_eq!(fixture.runs().pop().unwrap()["kind"], "refresh");
}

#[test]
fn the_status_says_the_landings_since_the_release_and_what_its_refresh_did() {
    let (fixture, _) = refreshing();
    let consumer = locked_consumer(&fixture);
    let port = shared_server(&fixture, &consumer);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let home = cargo_home(&fixture);
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--check", super::CHECK],
        &[
            ("CARGO_HOME", &home),
            ("SYMPHONY_RELEASE_RELOAD_SECONDS", "1"),
        ],
    );
    assert_eq!(code, 0, "{out}{err}");
    let run = fixture.runs().pop().unwrap();
    let made = git(&consumer, &["rev-parse", BRANCH]);
    let slugs = [
        "tools/a", "tools/b", "tools/c", "tools/d", "tools/e", "tools/f", "tools/g",
    ];
    let landed: Vec<String> = (slugs.iter())
        .map(|slug| super::cadence::landing(&fixture, slug))
        .collect();
    let last = &landed[landed.len() - 1];

    let status = fixture.run(NOW + 3600.0, &["--status"], 0);
    let from = status.find("landings: ").expect(&status);
    let shown = consumer.display();
    assert_eq!(
        &status[from..],
        format!(
            "landings: 7 on origin/main since {RELEASE} ({}); the last tools/g ({})\n\
             next run: after 13 more landings (7 of 20), or the scheduled run\n\
             last refresh: green, of {RELEASE} by the run {}, 2026-11-12 10:00 (1 h ago)\n  \
             consumer: its pin to {RELEASE} is on the branch {BRANCH} ({made}), made through its \
             own commit gate and not pushed\n  \
             consumer: its main is fast-forwarded to {BRANCH} ({made}) in its main checkout \
             {shown}, through its own hooks and not pushed: its own next push carries it\n  \
             consumer: it keeps no board ({shown}/.git/symphony/tasks is not there): nothing to \
             move\n  \
             the shared server (process 4242, port {port}) is built from {shown}, whose main this \
             refresh moved to {RELEASE}: no other process answered within 1 s, so it has not \
             taken it yet; its own reload (apps/symphony-server/src/reload.rs) builds and \
             installs a server from that checkout's new Cargo.lock and hands it its sockets, a \
             failed build leaving the old one serving and saying why on its notice (`just \
             symphony-status` says which process answers)\n",
            &release[..12],
            &last[..12],
            run["id"].as_str().unwrap(),
            made = &made[..12],
        )
    );
}

/// What `--status` says under a refresh one of whose steps is its project's to finish.
const BY_HAND: &str = "  by hand: a project whose own gate refused the move meets that itself; one \
                       whose hooks kept the move from its main lands its branch symphony/RELEASE \
                       through its own landing; a main checkout off its default branch or with \
                       tracked edits is put right by its owner; then an orchestrator's `just \
                       release --refresh` finishes the rest\n";

#[test]
fn a_commit_gate_that_refuses_the_move_is_the_projects_to_finish_and_filed_nowhere() {
    let (fixture, _) = refreshing();
    let consumer = locked_consumer(&fixture);
    // Released with no project registered (nothing to refresh), then registered.
    register(&fixture, &[]);
    fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains("release: refresh: the shared server: none answers on this machine"),
        "{out}"
    );
    register(&fixture, &[("consumer", &consumer)]);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    let before = checkout(&consumer);
    let home = cargo_home(&fixture);
    // The shared server is built from the Symphony's own main checkout.
    let port = shared_server(&fixture, &fixture.main);

    // The refresh's own work fails: a folder of someone's where its worktree goes. Red, filed.
    let tree = fixture.root.join("wt/release-refresh/consumer");
    fs::create_dir_all(&tree).unwrap();
    fs::write(tree.join("mine.txt"), "keep").unwrap();
    let (code, out, err) =
        fixture.release_with(NOW + 30.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("release: refresh-consumer red (")
            && out.contains("is in the way and is not the release run's worktree of"),
        "{out}"
    );
    assert!(
        out.contains("release: filed tests/release-red-refresh-consumer"),
        "{out}"
    );
    let red = &fixture.reds()["tests/release-red-refresh-consumer"];
    assert!(red.red());
    assert!(
        red.body
            .contains("Then an orchestrator reruns the refresh, `just release --refresh`"),
        "{}",
        red.body
    );
    assert_eq!(fs::read_to_string(tree.join("mine.txt")).unwrap(), "keep");
    fs::remove_dir_all(&tree).unwrap();

    // Its own commit gate refuses the move: no branch, its main as it was, and the step is the
    // project's to finish, green here, filed nowhere, closing nothing.
    fs::write(fixture.root.join("gate-red"), "").unwrap();
    let (code, out, err) =
        fixture.release_with(NOW + 60.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: the shared server (process 4242, port {port}) is built from {}, \
             this repository's main checkout, which its own reload follows\n",
            fixture.main.display()
        )),
        "{out}"
    );
    let said = format!(
        "release: refresh: consumer: the project's to finish: its commit gate refused the move to \
         {RELEASE} (`git commit --quiet --file "
    );
    assert!(
        out.contains(&said)
            && out.contains(
                "exited 1: the gate is red), so nothing of consumer moved: no branch was made, and \
                 its main is as it was; what its gate refuses (its last lines are in this step's \
                 log) is the project's own to meet, and once it passes an orchestrator's `just \
                 release --refresh` makes the move again\n"
            ),
        "{out}"
    );
    assert!(
        out.contains("release: refresh-consumer the project's to finish ("),
        "{out}"
    );
    assert!(
        !out.contains("release: filed") && !out.contains("release: closed"),
        "{out}"
    );
    assert!(
        fixture.reds()["tests/release-red-refresh-consumer"].red(),
        "the open red of its own work stays open"
    );
    assert!(git(&consumer, &["branch", "--list", BRANCH]).is_empty());
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), head);
    assert_eq!(checkout(&consumer), before);
    assert_eq!(
        worktrees(&consumer),
        [consumer.to_string_lossy().into_owned()]
    );
    assert!(!tree.exists());
    // The run's record says which, its log keeps the gate's last lines, and so does the status.
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["result"], "green", "{run}");
    assert_eq!(
        run["refreshed"]["theirs"],
        serde_json::json!(["refresh-consumer"])
    );
    assert_eq!(run["refreshed"]["steps"][0]["exit_code"], 0, "{run}");
    let said = run["refreshed"]["said"].as_array().unwrap();
    assert!(
        said.iter().any(|line| line
            .as_str()
            .unwrap()
            .starts_with("consumer: the project's to finish: its commit gate refused")),
        "{run}"
    );
    let log = fs::read_to_string(run["refreshed"]["steps"][0]["log"].as_str().unwrap()).unwrap();
    assert!(
        log.contains("the gate is red\n")
            && log.ends_with("the project's to finish, not red here\n"),
        "{log}"
    );
    let status = fixture.run(NOW + 90.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "last refresh: green, of {RELEASE} by the run {}, ",
            run["id"].as_str().unwrap()
        )) && status.contains("\n  consumer: the project's to finish: its commit gate refused")
            && status.contains(&format!(
                "  refresh-consumer the project's to finish, filed nowhere here; log {}\n{BY_HAND}",
                run["refreshed"]["steps"][0]["log"].as_str().unwrap()
            )),
        "{status}"
    );

    // Green again, a retry makes the branch, moves its main to it and closes its own work's red.
    fs::remove_file(fixture.root.join("gate-red")).unwrap();
    let (code, out, err) =
        fixture.release_with(NOW + 120.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("release: closed tests/release-red-refresh-consumer"),
        "{out}"
    );
    assert_eq!(
        fixture.reds()["tests/release-red-refresh-consumer"].status,
        "cancelled"
    );
    assert_eq!(git(&consumer, &["rev-parse", &format!("{BRANCH}^")]), head);
    assert_eq!(
        git(&consumer, &["rev-parse", "main"]),
        git(&consumer, &["rev-parse", BRANCH])
    );
    assert_eq!(gated(&fixture).len(), 2);
    let status = fixture.run(NOW + 150.0, &["--status"], 0);
    assert!(!status.contains("by hand"), "{status}");
}

#[test]
fn a_main_checkout_not_ready_keeps_its_main_and_its_board_catches_up_with_the_pin_it_holds() {
    let (fixture, first) = refreshing();
    let consumer = locked_consumer(&fixture);
    stand_in_tasks(&fixture, &consumer);
    // A guard of its own (as a project's landing guard does) refuses any move of its main while
    // the fixture holds `ff-red`; it is not committed, so the refresh's worktree runs none.
    executable(
        &consumer.join("hooks/reference-transaction"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = prepared ] || exit 0\n[ -e '{}' ] || exit 0\n\
             while read -r old new ref; do\n  if [ \"$ref\" = refs/heads/main ]; then \
             echo 'main moves only by its own landing' >&2; exit 1; fi\ndone\n",
            fixture.root.join("ff-red").display()
        ),
    );
    let (board, running) = board_at(&consumer, 5);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let head = git(&consumer, &["rev-parse", "main"]);
    assert_eq!(pinned(&consumer, "main", "fixture-core"), first);
    git(&consumer, &["checkout", "-q", "-b", "elsewhere"]);
    let home = cargo_home(&fixture);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let shown = consumer.display();
    let snapshot = consumer.join(format!(
        ".git/symphony/tasks/snapshots/before-schema-16-{RELEASE}.surql"
    ));

    let not_red = |out: &str| {
        assert!(
            out.contains("release: refresh-consumer the project's to finish (")
                && !out.contains("release: filed")
                && !out.contains("release: closed"),
            "{out}"
        );
    };
    let left_for_it = format!(
        "the branch {BRANCH} is left for it, and once its owner puts that right an orchestrator's \
         `just release --refresh` fast-forwards it and moves its board"
    );

    // Off its default branch while its board's move fails: the branch is made and its main stays,
    // which is the project's to finish, but the failed migrate is the refresh's own red, filed.
    fs::write(fixture.root.join("migrate-red"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW, &["--check", super::CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(fixture.remote_tags(), [RELEASE]);
    let made = git(&consumer, &["rev-parse", BRANCH]);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    let off = format!(
        "release: refresh: consumer: the project's to finish: its main checkout {shown} is on the \
         branch elsewhere, not its main, so its main was not moved: {left_for_it}\n"
    );
    assert!(out.contains(&off), "{off}\n{out}");
    assert!(
        out.contains("release: refresh-consumer red (")
            && out.contains("`just tasks migrate` in ")
            && out.contains("release: filed tests/release-red-refresh-consumer"),
        "{out}"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(5));
    assert!(fixture.reds()["tests/release-red-refresh-consumer"].red());
    fs::remove_file(fixture.root.join("migrate-red")).unwrap();

    // Its migrate green, its main checkout still off its branch: its board is moved to the schema
    // of the pin its main locks, whatever release is newest, and the step is the project's to
    // finish, which neither files nor closes a red.
    let (code, out, err) = fixture.release_with(NOW + 15.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(&off), "{off}\n{out}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its board moved from schema 5 to 16, its store \
             snapshotted first to {}\n",
            snapshot.display()
        )),
        "{out}"
    );
    not_red(&out);
    assert_eq!(board.schema("tasks").unwrap(), Some(16));
    assert!(fixture.reds()["tests/release-red-refresh-consumer"].red());
    let status = fixture.run(NOW + 20.0, &["--status"], 0);
    assert!(
        status.contains(&format!("\n  {}", &off["release: refresh: ".len()..]))
            && status
                .contains("  refresh-consumer the project's to finish, filed nowhere here; log ")
            && status.ends_with(BY_HAND),
        "{status}"
    );

    // On no branch at all: not moved either.
    git(&consumer, &["checkout", "-q", "--detach", "main"]);
    let (code, out, err) = fixture.release_with(NOW + 30.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: the project's to finish: its main checkout {shown} is on \
             a detached HEAD, not its main, so its main was not moved: {left_for_it}\n"
        )),
        "{out}"
    );
    not_red(&out);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);

    // On its main with a tracked edit: still not moved.
    git(&consumer, &["checkout", "-q", "main"]);
    let manifest = consumer.join("host/Cargo.toml");
    let kept = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{kept}# an edit of its owner's\n")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}) \
             already\n",
            &made[..12]
        )) && out.contains(&format!(
            "release: refresh: consumer: the project's to finish: its main checkout {shown} has \
             tracked edits (host/Cargo.toml), so its main was not moved: {left_for_it}\n"
        )) && out.contains("consumer: its board is at schema 16, its pin's: nothing to move"),
        "{out}"
    );
    not_red(&out);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    fs::write(&manifest, kept).unwrap();

    // Its hooks refuse the fast-forward: asked first, by a transaction aborted, they refuse it, so
    // nothing moves, nothing passes them by, and not a file of its checkout is written (the shared
    // server may be built from it). The branch waits for its own landing.
    fs::write(fixture.root.join("ff-red"), "").unwrap();
    let written = |path: &str| {
        fs::metadata(consumer.join(path))
            .unwrap()
            .modified()
            .unwrap()
    };
    let (lock_at, manifest_at) = (written("Cargo.lock"), written("host/Cargo.toml"));
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: the project's to finish: git refuses the move of its main \
             to {} when asked first, in a ref transaction then aborted (",
            &made[..12]
        )) && out.contains("main moves only by its own landing")
            && out.contains(&format!(
                "so its main was not moved and nothing was written in its checkout: the branch \
                 {BRANCH} is left for its own landing, and once that lands it an orchestrator's \
                 `just release --refresh` moves its board\n"
            )),
        "{out}"
    );
    not_red(&out);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(
        git(
            &consumer,
            &["status", "--porcelain", "--untracked-files=no"]
        ),
        "",
        "its checkout holds its main as it was"
    );
    assert_eq!(
        (written("Cargo.lock"), written("host/Cargo.toml")),
        (lock_at, manifest_at)
    );
    assert_eq!(pinned(&consumer, "main", "fixture-core"), first);
    fs::remove_file(fixture.root.join("ff-red")).unwrap();

    // Its main moved on meanwhile: the branch is made again on its tip, and its main
    // fast-forwarded to it; the red of the failed migrate closes.
    fs::write(consumer.join("notes.txt"), "its own work\n").unwrap();
    git(&consumer, &["add", "notes.txt"]);
    git(&consumer, &["commit", "-q", "-m", "its own work"]);
    let moved_on = git(&consumer, &["rev-parse", "main"]);
    let (code, out, err) = fixture.release_with(NOW + 180.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let again = git(&consumer, &["rev-parse", BRANCH]);
    assert_ne!(again, made);
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}), \
             made again on its tip through its own commit gate and not pushed\n",
            &again[..12]
        )) && out.contains(&format!(
            "release: refresh: consumer: its main is fast-forwarded to {BRANCH} ({}) in its main \
             checkout {shown}",
            &again[..12]
        )) && out.contains("release: closed tests/release-red-refresh-consumer"),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), again);
    assert_eq!(
        git(&consumer, &["rev-parse", &format!("{BRANCH}^")]),
        moved_on
    );
    assert_eq!(pinned(&consumer, "main", "fixture-core"), release);
    // Its gate ran for each move made (each staging its lock), beside its owner's own commits.
    let gated = gated(&fixture);
    let moves: Vec<&String> = gated
        .iter()
        .filter(|line| line.contains("Cargo.lock"))
        .collect();
    assert_eq!(moves.len(), 2, "{gated:?}");
    assert_eq!(gated.len(), 4, "{gated:?}");
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
}

/// The consumer's guard of its own on its main (a `reference-transaction` hook, not committed, so
/// the refresh's worktree runs none): while the fixture holds `late` it lets the refresh's first
/// ask through and then refuses the move itself, running `late` first in the checkout, as an owner
/// acting while git moved it.
fn refusing_late(fixture: &Fixture, consumer: &Path) -> PathBuf {
    let late = fixture.root.join("late.sh");
    let asked = fixture.root.join("asked");
    executable(
        &consumer.join("hooks/reference-transaction"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = prepared ] || exit 0\n[ -e '{late}' ] || exit 0\n\
             while read -r old new ref; do\n  [ \"$ref\" = refs/heads/main ] || continue\n  \
             if [ -e '{asked}' ]; then rm -f '{asked}'; sh '{late}'; \
             echo 'main moves only by its own landing' >&2; exit 1; fi\n  touch '{asked}'\ndone\n",
            late = late.display(),
            asked = asked.display()
        ),
    );
    late
}

#[test]
fn a_move_refused_after_its_ask_puts_back_only_what_it_wrote_and_names_what_it_cannot() {
    let (fixture, first) = refreshing();
    let consumer = locked_consumer(&fixture);
    let late = refusing_late(&fixture, &consumer);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    fs::write(consumer.join("justfile"), "# its justfile\n").unwrap();
    git(&consumer, &["add", "justfile"]);
    git(&consumer, &["commit", "-q", "-m", "its justfile"]);
    let head = git(&consumer, &["rev-parse", "main"]);
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", RELOADED),
    ];
    let shown = consumer.display();
    let status = || {
        git(
            &consumer,
            &["status", "--porcelain", "--untracked-files=no"],
        )
    };
    let at_main = |path: &str| {
        let file = fs::read_to_string(consumer.join(path)).unwrap();
        file.trim_end() == git(&consumer, &["show", &format!("main:{path}")])
            && git(&consumer, &["diff", "--cached", "--name-only", "--", path]).is_empty()
    };
    let refused = |made: &str| {
        format!(
            "release: refresh: consumer: the project's to finish: git refused the fast-forward of \
             its main to {}, through its hooks or its checkout (`git merge --ff-only --quiet \
             {made}` exited ",
            &made[..12]
        )
    };

    // Its owner stages a file of its own while git moves its main, which its guard then refuses:
    // the move's two files are put back to its main, and the owner's file stays staged.
    fs::write(
        &late,
        "echo '# the owner' >> justfile && git add justfile\n",
    )
    .unwrap();
    let (code, out, err) = fixture.release_with(NOW, &["--check", super::CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let made = git(&consumer, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
    assert!(
        out.contains(&refused(&made))
            && out.contains("main moves only by its own landing")
            && out.contains(&format!(
                "; its main is still at {}; git had written the move's Cargo.lock and \
                 host/Cargo.toml into its checkout first, now put back as its main has them: the \
                 branch {BRANCH} is left for its own landing",
                &head[..12]
            ))
            && out.contains("release: refresh-consumer the project's to finish ("),
        "{out}"
    );
    assert!(fixture.reds().is_empty(), "filed nowhere");
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(status(), "M  justfile", "the owner's staged file alone");
    assert!(at_main("Cargo.lock") && at_main("host/Cargo.toml"));
    git(
        &consumer,
        &["restore", "--staged", "--worktree", "--", "justfile"],
    );

    // An edit of its owner's, not staged: kept, and the move's files put back.
    fs::write(&late, "echo '# the owner' >> justfile\n").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&refused(&made))
            && out.contains("now put back as its main has them: the branch"),
        "{out}"
    );
    assert_eq!(status(), " M justfile", "the owner's edit alone");
    assert!(at_main("Cargo.lock") && at_main("host/Cargo.toml"));
    git(&consumer, &["restore", "--worktree", "--", "justfile"]);

    // Its owner edits the lock git wrote: that file is left as it is and named, with the command
    // that puts it back, never one that throws away anything else; the other is put back.
    fs::write(&late, "echo '# the owner' >> Cargo.lock\n").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let put_back = format!("git -C {shown} restore --staged --worktree -- Cargo.lock");
    assert!(
        out.contains(&format!(
            "; git had written the move's host/Cargo.toml into its checkout first, now put back \
             as its main has it; Cargo.lock is not just as the move wrote it, so it is left as it \
             is: `{put_back}` puts it back as its main has it, losing what changed there: the \
             branch {BRANCH} is left for its own landing"
        )),
        "{out}"
    );
    assert!(!out.contains("reset --hard"), "{out}");
    assert_eq!(status(), "MM Cargo.lock");
    assert!(at_main("host/Cargo.toml"));
    git(
        &consumer,
        &["restore", "--staged", "--worktree", "--", "Cargo.lock"],
    );
    assert_eq!(status(), "");
    assert!(at_main("Cargo.lock"));

    // Git refuses before it writes a file (its index is locked by another of its owner's
    // commands): nothing of the move is there to put back, and none is named.
    let locked = consumer.join(".git/index.lock");
    fs::write(&locked, "").unwrap();
    fs::write(&late, "true\n").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 180.0, &["--refresh"], &extra);
    fs::remove_file(&locked).unwrap();
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&refused(&made))
            && out.contains(&format!(
                "; its main is still at {}: the branch {BRANCH} is left for its own landing",
                &head[..12]
            )),
        "{out}"
    );
    assert_eq!(status(), "");
    assert_eq!(pinned(&consumer, "main", "fixture-core"), first);

    // Its guard lets it through: its main takes that very commit. No server was built from it
    // before the move, so the one that answers after is not taken as its reload's new build.
    fs::remove_file(&late).unwrap();
    let port = reloading_server(&fixture, &consumer, &release, None);
    let (code, out, err) = fixture.release_with(NOW + 240.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(git(&consumer, &["rev-parse", "main"]), made);
    assert_eq!(status(), "");
    assert!(
        out.contains(&format!(
            "release: refresh: the shared server (process 4343, port {port}) is built from \
             {shown}, whose main this refresh moved to {RELEASE}: none built from it answered \
             before the move, so whether this one is built after it cannot be told; its own \
             reload"
        )),
        "{out}"
    );
    // Each refusal was the project's to finish: nothing was filed, so nothing closes.
    assert!(
        !out.contains("release: closed") && fixture.reds().is_empty(),
        "{out}"
    );
}

#[test]
fn a_branch_holding_more_than_the_move_never_reaches_main() {
    let (fixture, _) = refreshing();
    let consumer = locked_consumer(&fixture);
    // Its own guard, refusing every move of its main while the fixture holds `ff-red`.
    executable(
        &consumer.join("hooks/reference-transaction"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = prepared ] || exit 0\n[ -e '{}' ] || exit 0\n\
             while read -r old new ref; do\n  if [ \"$ref\" = refs/heads/main ]; then \
             echo 'main moves only by its own landing' >&2; exit 1; fi\ndone\n",
            fixture.root.join("ff-red").display()
        ),
    );
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let head = git(&consumer, &["rev-parse", "main"]);
    let home = cargo_home(&fixture);
    let extra = [("CARGO_HOME", home.as_str())];
    let shown = consumer.display();
    let branch = format!("refs/heads/{BRANCH}");
    let left = format!(
        "so its main was not moved to it: the branch is left as it is, for its own landing, or \
         for `git -C {shown} branch -D {BRANCH}` and an orchestrator's `just release --refresh` \
         to make it again"
    );

    // Its commit gate stages a file of its own into the pin's commit: the commit, read as git
    // holds it after the hook, is more than the move, and its main is not moved to it.
    fs::write(fixture.root.join("gate-stages"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW, &["--check", super::CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let hooked = git(&consumer, &["rev-parse", &branch]);
    assert!(
        out.contains(&format!(
            "the commit {} on {BRANCH} changes more than its manifests and its lock (hooked.txt), \
             {left}",
            &hooked[..12]
        )),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    fs::remove_file(fixture.root.join("gate-stages")).unwrap();

    // A branch that is more than the move is the refresh's own red, filed.
    assert!(fixture.reds()["tests/release-red-refresh-consumer"].red());

    // Its owner drops that branch; and a tag of the same name, which git would take for the
    // name, points at its old main. Its guard refuses this time: the branch made again is left,
    // the project's to finish.
    git(&consumer, &["branch", "-D", BRANCH]);
    git(&consumer, &["tag", BRANCH, &head]);
    fs::write(fixture.root.join("ff-red"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("when asked first, in a ref transaction then aborted")
            && out.contains("release: refresh-consumer the project's to finish ("),
        "{out}"
    );
    let made = git(&consumer, &["rev-parse", &branch]);
    assert_ne!(made, hooked);
    fs::remove_file(fixture.root.join("ff-red")).unwrap();

    // Its owner adds a commit of its own to the branch left for its landing: the next refresh
    // never takes it onto its main, and leaves the branch as it is.
    git(&consumer, &["switch", "-q", BRANCH]);
    fs::write(consumer.join("notes.txt"), "the owner's, for its landing\n").unwrap();
    git(&consumer, &["add", "notes.txt"]);
    git(&consumer, &["commit", "-q", "-m", "the owner's own"]);
    git(&consumer, &["switch", "-q", "main"]);
    let owners = git(&consumer, &["rev-parse", &branch]);
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "{BRANCH} ({}) is 2 commits past its main ({}), not the refresh's one commit on it, \
             {left}",
            &owners[..12],
            &head[..12]
        )),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(git(&consumer, &["rev-parse", &branch]), owners);

    // Once its owner drops it, the move is made again and its main takes that very commit, not
    // the tag of its name.
    git(&consumer, &["branch", "-D", BRANCH]);
    let (code, out, err) = fixture.release_with(NOW + 180.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let again = git(&consumer, &["rev-parse", &branch]);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), again);
    assert_eq!(git(&consumer, &["rev-parse", &format!("{again}^")]), head);
    assert_eq!(
        git(&consumer, &["rev-parse", &format!("refs/tags/{BRANCH}")]),
        head
    );
    assert_eq!(pinned(&consumer, "main", "fixture-core"), release);
    assert!(
        git(
            &consumer,
            &["status", "--porcelain", "--untracked-files=no"]
        )
        .is_empty()
    );
}

/// The consumer's `just tasks`, a stand-in: what it was asked and the snapshots there when it ran
/// go to `migrated.log`, and a migrate moves its board's schema to 16, its pin's, or fails while
/// the fixture holds `migrate-red`.
fn stand_in_tasks(fixture: &Fixture, consumer: &Path) {
    let log = fixture.root.join("migrated.log");
    let script = format!(
        "#!/bin/sh\n\
         echo \"tasks $* in $PWD\" >> '{log}'\n\
         ls .git/symphony/tasks/snapshots >> '{log}' 2>&1\n\
         if [ -e '{red}' ]; then echo 'the migrate is red' >&2; exit 1; fi\n\
         addr=$(sed -n 's/.*\"addr\": *\"\\([^\"]*\\)\".*/\\1/p' .git/symphony/tasks/server.json)\n\
         curl -sf -X POST \"http://$addr/sql\" -H 'Accept: application/json' \
         --data 'USE NS sf; USE DB tasks; UPSERT meta:schema SET version = 16;' >/dev/null\n",
        log = log.display(),
        red = fixture.root.join("migrate-red").display()
    );
    executable(&consumer.join("hooks/tasks.sh"), &script);
    fs::write(
        consumer.join("justfile"),
        "tasks *ARGS:\n    sh hooks/tasks.sh {{ARGS}}\n",
    )
    .unwrap();
    git(consumer, &["add", "-A"]);
    git(consumer, &["commit", "-q", "-m", "its board's commands"]);
}

#[test]
fn a_project_whose_main_takes_the_release_has_its_board_moved_after_a_snapshot() {
    let (fixture, release) = refreshing();
    register(&fixture, &[]);
    fixture.night(NOW, 0);
    assert_eq!(fixture.tagged(RELEASE), release);
    // Its main took the release by its own landing: its lock pins the release already.
    let consumer = locked_consumer(&fixture);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), release);
    stand_in_tasks(&fixture, &consumer);
    let port = shared_server(&fixture, &consumer);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];

    // Its board's database is not running: nothing starts it, and nothing is moved.
    let common = consumer.join(".git");
    fs::create_dir_all(common.join("symphony/tasks")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its main takes {RELEASE} already (its lock pins {})\n",
            &release[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "the board's database in {} is not running (no address is recorded): nothing is read \
             or exported, and nothing is started: its schema was not read, snapshotted or moved; \
             once it runs (any `just tasks` in {} starts it), an orchestrator's `just release \
             --refresh` moves it",
            common.join("symphony/tasks").display(),
            consumer.display()
        )),
        "{out}"
    );
    assert!(
        !fixture.root.join("migrated.log").exists(),
        "no migrate ran"
    );
    assert!(
        !common.join("symphony/tasks/server.json").exists(),
        "nothing started it"
    );

    // Running, at schema 5 with a task on it.
    let (board, running) = board_at(&consumer, 5);
    let snapshot = common.join(format!(
        "symphony/tasks/snapshots/before-schema-16-{RELEASE}.surql"
    ));

    // A record whose version cannot be read fails closed: the move might replace that database.
    let record = common.join("symphony/tasks/server.json");
    let recorded = fs::read_to_string(&record).unwrap();
    let mut blank: serde_json::Value = serde_json::from_str(&recorded).unwrap();
    blank["version"] = serde_json::json!("");
    fs::write(&record, blank.to_string()).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 75.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "the SurrealDB its board's database runs cannot be read (its record in {} names no \
             version), so the move might replace that database and the refresh stops none: \
             nothing was moved; `cd {} && just tasks migrate` moves it when its owner says",
            common.join("symphony/tasks").display(),
            consumer.display()
        )),
        "{out}"
    );
    assert!(!snapshot.exists() && !fixture.root.join("migrated.log").exists());
    fs::write(&record, recorded).unwrap();

    // Its own migrate fails: red, its schema where it was and the snapshot kept.
    fs::write(fixture.root.join("migrate-red"), "").unwrap();
    let (code, out, err) = fixture.release_with(NOW + 90.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let root = consumer.display();
    assert!(
        out.contains(&format!(
            "`just tasks migrate` in {root} failed (`just tasks migrate` exited 1: "
        )) && out.contains("the migrate is red")
            && out.contains(&format!(
                "); its store as it was before is in {}, and `cd {root} && just tasks migrate` \
                 retries",
                snapshot.display()
            )),
        "{out}"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(5));
    assert!(
        fs::read_to_string(&snapshot).unwrap().contains("sim/kept"),
        "the store as it was is kept"
    );
    assert!(fixture.reds()["tests/release-red-refresh-consumer"].red());

    // Its migrate green again: snapshotted, then moved by its own command, and the red closed.
    fs::remove_file(fixture.root.join("migrate-red")).unwrap();
    let (code, out, err) = fixture.release_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its board moved from schema 5 to 16, its store \
             snapshotted first to {}\n",
            snapshot.display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: the shared server (process 4242, port {port}) is built from {root}, \
             whose main took {RELEASE} before this refresh, as its own reload \
             (apps/symphony-server/src/reload.rs) follows that checkout's Cargo.lock\n"
        )),
        "{out}"
    );
    assert!(
        out.contains("release: closed tests/release-red-refresh-consumer"),
        "{out}"
    );
    let kept = fs::read_to_string(&snapshot).unwrap();
    assert!(kept.contains("sim/kept"), "{kept}");
    let migrated = fs::read_to_string(fixture.root.join("migrated.log")).unwrap();
    let ran = format!("tasks migrate in {root}\nbefore-schema-16-{RELEASE}.surql\n");
    assert_eq!(
        migrated,
        ran.repeat(2),
        "the snapshot was there before each move"
    );
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );

    // At its pin's schema, a later refresh moves nothing.
    let (code, out, err) = fixture.release_with(NOW + 180.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(
            "release: refresh: consumer: its board is at schema 16, its pin's: nothing to move"
        ),
        "{out}"
    );
}

#[test]
fn a_project_pinned_past_the_release_takes_it_already_and_nothing_is_made() {
    let (fixture, release) = refreshing();
    register(&fixture, &[]);
    fixture.night(NOW, 0);
    assert_eq!(fixture.tagged(RELEASE), release);
    // Its pin was moved by hand to a commit of main past the release.
    let ahead = fixture.land(
        "crates/core/notes.txt",
        Some("b\n"),
        "a landing after the release",
    );
    let consumer = locked_consumer(&fixture);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), ahead);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    let before = checkout(&consumer);
    let gates = gated(&fixture).len();
    let home = cargo_home(&fixture);

    let (code, out, err) =
        fixture.release_with(NOW + 60.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its main takes {RELEASE} already (its lock pins {})\n",
            &ahead[..12]
        )),
        "{out}"
    );
    assert!(out.contains("release: refresh-consumer green ("), "{out}");
    // Never moved back: no branch, no commit gate, its main and its files as they were.
    assert!(git(&consumer, &["branch", "--list", BRANCH]).is_empty());
    assert_eq!(gated(&fixture).len(), gates);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(pinned(&consumer, "main", "fixture-core"), ahead);
    assert_eq!(checkout(&consumer), before);
}

#[test]
fn a_pin_beside_the_release_is_left_with_its_files_and_board_until_a_release_descends_from_it() {
    let (fixture, _) = refreshing();
    // A red's fix branched from main's tip, and a landing on main meanwhile.
    git(&fixture.main, &["switch", "-q", "-c", "fix"]);
    fixture.write("crates/core/fix.txt", "the fix\n");
    let fix = fixture.commit("the red's fix");
    git(&fixture.main, &["switch", "-q", "main"]);
    let meanwhile = fixture.land("crates/core/notes.txt", Some("b\n"), "a landing meanwhile");
    // Its pin was moved by hand to that landing, which the fix lacks; then the fix lands on main.
    let consumer = locked_consumer(&fixture);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), meanwhile);
    stand_in_tasks(&fixture, &consumer);
    git(
        &fixture.main,
        &["merge", "-q", "--no-ff", "-m", "Merge branch 'fix'", "fix"],
    );
    let merged = git(&fixture.main, &["rev-parse", "HEAD"]);
    git(&fixture.main, &["push", "-q", "origin", "main"]);

    // The fix itself is released, off main's first-parent line; then the consumer is registered.
    register(&fixture, &[]);
    fixture.run(NOW, &["--check", super::CHECK, "--commit", &fix], 0);
    assert_eq!(fixture.tagged(RELEASE), fix);
    register(&fixture, &[("consumer", &consumer)]);
    let (board, running) = board_at(&consumer, 5);
    let port = shared_server(&fixture, &consumer);
    let head = git(&consumer, &["rev-parse", "main"]);
    let before = checkout(&consumer);
    let gates = gated(&fixture).len();
    let home = cargo_home(&fixture);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
        ("SYMPHONY_RELEASE_RELOAD_SECONDS", "1"),
    ];
    let shown = consumer.display();
    let (m, f) = (&meanwhile[..12], &fix[..12]);
    let left = format!(
        "consumer: its main's lock pins the Symphony at {m}, neither ahead of {RELEASE} ({f}) nor \
         behind it: {RELEASE} does not descend from {m}, so a move to it would drop what {m} \
         holds; its pin, its files and its board are left as they are, and the next release that \
         descends from {m} moves it"
    );

    // Neither ahead nor behind: left, and said with both commits, green.
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!("release: refresh: {left}\n")),
        "{left}\n{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: the shared server (process 4242, port {port}) is built from \
             {shown}: it does not take {RELEASE}, as consumer's main pins the Symphony beside it, \
             which this refresh left as it is (its step says why)\n"
        )),
        "{out}"
    );
    assert!(out.contains("release: refresh-consumer green ("), "{out}");
    assert!(!out.contains("release: filed"), "{out}");
    assert!(fixture.reds().is_empty());
    // Its files: no branch, no worktree, no commit gate run, its main and checkout as they were.
    assert!(git(&consumer, &["branch", "--list", BRANCH]).is_empty());
    assert_eq!(
        worktrees(&consumer),
        [consumer.to_string_lossy().into_owned()]
    );
    assert!(!fixture.root.join("wt/release-refresh/consumer").exists());
    assert_eq!(gated(&fixture).len(), gates);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(pinned(&consumer, "main", "fixture-core"), meanwhile);
    assert_eq!(checkout(&consumer), before);
    // Its board: neither snapshotted nor migrated, though its pin's schema is past it.
    assert_eq!(board.schema("tasks").unwrap(), Some(5));
    assert!(!consumer.join(".git/symphony/tasks/snapshots").exists());
    assert!(
        !fixture.root.join("migrated.log").exists(),
        "no migrate ran"
    );
    assert_eq!(board.recorded(), Some(running.clone()));

    // The run's record and the status say it too.
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["refreshed"]["release"], RELEASE, "{run}");
    assert_eq!(run["refreshed"]["steps"][0]["exit_code"], 0, "{run}");
    let said = run["refreshed"]["said"].as_array().unwrap();
    assert!(said.iter().any(|line| *line == left.as_str()), "{run}");
    let status = fixture.run(NOW + 90.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "last refresh: green, of {RELEASE} by the run {}, ",
            run["id"].as_str().unwrap()
        )) && status.contains(&format!("\n  {left}\n")),
        "{status}"
    );

    // The next release descends from its pin: that one moves it forward, and its board with it.
    register(&fixture, &[]);
    fixture.night(NOW + DAY, 0);
    let next = "release-2026-11-13";
    assert_eq!(fixture.tagged(next), merged);
    register(&fixture, &[("consumer", &consumer)]);
    let (code, out, err) = fixture.release_with(NOW + DAY + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let made = git(&consumer, &["rev-parse", &format!("symphony/{next}")]);
    assert!(
        out.contains(&format!(
            "release: refresh: consumer: its main is fast-forwarded to symphony/{next} ({}) in its \
             main checkout {shown}",
            &made[..12]
        )),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), made);
    assert_eq!(git(&consumer, &["rev-parse", &format!("{made}^")]), head);
    assert_eq!(pinned(&consumer, "main", "fixture-core"), merged);
    assert_eq!(board.schema("tasks").unwrap(), Some(16));
    assert!(
        consumer
            .join(format!(
                ".git/symphony/tasks/snapshots/before-schema-16-{next}.surql"
            ))
            .is_file()
    );
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
}

#[test]
fn a_board_whose_move_could_replace_its_database_is_left_running_and_unmoved() {
    let (fixture, _) = refreshing();
    // The first release names no SurrealDB its board's source pins.
    let first = fixture.land("crates/tasks/src/server.rs", None, "no pinned SurrealDB");
    register(&fixture, &[]);
    fixture.night(NOW, 0);
    assert_eq!(fixture.tagged(RELEASE), first);
    let consumer = locked_consumer(&fixture);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), first);
    stand_in_tasks(&fixture, &consumer);
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_SURREAL", surreal.as_str()),
    ];
    let (board, running) = board_at(&consumer, 5);
    let by_hand = format!(
        "nothing was moved; `cd {} && just tasks migrate` moves it when its owner says",
        consumer.display()
    );

    // A pin whose SurrealDB cannot be read fails closed.
    let (code, out, err) = fixture.release_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "the SurrealDB its pin {} pins cannot be read (crates/tasks/src/server.rs there names \
             no SURREAL_VERSION), so the move might replace its board's database and the refresh \
             stops none: {by_hand}",
            &first[..12]
        )),
        "{out}"
    );

    // The next release pins a SurrealDB newer than the one the board runs: its main takes it, and
    // its board stays where it was.
    let newer = fixture.land(
        "crates/tasks/src/server.rs",
        Some("pub const SURREAL_VERSION: &str = \"99.0.0\";\n"),
        "a newer SurrealDB",
    );
    register(&fixture, &[]);
    fixture.night(NOW + DAY, 0);
    assert_eq!(fixture.tagged("release-2026-11-13"), newer);
    register(&fixture, &[("consumer", &consumer)]);
    let (code, out, err) = fixture.release_with(NOW + DAY + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(
        git(&consumer, &["rev-parse", "main"]),
        git(&consumer, &["rev-parse", "symphony/release-2026-11-13"])
    );
    assert!(
        out.contains(&format!(
            "its board's database runs SurrealDB {} and its pin {} pins 99.0.0: moving its schema \
             would replace that server, and the refresh stops no board's database, so {by_hand}",
            running.version,
            &newer[..12],
        )),
        "{out}"
    );
    assert!(
        !fixture.root.join("migrated.log").exists(),
        "no migrate ran"
    );
    assert!(
        !consumer.join(".git/symphony/tasks/snapshots").exists(),
        "nothing was snapshotted"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(5));
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
}
