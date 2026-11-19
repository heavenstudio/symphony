//! Delivery-workflow definitions as the board runs them (docs/WORKFLOW.md): the strict reader of
//! a project file's `[workflow]` and `[[workflow_mappings]]`, normalization, revision identity,
//! the host's prerequisites and capability grants, built-in definition v1, and the reader of the
//! project file the default branch commits, which is the only file a board activates from.
//!
//! Omitting `[workflow]` runs the board's existing writers unchanged. `symphony::project::workflow`
//! re-exports this module's items at their earlier paths.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;
use symphony_api::workflow::{Authority, Definition, FORMAT, Pair, Revision, Source};
use symphony_plugin::execution::{Principal, PrincipalKind};

pub mod activate;
pub mod admit;
mod builtin;
pub mod operate;
pub mod recover;
mod validate;
pub use crate::store::admission::{Receipt, Settled};
pub use crate::store::workflow::{
    Active, Followed, Moved, Recorded, Recovered, TaskPosition, Transition, Trigger,
};
pub use activate::Host;
pub use builtin::{builtin, legacy_pair};
pub use operate::Operating;

/// A project's file, at the top of the tree a commit holds.
const FILE: &str = "symphony.toml";

/// A validated, normalized definition. Deserialization alone never constructs this wrapper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validated(Definition);

impl Validated {
    pub fn new(definition: Definition) -> Result<Self, String> {
        validate::validate(&definition)?;
        Ok(Self(normalize(definition)))
    }

    pub fn definition(&self) -> &Definition {
        &self.0
    }

    /// Content identity includes format, labels/order and committed source metadata. The
    /// caller supplies metadata from its Git reader; this function does not attest provenance.
    pub fn revision(&self, source: Source) -> Result<Revision, String> {
        for (name, id) in [
            ("commit", &source.commit),
            ("project_blob", &source.project_blob),
        ] {
            if !object_id(id) {
                return Err(format!(
                    "workflow source {name}: expected a lowercase Git object ID"
                ));
            }
        }
        let bytes = canonical(&(FORMAT, &self.0, &source))?;
        Ok(Revision {
            format: FORMAT,
            digest: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            source,
        })
    }

    /// Refuse a stale or substituted revision, including a different source for equal content.
    pub fn check_revision(&self, revision: &Revision) -> Result<(), String> {
        if self.revision(revision.source.clone())? != *revision {
            return Err("stale_workflow: definition or source does not match revision".into());
        }
        Ok(())
    }

    /// Refuse mappings this definition cannot take: each names a pair the definition no longer
    /// declares (an ID keeps its meaning, so a pair still declared is never remapped) and a
    /// destination it does declare, and no pair is mapped twice.
    pub fn check_mappings(&self, mappings: &[Mapping]) -> Result<(), String> {
        let mut declared: BTreeSet<&Pair> = BTreeSet::new();
        for position in &self.0.positions {
            declared.insert(&position.at);
        }
        let mut from = BTreeSet::new();
        for mapping in mappings {
            let (f, t) = (&mapping.from, &mapping.to);
            if declared.contains(f) {
                return Err(format!(
                    "workflow_mappings: {}/{} is still declared; an ID keeps its meaning, so \
                     only a removed pair is mapped",
                    f.phase, f.state
                ));
            }
            if !declared.contains(t) {
                return Err(format!(
                    "workflow_mappings: {}/{} -> {}/{} names a destination the workflow does \
                     not declare",
                    f.phase, f.state, t.phase, t.state
                ));
            }
            if !from.insert(f) {
                return Err(format!(
                    "workflow_mappings: {}/{} is mapped twice",
                    f.phase, f.state
                ));
            }
        }
        Ok(())
    }
}

/// Where a task at a pair the newly active definition no longer declares goes when it activates:
/// its position, and the resume pair a blocked task returns to.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub from: Pair,
    pub to: Pair,
}

