//! The copy of a whole board: a dump in JSON, which `Store::copy` puts on either database in one
//! transaction (`tasks-copy`, into the database the board is in), and `Store::dump` reads it back in
//! the same shape, each table in the order of its key.

use crate::children::Child as _;
use crate::common::{Backend, Fixture, NOW, Seed, client, server_addr, session, task};
use serde_json::json;
use symphony_board::rows::{
    AskRow, BarredKeyRow, DeliveryKeyRow, DeliveryRow, Dump, HeldRow, LinkRow, MessageRow, RoleRow,
    SessionRow, TaskRow, TaskSessionRow, UsageRow,
};
use symphony_board::sys::{self, Command};
use symphony_board::{Database, Error, Opening, Store, cli, copy_main};

/// One row of every table, each optional column both set and not, in key order.
pub fn everything() -> Dump {
    Dump {
        tasks: vec![
            TaskRow {
                id: 1,
                body: "Body.".into(),
                priority: 1,
                ..task("sim", "ready")
            },
            TaskRow {
                id: 5,
                parent: Some("sim".into()),
                instruction: true,
                ..task("sim/jump", "in_progress")
            },
        ],
        task_links: vec![
            LinkRow {
                slug: "sim/jump".into(),
                kind: "dep".into(),
                target: "sim".into(),
            },
            LinkRow {
                slug: "sim/jump".into(),
                kind: "doc".into(),
                target: "docs/game/SPEC.md#Jumping".into(),
            },
        ],
        sessions: vec![
            session("amber-heron"),
            SessionRow {
                parent: Some("amber-heron".into()),
                pid: Some(77),
                host: "elsewhere".into(),
                left_at: Some(NOW + 1.0),
                agent: Some("agent-1".into()),
                busy_until: Some(NOW + 2.0),
                busy_call: Some("toolu_9".into()),
                last_tool: Some(NOW + 3.0),
                ..session("brisk-finch")
            },
        ],
        roles: vec![
            RoleRow {
                slug: "sim/jump".into(),
                handle: "amber-heron".into(),
                role: "executor".into(),
                since: NOW,
                expires: None,
            },
            RoleRow {
                slug: "sim/jump".into(),
                handle: "brisk-finch".into(),
                role: "reserved".into(),
                since: NOW,
                expires: Some(NOW + 60.0),
            },
        ],
        messages: vec![
            MessageRow {
                id: 4,
                slug: "sim/jump".into(),
                sender: "owner".into(),
                recipient: None,
                kind: "msg".into(),
                text: "go — ✓".into(),
                ts: NOW,
            },
            MessageRow {
                id: 9,
                slug: "sim/jump".into(),
                sender: "amber-heron".into(),
                recipient: Some("@reviewer".into()),
                kind: "system".into(),
                text: "x".into(),
                ts: NOW,
            },
        ],
        deliveries: vec![DeliveryRow {
            message: 4,
            handle: "amber-heron".into(),
            acked: NOW + 1.0,
        }],
        barred_keys: vec![BarredKeyRow { key: "0b9e".into() }],
        task_sessions: vec![TaskSessionRow {
            slug: "sim/jump".into(),
            handle: "amber-heron".into(),
            role: "executor".into(),
            claude_session: "c-1".into(),
            worktree: "/w".into(),
            branch: "b".into(),
            pane: "p".into(),
            model: "m".into(),
            started: NOW,
            closed: Some(NOW + 5.0),
            closed_why: "done".into(),
            restore: "r".into(),
        }],
        delivery_keys: vec![DeliveryKeyRow {
            slug: "sim/jump".into(),
            key: "main-red:3e0c".into(),
            message: 4,
            ts: NOW + 2.0,
        }],
        asks: vec![
            AskRow {
                id: 2,
                kind: "chat".into(),
                about: r#"["chat","7"]"#.into(),
                thread: Some("7".into()),
                words: "is it on?".into(),
                asked: NOW,
                lines: 1,
                last: NOW,
                open: true,
                gone: Some("chat 7 is gone from the ask store".into()),
                gone_since: Some(NOW + 5.0),
                ..AskRow::default()
            },
            AskRow {
                id: 3,
                kind: "verdict".into(),
                about: r#"["card","tools/x","docs/DECISIONS.md","owner-asks"]"#.into(),
                path: Some("docs/DECISIONS.md".into()),
                card: Some("owner-asks".into()),
                block: Some("12".into()),
                tree: Some("tools/x".into()),
                task: Some("sim/jump".into()),
                thread: Some("8".into()),
                words: "why not B?".into(),
                asked: NOW,
                lines: 2,
                last: NOW + 1.0,
                holder: Some("brisk-finch".into()),
                held: Some(NOW + 600.0),
                before: vec![HeldRow {
                    handle: "amber-heron".into(),
                    since: NOW,
                    until: NOW + 600.0,
                    why: "no answer in 10 min".into(),
                }],
                open: false,
                answered: Some(NOW + 700.0),
                answered_by: Some("brisk-finch".into()),
                answer: Some("the card".into()),
                closed: true,
                gone: None,
                gone_since: None,
            },
        ],
        usages: vec![
            UsageRow {
                handle: "amber-heron".into(),
                slug: "sim/jump".into(),
                role: "executor".into(),
                model: "claude-opus-5-5".into(),
                speed: "standard".into(),
                round: 2,
                input: 12,
                cache_write_5m: 3_400,
                cache_write_1h: 560,
                cache_read: 78_900,
                output: 1_200,
                calls: 7,
                first: NOW,
                last: NOW + 90.0,
            },
            UsageRow {
                handle: "brisk-finch".into(),
                slug: String::new(),
                role: "orchestrator".into(),
                model: "claude-opus-5-5".into(),
                speed: "fast".into(),
                round: 0,
                input: 1,
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_read: 4_000,
                output: 30,
                calls: 1,
                first: NOW + 1.0,
                last: NOW + 1.0,
            },
        ],
        // Execution attempts' rows are documents, each its record's id among its fields, in the
        // order of those ids; a field unset is absent, an object or a list kept whole.
        execution_attempts: vec![json!({
            "attempt": ATTEMPT, "task": "sim/jump", "configuration": "fake", "created": 1_000,
            "identity": {"lane": "executor", "handle": "amber-heron",
                "input": {"task_revision": "r1", "manifest": {"reference": "m", "digest": "md"}},
                "capabilities": {"operations": ["Start", "Observe"], "authoritative_absence": false}},
            "state": {"dispatch": "terminal", "ending": "succeeded", "sequence": 1, "pending": null},
            "revision": 4, "open": false, "live": false, "epoch": 2, "facts": 2,
        })],
        execution_facts: vec![
            json!({"attempt": ATTEMPT, "seq": 1, "at": 1_000,
                "fact": {"kind": "admitted", "initiator": {"kind": "Human", "issuer": "local", "subject": "owner"}}}),
            json!({"attempt": ATTEMPT, "seq": 2, "at": 1_200, "fact": {"kind": "ended", "ending": "succeeded"}}),
        ],
        execution_fences: vec![
            json!({"key": ["capacity", "pool"], "reserved": [], "limit": 2}),
            json!({"key": ["lane", "sim/jump", "executor"], "attempt": ATTEMPT}),
            json!({"key": ["request", "the-key"], "attempt": ATTEMPT, "digest": "dd"}),
        ],
        execution_artifacts: vec![json!({
            "attempt": ATTEMPT, "name": "logs/run.txt", "at": 1_500,
            "artifact": {"name": "logs/run.txt", "reference": "store://1", "digest": "ad", "size": 12,
                "generation": "g1", "runtime_epoch": 0, "verified": true},
        })],
        workflow: Vec::new(),
    }
}

