use super::*;

#[test]
fn cards_and_agents_share_two_machine_phases_without_rewriting_task_lifecycle() {
    use serde_json::json;
    let task: Task = serde_json::from_value(json!({
        "slug":"fixture/task","title":"Fixture","body":"","status":"review","priority":1,
        "parent":null,"created_by":"fixture","created":1.0,"updated":1.0,
        "links":[],"depends":[],"roles":[],"instruction":false,
    }))
    .unwrap();
    let machine = |id: &str, kind: &str, state: &str| {
        json!({
            "machine_id":id,"label":id,"producer_id":"fixture","sequence":1,
            "age_ms":0,"freshness":"fresh","complete":true,"reason":"",
            "counts":{"running_builds":0,"queued_builds":0,"running_tests":0,"queued_tests":0},
            "operations":[{
                "machine_id":id,"operation_id":"separate-machine-same-nonce","parent_id":null,
                "handle":"cobalt-otter","task":"fixture/task","checkout":"opaque",
                "recipe":"fixture","kind":kind,"state":state,"resource":"","reason":"","phase_since":null,
            }],
        })
    };
    let mut work: symphony_api::MachineWork = serde_json::from_value(json!({
        "freshness":"unknown","machines":[machine("alpha","build","queued"), machine("beta","test","running")],"unassigned":[],
    })).unwrap();
    for expected in [
        vec!["Build queued · alpha", "Test running · beta"],
        vec!["Build running · alpha", "Test running · beta"],
        vec!["Test running · beta"],
        vec!["Observed Test running · beta · disconnected"],
    ] {
        let card = dioxus_ssr::render_element(
            rsx! { TaskRow { task: task.clone(), work: SharedWork::from(work.clone()) } },
        );
        let agent = dioxus_ssr::render_element(
            rsx! { AgentRow { agent: agent("worker", None), tasks: Rc::new(Vec::new()), work: SharedWork::from(work.clone()) } },
        );
        for html in [&card, &agent] {
            assert_eq!(
                html.matches("data-work-kind=").count(),
                expected.len(),
                "{html}"
            );
            for label in &expected {
                assert!(html.contains(label), "missing {label}: {html}");
            }
        }
        assert!(
            card.contains("st-review"),
            "resource work changed task lifecycle: {card}"
        );
        assert!(
            !agent.contains("open tasks:"),
            "resource phases introduced author admission: {agent}"
        );
        match work.machines[0].operations.first() {
            Some(operation) if operation.state == symphony_api::WorkState::Queued => {
                work.machines[0].operations[0].state = symphony_api::WorkState::Running;
            }
            Some(_) => work.machines[0].operations.clear(),
            None => work = crate::machine_work::connection(work, false),
        }
    }
}

/// `cobalt-otter`, an agent of `kind` holding nothing, as the agents API sends it with `slots`.
fn agent(kind: &str, slots: Option<symphony_api::AgentSlots>) -> symphony_api::Agent {
    symphony_api::Agent {
        handle: "cobalt-otter".into(),
        kind: kind.into(),
        model: "opus".into(),
        parent: None,
        task: None,
        role: None,
        tasks: Vec::new(),
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
        slots,
        session_url: None,
        in_process: false,
        go: None,
    }
}

#[test]
fn an_agent_under_the_cap_shows_its_open_tasks_against_it_and_one_without_shows_none() {
    let capped = agent(
        "orchestrator",
        Some(symphony_api::AgentSlots { open: 3, cap: 8 }),
    );
    let html = dioxus_ssr::render_element(
        rsx! { AgentRow { agent: capped, tasks: Rc::new(Vec::new()), work: SharedWork::default() } },
    );
    assert!(html.contains("<p>open tasks: 3 of 8</p>"), "{html}");
    let html = dioxus_ssr::render_element(
        rsx! { AgentRow { agent: agent("worker", None), tasks: Rc::new(Vec::new()), work: SharedWork::default() } },
    );
    assert!(html.contains("cobalt-otter"), "{html}");
    assert!(!html.contains("open tasks:"), "{html}");
}

fn go(handle: &str, url: Option<&str>, pane: Option<&str>) -> symphony_api::SessionGo {
    symphony_api::SessionGo {
        handle: handle.into(),
        url: url.map(str::to_string),
        pane: pane.map(str::to_string),
    }
}

/// `cobalt-otter`'s control going by `go`, on a page asked here when `local`.
fn control(go: Option<symphony_api::SessionGo>, local: bool) -> String {
    dioxus_ssr::render_element(
        rsx! { GoToSession { handle: "cobalt-otter".to_string(), go, local } },
    )
}

/// The control when there is somewhere to go.
fn to(go: symphony_api::SessionGo, local: bool) -> String {
    control(Some(go), local)
}

