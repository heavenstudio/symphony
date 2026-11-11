//! A night's checks on Symphony's own board: one task per red check, `tests/nightly-red-KEY` (KEY
//! the check's name), filed at p0 and marked red ([`symphony_board::red`]: red work takes no
//! orchestrator slot, and PULL names it first while nobody is on it). A check red again while its
//! task is open gets a line on that task; a green one closes it. A closed task's key is filed
//! again under the next generation (`-2`, `-3`). The run acts as a session of kind filer, the one
//! kind the board takes a red mark from, which it registers for the night and leaves.

use serde_json::{Value, json};

use super::checks::{Ran, tail};
use super::git::short;
use super::record::Board;
use crate::taskcli::CliBoard;

/// The series a night's reds are filed in: [`symphony_board::red::SERIES`]'s nightly one.
pub(crate) const SERIES: &str = "tests/nightly-red-";
/// What a nightly red's mark says is red.
const MARK: &str = "nightly";
/// The document a red task implements.
const LINK: &str = "docs/RELEASES.md";
/// The generations of a key looked at before the board is taken to be wrong.
const GENERATIONS: u32 = 1000;
/// How much of a log a red task quotes: its last lines, within so many bytes.
const TAIL_LINES: usize = 40;
const TAIL_BYTES: u64 = 6000;
/// The commits since the last green run a red task lists at most.
const SINCE_LISTED: usize = 50;

/// The board, as far as a night's reds need it.
pub(crate) trait Reds {
    /// A session of kind filer: its handle.
    fn register(&self) -> Result<String, String>;
    fn leave(&self, handle: &str) -> Result<(), String>;
    /// The status of the task `slug`, none when the board has no such task.
    fn status(&self, slug: &str) -> Result<Option<String>, String>;
    /// A red task `slug`, filed by `by` at p0 and marked red.
    fn create(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String>;
    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String>;
    /// Close `slug` with `why` on its thread.
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String>;
}

impl Reds for CliBoard {
    fn register(&self) -> Result<String, String> {
        let session = self
            .call(
                "register",
                &json!({"kind": symphony_board::red::FILER, "model": "nightly"}),
            )
            .map_err(|e| e.0)?;
        session["handle"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("the board registered no handle: {session}"))
    }

    fn leave(&self, handle: &str) -> Result<(), String> {
        self.call("leave", &json!({"handle": handle}))
            .map(|_| ())
            .map_err(|e| e.0)
    }

    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        match self.call("get", &json!({"slug": slug})) {
            Ok(task) => Ok(task["status"].as_str().map(str::to_string)),
            Err(refused) if refused.0.starts_with("no task ") => Ok(None),
            Err(error) => Err(error.0),
        }
    }

    fn create(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String> {
        let task = json!({
            "slug": slug,
            "title": title,
            "body": body,
            "by": by,
            "priority": 0,
            "red": MARK,
            "links": [LINK],
        });
        self.call("create", &task).map(|_| ()).map_err(|e| e.0)
    }

    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
        self.call("say", &json!({"slug": slug, "sender": by, "text": text}))
            .map(|_| ())
            .map_err(|e| e.0)
    }

    /// The filer takes the reviewer role first: work started, or held by an executor, is moved
    /// only by a session holding a role on it, and a task has one orchestrator.
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
        let _ = self.call(
            "take_role",
            &json!({"slug": slug, "handle": by, "role": "reviewer"}),
        );
        self.say(slug, by, why)?;
        self.call(
            "move",
            &json!({"slug": slug, "status": "cancelled", "handle": by}),
        )
        .map(|_: Value| ())
        .map_err(|e| e.0)
    }
}

/// The night the reds are of.
pub(crate) struct Night<'a> {
    /// The run's id and when it started, as this machine's local minute.
    pub(crate) id: &'a str,
    pub(crate) when: &'a str,
    /// The candidate.
    pub(crate) commit: &'a str,
    /// The commit the last green run checked, when one did, and the commits from it to the
    /// candidate, one `SHORT SUBJECT` line each, newest first.
    pub(crate) green: Option<&'a str>,
    pub(crate) since: &'a [String],
}

