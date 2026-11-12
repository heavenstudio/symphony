//! Which board is a project's live one, by every way a run reaches it, and which builds may move it:
//! the main checkout's schema source, `tasks migrate` where it holds none, a board not made yet,
//! a repository with no main checkout, and the refusal to a process Cargo runs from its repository.

use super::*;
use std::fs;

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
    let folder = common.join(server::FOLDER);
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
fn a_server_named_by_tasks_server_is_the_live_board_when_the_runs_repository_records_its_port() {
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
        assert_eq!(named_server("127.0.0.1:5000", DATABASE, cwd, None), alpha);
        assert_eq!(named_server("localhost:05000", DATABASE, cwd, None), alpha);
        assert_eq!(named_server("127.0.0.1:5001", DATABASE, cwd, None), None);
        assert_eq!(named_server("127.0.0.1:5000", "scratch", cwd, None), None);
    }
    // Outside any repository nothing is recorded to compare with.
    assert_eq!(
        named_server("127.0.0.1:5000", DATABASE, &scratch.0, None),
        None
    );
    // A record naming no port it can be told apart from counts as any, as the launcher reads it.
    record(&common, "{}");
    assert_eq!(named_server("127.0.0.1:5001", DATABASE, &main, None), alpha);
}

#[test]
fn a_client_made_for_an_address_reaches_the_live_board_of_its_folders_repository_or_a_registered_one()
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
        find(
            "127.0.0.1:5000",
            DATABASE,
            Some(&alpha_main),
            Some(&machine)
        ),
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
            find("127.0.0.1:6000", DATABASE, cwd, Some(&machine)),
            beta_live
        );
        assert_eq!(find("127.0.0.1:7000", DATABASE, cwd, Some(&machine)), None);
        assert_eq!(find("127.0.0.1:6000", "scratch", cwd, Some(&machine)), None);
    }
    assert_eq!(
        find("127.0.0.1:6000", DATABASE, Some(&elsewhere), None),
        None
    );
    // A registered project's unreadable record names no port: only the run's own repository's
    // counts as any.
    record(&beta, "{}");
    assert_eq!(
        find("127.0.0.1:6000", DATABASE, Some(&elsewhere), Some(&machine)),
        None
    );
    record(&alpha, "{}");
    assert_eq!(
        find(
            "127.0.0.1:7000",
            DATABASE,
            Some(&alpha_main),
            Some(&machine)
        )
        .map(|live| live.common),
        Some(alpha)
    );
}

#[test]
fn a_board_not_made_yet_is_made_by_any_writer_but_where_main_holds_the_source_by_its_build_alone() {
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
        assert_eq!(may_move(&live, this_build, migrating, None, 16), Ok(()));
        assert_eq!(may_move(&bare, this_build, migrating, None, 16), Ok(()));
    }
    holds(&main, OWN_SOURCE);
    assert_eq!(may_move(&live, true, false, None, 16), Ok(()));
    let refusal = format!(
        "the alpha board is not made yet, and only a build of the schema source its main checkout \
         holds ({}) makes it: this build's is another, wherever it was built, so nothing was \
         written; `just tasks migrate` in {} makes it with main's own build, and a newer schema is \
         tried on a scratch board (`--database NAME`)",
        main.join(SOURCE).display(),
        main.display()
    );
    assert_eq!(
        may_move(&live, false, false, None, 16),
        Err(refusal.clone())
    );
    holds(&main, "// another schema\n");
    for migrating in [false, true] {
        assert_eq!(
            may_move(&live, true, migrating, None, 16),
            Err(refusal.clone())
        );
    }
}

#[test]
fn where_the_main_checkout_holds_the_boards_source_only_a_build_of_that_very_source_moves_it() {
    let scratch = Scratch::new("source");
    let (main, common) = scratch.repository("symphony");
    let live = Live {
        project: "symphony".into(),
        common,
    };
    holds(&main, OWN_SOURCE);
    for migrating in [false, true] {
        assert_eq!(may_move(&live, true, migrating, Some(15), 16), Ok(()));
        // A schema other than the build's own, as a test opens one, is no build of that source.
        assert!(may_move(&live, false, migrating, Some(15), 16).is_err());
    }
    let refusal = format!(
        "the symphony board is at schema 15, behind this build's 16, and only a build of the schema \
         source its main checkout holds ({}) moves it forward: this build's is another, wherever it \
         was built, so nothing was written; `just tasks migrate` in {} moves it with main's own \
         build, and a newer schema is tried on a scratch board (`--database NAME`)",
        main.join(SOURCE).display(),
        main.display()
    );
    // Any other text, a comment's difference included, is another schema, `migrate` or not.
    holds(&main, &OWN_SOURCE.replacen("//!", "// !", 1));
    for migrating in [false, true] {
        assert_eq!(
            may_move(&live, true, migrating, Some(15), 16),
            Err(refusal.clone())
        );
    }
}

#[test]
fn where_the_main_checkout_holds_no_board_source_only_migrate_moves_it() {
    let scratch = Scratch::new("migrate");
    let (main, common) = scratch.repository("star");
    let live = Live {
        project: "star".into(),
        common,
    };
    assert_eq!(may_move(&live, true, true, Some(15), 16), Ok(()));
    assert_eq!(
        may_move(&live, true, false, Some(15), 16),
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
    let (main, common) = scratch.repository("alpha");
    let live = Live {
        project: "alpha".into(),
        common,
    };
    fs::create_dir_all(main.join(SOURCE)).unwrap();
    let unread = fs::read_to_string(main.join(SOURCE)).unwrap_err();
    assert_eq!(
        may_move(&live, true, true, Some(15), 16),
        Err(format!(
            "the alpha board is at schema 15, behind this build's 16, and {} cannot be read \
             ({unread}) to say whether this build's schema is its main checkout's: nothing was \
             written",
            main.join(SOURCE).display()
        ))
    );
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
        for this_build in [true, false] {
            assert_eq!(may_move(&live, this_build, true, Some(15), 16), Ok(()));
            assert_eq!(may_move(&live, this_build, false, None, 16), Ok(()));
            assert_eq!(
                may_move(&live, this_build, false, Some(15), 16),
                Err(format!(
                    "the loose board is at schema 15, behind this build's 16, and its repository \
                     has no main checkout to say which schema is its own, so only `just tasks \
                     migrate` run in a checkout of it moves it forward: nothing was written; `just \
                     tasks migrate` in any checkout of the repository at {} moves it, and a newer \
                     schema is tried on a scratch board (`--database NAME`)",
                    common.display()
                ))
            );
        }
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
    assert_eq!(
        cargo_refusal(&live, Some(&package)),
        Some(format!(
            "the alpha board is the live board of the repository Cargo runs this process from \
             (CARGO_MANIFEST_DIR={}: a test, an example or `cargo run`), and none of those ever \
             reaches it: nothing was read or written; give it a board of its own (`TASKS_SERVER` \
             and `TASKS_DATABASE`, or a fixture repository in a temporary folder)",
            package.display()
        ))
    );
    assert!(cargo_refusal(&live, Some(&main)).is_some());
    // Not run by Cargo, or run from another repository (a fixture's own board): nothing refused.
    assert_eq!(cargo_refusal(&live, None), None);
    assert_eq!(cargo_refusal(&live, Some(&other_main)), None);
    assert_eq!(cargo_refusal(&live, Some(&scratch.0)), None);
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
