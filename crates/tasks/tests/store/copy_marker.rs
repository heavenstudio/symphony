//! A copy of the whole board is a commit a reader can see: it moves the board's marker, which
//! another store reads too, and a refused copy, which commits nothing, moves none.

use crate::common::{Fixture, Seed};
use crate::copy::everything;

#[test]
fn a_copy_moves_the_board_marker_and_a_refused_copy_does_not() {
    let f = Fixture::new(Seed { dump: everything() });
    let was = f.store.marker().unwrap();
    f.store.copy(&everything()).unwrap();
    let now = f.store.marker().unwrap();
    assert!(now > was, "the marker stayed at {was}, read {now}");
    assert_eq!(f.other().marker().unwrap(), now, "both read the one marker");
    let mut bad = everything();
    bad.messages[0].kind = "shout".into();
    assert!(f.store.copy(&bad).is_err());
    assert_eq!(
        f.store.marker().unwrap(),
        now,
        "a refused copy commits nothing"
    );
}
