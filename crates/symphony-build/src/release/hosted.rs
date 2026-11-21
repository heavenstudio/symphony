//! The release's verification on GitHub's machines (docs/RELEASES.md, "On GitHub's runners"):
//! asked of the command `[releases] hosted` names, the GitHub adapter's `symphony-github verify`,
//! and of nothing while the project names none. No package here builds with the adapter: it is run
//! as a program, `COMMAND… --request FILE`, in the main checkout, and answers one line of JSON.
//!
//! Only the full verification is asked for ([`serves`]): the check `verify` running `just verify`
//! whole, which is what the workflow runs of the commit. The adapter starts the workflow for the
//! candidate's commit, waits for its run and downloads each job's log and artifact into the
//! check's own folder among the run's logs (`KEY.hosted`). Its answer is believed here only when
//! the workflow file its run ran is the one main commits, by git's own name of the file at both
//! commits ([`Hosting::committed`]). What the run ended as is then laid out as `just verify` lays
//! its own steps out ([`super::steps`]): each job's artifact is the step's `build/verify` folder,
//! so the list and the steps' own logs are the ones a run here writes, and what files a red reads
//! them unchanged. A job GitHub cancelled, timed out or ended any other way than passed or failed
//! is a step that did not run, red and never green, and so is a job that says it passed and listed
//! no step.
//!
//! Whatever is not such an answer (the adapter refused, GitHub did not answer, the run is not
//! believed, an archive does not open) is no red of the commit: [`Judged::Here`] says why, and the
//! run verifies here as a project without the adapter does. The run's time ending while the
//! adapter waits is a check stopped at the run's time, as any is.
//!
//! A run the hosted clock started (docs/RELEASES.md, "The cadence") is the one run that verifies
//! nothing here: what GitHub does not judge of it is judged by no one ([`Hosted::nowhere`]), and
//! the run ends [`UNJUDGED`], a result that is no red, files nothing and is asked for again once
//! the clock's interval has passed.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use symphony_board::sys::Command;

use super::board;
use super::checks::{Ran, Step, Within};
use super::git::{git, short};
use super::group::{Failed, output_until};
use super::span::Span;
use super::steps;

/// The workflow file whose run is believed, as main commits it.
pub(crate) const WORKFLOW: &str = ".github/workflows/release-verify.yml";
/// The job of the workflow that lists the commit's steps and runs none.
pub(crate) const PLAN: &str = "The commit's steps";
/// What the workflow names the artifact of the step NAME: `verify-NAME`.
const ARTIFACT: &str = "verify-";
/// The folder a job's artifact holds, as the paths its list names say it.
const FOLDER: &str = "/build/verify/";

/// What a run the hosted clock started comes to when GitHub's runners judged nothing of its commit:
/// its record's result, neither green nor red.
pub(crate) const UNJUDGED: &str = "unjudged";

/// What judged a check: GitHub's run, this machine after GitHub could not, or no one.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Hosted {
    /// The check.
    pub(crate) check: String,
    /// `GitHub`, `here` or `nowhere`.
    pub(crate) judged: String,
    /// The workflow run that judged it: its page, attempt and the commit whose workflow file it
    /// ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) attempt: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) workflow_commit: Option<String>,
    /// Why this machine judged it, or no one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) why: Option<String>,
}

impl Hosted {
    /// The check `check` judged here, for `why`.
    pub(crate) fn here(check: &str, why: &str) -> Hosted {
        Hosted {
            check: check.into(),
            judged: "here".into(),
            run: None,
            attempt: None,
            workflow_commit: None,
            why: Some(why.into()),
        }
    }

    /// The check `check` judged by no one, for `why`: GitHub gave no result of it, and the run
    /// that asked verifies nothing here.
    pub(crate) fn nowhere(check: &str, why: &str) -> Hosted {
        Hosted {
            judged: "nowhere".into(),
            ..Hosted::here(check, why)
        }
    }

    /// Whether GitHub's run judged it.
    pub(crate) fn there(&self) -> bool {
        self.judged == "GitHub"
    }

