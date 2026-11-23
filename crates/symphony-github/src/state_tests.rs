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

    // The next line is not glued to the one cut short, which is cut away before it.
    store.journal(&entry("two", Event::Intent)).unwrap();
    let entries = store.entries().unwrap();
    let ops: Vec<(&str, Event)> = entries.iter().map(|e| (e.op.as_str(), e.event)).collect();
    assert_eq!(
        ops,
        [
            ("one", Event::Intent),
            ("one", Event::Ack),
            ("two", Event::Intent)
        ]
    );
    let text = fs::read_to_string(dir.join("journal.jsonl")).unwrap();
    assert!(!text.contains("inte\""), "{text}");
    assert_eq!(text.lines().count(), 3, "{text}");

    // A whole line whose end was not written yet is cut short too: its operation never went on.
    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    let whole = serde_json::to_string(&entry("three", Event::Intent)).unwrap();
    file.write_all(whole.as_bytes()).unwrap();
    assert_eq!(store.entries().unwrap().len(), 3);
    store.journal(&entry("four", Event::Intent)).unwrap();
    let last = store.entries().unwrap().pop().unwrap();
    assert_eq!(last.op, "four");
    assert_eq!(store.entries().unwrap().len(), 4);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_journal_with_nothing_but_a_line_cut_short_holds_nothing() {
    let dir = dir("journal-cut-only");
    let store = Store::open(&dir).unwrap();
    fs::write(dir.join("journal.jsonl"), "{\"at\":1,\"ev").unwrap();
    assert_eq!(store.entries().unwrap(), []);
    store.journal(&entry("one", Event::Intent)).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].op, "one");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_last_line_cut_inside_a_character_is_left_out_like_any_cut_short() {
    let dir = dir("journal-cut-in-a-character");
    let store = Store::open(&dir).unwrap();
    store.journal(&entry("one", Event::Ack)).unwrap();
    let mut whole = entry("café", Event::Intent);
    whole.target = "naïve/ünïcode".into();
    let line = serde_json::to_string(&whole).unwrap();
    let inside = line.find('é').unwrap() + 1;
    assert!(
        !line.is_char_boundary(inside),
        "the cut ends inside a character"
    );
    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    file.write_all(&line.as_bytes()[..inside]).unwrap();
    assert!(
        fs::read_to_string(dir.join("journal.jsonl")).is_err(),
        "not text whole"
    );
    let entries = store.entries().unwrap();
    let ops: Vec<&str> = entries.iter().map(|e| e.op.as_str()).collect();
    assert_eq!(ops, ["one"]);

    // Cut away before the next, which reads back whole, its characters too.
    store.journal(&whole).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1], whole);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_whole_line_that_is_not_text_is_a_refusal_naming_it() {
    let dir = dir("journal-not-text");
    let store = Store::open(&dir).unwrap();
    store.journal(&entry("one", Event::Ack)).unwrap();
    let line = serde_json::to_string(&entry("é", Event::Intent)).unwrap();
    let at = line.find('é').unwrap();
    let mut bytes = line.into_bytes();
    bytes[at + 1] = 0xff;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    file.write_all(&bytes).unwrap();
    store.journal(&entry("three", Event::Intent)).unwrap();
    let refused = store.entries().unwrap_err();
    assert!(refused.contains("journal.jsonl"), "{refused}");
    assert!(refused.contains("line 2 does not read"), "{refused}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_line_before_the_last_is_a_refusal_naming_it() {
    let dir = dir("journal-damaged");
    let store = Store::open(&dir).unwrap();
    store.journal(&entry("one", Event::Ack)).unwrap();
    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join("journal.jsonl"))
        .unwrap();
    file.write_all(b"{\"at\":2,\"event\":\"intent\",\"op\":\"lost\n")
        .unwrap();
    store.journal(&entry("two", Event::Intent)).unwrap();
    let refused = store.entries().unwrap_err();
    assert!(refused.contains("journal.jsonl"), "{refused}");
    assert!(refused.contains("line 2 does not read"), "{refused}");

    // A blank line is damage as well: no line of the journal is ever empty.
    fs::write(
        dir.join("journal.jsonl"),
        format!(
            "\n{}\n",
            serde_json::to_string(&entry("one", Event::Ack)).unwrap()
        ),
    )
    .unwrap();
    assert!(
        store
            .entries()
            .unwrap_err()
            .contains("line 1 does not read")
    );
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

/// A copy of the lock's descriptor kept past its drop, as a process started while it was held
/// keeps one, holds nothing: the drop unlocks it, where a close alone would release it only with
/// its last copy, and the next run would be refused as a second writer.
#[test]
fn a_copy_of_the_locks_descriptor_kept_past_its_drop_holds_nothing() {
    let dir = dir("lock-copy");
    let store = Store::open(&dir).unwrap();
    let held = store.lock().unwrap();
    let copy = held.0.try_clone().unwrap();
    assert!(store.lock().is_err(), "held, whatever copies it has");
    drop(held);
    let again = store
        .lock()
        .expect("a copy of its descriptor held it past the drop");
    // The copy, closed at last, lets go of nothing the new holder holds.
    drop(copy);
    assert!(store.lock().is_err(), "the new holder holds it still");
    drop(again);
    assert!(store.lock().is_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_flag_says_what_it_is() {
    assert!(Flag::Conflict.said().contains("both sides changed"));
    assert!(Flag::Inaccessible.said().contains("not accessible"));
    assert_eq!(Flag::Suspended("why".into()).said(), "suspended: why");
}
