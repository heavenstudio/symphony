//! The release candidate (docs/workflow/RELEASES.md, "The release candidate"): the commit a release run of
//! main's tip found red in a check, held while its reds are fixed, so the release that comes is that
//! commit and its fixes alone however fast main moves (docs/workflow/analysis/release-candidate.md, A).
//!
//! Its record is `candidate.json` in the run's record folder ([`FILE`]), written whole while the
//! run's lock is held: the commit it holds now (its tip), the commit first judged and by which run,
//! the runs of it that decided (its rounds), each red with its task and how it was answered, each
//! check and step with the commit it was last green on ([`Standing`]), the advances, and, once it
//! ends, how. Beside it the local branch `release-candidate` ([`BRANCH`]) names its tip for whoever
//! fixes its reds: only the run moves it ([`steer`]), never from a value it did not set, and nothing
//! pushes it.
//!
//! A red is answered by its own task's land, which advances the candidate to the newest commit of
//! the landed branch an approve on that task named, that descends from the candidate's tip and holds
//! beyond it nothing main held before the land ([`settle`], [`passes`]), once the remote's main holds
//! the merge; by its task closed as covered by a landing that advanced the candidate, whose fix
//! holds it (one covered by any other landing has its fix on main and not in the candidate, which
//! is dropped); or by its task cancelled with no landing, its fix made outside this repository (one
//! done otherwise is approved, and waits for its land). Once every
//! red is answered, its tip is rerun ([`Candidate::plan`]): each check or step that was red or did
//! not run, and each whose inputs changed. A step's inputs are what the tree itself says they are
//! ([`super::inputs`]): a step, or a part of one (a lane of the tests), keeps its green from a run
//! of this candidate whose digest of its inputs is the tip's, or whose commit holds the tip's tree,
//! on the environment the rerun runs on; one whose inputs the tree does not tell keeps it only
//! from the same tree. It is dropped at its bounds ([`Bounds`]), and main's tip is run at once.
//!
//! A land a pull request delivered leaves one squashed commit on main and the fix's own commits in
//! no history of it. Its receipt ([`super::receipts`]) is then the one record both destinations are
//! read from: the squash is the landing, and the head the receipt kept is what the candidate
//! advances within ([`advance`], [`Delivered`]), so main is never merged into it. The candidate's
//! tip, and the release tagged on it, are on the line by that squash ([`Receipts::holds`]).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::board::{self, Reds};
use super::cadence::{self, Hold};
use super::checks::Ran;
use super::git::{commit, git, is_ancestor, short};
use super::inputs::Inputs;
use super::receipts::Receipts;
use super::record::{Carried, Held, Run, age, local};
use super::steps;
use symphony::taskcli::CliBoard;
use symphony_board::land::delivery::Receipt;

/// The local branch that names the candidate's tip, never pushed.
pub(crate) const BRANCH: &str = "release-candidate";
/// Its full name.
const REFERENCE: &str = "refs/heads/release-candidate";
/// The candidate's record, in the run's record folder.
pub(crate) const FILE: &str = "candidate.json";
/// The version of the record this build writes and reads; a newer one is a newer build's, which
/// this build leaves alone.
pub(crate) const VERSION: u32 = 1;

/// Where a candidate is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    /// Held: its reds are being fixed, or its rerun is due.
    Open,
    /// Its tip was tagged as a release.
    Released,
    /// Given up at a bound or by `--drop`.
    Dropped,
}

/// The release candidate's record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Candidate {
    pub(crate) version: u32,
    pub(crate) state: State,
    /// Its tip: the commit a rerun judges.
    pub(crate) commit: String,
    /// The commit first judged, the run that judged it and when it started.
    pub(crate) first: String,
    pub(crate) first_run: String,
    pub(crate) since: f64,
    /// The runs of it that decided: 1 once the first held it.
    pub(crate) rounds: u32,
    #[serde(default)]
    pub(crate) reds: Vec<Red>,
    #[serde(default)]
    pub(crate) standing: Vec<Standing>,
    #[serde(default)]
    pub(crate) advances: Vec<Advance>,
    /// What the run last set [`BRANCH`] to.
    #[serde(default)]
    pub(crate) branch: Option<String>,
    /// When it was last settled.
    #[serde(default)]
    pub(crate) settled_at: Option<f64>,
    #[serde(default)]
    pub(crate) ended: Option<Ended>,
}

/// A red of the candidate: its key, its check or step, its task, the round that found it, and how
/// it was answered, once it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Red {
    pub(crate) key: String,
    pub(crate) name: String,
    /// None while the board has not filed it.
    #[serde(default)]
    pub(crate) slug: Option<String>,
    pub(crate) round: u32,
    #[serde(default)]
    pub(crate) answer: Option<Answer>,
    /// Its task's status at the last settle, while it is unanswered.
    #[serde(default)]
    pub(crate) seen: Option<String>,
}

/// How a red was answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub(crate) enum Answer {
    /// Its task's land, `merge`, advanced the candidate to `commit`.
    Landed { merge: String, commit: String },
    /// Its task was closed as covered by the land `merge` of the task `task`, which advanced the
    /// candidate to `commit`: that fix holds its own.
    Covered {
        task: String,
        merge: String,
        commit: String,
    },
    /// Its task was closed, `status`, with no landing: its fix made outside this repository.
    Closed { status: String },
}

/// A check the candidate's runs ran: the commit it was last green on, whole, whether it was last
/// red beyond the steps it listed, and each step it listed with the commit it was last green on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Standing {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) green: Option<On>,
    #[serde(default)]
    pub(crate) beyond: bool,
    #[serde(default)]
    pub(crate) steps: Vec<StepStanding>,
}

/// A step a check listed: its name (`CHECK/STEP`), the commit it was last green on, and each part
/// it listed (`CHECK/STEP/PART`, a lane of the tests) the same way. A step green in a rerun that
/// ran some of its parts alone stands green by those parts, never of itself: it was not run whole
/// there, and what the rerun carried keeps the commit and run it was green on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StepStanding {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) green: Option<On>,
    /// Whether it was last red beyond the parts it listed: red, and none of them red.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) beyond: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) parts: Vec<StepStanding>,
}

