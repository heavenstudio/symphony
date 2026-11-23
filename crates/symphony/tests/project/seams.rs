//! Each seam asked through `Plugins`, with fixture plugins: which plugin answers, in what order, and
//! what a project without plugins gets.

use std::path::Path;

use symphony::landing::Generator as Generated;
use symphony::plugins::{Plugins, Reply};
use symphony::project::Project;
use symphony_plugin::vocabulary::{
    Block, FieldValue, InlineKind, NavItem, ProgressBar, Region, Span, TaskOffer,
};
use symphony_plugin::{
    BoardAction, BoardField, Card, CardKind, Component, DocCheck, Docs, FieldKind, Generator,
    HomeRegion, Page, Plugin, Problem, ProseRule, Route, Submission, Submitted, TaskRef, Watch,
    check_kind,
};

use super::Tree;

mod calls;
mod compiled;
mod described;
mod every;
mod process;
mod renderers;

/// A plugin that only has a name: every seam takes its default.
struct Bare;

impl Plugin for Bare {
    fn name(&self) -> &str {
        "bare"
    }
}

/// A plugin with one of everything, each answering with `word` so a test sees which plugin spoke.
struct Fixture {
    plugin: &'static str,
    word: &'static str,
    versioned: bool,
    /// The card markers its kind reads.
    markers: &'static [&'static str],
}

static FIRST: Fixture = Fixture {
    plugin: "first",
    word: "one",
    versioned: true,
    markers: &["One"],
};
static SECOND: Fixture = Fixture {
    plugin: "second",
    word: "two",
    versioned: true,
    markers: &["Two"],
};
static UNVERSIONED: Fixture = Fixture {
    plugin: "unversioned",
    word: "live",
    versioned: false,
    markers: &[],
};

impl Plugin for Fixture {
    fn name(&self) -> &str {
        self.plugin
    }
    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
    fn routes(&self) -> Vec<&dyn Route> {
        vec![self]
    }
    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: self.word.into(),
            title: self.word.to_uppercase(),
            href: format!("/{}", self.word),
            document: format!("docs/{}.md", self.word),
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
    fn renderers(&self) -> Vec<&dyn symphony_plugin::Renderer> {
        vec![self]
    }
}

impl Component for Fixture {
    fn name(&self) -> &str {
        "word"
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        _: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if argument == "bad" {
            return Err(format!("{} refuses bad", self.word));
        }
        Ok(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("{} {argument} on {}", self.word, page.doc_path),
            }],
        }])
    }
    fn cache_version(&self, _: &Page<'_>) -> Option<String> {
        self.versioned.then(|| self.word.to_string())
    }
}

impl CardKind for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        card.text.contains(self.word)
    }
    fn markers(&self) -> &[&str] {
        self.markers
    }
}

impl Route for Fixture {
    fn path(&self) -> &str {
        self.word
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        match rest {
            "data" => Some(Reply::Json(format!("{{\"{}\":1}}", self.word))),
            "" => None,
            file => Some(Reply::File(root.join(file))),
        }
    }
}

impl HomeRegion for Fixture {
    fn id(&self) -> &str {
        self.word
    }
    fn region(&self, _: &Path) -> Option<Region> {
        Some(Region {
            id: self.word.into(),
            title: self.word.into(),
            note: String::new(),
            bars: vec![ProgressBar {
                key: "k".into(),
                name: "n".into(),
                page: "docs/A.md".into(),
                title: String::new(),
                done: 1,
                waiting: 0,
                total: 2,
            }],
            legend: ["done".into(), "waiting".into(), "left".into()],
        })
    }
}

impl BoardAction for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn offer(&self, _: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        Ok(TaskOffer {
            action: self.word.into(),
            label: format!("{} it", self.word),
            branch: task.branch.into(),
            payload: Some(format!("{}:{}", self.word, task.branch)),
        })
    }
    fn submit(
        &self,
        _: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        if submission.payload != format!("{}:{}", self.word, task.branch) {
            return Err(format!("{} refuses a stale offer", self.word));
        }
        Ok(Submitted {
            said: format!("{} {}: {}", self.word, task.branch, submission.note),
            released: vec![task.slug.into()],
        })
    }
}

impl BoardField for Fixture {
    fn name(&self) -> &str {
        "effort"
    }
    fn kind(&self) -> FieldKind {
        FieldKind::Number
    }
}

impl Watch for Fixture {
    fn paths(&self) -> Vec<String> {
        vec![format!("data/{}", self.word)]
    }
    fn changed(&self, _: &Path) -> Option<String> {
        Some(format!("{} warmed", self.word))
    }
}

impl DocCheck for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        if docs.root().join(self.word).exists() {
            vec![Problem::new(self.word, 0, "present".into())]
        } else {
            Vec::new()
        }
    }
}

impl ProseRule for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn check(&self, _: &str, text: &str) -> Vec<(usize, String)> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| line.contains(self.word))
            .map(|(number, _)| (number + 1, self.word.to_string()))
            .collect()
    }
}

impl Generator for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn outputs(&self) -> &str {
        "out/"
    }
    fn command(&self) -> Vec<String> {
        vec!["make".into(), self.word.into()]
    }
}

