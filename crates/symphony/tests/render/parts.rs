//! A long text rendered a part at a time (`Render::typed_reply_part`): every part comes from one
//! parse of the whole text, so the parts in turn render what the whole renders.

use symphony::render::Render;

use super::bare;

const DOC: &str = "docs/workflow/TASKS.md";

/// The HTML of each part of `text`, read in turn from its start with `limit` for the part and for
/// the longest block, and the byte each part began at.
fn parts(render: &Render, text: &str, limit: usize) -> Vec<(usize, String)> {
    let mut parts = Vec::new();
    let mut from = 0;
    while from < text.len() {
        let (fragment, next) = render.typed_reply_part(text, DOC, from, limit, limit);
        assert!(next > from, "the part from byte {from} read nothing");
        parts.push((from, fragment.html));
        from = next;
    }
    parts
}

fn html(parts: &[(usize, String)]) -> Vec<&str> {
    parts.iter().map(|(_, html)| html.as_str()).collect()
}

/// Blocks of every kind, none longer than 200 bytes, with references whose definitions sit in other
/// blocks, fences of both kinds holding what would otherwise be a definition, and a heading that
/// repeats.
const BLOCKS: &str = "\
# Notes

Open [the board][board], then [the agents][agents].

~~~
[board]: /wrong
~~~

[board]: /tasks

```text
[agents]: /wrong
```

> A quote that names [the board][board].

1. one
2. two

| a | b |
|---|---|
| 1 | 2 |

<div class=\"note\">raw HTML</div>

[agents]:
  /agents

# Notes

The end.
";

#[test]
fn a_text_within_the_limit_is_its_whole_reply() {
    let (_tree, render) = bare();
    for text in ["One paragraph, as a reply shows it.", BLOCKS] {
        let (fragment, next) = render.typed_reply_part(text, DOC, 0, text.len(), text.len());
        assert_eq!(fragment, render.typed_reply(text, DOC));
        assert_eq!(next, text.len());
    }
}

#[test]
fn a_text_cut_between_its_blocks_renders_part_after_part_what_the_whole_renders() {
    let (_tree, render) = bare();
    let whole = render.typed_reply(BLOCKS, DOC).html;
    for limit in [60, 100, 200] {
        let parts = parts(&render, BLOCKS, limit);
        assert!(parts.len() > 1, "limit {limit} cuts the text");
        assert_eq!(html(&parts).join("\n"), whole, "limit {limit}");
    }
    let parts = parts(&render, BLOCKS, 60);
    assert!(
        parts
            .iter()
            .any(|(_, html)| html.contains("<h1 id=\"notes_1\">"))
    );
    assert!(!whole.contains("href=\"/wrong\"") && whole.matches("href=\"/tasks\"").count() == 2);
    assert!(whole.contains("href=\"/agents\""));
}

#[test]
fn every_part_begins_where_a_line_of_the_text_begins() {
    let (_tree, render) = bare();
    for (from, _) in parts(&render, BLOCKS, 1) {
        assert!(from == 0 || BLOCKS.as_bytes()[from - 1] == b'\n', "{from}");
    }
}

#[test]
fn a_code_block_longer_than_the_limit_is_cut_between_its_lines() {
    let (_tree, render) = bare();
    let lines: Vec<String> = (0..200).map(|n| format!("line {n:03} of code")).collect();
    for (fence, close, wrapped) in [
        ("```", "```", true),
        ("~~~", "~~~", true),
        ("<pre>", "</pre>", false),
    ] {
        let text = format!("{fence}\n{}\n{close}\n\nAfter.\n", lines.join("\n"));
        let parts = parts(&render, &text, 500);
        assert!(parts.len() > 4, "{fence}");
        let mut shown = String::new();
        for (index, (from, html)) in parts.iter().enumerate() {
            let next = parts.get(index + 1).map_or(text.len(), |(next, _)| *next);
            assert!(
                next - from <= 500,
                "{fence}: a part of {} bytes",
                next - from
            );
            if wrapped {
                let code = usize::from(html.contains("line "));
                assert_eq!(
                    html.matches("<pre").count(),
                    code,
                    "{fence}: one block a part: {html}"
                );
            }
            shown.push_str(html);
        }
        let numbers: Vec<&str> = shown
            .match_indices("line ")
            .filter_map(|(at, _)| shown.get(at + 5..at + 8))
            .collect();
        let expected: Vec<String> = (0..200).map(|n| format!("{n:03}")).collect();
        assert_eq!(numbers, expected, "{fence}: every line once, in order");
    }
}

#[test]
fn a_long_paragraph_is_cut_between_its_lines() {
    let (_tree, render) = bare();
    let text: String = (0..100)
        .map(|n| format!("word {n:03} of a long paragraph\n"))
        .collect();
    let parts = parts(&render, &text, 300);
    assert!(parts.len() > 5);
    for (_, html) in &parts {
        assert!(html.starts_with("<p>") && html.ends_with("</p>"), "{html}");
        assert_eq!(
            html.matches("<p>").count(),
            1,
            "one paragraph a part: {html}"
        );
    }
    let shown: String = html(&parts).concat();
    assert_eq!(shown.matches("word ").count(), 100);
}

