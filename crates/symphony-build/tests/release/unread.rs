//! A project file that does not load stops every run, as the run's own red naming the file and
//! what is wrong with it: a `[releases]` key mistyped, a value of the wrong type or a file that
//! does not parse never leaves a run going with those settings unset, its checks dropped. The run
//! checks nothing, tags nothing and files `tests/release-red-run`, which the run after the file is
//! put right closes. A land's count (`--due`) does not count over it and hands over to a build of
//! the tip, as its own build may be older than the file: that build counts, or stops red when it
//! cannot load it either, as the count does where no such build is made: at a tip with no
//! launcher, and over a file that is not TOML, which no build reads. The build that loads it renews
//! the main checkout's link, so the next count is not handed over again. `--status` says it and
//! runs nothing, and `--schedule`, `--drop` and `--github` refuse.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::{Fixture, NOW};

const RED: &str = "tests/release-red-run";

/// The fixture, and a check that leaves the file it names behind when it runs.
fn fixture() -> (Fixture, PathBuf, String) {
    let fixture = Fixture::new();
    let ran = fixture.root.join("the-check-ran");
    let check = format!("verify=touch {}", ran.display());
    (fixture, ran, check)
}

/// Land `text` as the project file.
fn land(fixture: &Fixture, text: &str) {
    fixture.land("symphony.toml", Some(text), "the project file changes");
}

/// The project file's path as a run says it.
fn file(fixture: &Fixture) -> String {
    fixture.main.join("symphony.toml").display().to_string()
}

#[test]
fn a_run_over_a_project_file_that_does_not_load_is_red_naming_it_and_checks_nothing() {
    let (fixture, ran, check) = fixture();
    let would_run = fixture.root.join("the-mistyped-check-ran");
    // Each is the plain project, or the file less a key, to the forgiving read.
    let mistyped = Fixture::project_file(&format!(
        "ruleset = [\"touch\", \"{}\"]\n",
        would_run.display()
    ));
    for (text, wrong) in [
        (mistyped.as_str(), "ruleset"),
        (
            &Fixture::project_file("every_landings = \"often\"\n"),
            "every_landings",
        ),
        ("[releases\nrefresh = false\n", "symphony.toml"),
    ] {
        land(&fixture, text);
        let before = fixture.runs().len();
        let (code, out, err) = fixture.release_at(NOW, &["--check", &check]);
        assert_eq!(code, 1, "{wrong}\nstdout:\n{out}\nstderr:\n{err}");
        let said = format!("the project file {} does not load", file(&fixture));
        assert!(err.contains(&said), "{wrong}: {err}");
        assert!(err.contains(wrong), "the error itself, {wrong}: {err}");
        assert!(!out.contains("release: verify"), "no check ran: {out}");
        assert!(!ran.exists() && !would_run.exists(), "{wrong}: a check ran");
        assert!(fixture.remote_tags().is_empty(), "nothing is tagged");
        let runs = fixture.runs();
        assert_eq!(runs.len(), before + 1, "the run is recorded");
        let run = runs.last().unwrap();
        assert_eq!(run["result"], "error");
        assert!(run["error"].as_str().unwrap().contains(&said), "{run}");
        assert_eq!(run["checks"].as_array().unwrap().len(), 0, "{run}");
        assert_eq!(run["commit"], "", "before it had a candidate: {run}");
        // One red, the run's own, which says the file and the error and is no check's.
        let reds = fixture.reds();
        assert_eq!(reds.keys().collect::<Vec<_>>(), [RED]);
        assert!(reds[RED].red(), "{:?}", reds[RED]);
        if before == 0 {
            assert!(out.contains(&format!("release: filed {RED}")), "{out}");
            assert!(reds[RED].body.contains(&said), "{}", reds[RED].body);
            assert!(reds[RED].body.contains(wrong), "{}", reds[RED].body);
        } else {
            assert!(out.contains(&format!("release: updated {RED}")), "{out}");
            let thread = fixture.thread(RED);
            let last = thread.last().unwrap();
            assert!(last.contains(&said) && last.contains(wrong), "{last}");
        }
    }

    // Put right: the next run reads it, checks, releases and closes the red.
    land(&fixture, &Fixture::project_file(""));
    let out = fixture.run(NOW + 3600.0, &["--check", &check], 0);
    assert!(out.contains("release: verify green"), "{out}");
    assert!(ran.exists(), "the check ran");
    assert!(out.contains(&format!("release: closed {RED}")), "{out}");
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
}

