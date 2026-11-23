//! Delivery-workflow definitions as the board runs them (docs/workflow/WORKFLOW.md): the strict reader of
//! a project file's `[workflow]`, `[[workflow_mappings]]` and `[[workflow_authorities]]`,
//! normalization, revision identity,
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
pub mod effect;
pub mod guard;
pub mod operate;
pub mod recover;
mod validate;
pub use crate::store::admission::Admission;
pub use crate::store::admission::{Finalized, Receipt, Reviewed, Settled};
pub use crate::store::guard::{Authorities, Observing};
pub use crate::store::workflow::{
    Active, Followed, Moved, Recorded, Recovered, TaskPosition, Transition, Trigger,
};
pub use activate::Host;
pub use builtin::{DEFAULT, builtin, legacy_pair};
pub use guard::Attester;
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
    let workflow = taken(table(text)?.remove("workflow"), "workflow")?;
    Validated::new(workflow.unwrap_or_else(builtin))
}

/// The file's `[workflow]`, validated, or none when it has none; and its `[[workflow_mappings]]`,
/// checked against that workflow. Mappings without a workflow map onto built-in v1, the board's
/// own writers: where each task goes when a board that activated a workflow returns to them
/// ([`crate::Store::return_to_default`]).
pub fn written(text: &str) -> Result<(Option<Validated>, Vec<Mapping>), String> {
    let mut file = table(text)?;
    written_from(file.remove("workflow"), file.remove("workflow_mappings"))
}

