//! A green tip is tagged as the day's release and only that tag reaches the remote; a commit is
//! released once, out of cycle only from the remote's main; `--status` reads and changes nothing.

use super::{CHECK, DAY, Fixture, NOW, git};

#[test]
fn a_green_tip_is_tagged_as_the_days_release_and_only_the_tag_reaches_the_remote() {
    let fixture = Fixture::new();
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    let heads = git(&fixture.origin, &["for-each-ref", "refs/heads"]);
    let out = fixture.night(NOW, 0);
    assert!(out.contains("nightly: verify green (exit 0"), "{out}");
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert_eq!(fixture.tagged("release-2026-11-12"), first);
    assert_eq!(
        git(&fixture.origin, &["for-each-ref", "refs/heads"]),
        heads,
        "no branch of the remote moved"
    );
    assert_eq!(
        git(&fixture.origin, &["cat-file", "-t", "release-2026-11-12"]),
        "tag",
        "an annotated tag"
    );
    assert_eq!(
        fixture.message("release-2026-11-12"),
        format!(
            "release-2026-11-12: Symphony main at {}\n\nBoard schema 16.\nPin 1.\n\nGreen: verify.\n\nThe first release.\n",
            &first[..12]
        )
    );

    // The checks ran in a clean checkout of their own outside the repository's folder, which
    // the board's worktree sweep leaves alone.
    let worktree = fixture.worktree();
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), first);
    assert_eq!(git(&worktree, &["status", "--porcelain"]), "");
    let listed = git(&fixture.main, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains(&format!(
            "worktree {}\nHEAD {first}\ndetached\nlocked",
            worktree.display()
        )),
        "{listed}"
    );

    let runs = fixture.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["kind"], "nightly");
    assert_eq!(runs[0]["result"], "green");
    assert_eq!(runs[0]["commit"], first.as_str());
    assert_eq!(runs[0]["tag"], "release-2026-11-12");
    assert_eq!(runs[0]["pushed"], true);
    assert_eq!(runs[0]["checks"][0]["name"], "verify");
    assert_eq!(runs[0]["checks"][0]["command"], "test ! -e red");
    assert_eq!(runs[0]["checks"][0]["exit_code"], 0);
}

#[test]
fn a_second_release_that_day_is_numbered_and_the_next_day_starts_again() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    let second = fixture.land("notes.txt", Some("a fix"), "a fix");
    fixture.night(NOW + 3600.0, 0);
    let third = fixture.land(
        "crates/tasks/src/schema.rs",
        Some("pub const VERSION: i64 = 17;\n"),
        "the board's schema 17",
    );
    fixture.night(NOW + DAY, 0);
    assert_eq!(
        fixture.remote_tags(),
        [
            "release-2026-11-12",
            "release-2026-11-12.2",
            "release-2026-11-13"
        ]
    );
    assert_eq!(fixture.tagged("release-2026-11-12.2"), second);
    assert_eq!(fixture.tagged("release-2026-11-13"), third);
    assert_eq!(
        fixture.message("release-2026-11-12.2"),
        format!(
            "release-2026-11-12.2: Symphony main at {}\n\n\
             Board schema 16, unchanged since release-2026-11-12.\n\
             Pin 1, unchanged since release-2026-11-12.\n\n\
             Green: verify.\n\n\
             Changes on main since release-2026-11-12 (1):\n{} a fix\n",
            &second[..12],
            git(&fixture.main, &["rev-parse", "--short", &second]),
        )
    );
    let message = fixture.message("release-2026-11-13");
    assert!(
        message.contains(
            "Board schema 17, changed since release-2026-11-12.2 (16 then).\n\
             Pin 1, unchanged since release-2026-11-12.2.\n"
        ),
        "the release before is the day's newest, not its first: {message}"
    );
    assert!(
        message.contains("Changes on main since release-2026-11-12.2 (1):\n"),
        "{message}"
    );

    // Nothing landed since: nothing to check, and no tag.
    let out = fixture.night(NOW + DAY + 60.0, 0);
    assert!(
        out.contains("origin/main is release-2026-11-13 already"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags().len(), 3);
    assert_eq!(fixture.runs().last().unwrap()["result"], "unchanged");
}

#[test]
fn a_release_held_back_from_the_remote_is_tagged_here_alone() {
    let fixture = Fixture::new();
    let out = fixture.run(NOW, &["--check", CHECK, "--no-push"], 0);
    assert!(
        out.contains("tagged release-2026-11-12, not pushed"),
        "{out}"
    );
    assert!(fixture.remote_tags().is_empty());
    assert_eq!(git(&fixture.main, &["tag", "-l"]), "release-2026-11-12");
    // The next release counts the one held here.
    fixture.land("notes.txt", Some("more"), "more");
    fixture.night(NOW + 60.0, 0);
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12.2"]);
}

