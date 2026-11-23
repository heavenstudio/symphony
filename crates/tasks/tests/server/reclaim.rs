//! `tasks-reclaim`'s one whole run in a repository: the board in its git common dir given back
//! its dead space by PostgreSQL's VACUUM FULL and ANALYZE, its rows kept and its server serving
//! on. Which board its arguments name, and the refusal of a board still on SurrealDB, are the unit
//! tests' beside `reclaim_main`; what the vacuum keeps is `postgres.rs`'s.

use crate::own_board::Repository;
use symphony_board::NewTask;
use symphony_board::cli::connect;
use symphony_board::server::{Home, postgres};

#[test]
fn tasks_reclaim_in_a_repository_vacuums_the_board_in_its_common_dir() {
    let repo = Repository::new("reclaim-repository");
    connect(&repo.env())
        .unwrap()
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let pg = Home::in_common(&repo.common())
        .telling(|_| {})
        .postgresql(&postgres::discover_programs(&repo.dir).unwrap());
    let serving = pg.current().unwrap().expect("the board's server runs");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::reclaim_main(&[], &repo.env(), &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "tasks-reclaim: PostgreSQL reclaimed the board's dead space with VACUUM FULL and \
         ANALYZE; its rows and schema were kept\n"
    );
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    assert_eq!(
        pg.current().unwrap().map(|serving| serving.pid),
        Some(serving.pid),
        "the same server serves on"
    );
    assert_eq!(
        connect(&repo.env()).unwrap().get("sim/jump").unwrap().title,
        "Jump departure"
    );
}
