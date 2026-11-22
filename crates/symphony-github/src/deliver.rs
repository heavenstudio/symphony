//! `symphony-github deliver VERB --request FILE`: what answers a land delivered by a pull request
//! (`[land] delivery = ["symphony-github", "deliver"]`, docs/TASKS.md), one JSON object read and
//! one printed. It serves only `enabled` settings whose `pull_requests` is `publish`, and decides
//! nothing: what a task needs and whether it stands is the evidence the land hands it, which it
//! writes down as checks and never reads the board for.
//!
//! `publish` and `merge` first read the protection of the base branch ([`crate::preflight`]) and
//! refuse where it does not hold a delivery to its evidence; evidence that blocks is still
//! said there, by its checks alone on the pull request already bound. Every request carries the
//! one App's token, which administers nothing: who else bypasses the rules is what was verified
//! by someone who does ([`crate::rulesets`]), taken while each ruleset is unchanged since.
//!
//! - `publish` pushes the head to the branch named as the task, never forced and only to an
//!   `origin` that names the repository the settings do; finds the pull request of that branch
//!   before it opens one, ready for review (GitHub arms no auto-merge on a draft, and one bound
//!   as a draft is marked ready); and writes one check for the assessment, one for each stamp
//!   and `symphony/eligible`, then reads them back as written by the one App the settings name,
//!   again after each of [`READ_BACK`]'s rests while one is not listed yet, with nothing written
//!   again. A pull request open already is then read again the same way while it is still at the
//!   head it was at before the push, as GitHub moves it a few seconds after ([`Run::moved`]), so
//!   `published` names the head pushed. Each check passes, is neutral (a stamp waived), fails (a stamp red at the head, or
//!   evidence that stood there withdrawn) or is pending ([`Conclusion`]): a check run still in
//!   progress, which holds a required check and so GitHub's merge until it completes. A check
//!   that stands is found by its name and what it says, so a second run writes none;
//!   `symphony/eligible` is written last when it passes and first otherwise, so it never passes
//!   over a stamp not yet written.
//! - `arm` turns on GitHub's auto-merge for the bound pull request at the head it is given
//!   (`expectedHeadOid`), by the squash and the text `merge` would ask for, after reading the
//!   protection as `merge` does: GitHub then merges it itself once every required check passes,
//!   `symphony/eligible` among them. A pull request GitHub can merge already takes no
//!   auto-merge, which it answers `mergeable`, and the land asks `merge`.
//! - `merge` asks GitHub once to squash the bound pull request at the head it is given
//!   (`sha`, which GitHub refuses a merge of any other head by), after reading that it is not
//!   merged already.
//! - `disarm` turns that auto-merge off, so no check that passes later merges it; `close` turns
//!   it off and closes the pull request unmerged, its branch left. Each answers `merged` where
//!   GitHub merged it first.
//!
//! An import (`operation` `import`, with the `upstream`'s address and the commit it imports) is
//! the one delivery merged by a merge commit, titled as a land's own merge. Only settings that
//! name `upstream_id` deliver one, and before its head is pushed and again before its merge the
//! upstream is read at its address and held to that number, never the repository delivered to,
//! and refused where a private repository's history would go into a public one
//! ([`Run::upstream`]). A request of another operation is squashed, whatever it asks.
//! - `result` reads the bound pull request and writes nothing: whether it is open, merged or
//!   closed, whether auto-merge is on, GitHub's word on whether it merges (`mergeable_state`),
//!   and, while it is open, each check at its head that is not this App's and did not pass.
//! - `landed` records the commit, and closes the task's mapped issue where `close_issues` says.
//!
//! Every write is journaled before it is made, and what it would make is looked for first. The
//! pull request's public text is the task's curated text ([`crate::state::Draft`]) or, with
//! none, its slug: never a brief, a thread or the title the land hands over. No token is in
//! an argument, a URL, an environment variable, the journal, the state or a line said here.

use crate::config::{Config, PullRequests, Repository};
use crate::github::clean_title;
use crate::mint;
use crate::preflight::{self, Operation, Wanted};
use crate::reconcile::{Context, Outcome};
use crate::state::{Binding, Entry, Event, Locked, Pull, State, Store};
use crate::transport::{Failure, Request, Transport, rate_limited};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The check that says every other stands: the one a branch's protection requires.
pub const ELIGIBLE: &str = "symphony/eligible";
/// The check that says an assessor read the head.
pub const ASSESSMENT: &str = "symphony/assessment";
/// The evidence's shape this build writes checks of (`symphony_board::land::evidence::VERSION`).
pub const SNAPSHOT: u64 = 1;
/// The rests between two reads of a head's checks while one written is not listed yet, as
/// GitHub lists a check a few seconds after it takes it, and then between two reads of a pull
/// request still at the head it was at before a push, as GitHub moves it a few seconds after:
/// 15 s for each, so both end well inside the network bound the land runs this program in (300 s
/// at most), and a check that never shows or a head that never moves is answered `unavailable`
/// here rather than cut short there.
pub const READ_BACK: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

/// A delivery's rest between two reads of the checks it wrote or of the pull request it pushed
/// to ([`READ_BACK`]), as nothing of GitHub's tells when it lists the ones or moves the other.
pub fn rest(wait: Duration) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the named helper of RULES.md code/no-timed-waits for a retry with backoff: a delivery's read back of the checks it wrote and of the head it pushed"
    )]
    std::thread::sleep(wait);
}

/// The project's own git, as far as a delivery uses it.
pub trait Remote {
    /// Where `origin` leads.
    ///
    /// # Errors
    ///
    /// The checkout has no `origin`.
    fn url(&self) -> Result<String, String>;

    /// `head` pushed to `origin`'s branch `branch`, never forced.
    ///
    /// # Errors
    ///
    /// The push is refused: the branch there holds what `head` does not.
    fn push(&self, head: &str, branch: &str) -> Result<(), String>;
}

/// The main checkout's git, with the person's own credentials for it.
pub struct Git {
    pub main: PathBuf,
}

impl Git {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let ran = Command::new("git")
            .arg("-C")
            .arg(&self.main)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| format!("git: {error}"))?;
        if ran.status.success() {
            Ok(String::from_utf8_lossy(&ran.stdout).trim().to_string())
        } else {
            let said = String::from_utf8_lossy(&ran.stderr);
            Err(said.lines().last().unwrap_or("git failed").to_string())
        }
    }
}

impl Remote for Git {
    fn url(&self) -> Result<String, String> {
        self.run(&["remote", "get-url", "--push", "origin"])
    }

    fn push(&self, head: &str, branch: &str) -> Result<(), String> {
        let pushed = format!("{head}:refs/heads/{branch}");
        self.run(&["push", "--quiet", "origin", &pushed]).map(drop)
    }
}

/// The `OWNER/NAME` a remote address `url` on GitHub.com names, as written; none for another host.
fn path(url: &str) -> Option<&str> {
    let rest = [
        "https://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|start| url.strip_prefix(start))?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    Some(rest.strip_suffix(".git").unwrap_or(rest))
}

/// Whether the remote address `url` is `repository` on GitHub.com.
#[must_use]
pub fn names(url: &str, repository: &Repository) -> bool {
    path(url).is_some_and(|rest| rest.eq_ignore_ascii_case(&repository.full_name()))
}

/// The repository on GitHub.com the remote address `url` names; none for another host or for
/// what is no plain `OWNER/NAME`.
#[must_use]
pub fn repository(url: &str) -> Option<Repository> {
    crate::config::repository(path(url)?).ok()
}

/// An answer that made or found nothing, said for the land to carry.
pub(crate) fn refused(why: impl Into<String>) -> Value {
    json!({"state": "refused", "why": why.into()})
}

/// What a GraphQL mutation came to.
enum Mutated {
    Made,
    /// Refused: GitHub's status, and why it said.
    Refused(u16, String),
    /// Perhaps made: why it is not known.
    Lost(String),
}

/// The bound pull request read again after a write GitHub refused: `merged` or `closed` where it
/// is so now, else none.
fn ended(host: &Host<'_>, pull: &Pull) -> Result<Option<Value>, Stopped> {
    let now = host.read(&format!("/pulls/{}", pull.number), "the pull request")?;
    Ok(if now["merged"] == json!(true) {
        Some(json!({"state": "merged", "sha": now["merge_commit_sha"]}))
    } else if now["state"] != json!("open") {
        Some(json!({"state": "closed"}))
    } else {
        None
    })
}

/// Why GitHub refused a GraphQL mutation: the first of its errors, or, refused before GraphQL
/// read it (a 403), the message REST answers with.
fn refusal(said: &Value) -> String {
    clean_title(
        (said["errors"][0]["message"].as_str())
            .or_else(|| said["message"].as_str())
            .unwrap_or_default(),
    )
}

pub(crate) fn unavailable(why: impl Into<String>) -> Value {
    json!({"state": "unavailable", "why": why.into()})
}

/// A request that came to no answer the verb can go on by: the answer to give in its place.
pub(crate) type Stopped = Value;

/// The repository's calls, each answered with its status.
pub(crate) struct Host<'a> {
    pub(crate) transport: &'a dyn Transport,
    pub(crate) repository: Repository,
}

