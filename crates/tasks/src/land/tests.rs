//! What a land reads without a repository or a board: the project's settings, the approval on a
//! thread and the commits it names. The landing itself is `tests/sweep/land.rs`'s, on scratch
//! repositories and boards.

use super::moving::Restored;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn task(status: &str) -> Task {
    Task {
        branch: None,
        id: 1,
        slug: "tools/lamp".into(),
        title: "A lamp".into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    }
}

/// A thread of `(kind, sender, text)` lines, one a second from 100.
fn thread(lines: &[(&str, &str, &str)]) -> Vec<Message> {
    lines
        .iter()
        .zip(1..)
        .map(|((kind, sender, text), id)| Message {
            id,
            slug: "tools/lamp".into(),
            sender: (*sender).into(),
            recipient: None,
            kind: (*kind).into(),
            text: (*text).into(),
            #[expect(clippy::cast_precision_loss, reason = "a handful of ids")]
            ts: 100.0 + id as f64,
        })
        .collect()
}

#[test]
fn a_project_with_no_settings_lands_on_main_with_no_check_and_pushes() {
    assert_eq!(Settings::parse("").unwrap(), Settings::default());
    let settings = Settings::default();
    assert_eq!(
        (
            settings.default_branch.as_str(),
            settings.check.is_empty(),
            settings.worktrees,
            settings.generators.is_empty(),
            settings.push
        ),
        ("main", true, None, true, true)
    );
}

#[test]
fn the_settings_a_land_reads_and_the_rest_of_the_file_left_alone() {
    let settings = Settings::parse(
        r#"
[project]
name = "Lamps"
default_branch = "trunk"

[agents]
panes = "hc"
worktrees = "../lamps-wt"

[verdicts]
check = ["just", "verify"]
land = ["just", "land"]

[land]
push = false
generators = [
  { name = "the shades", outputs = "shades/", command = ["just", "shades"] },
  { name = "the index", outputs = "index.md", command = ["just", "index"], when_changed = true },
]
keys = { "the index" = ["tools/index-key"] }

[board]
on_review = ["just", "review"]
"#,
    )
    .unwrap();
    assert_eq!(
        settings,
        Settings {
            default_branch: "trunk".into(),
            check: vec!["just".into(), "verify".into()],
            main_check: Vec::new(),
            worktrees: Some("../lamps-wt".into()),
            generators: vec![
                Generator {
                    name: "the shades".into(),
                    outputs: "shades/".into(),
                    command: vec!["just".into(), "shades".into()],
                    when_changed: false,
                    key: Vec::new(),
                },
                Generator {
                    name: "the index".into(),
                    outputs: "index.md".into(),
                    command: vec!["just".into(), "index".into()],
                    when_changed: true,
                    key: vec!["tools/index-key".into()],
                },
            ],
            push: false,
            timeout: TIMEOUT,
        }
    );
}

#[test]
fn a_generators_outputs_read_as_the_symphony_reads_them() {
    for (written, read) in [
        ("gen/", "gen/"),
        ("./gen/", "gen/"),
        ("gen/./index.md", "gen/index.md"),
        ("a//b/", "a/b/"),
        ("index.md", "index.md"),
    ] {
        let text = format!(
            "[land]\ngenerators = [{{ name = \"x\", outputs = {written:?}, command = [\"true\"] }}]\n"
        );
        let settings = Settings::parse(&text).unwrap();
        assert_eq!(settings.generators[0].outputs, read, "{written}");
    }
}

#[test]
fn a_generator_or_branch_a_land_cannot_use_is_refused_by_name() {
    let refused = |text: &str| Settings::parse(text).unwrap_err();
    assert_eq!(
        refused("[project]\ndefault_branch = \" \"\n"),
        "symphony.toml: [project] default_branch is empty"
    );
    assert_eq!(
        refused("[land]\ngenerators = [{ name = \"x\", outputs = \"out/\", command = [] }]\n"),
        "symphony.toml: [land] generator x has no command"
    );
    assert_eq!(
        refused("[land]\ngenerators = [{ name = \"x\", outputs = \"out/\", command = [\"\"] }]\n"),
        "symphony.toml: [land] generator x has no command"
    );
    for outputs in ["", ".", "./", "/abs", "../up", "a/../b"] {
        let text = format!(
            "[land]\ngenerators = [{{ name = \"x\", outputs = {outputs:?}, command = [\"true\"] }}]\n"
        );
        assert_eq!(
            refused(&text),
            format!(
                "symphony.toml: [land] generator x writes {outputs:?}, not a path inside the project"
            )
        );
    }
    let misspelt = refused(
        "[land]\ngenerators = [{ name = \"x\", outputs = \"o/\", command = [\"true\"], when = 1 }]\n",
    );
    assert!(
        misspelt.starts_with("symphony.toml: ") && misspelt.contains("unknown field `when`"),
        "{misspelt}"
    );
    assert!(
        refused("[land]\npush = \"yes\"\n").starts_with("symphony.toml: "),
        "push is a boolean"
    );
}