/// The attempt `everything` holds.
const ATTEMPT: &str = "00112233445566778899aabbccddeeff";

/// An environment that names no board: the commands given an address read nothing from it.
fn no_env() -> cli::Env {
    cli::Env::default()
}

/// The environment of a run that reaches `db` on the tests' server of its database, as
/// `TASKS_SERVER` and `TASKS_DATABASE` point a run at a board.
fn board_env(db: &Database) -> cli::Env {
    cli::Env {
        vars: [
            ("TASKS_SERVER".to_string(), db.addr()),
            ("TASKS_DATABASE".to_string(), db.database().to_string()),
        ]
        .into_iter()
        .collect(),
        cwd: std::env::temp_dir(),
    }
}

/// `tasks-copy ARGS` in `env` with `input` on its standard input: its exit code, what it said and
/// what it refused.
fn copied(args: &[&str], env: &cli::Env, input: &[u8]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|word| (*word).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = copy_main(&args, env, &mut &input[..], &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The board `db` holds, read by a store of the test's own.
fn held(db: &Database) -> Dump {
    Store::opening_on(
        db.another(),
        Box::new(|| NOW),
        Box::new(|_| false),
        "host",
        &Opening::write(),
    )
    .unwrap()
    .dump()
    .unwrap()
}

/// How the copy names where it copied: SurrealDB's namespace and database, PostgreSQL's database.
fn place(db: &Database) -> String {
    if db.is_postgresql() {
        format!("{} at {}", db.database(), db.addr())
    } else {
        format!("sf/{} at {}", db.database(), db.addr())
    }
}

on_both! {
    fn a_copy_reads_back_as_it_was_written(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        assert_eq!(f.store.dump().unwrap(), everything());
    }

    fn copying_again_replaces_the_board(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        let smaller = Seed::default()
            .session("coral-hare")
            .task("docs/x", "backlog")
            .dump;
        f.store.copy(&smaller).unwrap();
        assert_eq!(f.store.dump().unwrap(), smaller);
        f.store.copy(&Dump::default()).unwrap();
        assert_eq!(f.store.dump().unwrap(), Dump::default());
    }

    fn a_dump_the_board_would_refuse_leaves_the_board_as_it_was(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        let mut bad = Seed::default()
            .session("coral-hare")
            .task("docs/x", "backlog")
            .dump;
        bad.messages.push(MessageRow {
            id: 1,
            slug: "docs/x".into(),
            sender: "coral-hare".into(),
            kind: "shout".into(),
            text: "!".into(),
            ts: NOW,
            ..Default::default()
        });
        match f.store.copy(&bad) {
            Err(Error::Db(why)) => assert!(why.contains("shout"), "{why}"),
            other => panic!("expected the server to refuse the copy, got {other:?}"),
        }
        assert_eq!(f.store.dump().unwrap(), everything());
    }

    fn a_board_opened_again_is_kept(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        assert_eq!(f.other().dump().unwrap(), everything());
    }

    fn a_null_inside_a_tasks_fields_is_copied_and_read_back(backend: Backend) {
        let mut dump = everything();
        dump.tasks[0].fields = [(
            "fixture".into(),
            [(
                "settings".into(),
                serde_json::json!({"empty": null, "items": [null, {"value": null}]}),
            )]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect();
        let f = Fixture::on(backend, Seed { dump: dump.clone() });
        assert_eq!(f.store.dump().unwrap(), dump);
    }

    fn a_board_with_nothing_staged_removes_no_staging(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        f.store.remove_staging().unwrap();
        assert_eq!(f.store.dump().unwrap(), everything());
    }
}

#[test]
fn the_copy_command_reads_a_dump_and_says_what_it_copied() {
    let db = client();
    let dump = serde_json::to_vec(&everything()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = [server_addr(), db.database().to_string()];
    let code = copy_main(&args, &no_env(), &mut dump.as_slice(), &mut out, &mut err);
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!(
            "tasks-copy: 2 tasks, 2 links, 2 sessions, 2 roles, 2 messages, 1 deliveries, 1 barred keys, 1 task sessions, 1 delivery keys, 2 asks, 2 usage rows into sf/{} at {}\n",
            db.database(),
            server_addr()
        )
    );
    assert_eq!(Store::open(db).unwrap().dump().unwrap(), everything());
}

#[test]
fn the_copy_command_takes_a_board_written_before_the_added_columns() {
    let db = client();
    let old = br#"{"tasks": [{"id": 1, "slug": "a", "title": "A", "body": "", "status": "ready", "priority": 2,
        "parent": null, "created_by": "owner", "created": 1.0, "updated": 1.0}],
        "sessions": [{"handle": "h", "kind": "worker", "parent": null, "worktree": "", "branch": "", "model": "",
        "pid": null, "host": "", "affinity": "", "connected": 1.0, "last_seen": 1.0, "left_at": null}]}"#;
    let args = [server_addr(), db.database().to_string()];
    let code = copy_main(
        &args,
        &no_env(),
        &mut old.as_slice(),
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert_eq!(code, 0);
    let store = Store::open(db).unwrap();
    assert!(!store.get("a").unwrap().instruction);
    assert_eq!(store.session("h").unwrap().row.busy_until, None);
}

#[test]
fn the_copy_command_refuses_in_one_line() {
    let refusal = |args: &[String], input: &[u8]| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = copy_main(args, &no_env(), &mut &input[..], &mut out, &mut err);
        assert!(out.is_empty());
        (code, String::from_utf8(err).unwrap())
    };
    let (code, said) = refusal(&[], b"{}");
    assert_eq!(
        (code, said.as_str()),
        (
            2,
            "❌ tasks-copy: usage: tasks-copy ADDR [DATABASE] < dump.json, or tasks-copy --board [--out]\n"
        )
    );
    let (code, said) = refusal(&[server_addr(), client().database().into()], b"{\xff}");
    assert_eq!(code, 2);
    assert!(
        said.starts_with("❌ tasks-copy: cannot read the dump: "),
        "{said}"
    );
    assert_eq!(said.lines().count(), 1);
    let (code, said) = refusal(&[server_addr(), client().database().into()], b"not json");
    assert_eq!(code, 2);
    assert!(
        said.starts_with("❌ tasks-copy: the dump is not a board's JSON: "),
        "{said}"
    );
    assert_eq!(said.lines().count(), 1);
    let (code, said) = refusal(&[server_addr(), "t-x".into()], b"{}");
    assert_eq!(
        (code, said.as_str()),
        (
            2,
            "❌ tasks-copy: \"t-x\" is not a plain name for a database\n"
        )
    );
    let (code, said) = refusal(&["127.0.0.1:1".into()], b"{}");
    assert_eq!(code, 2);
    assert!(
        said.starts_with("❌ tasks-copy: the board's database: "),
        "{said}"
    );
    assert_eq!(said.lines().count(), 1);
}

/// What the copy does not know it would drop, so a dump with a column or a table added to the SQLite
/// board since is refused whole, naming it, until the copy knows it.
#[test]
fn the_copy_command_refuses_a_column_or_a_table_it_does_not_know_by_name() {
    let f = Fixture::new(Seed { dump: everything() });
    let args = [server_addr(), f.other_client().database().to_string()];
    let refusal = |dump: &serde_json::Value| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let input = serde_json::to_vec(dump).unwrap();
        let code = copy_main(&args, &no_env(), &mut input.as_slice(), &mut out, &mut err);
        assert!(out.is_empty());
        (code, String::from_utf8(err).unwrap())
    };
    let mut column = serde_json::to_value(everything()).unwrap();
    column["tasks"][1]["estimate"] = serde_json::json!(3.5);
    let (code, said) = refusal(&column);
    assert_eq!((code, said.lines().count()), (2, 1), "{said}");
    assert!(
        said.starts_with(
            "❌ tasks-copy: the dump's tasks are not rows the copy knows: unknown field `estimate`"
        ),
        "{said}"
    );
    let mut table = serde_json::to_value(everything()).unwrap();
    table["notes"] = serde_json::json!([{"slug": "sim", "text": "x"}]);
    assert_eq!(
        refusal(&table),
        (
            2,
            "❌ tasks-copy: the dump has a table the copy does not know: notes\n".into()
        )
    );
    assert_eq!(f.store.dump().unwrap(), everything());
}

