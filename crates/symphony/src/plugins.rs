//! The plugins a project uses: of the plugins this process holds, the ones its `symphony.toml`
//! names (`[plugins] crates`), in that order. A host binary compiles in the plugins it lists and
//! [`install`]s that list before anything reads a project; every seam is asked through
//! [`Plugins`], so the Symphony's own code names no plugin. A plugin the project's
//! `[plugins.process]` names a command for answers from a process of its own instead, which this
//! process starts ([`process`], docs/engineering/PLUGIN_PROTOCOL.md): a project chooses per plugin. A project
//! whose file turns on `[plugins] timing` or `loopback` has each call into its plugins measured
//! ([`measure`], docs/engineering/PLUGIN_CALLS.md).

use std::path::Path;
use std::sync::{Arc, OnceLock};

use symphony_plugin::vocabulary::{Block, NavItem, Picture, Presentation, Span};
use symphony_plugin::{
    Authority, BoardAction, BoardField, Card, CardKind, Command, Component, DocCheck, Generator,
    HomeRegion, Page, Plugin, ProseRule, Route, ThreadLine, Watch,
};

pub use symphony_plugin::Reply;

use crate::project::{PluginCalls, Project};
use crate::register::{Decision, Who};

pub mod measure;
pub mod process;

/// What this process holds: the plugins its host installed, and how a call into one of them is
/// measured ([`measure::measured`]). The two are installed together, so a binary that installs no
/// plugin (the Symphony's own server and command line) links none of that measuring: above all the
/// loopback's plugin side of the protocol, which reads each request back and writes each answer.
/// A plugin answering from its process needs only the host's side ([`process`]).
struct Held {
    plugins: &'static [&'static dyn Plugin],
    measure: Measure,
}

/// `plugin` with each call into it measured into the record at a path, through the JSON loopback
/// when asked ([`measure::measured`]).
type Measure = fn(&'static dyn Plugin, &Path, bool) -> &'static dyn Plugin;

static COMPILED: OnceLock<Held> = OnceLock::new();

/// Makes `plugins` the ones this process holds, for good: a host installs its list before it reads
/// a project. Installing the same plugin values again changes nothing; another list is refused,
/// naming the plugins already held, and so is another value under a name already held.
pub fn install(plugins: &'static [&'static dyn Plugin]) -> Result<(), String> {
    let held = COMPILED
        .get_or_init(|| Held {
            plugins,
            measure: measure::measured,
        })
        .plugins;
    let names = |list: &[&dyn Plugin]| {
        list.iter()
            .map(|p| p.name().to_string())
            .collect::<Vec<_>>()
    };
    // A value is its place and its type's table of methods: a plugin with no fields has no place of
    // its own, so its table tells it from another. The same list read twice holds the same pointers.
    let same = std::ptr::eq(held, plugins)
        || (held.len() == plugins.len()
            && held
                .iter()
                .zip(plugins)
                .all(|(held, given)| std::ptr::eq(*held, *given)));
    if same {
        Ok(())
    } else if names(held) == names(plugins) {
        Err(format!(
            "this process already holds the plugins [{}]; another value of the same names cannot replace them",
            names(held).join(", ")
        ))
    } else {
        Err(format!(
            "this process already holds the plugins [{}]; it cannot hold [{}] instead",
            names(held).join(", "),
            names(plugins).join(", ")
        ))
    }
}

/// Every plugin this process holds: the list its host installed, none before.
pub fn compiled() -> &'static [&'static dyn Plugin] {
    COMPILED.get().map_or(&[], |held| held.plugins)
}

/// Every command the plugins this process holds add to the `symphony` binary, in their order.
pub fn commands() -> Vec<&'static dyn Command> {
    compiled()
        .iter()
        .flat_map(|plugin| plugin.commands())
        .collect()
}

/// The commands `symphony` runs in a project, and why those of a plugin answering from a process
/// that did not declare are not among them.
pub struct Commands {
    /// Each command, as [`commands_of`] lists them.
    pub listed: Vec<&'static dyn Command>,
    /// For each plugin of the project whose process did not declare, so whose commands are not
    /// known: a line naming it, why and its log.
    pub failed: Vec<String>,
}

