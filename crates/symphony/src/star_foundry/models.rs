//! The hull gallery's models, cached per served root and invalidated by the plugin's watch. No
//! cross-worktree scan, model hashing or history walk on a request: a branch shows its own hulls.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use symphony_plugin::vocabulary::GalleryItem;

/// The models folder, and the hull models in it.
pub(super) const MODELS: &str = "view/models";
pub(super) const HULLS: &str = "view/models/hulls";
/// Where a hull's concept pictures and its outline are.
pub(super) const CONCEPTS: &str = "artifacts/concepts";
pub(super) const OUTLINES: &str = "docs/data/hull-outlines";

static GALLERIES: LazyLock<Mutex<HashMap<PathBuf, Vec<GalleryItem>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn invalidate(root: &Path) {
    GALLERIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(root);
}

/// Populate document content before a visitor asks for it, at startup and watcher refresh.
pub fn warm(root: &Path) -> usize {
    GALLERIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(root.to_path_buf())
        .or_insert_with(|| build(root))
        .len()
}

fn url(path: &str) -> String {
    format!("/{}", crate::markup::quote(path, "/"))
}

fn project_file(root: &Path, relative: &str) -> bool {
    !relative.is_empty()
        && Path::new(relative)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && root
            .join(relative)
            .canonicalize()
            .is_ok_and(|path| path.starts_with(root) && path.is_file())
}

fn build(root: &Path) -> Vec<GalleryItem> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let folder = root
        .join(HULLS)
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
            path.extension().is_some_and(|extension| extension == "glb") && path.is_file()
        })
        .collect();
    paths.sort();
    let mut hulls = Vec::new();
    for path in paths {
        let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let picture = std::fs::read(root.join(format!("{OUTLINES}/{id}.notes.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|notes| notes["source"]["picture"].as_str().map(str::to_string))
            .filter(|picture| project_file(&root, picture));
        let outline = format!("{OUTLINES}/{id}.svg");
        let picture = picture.or_else(|| project_file(&root, &outline).then_some(outline));
        let picture = picture.map(|picture| {
            // Reuse the document image policy for the 160-device-pixel copy, including its
            // fallback on machines without sips. This cache is invalidated when its source moves.
            let image = crate::images::reserve(
                &format!(
                    "<img src=\"{}\" width=\"80\" height=\"80\">",
                    crate::markup::escape(&url(&picture))
                ),
                &root,
                false,
                crate::images::sips_available(),
            );
            crate::markup::unescape(
                image
                    .split("src=\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or_default(),
            )
        });
        hulls.push(GalleryItem {
            name: id.into(),
            src: format!(
                "/x/{}/{}",
                super::HULLS_ROUTE,
                crate::markup::quote(&format!("{id}.glb"), "")
            ),
            poster: picture,
        });
    }
    hulls
}

/// The models of the checkout at `root`, from the cache when it holds them.
pub fn gallery(root: &Path) -> Vec<GalleryItem> {
    GALLERIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(root.to_path_buf())
        .or_insert_with(|| build(root))
        .clone()
}
