//! What a land requires of a task (docs/workflow/analysis/needed-stamps.md, "The land's check"): the stamps
//! it needs at its branch's tip, each read there by the board's own reader ([`crate::stamps::read`]),
//! never a copy of one.
//!
//! 1. **Which stamps** ([`Needed::of`]): the newest assessment's, on the tip it read; on another
//!    tip, or none (made from the brief), those and the catalogue's defaults; with no assessment,
//!    the defaults ([`read::needed`]).
//! 2. **Ready**: where `review` is needed, done by an approve, the land's approval check as before
//!    stamps; where it is not, in review or blocked, or done by an approve ([`not_ready`]).
//! 3. **Standing on the tip** ([`standing`]): `review` by the land's approval check, each other
//!    stamp by its reader. A missing one refuses, naming each and who gives it ([`refusal`]).
//!
//! The task's gate decisions ([`crate::gates`]) are read with them: a waiver for the tip lifts its
//! stamp there ([`Needed::of`]), one for another tip lifts nothing and the refusal names it, and
//! each raise must be carried in its file at the tip ([`raises`]).

use super::short;
use crate::cli::Env;
use crate::cli::machine_work::recorded::{self, Record};
use crate::gates::{self, Decision, Kind, Limit, grouped};
use crate::stamps::{Catalogue, Needs, family, read, said};
use crate::{Message, Store, Task};
use std::path::{Path, PathBuf};

/// The stamp the land's own approval check reads: a fresh reviewer's approve covering the tip.
pub const REVIEW: &str = "review";
/// The stamp a task's recorded runs at the tip meet.
pub const TESTS: &str = "tests";
/// The stamp a post naming the tip and showing a picture meets.
pub const PICTURES: &str = "pictures";
/// The stamp the owner's approve of the tip meets.
pub const OWNER: &str = "owner";

/// The stamps a land requires of a task at its branch's tip, and the assessment they came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Needed {
    /// In the catalogue's order.
    pub stamps: Vec<String>,
    /// The task's assessment, when one was written.
    pub needs: Option<Needs>,
    /// The task's gate decisions that stand ([`crate::Task::gates`]).
    pub decisions: Vec<Decision>,
    /// The waivers that lifted a stamp at the tip, which `stamps` leaves out.
    pub waived: Vec<Decision>,
    /// The waivers of a stamp it needs given for another tip, which lift nothing there.
    pub elsewhere: Vec<Decision>,
}

impl Needed {
    /// What `task` needs at `tip` (its branch's; none before it has one), from the catalogue the
    /// main checkout `main` commits ([`Catalogue::of`]) and its assessment ([`read::needed`]). Each
    /// task is judged by its assessment and the catalogue.
    ///
    /// A waiver among its gate decisions for `tip` lifts its stamp; one for another tip lifts
    /// nothing ([`gates::waivers`]).
    pub fn of(task: &Task, main: &Path, tip: Option<&str>) -> Result<Needed, String> {
        Needed::under(task, main, "HEAD", tip)
    }

    /// [`Needed::of`] by the catalogue the commit `commit` holds ([`Catalogue::committed`]), not
    /// the main checkout's head: what the task needed before its own merge moved that head.
    pub fn under(
        task: &Task,
        main: &Path,
        commit: &str,
        tip: Option<&str>,
    ) -> Result<Needed, String> {
        let catalogue = Catalogue::committed(main, commit)?;
        let needs = task.needs();
        let mut stamps = read::needed(&catalogue, needs.as_ref(), tip);
        let decisions = task.gates();
        let (here, elsewhere) = gates::waivers(&decisions, &catalogue, &stamps, tip);
        let (waived, elsewhere): (Vec<Decision>, Vec<Decision>) = (
            here.into_iter().cloned().collect(),
            elsewhere.into_iter().cloned().collect(),
        );
        stamps.retain(|stamp| !waived.iter().any(|waiver| waiver.gate == *stamp));
        Ok(Needed {
            stamps,
            needs,
            decisions,
            waived,
            elsewhere,
        })
    }

    /// Whether it needs a fresh reviewer's approve.
    pub fn review(&self) -> bool {
        self.has(REVIEW)
    }

    fn has(&self, name: &str) -> bool {
        self.stamps.iter().any(|stamp| stamp == name)
    }

