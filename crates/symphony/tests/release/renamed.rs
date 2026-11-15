//! What keeps working across the rename of `nightly` to `release`: a brief's `--release COMMIT`
//! hears the flag's new name; a record of version 1, where the first run of a build that knew the
//! new name moved it, reads as a release run, and the reds the old series still holds open are
//! written to and closed. What a build older than the rename makes again under the old names, the
//! records' folder and the checkout, is left as it is.

use std::fs;

use symphony_board::{NewSession, NewTask};

use super::cadence::counting;
use super::{CHECK, DAY, Fixture, NOW, git};

#[test]
fn the_old_flag_is_refused_naming_the_new_one_and_runs_nothing() {
    let fixture = counting(1);
    // A brief's `--release COMMIT` hears the flag's new name, in one line.
    for args in [
        &["--release", "HEAD"][..],
        &["--check", CHECK, "--release=HEAD"][..],
    ] {
        let (code, out, err) = fixture.release_at(NOW, args);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", "`--release COMMIT` is now `--commit COMMIT`\n"),
            "{args:?}"
        );
    }
    assert!(!fixture.records().exists(), "nothing ran");
    assert!(!fixture.worktree().exists());
}

/// A record of a red run of the fixture's first commit, as a build older than the rename wrote it
/// (version 1, its log named under `symphony/nightly/`), and its log, both where the first run of a
/// build that knew the new name moved them; and `filed`, the red it filed.
fn old_record(fixture: &Fixture, filed: &str) {
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let named = fixture
        .main
        .join(".git/symphony/nightly/logs/old/verify.log");
    let log = fixture.records().join("logs/old/verify.log");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::write(&log, "red\n").unwrap();
    let record = serde_json::json!({
        "version": 1,
        "id": "old",
        "kind": "nightly",
        "commit": first,
        "tip": first,
        "started_at": NOW - DAY,
        "ended_at": NOW - DAY + 60.0,
        "result": "red",
        "checks": [{
            "name": "verify",
            "command": "test ! -e red",
            "exit_code": 1,
            "signal": null,
            "seconds": 1.0,
            "log": named,
        }],
        "board": {"filed": [filed]},
        "remote": "origin",
    });
    fs::create_dir_all(fixture.records().join("runs")).unwrap();
    fs::write(fixture.records().join("runs/old.json"), record.to_string()).unwrap();
}

#[test]
fn an_older_builds_record_reads_as_a_release_run_and_its_open_red_is_closed_by_a_green() {
    let fixture = Fixture::new();
    let old_red = "tests/nightly-red-verify";
    old_record(&fixture, old_red);
    let store = fixture.store();
    let filer = store
        .register(&NewSession::new("filer"))
        .unwrap()
        .row
        .handle;
    store
        .create(&NewTask {
            priority: 0,
            red: Some("nightly".into()),
            ..NewTask::new(old_red, "Nightly red: verify", &filer)
        })
        .unwrap();
    let status_of = |slug: &str| {
        (store.tasks(None).unwrap().into_iter())
            .find(|task| task.slug == slug)
            .unwrap()
            .status
    };

    let status = fixture.run(NOW, &["--status"], 0);
    assert!(
        status.contains("last run: red, ") && status.contains(&format!("; red tasks: {old_red}\n")),
        "{status}"
    );

    // Red again, its red is the old task's, and nothing is filed beside it.
    fixture.land("red", Some("planted"), "a change that turns the check red");
    let out = fixture.night(NOW + 60.0, 1);
    assert!(
        out.contains(&format!("release: updated {old_red}\n")),
        "{out}"
    );
    assert!(fixture.reds().is_empty(), "nothing filed beside it: {out}");
    assert_eq!(fixture.runs().len(), 2);

    // Green: the old red closes.
    fixture.land("red", None, "the fix");
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.contains(&format!("release: closed {old_red}\n")),
        "{out}"
    );
    assert_eq!(status_of(old_red), "cancelled");
    assert!(fixture.reds().is_empty());
}

#[test]
fn what_an_older_build_makes_again_under_the_old_names_is_left_as_it_is() {
    let fixture = Fixture::new();
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let common = fixture.main.join(".git");
    // Its records' folder, holding a record and the word of a run.
    let old_store = common.join("symphony/nightly");
    fs::create_dir_all(old_store.join("runs")).unwrap();
    fs::write(old_store.join("runs/stray.json"), "{}\n").unwrap();
    fs::write(old_store.join("running.json"), "{}\n").unwrap();
    // Its checkout, locked and marked as it did, with a build and a process working in it.
    let old_tree = fixture.root.join("wt/nightly");
    let reason = "the Symphony nightly's checkout (docs/RELEASES.md)";
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--lock",
            "--reason",
            reason,
            &old_tree.to_string_lossy(),
            &first,
        ],
    );
    fs::write(
        common.join("worktrees/nightly/symphony-nightly"),
        format!("{}\n", old_tree.display()),
    )
    .unwrap();
    fs::create_dir_all(old_tree.join(".target/debug")).unwrap();
    fs::write(old_tree.join(".target/debug/kept"), "build\n").unwrap();
    let mut working = std::process::Command::new("sleep")
        .arg("120")
        .current_dir(&old_tree)
        .spawn()
        .unwrap();

    let ran = fixture.release_at(NOW, &["--check", CHECK]);
    let _ = working.kill();
    let _ = working.wait();
    let (code, out, err) = ran;
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("release: verify"),
        "checked in its own checkout: {out}"
    );
    assert!(
        !out.contains("release: moved") && !err.contains("in use"),
        "{out}{err}"
    );
    assert_eq!(fixture.runs().len(), 1, "recorded in its own folder alone");
    assert_eq!(fixture.runs()[0]["result"], "green");
    assert!(fixture.worktree().join(".git").exists());

    assert_eq!(
        fs::read_to_string(old_store.join("runs/stray.json")).unwrap(),
        "{}\n"
    );
    assert!(old_store.join("running.json").exists());
    assert!(
        !old_store.join("run.lock").exists(),
        "its lock is not taken"
    );
    assert_eq!(
        fs::read_to_string(old_tree.join(".target/debug/kept")).unwrap(),
        "build\n"
    );
    let listed = git(&fixture.main, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains(&format!("detached\nlocked {reason}")),
        "still that build's: {listed}"
    );
    assert!(fixture.reds().is_empty(), "nothing stopped the run");
}
