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

/// [`people`] with both groups deciding: whom a page draws deciding controls for.
#[test]
fn a_member_holding_decide_decides_its_groups_thing_and_the_machine_every_thing() {
    use symphony_board::people::Power;
    let mut people = people();
    for group in &mut people.groups {
        group.powers = vec![Power::Decide];
    }
    let designs = ownership::owning(
        &people,
        &map(&[("area:app", "design")]),
        None,
        &[ownership::Place::Area("app".into())],
    );
    assert!(owner(&people, &designs, "ana", false, true).decides);
    assert!(
        !owner(&people, &designs, "bo", false, true).decides,
        "bo decides, but not design's"
    );
    assert!(owner(&people, &designs, "owner", true, true).decides);
    assert!(
        owner(&people, &designs, "ana", false, false).decides,
        "closed, it is still hers to decide: a page draws no hand-over, nothing else"
    );
    let machines = ownership::owning(&people, &Map::default(), None, &[]);
    assert!(!owner(&people, &machines, "ana", false, true).decides);
    let speaking = self::people();
    assert!(
        !owner(&speaking, &designs, "ana", false, true).decides,
        "a member holding no decide"
    );
    assert!(owner(&people, &machines, "owner", true, true).decides);
}

/// A person the board named, as the machine's roster admits them.
fn named(name: &str) -> symphony_plugin::Actor {
    symphony_plugin::Actor::new(
        symphony_plugin::Sender::new(name, name, vec![]),
        symphony_board::workflow::person(name),
    )
}

/// A board on a fixture naming ana in design and bo in engineering, each group deciding, with
/// `mapped` places given to design.
fn board(mapped: &[ownership::Place]) -> crate::taskview::page_tests::store_fixture::Fixture {
    use symphony_board::people::Power;
    let f = crate::taskview::page_tests::store_fixture::Fixture::empty();
    for group in ["design", "engineering"] {
        f.store.add_group(group, &[Power::Decide], "owner").unwrap();
    }
    for (name, group) in [("ana", "design"), ("bo", "engineering")] {
        (f.store.add_person(name, &[group.into()], &[], "owner")).unwrap();
    }
    for place in mapped {
        f.store.map_place(place, "design", "owner").unwrap();
    }
    f
}

#[test]
fn a_person_is_the_principal_the_board_issues_and_never_the_machines_by_name() {
    assert_eq!(person(&named("ana")), Some("ana"));
    assert_eq!(person(&symphony_board::workflow::local_actor()), None);
    let named_owner = symphony_plugin::Actor::new(
        symphony_plugin::Sender::new("owner", "owner", vec![]),
        symphony_board::workflow::local_owner(),
    );
    assert_eq!(person(&named_owner), None);
    let elsewhere = symphony_plugin::Actor::new(
        symphony_plugin::Sender::new("ana", "Ana", vec![]),
        symphony_plugin::execution::Principal::new(
            symphony_plugin::execution::PrincipalKind::Human,
            "test-host".into(),
            "u-ana".into(),
        ),
    );
    assert_eq!(
        person(&elsewhere),
        None,
        "a gate's own person at the machine"
    );
}

#[test]
fn a_task_card_and_the_settings_page_are_refused_a_non_member_and_never_the_machine() {
    use ownership::Place;
    let f = board(&[
        Place::Folder("docs".into()),
        Place::Area("app".into()),
        Place::Settings,
    ]);
    let task = f.task("app/x");
    let card = &decisions(&["# T", "", "- [ ] **one** — proposal."])[0];
    let machine = symphony_board::workflow::local_actor();
    for actor in [&named("ana"), &machine] {
        assert_eq!(refused_task(&f.store, &task, actor).unwrap(), None);
        assert_eq!(
            refused_card(&f.store, "docs/TOPIC.md", card, actor).unwrap(),
            None
        );
        assert_eq!(refused_settings(&f.store, actor).unwrap(), None);
    }
    let why = "bo is no member of the group design, whose it is, so may not decide it";
    let bo = named("bo");
    assert_eq!(
        refused_task(&f.store, &task, &bo).unwrap().as_deref(),
        Some(why)
    );
    assert_eq!(
        refused_card(&f.store, "docs/TOPIC.md", card, &bo)
            .unwrap()
            .as_deref(),
        Some(why)
    );
    assert_eq!(
        refused_settings(&f.store, &bo).unwrap().as_deref(),
        Some(why)
    );
    // A card written for engineering is bo's group's, wherever it sits.
    let theirs = &decisions(&["# T", "", "- [ ] **two** — *For:* engineering. proposal."])[0];
    assert_eq!(
        refused_card(&f.store, "docs/TOPIC.md", theirs, &bo).unwrap(),
        None
    );
    assert!(
        refused_card(&f.store, "docs/TOPIC.md", theirs, &named("ana"))
            .unwrap()
            .is_some()
    );

    assert!(changes_settings(&f.store, None));
    assert!(changes_settings(&f.store, Some("ana")));
    assert!(!changes_settings(&f.store, Some("bo")));
    assert!(!changes_settings(&f.store, Some("nobody")));
}

#[test]
fn a_board_naming_nobody_refuses_no_one_and_lets_every_reader_change_settings() {
    let f = crate::taskview::page_tests::store_fixture::Fixture::empty();
    let task = f.task("app/x");
    let card = &decisions(&["# T", "", "- [ ] **one** — proposal."])[0];
    let ana = named("ana");
    assert_eq!(refused_task(&f.store, &task, &ana).unwrap(), None);
    assert_eq!(
        refused_card(&f.store, "docs/T.md", card, &ana).unwrap(),
        None
    );
    assert_eq!(refused_settings(&f.store, &ana).unwrap(), None);
    assert!(changes_settings(&f.store, Some("ana")));
}

