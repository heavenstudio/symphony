//! Which process-table lines are a board's server ([`board_served`]): a board's PostgreSQL on its
//! own data in a git common dir, and nothing else, a SurrealDB server an earlier release started
//! on the board's old store among them.

use super::*;

const POSTGRES: &str = "/opt/homebrew/opt/postgresql@18/bin/postgres";

#[test]
fn a_board_server_names_the_common_dir_of_the_data_it_serves_and_nothing_else_is_one() {
    let root = std::env::temp_dir().join(format!("board-served-{}", std::process::id()));
    let common = root.join("My Work/.git");
    fs::create_dir_all(common.join(FOLDER)).unwrap();
    fs::write(common.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let data = common.join(FOLDER).join(postgres::DATA);
    let line = format!("{POSTGRES} -D {}", data.display());
    assert_eq!(board_served(&line), Some(common.clone()), "{line}");
    // Other data, data in no git common dir, a folder of the board's last name elsewhere in the
    // common dir, another program, and the SurrealDB server an earlier release ran on the board's
    // old store.
    let loose = root.join("loose").join(FOLDER);
    fs::create_dir_all(&loose).unwrap();
    let elsewhere = common.join("elsewhere/tasks");
    fs::create_dir_all(&elsewhere).unwrap();
    for line in [
        format!("{POSTGRES} -D {}", loose.join(postgres::DATA).display()),
        format!("{POSTGRES} -D {}", elsewhere.join(postgres::DATA).display()),
        format!("{POSTGRES} -D {}", common.join(FOLDER).join(DATA).display()),
        format!("{POSTGRES} -D {}-old", data.display()),
        format!("/opt/bin/not-postgres -D {}", data.display()),
        format!(
            "/opt/bin/surreal start --bind 127.0.0.1:8765 surrealkv://{}",
            common.join(FOLDER).join(DATA).display()
        ),
        format!("{POSTGRES} -D"),
        data.display().to_string(),
        String::new(),
    ] {
        assert_eq!(board_served(&line), None, "{line}");
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_last_argument_is_a_whole_word() {
    let data = "/repo/.git/symphony/tasks/postgresql";
    assert!(last_argument_is(&format!("{POSTGRES} -D {data}"), data));
    assert!(!last_argument_is(&format!("{POSTGRES} -D x{data}"), data));
    assert!(!last_argument_is(
        &format!("{POSTGRES} -D {data} -c x"),
        data
    ));
    assert!(
        !last_argument_is(data, data),
        "a program is not its own argument"
    );
}

#[test]
fn a_refusal_of_a_board_on_surrealdb_names_its_home_how_it_moves_and_what_was_not_done() {
    let home = Home::at(Path::new("/repo/.git/symphony/tasks"));
    assert_eq!(
        home.on_surrealdb("Nothing was started").to_string(),
        format!(
            "the board in /repo/.git/symphony/tasks is on SurrealDB, which this build serves no \
             more: {GONE}. Nothing was started"
        )
    );
}

/// The constructor a consumer calls with an earlier release's SurrealDB binary gives the board
/// `Home::at` gives, the binary read by nothing: not even looked for.
#[test]
fn a_home_named_with_a_surrealdb_binary_is_the_board_in_its_folder_alone() {
    let dir = Path::new("/repo/.git/symphony/tasks");
    let named = Home::new(dir, Path::new("/nowhere/surreal"));
    let at = Home::at(dir);
    assert_eq!(
        (named.dir(), &named.within, named.patience, named.lock_wait),
        (at.dir(), &at.within, at.patience, at.lock_wait)
    );
    assert_eq!(named.dir(), dir);
}
