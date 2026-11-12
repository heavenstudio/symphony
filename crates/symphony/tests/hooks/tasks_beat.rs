//! Generic heartbeat binding, checkout lookup and recorder observations.

use crate::beat_fixture::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

#[test]
fn an_event_with_no_key_never_runs_the_binary() {
    let tree = Tree::new("beat-keyless");
    let recorder = tree.recorder();
    let env = [("TASKS_BIN", recorder.to_str().unwrap())];
    let mut keyless = bash("TASKS_SESSION=h just tasks inbox", None, None);
    quiet(&tree.beat(&keyless, &env)); // neither an agent nor a session id: a no-op
    keyless["agent_id"] = Value::from("");
    keyless["session_id"] = Value::from("");
    quiet(&tree.beat(&keyless, &env)); // empty ones are none
    assert_eq!(runs(&recorder), Vec::<String>::new());
}

#[test]
fn an_event_with_a_key_is_handed_to_the_binarys_beat_whole() {
    // A board a test names (TASKS_SERVER) has no keys' folder: the binary judges a full session's key.
    let tree = Tree::new("beat-whole");
    let recorder = tree.recorder();
    let env = [
        ("TASKS_BIN", recorder.to_str().unwrap()),
        ("TASKS_SERVER", "127.0.0.1:1"),
    ];
    let agent = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&agent, &env));
    assert_eq!(runs(&recorder), ["beat"]);
    assert_eq!(handed(&recorder), agent.to_string());
    quiet(&tree.beat(&reading("s-9"), &env));
    assert_eq!(runs(&recorder), ["beat", "beat"]);
    assert_eq!(handed(&recorder), format!("{agent}{}", reading("s-9")));
}

#[test]
fn without_tasks_bin_it_runs_the_build_this_tree_last_ran() {
    // silver-ibis: a merge that touches the crate's sources leaves the tree's build of them unbuilt until
    // its next `just tasks`, and a subagent that never runs one would read lost after ten minutes of dropped
    // beats. The link still names the build the tree last ran (tools/rs-bin.sh), so it beats on that one.
    let tree = Tree::new("beat-link");
    tree.link_to(&tree.recorder());
    let root = tree.root().to_str().unwrap();
    let event = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", root)])); // the launcher's project
    quiet(&tree.beat(&event, &[("CLAUDE_PROJECT_DIR", root)])); // else Claude Code's
    assert_eq!(runs(&tree.link()), ["beat", "beat"]); // written beside the name it ran by
    assert_eq!(runs(&tree.recorder()), Vec::<String>::new());
}

#[test]
fn a_tree_that_ran_no_build_beats_on_none_and_builds_none() {
    // ember-otter: another tree's build may be an early one, without the check of a board not moved yet,
    // so none in `sf-cargo/bin` runs, only this tree's own or the live build of main `tools/tasks.sh`
    // recorded (below); and a build would hold up the tool call for minutes, so none is made.
    let tree = Tree::new("beat-unbuilt");
    let common = tree.root().join("common");
    std::fs::create_dir_all(&common).unwrap();
    git(&common, &["init", "-q", "--bare", "."]);
    let builds = common.join("sf-cargo/bin");
    std::fs::create_dir_all(&builds).unwrap();
    let other = builds.join(format!("tasks-{}", "e".repeat(16)));
    // This unselected build must be runnable without adding sidecars to the builds inventory.
    std::fs::write(&other, RECORDER).unwrap();
    executable(&other);
    let env = [
        ("SF_HOOKS_ROOT", tree.root().to_str().unwrap()),
        ("SF_GIT_COMMON_DIR", common.to_str().unwrap()),
    ];
    let event = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&event, &env)); // no link
    assert_eq!(names(&tree.root().join(".target")), Vec::<String>::new());
    tree.link_to(&tree.root().join("gone")); // a link to a build since removed
    quiet(&tree.beat(&event, &env));
    assert_eq!(names(&tree.root().join(".target")), ["bin"]);
    assert_eq!(names(&tree.root().join(".target/bin")), ["tasks"]);
    assert_eq!(runs(&other), Vec::<String>::new());
    assert_eq!(
        names(&builds),
        [other.file_name().unwrap().to_str().unwrap()]
    );
}

