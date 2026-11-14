//! `tasks snapshot [--list]`: a snapshot of the store of the board the run's project keeps in its
//! common dir ([`crate::server::snapshot`]), taken now under the lock every start of its server
//! takes, and refused while a process serves it; `--list` names those kept, the newest first. It
//! opens no board: what it reads and writes is the board's folder. A board `TASKS_SERVER` names has
//! no store here, and a process Cargo runs from a repository never touches that repository's own
//! ([`crate::live::cargo_refusal`]).

use super::{Clock, Env, args::Parsed, machine};
use crate::server::{
    Home,
    snapshot::{KEEP, KEPT},
};
use std::io::Write;
use std::path::Path;

pub(super) fn run(parsed: &Parsed, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match said(parsed, env) {
        Ok(text) => {
            let _ = writeln!(out, "{text}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}

fn said(parsed: &Parsed, env: &Env) -> Result<String, String> {
    if env.var("TASKS_SERVER").is_some() {
        return Err(
            "a board TASKS_SERVER names has no store here to take a snapshot of: nothing was taken"
                .into(),
        );
    }
    let project = crate::project::find(&env.cwd, machine(env).as_deref())?;
    let common = project.place.common.clone();
    let live = crate::live::Live {
        project: project.id.clone(),
        common: common.clone(),
    };
    if let Some(why) = crate::live::cargo_refusal(&live, crate::live::cargo_here()) {
        return Err(why);
    }
    let home = Home::in_common(&common, env.read(symphony_env::SURREAL).map(Path::new));
    let board = format!("the {} board's store", project.id);
    if parsed.flag("list") {
        let kept = home.snapshots()?;
        if kept.is_empty() {
            return Ok(format!(
                "no snapshot of {board} is kept in {}",
                home.snapshots_folder().display()
            ));
        }
        return Ok(kept
            .iter()
            .map(|one| {
                format!(
                    "{}  {}{}",
                    one.name,
                    one.path.display(),
                    if one.log { KEPT } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let taken = home.snapshot(Clock::of(env)?.now())?;
    let removed = if taken.removed.is_empty() {
        String::new()
    } else {
        format!(
            "; of those whose write-ahead log is empty the newest {KEEP} are kept, so {} went",
            taken.removed.join(", ")
        )
    };
    Ok(format!(
        "✔ took a snapshot of {board}: {}{}{removed}",
        taken.snapshot.path.display(),
        if taken.snapshot.log { KEPT } else { "" }
    ))
}