    /// The stamp `name` needed too, whatever the assessment and the catalogue name: the tests of
    /// a tip whose approve stands over a merge it did not read ([`super::carried`]).
    pub fn require(&mut self, name: &str) {
        if !self.has(name) {
            self.stamps.push(name.to_string());
        }
    }

    /// Whether the land says what the task needs: it was assessed, or needs more than `review`,
    /// the one stamp a land went by before stamps.
    pub fn told(&self) -> bool {
        self.needs.is_some() || self.stamps != [REVIEW] || !self.waived.is_empty()
    }

    /// Why its assessment does not stand on `tip`: it read another commit, or the brief alone, so
    /// the catalogue's defaults stand beside the stamps it names. None with no assessment, or one
    /// of the tip.
    pub fn stale(&self, tip: &str) -> Option<String> {
        let needs = self.needs.as_ref()?;
        let read = match needs.tip.as_deref() {
            Some(read) if read == tip => return None,
            Some(read) => short(read).to_string(),
            None => "the brief alone".into(),
        };
        Some(format!(
            "its assessment by {} read {read}, not the tip {}, so the catalogue's defaults stand \
             beside the stamps it names: an assessment lowers what a task needs only on the tip it \
             read",
            needs.by,
            short(tip)
        ))
    }

    /// The land's line saying what `slug` needs at `tip` and from where, then each stamp of `met`
    /// that stands, with its reader's detail.
    pub fn line(&self, slug: &str, tip: &str, met: &[(String, String)]) -> String {
        let whose = match &self.needs {
            Some(needs) if needs.tip.as_deref() == Some(tip) => {
                format!("its assessment by {}", needs.by)
            }
            Some(needs) => format!(
                "its assessment by {} {} and the catalogue's defaults",
                needs.by,
                needs.read()
            ),
            None => "the catalogue's defaults, no assessment".into(),
        };
        let mut line = format!(
            "land {slug}: it needs {} at {} ({whose})",
            said(&self.stamps),
            short(tip)
        );
        for (name, detail) in met {
            line.push_str(&format!("; {name}: {detail}"));
        }
        for waiver in &self.waived {
            line.push_str(&format!(
                "; {} waived by the gate decision of {}: {}",
                waiver.gate, waiver.by, waiver.why
            ));
        }
        line
    }

    /// What a missing stamp `name`'s detail adds where a waiver of it was given for another tip
    /// than `tip`: that it lifts nothing here.
    fn waived_elsewhere(&self, name: &str, tip: &str) -> Option<String> {
        let waiver = self.elsewhere.iter().find(|waiver| waiver.gate == name)?;
        Some(format!(
            "its waiver by {} was for {}, not the tip {}: a waiver is for one tip alone",
            waiver.by,
            short(waiver.tip.as_deref().unwrap_or_default()),
            short(tip)
        ))
    }

    /// What the landing's line of a task that needed no review adds: the stamps it needed and the
    /// whole `tip` they stood on, the commit its land vouched for, as an approve names one. The
    /// release run's candidate moves to it as to an approved commit ([`stood_at`]).
    pub fn stood(&self, tip: &str) -> String {
        format!("{STOOD}{}{STOOD_AT}{tip}", said(&self.stamps))
    }
}

/// How a landing's line says the stamps a task needed and the tip they stood on ([`Needed::stood`]).
const STOOD: &str = "; its needs (";
const STOOD_AT: &str = ") stood at ";

/// The tip a landing's `line` says the needed stamps stood on ([`Needed::stood`]); none on any
/// other line, an approve's or a landing's that went by one among them.
pub fn stood_at(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once(STOOD)?;
    let (_, rest) = rest.split_once(STOOD_AT)?;
    super::named(rest).first().copied()
}

/// Why a task in `status` that needs no review is not ready to land, said to whom; none when it is
/// (in review or blocked: its executor's move to review says the work is ready). A task done is
/// judged by whether an approve stands on it, as the approval check reads it.
pub fn not_ready(slug: &str, status: &str) -> Option<(String, Option<&'static str>)> {
    match status {
        "review" | "blocked" | "done" => None,
        "cancelled" => Some((format!("{slug} is cancelled: nothing of it lands"), None)),
        _ => Some((
            format!(
                "{slug} is {status}: it needs no review, so it lands once its executor says the \
                 work is ready by moving it to review (`just tasks move {slug} review`)"
            ),
            Some("@executor"),
        )),
    }
}

