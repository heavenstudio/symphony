//! What a delivery reads of the repository's protection before it writes: the rules GitHub
//! applies to the base branch, and whether they hold a delivery to its evidence. It reads and
//! never writes, and it is asked before each consequential step ([`crate::deliver`]): before a
//! head with standing evidence is pushed, and before a merge is asked for.
//!
//! Three reads, the first two with the token that merges:
//!
//! 1. the repository (`GET /repos/O/N`): its number, default branch, merge settings and what the
//!    token may do there;
//! 2. the rules in effect for the base branch (`GET /repos/O/N/rules/branches/BASE`, every page),
//!    which GitHub evaluates from every active ruleset that applies, the organization's among
//!    them;
//! 3. each ruleset a rule comes from (`GET /repos/O/N/rulesets/ID`), for who may bypass it, with
//!    the policy token ([`Reader`]).
//!
//! GitHub shows a ruleset's bypass list only to who may write the ruleset, which the token that
//! merges must not be able to do, so the third read has a token of its own: a third GitHub
//! App's, which could change the rules it reads. A [`Reader`] is all the adapter does with that
//! token, and a `Reader` has one request, that read.
//!
//! It passes only what it read whole. A rule counts as holding everyone only when it comes from
//! a ruleset whose bypass list was read and is empty; a list GitHub does not show is not known
//! to be empty. What must hold:
//!
//! - **identity**: the repository is the one bound, by its number, and the base is its default
//!   branch; it is not archived, and it says the token does not administer it, which is not
//!   proof the token cannot edit a ruleset;
//! - **a pull request** is required, by a rule nobody bypasses;
//! - **the check** [`crate::deliver::ELIGIBLE`] is required from the one App that writes it, by
//!   a rule nobody bypasses, and not with the branch held up to date, which a delivery's one
//!   merge of the default branch does not meet;
//! - **no force push and no deletion**, by rules nobody bypasses;
//! - **updates are restricted** to the landing App: a rule restricts them, and its ruleset's one
//!   bypass actor is that App. Without it anyone who may write merges a pull request whose check
//!   is green, evidence withdrawn since or not;
//! - **the merge method**: what the repository and those rules permit is the operation's method
//!   and no other; no merge queue and no auto-merge, which this build does not deliver through.
//!
//! Classic branch protection is not read: it can add to what the rules require and never take
//! from it, and reading it asks for administration, which the token must not have.
//!
//! What no read here establishes is said in docs/GITHUB.md, "The protection a delivery needs":
//! which App a token is of, that no administrator edits a rule between this read and GitHub's
//! own act, and how GitHub applies two rulesets together.

use crate::config::Repository;
use crate::deliver::{ELIGIBLE, Host};
use crate::transport::Transport;
use serde_json::{Value, json};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

/// A page's size, the largest GitHub serves.
const PER_PAGE: usize = 100;
/// The pages of rules read before the listing is said to be more than is read.
const MAX_PAGES: usize = 10;
/// The merge methods GitHub has, as its rules name them.
const METHODS: [(&str, &str); 3] = [
    ("squash", "allow_squash_merge"),
    ("merge", "allow_merge_commit"),
    ("rebase", "allow_rebase_merge"),
];

/// What is delivered, which decides the merge method the base branch must permit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// A task's head, squashed into one commit.
    Task,
}

impl Operation {
    /// The methods the base branch may permit for it: the one it is merged by.
    #[must_use]
    pub fn permitted(self) -> &'static [&'static str] {
        match self {
            Operation::Task => &["squash"],
        }
    }
}

/// What a delivery is held to.
#[derive(Clone, Copy, Debug)]
pub struct Wanted<'a> {
    /// The repository's number, as bound.
    pub repository: i64,
    pub base: &'a str,
    /// The App whose checks count.
    pub checks_app: i64,
    /// The App that merges.
    pub landing_app: i64,
    pub operation: Operation,
}

/// Who may bypass a ruleset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bypass {
    /// Its list was read: each actor's type, number and mode.
    Actors(Vec<(String, Option<i64>, String)>),
    /// Its list was not read, and why.
    Unknown(String),
}

/// A policy that passed: a digest of everything read, which another policy does not share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub digest: String,
}

/// What reads who bypasses a ruleset, with the policy token: a token that could write the
/// repository's administration, so this type is everything the adapter does with it. It has one
/// request, `GET /repos/O/N/rulesets/ID`, and no other way to the transport it holds.
pub struct Reader<'a> {
    host: Host<'a>,
}

