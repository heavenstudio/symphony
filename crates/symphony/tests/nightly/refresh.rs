//! A green release's refresh of the machine: each project the machine serves that builds on the
//! fixture's Symphony from git has its pin moved to the release on a branch of its own, through
//! its own commit gate and never pushed, and its board's schema moved once its main takes the
//! release, its store snapshotted first; a red gate or a board whose database is not running moves
//! nothing and is a red task. Every project is a scratch repository, its board a scratch database
//! of the test's own, and the shared server a stand-in on a private port.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::sys::{Command, output};
use symphony_board::{NewTask, Store};

use super::conciliation::{checkout, consumer, register, symphony_crate};
use super::{Fixture, NOW, git};

/// The release a night of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// The branch a project's pin moves on.
const BRANCH: &str = "symphony/release-2026-11-12";

/// The fixture with the refresh on and its Symphony a workspace of two packages, pushed: the
/// commit.
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
    let hook = root.join("hooks/pre-commit");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, gate).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    run_in(fixture, &root, "cargo", &["generate-lockfile"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "its lock and its gate"]);
    git(&root, &["config", "core.hooksPath", "hooks"]);
    root
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
/// machine folder records it, and it answers every `/api/front` until the test ends.
fn shared_server(fixture: &Fixture, root: &Path) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let front = serde_json::json!({
        "pid": 4242,
        "port": port,
        "root": root.to_string_lossy(),
        "ready": true,
        "app": null,
        "tailnet": null,
    })
    .to_string();
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

/// The fixture's run environment for the refresh: its Cargo home, and the `surreal` a snapshot
/// takes.
fn cargo_home(fixture: &Fixture) -> String {
    fixture
        .root
        .join("cargo-home")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_green_release_moves_each_projects_pin_on_a_branch_of_its_own_through_its_own_gate() {
    let (fixture, first) = refreshing();
    let consumer = locked_consumer(&fixture);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    assert_eq!(pinned(&consumer, "HEAD", "fixture-core"), first);
    let before = checkout(&consumer);
    let port = shared_server(&fixture, &consumer);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let home = cargo_home(&fixture);

    let (code, out, err) =
        fixture.nightly_with(NOW, &["--check", super::CHECK], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(fixture.remote_tags(), [RELEASE]);
    let made = git(&consumer, &["rev-parse", BRANCH]);
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}), \
             made through its own commit gate and not pushed; its own landing lands it\n",
            &made[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its board moves once its main takes {RELEASE}\n"
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "nightly: refresh: the shared server (process 4242, port {port}) is built from {}: \
             it takes {RELEASE} by its own reload once consumer's main lands its pin\n",
            consumer.display()
        )),
        "{out}"
    );
    assert!(out.contains("nightly: refresh-consumer green ("), "{out}");

    // The branch is one commit on its main: the manifest naming the release's tag in place of
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
    // Its main, its checkout and its other branches are as they were; nothing is pushed.
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), head);
    assert_eq!(checkout(&consumer), before);
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
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "last refresh: green, of {RELEASE} by the run {}, 2026-11-12 10:00 (1 min ago)\n  \
             consumer: its pin to {RELEASE} is on the branch {BRANCH}",
            run["id"].as_str().unwrap()
        )),
        "{status}"
    );

    // Asked again, the branch is there: nothing is made twice, and the gate does not run again.
    let out = fixture.run(NOW + 120.0, &["--refresh"], 0);
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}) \
             already, for its own landing\n",
            &made[..12]
        )),
        "{out}"
    );
    assert_eq!(gated(&fixture).len(), 1);
    assert_eq!(fixture.runs().pop().unwrap()["kind"], "refresh");
}

