//! The projects the machine serves, as a release run meets them: a release is judged by the checks
//! this repository owns, so a project that builds on the fixture's Symphony is never copied, built
//! or tested before the tag, and a Symphony change that breaks it is released all the same; the
//! project's own gate finds that when its own landing moves its pin. The projects themselves are
//! scratch repositories the refresh tests share ([`super::refresh`]).

use std::fs;
use std::path::{Path, PathBuf};

use super::{Fixture, NOW, git};
use symphony::machine::{Entry, Registry};

/// The toolchain this workspace pins, which the fixtures pin too: a test never reaches for another.
pub(super) fn toolchain() -> String {
    let pin = include_str!("../../../../rust-toolchain.toml");
    let channel = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel"))
        .expect("the workspace pins a channel");
    format!("[toolchain]\nchannel{channel}\n")
}

/// The fixture's Symphony as a Cargo workspace whose one package answers `answer`.
pub(super) fn symphony_crate(fixture: &Fixture, answer: u32) {
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

/// A git checkout at `ID` in the fixture's folder holding `files`, committed: its folder.
pub(super) fn project(fixture: &Fixture, id: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = fixture.root.join(id);
    for (file, text) in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", id]);
    root
}

/// The machine folder's registry: `projects`, each naming a plugin.
pub(super) fn register(fixture: &Fixture, projects: &[(&str, &Path)]) {
    let entry = |(id, root): &(&str, &Path)| Entry {
        id: (*id).into(),
        name: (*id).into(),
        root: root.to_path_buf(),
        manifest: None,
        plugins: vec![format!("{id}-plugin")],
        pin: 1,
        schema: 16,
    };
    Registry {
        projects: projects.iter().map(entry).collect(),
    }
    .write(&fixture.root.join("machine"))
    .unwrap();
}

/// The file a [`consumer`]'s test writes in the fixture's folder when it runs.
pub(super) fn tested(fixture: &Fixture) -> PathBuf {
    fixture.root.join("consumer-tested")
}

/// A project that builds on the fixture's Symphony from its remote, registered in the fixture's
/// machine folder, whose one test expects the answer 42 and, run, leaves [`tested`]: its checkout.
pub(super) fn consumer(fixture: &Fixture) -> PathBuf {
    let manifest = format!(
        "[package]\nname = \"consumer-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nfixture-core = {{ git = \"file://{}\", branch = \"main\" }}\n",
        fixture.origin.display()
    );
    let toolchain = toolchain();
    let host = format!(
        "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn the_symphony_answers() {{\n        \
         std::fs::write({mark:?}, \"tested\\n\").unwrap();\n        \
         assert_eq!(fixture_core::answer(), 42);\n    }}\n}}\n",
        mark = tested(fixture).to_string_lossy()
    );
    let root = project(
        fixture,
        "consumer",
        &[
            ("rust-toolchain.toml", toolchain.as_str()),
            (
                "symphony.toml",
                "[project]\nid = \"consumer\"\n\n[plugins]\ncrates = [\"consumer-plugin\"]\n",
            ),
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"host\"]\nresolver = \"2\"\n",
            ),
            ("host/Cargo.toml", manifest.as_str()),
            ("host/src/lib.rs", host.as_str()),
        ],
    );
    register(fixture, &[("consumer", &root)]);
    root
}

/// Everything under the checkout at `root` but its `.git`, as it stands. The walk never enters
/// `.git`: a project's board keeps its PostgreSQL data there, whose server makes and removes files
/// as it runs, so a file listed there may be gone by the time it is read.
pub(super) fn checkout(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if path.is_dir() {
                found.extend(Fixture::files(&path));
            } else {
                found.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// The names of the checks a recorded run `run` holds, in order.
pub(super) fn checked(run: &serde_json::Value) -> Vec<String> {
    (run["checks"].as_array().unwrap().iter())
        .map(|ran| ran["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_change_that_breaks_a_project_the_machine_serves_is_released_with_nothing_of_it_run() {
    let fixture = Fixture::new();
    symphony_crate(&fixture, 41);
    fixture.commit("an answer the consumer does not expect");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let consumer = consumer(&fixture);
    let head = git(&consumer, &["rev-parse", "HEAD"]);
    let before = checkout(&consumer);

    let out = fixture.night(NOW, 0);
    assert!(
        !out.contains("conciliat") && !out.contains("left out"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    let run = fixture.runs().pop().unwrap();
    assert_eq!(checked(&run), ["verify"], "{run}");
    assert!(run.get("skipped").is_none(), "{run}");
    let message = fixture.message("release-2026-11-12");
    assert!(
        message.contains("\nGreen: verify.\n") && !message.contains("onciliat"),
        "{message}"
    );
    assert!(
        !fixture.root.join("wt/release-conciliate").exists(),
        "no copy of it is made"
    );
    assert!(!tested(&fixture).exists(), "none of its tests ran");
    assert_eq!(checkout(&consumer), before, "its checkout is as it was");
    assert_eq!(git(&consumer, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&consumer, &["status", "--porcelain"]), "");
}

#[test]
fn a_run_an_older_build_recorded_with_the_projects_it_conciliated_still_reads_in_the_status() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    let mut earlier = fixture.runs().pop().unwrap();
    let id = format!("{}-earlier", earlier["id"].as_str().unwrap());
    earlier["id"] = id.clone().into();
    earlier["started_at"] = (NOW + 60.0).into();
    earlier["result"] = "red".into();
    earlier["tag"] = serde_json::Value::Null;
    earlier["skipped"] = serde_json::json!(["gone: its checkout /p is gone"]);
    let mut conciliated = earlier["checks"][0].clone();
    conciliated["name"] = "conciliate-consumer".into();
    conciliated["command"] =
        "cargo test --no-fail-fast -p consumer-host (in /copies/consumer)".into();
    conciliated["exit_code"] = 101.into();
    earlier["checks"].as_array_mut().unwrap().push(conciliated);
    fs::write(
        fixture.records().join(format!("runs/{id}.json")),
        serde_json::to_string_pretty(&earlier).unwrap(),
    )
    .unwrap();

    let status = fixture.run(NOW + 120.0, &["--status"], 0);
    let last = (status.lines())
        .find(|line| line.starts_with("last run: "))
        .expect(&status);
    assert!(last.starts_with("last run: red, "), "{status}");
    assert!(
        last.ends_with("; red: conciliate-consumer (exit 101)"),
        "{status}"
    );
}
