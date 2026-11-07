//! A store opens a board at its build's version on one read of it, which defines and writes
//! nothing: over a slow board that read goes once more, as a page's read does, so a page's open
//! and read get their answers after one resend. A board not made yet is still defined, its
//! namespace once.

use crate::common::{Fixture, HOST, NOW, client, server_addr};
use crate::relay::Relay;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use symphony_board::Store;
use symphony_board::rpc::{Client, DbError, Kind};

fn open(client: Client) -> Store {
    Store::with(client, Box::new(|| NOW), Box::new(|_| true), HOST).expect("the store opens")
}

/// A namespace no other test makes.
fn fresh_namespace() -> String {
    static NAMESPACES: AtomicUsize = AtomicUsize::new(0);
    format!(
        "open_read_{}_{}",
        std::process::id(),
        NAMESPACES.fetch_add(1, Ordering::SeqCst)
    )
}

#[test]
fn a_page_read_under_a_slow_board_is_answered_after_one_read_resend_and_its_open_writes_nothing() {
    let f = Fixture::empty();
    let made = f.task("p/a");
    let (relay, db) = (
        Relay::to(f.store.client().addr(), 0),
        f.store.client().database(),
    );
    // The open's read and the page's, rehearsed on nobody's short wait: the server's answers to
    // them are what the read's one resend and the page's read are answered from, at once.
    open(relay.client("sf", db)).get("p/a").unwrap();
    relay.hold(1);
    let store = open(relay.client("sf", db));
    let opened = relay.requests();
    assert_eq!(opened.len(), 2, "one read and its one resend: {opened:?}");
    assert_eq!(opened[0].1, opened[1].1, "the same read went once more");
    assert_eq!(
        (opened[0].0, opened[1].0),
        (0, 1),
        "the resend had a new connection"
    );
    for (_, sql) in &opened {
        assert_eq!(Kind::of(sql), Kind::Read, "the open sent a write: {sql}");
    }

    assert_eq!(store.get("p/a").unwrap().slug, made.slug);
    let read = relay.requests();
    assert_eq!(read.len(), 3, "the page's read, once: {read:?}");
    assert_eq!(read[2].0, 1, "on the connection the resend opened");
}

/// Over a slow board only the held read reaches the server: the read's resend and the page's read
/// are answered from the rehearsal, so none races the short wait for the server's answer however
/// loaded the machine is. A second relay in front of the server lists what reached it.
#[test]
fn over_a_slow_board_only_the_held_read_reaches_the_server() {
    let f = Fixture::empty();
    f.task("p/a");
    let server = Relay::to(f.store.client().addr(), 0);
    let (relay, db) = (Relay::to(&server.addr, 0), f.store.client().database());
    open(relay.client("sf", db)).get("p/a").unwrap();
    let rehearsed = server.requests();
    relay.hold(1);
    open(relay.client("sf", db)).get("p/a").unwrap();
    assert_eq!(
        relay.requests().len(),
        3,
        "a read, its resend, the page's read"
    );
    let asked = server.requests().split_off(rehearsed.len());
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].1, rehearsed[0].1, "the open's read, held");
}

#[test]
fn a_board_not_made_yet_is_defined_its_namespace_once_and_then_opens_on_one_read() {
    let ns = fresh_namespace();
    let relay = Relay::to(&server_addr(), 0);
    open(relay.client(&ns, "t"));
    let made = relay.requests();
    assert_eq!(Kind::of(&made[0].1), Kind::Read, "it read first: {made:?}");
    let namespaces = made
        .iter()
        .filter(|(_, sql)| sql.contains("DEFINE NAMESPACE"))
        .count();
    assert_eq!(namespaces, 1, "{made:?}");

    let again = Relay::to(&server_addr(), 0);
    open(again.client(&ns, "t"));
    let reopened = again.requests();
    assert_eq!(reopened.len(), 1, "{reopened:?}");
    assert_eq!(Kind::of(&reopened[0].1), Kind::Read);
}

/// `db`'s namespace and database made, with no board in them; again when that meets another test's
/// definition at once in the shared namespace.
fn bare(db: Client) -> Client {
    let made = (0..100).find_map(|_| match db.define_then("RETURN NONE;", json!({})) {
        Err(DbError::Conflict) => {
            std::thread::sleep(Duration::from_millis(50));
            None
        }
        made => Some(made),
    });
    made.expect("the database kept meeting other writes")
        .unwrap();
    db
}

/// A database the server holds with no tables in it, and one not on the server in a namespace that
/// is: each is made a board, the namespace defined only where the database was not there.
#[test]
fn a_database_without_a_board_is_made_one_its_namespace_defined_only_when_missing() {
    Fixture::empty(); // the shared namespace is on the server
    for (db, defines) in [(bare(client()), false), (client(), true)] {
        let relay = Relay::to(db.addr(), 0);
        let store = open(relay.client("sf", db.database()));
        let sent = relay.requests();
        assert_eq!(Kind::of(&sent[0].1), Kind::Read, "{sent:?}");
        let defined = sent.iter().any(|(_, sql)| sql.contains("DEFINE NAMESPACE"));
        assert_eq!(defined, defines, "{sent:?}");
        assert!(store.tasks(None).unwrap().is_empty());

        let again = Relay::to(db.addr(), 0);
        open(again.client("sf", db.database()));
        assert_eq!(again.requests().len(), 1, "the board was made");
    }
}
