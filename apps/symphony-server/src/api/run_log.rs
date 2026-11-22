//! One run's log ([`symphony_api::RUN_LOG_PATH`], docs/workflow/RUNS.md, *Logs*): part of the file the
//! board's recorder writes beside the project's day files, read through
//! [`symphony_board::run_logs::read`], so only a regular file in a day folder of
//! `symphony/runs/logs/` is ever read, never through a link; never the board, so a board slow or
//! down leaves it as it is.

use axum::http::StatusCode;
use symphony_api::{RUN_LOG_TAIL, RunLog};
use symphony_board::run_logs::{Asked, Unread};

/// The run and the part of its log a read asks for, from its query's `run`, `tail`, `from` and
/// `to`: the last [`RUN_LOG_TAIL`] bytes when it names neither; refused, saying why, when it names
/// no run, a number that is none, or a tail beside a range.
pub(crate) fn asked(
    run: Option<&str>,
    tail: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(String, Asked), String> {
    let run = run
        .filter(|run| !run.is_empty())
        .ok_or("run is required: a run's id")?;
    let bytes = |name: &str, value: Option<&str>| {
        value
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| format!("{name} is a number of bytes, not {value:?}"))
            })
            .transpose()
    };
    let asked = match (bytes("tail", tail)?, bytes("from", from)?, bytes("to", to)?) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            return Err("tail, or from and to: one part of a log at a time".into());
        }
        (Some(tail), None, None) => Asked::Tail(tail),
        (None, None, None) => Asked::Tail(RUN_LOG_TAIL),
        (None, from, to) => Asked::Range {
            from: from.unwrap_or(0),
            to,
        },
    };
    Ok((run.to_owned(), asked))
}

/// The part `asked` of the log of `run` in the project `hub` serves, from the day files' folder
/// found at the first read ([`crate::live::Hub::report_folders`]), or the status and why not.
pub(crate) fn answer(
    hub: &crate::live::Hub,
    run: &str,
    asked: Asked,
) -> Result<RunLog, (StatusCode, String)> {
    let folders = hub
        .report_folders
        .get_or_init(|| super::reports::Folders::of(&hub.root));
    let Some(runs) = &folders.runs else {
        return Err((
            StatusCode::NOT_FOUND,
            "this project keeps no runs where this server reads".into(),
        ));
    };
    symphony_board::run_logs::read(runs, run, asked).map_err(|unread| match unread {
        Unread::NotARun => (
            StatusCode::BAD_REQUEST,
            format!("run is a run's id, 32 hex digits, not {run:?}"),
        ),
        Unread::Missing(why) => (StatusCode::NOT_FOUND, why),
        Unread::Failed(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the log of the run {run} could not be read: {error}"),
        ),
    })
}

#[cfg(test)]
mod tests;
