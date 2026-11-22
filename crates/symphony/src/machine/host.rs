//! The host rule: which build is the machine's one server, and which projects it serves. A build
//! serves a project's plugin when the plugin answers from a process of its own (its
//! `[plugins.process]`, docs/engineering/PLUGIN_PROTOCOL.md), which any host starts, or when the build compiles
//! it: so the server of a release installed on the machine ([`super::release`]), which compiles
//! none, serves every project whose plugins all answer from a process. A host serves a project
//! registered at its pin or an older one and at its board schema or an older one, and refuses one
//! whose pin or board schema is newer than its own, naming who must bump; a release's server also
//! serves a project registered one board schema ahead of it, whose board it reads, and writes only
//! where the step that moved it is declared to keep a Symphony one behind writing
//! ([`symphony_board::schema::COMPATIBLE`]), as every reader does.
//!
//! While a project's build serves, its verdicts are the compiled plugins': a project with no plugins
//! never replaces a running host and is served by any host holding its pin; a project whose linked
//! plugins the running host lacks is registered and waits for its own `just symphony`, which
//! replaces the host by the socket hand-off, every project kept registered. While a release's server
//! serves, no start replaces it, as its reload follows the machine's `current` link: a project
//! naming a linked plugin is registered and refused, its pages saying why, and one whose pin or
//! schema is newer is refused until a release that serves it is installed. Which build a start runs
//! when none does, and what holds the machine on a project's build, is [`super::switch`]. A start in
//! a linked worktree never replaces the running host, whichever build it runs: only a start in a
//! main checkout does.

use std::path::{Path, PathBuf};

use super::release::Manifest;
use super::{Entry, Registry};

/// What a build of the server holds that a project needs: its plugins, its pin and its board
/// schema, the project that built it, which a refusal names, and the release it is, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub plugins: Vec<String>,
    pub pin: u32,
    pub schema: i64,
    /// The main checkout of the project that started it, when known.
    pub by: String,
    /// The linked worktree it was started in, when that is not a main checkout: a start there never
    /// replaces the running server, whichever build it runs.
    pub worktree: Option<PathBuf>,
    /// The release installed on the machine it is, when it is one: it compiles no plugin, and
    /// its server, once the machine's, is replaced only by its reload following `current`.
    pub release: Option<String>,
}

impl Build {
    /// This process: the plugins its host installed, this build's pin and board schema, and the
    /// release its binary is of ([`super::release::this`]).
    pub fn this(by: &Path) -> Build {
        Build {
            plugins: crate::plugins::compiled()
                .iter()
                .map(|plugin| plugin.name().to_string())
                .collect(),
            pin: super::PIN,
            schema: symphony_board::SCHEMA,
            by: by.display().to_string(),
            worktree: None,
            release: super::release::this().map(|(_, manifest)| manifest.release.clone()),
        }
    }

    /// The running server, as its `/api/front` says.
    pub fn of(front: &symphony_api::Front) -> Build {
        Build {
            plugins: front.plugins.clone(),
            pin: front.pin,
            schema: front.schema,
            by: front.root.clone(),
            worktree: None,
            release: front.release.clone(),
        }
    }

    /// The server of the installed release `manifest` names, as a start for the project at `by`
    /// would run it: no plugin compiled in, its pin and board schema as its manifest says.
    pub fn released(manifest: &Manifest, by: &Path) -> Build {
        Build {
            plugins: Vec::new(),
            pin: manifest.serves.pin,
            schema: manifest.serves.schema,
            by: by.display().to_string(),
            worktree: None,
            release: Some(manifest.release.clone()),
        }
    }

    /// This build as a start in the folder `dir` runs it, which may be a linked worktree
    /// ([`worktree_of`]).
    pub fn started_in(self, dir: &Path) -> Build {
        Build {
            worktree: worktree_of(dir),
            ..self
        }
    }

