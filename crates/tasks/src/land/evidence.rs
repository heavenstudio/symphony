//! The evidence of one task at one head, as a land delivered by a hosted pull request goes by it
//! ([`super::delivery`]) and as `tasks api`'s `evidence` answers it to the program that publishes
//! it: one function, [`snapshot`], so what is published and what the land decides on cannot
//! disagree. It reads and decides nothing of its own: which stamps the task needs is
//! [`Needed::of`]'s, the approve is [`super::approval`]'s, [`super::independent`]'s and
//! [`super::covered`]'s, and every other stamp is [`needs::standing`]'s, the readers the default
//! land goes by. It names no provider and writes nothing.
//!
//! It is stricter than the default land in what binds evidence to the head, since a hosted merge
//! is asked for one commit by its whole name:
//!
//! - the assessment is of the head itself. One of another commit, or of the brief alone, lowers
//!   nothing and waives nothing, and with none the task is not assessed: no default is taken for
//!   a waiver;
//! - the approve names the head. One that covers it by time, or that stands over a merge of
//!   documents it did not read, does not;
//! - each stamp of the catalogue is said: met at the head, waived by that assessment or by a gate
//!   decision for the head ([`crate::gates`]) with its author and reason, or missing; a raise a
//!   gate decision gave and the head does not carry blocks it, as it refuses the default land
//!   ([`needs::raises`]);
//! - the upstream commit the assessment names the head an import of, where it names one, is part
//!   of it, so its revision changes with it: a delivery merges an import only under a snapshot
//!   that says it is one ([`super::delivery`]).

use super::needs::{self, Needed, REVIEW, TESTS};
use super::{Covered, MANIFEST, approval, covered, independent, run_git, short};
use crate::cli::Env;
use crate::stamps::{Catalogue, read};
use crate::sweep::git::{rev, succeeds};
use crate::{Message, Store, Task};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The snapshot's shape: a reader of another number does not read it.
pub const VERSION: u32 = 1;

/// The SHA-256 of `parts`, each ended by its length so that no two lists read the same.
pub(super) fn digest(parts: &[&str]) -> String {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_le_bytes());
        hasher.update(part.as_bytes());
    }
    (hasher.finalize().iter()).fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Where a stamp of the catalogue stands at the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    /// Needed, and its reader finds it at the head.
    Met,
    /// Not needed, by the assessment of the head, which says why.
    Waived,
    /// Needed and not at the head, or left out by nothing that read the head.
    Missing,
}

/// One stamp of the catalogue at the head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub name: String,
    pub standing: Standing,
    /// Its reader's line, or the assessment's reason for a waiver.
    pub detail: String,
    /// What it stands on: the recorded runs' ids, the thread's line, or the cards the owner
    /// approved (`card:NAME`).
    pub evidence: Vec<String>,
    /// Missing because what it reads is red at the head (a command's newest run there failed),
    /// not because it is not given yet: what the check of it says fails, and no later write but
    /// a new run or a new head turns it. Said only where it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub red: bool,
}

/// The assessment the task carries, whatever commit it read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    pub by: String,
    /// The whole commit it read; none when it read the brief alone.
    pub tip: Option<String>,
    pub why: String,
    pub stamps: Vec<String>,
    /// The whole upstream commit it names the head an import of, where it names one
    /// ([`crate::stamps::Needs::import`]): said only then, so another task's snapshot reads as it
    /// did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

/// A task's evidence at its branch's head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub slug: String,
    /// The task's number on this board ([`Task::id`]).
    pub task: i64,
    pub status: String,
    /// The whole commit the branch named as the task is at.
    pub head: String,
    /// The default branch it is delivered to.
    pub branch: String,
    /// A digest of the project file the default branch commits, which holds the landing's
    /// settings; `absent` with none.
    pub policy: String,
    /// A digest of the stamps' catalogue as read.
    pub catalogue: String,
    pub assessment: Option<Assessment>,
    /// Whether the assessment read the head.
    pub assessed: bool,
    /// Every stamp of the catalogue, in its order.
    pub stamps: Vec<Stamp>,
    /// Each reason it is not eligible, one line each; none when it is.
    pub blocking: Vec<String>,
    /// Assessed at the head, every stamp met or waived, and the task where a land takes it from.
    pub eligible: bool,
    /// A digest of all the above: two snapshots of one revision say the same.
    pub revision: String,
}

