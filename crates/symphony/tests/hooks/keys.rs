//! Bound-key filtering, checkout identity and launcher behavior for isolated projects.

use super::*;

#[test]
fn identical_session_keys_and_builds_are_isolated_by_the_events_project() {
    let tree = Tree::new("beat-projects");
    let mut projects = Vec::new();
    for name in ["alpha", "beta"] {
        let repo = tree.root().join(name);
        std::fs::create_dir_all(repo.join("deep")).unwrap();
        git(&repo, &["init", "-q"]);
        std::fs::write(
            repo.join("symphony.toml"),
            format!("[project]\nid = '{name}'\n"),
        )
        .unwrap();
        let recorder = repo.join("recorder");
        recorder_at(&recorder);
        std::fs::create_dir_all(repo.join(".target/bin")).unwrap();
        std::os::unix::fs::symlink(&recorder, repo.join(".target/bin/tasks")).unwrap();
        let located = symphony_board::project::locate(&repo.join("deep")).unwrap();
        assert_eq!(
            symphony_board::project::locate_quick(&repo.join("deep")),
            Some(located)
        );
        projects.push(repo);
    }
    let alpha = &projects[0];
    let beta = &projects[1];
    std::fs::create_dir_all(alpha.join(".git/symphony/tasks/keys")).unwrap();
    std::fs::write(alpha.join(".git/symphony/tasks/keys/shared-session"), "").unwrap();
    for repo in [alpha, beta, alpha] {
        let mut event = reading("shared-session");
        event["cwd"] = json!(repo.join("deep"));
        quiet(&tree.beat(&event, &[]));
    }
    assert_eq!(runs(&alpha.join(".target/bin/tasks")), ["beat", "beat"]);
    assert!(runs(&beta.join(".target/bin/tasks")).is_empty());
    // No board was made or started for the checkout whose key it is not.
    for made in ["postgresql", "backend.json"] {
        assert!(!beta.join(".git/symphony/tasks").join(made).exists());
    }
}

#[test]
fn a_full_session_whose_key_is_not_bound_never_runs_the_binary() {
    // main's prefilter, kept: a full session's tool call (an orchestrator's, the owner's) goes to
    // the binary only when its key is bound (`symphony/tasks/keys/KEY`, written by the binary when
    // it binds one) or the call is a board command, which may bind it.
    let tree = Tree::new("beat-unbound");
    let recorder = tree.recorder();
    let keys = tree.root().join("keys");
    let env = [
        ("TASKS_BIN", recorder.to_str().unwrap()),
        ("TASKS_KEYS", keys.to_str().unwrap()),
    ];
    quiet(&tree.beat(&reading("s-free"), &env)); // no key bound: nothing to beat
    quiet(&tree.beat(&bash("ls", None, Some("s-free")), &env)); // not a board command
    assert_eq!(runs(&recorder), Vec::<String>::new());
    let mut expected_events = String::new();
    for cli in [
        "TASKS_SESSION=h just tasks inbox",
        "just tasks show tools/example --as amber-heron",
    ] {
        let command = bash(cli, None, Some("s-free"));
        quiet(&tree.beat(&command, &env)); // a board command may bind it
        // The recorder appends each event without rewriting its bytes.
        expected_events.push_str(&command.to_string());
        assert_eq!(handed(&recorder), expected_events);
    }
    assert_eq!(runs(&recorder), ["beat", "beat"]);
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("s-free"), "").unwrap();
    quiet(&tree.beat(&reading("s-free"), &env)); // now bound: its calls beat
    assert_eq!(runs(&recorder), ["beat", "beat", "beat"]);
}

#[test]
fn a_subagent_event_always_goes_to_the_binary() {
    // A subagent's key is never written to the keys' folder (the binary marks a full session's alone), so
    // the folder is never asked: every call of its, board command or not, is its heartbeat.
    let tree = Tree::new("beat-agent");
    let recorder = tree.recorder();
    let keys = tree.root().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let env = [
        ("TASKS_BIN", recorder.to_str().unwrap()),
        ("TASKS_KEYS", keys.to_str().unwrap()),
    ];
    let event = bash("ls", Some("agent-7"), Some("s-1"));
    quiet(&tree.beat(&event, &env));
    assert_eq!(runs(&recorder), ["beat"]);
    assert_eq!(handed(&recorder), event.to_string());
    assert_eq!(names(&keys), Vec::<String>::new());
}

