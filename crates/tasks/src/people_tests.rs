use super::*;
use std::os::unix::fs::PermissionsExt;

/// A machine folder of its own, removed when it drops.
struct Machine(std::path::PathBuf);

impl Machine {
    fn new(name: &str) -> Machine {
        let dir = std::env::temp_dir().join(format!("people-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Machine(dir)
    }

    /// The mode bits `name` in the folder has.
    fn mode(&self, name: &str) -> u32 {
        fs::metadata(self.0.join(name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn member(name: &str, groups: &[&str], powers: &[Power]) -> Member {
    Member {
        name: name.into(),
        groups: groups.iter().map(|g| (*g).to_string()).collect(),
        powers: powers.to_vec(),
        revision: 1,
    }
}

fn group(name: &str, powers: &[Power]) -> Group {
    Group {
        name: name.into(),
        powers: powers.to_vec(),
        revision: 1,
    }
}

fn ana() -> Profile {
    Profile {
        name: "ana".into(),
        display: "Ana Lima".into(),
        signins: vec![Signin {
            service: TAILSCALE.into(),
            subject: "Ana@Example.com".into(),
        }],
        git: vec!["ana-l".into()],
        ..Profile::default()
    }
}

#[test]
fn a_power_is_written_as_the_board_writes_it_and_read_with_a_hyphen_too() {
    let words: Vec<&str> = Power::ALL.iter().map(|power| power.word()).collect();
    assert_eq!(
        words,
        ["read", "speak", "decide", "direct_agents", "administer"]
    );
    assert_eq!(Power::DirectAgents.said(), "direct agents");
    assert_eq!(Power::Decide.said(), "decide");
    assert_eq!(Power::parse("direct-agents"), Ok(Power::DirectAgents));
    assert_eq!(Power::parse(" decide "), Ok(Power::Decide));
    assert_eq!(Power::Speak.to_string(), "speak");
    let refused = Power::parse("admin").unwrap_err();
    assert!(refused.starts_with("\"admin\" is no power"), "{refused}");
    assert!(refused.contains("read, speak, decide, direct_agents, administer"));
    assert_eq!(
        serde_json::to_value(Power::DirectAgents).unwrap(),
        serde_json::json!("direct_agents")
    );
}

#[test]
fn direct_agents_alone_is_given_to_no_one_yet_and_says_why() {
    let withheld: Vec<(Power, &str)> = (Power::ALL.into_iter())
        .filter_map(|power| Some((power, power.withheld()?)))
        .collect();
    assert_eq!(
        withheld,
        [(
            Power::DirectAgents,
            "until a board speaks to its agents for a person, asking them for work is the person \
             at the machine's"
        )]
    );
}

#[test]
fn every_power_includes_read_and_none_puts_no_one_on_the_board() {
    let none = Powers::default();
    assert!(!none.any() && !none.holds(Power::Read));
    assert_eq!(none.held(), []);
    assert_eq!(none.lacking(&[Power::Speak]), Some(Power::Speak));
    assert_eq!(none.lacking(&[]), None);

    let speaks = Powers::of([Power::Speak]);
    assert!(speaks.any() && speaks.holds(Power::Read) && speaks.holds(Power::Speak));
    assert!(!speaks.holds(Power::Decide));
    assert_eq!(speaks.held(), [Power::Read, Power::Speak]);
    assert_eq!(
        speaks.lacking(&[Power::Speak, Power::DirectAgents]),
        Some(Power::DirectAgents)
    );
    assert_eq!(speaks.lacking(&[Power::Read, Power::Speak]), None);

    let all = Powers::all();
    assert_eq!(all.held(), Power::ALL);
    assert_eq!(all.lacking(&Power::ALL), None);
}

#[test]
fn a_persons_powers_are_their_own_and_their_kept_groups_and_none_off_the_board() {
    let people = People {
        members: vec![
            member("ana", &["staff", "gone"], &[Power::Decide]),
            member("bob", &["staff"], &[]),
            member("cleo", &[], &[]),
        ],
        groups: vec![group("staff", &[Power::Speak])],
        revision: 4,
    };
    assert_eq!(
        people.powers_of("ana").held(),
        [Power::Read, Power::Speak, Power::Decide]
    );
    // A group the board keeps no more gives nothing.
    assert_eq!(people.powers_of("bob").held(), [Power::Read, Power::Speak]);
    assert!(
        !people.powers_of("cleo").any(),
        "listed with no power, she sees nothing"
    );
    assert!(!people.powers_of("dan").any(), "no row, no power");
    assert_eq!(people.holding(Power::Decide), ["ana"]);
    assert_eq!(people.holding(Power::Read), ["ana", "bob"]);
    assert_eq!(people.member("bob").map(|m| m.groups.len()), Some(1));
    assert!(people.group("gone").is_none());
}

#[test]
fn a_name_takes_a_short_names_form_and_one_name_space_with_no_session_in_it() {
    let people = People {
        members: vec![member("ana", &[], &[Power::Read])],
        groups: vec![group("staff", &[])],
        revision: 2,
    };
    // A handle no register gives now, as an older word list gave one: a session holds it all the
    // same.
    let handles = vec!["pale-moth".to_string()];
    assert_eq!(refused_name("bob", &people, &handles), None);
    assert_eq!(refused_name("ana-l", &people, &handles), None);
    for bad in [
        "Ana", "owner", "board", "agent", "unknown", "ana-12", "ana--l", "2ana", "",
    ] {
        let why = refused_name(bad, &people, &handles).unwrap();
        assert!(why.contains("is no short name"), "{bad}: {why}");
    }
    let long = "a".repeat(symphony_docs::syntax::SHORT_NAME_MAX + 1);
    assert!(refused_name(&long, &people, &handles).is_some());
    assert_eq!(
        refused_name("ana", &people, &handles).as_deref(),
        Some("ana is a person on this board already")
    );
    assert_eq!(
        refused_name("staff", &people, &handles).as_deref(),
        Some("staff is a group of this board")
    );
    let handle = refused_name("pale-moth", &people, &handles).unwrap();
    assert!(
        handle.starts_with("pale-moth is a session's handle"),
        "{handle}"
    );
    let pair = refused_name("amber-heron", &people, &[]).unwrap();
    assert!(
        pair.starts_with("amber-heron is a name a session's handle can take"),
        "{pair}"
    );
}

/// A register and an add of one name at the same instant never both take it: a name a register can
/// give is refused to every person and group, whatever the board holds, and a profile with them.
/// Each word alone, either order, or a word of either list beside another is a person's name.
#[test]
fn a_name_a_register_can_give_is_no_persons_and_its_words_apart_are() {
    for name in ["amber-heron", "swift-swift", "vivid-merlin", "slate-raven"] {
        let why = refused_person_name(name).unwrap();
        assert!(
            why.starts_with(&format!("{name} is a name a session's handle can take")),
            "{why}"
        );
    }
    for name in [
        "amber",
        "heron",
        "heron-amber",
        "amber-moth",
        "ana-heron",
        "amberheron",
        "amber-heron-l",
        "amber-heronx",
    ] {
        assert_eq!(refused_person_name(name), None, "{name}");
    }
    // A numbered handle, the board's past its fiftieth try, is no short name at all.
    let numbered = refused_person_name("amber-heron-12").unwrap();
    assert!(numbered.contains("is no short name"), "{numbered}");
}

#[test]
fn a_signin_is_service_and_subject_matched_ignoring_case() {
    let signin = Signin::parse(" Tailscale : ana@example.com ").unwrap();
    assert_eq!(
        signin,
        Signin {
            service: "tailscale".into(),
            subject: "ana@example.com".into()
        }
    );
    assert!(signin.is("TAILSCALE", "Ana@Example.COM"));
    assert!(!signin.is("tailscale", "bob@example.com"));
    assert!(!signin.is("github", "ana@example.com"));
    for bad in ["tailscale", ":ana", "tailscale:", " : "] {
        assert!(Signin::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_profile_is_shown_by_its_display_name_else_its_short_name() {
    assert_eq!(ana().shown(), "Ana Lima");
    let plain = Profile {
        name: "bob".into(),
        display: "  ".into(),
        ..Profile::default()
    };
    assert_eq!(plain.shown(), "bob");
}

#[test]
fn profiles_none_written_are_none_and_a_file_that_does_not_parse_is_never_written_over() {
    let machine = Machine::new("unparsed");
    assert_eq!(Profiles::read(&machine.0), Ok(Profiles::default()));
    fs::create_dir_all(&machine.0).unwrap();
    fs::write(machine.0.join(FILE), "{not json").unwrap();
    let read = Profiles::read(&machine.0).unwrap_err();
    assert!(read.contains(FILE), "{read}");
    let changed = Profiles::change(&machine.0, |profiles| {
        profiles.people.push(ana());
        Ok(())
    });
    assert!(changed.is_err());
    assert_eq!(
        fs::read_to_string(machine.0.join(FILE)).unwrap(),
        "{not json"
    );
}

/// A `people.json` that does not parse is said by its file, line and column, the field and the
/// kind of fault, never a value found in it: a sign-in's subject where a list goes, or a number
/// where a subject goes, is in no word of it. A syntax error keeps serde's words, which quote
/// nothing, and a missing field its name, one of a profile's own. A key the file chose, in `chat`
/// or a field no profile has, is never said as where.
#[test]
fn profiles_that_do_not_parse_say_where_and_what_kind_and_never_a_value_in_them() {
    let machine = Machine::new("unparsed-values");
    fs::create_dir_all(&machine.0).unwrap();
    let path = machine.0.join(FILE);
    for (text, said) in [
        (
            "{\"people\": [\n  {\"name\": \"ana\", \"signins\": \"ana@example.com\"}\n]}",
            "line 2, column 46: people[0].signins: a value of a type or form a profile does not take",
        ),
        (
            "{\"people\": [{\"name\": \"ana\", \"signins\": [{\"service\": \"tailscale\", \"subject\": 4155550100}]}]}",
            "line 1, column 86: people[0].signins[0].subject: a value of a type or form a profile does not take",
        ),
        (
            "{\"people\": [{\"display\": \"ana@example.com\"}]}",
            "line 1, column 42: people[0]: missing field `name`",
        ),
        (
            "{\"people\": [{\"name\": \"ana\", \"display\": \"Ana\", \"name\": \"ana@example.com\"}]}",
            "line 1, column 52: people[0]: duplicate field `name`",
        ),
        (
            "{\"people\": [{\"name\": \"ana\", \"emails\": [\"ana@example.com\"",
            "line 1, column 56: people[0].emails: EOF while parsing a list",
        ),
        (
            "{\"people\": [{\"name\": \"ana\", \"chat\": {\"ana@example.com\": [\"4155550100\" 1]}}]}",
            "line 1, column 71: people[0].chat: expected `,` or `]`",
        ),
        (
            "{\"people\": [{\"name\": \"ana\", \"ana@example.com\": {\"subject\": tru}}]}",
            "line 1, column 63: people[0]: expected ident",
        ),
        (
            "[\"ana@example.com\"]",
            // serde reads a struct from a list of its fields too: here its first, the people.
            "line 1, column 18: [0]: a value of a type or form a profile does not take",
        ),
        (
            "{\"people\": []} \"ana@example.com\"",
            "line 1, column 16: trailing characters",
        ),
    ] {
        fs::write(&path, text).unwrap();
        let read = Profiles::read(&machine.0).unwrap_err();
        assert_eq!(
            read,
            format!(
                "{}: {said}, so none of its profiles is read",
                path.display()
            )
        );
        for value in ["ana@example.com", "4155550100"] {
            assert!(!read.contains(value), "{value} in {read}");
        }
    }
    // Read through the field's path, a file that parses is read as serde reads it: numbers kept as
    // written, a field no profile has skipped.
    let text = r#"{"people": [{"name": "ana", "chat": {"slack": "@ana"}, "since": [1],
        "preferences": {"width": 1.50, "phone": 4155550100123456789012}}]}"#;
    fs::write(&path, text).unwrap();
    assert_eq!(
        Profiles::read(&machine.0),
        Ok(serde_json::from_str::<Profiles>(text).unwrap())
    );
    assert_eq!(
        Profiles::read(&machine.0).unwrap().people[0].preferences["phone"].to_string(),
        "4155550100123456789012"
    );
}

#[test]
fn profiles_and_their_lock_are_the_machine_accounts_alone_and_kept_as_written() {
    let machine = Machine::new("written");
    let mut chat = Map::new();
    chat.insert("slack".into(), serde_json::json!("@ana"));
    let found = Profiles::change(&machine.0, |profiles| {
        profiles.people.push(Profile {
            emails: vec!["ana@example.com".into()],
            chat: chat.clone(),
            ..ana()
        });
        Ok(profiles.people.len())
    });
    assert_eq!(found, Ok(1));
    assert_eq!(machine.mode(FILE), MODE);
    assert_eq!(machine.mode(LOCK), MODE);
    let read = Profiles::read(&machine.0).unwrap();
    assert_eq!(
        read.get("ana").map(|p| p.emails.clone()),
        Some(vec!["ana@example.com".into()])
    );
    assert_eq!(read.get("ana").map(|p| p.chat.clone()), Some(chat));
    assert_eq!(
        read.signed_in(TAILSCALE, "ana@example.com")
            .map(|p| p.name.as_str()),
        Some("ana")
    );
    assert!(read.signed_in(TAILSCALE, "bob@example.com").is_none());
    assert!(read.get("bob").is_none());
    // No temporary file is left beside it.
    let left: Vec<String> = (fs::read_dir(&machine.0).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut left = left;
    left.sort();
    assert_eq!(left, [FILE, LOCK]);
}

/// Profiles that cannot be read, or written, say where and leave nothing beside them: a folder
/// where the file would be is neither read as none nor written over.
#[test]
fn profiles_that_cannot_be_read_or_written_say_where_and_leave_no_temporary_file() {
    let machine = Machine::new("blocked");
    fs::create_dir_all(machine.0.join(FILE)).unwrap();
    let read = Profiles::read(&machine.0).unwrap_err();
    assert!(read.contains(FILE), "{read}");
    let written = Profiles::default().write(&machine.0).unwrap_err();
    assert!(written.contains(FILE), "{written}");
    let left: Vec<String> = (fs::read_dir(&machine.0).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, [FILE], "no temporary file is left");
}

#[test]
fn a_lock_made_open_by_another_is_held_to_the_mode_and_a_change_of_nothing_writes_nothing() {
    let machine = Machine::new("loose");
    fs::create_dir_all(&machine.0).unwrap();
    fs::write(machine.0.join(LOCK), "").unwrap();
    fs::set_permissions(machine.0.join(LOCK), fs::Permissions::from_mode(0o644)).unwrap();
    let unchanged = Profiles::change(&machine.0, |_| Ok("nothing"));
    assert_eq!(unchanged, Ok("nothing"));
    assert_eq!(machine.mode(LOCK), MODE);
    assert!(
        !machine.0.join(FILE).exists(),
        "a change of nothing writes no file"
    );
    let refused: Result<(), String> = Profiles::change(&machine.0, |profiles| {
        profiles.people.push(ana());
        Err("refused".into())
    });
    assert_eq!(refused, Err("refused".into()));
    assert!(
        !machine.0.join(FILE).exists(),
        "a change that fails writes nothing"
    );
}

#[test]
fn a_change_line_names_short_names_alone_and_reads_back() {
    let line = Change {
        n: 3,
        at: 1.5,
        by: "amber-heron".into(),
        did: Did::Join,
        name: "ana".into(),
        group: Some("staff".into()),
        powers: Vec::new(),
    };
    let written = serde_json::to_value(&line).unwrap();
    assert_eq!(
        written,
        serde_json::json!({"n": 3, "at": 1.5, "by": "amber-heron", "did": "join",
                           "name": "ana", "group": "staff"})
    );
    assert_eq!(serde_json::from_value::<Change>(written).unwrap(), line);
    let words: Vec<&str> = [
        Did::Add,
        Did::Remove,
        Did::Join,
        Did::Leave,
        Did::Grant,
        Did::Revoke,
        Did::GroupAdd,
        Did::GroupRemove,
    ]
    .iter()
    .map(|did| did.word())
    .collect();
    assert_eq!(
        words,
        [
            "add",
            "remove",
            "join",
            "leave",
            "grant",
            "revoke",
            "group-add",
            "group-remove"
        ]
    );
    assert_eq!(
        serde_json::to_value(Did::GroupRemove).unwrap(),
        serde_json::json!("group-remove")
    );
}
