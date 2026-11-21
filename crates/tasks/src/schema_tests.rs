//! The schema rule's every case, the refusals' words, the write guard's variables and which
//! statements count as writes.

use super::*;
use serde_json::json;

/// The program the cases open boards as.
fn program() -> Program {
    Program::named("gate-review", "/work/alpha/.target/debug/gate-review")
}

/// [`decide`] for [`program`].
fn decided(found: Option<i64>, own: i64, access: Access, project: Option<&str>) -> Opened {
    decide(found, own, access, project, &program())
}

#[test]
fn a_board_at_this_builds_version_opens_with_nothing_to_write_for_either_access() {
    for access in [Access::Write, Access::Read] {
        assert_eq!(
            decided(Some(11), 11, access, Some("alpha")),
            Opened::Current
        );
    }
}

#[test]
fn an_older_board_or_none_is_migrated_by_a_newer_writer() {
    for found in [None, Some(0), Some(9), Some(10)] {
        assert_eq!(
            decided(found, 11, Access::Write, None),
            Opened::Migrate,
            "{found:?}"
        );
    }
}

#[test]
fn a_reader_never_migrates_an_older_board_but_refuses_it_naming_the_board_command() {
    for found in [0, 9, 10] {
        assert_eq!(
            decided(Some(found), 11, Access::Read, Some("alpha")),
            Opened::Refused(format!(
                "the alpha board is at schema {found}, behind this build's 11, and a reader never \
                 moves a board forward: nothing was read or written; `just tasks migrate` in the \
                 main checkout of alpha moves it once the project's board command is at schema 11, \
                 and a newer schema is tried on a scratch board (`TASKS_DATABASE=NAME`)"
            )),
        );
    }
    let Opened::Refused(unnamed) = decided(Some(10), 11, Access::Read, None) else {
        panic!("an older board is refused to a reader");
    };
    assert!(
        unnamed.starts_with("this board is at schema 10, behind"),
        "{unnamed}"
    );
    assert!(
        unnamed.contains("`just tasks migrate` in the main checkout of its project"),
        "{unnamed}"
    );
}

#[test]
fn a_reader_never_makes_a_board_but_refuses_one_not_made_yet_naming_the_board_command() {
    assert_eq!(
        decided(None, 11, Access::Read, Some("alpha")),
        Opened::Refused(
            "the alpha board is not made yet, and a reader never makes a board: nothing was read \
             or written; `just tasks list` in alpha makes it"
                .into()
        )
    );
    let Opened::Refused(unnamed) = decided(None, 11, Access::Read, None) else {
        panic!("a board not made yet is refused to a reader");
    };
    assert_eq!(
        unnamed,
        "this board is not made yet, and a reader never makes a board: nothing was read or \
         written; `just tasks list` in its project makes it"
    );
}

/// Whatever version a board is at, or none, a reader's open never migrates it.
#[test]
fn no_board_a_reader_opens_is_migrated() {
    let own = 11;
    for found in std::iter::once(None).chain((0..=own + 3).map(Some)) {
        for project in [None, Some("alpha")] {
            assert_ne!(
                decided(found, own, Access::Read, project),
                Opened::Migrate,
                "{found:?} {project:?}"
            );
        }
    }
}

#[test]
fn a_board_one_ahead_is_read_by_a_reader_and_refused_to_a_writer_naming_the_project() {
    assert_eq!(
        decided(Some(12), 11, Access::Read, Some("alpha")),
        Opened::Ahead
    );
    assert_eq!(
        decided(Some(12), 11, Access::Write, Some("alpha")),
        Opened::Refused(
            "the alpha board is at schema 12, and this build writes only a board at its own \
             schema, 11: nothing was written; rebuild gate-review \
             (/work/alpha/.target/debug/gate-review) at the board's schema, and `just symphony` \
             in alpha brings its Symphony to it"
                .into()
        )
    );
}

