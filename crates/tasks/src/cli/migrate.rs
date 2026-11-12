//! `tasks migrate`: the board moved forward to this build's schema, or made, and its schema said.
//! On a project's live board it runs only in the main checkout, and it is what alone moves a live
//! board whose main checkout holds no board source ([`crate::live::may_move`]); on a scratch board it
//! moves it as any writer would.

use super::{Clock, Env, open_as, reach};
use crate::rpc::Client;
use crate::{NAMESPACE, Opening, Store};
use std::io::Write;

/// The word that runs it, read before the board's argument table, as `run` is.
pub const COMMAND: &str = "migrate";

/// Its usage line.
pub const USAGE: &str = "usage: just tasks migrate [-h] [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks migrate [-h] [--as ACTOR]

Move the board forward to this build's schema, or make it, and say its schema.
A project's live board moves only by a build its main checkout says is its
own: where that checkout holds the board's source (crates/tasks/src/schema.rs),
any command of a build of that very source; where it holds none, only this
command, run in the main checkout itself. A scratch board (--database NAME)
moves here as under any command.

options:
  -h, --help  show this help message and exit
  --as ACTOR  taken and not used: moving the board acts as no one
";

/// `tasks migrate ARGS` (the words after `migrate`) in `env`: what it did goes to `out`, a refusal
/// to `err`; the exit code is returned.
pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{HELP}");
                return 0;
            }
            "--as" if words.next().is_some() => {}
            _ => {
                let _ = writeln!(
                    err,
                    "{USAGE}just tasks migrate: error: unrecognized arguments: {}",
                    args.join(" ")
                );
                return 2;
            }
        }
    }
    match migrate(env) {
        Ok(said) => {
            let _ = writeln!(out, "✔ {said}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}

fn migrate(env: &Env) -> Result<String, String> {
    let reached = reach(env, &env.cwd, true)?;
    let board = match &reached.live {
        Some(live) => {
            in_main(live, env)?;
            format!("the {} board", live.project)
        }
        None => format!(
            "the board in database {} at {}",
            reached.client.database(),
            reached.client.addr()
        ),
    };
    let before = version(&reached.client)?;
    let clock = Clock::of(env)?;
    let store = open_as(reached, &clock, Opening::write().migrating())?;
    let after = store
        .board_version()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(match before {
        None => format!("made {board} at schema {after}"),
        Some(before) if before < after => {
            format!("moved {board} from schema {before} to {after}")
        }
        Some(_) => format!("{board} is at schema {after}, this build's: nothing to move"),
    })
}

/// Refused unless `env` runs in the main checkout of `live`'s repository.
fn in_main(live: &crate::live::Live, env: &Env) -> Result<(), String> {
    let here = crate::git::checkout(&env.cwd).map(|(top, _)| top);
    match live.main() {
        Some(main) if here.as_deref() == Some(main) => Ok(()),
        Some(main) => Err(format!(
            "`tasks migrate` moves the {} board only from its main checkout, {}: nothing was read \
             or written; run it there (`cd {} && just tasks migrate`)",
            live.project,
            main.display(),
            main.display()
        )),
        None => Err(format!(
            "the {} board's repository has no main checkout to run `tasks migrate` from: nothing \
             was read or written",
            live.project
        )),
    }
}

/// The schema of the board `client` reaches, none when it is not made yet: one read of a client of
/// its own, which defines and writes nothing.
fn version(client: &Client) -> Result<Option<i64>, String> {
    Store::unchecked(
        Client::new(client.addr(), NAMESPACE, client.database()),
        Box::new(crate::store::now),
        Box::new(|_| true),
        "",
    )
    .board_version()
    .map_err(|e| e.to_string())
}
