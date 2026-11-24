use super::*;

fn message(id: i64, at: f64, kind: &str, text: &str, sender: &str, to: Option<&str>) -> Message {
    Message {
        id,
        slug: "child".into(),
        sender: sender.into(),
        recipient: to.map(str::to_owned),
        kind: kind.into(),
        text: text.into(),
        ts: at,
    }
}
fn system(id: i64, text: &str) -> Message {
    message(id, id as f64, "system", text, "board", None)
}
fn source(_slug: &str, parent: Option<&str>, lines: Vec<Message>, more: bool) -> Source {
    Source::new(ThreadWindow {
        parent: parent.map(str::to_owned),
        status: "ready".into(),
        lines,
        more,
        uncertain: false,
        by_person: BTreeSet::new(),
    })
}

fn history(sources: BTreeMap<String, Source>) -> History {
    project("child", Vec::new(), &sources)
}

#[test]
fn a_persons_line_is_outside_the_tasks_lanes_by_its_mark_and_an_unmarked_one_is_a_sessions() {
    let lines = vec![
        system(1, "e claimed it"),
        message(2, 2.0, "msg", "from her", "ana", Some("e")),
        message(3, 3.0, "msg", "from a session", "ana", Some("e")),
    ];
    let source = Source::new(ThreadWindow {
        status: "ready".into(),
        lines,
        by_person: BTreeSet::from([2]),
        ..ThreadWindow::default()
    });
    let h = history(BTreeMap::from([("child".into(), source)]));
    let from: Vec<_> = h
        .messages
        .iter()
        .map(|m| (m.id, m.sender.as_str(), m.from.state))
        .collect();
    assert_eq!(
        from,
        [
            (2, "ana", EndpointState::Outside),
            (3, "ana", EndpointState::Unknown)
        ]
    );
    assert!(
        h.messages
            .iter()
            .all(|m| m.to.state == EndpointState::Known)
    );
}

#[test]
fn direct_role_broadcast_outside_and_multirole_addresses_keep_exact_metadata() {
    let h = history(BTreeMap::from([(
        "child".into(),
        source(
            "child",
            None,
            vec![
                system(1, "o joined as orchestrator"),
                system(2, "e claimed it"),
                message(
                    3,
                    5.0,
                    "msg",
                    "do not duplicate this body",
                    "e",
                    Some("@orchestrator"),
                ),
                message(4, 5.0, "msg", "outgoing", "o", Some("e")),
                message(5, 5.0, "msg", "broadcast", "o", None),
                message(6, 6.0, "msg", "owner", "owner", Some("board")),
                system(7, "e joined as reviewer"),
                message(8, 8.0, "msg", "two roles", "outside", Some("e")),
            ],
            false,
        ),
    )]));
    assert_eq!(
        h.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
        [3, 4, 5, 6, 8]
    );
    let m = &h.messages[0];
    assert_eq!(
        (&*m.sender, m.recipient.as_deref(), m.at, &*m.kind),
        ("e", Some("@orchestrator"), 5.0, "msg")
    );
    assert_eq!(
        (m.from.state, m.to.state),
        (EndpointState::Known, EndpointState::Known)
    );
    assert_eq!(m.to.candidates[0].by, "o");
    assert_eq!(h.messages[1].to.candidates[0].role, "executor");
    assert_eq!(h.messages[2].to.state, EndpointState::Everyone);
    assert_eq!(
        (h.messages[3].from.state, h.messages[3].to.state),
        (EndpointState::Outside, EndpointState::Outside)
    );
    assert_eq!(
        (h.messages[4].from.state, h.messages[4].to.state),
        (EndpointState::Unknown, EndpointState::Ambiguous)
    );
    assert_eq!(h.messages[4].to.candidates.len(), 2);
    assert!(
        !serde_json::to_string(&h)
            .unwrap()
            .contains("duplicate this body")
    );
}

