//! A section's ends (`Section::ends_of`): the kinds of the first and last blocks it draws, which the
//! outline carries so a page spaces a section it has not read yet as it will draw.

use serde_json::{Value, json};
use symphony_api::{Block, Section};

fn blocks(value: Value) -> Vec<Block> {
    serde_json::from_value(value).expect("blocks")
}

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "spans": [{"type": "text", "text": text}]})
}

fn heading(level: u8) -> Value {
    json!({"type": "heading", "level": level, "anchor": "lamps", "spans": []})
}

fn ends(value: Value) -> Option<[String; 2]> {
    Section::ends_of(&blocks(value))
}

fn pair(first: &str, last: &str) -> [String; 2] {
    [first.into(), last.into()]
}

#[test]
fn a_section_names_its_first_and_last_blocks_a_heading_by_its_level() {
    assert_eq!(
        ends(json!([paragraph("One.")])),
        Some(pair("paragraph", "paragraph"))
    );
    assert_eq!(
        ends(
            json!([heading(3), paragraph("One."), {"type": "code", "language": "rs", "text": "x"}])
        ),
        Some(pair("heading-3", "code"))
    );
    assert_eq!(
        ends(
            json!([heading(1), {"type": "list", "ordered": false, "start": 1, "items": [[paragraph("A")]]}])
        ),
        Some(pair("heading-1", "list"))
    );
}

#[test]
fn a_group_is_read_through_and_what_draws_nothing_is_passed_over() {
    let quote = json!({"type": "quote", "blocks": [paragraph("Said.")]});
    let table = json!({"type": "table", "head": [], "rows": []});
    assert_eq!(
        ends(json!([
            {"type": "comment", "text": "a note"},
            {"type": "group", "blocks": []},
            {"type": "group", "blocks": [{"type": "comment", "text": "x"}, quote]},
            paragraph("Middle."),
            {"type": "group", "blocks": [table, {"type": "group", "blocks": []}]},
            {"type": "comment", "text": "after"},
        ])),
        Some(pair("quote", "table"))
    );
    assert_eq!(ends(json!([])), None);
    assert_eq!(
        ends(
            json!([{"type": "comment", "text": "only"}, {"type": "group", "blocks": [{"type": "group", "blocks": []}]}])
        ),
        None,
        "a section that draws nothing has no ends"
    );
}

#[test]
fn a_container_drawn_with_the_callout_class_is_a_callout_and_any_other_a_container() {
    let container = |class: &str| json!({"type": "container", "tag": "div", "presentation": {"class": class}, "blocks": [paragraph("In it.")]});
    assert_eq!(
        ends(json!([container("wide callout")])),
        Some(pair("callout", "callout"))
    );
    assert_eq!(
        ends(json!([container("callouts")])),
        Some(pair("container", "container"))
    );
    assert_eq!(
        ends(
            json!([{"type": "callout", "kind": "comment", "title": [], "resolved": true, "blocks": []}, container("")])
        ),
        Some(pair("callout", "container"))
    );
}

/// Each kind but a heading's is the block's own `type` on the wire, so the page reads the two alike.
#[test]
fn each_kind_is_the_blocks_type_on_the_wire() {
    let each = json!([
        paragraph("P"),
        {"type": "list", "ordered": true, "start": 3, "items": []},
        {"type": "table", "head": [], "rows": []},
        {"type": "code", "language": "", "text": ""},
        {"type": "quote", "blocks": []},
        {"type": "rule"},
        {"type": "callout", "kind": "question", "title": [], "resolved": false, "blocks": []},
        {"type": "card", "slug": "lamp", "mark": "pending", "title": "Lamp", "body": [], "verdict": {"group": "decide", "open": true, "posted": null, "decided": null}, "thread": [], "line": null, "line_offset": null, "comparison": []},
        {"type": "component", "kind": "chart", "blocks": []},
        {"type": "caption", "blocks": []},
        {"type": "comparison", "figures": []},
        {"type": "container", "tag": "aside", "presentation": {}, "blocks": []},
        {"type": "details", "summary": [], "open": false, "blocks": []},
        {"type": "settled-card", "summary": [], "open": false, "blocks": []},
        {"type": "thread", "entries": []},
        {"type": "unknown", "tag": "marquee", "text": "x"},
    ]);
    for block in each.as_array().unwrap() {
        let kind = block["type"].as_str().unwrap();
        assert_eq!(ends(json!([block])), Some(pair(kind, kind)), "{block}");
    }
}

#[test]
fn a_section_carries_its_ends_on_the_wire_and_one_kept_from_before_reads_without_them() {
    let kept = json!({"start": 3, "end": 4, "key": "lamps", "hash": "h", "part": 1, "blocks": []});
    let section: Section =
        serde_json::from_value(kept.clone()).expect("a section kept from before");
    assert_eq!(section.ends, None);
    assert_eq!(
        serde_json::to_value(&section).unwrap(),
        kept,
        "none is left out"
    );
    let mut ended = section;
    ended.ends = Some(pair("heading-2", "table"));
    let written = serde_json::to_value(&ended).unwrap();
    assert_eq!(written["ends"], json!(["heading-2", "table"]));
    assert_eq!(serde_json::from_value::<Section>(written).unwrap(), ended);
}
