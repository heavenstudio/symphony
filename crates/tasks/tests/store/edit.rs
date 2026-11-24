//! The bound edit (`Store::edit_bound`): a task's title and brief rewritten
//! only on the task its token names, in one transaction with the fence, and a refusal that
//! writes nothing of it.

use crate::common::refused;
use crate::recovery::{Memory, RESTORED, STALE, TITLE, bound_board, original};
use std::sync::{Arc, Barrier};
use symphony_board::recovery::{Authority, Bound, BoundTask, COMPAT, Edit};
use symphony_board::{Error, NewTask, Store};

const SLUG: &str = "probe/original";

fn edit(title: Option<&str>, body: Option<&str>, links: &[&str]) -> Edit {
    Edit {
        title: title.map(Into::into),
        body: body.map(Into::into),
        links: links.iter().map(ToString::to_string).collect(),
    }
}

fn rewrite(store: &Store, at: &Bound, body: &str) -> Result<BoundTask, Error> {
    store.edit_bound(SLUG, at, &edit(None, Some(body), &[]), "owner")
}

#[test]
fn an_edit_rewrites_the_title_the_brief_and_the_links_in_one_write() {
    let (f, _, store) = bound_board(original());
    let worker = f.session("worker");
    let read = store.bound(SLUG).unwrap();
    f.advance(5.0);
    let wrote = store
        .edit_bound(
            SLUG,
            &read.bound,
            &edit(
                Some("  A new title "),
                Some("A brief of quokkas.\n"),
                &["docs/TASKS.md", "docs/TASKS.md"],
            ),
            &worker,
        )
        .unwrap();
    assert_eq!(
        (wrote.title.as_str(), wrote.body.as_str()),
        ("A new title", "A brief of quokkas.\n")
    );
    assert_eq!(
        wrote.bound,
        Bound {
            version: read.bound.version + 1,
            ..read.bound.clone()
        }
    );
    assert_eq!(store.bound(SLUG).unwrap(), wrote);
    let task = f.store.get(SLUG).unwrap();
    assert_eq!(
        (
            task.id,
            task.title.as_str(),
            task.body.as_str(),
            task.links.len()
        ),
        (read.bound.id, "A new title", "A brief of quokkas.\n", 1)
    );
    assert_eq!(
        f.said(SLUG),
        [
            format!("{worker} retitled it: {TITLE} → A new title"),
            format!("{worker} rewrote its brief"),
        ]
    );
    assert_eq!(f.store.session(&worker).unwrap().row.last_seen, f.now());
    // The search reads the brief as it is now.
    let found: Vec<String> = f
        .store
        .search("quokkas")
        .unwrap()
        .into_iter()
        .map(|hit| hit.slug)
        .collect();
    assert_eq!(found, [SLUG]);
}

#[test]
fn what_an_edit_leaves_out_stays_and_what_it_repeats_is_no_line() {
    let (f, _, store) = bound_board(original());
    let first = store.bound(SLUG).unwrap();
    let briefed = rewrite(&store, &first.bound, "Only the brief").unwrap();
    assert_eq!(
        (briefed.title.as_str(), briefed.body.as_str()),
        (TITLE, "Only the brief")
    );
    let titled = store
        .edit_bound(
            SLUG,
            &briefed.bound,
            &edit(Some("Only the title"), None, &[]),
            "owner",
        )
        .unwrap();
    assert_eq!(
        (titled.title.as_str(), titled.body.as_str()),
        ("Only the title", "Only the brief")
    );
    let linked = store
        .edit_bound(
            SLUG,
            &titled.bound,
            &edit(None, None, &["docs/RUNS.md"]),
            "owner",
        )
        .unwrap();
    assert_eq!(
        (linked.title.as_str(), linked.body.as_str()),
        ("Only the title", "Only the brief")
    );
    assert_eq!(f.store.get(SLUG).unwrap().links, ["docs/RUNS.md"]);
    // The same words again are a write, with its next token, and nothing said.
    let same = store
        .edit_bound(
            SLUG,
            &linked.bound,
            &edit(Some("Only the title"), Some("Only the brief"), &[]),
            "owner",
        )
        .unwrap();
    assert_eq!(same.bound.version, first.bound.version + 4);
    // A brief may be emptied.
    let emptied = rewrite(&store, &same.bound, "").unwrap();
    assert_eq!(emptied.body, "");
    assert_eq!(
        f.said(SLUG),
        [
            "owner rewrote its brief".to_string(),
            format!("owner retitled it: {TITLE} → Only the title"),
            "owner rewrote its brief".to_string(),
        ]
    );
}