#[test]
fn the_copy_binary_is_the_command() {
    let db = client();
    // The copy reads the whole dump before it says anything, so the dump is written first.
    let mut child = sys::spawn_piped(
        Command::of(env!("CARGO_BIN_EXE_tasks-copy"))
            .args([server_addr(), db.database().to_string()]),
    )
    .unwrap();
    std::io::Write::write_all(
        child.stdin.as_mut().unwrap(),
        &serde_json::to_vec(&everything()).unwrap(),
    )
    .unwrap();
    let done = sys::wait_output(child).unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(
        String::from_utf8(done.stdout)
            .unwrap()
            .starts_with("tasks-copy: 2 tasks")
    );
    let refused = sys::output(&mut Command::of(env!("CARGO_BIN_EXE_tasks-copy"))).unwrap();
    assert_eq!(refused.status.code(), Some(2));
}

on_both! {
    fn text_that_reads_as_another_type_stays_text(backend: Backend) {
        let mut seed = Seed::default().session("amber-heron");
        let lookalikes = [
            "approve: looks right",
            "a:b",
            "2024-01-01T00:00:00Z",
            "00000000-0000-0000-0000-000000000000",
            "1s",
            "NONE",
            "null",
            "<string> 1",
            "'); DROP TABLE task; --",
            "{\"a\": 1}",
        ];
        for (i, text) in lookalikes.iter().enumerate() {
            let id = i as i64 + 1;
            seed = seed.task_row(TaskRow {
                id,
                title: text.to_string(),
                body: text.to_string(),
                ..task(&format!("t/{id}"), "ready")
            });
            seed.dump.messages.push(MessageRow {
                id,
                slug: format!("t/{id}"),
                sender: "owner".into(),
                kind: "msg".into(),
                text: text.to_string(),
                ts: NOW,
                ..Default::default()
            });
        }
        let f = Fixture::on(backend, seed.clone());
        assert_eq!(f.store.dump().unwrap(), seed.dump);
    }
}

