//! What bounds a land: its timeout over its check, its fetch and its push, and none over its wait
//! for another land; and what its push sends, the checked commit by name, never forced, no tag.

use super::*;
use std::os::unix::fs::PermissionsExt;

/// The project's `[land] timeout_seconds` after the counting check of [`manifest`].
fn timed(seconds: u64) -> String {
    format!("\n[land]\ntimeout_seconds = {seconds}\n")
}

/// An executable script at `dir/name` running `body`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    write(dir, name, &format!("#!/bin/sh\n{body}\n"));
    let path = dir.join(name);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// How many lands, each in a repository of its own, [`refused_past_the_timeout`] is run for before
/// the test says no check reached its first act within the land's 1 s.
const ATTEMPTS: usize = 5;

#[test]
fn a_check_past_the_timeout_is_ended_whole_and_refused() {
    // The land's 1 s runs from the check's start, so a loaded machine may end the check before its
    // shell started its child and wrote down its pid: that land showed the refusal but not that
    // the child ends with it, and the test lands again in a fresh repository. A land that leaves
    // the child running fails whenever the child was started.
    for _ in 1..=ATTEMPTS {
        if let Some(pid) = refused_past_the_timeout() {
            wait_saying(
                || !symphony_board::sys::pid_alive(pid),
                || format!("the check's own child {pid} still runs"),
            );
            return;
        }
    }
    panic!(
        "no check wrote down its child within the land's 1 s in {ATTEMPTS} lands; {}",
        test_time::Load::now()
    );
}

/// A land whose check runs past the land's timeout of 1 s, in a repository of its own, refused
/// and logged as one: the pid of the child the check starts as its first act, which outlives it
/// unless the land ends its whole group, or none where the land ended the check before it wrote
/// that pid down whole.
fn refused_past_the_timeout() -> Option<i64> {
    let r = Repo::new();
    let child = r.dir.join("child.pid");
    // Neither the check nor its child ends by itself while the test runs, however slow the
    // machine, so only the land's ending ends them.
    let check = format!("sleep 600 & echo $! > '{}'; sleep 600", child.display());
    checked_with(&r, &check, &timed(1));
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let log = r.root.join(".git/symphony/tasks/land/sim%2Fjump.log");
    let line = format!(
        "the check `sh -c {check}` ran past the land's timeout ([land] timeout_seconds, 1 s) on \
         {SLUG} merged into main at {}; its output is in {}",
        &before[..7],
        log.canonicalize().unwrap().display()
    );
    assert_refused(&r, &result, &line, &before);
    let logged = fs::read_to_string(&log).unwrap();
    assert!(
        logged.ends_with("ended: it ran past the land's 1 s\n"),
        "{logged}"
    );
    fs::read_to_string(&child)
        .ok()
        .and_then(|text| text.trim().parse::<i64>().ok())
}

#[test]
fn an_origin_that_does_not_answer_is_a_push_to_retry_within_the_timeout() {
    let r = Repo::new();
    // No check, so the 2 s is the fetch's alone: nothing else the land runs is bound by it.
    project_file(&r, &timed(2));
    let bare = remote(&r);
    // An origin that answers only after any wait of the test's, so the fetch ends by its timeout.
    let silent = script(
        &r.dir,
        "silent-upload-pack",
        "sleep 600\nexec git-upload-pack \"$@\"",
    );
    git(
        &r.root,
        &[
            "config",
            "remote.origin.uploadpack",
            &silent.to_string_lossy(),
        ],
    );
    let before = head(&bare, "main");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // The line names the limit that ended the fetch: an origin that answered would push.
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; NOT PUSHED, landed here only: git fetch origin: no \
             answer within 2 s. Push it with: git push origin main",
            &head(&r.root, "main")[..12]
        )
    );
    assert_eq!(head(&bare, "main"), before);
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

