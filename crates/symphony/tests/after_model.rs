//! Behavior restored after the tooling migration, at the document and typed-content boundaries.

#[path = "common/tree.rs"]
mod tree;

use serde_json::{Value, json};
use symphony::render::{Builtin, Render};
use symphony::{blocks, register, schema};
use symphony_api::{
    Block, CalloutKind, CardGroup, CardState, LiveComponent, Media, Span, ThreadAuthor,
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
            assert!(pilot.open());
            let updated = symphony::edit::set_decision(
                &source,
                "pilot",
                "approved",
                "Kept.",
                "2026-10-06 09:00",
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
    let renderer = Render::new(&tree.root).with_components(Box::new(Builtin));
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
fn typed_prose_retains_inline_structure_tables_code_and_callouts() {
    let tree = Tree::new();
    let renderer = Render::new(&tree.root).with_components(Box::new(Builtin));
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
    let Block::Table { head, rows } = &typed[2] else {
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
    let renderer = Render::new(&tree.root).with_components(Box::new(Builtin));
    let source = "[[NAV#Course]] and [[task:sim/course|Set course]]\n\n![model](/artifacts/a.glb)\n\n![mockup](/artifacts/b.html)\n";
    let sections = renderer.typed_sections(source, "docs/x.md");
    let first = blocks::section(&sections[0], "docs/x.md");
    let [Block::Paragraph { spans }] = first.as_slice() else {
        panic!("{first:?}")
    };
    assert!(
        matches!(&spans[0], Span::WikiLink { target: Some(target), .. } if target.path == "docs/NAV.md" && target.anchor.as_deref() == Some("course"))
    );
    assert!(
        matches!(&spans[2], Span::Component { component: LiveComponent::Task { slug, label: Some(label), blocks } } if slug == "sim/course" && label == "Set course" && !blocks.is_empty())
    );
    assert_eq!(
        blocks::section(&sections[1], "docs/x.md"),
        [Block::Media {
            media: Media::Model {
                src: "/artifacts/a.glb".into(),
                label: "model".into()
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
    assert_eq!(sections[1].media.len(), 1);
    assert_eq!(sections[2].media.len(), 1);
    let changed = renderer.typed_sections(
        &source.replace("/artifacts/a.glb", "/artifacts/c.glb"),
        "docs/x.md",
    );
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
    assert_eq!(
        typed[2],
        Block::Unknown {
            tag: "widget".into(),
            text: "visible".into()
        }
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
fn typed_component_selectors_keep_options_and_their_display_content() {
    for (name, arg, option, expected) in [
        (
            "ship",
            "REP-F01",
            "large",
            json!({"kind":"ship","id":"REP-F01","size":"large"}),
        ),
        (
            "ships",
            " REP-F01, ,EMP-F01 ",
            "",
            json!({"kind":"ships","ids":["REP-F01","EMP-F01"],"size":"row"}),
        ),
        (
            "art",
            "Republic · ship",
            "",
            json!({"kind":"art","slot":"Republic · ship"}),
        ),
        (
            "ship-matrix",
            "republic",
            "",
            json!({"kind":"ship-matrix","faction":"republic"}),
        ),
        (
            "shape-matrix",
            "all",
            "CLASS",
            json!({"kind":"shape-matrix","faction":"all","pivot":"class"}),
        ),
        (
            "subfaction-ships",
            "Home Fleet",
            "",
            json!({"kind":"subfaction-ships","name":"Home Fleet","size":"small"}),
        ),
        (
            "relationships",
            "destroy",
            "",
            json!({"kind":"relationships","table":"destroy"}),
        ),
        (
            "capability",
            "jump",
            "name",
            json!({"kind":"capability","id":"jump","named":true}),
        ),
        (
            "weapon",
            "laser",
            "",
            json!({"kind":"weapon","id":"laser","named":false}),
        ),
        (
            "gear",
            "shield",
            "name",
            json!({"kind":"gear","id":"shield","named":true}),
        ),
        (
            "status",
            "burning",
            "",
            json!({"kind":"status","id":"burning","named":false}),
        ),
        (
            "defence",
            "armour",
            "name",
            json!({"kind":"defence","id":"armour","named":true}),
        ),
    ] {
        let html = format!(
            "<sf-component data-kind='{name}' data-arg='{arg}' data-option='{option}'><p>Displayed</p></sf-component>"
        );
        let parsed = blocks::parse(&html, &[], "docs/x.md");
        let [Block::Component { component }] = parsed.as_slice() else {
            panic!("{parsed:?}")
        };
        assert_eq!(component.blocks(), &[paragraph("Displayed")]);
        let mut selector = serde_json::to_value(component).unwrap();
        selector.as_object_mut().unwrap().remove("blocks");
        assert_eq!(selector, expected);
    }
    for (name, arg, option) in [
        ("ship", "REP-F01", "enormous"),
        ("shape-matrix", "all", "typo"),
        ("relationships", "typo", ""),
        ("unknown", "x", ""),
    ] {
        let html = format!(
            "<sf-component data-kind='{name}' data-arg='{arg}' data-option='{option}'><p>Retained</p></sf-component>"
        );
        let parsed = blocks::parse(&html, &[], "docs/x.md");
        assert_eq!(
            parsed,
            [
                Block::Unknown {
                    tag: format!("component:{name}"),
                    text: "Retained".into()
                },
                Block::Group {
                    blocks: vec![paragraph("Retained")]
                }
            ]
        );
    }
}

#[test]
fn semantic_groups_keep_their_content_and_preserve_extra_presentation() {
    for (class, expected) in [
        (
            "pair",
            Block::ArtSlot {
                figures: vec![paragraph("content")],
            },
        ),
        (
            "hull",
            Block::Hull {
                blocks: vec![paragraph("content")],
            },
        ),
        (
            "compare",
            Block::Comparison {
                figures: vec![paragraph("content")],
            },
        ),
        (
            "matrix-scale",
            Block::MatrixNote {
                blocks: vec![paragraph("content")],
            },
        ),
        (
            "subfaction-ships muted",
            Block::EmptyRoster {
                blocks: vec![paragraph("content")],
            },
        ),
    ] {
        assert_eq!(
            blocks::parse(
                &format!("<div class='{class}'><p>content</p></div>"),
                &[],
                "docs/x.md"
            ),
            [expected]
        );
        // An ID cannot disappear during semantic promotion: navigation still needs that anchor.
        let with_anchor = blocks::parse(
            &format!("<div class='{class}' id='anchor'><p>content</p></div>"),
            &[],
            "docs/x.md",
        );
        assert!(
            matches!(&with_anchor[0], Block::Container { presentation, blocks, .. } if presentation.id == "anchor" && presentation.class == class && blocks == &[paragraph("content")])
        );
    }
    let figure = blocks::parse(
        "<figure class='missing'><p>picture</p><figcaption>caption</figcaption></figure>",
        &[],
        "docs/x.md",
    );
    assert_eq!(
        figure,
        [Block::Figure {
            missing: true,
            blocks: vec![paragraph("picture")],
            caption: vec![paragraph("caption")]
        }]
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
    for (class, matrix) in [
        ("matrix", symphony_api::MatrixKind::Ships),
        ("matrix shape-matrix", symphony_api::MatrixKind::Shapes),
    ] {
        let parsed = blocks::parse(
            &format!("<div class='{class}'><table><tr><td>cell</td></tr></table></div>"),
            &[],
            "docs/x.md",
        );
        let [
            Block::Matrix {
                matrix: kind,
                head,
                rows,
            },
        ] = parsed.as_slice()
        else {
            panic!("{parsed:?}")
        };
        assert_eq!(*kind, matrix);
        assert!(head.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 1);
        assert_eq!(rows[0][0].spans, [text("cell")]);
    }
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
fn relationship_duplicate_keys_keep_their_first_place_and_last_effect() {
    use symphony::relationships::{Declaration, Settings, load};

    let declaration: Declaration = serde_json::from_value(json!({"factions":[{
        "name":"Pilots", "colour":"#123456", "subfactions":[
            {"code":"A", "name":"Alpha", "icon":"a"},
            {"code":"B", "name":"Beta", "icon":"b"},
            {"code":"C", "name":"Gamma", "icon":"c"},
            {"code":"D", "name":"Delta", "icon":"d"}
        ]
    }]}))
    .unwrap();
    let settings = Settings::parse(
        r#"{
        "self":{"destroy":"down strong","help":"up strong"},
        "destroy":{"D":{},"C":{},"B":{},"A":{
            "C":"up small","B":"up small","C":"down slight",
            "others":{"effect":"up medium"}
        }},
        "help":{"D":{},"C":{},"B":{},"A":{}}
    }"#,
    )
    .unwrap();
    let tables = load(&declaration, &settings).unwrap();
    assert_eq!(
        tables
            .destroy
            .iter()
            .map(|r| r.code.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C", "D"]
    );
    let row = tables.row("destroy", "A").unwrap();
    assert_eq!(
        row.effects,
        [
            ("A".into(), -2.0),
            ("C".into(), -0.25),
            ("B".into(), 0.5),
            ("D".into(), 1.0)
        ]
    );
    assert_eq!(row.others.as_ref().unwrap().members, ["D"]);
    assert_eq!(
        tables.to_json()["rows"]["destroy"]["A"],
        json!([["A", -2.0], ["C", -0.25], ["B", 0.5], ["D", 1.0]])
    );
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

#[test]
fn unreadable_screen_manifest_surfaces_in_components_and_the_document_audit() {
    let tree = Tree::new();
    tree.write(
        "docs/data/screens.json",
        r#"{"screens":[{"id":"title","title":"Title"}]}"#,
    );
    tree.write("docs/x.md", "# Screen\n\n[[screen:title]]\n");
    // A directory is unreadable as a manifest on every supported platform, without permission races.
    std::fs::create_dir_all(tree.path("artifacts/screens/manifest.json")).unwrap();
    let renderer = Render::new(&tree.root);
    let fragment = renderer.typed_reply("[[screen:title]]", "docs/x.md");
    let typed = format!("{:?}", blocks::fragment(&fragment, "docs/x.md"));
    assert!(typed.contains("manifest.json"), "{typed}");
    assert!(!typed.contains("not captured yet"), "{typed}");
    let audit = symphony::docs_audit::audit(&tree.root);
    assert_eq!(audit.errors.len(), 1, "{:?}", audit.errors);
    assert!(
        audit.errors[0].starts_with("docs/x.md: does not render cleanly:")
            && audit.errors[0].contains("manifest.json"),
        "{:?}",
        audit.errors
    );
    std::fs::remove_dir(tree.path("artifacts/screens/manifest.json")).unwrap();
    let repaired = renderer.typed_reply("[[screen:title]]", "docs/x.md");
    let typed = blocks::fragment(&repaired, "docs/x.md");
    assert!(
        matches!(&typed[0], Block::Component { component: LiveComponent::Screen { id, blocks } } if id == "title" && matches!(&blocks[0], Block::ScreenCapture { missing: true, .. }))
    );
    assert!(symphony::docs_audit::audit(&tree.root).errors.is_empty());
}
