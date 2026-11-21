//! `symphony-github`: the adapter's commands, run in a checkout of the project
//! (`symphony_github::cli`).

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
use symphony_github::board::{Api, Process};
use symphony_github::config::{Committed, checkouts};
use symphony_github::reconcile::Context;
use symphony_github::transport::{Curl, GITHUB, Transport};

/// The board's program: the one named, else the `tasks` beside this program, else the one on
/// `PATH`.
fn tasks(named: Option<String>) -> PathBuf {
    if let Some(named) = named {
        return named.into();
    }
    std::env::current_exe()
        .ok()
        .and_then(|me| Some(me.parent()?.join("tasks")))
        .filter(|beside| beside.is_file())
        .unwrap_or_else(|| "tasks".into())
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let named = args
        .iter()
        .position(|arg| arg == "--tasks")
        .filter(|at| at + 1 < args.len())
        .map(|at| {
            args.remove(at);
            args.remove(at)
        });
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    if symphony_github::cli::answered_without_a_project(&args, &mut out) {
        return ExitCode::SUCCESS;
    }
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            let _ = writeln!(err, "symphony-github: no working directory: {error}");
            return ExitCode::from(2);
        }
    };
    let repository = match checkouts(&cwd) {
        Ok(folders) => folders,
        Err(why) => {
            let _ = writeln!(err, "symphony-github: {why}");
            return ExitCode::from(2);
        }
    };
    let source = Committed {
        main: repository[0].clone(),
    };
    let board = Api(Process::new(&tasks(named), &cwd));
    let connect =
        |token| -> Result<Box<dyn Transport>, String> { Ok(Box::new(Curl::new(GITHUB, token)?)) };
    let ctx = Context {
        source: &source,
        board: &board,
        connect: &connect,
        now: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            }),
        repository,
        openssl: "openssl",
        gh: "gh",
        rest: &symphony_github::deliver::rest,
    };
    let code = symphony_github::cli::run(&args, &ctx, &mut out, &mut err);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
