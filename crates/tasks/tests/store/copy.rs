//! The copy of a whole board: a dump in JSON, which `tasks-copy` puts in
//! SurrealDB in one transaction, and `Store::dump` reads it back in the same shape, each table in the
//! order of its key.

use crate::common::{Fixture, NOW, Seed, client, server_addr, session, task};
use symphony_board::rows::{
    BarredKeyRow, DeliveryRow, Dump, LinkRow, MessageRow, RoleRow, SessionRow, TaskRow,
    TaskSessionRow,
};
use symphony_board::sys::{self, Command};
use symphony_board::{Error, Store, cli, copy_main};

/// One row of every table, each optional column both set and not, in key order.
fn everything() -> Dump {
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
    }
}

/// An environment that names no board: the commands given an address read nothing from it.
fn no_env() -> cli::Env {
    cli::Env::default()
}

/// The environment of a run that reaches `db` on the tests' server, as `TASKS_SERVER` and
/// `TASKS_DATABASE` point a run at a board.
fn board_env(db: &symphony_board::rpc::Client) -> cli::Env {
    cli::Env {
        vars: [
            ("TASKS_SERVER".to_string(), server_addr()),
            ("TASKS_DATABASE".to_string(), db.database().to_string()),
        ]
        .into_iter()
        .collect(),
        cwd: std::env::temp_dir(),
    }
}

#[test]
fn a_copy_reads_back_as_it_was_written() {
    let f = Fixture::new(Seed { dump: everything() });
    assert_eq!(f.store.dump().unwrap(), everything());
}

#[test]
fn copying_again_replaces_the_board() {
    let f = Fixture::new(Seed { dump: everything() });
    let smaller = Seed::default()
        .session("coral-hare")
        .task("docs/x", "backlog")
        .dump;
    f.store.copy(&smaller).unwrap();
    assert_eq!(f.store.dump().unwrap(), smaller);
}

