//! Behavior restored after the tooling migration, at the document and typed-content boundaries.

#[path = "common/items.rs"]
mod items;
#[path = "common/tree.rs"]
mod tree;

use serde_json::{Value, json};
use symphony::{blocks, register, render::Render, schema};
use symphony_api::{
    Block, CalloutKind, CardGroup, CardState, ContainerKind, FigureSize, Image, Media,
    Presentation, Span, ThreadAuthor,
};
use tree::Tree;

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn text(value: &str) -> Span {
    Span::Text { text: value.into() }
}

fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        spans: vec![text(value)],
    }
}

fn register(body: &str) -> String {
    format!("# Review\n\n<!-- register:start -->\n### Flight\n{body}\n<!-- register:end -->\n")
}

fn fenced_card(fence: &str, closed: bool) -> String {
    format!(
        "- [ ] **pilot** — Pick a course.\n  [2026-10-05 10:00] agent — Example:\n\n    {fence}text\n    keep this\n\n    - [x] **example** — This is code.\n{}    after the example\n  [2026-10-05 11:00] owner — Keep the example.\n\n- [x] **later** — A later card.\n  [2026-10-05 12:00] owner · approved\n",
        if closed {
            format!("    {fence}\n")
        } else {
            String::new()
        },
    )
}

#[test]
fn a_thread_uses_the_edit_boundary_even_with_indented_or_unclosed_fences() {
    for fence in ["```", "~~~"] {
        for closed in [true, false] {
            let source = register(&fenced_card(fence, closed));
            let lines: Vec<_> = source.lines().collect();
            let cards = register::parse_decisions(&lines);
            assert_eq!(
                cards.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
                ["pilot", "later"]
            );
            let pilot = &cards[0];
            let end = register::card_end(&lines, pilot.line - 1);
            assert_eq!(pilot.raw, lines[pilot.line..end]);
            assert!(pilot.raw.iter().any(|line| line == "    keep this"));
            assert_eq!(pilot.topic, "Flight");
            assert_eq!(pilot.entries().len(), 2);
            assert!(pilot.entries()[0].text.contains("**example**"));
            assert_eq!(pilot.entries()[1].text, "Keep the example.");
            assert!(pilot.open(&symphony::plugins::Plugins::default()));
            let updated = symphony::edit::set_decision(
                &source,
                "pilot",
                "approved",
                "Kept.",
                "2026-10-06 09:00",
                &symphony::plugins::Plugins::default(),
            )
            .unwrap();
            register::check_nothing_lost(&source, &updated, "verdict").unwrap();
            register::check_nothing_repeated(&source, &updated, "verdict").unwrap();
            let updated_cards = register::parse_decisions(&updated.lines().collect::<Vec<_>>());
            let updated_pilot = updated_cards.iter().find(|c| c.slug == "pilot").unwrap();
            assert_eq!(updated_pilot.state, register::State::Approved);
            assert_eq!(updated_pilot.entries().len(), 3);
            assert_eq!(updated_pilot.entries()[2].text, "Kept.");
            assert!(updated.contains("**later**"));
        }
    }
    // Document-level examples still hide apparent cards, then return to real registers.
    let cards = register::parse_decisions(&[
        "```text",
        "- [ ] **fake** — Example",
        "```",
        "- [ ] **real** — Real",
    ]);
    assert_eq!(
        cards.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
        ["real"]
    );
}

#[test]
fn tidy_preserves_the_fenced_thread_and_reaches_the_next_register() {
    let tree = Tree::new();
    let before = register(&fenced_card("```", true));
    tree.write("docs/DECISIONS.md", &before);
    tree.write(
        "docs/z.md",
        &register("- [x] **last** — Done.\n  [2026-10-05 12:00] owner · approved"),
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony::board::tidy(&tree.root, Some("2026-10-06 09:00"), &mut out, &mut err),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert!(err.is_empty());
    let after = tree.read("docs/DECISIONS.md");
    register::check_nothing_lost(&before, &after, "tidy").unwrap();
    register::check_nothing_repeated(&before, &after, "tidy").unwrap();
    assert!(after.contains("    keep this"));
    assert!(after.contains("- [r] **later**"));
    assert!(tree.read("docs/z.md").contains("- [r] **last**"));
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("regrouped docs/z.md")
    );
}

