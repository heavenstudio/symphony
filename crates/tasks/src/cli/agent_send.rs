//! The standalone agent messenger. Only transport and registration belong to the task board.

use super::{Env, connect_unfollowing};
use crate::Error;
use std::io::Write;

const USAGE: &str = "usage: just agent-send [--database NAME] HANDLE TEXT [--as SENDER]\n";

/// Send to a native channel, naming the sender and providing the same command back.
pub fn main(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if symphony_args::version(args, "agent-send", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
    let mut env = env.clone();
    let mut positional = Vec::new();
    let mut actor = None;
    let mut args = args.iter();
    let mut literal = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" if !literal => literal = true,
            "--help" | "-h" if !literal => {
                let _ = writeln!(
                    out,
                    "{USAGE}\nSend through the recipient's registered Claude socket or Codex thread. Sender defaults to TASKS_SESSION. Pending delivery stays in its inbox; no channel is a refusal. Use -- before positional text beginning with a dash. The board is the working directory's project's, or the one TASKS_SERVER (postgresql:SOCKET) and TASKS_DATABASE name; --database NAME is TASKS_DATABASE=NAME."
                );
                return 0;
            }
            "--as" | "--database" if !literal => {
                let Some(value) = args
                    .next()
                    .filter(|value| !value.is_empty() && !value.starts_with('-'))
                else {
                    let _ = writeln!(err, "{USAGE}✖ {arg} needs a value");
                    return 2;
                };
                if arg == "--as" {
                    actor = Some(value.clone());
                } else {
                    env.vars.insert("TASKS_DATABASE".into(), value.clone());
                }
            }
            value if !literal && value.starts_with('-') => {
                let _ = writeln!(
                    err,
                    "{USAGE}✖ unknown option {value}; use -- before literal text"
                );
                return 2;
            }
            _ => positional.push(arg),
        }
    }
    let [handle, text] = positional.as_slice() else {
        let _ = write!(err, "{USAGE}");
        return 2;
    };
    let Some(actor) = actor.as_deref().or_else(|| env.var("TASKS_SESSION")) else {
        let _ = writeln!(
            err,
            "✖ no sender: pass --as HANDLE or set TASKS_SESSION (just tasks register)"
        );
        return 2;
    };
    let store = match connect_unfollowing(&env) {
        Ok(store) => store,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    match store.send_agent(actor, handle, text) {
        Ok(true) => {
            let _ = writeln!(out, "sent from {actor} to {handle}");
            0
        }
        Ok(false) => {
            let board = store.board_command();
            let _ = writeln!(
                out,
                "delivery from {actor} to {handle} not yet confirmed; native delivery may still be in progress or retried; retained for the inbox until then ({board} inbox --as {handle}); do not resend"
            );
            0
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            if matches!(why, Error::Refused(_)) {
                1
            } else {
                2
            }
        }
    }
}
