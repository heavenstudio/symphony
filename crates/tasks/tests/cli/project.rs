//! The project a command runs in (`symphony_board::project`): the nearest `symphony.toml` from its
//! folder up, never past the top of its checkout; with none, the registry's entry for the
//! repository's main checkout, else that checkout's folder name. Its board is the repository's.

use crate::{git, repository, scratch};
use std::ffi::OsString;
use std::path::Path;
use symphony_board::project::{self, FILE, MACHINE_DIR, Place, Project};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The id `find` names the project of `cwd` by, with the registry in `machine`.
fn id(cwd: &Path, machine: Option<&Path>) -> String {
    project::find(cwd, machine).unwrap().id
}

/// A machine folder whose registry holds `entries` (id, root).
fn machine(dir: &Path, entries: &[(&str, &Path)]) -> std::path::PathBuf {
    let machine = dir.join("machine");
    let projects: Vec<serde_json::Value> = entries
        .iter()
        .map(|(id, root)| serde_json::json!({"id": id, "name": id, "root": root}))
        .collect();
    write(
        &machine.join(project::REGISTRY),
        &serde_json::json!({ "projects": projects }).to_string(),
    );
    machine
}

#[test]
fn the_file_at_the_checkouts_top_names_the_project_from_every_folder_under_it() {
    let dir = scratch("project-top");
    let main = repository(&dir.join("main"));
    write(
        &main.join(FILE),
        "[project]\nname = \"Alpha\"\nid = \"alpha\"\n",
    );
    let deep = main.join("docs/game/notes");
    std::fs::create_dir_all(&deep).unwrap();
    for cwd in [&main, &deep] {
        assert_eq!(
            project::find(cwd, None).unwrap(),
            Project {
                place: Place {
                    root: main.clone(),
                    file: Some(main.join(FILE)),
                    common: main.join(".git"),
                },
                id: "alpha".into(),
            }
        );
    }
}

#[test]
fn a_nested_file_names_the_folders_under_it_and_shares_the_repositorys_board() {
    let dir = scratch("project-nested");
    let main = repository(&dir.join("main"));
    write(&main.join(FILE), "[project]\nid = \"alpha\"\n");
    write(
        &main.join("tools/kit").join(FILE),
        "[project]\nid = \"kit\"\n",
    );
    std::fs::create_dir_all(main.join("tools/kit/src")).unwrap();
    let inner = project::find(&main.join("tools/kit/src"), None).unwrap();
    assert_eq!(inner.id, "kit");
    assert_eq!(inner.place.root, main.join("tools/kit"));
    assert_eq!(inner.place.common, main.join(".git"));
    assert_eq!(id(&main.join("tools"), None), "alpha");
}

#[test]
fn a_repository_inside_another_projects_folder_is_its_own_never_the_outer_ones() {
    let dir = scratch("project-inner-repository");
    let outer = repository(&dir.join("outer"));
    write(&outer.join(FILE), "[project]\nid = \"outer\"\n");
    // A fixture repository under the outer checkout's artifacts, as the tests' fixtures are.
    let fixture = repository(&outer.join("artifacts/fixture"));
    std::fs::create_dir_all(fixture.join("docs")).unwrap();
    let found = project::find(&fixture.join("docs"), None).unwrap();
    assert_eq!(
        found.place,
        Place {
            root: fixture.clone(),
            file: None,
            common: fixture.join(".git"),
        }
    );
    assert_eq!(found.id, "fixture");
    let mut beside = outer.join("artifacts/loose");
    std::fs::create_dir_all(&beside).unwrap();
    assert_eq!(id(&beside, None), "outer");
    beside.push("..");
    assert_eq!(project::locate(&beside).unwrap().common, outer.join(".git"));
}

/// A fixture project in its own repository under another checkout's artifacts, as a server's
/// scratch projects are: the outer checkout and the project's folder.
fn nested(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = scratch(name);
    let outer = repository(&dir.join("outer"));
    write(&outer.join(FILE), "[project]\nid = \"outer\"\n");
    let inner = repository(&outer.join("artifacts/scratch/inner"));
    write(&inner.join(FILE), "[project]\nid = \"inner\"\n");
    (outer, inner)
}

/// The project whose folder the Symphony reaches a board from (`project::at`) is the one there
/// while the folder is; once it is gone, none is, never the checkout its path leads up to: the
/// refusal counts as a project with no board.
#[test]
fn a_projects_folder_that_is_gone_names_no_project_never_the_checkout_above_it() {
    let (outer, inner) = nested("project-at-gone");
    assert_eq!(project::at(&inner, None).unwrap().id, "inner");
    // A folder in a project is that project's, as a command's lookup has it.
    std::fs::create_dir_all(outer.join("docs/notes")).unwrap();
    assert_eq!(
        project::at(&outer.join("docs/notes"), None).unwrap().id,
        "outer"
    );
    std::fs::remove_dir_all(&inner).unwrap();
    let refused = project::at(&inner, None).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "the project's folder {} is gone: {}",
            inner.display(),
            project::GONE
        )
    );
    assert!(symphony_board::board_health::absent(&refused), "{refused}");
    // A folder whose parent went too names nothing.
    let deeper = outer.join("artifacts/scratch/inner/docs");
    std::fs::remove_dir_all(outer.join("artifacts")).unwrap();
    assert!(
        project::at(&deeper, None)
            .unwrap_err()
            .ends_with(project::GONE)
    );
}