/// Green on `commit` in the run `run`: with the digest of its inputs there, when the tree told
/// one, and the environment that run ran on ([`super::inputs`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct On {
    pub(crate) commit: String,
    pub(crate) run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) environment: Option<String>,
}

/// What a rerun judges a standing green against: whether a commit's tree is the tip's, what the
/// tip's tree tells of its checks' inputs, and the environment the rerun runs on.
#[derive(Clone, Copy)]
pub(crate) struct Tip<'a> {
    pub(crate) same: &'a dyn Fn(&str) -> bool,
    pub(crate) inputs: Option<&'a Inputs>,
    pub(crate) environment: Option<&'a str>,
}

impl Tip<'_> {
    /// The green `on` of `name` as the rerun carries it, when it does: green on the rerun's
    /// environment, and on a commit whose tree is the tip's, or on inputs whose digest is the
    /// one the tip tells for `name`.
    fn kept(&self, name: &str, on: Option<&On>) -> Option<Carried> {
        let on = on.filter(|on| on.environment.as_deref() == self.environment)?;
        let carried = |digest: Option<String>| Carried {
            name: name.to_string(),
            commit: on.commit.clone(),
            run: on.run.clone(),
            digest,
        };
        if (self.same)(&on.commit) {
            return Some(carried(None));
        }
        let told = self.inputs.and_then(|inputs| inputs.of(name))?;
        (on.digest.as_deref() == Some(told)).then(|| carried(on.digest.clone()))
    }

    /// The names the tip tells inputs for directly under `parent`, that `known` does not hold: a
    /// step or part no run of the candidate listed, which a rerun of some of them runs too.
    fn unlisted(&self, parent: &str, known: &[StepStanding]) -> Vec<String> {
        let Some(inputs) = self.inputs else {
            return Vec::new();
        };
        (inputs.under(parent).into_iter())
            .filter(|name| !known.iter().any(|step| step.name == *name))
            .collect()
    }
}

/// An advance: the red's task whose land made it, from which tip to which, and its land's merge,
/// or the squash a pull request delivered it by, with what its receipt says of that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Advance {
    pub(crate) slug: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) merge: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delivered: Option<Delivered>,
}

/// What the receipt of a land a pull request delivered says of an advance: the one record both
/// destinations are read from, the squash on the default branch (the advance's `merge`) and the
/// head the candidate advanced within.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Delivered {
    /// The head the pull request squashed, kept reachable by the land.
    pub(crate) head: String,
    /// The one parent of the squash.
    pub(crate) parent: String,
    /// The repository's number at its host, its name then, and the pull request's number.
    pub(crate) repository: i64,
    pub(crate) name: String,
    pub(crate) pull: i64,
    /// The revision of the evidence the merge was asked under.
    pub(crate) revision: String,
}

impl Delivered {
    fn of(receipt: &Receipt) -> Delivered {
        Delivered {
            head: receipt.head.clone(),
            parent: receipt.parent.clone(),
            repository: receipt.repository,
            name: receipt.name.clone(),
            pull: receipt.pull,
            revision: receipt.revision.clone(),
        }
    }
}

/// How a candidate ended: when, why, the release it became, and, for a drop, whether the run of
/// main's tip it makes due has not started yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ended {
    pub(crate) at: f64,
    pub(crate) why: String,
    #[serde(default)]
    pub(crate) tag: Option<String>,
    #[serde(default)]
    pub(crate) due: bool,
}

/// The candidate's bounds, `[releases] candidate_reruns` and `candidate_hours`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bounds {
    pub(crate) reruns: u32,
    pub(crate) hours: u32,
}

impl Bounds {
    /// The project's.
    pub(crate) fn of(releases: &symphony::project::Releases) -> Bounds {
        Bounds {
            reruns: releases.candidate_reruns,
            hours: releases.candidate_hours,
        }
    }

    /// The seconds a candidate is held.
    fn seconds(self) -> f64 {
        f64::from(self.hours) * 3600.0
    }
}

/// The candidate's record in the run's record folder `folder`.
pub(crate) fn path(folder: &Path) -> PathBuf {
    folder.join(FILE)
}

/// The candidate recorded in `folder`, none when there is no record. A record that does not read,
/// or of a newer version than [`VERSION`], is an error: passing it over would run main's tip beside
/// a candidate.
pub(crate) fn read(folder: &Path) -> Result<Option<Candidate>, String> {
    let path = path(folder);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "the release candidate's record cannot be read: {}",
                symphony::text::os_error(&e, &path)
            ));
        }
    };
    let version = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value["version"].as_u64());
    if let Some(version) = version.filter(|version| *version > u64::from(VERSION)) {
        return Err(format!(
            "the release candidate's record {} is of version {version}, a newer build's than this \
             one ({VERSION}): this build leaves it as it is and runs nothing beside it",
            path.display()
        ));
    }
    serde_json::from_str(&text).map(Some).map_err(|e| {
        format!(
            "the release candidate's record {} does not read: {e}",
            path.display()
        )
    })
}

/// Write `candidate` whole into `folder`.
pub(crate) fn write(folder: &Path, candidate: &Candidate) -> Result<(), String> {
    let path = path(folder);
    let text = serde_json::to_string_pretty(candidate).map_err(|e| e.to_string())? + "\n";
    symphony::text::write_whole(&path, &text).map_err(|e| symphony::text::os_error(&e, &path))
}

/// Whether the check or step `ran` is a red the candidate holds: a check's or a step's, never a
/// release's refresh.
fn holds(ran: &Ran) -> bool {
    !board::is_refresh(&ran.name)
}

impl Candidate {
    /// The candidate a red run `run` holds; its reds' tasks are [`Candidate::named`] once the
    /// board has them.
    pub(crate) fn of(run: &Run) -> Candidate {
        let mut candidate = Candidate {
            version: VERSION,
            state: State::Open,
            commit: run.commit.clone(),
            first: run.commit.clone(),
            first_run: run.id.clone(),
            since: run.started_at,
            rounds: 0,
            reds: Vec::new(),
            standing: Vec::new(),
            advances: Vec::new(),
            branch: None,
            settled_at: None,
            ended: None,
        };
        candidate.decided(run);
        candidate
    }

