//! What reached a project's default branch in each hour of a day file, from two cheap reads of the
//! branch ([`read`]):
//!
//! - its line of first parents since the roll-up's window opened, from its tip, with the lines each
//!   commit there changed, a merge by what it brought against its first parent: a merge there is a
//!   landing, and those lines are the lines that reached the branch;
//! - every commit that reached it since the commit that line starts from, by its parents, subject
//!   and the paths it changed, no diff;
//!
//! and, for each red that names the commits it came between, the merges on the line of first
//! parents in that range, the landings it may trace back to ([`trace`]).
//!
//! Each commit counts in the hour it reached the branch ([`reached`]): the hour of the commit on
//! the line of first parents that brought it there, itself or the merge that landed it, so a
//! branch's commits made the day before their landing count with it, and commits, merges and lines
//! share one time base. A merge off the line of first parents (a branch taking the default branch
//! in) counts as neither a commit nor a landing. What [`excluded`] names is left out of the lines,
//! by git itself: each generator's outputs, which the land's regenerator commits (the client
//! distribution here), and lock files by name; a commit that changed nothing else (the
//! regenerator's own) is no commit. A binary file's change has no lines.
//!
//! One read of every commit with its lines would diff each commit of each branch merged, only to
//! count those lines again with the merge: 15.7 s of git's CPU over two days of this repository
//! (770 commits, 2026-11-12, at load 120), 2.3 s with the paths left to git, where the two reads
//! take 0.24 s.

use crate::project::Project;
use std::path::Path;
use symphony_api::reports::{GitCounts, GitRead, ReportDay};

/// The lock files whose lines are not counted, by file name, wherever they are.
pub const LOCKS: [&str; 13] = [
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "Gemfile.lock",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
];

/// The record of each commit: `hash\x1fparents\x1fcommit time\x1fsubject`, after a `\x1e`.
const FORMAT: &str = "--format=%x1e%H%x1f%P%x1f%ct%x1f%s";

/// Every commit a read walks is shown, those that change only what the pathspecs leave out too
/// (`--full-history --sparse`, which a pathspec would otherwise drop), each with its own parents.
const EVERY: [&str; 3] = ["--full-history", "--sparse", "--no-renames"];

/// One commit of a read: its lines and whether it changed a path outside [`excluded`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    /// Its commit time.
    pub at: f64,
    pub subject: String,
    pub added: u64,
    pub removed: u64,
    /// Whether it changed a path outside [`excluded`]; a merge's own change is never read.
    pub changed: bool,
}

/// The two reads ([`read`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    /// The branch's line of first parents, its tip first, each with its lines.
    pub line: Vec<Commit>,
    /// Every commit the branch holds that its line's first commit's parent does not, with no lines.
    pub all: Vec<Commit>,
}

/// What one commit on the line of first parents brought to the branch, at its commit time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reached {
    pub at: f64,
    /// It is a merge: a landing.
    pub merge: bool,
    /// The commits it brought, itself among them when it is no merge, a commit that changed
    /// nothing outside [`excluded`] and every merge left out.
    pub commits: u32,
    /// Of `commits`, the reverts.
    pub reverts: u32,
    pub added: u64,
    pub removed: u64,
}

/// The paths whose lines are not counted in `project`: each generator's outputs, as the project file
/// names them, then [`LOCKS`].
pub fn excluded(project: &Project) -> Vec<String> {
    let mut outputs: Vec<String> = project
        .land
        .generators
        .iter()
        .chain(&project.verdicts.generators)
        .map(|generator| generator.outputs.clone())
        .filter(|outputs| !outputs.is_empty())
        .collect();
    outputs.sort();
    outputs.dedup();
    outputs.extend(LOCKS.iter().map(|name| (*name).to_string()));
    outputs
}

/// The pathspecs that keep git from reading what `excluded` names: the whole tree but each output,
/// as written, and each lock file at any depth.
fn pathspecs(excluded: &[String]) -> Vec<String> {
    let mut specs = vec![".".to_string()];
    for out in excluded {
        let mut spec = String::from(if LOCKS.contains(&out.as_str()) {
            ":(exclude,glob)**/"
        } else {
            ":(exclude,literal)"
        });
        spec.push_str(out);
        specs.push(spec);
    }
    specs
}

