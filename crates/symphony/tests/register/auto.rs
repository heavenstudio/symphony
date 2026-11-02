//! AutoApprovedTest: `[a]`, the triage tool approved a picture on the owner's behalf, for the owner
//! to spot-check.

use symphony::board::{render, scan};
use symphony::register::{Group, State, Who, resolve, sort_register};

use crate::common::{Tree, matches};
use crate::{card, entry};

const CARD: [&str; 3] = [
    "- [a] **pic** — A hull. *Shows:* X · ship",
    "  [2026-09-27 10:00] agent — A ![X ship A](/a.png) B ![X ship B](/b.png)",
    "  [2026-09-27 11:00] agent · auto-approved — B: clear mounts, bold outline",
];

#[test]
fn the_marker_reads_as_auto_approved() {
    assert_eq!(State::from_mark('a'), Some(State::AutoApproved));
    assert_eq!(card(&CARD).state, State::AutoApproved);
}

#[test]
fn the_auto_line_is_an_agent_entry_with_its_verdict() {
    let last = card(&CARD).entries().pop().unwrap();
    assert_eq!(
        last,
        entry(
            Some("2026-09-27 11:00"),
            Who::Agent,
            Some("auto-approved"),
            "B: clear mounts, bold outline"
        )
    );
}

#[test]
fn it_sits_in_its_own_group_and_is_not_open() {
    let found = card(&CARD);
    assert_eq!(found.group(), Group::Auto);
    assert!(!found.open());
    assert_eq!(
        (Group::Auto.key(), Group::Auto.title()),
        ("auto", "Auto-approved: spot-check")
    );
}

#[test]
fn tidy_does_not_resolve_it() {
    let text = CARD.join("\n") + "\n";
    assert_eq!(resolve(&text, "2026-09-27 12:00"), (text.clone(), vec![]));
}

#[test]
fn the_register_groups_it_apart_from_decide_and_resolved() {
    let text = format!(
        "<!-- register:start -->\n### T\n\n- [ ] **new** — 1\n  [2026-09-27 09:00] agent — x\n{}\n- [x] **done** — 2\n<!-- register:end -->\n",
        CARD.join("\n")
    );
    let out = sort_register(&text).unwrap();
    let heads: Vec<&str> = out.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(
        heads,
        vec![
            "## Needs your decision",
            "## Auto-approved: spot-check",
            "## Resolved"
        ]
    );
    assert!(out.find("## Auto-approved: spot-check").unwrap() < out.find("- [a] **pic**").unwrap());
    assert!(out.find("- [a] **pic**").unwrap() < out.find("## Resolved").unwrap());
}

#[test]
fn an_owner_comment_on_it_is_work_for_an_agent() {
    let mut lines = CARD.to_vec();
    lines.push("  [2026-09-27 12:00] owner — the turrets are too small");
    let found = card(&lines);
    assert!(found.open());
    assert_eq!(found.group(), Group::Agent);
}

#[test]
fn an_owner_verdict_overrides_it() {
    let head = CARD[0].replace("[a]", "[-]");
    let found = card(&[
        &head,
        CARD[1],
        CARD[2],
        "  [2026-09-27 12:00] owner · rejected — no",
    ]);
    assert_eq!(found.state, State::Rejected);
    assert_eq!(found.decided_at().as_deref(), Some("2026-09-27 12:00"));
}

#[test]
fn the_board_counts_it() {
    let tree = Tree::new();
    tree.write("docs/x.md", &(CARD.join("\n") + "\n"));
    let out = render(&scan(&tree.root).unwrap(), false);
    assert!(matches(r"auto-approved\s+1", &out), "{out}");
    assert!(matches(r"auto-approved: spot-check\s+1", &out), "{out}");
}