#[test]
fn the_status_says_the_landings_since_the_release_and_what_its_refresh_did() {
    let (fixture, _) = refreshing();
    let consumer = locked_consumer(&fixture);
    let port = shared_server(&fixture, &consumer);
    let release = fixture.land("crates/core/notes.txt", Some("b\n"), "a change on main");
    let home = cargo_home(&fixture);
    let (code, out, err) =
        fixture.nightly_with(NOW, &["--check", super::CHECK], &[("CARGO_HOME", &home)]);
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
    assert_eq!(
        &status[from..],
        format!(
            "landings: 7 on origin/main since {RELEASE} ({}); the last tools/g ({})\n\
             next run: after 13 more landings (7 of 20), or the night's run\n\
             last refresh: green, of {RELEASE} by the run {}, 2026-11-12 10:00 (1 h ago)\n  \
             consumer: its pin to {RELEASE} is on the branch {BRANCH} ({}), made through its own \
             commit gate and not pushed; its own landing lands it\n  \
             consumer: its board moves once its main takes {RELEASE}\n  \
             the shared server (process 4242, port {port}) is built from {}: it takes {RELEASE} \
             by its own reload once consumer's main lands its pin\n",
            &release[..12],
            &last[..12],
            run["id"].as_str().unwrap(),
            &made[..12],
            consumer.display()
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

    let (code, out, err) =
        fixture.nightly_with(NOW + 60.0, &["--refresh"], &[("CARGO_HOME", &home)]);
    assert_eq!(code, 1, "{out}{err}");
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
        red.body.contains("`just nightly --refresh` retries it"),
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

    // Green again, a retry makes the branch and closes the red.
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
    assert_eq!(gated(&fixture).len(), 2);
}

/// The consumer's `just tasks`, a stand-in: what it was asked and the snapshots there when it ran
/// go to `migrated.log`, and a migrate moves its board's schema to 16, its pin's.
fn stand_in_tasks(fixture: &Fixture, consumer: &Path) {
    let log = fixture.root.join("migrated.log");
    let script = format!(
        "#!/bin/sh\n\
         echo \"tasks $* in $PWD\" >> '{log}'\n\
         ls .git/sf-tasks/snapshots >> '{log}' 2>&1\n\
         addr=$(sed -n 's/.*\"addr\": *\"\\([^\"]*\\)\".*/\\1/p' .git/sf-tasks/server.json)\n\
         curl -sf -X POST \"http://$addr/sql\" -H 'Accept: application/json' \
         --data 'USE NS sf; USE DB tasks; UPSERT meta:schema SET version = 16;' >/dev/null\n",
        log = log.display()
    );
    let path = consumer.join("hooks/tasks.sh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
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
    let surreal = super::board::surreal().to_string_lossy().into_owned();
    let home = cargo_home(&fixture);
    let extra = [
        ("CARGO_HOME", home.as_str()),
        ("SF_SURREAL", surreal.as_str()),
    ];

    // Its board's database is not running: nothing starts it, and nothing is moved.
    let (code, out, err) = fixture.nightly_with(NOW + 60.0, &["--refresh"], &extra);
    assert_eq!(code, 1, "{out}{err}");
    let common = consumer.join(".git");
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
             or exported, and nothing is started: its schema was not read, snapshotted or moved",
            common.join("sf-tasks").display()
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

    // Running, at schema 5 with a task on it: snapshotted, then moved by its own command.
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
            "UPSERT meta:schema SET version = 5 RETURN NONE;",
            serde_json::json!({}),
        )
        .unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 120.0, &["--refresh"], &extra);
    assert_eq!(code, 0, "{out}{err}");
    let snapshot = common.join(format!(
        "sf-tasks/snapshots/before-schema-16-{RELEASE}.surql"
    ));
    assert!(
        out.contains(&format!(
            "nightly: refresh: consumer: its board moved from schema 5 to 16, its store \
             snapshotted first to {}\n",
            snapshot.display()
        )),
        "{out}"
    );
    let kept = fs::read_to_string(&snapshot).unwrap();
    assert!(kept.contains("sim/kept"), "{kept}");
    let migrated = fs::read_to_string(fixture.root.join("migrated.log")).unwrap();
    assert_eq!(
        migrated,
        format!(
            "tasks migrate in {}\nbefore-schema-16-{RELEASE}.surql\n",
            consumer.display()
        ),
        "the snapshot was there before the move"
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
