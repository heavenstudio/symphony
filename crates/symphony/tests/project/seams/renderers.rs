//! A plugin's renderers: an entrypoint's `types` may name one of the project's own plugins, whose
//! files it then shows, lists and serves with the type the renderer names, never another plugin's
//! of the same name; a type no renderer of the project handles, or two of them sharing a name, is
//! refused at load. The fixtures draw files named by their word (`a.one`, `a.two`); [`TWIN`] gives a
//! renderer `two` of its own.

use symphony::docview;
use symphony::plugins::Plugins;
use symphony::project::Project;
use symphony::render::Render;
use symphony::renderers;
use symphony_plugin::vocabulary::Viewer;
use symphony_plugin::{Drawing, File, Page, Plugin, Renderer};

use super::compiled::naming;
use super::{Fixture, Tree};

impl Renderer for Fixture {
    fn name(&self) -> &str {
        self.word
    }
    fn extensions(&self) -> &[&str] {
        std::slice::from_ref(&self.word)
    }
    fn content_type(&self) -> &str {
        "text/plain; charset=utf-8"
    }
    fn draw(&self, _: &Page<'_>, _: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Viewer(Viewer::default()))
    }
}

/// A project naming `plugins` with one entrypoint, `/` over `files/`, of `types`.
fn serving(plugins: &[&str], types: &str) -> Tree {
    let tree = naming(plugins);
    let names: Vec<String> = plugins.iter().map(|name| format!("\"{name}\"")).collect();
    tree.write(
        symphony::project::FILE,
        &format!(
            "[plugins]\ncrates = [{}]\n\n[[entrypoints]]\nurl = \"/\"\nfolder = \"files\"\n\
             types = [{types}]\n",
            names.join(", ")
        ),
    );
    tree.write("files/index.md", "# Files\n");
    for name in ["a.one", "a.two", "b.png"] {
        tree.write(&format!("files/{name}"), name);
    }
    tree
}

#[test]
fn an_entrypoint_naming_its_plugins_renderer_shows_and_lists_that_plugins_files() {
    let tree = serving(&["first"], "\"markdown\", \"one\"");
    let project = Project::load(&tree.root).unwrap();
    let entry = &project.entrypoints[0];
    let shown = |inside: &str| entry.renderer(inside).map(|r| r.name);
    assert_eq!(
        (
            shown("a.one"),
            shown("a.ONE"),
            shown("a.two"),
            shown("b.png")
        ),
        (Some("one"), Some("one"), None, None)
    );
    assert!(entry.shows("a.one") && !entry.page("a.one"), "{entry:?}");
    let render = Render::new(&tree.root);
    let entries: Vec<String> = docview::folder(&render, "", "")
        .unwrap()
        .entries
        .iter()
        .map(|e| e.path.clone())
        .collect();
    assert_eq!(entries, ["index.md", "a.one"], "{entries:?}");
}

/// A plugin whose renderer has [`super::SECOND`]'s name, `two`, but draws `.twin` files, served as
/// `text/twin`.
pub(super) struct Twin;

pub(super) static TWIN: Twin = Twin;

impl Plugin for Twin {
    fn name(&self) -> &str {
        "twin"
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![self]
    }
}

impl Renderer for Twin {
    fn name(&self) -> &str {
        "two"
    }
    fn extensions(&self) -> &[&str] {
        &["twin"]
    }
    fn content_type(&self) -> &str {
        "text/twin"
    }
    fn draw(&self, _: &Page<'_>, _: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Blocks(Vec::new()))
    }
}

/// The names of the renderers the project using `plugins` has.
fn names(plugins: &[&str]) -> Result<Vec<&'static str>, String> {
    let plugins: Vec<String> = plugins.iter().map(|name| (*name).to_string()).collect();
    renderers::of_project(&plugins).map(|has| has.iter().map(|r| r.name).collect())
}

#[test]
fn a_type_no_renderer_of_the_project_handles_is_refused_naming_the_ones_it_has() {
    let unnamed = serving(&[], "\"markdown\", \"one\"");
    let refused = Project::load(&unnamed.root).unwrap_err();
    assert_eq!(
        refused,
        "symphony.toml: [[entrypoints]] / type \"one\": no renderer handles it (the renderers are \
         markdown, media)"
    );
    let other = serving(&["second", "bare"], "\"one\"");
    let refused = Project::load(&other.root).unwrap_err();
    assert_eq!(
        refused,
        "symphony.toml: [[entrypoints]] / type \"one\": no renderer handles it (the renderers are \
         markdown, media, two)"
    );
    assert_eq!(
        names(&["first", "second", "missing"]),
        Ok(vec!["markdown", "media", "one", "two"])
    );
}

#[test]
fn a_project_takes_the_renderer_of_its_own_plugins_whatever_another_plugin_calls_its_own() {
    for (plugin, shown, passed, served) in [
        ("second", "a.two", "a.twin", "text/plain; charset=utf-8"),
        ("twin", "a.twin", "a.two", "text/twin"),
    ] {
        let tree = serving(&[plugin], "\"two\"");
        tree.write(&format!("files/{shown}"), shown);
        tree.write(&format!("files/{passed}"), passed);
        let project = Project::load(&tree.root).unwrap();
        let entry = &project.entrypoints[0];
        assert!(
            entry.shows(shown) && !entry.shows(passed),
            "{plugin}: {entry:?}"
        );
        let plugins = Plugins::of(&tree.root);
        let types = (
            renderers::content_type(&plugins, &format!("/files/{shown}")),
            renderers::content_type(&plugins, &format!("/files/{passed}")),
        );
        assert_eq!(types, (Some(served), None), "{plugin}");
    }
}

#[test]
fn two_renderers_of_one_project_sharing_a_name_are_refused_naming_both_plugins() {
    let shared = serving(&["second", "twin"], "\"two\"");
    let refused = Project::load(&shared.root).unwrap_err();
    assert_eq!(
        refused,
        "symphony.toml: [plugins] plugin \"second\" and plugin \"twin\" both give a renderer named \
         \"two\""
    );
    let convention = naming(&["twin", "second"]);
    let refused = Project::load(&convention.root).unwrap_err();
    assert!(
        refused.contains("plugin \"twin\" and plugin \"second\""),
        "{refused}"
    );
}

#[test]
fn a_page_draws_only_its_projects_renderers_and_the_symphonys_own_types_stay_its_own() {
    let tree = naming(&["second"]);
    let plugins = Plugins::of(&tree.root);
    let drawer = |src: &str| renderers::renderer_of(&plugins, src).map(|r| r.name().to_string());
    assert_eq!(drawer("/files/a.two?v=1#top").as_deref(), Some("two"));
    assert_eq!(drawer("/files/a.one"), None, "first is not the project's");
    assert_eq!(drawer("/files/b.png"), None);
    assert_eq!(
        renderers::renderer_of(&Plugins::default(), "/a.two").map(|r| r.name().to_string()),
        None
    );
    assert_eq!(renderers::content_type(&plugins, "/x/a.three"), None);
}

#[test]
fn an_extension_is_the_last_names_lowered_without_query_or_fragment() {
    for (src, extension) in [
        ("/a/b.ONE?x=1#y", Some("one")),
        ("b.tar.gz", Some("gz")),
        ("/a.d/b", None),
        ("/a/.hidden", None),
        ("/a/b.", None),
        ("", None),
    ] {
        assert_eq!(renderers::extension(src).as_deref(), extension, "{src:?}");
    }
}
