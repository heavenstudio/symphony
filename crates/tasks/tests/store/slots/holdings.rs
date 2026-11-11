//! The canonical read-only holdings observation, through the real store and admission boundary.

use super::{F, K, O, P, W, capped, sessions, with};
use crate::common::{Fixture, HOST, NOW, RawWrite, refused, task};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::time::Duration;
use symphony_board::rows::TaskRow;
use symphony_board::{Error, Health, NewLaunch, NewTask, Store, rpc};

fn holdings(rows: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    rows.iter()
        .map(|(owner, keys)| {
            (
                owner.to_string(),
                keys.iter().map(|key| key.to_string()).collect(),
            )
        })
        .collect()
}

#[test]
fn one_owned_observation_groups_nested_and_branch_mirror_keys_without_writing_state() {
    let seed = with(sessions(), "feature", "ready", None, Some(O));
    let seed = with(seed, "part", "in_progress", Some("feature"), Some(P));
    let seed = seed.task_row(TaskRow {
        id: 3,
        parent: Some("part".into()),
        instruction: true,
        ..task("deep", "review")
    });
    let seed = with(seed, "review/alias", "review", None, Some(P));
    let seed = with(seed, "review-2/alias", "review", None, Some(P));
    let seed = with(seed, "second", "in_progress", None, Some(P));
    let f = Fixture::new(seed);
    f.store
        .record_launch(&NewLaunch {
            branch: "alias".into(),
            ..NewLaunch::new("feature", K, "executor")
        })
        .unwrap();
    let original = f.store.dump().unwrap();
    let counters = || {
        f.store
            .client()
            .query("SELECT * FROM counter ORDER BY id;", json!({}))
            .unwrap()
    };
    let original_counters = counters();
    let before = f.store.client().requests();
    let observed = f.store.slot_holdings().unwrap();
    assert_eq!(f.store.client().requests() - before, 1);
    let expected = holdings(&[(O, &["feature"]), (P, &["second"])]);
    assert_eq!(observed, expected);
    assert_eq!(f.store.dump().unwrap(), original);
    assert_eq!(
        counters(),
        original_counters,
        "no accounting or hold counter write"
    );

    f.other()
        .landed("second", P, "feature landed", false, &["in_progress"])
        .unwrap();
    assert_eq!(
        observed, expected,
        "the consumer retains one owned observation"
    );
    assert_eq!(
        f.store.slot_holdings().unwrap(),
        holdings(&[(O, &["feature"])])
    );
}

#[test]
fn only_a_blocked_hold_with_the_current_nonmissing_stamp_counts_as_admission_does() {
    let mut seed = sessions();
    for (slug, status) in [
        ("valid", "review"),
        ("stale", "review"),
        ("missing", "blocked"),
        ("unstamped", "blocked"),
        ("next", "ready"),
    ] {
        seed = with(seed, slug, status, None, Some(O));
    }
    let f = Fixture::new(seed);
    f.store.hold_blocked("valid", O).unwrap();
    f.store.hold_blocked("stale", O).unwrap();
    f.store
        .client()
        .write(
            "UPDATE task:stale SET updated += 1;
         UPSERT type::record('counter', ['hold', 'unstamped']) SET last = 0 RETURN NONE;",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        f.store.slot_holdings().unwrap(),
        holdings(&[(O, &["valid"])])
    );
    let said = refused(capped(&f, 1).claim("next", K));
    assert!(said.contains("holds 1 open tasks"), "{said}");
    assert!(said.contains("valid (blocked, held)"), "{said}");
    for excluded in ["stale (", "missing (", "unstamped ("] {
        assert!(!said.contains(excluded), "{said}");
    }
    f.other()
        .landed("valid", O, "feature landed", false, &["blocked"])
        .unwrap();
    assert!(f.store.slot_holdings().unwrap().is_empty());
    capped(&f, 1).claim("next", K).unwrap();
}

#[test]
fn finished_tasks_and_open_red_repairs_are_excluded_without_exempting_parent_work() {
    let mut seed = sessions();
    for (slug, status) in [
        ("feature", "in_progress"),
        ("finished", "done"),
        ("cancelled", "cancelled"),
    ] {
        seed = with(seed, slug, status, None, Some(O));
    }
    let f = Fixture::new(seed);
    for (slug, parent) in [
        ("tests/main-red-root", None),
        ("tests/main-red-child", Some("feature")),
    ] {
        f.store
            .create(&NewTask {
                red: Some("main".into()),
                parent: parent.map(String::from),
                ..NewTask::new(slug, "Red", F)
            })
            .unwrap();
        f.store.take_role(slug, O, "orchestrator").unwrap();
        f.store.claim(slug, W).unwrap();
        let repair = format!("{slug}/repair");
        f.store
            .create(&NewTask {
                parent: Some(slug.into()),
                ..NewTask::new(&repair, "Repair", K)
            })
            .unwrap();
        f.store.claim(&repair, W).unwrap();
        f.store.hold_blocked(&repair, W).unwrap();
    }
    assert_eq!(
        f.store.slot_holdings().unwrap(),
        holdings(&[(O, &["feature"])])
    );
    f.store
        .landed(
            "tests/main-red-root",
            O,
            "red repair landed",
            false,
            &["in_progress"],
        )
        .unwrap();
    assert_eq!(
        f.store.slot_holdings().unwrap(),
        holdings(&[(O, &["feature", "tests/main-red-root"])])
    );
}

#[test]
fn unknown_owners_retain_valid_held_slots_exactly_as_the_admission_view_does() {
    let seed = with(sessions(), "feature", "review", None, Some(O));
    let f = Fixture::new(with(seed, "next", "ready", None, Some(O)));
    f.store.hold_blocked("feature", O).unwrap();
    f.store.client().write(
        "UPSERT type::record('callback_state', $handle) SET handle = $handle,
             address = 'codex:abcdef01', host = $host, confirmed_until = $until, lease = $lease RETURN NONE;
         UPDATE type::record('session', $handle) SET last_seen = $seen RETURN NONE;",
        json!({"handle": O, "host": HOST, "until": NOW - 1.0,
            "lease": NOW + 1000.0, "seen": NOW - 1000.0}),
    ).unwrap();
    assert_eq!(f.health(O), Health::Unknown);
    assert_eq!(
        f.store.slot_holdings().unwrap(),
        holdings(&[(O, &["feature"])])
    );
    let said = refused(capped(&f, 1).claim("next", K));
    assert!(said.contains("holds 1 open tasks"), "{said}");
    assert!(said.contains("feature (blocked, held)"), "{said}");
    f.store.client().write(
        "UPDATE type::record('callback_state', $handle) SET confirmed_until = $until RETURN NONE;",
        json!({"handle": O, "until": NOW - 601.0}),
    ).unwrap();
    assert_eq!(f.health(O), Health::Left);
    assert!(f.store.slot_holdings().unwrap().is_empty());
}

#[test]
fn an_empty_observation_succeeds_but_a_failed_store_read_remains_an_error() {
    let f = Fixture::empty();
    assert_eq!(f.store.slot_holdings().unwrap(), BTreeMap::new());
    // Own the port, without a server answering it: no unrelated service can answer this read.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let store = Store::unchecked(
        rpc::Client::new(&address, "sf", "holdings").waiting(Duration::from_millis(30)),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
    );
    let error = store.slot_holdings().unwrap_err();
    assert!(matches!(error, Error::Db(_)), "{error}");
    assert!(error.to_string().contains(&address), "{error}");
}