/// What the stamps a land reads stand on.
pub struct Evidence<'a> {
    pub thread: &'a [Message],
    /// The recorder's runs, read only when `tests` is needed.
    pub runs: Vec<Record>,
    /// The task and the work under it, whose runs are its own ([`family`]).
    pub ours: Vec<String>,
    /// The task's artifacts folder, where its pictures go ([`read::folder`]).
    pub folder: String,
    /// The main checkout, where the `owner` stamp reads the cards the task's branch posted
    /// ([`read::owner_at`]).
    pub main: PathBuf,
    /// The people of the task's owner who hold decide, and the person at the machine by the name
    /// they gave themselves ([`crate::people::with_machine`]), whose approve meets the `owner` stamp
    /// as the owner's does ([`read::owner_at_by`], [`Store::task_deciders`]): read only when `owner`
    /// is needed, and none of the board's where the task, the board's people or its map cannot be
    /// read, so the owner's approve, under either name, alone meets it then.
    pub deciders: Vec<String>,
    /// The person at the machine, once they gave themselves a name, read from the machine folder
    /// the land's environment names ([`crate::people::machine_person_in`]): among the deciders,
    /// and named by how they are shown in what the `owner` stamp's reading says.
    pub machine: Option<crate::people::MachinePerson>,
}

impl<'a> Evidence<'a> {
    /// The evidence of the task `slug`, its `thread` read, for the stamps `needed`: the runs of the
    /// work folder the recorder keeps for `env`'s board ([`recorded::folder`]) and the tasks whose
    /// runs are the task's, both read only when `tests` is needed; the artifacts folder the main
    /// checkout `main` names; the task's deciders, read only when `owner` is.
    pub fn of(
        store: &Store,
        env: &Env,
        main: &Path,
        slug: &str,
        thread: &'a [Message],
        needed: &Needed,
    ) -> Evidence<'a> {
        let (runs, ours) = if needed.has(TESTS) {
            let runs = recorded::folder(env)
                .map(|folder| recorded::runs(&folder))
                .unwrap_or_default();
            (runs, family(store, slug))
        } else {
            (Vec::new(), vec![slug.to_string()])
        };
        let artifacts = crate::project::artifacts(main).unwrap_or_else(|| "artifacts".into());
        // The person at the machine decides on every board, under the name they gave themselves
        // as under `owner`, which no row says.
        let (deciders, machine) = if needed.has(OWNER) {
            let machine = crate::people::machine_person_in(|name| {
                env.var(name).map(std::ffi::OsString::from)
            });
            (
                crate::people::with_machine(store.task_deciders_of(slug), machine.as_ref()),
                machine,
            )
        } else {
            (Vec::new(), None)
        };
        Evidence {
            thread,
            runs,
            ours,
            folder: read::folder(&artifacts, slug),
            main: main.to_path_buf(),
            deciders,
            machine,
        }
    }
}

/// A stamp the task needs that does not stand at the tip: its name, and why, from its reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub name: String,
    pub detail: String,
}

/// What clears a red run of the `tests` stamp, after the red commands at `tip` are named.
fn clears(tip: &str) -> String {
    format!(
        "the newest run of each command at the tip is what counts, so running that command again, \
         green, at {} clears it; one that cannot pass (a command that names what is not there) is \
         cleared only by a new commit, where the stamps the task needs are given again",
        short(tip)
    )
}

/// The `tests` stamp's detail where it does not stand at `tip`: each red command, with what
/// clears it, else the reader's own words.
fn tests_missing(evidence: &Evidence<'_>, tip: &str, detail: String) -> String {
    let ours = |task: &str| evidence.ours.iter().any(|ours| ours == task);
    let reds = read::reds(&evidence.runs, tip, ours);
    if reds.is_empty() {
        return detail;
    }
    let named: Vec<String> = (reds.iter())
        .map(|(label, why)| format!("`{label}` ({why})"))
        .collect();
    format!(
        "red at {}: {}; {}",
        short(tip),
        named.join(", "),
        clears(tip)
    )
}

