use super::*;
use serde_json::json;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-state-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn entry(op: &str, event: Event) -> Entry {
    Entry {
        at: 1,
        event,
        preview: "p".into(),
        op: op.into(),
        kind: "publish".into(),
        target: "a/a".into(),
        digest: "d".into(),
        base: json!({"generation": "g", "id": 1}),
        config: "rev".into(),
        by: "amber-fox-1".into(),
        result: Value::Null,
    }
}

#[test]
fn nothing_is_kept_until_something_is() {
    let dir = dir("empty");
    assert!(Store::existing(&dir).is_none(), "a read makes no folder");
    assert!(!dir.exists());
    let store = Store::open(&dir.join("nested/github")).unwrap();
    assert_eq!(store.state().unwrap(), None);
    assert_eq!(store.preview().unwrap(), None);
    assert_eq!(store.entries().unwrap(), []);
    store.drop_preview().unwrap();
    assert!(Store::existing(&dir.join("nested/github")).is_some());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_state_is_replaced_whole_and_read_back() {
    let dir = dir("state");
    let store = Store::open(&dir).unwrap();
    let mut state = State {
        binding: Some(Binding {
            repository_id: 7,
            node_id: "R7".into(),
            full_name: "o/n".into(),
        }),
        cursor: Some("2026-01-01T00:00:00Z".into()),
        ..State::default()
    };
    state.drafts.insert(
        "a/a".into(),
        Draft {
            title: "T".into(),
            body: "B".into(),
        },
    );
    state.resolutions.insert("a/a".into(), Side::Remote);
    store.save(&state).unwrap();
    assert_eq!(store.state().unwrap().as_ref(), Some(&state));
    assert!(
        !dir.join("state.json.new").exists(),
        "the file written beside it was moved over it"
    );
    state.paused = Some("why".into());
    store.save(&state).unwrap();
    assert_eq!(
        store.state().unwrap().unwrap().paused.as_deref(),
        Some("why")
    );

    // A state file that does not read is said, never taken for none.
    fs::write(dir.join("state.json"), "{ cut").unwrap();
    assert!(store.state().unwrap_err().contains("state.json"));

    let preview = Preview::new("rev", Vec::new(), None, Vec::new());
    store.keep_preview(&preview).unwrap();
    assert_eq!(store.preview().unwrap(), Some(preview));
    store.drop_preview().unwrap();
    assert_eq!(store.preview().unwrap(), None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_journal_is_appended_and_a_line_cut_short_is_left_out() {
    let dir = dir("journal");
    let store = Store::open(&dir).unwrap();
    store.journal(&entry("one", Event::Intent)).unwrap();
    store.journal(&entry("one", Event::Ack)).unwrap();
    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    file.write_all(b"{\"at\":2,\"event\":\"inte").unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        (entries[0].event, entries[1].event),
        (Event::Intent, Event::Ack)
    );
    assert_eq!(
        entries[0].base["generation"], "g",
        "the board's generation is journaled with the intent"
    );
    let text = fs::read_to_string(dir.join("journal.jsonl")).unwrap();
    assert!(text.starts_with("{\"at\":1,\"event\":\"intent\""), "{text}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn one_holder_of_the_lock_at_a_time() {
    let dir = dir("lock");
    let store = Store::open(&dir).unwrap();
    let held = store.lock().unwrap();
    let other = Store::open(&dir).unwrap();
    let refused = other
        .lock()
        .err()
        .expect("a second holder is refused, not made to wait");
    assert!(refused.contains("one writer at a time"), "{refused}");
    drop(held);
    assert!(other.lock().is_ok(), "released when its holder drops it");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_flag_says_what_it_is() {
    assert!(Flag::Conflict.said().contains("both sides changed"));
    assert!(Flag::Inaccessible.said().contains("not accessible"));
    assert_eq!(Flag::Suspended("why".into()).said(), "suspended: why");
}
