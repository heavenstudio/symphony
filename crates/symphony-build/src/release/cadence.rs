//! The release cadence counted in landings (docs/workflow/RELEASES.md): a run on main's tip is due once
//! `[releases] every_landings` tasks (20 unless the project says) have landed on the default
//! branch since the last release, or since a later run that checked something, so a red run is
//! counted from as a release is and never asked for again on every landing after it. A landing is
//! a commit on the default branch's first-parent line that is the merge `just land` makes,
//! "Merge branch 'SLUG'", or the squash a pull request delivered, which its land's receipt names
//! and git bears out ([`super::receipts`]): never a commit taken for one by its subject or its
//! patch. The count is read from the line and the receipts each time, so a landing whose
//! `on_land` never ran is counted by the next count all the same, and one counted twice is one.
//! With no release and no run on record, the first count takes every landing on the line. A run
//! in progress is never doubled: while one holds the lock, the count waits for it. It reads the
//! remote's main as this checkout last fetched or pushed it (its own default branch when it has no
//! such ref), the release tags here, the receipts and the run records, with git and the files
//! alone: it fetches, builds and writes nothing.
//!
//! The hosted clock ([`Clock`]) is a second reason for a run, only where the project has its
//! releases verified on GitHub's runners (`[releases] hosted`): short of the count, a run on main's
//! tip is due once `[releases] hosted_every_minutes` passed since the last run started, unless that
//! tip is the commit last judged. Nothing ticks: a landing's count asks, as for the count itself.
//! Such a run verifies nothing on this machine ([`Cadence::clocked`]), so the count stays what
//! decides this machine's load.

use std::path::Path;

use symphony::reports::git::landing_slug;
use symphony_board::sys::pid_alive;

use super::git::{commit, git, short};
use super::receipts::{self, Receipts};
use super::record::{self, Run, Running, age, local};
use super::tags::Release;

/// A landing: the merge's commit and the slug it landed ([`landing_slug`], the one reading of a
/// merge's subject, which the reports' traces of a red share).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Landing {
    pub(crate) commit: String,
    pub(crate) slug: String,
}

/// The landings on `tip`'s first-parent line of the default branch `branch` after `base` (the
/// whole line with none), newest first, in the repository at `root`, by its receipts.
///
/// # Errors
///
/// Git does not list the line, or a receipt names a commit on it that is not the one it recorded
/// ([`receipts::unborne`]): the count is refused by name, never made without it.
pub(crate) fn landings(
    root: &Path,
    base: Option<&str>,
    tip: &str,
    branch: &str,
) -> Result<Vec<Landing>, String> {
    landings_by(root, &Receipts::read(root), base, tip, branch)
}

/// [`landings`], by `receipts`.
pub(crate) fn landings_by(
    root: &Path,
    receipts: &Receipts,
    base: Option<&str>,
    tip: &str,
    branch: &str,
) -> Result<Vec<Landing>, String> {
    let range = match base {
        Some(base) => format!("{base}..{tip}"),
        None => tip.to_string(),
    };
    let listed = git(
        root,
        &[
            "log",
            "--first-parent",
            "--format=%H%x09%P%x09%T%x09%s",
            &range,
        ],
    )?;
    let mut landings = Vec::new();
    for line in listed.lines() {
        let mut parts = line.splitn(4, '\t');
        let (Some(commit), Some(parents), Some(tree), Some(subject)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let slug = if parents.contains(' ') {
            landing_slug(subject, branch)
        } else if let Some(receipt) = receipts.squashed(commit) {
            if !receipts::borne_out(receipt, parents, tree) {
                return Err(receipts::unborne(receipt, branch));
            }
            Some(receipt.slug.as_str())
        } else {
            None
        };
        if let Some(slug) = slug {
            landings.push(Landing {
                commit: commit.to_string(),
                slug: slug.to_string(),
            });
        }
    }
    Ok(landings)
}

/// What the count starts after.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Since {
    /// The newest release on the line.
    Release { name: String, commit: String },
    /// A run after it that checked something: a red one, or one a check of which ended before it
    /// stopped.
    Run {
        id: String,
        result: String,
        commit: String,
    },
    /// Neither: the line's every landing counts.
    Start,
}

impl Since {
    /// The commit it names; none for the line's start.
    pub(crate) fn commit(&self) -> Option<&str> {
        match self {
            Since::Release { commit, .. } | Since::Run { commit, .. } => Some(commit),
            Since::Start => None,
        }
    }

    /// It, said after "since".
    pub(crate) fn said(&self) -> String {
        match self {
            Since::Release { name, commit } => format!("{name} ({})", short(commit)),
            Since::Run { id, result, commit } => {
                format!("the run {id} on {} ({result})", short(commit))
            }
            Since::Start => {
                "the line's start: no release and no run on record, so every landing counts".into()
            }
        }
    }
}

/// Whether `run` counts as an attempt the cadence counts from: it had a candidate and at least one
/// check of it ended. A run that stopped before any (a process in its folders, a fetch that
/// failed) is asked for again at the next landing.
pub(crate) fn attempted(run: &Run) -> bool {
    !run.commit.is_empty() && !run.checks.is_empty()
}

