use super::*;

/// 2026-11-12 10:00 UTC.
const NOW: f64 = 1_794_477_600.0;

fn asked(state: &str, said: &str, at: f64) -> Asked {
    Asked {
        state: state.into(),
        said: said.into(),
        at,
        by: "the run".into(),
    }
}

fn folder(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "symphony-build-github-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn releases_are_ordered_oldest_first_by_day_then_number_each_once() {
    assert_eq!(
        oldest_first([
            "release-2026-11-12.10",
            "release-2026-11-13",
            "release-2026-11-12",
            "not-a-release",
            "release-2026-11-12.2",
            "release-2026-11-13",
        ]),
        [
            "release-2026-11-12",
            "release-2026-11-12.2",
            "release-2026-11-12.10",
            "release-2026-11-13"
        ]
    );
    assert_eq!(oldest_first([]).len(), 0);
}

#[test]
fn each_answer_is_one_line_naming_the_release() {
    let line = |state: &str, said: &str| asked(state, said, NOW).line("release-2026-11-12");
    assert_eq!(
        line("published", "https://h/r"),
        "published release-2026-11-12 as https://h/r"
    );
    assert_eq!(
        line("found", "https://h/r"),
        "release-2026-11-12 is there already: https://h/r"
    );
    assert_eq!(
        line("pending", "unavailable: GitHub answered 503"),
        "pending: release-2026-11-12: unavailable: GitHub answered 503"
    );
}

#[test]
fn the_ledger_is_empty_with_none_kept_or_one_that_does_not_read_and_says_nothing() {
    let folder = folder("empty");
    assert_eq!(Ledger::read(&folder), Ledger::default());
    assert_eq!(status(&folder, NOW), "");
    fs::write(folder.join(LEDGER), "{ not json").unwrap();
    assert_eq!(Ledger::read(&folder), Ledger::default());
    assert_eq!(status(&folder, NOW), "");
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn status_counts_what_is_published_names_each_pending_release_and_the_newest_published() {
    let folder = folder("status");
    let mut ledger = Ledger::default();
    for (name, state, said, at) in [
        (
            "release-2026-11-10",
            "published",
            "https://h/10",
            NOW - 7200.0,
        ),
        (
            "release-2026-11-11",
            "pending",
            "unknown: lost",
            NOW - 3600.0,
        ),
        (
            "release-2026-11-11.2",
            "found",
            "https://h/11.2",
            NOW - 3600.0,
        ),
        (
            "release-2026-11-12",
            "pending",
            "unavailable: 503",
            NOW - 60.0,
        ),
    ] {
        ledger.releases.insert(name.into(), asked(state, said, at));
    }
    ledger.write(&folder).unwrap();
    assert_eq!(Ledger::read(&folder), ledger);
    assert_eq!(
        ledger.pending(),
        ["release-2026-11-11", "release-2026-11-12"]
    );
    assert_eq!(
        status(&folder, NOW),
        format!(
            "GitHub releases: 2 published, 2 pending\n  \
             pending release-2026-11-11: unknown: lost; asked by the run, {} (1 h ago); {AGAIN}\n  \
             pending release-2026-11-12: unavailable: 503; asked by the run, {} (1 min ago); \
             {AGAIN}\n  \
             newest published: release-2026-11-11.2 as https://h/11.2; asked by the run, {} (1 h \
             ago)\n",
            local(NOW - 3600.0),
            local(NOW - 60.0),
            local(NOW - 3600.0)
        )
    );
    // With none published, only the pending are named.
    ledger.releases.retain(|_, asked| asked.pending());
    ledger.write(&folder).unwrap();
    let said = status(&folder, NOW);
    assert!(
        said.starts_with("GitHub releases: 0 published, 2 pending\n"),
        "{said}"
    );
    assert!(!said.contains("newest published"), "{said}");
    let _ = fs::remove_dir_all(&folder);
}

/// A copy of the lock's descriptor that outlives the publication, as a child forked by another
/// thread holds one until its exec, does not keep the lock: the next publication takes it.
#[test]
fn a_copy_of_the_lock_left_after_a_publication_holds_nothing() {
    let folder = folder("copied");
    let publishing = Publishing::take(&folder).expect("the lock is free");
    let copy = publishing.0.try_clone().unwrap();
    drop(publishing);
    let next = Publishing::take(&folder);
    assert!(next.is_some(), "the copy kept the lock");
    drop(copy);
    drop(next);
    assert!(Publishing::take(&folder).is_some());
}

#[test]
fn one_publication_runs_at_a_time_and_a_second_asks_nothing() {
    let folder = folder("locked");
    // Another publication, holding the lock as one does.
    let held = Publishing::take(&folder).expect("the lock is free");
    assert!(
        Publishing::take(&folder).is_none(),
        "one at a time, in one process too"
    );
    let command = ["/nowhere/never-run".to_string()];
    let asking = Asking {
        root: &folder,
        main: &folder,
        folder: &folder,
        command: &command,
        by: "the run",
        now: NOW,
        until: Bound::until(std::time::Instant::now() + ASK),
    };
    let wanted = ["release-2026-11-12".to_string()];
    let releases = [(
        Release::parse("release-2026-11-12").unwrap(),
        "c".repeat(40),
    )];
    let lines = asking.publish(&wanted, &releases);
    assert_eq!(lines.len(), 1);
    assert!(
        !lines[0].0
            && lines[0].1.starts_with("another publication holds ")
            && lines[0].1.ends_with(AGAIN),
        "{lines:?}"
    );
    assert!(!folder.join(LEDGER).exists());
    drop(held);
    // A release that is not tagged here is said and kept nowhere.
    let lines = asking.publish(&["release-2026-11-13".to_string()], &releases);
    assert_eq!(
        lines,
        [(
            false,
            "release-2026-11-13: no release of that name is tagged here".to_string()
        )]
    );
    assert!(!folder.join(LEDGER).exists());
    let _ = fs::remove_dir_all(&folder);
}
