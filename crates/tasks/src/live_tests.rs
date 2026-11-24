//! Which board is a project's live one, by every way a run reaches it, and which builds may move it:
//! the main checkout's schema source, `tasks migrate` where it holds none, a board not made yet,
//! a repository with no main checkout, and the refusal to a process Cargo runs from its repository.

use super::*;
use std::fs;

#[test]
fn digest_comparison_requires_the_exact_lowercase_sha256() {
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert!(has_digest("", EMPTY));
    assert!(has_digest("abc", ABC));
    assert!(!has_digest("abc", EMPTY));
    for wrong in [
        String::new(),
        ABC[..63].to_string(),
        format!("{ABC}0"),
        ABC.to_uppercase(),
        "z".repeat(64),
        "λ".repeat(32),
    ] {
        assert!(!has_digest("abc", &wrong), "{wrong:?}");
    }
}

/// A folder of the test's own, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "live-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir.canonicalize().unwrap())
    }

    /// A main checkout at `name`: a folder holding `.git`, and its common dir.
    fn repository(&self, name: &str) -> (PathBuf, PathBuf) {
        let main = self.0.join(name);
        fs::create_dir_all(main.join(".git")).unwrap();
        (main.clone(), main.join(".git"))
    }

    /// A linked worktree of the repository whose common dir is `common`, at `name`.
    fn worktree(&self, common: &Path, name: &str) -> PathBuf {
        let top = self.0.join(name);
        let gitdir = common.join("worktrees").join(name);
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(&top).unwrap();
        fs::write(gitdir.join("commondir"), "../..\n").unwrap();
        fs::write(top.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        top
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `common`'s board server record, as `text`.
fn record(common: &Path, text: &str) {
    let folder = common.join(crate::server::FOLDER);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("server.json"), text).unwrap();
}

/// A record naming `port`, as the board's server writes it.
fn record_port(common: &Path, port: u16) {
    record(
        common,
        &format!(r#"{{"addr":"127.0.0.1:{port}","pid":1,"version":"3.3.0"}}"#),
    );
}

/// The main checkout `main` holding `text` as the board's source.
fn holds(main: &Path, text: &str) {
    let source = main.join(SOURCE);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(source, text).unwrap();
}

#[test]
fn a_port_is_read_as_the_boards_client_reads_it() {
    for (addr, port) in [
        ("127.0.0.1:4321", Some(4321)),
        ("127.0.0.1:04321", Some(4321)),
        ("127.0.0.1:+4321", Some(4321)),
        ("localhost:65535", Some(65535)),
        ("127.0.0.1:65536", None),
        ("127.0.0.1:port", None),
        ("127.0.0.1:", None),
        ("4321", None),
    ] {
        assert_eq!(port_of(addr), port, "{addr}");
    }
}

#[test]
fn a_postgresql_socket_names_its_repository_and_refuses_its_cargo_process() {
    let scratch = Scratch::new("postgres-socket");
    let (main, common) = scratch.repository("alpha");
    let worktree = scratch.worktree(&common, "topic");
    let socket = server::postgres::socket_folder(&common.join(server::FOLDER).join("postgresql"));
    fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let data = common.join(server::FOLDER).join("postgresql");
    assert_eq!(
        server::board_served(&format!("/pg/bin/postgres -D {}", data.display())),
        Some(common.clone())
    );
    assert_eq!(
        server::board_served(&format!("/pg/bin/not-postgres -D {}", data.display())),
        None
    );
    assert_eq!(
        server::board_served("/pg/bin/postgres -D /tmp/private-cluster"),
        None
    );
    let addr = format!("postgresql:{}", socket.display());
    for cwd in [&main, &worktree] {
        let found = named_server(&addr, DATABASE, cwd, None).unwrap();
        assert_eq!(found.common, common);
        assert_eq!(find(&addr, DATABASE, Some(cwd), None), Some(found));
        assert!(cargo_refusal_of(None, &addr, DATABASE, &[Cargo::own(cwd)]).is_some());
        assert_eq!(named_server(&addr, "scratch", cwd, None), None);
    }
    assert_eq!(
        find("postgresql:/tmp/another", DATABASE, Some(&main), None),
        None
    );
}

#[test]
fn a_postgresql_schema_move_requires_its_own_committed_definitions_too() {
    let scratch = Scratch::new("postgres-source");
    let main = scratch.0.join("alpha");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    holds(&main, OWN_SOURCE);
    let pg = main.join(POSTGRES_SOURCE);
    fs::create_dir_all(pg.parent().unwrap()).unwrap();
    let own = include_str!("store/postgres/schema.rs");
    fs::write(&pg, own).unwrap();
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-qm", "schema"]);
    let live = Live {
        project: "alpha".into(),
        common: main.join(".git"),
    };
    assert_eq!(
        may_move(&live, true, false, Some(1), 2, SCRATCH_FLAG),
        Ok(())
    );
    fs::write(&pg, format!("{own}\n")).unwrap();
    // An uncommitted change is irrelevant.
    assert_eq!(
        may_move(&live, true, false, Some(1), 2, SCRATCH_FLAG),
        Ok(())
    );
    git(&main, &["commit", "-qam", "changed definitions"]);
    let why = may_move(&live, true, true, Some(1), 2, SCRATCH_FLAG).unwrap_err();
    assert!(why.contains(POSTGRES_SOURCE), "{why}");
    assert!(why.contains("nothing was written"), "{why}");
}

#[test]
fn a_record_names_its_port_none_without_one_and_any_when_it_names_none_it_can_be_told_by() {
    let scratch = Scratch::new("record");
    let (_, common) = scratch.repository("repo");
    assert_eq!(recorded(&common), Recorded::None);
    record_port(&common, 5000);
    assert_eq!(recorded(&common), Recorded::Port(5000));
    for text in [
        "{}",
        r#"{"addr":"127.0.0.1:port"}"#,
        r#"{"addr":5000}"#,
        "not json",
    ] {
        record(&common, text);
        assert_eq!(recorded(&common), Recorded::Unreadable, "{text}");
    }
    // An unreadable record names any port, unless asked strictly.
    assert!(Recorded::Unreadable.names(Some(1), false));
    assert!(Recorded::Unreadable.names(None, false));
    assert!(!Recorded::Unreadable.names(Some(1), true));
    assert!(Recorded::Port(5000).names(Some(5000), true));
    assert!(!Recorded::Port(5000).names(Some(5001), false));
    assert!(!Recorded::Port(5000).names(None, false));
    assert!(!Recorded::None.names(Some(5000), false));
}

#[test]
fn an_address_reaches_the_board_its_listening_server_serves_wherever_the_run_is() {
    let scratch = Scratch::new("served");
    let (main, common) = scratch.repository("alpha");
    fs::write(main.join("symphony.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    let (_, other) = scratch.repository("copy");
    // The copy's record names the server's port: what the copy's folder says is no matter.
    record_port(&other, 5000);
    let alpha = Live {
        project: "alpha".into(),
        common: common.clone(),
    };
    assert_eq!(
        by_server(Served::Board(common.clone()), None),
        Some(Some(alpha.clone()))
    );
    assert_eq!(by_server(Served::Other, None), Some(None));
    assert_eq!(by_server(Served::Unknown, None), None);
    // A scratch database is never asked about.
    assert_eq!(named_server("127.0.0.1:5000", "scratch", &main, None), None);
    assert_eq!(find("127.0.0.1:5000", "scratch", Some(&main), None), None);
    // A process Cargo runs from the repository whose board the server serves is refused, from
    // whichever folder; one of the copy's is not, though the copy's record names the port.
    let package = main.join("crates/tasks");
    fs::create_dir_all(&package).unwrap();
    let copied = other.parent().unwrap().join("crates/tasks");
    fs::create_dir_all(&copied).unwrap();
    let board = Served::Board(common.clone());
    let refused = cargo_refusal_by(board.clone(), "127.0.0.1:5000", &[Cargo::own(&package)]);
    assert_eq!(refused, cargo_refusal(&alpha, &[Cargo::own(&package)]));
    assert!(refused.is_some());
    assert_eq!(
        cargo_refusal_by(board, "127.0.0.1:5000", &[Cargo::own(&copied)]),
        None
    );
    assert_eq!(
        cargo_refusal_by(Served::Other, "127.0.0.1:5000", &[Cargo::own(&copied)]),
        None
    );
    assert!(cargo_refusal_by(Served::Unknown, "127.0.0.1:5000", &[Cargo::own(&copied)]).is_some());
}

#[test]
fn a_listener_of_no_boards_serves_no_board_and_a_port_nothing_listens_on_cannot_be_told() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    // This test's own process listens there: no board's server.
    assert_eq!(served(&addr), Served::Other);
    drop(listener);
    assert_eq!(served(&addr), Served::Unknown);
    for addr in ["127.0.0.1", "127.0.0.1:port", ""] {
        assert_eq!(served(addr), Served::Unknown, "{addr}");
    }
}

#[test]
fn where_the_machine_cannot_say_who_listens_a_tasks_server_is_the_board_its_repository_records() {
    let scratch = Scratch::new("named");
    let (main, common) = scratch.repository("alpha");
    fs::write(main.join("symphony.toml"), "[project]\nid = \"alpha\"\n").unwrap();
    let worktree = scratch.worktree(&common, "alpha-wt");
    record_port(&common, 5000);
    let alpha = Some(Live {
        project: "alpha".into(),
        common: common.clone(),
    });
    for cwd in [&main, &worktree] {
        assert_eq!(named_by_record("127.0.0.1:5000", cwd, None), alpha);
        assert_eq!(named_by_record("localhost:05000", cwd, None), alpha);
        assert_eq!(named_by_record("127.0.0.1:5001", cwd, None), None);
    }
    // Outside any repository nothing is recorded to compare with.
    assert_eq!(named_by_record("127.0.0.1:5000", &scratch.0, None), None);
    // A record naming no port it can be told apart from counts as any, as the launcher reads it.
    record(&common, "{}");
    assert_eq!(named_by_record("127.0.0.1:5001", &main, None), alpha);
}

#[test]
fn where_the_machine_cannot_say_who_listens_an_address_is_the_board_its_folder_or_a_registered_project_records()
 {
    let scratch = Scratch::new("find");
    let (alpha_main, alpha) = scratch.repository("alpha");
    let (beta_main, beta) = scratch.repository("beta");
    record_port(&alpha, 5000);
    record_port(&beta, 6000);
    let machine = scratch.0.join("machine");
    fs::create_dir_all(&machine).unwrap();
    fs::write(
        machine.join(project::REGISTRY),
        serde_json::json!({"projects": [{"id": "beta-id", "root": beta_main}]}).to_string(),
    )
    .unwrap();
    let elsewhere = scratch.0.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    // From alpha's folder: alpha's board by its record, beta's by the registry.
    assert_eq!(
        found_by_record("127.0.0.1:5000", Some(&alpha_main), Some(&machine)),
        Some(Live {
            project: "alpha".into(),
            common: alpha.clone()
        })
    );
    let beta_live = Some(Live {
        project: "beta-id".into(),
        common: beta.clone(),
    });
    for cwd in [Some(alpha_main.as_path()), Some(elsewhere.as_path()), None] {
        assert_eq!(
            found_by_record("127.0.0.1:6000", cwd, Some(&machine)),
            beta_live
        );
        assert_eq!(found_by_record("127.0.0.1:7000", cwd, Some(&machine)), None);
    }
    assert_eq!(
        found_by_record("127.0.0.1:6000", Some(&elsewhere), None),
        None
    );
    // A registered project's unreadable record names no port: only the run's own repository's
    // counts as any.
    record(&beta, "{}");
    assert_eq!(
        found_by_record("127.0.0.1:6000", Some(&elsewhere), Some(&machine)),
        None
    );
    record(&alpha, "{}");
    assert_eq!(
        found_by_record("127.0.0.1:7000", Some(&alpha_main), Some(&machine))
            .map(|live| live.common),
        Some(alpha)
    );
}

/// What `branch` commits: `text`, or nothing.
fn on(branch: &str, text: Option<&str>) -> Committed {
    Committed {
        branch: branch.into(),
        source: text.map_or(Source::Nothing, |text| Source::Text(text.into())),
    }
}

#[test]
fn a_board_not_made_yet_is_made_by_any_writer_but_where_the_branch_commits_the_source_by_its_build_alone()
 {
    let scratch = Scratch::new("made");
    let (main, common) = scratch.repository("alpha");
    let live = Live {
        project: "alpha".into(),
        common,
    };
    let bare = Live {
        project: "bare".into(),
        common: scratch.0.join("bare.git"),
    };
    let writers = [(true, false), (false, false), (false, true), (true, true)];
    for (this_build, migrating) in writers {
        for board in [&live, &bare] {
            assert_eq!(
                judged(
                    board,
                    &on("main", None),
                    this_build,
                    migrating,
                    None,
                    16,
                    SCRATCH_FLAG
                ),
                Ok(())
            );
        }
    }
    let own = on("main", Some(OWN_SOURCE));
    assert_eq!(
        judged(&live, &own, true, false, None, 16, SCRATCH_FLAG),
        Ok(())
    );
    let refusal = format!(
        "the alpha board is not made yet, and only a build of the schema source its default \
         branch `main` commits ({SOURCE}) makes it: this build's is another, wherever it was built \
         and whatever a checkout holds, so nothing was written; `just tasks migrate` in {} makes it \
         with that branch's own build, and a newer schema is tried on a scratch board (`--database \
         NAME`)",
        main.display()
    );
    assert_eq!(
        judged(&live, &own, false, false, None, 16, SCRATCH_FLAG),
        Err(refusal.clone())
    );
    let another = on("main", Some("// another schema\n"));
    for migrating in [false, true] {
        assert_eq!(
            judged(&live, &another, true, migrating, None, 16, SCRATCH_FLAG),
            Err(refusal.clone())
        );
    }
}

#[test]
fn where_the_default_branch_commits_the_boards_source_only_a_build_of_that_very_source_moves_it() {
    let scratch = Scratch::new("source");
    let (main, common) = scratch.repository("symphony");
    let live = Live {
        project: "symphony".into(),
        common,
    };
    let own = on("trunk", Some(OWN_SOURCE));
    for migrating in [false, true] {
        assert_eq!(
            judged(&live, &own, true, migrating, Some(15), 16, SCRATCH_FLAG),
            Ok(())
        );
        // A schema other than the build's own, as a test opens one, is no build of that source.
        assert!(judged(&live, &own, false, migrating, Some(15), 16, SCRATCH_FLAG).is_err());
    }
    let refusal = format!(
        "the symphony board is at schema 15, behind this build's 16, and only a build of the schema \
         source its default branch `trunk` commits ({SOURCE}) moves it forward: this build's is \
         another, wherever it was built and whatever a checkout holds, so nothing was written; \
         `just tasks migrate` in {} moves it with that branch's own build, and a newer schema is \
         tried on a scratch board (`--database NAME`)",
        main.display()
    );
    // Any other text, a comment's difference included, is another schema, `migrate` or not.
    let another = on("trunk", Some(&OWN_SOURCE.replacen("//!", "// !", 1)));
    for migrating in [false, true] {
        assert_eq!(
            judged(&live, &another, true, migrating, Some(15), 16, SCRATCH_FLAG),
            Err(refusal.clone())
        );
    }
}

#[test]
fn where_the_default_branch_commits_no_board_source_only_migrate_moves_it() {
    let scratch = Scratch::new("migrate");
    let (main, common) = scratch.repository("star");
    let live = Live {
        project: "star".into(),
        common,
    };
    let none = on("main", None);
    assert_eq!(
        judged(&live, &none, true, true, Some(15), 16, SCRATCH_FLAG),
        Ok(())
    );
    assert_eq!(
        judged(&live, &none, true, false, Some(15), 16, SCRATCH_FLAG),
        Err(format!(
            "the star board is at schema 15, behind this build's 16, and only `just tasks migrate` \
             run in its main checkout moves it forward: nothing was written; `just tasks migrate` \
             in {} moves it, and a newer schema is tried on a scratch board (`--database NAME`)",
            main.display()
        ))
    );
}

#[test]
fn a_source_that_cannot_be_read_moves_nothing() {
    let scratch = Scratch::new("unread");
    let (_, common) = scratch.repository("alpha");
    let live = Live {
        project: "alpha".into(),
        common,
    };
    let unread = Committed {
        branch: "main".into(),
        source: Source::Unreadable("fatal: bad object".into()),
    };
    for (this_build, migrating, found) in [(true, true, Some(15)), (true, false, None)] {
        assert_eq!(
            judged(
                &live,
                &unread,
                this_build,
                migrating,
                found,
                16,
                SCRATCH_FLAG
            )
            .unwrap_err()
            .split_once(", and ")
            .map(|(_, why)| why.to_string()),
            Some(format!(
                "the schema source its default branch `main` commits ({SOURCE}) cannot be read \
                 (fatal: bad object) to say whether this build's schema is its own: nothing was \
                 written"
            ))
        );
    }
}

/// `git ARGS` in `dir`, which must succeed.
fn git(dir: &Path, args: &[&str]) {
    let done = crate::sys::output(
        crate::sys::command("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args),
    )
    .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

#[test]
fn the_source_is_what_the_default_branch_commits_whatever_the_main_checkout_has_out() {
    let scratch = Scratch::new("committed");
    let main = scratch.0.join("alpha");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    holds(&main, "// main's\n");
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-qm", "main's"]);
    // The main checkout has a branch out that commits another source, and holds a third.
    git(&main, &["checkout", "-qb", "topic"]);
    holds(&main, "// topic's\n");
    git(&main, &["commit", "-qam", "topic's"]);
    holds(&main, "// the working copy's\n");
    let common = main.join(".git");
    let unread = |branch: &str, why: &str| Committed {
        branch: branch.into(),
        source: Source::Unreadable(why.into()),
    };
    // Unnamed, the default branch is `main`.
    assert_eq!(committed(&common, None), on("main", Some("// main's\n")));
    assert_eq!(
        committed(&common, Some("main")),
        on("main", Some("// main's\n"))
    );
    assert_eq!(
        committed(&common, Some("topic")),
        on("topic", Some("// topic's\n"))
    );
    // A branch that commits no source: nothing.
    git(&main, &["checkout", "-qfb", "bare-trunk", "main"]);
    git(&main, &["rm", "-q", SOURCE]);
    git(&main, &["commit", "-qm", "no source"]);
    assert_eq!(
        committed(&common, Some("bare-trunk")),
        on("bare-trunk", None)
    );
    // A branch the project names that does not exist cannot be read, nor can a ref that names no
    // commit, nor one that names nothing git can read.
    assert_eq!(
        committed(&common, Some("trunk")),
        unread(
            "trunk",
            "there is no branch `trunk`, which `[project] default_branch` names"
        )
    );
    let missing = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n";
    fs::write(common.join("refs/heads/broken"), missing).unwrap();
    assert_eq!(
        committed(&common, Some("broken")),
        unread("broken", "refs/heads/broken names no commit")
    );
    fs::write(common.join("refs/heads/garbage"), "garbage\n").unwrap();
    let garbage = committed(&common, Some("garbage"));
    assert!(
        matches!(&garbage.source, Source::Unreadable(why) if !why.is_empty() && !why.contains('\n')),
        "{garbage:?}"
    );
    // A folder git cannot read as a repository says why.
    let nowhere = committed(&scratch.0.join("nowhere"), None);
    assert!(
        matches!(&nowhere.source, Source::Unreadable(why) if !why.is_empty()),
        "{nowhere:?}"
    );
    // An unnamed `main` that does not exist (no commit on it yet) commits nothing; named, it
    // cannot be read; broken, it cannot be read either.
    let empty = scratch.0.join("empty");
    fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q", "-b", "main"]);
    assert_eq!(committed(&empty.join(".git"), None), on("main", None));
    assert_eq!(
        committed(&empty.join(".git"), Some("main")),
        unread(
            "main",
            "there is no branch `main`, which `[project] default_branch` names"
        )
    );
    fs::write(common.join("refs/heads/main"), missing).unwrap();
    assert_eq!(
        committed(&common, None),
        unread("main", "refs/heads/main names no commit")
    );

    // The default branch: `[project] default_branch` in the main checkout's file, as written and
    // by the project file's one rule, else none named.
    assert_eq!(default_branch(Some(&main)), Ok(None));
    assert_eq!(default_branch(None), Ok(None));
    let empty = Err("[project] default_branch is empty".to_string());
    for (text, branch) in [
        (
            "[project]\nid = \"alpha\"\ndefault_branch = \"trunk\"\n",
            Ok(Some("trunk")),
        ),
        // Never trimmed, as the Symphony and the land read it: git finds no such branch.
        ("[project]\ndefault_branch = \" dev \"\n", Ok(Some(" dev "))),
        ("[project]\ndefault_branch = \"\"\n", empty.clone()),
        ("[project]\ndefault_branch = \"  \"\n", empty.clone()),
        ("[project]\ndefault_branch = \"\\t\"\n", empty.clone()),
        ("[project]\ndefault_branch = 7\n", Ok(None)),
        ("[agents]\nlaunch = \"x\"\n", Ok(None)),
        ("not toml [", Ok(None)),
    ] {
        fs::write(main.join("symphony.toml"), text).unwrap();
        assert_eq!(
            default_branch(Some(&main)),
            branch.map(|named| named.map(str::to_string)),
            "{text}"
        );
    }
}

#[test]
fn a_default_branch_the_project_files_rule_refuses_moves_no_board() {
    let scratch = Scratch::new("blank-branch");
    let (main, common) = scratch.repository("alpha");
    let live = Live {
        project: "alpha".into(),
        common,
    };
    assert_eq!(named_branch(&live), Ok(None), "none written");
    fs::write(
        main.join("symphony.toml"),
        "[project]\ndefault_branch = \"trunk\"\n",
    )
    .unwrap();
    assert_eq!(named_branch(&live), Ok(Some("trunk".into())));
    fs::write(
        main.join("symphony.toml"),
        "[project]\ndefault_branch = \"  \"\n",
    )
    .unwrap();
    let refused = Committed {
        branch: "  ".into(),
        source: Source::Unreadable("[project] default_branch is empty".into()),
    };
    assert_eq!(named_branch(&live), Err(refused));
    // Neither a build of the board's own source nor `migrate` moves it, nor makes it.
    for (this_build, migrating, found) in [
        (true, true, Some(15)),
        (true, false, Some(15)),
        (false, true, None),
    ] {
        let said = |moved: Result<(), String>| {
            let why = moved.expect_err("refused");
            assert!(
                why.contains("cannot be read ([project] default_branch is empty)"),
                "{why}"
            );
        };
        said(may_move(
            &live,
            this_build,
            migrating,
            found,
            16,
            SCRATCH_FLAG,
        ));
    }
}

#[test]
fn a_repository_with_no_main_checkout_is_moved_by_migrate_alone_and_its_refusal_says_where() {
    let scratch = Scratch::new("no-main");
    // A bare repository's common dir, and a submodule's (kept in its superproject's `modules/`).
    for common in [
        scratch.0.join("bare.git"),
        scratch.0.join("super/.git/modules/sub"),
    ] {
        let live = Live {
            project: "loose".into(),
            common: common.clone(),
        };
        assert_eq!(live.main(), None);
        let none = on("main", None);
        for this_build in [true, false] {
            assert_eq!(
                judged(&live, &none, this_build, true, Some(15), 16, SCRATCH_FLAG),
                Ok(())
            );
            assert_eq!(
                judged(&live, &none, this_build, false, None, 16, SCRATCH_FLAG),
                Ok(())
            );
            assert_eq!(
                judged(&live, &none, this_build, false, Some(15), 16, SCRATCH_FLAG),
                Err(format!(
                    "the loose board is at schema 15, behind this build's 16, and its repository \
                     has no main checkout and its default branch `main` commits no board source, \
                     so only `just tasks migrate` run in a checkout of it moves it forward: \
                     nothing was written; `just tasks migrate` in any checkout of the repository \
                     at {} moves it, and a newer schema is tried on a scratch board (`--database \
                     NAME`)",
                    common.display()
                ))
            );
        }
        // Where its default branch commits the board's source, it is judged by that source too.
        let own = on("main", Some(OWN_SOURCE));
        assert_eq!(
            judged(&live, &own, true, false, Some(15), 16, SCRATCH_FLAG),
            Ok(())
        );
        let refused = judged(&live, &own, false, true, Some(15), 16, SCRATCH_FLAG).unwrap_err();
        assert!(
            refused.contains(&format!(
                "`just tasks migrate` in any checkout of the repository at {} moves it with that \
                 branch's own build",
                common.display()
            )),
            "{refused}"
        );
    }
}

#[test]
fn a_process_cargo_runs_is_refused_the_live_board_of_the_repository_it_runs_from_alone() {
    let scratch = Scratch::new("cargo");
    let (main, common) = scratch.repository("alpha");
    let worktree = scratch.worktree(&common, "alpha-wt");
    let (other_main, _) = scratch.repository("fixture");
    let live = Live {
        project: "alpha".into(),
        common: common.clone(),
    };
    let package = worktree.join("crates/tasks");
    fs::create_dir_all(&package).unwrap();
    let refusal = |from: String| {
        format!(
            "the alpha board is the live board of the repository Cargo runs this process from \
             ({from}: a test, an example or `cargo run`), and none of those ever reaches it: \
             nothing was read or written; give it a board of its own (`TASKS_SERVER` and \
             `TASKS_DATABASE`, or a fixture repository in a temporary folder)"
        )
    };
    assert_eq!(
        cargo_refusal(&live, &[Cargo::own(&package)]),
        Some(refusal(format!("CARGO_MANIFEST_DIR={}", package.display())))
    );
    assert!(cargo_refusal(&live, &[Cargo::own(&main)]).is_some());
    // Not run by Cargo, or run from another repository (a fixture's own board): nothing refused.
    assert_eq!(cargo_refusal(&live, &[]), None);
    assert_eq!(cargo_refusal(&live, &[Cargo::own(&other_main)]), None);
    assert_eq!(cargo_refusal(&live, &[Cargo::own(&scratch.0)]), None);
    // A process a test started with a cleared environment, under a process Cargo started in the
    // repository: that one's folder is refused, whichever folder of another its own names.
    let caller = Cargo {
        dir: package.clone(),
        caller: Some((4242, "Cargo")),
    };
    let under = [Cargo::own(&other_main), caller.clone()];
    let said = refusal(format!(
        "process 4242, which Cargo started in {} and this process runs under",
        package.display()
    ));
    assert_eq!(cargo_refusal(&live, &under), Some(said.clone()));
    // One nextest started is named so.
    let by_nextest = Cargo {
        dir: package.clone(),
        caller: Some((4243, "nextest")),
    };
    assert_eq!(
        cargo_refusal(&live, &[by_nextest]),
        Some(refusal(format!(
            "process 4243, which nextest started in {} and this process runs under",
            package.display()
        )))
    );
    // Known by the board's port alone: the caller's repository records it.
    record(&common, r#"{"addr": "127.0.0.1:47811"}"#);
    let refused = cargo_refusal_by(Served::Unknown, "127.0.0.1:47811", &under).unwrap();
    assert!(
        refused.contains(&said[said.find('(').unwrap()..]),
        "{refused}"
    );
    assert_eq!(
        cargo_refusal_by(Served::Unknown, "127.0.0.1:47812", &under),
        None
    );
    assert_eq!(
        cargo_refusal_of(None, "127.0.0.1:47811", "scratch", &under),
        None
    );
    assert_eq!(cargo_refusal_of(Some(&live), "", "", &under), Some(said));
    assert_eq!(cargo_refusal_of(Some(&live), "", "", &under[..1]), None);
}

#[test]
fn a_process_cargo_or_nextest_started_is_each_caller_whose_own_parent_is_named_so() {
    let caller = |pid: u32, name: &str, parent: u32| crate::sys::Caller {
        pid,
        name: name.into(),
        parent,
    };
    let folder = |pid: u32| (pid != 13).then(|| PathBuf::from(format!("/at/{pid}")));
    let started = |dir: &str, pid: u32, by: &'static str| Cargo {
        dir: dir.into(),
        caller: Some((pid, by)),
    };
    // A test's shell, the test, Cargo, the shell that ran it; then a second Cargo above a tool.
    let chain = [
        caller(10, "sh", 11),
        caller(11, "cli-0123456789abcdef", 12),
        caller(12, "cargo", 20),
        caller(20, "cargo-watch", 21),
        caller(21, "cargo", 22),
        caller(22, "zsh", 1),
    ];
    assert_eq!(
        started_by_cargo(&chain, folder),
        [
            started("/at/11", 11, "Cargo"),
            started("/at/20", 20, "Cargo")
        ]
    );
    // Under `cargo nextest run`, which Cargo execs: nextest starts each test.
    let nextest = [
        caller(10, "sh", 11),
        caller(11, "cli-0123456789abcdef", 12),
        caller(12, "cargo-nextest", 22),
        caller(22, "zsh", 1),
    ];
    assert_eq!(
        started_by_cargo(&nextest, folder),
        [started("/at/11", 11, "nextest")]
    );
    // This process's own parent is no caller's: one Cargo starts holds its own variable.
    assert_eq!(started_by_cargo(&chain[2..3], folder), []);
    // A folder that cannot be read, a name like Cargo's, a chain whose links do not meet.
    assert_eq!(
        started_by_cargo(&[caller(13, "sh", 14), caller(14, "cargo", 1)], folder),
        []
    );
    assert_eq!(
        started_by_cargo(
            &[caller(10, "sh", 11), caller(11, "cargo-watch", 1)],
            folder
        ),
        []
    );
    // `cargo run` and an example, which Cargo execs: their parent is the shell that ran Cargo.
    assert_eq!(
        started_by_cargo(
            &[
                caller(10, "sh", 11),
                caller(11, "dispatch_load", 22),
                caller(22, "zsh", 1)
            ],
            folder
        ),
        []
    );
    assert_eq!(
        started_by_cargo(&[caller(10, "sh", 99), caller(11, "cargo", 1)], folder),
        []
    );
    assert_eq!(started_by_cargo(&[], folder), []);
}

/// The path a main checkout is read at is, in this repository, the very file the build embeds.
#[test]
fn the_source_a_main_checkout_is_read_at_is_the_file_this_build_embeds() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(
        fs::read_to_string(repository.join(SOURCE)).unwrap(),
        OWN_SOURCE
    );
}

/// The build knows its own source by the digest it holds, and a source changed by one byte, or cut
/// short, or empty, for another.
#[test]
fn the_digest_this_build_holds_is_its_schema_sources_and_no_other_texts() {
    assert_eq!(OWN_DIGEST.len(), 64, "{OWN_DIGEST}");
    assert!(is_own(OWN_SOURCE));
    assert!(!is_own(&OWN_SOURCE.replacen("//!", "// !", 1)));
    assert!(!is_own(&OWN_SOURCE[..OWN_SOURCE.len() - 1]));
    assert!(!is_own(&format!("{OWN_SOURCE}\n")));
    assert!(!is_own(""));
}
