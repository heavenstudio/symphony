//! A command's callback dispatch, off its path. Run as the `tasks` binary ([`super::main_detaching`]),
//! a write that asks for a dispatch hands it to a detached `tasks dispatch` it starts
//! ([`Store::dispatching_by`]): the command answers once its own write commits, never after
//! deliveries to every recipient on the board. One dispatcher runs a board at a time
//! ([`Store::dispatch_detached`]); one that finds another running leaves it its ask and ends.

use super::{Clock, Env, open_unchecked, reach};
use crate::git::common_dir;
use crate::store::HandOff;
use crate::sweep::trigger;
use crate::sys::{self, Command, Stdio};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// The command a detached dispatcher runs: `tasks dispatch`, which no usage lists.
pub(super) const COMMAND: &str = "dispatch";

/// What the detached dispatchers of the repository's own board say, beside its server in
/// `sf-tasks/` in the git common dir.
pub const LOG: &str = "dispatch.log";

/// What a write on a board `env` reaches does with its dispatch: starts `program dispatch` detached,
/// in a session of its own, with `env`'s environment and working directory. Whether it started; the
/// write dispatches itself when it did not.
pub(super) fn hand_off(env: &Env, program: &Path) -> HandOff {
    let (env, program) = (env.clone(), program.to_path_buf());
    Box::new(move || match start(&env, &program) {
        Ok(()) => true,
        Err(why) => {
            eprintln!("CALLBACK no detached dispatcher ({why}); dispatching here");
            false
        }
    })
}

fn start(env: &Env, program: &Path) -> std::io::Result<()> {
    let mut command = Command::new(program);
    command
        .arg(COMMAND)
        .env_clear()
        .envs(&env.vars)
        .current_dir(&env.cwd)
        .stdin(Stdio::null())
        .new_session();
    match log(env) {
        Some(log) => command.stdout(log.try_clone()?).stderr(log),
        None => command.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    let mut child = command.spawn()?;
    // A long-running caller (`tasks api --lines`) reaps it; a command's own end leaves it to init.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Where a detached dispatcher's lines go: the dispatch log of the repository's own board, none for a
/// board `TASKS_SERVER` names (a test's or a measurement's).
fn log(env: &Env) -> Option<File> {
    if env.var("TASKS_SERVER").is_some() {
        return None;
    }
    let common = common_dir(&env.cwd).ok()?;
    trigger::append_log(&trigger::folder(&common), LOG).ok()
}

/// `tasks dispatch`: the passes the writes asked for, on the board `env` reaches, whose server it never
/// starts and whose tables the write that started it has already checked. What goes wrong is said on
/// `err` with the minute, for the dispatch log.
pub(super) fn run(env: &Env, err: &mut dyn Write) -> i32 {
    let store =
        Clock::of(env).and_then(|clock| Ok(open_unchecked(reach(env, &env.cwd, false)?, &clock)));
    match store.and_then(|store| store.dispatch_detached().map_err(|e| e.to_string())) {
        Ok(_) => 0,
        Err(why) => {
            let _ = writeln!(
                err,
                "[{}] CALLBACK deferred: {why}",
                sys::local_minute(trigger::now())
            );
            2
        }
    }
}
