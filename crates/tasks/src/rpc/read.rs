//! A request that only reads, and the one more sending it may take. A request written on a
//! connection that then gave no answer may have run on the server; a write run twice is a message
//! said twice, so it is never sent again. A read run twice changes nothing: one that got no answer
//! goes once more, on a new connection, and never a third time. A request is a read only when its
//! text proves it ([`Kind::of`]). A request that went out and got no whole answer within the wait,
//! on a connection that worked, says that the server is slow ([`slow`]), not that it is gone.

use super::{Client, Failure, Step};
use std::net::TcpStream;

/// Whether a request only reads, so that sending it twice changes no more than sending it once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Each statement reads, and nothing inside any of them writes: one that got no answer goes once
    /// more.
    Read,
    /// It writes, or [`Kind::of`] cannot prove it does not: one that got no answer is never sent
    /// again.
    Write,
}

/// What each statement of a read may start with.
const READS: [&str; 7] = [
    "SELECT", "RETURN", "LET", "INFO", "BEGIN", "COMMIT", "CANCEL",
];
/// SurrealQL's words for a change, wherever they stand: a statement, or a subquery or a block
/// inside a read's (`RETURN (CREATE …)`).
const WRITES: [&str; 13] = [
    "CREATE", "UPDATE", "UPSERT", "DELETE", "INSERT", "RELATE", "DEFINE", "REMOVE", "ALTER",
    "REBUILD", "KILL", "LIVE", "ACCESS",
];
/// The function namespaces a read may call, each of whose functions only computes from what it is
/// given and what it reads. Any other (a defined function's `fn`, a module, a request beyond the
/// server, a sequence, a file, one this list has not met) may change something.
const PURE: [&str; 9] = [
    "array", "crypto", "math", "object", "record", "search", "string", "type", "vector",
];

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn among(words: &[&str], found: &str) -> bool {
    words.iter().any(|w| found.eq_ignore_ascii_case(w))
}

impl Kind {
    /// `sql`'s kind, from its text alone: a read when it has a statement, every statement starts
    /// with one of [`READS`], no word anywhere in it is one of [`WRITES`], and every `::` in it is
    /// a call in a namespace of [`PURE`], written so that the words read it ([`namespace`]). The
    /// board's schema computes nothing a read could run (no function, no parameter, no future; its
    /// events run on writes), so such a request changes nothing. This reads words, not SurrealQL,
    /// and so errs one way only: a write's word or a `::` in a string, a comment or a name, a call
    /// in a namespace not on the list, or a statement that starts otherwise (`IF`, `FOR`, a block),
    /// makes a write of a request that only reads, which is then never sent again. A write is never
    /// taken for a read.
    pub fn of(sql: &str) -> Kind {
        let mut statements = sql
            .split(';')
            .map(str::trim_start)
            .filter(|statement| !statement.is_empty())
            .peekable();
        let reads = statements.peek().is_some()
            && statements.all(|s| among(&READS, s.split(|c| !word(c)).next().unwrap_or("")));
        let writes = sql.split(|c| !word(c)).any(|found| among(&WRITES, found));
        let calls_compute = sql.match_indices("::").all(|(at, _)| {
            sql[at + 2..].starts_with(word)
                && namespace(&sql[..at]).is_some_and(|first| among(&PURE, first))
        });
        if reads && !writes && calls_compute {
            Kind::Read
        } else {
            Kind::Write
        }
    }
}

/// The first word of the path whose last `::` comes right after `before`, back over each `word::`
/// before it: `vector` of `vector::similarity::cosine`. None when no word stands right against a
/// `::` of it: SurrealQL takes the space, the line or the comment in `fn ::bump()` or
/// `fn/**/::bump()` for nothing, and the call is `fn::bump`, whose namespace the words cannot see.
fn namespace(before: &str) -> Option<&str> {
    let mut rest = before;
    loop {
        let earlier = rest.trim_end_matches(word);
        let name = &rest[earlier.len()..];
        if name.is_empty() {
            return None;
        }
        match earlier.strip_suffix("::") {
            Some(path) => rest = path,
            None => return Some(name),
        }
    }
}

/// What a failure says of a server that took the request on a connection that worked and gave no
/// whole answer within the wait: SurrealDB's client's and PostgreSQL's ([`crate::pg`]) alike.
pub(crate) const SLOW: &str = " is slow: no answer within ";

/// Whether `error` is a request's failure ([`Client::query`], or [`crate::pg::Client`]'s on a
/// request or a startup) on a server that was slow, not unreachable: the connection worked and the
/// answer was late. Whatever wraps it keeps its words.
pub fn slow(error: &str) -> bool {
    error.contains(SLOW)
}

