//! The host rule's cases: each refusal and each replacement.

use std::path::PathBuf;

use super::*;
use crate::test_layouts::{Others, others, repository};
use crate::test_tree::Tree;

fn build(plugins: &[&str], pin: u32, schema: i64) -> Build {
    Build {
        plugins: plugins.iter().map(|name| name.to_string()).collect(),
        pin,
        schema,
        by: "/work/host".into(),
        worktree: None,
        release: None,
    }
}

fn entry(id: &str, plugins: &[&str], pin: u32, schema: i64) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        root: PathBuf::from(format!("/work/{id}")),
        manifest: None,
        plugins: plugins.iter().map(|name| name.to_string()).collect(),
        pin,
        schema,
    }
}

fn registry(entries: &[Entry]) -> Registry {
    Registry {
        projects: entries.to_vec(),
    }
}

#[test]
fn a_host_serves_a_project_whose_plugins_it_holds_at_its_pin_or_an_older_one() {
    let running = build(&["shelf"], 2, 7);
    let ours = build(&[], 2, 7);
    for project in [
        entry("plain", &[], 2, 7),
        entry("older", &[], 1, 6),
        entry("shelved", &["shelf"], 2, 7),
    ] {
        assert_eq!(
            judge(&running, &ours, &project, &registry(&[])),
            Verdict::Serves,
            "{project:?}"
        );
    }
}

#[test]
fn a_project_with_no_plugins_never_replaces_a_running_host() {
    let running = build(&["shelf"], 1, 6);
    let ours = build(&[], 2, 7);
    let newer = entry("plain", &[], 2, 7);
    let Verdict::Refuse(why) = judge(&running, &ours, &newer, &registry(&[])) else {
        panic!("a plain project's newer pin is refused");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than plain's 2: a project with no \
         plugins never replaces the running Symphony: /work/host must bump its Symphony, then \
         run `just symphony` there"
    );
    let schema = entry("plain", &[], 1, 7);
    let Verdict::Refuse(why) = judge(&running, &ours, &schema, &registry(&[])) else {
        panic!("a plain project's newer board schema is refused");
    };
    assert!(why.starts_with("the running Symphony reads board schema 6, older than plain's 7"));
}

#[test]
fn a_build_holding_every_registered_plugin_replaces_a_host_that_lacks_one() {
    let running = build(&[], 1, 6);
    let ours = build(&["shelf"], 1, 6);
    let shelved = entry("shelf-project", &["shelf"], 1, 6);
    let plain = entry("plain", &[], 1, 6);
    let Verdict::Replace(why) = judge(&running, &ours, &shelved, &registry(&[plain])) else {
        panic!("the plugin project's build replaces the plain host");
    };
    assert_eq!(
        why,
        "the running Symphony lacks the plugin shelf, which shelf-project names; this build \
         serves every registered project and replaces it"
    );
    // A newer pin is replaced by a newer build that serves everyone too.
    let newer = build(&["shelf"], 2, 6);
    let bumped = entry("shelf-project", &["shelf"], 2, 6);
    assert!(matches!(
        judge(&running, &newer, &bumped, &registry(&[])),
        Verdict::Replace(_)
    ));
}

