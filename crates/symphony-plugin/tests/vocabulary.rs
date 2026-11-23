//! The kit's lists that grow in a minor (`docs/engineering/PLUGIN_PROTOCOL.md`, *Versions*), from a crate that
//! links the kit alone, as a plugin's does: a new kind of block, text span or media, a new word
//! one carries, a new state of a gate, a new kind of board field or way of drawing a file, or a
//! new word a Settings page's entry carries is a minor of the kit, never a new major. So each of those enums is `#[non_exhaustive]`: this crate
//! builds because every match on one below ends with `_`, which is all a plugin that lists them
//! needs to keep building on a later minor, and a plugin makes them as before.

use symphony_plugin::vocabulary::{
    Block, CalloutKind, CardGroup, CardState, ContainerKind, FieldValue, FigureSize, GateState,
    InlineKind, Media, Presentation, SettingClass, SettingKind, SettingScope, Span, ThreadAuthor,
    ThreadEntry, VectorKind, Verdict, Viewer,
};
use symphony_plugin::{Drawing, FieldKind, Reply};

/// What a plugin's own code says of a block it made: a match that names the kinds it knows and
/// leaves the rest, a later minor's among them, to `_`.
fn said(block: &Block) -> &'static str {
    match block {
        Block::Paragraph { .. } => "paragraph",
        Block::Rule => "rule",
        Block::Media {
            media: Media::Mockup { .. },
        } => "mockup",
        Block::Media { media } => match media {
            Media::Viewer { .. } => "viewer",
            Media::Gallery { .. } => "gallery",
            _ => "media",
        },
        Block::Container { tag, .. } => match tag {
            ContainerKind::Section => "section",
            _ => "container",
        },
        Block::Figure { size, .. } => match size {
            FigureSize::Wide => "wide figure",
            _ => "figure",
        },
        Block::Callout { kind, .. } => match kind {
            CalloutKind::Question => "question",
            _ => "callout",
        },
        _ => "another",
    }
}

fn spoken(span: &Span) -> &'static str {
    match span {
        Span::Text { .. } => "text",
        Span::LineBreak => "break",
        Span::Styled { tag, .. } => match tag {
            InlineKind::Small => "small",
            _ => "styled",
        },
        _ => "another",
    }
}