#[test]
fn a_land_key_it_does_not_know_is_refused_by_its_name() {
    let refused = |text: &str| Settings::parse(text).unwrap_err();
    for key in ["pushh", "timeout", "checks"] {
        let said = refused(&format!("[land]\n{key} = 1\n"));
        assert!(
            said.starts_with("symphony.toml: ") && said.contains(&format!("unknown field `{key}`")),
            "{said}"
        );
    }
    // The other tables are the Symphony's to read: a key the land does not read there is left.
    assert_eq!(
        Settings::parse("[project]\nname = \"x\"\nwhatever = 1\n[agents]\nlaunch = []\n").unwrap(),
        Settings::default()
    );
}

#[test]
fn a_project_turns_the_lands_check_off_and_keeps_it_for_its_verdicts() {
    let text = "[verdicts]\ncheck = [\"just\", \"audit\"]\n";
    let audit = ["just".to_string(), "audit".to_string()];
    let default = Settings::parse(text).unwrap();
    assert_eq!(default.check, audit);
    assert!(default.main_check.is_empty(), "{default:?}");
    // Off, the land runs none and the regenerator runs it on the default branch's tip instead.
    let off = Settings::parse(&format!("{text}[land]\ncheck = false\n")).unwrap();
    assert!(off.check.is_empty(), "{off:?}");
    assert_eq!(off.main_check, audit);
    let on = Settings::parse(&format!("{text}[land]\ncheck = true\n")).unwrap();
    assert_eq!(on.check.len(), 2);
    assert!(on.main_check.is_empty(), "{on:?}");
    // Off with no check to run: neither runs one.
    let none = Settings::parse("[land]\ncheck = false\n").unwrap();
    assert!(
        none.check.is_empty() && none.main_check.is_empty(),
        "{none:?}"
    );
    assert!(
        Settings::parse("[land]\ncheck = 1\n")
            .unwrap_err()
            .starts_with("symphony.toml: "),
        "check is a boolean"
    );
}

#[test]
fn generated_paths_taken_are_named_each_when_few_else_by_their_outputs() {
    let generators = Settings::parse(
        "[land]\ngenerators = [\n  { name = \"a\", outputs = \"gen/\", command = [\"true\"] },\n  \
         { name = \"b\", outputs = \"dist\", command = [\"true\"] },\n]\n",
    )
    .unwrap()
    .generators;
    let paths = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    assert_eq!(taken_said(&[], "main", &generators), "");
    assert_eq!(
        taken_said(&paths(&["gen/x"]), "main", &generators),
        ", its generated paths (gen/x) taken from main, for the regenerator to make"
    );
    assert_eq!(
        generated_named(&paths(&["dist/a", "gen/x", "gen/y"]), &generators),
        "dist/a, gen/x, gen/y"
    );
    // Past three, each generator's outputs once, in the order their paths came.
    assert_eq!(
        generated_named(
            &paths(&["dist/a", "gen/x", "gen/y", "dist/b", "gen/z"]),
            &generators
        ),
        "dist, gen/"
    );
    // A path no generator holds (none should be) is named as it is.
    assert_eq!(
        generated_named(&paths(&["gen/x", "gen/y", "gen/z", "other"]), &generators),
        "gen/, other"
    );
}

