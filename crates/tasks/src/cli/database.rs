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

/// Its usage, wrapped as argparse wraps a line past 78 columns.
pub const USAGE: &str = "usage: just tasks database [-h] [hold WHY | release [--of HOLDER]]
                           [--as ACTOR]\n";

/// Its `--help`.
pub const HELP: &str = "usage: just tasks database [-h] [hold WHY | release [--of HOLDER]]
                           [--as ACTOR]

Say the state of the board's database, hold it stopped for work on its data (a
snapshot, a replay on a clone, a swap), or release the hold. None of it opens
the board, so none of it ever starts the database.

  just tasks database          whether it is held, by whom, since when and
                               why; which server runs on its data; what a
                               start, stop or replay under way does; and
                               the snapshots that hold a log, kept until
                               the owner removes them
  just tasks database hold WHY write the hold (symphony/tasks/hold in the
                               git common dir), then stop every server on
                               the data cleanly: asked to end, and waited for
                               while it writes its memtable out and closes
                               its store, never killed; one still starting
                               is waited for until it answers. While it is
                               held, every board command and the Symphony
                               are refused, naming the hold, and nothing
                               starts the database. Refused while another
                               holds it; its setter holding it again
                               finishes a stop left under way.
  just tasks database release  remove the hold; it starts nothing: the
                               board's next command starts the database.
                               Only its holder releases it, or another
                               handle naming the holder with --of HOLDER

The board's store is moved to the other engine, and a PostgreSQL snapshot put
back over its board, by `tasks-move`, a program of its own (its --help).

Each hold and release is a line of symphony/tasks/hold.log. It acts on the
board of the repository it runs in, never on a server TASKS_SERVER names or
for a scratch database, which shares the board's server. Only builds that
carry it honour the hold (docs/BOARD_OPERATIONS.md). It reads no record of
the schema the board was seen at, so it holds a board that went back as any
other: that is step 2 of its recovery. A clean stop removes the log segments
the store's manifest counts as written: on a store that went back, whose
manifest counts a segment it never applied, keep the log first: the snapshot
taken before the start on the older state (`just tasks snapshot --list`), else
a clone of the data (`cp -c -R`) while its server still runs. A start that
replays the log takes such a snapshot itself.

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
    let pg = (|| -> Result<Option<crate::server::Postgres>, String> {
        if home.backend().map_err(|why| why.to_string())? != crate::server::Engine::PostgreSql {
            return Ok(None);
        }
        let project = crate::project::find(&env.cwd, machine(env).as_deref())?;
        home.project_postgresql(&project.place.root)
            .map(Some)
            .map_err(|why| why.to_string())
    })();
    let pg = match pg {
        Ok(pg) => pg,
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            return 2;
        }
    };
    if let Some(pg) = pg {
        match (&asked, actor.as_deref()) {
            (Asked::Say, _) => {
                return match pg.current() {
                    Ok(serving) => {
                        let _ = writeln!(
                            out,
                            "the board's PostgreSQL at {} is {}; {}",
                            pg.data().display(),
                            pg.held()
                                .map_or_else(|| "not held".into(), |hold| hold.to_string()),
                            serving.map_or_else(
                                || "no server runs on its data".into(),
                                |server| format!(
                                    "pid {} on {}",
                                    server.pid,
                                    server.socket.display()
                                )
                            )
                        );
                        0
                    }
                    Err(why) => {
                        let _ = writeln!(err, "✖ {why}");
                        2
                    }
                };
            }
            (Asked::Hold(why), Some(by)) => {
                return match pg.hold(by, why) {
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
                };
            }
            _ => {}
        }
    }
    match (asked, actor) {
        (Asked::Say, _) => {
            let _ = writeln!(out, "{}", said(&home));
            0
        }
        (Asked::Hold(why), Some(by)) => hold(&home, &by, &why, out, err),
        (Asked::Release, Some(by)) => release(&home, &by, of.as_deref(), out, err),
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
    Ok(Home::in_common(
        &project.place.common,
        env.read(symphony_env::SURREAL).map(Path::new),
    ))
}