impl Snapshot {
    fn revised(mut self) -> Snapshot {
        self.revision = String::new();
        let said = serde_json::to_string(&self).unwrap_or_default();
        self.revision = digest(&[&said]);
        self
    }
}

/// The evidence of `task` at `head`, its branch's tip, its `thread` read, in the main checkout
/// `main` from where `env` runs.
///
/// # Errors
///
/// The catalogue or the project file does not read.
pub fn snapshot(
    store: &Store,
    env: &Env,
    main: &Path,
    task: &Task,
    thread: &[Message],
    head: &str,
) -> Result<Snapshot, String> {
    let slug = &task.slug;
    let settings = super::Settings::of(main)?;
    let catalogue = Catalogue::of(main)?;
    let needed = Needed::of(task, main, Some(head))?;
    let at = format!("HEAD:{MANIFEST}");
    let policy = if succeeds(main, &["cat-file", "-e", &at]) {
        digest(&[&run_git(main, &["show", &at])?])
    } else {
        "absent".into()
    };
    let names = catalogue.names();
    let defaults = catalogue.defaults();
    let assessment = needed.needs.as_ref().map(|needs| Assessment {
        by: needs.by.clone(),
        tip: needs.tip.clone(),
        why: needs.why.clone(),
        stamps: needs.stamps.clone(),
        import: needs.import.clone(),
    });
    let assessed = (assessment.as_ref()).is_some_and(|read| read.tip.as_deref() == Some(head));
    let mut blocking = Vec::new();
    match &assessment {
        Some(_) if assessed => {}
        Some(read) => blocking.push(format!(
            "its assessment by {} read {}, not the head {}: an assessor reads the head",
            read.by,
            read.tip.as_deref().map_or("the brief alone", short),
            short(head)
        )),
        None => blocking.push(format!(
            "no assessment names what {slug} needs at {}: an assessor reads the head, and names \
             `none` for a task that needs no stamp",
            short(head)
        )),
    }
    // The approve, by the land's own check, held to naming the head.
    let review = || -> Result<(String, Vec<String>), String> {
        let approval = approval(task, thread)?;
        let held = store.ever_held(slug).map_err(|error| error.to_string())?;
        independent(task, &approval, &held)?;
        let base = rev(main, &format!("refs/heads/{}", settings.default_branch))
            .ok_or_else(|| format!("no branch {}", settings.default_branch))?;
        match covered(main, slug, &base, head, &approval)? {
            Covered::Named => {}
            Covered::ByTime(_) | Covered::Merged(_) => {
                return Err(format!(
                    "{slug}'s approve by {} does not name the head {}: a delivery by pull request \
                     takes an approve naming the head alone",
                    approval.by,
                    short(head)
                ));
            }
        }
        let line = crate::approved(thread).map(|verdict| format!("thread:{}", verdict.id));
        Ok((
            format!("Approved by {} naming {}", approval.by, short(head)),
            line.into_iter().collect(),
        ))
    };
    let evidence = needs::Evidence::of(store, env, main, slug, thread, &needed);
    let (met, missing) = needs::standing(&needed, head, &evidence);
    let ours = |named: &str| evidence.ours.iter().any(|ours| ours == named);
    let stood_on = |name: &str| -> Vec<String> {
        match name {
            TESTS => (read::at_tip(&evidence.runs, head, ours).commands.iter())
                .map(|command| format!("run:{}", command.newest.operation_id))
                .collect(),
            needs::PICTURES => (read::pictures(thread, head, &evidence.folder).line)
                .map(|line| format!("thread:{line}"))
                .into_iter()
                .collect(),
            needs::OWNER => {
                let reading = read::owner_at(thread, head, main);
                (reading
                    .line
                    .map(|line| format!("thread:{line}"))
                    .into_iter())
                .chain(reading.cards.iter().map(|card| format!("card:{card}")))
                .collect()
            }
            _ => Vec::new(),
        }
    };
    let mut stamps = Vec::new();
    for name in names {
        let wanted = needed.stamps.iter().any(|stamp| stamp == name);
        let stamp = |standing, detail: String, evidence| Stamp {
            name: name.to_string(),
            standing,
            detail,
            evidence,
            red: false,
        };
        let waiver = needed.waived.iter().find(|waiver| waiver.gate == name);
        stamps.push(if let Some(waiver) = waiver {
            stamp(
                Standing::Waived,
                format!(
                    "Waived at {} by the gate decision of {}: {}",
                    short(head),
                    waiver.by,
                    waiver.why
                ),
                Vec::new(),
            )
        } else if !wanted {
            match &assessment {
                Some(read) if assessed => stamp(
                    Standing::Waived,
                    format!("Waived by the assessment of {}: {}", read.by, read.why),
                    Vec::new(),
                ),
                _ => stamp(
                    Standing::Missing,
                    "Left out by no assessment of the head".into(),
                    Vec::new(),
                ),
            }
        } else if name == REVIEW {
            match review() {
                Ok((detail, line)) => stamp(Standing::Met, detail, line),
                Err(why) => stamp(Standing::Missing, why, Vec::new()),
            }
        } else if let Some((_, detail)) = met.iter().find(|(met, _)| met == name) {
            stamp(Standing::Met, detail.clone(), stood_on(name))
        } else {
            let detail = (missing.iter().find(|stamp| stamp.name == name))
                .map_or_else(|| "No reader says it stands".into(), |m| m.detail.clone());
            // Its reader's line says which command; the check of it fails rather than waits.
            let red = name == TESTS && !read::reds(&evidence.runs, head, ours).is_empty();
            Stamp {
                red,
                ..stamp(Standing::Missing, detail, Vec::new())
            }
        });
    }
    for stamp in &stamps {
        if stamp.standing == Standing::Missing && needed.stamps.contains(&stamp.name) {
            blocking.push(format!("{}: {}", stamp.name, stamp.detail));
        }
    }
    let base = rev(main, &format!("refs/heads/{}", settings.default_branch));
    if let Some(base) = base
        && let Err(line) = needs::raises(main, &base, head, &needed.decisions)
    {
        blocking.push(line);
    }
    // Where a land takes it from: done by an approve where review is needed, which the review
    // stamp's reader said already; else in review or blocked, or done by an approve.
    if !needed.review() {
        if let Some((line, _)) = needs::not_ready(slug, &task.status) {
            blocking.push(line);
        } else if task.status == "done"
            && let Err(line) = approval(task, thread)
        {
            blocking.push(line);
        }
    }
    let eligible = blocking.is_empty();
    // The catalogue as read: each stamp's name, a default's marked.
    let listed: Vec<String> = (catalogue.names().iter())
        .map(|name| {
            let default = defaults.iter().any(|default| default == name);
            format!("{name}{}", if default { "*" } else { "" })
        })
        .collect();
    let catalogue = digest(&listed.iter().map(String::as_str).collect::<Vec<_>>());
    Ok(Snapshot {
        version: VERSION,
        slug: slug.clone(),
        task: task.id,
        status: task.status.clone(),
        head: head.to_string(),
        branch: settings.default_branch,
        policy,
        catalogue,
        assessment,
        assessed,
        stamps,
        blocking,
        eligible,
        revision: String::new(),
    }
    .revised())
}

/// The evidence of the task `slug` on `store`, at the tip of the branch named as it in the main
/// checkout of the repository `env` runs in; `expected`, when given, is the head the caller
/// believes it at.
///
/// # Errors
///
/// No checkout, no such task or branch, a branch at another commit than `expected`, or settings
/// that do not read.
pub fn of(
    store: &Store,
    env: &Env,
    slug: &str,
    expected: Option<&str>,
) -> Result<Snapshot, String> {
    let common = crate::git::common_dir(&env.cwd).map_err(|error| error.to_string())?;
    let main = crate::sweep::trigger::main_checkout(&common)
        .ok_or_else(|| format!("{} has no main checkout", common.display()))?;
    let task = store.get(slug).map_err(|error| error.to_string())?;
    let thread = store.thread(slug).map_err(|error| error.to_string())?;
    let head = rev(main, &format!("refs/heads/{slug}"))
        .ok_or_else(|| format!("no branch {slug} in {}", main.display()))?;
    if let Some(expected) = expected
        && expected != head
    {
        return Err(format!(
            "{slug}'s branch is at {head}, not {expected}: evidence is of the head alone"
        ));
    }
    snapshot(store, env, main, &task, &thread, &head)
}
