//! `tasks schema [--accept VERSION]`: the board's schema, and the highest its live board was seen at
//! ([`crate::live::seen`]); exit 1, with the rollback's words, when the board reads below it. With
//! `--accept`, the owner's acceptance of a live board that is older on purpose (a fresh store, a
//! restore they chose), refused unless the board holds exactly the version named: from then on the
//! record counts from it, and the board is read and moved forward again as any board behind its
//! build is.

use super::{Clock, Env, args::Parsed, migrate, reach};
use crate::live::seen::{self, Accepted};
use std::io::Write;

pub(super) fn run(parsed: &Parsed, env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match said(parsed, env) {
        Ok((code, text)) => {
            let _ = writeln!(out, "{text}");
            code
        }
        Err(why) => {
            let _ = writeln!(err, "✖ {why}");
            2
        }
    }
}

/// The version `--accept` names: a schema version, or `none` for a store with no board (0).
fn accepted(word: &str) -> Result<i64, String> {
    if word == "none" {
        return Ok(0);
    }
    word.parse::<i64>()
        .ok()
        .filter(|version| *version > 0)
        .ok_or_else(|| {
            format!(
                "--accept takes the schema version the board holds, or none for a store with no \
                 board, not {word:?}"
            )
        })
}

/// What a board found at `found` holds, as the command says it.
fn holds(found: Option<i64>) -> String {
    found.map_or_else(
        || "holds no board".into(),
        |found| format!("is at schema {found}"),
    )
}

/// What the command says, and its exit code.
fn said(parsed: &Parsed, env: &Env) -> Result<(i32, String), String> {
    let accept = parsed.get("accept").map(accepted).transpose()?;
    let reached = reach(env, &env.cwd, true)?;
    let found = migrate::version(&reached.client)?;
    let Some(live) = &reached.live else {
        let board = format!(
            "the board in database {} at {}",
            reached.client.database(),
            reached.client.addr()
        );
        if accept.is_some() {
            return Err(format!(
                "{board} is a scratch board, which keeps no record of the schema it was seen at: \
                 nothing was accepted"
            ));
        }
        return Ok((
            0,
            format!(
                "{board} {}; a scratch board keeps no record of the schema it was seen at",
                holds(found)
            ),
        ));
    };
    let record = seen::path(&live.common);
    if let Some(version) = accept {
        let now = Clock::of(env)?.now();
        let at = if version == 0 {
            "with no board".to_string()
        } else {
            format!("at schema {version}")
        };
        return Ok(match seen::accept(live, version, found, now)? {
            Accepted::Accepted { from } => (
                0,
                format!(
                    "✔ accepted the {} board {at}, below the schema {from} it was seen at: its \
                     record ({}) counts from there now, and the board is read, and moved forward, \
                     again",
                    live.project,
                    record.display()
                ),
            ),
            Accepted::Nothing => (
                0,
                format!(
                    "✔ the {} board {} and reads below no schema it was seen at: nothing to accept",
                    live.project,
                    holds(found)
                ),
            ),
        });
    }
    if let Some(back) = seen::went_back(live, found) {
        return Ok((1, format!("⚠ {}", back.refusal())));
    }
    Ok((
        0,
        match seen::read(&live.common) {
            Some(seen) => format!(
                "the {} board {}; the highest schema it was seen at is {}, on {} ({})",
                live.project,
                holds(found),
                seen.version,
                crate::sys::local_minute(seen.at),
                record.display()
            ),
            None => format!(
                "the {} board {}; no schema it was seen at is recorded yet ({})",
                live.project,
                holds(found),
                record.display()
            ),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::accepted;

    #[test]
    fn accept_takes_a_schema_version_or_none() {
        assert_eq!(accepted("13"), Ok(13));
        assert_eq!(accepted("none"), Ok(0));
        for bad in ["0", "-1", "x", "", "13.0", "None"] {
            assert!(accepted(bad).is_err(), "{bad}");
        }
    }
}
