//! The statements a request's SurrealQL holds, as the plan check plans them: every `SELECT`, nested
//! ones too, and every `UPDATE`, `UPSERT` or `DELETE` of a table under a condition as the `SELECT`
//! of the rows it would change; the statements it runs one after another; and the variables a
//! statement reads.
//!
//! A keyword is read as the server's lexer reads one (SurrealDB 3.3.0, `surrealdb/syn/src/lexer`):
//! in any case, and a word of its own. A name goes on only with an ASCII letter, digit or `_`
//! (`is_identifier_continue`), so a keyword ends at any other byte: white space of any script (the
//! lexer skips sixteen spaces outside ASCII, `U+00A0` and `U+2003` among them), a comment, `*`,
//! `(` or a variable's `$` alike, and `SELECT$projection`, `SELECT/**/ *` and `select-- …` then a
//! line are reads as `SELECT *` is. One starts after any byte but those and the `$` of a variable,
//! the `.` of a path or the `:` of a record id (`$select`, `row.select` and `said:select` are
//! names), and a word followed by a lone `:` is an object's key (`{ select: 1 }`). Quoted text
//! (`'…'`, `"…"`, `` `…` ``, `⟨…⟩`, a backslash escaping the byte after it) and comments (`--`,
//! `//` or `#` to the end of the line, `/* … */`) hold no statement, and none of their brackets or
//! `;` ends one; a statement keeps its quoted text and has each comment read as a space. Every cut
//! falls on a character's first byte, so text in any script passes through whole.

/// Names SurrealQL binds itself, and the parameters of a closure, which a statement uses unbound.
const BUILT_IN: [&str; 7] = [
    "parent", "this", "value", "before", "after", "event", "input",
];

/// The brackets quoting a name as backticks do (`⟨said⟩`).
const OPEN: &str = "⟨";
const CLOSE: &str = "⟩";

/// What a byte of SurrealQL is part of.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Code,
    Quoted,
    Comment,
}

/// SurrealQL with each byte's [`Part`].
struct Lexed<'sql> {
    sql: &'sql str,
    parts: Vec<Part>,
}

impl<'sql> Lexed<'sql> {
    fn new(sql: &'sql str) -> Lexed<'sql> {
        let bytes = sql.as_bytes();
        let mut parts = vec![Part::Code; bytes.len()];
        let mut at = 0;
        while at < bytes.len() {
            let following = bytes.get(at + 1).copied();
            let (part, end) = match (bytes[at], following) {
                (quote @ (b'\'' | b'"' | b'`'), _) => {
                    (Part::Quoted, quoted_end(bytes, at + 1, &[quote]))
                }
                _ if bytes[at..].starts_with(OPEN.as_bytes()) => (
                    Part::Quoted,
                    quoted_end(bytes, at + OPEN.len(), CLOSE.as_bytes()),
                ),
                (b'-', Some(b'-')) | (b'/', Some(b'/')) | (b'#', _) => {
                    (Part::Comment, line_end(bytes, at))
                }
                (b'/', Some(b'*')) => (Part::Comment, block_end(bytes, at)),
                _ => {
                    at += 1;
                    continue;
                }
            };
            parts[at..end].fill(part);
            at = end;
        }
        Lexed { sql, parts }
    }

    fn is_code(&self, at: usize) -> bool {
        self.parts.get(at) == Some(&Part::Code)
    }

    /// Whether `keyword` (upper case) stands at `at` in code, in any case, a word of its own as the
    /// server reads one: it starts after no byte of a name, a variable, a path or a record id
    /// ([`goes_into_a_name`]), ends at a byte no name goes on with ([`goes_on_a_name`]), and is no
    /// object's key.
    fn keyword_at(&self, at: usize, keyword: &str) -> bool {
        let bytes = self.sql.as_bytes();
        let end = at + keyword.len();
        let spelled = bytes
            .get(at..end)
            .is_some_and(|word| word.eq_ignore_ascii_case(keyword.as_bytes()));
        spelled
            && (at..end).all(|inside| self.is_code(inside))
            && (at == 0 || !goes_into_a_name(bytes[at - 1]))
            && bytes.get(end).is_none_or(|&next| !goes_on_a_name(next))
            && !self.a_key_ends(end)
    }

    /// Whether the next code from `from`, past white space and comments, is a lone `:`: the word
    /// before it an object's key (`{ select: 1 }`), which no keyword can be followed by.
    fn a_key_ends(&self, from: usize) -> bool {
        let characters = (self.code_from(from)).filter(|&(at, _)| self.sql.is_char_boundary(at));
        let mut rests = characters.map(|(at, _)| &self.sql[at..]);
        let next = rests.find(|rest| !rest.starts_with(char::is_whitespace));
        next.is_some_and(|rest| rest.starts_with(':') && !rest.starts_with("::"))
    }

    /// Where the statement starting at `from` ends: the bracket closing the one it is in, a `;`
    /// outside its own brackets, or the end of the text.
    fn statement_end(&self, from: usize) -> usize {
        let mut depth = 0usize;
        let code = self.code_from(from);
        for (at, byte) in code {
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' | b';' if depth == 0 => return at,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
        }
        self.sql.len()
    }

    /// Where `keyword` first stands in code outside any bracket.
    fn top_level(&self, keyword: &str) -> Option<usize> {
        let mut depth = 0i32;
        let code = self.code_from(0);
        for (at, byte) in code {
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ if depth == 0 && self.keyword_at(at, keyword) => return Some(at),
                _ => {}
            }
        }
        None
    }