#[test]
fn a_board_two_or_more_ahead_is_refused_to_a_reader_too() {
    for found in [13, 40] {
        assert_eq!(
            decided(Some(found), 11, Access::Read, Some("alpha")),
            Opened::Refused(format!(
                "the alpha board is at schema {found}, more than one ahead of this build's 11: \
                 nothing was read; `just symphony` in alpha brings its Symphony to the board's \
                 schema"
            ))
        );
        assert!(matches!(
            decided(Some(found), 11, Access::Write, Some("alpha")),
            Opened::Refused(why) if why.contains("writes only a board at its own schema")
        ));
    }
}

#[test]
fn a_board_no_project_names_is_this_board_of_its_project() {
    let Opened::Refused(why) = decided(Some(12), 11, Access::Write, None) else {
        panic!("a newer board is refused to a writer");
    };
    assert!(why.starts_with("this board is at schema 12,"), "{why}");
    assert!(why.contains("`just symphony` in its project"), "{why}");
}

/// An older writer is told which program to rebuild: its file name and where it is.
#[test]
fn an_older_writers_refusal_names_the_program_to_rebuild() {
    let other = Program::named("tasks", "/opt/symphony/bin/tasks");
    let Opened::Refused(why) = decide(Some(19), 18, Access::Write, Some("alpha"), &other) else {
        panic!("a newer board is refused to a writer");
    };
    assert!(
        why.contains("rebuild tasks (/opt/symphony/bin/tasks) at the board's schema"),
        "{why}"
    );
    let refusal = Refusal::of(Some("alpha"), 18, &other);
    assert_eq!(why, format!("{}19{}", refusal.before, refusal.after));
}

#[test]
fn the_write_refusal_is_split_around_the_version_the_guard_reads() {
    let refusal = Refusal::of(Some("beta"), 11, &program());
    assert_eq!(refusal.before, "the beta board is at schema ");
    let Opened::Refused(whole) = decided(Some(12), 11, Access::Write, Some("beta")) else {
        panic!("a newer board is refused to a writer");
    };
    assert_eq!(whole, format!("{}12{}", refusal.before, refusal.after));
}

#[test]
fn the_guard_takes_a_writes_own_variables_and_adds_its_own() {
    let refusal = Refusal::of(Some("alpha"), 11, &program());
    let mut vars = json!({"slug": "a/b"});
    let steps = Steps::naming("fixture_new", 2, "the alpha board", 11);
    guard_vars(&mut vars, 11, false, &refusal, &steps, "refused: ");
    assert_eq!(
        vars,
        json!({
            "slug": "a/b",
            "board_schema": 11,
            "board_schema_ahead": false,
            "board_schema_before": "refused: the alpha board is at schema ",
            "board_schema_after": refusal.after,
            "board_additions_least": 2,
            "board_additions_most": i64::MAX,
            "board_additions_before": "refused: the alpha board has ",
            "board_additions_after": steps.after,
        })
    );
    assert!(
        steps
            .after
            .contains("names `fixture_new`, which step 2 defines")
    );
    // A write with no variables of its own is given an object for the guard's.
    let mut none = serde_json::Value::Null;
    guard_vars(&mut none, 11, true, &refusal, &Steps::any(), "refused: ");
    assert_eq!(none["board_schema"], 11);
    assert_eq!(none["board_schema_ahead"], true);
    assert_eq!(none["board_additions_least"], 0);
    assert!(GUARD.contains("$board_schema_found != $board_schema"));
    assert!(GUARD.contains("$board_schema_ahead = true"));
    assert!(GUARD.contains("type::is_int($board_schema_from)"));
    // The steps are judged on a board of the store's own contract alone.
    assert!(GUARD.contains("IF $board_schema_found = $board_schema AND ($board_additions_found"));
    // A copy's: exactly its own.
    let exactly = Steps::exactly(3, "this board", 11);
    assert_eq!((exactly.least, exactly.most), (3, 3));
}

