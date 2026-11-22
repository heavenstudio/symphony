//! A client of its own of a board ([`Database::another`]) is of the same board, has made no
//! request, and waits for each answer as the one it came from does, on either database.

use super::*;
use std::path::Path;

/// The wait each answer of `db`'s client is given.
fn wait(db: &Database) -> Duration {
    match db {
        Database::SurrealDb(client) => client.wait_limit(),
        Database::PostgreSql(client) => client.wait(),
    }
}

#[test]
fn another_client_is_of_the_same_board_and_waits_as_the_one_it_came_from() {
    let patient = Duration::from_secs(1_800);
    let boards = [
        Database::from(rpc::Client::new("127.0.0.1:9", "sf", "board")),
        Database::from(rpc::Client::new("127.0.0.1:9", "sf", "board").waiting(patient)),
        Database::from(pg::Client::new(Path::new("/tmp/symphony-pg-1"), "board")),
        Database::from(pg::Client::new(Path::new("/tmp/symphony-pg-1"), "board").waiting(patient)),
    ];
    for db in &boards {
        let another = db.another();
        assert_eq!(
            (another.addr(), another.database(), another.is_postgresql()),
            (db.addr(), db.database(), db.is_postgresql())
        );
        assert_eq!(wait(&another), wait(db), "{}", db.addr());
        assert_eq!(another.requests(), 0);
    }
    assert_eq!(wait(&boards[1].another()), patient);
    assert_eq!(wait(&boards[3].another()), patient);
}