/// A board whose senders each said several things: posts in turn on two threads, one number on
/// both threads (a copy takes one `sid` on two), a verdict, and the board's own lines after a
/// sender's last word, which are nobody's word, one sender having no other.
fn many_words() -> Dump {
    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch")
        .task("sim/a", "ready")
        .task("sim/b", "ready");
    let line = |id: i64, slug: &str, sender: &str, kind: &str| MessageRow {
        id,
        slug: slug.into(),
        sender: sender.into(),
        kind: kind.into(),
        text: format!("{kind} {id} on {slug}"),
        ts: NOW + id as f64,
        ..Default::default()
    };
    let mut messages: Vec<MessageRow> = (1..=40)
        .map(|id| {
            let slug = if id % 3 == 0 { "sim/b" } else { "sim/a" };
            let sender = if id % 2 == 0 {
                "amber-heron"
            } else {
                "brisk-finch"
            };
            line(id, slug, sender, "msg")
        })
        .collect();
    messages.extend([
        // The greatest number twice: the word is the one on the greater slug.
        line(50, "sim/a", "amber-heron", "msg"),
        line(50, "sim/b", "amber-heron", "msg"),
        line(51, "sim/a", "amber-heron", "system"),
        line(52, "sim/a", "brisk-finch", "verdict"),
        line(53, "sim/b", "brisk-finch", "system"),
        line(54, "sim/b", "board", "system"),
    ]);
    messages.sort_by(|a, b| (a.id, &a.slug).cmp(&(b.id, &b.slug)));
    seed.dump.messages = messages;
    seed.dump
}