    /// Whether it is held.
    pub(crate) fn open(&self) -> bool {
        self.state == State::Open
    }

    /// Its reds not answered yet.
    pub(crate) fn waiting(&self) -> Vec<&Red> {
        self.reds
            .iter()
            .filter(|red| red.answer.is_none())
            .collect()
    }

    /// The reruns of it that came back red.
    pub(crate) fn red_reruns(&self) -> u32 {
        self.rounds.saturating_sub(1)
    }

    /// The round a rerun of it now is.
    pub(crate) fn next_round(&self) -> u32 {
        self.rounds + 1
    }

    /// A run of it, `run`, decided with a red: one more round, each check and step it ran standing
    /// as it ended, and each red it found held, its task named once the board has it
    /// ([`Candidate::named`]). A red of a key the candidate holds unanswered is that red.
    pub(crate) fn decided(&mut self, run: &Run) {
        self.rounds += 1;
        for ran in run.checks.iter().filter(|ran| holds(ran)) {
            self.stand(ran, run);
        }
        let reds = (run.checks.iter())
            .filter(|ran| holds(ran))
            .flat_map(steps::reds);
        for ran in reds {
            let key = board::key(&ran.name);
            let known = (self.reds.iter()).any(|red| red.answer.is_none() && red.key == key);
            if !known {
                self.reds.push(Red {
                    key,
                    name: ran.name.clone(),
                    slug: None,
                    round: self.rounds,
                    answer: None,
                    seen: None,
                });
            }
        }
    }

    /// Each unanswered red with no task yet named by the task the board `held` its key on.
    pub(crate) fn named(&mut self, held: &[Held]) {
        for red in (self.reds.iter_mut()).filter(|red| red.answer.is_none() && red.slug.is_none()) {
            red.slug = (held.iter())
                .find(|held| held.key == red.key)
                .map(|held| held.slug.clone());
        }
    }

    /// The check `ran` of the run `run`, standing as it ended, each green with the digest the run
    /// was told for it and the environment it ran on; a step or part it did not list this time
    /// keeps what it had.
    fn stand(&mut self, ran: &Ran, run: &Run) {
        let environment = run
            .environment
            .as_ref()
            .map(|environment| environment.said());
        let on = |name: &str| On {
            commit: run.commit.clone(),
            run: run.id.clone(),
            digest: (run.inputs.as_ref())
                .and_then(|inputs| inputs.of(name))
                .map(str::to_string),
            environment: environment.clone(),
        };
        let index = match self.standing.iter().position(|s| s.name == ran.name) {
            Some(index) => index,
            None => {
                self.standing.push(Standing {
                    name: ran.name.clone(),
                    green: None,
                    beyond: false,
                    steps: Vec::new(),
                });
                self.standing.len() - 1
            }
        };
        // What this run kept green of it instead of running it: a check or step it ran in part
        // is green by its parts, each on the commit and run it was green on.
        let carried = run.rerun.as_ref().map(|rerun| rerun.carried.as_slice());
        let in_part = |name: &str| {
            let under = format!("{name}/");
            carried.is_some_and(|carried| carried.iter().any(|c| c.name.starts_with(&under)))
        };
        let standing = &mut self.standing[index];
        standing.green = (ran.green() && !in_part(&ran.name)).then(|| on(&ran.name));
        standing.beyond = steps::beyond(ran);
        stand_each(&mut standing.steps, &ran.steps, &on, &in_part);
    }

    /// After a red rerun: dropped at `now` once it has had as many red reruns as `bounds` allow.
    /// Whether it was.
    pub(crate) fn after_red(&mut self, now: f64, bounds: Bounds) -> bool {
        if self.red_reruns() < bounds.reruns {
            return false;
        }
        let reruns = self.red_reruns();
        self.drop(
            now,
            format!(
                "{reruns} rerun{} came back red, as many as [releases] candidate_reruns allows",
                if reruns == 1 { "" } else { "s" }
            ),
        );
        true
    }

    /// Dropped at `now` for `why`: a run of main's tip is due.
    pub(crate) fn drop(&mut self, now: f64, why: String) {
        self.state = State::Dropped;
        self.ended = Some(Ended {
            at: now,
            why,
            tag: None,
            due: true,
        });
    }

    /// Released at `now` as `tag`.
    pub(crate) fn release(&mut self, now: f64, tag: &str) {
        self.state = State::Released;
        self.ended = Some(Ended {
            at: now,
            why: format!("released as {tag}"),
            tag: Some(tag.to_string()),
            due: false,
        });
    }

    /// The run of main's tip a drop made due has started.
    pub(crate) fn followed(&mut self) {
        if let Some(ended) = &mut self.ended {
            ended.due = false;
        }
    }

    /// Whether it was dropped and the run of main's tip that makes due has not started.
    pub(crate) fn due(&self) -> bool {
        self.state == State::Dropped && self.ended.as_ref().is_some_and(|ended| ended.due)
    }

    /// What the cadence holds for it: its rerun waiting for its reds, its rerun due, or, dropped,
    /// a run of main's tip due; none once that run started or it was released.
    pub(crate) fn hold(&self) -> Option<Hold> {
        match self.state {
            State::Open => {
                let waiting: Vec<String> = self.waiting().iter().map(|red| red.said()).collect();
                Some(if waiting.is_empty() {
                    Hold::Answered {
                        tip: self.commit.clone(),
                    }
                } else {
                    Hold::Waiting {
                        tip: self.commit.clone(),
                        reds: waiting,
                    }
                })
            }
            State::Dropped if self.due() => Some(Hold::Dropped {
                why: self
                    .ended
                    .as_ref()
                    .map(|e| e.why.clone())
                    .unwrap_or_default(),
            }),
            _ => None,
        }
    }

