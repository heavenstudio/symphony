//! A night's checks on Symphony's own board: one task per red check, `tests/nightly-red-KEY` (KEY
//! the check's name), filed at p0 and marked red ([`symphony_board::red`]: red work takes no
//! orchestrator slot, and PULL names it first while nobody is on it). A check red again while its
//! task is open gets a line on that task; a green one closes it, when the candidate holds the
//! commit it was red on (an out-of-cycle release of an older commit leaves it open). A run that
//! stopped short of deciding (it could not fetch, check out, finish in time or push) is the red
//! [`RUN`], which the next run that gets through closes. A closed task's key is filed again under
//! the next generation (`-2`, `-3`). The run acts as a session of kind filer, the one kind the
//! board takes a red mark from, which it registers for the night and leaves; a night with nothing
//! to file, update or close writes nothing.

use serde_json::{Value, json};

use super::checks::{Ran, tail};
use super::git::short;
use super::record::Board;
use crate::taskcli::CliBoard;

/// The series a night's reds are filed in: [`symphony_board::red::SERIES`]'s nightly one.
pub(crate) const SERIES: &str = "tests/nightly-red-";
/// The key of a run that stopped short of deciding, which no check takes.
pub(crate) const RUN: &str = "run";
/// The key of a conciliation that could not read the machine's registry, which no check takes.
pub(crate) const CONCILIATE: &str = "conciliate";
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
    /// The candidate; empty when the run stopped before it had one.
    pub(crate) commit: &'a str,
    /// The commit the last green run checked, when one did, and the commits from it to the
    /// candidate, one `SHORT SUBJECT` line each, newest first.
    pub(crate) green: Option<&'a str>,
    pub(crate) since: &'a [String],
    /// Why the run stopped short of deciding, when it did: the red [`RUN`].
    pub(crate) stopped: Option<&'a str>,
    /// Whether what stopped it is the owner's to clear: a process in the run's folders, which may
    /// be anyone's, and which no executor ends.
    pub(crate) for_owner: bool,
    /// The keys this run found green beyond its checks: [`CONCILIATE`] once the registry read.
    pub(crate) cleared: &'a [&'a str],
    /// Whether a green this night closes the open red of a key: the candidate holds what it was
    /// red on.
    pub(crate) closes: &'a dyn Fn(&str) -> bool,
    /// Whether the run checked a candidate (a night or a release), so its getting through closes the
    /// red [`RUN`]; a `--refresh` checks none, and answers for its refresh steps alone.
    pub(crate) whole: bool,
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

/// Whether the step `name` is a release's refresh of a project ([`super::refresh::STEP`]), not a
/// check of the candidate.
pub(crate) fn is_refresh(name: &str) -> bool {
    let key = key(name);
    key == super::refresh::STEP || key.starts_with(&format!("{}-", super::refresh::STEP))
}

/// Why a refresh step is red: what it said, or how it ended.
fn refresh_why(ran: &Ran) -> String {
    ran.error.clone().unwrap_or_else(|| ran.ended())
}

/// A red refresh step's body: what it was refreshing and why it is red, its log, what it left as
/// it was, who fixes it and what closes it.
fn refresh_body(ran: &Ran, night: &Night<'_>) -> String {
    format!(
        "The Symphony nightly's {command} is red (run {id}, {when}, the release of {commit}): \
         {why}\n\n\
         Log: {log}\n\n\
         {tail}\n\n\
         What it left as it was: what the step had not reached. A red commit gate makes no \
         branch; a main checkout off its default branch, with tracked edits, or whose hooks \
         refuse the fast-forward keeps its main where it was, the pin's branch left for it; a red \
         board move moves no schema; the shared server keeps serving what it served. Nothing of \
         the release is undone: it is tagged and pushed, and other projects refreshed on their \
         own.\n\n\
         Who fixes: what the line above names. A red in the project's own gate, hooks or code is \
         that project's, a task on its own board; a main checkout to put right is its owner's; \
         one of the Symphony's is fixed here. Then an orchestrator reruns the refresh, `just \
         nightly --refresh` in the Symphony's main checkout, which finishes what this step did \
         not; a green refresh of it closes this task.\n\n\
         key: {key}\ncheck: {name}",
        command = ran.command,
        id = night.id,
        when = night.when,
        commit = short(night.commit),
        why = refresh_why(ran),
        log = ran.log.display(),
        tail = quoted_tail(ran),
        key = key(&ran.name),
        name = ran.name,
    )
}

