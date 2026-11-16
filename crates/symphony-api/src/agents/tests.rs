use super::*;

fn agent(handle: &str) -> Agent {
    Agent {
        handle: handle.into(),
        kind: "worker".into(),
        model: String::new(),
        parent: None,
        task: None,
        role: None,
        tasks: Vec::new(),
        following: 0,
        worktree: String::new(),
        branch: String::new(),
        agent: None,
        session_id: None,
        pane: None,
        callback: None,
        chat: false,
        state: "idle".into(),
        state_detail: None,
        health: "live".into(),
        last_tool_call: None,
        last_message: None,
        slots: None,
        tokens: None,
        cost: None,
        session_url: None,
        in_process: false,
        go: None,
    }
}

fn sub(handle: &str, parent: &str) -> Agent {
    Agent {
        parent: Some(parent.into()),
        in_process: true,
        ..agent(handle)
    }
}

fn go(handle: &str, url: Option<&str>, pane: Option<&str>) -> SessionGo {
    SessionGo {
        handle: handle.into(),
        url: url.map(str::to_string),
        pane: pane.map(str::to_string),
    }
}

fn resolved(mut agents: Vec<Agent>, panes: bool) -> Vec<Option<SessionGo>> {
    resolve_sessions(&mut agents, panes);
    agents.into_iter().map(|a| a.go).collect()
}

#[test]
fn an_agent_goes_to_its_own_pane_and_web_address() {
    let web = Agent {
        session_url: Some("https://claude.ai/code/s1".into()),
        ..agent("web")
    };
    let paned = Agent {
        pane: Some("t7".into()),
        ..agent("paned")
    };
    let both = Agent {
        pane: Some("t8".into()),
        session_url: Some("https://claude.ai/code/s2".into()),
        ..agent("both")
    };
    assert_eq!(
        resolved(vec![web, paned, both], true),
        [
            Some(go("web", Some("https://claude.ai/code/s1"), None)),
            Some(go("paned", None, Some("t7"))),
            Some(go("both", Some("https://claude.ai/code/s2"), Some("t8"))),
        ]
    );
}

#[test]
fn with_panes_off_a_pane_is_never_offered() {
    let paned = Agent {
        pane: Some("t7".into()),
        ..agent("paned")
    };
    let both = Agent {
        pane: Some("t8".into()),
        session_url: Some("https://h/x".into()),
        ..agent("both")
    };
    assert_eq!(
        resolved(vec![paned, both], false),
        [None, Some(go("both", Some("https://h/x"), None))]
    );
}

#[test]
fn a_subagent_goes_to_its_parents_session_and_one_of_its_own_wins() {
    let parent = Agent {
        pane: Some("t1".into()),
        kind: "orchestrator".into(),
        ..agent("orch")
    };
    let own = Agent {
        session_url: Some("https://h/own".into()),
        ..sub("own", "orch")
    };
    let agents = vec![
        parent,
        sub("child", "orch"),
        sub("grandchild", "child"),
        own,
    ];
    assert_eq!(
        resolved(agents, true),
        [
            Some(go("orch", None, Some("t1"))),
            Some(go("orch", None, Some("t1"))),
            Some(go("orch", None, Some("t1"))),
            Some(go("own", Some("https://h/own"), None)),
        ]
    );
}

#[test]
fn nothing_known_gives_no_control() {
    let launched = Agent {
        parent: Some("orch".into()),
        ..agent("launched")
    };
    let parent = Agent {
        pane: Some("t1".into()),
        ..agent("orch")
    };
    let agents = vec![
        parent,
        // A launched worker is no subagent: its parent's pane is not its session.
        launched,
        agent("bare"),
        sub("orphan", "nobody"),
        sub("self", "self"),
        sub("loop-a", "loop-b"),
        sub("loop-b", "loop-a"),
        Agent {
            pane: Some(String::new()),
            session_url: Some(String::new()),
            ..agent("empty")
        },
    ];
    assert_eq!(
        resolved(agents, true)[1..],
        [None, None, None, None, None, None, None]
    );
}

#[test]
fn a_chain_of_subagents_deeper_than_the_limit_finds_nothing() {
    let mut agents = vec![Agent {
        pane: Some("t1".into()),
        ..agent("a0")
    }];
    for n in 1..=PARENTS + 2 {
        agents.push(sub(&format!("a{n}"), &format!("a{}", n - 1)));
    }
    let found = resolved(agents, true);
    assert_eq!(found[PARENTS], Some(go("a0", None, Some("t1"))));
    assert_eq!(found[PARENTS + 1], None);
}