    /// How the rerun runs the check `name`: whole, some of its steps, or not at all, with what it
    /// carries green, each judged against `tip` ([`Tip::kept`]). A check with no standing, one
    /// that lists no steps, and one last red beyond its steps run whole; one whose own green is
    /// kept is carried whole. Else each step whose green is kept is carried, and each other runs:
    /// whole, or, when it listed parts and its red is theirs, by the parts that are not kept
    /// (`STEP/PART`), the rest of them carried. A step or part the tip tells inputs for that no
    /// run of the candidate listed runs too, a step told only by its parts among them (the tests
    /// step a run red in the runner's setup never listed), so nothing a run never reached is
    /// passed over. With nothing kept the check runs whole; with nothing to run it is carried, by
    /// its own green or by each step and part kept.
    pub(crate) fn plan(&self, name: &str, tip: &Tip<'_>) -> (Planned, Vec<Carried>) {
        let whole = (Planned::Whole, Vec::new());
        let Some(standing) = self.standing.iter().find(|s| s.name == name) else {
            return whole;
        };
        if let Some(kept) = tip.kept(name, standing.green.as_ref()) {
            return (Planned::Carried, vec![kept]);
        }
        if standing.steps.is_empty() || standing.beyond {
            return whole;
        }
        let prefix = format!("{name}/");
        let said = |full: &str| full.strip_prefix(&prefix).unwrap_or(full).to_string();
        let mut carried: Vec<Carried> = Vec::new();
        let mut run: Vec<String> = Vec::new();
        for step in &standing.steps {
            if let Some(kept) = tip.kept(&step.name, step.green.as_ref()) {
                carried.push(kept);
                continue;
            }
            // It is judged by its parts when it listed some, unless it was red beyond them: a red
            // none of them holds is its own, and it runs whole.
            let theirs = !step.parts.is_empty() && !step.beyond;
            let mut kept: Vec<Carried> = Vec::new();
            let mut parts: Vec<String> = Vec::new();
            if theirs {
                for part in &step.parts {
                    match tip.kept(&part.name, part.green.as_ref()) {
                        Some(part) => kept.push(part),
                        None => parts.push(said(&part.name)),
                    }
                }
                parts.extend(
                    tip.unlisted(&step.name, &step.parts)
                        .iter()
                        .map(|n| said(n)),
                );
            }
            if kept.is_empty() {
                run.push(said(&step.name));
            } else {
                carried.extend(kept);
                run.extend(parts);
            }
        }
        run.extend(tip.unlisted(name, &standing.steps).iter().map(|n| said(n)));
        if carried.is_empty() {
            return whole;
        }
        // Nothing to run: every step it listed is kept, so none was red, and its red was not
        // beyond them. It was green, whole or by its steps, and is carried by them.
        if run.is_empty() {
            return (Planned::Carried, carried);
        }
        (Planned::Steps(run), carried)
    }

    /// Why the rerun judged against `tip` carries less than a reader may expect, a line each, for
    /// its output: the tip's tree tells no inputs, or could not; a green stands on another
    /// environment; a green that has a digest has none at the tip.
    pub(crate) fn uncarried(&self, tip: &Tip<'_>) -> Vec<String> {
        let mut said: Vec<String> = Vec::new();
        let only = "a green is carried only from a commit of this tree";
        match tip.inputs {
            None => said.push(format!(
                "this tree has no {}, so no step's inputs are told: {only}",
                super::inputs::SCRIPT
            )),
            Some(Inputs {
                error: Some(why), ..
            }) => said.push(format!("no step's inputs can be told ({why}): {only}")),
            Some(_) => {}
        }
        let mut greens: Vec<(&str, &On)> = Vec::new();
        for standing in &self.standing {
            greens.extend(
                standing
                    .green
                    .as_ref()
                    .map(|on| (standing.name.as_str(), on)),
            );
            for step in &standing.steps {
                greens.extend(step.green.as_ref().map(|on| (step.name.as_str(), on)));
                for part in &step.parts {
                    greens.extend(part.green.as_ref().map(|on| (part.name.as_str(), on)));
                }
            }
        }
        let elsewhere = (greens.iter())
            .filter(|(_, on)| on.environment.as_deref() != tip.environment)
            .count();
        if elsewhere > 0 {
            said.push(format!(
                "{elsewhere} green{} of the candidate stood on another environment than this \
                 one ({}): none of them is carried",
                if elsewhere == 1 { "" } else { "s" },
                tip.environment.unwrap_or("unknown")
            ));
        }
        if let Some(inputs) = tip.inputs.filter(|inputs| inputs.error.is_none()) {
            let unnamed: Vec<&str> = (greens.iter())
                .filter(|(name, on)| on.digest.is_some() && inputs.of(name).is_none())
                .map(|(name, _)| *name)
                .collect();
            if !unnamed.is_empty() {
                said.push(format!(
                    "this tree tells no inputs for {}: {only}",
                    unnamed.join(", ")
                ));
            }
        }
        said
    }

    /// What `--status` says of it at `now`: open, its tip and first commit, its age and round, each
    /// red with its task and answer, and what its rerun waits for; ended, how.
    pub(crate) fn status(&self, now: f64, bounds: Bounds) -> String {
        let first = format!(
            "first judged {} by the run {} ({}, {})",
            short(&self.first),
            self.first_run,
            local(self.since),
            age(now, self.since)
        );
        match self.state {
            State::Open => {
                let mut text = format!(
                    "release candidate: {}, {first}; {} run{} of it, {} red rerun{} of at most \
                     {}; dropped at {} unless released\n",
                    short(&self.commit),
                    self.rounds,
                    if self.rounds == 1 { "" } else { "s" },
                    self.red_reruns(),
                    if self.red_reruns() == 1 { "" } else { "s" },
                    bounds.reruns,
                    local(self.since + bounds.seconds()),
                );
                for red in &self.reds {
                    text.push_str(&format!("  {}\n", red.status()));
                }
                let waiting: Vec<String> = self.waiting().iter().map(|red| red.said()).collect();
                if waiting.is_empty() {
                    text.push_str(
                        "  rerun: due, every red answered: the next landing starts it, or `just \
                         release --rerun`\n",
                    );
                } else {
                    text.push_str(&format!("  rerun: waits for {}\n", waiting.join(", ")));
                }
                if let Some(at) = self.settled_at {
                    text.push_str(&format!("  as last settled, {}\n", local(at)));
                }
                text
            }
            State::Released | State::Dropped => {
                let Some(ended) = &self.ended else {
                    return String::new();
                };
                let then = if self.due() {
                    "; a run of main's tip is due"
                } else {
                    ""
                };
                format!(
                    "last release candidate: {}, {} ({}): {}{then}; {first}, {} red run{} of it\n",
                    if self.state == State::Released {
                        "released"
                    } else {
                        "dropped"
                    },
                    local(ended.at),
                    age(now, ended.at),
                    ended.why,
                    self.rounds,
                    if self.rounds == 1 { "" } else { "s" },
                )
            }
        }
    }

