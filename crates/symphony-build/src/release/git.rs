//! The git the release run runs, each command in a folder of its choosing, without the session that
//! started it ([`symphony::children`]). A command that reaches the remote never waits on a prompt, and
//! none runs past [`GIT_BOUND`] or the run's own time ([`super::group`]).

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use super::group::{Bound, Failed, fed_until, output_until};

/// How long one git command may run, unless the run's own time ends sooner.
pub(super) const GIT_BOUND: Duration = Duration::from_secs(15 * 60);

#[cfg(test)]
thread_local! {
    /// The git commands this thread started ([`run`]), for a test that bounds a read by their
    /// count rather than its time.
    static RAN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many git commands this thread has started.
#[cfg(test)]
pub(super) fn ran() -> usize {
    RAN.with(std::cell::Cell::get)
}

/// `git ARGS` in `dir` run to its end within [`GIT_BOUND`] and `bound`, on the repository `dir` is
/// in alone: no repository, index or tree the release run's own starter named, as a hook's or a
/// worktree's environment may ([`symphony::children::git`]).
/// With `input`, that is written to its stdin ([`fed_until`]).
fn run(dir: &Path, args: &[&str], input: Option<&str>, bound: Bound<'_>) -> Result<Output, String> {
    #[cfg(test)]
    RAN.with(|ran| ran.set(ran.get() + 1));
    let mut git = symphony::children::git();
    git.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0");
    let bound = bound.sooner(GIT_BOUND);
    match input {
        None => output_until(&mut git, bound),
        Some(input) => fed_until(&mut git, input.as_bytes().to_vec(), bound),
    }
    .map_err(|failed| match failed {
        Failed::Io(e) => format!("cannot run git in {}: {e}", dir.display()),
        Failed::Late(took) => format!(
            "git {} in {} did not end within {} s and was stopped",
            args.join(" "),
            dir.display(),
            took.as_secs()
        ),
        Failed::Interrupted(took) => format!(
            "git {} in {} was stopped after {} s: the run was told to stop",
            args.join(" "),
            dir.display(),
            took.as_secs()
        ),
    })
}

/// `git ARGS` in `dir`: what it printed, its trailing newline gone, or what it said when it failed,
/// its lines joined by ` / `.
pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_until(dir, args, Bound::until(Instant::now() + GIT_BOUND))
}

/// [`git`], stopped when `bound` ends, if that is before [`GIT_BOUND`].
pub(super) fn git_until(dir: &Path, args: &[&str], bound: Bound<'_>) -> Result<String, String> {
    said(dir, args, run(dir, args, None, bound)?)
}

/// [`git`], `input` written to its stdin: for a command that reads what it is asked of there, as
/// `cat-file --batch-check` and `log --stdin` do, however many names that is.
pub(super) fn git_fed(dir: &Path, args: &[&str], input: &str) -> Result<String, String> {
    let bound = Bound::until(Instant::now() + GIT_BOUND);
    said(dir, args, run(dir, args, Some(input), bound)?)
}

/// What `git ARGS`, ended as `done`, printed, or what it said when it failed.
fn said(dir: &Path, args: &[&str], done: Output) -> Result<String, String> {
    if done.status.success() {
        return Ok(String::from_utf8_lossy(&done.stdout)
            .trim_end_matches('\n')
            .to_string());
    }
    // On one line, as a record and `--status` keep it.
    let stderr = String::from_utf8_lossy(&done.stderr);
    let said: Vec<&str> = (stderr.lines().map(str::trim))
        .filter(|line| !line.is_empty())
        .collect();
    Err(format!(
        "git {} failed in {}{}",
        args.join(" "),
        dir.display(),
        if said.is_empty() {
            String::new()
        } else {
            format!(": {}", said.join(" / "))
        }
    ))
}

