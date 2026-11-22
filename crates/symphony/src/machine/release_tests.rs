//! A release installed whole in a temporary machine folder, its `current` link and what is found
//! from a binary's own place.

use super::*;
use crate::test_tree::Tree;
use std::os::unix::fs::PermissionsExt;

const SERVES: Serves = Serves {
    schema: 18,
    additions: 0,
    protocol: Version { major: 0, minor: 3 },
    tasks_api: Version {
        major: 1,
        minor: 14,
    },
    pin: 1,
};

/// A build in `tree`: each binary of `names` in `build/`, saying its name, and a client of three
/// files in `client/`.
fn build(tree: &Tree, names: &[&str]) {
    for name in names {
        let path = tree.write(
            &format!("build/{name}"),
            &format!("#!/bin/sh\necho {name}\n"),
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    tree.write("client/index.html", "<script src=\"/assets/app-1.js\">");
    tree.write("client/assets/app-1.js", "app()");
    tree.write("client/builds.json", "{}");
}

fn built<'a>(tree: &'a Tree, release: &'a str, binaries: &'a Path, client: &'a Path) -> Built<'a> {
    let _ = tree;
    Built {
        release,
        commit: "0123abcd",
        serves: SERVES,
        binaries,
        client,
    }
}

/// `release` installed in `tree`'s `machine/` from its `build/` and `client/`.
fn installs(tree: &Tree, release: &str) -> Result<Manifest, String> {
    let (binaries, client) = (tree.path("build"), tree.path("client"));
    install(
        &tree.path("machine"),
        &built(tree, release, &binaries, &client),
    )
}

/// What `machine/releases` holds, by name, in order.
fn releases(tree: &Tree) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(tree.path("machine/releases"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn an_install_is_the_binaries_the_client_and_a_manifest_naming_each_artifacts_size() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let manifest = installs(&tree, "release-2026-11-20").unwrap();
    let machine = tree.path("machine");
    let place = folder(&machine, "release-2026-11-20");
    assert_eq!(releases(&tree), ["release-2026-11-20"], "no staging left");
    for binary in BINARIES {
        assert!(crate::builds::runnable(&place.join("bin").join(binary)));
    }
    let hash = manifest.client.strip_prefix("client/").unwrap();
    assert_eq!(hash.len(), 16, "{hash}");
    assert!(hash.chars().all(|digit| digit.is_ascii_hexdigit()));
    for (file, text) in [
        ("index.html", "<script src=\"/assets/app-1.js\">"),
        ("assets/app-1.js", "app()"),
        ("builds.json", "{}"),
    ] {
        let copied = place.join(&manifest.client).join(file);
        assert_eq!(fs::read_to_string(copied).unwrap(), text);
    }
    assert_eq!(manifest.release, "release-2026-11-20");
    assert_eq!(manifest.commit, "0123abcd");
    assert_eq!(manifest.serves, SERVES);
    let sizes: Vec<(&str, u64, Option<u64>)> = (manifest.artifacts.iter())
        .map(|artifact| (artifact.path.as_str(), artifact.size, artifact.files))
        .collect();
    let server = "#!/bin/sh\necho symphony-server\n".len() as u64;
    assert_eq!(
        sizes,
        [
            ("bin/symphony-server", server, None),
            ("bin/tasks", server - 10, None),
            ("bin/agent-send", server - 5, None),
            ("bin/symphony", server - 7, None),
            ("bin/tasks-move", server - 5, None),
            (manifest.client.as_str(), 31 + 5 + 2, Some(3)),
        ]
    );
    // As written: the file reads back the same, and names what the release serves flat.
    assert_eq!(
        installed(&machine, "release-2026-11-20"),
        Ok(Some(manifest.clone()))
    );
    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(place.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(raw["schema"], 18);
    assert_eq!(raw["protocol"]["major"], 0);
    assert_eq!(raw["protocol"]["minor"], 3);
    assert_eq!(raw["tasks_api"]["minor"], 14);
    assert_eq!(raw["artifacts"][1]["path"], "bin/tasks");
    assert!(raw["artifacts"][1].get("files").is_none());
    assert_eq!(
        manifest.sizes(),
        format!(
            "symphony-server {server} bytes, tasks {} bytes, agent-send {} bytes, symphony {} \
             bytes, tasks-move {} bytes, {} 38 bytes in 3 files",
            server - 10,
            server - 5,
            server - 7,
            server - 5,
            manifest.client
        )
    );
}

#[test]
fn sizes_are_said_with_their_thousands_apart() {
    for (n, said) in [
        (0, "0"),
        (999, "999"),
        (1000, "1,000"),
        (31_675_888, "31,675,888"),
        (295_428_896, "295,428,896"),
        (1_000_000, "1,000,000"),
    ] {
        assert_eq!(grouped(n), said);
    }
    let one = Manifest {
        release: "r".into(),
        commit: "c".into(),
        serves: SERVES,
        client: "client/ab".into(),
        artifacts: vec![Artifact {
            path: "client/ab".into(),
            size: 1234,
            files: Some(1),
        }],
    };
    assert_eq!(one.sizes(), "client/ab 1,234 bytes in 1 file");
}

#[test]
fn the_same_client_has_the_same_name_and_any_change_another() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let first = installs(&tree, "release-1").unwrap();
    let second = installs(&tree, "release-2").unwrap();
    assert_eq!(first.client, second.client);
    tree.write("client/assets/app-1.js", "app(2)");
    let changed = installs(&tree, "release-3").unwrap();
    assert_ne!(changed.client, first.client);
    // A file's name is part of it: the same bytes moved are another client.
    fs::rename(
        tree.path("client/assets/app-1.js"),
        tree.path("client/assets/app-2.js"),
    )
    .unwrap();
    let moved = installs(&tree, "release-4").unwrap();
    assert_ne!(moved.client, changed.client);
}

#[test]
fn a_failed_install_leaves_no_folder() {
    let tree = Tree::new();
    // No `symphony`, the first it misses: three of the five binaries were built.
    build(&tree, &BINARIES[..3]);
    let missing = installs(&tree, "release-1").unwrap_err();
    assert_eq!(
        missing,
        format!(
            "the build has no symphony that runs at {}",
            tree.path("build/symphony").display()
        )
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    // One that does not run is not one.
    tree.write("build/symphony", "not runnable\n");
    assert!(
        installs(&tree, "release-1")
            .unwrap_err()
            .starts_with("the build has no symphony that runs")
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    // A client with no page.
    build(&tree, &BINARIES);
    fs::remove_file(tree.path("client/index.html")).unwrap();
    assert_eq!(
        installs(&tree, "release-1").unwrap_err(),
        format!(
            "the client's build at {} has no index.html",
            tree.path("client").display()
        )
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    // A client holding a link: its files alone are a build.
    build(&tree, &BINARIES);
    std::os::unix::fs::symlink("index.html", tree.path("client/linked.html")).unwrap();
    assert!(
        installs(&tree, "release-1")
            .unwrap_err()
            .ends_with("a client's build holds regular files alone")
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    assert_eq!(installed(&tree.path("machine"), "release-1"), Ok(None));
}

#[test]
fn an_installed_release_is_never_written_into_and_a_name_that_is_no_folders_is_refused() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let first = installs(&tree, "release-1").unwrap();
    tree.write("client/assets/app-1.js", "app(2)");
    let again = installs(&tree, "release-1").unwrap_err();
    assert!(
        again.ends_with("is already there, and an installed release is never written into"),
        "{again}"
    );
    assert_eq!(
        installed(&tree.path("machine"), "release-1"),
        Ok(Some(first))
    );
    assert_eq!(releases(&tree), ["release-1"]);
    for name in ["", ".", "..", ".hidden", "a/b", "../up"] {
        assert_eq!(
            installs(&tree, name).unwrap_err(),
            format!("{name:?} names no release folder")
        );
        assert!(point(&tree.path("machine"), name).is_err());
    }
    assert_eq!(releases(&tree), ["release-1"]);
}

#[test]
fn what_an_install_killed_outright_left_is_cleared_by_the_next_of_that_release() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    tree.write(
        "machine/releases/.release-1.installing.999999/bin/tasks",
        "",
    );
    tree.write(
        "machine/releases/.release-2.installing.999999/bin/tasks",
        "",
    );
    installs(&tree, "release-1").unwrap();
    assert_eq!(
        releases(&tree),
        [".release-2.installing.999999", "release-1"],
        "another release's is not this install's to clear"
    );
}

#[test]
fn a_manifest_that_cannot_be_read_is_an_error_and_never_no_release() {
    let tree = Tree::new();
    tree.write("machine/releases/release-1/manifest.json", "{");
    let problem = installed(&tree.path("machine"), "release-1").unwrap_err();
    assert!(problem.contains("manifest.json: "), "{problem}");
    assert!(point(&tree.path("machine"), "release-1").is_err());
}

#[test]
fn current_is_renamed_over_and_names_the_release_before() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let machine = tree.path("machine");
    assert_eq!(current(&machine), None);
    installs(&tree, "release-1").unwrap();
    installs(&tree, "release-2").unwrap();
    assert_eq!(current(&machine), None, "an install points nothing");
    assert_eq!(point(&machine, "release-1"), Ok(None));
    assert_eq!(current(&machine).as_deref(), Some("release-1"));
    // The link is relative, so the machine folder moves whole.
    assert_eq!(
        fs::read_link(machine.join(CURRENT)).unwrap(),
        Path::new("releases/release-1")
    );
    assert_eq!(point(&machine, "release-2"), Ok(Some("release-1".into())));
    assert_eq!(current(&machine).as_deref(), Some("release-2"));
    assert_eq!(
        crate::builds::released(&machine, "tasks"),
        Some(machine.join("current/bin/tasks"))
    );
    // The way back is the same act.
    assert_eq!(point(&machine, "release-1"), Ok(Some("release-2".into())));
    assert_eq!(point(&machine, "release-1"), Ok(Some("release-1".into())));
    // A release that is not installed is never pointed at, and the link stays.
    assert_eq!(
        point(&machine, "release-9").unwrap_err(),
        format!(
            "release-9 is not installed in {}",
            machine.join("releases").display()
        )
    );
    assert_eq!(current(&machine).as_deref(), Some("release-1"));
    let left: Vec<String> = fs::read_dir(&machine)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert_eq!(left, [] as [&str; 0], "no temporary link left");
}

#[test]
fn a_current_that_is_a_folder_or_names_another_place_is_not_a_release() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let machine = tree.path("machine");
    installs(&tree, "release-1").unwrap();
    fs::create_dir_all(machine.join(CURRENT)).unwrap();
    assert_eq!(current(&machine), None);
    assert_eq!(
        point(&machine, "release-1").unwrap_err(),
        format!("{} is not a link", machine.join(CURRENT).display())
    );
    fs::remove_dir(machine.join(CURRENT)).unwrap();
    std::os::unix::fs::symlink("/elsewhere/release-1", machine.join(CURRENT)).unwrap();
    assert_eq!(current(&machine), None, "not a folder of releases/");
    // Pointing replaces it, naming no release before.
    assert_eq!(point(&machine, "release-1"), Ok(None));
    assert_eq!(current(&machine).as_deref(), Some("release-1"));
}

#[test]
fn a_binary_knows_its_release_and_its_client_through_the_current_link_and_no_other_does() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    let machine = tree.path("machine");
    let manifest = installs(&tree, "release-1").unwrap();
    point(&machine, "release-1").unwrap();
    let place = folder(&machine, "release-1").canonicalize().unwrap();
    for binary in [
        machine.join("current/bin/symphony-server"),
        place.join("bin/symphony-server"),
    ] {
        assert_eq!(
            of_binary(&binary),
            Some((place.clone(), manifest.clone())),
            "{}",
            binary.display()
        );
        assert_eq!(client_beside(&binary), Some(place.join(&manifest.client)));
    }
    // A checkout's build, a binary that is not there, and a `bin/` under no `releases/`.
    assert_eq!(of_binary(&tree.path("build/symphony-server")), None);
    assert_eq!(of_binary(&tree.path("gone/bin/symphony-server")), None);
    let stray = tree.write("stray/release-1/bin/symphony-server", "");
    tree.write("stray/release-1/manifest.json", "{}");
    assert_eq!(of_binary(&stray), None);
    // A release whose manifest names a client outside it, or one with no page, serves none.
    let outside = Manifest {
        client: "../release-1/client".into(),
        ..manifest.clone()
    };
    tree.write(
        "machine/releases/release-2/manifest.json",
        &serde_json::to_string(&outside).unwrap(),
    );
    let second = tree.write("machine/releases/release-2/bin/symphony-server", "");
    assert!(of_binary(&second).is_some());
    assert_eq!(client_beside(&second), None);
    let pageless = Manifest {
        client: "client/none".into(),
        ..manifest
    };
    tree.write(
        "machine/releases/release-2/manifest.json",
        &serde_json::to_string(&pageless).unwrap(),
    );
    assert_eq!(client_beside(&second), None);
    tree.write("machine/releases/release-2/manifest.json", "{");
    assert_eq!(of_binary(&second), None);
}

#[test]
fn an_install_holds_its_notes_beside_the_manifest_only_when_given_any() {
    let tree = Tree::new();
    build(&tree, &BINARIES);
    installs(&tree, "release-2026-11-20").unwrap();
    let noteless = folder(&tree.path("machine"), "release-2026-11-20");
    assert!(noteless.join(MANIFEST).is_file());
    assert!(!noteless.join(NOTES).exists());
    assert_eq!(noted(&tree.path("machine")), None);

    let (binaries, client) = (tree.path("build"), tree.path("client"));
    install_noted(
        &tree.path("machine"),
        &built(&tree, "release-2026-11-21", &binaries, &client),
        Some("## release-2026-11-21\n\nBoard:\n- a step\n"),
    )
    .unwrap();
    assert_eq!(
        noted(&tree.path("machine")),
        Some((
            "release-2026-11-21".into(),
            "## release-2026-11-21\n\nBoard:\n- a step\n".into()
        ))
    );
}

#[test]
fn the_notes_read_are_the_newest_noted_releases_by_day_then_number() {
    let tree = Tree::new();
    assert_eq!(noted(&tree.path("machine")), None, "no releases folder");
    for (name, notes) in [
        ("release-2026-11-20", Some("first")),
        ("release-2026-11-20.9", Some("ninth")),
        ("release-2026-11-20.10", Some("tenth")),
        // Newer, and installed before a release wrote notes: passed over.
        ("release-2026-11-21", None),
        // No release's name: never read.
        ("release-9999", Some("no release")),
        (".release-2026-12-01.installing.1", Some("half written")),
    ] {
        tree.write(&format!("machine/releases/{name}/{MANIFEST}"), "{}");
        if let Some(notes) = notes {
            tree.write(&format!("machine/releases/{name}/{NOTES}"), notes);
        }
    }
    assert_eq!(
        noted(&tree.path("machine")),
        Some(("release-2026-11-20.10".into(), "tenth".into()))
    );
}

#[test]
fn a_name_is_a_releases_by_its_day_and_its_number_from_two() {
    for name in [
        "release-2026-11-20",
        "release-2026-11-20.2",
        "release-2026-11-20.10",
    ] {
        assert!(named(name), "{name}");
    }
    for name in [
        "release-2026-11-20.1",
        "release-2026-11-20.x",
        "release-2026-11-2",
        "release-20261120",
        "release-candidate",
        "2026-11-20",
        "",
    ] {
        assert!(!named(name), "{name}");
    }
}

/// Notes of four releases, the day's tenth after its second.
const FOUR: &str = "# Notes\n\nOldest first.\n\n## release-2026-11-18\n\nCommit: \
                    aaaaaaa1111111111111111111111111111111aa\nThe first release.\n\nfirst\n\n## \
                    release-2026-11-18.2\n\nCommit: bbbbbbb2222222222222222222222222222222bb\nFrom \
                    release-2026-11-18.\n\nsecond\n\n## release-2026-11-18.10\n\nCommit: \
                    ccccccc3333333333333333333333333333333cc\n\ntenth\n\n## \
                    release-2026-11-19\n\nlast, which names no commit\n";

#[test]
fn the_notes_between_two_releases_are_those_after_the_first_up_to_the_second_in_order() {
    let between = |from, to| notes_between(FOUR, from, to).unwrap();
    assert_eq!(
        between(Some("release-2026-11-18"), Some("release-2026-11-18.10")),
        "## release-2026-11-18.2\n\nCommit: bbbbbbb2222222222222222222222222222222bb\nFrom \
         release-2026-11-18.\n\nsecond\n\n## release-2026-11-18.10\n\nCommit: \
         ccccccc3333333333333333333333333333333cc\n\ntenth\n"
    );
    assert_eq!(
        between(None, Some("release-2026-11-18")),
        "## release-2026-11-18\n\nCommit: aaaaaaa1111111111111111111111111111111aa\nThe first \
         release.\n\nfirst\n"
    );
    assert!(between(None, None).starts_with("## release-2026-11-18\n"));
    assert!(!between(None, None).contains("Oldest first"));
    assert!(between(None, None).ends_with("last, which names no commit\n"));
    // At the newest, and a span that ends before it starts: nothing to take.
    assert_eq!(between(Some("release-2026-11-19"), None), "");
    assert_eq!(
        between(Some("release-2026-11-18.10"), Some("release-2026-11-18")),
        ""
    );
}

#[test]
fn a_bound_may_be_a_releases_commit_whole_or_its_first_seven_characters() {
    let between = |from, to| notes_between(FOUR, from, to).unwrap();
    let by_name = between(Some("release-2026-11-18.2"), Some("release-2026-11-18.10"));
    for from in [
        "bbbbbbb2222222222222222222222222222222bb",
        "bbbbbbb",
        "BBBBBBB2222",
    ] {
        assert_eq!(between(Some(from), Some("ccccccc3333")), by_name, "{from}");
    }
}

#[test]
fn a_bound_the_notes_do_not_hold_is_refused_with_what_they_hold() {
    for (from, to, why) in [
        (
            Some("main"),
            None,
            "\"main\" names no release (release-YYYY-MM-DD",
        ),
        (None, Some("release-2026-11-18.1"), "names no release"),
        // Six characters name no commit.
        (Some("bbbbbb"), None, "names no release"),
        (
            Some("release-2026-11-17"),
            None,
            "the notes say nothing of release-2026-11-17: they hold release-2026-11-18 to \
             release-2026-11-19",
        ),
        (
            None,
            Some("release-2026-11-20"),
            "the notes say nothing of release-2026-11-20: they hold",
        ),
        (
            Some("1b07a67d6be2"),
            None,
            "1b07a67d6be2 is the commit of no release the notes hold (they hold \
             release-2026-11-18 to release-2026-11-19): name the newest release that commit \
             holds, which `git describe --tags --match 'release-*' --abbrev=0 1b07a67d6be2` says",
        ),
    ] {
        let refused = notes_between(FOUR, from, to).unwrap_err();
        assert!(refused.contains(why), "{from:?} {to:?}: {refused}");
    }
    let refused = notes_between("no release here\n", Some("release-2026-11-18"), None).unwrap_err();
    assert!(refused.ends_with("they hold no release"), "{refused}");
    assert_eq!(notes_between("no release here\n", None, None).unwrap(), "");
}