    /// As a run's line says it.
    pub(crate) fn said(&self) -> String {
        let why = || self.why.as_deref().unwrap_or("it gave no result");
        match &self.run {
            Some(run) if self.there() => format!("{} verified on GitHub ({run})", self.check),
            _ if self.there() => format!("{} asked of GitHub: {}", self.check, why()),
            _ if self.judged == "nowhere" => format!(
                "{} not verified: {}; the hosted clock's run verifies nothing here",
                self.check,
                why()
            ),
            _ => format!("{} verified here, not on GitHub: {}", self.check, why()),
        }
    }
}

/// The environment a check judged on GitHub ran on: that run's runners, which no run here is.
pub(crate) fn environment(judged: &Hosted) -> super::inputs::Environment {
    let runner = "the runner's";
    super::inputs::Environment {
        system: format!(
            "GitHub's runners, {}",
            judged.run.as_deref().unwrap_or("a workflow run")
        ),
        chrome: runner.into(),
        postgresql: runner.into(),
        postgresql_16: runner.into(),
    }
}

/// Whether `step` is what the workflow verifies: the full verification by its own command.
pub(crate) fn serves(step: &Step) -> bool {
    (step.name.as_str(), step.command.as_str()) == super::VERIFY
}

/// How a check asked of GitHub was judged.
pub(crate) enum Judged {
    /// By its run there: how it ran, and the run.
    There(Box<Ran>, Hosted),
    /// Not there, for this reason: it is verified here.
    Here(String),
}

/// What a verification is asked with.
pub(crate) struct Hosting<'a> {
    /// The checkout whose history is read.
    pub(crate) root: &'a Path,
    /// The main checkout, which the command is run in.
    pub(crate) main: &'a Path,
    /// `[releases] hosted`: the command.
    pub(crate) command: &'a [String],
    /// The candidate's commit.
    pub(crate) commit: &'a str,
    /// The remote's main as fetched, whose workflow file a run must have run, and where it is
    /// fetched from again for a run of a later commit of it.
    pub(crate) tip: &'a str,
    pub(crate) remote: &'a str,
    pub(crate) branch: &'a str,
    /// The run that asks.
    pub(crate) by: &'a str,
}

/// The folder the adapter downloads into for the check `name`, among the run's `logs`.
pub(crate) fn folder(logs: &Path, name: &str) -> PathBuf {
    logs.join(format!("{}.hosted", board::key(name)))
}

