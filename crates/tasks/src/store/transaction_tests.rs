//! The guard a write starts with, on a board of the store's contract that lacks an additive step
//! the store has ([`schema::Steps`]): a write that says it names a table of the step is refused
//! before any of its statements, its text naming the table in any quoting as a debug build holds
//! it to ([`Statement::disagreement`]); what it does not say, the database refuses itself (a field
//! or a table it does not define), as the transaction's; so nothing of it is written either way.
//! Once a build that may move the board has added the step, the same write passes.

use super::*;
use crate::live::Live;
use crate::store::statement::Statement;
use crate::store_fixture::{HOST, NOW, database};
use crate::{NewTask, Opening, Schema};
use serde_json::json;

/// This build's contract with a step it lacks: the table `fixture_tag`, and `task`'s optional
/// `fixture_note`.
fn ahead() -> Schema {
    Schema::this_build().then_additive(
        "CREATE TABLE IF NOT EXISTS fixture_tag (id jsonb PRIMARY KEY, doc jsonb NOT NULL);
         ALTER TABLE task ADD COLUMN IF NOT EXISTS fixture_note text;",
        &["fixture_tag"],
    )
}

/// A write that moves `sim/jump`'s priority to 4, then does `postgres`, in one transaction.
fn then(postgres: &str) -> Statement<'static> {
    Statement::from(Pg::script(format!(
        "UPDATE task SET priority = 4 WHERE slug = 'sim/jump';\n{postgres}"
    )))
}

/// Whether `table` is there, by a read of it, which a board without it refuses.
fn there(store: &Store, table: &str) -> bool {
    store.raw().table(table).get().is_ok()
}

#[test]
fn a_write_needing_a_step_the_board_lacks_is_refused_whole_until_the_board_takes_it() {
    let db = database();
    let open = |how: &Opening| {
        Store::opening_on(
            db.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            HOST,
            how,
        )
        .unwrap()
    };
    let behind = open(&Opening::write());
    behind
        .create(&NewTask::new("sim/jump", "Jump departure", "owner"))
        .unwrap();
    let was = behind.get("sim/jump").unwrap().priority;
    assert_ne!(was, 4);
    // A build ahead that may not move it: the project's live board, whose repository commits
    // no schema source, opened by no `just tasks migrate`.
    let common =
        std::env::temp_dir().join(format!("guard-{}-{}", std::process::id(), db.database()));
    std::fs::create_dir_all(&common).unwrap();
    let live = Live {
        project: "fixture".into(),
        common: common.clone(),
    };
    let store = open(&Opening::write().at(ahead()).live(Some(live)));
    assert_eq!(store.additions().unwrap(), crate::schema::ADDITIONS);
    let unchanged = |what: &str| {
        assert_eq!(behind.get("sim/jump").unwrap().priority, was, "{what}");
        assert!(!there(&behind, "fixture_tag"), "{what}");
        assert_eq!(
            behind.additions().unwrap(),
            crate::schema::ADDITIONS,
            "{what}"
        );
    };
    // Named, bare, quoted or nested: refused by the guard, before the write's first statement.
    let named = [
        then("INSERT INTO fixture_tag (id, doc) VALUES ('\"one\"', '{\"name\": \"one\"}');"),
        then("INSERT INTO \"fixture_tag\" (id, doc) VALUES ('\"one\"', '{\"name\": \"one\"}');"),
        then("PERFORM 1 FROM (SELECT count(*) FROM FIXTURE_TAG) AS n;"),
    ];
    for write in named.map(|write| write.naming(&["fixture_tag"])) {
        match store.transact::<Value>(write, json!({})) {
            Err(Failure::Stop(Error::Refused(why))) => assert!(
                why.contains(&format!(
                    "has {} of schema {}'s additive steps, and this write names `fixture_tag`, \
                     which step {} defines: nothing was written",
                    crate::schema::ADDITIONS,
                    crate::schema::VERSION,
                    crate::schema::ADDITIONS + 1
                )),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
        unchanged("a write naming the step's table");
    }
    // What no word shows the database refuses, the whole transaction with it: a field it does
    // not define, and a table it does not define, which no write makes.
    for (write, what) in [
        (
            then("UPDATE task SET fixture_note = 'x' WHERE slug = 'sim/jump';"),
            "a field the board does not define",
        ),
        (
            then("INSERT INTO fixture_other (id, doc) VALUES ('\"one\"', '{\"name\": \"one\"}');"),
            "a table the board does not define",
        ),
    ] {
        let refused = store.transact::<Value>(write, json!({}));
        assert!(refused.is_err(), "{what}");
        unchanged(what);
        assert!(!there(&behind, "fixture_other"), "{what}");
    }
    // Once a build that may move it adds the step, the write the guard refused passes.
    open(&Opening::write().at(ahead()).migrating());
    assert_eq!(store.additions().unwrap(), crate::schema::ADDITIONS + 1);
    store
        .transact::<Value>(
            then("INSERT INTO fixture_tag (id, doc) VALUES ('\"one\"', '{\"name\": \"one\"}');")
                .naming(&["fixture_tag"]),
            json!({}),
        )
        .unwrap();
    assert_eq!(behind.get("sim/jump").unwrap().priority, 4);
    assert!(there(&behind, "fixture_tag"));
    let _ = std::fs::remove_dir_all(common);
}
