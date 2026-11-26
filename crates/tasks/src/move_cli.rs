//! `tasks-move`: a PostgreSQL snapshot put back over the board
//! ([`server::Postgres::restore_over`]), or a move an earlier release's `tasks-move` left
//! standing settled ([`server::Home::move_to`]). A program of its own, as `tasks-copy` is: the
//! owner's to run, never a board command, so `tasks` links none of the copy it makes.

use crate::cli::{Env, machine};
use crate::server::{self, Engine, Home};
use std::io::Write;
use std::path::Path;

/// Its usage, said when its arguments are none of its forms.
pub const MOVE_USAGE: &str = "usage: tasks-move [-h] [--version]
                  (--restore SNAPSHOT | postgresql | surrealdb) [--as ACTOR]
";

/// Its `--help`.
pub const MOVE_HELP: &str = "usage: tasks-move [-h] [--version]
                  (--restore SNAPSHOT | postgresql | surrealdb) [--as ACTOR]

Put a snapshot of the PostgreSQL of the board in this checkout's git common
dir back over it, or settle a move an earlier release's tasks-move left
standing. Either holds the board meanwhile: a hold its caller set before
(`just tasks database hold`) is used and left standing, otherwise one is set
and released at the end.

  tasks-move --restore SNAPSHOT --as ACTOR
      Put a snapshot of the board's PostgreSQL (`just tasks snapshot --list`,
      by its name or path) back over the board it was taken from, under a
      journal (symphony/tasks/move.json): the cluster in place moved into
      symphony/tasks/moved/, the copy in, the commit. A failure before the
      commit leaves the board on the cluster it had.
  tasks-move ENGINE --as ACTOR
      Settle a journal an earlier release's move left, finishing what
      committed and undoing what did not. This build moves no board between
      the engines: a board still on SurrealDB moves to PostgreSQL with an
      earlier release's tasks-move (docs/operations/POSTGRESQL.md).

Nothing is deleted but the restore's own work folder. While the journal
stands nothing starts or reads the store and the hold is not released: its
holder's next tasks-move finishes what committed and undoes what did not.

Afterwards the board holds a new recovery generation and is bound to none:
`just tasks recovery rebind` binds it again
(docs/operations/BOARD_OPERATIONS.md). Under SYMPHONY_UNASKED, which the board
recipes set, a board outside a scratch place is refused: a guard against
accident, which grants nothing by its absence.

options:
  -h, --help          show this help message and exit
  --version           say the version and exit
  --restore SNAPSHOT  a snapshot of this board's PostgreSQL to put back
  --as ACTOR          who holds the board for it (else TASKS_SESSION)
";

/// What it was asked.
#[derive(Debug, PartialEq)]
enum Asked {
    Move(Engine),
    Restore(String),
}

