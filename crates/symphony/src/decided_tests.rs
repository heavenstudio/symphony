//! What the owner's lines under a card say of a block on it, how a line that decides nothing is
//! told, and when a held task's thread is read again.

use super::*;

const MAIN: &str = "docs/DECISIONS.md";
const BRANCH: &str = "@app~x/docs/DECISIONS.md";

/// The cards of a register whose card `layout` carries `lines` under it.
fn cards(lines: &str) -> Vec<Decision> {
    let text = format!(
        "# Decisions\n\n<!-- register:start -->\n\
         - [ ] **layout** — Wider or narrower?\n{lines}\
         - [ ] **lamp** — Lamp A or B?\n  [2026-11-20 03:00] owner · approved — A\n\
         <!-- register:end -->\n"
    );
    let lines: Vec<&str> = text.lines().collect();
    crate::register::parse_decisions(&lines)
}

fn at(stamp: &str) -> f64 {
    crate::verdicts::epoch(stamp).unwrap()
}

fn line(stamp: &str, verdict: Option<&str>, text: &str) -> OwnerLine {
    OwnerLine {
        by: "owner".into(),
        page: MAIN.into(),
        stamp: stamp.into(),
        at: at(stamp),
        verdict: verdict.map(str::to_string),
        text: text.into(),
    }
}

