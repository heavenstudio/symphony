//! Each plugin call measured: timed into the project's record, and through the JSON loopback
//! answering as the plugin does linked, seam by seam, with a value that does not survive JSON
//! named loudly.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use symphony::plugins::{Plugins, Reply};
use symphony::project::{PluginCalls, Project};
use symphony_plugin::vocabulary::{
    Block, FieldValue, Gate, GatePast, GateState, LinkTarget, NavItem, Picture, Presentation,
    ProgressBar, Region, Span, TaskOffer, View, Viewer,
};
use symphony_plugin::{
    BoardAction, BoardField, Card, CardKind, Command, Component, DocCheck, Docs, Drawing,
    FieldKind, File, Gates, Generator, HomeRegion, Page, Plugin, Problem, ProseRule, Renderer,
    Route, Submission, Submitted, TaskRef, Theme, ThreadLine, Transport, Watch,
};

use super::Tree;
use super::compiled::naming;

/// A plugin with one object at every seam, each answer made of every argument it is given, so a
/// request that lost or changed a field answers otherwise.
pub(super) struct Every {
    /// The root of each page whose shared data was loaded, once a load.
    loads: Mutex<Vec<PathBuf>>,
    /// The roots its watch was told to forget.
    forgot: Mutex<Vec<PathBuf>>,
}

pub(super) static EVERY: Every = Every {
    loads: Mutex::new(Vec::new()),
    forgot: Mutex::new(Vec::new()),
};

impl Plugin for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self, &AGAIN]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![self]
    }
    fn routes(&self) -> Vec<&dyn Route> {
        vec![self]
    }
    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: "⬡ every".into(),
            title: "Every".into(),
            href: "/x/every".into(),
            document: "docs/EVERY.md".into(),
        }]
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        vec![self]
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        vec![self]
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        vec![self]
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        vec![self]
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        vec![self]
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        vec![self]
    }
    fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        vec![self]
    }
    fn generators(&self) -> Vec<&dyn Generator> {
        vec![self]
    }
    fn commands(&self) -> Vec<&dyn Command> {
        vec![self]
    }
    fn transports(&self) -> Vec<&dyn Transport> {
        vec![self]
    }
    fn theme(&self) -> Option<Theme> {
        Some(Theme {
            package: "every".into(),
            name: "Every".into(),
            ..Theme::default()
        })
    }
}

/// What a page's components load once for a render: counted, so a test sees it shared.
fn shared(page: &Page<'_>) -> std::sync::Arc<String> {
    page.once("every-shared", || {
        EVERY.loads.lock().unwrap().push(page.root.to_path_buf());
        format!("{}|{}", page.root.display(), page.doc_path)
    })
}

impl Component for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if argument == "bad" {
            return Err(format!("every refuses {argument:?} on {}", page.doc_path));
        }
        Ok(vec![
            Block::Heading {
                level: 2,
                anchor: argument.into(),
                spans: vec![Span::Text {
                    text: format!("{argument} {option:?}"),
                }],
            },
            Block::Paragraph {
                spans: vec![Span::Styled {
                    tag: symphony_plugin::vocabulary::InlineKind::Span,
                    presentation: Presentation {
                        class: "every".into(),
                        attributes: [("data-at".to_string(), shared(page).to_string())].into(),
                        ..Presentation::default()
                    },
                    spans: vec![
                        Span::Link {
                            href: format!("/docs/{argument}.md"),
                            target: Some(LinkTarget {
                                path: format!("docs/{argument}.md"),
                                anchor: option.map(str::to_string),
                            }),
                            title: "\"quoted\" \\ title".into(),
                            spans: vec![Span::Code {
                                text: "a\tb\n".into(),
                            }],
                        },
                        Span::LineBreak,
                    ],
                }],
            },
        ])
    }
    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        Some(format!("v1:{}", shared(page)))
    }
}

/// A second component of the plugin, reading the page's shared data too.
struct Again;

static AGAIN: Again = Again;

impl Component for Again {
    fn name(&self) -> &str {
        "again"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        _: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("{argument} again on {}", shared(page)),
            }],
        }])
    }
    fn resolve(&self, _: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        match option {
            Some(option) => Err(format!(
                "again takes no option, not {option:?} on {argument}"
            )),
            None => Ok(()),
        }
    }
}

impl CardKind for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        card.text.contains("every")
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        card.thread
            .iter()
            .enumerate()
            .map(|(n, line)| Picture {
                src: line
                    .agent
                    .then(|| format!("{}/{}/{n}.png", root.display(), card.slug)),
                caption: format!("{} {}", card.topic, line.text),
                waiting: line.verdict.is_none(),
            })
            .collect()
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        let (_, after) = card.text.split_once("*Slot:* ")?;
        Some(after.lines().next().unwrap_or_default().to_string())
    }
    fn markers(&self) -> &[&str] {
        &["Slot"]
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        Some(format!("{} · {}", card.doc, card.topic))
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        !card.text.contains("hold")
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        Some((card.thread.len(), format!("line of {}", card.slug)))
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        card.thread.iter().any(|line| line.agent)
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        if card.text.contains("broken") {
            return Err(format!("{} names no candidate", card.slug));
        }
        Ok(card
            .thread
            .iter()
            .rev()
            .find(|line| line.verdict == Some("approved"))
            .and_then(|line| line.text.chars().next())
            .map(String::from))
    }
}