/// The newest of `releases` (each with its commit) and of the `runs` that were attempts, on
/// `tip`'s line, in `root`: the one whose commit has the most ancestors, so a later commit wins
/// whatever order the runs ran in. A release wins a run on its own commit. A commit is on the line
/// when the line holds it ([`Receipts::holds`]): a release candidate advanced to a head a pull
/// request delivered is released on that head, which a squash leaves in no history of the line.
pub(crate) fn since(root: &Path, releases: &[(Release, String)], runs: &[Run], tip: &str) -> Since {
    let receipts = Receipts::read(root);
    let on_line = |commit: &str| receipts.holds(root, commit, tip).unwrap_or(false);
    let depth = |commit: &str| {
        git(root, &["rev-list", "--count", commit])
            .ok()
            .and_then(|count| count.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let mut best: Option<(u64, Since)> = None;
    let mut consider = |commit: &str, since: Since| {
        if !on_line(commit) {
            return;
        }
        let depth = depth(commit);
        if best.as_ref().is_none_or(|(deepest, _)| depth > *deepest) {
            best = Some((depth, since));
        }
    };
    for (release, commit) in releases {
        consider(
            commit,
            Since::Release {
                name: release.name(),
                commit: commit.clone(),
            },
        );
    }
    for run in runs.iter().rev().filter(|run| attempted(run)) {
        consider(
            &run.commit,
            Since::Run {
                id: run.id.clone(),
                result: run.result.clone(),
                commit: run.commit.clone(),
            },
        );
    }
    best.map_or(Since::Start, |(_, since)| since)
}

/// What a release candidate holds of the cadence ([`super::candidate`]): while one is open, main's
/// tip is not run, whatever the count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hold {
    /// Open at `tip`, its rerun waiting for these reds to be answered.
    Waiting { tip: String, reds: Vec<String> },
    /// Open at `tip`, every red answered: its rerun is due.
    Answered { tip: String },
    /// Dropped for `why`: a run of main's tip is due.
    Dropped { why: String },
}

/// The hosted clock: how often a run of main's tip starts while the release is verified on GitHub's
/// runners, whatever has landed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clock {
    /// `[releases] hosted_every_minutes`, above 0.
    pub(crate) every: u32,
    /// Now, as the count was asked.
    pub(crate) now: f64,
    /// The release run on record that started last, whatever it came to: its id and when.
    pub(crate) last: Option<(String, f64)>,
}

impl Clock {
    /// The clock of `every` minutes at `now`, counted from the newest start among `runs`: a release
    /// run's, never a refresh's or an out-of-cycle release's, which judge nothing of main's tip.
    pub(crate) fn of(every: u32, now: f64, runs: &[Run]) -> Clock {
        let last = (runs.iter())
            .filter(|run| run.kind == "release")
            .max_by(|a, b| a.started_at.total_cmp(&b.started_at))
            .map(|run| (run.id.clone(), run.started_at));
        Clock { every, now, last }
    }

    /// When the interval since the last run's start ends; none with no run on record.
    fn ends(&self) -> Option<f64> {
        (self.last.as_ref()).map(|(_, started)| started + f64::from(self.every) * 60.0)
    }

    /// Whether the interval has passed: at its end, or with no run on record.
    fn passed(&self) -> bool {
        self.ends().is_none_or(|ends| self.now >= ends)
    }

    /// The last run and how long ago it started, as a cause and a line say it.
    fn last_said(&self) -> String {
        match &self.last {
            Some((id, started)) => format!(
                "the run {id} started {} ({})",
                local(*started),
                age(self.now, *started)
            ),
            None => "no run is on record".into(),
        }
    }
}

/// Where the cadence stands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cadence {
    /// `[releases] every_landings`; 0 starts no run.
    pub(crate) every: u32,
    /// The ref counted and the commit it names.
    pub(crate) line: String,
    pub(crate) tip: String,
    pub(crate) since: Since,
    /// Newest first.
    pub(crate) landings: Vec<Landing>,
    /// The run holding the lock, when its process lives.
    pub(crate) running: Option<Running>,
    /// What a release candidate holds, when one does.
    pub(crate) held: Option<Hold>,
    /// The hosted clock, where the project's releases are verified on GitHub's runners.
    pub(crate) clock: Option<Clock>,
}

impl Cadence {
    /// Whether enough have landed for the count's run.
    fn counted(&self) -> bool {
        self.landings.len() >= usize::try_from(self.every).unwrap_or(usize::MAX)
    }

    /// Whether the line's tip is the commit last judged: the newest release, or a later run that
    /// checked something.
    pub(crate) fn judged(&self) -> bool {
        self.since.commit() == Some(self.tip.as_str())
    }

    /// Whether a run starts now: the cadence is on, no run holds the lock, and a release
    /// candidate's rerun is due, or one was dropped, or none is open and enough have landed or the
    /// hosted clock says so ([`Cadence::clocked`]).
    pub(crate) fn due(&self) -> bool {
        self.every > 0
            && self.running.is_none()
            && match &self.held {
                Some(Hold::Waiting { .. }) => false,
                Some(Hold::Answered { .. } | Hold::Dropped { .. }) => true,
                None => self.counted() || self.clocked(),
            }
    }

    /// Whether the hosted clock alone starts a run now: the cadence is on, no run holds the lock,
    /// no release candidate holds anything of it, the count is not reached, the tip is not judged
    /// yet and the clock's interval has passed since the last run started. Its run is verified on
    /// GitHub's runners or not at all: the count's run alone may be verified here.
    pub(crate) fn clocked(&self) -> bool {
        self.every > 0
            && self.running.is_none()
            && self.held.is_none()
            && !self.counted()
            && !self.judged()
            && self.clock.as_ref().is_some_and(Clock::passed)
    }

