//! Native document read models. Threads and source positions are independent of section markup so
//! a reply or a moved block never retransmits the rest of a long register.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use symphony_api::{
    Document, DocumentChange, DocumentTask, Folder, Fragment, Media, SectionPosition,
};

use crate::render::Render;

mod comparison;

/// Resolve only document names (`docs/…`) whose canonical file remains under the project's
/// documents folder, whatever that folder is called ([`crate::project::Project::file`]).
pub fn target(root: &Path, rel: &str, document: bool) -> Result<PathBuf, String> {
    let route = crate::project::ROUTE;
    if !rel.starts_with(&format!("{route}/")) && rel != route
        || Path::new(rel)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(format!("not found: {rel}"));
    }
    let project = crate::project::Project::of(root);
    let docs = root
        .join(&project.docs)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !docs.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
        return Err(format!("not found: {rel}"));
    }
    let path = root
        .join(project.file(rel))
        .canonicalize()
        .map_err(|_| format!("not found: {rel}"))?;
    if !path.starts_with(docs)
        || (document && (!path.is_file() || path.extension().is_none_or(|e| e != "md")))
    {
        return Err(format!("not found: {rel}"));
    }
    Ok(path)
}

fn wire<T: Serialize, U: DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("review data serializes"))
        .expect("native and wire review shapes agree")
}

fn tasks(root: &Path, rel: &str) -> Vec<DocumentTask> {
    let paths = crate::render::doc_paths(root);
    crate::taskview::connect(root)
        .and_then(|store| {
            store
                .board()
                .map(|board| board.tasks)
                .map_err(|e| e.to_string())
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|task| {
            task.links.iter().any(|link| {
                let path = link.split('#').next().unwrap_or_default();
                path == rel
                    || (!root.join(path).is_file()
                        && Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .and_then(|name| crate::render::find_doc(name, path, &paths))
                            .as_deref()
                            == Some(rel))
            })
        })
        .map(|task| DocumentTask {
            slug: task.slug,
            title: task.title,
            status: task.status,
        })
        .collect()
}

pub fn document(render: &Render, rel: &str, scope: &str) -> Result<Document, String> {
    let path = target(render.root(), rel, true)?;
    let text = crate::text::read_text(&path).map_err(|e| e.to_string())?;
    let (text, _) = crate::register::supersede(&text, &crate::clock::now_stamp());
    let mut sections = render.typed_sections(&text, rel);
    for section in &mut sections {
        if let Some(slug) = section
            .html
            .split("data-slug=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        {
            section.key = format!("card:{slug}");
        }
    }
    crate::page::assign_parts(&mut sections);
    let cards = crate::register::parse_decisions(&crate::text::splitlines(&text))
        .into_iter()
        .map(|card| {
            let thread = card.thread(render.plugins());
            symphony_api::DocumentCard {
                slug: card.slug.clone(),
                state: card.state.as_str().into(),
                text: card.text.clone(),
                group: thread.group.key().into(),
                open: thread.open,
            }
        })
        .collect();
    let mut doc = Document {
        version: 0,
        path: rel.into(),
        title: crate::tree::first_heading(&text)
            .map(|title| crate::tree::plain(&title))
            .unwrap_or_else(|| {
                path.file_stem()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            }),
        waiting: crate::tree::pending(&text),
        crumbs: wire(crate::tree::crumbs(render.root(), rel)),
        headings: wire(crate::tree::headings(&text)),
        sections: wire(sections),
        cards,
        threads: wire(render.typed_history(&text, rel)),
        tasks: tasks(render.root(), rel),
        comparison: None,
    };
    for section in &mut doc.sections {
        prepare_fragment(
            &mut section.html,
            &mut section.media,
            render.root(),
            rel,
            scope,
        );
        section.blocks =
            crate::blocks::parse(&section.html, &section.media, &scoped_path(rel, scope));
        let mut digest = Sha256::new();
        digest.update(section.html.as_bytes());
        digest.update(serde_json::to_vec(&section.media).expect("media serializes"));
        digest.update(serde_json::to_vec(&section.blocks).expect("blocks serialize"));
        section.hash = digest
            .finalize()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
    }
    for fragment in doc.threads.values_mut() {
        prepare_fragment(
            &mut fragment.html,
            &mut fragment.media,
            render.root(),
            rel,
            scope,
        );
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    comparison::apply(render, rel, scope, &text, &mut doc);
    if !scope.is_empty() {
        doc.path = format!("@{scope}/{rel}");
        for crumb in &mut doc.crumbs {
            if let Some(href) = &mut crumb.href {
                *href = scoped(href, scope);
            }
        }
    }
    Ok(doc)
}

pub fn anchors(html: &str) -> Vec<String> {
    html.split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id.to_string()))
        .collect()
}

