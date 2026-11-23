//! Inbox acknowledgements, unread-body payload size, acknowledged-history scaling, and the inbox
//! of a recipient further behind than a dispatch pass reads.

use super::texts;
use crate::common::{Backend, Fixture, HOST, NOW, Seed};
use symphony_board::rpc::Client;
use symphony_board::{Message, Store};

on_both! {
fn ack_empties_the_inbox_and_new_messages_arrive_again(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    f.store.say("sim/jump", "owner", "one", None).unwrap();
    let ids: Vec<i64> = f.store.inbox(&a).unwrap().iter().map(|m| m.id).collect();
    f.store.ack(&a, &ids).unwrap();
    f.store.ack(&a, &ids).unwrap(); // again: nothing new
    assert!(f.store.inbox(&a).unwrap().is_empty());
    f.store.say("sim/jump", "owner", "two", None).unwrap();
    assert_eq!(texts(f.store.inbox(&a).unwrap()), ["two"]);
    assert_eq!(f.store.raw().table("delivery").count().unwrap(), 1);

}
}

on_both! {
fn acknowledgements_keep_adjacent_large_integer_ids_and_recipients_distinct(backend: Backend) {
    use symphony_board::rows::{DeliveryRow, MessageRow};

    let ids = [
        1,
        9_007_199_254_740_992,
        9_007_199_254_740_993,
        i64::MAX - 2,
    ];
    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch")
        .task("sim/jump", "ready");
    seed.dump.messages = ids
        .iter()
        .map(|&id| MessageRow {
            id,
            slug: "sim/jump".into(),
            sender: "owner".into(),
            recipient: Some("amber-heron".into()),
            kind: "msg".into(),
            text: format!("message {id}"),
            ts: NOW,
            person: false,
        })
        .collect();
    seed.dump.deliveries = [
        (ids[0], "amber-heron"),
        (ids[1], "amber-heron"),
        (ids[2], "brisk-finch"),
    ]
    .into_iter()
    .map(|(message, handle)| DeliveryRow {
        message,
        handle: handle.into(),
        acked: NOW,
    })
    .collect();
    let f = Fixture::on(backend, seed);
    let unread = || {
        f.store
            .inbox("amber-heron")
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(unread(), ids[2..]);
    f.store.ack("amber-heron", &[ids[2]]).unwrap();
    assert_eq!(unread(), [ids[3]]);
    f.store.ack("amber-heron", &[ids[3]]).unwrap();
    assert!(unread().is_empty());

}
}

/// Forward one real RPC to the private fixture, counting its complete HTTP response. Nothing here
/// is late, so the proxy and the client wait for the server's answer as long as it takes, and no
/// short wait races it on a loaded machine; only the loopback connect is bounded, which the kernel
/// answers however busy the server is.
fn measured_inbox(f: &Fixture, handle: &str) -> (Vec<Message>, usize) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    let connect = Duration::from_secs(3);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream = f.store.client().addr().parse().unwrap();
    std::thread::scope(|scope| {
        let proxy = scope.spawn(move || -> std::io::Result<usize> {
            let (mut downstream, _) = listener.accept()?;
            let mut reader = BufReader::new(&downstream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line)?, 0, "RPC has a complete head");
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("RPC declares its body length");
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            drop(reader);
            head.truncate(head.len() - 2);
            head.push_str("Connection: close\r\n\r\n");
            let mut server = symphony_board::sys::connect(&upstream, connect)?;
            server.write_all(head.as_bytes())?;
            server.write_all(&body)?;
            let mut bytes = 0;
            let mut chunk = [0; 16 * 1024];
            loop {
                let read = server.read(&mut chunk)?;
                if read == 0 {
                    return Ok(bytes);
                }
                downstream.write_all(&chunk[..read])?;
                bytes += read;
            }
        });
        let client = Client::new(&addr.to_string(), "sf", f.store.database().database());
        let store = Store::unchecked(client, Box::new(|| NOW), Box::new(|_| false), HOST);
        let messages = store.inbox(handle);
        let bytes = proxy.join().unwrap().unwrap();
        (messages.unwrap(), bytes)
    })
}