/// Each sender's last word on the board, as `said` holds it: its row there by the sender, read
/// as it is, and no row for anyone else.
fn said(f: &Fixture, senders: &[&str]) -> Vec<serde_json::Value> {
    let raw = f.store.raw();
    assert_eq!(raw.table("said").count().unwrap(), senders.len() as u64);
    (senders.iter())
        .map(|sender| raw.row("said", serde_json::json!([sender])).get().unwrap())
        .collect()
}

on_both! {
    fn a_copy_leaves_each_senders_last_word_as_its_messages_one_by_one_would(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: many_words() });
        let word = |sid: i64, slug: &str| serde_json::json!({"sid": sid, "slug": slug});
        assert_eq!(
            said(&f, &["amber-heron", "brisk-finch"]),
            [word(50, "sim/b"), word(52, "sim/a")],
            "the greatest number, then the greater slug, of its posts and verdicts alone"
        );
        let last = f.store.last_messages().unwrap();
        assert_eq!(last["amber-heron"].text, "msg 50 on sim/b");
        assert_eq!(last["brisk-finch"].text, "verdict 52 on sim/a");
        // The words are kept from then on as every post keeps them.
        f.store.say("sim/a", "amber-heron", "after the copy", None).unwrap();
        assert_eq!(said(&f, &["amber-heron", "brisk-finch"])[0], word(55, "sim/a"));
        // A copy refused at its last message leaves every word where it was, and they are kept
        // after it as before.
        let mut bad = many_words();
        bad.messages.last_mut().unwrap().kind = "shout".into();
        bad.messages.retain(|m| m.sender != "brisk-finch");
        assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
        assert_eq!(
            said(&f, &["amber-heron", "brisk-finch"]),
            [word(55, "sim/a"), word(52, "sim/a")]
        );
        f.store.say("sim/b", "brisk-finch", "after the refusal", None).unwrap();
        assert_eq!(said(&f, &["amber-heron", "brisk-finch"])[1], word(56, "sim/b"));
        // A board copied with fewer senders holds no word of the ones it lost.
        let mut fewer = many_words();
        fewer.messages.retain(|m| m.sender != "brisk-finch");
        f.store.copy(&fewer).unwrap();
        assert_eq!(said(&f, &["amber-heron"]), [word(50, "sim/b")]);
    }
}

/// The PostgreSQL client of `f`'s board, for a statement of the test's own.
fn postgresql_of(f: &Fixture) -> symphony_board::pg::Client {
    match f.other_database() {
        Database::PostgreSql(client) => client,
        Database::SurrealDb(_) => panic!("this board is on SurrealDB"),
    }
}

