//! What a delivery reads of the repository's protection before it writes: the rules GitHub
//! applies to the base branch, and whether they hold a delivery to its evidence. It reads and
//! never writes, and it is asked before each consequential step ([`crate::deliver`]): before a
//! head with standing evidence is pushed, and before a merge is asked for.
//!
//! Three reads, each with the one App's token, which administers nothing:
//!
//! 1. the repository (`GET /repos/O/N`): its number, default branch, merge settings and what the
//!    token may do there;
//! 2. the rules in effect for the base branch (`GET /repos/O/N/rules/branches/BASE`, every page),
//!    which GitHub evaluates from every active ruleset that applies, the organization's among
//!    them;
//! 3. each ruleset a rule comes from (`GET /repos/O/N/rulesets/ID`): whether it is active, when
//!    it last changed (`updated_at`) and whether the App itself may bypass it
//!    (`current_user_can_bypass`).
//!
//! GitHub shows a ruleset's bypass list only to who may write the ruleset, which the App must not
//! be able to do. Who else bypasses each is what someone who administers the repository verified
//! with their own gh CLI ([`crate::rulesets`]), kept in the state ([`Verified`]), and it is taken
//! only while the ruleset's `updated_at` is still the instant verified. A ruleset changed since,
//! in its enforcement or its bypass list, which each move `updated_at` (docs/GITHUB.md, "Who
//! bypasses the rules"), is not known to hold anyone until it is verified again.
//!
//! It passes only what it read whole. A rule counts as holding everyone only when it comes from
//! a ruleset verified with an empty bypass list, unchanged since, that the App does not bypass;
//! a list not verified is not known to be empty. What must hold:
//!
//! - **identity**: the repository is the one bound, by its number, and the base is its default
//!   branch; it is not archived, and it says the token does not administer it, which is not
//!   proof the token cannot edit a ruleset;
//! - **a pull request** is required, by a rule nobody bypasses;
//! - **the check** [`crate::deliver::ELIGIBLE`] is required from the one App that writes it, by
//!   a rule nobody bypasses, and not with the branch held up to date, which a delivery's one
//!   merge of the default branch does not meet;
//! - **no force push and no deletion**, by rules nobody bypasses;
//! - **no rule restricts updates** of the base: the required check is what holds a merge to its
//!   evidence, whoever asks for it, and a restriction would leave a pull request whose check is
//!   green to a bypass, which GitHub's auto-merge does not take;
//! - **the merge method**: what the repository and those rules permit is the operation's method
//!   and no other: squash alone, or squash and merge commits where the settings name an upstream
//!   an import is merged from ([`Operation::Imports`]), and then no rule that requires a linear
//!   history, which no merge commit meets; no merge queue, which this build does not deliver
//!   through.
//!
//! Classic branch protection is not read: it can add to what the rules require and never take
//! from it, and reading it asks for administration, which the token must not have.
//!
//! What no read here establishes is said in docs/GITHUB.md, "The protection a delivery needs":
//! which App a token is of, that no administrator edits a rule between this read and GitHub's
//! own act, and how GitHub applies two rulesets together.

use crate::deliver::{ELIGIBLE, Host};
pub use crate::state::Verified;
use serde_json::{Value, json};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

/// A page's size, the largest GitHub serves.
pub(crate) const PER_PAGE: usize = 100;
/// The pages of rules read before the listing is said to be more than is read.
pub(crate) const MAX_PAGES: usize = 10;
/// The merge methods GitHub has, as its rules name them.
const METHODS: [(&str, &str); 3] = [
    ("squash", "allow_squash_merge"),
    ("merge", "allow_merge_commit"),
    ("rebase", "allow_rebase_merge"),
];

/// What is delivered to the base branch, which decides the merge methods it must permit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Tasks alone, each head squashed into one commit.
    Task,
    /// Tasks, squashed, and imports of the one upstream the settings name, each merged by a merge
    /// commit that keeps the upstream's history: what a repository with `upstream_id` delivers,
    /// whichever of the two a step is for, since the branch's rules are one for both.
    Imports,
}

impl Operation {
    /// What a repository whose settings say `upstream_id` or not delivers.
    #[must_use]
    pub fn of(imports: bool) -> Operation {
        if imports {
            Operation::Imports
        } else {
            Operation::Task
        }
    }

    /// The methods the base branch may permit for it: the ones it merges by.
    #[must_use]
    pub fn permitted(self) -> &'static [&'static str] {
        match self {
            Operation::Task => &["squash"],
            Operation::Imports => &["merge", "squash"],
        }
    }
}

