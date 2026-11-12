//! `tasks database`: the board's database said, held stopped for work on its data, or released
//! ([`crate::server::Home::hold`]). Read before the board's argument table, as `migrate` is, and
//! run without opening the board, so none of it ever starts the database.

use super::{Env, machine};
use crate::DATABASE;
use crate::server::{Hold, Home, ServerError};
use std::io::Write;
use std::path::Path;

/// The word that runs it, read before the board's argument table.
pub const COMMAND: &str = "database";

/// Its usage line.
pub const USAGE: &str = "usage: just tasks database [-h] [hold WHY | release] [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks database [-h] [hold WHY | release] [--as ACTOR]

Say the state of the board's database, hold it stopped for work on its data (a
snapshot, a replay on a clone, a swap), or release the hold. None of it opens
the board, so none of it ever starts the database.

  just tasks database          whether it is held, by whom, since when and
                               why, and which server runs on its data
  just tasks database hold WHY write the hold (sf-tasks/hold in the git
                               common dir), then stop every server on the
                               data cleanly: asked to end, and waited for
                               while it writes its memtable out and closes
                               its store, never killed; one still starting
                               is waited for until it answers. While it is
                               held, every board command and the Symphony
                               are refused, naming the hold, and nothing
                               starts the database. Refused while another
                               holds it; its setter holding it again
                               finishes a stop left under way.
  just tasks database release  remove the hold; it starts nothing: the
                               board's next command starts the database

It acts on the board of the repository it runs in, never on a server
TASKS_SERVER names or for a scratch database, which shares the board's server.
Only builds that carry it honour the hold. A clean stop removes the log
segments the store's manifest counts as written: on a store that went back,
whose manifest counts a segment it never applied, clone the data first
(`cp -c -R`), while its server still runs.

options:
  -h, --help  show this help message and exit
  --as ACTOR  who holds or releases it (else TASKS_SESSION): taken as written,
              as the board is not read
";

/// What it was asked.
#[derive(Debug, PartialEq)]
enum Asked {
    Say,
    Hold(String),
    Release,
}

/// `tasks database ARGS` (the words after `database`) in `env`: what it did goes to `out`, a
/// refusal to `err`; the exit code is returned.
pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut actor = env.var("TASKS_SESSION").map(str::to_string);
    let mut words = Vec::new();
    let mut given = args.iter();
    while let Some(word) = given.next() {
        match word.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{HELP}");
                return 0;
            }
            "--as" => match given.next() {
                Some(named) => actor = Some(named.clone()),
                None => return usage(err, "argument --as: expected one argument"),
            },
            _ => words.push(word.as_str()),
        }
    }
    let asked = match words.as_slice() {
        [] => Asked::Say,
        ["hold", why] if !why.trim().is_empty() => Asked::Hold((*why).to_string()),
        ["hold", ..] => return usage(err, "hold takes one WHY, the reason in a few words"),
        ["release"] => Asked::Release,
        _ => {
            return usage(err, &format!("unrecognized arguments: {}", args.join(" ")));
        }
    };
    let home = match home(env) {
        Ok(home) => home,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    let actor = actor.filter(|actor| !actor.trim().is_empty());
    match (asked, actor) {
        (Asked::Say, _) => {
            let _ = writeln!(out, "{}", said(&home));
            0
        }
        (Asked::Hold(why), Some(by)) => hold(&home, &by, &why, out, err),
        (Asked::Release, Some(_)) => release(&home, out, err),
        (_, None) => {
            let _ = writeln!(
                err,
                "✖ no session: pass --as HANDLE or set TASKS_SESSION, the one who holds or \
                 releases the board's database"
            );
            2
        }
    }
}

fn usage(err: &mut dyn Write, said: &str) -> i32 {
    let _ = writeln!(err, "{USAGE}just tasks database: error: {said}");
    2
}