    /// Why this build cannot serve `entry`, said after the build's name; none when it can. A plugin
    /// is lacking when it is linked ([`Entry::linked`]) and this build does not compile it.
    pub fn lacks(&self, entry: &Entry) -> Option<Lack> {
        let missing: Vec<String> = (entry.linked().into_iter())
            .filter(|name| !self.plugins.contains(name))
            .collect();
        if !missing.is_empty() {
            return Some(Lack::Plugins(missing));
        }
        if entry.pin > self.pin {
            return Some(Lack::Pin(entry.pin));
        }
        // A release reads a board one schema ahead of its own, and writes it only where that
        // schema's step keeps it writing.
        let reach = self.schema + i64::from(self.release.is_some());
        (entry.schema > reach).then_some(Lack::Schema(entry.schema))
    }

    /// The first project this build cannot serve, with what it lacks, of `entry` and every other
    /// project of `registry`: `entry` is the project's new entry, judged in place of its old one.
    pub fn unserved<'a>(
        &self,
        entry: &'a Entry,
        registry: &'a Registry,
    ) -> Option<(&'a Entry, Lack)> {
        (registry.projects.iter())
            .filter(|other| other.id != entry.id && other.root != entry.root)
            .chain([entry])
            .find_map(|project| self.lacks(project).map(|lack| (project, lack)))
    }

    /// Whether this build is older than `other` in its pin or its board schema.
    fn older_than(&self, other: &Build) -> bool {
        self.pin < other.pin || self.schema < other.schema
    }
}

/// What a project needs that a build lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lack {
    /// The linked plugins it names that the build does not compile.
    Plugins(Vec<String>),
    /// The pin it was registered at, newer than the build's.
    Pin(u32),
    /// The board schema it was registered at, newer than the build's.
    Schema(i64),
}

impl Lack {
    /// The lack, said of `build` for the project `id`.
    pub fn said(&self, build: &Build, id: &str) -> String {
        let named = |names: &[String]| {
            let plugins = if names.len() == 1 {
                "the plugin"
            } else {
                "the plugins"
            };
            format!("{plugins} {}", names.join(", "))
        };
        match (self, &build.release) {
            (Lack::Plugins(names), None) => format!("lacks {}, which {id} names", named(names)),
            (Lack::Plugins(names), Some(release)) => format!(
                "is the release {release}, which serves a plugin only from its process: {id} names \
                 {} linked",
                named(names)
            ),
            (Lack::Pin(pin), None) => {
                format!("is at Symphony pin {}, older than {id}'s {pin}", build.pin)
            }
            (Lack::Pin(pin), Some(release)) => format!(
                "is the release {release}, at Symphony pin {}, older than {id}'s {pin}",
                build.pin
            ),
            (Lack::Schema(schema), None) => format!(
                "reads board schema {}, older than {id}'s {schema}",
                build.schema
            ),
            (Lack::Schema(schema), Some(release)) => format!(
                "is the release {release}, which reads board schema {} and one ahead, older than \
                 {id}'s {schema}",
                build.schema
            ),
        }
    }
}

/// What the host rule decides for a project registering while a server runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The running server serves it.
    Serves,
    /// The running server cannot serve it and this build can serve every registered project: this
    /// build replaces it. Why, in one line.
    Replace(String),
    /// The running server lacks its linked plugins and this build cannot replace it: it is
    /// registered, and served once what the line says is done (its own `just symphony` replaces a
    /// project's build; a release's server serves it once each plugin answers from a process).
    Waits(String),
    /// Its pin or board schema is newer than the running server's and this build cannot replace
    /// it: it is not registered. Who must bump, in one line.
    Refuse(String),
}