// SurrealDB-only: measures HTTP response bytes through its protocol relay.
#[test]
fn acknowledged_and_unaddressed_history_does_not_travel_as_message_bodies() {
    use symphony_board::rows::{DeliveryRow, MessageRow};

    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch")
        .task("sim/jump", "ready");
    let body = "x".repeat(4096);
    seed.dump.messages = (1..=129)
        .map(|id| MessageRow {
            id,
            slug: "sim/jump".into(),
            sender: if id <= 65 { "owner" } else { "brisk-finch" }.into(),
            recipient: (id <= 65).then(|| "amber-heron".into()),
            kind: "msg".into(),
            text: body.clone(),
            ts: NOW,
            person: false,
        })
        .collect();
    seed.dump.deliveries = (1..=64)
        .map(|message| DeliveryRow {
            message,
            handle: "amber-heron".into(),
            acked: NOW,
        })
        .collect();
    let f = Fixture::new(seed);
    let (messages, unread_bytes) = measured_inbox(&f, "amber-heron");
    assert_eq!(messages.len(), 1);
    assert_eq!((messages[0].id, &messages[0].text), (65, &body));
    // One unread 4 KiB body fits; two acknowledged or unaddressed bodies cross this bound.
    assert!(unread_bytes < 8 * 1024, "{unread_bytes} reply bytes");
    f.store.ack("amber-heron", &[65]).unwrap();
    let (messages, empty_bytes) = measured_inbox(&f, "amber-heron");
    assert!(messages.is_empty());
    assert!(
        empty_bytes < 8 * 1024,
        "{empty_bytes} empty-inbox reply bytes"
    );
    println!("inbox HTTP bytes: one unread {unread_bytes}, all acknowledged {empty_bytes}");
}

/// INBOX's selection of the unread among 70k acknowledged keys fits the 3 s every store client
/// waits, in the CPU the server spends on it: read before and after each query from a server of the
/// test's own, which no other test's queries reach. The client itself waits for the answer up to
/// the backstop of that wait, however long a loaded machine keeps the server from the CPU.
// SurrealDB alone: measures its server CPU and plans a SurrealQL key-selection query.
#[test]
fn acknowledged_key_selection_fits_the_shared_store_wait() {
    use std::time::{Duration, Instant};
    const STORE_WAIT: Duration = Duration::from_secs(3);

    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("surreal-inbox-keys-{}.log", std::process::id()));
    let (addr, server) = crate::common::launch(&crate::common::surreal(), &log)
        .unwrap_or_else(|why| panic!("the test's own server: {why}; its log is {}", log.display()));
    let pid = server.server.expect("the server's shell said its pid");
    // This is INBOX's production fragment. Opaque integer references keep the CPU case data-only.
    let query = concat!(
        "LET $candidates = $candidate_entries; LET $acked = $ack_entries;\n",
        include_str!("../../../src/store/surreal/inbox_unread.surql"),
        "\nRETURN $wanted;"
    );
    let client = Client::new(&addr, "sf", "inbox_keys").waiting(test_time::backstop(STORE_WAIT));
    for count in [5_000, 10_000, 20_000, 70_000] {
        let last = 10_000_i64 + count - 1;
        let candidates: Vec<_> = (10_000..=last)
            .map(|id| serde_json::json!([id.to_string(), id]))
            .collect();
        let acknowledged: Vec<_> = (10_000..last)
            .map(|id| serde_json::json!([id.to_string(), null]))
            .collect();
        let vars =
            serde_json::json!({"candidate_entries": candidates, "ack_entries": acknowledged});
        let (before, began) = (test_time::cpu::of(pid).unwrap(), Instant::now());
        let results = client.query(query, vars).unwrap();
        let (spent, took) = (test_time::cpu::of(pid).unwrap() - before, began.elapsed());
        assert_eq!(results.last(), Some(&serde_json::json!([last])));
        println!(
            "{count} inbox keys, one unread reference: {spent:.3?} of the server's CPU, \
             {took:.3?} on the wall"
        );
        assert!(
            spent < STORE_WAIT,
            "{count} keys took {spent:?} of the server's CPU, past the store's {STORE_WAIT:?} wait"
        );
    }
    drop(server);
    let _ = std::fs::remove_file(&log);
}