#[test]
fn a_linked_worktree_with_no_link_of_its_own_beats_on_its_main_checkouts_as_the_recipes_run() {
    // A session started in a linked worktree of a project that links its builds in the main checkout
    // alone, with no live build recorded.
    let tree = Tree::new("beat-main-link");
    let (repo, linked) = repo_with_worktree(&tree);
    let mains = repo.join(".target/bin/tasks");
    std::fs::create_dir_all(mains.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(tree.recorder(), &mains).unwrap();
    let event = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&event, &[("CLAUDE_PROJECT_DIR", linked.to_str().unwrap())]));
    assert_eq!(runs(&mains), ["beat"]);
    assert!(!linked.join(".target").exists(), "nothing is linked in the worktree");
}

#[test]
fn a_checkout_with_no_link_anywhere_beats_on_the_search_paths_build() {
    // A project that installed its builds rather than linking them.
    let tree = Tree::new("beat-path");
    let (_, linked) = repo_with_worktree(&tree);
    let folder = tree.root().join("installed");
    std::fs::create_dir_all(&folder).unwrap();
    recorder_at(&folder.join("tasks"));
    let path = format!("{}:/usr/bin:/bin", folder.display());
    let event = bash("ls", Some("agent-1"), None);
    for root in [tree.root().join("repo"), linked] {
        quiet(&tree.beat(
            &event,
            &[
                ("CLAUDE_PROJECT_DIR", root.to_str().unwrap()),
                ("PATH", &path),
            ],
        ));
    }
    assert_eq!(runs(&folder.join("tasks")), ["beat", "beat"]);
    // Without it on the search path, there is nothing to beat on.
    quiet(&tree.beat(
        &event,
        &[("CLAUDE_PROJECT_DIR", tree.root().join("repo").to_str().unwrap())],
    ));
    assert_eq!(runs(&folder.join("tasks")), ["beat", "beat"]);
}