/// Parse only the candidate `[workflow]` in a project file; other project tables belong to
/// `Project::parse`. Omission selects built-in v1. Unknown workflow fields/variants refuse.
pub fn candidate(text: &str) -> Result<Validated, String> {
    let workflow = taken(&mut table(text)?, "workflow")?;
    Validated::new(workflow.unwrap_or_else(builtin))
}

/// The file's `[workflow]`, validated, or none when it has none; and its `[[workflow_mappings]]`,
/// checked against that workflow. Mappings without a workflow are refused: they map onto nothing.
pub fn written(text: &str) -> Result<(Option<Validated>, Vec<Mapping>), String> {
    let mut file = table(text)?;
    let definition: Option<Definition> = taken(&mut file, "workflow")?;
    let mappings: Vec<Mapping> = taken(&mut file, "workflow_mappings")?.unwrap_or_default();
    let Some(definition) = definition else {
        if !mappings.is_empty() {
            return Err("workflow_mappings: no [workflow] to map onto".into());
        }
        return Ok((None, Vec::new()));
    };
    let validated = Validated::new(definition)?;
    validated.check_mappings(&mappings)?;
    Ok((Some(validated), mappings))
}

/// The project file as one TOML table, as [`crate::limits`] reads it: a syntax error is named at
/// its line.
fn table(text: &str) -> Result<toml::Table, String> {
    toml::from_str(text).map_err(|e| format!("workflow: {e}"))
}

/// `file`'s table `key`, taken out of it and read as the board reads the same types from its own
/// rows (`serde_json`), so every program that links the workflow links its readers once and none
/// for TOML; none where the file has no `key`. Its refusal names what is wrong, without TOML's
/// line, and refuses all that a reading straight from TOML refuses: a datetime is a table to it,
/// which no field of the workflow takes.
fn taken<T: serde::de::DeserializeOwned>(
    file: &mut toml::Table,
    key: &str,
) -> Result<Option<T>, String> {
    fn json(value: toml::Value) -> serde_json::Value {
        match value {
            toml::Value::String(text) => text.into(),
            toml::Value::Integer(number) => number.into(),
            toml::Value::Float(number) => (serde_json::Number::from_f64(number))
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
            toml::Value::Boolean(flag) => flag.into(),
            toml::Value::Datetime(at) => serde_json::json!({ "datetime": at.to_string() }),
            toml::Value::Array(values) => values.into_iter().map(json).collect(),
            toml::Value::Table(table) => (table.into_iter())
                .map(|(key, value)| (key, json(value)))
                .collect(),
        }
    }
    (file.remove(key))
        .map(|value| serde_json::from_value(json(value)).map_err(|e| format!("{key}: {e}")))
        .transpose()
}

/// The workflow a commit's project file sets: a custom definition with its revision and
/// mappings, or none for a file with no `[workflow]` (the board's existing writers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    pub commit: String,
    pub custom: Option<Custom>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Custom {
    pub definition: Validated,
    pub revision: Revision,
    pub mappings: Vec<Mapping>,
}

/// The project file the tip of `branch` commits in the repository whose main checkout is `main`,
/// read from the commit's tree, never from the index or a working tree. A missing file is a
/// project with no workflow; a file that is a link, a folder or not UTF-8 refuses the read, as
/// does a branch that names no commit.
pub fn committed(main: &Path, branch: &str) -> Result<Committed, String> {
    let commit = tip(main, branch)
        .ok_or_else(|| format!("workflow committed on {branch}: the branch names no commit"))?;
    committed_at(main, branch, commit)
}

/// The commit at the tip of `branch` in the repository whose main checkout is `main`, or none.
fn tip(main: &Path, branch: &str) -> Option<String> {
    let at = format!("refs/heads/{branch}^{{commit}}");
    git(main, &["rev-parse", "--verify", "--quiet", &at])
        .ok()
        .map(|commit| commit.trim().to_string())
}

