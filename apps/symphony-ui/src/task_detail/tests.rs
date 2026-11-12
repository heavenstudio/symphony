use super::*;
use dioxus::core::NoOpMutations;
use std::cell::Cell;

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

#[test]
fn owner_controls_show_current_priority_and_cancel_only_open_tasks() {
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "blocked",
        "review",
        "done",
        "cancelled",
    ] {
        for priority in 0..=4 {
            let html = dioxus_ssr::render_element(rsx! {
                OwnerControls { slug: "fixture/task".to_string(), priority, status: status.to_string() }
            });
            assert!(html.contains("for=\"task-priority\""), "{html}");
            assert!(
                html.contains(&format!("<span>p{priority}</span>")),
                "{html}"
            );
            assert!(
                html.contains("aria-haspopup=\"listbox\" aria-expanded=false"),
                "{html}"
            );
            assert_eq!(
                html.contains(">Cancel task</button>"),
                !matches!(status, "done" | "cancelled"),
                "{status}: {html}"
            );
            assert!(!html.contains("Confirm cancellation"), "{html}");
            assert!(
                !html.contains("role=\"status\""),
                "empty feedback takes no space: {html}"
            );
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
fn each_role_chip_on_a_task_carries_its_holders_control() {
    let role = |handle: &str, role: &str| Role {
        handle: handle.into(),
        role: role.into(),
        since: 1.0,
        expires: None,
        health: "live".into(),
    };
    let sessions = std::collections::BTreeMap::from([
        ("amber".to_string(), go("amber", None, Some("t7"))),
        (
            "sub".to_string(),
            go("orch", Some("https://claude.ai/code/s1"), None),
        ),
    ]);
    let roles = vec![
        role("amber", "executor"),
        role("sub", "reviewer"),
        role("bare", "orchestrator"),
    ];
    let html = dioxus_ssr::render_element(rsx! {
        RoleChips { roles: roles.clone(), sessions: sessions.clone(), local: true }
    });
    let chip = |name: &str| {
        let at = html
            .find(&format!(">{name}"))
            .unwrap_or_else(|| panic!("{name}: {html}"));
        let end = html[at..]
            .find("</span><span class=\"chip")
            .map_or(html.len(), |e| at + e);
        html[at..end].to_string()
    };
    assert!(
        chip("executor amber").contains(r#"data-go="focus""#),
        "{html}"
    );
    let sub = chip("reviewer sub");
    assert!(sub.contains(r#"href="https://claude.ai/code/s1""#), "{sub}");
    assert!(sub.contains("go-session-via"), "{sub}");
    assert!(
        !chip("orchestrator bare").contains("Go to session"),
        "{html}"
    );
    let phone = dioxus_ssr::render_element(rsx! { RoleChips { roles, sessions, local: false } });
    assert!(phone.contains(r#"data-go="screen""#), "{phone}");
    assert!(!phone.contains(r#"data-go="focus""#), "{phone}");
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

#[test]
fn a_thread_is_one_list_whose_items_are_its_messages() {
    let mut dom = VirtualDom::new(threaded);
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert_eq!(html.matches("class=\"history\"").count(), 1, "{html}");
    let list = html
        .split_once("<ul class=\"history\">")
        .and_then(|(_, rest)| rest.split_once("</ul>"))
        .map(|(list, _)| list)
        .expect("the thread's list");
    assert_eq!(list.matches("<li ").count(), 2, "{list}");
    assert!(
        list.contains("class=\"entry owner\"") && list.contains("class=\"entry agent\""),
        "{list}"
    );
}

/// A task page whose thread holds the owner's message and an agent's reply.
fn threaded() -> Element {
    let entry = |id: i64, sender: &str| -> symphony_api::TaskEntry {
        serde_json::from_value(serde_json::json!({
            "message":{"id":id,"slug":"fixture/task","sender":sender,"recipient":null,"kind":"message","text":"Said","ts":1.0},
            "stamp":"2026-11-11","blocks":[],
        }))
        .unwrap()
    };
    let page: TaskPage = serde_json::from_value(serde_json::json!({
        "task":{"slug":"fixture/task","title":"Fixture","body":"","status":"review","priority":1,
            "parent":null,"created_by":"fixture","created":1.0,"updated":1.0,
            "links":[],"depends":[],"roles":[],"instruction":false},
        "blocks":[],"children":[],"children_count":0,
    }))
    .unwrap();
    rsx! {
        TaskDetail { page, tasks: Vec::new(), entries: vec![entry(1, "owner"), entry(2, "cobalt-otter")], older: false, loaded: true, offer: None, onhistory: |_| {}, onchildren: |_| {}, onoffer: |_| {} }
    }
}

fn paragraph(text: &str) -> symphony_api::Block {
    symphony_api::Block::Paragraph {
        spans: vec![symphony_api::Span::Text { text: text.into() }],
    }
}

fn chunk(revision: &str, text: &str, next: usize, omitted: usize) -> symphony_api::TaskBrief {
    symphony_api::TaskBrief {
        blocks: vec![paragraph(text)],
        next,
        omitted,
        revision: revision.into(),
    }
}

/// The brief as the page's feed hands it over: its revision and the bytes its page left out.
fn fed_brief(brief: Rc<Cell<(&'static str, usize)>>) -> Element {
    let (revision, omitted) = brief.get();
    rsx! { Brief { slug: "fixture/task".to_owned(), revision, blocks: vec![paragraph("start")], omitted, next: 24_000 } }
}

#[test]
fn a_brief_edited_from_short_to_long_offers_its_remaining_text() {
    let brief = Rc::new(Cell::new(("short", 0)));
    let mut dom = VirtualDom::new_with_props(fed_brief, brief.clone());
    dom.rebuild(&mut NoOpMutations);
    let html = dioxus_ssr::render(&dom);
    assert!(!html.contains("brief-more"), "{html}");
    brief.set(("long", 90_000));
    dom.mark_dirty(ScopeId::APP);
    dom.render_immediate(&mut NoOpMutations);
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains("brief-more"), "{html}");
    assert!(
        html.contains("90000 bytes of it are not shown yet"),
        "{html}"
    );
}

#[test]
fn what_was_read_of_a_brief_ends_with_its_revision() {
    let mut read = BriefRead::default();
    read.add(
        "one",
        24_000,
        150_000,
        Ok(chunk("one", "second part", 124_000, 50_000)),
    );
    let held = read.of("one").expect("the read revision is held");
    assert_eq!(
        (held.blocks.clone(), held.next, held.omitted),
        (vec![paragraph("second part")], 124_000, 50_000)
    );
    assert_eq!(read.of("two"), None, "an edited brief starts from its page");
    read.add(
        "two",
        24_000,
        30_000,
        Ok(chunk("two", "new rest", 54_000, 0)),
    );
    assert_eq!(
        read,
        BriefRead {
            revision: "two".into(),
            blocks: vec![paragraph("new rest")],
            next: 54_000,
            omitted: 0,
            failed: String::new(),
        },
        "nothing read of the first revision follows the second's start"
    );
}

#[test]
fn a_chunk_cut_from_another_revision_is_not_joined_to_the_part_shown() {
    let mut read = BriefRead::default();
    read.add(
        "one",
        24_000,
        150_000,
        Ok(chunk("two", "misaligned", 124_000, 9)),
    );
    assert_eq!(
        (read.blocks.len(), read.next, read.omitted),
        (0, 24_000, 150_000),
        "{read:?}"
    );
    assert!(read.failed.contains("The brief changed"), "{read:?}");
    read.add(
        "one",
        24_000,
        150_000,
        Ok(chunk("one", "rest", 124_000, 50_000)),
    );
    assert_eq!((read.blocks.len(), read.failed.as_str()), (1, ""));
    read.add("one", 24_000, 150_000, Err("offline".into()));
    assert_eq!(
        (read.blocks.len(), read.next, read.failed.as_str()),
        (1, 124_000, "offline"),
        "a failed read keeps what was read and says why"
    );
}

/// A head's ghost kept as drawn holds its title and facts at their heights, with a bar for each
/// line they may hold; one not kept guesses a title of one line wide (two on a phone) and a fact.
#[test]
fn a_head_s_ghost_holds_its_title_and_facts_at_the_heights_kept() {
    let kept = dioxus_ssr::render_element(
        rsx! { GhostHead { height: Some(311), title: Some(97), facts: Some(67) } },
    );
    assert!(kept.contains("height:311px"), "{kept}");
    assert!(
        kept.contains(r#"class="ghost-title kept" style="height:97px""#),
        "{kept}"
    );
    assert!(
        kept.contains(r#"class="ghost-meta ghost-facts" style="height:67px""#),
        "{kept}"
    );
    let bars = |html: &str, part: &str| {
        let start = html.find(part).unwrap();
        let end = start
            + html[start..]
                .find("ghost-pill")
                .unwrap_or(html.len() - start);
        html[start..end].matches("class=\"bone\"").count()
    };
    assert_eq!(bars(&kept, "ghost-title kept"), 6, "{kept}");
    assert_eq!(bars(&kept, "ghost-facts"), 6, "{kept}");
    let guessed =
        dioxus_ssr::render_element(rsx! { GhostHead { height: None, title: None, facts: None } });
    assert!(
        !guessed.contains("kept") && !guessed.contains("ghost-facts"),
        "{guessed}"
    );
    assert!(!guessed.contains("height:"), "{guessed}");
    assert_eq!(bars(&guessed, "ghost-title"), 2, "{guessed}");
}