/// `tasks-move ARGS` in `env`: what it did goes to `out`, a refusal to `err`; the exit code is
/// returned.
pub fn move_main(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if symphony_args::version(args, "tasks-move", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
    let mut actor = env.var("TASKS_SESSION").map(str::to_string);
    let mut restore = None;
    let mut words = Vec::new();
    let mut given = args.iter();
    while let Some(word) = given.next() {
        match word.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{MOVE_HELP}");
                return 0;
            }
            "--as" => match given.next() {
                Some(named) => actor = Some(named.clone()),
                None => return usage(err, "argument --as: expected one argument"),
            },
            "--restore" => match given.next() {
                Some(named) if !named.trim().is_empty() => restore = Some(named.clone()),
                _ => return usage(err, "argument --restore: expected one SNAPSHOT"),
            },
            _ => words.push(word.as_str()),
        }
    }
    let asked = match (words.as_slice(), restore) {
        ([], Some(snapshot)) => Asked::Restore(snapshot),
        ([engine], None) => match Engine::named(engine) {
            Some(engine) => Asked::Move(engine),
            None => return usage(err, "the engine is postgresql or surrealdb"),
        },
        _ => return usage(err, "one of --restore SNAPSHOT, postgresql or surrealdb"),
    };
    let Some(by) = actor.filter(|actor| !actor.trim().is_empty()) else {
        let _ = writeln!(
            err,
            "✖ no session: pass --as HANDLE or set TASKS_SESSION, the one who holds the board's \
             database while it is moved or restored"
        );
        return 2;
    };
    match crate::cli::database::home(env).and_then(|home| switched(&asked, &home, env, &by)) {
        Ok(done) => {
            let _ = writeln!(out, "✔ {done}");
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}

fn usage(err: &mut dyn Write, said: &str) -> i32 {
    let _ = writeln!(err, "{MOVE_USAGE}tasks-move: error: {said}");
    2
}

/// A move or a restore of `home`'s store as `by`: what it did.
fn switched(asked: &Asked, home: &Home, env: &Env, by: &str) -> Result<String, String> {
    if env.var(crate::confine::MARKER).is_some() && !server::scratch(home.dir()) {
        return Err(format!(
            "{} is set, as the board recipes set it, and the board at {} is outside a scratch \
             place: its store is not moved or restored under it. Nothing was touched",
            crate::confine::MARKER,
            home.dir().display()
        ));
    }
    let rebind = "It holds a new recovery generation and is bound to none: `just tasks recovery \
                  rebind` binds it again";
    match asked {
        Asked::Move(to) => {
            let moved = (home.move_to(*to, by)).map_err(|why| why.to_string())?;
            Ok(format!(
                "the board is on {}: a move that had committed is finished. Its {} store is kept \
                 in {}, with its export. {rebind}. {}",
                moved.to,
                moved.from,
                moved.aside.display(),
                if moved.held {
                    "The board is still held, by the hold set before the move"
                } else {
                    "The hold is released: the board's next command starts its server"
                }
            ))
        }
        Asked::Restore(snapshot) => {
            let project = crate::project::find(&env.cwd, machine(env).as_deref())?;
            let programs = server::postgres::discover_programs(&project.place.root)?;
            let pg = home.postgresql(&programs);
            let named = Path::new(snapshot);
            let path = if named.components().count() == 1 {
                (pg.snapshots().map_err(|why| why.to_string())?.into_iter())
                    .find(|path| path.file_name() == Some(named.as_os_str()))
                    .ok_or_else(|| {
                        format!(
                            "{snapshot} is no snapshot of this board's PostgreSQL (`just tasks \
                             snapshot --list`): nothing was restored"
                        )
                    })?
            } else {
                env.cwd.join(named)
            };
            let now = crate::store::now();
            let aside = (pg.restore_over(&path, by, now)).map_err(|why| why.to_string())?;
            Ok(format!(
                "the board's PostgreSQL is the snapshot {}. The cluster it replaced is kept in \
                 {}. {rebind}",
                path.display(),
                aside.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ran(args: &[&str]) -> (i32, String, String) {
        let env = Env {
            cwd: std::env::temp_dir(),
            vars: BTreeMap::new(),
        };
        let args: Vec<String> = args.iter().map(|word| (*word).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = move_main(&args, &env, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// Its help and usage are laid out 78 columns wide, as every help of the board's is.
    #[test]
    fn the_help_fits_78_columns() {
        for line in MOVE_HELP.lines().chain(MOVE_USAGE.lines()) {
            assert!(line.chars().count() <= 78, "{line}");
        }
        assert!(MOVE_HELP.starts_with(MOVE_USAGE));
    }

    /// Its help, its version and every malformed line come before any board is looked for.
    #[test]
    fn the_help_the_version_and_the_usage_errors_come_before_any_board_is_found() {
        assert_eq!(ran(&["--help"]), (0, MOVE_HELP.into(), String::new()));
        assert_eq!(ran(&["postgresql", "-h"]).0, 0);
        let (code, out, _) = ran(&["--version"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("tasks-move "), "{out}");
        const BOTH: &str = "one of --restore SNAPSHOT, postgresql or surrealdb";
        for (args, said) in [
            (&[][..], BOTH),
            (&["oracle"], "the engine is postgresql or surrealdb"),
            (&["postgresql", "surrealdb"], BOTH),
            (&["postgresql", "--restore", "x"], BOTH),
            (&["--restore"], "argument --restore: expected one SNAPSHOT"),
            (
                &["--restore", " "],
                "argument --restore: expected one SNAPSHOT",
            ),
            (
                &["postgresql", "--as"],
                "argument --as: expected one argument",
            ),
        ] {
            let (code, out, err) = ran(args);
            assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
            assert_eq!(err, format!("{MOVE_USAGE}tasks-move: error: {said}\n"));
        }
        let (code, _, err) = ran(&["postgresql"]);
        assert_eq!(code, 2);
        assert!(err.contains("no session: pass --as HANDLE"), "{err}");
        let (code, _, err) = ran(&["postgresql", "--as", "  "]);
        assert_eq!(code, 2);
        assert!(err.contains("no session"), "{err}");
    }
}
