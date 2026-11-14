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
        declared::<api::TaskListing>(&cfg),
        declared::<api::Task>(&cfg),
        declared::<api::Role>(&cfg),
        declared::<api::FieldValue>(&cfg),
        declared::<api::TaskChange>(&cfg),
        declared::<api::TaskFields>(&cfg),
        declared::<api::ItemChange<api::Task>>(&cfg),
        declared::<api::MachineWork>(&cfg),
        declared::<api::MachineObservation>(&cfg),
        declared::<api::WorkOperation>(&cfg),
        declared::<api::WorkCounts>(&cfg),
        declared::<api::WorkGuidance>(&cfg),
        declared::<api::WorkFreshness>(&cfg),
        declared::<api::WorkKind>(&cfg),
        declared::<api::WorkState>(&cfg),
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
    cases["slugOrder"] = slug_orders();
    cases["stages"] = stages();
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
    ("task_queries.rs", "slug_order", "slugOrder, src/task.ts"),
    ("task_queries.rs", "kind", "isTask, src/task.ts"),
    ("task_queries.rs", "is_task", "isTask, src/task.ts"),
    ("task_queries.rs", "has_status", "taskStage, src/work.ts"),
    ("machine_work.rs", "is_for", "isFor, src/work.ts"),
    ("task_stage.rs", "task_stage", "taskStage, src/work.ts"),
    ("task_stage.rs", "places_alike", "placesAlike, src/work.ts"),
    ("task_stage.rs", "can_place", "canPlace, src/work.ts"),
    ("task_stage.rs", "conflicts", "conflicts, src/work.ts"),
];

