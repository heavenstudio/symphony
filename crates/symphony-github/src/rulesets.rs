//! `symphony-github rulesets [--verify] [--branch BRANCH]`: the rulesets in effect on the default
//! branch read in full, their bypass lists among them, with the gh CLI this machine is signed in
//! to. GitHub shows a ruleset's bypass list only to who may write it: someone who administers the
//! repository, which the App a delivery runs as must never be ([`crate::preflight`]).
//!
//! It says each ruleset, who bypasses it, and whether the policy holds a delivery to its evidence
//! ([`crate::preflight::judge_rules`]). With `--verify`, where it holds, it keeps each ruleset in
//! the state as [`Verified`]: who bypasses it as of the instant GitHub says it last changed. A
//! delivery takes that list while the ruleset's `updated_at` is still that instant, and refuses
//! once it is not, whatever changed. It is run by a person, at setup and after a ruleset changes,
//! never by a land.
//!
//! It reads and never writes to GitHub: gh is run as `gh api PATH`, a GET, so the token of who is
//! signed in stays gh's and is never handled here. Nothing is kept where the policy does not hold,
//! or a ruleset was not read whole; a record kept before stays until a verification replaces it.

use crate::config::Repository;
use crate::deliver::{self, PUBLISHES, Serving};
use crate::preflight::{self, Bypass, MAX_PAGES, Operation, PER_PAGE, Wanted};
use crate::reconcile::{Context, Outcome};
use crate::state::{Binding, State, Store, Verified};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;

/// One ruleset as gh read it.
struct Read {
    id: i64,
    name: String,
    enforcement: String,
    updated_at: String,
    bypass: Bypass,
}

/// What gh answers `GET path`, as JSON, or why there is none.
fn gh(program: &str, path: &str) -> Result<Value, String> {
    let ran = Command::new(program)
        .args(["api", "-H", "Accept: application/vnd.github+json", path])
        .output()
        .map_err(|error| format!("{program} is not run: {error}"))?;
    if !ran.status.success() {
        let said = String::from_utf8_lossy(&ran.stderr);
        let said: String = (said.lines().next().unwrap_or_default().chars())
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        return Err(format!("gh api {path}: {said}"));
    }
    serde_json::from_slice(&ran.stdout).map_err(|error| format!("gh api {path}: {error}"))
}

/// The rules in effect for `base`, every page.
fn rules(program: &str, repository: &Repository, base: &str) -> Result<Vec<Value>, String> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let path = format!(
            "repos/{}/rules/branches/{base}?per_page={PER_PAGE}&page={page}",
            repository.full_name()
        );
        let listed = gh(program, &path)?;
        let listed = listed
            .as_array()
            .ok_or_else(|| format!("gh api {path}: not a list of rules"))?;
        all.extend(listed.iter().cloned());
        if listed.len() < PER_PAGE {
            return Ok(all);
        }
    }
    Err(format!(
        "{base} has more rules than {MAX_PAGES} pages list: they are not judged in part"
    ))
}

/// Ruleset `id` as gh reads it: whose bypass list it shows only to who administers it.
fn ruleset(program: &str, repository: &Repository, id: i64) -> Result<Read, String> {
    let value = gh(
        program,
        &format!("repos/{}/rulesets/{id}", repository.full_name()),
    )?;
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    let bypass = match value["bypass_actors"].as_array() {
        _ if value["id"] != json!(id) => Bypass::Unknown("it does not read as itself".into()),
        _ if value["enforcement"] != json!("active") => {
            Bypass::Unknown("it does not read as active".into())
        }
        _ if preflight::instant(&text("updated_at")).is_none() => {
            Bypass::Unknown("when it last changed does not read".into())
        }
        None => Bypass::Unknown(
            "its bypass list is not shown: gh is not signed in as someone who administers the \
             repository"
                .into(),
        ),
        Some(listed) => Bypass::Actors(
            listed
                .iter()
                .map(|actor| {
                    (
                        actor["actor_type"].as_str().unwrap_or("unknown").into(),
                        actor["actor_id"].as_i64(),
                        actor["bypass_mode"].as_str().unwrap_or("unknown").into(),
                    )
                })
                .collect(),
        ),
    };
    Ok(Read {
        id,
        name: text("name"),
        enforcement: text("enforcement"),
        updated_at: text("updated_at"),
        bypass,
    })
}