#[test]
fn a_dump_the_board_would_refuse_leaves_the_board_as_it_was() {
    let f = Fixture::new(Seed { dump: everything() });
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

#[test]
fn a_board_opened_again_is_kept() {
    let f = Fixture::new(Seed { dump: everything() });
    let again = Store::open(f.other_client()).unwrap();
    assert_eq!(again.dump().unwrap(), everything());
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
            "tasks-copy: 2 tasks, 2 links, 2 sessions, 2 roles, 2 messages, 1 deliveries, 1 barred keys, 1 task sessions into sf/{} at {}\n",
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
    assert!(said.starts_with("❌ tasks-copy: SurrealDB: "), "{said}");
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
        Command::new(env!("CARGO_BIN_EXE_tasks-copy"))
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
    let refused = sys::output(&mut Command::new(env!("CARGO_BIN_EXE_tasks-copy"))).unwrap();
    assert_eq!(refused.status.code(), Some(2));
}

#[test]
fn text_that_reads_as_another_type_stays_text() {
    let mut seed = Seed::default().session("amber-heron");
    let lookalikes = [
        "approve: looks right",
        "a:b",
        "2024-01-01T00:00:00Z",
        "00000000-0000-0000-0000-000000000000",
        "1s",
        "NONE",
        "<string> 1",
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
    let f = Fixture::new(seed.clone());
    assert_eq!(f.store.dump().unwrap(), seed.dump);
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

const BOARD_TABLES: [&str; 18] = [
    "barred_key",
    "callback_event",
    "callback_lease",
    "callback_state",
    "chat_event",
    "counter",
    "delivery",
    "link",
    "message",
    "meta",
    "moved",
    "role",
    "session",
    "task",
    "task_chunk",
    "task_embedding_checkpoint",
    "task_embedding_source",
    "task_session",
];

#[test]
fn copy_replaces_derived_vectors_only_when_the_board_swap_commits() {
    let f = Fixture::new(Seed { dump: everything() });
    f.store.client().query(
        "CREATE task_chunk SET slug = 'sim/jump', source = 'brief', source_hash = 'old', text_hash = 'old', model = 'fake', text = 'cached board text', embedding = $vector; CREATE task_embedding_source SET slug = 'sim/jump', source = 'brief', source_hash = 'old', model = 'fake', parts = 1; CREATE task_embedding_checkpoint:board SET snapshot = {model: 'fake'};",
        serde_json::json!({"vector":vec![1.0_f32; symphony_board::embedding::DIMENSIONS]}),
    ).unwrap();
    let counts = || {
        f.store.client().query(
        "RETURN count(SELECT id FROM task_chunk); RETURN count(SELECT id FROM task_embedding_source); RETURN count(SELECT id FROM task_embedding_checkpoint);",
        serde_json::json!({}),
    ).unwrap()
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

/// The tables whose writes advance `counter:board` (`board_changed`).
const MARKED_TABLES: [&str; 9] = [
    "task",
    "session",
    "role",
    "link",
    "message",
    "delivery",
    "task_session",
    "callback_state",
    "chat_event",
];

fn marker(f: &Fixture) -> serde_json::Value {
    f.store
        .client()
        .query("RETURN counter:board.last;", serde_json::json!({}))
        .unwrap()
        .remove(0)
}

#[test]
fn a_copy_advances_the_board_marker_once_a_row_from_a_board_without_one() {
    // 2 tasks, links, sessions, roles and messages, a delivery and a task session; barred keys none.
    let f = Fixture::new(Seed { dump: everything() });
    assert_eq!(marker(&f), serde_json::json!(12));
    f.store.copy(&everything()).unwrap();
    assert_eq!(marker(&f), serde_json::json!(36), "12 removed, 12 written");
}

/// Schema 10's events UPSERT the marker: on a record that exists that tries a create and rolls it
/// back, and SurrealKV's rollback walks every key the transaction wrote, so the copy's one
/// transaction cost time quadratic in its rows. This schema's open redefines them, moving no row.
#[test]
fn a_board_of_schema_ten_has_its_marker_events_redefined_and_its_rows_kept() {
    let f = Fixture::new(Seed { dump: everything() });
    let events = || -> Vec<String> {
        MARKED_TABLES
            .iter()
            .map(|table| {
                let sql = format!("RETURN (INFO FOR TABLE {table}).events.board_changed;");
                let found = f.store.client().query(&sql, serde_json::json!({}));
                serde_json::from_value(found.unwrap().remove(0)).unwrap()
            })
            .collect()
    };
    let old: String = MARKED_TABLES
        .iter()
        .map(|table| format!("DEFINE EVENT OVERWRITE board_changed ON {table} WHEN $before != $after THEN {{ UPSERT counter:board SET last = (last ?? 0) + 1; }};"))
        .collect();
    f.store
        .client()
        .query(
            &format!(
                "BEGIN TRANSACTION; {old} UPSERT meta:schema SET version = 10; COMMIT TRANSACTION;"
            ),
            serde_json::json!({}),
        )
        .unwrap();
    assert!(events().iter().all(|e| e.contains("UPSERT counter:board")));
    let before = marker(&f);
    let reopened = f.other();
    let redefined = events();
    assert!(
        redefined
            .iter()
            .all(|e| e.contains("UPDATE counter:board") && !e.contains("UPSERT")),
        "{redefined:#?}"
    );
    assert_eq!(reopened.dump().unwrap(), everything());
    assert_eq!(marker(&f), before, "redefining moves no row");
    reopened.copy(&everything()).unwrap();
    assert_eq!(marker(&f), serde_json::json!(36));
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

#[test]
fn a_big_dump_refused_late_leaves_the_board_and_nothing_else() {
    let f = Fixture::new(Seed { dump: everything() });
    let mut bad = a_big_board();
    bad.messages.last_mut().unwrap().kind = "shout".into();
    assert!(matches!(f.store.copy(&bad), Err(Error::Db(_))));
    assert_eq!(f.store.dump().unwrap(), everything());
    assert_eq!(tables(&f), BOARD_TABLES);
}

#[test]
fn board_copies_into_the_board_the_environment_reaches() {
    let db = client();
    let dump = serde_json::to_vec(&everything()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--board".to_string()];
    let code = copy_main(
        &args,
        &board_env(&db),
        &mut dump.as_slice(),
        &mut out,
        &mut err,
    );
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    let said = String::from_utf8(out).unwrap();
    assert!(
        said.starts_with("tasks-copy: 2 tasks, ")
            && said.ends_with(&format!(
                " into sf/{} at {}\n",
                db.database(),
                server_addr()
            )),
        "{said}"
    );
    assert_eq!(Store::open(db).unwrap().dump().unwrap(), everything());
}

#[test]
fn board_out_writes_the_boards_dump_as_the_copy_reads_it_on_one_line() {
    let f = Fixture::new(Seed { dump: everything() });
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--board".to_string(), "--out".to_string()];
    let code = copy_main(
        &args,
        &board_env(&f.other_client()),
        &mut &b""[..],
        &mut out,
        &mut err,
    );
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert_eq!(Dump::from_json(&text).unwrap(), everything());
    assert_eq!(
        f.store.dump().unwrap(),
        everything(),
        "reading it writes nothing"
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
