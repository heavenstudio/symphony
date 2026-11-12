use super::*;
use crate::register::parse_decisions;
use crate::text::splitlines;

/// The one card of `text`.
fn card(text: &str) -> Decision {
    let mut cards = parse_decisions(&splitlines(text));
    assert_eq!(cards.len(), 1, "{text}");
    cards.remove(0)
}

/// The dated lines under `card`, each as `who verdict text`.
fn said(card: &Decision) -> Vec<String> {
    card.entries()
        .iter()
        .map(|e| {
            format!(
                "{} {} {}{}",
                e.ts.as_deref().unwrap_or("-"),
                e.who.as_str(),
                e.verdict
                    .as_deref()
                    .map_or(String::new(), |v| format!("{v} ")),
                e.text
            )
        })
        .collect()
}

const POSTED: &str = "- [ ] **c** — Pick one.\n  [2026-11-11 10:00] agent — Posted.\n";

#[test]
fn chunks_keep_continuations_blank_lines_and_hidden_lines_with_their_line() {
    let lines: Vec<String> = [
        "  [2026-11-11 10:00] agent — Posted.",
        "",
        "    **More.** A second paragraph.",
        "  [2026-11-11 10:05] owner · question — Which?",
        "  <!-- review:unseen-owner aa:1 bb:1 -->",
        "  ↳ An undated answer.",
        "  [2026-11-11 10:07] agent · question — A or B?",
    ]
    .map(String::from)
    .to_vec();
    let found = chunks(&lines);
    assert_eq!(found.len(), 4);
    assert_eq!(found[0].lines.len(), 3);
    assert_eq!(found[0].who, Who::Agent);
    assert_eq!(
        found[1].lines.len(),
        2,
        "the hidden line stays with the owner's"
    );
    assert_eq!(found[1].sets, Some(State::Question));
    assert_eq!((found[2].ts.as_deref(), found[2].who), (None, Who::Agent));
    assert_eq!(found[3].sets, None, "an agent's question sets no mark");
    assert_eq!(
        found[0].key(),
        [
            "  [2026-11-11 10:00] agent — Posted.",
            "",
            "    **More.** A second paragraph."
        ]
    );
}

#[test]
fn a_branch_answer_after_mains_question_reads_as_one_answered_card() {
    let main = card(
        "- [?] **c** — Pick one.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 17:42] owner · question — Why B?\n",
    );
    let branch = card(&format!(
        "{POSTED}  [2026-11-11 17:59] agent — Because B is faster.\n"
    ));
    let one = one_card(&main, &branch);
    assert_eq!(
        said(&one),
        [
            "2026-11-11 10:00 agent Posted.",
            "2026-11-11 17:42 owner question Why B?",
            "2026-11-11 17:59 agent Because B is faster."
        ]
    );
    assert_eq!(one.state, State::Question, "main's question set the mark");
    let plugins = crate::plugins::Plugins::default();
    assert_eq!(one.group(&plugins), crate::Group::Confirm);
    assert!(!one.open(&plugins), "the agent answered it");
}

#[test]
fn mains_resolution_after_a_stale_branch_copy_wins() {
    let main = card(
        "- [r] **c** — Pick one.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 18:00] owner · approved\n  [2026-11-11 19:00] agent · resolved\n",
    );
    let stale = card(
        "- [-] **c** — Pick one, revised.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 12:00] owner · rejected — Too dark.\n",
    );
    let one = one_card(&main, &stale);
    assert_eq!(one.state, State::Resolved);
    assert_eq!(one.text, "Pick one.", "main's line is the latest: its text");
    assert_eq!(said(&one).len(), 4, "the branch's line still shows");
    assert_eq!(
        one.group(&crate::plugins::Plugins::default()),
        crate::Group::Resolved
    );
}

#[test]
fn the_owners_later_verdict_on_a_branch_sets_the_mark_and_the_revision_its_text() {
    let main = card(POSTED);
    let branch = card(
        "- [x] **c** — Pick one, revised.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 11:00] owner · approved — Go.\n",
    );
    let one = one_card(&main, &branch);
    assert_eq!(one.state, State::Approved);
    assert_eq!(one.text, "Pick one, revised.");
    // A tie between the two copies' marking lines keeps main's mark.
    let marked = "- [x] **c** — Pick one.\n  [2026-11-11 10:00] owner · approved\n";
    let pending_copy = "- [ ] **c** — Pick one.\n  [2026-11-11 10:00] owner · approved\n  [2026-11-11 10:30] agent — Noted.\n";
    assert_eq!(
        one_card(&card(marked), &card(pending_copy)).state,
        State::Approved
    );
}

#[test]
fn lines_both_copies_hold_show_once_and_identical_copies_are_the_same() {
    let main = card(POSTED);
    let one = one_card(&main, &main);
    assert!(same(&one, &main));
    let answered = card(&format!("{POSTED}  [2026-11-11 11:00] agent — More.\n"));
    let one = one_card(&answered, &main);
    assert!(
        same(&one, &answered),
        "main already says all the branch does"
    );
    assert!(!same(&main, &answered));
}