/// `path` last written `hours` from now: before it when negative.
fn written_at(path: &Path, hours: i32) {
    let now = SystemTime::now();
    let shift = Duration::from_secs(3600 * u64::from(hours.unsigned_abs()));
    let at = if hours < 0 { now - shift } else { now + shift };
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn a_nested_manifest_uses_its_located_common_build_without_crossing_invalid_markers() {
    let tree = Tree::new("beat-nested-manifest");
    let (repo, linked) = repo_with_worktree(&tree);
    record_live(&repo.join(".git"), "current", &tree.recorder());
    let mut expected = String::new();
    for checkout in [&repo, &linked] {
        let component = checkout.join("component");
        std::fs::create_dir_all(component.join("src")).unwrap();
        std::fs::write(
            component.join("symphony.toml"),
            "[project]\nid='component'\n",
        )
        .unwrap();
        let mut event = bash("ls", Some("child"), None);
        event["cwd"] = json!(component.join("src"));
        for explicit in [false, true] {
            let named = [("CLAUDE_PROJECT_DIR", component.to_str().unwrap())];
            quiet(&tree.beat(&event, if explicit { &named } else { &[] }));
            expected.push_str(&event.to_string());
            let link = component.join(".target/bin/tasks");
            assert_eq!(std::fs::read_link(&link).unwrap(), tree.recorder());
            assert_eq!(handed(&tree.recorder()), expected);
            std::fs::remove_file(link).unwrap();
        }
        let invalid = component.join("invalid");
        std::fs::create_dir_all(invalid.join("src")).unwrap();
        std::fs::write(invalid.join(".git"), "gitdir: \n").unwrap();
        std::fs::write(invalid.join("symphony.toml"), "[project]\nid='invalid'\n").unwrap();
        event["cwd"] = json!(invalid.join("src"));
        quiet(&tree.beat(&event, &[]));
        assert!(
            !invalid.join(".target").exists(),
            "invalid marker reached a build: {invalid:?}"
        );
    }
    assert_eq!(runs(&tree.recorder()), ["beat"; 4]);
}

#[test]
fn the_live_build_is_the_newest_record_naming_a_build_read_from_the_checkouts_own_git() {
    let tree = Tree::new("beat-live-records");
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    git(&repo, &["init", "-q"]);
    let common = repo.join(".git");
    let builds = tree.root().join("builds");
    std::fs::create_dir_all(&builds).unwrap();
    for name in ["old", "new", "staged", "relative"] {
        recorder_at(&builds.join(name));
    }
    std::fs::write(builds.join("plain"), "#!/bin/sh\n").unwrap(); // not executable
    written_at(&record_live(&common, "a1", &builds.join("old")), -1);
    record_live(&common, "b2", &builds.join("new"));
    // Newer records with nothing to run: one still being written, one whose build is gone, one whose
    // build cannot be run, and one naming no absolute path (`relative` from the folder the hook runs in).
    written_at(&record_live(&common, ".c3.x", &builds.join("staged")), 1);
    written_at(&record_live(&common, "d4", &builds.join("gone")), 1);
    written_at(&record_live(&common, "e5", &builds.join("plain")), 1);
    written_at(&record_live(&common, "f6", Path::new("builds/relative")), 1);
    let event = bash("ls", Some("agent-1"), None);
    // A project inside the repository that is not a checkout's top looks in no common dir above it.
    let sub = repo.join("sub");
    quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", sub.to_str().unwrap())]));
    assert_eq!(names(&sub), Vec::<String>::new());
    quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", repo.to_str().unwrap())]));
    assert_eq!(runs(&builds.join("new")), ["beat"]);
    for other in ["old", "staged", "relative"] {
        assert_eq!(runs(&builds.join(other)), Vec::<String>::new(), "{other}");
    }
    assert_eq!(
        std::fs::read_link(repo.join(".target/bin/tasks")).unwrap(),
        builds.join("new")
    );
}

#[test]
fn a_checkout_where_no_link_can_be_made_still_beats_on_the_live_build() {
    // `.target` is a file here, so `.target/bin/tasks` cannot be made: each beat looks up the record.
    let tree = Tree::new("beat-live-unlinkable");
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    record_live(&repo.join(".git"), "f00d", &tree.recorder());
    std::fs::write(repo.join(".target"), "").unwrap();
    let event = bash("ls", Some("agent-1"), None);
    for _ in 0..2 {
        quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", repo.to_str().unwrap())]));
    }
    assert_eq!(runs(&tree.recorder()), ["beat", "beat"]);
    assert!(repo.join(".target").is_file());
}

#[test]
fn a_project_whose_git_names_nothing_never_runs_the_enclosing_repositorys_build() {
    // umber-finch-710: a `.git` that is neither a folder nor a file naming one (a link to nothing, a file
    // with no `gitdir:`, one naming a folder that is gone) is the project's own marker all the same, so
    // the record of the repository it sits in is never its live build.
    let tree = Tree::new("beat-live-marker");
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    record_live(&repo.join(".git"), "f00d", &tree.recorder());
    let event = bash("ls", Some("agent-1"), None);
    for name in ["dangling", "no-gitdir", "gone-gitdir"] {
        let project = repo.join(name);
        std::fs::create_dir_all(&project).unwrap();
        let dot = project.join(".git");
        match name {
            "dangling" => std::os::unix::fs::symlink("missing-admin", &dot).unwrap(),
            "no-gitdir" => std::fs::write(&dot, "not a pointer\n").unwrap(),
            _ => std::fs::write(&dot, "gitdir: ../missing-admin\n").unwrap(),
        }
        quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", project.to_str().unwrap())]));
        assert_eq!(runs(&tree.recorder()), Vec::<String>::new(), "{name}");
        assert!(!project.join(".target").exists(), "{name}");
    }
    // The repository's own top still finds its record.
    quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", repo.to_str().unwrap())]));
    assert_eq!(runs(&tree.recorder()), ["beat"]);
}

/// The git common dir git itself names for the checkout at `top`, links resolved.
fn gits_common_dir(top: &Path) -> PathBuf {
    let mut ask = Command::new("git");
    ask.args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(top);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        ask.env_remove(key);
    }
    let done = ask.output().unwrap();
    assert!(done.status.success(), "{done:?}");
    std::fs::canonicalize(String::from_utf8(done.stdout).unwrap().trim()).unwrap()
}