#[test]
fn typed_sections_keep_source_keys_and_detach_the_whole_thread() {
    let tree = Tree::new();
    let renderer = Render::new(&tree.root);
    let source = format!(
        "# Flight\n\n{}\n## Map\n\nA chart.\n",
        fenced_card("```", true)
    );
    let sections = renderer.typed_sections(&source, "docs/x.md");
    assert_eq!(sections.len(), 5);
    assert_eq!(
        blocks::section(&sections[0], "docs/x.md"),
        [Block::Heading {
            level: 1,
            anchor: "flight".into(),
            spans: vec![text("Flight")]
        }]
    );
    assert_eq!((sections[1].start, sections[1].end), (3, 12));
    assert_eq!(sections[1].key, "section:flight:1");
    let card = blocks::section(&sections[1], "docs/x.md");
    let [
        Block::Card {
            slug,
            mark,
            thread,
            line,
            line_offset,
            verdict,
            ..
        },
    ] = card.as_slice()
    else {
        panic!("{card:?}")
    };
    assert_eq!(slug, "pilot");
    assert_eq!(*mark, CardState::Pending);
    assert!(thread.is_empty());
    assert_eq!((*line, *line_offset), (None, Some(0)));
    assert!(verdict.open);
    assert_eq!(verdict.group, CardGroup::Agent);
    let attached = renderer.sections(&source, "docs/x.md");
    let attached_card = blocks::section(&attached[1], "docs/x.md");
    let [
        Block::Card {
            thread,
            line,
            line_offset,
            ..
        },
    ] = attached_card.as_slice()
    else {
        panic!("{attached_card:?}")
    };
    assert_eq!((*line, *line_offset), (Some(3), None));
    assert_eq!(thread.len(), 2);
    assert_eq!(thread[1].who, ThreadAuthor::Owner);
    assert_eq!(thread[1].blocks, [paragraph("Keep the example.")]);
    let history = renderer.typed_history(&source, "docs/x.md");
    let entries = blocks::fragment(&history["pilot"], "docs/x.md");
    assert_eq!(entries.len(), 2);
    let Block::Thread { entries: first } = &entries[0] else {
        panic!("{entries:?}")
    };
    assert_eq!(first[0].who, ThreadAuthor::Agent);
    assert_eq!(first[0].timestamp.as_deref(), Some("2026-10-05 10:00"));
    assert!(first[0].blocks.iter().any(|b| matches!(b, Block::Code { language, text } if language == "text" && text.contains("**example**"))));
    let Block::Thread { entries: last } = &entries[1] else {
        panic!("{entries:?}")
    };
    assert_eq!(last[0].who, ThreadAuthor::Owner);
    assert_eq!(last[0].blocks, [paragraph("Keep the example.")]);
    let moved = renderer.typed_sections(&format!("\n\n{source}"), "docs/x.md");
    assert_eq!(moved[1].start, sections[1].start + 2);
    assert_eq!(moved[1].hash, sections[1].hash);
    let edited = renderer.typed_sections(&source.replace("A chart.", "A new chart."), "docs/x.md");
    assert_eq!(edited[4].key, sections[4].key);
    assert_ne!(edited[4].hash, sections[4].hash);
    assert_eq!(edited[1].hash, sections[1].hash);
}

#[test]
fn a_persons_line_names_them_first_of_its_blocks_and_the_owners_and_an_agents_name_none() {
    let tree = Tree::new();
    let renderer = Render::new(&tree.root);
    let source = "# D\n\n- [x] **pilot** — Choose.\n  [2026-11-23 09:00] agent — Asked.\n  [2026-11-23 10:00] ana · approved — A\n  [2026-11-23 11:00] owner · approved — B\n";
    let history = renderer.typed_history(source, "docs/x.md");
    let entries: Vec<_> = blocks::fragment(&history["pilot"], "docs/x.md")
        .into_iter()
        .flat_map(|block| match block {
            Block::Thread { entries } => entries,
            other => panic!("{other:?}"),
        })
        .collect();
    let read: Vec<_> = (entries.iter())
        .map(|e| (e.who, e.verdict.as_deref(), e.blocks.clone()))
        .collect();
    let named = Block::Paragraph {
        spans: vec![Span::Styled {
            tag: symphony_api::InlineKind::Span,
            presentation: Presentation::class("entry-by"),
            spans: vec![text("ana")],
        }],
    };
    assert_eq!(
        read,
        [
            (ThreadAuthor::Agent, None, vec![paragraph("Asked.")]),
            (
                ThreadAuthor::Owner,
                Some("approved"),
                vec![named, paragraph("A")]
            ),
            (ThreadAuthor::Owner, Some("approved"), vec![paragraph("B")]),
        ]
    );
}

