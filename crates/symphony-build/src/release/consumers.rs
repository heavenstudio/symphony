//! The projects a release's refresh moves (docs/RELEASES.md, *The refresh*): each project the
//! machine serves that builds on the Symphony (its project file names plugins, `projects.json`),
//! this repository aside, and the git sources its workspace takes the Symphony's packages from,
//! read from its `cargo metadata`. The release run builds and tests nothing of a project's: a
//! Symphony change that breaks one is found by its own gate when it takes the refresh's branch.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::group::{Bound, Failed, output_until};
use symphony::children::command;
use symphony::machine::Registry;
use symphony::project::Project;

/// A project a refresh moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Consumer {
    pub(crate) id: String,
    pub(crate) root: PathBuf,
    /// Its default branch, whose pin the refresh moves.
    pub(crate) branch: String,
}

/// The projects of `registry` a refresh moves, the repository whose common dir is `ours` aside,
/// and a line for each one left out and why. A project whose file names no plugins builds nothing
/// on the Symphony, and is passed over without a word.
pub(crate) fn consumers(registry: &Registry, ours: &Path) -> (Vec<Consumer>, Vec<String>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for entry in &registry.projects {
        if entry.plugins.is_empty() {
            continue;
        }
        if !entry.root.is_dir() {
            skipped.push(format!(
                "{}: its checkout {} is gone",
                entry.id,
                entry.root.display()
            ));
            continue;
        }
        match symphony_board::git::checkout_common_dir(&entry.root) {
            Some(common) if common == ours => {}
            Some(_) => found.push(Consumer {
                id: entry.id.clone(),
                root: entry.root.clone(),
                branch: Project::of(&entry.root).default_branch.clone(),
            }),
            None => skipped.push(format!(
                "{}: {} is not a git checkout",
                entry.id,
                entry.root.display()
            )),
        }
    }
    (found, skipped)
}

