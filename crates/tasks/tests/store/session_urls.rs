//! A session's web address: set and cleared by the session itself, kept beside its row in
//! `callback_state`, gone when it leaves, and never mistaken for a callback.

use crate::common::{Fixture, Seed, refused};
use serde_json::json;
use std::collections::BTreeMap;

const WEB: &str = "https://claude.ai/code/session_01abc";

fn urls(f: &Fixture) -> BTreeMap<String, String> {
    f.store.session_urls().unwrap()
}

fn structure(f: &Fixture) -> u64 {
    let last = (f.store.raw().row("counter", "board_structure"))
        .field("last")
        .unwrap();
    if last.is_null() {
        return 0;
    }
    serde_json::from_value(last).unwrap()
}

#[test]
fn a_session_sets_replaces_and_clears_its_own_address() {
    let f = Fixture::new(Seed::default().session("amber-heron").session("swift-hare"));
    assert!(urls(&f).is_empty());
    assert_eq!(
        f.store.set_session_url("amber-heron", &format!("  {WEB} ")),
        Ok(Some(WEB.to_string()))
    );
    assert_eq!(
        urls(&f),
        BTreeMap::from([("amber-heron".into(), WEB.into())])
    );
    f.store
        .set_session_url("amber-heron", "https://claude.ai/code/session_02")
        .unwrap();
    f.store
        .set_session_url("swift-hare", "http://mac.tail.ts.net/x")
        .unwrap();
    assert_eq!(
        urls(&f),
        BTreeMap::from([
            (
                "amber-heron".into(),
                "https://claude.ai/code/session_02".into()
            ),
            ("swift-hare".into(), "http://mac.tail.ts.net/x".into()),
        ])
    );
    assert_eq!(f.store.set_session_url("amber-heron", ""), Ok(None));
    assert_eq!(
        urls(&f),
        BTreeMap::from([("swift-hare".into(), "http://mac.tail.ts.net/x".into())])
    );
    // Clearing one that has none is no refusal.
    assert_eq!(f.store.set_session_url("amber-heron", " "), Ok(None));
}

#[test]
fn an_address_the_check_refuses_an_unknown_or_a_left_session_writes_nothing() {
    let f = Fixture::new(Seed::default().session("amber-heron").session("swift-hare"));
    f.store.set_session_url("amber-heron", WEB).unwrap();
    assert!(
        refused(
            f.store
                .set_session_url("amber-heron", "javascript:alert(1)")
        )
        .starts_with("a session URL starts with https:// or http://")
    );
    assert_eq!(
        refused(f.store.set_session_url("ghost-owl", WEB)),
        f.no_session("ghost-owl")
    );
    f.store.leave("swift-hare").unwrap();
    assert_eq!(
        refused(f.store.set_session_url("swift-hare", WEB)),
        "session swift-hare has left; attach again"
    );
    assert_eq!(
        urls(&f),
        BTreeMap::from([("amber-heron".into(), WEB.into())])
    );
}

/// Write `url` as `handle`'s address straight into its row, as a copied dump, a hand write or
/// another build could, past the check a set makes.
fn written(f: &Fixture, handle: &str, url: serde_json::Value) {
    (f.store.raw())
        .row("callback_state", json!(["session_url", handle]))
        .put(json!({"url": url}))
        .unwrap();
}

#[test]
fn a_row_written_past_the_check_is_read_only_when_the_check_keeps_it() {
    let handles = [
        "amber-heron",
        "swift-hare",
        "calm-finch",
        "slate-owl",
        "coral-lynx",
        "solar-vole",
        "red-kite",
    ];
    let mut seed = Seed::default();
    for handle in handles {
        seed = seed.session(handle);
    }
    let f = Fixture::new(seed);
    f.store.set_session_url("amber-heron", WEB).unwrap();
    for (handle, url) in [
        ("swift-hare", json!("javascript:alert(document.cookie)")),
        ("calm-finch", json!("data:text/html,<script>x</script>")),
        ("slate-owl", json!("https://a b")),
        (
            "coral-lynx",
            json!(format!(
                "https://h/{}",
                "a".repeat(symphony_board::session_url::MAX_BYTES)
            )),
        ),
        ("solar-vole", json!(7)),
        // One the check keeps, as it keeps one being set: less the space around it.
        ("red-kite", json!(" http://mac.tail.ts.net/x ")),
    ] {
        written(&f, handle, url);
    }
    assert_eq!(
        urls(&f),
        BTreeMap::from([
            ("amber-heron".into(), WEB.into()),
            ("red-kite".into(), "http://mac.tail.ts.net/x".into()),
        ])
    );
}

#[test]
fn leaving_deletes_the_address_and_coming_back_does_not_restore_it() {
    let f = Fixture::new(Seed::default().session("amber-heron"));
    f.store.set_session_url("amber-heron", WEB).unwrap();
    f.store.leave("amber-heron").unwrap();
    assert!(urls(&f).is_empty());
    assert!(
        f.store
            .raw()
            .row("callback_state", json!(["session_url", "amber-heron"]))
            .get()
            .unwrap()
            .is_null()
    );
    f.store.revive("amber-heron").unwrap();
    assert!(urls(&f).is_empty());
}

#[test]
fn an_address_is_no_callback_and_moves_no_board_marker() {
    let f = Fixture::new(Seed::default().session("amber-heron"));
    let before = structure(&f);
    f.store.set_session_url("amber-heron", WEB).unwrap();
    f.store.set_session_url("amber-heron", "").unwrap();
    f.store.set_session_url("amber-heron", WEB).unwrap();
    assert_eq!(structure(&f), before);
    assert!(f.store.callbacks().unwrap().is_empty());
    assert_eq!(f.store.chat_recipient().unwrap(), None);
}