/// [`written`] of a file already read as TOML: the values of its `workflow` and
/// `workflow_mappings`, none where it has none, so a reader of the whole file reads it once.
pub fn written_from(
    workflow: Option<toml::Value>,
    mappings: Option<toml::Value>,
) -> Result<(Option<Validated>, Vec<Mapping>), String> {
    let definition: Option<Definition> = taken(workflow, "workflow")?;
    let mappings: Vec<Mapping> = taken(mappings, "workflow_mappings")?.unwrap_or_default();
    let Some(definition) = definition else {
        if !mappings.is_empty() {
            Validated::new(builtin())?.check_mappings(&mappings)?;
        }
        return Ok((None, mappings));
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

/// `value`, a file's table `key`, read as the board reads the same types from its own rows
/// (`serde_json`), so every program that links the workflow links its readers once and none for
/// TOML; none where the file has no `key`. Its refusal names what is wrong, without TOML's line,
/// and refuses all that a reading straight from TOML refuses: a datetime is a table to it, which
/// no field of the workflow takes.
fn taken<T: serde::de::DeserializeOwned>(
    value: Option<toml::Value>,
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
    value
        .map(|value| crate::objects::read(&json(value)).map_err(|e| format!("{key}: {e}")))
        .transpose()
}

/// The file's `[[workflow_authorities]]`, sorted by adapter and checked against `definition`
/// ([`guard::check_attesters`]); refused where there is no `[workflow]` to guard, or no
/// `[project] id` to bind an attestation to ([`attested_project`]).
pub fn authorities(text: &str, definition: Option<&Validated>) -> Result<Vec<Attester>, String> {
    let mut file = table(text)?;
    let project = project_id(&file);
    authorities_from(
        file.remove("workflow_authorities"),
        project.as_deref(),
        definition,
    )
}

/// [`authorities`] of a file already read as TOML: the value of its `workflow_authorities` and its
/// `[project] id`, none where it has none or one the project file's rule refuses
/// ([`crate::project::id`]), so a reader of the whole file reads it once, as [`written_from`] does.
/// A blank id given is none.
pub fn authorities_from(
    value: Option<toml::Value>,
    project: Option<&str>,
    definition: Option<&Validated>,
) -> Result<Vec<Attester>, String> {
    let written: Vec<WrittenAuthority> = taken(value, "workflow_authorities")?.unwrap_or_default();
    for authority in &written {
        authority.check_page()?;
    }
    let mut attesters: Vec<Attester> = (written.into_iter())
        .map(|authority| Attester {
            adapter: authority.adapter,
            issuer: authority.issuer,
            contract: authority.contract,
            key: authority.key,
        })
        .collect();
    let Some(definition) = definition else {
        if !attesters.is_empty() {
            return Err("workflow_authorities: no [workflow] to guard".into());
        }
        return Ok(attesters);
    };
    if !attesters.is_empty() && project.is_none_or(|id| id.trim().is_empty()) {
        return Err(
            "workflow_authorities: an attestation names the project it is for, and [project] \
             id names none (it is unwritten, blank, or no slug)"
                .into(),
        );
    }
    guard::check_attesters(definition, &attesters)?;
    // Each adapter is named once (`check_attesters`), so its name orders them.
    ordered(&mut attesters, |attester| attester.adapter.clone());
    Ok(attesters)
}

/// An authority as the file writes it: what the board records of it ([`Attester`]), and `page`,
/// where its ledger shows a guard of a task, which a task's page links and the board never
/// records or decides on.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenAuthority {
    adapter: String,
    issuer: String,
    contract: u32,
    key: String,
    #[serde(default)]
    page: Option<String>,
}

/// What a `page` names the task by, and the guard: each filled in for the link a page draws.
const PAGE_TASK: &str = "{task}";
const PAGE_GUARD: &str = "{guard}";

impl WrittenAuthority {
    /// Refused unless `page`, filled in, is an address a page may link
    /// ([`symphony_api::linkable`]): a path of the app or a web address.
    fn check_page(&self) -> Result<(), String> {
        let Some(page) = &self.page else {
            return Ok(());
        };
        if symphony_api::linkable(&page_for(page, "area/task", "guard")) {
            return Ok(());
        }
        Err(format!(
            "workflow_authorities: {}'s page is neither a path of the app nor a web address: \
             {page}",
            self.adapter
        ))
    }
}

/// `page`, an authority's as the file writes it, for `guard` of `task`: each `{task}` and
/// `{guard}` in it filled in.
pub fn page_for(page: &str, task: &str, guard: &str) -> String {
    page.replace(PAGE_TASK, task).replace(PAGE_GUARD, guard)
}

/// The page each authority of the project file `text` names, by its adapter: none for a file
/// that does not read, or an authority that names none. It is read for a link alone, so what the
/// file's own reading refuses ([`authorities_from`]) is no page here.
pub fn authority_pages(text: &str) -> std::collections::BTreeMap<String, String> {
    let written = table(text)
        .ok()
        .and_then(|mut file| file.remove("workflow_authorities"))
        .and_then(|value| taken::<Vec<WrittenAuthority>>(Some(value), "workflow_authorities").ok())
        .flatten()
        .unwrap_or_default();
    (written.into_iter())
        .filter(|authority| authority.check_page().is_ok())
        .filter_map(|authority| Some((authority.adapter, authority.page?)))
        .collect()
}

/// The `[project] id` the file commits, which every attestation names as the project it is for:
/// the committed id alone, never one a machine's registry or folder gives, so a project's
/// attestation reads the same on every board of it and on no other project's.
pub fn attested_project(text: &str) -> Option<String> {
    project_id(&table(text).ok()?)
}

/// `file`'s `[project] id`, where it is text the project file's rule takes ([`crate::project::id`]):
/// one it refuses (blank, or no slug) is none, as the board's lookup of the project reads it.
fn project_id(file: &toml::Table) -> Option<String> {
    let id = file.get("project")?.get("id")?.as_str()?;
    crate::project::id(id).ok().map(str::to_string)
}

/// The workflow a commit's project file sets: a custom definition with its revision, mappings
/// and authorities, or none for a file with no `[workflow]` (the board's existing writers), and
/// then the mappings onto built-in v1 the file keeps for a return to them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    pub commit: String,
    pub custom: Option<Custom>,
    /// The file's mappings where it has no `[workflow]`; empty beside a custom definition, whose
    /// own they are.
    pub returning: Vec<Mapping>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Custom {
    pub definition: Validated,
    pub revision: Revision,
    pub mappings: Vec<Mapping>,
    /// The approval authorities its guards are verified by, sorted by adapter: the public keys
    /// the default branch commits ([`guard::Attester`]).
    pub authorities: Vec<Attester>,
    /// The project an attestation is for ([`attested_project`]): none where no authority is
    /// committed.
    pub project: Option<String>,
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

/// [`committed`] at `commit`, the tip of `branch`, which its refusals name: the commit's top tree,
/// for the file's mode, and the file, in one `git cat-file --batch`.
fn committed_at(main: &Path, branch: &str, commit: String) -> Result<Committed, String> {
    let refuse = |what: String| format!("workflow committed on {branch}: {what}");
    let specs = [format!("{commit}^{{tree}}"), format!("{commit}:{FILE}")];
    let mut read = crate::sweep::git::objects(main, &specs).map_err(refuse)?;
    let (Some(file), Some(Some(tree))) = (read.pop(), read.pop()) else {
        return Err(refuse(format!("commit {commit} has no tree")));
    };
    let Some((mode, blob)) = entry(&tree, FILE) else {
        return Ok(Committed {
            commit,
            custom: None,
            returning: Vec::new(),
        });
    };
    if !matches!(mode.as_str(), "100644" | "100755") {
        let kind = match mode.as_str() {
            "40000" => "tree",
            "160000" => "commit",
            _ => "blob",
        };
        return Err(refuse(format!(
            "{FILE} must be a regular file, not mode {mode:0>6} {kind}"
        )));
    }
    let file = file
        .filter(|file| file.id == blob)
        .ok_or_else(|| refuse(format!("{FILE} is not readable at {commit}")))?;
    let text = String::from_utf8(file.bytes).map_err(|_| refuse("the file is not UTF-8".into()))?;
    // One reading of the file, as the Symphony's own.
    let mut file = table(&text).map_err(refuse)?;
    let project = project_id(&file);
    let (definition, mappings) =
        written_from(file.remove("workflow"), file.remove("workflow_mappings")).map_err(refuse)?;
    let authorities = authorities_from(
        file.remove("workflow_authorities"),
        project.as_deref(),
        definition.as_ref(),
    )
    .map_err(refuse)?;
    let project = project.filter(|_| !authorities.is_empty());
    let Some(definition) = definition else {
        return Ok(Committed {
            commit,
            custom: None,
            returning: mappings,
        });
    };
    let revision = definition
        .revision(Source {
            commit: commit.clone(),
            project_blob: blob.to_string(),
        })
        .map_err(refuse)?;
    Ok(Committed {
        commit,
        custom: Some(Custom {
            definition,
            revision,
            mappings,
            authorities,
            project,
        }),
        returning: Vec::new(),
    })
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

/// The mode and object id of the entry `name` in `tree`, a tree object as git stores it: each
/// entry `<mode> <name>\0` and its id's bytes, as many as its own id's hex digits halved.
fn entry(tree: &crate::sweep::git::Object, name: &str) -> Option<(String, String)> {
    let width = tree.id.len() / 2;
    let mut rest = tree.bytes.as_slice();
    while let Some(space) = rest.iter().position(|b| *b == b' ') {
        let end = space + rest[space..].iter().position(|b| *b == 0)?;
        let id = rest.get(end + 1..end + 1 + width)?;
        if &rest[space + 1..end] == name.as_bytes() {
            let hex = id.iter().map(|byte| format!("{byte:02x}")).collect();
            return Some((String::from_utf8_lossy(&rest[..space]).into_owned(), hex));
        }
        rest = &rest[end + 1 + width..];
    }
    None
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