/// A write that may have been made.
pub(crate) struct Lost(pub(crate) String);

impl Host<'_> {
    pub(crate) fn path(&self, rest: &str) -> String {
        crate::github::path(&self.repository, rest)
    }

    pub(crate) fn send(
        &self,
        method: &'static str,
        path: String,
        body: Option<&Value>,
    ) -> Result<Result<(u16, Value), Lost>, Stopped> {
        let request = Request {
            method,
            path,
            body: body.map(Value::to_string),
        };
        let write = method != "GET";
        let what = format!("{method} {}", request.path);
        let response = match self.transport.send(&request) {
            Ok(response) => response,
            Err(Failure::Unknown(why)) if write => return Ok(Err(Lost(why))),
            Err(Failure::NotSent(why) | Failure::Unknown(why)) => return Err(unavailable(why)),
        };
        let limited = rate_limited(&response).is_some();
        match response.status {
            _ if limited => Err(unavailable("GitHub's rate limit: nothing more is sent now")),
            401 => Err(unavailable("GitHub refused the credential (401)")),
            500..=599 if write => Ok(Err(Lost(format!(
                "GitHub answered {} to {what}",
                response.status
            )))),
            500..=599 => Err(unavailable(format!(
                "GitHub answered {} to {what}",
                response.status
            ))),
            status => Ok(Ok((
                status,
                serde_json::from_str(&response.body).unwrap_or(Value::Null),
            ))),
        }
    }

    /// A read: its status and JSON.
    pub(crate) fn get(&self, rest: &str) -> Result<(u16, Value), Stopped> {
        match self.send("GET", self.path(rest), None)? {
            Ok(answer) => Ok(answer),
            Err(Lost(why)) => Err(unavailable(why)),
        }
    }

    /// A read that must be answered 200.
    fn read(&self, rest: &str, what: &str) -> Result<Value, Stopped> {
        match self.get(rest)? {
            (200, value) => Ok(value),
            (status, _) => Err(unavailable(format!("{what}: GitHub answered {status}"))),
        }
    }
}

/// The evidence a land hands over, as far as checks are written of it.
#[derive(Deserialize)]
struct Snapshot {
    version: u64,
    slug: String,
    task: i64,
    head: String,
    assessed: bool,
    eligible: bool,
    revision: String,
    stamps: Vec<Stamp>,
    #[serde(default)]
    assessment: Option<Assessed>,
}

/// What a snapshot's assessment says, as far as a delivery reads it: the head it read, and the
/// upstream commit it names that head an import of, where it names one.
#[derive(Deserialize)]
struct Assessed {
    #[serde(default)]
    tip: Option<String>,
    #[serde(default)]
    import: Option<String>,
}

/// The method a merge of `request`'s `head` is asked by, which the request names itself: a task's
/// squashed, an import's (`imported`) by a merge commit. A head that is no whole commit, or
/// another method, is refused.
fn merge_method(request: &Value, head: &str, imported: bool) -> Result<&'static str, Stopped> {
    let method = if imported { "merge" } else { "squash" };
    if !commit(head) || request["method"] != json!(method) {
        return Err(refused(
            "a merge is of one whole commit: a task's squashed, an import's by a merge commit",
        ));
    }
    Ok(method)
}

/// What a request says it delivers: an import of `commit` from the repository at `url`, or none.
fn import(request: &Value) -> Option<(String, String)> {
    (request["operation"] == json!("import")).then(|| {
        (
            text(&request["upstream"], "url"),
            text(&request["upstream"], "commit"),
        )
    })
}

#[derive(Deserialize)]
struct Stamp {
    name: String,
    standing: String,
    /// Needed, missing, and red at the head (a red run there): its check fails, where a stamp
    /// missing otherwise is pending. A board that says nothing of it says none is red.
    #[serde(default)]
    red: bool,
}

/// How a check says it: concluded, or pending. A pending check is a check run still in progress,
/// with no conclusion, which holds a required check, and so GitHub's merge, until it completes.
pub(crate) type Conclusion = Option<&'static str>;

/// What a pending check is said and journaled as.
const PENDING: &str = "pending";

/// The title of `symphony/eligible` failing because evidence that stood at the head is no longer
/// there: how a later publication of evidence still short of standing tells it from one that
/// never stood, and keeps it failing rather than pending.
const WITHDRAWN: &str = "The evidence that stood at this commit was withdrawn";

/// The conclusions a check that did not pass ends in.
const FAILED: [&str; 5] = [
    "failure",
    "cancelled",
    "timed_out",
    "action_required",
    "startup_failure",
];

/// A check to stand on the head.
#[derive(Debug, PartialEq, Eq)]
struct Check {
    name: String,
    conclusion: Conclusion,
    /// What it is found again by: the repository, the task, the head, its name, how it
    /// concluded and the evidence's revision.
    id: String,
    title: String,
}

impl Check {
    /// Its conclusion, or [`PENDING`].
    fn said(&self) -> &'static str {
        self.conclusion.unwrap_or(PENDING)
    }

    /// Whether `run`, as GitHub lists it, says exactly this on `head`.
    fn stands(&self, run: &Value, head: &str) -> bool {
        let concluded = match self.conclusion {
            Some(conclusion) => run["conclusion"] == json!(conclusion),
            None => run["status"] != json!("completed") && run["conclusion"].is_null(),
        };
        run["name"] == json!(self.name)
            && run["external_id"] == json!(self.id)
            && run["head_sha"] == json!(head)
            && concluded
    }

    /// What is sent to write it on `head`.
    fn sent(&self, head: &str) -> Value {
        let mut sent = json!({
            "name": self.name, "head_sha": head, "external_id": self.id,
            "status": if self.conclusion.is_some() { "completed" } else { "in_progress" },
            "output": {"title": self.title, "summary": format!("{} at `{head}`.", self.title)},
        });
        if let Some(conclusion) = self.conclusion {
            sent["conclusion"] = json!(conclusion);
        }
        sent
    }
}

/// The newest of `runs` named `name`: the one GitHub goes by, as a check named again on a head is
/// a run of its own beside the one before.
fn newest<'a>(runs: &'a [Value], name: &str) -> Option<&'a Value> {
    (runs.iter())
        .filter(|run| run["name"] == json!(name))
        .max_by_key(|run| run["id"].as_i64().unwrap_or_default())
}

pub(crate) fn plain(text: &str, more: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && !text.starts_with('-')
        && (text.chars()).all(|c| c.is_ascii_alphanumeric() || more.contains(c))
}

/// The start of a commit's name, as a person reads it.
fn short(commit: &str) -> String {
    commit.chars().take(7).collect()
}

