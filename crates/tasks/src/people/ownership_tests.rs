use super::*;
use crate::people::{Group, Member};

fn board(members: &[(&str, &[&str])], groups: &[&str]) -> People {
    People {
        members: (members.iter())
            .map(|(name, groups)| Member {
                name: (*name).into(),
                groups: groups.iter().map(|g| (*g).to_string()).collect(),
                ..Member::default()
            })
            .collect(),
        groups: (groups.iter())
            .map(|name| Group {
                name: (*name).into(),
                ..Group::default()
            })
            .collect(),
        revision: 1,
    }
}

/// A board naming ana (design) and bo (engineering), with both groups.
fn named_board() -> People {
    board(
        &[("ana", &["design"]), ("bo", &["engineering"])],
        &["design", "engineering"],
    )
}

fn line(n: i64, did: Did, place: &str, name: Option<&str>) -> Line {
    Line {
        n,
        at: n as f64,
        by: "amber-heron".into(),
        did,
        place: Some(place.into()),
        thing: None,
        name: name.map(str::to_string),
        from: None,
    }
}

fn map(entries: &[(&str, &str)]) -> Map {
    let lines: Vec<Line> = (entries.iter().enumerate())
        .map(|(at, (place, group))| line(at as i64 + 1, Did::Map, place, Some(group)))
        .collect();
    Map::of(&lines)
}

fn card(route: &str) -> Thing {
    Thing::Card {
        slug: "new-palette".into(),
        route: route.into(),
    }
}

#[test]
fn a_place_reads_each_form_and_writes_it_back() {
    for (text, place) in [
        ("folder:docs/design", Place::Folder("docs/design".into())),
        ("folder:/docs/design/", Place::Folder("docs/design".into())),
        ("area:app", Place::Area("app".into())),
        ("area:tools-2", Place::Area("tools-2".into())),
        ("settings", Place::Settings),
        (" default ", Place::Default),
    ] {
        let read = Place::parse(text).expect(text);
        assert_eq!(read, place, "{text}");
        assert_eq!(Place::parse(&read.text()), Ok(read), "{text}");
    }
}

#[test]
fn a_place_of_no_form_is_refused_naming_the_forms() {
    for text in [
        "",
        "docs/design",
        "folder:",
        "folder:docs//design",
        "folder:../x",
        "folder:a b",
    ] {
        let why = Place::parse(text).expect_err(text);
        assert!(
            why.contains("folder") || why.contains("no place"),
            "{text}: {why}"
        );
    }
    for text in ["area:", "area:App", "area:app/x", "area:-x", "area:é"] {
        let why = Place::parse(text).expect_err(text);
        assert!(why.contains("no task's area"), "{text}: {why}");
    }
    let why = Place::parse("page:people").unwrap_err();
    assert!(why.contains("settings or default"), "{why}");
}

#[test]
fn a_place_is_said_in_words() {
    assert_eq!(
        Place::Folder("docs/design".into()).said(),
        "the folder docs/design"
    );
    assert_eq!(Place::Area("app".into()).said(), "the area app");
    assert_eq!(Place::Settings.said(), "the Settings page");
    assert_eq!(Place::Default.said(), "the board's default");
    assert_eq!(Place::Area("app".into()).to_string(), "area:app");
}

#[test]
fn a_card_is_in_each_folder_of_its_document_deepest_first_and_a_task_in_its_area() {
    assert_eq!(
        card("docs/product/analysis/users.md").places(),
        vec![
            Place::Folder("docs/product/analysis".into()),
            Place::Folder("docs/product".into()),
            Place::Folder("docs".into()),
        ]
    );
    assert_eq!(card("README.md").places(), Vec::<Place>::new());
    assert_eq!(
        Thing::Task("app/owning-group".into()).places(),
        vec![Place::Area("app".into())]
    );
    assert_eq!(Thing::Task("plain".into()).places(), Vec::<Place>::new());
    assert_eq!(card("x.md").text(), "card new-palette");
    assert_eq!(Thing::Task("app/x".into()).text(), "task app/x");
}

#[test]
fn the_map_is_each_places_newest_line_and_an_unmap_takes_it_off() {
    let lines = vec![
        line(1, Did::Map, "area:app", Some("design")),
        line(2, Did::Map, "area:app", Some("engineering")),
        line(3, Did::Map, "settings", Some("design")),
        line(4, Did::Unmap, "settings", None),
        line(5, Did::Map, "nowhere:x", Some("design")),
        Line {
            did: Did::Hand,
            place: None,
            thing: Some("task app/x".into()),
            ..line(6, Did::Hand, "", Some("design"))
        },
    ];
    let map = Map::of(&lines);
    assert_eq!(map.group(&Place::Area("app".into())), Some("engineering"));
    assert_eq!(map.group(&Place::Settings), None);
    assert_eq!(map.revision, 6, "the newest line, whatever it did");
    assert_eq!(
        map.entries().collect::<Vec<_>>(),
        vec![(&Place::Area("app".into()), "engineering")]
    );
    assert!(!map.is_empty());
    assert!(Map::of(&[]).is_empty());
    assert_eq!(Map::of(&[]).revision, 0);
}