impl Failure {
    /// Whether the request was written, or begun: the server may have run it.
    pub(super) fn written(&self) -> bool {
        matches!(self.step, Step::Write | Step::Read)
    }

    /// Whether the request went out on a working connection and its answer did not come in time.
    fn late(&self) -> bool {
        self.timed_out && self.written()
    }

    /// Where this failure met the request: the step, the socket and its history, and why.
    pub(super) fn on(&self, pooled: bool, served: usize) -> String {
        let origin = if pooled { "pooled" } else { "freshly opened" };
        format!(
            "{} failed on a {origin} socket after {served} completed requests: {}",
            self.step, self.why
        )
    }
}

impl Client {
    /// What `failure` says of the server: slow, when the request went out on a working connection
    /// and no whole answer came within the wait; else that it did not answer.
    pub(super) fn verdict(&self, failure: &Failure) -> String {
        if failure.late() {
            format!("{}{:?}", SLOW.trim_start(), self.timeout)
        } else {
            "did not answer".into()
        }
    }

    /// `request`, a read whose first sending got no answer (`first`, on a `pooled` socket after
    /// `served` requests), once more on a new connection: its answer, or both sendings' failures,
    /// the second deciding whether the server is slow, and whether either sending met its timeout.
    /// There is no third.
    pub(super) fn resend(
        &self,
        request: &[u8],
        check: &mut impl FnMut(&TcpStream) -> Result<(), Failure>,
        pooled: bool,
        served: usize,
        first: Failure,
    ) -> Result<(u16, Vec<u8>), (bool, String)> {
        #[cfg(test)]
        let timeout = self.resend_timeout.unwrap_or(self.timeout);
        #[cfg(not(test))]
        let timeout = self.timeout;
        let (again, second) = match self.fresh(check) {
            Ok(connection) => match self.over_within(connection, request, timeout) {
                Ok(answer) => return Ok(answer),
                Err(failed) => failed,
            },
            Err(failure) => (0, failure),
        };
        let outcome = if second.written() {
            "it was not sent a third time"
        } else {
            "it was not sent there"
        };
        let resent_pooled = false; // the second sending always has a new connection
        let said = format!(
            "the server at {} {}: {}; the request only reads, so it went once more, on a new connection: {}; {outcome}",
            self.addr,
            self.verdict(&second),
            first.on(pooled, served),
            second.on(resent_pooled, again)
        );
        Err((first.timed_out || second.timed_out, said))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, slow};

    /// The board's reads, as the store sends them: one transaction of `LET`s and a `RETURN`.
    #[test]
    fn the_boards_reads_are_reads() {
        for sql in [
            "RETURN 7;",
            "SELECT * FROM task;",
            "select value slug from task where status = $status",
            "INFO FOR DB;",
            "BEGIN TRANSACTION;\nLET $tasks = (SELECT * OMIT id FROM task ORDER BY priority, sid);\n\
             LET $slugs = $tasks.slug;\nRETURN { tasks: $tasks, latest: (SELECT VALUE last FROM ONLY \
             counter:message) ?? 0, known: record::exists(type::record('task', $slug)) };\n\
             COMMIT TRANSACTION;",
            "BEGIN TRANSACTION;\nRETURN { known: record::exists(type::record('task', $slug)),\n\
             messages: (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..]) };\nCOMMIT TRANSACTION;",
            "RETURN $slugs.map(|$slug| (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..]));",
            "RETURN object::keys((INFO FOR DB).tables);",
            "BEGIN; RETURN 1; CANCEL;",
            "SELECT created, updated, deleted_by FROM task;",
            "  \n RETURN 1 ;  ;\n",
            "SELECT * OMIT id FROM array::distinct(SELECT VALUE slug FROM link\n    WHERE kind = 'doc' AND \
             (target = $doc OR string::starts_with(target, $doc + '#')))\n    .map(|$s| type::record('task', $s));",
            "SELECT slug, vector::similarity::cosine(embedding, $query) AS score, search::score(1) FROM task_chunk;",
            "RETURN [math::max($a), string::concat($b, '-'), crypto::sha256(<string> $c), !record::exists($d)];",
        ] {
            assert_eq!(Kind::of(sql), Kind::Read, "{sql}");
        }
    }

