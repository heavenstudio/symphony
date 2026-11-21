//! The switch: what holds the machine on a project's build, the release a start runs in its place,
//! and the lines `just release --status` prints of it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::*;
use crate::machine::PIN;
use crate::machine::release::{self, Built, Serves, Version};
use crate::project::FILE;
use crate::test_tree::Tree;

const RELEASE: &str = "release-2026-11-20";

/// A project in `tree` at `folder` whose file names `plugins`, those of `processes` with a command.
fn project(tree: &Tree, folder: &str, plugins: &[&str], processes: &[&str]) -> Entry {
    let mut file = format!("[project]\nid = \"{folder}\"\n");
    if !plugins.is_empty() {
        let names: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
        file.push_str(&format!("[plugins]\ncrates = [{}]\n", names.join(", ")));
    }
    if !processes.is_empty() {
        file.push_str("[plugins.process]\n");
        for name in processes {
            file.push_str(&format!("{name} = [\"bin/{name}\"]\n"));
        }
    }
    tree.write(&format!("{folder}/{FILE}"), &file);
    Entry::of(&tree.path(folder)).unwrap()
}

fn registry(entries: &[&Entry]) -> Registry {
    Registry {
        projects: entries.iter().map(|entry| (*entry).clone()).collect(),
    }
}

