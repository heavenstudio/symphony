//! The worktree sweep takes its nonblocking lock before opening a board connection.

use super::{Clock, Env, args, open, reach};
use crate::{sweep, sys};
use std::io::Write;

pub(super) fn run(
    parsed: &args::Parsed,
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let request = sweep::Request {
        dry_run: parsed.flag("dry_run"),
        hours: parsed
            .get("recent_hours")
            .map(|_| parsed.float("recent_hours", 0.0)),
        tasks: parsed.all("task").iter().map(String::as_str).collect(),
        background: parsed.get("background"),
    };
    let mut connection_failed = false;
    let result = sweep::command_connect(env, &request, || {
        let connected = Clock::of(env)
            .and_then(|clock| open(reach(env, &env.cwd, request.background.is_none())?, &clock));
        connection_failed = connected.is_err();
        connected
    });
    match result {
        Ok(lines) => {
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            0
        }
        Err(why) => {
            let why = request
                .background
                .filter(|_| !connection_failed)
                .map_or_else(
                    || why.clone(),
                    |started| {
                        format!(
                            "[{}] {started}: {why}",
                            sys::local_minute(sweep::trigger::now())
                        )
                    },
                );
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}