#[test]
fn a_plugin_that_matches_the_vocabulary_with_a_wildcard_arm_builds_and_makes_every_kind_as_before()
{
    // Made by their fields and variants, as a plugin makes them: the mark changes no construction.
    let text = Span::Text { text: "A".into() };
    let blocks = [
        Block::Paragraph {
            spans: vec![text.clone()],
        },
        Block::Rule,
        Block::Media {
            media: Media::Mockup {
                src: "a.html".into(),
                label: "A".into(),
            },
        },
        Block::Media {
            media: Media::Viewer {
                src: "a.item".into(),
                label: "A".into(),
                poster: None,
                viewer: Viewer::default(),
            },
        },
        Block::Media {
            media: Media::Gallery {
                items: Vec::new(),
                viewer: Viewer::default(),
            },
        },
        Block::Container {
            tag: ContainerKind::Section,
            presentation: Presentation::default(),
            blocks: Vec::new(),
        },
        Block::Container {
            tag: ContainerKind::Aside,
            presentation: Presentation::default(),
            blocks: Vec::new(),
        },
        Block::Figure {
            picture: None,
            caption: Vec::new(),
            href: None,
            badge: None,
            size: FigureSize::Wide,
        },
        Block::Figure {
            picture: None,
            caption: Vec::new(),
            href: None,
            badge: None,
            size: FigureSize::Tile,
        },
        Block::Callout {
            kind: CalloutKind::Question,
            title: Vec::new(),
            resolved: false,
            blocks: Vec::new(),
        },
        Block::Callout {
            kind: CalloutKind::Like,
            title: Vec::new(),
            resolved: false,
            blocks: Vec::new(),
        },
        Block::Quote { blocks: Vec::new() },
    ];
    assert_eq!(
        blocks.iter().map(said).collect::<Vec<_>>(),
        [
            "paragraph",
            "rule",
            "mockup",
            "viewer",
            "gallery",
            "section",
            "container",
            "wide figure",
            "figure",
            "question",
            "callout",
            "another",
        ]
    );
    let spans = [
        text,
        Span::LineBreak,
        Span::Styled {
            tag: InlineKind::Small,
            presentation: Presentation::default(),
            spans: Vec::new(),
        },
        Span::Styled {
            tag: InlineKind::Kbd,
            presentation: Presentation::default(),
            spans: Vec::new(),
        },
        Span::Strong { spans: Vec::new() },
    ];
    assert_eq!(
        spans.iter().map(spoken).collect::<Vec<_>>(),
        ["text", "break", "small", "styled", "another"]
    );

    // The words a block carries, a gate's state, a field's kind and a drawing, each matched with
    // its `_`.
    let shape = |kind: VectorKind| match kind {
        VectorKind::Path => "path",
        _ => "shape",
    };
    assert_eq!(
        [shape(VectorKind::Path), shape(VectorKind::Rect)],
        ["path", "shape"]
    );
    let state = |mark: CardState| match mark {
        CardState::Pending => "pending",
        _ => "settled",
    };
    assert_eq!(
        [state(CardState::Pending), state(CardState::Approved)],
        ["pending", "settled"]
    );
    let group = |group: CardGroup| match group {
        CardGroup::Decide => "decide",
        _ => "group",
    };
    assert_eq!(
        [group(CardGroup::Decide), group(CardGroup::Auto)],
        ["decide", "group"]
    );
    let author = |who: ThreadAuthor| match who {
        ThreadAuthor::Owner => "owner",
        _ => "other",
    };
    assert_eq!(
        [author(ThreadAuthor::Owner), author(ThreadAuthor::Agent)],
        ["owner", "other"]
    );
    let gate = |state: GateState| match state {
        GateState::Met => "met",
        _ => "not met",
    };
    assert_eq!(
        [gate(GateState::Met), gate(GateState::Held)],
        ["met", "not met"]
    );
    let scope = |scope: SettingScope| match scope {
        SettingScope::Project => "project",
        _ => "not committed",
    };
    assert_eq!(
        [scope(SettingScope::Project), scope(SettingScope::Device)],
        ["project", "not committed"]
    );
    let class = |class: SettingClass| match class {
        SettingClass::Reviewed => "reviewed",
        _ => "set by hand",
    };
    assert_eq!(
        [
            class(SettingClass::Reviewed),
            class(SettingClass::Preference)
        ],
        ["reviewed", "set by hand"]
    );
    let kind = |kind: SettingKind| match kind {
        SettingKind::Switch => "switch",
        _ => "value",
    };
    assert_eq!(
        [kind(SettingKind::Switch), kind(SettingKind::Sentence)],
        ["switch", "value"]
    );
    let field = |kind: &FieldKind| match kind {
        FieldKind::Choice(words) if words.is_empty() => "no choice",
        FieldKind::Choice(_) => "choice",
        _ => "field",
    };
    assert_eq!(
        [
            field(&FieldKind::Choice(Vec::new())),
            field(&FieldKind::Choice(vec!["a".into()])),
            field(&FieldKind::Number),
        ],
        ["no choice", "choice", "field"]
    );
    let drawing = |drawing: &Drawing| match drawing {
        Drawing::Blocks(blocks) if blocks.is_empty() => "nothing",
        Drawing::Blocks(_) => "blocks",
        _ => "drawing",
    };
    assert_eq!(
        [
            drawing(&Drawing::Blocks(Vec::new())),
            drawing(&Drawing::Blocks(vec![Block::Rule])),
            drawing(&Drawing::Viewer(Viewer::default())),
        ],
        ["nothing", "blocks", "drawing"]
    );

    // The two left exhaustive are matched whole, with no `_`: a variant added to one fails this
    // build, as it would a plugin's (`EXHAUSTIVE`).
    let value = |value: &FieldValue| match value {
        FieldValue::Number(_) => "number",
        FieldValue::Text(_) => "text",
    };
    assert_eq!(
        [
            value(&FieldValue::Number(1.5)),
            value(&FieldValue::Text("a".into()))
        ],
        ["number", "text"]
    );
    let reply = |reply: &Reply| match reply {
        Reply::Json(_) => "json",
        Reply::File(_) => "file",
    };
    assert_eq!(
        [
            reply(&Reply::Json("{}".into())),
            reply(&Reply::File("a".into()))
        ],
        ["json", "file"]
    );
}