#[test]
fn a_local_page_focuses_a_pane_another_links_or_shows_the_screen() {
    let both = go("a", Some("https://h/x"), Some("t7"));
    assert_eq!(both.control(true), Some(SessionControl::Focus("t7")));
    assert_eq!(
        both.control(false),
        Some(SessionControl::Link("https://h/x"))
    );
    let pane = go("a", None, Some("t7"));
    assert_eq!(pane.control(true), Some(SessionControl::Focus("t7")));
    assert_eq!(pane.control(false), Some(SessionControl::Screen("t7")));
    let web = go("a", Some("https://h/x"), None);
    assert_eq!(web.control(true), Some(SessionControl::Link("https://h/x")));
    assert_eq!(
        web.control(false),
        Some(SessionControl::Link("https://h/x"))
    );
    assert_eq!(go("a", None, None).control(true), None);
    assert!(both.via_parent("child"));
    assert!(!both.via_parent("a"));
}

/// Addresses no page may link to, whatever wrote them to the board.
const UNLINKABLE: [&str; 14] = [
    "javascript:alert(1)",
    "JavaScript:alert(1)",
    " https://h/x",
    "data:text/html,<script>alert(1)</script>",
    "file:///etc/passwd",
    "vbscript:x",
    "//evil.example/x",
    "/task/x",
    "claude.ai/code/x",
    "https://",
    "https:///path",
    "http://?q",
    "https://a b",
    "https://a\u{7}b",
];

#[test]
fn a_web_address_is_http_or_https_naming_a_host_with_no_space_or_control_character() {
    for linked in [
        "https://claude.ai/code/session_01abc",
        "http://mac.tail.ts.net:8080/x?y=1#z",
        "HTTPS://Example.com",
        "Http://h",
        "https://h?q",
        "https://h#f",
    ] {
        assert!(web_address(linked), "{linked:?}");
    }
    for refused in
        UNLINKABLE
            .into_iter()
            .chain(["", "https", "https:/h", "https://h\u{a0}x", "ftp://h/x"])
    {
        assert!(!web_address(refused), "{refused:?}");
    }
}

#[test]
fn an_address_that_is_no_web_address_is_never_resolved_or_linked() {
    for bad in UNLINKABLE {
        let web_only = Agent {
            session_url: Some(bad.into()),
            ..agent("web")
        };
        let paned = Agent {
            session_url: Some(bad.into()),
            pane: Some("t7".into()),
            ..agent("paned")
        };
        assert_eq!(
            resolved(vec![web_only, paned], true),
            [None, Some(go("paned", None, Some("t7")))],
            "{bad:?}"
        );
        // An answer that carries one anyway (another build's, a hand-made one) links nothing.
        assert_eq!(go("a", Some(bad), None).control(true), None, "{bad:?}");
        assert_eq!(go("a", Some(bad), None).control(false), None, "{bad:?}");
        let both = go("a", Some(bad), Some("t7"));
        assert_eq!(both.control(true), Some(SessionControl::Focus("t7")));
        assert_eq!(both.control(false), Some(SessionControl::Screen("t7")));
    }
}

#[test]
fn an_older_answer_without_the_session_keys_reads_as_nothing_known() {
    let mut old = serde_json::to_value(agent("a")).unwrap();
    for key in ["session_url", "in_process", "go"] {
        old.as_object_mut().unwrap().remove(key);
    }
    let read: Agent = serde_json::from_value(old).unwrap();
    assert_eq!(read, agent("a"));
}

#[test]
fn the_open_tasks_it_orchestrates_are_counted_and_an_older_answer_counts_none() {
    let following = Agent {
        following: 3,
        ..agent("a")
    };
    let mut wire = serde_json::to_value(&following).unwrap();
    assert_eq!(wire["following"], 3);
    assert_eq!(
        serde_json::from_value::<Agent>(wire.clone()).unwrap(),
        following
    );
    wire.as_object_mut().unwrap().remove("following");
    assert_eq!(serde_json::from_value::<Agent>(wire).unwrap(), agent("a"));
}

#[test]
fn a_cost_reads_as_the_board_writes_it_and_an_older_answer_has_none() {
    let board = serde_json::json!({
        "dollars": 2.7, "input": 1.0, "cache_write_5m": 0.5, "cache_write_1h": 0.0,
        "cache_read": 0.2, "output": 1.0,
        "unpriced": [{"model": "gpt-x", "speed": "fast", "tokens": {"input": 7,
            "cache_write_5m": 0, "cache_write_1h": 0, "cache_read": 0, "output": 0, "calls": 1}}],
        "prices": "2026-11-16",
    });
    let mut wire = serde_json::to_value(agent("a")).unwrap();
    wire["cost"] = board;
    let read: Agent = serde_json::from_value(wire.clone()).unwrap();
    let cost = read.cost.unwrap();
    assert_eq!((cost.dollars, cost.prices.as_str()), (2.7, "2026-11-16"));
    assert_eq!(cost.unpriced[0].model, "gpt-x");
    assert_eq!(cost.unpriced[0].tokens.input, 7);
    wire.as_object_mut().unwrap().remove("cost");
    assert_eq!(serde_json::from_value::<Agent>(wire).unwrap(), agent("a"));
}
