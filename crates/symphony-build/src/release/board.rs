//! A run's checks on Symphony's own board: one task per red step of a check that lists its steps
//! ([`super::steps`]: `verify`'s lint, client check, size budgets, docs audit, crates check and
//! tests), and per red check that lists none or is red beyond its steps, `tests/release-red-KEY`
//! (KEY the check's name, a step's under its check's: `verify-size-check`), filed at p0, marked red
//! ([`symphony_board::red`]: red work takes no orchestrator slot, `next` offers it before other
//! work of its priority, and PULL names it first while nobody is on it 10 minutes after it was
//! filed), with no fast-fix mark: what its fix needs to land is read at review by an assessor, as
//! for any task ([`symphony_board::stamps`]: its tests for a fix in code, a fresh reviewer's
//! approve as well for one that changes a test or a check), and a stop the owner clears lands
//! nothing. Its text says what is red, what its log names
//! failed ([`super::failures`]: each failed test with where it panicked and cargo's list of the
//! targets that failed, or the lines that say an error, a refusal or the step's own last word)
//! before a few of the log's last lines, the commands that
//! rerun each failure alone ([`super::rerun`]: each failed test, else its own command, a step's
//! included), its log and the machine's load when it ended, a step's as the step's list says it,
//! and, when files its own build had made went while it ran, that those failures are the
//! environment's ([`super::gone`]: while which target, and what its build folder holds now). A
//! red step filed while its check's own red is open names that one, which hears of it. A check
//! red again while its task is open gets a line on that task; a green one closes it, when the
//! candidate holds the commit it was red on (an out-of-cycle release of an older commit leaves it
//! open). A run that stopped short of deciding (it could not fetch, check out, finish in time or
//! push) is the red [`RUN`], which the next run that gets through closes. A closed task's key is
//! filed again under the next generation (`-2`, `-3`). The run acts as a session of kind filer, the
//! one kind the board takes a red mark from, which it registers for the run and leaves; a run
//! with nothing to file, update or close writes nothing.

use serde_json::json;

use super::checks::{Ran, tail};
use super::git::short;
use super::record::{Board, Held};
use symphony::machine::updates::TaskBoard;
use symphony::taskcli::CliBoard;

/// The series a release run's reds are filed in: [`symphony_board::red::SERIES`]'s release one.
pub(crate) const SERIES: &str = "tests/release-red-";
/// The series a build older than the rename filed them in ([`symphony_board::red::SERIES`]'s last),
/// read while a red of it is open ([`generation`]).
pub(crate) const LEGACY_SERIES: &str = "tests/nightly-red-";
/// The key of a run that stopped short of deciding, which no check takes.
pub(crate) const RUN: &str = "run";
/// What a release run red's mark says is red.
const MARK: &str = "release";
/// The document a red task implements.
const LINK: &str = "docs/RELEASES.md";
/// The generations of a key looked at before the board is taken to be wrong.
const GENERATIONS: u32 = 1000;
/// How much of a log a red task quotes: its last lines, within so many bytes; fewer after what it
/// names failed ([`super::failures`]).
const TAIL_LINES: usize = 40;
const TAIL_BYTES: u64 = 6000;
const SHORT_TAIL_LINES: usize = 10;
const SHORT_TAIL_BYTES: u64 = 2000;
/// The commits since the last green run a red task lists at most.
const SINCE_LISTED: usize = 50;

/// The board, as far as a run's reds need it.
pub(crate) trait Reds {
    /// A session of kind filer: its handle.
    fn register(&self) -> Result<String, String>;
    fn leave(&self, handle: &str) -> Result<(), String>;
    /// The status of the task `slug`, none when the board has no such task.
    fn status(&self, slug: &str) -> Result<Option<String>, String>;
    /// A red task `slug`, filed by `by` at p0 and marked red.
    fn create(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String>;
    /// A task `slug` of no red, filed by `by` at p1: the tests a run's failures passed alone.
    fn file(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String>;
    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String>;
    /// Close `slug` with `why` on its thread.
    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String>;
}

/// The board an update's ledger tells ([`TaskBoard`]), as `CliBoard` answers it: the library's
/// calls, which a run's reds make alike.
impl Reds for CliBoard {
    fn register(&self) -> Result<String, String> {
        <CliBoard as TaskBoard>::register(self)
    }

    fn leave(&self, handle: &str) -> Result<(), String> {
        <CliBoard as TaskBoard>::leave(self, handle)
    }

    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        <CliBoard as TaskBoard>::status(self, slug)
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

    fn file(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String> {
        let task = json!({
            "slug": slug,
            "title": title,
            "body": body,
            "by": by,
            "priority": 1,
            "links": [LINK],
        });
        self.call("create", &task).map(|_| ()).map_err(|e| e.0)
    }

    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
        <CliBoard as TaskBoard>::say(self, slug, by, text)
    }

    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
        <CliBoard as TaskBoard>::close(self, slug, by, why)
    }
}

/// A run's board as an update's ledger tells it: the calls of [`TaskBoard`] its [`Reds`] make.
pub(crate) struct Told<'a>(pub(crate) &'a dyn Reds);

impl TaskBoard for Told<'_> {
    fn register(&self) -> Result<String, String> {
        self.0.register()
    }

    fn leave(&self, handle: &str) -> Result<(), String> {
        self.0.leave(handle)
    }

    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        self.0.status(slug)
    }

    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), String> {
        self.0.say(slug, by, text)
    }

    fn close(&self, slug: &str, by: &str, why: &str) -> Result<(), String> {
        self.0.close(slug, by, why)
    }
}

/// The run the reds are of.
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
    /// Whether a green this run closes the open red of a key: the candidate holds what it was
    /// red on.
    pub(crate) closes: &'a dyn Fn(&str) -> bool,
    /// Whether the run checked a candidate (a run or a release), so its getting through closes the
    /// red [`RUN`]; a `--refresh` checks none, and answers for its refresh steps alone.
    pub(crate) whole: bool,
    /// The release candidate's tip, when the run's reds are a release candidate's
    /// ([`super::candidate`]): their tasks say how a fix of one starts, merges and lands.
    pub(crate) candidate: Option<&'a str>,
    /// Whether the run reran the release candidate, whose tip it checked, rather than main's.
    pub(crate) rerun: bool,
    /// How long the machine slept under the run, in seconds; 0 when it did not
    /// ([`super::span`]).
    pub(crate) slept: f64,
    /// Whether the check [`super::rulesets::NAME`] is the one the project's `[releases] rulesets`
    /// names: its red is put right on GitHub by who administers the repository, which its task
    /// says in place of how a fix in code starts, lands and closes it ([`administered`]).
    pub(crate) rulesets: bool,
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

/// The task `key`'s reds go in now: an open red a build older than the rename filed for it in
/// [`LEGACY_SERIES`], so it is updated and closed as it would have been, never left open beside a
/// new one; else the first of its generations in [`SERIES`] that is open (true) or not on the board
/// (false), a closed one passed over.
pub(crate) fn generation(board: &dyn Reds, key: &str) -> Result<(String, bool), String> {
    let (old, open) = first(board, LEGACY_SERIES, key)?;
    if open {
        return Ok((old, true));
    }
    first(board, SERIES, key)
}

/// The open red task of `key`, as a run would tell it ([`generation`]): none when it has none.
pub(crate) fn open(board: &dyn Reds, key: &str) -> Result<Option<String>, String> {
    let (slug, open) = generation(board, key)?;
    Ok(open.then_some(slug))
}

/// The first of `key`'s generations in `series` that is open (true) or not on the board (false).
fn first(board: &dyn Reds, series: &str, key: &str) -> Result<(String, bool), String> {
    let base = format!("{series}{key}");
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

/// What a red task quotes of its log: what it names failed ([`super::failures`]: each failed test
/// with where it panicked and cargo's list of the targets that failed, or the lines that say an
/// error, a refusal or the step's own last word), then its last few lines; its last lines alone
/// when it names nothing, or when it is a check's that listed its steps, whose log is its own lines
/// and its steps' tails, each step being read from its own ([`log_of`]).
fn quoted(ran: &Ran) -> String {
    let named = (ran.steps.is_empty())
        .then(|| super::failures::read(&ran.log).quoted())
        .flatten();
    let Some(named) = named else {
        return quoted_tail(ran, TAIL_LINES, TAIL_BYTES);
    };
    format!(
        "{named}\n\n{}",
        quoted_tail(ran, SHORT_TAIL_LINES, SHORT_TAIL_BYTES)
    )
}

/// The log's end, fenced: its last `lines`, within `bytes`.
fn quoted_tail(ran: &Ran, lines: usize, bytes: u64) -> String {
    let tail = tail(&ran.log, lines, bytes);
    if tail.is_empty() {
        return "Its log is empty or cannot be read.".into();
    }
    format!("Its last lines:\n\n{}", super::failures::fenced(&tail))
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

/// The line that names the commits a red check came between, the last green run's and the
/// candidate, by their full names, for the roll-up to list the landings between them
/// ([`symphony_board::red::BETWEEN`]); nothing when no green run is recorded or the run had no
/// candidate.
fn between(night: &Night<'_>) -> String {
    match night.green {
        Some(green) if !night.commit.is_empty() => format!(
            "{}{green}..{}\n",
            symphony_board::red::BETWEEN,
            night.commit
        ),
        _ => String::new(),
    }
}

/// Why a red is staffed first and how it is fixed: what a red check's task, and a stop's an
/// executor clears, say.
const FIRST: &str = "It blocks every release, so it is staffed before anything else and fixed as \
a bug fix is (RULES.md `workflow/fast-bugfixes`): one agent, the smallest checks that cover the \
repair, and the code fixed, never a test or check loosened. A test the run reran alone that passed \
is no red, and has a task of its own; a test red here failed alone too, or was not rerun. One red \
on the machine's load all the same (it fails on a wall-clock bound, a sleep-ordered step or a \
timeout under load) is discarded in this fix instead, as RULES.md `tests/no-test-fails-on-load` \
says, with one line on this thread naming it, the evidence it is load and what it proved. It is \
red work, which takes no orchestrator slot.";
/// What a red's fix needs to land, as its assessor reads it at review ([`symphony_board::stamps`]):
/// the catalogue's worked example for a red, said in full.
const LANDS: &str = "What its fix needs to land is read at review by an assessor, before any \
reviewer (`/assess-task`): a fix in code that leaves the failing tests unchanged needs its tests \
(the task's recorded test runs green at the fix's tip, the failing tests rerun alone among them); \
one that changes a test, a check, a budget or a run step (`[land] check_paths`) needs a fresh \
reviewer's approve as well, which its orchestrator staffs, and the fix says on the thread why the \
code was not the thing to fix.";
/// Who puts right a red of the project's rulesets check ([`administered`]), and how its task is
/// answered: no executor's commit.
const ADMINISTRATORS: &str = "Who fixes: whoever administers the repository on GitHub, not an \
executor with a commit. This check reads who may bypass the default branch's rules, with the gh of \
whoever runs the release, and its lines above say what it found, each thing missing among them. \
Where they say the policy does not hold, the rules are put right on GitHub (docs/GITHUB.md, \"Who \
bypasses the rules\"); where they say the rulesets were not read whole, that gh is signed in as \
someone who administers the repository; only where they say the adapter is turned off is it a \
setting of the project's, put right as a change of settings. No code is changed and no check \
loosened for it. Whoever takes this task says on its thread what those lines name and puts it to \
the owner (`just tasks say SLUG TEXT --to owner`), then cancels the task once it is put right, \
saying where, as a red fixed outside this repository is answered (RULES.md \
`workflow/release-reds-first`). It blocks every release, so it is put to them before anything else.";
/// What a stop the owner clears says of its landing: nothing lands for it.
const NOTHING_LANDS: &str = "Nothing lands for it: what clears it is a process ended, not a fix.";
/// How many of the commands that rerun a failure alone a red task lists at most: as many as the
/// run reruns itself ([`super::alone::MOST`]).
const RERUNS_LISTED: usize = super::alone::MOST;

/// How a red task says the load when its check ended, and beside it how long the machine slept
/// under its run, when it did: a failure across a sleep is not one of load.
fn load(ran: &Ran, night: &Night<'_>) -> String {
    let load = match &ran.load {
        Some(load) => format!("Load average when it ended: {}.", load.said()),
        None => "Load average when it ended: not recorded.".into(),
    };
    match super::span::said(night.slept) {
        Some(slept) => format!(
            "{load}\nThe machine slept {slept} under this run, which its times count: a wait or a \
             timeout that failed across that sleep failed on the sleep, not on load."
        ),
        None => load,
    }
}

/// Whether `command` runs a whole test run: `cargo test` (through `tools/cargo.sh` too), `just
/// test-rs` or the tests' lanes (`tools/test-lanes.sh`, verify's tests step), whose rerun alone is
/// a target of it, never all of it again.
fn runs_tests(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    let lanes = |word: &&str| *word == "test-lanes.sh" || word.ends_with("/test-lanes.sh");
    words.iter().any(lanes)
        || words.windows(2).any(|pair| {
            let [tool, what] = pair else { return false };
            ((tool.ends_with("cargo") || tool.ends_with("cargo.sh")) && *what == "test")
                || (*tool == "just" && *what == "test-rs")
        })
}

/// What a red task says reruns each target of a test run alone when its log names none failed.
const EACH_TARGET: &str = "each target alone, as `just test-rs -p PACKAGE --test TARGET` (`--lib` \
for a package's own tests), the one its last lines name, never the whole run again";

/// How a red task says to rerun each failure of its check alone ([`super::rerun`]): each failed
/// test its own log names, as a step's red reads its own (the tests' step's naming each target);
/// else its own command, a step's as its list records it (`just size-check`), but a test run's,
/// which is rerun a target at a time ([`EACH_TARGET`]). `just verify` is never named, as no task
/// runs it: one that listed no steps (a verify older than its steps' list, as an out-of-cycle
/// release of an older commit runs) is rerun by the step its log's recipes name, its tests' a
/// target at a time; one that listed
/// its steps and is red beyond them, whose log is its own lines and its steps' tails, by the
/// commands its steps recorded.
fn alone(ran: &Ran, night: &Night<'_>) -> String {
    let verify = ran.command == super::VERIFY.1;
    let listed = !ran.steps.is_empty();
    let log = log_of(ran);
    let each = super::rerun::arguments(&log.targets);
    let place = format!("in a checkout of {}", short(night.commit));
    if each.is_empty() && log.targets.iter().any(|failed| !failed.gone.is_empty()) {
        let (went, after) = match &log.went {
            Some(super::rerun::Went::While(header, selected)) => (
                format!(
                    "the target they went while, {}",
                    selected.as_ref().map_or_else(
                        || format!("which `{header}` names"),
                        |args| format!("`{args}`")
                    )
                ),
                "it",
            ),
            // No one target: any of the lanes, run side by side, may have removed them.
            Some(super::rerun::Went::Lanes(..)) => (
                "each of the lanes they went while, alone and one after another, by the targets \
                 the log names for it (`just test-rs --workspace TARGETS`)"
                    .into(),
                "each",
            ),
            _ => ("its build".into(), "it"),
        };
        return format!(
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): none of its failures is a \
             test's to rerun, as each failed target found files of its build gone (above). What \
             removes them shows in a run of {went}, {place}, with a look at its build folder \
             after {after}."
        );
    }
    if each.is_empty() {
        let named = (verify && !listed)
            .then(|| super::rerun::step(&log.recipes))
            .flatten();
        // A test run's recipe named is the whole run, rerun a target at a time below.
        if let Some(step) = named.as_ref().filter(|step| !runs_tests(step)) {
            return format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), {place}, the step its \
                 log names failed: `{step}`."
            );
        }
        if listed {
            let steps: Vec<String> = (ran.steps.iter())
                .map(|step| {
                    let said = if step.green() {
                        "green".to_string()
                    } else if super::steps::not_run(step) {
                        step.ended()
                    } else {
                        format!("{}, a task of its own", step.ended())
                    };
                    format!("`{}` ({said})", step.command)
                })
                .collect();
            return format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), {place}: each step it \
                 listed by the command it recorded, {}; a step it did not reach, by the command its \
                 last lines name, a test run's {EACH_TARGET}.",
                steps.join(", ")
            );
        }
        if verify && named.is_none() {
            return format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), {place}: its log names no \
                 failed test or step, so its last lines say which of verify's steps failed, and \
                 that one alone reruns it, by its own recipe (`just RECIPE`), or its tests, \
                 {EACH_TARGET}."
            );
        }
        if named.is_some() || runs_tests(&ran.command) {
            return format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), {place}: its log names no \
                 failed test target, so {EACH_TARGET}."
            );
        }
        return format!(
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): its log names no failed test, \
             so nothing narrower than its own command, `{}`, {place}.",
            ran.command
        );
    }
    let mut text = format!(
        "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), {place}, as its log names each \
         failure ({}):\n",
        each.len()
    );
    for args in each.iter().take(RERUNS_LISTED) {
        text.push_str(&format!("- `just test-rs {args}`\n"));
    }
    if each.len() > RERUNS_LISTED {
        text.push_str(&format!(
            "- … and {} more in the log\n",
            each.len() - RERUNS_LISTED
        ));
    }
    text.trim_end().to_string()
}