pub(crate) fn commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// The checks of `snapshot` on repository `repository`, in the order they are written:
/// `withdrawn` where `symphony/eligible` passed at this head before, or failed there for evidence
/// withdrawn ([`withdrawn`]), so evidence short of standing now fails it rather than leave it
/// pending.
fn checks(snapshot: &Snapshot, repository: i64, withdrawn: bool) -> Result<Vec<Check>, String> {
    let check = |name: String, conclusion: Conclusion, title: String| Check {
        id: crate::digest(&[
            &repository.to_string(),
            &snapshot.task.to_string(),
            &snapshot.head,
            &name,
            conclusion.unwrap_or(PENDING),
            &snapshot.revision,
        ]),
        name,
        conclusion,
        title,
    };
    let mut wanted = vec![if snapshot.assessed {
        check(
            ASSESSMENT.into(),
            Some("success"),
            "An assessor read this commit".into(),
        )
    } else {
        check(
            ASSESSMENT.into(),
            None,
            "No assessment of this commit yet".into(),
        )
    }];
    let mut red = false;
    for stamp in &snapshot.stamps {
        if !plain(&stamp.name, "-_") {
            return Err(format!("no check is named for the stamp {:?}", stamp.name));
        }
        let (conclusion, said) = match stamp.standing.as_str() {
            "met" => (Some("success"), "stands at this commit"),
            "waived" => (
                Some("neutral"),
                "is waived by the assessment of this commit",
            ),
            "missing" if stamp.red => (Some("failure"), "is red at this commit"),
            "missing" => (None, "does not stand at this commit yet"),
            other => return Err(format!("no check says a stamp is {other:?}")),
        };
        red |= stamp.standing == "missing" && stamp.red;
        wanted.push(check(
            format!("symphony/{}", stamp.name),
            conclusion,
            format!("{} {said}", stamp.name),
        ));
    }
    let (conclusion, title) = if snapshot.eligible {
        (
            Some("success"),
            "Every stamp the task needs stands at this commit",
        )
    } else if red {
        (
            Some("failure"),
            "A stamp the task needs is red at this commit",
        )
    } else if withdrawn {
        (Some("failure"), WITHDRAWN)
    } else {
        (None, "The task is not eligible at this commit yet")
    };
    let eligible = check(ELIGIBLE.into(), conclusion, title.into());
    // Never passing over a stamp not written yet, never failing or pending after them.
    if snapshot.eligible {
        wanted.push(eligible);
    } else {
        wanted.insert(0, eligible);
    }
    Ok(wanted)
}

/// Whether `symphony/eligible` as GitHub holds it now among `ours`, the App's checks on one head,
/// says evidence stood there: it passed, or it failed as [`WITHDRAWN`].
fn withdrawn(ours: &[Value]) -> bool {
    newest(ours, ELIGIBLE).is_some_and(|run| {
        run["conclusion"] == json!("success")
            || (run["conclusion"] == json!("failure") && run["output"]["title"] == json!(WITHDRAWN))
    })
}

/// The repository `host` reaches as GitHub names it now, its number and name, which must be the
/// one `state` is bound to; with none bound yet, it is bound and kept in `store`.
pub(crate) fn bound(
    host: &Host<'_>,
    store: &Store,
    state: &mut State,
) -> Result<(i64, String), Stopped> {
    let value = match host.get("")? {
        (200, value) => value,
        (301, _) => return Err(refused("the repository moved to another name (301)")),
        (status, _) => {
            return Err(refused(format!(
                "{} is missing or not accessible ({status})",
                host.repository.full_name()
            )));
        }
    };
    let id = (value["id"].as_i64()).ok_or_else(|| unavailable("the repository has no id"))?;
    let name = clean_title(value["full_name"].as_str().unwrap_or_default());
    match &state.binding {
        Some(binding) if binding.repository_id != id => Err(refused(format!(
            "{name} is repository {id}, and what is kept here is bound to {} (id {}): \
             nothing is published to another repository",
            binding.full_name, binding.repository_id
        ))),
        Some(_) => Ok((id, name)),
        None => {
            state.binding = Some(Binding {
                repository_id: id,
                node_id: value["node_id"].as_str().unwrap_or_default().into(),
                full_name: name.clone(),
            });
            store.save(state).map_err(unavailable)?;
            Ok((id, name))
        }
    }
}

/// What makes the requests that carry the App's token, made first where the settings give its
/// file an App ([`crate::mint`]).
pub(crate) fn connect(ctx: &Context<'_>, config: &Config) -> Result<Box<dyn Transport>, Stopped> {
    let token = mint::token(ctx, config).map_err(|stop| match stop {
        mint::Stop::Refused(why) => refused(why),
        mint::Stop::Unavailable(why) => unavailable(why),
    })?;
    (ctx.connect)(token).map_err(unavailable)
}

/// One delivery's run: the settings, the state and what it is said through.
struct Run<'a> {
    ctx: &'a Context<'a>,
    config: Config,
    revision: String,
    repository: Repository,
    store: Store,
    state: State,
    by: String,
}

