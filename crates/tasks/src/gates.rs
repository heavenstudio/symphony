//! A task's gate decisions (docs/TASKS.md, "Gate decisions"): a quality gate given way for one
//! task by a recorded decision, read by its assessor, its reviewer and its land.
//!
//! A decision is one of two kinds ([`Kind`]):
//!
//! - **a raise** of a limit: a number named in a file of the repository ([`Limit`], `PATH:NAME`,
//!   the one line of `PATH` that sets `NAME` to it, [`Limit::read`]), from the value the default
//!   branch holds to a higher one. It reaches the gate as a change of that file committed on the
//!   task's branch, its commit's message the account, so it lands with the task and holds for
//!   every task after. Its file is always one `[land] check_paths` names whole, so the change
//!   keeps a fresh reviewer, who judges that the cut came first and the account is true;
//! - **a waiver** of a stamp the catalogue marks waivable ([`crate::stamps::Stamp::waivable`]), for
//!   one tip alone: the land does not require that stamp at that commit, and a later commit is
//!   held to it again. `tests`, `review` and `owner` are never waived ([`NEVER`]).
//!
//! The decisions are the board's own field `fields._board.gates` ([`GATES`]), every one kept, oldest
//! first, out of `set_field`'s reach; only `just tasks gate` writes them
//! ([`crate::Store::decide_gate`]), each a line on the task's thread ([`Decision::line`]). The
//! newest decision on a gate is the one that stands ([`standing`]). Who may decide is the
//! store's rule: the task's orchestrator, never a session that executed it or work under it, nor
//! one that reviews it or gave it a verdict.

use crate::stamps::{Catalogue, short};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The board's own field holding a task's gate decisions ([`crate::red::FIELDS`]`.`[`GATES`]).
pub const GATES: &str = "gates";

/// The stamps no gate decision waives, whatever a catalogue says.
pub const NEVER: [&str; 3] = ["tests", "review", "owner"];

/// Why [`NEVER`]'s stamps are never waived.
pub const NEVER_WHY: &str = "tests (a red or missing test run is a red), review (a missing review \
                             where the stamps name one) and owner (the owner's own word, which \
                             the owner gives by approving)";

/// What a decision does to its gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A limit raised from one number to a higher one ([`Limit`]).
    Raise,
    /// A stamp waived at one tip.
    Waive,
}

/// One gate decision, as the board keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// The gate: `PATH:NAME` for a raise ([`Limit`]), a stamp's name for a waiver.
    pub gate: String,
    pub kind: Kind,
    /// A raise's number before it, as the default branch held it when it was decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// A raise's number after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
    /// The whole commit a waiver is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    /// The session that decided it.
    pub by: String,
    /// Why: what was measured, and why the gate gives way.
    pub why: String,
    /// When, by the board's clock, in seconds.
    #[serde(default)]
    pub at: f64,
}

impl Decision {
    /// What it does to its gate, as a line says it: `tools/size-budget.toml:symphony raised
    /// 12,390,000 -> 12,400,000`, or `pictures waived at 1a2b3c4`.
    pub fn said(&self) -> String {
        match self.kind {
            Kind::Raise => format!(
                "{} raised {} -> {}",
                self.gate,
                grouped(self.from.unwrap_or_default()),
                grouped(self.to.unwrap_or_default())
            ),
            Kind::Waive => format!(
                "{} waived at {}",
                self.gate,
                short(self.tip.as_deref().unwrap_or_default())
            ),
        }
    }

    /// The thread's line for it: `gate tools/size-budget.toml:symphony raised 12,390,000 ->
    /// 12,400,000: WHY; by HANDLE`.
    pub fn line(&self) -> String {
        format!("gate {}: {}; by {}", self.said(), self.why, self.by)
    }
}

/// Every gate decision `fields` record, oldest first; none where they record none, or a list
/// that does not read.
pub fn of(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Vec<Decision> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(GATES))
        .and_then(|value| crate::objects::read(value).ok())
        .unwrap_or_default()
}

/// The decisions that stand: the newest on each gate, in the order they were given.
pub fn standing(all: &[Decision]) -> Vec<Decision> {
    let mut kept: Vec<Decision> = Vec::new();
    for decision in all {
        kept.retain(|earlier| earlier.gate != decision.gate);
        kept.push(decision.clone());
    }
    kept
}

impl crate::Task {
    /// Its gate decisions that stand ([`of`], [`standing`]).
    pub fn gates(&self) -> Vec<Decision> {
        standing(&of(&self.fields))
    }
}

/// The waivers among `decisions` that the land goes by for the stamps `needed` at `tip`: those for
/// that tip, which lift their stamp, and those for another tip, which lift nothing and which a
/// refusal names. A waiver of a stamp the catalogue does not mark waivable lifts nothing anywhere
/// ([`Catalogue::unwaivable`]): one recorded under another catalogue is read under this one.
pub fn waivers<'a>(
    decisions: &'a [Decision],
    catalogue: &Catalogue,
    needed: &[String],
    tip: Option<&str>,
) -> (Vec<&'a Decision>, Vec<&'a Decision>) {
    let (mut here, mut elsewhere) = (Vec::new(), Vec::new());
    for decision in decisions {
        let waives = decision.kind == Kind::Waive
            && needed.contains(&decision.gate)
            && catalogue.unwaivable(&decision.gate).is_none();
        if !waives {
            continue;
        }
        if tip.is_some() && decision.tip.as_deref() == tip {
            here.push(decision);
        } else {
            elsewhere.push(decision);
        }
    }
    (here, elsewhere)
}