#[test]
fn a_board_with_nobody_named_gives_every_thing_to_the_person_at_the_machine() {
    let people = board(&[], &["design"]);
    let map = map(&[("area:app", "design"), ("default", "design")]);
    for written in [None, Some("design"), Some("owner")] {
        let owning = owning(
            &people,
            &map,
            written,
            &Thing::Task("app/x".into()).places(),
        );
        assert_eq!(owning.name, MACHINE);
        assert_eq!(owning.kind, Kind::Machine);
        assert_eq!(owning.by, By::Default);
        assert_eq!(owning.unheld, None);
    }
}

#[test]
fn a_mapped_place_gives_its_group_the_deepest_folder_deciding() {
    let people = named_board();
    let map = map(&[
        ("folder:docs", "engineering"),
        ("folder:docs/design", "design"),
    ]);
    let deep = owning(
        &people,
        &map,
        None,
        &card("docs/design/palette/x.md").places(),
    );
    assert_eq!(
        deep,
        Owning {
            name: "design".into(),
            kind: Kind::Group,
            by: By::Map,
            place: Some("folder:docs/design".into()),
            unheld: None,
        }
    );
    assert_eq!(deep.said(), "from the folder docs/design");
    let shallow = owning(&people, &map, None, &card("docs/engineering/x.md").places());
    assert_eq!(shallow.name, "engineering");
    assert_eq!(shallow.place.as_deref(), Some("folder:docs"));
}

#[test]
fn an_unmapped_place_goes_to_the_default_group_else_the_person_at_the_machine() {
    let people = named_board();
    let task = Thing::Task("tools/x".into()).places();
    let mapped = map(&[("area:app", "design"), ("default", "engineering")]);
    let default = owning(&people, &mapped, None, &task);
    assert_eq!(default.name, "engineering");
    assert_eq!(default.kind, Kind::Group);
    assert_eq!(default.by, By::Default);
    assert_eq!(default.place.as_deref(), Some("default"));
    assert_eq!(default.said(), "the board's default group");
    let none = owning(&people, &map(&[("area:app", "design")]), None, &task);
    assert_eq!(none.name, MACHINE);
    assert_eq!(none.kind, Kind::Machine);
    assert_eq!(none.by, By::Default);
    assert_eq!(none.said(), "the board's default");
}

#[test]
fn a_place_mapped_to_a_group_the_board_keeps_no_more_falls_through() {
    let people = named_board();
    let map = map(&[
        ("folder:docs/design", "gone"),
        ("folder:docs", "design"),
        ("default", "gone-too"),
    ]);
    let owning_card = owning(&people, &map, None, &card("docs/design/x.md").places());
    assert_eq!(owning_card.name, "design");
    assert_eq!(owning_card.place.as_deref(), Some("folder:docs"));
    let task = owning(&people, &map, None, &Thing::Task("app/x".into()).places());
    assert_eq!(task.kind, Kind::Machine, "the default's group is gone too");
}

#[test]
fn a_name_written_on_it_overrides_the_map_a_group_a_person_or_the_machine() {
    let people = named_board();
    let map = map(&[("area:app", "engineering")]);
    let places = Thing::Task("app/x".into()).places();
    let group = owning(&people, &map, Some("design"), &places);
    assert_eq!(
        (group.kind, group.by, group.name.as_str()),
        (Kind::Group, By::Named, "design")
    );
    assert_eq!(group.place, None);
    assert_eq!(group.said(), "named on it");
    let person = owning(&people, &map, Some(" ana "), &places);
    assert_eq!(
        (person.kind, person.by, person.name.as_str()),
        (Kind::Person, By::Named, "ana")
    );
    let machine = owning(&people, &map, Some("owner"), &places);
    assert_eq!((machine.kind, machine.by), (Kind::Machine, By::Named));
}

#[test]
fn a_name_the_board_does_not_hold_falls_back_to_the_map_and_is_said() {
    let people = named_board();
    let map = map(&[("area:app", "engineering")]);
    let places = Thing::Task("app/x".into()).places();
    let owning_task = owning(&people, &map, Some("marketing"), &places);
    assert_eq!(owning_task.name, "engineering");
    assert_eq!(owning_task.by, By::Map);
    assert_eq!(owning_task.unheld.as_deref(), Some("marketing"));
    assert_eq!(
        owning_task.said(),
        "from the area app, as this board holds no group or person marketing"
    );
    let blank = owning(&people, &map, Some("  "), &places);
    assert_eq!(blank.unheld, None, "a blank name is no name");
    let unmapped = owning(&people, &Map::default(), Some("marketing"), &places);
    assert_eq!(unmapped.kind, Kind::Machine);
    assert_eq!(unmapped.unheld.as_deref(), Some("marketing"));
}