impl Renderer for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn extensions(&self) -> &[&str] {
        &["every"]
    }
    fn content_type(&self) -> &str {
        "model/every"
    }
    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        if file.label.is_empty() {
            return Ok(Drawing::Viewer(Viewer {
                element: "every-view".into(),
                script: format!("{}/every.js", page.doc_path),
                attributes: vec![
                    ("src".into(), file.src.into()),
                    ("bare".into(), String::new()),
                ],
                views: vec![View {
                    label: "spin".into(),
                    ..View::default()
                }],
                download: Some("Download".into()),
                open_label: None,
                close_label: Some("Close it".into()),
            }));
        }
        Ok(Drawing::Blocks(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("{} drawn from {}", file.label, file.src),
            }],
        }]))
    }
}

impl Route for Every {
    fn path(&self) -> &str {
        "every"
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        match rest {
            "" => None,
            "data.json" => Some(Reply::Json(format!(
                "{{\"root\":{:?}}}",
                root.display().to_string()
            ))),
            file => Some(Reply::File(root.join(file))),
        }
    }
}

impl HomeRegion for Every {
    fn id(&self) -> &str {
        "every"
    }
    fn region(&self, root: &Path) -> Option<Region> {
        Some(Region {
            id: "every".into(),
            title: root.display().to_string(),
            note: "1 of 3".into(),
            bars: vec![ProgressBar {
                key: "k".into(),
                name: "n".into(),
                page: "docs/EVERY.md".into(),
                title: String::new(),
                done: 1,
                waiting: 1,
                total: 3,
            }],
            legend: ["done".into(), "waiting".into(), "left".into()],
        })
    }
}

impl BoardAction for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        if task.status != "review" {
            return Err(format!("{} is {}", task.slug, task.status));
        }
        Ok(TaskOffer {
            action: "every".into(),
            label: format!("land from {}", root.display()),
            branch: task.branch.into(),
            payload: Some(format!("{}@{}", task.slug, task.branch)),
        })
    }
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        if submission.payload != format!("{}@{}", task.slug, task.branch) {
            return Err("a stale offer".into());
        }
        Ok(Submitted {
            said: format!("{} in {}: {}", task.slug, root.display(), submission.note),
            released: vec![task.slug.into(), task.branch.into()],
        })
    }
}

impl BoardField for Every {
    fn name(&self) -> &str {
        "effort"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Number
    }
}

impl Gates for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String> {
        let Some(tip) = tip else {
            return Err(format!("{} has no branch", task.branch));
        };
        Ok(vec![Gate {
            name: "certificate".into(),
            state: GateState::Met,
            by: Some(task.slug.into()),
            at: Some(1_762_934_400.125),
            commit: Some(tip.into()),
            why: None,
            detail: root.display().to_string(),
            link: Some(format!("/task/{}", task.slug)),
            past: vec![GatePast {
                state: GateState::Missing,
                at: 0.1 + 0.2,
                by: None,
                commit: None,
                detail: task.status.into(),
            }],
        }])
    }
    fn kept_while(&self, root: &Path, task: &TaskRef<'_>) -> String {
        format!("{} {} in {}", task.status, task.branch, root.display())
    }
}

impl Watch for Every {
    fn paths(&self) -> Vec<String> {
        vec!["data/every".into(), "art".into()]
    }
    fn changed(&self, root: &Path) -> Option<String> {
        Some(format!("warmed {}", root.display()))
    }
    fn forget(&self, root: &Path) {
        self.forgot.lock().unwrap().push(root.to_path_buf());
    }
}

impl DocCheck for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn components(&self, docs: &Docs) -> Vec<String> {
        docs.paths()
            .iter()
            .map(|path| format!("every:{path}"))
            .collect()
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        docs.paths()
            .iter()
            .filter(|path| docs.text(path).contains("every"))
            .map(|path| Problem::new(path, 1, format!("{path} says every")))
            .collect()
    }
}

impl ProseRule for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn check(&self, rel: &str, text: &str) -> Vec<(usize, String)> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| line.contains("every"))
            .map(|(n, line)| (n + 1, format!("{rel}: {line}")))
            .collect()
    }
}

impl Generator for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn outputs(&self) -> &str {
        "out/"
    }
    fn command(&self) -> Vec<String> {
        vec!["make".into(), "every".into()]
    }
}

impl Command for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn summary(&self) -> &str {
        "print its arguments"
    }
    fn run(
        &self,
        args: &[String],
        out: &mut dyn std::io::Write,
        err: &mut dyn std::io::Write,
    ) -> i32 {
        writeln!(out, "{}", args.join("|")).unwrap();
        writeln!(err, "{} arguments", args.len()).unwrap();
        i32::try_from(args.len()).unwrap()
    }
}

impl Transport for Every {
    fn name(&self) -> &str {
        "every"
    }
    fn register(&self, handle: &str, reported: &str) -> Result<String, String> {
        if reported.is_empty() {
            return Err(format!("{handle} reported nothing"));
        }
        Ok(format!("{handle}@{reported}"))
    }
    fn deliver(&self, address: &str, message: &str) -> Result<(), String> {
        if message.is_empty() {
            return Err(format!("nothing to send to {address}"));
        }
        Ok(())
    }
    fn alive(&self, address: &str) -> bool {
        address.starts_with('/')
    }
}

/// The record at `path`, each line read as JSON.
fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The loopback alone.
const LOOPBACK: PluginCalls = PluginCalls {
    timing: false,
    loopback: true,
};

