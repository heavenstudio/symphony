//! The web client's wire types: the TypeScript declarations of the types it reads and the
//! constants it names, written from these Rust types so they are never written by hand twice
//! (apps/symphony-web/src/wire.ts); and, with `cases`, what the functions of this crate and of
//! symphony-blocks the client carries in TypeScript answer for a set of inputs, with a sample of
//! each read the client checks at run time (apps/symphony-web/src/wire.cases.json), which the
//! client's unit tests hold its TypeScript to.
//!
//!     cargo run -p symphony-api --features typescript --example typescript          # wire.ts
//!     cargo run -p symphony-api --features typescript --example typescript cases    # the cases
//!
//! `just client-wire` writes both files from it (apps/symphony-web/wire.sh), and `just client-lint`
//! fails when a committed file is not what it prints. Every type a declared type names is declared
//! too, or this refuses. The cases hold the source of each function the client carries
//! ([`CARRIED`]), so a change to one fails that check even where no case's input shows it.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as _;

use serde_json::{Value, json};
use symphony_api as api;
use ts_rs::{Config, TS};

/// One type's declaration, the names of the types it names, and its own name.
struct Declared {
    name: String,
    decl: String,
    names: Vec<String>,
}

fn declared<T: TS + 'static>(cfg: &Config) -> Declared {
    Declared {
        name: T::ident(cfg),
        decl: T::decl(cfg),
        names: T::dependencies(cfg)
            .into_iter()
            .map(|dependency| dependency.ts_name)
            .collect(),
    }
}