    /// What a release of it says in its message: its first commit and run, its reruns, each red's
    /// task and how it was answered, and what its last rerun `carried` green.
    pub(crate) fn notes(&self, carried: &[Carried]) -> Vec<String> {
        let reruns = self.rounds;
        let mut lines = vec![format!(
            "Release candidate: first judged {} by the run {} ({}), released after {reruns} \
             rerun{}.",
            short(&self.first),
            self.first_run,
            local(self.since),
            if reruns == 1 { "" } else { "s" }
        )];
        if !self.reds.is_empty() {
            lines.push("Its reds and how each was answered:".into());
            for red in &self.reds {
                let task = red.slug.as_deref().unwrap_or("no task");
                let how = match &red.answer {
                    Some(Answer::Landed { commit, .. }) => {
                        format!("advanced it to {}", short(commit))
                    }
                    Some(Answer::Covered { task, commit, .. }) => {
                        format!(
                            "covered by {task}'s land, which advanced it to {}",
                            short(commit)
                        )
                    }
                    Some(Answer::Closed { .. }) => "answered outside this repository".into(),
                    None => "green in the rerun".into(),
                };
                lines.push(format!("- {task} ({}): {how}", red.name));
            }
        }
        let (by_inputs, by_tree): (Vec<&Carried>, Vec<&Carried>) =
            carried.iter().partition(|kept| kept.digest.is_some());
        if !by_tree.is_empty() {
            lines.push("Carried green from a commit whose tree is this one's:".into());
            for kept in by_tree {
                lines.push(format!(
                    "- {}: green on {} in the run {}",
                    kept.name,
                    short(&kept.commit),
                    kept.run
                ));
            }
        }
        if !by_inputs.is_empty() {
            lines.push(
                "Carried green from a run of this candidate, not run on this commit: its \
                 inputs' digest is the one it was green on:"
                    .into(),
            );
            for kept in by_inputs {
                lines.push(format!(
                    "- {}: green on {} in the run {}, inputs {}",
                    kept.name,
                    short(&kept.commit),
                    kept.run,
                    kept.digest.as_deref().unwrap_or_default()
                ));
            }
        }
        lines
    }
}

/// How a rerun runs a check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Planned {
    /// Its command alone: every step.
    Whole,
    /// Its command followed by these steps' names, as `just verify STEP...` takes them.
    Steps(Vec<String>),
    /// Not at all: green on a commit whose tree is the tip's, or on these same inputs.
    Carried,
}

/// Each of `listed`, standing in `known` as it ended, its green as `on` says it for its name; one
/// not listed keeps what it had, and so do the parts of one that listed none this time. One the
/// run ran `in_part`, some of its parts carried, stands green by its parts alone.
fn stand_each(
    known: &mut Vec<StepStanding>,
    listed: &[Ran],
    on: &dyn Fn(&str) -> On,
    in_part: &dyn Fn(&str) -> bool,
) {
    for step in listed {
        let green = (step.green() && !in_part(&step.name)).then(|| on(&step.name));
        let index = match known.iter().position(|s| s.name == step.name) {
            Some(index) => index,
            None => {
                known.push(StepStanding {
                    name: step.name.clone(),
                    green: None,
                    beyond: false,
                    parts: Vec::new(),
                });
                known.len() - 1
            }
        };
        known[index].green = green;
        stand_each(&mut known[index].parts, &step.steps, on, in_part);
        // Red while no part it listed is, of a step that has parts: its red is its own.
        known[index].beyond =
            !step.green() && !known[index].parts.is_empty() && step.steps.iter().all(Ran::green);
    }
}

impl Red {
    /// It, said by its task (its key while it has none).
    pub(crate) fn said(&self) -> String {
        match &self.slug {
            Some(slug) => slug.clone(),
            None => format!("{} (no task: the board did not file it)", self.key),
        }
    }

    /// Its line in `--status`.
    fn status(&self) -> String {
        let how = match &self.answer {
            Some(Answer::Landed { merge, commit }) => format!(
                "answered: its land {} advanced the candidate to {}",
                short(merge),
                short(commit)
            ),
            Some(Answer::Covered {
                task,
                merge,
                commit,
            }) => format!(
                "answered: its task closed as covered by {task}'s land {}, which advanced the \
                 candidate to {}",
                short(merge),
                short(commit)
            ),
            Some(Answer::Closed { status }) => format!(
                "answered: its task {status} with no landing, its fix made outside this repository"
            ),
            None => format!(
                "open ({})",
                self.seen.as_deref().unwrap_or("not seen on the board yet")
            ),
        };
        format!(
            "{} ({}, round {}): {how}",
            self.said(),
            self.name,
            self.round
        )
    }
}

/// What a settle reads of the board.
pub(crate) trait Board {
    /// The status of the task `slug`; none when the board has no such task.
    fn status(&self, slug: &str) -> Result<Option<String>, String>;
    /// The texts naming the commits that vouch for a land of `slug`, oldest first ([`approving`]).
    fn approves(&self, slug: &str) -> Result<Vec<String>, String>;
    /// The landing `slug` was closed as covered by, when its thread's last verdict is a covered
    /// close's line (`just tasks done SLUG NOTE --covered-by OTHER`).
    fn covered(&self, slug: &str) -> Result<Option<Covered>, String>;
    /// The open red task of the key `key`, when the board has one.
    fn open_red(&self, key: &str) -> Result<Option<String>, String>;
}