/// The board's database as it is: held or not, what runs on its data, what a start, stop or
/// replay under way does, and the snapshots that hold a log, kept until the owner removes them.
fn said(home: &Home) -> String {
    let data = home.dir().join("data");
    let held = home.held();
    let mut said = match &held {
        Some(hold) => format!(
            "the board's database at {} is {hold}; nothing starts it until its holder releases it",
            data.display()
        ),
        None => format!("the board's database at {} is not held", data.display()),
    };
    let under_way = home.under_way();
    let replaying = under_way
        .as_deref()
        .is_some_and(|doing| doing.contains("a private replay of the store's log"));
    let (left, untold) = match home.left_replay_told() {
        Ok(left) => (left, None),
        Err(why) => (None, Some(why)),
    };
    match home.serving() {
        Some(running) if replaying && home.recorded().is_none_or(|r| r.pid != running.pid) => {
            said.push_str(&format!(
                ". Its private replay's server is pid {} at {}: wait until it ends",
                running.pid, running.addr
            ));
        }
        Some(running)
            if left
                .as_ref()
                .is_some_and(|left| left.named && left.server.pid == running.pid) =>
        {
            said.push_str(&format!(
                ". Its private replay's server is pid {} at {}, left running when its starter \
                 ended: never the board's server, it is stopped cleanly by the board's next \
                 command once it answers (never before), which then starts the board's own",
                running.pid, running.addr
            ));
        }
        Some(running)
            if left
                .as_ref()
                .is_some_and(|left| left.server.pid == running.pid) =>
        {
            said.push_str(&format!(
                ". Its server is pid {} at {}, which the private replay's record \
                 (symphony/tasks/replay.json) does not name: while that record stands every server \
                 on the data is taken for the replay's, never the board's, so the board's next \
                 command stops it cleanly once it answers (never before), then starts the \
                 board's own",
                running.pid, running.addr
            ));
        }
        Some(running) => said.push_str(&format!(
            ". Its server is pid {} at {}{}",
            running.pid,
            running.addr,
            if held.is_some() {
                ", on held data: one the hold's stop waits for, still closing its store or still \
                 starting, or one a build that does not honour the hold started (docs/BOARD_OPERATIONS.md); \
                 wait until it ends before touching the data"
            } else {
                ""
            }
        )),
        None if untold.is_some() => said.push_str(". No server is found on its data"),
        None if held.is_some() => said.push_str(". No server runs on its data"),
        None => {
            said.push_str(". No server runs on its data: the board's next command starts one");
        }
    }
    if let Some(why) = untold {
        said.push_str(&format!(
            ". The private replay's record stands, and whether its server still runs on the data \
             cannot be told: {why}, so the board's next command is refused while it cannot. {}",
            home.untold_rule()
        ));
    }
    if let Some(doing) = under_way {
        said.push_str(&format!(". Under way: {doing}"));
    }
    let (kept, partial) = home.kept();
    if !kept.is_empty() {
        let listed: Vec<String> = kept
            .iter()
            .map(|path| format!("{} (its log {})", path.display(), crate::size(logged(path))))
            .collect();
        said.push_str(&format!(
            ". Snapshots that hold a log (taken after an unclean end, before a replay, or of a \
             store that went back), each kept until the owner removes it once the board reads as \
             it should: {}",
            listed.join(", ")
        ));
    }
    if !partial.is_empty() {
        let listed: Vec<String> = partial
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        said.push_str(&format!(
            ". Cut short in the middle of its clone or copy, not whole, removed by the next \
             snapshot: {}",
            listed.join(", ")
        ));
    }
    said
}

/// The bytes of the write-ahead log of the store at `data`.
fn logged(data: &Path) -> u64 {
    std::fs::read_dir(data.join("wal")).map_or(0, |segments| {
        segments
            .filter_map(Result::ok)
            .filter_map(|segment| segment.metadata().ok())
            .filter(std::fs::Metadata::is_file)
            .map(|meta| meta.len())
            .sum()
    })
}

/// `text` as one word of a shell's command line: in single quotes, each of its own closed and
/// escaped.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
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
                     Nothing starts another server meanwhile; `just tasks database hold {} --as \
                     {by}` again finishes it, `just tasks database` says what still runs",
                    quoted(why)
                ),
                _ => writeln!(err, "✖ {problem}"),
            };
            2
        }
    }
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
    use super::{quoted, run};
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

    /// A reason goes back into the command it is quoted in as one word, whatever it holds.
    #[test]
    fn a_reason_is_quoted_as_one_shell_word() {
        assert_eq!(quoted("a swap"), "'a swap'");
        assert_eq!(quoted("it's $HOME; `x`"), r"'it'\''s $HOME; `x`'");
        assert_eq!(quoted(""), "''");
    }
}
