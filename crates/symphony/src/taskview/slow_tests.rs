//! A task page over a slow board: a board late twice is said to be slow, never unreachable; a write
//! that got no answer runs once, a defined function's call however it is spelled.

use serde_json::json;
use symphony_board::rpc::{Client, slow};

use super::relay::{Relay, WAIT};
use super::store_fixture::{Fixture, NOW, RawWrite, Seed};
use super::{brief_row, page};
use crate::taskview::read_failure;
use crate::test_tree::Tree;

const UNREACHABLE: &str = "the task board cannot be reached: ";

/// A client of `fixture`'s board over `relay` ([`Relay::client`]).
fn client(relay: &Relay, fixture: &Fixture) -> Client {
    relay.client("sf", fixture.store.client().database())
}

/// A store over `relay`, sending nothing until asked.
fn store(relay: &Relay, fixture: &Fixture) -> symphony_board::Store {
    store_over(client(relay, fixture))
}

fn store_over(client: Client) -> symphony_board::Store {
    symphony_board::Store::unchecked(client, Box::new(|| NOW), Box::new(|_| true), "this-host")
}

fn family() -> Fixture {
    Fixture::new(
        Seed::default()
            .task_row(brief_row(1, "fixture/parent", "A parent.", None))
            .task_row(brief_row(
                2,
                "fixture/child",
                "A step.",
                Some("fixture/parent"),
            )),
    )
}

/// Late twice is slow, 504; a board that refuses the connection, beside it, cannot be reached, 503.
#[test]
fn a_page_whose_read_is_answered_late_twice_says_the_board_is_slow_not_unreachable() {
    let fixture = family();
    let record = fixture.store.get("fixture/parent").unwrap();
    let stall = Relay::to(fixture.store.client().addr(), usize::MAX);
    let failed = page(&store(&stall, &fixture), &record, &Tree::new().root).unwrap_err();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(
        said.starts_with("the task board is slow: the board's database: RPC query at "),
        "{said}"
    );
    assert!(
        said.contains(", configured wait 300.000ms: the server at "),
        "{said}"
    );
    assert!(said.contains("it was not sent a third time"), "{said}");
    assert!(!said.contains("cannot be reached"), "{said}");
    let sent = stall.requests();
    assert_eq!(
        sent.iter()
            .map(|(connection, _)| *connection)
            .collect::<Vec<_>>(),
        [0, 1],
        "one sending and one more, no third: {sent:?}"
    );

    let gone = {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        closed.local_addr().unwrap().to_string()
    };
    let refused =
        store_over(Client::new(&gone, "sf", fixture.store.client().database()).waiting(WAIT));
    let failed = refused.get("fixture/parent").unwrap_err();
    let (status, said) = read_failure(&failed.to_string(), UNREACHABLE);
    assert_eq!(status, 503, "{said}");
    assert!(
        said.starts_with("the task board cannot be reached: the board's database: the server at "),
        "{said}"
    );
    assert!(said.ends_with("the request was not sent"), "{said}");
}

/// The relay's list of what it was sent waits for the server's answer to the held write, so the
/// thread is read once the write has run there, with no clock.
#[test]
fn a_write_whose_answer_is_late_runs_once_and_says_the_board_is_slow() {
    let fixture = family();
    let stall = Relay::to(fixture.store.client().addr(), usize::MAX);
    let failed = store(&stall, &fixture)
        .say("fixture/parent", "owner", "said once", None)
        .unwrap_err()
        .to_string();
    let (status, said) = read_failure(&failed, UNREACHABLE);
    assert_eq!(status, 504, "{said}");
    assert!(said.contains("so it was not sent again"), "{said}");
    assert_eq!(stall.requests().len(), 1, "the write was sent again");
    let thread = fixture.store.thread("fixture/parent").unwrap();
    let said = thread
        .iter()
        .filter(|message| message.text == "said once")
        .count();
    assert_eq!(said, 1, "the write ran once");
}

/// A `RETURN` of a defined function's call, spelled with a space, a line or a comment against its
/// `::`, runs the function on the server just as `fn::bump()` does: each, answered late, is sent
/// once, and the record it counts on goes up by one each time.
#[test]
fn a_late_call_of_a_defined_function_runs_once_however_it_is_spelled() {
    let fixture = family();
    let board = fixture.store.client();
    board
        .write(
            "DEFINE TABLE bump SCHEMALESS; \
             DEFINE FUNCTION fn::bump() { UPSERT bump:count SET n += 1; RETURN 1; };",
            json!({}),
        )
        .unwrap();
    let bumps = || {
        let counted = board.query(
            "RETURN (SELECT VALUE n FROM ONLY bump:count) ?? 0;",
            json!({}),
        );
        usize::try_from(counted.unwrap()[0].as_u64().unwrap()).unwrap()
    };
    let stall = Relay::to(board.addr(), usize::MAX);
    let spellings = [
        "RETURN fn::bump();",
        "RETURN fn ::bump();",
        "RETURN fn\n::bump();",
        "RETURN fn/**/::bump();",
        "RETURN fn -- a comment\n::bump();",
    ];
    let mut sent = 0;
    for sql in spellings {
        sent += 1;
        let failed = client(&stall, &fixture)
            .query(sql, json!({}))
            .unwrap_err()
            .to_string();
        assert!(
            slow(&failed) && failed.ends_with("so it was not sent again"),
            "{sql:?}: {failed}"
        );
        // Once the server has answered each call it was sent: so each has run there.
        assert_eq!(stall.requests().len(), sent, "{sql:?} was sent again");
        assert_eq!(bumps(), sent, "{sql:?} ran other than once");
    }
}
