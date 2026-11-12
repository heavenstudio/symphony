//! A green tip is tagged as the day's release and only that tag reaches the remote; a commit is
//! released once, out of cycle only from the remote's main and past the newest release;
//! `--status` reads and changes nothing.

use std::fs;

use super::{CHECK, DAY, Fixture, NOW, git};

#[test]
fn a_green_tip_is_tagged_as_the_days_release_and_only_the_tag_reaches_the_remote() {
    let fixture = Fixture::new();
    let first = git(&fixture.main, &["rev-parse", "HEAD"]);
    // What a hostile configuration would push beside the release: an annotated tag on main's
    // line, a branch of this checkout's, and main moved on here.
    git(
        &fixture.main,
        &["tag", "-a", "wip-experiment", "-m", "not for the remote"],
    );
    git(&fixture.main, &["branch", "stray"]);
    let before = fixture.remote_refs();
    fixture.write("local.txt", "never pushed");
    fixture.commit("a commit of this checkout alone");
    let out = fixture.night(NOW, 0);
    assert!(out.contains("nightly: verify green (exit 0"), "{out}");
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert_eq!(fixture.tagged("release-2026-11-12"), first);
    let tag = git(
        &fixture.origin,
        &["rev-parse", "refs/tags/release-2026-11-12"],
    );
    assert_eq!(
        fixture.remote_refs(),
        format!("{before}\nrefs/tags/release-2026-11-12 {tag}"),
        "the remote gained the release and nothing else, under a configuration asking for more"
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
    let push = "`git push --no-follow-tags origin \
                refs/tags/release-2026-11-12:refs/tags/release-2026-11-12`";
    assert!(
        status.contains(&format!(
            "not on origin, as the last run to ask found: release-2026-11-12 (tagged here: \
             {push} delivers one)"
        )),
        "{status}"
    );

    // The next night has nothing new, but its release is still not on the remote: it says so,
    // fails, and the board hears of it; --status still says so.
    let (code, out, err) = fixture.nightly_at(NOW + 120.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("origin/main is release-2026-11-12 already"),
        "{out}"
    );
    assert!(
        err.contains(&format!(
            "release-2026-11-12 is tagged here but not on origin: {push} delivers it"
        )),
        "{err}"
    );
    // The refused push filed the run's red; this night adds to it.
    assert!(
        fixture.reds()["tests/nightly-red-run"].body.contains(push),
        "{:?}",
        fixture.reds()
    );
    assert!(
        out.contains("nightly: updated tests/nightly-red-run"),
        "{out}"
    );
    let thread = fixture.thread("tests/nightly-red-run");
    assert!(thread.last().unwrap().contains(push), "{thread:#?}");
    let status = fixture.run(NOW + 180.0, &["--status"], 0);
    assert!(status.contains("not on origin"), "{status}");

    // Delivered, the next night finds it there: nothing to say, and the red closed.
    std::fs::remove_file(&hook).unwrap();
    git(
        &fixture.main,
        &[
            "push",
            "-q",
            "origin",
            "refs/tags/release-2026-11-12:refs/tags/release-2026-11-12",
        ],
    );
    let out = fixture.night(NOW + 240.0, 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-run"),
        "{out}"
    );
    let status = fixture.run(NOW + 300.0, &["--status"], 0);
    assert!(!status.contains("not on origin"), "{status}");
}

#[test]
fn a_release_only_moves_forward() {
    let fixture = Fixture::new();
    let older = fixture.land("notes.txt", Some("a change"), "a change");
    let newer = fixture.land(
        "crates/tasks/src/schema.rs",
        Some("pub const VERSION: i64 = 17;\n"),
        "the board's schema 17",
    );
    fixture.night(NOW, 0);
    assert_eq!(fixture.tagged("release-2026-11-12"), newer);

    // A commit behind the newest release, out of cycle the next day: refused, nothing tagged.
    let runs = fixture.runs().len();
    let (code, out, err) = fixture.nightly_at(NOW + DAY, &["--check", CHECK, "--release", &older]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains(&format!(
            "{older} ({}) is not a descendant of the newest release release-2026-11-12 ({}): a \
             release only moves forward",
            &older[..12],
            &newer[..12]
        )),
        "{err}"
    );
    assert!(!out.contains("nightly: verify"), "nothing checked: {out}");
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert_eq!(fixture.runs().len(), runs, "a refusal records no run");

    // Main moved back on the remote: the night stops, tags nothing, and the board hears of it.
    git(
        &fixture.main,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{older}:refs/heads/main"),
        ],
    );
    let (code, out, err) = fixture.nightly_at(NOW + DAY, &["--check", CHECK]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.contains("is not a descendant of the newest release release-2026-11-12"),
        "{err}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert!(fixture.reds().contains_key("tests/nightly-red-run"));

    // And main back on a release before the newest is no release of the day either.
    git(
        &fixture.main,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{newer}:refs/heads/main"),
        ],
    );
    let fix = fixture.land("fix.txt", Some("fixed"), "a fix");
    fixture.night(NOW + DAY, 0);
    assert_eq!(fixture.tagged("release-2026-11-13"), fix);
    git(
        &fixture.main,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{newer}:refs/heads/main"),
        ],
    );
    let (code, _, err) = fixture.nightly_at(NOW + DAY + 60.0, &["--check", CHECK]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("origin/main is release-2026-11-12 (")
            && err.contains("behind the newest release release-2026-11-13: main went back"),
        "{err}"
    );
}

