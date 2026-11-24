//! `tasks people` as the command line holds to it: who is on the board and what each may do,
//! changed as `--as`'s with every change a line of the log; the machine's profiles written mode 600
//! in the machine folder the environment names; a sign-in one person's alone; a refusal that says
//! why and writes nothing; a profile's addresses and ways written by the person at the machine;
//! and nothing printed that names a sign-in's subject, an address or a way.

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
         sign-in is theirs yet, so they reach no board until `just tasks-people profile` gives \
         one\n"
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
            "the group staff is removed (change 9), and its members are out of it",
        ),
        (
            vec!["remove", "ana"],
            "ana is on this board no more (change 10): their lines stay theirs, and their \
             profile stays the machine's",
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
        fails(&c, &me, &["people", "profile", &me]),
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
    // The session's handle is a pair the board names sessions by: no person's name, on a board or
    // in a profile.
    for at in [5, 15] {
        assert!(
            said(at).contains(&format!("{me} is a name a session's handle can take")),
            "{}",
            said(at)
        );
    }
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

#[test]
fn a_profiles_addresses_and_ways_are_written_by_the_machine_and_printed_by_nothing() {
    let (c, me, machine) = case("people-reach");
    c.ok(&me, &["people", "add", "ana", "--power", "speak"]);
    let reach = |kind: &str| {
        let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
        profiles
            .reach("ana", kind)
            .map(|reach| (reach.way(), reach.address().to_string()))
    };
    assert_eq!(
        c.ok(
            &me,
            &[
                "people",
                "profile",
                "ana",
                "--email",
                "ana@example.com",
                "--slack",
                "U024BE7LH",
                "--whatsapp",
                "+1 555 123 4567",
                "--notice",
                "card=slack",
                "--notice",
                "verdict=whatsapp",
            ]
        ),
        "ana's profile is written in the machine folder\n"
    );
    assert_eq!(reach("card"), Some(("slack", "U024BE7LH".into())));
    assert_eq!(reach("question"), None);
    assert_eq!(reach("verdict"), Some(("whatsapp", "+15551234567".into())));
    let listed = c.ok(&me, &["people", "list"]);
    for private in ["ana@example.com", "U024BE7LH", "555", "slack", "whatsapp"] {
        assert!(!listed.contains(private), "{private} in {listed}");
    }
    // A way of none and an empty address each take one away.
    c.ok(
        &me,
        &[
            "people", "profile", "ana", "--notice", "card=", "--slack", "",
        ],
    );
    assert_eq!(reach("card"), None);
    let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
    assert!(!profiles.get("ana").unwrap().chat.contains_key("slack"));
    // A refusal says the rule, never the value, and writes nothing.
    let before = std::fs::read(machine.join(symphony_board::people::FILE)).unwrap();
    for (args, said) in [
        (
            ["--notice", "card"],
            "a notice is KIND=WAY, as card=slack, or KIND= for none: nothing was written",
        ),
        (
            ["--notice", "poll=slack"],
            "a notice is of one of card, question, verdict, and that kind is none of them: \
             nothing was written",
        ),
        (
            ["--slack", "C0SECRET1"],
            "the Slack member id is none: `U`, or `W` in an Enterprise organization, then capital \
             letters and digits, as U024BE7LH: nothing was written",
        ),
    ] {
        let mut asked = vec!["people", "profile", "ana"];
        asked.extend(args);
        let (code, err) = fails(&c, &me, &asked);
        assert_eq!(code, 1, "{err}");
        assert!(err.contains(said), "{args:?}: {err}");
        assert!(!err.contains("C0SECRET1"), "{err}");
    }
    assert_eq!(
        std::fs::read(machine.join(symphony_board::people::FILE)).unwrap(),
        before
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

/// Profiles that cannot be had (a folder where the lock would be) stop an add before the board is
/// asked: nothing is written, on the board or in the machine folder.
#[test]
fn an_add_whose_profiles_cannot_be_locked_writes_nothing_anywhere() {
    let (c, me, machine) = case("people-unlocked");
    std::fs::create_dir_all(machine.join(symphony_board::people::LOCK)).unwrap();
    let (code, said) = fails(&c, &me, &["people", "add", "ana", "--power", "decide"]);
    assert_eq!(code, 2, "a failure, not a refusal: {said}");
    assert!(
        said.contains("the machine's profiles could not be locked or read")
            && said.contains("nothing was written"),
        "{said}"
    );
    assert_eq!(
        c.ok(&me, &["people", "log"]),
        "no change was made to this board's people\n"
    );
    assert!(!machine.join(symphony_board::people::FILE).exists());
}

/// A profile that cannot be written once the board took the person takes the board's change back,
/// so the board names no one whose sign-ins the machine does not hold.
#[test]
fn a_person_whose_profile_cannot_be_written_is_taken_off_the_board_again() {
    let (c, me, machine) = case("people-unwritten");
    // The lock is there and the folder takes no new file: the profiles are locked and read as
    // none, and their write is refused once the board has the person.
    std::fs::create_dir_all(&machine).unwrap();
    std::fs::write(machine.join(symphony_board::people::LOCK), "").unwrap();
    let writable = |mode| {
        std::fs::set_permissions(&machine, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    writable(0o500);
    let ran = c.run_as(&me, &["people", "add", "ana", "--power", "decide"], "");
    writable(0o700);
    assert_eq!(ran.code, 2, "a failure, not a refusal: {}", ran.err);
    assert!(
        ran.err.contains("ana's profile could not be written"),
        "{}",
        ran.err
    );
    assert!(ran.err.contains("is taken back"), "{}", ran.err);
    assert_eq!(
        c.ok(&me, &["people", "list"]),
        "nobody is on this board but the person at the machine, who holds every power\n"
    );
    let log = c.ok(&me, &["people", "log"]);
    assert_eq!(log.lines().count(), 2, "the add and its taking back: {log}");
    assert!(!machine.join(symphony_board::people::FILE).exists());
}

/// Two adds of one sign-in waiting on the profiles' lock together: the first to take it is added
/// with the sign-in, and the second is refused with nothing written, on the board or in the
/// machine folder. Each add holds the lock from its check of the sign-ins through the board's
/// change and the profile's write, so the second reads the first's profile.
///
/// The test holds the lock until both adds wait on it. An add opens `people.lock` and holds it to
/// mode 600 right before it waits on it (`Profiles::change`), so the test sets it to 0644, starts
/// an add and waits for 600, then does the same for the other: both are past whatever they do
/// before the lock when it is let go. An add that checked the sign-ins and wrote the board before
/// the lock, as one did once, would then have found no profile holding the sign-in, both would be
/// on the board, and both would write it.
#[test]
fn two_adds_of_one_sign_in_waiting_on_the_lock_give_it_one_person_and_nothing_to_the_other() {
    let (c, me, machine) = case("people-at-once");
    std::fs::create_dir_all(&machine).unwrap();
    let lock = machine.join(symphony_board::people::LOCK);
    let held = std::fs::File::create(&lock).unwrap();
    held.lock().unwrap();
    let mode = |mode| std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(mode));
    let ran = std::thread::scope(|scope| {
        let mut adds = Vec::new();
        for name in ["ana", "bob"] {
            mode(0o644).unwrap();
            let (c, me) = (&c, &me);
            adds.push(scope.spawn(move || {
                let args = [
                    "people",
                    "add",
                    name,
                    "--signin",
                    "tailscale:one@example.com",
                ];
                (name, c.run_as(me, &args, ""))
            }));
            let waiting = test_time::wait::until_true(
                format!("{name}'s add waiting on the lock"),
                test_time::backstop(std::time::Duration::from_secs(30)),
                || {
                    std::fs::metadata(&lock)
                        .is_ok_and(|seen| seen.permissions().mode() & 0o777 == 0o600)
                },
            );
            if let Err(stall) = waiting {
                held.unlock().unwrap();
                panic!("{stall}");
            }
        }
        held.unlock().unwrap();
        adds.into_iter()
            .map(|add| add.join().unwrap())
            .collect::<Vec<_>>()
    });
    let (added, refused): (Vec<_>, Vec<_>) = ran.iter().partition(|(_, ran)| ran.code == 0);
    let ([(winner, _)], [(loser, lost)]) = (added.as_slice(), refused.as_slice()) else {
        let said: Vec<_> = (ran.iter())
            .map(|(name, ran)| (name, ran.code, ran.err.as_str()))
            .collect();
        panic!("one added and one refused: {said:?}");
    };
    assert_eq!(lost.code, 1, "a refusal: {}", lost.err);
    assert!(
        lost.err
            .contains(&format!("that tailscale sign-in is {winner}'s")),
        "{}",
        lost.err
    );
    let log = c.ok(&me, &["people", "log"]);
    assert_eq!(log.lines().count(), 1, "one add, and no taking back: {log}");
    assert!(log.contains(&format!("add {winner}")), "{log}");
    let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
    assert!(profiles.get(loser).is_none(), "no profile for {loser}");
    assert_eq!(
        profiles
            .signed_in("tailscale", "one@example.com")
            .map(|p| p.name.as_str()),
        Some(*winner)
    );
}

/// The forms `people` takes, as its own usage line says them to an unknown action (`people takes
/// list, log, add NAME, …`), each with a person, a group and a power for its words.
fn forms(said: &str) -> Vec<Vec<String>> {
    let listed = (said.split_once("people takes "))
        .and_then(|(_, rest)| rest.split_once(", not "))
        .unwrap_or_else(|| panic!("no usage line in {said}"))
        .0;
    (listed.split(", ").flat_map(|part| part.split(" or ")))
        .map(|form| {
            (form.split(' '))
                .map(|word| match word {
                    "NAME" => "bob".to_string(),
                    "GROUP" => "staff".to_string(),
                    "POWER…" | "[POWER…]" => "speak".to_string(),
                    "PLACE" => "area:app".to_string(),
                    word => word.to_string(),
                })
                .collect()
        })
        .collect()
}

/// The actions the help of `people` lists under ACTION.
fn help_actions(help: &str) -> Vec<String> {
    let listed: Vec<&str> = (help.lines())
        .skip_while(|line| *line != "positional arguments:")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with("WORDS "))
        .collect();
    let joined = listed.join(" ").replacen("ACTION", "", 1);
    (joined.split(',').flat_map(|part| part.split(" or ")))
        .map(|word| word.trim().to_string())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Under the marker `just tasks` sets, as an agent runs it unasked, every action but the three
/// reads (`list`, `log` and `map` alone) is refused with nothing written: the forms are read from the command's own usage line, checked
/// against the help's list of actions, and each run, so an action added later is refused until it
/// is written as a read. A word that is no action, or an action without its words, keeps its usage
/// error, and the reads run. A change runs by `just tasks-people`, which sets no marker.
#[test]
fn under_the_unasked_recipes_marker_every_action_but_the_reads_is_refused() {
    let (mut c, me, machine) = case("people-unasked");
    c.ok(&me, &["people", "add", "ana", "--power", "speak"]);
    c.ok(&me, &["people", "group", "add", "staff"]);
    let before = c.ok(&me, &["people", "log"]);
    let map_before = c.ok(&me, &["people", "map"]);
    (c.env.vars).insert("SYMPHONY_UNASKED".into(), "1".into());
    let (code, unknown) = fails(&c, &me, &["people", "frobnicate"]);
    assert_eq!(code, 1, "{unknown}");
    let forms = forms(&unknown);
    let mut actions: Vec<String> = Vec::new();
    for form in &forms {
        if !actions.contains(&form[0]) {
            actions.push(form[0].clone());
        }
    }
    assert_eq!(actions, help_actions(&c.ok(&me, &["help", "people"])));
    let mut reads = Vec::new();
    for form in &forms {
        let args: Vec<&str> = ["people"]
            .into_iter()
            .chain(form.iter().map(String::as_str))
            .collect();
        let ran = c.run_as(&me, &args, "");
        if ran.code == 0 {
            reads.push(form[0].as_str());
            continue;
        }
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.err);
        let action = &form[0];
        assert!(
            ran.err.contains(&format!(
                "`just tasks` refuses people {action}: a change to a board's people"
            )) && ran
                .err
                .contains(&format!("`just tasks-people {action} …`, which asks")),
            "{args:?}: {}",
            ran.err
        );
    }
    assert_eq!(reads, ["list", "log", "map"], "the reads alone run");
    // Wherever the command line puts the action, it is the one refused.
    let as_me = format!("--as={me}");
    for (args, action) in [
        (vec!["people", "--display", "Bob", "add", "bob"], "add"),
        (vec!["people", "--", "add", "bob"], "add"),
        (
            vec![as_me.as_str(), "people", "grant", "ana", "decide"],
            "grant",
        ),
    ] {
        let (code, said) = fails(&c, &me, &args);
        assert_eq!(code, 1, "{args:?}: {said}");
        assert!(
            said.contains(&format!("`just tasks` refuses people {action}: ")),
            "{args:?}: {said}"
        );
    }
    let (_, said) = fails(&c, &me, &["people", "add"]);
    assert!(said.contains("people takes list, log, add NAME"), "{said}");
    assert_eq!(
        c.ok(&me, &["people", "log"]),
        before,
        "nothing on the board"
    );
    assert_eq!(c.ok(&me, &["people", "map"]), map_before, "nothing mapped");
    let profiles = symphony_board::people::Profiles::read(&machine).unwrap();
    assert!(
        profiles.get("bob").is_none(),
        "nothing in the machine folder"
    );
}

/// The map of places to groups, kept from the command line as the other changes are: `map PLACE
/// GROUP` and `unmap PLACE`, each a numbered line of the map's log as `--as`'s, read back by `map`
/// and `map --json`; a task filed `--group` is written to that group, and `show` says whose a task
/// is and what said so once the board names anyone.
#[test]
fn the_map_is_kept_from_the_command_line_and_show_says_whose_a_task_is() {
    let (c, me, _machine) = case("people-map");
    assert_eq!(
        c.ok(&me, &["people", "map"]),
        "the map names no place: every card and task with no name written on it goes to the \
         board's default, the person at the machine\n"
    );
    c.ok(&me, &["create", "app/unnamed", "Unnamed"]);
    let shown = c.ok(&me, &["show", "app/unnamed"]);
    assert!(
        !shown.contains("group:"),
        "a board naming nobody says no group: {shown}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&me, &["show", "app/unnamed", "--json"])).unwrap();
    assert!(json.get("owning").is_none(), "nor its JSON: {json}");
    c.ok(&me, &["people", "group", "add", "design"]);
    c.ok(&me, &["people", "group", "add", "engineering"]);
    c.ok(
        &me,
        &[
            "people", "add", "ana", "--group", "design", "--power", "speak",
        ],
    );
    assert_eq!(
        c.ok(&me, &["people", "map", "area:app", "engineering"]),
        "the area app maps to the group engineering (change 1 of the map)\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "map", "area:app", "engineering"]),
        "the area app maps to engineering already: nothing written\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "map", "settings", "design"]),
        "the Settings page maps to the group design (change 2 of the map)\n"
    );
    assert_eq!(
        c.ok(&me, &["people", "unmap", "settings"]),
        "the Settings page is off the map (change 3 of the map): its things go to the board's \
         default\n"
    );
    let (code, said) = fails(&c, &me, &["people", "map", "area:app", "marketing"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("keeps no group marketing"), "{said}");
    let (code, said) = fails(&c, &me, &["people", "map", "docs/design", "design"]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("is no place"), "{said}");
    let map = c.ok(&me, &["people", "map"]);
    let lines: Vec<&str> = map.lines().collect();
    assert_eq!(lines[0], "area:app  engineering", "{map}");
    assert!(
        lines[1].ends_with(&format!("  1  {me}: map area:app engineering")),
        "{map}"
    );
    assert!(
        lines[3].ends_with(&format!("  3  {me}: unmap settings")),
        "{map}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&me, &["people", "map", "--json"])).unwrap();
    assert_eq!(json["revision"], 3);
    assert_eq!(
        json["map"],
        serde_json::json!([{"place": "area:app", "group": "engineering"}])
    );
    assert_eq!(json["log"][1]["did"], "map");
    assert_eq!(json["log"][2]["did"], "unmap");

    assert!(
        c.ok(&me, &["show", "app/unnamed"])
            .contains("  group: engineering (from the area app)\n")
    );
    let json: serde_json::Value =
        serde_json::from_str(&c.ok(&me, &["show", "app/unnamed", "--json"])).unwrap();
    assert_eq!(
        json["owning"],
        serde_json::json!({
            "name": "engineering", "kind": "group", "by": "map", "place": "area:app",
        })
    );
    c.ok(
        &me,
        &[
            "create",
            "app/for-design",
            "For design",
            "--group",
            "design",
        ],
    );
    assert!(
        c.ok(&me, &["show", "app/for-design"])
            .contains("  group: design (named on it)\n")
    );
    c.ok(
        &me,
        &[
            "create",
            "app/for-nobody",
            "For nobody",
            "--group",
            "marketing",
        ],
    );
    assert!(c.ok(&me, &["show", "app/for-nobody"]).contains(
        "  group: engineering (from the area app, as this board holds no group or person \
         marketing)\n"
    ));
    let (code, said) = fails(
        &c,
        &me,
        &["create", "app/bad", "Bad", "--group", "Two Words"],
    );
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("short name, or owner"), "{said}");
}