#[test]
fn said_keeps_the_owners_dated_lines_of_every_copy_once_oldest_first() {
    let main = cards(
        "  [2026-11-20 04:10] agent — Posted for the board task `app/x`.\n\
         \x20 [2026-11-20 04:20] owner · approved — wider\n",
    );
    // The branch's copy carries main's line and one of its own, older.
    let branch = cards(
        "  [2026-11-20 04:20] owner · approved — wider\n\
         \x20 [2026-11-20 04:15] owner — which screens?\n",
    );
    let read = said(
        main.iter()
            .map(|card| (MAIN, card))
            .chain(branch.iter().map(|card| (BRANCH, card))),
    );
    let layout = &read["layout"];
    assert_eq!(
        layout
            .iter()
            .map(|l| (
                l.page.as_str(),
                l.stamp.as_str(),
                l.verdict.as_deref(),
                l.text.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            (BRANCH, "2026-11-20 04:15", None, "which screens?"),
            (MAIN, "2026-11-20 04:20", Some("approved"), "wider"),
        ]
    );
    assert_eq!(layout[1].at, at("2026-11-20 04:20"));
    assert_eq!(
        read["lamp"].len(),
        1,
        "one line, though both copies carry it"
    );
    // An agent's line is none of the owner's, and a card nobody answered says nothing.
    let quiet = said(
        cards("  [2026-11-20 04:10] agent — Posted.\n")
            .iter()
            .map(|c| (MAIN, c)),
    );
    assert!(!quiet.contains_key("layout"));
}

#[test]
fn an_owners_line_with_no_time_it_reads_as_is_left_out() {
    let undated = cards(
        "  [2026-13-40 04:10] owner · approved — a month that is none\n  \
         [2026-11-20 04:20] owner · rejected — no\n",
    );
    let read = said(undated.iter().map(|card| (MAIN, card)));
    let stamps: Vec<&str> = read["layout"].iter().map(|l| l.stamp.as_str()).collect();
    assert_eq!(stamps, ["2026-11-20 04:20"]);
}

#[test]
fn a_verdict_since_the_block_decides_the_card_in_its_word() {
    let block = at("2026-11-20 04:00") + 30.0;
    for (verdict, word) in [
        ("approved", "approve"),
        ("rejected", "reject"),
        ("dismissed", "dismiss"),
    ] {
        let lines = [line("2026-11-20 04:20", Some(verdict), "")];
        assert_eq!(
            super::word(&lines, block),
            Word::Decided(word, "owner"),
            "{verdict}"
        );
    }
    // In the block's own minute too: the owner's stamp says the minute alone.
    let lines = [line("2026-11-20 04:00", Some("approved"), "")];
    assert_eq!(
        super::word(&lines, block),
        Word::Decided("approve", "owner")
    );
}

#[test]
fn a_verdict_from_before_the_block_decides_nothing() {
    let lines = [
        line("2026-11-20 03:00", Some("rejected"), "too wide"),
        line("2026-11-20 03:30", None, "try again"),
    ];
    assert_eq!(word(&lines, at("2026-11-20 04:00")), Word::Said(Vec::new()));
}

#[test]
fn his_newest_verdict_is_the_cards_and_a_line_without_one_leaves_it() {
    let block = at("2026-11-20 04:00");
    // Approved, then a word more: still approved.
    let lines = [
        line("2026-11-20 04:10", Some("approved"), "wider"),
        line("2026-11-20 04:12", None, "and taller"),
    ];
    assert_eq!(word(&lines, block), Word::Decided("approve", "owner"));
    // Approved, then set back to pending: undecided, and only the pending line is told.
    let lines = [
        line("2026-11-20 04:10", Some("approved"), "wider"),
        line("2026-11-20 04:12", Some("pending"), "on second thought"),
    ];
    assert_eq!(word(&lines, block), Word::Said(vec![&lines[1]]));
    // A question, then a rejection: decided.
    let lines = [
        line("2026-11-20 04:10", Some("question"), "why?"),
        line("2026-11-20 04:12", Some("rejected"), "no"),
    ];
    assert_eq!(word(&lines, block), Word::Decided("reject", "owner"));
}

#[test]
fn lines_that_decide_nothing_are_each_told_and_an_empty_one_is_not() {
    let block = at("2026-11-20 04:00");
    let lines = [
        line("2026-11-20 04:10", None, "which screens?"),
        line("2026-11-20 04:11", Some("question"), ""),
        line("2026-11-20 04:12", None, ""),
    ];
    assert_eq!(word(&lines, block), Word::Said(vec![&lines[0], &lines[1]]));
    assert_eq!(word(&[], block), Word::Said(Vec::new()));
}

#[test]
fn a_line_is_told_with_its_card_stamp_and_page_under_a_key_of_what_it_says() {
    let said = line("2026-11-20 04:10", None, "which screens?");
    let (text, key) = told("layout", &said, None);
    assert_eq!(
        text,
        "the owner on card layout [2026-11-20 04:10] (docs/DECISIONS.md#layout), deciding \
         nothing, so it stays blocked on decision:layout: which screens?"
    );
    assert!(key.starts_with("decided:layout:2026-11-20 04:10:"), "{key}");
    assert_eq!(
        told("layout", &said, None).1,
        key,
        "the same line, the same key"
    );
    let other = line("2026-11-20 04:10", None, "which pages?");
    assert_ne!(told("layout", &other, None).1, key);
    let asked = line("2026-11-20 04:10", Some("question"), "which screens?");
    assert_ne!(
        told("layout", &asked, None).1,
        key,
        "a verdict word changes what it says"
    );
    assert!(
        told("layout", &asked, None)
            .0
            .ends_with("decision:layout: question — which screens?")
    );
    assert!(
        told(
            "layout",
            &line("2026-11-20 04:10", Some("pending"), ""),
            None
        )
        .0
        .ends_with(": pending")
    );
    // A long line is cut: the card has the rest.
    let long = line("2026-11-20 04:10", None, &"é".repeat(400));
    let (text, _) = told("layout", &long, None);
    assert!(
        text.ends_with(&format!(": {}…", "é".repeat(WORDS))),
        "{text}"
    );
    assert!(!text.contains(&"é".repeat(WORDS + 1)));
}

#[test]
fn the_whys_name_the_card_and_verdict_or_the_commit_approved() {
    assert_eq!(
        decided_why("layout", "reject", "owner", None),
        "the owner decided layout (reject)"
    );
    let tip = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    assert_eq!(
        approved_why("owner", &[], tip, None),
        "the owner approved 1a2b3c4"
    );
    assert_eq!(
        approved_why("owner", &[], "abc", None),
        "the owner approved abc"
    );
    assert_eq!(
        approved_why("owner", &["layout".into()], tip, None),
        "the owner approved card layout, at 1a2b3c4"
    );
    assert_eq!(
        approved_why("ana", &["layout".into(), "colour".into()], tip, None),
        "ana approved cards layout, colour, at 1a2b3c4"
    );
}

/// [`line`] of `by`'s, a person a board named.
fn theirs(by: &str, stamp: &str, verdict: Option<&str>, text: &str) -> OwnerLine {
    OwnerLine {
        by: by.into(),
        ..line(stamp, verdict, text)
    }
}

#[test]
fn a_persons_lines_are_kept_by_name_and_decide_and_are_told_as_theirs() {
    let main = cards(
        "  [2026-11-20 04:10] ana · approved — wider\n  [2026-11-20 04:10] owner · approved — wider\n",
    );
    let said = said(main.iter().map(|card| (MAIN, card)));
    let by: Vec<&str> = said["layout"].iter().map(|line| line.by.as_str()).collect();
    assert_eq!(
        by,
        ["ana", "owner"],
        "two people's same words in one minute are two lines"
    );
    let block = at("2026-11-20 04:00");
    let lines = [theirs("ana", "2026-11-20 04:10", Some("approved"), "")];
    assert_eq!(word(&lines, block), Word::Decided("approve", "ana"));
    assert_eq!(
        decided_why("layout", "approve", "ana", None),
        "ana decided layout (approve)"
    );
    // Told as hers, under a key of her own, while the owner's key reads as it always has.
    let hers = theirs("ana", "2026-11-20 04:10", None, "which screens?");
    let mine = line("2026-11-20 04:10", None, "which screens?");
    let (text, key) = told("layout", &hers, None);
    assert!(
        text.starts_with("ana on card layout [2026-11-20 04:10]"),
        "{text}"
    );
    assert_ne!(key, told("layout", &mine, None).1);
    assert!(
        told("layout", &mine, None)
            .0
            .starts_with("the owner on card layout")
    );
}

fn post(id: i64, sender: &str, kind: &str, text: &str) -> Message {
    Message {
        id,
        slug: "app/x".into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts: id as f64,
    }
}

/// The owner's approve on the thread, with no checkout to read cards in: it counts where it names
/// the tip, the why naming that commit. The cards' own are the server's tests
/// (`apps/symphony-server/src/live/decided_tests.rs`).
#[test]
fn the_owners_approve_counts_where_it_names_the_tip() {
    let tip = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    let other = "9f8e7d6c5b4a39281706f5e4d3c2b1a098765432";
    let nowhere = Path::new("/nowhere");
    let approves = |thread: &[Message]| approves(thread, tip, nowhere, &[], None);
    assert_eq!(
        approves(&[post(1, "owner", "msg", &format!("approve {tip}"))]).as_deref(),
        Some("the owner approved 1a2b3c4")
    );
    // A decider's approve counts while they decide, and is said as theirs.
    let hers = [post(1, "ana", "msg", &format!("approve {tip}"))];
    let deciders = ["ana".to_string()];
    assert_eq!(
        super::approves(&hers, tip, nowhere, &deciders, None).as_deref(),
        Some("ana approved 1a2b3c4")
    );
    assert_eq!(approves(&hers), None, "no decide, no approve");
    for thread in [
        vec![post(1, "owner", "msg", &format!("approve {other}"))],
        vec![post(1, "orch", "msg", &format!("approve {tip}"))],
        vec![post(1, "owner", "system", &format!("approve {tip}"))],
        vec![post(1, "owner", "msg", "looks good")],
        Vec::new(),
    ] {
        assert_eq!(approves(&thread), None, "{thread:?}");
    }
}

/// The person at the machine, once named, ends a block by an approve under their name as under
/// `owner`, and every line telling of their word names them by how they are shown; the key a line
/// is told once under is the one it had.
#[test]
fn the_machines_persons_word_is_told_and_counted_by_how_they_are_shown() {
    let stefano = MachinePerson {
        name: "stefano".into(),
        shown: "Stefano".into(),
    };
    let machine = Some(&stefano);
    let tip = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    let nowhere = Path::new("/nowhere");
    let deciders = symphony_board::people::with_machine(Vec::new(), machine);
    for by in ["owner", "stefano"] {
        let theirs = [post(1, by, "msg", &format!("approve {tip}"))];
        assert_eq!(
            approves(&theirs, tip, nowhere, &deciders, machine).as_deref(),
            Some("Stefano approved 1a2b3c4"),
            "{by}"
        );
        assert_eq!(
            decided_why("layout", "approve", by, machine),
            "Stefano decided layout (approve)"
        );
    }
    let mine = line("2026-11-20 04:10", None, "which screens?");
    let (text, key) = told("layout", &mine, machine);
    assert!(
        text.starts_with("Stefano on card layout [2026-11-20 04:10]"),
        "{text}"
    );
    assert_eq!(
        key,
        told("layout", &mine, None).1,
        "told once, named or not"
    );
    assert_eq!(
        approved_why("ana", &[], tip, machine),
        "ana approved 1a2b3c4",
        "anyone else by their short name"
    );
}

#[test]
fn a_held_tasks_thread_is_read_when_the_board_wrote_a_line_or_a_while_went_by() {
    let mut kept = Kept::default();
    let now = 1_800_000_000.0;
    assert!(kept.due("app/x", 7, now), "never read");
    assert!(!kept.due("app/x", 7, now + 1.0), "nothing written since");
    assert!(kept.due("app/x", 8, now + 2.0), "a line written since");
    assert!(!kept.due("app/x", 8, now + 2.0 + RESCAN_SECONDS - 1.0));
    assert!(
        kept.due("app/x", 8, now + 2.0 + RESCAN_SECONDS),
        "a while went by"
    );
    assert!(kept.due("app/x", 8, now), "the clock went back");
    assert!(kept.due("app/y", 8, now), "each task its own");
}
