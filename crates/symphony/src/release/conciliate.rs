//! Conciliation, as read until the owner says otherwise (docs/RELEASES.md): each project the
//! machine serves that builds on the Symphony (its project file names plugins, `projects.json`)
//! built and tested with its Symphony moved to the candidate. It runs in a scratch copy of the
//! project's default branch as committed, never in its checkout or on its board: a repository
//! sharing the checkout's objects, at `RELEASE-conciliate/ID` beside the release run's own
//! checkout, marked as the release run's when it is made ([`place::copy_at`]) and refreshed each
//! run. Its lock is moved to the candidate's commit over each git source the project takes a
//! candidate package from (`cargo update -p 'URL#NAME' --precise COMMIT`, so no file of the project
//! but its `Cargo.lock` changes), from the source the project fetches the Symphony from, as a
//! refresh takes the release's, and its packages that depend on one are tested with `cargo test`,
//! with a machine folder of the run's own holding a copy of the registry, so no test of the
//! project's reaches the machine's. Every cargo the project's tests start in the copy, `--locked`
//! and `--offline` or not, reads that lock and so builds the candidate too. A patch on the command
//! line (`cargo --config patch…`) would reach only the cargo it is given, and leave those a lock
//! they resolve again, to whatever commit cargo's cache last knew the source's branch at. It builds
//! in a folder of the release run's own in the copy's `.git` ([`place::COPY_TARGET`], kept from run
//! to run), named to cargo alone (`--target-dir`): the project's own build folder is its
//! tooling's, which its tests run, and which may empty it while a test runs from it, as a folder no
//! run of that tooling made.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::checks::Step;
use super::git::{commit, git_until};
use super::group::{Bound, Failed, output_until};
use super::place::{self, BUILT, COPY_MARK, COPY_TARGET, Copy};
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

/// Make an empty repository at `scratch`, marked as the release run's copy of `consumer` and sharing
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

/// Where the conciliation of the copy at `scratch` builds: [`COPY_TARGET`] in its `.git`.
pub(crate) fn target(scratch: &Path) -> PathBuf {
    scratch.join(".git").join(COPY_TARGET)
}

