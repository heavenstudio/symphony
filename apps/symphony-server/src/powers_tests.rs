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
        "/api/ask",
        "/api/ask-reply",
        "/api/quick-chat",
        "/api/quick-task",
        symphony_api::TASK_PASS_ROUTE,
        symphony_api::DOCUMENT_PASS_ROUTE,
    ] {
        assert_eq!(
            needs(route),
            Needs::Powers(&[Power::Speak, Power::DirectAgents]),
            "{route}"
        );
    }
    // A task's message, a document's comment and a card's message are speaking alone: the board
    // holds them when their writer may not direct agents.
    for route in [
        "/api/task-message",
        "/api/comment",
        "/api/message",
        symphony_api::UPLOAD_PATH,
    ] {
        assert_eq!(needs(route), Needs::Powers(&[Power::Speak]), "{route}");
    }
    // A hand-over needs only being on the board: the board refuses one by a non-member.
    for route in [
        symphony_api::PROFILE_PATH,
        symphony_api::TASK_HAND_ROUTE,
        symphony_api::CARD_HAND_ROUTE,
    ] {
        assert_eq!(needs(route), Needs::Powers(&[]), "{route}");
    }
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
    for route in ["/api/task-message", "/api/comment", "/api/message"] {
        assert_eq!(
            refused("ana", &speaks, needs(route)),
            None,
            "{route}: her words are held"
        );
    }
    for route in [
        "/api/ask",
        symphony_api::TASK_PASS_ROUTE,
        symphony_api::DOCUMENT_PASS_ROUTE,
    ] {
        assert_eq!(
            refused("ana", &speaks, needs(route)).as_deref(),
            Some("Forbidden: ana may not direct agents on this board"),
            "{route}"
        );
    }
    assert_eq!(
        refused("ana", &Powers::default(), needs("/api/comment")).as_deref(),
        Some("Forbidden: ana may not speak on this board")
    );
    let decides = Powers::of([Power::Decide]);
    assert_eq!(refused("ana", &decides, needs("/api/task-priority")), None);
    assert_eq!(
        refused("ana", &decides, needs(symphony_api::ATTEMPT_CONTROL_ROUTE)).as_deref(),
        Some("Forbidden: ana may not direct agents on this board")
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

/// A board naming ana (speak and direct agents), bo (speak), cy (direct agents), di (speak, and
/// direct agents by the group leads) and ed (every power).
fn people() -> symphony_board::people::People {
    use symphony_board::people::{Group, Member, People};
    let member = |name: &str, groups: &[&str], powers: &[Power]| Member {
        name: name.into(),
        groups: groups.iter().map(|group| group.to_string()).collect(),
        powers: powers.to_vec(),
        revision: 0,
    };
    People {
        members: vec![
            member("ana", &[], &[Power::Speak, Power::DirectAgents]),
            member("bo", &[], &[Power::Speak]),
            member("cy", &[], &[Power::DirectAgents]),
            member("di", &["leads"], &[Power::Speak]),
            member("ed", &[], &Power::ALL),
        ],
        groups: vec![Group {
            name: "leads".into(),
            powers: vec![Power::DirectAgents],
            revision: 0,
        }],
        revision: 1,
    }
}

/// The names of [`people`], the person at the machine's word and a name the board holds no person
/// by, that `may` holds for.
fn those_who(may: fn(&symphony_board::people::People, &str) -> bool) -> Vec<&'static str> {
    let people = people();
    ["ana", "bo", "cy", "di", "ed", "owner", "zed"]
        .into_iter()
        .filter(|name| may(&people, name))
        .collect()
}

/// Who may pass a held message on, by what the pass route needs: speak and direct agents, their
/// own or a group's; a name the board holds no person by never may.
#[test]
fn one_may_pass_a_held_message_holding_speak_and_direct_agents_by_themselves_or_a_group() {
    assert_eq!(those_who(may_pass), ["ana", "di", "ed"]);
}

/// Who may ask about a document and follow an ask up, by what the ask needs: speak and direct
/// agents, their own or a group's. Speaking alone, or directing agents alone, is not asking, and a
/// name the board holds no person by never may. The follow-up needs what the ask does, so one
/// word says both.
#[test]
fn one_may_ask_and_follow_up_holding_speak_and_direct_agents_by_themselves_or_a_group() {
    assert_eq!(those_who(may_ask), ["ana", "di", "ed"]);
    assert_eq!(needs("/api/ask-reply"), needs("/api/ask"));
}