/// Every command `symphony` runs in the project at `root` ([`command`]): the commands of its
/// plugins answering from a process, then those of the plugins this process holds that none of its
/// processes answers for, each name once; and the plugins whose process did not declare.
pub fn commands_of(root: Option<&Path>) -> Commands {
    let piped = piped(root);
    let linked = compiled()
        .iter()
        .filter(|plugin| !piped.answers(plugin.name()))
        .flat_map(|plugin| plugin.commands());
    let mut found: Vec<&'static dyn Command> = Vec::new();
    for command in piped.commands.iter().copied().chain(linked) {
        if !found.iter().any(|held| held.name() == command.name()) {
            found.push(command);
        }
    }
    Commands {
        listed: found,
        failed: piped.failed,
    }
}

/// What a project has of plugins answering from a process, for its commands: their commands, run
/// as their programs ([`process`]), the names of the plugins its file gives a process, and a line
/// for each of those whose process did not declare ([`Commands::failed`]).
#[derive(Default)]
struct Piped {
    commands: Vec<&'static dyn Command>,
    names: Vec<String>,
    failed: Vec<String>,
}

impl Piped {
    /// Whether the plugin `name` answers from its process here, so a plugin of that name this
    /// process holds does not.
    fn answers(&self, name: &str) -> bool {
        self.names.iter().any(|piped| piped == name)
    }
}

/// The plugins answering from a process of the project at `root` ([`Piped`]): none without a root
/// or when its file names no `[plugins.process]`, whose names are read alone and quietly first
/// ([`PluginProcess::named_at`](crate::project::PluginProcess::named_at)), so they hold even while
/// the project does not load. A project that does not load, as when a process it needs for a
/// renderer did not declare, has no plugin's command: why is among the failed lines.
fn piped(root: Option<&Path>) -> Piped {
    let Some(root) = root else {
        return Piped::default();
    };
    let names = crate::project::PluginProcess::named_at(root);
    if names.is_empty() {
        return Piped::default();
    }
    let project = Project::of(root);
    let mut piped = Piped {
        names,
        failed: Project::problem(root)
            .map(|why| {
                format!("the project does not load, so its plugins' commands are not known: {why}")
            })
            .into_iter()
            .collect(),
        ..Piped::default()
    };
    for plugin in Plugins::for_project(&project).list() {
        let Some(remote) = process::remote(*plugin) else {
            continue;
        };
        match remote.declaration() {
            Ok(_) => piped.commands.extend(plugin.commands()),
            Err(why) => piped.failed.push(format!(
                "the plugin {}'s process failed, so its commands are not known: {why} (its log: \
                 {})",
                plugin.name(),
                remote.process().log.display()
            )),
        }
    }
    piped
}

/// The command `name` `symphony` runs in the project at `root`: one of a plugin of the project
/// answering from its process, run as its program; else that of the first plugin this process
/// holds that adds one, measured as the project at `root` says when there is one
/// ([`Plugins::at`]). Whether the project names a plugin's process, and its two settings, are read
/// alone and quietly first ([`PluginProcess::named_at`](crate::project::PluginProcess::named_at),
/// [`PluginCalls::read`]): with none of them, the command runs as it would with neither setting,
/// the project neither loaded nor checked and nothing said of its file. None found is
/// [`Commands::failed`]'s lines, empty when no plugin's process failed.
pub fn command(name: &str, root: Option<&Path>) -> Result<&'static dyn Command, Vec<String>> {
    let piped = piped(root);
    if let Some(found) = piped.commands.iter().copied().find(|c| c.name() == name) {
        return Ok(found);
    }
    let Some(plugin) = compiled()
        .iter()
        .copied()
        .filter(|plugin| !piped.answers(plugin.name()))
        .find(|plugin| plugin.commands().iter().any(|c| c.name() == name))
    else {
        return Err(piped.failed);
    };
    let plugin = match root {
        Some(root) if PluginCalls::read(root).recorded() => {
            let project = Project::of(root);
            Plugins::new(vec![plugin])
                .measured_as_held(project.plugin_calls, &record(&project, root))
                .list()[0]
        }
        _ => plugin,
    };
    plugin
        .commands()
        .into_iter()
        .find(|command| command.name() == name)
        .ok_or_else(Vec::new)
}