/// A statement needs the highest step whose table it names as a whole word, however it is quoted
/// or nested; a word that only contains a table's name, or none of them, needs nothing.
#[test]
fn a_statement_needs_the_highest_step_whose_table_it_names_in_any_quoting() {
    let added = vec![
        ("note".to_string(), 1),
        ("note_tag".to_string(), 2),
        ("other".to_string(), 3),
    ];
    let needs = |sql: &str| super::needs(sql, &added);
    assert_eq!(needs("CREATE note:1 SET a = 1;"), Some(("note", 1)));
    assert_eq!(
        needs("UPSERT type::record('note_tag', [$a]) SET b = 2;"),
        Some(("note_tag", 2))
    );
    assert_eq!(needs("CREATE `note` SET a = 1;"), Some(("note", 1)));
    assert_eq!(needs("CREATE ⟨note⟩:x SET a = 1;"), Some(("note", 1)));
    assert_eq!(
        needs("CREATE NOTE:1;"),
        Some(("note", 1)),
        "case is not looked at"
    );
    // Nested, in a subquery or a block: the highest step named anywhere.
    assert_eq!(
        needs("LET $x = (SELECT * FROM note); IF true { FOR $o IN [1] { CREATE other:1; }; };"),
        Some(("other", 3))
    );
    assert_eq!(needs("UPDATE task SET notes = 1, note_tags = 2;"), None);
    assert_eq!(needs("UPDATE task SET title = 'a';"), None);
    assert_eq!(super::needs("CREATE note:1;", &[]), None);
}

/// The guard's rule, as a reader asks it before it writes: its own schema always, one ahead only
/// for a write that may and a record that is an integer no greater than its own, nothing else.
#[test]
fn a_write_passes_at_its_own_schema_and_one_ahead_only_when_it_may_and_the_record_lets_it() {
    for (from, ahead) in [(json!(null), false), (json!(12), false), (json!(11), true)] {
        assert!(writes(Some(11), &from, 11, ahead), "{from} {ahead}");
    }
    // One ahead, by a write that may, on a record that names its schema or an older one.
    for from in [json!(11), json!(10), json!(-3)] {
        assert!(writes(Some(12), &from, 11, true), "{from}");
        assert!(!writes(Some(12), &from, 11, false), "a writer's: {from}");
    }
    // A record missing, newer than the write's schema, or of another shape lets nothing through.
    for from in [
        json!(null),
        json!(12),
        json!(13),
        json!("11"),
        json!(11.0),
        json!(10.5),
        json!(true),
        json!([11]),
        json!({"version": 11}),
        json!(u64::MAX),
    ] {
        assert!(!writes(Some(12), &from, 11, true), "{from}");
    }
    // Two ahead, behind, or no board: refused whatever the record says.
    for found in [Some(13), Some(40), Some(10), Some(0), None] {
        assert!(!writes(found, &json!(11), 11, true), "{found:?}");
        assert!(!writes(found, &json!(-100), 11, true), "{found:?}");
    }
    assert!(!writes(Some(i64::MIN), &json!(0), i64::MAX, true));
    assert!(
        !writes(None, &json!(0), i64::MAX, true),
        "no board is never one ahead"
    );
}

/// A schema's record of the oldest Symphony that may write a board it moved: its own version unless
/// its step is declared to keep the one behind writing, and a newer build's step declares nothing
/// until it says so.
#[test]
fn a_schema_names_the_oldest_symphony_that_may_write_the_board_it_moved() {
    let this = Schema::this_build();
    assert_eq!(
        this.writable_from(),
        VERSION - i64::from(keeps_writes(VERSION))
    );
    let next = this.then("DEFINE TABLE IF NOT EXISTS later SCHEMALESS;");
    assert_eq!(next.writable_from(), VERSION + 1);
    let keeping = next.clone().keeping_writes();
    assert_eq!(keeping.writable_from(), VERSION);
    assert_eq!(keeping.version(), next.version());
    assert_eq!(keeping.steps(), next.steps());
    assert_ne!(keeping, next, "the declaration is part of the schema");
    // A step after a declared one declares nothing of its own.
    let after = keeping.then_each("DEFINE TABLE IF NOT EXISTS later2 SCHEMALESS;", "");
    assert_eq!(after.writable_from(), VERSION + 2);
    assert_eq!(after.clone().keeping_writes().writable_from(), VERSION + 1);
    // An older build's declaration is not known here: it lets only its own schema write.
    assert_eq!(keeping.before().writable_from(), VERSION);
    assert_eq!(this.before().writable_from(), VERSION - 1);
}