/// `git log` in the repository at `root` of `revs`, `args` before them and `paths` after them,
/// every argument handed over as one list.
fn log(root: &Path, args: &[&str], revs: &[String], paths: &[String]) -> Result<String, String> {
    // The repository is the one at `root`, whatever the server was started from.
    let mut git = crate::children::git();
    let mut argv = vec![
        "-c",
        "core.quotePath=false",
        "-c",
        "log.showSignature=false",
        "log",
        "--no-color",
        FORMAT,
    ];
    argv.extend(EVERY);
    argv.extend(args);
    argv.push("--end-of-options");
    argv.extend(revs.iter().map(String::as_str));
    argv.push("--");
    argv.extend(paths.iter().map(String::as_str));
    let done = symphony_board::sys::output(
        git.env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(root)
            .args(&argv),
    )
    .map_err(|e| format!("git log could not start: {e}"))?;
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr);
        let branch = revs[0].trim_start_matches("refs/heads/");
        return Err(format!("git log of {branch} failed: {}", said.trim()));
    }
    Ok(String::from_utf8_lossy(&done.stdout).into_owned())
}

/// The two reads of `branch` since `since`, what `excluded` names left out: its line of first
/// parents since then with each commit's lines, then every commit it holds that the parent of that
/// line's first commit does not, with the paths each changed. No commit on the line since then
/// reads nothing more.
pub fn read(root: &Path, branch: &str, since: f64, excluded: &[String]) -> Result<Read, String> {
    let tip = format!("refs/heads/{branch}");
    let paths = pathspecs(excluded);
    let since = format!("--since=@{}", since.floor());
    let line = parse(&log(
        root,
        &[
            &since,
            "--first-parent",
            "--diff-merges=first-parent",
            "--numstat",
        ],
        std::slice::from_ref(&tip),
        &paths,
    )?);
    let Some(oldest) = line.last() else {
        return Ok(Read::default());
    };
    let mut revs = vec![tip];
    revs.extend(oldest.parents.first().map(|base| format!("^{base}")));
    let all = parse(&log(root, &["--name-only"], &revs, &paths)?);
    Ok(Read { line, all })
}

/// The rules every agent of a project follows, at its root.
pub const RULES: &str = "RULES.md";

/// The folders that hold skills, a folder each, at the root or under any folder: where a project's
/// agents find them installed (`.agents/skills`, `.claude/skills`) and where a package ships them
/// (`shipped/skills`). A folder of a project's own code named `skills` is none of them.
pub const SKILLS: [&str; 3] = [".agents/skills", ".claude/skills", "shipped/skills"];

/// What a change of `path` changes of what the agents of a project work under, the one list of the
/// files whose changes the Reports page marks: `settings` for the project's settings file
/// ([`crate::project::FILE`]), `rules` for [`RULES`] and `skill NAME` for a file of the skill NAME
/// (anything under a `NAME/` folder of one of [`SKILLS`], its role pages among them); none for
/// every other path, a link to a skill's folder (`.claude/skills/NAME`) among them. A guide
/// (`AGENTS.md`, `CLAUDE.md`) is none: here a third of the landings change one, 69 names in a day
/// beside 12 of skills and one of the settings (2026-11-15), which buried the marks the page is
/// read for.
pub fn instruction(path: &str) -> Option<String> {
    if path == crate::project::FILE {
        return Some("settings".to_string());
    }
    if path == RULES {
        return Some("rules".to_string());
    }
    // The folders alone: the path's last part is the file. A skill's is the first `PLACE/NAME/`
    // among them, PLACE one of `SKILLS`.
    let parts: Vec<&str> = path.split('/').collect();
    let (_file, folders) = parts.split_last()?;
    folders.windows(3).find_map(|three| {
        let placed = SKILLS
            .iter()
            .any(|place| place.split_once('/') == Some((three[0], three[1])));
        placed.then(|| format!("skill {}", three[2]))
    })
}