#[test]
fn typed_prose_retains_inline_structure_tables_code_and_callouts() {
    let tree = Tree::new();
    let renderer = Render::new(&tree.root);
    let fragment = renderer.typed_reply("**Bold** and *soft* with `x < y`.\n\n3. third\n4. fourth\n\n| Key | Value |\n|---|---|\n| a | **b** |\n\n```rs\nx < y && z\n```\n\n> quote\n\n---", "docs/x.md");
    let typed = blocks::fragment(&fragment, "docs/x.md");
    assert_eq!(
        typed[0],
        Block::Paragraph {
            spans: vec![
                Span::Strong {
                    spans: vec![text("Bold")]
                },
                text(" and "),
                Span::Emphasis {
                    spans: vec![text("soft")]
                },
                text(" with "),
                Span::Code {
                    text: "x < y".into()
                },
                text(".")
            ]
        }
    );
    assert_eq!(
        typed[1],
        Block::List {
            ordered: true,
            start: 3,
            items: vec![vec![paragraph("third")], vec![paragraph("fourth")]]
        }
    );
    let Block::Table { head, rows, .. } = &typed[2] else {
        panic!("{typed:?}")
    };
    assert_eq!(head.len(), 2);
    assert!(head.iter().all(|c| c.header));
    assert_eq!(head[0].spans, [text("Key")]);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][1].spans,
        [Span::Strong {
            spans: vec![text("b")]
        }]
    );
    assert_eq!(
        typed[3],
        Block::Code {
            language: "rs".into(),
            text: "x < y && z\n".into()
        }
    );
    assert_eq!(
        typed[4],
        Block::Quote {
            blocks: vec![paragraph("quote")]
        }
    );
    assert_eq!(typed[5], Block::Rule);
    let sections = renderer.typed_sections(
        "> [!question] Why?\n> Body.\n> > [!resolved] Answer.\n",
        "docs/x.md",
    );
    let callout = blocks::section(&sections[0], "docs/x.md");
    let [
        Block::Callout {
            kind,
            title,
            resolved,
            blocks: body,
        },
    ] = callout.as_slice()
    else {
        panic!("{callout:?}")
    };
    assert_eq!(*kind, CalloutKind::Question);
    assert_eq!(title.len(), 2);
    assert!(
        matches!(&title[0], Span::Styled { tag: symphony_api::InlineKind::Span, presentation, spans } if presentation.class == "callout-kind" && spans == &[text("question")])
    );
    assert_eq!(title[1], text(" Why?"));
    assert!(*resolved);
    assert_eq!(body.len(), 2);
    assert_eq!(body[0], paragraph("Body."));
    assert!(
        matches!(&body[1], Block::Callout { kind: CalloutKind::Resolved, title, blocks, .. } if title.last() == Some(&text(" Answer.")) && blocks.is_empty())
    );
}

#[test]
fn typed_links_components_and_media_retain_their_identity_and_fragment_index() {
    let tree = Tree::new();
    tree.write("docs/NAV.md", "# Navigation\n\n## Course\n");
    let renderer = Render::new(&tree.root).with_plugins(items::plugins());
    let source = "[[NAV#Course]] and [[task:sim/course|Set course]]\n\n![model](/artifacts/a.item)\n\n![mockup](/artifacts/b.html)\n";
    let sections = renderer.typed_sections(source, "docs/x.md");
    let first = blocks::section(&sections[0], "docs/x.md");
    let [Block::Paragraph { spans }] = first.as_slice() else {
        panic!("{first:?}")
    };
    assert!(
        matches!(&spans[0], Span::WikiLink { target: Some(target), .. } if target.path == "docs/NAV.md" && target.anchor.as_deref() == Some("course"))
    );
    assert!(
        matches!(&spans[2], Span::Component { kind, blocks } if kind == "task" && format!("{blocks:?}").contains("/task/sim/course") && format!("{blocks:?}").contains("Set course"))
    );
    assert_eq!(
        blocks::section(&sections[1], "docs/x.md"),
        [Block::Media {
            media: Media::Viewer {
                src: "/artifacts/a.item".into(),
                label: "model".into(),
                poster: None,
                viewer: items::viewer(),
            }
        }]
    );
    assert_eq!(
        blocks::section(&sections[2], "docs/x.md"),
        [Block::Media {
            media: Media::Mockup {
                src: "/artifacts/b.html".into(),
                label: "mockup".into()
            }
        }]
    );
    assert_eq!((sections[1].media.len(), sections[2].media.len()), (1, 1));
    let changed = renderer.typed_sections(&source.replace("a.item", "c.item"), "docs/x.md");
    assert_ne!(changed[1].hash, sections[1].hash);
    assert_eq!(changed[2].hash, sections[2].hash);
}

