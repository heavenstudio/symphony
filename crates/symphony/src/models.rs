//! Hull gallery content, cached per served root and invalidated by the document watcher. No
//! cross-worktree scan, model hashing or history walk on a request: a branch shows its own hulls.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use symphony_api::{GalleryHull, Media};

const MARKER: &str = "<div data-hull-gallery></div>";
const HULLS: &str = "view/models/hulls";
const OUTLINES: &str = "docs/data/hull-outlines";

static GALLERIES: LazyLock<Mutex<HashMap<PathBuf, Vec<GalleryHull>>>> =
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

fn build(root: &Path) -> Vec<GalleryHull> {
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
        hulls.push(GalleryHull {
            id: id.into(),
            model: url(&format!("{HULLS}/{id}.glb")),
            picture,
        });
    }
    hulls
}

/// Expand before the normal document URL, thumbnail and version preparation.
pub fn expand(root: &Path, html: &mut String, media: &mut Vec<Media>) {
    if !html.contains(MARKER) {
        return;
    }
    let mut cache = GALLERIES.lock().unwrap_or_else(|e| e.into_inner());
    let gallery = cache
        .entry(root.to_path_buf())
        .or_insert_with(|| build(root));
    let index = media.len();
    *html = html.replace(
        MARKER,
        &format!("<span class=\"media\" data-media=\"{index}\" data-kind=\"gallery\"></span>"),
    );
    media.push(Media::Gallery {
        hulls: gallery.clone(),
    });
}
