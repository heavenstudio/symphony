//! `tasks database`: the board's database said, held stopped for work on its data, or released
//! ([`crate::server::Postgres::hold`], [`crate::server::Home::release`]). Read before the board's
//! argument table, as `migrate` is, and run without opening the board, so none of it ever starts
//! the database. A board still on SurrealDB is refused, naming how it moves
//! ([`crate::server::GONE`]), but for a release, which starts nothing.

use super::{Env, machine};
use crate::DATABASE;
use crate::server::{Engine, Hold, Home, ServerError};
use std::io::Write;

/// The word that runs it, read before the board's argument table.
pub const COMMAND: &str = "database";

/// Its usage, wrapped as argparse wraps a line past 78 columns.
pub const USAGE: &str = "usage: just tasks database [-h] [hold WHY | release [--of HOLDER]]
                           [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks database [-h] [hold WHY | release [--of HOLDER]]
                           [--as ACTOR]

Say the state of the board's database, hold it stopped for work on its data (a
snapshot, a restore, a swap), or release the hold. None of it opens the board,
so none of it ever starts the database.

  just tasks database          whether it is held, by whom, since when and
                               why, and which server runs on its data
  just tasks database hold WHY write the hold (symphony/tasks/hold in the
                               git common dir), then stop the board's
                               PostgreSQL: its fast shutdown, waited for
                               until it has ended. While it is held, every
                               board command and the Symphony are refused,
                               naming the hold, and nothing starts the
                               database. Refused while another holds it;
                               its setter holding it again finishes a stop
                               left under way.
  just tasks database release  remove the hold; it starts nothing: the
                               board's next command starts the database.
                               Only its holder releases it, or another
                               handle naming the holder with --of HOLDER

A PostgreSQL snapshot is put back over its board by `tasks-move --restore`, a
program of its own (its --help). A board still on SurrealDB is refused, naming
how it moves.

Each hold and release is a line of symphony/tasks/hold.log. It acts on the
board of the repository it runs in, never on a server TASKS_SERVER names or
for a scratch database, which shares the board's server. Only builds that
carry it honour the hold (docs/operations/BOARD_OPERATIONS.md).

options:
  -h, --help     show this help message and exit
  --of HOLDER    release a hold another handle set, naming that holder
  --as ACTOR     who holds or releases it (else TASKS_SESSION): taken as
                 written, as the board is not read
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
    let mut of = None;
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
            "--of" => match given.next() {
                Some(named) if !named.trim().is_empty() => of = Some(named.clone()),
                _ => return usage(err, "argument --of: expected one argument"),
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
    if of.is_some() && asked != Asked::Release {
        return usage(err, "--of names the holder of a hold to release");
    }
    let home = match home(env) {
        Ok(home) => home,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    let actor = actor.filter(|actor| !actor.trim().is_empty());
    let no_session = |err: &mut dyn Write| {
        let _ = writeln!(
            err,
            "✖ no session: pass --as HANDLE or set TASKS_SESSION, the one who holds or releases \
             the board's database"
        );
        2
    };
    // A release starts nothing and stops nothing, on whatever engine the board is.
    if asked == Asked::Release {
        return match actor {
            Some(by) => release(&home, &by, of.as_deref(), out, err),
            None => no_session(err),
        };
    }
    let pg = (|| -> Result<crate::server::Postgres, String> {
        if home.backend().map_err(|why| why.to_string())? == Engine::SurrealDb {
            return Err(home
                .on_surrealdb("Nothing was held, stopped or read")
                .to_string());
        }
        let project = crate::project::find(&env.cwd, machine(env).as_deref())?;
        home.project_postgresql(&project.place.root)
            .map_err(|why| why.to_string())
    })();
    let pg = match pg {
        Ok(pg) => pg,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    match (&asked, actor.as_deref()) {
        (Asked::Say, _) => match pg.current() {
            Ok(serving) => {
                let _ = writeln!(
                    out,
                    "the board's PostgreSQL at {} is {}; {}",
                    pg.data().display(),
                    pg.held()
                        .map_or_else(|| "not held".into(), |hold| hold.to_string()),
                    serving.map_or_else(
                        || "no server runs on its data".into(),
                        |server| format!("pid {} on {}", server.pid, server.socket.display())
                    )
                );
                0
            }
            Err(why) => {
                let _ = writeln!(err, "✖ {why}");
                2
            }
        },
        (Asked::Hold(why), Some(by)) => match pg.hold(by, why) {
            Ok(holding) => {
                let _ = writeln!(
                    out,
                    "✔ the board's PostgreSQL is {}. Nothing serves {} now",
                    holding.hold,
                    holding.data.display()
                );
                0
            }
            Err(why) => {
                let _ = writeln!(err, "✖ {why}");
                2
            }
        },
        _ => no_session(err),
    }
}

fn usage(err: &mut dyn Write, said: &str) -> i32 {
    let _ = writeln!(err, "{USAGE}just tasks database: error: {said}");
    2
}

/// The board of the project `env`'s working directory is in, as [`super::reach_board`] finds it,
/// refused for a server `TASKS_SERVER` names, a scratch database and a process Cargo runs from that
/// board's repository ([`crate::live::cargo_refusal`]).
pub(crate) fn home(env: &Env) -> Result<Home, String> {
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
    Ok(Home::in_common(&project.place.common))
}

fn release(
    home: &Home,
    by: &str,
    of: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match home.release(by, of) {
        Ok(Some(Hold {
            by: holder,
            why,
            at,
            ..
        })) => {
            #[allow(clippy::cast_precision_loss)] // a time in seconds is far inside f64's integers
            let since = crate::sys::local_minute(at as f64);
            if holder != by {
                let _ = writeln!(
                    err,
                    "· {by} released the hold {holder} set at {since} ({why}), as \
                     symphony/tasks/hold.log records"
                );
            }
            let _ = writeln!(
                out,
                "✔ released the hold {holder} set at {since} ({why}). Nothing was started: the \
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
        Err(problem @ ServerError::NotHolder(_)) => {
            let _ = writeln!(err, "✖ {problem}");
            1
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

    /// Its help and usage are laid out 78 columns wide, as every help of `tasks` is.
    #[test]
    fn the_help_fits_78_columns() {
        for line in super::HELP.lines().chain(super::USAGE.lines()) {
            assert!(line.chars().count() <= 78, "{line}");
        }
        assert!(super::HELP.starts_with(super::USAGE));
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
            (&["release", "--of"], "argument --of: expected one argument"),
            (
                &["release", "--of", " "],
                "argument --of: expected one argument",
            ),
            (
                &["hold", "why", "--of", "a"],
                "--of names the holder of a hold to release",
            ),
            (&["--of", "a"], "--of names the holder of a hold to release"),
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
