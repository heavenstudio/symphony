//! `tasks migrate`: the board moved forward to this build's schema, or made, and its schema said.
//! On a project's live board it runs only in the main checkout, or in any checkout of a repository
//! with none, and it is what alone moves a live board whose default branch commits no board source
//! ([`crate::live::may_move`]); on a scratch board it moves it as any writer would.

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

#[cfg(test)]
mod tests {
    use super::in_main;
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
}