/// The record of `project`'s plugin calls, its checkout at `root`.
pub fn record(project: &Project, root: &Path) -> std::path::PathBuf {
    root.join(project.artifact(measure::RECORD))
}

/// A project's plugins.
#[derive(Clone, Default)]
pub struct Plugins {
    list: Arc<Vec<&'static dyn Plugin>>,
    /// What one register read had of the plugins answering from a process, every card kind of
    /// every card in one go ([`Plugins::reading`]): their card kinds answer from it.
    read: Arc<Vec<process::Read>>,
}

impl Plugins {
    pub fn new(plugins: Vec<&'static dyn Plugin>) -> Self {
        Self {
            list: Arc::new(plugins),
            read: Arc::default(),
        }
    }

    /// These plugins for one read of the registers `docs`, in the checkout at `root`: each plugin
    /// answering from a process is asked every card kind's answers of every card of `docs` in one
    /// request, or as many as keep each line within the protocol's bound, and its card kinds answer
    /// those cards from it (a register read costs a request, not a round trip for each call of each
    /// card). Each card is sent as the read asks it: with its document, and with none, as the
    /// card's slot, choice and thread are asked. Any other card is asked card by card. A linked
    /// plugin, or one whose process speaks a minor before the batch, is asked as it is.
    #[must_use]
    pub fn reading(&self, root: &Path, docs: &[crate::board::Doc]) -> Plugins {
        let remotes: Vec<&'static process::Remote> = self
            .list
            .iter()
            .filter_map(|plugin| process::remote(*plugin))
            .collect();
        if remotes.is_empty() {
            return self.clone();
        }
        // A card twice (two alike in one document) is asked twice and answered alike.
        let mut cards = Vec::new();
        for doc in docs {
            for decision in &doc.decisions {
                for on in [doc.path.as_str(), ""] {
                    cards.push(with_card(
                        decision,
                        on,
                        symphony_plugin::protocol::CardData::of,
                    ));
                }
            }
        }
        Plugins {
            list: self.list.clone(),
            read: Arc::new(
                remotes
                    .into_iter()
                    .filter_map(|remote| remote.read_cards(root, &cards))
                    .collect(),
            ),
        }
    }

    /// The plugins `project` names, in its order, as they are: for what they declare (a seam
    /// object's name, the navigation, the generators, the theme). Each is the compiled plugin of
    /// its name, or the one answering from its process when `[plugins.process]` names a command
    /// for it ([`process::made`]); in a branch's checkout, the main checkout's process
    /// ([`process::answering`]). A name this build has no plugin for is left out here and
    /// reported by [`problems`]. A call into a seam goes through [`Plugins::at`] or
    /// [`Plugins::of`], which measure it.
    pub fn for_project(project: &Project) -> Self {
        Self::new(
            project
                .plugins
                .iter()
                .filter_map(|name| named(project, name))
                .collect(),
        )
    }

    /// The plugins of `project`, its checkout at `root`, each call into them measured as its
    /// `[plugins] timing` and `loopback` say into its record ([`record`]); as they are when both
    /// are off. A plugin answering from its process records its own calls, as a process call.
    pub fn at(project: &Project, root: &Path) -> Self {
        let plugins = Self::for_project(project);
        let record = project
            .plugin_calls
            .recorded()
            .then(|| record(project, root));
        for plugin in plugins.list() {
            if let Some(remote) = process::remote(*plugin) {
                remote.record_into(record.as_deref().map(measure::Record::at));
            }
        }
        match record {
            Some(record) => plugins.measured_as_held(project.plugin_calls, &record),
            None => plugins,
        }
    }