/// The host rule for `entry`, which a start or `project add` of `ours` registers beside
/// `registry`'s projects, while `running` is the machine's server.
pub fn judge(running: &Build, ours: &Build, entry: &Entry, registry: &Registry) -> Verdict {
    let Some(lack) = running.lacks(entry) else {
        return Verdict::Serves;
    };
    let lacks = lack.said(running, &entry.id);
    if running.release.is_some() {
        return match lack {
            Lack::Plugins(names) => Verdict::Waits(format!(
                "the running Symphony {lacks}, and no start replaces a release's server: a \
                 [plugins.process] command for {} in {} lets it serve {} \
                 (docs/engineering/PLUGIN_PROTOCOL.md)",
                names.join(", "),
                entry.root.join(crate::project::FILE).display(),
                entry.id
            )),
            Lack::Pin(_) | Lack::Schema(_) => Verdict::Refuse(format!(
                "the running Symphony {lacks}, and no start replaces a release's server: it serves \
                 {} once a release that does is installed, which its reload follows",
                entry.id
            )),
        };
    }
    let unserved = ours.unserved(entry, registry);
    let replaces = ours.worktree.is_none()
        && ours.release.is_none()
        && !ours.plugins.is_empty()
        && unserved.is_none()
        && !ours.older_than(running);
    if replaces {
        return Verdict::Replace(format!(
            "the running Symphony {lacks}; this build serves every registered project and \
             replaces it"
        ));
    }
    let why = match (unserved, &ours.worktree) {
        _ if ours.plugins.is_empty() && !entry.linked().is_empty() => {
            format!(
                "run `just symphony` in {}, whose build holds them",
                entry.root.display()
            )
        }
        _ if ours.older_than(running) => format!(
            "this build is older than the running Symphony: {} must bump its Symphony",
            entry.root.display()
        ),
        (None, _) if ours.release.is_some() => format!(
            "a start of a release never replaces a running Symphony: `just symphony --stop`, then \
             `just symphony` in {}, serves every registered project from the release",
            entry.root.display()
        ),
        (Some((project, lack)), _) if ours.release.is_some() => format!(
            "a start of a release never replaces a running Symphony, and this one {}: {} must \
             bump its Symphony, then run `just symphony` there",
            lack.said(ours, &project.id),
            running.by
        ),
        _ if ours.plugins.is_empty() => format!(
            "a project with no plugins never replaces the running Symphony: {} must bump its \
             Symphony, then run `just symphony` there",
            running.by
        ),
        (_, Some(worktree)) => format!(
            "this start runs in the linked worktree {}, and a start in a linked worktree never \
             replaces the running Symphony: run `just symphony` in {}",
            worktree.display(),
            entry.root.display()
        ),
        (Some((project, lack)), None) => format!(
            "this build cannot replace it: it {}; one host must compile every registered \
             project's plugins",
            lack.said(ours, &project.id)
        ),
        (None, None) => {
            unreachable!(
                "a main checkout's build with plugins, not older, serving every project replaces"
            )
        }
    };
    match lack {
        Lack::Plugins(_) => Verdict::Waits(format!("the running Symphony {lacks}: {why}")),
        Lack::Pin(_) | Lack::Schema(_) => {
            Verdict::Refuse(format!("the running Symphony {lacks}: {why}"))
        }
    }
}

/// The top folder of the linked worktree `dir` is in, read from the `.git` files with no `git`
/// process; none for a main checkout, or a folder in no checkout. A linked worktree's `.git` file
/// names a gitdir of its own, whose `commondir` names the repository's; a submodule's or a
/// `--separate-git-dir` checkout's names the repository's gitdir itself, which has no `commondir`,
/// so each is its repository's main checkout ([`symphony_board::git::checkout_common_dir`]).
pub fn worktree_of(dir: &Path) -> Option<PathBuf> {
    let (top, _) = symphony_board::git::checkout(dir)?;
    let common = symphony_board::git::checkout_common_dir(&top)?;
    let marker = std::fs::read_to_string(top.join(".git")).ok()?;
    let gitdir = top.join(marker.trim().strip_prefix("gitdir:")?.trim());
    (gitdir.canonicalize().ok()? != common).then_some(top)
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