fn both() -> Plugins {
    Plugins::new(vec![&FIRST, &SECOND])
}

fn text(blocks: &[Block]) -> String {
    format!("{blocks:?}")
}

#[test]
fn a_plugin_that_names_no_seam_adds_nothing() {
    let plugins = Plugins::new(vec![&Bare]);
    assert_eq!(plugins.component_names(), ["task"]);
    assert!(plugins.component("word").is_none());
    assert!(plugins.card_kinds().is_empty());
    assert!(plugins.route("one").is_none());
    assert!(plugins.nav().is_empty());
    assert!(plugins.home_regions().is_empty());
    assert!(plugins.board_actions().is_empty());
    assert!(plugins.board_field("bare", "effort").is_none());
    assert!(plugins.watches().is_empty());
    assert!(plugins.doc_checks().is_empty());
    assert!(plugins.prose_rules().is_empty());
    assert!(plugins.generators().is_empty());
    assert_eq!(Bare.theme(), None);
    assert!(Bare.transports().is_empty());
    assert_eq!(
        plugins
            .cache_version(&Page::new(Path::new("/r"), "docs/A.md"))
            .as_deref(),
        Some("")
    );
}

#[test]
fn a_component_comes_from_the_first_plugin_naming_it_after_the_built_in_task() {
    let plugins = both();
    assert_eq!(plugins.component_names(), ["task", "word", "word"]);
    let page = Page::new(Path::new("/r"), "docs/A.md");
    assert_eq!(
        plugins.render(&page, "word", "x", None),
        [Block::Paragraph {
            spans: vec![Span::Text {
                text: "one x on docs/A.md".into()
            }]
        }]
    );
    let reversed = Plugins::new(vec![&SECOND, &FIRST]);
    assert!(text(&reversed.render(&page, "word", "x", None)).contains("two x"));
    // The built-in task link is the Symphony's own, whatever the plugins name.
    assert!(text(&plugins.render(&page, "task", "sim/jump", None)).contains("/task/sim/jump"));
}

#[test]
fn a_component_that_cannot_render_or_is_unknown_shows_its_problem_in_its_place() {
    let page = Page::new(Path::new("/r"), "docs/A.md");
    let error = |problem: &str| Block::Paragraph {
        spans: vec![Span::Styled {
            tag: InlineKind::Span,
            presentation: symphony_plugin::vocabulary::Presentation {
                class: "component-error".into(),
                title: "component error".into(),
                ..Default::default()
            },
            spans: vec![Span::Text {
                text: format!("⚠ {problem}"),
            }],
        }],
    };
    assert_eq!(
        both().render(&page, "word", "bad", None),
        [error("one refuses bad")]
    );
    assert_eq!(
        both().render(&page, "nope", "x", None),
        [error("unknown component nope")]
    );
    assert_eq!(
        Plugins::default().render(&page, "word", "x", None),
        [error("unknown component word")]
    );
}

#[test]
fn a_cache_version_covers_every_component_and_none_when_one_has_none() {
    let page = Page::new(Path::new("/r"), "docs/A.md");
    assert_eq!(
        both().cache_version(&page).as_deref(),
        Some("word=one;word=two;")
    );
    assert_eq!(
        Plugins::new(vec![&FIRST, &UNVERSIONED]).cache_version(&page),
        None
    );
}