    /// The plugins of the project at `root` ([`Plugins::at`]).
    pub fn of(root: &Path) -> Self {
        Self::at(&Project::of(root), root)
    }

    /// These plugins, each call into them measured as `calls` says into the record at `record`
    /// ([`measure`]); these same plugins when `calls` records nothing. A plugin answering from its
    /// process is left as it is: it records its own calls ([`Plugins::at`]).
    pub fn measured(self, calls: PluginCalls, record: &Path) -> Self {
        self.measured_by(measure::measured, calls, record)
    }

    /// [`Plugins::measured`], by the measuring installed with the plugins this process holds
    /// ([`install`]), which these are among: none is held while none is installed, and then each
    /// of these answers from its process, which records its own calls.
    fn measured_as_held(self, calls: PluginCalls, record: &Path) -> Self {
        match COMPILED.get() {
            Some(held) => self.measured_by(held.measure, calls, record),
            None => self,
        }
    }

    fn measured_by(self, measure: Measure, calls: PluginCalls, record: &Path) -> Self {
        if !calls.recorded() {
            return self;
        }
        Self::new(
            self.list
                .iter()
                .map(|plugin| match process::remote(*plugin) {
                    Some(_) => *plugin,
                    None => measure(*plugin, record, calls.loopback),
                })
                .collect(),
        )
    }

    pub fn list(&self) -> &[&'static dyn Plugin] {
        &self.list
    }

    /// The component called `name`: the built-in `task`, else the first plugin's that has one.
    pub fn component(&self, name: &str) -> Option<&dyn Component> {
        if name == TASK.name() {
            return Some(&TASK);
        }
        self.list
            .iter()
            .flat_map(|plugin| plugin.components())
            .find(|component| component.name() == name)
    }

    /// Every component's name: the built-in `task`, then the plugins'.
    pub fn component_names(&self) -> Vec<String> {
        std::iter::once(TASK.name().to_string())
            .chain(
                self.list
                    .iter()
                    .flat_map(|plugin| plugin.components())
                    .map(|component| component.name().to_string()),
            )
            .collect()
    }

    pub fn card_kinds(&self) -> Vec<&dyn CardKind> {
        self.list
            .iter()
            .flat_map(
                |plugin| match self.read.iter().find(|read| read.of(*plugin)) {
                    Some(read) => read.card_kinds(),
                    None => plugin.card_kinds(),
                },
            )
            .collect()
    }

    pub fn route(&self, path: &str) -> Option<&dyn Route> {
        self.list
            .iter()
            .flat_map(|p| p.routes())
            .find(|route| route.path() == path)
    }

    pub fn nav(&self) -> Vec<NavItem> {
        self.list.iter().flat_map(|p| p.nav()).collect()
    }

