//! A night on a main whose generated files the regenerator a land starts makes after it: it waits
//! while one makes them, releases nothing of a main whose committed files are behind its sources,
//! and files nothing for it, the regenerator's own red being the task; `--status` says where the
//! regenerator stands.

use std::fs;
use std::time::{Duration, Instant};

use symphony_board::land::regenerate::State;
use symphony_board::sys::{Child, Command, Stdio};

use super::{CHECK, Fixture, NOW, git};

/// The fixture's project file with a generator of `gen/`, landed on the remote's main: the commit.
fn generating(fixture: &Fixture) -> String {
    fixture.land(
        "symphony.toml",
        Some(
            "[project]\nid = \"fixture\"\n\n[agents]\nworktrees = \"../wt\"\n\n[land]\n\
             generators = [{ name = \"the client\", outputs = \"gen/\", command = [\"true\"] }]\n",
        ),
        "a generator",
    )
}

/// The regenerator's state, as one with `pid` wrote it, its round on `tip`.
fn regenerator(fixture: &Fixture, state: &str, tip: &str, pid: i64) {
    let folder = fixture.main.join(".git/sf-tasks/land");
    fs::create_dir_all(&folder).unwrap();
    let state = State {
        state: state.into(),
        tip: tip.into(),
        landed: None,
        at: NOW,
        pid,
        start: symphony_board::land::started(pid),
        line: "the generator the client failed".into(),
    };
    fs::write(
        folder.join("regenerate.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();
}

/// A process that lives until it is ended: a regenerator, as far as its state says.
fn living() -> Child {
    let mut sleep = Command::new("sleep");
    sleep.arg("60").stdin(Stdio::null());
    sleep.spawn().unwrap()
}

#[test]
fn a_main_whose_generated_files_are_behind_it_is_released_nothing_and_filed_nothing() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    let ended = {
        let mut done = Command::new("true");
        let mut child = done.spawn().unwrap();
        child.wait().unwrap();
        i64::from(child.id())
    };
    regenerator(&fixture, "red", &tip, ended);
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(
        err.contains(&format!(
            "symphony nightly: the client: red on main at {} since ",
            &tip[..7]
        )) && err.contains(
            ": the generator the client failed (tests/main-red-regenerate): no release of a main \
             whose generated files are behind its sources"
        ),
        "{err}"
    );
    assert!(!out.contains("verify"), "nothing checked: {out}");
    assert!(fixture.remote_tags().is_empty());
    assert!(fixture.reds().is_empty(), "nothing filed");
    assert!(fixture.runs().is_empty(), "a refusal is not recorded");
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "regenerator: the client: red on main at {}",
            &tip[..7]
        )),
        "{status}"
    );
}

#[test]
fn a_night_waits_while_the_regenerator_makes_mains_files_then_checks_and_releases() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    let mut making = living();
    regenerator(&fixture, "building", &tip, i64::from(making.id()));
    let said = fixture.root.join("night.out");
    let mut night = fixture
        .command(NOW, &["--check", CHECK], &[])
        .stdin(Stdio::null())
        .stdout(fs::File::create(&said).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let waiting = format!(
        "nightly: waiting for the regenerator: the client: making them of main at {} since ",
        &tip[..7]
    );
    let until = Instant::now() + Duration::from_secs(60);
    while !fs::read_to_string(&said)
        .unwrap_or_default()
        .contains(&waiting)
    {
        assert!(Instant::now() < until, "it never waited");
        assert!(
            night.try_wait().unwrap().is_none(),
            "it ended before it waited"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // It waits on while the regenerator makes them, then checks once they are current.
    std::thread::sleep(Duration::from_secs(3));
    assert!(night.try_wait().unwrap().is_none(), "it waits");
    regenerator(&fixture, "current", &tip, 1);
    let until = Instant::now() + Duration::from_secs(120);
    let code = loop {
        if let Some(status) = night.try_wait().unwrap() {
            break status.code();
        }
        assert!(Instant::now() < until, "the night never ended");
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = making.kill();
    let _ = making.wait();
    let out = fs::read_to_string(&said).unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains(&format!(
            "nightly: the client: current on main at {}",
            &tip[..7]
        )),
        "{out}"
    );
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
}

#[test]
fn a_night_on_a_main_with_no_generators_or_another_tip_waits_for_nothing() {
    let fixture = Fixture::new();
    let status = fixture.run(NOW, &["--status"], 0);
    assert!(!status.contains("regenerator"), "{status}");
    // Behind, but on a commit of this checkout's that the remote's main is not: the night checks
    // the remote's, whose own check says.
    let tip = generating(&fixture);
    regenerator(&fixture, "red", &tip, 1);
    fixture.write("local.txt", "never pushed");
    fixture.commit("a commit of this checkout alone");
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    assert_eq!(git(&fixture.main, &["status", "--porcelain"]), "");
}