/// The landing a task was closed as covered by: the covering task and the commit its landing
/// names, shortened as the board's line shortens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Covered {
    pub(crate) task: String,
    pub(crate) commit: String,
}

impl Board for CliBoard {
    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        Reds::status(self, slug)
    }

    fn approves(&self, slug: &str) -> Result<Vec<String>, String> {
        Ok(approving(&thread(self, slug)?))
    }

    fn covered(&self, slug: &str) -> Result<Option<Covered>, String> {
        Ok(covering(&thread(self, slug)?))
    }

    fn open_red(&self, key: &str) -> Result<Option<String>, String> {
        board::open(self, key)
    }
}

/// The messages of `slug`'s thread, as the board shapes them, oldest first.
fn thread(board: &CliBoard, slug: &str) -> Result<Vec<serde_json::Value>, String> {
    let thread = board
        .call("thread", &json!({"slug": slug}))
        .map_err(|e| e.0)?;
    Ok(match thread {
        serde_json::Value::Array(messages) => messages,
        _ => Vec::new(),
    })
}

/// The landing a thread's `messages` say their task was closed as covered by: its last verdict,
/// when that is a covered close's line ([`symphony_board::covered_by`]).
fn covering(messages: &[serde_json::Value]) -> Option<Covered> {
    let last = (messages.iter().rev())
        .filter(|message| message["kind"] == "verdict")
        .find_map(|message| message["text"].as_str())?;
    let line = symphony_board::covered_by(last)?;
    Some(Covered {
        task: line.other.to_string(),
        commit: line.commit.to_string(),
    })
}

/// The texts naming the commits that vouch for a land among a thread's `messages`, as the board
/// shapes them: a verdict whose text is `approve`, or `approve:` and why; and, of a task that
/// needed no review, the tip its landing's line (a verdict starting `landed `, never another that
/// quotes one) says its needed stamps stood on ([`symphony_board::land::needs::stood_at`]), which
/// no approve names.
fn approving(messages: &[serde_json::Value]) -> Vec<String> {
    (messages.iter())
        .filter(|message| message["kind"] == "verdict")
        .filter_map(|message| message["text"].as_str())
        .filter_map(|text| {
            let approve = text
                .strip_prefix("approve")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'));
            if approve {
                Some(text)
            } else if text.starts_with("landed ") {
                symphony_board::land::needs::stood_at(text)
            } else {
                None
            }
        })
        .map(str::to_string)
        .collect()
}

/// The line a settle reads: the repository, its default branch, the remote's main as last fetched
/// or pushed and the main checkout's own default branch, each a commit, and the newest release
/// tagged here.
pub(crate) struct Line<'a> {
    pub(crate) root: &'a Path,
    pub(crate) branch: &'a str,
    /// The ref the remote's main is read from, as said (`origin/main`), and its commit.
    pub(crate) remote_name: &'a str,
    pub(crate) remote: &'a str,
    pub(crate) local: Option<&'a str>,
    pub(crate) newest: Option<(&'a str, &'a str)>,
}