/// Bring the scratch copy at `scratch` to `consumer`'s default branch as committed, by `until`: a
/// repository of the release run's sharing its checkout's objects the first time, fetched and reset
/// later, every untracked and ignored file gone, the project's own build folder too. Its
/// conciliation's build folder, in its `.git` ([`target`]), stays, and is cleared once it is
/// [`place::BUILD_KEPT_DAYS`] old at `now`. A folder there that is not the release run's copy of it
/// is refused and left as it is. Its commit, and what it noted on the way.
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
        git_until(scratch, &["clean", "-ffdxq"], until)?;
        commit(scratch, "HEAD")
    };
    let mut noted = Vec::new();
    match place::copy_at(scratch, &consumer.root)? {
        Copy::Ours => match refresh(scratch) {
            Ok(at) => {
                let stamp = scratch.join(".git").join(BUILT);
                noted.extend(place::prune_build(&target(scratch), &stamp, now));
                return Ok((at, noted));
            }
            Err(error) => {
                // The release run's own copy, which no longer refreshes: made again.
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

/// `cargo ARGS` in the workspace at `dir`, with the toolchain its own pin names and no build folder
/// the run inherited ([`UNSET`]), by `until`: its standard output, or why it failed, said as
/// `cargo WHAT` (its first argument).
fn cargo(dir: &Path, args: &[&str], until: Bound<'_>) -> Result<Vec<u8>, String> {
    let what = args.first().copied().unwrap_or_default();
    let mut cargo = command("cargo");
    cargo.args(args).current_dir(dir);
    for name in UNSET {
        cargo.env_remove(name);
    }
    let done = output_until(&mut cargo, until).map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run cargo in {}: {e}", dir.display()),
        Failed::Late(took) => format!(
            "cargo {what} in {} did not end within {} s and was stopped",
            dir.display(),
            took.as_secs()
        ),
        Failed::Interrupted(took) => format!(
            "cargo {what} in {} was stopped after {} s: the run was told to stop",
            dir.display(),
            took.as_secs()
        ),
    })?;
    if !done.status.success() {
        return Err(format!(
            "cargo {what} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(done.stdout)
}

/// `cargo metadata --no-deps` of the workspace at `dir`, with the toolchain its own pin names, by
/// `until`.
pub(crate) fn metadata(dir: &Path, until: Bound<'_>) -> Result<Value, String> {
    let out = cargo(
        dir,
        &["metadata", "--no-deps", "--format-version", "1"],
        until,
    )?;
    serde_json::from_slice(&out)
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

/// What a project takes from the Symphony: each git source it names a candidate package from, with
/// the first such package its members name (by which `cargo update` moves every package it locks
/// from that source), and its own packages that depend on one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) sources: Vec<(String, String)>,
    pub(crate) packages: Vec<String>,
}

impl Plan {
    /// The URL of each of its sources.
    pub(crate) fn urls(&self) -> Vec<String> {
        self.sources.iter().map(|(url, _)| url.clone()).collect()
    }
}

/// The URL a dependency's git source names, without the branch, tag or commit it asks for.
pub(crate) fn git_url(source: &str) -> Option<&str> {
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
                if !plan.sources.iter().any(|(source, _)| source == url) {
                    plan.sources.push((url.to_string(), named.to_string()));
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

/// The command moving every package a project locks from the git source `url` to the candidate's
/// `commit`, by `name`, one package it takes from there: the source's packages share one commit.
pub(crate) fn update_argv(url: &str, name: &str, commit: &str) -> Vec<String> {
    [
        "cargo",
        "update",
        "-p",
        &format!("{url}#{name}"),
        "--precise",
        commit,
    ]
    .map(String::from)
    .to_vec()
}

/// The copy at `scratch`'s lock moved to the candidate's `commit` over each source of `plan`, by
/// `until`: the commands that moved it, or why one could not (the candidate's commit is not in
/// that source, say), which leaves nothing to test.
fn move_lock(
    scratch: &Path,
    plan: &Plan,
    commit: &str,
    until: Bound<'_>,
) -> Result<Vec<String>, String> {
    let mut moved = Vec::new();
    for (url, name) in &plan.sources {
        let argv = update_argv(url, name, commit);
        let words: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        cargo(scratch, &words, until).map_err(|problem| {
            format!(
                "its Cargo.lock could not be moved to the candidate {commit} by `{}`: {problem}",
                line(&argv)
            )
        })?;
        moved.push(line(&argv));
    }
    Ok(moved)
}

/// The command testing `plan`'s packages in the build folder `target`, against the lock
/// [`move_lock`] moved to the candidate.
pub(crate) fn argv(plan: &Plan, target: &Path) -> Vec<String> {
    let mut argv: Vec<String> = ["cargo", "test", "--no-fail-fast", "--target-dir"]
        .map(String::from)
        .to_vec();
    argv.push(target.to_string_lossy().into_owned());
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

/// The files of a scratch copy that its conciliation rewrites by design, which the look at the
/// copy after it passes over: its lock, which [`move_lock`] moves to the candidate's commit.
pub(crate) const REWRITTEN: [&str; 1] = ["Cargo.lock"];

/// What a conciliation's cargo, and so the tests it runs, never inherit from the run: the toolchain
/// the run's own build chose (the project's pin chooses), and a build folder named for the run's.
const UNSET: [&str; 3] = [
    "RUSTUP_TOOLCHAIN",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
];

/// A project's scratch copy, ready to conciliate.
pub(crate) struct Prepared {
    /// The check conciliating it: none when the project takes no package of the candidate's from
    /// git, said why.
    pub(crate) step: Result<Step, String>,
    /// The commit the copy holds.
    pub(crate) at: String,
    /// What was noted on the way.
    pub(crate) noted: Vec<String>,
}

/// `consumer`'s copy made ready to conciliate `against` the candidate: at the project's commit, its
/// lock moved to the candidate's ([`move_lock`]).
pub(crate) fn step(consumer: &Consumer, against: &Against<'_>) -> Result<Prepared, String> {
    let scratch = against.scratch;
    let (at, noted) = prepare(consumer, scratch, against.until, against.now)?;
    let candidate = packages(&metadata(against.checkout, against.until)?);
    let plan = plan(&metadata(scratch, against.until)?, &candidate);
    if plan.packages.is_empty() {
        let none = format!(
            "{}: none of its packages takes one of the Symphony's from git",
            consumer.id
        );
        return Ok(Prepared {
            step: Err(none),
            at,
            noted,
        });
    }
    let commit = commit(against.checkout, "HEAD")?;
    let moved = move_lock(scratch, &plan, &commit, against.until)?;
    Ok(Prepared {
        step: Ok(check(consumer, against, &at, &plan, &moved)),
        at,
        noted,
    })
}

/// The check conciliating `consumer`'s copy, at the commit `at`, by `plan`, its lock moved to the
/// candidate by the commands `moved`: in the copy, building in its own folder ([`target`]), which
/// is named to cargo alone, so what the project's tests start builds where the project's own runs
/// build and never in the conciliation's folder; with the run's machine folder.
fn check(
    consumer: &Consumer,
    against: &Against<'_>,
    at: &str,
    plan: &Plan,
    moved: &[String],
) -> Step {
    let scratch = against.scratch;
    let argv = argv(plan, &target(scratch));
    Step {
        name: format!("conciliate-{}", consumer.id),
        command: format!(
            "{} (in {} at {at}, its Cargo.lock moved to the candidate by {})",
            line(&argv),
            scratch.display(),
            moved.join(" and ")
        ),
        argv,
        cwd: Some(scratch.to_path_buf()),
        set: vec![(
            "SYMPHONY_MACHINE_DIR".into(),
            against.machine.to_string_lossy().into_owned(),
        )],
        unset: UNSET.map(String::from).to_vec(),
    }
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
    fn the_scratch_copy_takes_the_projects_committed_branch_and_keeps_its_own_build_folder() {
        let tree = Tree::new();
        let source = repo(&tree, "source");
        let first = commit_file(&source, "a.txt", "one");
        tree.write("source/uncommitted.txt", "never copied");
        let consumer = Consumer {
            id: "source".into(),
            root: source.clone(),
            branch: "main".into(),
        };
        let scratch = tree.path("wt/release-conciliate/source");
        assert_eq!(prepared(&consumer, &scratch, NOW).unwrap(), first);
        assert_eq!(fs::read_to_string(scratch.join("a.txt")).unwrap(), "one");
        assert!(!scratch.join("uncommitted.txt").exists());
        assert_eq!(
            fs::read_to_string(scratch.join(".git").join(COPY_MARK)).unwrap(),
            format!("{}\n", source.display()),
            "marked as the release run's copy of the project"
        );
        assert_eq!(
            fs::read_to_string(scratch.join(".git/objects/info/alternates")).unwrap(),
            format!("{}/.git/objects\n", source.display()),
            "sharing the project's objects"
        );

        assert_eq!(
            target(&scratch),
            scratch.join(".git/symphony-release-target"),
            "the conciliation builds in a folder of the release run's own, in the copy's .git"
        );
        tree.write(
            "wt/release-conciliate/source/.git/symphony-release-target/kept",
            "the conciliation's build",
        );
        tree.write(
            "wt/release-conciliate/source/.target/made",
            "what the project's tests built",
        );
        tree.write("wt/release-conciliate/source/stray.txt", "left by a build");
        tree.write("wt/release-conciliate/source/a.txt", "changed in the copy");
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
        assert!(
            !scratch.join(".target").exists(),
            "the project's own build folder is no build of the release run's: the copy is its \
             commit"
        );
        assert_eq!(
            fs::read_to_string(target(&scratch).join("kept")).unwrap(),
            "the conciliation's build"
        );
        assert_eq!(
            git_ok(&source, &["status", "--porcelain"]),
            "?? uncommitted.txt",
            "the project's checkout is as it was"
        );

        // A week of builds later the build folder goes.
        let (at, noted) = prepare(&consumer, &scratch, soon(), NOW + 8.0 * DAY).unwrap();
        assert_eq!(at, third);
        assert!(!target(&scratch).exists());
        assert!(scratch.join(".git").join(COPY_MARK).is_file());
        assert_eq!(
            noted,
            [format!(
                "cleared {}, built on for 7 days or more",
                target(&scratch).display()
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
    fn a_folder_in_a_copys_place_that_the_release_run_did_not_make_is_left_as_it_is() {
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
            refused.contains("is not the release run's copy of")
                && refused.contains("(no marker of the release run's)"),
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
            refused.contains("(no marker of the release run's)"),
            "{refused}"
        );

        // A link, even to the release run's own copy, is never followed.
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
    fn the_plan_moves_each_symphony_source_and_tests_the_packages_that_take_one() {
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
        // Each source by the first candidate package a member names from it: `symphony` for the
        // https one (host's), `symphony-board` for the ssh one (only tasks names it there).
        assert_eq!(
            planned,
            Plan {
                sources: vec![
                    ("https://github.com/o/symphony".into(), "symphony".into()),
                    (
                        "ssh://git@github.com/o/symphony.git".into(),
                        "symphony-board".into()
                    ),
                ],
                packages: vec!["host".into(), "tasks".into()],
            }
        );
        // What the refresh reads its pins from: the sources alone.
        assert_eq!(
            planned.urls(),
            [
                "https://github.com/o/symphony",
                "ssh://git@github.com/o/symphony.git"
            ]
        );
        // No patch: the tests read the lock moved to the candidate, as what they start does.
        assert_eq!(
            argv(&planned, Path::new("/copy/.git/symphony-release-target")),
            [
                "cargo",
                "test",
                "--no-fail-fast",
                "--target-dir",
                "/copy/.git/symphony-release-target",
                "-p",
                "host",
                "-p",
                "tasks",
            ]
        );
        assert_eq!(
            planned
                .sources
                .iter()
                .map(|(url, name)| line(&update_argv(url, name, "c0ffee")))
                .collect::<Vec<_>>(),
            [
                "cargo update -p 'https://github.com/o/symphony#symphony' --precise c0ffee",
                "cargo update -p 'ssh://git@github.com/o/symphony.git#symphony-board' --precise \
                 c0ffee",
            ]
        );
        assert_eq!(
            plan(&metadata_of(json!([]), &[]), &candidate),
            Plan::default()
        );
    }

    #[test]
    fn a_conciliation_builds_in_its_own_folder_named_to_cargo_alone() {
        let consumer = Consumer {
            id: "lamp-shop".into(),
            root: PathBuf::from("/p/lamp-shop"),
            branch: "main".into(),
        };
        let scratch = Path::new("/wt/release-conciliate/lamp-shop");
        let against = Against {
            scratch,
            checkout: Path::new("/wt/release"),
            machine: Path::new("/logs/run/machine"),
            until: soon(),
            now: NOW,
        };
        let plan = Plan {
            sources: vec![("https://h/o/symphony".into(), "symphony".into())],
            packages: vec!["host".into()],
        };
        let moved =
            ["cargo update -p 'https://h/o/symphony#symphony' --precise c0ffee".to_string()];
        let step = check(&consumer, &against, "1a2b", &plan, &moved);
        assert_eq!(step.name, "conciliate-lamp-shop");
        assert_eq!(step.cwd.as_deref(), Some(scratch));
        assert_eq!(
            step.argv,
            [
                "cargo",
                "test",
                "--no-fail-fast",
                "--target-dir",
                "/wt/release-conciliate/lamp-shop/.git/symphony-release-target",
                "-p",
                "host"
            ]
        );
        assert_eq!(
            step.command,
            "cargo test --no-fail-fast --target-dir \
             /wt/release-conciliate/lamp-shop/.git/symphony-release-target -p host (in \
             /wt/release-conciliate/lamp-shop at 1a2b, its Cargo.lock moved to the candidate by \
             cargo update -p 'https://h/o/symphony#symphony' --precise c0ffee)"
        );
        // No variable names a build folder to what its tests start, whatever the run inherited:
        // those build where the project's own runs build.
        assert_eq!(
            step.set,
            [(
                "SYMPHONY_MACHINE_DIR".to_string(),
                "/logs/run/machine".to_string()
            )]
        );
        assert_eq!(
            step.unset,
            [
                "RUSTUP_TOOLCHAIN",
                "CARGO_TARGET_DIR",
                "CARGO_BUILD_TARGET_DIR"
            ]
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