    /// A call SurrealQL reads past a space, a line or a comment before or after its `::` is a call
    /// the words cannot see the namespace of, and so a write, as is one in a namespace not known
    /// to only compute.
    #[test]
    fn a_call_whose_namespace_the_words_cannot_read_as_pure_is_a_write() {
        for sql in [
            "RETURN fn ::bump();",
            "RETURN fn\n::bump();",
            "RETURN fn\t::bump();",
            "RETURN fn/**/::bump();",
            "RETURN fn /* a comment */ ::bump();",
            "RETURN fn -- a comment\n::bump();",
            "RETURN fn # a comment\n::bump();",
            "RETURN fn // a comment\n::bump();",
            "RETURN fn:: bump();",
            "RETURN fn::/**/bump();",
            "RETURN `fn`::bump();",
            "RETURN ⟨fn⟩::bump();",
            "RETURN FN::bump();",
            "RETURN ::bump();",
            "RETURN type:: record('task', 'a');",
            "RETURN type::record('task', 'a') + fn ::bump();",
            "RETURN time::now();",
            "RETURN 'a::b';",
        ] {
            assert_eq!(Kind::of(sql), Kind::Write, "{sql:?}");
        }
    }

    /// Every statement that changes something, alone, in another case, or inside a read's subquery
    /// or block, and every call that can change something, makes a write.
    #[test]
    fn a_write_anywhere_makes_a_write() {
        for sql in [
            "CREATE task:a SET title = 'x';",
            "UPDATE task SET status = 'done';",
            "UPSERT counter:board SET last = 1;",
            "DELETE task:a;",
            "INSERT INTO task $rows;",
            "RELATE task:a->link->task:b;",
            "DEFINE TABLE task SCHEMALESS;",
            "REMOVE TABLE IF EXISTS copy_a;",
            "ALTER TABLE task COMMENT 'x';",
            "REBUILD INDEX slug ON task;",
            "KILL u'0189d6e3-8eac-703a-9a48-d9faa78b44b9';",
            "LIVE SELECT * FROM task;",
            "ACCESS account GRANT FOR USER tobie;",
            "create task:a;",
            "RETURN (CREATE task:a);",
            "LET $x = (UPDATE task:a SET title = 'y');",
            "SELECT * FROM (DELETE task:a RETURN BEFORE);",
            "BEGIN TRANSACTION;\nRETURN 1;\nUPSERT meta:schema SET version = 3;\nCOMMIT TRANSACTION;",
            "DEFINE NAMESPACE IF NOT EXISTS sf; USE NS sf; DEFINE DATABASE IF NOT EXISTS tasks; USE DB tasks;\nRETURN 1;",
            "RETURN fn::claim($slug);",
            "RETURN http::post('http://example.com', {});",
            "RETURN sequence::nextval('ids');",
            "RETURN file::put(f\"bucket:/a\", 'x');",
            "RETURN mod::plugin::run();",
            "RETURN api::invoke('/x');",
        ] {
            assert_eq!(Kind::of(sql), Kind::Write, "{sql}");
        }
    }

    /// What the words alone cannot prove a read is taken for a write: a request with no statement,
    /// a statement that starts otherwise, and a write's word in a string or a variable's name.
    #[test]
    fn what_cannot_be_proved_a_read_is_a_write() {
        for sql in [
            "",
            " ; ;",
            "IF true { RETURN 1 } ELSE { RETURN 2 };",
            "FOR $x IN [1] { RETURN $x };",
            "THROW 'stale';",
            "USE NS sf;",
            "SLEEP 1s;",
            "SHOW CHANGES FOR TABLE task SINCE 1;",
            "(SELECT * FROM task);",
            "RETURN 'please delete this';",
            "SELECT * FROM task WHERE kind = 'live';",
            "LET $update = 1; RETURN $update;",
        ] {
            assert_eq!(Kind::of(sql), Kind::Write, "{sql:?}");
        }
    }

    #[test]
    fn a_slow_failure_is_known_by_its_words_wherever_it_is_wrapped() {
        let late = "the server at 127.0.0.1:1 is slow: no answer within 3s: read failed on a pooled \
                    socket after 4 completed requests: Resource temporarily unavailable (os error 35); \
                    the request may have run there, so it was not sent again";
        assert!(slow(late), "{late}");
        let wrapped = format!("the board's database: {late}");
        assert!(slow(&wrapped), "{wrapped}");
        for other in [
            "the server at 127.0.0.1:1 did not answer: connect failed on a freshly opened socket after 0 completed requests: Connection refused (os error 61); the request was not sent",
            "the task board is starting",
            "a slow board",
        ] {
            assert!(!slow(other), "{other}");
        }
    }
}