#[test]
fn a_build_that_cannot_serve_every_project_waits_or_refuses_naming_the_fix() {
    let running = build(&[], 1, 6);
    // `project add` from a plain build for a project with plugins: its own start replaces the host.
    let plain_build = build(&[], 1, 6);
    let shelved = entry("shelf-project", &["shelf", "map"], 1, 6);
    let Verdict::Waits(why) = judge(&running, &plain_build, &shelved, &registry(&[])) else {
        panic!("a plugin project waits for its own start");
    };
    assert_eq!(
        why,
        "the running Symphony lacks the plugins shelf, map, which shelf-project names: run \
         `just symphony` in /work/shelf-project, whose build holds them"
    );
    // A build lacking another registered project's plugin cannot be the one host.
    let ours = build(&["shelf", "map"], 1, 6);
    let other = entry("other", &["atlas"], 1, 6);
    let Verdict::Waits(why) = judge(&running, &ours, &shelved, &registry(&[other])) else {
        panic!("a build lacking a registered project's plugin does not replace");
    };
    assert!(
        why.ends_with(
            "this build cannot replace it: it lacks the plugin atlas, which other names; one host \
             must compile every registered project's plugins"
        ),
        "{why}"
    );
    // A build older than the running host never replaces it, whatever it holds.
    let newer_host = build(&[], 2, 6);
    let Verdict::Waits(why) = judge(&newer_host, &ours, &shelved, &registry(&[])) else {
        panic!("an older build does not replace a newer host");
    };
    assert!(
        why.ends_with("this build is older than the running Symphony: /work/shelf-project must bump its Symphony"),
        "{why}"
    );
    // A newer pin is refused, not registered, when this build cannot serve every project.
    let host = build(&["shelf", "map", "atlas"], 1, 6);
    let bumped = entry("shelf-project", &["shelf", "map"], 2, 6);
    let atlas = entry("other", &["atlas"], 1, 6);
    let Verdict::Refuse(why) = judge(
        &host,
        &build(&["shelf", "map"], 2, 6),
        &bumped,
        &registry(&[atlas]),
    ) else {
        panic!("a newer pin this build cannot host everyone at is refused");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than shelf-project's 2: this build \
         cannot replace it: it lacks the plugin atlas, which other names; one host must compile \
         every registered project's plugins"
    );
}

#[test]
fn the_project_itself_is_judged_by_its_new_entry_not_its_old_one() {
    // The registry still holds the project at its old pin; its new entry is the one judged.
    let running = build(&["shelf"], 1, 6);
    let ours = build(&["shelf"], 2, 6);
    let old = entry("shelf-project", &["shelf"], 1, 6);
    let new = entry("shelf-project", &["shelf"], 2, 6);
    assert!(matches!(
        judge(&running, &ours, &new, &registry(&[old])),
        Verdict::Replace(_)
    ));
}

#[test]
fn a_branchs_build_never_replaces_the_running_host_and_names_the_main_checkout() {
    let running = build(&[], 1, 6);
    let shelved = entry("shelf-project", &["shelf"], 1, 6);
    let ours = build(&["shelf"], 1, 6);
    let tree = "/work/trees/shelf-feature";
    // The main checkout's build replaces a host lacking the project's plugin; the same build
    // started in a linked worktree waits, registered, for the main checkout's start.
    assert!(matches!(
        judge(&running, &ours, &shelved, &registry(&[])),
        Verdict::Replace(_)
    ));
    let Verdict::Waits(why) = judge(
        &running,
        &ours.clone().in_worktree(tree),
        &shelved,
        &registry(&[]),
    ) else {
        panic!("a branch's build does not replace the host");
    };
    assert_eq!(
        why,
        "the running Symphony lacks the plugin shelf, which shelf-project names: this start runs \
         in the linked worktree /work/trees/shelf-feature, and a start in a linked worktree never \
         replaces the running Symphony: run `just symphony` in /work/shelf-project"
    );
    // A newer pin or board schema the main checkout's build would replace the host for is refused,
    // not registered, from a branch's.
    let host = build(&["shelf"], 1, 6);
    for (newer, bumped, lacks) in [
        (
            build(&["shelf"], 2, 6),
            entry("shelf-project", &["shelf"], 2, 6),
            "is at Symphony pin 1, older than shelf-project's 2",
        ),
        (
            build(&["shelf"], 1, 7),
            entry("shelf-project", &["shelf"], 1, 7),
            "reads board schema 6, older than shelf-project's 7",
        ),
    ] {
        assert!(matches!(
            judge(&host, &newer, &bumped, &registry(&[])),
            Verdict::Replace(_)
        ));
        let Verdict::Refuse(why) = judge(&host, &newer.in_worktree(tree), &bumped, &registry(&[]))
        else {
            panic!("a branch's newer build is refused");
        };
        assert_eq!(
            why,
            format!(
                "the running Symphony {lacks}: this start runs in the linked worktree \
                 /work/trees/shelf-feature, and a start in a linked worktree never replaces the \
                 running Symphony: run `just symphony` in /work/shelf-project"
            )
        );
    }
    // A host that serves the project serves it whichever checkout's build registers it.
    assert_eq!(
        judge(&host, &ours.in_worktree(tree), &shelved, &registry(&[])),
        Verdict::Serves
    );
}