/// `name` installed whole in `tree`'s `machine/`, at `pin` and `schema`, and `current` pointed at
/// it: the machine folder.
fn installed(tree: &Tree, name: &str, pin: u32, schema: i64) -> PathBuf {
    let build = tree.path(&format!("build-{name}"));
    for binary in release::BINARIES {
        let path = tree.write(
            &format!("build-{name}/{binary}"),
            &format!("#!/bin/sh\necho {binary}\n"),
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let client = tree.path(&format!("client-{name}"));
    tree.write(&format!("client-{name}/index.html"), "<p>");
    let machine = tree.path("machine");
    let version = Version { major: 1, minor: 0 };
    release::install(
        &machine,
        &Built {
            release: name,
            commit: "0123abcd",
            serves: Serves {
                schema,
                additions: 0,
                protocol: version,
                tasks_api: version,
                pin,
            },
            binaries: &build,
            client: &client,
        },
    )
    .unwrap();
    release::point(&machine, name).unwrap();
    machine
}

/// The registry of `machine` holding `entries`.
fn registered(machine: &Path, entries: &[&Entry]) {
    Registry::change(machine, |held| {
        for entry in entries {
            held.add((*entry).clone())?;
        }
        Ok(())
    })
    .unwrap();
}

/// `machine`'s record of the one server: on `port` as `pid`, started for `root`, of `release`.
fn recorded(machine: &Path, root: &str, release: Option<&str>) {
    let release = release.map_or(String::new(), |name| format!(",\"release\":\"{name}\""));
    fs::write(
        machine.join(super::super::command::RECORD),
        format!("{{\"port\":8,\"pid\":9,\"root\":\"{root}\"{release}}}"),
    )
    .unwrap();
}

#[test]
fn a_project_naming_a_linked_plugin_holds_the_switch_and_one_whose_plugins_are_processes_does_not()
{
    let tree = Tree::new();
    let plain = project(&tree, "plain", &[], &[]);
    let served = project(&tree, "served", &["shelf", "map"], &["shelf", "map"]);
    let linked = project(&tree, "linked", &["shelf", "atlas", "map"], &["shelf"]);
    assert_eq!(held(&registry(&[&plain, &served])), None);
    assert_eq!(
        held(&registry(&[&plain, &linked, &served])),
        Some(Hold {
            id: "linked".into(),
            root: linked.root.clone(),
            plugins: vec!["atlas".into(), "map".into()],
        })
    );
    // The file is read as it is now: a command written since it registered lets the switch go.
    project(
        &tree,
        "linked",
        &["shelf", "atlas", "map"],
        &["shelf", "atlas", "map"],
    );
    assert_eq!(held(&registry(&[&plain, &linked, &served])), None);
}

#[test]
fn a_hold_names_its_project_its_plugins_and_where_their_commands_go() {
    let one = Hold {
        id: "pantry".into(),
        root: PathBuf::from("/work/pantry"),
        plugins: vec!["shelf".into()],
    };
    assert_eq!(
        one.said(),
        "pantry names the linked plugin shelf, and a release's server serves a plugin only from \
         its process: a [plugins.process] command for it in /work/pantry/symphony.toml lets the \
         release serve every project (docs/PLUGIN_PROTOCOL.md)"
    );
    // Its own pages name the file as its own; the status and a start keep its path.
    assert_eq!(
        one.said_on_its_pages(),
        "pantry names the linked plugin shelf, and a release's server serves a plugin only from \
         its process: a [plugins.process] command for it in its symphony.toml lets the release \
         serve every project (docs/PLUGIN_PROTOCOL.md)"
    );
    let two = Hold {
        plugins: vec!["shelf".into(), "map".into()],
        ..one
    };
    assert!(
        two.said().starts_with(
            "pantry names the linked plugins shelf, map, and a release's server serves a plugin \
             only from its process: a [plugins.process] command for each in"
        ),
        "{}",
        two.said()
    );
}

#[test]
fn a_start_runs_the_installed_release_by_its_own_folder_when_it_serves_every_project() {
    let tree = Tree::new();
    let alpha = project(&tree, "alpha", &[], &[]);
    let pantry = project(&tree, "pantry", &["shelf"], &["shelf"]);
    let none = tree.path("empty-machine");
    assert_eq!(to_start(&none, &registry(&[&pantry]), &alpha), None);
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    let Some((server, build)) = to_start(&machine, &registry(&[&pantry]), &alpha) else {
        panic!("the release serves both projects");
    };
    assert_eq!(
        server,
        release::folder(&machine, RELEASE).join("bin/symphony-server")
    );
    assert_eq!(build.release.as_deref(), Some(RELEASE));
    assert!(build.plugins.is_empty());
    // A project naming a linked plugin, beside or starting, keeps the start on its own build.
    let linked = project(&tree, "linked", &["shelf"], &[]);
    assert_eq!(to_start(&machine, &registry(&[&linked]), &alpha), None);
    assert_eq!(to_start(&machine, &registry(&[&alpha]), &linked), None);
}

#[test]
fn a_start_runs_a_release_one_board_schema_behind_and_never_one_at_an_older_pin() {
    let tree = Tree::new();
    let alpha = project(&tree, "alpha", &[], &[]);
    let behind = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA - 1);
    assert!(to_start(&behind, &registry(&[]), &alpha).is_some());
    let tree = Tree::new();
    let alpha = project(&tree, "alpha", &[], &[]);
    let two_behind = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA - 2);
    assert_eq!(to_start(&two_behind, &registry(&[]), &alpha), None);
    let tree = Tree::new();
    let alpha = project(&tree, "alpha", &[], &[]);
    let unpinned = installed(&tree, RELEASE, 0, symphony_board::SCHEMA);
    assert_eq!(to_start(&unpinned, &registry(&[]), &alpha), None);
}

#[test]
fn a_release_not_installed_whole_is_never_started() {
    let tree = Tree::new();
    let alpha = project(&tree, "alpha", &[], &[]);
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    fs::remove_file(release::folder(&machine, RELEASE).join("bin/symphony-server")).unwrap();
    assert_eq!(release::current_installed(&machine), None);
    assert_eq!(to_start(&machine, &registry(&[]), &alpha), None);
}