/// The key a check's red task is filed under: its name in lower case, every run of anything but a
/// letter or digit one `-`.
pub(crate) fn key(name: &str) -> String {
    let mut key = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c.to_ascii_lowercase());
        } else if !key.is_empty() && !key.ends_with('-') {
            key.push('-');
        }
    }
    let key = key.trim_end_matches('-');
    if key.is_empty() {
        "check".into()
    } else {
        key.into()
    }
}

fn closed(status: &str) -> bool {
    matches!(status, "done" | "cancelled")
}

/// The task `key`'s reds go in now: the first of its generations that is open (true) or not on the
/// board (false); a closed one is passed over.
fn generation(board: &dyn Reds, key: &str) -> Result<(String, bool), String> {
    let base = format!("{SERIES}{key}");
    for n in 1..=GENERATIONS {
        let slug = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        match board.status(&slug)? {
            Some(status) if closed(&status) => {}
            Some(_) => return Ok((slug, true)),
            None => return Ok((slug, false)),
        }
    }
    Err(format!("{base}: {GENERATIONS} generations, all closed"))
}

/// The log's end, fenced.
fn quoted_tail(ran: &Ran) -> String {
    let tail = tail(&ran.log, TAIL_LINES, TAIL_BYTES);
    if tail.is_empty() {
        return "Its log is empty or cannot be read.".into();
    }
    format!("Its last lines:\n\n```\n{tail}\n```")
}

/// What the commits since the last green run are, as a red task says them.
fn since(night: &Night<'_>) -> String {
    let Some(green) = night.green else {
        return "No green run is recorded yet, so no range of commits narrows it.".into();
    };
    if night.since.is_empty() {
        return format!(
            "The last green run checked this same commit ({}): the red is not a change of main's.",
            short(green)
        );
    }
    let mut text = format!(
        "Commits since the last green run, `git log --oneline {}..{}` ({}):\n",
        short(green),
        short(night.commit),
        night.since.len()
    );
    for line in night.since.iter().take(SINCE_LISTED) {
        text.push_str(&format!("- {line}\n"));
    }
    if night.since.len() > SINCE_LISTED {
        text.push_str(&format!(
            "- … and {} more\n",
            night.since.len() - SINCE_LISTED
        ));
    }
    text.trim_end().to_string()
}

/// A red task's body: what is red, how it ended, its log, the commits since the last green run,
/// who fixes it and what closes it.
fn body(ran: &Ran, night: &Night<'_>) -> String {
    format!(
        "The Symphony nightly is red in `{name}` on main at {commit} (run {id}, {when}).\n\n\
         Command: `{command}`, in the nightly's clean checkout of that commit: {ended}.\n\
         Log: {log}\n\n\
         {tail}\n\n\
         {since}\n\n\
         Who fixes: any free executor takes this from the board; it is red work, which takes no \
         orchestrator slot. Reproduce with the command above on {commit}. The next run of `just \
         nightly` closes this task once `{name}` is green; a fix released out of cycle \
         (`just nightly --release COMMIT`) closes it too.\n\n\
         key: {key}\ncheck: {name}",
        name = ran.name,
        commit = short(night.commit),
        id = night.id,
        when = night.when,
        command = ran.command,
        ended = ran.ended(),
        log = ran.log.display(),
        tail = quoted_tail(ran),
        since = since(night),
        key = key(&ran.name),
    )
}

/// The line a red task that is still open gets on a later red night.
fn again(ran: &Ran, night: &Night<'_>) -> String {
    format!(
        "Still red on {} (run {}, {}): {}. Log: {}\n\n{}",
        short(night.commit),
        night.id,
        night.when,
        ran.ended(),
        ran.log.display(),
        quoted_tail(ran)
    )
}