impl Run<'_> {
    fn journal(&self, event: Event, kind: &str, target: &str, id: &str, result: Value) {
        // A journal that cannot be written stops nothing already looked for: the next run looks
        // again before it makes anything.
        let _ = self.store.journal(&Entry {
            at: self.ctx.now,
            event,
            preview: String::new(),
            op: id.into(),
            kind: kind.into(),
            target: target.into(),
            digest: id.into(),
            base: Value::Null,
            config: self.revision.clone(),
            by: self.by.clone(),
            result,
        });
    }

    fn connect(&self) -> Result<Box<dyn Transport>, Stopped> {
        connect(self.ctx, &self.config)
    }

    /// The repository as GitHub names it now, which must be the one the state is bound to.
    fn bound(&mut self, host: &Host<'_>) -> Result<(i64, String), Stopped> {
        bound(host, &self.store, &mut self.state)
    }

    /// The protection of `base` read now and held to what a delivery needs
    /// ([`crate::preflight`]), journaled by its digest: asked before each step that a policy
    /// gone since the last one must stop.
    fn protected(&self, host: &Host<'_>, id: i64, base: &str, slug: &str) -> Result<(), Stopped> {
        let wanted = Wanted {
            repository: id,
            base,
            app: self.config.app_id.unwrap_or_default(),
            operation: Operation::of(self.config.upstream_id.is_some()),
        };
        let read = preflight::check(host, &self.state.rulesets, &wanted);
        match read {
            Ok(policy) => {
                self.journal(Event::Ack, "policy", slug, &policy.digest, json!(base));
                Ok(())
            }
            Err(stopped) => {
                if stopped["kind"] == json!("policy") && stopped["missing"].is_array() {
                    let id = crate::digest(&["policy", slug, base]);
                    self.journal(
                        Event::Failed,
                        "policy",
                        slug,
                        &id,
                        stopped["missing"].clone(),
                    );
                }
                Err(stopped)
            }
        }
    }

    /// The upstream an import of `request` is of, held to what the settings pin, read now with the
    /// App's token `host` carries: the repository at the request's address is the one of
    /// `upstream_id`, not the one bound (`id`), and its history is taken into this repository only
    /// where that makes nothing private public. Its number and name.
    fn upstream(&self, host: &Host<'_>, request: &Value, id: i64) -> Result<Value, Stopped> {
        let (url, commit) = import(request).unwrap_or_default();
        let Some(pinned) = self.config.upstream_id else {
            return Err(refused(format!(
                "the settings name no upstream_id: no import is delivered here, from {} or any \
                 other",
                clean_title(&url)
            )));
        };
        let Some(upstream) = repository(&url).filter(|_| self::commit(&commit)) else {
            return Err(refused(
                "an import names its upstream by its address on GitHub.com and the commit it \
                 imports whole",
            ));
        };
        let theirs = Host {
            transport: host.transport,
            repository: upstream.clone(),
        };
        let said = upstream.full_name();
        let value = match theirs.get("")? {
            (200, value) => value,
            (301, _) => return Err(refused(format!("the upstream {said} moved (301)"))),
            (status, _) => {
                return Err(refused(format!(
                    "the upstream {said} is not readable with the App's token ({status})"
                )));
            }
        };
        if value["id"] != json!(pinned) {
            return Err(refused(format!(
                "the upstream {said} is repository {}, not the {pinned} upstream_id pins: nothing \
                 is imported from another",
                value["id"]
            )));
        }
        if pinned == id {
            return Err(refused(
                "upstream_id is the repository delivered to: an import is of another",
            ));
        }
        let (_, ours) = host.get("")?;
        match (value["private"].as_bool(), ours["private"].as_bool()) {
            (Some(false), _) | (Some(true), Some(true)) => {}
            (Some(true), Some(false)) => {
                return Err(refused(format!(
                    "the upstream {said} is private and {} is not: an import would publish its \
                     history",
                    self.repository.full_name()
                )));
            }
            _ => {
                return Err(refused(
                    "whether the upstream or this repository is private is not shown: an import \
                     is made only where nothing private is published",
                ));
            }
        }
        Ok(json!({"id": pinned, "name": clean_title(&said)}))
    }

    /// The pull request of the branch `slug` into `base`: the one bound, the open one found by
    /// its head, or a new one.
    fn pull(&mut self, host: &Host<'_>, snapshot: &Snapshot, base: &str) -> Result<Pull, Stopped> {
        let slug = &snapshot.slug;
        if let Some(kept) = self
            .state
            .pulls
            .get(slug)
            .filter(|kept| kept.task == snapshot.task)
        {
            return Ok(kept.clone());
        }
        let listed = host.read(
            &format!(
                "/pulls?state=open&head={}:{slug}&base={base}&per_page=100",
                self.repository.owner
            ),
            "the pull requests of the branch",
        )?;
        let found = (listed.as_array().into_iter().flatten()).find(|pull| {
            pull["head"]["ref"] == json!(slug)
                && pull["base"]["ref"] == json!(base)
                && pull["head"]["repo"]["id"] == pull["base"]["repo"]["id"]
        });
        let made = if let Some(found) = found {
            found.clone()
        } else {
            {
                let (title, body) = match self.state.drafts.get(slug) {
                    Some(draft) => (draft.title.clone(), draft.body.clone()),
                    None => (slug.clone(), format!("The Symphony task `{slug}`.")),
                };
                let id = crate::digest(&["pull", slug, base]);
                self.journal(Event::Intent, "pull", slug, &id, Value::Null);
                // Ready for review: GitHub arms no auto-merge on a draft. Nothing merges it
                // before its evidence stands, as the rulesets require `symphony/eligible` from
                // this App alone, and that check is pending or failing until then.
                let sent = json!({
                    "title": title, "body": body, "head": slug, "base": base, "draft": false,
                });
                match host.send("POST", host.path("/pulls"), Some(&sent))? {
                    Ok((201, made)) => made,
                    Ok((status, said)) => {
                        let why = clean_title(said["message"].as_str().unwrap_or_default());
                        self.journal(Event::Failed, "pull", slug, &id, json!(status));
                        return Err(refused(format!(
                            "GitHub opened no pull request of {slug} ({status}): {why}"
                        )));
                    }
                    Err(Lost(why)) => {
                        self.journal(Event::Unknown, "pull", slug, &id, json!(why));
                        return Err(unavailable(format!(
                            "whether the pull request of {slug} was opened is not known ({why}): \
                             the next run looks for it first"
                        )));
                    }
                }
            }
        };
        let number = (made["number"].as_i64())
            .ok_or_else(|| unavailable("the pull request has no number"))?;
        let pull = Pull {
            number,
            url: made["html_url"].as_str().unwrap_or_default().into(),
            task: snapshot.task,
            landed: None,
        };
        self.journal(Event::Ack, "pull", slug, &number.to_string(), json!(number));
        self.state.pulls.insert(slug.clone(), pull.clone());
        self.store.save(&self.state).map_err(unavailable)?;
        Ok(pull)
    }

    /// The checks of `snapshot` on its head, written where they do not stand and read back.
    fn stand(&self, snapshot: &Snapshot, repository: i64) -> Result<(), Stopped> {
        let (head, slug) = (snapshot.head.as_str(), snapshot.slug.as_str());
        let app = self.config.app_id.unwrap_or_default();
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let listing = format!("/commits/{head}/check-runs?per_page=100");
        let ours = |listed: &Value| -> Result<Vec<Value>, Stopped> {
            let runs = listed["check_runs"].as_array().cloned().unwrap_or_default();
            if listed["total_count"].as_u64().unwrap_or(0) > runs.len() as u64 {
                return Err(unavailable(format!(
                    "{head} has more checks than one page lists: ours are not told apart"
                )));
            }
            Ok((runs.into_iter())
                .filter(|run| run["app"]["id"] == json!(app))
                .collect())
        };
        // What GitHub goes by is the newest run of each name.
        let standing = |runs: &[Value], check: &Check| {
            newest(runs, &check.name).is_some_and(|run| check.stands(run, head))
        };
        let before = ours(&host.read(&listing, "the checks of the head")?)?;
        let wanted = checks(snapshot, repository, withdrawn(&before)).map_err(refused)?;
        for check in wanted.iter().filter(|check| !standing(&before, check)) {
            let sent = check.sent(head);
            // The newest of that name by us is rewritten, so each name is said once on a head;
            // one that completed is not made pending again but followed by a new run, which
            // GitHub goes by in its place.
            let old = newest(&before, &check.name)
                .filter(|run| check.conclusion.is_some() || run["status"] != json!("completed"));
            let (method, path) = match old.and_then(|run| run["id"].as_i64()) {
                Some(id) => ("PATCH", host.path(&format!("/check-runs/{id}"))),
                None => ("POST", host.path("/check-runs")),
            };
            self.journal(Event::Intent, "check", slug, &check.id, json!(check.name));
            match host.send(method, path, Some(&sent))? {
                Ok((200 | 201, _)) => {
                    self.journal(Event::Ack, "check", slug, &check.id, json!(check.said()));
                }
                Ok((status, _)) => {
                    self.journal(Event::Failed, "check", slug, &check.id, json!(status));
                    return Err(refused(format!(
                        "GitHub took no check {} of {slug} ({status}): the checks' token is the \
                         App's, with checks: write",
                        check.name
                    )));
                }
                Err(Lost(why)) => {
                    self.journal(Event::Unknown, "check", slug, &check.id, json!(why));
                    return Err(unavailable(format!(
                        "whether the check {} was written is not known ({why}): the next run \
                         looks for it first",
                        check.name
                    )));
                }
            }
        }
        // GitHub lists a check a little after it takes it: one not listed yet is read again,
        // after each of READ_BACK's rests, and nothing is written again.
        let mut rests = READ_BACK.iter();
        loop {
            let after = ours(&host.read(&listing, "the checks of the head")?)?;
            let Some(check) = wanted.iter().find(|check| !standing(&after, check)) else {
                return Ok(());
            };
            let Some(wait) = rests.next() else {
                return Err(unavailable(format!(
                    "the check {} does not read back on {head} as App {app} wrote it, read again \
                     for {} s",
                    check.name,
                    READ_BACK.iter().sum::<Duration>().as_secs()
                )));
            };
            (self.ctx.rest)(*wait);
        }
    }

    fn publish(&mut self, remote: &dyn Remote, request: &Value) -> Result<Value, Stopped> {
        let snapshot: Snapshot = serde_json::from_value(request["snapshot"].clone())
            .map_err(|error| refused(format!("the evidence does not read: {error}")))?;
        let (head, base) = (text(request, "head"), text(request, "branch"));
        if snapshot.version != SNAPSHOT {
            return Err(refused(format!(
                "the evidence is of shape {}, and this build writes checks of shape {SNAPSHOT}",
                snapshot.version
            )));
        }
        if snapshot.head != head || snapshot.slug != text(request, "slug") {
            return Err(refused(
                "the evidence is of another task or head than the one asked for",
            ));
        }
        if !commit(&head) || !plain(&snapshot.slug, "-._/") || !plain(&base, "-._/") {
            return Err(refused(
                "the head is a whole commit, and the branches plain names",
            ));
        }
        // Refused before anything is pushed, where a stamp names no check, or where the evidence
        // that stands is of another operation than the one asked for.
        checks(&snapshot, 0, false).map_err(refused)?;
        let named = (snapshot.assessment.as_ref())
            .filter(|read| read.tip.as_deref() == Some(head.as_str()))
            .and_then(|read| read.import.clone());
        if snapshot.eligible && named != import(request).map(|(_, commit)| commit) {
            return Err(refused(
                "the evidence names another import, or none, than the operation asked for",
            ));
        }
        let origin = remote.url().map_err(refused)?;
        if !names(&origin, &self.repository) {
            return Err(refused(format!(
                "origin is not {} on GitHub.com: nothing is pushed anywhere else",
                self.repository.full_name()
            )));
        }
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (id, name) = self.bound(&host)?;
        // Evidence that stands is made public only where the branch is held to it. Evidence
        // that blocks takes nothing from any protection, so it is still said where the policy
        // does not hold, and by its checks alone.
        if let Err(stopped) = self.protected(&host, id, &base, &snapshot.slug) {
            if snapshot.eligible || stopped["kind"] != json!("policy") {
                return Err(stopped);
            }
            return self.blocked(&host, &snapshot, id, &name, stopped);
        }
        // An import's head carries its upstream's history: pushed only where the upstream is
        // the one pinned, and only where that publishes nothing private.
        let upstream = match import(request) {
            Some(_) => self.upstream(&host, request, id)?,
            None => Value::Null,
        };
        let before = self.before(&host, &snapshot)?;
        (remote.push(&head, &snapshot.slug)).map_err(|why| {
            refused(format!(
                "{} was not pushed to the branch {} of {name}: {}",
                short(&head),
                snapshot.slug,
                clean_title(&why)
            ))
        })?;
        let pull = self.pull(&host, &snapshot, &base)?;
        let now = host.read(&format!("/pulls/{}", pull.number), "the pull request")?;
        if now["state"] != json!("open") {
            return Err(refused(format!(
                "pull request #{} of {name} is {}: nothing more is published to it",
                pull.number,
                if now["merged"] == json!(true) {
                    "merged"
                } else {
                    "closed"
                }
            )));
        }
        if now["base"]["repo"]["id"] != json!(id)
            || now["head"]["ref"] != json!(snapshot.slug)
            || now["base"]["ref"] != json!(base)
        {
            return Err(refused(format!(
                "pull request #{} is not {}'s into {base} in {name}",
                pull.number, snapshot.slug
            )));
        }
        // One opened as a draft by an earlier build is made ready, which auto-merge needs.
        if now["draft"] == json!(true) {
            self.ready(&host, &snapshot.slug, &pull, &now)?;
        }
        self.stand(&snapshot, id)?;
        let at = self.moved(&host, &pull, &head, before, &now["head"]["sha"])?;
        let mut published = json!({
            "state": "published",
            "repository": {"id": id, "name": name},
            "pull": {"number": pull.number, "url": pull.url},
            "head": at,
        });
        if !upstream.is_null() {
            published["upstream"] = upstream;
        }
        Ok(published)
    }

    /// The head the pull request bound to the task of `snapshot` already is at, read before a
    /// push to its branch, which GitHub moves it from a little after; none where none is bound.
    fn before(&self, host: &Host<'_>, snapshot: &Snapshot) -> Result<Option<Value>, Stopped> {
        let kept = (self.state.pulls.get(&snapshot.slug)).filter(|kept| kept.task == snapshot.task);
        let Some(kept) = kept else {
            return Ok(None);
        };
        let was = host.read(&format!("/pulls/{}", kept.number), "the pull request")?;
        Ok(Some(was["head"]["sha"].clone()))
    }

    /// The head pull request `pull` is at once GitHub moved it to the `head` pushed to its branch,
    /// which it does a few seconds after the push: one read at `first` after it, where that is
    /// the head it was at `before` (with none read, as for one not bound yet, whatever `first`
    /// is), is read again after each of [`READ_BACK`]'s rests while it is still there. A head
    /// that is neither, as someone else pushed, is answered as it is, for the land to refuse.
    fn moved(
        &self,
        host: &Host<'_>,
        pull: &Pull,
        head: &str,
        before: Option<Value>,
        first: &Value,
    ) -> Result<Value, Stopped> {
        let before = before.unwrap_or_else(|| first.clone());
        let mut rests = READ_BACK.iter();
        let mut at = first.clone();
        while at == before && at != json!(head) {
            let Some(wait) = rests.next() else {
                return Err(unavailable(format!(
                    "pull request #{} is still at {}, not the head {} pushed to its branch, read \
                     again for {} s: the next run reads it first",
                    pull.number,
                    at.as_str().map_or_else(|| "no commit".into(), short),
                    short(head),
                    READ_BACK.iter().sum::<Duration>().as_secs()
                )));
            };
            (self.ctx.rest)(*wait);
            let now = host.read(&format!("/pulls/{}", pull.number), "the pull request")?;
            at = now["head"]["sha"].clone();
        }
        Ok(at)
    }

    /// Evidence that blocks, said on a branch whose policy does not hold: its checks written on
    /// the pull request already bound to the task, at the head that pull request is at, and
    /// nothing else. No head is pushed and no pull request opened past the policy: with none
    /// bound, or one at another head, the policy's refusal `stopped` is the answer.
    fn blocked(
        &self,
        host: &Host<'_>,
        snapshot: &Snapshot,
        id: i64,
        name: &str,
        stopped: Stopped,
    ) -> Result<Value, Stopped> {
        let kept = (self.state.pulls.get(&snapshot.slug)).filter(|kept| kept.task == snapshot.task);
        let Some(pull) = kept else {
            return Err(stopped);
        };
        let now = host.read(&format!("/pulls/{}", pull.number), "the pull request")?;
        if now["state"] != json!("open")
            || now["base"]["repo"]["id"] != json!(id)
            || now["head"]["sha"] != json!(snapshot.head)
        {
            return Err(stopped);
        }
        self.stand(snapshot, id)?;
        Ok(json!({
            "state": "published",
            "repository": {"id": id, "name": name},
            "pull": {"number": pull.number, "url": pull.url},
            "head": now["head"]["sha"],
        }))
    }

    /// The pull request the request names, which must be the one bound to its task here.
    fn asked(&mut self, host: &Host<'_>, request: &Value) -> Result<(Pull, Value), Stopped> {
        let slug = text(request, "slug");
        let (id, name) = self.bound(host)?;
        let pull = (self.state.pulls.get(&slug).cloned())
            .ok_or_else(|| refused(format!("{slug} is bound to no pull request here")))?;
        if request["repository"] != json!(id) || request["pull"] != json!(pull.number) {
            return Err(refused(format!(
                "{slug} is bound to pull request #{} of {name} (id {id}), not the one asked for",
                pull.number
            )));
        }
        let now = host.read(&format!("/pulls/{}", pull.number), "the pull request")?;
        Ok((pull, now))
    }

    /// The draft pull request `pull` marked ready for review, which GitHub's REST API has no
    /// call for: one GraphQL mutation naming it by its node.
    fn ready(&self, host: &Host<'_>, slug: &str, pull: &Pull, now: &Value) -> Result<(), Stopped> {
        let node = now["node_id"].as_str().unwrap_or_default();
        let id = crate::digest(&["ready", slug, node]);
        let sent = json!({
            "query": "mutation($id: ID!) { markPullRequestReadyForReview(input: \
                      {pullRequestId: $id}) { pullRequest { isDraft } } }",
            "variables": {"id": node},
        });
        self.journal(Event::Intent, "ready", slug, &id, json!(pull.number));
        let made = host.send("POST", "/graphql".into(), Some(&sent))?;
        match made {
            Ok((200, said)) if said["errors"].is_null() => {
                self.journal(Event::Ack, "ready", slug, &id, json!(pull.number));
                Ok(())
            }
            Ok((status, said)) => {
                self.journal(Event::Failed, "ready", slug, &id, json!(status));
                let why = refusal(&said);
                Err(json!({
                    "state": "refused", "kind": "permission",
                    "why": format!(
                        "GitHub left pull request #{} a draft ({status}): {why}",
                        pull.number
                    ),
                }))
            }
            Err(Lost(why)) => {
                self.journal(Event::Unknown, "ready", slug, &id, json!(why));
                Err(unavailable(format!(
                    "whether pull request #{} was marked ready is not known ({why}): nothing was \
                     merged, and the next run reads it first",
                    pull.number
                )))
            }
        }
    }

    /// What a merge and an arming both read again before GitHub is asked to merge the pull
    /// request `pull` of `request`, as GitHub shows it `now`: the base branch it names, still
    /// protected for this delivery; an import's upstream, still the one pinned; and a draft marked
    /// ready, the one write here.
    fn mergeable_at(
        &self,
        host: &Host<'_>,
        request: &Value,
        pull: &Pull,
        now: &Value,
    ) -> Result<(), Stopped> {
        let slug = text(request, "slug");
        let id = request["repository"].as_i64().unwrap_or_default();
        let base = now["base"]["ref"].as_str().unwrap_or_default().to_string();
        if !plain(&base, "-._/") {
            return Err(refused("the pull request names no base branch"));
        }
        self.protected(host, id, &base, &slug)?;
        if import(request).is_some() {
            self.upstream(host, request, id)?;
        }
        if now["draft"] == json!(true) {
            self.ready(host, &slug, pull, now)?;
        }
        Ok(())
    }

    fn merge(&mut self, request: &Value) -> Result<Value, Stopped> {
        let (slug, head) = (text(request, "slug"), text(request, "head"));
        let imported = import(request);
        let method = merge_method(request, &head, imported.is_some())?;
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (pull, now) = self.asked(&host, request)?;
        if now["merged"] == json!(true) {
            return Ok(json!({"state": "merged", "sha": now["merge_commit_sha"]}));
        }
        let kind = |kind: &str, why: String| json!({"state": "refused", "kind": kind, "why": why});
        if now["state"] != json!("open") {
            return Ok(kind(
                "closed",
                format!("pull request #{} is closed", pull.number),
            ));
        }
        if now["head"]["sha"] != json!(head) {
            return Ok(kind(
                "head",
                format!(
                    "pull request #{} is at another head than {}",
                    pull.number,
                    short(&head)
                ),
            ));
        }
        // Read again here: the policy that stood when the head was published may be gone, and
        // from this step on the pull request is ready and its merge asked for.
        self.mergeable_at(&host, request, &pull, &now)?;
        let (title, message) = self.commit_text(&slug, &head, &pull, imported.as_ref());
        let sent = json!({
            "sha": head, "merge_method": method, "commit_title": title, "commit_message": message,
        });
        let id = crate::digest(&["merge", &slug, &head]);
        self.journal(Event::Intent, "merge", &slug, &id, json!(pull.number));
        let path = host.path(&format!("/pulls/{}/merge", pull.number));
        match host.send("PUT", path, Some(&sent))? {
            Ok((200, said)) if said["merged"] == json!(true) => {
                self.journal(Event::Ack, "merge", &slug, &id, said["sha"].clone());
                Ok(json!({"state": "merged", "sha": said["sha"]}))
            }
            Ok((status, said)) => {
                self.journal(Event::Failed, "merge", &slug, &id, json!(status));
                let why = format!(
                    "GitHub merged nothing ({status}): {}",
                    clean_title(said["message"].as_str().unwrap_or_default())
                );
                Ok(kind(
                    match status {
                        409 => "head",
                        403 | 404 => "permission",
                        _ if now["mergeable_state"] == json!("dirty") => "conflict",
                        _ => "checks",
                    },
                    why,
                ))
            }
            Err(Lost(why)) => {
                self.journal(Event::Unknown, "merge", &slug, &id, json!(why));
                Ok(json!({"state": "unknown", "why": why}))
            }
        }
    }

    fn result(&mut self, request: &Value) -> Result<Value, Stopped> {
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (_, now) = self.asked(&host, request)?;
        let merged = now["merged"] == json!(true);
        let open = !merged && now["state"] == json!("open");
        let foreign = match now["head"]["sha"].as_str() {
            Some(head) if open && commit(head) => self.foreign(&host, head)?,
            _ => Vec::new(),
        };
        Ok(json!({
            "state": if merged { "merged" } else if open { "open" } else { "closed" },
            "head": now["head"]["sha"],
            "sha": if merged { now["merge_commit_sha"].clone() } else { Value::Null },
            "base": now["base"]["ref"],
            "repository": now["base"]["repo"]["id"],
            "auto_merge": now["auto_merge"].is_object(),
            "mergeable_state": now["mergeable_state"],
            "foreign": foreign,
        }))
    }

    /// Each check run on `head` another App wrote whose newest run ended in one of [`FAILED`],
    /// as `{"name", "conclusion"}`, every page read. Commit statuses are not read: the App's
    /// token is not given that permission.
    fn foreign(&self, host: &Host<'_>, head: &str) -> Result<Vec<Value>, Stopped> {
        use crate::preflight::{MAX_PAGES, PER_PAGE};
        let app = json!(self.config.app_id.unwrap_or_default());
        let mut runs = Vec::new();
        for page in 1..=MAX_PAGES {
            let listed = host.read(
                &format!("/commits/{head}/check-runs?per_page={PER_PAGE}&page={page}"),
                "the checks of the head",
            )?;
            let listed_runs = listed["check_runs"].as_array().cloned().unwrap_or_default();
            let total = listed["total_count"].as_u64().unwrap_or(0);
            let done = listed_runs.len() < PER_PAGE;
            runs.extend(listed_runs);
            if done || runs.len() as u64 >= total {
                return Ok((runs.iter())
                    .filter(|run| {
                        run["app"]["id"] != app
                            && (run["conclusion"].as_str())
                                .is_some_and(|said| FAILED.contains(&said))
                    })
                    .map(|run| json!({"name": run["name"], "conclusion": run["conclusion"]}))
                    .collect());
            }
        }
        Err(unavailable(format!(
            "{head} has more than {MAX_PAGES} pages of checks: the rest are not read"
        )))
    }

    /// The commit's title and message for `slug`'s head `head` by pull request `pull`, as
    /// `merge` and `arm` ask for: an import's titled as a land's own merge is, which the release
    /// cadence counts; a task's by its curated title where it has one, else its slug.
    fn commit_text(
        &self,
        slug: &str,
        head: &str,
        pull: &Pull,
        imported: Option<&(String, String)>,
    ) -> (String, String) {
        match (imported, self.state.drafts.get(slug)) {
            (Some((_, commit)), _) => (
                format!("Merge branch '{slug}'"),
                format!(
                    "Merged from {head} by pull request #{}, an import of {commit}.",
                    pull.number
                ),
            ),
            (None, Some(draft)) => (
                format!("{slug}: {}", draft.title),
                format!("Squashed from {head} by pull request #{}.", pull.number),
            ),
            (None, None) => (
                slug.to_string(),
                format!("Squashed from {head} by pull request #{}.", pull.number),
            ),
        }
    }

    /// One GraphQL mutation on pull request `now` (its node), journaled as `kind`.
    fn mutate(
        &self,
        host: &Host<'_>,
        kind: &str,
        slug: &str,
        now: &Value,
        query: &str,
        mut variables: Value,
    ) -> Result<Mutated, Stopped> {
        let node = now["node_id"].as_str().unwrap_or_default();
        let head = now["head"]["sha"].as_str().unwrap_or_default();
        let id = crate::digest(&[kind, slug, node, head]);
        variables["id"] = json!(node);
        let sent = json!({"query": query, "variables": variables});
        self.journal(Event::Intent, kind, slug, &id, now["number"].clone());
        Ok(match host.send("POST", "/graphql".into(), Some(&sent))? {
            Ok((200, said)) if said["errors"].is_null() => {
                self.journal(Event::Ack, kind, slug, &id, now["number"].clone());
                Mutated::Made
            }
            Ok((status, said)) => {
                self.journal(Event::Failed, kind, slug, &id, json!(status));
                Mutated::Refused(status, refusal(&said))
            }
            Err(Lost(why)) => {
                self.journal(Event::Unknown, kind, slug, &id, json!(why));
                Mutated::Lost(why)
            }
        })
    }

    /// GitHub's auto-merge turned on for the bound pull request at the request's head, by the
    /// method and text a merge of it is asked by ([`Run::merge`]): it merges once every check
    /// the branch requires passes. One on already by that method and text is left as it is; one
    /// on by another (armed at an earlier head, or before its curated text was written) is
    /// turned off and on again, as GitHub merges by the text it was armed with. A pull request
    /// GitHub can merge already takes none and is answered `mergeable`, for the land to ask the
    /// merge itself.
    fn arm(&mut self, request: &Value) -> Result<Value, Stopped> {
        let (slug, head) = (text(request, "slug"), text(request, "head"));
        let imported = import(request);
        let method = merge_method(request, &head, imported.is_some())?;
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (pull, now) = self.asked(&host, request)?;
        let kind = |kind: &str, why: String| json!({"state": "refused", "kind": kind, "why": why});
        if now["merged"] == json!(true) {
            return Ok(json!({"state": "merged", "sha": now["merge_commit_sha"]}));
        }
        if now["state"] != json!("open") {
            return Ok(json!({"state": "closed"}));
        }
        if now["head"]["sha"] != json!(head) {
            return Ok(kind(
                "head",
                format!(
                    "pull request #{} is at another head than {}",
                    pull.number,
                    short(&head)
                ),
            ));
        }
        // Read as a merge reads it: from here GitHub merges by itself once the checks pass.
        self.mergeable_at(&host, request, &pull, &now)?;
        let armed = json!({"state": "armed", "pull": pull.number, "head": head});
        let (title, message) = self.commit_text(&slug, &head, &pull, imported.as_ref());
        let on = &now["auto_merge"];
        if on.is_object() {
            // GitHub merges by the text it was armed with, which names the head and the curated
            // title of that time: on by this method and this text, it is left as it is.
            if on["merge_method"] == json!(method)
                && on["commit_title"] == json!(title)
                && on["commit_message"] == json!(message)
            {
                return Ok(armed);
            }
            // By another: turned off, then on again below. One turned off whose arming fails
            // or is lost is left off, which the next run reads and arms.
            let off = self.unarm(&host, &slug, &pull, &now)?;
            if off["state"] != json!("disarmed") {
                return Ok(off);
            }
        }
        let query = "mutation($id: ID!, $method: PullRequestMergeMethod!, $head: GitObjectID!, \
                     $headline: String!, $body: String!) { enablePullRequestAutoMerge(input: \
                     {pullRequestId: $id, mergeMethod: $method, expectedHeadOid: $head, \
                     commitHeadline: $headline, commitBody: $body}) { pullRequest { \
                     autoMergeRequest { enabledAt } } } }";
        let variables = json!({
            "method": method.to_uppercase(), "head": head, "headline": title, "body": message,
        });
        match self.mutate(&host, "arm", &slug, &now, query, variables)? {
            Mutated::Made => Ok(armed),
            Mutated::Refused(_, why)
                if why.contains("clean status") || why.contains("unstable status") =>
            {
                Ok(json!({"state": "mergeable", "pull": pull.number, "head": head}))
            }
            Mutated::Refused(status, why) => {
                let said = format!(
                    "GitHub armed no auto-merge of pull request #{} ({status}): {why}",
                    pull.number
                );
                Ok(kind(
                    if why.contains("conflict") || now["mergeable_state"] == json!("dirty") {
                        "conflict"
                    } else if why.to_lowercase().contains("head") {
                        "head"
                    } else {
                        "permission"
                    },
                    said,
                ))
            }
            Mutated::Lost(why) => Err(unavailable(format!(
                "whether auto-merge was armed on pull request #{} is not known ({why}): the next \
                 run reads it first",
                pull.number
            ))),
        }
    }

    /// Auto-merge turned off on `now`, the bound pull request as read: `disarmed`, or what GitHub
    /// made of it first.
    fn unarm(
        &self,
        host: &Host<'_>,
        slug: &str,
        pull: &Pull,
        now: &Value,
    ) -> Result<Value, Stopped> {
        if now["merged"] == json!(true) {
            return Ok(json!({"state": "merged", "sha": now["merge_commit_sha"]}));
        }
        if now["state"] != json!("open") {
            return Ok(json!({"state": "closed"}));
        }
        let disarmed = json!({"state": "disarmed"});
        if !now["auto_merge"].is_object() {
            return Ok(disarmed);
        }
        let query = "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: \
                     $id}) { pullRequest { autoMergeRequest { enabledAt } } } }";
        match self.mutate(host, "disarm", slug, now, query, json!({}))? {
            Mutated::Made => Ok(disarmed),
            Mutated::Refused(status, why) => match ended(host, pull)? {
                Some(ended) => Ok(ended),
                None => Ok(json!({
                    "state": "refused", "kind": "permission",
                    "why": format!(
                        "GitHub left auto-merge on for pull request #{} ({status}): {why}",
                        pull.number
                    ),
                })),
            },
            Mutated::Lost(why) => Err(unavailable(format!(
                "whether auto-merge was turned off on pull request #{} is not known ({why}): \
                 the next run reads it first",
                pull.number
            ))),
        }
    }

    /// GitHub's auto-merge of the bound pull request turned off, so no check that passes later
    /// merges it.
    fn disarm(&mut self, request: &Value) -> Result<Value, Stopped> {
        let slug = text(request, "slug");
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (pull, now) = self.asked(&host, request)?;
        self.unarm(&host, &slug, &pull, &now)
    }

    /// The bound pull request closed unmerged, its auto-merge turned off first and its branch
    /// left: a task that will not be delivered.
    fn close(&mut self, request: &Value) -> Result<Value, Stopped> {
        let slug = text(request, "slug");
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (pull, now) = self.asked(&host, request)?;
        let unarmed = self.unarm(&host, &slug, &pull, &now)?;
        if unarmed["state"] != json!("disarmed") {
            return Ok(unarmed);
        }
        let id = crate::digest(&["unmerged", &slug, &pull.number.to_string()]);
        self.journal(Event::Intent, "unmerged", &slug, &id, json!(pull.number));
        let path = host.path(&format!("/pulls/{}", pull.number));
        match host.send("PATCH", path, Some(&json!({"state": "closed"})))? {
            Ok((200, said)) if said["state"] == json!("closed") => {
                self.journal(Event::Ack, "unmerged", &slug, &id, json!(pull.number));
                Ok(json!({"state": "closed"}))
            }
            Ok((status, said)) => {
                self.journal(Event::Failed, "unmerged", &slug, &id, json!(status));
                match ended(&host, &pull)? {
                    Some(ended) => Ok(ended),
                    None => Ok(json!({
                        "state": "refused", "kind": "permission",
                        "why": format!(
                            "GitHub left pull request #{} open ({status}): {}",
                            pull.number,
                            clean_title(said["message"].as_str().unwrap_or_default())
                        ),
                    })),
                }
            }
            Err(Lost(why)) => {
                self.journal(Event::Unknown, "unmerged", &slug, &id, json!(why));
                Err(unavailable(format!(
                    "whether pull request #{} was closed is not known ({why}): the next run reads \
                     it first",
                    pull.number
                )))
            }
        }
    }

    fn landed(&mut self, request: &Value) -> Result<Value, Stopped> {
        let (slug, squash) = (text(request, "slug"), text(request, "squash"));
        if !commit(&squash) {
            return Err(refused("a task lands as one whole commit"));
        }
        let Some(pull) = self.state.pulls.get_mut(&slug) else {
            return Err(refused(format!("{slug} is bound to no pull request here")));
        };
        if pull.landed.as_deref() != Some(squash.as_str()) {
            pull.landed = Some(squash.clone());
            self.store.save(&self.state).map_err(unavailable)?;
        }
        let noted = |said: String| json!({"state": "noted", "said": said});
        let issue = self
            .state
            .mappings
            .get(&slug)
            .map(|mapping| mapping.issue_number);
        let Some(number) = issue.filter(|_| self.config.close_issues) else {
            return Ok(noted(String::new()));
        };
        let transport = self.connect()?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let now = host.read(&format!("/issues/{number}"), "the task's issue")?;
        if now["state"] == json!("closed") {
            return Ok(noted(format!("issue #{number} is closed")));
        }
        let id = crate::digest(&["close", &slug, &squash]);
        self.journal(Event::Intent, "close", &slug, &id, json!(number));
        let sent = json!({"state": "closed", "state_reason": "completed"});
        match host.send(
            "PATCH",
            host.path(&format!("/issues/{number}")),
            Some(&sent),
        )? {
            Ok((200, _)) => {
                self.journal(Event::Ack, "close", &slug, &id, json!(number));
                Ok(noted(format!("issue #{number} is closed")))
            }
            Ok((status, _)) => {
                self.journal(Event::Failed, "close", &slug, &id, json!(status));
                Ok(noted(format!("issue #{number} was not closed ({status})")))
            }
            Err(Lost(why)) => {
                self.journal(Event::Unknown, "close", &slug, &id, json!(why));
                Ok(noted(format!(
                    "whether issue #{number} was closed is not known"
                )))
            }
        }
    }
}