#[test]
fn a_generators_key_is_a_command_named_in_land_keys_or_none() {
    let parsed = |keys: &str| {
        Settings::parse(&format!(
            "[land]\ngenerators = [\n  {{ name = \"x\", outputs = \"o/\", command = [\"true\"] }},\n  \
             {{ name = \"y\", outputs = \"p/\", command = [\"true\"] }},\n]\n{keys}"
        ))
    };
    let unkeyed = parsed("").unwrap();
    assert!(unkeyed.generators.iter().all(|g| g.key.is_empty()));
    let keyed = parsed("keys = { x = [\"tools/key.sh\", \"--short\"] }\n").unwrap();
    assert_eq!(
        keyed.generators[0].key,
        ["tools/key.sh".to_string(), "--short".to_string()]
    );
    assert!(keyed.generators[1].key.is_empty(), "y has none");
    for (keys, refusal) in [
        (
            "keys = { x = [] }\n",
            "symphony.toml: [land] keys gives the generator x a key with no command",
        ),
        (
            "keys = { x = [\"\"] }\n",
            "symphony.toml: [land] keys gives the generator x a key with no command",
        ),
        (
            "keys = { z = [\"true\"] }\n",
            "symphony.toml: [land] keys names \"z\", which no [land] generator is",
        ),
    ] {
        assert_eq!(parsed(keys).unwrap_err(), refusal, "{keys}");
    }
    // In the generator's own table it is not a key: an older Symphony would refuse the file.
    let inline = Settings::parse(
        "[land]\ngenerators = [{ name = \"x\", outputs = \"o/\", command = [\"true\"], key = \
         [\"k\"] }]\n",
    )
    .unwrap_err();
    assert!(inline.contains("unknown field `key`"), "{inline}");
}

#[test]
fn the_timeout_bounds_each_command_and_the_network_five_minutes_at_most() {
    assert_eq!(Settings::default().timeout, TIMEOUT);
    assert_eq!(Settings::default().network(), NETWORK);
    let settings = Settings::parse("[land]\ntimeout_seconds = 90\n").unwrap();
    assert_eq!(
        (settings.timeout, settings.network()),
        (Duration::from_secs(90), Duration::from_secs(90))
    );
    let settings = Settings::parse("[land]\ntimeout_seconds = 7200\n").unwrap();
    assert_eq!(
        (settings.timeout, settings.network()),
        (Duration::from_secs(7200), NETWORK)
    );
    assert_eq!(
        Settings::parse("[land]\ntimeout_seconds = 0\n").unwrap_err(),
        "symphony.toml: [land] timeout_seconds is 0, which no generator or check runs in"
    );
    assert!(
        Settings::parse("[land]\ntimeout_seconds = -1\n")
            .unwrap_err()
            .starts_with("symphony.toml: "),
        "a timeout is a count of seconds"
    );
}

