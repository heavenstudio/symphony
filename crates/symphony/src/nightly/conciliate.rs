//! Conciliation, as read until the owner says otherwise (docs/RELEASES.md): each project the
//! machine serves that builds on the Symphony (its project file names plugins, `projects.json`)
//! built and tested with its Symphony moved to the candidate. It runs in a scratch copy of the
//! project's default branch as committed, never in its checkout or on its board: a repository
//! sharing the checkout's objects, at `NIGHTLY-conciliate/ID` beside the nightly's own checkout,
//! marked as the nightly's when it is made ([`place::copy_at`]) and refreshed each night with its
//! build folder kept. Every package of the candidate's workspace is patched in over each git
//! source the project takes one from (`cargo --config patch…`, so no file of the project changes),
//! and its packages that depend on one are tested with `cargo test`, with a machine folder of the
//! run's own holding a copy of the registry, so no test of the project's reaches the machine's.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::checks::Step;
use super::git::{commit, git_until};
use super::group::{Bound, Failed, output_until};
use super::place::{self, BUILT, COPY_MARK, Copy};
use crate::children::command;
use crate::machine::Registry;
use crate::project::Project;

/// A project to conciliate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Consumer {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    /// Its default branch, which its scratch copy takes.
    pub(crate) branch: String,
}

/// The projects of `registry` to conciliate, the repository whose common dir is `ours` aside, and
/// a line for each one left out and why. A project whose file names no plugins builds nothing on
/// the Symphony that a release could break, and is passed over without a word.
pub(crate) fn consumers(registry: &Registry, ours: &Path) -> (Vec<Consumer>, Vec<String>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for entry in &registry.projects {
        if entry.plugins.is_empty() {
            continue;
        }
        if !entry.root.is_dir() {
            skipped.push(format!(
                "{}: its checkout {} is gone",
                entry.id,
                entry.root.display()
            ));
            continue;
        }
        match symphony_board::git::checkout_common_dir(&entry.root) {
            Some(common) if common == ours => {}
            Some(_) => found.push(Consumer {
                id: entry.id.clone(),
                root: entry.root.clone(),
                branch: Project::of(&entry.root).default_branch.clone(),
            }),
            None => skipped.push(format!(
                "{}: {} is not a git checkout",
                entry.id,
                entry.root.display()
            )),
        }
    }
    (found, skipped)
}

/// Make an empty repository at `scratch`, marked as the nightly's copy of `consumer` and sharing
/// its checkout's objects, before anything is fetched into it.
fn make(consumer: &Consumer, scratch: &Path, until: Bound<'_>) -> Result<(), String> {
    fs::create_dir_all(scratch).map_err(|e| crate::text::os_error(&e, scratch))?;
    git_until(scratch, &["init", "--quiet"], until)?;
    let marker = scratch.join(".git").join(COPY_MARK);
    fs::write(&marker, format!("{}\n", consumer.root.display()))
        .map_err(|e| crate::text::os_error(&e, &marker))?;
    let common = git_until(
        &consumer.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        until,
    )?;
    let alternates = scratch.join(".git/objects/info/alternates");
    fs::write(&alternates, format!("{common}/objects\n"))
        .map_err(|e| crate::text::os_error(&e, &alternates))
}