/// The project whose folder the Symphony reaches a board from, held to the repository it found
/// there when it began to serve it (`project::held`), is the one there while the folder holds that
/// repository. Once its `.git` went, the folder still there, the lookup from it finds the outer
/// checkout's repository, and the folder is refused as gone; so it is once a `.git` made again
/// holds only the board's folders, a repository no more.
#[test]
fn a_projects_folder_held_to_its_repository_names_no_other_once_its_git_goes() {
    let (outer, inner) = nested("project-held");
    let repository = project::locate_quick(&inner).unwrap().common;
    assert!(project::is_repository(&repository));
    assert_eq!(
        project::held(&inner, None, &repository).unwrap().id,
        "inner"
    );
    let refused = |why: String| {
        assert_eq!(
            why,
            format!(
                "the project's folder {} no longer holds the repository it was served from, {}: \
                 {}",
                inner.display(),
                repository.display(),
                project::GONE
            )
        );
        assert!(symphony_board::board_health::absent(&why), "{why}");
    };

    std::fs::remove_dir_all(inner.join(".git")).unwrap();
    assert_eq!(
        project::at(&inner, None).unwrap().place.common,
        outer.join(".git"),
        "the lookup from the folder walks up"
    );
    refused(project::held(&inner, None, &repository).unwrap_err());

    std::fs::create_dir_all(inner.join(".git").join(symphony_board::folders::TASKS)).unwrap();
    assert_eq!(project::at(&inner, None).unwrap().place.common, repository);
    assert!(!project::is_repository(&repository));
    refused(project::held(&inner, None, &repository).unwrap_err());
}

/// A board's server, stopped when the case ends: one a reach that should start none started all
/// the same.
struct Stopped(symphony_board::server::Home);

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}

/// The Symphony's reaches of the board of a project removed under it (`cli::board`, which starts
/// a board's server, and `cli::running_board`) are refused as gone, and start no server and write
/// nothing in the checkout that held its folder, though a SurrealDB is there to start one.
#[test]
fn the_symphonys_reach_from_a_projects_folder_that_is_gone_starts_no_board() {
    let (outer, inner) = nested("project-reach-gone");
    let surreal = crate::common::surreal();
    let outer_board = Stopped(symphony_board::server::Home::in_common(
        &outer.join(".git"),
        Some(&surreal),
    ));
    std::fs::remove_dir_all(&inner).unwrap();
    let env = symphony_board::cli::Env {
        vars: [
            ("SYMPHONY_SURREAL", surreal.display().to_string()),
            (
                MACHINE_DIR,
                outer
                    .parent()
                    .unwrap()
                    .join("machine")
                    .display()
                    .to_string(),
            ),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect(),
        cwd: inner.clone(),
    };
    type Reach = fn(&symphony_board::cli::Env) -> Result<symphony_board::cli::Reached, String>;
    for (how, reach) in [
        ("board", symphony_board::cli::board as Reach),
        ("running_board", symphony_board::cli::running_board),
    ] {
        let refused = reach(&env)
            .err()
            .unwrap_or_else(|| panic!("{how} reached a board"));
        assert_eq!(
            refused,
            format!(
                "the project's folder {} is gone: {}",
                inner.display(),
                project::GONE
            ),
            "{how}"
        );
    }
    assert_eq!(outer_board.0.recorded(), None, "a board's server started");
    assert!(
        !outer
            .join(".git")
            .join(symphony_board::folders::TASKS)
            .exists(),
        "the outer checkout's board folder was made"
    );
}

#[test]
fn a_checkout_with_no_file_is_named_by_the_registry_else_by_its_main_checkouts_folder() {
    let dir = scratch("project-registry");
    let main = repository(&dir.join("plain main"));
    let machine = machine(&dir, &[("other", &dir.join("elsewhere")), ("plain", &main)]);
    assert_eq!(id(&main, Some(&machine)), "plain");
    assert_eq!(id(&main, None), "plain main");
    // An empty file is a plain project: the registry still names it.
    write(&main.join(FILE), "");
    assert_eq!(id(&main, Some(&machine)), "plain");
    // A registry that does not parse names nothing.
    write(&machine.join(project::REGISTRY), "{ not json");
    assert_eq!(id(&main, Some(&machine)), "plain main");
    // Nor does a missing one.
    assert_eq!(id(&main, Some(&dir.join("no machine"))), "plain main");
}

#[test]
fn a_worktree_is_its_main_checkouts_project_on_its_board_with_or_without_the_file() {
    let dir = scratch("project-worktree");
    let main = repository(&dir.join("main"));
    write(&main.join(FILE), "[project]\nid = \"alpha\"\n");
    git(&main, &["add", FILE]);
    git(&main, &["commit", "-q", "-m", "the project file"]);
    let with = dir.join("with-file");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "with",
            with.to_str().unwrap(),
        ],
    );
    let found = project::find(&with, None).unwrap();
    assert_eq!(found.id, "alpha");
    assert_eq!(found.place.root, with.canonicalize().unwrap());
    assert_eq!(found.place.common, main.join(".git"));
    // A worktree on a branch from before the file: the registry names its main checkout.
    let without = dir.join("without-file");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "old",
            without.to_str().unwrap(),
            "HEAD~1",
        ],
    );
    let machine = machine(&dir, &[("alpha", &main)]);
    let found = project::find(&without, Some(&machine)).unwrap();
    assert_eq!(found.id, "alpha");
    assert_eq!(found.place.file, None);
    assert_eq!(found.place.root, without.canonicalize().unwrap());
    assert_eq!(found.place.common, main.join(".git"));
    assert_eq!(id(&without, None), "main");
}

