//! `runs SLUG [--tip SHA]`: what the recorder kept of a task at a tip, which its reviewer reads in
//! place of building again (RULES.md `workflow/no-second-build-in-review`): each build and test
//! command the task ran there with its newest result and the runs before it there, the `tests`
//! stamp as its reader reads it, the project's `[verdicts] check` (the task page's check gate) with
//! its newest run at the tip or none, and the task's runs at other commits, which are not the tip's.
//! Each red run there that kept a log, the newest of its command or one before it, is named after
//! the rows with the log's path, where the command's own output says what failed, whatever its
//! runner.
//! A command its runner refused there before building or running anything is named apart, with the
//! refusal's line: no run of the command, so no red and no green (`stamps::read::refused`).
//! The tip is the task's branch's in the main checkout, else the commit `--tip` names there.
//!
//! It reads what the land and the task page read: the recorder's runs (`recorded::runs`), the task
//! and the work under it (`stamps::family`), through the `tests` stamp's own reader
//! (`stamps::read`), and the check from the main checkout's committed settings
//! (`land::Settings`). It stores nothing.

use super::stamps::main_of;
use super::{Context, Stop};
use crate::Store;
use crate::cli::machine_work::recorded::{self, Record, Step};
use crate::cli::text::columns;
use crate::land::Settings;
use crate::on_review::branch_of;
use crate::stamps::{family, read, short};
use crate::sweep::git::rev;
use crate::sys::local_minute;

/// `runs SLUG [--tip SHA]`: the task's runs at its tip ([`lines`]). Refused for a task the board
/// does not hold, outside a checkout of its repository, and with no tip to read: no branch and no
/// `--tip`, or a `--tip` naming no commit.
pub(super) fn runs(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let slug = c.arg("slug");
    store.get(&slug)?;
    let main = main_of(c).ok_or_else(|| {
        Stop::Refused(
            "tasks runs runs in a checkout of the task's repository: the tip it reads runs at is \
             a commit there"
                .into(),
        )
    })?;
    let branch = branch_of(&slug).to_string();
    let named = c.parsed.get("tip").map(str::trim).filter(|t| !t.is_empty());
    let (tip, from) = match named {
        Some(named) => {
            let tip = rev(&main, &format!("{named}^{{commit}}")).ok_or_else(|| {
                Stop::Refused(format!(
                    "--tip {named} names no commit in {}",
                    main.display()
                ))
            })?;
            (tip, Tip::Named)
        }
        None => {
            let tip = rev(&main, &format!("refs/heads/{branch}")).ok_or_else(|| {
                Stop::Refused(format!(
                    "no branch {branch} in {}: name the commit to read with --tip",
                    main.display()
                ))
            })?;
            (tip, Tip::Branch(branch))
        }
    };
    let runs = recorded::folder(c.env)
        .map(|folder| recorded::runs(&folder))
        .unwrap_or_default();
    let check = Settings::of(&main).map(|settings| {
        let check = if settings.check.is_empty() {
            settings.main_check
        } else {
            settings.check
        };
        check.join(" ")
    });
    let shown = Shown {
        slug: &slug,
        tip: &tip,
        from: &from,
        runs: &runs,
        ours: &family(store, &slug),
        check: check.as_deref().map_err(String::as_str),
    };
    for line in lines(&shown) {
        c.say(&line);
    }
    Ok(())
}

/// Where the tip `runs` reads at came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Tip {
    /// The task's branch, by its name.
    Branch(String),
    /// `--tip`.
    Named,
}

/// What `runs` prints from.
pub(super) struct Shown<'a> {
    pub slug: &'a str,
    /// The whole commit.
    pub tip: &'a str,
    pub from: &'a Tip,
    /// The recorder's runs, every task's.
    pub runs: &'a [Record],
    /// The task and the work under it, whose runs are its own.
    pub ours: &'a [String],
    /// The project's `[verdicts] check` as one command line, empty when it names none; why the
    /// settings cannot be read.
    pub check: Result<&'a str, &'a str>,
}

