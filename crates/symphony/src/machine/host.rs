//! The host rule: which build is the machine's one server. The one server compiles every registered
//! project's plugins. A project with no plugins never replaces a running host and is served by any
//! host holding its pin. A project whose plugins the running host lacks is registered and waits for
//! its own `just symphony`, which replaces the host by the socket hand-off, every project kept
//! registered. A host serves a project registered at its pin or an older one, and refuses one whose
//! pin or board schema is newer than its own, naming the project that must bump.

use std::path::Path;

use super::{Entry, Registry};

/// What a build of the server holds that a project needs: its plugins, its pin and its board
/// schema, and the project that built it, which a refusal names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub plugins: Vec<String>,
    pub pin: u32,
    pub schema: i64,
    /// The main checkout of the project that started it, when known.
    pub by: String,
}

impl Build {
    /// This process: the plugins its host installed, this build's pin and board schema.
    pub fn this(by: &Path) -> Build {
        Build {
            plugins: crate::plugins::compiled()
                .iter()
                .map(|plugin| plugin.name().to_string())
                .collect(),
            pin: super::PIN,
            schema: symphony_board::SCHEMA,
            by: by.display().to_string(),
        }
    }

    /// The running server, as its `/api/front` says.
    pub fn of(front: &symphony_api::Front) -> Build {
        Build {
            plugins: front.plugins.clone(),
            pin: front.pin,
            schema: front.schema,
            by: front.root.clone(),
        }
    }

    /// Why this build cannot serve `entry`, said after the build's name; none when it can.
    pub fn lacks(&self, entry: &Entry) -> Option<Lack> {
        let missing: Vec<String> = entry
            .plugins
            .iter()
            .filter(|name| !self.plugins.contains(name))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Some(Lack::Plugins(missing));
        }
        if entry.pin > self.pin {
            return Some(Lack::Pin(entry.pin));
        }
        (entry.schema > self.schema).then_some(Lack::Schema(entry.schema))
    }

    /// Whether this build is older than `other` in its pin or its board schema.
    fn older_than(&self, other: &Build) -> bool {
        self.pin < other.pin || self.schema < other.schema
    }
}

/// What a project needs that a build lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lack {
    /// The plugins it names that the build does not compile.
    Plugins(Vec<String>),
    /// The pin it was registered at, newer than the build's.
    Pin(u32),
    /// The board schema it was registered at, newer than the build's.
    Schema(i64),
}

impl Lack {
    /// The lack, said of `build` for the project `id`.
    pub fn said(&self, build: &Build, id: &str) -> String {
        match self {
            Lack::Plugins(names) => format!(
                "lacks {} {}, which {id} names",
                if names.len() == 1 {
                    "the plugin"
                } else {
                    "the plugins"
                },
                names.join(", ")
            ),
            Lack::Pin(pin) => format!("is at Symphony pin {}, older than {id}'s {pin}", build.pin),
            Lack::Schema(schema) => format!(
                "reads board schema {}, older than {id}'s {schema}",
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
    /// The running server lacks its plugins and this build cannot replace it: it is registered,
    /// and served once its own `just symphony` replaces the host. What to run, in one line.
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
    let projects: Vec<&Entry> = registry
        .projects
        .iter()
        .filter(|other| other.id != entry.id && other.root != entry.root)
        .chain([entry])
        .collect();
    let unserved = projects
        .iter()
        .find_map(|project| ours.lacks(project).map(|lack| (project, lack)));
    let replaces = !ours.plugins.is_empty() && unserved.is_none() && !ours.older_than(running);
    if replaces {
        return Verdict::Replace(format!(
            "the running Symphony {lacks}; this build serves every registered project and \
             replaces it"
        ));
    }
    let why = match unserved {
        _ if ours.plugins.is_empty() && !entry.plugins.is_empty() => {
            format!(
                "run `just symphony` in {}, whose build holds them",
                entry.root.display()
            )
        }
        _ if ours.older_than(running) => format!(
            "this build is older than the running Symphony: {} must bump its Symphony",
            entry.root.display()
        ),
        _ if ours.plugins.is_empty() => format!(
            "a project with no plugins never replaces the running Symphony: {} must bump its \
             Symphony, then run `just symphony` there",
            running.by
        ),
        Some((project, lack)) => format!(
            "this build cannot replace it: it {}; one host must compile every registered \
             project's plugins",
            lack.said(ours, &project.id)
        ),
        None => unreachable!("a build with plugins, not older, serving every project replaces"),
    };
    match lack {
        Lack::Plugins(_) => Verdict::Waits(format!("the running Symphony {lacks}: {why}")),
        Lack::Pin(_) | Lack::Schema(_) => {
            Verdict::Refuse(format!("the running Symphony {lacks}: {why}"))
        }
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
