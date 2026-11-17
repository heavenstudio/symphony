use super::*;

fn line(id: i64, ts: f64, kind: &str, text: &str) -> Line {
    Line {
        id,
        ts,
        kind: kind.into(),
        text: text.into(),
        sender: "writer".into(),
    }
}
fn system(id: i64, text: &str) -> Line {
    line(id, id as f64, "system", text)
}

#[test]
fn gives_joins_handover_takeover_and_release_keep_exact_boundaries_and_gaps() {
    let mut lines = vec![
        system(1, "a was given orchestrator (filed by worker)"),
        system(2, "a joined as orchestrator"),
        system(
            3,
            "a handed orchestrator over to b; address b or @orchestrator",
        ),
        system(4, "b released orchestrator (released)"),
        system(6, "c joined as orchestrator"),
        system(7, "d joined as orchestrator"),
        system(8, "e took orchestrator over from c (lost), d (left)"),
        system(9, "e released every role (left)"),
    ];
    lines.push(lines[0].clone());
    lines.reverse();
    let got = replay(&lines);
    assert!(!got.uncertain);
    let bounds: Vec<_> = got
        .spans
        .iter()
        .map(|s| (s.by.as_str(), s.from_id, s.to_id))
        .collect();
    assert_eq!(
        bounds,
        [
            ("a", 1, Some(3)),
            ("b", 3, Some(4)),
            ("c", 6, Some(8)),
            ("d", 7, Some(8)),
            ("e", 8, Some(9))
        ]
    );
    assert!(
        got.spans
            .iter()
            .all(|s| s.role == "orchestrator" && s.from == s.from_id as f64 && !s.uncertain)
    );
}

#[test]
fn multiple_roles_and_holders_are_preserved_and_every_role_release_is_scoped_to_its_handle() {
    let got = replay(&[
        system(1, "a claimed it"),
        system(2, "a joined as reviewer"),
        system(3, "a joined as orchestrator"),
        system(4, "b joined as orchestrator"),
        system(5, "c claimed it, handed over from a (lost)"),
        system(6, "a released every role (left)"),
    ]);
    let spans: Vec<_> = got
        .spans
        .iter()
        .map(|s| (s.by.as_str(), s.role.as_str(), s.to_id))
        .collect();
    assert_eq!(
        spans,
        [
            ("a", "executor", Some(5)),
            ("a", "reviewer", Some(6)),
            ("a", "orchestrator", Some(6)),
            ("b", "orchestrator", None),
            ("c", "executor", None)
        ]
    );
}

#[test]
fn closure_suspends_roles_and_recorded_reopening_starts_a_fresh_stretch() {
    let got = replay(&[
        system(1, "a joined as orchestrator"),
        system(2, "owner moved it ready → cancelled"),
        line(3, 3.0, "msg", "after closure"),
        system(4, "owner moved it cancelled → ready"),
        system(5, "a released orchestrator (released)"),
        line(6, 6.0, "verdict", "done: finished"),
    ]);
    assert!(got.closed);
    assert_eq!(got.spans.len(), 2);
    assert_eq!((got.spans[0].from_id, got.spans[0].to_id), (1, Some(2)));
    assert_eq!((got.spans[1].from_id, got.spans[1].to_id), (4, Some(5)));
}

#[test]
fn clock_regressions_and_conflicting_duplicates_never_invent_clean_times() {
    let lines = [
        line(1, 20.0, "system", "a joined as orchestrator"),
        line(2, 10.0, "system", "a released orchestrator (released)"),
    ];
    let got = replay(&lines);
    assert_eq!((got.spans[0].from, got.spans[0].to), (20.0, Some(10.0)));
    assert!(got.spans[0].uncertain);
    let mut conflicting = lines.to_vec();
    conflicting.push(line(1, 20.0, "system", "b joined as orchestrator"));
    assert!(replay(&conflicting).uncertain);
    assert!(replay(&[line(1, f64::NAN, "system", "a joined as orchestrator")]).uncertain);
}

#[test]
fn plain_text_and_malformed_events_supply_no_roles() {
    let got = replay(&[
        line(1, 1.0, "msg", "a was given orchestrator (filed by worker)"),
        system(2, "a joined as owner"),
        system(3, "a b joined as orchestrator"),
        system(4, " joined as orchestrator"),
        system(5, "a took orchestrator over from "),
        system(6, "a released owner (released)"),
        system(7, "a b claimed it"),
    ]);
    assert!(got.spans.is_empty());
    assert!(!got.closed);
}
