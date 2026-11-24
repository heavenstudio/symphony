//! `tasks people` as the command line holds to it: who is on the board and what each may do,
//! changed as `--as`'s with every change a line of the log; the machine's profiles written mode 600
//! in the machine folder the environment names; a sign-in one person's alone; a refusal that says
//! why and writes nothing; and nothing printed that names a sign-in's subject.

use crate::{Case, scratch};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// A board of its own whose machine folder is a scratch one: the case, a session's handle and the
/// folder.
fn case(name: &str) -> (Case, String, PathBuf) {
    let mut c = Case::new();
    let machine = scratch(name).join("machine");
    (c.env.vars).insert("SYMPHONY_MACHINE_DIR".into(), machine.display().to_string());
    let handle = c.register("orchestrator");
    (c, handle, machine)
}

/// The exit code and stderr of `tasks ARGS` as `who`, which must have failed.
fn fails(c: &Case, who: &str, args: &[&str]) -> (i32, String) {
    let ran = c.run_as(who, args, "");
    assert_ne!(ran.code, 0, "{args:?}: {}", ran.out);
    (ran.code, ran.err)
}

#[test]
fn a_person_is_added_with_a_profile_of_their_own_and_listed_by_what_they_hold() {
    let (c, me, machine) = case("people-add");
    assert_eq!(
        c.ok(&me, &["people", "list"]),
        "nobody is on this board but the person at the machine, who holds every power\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "log"]),
        "no change was made to this board's people\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "group", "add", "staff", "speak"]),
        "the group staff is on this board (change 1), holding speak\n"
    );
    let added = c.ok(
        &me,
        &[
            "people",
            "add",
            "ana",
            "--display",
            "Ana Lima",
            "--signin",
            "tailscale:ana@example.com",
            "--git",
            "ana-l",
            "--power",
            "decide",
            "--group",
            "staff",
        ],
    );
    assert_eq!(
        added,
        "ana is on this board (change 2), holding read, decide: they reach it by their sign-ins\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "add", "bob"]),
        "bob is on this board (change 3), holding no power: listed, and sees nothing: no \
         sign-in is theirs yet, so they reach no board until `people profile` gives one\n"
    );
    let list = c.ok(&me, &["people", "list"]);
    assert_eq!(
        list,
        "ana (Ana Lima)  read, speak, decide  groups: staff  signs in by tailscale\n\
         bob  no power: listed, and sees nothing  no sign-in: reaches no board\n\
         group staff  speak\n"
    );
    assert!(!list.contains("example.com"));
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&me, &["people", "list", "--json"])).unwrap();
    assert_eq!(json["revision"], 3);
    assert_eq!(
        json["people"][0]["powers"],
        serde_json::json!(["read", "speak", "decide"])
    );
    assert_eq!(json["people"][0]["own"], serde_json::json!(["decide"]));
    assert_eq!(
        json["people"][0]["signins"],
        serde_json::json!(["tailscale"])
    );
    assert_eq!(json["people"][0]["display"], "Ana Lima");
    assert_eq!(
        json["groups"][0],
        serde_json::json!({"name": "staff", "powers": ["speak"]})
    );
    assert!(!json.to_string().contains("example.com"));

    let file = machine.join(symphony_board::people::FILE);
    let mode = |path: &PathBuf| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&machine.join(symphony_board::people::LOCK)), 0o600);
    let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
    let ana = profiles.get("ana").unwrap();
    assert_eq!(
        (
            ana.shown(),
            ana.git.clone(),
            ana.signins[0].subject.as_str()
        ),
        ("Ana Lima", vec!["ana-l".to_string()], "ana@example.com")
    );
    let log = c.ok(&me, &["people", "log"]);
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "{log}");
    assert!(
        lines[0].ends_with(&format!("  1  {me}: group-add staff speak")),
        "{log}"
    );
    assert!(
        lines[1].ends_with(&format!("  2  {me}: add ana decide")),
        "{log}"
    );
    assert!(!log.contains("example.com"));
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&me, &["people", "log", "--json"])).unwrap();
    assert_eq!(json[1]["did"], "add");
}

#[test]
fn powers_and_groups_change_by_name_and_a_change_of_nothing_writes_nothing() {
    let (c, me, _) = case("people-change");
    c.ok(&me, &["people", "group", "add", "staff"]);
    c.ok(&me, &["people", "add", "ana"]);
    for (args, said) in [
        (vec!["join", "ana", "staff"], "ana is in staff (change 3)"),
        (
            vec!["join", "ana", "staff"],
            "ana is in staff already: nothing written",
        ),
        (
            vec!["grant", "ana", "speak", "decide"],
            "ana is given speak, decide (change 4)",
        ),
        (
            vec!["grant", "ana", "decide"],
            "ana holds those already: nothing written",
        ),
        (
            vec!["revoke", "ana", "decide"],
            "ana holds decide no more (change 5)",
        ),
        (
            vec!["grant", "--group", "staff", "administer"],
            "the group staff is given administer (change 6)",
        ),
        (
            vec!["revoke", "--group", "staff", "administer"],
            "the group staff holds administer no more (change 7)",
        ),
        (
            vec!["leave", "ana", "staff"],
            "ana is not in staff now (change 8)",
        ),
        (
            vec!["group", "remove", "staff"],
            "the group staff is removed (change 9), its members are out of it, and staff is \
             retired, never given to anyone again",
        ),
        (
            vec!["remove", "ana"],
            "ana is on this board no more (change 10): their lines stay theirs, their profile \
             stays the machine's, and ana is retired, never given to anyone again",
        ),
    ] {
        let mut full = vec!["people"];
        full.extend(args.iter().copied());
        assert_eq!(c.ok(&me, &full), format!("{said}\n"), "{args:?}");
    }
    assert_eq!(c.ok(&me, &["people", "log"]).lines().count(), 10);
}