/// The board of the project `env`'s working directory is in, as [`super::reach_board`] finds it,
/// refused for a server `TASKS_SERVER` names, a scratch database, and a process Cargo runs from
/// that board's repository ([`crate::live::cargo_refusal`]).
fn home(env: &Env) -> Result<Home, String> {
    if env.var("TASKS_SERVER").is_some() {
        return Err(
            "TASKS_SERVER names a server by its address, which `tasks database` does not hold: \
             it holds the database of a project's board in its repository's common dir; run it \
             without TASKS_SERVER"
                .into(),
        );
    }
    if let Some(database) = env.var("TASKS_DATABASE").filter(|name| *name != DATABASE) {
        return Err(format!(
            "`tasks database` acts on the board's server, which serves the scratch database \
             {database} beside the project's board: holding it would stop that board too; run it \
             without --database or TASKS_DATABASE"
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
    Ok(Home::in_common(
        &project.place.common,
        env.var("SF_SURREAL").map(Path::new),
    ))
}

/// The board's database as it is: held or not, and what runs on its data.
fn said(home: &Home) -> String {
    let data = home.dir().join("data");
    let held = match home.held() {
        Some(hold) => format!(
            "the board's database at {} is {hold}; nothing starts it until `just tasks database \
             release --as HANDLE`",
            data.display()
        ),
        None => format!("the board's database at {} is not held", data.display()),
    };
    match home.serving() {
        Some(running) => format!(
            "{held}. Its server is pid {} at {}{}",
            running.pid,
            running.addr,
            if home.held().is_some() {
                ", still closing its store or still starting: wait until it ends before touching \
                 the data"
            } else {
                ""
            }
        ),
        None if home.held().is_some() => format!("{held}. No server runs on its data"),
        None => format!("{held}. No server runs on its data: the board's next command starts one"),
    }
}

fn hold(home: &Home, by: &str, why: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match home.hold(by, why) {
        Ok(holding) => {
            let stopped: Vec<String> = holding
                .stopped
                .iter()
                .map(|running| format!("pid {} at {}", running.pid, running.addr))
                .collect();
            let _ = writeln!(
                out,
                "✔ the board's database is {}. {}. Nothing serves {} now, and nothing starts a \
                 server on it until `just tasks database release --as {by}`",
                holding.hold,
                if stopped.is_empty() {
                    "No server ran on its data".to_string()
                } else {
                    format!("Stopped {} cleanly", stopped.join(" and "))
                },
                holding.data.display()
            );
            0
        }
        Err(ServerError::Held(hold)) => {
            let _ = writeln!(err, "✖ {}", ServerError::Held(hold));
            1
        }
        Err(problem) => {
            let _ = match home.held() {
                Some(hold) if hold.by == by => writeln!(
                    err,
                    "✖ the board's database is {hold}, and its stop is not done: {problem}. \
                     Nothing starts another server meanwhile; `just tasks database hold \"{why}\" \
                     --as {by}` again finishes it, `just tasks database` says what still runs"
                ),
                _ => writeln!(err, "✖ {problem}"),
            };
            2
        }
    }
}

fn release(home: &Home, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match home.release() {
        Ok(Some(Hold { by, why, at, .. })) => {
            #[allow(clippy::cast_precision_loss)] // a time in seconds is far inside f64's integers
            let since = crate::sys::local_minute(at as f64);
            let _ = writeln!(
                out,
                "✔ released the hold {by} set at {since} ({why}). Nothing was started: the \
                 board's next command starts the database"
            );
            0
        }
        Ok(None) => {
            let _ = writeln!(
                out,
                "✔ the board's database was not held: nothing to release"
            );
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::cli::Env;

    fn ran(args: &[&str], vars: &[(&str, &str)]) -> (i32, String, String) {
        let env = Env {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        };
        let args: Vec<String> = args.iter().map(|word| (*word).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, &env, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// Its help, and every malformed line refused with the usage before any board is looked for.
    #[test]
    fn the_help_and_the_usage_errors_come_before_any_board_is_found() {
        assert_eq!(
            ran(&["--help"], &[]),
            (0, super::HELP.into(), String::new())
        );
        for (args, said) in [
            (
                &["hold"][..],
                "hold takes one WHY, the reason in a few words",
            ),
            (
                &["hold", "  "],
                "hold takes one WHY, the reason in a few words",
            ),
            (
                &["hold", "a", "b"],
                "hold takes one WHY, the reason in a few words",
            ),
            (&["release", "now"], "unrecognized arguments: release now"),
            (&["stop"], "unrecognized arguments: stop"),
            (&["--as"], "argument --as: expected one argument"),
        ] {
            assert_eq!(
                ran(args, &[]),
                (
                    2,
                    String::new(),
                    format!("{}just tasks database: error: {said}\n", super::USAGE)
                ),
                "{args:?}"
            );
        }
    }

    /// A server an address names and a scratch database are refused before any board is found:
    /// the hold stops the whole server the project's board runs on.
    #[test]
    fn a_named_server_and_a_scratch_database_are_refused() {
        let (code, out, err) = ran(
            &["hold", "why", "--as", "a"],
            &[("TASKS_SERVER", "127.0.0.1:9")],
        );
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(
            err.starts_with("✖ TASKS_SERVER names a server by its address"),
            "{err}"
        );
        let (code, out, err) = ran(&[], &[("TASKS_DATABASE", "scratch1")]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(
            err.contains("serves the scratch database scratch1 beside the project's board"),
            "{err}"
        );
        // The board's own database by name is the board's: no repository here, so none is found.
        let machine = std::env::temp_dir().join(format!("database-machine-{}", std::process::id()));
        let machine = machine.display().to_string();
        let (_, _, err) = ran(
            &[],
            &[
                ("TASKS_DATABASE", "tasks"),
                ("SYMPHONY_MACHINE_DIR", &machine),
            ],
        );
        assert!(!err.contains("scratch database"), "{err}");
    }
}
