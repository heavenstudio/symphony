//! A green release's refresh of the machine: each project the machine serves that builds on the
//! fixture's Symphony from git has its pin moved to the release on a branch of its own, through
//! its own commit gate and never pushed, its main fast-forwarded to it in its main checkout when
//! that checkout is ready for it, and its board's schema moved to the pin its main locks, its store
//! snapshotted first; a red gate, a main checkout not ready, hooks that refuse, or a board whose
//! database is not running moves nothing more and is a red task until a refresh gets through.
//! Every project is a scratch repository, its board a scratch database of the test's own, and the
//! shared server a stand-in on a private port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use symphony_board::rpc::Client;
use symphony_board::server::{Home, Running};
use symphony_board::sys::{Command, output};
use symphony_board::{NewTask, Store};

use super::conciliation::{checkout, consumer, register, symphony_crate};
use super::{DAY, Fixture, NOW, git};

/// The release a night of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// The branch a project's pin moves on.
const BRANCH: &str = "symphony/release-2026-11-12";

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

/// The conciliation's consumer taking both packages, its lock committed at the fixture's main as
/// pushed now, and a commit gate of its own (`hooks/pre-commit`, red while the fixture holds
/// `gate-red`) logging each commit it gates to `gate.log`: its checkout.
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
         if [ -e '{red}' ]; then echo 'the gate is red' >&2; exit 1; fi\n",
        log = fixture.root.join("gate.log").display(),
        red = fixture.root.join("gate-red").display(),
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
/// `pid()` says then.
fn shared_server_as(fixture: &Fixture, root: &Path, pid: impl Fn() -> i64 + Send + 'static) -> u16 {
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
            let front = serde_json::json!({
                "pid": pid(),
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
    shared_server_as(fixture, root, || 4242)
}

/// The same, built from the checkout `root`: process 4242 until that checkout's `Cargo.lock` pins
/// `release`, then 4343, as its reload's new build.
fn reloading_server(fixture: &Fixture, root: &Path, release: &str) -> u16 {
    let lock = root.join("Cargo.lock");
    let release = release.to_string();
    shared_server_as(fixture, root, move || {
        let pins = fs::read_to_string(&lock).unwrap_or_default();
        if pins.contains(&format!("#{release}\"")) {
            4343
        } else {
            4242
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

/// The consumer's board, started as its scratch database and made, then put at schema `schema`
/// with a task on it: its home and the server that runs it.
fn board_at(consumer: &Path, schema: i64) -> (Home, Running) {
    let common = consumer.join(".git");
    let board = Home::new(&common.join("sf-tasks"), &super::board::surreal()).supervised(true);
    let running = board.ensure().unwrap();
    let store = Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap();
    store
        .create(&NewTask::new(
            "sim/kept",
            "What the snapshot keeps",
            "owner",
        ))
        .unwrap();
    Client::new(&running.addr, "sf", "tasks")
        .query(
            &format!("UPSERT meta:schema SET version = {schema} RETURN NONE;"),
            serde_json::json!({}),
        )
        .unwrap();
    (board, running)
}

#[test]
fn a_green_release_moves_each_projects_pin_and_main_through_its_own_gate_and_hooks() {
    let (fixture, first) = refreshing();
    let consumer = locked_consumer(&fixture);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), first);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let port = reloading_server(&fixture, &consumer, &release);
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SYMPHONY_NIGHTLY_RELOAD_SECONDS", "60"),
    ];

    let (code, out, err) = fixture.nightly_with(NOW, &["--check", super::CHECK], &extra);
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
             through its own hooks and not pushed\n",
            &made[..12]
        ),
        format!(
            "consumer: it keeps no board ({shown}/.git/sf-tasks is not there): nothing to move\n"
        ),
        format!(
            "the shared server (port {port}) is built from {shown}, whose main this refresh moved \
             to {RELEASE}: its own reload (apps/symphony-server/src/reload.rs) built it again, \
             and process 4343 answers in place of 4242\n"
        ),
    ] {
        assert!(
            out.contains(&format!("nightly: refresh: {said}")),
            "{said}\n{out}"
        );
    }
    assert!(out.contains("nightly: refresh-consumer green ("), "{out}");

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
    let tree = fixture.root.join("wt/nightly-refresh/consumer");
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

    // Asked again, its main takes the release: nothing is made or moved twice.
    let (code, out, err) = fixture.nightly_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its main takes {RELEASE} already (its lock pins {})\n",
            &release[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "nightly: refresh: the shared server (process 4343, port {port}) is built from \
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
    let (code, out, err) = fixture.nightly_with(
        NOW,
        &["--check", super::CHECK],
        &[
            ("CARGO_HOME", &home),
            ("SYMPHONY_NIGHTLY_RELOAD_SECONDS", "1"),
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
             next run: after 13 more landings (7 of 20), or the night's run\n\
             last refresh: green, of {RELEASE} by the run {}, 2026-11-12 10:00 (1 h ago)\n  \
             consumer: its pin to {RELEASE} is on the branch {BRANCH} ({made}), made through its \
             own commit gate and not pushed\n  \
             consumer: its main is fast-forwarded to {BRANCH} ({made}) in its main checkout \
             {shown}, through its own hooks and not pushed\n  \
             consumer: it keeps no board ({shown}/.git/sf-tasks is not there): nothing to move\n  \
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

#[test]
fn a_red_commit_gate_moves_nothing_and_is_a_red_task_until_a_refresh_gets_through() {
    let (fixture, _) = refreshing();
    let consumer = locked_consumer(&fixture);
    // Released with no project registered (nothing conciliated), then registered.
    register(&fixture, &[]);
    fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains("nightly: refresh: the shared server: none answers on this machine"),
        "{out}"
    );
    register(&fixture, &[("consumer", &consumer)]);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    let before = checkout(&consumer);
    fs::write(fixture.root.join("gate-red"), "").unwrap();
    let home = cargo_home(&fixture);
    // The shared server is built from the Symphony's own main checkout.
    let port = shared_server(&fixture, &fixture.main);

    let (code, out, err) =
        fixture.nightly_with(NOW + 60.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: refresh: the shared server (process 4242, port {port}) is built from {}, \
             this repository's main checkout, which its own reload follows\n",
            fixture.main.display()
        )),
        "{out}"
    );
    assert!(
        out.contains("nightly: refresh-consumer red (")
            && out.contains(&format!(
                "its commit gate refused the move to {RELEASE} (`git commit --quiet --file "
            ))
            && out.contains(
                "exited 1: the gate is red), so nothing of consumer moved: no branch was made, and \
             its main is as it was"
            ),
        "{out}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-refresh-consumer"),
        "{out}"
    );
    let red = &fixture.reds()["tests/nightly-red-refresh-consumer"];
    assert!(red.red());
    assert!(red.body.contains("the gate is red"), "{}", red.body);
    assert!(
        red.body
            .contains("Then an orchestrator reruns the refresh, `just nightly --refresh`"),
        "{}",
        red.body
    );
    assert!(git(&consumer, &["branch", "--list", BRANCH]).is_empty());
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), head);
    assert_eq!(checkout(&consumer), before);
    assert_eq!(
        worktrees(&consumer),
        [consumer.to_string_lossy().into_owned()]
    );
    assert!(!fixture.root.join("wt/nightly-refresh/consumer").exists());
    let status = fixture.run(NOW + 90.0, &["--status"], 0);
    assert!(
        status.contains(
            "  by hand: a project whose own gate or hooks refuse the move lands its branch \
             symphony/RELEASE through its own landing; a main checkout off its default branch or \
             with tracked edits is put right by its owner; then an orchestrator's `just nightly \
             --refresh` finishes the rest\n"
        ),
        "{status}"
    );

    // Green again, a retry makes the branch, moves its main to it and closes the red.
    fs::remove_file(fixture.root.join("gate-red")).unwrap();
    let (code, out, err) =
        fixture.nightly_with(NOW + 120.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("nightly: closed tests/nightly-red-refresh-consumer"),
        "{out}"
    );
    assert_eq!(
        fixture.reds()["tests/nightly-red-refresh-consumer"].status,
        "cancelled"
    );
    assert_eq!(git(&consumer, &["rev-parse", &format!("{BRANCH}^")]), head);
    assert_eq!(
        git(&consumer, &["rev-parse", "main"]),
        git(&consumer, &["rev-parse", BRANCH])
    );
    assert_eq!(gated(&fixture).len(), 2);
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
        ("SF_SURREAL", surreal.as_str()),
    ];
    let shown = consumer.display();
    let snapshot = consumer.join(format!(
        ".git/sf-tasks/snapshots/before-schema-16-{RELEASE}.surql"
    ));

    // Off its default branch: the branch is made, its main stays, and its board is moved to the
    // schema of the pin its main locks, whatever release is newest.
    let (code, out, err) = fixture.nightly_with(NOW, &["--check", super::CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert_eq!(fixture.remote_tags(), [RELEASE]);
    let made = git(&consumer, &["rev-parse", BRANCH]);
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert!(
        out.contains(&format!(
            "its main checkout {shown} is on the branch elsewhere, not its main, so its main was \
             not moved: the branch {BRANCH} is left for it, and once that is put right an \
             orchestrator's `just nightly --refresh` fast-forwards it and moves its board"
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its board moved from schema 5 to 16, its store \
             snapshotted first to {}\n",
            snapshot.display()
        )),
        "{out}"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(16));
    assert!(fixture.reds()["tests/nightly-red-refresh-consumer"].red());

    // On its main with a tracked edit: still not moved.
    git(&consumer, &["checkout", "-q", "main"]);
    let manifest = consumer.join("host/Cargo.toml");
    let kept = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{kept}# an edit of its owner's\n")).unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}) \
             already\n",
            &made[..12]
        )) && out.contains(&format!(
            "its main checkout {shown} has tracked edits (host/Cargo.toml), so its main was not \
             moved"
        )) && out.contains("consumer: its board is at schema 16, its pin's: nothing to move"),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    fs::write(&manifest, kept).unwrap();

    // Its hooks refuse the fast-forward: nothing moves, and nothing passes them by. The files git
    // checked out before its hooks refused are put back, so its checkout holds its main as it was.
    fs::write(fixture.root.join("ff-red"), "").unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "git refused the fast-forward of its main to {BRANCH}, through its hooks or its \
             checkout (`git merge --ff-only --quiet {BRANCH}` exited 1"
        )) && out.contains("main moves only by its own landing")
            && out.contains(
                "so nothing was moved (the files git had checked out first are put back): the \
                 branch is left for its own landing"
            ),
        "{out}"
    );
    assert_eq!(git(&consumer, &["rev-parse", "main"]), head);
    assert_eq!(
        git(
            &consumer,
            &["status", "--porcelain", "--untracked-files=no"]
        ),
        "",
        "its checkout holds its main as it was"
    );
    assert_eq!(pinned(&consumer, "main", "fixture-core"), first);
    fs::remove_file(fixture.root.join("ff-red")).unwrap();

    // Its main moved on meanwhile: the branch is made again on its tip, and its main
    // fast-forwarded to it; the red closes.
    fs::write(consumer.join("notes.txt"), "its own work\n").unwrap();
    git(&consumer, &["add", "notes.txt"]);
    git(&consumer, &["commit", "-q", "-m", "its own work"]);
    let moved_on = git(&consumer, &["rev-parse", "main"]);
    let (code, out, err) = fixture.nightly_with(NOW + 180.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let again = git(&consumer, &["rev-parse", BRANCH]);
    assert_ne!(again, made);
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}), \
             made again on its tip through its own commit gate and not pushed\n",
            &again[..12]
        )) && out.contains(&format!(
            "nightly: refresh: consumer: its main is fast-forwarded to {BRANCH} ({}) in its main \
             checkout {shown}",
            &again[..12]
        )) && out.contains("nightly: closed tests/nightly-red-refresh-consumer"),
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

