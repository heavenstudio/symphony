//! A red's mark ([`crate::red`]) is its filer's alone: a session of kind [`FILER`], as the tools that
//! file reds register, marks a task where it files it ([`crate::NewTask::red`]) or, for a red it
//! filed before it marked reds, afterwards ([`Store::mark_red`]), and lifts it from a red left open
//! as ordinary work ([`Store::unmark_red`]). No other session marks one or lifts its mark, and
//! the mark goes on a task of a red series alone ([`crate::red::SERIES`]), so no work marks itself
//! red to take no slot, nor any feature, whoever registers as a filer.

use super::{Error, REFUSED, Store, Task, no_task, pg, refused, repr};
use crate::red::{FILER, SERIES, filed};
use crate::store::statement::Statement;
use serde_json::{Value, json};

/// [`sql::FILED`]'s variables for `actor`.
pub(super) fn filing(actor: &str) -> Value {
    json!({
        "filer": FILER,
        "unfiled": format!(
            "{REFUSED}only a red's filer marks a task red, a session of kind {FILER} as the tools that file reds register; {} is not one",
            repr(actor)
        ),
    })
}

/// What a red names, trimmed, for a red mark on `slug`: refused when it names nothing, or when
/// `slug` is no red series' own task ([`filed`]).
pub(super) fn named<'w>(slug: &str, what: &'w str) -> Result<&'w str, Error> {
    if !filed(slug) {
        let mut series: Vec<String> = SERIES.iter().map(|s| format!("{s}*")).collect();
        let last = series.pop().unwrap_or_default();
        return Err(refused(format!(
            "{slug} is no red series' own task: a red is marked only on a task of the series reds are filed in, {} and {last}, never on one named under it",
            series.join(", ")
        )));
    }
    let what = what.trim();
    if what.is_empty() {
        return Err(refused("a red task names what is red, such as main".into()));
    }
    Ok(what)
}

impl Store {
    /// Marks the open task `slug` of a red series ([`crate::red::SERIES`]) red, naming `what` is red,
    /// and makes it most urgent, as its filer files a red now: for a red filed before its filer
    /// marked reds. Only a filer marks ([`FILER`]); a task marked and most urgent already is left as
    /// it is, its thread unchanged.
    pub fn mark_red(&self, slug: &str, what: &str, by: &str) -> Result<Task, Error> {
        let what = named(slug, what)?;
        let body = || Statement::from(pg::mark_red()).posting();
        self.retrying(|| {
            let mut vars = self.acting(
                by,
                (self.clock)(),
                json!({
                    "slug": slug, "red": what,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                    "closed": format!("{REFUSED}{slug} is "),
                }),
            );
            if let (Some(vars), Value::Object(filer)) = (vars.as_object_mut(), filing(by)) {
                vars.extend(filer);
            }
            self.written(body(), vars, slug)
        })
    }

    /// Lifts the red mark of the open task `slug`, which stays open red no more: main's green
    /// reached it once work on it had started, so it is ordinary work, and it and the open work
    /// under it take a slot as any work does from then. As a red's close, it is never refused at
    /// the cap and holds the counters it changes, judged on the task as read ([`super::cap`]), so a
    /// start decided before it is decided again, counted. Only a filer lifts one ([`FILER`]); a
    /// task unmarked or closed is left as it is, its thread unchanged.
    pub fn unmark_red(&self, slug: &str, by: &str) -> Result<Task, Error> {
        self.retrying(|| {
            let held = self.unmarking_slots(slug)?;
            let mut vars = self.acting(
                by,
                (self.clock)(),
                json!({
                    "slug": slug, "slots": held.slots, "version": held.version,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                }),
            );
            if let (Some(vars), Value::Object(filer)) = (vars.as_object_mut(), filing(by)) {
                vars.extend(filer);
            }
            let (fenced, guarded) = (held.fenced(), held.guarded());
            let body = Statement::from(pg::unmark_red(fenced, guarded)).posting();
            self.written(body, vars, slug)
        })
    }
}
