//! The changes of a person that write the machine's profile as well as the board's row
//! (`symphony_board::people::change`), as the command line and the People page make them: who
//! may write a profile ([`Writer`]), a sign-in one person's and the person at the machine's alone
//! to give, a refusal that writes neither the board nor the profiles and names no one else to an
//! admin, and a failure whose cause is kept apart.

use crate::common::{Fixture, Seed};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use symphony_board::Store;
use symphony_board::people::change::{self, Failure, Given, Stopped, Writer};
use symphony_board::people::{Did, FILE, LOCK, Power, Profile, Profiles, Signin};

/// The session every change is recorded as made by.
const BY: &str = "amber-heron";

/// A machine folder of the test's own, removed when it ends.
struct Machine(PathBuf);

impl Machine {
    fn new(name: &str) -> Machine {
        let dir = std::env::temp_dir().join(format!("change-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Machine(dir)
    }

    fn profiles(&self) -> Profiles {
        Profiles::read(&self.0).unwrap()
    }

    /// The machine holding `profiles`, as written before the change a test makes.
    fn holding(self, profiles: &[Profile]) -> Machine {
        (Profiles {
            people: profiles.to_vec(),
            ..Profiles::default()
        })
        .write(&self.0)
        .unwrap();
        self
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn board() -> Fixture {
    Fixture::new(Seed::default().session(BY))
}

fn signin(text: &str) -> Signin {
    Signin::parse(text).unwrap()
}

/// `ana`'s profile as another board's admin left it: a sign-in and a git handle.
fn anas() -> Profile {
    Profile {
        name: "ana".into(),
        display: "Ana Lima".into(),
        signins: vec![signin("tailscale:ana@example.com")],
        git: vec!["ana-lima".into()],
        ..Profile::default()
    }
}

fn given(display: &str, signins: &[&str], git: &[&str]) -> Given {
    Given {
        display: Some(display.into()),
        signins: signins.iter().map(|text| signin(text)).collect(),
        git: git.iter().map(|git| (*git).to_string()).collect(),
        ..Given::default()
    }
}

/// Who the board names, by short name.
fn named(store: &Store) -> Vec<String> {
    (store.people().unwrap().members.into_iter())
        .map(|member| member.name)
        .collect()
}

fn refused(stopped: Result<impl std::fmt::Debug, Stopped>) -> String {
    match stopped {
        Err(Stopped::Refused(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn add(store: &Store, dir: &Path, name: &str, given: &Given, writer: Writer) -> change::Added {
    change::add(store, dir, name, &[], &[Power::Speak], given, writer, BY).unwrap()
}

#[test]
fn the_person_at_the_machine_adds_a_person_with_a_profile_and_gives_an_existing_one_more() {
    let f = board();
    let machine = Machine::new("machine-adds").holding(&[anas()]);
    let added = add(
        &f.store,
        &machine.0,
        "ana",
        &given("Ana L.", &["github:ana"], &["ana-l", "ana-lima"]),
        Writer::Machine,
    );
    assert_eq!(
        (added.line.n, added.line.did, added.signins),
        (1, Did::Add, 2)
    );
    let ana = machine.profiles().get("ana").cloned().unwrap();
    assert_eq!(ana.display, "Ana L.");
    assert_eq!(
        ana.git,
        ["ana-lima", "ana-l"],
        "a handle it holds is kept once"
    );
    assert_eq!(named(&f.store), ["ana"]);
}

#[test]
fn a_boards_admin_makes_a_profile_for_someone_who_has_none_with_no_sign_in() {
    let f = board();
    let machine = Machine::new("admin-makes");
    let added = add(
        &f.store,
        &machine.0,
        "bo",
        &given("  Bo Reis ", &[], &[" ", "bo"]),
        Writer::Other,
    );
    assert_eq!(added.signins, 0, "bo reaches no board until one is given");
    let bo = machine.profiles().get("bo").cloned().unwrap();
    assert_eq!(
        (bo.display.as_str(), bo.git.as_slice()),
        ("Bo Reis", ["bo".to_string()].as_slice()),
        "the display name trimmed, an empty handle never kept"
    );
}

/// The sign-in an admin who is not the person at the machine names, refused for a name new to the
/// machine as for one it holds, in words that name no profile and no one else.
#[test]
fn a_boards_admin_gives_no_sign_in_and_their_refusal_names_no_one() {
    let machine = Machine::new("admin-signs").holding(&[anas()]);
    for (name, subject) in [
        ("bo", "tailscale:bo@example.com"),
        ("bo", "tailscale:ana@example.com"),
        ("ana", "tailscale:mallory@example.com"),
    ] {
        let f = board();
        let why = refused(change::add(
            &f.store,
            &machine.0,
            name,
            &[],
            &[],
            &given("", &[subject], &[]),
            Writer::Other,
            BY,
        ));
        assert_eq!(
            why,
            "a sign-in is the person at the machine's to give, as it says who acts under a name \
             on every board of the machine: add them with none, so nothing was written"
        );
        assert!(named(&f.store).is_empty(), "{name}");
        assert_eq!(f.store.people_log().unwrap(), []);
    }
    assert_eq!(machine.profiles().people, [anas()]);
}

/// Two boards of one machine, sharing its machine folder: a person on the first with no profile
/// (as a board moved from another machine holds them, or a failed take-back leaves them) is no
/// name the second board's admin can give a sign-in to and so act as on the first; the person at
/// the machine alone gives it, and the second board then adds them as their profile is.
#[test]
fn a_name_one_board_holds_with_no_profile_is_given_a_sign_in_by_the_person_at_the_machine_alone() {
    let (first, second) = (board(), board());
    let machine = Machine::new("two-boards");
    first
        .store
        .add_person("bo", &[], &[Power::Speak], BY)
        .unwrap();
    assert!(!machine.0.join(FILE).exists(), "no profile of bo");
    let why = refused(change::add(
        &second.store,
        &machine.0,
        "bo",
        &[],
        &[Power::Administer],
        &given("Bo", &["tailscale:mallory@example.com"], &[]),
        Writer::Other,
        BY,
    ));
    assert!(
        why.starts_with("a sign-in is the person at the machine's to give"),
        "{why}"
    );
    assert!(named(&second.store).is_empty());
    assert!(
        machine.profiles().get("bo").is_none(),
        "nor any profile of bo"
    );
    assert_eq!(
        first.store.people_log().unwrap().len(),
        1,
        "the first board as it was"
    );

    assert_eq!(
        change::profile(
            &machine.0,
            "bo",
            &given("Bo Reis", &["tailscale:bo@example.com"], &[]),
            Writer::Machine,
        ),
        Ok(true)
    );
    let added = add(
        &second.store,
        &machine.0,
        "bo",
        &Given::default(),
        Writer::Other,
    );
    assert_eq!(
        added.signins, 1,
        "bo reaches the second board by the sign-in given them"
    );
    assert_eq!(
        machine.profiles().get("bo").map(|bo| bo.signins.len()),
        Some(1)
    );
}

#[test]
fn a_boards_admin_adds_someone_with_a_profile_as_it_is_and_never_changes_it() {
    let f = board();
    let machine = Machine::new("admin-keeps").holding(&[anas()]);
    let added = add(
        &f.store,
        &machine.0,
        "ana",
        &Given::default(),
        Writer::Other,
    );
    assert_eq!(added.signins, 1, "she reaches the board by her own sign-in");
    assert_eq!(machine.profiles().people, [anas()]);

    let f = board();
    let why = refused(change::add(
        &f.store,
        &machine.0,
        "ana",
        &[],
        &[],
        &given("Ana", &[], &["mallory"]),
        Writer::Other,
        BY,
    ));
    assert_eq!(
        why,
        "an admin's add gives a display name or git handles only to a name new to the machine: \
         add ana with none, and they or the person at the machine write theirs, so nothing was \
         written"
    );
    assert_eq!(
        machine.profiles().people,
        [anas()],
        "her profile is as it was"
    );
    assert!(named(&f.store).is_empty(), "nor is anyone on the board");
    assert_eq!(f.store.people_log().unwrap(), []);
}

#[test]
fn a_display_name_of_spaces_or_a_blank_handle_gives_nothing() {
    let blank = Given {
        display: Some("   ".into()),
        signins: vec![],
        git: vec![" ".into()],
        ..Given::default()
    };
    assert!(blank.is_empty());
    assert!(Given::default().is_empty());
    assert!(!given("", &[], &["ana"]).is_empty());
    assert!(!given("", &["github:ana"], &[]).is_empty());
    // An admin adds a person with a profile given nothing that would change it.
    let f = board();
    let machine = Machine::new("admin-blank").holding(&[anas()]);
    add(&f.store, &machine.0, "ana", &blank, Writer::Other);
    assert_eq!(machine.profiles().people, [anas()]);
}

#[test]
fn a_person_adding_as_their_boards_admin_writes_no_profile_they_have() {
    let f = board();
    let machine = Machine::new("own-add").holding(&[anas()]);
    let why = refused(change::add(
        &f.store,
        &machine.0,
        "ana",
        &[],
        &[],
        &given("Ana", &[], &[]),
        Writer::Own,
        "ana",
    ));
    assert!(
        why.starts_with("an admin's add gives a display name"),
        "{why}"
    );
    assert!(named(&f.store).is_empty());
}

#[test]
fn a_sign_in_another_profile_holds_is_refused_and_neither_the_board_nor_the_profiles_change() {
    let f = board();
    let machine = Machine::new("taken").holding(&[anas()]);
    for writer in [Writer::Machine, Writer::Other] {
        let why = refused(change::add(
            &f.store,
            &machine.0,
            "bo",
            &[],
            &[],
            &given("Bo", &["tailscale:ANA@example.com"], &[]),
            writer,
            BY,
        ));
        // Whose it is is said to the person at the machine alone.
        assert_eq!(
            why.contains("ana"),
            writer == Writer::Machine,
            "{writer:?}: {why}"
        );
        if writer == Writer::Machine {
            assert_eq!(
                why,
                "that tailscale sign-in is ana's: a sign-in is one person's, so nothing was \
                 written"
            );
        }
    }
    assert!(named(&f.store).is_empty());
    assert_eq!(f.store.people_log().unwrap(), []);
    assert_eq!(machine.profiles().people, [anas()]);
}

#[test]
fn what_the_board_refuses_writes_no_profile() {
    let f = board();
    let machine = Machine::new("board-refuses");
    let why = refused(change::add(
        &f.store,
        &machine.0,
        BY,
        &[],
        &[],
        &given("Amber", &[], &[]),
        Writer::Machine,
        BY,
    ));
    assert!(
        why.contains(BY),
        "a session's handle is never a person's: {why}"
    );
    assert!(!machine.0.join(FILE).exists());
}

#[test]
fn profiles_that_cannot_be_read_write_nothing_on_the_board() {
    let f = board();
    let machine = Machine::new("unread");
    // A folder where the lock would be: the profiles are never read, so the board is not asked.
    fs::create_dir_all(machine.0.join(LOCK)).unwrap();
    let stopped = change::add(
        &f.store,
        &machine.0,
        "ana",
        &[],
        &[],
        &Given::default(),
        Writer::Machine,
        BY,
    );
    let Err(Stopped::Failed(failure)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert_eq!(
        failure.said,
        "the machine's profiles could not be locked or read, so nothing was written"
    );
    assert!(failure.why.contains(LOCK), "{failure:?}");
    assert_eq!(failure.left, None);
    assert!(named(&f.store).is_empty());
    assert_eq!(f.store.people_log().unwrap(), []);
}

#[test]
fn a_profile_that_cannot_be_written_takes_the_boards_change_back() {
    let f = board();
    let machine = Machine::new("unwritten");
    // The lock made, then the folder closed to new files: the profiles are read and locked, the
    // board takes the person, and the profiles' new file cannot be made.
    Profiles::change(&machine.0, |_| Ok(())).unwrap();
    fs::set_permissions(&machine.0, fs::Permissions::from_mode(0o555)).unwrap();
    let stopped = change::add(
        &f.store,
        &machine.0,
        "ana",
        &[],
        &[Power::Decide],
        &given("Ana", &[], &[]),
        Writer::Machine,
        BY,
    );
    let Err(Stopped::Failed(failure)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert_eq!(
        failure.said,
        "ana's profile could not be written, so the board's change 1 is taken back"
    );
    assert!(failure.why.contains(FILE), "{failure:?}");
    assert_eq!(failure.left, None);
    assert!(named(&f.store).is_empty());
    let log: Vec<Did> = (f.store.people_log().unwrap().into_iter())
        .map(|line| line.did)
        .collect();
    assert_eq!(log, [Did::Add, Did::Remove], "the add and its taking back");
}

#[test]
fn the_person_at_the_machine_writes_any_profile_on_no_board() {
    let machine = Machine::new("profile-machine").holding(&[anas()]);
    let written = change::profile(
        &machine.0,
        "ana",
        &given("Ana", &["github:ana"], &["ana-l"]),
        Writer::Machine,
    );
    assert_eq!(written, Ok(true));
    let ana = machine.profiles().get("ana").cloned().unwrap();
    assert_eq!((ana.display.as_str(), ana.signins.len()), ("Ana", 2));
    let again = change::profile(
        &machine.0,
        "ana",
        &given("Ana", &["GITHUB:Ana"], &["ana-l"]),
        Writer::Machine,
    );
    assert_eq!(
        again,
        Ok(false),
        "a profile holding those already is not written"
    );
    let made = change::profile(&machine.0, "cy", &Given::default(), Writer::Machine);
    assert_eq!(made, Ok(true), "a profile made for a name with none");
}

#[test]
fn a_person_writes_their_own_display_name_and_git_handles_and_no_sign_in() {
    let machine = Machine::new("profile-own").holding(&[anas()]);
    let written = change::profile(
        &machine.0,
        "ana",
        &given("Ana", &[], &["ana-l"]),
        Writer::Own,
    );
    assert_eq!(written, Ok(true));
    assert_eq!(
        machine.profiles().get("ana").unwrap().git,
        ["ana-lima", "ana-l"]
    );
    let why = refused(change::profile(
        &machine.0,
        "ana",
        &given("", &["github:ana"], &[]),
        Writer::Own,
    ));
    assert_eq!(
        why,
        "a sign-in is the person at the machine's to give: nothing was written"
    );
    assert_eq!(machine.profiles().get("ana").unwrap().signins.len(), 1);
}

#[test]
fn nobody_but_the_machines_person_and_the_person_themself_writes_a_profile_on_no_board() {
    let machine = Machine::new("profile-admin").holding(&[anas()]);
    let why = refused(change::profile(
        &machine.0,
        "ana",
        &given("Mallory", &[], &[]),
        Writer::Other,
    ));
    assert!(
        why.starts_with("only the person at the machine, or ana,"),
        "{why}"
    );
    assert_eq!(machine.profiles().people, [anas()]);
}

#[test]
fn a_profile_refuses_no_persons_name_and_a_sign_in_another_holds() {
    let machine = Machine::new("profile-refused").holding(&[anas()]);
    let why = refused(change::profile(
        &machine.0,
        "Ana Lima",
        &Given::default(),
        Writer::Machine,
    ));
    assert!(
        why.starts_with("\"Ana Lima\" is no short name:") && why.ends_with(": nothing was written"),
        "{why}"
    );
    // A pair the board names sessions by is no person's name either, so a profile never takes one.
    let why = refused(change::profile(
        &machine.0,
        "calm-ferret",
        &Given::default(),
        Writer::Machine,
    ));
    assert!(
        why.starts_with("calm-ferret is a name a session's handle can take"),
        "{why}"
    );
    let why = refused(change::profile(
        &machine.0,
        "bo",
        &given("", &["tailscale:ana@example.com"], &[]),
        Writer::Machine,
    ));
    assert!(why.contains("is ana's"), "{why}");
    assert_eq!(machine.profiles().people, [anas()]);
}

#[test]
fn a_profile_that_cannot_be_written_says_so() {
    let machine = Machine::new("profile-unwritten");
    fs::create_dir_all(machine.0.join(LOCK)).unwrap();
    let stopped = change::profile(&machine.0, "ana", &Given::default(), Writer::Machine);
    let Err(Stopped::Failed(failure)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert_eq!(
        failure.said,
        "ana's profile could not be written, so nothing changed"
    );
    assert!(failure.why.contains(LOCK), "{failure:?}");
}

/// A failure said with its cause or without it, and with its caller's way to take off a person
/// it left on the board.
#[test]
fn a_failure_is_told_with_or_without_its_cause_and_its_remedy() {
    let remedy = |name: &str| format!("removing {name} takes it back");
    let failure = Failure {
        said: "bo's profile could not be written, and the board's change 3 still stands".into(),
        why: "/machine/people.json: Permission denied".into(),
        left: Some("bo".into()),
    };
    assert_eq!(
        failure.told(true, remedy),
        "bo's profile could not be written, and the board's change 3 still stands \
         (/machine/people.json: Permission denied): removing bo takes it back"
    );
    assert_eq!(
        failure.told(false, remedy),
        "bo's profile could not be written, and the board's change 3 still stands: removing bo \
         takes it back"
    );
    let failure = Failure {
        left: None,
        ..failure
    };
    assert_eq!(
        failure.told(false, |_| unreachable!("no one left")),
        "bo's profile could not be written, and the board's change 3 still stands"
    );
}

/// A group is never named as a profile of the machine is, the person at the machine's own among
/// them, as a line under the name would read as either's; any other name it takes as before.
#[test]
fn a_group_named_as_a_profile_is_refused_and_any_other_is_made() {
    let f = board();
    let machine = Machine::new("group-named");
    (Profiles {
        people: vec![
            anas(),
            Profile {
                name: "stefano".into(),
                ..Profile::default()
            },
        ],
        machine: Some("stefano".into()),
    })
    .write(&machine.0)
    .unwrap();
    for name in ["ana", "stefano"] {
        let why = refused(change::add_group(
            &f.store,
            &machine.0,
            name,
            &[Power::Decide],
            BY,
        ));
        assert_eq!(
            why,
            format!(
                "{name} is a person's name on this machine, and a person's name and a group's \
                 never meet: name the group otherwise, so nothing was written"
            )
        );
    }
    assert!(
        f.store.people().unwrap().groups.is_empty(),
        "nothing was written"
    );
    let made = change::add_group(&f.store, &machine.0, "design", &[Power::Decide], BY).unwrap();
    assert_eq!((made.did, made.name.as_str()), (Did::GroupAdd, "design"));
    // What the board refuses is said as its refusal.
    let twice = refused(change::add_group(&f.store, &machine.0, "design", &[], BY));
    assert_eq!(
        twice,
        "design is a group of this board: nothing was written"
    );
}

/// The person at the machine is on every board with every power, and no row adds them: an add of
/// their name is refused to anyone, writing neither the board nor the profiles.
#[test]
fn nobody_adds_the_person_at_the_machine_to_a_board() {
    let f = board();
    let machine = Machine::new("add-machine");
    let profiles = Profiles {
        people: vec![Profile {
            name: "stefano".into(),
            ..Profile::default()
        }],
        machine: Some("stefano".into()),
    };
    profiles.write(&machine.0).unwrap();
    for writer in [Writer::Machine, Writer::Other] {
        let why = refused(change::add(
            &f.store,
            &machine.0,
            "stefano",
            &[],
            &[Power::Speak],
            &Given::default(),
            writer,
            BY,
        ));
        assert_eq!(
            why,
            "stefano is the person at the machine, on every board with every power, and no row \
             adds them: nothing was written"
        );
    }
    assert!(named(&f.store).is_empty());
    assert_eq!(machine.profiles(), profiles);
}

/// What gives a profile `addresses` (way, address) and `notices` (kind, way), nothing else.
fn reaching(addresses: &[(&str, &str)], notices: &[(&str, &str)]) -> Given {
    let pairs = |pairs: &[(&str, &str)]| {
        (pairs.iter())
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    };
    Given {
        addresses: pairs(addresses),
        notices: pairs(notices),
        ..Given::default()
    }
}

/// Where `name`'s notice of each kind reaches, as the machine in `dir` holds it.
fn reaches(dir: &Path, name: &str) -> Vec<Option<(&'static str, String)>> {
    let profiles = Profiles::read(dir).unwrap();
    ["card", "question", "verdict"]
        .map(|kind| {
            (profiles.reach(name, kind)).map(|reach| (reach.way(), reach.address().to_string()))
        })
        .to_vec()
}

#[test]
fn a_person_sets_their_own_addresses_and_ways_each_as_its_sender_takes_it() {
    let machine = Machine::new("reach-own").holding(&[anas()]);
    let given = reaching(
        &[
            ("email", " ana@example.com "),
            ("slack", "U024BE7LH"),
            ("whatsapp", "+1 (555) 123-4567"),
        ],
        &[("card", "slack"), ("verdict", "whatsapp")],
    );
    assert_eq!(
        change::profile(&machine.0, "ana", &given, Writer::Own),
        Ok(true)
    );
    let ana = machine.profiles().get("ana").cloned().unwrap();
    assert_eq!(ana.emails, ["ana@example.com"]);
    assert_eq!(
        serde_json::Value::Object(ana.chat.clone()),
        serde_json::json!({"slack": "U024BE7LH", "whatsapp": "+15551234567"})
    );
    assert_eq!(
        reaches(&machine.0, "ana"),
        [
            Some(("slack", "U024BE7LH".into())),
            None,
            Some(("whatsapp", "+15551234567".into()))
        ]
    );
    assert_eq!(
        change::profile(&machine.0, "ana", &given, Writer::Own),
        Ok(false),
        "a profile holding those already is not written"
    );
    // An empty address takes it away, an empty way sends the kind by none, and what is not named
    // stays.
    let taken = reaching(&[("slack", " ")], &[("card", ""), ("question", "email")]);
    assert_eq!(
        change::profile(&machine.0, "ana", &taken, Writer::Own),
        Ok(true)
    );
    let ana = machine.profiles().get("ana").cloned().unwrap();
    assert_eq!(ana.emails, ["ana@example.com"]);
    assert_eq!(
        serde_json::Value::Object(ana.chat),
        serde_json::json!({"whatsapp": "+15551234567"})
    );
    assert_eq!(
        serde_json::Value::Object(ana.preferences),
        serde_json::json!({"verdict": "whatsapp", "question": "email"})
    );
    assert_eq!(
        reaches(&machine.0, "ana"),
        [
            None,
            Some(("email", "ana@example.com".into())),
            Some(("whatsapp", "+15551234567".into()))
        ]
    );
    // Taking away what is not held changes nothing.
    let none = reaching(&[("slack", "")], &[("card", "")]);
    assert_eq!(
        change::profile(&machine.0, "ana", &none, Writer::Own),
        Ok(false)
    );
}

#[test]
fn an_email_set_replaces_the_first_and_a_clear_takes_it_leaving_those_after_it() {
    let machine = Machine::new("reach-email").holding(&[anas()]);
    let email = |text: &str| reaching(&[("email", text)], &[]);
    let set = |text: &str| change::profile(&machine.0, "ana", &email(text), Writer::Own);
    let emails = || machine.profiles().get("ana").unwrap().emails.clone();
    // As the page's one field sets it: A then B holds B alone, and a clear holds none.
    assert_eq!(set("ana@example.com"), Ok(true));
    assert_eq!(set("ana@work.example"), Ok(true));
    assert_eq!(emails(), ["ana@work.example"]);
    assert_eq!(set("ana@work.example"), Ok(false), "set again it is held");
    assert_eq!(set(""), Ok(true));
    assert!(emails().is_empty());
    assert_eq!(set(""), Ok(false), "a clear of none changes nothing");
    // Emails after the first, as a file written by hand holds them, stay as the first is
    // replaced: a copy of the new one among them is dropped, never held twice.
    let three = Profile {
        emails: ["ana@example.com", "ana@home.example", "ana@work.example"]
            .map(String::from)
            .to_vec(),
        ..anas()
    };
    let machine = Machine::new("reach-emails").holding(&[three]);
    let set = |text: &str| change::profile(&machine.0, "ana", &email(text), Writer::Own);
    let emails = || machine.profiles().get("ana").unwrap().emails.clone();
    assert_eq!(set("ana@new.example"), Ok(true));
    assert_eq!(
        emails(),
        ["ana@new.example", "ana@home.example", "ana@work.example"]
    );
    assert_eq!(set("ana@work.example"), Ok(true));
    assert_eq!(emails(), ["ana@work.example", "ana@home.example"]);
    assert_eq!(set("ana@work.example"), Ok(false));
    // A clear takes the first alone: the one after it is the one a notice goes to then.
    assert_eq!(set(" "), Ok(true));
    assert_eq!(emails(), ["ana@home.example"]);
}

#[test]
fn an_address_its_sender_refuses_is_refused_by_its_rule_and_nothing_is_written() {
    let machine = Machine::new("reach-refused").holding(&[anas()]);
    for (way, address, rule) in [
        (
            "email",
            "Ana <ana@example.com>",
            "the email address is no plain one: ASCII `local@domain`",
        ),
        (
            "slack",
            "C024BE7LH",
            "the Slack member id is none: `U`, or `W`",
        ),
        (
            "slack",
            "u024be7lh",
            "the Slack member id is none: `U`, or `W`",
        ),
        (
            "whatsapp",
            "15551234567",
            "the WhatsApp number is none: it does not start with `+`",
        ),
        (
            "whatsapp",
            "+1555",
            "the WhatsApp number is none: it has fewer than 7 digits",
        ),
        (
            "sms",
            "+15551234567",
            "an address is by one of email, slack, whatsapp, and that way is none of them",
        ),
    ] {
        // Beside an address that is fine, so a refusal is seen to write that one neither.
        let fine = if way == "slack" {
            ("email", "ana@example.com")
        } else {
            ("slack", "U024BE7LH")
        };
        let given = reaching(&[fine, (way, address)], &[]);
        let why = refused(change::profile(&machine.0, "ana", &given, Writer::Own));
        assert!(
            why.starts_with(rule) && why.ends_with(": nothing was written"),
            "{way}: {why}"
        );
        assert!(!why.contains(address), "{why}");
        assert_eq!(machine.profiles().people, [anas()], "{way}");
    }
    for (kind, way, said) in [
        (
            "poll",
            "slack",
            "a notice is of one of card, question, verdict, and that kind is none of them",
        ),
        (
            "card",
            "sms",
            "a notice goes by one of email, slack, whatsapp, or by none, and that way is none of them",
        ),
        (
            "card",
            "Slack",
            "a notice goes by one of email, slack, whatsapp, or by none, and that way is none of them",
        ),
    ] {
        let given = reaching(&[], &[("verdict", "email"), (kind, way)]);
        let why = refused(change::profile(&machine.0, "ana", &given, Writer::Machine));
        assert_eq!(why, format!("{said}: nothing was written"));
        assert_eq!(machine.profiles().people, [anas()]);
    }
}

#[test]
fn only_the_person_and_the_person_at_the_machine_write_addresses_and_ways() {
    let f = board();
    let machine = Machine::new("reach-who").holding(&[anas()]);
    let given = reaching(&[("slack", "U024BE7LH")], &[("card", "slack")]);
    // Another person, an admin among them, writes no one's: on no board, nor in an add.
    let why = refused(change::profile(&machine.0, "ana", &given, Writer::Other));
    assert!(
        why.starts_with("only the person at the machine, or ana,"),
        "{why}"
    );
    let why = refused(change::add(
        &f.store,
        &machine.0,
        "cy",
        &[],
        &[Power::Read],
        &given,
        Writer::Other,
        BY,
    ));
    assert_eq!(
        why,
        "a person's addresses and the ways their notices go by are theirs and the person at the \
         machine's to write: nothing was written"
    );
    assert!(named(&f.store).is_empty(), "nor is anyone on the board");
    assert_eq!(machine.profiles().people, [anas()]);
    // The person at the machine writes anyone's, in an add as on no board.
    add(&f.store, &machine.0, "cy", &given, Writer::Machine);
    assert_eq!(
        reaches(&machine.0, "cy")[0],
        Some(("slack", "U024BE7LH".into()))
    );
    assert_eq!(
        change::profile(&machine.0, "ana", &given, Writer::Machine),
        Ok(true)
    );
    assert_eq!(
        reaches(&machine.0, "ana")[0],
        Some(("slack", "U024BE7LH".into()))
    );
}

#[test]
fn what_is_given_prints_no_address() {
    let given = reaching(&[("slack", "U024BE7LH")], &[("card", "slack")]);
    let printed = format!("{given:?}");
    assert!(!printed.contains("U024BE7LH"), "{printed}");
    assert!(printed.contains("\"slack\""), "{printed}");
    assert!(!reaching(&[], &[("card", "")]).is_empty());
    assert!(!reaching(&[("email", "")], &[]).is_empty());
}