impl Hosting<'_> {
    /// Git's name of the workflow file at `commit`, none where git does not hold it there.
    fn file_at(&self, commit: &str) -> Option<String> {
        git(
            self.root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{commit}:{WORKFLOW}"),
            ],
        )
        .ok()
    }

    /// Why the run that ran the workflow file of `commit` is not believed; none when that file is
    /// the one main commits at the tip fetched. A commit not held here is fetched for once.
    fn committed(&self, commit: &str, within: Within<'_>) -> Option<String> {
        let plain = commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit());
        if !plain {
            return Some(format!(
                "its run names no commit its workflow file is of: {commit:?}"
            ));
        }
        let Some(ours) = self.file_at(self.tip) else {
            return Some(format!(
                "{} commits no {WORKFLOW} at {}",
                self.branch,
                short(self.tip)
            ));
        };
        let theirs = self.file_at(commit).or_else(|| {
            let _ = super::git::fetch_line(self.root, self.remote, self.branch, within.bound);
            self.file_at(commit)
        });
        match theirs {
            Some(theirs) if theirs == ours => None,
            Some(_) => Some(format!(
                "its run ran the {WORKFLOW} of {}, which is not the file {} commits at {}",
                short(commit),
                self.branch,
                short(self.tip)
            )),
            None => Some(format!(
                "its run ran the {WORKFLOW} of {}, a commit {}/{} does not hold",
                short(commit),
                self.remote,
                self.branch
            )),
        }
    }

    /// The adapter asked to verify the commit, its downloads in `folder`: its answer, how long it
    /// took, or the check `step` as the run's bound ended it.
    fn ask(
        &self,
        step: &Step,
        folder: &Path,
        log: &Path,
        within: Within<'_>,
    ) -> Result<Result<Value, String>, Box<Ran>> {
        let file = folder.join("request.json");
        let left = within
            .bound
            .until
            .saturating_duration_since(std::time::Instant::now());
        let request = json!({
            "commit": self.commit, "folder": folder, "by": self.by, "seconds": left.as_secs(),
        });
        if let Err(e) = fs::write(&file, format!("{request}\n")) {
            return Ok(Err(symphony::text::os_error(&e, &file)));
        }
        let said = format!("{} --request {}", self.command.join(" "), file.display());
        let _ = fs::write(log, format!("$ {said}\n"));
        let mut command = Command::new(&self.command[0]);
        command
            .args(&self.command[1..])
            .arg("--request")
            .arg(&file)
            .current_dir(self.main);
        let started = Span::begin();
        let done = match output_until(&mut command, within.bound) {
            Ok(done) => done,
            Err(Failed::Io(e)) => return Ok(Err(format!("cannot run {said}: {e}"))),
            Err(stopped) => {
                let interrupted = matches!(stopped, Failed::Interrupted(_));
                let mut ran = Ran::never(step, log, String::new());
                ran.error = None;
                ran.seconds = started.took().seconds;
                ran.timed_out = !interrupted;
                ran.interrupted = interrupted;
                let _ = fs::write(
                    log,
                    format!(
                        "$ {said}\n(stopped after {:.0} s: {}; a run it started on GitHub is left \
                         to end, and writes nothing)\n",
                        ran.seconds,
                        if interrupted {
                            "the run was told to stop"
                        } else {
                            "the run's time was up"
                        }
                    ),
                );
                return Err(Box::new(ran));
            }
        };
        let (out, err) = (
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr),
        );
        let _ = fs::write(log, format!("$ {said}\n{out}{err}({})\n", done.status));
        let first = |text: &str| {
            (text.lines().map(str::trim))
                .find(|line| !line.is_empty())
                .map(str::to_string)
        };
        if !done.status.success() {
            return Ok(Err(format!(
                "{said}: {}",
                first(&err).unwrap_or_else(|| done.status.to_string())
            )));
        }
        let answer: Option<Value> = (first(&out).and_then(|line| serde_json::from_str(&line).ok()))
            .filter(Value::is_object);
        Ok(answer.ok_or_else(|| format!("{said}: its answer is not a JSON object")))
    }

    /// The check `step` verified on GitHub: its downloads beside the run's `logs`, its own log at
    /// `log`, its steps laid out in its folder there as a run here lists them.
    pub(crate) fn verify(
        &self,
        step: &Step,
        logs: &Path,
        log: &Path,
        within: Within<'_>,
    ) -> Judged {
        let folder = folder(logs, &step.name);
        // Its own: what an earlier check of this name left is no part of this answer.
        let _ = fs::remove_dir_all(&folder);
        if let Err(e) = fs::create_dir_all(&folder) {
            return Judged::Here(symphony::text::os_error(&e, &folder));
        }
        let started = Span::begin();
        let answer = match self.ask(step, &folder, log, within) {
            Ok(Ok(answer)) => answer,
            Ok(Err(why)) => return Judged::Here(why),
            Err(stopped) => {
                let judged = Hosted {
                    judged: "GitHub".into(),
                    why: Some("its run there had not ended when this run stopped".into()),
                    ..Hosted::here(&step.name, "")
                };
                return Judged::There(stopped, judged);
            }
        };
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        match text(&answer["state"]).as_str() {
            "ended" => {}
            // The command's own wait ended first: the run's time is up all the same, so the check
            // is timed out and nothing is verified here.
            "late" => {
                let why = text(&answer["why"]);
                let mut ran = Ran::never(step, log, String::new());
                ran.error = None;
                ran.seconds = started.took().seconds;
                ran.timed_out = true;
                if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log) {
                    use std::io::Write as _;
                    let _ = writeln!(
                        file,
                        "(stopped after {:.0} s: {why}; a run it started on GitHub is left to \
                         end, and writes nothing)",
                        ran.seconds
                    );
                }
                let judged = Hosted {
                    judged: "GitHub".into(),
                    why: Some(format!("its run there had not ended in time: {why}")),
                    ..Hosted::here(&step.name, "")
                };
                return Judged::There(Box::new(ran), judged);
            }
            state @ ("refused" | "unavailable") => {
                return Judged::Here(format!("{state}: {}", text(&answer["why"])));
            }
            other => return Judged::Here(format!("the command answered the state {other:?}")),
        }
        if answer["commit"] != json!(self.commit) {
            return Judged::Here(format!(
                "the command answered for the commit {}, not {}",
                answer["commit"],
                short(self.commit)
            ));
        }
        let run = &answer["run"];
        let workflow_commit = text(&run["workflow_commit"]);
        if let Some(why) = self.committed(&workflow_commit, within) {
            return Judged::Here(format!(
                "the run {} is not believed: {why}",
                text(&run["url"])
            ));
        }
        let listed = match lay_out(&answer, &folder, &steps::folder(logs, &step.name), within) {
            Ok(()) => steps::read(&steps::folder(logs, &step.name), &step.name),
            Err(why) => Err(why),
        };
        let mut listed = match listed {
            Ok(listed) => listed,
            Err(why) => {
                return Judged::Here(format!(
                    "what the run {} left does not read: {why}",
                    text(&run["url"])
                ));
            }
        };
        jobs(&answer, &folder, &step.name, &mut listed);
        let passed = run["conclusion"] == json!("success");
        if passed && listed.is_empty() {
            return Judged::Here(format!(
                "the run {} ended success and listed no step",
                text(&run["url"])
            ));
        }
        let url = text(&run["url"]);
        let mut said = fs::read_to_string(log).unwrap_or_default();
        said.push_str(&format!(
            "the workflow run {url} ended {}; its jobs' logs and artifacts are in {}\n",
            text(&run["conclusion"]),
            folder.display()
        ));
        let _ = fs::write(log, said);
        let ran = Ran {
            name: step.name.clone(),
            command: format!("{} on GitHub's runners ({url})", step.command),
            exit_code: Some(i32::from(!passed)),
            signal: None,
            seconds: started.took().seconds,
            log: log.to_path_buf(),
            steps: listed,
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
            alone: None,
        };
        let judged = Hosted {
            check: step.name.clone(),
            judged: "GitHub".into(),
            run: Some(url),
            attempt: run["attempt"].as_u64(),
            workflow_commit: Some(workflow_commit),
            why: None,
        };
        Judged::There(Box::new(ran), judged)
    }
}