    pub fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        self.list.iter().flat_map(|p| p.home_regions()).collect()
    }

    pub fn board_actions(&self) -> Vec<&dyn BoardAction> {
        self.list.iter().flat_map(|p| p.board_actions()).collect()
    }

    /// The field `name` of the plugin `plugin`.
    pub fn board_field(&self, plugin: &str, name: &str) -> Option<&dyn BoardField> {
        self.list
            .iter()
            .find(|p| p.name() == plugin)?
            .board_fields()
            .into_iter()
            .find(|field| field.name() == name)
    }

    /// The approval authority answering for `adapter`: the first plugin's that does, for the
    /// process's life, so it can be asked on a thread of its own.
    pub fn authority(&self, adapter: &str) -> Option<&'static dyn Authority> {
        self.list.iter().find_map(|plugin| {
            let plugin: &'static dyn Plugin = *plugin;
            (plugin.authorities().into_iter()).find(|authority| authority.adapter() == adapter)
        })
    }

    pub fn watches(&self) -> Vec<&dyn Watch> {
        self.list.iter().flat_map(|p| p.watches()).collect()
    }

    pub fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        self.list.iter().flat_map(|p| p.doc_checks()).collect()
    }

    pub fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        self.list.iter().flat_map(|p| p.prose_rules()).collect()
    }

    pub fn generators(&self) -> Vec<&dyn Generator> {
        self.list.iter().flat_map(|p| p.generators()).collect()
    }

    /// The theme package called `package`: the first plugin's that provides one by that name.
    pub fn theme(&self, package: &str) -> Option<symphony_plugin::Theme> {
        self.list
            .iter()
            .filter_map(|p| p.theme())
            .find(|theme| theme.package == package)
    }

    /// The component `name` for `argument` and `option` on `page`, its problem shown in its place.
    pub fn render(
        &self,
        page: &Page<'_>,
        name: &str,
        argument: &str,
        option: Option<&str>,
    ) -> Vec<Block> {
        match self.component(name) {
            Some(component) => component
                .render(page, argument, option)
                .unwrap_or_else(|problem| vec![error(&problem)]),
            None => vec![error(&format!("unknown component {name}"))],
        }
    }

    /// A version of every component's inputs on `page`, or `None` when one has none: then a
    /// document is rendered afresh on every read.
    pub fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        let mut version = String::new();
        for component in self.list.iter().flat_map(|plugin| plugin.components()) {
            version.push_str(component.name());
            version.push('=');
            version.push_str(&component.cache_version(page)?);
            version.push(';');
        }
        Some(version)
    }
}

/// The process answering for the plugin `name` of `project`, when its `[plugins.process]` names a
/// command for it ([`process::answering`]).
fn answering(project: &Project, name: &str) -> Option<crate::project::PluginProcess> {
    process::answering(name, project.plugin_processes.get(name)?)
}

/// The plugin `name` of `project` as it answers: from its process ([`answering`]), else the
/// compiled plugin of that name, else none.
fn named(project: &Project, name: &str) -> Option<&'static dyn Plugin> {
    match answering(project, name) {
        Some(started) => Some(process::made(name, &started)),
        None => compiled().iter().find(|p| p.name() == name).copied(),
    }
}

/// What in `project` names a plugin this build does not hold and no process answers for, a
/// plugin's process that does not start or declare, and a plugin's generator in a project that
/// names no land command: a generated branch never moves the default branch unchecked.
pub fn problems(project: &Project) -> Vec<String> {
    let mut found: Vec<String> = project
        .plugins
        .iter()
        .filter(|name| named(project, name).is_none())
        .map(|name| {
            format!(
                "{}: [plugins] crates names {name}, which this build does not hold",
                crate::project::FILE
            )
        })
        .collect();
    for name in project.plugin_processes.keys() {
        let Some(started) = answering(project, name) else {
            continue;
        };
        if let Err(why) = process::made(name, &started).declaration() {
            found.push(format!(
                "{}: the plugin {name}: {why}",
                crate::project::FILE
            ));
        }
    }
    found.extend(unlanded(project, &Plugins::for_project(project)));
    found.extend(crate::theme::problems(project));
    found
}

/// Each generator of `plugins` in `project` when it names no land command.
pub fn unlanded(project: &Project, plugins: &Plugins) -> Vec<String> {
    if !project.verdicts.land.is_empty() {
        return Vec::new();
    }
    plugins
        .generators()
        .iter()
        .map(|generator| {
            format!(
                "{}: the plugin generator {} needs a [verdicts] land command",
                crate::project::FILE,
                generator.name()
            )
        })
        .collect()
}

/// A component's problem, shown where it would be: the plugin kit's, which a plugin's own tests
/// compare with.
pub use symphony_plugin::error;

/// `[[task:sim/jump|label]]`: a link to a task on the board. It reads only its argument.
struct Task;

static TASK: Task = Task;

impl Component for Task {
    fn name(&self) -> &str {
        "task"
    }