#[test]
fn inheritance_uses_the_recorded_parent_at_send_time_and_names_its_source() {
    let mut sources: BTreeMap<String, Source> = BTreeMap::from([
        (
            "child".into(),
            source(
                "child",
                Some("new"),
                vec![
                    system(3, "created by worker"),
                    message(4, 4.0, "msg", "old", "e", Some("@orchestrator")),
                    system(5, "owner put it under new (was old)"),
                    message(6, 6.0, "msg", "new", "e", Some("@orchestrator")),
                    system(7, "local joined as orchestrator"),
                    message(8, 8.0, "msg", "local", "e", Some("@orchestrator")),
                ],
                false,
            ),
        ),
        (
            "old".into(),
            source(
                "old",
                None,
                vec![system(1, "a joined as orchestrator")],
                false,
            ),
        ),
        (
            "new".into(),
            source(
                "new",
                None,
                vec![system(2, "b joined as orchestrator")],
                false,
            ),
        ),
    ]);
    let h = load("child", |slug| {
        sources
            .remove(slug)
            .ok_or_else(|| symphony_board::Error::Refused("missing source".into()))
    });
    assert_eq!(
        h.sources
            .iter()
            .map(|source| source.slug.as_str())
            .collect::<Vec<_>>(),
        ["child", "new", "old"]
    );
    let endpoints: Vec<_> = h
        .messages
        .iter()
        .map(|m| {
            (
                &*m.to.candidates[0].by,
                &*m.to.candidates[0].source,
                m.to.state,
            )
        })
        .collect();
    assert_eq!(
        endpoints,
        [
            ("a", "old", EndpointState::Known),
            ("b", "new", EndpointState::Known),
            ("local", "child", EndpointState::Known)
        ]
    );
    let bounds: Vec<_> = h
        .roles
        .iter()
        .filter(|s| s.role == "orchestrator")
        .map(|s| (&*s.source, s.from_id, s.to_id))
        .collect();
    assert_eq!(
        bounds,
        [("old", 3, Some(5)), ("new", 5, Some(7)), ("child", 7, None)]
    );
    assert!(h.roles.iter().all(|span| !span.uncertain));
}

#[test]
fn a_message_without_child_creation_history_does_not_prove_an_inherited_start() {
    let h = history(BTreeMap::from([
        (
            "child".into(),
            source(
                "child",
                Some("parent"),
                vec![
                    message(
                        3,
                        3.0,
                        "msg",
                        "first observation",
                        "e",
                        Some("@orchestrator"),
                    ),
                    message(
                        4,
                        4.0,
                        "msg",
                        "later observation",
                        "e",
                        Some("@orchestrator"),
                    ),
                ],
                false,
            ),
        ),
        (
            "parent".into(),
            source(
                "parent",
                None,
                vec![system(1, "o joined as orchestrator")],
                false,
            ),
        ),
    ]));
    assert_eq!(h.roles.len(), 1);
    let span = &h.roles[0];
    assert_eq!(
        (
            span.source.as_str(),
            span.by.as_str(),
            span.from_id,
            span.from,
            span.to_id
        ),
        ("parent", "o", 3, 3.0, None)
    );
    assert!(
        span.uncertain,
        "the first message is an observation, not an assignment boundary"
    );
    assert!(
        h.messages
            .iter()
            .all(|message| message.to.state == EndpointState::Known)
    );
}

#[test]
fn a_parent_snapshot_or_move_chain_contradiction_cannot_identify_a_historical_holder() {
    for (current_parent, later_moves) in [
        (Some("new"), vec![]),
        (None, vec![]),
        (
            Some("new"),
            vec![system(6, "owner put it under new (was other)")],
        ),
    ] {
        let mut lines = vec![
            system(3, "owner put it under old (was nothing)"),
            message(
                4,
                4.0,
                "msg",
                "after an unrecorded move",
                "e",
                Some("@orchestrator"),
            ),
        ];
        lines.extend(later_moves);
        let mut sources: BTreeMap<String, Source> = BTreeMap::from([
            (
                "child".into(),
                source("child", current_parent, lines, false),
            ),
            (
                "old".into(),
                source(
                    "old",
                    None,
                    vec![system(1, "old-holder joined as orchestrator")],
                    false,
                ),
            ),
            (
                "new".into(),
                source(
                    "new",
                    None,
                    vec![system(2, "new-holder joined as orchestrator")],
                    false,
                ),
            ),
            ("other".into(), source("other", None, vec![], false)),
        ]);
        let h = load("child", |slug| {
            sources
                .remove(slug)
                .ok_or_else(|| symphony_board::Error::Refused("missing source".into()))
        });
        assert_eq!(h.sources[0].state, HistoryState::Partial);
        assert_eq!(
            h.sources[0].before, None,
            "no range cutoff explains the contradiction"
        );
        assert_eq!(h.messages[0].recipient.as_deref(), Some("@orchestrator"));
        assert_eq!(h.messages[0].to.state, EndpointState::Unknown);
        assert!(
            h.messages[0].to.candidates.is_empty(),
            "neither parent may be backfilled"
        );
        assert!(h.roles.is_empty());
    }
}

