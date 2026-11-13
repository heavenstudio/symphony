//! A run on a main whose generated files the regenerator a land starts makes after it: it waits
//! while one makes them, gives one a land has just started its time to take main, follows main
//! when what it made lands, releases nothing of a main whose committed files are behind its
//! sources, and files nothing for it, the regenerator's own red being the task; `--status` says
//! where the regenerator stands.

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
    regenerator_landing(fixture, state, tip, None, pid);
}

/// The same, the round having landed `landed` on `tip`.
fn regenerator_landing(fixture: &Fixture, state: &str, tip: &str, landed: Option<&str>, pid: i64) {
    let folder = fixture.main.join(".git/symphony/tasks/land");
    fs::create_dir_all(&folder).unwrap();
    let state = State {
        state: state.into(),
        tip: tip.into(),
        landed: landed.map(str::to_string),
        current: None,
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

/// A process that has ended: its pid.
fn ended() -> i64 {
    let mut done = Command::new("true");
    let mut child = done.spawn().unwrap();
    child.wait().unwrap();
    i64::from(child.id())
}

/// A run with the stand-in check, `extra` in its environment, started in the background, its
/// stdout to `said`.
fn started(fixture: &Fixture, said: &std::path::Path, extra: &[(&str, &str)]) -> Child {
    fixture
        .command(NOW, &["--check", CHECK], extra)
        .stdin(Stdio::null())
        .stdout(fs::File::create(said).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Wait until the run started with its stdout to `said` has said `line`, still running.
fn until_said(night: &mut Child, said: &std::path::Path, line: &str) {
    let until = Instant::now() + Duration::from_secs(60);
    while !fs::read_to_string(said).unwrap_or_default().contains(line) {
        assert!(
            Instant::now() < until,
            "it never said {line:?}: {}",
            fs::read_to_string(said).unwrap_or_default()
        );
        assert!(
            night.try_wait().unwrap().is_none(),
            "it ended before it said {line:?}: {}",
            fs::read_to_string(said).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The exit code of `night` once it ends, within two minutes.
fn ended_with(night: &mut Child) -> Option<i32> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = night.try_wait().unwrap() {
            break status.code();
        }
        assert!(Instant::now() < until, "the run never ended");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_main_whose_generated_files_are_behind_it_is_released_nothing_and_filed_nothing() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    // What the regenerator wrote of the candidate refuses it at once.
    regenerator(&fixture, "red", &tip, ended());
    let (code, out, err) = fixture.release_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(
        err.contains(&format!(
            "symphony release: the client: red on main at {} since ",
            &tip[..7]
        )) && err.contains(
            ": the generator the client failed (tests/main-red-regenerate); main holds none found \
             current here yet: no release of a main whose generated files are behind its sources"
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
    let mut night = started(&fixture, &said, &[]);
    until_said(
        &mut night,
        &said,
        &format!(
            "release: waiting for the regenerator: the client: making them of main at {} since ",
            &tip[..7]
        ),
    );
    // It waits on while the regenerator makes them, then checks once they are current.
    std::thread::sleep(Duration::from_secs(3));
    assert!(night.try_wait().unwrap().is_none(), "it waits");
    regenerator(&fixture, "current", &tip, 1);
    let code = ended_with(&mut night);
    let _ = making.kill();
    let _ = making.wait();
    let out = fs::read_to_string(&said).unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains(&format!(
            "release: the client: current on main at {}",
            &tip[..7]
        )),
        "{out}"
    );
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
}

#[test]
fn a_night_on_a_main_with_no_generators_or_another_tip_waits_for_nothing() {
    let fixture = Fixture::new();
    let status = fixture.run(NOW, &["--status"], 0);
    assert!(!status.contains("regenerator"), "{status}");
    // Behind, but on a commit of this checkout's that the remote's main is not: the run checks
    // the remote's, whose own check says.
    let tip = generating(&fixture);
    regenerator(&fixture, "red", &tip, 1);
    fixture.write("local.txt", "never pushed");
    fixture.commit("a commit of this checkout alone");
    let out = fixture.night(NOW, 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12"),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    assert_eq!(git(&fixture.main, &["status", "--porcelain"]), "");
}

#[test]
fn a_night_gives_the_regenerator_a_land_has_just_started_its_time_to_take_main() {
    let fixture = Fixture::new();
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let tip = generating(&fixture);
    // What the regenerator last wrote is of an earlier tip, by one that ended: the one the land
    // has just started has written nothing yet.
    regenerator(&fixture, "red", &first, ended());
    let said = fixture.root.join("night.out");
    let mut night = started(&fixture, &said, &[]);
    until_said(
        &mut night,
        &said,
        &format!(
            "release: waiting for the regenerator a land starts to take it: the client: red on \
             main at {} since ",
            &first[..7]
        ),
    );
    std::thread::sleep(Duration::from_secs(3));
    assert!(night.try_wait().unwrap().is_none(), "it waits");
    // It takes main, makes them, and they are current.
    let mut making = living();
    regenerator(&fixture, "building", &tip, i64::from(making.id()));
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        night.try_wait().unwrap().is_none(),
        "it waits while they are made"
    );
    regenerator(&fixture, "current", &tip, 1);
    let code = ended_with(&mut night);
    let _ = making.kill();
    let _ = making.wait();
    let out = fs::read_to_string(&said).unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains(&format!(
            "release: the client: current on main at {}",
            &tip[..7]
        )),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
}

#[test]
fn a_night_no_regenerator_takes_main_for_is_refused_once_its_time_is_up() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--check", CHECK],
        &[("SYMPHONY_RELEASE_REGENERATOR_SECONDS", "1")],
    );
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(
        out.contains(&format!(
            "release: waiting for the regenerator a land starts to take it: the client: never \
             regenerated here; the next land starts the regenerator, which makes them of main at \
             {}",
            &tip[..7]
        )),
        "{out}"
    );
    assert!(
        err.contains(&format!(
            "symphony release: the client: never regenerated here; the next land starts the \
             regenerator, which makes them of main at {}: no release of a main whose generated \
             files are behind its sources",
            &tip[..7]
        )),
        "{err}"
    );
    assert!(!out.contains("verify"), "nothing checked: {out}");
    assert!(fixture.remote_tags().is_empty());
    assert!(fixture.reds().is_empty(), "nothing filed");
    assert!(fixture.runs().is_empty(), "a refusal is not recorded");
}

#[test]
fn a_night_checks_what_the_regenerator_lands_on_main_while_it_waits() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    let mut making = living();
    let pid = i64::from(making.id());
    regenerator(&fixture, "building", &tip, pid);
    let said = fixture.root.join("night.out");
    let mut night = started(&fixture, &said, &[]);
    until_said(&mut night, &said, "release: waiting for the regenerator: ");
    // The regenerator lands what it made of the tip, pushes it, then says so.
    let landed = fixture.land(
        "gen/index",
        Some("made\n"),
        "Regenerate the client for main",
    );
    regenerator_landing(&fixture, "current", &tip, Some(&landed), pid);
    let code = ended_with(&mut night);
    let _ = making.kill();
    let _ = making.wait();
    let out = fs::read_to_string(&said).unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains(&format!(
            "release: origin/main moved on to {} while the run waited for the regenerator: \
             that is what it checks",
            &landed[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: the client: current on main at {}",
            &landed[..7]
        )),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), landed);
    let run = fixture.runs().pop().unwrap();
    assert_eq!(run["commit"], landed.as_str(), "{run}");
}

#[test]
fn a_move_of_main_the_remote_never_gets_refuses_a_candidate_seen_behind() {
    let fixture = Fixture::new();
    let tip = generating(&fixture);
    let mut making = living();
    regenerator(&fixture, "building", &tip, i64::from(making.id()));
    let said = fixture.root.join("night.out");
    let heard = fixture.root.join("night.err");
    let mut night = fixture
        .command(
            NOW,
            &["--check", CHECK],
            &[("SYMPHONY_RELEASE_REGENERATOR_SECONDS", "2")],
        )
        .stdin(Stdio::null())
        .stdout(fs::File::create(&said).unwrap())
        .stderr(fs::File::create(&heard).unwrap())
        .spawn()
        .unwrap();
    until_said(&mut night, &said, "release: waiting for the regenerator: ");
    fixture.write("local.txt", "never pushed");
    let local = fixture.commit("a commit of this checkout alone");
    let code = ended_with(&mut night);
    let _ = making.kill();
    let _ = making.wait();
    let out = fs::read_to_string(&said).unwrap();
    let err = fs::read_to_string(&heard).unwrap();
    assert_eq!(code, Some(1), "{out}\n{err}");
    assert!(
        err.contains(&format!(
            "symphony release: main moved on to {} here, but origin/main is still at {}, whose \
             generated files the regenerator was last seen not to have made: no release of a \
             main whose generated files are behind its sources",
            &local[..12],
            &tip[..12]
        )),
        "{err}"
    );
    assert!(!out.contains("verify"), "nothing checked: {out}");
    assert!(fixture.remote_tags().is_empty());
    assert!(fixture.reds().is_empty(), "nothing filed");
    assert!(fixture.runs().is_empty(), "a refusal is not recorded");
}