/// [`committed`] at `commit`, the tip of `branch`, which its refusals name.
fn committed_at(main: &Path, branch: &str, commit: String) -> Result<Committed, String> {
    let refuse = |what: String| format!("workflow committed on {branch}: {what}");
    let listed = git(main, &["ls-tree", "-z", &commit, "--", FILE]).map_err(refuse)?;
    let Some(entry) = listed.split('\0').find(|line| !line.is_empty()) else {
        return Ok(Committed {
            commit,
            custom: None,
        });
    };
    // `<mode> <type> <object>\t<path>`
    let (meta, _) = entry
        .split_once('\t')
        .ok_or_else(|| refuse(format!("unreadable tree entry {entry:?}")))?;
    let fields: Vec<&str> = meta.split(' ').collect();
    let [mode, kind, blob] = fields[..] else {
        return Err(refuse(format!("unreadable tree entry {entry:?}")));
    };
    if kind != "blob" || !matches!(mode, "100644" | "100755") {
        return Err(refuse(format!(
            "{FILE} must be a regular file, not mode {mode} {kind}"
        )));
    }
    let text = git(main, &["cat-file", "blob", blob]).map_err(refuse)?;
    let (definition, mappings) = written(&text).map_err(refuse)?;
    let custom = match definition {
        None => None,
        Some(definition) => {
            let revision = definition
                .revision(Source {
                    commit: commit.clone(),
                    project_blob: blob.to_string(),
                })
                .map_err(refuse)?;
            Some(Custom {
                definition,
                revision,
                mappings,
            })
        }
    };
    Ok(Committed { commit, custom })
}

/// The workflow the project's default branch commits, followed on its board (automatic
/// activation B, [`crate::Store::follow_committed`]) under the local host's policy
/// ([`Host::local`]). The project is the one whose main checkout is `main`: its default branch
/// is named in the file there, and the workflow is read from that branch's tip alone.
///
/// A repository with no such branch commits no workflow: on a board that never activated one it
/// changes nothing and says nothing, as a project file the default branch cannot be read from
/// (`main` by default) does. Once a workflow is active, a missing branch is said.
pub fn follow(store: &crate::Store, main: &Path) -> Result<Followed, String> {
    let text = std::fs::read_to_string(main.join(FILE)).unwrap_or_default();
    let branch = crate::land::Settings::parse(&text)
        .map(|settings| settings.default_branch)
        .unwrap_or_else(|_| "main".into());
    let committed = match tip(main, &branch) {
        Some(commit) => committed_at(main, &branch, commit)?,
        None => {
            if store
                .workflow_active()
                .map_err(|e| e.to_string())?
                .is_none()
            {
                return Ok(Followed::Legacy);
            }
            return Err(format!(
                "workflow committed on {branch}: the branch names no commit"
            ));
        }
    };
    store
        .follow_committed(&committed, &Host::local())
        .map_err(|e| e.to_string())
}

/// `git -C main ARGS`'s output, as text; its error output when it fails, or text that is not
/// UTF-8.
fn git(main: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::sys::output(crate::sys::git().arg("-C").arg(main).args(args))
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8(out.stdout).map_err(|_| "the file is not UTF-8".to_string())
}

fn object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64)
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Host support is checked separately from syntax. Declaring a capability never supplies it.
/// These are adapter names, not membership or authorization decisions.
pub fn prerequisites(
    definition: &Validated,
    capabilities: &[&str],
    adapters: &[&str],
) -> Result<(), String> {
    for capability in &definition.0.capabilities {
        if !capabilities.contains(&capability.as_str()) {
            return Err(format!("unsupported capability: {capability}"));
        }
    }
    for adapter in &definition.0.adapters {
        if !adapters.contains(&adapter.as_str()) {
            return Err(format!("unsupported adapter: {adapter}"));
        }
    }
    Ok(())
}

/// The capabilities a host grants, each to the principals it names: the host's own policy, never
/// a definition's. A capability a definition declares and no grant names is unsupported, and a
/// principal holds only what a grant gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants(Vec<(String, Principal)>);