/// The consumer's `just tasks`, a stand-in: what it was asked and the snapshots there when it ran
/// go to `migrated.log`, and a migrate moves its board's schema to 16, its pin's, or fails while
/// the fixture holds `migrate-red`.
fn stand_in_tasks(fixture: &Fixture, consumer: &Path) {
    let log = fixture.root.join("migrated.log");
    let script = format!(
        "#!/bin/sh\n\
         echo \"tasks $* in $PWD\" >> '{log}'\n\
         ls .git/sf-tasks/snapshots >> '{log}' 2>&1\n\
         if [ -e '{red}' ]; then echo 'the migrate is red' >&2; exit 1; fi\n\
         addr=$(sed -n 's/.*\"addr\": *\"\\([^\"]*\\)\".*/\\1/p' .git/sf-tasks/server.json)\n\
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
        ("SF_SURREAL", surreal.as_str()),
    ];

    // Its board's database is not running: nothing starts it, and nothing is moved.
    let common = consumer.join(".git");
    fs::create_dir_all(common.join("sf-tasks")).unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its main takes {RELEASE} already (its lock pins {})\n",
            &release[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "the board's database in {} is not running (no address is recorded): nothing is read \
             or exported, and nothing is started: its schema was not read, snapshotted or moved; \
             once it runs (any `just tasks` in {} starts it), an orchestrator's `just nightly \
             --refresh` moves it",
            common.join("sf-tasks").display(),
            consumer.display()
        )),
        "{out}"
    );
    assert!(
        !fixture.root.join("migrated.log").exists(),
        "no migrate ran"
    );
    assert!(
        !common.join("sf-tasks/server.json").exists(),
        "nothing started it"
    );

    // Running, at schema 5 with a task on it.
    let (board, running) = board_at(&consumer, 5);
    let snapshot = common.join(format!(
        "sf-tasks/snapshots/before-schema-16-{RELEASE}.surql"
    ));

    // A record whose version cannot be read fails closed: the move might replace that database.
    let record = common.join("sf-tasks/server.json");
    let recorded = fs::read_to_string(&record).unwrap();
    let mut blank: serde_json::Value = serde_json::from_str(&recorded).unwrap();
    blank["version"] = serde_json::json!("");
    fs::write(&record, blank.to_string()).unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 75.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "the SurrealDB its board's database runs cannot be read (its record in {} names no \
             version), so the move might replace that database and the refresh stops none: \
             nothing was moved; `cd {} && just tasks migrate` moves it when its owner says",
            common.join("sf-tasks").display(),
            consumer.display()
        )),
        "{out}"
    );
    assert!(!snapshot.exists() && !fixture.root.join("migrated.log").exists());
    fs::write(&record, recorded).unwrap();

    // Its own migrate fails: red, its schema where it was and the snapshot kept.
    fs::write(fixture.root.join("migrate-red"), "").unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 90.0, &["--refresh"], &extra);
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
    assert!(fixture.reds()["tests/nightly-red-refresh-consumer"].red());

    // Its migrate green again: snapshotted, then moved by its own command, and the red closed.
    fs::remove_file(fixture.root.join("migrate-red")).unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its board moved from schema 5 to 16, its store \
             snapshotted first to {}\n",
            snapshot.display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "nightly: refresh: the shared server (process 4242, port {port}) is built from {root}, \
             whose main took {RELEASE} before this refresh, as its own reload \
             (apps/symphony-server/src/reload.rs) follows that checkout's Cargo.lock\n"
        )),
        "{out}"
    );
    assert!(
        out.contains("nightly: closed tests/nightly-red-refresh-consumer"),
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
    let (code, out, err) = fixture.nightly_with(NOW + 180.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(
            "nightly: refresh: consumer: its board is at schema 16, its pin's: nothing to move"
        ),
        "{out}"
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
        ("SF_SURREAL", surreal.as_str()),
    ];
    let (board, running) = board_at(&consumer, 5);
    let by_hand = format!(
        "nothing was moved; `cd {} && just tasks migrate` moves it when its owner says",
        consumer.display()
    );

    // A pin whose SurrealDB cannot be read fails closed.
    let (code, out, err) = fixture.nightly_with(NOW + 60.0, &["--refresh"], &extra);
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
    let (code, out, err) = fixture.nightly_with(NOW + DAY + 60.0, &["--refresh"], &extra);
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
        !consumer.join(".git/sf-tasks/snapshots").exists(),
        "nothing was snapshotted"
    );
    assert_eq!(board.schema("tasks").unwrap(), Some(5));
    assert_eq!(
        board.recorded(),
        Some(running),
        "the same database, never restarted"
    );
}