/// The line an open red refresh step gets on a later red refresh.
fn refresh_again(ran: &Ran, night: &Night<'_>) -> String {
    format!(
        "Still red, refreshing to the release of {} (run {}, {}): {}. Log: {}\n\n{}",
        short(night.commit),
        night.id,
        night.when,
        refresh_why(ran),
        ran.log.display(),
        quoted_tail(ran)
    )
}

/// Where a run stopped, as a red task says it: on its candidate, or before it had one.
fn on(night: &Night<'_>) -> String {
    if night.commit.is_empty() {
        "before it had a candidate".into()
    } else {
        format!("on {}", short(night.commit))
    }
}

/// Who clears a stop for a process in the run's folders: the process is named in why the run
/// stopped, with the way out.
const OWNERS: &str = "Who fixes: the owner, or an orchestrator on the owner's word; not an \
executor. A process stopped the run by being in the nightly's checkout or its copies' folder, and \
it may be anyone's: a shell, a terminal tab, an editor or a Finder window there counts. An \
executor ends only a process it started (RULES `delete-by-name`). End it, or move it out of that \
folder, and the next run goes.";
/// Who fixes any other stop.
const EXECUTORS: &str = "Who fixes: any free executor takes this from the board; it is red work, \
which takes no orchestrator slot.";

/// The red [`RUN`]'s body: why the run stopped, who fixes it and what closes it.
fn stopped_body(why: &str, night: &Night<'_>) -> String {
    format!(
        "The Symphony nightly stopped {on} (run {id}, {when}) short of a release: {why}\n\n\
         Until a run gets through, main goes unverified and unreleased. The run's record is \
         `symphony/nightly/runs/{id}.json` in the git common dir, and `just nightly --status` \
         says the last night.\n\n\
         {who} The next run that gets through closes this task.\n\n\
         key: {RUN}\ncheck: {RUN}",
        on = on(night),
        id = night.id,
        when = night.when,
        who = if night.for_owner { OWNERS } else { EXECUTORS },
    )
}

/// What a night does to one key's task.
enum Act {
    /// File the red `slug`, titled, with its body; a later red line in case another run filed it.
    File {
        slug: String,
        title: String,
        body: String,
        again: String,
    },
    /// Add a line to the open red `slug`.
    Say { slug: String, text: String },
    /// Close the open red `slug`, saying why.
    Close { slug: String, why: String },
}

/// The act on the key `key`'s task: red with its title, body and later line, else green with
/// the name of what passed; none when there is nothing to write.
fn act(
    board: &dyn Reds,
    night: &Night<'_>,
    key: &str,
    red: Option<(String, String, String)>,
    passed: &str,
) -> Result<Option<Act>, String> {
    let (slug, open) = generation(board, key)?;
    Ok(match (red, open) {
        (None, false) => None,
        (None, true) if !(night.closes)(key) => None,
        (None, true) => Some(Act::Close {
            slug,
            why: format!(
                "Green {} in run {} ({}): {passed}. Closed by the nightly.",
                on(night),
                night.id,
                night.when,
            ),
        }),
        (Some((_, _, again)), true) => Some(Act::Say { slug, text: again }),
        (Some((title, body, again)), false) => Some(Act::File {
            slug,
            title,
            body,
            again,
        }),
    })
}