#[test]
fn a_build_started_in_a_linked_worktree_is_a_branchs_and_in_a_main_checkout_is_not() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    let Others {
        submodule,
        separate,
        bare,
        bare_worktree,
    } = others(&tree.root);
    let this = Build::this(&main);
    assert_eq!(this.worktree, None);
    // A submodule's and a `--separate-git-dir` checkout's `.git` is a file naming their repository's
    // own git dir: each is its repository's main checkout.
    for dir in [
        main.clone(),
        main.join("docs"),
        tree.root.clone(),
        submodule.clone(),
        submodule.join("docs"),
        separate.clone(),
        separate.join("docs"),
        bare,
    ] {
        assert_eq!(
            this.clone().started_in(&dir).worktree,
            None,
            "{}",
            dir.display()
        );
    }
    // A bare repository's worktree is a linked one: its git dir has a `commondir`.
    for (dir, top) in [
        (linked.clone(), &linked),
        (linked.join("docs"), &linked),
        (bare_worktree.clone(), &bare_worktree),
        (bare_worktree.join("docs"), &bare_worktree),
    ] {
        assert_eq!(
            this.clone().started_in(&dir).worktree.as_ref(),
            Some(top),
            "{}",
            dir.display()
        );
    }
}

#[test]
fn a_build_reads_its_own_plugins_pin_and_schema_and_a_server_its_front() {
    let this = Build::this(Path::new("/work/here"));
    assert_eq!(this.pin, crate::machine::PIN);
    assert_eq!(this.schema, symphony_board::SCHEMA);
    assert_eq!(this.by, "/work/here");
    let front = symphony_api::Front {
        pid: 1,
        port: 2,
        root: "/work/there".into(),
        ready: true,
        app: None,
        tailnet: None,
        orphans: 0,
        plugins: vec!["shelf".into()],
        pin: 3,
        schema: 4,
        projects: vec!["there".into()],
        release: None,
    };
    assert_eq!(
        Build::of(&front),
        build(&["shelf"], 3, 4).with_by("/work/there")
    );
}

/// `build` as the server of the release `name`.
fn release(name: &str, pin: u32, schema: i64) -> Build {
    Build {
        release: Some(name.into()),
        ..build(&[], pin, schema)
    }
}

/// The entry of a project in `tree` whose file names `plugins`, those of `processes` with a
/// command, at `pin` and `schema`.
fn project(
    tree: &Tree,
    id: &str,
    plugins: &[&str],
    processes: &[&str],
    pin: u32,
    schema: i64,
) -> Entry {
    let names: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
    let mut file = format!(
        "[project]\nid = \"{id}\"\n[plugins]\ncrates = [{}]\n",
        names.join(", ")
    );
    if !processes.is_empty() {
        file.push_str("[plugins.process]\n");
        for name in processes {
            file.push_str(&format!("{name} = [\"bin/{name}\"]\n"));
        }
    }
    tree.write(&format!("{id}/{}", crate::project::FILE), &file);
    Entry {
        root: tree.path(id),
        ..entry(id, plugins, pin, schema)
    }
}

#[test]
fn any_host_serves_a_plugin_that_answers_from_its_process_and_lacks_only_a_linked_one() {
    let tree = Tree::new();
    let processes = project(
        &tree,
        "processes",
        &["shelf", "map"],
        &["shelf", "map"],
        1,
        6,
    );
    let linked = project(&tree, "linked", &["shelf", "map"], &["shelf"], 1, 6);
    for host in [build(&[], 1, 6), release("release-1", 1, 6)] {
        assert_eq!(host.lacks(&processes), None, "{host:?}");
        assert_eq!(
            host.lacks(&linked),
            Some(Lack::Plugins(vec!["map".into()])),
            "{host:?}"
        );
    }
    assert_eq!(linked.linked(), ["map"]);
    assert_eq!(build(&["map"], 1, 6).lacks(&linked), None);
}

#[test]
fn a_release_reads_a_board_one_schema_ahead_and_refuses_two_or_a_newer_pin() {
    let host = release("release-1", 2, 6);
    assert_eq!(host.lacks(&entry("ahead", &[], 2, 7)), None);
    assert_eq!(host.lacks(&entry("two", &[], 2, 8)), Some(Lack::Schema(8)));
    assert_eq!(host.lacks(&entry("pinned", &[], 3, 6)), Some(Lack::Pin(3)));
    // A project's build reads none ahead.
    assert_eq!(
        build(&[], 2, 6).lacks(&entry("ahead", &[], 2, 7)),
        Some(Lack::Schema(7))
    );
}