impl<'a> Reader<'a> {
    /// A reader of `repository`'s rulesets over `transport`, which carries the policy token.
    #[must_use]
    pub fn new(transport: &'a dyn Transport, repository: Repository) -> Reader<'a> {
        Reader {
            host: Host {
                transport,
                repository,
            },
        }
    }

    /// Who may bypass ruleset `id`, read from the ruleset itself.
    ///
    /// # Errors
    ///
    /// The answer that stops a delivery where GitHub gave none to judge.
    pub fn bypass(&self, id: i64) -> Result<Bypass, Value> {
        let (status, value) = self.host.get(&format!("/rulesets/{id}"))?;
        if status != 200 {
            return Ok(Bypass::Unknown(format!(
                "GitHub answered {status} to the policy token"
            )));
        }
        if value["id"] != json!(id) || value["enforcement"] != json!("active") {
            return Ok(Bypass::Unknown("it does not read as active".into()));
        }
        let Some(listed) = value["bypass_actors"].as_array() else {
            return Ok(Bypass::Unknown(
                "its bypass list is not shown to the policy token".into(),
            ));
        };
        Ok(Bypass::Actors(
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
        ))
    }
}

/// The answer that refuses a delivery because the policy token is not there to read with:
/// `why`, as the token's file is refused.
pub(crate) fn unread(wanted: &Wanted<'_>, why: &str) -> Value {
    refused(
        wanted,
        &[format!(
            "who bypasses the rules of {} cannot be read, policy_token_file not being usable: \
             {why}",
            wanted.base
        )],
    )
}

/// The answer that refuses a delivery for `missing`.
fn refused(wanted: &Wanted<'_>, missing: &[String]) -> Value {
    json!({
        "state": "refused", "kind": "policy",
        "why": format!(
            "the protection of {} does not hold a delivery to its evidence ({} missing): {}. \
             Nothing was pushed or merged; a repository administrator puts it right (docs/GITHUB.md, \
             \"The protection a delivery needs\")",
            wanted.base,
            missing.len(),
            missing.join("; ")
        ),
        "missing": missing,
    })
}

/// The rules in effect for the base branch, every page, or what keeps them from being read.
fn rules(host: &Host<'_>, base: &str) -> Result<Result<Vec<Value>, String>, Value> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let (status, value) = host.get(&format!(
            "/rules/branches/{base}?per_page={PER_PAGE}&page={page}"
        ))?;
        let Some(listed) = value.as_array().filter(|_| status == 200) else {
            return Ok(Err(format!(
                "the rules of {base} are not readable with the merging token (GitHub answered \
                 {status})"
            )));
        };
        all.extend(listed.iter().cloned());
        if listed.len() < PER_PAGE {
            return Ok(Ok(all));
        }
    }
    Ok(Err(format!(
        "{base} has more rules than {MAX_PAGES} pages list: they are not judged in part"
    )))
}

/// The policy of the base branch read now, the repository and its rules through `host` and who
/// bypasses each ruleset through `reader`, or the answer that stops the delivery: `refused`
/// with what is missing, or `unavailable` where GitHub gave no answer to judge.
pub(crate) fn check(
    host: &Host<'_>,
    reader: &Reader<'_>,
    wanted: &Wanted<'_>,
) -> Result<Policy, Value> {
    let (status, repository) = host.get("")?;
    if status != 200 {
        let missing = [format!(
            "the repository is not readable with the merging token (GitHub answered {status})"
        )];
        return Err(refused(wanted, &missing));
    }
    let rules = match rules(host, wanted.base)? {
        Ok(rules) => rules,
        Err(why) => return Err(refused(wanted, &[why])),
    };
    let mut bypasses = BTreeMap::new();
    for id in rules.iter().filter_map(|rule| rule["ruleset_id"].as_i64()) {
        if let Entry::Vacant(unread) = bypasses.entry(id) {
            unread.insert(reader.bypass(id)?);
        }
    }
    let missing = judge(&repository, &rules, &bypasses, wanted);
    if !missing.is_empty() {
        return Err(refused(wanted, &missing));
    }
    Ok(Policy {
        digest: crate::digest(&[
            &repository["id"].to_string(),
            wanted.base,
            &Value::Array(rules).to_string(),
            &format!("{bypasses:?}"),
        ]),
    })
}

/// One rule's kind, as GitHub names it.
fn kind(rule: &Value) -> &str {
    rule["type"].as_str().unwrap_or_default()
}

