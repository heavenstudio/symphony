//! The changes of a person that write the machine's profile as well as the board's row
//! (`symphony_board::people::change`), as the command line and the People page make them: who
//! may write a profile ([`Writer`]), a sign-in one person's, and a refusal that writes neither the
//! board nor the profiles.

use crate::common::{Fixture, Seed};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use symphony_board::Store;
use symphony_board::people::change::{self, Given, Stopped, Writer};
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
fn a_boards_admin_makes_a_profile_for_someone_who_has_none() {
    let f = board();
    let machine = Machine::new("admin-makes");
    let added = add(
        &f.store,
        &machine.0,
        "bo",
        &given("  Bo Reis ", &["tailscale:bo@example.com"], &[" ", "bo"]),
        Writer::Other,
    );
    assert_eq!(added.signins, 1);
    let bo = machine.profiles().get("bo").cloned().unwrap();
    assert_eq!(
        (bo.display.as_str(), bo.git.as_slice()),
        ("Bo Reis", ["bo".to_string()].as_slice()),
        "the display name trimmed, an empty handle never kept"
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
        &given("", &["tailscale:mallory@example.com"], &[]),
        Writer::Other,
        BY,
    ));
    assert!(why.contains("only the person at the machine"), "{why}");
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
    assert!(why.contains("only the person at the machine"), "{why}");
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
        assert_eq!(
            why,
            "that tailscale sign-in is ana's: a sign-in is one person's, so nothing was written"
        );
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
    let Err(Stopped::Failed(why)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert!(
        why.starts_with("the machine's profiles could not be locked or read") && why.contains(LOCK),
        "{why}"
    );
    assert!(why.ends_with("nothing was written"), "{why}");
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
    let Err(Stopped::Failed(why)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert!(
        why.starts_with("ana's profile could not be written") && why.ends_with("is taken back"),
        "{why}"
    );
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
    let Err(Stopped::Failed(why)) = stopped else {
        panic!("expected a failure: {stopped:?}");
    };
    assert!(
        why.starts_with("ana's profile could not be written"),
        "{why}"
    );
}