on_both! {
/// A recipient further behind than one dispatch pass's pool reaches (a session long unobserved,
/// its cursor 2100 lines back) reads its inbox from the message table, from its cursor on, and
/// still hears what was said to it, and nothing of the lines it could never hear.
fn a_recipient_far_behind_the_board_reads_its_inbox_from_its_cursor(backend: Backend) {
    use crate::callbacks::Listener;
    use symphony_board::rows::MessageRow;
    let mut seed = Seed::default()
        .session("worker")
        .session("writer")
        .task("tools/far", "in_progress");
    seed.dump.messages = (1..=2100)
        .map(|id| MessageRow {
            id,
            slug: "tools/far".into(),
            sender: "writer".into(),
            recipient: None,
            kind: "msg".into(),
            text: format!("line {id}"),
            ts: NOW,
            person: false,
        })
        .collect();
    let f = Fixture::on(backend, seed);
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "worker",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    (f.store.raw().row("callback_state", "worker"))
        .set(serde_json::json!({"message_cursor": 1, "inbox_cursor": 1}))
        .unwrap();
    f.store
        .say("tools/far", "writer", "far news", Some("worker"))
        .unwrap();
    let heard = listener.drain(&f, "writer", "worker");
    assert!(
        heard.iter().any(|line| line.contains("far news")),
        "{heard:?}"
    );
    assert!(
        !heard.iter().any(|line| line.contains("line ")),
        "{heard:?}"
    );
}
}

on_both! {
/// The reader advances only after acknowledging every line it saw. Writers share the message
/// counter, so no committed smaller id can appear behind that cursor afterwards.
fn concurrent_writers_leave_no_gap_or_duplicate_in_a_readers_inbox(backend: Backend) {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Barrier};
    let f = Fixture::on(backend, Seed::default());
    let reader = f.session("worker");
    let writers: Vec<_> = (0..3).map(|_| f.session("worker")).collect();
    for slug in ["one", "two"] { f.task(slug); }
    let stores: Vec<_> = writers.iter().map(|_| f.other()).collect();
    let start = Arc::new(Barrier::new(4));
    let mut seen = Vec::new();
    let mut posted = Vec::new();
    std::thread::scope(|scope| {
        let joins: Vec<_> = stores.into_iter().zip(&writers).map(|(store, writer)| {
            let start = Arc::clone(&start);
            let reader = &reader;
            scope.spawn(move || {
                start.wait();
                (0..12).map(|n| store.say(if n % 2 == 0 {"one"} else {"two"}, writer,
                    &format!("{writer}:{n}"), Some(reader)).unwrap().id).collect::<Vec<_>>()
            })
        }).collect();
        start.wait();
        loop {
            let messages = f.store.inbox(&reader).unwrap();
            let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
            for id in &ids { assert!(seen.last().is_none_or(|last| last < id), "{seen:?} then {ids:?}"); seen.push(*id); }
            f.store.ack(&reader, &ids).unwrap();
            if joins.iter().all(|join| join.is_finished()) { break; }
            std::thread::yield_now();
        }
        for join in joins { posted.extend(join.join().unwrap()); }
    });
    let remaining = f.store.inbox(&reader).unwrap();
    let ids: Vec<_> = remaining.iter().map(|m| m.id).collect();
    for id in &ids { assert!(seen.last().is_none_or(|last| last < id)); seen.push(*id); }
    f.store.ack(&reader, &ids).unwrap();
    posted.sort_unstable();
    assert_eq!(seen, posted);
    assert_eq!(seen.len(), 36);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 36);
    assert!(f.store.inbox(&reader).unwrap().is_empty());
}
}
