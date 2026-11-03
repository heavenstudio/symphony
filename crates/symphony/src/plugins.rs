//! The plugins a project uses: of the plugins compiled into this build, the ones its
//! `symphony.toml` names (`[plugins] crates`), in that order. Every seam is asked through
//! [`Plugins`], so the Symphony's own code names no plugin.

use std::path::Path;
use std::sync::Arc;

use symphony_plugin::vocabulary::{Block, NavItem, Picture, Presentation, Span};
use symphony_plugin::{
    BoardAction, BoardField, Card, CardKind, Component, DocCheck, Generator, HomeRegion, Page,
    Plugin, ProseRule, Route, ThreadLine, Watch,
};

pub use symphony_plugin::Reply;

use crate::project::Project;
use crate::register::{Decision, Who};

/// Every plugin this build holds.
pub fn compiled() -> Vec<&'static dyn Plugin> {
    vec![&crate::star_foundry::PLUGIN]
}

/// A project's plugins.
#[derive(Clone, Default)]
pub struct Plugins(Arc<Vec<&'static dyn Plugin>>);

impl Plugins {
    pub fn new(plugins: Vec<&'static dyn Plugin>) -> Self {
        Self(Arc::new(plugins))
    }

    /// The compiled plugins `project` names, in its order. A name this build has no plugin for is
    /// left out here and reported by [`problems`].
    pub fn for_project(project: &Project) -> Self {
        let compiled = compiled();
        Self::new(
            project
                .plugins
                .iter()
                .filter_map(|name| compiled.iter().find(|p| p.name() == name).copied())
                .collect(),
        )
    }

    /// The plugins of the project at `root`.
    pub fn of(root: &Path) -> Self {
        Self::for_project(&Project::of(root))
    }

    pub fn list(&self) -> &[&'static dyn Plugin] {
        &self.0
    }

    /// The component called `name`: the built-in `task`, else the first plugin's that has one.
    pub fn component(&self, name: &str) -> Option<&dyn Component> {
        if name == TASK.name() {
            return Some(&TASK);
        }
        self.0
            .iter()
            .flat_map(|plugin| plugin.components())
            .find(|component| component.name() == name)
    }

    /// Every component's name: the built-in `task`, then the plugins'.
    pub fn component_names(&self) -> Vec<String> {
        std::iter::once(TASK.name().to_string())
            .chain(
                self.0
                    .iter()
                    .flat_map(|plugin| plugin.components())
                    .map(|component| component.name().to_string()),
            )
            .collect()
    }

    pub fn card_kinds(&self) -> Vec<&dyn CardKind> {
        self.0.iter().flat_map(|p| p.card_kinds()).collect()
    }

    pub fn route(&self, path: &str) -> Option<&dyn Route> {
        self.0
            .iter()
            .flat_map(|p| p.routes())
            .find(|route| route.path() == path)
    }

    pub fn nav(&self) -> Vec<NavItem> {
        self.0.iter().flat_map(|p| p.nav()).collect()
    }

    pub fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        self.0.iter().flat_map(|p| p.home_regions()).collect()
    }

    pub fn board_actions(&self) -> Vec<&dyn BoardAction> {
        self.0.iter().flat_map(|p| p.board_actions()).collect()
    }

    /// The field `name` of the plugin `plugin`.
    pub fn board_field(&self, plugin: &str, name: &str) -> Option<&dyn BoardField> {
        self.0
            .iter()
            .find(|p| p.name() == plugin)?
            .board_fields()
            .into_iter()
            .find(|field| field.name() == name)
    }

    pub fn watches(&self) -> Vec<&dyn Watch> {
        self.0.iter().flat_map(|p| p.watches()).collect()
    }

    pub fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        self.0.iter().flat_map(|p| p.doc_checks()).collect()
    }

    pub fn prose_rules(&self) -> Vec<&dyn ProseRule> {
        self.0.iter().flat_map(|p| p.prose_rules()).collect()
    }

    pub fn generators(&self) -> Vec<&dyn Generator> {
        self.0.iter().flat_map(|p| p.generators()).collect()
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
        for component in self.0.iter().flat_map(|plugin| plugin.components()) {
            version.push_str(component.name());
            version.push('=');
            version.push_str(&component.cache_version(page)?);
            version.push(';');
        }
        Some(version)
    }
}

/// What in `project` names a plugin this build does not hold.
pub fn problems(project: &Project) -> Vec<String> {
    let compiled = compiled();
    project
        .plugins
        .iter()
        .filter(|name| !compiled.iter().any(|p| p.name() == name.as_str()))
        .map(|name| {
            format!(
                "{}: [plugins] crates names {name}, which this build does not hold",
                crate::project::FILE
            )
        })
        .collect()
}

/// A component's problem, shown where it would be.
pub fn error(problem: &str) -> Block {
    Block::Paragraph {
        spans: vec![Span::Styled {
            tag: symphony_plugin::vocabulary::InlineKind::Span,
            presentation: Presentation {
                class: "component-error".into(),
                title: "component error".into(),
                ..Presentation::default()
            },
            spans: vec![Span::Text {
                text: format!("⚠ {problem}"),
            }],
        }],
    }
}

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

/// The card kinds of every compiled plugin: a card's kind is read from its own text, wherever it
/// sits.
pub fn card_kinds() -> Vec<&'static dyn CardKind> {
    compiled()
        .into_iter()
        .flat_map(|p| p.card_kinds())
        .collect()
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

/// The slot a picture card fills (`Republic · ship`), as the first card kind that names one says.
pub fn slot(decision: &Decision) -> Option<String> {
    with_card(decision, "", |card| {
        card_kinds().iter().find_map(|kind| kind.slot(card))
    })
}

/// The pictures the board shows on `decision`, a card of `doc` in the checkout at `root`.
pub fn pictures(root: &Path, doc: &str, decision: &Decision) -> Vec<Picture> {
    with_card(decision, doc, |card| {
        card_kinds()
            .iter()
            .map(|kind| kind.pictures(root, card))
            .find(|found| !found.is_empty())
            .unwrap_or_default()
    })
}

/// Whether the owner's approval of `decision` waits for an agent to apply it.
pub fn approval_needs_agent(decision: &Decision) -> bool {
    with_card(decision, "", |card| {
        card_kinds()
            .iter()
            .any(|kind| kind.approval_needs_agent(card))
    })
}