#[test]
fn a_second_night_resets_the_checkout_and_keeps_only_its_build_folder() {
    let fixture = Fixture::new();
    // Ignored as this repository's own `.gitignore` ignores it.
    fixture.write(".gitignore", "ignored/\n/.target/\n");
    fixture.land("tracked.txt", Some("as committed\n"), "a tracked file");
    fixture.night(NOW, 0);
    let worktree = fixture.worktree();
    for (file, text) in [
        ("tracked.txt", "changed by a check\n"),
        ("stray.txt", "left by a check\n"),
        ("ignored/output", "ignored output\n"),
        ("deep/.target/nested", "not the build folder\n"),
        (".target/debug/kept", "the build\n"),
    ] {
        let path = worktree.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let next = fixture.land("notes.txt", Some("more"), "more");
    fixture.night(NOW + 60.0, 0);
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), next);
    assert_eq!(
        fs::read_to_string(worktree.join("tracked.txt")).unwrap(),
        "as committed\n"
    );
    for gone in ["stray.txt", "ignored", "deep"] {
        assert!(!worktree.join(gone).exists(), "{gone} is gone");
    }
    assert_eq!(
        fs::read_to_string(worktree.join(".target/debug/kept")).unwrap(),
        "the build\n",
        "the build folder stays"
    );
    assert_eq!(
        git(&worktree, &["status", "--porcelain", "--ignored"]),
        "!! .target/"
    );

    // A week of nights later the build folder is cleared before the night builds.
    fixture.land("notes.txt", Some("later"), "later");
    let out = fixture.night(NOW + 8.0 * DAY, 0);
    assert!(
        out.contains(&format!(
            "nightly: cleared {}, built on for 7 days or more",
            worktree.join(".target").display()
        )),
        "{out}"
    );
    assert!(!worktree.join(".target").exists());
}

#[test]
fn another_remote_and_another_main_line_are_released_from_and_to_alone() {
    let fixture = Fixture::new();
    // A second remote whose main moved on, and a trunk on the first.
    let upstream = fixture.root.join("upstream.git");
    fs::create_dir_all(&upstream).unwrap();
    git(&upstream, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &fixture.main,
        &["remote", "add", "upstream", &upstream.to_string_lossy()],
    );
    let upstream_tip = fixture.commit("on upstream alone");
    git(
        &fixture.main,
        &["push", "-q", "upstream", "HEAD:refs/heads/main"],
    );
    let out = fixture.run(NOW, &["--check", CHECK, "--remote", "upstream"], 0);
    assert!(
        out.contains("nightly: green; tagged release-2026-11-12, pushed to upstream"),
        "{out}"
    );
    assert_eq!(
        git(&upstream, &["rev-parse", "release-2026-11-12^{commit}"]),
        upstream_tip
    );
    assert!(fixture.remote_tags().is_empty(), "origin took nothing");

    let trunk = fixture.commit("on trunk alone");
    git(
        &fixture.main,
        &["push", "-q", "origin", "HEAD:refs/heads/trunk"],
    );
    let out = fixture.run(NOW + 60.0, &["--check", CHECK, "--branch", "trunk"], 0);
    assert!(out.contains("(origin/trunk at "), "{out}");
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12.2"]);
    assert_eq!(fixture.tagged("release-2026-11-12.2"), trunk);
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
        "newest release: none yet\nlast night: never run\nlast green: none recorded\n\
         landings: 0 on origin/main since the line's start: no release and no run on record, so \
         every landing counts\n\
         next run: after 20 more landings (0 of 20), or the night's run\n\
         last refresh: none recorded\n"
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
    // The count starts again from the red run, which checked the tip, as from a release.
    let red = fixture.runs().pop().unwrap();
    assert_eq!(
        lines[3..],
        [
            format!(
                "landings: 0 on origin/main since the run {} on {} (red)",
                red["id"].as_str().unwrap(),
                &red["commit"].as_str().unwrap()[..12]
            )
            .as_str(),
            "next run: after 20 more landings (0 of 20), or the night's run",
            "last refresh: none recorded",
        ]
    );
    let late = fixture.run(NOW + 2.0 * DAY, &["--status"], 0);
    assert!(
        late.contains("(overdue: the nightly runs once a day)"),
        "{late}"
    );
}

#[test]
fn the_tests_clock_never_pushes_a_release_off_this_machine() {
    let fixture = Fixture::new();
    // A remote that fetches from a folder here but pushes elsewhere.
    let far = "https://example.invalid/symphony.git";
    git(
        &fixture.main,
        &["remote", "set-url", "--push", "origin", far],
    );
    let future = NOW + 400.0 * DAY;
    let head = git(&fixture.main, &["rev-parse", "HEAD"]);
    for args in [
        &["--check", CHECK][..],
        &["--check", CHECK, "--release", head.as_str()][..],
    ] {
        let (code, out, err) = fixture.nightly_at(future, args);
        assert_eq!(code, 2, "{args:?}: {out}{err}");
        assert!(
            err.contains(&format!(
                "SYMPHONY_NIGHTLY_NOW is the tests' clock: a run on it pushes only to a remote \
                 that is a folder on this machine, and origin pushes to {far}; add --no-push"
            )),
            "{args:?}: {err}"
        );
    }
    assert_eq!(
        git(&fixture.main, &["tag", "-l"]),
        "",
        "no release named for that day"
    );
    assert!(fixture.remote_tags().is_empty());
    assert!(!fixture.records().join("runs").exists(), "nothing ran");

    // Kept here, a release may be named for any day: nothing leaves this machine.
    let out = fixture.run(future, &["--check", CHECK, "--no-push"], 0);
    assert!(
        out.contains("nightly: green; tagged release-2027-12-17, not pushed"),
        "{out}"
    );
    assert!(fixture.remote_tags().is_empty());
}
