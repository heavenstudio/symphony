//! `symphony-github verify --request FILE`: what answers a release run that verifies its commit
//! on GitHub's machines (`[releases] hosted = ["symphony-github", "verify"]`, docs/workflow/RELEASES.md,
//! "On GitHub's runners"), one JSON object read and one printed. It serves only `enabled` settings
//! whose `verification` is `hosted`, to the repository the state is bound to by its number.
//!
//! The request names the `commit` to verify, its 40 hexadecimal digits, the `folder` the run's
//! logs are downloaded into, and optionally the `seconds` it may wait. It starts the workflow
//! [`WORKFLOW`] of the repository's default branch for that commit, waits until the run it
//! started has ended, and downloads each job's log and each artifact the run kept. The answer's
//! `state` is one of:
//!
//! - `ended`: the run ended, whatever its conclusion: `run` names it (its number, page, attempt,
//!   conclusion and the commit whose workflow file it ran), `jobs` each job with its conclusion,
//!   seconds and log, and `artifacts` each archive downloaded;
//! - `late`: the run was started and had not ended within the time given (`why`, and `run`);
//! - `refused`: nothing can be verified there as things are, and asking again changes nothing
//!   (`why`);
//! - `unavailable`: GitHub gave no answer to go on by (`why`).
//!
//! A run is believed only if GitHub says it is of the bound repository, of the workflow file at
//! [`PATH`], started by a `workflow_dispatch` on the default branch, titled for exactly the
//! request's commit, and completed; anything else is `refused`, and so is never read as green or
//! red. Whether the workflow file it ran is the one the default branch commits is the release
//! run's to check, with its own git, by the commit `run.workflow_commit` names. A job cancelled
//! or timed out is answered as GitHub concluded it, which the release run reads as a step that did
//! not run.
//!
//! It sends one kind of write, the dispatch, journaled before it is made; the workflow it starts
//! is given no secret and writes nothing. The state's lock is held only until the dispatch is
//! answered, never while it waits. No token is in an argument, a URL, the journal or a line said
//! here, and a download's URL is said nowhere.

use crate::config::Verification;
use crate::deliver::{self, Host, Lost, Stopped, refused, unavailable};
use crate::reconcile::{Context, Outcome};
use crate::state::{Entry, Event, Store};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// The workflow file, as the dispatch names it.
pub const WORKFLOW: &str = "release-verify.yml";
/// Its path in the repository, as a run of it says it.
pub const PATH: &str = ".github/workflows/release-verify.yml";
/// What the workflow titles a run of `commit` (its `run-name`).
#[must_use]
pub fn title(commit: &str) -> String {
    format!("Release verification of {commit}")
}

/// The rests between looks for the run a dispatch started, which GitHub lists a little after it
/// takes the dispatch: two minutes in all.
const SHOWS: [Duration; 9] = [
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
    Duration::from_secs(16),
    Duration::from_secs(16),
    Duration::from_secs(16),
    Duration::from_secs(16),
    Duration::from_secs(26),
];
/// The rest between two reads of a run that has not ended.
const POLL: Duration = Duration::from_secs(30);
/// The reads in a row GitHub may leave unanswered while a run is waited for before the wait is
/// given up: ten minutes of them.
const UNANSWERED: usize = 20;
/// The longest a run is waited for when the request names no time: six hours, the longest GitHub
/// lets one job run.
const WAIT_SECONDS: u64 = 6 * 3600;

/// One verification's run: the repository's calls and what it rests by.
struct Run<'a> {
    ctx: &'a Context<'a>,
    host: &'a Host<'a>,
}

/// A run's answer with what GitHub says of it now.
fn named(run: &Value) -> Value {
    json!({
        "id": run["id"],
        "url": run["html_url"],
        "attempt": run["run_attempt"],
        "conclusion": run["conclusion"],
        "workflow_commit": run["head_sha"],
        "started": run["run_started_at"],
        "ended": run["updated_at"],
    })
}

