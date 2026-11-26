//! `projects.json`, the registry of the projects the machine's server serves: each entry read from
//! a project's file, and the file read, changed under its lock and written whole.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{LOCK, PIN, PROJECTS};
use crate::project::{FILE, Project};

/// One registered project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The URL segment, `/<id>/`: the project file's id.
    pub id: String,
    pub name: String,
    /// The project's main checkout, canonical; its worktrees are found from its git common dir.
    pub root: PathBuf,
    /// Its `symphony.toml`; none for a root without one, a plain project.
    #[serde(default)]
    pub manifest: Option<PathBuf>,
    /// The plugins its file names, which the host must compile to serve it.
    #[serde(default)]
    pub plugins: Vec<String>,
    /// The pin of the build that registered it ([`PIN`]).
    #[serde(default)]
    pub pin: u32,
    /// The board schema of that build.
    #[serde(default)]
    pub schema: i64,
}

impl Entry {
    /// The project at `root` as this build registers it: its file's id, name and plugins, with
    /// this build's pin and board schema.
    ///
    /// # Errors
    ///
    /// When `root` is not a folder, or its project file does not load.
    pub fn of(root: &Path) -> Result<Entry, String> {
        let root = root
            .canonicalize()
            .map_err(|e| crate::text::os_error(&e, root))?;
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let project = Project::load(&root)?;
        let manifest = root.join(FILE);
        Ok(Entry {
            id: project.id,
            name: project.name,
            manifest: manifest.is_file().then_some(manifest),
            root,
            plugins: project.plugins,
            pin: PIN,
            schema: symphony_board::SCHEMA,
        })
    }

    /// Whether the entry's root still holds the project it names: the folder is there and its
    /// project file gives the same id.
    pub fn holds(&self) -> bool {
        self.root.is_dir() && Project::load(&self.root).is_ok_and(|project| project.id == self.id)
    }

    /// The plugins it names that are linked into the host: each one its file gives no
    /// `[plugins.process]` command, read from the file as it is now
    /// ([`crate::project::PluginProcess::named_at`]), so a command written since it registered
    /// counts at once. A plugin that answers from a process any host serves.
    pub fn linked(&self) -> Vec<String> {
        let processes = crate::project::PluginProcess::named_at(&self.root);
        (self.plugins.iter())
            .filter(|name| !processes.contains(name))
            .cloned()
            .collect()
    }
}

/// What [`Registry::add`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Added {
    /// A project the registry did not hold.
    New,
    /// The same entry again: nothing changed.
    Same,
    /// The project at that root, read again: its id, name, plugins or pin changed.
    Updated,
    /// The id's project, from the root it left (that root no longer holds it).
    Moved(PathBuf),
}

/// `projects.json`: the projects the machine's server serves, in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub projects: Vec<Entry>,
}

