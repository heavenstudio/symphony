//! Every kind of the plugin kit's vocabulary has its arm wherever this workspace matches the
//! kinds. The kit's lists grow in a minor (`docs/engineering/PLUGIN_PROTOCOL.md`, *Versions*), so they are
//! `#[non_exhaustive]` and each match outside the kit ends with `_`: the compiler does not refuse
//! a build in which a kind has no arm. This reads the sources in its place: a variant of a list
//! that a match below does not name fails here, naming the variant and the function. The client
//! is no Rust: its own check is its type check and `blocks.test.tsx`, over the types
//! `just client-wire` writes from these same lists.

/// The variants of the enum `name` in `source`, as rustfmt writes it: each line one step in from
/// the `pub enum` line that starts with a capital letter, up to the line that closes it.
fn variants(source: &str, name: &str) -> Vec<String> {
    let opening = format!("pub enum {name} {{");
    let mut lines = source.lines().skip_while(|line| line.trim() != opening);
    let Some(first) = lines.next() else {
        return Vec::new();
    };
    let indent = first.len() - first.trim_start().len();
    let closing = format!("{}}}", " ".repeat(indent));
    let step = " ".repeat(indent + 4);
    lines
        .take_while(|line| *line != closing)
        .filter_map(|line| line.strip_prefix(step.as_str()))
        .filter(|rest| rest.starts_with(|first: char| first.is_ascii_uppercase()))
        .map(|rest| {
            rest.split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or(rest)
                .to_string()
        })
        .collect()
}

/// The function whose first line holds `header`, from that line to the line at its indentation
/// that closes it (rustfmt's form), each line ended; empty when `source` has none.
fn function(source: &str, header: &str) -> String {
    let mut lines = source.lines().skip_while(|line| !line.contains(header));
    let Some(first) = lines.next() else {
        return String::new();
    };
    let indent = first.len() - first.trim_start().len();
    let closing = format!("{}}}", " ".repeat(indent));
    let mut body = format!("{first}\n");
    for line in lines {
        body.push_str(line);
        body.push('\n');
        if line == closing {
            break;
        }
    }
    body
}

/// The variants of `list` that `body` does not name as `LIST::Variant`.
fn unnamed(list: &str, variants: &[String], body: &str) -> Vec<String> {
    variants
        .iter()
        .filter(|variant| {
            let name = format!("{list}::{variant}");
            // The whole name, not the start of a longer one (`Block::Card` in `Block::Cards`).
            !body
                .split(name.as_str())
                .skip(1)
                .any(|after| !after.starts_with(|next: char| next.is_ascii_alphanumeric()))
        })
        .cloned()
        .collect()
}

const BLOCKS: &str = include_str!("../../symphony-blocks/src/blocks.rs");
const MEDIA: &str = include_str!("../../symphony-blocks/src/media.rs");
const RENDERER: &str = include_str!("../../symphony-plugin/src/renderer.rs");
const WIRE: &str = include_str!("../src/lib.rs");
const WRITE: &str = include_str!("../../symphony/src/blocks/write.rs");
const LEGACY: &str = include_str!("../../symphony/src/docview/legacy.rs");
const DOCVIEW: &str = include_str!("../../symphony/src/docview.rs");
const RENDER_MEDIA: &str = include_str!("../../symphony/src/render/media.rs");