/// What `repository`, the `rules` in effect and each ruleset's `bypasses` leave missing of what
/// `wanted` asks: nothing where the policy holds.
#[must_use]
pub fn judge(
    repository: &Value,
    rules: &[Value],
    bypasses: &BTreeMap<i64, Bypass>,
    wanted: &Wanted<'_>,
) -> Vec<String> {
    let base = wanted.base;
    let mut missing = Vec::new();
    identity(repository, wanted, &mut missing);
    // A rule holds everyone only from a ruleset whose bypass list was read and is empty.
    let of = |rule: &Value| rule["ruleset_id"].as_i64().and_then(|id| bypasses.get(&id));
    let firm =
        |rule: &&Value| matches!(of(rule), Some(Bypass::Actors(actors)) if actors.is_empty());
    let named = |name: &'static str| rules.iter().filter(move |rule| kind(rule) == name);
    if rules.iter().any(|rule| kind(rule).is_empty()) {
        missing.push(format!("a rule of {base} does not read: it names no type"));
    }
    for (name, what) in [
        ("pull_request", "requires a pull request"),
        ("non_fast_forward", "blocks a force push"),
        ("deletion", "blocks its deletion"),
    ] {
        let found: Vec<&Value> = named(name).collect();
        if !found.iter().any(firm) {
            missing.push(unfirm(base, what, &found, bypasses));
        }
    }
    // The check, from the one App that writes it.
    let requires = |rule: &Value, app: Option<i64>| {
        (rule["parameters"]["required_status_checks"].as_array())
            .into_iter()
            .flatten()
            .any(|check| {
                check["context"] == json!(ELIGIBLE)
                    && app.is_none_or(|app| check["integration_id"] == json!(app))
            })
    };
    let checks: Vec<&Value> = named("required_status_checks").collect();
    let pinned: Vec<&Value> = (checks.iter().copied())
        .filter(|rule| requires(rule, Some(wanted.checks_app)))
        .collect();
    if !pinned.iter().any(firm) {
        missing.push(
            if pinned.is_empty() && checks.iter().any(|rule| requires(rule, None)) {
                format!(
                    "{ELIGIBLE} is required of {base} from another source than App {}: a status of \
                 that name by anyone else would pass it",
                    wanted.checks_app
                )
            } else {
                let what = format!("requires {ELIGIBLE} from App {}", wanted.checks_app);
                unfirm(base, &what, &pinned, bypasses)
            },
        );
    }
    if checks
        .iter()
        .any(|rule| rule["parameters"]["strict_required_status_checks_policy"] == json!(true))
    {
        missing.push(format!(
            "a rule holds {base}'s pull requests up to date with it before they merge, which a \
             delivery's one merge of {base} does not meet"
        ));
    }
    updates(
        &named("update").collect::<Vec<_>>(),
        bypasses,
        wanted,
        &mut missing,
    );
    if named("merge_queue").next().is_some() {
        missing.push(format!(
            "{base} merges through a merge queue, which this build delivers nothing through"
        ));
    }
    methods(
        repository,
        &named("pull_request").filter(firm).collect::<Vec<_>>(),
        wanted,
        &mut missing,
    );
    missing
}

/// Why no rule that `what` holds everyone: none in effect, or each from a ruleset someone may
/// bypass or whose bypass list is not known.
fn unfirm(base: &str, what: &str, found: &[&Value], bypasses: &BTreeMap<i64, Bypass>) -> String {
    if found.is_empty() {
        return format!("no rule in effect for {base} {what}");
    }
    let why: Vec<String> = found
        .iter()
        .map(|rule| match rule["ruleset_id"].as_i64() {
            None => "one from no ruleset named".to_string(),
            Some(id) => match bypasses.get(&id) {
                Some(Bypass::Actors(actors)) => format!(
                    "ruleset {id} is bypassed by {}",
                    actors.iter().map(actor).collect::<Vec<_>>().join(", ")
                ),
                Some(Bypass::Unknown(why)) => {
                    format!("who bypasses ruleset {id} is not known ({why})")
                }
                None => format!("ruleset {id} was not read"),
            },
        })
        .collect();
    format!(
        "the rule that {what} holds not everyone on {base}: {}",
        why.join(", ")
    )
}

/// A bypass actor, as a person reads it: remote text kept to plain characters.
fn actor((kind, id, mode): &(String, Option<i64>, String)) -> String {
    let plain = |text: &str| -> String {
        (text.chars())
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(40)
            .collect()
    };
    format!(
        "{} {} ({})",
        plain(kind),
        id.map_or_else(|| "of no number".into(), |id| id.to_string()),
        plain(mode)
    )
}