/// Bring the scratch copy at `scratch` to `consumer`'s default branch as committed, by `until`: a
/// repository of the nightly's sharing its checkout's objects the first time, fetched and reset
/// later, untracked and ignored files gone but its build folder (`.target/`), which is cleared once
/// it is [`place::BUILD_KEPT_DAYS`] old at `now`. A folder there that is not the nightly's copy of
/// it is refused and left as it is. Its commit, and what it noted on the way.
pub(crate) fn prepare(
    consumer: &Consumer,
    scratch: &Path,
    until: Bound<'_>,
    now: f64,
) -> Result<(String, Vec<String>), String> {
    let source = consumer.root.to_string_lossy().into_owned();
    let branch = format!("refs/heads/{}", consumer.branch);
    let refresh = |scratch: &Path| {
        git_until(
            scratch,
            &["fetch", "--quiet", "--no-tags", &source, &branch],
            until,
        )?;
        git_until(
            scratch,
            &["checkout", "--quiet", "--force", "--detach", "FETCH_HEAD"],
            until,
        )?;
        git_until(scratch, &["clean", "-ffdxq", "-e", "/.target/"], until)?;
        commit(scratch, "HEAD")
    };
    let mut noted = Vec::new();
    match place::copy_at(scratch, &consumer.root)? {
        Copy::Ours => match refresh(scratch) {
            Ok(at) => {
                let stamp = scratch.join(".git").join(BUILT);
                noted.extend(place::prune_build(&scratch.join(".target"), &stamp, now));
                return Ok((at, noted));
            }
            Err(error) => {
                // The nightly's own copy, which no longer refreshes: made again.
                fs::remove_dir_all(scratch).map_err(|e| crate::text::os_error(&e, scratch))?;
                noted.push(format!("made {} again: {error}", scratch.display()));
            }
        },
        Copy::None | Copy::Empty => {}
    }
    make(consumer, scratch, until)?;
    let at = refresh(scratch)?;
    let _ = fs::write(scratch.join(".git").join(BUILT), format!("{now}\n"));
    Ok((at, noted))
}

/// `cargo metadata --no-deps` of the workspace at `dir`, with the toolchain its own pin names, by
/// `until`.
fn metadata(dir: &Path, until: Bound<'_>) -> Result<Value, String> {
    let done = output_until(
        command("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(dir)
            .env_remove("RUSTUP_TOOLCHAIN"),
        until,
    )
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run cargo in {}: {e}", dir.display()),
        Failed::Late(took) => format!(
            "cargo metadata in {} did not end within {} s and was stopped",
            dir.display(),
            took.as_secs()
        ),
        Failed::Interrupted(took) => format!(
            "cargo metadata in {} was stopped after {} s: the run was told to stop",
            dir.display(),
            took.as_secs()
        ),
    })?;
    if !done.status.success() {
        return Err(format!(
            "cargo metadata failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    serde_json::from_slice(&done.stdout)
        .map_err(|e| format!("cargo metadata in {} said no JSON: {e}", dir.display()))
}

/// The packages a workspace's metadata lists: each name with its folder.
pub(crate) fn packages(metadata: &Value) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = (metadata["packages"].as_array().into_iter())
        .flatten()
        .filter_map(|package| {
            let name = package["name"].as_str()?;
            let folder = Path::new(package["manifest_path"].as_str()?).parent()?;
            Some((name.to_string(), folder.to_path_buf()))
        })
        .collect();
    found.sort();
    found
}

/// What a project takes from the Symphony: the git sources it names a candidate package from, and
/// its own packages that depend on one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) sources: Vec<String>,
    pub(crate) packages: Vec<String>,
}

/// The URL a dependency's git source names, without the branch, tag or commit it asks for.
fn git_url(source: &str) -> Option<&str> {
    let url = source.strip_prefix("git+")?;
    Some(url.split(['?', '#']).next().unwrap_or(url))
}

