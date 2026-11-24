//! What `people list` and `people log` print of the board's people.

use super::*;
use crate::people::{Did, Group, Member, Signin};

fn people() -> People {
    People {
        members: vec![
            Member {
                name: "ana".into(),
                groups: vec!["staff".into()],
                powers: vec![Power::Decide],
                revision: 2,
            },
            Member {
                name: "bob".into(),
                groups: Vec::new(),
                powers: vec![Power::Speak],
                revision: 3,
            },
        ],
        groups: vec![Group {
            name: "staff".into(),
            powers: Vec::new(),
            revision: 1,
        }],
        revision: 3,
    }
}

fn profiles() -> Profiles {
    let signin = |service: &str, subject: &str| Signin {
        service: service.into(),
        subject: subject.into(),
    };
    Profiles {
        people: vec![Profile {
            name: "ana".into(),
            display: "ana".into(),
            signins: vec![
                signin("tailscale", "ana@example.com"),
                signin("tailscale", "ana@work.example"),
                signin("github", "ana-l"),
            ],
            ..Profile::default()
        }],
    }
}

#[test]
fn a_person_shown_by_their_name_is_listed_once_and_one_with_no_profile_reaches_nothing() {
    assert_eq!(
        lines(&people(), &profiles()),
        [
            "ana  read, decide  groups: staff  signs in by tailscale, github",
            "bob  read, speak  no sign-in: reaches no board",
            "group staff  no power: listed, and sees nothing",
        ]
    );
    let listed = listed(&people(), &profiles());
    assert_eq!(listed["people"][1]["display"], Value::Null);
    assert_eq!(listed["people"][1]["signins"], json!([]));
    assert_eq!(
        listed["people"][0]["signins"],
        json!(["tailscale", "github"])
    );
    assert!(!listed.to_string().contains("example"), "{listed}");
}

#[test]
fn a_group_alone_is_listed_and_an_empty_board_says_who_holds_every_power() {
    let groups = People {
        members: Vec::new(),
        ..people()
    };
    assert_eq!(
        lines(&groups, &Profiles::default()),
        ["group staff  no power: listed, and sees nothing"]
    );
    assert_eq!(
        lines(&People::default(), &Profiles::default()),
        ["nobody is on this board but the person at the machine, who holds every power"]
    );
}

#[test]
fn a_log_line_names_the_group_beside_a_person_and_its_powers_as_the_board_writes_them() {
    let line = |did, name: &str, group: Option<&str>, powers: Vec<Power>| Change {
        n: 7,
        at: 1_800_000_000.0,
        by: "amber-heron".into(),
        did,
        name: name.into(),
        group: group.map(str::to_string),
        powers,
    };
    let at = local_minute(1_800_000_000.0);
    assert_eq!(
        logged(&line(Did::Join, "ana", Some("staff"), Vec::new())),
        format!("{at}  7  amber-heron: join ana staff")
    );
    assert_eq!(
        logged(&line(
            Did::Grant,
            "staff",
            Some("staff"),
            vec![Power::Speak, Power::Administer]
        )),
        format!("{at}  7  amber-heron: grant staff speak,administer")
    );
    assert_eq!(
        logged(&line(Did::Remove, "bob", None, Vec::new())),
        format!("{at}  7  amber-heron: remove bob")
    );
}
