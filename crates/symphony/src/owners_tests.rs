use super::*;
use symphony_board::people::ownership::{Did, Line};
use symphony_board::people::{Group, Member};

/// A board naming ana (design) and bo (engineering), with both groups.
fn people() -> People {
    let member = |name: &str, group: &str| Member {
        name: name.into(),
        groups: vec![group.into()],
        ..Member::default()
    };
    let group = |name: &str| Group {
        name: name.into(),
        ..Group::default()
    };
    People {
        members: vec![member("ana", "design"), member("bo", "engineering")],
        groups: vec![group("design"), group("engineering")],
        revision: 3,
    }
}

/// The map giving each `(place, group)`.
fn map(entries: &[(&str, &str)]) -> Map {
    let lines: Vec<Line> = (entries.iter().enumerate())
        .map(|(at, (place, group))| Line {
            n: at as i64 + 1,
            at: 1.0,
            by: "ana".into(),
            did: Did::Map,
            place: Some((*place).into()),
            thing: None,
            name: Some((*group).into()),
            from: None,
        })
        .collect();
    Map::of(&lines)
}

/// The cards of a document holding `lines`.
fn decisions(lines: &[&str]) -> Vec<Decision> {
    crate::register::parse_decisions(lines)
}

#[test]
fn a_members_view_of_a_group_thing_hands_it_to_every_other_name_the_board_holds() {
    let people = people();
    let owning = ownership::owning(
        &people,
        &map(&[("area:app", "design")]),
        None,
        &[ownership::Place::Area("app".into())],
    );
    let said = owner(&people, &owning, "ana", false, true);
    assert_eq!(said.name, "design");
    assert_eq!(said.kind, "group");
    assert_eq!(said.by, "map");
    assert_eq!(said.said, "from the area app");
    assert_eq!(said.hand_to, ["engineering", "ana", "bo", "owner"]);
    assert!(
        owner(&people, &owning, "bo", false, true)
            .hand_to
            .is_empty(),
        "bo is no member of design"
    );
    assert!(
        owner(&people, &owning, "ana", false, false)
            .hand_to
            .is_empty(),
        "a closed thing is handed to no one"
    );
    let machine = owner(&people, &owning, "owner", true, true);
    assert_eq!(machine.hand_to, ["engineering", "ana", "bo", "owner"]);
}

#[test]
fn a_person_and_the_machine_are_said_by_their_kind() {
    let people = people();
    let named = ownership::owning(&people, &Map::default(), Some("bo"), &[]);
    let said = owner(&people, &named, "bo", false, true);
    assert_eq!((said.kind.as_str(), said.by.as_str()), ("person", "named"));
    assert_eq!(said.hand_to, ["design", "engineering", "ana", "owner"]);
    let default = ownership::owning(&people, &Map::default(), None, &[]);
    let said = owner(&people, &default, "ana", false, true);
    assert_eq!(
        (said.name.as_str(), said.kind.as_str(), said.by.as_str()),
        ("owner", "machine", "default")
    );
    assert!(
        said.hand_to.is_empty(),
        "the machine's is handed by it alone"
    );
}

#[test]
fn a_documents_cards_are_owned_by_their_for_else_their_folders_group() {
    let people = people();
    let map = map(&[("folder:docs/design", "design")]);
    let cards = decisions(&[
        "# Palette",
        "",
        "- [ ] **new-palette** — Which palette?",
        "- [ ] **new-type** — *For:* engineering. Which typeface?",
        "- [x] **old-grid** — *For:* nobody-here. The grid.",
    ]);
    let owners = owners_of(
        &people,
        &map,
        "docs/design/palette.md",
        &cards,
        "ana",
        false,
    );
    assert_eq!(owners.len(), 3);
    assert_eq!(owners["new-palette"].name, "design");
    assert_eq!(owners["new-palette"].said, "from the folder docs/design");
    assert_eq!(owners["new-palette"].hand_to.len(), 4, "ana is in design");
    assert_eq!(owners["new-type"].name, "engineering");
    assert_eq!(owners["new-type"].by, "named");
    assert!(owners["new-type"].hand_to.is_empty(), "ana is not in it");
    let old = &owners["old-grid"];
    assert_eq!(old.name, "design", "a name the board lacks falls back");
    assert!(old.said.contains("nobody-here"), "{}", old.said);
    assert!(old.hand_to.is_empty(), "an approved card waits on no one");
    assert_eq!(
        thing("/docs/design/palette.md", &cards[0]),
        Thing::Card {
            slug: "new-palette".into(),
            route: "docs/design/palette.md".into()
        }
    );
}

#[test]
fn a_pending_card_and_a_question_wait_and_an_answered_one_does_not() {
    assert!(waits(State::Pending));
    assert!(waits(State::Question));
    for state in [
        State::AutoApproved,
        State::Approved,
        State::Rejected,
        State::Resolved,
        State::Dismissed,
    ] {
        assert!(!waits(state), "{state:?}");
    }
}