#[test]
fn a_member_of_its_group_hands_it_and_a_non_member_is_refused() {
    let people = named_board();
    let map = map(&[("area:app", "design")]);
    let owning_task = owning(&people, &map, None, &Thing::Task("app/x".into()).places());
    assert!(may_hand(&people, &owning_task, "ana", false));
    assert_eq!(
        refused_hand(&people, &owning_task, "engineering", "ana", false),
        None
    );
    assert!(!may_hand(&people, &owning_task, "bo", false));
    assert_eq!(
        refused_hand(&people, &owning_task, "engineering", "bo", false).as_deref(),
        Some("bo is no member of the group design, whose it is, so may not hand it on")
    );
    assert!(
        may_hand(&people, &owning_task, "owner", true),
        "the machine's person"
    );
    assert!(!may_hand(&people, &owning_task, "nobody", false));
}

#[test]
fn a_thing_written_to_a_person_is_theirs_to_hand_and_the_machines_its_alone() {
    let people = named_board();
    let places = Thing::Task("app/x".into()).places();
    let hers = owning(&people, &Map::default(), Some("ana"), &places);
    assert!(may_hand(&people, &hers, "ana", false));
    assert!(!may_hand(&people, &hers, "bo", false));
    let machines = owning(&people, &Map::default(), None, &places);
    assert!(!may_hand(&people, &machines, "ana", false));
    assert_eq!(
        refused_hand(&people, &machines, "design", "ana", false).as_deref(),
        Some(
            "it is the person at the machine's, so ana may not hand it on: only the person at the \
             machine does"
        )
    );
    assert_eq!(
        refused_hand(&people, &hers, "bo", "bo", false).as_deref(),
        Some("it is ana's, so bo may not hand it on: only ana or the person at the machine does")
    );
}

#[test]
fn a_hand_over_is_refused_to_a_name_the_board_does_not_hold_and_on_a_board_naming_nobody() {
    let people = named_board();
    let owning_task = owning(&people, &Map::default(), Some("design"), &[]);
    assert_eq!(
        refused_hand(&people, &owning_task, "marketing", "ana", false).as_deref(),
        Some("this board holds no group or person marketing to hand it to")
    );
    assert_eq!(
        refused_hand(&people, &owning_task, "owner", "ana", false),
        None
    );
    let empty = board(&[], &["design"]);
    let machines = owning(&empty, &Map::default(), None, &[]);
    let why = refused_hand(&empty, &machines, "design", "owner", true).unwrap();
    assert!(why.contains("names nobody"), "{why}");
}

#[test]
fn a_line_says_who_did_what() {
    let mapped = line(3, Did::Map, "area:app", Some("design"));
    assert_eq!(mapped.said(), "amber-heron: map area:app design");
    let handed = Line {
        by: "ana".into(),
        place: None,
        thing: Some("task app/x".into()),
        from: Some("design".into()),
        ..line(4, Did::Hand, "", Some("engineering"))
    };
    assert_eq!(
        handed.said(),
        "ana: hand task app/x engineering (was design)"
    );
    assert_eq!(Did::Unmap.word(), "unmap");
    let json = serde_json::to_value(&handed).unwrap();
    assert_eq!(json["did"], "hand");
    assert!(json.get("place").is_none(), "{json}");
    let back: Line = serde_json::from_value(json).unwrap();
    assert_eq!(back, handed);
}

#[test]
fn what_a_name_names_is_the_boards_rows() {
    let people = named_board();
    assert_eq!(named(&people, "design"), Some(Kind::Group));
    assert_eq!(named(&people, "ana"), Some(Kind::Person));
    assert_eq!(named(&people, "owner"), Some(Kind::Machine));
    assert_eq!(named(&people, "marketing"), None);
}

#[test]
fn a_group_tells_each_of_its_members_a_person_themself_and_the_machine_nobody() {
    let people = board(
        &[
            ("ana", &["design"]),
            ("bo", &["engineering", "design"]),
            ("cy", &["engineering"]),
        ],
        &["design", "engineering"],
    );
    let design = owning(&people, &Map::default(), Some("design"), &[]);
    assert_eq!(told(&people, &design), ["ana", "bo"], "the board's order");
    let cy = owning(&people, &Map::default(), Some("cy"), &[]);
    assert_eq!(told(&people, &cy), ["cy"]);
    let machine = owning(&people, &Map::default(), None, &[]);
    assert_eq!(machine.kind, Kind::Machine);
    assert!(told(&people, &machine).is_empty());
    // A group nobody is in tells nobody, and a person the board no longer holds is not told.
    let empty = board(&[("ana", &[])], &["design"]);
    let unpeopled = owning(&empty, &map(&[("default", "design")]), None, &[]);
    assert_eq!(unpeopled.kind, Kind::Group);
    assert!(told(&empty, &unpeopled).is_empty());
    let gone = Owning {
        name: "dee".into(),
        kind: Kind::Person,
        by: By::Named,
        place: None,
        unheld: None,
    };
    assert!(told(&people, &gone).is_empty());
    // A board that names nobody: every thing is the person at the machine's, told to no one.
    let nobody = board(&[], &[]);
    let any = owning(
        &nobody,
        &Map::default(),
        Some("design"),
        &[Place::Area("app".into())],
    );
    assert!(told(&nobody, &any).is_empty());
}