/// What a delivery is held to.
#[derive(Clone, Copy, Debug)]
pub struct Wanted<'a> {
    /// The repository's number, as bound.
    pub repository: i64,
    pub base: &'a str,
    /// The one App: whose checks count, and whose token reads.
    pub app: i64,
    pub operation: Operation,
}

/// Who may bypass a ruleset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bypass {
    /// Its list is known: each actor's type, number and mode.
    Actors(Vec<(String, Option<i64>, String)>),
    /// Its list is not known, and why.
    Unknown(String),
}

/// What a person runs to verify the rulesets again, said where one is not known.
pub const VERIFY: &str = "who administers the repository verifies them with `symphony-github \
                          rulesets --verify` (docs/GITHUB_SETUP.md)";

/// A policy that passed: a digest of everything read, which another policy does not share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub digest: String,
}

/// Who may bypass ruleset `id`: read with the App's token through `host`, whether it is active,
/// when it last changed and whether the App bypasses it; who else does, from `verified`, the
/// record of it kept, where the ruleset has not changed since.
///
/// # Errors
///
/// GitHub gave no answer to judge: `unavailable`, of `kind` `policy`, so a land tries again and
/// evidence that blocks is still said.
pub(crate) fn bypass(
    host: &Host<'_>,
    verified: Option<&Verified>,
    id: i64,
    wanted: &Wanted<'_>,
) -> Result<Bypass, Value> {
    let (status, value) = host
        .get(&format!("/rulesets/{id}"))
        .map_err(|mut stopped| {
            stopped["kind"] = json!("policy");
            stopped["why"] = json!(format!(
                "ruleset {id} was not read with the App's token: {}",
                stopped["why"].as_str().unwrap_or_default()
            ));
            stopped
        })?;
    if status != 200 {
        return Ok(Bypass::Unknown(format!(
            "GitHub answered {status} to the App's token"
        )));
    }
    if value["id"] != json!(id) || value["enforcement"] != json!("active") {
        return Ok(Bypass::Unknown("it does not read as active".into()));
    }
    let Some(verified) = verified.filter(|verified| verified.repository == wanted.repository)
    else {
        return Ok(Bypass::Unknown(format!("it was never verified: {VERIFY}")));
    };
    let said = value["updated_at"].as_str();
    match (said.and_then(instant), instant(&verified.updated_at)) {
        (Some(now), Some(then)) if now == then => {}
        (None, _) => {
            return Ok(Bypass::Unknown(
                "it shows no time it last changed that reads, so its verification cannot be held \
                 to it"
                    .into(),
            ));
        }
        (Some(_), None) => {
            return Ok(Bypass::Unknown(format!(
                "its verification kept no time it last changed that reads: {VERIFY}"
            )));
        }
        (Some(_), Some(_)) => {
            return Ok(Bypass::Unknown(format!(
                "it changed at {} after it was verified as last changed at {}: {VERIFY}",
                plain(said.unwrap_or_default()),
                plain(&verified.updated_at)
            )));
        }
    }
    let mut actors = verified.bypass.clone();
    match value["current_user_can_bypass"].as_str() {
        Some("never") => {}
        Some(mode) => {
            let own = |(kind, actor, _): &(String, Option<i64>, String)| {
                kind == "Integration" && *actor == Some(wanted.app)
            };
            if !actors.iter().any(own) {
                actors.push(("Integration".into(), Some(wanted.app), mode.into()));
            }
        }
        None => {
            return Ok(Bypass::Unknown(
                "whether the App bypasses it is not shown".into(),
            ));
        }
    }
    Ok(Bypass::Actors(actors))
}

/// GitHub's time `text` (`YYYY-MM-DDTHH:MM:SS`, a fraction of a second or none, then `Z` or an
/// offset `+HH:MM`), as milliseconds since 1970: one instant reads alike whichever zone it is
/// said in, as gh and the App's token say it in different ones. None for any other text.
#[must_use]
pub fn instant(text: &str) -> Option<i64> {
    let (clock, rest) = (text.get(..19)?, text.get(19..)?);
    let seconds = crate::mint::seconds(&format!("{clock}Z"))?;
    let (fraction, zone) = match rest.strip_prefix('.') {
        Some(rest) => {
            let digits = rest.find(|c: char| !c.is_ascii_digit())?;
            (rest.get(..digits)?, rest.get(digits..)?)
        }
        None => ("", rest),
    };
    if (rest.starts_with('.') && fraction.is_empty()) || fraction.len() > 9 {
        return None;
    }
    let millis: i64 = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    let offset = if zone == "Z" { 0 } else { offset(zone)? };
    Some((seconds - offset) * 1_000 + millis)
}