/// A copy the server ends (as its limit ends one, here by a trigger that raises the same
/// condition at a table of the test's choosing) is rolled back whole and says in one line how
/// far it was: at its first table, in the middle, and past its last.
#[test]
fn a_postgresql_copy_the_server_ends_says_the_rows_it_reached_and_leaves_the_board() {
    let f = Fixture::on(Backend::PostgreSql, Seed { dump: many_words() });
    let client = postgresql_of(&f);
    client
        .simple(
            "CREATE FUNCTION fixture_ended() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN RAISE EXCEPTION USING ERRCODE = 'query_canceled', \
                 MESSAGE = 'canceling statement due to statement timeout'; END $$;",
        )
        .unwrap();
    let generation = f.store.recovery_generation().unwrap();
    let words = said(&f, &["amber-heron", "brisk-finch"]);
    for (table, reached) in [
        ("task", "it had copied no table and was at the 2 of task"),
        (
            "delivery",
            "it had copied 2 of task, 2 of link, 2 of session, 2 of role, 2 of message and was \
             at the 1 of delivery",
        ),
        (
            "copied_fields",
            "it had copied 2 of task, 2 of link, 2 of session, 2 of role, 2 of message, 1 of \
             delivery, 1 of barred_key, 1 of task_session, 1 of delivery_key, 2 of ask, 2 of \
             usage, 1 of execution_attempt, 2 of execution_fact, 3 of execution_fence, 1 of \
             execution_artifact and was at its counters",
        ),
    ] {
        client
            .simple(&format!(
                "CREATE TRIGGER fixture_ended BEFORE INSERT ON {table} \
                 FOR EACH STATEMENT EXECUTE FUNCTION fixture_ended()"
            ))
            .unwrap();
        match f.store.copy(&everything()) {
            Err(Error::Db(why)) => assert_eq!(
                why,
                format!(
                    "the copy did not finish in 270 s and was ended, the board left as it was: \
                     {reached}"
                )
            ),
            other => panic!("expected the copy ended at {table}, got {other:?}"),
        }
        assert_eq!(f.store.dump().unwrap(), many_words(), "{table}");
        assert_eq!(said(&f, &["amber-heron", "brisk-finch"]), words);
        assert_eq!(f.store.recovery_generation().unwrap(), generation);
        client
            .simple(&format!("DROP TRIGGER fixture_ended ON {table}"))
            .unwrap();
    }
    // The board takes a copy as before once nothing ends it.
    f.store.copy(&everything()).unwrap();
    assert_eq!(f.store.dump().unwrap(), everything());
    assert_ne!(f.store.recovery_generation().unwrap(), generation);
}

/// A board bigger than the server takes in one request (4 MiB): 1200 messages of 4 KiB each.
fn a_big_board() -> Dump {
    let mut seed = Seed::default()
        .session("amber-heron")
        .task("sim/jump", "ready");
    seed.dump.messages = (1..=1200)
        .map(|id| MessageRow {
            id,
            slug: "sim/jump".into(),
            sender: "owner".into(),
            kind: "msg".into(),
            text: format!("{id:04}").repeat(1024),
            ts: NOW,
            ..Default::default()
        })
        .collect();
    seed.dump
}

fn tables(f: &Fixture) -> Vec<String> {
    let found = f
        .store
        .client()
        .query("RETURN (INFO FOR DB).tables;", serde_json::json!({}))
        .unwrap();
    let mut names: Vec<String> = found[0].as_object().unwrap().keys().cloned().collect();
    names.sort();
    names
}

const BOARD_TABLES: [&str; 33] = [
    "artifact_ledger",
    "ask",
    "barred_key",
    "callback_event",
    "callback_lease",
    "callback_state",
    "chat_event",
    "counter",
    "delivery",
    "delivery_key",
    "execution_artifact",
    "execution_attempt",
    "execution_fact",
    "execution_fence",
    "link",
    "message",
    "meta",
    "moved",
    "recovery",
    "role",
    "said",
    "session",
    "session_revision",
    "setting",
    "task",
    "task_chunk",
    "task_embedding_checkpoint",
    "task_embedding_source",
    "task_session",
    "unanswered_restore",
    "usage",
    "workflow",
    "writer",
];

#[test]
fn copy_replaces_derived_vectors_only_when_the_board_swap_commits() {
    let f = Fixture::new(Seed { dump: everything() });
    let raw = f.store.raw();
    let derived = serde_json::json!({
        "slug": "sim/jump", "source": "brief", "source_hash": "old", "model": "fake",
    });
    let mut chunk = derived.clone();
    chunk["text_hash"] = "old".into();
    chunk["text"] = "cached board text".into();
    chunk["embedding"] = serde_json::json!(vec![1.0_f32; 384]);
    raw.row("task_chunk", "one").create(chunk).unwrap();
    let mut source = derived;
    source["parts"] = 1.into();
    raw.row("task_embedding_source", "one")
        .create(source)
        .unwrap();
    (raw.row("task_embedding_checkpoint", "board"))
        .create(serde_json::json!({"snapshot": {"model": "fake"}}))
        .unwrap();
    let counts = || -> Vec<serde_json::Value> {
        [
            "task_chunk",
            "task_embedding_source",
            "task_embedding_checkpoint",
        ]
        .map(|table| serde_json::json!(f.store.raw().table(table).count().unwrap()))
        .to_vec()
    };
    assert_eq!(counts(), vec![serde_json::json!(1); 3]);
    let mut bad = everything();
    bad.messages[0].kind = "shout".into();
    assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(f.store.dump().unwrap(), everything());
    assert_eq!(counts(), vec![serde_json::json!(1); 3]);
    f.store.copy(&Dump::default()).unwrap();
    assert_eq!(f.store.dump().unwrap(), Dump::default());
    assert_eq!(counts(), vec![serde_json::json!(0); 3]);
}

