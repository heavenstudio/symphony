//! The switch: when the machine's one server is the release installed on it. A release's server
//! compiles no plugin ([`super::host`]), so it serves every registered project only while none
//! names a plugin linked into the host: the first that does holds the machine on a project's build
//! ([`held`]), and `just release --status` ([`standing`]) and that project's pages say so. While
//! nothing holds it, a start of the one server that finds none running starts the server of the
//! release `current` names in place of its own build ([`to_start`]), so no project's build is the
//! machine's server; a project's build already running is switched by a stop and a start, an act
//! of its own, as no start replaces a running server with a release's. The refresh reads the same
//! rule before it points `current`.

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
    /// The hold, in one line: who holds it, by which plugins, and what lets it go.
    pub fn said(&self) -> String {
        let (plugins, them) = if self.plugins.len() == 1 {
            ("plugin", "it")
        } else {
            ("plugins", "each")
        };
        format!(
            "{} names the linked {plugins} {}, and a release's server serves a plugin only from its \
             process: a [plugins.process] command for {them} in {} lets the release serve every \
             project (docs/PLUGIN_PROTOCOL.md)",
            self.id,
            self.plugins.join(", "),
            self.root.join(crate::project::FILE).display()
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

/// The server a start of the one server for `entry` runs in `machine` when none runs, in place of
/// its own build: that of the release `current` names, installed whole, by its own folder's path,
/// when it serves `entry` and every other project of `registry`; none otherwise, and the start runs
/// its own build.
pub fn to_start(machine: &Path, registry: &Registry, entry: &Entry) -> Option<(PathBuf, Build)> {
    let manifest = release::current_installed(machine)?;
    let build = Build::released(&manifest, &entry.root);
    build.unserved(entry, registry).is_none().then(|| {
        let server = release::binary(machine, &manifest.release, release::SERVER);
        (server, build)
    })
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
        return match release::current(machine) {
            Some(name) => format!(
                "machine: {} names {name}, which is not installed whole in {}\n",
                machine.join(release::CURRENT).display(),
                machine.join(release::RELEASES).display()
            ),
            None => format!(
                "machine: no release is installed in {}: a project's build is the machine's \
                 server\n",
                machine.display()
            ),
        };
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
    // A linked plugin first, as the refresh reads it; then a project the release is too old for.
    let lacking = (registry.projects.iter()).find_map(|entry| {
        let lack = build.lacks(entry)?;
        Some(format!("its server {}", lack.said(&build, &entry.id)))
    });
    match held(&registry).map(|hold| hold.said()).or(lacking) {
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

#[cfg(test)]
#[path = "switch_tests.rs"]
mod tests;
