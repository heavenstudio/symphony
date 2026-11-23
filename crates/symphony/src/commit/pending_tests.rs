//! [`super`] where no repository is needed: what a journal entry's name and a kept failure's
//! words become, and what a folder that is no checkout answers.

use super::{
    BRANCHES, Batched, Delivery, QueuedPublication, Save, State, Together, applying, behind, flat,
    going, inert, joining, kept, led, note, saves, sequence_of, shown, stopped, whose,
};
use crate::test_tree::Tree;
use std::path::Path;

/// A temporary folder that is a repository on `main`, with nothing committed: a git common
/// folder for the journal to be read from.
fn repository() -> Tree {
    let tree = Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    tree
}

/// A journal entry of `file`, saved by `by` (empty for the owner), as far as `delivery` went.
fn entry(tree: &Tree, sequence: u64, file: &str, by: &str, delivery: Delivery) {
    let saved = QueuedPublication {
        edit: crate::commit::Edit {
            rel: file.into(),
            base: String::new(),
            new: format!("{sequence}"),
        },
        publication: None,
        dirty_retry: false,
        dirty_reported: false,
        delivery: Some(Delivery {
            by: by.into(),
            ..delivery
        }),
    };
    let folder = tree
        .root
        .join(".git")
        .join(symphony_board::folders::SERVER)
        .join(super::JOURNAL);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{sequence:020}.json"));
    std::fs::write(path, serde_json::to_string(&saved).unwrap()).unwrap();
}

/// Committed on its branch: `tip` its commit.
fn committed(sequence: u64, tip: &str) -> Delivery {
    Delivery {
        branch: format!("{BRANCHES}{sequence}"),
        tip: tip.into(),
        ..Delivery::default()
    }
}

fn numbers(joining: Vec<(u64, QueuedPublication)>) -> Vec<u64> {
    joining.into_iter().map(|(sequence, _)| sequence).collect()
}

#[test]
fn a_journal_entrys_number_is_its_file_name() {
    let name = format!("{:020}.json", 42);
    assert_eq!(
        sequence_of(Path::new("journal").join(name).as_path()),
        Some(42)
    );
    assert_eq!(sequence_of(Path::new("journal/notes.json")), None);
    assert_eq!(sequence_of(Path::new("")), None);
}

#[test]
fn a_failures_words_stay_on_the_notes_one_line() {
    assert_eq!(
        flat("git exited\n  status 1\n\n> quoted"),
        "git exited status 1 > quoted"
    );
    assert_eq!(flat(""), "");
    // Cut at a word, within the limit, and said as cut.
    let long = flat(&"word ".repeat(100));
    assert_eq!(long, format!("{}…", "word ".repeat(39).trim_end()));
    assert!(long.chars().count() <= super::WHY_CHARS);
}

