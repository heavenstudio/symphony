//! `tasks recovery`: the board's recovery binding said, revoked or made again
//! ([`crate::store::recovery`]). The authority it reads and writes is the board home's own file
//! for the database the run reaches ([`crate::server::Home::recovery`]); a board `TASKS_SERVER`
//! names has no home, so no authority, and is refused. Read before the board's argument table, as
//! `database` is: `revoke` opens no board and starts no database, as it is run before restored
//! data is served.

use super::{Clock, Env, machine, open, reach};
use crate::server::{FileAuthority, Home};
use crate::store::recovery::Authority as _;
use crate::{DATABASE, Store};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// The word that runs it, read before the board's argument table.
pub const COMMAND: &str = "recovery";

/// Its usage line.
pub const USAGE: &str = "usage: just tasks recovery [-h] [revoke | rebind] [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks recovery [-h] [revoke | rebind] [--as ACTOR]

Say, revoke or make again the board's recovery binding. A write bound to a
recovery generation (a mapping or an edit a program outside the board keeps of
a task) is taken only while the board's database holds the generation the
binding names, a file of the board's folder outside the database's data
(symphony/tasks/recovery/DATABASE.json in the git common dir).

  just tasks recovery         the generation the board holds, the one it is
                              bound to, and whether bound writes are taken
  just tasks recovery revoke  bind the board to no generation: every bound
                              write is refused from then on. Run it before
                              data put back from a snapshot is served; it
                              opens no board and starts no database
  just tasks recovery rebind  revoke, replace the board's generation with a
                              new one, then bind the board to it: every
                              token read before it is refused. Run it once
                              the board is the one to keep; cut short, it
                              leaves the board bound to none: run it again

A copy of the whole board replaces the generation by itself. Putting a
database's files back does not: revoke first, rebind after (docs/workflow/TASKS.md).
Not told apart: data put back without the revoke where no copy came between,
and data and binding both put back to an older pair that agree. revoke and
rebind act on the project's own board only from its main checkout; a scratch
database (--database NAME) is bound by its own file. A board TASKS_SERVER
names has no folder here, so no binding: bound writes are refused there.

options:
  -h, --help  show this help message and exit
  --as ACTOR  taken and not used: the binding is the board's, no one's act
";

/// The board home's authority for the database `env` reaches, with how its refusals name the
/// board: refused for a server `TASKS_SERVER` names and for a process Cargo runs from the
/// board's repository ([`crate::live::cargo_refusal`]).
fn authority(env: &Env, acts: bool) -> Result<(FileAuthority, String), String> {
    if env.var("TASKS_SERVER").is_some() {
        return Err(
            "TASKS_SERVER names a server by its address, which has no board folder here and so \
             no recovery binding: bound writes are refused on it, and nothing was changed"
                .into(),
        );
    }
    let database = env.var("TASKS_DATABASE").unwrap_or(DATABASE);
    if !crate::rpc::plain(database) {
        return Err(format!(
            "TASKS_DATABASE={database:?} is not a plain name for a database"
        ));
    }
    let project = crate::project::find(&env.cwd, machine(env).as_deref())?;
    let live = crate::live::Live {
        project: project.id.clone(),
        common: project.place.common.clone(),
    };
    if let Some(why) = crate::live::cargo_refusal(&live, crate::live::cargo_here()) {
        return Err(why);
    }
    let board = if database == DATABASE {
        if acts {
            in_main(&live, env)?;
        }
        format!("the {} board", project.id)
    } else {
        format!(
            "the scratch database {database} of the {} board",
            project.id
        )
    };
    let home = Home::in_common(
        &project.place.common,
        env.read(symphony_env::SURREAL).map(Path::new),
    );
    Ok((home.recovery(database), board))
}

/// Refused unless `env` runs in the main checkout of `live`'s repository, or in any checkout of
/// one that has none: where the project's own board is rebound or revoked from.
fn in_main(live: &crate::live::Live, env: &Env) -> Result<(), String> {
    let here = match live.main() {
        Some(main) => crate::git::checkout(&env.cwd).is_some_and(|(top, _)| top == main),
        None => crate::git::read(&env.cwd).as_deref() == Some(live.common.as_path()),
    };
    if here {
        return Ok(());
    }
    Err(format!(
        "`tasks recovery` changes the {} board's binding only from its main checkout{}: nothing \
         was changed; a scratch board is bound with --database NAME",
        live.project,
        live.main()
            .map_or_else(String::new, |main| format!(", {}", main.display()))
    ))
}

/// `store`, a board `env` reached, answering to its home's recovery authority: none, so every
/// bound call refused, for a server `TASKS_SERVER` names.
pub fn recovering(store: Store, env: &Env) -> Store {
    match authority(env, false) {
        Ok((authority, _)) => store.recovering(Arc::new(authority)),
        Err(_) => store,
    }
}

/// `tasks recovery ARGS` (the words after `recovery`) in `env`: what it did goes to `out`, a
/// refusal to `err`; the exit code is returned.
pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut words = Vec::new();
    let mut given = args.iter();
    while let Some(word) = given.next() {
        match word.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{HELP}");
                return 0;
            }
            "--as" if given.next().is_some() => {}
            _ => words.push(word.as_str()),
        }
    }
    let did = match words.as_slice() {
        [] => status(env),
        ["revoke"] => revoke(env),
        ["rebind"] => rebind(env),
        _ => {
            let _ = writeln!(
                err,
                "{USAGE}just tasks recovery: error: unrecognized arguments: {}",
                args.join(" ")
            );
            return 2;
        }
    };
    match did {
        Ok(said) => {
            let _ = writeln!(out, "{said}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}

fn status(env: &Env) -> Result<String, String> {
    let (authority, board) = authority(env, false)?;
    let bound = authority.bound()?;
    let store = open(reach(env, &env.cwd, true)?, &Clock::of(env)?)?;
    let held = store.recovery_generation().map_err(|why| why.to_string())?;
    let none = || "none".to_string();
    let verdict = match (&held, &bound) {
        (Some(held), Some(bound)) if held == bound => "bound writes are taken",
        (_, None) => {
            "bound to none, so bound writes are refused; `just tasks recovery rebind` binds it"
        }
        _ => {
            "the two differ (a copy or a restore came between), so bound writes are refused; \
             `just tasks recovery rebind` binds it"
        }
    };
    Ok(format!(
        "{board} holds recovery generation {} and is bound to {}: {verdict}",
        held.unwrap_or_else(none),
        bound.unwrap_or_else(none)
    ))
}

fn revoke(env: &Env) -> Result<String, String> {
    let (authority, board) = authority(env, true)?;
    authority.revoke()?;
    Ok(format!(
        "✔ {board} is bound to no recovery generation: bound writes are refused until `just \
         tasks recovery rebind`"
    ))
}

fn rebind(env: &Env) -> Result<String, String> {
    let (authority, board) = authority(env, true)?;
    let store =
        open(reach(env, &env.cwd, true)?, &Clock::of(env)?)?.recovering(Arc::new(authority));
    let generation = store.rebind().map_err(|why| why.to_string())?;
    Ok(format!(
        "✔ {board} holds a new recovery generation, {generation}, and is bound to it: every token \
         read before is refused"
    ))
}