#[test]
fn a_long_list_is_cut_between_its_items_and_keeps_its_numbering_and_spacing() {
    let (_tree, render) = bare();
    let tight: String = (1..=60).map(|n| format!("{n}. item {n:02}\n")).collect();
    let cut = parts(&render, &tight, 200);
    assert!(cut.len() > 3);
    for (_, html) in &cut {
        let at = html.find("item ").unwrap() + 5;
        let first: usize = html.get(at..at + 2).unwrap().parse().unwrap();
        let opening = if first == 1 {
            "<ol>".to_owned()
        } else {
            format!("<ol start=\"{first}\">")
        };
        assert!(html.starts_with(&opening), "{html}");
        assert_eq!(html.matches("<ol").count(), 1, "one list a part: {html}");
        assert!(!html.contains("<p>"), "a tight list stays tight: {html}");
    }
    let long_item = format!(
        "- {}\n- last\n",
        (0..40)
            .map(|n| format!("item line {n:02}"))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let cut = parts(&render, &long_item, 120);
    assert!(cut.len() > 3);
    for (_, html) in &cut {
        assert!(
            html.starts_with("<ul>\n<li>") && html.ends_with("</li>\n</ul>"),
            "{html}"
        );
        assert_eq!(html.matches("<ul>").count(), 1, "one list a part: {html}");
    }
    let shown = html(&cut).concat();
    assert_eq!(
        (
            shown.matches("item line").count(),
            shown.matches("last").count()
        ),
        (40, 1)
    );
    let loose: String = (1..=30).map(|n| format!("- item {n:02}\n\n")).collect();
    for (_, html) in &parts(&render, &loose, 100) {
        assert!(
            html.starts_with("<ul>\n<li>\n<p>item"),
            "a loose list stays loose: {html}"
        );
    }
}

#[test]
fn a_line_longer_than_the_limit_is_never_cut() {
    let (_tree, render) = bare();
    let text = format!("{}\n\nAfter.\n", "é".repeat(500));
    let (fragment, next) = render.typed_reply_part(&text, DOC, 0, 100, 100);
    assert_eq!(fragment.html, format!("<p>{}</p>", "é".repeat(500)));
    assert_eq!(text.get(next..), Some("After.\n"));
}

#[test]
fn a_part_from_the_end_is_empty() {
    let (_tree, render) = bare();
    for from in [12, 13, 999] {
        let (fragment, next) = render.typed_reply_part("A brief end.", DOC, from, 100, 100);
        assert_eq!((fragment.html.as_str(), next), ("", 12));
    }
}

#[test]
fn parts_read_with_one_block_size_meet_whatever_each_holds() {
    let (_tree, render) = bare();
    let text: String = (0..100).map(|n| format!("- item {n:03}\n")).collect();
    // The first part holds less than the list and every later one all of it: both cut it alike.
    let (first, mut from) = render.typed_reply_part(&text, DOC, 0, 300, 300);
    let mut shown = first.html;
    while from < text.len() {
        let (part, next) = render.typed_reply_part(&text, DOC, from, 5_000, 300);
        assert!(next > from);
        shown.push_str(&part.html);
        from = next;
    }
    let numbers: Vec<&str> = shown
        .match_indices("item ")
        .filter_map(|(at, _)| shown.get(at + 5..at + 8))
        .collect();
    let expected: Vec<String> = (0..100).map(|n| format!("{n:03}")).collect();
    assert_eq!(numbers, expected, "every item once, in order");
}

/// The text of `html`, its tags and whitespace left out.
fn words(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in html.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag && !c.is_whitespace() => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn a_paragraph_is_cut_at_line_breaks_inside_its_emphasis_and_links() {
    let (_tree, render) = bare();
    let text: String = (0..40)
        .map(|n| format!("**bold {n:03} *and\nem {n:03}* [a link\nto {n:03}](/x)**\n"))
        .collect();
    let whole = render.typed_reply(&text, DOC).html;
    let parts = parts(&render, &text, 200);
    assert!(parts.len() > 5);
    for (index, (from, html)) in parts.iter().enumerate() {
        let next = parts.get(index + 1).map_or(text.len(), |(next, _)| *next);
        assert!(next - from <= 200, "a part of {} bytes", next - from);
        assert_eq!(
            html.matches("<p>").count(),
            1,
            "one paragraph a part: {html}"
        );
        for (open, close) in [
            ("<strong>", "</strong>"),
            ("<em>", "</em>"),
            ("<a ", "</a>"),
        ] {
            assert_eq!(
                html.matches(open).count(),
                html.matches(close).count(),
                "{html}"
            );
        }
    }
    let opens = |start: &str| parts.iter().any(|(_, html)| html.starts_with(start));
    assert!(
        opens("<p><strong><em>em "),
        "a part reopens the emphasis it begins in"
    );
    assert!(
        opens("<p><strong><a href=\"/x\">to "),
        "a part reopens the link it begins in"
    );
    assert_eq!(words(&html(&parts).concat()), words(&whole));
}