/// `remote`'s `branch` fetched into its remote-tracking ref (`refs/remotes/REMOTE/BRANCH`) in the
/// repository at `root`, within `bound`: the commit that ref names once fetched. Other processes
/// of the repository write that ref too: a land's or the regenerator's push records there what it
/// pushed, and another fetch what it fetched. Git refuses a fetch whose ref moved after the fetch
/// read it (`cannot lock ref`) rather than write over the newer value. So a fetch that failed
/// while the ref moved under it is run again, to learn where the remote is now. One that failed
/// with the ref where it was is the failure, as is one at the bound's end or after the run was
/// told to stop. Git's words are never read: the ref says whether it moved.
pub(super) fn fetch_line(
    root: &Path,
    remote: &str,
    branch: &str,
    bound: Bound<'_>,
) -> Result<String, String> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let refspec = format!("+refs/heads/{branch}:{tracking}");
    let fetch = ["fetch", "--quiet", "--no-tags", remote, &refspec];
    loop {
        let before = commit(root, &tracking).ok();
        let failed = match git_until(root, &fetch, bound) {
            Ok(_) => return commit(root, &tracking),
            Err(failed) => failed,
        };
        let moved = commit(root, &tracking).ok() != before;
        if !moved || bound.interrupted() || Instant::now() >= bound.until {
            return Err(failed);
        }
    }
}

/// Whether `ancestor` is `descendant` or one of its ancestors, in the repository at `dir`.
pub(super) fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let done = run(
        dir,
        &["merge-base", "--is-ancestor", ancestor, descendant],
        None,
        Bound::until(Instant::now() + GIT_BOUND),
    )?;
    match done.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!(
            "git merge-base --is-ancestor {ancestor} {descendant} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        )),
    }
}

/// The commit `rev` names in the repository at `dir`, in full.
pub(super) fn commit(dir: &Path, rev: &str) -> Result<String, String> {
    git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .map_err(|_| format!("{rev} names no commit in {}", dir.display()))
}

