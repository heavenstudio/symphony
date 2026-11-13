//! Generic heartbeat binding, checkout lookup and recorder observations.

use crate::beat_fixture::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", root)])); // the launcher's project
    quiet(&tree.beat(&event, &[("CLAUDE_PROJECT_DIR", root)])); // else Claude Code's
    assert_eq!(runs(&tree.link()), ["beat", "beat"]); // written beside the name it ran by
    assert_eq!(runs(&tree.recorder()), Vec::<String>::new());
}

#[test]
fn a_tree_that_ran_no_build_beats_on_none_and_builds_none() {
    // ember-otter: another tree's build may be an early one, without the check of a board not moved yet,
    // so no build lying in the common dir runs, only this tree's own link or its main checkout's
    // (below); and a build would hold up the tool call for minutes, so none is made.
    let tree = Tree::new("beat-unbuilt");
    let common = tree.root().join("common");
    std::fs::create_dir_all(&common).unwrap();
    git(&common, &["init", "-q", "--bare", "."]);
    let builds = common.join("builds");
    std::fs::create_dir_all(&builds).unwrap();
    let other = builds.join(format!("tasks-{}", "e".repeat(16)));
    // This unselected build must be runnable without adding sidecars to the builds inventory.
    std::fs::write(&other, RECORDER).unwrap();
    executable(&other);
    let env = [
        ("SYMPHONY_HOOK_ROOT", tree.root().to_str().unwrap()),
        ("SYMPHONY_GIT_COMMON_DIR", common.to_str().unwrap()),
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
    assert!(
        !linked.join(".target").exists(),
        "nothing is linked in the worktree"
    );
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
        &[(
            "CLAUDE_PROJECT_DIR",
            tree.root().join("repo").to_str().unwrap(),
        )],
    ));
    assert_eq!(runs(&folder.join("tasks")), ["beat", "beat"]);
}

#[test]
fn a_nested_manifest_uses_its_located_common_build_without_crossing_invalid_markers() {
    let tree = Tree::new("beat-nested-manifest");
    let (repo, linked) = repo_with_worktree(&tree);
    let main = link_main(&repo.join(".git"), &tree.recorder());
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
            assert_eq!(handed(&main), expected);
            assert!(
                !component.join(".target").exists(),
                "a beat makes no link: {component:?}"
            );
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
    assert_eq!(runs(&main), ["beat"; 4]);
}

#[test]
fn a_checkout_without_a_link_beats_on_its_main_checkouts_and_makes_none() {
    let tree = Tree::new("beat-main-link");
    let (repo, linked) = repo_with_worktree(&tree);
    let event = bash("ls", Some("agent-1"), None);
    let beat_at =
        |top: &Path| quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", top.to_str().unwrap())]));
    // Nothing linked yet: no beat, and no build made.
    beat_at(&linked);
    assert_eq!(names(&linked.join(".target")), Vec::<String>::new());
    let main = link_main(&repo.join(".git"), &tree.recorder());
    beat_at(&linked);
    assert_eq!(runs(&main), ["beat"]);
    assert!(!linked.join(".target").exists(), "a beat makes no link");
    // The main checkout beats on its own link.
    beat_at(&repo);
    assert_eq!(runs(&main), ["beat", "beat"]);
    // A worktree's own link wins over its main checkout's.
    let own = tree.root().join("own-recorder");
    recorder_at(&own);
    std::fs::create_dir_all(linked.join(".target/bin")).unwrap();
    let link = linked.join(".target/bin/tasks");
    std::os::unix::fs::symlink(&own, &link).unwrap();
    beat_at(&linked);
    assert_eq!(runs(&link), ["beat"]);
    assert_eq!(runs(&main), ["beat", "beat"]);
    // A project inside the repository that is not a checkout's top looks in no common dir above it.
    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    beat_at(&sub);
    assert_eq!(names(&sub), Vec::<String>::new());
    assert_eq!(runs(&main), ["beat", "beat"]);
}

#[test]
fn a_project_whose_git_names_nothing_never_runs_the_enclosing_repositorys_build() {
    // umber-finch-710: a `.git` that is neither a folder nor a file naming one (a link to nothing, a file
    // with no `gitdir:`, one naming a folder that is gone) is the project's own marker all the same, so
    // the link of the repository it sits in is never its build.
    let tree = Tree::new("beat-live-marker");
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let main = link_main(&repo.join(".git"), &tree.recorder());
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
        quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", project.to_str().unwrap())]));
        assert_eq!(runs(&main), Vec::<String>::new(), "{name}");
        assert!(!project.join(".target").exists(), "{name}");
    }
    // The repository's own top still finds its link.
    quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", repo.to_str().unwrap())]));
    assert_eq!(runs(&main), ["beat"]);
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

/// What is wrong with a beat whose project is `top`: git names `common` its common dir, whose main
/// checkout links the tree's recorder, and `decoy` is another repository's link, which must never run.
fn beats_where_git_looks(tree: &Tree, top: &Path, common: &Path, decoy: &Path) -> Option<String> {
    let common = std::fs::canonicalize(common).unwrap();
    assert_eq!(gits_common_dir(top), common, "{top:?}");
    let main = link_main(&common, &tree.recorder());
    let event = bash("ls", Some("agent-1"), None);
    quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", top.to_str().unwrap())]));
    let (main, other) = (runs(&main), runs(decoy));
    (main != ["beat"] || !other.is_empty())
        .then(|| format!("{top:?}: the main checkout's build ran {main:?}, the other {other:?}"))
}

/// A recorder linked as the main checkout's build of the repository whose common dir is `common`,
/// another repository's: the link.
fn decoy(tree: &Tree, common: &Path) -> PathBuf {
    let recorder = tree.root().join("decoy-recorder");
    recorder_at(&recorder);
    std::fs::create_dir_all(common).unwrap();
    link_main(common, &recorder)
}

#[test]
fn a_link_before_a_parent_step_is_followed_as_git_follows_it() {
    // umber-finch-710: in `alias/../x` the `..` is the folder above the link's target, as git and the
    // kernel read it. A fold of the text takes the folder above the link, where each case keeps another
    // repository's linked build: in the project's top, in its `gitdir:` and in its admin folder's
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
    std::fs::create_dir_all(real.join("x")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let admin = real.join("x/.git");
    git(
        &project,
        &["init", "-q", "--separate-git-dir", admin.to_str().unwrap()],
    );
    std::os::unix::fs::symlink(real.join("inner"), project.join("alias")).unwrap();
    std::fs::write(project.join(".git"), "gitdir: alias/../x/.git\n").unwrap();
    let other = decoy(&tree, &project.join("x/.git"));
    wrong.extend(beats_where_git_looks(&tree, &project, &admin, &other));

    let tree = Tree::new("beat-alias-commondir");
    let (repo, linked) = repo_with_worktree(&tree);
    let pointer = std::fs::read_to_string(linked.join(".git")).unwrap();
    let admin = PathBuf::from(pointer.trim().strip_prefix("gitdir:").unwrap().trim());
    std::fs::create_dir_all(repo.join("inner")).unwrap();
    std::os::unix::fs::symlink(repo.join("inner"), admin.join("alias")).unwrap();
    std::fs::write(admin.join("commondir"), "alias/../.git\n").unwrap();
    let other = decoy(&tree, &admin.join(".git"));
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