#[test]
fn every_list_seam_gathers_each_plugin_in_the_projects_order() {
    let plugins = both();
    let root = Path::new("/r");
    assert_eq!(
        plugins
            .card_kinds()
            .iter()
            .map(|k| k.name())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(
        plugins
            .nav()
            .iter()
            .map(|n| n.href.as_str())
            .collect::<Vec<_>>(),
        ["/one", "/two"]
    );
    assert_eq!(
        plugins
            .home_regions()
            .iter()
            .map(|r| r.region(root).unwrap().id)
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    let task = TaskRef {
        slug: "sim/jump",
        status: "review",
        branch: "sim/jump",
    };
    assert_eq!(
        plugins.board_actions()[1].offer(root, &task).unwrap(),
        TaskOffer {
            action: "two".into(),
            label: "two it".into(),
            branch: "sim/jump".into(),
            payload: Some("two:sim/jump".into()),
        }
    );
    let submission = |payload: &str| Submission {
        payload: payload.into(),
        note: "send it".into(),
    };
    assert_eq!(
        plugins.board_actions()[1].submit(root, &task, &submission("two:sim/jump")),
        Ok(Submitted {
            said: "two sim/jump: send it".into(),
            released: vec!["sim/jump".into()],
        })
    );
    assert_eq!(
        plugins.board_actions()[0].submit(root, &task, &submission("two:sim/jump")),
        Err("one refuses a stale offer".into())
    );
    assert_eq!(
        plugins
            .watches()
            .iter()
            .flat_map(|w| w.paths())
            .collect::<Vec<_>>(),
        ["data/one", "data/two"]
    );
    assert_eq!(
        plugins.watches()[0].changed(root).as_deref(),
        Some("one warmed")
    );
    // The default forget keeps nothing to drop.
    plugins.watches()[0].forget(root);
    assert_eq!(
        plugins
            .generators()
            .iter()
            .map(|g| (g.outputs().to_string(), g.command(), g.when_changed()))
            .collect::<Vec<_>>(),
        [
            (
                "out/".to_string(),
                vec!["make".to_string(), "one".to_string()],
                false
            ),
            (
                "out/".to_string(),
                vec!["make".to_string(), "two".to_string()],
                false
            )
        ]
    );
    let rules = plugins.prose_rules();
    assert_eq!(
        rules[1].check("docs/A.md", "a\ntwo\nb two\n"),
        [(2, "two".into()), (3, "two".into())]
    );
}

#[test]
fn a_plugins_generators_run_after_the_projects_and_need_its_land_command() {
    let landed = Project::parse(
        Path::new("/r"),
        "[verdicts]\nland = [\"l\"]\ngenerators = [{ name = \"g\", outputs = \"o\", command = [\"x\"] }]\n",
    )
    .unwrap();
    let generator = |name: &str, outputs: &str, command: &[&str]| Generated {
        name: name.into(),
        outputs: outputs.into(),
        command: command.iter().map(|word| word.to_string()).collect(),
        when_changed: false,
    };
    assert_eq!(
        symphony::landing::with_plugins(&landed, &both()),
        [
            generator("g", "o", &["x"]),
            generator("one", "out/", &["make", "one"]),
            generator("two", "out/", &["make", "two"]),
        ]
    );
    assert!(symphony::plugins::unlanded(&landed, &both()).is_empty());
    let unlanded = Project::parse(Path::new("/r"), "").unwrap();
    assert_eq!(
        symphony::plugins::unlanded(&unlanded, &both()),
        [
            "symphony.toml: the plugin generator one needs a [verdicts] land command",
            "symphony.toml: the plugin generator two needs a [verdicts] land command",
        ]
    );
    assert!(symphony::plugins::unlanded(&unlanded, &Plugins::new(vec![&Bare])).is_empty());
    assert!(symphony::landing::of_project(&unlanded).is_empty());
}

#[test]
fn a_route_is_found_by_its_path_and_answers_or_finds_nothing() {
    let plugins = both();
    let root = Path::new("/r");
    let route = plugins.route("two").unwrap();
    assert_eq!(
        route.serve(root, "data"),
        Some(Reply::Json("{\"two\":1}".into()))
    );
    assert_eq!(
        route.serve(root, "a.bin"),
        Some(Reply::File(root.join("a.bin")))
    );
    assert_eq!(route.serve(root, ""), None);
    assert!(plugins.route("three").is_none());
}

#[test]
fn a_board_field_is_found_only_under_its_own_plugin_and_checks_its_kind() {
    let plugins = both();
    let field = plugins.board_field("second", "effort").unwrap();
    assert!(!field.required());
    assert_eq!(
        field.validate(&FieldValue::Text(" 3 ".into())),
        Ok(FieldValue::Number(3.0))
    );
    assert!(plugins.board_field("third", "effort").is_none());
    assert!(plugins.board_field("second", "risk").is_none());
}

#[test]
fn a_value_is_stored_as_its_fields_kind_reads_it() {
    let choice = FieldKind::Choice(vec!["low".into(), "high".into()]);
    for (kind, value, stored) in [
        (
            FieldKind::Text,
            FieldValue::Text("  why  ".into()),
            Ok(FieldValue::Text("why".into())),
        ),
        (
            FieldKind::Text,
            FieldValue::Text("  ".into()),
            Err("the value is empty".to_string()),
        ),
        (
            FieldKind::Text,
            FieldValue::Number(2.0),
            Err("2 is a number, not text".into()),
        ),
        (
            FieldKind::Number,
            FieldValue::Number(1.5),
            Ok(FieldValue::Number(1.5)),
        ),
        (
            FieldKind::Number,
            FieldValue::Text("-4".into()),
            Ok(FieldValue::Number(-4.0)),
        ),
        (
            FieldKind::Number,
            FieldValue::Text("many".into()),
            Err("\"many\" is not a number".into()),
        ),
        (
            FieldKind::Number,
            FieldValue::Number(f64::NAN),
            Err("NaN is not a finite number".into()),
        ),
        (
            FieldKind::Number,
            FieldValue::Text("inf".into()),
            Err("inf is not a finite number".into()),
        ),
        (
            choice.clone(),
            FieldValue::Text("high".into()),
            Ok(FieldValue::Text("high".into())),
        ),
        (
            choice,
            FieldValue::Text("mid".into()),
            Err("\"mid\" is not one of low, high".into()),
        ),
        (
            FieldKind::Link,
            FieldValue::Text("docs/A.md".into()),
            Ok(FieldValue::Text("docs/A.md".into())),
        ),
        (
            FieldKind::Link,
            FieldValue::Text("a b".into()),
            Err("\"a b\" is not a link: it holds a space".into()),
        ),
    ] {
        assert_eq!(check_kind(&kind, &value), stored, "{kind:?} {value:?}");
    }
}