/// What a red check's own log says failed: none for a check that listed its steps, whose log is
/// its own lines and its steps' tails, each step being read from its own.
fn log_of(ran: &Ran) -> super::rerun::Log {
    if ran.steps.is_empty() {
        super::rerun::read(&ran.log)
    } else {
        super::rerun::Log::default()
    }
}

/// What a red task says of the files of its build its targets found gone ([`super::gone`]), a
/// paragraph of its own; nothing when none did.
fn environment(ran: &Ran) -> String {
    super::gone::said(&log_of(ran))
        .map(|said| format!("{said}\n\n"))
        .unwrap_or_default()
}

/// What a red task of the release candidate held at `tip` says of its fix ([`super::candidate`]):
/// where its branch starts and what it merges, when it commits the client, how its approve and its
/// land move the candidate, and what reruns it.
fn held(tip: &str) -> String {
    let branch = super::candidate::BRANCH;
    format!(
        "The release candidate: this run holds {tip} as the commit to release, the local branch \
         `{branch}` (never pushed), so the release is that commit and its reds' fixes alone, however \
         fast main moves. Fix it there, not on main: start this task's branch, named for its slug, \
         at `{branch}`, never at main; merge `{branch}` again before review, as it moves when \
         another red's fix lands; and never merge main before its land. When the fix changes \
         what the client is built from, commit the distribution its own sources make (`just \
         client-update`): the one kind of branch that commits it, as no regenerator builds the \
         candidate, and the land keeps main's side of it. Its approve, where it needs review, names \
         the full tip it read. It lands by `just land` like any task, and its land advances the \
         candidate to the newest commit of this branch that holds the candidate and the fix alone \
         among those an approve names, or, where it needed no review, the tip its landing's line \
         says its needed stamps stood on. A land that conflicts comes back to its executor: merge \
         main then and ask for a fresh approve, and the candidate takes the commit approved before \
         that merge; a fix that needed no review has none, and drops it. A fix that merges main \
         before any approve leaves the candidate nothing to take, and drops it. A fix that landed \
         with another red's task answers this red by the task closed as covered by that landing \
         (`just tasks done SLUG NOTE --covered-by OTHER`), once that land advances the candidate; \
         closed as covered by any other landing, its fix is on main and not in the candidate, \
         which is dropped. A fix made outside this repository answers this red by the task \
         cancelled, saying where. Once every red of the candidate is answered, its tip is rerun \
         (the next landing starts it, or `just release --rerun`): each step that was red, did not \
         run or whose inputs a fix changed.\n\n",
        tip = short(tip)
    )
}

/// Whether `ran` is the project's rulesets check ([`super::rulesets`]), whose red no commit
/// answers: a check a run was given under that name, in a project that names no command, is any
/// check.
fn administered(ran: &Ran, night: &Night<'_>) -> bool {
    night.rulesets && ran.name == super::rulesets::NAME
}

/// What a red of the project's rulesets check held with the release candidate at `tip` says of it,
/// in [`held`]'s place: no branch starts there for it.
fn held_administered(tip: &str) -> String {
    format!(
        "The release candidate: this run holds {tip} as the commit to release, the local branch \
         `{branch}` (never pushed). This red asks for no branch and no fix there. Once it is \
         answered, and every other red of the candidate with it, the candidate's tip is rerun (the \
         next landing starts it, or `just release --rerun`), which runs `{name}` again.\n\n",
        tip = short(tip),
        branch = super::candidate::BRANCH,
        name = super::rulesets::NAME,
    )
}

/// What closes a red of the project's rulesets check, in [`closed_by`]'s place: the task
/// cancelled once it is put right, or a run green in it.
fn closed_administered(name: &str, night: &Night<'_>) -> String {
    if night.candidate.is_some() {
        return format!(
            "Cancelling this task once it is put right, saying where, answers it for the release \
             candidate, whose rerun runs `{name}` again; a rerun of the candidate green in `{name}` \
             closes it too."
        );
    }
    format!(
        "The next run of `just release` closes this task once `{name}` is green; cancelling it \
         once it is put right, saying where, answers it before that."
    )
}

/// What closes a red task of the check `name`: a run green in it, and, for a red of the release
/// candidate, its own land.
fn closed_by(name: &str, night: &Night<'_>) -> String {
    if night.candidate.is_some() {
        return format!(
            "Its own land closes this task and answers it for the release candidate; a rerun of \
             the candidate green in `{name}` closes it too."
        );
    }
    format!(
        "The next run of `just release` closes this task once `{name}` is green; a fix released out \
         of cycle (`just release --commit COMMIT`) closes it too."
    )
}

/// A red task's body: what is red, how it ended, its log, the load it ended under, what its log
/// names failed and its last lines ([`quoted`]), what of it is
/// the environment's (files of its build gone while it ran), the commands that rerun its failures
/// alone, the commits since the last green run, how a release candidate's red is fixed ([`held`]),
/// who fixes it, how its fix is approved, the open red `beside` it ([`beside`]), what closes it and
/// the commits it came between ([`between`]). A red of the project's rulesets check says who
/// administers the repository puts it right, and how its task is answered, in place of those of a
/// fix in code ([`administered`]).
fn body(ran: &Ran, night: &Night<'_>, beside: Option<&str>) -> String {
    let administered = administered(ran, night);
    format!(
        "The Symphony release run is red in `{name}` on {on} at {commit} (run {id}, {when}).\n\n\
         Command: `{command}`, in the release run's clean checkout of that commit: {ended}.\n\
         Log: {log}\n\
         {load}\n\n\
         {tail}\n\n\
         {environment}\
         {alone}\n\n\
         {reran}\
         {since}\n\n\
         {held}\
         {who}\n\n\
         {beside}\
         {closed}\n\n\
         {between}\
         key: {key}\ncheck: {name}",
        between = between(night),
        held = (night.candidate)
            .map(if administered {
                held_administered
            } else {
                held
            })
            .unwrap_or_default(),
        who = if administered {
            ADMINISTRATORS.to_string()
        } else {
            format!("{EXECUTORS} {FIRST} {LANDS}")
        },
        on = if night.rerun {
            "the release candidate"
        } else {
            "main"
        },
        closed = if administered {
            closed_administered(&ran.name, night)
        } else {
            closed_by(&ran.name, night)
        },
        beside = beside.unwrap_or_default(),
        name = ran.name,
        commit = short(night.commit),
        id = night.id,
        when = night.when,
        command = ran.command,
        ended = ran.ended(),
        log = ran.log.display(),
        load = load(ran, night),
        tail = quoted(ran),
        environment = environment(ran),
        alone = alone(ran, night),
        reran = reran(ran),
        since = since(night),
        key = key(&ran.name),
    )
}

/// The line a red task that is still open gets on a later red run: for a release candidate's, its
/// tip, which its fix starts from or merges, or, for the project's rulesets check
/// ([`administered`]), which no branch starts from.
fn again(ran: &Ran, night: &Night<'_>) -> String {
    let held = night.candidate.map_or_else(String::new, |tip| {
        if administered(ran, night) {
            return format!(
                "\n\nThe release candidate is {}, the branch `{}`: no fix of this red starts \
                 there, as who administers the repository puts it right on GitHub.",
                short(tip),
                super::candidate::BRANCH
            );
        }
        format!(
            "\n\nThe release candidate is {}, the branch `{}`: a fix of this red starts from it, or \
             merges it before review, and never merges main before the approve.",
            short(tip),
            super::candidate::BRANCH
        )
    });
    format!(
        "Still red on {} (run {}, {}): {}. Log: {}\n{}\n\n{}{}\n\n{}{}{held}",
        short(night.commit),
        night.id,
        night.when,
        ran.ended(),
        ran.log.display(),
        load(ran, night),
        environment(ran),
        alone(ran, night),
        reran(ran),
        quoted(ran)
    )
}

/// What a red task says of its failures the run reran alone ([`super::alone`]), a paragraph of its
/// own: each rerun's result, with its time, load and log, or why none was rerun; nothing for a
/// check whose failures the run does not rerun.
fn reran(ran: &Ran) -> String {
    let Some(alone) = &ran.alone else {
        return String::new();
    };
    if alone.reruns.is_empty() {
        let why = alone.why_not.as_deref().unwrap_or("it named none");
        return format!("The run reran none of its failures alone: {why}.\n\n");
    }
    let mut text = format!(
        "The run reran each failure alone, one at a time, in this checkout and over the build that \
         failed (`{} --alone`); each failed in the run under the load above, and alone:\n",
        super::alone::RUNNER
    );
    for rerun in &alone.reruns {
        let twice = if super::alone::failed_again(rerun) {
            ": it failed twice"
        } else {
            ""
        };
        text.push_str(&format!(
            "- `{}`: {} ({}, {:.0} s; {}){twice}. Log: {}\n",
            rerun.name,
            super::alone::said_of(rerun),
            rerun.ended(),
            rerun.seconds,
            super::alone::load_of(rerun),
            rerun.log.display()
        ));
    }
    if let Some(why) = &alone.why_not {
        text.push_str(&format!("Those reruns judge nothing: {why}.\n"));
    }
    text.push('\n');
    text
}