/// Every call of every seam, as the record names it (`seam.call`).
const EVERY_CALL: [&str; 28] = [
    "components.render",
    "components.resolve",
    "components.cache_version",
    "card_kinds.recognises",
    "card_kinds.pictures",
    "card_kinds.slot",
    "card_kinds.triage_category",
    "card_kinds.auto_approvable",
    "card_kinds.gates",
    "card_kinds.approval_needs_agent",
    "card_kinds.chosen",
    "renderers.draw",
    "routes.serve",
    "home_regions.region",
    "board_actions.offer",
    "board_actions.submit",
    "board_fields.validate",
    "gates.answer",
    "gates.kept_while",
    "watches.changed",
    "watches.forget",
    "doc_checks.check",
    "doc_checks.components",
    "prose_rules.check",
    "commands.run",
    "transports.register",
    "transports.deliver",
    "transports.alive",
];

/// The calls the record at `path` names, each once (`seam.call`).
fn recorded_calls(path: &Path) -> BTreeSet<String> {
    lines(path)
        .iter()
        .map(|line| {
            format!(
                "{}.{}",
                line["seam"].as_str().unwrap(),
                line["call"].as_str().unwrap()
            )
        })
        .collect()
}

/// `plugins` linked and the same measured, its record read as each call writes it.
struct Both {
    linked: Plugins,
    looped: Plugins,
    record: PathBuf,
    /// The record's lines already read.
    read: Mutex<usize>,
}

impl Both {
    /// `plugin` linked and through the loopback.
    fn new(plugin: &'static dyn Plugin, tree: &Tree) -> Both {
        Both::measured(plugin, tree, LOOPBACK)
    }

    /// `plugin` linked and measured as `calls` says.
    fn measured(plugin: &'static dyn Plugin, tree: &Tree, calls: PluginCalls) -> Both {
        let record = tree.path("artifacts/plugin-calls.jsonl");
        Both {
            linked: Plugins::new(vec![plugin]),
            looped: Plugins::new(vec![plugin]).measured(calls, &record),
            read: Mutex::new(lines(&record).len()),
            record,
        }
    }

    /// That `call` answers the same measured as linked, every value read back as it was written:
    /// a call whose value did not survive JSON fails, naming it.
    fn same<T: PartialEq + Debug>(&self, what: &str, call: impl Fn(&Plugins) -> T) {
        let linked = call(&self.linked);
        let looped = call(&self.looped);
        let lines = lines(&self.record);
        let mut read = self.read.lock().unwrap();
        let written = &lines[*read..];
        *read = lines.len();
        assert!(!written.is_empty(), "{what}: no call was recorded");
        for line in written {
            assert!(
                line.get("lost").is_none(),
                "{what}: {}.{} did not survive JSON: {}",
                line["seam"].as_str().unwrap(),
                line["call"].as_str().unwrap(),
                line["lost"]
            );
        }
        assert_eq!(linked, looped, "{what}");
    }
}

fn card_lines() -> Vec<(bool, Option<&'static str>, &'static str)> {
    vec![
        (true, None, "A or B? \"quoted\""),
        (false, Some("approved"), "B, with ✅ and a tab\t"),
    ]
}

/// What `read` makes of a card with `text`.
fn with_card<R>(text: &str, read: impl FnOnce(&Card<'_>) -> R) -> R {
    let thread: Vec<ThreadLine<'_>> = card_lines()
        .into_iter()
        .map(|(agent, verdict, text)| ThreadLine {
            agent,
            verdict,
            text,
        })
        .collect();
    read(&Card {
        slug: "pick-a-hull",
        doc: "docs/SHIPS.md",
        topic: "Hulls · \\ back",
        text,
        thread: &thread,
    })
}

#[test]
fn every_seam_answers_the_same_linked_and_through_the_loopback() {
    let tree = Tree::new();
    let both = Both::new(&EVERY, &tree);
    every_seam_answers_the_same(&both, &tree);
    // Every call of every seam went through the loopback, and the plugin's declaration once.
    let mut every: BTreeSet<String> = EVERY_CALL.into_iter().map(String::from).collect();
    every.insert("plugin.declare".into());
    assert_eq!(recorded_calls(&both.record), every);
}

#[test]
fn every_seam_answers_the_same_linked_and_timed_and_its_line_holds_its_time_alone() {
    let tree = Tree::new();
    let timing = PluginCalls {
        timing: true,
        loopback: false,
    };
    let both = Both::measured(&EVERY, &tree, timing);
    every_seam_answers_the_same(&both, &tree);
    // Every call of every seam was timed, and nothing was sent through JSON: not the declaration.
    let every: BTreeSet<String> = EVERY_CALL.into_iter().map(String::from).collect();
    assert_eq!(recorded_calls(&both.record), every);
    for line in lines(&both.record) {
        let mut keys: Vec<&str> = line
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["at_ms", "call", "name", "pid", "plugin", "seam", "us"],
            "{line}"
        );
    }
}

