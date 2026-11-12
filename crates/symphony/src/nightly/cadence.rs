//! The release cadence counted in landings (docs/RELEASES.md): a run on main's tip is due once
//! `[releases] every_landings` tasks (20 unless the project says) have landed on the default
//! branch since the last release, or since a later run that checked something, so a red run is
//! counted from as a release is and never asked for again on every landing after it. A landing is
//! the merge `just land` makes on the default branch's first-parent line, "Merge branch 'SLUG'".
//! With no release and no run on record, the first count takes every landing on the line. A run
//! in progress is never doubled: while one holds the lock, the count waits for it. It reads the
//! remote's main as this checkout last fetched or pushed it (its own default branch when it has no
//! such ref), the release tags here and the run records, with git and the files alone: it fetches,
//! builds and writes nothing.

use std::path::Path;

use symphony_board::sys::pid_alive;

use super::git::{commit, git, is_ancestor, short};
use super::record::{self, Run, Running, local};
use super::tags::Release;

/// The landing a merge's subject says, by its slug: the land's own merge ("Merge branch 'SLUG'"),
/// or git's for one made by hand on that branch ("Merge branch 'SLUG' into BRANCH"). A merge of
/// the default branch `branch` itself ("Merge branch 'main' into SLUG": a task catching up, which
/// reached the line), of another repository's branch ("… of URL") or of a remote-tracking branch
/// is none.
pub(crate) fn landed<'a>(subject: &'a str, branch: &str) -> Option<&'a str> {
    let rest = subject.strip_prefix("Merge branch '")?;
    let (slug, after) = rest.split_once('\'')?;
    let lands = !slug.is_empty() && slug != branch;
    (lands && (after.is_empty() || after.starts_with(" into "))).then_some(slug)
}

/// A landing: the merge's commit and the slug it landed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Landing {
    pub(crate) commit: String,
    pub(crate) slug: String,
}

/// The landings on `tip`'s first-parent line of the default branch `branch` after `base` (the
/// whole line with none), newest first.
pub(crate) fn landings(
    root: &Path,
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
            "--merges",
            "--format=%H %s",
            &range,
        ],
    )?;
    Ok(listed
        .lines()
        .filter_map(|line| {
            let (commit, subject) = line.split_once(' ')?;
            Some(Landing {
                commit: commit.to_string(),
                slug: landed(subject, branch)?.to_string(),
            })
        })
        .collect())
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
/// whatever order the runs ran in. A release wins a run on its own commit.
pub(crate) fn since(root: &Path, releases: &[(Release, String)], runs: &[Run], tip: &str) -> Since {
    let on_line = |commit: &str| is_ancestor(root, commit, tip).unwrap_or(false);
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
}

impl Cadence {
    /// Whether a run on main's tip starts now: the cadence is on, enough have landed, and no run
    /// holds the lock.
    pub(crate) fn due(&self) -> bool {
        self.every > 0
            && self.running.is_none()
            && self.landings.len() >= usize::try_from(self.every).unwrap_or(usize::MAX)
    }

    /// What starts a run it makes due, as its record keeps it.
    pub(crate) fn cause(&self) -> String {
        let last = self
            .landings
            .first()
            .map(|landing| format!(", the last {}", landing.slug))
            .unwrap_or_default();
        format!(
            "{} landings since {}{last}",
            self.landings.len(),
            self.since.said()
        )
    }

    /// What `--due` says as the run it makes due starts: the first count's, from the line's start,
    /// in words of its own, since it takes the line's whole history at once.
    pub(crate) fn starts(&self) -> String {
        match self.since {
            Since::Start => format!(
                "nightly: due: the first count, {} landings from {}'s start with no release and no \
                 run on record: a night on its tip starts now\n",
                self.landings.len(),
                self.line
            ),
            _ => format!("nightly: due: a night on {}'s tip starts\n", self.line),
        }
    }