pub fn outline(mut document: Document) -> symphony_api::DocumentOutline {
    let anchors = document
        .sections
        .iter()
        .map(|section| {
            (
                section.key.clone(),
                if section.start == 0 {
                    Vec::new()
                } else {
                    anchors(&section.html)
                },
            )
        })
        .collect();
    for section in &mut document.sections {
        section.html.clear();
        section.media.clear();
        section.blocks.clear();
        if let Some(comparison) = &mut section.comparison {
            comparison.before.clear();
        }
    }
    for card in &mut document.cards {
        card.text = crate::tree::plain(&card.text).chars().take(140).collect();
    }
    document.threads.clear();
    symphony_api::DocumentOutline { document, anchors }
}

pub fn selected(
    document: &Document,
    keys: &[String],
    version: u64,
) -> symphony_api::DocumentSections {
    let mut change = symphony_api::DocumentChange {
        path: document.path.clone(),
        ..Default::default()
    };
    change.sections = document
        .sections
        .iter()
        .filter(|section| keys.contains(&section.key))
        .cloned()
        .collect();
    for card in document
        .cards
        .iter()
        .filter(|card| keys.contains(&format!("card:{}", card.slug)))
    {
        change.cards.push(symphony_api::ItemChange {
            key: card.slug.clone(),
            value: Some(card.clone()),
            removed: false,
        });
        change
            .threads
            .insert(card.slug.clone(), document.threads.get(&card.slug).cloned());
    }
    symphony_api::DocumentSections { version, change }
}

pub fn folder(render: &Render, rel: &str, scope: &str) -> Result<Folder, String> {
    target(render.root(), rel.trim_end_matches('/'), false)?;
    let page = render.folder(rel).map_err(|e| e.to_string())?;
    let mut folder = Folder {
        path: page.path.clone(),
        title: page.title,
        crumbs: wire(page.crumbs),
        docs: page.docs,
        intro: page
            .intro
            .map(|p| document(render, &p.path, scope))
            .transpose()?,
        entries: wire(page.entries),
    };
    if !scope.is_empty() {
        folder.path = format!("@{scope}/{}", folder.path);
        for entry in &mut folder.entries {
            entry.path = format!("@{scope}/{}", entry.path);
        }
        for crumb in &mut folder.crumbs {
            if let Some(href) = &mut crumb.href {
                *href = scoped(href, scope);
            }
        }
    }
    Ok(folder)
}

pub fn section(render: &Render, rel: &str, anchor: &str, scope: &str) -> Result<Fragment, String> {
    section_with(render, rel, anchor, scope, true)
}

/// The still-served legacy card excerpt needs ordinary markup rather than typed-parser markers.
pub fn legacy_section(
    render: &Render,
    rel: &str,
    anchor: &str,
    scope: &str,
) -> Result<Fragment, String> {
    section_with(render, rel, anchor, scope, false)
}