/// A job's log among the downloads in `folder`, or a file there saying why there is none.
fn job_log(job: &Value, folder: &Path) -> PathBuf {
    if let Some(log) = job["log"].as_str().filter(|log| plain(log).is_some()) {
        return folder.join(log);
    }
    let file = folder.join(format!("job-{}.none.log", job["id"]));
    let why = job["no_log"].as_str().unwrap_or("it named none");
    let _ = fs::write(
        &file,
        format!("GitHub gave no log of this job: {why}\n{}\n", job["url"]),
    );
    file
}

/// Each job of `answer` that is a step told as GitHub concluded it, among `listed`, the steps the
/// run's artifacts listed for the check `check`: a job that passed or failed keeps the step it
/// listed, and is one by its conclusion where it listed none; any other end is a step that did
/// not run, whatever it listed.
fn jobs(answer: &Value, folder: &Path, check: &str, listed: &mut Vec<Ran>) {
    for job in answer["jobs"].as_array().into_iter().flatten() {
        let name = job["name"].as_str().unwrap_or_default();
        if name == PLAN || name.is_empty() {
            continue;
        }
        let full = format!("{check}/{name}");
        let conclusion = job["conclusion"].as_str().unwrap_or("no conclusion");
        let known = listed.iter().any(|step| step.name == full);
        let ended = matches!(conclusion, "success" | "failure");
        if ended && known {
            continue;
        }
        listed.retain(|step| step.name != full);
        let (exit_code, error) = match conclusion {
            "failure" => (Some(1), None),
            "success" => (
                None,
                Some(
                    "its job on GitHub ended success and listed no step: its result is not known"
                        .to_string(),
                ),
            ),
            other => (
                None,
                Some(format!(
                    "GitHub ended its job {other}: the step did not run to its end"
                )),
            ),
        };
        listed.push(Ran {
            name: full,
            command: format!("just verify {name}"),
            exit_code,
            signal: None,
            #[expect(
                clippy::cast_precision_loss,
                reason = "a job's seconds are far below 2^52"
            )]
            seconds: job["seconds"].as_u64().unwrap_or(0) as f64,
            log: job_log(job, folder),
            steps: Vec::new(),
            error,
            timed_out: false,
            interrupted: false,
            load: None,
            alone: None,
        });
    }
}

