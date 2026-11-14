//! The plugins a process holds, as a host compiles them in: the ones a project's file names, in its
//! order, a name the build lacks reported, another list refused, and the lint asking them.

use symphony::plugins::Plugins;
use symphony::project::Project;
use symphony_plugin::Plugin;

use super::every::EVERY;
use super::renderers::TWIN;
use super::{Bare, FIRST, SECOND, Tree};

/// The plugins the project tests' process holds, as a host would compile them in.
static COMPILED: &[&dyn Plugin] = &[&FIRST, &SECOND, &Bare, &TWIN, &EVERY];

/// A tree whose `symphony.toml` names `plugins`, in that order; the process holds [`COMPILED`].
pub(super) fn naming(plugins: &[&str]) -> Tree {
    symphony::plugins::install(COMPILED).unwrap();
    let tree = Tree::new();
    let names: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
    tree.write(
        symphony::project::FILE,
        &format!("[plugins]\ncrates = [{}]\n", names.join(", ")),
    );
    tree
}

#[test]
fn a_projects_plugins_are_the_compiled_ones_its_file_names() {
    let plain = Tree::new();
    assert!(Plugins::of(&plain.root).list().is_empty());
    let named = naming(&["second", "bare"]);
    let plugins = Plugins::of(&named.root);
    assert_eq!(
        plugins.list().iter().map(|p| p.name()).collect::<Vec<_>>(),
        ["second", "bare"]
    );
    assert_eq!(
        plugins
            .nav()
            .iter()
            .map(|n| n.href.as_str())
            .collect::<Vec<_>>(),
        ["/two"]
    );
    assert!(plugins.route("two").is_some());
    assert!(plugins.route("one").is_none());
    assert!(plugins.component("word").is_some());
    assert!(Plugins::of(&plain.root).component("word").is_none());
}

#[test]
fn a_name_the_build_does_not_hold_is_left_out_and_reported() {
    let tree = naming(&["missing", "bare"]);
    let project = Project::of(&tree.root);
    assert_eq!(
        Plugins::for_project(&project)
            .list()
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>(),
        ["bare"]
    );
    assert_eq!(
        symphony::plugins::problems(&project),
        ["symphony.toml: [plugins] crates names missing, which this build does not hold"]
    );
}

#[test]
fn a_process_keeps_the_plugins_it_was_given_and_refuses_another_list() {
    symphony::plugins::install(COMPILED).unwrap();
    assert!(symphony::plugins::install(COMPILED).is_ok());
    let names: Vec<&str> = symphony::plugins::compiled()
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(names, ["first", "second", "bare", "twin", "every"]);
    static OTHER: &[&dyn Plugin] = &[&Bare];
    assert_eq!(
        symphony::plugins::install(OTHER),
        Err(
            "this process already holds the plugins [first, second, bare, twin, every]; it cannot hold [bare] instead"
                .into()
        )
    );
    // The same names, but a first that answers otherwise: another value, refused.
    static IMPOSTOR: super::Fixture = super::Fixture {
        plugin: "first",
        word: "else",
        versioned: false,
        markers: &[],
    };
    static SAME_NAMES: &[&dyn Plugin] = &[&IMPOSTOR, &SECOND, &Bare, &TWIN, &EVERY];
    assert_eq!(
        symphony::plugins::install(SAME_NAMES),
        Err(
            "this process already holds the plugins [first, second, bare, twin, every]; another value of the same names cannot replace them"
                .into()
        )
    );
    // Another list of the very same values is the same plugins.
    static AGAIN: &[&dyn Plugin] = &[&FIRST, &SECOND, &Bare, &TWIN, &EVERY];
    assert!(symphony::plugins::install(AGAIN).is_ok());
    assert!(std::ptr::eq(
        symphony::plugins::compiled()[0],
        &FIRST as &dyn Plugin
    ));
}

#[test]
fn the_lint_runs_the_projects_document_checks_and_prose_rules() {
    let tree = naming(&["first"]);
    // The fixture's generator needs a land command, or the project file is refused first.
    tree.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"first\"]\n\n[verdicts]\nland = [\"l\"]\n",
    );
    tree.write("docs/x.md", "# X\n\nThe one rule.\n");
    tree.write("one", "");
    let (prose, rules) = symphony::lint::rule_findings(&tree.root);
    assert_eq!(rules, ["one"], "{prose:?}");
    assert_eq!(prose, [("docs/x.md".to_string(), 3, "one".to_string())]);
    let (problems, checks) = symphony::lint::check_findings(&tree.root);
    assert_eq!(checks, ["one"]);
    assert_eq!(problems, ["one: present"]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(symphony::lint::main(&tree.root, &mut out, &mut err), 1);
    let said = String::from_utf8(err).unwrap();
    assert!(
        said.contains("✖ 1 document check problem(s): one."),
        "{said}"
    );
    assert!(said.contains("docs/x.md:3 one"), "{said}");
    // A plain project has no plugin, so neither runs.
    let plain = Tree::new();
    plain.write("docs/x.md", "# X\n\nThe one rule.\n");
    plain.write("one", "");
    assert!(symphony::lint::rule_findings(&plain.root).0.is_empty());
    assert!(symphony::lint::check_findings(&plain.root).0.is_empty());
}

#[test]
fn a_cards_title_and_summary_leave_out_the_registers_markers_and_its_plugins_kinds_ones() {
    let card = &symphony::register::parse_decisions(&[
        "- [ ] **pick** — Pick one. *One:* a b *Two:* c d *Replaces:* old *Else:* kept *One:*",
    ])[0];
    let summary = |tree: &Tree| symphony::dashboard::card_summary(&Plugins::of(&tree.root), card);
    assert_eq!(
        summary(&naming(&["first"])),
        "Pick one. Two: c d Else: kept"
    );
    assert_eq!(
        summary(&naming(&["first", "second"])),
        "Pick one. Else: kept"
    );
    assert_eq!(
        summary(&naming(&[])),
        "Pick one. One: a b Two: c d Else: kept One:"
    );
    assert_eq!(
        symphony::dashboard::card_title(&Plugins::of(&naming(&["second"]).root), card),
        "Pick one."
    );
}

#[test]
fn a_page_hands_its_components_the_projects_artifacts_folder() {
    let doc = "# A\n\n[[every:lantern]]\n";
    let plain = naming(&["every"]);
    let html = symphony::render::Render::new(&plain.root).html(doc, "docs/A.md");
    assert!(html.contains("lantern None in artifacts/every"), "{html}");
    // A project that names a folder of its own hands that one.
    let named = naming(&["every"]);
    named.write(
        symphony::project::FILE,
        "[project]\nartifacts = \"out/kept\"\n\n[plugins]\ncrates = [\"every\"]\n",
    );
    let html = symphony::render::Render::new(&named.root).html(doc, "docs/A.md");
    assert!(html.contains("lantern None in out/kept/every"), "{html}");
}