#[test]
fn an_approval_stands_when_it_is_the_last_verdict_and_left_the_task_done() {
    let lines = thread(&[
        ("system", "amber", "amber claimed it"),
        ("verdict", "slate", "changes: the hash moved"),
        ("msg", "amber", "fixed at 4b5e7b0"),
        ("verdict", "slate", "approve: Approve at 4b5e7b0."),
        ("msg", "amber", "thanks"),
    ]);
    assert_eq!(
        approval(&task("done"), &lines).unwrap(),
        Approval {
            by: "slate".into(),
            at: 104.0,
            text: "approve: Approve at 4b5e7b0.".into(),
            review_from: None,
        }
    );
    // A bare `approve` is one too; `approved` is not.
    let bare = thread(&[("verdict", "slate", "approve")]);
    assert!(approval(&task("done"), &bare).is_ok());
    let not = thread(&[("verdict", "slate", "approved: fine")]);
    assert_eq!(
        approval(&task("done"), &not).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
}

#[test]
fn an_approval_knows_when_the_review_it_ends_began() {
    let lines = thread(&[
        ("system", "amber", "amber claimed it"),
        ("system", "amber", "amber moved it in_progress → review"),
        ("verdict", "slate", "changes: the hash moved"),
        ("system", "amber", "amber moved it in_progress → review"),
        ("verdict", "slate", "approve: good"),
        // After the approve: not the review it ends.
        ("system", "amber", "amber moved it done → review"),
    ]);
    // The last verdict is the approve; the move after it is read as no part of its review.
    let approved = &lines[..5];
    assert_eq!(
        approval(&task("done"), approved).unwrap().review_from,
        Some(104.0)
    );
    // A message saying the words is not a move.
    let said = thread(&[
        ("msg", "amber", "it moved it in_progress → review"),
        ("verdict", "slate", "approve"),
    ]);
    assert_eq!(approval(&task("done"), &said).unwrap().review_from, None);
}

#[test]
fn an_approve_is_the_word_alone_or_followed_by_its_reasons() {
    for (text, is) in [
        ("approve", true),
        ("approve: yes", true),
        ("approve:", true),
        ("approves", false),
        ("approved: fine", false),
        ("changes: approve it later", false),
        ("landed tools/lamp on main at 3e0c661", false),
    ] {
        let lines = thread(&[("verdict", "slate", text)]);
        assert_eq!(crate::approved(&lines).is_some(), is, "{text:?}");
    }
    // Only the last verdict counts, whatever comes after it that is no verdict.
    let lines = thread(&[
        ("verdict", "slate", "approve: good"),
        ("verdict", "slate", "changes: not yet"),
        ("msg", "slate", "approve"),
    ]);
    assert!(crate::approved(&lines).is_none());
}

fn approval_by(by: &str) -> Approval {
    Approval {
        by: by.into(),
        at: 100.0,
        text: "approve".into(),
        review_from: None,
    }
}

fn holding(roles: &[(&str, &str)], fast_fix: Option<&str>) -> Task {
    let mut task = task("done");
    task.roles = roles
        .iter()
        .map(|(handle, role)| crate::Role {
            handle: (*handle).into(),
            role: (*role).into(),
            since: 0.0,
            expires: None,
            health: crate::Health::Live,
        })
        .collect();
    if let Some(why) = fast_fix {
        task.fields.insert(
            crate::red::FIELDS.into(),
            BTreeMap::from([(crate::fast_fix::MARK.into(), why.into())]),
        );
    }
    task
}

#[test]
fn only_a_fresh_reviewers_approve_counts_and_the_orchestrators_on_a_fast_fix() {
    // amber executed it (or a subtask), and taupe orchestrated it before it handed it to ochre.
    let held = Held {
        executed: BTreeSet::from(["amber".to_string()]),
        orchestrated: BTreeSet::from(["taupe".to_string(), "ochre".to_string()]),
    };
    let roles = [
        ("ochre", "orchestrator"),
        ("slate", "reviewer"),
        ("amber", "reviewer"),
        ("taupe", "reviewer"),
    ];
    let plain = holding(&roles, None);
    let fast = holding(&roles, Some("a one-line fix"));
    for task in [&plain, &fast] {
        assert_eq!(independent(task, &approval_by("slate"), &held), Ok(()));
        // Who executed it never approves it, whatever it holds now: a fast fix its orchestrator
        // executed is approved by a fresh reviewer too.
        assert_eq!(
            independent(task, &approval_by("amber"), &held).unwrap_err(),
            "tools/lamp's approve is amber's, which executed it or work under it: a fresh \
             reviewer, one that did not write or execute it, approves it"
        );
    }
    let orchestrated = |by: &str| {
        format!(
            "tools/lamp's approve is {by}'s, which orchestrated it: a fresh reviewer approves it, \
             and its orchestrator only a fast fix (filed with `just tasks create --fast-fix WHY`)"
        )
    };
    // Its orchestrator now, and one that orchestrated it before and reviews it now.
    for by in ["ochre", "taupe"] {
        assert_eq!(
            independent(&plain, &approval_by(by), &held).unwrap_err(),
            orchestrated(by)
        );
    }
    // One holding the role now that the record does not show (an older board's) is one too.
    assert_eq!(
        independent(&plain, &approval_by("ochre"), &Held::default()).unwrap_err(),
        orchestrated("ochre")
    );
    assert_eq!(
        independent(&plain, &approval_by("owner"), &held).unwrap_err(),
        "tools/lamp's approve is owner's, which filed it: a fresh reviewer approves it, and its \
         orchestrator only a fast fix (filed with `just tasks create --fast-fix WHY`)"
    );
    for by in ["ochre", "taupe"] {
        assert_eq!(independent(&fast, &approval_by(by), &held), Ok(()));
    }
}

#[test]
fn a_record_of_a_land_is_read_back_only_as_its_own_tasks() {
    let folder = std::env::temp_dir().join(format!("sf-land-record-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let record = Record {
        slug: "tools/lamp".into(),
        tip: "a".repeat(40),
        merge: "b".repeat(40),
        checked: "c".repeat(40),
        regenerated: Vec::new(),
    };
    assert_eq!(Record::read(&folder, "tools/lamp"), None);
    record.write(&folder).unwrap();
    assert_eq!(
        Record::path(&folder, "tools/lamp"),
        folder.join("tools%2Flamp.merged")
    );
    assert_eq!(Record::read(&folder, "tools/lamp"), Some(record.clone()));
    // Written whole: no staged copy is left beside it, and no list of paths it took nothing for.
    let names: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["tools%2Flamp.merged"]);
    assert!(
        !fs::read_to_string(Record::path(&folder, "tools/lamp"))
            .unwrap()
            .contains("regenerated")
    );
    assert_eq!(Record::read(&folder, "tools-lamp"), None);
    fs::write(Record::path(&folder, "tools/lamp"), "not a record").unwrap();
    assert_eq!(Record::read(&folder, "tools/lamp"), None);
    record.write(&folder).unwrap();
    Record::drop_in(&folder, "tools/lamp");
    assert_eq!(Record::read(&folder, "tools/lamp"), None);
    Record::drop_in(&folder, "tools/lamp");
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn two_slugs_a_dash_apart_keep_two_records_and_an_earlier_lands_is_read_as_its_own() {
    let folder = std::env::temp_dir().join(format!("sf-land-stems-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let of = |slug: &str, tip: char| Record {
        slug: slug.into(),
        tip: tip.to_string().repeat(40),
        merge: "b".repeat(40),
        checked: "c".repeat(40),
        regenerated: vec!["gen/out.txt".into()],
    };
    // `a/b-c` and `a-b/c` were both `a-b-c.merged`: the second wrote over the first.
    let (one, two) = (of("a/b-c", '1'), of("a-b/c", '2'));
    one.write(&folder).unwrap();
    two.write(&folder).unwrap();
    assert_ne!(
        Record::path(&folder, "a/b-c"),
        Record::path(&folder, "a-b/c")
    );
    assert_eq!(Record::read(&folder, "a/b-c"), Some(one.clone()));
    assert_eq!(Record::read(&folder, "a-b/c"), Some(two.clone()));
    // The name gives the slug back, a `%` of its own too.
    for slug in ["a/b-c", "a-b/c", "50%/off", "a%2Fb", "plain"] {
        assert_eq!(slug_of(&file_stem(slug)), slug, "{slug}");
    }
    assert_ne!(file_stem("a%2Fb"), file_stem("a/b"));
    // A record an earlier land kept under the dashed name is read and dropped as its slug's alone.
    Record::drop_in(&folder, "a/b-c");
    Record::drop_in(&folder, "a-b/c");
    let dashed = folder.join("a-b-c.merged");
    fs::write(&dashed, serde_json::to_string(&one).unwrap()).unwrap();
    assert_eq!(Record::read(&folder, "a/b-c"), Some(one));
    assert_eq!(Record::read(&folder, "a-b/c"), None);
    Record::drop_in(&folder, "a-b/c");
    assert!(dashed.exists(), "another task's record stays");
    Record::drop_in(&folder, "a/b-c");
    assert!(!dashed.exists());
    // One written before records said what they regenerated reads as regenerating nothing.
    fs::write(
        Record::path(&folder, "a/b-c"),
        format!(
            "{{\"slug\":\"a/b-c\",\"tip\":\"{t}\",\"merge\":\"{t}\",\"checked\":\"{t}\"}}",
            t = "d".repeat(40)
        ),
    )
    .unwrap();
    assert_eq!(
        Record::read(&folder, "a/b-c").map(|record| record.regenerated),
        Some(Vec::new())
    );
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_generators_outputs_hold_their_own_paths_and_never_a_neighbours() {
    for outputs in ["gen", "gen/"] {
        assert!(owns(outputs, "gen/a.txt"), "{outputs}");
        assert!(owns(outputs, "gen/deep/b.js"), "{outputs}");
        assert!(!owns(outputs, "gen-x/a.txt"), "{outputs}");
        assert!(!owns(outputs, "gen.txt"), "{outputs}");
        assert!(!owns(outputs, "generated/a.txt"), "{outputs}");
        assert!(!owns(outputs, "src/gen/a.txt"), "{outputs}");
    }
    // A file named as outputs is its own; a folder named with its slash is not a file.
    assert!(owns("gen", "gen"));
    assert!(!owns("gen/", "gen"));
    assert!(owns(
        "apps/symphony-server/client/public/",
        "apps/symphony-server/client/public/index.html"
    ));
    assert!(!owns(
        "apps/symphony-server/client/public/",
        "apps/symphony-server/client/public-old/index.html"
    ));
}

#[test]
fn the_group_a_land_runs_is_named_with_its_start_and_an_earlier_lands_line_still_reads() {
    let named = Named {
        group: 4242,
        start: Some("1731400000.123456".into()),
        waits: true,
        command: "git merge --ff-only --quiet abc".into(),
    };
    assert_eq!(
        named.line(),
        "4242\tstarted 1731400000.123456\twait\tgit merge --ff-only --quiet abc\n"
    );
    assert_eq!(Named::parse(&named.line()), Some(named.clone()));
    let unread = Named {
        start: None,
        waits: false,
        command: "sh -c a\tb".into(),
        ..named
    };
    assert_eq!(unread.line(), "4242\tstarted -\tend\tsh -c a\tb\n");
    assert_eq!(Named::parse(&unread.line()), Some(unread));
    // An earlier land's line: the group and its command, no start, to be ended.
    assert_eq!(
        Named::parse("77\tsh -c check\n"),
        Some(Named {
            group: 77,
            start: None,
            waits: false,
            command: "sh -c check".into(),
        })
    );
    assert_eq!(
        Named::parse("77\tstarted x\tlater\tcmd").map(|n| n.command),
        Some("started x\tlater\tcmd".into())
    );
    for text in ["", "x\tcmd", "-1\tcmd"] {
        assert_eq!(Named::parse(text), None, "{text:?}");
    }
}

#[test]
fn a_process_start_is_read_for_a_live_process_alone() {
    let ours = i64::from(std::process::id());
    let start = started(ours).expect("this process's start");
    assert_eq!(started(ours).as_deref(), Some(start.as_str()));
    assert!(lands_still(ours, Some(&start)));
    assert!(lands_still(ours, None), "an earlier land's lock: by pid");
    // Another start under the same number is another process.
    assert!(!lands_still(ours, Some("0.000000")));
    assert_eq!(started(0), None);
    assert_eq!(started(-5), None);
}

#[test]
fn what_a_land_restored_is_said_path_by_path() {
    let from = "0123456789abcdef";
    assert_eq!(Restored::default().said("main", from), None);
    let restored = Restored {
        put: vec!["sim/world.gd".into()],
        removed: vec!["sim/jump.gd".into(), "sim/new/a.gd".into()],
        left: Vec::new(),
    };
    assert_eq!(
        restored.said("main", from).unwrap(),
        "in the main checkout, which the move half-wrote, the land restored sim/world.gd as main \
         at 0123456 has it; removed sim/jump.gd and sim/new/a.gd, which main at 0123456 lacks"
    );
    let left = Restored {
        left: vec!["notes.md".into()],
        ..Restored::default()
    };
    assert_eq!(
        left.said("trunk", from).unwrap(),
        "in the main checkout, which the move half-wrote, the land left notes.md as it found it, \
         changed otherwise"
    );
}

#[test]
fn a_signal_is_named_as_a_terminal_names_it() {
    assert_eq!(signal_name(libc::SIGHUP), "SIGHUP");
    assert_eq!(signal_name(libc::SIGINT), "SIGINT");
    assert_eq!(signal_name(libc::SIGTERM), "SIGTERM");
    assert_eq!(
        signal_name(libc::SIGUSR1),
        format!("signal {}", libc::SIGUSR1)
    );
}

#[test]
fn a_lands_worktree_lock_names_its_pid_and_when_it_started() {
    assert_eq!(
        locked_by("symphony land: tools/lamp (pid 4242)"),
        Some((4242, None))
    );
    assert_eq!(
        locked_by("symphony land: a (pid 1) b (pid 77)"),
        Some((77, None))
    );
    assert_eq!(
        locked_by("symphony land: tools/lamp (pid 4242, started 1731400000.123456)"),
        Some((4242, Some("1731400000.123456".into())))
    );
    assert_eq!(
        locked_by("symphony land: tools/lamp (pid 4242, started -)"),
        Some((4242, None))
    );
    // This land's own reason names it and its start.
    let ours = std::process::id();
    assert_eq!(
        locked_by(&lock_reason("tools/lamp")),
        Some((i64::from(ours), started(i64::from(ours))))
    );
    assert!(lock_reason("tools/lamp").starts_with("symphony land: tools/lamp (pid "));
    for reason in [
        "symphony land: tools/lamp (pid x, started 1)",
        "symphony land: tools/lamp",
        "symphony land: tools/lamp (pid )",
        "symphony land: tools/lamp (pid x)",
        "symphony land: tools/lamp (pid 4242",
        "",
    ] {
        assert_eq!(locked_by(reason), None, "{reason:?}");
    }
}

#[test]
fn a_command_past_its_limit_is_ended_with_its_whole_group() {
    let mut child = sys::command("sh")
        .args(["-c", "sleep 30 & sleep 30; wait"])
        .process_group(0)
        .spawn()
        .unwrap();
    let began = Instant::now();
    let group = child.id();
    let (status, ended) = watched(group, Duration::from_millis(300), None, || child.wait());
    assert_eq!(ended, Ended::Late);
    assert!(!status.unwrap().success());
    assert!(
        !sys::group_alive(group),
        "the background sleep went with it"
    );
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
    let mut quick = sys::command("true").process_group(0).spawn().unwrap();
    let (status, ended) = watched(quick.id(), Duration::from_secs(30), None, || quick.wait());
    assert_eq!(ended, Ended::No);
    assert!(status.unwrap().success());
}

#[test]
fn what_a_command_leaves_running_in_its_group_ends_when_it_does() {
    // The shell ends at once; the sleep it left in the background, in its group, would run on.
    let mut child = sys::command("sh")
        .args(["-c", "sleep 30 &"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let group = child.id();
    let began = Instant::now();
    let (status, ended) = watched(group, Duration::from_secs(30), None, || child.wait());
    assert_eq!(ended, Ended::No);
    assert!(status.unwrap().success());
    assert!(!sys::group_alive(group), "the sleep it left was ended");
    assert!(began.elapsed() < GRACE + Duration::from_secs(5));
}

/// A scratch main checkout: the folder `dir` holding `files` alone.
fn checkout(dir: &Path, files: &[(&str, &str)]) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    for (name, text) in files {
        fs::write(dir.join(name), text).unwrap();
    }
}

const RECIPES: &str = "set allow-duplicate-recipes\n\nland *ARGS:\n    @echo symphony land\n\ntasks *ARGS:\n    @echo tasks\n";

#[test]
fn a_projects_own_land_is_the_one_its_justfile_runs_when_not_the_symphonys() {
    let root = std::env::temp_dir().join(format!("sf-land-own-{}", std::process::id()));
    let env = Env::of_process();
    let own = "import 'symphony.just'\n\nland *ARGS:\n    @echo own land\n";
    for (files, case, owned) in [
        (vec![], "no justfile", false),
        (
            vec![
                ("justfile", "import 'symphony.just'\n"),
                ("symphony.just", RECIPES),
            ],
            "the Symphony's alone",
            false,
        ),
        (
            vec![("justfile", "build:\n    @echo b\n")],
            "no land at all",
            false,
        ),
        (
            vec![("justfile", own), ("symphony.just", RECIPES)],
            "its own after the import",
            true,
        ),
        (
            vec![("Justfile", own), ("symphony.just", RECIPES)],
            "its own, the justfile named in another case",
            true,
        ),
        (
            vec![("justfile", "land:\n    @echo own land\n")],
            "its own, no symphony.just",
            true,
        ),
    ] {
        checkout(&root, &files);
        let found = own_land(&root, &env).unwrap();
        assert_eq!(found.is_some(), owned, "{case}");
        if let Some(justfile) = found {
            assert_eq!(justfile.parent(), Some(root.as_path()), "{case}");
        }
    }
    checkout(&root, &[("justfile", "land:\n  @echo a\n\tbad\n")]);
    let broken = own_land(&root, &env).unwrap_err();
    assert!(
        broken.starts_with("cannot tell whether ") && broken.contains("just --show land: "),
        "{broken}"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_land_runs_only_by_its_recipe() {
    let env = Env {
        vars: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
        cwd: std::env::temp_dir(),
    };
    assert_eq!(
        admit(&env).unwrap_err(),
        "`tasks land` runs only as `just land SLUG`, the recipe that moves and pushes the default \
         branch"
    );
    let empty = Env {
        vars: BTreeMap::from([(RECIPE.into(), String::new())]),
        ..env
    };
    assert!(admit(&empty).is_err(), "an empty marker is none");
}

#[test]
fn no_approval_stands_on_open_work_on_work_closed_since_or_closed_another_way() {
    let approved = thread(&[("verdict", "slate", "approve: good")]);
    for status in ["ready", "in_progress", "review", "blocked"] {
        assert_eq!(
            approval(&task(status), &approved).unwrap_err(),
            format!("tools/lamp is {status}, with no approving verdict standing")
        );
    }
    let landed = thread(&[
        ("verdict", "slate", "approve: good"),
        (
            "verdict",
            "amber",
            "landed tools/lamp on main at 3e0c66192471\nmore",
        ),
    ]);
    assert_eq!(
        approval(&task("done"), &landed).unwrap_err(),
        "tools/lamp was closed after its approval: landed tools/lamp on main at 3e0c66192471"
    );
    let done = thread(&[("verdict", "amber", "done: integrated in 3e0c661")]);
    assert_eq!(
        approval(&task("done"), &done).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
    assert_eq!(
        approval(&task("done"), &[]).unwrap_err(),
        "tools/lamp is done with no approving verdict from a reviewer"
    );
}

#[test]
fn the_words_that_may_name_a_commit() {
    assert_eq!(
        named("Approve at 4b5e7b0, merged with main 4f4c8fd (tip 4b5e7b0a1c2d)."),
        ["4b5e7b0", "4f4c8fd", "4b5e7b0a1c2d"]
    );
    // Forty digits at most.
    let full = "a".repeat(39) + "1";
    assert_eq!(named(&format!("{full} {full}0")), [full.as_str()]);
    // Too short, a letter past f, upper case, no digit, part of a longer word.
    for text in [
        "abc123",
        "4b5e7g0",
        "4B5E7B0",
        "defaced",
        "x4b5e7b0",
        "4b5e7b0_tip",
    ] {
        assert_eq!(named(text), Vec::<&str>::new(), "{text:?}");
    }
}

#[test]
fn counts_and_names_read_as_words() {
    assert_eq!(commits(1), "1 commit");
    assert_eq!(commits(3), "3 commits");
    assert_eq!(and(&[]), "");
    assert_eq!(and(&["the shades"]), "the shades");
    assert_eq!(and(&["a", "b", "c"]), "a, b and c");
    assert_eq!(short("4b5e7b0a1c2d"), "4b5e7b0");
    assert_eq!(short("4b5"), "4b5");
}

#[test]
fn two_names_of_one_place_are_the_same_and_others_are_not() {
    let dir = std::env::temp_dir().join(format!("sf-land-same-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("real/inner")).unwrap();
    std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
    let real = dir.join("real/land");
    // The land's folder need not be there yet; its parent is resolved.
    assert!(same(&dir.join("link/land"), &real));
    assert!(same(&dir.join("real/inner/../land"), &real));
    assert!(same(&dir.join("gone/land"), &dir.join("gone/land")));
    assert!(!same(&dir.join("gone/land"), &dir.join("gone/other")));
    assert!(!same(&dir.join("link/other"), &real));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_land_starts_the_regenerator_on_its_own_board_alone_unless_told() {
    let env = |vars: &[(&str, &str)]| Env {
        vars: vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        cwd: PathBuf::from("/"),
    };
    assert!(regenerate::enabled(&env(&[])));
    assert!(!regenerate::enabled(&env(&[(
        "TASKS_SERVER",
        "127.0.0.1:1"
    )])));
    assert!(regenerate::enabled(&env(&[
        ("TASKS_SERVER", "127.0.0.1:1"),
        ("TASKS_REGENERATE", "on")
    ])));
    assert!(!regenerate::enabled(&env(&[("TASKS_REGENERATE", "off")])));
    // Anything else is no word: the board decides.
    assert!(regenerate::enabled(&env(&[("TASKS_REGENERATE", "yes")])));
    // With no generators there is nothing to start, whatever it is told.
    let none = Settings::default();
    assert_eq!(
        regenerate::start(&env(&[("TASKS_REGENERATE", "on")]), Path::new("/"), &none),
        None
    );
}

#[test]
fn the_pairs_passed_keep_the_newest_first_once_each_and_no_more_than_kept() {
    use regenerate::{Pair, Passed};
    let pair = |n: usize| Pair {
        generator: "the client".into(),
        key: format!("key-{n}"),
        outputs: format!("tree-{n}"),
    };
    let mut passed = Passed::default();
    for n in 0..70 {
        passed.add(pair(n));
    }
    assert_eq!(passed.pairs.len(), 64);
    assert_eq!(passed.pairs[0], pair(69));
    assert!(passed.holds(&pair(6)) && !passed.holds(&pair(5)));
    passed.add(pair(30));
    assert_eq!(passed.pairs[0], pair(30));
    assert_eq!(passed.pairs.iter().filter(|p| **p == pair(30)).count(), 1);
    assert!(!passed.holds(&Pair {
        outputs: "tree-other".into(),
        ..pair(30)
    }));
    // Kept whole in a folder, and read back; a folder with none or a file unread holds none.
    let dir = std::env::temp_dir().join(format!("sf-land-passed-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(Passed::read(&dir), Passed::default());
    passed.write(&dir).unwrap();
    assert_eq!(Passed::read(&dir), passed);
    fs::write(dir.join(regenerate::PASSED), "not json").unwrap();
    assert_eq!(Passed::read(&dir), Passed::default());
    fs::remove_dir_all(&dir).unwrap();
}
