//! The switch: when the machine's one server is the release installed on it. A release's server
//! compiles no plugin ([`super::host`]), so it serves every registered project only while none
//! names a plugin linked into the host: the first that does holds the machine on a project's build
//! ([`held`]), and `just release --status` ([`standing`]) and that project's pages say so. While
//! nothing holds it, a start of the one server that finds none running starts the server of the
//! release `current` names in place of its own build ([`to_start`]), so no project's build is the
//! machine's server; a project's build already running is switched by a stop and a start, an act
//! of its own, as no start replaces a running server with a release's. While something holds it,
//! a start by the release's own binary (`just symphony` in a project with no build of its own)
//! starts nothing and names the project that holds it ([`refused`]). The refresh reads the same
//! rule: while it holds, a release is installed and `current` is not pointed at it, which
//! [`standing`] says with the project that holds it.

use std::path::{Path, PathBuf};

use super::host::Build;
use super::release::{self, Manifest};
use super::{Entry, Registry};

/// What holds the machine's server on a project's build: a registered project naming plugins
/// linked into the host ([`Entry::linked`]), which a release's server cannot serve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub id: String,
    pub root: PathBuf,
    /// The linked plugins it names, in its file's order.
    pub plugins: Vec<String>,
}

impl Hold {
    /// The hold, in one line: who holds it, by which plugins, and what lets it go, its project file
    /// by its path, as the status and a start say it.
    pub fn said(&self) -> String {
        self.told(&self.root.join(crate::project::FILE).display().to_string())
    }

    /// The hold as the holding project's own pages say it: its project file named as its own
    /// (`its symphony.toml`), as the reader of its pages knows where its checkout is.
    pub fn said_on_its_pages(&self) -> String {
        self.told(&format!("its {}", crate::project::FILE))
    }

    /// The hold's line, its project file named `file`.
    fn told(&self, file: &str) -> String {
        let (plugins, them) = if self.plugins.len() == 1 {
            ("plugin", "it")
        } else {
            ("plugins", "each")
        };
        format!(
            "{} names the linked {plugins} {}, and a release's server serves a plugin only from its \
             process: a [plugins.process] command for {them} in {file} lets the release serve every \
             project (docs/engineering/PLUGIN_PROTOCOL.md)",
            self.id,
            self.plugins.join(", "),
        )
    }
}

/// The switch rule: the first project of `registry` that holds the machine's server on a project's
/// build; none while a release's server can serve every registered project's plugins.
pub fn held(registry: &Registry) -> Option<Hold> {
    registry.projects.iter().find_map(|entry| {
        let plugins = entry.linked();
        (!plugins.is_empty()).then(|| Hold {
            id: entry.id.clone(),
            root: entry.root.clone(),
            plugins,
        })
    })
}

/// What holds the switch to the release `build` in `registry`: the first project naming a linked
/// plugin ([`held`]), as the refresh reads it, else the first project the release is too old for
/// (its pin or its board schema). That project's id and root, and why, in words.
pub fn holding(build: &Build, registry: &Registry) -> Option<(String, PathBuf, String)> {
    if let Some(hold) = held(registry) {
        let why = hold.said();
        return Some((hold.id, hold.root, why));
    }
    registry.projects.iter().find_map(|entry| {
        let lack = build.lacks(entry)?;
        let why = format!("its server {}", lack.said(build, &entry.id));
        Some((entry.id.clone(), entry.root.clone(), why))
    })
}

/// The server a start of the one server for `entry` runs in `machine` when none runs, in place of
/// its own build: that of the release `current` names, installed whole, by its own folder's path,
/// when it serves `entry` and every other project of `registry`; none otherwise, and the start runs
/// its own build ([`refused`] when its own build is a release's).
pub fn to_start(machine: &Path, registry: &Registry, entry: &Entry) -> Option<(PathBuf, Build)> {
    let manifest = release::current_installed(machine)?;
    let build = Build::released(&manifest, &entry.root);
    build.unserved(entry, registry).is_none().then(|| {
        let server = release::binary(machine, &manifest.release, release::SERVER);
        (server, build)
    })
}

