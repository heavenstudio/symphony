//! A fixture project's plugin, which the server's tests and the client's browser cases serve
//! (`symphony-shelf`, the server holding it): `[[shelf:all]]` draws every item on the checkout's
//! shelf, each with the poster its label names, else its label's own picture. The gallery is cached
//! per root and its watch drops and warms it, as a project's gallery would. Its page is `/shelf`, and
//! `/x/shelf/NAME.item` serves an item's file. Its renderer, `shelf-item`, draws an `.item` file in
//! the fixture viewer [`viewer`], in the gallery and wherever a page links or pictures one; its
//! `shelf-ledger` draws a `.csv` file as a line of text. Its gates answerer, [`GATES_ANSWERER`],
//! answers a task's gates as the checkout's [`GATES`] file says.

use crate::lock::Held as _;
use std::collections::HashMap;
use std::path::{Component as Part, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use sha2::{Digest, Sha256};
use symphony_plugin::vocabulary::{Block, GalleryItem, Gate, Media, NavItem, Span, View, Viewer};
use symphony_plugin::{
    Component, Drawing, File, Gates, Page, Plugin, Renderer, Reply, Route, TaskRef, Watch,
};

/// The plugin's name, as a project's `[plugins] crates` lists it.
pub const NAME: &str = "fixture-shelf";
/// The document that draws the shelf.
pub const DOC: &str = "docs/SHELF.md";
/// The shelf, and the folder of its items (`NAME.item`).
pub const SHELF: &str = "room/shelf";
pub const ITEMS: &str = "room/shelf/items";
/// Where the posters a label names are.
pub const POSTERS: &str = "posters";
/// Each item's label (`NAME.notes.json`, naming its poster) and own picture (`NAME.svg`).
pub const LABELS: &str = "docs/labels";

/// The plugins a process serving the fixture holds: the shelf alone.
pub static PLUGINS: &[&dyn Plugin] = &[&Shelf];

/// A project file naming the plugin.
pub const MANIFEST: &str = "[plugins]\ncrates = [\"fixture-shelf\"]\n";

/// Makes [`PLUGINS`] the process's plugins; again is no change.
///
/// # Errors
///
/// When the process already holds another list.
pub fn install() -> Result<(), String> {
    symphony::plugins::install(PLUGINS)
}

struct Shelf;

impl Plugin for Shelf {
    fn name(&self) -> &str {
        NAME
    }

    fn components(&self) -> Vec<&dyn Component> {
        vec![self]
    }

    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![self, &Ledger]
    }

    fn watches(&self) -> Vec<&dyn Watch> {
        vec![self]
    }

    fn routes(&self) -> Vec<&dyn Route> {
        vec![self]
    }

    fn gates(&self) -> Vec<&dyn Gates> {
        vec![&ShelfGates]
    }

    fn nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            label: "▤ shelf".into(),
            title: "Shelf items".into(),
            href: "/shelf".into(),
            document: DOC.into(),
        }]
    }

    /// Its theme package, [`THEME`]: a name, one token and the compact header.
    fn theme(&self) -> Option<symphony_plugin::Theme> {
        Some(symphony_plugin::Theme {
            package: THEME.into(),
            name: "The Shelf".into(),
            tokens: [("primary".to_string(), "#2a6f4e".to_string())].into(),
            layouts: [("header".to_string(), "compact".to_string())].into(),
            ..symphony_plugin::Theme::default()
        })
    }
}

/// The theme package the plugin provides, as a fixture project's `[theme] package` names it.
pub const THEME: &str = "fixture-shelf";

/// The renderer's name, as an entrypoint's `types` gives it, and its files' type.
pub const ITEM: &str = "shelf-item";
pub const ITEM_TYPE: &str = "text/plain; charset=utf-8";