#[test]
fn the_fragment_reader_preserves_entities_attributes_and_unsupported_content() {
    let typed = blocks::parse(
        "<details open><summary>A &amp; B</summary><p title='x > y'>one&nbsp;&#x3b1;<br>two</p></details><!--kept--><widget>visible</widget>",
        &[],
        "docs/x.md",
    );
    let Block::Details {
        summary,
        open,
        blocks: body,
    } = &typed[0]
    else {
        panic!("{typed:?}")
    };
    assert_eq!(summary, &[text("A & B")]);
    assert!(*open);
    let Block::Container {
        presentation,
        blocks,
        ..
    } = &body[0]
    else {
        panic!("{body:?}")
    };
    assert_eq!(presentation.title, "x > y");
    assert_eq!(
        blocks,
        &[Block::Paragraph {
            spans: vec![text("one\u{a0}α"), Span::LineBreak, text("two")]
        }]
    );
    assert_eq!(
        typed[1],
        Block::Comment {
            text: "kept".into()
        }
    );
    // An element it cannot draw is named alone where it stands, and what it holds is drawn once,
    // after it, never inside the notice as well.
    assert_eq!(
        typed[2..],
        [
            Block::Unknown {
                tag: "widget".into(),
                text: String::new()
            },
            Block::Group {
                blocks: vec![paragraph("visible")]
            }
        ]
    );
    // In a line too: an element no inline kind names is read as blocks there.
    assert_eq!(
        blocks::parse("<p>a <blink>old</blink> b</p>", &[], "docs/x.md"),
        [Block::Paragraph {
            spans: vec![
                text("a "),
                Span::Blocks {
                    blocks: vec![
                        Block::Unknown {
                            tag: "blink".into(),
                            text: String::new()
                        },
                        Block::Group {
                            blocks: vec![paragraph("old")]
                        }
                    ]
                },
                text(" b"),
            ]
        }]
    );
    let unsupported = blocks::parse(
        "<p surprise='yes'>retained</p><span class='media' data-media='9'></span>",
        &[],
        "docs/x.md",
    );
    assert!(
        unsupported
            .iter()
            .any(|b| matches!(b, Block::Unknown { tag, .. } if tag == "p@surprise"))
    );
    assert!(unsupported.iter().any(|b| matches!(b, Block::Paragraph { spans } if spans.iter().any(|s| matches!(s, Span::Unknown { tag, text } if tag == "media-index" && text == "9")))));
}

#[test]
fn a_typed_component_keeps_its_kind_and_display_content_whatever_its_name() {
    for name in [
        "lamp",
        "shape-matrix",
        "relationships",
        "a-plugin-component",
    ] {
        let html =
            format!("<symphony-component data-kind='{name}'><p>Displayed</p></symphony-component>");
        assert_eq!(
            blocks::parse(&html, &[], "docs/x.md"),
            [Block::Component {
                kind: name.into(),
                blocks: vec![paragraph("Displayed")]
            }]
        );
        let inline = blocks::parse(&format!("<p>a {html} b</p>"), &[], "docs/x.md");
        let [Block::Paragraph { spans }] = inline.as_slice() else {
            panic!("{inline:?}")
        };
        assert_eq!(
            spans[1],
            Span::Component {
                kind: name.into(),
                blocks: vec![paragraph("Displayed")]
            }
        );
    }
}

