//! A plugin with one object at every seam, shared by the seam tests, which link it, and by the
//! `symphony-test-plugin` binary, which serves it from a process of its own
//! (`[plugins.process]`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use symphony_plugin::authority::{Operation, Signed};
use symphony_plugin::vocabulary::{
    Block, EntryNeed, EntryState, Gate, GatePast, GateState, LinkTarget, NavItem, Picture,
    Presentation, ProgressBar, Region, Setting, SettingClass, SettingKind, SettingScope,
    SettingSource, SettingsEntry, Span, TaskOffer, View, Viewer,
};
use symphony_plugin::{
    Authority, BoardAction, BoardField, Card, CardKind, Command, Component, Describer, DocCheck,
    Docs, Drawing, FieldKind, File, Gates, Generator, HomeRegion, Page, Plugin, Problem, ProseRule,
    Renderer, Reply, Route, Submission, Submitted, TaskRef, Theme, Transport, Watch,
};

/// A plugin with one object at every seam, each answer made of every argument it is given, so a
/// request that lost or changed a field answers otherwise.
pub(super) struct Every {
    /// The root of each page whose shared data was loaded, once a load.
    pub(super) loads: Mutex<Vec<PathBuf>>,
    /// The roots its watch was told to forget.
    pub(super) forgot: Mutex<Vec<PathBuf>>,
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
    fn authorities(&self) -> Vec<&dyn Authority> {
        vec![self]
    }
    fn describers(&self) -> Vec<&dyn Describer> {
        vec![self, &UNREAD]
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
            tokens: std::collections::BTreeMap::from([
                ("primary".into(), "red".into()),
                ("background".into(), "white".into()),
                ("unsafe".into(), "url(remote)".into()),
            ]),
            layouts: std::collections::BTreeMap::from([
                ("header".into(), "compact".into()),
                ("home".into(), "default".into()),
                ("unknown".into(), "wide".into()),
            ]),
            ..Theme::default()
        })
    }
}

/// [`EVERY`] as a plugin's process serves it: its name and each seam a process answers (its
/// components, card kind, renderer, route, navigation, home region, board action, gates answerer,
/// approval authority, Settings page describers, watch, doc check, command and theme), and a
/// component and a gates answerer that take as long as they are told.
pub(super) struct Piped;

pub(super) static PIPED: Piped = Piped;

impl Plugin for Piped {
    fn name(&self) -> &str {
        "every"
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![&EVERY, &AGAIN, &SLOW]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![&EVERY]
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        EVERY.renderers()
    }
    fn routes(&self) -> Vec<&dyn Route> {
        EVERY.routes()
    }
    fn nav(&self) -> Vec<NavItem> {
        EVERY.nav()
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        EVERY.home_regions()
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        EVERY.board_actions()
    }
    fn gates(&self) -> Vec<&dyn Gates> {
        vec![&EVERY, &SLOW]
    }
    fn authorities(&self) -> Vec<&dyn Authority> {
        EVERY.authorities()
    }
    fn describers(&self) -> Vec<&dyn Describer> {
        EVERY.describers()
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        EVERY.watches()
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        EVERY.doc_checks()
    }
    fn commands(&self) -> Vec<&dyn Command> {
        EVERY.commands()
    }
    fn theme(&self) -> Option<Theme> {
        EVERY.theme()
    }
}

/// A component that spends its argument's microseconds before it answers, so a measurement shows
/// its meters move by a known time; `hang` it never answers in a test's time, the file
/// [`HANGING`] written in the page's root first, so a test sees the call is in it. As a gates
/// answerer it meets its gate at the tip it is asked, and at the tip `hang` hangs the same way.
struct Slow;

static SLOW: Slow = Slow;

/// What `slow hang` writes in its page's root once the call is in the component.
pub const HANGING: &str = "slow-hanging";

impl Component for Slow {
    fn name(&self) -> &str {
        "slow"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        _: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if argument == "hang" {
            hang(page.root);
        }
        let micros: u64 = argument
            .parse()
            .map_err(|_| format!("slow takes microseconds, not {argument:?}"))?;
        let started = std::time::Instant::now();
        // Spent, not slept: a sleep ends when the scheduler says, later than asked.
        while started.elapsed() < std::time::Duration::from_micros(micros) {
            std::hint::spin_loop();
        }
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("slow {micros}"),
            }],
        }])
    }
}