    /// What starts a run it makes due, as its record keeps it.
    pub(crate) fn cause(&self) -> String {
        let last = self
            .landings
            .first()
            .map(|landing| format!(", the last {}", landing.slug))
            .unwrap_or_default();
        match &self.held {
            Some(Hold::Answered { tip }) => {
                format!("the release candidate {}'s reds answered{last}", short(tip))
            }
            Some(Hold::Dropped { why }) => format!("the release candidate dropped: {why}"),
            None if self.clocked() => {
                let clock = self.clock.as_ref();
                format!(
                    "the hosted clock, every {} min: {}; {} landings since {}{last}",
                    clock.map_or(0, |clock| clock.every),
                    clock.map(Clock::last_said).unwrap_or_default(),
                    self.landings.len(),
                    self.since.said()
                )
            }
            _ => format!(
                "{} landings since {}{last}",
                self.landings.len(),
                self.since.said()
            ),
        }
    }

    /// What `--due` says as the run it makes due starts: the first count's, from the line's start,
    /// in words of its own, since it takes the line's whole history at once.
    pub(crate) fn starts(&self) -> String {
        match &self.held {
            Some(Hold::Answered { tip }) => {
                return format!(
                    "release: due: the rerun of the release candidate {} starts\n",
                    short(tip)
                );
            }
            Some(Hold::Dropped { .. }) => {
                return format!(
                    "release: due: the release candidate was dropped, so a run on {}'s tip starts\n",
                    self.line
                );
            }
            _ => {}
        }
        if self.clocked() {
            return format!(
                "release: due: the hosted clock: a run on {}'s tip starts, verified on GitHub's \
                 runners or not at all\n",
                self.line
            );
        }
        match self.since {
            Since::Start => format!(
                "release: due: the first count, {} landings from {}'s start with no release and no \
                 run on record: a run on its tip starts now\n",
                self.landings.len(),
                self.line
            ),
            _ => format!("release: due: a run on {}'s tip starts\n", self.line),
        }
    }

    /// What the hosted clock waits for, as its line says it after `hosted clock: `; none while the
    /// project has no clock or the cadence is off.
    fn clock_said(&self) -> Option<String> {
        let clock = self.clock.as_ref().filter(|_| self.every > 0)?;
        let every = format!("every {} min on GitHub's runners", clock.every);
        Some(if self.running.is_some() {
            format!("{every}; it waits for the run in progress")
        } else if self.held.is_some() {
            format!("{every}; the release candidate comes first")
        } else if self.judged() {
            format!(
                "{every}; {}'s tip ({}) is judged already: no run until it moves",
                self.line,
                short(&self.tip)
            )
        } else if self.counted() {
            format!("{every}; the count's run is due first")
        } else if let Some(ends) = clock.ends().filter(|_| !clock.passed()) {
            format!(
                "{every}; {}: the first landing from {} starts a run of {}'s tip",
                clock.last_said(),
                local(ends),
                self.line
            )
        } else {
            format!(
                "due, {every}; {}: the next landing starts a run of {}'s tip, or `just release \
                 --due`",
                clock.last_said(),
                self.line
            )
        })
    }

    /// The lines `--status` and `--due` say: what has landed, what the next run waits for and,
    /// where the project has one, what the hosted clock waits for.
    pub(crate) fn lines(&self) -> String {
        let count = self.landings.len();
        let last = self
            .landings
            .first()
            .map(|landing| format!("; the last {} ({})", landing.slug, short(&landing.commit)))
            .unwrap_or_default();
        let mut text = format!(
            "landings: {count} on {} since {}{last}\n",
            self.line,
            self.since.said()
        );
        let every = usize::try_from(self.every).unwrap_or(usize::MAX);
        let next = match &self.running {
            _ if self.every == 0 => {
                "off ([releases] every_landings = 0): only the scheduled run or `just release`"
                    .into()
            }
            Some(running) => format!(
                "in progress: the run {} (process {}, since {}); the count waits for it",
                running.id,
                running.pid,
                local(running.started_at)
            ),
            None if self.held.is_some() => match &self.held {
                Some(Hold::Waiting { tip, reds }) => format!(
                    "held for the release candidate {}: its rerun waits for {}, and main's tip is \
                     not run while it is open",
                    short(tip),
                    reds.join(", ")
                ),
                Some(Hold::Answered { tip }) => format!(
                    "due: the release candidate {}'s reds are answered: the next landing starts \
                     its rerun, or `just release --rerun`",
                    short(tip)
                ),
                Some(Hold::Dropped { why }) => format!(
                    "due: the release candidate was dropped ({why}): the next landing starts a \
                     run of {}'s tip, or `just release --due`",
                    self.line
                ),
                None => String::new(),
            },
            None if count >= every => format!(
                "due: {count} of {} landed; the next landing starts it, or `just release --due`",
                self.every
            ),
            None => {
                let more = every - count;
                format!(
                    "after {more} more {} ({count} of {}), or the scheduled run",
                    if more == 1 { "landing" } else { "landings" },
                    self.every
                )
            }
        };
        text.push_str(&format!("next run: {next}\n"));
        if let Some(clock) = self.clock_said() {
            text.push_str(&format!("hosted clock: {clock}\n"));
        }
        text
    }
}