impl Registry {
    /// The registry in the machine folder `dir`; none written is empty.
    ///
    /// # Errors
    ///
    /// When it cannot be read or does not parse: a registry that is not read is never written over.
    pub fn read(dir: &Path) -> Result<Registry, String> {
        let path = dir.join(PROJECTS);
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_slice(text.as_bytes())
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(crate::text::os_error(&e, &path)),
        }
    }

    /// Write the registry whole into `dir`, so a reader never sees half of it.
    ///
    /// # Errors
    ///
    /// When the folder or the file cannot be written.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| crate::text::os_error(&e, dir))?;
        let path = dir.join(PROJECTS);
        let next = dir.join(format!("{PROJECTS}.{}", std::process::id()));
        let text = serde_json::to_string_pretty(self).expect("a registry serializes") + "\n";
        let written = fs::write(&next, text).and_then(|()| fs::rename(&next, &path));
        if written.is_err() {
            let _ = fs::remove_file(&next);
        }
        written.map_err(|e| crate::text::os_error(&e, &path))
    }

    /// Read the registry in `dir`, `change` it and write it back, under the registry's lock: what
    /// `change` returns. A change that fails writes nothing.
    ///
    /// # Errors
    ///
    /// When the registry cannot be locked, read or written, or `change` fails.
    pub fn change<T>(
        dir: &Path,
        change: impl FnOnce(&mut Registry) -> Result<T, String>,
    ) -> Result<T, String> {
        fs::create_dir_all(dir).map_err(|e| crate::text::os_error(&e, dir))?;
        let path = dir.join(LOCK);
        let lock: File = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| crate::text::os_error(&e, &path))?;
        lock.lock().map_err(|e| crate::text::os_error(&e, &path))?;
        let mut registry = Registry::read(dir)?;
        let before = registry.clone();
        let result = change(&mut registry)?;
        if registry != before {
            registry.write(dir)?;
        }
        Ok(result)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.projects.iter().find(|entry| entry.id == id)
    }

    /// Register `entry`: the project at its root is read again in place, an id another root left
    /// moves to this one, and a new project goes last.
    ///
    /// # Errors
    ///
    /// When the id is a path the one server answers before any project (a Symphony route, or the
    /// first segment of a route a registered project's pages answer, which today's links redirect
    /// from), or another registered root still holds a project of that id.
    pub fn add(&mut self, entry: Entry) -> Result<Added, String> {
        if let Some(taken) = self.taken(&entry) {
            return Err(format!(
                "the project id {:?} would take /{}/, which {taken}; give {} another [project] id \
                 in {FILE}",
                entry.id,
                entry.id,
                entry.root.display()
            ));
        }
        if let Some(other) = self
            .projects
            .iter()
            .find(|other| other.id == entry.id && other.root != entry.root)
            && other.holds()
        {
            return Err(format!(
                "the project id {:?} is already {}'s; give {} another [project] id in {FILE}",
                entry.id,
                other.root.display(),
                entry.root.display()
            ));
        }
        let moved = self
            .projects
            .iter()
            .position(|other| other.id == entry.id && other.root != entry.root);
        let at_root = self
            .projects
            .iter()
            .position(|other| other.root == entry.root);
        let added = match (at_root, moved) {
            (Some(at), _) if self.projects[at] == entry => return Ok(Added::Same),
            (Some(at), moved) => {
                self.projects[at] = entry;
                if let Some(moved) = moved {
                    self.projects.remove(moved);
                }
                Added::Updated
            }
            (None, Some(at)) => {
                let from = std::mem::replace(&mut self.projects[at], entry).root;
                Added::Moved(from)
            }
            (None, None) => {
                self.projects.push(entry);
                Added::New
            }
        };
        Ok(added)
    }

    /// Who answers `/<entry.id>/` before any project, if anyone: a Symphony route, or a route of a
    /// registered project's pages (or the entry's own), which the server redirects from.
    fn taken(&self, entry: &Entry) -> Option<String> {
        let id = entry.id.as_str();
        let first = |route: &str| route.split('/').next() == Some(id);
        if symphony_api::SYMPHONY_ROUTES
            .iter()
            .any(|route| first(route))
        {
            return Some("is a route the Symphony answers".into());
        }
        let mut roots: BTreeSet<&Path> = BTreeSet::new();
        roots.extend(
            self.projects
                .iter()
                .map(|other| other.root.as_path())
                .chain([entry.root.as_path()]),
        );
        roots.into_iter().find_map(|root| {
            let project = Project::of(root);
            let mut routes: Vec<String> = project.reserved();
            for entrypoint in &project.entrypoints {
                if entrypoint.url.is_empty() {
                    // An entrypoint at `/` answers every name its folder holds there.
                    project
                        .walk_folders(root, entrypoint, "", &mut |name| routes.push(name.into()));
                    project.walk_files(root, entrypoint, "", &mut |name| routes.push(name.into()));
                } else {
                    routes.push(entrypoint.url.clone());
                }
            }
            routes
                .iter()
                .any(|route| first(route))
                .then(|| format!("{}'s pages answer at the server's root", project.id))
        })
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