/// The pathspecs of every path [`instruction`] names, so git reads no other.
fn instructions() -> Vec<String> {
    let mut specs = vec![
        format!(":(literal){}", crate::project::FILE),
        format!(":(literal){RULES}"),
    ];
    specs.extend(SKILLS.iter().map(|place| format!(":(glob)**/{place}/*/**")));
    specs
}

/// A commit that changed what the agents of a project work under.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub commit: Commit,
    /// What it changed, as [`instruction`] names each, once: the settings, the rules, then the
    /// skills by name.
    pub changed: Vec<String>,
}

/// The commits on `branch`'s line of first parents since `since` that changed the project's
/// settings or the instructions its agents work under ([`instruction`]), a merge by what it brought
/// against its first parent, tip first: where the Reports page marks a change of the gates and
/// limits the settings file sets, of a skill or of the rules. One `git log`.
pub fn changes(root: &Path, branch: &str, since: f64) -> Result<Vec<Change>, String> {
    let since = format!("--since=@{}", since.floor());
    let tip = format!("refs/heads/{branch}");
    let text = log(
        root,
        &[
            &since,
            "--first-parent",
            "--diff-merges=first-parent",
            "--name-only",
        ],
        std::slice::from_ref(&tip),
        &instructions(),
    )?;
    // The settings and the rules first, then the rest by name.
    let rank = |name: &String| match name.as_str() {
        "settings" => 0,
        "rules" => 1,
        _ => 2,
    };
    Ok(parse_each(&text, instruction)
        .into_iter()
        .filter(|(_, changed)| !changed.is_empty())
        .map(|(commit, mut changed)| {
            symphony_board::order::sort_by(&mut changed, &|a, b| (rank(a), a).cmp(&(rank(b), b)));
            changed.dedup();
            Change { commit, changed }
        })
        .collect())
}

/// The commits of a read's `text`, in its order: after each record's head, a `--numstat` line
/// (`added\tremoved\tpath`, `-` for a binary file's) or a `--name-only` path is a change.
pub fn parse(text: &str) -> Vec<Commit> {
    parse_each(text, |_| None)
        .into_iter()
        .map(|(commit, _)| commit)
        .collect()
}

/// [`parse`], each commit with what `named` makes of each of its changes' lines.
fn parse_each(text: &str, named: impl Fn(&str) -> Option<String>) -> Vec<(Commit, Vec<String>)> {
    text.split('\x1e')
        .filter_map(|record| {
            let mut names = Vec::new();
            let mut lines = record.lines();
            let mut head = lines.next()?.splitn(4, '\x1f');
            let hash = head.next()?.trim().to_string();
            let parents = head
                .next()?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let at = head.next()?.trim().parse::<f64>().ok()?;
            let subject = head.next().unwrap_or("").to_string();
            let mut commit = Commit {
                hash,
                parents,
                at,
                subject,
                ..Commit::default()
            };
            for line in lines.filter(|line| !line.trim().is_empty()) {
                commit.changed = true;
                let mut fields = line.splitn(3, '\t');
                if let (Some(Ok(added)), Some(Ok(removed))) = (
                    fields.next().map(str::parse::<u64>),
                    fields.next().map(str::parse::<u64>),
                ) {
                    commit.added += added;
                    commit.removed += removed;
                }
                names.extend(named(line));
            }
            (!commit.hash.is_empty()).then_some((commit, names))
        })
        .collect()
}

