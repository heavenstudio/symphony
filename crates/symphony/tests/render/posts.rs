//! An agent's post outside a document (a task's message or brief, an ask's answer): its Markdown is
//! read as a document's, and raw HTML in it is the words typed, never an element, a notice of
//! unsupported content or words drawn twice. A document keeps its raw HTML.

use serde_json::Value;
use symphony::blocks;
use symphony::render::Render;
use symphony_api::{Block, Span};

use super::{Tree, bare};

const DOC: &str = "docs/x.md";

/// `text` as a post's blocks, as the task page and an ask are sent them.
fn post(text: &str) -> Vec<Block> {
    let (_tree, render) = bare();
    let fragment = render.typed_reply(text, DOC);
    blocks::parse(&fragment.html, &fragment.media, DOC)
}

/// Every kind the blocks hold, at any depth, and the words of their text and code, in order.
fn shown(blocks: &[Block]) -> (Vec<String>, String) {
    fn walk(value: &Value, kinds: &mut Vec<String>, words: &mut String) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(kind)) = map.get("type") {
                    kinds.push(kind.clone());
                    if let ("text" | "code", Some(Value::String(text))) =
                        (kind.as_str(), map.get("text"))
                    {
                        words.push_str(text);
                    }
                }
                map.values().for_each(|value| walk(value, kinds, words));
            }
            Value::Array(items) => items.iter().for_each(|value| walk(value, kinds, words)),
            _ => {}
        }
    }
    let (mut kinds, mut words) = (Vec::new(), String::new());
    walk(
        &serde_json::to_value(blocks).unwrap(),
        &mut kinds,
        &mut words,
    );
    (kinds, words)
}

fn text(words: &str) -> Span {
    Span::Text { text: words.into() }
}

fn paragraph(spans: Vec<Span>) -> Block {
    Block::Paragraph { spans }
}

#[test]
fn a_tag_like_word_in_a_line_is_drawn_as_typed_once() {
    for line in [
        "TasksPage wrapped in <CardCostsOf key=scope scope>, the key moved onto it.",
        "One <TokensPart/> line after the Changes section.",
        "It ends at </Foo> here.",
        "An unclosed <Foo never ends, nor <Foo bar='x' does.",
        "Vec<String> and Option<Vec<u8>> and a < b > c.",
    ] {
        assert_eq!(post(line), [paragraph(vec![text(line)])], "{line}");
    }
}

#[test]
fn a_tag_alone_on_its_line_opens_no_block_and_the_markdown_after_it_is_read() {
    assert_eq!(
        post("<Foo>\nthen **bold** after it\n\n<Foo/>\n\n</Foo>\n\n<Foo\nover two lines"),
        [
            paragraph(vec![
                text("<Foo>\nthen "),
                Span::Strong {
                    spans: vec![text("bold")]
                },
                text(" after it"),
            ]),
            paragraph(vec![text("<Foo/>")]),
            paragraph(vec![text("</Foo>")]),
            paragraph(vec![text("<Foo\nover two lines")]),
        ]
    );
}

#[test]
fn a_tag_the_documents_know_is_words_in_a_post_too() {
    let typed = "A button inside an open <details> and <a href=\"//evil.example\">, its <p> child, \
                 <b>raw</b>, <img src=x onerror=y> and <script>.\n\n\
                 <details>\n<summary>More</summary>\n\n<!-- said -->\n\n<div class=\"note\">\n\nkept\n\n</div>";
    let (kinds, words) = shown(&post(typed));
    for kind in [
        "details",
        "link",
        "image",
        "strong",
        "comment",
        "unknown",
        "group",
        "container",
    ] {
        assert!(!kinds.iter().any(|k| k == kind), "{kind} in {kinds:?}");
    }
    for word in [
        "<details>",
        "<a href=\"//evil.example\">",
        "<p>",
        "<b>raw</b>",
        "<img src=x onerror=y>",
        "<script>",
        "<summary>More</summary>",
        "<!-- said -->",
        "<div class=\"note\">",
        "</div>",
    ] {
        assert!(words.contains(word), "{word} in {words:?}");
    }
    assert_eq!(words.matches("kept").count(), 1, "{words}");
}