#[test]
fn a_push_the_remote_refuses_keeps_the_tag_here_and_fails_the_run() {
    let fixture = Fixture::new();
    let hook = fixture.origin.join("hooks/pre-receive");
    std::fs::write(&hook, "#!/bin/sh\necho no tags today >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (code, out, err) = fixture.nightly_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, not pushed"),
        "{out}"
    );
    assert!(
        err.contains("tagged release-2026-11-12 here, but the push to origin failed"),
        "{err}"
    );
    assert!(err.contains("no tags today"), "the remote's words: {err}");
    assert!(fixture.remote_tags().is_empty());
    assert_eq!(git(&fixture.main, &["tag", "-l"]), "release-2026-11-12");
    let run = fixture.runs().pop().unwrap();
    assert_eq!(
        (
            run["result"].as_str(),
            run["tag"].as_str(),
            run["pushed"].as_bool()
        ),
        (Some("green"), Some("release-2026-11-12"), Some(false)),
        "{run}"
    );
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    assert!(
        status.contains("; tagged release-2026-11-12, not pushed"),
        "{status}"
    );
}

#[test]
fn out_of_cycle_a_commit_of_the_remotes_main_is_released_after_the_same_checks() {
    let fixture = Fixture::new();
    fixture.night(NOW, 0);
    let fix = fixture.land("fix.txt", Some("urgent"), "an urgent fix");
    let later = fixture.land("red", Some("planted"), "a red change after it");

    let out = fixture.run(NOW + 3600.0, &["--check", CHECK, "--release", &fix], 0);
    assert!(
        out.contains(&format!("nightly: release of {}", &fix[..12])),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12.2"), fix);
    assert_eq!(
        git(&fixture.worktree(), &["rev-parse", "HEAD"]),
        fix,
        "checked there"
    );
    assert_eq!(fixture.runs().last().unwrap()["kind"], "release");

    // Red: no tag.
    let (code, out, _) = fixture.nightly_at(NOW + 7200.0, &["--check", CHECK, "--release", &later]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("nightly: red: verify; no release"), "{out}");
    assert_eq!(fixture.remote_tags().len(), 2);

    // A commit the remote's main does not hold: refused before anything runs.
    fixture.write("local.txt", "never pushed");
    let local = fixture.commit("a commit of this checkout alone");
    let runs = fixture.runs().len();
    let (code, _, err) = fixture.nightly_at(NOW + 7300.0, &["--check", CHECK, "--release", &local]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains(&format!("({}) is not on origin/main", &local[..12])),
        "{err}"
    );
    let (code, _, err) = fixture.nightly_at(
        NOW + 7300.0,
        &["--check", CHECK, "--release", "no-such-rev"],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("no-such-rev names no commit"), "{err}");

    // A commit released already is not released again.
    let (code, _, err) = fixture.nightly_at(NOW + 7400.0, &["--check", CHECK, "--release", &fix]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("is release-2026-11-12.2 already"), "{err}");
    assert_eq!(fixture.runs().len(), runs, "a refusal records no run");
    assert_eq!(fixture.remote_tags().len(), 2);
}

#[test]
fn the_status_reads_the_record_and_the_tags_and_changes_nothing() {
    let fixture = Fixture::new();
    let out = fixture.run(NOW, &["--status"], 0);
    assert_eq!(
        out,
        "newest release: none yet\nlast night: never run\nlast green: none recorded\n"
    );
    fixture.night(NOW, 0);
    let green = fixture.land("notes.txt", Some("n"), "n");
    fixture.night(NOW + 3600.0, 0);
    fixture.land("red", Some("planted"), "a red change");
    fixture.night(NOW + 2.0 * 3600.0, 1);

    // The fixture's folder holds the checkout, its git common dir, the run's checkout, the remote
    // and the machine folder.
    let before = Fixture::files(&fixture.root);
    let out = fixture.run(NOW + 5.0 * 3600.0, &["--status"], 0);
    assert!(
        Fixture::files(&fixture.root) == before,
        "--status wrote nothing: not here, in the run's checkout, on the remote or in the machine folder"
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!("newest release: release-2026-11-12.2 ({})", &green[..12])
    );
    assert!(
        lines[1].starts_with("last night: red, 2026-11-12 12:00 (3 h ago), on "),
        "{out}"
    );
    assert!(
        lines[1].contains("; red: verify (exit 1); red tasks: tests/nightly-red-verify"),
        "{out}"
    );
    assert_eq!(
        lines[2],
        format!(
            "last green: 2026-11-12 11:00 (4 h ago), on {}",
            &green[..12]
        )
    );
    let late = fixture.run(NOW + 2.0 * DAY, &["--status"], 0);
    assert!(
        late.contains("(overdue: the nightly runs once a day)"),
        "{late}"
    );
}