#[test]
fn a_checkout_whose_repository_is_kept_elsewhere_is_named_by_the_entry_in_that_repository() {
    let dir = scratch("project-separate");
    let checkout = dir.join("separate");
    std::fs::create_dir_all(&checkout).unwrap();
    let kept = dir.join("kept.git");
    git(
        &checkout,
        &[
            "init",
            "-q",
            "-b",
            "main",
            "--separate-git-dir",
            kept.to_str().unwrap(),
        ],
    );
    let place = project::locate(&checkout).unwrap();
    assert_eq!(
        (place.root.as_path(), place.common.as_path()),
        (checkout.as_path(), kept.as_path())
    );
    // The registry's entry for a root in the same repository names it, as no main checkout does.
    let machine = machine(
        &dir,
        &[("other", &dir.join("elsewhere")), ("kept", &checkout)],
    );
    assert_eq!(id(&checkout, Some(&machine)), "kept");
    assert_eq!(id(&checkout, None), "separate");
}

#[test]
fn a_file_naming_no_id_is_named_like_a_checkout_with_none() {
    let dir = scratch("project-no-id");
    let main = repository(&dir.join("named"));
    write(&main.join(FILE), "[project]\nname = \"Named\"\n");
    assert_eq!(id(&main, None), "named");
    write(&main.join(FILE), "[project]\nid = \"\"\n");
    assert_eq!(id(&main, None), "named");
    write(&main.join(FILE), "not toml [");
    assert_eq!(id(&main, None), "named");
}

/// The board's guide is the one the project's file names, else the default: a file naming none, an
/// empty one, or a file that does not read or parse.
#[test]
fn the_board_guide_is_the_files_else_the_default() {
    let dir = scratch("project-guide");
    let main = repository(&dir.join("main"));
    let guide = || project::find(&main, None).unwrap().guide();
    assert_eq!(guide(), project::GUIDE, "no file");
    for (text, named) in [
        (
            "[project]\nid = \"alpha\"\n[board]\nguide = \"handbook/BOARD.md\"\n",
            "handbook/BOARD.md",
        ),
        ("[project]\nid = \"alpha\"\n", project::GUIDE),
        ("[board]\non_review = \"true\"\n", project::GUIDE),
        ("[board]\nguide = \"\"\n", project::GUIDE),
        ("[board]\nguide = 7\n", project::GUIDE),
        (
            "[board]\nguide = \"docs/TASKS.md\"\nnot toml [",
            project::GUIDE,
        ),
    ] {
        write(&main.join(FILE), text);
        assert_eq!(guide(), named, "{text}");
    }
    // The file found, then gone before the guide is read.
    write(&main.join(FILE), "[board]\nguide = \"docs/TASKS.md\"\n");
    let found = project::find(&main, None).unwrap();
    assert_eq!(found.guide(), "docs/TASKS.md");
    std::fs::remove_file(main.join(FILE)).unwrap();
    assert_eq!(found.guide(), project::GUIDE);
}