/// `path` as a path inside a folder: none where it is empty, absolute or names a parent.
fn plain(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    let inside = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    (inside && path.components().next().is_some()).then_some(path)
}

/// The artifacts of `answer`, downloaded into `folder`, laid out in `steps` as a run here lists
/// its steps: each archive opened in a folder of its own, its list's lines taken with each log
/// named by its place in the step's folder, and each log that is a plain file there copied.
fn lay_out(answer: &Value, folder: &Path, steps: &Path, within: Within<'_>) -> Result<(), String> {
    let _ = fs::remove_dir_all(steps);
    fs::create_dir_all(steps).map_err(|e| symphony::text::os_error(&e, steps))?;
    let mut list = String::new();
    let mut names: Vec<String> = Vec::new();
    for artifact in answer["artifacts"].as_array().into_iter().flatten() {
        let name = artifact["name"].as_str().unwrap_or_default();
        let (Some(step), Some(file)) = (
            name.strip_prefix(ARTIFACT).and_then(plain),
            artifact["file"].as_str().and_then(plain),
        ) else {
            continue;
        };
        let opened = folder.join(format!("{}.artifact", board::key(&step.to_string_lossy())));
        let mut unzip = symphony::children::command("unzip");
        unzip
            .args(["-q", "-o"])
            .arg(folder.join(file))
            .arg("-d")
            .arg(&opened);
        match output_until(&mut unzip, within.bound) {
            Ok(done) if done.status.success() => {}
            Ok(done) => {
                return Err(format!(
                    "the artifact {name} does not open: unzip: {}",
                    String::from_utf8_lossy(&done.stderr).trim()
                ));
            }
            Err(Failed::Io(e)) => {
                return Err(format!("the artifact {name} is not opened: unzip: {e}"));
            }
            Err(_) => {
                return Err(format!(
                    "the artifact {name} was not opened in the run's time"
                ));
            }
        }
        let Ok(lines) = fs::read_to_string(opened.join(steps::FILE)) else {
            continue;
        };
        for line in lines.lines() {
            let mut fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 5 || names.iter().any(|known| known == fields[0]) {
                continue;
            }
            // The log by its place in the step's folder, as the runner's path names it.
            let said = fields[3];
            let inside = said.rsplit_once(FOLDER).map_or(said, |(_, inside)| inside);
            let kept = plain(inside).filter(|inside| {
                let from = opened.join(inside);
                let to = steps.join(inside);
                // A plain file alone: an archive's link leads nowhere here.
                fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_file())
                    && to
                        .parent()
                        .is_some_and(|parent| fs::create_dir_all(parent).is_ok())
                    && fs::copy(&from, &to).is_ok()
            });
            let missing;
            fields[3] = match kept {
                Some(_) => inside,
                None => {
                    missing = format!("{}.missing.log", board::key(fields[0]));
                    let _ = fs::write(
                        steps.join(&missing),
                        format!("the artifact {name} holds no log at {said}\n"),
                    );
                    &missing
                }
            };
            names.push(fields[0].to_string());
            list.push_str(&fields.join("\t"));
            list.push('\n');
        }
    }
    let file = steps.join(steps::FILE);
    fs::write(&file, list).map_err(|e| symphony::text::os_error(&e, &file))
}

#[cfg(test)]
#[path = "hosted_tests.rs"]
mod tests;
