use super::*;
use crate::people::Profiles;

fn key(key: &str) -> Step {
    Step::Key(key.into())
}

/// The steps a fault in `text` read as any JSON value is at: a fault of syntax.
fn syntax(text: &str) -> Vec<Step> {
    let error = serde_json::from_str::<Value>(text).unwrap_err();
    assert_ne!(error.classify(), Category::Data, "{text}: {error}");
    steps(text, &error)
}

/// The steps a fault in `text` read as profiles is at.
fn profiles(text: &str) -> Vec<Step> {
    let error = serde_json::from_str::<Profiles>(text).unwrap_err();
    steps(text, &error)
}

/// A fault of syntax between a list's items, at its comma or its close, is the list's; one where
/// an item goes, other than a trailing comma's close, is that item's.
#[test]
fn a_fault_between_a_lists_items_is_the_lists_and_one_in_an_item_the_items() {
    for (text, at) in [
        ("[1 2]", vec![]),
        ("[1,]", vec![]),
        ("[1,", vec![]),
        ("[1, ", vec![]),
        ("[", vec![]),
        ("[1", vec![]),
        ("[1.5e3", vec![]),
        ("[true", vec![]),
        ("[1x]", vec![]),
        ("[truex]", vec![]),
        ("[x]", vec![Step::Item(0)]),
        ("[1, x]", vec![Step::Item(1)]),
        ("[1,,2]", vec![Step::Item(1)]),
        ("[,1]", vec![Step::Item(0)]),
        ("[1, [2, x]]", vec![Step::Item(1), Step::Item(1)]),
        ("[1, [2, 3] 4]", vec![]),
        ("[1, [2, 3]", vec![]),
        ("[1, [2, 3", vec![Step::Item(1)]),
    ] {
        assert_eq!(syntax(text), at, "{text}");
    }
}