#[test]
fn owner_lines_are_the_owners_dated_lines_no_commit_has() {
    let posted = card(POSTED);
    let worktree = card(
        "- [-] **c** — Pick one, being rewritten.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 10:30] owner · rejected — Too dark.\n  [2026-11-11 10:40] agent — A draft answer with ![A](missing.png).\n",
    );
    let owner = owner_lines(&worktree, Some(&posted));
    assert_eq!(owner.len(), 1);
    let shown = with_lines(&posted, &owner);
    assert_eq!(
        shown.state,
        State::Rejected,
        "the owner's verdict counts at once"
    );
    assert_eq!(shown.text, "Pick one.", "the agent's draft text does not");
    assert_eq!(
        said(&shown),
        [
            "2026-11-11 10:00 agent Posted.",
            "2026-11-11 10:30 owner rejected Too dark."
        ]
    );
    assert!(owner_lines(&posted, Some(&posted)).is_empty());
    assert_eq!(owner_lines(&worktree, None).len(), 1);
}

#[test]
fn owner_lines_join_mains_copy_without_its_text_or_a_newer_mark() {
    let main = card(
        "- [r] **c** — Pick one.\n  [2026-11-11 10:00] agent — Posted.\n  [2026-11-11 19:00] agent · resolved\n",
    );
    let late = card("- [x] **c** — Old text.\n  [2026-11-11 18:48] owner · approved — Ship it.\n");
    let one = with_owner_lines(&main, &late);
    assert_eq!(
        one.state,
        State::Resolved,
        "main resolved after the owner's line"
    );
    assert_eq!(one.text, "Pick one.");
    assert_eq!(
        said(&one)[1],
        "2026-11-11 18:48 owner approved Ship it.",
        "placed in time order"
    );
    let later = card("- [-] **c** — Old text.\n  [2026-11-11 20:00] owner · rejected — No.\n");
    let reopened = with_owner_lines(&main, &later);
    assert_eq!(reopened.state, State::Rejected);
    assert!(reopened.open(&crate::plugins::Plugins::default()));
}

#[test]
fn markdown_writes_the_card_back_as_the_register_reads_it() {
    let one = card(&format!(
        "{POSTED}  [2026-11-11 11:00] owner — A question.\n"
    ));
    let again = card(&markdown(&one));
    assert!(same(&again, &one));
    assert_eq!(again.line, 1);
}

#[test]
fn written_is_what_no_copy_says_the_text_and_lines_its_author_added() {
    let main = card(
        "- [?] **c** — Pick one: ![S](/artifacts/s.png)\n  [2026-11-11 10:00] agent — Posted ![P](/artifacts/p.png).\n  [2026-11-11 17:42] owner · question — Why B?\n",
    );
    let fork = card(
        "- [ ] **c** — Pick one: ![S](/artifacts/s.png)\n  [2026-11-11 10:00] agent — Posted ![P](/artifacts/p.png).\n",
    );
    let answer = "  [2026-11-11 17:59] agent — Because B ![B](/artifacts/b.png).";
    let branch = card(&format!(
        "- [ ] **c** — Pick one: ![S](/artifacts/s.png)\n  [2026-11-11 10:00] agent — Posted ![P](/artifacts/p.png).\n{answer}\n"
    ));
    let copies = [&fork, &main].map(|c| (c.text.as_str(), c.raw.as_slice()));
    let own = written(&branch, copies);
    assert_eq!(own.text, "", "the text is the fork's");
    assert_eq!(own.raw, [answer]);
    // A revised text is the author's; so is every line when no copy holds the card.
    let revised = card(
        "- [ ] **c** — Pick two.\n  [2026-11-11 10:00] agent — Posted ![P](/artifacts/p.png).\n",
    );
    let own = written(&revised, copies);
    assert_eq!((own.text.as_str(), own.raw.len()), ("Pick two.", 0));
    assert_eq!(written(&branch, []).raw.len(), 2);
}

#[test]
fn awaits_agent_while_the_owner_spoke_last() {
    let asked = card(&format!(
        "{POSTED}  [2026-11-11 11:00] owner · question — Which?\n"
    ));
    assert!(awaits_agent(&asked));
    let answered = card(&format!(
        "{POSTED}  [2026-11-11 11:00] owner · question — Which?\n  [2026-11-11 11:05] agent — This one.\n"
    ));
    assert!(!awaits_agent(&answered));
    assert!(!awaits_agent(&card(POSTED)), "the owner said nothing");
    let draft = card("- [-] **c** — Pick one.\n  [2026-11-11 11:00] owner · rejected — No.\n");
    assert!(awaits_agent(&draft), "a card only the owner spoke on");
    let posted = card(POSTED);
    assert_eq!(owner_lines_beside(&asked, [posted.raw.as_slice()]).len(), 1);
    assert!(owner_lines_beside(&asked, [asked.raw.as_slice()]).is_empty());
}
