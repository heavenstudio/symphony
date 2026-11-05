//! The `[[…]]` forms of a page outside code: document links and components.

use super::{LineIndex, outside_code};

/// A document link, `[[target#heading|label]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiLink {
    pub line: usize,
    pub whole: String,
    pub target: String,
    pub heading: Option<String>,
    pub label: Option<String>,
}

/// Every `[[target#heading|label]]` outside code, `\[\[([^\]|#]+)(?:#([^\]|]+))?(?:\|([^\]]+))?\]\]`.
/// A target holding `:` is a component or a task, not a document; it is kept, and the caller decides.
pub fn wikilinks(text: &str) -> Vec<WikiLink> {
    let starts = LineIndex::new(text);
    let mut found = Vec::new();
    for (offset, part) in outside_code(text) {
        let mut from = 0;
        while let Some(at) = part.get(from..).and_then(|rest| rest.find("[[")) {
            let open = from + at;
            match part.get(open..).and_then(wikilink_at) {
                Some(WikiParts {
                    whole,
                    target,
                    heading,
                    label,
                }) => {
                    found.push(WikiLink {
                        line: starts.line(offset + open),
                        whole: whole.to_string(),
                        target: target.to_string(),
                        heading: heading.map(str::to_string),
                        label: label.map(str::to_string),
                    });
                    from = open + whole.len();
                }
                None => from = open + 1,
            }
        }
    }
    found
}

/// A wiki link as the text spells it: its whole text, its target, heading and label.
struct WikiParts<'a> {
    whole: &'a str,
    target: &'a str,
    heading: Option<&'a str>,
    label: Option<&'a str>,
}

/// The wiki link `text` starts with (at its `[[`), if it is one.
fn wikilink_at(text: &str) -> Option<WikiParts<'_>> {
    let body = text.get(2..)?;
    let target_len = body.find([']', '|', '#']).unwrap_or(body.len());
    if target_len == 0 {
        return None;
    }
    let (target, mut tail) = body.split_at_checked(target_len)?;
    let mut heading = None;
    if let Some(rest) = tail.strip_prefix('#') {
        let length = rest.find([']', '|']).unwrap_or(rest.len());
        if length > 0 {
            let (named, after) = rest.split_at_checked(length)?;
            heading = Some(named);
            tail = after;
        }
    }
    let mut label = None;
    if let Some(rest) = tail.strip_prefix('|') {
        let length = rest.find(']').unwrap_or(rest.len());
        if length > 0 {
            let (named, after) = rest.split_at_checked(length)?;
            label = Some(named);
            tail = after;
        }
    }
    if !tail.starts_with("]]") {
        return None;
    }
    let whole = text.get(..text.len() - tail.len() + 2)?;
    Some(WikiParts {
        whole,
        target,
        heading,
        label,
    })
}

/// A component, `[[name:argument|option]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub line: usize,
    pub whole: String,
    pub name: String,
    pub arg: String,
    pub option: Option<String>,
}

/// Every `[[name:argument|option]]` outside code,
/// `\[\[([a-z][a-z-]*):([^\]|]*)(?:\|([^\]]*))?\]\]`.
pub fn components(text: &str) -> Vec<Component> {
    let starts = LineIndex::new(text);
    let mut found = Vec::new();
    for (offset, part) in outside_code(text) {
        let mut from = 0;
        while let Some(at) = part.get(from..).and_then(|rest| rest.find("[[")) {
            let open = from + at;
            match part.get(open..).and_then(component_at) {
                Some(ComponentParts {
                    whole,
                    name,
                    arg,
                    option,
                }) => {
                    found.push(Component {
                        line: starts.line(offset + open),
                        whole: whole.to_string(),
                        name: name.to_string(),
                        arg: arg.to_string(),
                        option: option.map(str::to_string),
                    });
                    from = open + whole.len();
                }
                None => from = open + 1,
            }
        }
    }
    found
}

/// A component as the text spells it: its whole text, its name, argument and option.
struct ComponentParts<'a> {
    whole: &'a str,
    name: &'a str,
    arg: &'a str,
    option: Option<&'a str>,
}

/// The component `text` starts with (at its `[[`), if it is one.
fn component_at(text: &str) -> Option<ComponentParts<'_>> {
    let body = text.get(2..)?;
    if !body.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }
    let name_len = body
        .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .unwrap_or(body.len());
    let (name, rest) = body.split_at_checked(name_len)?;
    let rest = rest.strip_prefix(':')?;
    let arg_len = rest.find([']', '|']).unwrap_or(rest.len());
    let (arg, mut tail) = rest.split_at_checked(arg_len)?;
    let mut option = None;
    if let Some(rest) = tail.strip_prefix('|') {
        let length = rest.find(']').unwrap_or(rest.len());
        let (named, after) = rest.split_at_checked(length)?;
        option = Some(named);
        tail = after;
    }
    if !tail.starts_with("]]") {
        return None;
    }
    let whole = text.get(..text.len() - tail.len() + 2)?;
    Some(ComponentParts {
        whole,
        name,
        arg,
        option,
    })
}