/// The first twelve characters of a commit's name.
pub(super) fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// The tags `git ls-remote --tags` printed, each name with the commit it tags: an annotated tag's
/// peeled line (`NAME^{}`) names its commit, a lightweight tag's own line does.
pub(super) fn remote_tags(listing: &str) -> Vec<(String, String)> {
    let mut tags: Vec<(String, String)> = Vec::new();
    for line in listing.lines() {
        let Some((object, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        match name.strip_suffix("^{}") {
            Some(name) => match tags.iter_mut().find(|(tag, _)| tag == name) {
                Some(entry) => entry.1 = object.to_string(),
                None => tags.push((name.to_string(), object.to_string())),
            },
            None if !tags.iter().any(|(tag, _)| tag == name) => {
                tags.push((name.to_string(), object.to_string()));
            }
            None => {}
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::super::waits::{far, untimed};
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    #[test]
    fn a_remote_tag_names_the_commit_its_peeled_line_names() {
        let listing = "aaa\trefs/tags/release-2026-11-12\n\
                       ccc\trefs/tags/release-2026-11-12^{}\n\
                       ddd\trefs/tags/light\n\
                       eee\trefs/tags/late^{}\n\
                       fff\trefs/tags/late\n\
                       not a tag line\n\
                       ggg\trefs/heads/main\n";
        assert_eq!(
            remote_tags(listing),
            [
                ("release-2026-11-12".to_string(), "ccc".to_string()),
                ("light".to_string(), "ddd".to_string()),
                ("late".to_string(), "eee".to_string()),
            ]
        );
        assert!(remote_tags("").is_empty());
    }

    #[test]
    fn git_answers_what_it_printed_or_why_it_failed_on_one_line() {
        let tree = crate::test_tree::Tree::new();
        let repo = tree
            .write("repo/README.md", "a\n")
            .parent()
            .unwrap()
            .to_path_buf();
        let commit_here = |message: &str| {
            git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.com",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    message,
                ],
            )
        };
        assert_eq!(git(&repo, &["init", "-q", "-b", "main"]), Ok(String::new()));
        assert_eq!(commit_here("one"), Ok(String::new()));
        let first = commit(&repo, "HEAD").unwrap();
        commit_here("two").unwrap();
        let second = commit(&repo, "main").unwrap();
        assert_eq!(first.len(), 40);
        assert_ne!(first, second);
        assert_eq!(git(&repo, &["log", "--format=%s"]), Ok("two\none".into()));

        assert_eq!(is_ancestor(&repo, &first, &second), Ok(true));
        assert_eq!(is_ancestor(&repo, &second, &first), Ok(false));
        let unknown = is_ancestor(&repo, "nothing-here", &first).unwrap_err();
        assert!(
            unknown.contains("merge-base --is-ancestor nothing-here"),
            "{unknown}"
        );
        assert_eq!(
            commit(&repo, "nothing-here"),
            Err(format!(
                "nothing-here names no commit in {}",
                repo.display()
            ))
        );

        let failed = git(&repo, &["push", "/no/such/remote", "main"]).unwrap_err();
        assert!(
            failed.starts_with(&format!(
                "git push /no/such/remote main failed in {}: ",
                repo.display()
            )),
            "{failed}"
        );
        assert!(
            !failed.contains('\n') && failed.contains(" / "),
            "git's lines, joined: {failed}"
        );
    }

    /// A repository `repo/` in `tree` cloned from the bare `origin.git` beside it, whose main is
    /// three commits on, its tracking ref `origin/main` left at the first: those commits, oldest
    /// first.
    fn behind_its_remote(tree: &crate::test_tree::Tree) -> (PathBuf, [String; 3]) {
        let origin = tree.path("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]).unwrap();
        git(
            &tree.root,
            &["clone", "-q", &origin.to_string_lossy(), "repo"],
        )
        .unwrap();
        let repo = tree.path("repo");
        let made = ["one", "two", "three"].map(|message| {
            git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.com",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    message,
                ],
            )
            .unwrap();
            git(&repo, &["push", "-q", "origin", "HEAD:main"]).unwrap();
            commit(&repo, "HEAD").unwrap()
        });
        git(&repo, &["update-ref", "refs/remotes/origin/main", &made[0]]).unwrap();
        (repo, made)
    }

    /// `repo`'s `reference-transaction` hook, in a folder of `tree`'s that its own `core.hooksPath`
    /// names, whatever the machine's configuration names: each transaction that would write
    /// `origin/main` adds a line to `fetches` as it is prepared, before any lock is taken, and
    /// `then` runs, `MOVING` set for what it runs, which the hook lets through.
    fn hooked(tree: &crate::test_tree::Tree, repo: &Path, fetches: &Path, then: &str) {
        let hook = tree.write(
            "hooks/reference-transaction",
            &format!(
                "#!/bin/sh\n[ -z \"$MOVING\" ] || exit 0\n[ \"$1\" = preparing ] || exit 0\n\
                 grep -q ' refs/remotes/origin/main$' || exit 0\necho fetch >> '{}'\n{then}\n",
                fetches.display()
            ),
        );
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hooks = hook.parent().unwrap().to_string_lossy().into_owned();
        git(repo, &["config", "core.hooksPath", &hooks]).unwrap();
    }

    /// A push that records what it pushed in `origin/main` while a fetch of it runs moves the ref
    /// after the fetch read it, and git refuses the fetch's write over it: the fetch is run again,
    /// once, and answers where the remote's main is now.
    #[test]
    fn a_fetch_whose_ref_another_writes_meanwhile_is_fetched_again_and_names_the_remotes_tip() {
        let tree = crate::test_tree::Tree::new();
        let (repo, [_, second, third]) = behind_its_remote(&tree);
        let (fetches, moved) = (tree.path("fetches"), tree.path("moved"));
        hooked(
            &tree,
            &repo,
            &fetches,
            &format!(
                "[ -e '{moved}' ] && exit 0\ntouch '{moved}'\n\
                 MOVING=1 git update-ref refs/remotes/origin/main {second}",
                moved = moved.display()
            ),
        );
        assert_eq!(
            fetch_line(&repo, "origin", "main", far()),
            Ok(third.clone())
        );
        assert!(
            moved.exists(),
            "the hook moved the ref under the first fetch: a git that runs the \
             reference-transaction hook as a transaction is prepared"
        );
        assert_eq!(
            tree.read("fetches"),
            "fetch\nfetch\n",
            "the first refused, the second through"
        );
        assert_eq!(commit(&repo, "refs/remotes/origin/main"), Ok(third));
    }

    /// A fetch that fails with its ref where it was is the answer at once, git's words in it.
    #[test]
    fn a_fetch_that_fails_with_its_ref_unmoved_is_the_failure() {
        let tree = crate::test_tree::Tree::new();
        let (repo, [first, ..]) = behind_its_remote(&tree);
        let fetches = tree.path("fetches");
        hooked(&tree, &repo, &fetches, "");
        let failed = fetch_line(&repo, "origin", "gone", far()).unwrap_err();
        assert!(
            failed.starts_with(&format!(
                "git fetch --quiet --no-tags origin +refs/heads/gone:refs/remotes/origin/gone \
                 failed in {}: ",
                repo.display()
            )),
            "{failed}"
        );
        let nowhere = fetch_line(&repo, "nowhere", "main", far()).unwrap_err();
        assert!(nowhere.contains("nowhere"), "{nowhere}");
        assert!(!fetches.exists(), "neither came to write origin/main");
        assert_eq!(commit(&repo, "refs/remotes/origin/main"), Ok(first));
    }

    /// A ref moved under every fetch holds the run no longer than its bound, nor past its stop.
    #[test]
    fn a_ref_that_keeps_moving_is_fetched_no_further_than_the_bound_or_the_stop() {
        let tree = crate::test_tree::Tree::new();
        let (repo, [first, second, _]) = behind_its_remote(&tree);
        let fetches = tree.path("fetches");
        hooked(
            &tree,
            &repo,
            &fetches,
            &format!(
                "at=$(git rev-parse refs/remotes/origin/main)\n\
                 if [ \"$at\" = {first} ]; then to={second}; else to={first}; fi\n\
                 MOVING=1 git update-ref refs/remotes/origin/main \"$to\""
            ),
        );
        // How many fetches came to write `origin/main`: one stopped first came to none.
        let fetched = || std::fs::read_to_string(&fetches).map_or(0, |text| text.lines().count());
        // Told to stop: the fetch it ran, refused or stopped, is the answer, and none runs again.
        let told = || true;
        let stopped = fetch_line(&repo, "origin", "main", Bound::hearing(far().until, &told));
        assert!(stopped.is_err(), "{stopped:?}");
        assert!(fetched() <= 1, "one fetch at most: {}", fetched());
        // Its time is up before it starts: the same.
        let before = fetched();
        let late = fetch_line(&repo, "origin", "main", Bound::until(Instant::now()));
        assert!(late.is_err(), "{late:?}");
        assert!(fetched() <= before + 1, "one fetch at most: {}", fetched());
        // A bound that ends while the ref keeps moving under every fetch ends the fetching.
        let bound = Bound::until(Instant::now() + Duration::from_millis(500));
        assert!(fetch_line(&repo, "origin", "main", bound).is_err());
    }

    #[test]
    fn a_git_past_its_time_is_stopped_and_says_so() {
        let tree = crate::test_tree::Tree::new();
        // Its time is up before it starts, so the first look stops it.
        let late = git_until(
            &tree.root,
            &["-c", "alias.hang=!sleep 30", "hang"],
            Bound::until(Instant::now()),
        )
        .unwrap_err();
        assert_eq!(
            untimed(&late),
            untimed(&format!(
                "git -c alias.hang=!sleep 30 hang in {} did not end within 0 s and was stopped",
                tree.root.display()
            ))
        );
        let told = || true;
        let stopped = git_until(
            &tree.root,
            &["-c", "alias.hang=!sleep 30", "hang"],
            Bound::hearing(far().until, &told),
        )
        .unwrap_err();
        assert_eq!(
            untimed(&stopped),
            untimed(&format!(
                "git -c alias.hang=!sleep 30 hang in {} was stopped after 0 s: the run was told \
                 to stop",
                tree.root.display()
            ))
        );
    }

    #[test]
    fn a_short_commit_is_its_first_twelve_characters() {
        assert_eq!(
            short("0123456789abcdef0123456789abcdef01234567"),
            "0123456789ab"
        );
        assert_eq!(short("abc"), "abc");
    }
}