/// The ref the count reads: the remote's main as last fetched or pushed, else this repository's
/// own default branch; with the commit it names.
pub(crate) fn line_of(root: &Path, remote: &str, branch: &str) -> Result<(String, String), String> {
    let tracking = format!("{remote}/{branch}");
    if let Ok(tip) = commit(root, &format!("refs/remotes/{tracking}")) {
        return Ok((tracking, tip));
    }
    commit(root, &format!("refs/heads/{branch}"))
        .map(|tip| (branch.to_string(), tip))
        .map_err(|e| format!("neither {tracking} nor {branch} names a commit here: {e}"))
}

/// The cadence of the repository at `root`, whose release run records are in `folder`, with `releases`
/// here newest first, counting every `every` landings on `remote`'s `branch`, what a release
/// candidate `held` of it, and the hosted clock where the project has one: its minutes
/// (`symphony::project::Releases::hosted_clock`) and now.
#[expect(
    clippy::too_many_arguments,
    reason = "where the count is read and what it is counted by, each named once by its caller"
)]
pub(crate) fn of(
    root: &Path,
    folder: &Path,
    releases: &[(Release, String)],
    every: u32,
    remote: &str,
    branch: &str,
    held: Option<Hold>,
    clock: Option<(u32, f64)>,
) -> Result<Cadence, String> {
    let (line, tip) = line_of(root, remote, branch)?;
    let runs = record::read(folder);
    let since = since(root, releases, &runs, &tip);
    let landings = landings(root, since.commit(), &tip, branch)?;
    let running = record::holder(folder).filter(|holder| pid_alive(i64::from(holder.pid)));
    Ok(Cadence {
        every,
        line,
        tip,
        since,
        landings,
        running,
        held,
        clock: clock.map(|(every, now)| Clock::of(every, now, &runs)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;
    use std::path::PathBuf;
    use symphony_board::land::delivery::Receipt;

    /// A repository on `main` with a commit, then a landing of each of `slugs` (a branch merged
    /// with the land's own message), a plain commit and a merge of a remote-tracking branch between
    /// them: the repository and each landing's merge commit.
    fn repository(tree: &Tree, slugs: &[&str]) -> (PathBuf, Vec<String>) {
        let root = tree.path("repo");
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            git(
                &root,
                &[
                    &["-c", "user.name=t", "-c", "user.email=t@example.com"][..],
                    args,
                ]
                .concat(),
            )
            .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["commit", "-q", "--allow-empty", "-m", "first"]);
        let mut merges = Vec::new();
        for slug in slugs {
            run(&["checkout", "-q", "-b", slug]);
            run(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("work on {slug}"),
            ]);
            run(&["checkout", "-q", "main"]);
            run(&[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("Merge branch '{slug}'"),
                slug,
            ]);
            merges.push(run(&["rev-parse", "HEAD"]));
            run(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "a generator's commit",
            ]);
        }
        // A task branch catching up with main, then fast-forwarded onto it: git's merge of `main`
        // on the line, which is no landing.
        run(&["checkout", "-q", "-b", "caught-up"]);
        run(&["commit", "-q", "--allow-empty", "-m", "a task's work"]);
        run(&["checkout", "-q", "main"]);
        run(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "main moved meanwhile",
        ]);
        run(&["checkout", "-q", "caught-up"]);
        run(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'main' into caught-up",
            "main",
        ]);
        run(&["checkout", "-q", "main"]);
        run(&["merge", "-q", "--ff-only", "caught-up"]);
        // A merge that is no landing: the land's own merge of what origin gained.
        run(&["checkout", "-q", "-b", "gained"]);
        run(&["commit", "-q", "--allow-empty", "-m", "gained"]);
        run(&["checkout", "-q", "main"]);
        run(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge remote-tracking branch 'origin/main'",
            "gained",
        ]);
        (root, merges)
    }

    fn run_on(id: &str, commit: &str, result: &str, checked: bool) -> Run {
        Run {
            version: record::VERSION,
            id: id.into(),
            kind: "release".into(),
            commit: commit.into(),
            tip: commit.into(),
            result: result.into(),
            checks: if checked {
                vec![super::super::checks::Ran {
                    name: "verify".into(),
                    command: "just verify".into(),
                    exit_code: Some(1),
                    signal: None,
                    seconds: 1.0,
                    log: PathBuf::from("/l"),
                    steps: Vec::new(),
                    error: None,
                    timed_out: false,
                    interrupted: false,
                    load: None,
                    alone: None,
                }]
            } else {
                Vec::new()
            },
            remote: "origin".into(),
            ..Run::default()
        }
    }

    #[test]
    fn a_squash_a_receipt_names_is_a_landing_and_no_other_commit_of_one_parent_is() {
        let tree = Tree::new();
        let (root, merges) = repository(&tree, &["a/one"]);
        let before = commit(&root, "main").unwrap();
        // A line with no receipt counts as it always did, whatever its project file says.
        std::fs::write(
            root.join("symphony.toml"),
            "[land]\ndelivery = [\"host\", \"deliver\"]\n",
        )
        .unwrap();
        receipts::tests::run(&root, &["add", "symphony.toml"]);
        receipts::tests::run(
            &root,
            &["commit", "-q", "-m", "a/one: deliver by pull request"],
        );
        let keyed = commit(&root, "main").unwrap();
        let slugs = |tip: &str, base: Option<&str>| -> Vec<String> {
            (landings(&root, base, tip, "main").unwrap().into_iter())
                .map(|landing| landing.slug)
                .collect()
        };
        assert_eq!(slugs(&keyed, None), ["a/one"], "no subject makes a landing");
        let two = receipts::tests::deliver(&root, "b/two", "main");
        let three = receipts::tests::deliver(&root, "c/three", &before);
        let tip = commit(&root, "main").unwrap();
        let all = landings(&root, None, &tip, "main").unwrap();
        assert_eq!(
            all,
            [
                Landing {
                    commit: three.squash.clone(),
                    slug: "c/three".into()
                },
                Landing {
                    commit: two.squash.clone(),
                    slug: "b/two".into()
                },
                Landing {
                    commit: merges[0].clone(),
                    slug: "a/one".into()
                },
            ],
            "merges and squashes on one line, newest first"
        );
        assert_eq!(slugs(&tip, Some(&two.squash)), ["c/three"]);
        assert_eq!(slugs(&tip, Some(&tip)), [] as [&str; 0]);
        // Counted from a delivered head, which no history of the line holds: what the line
        // gained since that head left it.
        assert_eq!(slugs(&tip, Some(&two.head)), ["c/three", "b/two"]);
        // Counted twice, one count: reading again changes nothing.
        assert_eq!(landings(&root, None, &tip, "main").unwrap(), all);
        // A squash made by hand with a landing's subject and no receipt is none.
        let none = Receipts::default();
        assert_eq!(
            landings_by(&root, &none, Some(&keyed), &tip, "main").unwrap(),
            []
        );
        // A receipt of a commit off the line counts nothing there.
        let off = Receipt {
            squash: two.head.clone(),
            ..two.clone()
        };
        let only = Receipts::of(vec![off]);
        assert_eq!(
            landings_by(&root, &only, Some(&keyed), &tip, "main").unwrap(),
            []
        );
    }

    #[test]
    fn a_receipt_git_does_not_bear_out_refuses_the_count_by_name() {
        let tree = Tree::new();
        let (root, _) = repository(&tree, &["a/one"]);
        let two = receipts::tests::deliver(&root, "b/two", "main");
        let tip = commit(&root, "main").unwrap();
        for wrong in [
            Receipt {
                tree: two.parent.clone(),
                ..two.clone()
            },
            Receipt {
                parent: two.head.clone(),
                ..two.clone()
            },
        ] {
            let refused = landings_by(
                &root,
                &Receipts::of(vec![wrong.clone()]),
                None,
                &tip,
                "main",
            )
            .unwrap_err();
            assert_eq!(refused, receipts::unborne(&wrong, "main"));
        }
        // Before the commit it names, the line is counted as ever.
        let wrong = Receipts::of(vec![Receipt {
            tree: two.parent.clone(),
            ..two.clone()
        }]);
        assert_eq!(
            landings_by(&root, &wrong, None, &two.parent, "main")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_release_on_a_delivered_head_is_counted_from_once_the_line_holds_its_squash() {
        let tree = Tree::new();
        let (root, merges) = repository(&tree, &["a/one"]);
        let two = receipts::tests::deliver(&root, "b/two", "main");
        let tip = commit(&root, "main").unwrap();
        let release =
            |name: &str, commit: &str| (Release::parse(name).unwrap(), commit.to_string());
        let releases = [
            release("release-2026-11-13", &two.head),
            release("release-2026-11-12", &merges[0]),
        ];
        assert!(matches!(
            since(&root, &releases, &[], &tip),
            Since::Release { name, commit } if name == "release-2026-11-13" && commit == two.head
        ));
        // Before its squash is on the line it is on no line, and the older release counts.
        assert!(matches!(
            since(&root, &releases, &[], &two.parent),
            Since::Release { name, .. } if name == "release-2026-11-12"
        ));
        // A red rerun of that head is counted from as a red run on the line is.
        let runs = [run_on("rerun", &two.head, "red", true)];
        assert_eq!(
            since(&root, &releases[1..], &runs, &tip).commit(),
            Some(two.head.as_str())
        );
    }

    #[test]
    fn the_landings_are_the_lands_merges_on_the_first_parent_line_after_the_base() {
        let tree = Tree::new();
        let (root, merges) = repository(&tree, &["a/one", "b/two", "c/three"]);
        let tip = commit(&root, "main").unwrap();
        let all = landings(&root, None, &tip, "main").unwrap();
        let slugs: Vec<&str> = all.iter().map(|l| l.slug.as_str()).collect();
        assert_eq!(slugs, ["c/three", "b/two", "a/one"], "newest first");
        assert_eq!(all[0].commit, merges[2]);
        let after = landings(&root, Some(&merges[0]), &tip, "main").unwrap();
        let slugs: Vec<&str> = after.iter().map(|l| l.slug.as_str()).collect();
        assert_eq!(slugs, ["c/three", "b/two"]);
        assert!(
            landings(&root, Some(&tip), &tip, "main")
                .unwrap()
                .is_empty()
        );
        // Counted on a line whose default branch is another, the merge of `main` is a landing.
        let elsewhere = landings(&root, None, &tip, "trunk").unwrap();
        assert_eq!(elsewhere[0].slug, "main", "{elsewhere:?}");
        assert_eq!(elsewhere.len(), 4);
    }

    #[test]
    fn the_count_starts_after_the_newest_release_or_a_later_run_that_checked_something() {
        let tree = Tree::new();
        let (root, merges) = repository(&tree, &["a/one", "b/two", "c/three", "d/four"]);
        let tip = commit(&root, "main").unwrap();
        let release =
            |name: &str, commit: &str| (Release::parse(name).unwrap(), commit.to_string());
        assert_eq!(since(&root, &[], &[], &tip), Since::Start);
        let releases = [release("release-2026-11-12", &merges[0])];
        assert_eq!(
            since(&root, &releases, &[], &tip),
            Since::Release {
                name: "release-2026-11-12".into(),
                commit: merges[0].clone()
            }
        );
        // A red run later than the release counts from its commit; one that checked nothing does
        // not count, nor does one older than the release, nor one off the line.
        let runs = [
            run_on("older", &merges[0], "red", true),
            run_on("red", &merges[2], "red", true),
            run_on("stopped", &merges[3], "error", false),
            run_on("gone", &"f".repeat(40), "red", true),
        ];
        assert_eq!(
            since(&root, &releases, &runs, &tip),
            Since::Run {
                id: "red".into(),
                result: "red".into(),
                commit: merges[2].clone()
            }
        );
        // Whatever order the runs ran in, the latest commit wins; a release on a run's commit wins.
        let runs = [
            run_on("later", &merges[3], "red", true),
            run_on("earlier", &merges[1], "red", true),
        ];
        assert_eq!(
            since(&root, &releases, &runs, &tip).commit(),
            Some(merges[3].as_str())
        );
        let releases = [
            release("release-2026-11-13", &merges[3]),
            release("release-2026-11-12", &merges[0]),
        ];
        assert!(matches!(
            since(&root, &releases, &runs, &tip),
            Since::Release { name, .. } if name == "release-2026-11-13"
        ));
        assert!(!attempted(&run_on("none", "", "error", true)));
    }

    fn cadence(every: u32, count: usize, running: Option<Running>) -> Cadence {
        Cadence {
            every,
            line: "origin/main".into(),
            tip: "t".repeat(40),
            since: Since::Release {
                name: "release-2026-11-12".into(),
                commit: "a".repeat(40),
            },
            landings: (0..count)
                .map(|n| Landing {
                    commit: format!("{n:0>40}"),
                    slug: format!("tools/task-{n}"),
                })
                .collect(),
            running,
            held: None,
            clock: None,
        }
    }

    #[test]
    fn a_run_is_due_at_the_count_unless_one_runs_or_the_cadence_is_off() {
        assert!(!cadence(20, 19, None).due());
        assert!(cadence(20, 20, None).due());
        assert!(cadence(20, 25, None).due(), "past the count, still due");
        assert!(!cadence(0, 25, None).due(), "off");
        let running = Running {
            version: record::VERSION,
            id: "r1".into(),
            kind: "release".into(),
            pid: 7,
            started_at: 1_800_000_000.0,
            until: 1_800_003_600.0,
            check: None,
        };
        assert!(!cadence(20, 40, Some(running.clone())).due(), "not doubled");
        let said = cadence(20, 40, Some(running)).lines();
        assert!(
            said.ends_with(&format!(
                "next run: in progress: the run r1 (process 7, since {}); the count waits for it\n",
                local(1_800_000_000.0)
            )),
            "{said}"
        );
    }

    #[test]
    fn the_lines_say_what_landed_since_what_and_what_the_next_run_waits_for() {
        assert_eq!(
            cadence(20, 7, None).lines(),
            "landings: 7 on origin/main since release-2026-11-12 (aaaaaaaaaaaa); the last \
             tools/task-0 (000000000000)\n\
             next run: after 13 more landings (7 of 20), or the scheduled run\n"
        );
        assert!(
            cadence(20, 19, None)
                .lines()
                .contains("next run: after 1 more landing (19 of 20)")
        );
        assert!(cadence(20, 20, None).lines().contains(
            "next run: due: 20 of 20 landed; the next landing starts it, or `just release --due`"
        ));
        assert!(
            cadence(0, 3, None)
                .lines()
                .contains("next run: off ([releases] every_landings = 0)")
        );
        let none = cadence(20, 0, None).lines();
        assert!(
            none.starts_with(
                "landings: 0 on origin/main since release-2026-11-12 (aaaaaaaaaaaa)\n"
            ),
            "{none}"
        );
        assert_eq!(
            cadence(20, 20, None).cause(),
            "20 landings since release-2026-11-12 (aaaaaaaaaaaa), the last tools/task-0"
        );
        let mut start = cadence(20, 2, None);
        start.since = Since::Start;
        assert!(start.lines().contains(
            "since the line's start: no release and no run on record, so every landing counts;"
        ));
    }

    #[test]
    fn a_release_candidate_holds_main_while_its_reds_wait_and_its_rerun_or_drop_is_due() {
        let held = |hold: Hold, every: u32, count: usize| Cadence {
            held: Some(hold),
            ..cadence(every, count, None)
        };
        let waiting = || Hold::Waiting {
            tip: "c".repeat(40),
            reds: vec!["tests/release-red-verify".into()],
        };
        let answered = || Hold::Answered {
            tip: "c".repeat(40),
        };
        let dropped = || Hold::Dropped {
            why: "dropped by `just release --drop`".into(),
        };
        assert!(!held(waiting(), 20, 40).due(), "however many landed");
        assert!(held(answered(), 20, 0).due(), "whatever the count");
        assert!(held(dropped(), 20, 0).due());
        assert!(!held(answered(), 0, 0).due(), "the cadence off starts none");
        let running = Running {
            version: record::VERSION,
            id: "r1".into(),
            kind: "release".into(),
            pid: 7,
            started_at: 1_800_000_000.0,
            until: 1_800_003_600.0,
            check: None,
        };
        let busy = Cadence {
            held: Some(answered()),
            ..cadence(20, 0, Some(running))
        };
        assert!(!busy.due(), "never doubled");

        assert!(held(waiting(), 20, 40).lines().ends_with(
            "next run: held for the release candidate cccccccccccc: its rerun waits for \
             tests/release-red-verify, and main's tip is not run while it is open\n"
        ));
        assert!(held(answered(), 20, 1).lines().ends_with(
            "next run: due: the release candidate cccccccccccc's reds are answered: the next \
             landing starts its rerun, or `just release --rerun`\n"
        ));
        assert!(held(dropped(), 20, 1).lines().ends_with(
            "next run: due: the release candidate was dropped (dropped by `just release --drop`): \
             the next landing starts a run of origin/main's tip, or `just release --due`\n"
        ));
        assert_eq!(
            held(answered(), 20, 1).cause(),
            "the release candidate cccccccccccc's reds answered, the last tools/task-0"
        );
        assert_eq!(
            held(dropped(), 20, 1).cause(),
            "the release candidate dropped: dropped by `just release --drop`"
        );
        assert_eq!(
            held(answered(), 20, 1).starts(),
            "release: due: the rerun of the release candidate cccccccccccc starts\n"
        );
        assert_eq!(
            held(dropped(), 20, 1).starts(),
            "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"
        );
    }

    /// The cadence of 20 with `count` landed, its tip not judged, and a clock of 30 minutes whose
    /// last run started `ago` minutes before now (none on record with none).
    fn clocked(count: usize, ago: Option<f64>) -> Cadence {
        const NOW: f64 = 1_800_000_000.0;
        Cadence {
            clock: Some(Clock {
                every: 30,
                now: NOW,
                last: ago.map(|ago| ("r0".to_string(), NOW - ago * 60.0)),
            }),
            ..cadence(20, count, None)
        }
    }

    fn a_run() -> Running {
        Running {
            version: record::VERSION,
            id: "r1".into(),
            kind: "release".into(),
            pid: 7,
            started_at: 1_800_000_000.0,
            until: 1_800_003_600.0,
            check: None,
        }
    }

    #[test]
    fn the_hosted_clock_starts_a_run_once_its_interval_passed_since_the_last_run_started() {
        assert!(!clocked(3, Some(29.9)).due(), "short of the interval");
        assert!(clocked(3, Some(30.0)).due(), "at the interval");
        assert!(clocked(3, Some(30.0)).clocked());
        assert!(clocked(3, Some(600.0)).due());
        assert!(clocked(3, None).clocked(), "no run on record: at once");
        // Whatever landed: a tip moved by a commit that is no landing is not judged either.
        assert!(clocked(0, Some(45.0)).clocked());
        // Without a clock the same count starts nothing.
        assert!(!cadence(20, 3, None).due());
        assert!(!cadence(20, 3, None).clocked());
    }

    #[test]
    fn the_hosted_clock_starts_no_run_of_a_tip_judged_already() {
        let mut judged = clocked(0, Some(600.0));
        judged.since = Since::Run {
            id: "r0".into(),
            result: "red".into(),
            commit: judged.tip.clone(),
        };
        assert!(judged.judged());
        assert!(!judged.clocked() && !judged.due());
        assert!(
            judged.lines().ends_with(
                "hosted clock: every 30 min on GitHub's runners; origin/main's tip \
                 (tttttttttttt) is judged already: no run until it moves\n"
            ),
            "{}",
            judged.lines()
        );
        judged.since = Since::Release {
            name: "release-2026-11-12".into(),
            commit: judged.tip.clone(),
        };
        assert!(!judged.due(), "released is judged");
    }

    #[test]
    fn the_hosted_clock_gives_way_to_a_run_a_candidate_the_count_and_the_cadence_turned_off() {
        // A run in progress is never doubled.
        let busy = Cadence {
            running: Some(a_run()),
            ..clocked(3, Some(45.0))
        };
        assert!(!busy.clocked() && !busy.due());
        assert!(
            busy.lines().ends_with(
                "hosted clock: every 30 min on GitHub's runners; it waits for the run in \
                 progress\n"
            ),
            "{}",
            busy.lines()
        );
        // No run of main's tip while a release candidate is open, and its rerun or its drop's run
        // is its own, never the clock's: it may be verified here.
        for (hold, due) in [
            (
                Hold::Waiting {
                    tip: "c".repeat(40),
                    reds: vec!["tests/release-red-verify".into()],
                },
                false,
            ),
            (
                Hold::Answered {
                    tip: "c".repeat(40),
                },
                true,
            ),
            (Hold::Dropped { why: "why".into() }, true),
        ] {
            let held = Cadence {
                held: Some(hold),
                ..clocked(3, Some(45.0))
            };
            assert_eq!(held.due(), due, "{held:?}");
            assert!(!held.clocked(), "{held:?}");
            assert!(
                held.lines().ends_with(
                    "hosted clock: every 30 min on GitHub's runners; the release candidate \
                     comes first\n"
                ),
                "{}",
                held.lines()
            );
            assert!(!held.cause().contains("clock"), "{}", held.cause());
        }
        // At the count the run is the count's, which may be verified here.
        let counted = clocked(20, Some(45.0));
        assert!(counted.due() && !counted.clocked());
        assert_eq!(
            counted.cause(),
            "20 landings since release-2026-11-12 (aaaaaaaaaaaa), the last tools/task-0"
        );
        assert_eq!(
            counted.starts(),
            "release: due: a run on origin/main's tip starts\n"
        );
        assert!(
            (counted.lines()).ends_with(
                "hosted clock: every 30 min on GitHub's runners; the count's run is due first\n"
            ),
            "{}",
            counted.lines()
        );
        // The cadence off turns the clock off with it, and says nothing of it.
        let off = Cadence {
            every: 0,
            ..clocked(3, Some(45.0))
        };
        assert!(!off.due() && !off.clocked());
        assert!(!off.lines().contains("hosted clock"), "{}", off.lines());
        assert!(!cadence(20, 3, None).lines().contains("hosted clock"));
    }

    #[test]
    fn the_hosted_clock_says_what_it_waits_for_what_started_its_run_and_that_nothing_runs_here() {
        const NOW: f64 = 1_800_000_000.0;
        let waiting = clocked(3, Some(12.0));
        let started = local(NOW - 12.0 * 60.0);
        assert_eq!(
            waiting.lines().lines().last().unwrap(),
            format!(
                "hosted clock: every 30 min on GitHub's runners; the run r0 started {started} \
                 (12 min ago): the first landing from {} starts a run of origin/main's tip",
                local(NOW + 18.0 * 60.0)
            )
        );
        let due = clocked(3, Some(45.0));
        let started = local(NOW - 45.0 * 60.0);
        assert_eq!(
            due.lines().lines().last().unwrap(),
            format!(
                "hosted clock: due, every 30 min on GitHub's runners; the run r0 started \
                 {started} (45 min ago): the next landing starts a run of origin/main's tip, or \
                 `just release --due`"
            )
        );
        assert_eq!(
            due.cause(),
            format!(
                "the hosted clock, every 30 min: the run r0 started {started} (45 min ago); 3 \
                 landings since release-2026-11-12 (aaaaaaaaaaaa), the last tools/task-0"
            )
        );
        assert_eq!(
            due.starts(),
            "release: due: the hosted clock: a run on origin/main's tip starts, verified on \
             GitHub's runners or not at all\n"
        );
        let first = clocked(0, None);
        assert_eq!(
            first.cause(),
            "the hosted clock, every 30 min: no run is on record; 0 landings since \
             release-2026-11-12 (aaaaaaaaaaaa)"
        );
        assert!(
            first.lines().ends_with(
                "hosted clock: due, every 30 min on GitHub's runners; no run is on record: the \
                 next landing starts a run of origin/main's tip, or `just release --due`\n"
            ),
            "{}",
            first.lines()
        );
    }

    #[test]
    fn the_hosted_clock_counts_from_the_newest_release_run_started_whatever_it_came_to() {
        let at = |id: &str, kind: &str, result: &str, started: f64| Run {
            kind: kind.into(),
            started_at: started,
            ..run_on(id, "", result, false)
        };
        assert_eq!(Clock::of(30, 100.0, &[]).last, None);
        let runs = [
            at("newest", "release", "unjudged", 90.0),
            at("older", "release", "green", 50.0),
            at("refresh", "refresh", "green", 95.0),
            at("urgent", "out-of-cycle", "green", 99.0),
        ];
        let clock = Clock::of(30, 100.0, &runs);
        assert_eq!(clock.last, Some(("newest".to_string(), 90.0)));
        assert_eq!((clock.every, clock.now), (30, 100.0));
        assert!(!clock.passed());
        assert!(Clock::of(30, 90.0 + 1800.0, &runs).passed());
    }

    #[test]
    fn the_first_count_says_it_starts_a_run_from_the_lines_whole_history() {
        let mut first = cadence(20, 86, None);
        first.since = Since::Start;
        assert_eq!(
            first.starts(),
            "release: due: the first count, 86 landings from origin/main's start with no release \
             and no run on record: a run on its tip starts now\n"
        );
        assert_eq!(
            cadence(20, 20, None).starts(),
            "release: due: a run on origin/main's tip starts\n"
        );
    }

    #[test]
    fn the_line_is_the_remotes_main_as_fetched_else_the_branch_itself() {
        let tree = Tree::new();
        let (root, _) = repository(&tree, &["a/one"]);
        let main = commit(&root, "main").unwrap();
        assert_eq!(
            line_of(&root, "origin", "main").unwrap(),
            ("main".to_string(), main.clone())
        );
        git(&root, &["update-ref", "refs/remotes/origin/main", "main~1"]).unwrap();
        let (line, tip) = line_of(&root, "origin", "main").unwrap();
        assert_eq!(line, "origin/main");
        assert_ne!(tip, main);
        assert!(line_of(&root, "origin", "trunk").is_err());
    }
}