/// Settle the open `candidate` at `now`: each red whose task landed on the remote's main advances
/// it ([`advance`]), oldest landing first, or drops it when no approved commit passes; a land the
/// remote's main lacks yet waits for its push; a task closed as covered by another's landing is
/// read by [`covering_says`]; a task cancelled with no landing answers its red; then its bounds.
/// What it did, each said.
pub(crate) fn settle(
    candidate: &mut Candidate,
    line: &Line<'_>,
    board: &dyn Board,
    now: f64,
    bounds: Bounds,
) -> Vec<String> {
    let mut noted = Vec::new();
    if !candidate.open() {
        return noted;
    }
    for red in (candidate.reds.iter_mut()).filter(|red| red.slug.is_none() && red.answer.is_none())
    {
        match board.open_red(&red.key) {
            Ok(found) => red.slug = found,
            Err(error) => noted.push(format!("the board: {error}")),
        }
    }
    let receipts = Receipts::read(line.root);
    let landed = cadence::landings_by(
        line.root,
        &receipts,
        Some(&candidate.first),
        line.remote,
        line.branch,
    )
    .unwrap_or_else(|error| {
        noted.push(format!(
            "the landings since its first commit cannot be read: {error}"
        ));
        Vec::new()
    });
    for landing in landed.iter().rev() {
        let answers =
            |red: &Red| red.answer.is_none() && red.slug.as_deref() == Some(landing.slug.as_str());
        if !candidate.reds.iter().any(answers) {
            continue;
        }
        let from = candidate.commit.clone();
        let receipt = receipts.squashed(&landing.commit);
        match advance(
            line.root,
            &from,
            &landing.commit,
            receipt,
            board,
            &landing.slug,
        ) {
            Ok(Some(to)) => {
                for red in candidate.reds.iter_mut().filter(|red| answers(red)) {
                    red.answer = Some(Answer::Landed {
                        merge: landing.commit.clone(),
                        commit: to.clone(),
                    });
                    red.seen = None;
                }
                noted.push(format!(
                    "advanced the release candidate from {} to {}, the newest approved commit of \
                     {}'s land ({}){}",
                    short(&from),
                    short(&to),
                    landing.slug,
                    short(&landing.commit),
                    receipt.map_or_else(String::new, |receipt| format!(
                        ", the squash of {} by pull request #{} of {}",
                        short(&receipt.head),
                        receipt.pull,
                        receipt.name
                    ))
                ));
                candidate.advances.push(Advance {
                    slug: landing.slug.clone(),
                    from,
                    to: to.clone(),
                    merge: landing.commit.clone(),
                    delivered: receipt.map(Delivered::of),
                });
                candidate.commit = to;
            }
            Ok(None) => {
                let why = format!(
                    "{} landed ({}) with no approved commit that descends from the candidate ({}) \
                     and holds beyond it nothing main held before the land",
                    landing.slug,
                    short(&landing.commit),
                    short(&from)
                );
                noted.push(format!("dropped the release candidate: {why}"));
                candidate.drop(now, why);
                candidate.settled_at = Some(now);
                return noted;
            }
            Err(error) => noted.push(format!(
                "{}'s land ({}) cannot be read, so it waits: {error}",
                landing.slug,
                short(&landing.commit)
            )),
        }
    }
    let pushed: Vec<&str> = landed
        .iter()
        .map(|landing| landing.commit.as_str())
        .collect();
    let here = match line.local.filter(|local| *local != line.remote) {
        Some(local) => cadence::landings_by(
            line.root,
            &receipts,
            Some(&candidate.first),
            local,
            line.branch,
        )
        .unwrap_or_default(),
        None => Vec::new(),
    };
    // What a red closed as covered by another's landing is read against: the landings that
    // advanced the candidate, and the reds' tasks that may yet.
    let advances = candidate.advances.clone();
    let unanswered: Vec<String> = (candidate.reds.iter())
        .filter(|red| red.answer.is_none())
        .filter_map(|red| red.slug.clone())
        .collect();
    let mut dropped = None;
    for red in candidate.reds.iter_mut().filter(|red| red.answer.is_none()) {
        let Some(slug) = red.slug.clone() else {
            continue;
        };
        let unpushed = here
            .iter()
            .find(|landing| landing.slug == slug && !pushed.contains(&landing.commit.as_str()));
        if let Some(landing) = unpushed {
            noted.push(format!(
                "{slug} landed here ({}), but {} does not hold it yet: the release candidate waits \
                 for its push",
                short(&landing.commit),
                line.remote_name
            ));
            red.seen = Some("landed here, not pushed".into());
            continue;
        }
        match board.status(&slug) {
            // A red's task cancelled with no landing: its fix was made outside this repository.
            // One done is approved, and waits for its land.
            Ok(Some(status)) if status == "cancelled" => {
                noted.push(format!(
                    "{slug} is cancelled with no landing: its red is answered, its fix made outside \
                     this repository"
                ));
                red.answer = Some(Answer::Closed { status });
                red.seen = None;
            }
            // One done is approved and waits for its land, unless it was closed as covered by
            // another's landing, which answers it, waits or drops the candidate.
            Ok(Some(status)) if status == "done" => match board.covered(&slug) {
                Ok(None) => red.seen = Some("approved, waiting for its land".into()),
                Ok(Some(covered)) => match covering_says(&slug, &covered, &advances, &unanswered) {
                    Covering::Answered(answer, said) => {
                        noted.push(said);
                        red.answer = Some(answer);
                        red.seen = None;
                    }
                    Covering::Waits(seen) => red.seen = Some(seen),
                    Covering::Drops(why) => {
                        dropped = Some(why);
                        break;
                    }
                },
                Err(error) => noted.push(format!("the board: {error}")),
            },
            Ok(Some(status)) => red.seen = Some(status),
            Ok(None) => red.seen = Some("not on the board".into()),
            Err(error) => noted.push(format!("the board: {error}")),
        }
    }
    if let Some(why) = dropped {
        noted.push(format!("dropped the release candidate: {why}"));
        candidate.drop(now, why);
        candidate.settled_at = Some(now);
        return noted;
    }
    match bound(candidate, line, &receipts, now, bounds) {
        Some(Bound::Dropped(why)) => {
            noted.push(format!("dropped the release candidate: {why}"));
            candidate.drop(now, why);
        }
        Some(Bound::Released(tag)) => {
            noted.push(format!(
                "the release candidate's tip {} is {tag} already",
                short(&candidate.commit)
            ));
            candidate.release(now, &tag);
        }
        None => {}
    }
    candidate.settled_at = Some(now);
    noted
}

/// What a red closed as covered by another's landing makes of the candidate.
enum Covering {
    /// Answered, said: the covering landing advanced the candidate.
    Answered(Answer, String),
    /// Unanswered, seen so: the covering task is a red's whose land has not advanced it yet.
    Waits(String),
    /// Dropped, for this reason: its fix landed on main and not in the candidate.
    Drops(String),
}

/// What the red task `slug`, closed as `covered`, makes of the candidate: answered when the
/// covering landing is one of its `advances`, whose fix holds this red's too; waiting while the
/// covering task is one of the `unanswered` reds' tasks, whose land advances it once pushed; else
/// dropped, as the fix is on main and not in the candidate, as a red's land with no approved
/// commit that passes drops it.
fn covering_says(
    slug: &str,
    covered: &Covered,
    advances: &[Advance],
    unanswered: &[String],
) -> Covering {
    let advanced = advances
        .iter()
        .find(|advance| advance.slug == covered.task && advance.merge.starts_with(&covered.commit));
    if let Some(advance) = advanced {
        return Covering::Answered(
            Answer::Covered {
                task: advance.slug.clone(),
                merge: advance.merge.clone(),
                commit: advance.to.clone(),
            },
            format!(
                "{slug} is closed as covered by {}'s land ({}), which advanced the release \
                 candidate to {}: its red is answered",
                advance.slug,
                short(&advance.merge),
                short(&advance.to)
            ),
        );
    }
    if unanswered.contains(&covered.task) {
        return Covering::Waits(format!(
            "closed as covered by {}, whose land has not advanced the candidate yet",
            covered.task
        ));
    }
    Covering::Drops(format!(
        "{slug} was closed as covered by {}'s land ({}), which did not advance the candidate: its \
         fix is on main, not in the candidate",
        covered.task, covered.commit
    ))
}

/// What a candidate's bounds found.
enum Bound {
    Dropped(String),
    Released(String),
}

