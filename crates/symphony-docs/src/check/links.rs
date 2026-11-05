//! Reachable through READMEs, and links: every document is reached from `docs/README.md` through
//! its folders' READMEs, and every inline link and `[[NAME#Heading]]` resolves, heading included.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::{
    Anchors, DOCS, Docs, Problem, ROOT_README, extension, is_dir, normpath, page_for, problem,
    resolve,
};
use crate::syntax;

// --- reachable through READMEs ---

pub(super) fn reachable(tree: &Docs, problems: &mut Vec<Problem>) {
    if !tree.has_doc(ROOT_README) {
        problem(
            problems,
            ROOT_README,
            0,
            "missing, nothing can be reached from it".to_string(),
        );
        return;
    }
    let mut seen: HashSet<String> = HashSet::from([ROOT_README.to_string()]);
    let mut queue = VecDeque::from([ROOT_README.to_string()]);
    while let Some(rel) = queue.pop_front() {
        let folder = tree
            .root
            .join(&rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for (_, target) in syntax::links(tree.text(&rel)) {
            let Some((path, _)) = resolve(tree, &rel, &target) else {
                continue;
            };
            let Some(page) = page_for(tree, &path) else {
                continue;
            };
            if seen.contains(&page) {
                continue;
            }
            let path = tree.root.join(&page);
            let readme = path.file_name().is_some_and(|n| n == "README.md");
            let parent = path.parent().unwrap_or(&tree.root);
            if parent == folder || (readme && parent.parent() == Some(folder.as_path())) {
                seen.insert(page.clone());
                if readme {
                    queue.push_back(page);
                }
            }
        }
    }
    for rel in &tree.documents {
        if seen.contains(rel) {
            continue;
        }
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        if rel.ends_with("/README.md") {
            parts.pop();
        }
        let owner = format!("{}/README.md", parts.join("/"));
        problem(
            problems,
            rel,
            0,
            format!(
                "not reachable from {ROOT_README}: {owner} must link it (a link in a README, not in another page)"
            ),
        );
    }
}

// --- links ---

pub(super) fn links(tree: &Docs, problems: &mut Vec<Problem>, watch: &mut BTreeSet<PathBuf>) {
    let mut anchors = Anchors {
        tree,
        cache: HashMap::new(),
    };
    let artifacts = tree.root.join("artifacts");
    let docs = tree.root.join(DOCS);
    for rel in tree.live() {
        for (line, target) in syntax::links(tree.text(rel)) {
            let Some((path, anchor)) = resolve(tree, rel, &target) else {
                continue;
            };
            if !tree.exists(&path) {
                if !(path.starts_with(&artifacts) && path != artifacts) {
                    problem(problems, rel, line, format!("[{target}]: no such file"));
                }
                continue;
            }
            if !path.starts_with(&docs) && !path.starts_with(&artifacts) {
                if is_dir(&path) {
                    // Cargo watches docs/ and each file named here; a folder outside docs/ moved away
                    // would leave the build green until some document changed.
                    problem(
                        problems,
                        rel,
                        line,
                        format!(
                            "[{target}]: a folder outside docs/, which the build cannot watch; \
                             link a file in it (its README.md or AGENTS.md)"
                        ),
                    );
                    continue;
                }
                watch.insert(path.clone());
            }
            if anchor.is_empty() {
                continue;
            }
            let doc = if is_dir(&path) {
                path.join("README.md")
            } else {
                path
            };
            if extension(&doc) == ".md"
                && tree.exists(&doc)
                && doc.is_file()
                && !anchors.of(&doc).contains(&anchor)
            {
                problem(
                    problems,
                    rel,
                    line,
                    format!("[{target}]: no heading '{anchor}' in {}", tree.rel(&doc)),
                );
            }
        }
    }
}

/// The document a `[[NAME]]` names: the path it spells from the linking page if that exists, else the
/// first document whose path ends with it (live ones before history), so a document can move folders.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "a link names a document with `.md` in lower case, as the renderer resolves it"
)]
fn find_doc<'a>(target: &str, from: &str, order: &[&'a str]) -> Option<&'a str> {
    let mut name = target.trim_matches(syntax::is_space).to_string();
    if !name.ends_with(".md") {
        name.push_str(".md");
    }
    let folder = from.rsplit_once('/').map_or("", |(dir, _)| dir);
    let direct = if name.starts_with('/') {
        normpath(&name)
    } else {
        normpath(&format!("{folder}/{name}"))
    };
    if let Some(found) = order.iter().find(|p| **p == direct) {
        return Some(found);
    }
    let tail = format!("/{}", name.trim_start_matches('/'));
    order.iter().find(|p| p.ends_with(&tail)).copied()
}

pub(super) fn wikilinks(tree: &Docs, problems: &mut Vec<Problem>) {
    let order = tree.search_order();
    for rel in tree.live() {
        for link in syntax::wikilinks(tree.text(rel)) {
            if link.target.contains(':') {
                continue; // a task or a component, not a document
            }
            let target = link.target.trim_matches(syntax::is_space);
            let Some(found) = find_doc(target, rel, &order) else {
                problem(
                    problems,
                    rel,
                    link.line,
                    format!("{}: no document called {target}", link.whole),
                );
                continue;
            };
            if let Some(heading) = &link.heading
                && !tree
                    .doc_anchors(found)
                    .contains(&syntax::heading_anchor(heading))
            {
                problem(
                    problems,
                    rel,
                    link.line,
                    format!("{}: no heading '{heading}' in {found}", link.whole),
                );
            }
        }
    }
}
