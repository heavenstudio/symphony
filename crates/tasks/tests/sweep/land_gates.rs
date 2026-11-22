//! The land by a task's gate decisions (docs/workflow/TASKS.md, "Gate decisions"), in scratch repositories
//! on a board of their own: a waiver lifts its stamp at the tip it was given for and at no later
//! one; a raise lands only carried at the number decided, its line saying so, and one the branch
//! never made, or a limit the branch lowered below main's, lands saying it raised nothing.

use super::stamps::{assess, green, in_review, landed_by, told};
use super::*;
use symphony_board::gates::{Decision, Kind};
use symphony_board::stamps::Catalogue;

pub(super) const BUDGET: &str = "tools/size-budget.toml";
pub(super) const GATE: &str = "tools/size-budget.toml:symphony";

/// A commit on `slug`'s worktree (as `in_review` made it) writing `file` as `text`: the new tip.
fn commit(r: &Repo, slug: &str, file: &str, text: &str) -> String {
    let wt = r.dir.join(slug.replace('/', "-"));
    write(&wt, file, text);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", &format!("more on {slug}")]);
    head(&wt, "HEAD")
}

pub(super) fn decide(r: &Repo, slug: &str, decision: Decision) {
    (r.board.store)
        .decide_gate(slug, &Catalogue::symphony(), &decision)
        .unwrap();
}

fn waiver(by: &str, tip: &str) -> Decision {
    Decision {
        gate: "pictures".into(),
        kind: Kind::Waive,
        from: None,
        to: None,
        tip: Some(tip.into()),
        by: by.into(),
        why: "the row is measured, never seen".into(),
        at: 0.0,
    }
}

pub(super) fn raise(by: &str, from: u64, to: u64) -> Decision {
    Decision {
        gate: GATE.into(),
        kind: Kind::Raise,
        from: Some(from),
        to: Some(to),
        tip: None,
        by: by.into(),
        why: "over after the cut".into(),
        at: 0.0,
    }
}

/// A waiver of `pictures` given for the tip lifts it there alone: at a later commit the land
/// refuses, naming the waiver as given for another tip, until one is given for that tip too.
#[test]
fn a_waiver_lifts_its_stamp_at_the_tip_it_was_given_for_and_never_at_a_later_one() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "ui/rows";
    let (staff, first) = in_review(&r, slug, &[("ui/rows.gd", "extends Node\n")]);
    decide(&r, slug, waiver(&staff.orch, &first));
    let second = commit(&r, slug, "ui/rows.gd", "extends Node # a later commit\n");
    assess(&r, slug, &staff.assessor, &["tests", "pictures"], &second);
    green(&r, slug, &second);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "{slug} lacks at its tip {} what it needs: pictures (No picture posted at",
            &second[..7]
        )),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "; its waiver by {} was for {}, not the tip {}: a waiver is for one tip alone)",
            staff.orch,
            &first[..7],
            &second[..7]
        )),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before, "nothing landed");
    assert_eq!(told(&r, slug).1.as_deref(), Some("@executor"));
    // Given for this tip, it lands: its line says the stamp waived, by whom and why.
    decide(&r, slug, waiver(&staff.orch, &second));
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(
        said.contains(&format!(
            "land {slug}: it needs tests at {} (its assessment by {}); tests: ",
            &second[..7],
            staff.assessor
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "; pictures waived by the gate decision of {}: the row is measured, never seen\n",
            staff.orch
        )),
        "{said}"
    );
    result.unwrap();
    assert_eq!(head(&r.root, "main^2"), second);
}

