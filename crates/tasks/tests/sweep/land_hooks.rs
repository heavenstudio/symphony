//! The project's git hooks on a land's merge: those git runs on a merge committed after `git merge
//! --no-commit` (pre-commit, prepare-commit-msg, commit-msg), from the repository's hooks folder
//! or `core.hooksPath`. One that refuses refuses the land as a red check does, naming the hooks,
//! main unmoved; hooks that pass leave the land reading as before.

use super::*;
use std::os::unix::fs::PermissionsExt;

/// An executable hook `name` in the folder `hooks` running `body`.
fn hook(hooks: &Path, name: &str, body: &str) {
    write(hooks, name, &format!("#!/bin/sh\n{body}\n"));
    fs::set_permissions(hooks.join(name), fs::Permissions::from_mode(0o755)).unwrap();
}

/// A hook `name` in the repository's own hooks folder that says it refuses, and does.
fn refusing(r: &Repo, name: &str) {
    hook(
        &r.root.join(".git/hooks"),
        name,
        &format!("echo '{name} says no' >&2; exit 1"),
    );
}

/// The land's log, as its refusals name it.
fn log(r: &Repo) -> PathBuf {
    r.root
        .join(".git/symphony/tasks/land/sim%2Fjump.log")
        .canonicalize()
        .unwrap()
}

/// What a land refused by `by`, `did`, says of the merge of [`SLUG`] into main at `before`.
fn refused_by(r: &Repo, by: &str, did: &str, before: &str) -> String {
    format!(
        "{by} {did} the merge of {SLUG} into main at {}; its output is in {}",
        &before[..7],
        log(r).display()
    )
}

#[test]
fn a_refusing_hook_refuses_the_land_naming_it_main_unmoved_and_the_approve_standing() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    for name in ["pre-commit", "pre-merge-commit", "commit-msg"] {
        refusing(&r, name);
    }
    let before = head(&r.root, "main");
    let (said, result) = landed(&r, SLUG, &people.orch);
    // git ran the first hook, which refused: the line names those a merge's commit runs.
    let line = refused_by(
        &r,
        "one of the project's git hooks (pre-commit, commit-msg)",
        "refused",
        &before,
    );
    assert_refused(&r, &result, &line, &before);
    assert!(runs(&r).is_empty(), "no check runs on a refused merge");
    assert!(!said.contains("checking"), "{said}");
    let logged = fs::read_to_string(log(&r)).unwrap();
    assert!(logged.contains("pre-commit says no\n"), "{logged}");
    assert!(!logged.contains("commit-msg says no"), "{logged}");
    // Said to the executor as a red check is, the task where it stood and its approve standing.
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        told.text,
        format!(
            "land refused: {line}. Merge main into {SLUG} and fix it there, then move it back to \
             review (`just tasks move {SLUG} review`) and ask its reviewer to approve the new tip \
             naming it"
        )
    );
    assert_eq!(told.recipient.as_deref(), Some("@executor"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    // One hook alone is named as the one.
    fs::remove_file(r.root.join(".git/hooks/commit-msg")).unwrap();
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = refused_by(&r, "the project's git hook pre-commit", "refused", &before);
    assert_refused(&r, &result, &line, &before);
    // A hook git does not run (not executable) refuses nothing, and pre-merge-commit is `git
    // merge`'s own: the merge lands, as approved.
    let pre_commit = r.root.join(".git/hooks/pre-commit");
    fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o644)).unwrap();
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    let merge = head(&r.root, "main");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
}

#[test]
fn hooks_are_found_where_core_hooks_path_names_them_relative_to_the_merge() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // Main holds its hooks in a folder of its own, which the land's merge holds too; the
    // repository's own hooks folder is then not git's.
    hook(
        &r.root.join(".githooks"),
        "commit-msg",
        "grep -q '^Merge branch' \"$1\" && echo 'no merges here' >&2 && exit 1; exit 0",
    );
    git(&r.root, &["add", ".githooks"]);
    git(&r.root, &["commit", "-qm", "the project's hooks"]);
    git(&r.root, &["config", "core.hooksPath", ".githooks"]);
    refusing(&r, "pre-commit");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = refused_by(&r, "the project's git hook commit-msg", "refused", &before);
    assert_refused(&r, &result, &line, &before);
    let logged = fs::read_to_string(log(&r)).unwrap();
    assert!(logged.contains("no merges here\n"), "{logged}");
    assert!(!logged.contains("pre-commit says no"), "{logged}");
    // An absolute folder is found as well.
    let elsewhere = r.dir.join("hooks");
    hook(&elsewhere, "prepare-commit-msg", "exit 3");
    git(
        &r.root,
        &["config", "core.hooksPath", &elsewhere.to_string_lossy()],
    );
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = refused_by(
        &r,
        "the project's git hook prepare-commit-msg",
        "refused",
        &before,
    );
    assert_refused(&r, &result, &line, &before);
}

#[test]
fn hooks_that_pass_run_on_the_merge_and_the_land_reads_as_before() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    let heard = r.dir.join("heard");
    let hooks = r.root.join(".git/hooks");
    hook(
        &hooks,
        "pre-commit",
        &format!(
            "echo \"pre-commit $(pwd -P) $TASKS_TASK\" >> '{}'",
            heard.display()
        ),
    );
    hook(
        &hooks,
        "commit-msg",
        &format!("echo \"commit-msg $(cat \"$1\")\" >> '{}'", heard.display()),
    );
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    let merge = head(&r.root, "main");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    // Each ran once, on the merge in the land's worktree, for the task.
    let heard = fs::read_to_string(&heard).unwrap();
    let lines: Vec<&str> = heard.lines().collect();
    assert_eq!(lines.len(), 2, "{heard}");
    assert!(lines[0].starts_with("pre-commit "), "{heard}");
    assert!(
        lines[0].ends_with(&format!(".git/symphony/tasks/land/worktree {SLUG}")),
        "{heard}"
    );
    assert_eq!(lines[1], format!("commit-msg Merge branch '{SLUG}'"));
    // The land reads as it did: checked, its record naming where.
    assert!(said.contains("checking (sh -c "), "{said}");
    assert_eq!(runs(&r).len(), 1);
    assert!(
        r.board.said(SLUG).contains(&format!(
            "land {SLUG}: merged {tip} as {merge}, checked at {merge}; moving main there"
        )),
        "{:?}",
        r.board.said(SLUG)
    );
    assert_eq!(head(&r.root, "main^2"), tip);
}

#[test]
fn a_commit_that_fails_with_no_hook_is_named_as_git_commits() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // A signature its program refuses: the commit fails with no hook to blame.
    git(&r.root, &["config", "commit.gpgsign", "true"]);
    git(&r.root, &["config", "gpg.program", "false"]);
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = refused_by(&r, "git commit", "failed on", &before);
    assert_refused(&r, &result, &line, &before);
    assert!(runs(&r).is_empty());
}

#[test]
fn a_hook_past_the_timeout_is_ended_and_refused() {
    let r = Repo::new();
    manifest(&r, "\n[land]\ntimeout_seconds = 1\n");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // It never ends by itself while the test runs, so only the land's ending ends it.
    hook(&r.root.join(".git/hooks"), "pre-commit", "sleep 600");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = refused_by(
        &r,
        "the project's git hook pre-commit",
        "ran past the land's timeout ([land] timeout_seconds, 1 s) on",
        &before,
    );
    assert_refused(&r, &result, &line, &before);
    let logged = fs::read_to_string(log(&r)).unwrap();
    assert!(
        logged.ends_with("ended: it ran past the land's 1 s\n"),
        "{logged}"
    );
}