/// That every call of every seam of [`EVERY`] answers the same through `both`'s measured plugins
/// as linked, the project's checkout at `tree`.
fn every_seam_answers_the_same(both: &Both, tree: &Tree) {
    tree.write("docs/A.md", "# A\n\nevery word\n");
    tree.write("docs/B.md", "# B\n");
    let root = tree.root.as_path();
    let page = || Page::new(root, "docs/ünï code.md");
    for (argument, option) in [("lantern", Some("wide")), ("", None), ("bad", None)] {
        both.same("render", |p| p.render(&page(), "every", argument, option));
        both.same("resolve", |p| {
            p.component("every")
                .unwrap()
                .resolve(&page(), argument, option)
        });
        both.same("resolve again", |p| {
            p.component("again")
                .unwrap()
                .resolve(&page(), argument, option)
        });
    }
    both.same("cache_version", |p| p.cache_version(&page()));
    for text in [
        "Pick every hull.\n*Slot:* North · cover",
        "hold every broken one",
        "",
    ] {
        both.same("recognises", |p| {
            with_card(text, |card| p.card_kinds()[0].recognises(card))
        });
        both.same("pictures", |p| {
            with_card(text, |card| p.card_kinds()[0].pictures(root, card))
        });
        both.same("slot", |p| {
            with_card(text, |card| p.card_kinds()[0].slot(card))
        });
        both.same("triage_category", |p| {
            with_card(text, |card| p.card_kinds()[0].triage_category(card))
        });
        both.same("auto_approvable", |p| {
            with_card(text, |card| p.card_kinds()[0].auto_approvable(card))
        });
        both.same("gates", |p| {
            with_card(text, |card| p.card_kinds()[0].gates(card))
        });
        both.same("approval_needs_agent", |p| {
            with_card(text, |card| p.card_kinds()[0].approval_needs_agent(card))
        });
        both.same("chosen", |p| {
            with_card(text, |card| p.card_kinds()[0].chosen(card))
        });
    }
    for (src, label) in [("/docs/a.every?v=2#top", ""), ("/docs/b.every", "the hull")] {
        let file = File { src, label };
        both.same("draw", |p| {
            symphony::renderers::renderer_of(p, src)
                .unwrap()
                .draw(&page(), &file)
        });
    }
    for rest in ["", "data.json", "models/a b.glb"] {
        both.same("serve", |p| p.route("every").unwrap().serve(root, rest));
    }
    both.same("region", |p| p.home_regions()[0].region(root));
    for status in ["review", "in_progress"] {
        let task = TaskRef {
            slug: "sim/jump",
            status,
            branch: "sim/jump-2",
        };
        both.same("offer", |p| p.board_actions()[0].offer(root, &task));
        for payload in ["sim/jump@sim/jump-2", "stale"] {
            let submission = Submission {
                payload: payload.into(),
                note: "send it ✅".into(),
            };
            both.same("submit", |p| {
                p.board_actions()[0].submit(root, &task, &submission)
            });
        }
        for tip in [Some("c0ffee"), None] {
            both.same("answer", |p| {
                p.list()[0].gates()[0].answer(root, &task, tip)
            });
        }
        both.same("kept_while", |p| {
            p.list()[0].gates()[0].kept_while(root, &task)
        });
    }
    for value in [
        FieldValue::Text(" 3.5 ".into()),
        FieldValue::Number(0.1 + 0.2),
        FieldValue::Text("many".into()),
    ] {
        both.same("validate", |p| {
            p.board_field("every", "effort").unwrap().validate(&value)
        });
    }
    both.same("changed", |p| p.watches()[0].changed(root));
    both.same("forget", |p| p.watches()[0].forget(root));
    let forgot = EVERY.forgot.lock().unwrap();
    assert_eq!(forgot.iter().filter(|forgot| *forgot == root).count(), 2);
    drop(forgot);
    let docs = Docs::read(root);
    both.same("doc check", |p| p.doc_checks()[0].check(&docs));
    both.same("doc components", |p| p.doc_checks()[0].components(&docs));
    both.same("prose", |p| {
        p.prose_rules()[0].check("docs/A.md", "# A\n\nevery line\r\nnot this\n")
    });
    for args in [
        vec![],
        vec!["--all".to_string(), "a b".into(), String::new()],
    ] {
        both.same("run", |p| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = p.list()[0].commands()[0].run(&args, &mut out, &mut err);
            (code, out, err)
        });
    }
    let transport = |p: &Plugins| p.list()[0].transports()[0];
    for (handle, reported) in [("amber-heron", "/tmp/s.sock"), ("x", "")] {
        both.same("register", |p| transport(p).register(handle, reported));
    }
    for message in ["PULL\n", ""] {
        both.same("deliver", |p| transport(p).deliver("/tmp/s.sock", message));
    }
    both.same("alive", |p| transport(p).alive("relative"));
}

#[test]
fn what_a_measured_plugin_declares_is_the_plugins_own() {
    let tree = Tree::new();
    let both = Both::new(&EVERY, &tree);
    let declared = |p: &Plugins| symphony_plugin::protocol::Declaration::of(p.list()[0]);
    assert_eq!(declared(&both.linked), declared(&both.looped));
    assert_eq!(
        symphony::landing::with_plugins(&Project::plain(&tree.root), &both.looped),
        symphony::landing::with_plugins(&Project::plain(&tree.root), &both.linked)
    );
    assert_eq!(both.looped.nav(), EVERY.nav());
    assert_eq!(both.looped.theme("every"), EVERY.theme());
    assert_eq!(both.looped.component_names(), ["task", "every", "again"]);
    assert_eq!(
        symphony::renderers::content_type(&both.looped, "/a.every"),
        Some("model/every")
    );
}

#[test]
fn a_pages_components_share_what_they_load_through_the_loopback_as_linked() {
    let tree = Tree::new();
    let both = Both::new(&EVERY, &tree);
    let loads = |plugins: &Plugins| {
        let page = Page::new(&tree.root, "docs/A.md");
        let loaded = || {
            let loads = EVERY.loads.lock().unwrap();
            loads.iter().filter(|loaded| **loaded == tree.root).count()
        };
        let before = loaded();
        plugins.render(&page, "every", "a", None);
        plugins.render(&page, "again", "b", None);
        plugins.cache_version(&page);
        loaded() - before
    };
    assert_eq!(loads(&both.linked), 1);
    assert_eq!(loads(&both.looped), 1);
}