/// The viewer an item opens in: the element `shelf-viewer` its script defines, a `Turn` view and a
/// `Back` one that set its `angle` and call its `settle`, and a download link.
pub fn viewer() -> Viewer {
    let view = |label: &str, angle: &str| View {
        label: label.into(),
        title: Some(format!("{label} the item")),
        set: vec![("angle".into(), angle.into())],
        call: Some("settle".into()),
    };
    Viewer {
        element: "shelf-viewer".into(),
        script: "/static/shelf-viewer.js".into(),
        attributes: vec![
            ("loading".into(), "lazy".into()),
            ("angle".into(), "0".into()),
        ],
        views: vec![view("Turn", "90"), view("Back", "0")],
        download: Some(".item".into()),
        open_label: Some("Take it down".into()),
        close_label: Some("Put it back".into()),
    }
}

impl Renderer for Shelf {
    fn name(&self) -> &str {
        ITEM
    }

    fn extensions(&self) -> &[&str] {
        &["item"]
    }

    fn content_type(&self) -> &str {
        ITEM_TYPE
    }

    fn draw(&self, _page: &Page<'_>, _file: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Viewer(viewer()))
    }
}

/// The shelf's ledger, a `.csv` file drawn as a line of text and served as [`LEDGER_TYPE`]: a
/// renderer of an extension the server knows a kind of its own for.
struct Ledger;

pub const LEDGER_TYPE: &str = "text/vnd.shelf-ledger";

impl Renderer for Ledger {
    fn name(&self) -> &str {
        "shelf-ledger"
    }

    fn extensions(&self) -> &[&str] {
        &["csv"]
    }

    fn content_type(&self) -> &str {
        LEDGER_TYPE
    }

    fn draw(&self, _page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        Ok(Drawing::Blocks(vec![Block::Paragraph {
            spans: vec![Span::Text {
                text: format!("ledger {}", file.label),
            }],
        }]))
    }
}

impl Route for Shelf {
    fn path(&self) -> &str {
        "shelf"
    }

    /// An item's file by its name alone; the server refuses what is missing, a folder or a link
    /// out of the checkout.
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        let plain = !rest.is_empty()
            && Path::new(rest)
                .components()
                .all(|part| matches!(part, Part::Normal(_)));
        (plain && rest.ends_with(".item") && !rest.contains('/'))
            .then(|| Reply::File(root.join(ITEMS).join(rest)))
    }
}

impl Component for Shelf {
    fn name(&self) -> &str {
        "shelf"
    }

    fn render(&self, page: &Page<'_>, _: &str, _: Option<&str>) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Media {
            media: Media::Gallery {
                items: gallery(page.root),
                viewer: viewer(),
            },
        }])
    }

    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        let bytes = serde_json::to_vec(&gallery(page.root)).unwrap_or_default();
        Some(
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
}

/// The gates answerer's name: the gate shown in its place when it fails.
pub const GATES_ANSWERER: &str = "shelf-gates";
/// The file a fixture checkout's gates come from: a JSON object of task slugs, each
/// `{"gates": [Gate, …]}` answered as given, `{"error": "why"}` refused, `"sleep_ms"` waited
/// first, `"held_until": "FILE"` held first until the checkout holds FILE, `"panic": true`
/// panicking. A task it does not name has no gates of the shelf's.
pub const GATES: &str = "gates.json";
/// Where each call is written, a line `SLUG BRANCH TIP` (`-` for no tip).
pub const GATES_CALLS: &str = "gates.calls";

struct ShelfGates;

impl Gates for ShelfGates {
    fn name(&self) -> &str {
        GATES_ANSWERER
    }

    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String> {
        let call = format!("{} {} {}\n", task.slug, task.branch, tip.unwrap_or("-"));
        let mut calls = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(GATES_CALLS))
            .map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut calls, call.as_bytes()).map_err(|e| e.to_string())?;
        let answers: serde_json::Value = match std::fs::read(root.join(GATES)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{GATES}: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("{GATES}: {e}")),
        };
        let answer = &answers[task.slug];
        if let Some(ms) = answer["sleep_ms"].as_u64() {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: a test plugin's gate made slow by a length of time, not held until its test releases it"
            )]
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        if let Some(file) = answer["held_until"].as_str() {
            while !root.join(file).exists() {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: a test plugin looks again for the file its test makes to release it"
                )]
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        assert!(
            answer["panic"] != true,
            "{GATES} says to panic for {}",
            task.slug
        );
        if let Some(error) = answer["error"].as_str() {
            return Err(error.into());
        }
        match answer.get("gates") {
            Some(gates) => {
                serde_json::from_value(gates.clone()).map_err(|e| format!("{GATES}: {e}"))
            }
            None => Ok(Vec::new()),
        }
    }
}

