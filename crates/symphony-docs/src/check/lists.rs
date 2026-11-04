//! The Folders list in `docs/README.md` names every folder under `docs/`, and a list README lists
//! every document and subfolder of its folder and nothing that is gone.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::{DOCS, Docs, Problem, ROOT_README, extension, problem, resolve};
use crate::syntax;

/// A list row: a bullet (`-`, `*` or `+` and a space) or a table row (`|`), after any indent.
fn is_row(line: &str) -> bool {
    let body = line.trim_start_matches(syntax::is_space);
    body.starts_with('|')
        || (body.starts_with(['-', '*', '+']) && body[1..].starts_with(syntax::is_space))
}

fn subfolders(path: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

pub(super) fn folders_list(tree: &Docs, problems: &mut Vec<Problem>) {
    if !tree.has_doc(ROOT_README) {
        return;
    }
    let mut listed: BTreeSet<String> = BTreeSet::new();
    let mut inside = false;
    for line in syntax::lines(tree.text(ROOT_README)) {
        if let Some(rest) = line.strip_prefix("##")
            && rest.starts_with(syntax::is_space)
            && rest.trim_matches(syntax::is_space) == "Folders"
        {
            inside = true;
        } else if line.starts_with('#') {
            inside = false;
        } else if inside && is_row(line) {
            listed.extend(
                syntax::line_links(line)
                    .into_iter()
                    .map(|t| t.split('/').next().unwrap_or("").to_string()),
            );
        }
    }
    let real: BTreeSet<String> = subfolders(&tree.root.join(DOCS)).into_iter().collect();
    for name in real.difference(&listed) {
        problem(
            problems,
            ROOT_README,
            0,
            format!("the Folders list is missing {name}/ (a folder under docs/)"),
        );
    }
    for name in listed.difference(&real) {
        problem(
            problems,
            ROOT_README,
            0,
            format!("the Folders list names {name}/, but there is no such folder under docs/"),
        );
    }
}

fn holds_docs(tree: &Docs, folder: &Path) -> bool {
    let prefix = format!("{}/", tree.rel(folder));
    tree.docs.iter().any(|rel| rel.starts_with(&prefix))
}

pub(super) fn guides(tree: &Docs, problems: &mut Vec<Problem>) {
    for rel in tree
        .docs
        .iter()
        .filter(|rel| rel.ends_with("/README.md") && rel.as_str() != ROOT_README)
    {
        let folder = tree
            .root
            .join(rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut listed: HashSet<PathBuf> = HashSet::new();
        let mut dead: Vec<(usize, &str)> = Vec::new();
        let text = tree.text(rel);
        let lines = syntax::lines(text);
        for (index, line) in syntax::outside_fences(&lines).into_iter().enumerate() {
            let Some(line) = line.filter(|l| is_row(l)) else {
                continue;
            };
            let Some(first) = syntax::line_links(line).into_iter().next() else {
                continue;
            };
            let Some((mut path, _)) = resolve(tree, rel, first) else {
                continue;
            };
            if path.file_name().is_some_and(|n| n == "README.md")
                && path.parent().and_then(Path::parent) == Some(folder.as_path())
            {
                path.pop();
            }
            let ext = extension(&path);
            if path.parent() != Some(folder.as_path()) || !(ext.is_empty() || ext == ".md") {
                continue;
            }
            if tree.exists(&path) {
                listed.insert(path);
            } else {
                dead.push((index + 1, first));
            }
        }
        if listed.is_empty() && dead.is_empty() {
            continue; // prose: it says what the folder holds without listing it
        }
        for (line, target) in dead {
            problem(
                problems,
                rel,
                line,
                format!("lists {target}, which does not exist"),
            );
        }
        let prefix = format!("{}/", tree.rel(&folder));
        let mut wanted: Vec<(PathBuf, bool)> = tree
            .docs
            .iter()
            .filter_map(|doc| doc.strip_prefix(&prefix))
            .filter(|name| !name.contains('/') && *name != "README.md")
            .map(|name| (folder.join(name), false))
            .collect();
        wanted.extend(
            subfolders(&folder)
                .into_iter()
                .map(|name| folder.join(name))
                .filter(|p| holds_docs(tree, p))
                .map(|p| (p, true)),
        );
        for (path, dir) in wanted {
            if !listed.contains(&path) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                problem(
                    problems,
                    rel,
                    0,
                    format!(
                        "does not list {name}{} (add a row for it)",
                        if dir { "/" } else { "" }
                    ),
                );
            }
        }
    }
}
