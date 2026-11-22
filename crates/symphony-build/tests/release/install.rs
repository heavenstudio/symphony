//! A green release's install on the machine: the release built by a stand-in for cargo in the
//! run's own checkout, installed whole in a temporary machine folder with its manifest, and
//! `current` pointed at it where no project is registered to hold it; a build that fails leaves no
//! folder and `current` where it was, and is the refresh's red, which a later refresh that installs
//! closes; on the tests' clock with no stand-in, nothing is built or installed.

use std::fs;
use std::path::PathBuf;

use super::consumers::register;
use super::refresh::refreshing;
use super::{CHECK, DAY, Fixture, NOW};

/// The release a run of the fixture's clock tags.
const RELEASE: &str = "release-2026-11-12";
/// What names the stand-in for cargo.
const BUILDER: &str = "SYMPHONY_RELEASE_BUILDER";

/// The refresh's stand-in for cargo ([`super::refresh::builder`]), building releases at schema 18.
fn builder(fixture: &Fixture) -> String {
    super::refresh::builder(fixture, 18)
}

/// The fixture with the refresh on and no project registered: the commit a run releases.
fn installing() -> (Fixture, String) {
    let (fixture, commit) = refreshing();
    register(&fixture, &[]);
    (fixture, commit)
}

fn machine(fixture: &Fixture) -> PathBuf {
    fixture.root.join("machine")
}

fn builds(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(fixture.root.join("builds"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_green_release_is_built_once_in_the_runs_checkout_and_installed_on_the_machine() {
    let (fixture, _) = installing();
    // On the tests' clock with no stand-in for cargo, a release builds and installs nothing.
    let out = fixture.night(NOW - DAY, 0);
    assert!(!out.contains("refresh-install"), "{out}");
    assert!(!machine(&fixture).join("releases").exists());
    assert!(builds(&fixture).is_empty());

    // The next day's, with the stand-in.
    let commit = fixture.land("crates/core/notes.txt", Some("b\n"), "a landing after it");
    let builder = builder(&fixture);
    let (code, out, err) =
        fixture.release_with(NOW, &["--check", CHECK], &[(BUILDER, builder.as_str())]);
    assert_eq!(code, 0, "{out}{err}");
    let place = machine(&fixture).join("releases").join(RELEASE);
    assert!(
        out.contains(&format!(
            "release: refresh: installed {RELEASE} in {}, built in ",
            place.display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: {RELEASE} serves board schema 18, plugin protocol 0.3 and tasks \
             api 1.14, at Symphony pin 1\n"
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: refresh: {} names {RELEASE} (it named no release): a project with no build \
             of its own runs it\n",
            machine(&fixture).join("current").display()
        )),
        "{out}"
    );
    assert!(out.contains("release: refresh-install green ("), "{out}");
    // Built once, in the run's own checkout, at the release's commit.
    let built = builds(&fixture);
    assert_eq!(built.len(), 1, "{built:?}");
    assert!(
        built[0].starts_with(&format!(
            "{} build --release --locked -p symphony-server ",
            fixture.worktree().display()
        )),
        "{built:?}"
    );
    // The folder is whole: the binaries, the client under its hash, and the manifest.
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(place.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["release"], RELEASE);
    assert_eq!(manifest["commit"], commit.as_str());
    assert_eq!(manifest["schema"], 18);
    assert_eq!(manifest["protocol"]["minor"], 3);
    assert_eq!(manifest["tasks_api"]["minor"], 14);
    assert_eq!(manifest["pin"], 1);
    let client = manifest["client"].as_str().unwrap();
    assert_eq!(
        fs::read_to_string(place.join(client).join("assets/app.js")).unwrap(),
        "app()\n"
    );
    let artifacts = manifest["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 6);
    for artifact in artifacts {
        let path = place.join(artifact["path"].as_str().unwrap());
        assert!(path.exists(), "{}", path.display());
        assert!(artifact["size"].as_u64().unwrap() > 0, "{artifact}");
    }
    assert_eq!(artifacts[5]["files"], 2);
    assert_eq!(
        fs::read_link(machine(&fixture).join("current")).unwrap(),
        PathBuf::from("releases").join(RELEASE)
    );
    let entries: Vec<String> = fs::read_dir(machine(&fixture).join("releases"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, [RELEASE], "nothing temporary left");
    // The record keeps the step first among the refresh's, and the status says what it did.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!("  installed {RELEASE} in {}", place.display()))
            && status.contains("last refresh: green"),
        "{status}"
    );
    // The machine's own line: what `current` names, and what holds the server off it, if anything.
    assert!(
        status.contains(&format!("\nmachine: current names {RELEASE}; ")),
        "{status}"
    );
    // A retry finds it installed: nothing is built again, though the build would now fail.
    fs::write(fixture.root.join("build-red"), "").unwrap();
    let (code, out, err) =
        fixture.release_with(NOW + 120.0, &["--refresh"], &[(BUILDER, builder.as_str())]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains(&format!(
            "release: refresh: {RELEASE} is installed already in {}",
            place.display()
        )) && out.contains(&format!("names {RELEASE} (as it did)")),
        "{out}"
    );
    assert_eq!(builds(&fixture).len(), 1);
}

#[test]
fn a_build_that_fails_leaves_no_folder_and_a_red_refresh_which_the_next_install_closes() {
    let (fixture, _) = installing();
    let builder = builder(&fixture);
    fs::write(fixture.root.join("build-red"), "").unwrap();
    let (code, out, err) =
        fixture.release_with(NOW, &["--check", CHECK], &[(BUILDER, builder.as_str())]);
    assert_eq!(code, 1, "{out}{err}");
    // The release itself is tagged and pushed: only its refresh is red.
    assert_eq!(fixture.remote_tags(), [RELEASE]);
    let releases = machine(&fixture).join("releases");
    assert!(
        out.contains(&format!(
            "release: refresh: {RELEASE} is not installed: its build failed (exit 101), and \
             nothing of it is in {}\n",
            releases.display()
        )),
        "{out}"
    );
    assert!(out.contains("release: refresh-install red ("), "{out}");
    assert!(
        out.contains("release: refresh: current is as it was: it names no release\n"),
        "{out}"
    );
    assert!(
        !releases.join(RELEASE).exists() && !machine(&fixture).join("current").exists(),
        "a failed build installs nothing"
    );
    assert!(
        fs::read_dir(&releases).map_or(true, |mut entries| entries.next().is_none()),
        "no folder left"
    );
    assert!(
        out.contains("release: filed tests/release-red-refresh-install"),
        "{out}"
    );
    let red = &fixture.reds()["tests/release-red-refresh-install"];
    assert!(red.red());
    assert_eq!(
        red.title,
        format!("Release red: the install of {RELEASE} on the machine")
    );
    assert!(
        red.body.contains("error: could not compile")
            && red
                .body
                .contains("Then an orchestrator reruns the refresh, `just release --refresh`"),
        "{}",
        red.body
    );
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("last refresh: red") && status.contains("  refresh-install red: exit 101"),
        "{status}"
    );
    // The retry, once the build passes, installs it and closes the red.
    fs::remove_file(fixture.root.join("build-red")).unwrap();
    let (code, out, err) =
        fixture.release_with(NOW + 120.0, &["--refresh"], &[(BUILDER, builder.as_str())]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("release: refresh-install green ("), "{out}");
    assert!(
        out.contains("release: closed tests/release-red-refresh-install"),
        "{out}"
    );
    assert!(releases.join(RELEASE).join("manifest.json").is_file());
    assert_eq!(builds(&fixture).len(), 2);
}