#[test]
fn a_board_bigger_than_one_request_is_copied_whole() {
    let f = Fixture::new(Seed {
        dump: a_big_board(),
    });
    assert_eq!(f.store.dump().unwrap(), a_big_board());
    assert_eq!(
        tables(&f),
        BOARD_TABLES,
        "nothing of the copy is left behind"
    );
}

/// On PostgreSQL a board bigger than SurrealDB's request is still one request, and a copy refused
/// at its last row leaves the board as it was.
#[test]
fn on_postgresql_a_big_board_is_copied_in_one_request_and_refused_whole() {
    let f = Fixture::on(Backend::PostgreSql, Seed { dump: everything() });
    let before = f.store.database().requests();
    f.store.copy(&a_big_board()).unwrap();
    assert_eq!(f.store.database().requests() - before, 1);
    assert_eq!(f.store.dump().unwrap(), a_big_board());
    let mut bad = a_big_board();
    bad.messages.last_mut().unwrap().kind = "shout".into();
    bad.tasks[0].title = "Not this one".into();
    assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(f.store.dump().unwrap(), a_big_board());
}

/// What a copy that ended before its cleanup left staged, or one whose cleanup failed, is
/// cleared by the next copy before it stages, whether that copy is kept or refused.
#[test]
fn a_copy_clears_the_staging_an_earlier_copy_left() {
    let f = Fixture::new(Seed { dump: everything() });
    let leave = || {
        (f.store.client())
            .query(
                "DEFINE TABLE copy_00000000deadbeef_task SCHEMALESS; \
                 DEFINE TABLE copy_00000000deadbeef_message SCHEMALESS; \
                 INSERT INTO copy_00000000deadbeef_message [{ row: { text: 'left' } }];",
                serde_json::json!({}),
            )
            .unwrap();
        assert_eq!(tables(&f).len(), BOARD_TABLES.len() + 2);
    };
    leave();
    f.store.copy(&many_words()).unwrap();
    assert_eq!(tables(&f), BOARD_TABLES);
    assert_eq!(f.store.dump().unwrap(), many_words());
    leave();
    let mut bad = everything();
    bad.messages[0].kind = "shout".into();
    assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(tables(&f), BOARD_TABLES);
    assert_eq!(f.store.dump().unwrap(), many_words());
}

#[test]
fn a_big_dump_refused_late_leaves_the_board_and_nothing_else() {
    let f = Fixture::new(Seed { dump: everything() });
    let mut bad = a_big_board();
    bad.messages.last_mut().unwrap().kind = "shout".into();
    assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(f.store.dump().unwrap(), everything());
    assert_eq!(tables(&f), BOARD_TABLES);
}

on_both! {
    fn board_copies_into_the_board_the_environment_reaches(backend: Backend) {
        let db = backend.database();
        let dump = serde_json::to_vec(&everything()).unwrap();
        let (code, said, err) = copied(&["--board"], &board_env(&db), &dump);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(
            said.starts_with("tasks-copy: 2 tasks, ")
                && said.ends_with(&format!(" into {}\n", place(&db))),
            "{said}"
        );
        assert_eq!(held(&db), everything());
    }

    fn board_out_writes_the_boards_dump_as_the_copy_reads_it_on_one_line(backend: Backend) {
        let f = Fixture::on(backend, Seed { dump: everything() });
        let env = board_env(&f.other_database());
        let (code, text, err) = copied(&["--board", "--out"], &env, b"");
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(text.lines().count(), 1);
        assert_eq!(Dump::from_json(&text).unwrap(), everything());
        assert_eq!(
            f.store.dump().unwrap(),
            everything(),
            "reading it writes nothing"
        );
    }

    /// The address form takes either server: SurrealDB's `HOST:PORT`, PostgreSQL's socket.
    fn the_copy_command_copies_into_the_database_at_an_address(backend: Backend) {
        let db = backend.database();
        let dump = serde_json::to_vec(&everything()).unwrap();
        let (code, said, err) = copied(&[&db.addr(), db.database()], &no_env(), &dump);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(said.ends_with(&format!(" into {}\n", place(&db))), "{said}");
        assert_eq!(held(&db), everything());
    }
}

/// One board read out of each database is one document, byte for byte: what was copied into
/// SurrealDB, written out, copied into PostgreSQL from that and written out again.
#[test]
fn board_out_is_the_same_document_from_either_database() {
    for board in [everything(), many_words(), a_big_board()] {
        let surreal = Fixture::on(Backend::SurrealDb, Seed { dump: board });
        let env = board_env(&surreal.other_database());
        let (code, from_surreal, err) = copied(&["--board", "--out"], &env, b"");
        assert_eq!((code, err.as_str()), (0, ""));
        let postgres = Backend::PostgreSql.database();
        let env = board_env(&postgres);
        let (code, _, err) = copied(&["--board"], &env, from_surreal.as_bytes());
        assert_eq!((code, err.as_str()), (0, ""));
        let (code, from_postgres, err) = copied(&["--board", "--out"], &env, b"");
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(
            from_postgres == from_surreal,
            "the two documents differ ({} and {} bytes)",
            from_surreal.len(),
            from_postgres.len()
        );
    }
}