/// The plan for a project whose workspace metadata is `metadata`, against the candidate's
/// packages `candidate`.
pub(crate) fn plan(metadata: &Value, candidate: &[(String, PathBuf)]) -> Plan {
    let members: Vec<&str> = (metadata["workspace_members"].as_array().into_iter())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut plan = Plan::default();
    for package in (metadata["packages"].as_array().into_iter()).flatten() {
        if !package["id"]
            .as_str()
            .is_some_and(|id| members.contains(&id))
        {
            continue;
        }
        let mut takes = false;
        for dependency in (package["dependencies"].as_array().into_iter()).flatten() {
            let named = dependency["name"].as_str().unwrap_or("");
            let Some(url) = dependency["source"].as_str().and_then(git_url) else {
                continue;
            };
            if candidate.iter().any(|(name, _)| name == named) {
                takes = true;
                if !plan.sources.iter().any(|source| source == url) {
                    plan.sources.push(url.to_string());
                }
            }
        }
        if takes && let Some(name) = package["name"].as_str() {
            plan.packages.push(name.to_string());
        }
    }
    plan.sources.sort();
    plan.packages.sort();
    plan
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The command testing `plan`'s packages with every package of `candidate` patched in over each
/// of its sources.
pub(crate) fn argv(plan: &Plan, candidate: &[(String, PathBuf)]) -> Vec<String> {
    let mut argv: Vec<String> = ["cargo", "test", "--no-fail-fast"]
        .map(String::from)
        .to_vec();
    for source in &plan.sources {
        for (name, folder) in candidate {
            argv.push("--config".into());
            argv.push(format!(
                "patch.{}.{}.path={}",
                toml_string(source),
                toml_string(name),
                toml_string(&folder.to_string_lossy())
            ));
        }
    }
    for package in &plan.packages {
        argv.push("-p".into());
        argv.push(package.clone());
    }
    argv
}

/// `argv` as one shell line, each word quoted that needs it.
fn line(argv: &[String]) -> String {
    let plain = |word: &str| {
        !word.is_empty()
            && word
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b))
    };
    argv.iter()
        .map(|word| {
            if plain(word) {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where a project is conciliated: its scratch copy, the candidate's checkout, the run's machine
/// folder, the end of the run's time and the run's clock.
pub(crate) struct Against<'a> {
    pub(crate) scratch: &'a Path,
    pub(crate) checkout: &'a Path,
    pub(crate) machine: &'a Path,
    pub(crate) until: Bound<'a>,
    pub(crate) now: f64,
}

/// The check conciliating `consumer` `against` the candidate: none when the project takes no
/// package of the candidate's from git, said why; and what it noted on the way.
pub(crate) fn step(
    consumer: &Consumer,
    against: &Against<'_>,
) -> Result<(Result<Step, String>, Vec<String>), String> {
    let scratch = against.scratch;
    let (at, noted) = prepare(consumer, scratch, against.until, against.now)?;
    let candidate = packages(&metadata(against.checkout, against.until)?);
    let plan = plan(&metadata(scratch, against.until)?, &candidate);
    if plan.packages.is_empty() {
        let none = format!(
            "{}: none of its packages takes one of the Symphony's from git",
            consumer.id
        );
        return Ok((Err(none), noted));
    }
    let argv = argv(&plan, &candidate);
    let step = Step {
        name: format!("conciliate-{}", consumer.id),
        command: format!("{} (in {} at {at})", line(&argv), scratch.display()),
        argv,
        cwd: Some(scratch.to_path_buf()),
        set: vec![
            (
                "CARGO_TARGET_DIR".into(),
                scratch.join(".target").to_string_lossy().into_owned(),
            ),
            (
                "SYMPHONY_MACHINE_DIR".into(),
                against.machine.to_string_lossy().into_owned(),
            ),
        ],
        unset: vec!["RUSTUP_TOOLCHAIN".into()],
    };
    Ok((Ok(step), noted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Entry;
    use crate::test_tree::Tree;
    use serde_json::json;
    use std::time::Instant;

    fn entry(id: &str, root: &Path, plugins: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            root: root.to_path_buf(),
            manifest: None,
            plugins: plugins.iter().map(|p| (*p).to_string()).collect(),
            pin: 1,
            schema: 16,
        }
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        super::super::git::git(dir, args).unwrap_or_else(|e| panic!("{e}"))
    }

    fn soon() -> Bound<'static> {
        Bound::until(Instant::now() + std::time::Duration::from_secs(120))
    }

    const NOW: f64 = 1_800_000_000.0;
    const DAY: f64 = 86_400.0;

    /// `prepare` within two minutes at `now`: the commit.
    fn prepared(consumer: &Consumer, scratch: &Path, now: f64) -> Result<String, String> {
        prepare(consumer, scratch, soon(), now).map(|(at, _)| at)
    }

    fn repo(tree: &Tree, name: &str) -> PathBuf {
        let root = tree.path(name);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.email", "t@example.com"]);
        git_ok(&root, &["config", "user.name", "T"]);
        root
    }

    fn commit_file(root: &Path, file: &str, text: &str) -> String {
        fs::write(root.join(file), text).unwrap();
        git_ok(root, &["add", file]);
        git_ok(root, &["commit", "-q", "-m", file]);
        git_ok(root, &["rev-parse", "HEAD"])
    }

    #[test]
    fn the_projects_to_conciliate_name_plugins_and_are_not_this_one() {
        let tree = Tree::new();
        let ours = repo(&tree, "ours");
        let theirs = repo(&tree, "theirs");
        let plain = repo(&tree, "plain");
        tree.write(
            "theirs/symphony.toml",
            "[project]\ndefault_branch = \"trunk\"\n",
        );
        let not_git = tree.path("folder");
        fs::create_dir_all(&not_git).unwrap();
        let registry = Registry {
            projects: vec![
                entry("ours", &ours, &["a"]),
                entry("theirs", &theirs, &["b"]),
                entry("plain", &plain, &[]),
                entry("gone", &tree.path("gone"), &["c"]),
                entry("loose", &not_git, &["d"]),
            ],
        };
        let (found, skipped) = consumers(&registry, &ours.join(".git"));
        assert_eq!(
            found,
            [Consumer {
                id: "theirs".into(),
                root: theirs,
                branch: "trunk".into()
            }]
        );
        assert_eq!(
            skipped,
            [
                format!("gone: its checkout {} is gone", tree.path("gone").display()),
                format!("loose: {} is not a git checkout", not_git.display()),
            ]
        );
    }

    #[test]
    fn the_scratch_copy_takes_the_projects_committed_branch_and_keeps_its_build_folder() {
        let tree = Tree::new();
        let source = repo(&tree, "source");
        let first = commit_file(&source, "a.txt", "one");
        tree.write("source/uncommitted.txt", "never copied");
        let consumer = Consumer {
            id: "source".into(),
            root: source.clone(),
            branch: "main".into(),
        };
        let scratch = tree.path("wt/nightly-conciliate/source");
        assert_eq!(prepared(&consumer, &scratch, NOW).unwrap(), first);
        assert_eq!(fs::read_to_string(scratch.join("a.txt")).unwrap(), "one");
        assert!(!scratch.join("uncommitted.txt").exists());
        assert_eq!(
            fs::read_to_string(scratch.join(".git").join(COPY_MARK)).unwrap(),
            format!("{}\n", source.display()),
            "marked as the nightly's copy of the project"
        );
        assert_eq!(
            fs::read_to_string(scratch.join(".git/objects/info/alternates")).unwrap(),
            format!("{}/.git/objects\n", source.display()),
            "sharing the project's objects"
        );

        tree.write("wt/nightly-conciliate/source/.target/kept", "build");
        tree.write("wt/nightly-conciliate/source/stray.txt", "left by a build");
        tree.write("wt/nightly-conciliate/source/a.txt", "changed in the copy");
        let third = commit_file(&source, "a.txt", "two");
        git_ok(&source, &["checkout", "-q", "-b", "elsewhere"]);
        let checked_out = commit_file(&source, "b.txt", "on another branch");
        assert_ne!(checked_out, third);
        assert_eq!(prepared(&consumer, &scratch, NOW + DAY).unwrap(), third);
        assert_eq!(fs::read_to_string(scratch.join("a.txt")).unwrap(), "two");
        assert!(
            !scratch.join("b.txt").exists(),
            "the default branch, not the one checked out"
        );
        assert!(!scratch.join("stray.txt").exists());
        assert_eq!(
            fs::read_to_string(scratch.join(".target/kept")).unwrap(),
            "build"
        );
        assert_eq!(
            git_ok(&source, &["status", "--porcelain"]),
            "?? uncommitted.txt",
            "the project's checkout is as it was"
        );

        // A week of builds later the build folder goes.
        let (at, noted) = prepare(&consumer, &scratch, soon(), NOW + 8.0 * DAY).unwrap();
        assert_eq!(at, third);
        assert!(!scratch.join(".target").exists());
        assert_eq!(
            noted,
            [format!(
                "cleared {}, built on for 7 days or more",
                scratch.join(".target").display()
            )]
        );

        fs::write(scratch.join(".git/HEAD"), "not a ref").unwrap();
        let (at, noted) = prepare(&consumer, &scratch, soon(), NOW + 8.0 * DAY).unwrap();
        assert_eq!(at, third, "a copy that no longer refreshes is made again");
        assert!(
            noted[0].starts_with(&format!("made {} again: ", scratch.display())),
            "{noted:?}"
        );
    }

    #[test]
    fn a_folder_in_a_copys_place_that_the_nightly_did_not_make_is_left_as_it_is() {
        let tree = Tree::new();
        let source = repo(&tree, "source");
        commit_file(&source, "a.txt", "one");
        let consumer = Consumer {
            id: "source".into(),
            root: source.clone(),
            branch: "main".into(),
        };
        // Someone's repository, with work of its own.
        let theirs = repo(&tree, "copies/source");
        commit_file(&theirs, "theirs.txt", "committed");
        tree.write("copies/source/precious.txt", "uncommitted");
        let refused = prepared(&consumer, &theirs, NOW).unwrap_err();
        assert!(
            refused.contains("is not the nightly's copy of")
                && refused.contains("(no marker of the nightly's)"),
            "{refused}"
        );
        assert_eq!(
            fs::read_to_string(theirs.join("precious.txt")).unwrap(),
            "uncommitted"
        );
        assert_eq!(
            git_ok(&theirs, &["status", "--porcelain"]),
            "?? precious.txt"
        );

        // A copy of another project's is not this one's.
        let other = tree.path("copies/other");
        let elsewhere = Consumer {
            root: theirs.clone(),
            ..consumer.clone()
        };
        prepared(&elsewhere, &other, NOW).unwrap();
        let refused = prepared(&consumer, &other, NOW).unwrap_err();
        assert!(
            refused.contains("(no marker of the nightly's)"),
            "{refused}"
        );

        // A link, even to the nightly's own copy, is never followed.
        let link = tree.path("copies/link");
        std::os::unix::fs::symlink(&other, &link).unwrap();
        let refused = prepared(&elsewhere, &link, NOW).unwrap_err();
        assert!(refused.contains("(a link)"), "{refused}");
        assert!(other.join(".git").is_dir());

        let file = tree.write("copies/file", "a file");
        assert!(
            prepared(&consumer, &file, NOW)
                .unwrap_err()
                .contains("(not a folder)")
        );

        // An empty folder holds nothing to lose: it is taken.
        let empty = tree.path("copies/empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(prepared(&consumer, &empty, NOW).is_ok());
    }

    #[test]
    fn a_copy_whose_project_left_the_registry_goes_and_nothing_else_does() {
        let tree = Tree::new();
        let source = repo(&tree, "source");
        commit_file(&source, "a.txt", "one");
        let copies = tree.path("copies");
        for id in ["kept", "left"] {
            let consumer = Consumer {
                id: id.into(),
                root: source.clone(),
                branch: "main".into(),
            };
            prepared(&consumer, &copies.join(id), NOW).unwrap();
        }
        let theirs = repo(&tree, "copies/theirs");
        commit_file(&theirs, "theirs.txt", "committed");
        tree.write("copies/notes.txt", "a file");
        std::os::unix::fs::symlink(copies.join("kept"), copies.join("link")).unwrap();
        let removed = place::prune_copies(&copies, &["kept".to_string()]);
        assert_eq!(
            removed,
            [format!(
                "removed the copy {}: its project is no longer registered",
                copies.join("left").display()
            )]
        );
        assert_eq!(
            tree.names("copies"),
            ["kept", "link", "notes.txt", "theirs"]
        );
        assert!(place::prune_copies(&tree.path("none"), &[]).is_empty());
    }

    fn metadata_of(packages: Value, members: &[&str]) -> Value {
        json!({"packages": packages, "workspace_members": members})
    }

    #[test]
    fn the_plan_patches_each_symphony_source_and_tests_the_packages_that_take_one() {
        let candidate = vec![
            ("symphony".to_string(), PathBuf::from("/c/crates/symphony")),
            (
                "symphony-board".to_string(),
                PathBuf::from("/c/crates/tasks"),
            ),
        ];
        let metadata = metadata_of(
            json!([
                {"id": "host 0.1", "name": "host", "manifest_path": "/p/apps/host/Cargo.toml", "dependencies": [
                    {"name": "symphony", "source": "git+https://github.com/o/symphony?branch=main"},
                    {"name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index"},
                ]},
                {"id": "tasks 0.1", "name": "tasks", "manifest_path": "/p/apps/tasks/Cargo.toml", "dependencies": [
                    {"name": "symphony-board", "source": "git+https://github.com/o/symphony?branch=main#abc"},
                    {"name": "symphony-board", "source": "git+ssh://git@github.com/o/symphony.git"},
                ]},
                {"id": "game 0.1", "name": "game", "manifest_path": "/p/game/Cargo.toml", "dependencies": [
                    {"name": "symphony-like", "source": "git+https://example.com/other"},
                    {"name": "symphony", "source": null, "path": "/elsewhere"},
                ]},
                {"id": "dep 0.1", "name": "outside", "manifest_path": "/x/Cargo.toml", "dependencies": [
                    {"name": "symphony", "source": "git+https://github.com/o/symphony?branch=main"},
                ]},
            ]),
            &["host 0.1", "tasks 0.1", "game 0.1"],
        );
        let planned = plan(&metadata, &candidate);
        assert_eq!(
            planned,
            Plan {
                sources: vec![
                    "https://github.com/o/symphony".into(),
                    "ssh://git@github.com/o/symphony.git".into()
                ],
                packages: vec!["host".into(), "tasks".into()],
            }
        );
        assert_eq!(
            argv(&planned, &candidate),
            [
                "cargo",
                "test",
                "--no-fail-fast",
                "--config",
                r#"patch."https://github.com/o/symphony"."symphony".path="/c/crates/symphony""#,
                "--config",
                r#"patch."https://github.com/o/symphony"."symphony-board".path="/c/crates/tasks""#,
                "--config",
                r#"patch."ssh://git@github.com/o/symphony.git"."symphony".path="/c/crates/symphony""#,
                "--config",
                r#"patch."ssh://git@github.com/o/symphony.git"."symphony-board".path="/c/crates/tasks""#,
                "-p",
                "host",
                "-p",
                "tasks",
            ]
        );
        assert_eq!(
            plan(&metadata_of(json!([]), &[]), &candidate),
            Plan::default()
        );
    }

    #[test]
    fn a_workspaces_packages_are_named_with_their_folders() {
        let metadata = metadata_of(
            json!([
                {"name": "b", "manifest_path": "/w/crates/b/Cargo.toml"},
                {"name": "a", "manifest_path": "/w/Cargo.toml"},
                {"name": "no-path"},
            ]),
            &[],
        );
        assert_eq!(
            packages(&metadata),
            [
                ("a".to_string(), PathBuf::from("/w")),
                ("b".to_string(), PathBuf::from("/w/crates/b"))
            ]
        );
        assert!(packages(&json!({})).is_empty());
    }

    #[test]
    fn a_command_line_quotes_only_the_words_that_need_it() {
        assert_eq!(
            line(&[
                "cargo".into(),
                "-p".into(),
                "a b".into(),
                "it's".into(),
                String::new()
            ]),
            r"cargo -p 'a b' 'it'\''s' ''"
        );
        assert_eq!(toml_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn a_dependencys_git_url_drops_what_it_asks_for() {
        assert_eq!(
            git_url("git+https://h/o/r?branch=main#abc"),
            Some("https://h/o/r")
        );
        assert_eq!(
            git_url("git+file:///tmp/x.git#abc"),
            Some("file:///tmp/x.git")
        );
        assert_eq!(git_url("registry+https://h"), None);
    }
}
