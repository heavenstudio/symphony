//! The relay of the slow board's tests reads a connection it holds to its end: a request sent again
//! on the held stream, as a retry on that same connection would be, is counted and reaches the
//! server, so neither what a test counts as sent nor what it finds done there misses it.

use crate::common::{Fixture, RawWrite};
use crate::relay::Relay;
use serde_json::json;
use std::io::Write;
use std::net::TcpStream;
use symphony_board::cbor;

const BUMP: &str = "UPSERT bump:count SET n += 1;";

/// `sql` as one whole RPC request in `db` to the server at `host`, written as a client writes it.
fn request(host: &str, db: &str, sql: &str) -> Vec<u8> {
    let body = cbor::encode(&json!({"id": 1, "method": "query", "params": [sql, {}]}));
    let mut request = format!(
        "POST /rpc HTTP/1.1\r\nHost: {host}\r\nAccept: application/cbor\r\nContent-Type: \
         application/cbor\r\nContent-Length: {}\r\nsurreal-ns: sf\r\nsurreal-db: {db}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend(body);
    request
}

/// The second write comes on the held connection after the first, which the relay has sent on
/// while it holds the answer; the caller then closes the connection, as a client closes one whose
/// answer did not come.
#[test]
fn a_write_sent_again_on_a_held_connection_is_counted_and_runs_on_the_server() {
    let f = Fixture::empty();
    let board = f.store.client();
    board
        .write("DEFINE TABLE bump SCHEMALESS;", json!({}))
        .unwrap();
    let relay = Relay::to(board.addr(), 1);
    let write = request(&relay.addr, board.database(), BUMP);
    let mut held = TcpStream::connect(&relay.addr).unwrap();
    held.write_all(&write).unwrap();
    held.write_all(&write).unwrap();
    drop(held);
    assert_eq!(
        relay.requests(),
        [(0, BUMP.to_owned()), (0, BUMP.to_owned())],
        "both writes, on the held connection"
    );
    let bumped = f.store.raw().row("bump", "count").field("n");
    assert_eq!(
        bumped.unwrap().as_u64(),
        Some(2),
        "each write ran on the server"
    );
}