/// The seconds east of UTC an offset `+HH:MM` or `-HH:MM` says; none for any other text.
fn offset(zone: &str) -> Option<i64> {
    let (sign, rest) = match zone.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':')?;
    let two = |part: &str| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit());
    if !two(hours) || !two(minutes) {
        return None;
    }
    let (hours, minutes): (i64, i64) = (hours.parse().ok()?, minutes.parse().ok()?);
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

/// Remote text as a person reads it: kept to plain characters.
fn plain(text: &str) -> String {
    (text.chars())
        .filter(|c| c.is_ascii_alphanumeric() || "-:.+_".contains(*c))
        .take(40)
        .collect()
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
                "the rules of {base} are not readable with the App's token (GitHub answered \
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

/// The policy of the base branch read now through `host`, the repository, its rules and each
/// ruleset they come from, who bypasses each as `verified` says where it has not changed since,
/// or the answer that stops the delivery: `refused` with what is missing, or `unavailable` where
/// GitHub gave no answer to judge.
pub(crate) fn check(
    host: &Host<'_>,
    verified: &BTreeMap<i64, Verified>,
    wanted: &Wanted<'_>,
) -> Result<Policy, Value> {
    let (status, repository) = host.get("")?;
    if status != 200 {
        let missing = [format!(
            "the repository is not readable with the App's token (GitHub answered {status})"
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
            unread.insert(bypass(host, verified.get(&id), id, wanted)?);
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

/// What `repository`, read with the App's token, the `rules` in effect and each ruleset's
/// `bypasses` leave missing of what `wanted` asks: nothing where the policy holds.
#[must_use]
pub fn judge(
    repository: &Value,
    rules: &[Value],
    bypasses: &BTreeMap<i64, Bypass>,
    wanted: &Wanted<'_>,
) -> Vec<String> {
    let mut missing = Vec::new();
    unadministered(repository, &mut missing);
    missing.extend(judge_rules(repository, rules, bypasses, wanted));
    missing
}

/// What [`judge`] finds missing but whether the token read with administers the repository: the
/// judgement of a read made by someone who does ([`crate::rulesets`]).
#[must_use]
pub fn judge_rules(
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
        .filter(|rule| requires(rule, Some(wanted.app)))
        .collect();
    if !pinned.iter().any(firm) {
        missing.push(
            if pinned.is_empty() && checks.iter().any(|rule| requires(rule, None)) {
                format!(
                    "{ELIGIBLE} is required of {base} from another source than App {}: a status of \
                 that name by anyone else would pass it",
                    wanted.app
                )
            } else {
                let what = format!("requires {ELIGIBLE} from App {}", wanted.app);
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
    updates(&named("update").collect::<Vec<_>>(), wanted, &mut missing);
    if named("merge_queue").next().is_some() {
        missing.push(format!(
            "{base} merges through a merge queue, which this build delivers nothing through"
        ));
    }
    if wanted.operation == Operation::Imports && named("required_linear_history").next().is_some() {
        missing.push(format!(
            "a rule requires a linear history of {base}, which refuses the merge commit an import \
             is delivered by"
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
pub(crate) fn actor((kind, id, mode): &(String, Option<i64>, String)) -> String {
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
}

/// The App's token administers nothing: what merges must not be able to change the rules it is
/// held to. Judged of a read with that token alone.
fn unadministered(repository: &Value, missing: &mut Vec<String>) {
    match repository["permissions"]["admin"].as_bool() {
        Some(false) => {}
        Some(true) => missing.push(
            "the App's token administers the repository: what merges must not be able to \
             change the rules it is held to"
                .into(),
        ),
        None => missing.push(
            "whether the App's token administers the repository is not shown: the repository \
             does not say it does not"
                .into(),
        ),
    }
}

/// No rule restricts updates of the base: whoever it lets through, a pull request whose check is
/// green is then merged only past it, which GitHub's auto-merge does not do, and the check is what
/// holds a merge to its evidence.
fn updates(found: &[&Value], wanted: &Wanted<'_>, missing: &mut Vec<String>) {
    let rulesets: BTreeSet<String> = (found.iter())
        .map(|rule| {
            rule["ruleset_id"].as_i64().map_or_else(
                || "one from no ruleset named".into(),
                |id| format!("ruleset {id}"),
            )
        })
        .collect();
    if !rulesets.is_empty() {
        missing.push(format!(
            "a rule restricts updates of {} ({}): a pull request whose {ELIGIBLE} is green \
             merges only past it, and the check alone holds a merge to its evidence",
            wanted.base,
            rulesets.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
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
                    "the repository's merge methods are not shown to the App's token (GitHub \
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
