//! A red's mark ([`crate::red`]) is its filer's alone: a session of kind [`FILER`], as the tools that
//! file reds register, marks a task where it files it ([`crate::NewTask::red`]) or, for a red it
//! filed before it marked reds, afterwards ([`Store::mark_red`]). No other session marks one, so no
//! work marks itself red to take no slot.

use super::{ACT, Error, REFUSED, Store, Task, acting, logged, no_task, refused, repr};
use crate::red::FILER;
use serde_json::{Value, json};

/// The write's refusal unless its actor is a red's filer: `$filer` and `$unfiled` ([`filing`]).
pub(super) const FILED: &str = "IF (SELECT VALUE kind FROM ONLY type::record('session', $actor)) != $filer { THROW $unfiled };";

/// [`FILED`]'s variables for `actor`.
pub(super) fn filing(actor: &str) -> Value {
    json!({
        "filer": FILER,
        "unfiled": format!(
            "{REFUSED}only a red's filer marks a task red, a session of kind {FILER} as the tools that file reds register; {} is not one",
            repr(actor)
        ),
    })
}

/// What a red names, trimmed: refused when it names nothing.
pub(super) fn named(what: &str) -> Result<&str, Error> {
    let what = what.trim();
    if what.is_empty() {
        return Err(refused("a red task names what is red, such as main".into()));
    }
    Ok(what)
}

impl Store {
    /// Marks the open task `slug` red, naming `what` is red, and makes it most urgent, as its filer
    /// files a red now: for a red filed before its filer marked reds. Only a filer marks
    /// ([`FILER`]); a task marked and most urgent already is left as it is, its thread unchanged.
    pub fn mark_red(&self, slug: &str, what: &str, by: &str) -> Result<Task, Error> {
        let what = named(what)?;
        let task = self.get(slug)?;
        if matches!(task.status.as_str(), "done" | "cancelled") {
            return Err(refused(format!(
                "{slug} is {}: only an open task is marked red",
                task.status
            )));
        }
        if task.red() && task.priority == 0 {
            return Ok(task);
        }
        let body = format!(
            "{ACT}
IF !record::exists(type::record('task', $slug)) {{ THROW $missing }};
{FILED}
UPDATE type::record('task', $slug) SET fields._board.red = $red, priority = 0, version += 1 RETURN NONE;
LET $said = $actor + ' marked it red (' + $red + ') and most urgent, as a red is filed';
{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        );
        self.retrying(|| {
            let mut vars = acting(
                by,
                (self.clock)(),
                json!({"slug": slug, "red": what, "missing": format!("{REFUSED}{}", no_task(slug))}),
            );
            if let (Some(vars), Value::Object(filer)) = (vars.as_object_mut(), filing(by)) {
                vars.extend(filer);
            }
            self.written(&body, vars, slug)
        })
    }
}