    /// Each byte of code from `from` on, with where it is.
    fn code_from(&self, from: usize) -> impl Iterator<Item = (usize, u8)> + '_ {
        let bytes = self.sql.as_bytes().iter().copied().enumerate();
        bytes.skip(from).filter(|&(at, _)| self.is_code(at))
    }

    /// The text from `from` to `to`, each comment in it read as one space, trimmed.
    fn text(&self, from: usize, to: usize) -> String {
        let mut text = String::new();
        let mut at = from;
        while at < to {
            let comment = self.parts[at] == Part::Comment;
            let run_end = (at..to)
                .find(|&inside| (self.parts[inside] == Part::Comment) != comment)
                .unwrap_or(to);
            if comment {
                text.push(' ');
            } else {
                text.push_str(&self.sql[at..run_end]);
            }
            at = run_end;
        }
        text.trim().to_string()
    }
}

/// Past the `close` that ends the quoted text going on from `from`, a backslash escaping the byte
/// after it.
fn quoted_end(bytes: &[u8], from: usize, close: &[u8]) -> usize {
    let mut at = from;
    while at < bytes.len() {
        if bytes[at] == b'\\' {
            at += 2;
        } else if bytes[at..].starts_with(close) {
            return at + close.len();
        } else {
            at += 1;
        }
    }
    bytes.len()
}

/// The end of the line a comment starting at `open` is on (its newline is code).
fn line_end(bytes: &[u8], open: usize) -> usize {
    (open..bytes.len())
        .find(|&at| bytes[at] == b'\n')
        .unwrap_or(bytes.len())
}

/// Past the `*/` closing the block comment opened at `open`.
fn block_end(bytes: &[u8], open: usize) -> usize {
    (open + 2..bytes.len().saturating_sub(1))
        .find(|&at| bytes[at] == b'*' && bytes[at + 1] == b'/')
        .map_or(bytes.len(), |close| close + 2)
}

/// Whether a name goes on with `byte`, as the server's lexer reads one (`is_identifier_continue`):
/// an ASCII letter, digit or `_`. Any other byte ends a name, a byte of a character outside ASCII
/// (white space of any script among them) too.
fn goes_on_a_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether a word after `byte` goes on something other than a statement: a name, a variable
/// (`$select`), a path (`row.select`) or a record id (`said:select`).
fn goes_into_a_name(byte: u8) -> bool {
    goes_on_a_name(byte) || matches!(byte, b'$' | b'.' | b':')
}

/// The verbs of a statement that changes the rows of a table under a condition.
const CHANGES: [&str; 3] = ["UPDATE", "UPSERT", "DELETE"];

/// Each statement `sql` holds that reads rows of a table under a condition, as a `SELECT`: every
/// `SELECT`, nested ones too, and every `UPDATE TABLE … WHERE …`, `UPSERT TABLE … WHERE …` and
/// `DELETE [FROM] TABLE WHERE …` as the `SELECT id` of the rows it would change. A `SELECT` ends
/// where the bracket it opened in closes, or at a `;` outside any bracket.
pub fn statements(sql: &str) -> Vec<String> {
    let lexed = Lexed::new(sql);
    let mut found = Vec::new();
    for at in 0..sql.len() {
        if lexed.keyword_at(at, "SELECT") {
            found.push(lexed.text(at, lexed.statement_end(at)));
        } else if CHANGES.iter().any(|verb| lexed.keyword_at(at, verb))
            && let Some(read) = as_select(&lexed.text(at, lexed.statement_end(at)))
        {
            found.push(read);
        }
    }
    found
}

/// The statements `sql` runs one after another, each comment read as a space: its text between the
/// `;`s outside any bracket, an empty one left out.
pub fn sequence(sql: &str) -> Vec<String> {
    let lexed = Lexed::new(sql);
    let mut found = Vec::new();
    let mut from = 0;
    while from < sql.len() {
        let end = lexed.statement_end(from);
        let statement = lexed.text(from, end);
        if !statement.is_empty() {
            found.push(statement);
        }
        from = end + 1;
    }
    found
}