/// Each match of this workspace on a list of the kit that grows: the list, the source that
/// defines it, the file of the match, that file's source and the function holding the match.
const MATCHES: [(&str, &str, &str, &str, &str); 10] = [
    (
        "Block",
        BLOCKS,
        "crates/symphony-api/src/lib.rs",
        WIRE,
        "fn edge(",
    ),
    (
        "Block",
        BLOCKS,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn block(",
    ),
    (
        "Span",
        BLOCKS,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn span(",
    ),
    (
        "Media",
        MEDIA,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn media(",
    ),
    (
        "VectorKind",
        BLOCKS,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn shapes(",
    ),
    (
        "ContainerKind",
        BLOCKS,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn container_tag(",
    ),
    (
        "InlineKind",
        BLOCKS,
        "crates/symphony/src/blocks/write.rs",
        WRITE,
        "fn inline_tag(",
    ),
    (
        "Media",
        MEDIA,
        "crates/symphony/src/docview/legacy.rs",
        LEGACY,
        "fn legacy_html(",
    ),
    (
        "Media",
        MEDIA,
        "crates/symphony/src/docview.rs",
        DOCVIEW,
        "fn media_urls(",
    ),
    (
        "Drawing",
        RENDERER,
        "crates/symphony/src/render/media.rs",
        RENDER_MEDIA,
        "fn drawn(",
    ),
];

#[test]
fn every_kind_of_the_vocabulary_has_its_arm_in_each_match_of_the_workspace() {
    let mut missing = Vec::new();
    for (list, defined, file, source, header) in MATCHES {
        let variants = variants(defined, list);
        assert!(variants.len() >= 2, "{list}: read as {variants:?}");
        let body = function(source, header);
        assert!(
            body.contains("match ") && (body.contains("_ =>") || body.contains("(_) =>")),
            "{file}: `{header}` holds a match ending with `_`"
        );
        for variant in unnamed(list, &variants, &body) {
            missing.push(format!("{list}::{variant} in `{header}` of {file}"));
        }
    }
    assert_eq!(
        missing,
        Vec::<String>::new(),
        "a kind of the vocabulary with no arm of its own, which `_` would take in silence: give \
         each its arm"
    );
}

#[test]
fn the_lists_are_read_whole_as_the_kit_defines_them() {
    // Each list from its first variant to its last, and no fewer than it had when this was
    // written: a variant added changes nothing here.
    for (source, list, first, last, least) in [
        (BLOCKS, "Block", "Heading", "Unknown", 22),
        (BLOCKS, "Span", "Text", "Unknown", 15),
        (MEDIA, "Media", "Mockup", "Gallery", 3),
        (RENDERER, "Drawing", "Blocks", "Viewer", 2),
        (BLOCKS, "VectorKind", "Path", "Polygon", 7),
        (BLOCKS, "ContainerKind", "Div", "Center", 9),
        (BLOCKS, "InlineKind", "Span", "Abbreviation", 12),
    ] {
        let read = variants(source, list);
        assert!(read.len() >= least, "{list}: {read:?}");
        assert_eq!(read.first().map(String::as_str), Some(first), "{list}");
        assert!(
            read.iter().any(|variant| variant == last),
            "{list}: {read:?}"
        );
    }
    let blocks = variants(BLOCKS, "Block");
    for inner in ["Rule", "Media", "SettledCard", "Comment"] {
        assert!(blocks.iter().any(|variant| variant == inner), "{inner}");
    }
    // A variant's own fields are none of the list.
    for field in ["level", "anchor", "spans", "Vec"] {
        assert!(!blocks.iter().any(|variant| variant == field), "{field}");
    }
    assert_eq!(variants(BLOCKS, "Missing"), Vec::<String>::new());
}

#[test]
fn a_variant_no_arm_names_is_found_and_a_longer_name_is_not_taken_for_it() {
    let kit = "/// Things.\n#[non_exhaustive]\npub enum Thing {\n    /// One.\n    One {\n        \
               inner: Vec<Other>,\n    },\n    #[serde(default)]\n    Two,\n    TwoMore(u8),\n}\n\
               pub enum Other {\n    Three,\n}\n";
    assert_eq!(variants(kit, "Thing"), ["One", "Two", "TwoMore"]);
    assert_eq!(variants(kit, "Other"), ["Three"]);

    let host = "fn other(thing: &Thing) {\n    let _ = Thing::Two;\n}\n\n\
                impl Writer {\n    fn write(&mut self, thing: &Thing) {\n        match thing {\n            \
                Thing::One { .. } => {}\n            Thing::TwoMore(_) => {}\n            _ => {}\n        \
                }\n    }\n\n    fn after(&self) {\n        let _ = Thing::Two;\n    }\n}\n";
    let body = function(host, "fn write(");
    assert!(body.starts_with("    fn write(") && body.ends_with("    }\n"));
    assert!(!body.contains("fn after") && !body.contains("fn other"));
    let all = variants(kit, "Thing");
    // `Thing::Two` is named outside the function, and `Thing::TwoMore` inside is not it.
    assert_eq!(unnamed("Thing", &all, &body), ["Two"]);
    assert_eq!(unnamed("Thing", &all, host), Vec::<String>::new());
    assert_eq!(unnamed("Thing", &all, ""), all);
    assert_eq!(function(host, "fn none("), "");
    // A function at the start of its file, and one that is the file's last lines.
    assert!(function(host, "fn other(").ends_with("Thing::Two;\n}\n"));
    assert_eq!(
        function("fn last() {\n    one();\n}", "fn last("),
        "fn last() {\n    one();\n}\n"
    );
    // One whose closing line never comes runs to the end of what it was given.
    assert_eq!(
        function("fn open() {\n    one();", "fn open("),
        "fn open() {\n    one();\n"
    );
}