fn section_with(
    render: &Render,
    rel: &str,
    anchor: &str,
    scope: &str,
    typed: bool,
) -> Result<Fragment, String> {
    let path = target(render.root(), rel, true)?;
    let text = crate::text::read_text(&path).map_err(|e| e.to_string())?;
    let part = crate::edit::section_text(&text, anchor)
        .ok_or_else(|| format!("no such heading in {rel}"))?;
    let mut fragment: Fragment = wire(if typed {
        render.typed_reply(&part, rel)
    } else {
        render.reply(&part, rel)
    });
    prepare_fragment(
        &mut fragment.html,
        &mut fragment.media,
        render.root(),
        rel,
        scope,
    );
    if typed {
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    Ok(fragment)
}

pub fn history(
    render: &Render,
    rel: &str,
    scope: &str,
) -> Result<BTreeMap<String, Fragment>, String> {
    let path = target(render.root(), rel, true)?;
    let text = crate::text::read_text(&path).map_err(|e| e.to_string())?;
    let mut threads: BTreeMap<String, Fragment> = wire(render.typed_history(&text, rel));
    for fragment in threads.values_mut() {
        prepare_fragment(
            &mut fragment.html,
            &mut fragment.media,
            render.root(),
            rel,
            scope,
        );
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    Ok(threads)
}

fn scoped_path(rel: &str, scope: &str) -> String {
    if scope.is_empty() {
        rel.into()
    } else {
        format!("@{scope}/{rel}")
    }
}

pub fn changes(old: &Document, new: &Document) -> Option<DocumentChange> {
    let mut delta = DocumentChange {
        path: new.path.clone(),
        ..DocumentChange::default()
    };
    for section in &new.sections {
        match old.sections.iter().find(|s| s.key == section.key) {
            Some(before)
                if before.hash == section.hash && before.comparison == section.comparison =>
            {
                if (before.start, before.end, before.part)
                    != (section.start, section.end, section.part)
                {
                    delta.positions.push(SectionPosition {
                        key: section.key.clone(),
                        start: section.start,
                        end: section.end,
                        part: section.part,
                    });
                }
            }
            _ => delta.sections.push(section.clone()),
        }
    }
    delta.deleted = old
        .sections
        .iter()
        .filter(|s| !new.sections.iter().any(|n| n.key == s.key))
        .map(|s| s.key.clone())
        .collect();
    let keys = |doc: &Document| {
        doc.sections
            .iter()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>()
    };
    if keys(old) != keys(new) {
        delta.order = Some(keys(new));
    }
    for (slug, fragment) in &new.threads {
        if old.threads.get(slug) != Some(fragment) {
            delta.threads.insert(slug.clone(), Some(fragment.clone()));
        }
    }
    for slug in old.threads.keys().filter(|s| !new.threads.contains_key(*s)) {
        delta.threads.insert(slug.clone(), None);
    }
    macro_rules! field {
        ($name:ident) => {
            if old.$name != new.$name {
                delta.$name = Some(new.$name.clone());
            }
        };
    }
    for card in &new.cards {
        if old.cards.iter().find(|before| before.slug == card.slug) != Some(card) {
            delta.cards.push(symphony_api::ItemChange {
                key: card.slug.clone(),
                value: Some(card.clone()),
                removed: false,
            });
        }
    }
    for card in old
        .cards
        .iter()
        .filter(|card| !new.cards.iter().any(|after| after.slug == card.slug))
    {
        delta.cards.push(symphony_api::ItemChange {
            key: card.slug.clone(),
            value: None,
            removed: true,
        });
    }
    field!(title);
    field!(waiting);
    field!(crumbs);
    field!(headings);
    field!(tasks);
    if old.comparison != new.comparison {
        delta.comparison = Some(new.comparison.clone().into_iter().collect());
    }
    let empty = DocumentChange {
        path: new.path.clone(),
        ..DocumentChange::default()
    };
    (delta != empty).then_some(delta)
}

pub fn scoped(path: &str, scope: &str) -> String {
    if !scope.is_empty() && path.starts_with('/') && !path.starts_with("//") {
        format!("/@{scope}{path}")
    } else {
        path.into()
    }
}

/// Resolve URLs against the document rather than the SPA shell, reserve picture sizes/thumbnails,
/// and name assets by their current bytes before computing a section's hash.
pub fn prepare_fragment(
    html: &mut String,
    media: &mut [Media],
    root: &Path,
    rel: &str,
    scope: &str,
) {
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"((?:href|src|data-full)=")([^"]*)"#).expect("URL attribute pattern")
    });
    *html = URL
        .replace_all(html, |c: &regex::Captures<'_>| {
            let value = if c[2].starts_with('#') {
                format!("/{rel}{}", &c[2])
            } else {
                crate::markup::target(&c[2], rel)
            };
            format!("{}{}", &c[1], crate::markup::escape(&value))
        })
        .into_owned();
    *html = crate::images::reserve(html, root, false, crate::images::sips_available());
    *html = crate::assets::bust(html, root, &root.join("tools/static"), &format!("/{rel}")).0;
    for src in media.iter_mut().flat_map(media_urls) {
        if let Some(path) =
            crate::assets::local_file(src, root, &root.join("tools/static"), &format!("/{rel}"))
        {
            let token = crate::assets::validator(&path);
            let (path, anchor) = src.split_once('#').unwrap_or((src, ""));
            *src = format!(
                "{}{}v={token}{}",
                path,
                if path.contains('?') { "&" } else { "?" },
                if anchor.is_empty() {
                    String::new()
                } else {
                    format!("#{anchor}")
                }
            );
        }
    }
    scope_fragment(html, media, scope);
}