/// A limit a raise names: the number `NAME` is set to in the file `PATH` of the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limit {
    /// The file, relative to the project's root, with no leading `/`.
    pub path: String,
    /// The name the number is set under: a TOML key or a Rust constant's name.
    pub name: String,
}

impl Limit {
    /// The limit `gate` names as `PATH:NAME`, or why it names none.
    pub fn parse(gate: &str) -> Result<Limit, String> {
        let refused = |why: &str| {
            format!(
                "a raise names its limit as PATH:NAME, the file and the name its number is set \
                 under (tools/size-budget.toml:symphony): {gate:?} {why}"
            )
        };
        let (path, name) = gate
            .trim()
            .rsplit_once(':')
            .ok_or_else(|| refused("has no :"))?;
        let path = path.trim().trim_start_matches('/');
        let name = name.trim();
        if path.is_empty()
            || path.ends_with('/')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(refused("names no file inside the project"));
        }
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || "_-".contains(c))
        {
            return Err(refused("names no key or constant"));
        }
        Ok(Limit {
            path: path.into(),
            name: name.into(),
        })
    }

    /// The limit as a gate names it: `PATH:NAME`.
    pub fn gate(&self) -> String {
        format!("{}:{}", self.path, self.name)
    }

    /// The number `text`, the file's content, sets the limit to: the one line whose first word is
    /// its name (after `pub`, `const`, `static` or `let`), then an optional `: TYPE`, then `=` and
    /// a number (`symphony = 12_390_000`, `const TASKS_FIRST_VISIT_BYTES: f64 = 125_000.0;`).
    /// Refused where no line sets it, where more than one does, or where its value is no whole
    /// number.
    pub fn read(&self, text: &str) -> Result<u64, String> {
        let found: Vec<(usize, Option<u64>)> = (text.lines().enumerate())
            .filter_map(|(at, line)| self.value(line).map(|value| (at + 1, value)))
            .collect();
        match found.as_slice() {
            [(_, Some(value))] => Ok(*value),
            [(line, None)] => Err(format!(
                "{} sets {} on line {line} to no whole number",
                self.path, self.name
            )),
            [] => Err(format!("no line of {} sets {}", self.path, self.name)),
            many => Err(format!(
                "{} sets {} on {} lines ({}), so no one number is its limit",
                self.path,
                self.name,
                many.len(),
                (many.iter())
                    .map(|(line, _)| line.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// Whether `line` sets the limit, and to what: `None` for a line that does not, `Some(None)`
    /// for one that sets it to no whole number.
    fn value(&self, line: &str) -> Option<Option<u64>> {
        let mut rest = line.trim_start();
        for word in ["pub ", "const ", "static ", "let "] {
            rest = rest.strip_prefix(word).map_or(rest, str::trim_start);
        }
        let rest = rest.strip_prefix(self.name.as_str())?;
        let rest = rest.trim_start();
        let rest = match rest.strip_prefix(':') {
            Some(typed) => &typed[typed.find('=')?..],
            None => rest,
        };
        let value = rest.strip_prefix('=')?.trim_start();
        if value.starts_with('=') {
            return None;
        }
        let number: String = (value.chars())
            .take_while(|c| c.is_ascii_digit() || "_,.".contains(*c))
            .collect();
        // What follows the number ends it: `3e4` or `10px` is no number of this reading.
        let after = value[number.len()..].chars().next();
        if after.is_some_and(|c| c.is_alphanumeric() || "\"'".contains(c)) {
            return Some(None);
        }
        Some(parse(number.trim_end_matches(',')))
    }
}

/// The number `limit` is set to in the file it names as the commit `commit` holds it, in the
/// checkout `dir` ([`Limit::read`]); or why it cannot be read there.
pub fn at(dir: &std::path::Path, commit: &str, limit: &Limit) -> Result<u64, String> {
    let shown = crate::sweep::git::git(dir, &["show", &format!("{commit}:{}", limit.path)])
        .filter(|done| done.status.success())
        .ok_or_else(|| format!("{} holds no file {}", short(commit), limit.path))?;
    limit.read(&String::from_utf8_lossy(&shown.stdout))
}

/// `text` as a whole number, its digits grouped by `_` or `,` as written (`12_390_000`,
/// `150,000`), with a fraction of zeros alone (`125_000.0`); none for anything else.
pub fn parse(text: &str) -> Option<u64> {
    let text = text.trim();
    let whole = match text.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() && fraction.chars().all(|c| c == '0') => {
            whole
        }
        Some(_) => return None,
        None => text,
    };
    let digits: String = whole.chars().filter(|c| !"_,".contains(*c)).collect();
    let grouped_well = !whole.starts_with(['_', ',']) && !whole.ends_with(['_', ',']);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) || !grouped_well {
        return None;
    }
    digits.parse().ok()
}

/// `n` with its digits grouped by threes: `12,390,000`.
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut said = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            said.push(',');
        }
        said.push(digit);
    }
    said
}

#[cfg(test)]
mod tests;