/// A raise lands only where the tip sets its limit to the number decided: a branch at another
/// number is refused to its executor, and once carried, the landing's line names the decision. A
/// raise the branch never made lands, saying the tip holds what main does.
#[test]
fn a_raise_lands_carried_at_the_number_decided_and_a_raise_left_unmade_changes_nothing() {
    let r = Repo::new();
    manifest(&r, "");
    write(&r.root, BUDGET, "[binaries]\nsymphony = 100_000\n");
    git(&r.root, &["add", BUDGET]);
    git(&r.root, &["commit", "-qm", "the budget"]);
    let slug = "tools/heavier";
    let (staff, over) = in_review(&r, slug, &[(BUDGET, "[binaries]\nsymphony = 120_000\n")]);
    decide(&r, slug, raise(&staff.orch, 100_000, 110_000));
    assess(&r, slug, &staff.assessor, &["tests"], &over);
    green(&r, slug, &over);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let line = result.unwrap_err();
    assert_eq!(
        line,
        format!(
            "{GATE} raised 100,000 -> 110,000 by {}, but the tip {} sets it to 120,000, which no \
             decision gave: a raise lands at the number decided",
            staff.orch,
            &over[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), before, "nothing landed");
    let (text, to) = told(&r, slug);
    assert!(
        text.ends_with(
            "ask the task's orchestrator for a decision of the number it holds; then land it again"
        ),
        "{text}"
    );
    assert_eq!(to.as_deref(), Some("@executor"));
    // Carried at the number decided, it lands, its line naming the decision.
    let carried = commit(&r, slug, BUDGET, "[binaries]\nsymphony = 110_000\n");
    assess(&r, slug, &staff.assessor, &["tests"], &carried);
    green(&r, slug, &carried);
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(
        said.contains(&format!(
            "land {slug}: its gate decisions: {GATE} raised 100,000 -> 110,000 by {}, carried at {}\n",
            staff.orch,
            &carried[..7]
        )),
        "{said}"
    );
    result.unwrap();
    assert_eq!(head(&r.root, "main^2"), carried);
    // A raise decided and never made: the tip holds main's number, and it lands saying so.
    let slug = "tools/lighter";
    let (staff, tip) = in_review(&r, slug, &[("sim/light.gd", "extends Node\n")]);
    decide(&r, slug, raise(&staff.orch, 110_000, 115_000));
    assess(&r, slug, &staff.assessor, &["tests"], &tip);
    green(&r, slug, &tip);
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(
        said.contains(&format!(
            "{GATE} raised 110,000 -> 115,000 by {}, not carried: the tip {} holds 110,000 as the \
             default branch does",
            staff.orch,
            &tip[..7]
        )),
        "{said}"
    );
    result.unwrap();
}

/// A raise decided, then a further cut: the tip lowers the limit below main's, which needs no
/// decision, so it lands saying the raise was not carried; a number above main's that is not the
/// one decided is still refused.
#[test]
fn a_tip_that_lowers_a_raised_limit_below_mains_lands_and_one_above_it_undecided_is_refused() {
    let r = Repo::new();
    manifest(&r, "");
    write(&r.root, BUDGET, "[binaries]\nsymphony = 100_000\n");
    git(&r.root, &["add", BUDGET]);
    git(&r.root, &["commit", "-qm", "the budget"]);
    let slug = "tools/cut";
    let (staff, over) = in_review(&r, slug, &[(BUDGET, "[binaries]\nsymphony = 105_000\n")]);
    decide(&r, slug, raise(&staff.orch, 100_000, 110_000));
    assess(&r, slug, &staff.assessor, &["tests"], &over);
    green(&r, slug, &over);
    let (_, result) = landed_by(&r, slug, &staff.orch);
    assert!(result.unwrap_err().ends_with(&format!(
        "but the tip {} sets it to 105,000, which no decision gave: a raise lands at the \
             number decided",
        &over[..7]
    )));
    // Cut further, the tip lowers it below main's: it lands, saying so.
    let cut = commit(&r, slug, BUDGET, "[binaries]\nsymphony = 90_000\n");
    assess(&r, slug, &staff.assessor, &["tests"], &cut);
    green(&r, slug, &cut);
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(
        said.contains(&format!(
            "land {slug}: its gate decisions: {GATE} raised 100,000 -> 110,000 by {}, not \
             carried: the tip {} lowers it to 90,000, below the default branch's 100,000, which \
             needs no decision\n",
            staff.orch,
            &cut[..7]
        )),
        "{said}"
    );
    result.unwrap();
    assert_eq!(head(&r.root, "main^2"), cut);
}