#[test]
fn semantic_groups_keep_their_content_and_preserve_extra_presentation() {
    assert_eq!(
        blocks::parse(
            "<div class='compare'><p>content</p></div>",
            &[],
            "docs/x.md"
        ),
        [Block::Comparison {
            figures: vec![paragraph("content")],
        }]
    );
    // An ID cannot disappear during semantic promotion: navigation still needs that anchor. A
    // plugin's class is no wire variant: it stays a container a stylesheet selects.
    for (class, id) in [
        ("compare", "anchor"),
        ("pair", ""),
        ("matrix-scale", ""),
        ("workshop-lamps muted", ""),
    ] {
        let parsed = blocks::parse(
            &format!("<div class='{class}' id='{id}'><p>content</p></div>"),
            &[],
            "docs/x.md",
        );
        assert!(
            matches!(&parsed[0], Block::Container { tag: ContainerKind::Div, presentation, blocks } if presentation.id == id && presentation.class == class && blocks == &[paragraph("content")]),
            "{parsed:?}"
        );
    }
    let figure = blocks::parse(
        "<figure data-figure='wide' data-badge='coming'><a href='/docs/A.md'><img src='/a.png' alt='A' loading='lazy'></a><figcaption>caption</figcaption></figure>",
        &[],
        "docs/x.md",
    );
    assert_eq!(
        figure,
        [Block::Figure {
            picture: Some(Image {
                src: "/a.png".into(),
                alt: "A".into(),
                presentation: Presentation {
                    attributes: [("loading".to_string(), "lazy".to_string())].into(),
                    ..Presentation::default()
                },
            }),
            caption: vec![text("caption")],
            href: Some("/docs/A.md".into()),
            badge: Some("coming".into()),
            size: FigureSize::Wide,
        }]
    );
    // A size the vocabulary does not name is reported, and its content kept.
    let unsized_figure = blocks::parse(
        "<figure data-figure='enormous'><p>picture</p></figure>",
        &[],
        "docs/x.md",
    );
    assert!(
        matches!(&unsized_figure[0], Block::Unknown { tag, text } if tag == "figure-size" && text == "enormous"),
        "{unsized_figure:?}"
    );
    let settled = blocks::parse(
        "<div class='settled'><details open><summary>Done</summary><p>history</p></details></div>",
        &[],
        "docs/x.md",
    );
    assert_eq!(
        settled,
        [Block::SettledCard {
            summary: vec![text("Done")],
            open: true,
            blocks: vec![paragraph("history")]
        }]
    );
    let parsed = blocks::parse(
        "<table class='matrix shape-matrix'><tr><td>cell</td></tr></table>",
        &[],
        "docs/x.md",
    );
    let [
        Block::Table {
            head,
            rows,
            presentation,
        },
    ] = parsed.as_slice()
    else {
        panic!("{parsed:?}")
    };
    assert_eq!(presentation.class, "matrix shape-matrix");
    assert!(head.is_empty());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[0][0].spans, [text("cell")]);
}

#[test]
fn graphics_and_malformed_input_retain_geometry_text_and_diagnostics() {
    let parsed = blocks::parse(
        "<svg viewBox='0 0 10 10' aria-label='Diagram'><path d='M0 0L1 1'/><circle cx='2' cy='3' r='1'/><ellipse/><rect/><line/><polyline/><polygon/></svg>",
        &[],
        "docs/x.md",
    );
    let [Block::Paragraph { spans }] = parsed.as_slice() else {
        panic!("{parsed:?}")
    };
    let [
        Span::Graphic {
            view_box,
            presentation,
            shapes,
        },
    ] = spans.as_slice()
    else {
        panic!("{spans:?}")
    };
    assert_eq!(view_box, "0 0 10 10");
    assert_eq!(presentation.attributes["aria-label"], "Diagram");
    use symphony_api::VectorKind::{Circle, Ellipse, Line, Path, Polygon, Polyline, Rect};
    assert_eq!(
        shapes.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [Path, Circle, Ellipse, Rect, Line, Polyline, Polygon]
    );
    assert_eq!(shapes[0].attributes["d"], "M0 0L1 1");
    assert_eq!(shapes[1].attributes["r"], "1");
    let malformed = blocks::parse("<p id='first' id='last'>kept</p></stray>", &[], "docs/x.md");
    assert!(malformed.iter().any(|b| matches!(b, Block::Unknown { tag, text } if tag == "malformed" && text == "duplicate p@id: first")));
    assert!(malformed.iter().any(
        |b| matches!(b, Block::Unknown { tag, text } if tag == "malformed" && text == "</stray>")
    ));
    assert!(malformed.iter().any(|b| matches!(b, Block::Container { presentation, blocks, .. } if presentation.id == "last" && blocks == &[paragraph("kept")])));
}