/// The lines `runs` prints: `TIP`, then `RUNS`, each command the task ran at the tip a row (its
/// newest run's state, phase, command, when, how long, who, and why it is red, whose it is when work
/// under the task ran it, how many runs of it came before there), then a `LOG` line for each red
/// run there that kept a log, oldest first within its command, `TESTS`, the `tests` stamp as its
/// reader reads it (after `REFUSED`, the commands a runner refused at the tip, each with the
/// refusal's line, when there are any), `CHECK`, the `[verdicts] check` at the tip as its gate reads it (none when the
/// project names none), and `ELSEWHERE`, the task's runs at other commits (none when there are
/// none).
pub(super) fn lines(shown: &Shown<'_>) -> Vec<String> {
    let (tip, at) = (shown.tip, short(shown.tip));
    let ours = |task: &str| shown.ours.iter().any(|ours| ours == task);
    let mut lines = vec![match shown.from {
        Tip::Branch(branch) => format!("TIP {branch} {tip}"),
        Tip::Named => format!("TIP {tip} (--tip)"),
    }];
    let found = read::at_tip(shown.runs, tip, ours);
    if found.commands.is_empty() {
        lines.push(format!(
            "RUNS none at {at}: no build or test of the task is recorded there"
        ));
    } else {
        lines.push(format!("RUNS at {at}, each command's newest run there:"));
        let rows: Vec<Vec<String>> = (found.commands.iter())
            .map(|command| row(shown.slug, command))
            .collect();
        lines.extend(columns(&rows).into_iter().map(|row| format!("  {row}")));
        let each = (found.commands.iter())
            .flat_map(|command| command.before.iter().copied().chain([command.newest]));
        lines.extend(each.filter_map(|run| {
            let red = run.state == Step::Finished && read::red(run).is_some();
            let log = log_of(run).filter(|_| red)?;
            Some(format!(
                "  LOG of the red `{}` at {}, what failed is at its end: {log}",
                run.label,
                local_minute(when(run))
            ))
        }));
    }
    if !found.refused.is_empty() {
        lines.push(format!(
            "REFUSED at {at}, each by its runner before it built or ran anything: no red and no \
             green there"
        ));
        let rows: Vec<Vec<String>> = (found.refused.iter())
            .map(|run| refused_row(shown.slug, run))
            .collect();
        lines.extend(columns(&rows).into_iter().map(|row| format!("  {row}")));
    }
    let tests = read::tests(shown.runs, tip, ours);
    let met = if tests.met { "met" } else { "not met" };
    lines.push(format!("TESTS {met}: {}", tests.detail));
    match shown.check {
        Ok("") => {}
        Ok(label) => lines.push(format!(
            "CHECK `{label}`, the project's [verdicts] check: {}",
            checked(read::newest_of(shown.runs, tip, label), at)
        )),
        Err(why) => lines.push(format!("CHECK cannot tell: {why}")),
    }
    if let Some(last) = found.elsewhere.last() {
        let n = found.elsewhere.len();
        let (runs, are) = if n == 1 {
            ("run", "is")
        } else {
            ("runs", "are")
        };
        lines.push(format!(
            "ELSEWHERE {n} {runs} of the task at other commits, the newest at {}, {}: none {are} \
             this tip's",
            short(last.commit.as_deref().unwrap_or_default()),
            local_minute(when(last))
        ));
    }
    lines
}

/// Where `run`'s log is: the file its recorder kept its command's output in, by its whole path;
/// none for a run recorded without one.
fn log_of(run: &Record) -> Option<String> {
    let kept = run.run.as_ref()?;
    let (runs, log) = (kept.runs.as_deref()?, kept.log.as_deref()?);
    Some(std::path::Path::new(runs).join(log).display().to_string())
}

/// When `run` last moved: ended, else began.
fn when(run: &Record) -> f64 {
    run.ended_at.unwrap_or_else(|| read::began(run))
}

/// `run`'s state as a word: `green`, `red`, `running` or `queued`.
fn state(run: &Record) -> &'static str {
    match run.state {
        Step::Finished if read::red(run).is_some() => "red",
        Step::Finished => "green",
        Step::Running => "running",
        Step::Queued => "queued",
    }
}

/// `run`'s phase as a word.
fn phase(run: &Record) -> &'static str {
    match run.phase {
        recorded::Phase::Build => "build",
        recorded::Phase::Test => "test",
    }
}

/// The row of `command`, run for the task `slug` or work under it.
fn row(slug: &str, command: &read::Command<'_>) -> Vec<String> {
    let run = command.newest;
    let took = match run.duration() {
        Some(seconds) => format!("{seconds:.0} s"),
        None => "-".into(),
    };
    let mut notes = Vec::new();
    notes.extend(read::red(run).filter(|_| run.state == Step::Finished));
    if let Some(task) = run.task.as_deref().filter(|task| *task != slug) {
        notes.push(format!("for {task}"));
    }
    if !command.before.is_empty() {
        let reds = (command.before.iter())
            .filter(|run| run.state == Step::Finished && read::red(run).is_some())
            .count();
        notes.push(format!("{} earlier here, {reds} red", command.before.len()));
    }
    vec![
        state(run).into(),
        phase(run).into(),
        run.label.clone(),
        local_minute(when(run)),
        took,
        run.handle.clone().unwrap_or_else(|| "-".into()),
        notes.join("; "),
    ]
}

/// The row of `run`, which its runner refused: its phase, command, when, who and the refusal's
/// line, and whose it is when work under the task `slug` ran it.
fn refused_row(slug: &str, run: &Record) -> Vec<String> {
    let mut notes = vec![read::refused(run).unwrap_or_default().to_string()];
    if let Some(task) = run.task.as_deref().filter(|task| *task != slug) {
        notes.push(format!("for {task}"));
    }
    vec![
        "refused".into(),
        phase(run).into(),
        run.label.clone(),
        local_minute(when(run)),
        run.handle.clone().unwrap_or_else(|| "-".into()),
        notes.join("; "),
    ]
}

/// What the check's newest run at the tip `at` says, as its gate reads it: `run`'s result, or none.
fn checked(run: Option<&Record>, at: &str) -> String {
    let Some(run) = run else {
        return format!("no run at {at}");
    };
    let by = run.handle.as_deref().unwrap_or("no session");
    let since = local_minute(when(run));
    match (run.state, read::red(run)) {
        (Step::Finished, None) => format!("green at {at}, {since}, by {by}"),
        (Step::Finished, Some(why)) => format!("red at {at}: {why}, {since}, by {by}"),
        (Step::Running, _) => format!("running at {at} since {since}, by {by}"),
        (Step::Queued, _) => format!("queued at {at} since {since}, by {by}"),
    }
}

#[cfg(test)]
mod tests;