#[test]
fn a_sign_in_is_one_persons_and_a_refusal_writes_nothing_anywhere() {
    let (c, me, machine) = case("people-refused");
    c.ok(
        &me,
        &[
            "people",
            "add",
            "ana",
            "--signin",
            "tailscale:ana@example.com",
        ],
    );
    let before = (
        c.ok(&me, &["people", "list", "--json"]),
        std::fs::read_to_string(machine.join(symphony_board::people::FILE)).unwrap(),
    );
    let refusals = [
        fails(
            &c,
            &me,
            &[
                "people",
                "add",
                "bob",
                "--signin",
                "TAILSCALE:Ana@Example.com",
            ],
        ),
        fails(&c, &me, &["people", "add", "bob", "--signin", "tailscale"]),
        fails(&c, &me, &["people", "add", "bob", "--power", "admin"]),
        fails(
            &c,
            &me,
            &["people", "add", "bob", "--power", "direct-agents"],
        ),
        fails(&c, &me, &["people", "add", "Bob"]),
        fails(&c, &me, &["people", "add", &me]),
        fails(&c, &me, &["people", "grant", "ana"]),
        fails(&c, &me, &["people", "grant", "--group", "staff", "decide"]),
        fails(&c, &me, &["people", "revoke", "ana", "decide"]),
        fails(&c, &me, &["people", "profile", "Bob"]),
        fails(
            &c,
            &me,
            &[
                "people",
                "profile",
                "bob",
                "--signin",
                "tailscale:ana@example.com",
            ],
        ),
        fails(&c, &me, &["people", "frobnicate"]),
        fails(&c, &me, &["people", "add"]),
        fails(&c, &me, &["people", "grant"]),
        fails(
            &c,
            &me,
            &["people", "grant", "--group", "a", "--group", "b", "decide"],
        ),
    ];
    assert!(refusals.iter().all(|(code, _)| *code == 1), "{refusals:?}");
    let said = |at: usize| refusals[at].1.as_str();
    assert!(
        said(0).contains("that tailscale sign-in is ana's"),
        "{}",
        said(0)
    );
    assert!(!said(0).contains("example.com"), "{}", said(0));
    assert!(
        said(1).contains("a sign-in is SERVICE:SUBJECT"),
        "{}",
        said(1)
    );
    assert!(said(2).contains("\"admin\" is no power"), "{}", said(2));
    assert!(
        said(3).contains("direct agents is given to no one yet"),
        "{}",
        said(3)
    );
    assert!(said(4).contains("is no short name"), "{}", said(4));
    assert!(said(5).contains("is a session's handle"), "{}", said(5));
    assert!(said(6).contains("name at least one power"), "{}", said(6));
    assert!(
        said(7).contains("this board keeps no group staff"),
        "{}",
        said(7)
    );
    assert!(
        said(8).contains("holds none of those powers as its own"),
        "{}",
        said(8)
    );
    assert!(
        said(11).contains("people takes list, log, add NAME"),
        "{}",
        said(11)
    );
    for at in [13, 14] {
        assert!(
            said(at).contains("grant and revoke take NAME POWER…, or --group GROUP POWER…"),
            "{}",
            said(at)
        );
    }
    assert_eq!(
        (
            c.ok(&me, &["people", "list", "--json"]),
            std::fs::read_to_string(machine.join(symphony_board::people::FILE)).unwrap()
        ),
        before
    );
}

#[test]
fn a_profile_is_filled_on_no_board_and_a_person_already_named_keeps_theirs() {
    let (c, me, machine) = case("people-profile");
    assert_eq!(
        c.ok(
            &me,
            &[
                "people",
                "profile",
                "ana",
                "--display",
                "Ana",
                "--git",
                "ana-l"
            ]
        ),
        "ana's profile is written in the machine folder\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "profile", "ana", "--git", "ana-l"]),
        "ana's profile holds those already: nothing written\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "list"]),
        "nobody is on this board but the person at the machine, who holds every power\n",
        "a profile puts no one on a board"
    );
    // Added to the board, she keeps the profile she had and gains the sign-in given.
    let added = c.ok(
        &me,
        &[
            "people",
            "add",
            "ana",
            "--signin",
            "tailscale:ana@example.com",
            "--power",
            "speak",
        ],
    );
    assert!(
        added.ends_with("they reach it by their sign-ins\n"),
        "{added}"
    );
    let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
    let ana = profiles.get("ana").unwrap();
    assert_eq!(
        (ana.display.as_str(), ana.git.len(), ana.signins.len()),
        ("Ana", 1, 1)
    );
}