#[test]
fn a_land_waits_for_another_past_its_timeout_naming_it_and_lands_once_it_ends() {
    let r = Repo::new();
    manifest(&r, &timed(1));
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let path = r.root.join(".git/symphony/tasks/land.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut held = fs::File::create(&path).unwrap();
    held.lock().unwrap();
    held.write_all(b"the land of sim/fall (pid 4242)").unwrap();
    let before = head(&r.root, "main");
    let said = Arc::new(Mutex::new(Vec::new()));
    let line = std::thread::scope(|scope| {
        let other = r.board.other();
        let env = r.env(&r.root);
        let shared = Shared(Arc::clone(&said));
        let orch = people.orch.clone();
        let waiting = scope.spawn(move || {
            let mut out = shared;
            land(&other, &env, SLUG, &orch, None, &mut out).map_err(refusal)
        });
        wait_for(|| {
            String::from_utf8_lossy(&said.lock().unwrap())
                .contains("waiting for the land of sim/fall (pid 4242) to finish")
        });
        // Past the land's timeout, which bounds its check and not its wait, it waits on.
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!waiting.is_finished(), "it waits with no timeout");
        assert_eq!(head(&r.root, "main"), before);
        drop(held);
        waiting.join().unwrap()
    });
    let merge = head(&r.root, "main");
    assert_eq!(
        line.unwrap(),
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert!(
        String::from_utf8_lossy(&said.lock().unwrap()).starts_with(&format!(
            "land {SLUG}: waiting for the land of sim/fall (pid 4242) to finish\n"
        )),
        "said once"
    );
}

#[test]
fn the_push_sends_the_checked_commit_by_name_never_forced_and_no_tag() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    // Origin refuses any push that is not a fast-forward, and the checkout would push its tags.
    git(&bare, &["config", "receive.denyNonFastForwards", "true"]);
    git(&r.root, &["config", "push.followTags", "true"]);
    git(&r.root, &["tag", "-a", "v1", "-m", "a release"]);
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    git(&wt, &["tag", "-a", "v2", "-m", "on the branch"]);
    let people = approved(&r, SLUG, &format!("{} is good", head(&wt, "HEAD")));
    let gained = pushed_elsewhere(&r, &bare, "sim/other.gd", "extends Node\n");
    // A `git` before the real one on the land's PATH, writing down what it was asked.
    let shims = r.dir.join("shims");
    let asked = r.dir.join("asked");
    script(
        &shims,
        "git",
        &format!(
            "printf '%s\\n' \"$*\" >> '{}'\nPATH=\"${{PATH#*:}}\" exec git \"$@\"",
            asked.display()
        ),
    );
    let mut env = r.env(&r.root);
    let path = format!("{}:{}", shims.display(), env.vars["PATH"]);
    env.vars.insert("PATH".into(), path);
    let mut out = Vec::new();
    let line = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out).unwrap();
    assert!(line.ends_with("; pushed to origin"), "{line}");
    let top = head(&r.root, "main");
    assert_eq!(head(&bare, "main"), top);
    assert_eq!(
        head(&r.root, "main^2"),
        gained,
        "what origin gained is kept, under the push"
    );
    assert_eq!(
        git(&bare, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main",
        "no tag and no other branch"
    );
    let asked = fs::read_to_string(&asked).unwrap();
    let pushes: Vec<&str> = asked.lines().filter(|l| l.starts_with("push")).collect();
    assert_eq!(
        pushes,
        [format!("push --quiet --no-follow-tags origin {top}:refs/heads/main").as_str()],
        "{asked}"
    );
    // Whatever else the push is asked, nothing forces it or adds tags or other branches to it.
    for word in pushes[0].split(' ') {
        assert!(
            !matches!(
                word,
                "-f" | "--force" | "--tags" | "--follow-tags" | "--all" | "--mirror"
            ) && !word.starts_with("--force-with-lease")
                && !word.starts_with('+'),
            "{word}"
        );
    }
}