/// Why a start of the one server by the release `ours`'s own binary (what `just symphony` runs in
/// a project with no build of its own) starts nothing in `machine` when none runs and
/// [`to_start`] names no server: the release would serve the machine while something holds the
/// switch, refusing the project that holds it. One line, naming that project and the command that
/// starts the machine's server from its checkout.
pub fn refused(machine: &Path, registry: &Registry, ours: &str) -> String {
    let Some(manifest) = release::current_installed(machine) else {
        return format!(
            "the machine's Symphony is not started from the release {ours}: {} names no release \
             installed whole in {}, and a release's server is started only as the release it names",
            machine.join(release::CURRENT).display(),
            machine.join(release::RELEASES).display()
        );
    };
    let build = Build::released(&manifest, machine);
    match holding(&build, registry) {
        Some((id, root, why)) => format!(
            "the machine's Symphony is not started from the release {}, as the switch to it is \
             held: {why}; `just symphony` in {}, by {id}'s own build, starts the machine's server",
            manifest.release,
            root.display()
        ),
        // Nothing holds it, yet the release served no start: `current` moved between the reads.
        None => format!(
            "the machine's Symphony is not started from the release {}: {} moved as it was read; \
             `just symphony` again judges it anew",
            manifest.release,
            machine.join(release::CURRENT).display()
        ),
    }
}

/// The one server as the machine folder records it (`server.json`).
#[derive(serde::Deserialize)]
struct Record {
    port: u16,
    pid: i64,
    root: String,
    #[serde(default)]
    release: Option<String>,
}

/// Where the switch stands in the machine folder `machine`, as lines for `just release --status`,
/// each starting `machine: `: the release `current` names and the server the record names, then
/// what holds the switch, or what makes it. It reads the folder alone: no server is asked.
pub fn standing(machine: &Path) -> String {
    let Some(manifest) = release::current_installed(machine) else {
        if let Some(name) = release::current(machine) {
            return format!(
                "machine: {} names {name}, which is not installed whole in {}\n",
                machine.join(release::CURRENT).display(),
                machine.join(release::RELEASES).display()
            );
        }
        let installed = installed_releases(machine);
        if installed.is_empty() {
            return format!(
                "machine: no release is installed in {}: a project's build is the machine's \
                 server\n",
                machine.display()
            );
        }
        // Installed and never pointed at: what the refresh does while the switch is held.
        let mut said = format!(
            "machine: current names no release, and {} installed in {}: a project's build is the \
             machine's server\n",
            match installed.as_slice() {
                [one] => format!("{one} is"),
                many => format!("{} are", many.join(", ")),
            },
            machine.join(release::RELEASES).display()
        );
        if let Some(hold) = Registry::read(machine).ok().as_ref().and_then(held) {
            said.push_str(&format!("machine: the switch is held: {}\n", hold.said()));
        }
        return said;
    };
    let build = Build::released(&manifest, machine);
    let Manifest { release, .. } = manifest;
    let recorded: Option<Record> = std::fs::read_to_string(machine.join(super::command::RECORD))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let server = match &recorded {
        Some(record) if record.release.as_deref() == Some(release.as_str()) => format!(
            "the machine's server (port {}, pid {}) runs it",
            record.port, record.pid
        ),
        Some(Record {
            port,
            pid,
            release: Some(other),
            ..
        }) => format!(
            "the machine's server (port {port}, pid {pid}) runs the release {other} until its \
             reload follows current"
        ),
        Some(record) => format!(
            "the machine's server (port {}, pid {}) is the build of {}",
            record.port, record.pid, record.root
        ),
        None => "no server is recorded".to_string(),
    };
    let mut said = format!("machine: current names {release}; {server}\n");
    if recorded
        .as_ref()
        .is_some_and(|record| record.release.is_some())
    {
        return said;
    }
    let registry = match Registry::read(machine) {
        Ok(registry) => registry,
        Err(why) => {
            said.push_str(&format!("machine: the registry cannot be read: {why}\n"));
            return said;
        }
    };
    match holding(&build, &registry).map(|(_, _, why)| why) {
        Some(why) => said.push_str(&format!(
            "machine: the switch to {release} is held: {why}\n"
        )),
        None if recorded.is_some() => said.push_str(&format!(
            "machine: nothing holds the switch to {release}: `just symphony --stop`, then `just \
             symphony`, makes it the machine's server\n"
        )),
        None => said.push_str(&format!(
            "machine: nothing holds the switch to {release}: the next start of the one server runs \
             it\n"
        )),
    }
    said
}

/// The releases installed whole in `machine`, by name.
fn installed_releases(machine: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(machine.join(release::RELEASES)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = (entries.flatten())
        .filter_map(|entry| entry.file_name().into_string().ok())
        // An install's temporary folder is hidden: never a release until it is renamed.
        .filter(|name| !name.starts_with('.'))
        .filter(|name| matches!(release::installed(machine, name), Ok(Some(_))))
        .collect();
    symphony_board::order::sort_by(&mut names, &Ord::cmp);
    names
}

#[cfg(test)]
#[path = "switch_tests.rs"]
mod tests;