/// A plugin whose answers hold numbers JSON cannot carry: a field validated to infinity, and a
/// gate whose time is not a number.
struct Lossy;

static LOSSY: Lossy = Lossy;

impl Plugin for Lossy {
    fn name(&self) -> &str {
        "lossy"
    }
    fn board_fields(&self) -> Vec<&dyn BoardField> {
        vec![self]
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        vec![self]
    }
}

impl BoardField for Lossy {
    fn name(&self) -> &str {
        "effort"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Number
    }
    fn validate(&self, _: &FieldValue) -> Result<FieldValue, String> {
        Ok(FieldValue::Number(f64::INFINITY))
    }
}

impl Gates for Lossy {
    fn name(&self) -> &str {
        "lossy"
    }
    fn answer(&self, _: &Path, _: &TaskRef<'_>, _: Option<&str>) -> Result<Vec<Gate>, String> {
        Ok(vec![Gate {
            name: "certificate".into(),
            at: Some(f64::NEG_INFINITY),
            ..Gate::default()
        }])
    }
}

#[test]
#[should_panic(expected = "validate: board_fields.validate did not survive JSON: \"the answer: ")]
fn an_answer_that_does_not_read_back_fails_the_comparison_loudly_though_the_answers_agree() {
    let tree = Tree::new();
    let both = Both::new(&LOSSY, &tree);
    // The loopback keeps the plugin's own answer when JSON cannot carry it, so the two agree: the
    // record is what tells.
    both.same("validate", |p| {
        p.board_field("lossy", "effort")
            .unwrap()
            .validate(&FieldValue::Number(1.0))
    });
}

#[test]
#[should_panic(
    expected = "answer: gates.answer did not survive JSON: \"the answer read back otherwise\""
)]
fn an_answer_that_reads_back_otherwise_fails_the_comparison_loudly() {
    let tree = Tree::new();
    let both = Both::new(&LOSSY, &tree);
    let task = TaskRef {
        slug: "a/b",
        status: "review",
        branch: "a/b",
    };
    both.same("answer", |p| {
        p.list()[0].gates()[0].answer(&tree.root, &task, None)
    });
}

#[test]
fn a_lost_value_is_named_on_its_line_and_the_seam_answers_what_came_back() {
    let tree = Tree::new();
    let both = Both::new(&LOSSY, &tree);
    let task = TaskRef {
        slug: "a/b",
        status: "review",
        branch: "a/b",
    };
    let gates = both.looped.list()[0].gates()[0]
        .answer(&tree.root, &task, None)
        .unwrap();
    // A time JSON writes as null reads back as no time.
    assert_eq!(gates[0].at, None);
    let lost: Vec<String> = lines(&both.record)
        .iter()
        .filter_map(|line| Some(format!("{} {}", line["call"], line.get("lost")?)))
        .collect();
    assert_eq!(lost, ["\"answer\" \"the answer read back otherwise\""]);
}

#[test]
fn timing_records_each_call_with_its_plugin_seam_object_and_time() {
    let tree = naming(&["first"]);
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"first\"]\ntiming = true\n",
    );
    let plugins = Plugins::of(&tree.root);
    let page = Page::new(&tree.root, "docs/A.md");
    plugins.render(&page, "word", "x", None);
    plugins.watches()[0].changed(&tree.root);
    let lines = lines(&tree.path("artifacts/plugin-calls.jsonl"));
    let said: Vec<(String, String, String, String)> = lines
        .iter()
        .map(|line| {
            let text = |key: &str| line[key].as_str().unwrap().to_string();
            (text("plugin"), text("seam"), text("name"), text("call"))
        })
        .collect();
    let said: Vec<(&str, &str, &str, &str)> = said
        .iter()
        .map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("first", "components", "word", "render"),
            ("first", "watches", "data/one", "changed")
        ]
    );
    for line in &lines {
        assert!(line["us"].as_f64().unwrap() >= 0.0, "{line}");
        assert!(line.get("round_trip_us").is_none(), "{line}");
    }
}

#[test]
fn the_record_goes_under_the_projects_artifacts_folder_and_the_loopback_records_both_times() {
    let tree = naming(&["first"]);
    tree.write(
        symphony::project::FILE,
        "[project]\nartifacts = \"out/kept\"\n\n[plugins]\ncrates = [\"first\"]\nloopback = true\n",
    );
    let project = Project::of(&tree.root);
    assert_eq!(project.plugin_calls, LOOPBACK);
    assert_eq!(
        symphony::plugins::record(&project, &tree.root),
        tree.path("out/kept/plugin-calls.jsonl")
    );
    let plugins = Plugins::of(&tree.root);
    assert_eq!(
        plugins.route("one").unwrap().serve(&tree.root, "data"),
        Some(Reply::Json("{\"one\":1}".into()))
    );
    let lines = lines(&tree.path("out/kept/plugin-calls.jsonl"));
    let serve = lines
        .iter()
        .find(|line| line["seam"] == "routes")
        .expect("the route's call is recorded");
    assert_eq!(serve["call"], "serve");
    assert!(serve["round_trip_us"].as_f64().unwrap() >= serve["us"].as_f64().unwrap());
    assert!(serve["request_bytes"].as_u64().unwrap() > 0);
    assert!(!tree.path("artifacts").exists());
}

