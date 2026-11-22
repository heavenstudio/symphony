//! The client cache's format names the shapes the client keeps on the device
//! (docs/engineering/analysis/client-cache.md, "Across versions"): a kept type whose shape changes lands only
//! with a new `CACHE_FORMAT`, so no client reads a kept value of a shape it no longer writes.

use serde_json::{Value, json};
use symphony_api::{Board, CACHE_FORMAT, DocumentOutline, DocumentSections, Folder};

/// The format number, and the fingerprint of the kept shapes it was given for.
const FORMATS: (u32, &str) = (2, "defc018899698268");

/// A value's shape: its keys and the kinds of its values, not the values themselves, so a
/// fixture's text can change without moving the format. Its keys go in order whatever order the
/// map keeps them in: `serde_json` keeps insertion order when another crate of the build turns on
/// its `preserve_order`, as a workspace-wide build does.
fn shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("bool"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(items) => {
            let mut shapes: Vec<Value> = items.iter().map(shape).collect();
            shapes.sort_by_key(Value::to_string);
            shapes.dedup();
            Value::Array(shapes)
        }
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), shape(&fields[key])))
                    .collect(),
            )
        }
    }
}

/// FNV-1a, 64 bits: a fingerprint that is the same on every machine and toolchain.
fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The shapes of every kept type, each fixture read into its type and written back: what the
/// client writes to the device is what the type writes, fields it adds with a default included.
/// A slice that starts keeping a type adds its fixtures here.
fn kept() -> Value {
    json!({
        "board": [
            shape(&super::round_trip::<Board>("board.json").0),
            shape(&super::round_trip::<Board>("board-empty.json").0),
        ],
        // A document's outline, and its sections' bodies with their cards and threads, which the
        // client keeps in a body of its own (`cache::document::KeptBody`, whose own keys its tests
        // pin); an entrypoint's folder page.
        "document": [
            shape(&super::round_trip::<DocumentOutline>("document-outline.json").0),
            shape(&super::round_trip::<DocumentSections>("document-sections.json").0),
            shape(&super::round_trip::<Folder>("folder.json").0),
        ],
    })
}

#[test]
fn the_cache_format_moves_with_the_shapes_the_client_keeps() {
    let now = fingerprint(&kept().to_string());
    assert_eq!(
        (CACHE_FORMAT, now.as_str()),
        FORMATS,
        "a kept shape changed: give symphony_api::CACHE_FORMAT the next number and FORMATS \
         this fingerprint, {now}"
    );
}

/// What a document keeps is what its types write: each fixture reads and writes back as itself.
#[test]
fn a_documents_kept_reads_write_back_as_they_were_read() {
    for (name, (written, read)) in [
        (
            "document-outline.json",
            super::round_trip::<DocumentOutline>("document-outline.json"),
        ),
        (
            "document-sections.json",
            super::round_trip::<DocumentSections>("document-sections.json"),
        ),
        ("folder.json", super::round_trip::<Folder>("folder.json")),
    ] {
        assert_eq!(written, read, "{name}");
    }
}

#[test]
fn a_shape_is_of_keys_and_kinds_and_not_of_values() {
    let shaped = |text: &str| shape(&serde_json::from_str(text).unwrap());
    assert_eq!(
        shaped(r#"{"a": 1, "b": "x"}"#),
        shaped(r#"{"b": "y", "a": 2}"#)
    );
    assert_eq!(shaped("[1, 2, 3]"), shaped("[4]"));
    assert_ne!(
        shaped(r#"{"a": 1}"#),
        shaped(r#"{"a": "1"}"#),
        "a kind changed"
    );
    assert_ne!(
        shaped(r#"{"a": 1}"#),
        shaped(r#"{"b": 1}"#),
        "a key renamed"
    );
    assert_ne!(
        shaped(r#"{"a": 1}"#),
        shaped(r#"{"a": 1, "b": null}"#),
        "a key added"
    );
    assert_ne!(shaped(r#"{"a": [1]}"#), shaped(r#"{"a": [1, "x"]}"#));
    // Written in key order, whatever order the map keeps: the fingerprint hashes the text.
    assert_eq!(
        shaped(r#"{"b": 1, "a": {"d": true, "c": null}}"#).to_string(),
        r#"{"a":{"c":"null","d":"bool"},"b":"number"}"#
    );
    assert_eq!(fingerprint(""), "cbf29ce484222325");
    assert_eq!(fingerprint("a"), "af63dc4c8601ec8c");
}