/// A card and a thread, made as a plugin's component makes them, by every word their fields carry,
/// each named from `vocabulary` alone.
#[test]
fn a_plugin_makes_a_card_and_a_thread_from_the_words_vocabulary_names() {
    let entry = ThreadEntry {
        who: ThreadAuthor::Owner,
        timestamp: Some("2026-11-17 09:00".into()),
        verdict: Some("approved".into()),
        blocks: vec![Block::Paragraph {
            spans: vec![Span::Text { text: "Yes".into() }],
        }],
    };
    let card = Block::Card {
        slug: "kit-words".into(),
        mark: CardState::Approved,
        title: "The kit names every word".into(),
        body: Vec::new(),
        verdict: Verdict {
            group: CardGroup::Resolved,
            open: false,
            posted: Some("2026-11-16".into()),
            decided: Some("2026-11-17".into()),
        },
        thread: vec![entry.clone()],
        line: None,
        line_offset: None,
        comparison: Vec::new(),
    };
    let thread = Block::Thread {
        entries: vec![
            entry,
            ThreadEntry {
                who: ThreadAuthor::Agent,
                timestamp: None,
                verdict: None,
                blocks: Vec::new(),
            },
        ],
    };
    // Read back by their words, the kinds a plugin leaves to `_`.
    let words = |block: &Block| -> Vec<&'static str> {
        let who = |entries: &[ThreadEntry]| -> Vec<&'static str> {
            entries.iter().map(|entry| entry.who.as_str()).collect()
        };
        match block {
            Block::Card {
                mark,
                verdict,
                thread,
                ..
            } => [mark.as_str(), verdict.group.as_str()]
                .into_iter()
                .chain(who(thread))
                .collect(),
            Block::Thread { entries } => who(entries),
            _ => Vec::new(),
        }
    };
    assert_eq!(words(&card), ["approved", "resolved", "owner"]);
    assert_eq!(words(&thread), ["owner", "agent"]);
    assert_eq!(words(&Block::Rule), Vec::<&str>::new());
}

/// The enums `source` defines, a `pub enum` line each (the one `vocabulary!` writes for each of
/// its words among them, as `$name`), each with whether `#[non_exhaustive]` stands among the
/// attributes above it.
fn enums(source: &str) -> Vec<(String, bool)> {
    let lines: Vec<&str> = source.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| {
            let name = line.trim().strip_prefix("pub enum ")?;
            let name = name.split([' ', '<', '{']).next().unwrap_or(name);
            let marked = lines[..at]
                .iter()
                .rev()
                .map(|above| above.trim())
                .take_while(|above| above.starts_with("#[") || above.starts_with("///"))
                .any(|above| above == "#[non_exhaustive]");
            Some((name.to_string(), marked))
        })
        .collect()
}

/// The kit's enums outside the protocol's (whose own are held by `protocol_tests.rs`) that a match
/// outside the kit covers whole, each for its reason. Every other is `#[non_exhaustive]`.
const EXHAUSTIVE: [(&str, &str); 2] = [
    (
        "FieldValue",
        "a board field's value is JSON's number or its text, which a field's `validate` tells apart: a third shape is another wire format, a new major",
    ),
    (
        "Reply",
        "a consuming project's route test matches it whole, so marking it breaks that build: a new kind of reply waits for a consumer release plan",
    ),
];

#[test]
fn every_enum_of_the_kit_a_minor_may_grow_is_non_exhaustive() {
    let sources = [
        include_str!("../../symphony-blocks/src/lib.rs"),
        include_str!("../../symphony-blocks/src/blocks.rs"),
        include_str!("../../symphony-blocks/src/media.rs"),
        include_str!("../../symphony-blocks/src/gates.rs"),
        include_str!("../../symphony-blocks/src/field_value.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/renderer.rs"),
    ];
    let found: Vec<(String, bool)> = sources.iter().flat_map(|source| enums(source)).collect();
    let names = |marked: bool| -> Vec<&str> {
        found
            .iter()
            .filter(|(_, is)| *is == marked)
            .map(|(name, _)| name.as_str())
            .collect()
    };
    // `$name` is `vocabulary!`'s: `CardState`, `CardGroup`, `ThreadAuthor`, `CalloutKind` and
    // `FigureSize`, which the build above matches with `_`.
    assert_eq!(
        names(true),
        [
            "$name",
            "Block",
            "Span",
            "ContainerKind",
            "InlineKind",
            "VectorKind",
            "Media",
            "GateState",
            "FieldKind",
            "Drawing",
        ],
        "a list of the kit that grows in a minor is `#[non_exhaustive]`"
    );
    assert_eq!(
        names(false),
        EXHAUSTIVE.map(|(name, _)| name),
        "an enum left exhaustive is named in EXHAUSTIVE with its reason; any other is marked"
    );
    assert!(EXHAUSTIVE.iter().all(|(_, why)| !why.is_empty()));
}

#[test]
fn the_reading_tells_a_marked_enum_from_one_left_exhaustive() {
    let read = "/// A thing.\n#[derive(Debug)]\n#[non_exhaustive]\npub enum Marked<T> {\n    A(T),\n}\n\
                #[derive(Debug)]\npub enum Left {\n    B,\n}\n\
                #[non_exhaustive]\npub struct NoEnum;\n\
                        #[non_exhaustive]\n        pub enum $name { A }\n\
                #[non_exhaustive]\n\npub enum Apart {}\nenum Private {}\n";
    assert_eq!(
        enums(read),
        [
            ("Marked".to_string(), true),
            ("Left".to_string(), false),
            ("$name".to_string(), true),
            // A blank line ends the attributes above an item.
            ("Apart".to_string(), false),
        ]
    );
    assert_eq!(enums(""), Vec::new());
}