    /// The two lines `--status` and `--due` say: what has landed, and what the next run waits for.
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
                "off ([releases] every_landings = 0): only the night's run or `just nightly`".into()
            }
            Some(running) => format!(
                "in progress: the run {} (process {}, since {}); the count waits for it",
                running.id,
                running.pid,
                local(running.started_at)
            ),
            None if count >= every => format!(
                "due: {count} of {} landed; the next landing starts it, or `just nightly --due`",
                self.every
            ),
            None => {
                let more = every - count;
                format!(
                    "after {more} more {} ({count} of {}), or the night's run",
                    if more == 1 { "landing" } else { "landings" },
                    self.every
                )
            }
        };
        text.push_str(&format!("next run: {next}\n"));
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

/// The cadence of the repository at `root`, whose nightly records are in `folder`, with `releases`
/// here newest first, counting every `every` landings on `remote`'s `branch`.
pub(crate) fn of(
    root: &Path,
    folder: &Path,
    releases: &[(Release, String)],
    every: u32,
    remote: &str,
    branch: &str,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nightly::record::Board;
    use crate::test_tree::Tree;
    use std::path::PathBuf;

    #[test]
    fn a_landing_is_the_lands_merge_or_gits_own_onto_the_branch() {
        assert_eq!(landed("Merge branch 'tools/x'", "main"), Some("tools/x"));
        assert_eq!(
            landed("Merge branch 'tools/x' into main", "main"),
            Some("tools/x")
        );
        for none in [
            "Merge remote-tracking branch 'origin/main'",
            "Merge branch 'main' of github.com:heavenstudio/symphony",
            "Merge branch 'main' into tools/x",
            "Merge branch 'main'",
            "Merge branch ''",
            "Merge branch 'unclosed",
            "Regenerate the client distribution after tools/x",
            "Merge pull request #3 from x/y",
        ] {
            assert_eq!(landed(none, "main"), None, "{none}");
        }
        // The default branch is the project's: another's merge of `main` is a landing of it.
        assert_eq!(
            landed("Merge branch 'main' into trunk", "trunk"),
            Some("main")
        );
        assert_eq!(landed("Merge branch 'trunk' into tools/x", "trunk"), None);
    }

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
            version: 1,
            id: id.into(),
            kind: "nightly".into(),
            commit: commit.into(),
            tip: commit.into(),
            started_at: 0.0,
            ended_at: 0.0,
            result: result.into(),
            error: None,
            previous: None,
            checks: if checked {
                vec![super::super::checks::Ran {
                    name: "verify".into(),
                    command: "just verify".into(),
                    exit_code: Some(1),
                    signal: None,
                    seconds: 1.0,
                    log: PathBuf::from("/l"),
                    error: None,
                    timed_out: false,
                    interrupted: false,
                    load: None,
                }]
            } else {
                Vec::new()
            },
            skipped: Vec::new(),
            tag: None,
            pushed: false,
            board: Board::default(),
            remote: "origin".into(),
            unpushed: None,
            cause: None,
            refreshed: None,
        }
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
        }
    }

    #[test]
    fn a_run_is_due_at_the_count_unless_one_runs_or_the_cadence_is_off() {
        assert!(!cadence(20, 19, None).due());
        assert!(cadence(20, 20, None).due());
        assert!(cadence(20, 25, None).due(), "past the count, still due");
        assert!(!cadence(0, 25, None).due(), "off");
        let running = Running {
            id: "r1".into(),
            kind: "nightly".into(),
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
             next run: after 13 more landings (7 of 20), or the night's run\n"
        );
        assert!(
            cadence(20, 19, None)
                .lines()
                .contains("next run: after 1 more landing (19 of 20)")
        );
        assert!(cadence(20, 20, None).lines().contains(
            "next run: due: 20 of 20 landed; the next landing starts it, or `just nightly --due`"
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
    fn the_first_count_says_it_starts_a_night_from_the_lines_whole_history() {
        let mut first = cadence(20, 86, None);
        first.since = Since::Start;
        assert_eq!(
            first.starts(),
            "nightly: due: the first count, 86 landings from origin/main's start with no release \
             and no run on record: a night on its tip starts now\n"
        );
        assert_eq!(
            cadence(20, 20, None).starts(),
            "nightly: due: a night on origin/main's tip starts\n"
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
