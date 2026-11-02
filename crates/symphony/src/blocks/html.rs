//! A bounded reader for the pipeline's HTML fragments, not a browser DOM. In particular it keeps
//! nested block/inline component output as written rather than applying browser error correction.
//! Quoted `>` characters, comments, void elements and SVG self-closing primitives are retained.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Default)]
pub(super) struct Node {
    pub tag: String,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map_or("", String::as_str)
    }
    pub fn class(&self, class: &str) -> bool {
        self.attr("class").split_whitespace().any(|c| c == class)
    }
    pub fn blank(&self) -> bool {
        self.tag.is_empty() && self.text.trim().is_empty()
    }
    pub fn plain(&self) -> String {
        if self.tag == "br" {
            return "\n".into();
        }
        let mut text = self.text.clone();
        for child in &self.children {
            text.push_str(&child.plain());
        }
        text
    }
    pub fn is_inline(&self) -> bool {
        self.tag.is_empty()
            || super::inline(&self.tag).is_some()
            || ["a", "em", "i", "strong", "b", "code", "br", "img", "svg"]
                .contains(&self.tag.as_str())
    }
}

fn decode(text: &str) -> String {
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(?:#[0-9]+|#[xX][0-9a-fA-F]+|[A-Za-z][A-Za-z0-9]+);").expect("entity pattern")
    });
    ENTITY
        .replace_all(text, |found: &regex::Captures<'_>| {
            if found[0].starts_with("&#") {
                return crate::markup::unescape(&found[0]);
            }
            // Use the existing Markdown reader's complete HTML entity catalogue. Parsing only the
            // entity (not its surrounding text) cannot interpret Markdown or unescape code twice.
            let arena = comrak::Arena::new();
            let doc = comrak::parse_document(&arena, &found[0], &comrak::Options::default());
            doc.descendants()
                .find_map(|n| match &n.data.borrow().value {
                    comrak::nodes::NodeValue::Text(text) => Some(text.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| found[0].to_string())
        })
        .into_owned()
}

pub(super) fn parse(html: &str) -> Vec<Node> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?s)<!--.*?-->|</?[A-Za-z][A-Za-z0-9:-]*(?:\s+(?:[^'">]|'[^']*'|"[^"]*")*)?/?>"#,
        )
        .expect("fragment token pattern")
    });
    static ATTR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"([^\s=/'">]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?"#)
            .expect("fragment attribute pattern")
    });
    let mut stack = vec![Node::default()];
    let mut last = 0;
    for token in TOKEN.find_iter(html) {
        push_text(&mut stack, &html[last..token.start()]);
        last = token.end();
        let text = token.as_str();
        if let Some(comment) = text
            .strip_prefix("<!--")
            .and_then(|s| s.strip_suffix("-->"))
        {
            push(
                &mut stack,
                Node {
                    tag: "#comment".into(),
                    text: comment.into(),
                    ..Node::default()
                },
            );
            continue;
        }
        let closing = text.starts_with("</");
        let content = &text[if closing { 2 } else { 1 }..text.len() - 1];
        let length = content
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || b":-".contains(c))
            .count();
        let tag = content[..length].to_ascii_lowercase();
        if closing {
            if let Some(at) = stack.iter().rposition(|n| n.tag == tag) {
                while stack.len() > at {
                    close(&mut stack);
                }
            } else {
                push(
                    &mut stack,
                    Node {
                        tag: "#error".into(),
                        text: text.into(),
                        ..Node::default()
                    },
                );
            }
            continue;
        }
        if stack.len() >= 256 {
            push(
                &mut stack,
                Node {
                    tag: "#error".into(),
                    text: text.into(),
                    ..Node::default()
                },
            );
            continue;
        }
        let mut attrs = BTreeMap::new();
        for c in ATTR.captures_iter(content[length..].trim_end_matches('/')) {
            let key = c[1].to_ascii_lowercase();
            let value = decode(
                c.get(2)
                    .or_else(|| c.get(3))
                    .or_else(|| c.get(4))
                    .map_or("", |m| m.as_str()),
            );
            if let Some(previous) = attrs.insert(key.clone(), value) {
                push(
                    &mut stack,
                    Node {
                        tag: "#error".into(),
                        text: format!("duplicate {tag}@{key}: {previous}"),
                        ..Node::default()
                    },
                );
            }
        }
        let node = Node {
            tag: tag.clone(),
            attrs,
            ..Node::default()
        };
        if text.ends_with("/>")
            || [
                "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
                "param", "source", "track", "wbr",
            ]
            .contains(&tag.as_str())
        {
            push(&mut stack, node);
        } else {
            stack.push(node);
        }
    }
    push_text(&mut stack, &html[last..]);
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().unwrap_or_default().children
}

fn push(stack: &mut [Node], node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    }
}
fn push_text(stack: &mut [Node], text: &str) {
    if !text.is_empty() {
        push(
            stack,
            Node {
                text: decode(text),
                ..Node::default()
            },
        );
    }
}
fn close(stack: &mut Vec<Node>) {
    if stack.len() > 1
        && let Some(node) = stack.pop()
    {
        push(stack, node);
    }
}