/// The seconds between two of GitHub's times, where both read.
fn between(from: &Value, to: &Value) -> Option<i64> {
    let seconds = |value: &Value| crate::mint::seconds(value.as_str()?);
    Some((seconds(to)? - seconds(from)?).max(0))
}

impl Run<'_> {
    /// The numbers of the workflow's dispatched runs GitHub lists now, newest first, each with
    /// its title.
    fn listed(&self) -> Result<Vec<(i64, String)>, Stopped> {
        let path =
            format!("/actions/workflows/{WORKFLOW}/runs?event=workflow_dispatch&per_page=100");
        match self.host.get(&path)? {
            (200, value) => Ok((value["workflow_runs"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|run| {
                    let title = run["display_title"].as_str().unwrap_or_default();
                    Some((run["id"].as_i64()?, title.to_string()))
                })
                .collect()),
            (404, _) => Err(refused(format!(
                "{} has no workflow {WORKFLOW} on its default branch, or token_file's token may \
                 not read its runs (actions: write)",
                self.host.repository.full_name()
            ))),
            (status, _) => Err(unavailable(format!(
                "listing the runs of {WORKFLOW}: GitHub answered {status}"
            ))),
        }
    }

    /// The workflow started on `branch` for `commit`: the run's number where GitHub's answer
    /// names it, or that the answer was lost.
    fn dispatch(&self, branch: &str, commit: &str) -> Result<Result<Option<i64>, Lost>, Stopped> {
        let sent = json!({"ref": branch, "inputs": {"commit": commit}});
        let path = (self.host).path(&format!("/actions/workflows/{WORKFLOW}/dispatches"));
        Ok(match self.host.send("POST", path, Some(&sent))? {
            Ok((200 | 204, value)) => Ok(value["workflow_run_id"].as_i64()),
            Ok((status @ (403 | 404), _)) => {
                return Err(refused(format!(
                    "GitHub refused to start {WORKFLOW} ({status}): token_file's token needs \
                     actions read and write on the repository, and the default branch the \
                     workflow file"
                )));
            }
            Ok((422, value)) => {
                return Err(refused(format!(
                    "GitHub refused to start {WORKFLOW} (422): {}",
                    crate::github::clean_title(value["message"].as_str().unwrap_or_default())
                )));
            }
            Ok((status, _)) => {
                return Err(unavailable(format!(
                    "GitHub answered {status} to the start of {WORKFLOW}: none was started"
                )));
            }
            Err(lost) => Err(lost),
        })
    }

    /// The default branch of the repository, which must hold `commit`.
    fn branch_holding(&self, commit: &str) -> Result<String, Stopped> {
        let branch = match self.host.get("")? {
            (200, value) => (value["default_branch"].as_str())
                .unwrap_or_default()
                .to_string(),
            (status, _) => {
                return Err(unavailable(format!(
                    "reading the repository's default branch: GitHub answered {status}"
                )));
            }
        };
        if !deliver::plain(&branch, "-._/") {
            return Err(unavailable("the repository names no default branch"));
        }
        match self.host.get(&format!("/commits/{commit}"))? {
            (200, value) if value["sha"] == json!(commit) => Ok(branch),
            (200 | 404 | 422, _) => Err(unavailable(format!(
                "{} does not hold the commit {commit}: nothing is verified there until it is \
                 pushed",
                self.host.repository.full_name()
            ))),
            (status, _) => Err(unavailable(format!(
                "reading the commit {commit}: GitHub answered {status}"
            ))),
        }
    }

    /// The run a dispatch for `commit` started: the one GitHub named in its answer, else the
    /// newest titled for the commit that was not among `before`, looked for until it shows.
    fn started(
        &self,
        commit: &str,
        before: &BTreeSet<i64>,
        said: Option<i64>,
    ) -> Result<i64, Stopped> {
        if let Some(id) = said {
            return Ok(id);
        }
        let title = title(commit);
        let mut last = None;
        for wait in SHOWS {
            (self.ctx.rest)(wait);
            match self.listed() {
                Ok(runs) => {
                    let new =
                        (runs.iter()).find(|(id, named)| !before.contains(id) && *named == title);
                    if let Some((id, _)) = new {
                        return Ok(*id);
                    }
                }
                Err(stopped) => last = Some(stopped),
            }
        }
        Err(last.unwrap_or_else(|| {
            unavailable(format!(
                "the workflow was started for {commit} and GitHub listed no run of it within {} \
                 s",
                SHOWS.iter().map(Duration::as_secs).sum::<u64>()
            ))
        }))
    }

    /// The run `id` once it has ended, read every [`POLL`] for `seconds` at most: `Err` with the
    /// run as last read when it had not.
    fn ended(&self, id: i64, seconds: u64) -> Result<Result<Value, Value>, Stopped> {
        let (mut waited, mut unanswered) = (0, 0);
        let mut last = Value::Null;
        loop {
            match self.host.get(&format!("/actions/runs/{id}")) {
                Ok((200, run)) if run["status"] == json!("completed") => return Ok(Ok(run)),
                Ok((200, run)) => {
                    unanswered = 0;
                    last = run;
                }
                Ok((status @ (403 | 404), _)) => {
                    return Err(unavailable(format!(
                        "reading the run {id}: GitHub answered {status}"
                    )));
                }
                answer => {
                    unanswered += 1;
                    if unanswered >= UNANSWERED {
                        let why = match answer {
                            Ok((status, _)) => format!("GitHub answered {status}"),
                            Err(stopped) => stopped["why"].as_str().unwrap_or_default().into(),
                        };
                        return Err(unavailable(format!(
                            "the run {id} could not be read {UNANSWERED} times in a row: {why}"
                        )));
                    }
                }
            }
            if waited >= seconds {
                return Ok(Err(last));
            }
            (self.ctx.rest)(POLL);
            waited += POLL.as_secs();
        }
    }

    /// Why the ended run `run` is not believed as the verification of `commit` in the
    /// repository numbered `repository` whose default branch is `branch`; none when it is.
    fn disbelieved(run: &Value, repository: i64, branch: &str, commit: &str) -> Option<String> {
        let text = |field: &str| run[field].as_str().unwrap_or_default();
        if run["repository"]["id"].as_i64() != Some(repository) {
            return Some(format!(
                "it is of the repository {}, not the one bound here ({repository})",
                run["repository"]["id"]
            ));
        }
        if text("path") != PATH {
            return Some(format!(
                "it ran the workflow {:?}, not {PATH}",
                text("path")
            ));
        }
        if text("event") != "workflow_dispatch" {
            return Some(format!("it was started by {:?}", text("event")));
        }
        if text("head_branch") != branch {
            return Some(format!(
                "it ran the workflow of {:?}, not of the default branch {branch}",
                text("head_branch")
            ));
        }
        if text("display_title") != title(commit) {
            return Some(format!(
                "it is titled {:?}, not for the commit {commit}",
                crate::github::clean_title(text("display_title"))
            ));
        }
        if !deliver::commit(text("head_sha")) {
            return Some("it names no commit its workflow file is of".into());
        }
        if text("status") != "completed" || text("conclusion").is_empty() {
            return Some("it has not ended".into());
        }
        None
    }

    /// What GitHub redirects `rest` to, downloaded as `name` in `folder`: the file's name, or
    /// why there is none.
    fn download(&self, rest: &str, folder: &Path, name: &str) -> Result<String, String> {
        let request = crate::transport::Request {
            method: "GET",
            path: self.host.path(rest),
            body: None,
        };
        let failed = |failure| match failure {
            crate::transport::Failure::NotSent(why) | crate::transport::Failure::Unknown(why) => {
                why
            }
        };
        let response = self.host.transport.send(&request).map_err(failed)?;
        let Some(url) = (response.headers.get("location")).filter(|_| response.status == 302)
        else {
            return Err(format!(
                "GitHub answered {} and no download",
                response.status
            ));
        };
        match self
            .host
            .transport
            .fetch(url, &folder.join(name))
            .map_err(failed)?
        {
            200 => Ok(name.into()),
            status => Err(format!("its download answered {status}")),
        }
    }

    /// The jobs of the run `id`'s last attempt, each with its log downloaded into `folder`.
    fn jobs(&self, id: i64, folder: &Path) -> Result<Vec<Value>, Stopped> {
        let path = format!("/actions/runs/{id}/jobs?filter=latest&per_page=100");
        let listed = match self.host.get(&path)? {
            (200, value) => value,
            (status, _) => {
                return Err(unavailable(format!(
                    "listing the jobs of the run {id}: GitHub answered {status}"
                )));
            }
        };
        let jobs = listed["jobs"].as_array().cloned().unwrap_or_default();
        if listed["total_count"]
            .as_u64()
            .is_some_and(|total| total > jobs.len() as u64)
        {
            return Err(unavailable(format!(
                "the run {id} has {} jobs and GitHub listed {}",
                listed["total_count"],
                jobs.len()
            )));
        }
        let mut answered = Vec::new();
        for job in &jobs {
            let Some(number) = job["id"].as_i64() else {
                return Err(unavailable(format!("a job of the run {id} has no id")));
            };
            let log = self.download(
                &format!("/actions/jobs/{number}/logs"),
                folder,
                &format!("job-{number}.log"),
            );
            answered.push(json!({
                "id": number,
                "name": crate::github::clean_title(job["name"].as_str().unwrap_or_default()),
                "conclusion": job["conclusion"],
                "seconds": between(&job["started_at"], &job["completed_at"]),
                "url": job["html_url"],
                "log": log.as_ref().ok(),
                "no_log": log.err(),
            }));
        }
        Ok(answered)
    }

    /// The artifacts the run `id` kept, each downloaded into `folder`; one that is not is left
    /// out, with why in `missing`.
    fn artifacts(&self, id: i64, folder: &Path) -> Result<(Vec<Value>, Vec<String>), Stopped> {
        let path = format!("/actions/runs/{id}/artifacts?per_page=100");
        let listed = match self.host.get(&path)? {
            (200, value) => value,
            (status, _) => {
                return Err(unavailable(format!(
                    "listing the artifacts of the run {id}: GitHub answered {status}"
                )));
            }
        };
        let (mut kept, mut missing) = (Vec::new(), Vec::new());
        for artifact in listed["artifacts"].as_array().into_iter().flatten() {
            let name = crate::github::clean_title(artifact["name"].as_str().unwrap_or_default());
            let Some(number) = artifact["id"].as_i64() else {
                missing.push(format!("{name}: it has no id"));
                continue;
            };
            let file = format!("artifact-{number}.zip");
            match self.download(&format!("/actions/artifacts/{number}/zip"), folder, &file) {
                Ok(file) => kept.push(json!({"id": number, "name": name, "file": file})),
                Err(why) => missing.push(format!("{name}: {why}")),
            }
        }
        Ok((kept, missing))
    }
}