fn identity(repository: &Value, wanted: &Wanted<'_>, missing: &mut Vec<String>) {
    let base = wanted.base;
    if repository["id"] != json!(wanted.repository) {
        missing.push(format!(
            "the repository answering is number {}, not the {} bound",
            repository["id"], wanted.repository
        ));
    }
    if repository["default_branch"] != json!(base) {
        missing.push(format!(
            "{base} is not the repository's default branch as GitHub names it"
        ));
    }
    if repository["archived"] != json!(false) {
        missing.push("the repository is archived, or does not say it is not".into());
    }
    match repository["permissions"]["admin"].as_bool() {
        Some(false) => {}
        Some(true) => missing.push(
            "the merging token administers the repository: what merges must not be able to \
             change the rules it is held to"
                .into(),
        ),
        None => missing.push(
            "whether the merging token administers the repository is not shown: the repository \
             does not say it does not"
                .into(),
        ),
    }
    if repository["allow_auto_merge"] != json!(false) {
        missing.push(
            "auto-merge is allowed, or the repository's merge settings are not shown to the \
             merging token: a merge GitHub makes later by itself is not delivered through"
                .into(),
        );
    }
}

/// Updates of the base restricted to the landing App: a rule restricts them whose ruleset's one
/// bypass actor is that App.
fn updates(
    found: &[&Value],
    bypasses: &BTreeMap<i64, Bypass>,
    wanted: &Wanted<'_>,
    missing: &mut Vec<String>,
) {
    let (base, app) = (wanted.base, wanted.landing_app);
    let landing = |(kind, id, mode): &(String, Option<i64>, String)| {
        kind == "Integration"
            && *id == Some(app)
            && ["always", "pull_request", "exempt"].contains(&mode.as_str())
    };
    let of = |rule: &&Value| rule["ruleset_id"].as_i64().and_then(|id| bypasses.get(&id));
    let alone = found.iter().any(|rule| {
        matches!(of(rule), Some(Bypass::Actors(actors)) if actors.len() == 1 && landing(&actors[0]))
    });
    if alone {
        return;
    }
    if found.is_empty() {
        missing.push(format!(
            "no rule restricts updates of {base} to the landing App {app}: anyone who may write \
             merges a pull request whose {ELIGIBLE} is green, its evidence withdrawn since or not"
        ));
        return;
    }
    let why: Vec<String> = found
        .iter()
        .map(|rule| match (rule["ruleset_id"].as_i64(), of(rule)) {
            (Some(id), Some(Bypass::Actors(actors))) if actors.is_empty() => {
                format!("ruleset {id} lets nobody through, the landing App neither")
            }
            (Some(id), Some(Bypass::Actors(actors))) => format!(
                "ruleset {id} lets through {}",
                actors.iter().map(actor).collect::<Vec<_>>().join(", ")
            ),
            (Some(id), Some(Bypass::Unknown(why))) => {
                format!("who ruleset {id} lets through is not known ({why})")
            }
            _ => "one from no ruleset read".to_string(),
        })
        .collect();
    missing.push(format!(
        "updates of {base} are not restricted to the landing App {app} alone (Integration {app}): \
         {}",
        why.join(", ")
    ));
}

/// The merge methods the repository and the rules nobody bypasses permit, which must be the
/// operation's and no other.
fn methods(repository: &Value, pulls: &[&Value], wanted: &Wanted<'_>, missing: &mut Vec<String>) {
    let base = wanted.base;
    let mut permitted = BTreeSet::new();
    for (method, setting) in METHODS {
        match repository[setting].as_bool() {
            Some(true) => {
                permitted.insert(method);
            }
            Some(false) => {}
            None => {
                missing.push(
                    "the repository's merge methods are not shown to the merging token (GitHub \
                     shows them with contents: write)"
                        .into(),
                );
                return;
            }
        }
    }
    for rule in pulls {
        if let Some(allowed) = rule["parameters"]["allowed_merge_methods"].as_array() {
            permitted.retain(|method| allowed.contains(&json!(method)));
        }
    }
    let wants: BTreeSet<&str> = wanted.operation.permitted().iter().copied().collect();
    if permitted != wants {
        let said = |set: &BTreeSet<&str>| {
            if set.is_empty() {
                "none".to_string()
            } else {
                set.iter().copied().collect::<Vec<_>>().join(", ")
            }
        };
        missing.push(format!(
            "{base} permits these merge methods: {}; a delivery takes exactly: {}",
            said(&permitted),
            said(&wants)
        ));
    }
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
