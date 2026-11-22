//! `gate SLUG GATE (--raise NEW | --waive [--tip TIP]) --why WHY`: a gate decision on a task, as
//! its orchestrator gives it ([`crate::gates`], [`crate::Store::decide_gate`]).
//!
//! A raise names a limit, `PATH:NAME`, read from the main checkout's default branch, which is the
//! number it raises: its file must be one `[land] check_paths` names whole, so the change keeps a
//! fresh reviewer. A waiver names a stamp of the main checkout's committed catalogue, for the tip
//! `--tip` names, else the task's branch's tip, each resolved to its whole commit there.

use super::stamps::main_of;
use super::{Context, Stop};
use crate::Store;
use crate::gates::{self, Decision, Kind, Limit, grouped};
use crate::land::Settings;
use crate::on_review::branch_of;
use crate::stamps::{Catalogue, short};
use crate::sweep::git::rev;
use std::path::Path;

/// The decision `--raise NEW` makes of `gate` in the main checkout `main`: the limit, its number
/// on the default branch and `NEW`. Refused: a limit not in a file `[land] check_paths` names
/// whole, one the default branch does not set once to a whole number, and a `NEW` that is no
/// whole number.
fn raised(main: &Path, gate: &str, new: &str) -> Result<(Limit, u64, u64), String> {
    let limit = Limit::parse(gate)?;
    let settings = Settings::of(main)?;
    let checks =
        (settings.check_paths.iter()).any(|entry| entry.whole() && entry.names(&limit.path));
    if !checks {
        return Err(format!(
            "{} is no check: a raised limit is a number in a file [land] check_paths names whole \
             (a test, tools/, a budget), so the raise keeps a fresh reviewer",
            limit.path
        ));
    }
    let to = gates::parse(new).ok_or_else(|| {
        format!("--raise names the number the limit goes to, a whole number: not {new:?}")
    })?;
    let default = &settings.default_branch;
    let commit = rev(main, &format!("refs/heads/{default}^{{commit}}"))
        .ok_or_else(|| format!("no branch {default} in {}", main.display()))?;
    let from = gates::at(main, &commit, &limit).map_err(|why| format!("{default}: {why}"))?;
    Ok((limit, from, to))
}

pub(super) fn gate(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let actor = c.actor()?;
    let (slug, gate) = (c.arg("slug"), c.arg("gate"));
    let main = main_of(c).ok_or_else(|| {
        Stop::Refused(
            "tasks gate runs in a checkout of the task's repository: a raise reads its limit, and \
             a waiver its stamp and tip, from the main checkout"
                .into(),
        )
    })?;
    let catalogue = Catalogue::of(&main).map_err(Stop::Refused)?;
    let raise = c.parsed.get("raise").map(str::to_string);
    let decision = match (raise, c.parsed.flag("waive")) {
        (Some(new), _) => {
            if c.parsed.get("tip").is_some() {
                return Err(Stop::Refused(
                    "a raise holds for every tip after it lands: --tip is a waiver's".into(),
                ));
            }
            let (limit, from, to) = raised(&main, &gate, &new).map_err(Stop::Refused)?;
            Decision {
                gate: limit.gate(),
                kind: Kind::Raise,
                from: Some(from),
                to: Some(to),
                tip: None,
                by: actor,
                why: c.arg("why"),
                at: 0.0,
            }
        }
        (None, true) => {
            let named = c.parsed.get("tip").map(str::trim).filter(|t| !t.is_empty());
            let tip = match named {
                Some(named) => rev(&main, &format!("{named}^{{commit}}")).ok_or_else(|| {
                    Stop::Refused(format!(
                        "--tip {named} names no commit in {}",
                        main.display()
                    ))
                })?,
                None => {
                    let branch = branch_of(&slug);
                    rev(&main, &format!("refs/heads/{branch}")).ok_or_else(|| {
                        Stop::Refused(format!(
                            "no branch {branch} in {}: a waiver is for one tip, which --tip names",
                            main.display()
                        ))
                    })?
                }
            };
            Decision {
                gate: gate.trim().to_string(),
                kind: Kind::Waive,
                from: None,
                to: None,
                tip: Some(tip),
                by: actor,
                why: c.arg("why"),
                at: 0.0,
            }
        }
        (None, false) => {
            return Err(Stop::Refused(
                "name what the decision does: --raise NEW for a limit (PATH:NAME), --waive for a \
                 stamp at one tip"
                    .into(),
            ));
        }
    };
    store.decide_gate(&slug, &catalogue, &decision)?;
    c.say(&match decision.kind {
        Kind::Raise => format!(
            "{slug}: {} raised {} -> {}; commit the file's change on the task's branch, its \
             message the account",
            decision.gate,
            grouped(decision.from.unwrap_or_default()),
            grouped(decision.to.unwrap_or_default())
        ),
        Kind::Waive => format!(
            "{slug}: {} waived at {} alone",
            decision.gate,
            short(decision.tip.as_deref().unwrap_or_default())
        ),
    });
    super::finish::poke(c, &slug);
    Ok(())
}