/// Each stamp of `needed` but `review` (the land's own approval check) read at `tip` by its
/// reader: those that stand, with the reader's detail, and those that do not.
pub fn standing(
    needed: &Needed,
    tip: &str,
    evidence: &Evidence<'_>,
) -> (Vec<(String, String)>, Vec<Missing>) {
    let (mut met, mut missing) = (Vec::new(), Vec::new());
    for name in needed.stamps.iter().filter(|name| *name != REVIEW) {
        let reading = match name.as_str() {
            TESTS => read::tests(&evidence.runs, tip, |task| {
                evidence.ours.iter().any(|ours| ours == task)
            }),
            PICTURES => read::pictures(evidence.thread, tip, &evidence.folder),
            OWNER => read::owner_at_by(
                evidence.thread,
                tip,
                &evidence.main,
                &evidence.deciders,
                evidence.machine.as_ref(),
            ),
            _ => continue,
        };
        if reading.met {
            met.push((name.clone(), reading.detail));
        } else {
            let mut detail = match name.as_str() {
                TESTS => tests_missing(evidence, tip, reading.detail),
                _ => reading.detail,
            };
            if let Some(elsewhere) = needed.waived_elsewhere(name, tip) {
                detail = format!("{detail}; {elsewhere}");
            }
            missing.push(Missing {
                name: name.clone(),
                detail,
            });
        }
    }
    (met, missing)
}

/// Each raise among `decisions` read in the checkout `dir` at the tip `tip` and on the default
/// branch at `base` ([`gates::at`]): carried, where the tip's file sets the limit to the number
/// decided; not carried, where the tip holds the default branch's number still or one below it,
/// which lands, the decision having raised nothing and a limit lowered needing none; else the
/// refusal, a number above the default branch's that no decision gave, or a limit the tip or the
/// default branch no longer sets once. Said as the landing's line says each.
pub fn raises(
    dir: &Path,
    base: &str,
    tip: &str,
    decisions: &[Decision],
) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    for decision in decisions.iter().filter(|d| d.kind == Kind::Raise) {
        let to = decision.to.unwrap_or_default();
        let decided = format!("{} by {}", decision.said(), decision.by);
        let limit = Limit::parse(&decision.gate)?;
        let held = gates::at(dir, tip, &limit)
            .map_err(|why| format!("{decided}, but at the tip {}: {why}", short(tip)))?;
        if held == to {
            said.push(format!("{decided}, carried at {}", short(tip)));
            continue;
        }
        match gates::at(dir, base, &limit) {
            Ok(main) if held == main => {
                said.push(format!(
                    "{decided}, not carried: the tip {} holds {} as the default branch does",
                    short(tip),
                    grouped(held)
                ));
                continue;
            }
            Ok(main) if held < main => {
                said.push(format!(
                    "{decided}, not carried: the tip {} lowers it to {}, below the default \
                     branch's {}, which needs no decision",
                    short(tip),
                    grouped(held),
                    grouped(main)
                ));
                continue;
            }
            _ => {}
        }
        return Err(format!(
            "{decided}, but the tip {} sets it to {}, which no decision gave: a raise lands at \
             the number decided",
            short(tip),
            grouped(held)
        ));
    }
    Ok(said)
}

/// Whether the owner's approve is all `missing` holds: the task then waits on the owner alone.
pub fn owner_alone(missing: &[Missing]) -> bool {
    !missing.is_empty() && missing.iter().all(|stamp| stamp.name == OWNER)
}

/// The refusal of a land of `slug` at `tip` for the stamps `missing`: the one line naming each and
/// who gives it (`tests` and `pictures` its executor, at the tip; `owner` the owner, on the task's
/// page), and whom the thread's post is to: its executor when a stamp of its is missing, else its
/// orchestrator, which follows the task while it waits on the owner.
pub fn refusal(slug: &str, tip: &str, missing: &[Missing]) -> (String, &'static str) {
    let said = |of: &dyn Fn(&Missing) -> bool| {
        (missing.iter())
            .filter(|stamp| of(stamp))
            .map(|stamp| format!("{} ({})", stamp.name, stamp.detail))
            .collect::<Vec<_>>()
    };
    let executors = said(&|stamp| stamp.name != OWNER);
    let owners = said(&|stamp| stamp.name == OWNER);
    let mut parts = Vec::new();
    if !executors.is_empty() {
        parts.push(format!(
            "{}, which its executor gives at that tip",
            executors.join(" and ")
        ));
    }
    if !owners.is_empty() {
        parts.push(format!(
            "{}, which the owner gives on the task's page",
            owners.join(" and ")
        ));
    }
    let to = if executors.is_empty() {
        "@orchestrator"
    } else {
        "@executor"
    };
    (
        format!(
            "{slug} lacks at its tip {} what it needs: {}",
            short(tip),
            parts.join("; ")
        ),
        to,
    )
}

#[cfg(test)]
mod tests;