fn text(request: &Value, field: &str) -> String {
    request[field].as_str().unwrap_or_default().to_string()
}

/// The setting a delivery is turned on by, as the settings file writes it.
const PUBLISHES: &str = "pull_requests = \"publish\"";

/// Whether `config` has that setting.
fn publishes(config: &Config) -> bool {
    config.pull_requests == PullRequests::Publish
}

/// What a command of this program serves by: the committed settings, their revision and the
/// repository they name.
pub(crate) struct Serving {
    pub(crate) config: Config,
    pub(crate) revision: String,
    pub(crate) repository: Repository,
}

/// The committed settings a command serves by: read, enabled, naming a repository, and with the
/// command's own setting, which `on` reads and `setting` writes as the file does. Else why the
/// command is turned off, the line each command says.
pub(crate) fn served(
    ctx: &Context<'_>,
    setting: &str,
    on: impl FnOnce(&Config) -> bool,
) -> Result<Serving, String> {
    let loaded = (ctx.source.load())
        .map_err(|why| format!("turned off: its settings do not read: {why}"))?;
    let config = loaded.config;
    let serves = config.enabled && on(&config);
    let Some(repository) = config.repository.clone().filter(|_| serves) else {
        return Err(format!(
            "turned off: the committed settings do not say enabled = true and {setting}"
        ));
    };
    Ok(Serving {
        config,
        revision: loaded.revision,
        repository,
    })
}

