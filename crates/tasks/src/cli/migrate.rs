//! `tasks migrate`: the board moved forward to this build's schema, or made, and its schema said; a
//! board of this build's contract lacking additive steps it has takes them
//! ([`crate::schema::ADDED`]), with no writer held, as every build of the contract writes it after.
//! On a project's live board it runs only in the main checkout, or in any checkout of a repository
//! with none, and it is what alone moves a live board whose default branch commits no board source
//! ([`crate::live::may_move`]); on a scratch board it moves it as any writer would. It moves nothing
//! while a writer seen in the last week is behind the schema it would move to, naming each, unless
//! `--leave-behind` says to ([`crate::Store::opening`]).

use super::{Clock, Env, open_as_database, reach};
use crate::rpc::Client;
use crate::{NAMESPACE, Opening, Store};
use std::io::Write;

/// The word that runs it, read before the board's argument table, as `run` is.
pub const COMMAND: &str = "migrate";

/// Its usage line.
pub const USAGE: &str = "usage: just tasks migrate [-h] [--leave-behind] [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks migrate [-h] [--leave-behind] [--as ACTOR]

Move the board forward to this build's schema, or make it, and say its schema.
A project's live board moves only by a build its default branch ([project]
default_branch in the main checkout's symphony.toml, else main) says is its
own, by what that branch commits, whatever a checkout holds: where it commits
the board's source (crates/tasks/src/schema.rs), any command of a build of
that very source; where it commits none, only this command, run in the main
checkout itself, or in any checkout of a repository with none (a bare
repository's worktrees, a submodule). A default branch that cannot be read (one
default_branch names that does not exist, a ref that names no commit) moves it
by no command, this one included. A scratch board (--database NAME) moves here
as under any command.

Each program that opens the board to write keeps a line on it: its name, where
it is, its host, the schema it was built at and when it was last seen. Before
it moves the board, this command names each writer seen in the last week that
is behind the schema it would move it to, which would be refused the board
from then on, and moves nothing: rebuild each and run it once, then migrate
again.

options:
  -h, --help      show this help message and exit
  --leave-behind  move the board though writers seen in the last week are
                  behind, leaving them refused until they are rebuilt
  --as ACTOR      taken and not used: moving the board acts as no one
";