/// `hang` in `root`: [`HANGING`] written there, so a test sees the call is in it, then never an
/// answer in a test's time.
fn hang(root: &Path) {
    // A root that is no folder (a test's `/r`) takes no mark, and the call hangs all the same.
    let _ = std::fs::write(root.join(HANGING), "");
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test plugin hangs by a length of time, not until its test releases it"
    )]
    std::thread::sleep(std::time::Duration::from_secs(600));
}

impl Gates for Slow {
    fn name(&self) -> &str {
        "slow"
    }
    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String> {
        if tip == Some("hang") {
            hang(root);
        }
        Ok(vec![Gate {
            name: "slow".into(),
            state: GateState::Met,
            by: Some(task.slug.into()),
            commit: tip.map(str::to_string),
            ..Gate::default()
        }])
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
                    text: format!("{argument} {option:?} in {}", page.artifact("every")),
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

/// A second component of the plugin, reading the page's shared data too; asked for `loads`, it
/// also says how many times its process loaded that data, so a test of a plugin's own process sees
/// one page's components share a load.
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
        let mut text = format!("{argument} again on {}", shared(page));
        if argument == "loads" {
            let loads = EVERY.loads.lock().unwrap().len();
            text.push_str(&format!(", loaded {loads} times"));
        }
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text { text }],
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

/// The file in a project's root whose text is the shade [`EVERY`]'s entry says: what shows its
/// describer reads the checkout it is asked of.
pub const SHADE: &str = "every-shade";

/// What [`EVERY`]'s describer answers for the project at `root`: an entry of every part, with a
/// setting of its own, a preference of the machine's scope, and the land's delivery, a protected key,
/// which it calls a preference too.
pub fn described(root: &Path) -> SettingsEntry {
    let shade = std::fs::read_to_string(root.join(SHADE)).unwrap_or_else(|_| "plain".into());
    let source = |said: &str, file: &str, line: u32| SettingSource {
        scope: Some(SettingScope::Project),
        said: said.into(),
        file: Some(file.into()),
        line: Some(line),
        over: None,
    };
    SettingsEntry {
        id: "anything".into(),
        name: "Every widget".into(),
        about: "Draws every widget the project holds.".into(),
        state: Some(EntryState {
            word: "On".into(),
            on: Some(true),
            why: format!("Its shade is {}, read in {}.", shade.trim(), root.display()),
        }),
        needs: vec![EntryNeed {
            what: "the widget folder".into(),
            met: root.join("widgets").is_dir(),
            said: "widgets/ in the checkout".into(),
        }],
        settings: vec![
            Setting {
                key: "[every] shade".into(),
                name: "The widgets' shade".into(),
                kind: SettingKind::Choice,
                value: shade.trim().into(),
                choices: vec!["plain".into(), "bright".into()],
                source: source("every.toml [every] shade, line 1", "every.toml", 1),
                scopes: vec![SettingScope::Project, SettingScope::Machine],
                class: SettingClass::Preference,
            },
            Setting {
                key: "[land] delivery".into(),
                name: "How a land is delivered".into(),
                kind: SettingKind::Text,
                value: "pull_request".into(),
                choices: Vec::new(),
                source: source("symphony.toml [land] delivery, line 3", "symphony.toml", 3),
                scopes: vec![SettingScope::Project, SettingScope::Machine],
                class: SettingClass::Preference,
            },
        ],
        source: None,
    }
}

impl Describer for Every {
    fn id(&self) -> &str {
        "widgets"
    }
    fn describe(&self, root: &Path) -> Result<SettingsEntry, String> {
        Ok(described(root))
    }
}

/// A describer that cannot say.
struct Unread;

static UNREAD: Unread = Unread;

impl Describer for Unread {
    fn id(&self) -> &str {
        "unread"
    }
    fn describe(&self, _: &Path) -> Result<SettingsEntry, String> {
        Err("every.toml does not read".into())
    }
}

impl Authority for Every {
    fn adapter(&self) -> &str {
        "every"
    }
    /// The operation signed whole, with the root and the time it was given: none at all it
    /// cannot say.
    fn attest(
        &self,
        root: &Path,
        operation: &Operation,
        within: Duration,
    ) -> Result<Signed, String> {
        if within.is_zero() {
            return Err(format!(
                "{} of {} has no time in {}",
                operation.guard,
                operation.task,
                root.display()
            ));
        }
        Ok(Signed::new(
            format!("{operation:?} in {} within {within:?}", root.display()),
            format!("{:0128x}", operation.snapshot.task_revision),
        ))
    }
}

impl Watch for Every {
    fn paths(&self) -> Vec<String> {
        vec!["data/every".into(), "pictures".into()]
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