/// A fault of syntax at an object's key, its colon or between its members is the object's; one
/// after a colon, the value due or begun, is that member's.
#[test]
fn a_fault_at_an_objects_key_or_colon_is_the_objects_and_one_after_its_colon_the_members() {
    for (text, at) in [
        (r#"{"a" 1}"#, vec![]),
        (r#"{"a""#, vec![]),
        (r#"{"a"#, vec![]),
        (r#"{1: 2}"#, vec![]),
        (r#"{"a": 1 "b": 2}"#, vec![]),
        (r#"{"a": 1,}"#, vec![]),
        (r#"{"a": 1, 2}"#, vec![]),
        (r#"{"a": 1"#, vec![]),
        (r#"{"a": }"#, vec![key("a")]),
        (r#"{"a":"#, vec![key("a")]),
        (
            r#"{"a": {"b": [1, {"c" 2}]}}"#,
            vec![key("a"), key("b"), Step::Item(1)],
        ),
        (
            r#"{"a": {"b": 1}, "c": [x]}"#,
            vec![key("c"), Step::Item(0)],
        ),
    ] {
        assert_eq!(syntax(text), at, "{text}");
    }
}

/// A word, a number or a string cut short, or with a byte it does not take, is a fault in its
/// value, its member's or item's: never a close or a comma the bad byte happens to be.
#[test]
fn a_token_cut_short_or_written_wrong_is_a_fault_in_its_value() {
    for text in [
        r#"{"a": tru}"#,
        r#"{"a": nul"#,
        r#"{"a": fals}"#,
        r#"{"a": -}"#,
        r#"{"a": -x}"#,
        r#"{"a": 01}"#,
        r#"{"a": 1.}"#,
        r#"{"a": 1.x}"#,
        r#"{"a": 1e}"#,
        r#"{"a": 1e+}"#,
        r#"{"a": "b"#,
        r#"{"a": "b\"}"#,
        r#"{"a": "\q"}"#,
        r#"{"a": "\u0"00"}"#,
    ] {
        assert_eq!(syntax(text), [key("a")], "{text}");
    }
    // A string's escaped quote, and a close inside it, close nothing.
    assert_eq!(syntax(r#"["a\"]", "\\", x]"#), [Step::Item(2)]);
    assert_eq!(syntax(r#"["]}", 1 2]"#), []);
}

/// A key is said as serde_json reads it, its escapes read, and a fault before any value, or after
/// the text's one value, is at no step.
#[test]
fn a_key_is_read_with_its_escapes_and_a_fault_outside_every_container_is_at_none() {
    assert_eq!(syntax(r#"{"na\u006de": tru}"#), [key("name")]);
    // Text that is not a whole string serde_json reads is no key.
    assert_eq!(super::key(r#""a\q""#), "");
    assert_eq!(syntax(r#"{"a\"b": tru}"#), [key("a\"b")]);
    for text in ["", " ", "x", "{} x", "[] []", "\u{feff}{}"] {
        assert_eq!(syntax(text), [], "{text:?}");
    }
}

/// A fault of data is the value's just read: a value of the wrong type, or a struct closed without
/// a field it needs; where a container goes before another value, the value that stands there,
/// unread. A field written twice is its object's, as serde_json faults it at its key.
#[test]
fn a_fault_of_data_is_the_value_just_read_or_its_objects_at_a_key() {
    let ana = |rest: &str| format!(r#"{{"people": [{{"name": "ana"{rest}}}]}}"#);
    let signins = |rest: &str| {
        vec![key("people"), Step::Item(0), key("signins")]
            .into_iter()
            .chain(rest.is_empty().then_some(Step::Item(0)))
            .collect::<Vec<_>>()
    };
    assert_eq!(profiles(&ana(r#", "signins": 4"#)), signins("x"));
    assert_eq!(profiles(&ana(r#", "signins": {"a": 1}"#)), signins("x"));
    assert_eq!(
        profiles(&ana(r#", "signins": [{"service": "s"}]"#)),
        signins("")
    );
    assert_eq!(profiles(&ana(r#", "signins": [4]"#)), signins(""));
    assert_eq!(
        profiles(&ana(r#", "git": ["a", {"b": 1}]"#)),
        [key("people"), Step::Item(0), key("git"), Step::Item(1)]
    );
    assert_eq!(
        profiles(&ana(r#", "git": [{"b": 1}]"#)),
        [key("people"), Step::Item(0), key("git"), Step::Item(0)]
    );
    assert_eq!(
        profiles(&ana(r#", "name": "bea""#)),
        [key("people"), Step::Item(0)]
    );
    assert_eq!(
        profiles(r#"{"people": [{"name": "ana"}, {}]}"#),
        [key("people"), Step::Item(1)]
    );
    // A list read as a struct with more items than its fields is faulted at the first byte of
    // the next, which serde_json never read: the list's, whatever that byte begins.
    for extra in [r#""u""#, "[1]", r#"{"a": 1}"#, "-1", "true", ", 1"] {
        assert_eq!(
            profiles(&ana(&format!(r#", "signins": [["s", "t", {extra}]]"#))),
            signins(""),
            "{extra}"
        );
    }
    assert_eq!(profiles(r#""ana""#), []);
    assert_eq!(profiles("4"), []);
    assert_eq!(profiles("[4]"), [Step::Item(0)]);
}

/// serde_json counts a fault's column in the bytes of its line, a letter of several bytes and a
/// carriage return among them.
#[test]
fn a_faults_column_counts_bytes_on_its_line() {
    let text = "{\"people\": [\r\n {\"name\": \"Ana Müller\",\r\n  \"signins\": 4}]}";
    assert_eq!(
        profiles(text),
        [key("people"), Step::Item(0), key("signins")]
    );
    let text = "[\n\"ü\",\n \"é\" 2]";
    assert_eq!(syntax(text), []);
    let text = "[\n\"ü\",\n \"é\", x]";
    assert_eq!(syntax(text), [Step::Item(2)]);
}
