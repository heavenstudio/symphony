//! `tasks snapshot [--list]`: a snapshot of the PostgreSQL data of the board the run's project
//! keeps in its common dir ([`crate::server::Postgres::snapshot`]), taken now under the lock every
//! start of its server takes, and refused while its server runs; `--list` names those kept, the
//! newest first. It opens no board: what it reads and writes is the board's folder. A board
//! `TASKS_SERVER` names has no data here, a board still on SurrealDB is refused, naming how it
//! moves ([`crate::server::GONE`]), and a process Cargo runs from a repository never touches that
//! repository's own ([`crate::live::cargo_refusal`]).

use super::{Clock, Env, args::Parsed, machine};
use crate::server::{Engine, Home};
use std::io::Write;

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
    let home = Home::in_common(&common);
    let board = format!("the {} board's store", project.id);
    if home.backend().map_err(|why| why.to_string())? == Engine::SurrealDb {
        let nothing = if parsed.flag("list") {
            "No snapshot was listed"
        } else {
            "No snapshot was taken"
        };
        return Err(home.on_surrealdb(nothing).to_string());
    }
    let pg = home
        .project_postgresql(&project.place.root)
        .map_err(|why| why.to_string())?;
    if parsed.flag("list") {
        let paths = pg.snapshots().map_err(|why| why.to_string())?;
        return Ok(if paths.is_empty() {
            format!("no PostgreSQL snapshot of {board} is kept")
        } else {
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        });
    }
    let path = pg
        .snapshot(Clock::of(env)?.now())
        .map_err(|why| why.to_string())?;
    Ok(format!(
        "✔ took a PostgreSQL snapshot of {board}: {}",
        path.display()
    ))
}
