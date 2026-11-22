//! `stamps` and `needs`: the stamps a project's land can require and what a task needs of them
//! ([`crate::stamps`]), the catalogue read from the main checkout's committed `symphony.toml`, as
//! the land reads its settings. `stamps SLUG` is what an assessor reads first: the catalogue, the
//! task's needs as they stand, its branch's tip and which paths `[land] check_paths` names that the
//! branch changes, a fact the board computes so the assessor need not find the tests in a diff;
//! `needs` writes its assessment ([`crate::Store::set_needs`]).

use super::{Context, Stop};
use crate::land::{Settings, check_paths};
use crate::on_review::branch_of;
use crate::stamps::{Catalogue, Needs, said, short};
use crate::sweep::git::rev;
use crate::sweep::trigger::main_checkout;
use crate::{Store, Task};
use std::path::{Path, PathBuf};

/// The main checkout of the repository the command runs in: none outside one, or in one with none.
pub(super) fn main_of(c: &Context) -> Option<PathBuf> {
    let common = crate::git::common_dir(&c.env.cwd).ok()?;
    main_checkout(&common).map(Path::to_path_buf)
}

/// `stamps [SLUG]`: the catalogue ([`catalogue_lines`]), then, for a task, its needs and its
/// branch ([`task_lines`]). Outside a repository the catalogue is the Symphony's own.
pub(super) fn stamps(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let main = main_of(c);
    let catalogue = match &main {
        Some(main) => Catalogue::of(main).map_err(Stop::Refused)?,
        None => Catalogue::symphony(),
    };
    let mut lines = catalogue_lines(&catalogue, main.as_deref());
    if let Some(slug) = c.parsed.get("slug") {
        let task = store.get(slug)?;
        lines.extend(task_lines(&task, &catalogue, main.as_deref()));
    }
    for line in lines {
        c.say(&line);
    }
    Ok(())
}

/// The catalogue as `stamps` prints it: where it was read, then each stamp, whether a task no
/// assessment has read needs it, how it is met, where it applies and where it does not.
fn catalogue_lines(catalogue: &Catalogue, main: Option<&Path>) -> Vec<String> {
    let mut lines = vec![match (main, catalogue.written) {
        (Some(main), true) => format!(
            "STAMPS from [stamps] in symphony.toml at HEAD of {}",
            main.display()
        ),
        (Some(main), false) => format!(
            "STAMPS the Symphony's own: no [stamps] in symphony.toml at HEAD of {}",
            main.display()
        ),
        (None, _) => "STAMPS the Symphony's own: no repository here".into(),
    }];
    for stamp in &catalogue.stamps {
        let marks: Vec<&str> = [
            (stamp.default, "needed by default"),
            (stamp.waivable, "waivable at one tip by a gate decision"),
        ]
        .into_iter()
        .filter_map(|(marked, said)| marked.then_some(said))
        .collect();
        lines.push(match marks.as_slice() {
            [] => stamp.name.to_string(),
            marks => format!("{} ({})", stamp.name, marks.join(", ")),
        });
        lines.push(format!("  met: {}", stamp.met));
        lines.push(format!("  applies: {}", stamp.applies));
        lines.push(format!("  skips: {}", stamp.skips));
    }
    lines
}

/// What `stamps SLUG` adds for `task`: its needs as they stand (its assessment, else the
/// catalogue's defaults), its gate decisions that stand ([`crate::gates`]), its branch's tip in
/// the main checkout `main`, and what of `[land] check_paths` the branch changes from where it left
/// the default branch.
fn task_lines(task: &Task, catalogue: &Catalogue, main: Option<&Path>) -> Vec<String> {
    let mut lines = vec![match task.needs() {
        Some(needs) => format!(
            "NEEDS {} {} by {}: {}",
            said(&needs.stamps),
            needs.read(),
            needs.by,
            needs.why
        ),
        None => format!(
            "NEEDS {}: no assessment yet, so the catalogue's defaults",
            said(&catalogue.defaults())
        ),
    }];
    let decisions = task.gates();
    if decisions.is_empty() {
        lines.push("GATES none: no gate decision on the task".into());
    } else {
        lines.push("GATES its decisions that stand, the newest on each gate:".into());
        lines.extend(
            decisions
                .iter()
                .map(|decision| format!("  {}", decision.line())),
        );
    }
    let Some(main) = main else {
        lines.push("TIP none: no repository here".into());
        return lines;
    };
    let branch = branch_of(&task.slug);
    let Some(tip) = rev(main, &format!("refs/heads/{branch}")) else {
        lines.push(format!(
            "TIP none: no branch {branch}, so an assessment reads the brief alone (no --tip)"
        ));
        return lines;
    };
    lines.push(format!("TIP {branch} {tip}"));
    let touched = Settings::of(main).and_then(|settings| {
        let base = format!("refs/heads/{}", settings.default_branch);
        check_paths::touched(main, &base, &tip, &settings.check_paths)
    });
    match touched {
        Ok(found) if found.is_empty() => lines.push(format!(
            "CHECK PATHS none: {} changes nothing [land] check_paths names",
            short(&tip)
        )),
        Ok(found) => {
            lines.push(format!(
                "CHECK PATHS {} changes what [land] check_paths names:",
                short(&tip)
            ));
            lines.extend(found.into_iter().map(|path| format!("  {path}")));
        }
        Err(why) => lines.push(format!("CHECK PATHS cannot tell: {why}")),
    }
    lines
}

/// `needs SLUG STAMP… [--tip TIP] --why WHY`: the task's needs, as its assessor writes them,
/// against the main checkout's catalogue, the tip resolved to its whole commit there.
pub(super) fn needs(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let actor = c.actor()?;
    let slug = c.arg("slug");
    let main = main_of(c).ok_or_else(|| {
        Stop::Refused(
            "tasks needs runs in a checkout of the task's repository: the catalogue it names \
             stamps from is the main checkout's committed symphony.toml"
                .into(),
        )
    })?;
    let catalogue = Catalogue::of(&main).map_err(Stop::Refused)?;
    let commit = |option: &str| -> Result<Option<String>, Stop> {
        match c
            .parsed
            .get(option)
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            None => Ok(None),
            Some(named) => rev(&main, &format!("{named}^{{commit}}"))
                .map(Some)
                .ok_or_else(|| {
                    Stop::Refused(format!(
                        "--{option} {named} names no commit in {}",
                        main.display()
                    ))
                }),
        }
    };
    let (tip, import) = (commit("tip")?, commit("import")?);
    let needs = Needs {
        stamps: c.parsed.all("stamps").to_vec(),
        tip,
        by: actor,
        why: c.arg("why"),
        import,
    };
    let task = store.set_needs(&slug, &catalogue, &needs)?;
    let written = task.needs().unwrap_or(needs);
    c.say(&format!(
        "{slug} needs {} {}",
        said(&written.stamps),
        written.read()
    ));
    super::finish::poke(c, &slug);
    Ok(())
}
