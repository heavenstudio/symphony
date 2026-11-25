//! An older host's reading of newer block content. Only vocabulary nodes are replaced, with the
//! existing Unknown block or span. The rest of the answer stays byte for byte the plugin's, so
//! the ordinary decoder still refuses bad fields, duplicate fields and malformed answers.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserializer as _;
use serde::de::{self, MapAccess, Visitor};
use serde_json::value::RawValue;
use symphony_plugin::protocol::{Ask, Draw, Render};

#[derive(Clone, Copy)]
enum Shape {
    Block,
    Span,
    List(&'static Shape),
    Fields(&'static [(&'static str, Shape)]),
}

const BLOCKS: Shape = Shape::List(&Shape::Block);
const SPANS: Shape = Shape::List(&Shape::Span);
const CELLS: Shape = Shape::List(&Shape::Fields(&[("spans", SPANS)]));
const THREAD: Shape = Shape::List(&Shape::Fields(&[("blocks", BLOCKS)]));

/// A changed answer to one of the two calls carrying blocks, or none when nothing was replaced.
pub(super) fn compatible(tag: &str, raw: &RawValue) -> Option<String> {
    let shape = match tag {
        Render::TAG => Shape::Fields(&[("Ok", BLOCKS)]),
        Draw::TAG => Shape::Fields(&[("Ok", Shape::Fields(&[("Blocks", BLOCKS)]))]),
        _ => return None,
    };
    let mut reader = Reader { edits: Vec::new() };
    reader.walk(raw, shape, 0);
    if reader.edits.is_empty() {
        return None;
    }
    let mut answer = raw.get().to_owned();
    // Each borrowed RawValue is a slice of this answer. Apply edits from the end so every
    // untouched field retains its original bytes, including duplicate fields a decoder refuses.
    symphony_board::order::sort_by_key(&mut reader.edits, &|(part, _)| part.get().as_ptr());
    for (part, replacement) in reader.edits.into_iter().rev() {
        let start = part.get().as_ptr() as usize - raw.get().as_ptr() as usize;
        answer.replace_range(start..start + part.get().len(), &replacement);
    }
    Some(answer)
}

struct Reader<'a> {
    edits: Vec<(&'a RawValue, String)>,
}

impl<'a> Reader<'a> {
    fn walk(&mut self, raw: &'a RawValue, shape: Shape, depth: usize) {
        // The ordinary JSON decoder bounds nesting too. A deeper answer is left for it to refuse.
        if depth >= 128 {
            return;
        }
        if let Shape::List(item) = shape {
            if let Ok(items) = serde_json::from_str::<Vec<&RawValue>>(raw.get()) {
                for raw in items {
                    self.walk(raw, *item, depth + 1);
                }
            }
            return;
        }
        let Some(object) = object_of(raw) else { return };
        let fields = match shape {
            Shape::Fields(fields) => fields,
            Shape::Block | Shape::Span => {
                let Some(kind) = word(&object, "type") else {
                    return;
                };
                let fields: &[(&str, Shape)] = match (shape, kind.as_str()) {
                    (Shape::Block, "heading" | "paragraph") => &[("spans", SPANS)],
                    (Shape::Block, "list") => &[("items", Shape::List(&BLOCKS))],
                    (Shape::Block, "table") => &[("head", CELLS), ("rows", Shape::List(&CELLS))],
                    (Shape::Block, "callout") => &[("title", SPANS), ("blocks", BLOCKS)],
                    (Shape::Block, "card") => {
                        &[("body", BLOCKS), ("thread", THREAD), ("comparison", BLOCKS)]
                    }
                    (Shape::Block, "figure") => &[("caption", SPANS)],
                    (Shape::Block, "comparison") => &[("figures", BLOCKS)],
                    (Shape::Block, "thread") => &[("entries", THREAD)],
                    (Shape::Block, "details" | "settled-card") => {
                        &[("summary", SPANS), ("blocks", BLOCKS)]
                    }
                    (Shape::Block, "quote" | "caption" | "group" | "container")
                    | (_, "component")
                    | (Shape::Span, "blocks") => &[("blocks", BLOCKS)],
                    (Shape::Span, "emphasis" | "strong" | "link" | "wiki-link" | "styled") => {
                        &[("spans", SPANS)]
                    }
                    (Shape::Block, "rule" | "comment")
                    | (Shape::Span, "text" | "line-break" | "icon" | "graphic")
                    | (_, "code" | "image" | "unknown") => &[],
                    (_, "media") => {
                        if let Some(media) = object.get("media").and_then(|raw| object_of(raw))
                            && let Some(kind) = word(&media, "kind")
                            && !matches!(kind.as_str(), "mockup" | "viewer" | "gallery")
                        {
                            self.unknown(raw, &format!("media: {kind}"));
                        }
                        return;
                    }
                    _ => {
                        self.unknown(raw, &kind);
                        return;
                    }
                };
                fields
            }
            Shape::List(_) => unreachable!(),
        };
        for (name, shape) in fields {
            if let Some(raw) = object.get(*name) {
                self.walk(raw, *shape, depth + 1);
            }
        }
    }

    fn unknown(&mut self, raw: &'a RawValue, tag: &str) {
        self.edits.push((
            raw,
            serde_json::json!({"type": "unknown", "tag": tag, "text": ""}).to_string(),
        ));
    }
}

/// A tag is text, never a missing or wrongly typed field repaired by the compatibility reader.
fn word(object: &BTreeMap<String, &RawValue>, key: &str) -> Option<String> {
    serde_json::from_str(object.get(key)?.get()).ok()
}

/// Read only this object's fields, rejecting duplicates before a replacement could hide them.
fn object_of(raw: &RawValue) -> Option<BTreeMap<String, &RawValue>> {
    struct Object;
    impl<'de> Visitor<'de> for Object {
        type Value = BTreeMap<String, &'de RawValue>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object without duplicate fields")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut fields = BTreeMap::new();
            while let Some((name, value)) = map.next_entry()? {
                if fields.insert(name, value).is_some() {
                    return Err(de::Error::custom("duplicate field"));
                }
            }
            Ok(fields)
        }
    }
    serde_json::Deserializer::from_str(raw.get())
        .deserialize_map(Object)
        .ok()
}

#[cfg(test)]
#[path = "process_content_tests.rs"]
mod tests;
