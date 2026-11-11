//! Conciliation with a project the machine serves: a consumer of the fixture's Symphony, built and
//! tested by Cargo with its Symphony moved to the candidate, in a scratch copy of its own. A
//! candidate that breaks it is a red night and a red task; its checkout never changes.

use std::fs;
use std::path::{Path, PathBuf};

use super::{CHECK, Fixture, NOW, git};
use symphony::machine::{Entry, Registry};

/// The toolchain this workspace pins, which the fixtures pin too: a test never reaches for another.
fn toolchain() -> String {
    let pin = include_str!("../../../../rust-toolchain.toml");
    let channel = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel"))
        .expect("the workspace pins a channel");
    format!("[toolchain]\nchannel{channel}\n")
}

/// The fixture's Symphony as a Cargo workspace whose one package answers `answer`.
fn symphony_crate(fixture: &Fixture, answer: u32) {
    fixture.write("rust-toolchain.toml", &toolchain());
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/core\"]\nresolver = \"2\"\n",
    );
    fixture.write(
        "crates/core/Cargo.toml",
        "[package]\nname = \"fixture-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    fixture.write(
        "crates/core/src/lib.rs",
        &format!("pub fn answer() -> u32 {{\n    {answer}\n}}\n"),
    );
}

/// A project that builds on the fixture's Symphony from its remote, registered in the fixture's
/// machine folder: its checkout.
fn consumer(fixture: &Fixture) -> PathBuf {
    let root = fixture.root.join("consumer");
    let write = |file: &str, text: &str| {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write("rust-toolchain.toml", &toolchain());
    write(
        "symphony.toml",
        "[project]\nid = \"consumer\"\n\n[plugins]\ncrates = [\"consumer-plugin\"]\n",
    );
    write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"host\"]\nresolver = \"2\"\n",
    );
    write(
        "host/Cargo.toml",
        &format!(
            "[package]\nname = \"consumer-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nfixture-core = {{ git = \"file://{}\", branch = \"main\" }}\n",
            fixture.origin.display()
        ),
    );
    write(
        "host/src/lib.rs",
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn the_symphony_answers() {\n        \
         assert_eq!(fixture_core::answer(), 42);\n    }\n}\n",
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "the consumer"]);
    let machine = fixture.root.join("machine");
    let registry = Registry {
        projects: vec![Entry {
            id: "consumer".into(),
            name: "Consumer".into(),
            root: root.clone(),
            manifest: Some(root.join("symphony.toml")),
            plugins: vec!["consumer-plugin".into()],
            pin: 1,
            schema: 16,
        }],
    };
    registry.write(&machine).unwrap();
    root
}

/// Everything under the checkout at `root` but its `.git`, as it stands.
fn checkout(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    Fixture::files(root)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(root.join(".git")))
        .collect()
}

#[test]
fn a_project_the_machine_serves_is_built_and_tested_against_the_candidate() {
    let fixture = Fixture::new();
    symphony_crate(&fixture, 42);
    fixture.commit("the Symphony's crate");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let consumer = consumer(&fixture);
    let consumer_head = git(&consumer, &["rev-parse", "HEAD"]);
    let before = checkout(&consumer);
    let home = fixture.root.join("cargo-home");
    let cargo_home = home.to_string_lossy().into_owned();
    let extra = [("CARGO_HOME", cargo_home.as_str())];

    let (code, out, err) = fixture.nightly_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("nightly: conciliate-consumer green (exit 0"),
        "{out}"
    );
    let scratch = fixture.root.join("wt/nightly-conciliate/consumer");
    assert_eq!(git(&scratch, &["rev-parse", "HEAD"]), consumer_head);
    let run = fixture.runs().last().unwrap().clone();
    let conciliated = &run["checks"][1];
    assert_eq!(conciliated["name"], "conciliate-consumer");
    let command = conciliated["command"].as_str().unwrap();
    assert!(
        command.starts_with("cargo test --no-fail-fast --config 'patch."),
        "{command}"
    );
    assert!(
        command.contains(&format!(
            "\"fixture-core\".path=\"{}\"",
            fixture.worktree().join("crates/core").display()
        )),
        "the candidate's checkout is patched in: {command}"
    );
    assert!(command.contains(" -p consumer-host (in "), "{command}");
    let log = fs::read_to_string(conciliated["log"].as_str().unwrap()).unwrap();
    assert!(
        log.contains("test tests::the_symphony_answers ... ok"),
        "{log}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert!(
        fixture
            .message("release-2026-11-12")
            .contains("Green: verify, conciliate-consumer.\n"),
        "{}",
        fixture.message("release-2026-11-12")
    );

    // A candidate that breaks the consumer: red, and no release.
    symphony_crate(&fixture, 41);
    fixture.commit("an answer the consumer does not expect");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let (code, out, err) = fixture.nightly_with(NOW + 3600.0, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(out.contains("nightly: verify green"), "{out}");
    assert!(
        out.contains("nightly: conciliate-consumer red (exit 101"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        ["tests/nightly-red-conciliate-consumer"]
    );
    assert!(
        reds["tests/nightly-red-conciliate-consumer"]
            .body
            .contains("the_symphony_answers"),
        "the log's end names the failing test: {}",
        reds["tests/nightly-red-conciliate-consumer"].body
    );
    assert_eq!(
        checkout(&consumer),
        before,
        "the project's checkout is as it was"
    );
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), consumer_head);
    assert_eq!(git(&consumer, &["status", "--porcelain"]), "");
}

#[test]
fn a_registered_project_that_cannot_be_conciliated_is_named_and_the_rest_goes_on() {
    let fixture = Fixture::new();
    let gone = fixture.root.join("gone");
    Registry {
        projects: vec![
            Entry {
                id: "gone".into(),
                name: "Gone".into(),
                root: gone.clone(),
                manifest: None,
                plugins: vec!["p".into()],
                pin: 1,
                schema: 16,
            },
            Entry {
                id: "itself".into(),
                name: "Itself".into(),
                root: fixture.main.clone(),
                manifest: None,
                plugins: vec!["p".into()],
                pin: 1,
                schema: 16,
            },
        ],
    }
    .write(&fixture.root.join("machine"))
    .unwrap();
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains(&format!(
            "nightly: left out: gone: its checkout {} is gone",
            gone.display()
        )),
        "{out}"
    );
    assert!(
        !out.contains("conciliate-itself"),
        "the repository released is not its own consumer: {out}"
    );
    let run = fixture.runs().last().unwrap().clone();
    assert_eq!(run["skipped"].as_array().unwrap().len(), 1, "{run}");
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}
