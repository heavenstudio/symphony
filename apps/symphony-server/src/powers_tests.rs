use super::*;

/// Every write route the server answers: a save, the upload, an attempt's control and view, and
/// the agent focus.
fn routes() -> Vec<&'static str> {
    let mut routes = symphony::save::ROUTES.to_vec();
    routes.extend([
        symphony_api::UPLOAD_PATH,
        symphony_api::ATTEMPT_CONTROL_ROUTE,
        symphony_api::ATTEMPT_VIEW_ROUTE,
        symphony_api::AGENT_FOCUS_PATH,
    ]);
    routes
}

#[test]
fn every_write_route_is_placed_and_only_the_agent_focus_is_the_machines_alone() {
    for route in routes() {
        let placed = needs(route);
        assert_eq!(
            placed == Needs::Machine,
            route == symphony_api::AGENT_FOCUS_PATH,
            "{route}: {placed:?}"
        );
        // Every route the server answers as a write is one this table names.
        assert!(crate::write::methods(route).is_some(), "{route}");
    }
    assert_eq!(
        needs("/api/a-route-of-tomorrow"),
        Needs::Machine,
        "unplaced: the machine's"
    );
}

#[test]
fn deciding_speaking_to_agents_uploading_and_directing_attempts_need_what_the_rulings_say() {
    for route in [
        "/api/postpone",
        "/api/decision",
        "/api/task-priority",
        "/api/task-field",
        "/api/task-cancel",
        symphony_api::TASK_ACTION_ROUTE,
        symphony::save::WORKFLOW_APPLY,
        symphony_api::SETTING_WRITE_PATH,
    ] {
        assert_eq!(needs(route), Needs::Powers(&[Power::Decide]), "{route}");
    }
    for route in [
        "/api/comment",
        "/api/message",
        "/api/task-message",
        "/api/ask",
        "/api/ask-reply",
        "/api/quick-chat",
        "/api/quick-task",
    ] {
        assert_eq!(
            needs(route),
            Needs::Powers(&[Power::Speak, Power::DirectAgents]),
            "{route}"
        );
    }
    assert_eq!(
        needs(symphony_api::UPLOAD_PATH),
        Needs::Powers(&[Power::Speak])
    );
    for route in [
        symphony_api::ATTEMPT_CONTROL_ROUTE,
        symphony_api::ATTEMPT_VIEW_ROUTE,
    ] {
        assert_eq!(
            needs(route),
            Needs::Powers(&[Power::Decide, Power::DirectAgents]),
            "{route}"
        );
    }
}

#[test]
fn a_refusal_names_the_person_and_the_first_power_they_lack() {
    let speaks = Powers::of([Power::Speak]);
    assert_eq!(
        refused("ana", &speaks, needs(symphony_api::UPLOAD_PATH)),
        None
    );
    assert_eq!(
        refused("ana", &speaks, needs("/api/decision")).as_deref(),
        Some("Forbidden: ana may not decide on this board")
    );
    assert_eq!(
        refused("ana", &speaks, needs("/api/comment")).as_deref(),
        Some("Forbidden: this board holds no words for agents from ana yet")
    );
    assert_eq!(
        refused("ana", &Powers::default(), needs("/api/comment")).as_deref(),
        Some("Forbidden: ana may not speak on this board")
    );
    let decides = Powers::of([Power::Decide]);
    assert_eq!(refused("ana", &decides, needs("/api/task-priority")), None);
    assert_eq!(
        refused("ana", &decides, needs(symphony_api::ATTEMPT_CONTROL_ROUTE)).as_deref(),
        Some("Forbidden: this board holds no words for agents from ana yet")
    );
    assert_eq!(
        refused("ana", &Powers::all(), Needs::Machine).as_deref(),
        Some("Forbidden: this route answers the person at this machine alone")
    );
    // Holding every power, as the person at the machine does, every placed route passes.
    for route in routes() {
        if needs(route) != Needs::Machine {
            assert_eq!(
                refused("owner", &Powers::all(), needs(route)),
                None,
                "{route}"
            );
        }
    }
}