/// A PostgreSQL board that cannot be read is refused as PostgreSQL's, naming the board's
/// database, its socket and what says its state, in one line, and never as SurrealDB's.
#[test]
fn a_postgresql_board_that_does_not_answer_is_refused_by_name() {
    let nowhere = std::env::temp_dir().join(format!("no-pg-{}", std::process::id()));
    let addr = format!("postgresql:{}", nowhere.display());
    let env = cli::Env {
        vars: [("TASKS_SERVER".to_string(), addr.clone())].into(),
        cwd: std::env::temp_dir(),
    };
    for (args, env) in [
        (&["--board", "--out"][..], &env),
        (&["--board"], &env),
        (&[addr.as_str()], &no_env()),
    ] {
        let (code, out, err) = copied(args, env, b"{}");
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(
            err.starts_with(&format!(
                "❌ tasks-copy: PostgreSQL: the board's PostgreSQL is not answering on {}",
                nowhere.join(".s.PGSQL.5432").display()
            )),
            "{err}"
        );
        assert!(
            err.ends_with(&format!(
                " (the board's database tasks at {addr}: `just tasks database` says what serves \
                 it and whether it is held)\n"
            )),
            "{err}"
        );
        assert!(!err.contains("SurrealDB"), "{err}");
    }
}

/// A PostgreSQL server is named by its socket's folder, an absolute path: any other is refused
/// before anything is reached, saying which argument or variable named it.
#[test]
fn a_postgresql_address_that_is_no_absolute_folder_is_refused() {
    for socket in ["", "relative/folder", "//host/folder"] {
        let addr = format!("postgresql:{socket}");
        let (code, out, err) = copied(&[&addr], &no_env(), b"{}");
        assert_eq!((code, out.as_str()), (2, ""), "{addr}");
        assert_eq!(
            err,
            format!(
                "❌ tasks-copy: ADDR {addr:?} must name an absolute PostgreSQL socket folder as \
                 postgresql:/path\n"
            )
        );
        let env = cli::Env {
            vars: [("TASKS_SERVER".to_string(), addr.clone())].into(),
            cwd: std::env::temp_dir(),
        };
        let (code, _, err) = copied(&["--board", "--out"], &env, b"");
        assert_eq!(
            (code, err),
            (
                2,
                format!(
                    "❌ tasks-copy: TASKS_SERVER={addr:?} must name an absolute PostgreSQL socket \
                     folder as postgresql:/path\n"
                )
            )
        );
    }
}

#[test]
fn help_alone_says_every_form_and_variable_and_reaches_no_board() {
    for help in ["--help", "-h"] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = copy_main(
            &[help.to_string()],
            &no_env(),
            &mut &b""[..],
            &mut out,
            &mut err,
        );
        assert_eq!(
            (
                code,
                String::from_utf8(out).unwrap().as_str(),
                err.as_slice()
            ),
            (0, symphony_board::COPY_HELP, &b""[..])
        );
    }
    for word in [
        "--board",
        "--out",
        "TASKS_SERVER",
        "TASKS_DATABASE",
        "ADDR [DATABASE]",
    ] {
        assert!(symphony_board::COPY_HELP.contains(word), "{word}");
    }
    // Beside another argument it is no form of the copy's.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--board".to_string(), "--help".to_string()];
    assert_eq!(
        copy_main(&args, &no_env(), &mut &b""[..], &mut out, &mut err),
        2
    );
    assert!(
        String::from_utf8(err)
            .unwrap()
            .contains("usage: tasks-copy")
    );
}

#[test]
fn board_takes_no_address_and_out_needs_board() {
    let usage = "❌ tasks-copy: usage: tasks-copy ADDR [DATABASE] < dump.json, or tasks-copy --board [--out]\n";
    for args in [
        vec!["--out".to_string()],
        vec!["--board".to_string(), server_addr()],
        vec![server_addr(), "--board".to_string()],
        vec![
            "--board".to_string(),
            "--out".to_string(),
            "--out".to_string(),
        ],
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = copy_main(&args, &no_env(), &mut &b"{}"[..], &mut out, &mut err);
        assert_eq!(
            (code, String::from_utf8(err).unwrap().as_str()),
            (2, usage),
            "{args:?}"
        );
        assert!(out.is_empty());
    }
}
