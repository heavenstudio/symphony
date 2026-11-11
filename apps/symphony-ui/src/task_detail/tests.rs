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