#[test]
fn neither_setting_is_on_unless_the_file_says_so_and_then_nothing_is_recorded() {
    assert_eq!(
        Project::parse(Path::new("/r"), "").unwrap().plugin_calls,
        PluginCalls::default()
    );
    assert!(!PluginCalls::default().recorded());
    let tree = naming(&["first"]);
    let plugins = Plugins::of(&tree.root);
    assert!(std::ptr::eq(
        plugins.list()[0],
        symphony::plugins::compiled()[0]
    ));
    let page = Page::new(&tree.root, "docs/A.md");
    plugins.render(&page, "word", "x", None);
    plugins.watches()[0].changed(&tree.root);
    assert!(!tree.path("artifacts").exists());
    let timed = Project::parse(Path::new("/r"), "[plugins]\ntiming = true\n").unwrap();
    assert_eq!(
        timed.plugin_calls,
        PluginCalls {
            timing: true,
            loopback: false
        }
    );
    assert!(timed.plugin_calls.recorded());
    assert!(
        Project::parse(Path::new("/r"), "[plugins]\ntiming = \"yes\"\n").is_err(),
        "a setting that is not true or false is refused"
    );
}

#[test]
fn a_plugin_command_is_measured_as_the_project_it_runs_in_says() {
    let tree = naming(&[]);
    let run = |root: Option<&Path>| {
        let command = symphony::plugins::command("every", root).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = command.run(&["a".into(), "b c".into()], &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let said = (2, "a|b c\n".to_string(), "2 arguments\n".to_string());
    assert_eq!(run(None), said);
    assert_eq!(run(Some(&tree.root)), said);
    assert!(!tree.path("artifacts").exists());
    tree.write(symphony::project::FILE, "[plugins]\nloopback = true\n");
    assert_eq!(run(Some(&tree.root)), said);
    let lines = lines(&tree.path("artifacts/plugin-calls.jsonl"));
    let run_line = lines
        .iter()
        .find(|line| line["seam"] == "commands")
        .expect("the command's run is recorded");
    assert_eq!(run_line["call"], "run");
    assert_eq!(run_line["name"], "every");
    assert!(run_line.get("lost").is_none());
    assert!(symphony::plugins::command("nobody", Some(&tree.root)).is_none());
}

/// The environment variable naming the project the child of
/// [`with_both_settings_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file`] runs
/// its command in.
const QUIET_ROOT: &str = "SYMPHONY_TEST_QUIET_COMMAND_ROOT";

#[test]
fn with_both_settings_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file() {
    if let Some(root) = std::env::var_os(QUIET_ROOT) {
        // The child: the command found and run as `symphony NAME` finds and runs it.
        naming(&[]);
        let command = symphony::plugins::command("every", Some(Path::new(&root))).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(command.run(&["a".into()], &mut out, &mut err), 1);
        return;
    }
    // A key this build does not know, which a loaded project names on stderr.
    let run = |file: &str| {
        let tree = Tree::new();
        tree.write(symphony::project::FILE, file);
        let done = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "seams::calls::with_both_settings_off_a_plugin_command_loads_no_project_and_says_nothing_of_its_file",
                "--nocapture",
            ])
            .env(QUIET_ROOT, &tree.root)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&done.stdout).into_owned();
        assert!(done.status.success() && said.contains("1 passed"), "{said}");
        (
            String::from_utf8_lossy(&done.stderr).into_owned(),
            tree.path("artifacts").exists(),
        )
    };
    let (said, recorded) = run("[plugins]\ncolour = \"red\"\n");
    assert!(!said.contains("colour"), "{said}");
    assert!(!recorded);
    // With a setting on, the project is loaded as the server loads it, and its file named.
    let (said, recorded) = run("[plugins]\ncolour = \"red\"\ntiming = true\n");
    assert!(
        said.contains("colour") && said.contains("left out"),
        "{said}"
    );
    assert!(recorded);
}

#[test]
fn the_two_settings_are_read_alone_and_any_file_they_cannot_be_read_from_turns_neither_on() {
    let tree = Tree::new();
    let read = |file: &str| {
        tree.write(symphony::project::FILE, file);
        PluginCalls::read(&tree.root)
    };
    let off = PluginCalls::default();
    assert_eq!(PluginCalls::read(&tree.root), off, "no file");
    assert_eq!(read(""), off);
    assert_eq!(
        read("[project]\nlost = 1\n[plugins]\ncolour = \"red\"\nloopback = true\n"),
        LOOPBACK
    );
    assert_eq!(
        read("[plugins]\ntiming = true\nloopback = true\n"),
        PluginCalls {
            timing: true,
            loopback: true
        }
    );
    assert_eq!(read("[plugins]\ntiming = \"yes\"\n"), off);
    assert_eq!(read("[plugins\ntiming = true\n"), off);
    // With both off, a project's plugins and a plugin command are the plugins' own.
    let tree = naming(&["every"]);
    let project = Project::of(&tree.root);
    assert!(std::ptr::eq(
        Plugins::at(&project, &tree.root).list()[0],
        Plugins::for_project(&project).list()[0]
    ));
    let own = EVERY.commands()[0];
    let command = symphony::plugins::command("every", Some(&tree.root)).unwrap();
    assert!(std::ptr::addr_eq(command, own));
    let unmeasured = Plugins::new(vec![&EVERY]).measured(off, &tree.path("never.jsonl"));
    assert!(std::ptr::addr_eq(unmeasured.list()[0], &EVERY));
}

/// Each `pub trait` of `source` and the methods it declares.
fn traits(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    blocks(source, |line| {
        let named = line.strip_prefix("pub trait ")?;
        Some(named.chars().take_while(|c| c.is_alphanumeric()).collect())
    })
}

/// Each `impl` block of `source`, by its line (`impl Component for On<dyn Component>`), and the
/// methods it defines.
fn impls(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    blocks(source, |line| {
        line.starts_with("impl ")
            .then(|| line.strip_suffix(" {"))
            .flatten()
            .map(str::to_string)
    })
}