/// The state kept here for a command that writes it: its store, made if it is not there, the
/// lock held while the command runs, and the state as it reads, new where none was kept. Else why
/// it is unavailable.
pub(crate) fn kept(ctx: &Context<'_>) -> Result<(Store, Locked, State), String> {
    let folder = (ctx.board.state_folder()).map_err(|error| error.said().to_string())?;
    let store = Store::open(&folder)?;
    let lock = store.lock()?;
    let state = store.state()?.unwrap_or_default();
    Ok((store, lock, state))
}

/// The answer to `verb` of `request`.
fn answer(ctx: &Context<'_>, remote: &dyn Remote, verb: &str, request: &Value) -> Value {
    let serving = match served(ctx, PUBLISHES, publishes) {
        Ok(serving) => serving,
        Err(why) => return refused(why),
    };
    let (store, _lock, state) = match kept(ctx) {
        Ok(kept) => kept,
        Err(why) => return unavailable(why),
    };
    let mut run = Run {
        ctx,
        config: serving.config,
        revision: serving.revision,
        repository: serving.repository,
        store,
        state,
        by: clean_title(&text(request, "by")),
    };
    let answered = match verb {
        "publish" => run.publish(remote, request),
        "arm" => run.arm(request),
        "merge" => run.merge(request),
        "disarm" => run.disarm(request),
        "close" => run.close(request),
        "result" => run.result(request),
        "landed" => run.landed(request),
        other => Err(refused(format!("no verb {other:?}"))),
    };
    answered.unwrap_or_else(|stopped| stopped)
}