/// What ends `candidate` at `now`, by its time or a release: its hours past since it was first
/// judged; its tip released already; a newer release it does not hold (one on main passed it, and
/// a release only moves forward); or its tip off the remote's main. A tip or a release that is a
/// head a pull request delivered is held by what holds its squash, by `receipts`
/// ([`Receipts::holds`]), as no history of the line holds the head itself.
fn bound(
    candidate: &Candidate,
    line: &Line<'_>,
    receipts: &Receipts,
    now: f64,
    bounds: Bounds,
) -> Option<Bound> {
    if now - candidate.since >= bounds.seconds() {
        let hours = ((now - candidate.since) / 3600.0).floor();
        return Some(Bound::Dropped(format!(
            "it was first judged {hours} hour{} ago, at {}: [releases] candidate_hours holds one \
             {}",
            if (hours - 1.0).abs() < f64::EPSILON {
                ""
            } else {
                "s"
            },
            local(candidate.since),
            bounds.hours
        )));
    }
    if let Some((name, at)) = line.newest {
        if at == candidate.commit {
            return Some(Bound::Released(name.to_string()));
        }
        if !(receipts.holds(line.root, at, &candidate.commit)).unwrap_or(true) {
            return Some(Bound::Dropped(format!(
                "the release {name} ({}) passed it: its tip {} does not descend from it, and a \
                 release only moves forward",
                short(at),
                short(&candidate.commit)
            )));
        }
    }
    if !(receipts.holds(line.root, &candidate.commit, line.remote)).unwrap_or(true) {
        return Some(Bound::Dropped(format!(
            "its tip {} is not on {}",
            short(&candidate.commit),
            line.remote_name
        )));
    }
    None
}

/// The commit the land `merge` of the task `slug` advances the candidate's tip `tip` to: the newest
/// of the commits the approves on its thread name that [`passes`]; none when none does. A land a
/// pull request delivered is read from its `receipt`, never from the squash, which has no second
/// parent: main before the land is the squash's parent, what landed is the head the receipt kept,
/// and that head is itself a commit that vouches, as the squash was asked for under evidence that
/// stood on it, whether or not the board has closed the task yet. So the candidate takes the
/// branch's own commits and never main's, however far main moved under the pull request.
fn advance(
    root: &Path,
    tip: &str,
    merge: &str,
    receipt: Option<&Receipt>,
    board: &dyn Board,
    slug: &str,
) -> Result<Option<String>, String> {
    let (before, landed) = match receipt {
        Some(receipt) => (commit(root, &receipt.parent)?, commit(root, &receipt.head)?),
        None => (
            commit(root, &format!("{merge}^1"))?,
            commit(root, &format!("{merge}^2"))?,
        ),
    };
    let approved = board.approves(slug)?;
    let named = (approved.iter())
        .flat_map(|text| symphony_board::land::named(text))
        .chain(receipt.map(|receipt| receipt.head.as_str()));
    let mut passing: Vec<(u64, String)> = Vec::new();
    for word in named {
        let Ok(named) = commit(root, word) else {
            continue;
        };
        if passing.iter().any(|(_, known)| *known == named) {
            continue;
        }
        if passes(root, tip, &before, &landed, &named)? {
            passing.push((count(root, &[&named])?, named));
        }
    }
    Ok(passing
        .into_iter()
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, named)| named))
}

/// Whether `named` may be the candidate's tip after a land of `landed` onto main at `before`: it
/// is `landed` or behind it, it descends from the candidate's tip `tip` (and is not that tip), and
/// every commit it holds beyond the tip is the branch's own, none that main held before the land.
pub(crate) fn passes(
    root: &Path,
    tip: &str,
    before: &str,
    landed: &str,
    named: &str,
) -> Result<bool, String> {
    if named == tip || !is_ancestor(root, named, landed)? || !is_ancestor(root, tip, named)? {
        return Ok(false);
    }
    let not_tip = format!("^{tip}");
    let beyond = count(root, &[named, &not_tip])?;
    let own = count(root, &[named, &not_tip, &format!("^{before}")])?;
    Ok(beyond == own)
}

/// How many commits `git rev-list` names for `revs`.
fn count(root: &Path, revs: &[&str]) -> Result<u64, String> {
    let mut args = vec!["rev-list", "--count"];
    args.extend_from_slice(revs);
    let counted = git(root, &args)?;
    counted
        .parse()
        .map_err(|_| format!("git rev-list --count said {counted:?}"))
}

/// Whether the commits `a` and `b` hold the same tree in `root`.
pub(crate) fn same_tree(root: &Path, a: &str, b: &str) -> bool {
    let tree = |rev: &str| {
        git(
            root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{tree}}"),
            ],
        )
    };
    matches!((tree(a), tree(b)), (Ok(a), Ok(b)) if a == b)
}

/// Bring [`BRANCH`] in `root` to what `candidate` says: its tip while open, gone once it ended. It
/// moves only from the value the run last set (or from none, made anew), never while a worktree has
/// it checked out, so a branch of that name someone else made or uses is left as it is. What it
/// could not do, said; the record is the truth either way.
pub(crate) fn steer(root: &Path, candidate: &mut Candidate) -> Option<String> {
    let want = candidate.open().then(|| candidate.commit.clone());
    let now = commit(root, REFERENCE).ok();
    if now == want {
        candidate.branch = want;
        return None;
    }
    if let Some(now) = &now
        && candidate.branch.as_ref() != Some(now)
    {
        return Some(format!(
            "the branch {BRANCH} is at {}, which the release run did not set it to: it is left as \
             it is{}",
            short(now),
            want.as_ref()
                .map(|tip| format!(", and the candidate's tip is {}", short(tip)))
                .unwrap_or_default()
        ));
    }
    let checked_out = git(root, &["worktree", "list", "--porcelain"]).is_ok_and(|listed| {
        listed
            .lines()
            .any(|line| line == format!("branch {REFERENCE}"))
    });
    if checked_out {
        return Some(format!(
            "the branch {BRANCH} is checked out in a worktree, so the release run leaves it where it \
             is; the candidate's tip is {}",
            want.as_deref().map_or("none, as it ended", short)
        ));
    }
    let old = now.clone().unwrap_or_default();
    let moved = match &want {
        Some(tip) => git(
            root,
            &[
                "update-ref",
                "-m",
                "the release run: the release candidate",
                REFERENCE,
                tip,
                &old,
            ],
        ),
        None => git(root, &["update-ref", "-d", REFERENCE, &old]),
    };
    match moved {
        Ok(_) => {
            candidate.branch = want;
            None
        }
        Err(error) => Some(format!("cannot move the branch {BRANCH}: {error}")),
    }
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
