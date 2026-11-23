//! A task's gate decisions on the real store ([`symphony_board::gates`]): its orchestrator's
//! alone, refused to a session that executed it or work under it, reviews it or gave it a verdict;
//! checked against the catalogue (`tests`, `review` and `owner` never waived) and the limit's
//! shape; every decision kept, each a line on the thread, in the board's own field.

use super::common::{Fixture, Seed, refused};
use symphony_board::NewTask;
use symphony_board::gates::{Decision, Kind};
use symphony_board::stamps::Catalogue;

const TIP: &str = "1a2b3c4d5e6f7a8b9c0d1a2b3c4d5e6f7a8b9c0d";
const GATE: &str = "tools/size-budget.toml:symphony";

fn raise(by: &str, from: u64, to: u64) -> Decision {
    Decision {
        gate: GATE.into(),
        kind: Kind::Raise,
        from: Some(from),
        to: Some(to),
        tip: None,
        by: by.into(),
        why: "3,664 bytes over after the cut".into(),
        at: 0.0,
    }
}

fn waive(by: &str, stamp: &str, tip: &str) -> Decision {
    Decision {
        gate: stamp.into(),
        kind: Kind::Waive,
        from: None,
        to: None,
        tip: Some(tip.into()),
        by: by.into(),
        why: "no page shows the change".into(),
        at: 0.0,
    }
}

/// `slug` filed and orchestrated by `orch`, claimed by `exe`.
fn worked(f: &Fixture, slug: &str, orch: &str, exe: &str) {
    f.task_with(NewTask::new(slug, "A change", orch));
    f.store.take_role(slug, orch, "orchestrator").unwrap();
    f.store.claim(slug, exe).unwrap();
}

#[test]
fn the_orchestrators_decisions_are_kept_each_a_line_and_the_newest_on_a_gate_stands() {
    let f = Fixture::new(Seed::default());
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    worked(&f, "tools/size", &orch, &exe);
    let own = Catalogue::symphony();
    assert!(f.store.get("tools/size").unwrap().gates().is_empty());
    let task = f
        .store
        .decide_gate(
            "tools/size",
            &own,
            &Decision {
                gate: format!(" /{GATE} "),
                why: "  3,664 bytes over after the cut ".into(),
                tip: Some(TIP.into()),
                ..raise(&orch, 12_390_000, 12_400_000)
            },
        )
        .unwrap();
    let first = task.gates();
    assert_eq!(first.len(), 1);
    assert!(first[0].at > 0.0, "the board's clock");
    assert_eq!(
        Decision {
            at: 0.0,
            ..first[0].clone()
        },
        raise(&orch, 12_390_000, 12_400_000),
        "the gate named as a limit names it, trimmed, a raise keeping no tip"
    );
    let last = f.store.thread("tools/size").unwrap().pop().unwrap();
    assert_eq!(
        (last.sender.as_str(), last.kind.as_str(), last.text.as_str()),
        (
            orch.as_str(),
            "system",
            format!(
                "gate {GATE} raised 12,390,000 -> 12,400,000: 3,664 bytes over after the cut; \
                     by {orch}"
            )
            .as_str()
        )
    );
    // A waiver beside it, then the raise given again at another number: both kept, the
    // newest raise standing.
    f.store
        .decide_gate(
            "tools/size",
            &own,
            &waive(&orch, "pictures", &TIP.to_uppercase()),
        )
        .unwrap();
    f.store
        .decide_gate("tools/size", &own, &raise(&orch, 12_390_000, 12_410_000))
        .unwrap();
    let task = f.store.get("tools/size").unwrap();
    let all = symphony_board::gates::of(&task.fields);
    assert_eq!(all.len(), 3, "every decision kept");
    let standing: Vec<(String, Option<u64>, Option<String>)> = (task.gates().into_iter())
        .map(|d| (d.gate, d.to, d.tip))
        .collect();
    assert_eq!(
        standing,
        [
            ("pictures".to_string(), None, Some(TIP.to_string())),
            (GATE.to_string(), Some(12_410_000), None),
        ],
        "a waiver's tip lowercased whole, the newest raise standing"
    );
    assert_eq!(
        f.said("tools/size").last().unwrap(),
        &format!(
            "gate {GATE} raised 12,390,000 -> 12,410,000: 3,664 bytes over after the cut; by \
                 {orch}"
        )
    );
    // Nothing else of the task moved.
    assert_eq!(task.status, "in_progress");
    assert!(task.holders("executor").contains(&exe.as_str()));
    // The board's own field: a plugin's field write cannot reach it.
    let field = f.store.set_field(
        "tools/size",
        "_board",
        "gates",
        &serde_json::json!("x"),
        &orch,
    );
    assert!(refused(field).ends_with("its gate decisions by its orchestrator (tasks gate)"));
    assert_eq!(f.store.get("tools/size").unwrap().gates().len(), 2);
}