/// `deliver VERB --request FILE`: the answer on `out`, one line of JSON.
///
/// # Errors
///
/// The request does not read: nothing was asked of anyone.
pub fn run(
    ctx: &Context<'_>,
    remote: &dyn Remote,
    verb: &str,
    file: &Path,
    out: &mut dyn Write,
) -> Outcome {
    let said = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let request: Value =
        serde_json::from_str(&said).map_err(|e| format!("{}: {e}", file.display()))?;
    if !request.is_object() {
        return Err(format!("{}: a request is one JSON object", file.display()));
    }
    let _ = writeln!(out, "{}", answer(ctx, remote, verb, &request));
    Ok(0)
}

/// `preflight [--branch BASE]`: the protection of `base` read as a delivery reads it before it
/// writes, and said: that it holds, or each thing missing on a line of its own. It writes
/// nothing, here or there, and exits 0 only where a delivery would go on.
///
/// # Errors
///
/// The state kept here does not read.
pub fn preflight(ctx: &Context<'_>, base: &str, out: &mut dyn Write) -> Outcome {
    let stopped = |out: &mut dyn Write, answer: &Value| {
        let _ = writeln!(out, "{}", text(answer, "why"));
        Ok(1)
    };
    let Serving {
        config, repository, ..
    } = match served(ctx, PUBLISHES, publishes) {
        Ok(serving) => serving,
        Err(why) => {
            let _ = writeln!(out, "{why}");
            return Ok(1);
        }
    };
    if !plain(base, "-._/") {
        return Err(format!("no branch is named {base:?}"));
    }
    let folder = ctx.board.state_folder().map_err(|e| e.said().to_string())?;
    let kept = match Store::existing(&folder) {
        Some(store) => store.state()?,
        None => None,
    };
    let verified = (kept.as_ref())
        .map(|state| state.rulesets.clone())
        .unwrap_or_default();
    let bound = kept.and_then(|state| state.binding);
    let transport = match connect(ctx, &config) {
        Ok(transport) => transport,
        Err(answer) => return stopped(out, &answer),
    };
    let host = Host {
        transport: transport.as_ref(),
        repository: repository.clone(),
    };
    let id = match &bound {
        Some(binding) => binding.repository_id,
        None => match host.get("") {
            Ok((_, value)) => value["id"].as_i64().unwrap_or_default(),
            Err(answer) => return stopped(out, &answer),
        },
    };
    let app = config.app_id.unwrap_or_default();
    let _ = writeln!(
        out,
        "{} (id {id}{}), {base}: checks written and merged by App {app}, who else bypasses the \
         rules as verified",
        repository.full_name(),
        if bound.is_some() {
            ", as bound here"
        } else {
            ", bound to nothing here yet"
        },
    );
    let wanted = Wanted {
        repository: id,
        base,
        app,
        operation: Operation::of(config.upstream_id.is_some()),
    };
    match preflight::check(&host, &verified, &wanted) {
        Ok(policy) => {
            let _ = writeln!(
                out,
                "holds: a delivery would go on (policy {})",
                short(&policy.digest)
            );
            Ok(0)
        }
        Err(answer) => match answer["missing"].as_array() {
            Some(missing) => {
                let _ = writeln!(out, "does not hold: a delivery would be refused");
                for line in missing {
                    let _ = writeln!(out, "  missing: {}", line.as_str().unwrap_or_default());
                }
                Ok(1)
            }
            None => stopped(out, &answer),
        },
    }
}

#[cfg(test)]
#[path = "deliver_tests.rs"]
mod tests;