/// A board whose server cannot start says where its server comes from in the board guide the
/// project's file names: with no store yet, the board is made on PostgreSQL, which is missing;
/// with the board's SurrealDB store there, it is a board that cannot be served, and SurrealDB is
/// what is missing, never PostgreSQL.
#[test]
fn a_board_that_cannot_be_served_names_the_guide_the_project_file_names() {
    let main = repository(&scratch("project-guide-refusal").join("main"))
        .canonicalize()
        .unwrap();
    let programs = main.join("no-postgresql");
    write(
        &main.join(FILE),
        &format!(
            "[project]\nid = \"lamp-works\"\n[board]\nguide = \"handbook/BOARD.md\"\n\
             postgresql = {:?}\n",
            programs.display().to_string()
        ),
    );
    let missing = main.join("no-surreal");
    let vars = [(
        "SYMPHONY_SURREAL".to_string(),
        missing.display().to_string(),
    )];
    let env = symphony_board::cli::Env {
        vars: vars.into_iter().collect(),
        cwd: main.clone(),
    };
    let ran = crate::run(&env, &["list"], "");
    let home = main.join(".git").join(symphony_board::folders::TASKS);
    assert_eq!(ran.code, 2, "{}", ran.err);
    assert!(
        ran.err.starts_with(&format!(
            "✖ nothing was made for the board in {}, which is not made yet and is made on \
             PostgreSQL: PostgreSQL is not at {} (postgres is missing or not executable); \
             install PostgreSQL 18 or later",
            home.display(),
            programs.display()
        )),
        "{}",
        ran.err
    );
    assert!(
        ran.err.ends_with(" (handbook/BOARD.md#where-it-lives)\n"),
        "{}",
        ran.err
    );
    assert!(!ran.err.contains("SurrealDB"), "{}", ran.err);
    let data = main
        .join(".git")
        .join(symphony_board::folders::TASKS)
        .join("data");
    std::fs::create_dir_all(&data).unwrap();
    let ran = crate::run(&env, &["list"], "");
    assert_eq!(
        (ran.code, ran.err),
        (
            2,
            format!(
                "✖ the board's store at {} cannot be served: SurrealDB {}, which serves it, is not \
                 at {}, where `SYMPHONY_SURREAL` names it (handbook/BOARD.md#where-it-lives)\n",
                data.display(),
                symphony_board::server::SURREAL_VERSION,
                missing.display()
            )
        )
    );
}

#[test]
fn a_folder_in_no_repository_has_no_board() {
    let dir = scratch("project-none");
    let why = project::find(&dir, None).unwrap_err();
    assert!(why.contains("is not inside a git repository"), "{why}");
    write(&dir.join("loose").join(FILE), "[project]\nid = \"loose\"\n");
    let why = project::locate(&dir.join("loose")).unwrap_err();
    assert!(why.contains("is not inside a git repository"), "{why}");
}

#[test]
fn quick_lookup_rejects_empty_checkout_destinations_but_keeps_blank_commondir() {
    let dir = scratch("project-marker-destinations");
    let outer = repository(&dir.join("outer"));
    for (name, marker) in [("empty", "gitdir:"), ("spaces", "gitdir:   \n")] {
        let inner = outer.join(name);
        write(&inner.join(".git"), marker);
        write(
            &inner.join("component/symphony.toml"),
            "[project]\nid='inner'\n",
        );
        for cwd in [&inner, &inner.join("component")] {
            assert_eq!(project::locate_quick(cwd), None, "{name}: {cwd:?}");
            let error = project::locate(cwd).unwrap_err();
            assert!(error.contains("not inside a git repository"), "{error}");
        }
    }
    let linked = outer.join("linked");
    write(&linked.join(".git"), "gitdir: ../admin\n");
    write(&outer.join("admin/commondir"), " \n");
    let expected = Place {
        root: linked.clone(),
        file: None,
        common: outer.join("admin"),
    };
    assert_eq!(project::locate_quick(&linked), Some(expected.clone()));
    assert_eq!(project::locate(&linked).unwrap(), expected);
}

#[test]
fn the_machine_folder_is_the_one_a_test_names_else_the_platforms() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(named, _)| *named == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    assert_eq!(
        project::machine_folder(vars(&[(MACHINE_DIR, "/tmp/m"), ("HOME", "/h")]), true).unwrap(),
        Path::new("/tmp/m")
    );
    assert_eq!(
        project::machine_folder(vars(&[("HOME", "/h")]), true).unwrap(),
        Path::new("/h/Library/Application Support/Symphony")
    );
    assert_eq!(
        project::machine_folder(vars(&[("HOME", "/h"), ("XDG_DATA_HOME", "/d")]), false).unwrap(),
        Path::new("/d/symphony")
    );
    assert_eq!(
        project::machine_folder(vars(&[("HOME", "/h")]), false).unwrap(),
        Path::new("/h/.local/share/symphony")
    );
    let relative = project::machine_folder(vars(&[(MACHINE_DIR, "m")]), true).unwrap_err();
    assert!(relative.contains("not an absolute folder"), "{relative}");
    let homeless = project::machine_folder(vars(&[]), false).unwrap_err();
    assert!(homeless.contains("needs HOME"), "{homeless}");
}