#[test]
fn unrecorded_closure_reopening_cut_events_and_legacy_parent_moves_never_supply_confidence() {
    for (status, cut, lines) in [
        (
            "cancelled",
            false,
            vec![system(1, "o joined as orchestrator")],
        ),
        ("ready", true, vec![system(1, "o joined as orchestrator")]),
        (
            "ready",
            false,
            vec![
                system(1, "o joined as orchestrator"),
                system(2, "owner moved it ready → cancelled"),
            ],
        ),
        (
            "cancelled",
            true,
            vec![
                system(1, "o joined as orchestrator"),
                system(2, "owner moved it ready → cancelled"),
            ],
        ),
        (
            "ready",
            false,
            vec![
                system(1, "o joined as orchestrator"),
                system(2, "owner put it under parent"),
            ],
        ),
        (
            "ready",
            false,
            vec![
                system(1, "o joined as orchestrator"),
                system(2, "a b put it under parent (was nothing)"),
            ],
        ),
    ] {
        let mut lines = lines;
        lines.push(message(3, 3.0, "msg", "?", "o", Some("@orchestrator")));
        let mut s = source("child", None, lines, false);
        s.window.status = status.into();
        s.window.uncertain = cut;
        let s = Source::new(s.window);
        let h = history(BTreeMap::from([("child".into(), s)]));
        assert_eq!(h.messages[0].to.state, EndpointState::Unknown);
        assert_eq!(h.messages[0].after_close, None);
        assert!(h.roles.iter().all(|span| span.uncertain));
    }
}

#[test]
fn reparenting_to_the_top_stops_inheritance_without_filling_the_gap() {
    let s = source(
        "child",
        None,
        vec![
            system(1, "created by worker"),
            system(2, "owner put it under parent (was nothing)"),
            system(3, "owner put it under nothing (was parent)"),
            message(4, 4.0, "msg", "?", "e", Some("@orchestrator")),
        ],
        false,
    );
    assert!(
        s.complete(),
        "a coherent recorded move chain stays reliable"
    );
    assert_eq!(s.parent_at(2), Some("parent"));
    assert_eq!(s.parent_at(3), None);
    assert_eq!(s.parents(), ["parent"]);
    assert_eq!(s.parent_at(4), None);
    assert_eq!(
        symphony_api::timeline::TIMELINE_HISTORY_LIMIT,
        symphony_board::THREAD_WINDOW_LIMIT
    );
}

#[test]
fn partial_missing_and_cyclic_sources_leave_the_individual_unknown() {
    for sources in [
        BTreeMap::from([(
            "child".into(),
            source(
                "child",
                Some("missing"),
                vec![message(2, 2.0, "msg", "?", "e", Some("@orchestrator"))],
                false,
            ),
        )]),
        BTreeMap::from([(
            "child".into(),
            source(
                "child",
                Some("child"),
                vec![message(2, 2.0, "msg", "?", "e", Some("@orchestrator"))],
                false,
            ),
        )]),
        BTreeMap::from([(
            "child".into(),
            source(
                "child",
                None,
                vec![
                    system(1, "o joined as orchestrator"),
                    message(2, 2.0, "msg", "?", "e", Some("@orchestrator")),
                ],
                true,
            ),
        )]),
    ] {
        let h = history(sources);
        assert_eq!(h.messages[0].to.state, EndpointState::Unknown);
    }
}

#[test]
fn gaps_multiple_holders_closure_and_reopening_never_backfill_a_role() {
    let h = history(BTreeMap::from([(
        "child".into(),
        source(
            "child",
            None,
            vec![
                system(1, "a joined as orchestrator"),
                system(2, "b joined as orchestrator"),
                message(3, 3.0, "msg", "ambiguous", "e", Some("@orchestrator")),
                system(4, "a released every role (left)"),
                system(5, "b released orchestrator (released)"),
                message(6, 6.0, "msg", "gap", "e", Some("@orchestrator")),
                system(7, "c joined as orchestrator"),
                system(8, "owner moved it ready → cancelled"),
                message(9, 9.0, "msg", "closed", "c", Some("@orchestrator")),
                system(10, "owner moved it cancelled → ready"),
                message(11, 11.0, "msg", "open", "c", Some("@orchestrator")),
            ],
            false,
        ),
    )]));
    assert_eq!(
        h.messages.iter().map(|m| m.to.state).collect::<Vec<_>>(),
        [
            EndpointState::Ambiguous,
            EndpointState::Unknown,
            EndpointState::Unknown,
            EndpointState::Known
        ]
    );
    assert_eq!(h.messages[2].after_close, Some(true));
    assert_eq!(h.messages[3].after_close, Some(false));
    assert_eq!(h.messages[2].from.state, EndpointState::Unknown);
    assert_eq!(
        h.roles
            .iter()
            .filter(|s| s.by == "c")
            .map(|s| (s.from_id, s.to_id))
            .collect::<Vec<_>>(),
        [(7, Some(8)), (10, None)]
    );
}