#[test]
fn an_edit_on_a_stale_or_foreign_token_writes_nothing() {
    let (f, _, store) = bound_board(original().task("probe/other", "ready"));
    let worker = f.session("worker");
    let at = store.bound(SLUG).unwrap().bound;
    let other = store.bound("probe/other").unwrap().bound;
    f.store.amend(SLUG, "Moved on", &[], "owner").unwrap();
    let now = store.bound(SLUG).unwrap().bound;
    f.advance(5.0);
    let before = f.store.dump().unwrap();
    let tried = Store::tried_again_here();
    let whole = edit(Some("Stale title"), Some("Stale brief"), &["docs/TASKS.md"]);
    let write = |slug: &str, at: &Bound, edit: &Edit, by: &str| {
        refused(store.edit_bound(slug, at, edit, by))
    };
    let cases = [
        (write(SLUG, &at, &whole, &worker), STALE),
        (write(SLUG, &other, &whole, &worker), STALE),
        (
            write(
                SLUG,
                &Bound {
                    version: now.version + 1,
                    ..now.clone()
                },
                &whole,
                &worker,
            ),
            STALE,
        ),
        (
            write(
                SLUG,
                &Bound {
                    generation: "another".into(),
                    ..now.clone()
                },
                &whole,
                &worker,
            ),
            RESTORED,
        ),
        (
            write(
                SLUG,
                &Bound {
                    compat: COMPAT + 1,
                    ..now.clone()
                },
                &whole,
                &worker,
            ),
            "compatibility version 2",
        ),
        (
            write("probe/none", &now, &whole, &worker),
            "no task 'probe/none'",
        ),
        (write(SLUG, &now, &whole, "nobody-there"), "nobody-there"),
        (
            write(SLUG, &now, &edit(Some("  "), Some("A brief"), &[]), &worker),
            "a task needs a title",
        ),
        (
            write(SLUG, &now, &Edit::default(), &worker),
            "an edit gives a title, a brief or links",
        ),
        (
            refused(f.store.edit_bound(SLUG, &now, &whole, &worker)),
            "opened with no recovery authority",
        ),
    ];
    for (why, expected) in &cases {
        assert!(why.contains(expected), "{expected}: {why}");
    }
    assert_eq!(
        Store::tried_again_here(),
        tried,
        "a mismatch is refused, never read again"
    );
    assert_eq!(
        f.store.dump().unwrap(),
        before,
        "no title, brief, link, line or act was written"
    );
    assert_eq!(
        f.store.session(&worker).unwrap().row.last_seen,
        crate::common::NOW
    );
    let wrote = store.edit_bound(SLUG, &now, &whole, &worker).unwrap();
    assert_eq!(
        (wrote.title.as_str(), wrote.body.as_str()),
        ("Stale title", "Stale brief")
    );
}

#[test]
fn an_edit_read_before_a_copy_never_rewrites_the_task_the_copy_gave_its_number() {
    let seed = original();
    let snapshot = seed.dump.clone();
    let (f, _, store) = bound_board(seed);
    let lost = f.task_with(NewTask::new("probe/lost", "Lost", "owner"));
    let lost_at = store.bound("probe/lost").unwrap().bound;
    let at = store.bound(SLUG).unwrap().bound;

    store.copy(&snapshot).unwrap();
    let replacement = f.task_with(NewTask::new("probe/replacement", "Replacement", "owner"));
    assert_eq!(
        replacement.id, lost.id,
        "the copy gives the lost task's number again"
    );
    let now = store.bound(SLUG).unwrap().bound;
    assert_eq!(
        (now.id, now.version),
        (at.id, at.version),
        "the same number and version"
    );
    let before = f.store.dump().unwrap();
    for (slug, at) in [(SLUG, &at), ("probe/replacement", &lost_at)] {
        let whole = edit(Some("Across the copy"), Some("Across the copy"), &[]);
        let why = refused(store.edit_bound(slug, at, &whole, "owner"));
        assert!(why.contains(RESTORED), "{slug}: {why}");
        // The fence itself, by a writer whose authority still names the generation it read.
        let behind = Arc::new(Memory::default());
        behind.publish(&at.generation).unwrap();
        let why = refused(
            f.other()
                .recovering(behind)
                .edit_bound(slug, at, &whole, "owner"),
        );
        assert!(why.contains(RESTORED), "{slug}: {why}");
    }
    assert_eq!(f.store.dump().unwrap(), before, "nothing was written");
    assert_eq!(
        rewrite(&store, &now, "On this history").unwrap().body,
        "On this history"
    );
}

#[test]
fn two_edits_of_one_token_at_once_write_once() {
    let (f, authority, store) = bound_board(original());
    let at = store.bound(SLUG).unwrap().bound;
    let (first, second) = (
        f.other().recovering(authority.clone()),
        f.other().recovering(authority.clone()),
    );
    let barrier = Barrier::new(2);
    let (a, b) = std::thread::scope(|threads| {
        let a = threads.spawn(|| {
            barrier.wait();
            rewrite(&first, &at, "One")
        });
        let b = threads.spawn(|| {
            barrier.wait();
            rewrite(&second, &at, "Two")
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    let (won, lost) = match (a, b) {
        (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
        both => panic!("one of two edits of one token wins: {both:?}"),
    };
    assert!(lost.to_string().contains(STALE), "{lost}");
    assert_eq!(store.bound(SLUG).unwrap(), won);
    assert_eq!(f.said(SLUG), ["owner rewrote its brief"]);
}

#[test]
fn an_edit_reads_from_json_with_what_it_leaves_out_and_refuses_what_it_does_not_know() {
    let read = |value| serde_json::from_value::<Edit>(value);
    assert_eq!(read(serde_json::json!({})).unwrap(), Edit::default());
    assert_eq!(
        read(serde_json::json!({"title": "T", "body": null, "links": ["a"]})).unwrap(),
        edit(Some("T"), None, &["a"])
    );
    assert!(read(serde_json::json!({"brief": "B"})).is_err());
}