#[test]
fn a_full_session_whose_key_names_no_file_is_left_to_the_binary() {
    // A key no file can name (a path, a dot, a space) is never looked up as one: the binary decides.
    let tree = Tree::new("beat-unsafe");
    let recorder = tree.recorder();
    let keys = tree.root().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let env = [
        ("TASKS_BIN", recorder.to_str().unwrap()),
        ("TASKS_KEYS", keys.to_str().unwrap()),
    ];
    let unsafe_keys = ["../keys", "a.b", "a/b", "a b"];
    for key in unsafe_keys {
        quiet(&tree.beat(&reading(key), &env));
    }
    assert_eq!(runs(&recorder), ["beat"; 4]);
    quiet(&tree.beat(&reading("8f2c_session-ID"), &env)); // a key a file can name, and none does
    assert_eq!(runs(&recorder).len(), unsafe_keys.len());
}

#[test]
fn the_keys_folder_is_tasks_keys_else_none_for_a_named_board_else_the_common_dirs() {
    let tree = Tree::new("beat-folder");
    let recorder = tree.recorder();
    let bin = ("TASKS_BIN", recorder.to_str().unwrap());
    let keys = tree.root().join("keys");
    let repo = tree.root().join("repo");
    std::fs::create_dir_all(repo.join("deep")).unwrap();
    git(&repo, &["init", "-q"]);
    let mut event = reading("s-1");
    event["cwd"] = Value::from(repo.join("deep").to_str().unwrap());
    let both = [
        bin,
        ("TASKS_KEYS", keys.to_str().unwrap()),
        ("TASKS_SERVER", "x"),
    ];
    quiet(&tree.beat(&event, &both)); // TASKS_KEYS, where s-1 is not
    quiet(&tree.beat(&event, &[bin])); // the common dir's, where s-1 is not either
    assert_eq!(runs(&recorder), Vec::<String>::new());
    quiet(&tree.beat(&event, &[bin, ("TASKS_SERVER", "127.0.0.1:1")])); // none: the binary decides
    assert_eq!(runs(&recorder), ["beat"]);
    let common_keys = repo.join(".git/symphony/tasks/keys");
    std::fs::create_dir_all(&common_keys).unwrap();
    std::fs::write(common_keys.join("s-1"), "").unwrap();
    quiet(&tree.beat(&event, &[bin]));
    assert_eq!(runs(&recorder), ["beat", "beat"]);
    quiet(&tree.beat(&event, &both)); // still TASKS_KEYS' answer
    assert_eq!(runs(&recorder), ["beat", "beat"]);
}

#[test]
fn the_keys_folder_is_found_from_the_events_folder() {
    // Read from the `.git` files themselves, following links without running git: a worktree's `.git`
    // file names its folder in the common dir, whose `commondir` names the common dir.
    let tree = Tree::new("beat-layout");
    let recorder = tree.recorder();
    let env = [("TASKS_BIN", recorder.to_str().unwrap())];
    let repo = tree.root().join("repo");
    let git_dir = repo.join(".git");
    std::fs::create_dir_all(git_dir.join("worktrees/wt")).unwrap();
    let worktree = repo.join(".claude/worktrees/wt");
    std::fs::create_dir_all(worktree.join("sub")).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git_dir.join("worktrees/wt").display()),
    )
    .unwrap();
    std::fs::write(git_dir.join("worktrees/wt/commondir"), "../..\n").unwrap();
    let mut event = reading("s-free");
    for cwd in [repo.join("sub-missing"), worktree.join("sub")] {
        event["cwd"] = Value::from(cwd.to_str().unwrap());
        quiet(&tree.beat(&event, &env));
    }
    assert_eq!(runs(&recorder), Vec::<String>::new(), "an unbound key beat");
    std::fs::create_dir_all(git_dir.join("symphony/tasks/keys")).unwrap();
    std::fs::write(git_dir.join("symphony/tasks/keys/s-free"), "").unwrap();
    for cwd in [repo.clone(), worktree.join("sub")] {
        event["cwd"] = Value::from(cwd.to_str().unwrap());
        quiet(&tree.beat(&event, &env));
    }
    assert_eq!(runs(&recorder), ["beat", "beat"]);
}

#[test]
fn a_linked_worktree_looks_in_the_common_dirs_folder() {
    let tree = Tree::new("beat-linked");
    let recorder = tree.recorder();
    let env = [("TASKS_BIN", recorder.to_str().unwrap())];
    let (repo, linked) = repo_with_worktree(&tree);
    let mut event = reading("s-1");
    event["cwd"] = Value::from(linked.to_str().unwrap());
    quiet(&tree.beat(&event, &env));
    assert_eq!(runs(&recorder), Vec::<String>::new());
    std::fs::create_dir_all(repo.join(".git/symphony/tasks/keys")).unwrap();
    std::fs::write(repo.join(".git/symphony/tasks/keys/s-1"), "").unwrap();
    quiet(&tree.beat(&event, &env));
    assert_eq!(runs(&recorder), ["beat"]);
}