impl Grants {
    /// No capability granted to anyone.
    pub fn none() -> Grants {
        Grants::default()
    }

    /// These grants, and `capability` to `principal` as well.
    #[must_use]
    pub fn grant(mut self, capability: &str, principal: Principal) -> Grants {
        if !self.holds(&principal, capability) {
            self.0.push((capability.to_string(), principal));
        }
        self
    }

    /// Whether `principal` holds `capability`: a grant names both.
    pub fn holds(&self, principal: &Principal, capability: &str) -> bool {
        self.0
            .iter()
            .any(|(granted, to)| granted == capability && to == principal)
    }

    /// The capabilities granted to anyone: the ones the host supports.
    pub fn supported(&self) -> BTreeSet<&str> {
        self.0
            .iter()
            .map(|(capability, _)| capability.as_str())
            .collect()
    }
}

/// The local owner, as the local host authenticates it: the person at this machine's board.
pub fn local_owner() -> Principal {
    Principal::new(PrincipalKind::Human, "local".into(), "owner".into())
}

/// The board's own service `name` (its activation, its recovery), acting for no person.
pub fn board_service(name: &str) -> Principal {
    Principal::new(PrincipalKind::Service, "board".into(), name.into())
}

/// An agent session, by the handle the board bound it to.
pub fn agent_session(handle: &str) -> Principal {
    Principal::new(PrincipalKind::Agent, "local".into(), handle.into())
}

fn normalize(mut d: Definition) -> Definition {
    ordered(&mut d.phases, |phase| phase.id.clone());
    ordered(&mut d.states, |state| state.id.clone());
    ordered(&mut d.positions, |position| pair_key(&position.at));
    ordered(&mut d.edges, |edge| edge.id.clone());
    ordered(&mut d.guards, |guard| guard.id.clone());
    d.capabilities.sort();
    d.adapters.sort();
    d.task_kinds.sort();
    for edge in &mut d.edges {
        edge.inputs.sort();
        edge.guards.sort();
        // Authority is an unordered OR; effects remain in application order.
        ordered(&mut edge.authority, authority_key);
    }
    d
}

/// The JSON text a digest is taken of: `value` as compact JSON, written by way of a JSON value,
/// which writes the same bytes as writing it straight, so a type the board already turns into a
/// value links no writer of its own for its digest.
pub(crate) fn canonical(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    Ok(value.to_string().into_bytes())
}

/// `items` in the order of their `key`, those of one key in the order they came: one sort on
/// `(String, usize)` for every list the workflow orders, where a sort of each element type would
/// link a sort each.
pub(crate) fn ordered<T>(items: &mut Vec<T>, key: impl Fn(&T) -> String) {
    let mut keyed: Vec<(String, usize)> = items.iter().map(key).zip(0..).collect();
    keyed.sort_unstable();
    let mut slots: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend((keyed.into_iter()).map(|(_, at)| slots[at].take().expect("each index once")));
}

/// An authority as a key that orders as the authority does: its kind in the order the kinds are
/// declared, then a task role in the order the roles are, or the capability.
fn authority_key(authority: &Authority) -> String {
    match authority {
        Authority::TaskRole { role } => format!("0\0{}", *role as u8),
        Authority::ProjectCapability { capability } => format!("1\0{capability}"),
        Authority::SystemCapability { capability } => format!("2\0{capability}"),
    }
}

/// A pair as a key that orders as the pair does, phase then state: no ID holds a NUL.
pub(crate) fn pair_key(pair: &Pair) -> String {
    format!("{}\0{}", pair.phase, pair.state)
}

/// Each mapping's source pair to its destination, built by insertion: a collected map sorts what
/// it collects, a sort of its own for each key type.
pub(crate) fn mapped(mappings: &[Mapping]) -> std::collections::BTreeMap<&Pair, &Pair> {
    let mut map = std::collections::BTreeMap::new();
    for mapping in mappings {
        map.insert(&mapping.from, &mapping.to);
    }
    map
}

#[cfg(test)]
pub(crate) mod tests;
