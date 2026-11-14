//! The web client's wire types: the TypeScript declarations of the types it reads and the
//! constants it names, written from these Rust types so they are never written by hand twice
//! (apps/symphony-web/src/wire.ts); and, with `cases`, what this crate's functions the client
//! carries in TypeScript answer for a set of inputs, with a sample of each read the client checks
//! at run time (apps/symphony-web/src/wire.cases.json), which the client's unit tests hold its
//! TypeScript to.
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
    let mut cases = bases();
    cases["views"] = routes();
    cases["queueCategory"] = categories.into();
    cases["isHtml"] = html.into();
    cases["samples"] = samples();
    cases["carried"] = carried()?;
    Ok(cases)
}

/// The Rust the web client carries in TypeScript, each function by its file, its name and the
/// TypeScript that carries it. Their source goes into the cases whole ([`carried`]), so a change to
/// one, which the cases' inputs may not show, fails `just client-lint` until the cases are written
/// again: the change is then ported, with cases that show it.
const CARRIED: &[(&str, &str, &str)] = &[
    ("project.rs", "here", "here, src/project.ts"),
    ("project.rs", "strip", "strip, src/project.ts"),
    ("project.rs", "entrypoints", "entrypoints, src/project.ts"),
    (
        "project.rs",
        "is_document_route",
        "isDocumentRoute, src/project.ts",
    ),
    (
        "project.rs",
        "is_folder_route",
        "isFolderRoute, src/project.ts",
    ),
    (
        "project.rs",
        "documents_href",
        "documentsHref, src/project.ts",
    ),
    ("project.rs", "readme_route", "readmeRoute, src/project.ts"),
    ("project.rs", "own_pages", "ownPages, src/project.ts"),
    ("project.rs", "is_page", "isPage, src/project.ts"),
    ("project.rs", "is_html", "isHtml, src/project.ts"),
    ("project.rs", "extension", "extension, src/project.ts"),
    (
        "project.rs",
        "html_extension",
        "htmlExtension, src/project.ts",
    ),
    ("project.rs", "within", "within, src/project.ts"),
    ("lib.rs", "queue_category", "queueCategory, src/queue.tsx"),
];

/// Each of [`CARRIED`]: its name, the TypeScript that carries it, and its source, line by line.
fn carried() -> Result<Value, String> {
    CARRIED
        .iter()
        .map(|(file, name, ts)| {
            let text = match *file {
                "project.rs" => include_str!("../src/project.rs"),
                _ => include_str!("../src/lib.rs"),
            };
            Ok(json!({
                "rust": format!("crates/symphony-api/src/{file} {name}"),
                "typescript": ts,
                "source": source(text, name)?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Value::from)
}

/// The function `name` in `text`, line by line: from its `fn` line to the first line after it that
/// is that line's indentation and `}`, rustfmt's form, which `cargo fmt --check` in `just lint-rs`
/// holds; so no brace inside it, in a string, a character or a comment, ends it early. A function
/// declared on no line or on two, or with no such closing line, stops the example. Its tests run
/// with `just test-rs -p symphony-api --features typescript --example typescript`.
fn source<'a>(text: &'a str, name: &str) -> Result<Vec<&'a str>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let declared: Vec<usize> = (0..lines.len())
        .filter(|&at| lines.get(at).is_some_and(|line| declares(line, name)))
        .collect();
    let [start] = declared[..] else {
        return Err(format!(
            "crates/symphony-api declares `fn {name}` on {} lines: CARRIED names one",
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
    json!({ "queue": queue, "emptyQueue": api::Queue::default(), "board": board })
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
            Err("crates/symphony-api declares `fn missing` on 0 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "twice"),
            Err("crates/symphony-api declares `fn twice` on 2 lines: CARRIED names one".into())
        );
        assert_eq!(
            source(TEXT, "open"),
            Err("`fn open` has no line `}` after it".into())
        );
    }
}