/// The blocks of `source` that `opens` names by their first line, each with the methods at its
/// first depth (`    fn NAME`), to the `}` that closes it.
fn blocks(
    source: &str,
    opens: impl Fn(&str) -> Option<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut found = BTreeMap::new();
    let mut open: Option<(String, BTreeSet<String>)> = None;
    for line in source.lines() {
        if let Some((_, methods)) = open.as_mut() {
            if line == "}" {
                let (name, methods) = open.take().unwrap();
                found.insert(name, methods);
            } else if let Some(method) = line.strip_prefix("    fn ") {
                methods.insert(
                    method
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect(),
                );
            }
        } else if let Some(name) = opens(line) {
            open = Some((name, BTreeSet::new()));
        }
    }
    found
}

/// The method a seam's declared entry is the answer of, where the entry is that answer alone: a
/// component declared by its name, a route by its path…
const DECLARED_BY: [(&str, &str); 8] = [
    ("components", "name"),
    ("routes", "path"),
    ("home_regions", "id"),
    ("board_actions", "name"),
    ("gates", "name"),
    ("watches", "paths"),
    ("prose_rules", "name"),
    ("transports", "name"),
];

#[test]
fn every_method_of_every_seam_is_a_call_or_declared_and_the_wrapper_measures_each() {
    let mut seams_traits = traits(include_str!("../../../../symphony-plugin/src/lib.rs"));
    seams_traits.extend(traits(include_str!(
        "../../../../symphony-plugin/src/renderer.rs"
    )));
    seams_traits.extend(traits(include_str!(
        "../../../../symphony-docs/src/check.rs"
    )));
    let wrapper = impls(include_str!("../../../src/plugins/measure.rs"));
    let calls: BTreeSet<&str> = symphony_plugin::protocol::CALLS.iter().copied().collect();
    let declared =
        serde_json::to_value(symphony_plugin::protocol::Declaration::of(&EVERY)).unwrap();
    // Each seam, as `Plugin` hands its objects out: `fn card_kinds(&self) -> Vec<&dyn CardKind>`.
    let seams: Vec<(String, String)> = include_str!("../../../../symphony-plugin/src/lib.rs")
        .lines()
        .filter_map(|line| {
            let method = line.strip_prefix("    fn ")?;
            let (seam, rest) = method.split_once("(&self) -> Vec<&dyn ")?;
            Some((seam.to_string(), rest.split_once('>')?.0.to_string()))
        })
        .collect();
    assert!(seams.len() >= 14, "{seams:?}");
    let plugin = &seams_traits["Plugin"];
    // The plugin's own methods: each declared, and each the wrapper's.
    for method in plugin {
        assert!(
            declared.get(method).is_some(),
            "Plugin::{method} is not in the declaration"
        );
    }
    assert_eq!(
        &wrapper["impl Plugin for Measured"], plugin,
        "the wrapper's Plugin methods"
    );
    for (seam, name) in &seams {
        let methods = &seams_traits[name];
        let entry = &declared[seam.as_str()][0];
        let declares = |method: &str| match entry {
            serde_json::Value::Object(fields) => fields.contains_key(method),
            serde_json::Value::Null => panic!("EVERY declares no {seam}"),
            _ => DECLARED_BY.contains(&(seam.as_str(), method)),
        };
        let mut asked = false;
        for method in methods {
            let call = calls.contains(format!("{seam}.{method}").as_str());
            asked |= call;
            assert!(
                call || declares(method),
                "{name}::{method} is neither a call of the protocol nor declared"
            );
        }
        match wrapper.get(&format!("impl {name} for On<dyn {name}>")) {
            Some(measured) => assert_eq!(measured, methods, "the wrapper's {name} methods"),
            None => assert!(
                !asked,
                "{name} has calls, and the wrapper hands the plugin's own out unmeasured"
            ),
        }
    }
    // And every call of the protocol is a seam's method, and asked of the test plugin.
    for tag in &calls {
        let (seam, method) = tag.split_once('.').unwrap();
        if seam == "plugin" {
            continue;
        }
        let (_, name) = seams
            .iter()
            .find(|(named, _)| named == seam)
            .unwrap_or_else(|| panic!("{tag}: no such seam"));
        assert!(seams_traits[name].contains(method), "{tag}: no such method");
    }
    let mut every: BTreeSet<&str> = EVERY_CALL.into_iter().collect();
    every.insert("plugin.declare");
    assert_eq!(every, calls, "the test plugin is asked every call");
    for (seam, method) in DECLARED_BY {
        let (_, name) = seams.iter().find(|(named, _)| named == seam).unwrap();
        assert!(seams_traits[name].contains(method), "{seam} {method}");
    }
}

#[test]
fn the_source_reading_finds_a_traits_methods_and_an_impls_and_nothing_else() {
    let source = "\
/// A seam.
pub trait Seam: Send {
    /// Its name.
    fn name(&self) -> &str;
    fn many_lines(
        &self,
    ) -> bool {
        // fn not_a_method()
        true
    }
}

impl Seam for On<dyn Seam> {
    fn name(&self) -> &str {
        \"x\"
    }
}
";
    let set =
        |names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| n.to_string()).collect() };
    assert_eq!(
        traits(source),
        BTreeMap::from([("Seam".to_string(), set(&["name", "many_lines"]))])
    );
    assert_eq!(
        impls(source),
        BTreeMap::from([("impl Seam for On<dyn Seam>".to_string(), set(&["name"]))])
    );
}

