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
    guard_vars(&mut vars, 11, &refusal, "refused: ");
    assert_eq!(
        vars,
        json!({
            "slug": "a/b",
            "board_schema": 11,
            "board_schema_before": "refused: the alpha board is at schema ",
            "board_schema_after": refusal.after,
        })
    );
    // A write with no variables of its own is given an object for the guard's.
    let mut none = serde_json::Value::Null;
    guard_vars(&mut none, 11, &refusal, "refused: ");
    assert_eq!(none["board_schema"], 11);
    assert!(GUARD.contains("$board_schema_found != $board_schema"));
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
