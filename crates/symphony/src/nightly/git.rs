//! The git the nightly runs, each command in a folder of its choosing, without the session that
//! started it ([`crate::children`]). A command that reaches the remote never waits on a prompt.

use std::path::Path;

use crate::children::command;

/// `git ARGS` in `dir`: what it printed, its trailing newline gone, or what it said when it failed,
/// its lines joined by ` / `.
pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let done = symphony_board::sys::output(
        command("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0"),
    )
    .map_err(|e| format!("cannot run git in {}: {e}", dir.display()))?;
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

/// Whether `ancestor` is `descendant` or one of its ancestors, in the repository at `dir`.
pub(super) fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let done = symphony_board::sys::output(
        command("git")
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .current_dir(dir),
    )
    .map_err(|e| format!("cannot run git in {}: {e}", dir.display()))?;
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
    use super::*;

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

    #[test]
    fn a_short_commit_is_its_first_twelve_characters() {
        assert_eq!(
            short("0123456789abcdef0123456789abcdef01234567"),
            "0123456789ab"
        );
        assert_eq!(short("abc"), "abc");
    }
}
