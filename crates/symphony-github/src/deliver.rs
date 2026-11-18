//! `symphony-github deliver VERB --request FILE`: what answers a land delivered by a pull request
//! (`[land] delivery = ["symphony-github", "deliver"]`, docs/TASKS.md), one JSON object read and
//! one printed. It serves only `enabled` settings whose `pull_requests` is `publish`, and decides
//! nothing: what a task needs and whether it stands is the evidence the land hands it, which it
//! writes down as checks and never reads the board for.
//!
//! - `publish` pushes the head to the branch named as the task, never forced and only to an
//!   `origin` that names the repository the settings do; finds the pull request of that branch
//!   before it makes one; and writes one check for the assessment, one for each stamp and
//!   `symphony/eligible`, with the checks' own token, then reads them back as written by the one
//!   App the settings name. A check that stands is found by its name and what it says, so a
//!   second run writes none; `symphony/eligible` is written last when it passes and first when
//!   it fails, so it never passes over a stamp not yet written.
//! - `merge` asks GitHub once to squash the bound pull request at the head it is given
//!   (`sha`, which GitHub refuses a merge of any other head by), after reading that it is not
//!   merged already.
//! - `result` reads the bound pull request and writes nothing.
//! - `landed` records the commit, and closes the task's mapped issue where `close_issues` says.
//!
//! Every write is journaled before it is made, and what it would make is looked for first. The
//! pull request's public text is the task's curated text ([`crate::state::Draft`]) or, with
//! none, its slug: never a brief, a thread or the title the land hands over. Neither token is in
//! an argument, a URL, an environment variable, the journal, the state or a line said here.

use crate::config::{Config, PullRequests, Repository};
use crate::credential;
use crate::github::clean_title;
use crate::reconcile::{Context, Outcome};
use crate::state::{Binding, Entry, Event, Pull, State, Store};
use crate::transport::{Failure, Request, Transport};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The check that says every other stands: the one a branch's protection requires.
pub const ELIGIBLE: &str = "symphony/eligible";
/// The check that says an assessor read the head.
pub const ASSESSMENT: &str = "symphony/assessment";
/// The evidence's shape this build writes checks of (`symphony_board::land::evidence::VERSION`).
pub const SNAPSHOT: u64 = 1;

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