#[test]
fn schema_diagnostics_follow_file_order_including_duplicate_keys_and_cached_schemas() {
    let instance = parsed(r#"{"z": {"b": 1, "a": 2, "b": 3}, "a": 4}"#);
    let schema = parsed(
        r#"{"properties":{"z":{"additionalProperties":{"type":"string"}},"a":{"type":"string"}}}"#,
    );
    assert_eq!(
        schema::validate(&instance, &schema).unwrap(),
        [
            "$.z.b: expected string, got int",
            "$.z.a: expected string, got int",
            "$.a: expected string, got int"
        ]
    );
    assert_eq!(schema::repr(&instance), "{'z': {'b': 3, 'a': 2}, 'a': 4}");
    for (raw, expected) in [
        (
            r#"{"dependentRequired":{"z":["second","first"],"a":["last"]}}"#,
            vec![
                "$: 'z' requires 'second'",
                "$: 'z' requires 'first'",
                "$: 'a' requires 'last'",
            ],
        ),
        (
            r#"{"dependentRequired":{"a":["last"],"z":["first","second"]}}"#,
            vec![
                "$: 'a' requires 'last'",
                "$: 'z' requires 'first'",
                "$: 'z' requires 'second'",
            ],
        ),
    ] {
        assert_eq!(schema::validate(&instance, &parsed(raw)).unwrap(), expected);
    }
}

#[test]
fn integer_tokens_preserve_precision_and_negative_zero_keeps_its_integer_type() {
    let schema = schema::Schema::compile(&json!({"type":"integer"})).unwrap();
    for token in [
        "-0",
        "18446744073709551616",
        "-18446744073709551617",
        "9007199254740993",
    ] {
        assert_eq!(
            schema.errors(&parsed(token)),
            Vec::<String>::new(),
            "{token}"
        );
    }
    for token in ["-0.0", "0e0", "9007199254740993.0"] {
        assert_eq!(
            schema.errors(&parsed(token)),
            ["$: expected integer, got float"],
            "{token}"
        );
    }
    let large = parsed("18446744073709551616");
    assert_eq!(
        schema::validate(&large, &parsed(r#"{"const":18446744073709551617}"#)).unwrap(),
        ["$: must be 18446744073709551617"]
    );
    assert_eq!(
        schema::validate(&large, &parsed(r#"{"minimum":18446744073709551617}"#)).unwrap(),
        ["$: must be at least 18446744073709551617"]
    );
    assert_eq!(
        schema::validate(&large, &json!({"enum":[9007199254740992u64]})).unwrap(),
        ["$: 18446744073709551616 is not one of 9007199254740992"]
    );
}

#[test]
fn unenforced_keywords_and_formats_are_refused_even_in_unused_subschemas() {
    for keyword in [
        "contentEncoding",
        "contentMediaType",
        "contentSchema",
        "$recursiveRef",
    ] {
        let value = match keyword {
            "contentSchema" => json!({}),
            _ => json!("unused"),
        };
        let rule = json!({"$defs":{"unused":{keyword:value}}});
        let error = schema::Schema::compile(&rule).err().unwrap().0;
        assert_eq!(
            error,
            format!("unsupported keyword(s): {keyword} (at $.$defs.unused)")
        );
    }
    for raw in [
        r#"{"$defs":{"unused":{"format":"date-ish"}}}"#,
        r#"{"allOf":[{"items":{"format":"date-ish"}}]}"#,
        r#"{"if":{"format":"date-ish"}}"#,
    ] {
        let error = schema::Schema::compile(&parsed(raw)).err().unwrap().0;
        assert!(
            error.starts_with("unsupported format 'date-ish' (at $.") && error.ends_with(')'),
            "{error}"
        );
    }
    // Keyword-shaped names in const/enum are data, not schemas.
    let data = json!({"contentEncoding":"anything", "format":"date-ish"});
    assert!(
        schema::validate(&data, &json!({"const":data, "enum":[data]}))
            .unwrap()
            .is_empty()
    );
    let date = schema::Schema::compile(&json!({"type":"string", "format":"date"})).unwrap();
    assert!(date.errors(&json!("2024-02-29")).is_empty());
    assert_eq!(date.errors(&json!("2025-02-29")).len(), 1);
}

#[test]
fn wrong_type_suppresses_only_its_own_subtree_through_escaped_references() {
    let rule = parsed(
        r##"{"$defs":{"row/~":{"type":"array","const":[],"items":{"type":"integer"}}},"properties":{"bad":{"$ref":"#/$defs/row~1~0"},"sibling":{"type":"integer"}}}"##,
    );
    assert_eq!(
        schema::validate(&json!({"bad":{"x":"wrong"}, "sibling":"also wrong"}), &rule).unwrap(),
        [
            "$.bad: expected array, got dict",
            "$.sibling: expected integer, got str"
        ]
    );
    assert_eq!(
        schema::validate(&json!({"bad":["wrong"], "sibling":2}), &rule).unwrap(),
        ["$.bad: must be []", "$.bad[0]: expected integer, got str"]
    );
    let union = json!({"type":["array", "null"], "const":[], "items":{"type":"string"}});
    assert_eq!(
        schema::validate(&json!(false), &union).unwrap(),
        ["$: expected ['array', 'null'], got bool"]
    );
}

/// Every generic shape a plugin renders to, as the server writes it into a section's HTML and
/// reads it back: the same blocks, whatever plugin made them.
#[test]
fn the_generic_blocks_and_spans_write_and_read_back_as_themselves() {
    use symphony_api::{Cell, GalleryItem, VectorKind, VectorShape};
    let lumen = Image {
        src: "/x/k.png".into(),
        alt: "Lumen".into(),
        presentation: Presentation::default(),
    };
    let figure =
        |picture: Option<Image>, href: Option<&str>, badge: Option<&str>, size| Block::Figure {
            picture,
            caption: vec![text("NOR-L01 Lumen")],
            href: href.map(str::to_string),
            badge: badge.map(str::to_string),
            size,
        };
    let icon = |named: bool, tip: &str| Span::Icon {
        shape: vec![VectorShape {
            kind: VectorKind::Path,
            attributes: [("d".to_string(), "M4 4L20 20".to_string())].into(),
        }],
        label: "Cloning bay".into(),
        named,
        tip: tip.into(),
    };
    let cell = |header: bool, spans: Vec<Span>| Cell {
        header,
        spans,
        presentation: Presentation::default(),
    };
    let original = vec![
        figure(
            Some(lumen.clone()),
            Some("/docs/lamps/north.md#nor-l01"),
            None,
            FigureSize::Row,
        ),
        figure(None, None, Some("coming"), FigureSize::Tile),
        figure(Some(lumen.clone()), None, None, FigureSize::Wide),
        Block::Component {
            kind: "lamp-matrix".into(),
            blocks: vec![paragraph("inside")],
        },
        Block::Paragraph {
            spans: vec![
                text("see "),
                Span::Component {
                    kind: "task".into(),
                    blocks: vec![paragraph("sim/jump")],
                },
                text(" and "),
                icon(true, "Cloning bay: brings a lost crew back"),
                icon(false, ""),
            ],
        },
        Block::Table {
            head: vec![cell(true, vec![text("Lamp")])],
            rows: vec![vec![cell(
                false,
                vec![Span::Blocks {
                    blocks: vec![figure(Some(lumen), None, Some("concept"), FigureSize::Cell)],
                }],
            )]],
            presentation: Presentation::class("matrix shape-matrix"),
        },
        Block::Media {
            media: Media::Viewer {
                src: "/x/lamps/NOR-L01.item".into(),
                label: "NOR-L01".into(),
                poster: Some("/x/lamps/NOR-L01.png".into()),
                viewer: items::viewer(),
            },
        },
        // A paragraph of one media span reads as that media, so a component's media goes in it.
        Block::Component {
            kind: "lamp-gallery".into(),
            blocks: vec![Block::Media {
                media: Media::Gallery {
                    items: vec![
                        GalleryItem {
                            name: "NOR-L01".into(),
                            src: "/x/lamps/NOR-L01.item".into(),
                            poster: Some("/x/lamps/NOR-L01.png".into()),
                        },
                        GalleryItem {
                            name: "SOU-L03".into(),
                            src: "/x/lamps/SOU-L03.item".into(),
                            poster: None,
                        },
                    ],
                    viewer: items::viewer(),
                },
            }],
        },
    ];
    let mut media = Vec::new();
    let html = blocks::write::html(&original, &mut |item| {
        media.push(item);
        media.len() - 1
    });
    assert_eq!(
        blocks::parse(&html, &media, "docs/x.md"),
        original,
        "{html}"
    );
    let wire = serde_json::to_string(&original).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<Block>>(&wire).unwrap(),
        original,
        "{wire}"
    );
    assert!(
        !wire.contains("\"tip\":\"\""),
        "an icon without a tip sends none: {wire}"
    );
}

/// The prose a component may return, written into its section's HTML and read back: headings,
/// lists, code, quotes, rules, containers, details, settled cards, callouts and comments, with
/// every inline kind.
#[test]
fn a_components_prose_writes_and_reads_back_as_itself() {
    use symphony_api::{InlineKind, VectorKind, VectorShape};
    let original = vec![
        Block::Heading {
            level: 3,
            anchor: "the-fleet".into(),
            spans: vec![text("The fleet")],
        },
        Block::Paragraph {
            spans: vec![
                Span::Emphasis {
                    spans: vec![text("soft")],
                },
                text(" "),
                Span::Strong {
                    spans: vec![text("loud")],
                },
                text(" "),
                Span::Code {
                    text: "a < b".into(),
                },
                Span::LineBreak,
                Span::Styled {
                    tag: InlineKind::Small,
                    presentation: Presentation::class("muted"),
                    spans: vec![text("aside")],
                },
                Span::Graphic {
                    view_box: "0 0 24 24".into(),
                    presentation: Presentation::default(),
                    shapes: vec![VectorShape {
                        kind: VectorKind::Circle,
                        attributes: [
                            ("cx".to_string(), "12".to_string()),
                            ("cy".to_string(), "12".to_string()),
                            ("r".to_string(), "4".to_string()),
                        ]
                        .into(),
                    }],
                },
            ],
        },
        Block::List {
            ordered: true,
            start: 3,
            items: vec![vec![paragraph("third")], vec![paragraph("fourth")]],
        },
        Block::List {
            ordered: false,
            start: 1,
            items: vec![vec![paragraph("any")]],
        },
        Block::Code {
            language: "rust".into(),
            text: "let a = 1 < 2;".into(),
        },
        Block::Code {
            language: String::new(),
            text: "plain".into(),
        },
        Block::Quote {
            blocks: vec![paragraph("quoted")],
        },
        Block::Rule,
        Block::Container {
            tag: ContainerKind::Section,
            presentation: Presentation::class("lamp-strip"),
            blocks: vec![paragraph("held")],
        },
        Block::Details {
            summary: vec![text("More")],
            open: true,
            blocks: vec![paragraph("hidden")],
        },
        Block::SettledCard {
            summary: vec![text("Settled")],
            open: false,
            blocks: vec![paragraph("done")],
        },
        Block::Callout {
            kind: CalloutKind::Question,
            title: vec![text("Why?")],
            resolved: false,
            blocks: vec![paragraph("Because.")],
        },
        Block::Callout {
            kind: CalloutKind::Decision,
            title: vec![text("Done")],
            resolved: true,
            blocks: vec![paragraph("Decided.")],
        },
        Block::Comment {
            text: " a note ".into(),
        },
    ];
    let html = blocks::write::html(&original, &mut |_| unreachable!("no media"));
    assert_eq!(blocks::parse(&html, &[], "docs/x.md"), original, "{html}");
}

#[test]
fn history_maps_keep_lexical_keys_and_the_last_duplicate_fragment() {
    let tree = Tree::new();
    let renderer = Render::new(&tree.root);
    let source = "- [ ] **z** — Last key.\n  [2026-10-20 10:00] agent — Zed.\n- [ ] **a** — First copy.\n  [2026-10-20 10:00] agent — Older.\n- [ ] **a** — Last copy.\n  [2026-10-20 11:00] agent — Newer.\n";
    for history in [
        renderer.history(source, "docs/A.md"),
        renderer.typed_history(source, "docs/A.md"),
    ] {
        assert_eq!(
            history.keys().map(String::as_str).collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(history["a"].html.contains("Newer."));
        assert!(!history["a"].html.contains("Older."));
        assert!(history["a"].media.is_empty());
    }
    assert!(renderer.history("", "docs/A.md").is_empty());
}

#[test]
fn graphic_attribute_maps_keep_allowed_values_and_report_rejected_ones() {
    use std::collections::BTreeMap;
    let parsed = blocks::parse(
        "<svg id='drawing' class='chart' title='Plot' viewBox='0 0 10 10' width='10' fill='none' aria-label='shown' onclick='bad-svg'><path d='M0 0L1 1' id='path-one' class='accent' title='Shape' stroke='red' onclick='bad-path'/><circle/></svg>",
        &[],
        "docs/x.md",
    );
    let [
        Block::Paragraph { spans },
        Block::Unknown {
            tag: bad_svg,
            text: svg_value,
        },
        Block::Unknown {
            tag: bad_path,
            text: path_value,
        },
    ] = parsed.as_slice()
    else {
        panic!("{parsed:?}")
    };
    assert_eq!(
        (bad_svg.as_str(), svg_value.as_str()),
        ("svg@onclick", "bad-svg")
    );
    assert_eq!(
        (bad_path.as_str(), path_value.as_str()),
        ("path@onclick", "bad-path")
    );
    let [
        Span::Graphic {
            view_box,
            presentation,
            shapes,
        },
    ] = spans.as_slice()
    else {
        panic!("{spans:?}")
    };
    assert_eq!(view_box, "0 0 10 10");
    assert_eq!(
        presentation,
        &Presentation {
            class: "chart".into(),
            id: "drawing".into(),
            title: "Plot".into(),
            attributes: BTreeMap::from([
                ("aria-label".into(), "shown".into()),
                ("fill".into(), "none".into()),
                ("width".into(), "10".into())
            ])
        }
    );
    assert_eq!(shapes.len(), 2);
    assert_eq!(
        shapes[0].attributes,
        BTreeMap::from([
            ("class".into(), "accent".into()),
            ("d".into(), "M0 0L1 1".into()),
            ("id".into(), "path-one".into()),
            ("stroke".into(), "red".into()),
            ("title".into(), "Shape".into())
        ])
    );
    assert!(shapes[1].attributes.is_empty());
    let encoded = serde_json::to_value(&parsed).unwrap();
    assert_eq!(
        serde_json::from_value::<Vec<Block>>(encoded).unwrap(),
        parsed
    );
}