#[test]
fn a_linked_plugin_holds_the_switch_before_a_project_the_release_is_too_old_for() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, 0, symphony_board::SCHEMA);
    let build = Build::released(&release::current_installed(&machine).unwrap(), &machine);
    let alpha = project(&tree, "alpha", &[], &[]);
    let served = project(&tree, "served", &["map"], &["map"]);
    let linked = project(&tree, "linked", &["shelf"], &[]);
    // The release at pin 0 is too old for every project: the first is named, with its lack.
    let Some((id, root, why)) = holding(&build, &registry(&[&served, &alpha])) else {
        panic!("a release at pin 0 serves no project");
    };
    assert_eq!((id.as_str(), &root), ("served", &served.root));
    assert_eq!(
        why,
        format!(
            "its server is the release {RELEASE}, at Symphony pin 0, older than served's {PIN}"
        )
    );
    // A linked plugin anywhere in the registry is named first, as the refresh reads it.
    let Some((id, root, why)) = holding(&build, &registry(&[&alpha, &linked])) else {
        panic!("a linked plugin holds the switch");
    };
    assert_eq!((id.as_str(), &root), ("linked", &linked.root));
    assert!(
        why.starts_with("linked names the linked plugin shelf, "),
        "{why}"
    );
    // A release at this pin holds nothing for projects it serves.
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    let build = Build::released(&release::current_installed(&machine).unwrap(), &machine);
    let alpha = project(&tree, "alpha", &[], &[]);
    let served = project(&tree, "served", &["map"], &["map"]);
    assert_eq!(holding(&build, &registry(&[&alpha, &served])), None);
}

#[test]
fn a_start_by_a_release_while_the_switch_is_held_is_refused_naming_the_holder_and_its_command() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    let alpha = project(&tree, "alpha", &[], &[]);
    let linked = project(&tree, "linked", &["shelf"], &[]);
    assert_eq!(
        refused(&machine, &registry(&[&alpha, &linked]), RELEASE),
        format!(
            "the machine's Symphony is not started from the release {RELEASE}, as the switch to it \
             is held: linked names the linked plugin shelf, and a release's server serves a plugin \
             only from its process: a [plugins.process] command for it in {} lets the release \
             serve every project (docs/PLUGIN_PROTOCOL.md); `just symphony` in {}, by linked's own \
             build, starts the machine's server",
            linked.root.join(FILE).display(),
            linked.root.display()
        )
    );
    // A project the release is too old for holds it the same way.
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, 0, symphony_board::SCHEMA);
    let alpha = project(&tree, "alpha", &[], &[]);
    assert_eq!(
        refused(&machine, &registry(&[&alpha]), RELEASE),
        format!(
            "the machine's Symphony is not started from the release {RELEASE}, as the switch to it \
             is held: its server is the release {RELEASE}, at Symphony pin 0, older than alpha's \
             {PIN}; `just symphony` in {}, by alpha's own build, starts the machine's server",
            alpha.root.display()
        )
    );
    // Nothing holding it: current moved between the reads, and a start again judges it anew.
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    assert_eq!(
        refused(&machine, &registry(&[]), RELEASE),
        format!(
            "the machine's Symphony is not started from the release {RELEASE}: {} moved as it was \
             read; `just symphony` again judges it anew",
            machine.join(release::CURRENT).display()
        )
    );
    // No release installed whole: a release's server starts only as the one current names.
    fs::remove_file(release::folder(&machine, RELEASE).join("bin/symphony-server")).unwrap();
    assert_eq!(
        refused(&machine, &registry(&[]), "release-2026-11-01"),
        format!(
            "the machine's Symphony is not started from the release release-2026-11-01: {} names \
             no release installed whole in {}, and a release's server is started only as the \
             release it names",
            machine.join(release::CURRENT).display(),
            machine.join(release::RELEASES).display()
        )
    );
}

#[test]
fn the_standing_says_no_release_or_a_current_naming_none_installed() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    fs::create_dir_all(&machine).unwrap();
    assert_eq!(
        standing(&machine),
        format!(
            "machine: no release is installed in {}: a project's build is the machine's server\n",
            machine.display()
        )
    );
    std::os::unix::fs::symlink("releases/release-gone", machine.join(release::CURRENT)).unwrap();
    assert_eq!(
        standing(&machine),
        format!(
            "machine: {} names release-gone, which is not installed whole in {}\n",
            machine.join(release::CURRENT).display(),
            machine.join(release::RELEASES).display()
        )
    );
}