/// The series of the tasks a run files for the tests that failed in it and passed alone: one task
/// a run, never a red, and never in [`SERIES`].
pub(crate) const PASSED_ALONE: &str = "tests/passed-alone-";

/// The task the run `id` files for its tests that passed alone.
pub(crate) fn passed_alone_slug(id: &str) -> String {
    format!("{PASSED_ALONE}{}", key(id))
}

/// The task a run files for every test that failed in it and passed when the run reran it alone,
/// whatever else it found: its slug, title and body; none when no such test. Reruns that judge
/// nothing ([`super::alone::Alone::why_not`]) name none.
fn passed_alone(night: &Night<'_>, checks: &[Ran]) -> Option<(String, String, String)> {
    let load = super::alone::load_of;
    let lines: Vec<String> = (super::alone::passed_in(checks).into_iter())
        .map(|(step, rerun)| {
            format!(
                "- `{}`, in `{}`: failed in the run ({}, {}; log {}); passed alone ({}, {:.0} s, \
                 {}; log {}).",
                rerun.name,
                step.name,
                super::alone::failed(step),
                load(step),
                step.log.display(),
                rerun.ended(),
                rerun.seconds,
                load(rerun),
                rerun.log.display()
            )
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let slug = passed_alone_slug(night.id);
    let title = format!(
        "Passed alone: {} test{} failed in release run {} and passed rerun alone",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" },
        night.id
    );
    let body = format!(
        "The Symphony release run {id} ({when}), on {on} at {commit}, failed these tests in company, \
         and each passed when the run reran it alone, one at a time, in the same checkout and over \
         the same build (RULES.md `tests/rerun-the-failure-alone`). So none of them is a red of the \
         release (docs/RELEASES.md, \"The release run\", step 5), and the release was judged \
         without them; this task is for them, all of this run's in one.\n\n\
         {lines}\n\n\
         Find why each failed in company and passed alone: an order or a resource it shares with \
         another test, the machine's load (a wall-clock bound, a sleep-ordered step or a timeout), \
         or the runner. Then fix the code or the runner, rewrite the test on a condition, a count, \
         CPU time or an injected clock (docs/CHECKS.md, \"Time in tests\"), or discard it with one \
         line on this thread naming the test, the evidence and what it proved (RULES.md \
         `tests/no-test-fails-on-load`). Never a timeout, a budget or a tolerance widened, a retry \
         added or a test ignored to pass. It holds no release: it is p1 work, not red, staffed as \
         any task.\n\n\
         run: {id}",
        id = night.id,
        when = night.when,
        on = if night.rerun {
            "the release candidate"
        } else {
            "main"
        },
        commit = short(night.commit),
        lines = lines.join("\n"),
    );
    Some((slug, title, body))
}

/// Whether the step `name` is one of a release's refresh ([`super::refresh::STEP`]: its install
/// or a project's board), not a check of the candidate.
pub(crate) fn is_refresh(name: &str) -> bool {
    let key = key(name);
    key == super::refresh::STEP || key.starts_with(&format!("{}-", super::refresh::STEP))
}

/// Why a refresh step is red: what it said, or how it ended.
fn refresh_why(ran: &Ran) -> String {
    ran.error.clone().unwrap_or_else(|| ran.ended())
}

/// A red refresh step's body: what it was refreshing and why it is red, its log, what it left as
/// it was, who fixes it and what closes it. Only the refresh's own work is red: a board whose
/// writers its project rebuilds is that project's to finish, filed nowhere.
fn refresh_body(ran: &Ran, night: &Night<'_>) -> String {
    format!(
        "The Symphony release run's {command} is red (run {id}, {when}, the release of {commit}): \
         {why}\n\n\
         Log: {log}\n\n\
         {tail}\n\n\
         What it left as it was: what the step had not reached, and `current`, which a red refresh \
         never points. A red install leaves nothing of the release in the machine folder; a red \
         board move moves no schema, the snapshot before it kept; the machine's server keeps \
         serving what it served. No project's checkout is written. Nothing of the release is \
         undone: it is tagged and pushed.\n\n\
         Who fixes: what the line above names. A red here is the refresh's own work that failed, \
         fixed here; a board whose writers lag behind the release is that project's to finish and \
         is never filed here. {LANDS} Then an orchestrator reruns the refresh, `just release \
         --refresh` in the Symphony's main checkout, which finishes what this step did not; a \
         green refresh of it closes this task.\n\n\
         key: {key}\ncheck: {name}",
        command = ran.command,
        id = night.id,
        when = night.when,
        commit = short(night.commit),
        why = refresh_why(ran),
        log = ran.log.display(),
        tail = quoted(ran),
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
        quoted(ran)
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
executor. A process stopped the run by being in the release run's checkout, and it may be \
anyone's: a shell, a terminal tab, an editor or a Finder window there counts. An executor ends \
only a process it started (RULES `delete-by-name`). End it, or move it out of that folder, and the \
next run goes.";
/// Who fixes any other stop.
const EXECUTORS: &str = "Who fixes: any free executor takes this from the board.";

/// What the red [`RUN`] says of the reds its checks found before it stopped, `held`, each filed or
/// heard on a task of its own this run: nothing when there are none.
fn reds_beside(held: &[String]) -> String {
    if held.is_empty() {
        return String::new();
    }
    format!(
        "The reds its checks found before it stopped are tasks of their own, not this one's: {}.",
        held.join(", ")
    )
}

/// The red [`RUN`]'s body: why the run stopped, the reds `held` beside it ([`reds_beside`]), who
/// fixes it and what closes it.
fn stopped_body(why: &str, night: &Night<'_>, held: &[String]) -> String {
    let beside = reds_beside(held);
    format!(
        "The Symphony release run stopped {on} (run {id}, {when}) short of a release: {why}\n\n\
         {beside}{gap}\
         Until a run gets through, main goes unverified and unreleased. The run's record is \
         `symphony/release/runs/{id}.json` in the git common dir, and `just release --status` \
         says the last run.\n\n\
         {who} The next run that gets through closes this task.\n\n\
         key: {RUN}\ncheck: {RUN}",
        on = on(night),
        id = night.id,
        when = night.when,
        gap = if beside.is_empty() { "" } else { "\n\n" },
        who = if night.for_owner {
            format!("{OWNERS} {NOTHING_LANDS}")
        } else {
            format!("{EXECUTORS} {FIRST} {LANDS}")
        },
    )
}

/// What a key that is red files: the task's title and body, and the line a later red run adds.
struct Red {
    title: String,
    body: String,
    again: String,
}

/// What a run does to one key's task.
enum Act {
    /// File the red `slug`; its later line goes on it instead in case another run filed it.
    File {
        slug: String,
        red: Red,
        of: Option<Of>,
    },
    /// Add a line to the open red `slug`.
    Say {
        slug: String,
        text: String,
        of: Option<Of>,
    },
    /// Close the open red `slug`, saying why.
    Close { slug: String, why: String },
    /// File the task `slug` of the tests that passed alone ([`passed_alone`]): no red.
    Alone {
        slug: String,
        title: String,
        body: String,
    },
}

/// The red check or step an act files or tells: its key and name.
struct Of {
    key: String,
    name: String,
}

impl Of {
    /// The red [`Held`] on the task `slug`.
    fn on(&self, slug: &str) -> Held {
        Held {
            key: self.key.clone(),
            name: self.name.clone(),
            slug: slug.to_string(),
        }
    }
}

/// `act` as the red of the check or step `ran` when it files or tells one, so the run knows each
/// red's task ([`Board::reds`]).
fn as_red(act: Result<Option<Act>, String>, ran: &Ran) -> Result<Option<Act>, String> {
    let of = || {
        Some(Of {
            key: key(&ran.name),
            name: ran.name.clone(),
        })
    };
    act.map(|act| {
        act.map(|act| match act {
            Act::File { slug, red, .. } => Act::File {
                slug,
                red,
                of: of(),
            },
            Act::Say { slug, text, .. } => Act::Say {
                slug,
                text,
                of: of(),
            },
            other @ (Act::Close { .. } | Act::Alone { .. }) => other,
        })
    })
}

/// The act on the key `key`'s task: red with what it files, else green with the name of what
/// passed; none when there is nothing to write.
fn act(
    board: &dyn Reds,
    night: &Night<'_>,
    key: &str,
    red: Option<Red>,
    passed: &str,
) -> Result<Option<Act>, String> {
    let (slug, open) = generation(board, key)?;
    Ok(match (red, open) {
        (None, false) => None,
        (None, true) if !(night.closes)(key) => None,
        (None, true) => Some(Act::Close {
            slug,
            why: format!(
                "Green {} in run {} ({}): {passed}. Closed by the release run.",
                on(night),
                night.id,
                night.when,
            ),
        }),
        (Some(red), true) => Some(Act::Say {
            slug,
            text: red.again,
            of: None,
        }),
        (Some(red), false) => Some(Act::File {
            slug,
            red,
            of: None,
        }),
    })
}

/// What a red check files, as a refresh step's or a check's (a step a check listed among them), its
/// body naming the open red `beside` it when there is one; none when green.
fn red_of(ran: &Ran, night: &Night<'_>, beside: Option<&str>) -> Option<Red> {
    (!ran.green()).then(|| {
        if is_refresh(&ran.name) {
            Red {
                title: format!("Release red: {}", ran.command),
                body: refresh_body(ran, night),
                again: refresh_again(ran, night),
            }
        } else {
            Red {
                title: format!("Release red: {} on {}", ran.name, short(night.commit)),
                body: body(ran, night, beside),
                again: again(ran, night),
            }
        }
    })
}

/// What the body of a red step filed while its check's own red `slug` is open says of that one:
/// one filed before the check listed its steps, as a whole-verify red was, can be this same red.
fn beside(slug: &str, check: &str) -> String {
    format!(
        "The open red of `{check}` itself, {slug}, was filed before this one: one filed before \
         `{check}` listed its steps, or for a red beyond them, can be this same failure, so whoever \
         takes this reads its thread first, and the two are fixed as one where they are. That task \
         hears of this one, and closes once `{check}` is green.\n\n"
    )
}

/// The line the open red of a check gets on a run it is red in its steps alone, each a task of
/// its own ([`super::steps`]): `held`, each red step with its task.
fn in_steps(ran: &Ran, night: &Night<'_>, held: &[String]) -> String {
    format!(
        "Red on {} (run {}, {}) in its steps alone, each a task of its own: {}. Whoever holds this \
         task reads those first, as one can be this same failure. This task closes once `{}` is \
         green.",
        short(night.commit),
        night.id,
        night.when,
        held.join(", "),
        ran.name,
    )
}

/// What the run does to the tasks of the check `ran` and of each step it listed, in that order
/// but the check's last, with what the board refused. A check red in its steps alone files nothing
/// of its own: its open red, if it has one, hears each red step's task, and a red step filed now
/// names it on its own ([`beside`]), so two tasks for one red are linked both ways.
fn of_check(board: &dyn Reds, night: &Night<'_>, ran: &Ran) -> Vec<Result<Option<Act>, String>> {
    let passed = |ran: &Ran| format!("`{}` passed", ran.name);
    let mut found = Vec::new();
    let told = ran.green() || super::steps::beyond(ran);
    let open = if told {
        None
    } else {
        match generation(board, &key(&ran.name)) {
            Ok((slug, true)) => Some(slug),
            Ok(_) => None,
            Err(error) => {
                found.push(Err(error));
                None
            }
        }
    };
    let mut held = Vec::new();
    // A step listed as not run is no red of its own: its task, if one is open, hears nothing.
    for step in (ran.steps.iter()).filter(|step| !super::steps::not_run(step)) {
        let near = open.as_deref().map(|slug| beside(slug, &ran.name));
        let red = red_of(step, night, near.as_deref());
        let act = as_red(
            act(board, night, &key(&step.name), red, &passed(step)),
            step,
        );
        if let Ok(Some(Act::File { slug, .. } | Act::Say { slug, .. })) = &act {
            held.push(format!("`{}` ({}) in {slug}", step.name, step.ended()));
        }
        found.push(act);
    }
    if told {
        let red = red_of(ran, night, None);
        found.push(as_red(
            act(board, night, &key(&ran.name), red, &passed(ran)),
            ran,
        ));
    } else if let Some(slug) = open {
        found.push(Ok(Some(Act::Say {
            slug,
            text: in_steps(ran, night, &held),
            of: None,
        })));
    }
    found
}

/// File or update the red task of each red check of `checks` and of the run when it stopped, and
/// close the open one of each green check and of the run when it got through: what it did on the
/// board, with every refusal it met. Each step a check listed is
/// a check here; a check red in its steps alone files nothing of its own, and its open red, if it
/// has one, hears which ([`super::steps::beyond`]). The red run names the reds filed or heard beside
/// it ([`reds_beside`]), so a stop never hides one. A run with nothing to write registers no
/// session.
pub(crate) fn report(board: &dyn Reds, night: &Night<'_>, checks: &[Ran]) -> Board {
    let mut done = Board::default();
    let mut found: Vec<Result<Option<Act>, String>> = Vec::new();
    for ran in checks {
        found.extend(of_check(board, night, ran));
    }
    // The reds filed or heard so far, which the red run names beside it.
    let held: Vec<String> = (found.iter())
        .filter_map(|act| match act {
            Ok(Some(Act::File { slug, .. } | Act::Say { slug, .. })) => Some(slug.clone()),
            _ => None,
        })
        .collect();
    if night.whole {
        let stopped = night.stopped.map(|why| {
            let beside = reds_beside(&held);
            let gap = if beside.is_empty() { "" } else { " " };
            Red {
                title: format!("Release red: the run stopped {}", on(night)),
                body: stopped_body(why, night, &held),
                again: format!(
                    "Stopped again (run {}, {}): {why}{gap}{beside}",
                    night.id, night.when
                ),
            }
        });
        found.push(act(board, night, RUN, stopped, "the run got through"));
    }
    if let Some((slug, title, body)) = passed_alone(night, checks) {
        found.push(Ok(Some(Act::Alone { slug, title, body })));
    }
    let mut errors = Vec::new();
    let mut acts = Vec::new();
    for found in found {
        match found {
            Ok(found) => acts.extend(found),
            Err(error) => errors.push(error),
        }
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
            Act::Say { slug, text, of } => board.say(slug, &handle, text).map(|()| {
                done.updated.push(slug.clone());
                done.reds.extend(of.as_ref().map(|of| of.on(slug)));
            }),
            Act::Alone { slug, title, body } => board
                .file(slug, title, body, &handle)
                .map(|()| done.filed.push(slug.clone())),
            Act::File { slug, red, of } => {
                let wrote = match board.create(slug, &red.title, &red.body, &handle) {
                    Ok(()) => {
                        done.filed.push(slug.clone());
                        Ok(())
                    }
                    // Another run filed it first: update it, never file a second.
                    Err(error) => match board.status(slug) {
                        Ok(Some(status)) if !closed(&status) => board
                            .say(slug, &handle, &red.again)
                            .map(|()| done.updated.push(slug.clone())),
                        _ => Err(error),
                    },
                };
                if wrote.is_ok() {
                    done.reds.extend(of.as_ref().map(|of| of.on(slug)));
                }
                wrote
            }
        };
        if let Err(error) = wrote {
            let (Act::Close { slug, .. }
            | Act::Say { slug, .. }
            | Act::File { slug, .. }
            | Act::Alone { slug, .. }) = &act;
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

        /// A task of no red: its first post says so, before its title and body.
        fn file(&self, slug: &str, title: &str, body: &str, by: &str) -> Result<(), String> {
            assert_eq!(by, "filer-1");
            self.refused("file")?;
            self.tasks.borrow_mut().insert(
                slug.into(),
                ("ready".into(), vec![format!("p1, no red: {title}\n{body}")]),
            );
            Ok(())
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
            steps: Vec::new(),
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
            alone: None,
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
            closes: &|_| true,
            whole: true,
            candidate: None,
            rerun: false,
            slept: 0.0,
            rulesets: false,
        }
    }

    #[test]
    fn a_stop_for_a_process_in_the_runs_folders_is_the_owners_and_any_other_an_executors() {
        let board = Fake::default();
        let mut stopped = night(&[]);
        stopped.stopped = Some(
            "/wt/release is in use by process 10 (zsh), working there: nothing is checked while \
             a process is in the run's folders; end it, or move it out of that folder (a shell, \
             a terminal tab, an editor or a Finder window there counts), and the next run goes",
        );
        stopped.for_owner = true;
        report(&board, &stopped, &[]);
        let filed = board.thread("tests/release-red-run").join("\n");
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
                "\n\n{OWNERS} {NOTHING_LANDS} The next run that gets through closes this task.\n\nkey: \
                 run\n"
            )),
            "{filed}"
        );
        assert!(!filed.contains("any free executor"), "{filed}");
        assert!(
            !filed.contains(LANDS),
            "nothing lands for it, so nothing to assess: {filed}"
        );

        let board = Fake::default();
        stopped.stopped = Some("git fetch origin failed");
        stopped.for_owner = false;
        report(&board, &stopped, &[]);
        let filed = board.thread("tests/release-red-run").join("\n");
        assert!(
            filed.contains(&format!(
                "\n\n{EXECUTORS} {FIRST} {LANDS} The next run that gets through closes this \
                 task.\n\nkey: run\n"
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
        assert_eq!(done.filed, ["tests/release-red-run"], "{done:?}");
        let filed = board.thread("tests/release-red-run").join("\n");
        assert!(
            filed.starts_with("Release red: the run stopped before it had a candidate\n"),
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
        assert_eq!(done.updated, ["tests/release-red-run"]);
        assert_eq!(
            board.thread("tests/release-red-run")[1],
            "Stopped again (run 2026-11-12T0300-1, 2026-11-12 03:00): a timeout"
        );

        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/release-red-run"]);
        assert_eq!(
            board.thread("tests/release-red-run")[2],
            "Green on cafe01234567 in run 2026-11-12T0300-1 (2026-11-12 03:00): the run got \
             through. Closed by the release run."
        );
    }

    #[test]
    fn a_night_with_nothing_to_write_registers_no_session() {
        let board = Fake::with(&[("tests/release-red-verify", "cancelled")]);
        let done = report(&board, &night(&[]), &[ran("verify", 0), ran("lint", 0)]);
        assert_eq!(done, Board::default());
        assert!(board.sessions.borrow().is_empty(), "no session for nothing");
    }

    #[test]
    fn a_green_that_does_not_hold_the_reds_commit_leaves_it_open() {
        let board = Fake::with(&[
            ("tests/release-red-verify", "ready"),
            ("tests/release-red-lint", "ready"),
        ]);
        let older = Night {
            closes: &|key| key != "verify",
            ..night(&[])
        };
        let done = report(&board, &older, &[ran("verify", 0), ran("lint", 0)]);
        assert_eq!(done.closed, ["tests/release-red-lint"]);
        assert_eq!(board.status_of("tests/release-red-verify"), "ready");
        assert!(board.thread("tests/release-red-verify").is_empty());
    }

    /// The load each step of [`verify`] ended under, as its list says it.
    fn step_load() -> super::super::checks::Load {
        super::super::checks::Load {
            averages: [1.5, 2.0, 2.5],
            processors: 8,
        }
    }

    /// `verify`, red when a step is, listing its steps each with its exit status, its command
    /// `just STEP`, a log of its own and the load it ended under.
    fn verify(steps: &[(&str, i32)]) -> Ran {
        let steps: Vec<Ran> = (steps.iter())
            .map(|(name, code)| Ran {
                command: format!("just {name}"),
                log: PathBuf::from(format!("/logs/verify/{name}.log")),
                load: Some(step_load()),
                ..ran(&format!("verify/{name}"), *code)
            })
            .collect();
        let red = steps.iter().any(|step| !step.green());
        Ran {
            steps,
            ..ran("verify", i32::from(red))
        }
    }

    #[test]
    fn a_stop_after_a_red_check_files_each_red_step_beside_the_run_which_names_them() {
        let board = Fake::with(&[("tests/release-red-verify-lint", "ready")]);
        let mut stopped = night(&[]);
        stopped.stopped = Some(
            "after `verify` ended red (exit 1), /wt/release is in use by process 10 \
             (chrome_crashpad_handler), working in apps/symphony-web",
        );
        stopped.for_owner = true;
        // As the run keeps a check a look stopped it after: its red steps alone.
        let red = verify(&[("tests", 101)]);
        let done = report(&board, &stopped, std::slice::from_ref(&red));
        assert_eq!(
            done.filed,
            ["tests/release-red-verify-tests", "tests/release-red-run"]
        );
        assert!(
            done.closed.is_empty(),
            "a step it found green closes nothing: {done:?}"
        );
        assert_eq!(board.status_of("tests/release-red-verify-lint"), "ready");
        let run = board.thread("tests/release-red-run").join("\n");
        assert!(
            run.contains(
                "working in apps/symphony-web\n\nThe reds its checks found before it stopped are \
                 tasks of their own, not this one's: tests/release-red-verify-tests.\n\nUntil a run \
                 gets through"
            ),
            "{run}"
        );

        // Stopped again on the same red: the step's task hears it, and the run's line names it.
        let done = report(&board, &stopped, &[red]);
        assert_eq!(
            done.updated,
            ["tests/release-red-verify-tests", "tests/release-red-run"]
        );
        assert!(
            board.thread("tests/release-red-run")[1].ends_with(
                "working in apps/symphony-web The reds its checks found before it stopped are \
                 tasks of their own, not this one's: tests/release-red-verify-tests."
            ),
            "{:?}",
            board.thread("tests/release-red-run")
        );
    }

    #[test]
    fn each_red_step_of_a_check_is_its_own_task_and_the_check_files_nothing_of_its_own() {
        let board = Fake::default();
        let red = verify(&[("lint", 0), ("size-check", 1), ("tests", 101)]);
        let done = report(&board, &night(&[]), &[red]);
        assert_eq!(
            done.filed,
            [
                "tests/release-red-verify-size-check",
                "tests/release-red-verify-tests"
            ]
        );
        assert!(
            done.updated.is_empty() && done.closed.is_empty(),
            "{done:?}"
        );
        let filed = board
            .thread("tests/release-red-verify-size-check")
            .join("\n");
        for said in [
            "Release red: verify/size-check on cafe01234567\n",
            "The Symphony release run is red in `verify/size-check` on main at cafe01234567",
            "Command: `just size-check`, in the release run's clean checkout of that commit: exit 1.",
            "Log: /logs/verify/size-check.log\nLoad average when it ended: 1.50, 2.00, 2.50 (over \
             1, 5 and 15 minutes) on 8 processors.\n",
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): its log names no failed \
             test, so nothing narrower than its own command, `just size-check`, in a checkout of \
             cafe01234567.",
            "once `verify/size-check` is green",
            "key: verify-size-check\ncheck: verify/size-check",
        ] {
            assert!(filed.contains(said), "{said:?} in:\n{filed}");
        }
        assert!(!filed.contains("just verify"), "{filed}");
        assert!(!filed.contains("The open red of"), "none was open: {filed}");

        // Its size fixed, its tests still red: the one closed, the other told again.
        let done = report(
            &board,
            &night(&[]),
            &[verify(&[("lint", 0), ("size-check", 0), ("tests", 101)])],
        );
        assert_eq!(done.closed, ["tests/release-red-verify-size-check"]);
        assert_eq!(done.updated, ["tests/release-red-verify-tests"]);
        assert!(done.filed.is_empty(), "{done:?}");
        assert!(
            board.thread("tests/release-red-verify-size-check")[1]
                .contains("`verify/size-check` passed. Closed by the release run.")
        );
        assert!(board.thread("tests/release-red-verify-tests")[1].starts_with("Still red on "));

        // All green: the last one closed, and nothing for the check that had none.
        let done = report(
            &board,
            &night(&[]),
            &[verify(&[("lint", 0), ("size-check", 0), ("tests", 0)])],
        );
        assert_eq!(done.closed, ["tests/release-red-verify-tests"]);
        assert!(
            !board
                .tasks
                .borrow()
                .contains_key("tests/release-red-verify")
        );
    }

    #[test]
    fn an_open_red_of_the_check_and_each_red_step_name_each_other_and_it_closes_once_it_is_green() {
        // A whole-verify red filed before verify listed its steps, and a step's red already open.
        let board = Fake::with(&[
            ("tests/release-red-verify", "in_progress"),
            ("tests/release-red-verify-tests", "ready"),
        ]);
        let done = report(
            &board,
            &night(&[]),
            &[verify(&[("lint", 2), ("size-check", 0), ("tests", 101)])],
        );
        assert_eq!(done.filed, ["tests/release-red-verify-lint"]);
        assert_eq!(
            done.updated,
            ["tests/release-red-verify-tests", "tests/release-red-verify"]
        );
        assert_eq!(
            board.thread("tests/release-red-verify"),
            [
                "Red on cafe01234567 (run 2026-11-12T0300-1, 2026-11-12 03:00) in its steps alone, \
                 each a task of its own: `verify/lint` (exit 2) in tests/release-red-verify-lint, \
                 `verify/tests` (exit 101) in tests/release-red-verify-tests. Whoever holds this \
                 task reads those first, as one can be this same failure. This task closes once \
                 `verify` is green."
            ]
        );
        assert_eq!(board.status_of("tests/release-red-verify"), "in_progress");
        // The step filed now names the check's open red; the one already open heard of it above.
        let lint = board.thread("tests/release-red-verify-lint").join("\n");
        assert!(
            lint.contains(
                "\n\nThe open red of `verify` itself, tests/release-red-verify, was filed before \
                 this one: one filed before `verify` listed its steps, or for a red beyond them, \
                 can be this same failure, so whoever takes this reads its thread first, and the \
                 two are fixed as one where they are. That task hears of this one, and closes once \
                 `verify` is green.\n\nThe next run of `just release` closes this task once \
                 `verify/lint` is green;"
            ),
            "{lint}"
        );

        let done = report(&board, &night(&[]), &[verify(&[("lint", 0), ("tests", 0)])]);
        assert_eq!(
            done.closed,
            [
                "tests/release-red-verify-lint",
                "tests/release-red-verify-tests",
                "tests/release-red-verify"
            ]
        );
    }

    #[test]
    fn a_check_red_beyond_its_steps_is_filed_itself_beside_each_red_step() {
        let board = Fake::default();
        let stopped = Ran {
            timed_out: true,
            ..verify(&[("lint", 4)])
        };
        let done = report(&board, &night(&[]), &[stopped]);
        assert_eq!(
            done.filed,
            ["tests/release-red-verify-lint", "tests/release-red-verify"]
        );
        let filed = board.thread("tests/release-red-verify").join("\n");
        assert!(filed.contains("timed out: stopped after 1 s"), "{filed}");
        // Its log is its own lines and its steps' tails: its steps are named by the commands they
        // recorded, never verify.
        assert!(
            filed.contains(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), in a checkout of \
                 cafe01234567: each step it listed by the command it recorded, `just lint` (exit \
                 4, a task of its own); a step it did not reach, by the command its last lines \
                 name, a test run's each target alone, as `just test-rs -p PACKAGE --test TARGET` \
                 (`--lib` for a package's own tests), the one its last lines name, never the whole \
                 run again."
            ),
            "{filed}"
        );

        // Red with every step it listed green: the check, and no step.
        let board = Fake::default();
        let unexplained = Ran {
            exit_code: Some(1),
            ..verify(&[("lint", 0)])
        };
        let done = report(&board, &night(&[]), &[unexplained]);
        assert_eq!(done.filed, ["tests/release-red-verify"]);
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
        let migrate = "release-2026-11-12's `tasks migrate` in /work/alpha failed (exited 1)";
        let done = report(&board, &night(&[]), &[refresh_red("alpha", migrate)]);
        assert_eq!(done.filed, ["tests/release-red-refresh-alpha"]);
        let filed = board.thread("tests/release-red-refresh-alpha").join("\n");
        assert!(
            filed.starts_with(
                "Release red: the refresh of alpha to release-2026-11-12\nThe Symphony release run's \
                 the refresh of alpha to release-2026-11-12 is red (run 2026-11-12T0300-1, \
                 2026-11-12 03:00, the release of cafe01234567): release-2026-11-12's `tasks \
                 migrate` in /work/alpha failed"
            ),
            "{filed}"
        );
        // Its own work's red is fixed here; a project's writers to rebuild are never filed.
        assert!(
            filed.contains(
                "A red here is the refresh's own work that failed, fixed here; a board whose \
                 writers lag behind the release is that project's to finish and is never filed \
                 here."
            ),
            "{filed}"
        );
        // An orchestrator's rerun finishes it, not any executor's.
        assert!(
            filed.contains(
                "Then an orchestrator reruns the refresh, `just release --refresh` in the \
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
            !filed.contains("in the release run's clean checkout"),
            "{filed}"
        );

        // Red again: a line naming why. Green: closed.
        report(
            &board,
            &night(&[]),
            &[refresh_red("alpha", "still refused")],
        );
        let thread = board.thread("tests/release-red-refresh-alpha");
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
        assert_eq!(done.closed, ["tests/release-red-refresh-alpha"]);
        assert!(is_refresh("refresh") && is_refresh("refresh-x") && !is_refresh("refreshing"));
    }

    #[test]
    fn a_refresh_alone_never_closes_the_runs_red() {
        let board = Fake::with(&[("tests/release-red-run", "ready")]);
        let refresh_only = Night {
            whole: false,
            ..night(&[])
        };
        let done = report(&board, &refresh_only, &[]);
        assert_eq!(done, Board::default());
        assert_eq!(board.status_of("tests/release-red-run"), "ready");
        let done = report(&board, &night(&[]), &[]);
        assert_eq!(
            done.closed,
            ["tests/release-red-run"],
            "a run that got through does"
        );
    }

    #[test]
    fn a_checks_key_is_its_name_in_lower_case_words() {
        assert_eq!(key("verify"), "verify");
        assert_eq!(key("Lint lamp_shop!"), "lint-lamp-shop");
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
        assert_eq!(LEGACY_SERIES, symphony_board::red::SERIES[2]);
        assert!(symphony_board::red::filed(&format!(
            "{LEGACY_SERIES}{}",
            key("a/b c")
        )));
    }

    #[test]
    fn a_red_check_is_filed_once_and_a_second_red_night_updates_it() {
        let board = Fake::default();
        let checks = [ran("verify", 101), ran("lint", 0)];
        let done = report(&board, &night(&[]), &checks);
        assert_eq!(done.filed, ["tests/release-red-verify"]);
        assert!(done.updated.is_empty() && done.closed.is_empty() && done.error.is_none());
        assert_eq!(board.tasks.borrow().len(), 1, "a green check files nothing");
        let filed = board.thread("tests/release-red-verify").join("\n");
        assert!(
            filed.starts_with("Release red: verify on cafe01234567\n"),
            "{filed}"
        );
        assert!(
            filed.contains(
                "`just verify`, in the release run's clean checkout of that commit: exit 101."
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
        assert_eq!(done.updated, ["tests/release-red-verify"]);
        assert!(done.filed.is_empty());
        assert_eq!(board.tasks.borrow().len(), 1);
        let thread = board.thread("tests/release-red-verify");
        assert_eq!(thread.len(), 2);
        assert!(
            thread[1].starts_with(
                "Still red on cafe01234567 (run 2026-11-12T0300-1, 2026-11-12 03:00): exit 101."
            ),
            "{thread:?}"
        );
    }

    /// What only a red a commit fixes says: who takes it, how its fix lands and where it starts.
    const OF_A_FIX: [&str; 5] = [
        "any free executor takes this from the board",
        "What its fix needs to land",
        "Its own land closes this task",
        "start this task's branch",
        "a fix of this red starts from it",
    ];

    #[test]
    fn a_red_of_the_projects_rulesets_check_says_an_administrators_act_answers_it_never_a_fix() {
        let board = Fake::default();
        let mut named = night(&[]);
        named.rulesets = true;
        let done = report(&board, &named, &[ran("rulesets", 1), ran("verify", 1)]);
        assert_eq!(
            done.filed,
            ["tests/release-red-rulesets", "tests/release-red-verify"]
        );
        let filed = board.thread("tests/release-red-rulesets").join("\n");
        for said in [
            "Who fixes: whoever administers the repository on GitHub, not an executor with a \
             commit.",
            "the rules are put right on GitHub (docs/GITHUB.md, \"Who bypasses the rules\")",
            "that gh is signed in as someone who administers the repository",
            "No code is changed and no check loosened for it.",
            "puts it to the owner (`just tasks say SLUG TEXT --to owner`), then cancels the task \
             once it is put right, saying where",
            "The next run of `just release` closes this task once `rulesets` is green; cancelling \
             it once it is put right, saying where, answers it before that.",
        ] {
            assert!(filed.contains(said), "{said}\n\n{filed}");
        }
        for of_a_fix in OF_A_FIX {
            assert!(!filed.contains(of_a_fix), "{of_a_fix}\n\n{filed}");
        }
        // The check beside it is a fix's, as ever.
        let verify = board.thread("tests/release-red-verify").join("\n");
        assert!(verify.contains(OF_A_FIX[0]) && verify.contains(OF_A_FIX[1]));
        assert!(!verify.contains("administers"), "{verify}");
    }

    #[test]
    fn a_check_given_the_name_rulesets_where_the_project_names_none_is_any_checks_red() {
        let board = Fake::default();
        report(&board, &night(&[]), &[ran("rulesets", 1)]);
        let filed = board.thread("tests/release-red-rulesets").join("\n");
        assert!(filed.contains(OF_A_FIX[0]) && filed.contains(OF_A_FIX[1]));
        assert!(!filed.contains("administers"), "{filed}");
    }

    #[test]
    fn a_release_candidates_rulesets_red_starts_no_branch_and_is_answered_cancelled() {
        let board = Fake::with(&[("tests/release-red-rulesets", "in_progress")]);
        let mut held = night(&[]);
        held.rulesets = true;
        held.candidate = Some("cafe0123456789abcdef");
        // Still red: the open task hears where the candidate is, and that nothing starts there.
        report(&board, &held, &[ran("rulesets", 1)]);
        let again = &board.thread("tests/release-red-rulesets")[0];
        assert!(
            again.ends_with(
                "\n\nThe release candidate is cafe01234567, the branch `release-candidate`: no \
                 fix of this red starts there, as who administers the repository puts it right on \
                 GitHub."
            ),
            "{again}"
        );
        // Filed on the run that first holds the candidate.
        let board = Fake::default();
        report(&board, &held, &[ran("rulesets", 1)]);
        let filed = board.thread("tests/release-red-rulesets").join("\n");
        for said in [
            "The release candidate: this run holds cafe01234567 as the commit to release, the local \
             branch `release-candidate` (never pushed). This red asks for no branch and no fix \
             there.",
            "(the next landing starts it, or `just release --rerun`), which runs `rulesets` again.",
            "Who fixes: whoever administers the repository on GitHub",
            "Cancelling this task once it is put right, saying where, answers it for the release \
             candidate, whose rerun runs `rulesets` again; a rerun of the candidate green in \
             `rulesets` closes it too.",
        ] {
            assert!(filed.contains(said), "{said}\n\n{filed}");
        }
        for of_a_fix in OF_A_FIX {
            assert!(!filed.contains(of_a_fix), "{of_a_fix}\n\n{filed}");
        }
        assert!(!filed.contains("The next run of `just release` closes"));
    }

    #[test]
    fn a_release_candidates_red_says_how_its_fix_starts_merges_and_lands() {
        let board = Fake::with(&[("tests/release-red-lint", "in_progress")]);
        let mut held = night(&[]);
        held.candidate = Some("cafe0123456789abcdef");
        let checks = [ran("verify", 101), ran("lint", 1), ran("docs", 0)];
        let done = report(&board, &held, &checks);
        assert_eq!(done.filed, ["tests/release-red-verify"]);
        assert_eq!(done.updated, ["tests/release-red-lint"]);
        let red = |key: &str| Held {
            key: key.into(),
            name: key.into(),
            slug: format!("tests/release-red-{key}"),
        };
        assert_eq!(
            done.reds,
            vec![red("verify"), red("lint")],
            "each red's task"
        );
        let filed = board.thread("tests/release-red-verify").join("\n");
        for said in [
            "The release candidate: this run holds cafe01234567 as the commit to release, the local \
             branch `release-candidate` (never pushed), so the release is that commit and its \
             reds' fixes alone",
            "start this task's branch, named for its slug, at `release-candidate`, never at main; \
             merge `release-candidate` again before review",
            "never merge main before its land",
            "commit the distribution its own sources make (`just client-update`)",
            "A land that conflicts comes back to its executor: merge main then and ask for a fresh \
             approve, and the candidate takes the commit approved before that merge; a fix that \
             needed no review has none, and drops it.",
            "or, where it needed no review, the tip its landing's line says its needed stamps \
             stood on.",
            "A fix that landed with another red's task answers this red by the task closed as \
             covered by that landing (`just tasks done SLUG NOTE --covered-by OTHER`), once that \
             land advances the candidate; closed as covered by any other landing, its fix is on \
             main and not in the candidate, which is dropped.",
            "A fix made outside this repository answers this red by the task cancelled, saying \
             where.",
            "(the next landing starts it, or `just release --rerun`)",
            "Its own land closes this task and answers it for the release candidate; a rerun of \
             the candidate green in `verify` closes it too.",
        ] {
            assert!(filed.contains(said), "{said}\n\n{filed}");
        }
        assert!(
            !filed.contains("The next run of `just release` closes"),
            "{filed}"
        );
        let again = &board.thread("tests/release-red-lint")[0];
        assert!(
            again.ends_with(
                "\n\nThe release candidate is cafe01234567, the branch `release-candidate`: a fix \
                 of this red starts from it, or merges it before review, and never merges main \
                 before the approve."
            ),
            "{again}"
        );
        assert!(
            filed.contains("\nThe Symphony release run is red in `verify` on main at cafe01234567"),
            "the run that first holds it ran main's tip: {filed}"
        );

        // A rerun of the candidate red again says what it checked: the candidate, not main.
        let board = Fake::default();
        let rerun = Night {
            rerun: true,
            ..held
        };
        report(&board, &rerun, &checks);
        let filed = board.thread("tests/release-red-verify").join("\n");
        assert!(
            filed.contains(
                "\nThe Symphony release run is red in `verify` on the release candidate at \
                 cafe01234567 (run "
            ),
            "{filed}"
        );

        // A red run that holds no candidate says none of it.
        let board = Fake::default();
        let done = report(&board, &night(&[]), &checks);
        assert_eq!(done.reds.len(), 2);
        let filed = board.thread("tests/release-red-verify").join("\n");
        assert!(!filed.contains("release candidate"), "{filed}");
        assert!(
            filed.contains(
                "The next run of `just release` closes this task once `verify` is green; a fix \
                 released out of cycle (`just release --commit COMMIT`) closes it too."
            ),
            "{filed}"
        );
        assert!(!board.thread("tests/release-red-lint")[0].contains("candidate"));
    }

    #[test]
    fn a_green_check_closes_its_open_red_and_a_later_red_files_the_next_generation() {
        let board = Fake::with(&[("tests/release-red-verify", "in_progress")]);
        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/release-red-verify"]);
        assert_eq!(board.status_of("tests/release-red-verify"), "cancelled");
        assert!(
            board.thread("tests/release-red-verify")[0].starts_with("Green on cafe01234567 in run")
        );

        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert!(done.closed.is_empty(), "nothing open to close: {done:?}");

        let done = report(&board, &night(&[]), &[ran("verify", 2)]);
        assert_eq!(done.filed, ["tests/release-red-verify-2"]);
        assert_eq!(
            board.status_of("tests/release-red-verify"),
            "cancelled",
            "a closed red stays closed"
        );
    }

    #[test]
    fn the_open_generation_is_the_one_a_night_writes_to() {
        let board = Fake::with(&[
            ("tests/release-red-verify", "cancelled"),
            ("tests/release-red-verify-2", "done"),
            ("tests/release-red-verify-3", "review"),
        ]);
        let done = report(&board, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.updated, ["tests/release-red-verify-3"]);
        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/release-red-verify-3"]);
    }

    #[test]
    fn an_open_red_a_build_older_than_the_rename_filed_is_updated_and_closed_never_orphaned() {
        let board = Fake::with(&[
            ("tests/nightly-red-verify", "cancelled"),
            ("tests/nightly-red-verify-2", "in_progress"),
            ("tests/nightly-red-lint", "review"),
            ("tests/nightly-red-run", "ready"),
            ("tests/nightly-red-docs", "done"),
        ]);
        // Red again: its old task hears of it, and nothing is filed beside it.
        let done = report(&board, &night(&[]), &[ran("verify", 1), ran("lint", 0)]);
        assert_eq!(done.updated, ["tests/nightly-red-verify-2"]);
        assert_eq!(
            done.closed,
            ["tests/nightly-red-lint", "tests/nightly-red-run"]
        );
        assert!(done.filed.is_empty() && done.error.is_none(), "{done:?}");
        assert!(
            board.thread("tests/nightly-red-verify-2")[0].starts_with(
                "Still red on cafe01234567 (run 2026-11-12T0300-1, 2026-11-12 03:00): exit 1."
            ),
            "{:?}",
            board.thread("tests/nightly-red-verify-2")
        );
        assert_eq!(board.status_of("tests/nightly-red-lint"), "cancelled");
        assert!(board.thread("tests/nightly-red-lint")[0].starts_with("Green on cafe01234567"));
        assert_eq!(board.status_of("tests/nightly-red-run"), "cancelled");
        // Green: it closes. Red once more: the new series' first, the old one being all closed.
        let done = report(&board, &night(&[]), &[ran("verify", 0)]);
        assert_eq!(done.closed, ["tests/nightly-red-verify-2"]);
        let done = report(&board, &night(&[]), &[ran("verify", 1), ran("docs", 1)]);
        assert_eq!(
            done.filed,
            ["tests/release-red-verify", "tests/release-red-docs"]
        );
        assert_eq!(board.status_of("tests/nightly-red-docs"), "done");
    }

    #[test]
    fn a_red_another_run_filed_first_is_updated_never_filed_twice() {
        let board = Fake {
            raced: Some("tests/release-red-verify"),
            ..Fake::default()
        };
        let done = report(&board, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.updated, ["tests/release-red-verify"]);
        assert!(done.filed.is_empty() && done.error.is_none(), "{done:?}");
    }

    #[test]
    fn what_the_board_refuses_is_said_and_the_other_checks_still_go() {
        let board = Fake {
            refuse: vec!["create"],
            ..Fake::with(&[("tests/release-red-lint", "ready")])
        };
        let done = report(&board, &night(&[]), &[ran("verify", 1), ran("lint", 0)]);
        assert_eq!(done.closed, ["tests/release-red-lint"]);
        assert_eq!(
            done.error.as_deref(),
            Some("tests/release-red-verify: create refused")
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
        // A check red in its steps alone looks for an open red of its own too.
        let done = report(&blind, &night(&[]), &[verify(&[("lint", 1)])]);
        assert!(done.filed.is_empty() && done.updated.is_empty());
        assert_eq!(
            done.error.as_deref(),
            Some("status refused; status refused; status refused"),
            "the check's, its step's and the run's"
        );

        let unleaving = Fake {
            refuse: vec!["leave"],
            ..Fake::default()
        };
        let done = report(&unleaving, &night(&[]), &[ran("verify", 1)]);
        assert_eq!(done.filed, ["tests/release-red-verify"]);
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
            Some("tests/release-red-verify: 1000 generations, all closed")
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
        let body = board.thread("tests/release-red-verify").join("\n");
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

    /// A red check's task names the commits it came between on a line of its own, the last green
    /// run's and the candidate, in full, which the board reads back as the red's range; none
    /// without a green run or a candidate.
    #[test]
    fn a_red_task_names_the_commits_it_came_between_for_the_roll_up() {
        let board = Fake::default();
        report(&board, &night(&[]), &[ran("verify", 1)]);
        let filed = board.thread("tests/release-red-verify").join("\n");
        assert!(
            filed.contains("\nbetween: beef0123456789abcdef..cafe0123456789abcdef\nkey: verify\n"),
            "{filed}"
        );
        assert_eq!(
            symphony_board::red::between(&filed),
            Some("beef0123456789abcdef..cafe0123456789abcdef")
        );
        let mut first = night(&[]);
        first.green = None;
        let mut early = night(&[]);
        early.commit = "";
        for (night, why) in [(first, "no green run"), (early, "no candidate")] {
            let said = body(&ran("verify", 1), &night, None);
            assert!(!said.contains("between: "), "{why}: {said}");
            assert_eq!(symphony_board::red::between(&said), None, "{why}");
        }
    }

    /// A verify log in which cargo named failed targets: `count` tests of the lib failing, and an
    /// integration target that crashed naming none.
    fn cargo_log(tree: &crate::test_tree::Tree, count: usize) -> PathBuf {
        let mut text = String::from("running 9 tests\n");
        for n in 0..count {
            text.push_str(&format!("test store::case_{n} ... FAILED\n"));
        }
        text.push_str(
            "error: test failed, to rerun pass `-p symphony-board --lib`\n\
             running 1 test\n\
             error: test failed, to rerun pass `-p symphony --test nightly`\n",
        );
        tree.write("verify.log", &text)
    }

    #[test]
    fn a_red_task_says_the_load_its_check_ended_under_and_how_to_rerun_each_failure_alone() {
        let tree = crate::test_tree::Tree::new();
        let red = Ran {
            log: cargo_log(&tree, 2),
            load: Some(super::super::checks::Load {
                averages: [31.92, 70.68, 171.07],
                processors: 10,
            }),
            ..ran("verify", 101)
        };
        let board = Fake::default();
        report(&board, &night(&[]), std::slice::from_ref(&red));
        let filed = board.thread("tests/release-red-verify").join("\n");
        for said in [
            format!(
                "Log: {}\nLoad average when it ended: 31.92, 70.68, 171.07 (over 1, 5 and 15 \
                 minutes) on 10 processors.\n",
                red.log.display()
            ),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), in a checkout of \
             cafe01234567, as its log names each failure (3):\n\
             - `just test-rs -p symphony-board --lib -- store::case_0 --exact`\n\
             - `just test-rs -p symphony-board --lib -- store::case_1 --exact`\n\
             - `just test-rs -p symphony --test nightly`\n\n"
                .into(),
            format!("Who fixes: any free executor takes this from the board. {FIRST} {LANDS}"),
            // A test that passed alone is no red; one red on load all the same is discarded.
            "the code fixed, never a test or check loosened. A test the run reran alone that \
             passed is no red, and has a task of its own; a test red here failed alone too, or was \
             not rerun. One red on the machine's load all the same (it fails on a wall-clock \
             bound, a sleep-ordered step or a timeout under load) is discarded in this fix \
             instead, as RULES.md `tests/no-test-fails-on-load` says, with one line on this \
             thread naming it, the evidence it is load and what it proved."
                .into(),
        ] {
            assert!(filed.contains(&said), "{said:?} in:\n{filed}");
        }

        // Red again: the line says the load and the reruns of this run.
        let later = Ran {
            log: cargo_log(&tree, 1),
            load: None,
            ..red
        };
        report(&board, &night(&[]), &[later]);
        let again = &board.thread("tests/release-red-verify")[1];
        assert!(
            again.contains("\nLoad average when it ended: not recorded.\n\nRerun alone"),
            "{again}"
        );
        assert!(
            again.contains("(2):\n- `just test-rs -p symphony-board --lib -- store::case_0 --exact`\n- `just test-rs -p symphony --test nightly`"),
            "{again}"
        );
        assert!(
            !board
                .thread("tests/release-red-verify")
                .join("\n")
                .contains("slept"),
            "a run that did not sleep says nothing of one"
        );
    }

    #[test]
    fn a_red_of_a_run_the_machine_slept_under_says_for_how_long_beside_its_load() {
        let tree = crate::test_tree::Tree::new();
        let red = Ran {
            log: cargo_log(&tree, 1),
            load: Some(super::super::checks::Load {
                averages: [50.0, 68.0, 62.0],
                processors: 16,
            }),
            ..ran("verify", 101)
        };
        let slept = Night {
            slept: 1589.0,
            ..night(&[])
        };
        let board = Fake::default();
        report(&board, &slept, std::slice::from_ref(&red));
        // Red again, on a later run that slept too: its line says so as well.
        report(&board, &slept, std::slice::from_ref(&red));
        let said = "Load average when it ended: 50.00, 68.00, 62.00 (over 1, 5 and 15 minutes) on \
                    16 processors.\nThe machine slept 26 min under this run, which its times \
                    count: a wait or a timeout that failed across that sleep failed on the sleep, \
                    not on load.\n\n";
        let thread = board.thread("tests/release-red-verify");
        assert_eq!(thread.len(), 2, "{thread:?}");
        for post in &thread {
            assert!(post.contains(said), "{said:?} in:\n{post}");
        }
    }

    #[test]
    fn a_red_step_of_verify_is_rerun_by_its_own_command_and_never_by_verify() {
        let tree = crate::test_tree::Tree::new();
        // A step's red reads its own log: the tests' names each failure, any other its command.
        let tests = Ran {
            command: "tools/cargo.sh test --workspace --locked --no-fail-fast".into(),
            log: cargo_log(&tree, 1),
            ..ran("verify/tests", 101)
        };
        assert!(
            alone(&tests, &night(&[])).ends_with(
                "(2):\n- `just test-rs -p symphony-board --lib -- store::case_0 --exact`\n- `just \
                 test-rs -p symphony --test nightly`"
            ),
            "{}",
            alone(&tests, &night(&[]))
        );
        let lint = Ran {
            command: "just lint-rs".into(),
            log: tree.write("lint-step.log", "error: recipe `_lint-rs` failed\n"),
            ..ran("verify/lint", 1)
        };
        assert_eq!(
            alone(&lint, &night(&[])),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): its log names no failed test, \
             so nothing narrower than its own command, `just lint-rs`, in a checkout of \
             cafe01234567."
        );
        // A verify that listed no steps, as an older one: today's size budget, red, where the
        // recipe nearest verify names the step.
        let size = Ran {
            log: tree.write(
                "size.log",
                "size-budget: symphony is 18.8 MiB, 0.5 MiB past its budget\n\
                 error: recipe `_size-check` failed on line 101 with exit code 1\n\
                 error: recipe `size-check` failed on line 96 with exit code 1\n\
                 error: recipe `verify` failed on line 105 with exit code 1\n",
            ),
            ..ran("verify", 1)
        };
        assert_eq!(
            alone(&size, &night(&[])),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), in a checkout of \
             cafe01234567, the step its log names failed: `just size-check`."
        );
        // The docs audit's recipe says nothing when it fails: the log names no step, and verify's
        // steps are listed, never verify itself.
        let docs = Ran {
            log: tree.write(
                "docs.log",
                "✖ docs/TASKS.md: a broken link\n\
                 error: recipe `_verify` failed on line 109 with exit code 1\n\
                 error: recipe `verify` failed on line 105 with exit code 1\n",
            ),
            ..ran("verify", 1)
        };
        let said = alone(&docs, &night(&[]));
        assert_eq!(
            said,
            format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), in a checkout of \
                 cafe01234567: its log names no failed test or step, so its last lines say which \
                 of verify's steps failed, and that one alone reruns it, by its own recipe (`just \
                 RECIPE`), or its tests, {EACH_TARGET}."
            )
        );
        assert!(!said.contains("just verify"), "{said}");
        // A test run whose log names no failed target (it did not build): a target at a time,
        // never the whole suite again.
        let unbuilt = Ran {
            command: "SYMPHONY_UI_DIST=\"$PWD/build/verified-client/public\" tools/cargo.sh \
                      test --workspace --locked --no-fail-fast"
                .into(),
            log: tree.write("unbuilt.log", "error[E0425]: cannot find value `x`\n"),
            ..ran("verify/tests", 101)
        };
        assert_eq!(
            alone(&unbuilt, &night(&[])),
            format!(
                "Rerun alone (RULES.md `tests/rerun-the-failure-alone`), in a checkout of \
                 cafe01234567: its log names no failed test target, so {EACH_TARGET}."
            )
        );
        // Nor when an older verify's log names its tests' recipe failed.
        let suite = Ran {
            log: tree.write(
                "suite.log",
                "error[E0425]: cannot find value `x`\n\
                 error: recipe `test-rs` failed on line 40 with exit code 101\n\
                 error: recipe `verify` failed on line 105 with exit code 101\n",
            ),
            ..ran("verify", 101)
        };
        assert_eq!(
            alone(&suite, &night(&[])),
            alone(&unbuilt, &night(&[])),
            "a target at a time"
        );
        // Nor when the tests ran in lanes, the tests step's command since.
        let lanes = Ran {
            command: "SYMPHONY_UI_DIST=\"$PWD/build/verified-client/public\" tools/test-lanes.sh"
                .into(),
            ..unbuilt.clone()
        };
        assert_eq!(alone(&lanes, &night(&[])), alone(&unbuilt, &night(&[])));
        // A red lane that listed no target of its own is rerun whole, by the tests it named failed
        // when it named any, and the run's list names it.
        let dead = Ran {
            command: "tools/test-lanes.sh".into(),
            log: tree.write(
                "lanes-dead.log",
                include_str!("fixtures/tests-lanes-dead.log"),
            ),
            ..ran("verify/tests", 137)
        };
        assert!(
            alone(&dead, &night(&[])).ends_with(
                "(3):\n- `just test-rs --workspace --test sweep -- land::a --exact`\n- `just \
                 test-rs --workspace --test browser`\n- `just test-rs -p symphony-board --test cli`"
            ),
            "{}",
            alone(&dead, &night(&[]))
        );
        assert!(
            quoted(&dead)
                .contains("    `--workspace --test sweep`\n    `--workspace --test browser`\n"),
            "{}",
            quoted(&dead)
        );
        assert!(runs_tests("just test-rs -p symphony") && runs_tests("cargo test --lib"));
        assert!(runs_tests("tools/test-lanes.sh") && runs_tests("test-lanes.sh"));
        assert!(!runs_tests("just size-check") && !runs_tests("cargo build --tests"));
        assert!(!runs_tests("tools/verify.sh") && !runs_tests("tools/test-lanes.sh.orig"));
        assert!(!runs_tests("tools/my-test-lanes.sh"));
        // A verify that listed its steps never reads its own log, whose tails can quote a step's
        // recipe or test: its steps are each a task of their own.
        let quoting = Ran {
            log: tree.write(
                "quoting.log",
                "verify: size-check: red, exit 1 (3 s); its last lines:\n    error: recipe \
                 `size-check` failed on line 96 with exit code 1\n    error: test failed, to rerun \
                 pass `-p symphony --lib`\n",
            ),
            timed_out: true,
            ..verify(&[("size-check", 1)])
        };
        assert!(
            alone(&quoting, &night(&[])).contains(
                ": each step it listed by the command it recorded, `just size-check` (exit 1, a \
                 task of its own);"
            ),
            "{}",
            alone(&quoting, &night(&[]))
        );
        // Another check is rerun by its own command.
        let other = Ran {
            command: "sh -c 'exit 1'".into(),
            log: tree.write("other.log", "error: could not compile `symphony` (lib)\n"),
            ..ran("other", 101)
        };
        assert_eq!(
            alone(&other, &night(&[])),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): its log names no failed \
             test, so nothing narrower than its own command, `sh -c 'exit 1'`, in a checkout of \
             cafe01234567."
        );
    }

    #[test]
    fn a_log_naming_many_failed_tests_is_cut() {
        let tree = crate::test_tree::Tree::new();
        let many = Ran {
            log: cargo_log(&tree, RERUNS_LISTED + 3),
            ..ran("verify", 101)
        };
        let said = alone(&many, &night(&[]));
        assert_eq!(said.lines().count(), 1 + RERUNS_LISTED + 1, "{said}");
        assert!(
            said.ends_with(&format!(
                "- `just test-rs -p symphony-board --lib -- store::case_{} --exact`\n\
                 - … and 4 more in the log",
                RERUNS_LISTED - 1
            )),
            "{said}"
        );
    }

    /// A test run's log, cut, whose own test (`dependency_boundary`) emptied its build folder
    /// `target` while it ran: the targets after it found their executable gone, or a
    /// build script's output. When `failed`, one before it failed in a test of its own; else that
    /// one ran after it and found its executable gone too.
    fn emptied_log(
        tree: &crate::test_tree::Tree,
        target: &std::path::Path,
        failed: bool,
    ) -> PathBuf {
        let target = target.display();
        let unexecuted = |name: &str| {
            format!(
                "     Running tests/{name}.rs (.target/debug/deps/{name}-c2)\n\
                 error: test failed, to rerun pass `-p app --test {name}`\n\n\
                 Caused by:\n  could not execute process `{target}/debug/deps/{name}-c2` (never \
                 executed)\n\nCaused by:\n  No such file or directory (os error 2)\n"
            )
        };
        let boundary = "     Running tests/dependency_boundary.rs \
                        (.target/debug/deps/dependency_boundary-c5)\n\
                        test wrappers_build_without_the_symphony_app_or_board ... ok\n";
        let (before, after) = if failed {
            (
                "     Running tests/golden_card.rs (.target/debug/deps/golden_card-87ae)\n\
                 test shared_app_discovers_branch_card ... FAILED\n\
                 error: test failed, to rerun pass `-p app --test golden_card`\n"
                    .to_string(),
                String::new(),
            )
        } else {
            (String::new(), unexecuted("golden_card"))
        };
        tree.write(
            "tests.log",
            &format!(
                "{before}{boundary}{after}{failure_capture}   Doc-tests content_index\n\
                 error: couldn't find file `{target}/debug/build/content-index-c3/out/index.rs`\n\
                 error: doctest failed, to rerun pass `-p app --doc`\n",
                failure_capture = unexecuted("failure_capture"),
            ),
        )
    }

    #[test]
    fn a_test_run_whose_build_lost_files_is_filed_as_the_environment_failure_it_is() {
        let tree = crate::test_tree::Tree::new();
        let target = tree.path("checkout/.target");
        tree.write("checkout/.target/tree", "the tree that emptied it");
        let tests = Ran {
            command: "tools/cargo.sh test --workspace --locked --no-fail-fast".into(),
            log: emptied_log(&tree, &target, true),
            ..ran("tests", 101)
        };
        let board = Fake::default();
        report(&board, &night(&[]), std::slice::from_ref(&tests));
        let filed = board.thread("tests/release-red-tests").join("\n");
        let paragraph = format!(
            "\n\nAn environment failure, not a failed test: 2 of its failed targets found files \
             its own build had made gone, and never ran or never compiled for that (`-p app --test \
             failure_capture` and `-p app --doc`): `{target}/debug/deps/failure_capture-c2` and \
             `{target}/debug/build/content-index-c3/out/index.rs`. They went while `Running \
             tests/dependency_boundary.rs (.target/debug/deps/dependency_boundary-c5)` (`-p app \
             --test dependency_boundary`) ran, the last target cargo started before the first \
             that found one gone: what removed them ran then, that target or something it \
             started, or something outside the run. Its build folder, {target}, holds now: \
             `tree` (made ",
            target = target.display()
        );
        assert!(filed.contains(&paragraph), "{paragraph:?} in:\n{filed}");
        assert!(
            filed.contains(
                "and no `debug/`.\n\nRerun alone (RULES.md `tests/rerun-the-failure-alone`), in \
                 a checkout of cafe01234567, as its log names each failure (1):\n- `just test-rs \
                 -p app --test golden_card -- shared_app_discovers_branch_card --exact`\n\n"
            ),
            "the targets that found files gone are no failures to rerun:\n{filed}"
        );

        // Red again with every target failed so: the environment's again, and no rerun but the
        // target they went while, with a look at the build folder; with no failure before it the
        // log says no package for it, so its line names it.
        let gone = Ran {
            log: emptied_log(&tree, &target, false),
            ..tests
        };
        report(&board, &night(&[]), std::slice::from_ref(&gone));
        let again = &board.thread("tests/release-red-tests")[1];
        assert!(
            again.contains(
                "\n\nAn environment failure, not a failed test: 3 of its failed targets found"
            ),
            "{again}"
        );
        assert!(
            again.contains(
                "and no `debug/`.\n\nRerun alone (RULES.md `tests/rerun-the-failure-alone`): none \
                 of its failures is a test's to rerun, as each failed target found files of its \
                 build gone (above). What removes them shows in a run of the target they went \
                 while, which `Running tests/dependency_boundary.rs \
                 (.target/debug/deps/dependency_boundary-c5)` names, in a checkout of \
                 cafe01234567, with a look at its build folder after it.\n\n"
            ),
            "{again}"
        );

        // Gone before any target ran, in a check of main's own: its build is what to rerun.
        let unbuilt = Ran {
            command: "tools/cargo.sh test --workspace --locked --no-fail-fast".into(),
            log: tree.write(
                "unbuilt.log",
                "error: extern location for a does not exist: /t/debug/deps/liba-1.rlib\n\
                 error: test failed, to rerun pass `-p symphony --lib`\n",
            ),
            ..ran("verify/tests", 101)
        };
        assert_eq!(
            alone(&unbuilt, &night(&[])),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): none of its failures is a \
             test's to rerun, as each failed target found files of its build gone (above). What \
             removes them shows in a run of its build, in a checkout of cafe01234567, with a look \
             at its build folder after it."
        );
        assert!(
            environment(&unbuilt).ends_with("Its build folder, /t, is gone.\n\n"),
            "{}",
            environment(&unbuilt)
        );
        // Gone while the tests' lanes ran side by side: no one target is named or rerun, but each
        // of those lanes alone, never the target before them in the log's order (another lane's).
        let lanes = Ran {
            command: "tools/test-lanes.sh".into(),
            log: tree.write(
                "lanes-gone.log",
                include_str!("fixtures/tests-lanes-gone.log"),
            ),
            ..ran("verify/tests", 101)
        };
        assert_eq!(
            alone(&lanes, &night(&[])),
            "Rerun alone (RULES.md `tests/rerun-the-failure-alone`): none of its failures is a \
             test's to rerun, as each failed target found files of its build gone (above). What \
             removes them shows in a run of each of the lanes they went while, alone and one after \
             another, by the targets the log names for it (`just test-rs --workspace TARGETS`), in \
             a checkout of cafe01234567, with a look at its build folder after each."
        );
        let said = environment(&lanes);
        assert!(
            said.contains(
                "They went while the tests' 4 lanes ran side by side, all at once (`sweep`, \
                 `browser`, `rest` and `doc`): what removed them ran then, a test of any of those \
                 lanes or something it started"
            ),
            "{said}"
        );
        assert!(!said.contains("tests/browser"), "{said}");
        // A check whose targets failed by themselves says nothing of the environment.
        assert_eq!(environment(&ran("verify", 101)), "");
    }

    #[test]
    fn a_red_task_quotes_the_end_of_its_log_when_it_names_no_failure() {
        let tree = crate::test_tree::Tree::new();
        let lines: String = (0..60).map(|n| format!("line {n}\n")).collect();
        let log = tree.write("verify.log", &lines);
        let red = Ran {
            log,
            ..ran("verify", 1)
        };
        let said = quoted(&red);
        assert!(
            said.starts_with("Its last lines:\n\n```\nline 20\n"),
            "{said}"
        );
        assert!(said.ends_with("line 59\n```"), "{said}");
        assert_eq!(
            quoted(&ran("verify", 1)),
            "Its log is empty or cannot be read."
        );
        // A step stopped at the run's time while it built says no failure of its own: its whole
        // log, which is shorter than the tail, is its last lines.
        let stopped = include_str!("fixtures/no-failure.log");
        let red = Ran {
            log: tree.write("size-check.log", stopped),
            ..ran("verify/size-check", 1)
        };
        assert_eq!(
            quoted(&red),
            format!("Its last lines:\n\n```\n{}\n```", stopped.trim_end())
        );
        // So does a test run stopped at the run's time before a test failed: nothing its passing
        // tests printed (a refusal, a recipe failed, a file missing) is named as what failed.
        let stopped = include_str!("fixtures/tests-stopped.log");
        let red = Ran {
            log: tree.write("tests.log", stopped),
            ..ran("verify/tests", 1)
        };
        assert_eq!(
            quoted(&red),
            format!("Its last lines:\n\n```\n{}\n```", stopped.trim_end())
        );
        // A line of the log that is a fence of its own does not close the quote's.
        let red = Ran {
            log: tree.write("fence.log", "a\n```\nb\n"),
            ..ran("verify", 1)
        };
        assert_eq!(quoted(&red), "Its last lines:\n\n````\na\n```\nb\n````");
    }

    /// The fence after `heading` in `body`, its lines.
    fn fenced<'a>(body: &'a str, heading: &str) -> Vec<&'a str> {
        let (_, rest) = (body.split_once(&format!("{heading}\n\n```\n")))
            .unwrap_or_else(|| panic!("no {heading:?} in:\n{body}"));
        rest.split_once("\n```").unwrap().0.lines().collect()
    }

    #[test]
    fn a_red_steps_task_names_what_failed_before_a_few_of_its_last_lines() {
        let tree = crate::test_tree::Tree::new();
        let tests = include_str!("fixtures/tests-red.log");
        let client = include_str!("fixtures/client-check-red.log");
        let mut red = verify(&[("client-check", 1), ("tests", 101)]);
        red.steps[0].log = tree.write("client-check.log", client);
        red.steps[1].log = tree.write("tests.log", tests);
        let board = Fake::default();
        report(&board, &night(&[]), &[red]);

        let filed = board.thread("tests/release-red-verify-tests").join("\n");
        let named = fenced(&filed, "What failed, as its log says it:");
        assert_eq!(
            named.first(),
            Some(
                &"test tests::a_real_page_blocked_in_its_renderer_spends_nothing_and_a_stopped_\
                  browser_ends_at_the_backstop ... FAILED"
            ),
            "{filed}"
        );
        assert_eq!(
            named[named.len() - 3..],
            [
                "error: 2 targets failed:",
                "    `-p chrome-devtools --lib`",
                "    `-p symphony --lib`"
            ]
        );
        assert_eq!(
            named.len(),
            3 * 2 + 3,
            "three tests, each where it panicked, and the list"
        );
        // Then a few of its last lines, and the reruns.
        let last = fenced(&filed, "Its last lines:");
        assert_eq!(last.len(), SHORT_TAIL_LINES, "{filed}");
        assert_eq!(
            last.last(),
            Some(
                &"verify: the client check is red: the tests ran against the client it built from \
                  this tree, /work/symphony-wt/release/build/verified-client/public, not the \
                  committed one"
            )
        );
        let at = |what: &str| {
            filed
                .find(what)
                .unwrap_or_else(|| panic!("{what}: {filed}"))
        };
        assert!(at("Load average when it ended") < at("What failed"));
        assert!(at("What failed") < at("Its last lines"));
        assert!(at("Its last lines") < at("Rerun alone"));

        let filed = board
            .thread("tests/release-red-verify-client-check")
            .join("\n");
        assert_eq!(
            fenced(&filed, "What failed, as its log says it:"),
            [
                "client-check: rebuild and commit the client asset: index.html",
                "error: recipe `_client-check` failed on line 98 with exit code 1",
                "error: recipe `client-check` failed on line 93 with exit code 1",
            ],
            "{filed}"
        );
        assert_eq!(fenced(&filed, "Its last lines:").len(), SHORT_TAIL_LINES);

        // Red again: its line names what failed this time too.
        let mut again = verify(&[("client-check", 1)]);
        again.steps[0].log = tree.write("client-check-2.log", "client-check: refused x\n");
        report(&board, &night(&[]), &[again]);
        let line = &board.thread("tests/release-red-verify-client-check")[1];
        assert_eq!(
            fenced(line, "What failed, as its log says it:"),
            ["client-check: refused x"],
            "{line}"
        );
    }

    #[test]
    fn a_check_that_listed_its_steps_quotes_its_own_log_by_its_last_lines() {
        // Its log is its own lines and its steps' tails, whose errors are its steps' tasks' to name.
        let tree = crate::test_tree::Tree::new();
        let red = Ran {
            log: tree.write(
                "verify.log",
                "verify: tests: red, exit 101 (3 s); its last lines:\n    error: 2 targets \
                 failed:\n        `-p symphony --lib`\nerror: recipe `_verify` failed on line 120\n",
            ),
            ..verify(&[("tests", 101)])
        };
        let said = quoted(&red);
        assert!(
            said.starts_with("Its last lines:\n\n```\nverify: tests: red"),
            "{said}"
        );
        assert!(!said.contains("What failed"), "{said}");
        // A refresh's red names what its log says failed as a check's does.
        let refresh = Ran {
            command: "refresh consumer".into(),
            log: tree.write(
                "refresh.log",
                "$ /m/releases/r/bin/tasks migrate\nthe migrate was refused\n",
            ),
            ..ran("refresh-consumer", 1)
        };
        let filed = refresh_body(&refresh, &night(&[]));
        assert_eq!(
            fenced(&filed, "What failed, as its log says it:"),
            ["the migrate was refused"],
            "{filed}"
        );
        assert!(
            filed.contains("`current`, which a red refresh never points")
                && filed.contains("No project's checkout is written.")
                && !filed.contains("branch"),
            "{filed}"
        );
    }

    /// The tests step `verify/tests` red, its failures rerun alone, each `(ARGS, CODE)`, or none
    /// rerun for `why_not`.
    fn tests_reran(reruns: &[(&str, i32)], why_not: Option<&str>) -> Ran {
        let load = Some(super::super::checks::Load {
            averages: [31.92, 70.68, 171.07],
            processors: 16,
        });
        let reruns = (reruns.iter())
            .enumerate()
            .map(|(n, (args, code))| Ran {
                seconds: 3.0,
                log: PathBuf::from(format!("/logs/verify-tests.alone/{}.log", n + 1)),
                load: load.clone(),
                ..ran(args, *code)
            })
            .collect();
        Ran {
            command: "tools/test-lanes.sh".into(),
            log: PathBuf::from("/logs/verify.steps/tests.log"),
            load,
            alone: Some(super::super::alone::Alone {
                reruns,
                why_not: why_not.map(str::to_string),
            }),
            ..ran("verify/tests", 100)
        }
    }

    #[test]
    fn a_step_whose_failures_each_passed_alone_files_no_red_and_one_task_for_them_at_p1() {
        let board = Fake::with(&[("tests/release-red-verify-tests", "ready")]);
        let passed = tests_reran(&[("-p a --lib -- x --exact", 0), ("-p b --lib", 0)], None);
        let verify = Ran {
            steps: vec![passed],
            ..ran("verify", 1)
        };
        let done = report(&board, &night(&[]), &[verify]);
        let slug = "tests/passed-alone-2026-11-12t0300-1";
        assert_eq!(done.filed, [slug], "{done:?}");
        assert_eq!(
            done.closed,
            ["tests/release-red-verify-tests"],
            "a step green as judged closes its red"
        );
        assert!(done.reds.is_empty(), "{done:?}");
        let filed = board.thread(slug).join("\n");
        let load = "load 31.92, 70.68, 171.07 (over 1, 5 and 15 minutes) on 16 processors";
        for said in [
            "p1, no red: Passed alone: 2 tests failed in release run 2026-11-12T0300-1 and passed \
             rerun alone\n"
                .to_string(),
            "The Symphony release run 2026-11-12T0300-1 (2026-11-12 03:00), on main at \
             cafe01234567, failed these tests in company"
                .into(),
            format!(
                "- `-p a --lib -- x --exact`, in `verify/tests`: failed in the run (exit 100, \
                 {load}; log /logs/verify.steps/tests.log); passed alone (exit 0, 3 s, {load}; log \
                 /logs/verify-tests.alone/1.log).\n- `-p b --lib`, in `verify/tests`:"
            ),
            "rewrite the test on a condition, a count, CPU time or an injected clock".into(),
            "\n\nrun: 2026-11-12T0300-1".into(),
        ] {
            assert!(filed.contains(&said), "{said:?} in:\n{filed}");
        }
        assert_eq!(passed_alone_slug("2026-11-12T0300-1"), slug);
        assert!(!slug.starts_with(SERIES));

        // On the release candidate, and refused by the board: said, nothing else held up.
        let board = Fake {
            refuse: vec!["file"],
            ..Fake::default()
        };
        let mut on_candidate = night(&[]);
        on_candidate.rerun = true;
        let verify = Ran {
            steps: vec![tests_reran(&[("-p a --lib", 0)], None)],
            ..ran("verify", 1)
        };
        assert!(
            passed_alone(&on_candidate, std::slice::from_ref(&verify))
                .unwrap()
                .2
                .contains("on the release candidate at cafe01234567")
        );
        let done = report(&board, &on_candidate, &[verify]);
        assert_eq!(
            done.error.as_deref(),
            Some("tests/passed-alone-2026-11-12t0300-1: file refused")
        );
    }

    #[test]
    fn a_step_with_a_failure_that_failed_again_alone_is_red_saying_each_rerun_and_the_rest_get_their_task()
     {
        let board = Fake::default();
        let twice = tests_reran(
            &[
                ("-p a --lib -- x --exact", 0),
                ("-p b --lib -- y --exact", 100),
            ],
            None,
        );
        let verify = Ran {
            steps: vec![twice],
            ..ran("verify", 1)
        };
        let done = report(&board, &night(&[]), &[verify]);
        assert_eq!(
            done.filed,
            [
                "tests/release-red-verify-tests",
                "tests/passed-alone-2026-11-12t0300-1"
            ]
        );
        let red = board.thread("tests/release-red-verify-tests").join("\n");
        let load = "load 31.92, 70.68, 171.07 (over 1, 5 and 15 minutes) on 16 processors";
        assert!(
            red.contains(&format!(
                "The run reran each failure alone, one at a time, in this checkout and over the \
                 build that failed (`tools/test-lanes.sh --alone`); each failed in the run under \
                 the load above, and alone:\n\
                 - `-p a --lib -- x --exact`: passed alone (exit 0, 3 s; {load}). Log: \
                 /logs/verify-tests.alone/1.log\n\
                 - `-p b --lib -- y --exact`: failed again alone (exit 100, 3 s; {load}): it failed \
                 twice. Log: /logs/verify-tests.alone/2.log\n\n"
            )),
            "{red}"
        );
        assert!(red.contains(FIRST), "{red}");
        let passed = board
            .thread("tests/passed-alone-2026-11-12t0300-1")
            .join("\n");
        assert!(passed.contains("1 test failed"), "{passed}");
        assert!(!passed.contains("-p b --lib"), "{passed}");

        // Red again while its task is open: the line it gets says the reruns too.
        let done = report(
            &board,
            &night(&[]),
            &[Ran {
                steps: vec![tests_reran(&[("-p b --lib -- y --exact", 100)], None)],
                ..ran("verify", 1)
            }],
        );
        assert_eq!(done.updated, ["tests/release-red-verify-tests"]);
        let again = board.thread("tests/release-red-verify-tests");
        assert!(
            again
                .last()
                .unwrap()
                .contains("failed again alone (exit 100, 3 s;"),
            "{again:?}"
        );
    }

    #[test]
    fn a_step_none_of_whose_failures_was_rerun_is_red_saying_why_and_files_no_task_for_them() {
        let board = Fake::default();
        let none = tests_reran(
            &[],
            Some("it names 13 failures, more than the 12 the run reruns"),
        );
        let voided = tests_reran(&[("-p a --lib", 0)], Some("the run stopped"));
        for step in [none, voided] {
            let verify = Ran {
                steps: vec![step],
                ..ran("verify", 1)
            };
            assert!(passed_alone(&night(&[]), std::slice::from_ref(&verify)).is_none());
            let said = reran(&verify.steps[0]);
            assert!(
                said == "The run reran none of its failures alone: it names 13 failures, more \
                         than the 12 the run reruns.\n\n"
                    || said.ends_with("Those reruns judge nothing: the run stopped.\n\n"),
                "{said}"
            );
        }
        let verify = Ran {
            steps: vec![tests_reran(&[], Some("it names 13 failures"))],
            ..ran("verify", 1)
        };
        let done = report(&board, &night(&[]), &[verify]);
        assert_eq!(done.filed, ["tests/release-red-verify-tests"]);
        assert!(
            board
                .thread("tests/release-red-verify-tests")
                .join("\n")
                .contains("The run reran none of its failures alone: it names 13 failures.\n\n")
        );
        assert_eq!(
            reran(&ran("verify/lint", 1)),
            "",
            "a step the run reruns nothing of"
        );
    }
}
