//! Marker observation and start eligibility remain independent evidence.

use super::{Location, Whose, works_in};
use std::io::{self, ErrorKind};
use std::path::Path;

fn error(kind: ErrorKind) -> io::Result<()> {
    Err(io::Error::from(kind))
}

#[test]
fn only_known_absence_allows_outer_attribution() {
    assert_eq!(Location::boundaries([]), Location::Inside);
    assert_eq!(
        Location::boundaries([error(ErrorKind::NotFound), error(ErrorKind::NotFound)]),
        Location::Inside
    );
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::NotADirectory,
        ErrorKind::Other,
    ] {
        assert_eq!(Location::boundaries([error(kind)]), Location::Unknown);
        assert_eq!(
            Location::boundaries([error(kind), error(ErrorKind::NotFound)]),
            Location::Unknown
        );
        assert_eq!(
            Location::boundaries([error(kind), Ok(())]),
            Location::Outside
        );
        assert_eq!(
            Location::boundaries([Ok(()), error(kind)]),
            Location::Outside
        );
    }
    assert_eq!(Location::boundaries([Ok(())]), Location::Outside);
}

#[test]
fn boundary_uncertainty_matters_only_for_a_possible_start() {
    for location in [Location::Inside, Location::Outside, Location::Unknown] {
        assert_eq!(location.with_start(Whose::Not), Whose::Not);
    }
    for whose in [Whose::Its, Whose::Maybe, Whose::Not] {
        assert_eq!(Location::Outside.with_start(whose), Whose::Not);
    }
    assert_eq!(Location::Inside.with_start(Whose::Its), Whose::Its);
    assert_eq!(Location::Inside.with_start(Whose::Maybe), Whose::Maybe);
    assert_eq!(Location::Unknown.with_start(Whose::Its), Whose::Maybe);
    assert_eq!(Location::Unknown.with_start(Whose::Maybe), Whose::Maybe);
}

#[test]
fn foreign_paths_do_not_need_marker_evidence() {
    let tree = Path::new("/boundary/tree");
    for cwd in [
        Path::new("/boundary/tree-other"),
        Path::new("/boundary"),
        Path::new("/"),
    ] {
        assert_eq!(works_in(cwd, tree), Location::Outside);
    }
    assert_eq!(works_in(tree, tree), Location::Inside);
}
