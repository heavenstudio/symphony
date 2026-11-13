//! Conciliation with a project the machine serves: a consumer of the fixture's Symphony, built and
//! tested by Cargo with its Symphony moved to the candidate, in a scratch copy of its own. A
//! candidate that breaks it is a red night and a red task; its checkout never changes.

use std::fs;
use std::path::{Path, PathBuf};

use super::stops::{Process, ends_within, written_pid};
use super::{CHECK, Fixture, NOW, git};
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

/// A project that builds on the fixture's Symphony from its remote, registered in the fixture's
/// machine folder: its checkout.
pub(super) fn consumer(fixture: &Fixture) -> PathBuf {
    let manifest = format!(
        "[package]\nname = \"consumer-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nfixture-core = {{ git = \"file://{}\", branch = \"main\" }}\n",
        fixture.origin.display()
    );
    let toolchain = toolchain();
    // Its tests see the machine folder they run with: a copy of the registry, never the machine's.
    let host = format!(
        "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn the_symphony_answers() {{\n        \
         assert_eq!(fixture_core::answer(), 42);\n    }}\n\n    #[test]\n    \
         fn the_machine_is_a_copy() {{\n        \
         let dir = std::env::var(\"SYMPHONY_MACHINE_DIR\").unwrap();\n        \
         assert_ne!(dir, {real:?});\n        \
         let projects = std::fs::read_to_string(format!(\"{{dir}}/projects.json\")).unwrap();\n        \
         assert!(projects.contains(\"\\\"consumer\\\"\"), \"{{projects}}\");\n    }}\n}}\n",
        real = fixture.root.join("machine").to_string_lossy()
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

/// Everything under the checkout at `root` but its `.git`, as it stands.
pub(super) fn checkout(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
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
    for test in ["the_symphony_answers", "the_machine_is_a_copy"] {
        assert!(log.contains(&format!("test tests::{test} ... ok")), "{log}");
    }
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
fn a_copy_its_conciliation_left_changed_or_in_use_is_never_released() {
    let fixture = Fixture::new();
    symphony_crate(&fixture, 42);
    fixture.commit("the Symphony's crate");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let manifest = format!(
        "[package]\nname = \"consumer-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nfixture-core = {{ git = \"file://{}\", branch = \"main\" }}\n",
        fixture.origin.display()
    );
    // Its test leaves a file git does not ignore in its copy, and a process of a group of its own
    // working there, which outlives the check's group.
    let host = "#[test]\nfn leaves_a_file_and_a_process() {\n    \
                use std::os::unix::process::CommandExt;\n    \
                use std::process::{Command, Stdio};\n    \
                assert_eq!(fixture_core::answer(), 42);\n    \
                std::fs::write(\"left.txt\", \"left\\n\").unwrap();\n    \
                let child = Command::new(\"sleep\").arg(\"120\").process_group(0)\n        \
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())\n        \
                .spawn().unwrap();\n    \
                std::fs::write(std::env::var(\"SURVIVOR_PID\").unwrap(), \
                format!(\"{}\\n\", child.id())).unwrap();\n}\n";
    let toolchain = toolchain();
    let root = project(
        &fixture,
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
            ("host/src/lib.rs", host),
        ],
    );
    register(&fixture, &[("consumer", &root)]);
    let home = fixture.root.join("cargo-home");
    let cargo_home = home.to_string_lossy().into_owned();
    let pid_file = fixture.root.join("survivor.pid");
    let pid_path = pid_file.to_string_lossy().into_owned();
    let extra = [
        ("CARGO_HOME", cargo_home.as_str()),
        ("SURVIVOR_PID", pid_path.as_str()),
    ];

    let (code, out, err) = fixture.nightly_with(NOW, &["--check", CHECK], &extra);
    let survivor = written_pid(&pid_file);
    let guard = Process(libc::pid_t::try_from(survivor).unwrap());
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("nightly: conciliate-consumer green (exit 0"),
        "{out}"
    );
    let copy = fixture.root.join("wt/nightly-conciliate/consumer");
    let at = git(&copy, &["rev-parse", "HEAD"]);
    // Its build folder and the lock its patched build wrote are passed over; what its test left
    // is not.
    assert_eq!(
        git(&copy, &["status", "--porcelain"]),
        "?? .target/\n?? Cargo.lock\n?? host/left.txt"
    );
    assert!(
        err.contains(&format!(
            "after `conciliate-consumer` ended green (exit 0), {copy} is in use by process \
             {survivor} (sleep), working in host, where cargo runs the tests of the package \
             consumer-host (a test of it may have left it); and {copy} no longer holds {short}: ?? \
             host/left.txt: what was checked may have changed, so nothing checked is released; \
             end it, or move it out of that folder (a shell, a terminal tab, an editor or a \
             Finder window there counts), and the next run goes",
            copy = copy.display(),
            short = &at[..12]
        )),
        "{err}"
    );
    assert!(fixture.remote_tags().is_empty(), "nothing is released");
    let run = fixture.runs().pop().unwrap();
    let checked: Vec<&str> = (run["checks"].as_array().unwrap().iter())
        .map(|ran| ran["name"].as_str().unwrap())
        .collect();
    assert_eq!(checked, ["verify"], "the conciliation counts for nothing");
    // SAFETY: kill signals the survivor this test's consumer started, which the test saw running.
    unsafe {
        libc::kill(libc::pid_t::try_from(survivor).unwrap(), libc::SIGKILL);
    }
    assert!(ends_within(survivor, 20));
    guard.ended();
}

#[test]
fn a_registered_project_that_cannot_be_conciliated_is_named_and_the_rest_goes_on() {
    let fixture = Fixture::new();
    let gone = fixture.root.join("gone");
    register(&fixture, &[("gone", &gone), ("itself", &fixture.main)]);
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

#[test]
fn a_project_that_takes_nothing_from_the_symphony_is_passed_over_and_one_unread_is_red() {
    let fixture = Fixture::new();
    symphony_crate(&fixture, 42);
    fixture.commit("the Symphony's crate");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let toolchain = toolchain();
    let alone = project(
        &fixture,
        "alone",
        &[
            ("rust-toolchain.toml", toolchain.as_str()),
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n",
            ),
            (
                "app/Cargo.toml",
                "[package]\nname = \"alone-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("app/src/lib.rs", ""),
        ],
    );
    let unbuilt = project(
        &fixture,
        "unbuilt",
        &[("README.md", "no Cargo workspace\n")],
    );
    register(&fixture, &[("alone", &alone), ("unbuilt", &unbuilt)]);
    let home = fixture.root.join("cargo-home");
    let cargo_home = home.to_string_lossy().into_owned();
    let extra = [("CARGO_HOME", cargo_home.as_str())];

    let (code, out, err) = fixture.nightly_with(NOW, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(
            "nightly: left out: alone: none of its packages takes one of the Symphony's from git"
        ),
        "{out}"
    );
    assert!(
        out.contains("nightly: conciliate-unbuilt red (did not run: cargo metadata failed in "),
        "a project it cannot read is red, never passed over: {out}"
    );
    assert!(fixture.remote_tags().is_empty());
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/nightly-red-conciliate-unbuilt"]
    );

    // A registry that does not read is red: nothing was conciliated, so nothing is released.
    fs::write(fixture.root.join("machine/projects.json"), "not json\n").unwrap();
    let (code, out, err) = fixture.nightly_with(NOW + 60.0, &["--check", CHECK], &extra);
    assert_eq!(code, 1, "{out}{err}");
    assert!(out.contains("nightly: verify green"), "{out}");
    assert!(
        out.contains(
            "nightly: conciliate red (did not run: the machine's registry cannot be read, so \
             nothing was conciliated: "
        ),
        "{out}"
    );
    assert!(
        out.contains("nightly: filed tests/nightly-red-conciliate"),
        "{out}"
    );
    assert!(fixture.remote_tags().is_empty());

    // Read again, the night is green, closes that red, and its tag says what it left out.
    register(&fixture, &[("alone", &alone)]);
    let (code, out, err) = fixture.nightly_with(NOW + 120.0, &["--check", CHECK], &extra);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("nightly: closed tests/nightly-red-conciliate"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    let message = fixture.message("release-2026-11-12");
    assert!(
        message.contains(
            "Left out of conciliation:\n- alone: none of its packages takes one of the \
             Symphony's from git\n"
        ),
        "{message}"
    );
}

#[test]
fn a_copy_the_nightly_did_not_make_is_left_and_one_whose_project_left_the_registry_goes() {
    let fixture = Fixture::new();
    let kept = project(&fixture, "kept", &[("README.md", "a project\n")]);
    let other = project(&fixture, "other", &[("README.md", "another\n")]);
    // Someone's repository where the copy of `other` would go.
    let copies = fixture.root.join("wt/nightly-conciliate");
    let foreign = project(
        &fixture,
        "wt/nightly-conciliate/other",
        &[("mine.txt", "someone's\n")],
    );
    let files = Fixture::files(&foreign);
    register(&fixture, &[("kept", &kept), ("other", &other)]);
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains(&format!(
            "nightly: conciliate-other red (did not run: {} is not the nightly's copy of {} (no \
             marker of the nightly's); it is left as it is: move it away",
            foreign.display(),
            other.display()
        )),
        "{out}"
    );
    assert_eq!(Fixture::files(&foreign), files, "not a byte of it changed");
    assert_eq!(
        fs::read_to_string(copies.join("kept/.git/symphony-nightly-copy")).unwrap(),
        format!("{}\n", kept.display()),
        "the copy it made is marked as its own"
    );

    // Neither registered now: its own copy goes, the folder it did not make stays.
    register(&fixture, &[]);
    let out = fixture.night(NOW + 60.0, 0);
    assert!(
        out.contains(&format!(
            "nightly: removed the copy {}: its project is no longer registered",
            copies.join("kept").display()
        )),
        "{out}"
    );
    assert!(!copies.join("kept").exists());
    assert_eq!(Fixture::files(&foreign), files);
}