/// Each of [`CARRIED`]: its name, the TypeScript that carries it, and its source, line by line.
fn carried() -> Result<Value, String> {
    CARRIED
        .iter()
        .map(|(file, name, ts)| {
            let text = match *file {
                "project.rs" => include_str!("../src/project.rs"),
                "task_queries.rs" => include_str!("../src/task_queries.rs"),
                "machine_work.rs" => include_str!("../src/machine_work.rs"),
                "task_stage.rs" => include_str!("../src/task_stage.rs"),
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
/// with `just test-rs -p symphony-api --features typescript --example typescript`, which `just
/// client-lint` runs (apps/symphony-web/check.sh).
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

/// How `slug_order` orders each pair of a set of slugs: -1, 0 or 1, as `a` comes before, with or
/// after `b`. Runs of digits by their number, leading zeros apart, text and bytes past ASCII.
fn slug_orders() -> Value {
    let slugs = [
        "",
        "0",
        "00",
        "01",
        "1",
        "9",
        "10",
        "ready-3",
        "ready-30",
        "ready-03",
        "ready-3a",
        "ready-3-1",
        "a2b9",
        "a2b10",
        "a10b1",
        "ready",
        "ready-",
        "ready-#",
        "ready-a",
        "ready-é",
        "é01",
        "a/2",
        "a/10",
        "A1",
        "a 1",
        "\u{ffff}",
        "\u{10000}",
    ];
    let pairs: Vec<Value> = slugs
        .iter()
        .flat_map(|a| {
            slugs.iter().map(move |b| {
                let order = match api::slug_order(a, b) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                json!([a, b, order])
            })
        })
        .collect();
    pairs.into()
}

/// An operation of a machine's work: `kind` (`build`, `test`, `command`) and `state` (`queued`,
/// `running`) of the task `task`, as the machine `machine` reports it by the id `id`.
fn operation(machine: &str, id: &str, task: &str, kind: &str, state: &str) -> api::WorkOperation {
    let kind = match kind {
        "build" => api::WorkKind::Build,
        "test" => api::WorkKind::Test,
        _ => api::WorkKind::Command,
    };
    let state = if state == "running" {
        api::WorkState::Running
    } else {
        api::WorkState::Queued
    };
    api::WorkOperation {
        machine_id: (!machine.is_empty()).then(|| machine.into()),
        operation_id: Some(id.into()),
        parent_id: None,
        handle: Some("amber".into()),
        task: Some(task.into()),
        checkout: "/opaque".into(),
        recipe: "fixture".into(),
        kind,
        state,
        resource: "run_slot".into(),
        reason: String::new(),
        phase_since: None,
        under: Vec::new(),
    }
}

/// A machine `id` reporting `operations`, as `freshness` and `complete` say it.
fn machine(
    id: &str,
    freshness: api::WorkFreshness,
    complete: bool,
    operations: Vec<api::WorkOperation>,
) -> api::MachineObservation {
    api::MachineObservation {
        machine_id: id.into(),
        label: id.into(),
        producer_id: "fixture".into(),
        sequence: 1,
        age_ms: Some(0),
        freshness,
        complete,
        reason: String::new(),
        counts: None,
        operations,
    }
}

/// The work of one machine `alpha`, fresh and complete, reporting `operations`.
fn work(operations: Vec<api::WorkOperation>) -> api::MachineWork {
    api::MachineWork {
        freshness: api::WorkFreshness::Fresh,
        machines: vec![machine(
            "alpha",
            api::WorkFreshness::Fresh,
            true,
            operations,
        )],
        ..api::MachineWork::default()
    }
}

/// A task `slug` in `status`, under `parent` when one is given.
fn task(slug: &str, status: &str, parent: Option<&str>) -> api::Task {
    api::Task {
        slug: slug.into(),
        title: "A task".into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: parent.map(Into::into),
        created_by: "owner".into(),
        created: 1.0,
        updated: 2.0,
        links: vec!["docs/A.md".into()],
        depends: Vec::new(),
        roles: vec![api::Role {
            handle: "amber".into(),
            role: "executor".into(),
            since: 1.0,
            expires: None,
            health: "live".into(),
        }],
        instruction: false,
        fields: std::collections::BTreeMap::new(),
    }
}

/// The works the stage cases place tasks by: each what it shows, by its name.
fn works() -> Vec<(&'static str, api::MachineWork)> {
    let op = operation;
    let mut under = op("alpha", "u", "w/a/1", "build", "running");
    under.under = vec!["w/a".into(), "w".into()];
    let mut no_id = op("alpha", "n", "w/a", "test", "running");
    no_id.operation_id = None;
    let mut empty_id = op("alpha", "e", "w/a", "test", "running");
    empty_id.operation_id = Some(String::new());
    let mut contradicted = work(vec![op("alpha", "c", "w/a", "build", "running")]);
    contradicted.machines.push(machine(
        "beta",
        api::WorkFreshness::Fresh,
        true,
        vec![op("alpha", "c", "w/a", "build", "queued")],
    ));
    let mut stale = work(vec![op("alpha", "s", "w/a", "test", "running")]);
    stale.machines[0].freshness = api::WorkFreshness::Stale;
    let mut partial = work(vec![op("alpha", "p", "w/a", "test", "queued")]);
    partial.machines[0].freshness = api::WorkFreshness::Incomplete;
    partial.machines[0].complete = false;
    let unassigned = api::MachineWork {
        unassigned: vec![op("alpha", "x", "w/a", "build", "running")],
        ..api::MachineWork::default()
    };
    vec![
        ("none", api::MachineWork::default()),
        (
            "queued build",
            work(vec![op("alpha", "b", "w/a", "build", "queued")]),
        ),
        (
            "running build",
            work(vec![op("alpha", "b", "w/a", "build", "running")]),
        ),
        (
            "queued test",
            work(vec![op("alpha", "t", "w/a", "test", "queued")]),
        ),
        (
            "running build, queued test",
            work(vec![
                op("alpha", "b", "w/a", "build", "running"),
                op("alpha", "t", "w/a", "test", "queued"),
            ]),
        ),
        (
            "queued test, running test",
            work(vec![
                op("alpha", "t", "w/a", "test", "queued"),
                op("alpha", "r", "w/a", "test", "running"),
            ]),
        ),
        (
            "another task's",
            work(vec![op("alpha", "o", "w/b", "test", "running")]),
        ),
        (
            "a command",
            work(vec![op("alpha", "c", "w/a", "command", "running")]),
        ),
        (
            "another machine's id",
            work(vec![op("beta", "m", "w/a", "build", "running")]),
        ),
        (
            "no machine id",
            work(vec![op("", "m", "w/a", "build", "running")]),
        ),
        ("no operation id", work(vec![no_id])),
        ("an empty operation id", work(vec![empty_id])),
        ("a subtask's", work(vec![under])),
        ("contradicted", contradicted),
        ("stale", stale),
        ("partial", partial),
        ("unassigned", unassigned),
    ]
}

/// The stage each work puts each task in, connected and not, and whether each two works place
/// alike (`places_alike`), the same works told apart by a heartbeat and by the order of their
/// operations.
fn stages() -> Value {
    let works = works();
    let tasks = [
        task("w/a", "in_progress", None),
        task("w/a", "review", None),
        task("w", "in_progress", None),
        task("w/a/1", "in_progress", Some("w/a")),
        task("w/b", "in_progress", None),
    ];
    let placed: Vec<Value> = works
        .iter()
        .flat_map(|(name, work)| {
            tasks.iter().enumerate().flat_map(move |(at, task)| {
                [true, false].map(|connected| {
                    json!([
                        name,
                        at,
                        connected,
                        work.task_stage(task, connected).as_str()
                    ])
                })
            })
        })
        .collect();
    // Two operations, a running build and a queued test of `w/a`, told apart from this work by
    // what places nothing (a heartbeat, a command) and by their order.
    let both = || {
        work(vec![
            operation("alpha", "b", "w/a", "build", "running"),
            operation("alpha", "t", "w/a", "test", "queued"),
        ])
    };
    let mut beat = both();
    for machine in &mut beat.machines {
        machine.sequence = 9;
        machine.age_ms = Some(900);
    }
    let mut swapped = both();
    for machine in &mut swapped.machines {
        machine.operations.reverse();
    }
    let mut others = both();
    for machine in &mut others.machines {
        machine
            .operations
            .push(operation("alpha", "x", "w/c", "command", "running"));
    }
    let mut compared: Vec<(&str, api::MachineWork)> = works.clone();
    compared.extend([
        ("a heartbeat later", beat),
        ("swapped", swapped),
        ("with a command", others),
    ]);
    let alike: Vec<Value> = compared
        .iter()
        .flat_map(|(a, one)| {
            compared
                .iter()
                .map(move |(b, other)| json!([a, b, one.places_alike(other)]))
        })
        .collect();
    let works: serde_json::Map<String, Value> = compared
        .iter()
        .map(|(name, work)| ((*name).to_string(), json!(work)))
        .collect();
    let tasks: Vec<Value> = tasks.iter().map(|task| json!(task)).collect();
    json!({ "works": works, "tasks": tasks, "placed": placed, "alike": alike })
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
    let mut idle = task("w/b", "ready", None);
    idle.links.clear();
    idle.roles.clear();
    let mut fielded = task("w/a/1", "in_progress", Some("w/a"));
    fielded.fields.insert(
        "game".into(),
        [
            ("effort".to_string(), api::FieldValue::Number(3.0)),
            ("risk".to_string(), api::FieldValue::Text("high".into())),
        ]
        .into(),
    );
    let mut guided = work(vec![operation("alpha", "b", "w/a", "build", "running")]);
    guided.guidance.insert(
        "alpha".into(),
        api::WorkGuidance {
            build: Some(2),
            test: None,
        },
    );
    guided.unassigned = vec![operation("", "x", "w/b", "test", "queued")];
    let listing = api::TaskListing {
        machine_work: guided.clone(),
        tasks: vec![task("w/a", "in_progress", None), fielded, idle],
        statuses: vec!["ready".into(), "in_progress".into()],
        signoff: vec!["w/a".into()],
        counts: vec![("ready".into(), 1), ("in_progress".into(), 2)],
        lifecycle_counts: vec![("ready".into(), 1), ("in_progress".into(), 2)],
        next: Some("w/b".into()),
    };
    let empty = api::TaskListing {
        machine_work: api::MachineWork::default(),
        tasks: Vec::new(),
        statuses: Vec::new(),
        signoff: Vec::new(),
        counts: Vec::new(),
        lifecycle_counts: Vec::new(),
        next: None,
    };
    let change = api::TaskChange {
        key: "w/a".into(),
        fields: api::TaskFields {
            status: Some("review".into()),
            priority: Some(1),
            parent: Some(String::new()),
            roles: Some(Vec::new()),
            ..api::TaskFields::default()
        },
    };
    let added = api::ItemChange {
        key: "w/a".into(),
        value: Some(task("w/a", "ready", None)),
        removed: false,
    };
    let removed = api::ItemChange::<api::Task> {
        key: "w/a".into(),
        value: None,
        removed: true,
    };
    json!({
        "queue": queue,
        "emptyQueue": api::Queue::default(),
        "board": board,
        "listing": listing,
        "emptyListing": empty,
        "work": guided,
        "emptyWork": api::MachineWork::default(),
        "taskChange": change,
        "bareChange": api::TaskChange { key: "w/b".into(), fields: api::TaskFields::default() },
        "taskAdded": added,
        "taskRemoved": removed,
    })
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