#[test]
fn bound_keys_through_a_link_then_parent_belong_to_the_physical_checkout() {
    physical_bound_keys("cwd");
}

#[test]
fn bound_keys_follow_links_before_parent_steps_in_the_gitdir_pointer() {
    physical_bound_keys("gitdir");
}

#[test]
fn bound_keys_follow_links_before_parent_steps_in_commondir() {
    physical_bound_keys("commondir");
}

fn physical_bound_keys(location: &str) {
    let tree = Tree::new("beat-physical-keys");
    let (repo, linked) = repo_with_worktree(&tree);
    let other = Tree::new("beat-lexical-keys");
    git(other.root(), &["init", "-q"]);
    let expected = gits_common_dir(&repo);
    let decoy = gits_common_dir(other.root());
    let marker = std::fs::read_to_string(linked.join(".git")).unwrap();
    let admin = PathBuf::from(marker.trim().strip_prefix("gitdir: ").unwrap());
    let cwd = match location {
        "cwd" => {
            std::fs::create_dir_all(linked.join("inner")).unwrap();
            std::os::unix::fs::symlink(linked.join("inner"), other.root().join("alias")).unwrap();
            other.root().join("alias/..")
        }
        "gitdir" => {
            std::fs::create_dir_all(admin.join("inner")).unwrap();
            std::os::unix::fs::symlink(admin.join("inner"), decoy.join("alias")).unwrap();
            std::fs::write(decoy.join("commondir"), ".\n").unwrap();
            std::fs::write(
                linked.join(".git"),
                format!("gitdir: {}\n", decoy.join("alias/..").display()),
            )
            .unwrap();
            linked
        }
        "commondir" => {
            std::fs::create_dir_all(expected.join("inner")).unwrap();
            std::os::unix::fs::symlink(expected.join("inner"), decoy.join("alias")).unwrap();
            std::fs::write(
                admin.join("commondir"),
                format!("{}\n", decoy.join("alias/..").display()),
            )
            .unwrap();
            linked
        }
        _ => panic!("unknown path location: {location}"),
    };
    let common = gits_common_dir(&cwd);
    assert_eq!(common, expected, "{location}");
    assert_ne!(common, decoy, "{location}");
    let keys = common.join("symphony/tasks/keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("s-physical"), "").unwrap();
    let main = link_main(&common, &tree.recorder());
    let lexical = link_main(&decoy, &other.recorder());
    let env = [("SYMPHONY_HOOK_ROOT", cwd.to_str().unwrap())];
    let mut event = reading("s-physical");
    event["cwd"] = Value::from(cwd.to_str().unwrap());
    quiet(&tree.beat(&event, &env));
    assert_eq!(runs(&main), ["beat"], "{location}");
    assert!(runs(&lexical).is_empty(), "{location}");
}

#[test]
fn outside_a_repository_the_key_is_left_to_the_binary() {
    let tree = Tree::new("beat-outside");
    let recorder = tree.recorder();
    let mut event = reading("s-1");
    event["cwd"] = Value::from("/");
    quiet(&tree.beat(&event, &[("TASKS_BIN", recorder.to_str().unwrap())]));
    assert_eq!(runs(&recorder), ["beat"]);
}

#[test]
fn the_beat_never_fails_the_tool_call() {
    let tree = Tree::new("beat-garbage");
    let recorder = tree.recorder();
    let env = [("TASKS_BIN", recorder.to_str().unwrap())];
    for raw in [
        "",
        "not json",
        "{\"session_id\": \"s-1\"",
        "[]",
        "{\"session_id\": \"s-1\", \"tool_input\": \"x\"}",
        "{\"agent_id\": 7}",
    ] {
        let done = tree.beat_raw(raw, &env);
        assert_eq!((done.code, done.stdout.as_str()), (0, ""), "{raw}");
    }
    // A binary that cannot run, or one that fails out loud: still a quiet exit 0.
    let event = bash("ls", Some("a"), Some("s-1"));
    let gone = tree.root().join("gone");
    quiet(&tree.beat(&event, &[("TASKS_BIN", gone.to_str().unwrap())]));
    let loud = tree.root().join("loud");
    std::fs::write(&loud, "#!/bin/sh\necho out; echo err >&2; exit 3\n").unwrap();
    executable(&loud);
    quiet(&tree.beat(&event, &[("TASKS_BIN", loud.to_str().unwrap())]));
    // A project root that is not there: nothing to run.
    quiet(&tree.beat(&event, &[("SYMPHONY_HOOK_ROOT", gone.to_str().unwrap())]));
}