/// A ruleset as a person reads it, its remote text kept to plain characters.
fn said(read: &Read) -> String {
    let plain = |text: &str| -> String {
        (text.chars())
            .filter(|c| c.is_ascii_alphanumeric() || " -_:.+".contains(*c))
            .take(60)
            .collect()
    };
    let who = match &read.bypass {
        Bypass::Actors(actors) if actors.is_empty() => "bypassed by nobody".to_string(),
        Bypass::Actors(actors) => format!(
            "bypassed by {}",
            (actors.iter().map(preflight::actor))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Bypass::Unknown(why) => format!("who bypasses it is not known ({why})"),
    };
    format!(
        "ruleset {} {:?} ({}), last changed {}: {who}",
        read.id,
        plain(&read.name),
        plain(&read.enforcement),
        plain(&read.updated_at)
    )
}

/// The repository, its rules on a branch and each ruleset they come from, read whole with gh.
struct Whole {
    repository: Value,
    id: i64,
    rules: Vec<Value>,
    reads: Vec<Read>,
}

/// What gh reads of `repository` and its rules on `base`, held to the repository `bound` names
/// where the project is bound to one, or why it was not read whole.
fn whole(
    program: &str,
    repository: &Repository,
    base: &str,
    bound: Option<&Binding>,
) -> Result<Whole, String> {
    let full = repository.full_name();
    let read = gh(program, &format!("repos/{full}"))?;
    let id = (read["id"].as_i64())
        .ok_or_else(|| format!("gh api repos/{full}: names no repository number"))?;
    if let Some(binding) = bound.filter(|binding| binding.repository_id != id) {
        return Err(format!(
            "{full} is repository {id} on GitHub, and this project is bound to {} ({})",
            binding.repository_id, binding.full_name
        ));
    }
    let rules = rules(program, repository, base)?;
    let mut ids: Vec<i64> = (rules.iter())
        .filter_map(|rule| rule["ruleset_id"].as_i64())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let reads = (ids.into_iter())
        .map(|ruleset_id| ruleset(program, repository, ruleset_id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Whole {
        repository: read,
        id,
        rules,
        reads,
    })
}

/// Each of `reads` whose bypass list was read, kept in the state in `folder` as verified of
/// repository `id` at `now`, in place of what was kept before: how many.
fn keep(folder: &std::path::Path, reads: Vec<Read>, id: i64, now: i64) -> Result<usize, String> {
    let verified: BTreeMap<i64, Verified> = reads
        .into_iter()
        .filter_map(|read| match read.bypass {
            Bypass::Actors(bypass) => Some((
                read.id,
                Verified {
                    repository: id,
                    updated_at: read.updated_at,
                    bypass,
                    at: now,
                },
            )),
            Bypass::Unknown(_) => None,
        })
        .collect();
    let store = Store::open(folder)?;
    let _lock = store.lock()?;
    let mut state: State = store.state()?.unwrap_or_default();
    let count = verified.len();
    state.rulesets = verified;
    store.save(&state)?;
    Ok(count)
}

/// `rulesets`: what gh reads of the rulesets in effect on `base`, judged, and with `verify` kept
/// where the policy holds. Exits 0 where it holds, 1 where it does not or was not read whole.
///
/// # Errors
///
/// The state folder cannot be read or written.
pub fn run(ctx: &Context<'_>, base: &str, verify: bool, out: &mut dyn Write) -> Outcome {
    let Serving {
        config, repository, ..
    } = match deliver::served(ctx, PUBLISHES, deliver::publishes) {
        Ok(serving) => serving,
        Err(why) => {
            let _ = writeln!(out, "{why}");
            return Ok(1);
        }
    };
    // Settings that publish and name no App do not read, so this is never said.
    let Some(app) = config.app_id else {
        let _ = writeln!(out, "turned off: the committed settings name no app_id");
        return Ok(1);
    };
    let folder = ctx.board.state_folder().map_err(|e| e.said().to_string())?;
    let bound = match Store::existing(&folder) {
        Some(store) => store.state()?.and_then(|state| state.binding),
        None => None,
    };
    let Whole {
        repository: read,
        id,
        rules,
        reads,
    } = match whole(ctx.gh, &repository, base, bound.as_ref()) {
        Ok(whole) => whole,
        Err(why) => {
            let _ = writeln!(out, "not read whole, nothing kept: {why}");
            return Ok(1);
        }
    };
    let full = repository.full_name();
    let _ = writeln!(
        out,
        "{full} (id {id}), {base}: {} ruleset(s) in effect, read with gh",
        reads.len()
    );
    for read in &reads {
        let _ = writeln!(out, "  {}", said(read));
    }
    let bypasses: BTreeMap<i64, Bypass> = (reads.iter())
        .map(|read| (read.id, read.bypass.clone()))
        .collect();
    let wanted = Wanted {
        repository: id,
        base,
        app,
        operation: Operation::of(config.upstream_id.is_some()),
    };
    let missing = preflight::judge_rules(&read, &rules, &bypasses, &wanted);
    if !missing.is_empty() {
        let _ = writeln!(
            out,
            "does not hold: a delivery would be refused{}",
            if verify { "; nothing kept" } else { "" }
        );
        for line in &missing {
            let _ = writeln!(out, "  missing: {line}");
        }
        return Ok(1);
    }
    if !verify {
        let _ = writeln!(
            out,
            "holds; `--verify` keeps it, which a delivery takes while each ruleset is unchanged"
        );
        return Ok(0);
    }
    let count = keep(&folder, reads, id, ctx.now)?;
    let _ = writeln!(
        out,
        "holds, and verified: {count} ruleset(s) kept, which a delivery takes until one changes"
    );
    Ok(0)
}