#[test]
fn a_lands_count_over_it_stops_red_without_counting_and_the_other_commands_say_or_refuse() {
    let (fixture, ran, check) = fixture();
    // Read forgivingly this is a project with no cadence, whose count would say nothing is due.
    land(
        &fixture,
        &Fixture::project_file("every_landings = 0\nevery_landing = 1\n"),
    );
    let said = format!("the project file {} does not load", file(&fixture));
    let landed = super::git(&fixture.main, &["rev-parse", "HEAD"]);

    let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
    let (code, out, err) = fixture.release_at(NOW, &args);
    assert_eq!(code, 1, "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        err.contains(&said) && err.contains("every_landing"),
        "{err}"
    );
    assert!(!out.contains("landings"), "nothing is counted: {out}");
    assert!(!ran.exists(), "no check ran");
    assert_eq!(fixture.reds().keys().collect::<Vec<_>>(), [RED]);
    assert_eq!(fixture.runs().len(), 1);

    // `--status` runs nothing, so it says where things stand, and this first.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.starts_with(&format!("project file: {said}")),
        "{status}"
    );
    assert!(status.contains("release candidate:"), "the rest: {status}");

    // What would decide or write from the settings refuses, and records no run.
    for args in [
        &["--schedule"][..],
        &["--drop"][..],
        &["--github", "release-2026-11-12"][..],
    ] {
        let (code, out, err) = fixture.release_at(NOW + 120.0, args);
        assert_eq!((code, out.as_str()), (1, ""), "{args:?}: {err}");
        assert!(err.contains(&said), "{args:?}: {err}");
    }
    for args in [&["--refresh"][..], &["--rerun"][..]] {
        let (code, _, err) = fixture.release_at(NOW + 180.0, args);
        assert_eq!(code, 1, "{args:?}: {err}");
        assert!(err.contains(&said), "{args:?}: {err}");
    }
    assert!(fixture.remote_tags().is_empty() && !ran.exists());
}

/// Write `text` as the executable `path`.
fn program(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Land a launcher at the tip that names `build` as the tip's build, and a project file the
/// count's build cannot load.
fn tip_builds(fixture: &Fixture, build: &Path) {
    launcher(fixture, &format!("echo '{}'", build.display()));
    land(
        fixture,
        &Fixture::project_file("a_key_of_a_newer_build = 1\n"),
    );
}

/// Write the main checkout's launcher, which the next landing lands: it says where and how it was
/// asked in the log it names, then runs `then`.
fn launcher(fixture: &Fixture, then: &str) -> PathBuf {
    let asked = fixture.root.join("launcher.log");
    fs::create_dir_all(fixture.main.join("tools")).unwrap();
    program(
        &fixture.main.join("tools/rs-bin.sh"),
        &format!(
            "#!/bin/sh\necho \"$PWD $*\" >> '{}'\n{then}\n",
            asked.display()
        ),
    );
    asked
}

#[test]
fn a_count_whose_build_cannot_load_the_file_hands_over_to_the_tips_and_stops_nothing_itself() {
    let (fixture, ran, check) = fixture();
    // The tip's build, a stand-in for one that knows the key: it says how it was run and ends.
    let newer = fixture.root.join("newer-build");
    let asked = fixture.root.join("asked.log");
    program(
        &newer,
        &format!(
            "#!/bin/sh\necho \"$* driven by $SYMPHONY_RELEASE_DRIVER, stale \
             $SYMPHONY_RELEASE_STALE\" >> '{}'\n",
            asked.display()
        ),
    );
    tip_builds(&fixture, &newer);
    let landed = super::git(&fixture.main, &["rev-parse", "HEAD"]);
    let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
    let (code, out, err) = fixture.release_at(NOW, &args);
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}");
    assert!(
        out.contains("release: this build cannot load the project file"),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: the run goes on from {}, built in its own checkout\n",
            newer.display()
        )),
        "{out}"
    );
    // The count as given, to that build, told the linked one is older than the file, and nothing
    // judged by the older one.
    assert_eq!(
        fs::read_to_string(&asked).unwrap(),
        format!(
            "release {} driven by {}, stale 1\n",
            args.join(" "),
            newer.display()
        )
    );
    assert!(!err.contains("does not load"), "nothing stops here: {err}");
    assert!(
        fixture.runs().is_empty(),
        "no run stopped in the count's build"
    );
    assert!(fixture.reds().is_empty(), "and no red");
    assert!(!ran.exists());
    assert!(!fixture.records().join("running.json").exists());
}

#[test]
fn a_tips_build_that_cannot_load_it_either_stops_red_once_and_hands_over_no_further() {
    let (fixture, ran, check) = fixture();
    // The tip's build is this test's own, to which the key is as unknown: asked once.
    let built = fixture.root.join("built.log");
    let tip = fixture.root.join("tip-build");
    program(
        &tip,
        &format!(
            "#!/bin/sh\necho built >> '{}'\nexec '{}' \"$@\"\n",
            built.display(),
            super::BIN
        ),
    );
    tip_builds(&fixture, &tip);
    let landed = super::git(&fixture.main, &["rev-parse", "HEAD"]);
    let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
    let (code, out, err) = fixture.release_at(NOW, &args);
    assert_eq!(code, 1, "stdout:\n{out}\nstderr:\n{err}");
    assert_eq!(fs::read_to_string(&built).unwrap(), "built\n", "once");
    let said = format!("the project file {} does not load", file(&fixture));
    assert!(
        err.contains(&said) && err.contains("a_key_of_a_newer_build"),
        "{err}"
    );
    assert!(!out.contains("landings"), "nothing is counted: {out}");
    assert_eq!(fixture.runs().len(), 1, "one run, the tip build's");
    assert_eq!(fixture.reds().keys().collect::<Vec<_>>(), [RED]);
    assert!(!ran.exists(), "no check ran");
}