/// File or update the red task of each red check of `checks` and of the run when it stopped, and
/// close the open one of each green check, of the run when it got through and of each key the
/// night cleared: what it did on the board, with every refusal it met. A night with nothing to
/// write registers no session.
pub(crate) fn report(board: &dyn Reds, night: &Night<'_>, checks: &[Ran]) -> Board {
    let mut done = Board::default();
    let mut errors = Vec::new();
    let mut acts = Vec::new();
    let mut plan = |key: &str, red, passed: &str| match act(board, night, key, red, passed) {
        Ok(found) => acts.extend(found),
        Err(error) => errors.push(error),
    };
    for ran in checks {
        let refresh = is_refresh(&ran.name);
        let red = (!ran.green()).then(|| {
            if refresh {
                (
                    format!("Nightly red: {}", ran.command),
                    refresh_body(ran, night),
                    refresh_again(ran, night),
                )
            } else {
                (
                    format!("Nightly red: {} on {}", ran.name, short(night.commit)),
                    body(ran, night),
                    again(ran, night),
                )
            }
        });
        plan(&key(&ran.name), red, &format!("`{}` passed", ran.name));
    }
    if night.whole {
        let stopped = night.stopped.map(|why| {
            (
                format!("Nightly red: the run stopped {}", on(night)),
                stopped_body(why, night),
                format!("Stopped again (run {}, {}): {why}", night.id, night.when),
            )
        });
        plan(RUN, stopped, "the run got through");
    }
    for cleared in night.cleared {
        plan(cleared, None, &format!("`{cleared}` passed"));
    }
    if acts.is_empty() {
        if !errors.is_empty() {
            done.error = Some(errors.join("; "));
        }
        return done;
    }
    let handle = match board.register() {
        Ok(handle) => handle,
        Err(error) => {
            errors.insert(0, format!("cannot register on the board: {error}"));
            done.error = Some(errors.join("; "));
            return done;
        }
    };
    for act in acts {
        let wrote = match &act {
            Act::Close { slug, why } => board
                .close(slug, &handle, why)
                .map(|()| done.closed.push(slug.clone())),
            Act::Say { slug, text } => board
                .say(slug, &handle, text)
                .map(|()| done.updated.push(slug.clone())),
            Act::File {
                slug,
                title,
                body,
                again,
            } => match board.create(slug, title, body, &handle) {
                Ok(()) => {
                    done.filed.push(slug.clone());
                    Ok(())
                }
                // Another run filed it first: update it, never file a second.
                Err(error) => match board.status(slug) {
                    Ok(Some(status)) if !closed(&status) => board
                        .say(slug, &handle, again)
                        .map(|()| done.updated.push(slug.clone())),
                    _ => Err(error),
                },
            },
        };
        if let Err(error) = wrote {
            let (Act::Close { slug, .. } | Act::Say { slug, .. } | Act::File { slug, .. }) = &act;
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
            timed_out: false,
            interrupted: false,
        }
    }

    fn night<'a>(since: &'a [String]) -> Night<'a> {
        Night {
            id: "2026-11-12T0300-1",
            when: "2026-11-12 03:00",
            commit: "cafe0123456789abcdef",
            green: Some("beef0123456789abcdef"),
            since,
            stopped: None,
            for_owner: false,
            cleared: &[],
            closes: &|_| true,
            whole: true,
        }
    }

    #[test]
    fn a_stop_for_a_process_in_the_runs_folders_is_the_owners_and_any_other_an_executors() {
        let board = Fake::default();
        let mut stopped = night(&[]);
        stopped.stopped = Some(
            "/wt/nightly is in use by process 10 (zsh), working there: nothing is checked while \
             a process is in the run's folders; end it, or move it out of that folder (a shell, \
             a terminal tab, an editor or a Finder window there counts), and the next run goes",
        );
        stopped.for_owner = true;
        report(&board, &stopped, &[]);
        let filed = board.thread("tests/nightly-red-run").join("\n");
        assert!(
            filed.contains(
                "process 10 (zsh), working there: nothing is checked while a process is in the \
                 run's folders; end it, or move it out of that folder (a shell, a terminal tab, an \
                 editor or a Finder window there counts), and the next run goes\n\n\
                 Until a run gets through, main goes unverified and unreleased. "
            ),
            "{filed}"
        );
        assert!(
            filed.contains(&format!(
                "\n\n{OWNERS} The next run that gets through closes this task.\n\nkey: run\n"
            )),
            "{filed}"
        );
        assert!(!filed.contains("any free executor"), "{filed}");

        let board = Fake::default();
        stopped.stopped = Some("git fetch origin failed");
        stopped.for_owner = false;
        report(&board, &stopped, &[]);
        let filed = board.thread("tests/nightly-red-run").join("\n");
        assert!(
            filed.contains(&format!(
                "\n\n{EXECUTORS} The next run that gets through closes this task.\n\nkey: run\n"
            )),
            "{filed}"
        );
        assert!(!filed.contains("the owner"), "{filed}");
    }

    #[test]
    fn a_run_that_stops_is_one_red_until_a_run_gets_through() {
        let board = Fake::default();
        let mut stopped = night(&[]);
        stopped.commit = "";
        stopped.stopped = Some("git fetch origin failed");
        let done = report(&board, &stopped, &[]);
        assert_eq!(done.filed, ["tests/nightly-red-run"], "{done:?}");
        let filed = board.thread("tests/nightly-red-run").join("\n");
        assert!(
            filed.starts_with("Nightly red: the run stopped before it had a candidate\n"),
            "{filed}"
        );
        assert!(
            filed.contains(
                "stopped before it had a candidate (run 2026-11-12T0300-1, 2026-11-12 03:00) \
                 short of a release: git fetch origin failed\n"
            ),
            "{filed}"
        );
        assert!(filed.ends_with("key: run\ncheck: run"), "{filed}");

        stopped.commit = "cafe0123456789abcdef";
        stopped.stopped = Some("a timeout");
        let done = report(&board, &stopped, &[]);
        assert_eq!(done.updated, ["tests/nightly-red-run"]);
        assert_eq!(
            board.thread("tests/nightly-red-run")[1],
            "Stopped again (run 2026-11-12T0300-1, 2026-11-12 03:00): a timeout"
        );

        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-run"]);
        assert_eq!(
            board.thread("tests/nightly-red-run")[2],
            "Green on cafe01234567 in run 2026-11-12T0300-1 (2026-11-12 03:00): the run got \
             through. Closed by the nightly."
        );
    }

    #[test]
    fn a_night_with_nothing_to_write_registers_no_session() {
        let board = Fake::with(&[("tests/nightly-red-verify", "cancelled")]);
        let done = report(&board, &night(&[]), &[ran("verify", 0), ran("lint", 0)]);
        assert_eq!(done, Board::default());
        assert!(board.sessions.borrow().is_empty(), "no session for nothing");
    }

    #[test]
    fn a_green_that_does_not_hold_the_reds_commit_leaves_it_open() {
        let board = Fake::with(&[
            ("tests/nightly-red-verify", "ready"),
            ("tests/nightly-red-lint", "ready"),
        ]);
        let older = Night {
            closes: &|key| key != "verify",
            ..night(&[])
        };
        let done = report(&board, &older, &[ran("verify", 0), ran("lint", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-lint"]);
        assert_eq!(board.status_of("tests/nightly-red-verify"), "ready");
        assert!(board.thread("tests/nightly-red-verify").is_empty());
    }

    #[test]
    fn a_key_the_night_cleared_closes_its_open_red() {
        let board = Fake::with(&[("tests/nightly-red-conciliate", "ready")]);
        let read = Night {
            cleared: &[CONCILIATE],
            ..night(&[])
        };
        let done = report(&board, &read, &[]);
        assert_eq!(done.closed, ["tests/nightly-red-conciliate"]);
        assert!(
            board.thread("tests/nightly-red-conciliate")[0].contains("`conciliate` passed"),
            "{:?}",
            board.thread("tests/nightly-red-conciliate")
        );
    }

    fn refresh_red(project: &str, why: &str) -> Ran {
        Ran {
            command: format!("the refresh of {project} to release-2026-11-12"),
            exit_code: None,
            error: Some(why.into()),
            ..ran(&format!("refresh-{project}"), 1)
        }
    }

    #[test]
    fn a_red_refresh_is_filed_as_the_refresh_it_is_and_a_green_one_closes_it() {
        let board = Fake::default();
        let gate = "its commit gate refused the move to release-2026-11-12 (`git commit` exited 1)";
        let done = report(&board, &night(&[]), &[refresh_red("alpha", gate)]);
        assert_eq!(done.filed, ["tests/nightly-red-refresh-alpha"]);
        let filed = board.thread("tests/nightly-red-refresh-alpha").join("\n");
        assert!(
            filed.starts_with(
                "Nightly red: the refresh of alpha to release-2026-11-12\nThe Symphony nightly's \
                 the refresh of alpha to release-2026-11-12 is red (run 2026-11-12T0300-1, \
                 2026-11-12 03:00, the release of cafe01234567): its commit gate refused"
            ),
            "{filed}"
        );
        // An orchestrator's rerun finishes it, not any executor's.
        assert!(
            filed.contains(
                "Then an orchestrator reruns the refresh, `just nightly --refresh` in the \
                 Symphony's main checkout, which finishes what this step did not"
            ),
            "{filed}"
        );
        assert!(!filed.contains("any free executor"), "{filed}");
        assert!(
            filed.contains("key: refresh-alpha\ncheck: refresh-alpha"),
            "{filed}"
        );
        assert!(
            !filed.contains("in the nightly's clean checkout"),
            "{filed}"
        );

        // Red again: a line naming why. Green: closed.
        report(
            &board,
            &night(&[]),
            &[refresh_red("alpha", "still refused")],
        );
        let thread = board.thread("tests/nightly-red-refresh-alpha");
        assert!(
            thread[1].starts_with("Still red, refreshing to the release of cafe01234567 (run ")
                && thread[1].contains(": still refused. Log: /no/log"),
            "{thread:?}"
        );
        let green = Ran {
            exit_code: Some(0),
            error: None,
            ..refresh_red("alpha", "")
        };
        let done = report(&board, &night(&[]), &[green]);
        assert_eq!(done.closed, ["tests/nightly-red-refresh-alpha"]);
        assert!(is_refresh("refresh") && is_refresh("refresh-x") && !is_refresh("refreshing"));
    }

    #[test]
    fn a_refresh_alone_never_closes_the_runs_red() {
        let board = Fake::with(&[("tests/nightly-red-run", "ready")]);
        let refresh_only = Night {
            whole: false,
            ..night(&[])
        };
        let done = report(&board, &refresh_only, &[]);
        assert_eq!(done, Board::default());
        assert_eq!(board.status_of("tests/nightly-red-run"), "ready");
        let done = report(&board, &night(&[]), &[]);
        assert_eq!(
            done.closed,
            ["tests/nightly-red-run"],
            "a night that got through does"
        );
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
            refuse: vec!["status"],
            ..Fake::default()
        };
        let done = report(&blind, &night(&[]), &[ran("verify", 1), ran("lint", 0)]);
        assert!(done.filed.is_empty() && done.updated.is_empty() && done.closed.is_empty());
        assert_eq!(
            done.error.as_deref(),
            Some("status refused; status refused; status refused"),
            "each check's and the run's"
        );
        assert!(blind.tasks.borrow().is_empty());
        assert!(
            blind.sessions.borrow().is_empty(),
            "nothing to write: no session"
        );

        let unleaving = Fake {
            refuse: vec!["leave"],
            ..Fake::default()
        };
        let done = report(&unleaving, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.filed, ["tests/nightly-red-verify"]);
        assert_eq!(
            done.error.as_deref(),
            Some("cannot leave the board session filer-1: leave refused")
        );
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
