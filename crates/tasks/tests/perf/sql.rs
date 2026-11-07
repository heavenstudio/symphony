//! The statements a request's SurrealQL holds, as the plan check plans them: every `SELECT`, nested
//! ones too, and every `UPDATE` or `DELETE` of a table under a condition as the `SELECT` of the rows
//! it would change; and the variables a statement reads.
//!
//! Keywords count in any case and end at any byte no name goes on with, as the server takes them:
//! `SELECT*`, `SELECT/**/ *` and `select-- …` then a line are reads as `SELECT *` is. Quoted text
//! (`'…'`, `"…"`, `` `…` ``, a backslash escaping the byte after it) and comments (`--`, `//` or
//! `#` to the end of the line, `/* … */`) hold no statement, and none of their brackets or `;` ends
//! one; a statement keeps its quoted text and has each comment read as a space. Every cut falls on
//! an ASCII byte, which UTF-8 never puts inside a character, so text in any script passes through
//! whole.

/// Names SurrealQL binds itself, and the parameters of a closure, which a statement uses unbound.
const BUILT_IN: [&str; 7] = [
    "parent", "this", "value", "before", "after", "event", "input",
];

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
                (b'\'' | b'"' | b'`', _) => (Part::Quoted, quoted_end(bytes, at)),
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
    /// server reads one: after no byte of a name, a variable or a path (`.`), and followed by a
    /// byte no name goes on with, white space, a comment, `*` or `(` alike.
    fn keyword_at(&self, at: usize, keyword: &str) -> bool {
        let bytes = self.sql.as_bytes();
        let end = at + keyword.len();
        let spelled = bytes
            .get(at..end)
            .is_some_and(|word| word.eq_ignore_ascii_case(keyword.as_bytes()));
        let after_a_name = at > 0 && (is_word(bytes[at - 1]) || bytes[at - 1] == b'.');
        spelled
            && (at..end).all(|inside| self.is_code(inside))
            && !after_a_name
            && bytes.get(end).is_some_and(|&next| !is_word(next))
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

/// Past the quote closing the one at `open`.
fn quoted_end(bytes: &[u8], open: usize) -> usize {
    let quote = bytes[open];
    let mut at = open + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            byte if byte == quote => return at + 1,
            _ => at += 1,
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

/// Whether a name, a variable (`$`) or a record id (`table:id`) goes on with `byte`: a letter
/// outside ASCII is taken to, so no keyword ends inside one.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b':') || !byte.is_ascii()
}

/// Each statement `sql` holds that reads rows of a table under a condition, as a `SELECT`: every
/// `SELECT`, nested ones too, and every `UPDATE TABLE … WHERE …` and `DELETE TABLE WHERE …` as
/// the `SELECT id` of the rows it would change. A `SELECT` ends where the bracket it opened in
/// closes, or at a `;` outside any bracket.
pub fn statements(sql: &str) -> Vec<String> {
    let lexed = Lexed::new(sql);
    let mut found = Vec::new();
    for at in 0..sql.len() {
        if lexed.keyword_at(at, "SELECT") {
            found.push(lexed.text(at, lexed.statement_end(at)));
        } else if (lexed.keyword_at(at, "UPDATE") || lexed.keyword_at(at, "DELETE"))
            && let Some(read) = as_select(&lexed.text(at, lexed.statement_end(at)))
        {
            found.push(read);
        }
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

/// `UPDATE TABLE SET … WHERE COND` or `DELETE TABLE WHERE COND` (a table by its name, not one
/// record) as `SELECT id FROM TABLE WHERE COND`; none for any other.
fn as_select(statement: &str) -> Option<String> {
    let after_verb = statement.get("UPDATE".len()..)?;
    if !after_verb.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = after_verb.trim_start();
    let table: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let after_table = &rest[table.len()..];
    if table.is_empty() || !after_table.starts_with(char::is_whitespace) {
        return None;
    }
    let condition_at = Lexed::new(after_table).top_level("WHERE")? + "WHERE".len();
    let condition = &after_table[condition_at..];
    let condition = (Lexed::new(condition).top_level("RETURN"))
        .map_or(condition, |returning| &condition[..returning]);
    Some(format!("SELECT id FROM {table} WHERE {}", condition.trim()))
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

    /// A keyword ends at any byte no name goes on with, as the server reads it: a comment, `*`,
    /// a line or `(` straight after one starts its statement as a space does. One a name, a
    /// variable, a path or a record id holds starts none.
    #[test]
    fn a_keyword_ends_at_any_byte_no_name_goes_on_with() {
        let sql = "SELECT/**/ * FROM session WHERE kind = 'reviewer';
SELECT* FROM session;
select/* café */ * from session;
SELECT-- comment
* FROM session;
DELETE/**/callback_event WHERE key = 'pull-1';
UPDATE-- the events
callback_event SET attempts = 0 WHERE(handle = $actor);
LET $selected = selection.select; RETURN [select:one, selectédeleted];";
        assert_eq!(
            statements(sql),
            [
                "SELECT  * FROM session WHERE kind = 'reviewer'",
                "SELECT* FROM session",
                "select  * from session",
                "SELECT \n* FROM session",
                "SELECT id FROM callback_event WHERE key = 'pull-1'",
                "SELECT id FROM callback_event WHERE (handle = $actor)",
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
