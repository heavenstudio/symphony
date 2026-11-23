//! A client of its own of a board ([`Database::another`]) is of the same board, has made no
//! request, and waits for each answer as the one it came from does; a board's address names its
//! board ([`Database::named`]), and a SurrealDB server's is refused.

use super::*;
use std::path::Path;

/// The wait each answer of `db`'s client is given.
fn wait(db: &Database) -> Duration {
    match db {
        Database::PostgreSql(client) => client.wait(),
    }
}

#[test]
fn another_client_is_of_the_same_board_and_waits_as_the_one_it_came_from() {
    let patient = Duration::from_secs(1_800);
    let boards = [
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
}

#[test]
fn a_boards_address_names_its_postgresql_and_a_surrealdb_servers_is_refused() {
    let patient = Duration::from_secs(1_800);
    let named = |client: rpc::Client| Database::named(&client);
    let db = named(rpc::Client::new(
        "postgresql:/tmp/symphony-pg-1",
        "sf",
        "board",
    ))
    .unwrap();
    assert_eq!(
        (db.addr().as_str(), db.database()),
        ("postgresql:/tmp/symphony-pg-1", "board")
    );
    let waiting = rpc::Client::new("postgresql:/tmp/symphony-pg-1", "sf", "b").waiting(patient);
    assert_eq!(wait(&named(waiting).unwrap()), patient);
    let refused = named(rpc::Client::new("127.0.0.1:9", "sf", "board")).unwrap_err();
    assert!(
        refused.starts_with("127.0.0.1:9 names a SurrealDB server"),
        "{refused}"
    );
    assert!(refused.contains(crate::server::GONE), "{refused}");
}
