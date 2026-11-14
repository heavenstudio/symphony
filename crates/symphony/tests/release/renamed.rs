//! What keeps working across the rename of `nightly` to `release`: a brief's `--release COMMIT`
//! hears the flag's new name; the first run moves the records, the checkout and its build from
//! their old names, reads the old records as release runs, and writes to and closes the reds the
//! old series still holds open.

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

/// The checkout a build older than the rename made at `wt/nightly`, locked and marked as it did,
/// with a build: its path.
fn old_checkout(fixture: &Fixture) -> std::path::PathBuf {
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let old_tree = fixture.root.join("wt/nightly");
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--lock",
            "--reason",
            "the Symphony nightly's checkout (docs/RELEASES.md)",
            &old_tree.to_string_lossy(),
            &first,
        ],
    );
    fs::write(
        fixture.main.join(".git/worktrees/nightly/symphony-nightly"),
        format!("{}\n", old_tree.display()),
    )
    .unwrap();
    fs::create_dir_all(old_tree.join(".target/debug")).unwrap();
    fs::write(old_tree.join(".target/debug/kept"), "build\n").unwrap();
    old_tree
}

#[test]
fn a_process_in_what_an_older_build_left_keeps_it_where_it_is_and_stops_the_run() {
    let fixture = Fixture::new();
    let old_tree = old_checkout(&fixture);
    let mut shell = std::process::Command::new("sleep")
        .arg("120")
        .current_dir(&old_tree)
        .spawn()
        .unwrap();
    let (code, out, err) = fixture.release_at(NOW, &["--check", CHECK]);
    let _ = shell.kill();
    let _ = shell.wait();
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "{} is in use by process {} (sleep), working there: nothing is checked while a \
             process is in the run's folders; end it, or move it out of that folder",
            old_tree.display(),
            shell.id()
        )),
        "{err}"
    );
    assert!(!out.contains("moved the release run's checkout"), "{out}");
    assert!(!out.contains("release: verify"), "nothing checked: {out}");
    assert!(old_tree.join(".target/debug/kept").exists() && !fixture.worktree().exists());
    let listed = git(&fixture.main, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains("locked the Symphony nightly's checkout (docs/RELEASES.md)"),
        "still that build's: {listed}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-run"],
        "a stop the owner clears"
    );

    // Gone, the next run moves it and gets through.
    let out = fixture.night(NOW + 60.0, 0);
    assert!(
        out.contains(&format!(
            "release: moved the release run's checkout from {} to {}",
            old_tree.display(),
            fixture.worktree().display()
        )),
        "{out}"
    );
    assert!(
        out.contains("release: closed tests/release-red-run"),
        "{out}"
    );
    assert!(!old_tree.exists());
}

#[test]
fn the_first_run_moves_what_an_older_build_left_and_reads_its_runs_and_reds_as_its_own() {
    let fixture = Fixture::new();
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let common = fixture.main.join(".git");
    let old_store = common.join("symphony/nightly");
    let old_tree = fixture.root.join("wt/nightly");
    // The checkout a build older than the rename made, locked and marked as it did, with a build.
    git(
        &fixture.main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--lock",
            "--reason",
            "the Symphony nightly's checkout (docs/RELEASES.md)",
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
    // Its record of a red run of the first commit, with its log, and that red open on the board.
    let log = old_store.join("logs/old/verify.log");
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
            "log": log,
        }],
        "board": {"filed": ["tests/nightly-red-verify"]},
        "remote": "origin",
    });
    fs::create_dir_all(old_store.join("runs")).unwrap();
    fs::write(old_store.join("runs/old.json"), record.to_string()).unwrap();
    let store = fixture.store();
    let filer = store
        .register(&NewSession::new("filer"))
        .unwrap()
        .row
        .handle;
    let old_red = NewTask {
        priority: 0,
        red: Some("nightly".into()),
        ..NewTask::new("tests/nightly-red-verify", "Nightly red: verify", &filer)
    };
    store.create(&old_red).unwrap();
    let status_of = |slug: &str| {
        (store.tasks(None).unwrap().into_iter())
            .find(|task| task.slug == slug)
            .unwrap()
            .status
    };

    // Read where that build left it, as a release run, and nothing moved by reading.
    let status = fixture.run(NOW, &["--status"], 0);
    assert!(
        status.contains("last run: red, ")
            && status.contains("; red tasks: tests/nightly-red-verify\n"),
        "{status}"
    );
    assert!(old_store.join("runs/old.json").exists());
    assert!(!fixture.records().exists(), "reading moves nothing");

    // The first run moves the records and the checkout; red again, its red is the old task's.
    fixture.land("red", Some("planted"), "a change that turns the check red");
    let out = fixture.night(NOW + 60.0, 1);
    assert!(
        out.contains(&format!(
            "release: moved the release run's records from {} to {}, where a build older than the \
             rename to `just release` kept them\n",
            old_store.display(),
            fixture.records().display()
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "release: moved the release run's checkout from {} to {}, where a build older than \
             the rename to `just release` made it\n",
            old_tree.display(),
            fixture.worktree().display()
        )),
        "{out}"
    );
    assert!(
        out.contains("release: updated tests/nightly-red-verify\n"),
        "{out}"
    );
    assert!(fixture.reds().is_empty(), "nothing filed beside it: {out}");
    assert!(!old_store.exists() && !old_tree.exists());
    assert_eq!(
        fs::read_to_string(fixture.records().join("logs/old/verify.log")).unwrap(),
        "red\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.worktree().join(".target/debug/kept")).unwrap(),
        "build\n",
        "its build moved with it"
    );
    let listed = git(&fixture.main, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains(&format!(
            "worktree {}\nHEAD {}\ndetached\nlocked the Symphony release run's checkout \
             (docs/RELEASES.md)",
            fixture.worktree().display(),
            git(&fixture.main, &["rev-parse", "HEAD"])
        )),
        "{listed}"
    );
    assert_eq!(fixture.runs().len(), 2);

    // Green: the old red closes, and nothing is moved again.
    fixture.land("red", None, "the fix");
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.contains("release: closed tests/nightly-red-verify\n"),
        "{out}"
    );
    assert!(!out.contains("release: moved"), "moved once: {out}");
    assert_eq!(status_of("tests/nightly-red-verify"), "cancelled");
    assert!(fixture.reds().is_empty());
}