#[test]
fn no_session_decides_its_own_gate_and_tests_review_and_owner_are_never_waived() {
    let f = Fixture::new(Seed::default());
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    let (rev, judged, slice, outsider) = (
        f.session("reviewer"),
        f.session("reviewer"),
        f.session("worker"),
        f.session("orchestrator"),
    );
    worked(&f, "tools/size", &orch, &exe);
    f.task_with(NewTask {
        parent: Some("tools/size".into()),
        ..NewTask::new("tools/size/slice", "A slice", &orch)
    });
    f.store.claim("tools/size/slice", &slice).unwrap();
    // One reviewer holds the role; another gave a verdict and let the role go.
    f.store.move_to("tools/size", "review", &exe).unwrap();
    f.store
        .take_role("tools/size", &judged, "reviewer")
        .unwrap();
    f.store
        .verdict("tools/size", &judged, "changes", "measure it again")
        .unwrap();
    f.store
        .release("tools/size", &judged, Some("reviewer"))
        .unwrap();
    f.store.take_role("tools/size", &rev, "reviewer").unwrap();
    let own = Catalogue::symphony();
    let before = f.said("tools/size");
    let tail = "a gate decision is its orchestrator's, never its executor's or its reviewer's, \
                    so no session decides its own gate";
    for (who, did) in [
        (&exe, "executed"),
        (&slice, "executed"),
        (&rev, "reviews"),
        (&judged, "reviews"),
    ] {
        for decision in [raise(who, 1, 2), waive(who, "pictures", TIP)] {
            assert_eq!(
                refused(f.store.decide_gate("tools/size", &own, &decision)),
                format!("{who} {did} tools/size: {tail}"),
                "{did}"
            );
        }
    }
    assert_eq!(
        refused(
            f.store
                .decide_gate("tools/size", &own, &raise(&outsider, 1, 2))
        ),
        format!(
            "{outsider} does not orchestrate tools/size: a gate decision is its \
                 orchestrator's, who records the owner's word too"
        )
    );
    assert_eq!(
        refused(
            f.store
                .decide_gate("tools/size", &own, &raise("nobody", 1, 2))
        ),
        f.no_session("nobody")
    );
    // What it decides on is refused before who decides is read.
    for never in ["tests", "review", "owner"] {
        let why = refused(
            f.store
                .decide_gate("tools/size", &own, &waive(&orch, never, TIP)),
        );
        assert!(
            why.starts_with(&format!("{never} is never waived: tests (a red")),
            "{why}"
        );
    }
    let strict =
        Catalogue::parse("[stamps.review]\n[stamps.pictures]\nwaivable = false\n").unwrap();
    assert_eq!(
        refused(
            f.store
                .decide_gate("tools/size", &strict, &waive(&orch, "pictures", TIP))
        ),
        "the catalogue does not mark pictures waivable ([stamps.pictures] waivable); the \
             catalogue marks none waivable"
    );
    for tip in ["1a2b3c4", "", "zz2b3c4d5e6f7a8b9c0d1a2b3c4d5e6f7a8b9c0d"] {
        assert_eq!(
            refused(
                f.store
                    .decide_gate("tools/size", &own, &waive(&orch, "pictures", tip))
            ),
            format!("a waiver names the tip it is for, whole: not {tip:?}")
        );
    }
    for (from, to) in [(5, 5), (5, 4)] {
        assert_eq!(
            refused(
                f.store
                    .decide_gate("tools/size", &own, &raise(&orch, from, to))
            ),
            format!(
                "a raise of {GATE} goes above {from}, its number now: {to} is not, and a \
                     limit lowered needs no decision"
            )
        );
    }
    assert_eq!(
        refused(f.store.decide_gate(
            "tools/size",
            &own,
            &Decision {
                to: None,
                ..raise(&orch, 1, 2)
            }
        )),
        "a raise names the number it raises and the number it raises it to"
    );
    assert!(
        refused(f.store.decide_gate(
            "tools/size",
            &own,
            &Decision {
                gate: "pictures".into(),
                ..raise(&orch, 1, 2)
            }
        ))
        .starts_with("a raise names its limit as PATH:NAME")
    );
    assert_eq!(
        refused(f.store.decide_gate(
            "tools/size",
            &own,
            &Decision {
                why: " ".into(),
                ..raise(&orch, 1, 2)
            }
        )),
        "a gate decision says why (--why WHY): what was measured, and why the gate gives way"
    );
    assert!(f.store.get("tools/size").unwrap().gates().is_empty());
    assert_eq!(f.said("tools/size"), before, "no line either");
    // Its orchestrator decides. Handed the role, its executor is refused still.
    f.store
        .decide_gate("tools/size", &own, &raise(&orch, 1, 2))
        .unwrap();
    f.store.hand_over("tools/size", &orch, &exe).unwrap();
    assert_eq!(
        refused(f.store.decide_gate("tools/size", &own, &raise(&exe, 1, 3))),
        format!("{exe} executed tools/size: {tail}")
    );
    // A task closed takes none.
    f.store.move_to("tools/size", "cancelled", &exe).unwrap();
    assert_eq!(
        refused(f.store.decide_gate("tools/size", &own, &raise(&exe, 1, 3))),
        "tools/size is cancelled: a gate decision is for an open task's land"
    );
}

#[test]
fn an_approved_task_takes_a_decision_until_it_lands() {
    let f = Fixture::new(Seed::default());
    let (orch, exe, rev) = (
        f.session("orchestrator"),
        f.session("worker"),
        f.session("reviewer"),
    );
    worked(&f, "tools/size", &orch, &exe);
    let own = Catalogue::symphony();
    f.store.move_to("tools/size", "review", &exe).unwrap();
    f.store.take_role("tools/size", &rev, "reviewer").unwrap();
    f.store
        .verdict("tools/size", &rev, "approve", &format!("{TIP}: reads well"))
        .unwrap();
    // Done by its approve, its land is still to come: a decision is its orchestrator's still.
    assert_eq!(f.store.get("tools/size").unwrap().status, "done");
    f.store
        .decide_gate("tools/size", &own, &waive(&orch, "pictures", TIP))
        .unwrap();
    // Landed, nothing of it is left to land.
    f.store
        .landed(
            "tools/size",
            &orch,
            "landed tools/size on main",
            false,
            &["done"],
        )
        .unwrap();
    assert_eq!(
        refused(f.store.decide_gate("tools/size", &own, &raise(&orch, 1, 2))),
        "tools/size is done, and no land of it is still to come: a gate decision is for a \
             task's land"
    );
    assert_eq!(f.store.get("tools/size").unwrap().gates().len(), 1);
}