/// `cargo metadata --no-deps` of the workspace at `dir`, by `until`, with the toolchain its own
/// pin names, not the one the run's own build chose.
pub(crate) fn metadata(dir: &Path, until: Bound<'_>) -> Result<Value, String> {
    let mut cargo = command("cargo");
    cargo
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(dir)
        .env_remove("RUSTUP_TOOLCHAIN");
    let done = output_until(&mut cargo, until).map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run cargo in {}: {e}", dir.display()),
        Failed::Late(took) => format!(
            "cargo metadata in {} did not end within {} s and was stopped",
            dir.display(),
            took.as_secs()
        ),
        Failed::Interrupted(took) => format!(
            "cargo metadata in {} was stopped after {} s: the run was told to stop",
            dir.display(),
            took.as_secs()
        ),
    })?;
    if !done.status.success() {
        return Err(format!(
            "cargo metadata failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    serde_json::from_slice(&done.stdout)
        .map_err(|e| format!("cargo metadata in {} said no JSON: {e}", dir.display()))
}

/// The URL a dependency's git source names, without the branch, tag or commit it asks for.
pub(crate) fn git_url(source: &str) -> Option<&str> {
    let url = source.strip_prefix("git+")?;
    Some(url.split(['?', '#']).next().unwrap_or(url))
}

/// The git sources, by URL and sorted, that the members of a workspace whose metadata is
/// `metadata` take one of the Symphony's packages `names` from.
pub(crate) fn sources(metadata: &Value, names: &[String]) -> Vec<String> {
    let members: Vec<&str> = (metadata["workspace_members"].as_array().into_iter())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut sources: Vec<String> = Vec::new();
    for package in (metadata["packages"].as_array().into_iter()).flatten() {
        if !package["id"]
            .as_str()
            .is_some_and(|id| members.contains(&id))
        {
            continue;
        }
        for dependency in (package["dependencies"].as_array().into_iter()).flatten() {
            let named = dependency["name"].as_str().unwrap_or("");
            let Some(url) = dependency["source"].as_str().and_then(git_url) else {
                continue;
            };
            if names.iter().any(|name| name == named) && !sources.iter().any(|known| known == url) {
                sources.push(url.to_string());
            }
        }
    }
    sources.sort();
    sources
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;
    use serde_json::json;
    use std::fs;
    use std::time::{Duration, Instant};
    use symphony::machine::Entry;

    fn entry(id: &str, root: &Path, plugins: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            root: root.to_path_buf(),
            manifest: None,
            plugins: plugins.iter().map(|p| (*p).to_string()).collect(),
            pin: 1,
            schema: 16,
        }
    }

    fn repo(tree: &Tree, name: &str) -> PathBuf {
        let root = tree.path(name);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        super::super::git::git(&root, &["init", "-q", "-b", "main"]).unwrap();
        root
    }

    #[test]
    fn the_projects_a_refresh_moves_name_plugins_and_are_not_this_one() {
        let tree = Tree::new();
        let ours = repo(&tree, "ours");
        let theirs = repo(&tree, "theirs");
        let plain = repo(&tree, "plain");
        tree.write(
            "theirs/symphony.toml",
            "[project]\ndefault_branch = \"trunk\"\n",
        );
        let not_git = tree.path("folder");
        fs::create_dir_all(&not_git).unwrap();
        let registry = Registry {
            projects: vec![
                entry("ours", &ours, &["a"]),
                entry("theirs", &theirs, &["b"]),
                entry("plain", &plain, &[]),
                entry("gone", &tree.path("gone"), &["c"]),
                entry("loose", &not_git, &["d"]),
            ],
        };
        let (found, skipped) = consumers(&registry, &ours.join(".git"));
        assert_eq!(
            found,
            [Consumer {
                id: "theirs".into(),
                root: theirs,
                branch: "trunk".into()
            }]
        );
        assert_eq!(
            skipped,
            [
                format!("gone: its checkout {} is gone", tree.path("gone").display()),
                format!("loose: {} is not a git checkout", not_git.display()),
            ]
        );
    }

    #[test]
    fn a_workspaces_symphony_sources_are_each_git_source_a_member_takes_a_package_from() {
        let names = ["symphony".to_string(), "symphony-board".to_string()];
        let metadata = json!({
            "packages": [
                {"id": "host 0.1", "name": "host", "dependencies": [
                    {"name": "symphony", "source": "git+https://github.com/o/symphony?branch=main"},
                    {"name": "serde",
                     "source": "registry+https://github.com/rust-lang/crates.io-index"},
                ]},
                {"id": "tasks 0.1", "name": "tasks", "dependencies": [
                    {"name": "symphony-board",
                     "source": "git+https://github.com/o/symphony?branch=main#abc"},
                    {"name": "symphony-board", "source": "git+ssh://git@github.com/o/symphony.git"},
                ]},
                {"id": "game 0.1", "name": "game", "dependencies": [
                    {"name": "symphony-like", "source": "git+https://example.com/other"},
                    {"name": "symphony", "source": null, "path": "/elsewhere"},
                ]},
                {"id": "dep 0.1", "name": "outside", "dependencies": [
                    {"name": "symphony", "source": "git+https://example.com/not-a-member"},
                ]},
            ],
            "workspace_members": ["host 0.1", "tasks 0.1", "game 0.1"],
        });
        assert_eq!(
            sources(&metadata, &names),
            [
                "https://github.com/o/symphony",
                "ssh://git@github.com/o/symphony.git"
            ]
        );
        assert!(sources(&json!({}), &names).is_empty());
        assert!(sources(&metadata, &[]).is_empty());
    }

    #[test]
    fn a_workspaces_metadata_is_read_by_its_own_cargo_and_a_failure_says_where() {
        let tree = Tree::new();
        // Each folder pins the toolchain this workspace pins: the test never reaches for another.
        let pin = include_str!("../../../../rust-toolchain.toml");
        tree.write("w/rust-toolchain.toml", pin);
        tree.write(
            "w/Cargo.toml",
            "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        tree.write("w/src/lib.rs", "");
        // A bound only a stall reaches: the backstop of a two-minute allowance.
        let until = Bound::until(Instant::now() + test_time::backstop(Duration::from_secs(120)));
        let read = metadata(&tree.path("w"), until).unwrap();
        assert_eq!(read["packages"][0]["name"], "w");
        tree.write("none/rust-toolchain.toml", pin);
        let none = tree.path("none");
        let said = metadata(&none, until).unwrap_err();
        assert!(
            said.starts_with(&format!("cargo metadata failed in {}: ", none.display())),
            "{said}"
        );
    }

    #[test]
    fn a_dependencys_git_url_drops_what_it_asks_for() {
        assert_eq!(
            git_url("git+https://h/o/r?branch=main#abc"),
            Some("https://h/o/r")
        );
        assert_eq!(
            git_url("git+file:///tmp/x.git#abc"),
            Some("file:///tmp/x.git")
        );
        assert_eq!(git_url("registry+https://h"), None);
    }
}
