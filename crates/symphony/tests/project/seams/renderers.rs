//! A plugin's renderers: an entrypoint's `types` may name one of the project's own plugins, whose
//! files it then shows, lists and serves with the type the renderer names; a type no renderer of the
//! project handles is refused at load, naming the renderers it has. The fixtures draw files named by
//! their word (`a.one`, `a.two`).

use symphony::docview;
use symphony::plugins::Plugins;
use symphony::project::Project;
use symphony::render::Render;
use symphony::renderers;
use symphony_plugin::vocabulary::Viewer;
use symphony_plugin::{Drawing, File, Page, Renderer};

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
        renderers::names(&["first".into(), "second".into(), "first".into()]),
        ["markdown", "media", "one", "two"]
    );
}

#[test]
fn a_page_draws_only_its_projects_renderers_and_the_symphonys_own_types_stay_its_own() {
    let tree = naming(&["second"]);
    let plugins = Plugins::of(&tree.root);
    let drawer = |src: &str| renderers::drawing(&plugins, src).map(|d| d.name().to_string());
    assert_eq!(drawer("/files/a.two?v=1#top").as_deref(), Some("two"));
    assert_eq!(drawer("/files/a.one"), None, "first is not the project's");
    assert_eq!(drawer("/files/b.png"), None);
    assert_eq!(
        renderers::drawing(&Plugins::default(), "/a.two").map(|d| d.name().to_string()),
        None
    );
}

#[test]
fn a_plugin_files_type_is_the_renderers_and_names_resolve_the_symphonys_first() {
    naming(&[]);
    assert_eq!(
        renderers::content_type("/x/a.TWO"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(renderers::content_type("/x/a.three"), None);
    assert_eq!(renderers::named("one").map(|r| r.name), Some("one"));
    assert_eq!(
        renderers::named("media").map(|r| r.extensions.contains(&"png")),
        Some(true)
    );
    assert_eq!(renderers::named("three"), None);
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