impl Watch for Shelf {
    fn paths(&self) -> Vec<String> {
        vec![SHELF.into(), POSTERS.into(), LABELS.into()]
    }

    fn changed(&self, root: &Path) -> Option<String> {
        invalidate(root);
        let items = warm(root);
        Some(format!("shelf warmed: {}: {items} items", root.display()))
    }

    fn forget(&self, root: &Path) {
        invalidate(root);
    }
}

static GALLERIES: LazyLock<Mutex<HashMap<PathBuf, Vec<GalleryItem>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn invalidate(root: &Path) {
    GALLERIES.lock().held().remove(root);
}

/// The shelf of `root` built into the cache when it is not there: how many items it holds.
pub fn warm(root: &Path) -> usize {
    gallery(root).len()
}

fn gallery(root: &Path) -> Vec<GalleryItem> {
    GALLERIES
        .lock()
        .held()
        .entry(root.to_path_buf())
        .or_insert_with(|| build(root))
        .clone()
}

/// Whether `relative` names a file inside `root`, reached through no link out of it.
fn project_file(root: &Path, relative: &str) -> bool {
    !relative.is_empty()
        && Path::new(relative)
            .components()
            .all(|part| matches!(part, Part::Normal(_)))
        && root
            .join(relative)
            .canonicalize()
            .is_ok_and(|path| path.starts_with(root) && path.is_file())
}

fn build(root: &Path) -> Vec<GalleryItem> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let folder = root
        .join(ITEMS)
        .canonicalize()
        .ok()
        .filter(|folder| folder.starts_with(&root));
    let mut paths: Vec<_> = folder
        .and_then(|folder| std::fs::read_dir(folder).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "item")
                && path.is_file()
        })
        .collect();
    paths.sort();
    let mut items = Vec::new();
    for path in paths {
        let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let poster = std::fs::read(root.join(format!("{LABELS}/{name}.notes.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|notes| notes["source"]["picture"].as_str().map(str::to_string))
            .filter(|picture| project_file(&root, picture));
        let own = format!("{LABELS}/{name}.svg");
        let poster = poster.or_else(|| project_file(&root, &own).then_some(own));
        let poster = poster.map(|picture| {
            let image = symphony::images::reserve(
                &format!("<img src=\"/{picture}\" width=\"80\" height=\"80\">"),
                &root,
                false,
                symphony::images::sips_available(),
            );
            symphony::markup::unescape(
                image
                    .split("src=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or_default(),
            )
        });
        items.push(GalleryItem {
            name: name.into(),
            src: format!("/{ITEMS}/{name}.item"),
            poster,
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    #[test]
    fn a_page_linking_an_item_draws_it_in_the_viewer_its_gallery_opens() {
        install().unwrap();
        let tree = Tree::new();
        tree.write(symphony::project::FILE, MANIFEST);
        let render = symphony::render::Render::new(&tree.root);
        let media: Vec<Media> = render
            .sections("[First](/room/shelf/items/FIRST.item)\n", DOC)
            .into_iter()
            .flat_map(|section| section.media)
            .collect();
        assert_eq!(
            media,
            [Media::Viewer {
                src: "/room/shelf/items/FIRST.item".into(),
                label: "First".into(),
                poster: None,
                viewer: viewer(),
            }],
            "{media:?}"
        );
        let page = Page::new(&tree.root, DOC);
        let gallery = Shelf.render(&page, "all", None).unwrap();
        assert!(
            matches!(&gallery[..], [Block::Media { media: Media::Gallery { viewer: shown, .. } }] if *shown == viewer()),
            "{gallery:?}"
        );
    }
}