/// The command line writes as a session's handle and never as a person: a person's name, a
/// decider's among them, is no session, so no line on a thread reads as hers but through the
/// Symphony, and nothing an agent types under her name counts as her approve.
#[test]
fn a_persons_name_is_no_handle_the_command_line_writes_as() {
    let (c, me, _) = case("people-as");
    c.ok(&me, &["people", "add", "ana", "--power", "decide"]);
    c.ok(&me, &["create", "sim/look", "Look"]);
    let before = c.ok(&me, &["show", "sim/look"]);
    let (code, said) = fails(&c, "ana", &["say", "sim/look", "approve 4be91c2"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("no session 'ana'"), "{said}");
    assert_eq!(
        c.ok(&me, &["show", "sim/look"]),
        before,
        "nothing is on the thread"
    );
}

/// A profile that cannot be written puts no one on the board: the profile is written first, so a
/// board never names one the machine holds no profile of, and is never taken off it to undo an add.
#[test]
fn a_person_whose_profile_cannot_be_written_is_not_put_on_the_board() {
    let (c, me, machine) = case("people-unwritten");
    // A folder where the lock would be: the profiles read as none, and no change of them is made.
    std::fs::create_dir_all(machine.join(symphony_board::people::LOCK)).unwrap();
    let (code, said) = fails(&c, &me, &["people", "add", "ana", "--power", "decide"]);
    assert_eq!(code, 2, "a failure, not a refusal: {said}");
    assert!(
        said.contains("ana's profile could not be written"),
        "{said}"
    );
    assert!(said.contains("nothing was written"), "{said}");
    assert_eq!(
        c.ok(&me, &["people", "log"]),
        "no change was made to this board's people\n"
    );
    assert!(!machine.join(symphony_board::people::FILE).exists());
}

/// A removed name is retired on the board that removed it and in the machine's profiles: refused
/// to any new person or group there and on every board of the machine, and to a new profile, with
/// nothing written; the profile of the one removed stays theirs. A refusal by the board writes
/// nothing in the machine folder either.
#[test]
fn a_removed_name_is_retired_on_the_board_and_in_the_machines_profiles() {
    let (c, me, machine) = case("people-retired");
    let (code, said) = fails(&c, &me, &["people", "add", "owner"]);
    assert_eq!(code, 1, "{said}");
    assert!(
        !machine.exists(),
        "a refusal writes nothing in the machine folder"
    );
    c.ok(
        &me,
        &[
            "people",
            "add",
            "ana",
            "--signin",
            "tailscale:ana@example.com",
        ],
    );
    c.ok(&me, &["people", "group", "add", "crew", "speak"]);
    let (code, said) = fails(&c, &me, &["people", "remove", "zed"]);
    assert_eq!(code, 1, "{said}");
    let profiles = || symphony_board::people::Profiles::read(&machine).unwrap();
    assert!(
        profiles().retired.is_empty(),
        "a refused removal retires nothing"
    );
    c.ok(&me, &["people", "remove", "ana"]);
    c.ok(&me, &["people", "group", "remove", "crew"]);
    assert_eq!(profiles().retired, ["ana", "crew"]);
    assert!(profiles().get("ana").is_some(), "her profile stays hers");
    assert_eq!(
        c.ok(&me, &["people", "list"]),
        "nobody is on this board but the person at the machine, who holds every power\n\
         retired, never given again: ana, crew\n"
    );
    let before = (
        c.ok(&me, &["people", "log"]),
        std::fs::read_to_string(machine.join(symphony_board::people::FILE)).unwrap(),
    );
    // Another board of the same machine, which never held either.
    let mut other = crate::Case::new();
    (other.env.vars).insert("SYMPHONY_MACHINE_DIR".into(), machine.display().to_string());
    let theirs = other.register("orchestrator");
    for (case, who, args) in [
        (&c, &me, vec!["people", "add", "ana", "--power", "decide"]),
        (&c, &me, vec!["people", "group", "add", "ana"]),
        (&c, &me, vec!["people", "add", "crew"]),
        (&other, &theirs, vec!["people", "add", "ana"]),
        (
            &c,
            &me,
            vec!["people", "profile", "crew", "--display", "Crew"],
        ),
    ] {
        let (code, said) = fails(case, who, &args);
        assert_eq!(code, 1, "{args:?}: {said}");
        assert!(
            said.contains("is a name removed before, retired"),
            "{args:?}: {said}"
        );
    }
    assert_eq!(
        (
            c.ok(&me, &["people", "log"]),
            std::fs::read_to_string(machine.join(symphony_board::people::FILE)).unwrap(),
        ),
        before
    );
    assert_eq!(
        c.ok(&me, &["people", "profile", "ana", "--display", "Ana Lima"]),
        "ana's profile is written in the machine folder\n",
        "a kept profile stays its person's to change"
    );
}