/// One call at every seam of [`EVERY`], as the measurement below makes it.
fn every_call(plugins: &Plugins, root: &Path, docs: &Docs) {
    use std::hint::black_box;
    let page = Page::new(root, "docs/A.md");
    let task = TaskRef {
        slug: "sim/jump",
        status: "review",
        branch: "sim/jump-2",
    };
    let plugin = plugins.list()[0];
    let _ = black_box(plugins.render(&page, "every", "lantern", Some("wide")));
    let _ = black_box(
        plugins
            .component("every")
            .map(|c| c.resolve(&page, "a", None)),
    );
    let _ = black_box(plugins.cache_version(&page));
    let kind = plugins.card_kinds()[0];
    with_card("Pick every hull.\n*Slot:* North · cover", |card| {
        let _ = black_box((
            kind.recognises(card),
            kind.pictures(root, card),
            kind.slot(card),
            kind.triage_category(card),
            kind.auto_approvable(card),
            kind.gates(card),
            kind.approval_needs_agent(card),
            kind.chosen(card),
        ));
    });
    let file = File {
        src: "/docs/b.every",
        label: "the hull",
    };
    let _ = black_box(plugin.renderers()[0].draw(&page, &file));
    let _ = black_box(plugins.route("every").map(|r| r.serve(root, "data.json")));
    let _ = black_box(plugins.home_regions()[0].region(root));
    let action = plugins.board_actions()[0];
    let _ = black_box(action.offer(root, &task));
    let submission = Submission {
        payload: "sim/jump@sim/jump-2".into(),
        note: "send it".into(),
    };
    let _ = black_box(action.submit(root, &task, &submission));
    let field = plugins.board_field("every", "effort");
    let _ = black_box(field.map(|f| f.validate(&FieldValue::Text(" 3.5 ".into()))));
    let _ = black_box(plugin.gates()[0].answer(root, &task, Some("c0ffee")));
    let _ = black_box(plugin.gates()[0].kept_while(root, &task));
    let _ = black_box(plugins.watches()[0].changed(root));
    plugins.watches()[0].forget(root);
    let _ = black_box(plugins.doc_checks()[0].check(docs));
    let _ = black_box(plugins.doc_checks()[0].components(docs));
    let _ = black_box(plugins.prose_rules()[0].check("docs/A.md", "# A\n\nevery line\n"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let _ = black_box(plugin.commands()[0].run(&["a".into()], &mut out, &mut err));
    let transport = plugin.transports()[0];
    let _ = black_box((
        transport.register("amber-heron", "/tmp/s.sock"),
        transport.deliver("/tmp/s.sock", "PULL"),
        transport.alive("/tmp/s.sock"),
    ));
}

/// The median of `values`.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Each call's values of `key` in the record at `path`, by `seam.call`.
fn by_call(path: &Path, key: &str) -> std::collections::BTreeMap<String, Vec<f64>> {
    let mut found: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for line in lines(path) {
        if let Some(value) = line[key].as_f64() {
            let seam = line["seam"].as_str().unwrap();
            let call = line["call"].as_str().unwrap();
            found
                .entry(format!("{seam}.{call}"))
                .or_default()
                .push(value);
        }
    }
    found
}

/// What each call of the test plugin costs, timed alone and through the loopback: a measurement,
/// not a check. Run alone, it prints for each call the median of its calls' `us` timed alone, and
/// through the loopback its `us`, its `round_trip_us` and the JSON's bytes, best in a release build:
/// `just test-rs --release -p symphony --test project the_cost_of_each -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check: run it alone with --ignored --nocapture"]
fn the_cost_of_each_call_timed_and_through_the_loopback() {
    const ROUNDS: usize = 1_000;
    let tree = Tree::new();
    tree.write("docs/A.md", "# A\n\nevery word\n");
    tree.write("docs/B.md", "# B\n");
    let docs = Docs::read(&tree.root);
    let (timed_record, looped_record) = (tree.path("timed.jsonl"), tree.path("looped.jsonl"));
    let calls = |timing, loopback| PluginCalls { timing, loopback };
    let timed = Plugins::new(vec![&EVERY]).measured(calls(true, false), &timed_record);
    let looped = Plugins::new(vec![&EVERY]).measured(calls(false, true), &looped_record);
    // Interleaved, so the machine's load weighs on both alike.
    for _ in 0..ROUNDS {
        every_call(&timed, &tree.root, &docs);
        every_call(&looped, &tree.root, &docs);
    }
    let call = by_call(&looped_record, "us");
    let trip = by_call(&looped_record, "round_trip_us");
    let asked = by_call(&looped_record, "request_bytes");
    let answered = by_call(&looped_record, "answer_bytes");
    let median_of = |table: &std::collections::BTreeMap<String, Vec<f64>>, name: &str| {
        median(&mut table.get(name).cloned().unwrap_or_default())
    };
    println!(
        "{ROUNDS} rounds; medians in microseconds\n{:<34} {:>9} {:>9} {:>11} {:>9} {:>8} {:>8}",
        "call", "timed", "looped", "round trip", "json", "req B", "ans B"
    );
    for (name, mut times) in by_call(&timed_record, "us") {
        let (looped_us, round_trip) = (median_of(&call, &name), median_of(&trip, &name));
        println!(
            "{name:<34} {:>9.3} {looped_us:>9.3} {round_trip:>11.3} {:>9.3} {:>8} {:>8}",
            median(&mut times),
            round_trip - looped_us,
            median_of(&asked, &name),
            median_of(&answered, &name),
        );
    }
}