/// Existing pages consume string HTML from section/history reads. The SPA requests
/// `format=fragments` (or the document snapshot) for structured media instead.
pub fn legacy_html(fragment: &Fragment) -> String {
    let mut html = fragment.html.clone();
    for (index, item) in fragment.media.iter().enumerate() {
        let (kind, replacement) = match item {
            Media::Model { src, label, .. } => (
                "model",
                format!(
                    "<span class=\"glb-viewer\"><model-viewer src=\"{0}\" alt=\"{1}\" camera-controls touch-action=\"pan-y\" environment-image=\"neutral\" shadow-intensity=\"0\" interaction-prompt=\"none\" exposure=\"1\" camera-orbit=\"-45deg 60deg auto\" loading=\"lazy\"></model-viewer><span class=\"glb-bar\"><span class=\"glb-name\">{1}</span><button type=\"button\" data-glb-view=\"tilt\" title=\"The game's top-down camera, slightly tilted\">Top-down tilt</button><button type=\"button\" data-glb-view=\"reset\">Reset</button><a href=\"{0}\" download>.glb</a></span></span>",
                    crate::markup::escape(src),
                    crate::markup::escape(label)
                ),
            ),
            Media::Mockup { src, label } => (
                "mockup",
                format!(
                    "<span class=\"mockup\" data-w=\"1280\" data-h=\"800\"><span class=\"mockup-frame\"><iframe src=\"{0}\" sandbox=\"allow-scripts\" loading=\"lazy\" width=\"1280\" height=\"800\" title=\"{1}\" tabindex=\"-1\"></iframe><a class=\"mockup-open\" href=\"{0}\" target=\"_blank\" rel=\"noopener\" aria-label=\"Open {1} full size\"></a></span><span class=\"mockup-bar\"><span class=\"mockup-name\">{1}</span><a href=\"{0}\" target=\"_blank\" rel=\"noopener\">full size</a></span></span>",
                    crate::markup::escape(src),
                    crate::markup::escape(label)
                ),
            ),
            Media::Gallery { items } => (
                "gallery",
                items
                    .iter()
                    .map(|item| {
                        format!(
                            "<a href=\"{}\" download>{}</a>",
                            crate::markup::escape(&item.src),
                            crate::markup::escape(&item.name)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
        };
        html = html.replace(
            &format!("<span class=\"media\" data-media=\"{index}\" data-kind=\"{kind}\"></span>"),
            &replacement,
        );
    }
    html
}

pub fn scope_fragment(html: &mut String, media: &mut [Media], scope: &str) {
    if scope.is_empty() {
        return;
    }
    static LINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"((?:href|src|data-full)=")/([^/])"#).expect("local URL pattern")
    });
    *html = LINK
        .replace_all(html, |c: &regex::Captures<'_>| {
            format!("{}/@{scope}/{}", &c[1], &c[2])
        })
        .into_owned();
    for src in media.iter_mut().flat_map(media_urls) {
        *src = scoped(src, scope);
    }
}

fn media_urls(media: &mut Media) -> Vec<&mut String> {
    match media {
        Media::Mockup { src, .. } => vec![src],
        Media::Model { src, poster, .. } => std::iter::once(src).chain(poster.iter_mut()).collect(),
        Media::Gallery { items } => items
            .iter_mut()
            .flat_map(|item| std::iter::once(&mut item.src).chain(item.poster.iter_mut()))
            .collect(),
    }
}