#[test]
fn a_posts_markdown_is_read_as_a_documents() {
    let blocks = post(
        "# Heading\n\n#Spaced\n\n*em*, **strong**, `<i>` in code, a [link](https://example.com) \
         and <https://example.org>\n\n- one\n- two\n\n```html\n<div>kept</div>\n```",
    );
    let [
        Block::Heading {
            level: 1,
            spans: first,
            ..
        },
        Block::Heading {
            level: 1,
            spans: second,
            ..
        },
        Block::Paragraph { spans },
        Block::List {
            ordered: false,
            items,
            ..
        },
        Block::Code {
            language,
            text: code,
        },
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}")
    };
    assert_eq!(
        (first.as_slice(), second.as_slice()),
        (&[text("Heading")][..], &[text("Spaced")][..])
    );
    assert!(matches!(&spans[0], Span::Emphasis { spans } if spans == &[text("em")]));
    assert!(matches!(&spans[2], Span::Strong { spans } if spans == &[text("strong")]));
    assert!(
        spans.contains(&Span::Code { text: "<i>".into() }),
        "{spans:?}"
    );
    let links: Vec<&str> = spans
        .iter()
        .filter_map(|span| match span {
            Span::Link { href, .. } => Some(href.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(links, ["https://example.com", "https://example.org"]);
    assert_eq!(items.len(), 2);
    assert_eq!(
        (language.as_str(), code.as_str()),
        ("html", "<div>kept</div>\n")
    );
}

#[test]
fn a_posts_picture_is_drawn() {
    let (kinds, _) = shown(&post("Here: ![the board](/shot.png) <Foo>"));
    assert!(kinds.iter().any(|k| k == "image"), "{kinds:?}");
    assert!(!kinds.iter().any(|k| k == "unknown"), "{kinds:?}");
}

#[test]
fn a_post_read_a_part_at_a_time_keeps_its_words_as_typed() {
    let (_tree, render) = bare();
    let text = (0..40)
        .map(|n| format!("<Foo{n}>\nline {n} with <Bar/> and **bold**\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let whole = render.typed_reply(&text, DOC);
    let mut from = 0;
    let mut parts = Vec::new();
    while from < text.len() {
        let (fragment, next) = render.typed_reply_part(&text, DOC, from, 200, 200);
        assert!(next > from);
        parts.push(fragment.html);
        from = next;
    }
    assert!(parts.len() > 1);
    assert_eq!(parts.join("\n"), whole.html);
    let (kinds, words) = shown(&blocks::parse(&whole.html, &[], DOC));
    assert!(!kinds.iter().any(|k| k == "unknown"), "{kinds:?}");
    for n in 0..40 {
        assert_eq!(
            words.matches(&format!("<Foo{n}>\nline {n} ")).count(),
            1,
            "{words}"
        );
    }
    assert_eq!(kinds.iter().filter(|k| *k == "strong").count(), 40);
}

#[test]
fn a_document_keeps_its_raw_html_and_names_what_it_cannot_draw_once() {
    let (_tree, render) = bare();
    let sections = render.typed_sections(
        "<details>\n<summary>More</summary>\nBody\n</details>\n\nA <widget>visible</widget> word.\n",
        DOC,
    );
    let blocks: Vec<Block> = sections
        .iter()
        .flat_map(|section| blocks::section(section, DOC))
        .collect();
    let (kinds, words) = shown(&blocks);
    assert!(kinds.iter().any(|k| k == "details"), "{kinds:?}");
    assert!(!kinds.iter().any(|k| k == "malformed"), "{kinds:?}");
    let named = serde_json::json!({ "type": "unknown", "tag": "widget", "text": "" });
    assert!(
        serde_json::to_string(&blocks)
            .unwrap()
            .contains(&named.to_string()),
        "{blocks:?}"
    );
    assert_eq!(words.matches("visible").count(), 1, "{words}");
}

#[test]
fn a_documents_section_shown_alone_keeps_its_raw_html() {
    let tree = Tree::new();
    let rel = "docs/x.md";
    tree.write(
        rel,
        "# Doc\n\n## Part\n\n<details><summary>More</summary>Body</details>\n",
    );
    let render = Render::new(&tree.root);
    let fragment = symphony::docview::section(&render, rel, "part", "").unwrap();
    let (kinds, _) = shown(&fragment.blocks);
    assert!(kinds.iter().any(|k| k == "details"), "{kinds:?}");
}