/// The tip's build of a count handed over for its file, which this test's own build is told it is.
const HANDED: [(&str, &str); 2] = [
    ("SYMPHONY_RELEASE_DRIVER", super::BIN),
    ("SYMPHONY_RELEASE_STALE", "1"),
];

#[test]
fn the_tips_build_that_loads_the_file_renews_the_main_checkouts_link_once_and_counts() {
    let (fixture, ran, check) = fixture();
    let asked = launcher(&fixture, "echo linking >&2");
    let landed = fixture.land("landed", Some("one\n"), "a landing");
    let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
    let link = fixture.main.join(".target/bin/symphony-build");

    let (code, out, err) = fixture.release_with(NOW, &args, &HANDED);
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}");
    // In the main checkout, by the launcher's own word for it, and never the release checkout's.
    assert_eq!(
        fs::read_to_string(&asked).unwrap(),
        format!("{} --link symphony-build\n", fixture.main.display())
    );
    let renewed = format!(
        "linking\nrelease: the build {} links was older than the project file: renewed, so the \
         next count builds nothing\n",
        link.display()
    );
    assert!(out.contains(&renewed), "{out}");
    let counted = out.split_once(&renewed).unwrap().1;
    assert!(counted.contains("landings"), "then it counts: {out}");
    assert!(!ran.exists() && fixture.runs().is_empty(), "nothing is due");

    // A build handed a due count, whose linked build loaded the file, renews nothing; neither
    // does one nobody handed anything.
    for told in [&HANDED[..1], &HANDED[1..], &[]] {
        let (code, out, err) = fixture.release_with(NOW, &args, told);
        assert_eq!(code, 0, "{told:?}\nstdout:\n{out}\nstderr:\n{err}");
        assert!(!out.contains("links"), "{told:?}: {out}");
    }
    assert_eq!(fs::read_to_string(&asked).unwrap().lines().count(), 1);
}

#[test]
fn a_link_that_cannot_be_renewed_is_said_and_the_count_goes_on() {
    let (fixture, _, check) = fixture();
    let asked = launcher(&fixture, "echo 'error: main does not build' >&2\nexit 1");
    let landed = fixture.land("landed", Some("one\n"), "a landing");
    let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
    let (code, out, err) = fixture.release_with(NOW, &args, &HANDED);
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}");
    let said = format!(
        "release: the build {} links is older than the project file and was not renewed (the \
         build of its tip in {} failed (exit status: 1)): each count builds the tip until `just \
         release --status` there renews it\n",
        fixture.main.join(".target/bin/symphony-build").display(),
        fixture.main.display()
    );
    assert!(
        out.contains(&format!("error: main does not build\n{said}")),
        "{out}"
    );
    assert!(
        out.split_once(&said).unwrap().1.contains("landings"),
        "{out}"
    );

    // A main checkout with no launcher has no link to renew: nothing is said of it.
    fs::remove_file(fixture.main.join("tools/rs-bin.sh")).unwrap();
    let (code, out, err) = fixture.release_with(NOW, &args, &HANDED);
    assert_eq!(code, 0, "stdout:\n{out}\nstderr:\n{err}");
    assert!(!out.contains("links") && out.contains("landings"), "{out}");
    assert_eq!(fs::read_to_string(&asked).unwrap().lines().count(), 1);
}

#[test]
fn a_count_over_a_file_no_build_reads_builds_no_tip_and_stops_red_itself() {
    let (fixture, ran, check) = fixture();
    let asked = launcher(&fixture, "echo /no/build/is/asked/for");
    // Not TOML, then not text: neither is a newer build's to read.
    for (bytes, wrong) in [
        (&b"[releases\nrefresh = false\n"[..], "symphony.toml"),
        (&b"[releases]\nrefresh = \xff\n"[..], "UTF-8"),
    ] {
        fs::write(fixture.main.join("symphony.toml"), bytes).unwrap();
        fixture.commit("a project file no build reads");
        super::git(&fixture.main, &["push", "-q", "origin", "main"]);
        let landed = super::git(&fixture.main, &["rev-parse", "HEAD"]);
        let before = fixture.runs().len();
        let args = ["--check", check.as_str(), "--due", "tools/last", &landed];
        let (code, out, err) = fixture.release_at(NOW, &args);
        assert_eq!(code, 1, "{wrong}\nstdout:\n{out}\nstderr:\n{err}");
        let said = format!("the project file {} does not load", file(&fixture));
        assert!(err.contains(&said) && err.contains(wrong), "{wrong}: {err}");
        assert!(!out.contains("a build of the tip"), "{wrong}: {out}");
        assert!(!asked.exists(), "{wrong}: the tip was built for nothing");
        assert!(!fixture.worktree().exists(), "{wrong}: nor checked out");
        assert_eq!(fixture.runs().len(), before + 1, "{wrong}: the red run");
        assert_eq!(fixture.reds().keys().collect::<Vec<_>>(), [RED]);
        assert!(!ran.exists(), "no check ran");
    }
}