/// What each commit on `read`'s line of first parents brought to the branch, oldest first: a
/// commit there itself, a merge the commits of what it merged that no commit before it on the line
/// brought (walked back from its other parents through [`Read::all`]). The commits are looked up by
/// a scan, where a map would cost the server's binary its own (docs/workflow/CHECKS.md, size budgets): 8 ms
/// in a debug build for this repository's two days to 2026-11-13 (941 commits, 151 of them on the
/// line), at load 40.
pub fn reached(read: &Read) -> Vec<Reached> {
    let mut seen: Vec<&str> = read.line.iter().map(|c| c.hash.as_str()).collect();
    let counted = |commit: &Commit| commit.parents.len() < 2 && commit.changed;
    let revert = |commit: &Commit| commit.subject.starts_with("Revert \"");
    read.line
        .iter()
        .rev()
        .map(|on| {
            let merge = on.parents.len() > 1;
            let mut reached = Reached {
                at: on.at,
                merge,
                added: on.added,
                removed: on.removed,
                ..Reached::default()
            };
            if counted(on) {
                reached.commits += 1;
                reached.reverts += u32::from(revert(on));
            }
            let mut walk: Vec<&str> = on.parents.iter().skip(1).map(String::as_str).collect();
            while let Some(hash) = walk.pop() {
                if seen.contains(&hash) {
                    continue;
                }
                let Some(commit) = read.all.iter().find(|c| c.hash == hash) else {
                    continue; // the branch held it before the window
                };
                seen.push(hash);
                if counted(commit) {
                    reached.commits += 1;
                    reached.reverts += u32::from(revert(commit));
                }
                walk.extend(commit.parents.iter().map(String::as_str));
            }
            reached
        })
        .collect()
}

/// The landing a merge's subject says, by its slug: the land's own merge ("Merge branch 'SLUG'"),
/// or git's for one made by hand on that branch ("Merge branch 'SLUG' into BRANCH"). A merge of
/// the default branch `branch` itself ("Merge branch 'main' into SLUG": a task catching up, which
/// reached the line), of another repository's branch ("… of URL") or of a remote-tracking branch
/// is none. The release run's cadence counts landings by it (`symphony-build release`).
pub fn landing_slug<'a>(subject: &'a str, branch: &str) -> Option<&'a str> {
    let rest = subject.strip_prefix("Merge branch '")?;
    let (slug, after) = rest.split_once('\'')?;
    let lands = !slug.is_empty() && slug != branch;
    (lands && (after.is_empty() || after.starts_with(" into "))).then_some(slug)
}

/// The tasks that landed on `branch` in `range` (`GOOD..BAD`, the commits a red came between,
/// [`symphony_board::red::between`]), newest first: each merge on the line of first parents from
/// `BAD` back to `GOOD` that the land made, by the slug it landed ([`landing_slug`]), as the
/// release cadence counts landings. A commit the repository does not hold is the error.
pub fn landed(root: &Path, branch: &str, range: &str) -> Result<Vec<String>, String> {
    let merges = log(
        root,
        &["--first-parent", "--merges"],
        &[range.to_string()],
        &[],
    )?;
    Ok(parse(&merges)
        .iter()
        .filter_map(|merge| landing_slug(&merge.subject, branch))
        .map(str::to_string)
        .collect())
}

/// Each red of `day` that names the commits it came between gets the tasks that landed in them
/// ([`landed`]); one whose range git cannot read keeps none, and `problems` says why, once.
pub fn trace(day: &mut ReportDay, root: &Path, branch: &str, problems: &mut Vec<String>) {
    for red in &mut day.reds {
        let Some(range) = &red.between else {
            continue;
        };
        match landed(root, branch, range) {
            Ok(slugs) => red.landed = Some(slugs),
            Err(why) => {
                let said = format!("git: {}: {why}", red.slug);
                if !problems.contains(&said) {
                    problems.push(said);
                }
            }
        }
    }
}

/// Fill `day`'s hours from `read` ([`read`]), `from` naming what it was read from.
pub fn fill(day: &mut ReportDay, read: &Read, from: GitRead) {
    for hour in &mut day.hours {
        hour.git = Some(GitCounts::default());
    }
    for on in reached(read) {
        let hour = symphony_board::reports::hour_at(&mut day.hours, day.until, on.at);
        if let Some(counts) = hour.and_then(|hour| hour.git.as_mut()) {
            counts.commits += on.commits;
            counts.merges += u32::from(on.merge);
            counts.reverts += on.reverts;
            counts.added += on.added;
            counts.removed += on.removed;
        }
    }
    day.git = Some(from);
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