    fn render(
        &self,
        _page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        let slug = argument.trim();
        let label = option.filter(|label| !label.is_empty()).unwrap_or(slug);
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Styled {
                tag: symphony_plugin::vocabulary::InlineKind::Span,
                presentation: Presentation {
                    class: "tasklink".into(),
                    attributes: [("data-task".to_string(), slug.to_string())].into(),
                    ..Presentation::default()
                },
                spans: vec![Span::Link {
                    href: format!("/task/{slug}"),
                    target: Some(symphony_plugin::vocabulary::LinkTarget {
                        path: format!("task/{slug}"),
                        anchor: None,
                    }),
                    title: String::new(),
                    spans: vec![Span::Text { text: label.into() }],
                }],
            }],
        }])
    }

    fn cache_version(&self, _page: &Page<'_>) -> Option<String> {
        Some(String::new())
    }
}

/// `decision`, a card of the document `doc`, as card kinds read it.
pub fn with_card<R>(decision: &Decision, doc: &str, read: impl FnOnce(&Card<'_>) -> R) -> R {
    let entries = decision.entries();
    let thread: Vec<ThreadLine<'_>> = entries
        .iter()
        .map(|entry| ThreadLine {
            agent: entry.who == Who::Agent,
            verdict: entry.verdict.as_deref(),
            text: &entry.text,
        })
        .collect();
    read(&Card {
        slug: &decision.slug,
        doc,
        topic: &decision.topic,
        text: &decision.text,
        thread: &thread,
    })
}

/// What the project's card kinds say of a card: a kind is read from the card's own text, wherever
/// it sits, and only the kinds of the plugins the project names read it.
impl Plugins {
    /// The slot a picture card fills (`North · cover`), as the first card kind that names one
    /// says.
    pub fn slot(&self, decision: &Decision) -> Option<String> {
        with_card(decision, "", |card| {
            self.card_kinds().iter().find_map(|kind| kind.slot(card))
        })
    }

    /// The markers every card kind of these plugins reads in a card's text, by name.
    pub fn markers(&self) -> Vec<&str> {
        self.card_kinds()
            .into_iter()
            .flat_map(|kind| kind.markers().iter().copied())
            .collect()
    }

    /// The pictures the board shows on `decision`, a card of `doc` in the checkout at `root`.
    pub fn pictures(&self, root: &Path, doc: &str, decision: &Decision) -> Vec<Picture> {
        with_card(decision, doc, |card| {
            self.card_kinds()
                .iter()
                .map(|kind| kind.pictures(root, card))
                .find(|found| !found.is_empty())
                .unwrap_or_default()
        })
    }

    /// Whether the owner's approval of `decision` waits for an agent to apply it.
    pub fn approval_needs_agent(&self, decision: &Decision) -> bool {
        with_card(decision, "", |card| {
            self.card_kinds()
                .iter()
                .any(|kind| kind.approval_needs_agent(card))
        })
    }

    /// The triage's category for `decision`, a card of `doc`, as the first kind to give one says.
    pub fn triage_category(&self, doc: &str, decision: &Decision) -> Option<String> {
        with_card(decision, doc, |card| {
            self.card_kinds()
                .iter()
                .find_map(|kind| kind.triage_category(card))
        })
    }

    /// The letter of the candidate an approval of `decision` chose, as the first kind to answer
    /// says, or why its thread cannot be read.
    pub fn chosen(&self, decision: &Decision) -> Result<Option<String>, String> {
        with_card(decision, "", |card| {
            for kind in self.card_kinds() {
                if let Some(letter) = kind.chosen(card)? {
                    return Ok(Some(letter));
                }
            }
            Ok(None)
        })
    }

    /// Whether every kind lets a predicted approval stand for the owner's on `decision`.
    pub fn auto_approvable(&self, decision: &Decision) -> bool {
        with_card(decision, "", |card| {
            self.card_kinds()
                .iter()
                .all(|kind| kind.auto_approvable(card))
        })
    }

    /// What `decision`, a card of `doc`, gates as the kinds count it: (how many, the word for one),
    /// in the kinds' order.
    pub fn gates(&self, doc: &str, decision: &Decision) -> Vec<(usize, String)> {
        with_card(decision, doc, |card| {
            self.card_kinds()
                .iter()
                .filter_map(|kind| kind.gates(card))
                .collect()
        })
    }
}
