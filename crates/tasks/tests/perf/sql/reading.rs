//! How [`super`] reads SurrealQL: a statement in every spelling the server takes and none in a
//! quoted value, a comment, a name or an object's key; an event whole; the statements a request
//! runs; the variables a statement reads.

use super::*;

#[test]
fn every_select_is_found_nested_ones_too_and_ends_where_its_bracket_closes() {
    let sql = "LET $me = (SELECT connected FROM ONLY type::record('session', $handle));
RETURN { a: (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'), b: IF $x { (SELECT * FROM task) } ELSE { [] } };
RETURN SELECT * OMIT id FROM message WHERE recipient = 'owner; SELECT not' ORDER BY sid DESC LIMIT $limit;";
    assert_eq!(
        statements(sql),
        [
            "SELECT connected FROM ONLY type::record('session', $handle)",
            "SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'",
            "SELECT * FROM task",
            "SELECT * OMIT id FROM message WHERE recipient = 'owner; SELECT not' ORDER BY sid DESC LIMIT $limit",
        ]
    );
}

#[test]
fn a_subquery_in_a_select_is_its_own_statement_as_well() {
    let sql = "SELECT * FROM session WHERE handle IN (SELECT VALUE handle FROM role WHERE role = 'orchestrator');";
    assert_eq!(
        statements(sql),
        [
            "SELECT * FROM session WHERE handle IN (SELECT VALUE handle FROM role WHERE role = 'orchestrator')",
            "SELECT VALUE handle FROM role WHERE role = 'orchestrator'",
        ]
    );
}

#[test]
fn an_update_or_delete_of_a_table_under_a_condition_reads_as_the_select_of_its_rows() {
    let sql = "UPDATE callback_event SET attempts = 0 WHERE handle = $actor AND delivered != true RETURN NONE;
DELETE callback_event WHERE handle = $handle AND string::starts_with(key, 'pull-') RETURN NONE;
UPDATE type::record('callback_lease', $handle) SET until = 0 WHERE revision = $revision RETURN NONE;
UPDATE counter:board SET last = 1;
DELETE task_chunk RETURN NONE;";
    assert_eq!(
        statements(sql),
        [
            "SELECT id FROM callback_event WHERE handle = $actor AND delivered != true",
            "SELECT id FROM callback_event WHERE handle = $handle AND string::starts_with(key, 'pull-')",
        ]
    );
}

/// The server takes a keyword in any case, so the check does: a lower-case read is planned as
/// its upper-case twin is, and so is a lower-case change of a table's rows.
#[test]
fn a_keyword_in_any_case_starts_a_statement() {
    let sql = "select * from session where kind = 'reviewer';
Select VALUE sid From message;
update callback_event set attempts = 0
where handle = $actor return none;
delete callback_event where key = 'pull-1';
LET $selected = 1; RETURN deleted_at;";
    assert_eq!(
        statements(sql),
        [
            "select * from session where kind = 'reviewer'",
            "Select VALUE sid From message",
            "SELECT id FROM callback_event WHERE handle = $actor",
            "SELECT id FROM callback_event WHERE key = 'pull-1'",
        ]
    );
}

/// A keyword ends at any byte no name goes on with, as the server's lexer reads it: a comment,
/// `*`, a line, `(`, a variable's `$` or white space outside ASCII straight after one starts
/// its statement as a space does, and so does a keyword after a space outside ASCII.
#[test]
fn a_keyword_ends_at_any_byte_no_name_goes_on_with() {
    let sql = "SELECT/**/ * FROM session WHERE kind = 'reviewer';
SELECT* FROM session;
select/* café */ * from session;
SELECT-- comment
* FROM session;
SELECT$projection FROM session;
SELECT\u{a0}* FROM session;
SELECT\u{2003}*\u{3000}FROM session;
RETURN\u{a0}SELECT * FROM task;
DELETE/**/callback_event WHERE key = 'pull-1';
UPDATE-- the events
callback_event SET attempts = 0 WHERE(handle = $actor);
DELETE\u{2003}callback_event\u{a0}WHERE$pull RETURN NONE;";
    assert_eq!(
        statements(sql),
        [
            "SELECT  * FROM session WHERE kind = 'reviewer'",
            "SELECT* FROM session",
            "select  * from session",
            "SELECT \n* FROM session",
            "SELECT$projection FROM session",
            "SELECT\u{a0}* FROM session",
            "SELECT\u{2003}*\u{3000}FROM session",
            "SELECT * FROM task",
            "SELECT id FROM callback_event WHERE key = 'pull-1'",
            "SELECT id FROM callback_event WHERE (handle = $actor)",
            "SELECT id FROM callback_event WHERE $pull",
        ]
    );
}

/// A word a name goes on with, or one that goes on a variable, a path or a record id, is a name
/// and starts no statement; nor does an object's key, a lone `:` after it past white space
/// and comments, nor a name quoted in `⟨…⟩`.
#[test]
fn a_keyword_inside_a_name_or_as_a_key_starts_none() {
    let sql = "LET $selected = selection.select;
RETURN [$select, said:select, fn::update, _select, select_one, select2, ⟨select * from session⟩];
RETURN { select: 1, update : 2, delete/**/: 3, where\u{a0}: 4, `select`: 5 };
RETURN session:⟨delete⟩;";
    assert_eq!(statements(sql), Vec::<String>::new());
}

/// `UPSERT` of a table under a condition reads its rows as `UPDATE` does, `DELETE` with or
/// without `FROM`, a table's name bare or quoted, a quoted one straight after the verb too; a
/// record, a function, a variable or a table followed by no clause changes no rows under a
/// condition.
#[test]
fn a_change_of_rows_reads_them_in_any_spelling_of_its_table() {
    let sql = "UPSERT session SET seen = 1 WHERE kind = 'reviewer';
DELETE FROM session WHERE kind = 'x' RETURN BEFORE;
delete from⟨session⟩ where kind = 'y';
DELETE `session`WHERE kind = 'z';
UPDATE ⟨call back⟩ SET a = 1 WHERE b = 2;
DELETE`session` WHERE b = 3;
UPSERT⟨session⟩ SET a = 1 WHERE b = 4;
UPDATE session:one SET a = 1 WHERE b = 2;
UPDATE`session`:one SET a = 1 WHERE b = 2;
DELETE⟨session⟩:one WHERE b = 2;
DELETE type::record('session', $h) WHERE b = 2;
DELETE$table WHERE b = 2;
UPSERT session, task WHERE b = 2;";
    assert_eq!(
        statements(sql),
        [
            "SELECT id FROM session WHERE kind = 'reviewer'",
            "SELECT id FROM session WHERE kind = 'x'",
            "SELECT id FROM ⟨session⟩ WHERE kind = 'y'",
            "SELECT id FROM `session` WHERE kind = 'z'",
            "SELECT id FROM ⟨call back⟩ WHERE b = 2",
            "SELECT id FROM `session` WHERE b = 3",
            "SELECT id FROM ⟨session⟩ WHERE b = 4",
        ]
    );
}

/// Quoted text is read as a space and the rest kept whole: what a plan's expression holds
/// outside its values.
#[test]
fn unquoted_text_keeps_all_but_its_quoted_values() {
    assert_eq!(
        unquoted("{ note: 'status (Ready)', \"it's\": `a b`, ⟨c⟩: (ProjectValue) } é"),
        "{ note:  ,  :  ,  : (ProjectValue) } é"
    );
}

/// The statements a request runs one after another, past quoted `;`s, brackets and comments.
#[test]
fn a_request_runs_its_statements_one_after_another() {
    let sql = "BEGIN; LET $x = (SELECT * FROM a; ); -- no; statement
RETURN { a: 'b;c' } /* ; */ ;; IF $x { UPDATE a SET b = 1; } ;
COMMIT";
    assert_eq!(
        sequence(sql),
        [
            "BEGIN",
            "LET $x = (SELECT * FROM a; )",
            "RETURN { a: 'b;c' }",
            "IF $x { UPDATE a SET b = 1; }",
            "COMMIT",
        ]
    );
}

/// Each event a schema defines is read whole, the braces and `;` of its body inside it, in
/// any case and after a comment; no other definition is.
#[test]
fn an_event_definition_is_read_whole_and_no_other_definition() {
    let sql = "DEFINE TABLE IF NOT EXISTS message SCHEMAFULL;
DEFINE EVENT OVERWRITE said ON message WHEN $event != 'UPDATE' THEN {
    IF $event != 'CREATE' { LET $was = (SELECT sid FROM said); };
};
define event/**/kept ON task THEN { UPDATE counter:board SET last = 1 RETURN NONE; };
DEFINE INDEX IF NOT EXISTS message_sid ON message FIELDS sid; -- DEFINE EVENT noted ON x THEN {};";
    assert_eq!(
        events(sql),
        [
            "DEFINE EVENT OVERWRITE said ON message WHEN $event != 'UPDATE' THEN {\n    IF $event != 'CREATE' { LET $was = (SELECT sid FROM said); };\n}",
            "define event kept ON task THEN { UPDATE counter:board SET last = 1 RETURN NONE; }",
        ]
    );
}

/// A comment holds no statement, and none of its quotes, brackets or `;` count: the statement
/// around one runs on past it, the comment read as a space.
#[test]
fn a_comment_holds_no_statement_and_ends_none() {
    let sql = "-- SELECT nothing; it's a note
# select none (
/* SELECT x; ( it's */ SELECT * FROM task // the end; )
WHERE status = 'ready';
DELETE callback_event /* no WHERE here */ RETURN NONE;";
    assert_eq!(
        statements(sql),
        ["SELECT * FROM task  \nWHERE status = 'ready'"]
    );
}

/// A quote a backslash escapes leaves its text quoted, so a `;` or bracket after it is text.
#[test]
fn an_escaped_quote_keeps_its_text_quoted() {
    let sql = r#"SELECT * FROM message WHERE text = 'it\'s; (' AND kind = "x\"; [" -- it's
ORDER BY sid; SELECT VALUE sid FROM said;"#;
    assert_eq!(
        statements(sql),
        [
            "SELECT * FROM message WHERE text = 'it\\'s; (' AND kind = \"x\\\"; [\"  \nORDER BY sid",
            "SELECT VALUE sid FROM said",
        ]
    );
}

/// Text outside ASCII, in a comment before a statement or quoted in one, passes whole, and its
/// bytes start no statement.
#[test]
fn text_in_any_script_passes_whole() {
    let commented = "// unicode comment: café\nSELECT * FROM session WHERE kind = 'reviewer'";
    assert_eq!(
        statements(commented),
        ["SELECT * FROM session WHERE kind = 'reviewer'"]
    );
    let quoted = "SELECT * FROM message WHERE text = 'café; SELECT ü' AND sender = $é_name;";
    assert_eq!(
        statements(quoted),
        ["SELECT * FROM message WHERE text = 'café; SELECT ü' AND sender = $é_name"]
    );
    assert_eq!(variables(quoted), Vec::<String>::new());
}

#[test]
fn a_closure_parameter_and_surreals_own_names_are_not_variables_to_bind() {
    assert_eq!(
        variables(
            "SELECT * FROM $pool.filter(|$m| $m.sid > $since).map(|$a, $b| $a) WHERE x = $parent.handle AND y = '$quoted' -- $noted"
        ),
        ["pool", "since"]
    );
}