#[test]
fn on_a_board_mapping_nothing_a_persons_refusal_says_which_group_to_map() {
    let f = board(&[]);
    let task = f.task("app/x");
    let why = refused_task(&f.store, &task, &named("ana"))
        .unwrap()
        .unwrap();
    assert_eq!(
        why,
        "it is the person at the machine's, as this board maps no group it keeps to the area app \
         or to the board's default, so ana may not decide it until an admin maps one \
         (just tasks-people map PLACE GROUP)"
    );
    assert!(!changes_settings(&f.store, Some("ana")));
    assert_eq!(
        refused_task(&f.store, &task, &symphony_board::workflow::local_actor()).unwrap(),
        None
    );
}

#[test]
fn the_waiting_cards_are_owned_as_mains_file_holds_them_each_document_read_once() {
    let tree = crate::test_tree::Tree::checkout();
    tree.write(
        "docs/design/palette.md",
        "# Palette\n\n<!-- register:start -->\n\
         - [ ] **new-palette** — Which palette?\n\
         - [ ] **new-type** — *For:* bo. Which typeface?\n\
         <!-- register:end -->\n",
    );
    let people = people();
    let map = map(&[("folder:docs/design", "design"), ("default", "engineering")]);
    let card = |doc: &str, slug: &str| (doc.to_string(), slug.to_string());
    let owners = waiting(
        &people,
        &map,
        &tree.path(""),
        &[
            card("docs/design/palette.md", "new-palette"),
            card("docs/design/palette.md", "new-type"),
            // A branch's proposal of a card main's file holds, and of one it does not.
            card("@app-x/docs/design/palette.md", "new-type"),
            card("@app-x/docs/design/palette.md", "brand-new"),
            // A document no file is: its places alone, then the default.
            card("docs/gone.md", "lost"),
        ],
    );
    let said: Vec<(&str, Kind, By)> = (owners.iter())
        .map(|owning| (owning.name.as_str(), owning.kind, owning.by))
        .collect();
    assert_eq!(
        said,
        [
            ("design", Kind::Group, By::Map),
            ("bo", Kind::Person, By::Named),
            ("bo", Kind::Person, By::Named),
            ("design", Kind::Group, By::Map),
            ("engineering", Kind::Group, By::Default),
        ]
    );
    // On a board that names nobody, every card is the person at the machine's.
    let nobody = waiting(
        &People::default(),
        &map,
        &tree.path(""),
        &[card("docs/design/palette.md", "new-type")],
    );
    assert_eq!(nobody[0].kind, Kind::Machine);
    assert_eq!(split_route("/@scope/docs/a.md"), ("scope", "docs/a.md"));
    assert_eq!(split_route("docs/a.md"), ("", "docs/a.md"));
}

#[test]
fn a_branchs_proposal_is_owned_as_its_worktrees_file_holds_it_its_written_name_read() {
    use crate::test_layouts::git;
    let tree = crate::test_tree::Tree::new();
    let main = tree.path("main");
    let register = |cards: &str| {
        format!(
            "# Palette\n\n<!-- register:start -->\n- [ ] **new-palette** — Which palette?\n\
             {cards}<!-- register:end -->\n"
        )
    };
    // Main's file holds a card written for bo that the worktree's does not.
    tree.write(
        "main/docs/design/palette.md",
        &register("- [ ] **kept** — *For:* bo. Which grid?\n"),
    );
    git(&main, &["init", "-q"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "main"]);
    let proposal = tree.path("proposal");
    let branch = proposal.to_str().unwrap();
    git(
        &main,
        &["worktree", "add", "-q", "-b", "design/policy", branch],
    );
    // Written for engineering by its author, handed to bo (its line rewritten so), and one with
    // no name: each in design's folder.
    tree.write(
        "proposal/docs/design/palette.md",
        &register(
            "- [ ] **written** — *For:* engineering. Which build tool?\n\
             - [ ] **handed** — *For:* bo. Which typeface?\n\
             - [ ] **plain** — Which colour?\n",
        ),
    );
    let people = people();
    let map = map(&[("folder:docs/design", "design"), ("default", "engineering")]);
    let card = |doc: &str, slug: &str| (doc.to_string(), slug.to_string());
    let proposed = "@design~policy/docs/design/palette.md";
    let owners = waiting(
        &people,
        &map,
        &main,
        &[
            card(proposed, "written"),
            card(proposed, "handed"),
            card(proposed, "plain"),
            // A proposal of main's card its worktree does not hold: as main's file holds it.
            card(proposed, "kept"),
            // A branch with no worktree: as main's file holds the card.
            card("@gone~branch/docs/design/palette.md", "kept"),
            // A card neither file holds: by its places alone.
            card("@gone~branch/docs/design/palette.md", "written"),
        ],
    );
    let said: Vec<(&str, Kind, By)> = (owners.iter())
        .map(|owning| (owning.name.as_str(), owning.kind, owning.by))
        .collect();
    assert_eq!(
        said,
        [
            ("engineering", Kind::Group, By::Named),
            ("bo", Kind::Person, By::Named),
            ("design", Kind::Group, By::Map),
            ("bo", Kind::Person, By::Named),
            ("bo", Kind::Person, By::Named),
            ("design", Kind::Group, By::Map),
        ],
        "the worktree's file, then main's, then the places: each case says which it read"
    );
}