/// `tasks migrate ARGS` (the words after `migrate`) in `env`: what it did goes to `out`, a refusal
/// to `err`; the exit code is returned.
pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut words = args.iter();
    let mut leave_behind = false;
    while let Some(word) = words.next() {
        match word.as_str() {
            "-h" | "--help" => {
                let _ = write!(out, "{HELP}");
                return 0;
            }
            "--leave-behind" => leave_behind = true,
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
    match migrate(env, leave_behind) {
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

/// The board `env` reaches moved forward, or made, and what was done said; refused while a writer
/// seen in the last week is behind the schema it would move to, unless `leave_behind`.
fn migrate(env: &Env, leave_behind: bool) -> Result<String, String> {
    let reached = reach(env, &env.cwd, true)?;
    let version_reader = Store::unchecked_on(
        reached.database()?,
        Box::new(crate::store::now),
        Box::new(|_| true),
        "",
        &Opening::read(),
    );
    let mut before = None;
    let board = match &reached.live {
        Some(live) => {
            // A board that went back is refused as one wherever this runs, before where it runs is
            // asked: a worktree's refusal must not send anyone to run a migrate in the main checkout.
            if let Some(back) = crate::live::seen::checked(live, || {
                let version = version_reader
                    .board_version()
                    .map_err(|why| why.to_string())?;
                before = Some(version);
                Ok(version)
            })? {
                return Err(back.refusal());
            }
            in_main(live, env)?;
            format!("the {} board", live.project)
        }
        None => format!(
            "the board in database {} at {}",
            reached.client.database(),
            reached.client.addr()
        ),
    };
    let before = match before {
        Some(before) => before,
        None => version_reader
            .board_version()
            .map_err(|why| why.to_string())?,
    };
    // The steps of its contract it has, as far as this build's go.
    let steps_before = version_reader.additions().map_err(|why| why.to_string())?;
    let clock = Clock::of(env)?;
    let how = Opening::write().migrating();
    let how = if leave_behind {
        how.leave_behind()
    } else {
        how
    };
    let store = open_as_database(reached, version_reader.into_database(), &clock, how)?;
    let after = store
        .board_version()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let steps = store.additions().map_err(|e| e.to_string())?;
    Ok(moved(&board, before, after, (steps_before, steps)))
}

/// What a migrate of `board` says: made at schema `after`, its version `before` moved to it, or,
/// at the same version, its additive steps moved from the first of `steps` to the second; else
/// nothing moved.
fn moved(board: &str, before: Option<i64>, after: i64, steps: (i64, i64)) -> String {
    match before {
        None => format!("made {board} at schema {after}"),
        Some(before) if before < after => {
            format!("moved {board} from schema {before} to {after}")
        }
        Some(_) if steps.0 < steps.1 => format!(
            "moved {board} at schema {after} from {} to {}",
            additive(steps.0),
            additive(steps.1)
        ),
        Some(_) => format!("{board} is at schema {after}, this build's: nothing to move"),
    }
}

/// `steps` additive steps, as a line says them.
fn additive(steps: i64) -> String {
    match steps {
        0 => "no additive step".into(),
        1 => "1 additive step".into(),
        steps => format!("{steps} additive steps"),
    }
}

/// Refused unless `env` runs in the main checkout of `live`'s repository, or, where it has none (a
/// bare repository's worktrees, a submodule's working tree), in any checkout of it.
fn in_main(live: &crate::live::Live, env: &Env) -> Result<(), String> {
    match live.main() {
        Some(main) => {
            let here = crate::git::checkout(&env.cwd).map(|(top, _)| top);
            if here.as_deref() == Some(main) {
                return Ok(());
            }
            Err(format!(
                "`tasks migrate` moves the {} board only from its main checkout, {}: nothing was \
                 read or written; run it there (`cd {} && just tasks migrate`)",
                live.project,
                main.display(),
                main.display()
            ))
        }
        None if crate::git::read(&env.cwd).as_deref() == Some(live.common.as_path()) => Ok(()),
        None => Err(format!(
            "`tasks migrate` moves the {} board only from a checkout of its repository, which has \
             no main checkout, so any of them: nothing was read or written; run it in a checkout \
             of the repository at {}",
            live.project,
            live.common.display()
        )),
    }
}

/// The schema of the board `client` reaches, none when it is not made yet: one read of a client of
/// its own, which defines and writes nothing.
pub(super) fn version(client: &Client) -> Result<Option<i64>, String> {
    Store::unchecked_on(
        crate::Database::named(
            &Client::new(client.addr(), NAMESPACE, client.database()).waiting(client.wait_limit()),
        )?,
        Box::new(crate::store::now),
        Box::new(|_| true),
        "",
        &Opening::read(),
    )
    .board_version()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{additive, in_main, moved};
    use crate::cli::Env;
    use crate::live::Live;
    use std::fs;

    /// Where `tasks migrate` runs for a live board: its main checkout, else any checkout of its
    /// repository, and nowhere else.
    #[test]
    fn migrate_runs_in_the_main_checkout_or_in_any_checkout_of_a_repository_with_none() {
        let dir = std::env::temp_dir().join(format!("migrate-in-main-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        // A main checkout, a bare repository with a linked worktree, and a folder in neither.
        fs::create_dir_all(dir.join("main/.git")).unwrap();
        fs::write(dir.join("main/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(dir.join("main/docs")).unwrap();
        let bare = dir.join("bare.git");
        fs::create_dir_all(bare.join("worktrees/wt")).unwrap();
        fs::write(bare.join("worktrees/wt/commondir"), "../..\n").unwrap();
        fs::write(bare.join("worktrees/wt/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(dir.join("wt")).unwrap();
        fs::write(
            dir.join("wt/.git"),
            format!("gitdir: {}\n", bare.join("worktrees/wt").display()),
        )
        .unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        let at = |cwd: &str| Env {
            vars: Default::default(),
            cwd: dir.join(cwd),
        };
        let main = Live {
            project: "alpha".into(),
            common: dir.join("main/.git"),
        };
        assert_eq!(in_main(&main, &at("main/docs")), Ok(()));
        assert!(in_main(&main, &at("outside")).is_err());
        let loose = Live {
            project: "loose".into(),
            common: bare.clone(),
        };
        assert_eq!(in_main(&loose, &at("wt")), Ok(()));
        for elsewhere in ["outside", "main"] {
            assert_eq!(
                in_main(&loose, &at(elsewhere)),
                Err(format!(
                    "`tasks migrate` moves the loose board only from a checkout of its \
                     repository, which has no main checkout, so any of them: nothing was read or \
                     written; run it in a checkout of the repository at {}",
                    bare.display()
                ))
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_move_of_additive_steps_says_how_many_the_board_had_and_has() {
        assert_eq!(
            [additive(0), additive(1), additive(2)],
            ["no additive step", "1 additive step", "2 additive steps"]
        );
        let board = "the board in database tasks";
        assert_eq!(
            moved(board, None, 28, (0, 1)),
            "made the board in database tasks at schema 28"
        );
        // A move of the version says the versions alone, whatever steps came with it.
        assert_eq!(
            moved(board, Some(27), 28, (0, 2)),
            "moved the board in database tasks from schema 27 to 28"
        );
        assert_eq!(
            moved(board, Some(28), 28, (0, 1)),
            "moved the board in database tasks at schema 28 from no additive step to 1 additive step"
        );
        assert_eq!(
            moved(board, Some(28), 28, (1, 3)),
            "moved the board in database tasks at schema 28 from 1 additive step to 3 additive steps"
        );
        // A board with as many steps as this build, or more, which it leaves as they are.
        for steps in [(0, 0), (2, 2), (3, 1)] {
            assert_eq!(
                moved(board, Some(28), 28, steps),
                "the board in database tasks is at schema 28, this build's: nothing to move"
            );
        }
    }
}