#[test]
fn regressing_clocks_keep_exact_send_times_and_refuse_a_definite_holder() {
    let h = history(BTreeMap::from([(
        "child".into(),
        source(
            "child",
            None,
            vec![
                message(2, 10.0, "system", "o joined as orchestrator", "board", None),
                message(3, 5.0, "msg", "earlier clock", "o", Some("@orchestrator")),
            ],
            false,
        ),
    )]));
    assert_eq!(h.messages[0].at, 5.0);
    assert_eq!(h.messages[0].to.state, EndpointState::Unknown);
    assert_eq!(h.roles[0].from, 10.0);
    assert!(h.roles[0].uncertain);
}

#[test]
fn duplicate_and_unsorted_lines_have_one_entry_per_id_and_ties_use_id_order() {
    let mut lines = vec![
        system(1, "o joined as orchestrator"),
        message(2, 5.0, "msg", "one", "o", Some("@orchestrator")),
        message(3, 5.0, "msg", "two", "o", Some("@orchestrator")),
    ];
    lines.push(lines[1].clone());
    lines.reverse();
    let h = history(BTreeMap::from([(
        "child".into(),
        source("child", None, lines, false),
    )]));
    assert_eq!(h.messages.iter().map(|m| m.id).collect::<Vec<_>>(), [2, 3]);
    assert!(
        h.messages
            .iter()
            .all(|m| m.to.state == EndpointState::Known)
    );
}

#[test]
fn a_closing_verdict_keeps_its_senders_role_at_send_but_no_hold_after_it() {
    let source = Source::new(ThreadWindow {
        status: "done".into(),
        lines: vec![
            system(1, "o joined as orchestrator"),
            system(2, "r joined as reviewer"),
            message(
                3,
                3.0,
                "verdict",
                "approve: fine",
                "r",
                Some("@orchestrator"),
            ),
            message(4, 4.0, "msg", "after", "r", Some("@reviewer")),
        ],
        ..ThreadWindow::default()
    });
    let h = history(BTreeMap::from([("child".into(), source)]));
    assert_eq!(h.messages[0].from.state, EndpointState::Known);
    assert_eq!(h.messages[0].to.state, EndpointState::Known);
    assert_eq!(h.messages[0].after_close, Some(false));
    assert_eq!(h.messages[1].from.state, EndpointState::Unknown);
    assert_eq!(h.messages[1].after_close, Some(true));
    assert!(h.roles.iter().all(|span| span.to_id == Some(3)));
}

#[test]
fn loading_is_bounded_and_missing_sources_are_explicit_without_current_holder_backfill() {
    let mut read = Vec::new();
    let h = load("child", |slug| {
        read.push(slug.to_string());
        let parent = format!("parent-{}", read.len());
        Ok(source(
            slug,
            Some(&parent),
            vec![message(100, 100.0, "msg", "?", "e", Some("@orchestrator"))],
            false,
        ))
    });
    assert_eq!(read.len(), TIMELINE_HISTORY_SOURCES);
    assert!(!h.sources_complete);
    assert_eq!(h.sources.len(), TIMELINE_HISTORY_SOURCES);
    assert!(h.roles.is_empty());
    assert_eq!(h.messages[0].to.state, EndpointState::Unknown);
    let failed = load("child", |_| {
        Err(symphony_board::Error::Db("unreachable".into()))
    });
    assert_eq!(failed.sources[0].state, HistoryState::Unavailable);
    assert!(failed.messages.is_empty());
    let partial = load("child", |slug| {
        Ok(source(
            slug,
            None,
            vec![message(100, 100.0, "msg", "?", "e", None)],
            true,
        ))
    });
    assert_eq!(partial.sources[0].state, HistoryState::Partial);
    assert_eq!(partial.sources[0].before, Some(100));
    assert_eq!(partial.messages[0].after_close, None);
    let empty = load("child", |slug| Ok(source(slug, None, vec![], false)));
    assert_eq!(empty.sources[0].state, HistoryState::Complete);
    assert_eq!(empty.sources[0].first_id, None);
    assert!(empty.sources_complete);
    let mut count = 0;
    let cycle = load("child", |slug| {
        count += 1;
        Ok(source(
            slug,
            Some("child"),
            vec![message(1, 1.0, "msg", "?", "e", Some("@orchestrator"))],
            false,
        ))
    });
    assert_eq!(count, 1);
    assert_eq!(cycle.messages[0].to.state, EndpointState::Unknown);
}
