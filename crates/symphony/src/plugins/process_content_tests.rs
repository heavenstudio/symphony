use serde_json::{Value, json};
use symphony_plugin::Drawing;
use symphony_plugin::vocabulary::{Block, Span};

use super::*;
use crate::plugins::process::answer_of;

fn raw(text: &str) -> Box<RawValue> {
    RawValue::from_string(text.to_owned()).unwrap()
}

fn blocks(text: &str) -> Result<Vec<Block>, String> {
    answer_of::<Result<Vec<Block>, String>>(Render::TAG, &raw(text))?
}

#[test]
fn every_nested_content_position_keeps_unknown_nodes_in_place() {
    let unknown = json!({"type":"future"});
    let cell = json!({"header":false,"spans":[unknown],"presentation":{}});
    let entry = json!({"who":"owner","blocks":[unknown]});
    let cases = [
        (
            json!({"type":"heading","level":2,"anchor":"a","spans":[unknown]}),
            "/spans/0",
        ),
        (
            json!({"type":"list","ordered":true,"start":1,"items":[[unknown]]}),
            "/items/0/0",
        ),
        (
            json!({"type":"table","head":[cell],"rows":[]}),
            "/head/0/spans/0",
        ),
        (
            json!({"type":"table","head":[],"rows":[[cell]]}),
            "/rows/0/0/spans/0",
        ),
        (
            json!({"type":"callout","kind":"comment","title":[unknown],"resolved":false,"blocks":[]}),
            "/title/0",
        ),
        (
            json!({"type":"callout","kind":"comment","title":[],"resolved":false,"blocks":[unknown]}),
            "/blocks/0",
        ),
        (
            json!({"type":"card","slug":"s","mark":"pending","title":"t","body":[unknown],"verdict":{"group":"decide","open":true},"thread":[],"comparison":[]}),
            "/body/0",
        ),
        (
            json!({"type":"card","slug":"s","mark":"pending","title":"t","body":[],"verdict":{"group":"decide","open":true},"thread":[entry],"comparison":[]}),
            "/thread/0/blocks/0",
        ),
        (
            json!({"type":"card","slug":"s","mark":"pending","title":"t","body":[],"verdict":{"group":"decide","open":true},"thread":[],"comparison":[unknown]}),
            "/comparison/0",
        ),
        (
            json!({"type":"figure","size":"small","caption":[unknown]}),
            "/caption/0",
        ),
        (
            json!({"type":"comparison","figures":[unknown]}),
            "/figures/0",
        ),
        (
            json!({"type":"thread","entries":[entry]}),
            "/entries/0/blocks/0",
        ),
        (
            json!({"type":"details","summary":[unknown],"open":false,"blocks":[]}),
            "/summary/0",
        ),
        (
            json!({"type":"settled-card","summary":[],"open":false,"blocks":[unknown]}),
            "/blocks/0",
        ),
        (
            json!({"type":"container","tag":"section","presentation":{},"blocks":[unknown]}),
            "/blocks/0",
        ),
    ];
    for (node, path) in cases
        .into_iter()
        .chain(["quote", "caption", "group", "component"].map(|kind| {
            (
                json!({"type":kind,"kind":"tiny","blocks":[unknown]}),
                "/blocks/0",
            )
        }))
    {
        let input = json!({"Ok":[node]});
        let changed = compatible(Render::TAG, &raw(&input.to_string())).unwrap();
        let mut expected = input;
        *expected["Ok"][0].pointer_mut(path).unwrap() =
            json!({"type":"unknown","tag":"future","text":""});
        assert_eq!(
            serde_json::from_str::<Value>(&changed).unwrap(),
            expected,
            "{path}"
        );
        serde_json::from_str::<Result<Vec<Block>, String>>(&changed)
            .unwrap()
            .unwrap();
    }
}

#[test]
fn nested_spans_and_their_embedded_blocks_are_read_too() {
    for (kind, field) in [
        ("emphasis", "spans"),
        ("strong", "spans"),
        ("link", "spans"),
        ("wiki-link", "spans"),
        ("styled", "spans"),
        ("component", "blocks"),
        ("blocks", "blocks"),
    ] {
        let span = json!({"type":kind,"href":"/a","title":"a","tag":"mark","presentation":{},"kind":"tiny",field:[{"type":"future"}]});
        let input = json!({"answer":{"Ok":[{"type":"paragraph","spans":[span]}]}});
        let read = blocks(&input.to_string()).unwrap();
        let written = serde_json::to_value(read).unwrap();
        assert_eq!(
            written[0]["spans"][0][field][0],
            json!({"type":"unknown","tag":"future","text":""}),
            "{kind}"
        );
    }
}