/// What is wrong with a beat whose project is `top`: git names `common` its common dir, whose live build
/// is the tree's recorder, and `decoy` is another repository's, which must never run.
fn beats_where_git_looks(tree: &Tree, top: &Path, common: &Path, decoy: &Path) -> Option<String> {
    let common = std::fs::canonicalize(common).unwrap();
    assert_eq!(gits_common_dir(top), common, "{top:?}");
    record_live(&common, "f00d", &tree.recorder());
    let event = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&event, &[("SF_HOOKS_ROOT", top.to_str().unwrap())]));
    let (live, other) = (runs(&tree.recorder()), runs(decoy));
    (live != ["beat"] || !other.is_empty())
        .then(|| format!("{top:?}: the live build ran {live:?}, the other {other:?}"))
}

/// A recorder recorded as the live build in `common`, another repository's.
fn decoy(tree: &Tree, common: &Path) -> PathBuf {
    let recorder = tree.root().join("decoy-recorder");
    recorder_at(&recorder);
    std::fs::create_dir_all(common).unwrap();
    record_live(common, "f00d", &recorder);
    recorder
}

#[test]
fn a_link_before_a_parent_step_is_followed_as_git_follows_it() {
    // umber-finch-710: in `alias/../x` the `..` is the folder above the link's target, as git and the
    // kernel read it. A fold of the text takes the folder above the link, where each case keeps another
    // repository's live build: in the project's top, in its `gitdir:` and in its admin folder's
    // `commondir`.
    let mut wrong = Vec::new();

    let tree = Tree::new("beat-alias-top");
    let real = tree.root().join("real");
    std::fs::create_dir_all(real.join("inner")).unwrap();
    for repo in [real.join("project"), tree.root().join("project")] {
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
    }
    std::os::unix::fs::symlink(real.join("inner"), tree.root().join("alias")).unwrap();
    let other = decoy(&tree, &tree.root().join("project/.git"));
    let top = tree.root().join("alias/../project");
    wrong.extend(beats_where_git_looks(
        &tree,
        &top,
        &real.join("project/.git"),
        &other,
    ));

    let tree = Tree::new("beat-alias-gitdir");
    let (project, real) = (tree.root().join("project"), tree.root().join("real"));
    std::fs::create_dir_all(real.join("inner")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let admin = real.join("common");
    git(
        &project,
        &["init", "-q", "--separate-git-dir", admin.to_str().unwrap()],
    );
    std::os::unix::fs::symlink(real.join("inner"), project.join("alias")).unwrap();
    std::fs::write(project.join(".git"), "gitdir: alias/../common\n").unwrap();
    let other = decoy(&tree, &project.join("common"));
    wrong.extend(beats_where_git_looks(&tree, &project, &admin, &other));

    let tree = Tree::new("beat-alias-commondir");
    let (repo, linked) = repo_with_worktree(&tree);
    let pointer = std::fs::read_to_string(linked.join(".git")).unwrap();
    let admin = PathBuf::from(pointer.trim().strip_prefix("gitdir:").unwrap().trim());
    std::fs::create_dir_all(repo.join(".git/alias-inner")).unwrap();
    std::os::unix::fs::symlink(repo.join(".git/alias-inner"), admin.join("alias")).unwrap();
    std::fs::write(admin.join("commondir"), "alias/..\n").unwrap();
    let other = decoy(&tree, &admin);
    wrong.extend(beats_where_git_looks(
        &tree,
        &linked,
        &repo.join(".git"),
        &other,
    ));

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[path = "keys.rs"]
mod keys;