/// Whether the remote address `url` is `repository` on GitHub.com.
#[must_use]
pub fn names(url: &str, repository: &Repository) -> bool {
    let rest = [
        "https://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|start| url.strip_prefix(start));
    let Some(rest) = rest else {
        return false;
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    rest.eq_ignore_ascii_case(&repository.full_name())
}

/// An answer that made or found nothing, said for the land to carry.
fn refused(why: impl Into<String>) -> Value {
    json!({"state": "refused", "why": why.into()})
}

fn unavailable(why: impl Into<String>) -> Value {
    json!({"state": "unavailable", "why": why.into()})
}

/// A request that came to no answer the verb can go on by: the answer to give in its place.
type Stopped = Value;

/// The repository's calls, each answered with its status.
struct Host<'a> {
    transport: &'a dyn Transport,
    repository: Repository,
}

/// A write that may have been made.
struct Lost(String);

impl Host<'_> {
    fn path(&self, rest: &str) -> String {
        format!(
            "/repos/{}/{}{rest}",
            self.repository.owner, self.repository.name
        )
    }

    fn send(
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
        let limited = response.status == 429
            || (response.status == 403 && response.headers.contains_key("retry-after"));
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
    fn get(&self, rest: &str) -> Result<(u16, Value), Stopped> {
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
}

#[derive(Deserialize)]
struct Stamp {
    name: String,
    standing: String,
}

/// A check to stand on the head.
#[derive(Debug, PartialEq, Eq)]
struct Check {
    name: String,
    conclusion: &'static str,
    /// What it is found again by: the repository, the task, the head, its name, how it
    /// concluded and the evidence's revision.
    id: String,
    title: String,
}

fn plain(text: &str, more: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && !text.starts_with('-')
        && (text.chars()).all(|c| c.is_ascii_alphanumeric() || more.contains(c))
}

/// The start of a commit's name, as a person reads it.
fn short(commit: &str) -> String {
    commit.chars().take(7).collect()
}

fn commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// The checks of `snapshot` on repository `repository`, in the order they are written.
fn checks(snapshot: &Snapshot, repository: i64) -> Result<Vec<Check>, String> {
    let check = |name: String, conclusion: &'static str, title: String| Check {
        id: crate::digest(&[
            &repository.to_string(),
            &snapshot.task.to_string(),
            &snapshot.head,
            &name,
            conclusion,
            &snapshot.revision,
        ]),
        name,
        conclusion,
        title,
    };
    let mut wanted = vec![check(
        ASSESSMENT.into(),
        if snapshot.assessed {
            "success"
        } else {
            "failure"
        },
        if snapshot.assessed {
            "An assessor read this commit".into()
        } else {
            "No assessment of this commit".into()
        },
    )];
    for stamp in &snapshot.stamps {
        if !plain(&stamp.name, "-_") {
            return Err(format!("no check is named for the stamp {:?}", stamp.name));
        }
        let (conclusion, said) = match stamp.standing.as_str() {
            "met" => ("success", "stands at this commit"),
            "waived" => ("neutral", "is waived by the assessment of this commit"),
            "missing" => ("failure", "does not stand at this commit"),
            other => return Err(format!("no check says a stamp is {other:?}")),
        };
        wanted.push(check(
            format!("symphony/{}", stamp.name),
            conclusion,
            format!("{} {said}", stamp.name),
        ));
    }
    let eligible = check(
        ELIGIBLE.into(),
        if snapshot.eligible {
            "success"
        } else {
            "failure"
        },
        if snapshot.eligible {
            "Every stamp the task needs stands at this commit".into()
        } else {
            "The task is not eligible at this commit".into()
        },
    );
    // Never passing over a stamp not written yet, never failing after the stamps moved.
    if snapshot.eligible {
        wanted.push(eligible);
    } else {
        wanted.insert(0, eligible);
    }
    Ok(wanted)
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

    fn connect(&self, file: Option<&Path>, key: &str) -> Result<Box<dyn Transport>, Stopped> {
        let file = file.ok_or_else(|| refused(format!("the settings name no {key}")))?;
        let folders: Vec<&Path> = self.ctx.repository.iter().map(PathBuf::as_path).collect();
        let token = credential::read(file, &folders).map_err(refused)?;
        (self.ctx.connect)(token).map_err(unavailable)
    }

    /// The repository as GitHub names it now, which must be the one the state is bound to.
    fn bound(&mut self, host: &Host<'_>) -> Result<(i64, String), Stopped> {
        let value = match host.get("")? {
            (200, value) => value,
            (301, _) => return Err(refused("the repository moved to another name (301)")),
            (status, _) => {
                return Err(refused(format!(
                    "{} is missing or not accessible ({status})",
                    self.repository.full_name()
                )));
            }
        };
        let id = (value["id"].as_i64()).ok_or_else(|| unavailable("the repository has no id"))?;
        let name = clean_title(value["full_name"].as_str().unwrap_or_default());
        match &self.state.binding {
            Some(binding) if binding.repository_id != id => Err(refused(format!(
                "{name} is repository {id}, and what is kept here is bound to {} (id {}): \
                 nothing is published to another repository",
                binding.full_name, binding.repository_id
            ))),
            Some(_) => Ok((id, name)),
            None => {
                self.state.binding = Some(Binding {
                    repository_id: id,
                    node_id: value["node_id"].as_str().unwrap_or_default().into(),
                    full_name: name.clone(),
                });
                self.store.save(&self.state).map_err(unavailable)?;
                Ok((id, name))
            }
        }
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
                let sent = json!({"title": title, "body": body, "head": slug, "base": base});
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

    /// The checks of `wanted` on `head`, written where they do not stand and read back.
    fn stand(&self, wanted: &[Check], head: &str, slug: &str) -> Result<(), Stopped> {
        let app = self.config.checks_app_id.unwrap_or_default();
        let transport = self.connect(
            self.config.checks_token_file.as_deref(),
            "checks_token_file",
        )?;
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
        let standing = |runs: &[Value], check: &Check| {
            runs.iter().any(|run| {
                run["name"] == json!(check.name)
                    && run["external_id"] == json!(check.id)
                    && run["conclusion"] == json!(check.conclusion)
                    && run["head_sha"] == json!(head)
            })
        };
        let before = ours(&host.read(&listing, "the checks of the head")?)?;
        for check in wanted.iter().filter(|check| !standing(&before, check)) {
            let sent = json!({
                "name": check.name, "head_sha": head, "status": "completed",
                "conclusion": check.conclusion, "external_id": check.id,
                "output": {"title": check.title, "summary": format!("{} at `{head}`.", check.title)},
            });
            // One of that name by us is rewritten, so each name is said once on a head.
            let old = (before.iter()).find(|run| run["name"] == json!(check.name));
            let (method, path) = match old.and_then(|run| run["id"].as_i64()) {
                Some(id) => ("PATCH", host.path(&format!("/check-runs/{id}"))),
                None => ("POST", host.path("/check-runs")),
            };
            self.journal(Event::Intent, "check", slug, &check.id, json!(check.name));
            match host.send(method, path, Some(&sent))? {
                Ok((200 | 201, _)) => {
                    self.journal(
                        Event::Ack,
                        "check",
                        slug,
                        &check.id,
                        json!(check.conclusion),
                    );
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
        let after = ours(&host.read(&listing, "the checks of the head")?)?;
        match wanted.iter().find(|check| !standing(&after, check)) {
            Some(check) => Err(unavailable(format!(
                "the check {} does not read back on {head} as App {app} wrote it",
                check.name
            ))),
            None => Ok(()),
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
        // Refused before anything is pushed, where a stamp names no check.
        checks(&snapshot, 0).map_err(refused)?;
        let origin = remote.url().map_err(refused)?;
        if !names(&origin, &self.repository) {
            return Err(refused(format!(
                "origin is not {} on GitHub.com: nothing is pushed anywhere else",
                self.repository.full_name()
            )));
        }
        let transport = self.connect(self.config.token_file.as_deref(), "token_file")?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (id, name) = self.bound(&host)?;
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
        let wanted = checks(&snapshot, id).map_err(refused)?;
        self.stand(&wanted, &head, &snapshot.slug)?;
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

    fn merge(&mut self, request: &Value) -> Result<Value, Stopped> {
        let (slug, head) = (text(request, "slug"), text(request, "head"));
        if !commit(&head) || request["method"] != json!("squash") {
            return Err(refused("a merge is a squash of one whole commit"));
        }
        let transport = self.connect(self.config.token_file.as_deref(), "token_file")?;
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
        let title = match self.state.drafts.get(&slug) {
            Some(draft) => format!("{slug}: {}", draft.title),
            None => slug.clone(),
        };
        let sent = json!({
            "sha": head, "merge_method": "squash", "commit_title": title,
            "commit_message": format!("Squashed from {head} by pull request #{}.", pull.number),
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
        let transport = self.connect(self.config.token_file.as_deref(), "token_file")?;
        let host = Host {
            transport: transport.as_ref(),
            repository: self.repository.clone(),
        };
        let (_, now) = self.asked(&host, request)?;
        let merged = now["merged"] == json!(true);
        Ok(json!({
            "state": if merged { "merged" } else if now["state"] == json!("open") { "open" } else { "closed" },
            "head": now["head"]["sha"],
            "sha": if merged { now["merge_commit_sha"].clone() } else { Value::Null },
            "base": now["base"]["ref"],
            "repository": now["base"]["repo"]["id"],
        }))
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
        let transport = self.connect(self.config.token_file.as_deref(), "token_file")?;
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

/// The answer to `verb` of `request`.
fn answer(ctx: &Context<'_>, remote: &dyn Remote, verb: &str, request: &Value) -> Value {
    let loaded = match ctx.source.load() {
        Ok(loaded) => loaded,
        Err(why) => return refused(format!("turned off: its settings do not read: {why}")),
    };
    let config = loaded.config;
    let serves = config.enabled && config.pull_requests == PullRequests::Publish;
    let Some(repository) = config.repository.clone().filter(|_| serves) else {
        return refused(
            "turned off: the committed settings do not say enabled = true and pull_requests = \
             \"publish\"",
        );
    };
    let opened = (ctx.board.state_folder())
        .map_err(|error| error.said().to_string())
        .and_then(|folder| Store::open(&folder));
    let store = match opened {
        Ok(store) => store,
        Err(why) => return unavailable(why),
    };
    let _lock = match store.lock() {
        Ok(lock) => lock,
        Err(why) => return unavailable(why),
    };
    let state = match store.state() {
        Ok(state) => state.unwrap_or_default(),
        Err(why) => return unavailable(why),
    };
    let mut run = Run {
        ctx,
        config,
        revision: loaded.revision,
        repository,
        store,
        state,
        by: clean_title(&text(request, "by")),
    };
    let answered = match verb {
        "publish" => run.publish(remote, request),
        "merge" => run.merge(request),
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

#[cfg(test)]
#[path = "deliver_tests.rs"]
mod tests;