/// Each `DEFINE EVENT` statement `sql` holds, whole: what the server runs in the write that fires
/// it, which no client sends.
pub fn events(sql: &str) -> Vec<String> {
    let lexed = Lexed::new(sql);
    let defines = (0..sql.len()).filter(|&at| lexed.keyword_at(at, "DEFINE"));
    let definitions = defines.map(|at| lexed.text(at, lexed.statement_end(at)));
    definitions
        .filter(|definition| {
            let defined = definition["DEFINE".len()..].trim_start();
            Lexed::new(defined).keyword_at(0, "EVENT")
        })
        .collect()
}

/// `UPDATE TABLE SET … WHERE COND`, `UPSERT TABLE SET … WHERE COND` or `DELETE [FROM] TABLE WHERE
/// COND` (a table by its name, bare or quoted, not one record) as `SELECT id FROM TABLE WHERE
/// COND`; none for any other.
fn as_select(statement: &str) -> Option<String> {
    let after_verb = statement.get("UPDATE".len()..)?;
    if !after_verb.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = after_verb.trim_start();
    if statement[.."DELETE".len()].eq_ignore_ascii_case("DELETE")
        && Lexed::new(rest).keyword_at(0, "FROM")
    {
        rest = rest["FROM".len()..].trim_start();
    }
    let table = table_at(rest)?;
    let after_table = &rest[table.len()..];
    // A clause's word follows a table; a record (`said:x`), a function (`type::record(…)`) or a
    // second target goes on otherwise.
    if !(after_table.trim_start()).starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let condition_at = Lexed::new(after_table).top_level("WHERE")? + "WHERE".len();
    let condition = &after_table[condition_at..];
    let condition = (Lexed::new(condition).top_level("RETURN"))
        .map_or(condition, |returning| &condition[..returning]);
    Some(format!("SELECT id FROM {table} WHERE {}", condition.trim()))
}

/// The table name `text` starts with as written, bare (`session`) or quoted (`` `session` ``,
/// `⟨session⟩`); none when it starts with no name.
fn table_at(text: &str) -> Option<&str> {
    let end = if text.starts_with(['`', '⟨']) {
        let lexed = Lexed::new(text);
        (0..text.len()).find(|&at| lexed.parts[at] != Part::Quoted)
    } else {
        text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
    };
    let name = &text[..end.unwrap_or(text.len())];
    (!name.is_empty()).then_some(name)
}

/// The variables `statement` reads (`$name`, in its code), but a closure's parameters and
/// SurrealQL's own.
pub fn variables(statement: &str) -> Vec<String> {
    let lexed = Lexed::new(statement);
    let name_at = |at: usize| -> String {
        statement[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    };
    let dollars = statement
        .match_indices('$')
        .filter(|&(at, _)| lexed.is_code(at));
    let mut names: Vec<String> = dollars
        .map(|(at, _)| name_at(at))
        .filter(|name| !name.is_empty())
        .collect();
    // A closure's head: `|$m|` or `|$a, $b|`.
    let params: Vec<String> = statement
        .split('|')
        .skip(1)
        .step_by(2)
        .filter(|head| {
            head.split(',').all(|param| {
                param.trim().strip_prefix('$').is_some_and(|name| {
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            })
        })
        .flat_map(|head| head.split(',').map(|param| param.trim()[1..].to_string()))
        .collect();
    names.sort();
    names.dedup();
    names.retain(|name| !params.contains(name) && !BUILT_IN.contains(&name.as_str()));
    names
}

#[cfg(test)]
mod reading {
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
    /// without `FROM`, a table's name bare or quoted; a record, a function or a table followed by no
    /// clause changes no rows under a condition.
    #[test]
    fn a_change_of_rows_reads_them_in_any_spelling_of_its_table() {
        let sql = "UPSERT session SET seen = 1 WHERE kind = 'reviewer';
DELETE FROM session WHERE kind = 'x' RETURN BEFORE;
delete from⟨session⟩ where kind = 'y';
DELETE `session`WHERE kind = 'z';
UPDATE ⟨call back⟩ SET a = 1 WHERE b = 2;
UPDATE session:one SET a = 1 WHERE b = 2;
DELETE type::record('session', $h) WHERE b = 2;
UPSERT session, task WHERE b = 2;";
        assert_eq!(
            statements(sql),
            [
                "SELECT id FROM session WHERE kind = 'reviewer'",
                "SELECT id FROM session WHERE kind = 'x'",
                "SELECT id FROM ⟨session⟩ WHERE kind = 'y'",
                "SELECT id FROM `session` WHERE kind = 'z'",
                "SELECT id FROM ⟨call back⟩ WHERE b = 2",
            ]
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
}