#[test]
fn the_standing_of_releases_installed_and_never_pointed_at_names_them_and_the_hold() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    installed(&tree, "release-2026-11-21", PIN, symphony_board::SCHEMA);
    // The refresh installs them and points current at none while the switch is held.
    fs::remove_file(machine.join(release::CURRENT)).unwrap();
    // An install's hidden folder and a folder that is no whole release are no releases.
    tree.write(
        &format!("machine/releases/.{RELEASE}.1/manifest.json"),
        "{}",
    );
    tree.write("machine/releases/broken/manifest.json", "{");
    let releases = machine.join(release::RELEASES);
    let line = |names: &str| {
        format!(
            "machine: current names no release, and {names} installed in {}: a project's build is \
             the machine's server\n",
            releases.display()
        )
    };
    let alpha = project(&tree, "alpha", &[], &[]);
    registered(&machine, &[&alpha]);
    assert_eq!(
        standing(&machine),
        line(&format!("{RELEASE}, release-2026-11-21 are"))
    );
    let linked = project(&tree, "linked", &["shelf"], &[]);
    registered(&machine, &[&linked]);
    let said = standing(&machine);
    assert!(
        said.starts_with(&line(&format!("{RELEASE}, release-2026-11-21 are")))
            && said.contains("\nmachine: the switch is held: linked names the linked plugin shelf"),
        "{said}"
    );
    fs::remove_dir_all(releases.join("release-2026-11-21")).unwrap();
    assert!(standing(&machine).starts_with(&line(&format!("{RELEASE} is"))));
}

#[test]
fn the_standing_names_the_hold_or_what_makes_the_switch() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    let alpha = project(&tree, "alpha", &[], &[]);
    registered(&machine, &[&alpha]);
    assert_eq!(
        standing(&machine),
        format!(
            "machine: current names {RELEASE}; no server is recorded\nmachine: nothing holds the \
             switch to {RELEASE}: the next start of the one server runs it\n"
        )
    );
    recorded(&machine, "/work/alpha", None);
    assert_eq!(
        standing(&machine),
        format!(
            "machine: current names {RELEASE}; the machine's server (port 8, pid 9) is the build \
             of /work/alpha\nmachine: nothing holds the switch to {RELEASE}: `just symphony \
             --stop`, then `just symphony`, makes it the machine's server\n"
        )
    );
    let linked = project(&tree, "linked", &["shelf"], &[]);
    registered(&machine, &[&linked]);
    let said = standing(&machine);
    let held =
        format!("machine: the switch to {RELEASE} is held: linked names the linked plugin shelf");
    assert!(said.contains(&held), "{said}");
}

#[test]
fn the_standing_names_a_project_the_release_is_too_old_for() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, 0, symphony_board::SCHEMA);
    let alpha = project(&tree, "alpha", &[], &[]);
    registered(&machine, &[&alpha]);
    let said = standing(&machine);
    assert!(
        said.ends_with(&format!(
            "machine: the switch to {RELEASE} is held: its server is the release {RELEASE}, at \
             Symphony pin 0, older than alpha's {PIN}\n"
        )),
        "{said}"
    );
}

#[test]
fn the_standing_of_a_release_server_names_the_release_it_runs() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    let linked = project(&tree, "linked", &["shelf"], &[]);
    registered(&machine, &[&linked]);
    recorded(&machine, "/work/alpha", Some(RELEASE));
    assert_eq!(
        standing(&machine),
        format!("machine: current names {RELEASE}; the machine's server (port 8, pid 9) runs it\n")
    );
    recorded(&machine, "/work/alpha", Some("release-2026-11-01"));
    assert_eq!(
        standing(&machine),
        format!(
            "machine: current names {RELEASE}; the machine's server (port 8, pid 9) runs the \
             release release-2026-11-01 until its reload follows current\n"
        )
    );
}

#[test]
fn the_standing_says_a_registry_that_does_not_read() {
    let tree = Tree::new();
    let machine = installed(&tree, RELEASE, PIN, symphony_board::SCHEMA);
    fs::write(machine.join(super::super::PROJECTS), "not json").unwrap();
    let said = standing(&machine);
    assert!(
        said.contains("machine: the registry cannot be read: "),
        "{said}"
    );
}
