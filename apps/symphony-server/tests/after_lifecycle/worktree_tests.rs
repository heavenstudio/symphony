//! A start in a linked worktree, whichever build it runs, never starts the one server, registers
//! its project or replaces the running server. It prints the running server's page for its
//! worktree when that server serves the project, reading the server and the registry alone, and is
//! refused otherwise, naming `--port 0`, a server of its own, which serves it beside the one server.

use super::*;
use symphony::machine::Registry;

/// The registry's file in the case's machine folder.
fn registry(repo: &Repo) -> PathBuf {
    repo.home.join(symphony::machine::PROJECTS)
}

/// A start of the one server in the case's worktree with `args`, which the start refuses: what it
/// said.
fn refused(repo: &Repo, args: &[&str]) -> String {
    let mut command = repo.command(&repo.tree);
    command.args(args);
    let mut start = repo.run(command);
    assert_eq!(start.wait().code(), Some(2), "{args:?}: {}", start.output());
    start.output()
}

/// The line a refused start ends with, after what it found: what to run instead.
fn instead(repo: &Repo) -> String {
    format!(
        ", and a start in a linked worktree ({}) never starts the machine's Symphony, registers \
         a project with it or replaces it: `just symphony --port 0` here serves this worktree on \
         a server of its own; `just symphony` in {} has the machine's serve it; `--shared` lets \
         a start here start the machine's, never replacing one that runs\n",
        repo.tree.display(),
        repo.main.display()
    )
}

#[test]
fn a_start_in_a_worktree_with_no_server_starts_and_registers_nothing() {
    let repo = Repo::new("worktree-no-server");
    // `--if-up` too: exit 3 would have its caller build and start this branch's build.
    for args in [&[][..], &["--if-up"]] {
        let said = refused(&repo, args);
        assert!(
            said.contains(&format!(
                "✖ Symphony: no Symphony answers{}",
                instead(&repo)
            )),
            "{args:?}: {said}"
        );
    }
    assert!(repo.record().is_none(), "a server was started");
    assert!(!registry(&repo).exists(), "the project was registered");
    assert!(
        !repo.home.join("server.log").exists(),
        "a server was started"
    );
}

#[test]
fn a_start_in_a_worktree_reads_the_running_server_and_the_registry_and_changes_neither() {
    let repo = Repo::new("worktree-running");
    repo.start(&repo.main, false);
    let running = repo.check_server();
    let id = &running.projects[0];
    // The project as an older build registered it, which the running server still serves: a start
    // that registered would write this build's pin over it.
    let mut projects = Registry::read(&repo.home).unwrap();
    projects.projects[0].pin = 0;
    projects.write(&repo.home).unwrap();
    let before = fs::read(registry(&repo)).unwrap();
    let page = format!(
        "→ Symphony at http://127.0.0.1:{}/{id}/@feature~lifecycle/  (pid {};",
        running.port, running.pid
    );
    for args in [&[][..], &["--if-up"]] {
        let mut command = repo.command(&repo.tree);
        command.args(args);
        let mut start = repo.run(command);
        start.success();
        assert!(
            start.output().contains(&page),
            "{args:?}: {}",
            start.output()
        );
    }
    assert_eq!(fs::read(registry(&repo)).unwrap(), before, "it registered");
    // A server of its own serves the worktree at `/` beside the one server, which it leaves alone.
    let mut command = repo.command(&repo.tree);
    command.args(["--port", "0", "--host", "127.0.0.1"]);
    let mut own = repo.run(command);
    let actual = own.ready();
    assert_ne!(actual.port, running.port);
    assert_eq!(actual.root, repo.tree.to_str().unwrap());
    assert_eq!(actual.projects, Vec::<String>::new());
    own.signal(libc::SIGTERM);
    own.success();
    // A project registered at a newer pin than the running server's is one it does not serve: the
    // start says so, and registers nothing over it.
    projects.projects[0].pin = symphony::machine::PIN + 1;
    projects.write(&repo.home).unwrap();
    let before = fs::read(registry(&repo)).unwrap();
    let said = refused(&repo, &[]);
    assert!(
        said.contains(&format!(
            "✖ Symphony: the running Symphony does not serve {}{}",
            repo.main.display(),
            instead(&repo)
        )),
        "{said}"
    );
    assert_eq!(fs::read(registry(&repo)).unwrap(), before, "it registered");
    // The one server is the one main's start made, all along.
    assert_eq!(repo.record().unwrap().pid, running.pid);
    assert_eq!(status(running.port).pid, running.pid);
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert_eq!(log.matches("→ Symphony at ").count(), 1, "{log}");
}

/// A record missing, malformed, stale or naming another process hides no running server from a
/// start in a worktree, which finds it as a start in the main checkout would, from the process
/// table, and mends no record: it changes nothing.
#[test]
fn a_start_in_a_worktree_finds_the_server_its_record_misses_and_mends_nothing() {
    let repo = Repo::new("worktree-unrecorded");
    repo.start(&repo.main, false);
    let running = repo.check_server();
    let page = format!(
        "→ Symphony at http://127.0.0.1:{}/{}/@feature~lifecycle/  (pid {};",
        running.port, running.projects[0], running.pid
    );
    let address = repo.home.join("server.json");
    let kept = fs::read(&address).unwrap();
    let mut dead = Command::new("/usr/bin/true").spawn().unwrap();
    let stale_pid = dead.id();
    dead.wait().unwrap();
    let record = |pid: i64| {
        serde_json::json!({"pid": pid, "port": running.port, "root": running.root}).to_string()
    };
    for contents in [
        None,
        Some("{malformed".to_string()),
        Some(record(i64::from(stale_pid))),
        Some(record(i64::from(std::process::id()))),
    ] {
        match &contents {
            Some(contents) => fs::write(&address, contents).unwrap(),
            None => fs::remove_file(&address).unwrap(),
        }
        let mut start = repo.run(repo.command(&repo.tree));
        start.success();
        assert!(
            start.output().contains(&page),
            "{contents:?}: {}",
            start.output()
        );
        assert_eq!(
            fs::read_to_string(&address).ok(),
            contents,
            "the record was mended"
        );
    }
    fs::write(&address, kept).unwrap();
    assert_eq!(repo.check_server().pid, running.pid);
    let log = fs::read_to_string(repo.home.join("server.log")).unwrap();
    assert_eq!(log.matches("→ Symphony at ").count(), 1, "{log}");
}

#[test]
fn shared_lets_a_worktrees_build_start_the_one_server_for_its_main_checkout() {
    let repo = Repo::new("worktree-shared");
    let mut command = repo.command(&repo.tree);
    command.arg("--shared");
    let mut start = repo.run(command);
    start.success();
    repo.remember();
    let running = repo.check_server();
    assert!(
        start
            .output()
            .contains(&format!("/{}/@feature~lifecycle/", running.projects[0])),
        "{}",
        start.output()
    );
    let registered = Registry::read(&repo.home).unwrap();
    assert_eq!(registered.projects.len(), 1);
    assert_eq!(registered.projects[0].root, repo.main);
}