#[test]
fn renderer_block_drawings_have_the_same_compatibility() {
    let answer: Result<Drawing, String> = answer_of(
        Draw::TAG,
        &raw(r#"{"answer":{"Ok":{"Blocks":[{"type":"future"},{"type":"rule"}]}}}"#),
    )
    .unwrap();
    assert_eq!(
        answer,
        Ok(Drawing::Blocks(vec![
            Block::Unknown {
                tag: "future".into(),
                text: String::new()
            },
            Block::Rule
        ]))
    );
}

#[test]
fn unknown_tags_are_json_text_including_escapes_and_empty_names() {
    for kind in ["", "quote\"\\\n☃", "__proto__"] {
        let input = json!({"answer":{"Ok":[{"type":kind}]}});
        assert_eq!(
            blocks(&input.to_string()),
            Ok(vec![Block::Unknown {
                tag: kind.into(),
                text: String::new()
            }])
        );
    }
    assert_eq!(
        blocks(r#"{"answer":{"Ok":[{"type":"fut\u0075re"}]}}"#),
        Ok(vec![Block::Unknown {
            tag: "future".into(),
            text: String::new()
        }])
    );
}

#[test]
fn unknown_kinds_do_not_repair_bad_known_fields_or_answer_shapes() {
    let bad = [
        r#"{}"#,
        r#"{"type":null}"#,
        r#"{"type":7}"#,
        r#"{"type":"paragraph"}"#,
        r#"{"type":"paragraph","spans":false}"#,
        r#"{"type":"paragraph","spans":[{"type":"text","text":7}]}"#,
        r#"{"type":"paragraph","spans":[{"type":"text"}]}"#,
        r#"{"type":"container","tag":"future","presentation":{},"blocks":[{"type":"future"}]}"#,
        r#"{"type":"media","media":{}}"#,
        r#"{"type":"media","media":{"kind":7}}"#,
        r#"{"type":"media","media":{"kind":"viewer","src":7}}"#,
        r#"{"type":"media","media":{"kind":"mockup"}}"#,
        r#"{"type":"media","media":{"kind":"gallery","items":false}}"#,
        r#"{"type":"paragraph","spans":[{"type":"strong","spans":null}]}"#,
        r#"{"type":"paragraph","spans":[],"spans":[]}"#,
        r#"{"type":"future","type":"future"}"#,
        r#"{"type":"media","media":{"kind":"future","kind":"future"}}"#,
    ];
    for node in bad {
        let text = format!(r#"{{"answer":{{"Ok":[{{"type":"future"}},{node}]}}}}"#);
        assert!(blocks(&text).is_err(), "accepted {node}");
    }
    for text in [
        r#"{"answer":{"Ok":false}}"#,
        r#"{"answer":{"Ok":[{"type":"future"}],"Err":"refused"}}"#,
        r#"{"answer":{"Ok":[],"Ok":[{"type":"future"}]}}"#,
        r#"{"answer":{},"refused":"no"}"#,
        r#"{"refused":"no"}"#,
    ] {
        assert!(blocks(text).is_err(), "accepted {text}");
    }
    assert!(
        serde_json::from_str::<Box<RawValue>>(r#"{"answer":{"Ok":[{"type":"future"},]}}"#).is_err()
    );
}

#[test]
fn known_content_and_unrelated_data_are_untouched() {
    for (tag, text) in [
        (
            Render::TAG,
            r#"{"Ok":[{"type":"rule"},{"type":"unknown","tag":"markup","text":"kept"}]}"#,
        ),
        (Render::TAG, r#"{"Err":"plugin refused"}"#),
        (
            Render::TAG,
            r#"{"Ok":[{"type":"media","media":{"kind":"viewer"}}]}"#,
        ),
        (
            Render::TAG,
            r#"{"Ok":[{"type":"media","media":{"kind":"gallery"}}]}"#,
        ),
        (Draw::TAG, r#"{"Ok":{"Viewer":{"type":"future"}}}"#),
        ("routes.serve", r#"{"Ok":[{"type":"future"}]}"#),
    ] {
        assert_eq!(compatible(tag, &raw(text)), None, "{tag}: {text}");
    }
    let text = r#"{"Ok":[ {"type":"future"}, {"type":"rule", "extra":{"type":"future","number":123456789012345678901234567890}} ]}"#;
    let changed = compatible(Render::TAG, &raw(text)).unwrap();
    assert!(changed.ends_with(
        r#", {"type":"rule", "extra":{"type":"future","number":123456789012345678901234567890}} ]}"#
    ));
    let read = blocks(&format!(r#"{{"answer":{changed}}}"#)).unwrap();
    assert_eq!(read[1], Block::Rule);
}

#[test]
fn kinds_from_the_other_context_are_unsupported() {
    let read = blocks(r#"{"answer":{"Ok":[{"type":"strong"},{"type":"paragraph","spans":[{"type":"paragraph"}]}]}}"#).unwrap();
    assert_eq!(
        read,
        vec![
            Block::Unknown {
                tag: "strong".into(),
                text: String::new()
            },
            Block::Paragraph {
                spans: vec![Span::Unknown {
                    tag: "paragraph".into(),
                    text: String::new()
                }]
            }
        ]
    );
}

#[test]
fn deep_content_is_left_for_the_normal_decoders_nesting_limit() {
    let mut node = r#"{"type":"future"}"#.to_owned();
    for _ in 0..130 {
        node = format!(r#"{{"type":"group","blocks":[{node}]}}"#);
    }
    let text = format!(r#"{{"Ok":[{node}]}}"#);
    assert!(compatible(Render::TAG, &raw(&text)).is_none());
    assert!(blocks(&format!(r#"{{"answer":{text}}}"#)).is_err());
}