/// The list starts empty: the schema that brought it and the record changed `meta` and keeps the
/// Symphony one behind read-only. Each version is listed once, with its reason, and none at or
/// before that schema or ahead of this build's.
#[test]
fn the_compatible_versions_are_each_listed_once_with_a_reason_and_none_ahead_of_this_build() {
    assert!(
        !keeps_writes(WRITABLE_FROM_SINCE),
        "the mechanism's own step changed meta"
    );
    let mut seen = std::collections::BTreeSet::new();
    for (version, why) in COMPATIBLE {
        assert!(seen.insert(*version), "{version} listed twice");
        assert!(*version > WRITABLE_FROM_SINCE, "{version}");
        assert!(*version <= VERSION, "{version}");
        assert!(why.split_whitespace().count() >= 5, "{version}: {why}");
        assert!(keeps_writes(*version));
    }
    assert!(!keeps_writes(VERSION + 1));
    assert!(DEFINE.contains("DEFINE FIELD IF NOT EXISTS writable_from ON meta TYPE option<int>;"));
}

#[test]
fn a_statement_that_changes_the_database_makes_a_write_in_any_case_and_a_read_never() {
    for write in [
        "UPDATE task SET x = 1",
        "LET $m = (CREATE message SET a = 1);",
        "upsert counter:board SET last = 1",
        "DELETE task_chunk RETURN NONE;",
        "INSERT INTO t $rows",
        "RELATE a->b->c",
        "DEFINE TABLE x SCHEMALESS;",
        "REMOVE TABLE IF EXISTS copy_1;",
        "ALTER TABLE x COMMENT 'y'",
        "REBUILD INDEX i ON t",
        "IF $x { UPDATE counter:board SET last = 1 }",
    ] {
        assert!(mutates(write), "{write}");
    }
    for read in [
        "RETURN (SELECT * FROM task WHERE updated > $since);",
        "LET $created = (SELECT VALUE created FROM ONLY task:x); RETURN $created;",
        "RETURN IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { 1 };",
        "SELECT * FROM message WHERE kind IN ['system', 'msg'] ORDER BY sid",
        "RETURN { removed: $removed_at, inserted_by: $who };",
    ] {
        assert!(!mutates(read), "{read}");
    }
}

#[test]
fn this_builds_schema_is_the_version_and_its_neighbours_one_either_side() {
    let schema = Schema::this_build();
    assert_eq!(schema.version(), VERSION);
    assert_eq!(schema.steps(), DEFINE);
    let next = schema.then("DEFINE TABLE IF NOT EXISTS later SCHEMALESS;");
    assert_eq!(next.version(), VERSION + 1);
    assert!(next.steps().starts_with(DEFINE));
    assert!(
        next.steps()
            .ends_with("DEFINE TABLE IF NOT EXISTS later SCHEMALESS;")
    );
    let before = schema.before();
    assert_eq!(before.version(), VERSION - 1);
    assert_eq!(before.steps(), DEFINE);
}

/// The writers' table arrives with its own schema raise, schemaless and with no event, so builds
/// of other schemas write their line and a line written moves no marker.
#[test]
fn the_writers_table_is_schemaless_with_no_event() {
    assert!(DEFINE.contains("DEFINE TABLE IF NOT EXISTS writer SCHEMALESS;"));
    assert!(!DEFINE.contains(" ON writer"), "an event or a field on it");
}