/// File or update the red task of each red check of `checks`, and close the open one of each green
/// check: what it did on the board, with every refusal it met.
pub(crate) fn report(board: &dyn Reds, night: &Night<'_>, checks: &[Ran]) -> Board {
    let mut done = Board::default();
    let handle = match board.register() {
        Ok(handle) => handle,
        Err(error) => {
            done.error = Some(format!("cannot register on the board: {error}"));
            return done;
        }
    };
    let mut errors = Vec::new();
    for ran in checks {
        let key = key(&ran.name);
        let (slug, open) = match generation(board, &key) {
            Ok(found) => found,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let wrote = match (ran.green(), open) {
            (true, false) => continue,
            (true, true) => board
                .close(
                    &slug,
                    &handle,
                    &format!(
                        "Green on {} in run {} ({}): `{}` passed. Closed by the nightly.",
                        short(night.commit),
                        night.id,
                        night.when,
                        ran.name
                    ),
                )
                .map(|()| done.closed.push(slug.clone())),
            (false, true) => (board.say(&slug, &handle, &again(ran, night)))
                .map(|()| done.updated.push(slug.clone())),
            (false, false) => {
                let title = format!("Nightly red: {} on {}", ran.name, short(night.commit));
                match board.create(&slug, &title, &body(ran, night), &handle) {
                    Ok(()) => {
                        done.filed.push(slug.clone());
                        Ok(())
                    }
                    // Another run filed it first: update it, never file a second.
                    Err(error) => match board.status(&slug) {
                        Ok(Some(status)) if !closed(&status) => {
                            (board.say(&slug, &handle, &again(ran, night)))
                                .map(|()| done.updated.push(slug.clone()))
                        }
                        _ => Err(error),
                    },
                }
            }
        };
        if let Err(error) = wrote {
            errors.push(format!("{slug}: {error}"));
        }
    }
    if let Err(error) = board.leave(&handle) {
        errors.push(format!("cannot leave the board session {handle}: {error}"));
    }
    if !errors.is_empty() {
        done.error = Some(errors.join("; "));
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A board of tasks by slug, each its status and thread, refusing what `refuse` names.
    #[derive(Default)]
    struct Fake {
        tasks: RefCell<BTreeMap<String, (String, Vec<String>)>>,
        sessions: RefCell<Vec<(String, bool)>>,
        refuse: Vec<&'static str>,
        /// A task another run files between this one's look and its create.
        raced: Option<&'static str>,
    }

    impl Fake {
        fn with(tasks: &[(&str, &str)]) -> Fake {
            let fake = Fake::default();
            for (slug, status) in tasks {
                fake.tasks
                    .borrow_mut()
                    .insert((*slug).into(), ((*status).into(), Vec::new()));
            }
            fake
        }

        fn refused(&self, what: &str) -> Result<(), String> {
            if self.refuse.contains(&what) {
                return Err(format!("{what} refused"));
            }
            Ok(())
        }

        fn status_of(&self, slug: &str) -> String {
            self.tasks.borrow()[slug].0.clone()
        }

        fn thread(&self, slug: &str) -> Vec<String> {
            self.tasks.borrow()[slug].1.clone()
        }
    }

    impl Reds for Fake {
        fn register(&self) -> Result<String, String> {
            self.refused("register")?;
            self.sessions.borrow_mut().push(("filer-1".into(), true));
            Ok("filer-1".into())
        }

        fn leave(&self, handle: &str) -> Result<(), String> {
            self.refused("leave")?;
            for session in self.sessions.borrow_mut().iter_mut() {
                if session.0 == handle {
                    session.1 = false;
                }
            }
            Ok(())
        }

        fn status(&self, slug: &str) -> Result<Option<String>, String> {
            self.refused("status")?;
            Ok(self
                .tasks
                .borrow()
                .get(slug)
                .map(|(status, _)| status.clone()))
        }

        fn create(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String> {
            assert_eq!(by, "filer-1");
            if self.raced == Some(slug) {
                self.tasks
                    .borrow_mut()
                    .insert(slug.into(), ("ready".into(), Vec::new()));
                return Err(format!("task {slug} exists"));
            }
            self.refused("create")?;
            self.tasks.borrow_mut().insert(
                slug.into(),
                ("ready".into(), vec![format!("{title}\n{body}")]),
            );
            Ok(())
        }

        fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
            assert_eq!(by, "filer-1");
            self.refused("say")?;
            let mut tasks = self.tasks.borrow_mut();
            tasks.get_mut(slug).unwrap().1.push(text.into());
            Ok(())
        }

        fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
            self.say(slug, by, why)?;
            self.tasks.borrow_mut().get_mut(slug).unwrap().0 = "cancelled".into();
            Ok(())
        }
    }

    fn ran(name: &str, code: i32) -> Ran {
        Ran {
            name: name.into(),
            command: "just verify".into(),
            exit_code: Some(code),
            signal: None,
            seconds: 1.0,
            log: PathBuf::from("/no/log"),
            error: None,
        }
    }

    fn night<'a>(since: &'a [String]) -> Night<'a> {
        Night {
            id: "2026-11-12T0300-1",
            when: "2026-11-12 03:00",
            commit: "cafe0123456789abcdef",
            green: Some("beef0123456789abcdef"),
            since,
        }
    }

    #[test]
    fn a_checks_key_is_its_name_in_lower_case_words() {
        assert_eq!(key("verify"), "verify");
        assert_eq!(key("Conciliate star_foundry!"), "conciliate-star-foundry");
        assert_eq!(key("--a--b--"), "a-b");
        assert_eq!(
            key("tests/perf"),
            "tests-perf",
            "no slash: a name under the series is not a red"
        );
        assert_eq!(key("!!!"), "check");
        assert!(symphony_board::red::filed(&format!(
            "{SERIES}{}",
            key("a/b c")
        )));
        assert_eq!(SERIES, symphony_board::red::SERIES[1]);
    }

    #[test]
    fn a_red_check_is_filed_once_and_a_second_red_night_updates_it() {
        let board = Fake::default();
        let checks = [ran("verify", 101), ran("lint", 0)];
        let done = report(&board, &night(&[]), &checks);
        assert_eq!(done.filed, ["tests/nightly-red-verify"]);
        assert!(done.updated.is_empty() && done.closed.is_empty() && done.error.is_none());
        assert_eq!(board.tasks.borrow().len(), 1, "a green check files nothing");
        let filed = board.thread("tests/nightly-red-verify").join("\n");
        assert!(
            filed.starts_with("Nightly red: verify on cafe01234567\n"),
            "{filed}"
        );
        assert!(
            filed.contains(
                "`just verify`, in the nightly's clean checkout of that commit: exit 101."
            ),
            "{filed}"
        );
        assert!(filed.contains("Log: /no/log"), "{filed}");
        assert!(filed.ends_with("key: verify\ncheck: verify"), "{filed}");
        assert_eq!(
            board.sessions.borrow().as_slice(),
            [("filer-1".to_string(), false)],
            "left"
        );

        let done = report(&board, &night(&[]), &checks);
        assert_eq!(done.updated, ["tests/nightly-red-verify"]);
        assert!(done.filed.is_empty());
        assert_eq!(board.tasks.borrow().len(), 1);
        let thread = board.thread("tests/nightly-red-verify");
        assert_eq!(thread.len(), 2);
        assert!(
            thread[1].starts_with(
                "Still red on cafe01234567 (run 2026-11-12T0300-1, 2026-11-12 03:00): exit 101."
            ),
            "{thread:?}"
        );
    }

    #[test]
    fn a_green_check_closes_its_open_red_and_a_later_red_files_the_next_generation() {
        let board = Fake::with(&[("tests/nightly-red-verify", "in_progress")]);
        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-verify"]);
        assert_eq!(board.status_of("tests/nightly-red-verify"), "cancelled");
        assert!(
            board.thread("tests/nightly-red-verify")[0].starts_with("Green on cafe01234567 in run")
        );

        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert!(done.closed.is_empty(), "nothing open to close: {done:?}");

        let done = report(&board, &night(&[]), &[ran("verify", 2)]);
        assert_eq!(done.filed, ["tests/nightly-red-verify-2"]);
        assert_eq!(
            board.status_of("tests/nightly-red-verify"),
            "cancelled",
            "a closed red stays closed"
        );
    }

    #[test]
    fn the_open_generation_is_the_one_a_night_writes_to() {
        let board = Fake::with(&[
            ("tests/nightly-red-verify", "cancelled"),
            ("tests/nightly-red-verify-2", "done"),
            ("tests/nightly-red-verify-3", "review"),
        ]);
        let done = report(&board, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.updated, ["tests/nightly-red-verify-3"]);
        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-verify-3"]);
    }

    #[test]
    fn a_red_another_run_filed_first_is_updated_never_filed_twice() {
        let board = Fake {
            raced: Some("tests/nightly-red-verify"),
            ..Fake::default()
        };
        let done = report(&board, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.updated, ["tests/nightly-red-verify"]);
        assert!(done.filed.is_empty() && done.error.is_none(), "{done:?}");
    }

    #[test]
    fn what_the_board_refuses_is_said_and_the_other_checks_still_go() {
        let board = Fake {
            refuse: vec!["create"],
            ..Fake::with(&[("tests/nightly-red-lint", "ready")])
        };
        let done = report(&board, &night(&[]), &[ran("verify", 1), ran("lint", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-lint"]);
        assert_eq!(
            done.error.as_deref(),
            Some("tests/nightly-red-verify: create refused")
        );
        assert!(
            !board.sessions.borrow()[0].1,
            "the session is left all the same"
        );

        let unregistered = Fake {
            refuse: vec!["register"],
            ..Fake::default()
        };
        let done = report(&unregistered, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(
            done.error.as_deref(),
            Some("cannot register on the board: register refused")
        );
        assert!(unregistered.tasks.borrow().is_empty());
    }

    #[test]
    fn a_board_that_cannot_say_a_status_or_end_the_session_is_said_and_writes_nothing() {
        let blind = Fake {
            refuse: vec!["status", "leave"],
            ..Fake::default()
        };
        let done = report(&blind, &night(&[]), &[ran("verify", 1), ran("lint", 0)]);
        assert!(done.filed.is_empty() && done.updated.is_empty() && done.closed.is_empty());
        assert_eq!(
            done.error.as_deref(),
            Some(
                "status refused; status refused; \
                 cannot leave the board session filer-1: leave refused"
            )
        );
        assert!(blind.tasks.borrow().is_empty());
    }

    #[test]
    fn a_key_whose_every_generation_is_closed_is_said_never_filed_past_them() {
        let spent: Vec<String> = (1..=GENERATIONS)
            .map(|n| match n {
                1 => format!("{SERIES}verify"),
                n => format!("{SERIES}verify-{n}"),
            })
            .collect();
        let tasks: Vec<(&str, &str)> = spent.iter().map(|s| (s.as_str(), "done")).collect();
        let board = Fake::with(&tasks);
        let done = report(&board, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(
            done.error.as_deref(),
            Some("tests/nightly-red-verify: 1000 generations, all closed")
        );
        assert!(done.filed.is_empty());
        assert_eq!(board.tasks.borrow().len(), spent.len());
    }

    #[test]
    fn a_red_task_names_the_commits_since_the_last_green_run() {
        let listed: Vec<String> = (0..SINCE_LISTED + 2)
            .map(|n| format!("c{n} change {n}"))
            .collect();
        let board = Fake::default();
        report(&board, &night(&listed), &[ran("verify", 1)]);
        let body = board.thread("tests/nightly-red-verify").join("\n");
        assert!(
            body.contains(&format!(
                "Commits since the last green run, `git log --oneline beef01234567..cafe01234567` ({}):\n- c0 change 0\n",
                SINCE_LISTED + 2
            )),
            "{body}"
        );
        assert!(body.contains("- … and 2 more\n"), "{body}");
        let mut first = night(&[]);
        first.green = None;
        assert_eq!(
            since(&first),
            "No green run is recorded yet, so no range of commits narrows it."
        );
        assert!(since(&night(&[])).contains("this same commit (beef01234567)"));
    }

    #[test]
    fn a_red_task_quotes_the_end_of_its_log() {
        let tree = crate::test_tree::Tree::new();
        let lines: String = (0..60).map(|n| format!("line {n}\n")).collect();
        let log = tree.write("verify.log", &lines);
        let red = Ran {
            log,
            ..ran("verify", 1)
        };
        let quoted = quoted_tail(&red);
        assert!(
            quoted.starts_with("Its last lines:\n\n```\nline 20\n"),
            "{quoted}"
        );
        assert!(quoted.ends_with("line 59\n```"), "{quoted}");
        assert_eq!(
            quoted_tail(&ran("verify", 1)),
            "Its log is empty or cannot be read."
        );
    }
}
