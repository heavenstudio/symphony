//! A red's mark ([`crate::red`]) is its filer's alone: a session of kind [`FILER`], as the tools that
//! file reds register, marks a task where it files it ([`crate::NewTask::red`]) or, for a red it
//! filed before it marked reds, afterwards ([`Store::mark_red`]). No other session marks one, and
//! the mark goes on a task of a red series alone ([`crate::red::SERIES`]), so no work marks itself
//! red to take no slot, nor any feature, whoever registers as a filer.

use super::{ACT, Error, REFUSED, Store, Task, acting, logged, no_task, refused, repr};
use crate::red::{FILER, SERIES, filed};
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

/// What a red names, trimmed, for a red mark on `slug`: refused when it names nothing, or when
/// `slug` is of no red series ([`filed`]).
pub(super) fn named<'w>(slug: &str, what: &'w str) -> Result<&'w str, Error> {
    if !filed(slug) {
        let series: Vec<String> = SERIES.iter().map(|s| format!("{s}*")).collect();
        return Err(refused(format!(
            "{slug} is of no red series: a red is marked only on a task of the series reds are filed in, {}",
            series.join(" and ")
        )));
    }
    let what = what.trim();
    if what.is_empty() {
        return Err(refused("a red task names what is red, such as main".into()));
    }
    Ok(what)
}

/// [`Store::mark_red`]'s write: what it judges is read in the transaction that writes, so two marks
/// at once post one line.
const MARK_RED: &str = "
LET $task = (SELECT status, priority, fields._board.red AS red FROM ONLY type::record('task', $slug));
IF $task = NONE { THROW $missing };
IF $task.status IN ['done', 'cancelled'] {
    LET $refusal = $closed + $task.status + ': only an open task is marked red';
    THROW $refusal;
};";

impl Store {
    /// Marks the open task `slug` of a red series ([`crate::red::SERIES`]) red, naming `what` is red,
    /// and makes it most urgent, as its filer files a red now: for a red filed before its filer
    /// marked reds. Only a filer marks ([`FILER`]); a task marked and most urgent already is left as
    /// it is, its thread unchanged.
    pub fn mark_red(&self, slug: &str, what: &str, by: &str) -> Result<Task, Error> {
        let what = named(slug, what)?;
        let body = format!(
            "{ACT}{MARK_RED}
{FILED}
IF !$task.red OR $task.priority > 0 {{
    UPDATE type::record('task', $slug) SET fields._board.red = $red, priority = 0, version += 1 RETURN NONE;
    LET $said = $actor + ' marked it red (' + $red + ') and most urgent, as a red is filed';
    {}
}};",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        );
        self.retrying(|| {
            let mut vars = acting(
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
            self.written(&body, vars, slug)
        })
    }
}