#[test]
fn an_entry_without_a_word_of_delivery_reads_as_none_and_writes_none() {
    let plain = r#"{"edit":{"rel":"docs/a.md","base":"a","new":"b"},"publication":null}"#;
    let saved: QueuedPublication = serde_json::from_str(plain).unwrap();
    assert!(saved.delivery.is_none());
    assert!(!serde_json::to_string(&saved).unwrap().contains("delivery"));
    // One a pass has not reached yet: every field at its default.
    let kept = format!(r#"{},"delivery":{{}}}}"#, plain.strip_suffix('}').unwrap());
    let saved: QueuedPublication = serde_json::from_str(&kept).unwrap();
    assert_eq!(saved.delivery, Some(Delivery::default()));
}

#[test]
fn a_folder_that_is_no_checkout_keeps_and_shows_nothing() {
    let tree = Tree::new();
    let root = tree.path("");
    assert!(!kept(&root));
    assert!(saves(&root).is_empty());
    assert_eq!(note(&root, "docs/a.md"), None);
    assert_eq!(shown(&root, "docs/a.md", "text".into()), "text");
}

#[test]
fn a_note_names_its_saves_by_whose_they_are() {
    let save = |by: &str| Save {
        sequence: 1,
        file: "docs/a.md".into(),
        branch: "docs/owner-save-1".into(),
        shown: true,
        state: State::Queued,
        by: by.into(),
        keys: Vec::new(),
    };
    assert_eq!(whose(&[save("")]), "an owner's save");
    assert_eq!(whose(&[save("fox-1")]), "an agent's save");
    assert_eq!(whose(&[save(""), save("")]), "2 owner's saves");
    assert_eq!(whose(&[save(""), save("fox-1")]), "2 saves");
    assert_eq!(whose(&[save("fox-1"), save("agent"), save("")]), "3 saves");
}

#[test]
fn an_owners_entry_names_no_one_and_an_agents_names_its_session() {
    let owner = Delivery::default();
    let written = serde_json::to_string(&owner).unwrap();
    assert!(!written.contains("\"by\""), "{written}");
    let agent = Delivery {
        by: "fox-1".into(),
        ..Delivery::default()
    };
    let written = serde_json::to_string(&agent).unwrap();
    assert!(written.contains("\"by\":\"fox-1\""), "{written}");
    assert_eq!(serde_json::from_str::<Delivery>(&written).unwrap(), agent);
}

#[test]
fn an_agents_save_takes_the_later_agents_saves_whose_turn_has_come_of_any_document() {
    let tree = repository();
    entry(&tree, 1, "docs/a.md", "fox-1", Delivery::default());
    entry(&tree, 2, "docs/b.md", "fox-1", Delivery::default());
    entry(&tree, 3, "docs/a.md", "", Delivery::default());
    entry(&tree, 4, "docs/c.md", "agent", Delivery::default());
    entry(&tree, 5, "docs/a.md", "fox-1", Delivery::default());
    // The owner's save of a.md goes with none, and holds back the agent's after it there.
    assert_eq!(numbers(joining(&tree.root, 1)), [2, 4]);
    // An earlier save is never taken: a leader takes only those kept after it.
    assert_eq!(numbers(joining(&tree.root, 2)), [4]);
    assert_eq!(numbers(joining(&tree.root, 4)), Vec::<u64>::new());
}

#[test]
fn a_save_still_on_its_way_holds_back_the_later_saves_of_its_document_and_no_other() {
    let tree = repository();
    entry(&tree, 1, "docs/a.md", "", committed(1, "c1"));
    entry(&tree, 2, "docs/b.md", "fox-1", Delivery::default());
    entry(&tree, 3, "docs/a.md", "fox-1", Delivery::default());
    entry(&tree, 4, "docs/b.md", "fox-1", Delivery::default());
    // 4 follows 2 on b.md, which goes with it; 3 waits behind the owner's 1 on a.md.
    assert_eq!(numbers(joining(&tree.root, 2)), [4]);
    // Each later save of a held-back document waits, whatever its own turn.
    entry(&tree, 5, "docs/a.md", "fox-1", Delivery::default());
    assert_eq!(numbers(joining(&tree.root, 2)), [4]);
}

#[test]
fn a_save_on_no_way_to_the_default_branch_holds_back_nothing() {
    let tree = repository();
    let closed = Delivery {
        closed: true,
        ..committed(1, "c1")
    };
    entry(&tree, 1, "docs/a.md", "", closed);
    let stopped = Delivery {
        problem: "conflicts with main".into(),
        ..Delivery::default()
    };
    entry(&tree, 2, "docs/a.md", "fox-1", stopped);
    entry(&tree, 3, "docs/b.md", "fox-1", Delivery::default());
    entry(&tree, 4, "docs/a.md", "fox-1", Delivery::default());
    // The stopped save retries alone and goes with none, as does a closed one.
    assert_eq!(numbers(joining(&tree.root, 3)), [4]);
}

#[test]
fn a_save_another_took_or_already_committed_is_not_taken_again() {
    let tree = repository();
    let leading = Delivery {
        members: vec![2],
        ..committed(1, "c1")
    };
    entry(&tree, 1, "docs/a.md", "fox-1", leading);
    entry(&tree, 2, "docs/b.md", "fox-1", Delivery::default());
    entry(&tree, 3, "docs/c.md", "fox-1", committed(3, "c3"));
    entry(&tree, 4, "docs/d.md", "fox-1", Delivery::default());
    entry(&tree, 5, "docs/b.md", "fox-1", Delivery::default());
    entry(&tree, 6, "docs/c.md", "fox-1", Delivery::default());
    // 2 is 1's, and so b.md waits for it; 3 is committed, and c.md waits for it.
    assert_eq!(numbers(joining(&tree.root, 4)), Vec::<u64>::new());
    assert_eq!(
        led(&tree.root, 2),
        Some((format!("{BRANCHES}1"), "c1".into()))
    );
    assert_eq!(led(&tree.root, 4), None);
}

#[test]
fn a_save_is_delivered_by_the_branch_it_went_on_else_by_its_own() {
    assert_eq!(Delivery::default().slug(7), format!("{BRANCHES}7"));
    assert_eq!(committed(3, "c3").slug(7), format!("{BRANCHES}3"));
    // On its way unless closed, or stopped short of a branch.
    assert!(Delivery::default().on_its_way());
    assert!(committed(3, "c3").on_its_way());
    let stopped = Delivery {
        problem: "a board that does not answer".into(),
        ..Delivery::default()
    };
    assert!(!stopped.on_its_way());
    let stopped_after = Delivery {
        problem: "its task could not be read".into(),
        ..committed(3, "c3")
    };
    assert!(stopped_after.on_its_way());
    let closed = Delivery {
        closed: true,
        ..committed(3, "c3")
    };
    assert!(!closed.on_its_way());
}

#[test]
fn an_entry_names_the_saves_that_went_with_it_and_nothing_with_none() {
    let lone = serde_json::to_string(&Delivery::default()).unwrap();
    assert!(!lone.contains("members"), "{lone}");
    let leading = Delivery {
        members: vec![4, 9],
        ..Delivery::default()
    };
    let written = serde_json::to_string(&leading).unwrap();
    assert!(written.contains(r#""members":[4,9]"#), "{written}");
    assert_eq!(serde_json::from_str::<Delivery>(&written).unwrap(), leading);
}

/// `args` run by git in `tree`, its output.
fn git(tree: &Tree, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(&tree.root)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com"),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

fn edit(rel: &str, base: &str, new: &str) -> crate::commit::Edit {
    crate::commit::Edit {
        rel: rel.into(),
        base: base.into(),
        new: new.into(),
    }
}

#[test]
fn a_save_that_no_longer_applies_after_those_before_it_is_left_out_with_the_rest_of_its_page() {
    let tree = repository();
    tree.write("docs/a.md", "one\n");
    tree.write("docs/b.md", "x\n");
    git(&tree, &["add", "docs"]);
    git(&tree, &["commit", "-qm", "docs"]);
    let parent = git(&tree, &["rev-parse", "HEAD"]);
    let first = edit("docs/b.md", "x\n", "x\ny\n");
    // Written against a line main no longer has, and the one written after it.
    let stale = edit("docs/a.md", "old\n", "new\n");
    let after = edit("docs/a.md", "new\n", "new\nmore\n");
    let next = edit("docs/b.md", "x\ny\n", "x\ny\nz\n");
    let batch =
        [(1, &first), (2, &stale), (3, &after), (4, &next)].map(|(sequence, edit)| Batched {
            sequence,
            edit,
            by: "fox-1",
            together: None,
        });
    let (edits, members) = applying(&tree.root, &parent, &batch).unwrap();
    assert_eq!(members, [4]);
    let files: Vec<&str> = edits.iter().map(|edit| edit.new.as_str()).collect();
    assert_eq!(files, ["x\ny\n", "x\ny\nz\n"]);
    // The first save's own failure is the batch's.
    let batch = [Batched {
        sequence: 2,
        edit: &stale,
        by: "fox-1",
        together: None,
    }];
    assert!(applying(&tree.root, &parent, &batch).is_err());
}

/// Kept by one tidy with others: the first of them, and how many.
fn together(first: u64, count: u64) -> Delivery {
    Delivery {
        together: Some(Together { first, count }),
        ..Delivery::default()
    }
}

/// The journal entry of `sequence`, gone: its save delivered and dropped.
fn dropped(tree: &Tree, sequence: u64) {
    let folder = tree
        .root
        .join(".git")
        .join(symphony_board::folders::SERVER)
        .join(super::JOURNAL);
    std::fs::remove_file(folder.join(format!("{sequence:020}.json"))).unwrap();
}

const LEDGER: &str = "docs/history/review-ledger.md";

#[test]
fn a_tidys_page_saves_wait_with_its_ledger_save_behind_an_earlier_one() {
    let tree = repository();
    // Tidy 1's ledger and page, committed as one; a reply on page Q; tidy 2's ledger and Q.
    let first = Some(Together { first: 1, count: 2 });
    let ledger = Delivery {
        members: vec![2],
        together: first,
        ..committed(1, "c1")
    };
    entry(&tree, 1, LEDGER, "fox-1", ledger);
    let page = Delivery {
        together: first,
        ..committed(1, "c1")
    };
    entry(&tree, 2, "docs/p.md", "fox-1", page);
    entry(&tree, 3, "docs/q.md", "fox-1", Delivery::default());
    entry(&tree, 4, LEDGER, "fox-1", together(4, 2));
    entry(&tree, 5, "docs/q.md", "fox-1", together(4, 2));
    let tidy = Some(Together { first: 4, count: 2 });
    // The reply leads, and tidy 2 goes with none of it: its ledger waits behind tidy 1's, and its
    // page with it, so no branch holds Q's archived comment without the ledger that holds it.
    assert!(!behind(&tree.root, 3, "docs/q.md", None));
    assert_eq!(numbers(joining(&tree.root, 3)), Vec::<u64>::new());
    assert!(behind(&tree.root, 4, LEDGER, tidy));
    assert!(behind(&tree.root, 5, "docs/q.md", tidy));
    // Tidy 1 delivered, the reply committed: Q waits for the reply, so the ledger does too.
    dropped(&tree, 1);
    dropped(&tree, 2);
    entry(&tree, 3, "docs/q.md", "fox-1", committed(3, "c3"));
    assert!(behind(&tree.root, 4, LEDGER, tidy));
    // The reply delivered: tidy 2 goes, led by its ledger, its page with it.
    dropped(&tree, 3);
    assert!(!behind(&tree.root, 4, LEDGER, tidy));
    assert!(
        behind(&tree.root, 5, "docs/q.md", tidy),
        "led by its first alone"
    );
    assert_eq!(numbers(joining(&tree.root, 4)), [5]);
}

#[test]
fn saves_kept_together_go_only_once_each_of_them_is_kept() {
    let tree = repository();
    // The first is written last: until it is, the others are inert, and nothing waits for them.
    entry(&tree, 2, LEDGER, "fox-1", together(1, 2));
    let tidy = Some(Together { first: 1, count: 2 });
    let entries = super::entries(&tree.root);
    let ledger = entries[0].1.delivery.clone().unwrap();
    assert!(inert(&entries, &ledger));
    assert!(!going(&entries, &ledger));
    assert!(
        behind(&tree.root, 2, LEDGER, tidy),
        "never led by any but its first"
    );
    entry(&tree, 3, LEDGER, "fox-1", Delivery::default());
    assert!(!behind(&tree.root, 3, LEDGER, None));
    // Once the first is written they go together, and are waited for.
    entry(&tree, 1, "docs/s.md", "fox-1", together(1, 2));
    assert!(!behind(&tree.root, 1, "docs/s.md", tidy));
    assert!(behind(&tree.root, 3, LEDGER, None));
    assert_eq!(numbers(joining(&tree.root, 1)), [2, 3]);
}

#[test]
fn a_tidy_that_cannot_be_committed_holds_back_no_later_save_of_its_files() {
    let tree = repository();
    // Its first conflicts with main: the tidy goes nowhere, as a save alone that could not be
    // committed does.
    let held = Delivery {
        problem: "the verdict on docs/DECISIONS.md conflicts with main".into(),
        ..together(1, 3)
    };
    entry(&tree, 1, "docs/DECISIONS.md", "fox-1", held);
    entry(&tree, 2, LEDGER, "fox-1", together(1, 3));
    entry(&tree, 3, "docs/p.md", "fox-1", together(1, 3));
    // The owner's save of its page, then a later tidy, its ledger first.
    entry(&tree, 4, "docs/p.md", "", Delivery::default());
    entry(&tree, 5, LEDGER, "fox-1", together(5, 2));
    entry(&tree, 6, "docs/q.md", "fox-1", together(5, 2));
    assert!(!behind(&tree.root, 4, "docs/p.md", None));
    let later = Some(Together { first: 5, count: 2 });
    assert!(!behind(&tree.root, 5, LEDGER, later));
    assert_eq!(numbers(joining(&tree.root, 5)), [6]);
    // Each of its saves says why.
    let first = Together { first: 1, count: 3 };
    assert_eq!(
        stopped(&super::entries(&tree.root), first).as_deref(),
        Some(
            "kept with the save of docs/DECISIONS.md its tidy kept first, which is held: the \
             verdict on docs/DECISIONS.md conflicts with main"
        )
    );
    // Closed with no delivery, it holds back nothing either.
    let closed = Delivery {
        closed: true,
        ..together(1, 3)
    };
    entry(&tree, 1, "docs/DECISIONS.md", "fox-1", closed);
    assert!(!behind(&tree.root, 4, "docs/p.md", None));
    assert!(
        stopped(&super::entries(&tree.root), first)
            .is_some_and(|why| why.ends_with("whose task closed with no delivery"))
    );
}

#[test]
fn a_tidy_no_longer_whole_once_kept_goes_nowhere_and_holds_back_nothing() {
    let tree = repository();
    // Its ledger's save was dropped, covered by another delivery: its first goes without it
    // never, and is waited for no longer.
    entry(&tree, 1, "docs/p.md", "fox-1", together(1, 2));
    entry(&tree, 3, "docs/p.md", "", Delivery::default());
    let tidy = Together { first: 1, count: 2 };
    assert_eq!(
        stopped(&super::entries(&tree.root), tidy).as_deref(),
        Some("1 of the 2 saves its tidy kept are left, and none goes without the others")
    );
    assert!(!behind(&tree.root, 3, "docs/p.md", None));
    // Committed, each save of it stands for itself.
    entry(
        &tree,
        1,
        "docs/p.md",
        "fox-1",
        Delivery {
            together: Some(tidy),
            ..committed(1, "c1")
        },
    );
    assert_eq!(stopped(&super::entries(&tree.root), tidy), None);
    assert!(behind(&tree.root, 3, "docs/p.md", None));
}

#[test]
fn a_tidy_that_could_not_keep_its_first_save_shows_nothing() {
    let tree = repository();
    let ledger = crate::commit::Edit {
        rel: LEDGER.into(),
        base: "text".into(),
        new: "text, archived".into(),
    };
    let saved = QueuedPublication {
        edit: ledger,
        publication: None,
        dirty_retry: false,
        dirty_reported: false,
        delivery: Some(Delivery {
            by: "fox-1".into(),
            ..together(1, 2)
        }),
    };
    let folder = tree
        .root
        .join(".git")
        .join(symphony_board::folders::SERVER)
        .join(super::JOURNAL);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{:020}.json", 2)),
        serde_json::to_string(&saved).unwrap(),
    )
    .unwrap();
    // A save of the same page kept alone is shown; the inert one never is.
    let alone = QueuedPublication {
        edit: crate::commit::Edit {
            rel: LEDGER.into(),
            base: "text".into(),
            new: "a text".into(),
        },
        publication: None,
        dirty_retry: false,
        dirty_reported: false,
        delivery: Some(Delivery::default()),
    };
    std::fs::write(
        folder.join(format!("{:020}.json", 3)),
        serde_json::to_string(&alone).unwrap(),
    )
    .unwrap();
    assert_eq!(shown(&tree.root, LEDGER, "text".into()), "a text");
    assert!(
        stopped(&super::entries(&tree.root), Together { first: 1, count: 2 })
            .is_some_and(|why| why.contains("could not keep the first of its saves"))
    );
}

#[test]
fn another_tidys_saves_go_with_a_batch_all_of_them_or_none() {
    let tree = repository();
    entry(&tree, 1, "docs/r.md", "fox-1", Delivery::default());
    entry(&tree, 2, LEDGER, "fox-1", together(2, 2));
    entry(&tree, 3, "docs/p.md", "fox-1", together(2, 2));
    assert_eq!(numbers(joining(&tree.root, 1)), [2, 3]);
    // One of them held back holds back both, and every later save of either file.
    entry(&tree, 1, "docs/p.md", "", committed(1, "c1"));
    entry(&tree, 4, "docs/r.md", "fox-1", Delivery::default());
    entry(&tree, 5, LEDGER, "fox-1", Delivery::default());
    assert_eq!(numbers(joining(&tree.root, 4)), Vec::<u64>::new());
}

#[test]
fn a_tidys_saves_are_left_out_together_when_one_no_longer_applies() {
    let tree = repository();
    tree.write("docs/a.md", "one\n");
    tree.write("docs/b.md", "x\n");
    tree.write("docs/c.md", "c\n");
    git(&tree, &["add", "docs"]);
    git(&tree, &["commit", "-qm", "docs"]);
    let parent = git(&tree, &["rev-parse", "HEAD"]);
    let first = edit("docs/b.md", "x\n", "x\ny\n");
    let ledger = edit("docs/c.md", "c\n", "c\nentry\n");
    let stale = edit("docs/a.md", "old\n", "new\n");
    let after = edit("docs/c.md", "c\nentry\n", "c\nentry\nmore\n");
    let tidy = Some(Together { first: 2, count: 2 });
    let batch = [
        (1, &first, None),
        (2, &ledger, tidy),
        (3, &stale, tidy),
        (4, &after, None),
    ]
    .map(|(sequence, edit, together)| Batched {
        sequence,
        edit,
        by: "fox-1",
        together,
    });
    // Its ledger applies, and goes nowhere without the page that no longer does; the save
    // written after that ledger waits with it.
    let (edits, members) = applying(&tree.root, &parent, &batch).unwrap();
    assert!(members.is_empty(), "{members:?}");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].rel, "docs/b.md");
    // Led by such a tidy, the batch fails whole.
    assert!(applying(&tree.root, &parent, &batch[1..]).is_err());
    // Its saves are one wherever they stand: another save between them changes nothing.
    let apart = [
        (1, &first, None),
        (2, &ledger, tidy),
        (4, &after, None),
        (3, &stale, tidy),
    ]
    .map(|(sequence, edit, together)| Batched {
        sequence,
        edit,
        by: "fox-1",
        together,
    });
    let (edits, members) = applying(&tree.root, &parent, &apart).unwrap();
    assert!(members.is_empty(), "{members:?}");
    assert_eq!(edits.len(), 1);
}

#[test]
fn an_entry_names_the_saves_kept_with_it_and_nothing_alone() {
    let lone = serde_json::to_string(&Delivery::default()).unwrap();
    assert!(!lone.contains("together"), "{lone}");
    let written = serde_json::to_string(&together(4, 3)).unwrap();
    assert!(
        written.contains(r#""together":{"first":4,"count":3}"#),
        "{written}"
    );
    assert_eq!(
        serde_json::from_str::<Delivery>(&written).unwrap(),
        together(4, 3)
    );
}