/// What a request asks, read: the commit, the folder and the seconds to wait.
fn asked(request: &Value) -> Result<(String, std::path::PathBuf, u64), Stopped> {
    let commit = request["commit"].as_str().unwrap_or_default();
    if !deliver::commit(commit) {
        return Err(refused(format!(
            "a commit to verify is its whole name, 40 hexadecimal digits: {commit:?}"
        )));
    }
    let folder = Path::new(request["folder"].as_str().unwrap_or_default());
    if !folder.is_absolute() || !folder.is_dir() {
        return Err(refused(format!(
            "folder is the absolute path of a folder that is there, which the run's logs are \
             downloaded into: {:?}",
            folder.display()
        )));
    }
    let seconds = match &request["seconds"] {
        Value::Null => WAIT_SECONDS,
        value => value.as_u64().ok_or_else(|| {
            refused("seconds is how long the run may be waited for, a whole number")
        })?,
    };
    Ok((commit.into(), folder.into(), seconds))
}

/// The answer to `request`.
fn answer(ctx: &Context<'_>, request: &Value) -> Result<Value, Stopped> {
    let loaded = (ctx.source.load())
        .map_err(|why| refused(format!("turned off: its settings do not read: {why}")))?;
    let config = loaded.config;
    let serves = config.enabled && config.verification == Verification::Hosted;
    let Some(repository) = config.repository.clone().filter(|_| serves) else {
        return Err(refused(
            "turned off: the committed settings do not say enabled = true and verification = \
             \"hosted\"",
        ));
    };
    let (commit, folder, seconds) = asked(request)?;
    let by = crate::github::clean_title(request["by"].as_str().unwrap_or_default());
    let store = (ctx.board.state_folder())
        .map_err(|error| error.said().to_string())
        .and_then(|folder| Store::open(&folder))
        .map_err(unavailable)?;
    let transport = deliver::connect(ctx, &config)?;
    let host = Host {
        transport: transport.as_ref(),
        repository,
    };
    let run = Run { ctx, host: &host };
    let journal = |event: Event, result: Value| {
        // A journal that cannot be written stops no verification: a run started twice writes
        // nothing, and each is read by its own number.
        let _ = store.journal(&Entry {
            at: ctx.now,
            event,
            preview: String::new(),
            op: format!("verify:{commit}"),
            kind: "verify".into(),
            target: commit.clone(),
            digest: crate::digest(&[WORKFLOW, &commit]),
            base: Value::Null,
            config: loaded.revision.clone(),
            by: by.clone(),
            result,
        });
    };
    // Held until the dispatch is answered, never while the run is waited for: a land's delivery
    // takes the same lock.
    let lock = store.lock().map_err(unavailable)?;
    let mut state = store.state().map_err(unavailable)?.unwrap_or_default();
    let (number, _) = deliver::bound(&host, &store, &mut state)?;
    let branch = run.branch_holding(&commit)?;
    let before: BTreeSet<i64> = run.listed()?.into_iter().map(|(id, _)| id).collect();
    journal(Event::Intent, Value::Null);
    let said = match run.dispatch(&branch, &commit) {
        Ok(Ok(said)) => said,
        // Looked for as any run is: started, it shows; not shown is no run to read.
        Ok(Err(Lost(why))) => {
            journal(Event::Unknown, json!(why));
            None
        }
        Err(stopped) => {
            journal(Event::Failed, stopped["why"].clone());
            return Err(stopped);
        }
    };
    drop(lock);
    let id = run.started(&commit, &before, said)?;
    journal(Event::Ack, json!({"run": id}));
    let ended = match run.ended(id, seconds)? {
        Ok(ended) => ended,
        Err(last) => {
            return Ok(json!({
                "state": "late",
                "run": named(&last),
                "why": format!(
                    "the run {id} had not ended within {seconds} s: it is left to end, and writes \
                     nothing"
                ),
            }));
        }
    };
    if let Some(why) = Run::disbelieved(&ended, number, &branch, &commit) {
        return Err(refused(format!("the run {id} is not believed: {why}")));
    }
    let jobs = run.jobs(id, &folder)?;
    let (artifacts, missing) = run.artifacts(id, &folder)?;
    Ok(json!({
        "state": "ended",
        "commit": commit,
        "run": named(&ended),
        "jobs": jobs,
        "artifacts": artifacts,
        "missing": missing,
    }))
}

/// `verify --request FILE`: the answer on `out`, one line of JSON.
///
/// # Errors
///
/// The request does not read: nothing was asked of anyone.
pub fn run(ctx: &Context<'_>, file: &Path, out: &mut dyn Write) -> Outcome {
    let said = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let request: Value =
        serde_json::from_str(&said).map_err(|e| format!("{}: {e}", file.display()))?;
    if !request.is_object() {
        return Err(format!("{}: a request is one JSON object", file.display()));
    }
    let answer = answer(ctx, &request).unwrap_or_else(|stopped| stopped);
    let _ = writeln!(out, "{answer}");
    Ok(0)
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