#[test]
fn a_session_control_links_focuses_or_shows_the_screen_by_where_the_page_was_asked() {
    let web = go(
        "cobalt-otter",
        Some("https://claude.ai/code/session_01abc"),
        None,
    );
    for local in [true, false] {
        let html = to(web.clone(), local);
        assert!(
            html.contains(r#"<a class="go-session-link" href="https://claude.ai/code/session_01abc" target="_blank" rel="noopener noreferrer">Go to session</a>"#),
            "{html}"
        );
        assert!(!html.contains("<button"), "{html}");
    }
    let paned = go("cobalt-otter", None, Some("t7"));
    let here = to(paned.clone(), true);
    assert!(here.contains(r#"data-go="focus""#), "{here}");
    assert!(
        here.contains("Shows t7 in the Herdr app on this machine"),
        "{here}"
    );
    assert!(here.contains(">Go to session</button>"), "{here}");
    let phone = to(paned, false);
    assert!(phone.contains(r#"data-go="screen""#), "{phone}");
    assert!(phone.contains("screen, read only"), "{phone}");
    assert!(!phone.contains("Herdr app"), "{phone}");
    // With both, this machine focuses the pane and another follows the web address.
    let both = go("cobalt-otter", Some("https://h/x"), Some("t7"));
    assert!(to(both.clone(), true).contains(r#"data-go="focus""#));
    assert!(to(both, false).contains(r#"href="https://h/x""#));
    for local in [true, false] {
        assert!(!to(go("cobalt-otter", None, None), local).contains("Go to session"));
        assert!(!control(None, local).contains("Go to session"));
    }
}

#[test]
fn an_address_that_is_no_web_address_never_becomes_a_link() {
    for bad in [
        "javascript:alert(1)",
        "JAVASCRIPT:alert(document.cookie)",
        "data:text/html,x",
        "/api/agent-focus",
        " https://h/x",
    ] {
        for local in [true, false] {
            let html = to(go("cobalt-otter", Some(bad), None), local);
            assert!(!html.contains("<a"), "{bad:?}: {html}");
            assert!(!html.contains("Go to session"), "{bad:?}: {html}");
        }
        // With a pane beside it, the pane is the way in, here and from a phone.
        let here = to(go("cobalt-otter", Some(bad), Some("t7")), true);
        assert!(here.contains(r#"data-go="focus""#), "{here}");
        let phone = to(go("cobalt-otter", Some(bad), Some("t7")), false);
        assert!(phone.contains(r#"data-go="screen""#), "{phone}");
        for html in [here, phone] {
            assert!(!html.contains("<a"), "{bad:?}: {html}");
            assert!(!html.contains("href"), "{bad:?}: {html}");
        }
    }
}

#[test]
fn a_subagents_control_goes_to_its_parents_session_and_says_so() {
    let html = to(go("orch-handle", None, Some("t1")), true);
    assert!(html.contains(r#"data-go="focus""#), "{html}");
    let via = html
        .split(r#"<span class="go-session-via">"#)
        .nth(1)
        .and_then(|rest| rest.split("</span>").next())
        .unwrap_or_else(|| panic!("no parent named: {html}"));
    assert!(
        via.starts_with("in orch-handle") && via.ends_with("s session"),
        "{via}"
    );
    let own = to(go("cobalt-otter", None, Some("t1")), true);
    assert!(!own.contains("go-session-via"), "{own}");
}

#[test]
fn an_agent_row_carries_its_control_and_one_with_nothing_known_has_none() {
    let row = |agent: symphony_api::Agent, local: bool| {
        dioxus_ssr::render_element(
            rsx! { AgentRow { agent, local, tasks: Rc::new(Vec::new()), work: SharedWork::default() } },
        )
    };
    let paned = symphony_api::Agent {
        go: Some(go("cobalt-otter", None, Some("t7"))),
        ..agent("worker", None)
    };
    assert!(row(paned.clone(), true).contains(r#"data-go="focus""#));
    assert!(row(paned, false).contains(r#"data-go="screen""#));
    let html = row(agent("worker", None), true);
    assert!(!html.contains("Go to session"), "{html}");
}

#[test]
fn an_agent_counts_the_open_tasks_it_follows_in_words_and_following_none_offers_nothing() {
    let held = |slug: &str, role: &str, status: &str| symphony_api::HeldTask {
        slug: slug.into(),
        role: role.into(),
        status: status.into(),
    };
    let row = |tasks: Vec<symphony_api::HeldTask>| {
        let mut agent = agent("orchestrator", None);
        agent.tasks = tasks;
        dioxus_ssr::render_element(
            rsx! { AgentRow { agent, tasks: Rc::new(Vec::new()), work: SharedWork::default() } },
        )
    };
    let none = row(vec![
        held("a/done", "orchestrator", "done"),
        held("a/worked", "worker", "in_progress"),
    ]);
    assert!(!none.contains("Following"), "{none}");
    let one = row(vec![held("a/one", "orchestrator", "ready")]);
    assert!(one.contains(">Following 1 open task</summary>"), "{one}");
    let two = row(vec![
        held("a/one", "orchestrator", "ready"),
        held("a/two", "orchestrator", "review"),
        held("a/old", "orchestrator", "cancelled"),
    ]);
    assert!(two.contains(">Following 2 open tasks</summary>"), "{two}");
}

/// An agent's Following list orders its tasks as the server pages them, numbers by their value,
/// so the rows Load more brings (`ready-24` on) come after the first page's (`ready-23`), never
/// among them, and a task the board shows it orchestrating takes its place in that order.
#[test]
fn the_following_list_orders_slugs_by_their_numbers_as_the_server_pages_them() {
    use serde_json::json;
    let task = |slug: &str, priority: i64, status: &str, orchestrator: Option<&str>| -> Task {
        let roles: Vec<_> = orchestrator
            .map(|handle| json!({"handle":handle,"role":"orchestrator","since":0.0,"expires":null,"health":"live"}))
            .into_iter()
            .collect();
        serde_json::from_value(json!({
            "slug":slug,"title":slug,"body":"","status":status,"priority":priority,
            "parent":null,"created_by":"fixture","created":1.0,"updated":1.0,
            "links":[],"depends":[],"roles":roles,"instruction":false,
        }))
        .unwrap()
    };
    let page = |numbers: std::ops::RangeInclusive<u32>| -> Vec<Task> {
        numbers
            .map(|n| task(&format!("bulk/ready-{n}"), 3, "ready", None))
            .collect()
    };
    let board = vec![
        task("bulk/ready-5", 3, "review", Some("cobalt-otter")),
        task("app/urgent", 1, "ready", Some("cobalt-otter")),
        task("bulk/ready-12", 3, "done", Some("cobalt-otter")),
        task("bulk/ready-40", 3, "ready", Some("someone-else")),
    ];
    let slugs = |rows: Vec<Task>| {
        followed_tasks(rows, &board, "cobalt-otter")
            .into_iter()
            .map(|t| (t.slug, t.status))
            .collect::<Vec<_>>()
    };
    let expected = |numbers: &[u32]| {
        std::iter::once(("app/urgent".to_string(), "ready".to_string()))
            .chain(numbers.iter().map(|n| {
                let status = if *n == 5 { "review" } else { "ready" };
                (format!("bulk/ready-{n}"), status.to_string())
            }))
            .collect::<Vec<_>>()
    };
    let first: Vec<u32> = (1..=23).filter(|n| *n != 12).collect();
    assert_eq!(slugs(page(1..=23)), expected(&first));
    let mut both = page(1..=23);
    both.extend(page(24..=36));
    let all: Vec<u32> = (1..=36).filter(|n| *n != 12).collect();
    assert_eq!(
        slugs(both),
        expected(&all),
        "the second page follows the first"
    );
    assert_eq!(slugs(Vec::new()), expected(&[5]), "the board's own, alone");
}

#[test]
fn an_agents_line_names_only_what_is_known_and_its_finished_tasks_end_without_a_dot() {
    // The meta line's parts, empty ones left out.
    assert_eq!(meta(&["worker", "", "working", "", ""]), "worker · working");
    assert_eq!(
        meta(&["worker", "opus", "idle", "app/x", "x"]),
        "worker · opus · idle · app/x · x"
    );
    assert_eq!(meta(&["", " ", ""]), "");

    let finished = |count: usize| -> Vec<Task> {
        (1..=count)
            .map(|n| {
                serde_json::from_value(serde_json::json!({
                    "slug":format!("done/{n}"),"title":format!("Task {n}"),"body":"","status":"done",
                    "priority":1,"parent":null,"created_by":"owner","created":1.0,"updated":f64::from(u32::try_from(n).unwrap()),
                    "links":[],"depends":[],"roles":[{"handle":"cobalt-otter","role":"executor","since":1.0,"expires":null,"health":"live"}],
                    "instruction":false,
                }))
                .unwrap()
            })
            .collect()
    };
    let line = |count: usize| {
        let mut bare = agent("worker", None);
        bare.model = String::new();
        bare.state = "working".into();
        let html = dioxus_ssr::render_element(
            rsx! { AgentRow { agent: bare, tasks: Rc::new(finished(count)), work: SharedWork::default() } },
        );
        let at = html.find("Finished: ").unwrap_or_else(|| panic!("{html}"));
        let end = at + html[at..].find("</p>").unwrap();
        (html.clone(), html[at..end].to_string())
    };
    let (html, two) = line(2);
    assert!(
        html.contains("<p class=\"muted\">worker · working</p>"),
        "no empty part between dots: {html}"
    );
    assert!(
        two.ends_with(">Task 1</a>") && two.matches(" · ").count() == 1,
        "one dot between two tasks and none after the last: {two}"
    );
    let (_, seven) = line(7);
    assert!(
        seven.ends_with(">Task 3</a> · and 2 more"),
        "the five newest, then how many more: {seven}"
    );
    assert_eq!(seven.matches(" · ").count(), 5, "{seven}");
}