fn main() {
    let printed = match std::env::args().nth(1).as_deref() {
        None => write!(std::io::stdout().lock(), "{}", types()),
        Some("cases") => match cases() {
            Ok(cases) => writeln!(std::io::stdout().lock(), "{cases}"),
            Err(why) => {
                let _ = writeln!(std::io::stderr(), "typescript: {why}");
                std::process::exit(1);
            }
        },
        Some(other) => {
            let _ = writeln!(
                std::io::stderr(),
                "typescript: no `{other}`; it prints wire.ts, or with `cases` the cases"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = printed {
        let _ = writeln!(std::io::stderr(), "typescript: {error}");
        std::process::exit(1);
    }
}

/// apps/symphony-web/src/wire.ts, before Biome formats it.
fn types() -> String {
    // JSON numbers are read by the browser as doubles whatever their Rust width.
    let cfg = Config::new().with_large_int("number");
    // The types the client reads, in the file's order.
    let types = [
        declared::<api::ProjectView>(&cfg),
        declared::<api::NavItem>(&cfg),
        declared::<api::Board>(&cfg),
        declared::<api::Card>(&cfg),
        declared::<api::Picture>(&cfg),
        declared::<api::Counts>(&cfg),
        declared::<api::TaskCounts>(&cfg),
        declared::<api::Progress>(&cfg),
        declared::<api::Region>(&cfg),
        declared::<api::ProgressBar>(&cfg),
        declared::<api::Delta>(&cfg),
        declared::<api::Change>(&cfg),
        declared::<api::Queue>(&cfg),
        declared::<api::QueueItem>(&cfg),
        declared::<api::Activity>(&cfg),
        declared::<api::ActivityItem>(&cfg),
        declared::<api::Pending>(&cfg),
        declared::<api::PendingItem>(&cfg),
        // The blocks a plugin renders to, and what they hold.
        declared::<api::Block>(&cfg),
        declared::<api::Span>(&cfg),
        declared::<api::LinkTarget>(&cfg),
        declared::<api::Presentation>(&cfg),
        declared::<api::Image>(&cfg),
        declared::<api::Cell>(&cfg),
        declared::<api::Verdict>(&cfg),
        declared::<api::ThreadEntry>(&cfg),
        declared::<api::CardState>(&cfg),
        declared::<api::CardGroup>(&cfg),
        declared::<api::ThreadAuthor>(&cfg),
        declared::<api::CalloutKind>(&cfg),
        declared::<api::FigureSize>(&cfg),
        declared::<api::ContainerKind>(&cfg),
        declared::<api::InlineKind>(&cfg),
        declared::<api::VectorShape>(&cfg),
        declared::<api::VectorKind>(&cfg),
        declared::<api::Media>(&cfg),
        declared::<api::GalleryItem>(&cfg),
        declared::<api::Viewer>(&cfg),
        declared::<api::View>(&cfg),
    ];
    let names: BTreeSet<&str> = types.iter().map(|one| one.name.as_str()).collect();
    for one in &types {
        for named in &one.names {
            assert!(
                names.contains(named.as_str()),
                "{} names {named}, which the file does not declare: add it to the list",
                one.name
            );
        }
    }
    let mut out = String::from(
        "// The Symphony's wire types as the web client reads them, written from crates/symphony-api\n\
         // by its `typescript` example (`just client-wire`). Never edited by hand: `just client-lint`\n\
         // fails when this file and the Rust types disagree.\n\n",
    );
    let text = |text: &str| Value::from(text).to_string();
    let constants = [
        ("PROJECT_ELEMENT", text(api::PROJECT_ELEMENT)),
        ("BOARD_PATH", text(api::BOARD_PATH)),
        ("FEED_PATH", text(api::FEED_PATH)),
        ("FEED_EVENT", text(api::FEED_EVENT)),
        ("PING_EVENT", text(api::PING_EVENT)),
        ("SERVER_EVENT", text(api::SERVER_EVENT)),
        ("CACHE_FORMAT", api::CACHE_FORMAT.to_string()),
        ("BOARD_CARDS", api::BOARD_CARDS.to_string()),
        ("FILE_QUERY", text(api::FILE_QUERY)),
    ];
    for (name, value) in constants {
        let _ = writeln!(out, "export const {name} = {value};");
    }
    let _ = writeln!(
        out,
        "export const SYMPHONY_ROUTES = {} as const;",
        json!(api::SYMPHONY_ROUTES)
    );
    let pages = json!({
        "HOME": api::page_names::HOME,
        "QUEUE": api::page_names::QUEUE,
        "DOCS": api::page_names::DOCS,
        "BOARD": api::page_names::BOARD,
        "REPORTS": api::page_names::REPORTS,
        "AGENTS": api::page_names::AGENTS,
        "PROJECTS": api::page_names::PROJECTS,
    });
    let _ = writeln!(out, "export const PAGE_NAMES = {pages} as const;");
    for one in &types {
        let _ = write!(out, "\nexport {}\n", one.decl);
    }
    out
}

/// The project views the cases read routes under: the plain project, one under the one server's
/// base, one with two entrypoints, one at the root, and one with plugin pages.
fn views() -> Vec<api::ProjectView> {
    let nav = |label: &str, href: &str, document: &str| api::NavItem {
        label: label.into(),
        title: label.into(),
        href: href.into(),
        document: document.into(),
    };
    vec![
        api::ProjectView::default(),
        api::ProjectView {
            base: "/alpha".into(),
            ..api::ProjectView::default()
        },
        api::ProjectView {
            entrypoints: vec!["docs".into(), "design".into()],
            ..api::ProjectView::default()
        },
        api::ProjectView {
            entrypoints: vec![String::new()],
            ..api::ProjectView::default()
        },
        api::ProjectView {
            nav: vec![
                nav("▤ shelf", "/shelf", "docs/SHELF.md"),
                nav("◇ maps", "/maps/", "docs/MAPS.md"),
            ],
            ..api::ProjectView::default()
        },
    ]
}

/// apps/symphony-web/src/wire.cases.json, before Biome formats it; why not, when a function
/// [`CARRIED`] names is not found whole.
fn cases() -> Result<Value, String> {
    let categories: Vec<Value> = [
        "docs/ROOT.md",
        "docs/lamps/a.md",
        "docs/a/b/c.md",
        "@proposal~topic/docs/pictures/CARDS.md",
        "@scope",
        "other/a.md",
        "docs/",
        "",
    ]
    .iter()
    .map(|path| json!([path, api::queue_category(path)]))
    .collect();
    let html: Vec<Value> = [
        "a.html",
        "A.HTML",
        "a.Htm",
        "docs/a.htm",
        "a.md",
        "html",
        ".html",
        "docs/.htm",
        "a.html/",
        "a.html.md",
        "a.xhtml",
        "a.html5",
    ]
    .iter()
    .map(|name| json!([name, api::is_html(name)]))
    .collect();
    let addresses: Vec<Value> = [
        "/docs/a.html",
        "/docs/a.html#top",
        "/docs/a.html?state=b#x",
        "/docs/a.html?raw",
        "/docs/a.html?x=1&raw=1#y",
        "/docs/a.html?rawness=1",
        "/docs/a.html?#x",
        "",
    ]
    .iter()
    .map(|src| json!([src, api::file_address(src)]))
    .collect();
    let mut cases = bases();
    cases["views"] = routes();
    cases["queueCategory"] = categories.into();
    cases["isHtml"] = html.into();
    cases["fileAddress"] = addresses.into();
    cases["elementHtml"] = elements();
    cases["encoded"] = encodings();
    cases["decode"] = decodings();
    cases["samples"] = samples();
    cases["carried"] = carried()?;
    Ok(cases)
}

/// What [`api::Viewer::element_html`] writes for a file under a set of viewers: their element, the
/// file, its caption and poster, and the markup (none for an element that is no custom element's).
fn elements() -> Value {
    let viewer = |element: &str, attributes: &[(&str, &str)]| api::Viewer {
        element: element.into(),
        script: "/static/orbit.js".into(),
        attributes: attributes
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
        ..api::Viewer::default()
    };
    let plain = viewer("orbit-view", &[("controls", ""), ("angle", "-45deg")]);
    let odd = viewer(
        "orbit-view",
        &[
            ("onload x", "1"),
            ("Upper", "1"),
            ("9th", "1"),
            ("data-x", "\"><script>'&"),
        ],
    );
    let mut asked: Vec<(api::Viewer, &str, &str, Option<&str>)> = vec![
        (plain.clone(), "/a.item", "A", Some("/a.svg")),
        (plain.clone(), "/a.item", "A", None),
        (odd, "/a\".item", "<b>", Some("/p&q.png")),
    ];
    for element in [
        "",
        "script",
        "Orbit-view",
        "-view",
        "orbit view",
        "orbit-view>",
        "x-1",
    ] {
        asked.push((viewer(element, &[]), "/a.item", "A", None));
    }
    asked
        .into_iter()
        .map(|(viewer, src, label, poster)| {
            let html = viewer.element_html(src, label, poster);
            json!([viewer, src, label, poster, html])
        })
        .collect()
}

/// What [`api::View::encoded`] makes of a set of views' attributes.
fn encodings() -> Value {
    let pairs: [&[(&str, &str)]; 4] = [
        &[("angle", "0deg 25deg auto"), ("zoom", "a=b")],
        &[("bad name", "x"), ("wrapped", "one\ntwo"), ("cr", "a\rb")],
        &[("Upper", "1"), ("ok", ""), ("x-2", "y")],
        &[],
    ];
    pairs
        .iter()
        .map(|set| {
            let view = api::View {
                label: "Above".into(),
                set: set
                    .iter()
                    .map(|(name, value)| ((*name).into(), (*value).into()))
                    .collect(),
                ..api::View::default()
            };
            json!([view, view.encoded()])
        })
        .collect()
}

/// What [`api::View::decode`] reads out of a view's attribute value, line by line: the pairs.
fn decodings() -> Value {
    [
        "angle=0deg 25deg auto\nzoom=a=b",
        "no pair\n=empty\nBad=x\nok=",
        "a=1\r\nb=2\r\n",
        "a=1\r",
        "x=a\rb\n\n\ny-2=",
        "\n",
        "",
        "a-=1\n9=2\nb c=3",
    ]
    .iter()
    .map(|value| json!([value, api::View::decode(value)]))
    .collect()
}

/// The Rust the web client carries in TypeScript, each function by its file, its name and the
/// TypeScript that carries it. Their source goes into the cases whole ([`carried`]), so a change to
/// one, which the cases' inputs may not show, fails `just client-lint` until the cases are written
/// again: the change is then ported, with cases that show it.
const CARRIED: &[(&str, &str, &str)] = &[
    (API_PROJECT, "here", "here, src/project.ts"),
    (API_PROJECT, "strip", "strip, src/project.ts"),
    (API_PROJECT, "entrypoints", "entrypoints, src/project.ts"),
    (
        API_PROJECT,
        "is_document_route",
        "isDocumentRoute, src/project.ts",
    ),
    (
        API_PROJECT,
        "is_folder_route",
        "isFolderRoute, src/project.ts",
    ),
    (
        API_PROJECT,
        "documents_href",
        "documentsHref, src/project.ts",
    ),
    (API_PROJECT, "readme_route", "readmeRoute, src/project.ts"),
    (API_PROJECT, "own_pages", "ownPages, src/project.ts"),
    (API_PROJECT, "is_page", "isPage, src/project.ts"),
    (API_PROJECT, "is_html", "isHtml, src/project.ts"),
    (API_PROJECT, "extension", "extension, src/project.ts"),
    (
        API_PROJECT,
        "html_extension",
        "htmlExtension, src/project.ts",
    ),
    (API_PROJECT, "within", "within, src/project.ts"),
    (API_PROJECT, "asks_for_file", "asksForFile, src/project.ts"),
    (API_PROJECT, "file_address", "fileAddress, src/project.ts"),
    (API_LIB, "queue_category", "queueCategory, src/queue.tsx"),
    (BLOCKS_MEDIA, "element_html", "elementHtml, src/media.tsx"),
    (BLOCKS_MEDIA, "encoded", "encoded, src/media.tsx"),
    (BLOCKS_MEDIA, "decode", "decode, src/media.tsx"),
    (BLOCKS_MEDIA, "is_name", "isName, src/media.tsx"),
    (BLOCKS_MEDIA, "is_element", "isElement, src/media.tsx"),
    (BLOCKS_MEDIA, "escape", "escapeText, src/media.tsx"),
];

/// The files [`CARRIED`] names, by their path in the repository.
const API_PROJECT: &str = "crates/symphony-api/src/project.rs";
const API_LIB: &str = "crates/symphony-api/src/lib.rs";
const BLOCKS_MEDIA: &str = "crates/symphony-blocks/src/media.rs";

/// Each of [`CARRIED`]: its name, the TypeScript that carries it, and its source, line by line.
fn carried() -> Result<Value, String> {
    CARRIED
        .iter()
        .map(|(file, name, ts)| {
            let text = match *file {
                API_PROJECT => include_str!("../src/project.rs"),
                BLOCKS_MEDIA => include_str!("../../symphony-blocks/src/media.rs"),
                _ => include_str!("../src/lib.rs"),
            };
            Ok(json!({
                "rust": format!("{file} {name}"),
                "typescript": ts,
                "source": source(text, name).map_err(|why| format!("{file}: {why}"))?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::from)
}

/// The function `name` in `text`, line by line: from its `fn` line to the first line after it that
/// is that line's indentation and `}`, rustfmt's form, which `cargo fmt --check` in `just lint-rs`
/// holds; so no brace inside it, in a string, a character or a comment, ends it early. A function
/// declared on no line or on two, or with no such closing line, stops the example. Its tests run
/// with `just test-rs -p symphony-api --features typescript --example typescript`, which `just
/// client-lint` runs (apps/symphony-web/check.sh).
fn source<'a>(text: &'a str, name: &str) -> Result<Vec<&'a str>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let declared: Vec<usize> = (0..lines.len())
        .filter(|&at| lines.get(at).is_some_and(|line| declares(line, name)))
        .collect();
    let [start] = declared[..] else {
        return Err(format!(
            "its file declares `fn {name}` on {} lines: CARRIED names one",
            declared.len()
        ));
    };
    let first = lines.get(start).copied().unwrap_or_default();
    // An empty body, which rustfmt leaves on the `fn` line.
    if first.trim_end().ends_with("{}") {
        return Ok(vec![first]);
    }
    let indent = first.strip_suffix(first.trim_start()).unwrap_or_default();
    let close = format!("{indent}}}");
    let after = lines.iter().skip(start + 1).position(|line| *line == close);
    let end = after.ok_or_else(|| format!("`fn {name}` has no line `{close}` after it"))?;
    Ok(lines
        .get(start..=start + 1 + end)
        .unwrap_or_default()
        .to_vec())
}

/// Whether `line` declares the function `name`: code, not a comment, naming `fn name(` or
/// `fn name<`.
fn declares(line: &str, name: &str) -> bool {
    !line.trim_start().starts_with("//")
        && ["(", "<"]
            .iter()
            .any(|after| line.contains(&format!("fn {name}{after}")))
}

/// `here` and `strip` under no base and one, each for a set of paths.
fn bases() -> Value {
    let bases = ["", "/alpha"];
    let here: Vec<Value> = bases
        .iter()
        .flat_map(|base| {
            let view = api::ProjectView {
                base: (*base).into(),
                ..api::ProjectView::default()
            };
            [
                "/tasks",
                "/",
                "tasks",
                "//cdn.example/x",
                "#top",
                "https://a.example/b",
                "",
            ]
            .map(|path| json!([base, path, view.here(path)]))
        })
        .collect();
    let strip: Vec<Value> = bases
        .iter()
        .flat_map(|base| {
            let view = api::ProjectView {
                base: (*base).into(),
                ..api::ProjectView::default()
            };
            [
                "/alpha",
                "/alpha/",
                "/alpha/queue",
                "/alphabet",
                "/queue",
                "/",
            ]
            .map(|path| json!([base, path, view.strip(path)]))
        })
        .collect();
    json!({ "here": here, "strip": strip })
}

/// Each of [`views`] with what it answers for a set of routes, its own pages and its README.
fn routes() -> Value {
    let paths = [
        "",
        "queue",
        "tasks",
        "task/a/b",
        "docs",
        "docs/",
        "docs/A.md",
        "docs/a/",
        "docs/a",
        "docs/x.png",
        "docs/page.html",
        "docs/A.HTM",
        "docs/a/b.Htm",
        "docs/.html",
        "docs/page.html/",
        "docs/a.html.md",
        "docs/a.xhtml",
        "docs/html",
        "design/c.html",
        "readout.html",
        "tasks/a.html",
        "docs/guide.md/",
        "design/",
        "design/B.md",
        "README.md",
        "notes",
        "shelf",
        "maps/",
        "docs/SHELF.md",
        "taskforce/notes.md",
        "api/x",
        "assets/a.js",
        "artifacts/uploads/a",
    ];
    views()
        .iter()
        .map(|view| {
            let answers: Vec<Value> = paths
                .iter()
                .map(|path| {
                    json!([
                        path,
                        view.is_document_route(path),
                        view.is_folder_route(path)
                    ])
                })
                .collect();
            json!({
                "view": view,
                "routes": answers,
                "ownPages": view.own_pages(),
                "readme": view.readme_route(),
            })
        })
        .collect()
}

/// A value of each read the client checks at run time, as the server writes it: every field the
/// type has, then only those it always sends.
fn samples() -> Value {
    let card = api::Card {
        postponed: true,
        slug: "lamp-colour".into(),
        doc: "@proposal~lamps/docs/lamps/LAMPS.md".into(),
        doc_title: "Lamps".into(),
        title: "Which colour do the lamps take?".into(),
        summary: "Warm or cold.".into(),
        why: "design · gates 2 items".into(),
        posted: Some("2026-11-14 08:32".into()),
        pictures: vec![api::Picture {
            src: Some("/thumb/a.png".into()),
            caption: "A".into(),
            waiting: true,
        }],
        task: Some("app/lamps".into()),
    };
    let bare = api::Card {
        postponed: false,
        slug: "s".into(),
        doc: "docs/DECISIONS.md".into(),
        doc_title: "Decisions".into(),
        title: "T".into(),
        summary: String::new(),
        why: "w".into(),
        posted: None,
        pictures: vec![],
        task: None,
    };
    let item = |branch: Option<&str>| api::QueueItem {
        href: "/docs/A.md#s".into(),
        path: "docs/A.md".into(),
        slug: "s".into(),
        title: "A card".into(),
        kind: "comment".into(),
        text: "Answered.".into(),
        branch: branch.map(Into::into),
    };
    let queue = api::Queue {
        waiting: vec![card.clone(), bare.clone()],
        spot_check: vec![item(None)],
        agent: vec![item(Some("app/lamps"))],
        branches: vec![],
    };
    let board = api::Board {
        version: 7,
        build: "/assets/symphony-shell-0123456789abcdef.js".into(),
        ping_ms: 15_000,
        next: vec![card, bare],
        counts: api::Counts::default(),
        progress: api::Progress::default(),
    };
    json!({
        "queue": queue,
        "emptyQueue": api::Queue::default(),
        "board": board,
        "blocks": blocks(),
    })
}

/// Blocks holding every kind of block, span, media, container, inline tag, vector shape and
/// figure size, as a plugin or a document renders them; the client's unit tests read them all and
/// draw each, and hold that no kind its types know is missing here.
fn blocks() -> Vec<api::Block> {
    let mut blocks = sample::prose();
    blocks.extend(sample::cards());
    blocks.extend(sample::media());
    blocks.extend(sample::wrappers());
    blocks
}

/// The parts of the blocks' sample, each holding some of the kinds.
mod sample {
    use std::collections::BTreeMap;

    use serde_json::json;
    use symphony_api::{
        self as api, Block, CalloutKind, CardGroup, CardState, ContainerKind, FigureSize, Image,
        InlineKind, Span, ThreadAuthor, VectorKind,
    };

    fn text(text: &str) -> Span {
        Span::Text { text: text.into() }
    }

    fn words(words: &str) -> Vec<Span> {
        vec![text(words)]
    }

    fn paragraph(words: &str) -> Block {
        Block::Paragraph {
            spans: vec![text(words)],
        }
    }

    fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect()
    }

    fn presentation(class: &str, attributes: &[(&str, &str)]) -> api::Presentation {
        api::Presentation {
            class: class.into(),
            attributes: pairs(attributes),
            ..api::Presentation::default()
        }
    }

    fn picture(src: &str, alt: &str) -> Image {
        let sizes = [
            ("width", "640"),
            ("height", "480"),
            ("data-full", "/full.png"),
        ];
        Image {
            src: src.into(),
            alt: alt.into(),
            presentation: presentation("", &sizes),
        }
    }

    fn lumen() -> Image {
        picture("/k.png", "Lumen")
    }

    fn figure(
        picture: Option<Image>,
        href: Option<&str>,
        badge: Option<&str>,
        size: FigureSize,
    ) -> Block {
        Block::Figure {
            picture,
            caption: words("NOR-L01 Lumen"),
            href: href.map(Into::into),
            badge: badge.map(Into::into),
            size,
        }
    }

    fn entry(who: ThreadAuthor, verdict: Option<&str>, words: &str) -> api::ThreadEntry {
        api::ThreadEntry {
            who,
            timestamp: Some("2026-11-14 08:32".into()),
            verdict: verdict.map(Into::into),
            blocks: vec![paragraph(words)],
        }
    }

    fn target(anchor: Option<&str>) -> api::LinkTarget {
        api::LinkTarget {
            path: "docs/lamps/north.md".into(),
            anchor: anchor.map(Into::into),
        }
    }

    /// One shape of each kind, the last with an attribute no shape may carry.
    fn shapes() -> Vec<api::VectorShape> {
        let shape = |kind, attributes: &[(&str, &str)]| api::VectorShape {
            kind,
            attributes: pairs(attributes),
        };
        vec![
            shape(VectorKind::Path, &[("d", "M4 12h16")]),
            shape(
                VectorKind::Circle,
                &[("cx", "12"), ("cy", "12"), ("r", "4")],
            ),
            shape(
                VectorKind::Ellipse,
                &[("cx", "12"), ("cy", "12"), ("rx", "6"), ("ry", "3")],
            ),
            shape(
                VectorKind::Rect,
                &[("x", "2"), ("y", "2"), ("width", "8"), ("height", "8")],
            ),
            shape(
                VectorKind::Line,
                &[("x1", "0"), ("y1", "0"), ("x2", "24"), ("y2", "24")],
            ),
            shape(VectorKind::Polyline, &[("points", "0,0 12,6 24,0")]),
            shape(
                VectorKind::Polygon,
                &[("points", "0,24 12,12 24,24"), ("onclick", "x()")],
            ),
        ]
    }

    /// One span of each kind, and a styled span of each inline tag.
    fn spans() -> Vec<Span> {
        let link = Span::Link {
            href: "/docs/lamps/north.md#nor-l01".into(),
            target: Some(target(Some("nor-l01"))),
            title: "North".into(),
            spans: words("NOR-L01"),
        };
        let mut spans = vec![
            text("Text "),
            Span::Emphasis {
                spans: words("emphasis"),
            },
            Span::Strong {
                spans: words("strong"),
            },
            Span::Code {
                text: "code".into(),
            },
            link.clone(),
            Span::WikiLink {
                href: "/docs/lamps/north.md".into(),
                target: Some(target(None)),
                spans: words("North"),
            },
            Span::LineBreak,
            Span::Image {
                image: picture("/k.png", "Lumen"),
            },
            Span::Media {
                media: api::Media::Mockup {
                    src: "/docs/ui/hud.html".into(),
                    label: "HUD".into(),
                },
            },
            Span::Component {
                kind: "lamp".into(),
                blocks: vec![paragraph("NOR-L01")],
            },
            Span::Icon {
                shape: shapes(),
                label: "Lamp".into(),
                named: true,
                tip: "A lamp".into(),
            },
            Span::Styled {
                tag: InlineKind::Span,
                presentation: presentation("tasklink", &[("data-task", "app/lamps")]),
                spans: vec![link],
            },
            Span::Graphic {
                view_box: "0 0 24 24".into(),
                presentation: presentation("chart", &[("role", "img")]),
                shapes: shapes(),
            },
            Span::Blocks {
                blocks: vec![figure(Some(lumen()), None, None, FigureSize::Cell)],
            },
            Span::Unknown {
                tag: "blink".into(),
                text: "old".into(),
            },
        ];
        spans.extend(
            [
                InlineKind::Span,
                InlineKind::Small,
                InlineKind::Sub,
                InlineKind::Sup,
                InlineKind::Time,
                InlineKind::Mark,
                InlineKind::Kbd,
                InlineKind::Samp,
                InlineKind::Delete,
                InlineKind::Insert,
                InlineKind::Underline,
                InlineKind::Abbreviation,
            ]
            .map(|tag| {
                let word = json!(tag).as_str().unwrap_or_default().to_string();
                Span::Styled {
                    tag,
                    presentation: presentation(&word, &[]),
                    spans: words(&word),
                }
            }),
        );
        spans
    }

    /// A heading, a paragraph of every span, lists, a table, code, a quote and a rule.
    pub(super) fn prose() -> Vec<Block> {
        vec![
            Block::Heading {
                level: 2,
                anchor: "lamps".into(),
                spans: words("Lamps"),
            },
            Block::Paragraph { spans: spans() },
            Block::List {
                ordered: true,
                start: 3,
                items: vec![vec![paragraph("third")], vec![paragraph("fourth")]],
            },
            Block::List {
                ordered: false,
                start: 1,
                items: vec![vec![paragraph("one")]],
            },
            Block::Table {
                head: vec![api::Cell {
                    header: true,
                    spans: words("Lamp"),
                    presentation: presentation("", &[("scope", "col")]),
                }],
                rows: vec![vec![api::Cell {
                    header: false,
                    spans: words("NOR-L01"),
                    presentation: api::Presentation::default(),
                }]],
                presentation: presentation("lamps", &[]),
            },
            Block::Code {
                language: "rust".into(),
                text: "fn main() {}".into(),
            },
            Block::Quote {
                blocks: vec![paragraph("Quoted")],
            },
            Block::Rule,
        ]
    }

    /// Callouts open and resolved, and cards pending and decided.
    pub(super) fn cards() -> Vec<Block> {
        vec![
            Block::Callout {
                kind: CalloutKind::Question,
                title: vec![
                    Span::Styled {
                        tag: InlineKind::Span,
                        presentation: presentation("callout-kind", &[]),
                        spans: words("question"),
                    },
                    text("Warm or cold?"),
                ],
                resolved: false,
                blocks: vec![paragraph("Which one?")],
            },
            Block::Callout {
                kind: CalloutKind::Resolved,
                title: words("Settled"),
                resolved: true,
                blocks: vec![paragraph("Warm.")],
            },
            Block::Card {
                slug: "lamp-colour".into(),
                mark: CardState::Pending,
                title: "Which colour do the lamps take?".into(),
                body: vec![paragraph("Warm or cold.")],
                verdict: api::Verdict {
                    group: CardGroup::Decide,
                    open: true,
                    posted: Some("2026-11-14 08:32".into()),
                    decided: None,
                },
                thread: vec![
                    entry(ThreadAuthor::Agent, None, "Asked."),
                    entry(ThreadAuthor::Owner, Some("approved"), "Warm."),
                ],
                line: Some(12),
                line_offset: None,
                comparison: vec![Block::Comparison {
                    figures: vec![figure(
                        Some(picture("/a.png", "A")),
                        None,
                        None,
                        FigureSize::Slot,
                    )],
                }],
            },
            Block::Card {
                slug: "lamp-size".into(),
                mark: CardState::Approved,
                title: "How large?".into(),
                body: Vec::new(),
                verdict: api::Verdict {
                    group: CardGroup::Resolved,
                    open: false,
                    posted: None,
                    decided: Some("2026-11-14".into()),
                },
                thread: Vec::new(),
                line: None,
                line_offset: Some(4),
                comparison: Vec::new(),
            },
            Block::Component {
                kind: "lamp".into(),
                blocks: vec![paragraph("A lamp")],
            },
        ]
    }

    /// Each kind of media, a picture, and a figure of each size.
    pub(super) fn media() -> Vec<Block> {
        let viewer = api::Viewer {
            element: "orbit-view".into(),
            script: "/static/orbit.js".into(),
            attributes: vec![("controls".into(), String::new())],
            views: vec![api::View {
                label: "Above".into(),
                title: Some("From above".into()),
                set: vec![("angle".into(), "0deg 25deg".into())],
                call: Some("settle".into()),
            }],
            download: Some(".item".into()),
            open_label: Some("Look".into()),
            close_label: None,
        };
        let item = |name: &str, src: &str, poster: Option<&str>| api::GalleryItem {
            name: name.into(),
            src: src.into(),
            poster: poster.map(Into::into),
        };
        let media = |media| Block::Media { media };
        vec![
            media(api::Media::Mockup {
                src: "/docs/ui/hud.html?state=b#x".into(),
                label: String::new(),
            }),
            media(api::Media::Viewer {
                src: "/models/a.item".into(),
                label: "Model".into(),
                poster: Some("/models/a.png".into()),
                viewer: viewer.clone(),
            }),
            media(api::Media::Gallery {
                items: vec![
                    item("FIRST", "/first.item", Some("/first.png")),
                    item("SECOND", "/second.item", None),
                ],
                viewer,
            }),
            Block::Image {
                image: picture("/k.png", "Lumen"),
            },
            figure(
                Some(lumen()),
                Some("/docs/lamps/north.md#nor-l01"),
                None,
                FigureSize::Small,
            ),
            figure(None, None, Some("coming"), FigureSize::Row),
            figure(None, None, None, FigureSize::Large),
            figure(Some(lumen()), None, None, FigureSize::Tile),
            figure(
                Some(lumen()),
                Some("/docs/lamps/north.md"),
                None,
                FigureSize::Wide,
            ),
            figure(None, None, Some("coming"), FigureSize::Cell),
        ]
    }

    /// Blocks that hold blocks (two threads beside each other among them), a comment, markup the
    /// server does not know, and a container of each tag.
    pub(super) fn wrappers() -> Vec<Block> {
        let mut blocks = vec![
            Block::Caption {
                blocks: vec![paragraph("A caption")],
            },
            Block::Group {
                blocks: vec![paragraph("Grouped")],
            },
            Block::Details {
                summary: words("More"),
                open: true,
                blocks: vec![paragraph("Detail")],
            },
            Block::SettledCard {
                summary: words("lamp-size · approved"),
                open: false,
                blocks: vec![paragraph("Settled")],
            },
            Block::Thread {
                entries: vec![entry(ThreadAuthor::Unknown, None, "Who?")],
            },
            Block::Thread {
                entries: vec![entry(ThreadAuthor::Owner, None, "Still waiting")],
            },
            Block::Comment {
                text: "a note".into(),
            },
            Block::Unknown {
                tag: "marquee".into(),
                text: "Scrolling".into(),
            },
        ];
        blocks.extend(
            [
                ContainerKind::Div,
                ContainerKind::Section,
                ContainerKind::Figure,
                ContainerKind::Figcaption,
                ContainerKind::Aside,
                ContainerKind::Header,
                ContainerKind::Footer,
                ContainerKind::Address,
                ContainerKind::Center,
            ]
            .map(|tag| Block::Container {
                tag,
                presentation: presentation("box", &[("data-kind", "lamp")]),
                blocks: vec![paragraph("Inside")],
            }),
        );
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::source;

    /// Functions as rustfmt writes them, with braces no closing line holds: in a character, a
    /// comment, a string and a format argument; and a doc comment that names one of them.
    const TEXT: &str = "\
pub fn empty() {}

/// Says `fn brace(`, which declares nothing.
pub fn brace(value: char) -> bool {
    // A comment's } closes nothing.
    let closing = '}';
    if value == closing {
        return true;
    }
    format!(\"{value}}}\") == \"}\"
}

impl Wire {
    fn method(&self) -> char {
        '}'
    }
}

fn twice() {
    let _ = 1;
}

fn twice() {
    let _ = 2;
}

fn open() {
    let _ = 1;";

    #[test]
    fn a_function_runs_from_its_fn_line_to_the_line_at_its_indentation_that_closes_it() {
        assert_eq!(
            source(TEXT, "brace"),
            Ok(vec![
                "pub fn brace(value: char) -> bool {",
                "    // A comment's } closes nothing.",
                "    let closing = '}';",
                "    if value == closing {",
                "        return true;",
                "    }",
                "    format!(\"{value}}}\") == \"}\"",
                "}",
            ])
        );
        assert_eq!(
            source(TEXT, "method"),
            Ok(vec![
                "    fn method(&self) -> char {",
                "        '}'",
                "    }"
            ])
        );
        assert_eq!(source(TEXT, "empty"), Ok(vec!["pub fn empty() {}"]));
    }

    #[test]
    fn a_function_declared_on_no_line_or_two_or_never_closed_stops_the_example() {
        assert_eq!(
            source(TEXT, "missing"),
            Err("its file declares `fn missing` on 0 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "twice"),
            Err("its file declares `fn twice` on 2 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "open"),
            Err("`fn open` has no line `}` after it".into())
        );
    }
}