#[test]
fn no_start_replaces_a_release_and_each_lack_says_what_serves_the_project() {
    let running = release("release-1", 1, 6);
    // Even a build that compiles the plugin and serves every project waits.
    let ours = build(&["shelf"], 1, 6);
    let shelved = entry("shelf-project", &["shelf"], 1, 6);
    let Verdict::Waits(why) = judge(&running, &ours, &shelved, &registry(&[])) else {
        panic!("a linked plugin waits on a release");
    };
    assert_eq!(
        why,
        "the running Symphony is the release release-1, which serves a plugin only from its \
         process: shelf-project names the plugin shelf linked, and no start replaces a release's \
         server: a [plugins.process] command for shelf in /work/shelf-project/symphony.toml lets \
         it serve shelf-project (docs/engineering/PLUGIN_PROTOCOL.md)"
    );
    for (bumped, lacks) in [
        (
            entry("plain", &[], 2, 6),
            "is the release release-1, at Symphony pin 1, older than plain's 2",
        ),
        (
            entry("plain", &[], 1, 8),
            "is the release release-1, which reads board schema 6 and one ahead, older than \
             plain's 8",
        ),
    ] {
        let Verdict::Refuse(why) = judge(&running, &build(&[], 2, 8), &bumped, &registry(&[]))
        else {
            panic!("a newer project is refused by a release");
        };
        assert_eq!(
            why,
            format!(
                "the running Symphony {lacks}, and no start replaces a release's server: it \
                 serves plain once a release that does is installed, which its reload follows"
            )
        );
    }
    assert_eq!(
        judge(&running, &ours, &entry("ahead", &[], 1, 7), &registry(&[])),
        Verdict::Serves
    );
}

#[test]
fn a_start_of_a_release_never_replaces_a_running_project_build() {
    let running = build(&["shelf"], 1, 6);
    let ours = release("release-2", 2, 6);
    let bumped = entry("plain", &[], 2, 6);
    let Verdict::Refuse(why) = judge(&running, &ours, &bumped, &registry(&[])) else {
        panic!("a release's start does not replace a project's build");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than plain's 2: a start of a release \
         never replaces a running Symphony: `just symphony --stop`, then `just symphony` in \
         /work/plain, serves every registered project from the release"
    );
    // Where the release cannot serve every project, the project naming its plugin says so.
    let Verdict::Refuse(why) = judge(
        &running,
        &ours,
        &bumped,
        &registry(&[entry("shelved", &["shelf"], 1, 6)]),
    ) else {
        panic!("a release lacking a registered plugin does not replace");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than plain's 2: a start of a release \
         never replaces a running Symphony, and this one is the release release-2, which serves \
         a plugin only from its process: shelved names the plugin shelf linked: /work/host must \
         bump its Symphony, then run `just symphony` there"
    );
}

#[test]
fn a_release_build_is_its_manifest_and_a_running_one_its_front() {
    let version = super::super::release::Version { major: 0, minor: 3 };
    let manifest = Manifest {
        release: "release-9".into(),
        commit: "abc".into(),
        serves: super::super::release::Serves {
            schema: 6,
            additions: 0,
            protocol: version,
            tasks_api: version,
            pin: 2,
        },
        client: "client/0".into(),
        artifacts: Vec::new(),
    };
    let released = Build::released(&manifest, Path::new("/work/alpha"));
    assert_eq!(
        released,
        Build {
            by: "/work/alpha".into(),
            ..release("release-9", 2, 6)
        }
    );
    let front = symphony_api::Front {
        pid: 1,
        port: 2,
        root: "/work/there".into(),
        ready: true,
        app: None,
        tailnet: None,
        orphans: 0,
        plugins: Vec::new(),
        pin: 2,
        schema: 6,
        projects: Vec::new(),
        release: Some("release-9".into()),
    };
    assert_eq!(Build::of(&front).release.as_deref(), Some("release-9"));
}

impl Build {
    fn with_by(self, by: &str) -> Build {
        Build {
            by: by.into(),
            ..self
        }
    }

    /// This build as started in the linked worktree `tree`.
    fn in_worktree(self, tree: &str) -> Build {
        Build {
            worktree: Some(tree.into()),
            ..self
        }
    }
}
