use super::*;
use crate::gate::Login;
use symphony_board::people::{Member, People, Power, Profile, Signin};
use symphony_plugin::execution::PrincipalKind;

/// A machine folder of its own holding Ana's profile, removed when it drops.
struct Machine(PathBuf);

impl Machine {
    fn with_ana(name: &str) -> Machine {
        let dir = std::env::temp_dir().join(format!("roster-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Profiles::change(&dir, |profiles| {
            profiles.people.push(Profile {
                name: "ana".into(),
                display: "Ana Lima".into(),
                signins: vec![Signin {
                    service: TAILSCALE.into(),
                    subject: "ana@example.com".into(),
                }],
                git: vec!["ana-l".into()],
                ..Profile::default()
            });
            Ok(())
        })
        .unwrap();
        Machine(dir)
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn login(name: &str) -> Visitor {
    Visitor::Login(Login {
        id: 12,
        name: name.into(),
        display: "whoever Tailscale says".into(),
    })
}

/// A board whose people are Ana with `powers`.
fn holding(powers: &[Power]) -> impl FnOnce(&Path) -> Result<People, String> {
    let powers = powers.to_vec();
    move |_| {
        Ok(People {
            members: vec![Member {
                name: "ana".into(),
                powers,
                ..Member::default()
            }],
            ..People::default()
        })
    }
}

#[test]
fn a_login_whose_profile_the_board_gives_a_power_is_on_it_as_her_short_name() {
    let machine = Machine::with_ana("on");
    let root = Path::new("/a/project");
    let ana = member(
        &machine.0,
        root,
        &login("Ana@Example.com"),
        holding(&[Power::Speak]),
    );
    let ana = ana.expect("on the board");
    assert_eq!(ana.name(), "ana");
    assert_eq!(
        ana.sender.display, "Ana Lima",
        "as her profile shows her, not Tailscale"
    );
    assert_eq!(ana.sender.git, ["ana-l"]);
    assert_eq!(ana.principal.kind, PrincipalKind::Human);
    assert_eq!(ana.principal, symphony_board::workflow::person("ana"));
}

#[test]
fn a_visitor_no_profile_or_no_power_names_is_nobody_and_no_failed_read_lets_one_in() {
    let machine = Machine::with_ana("off");
    let root = Path::new("/a/project");
    let never = |_: &Path| -> Result<People, String> { panic!("the board is not asked") };
    assert!(member(&machine.0, root, &Visitor::Node("ci".into()), never).is_none());
    assert!(member(&machine.0, root, &Visitor::Machine, never).is_none());
    assert!(member(&machine.0, root, &login("bob@example.com"), never).is_none());
    assert!(member(&machine.0, root, &login("ana@example.com"), holding(&[])).is_none());
    let nobody = |_: &Path| Ok(People::default());
    assert!(member(&machine.0, root, &login("ana@example.com"), nobody).is_none());
    let unread = |_: &Path| Err("the board is down".to_string());
    assert!(member(&machine.0, root, &login("ana@example.com"), unread).is_none());
    // Profiles that cannot be read let no one in.
    std::fs::write(machine.0.join(symphony_board::people::FILE), "{not json").unwrap();
    let hers = holding(&[Power::Decide]);
    assert!(member(&machine.0, root, &login("ana@example.com"), hers).is_none());
}

#[test]
fn the_roster_of_a_machine_asks_the_board_at_the_root_it_is_given() {
    let machine = Machine::with_ana("of");
    let roster = of(machine.0.clone());
    // A folder that holds no project has no board to read: no visitor is on it.
    let nowhere = std::env::temp_dir().join(format!("roster-{}-nowhere", std::process::id()));
    assert!(roster(&nowhere, &login("ana@example.com")).is_none());
    assert!(roster(&nowhere, &Visitor::Node("ci".into())).is_none());
}

fn signed(service: &str, subject: &str) -> Visitor {
    Visitor::Signed(symphony::signed_in::SignedIn {
        service: service.into(),
        subject: subject.into(),
        display: Some("whoever the service says".into()),
        origin: "https://board.example.com".into(),
    })
}

#[test]
fn a_signed_visitor_is_the_profile_holding_its_services_sign_in_and_only_that_one() {
    let machine = Machine::with_ana("signed");
    Profiles::change(&machine.0, |profiles| {
        profiles.people[0].signins.push(Signin {
            service: "google".into(),
            subject: "110248495921238986420".into(),
        });
        Ok(())
    })
    .unwrap();
    let root = Path::new("/a/project");
    let ana = member(
        &machine.0,
        root,
        &signed("google", "110248495921238986420"),
        holding(&[Power::Speak]),
    )
    .expect("on the board");
    assert_eq!(ana.name(), "ana");
    assert_eq!(ana.sender.display, "Ana Lima", "as her profile shows her");
    assert_eq!(ana.principal, symphony_board::workflow::person("ana"));
    for (service, subject) in [
        ("github", "110248495921238986420"),
        ("google", "ana@example.com"),
        ("google", "999"),
    ] {
        let nobody = member(
            &machine.0,
            root,
            &signed(service, subject),
            holding(&[Power::Decide]),
        );
        assert!(nobody.is_none(), "{service}:{subject}");
    }
    let held = member(
        &machine.0,
        root,
        &signed("google", "110248495921238986420"),
        holding(&[]),
    );
    assert!(held.is_none(), "on no board that gives her no power");
}
