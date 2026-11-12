//! What bounds a land: its timeout over each generator and check, its fetch and its push, and its
//! wait for another land; and what its push sends, the checked commit by name, never forced, no
//! tag.

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

#[test]
fn a_check_past_the_timeout_is_ended_whole_and_refused() {
    let r = Repo::new();
    let mark = r.dir.join("outlived");
    // The check's own child outlives it unless the whole group ends.
    let check = format!("(sleep 3; touch '{}') & sleep 30", mark.display());
    checked_with(&r, &check, &timed(1));
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let began = Instant::now();
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "{:?}",
        began.elapsed()
    );
    let log = r.root.join(".git/sf-tasks/land/sim%2Fjump.log");
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
    std::thread::sleep(Duration::from_secs(4));
    assert!(!mark.exists(), "the check's own child was ended with it");
}

#[test]
fn a_generator_past_the_timeout_is_refused() {
    let r = Repo::new();
    manifest(
        &r,
        "\n[land]\ntimeout_seconds = 1\ngenerators = [\n  { name = \"the slow one\", outputs = \"gen/\", command = [\"sleep\", \"30\"] },\n]\n",
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let began = Instant::now();
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "{:?}",
        began.elapsed()
    );
    let log = r.root.join(".git/sf-tasks/land/sim%2Fjump.log");
    let line = format!(
        "the generator the slow one ran past the land's timeout ([land] timeout_seconds, 1 s) on \
         {SLUG} merged into main; its output is in {}",
        log.canonicalize().unwrap().display()
    );
    assert_refused(&r, &result, &line, &before);
    assert!(
        runs(&r).is_empty(),
        "no check after a generator that did not finish"
    );
}

#[test]
fn an_origin_that_does_not_answer_is_a_push_to_retry_within_the_timeout() {
    let r = Repo::new();
    manifest(&r, &timed(2));
    let bare = remote(&r);
    let silent = script(
        &r.dir,
        "silent-upload-pack",
        "sleep 30\nexec git-upload-pack \"$@\"",
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
    let began = Instant::now();
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(30),
        "{:?}",
        began.elapsed()
    );
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
fn a_land_waits_for_another_a_bounded_time_naming_it() {
    let r = Repo::new();
    manifest(&r, &timed(1));
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let path = r.root.join(".git/sf-tasks/land.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut held = fs::File::create(&path).unwrap();
    held.lock().unwrap();
    held.write_all(b"the land of sim/fall (pid 4242)").unwrap();
    let before = head(&r.root, "main");
    let began = Instant::now();
    let (said, result) = landed(&r, SLUG, &people.orch);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(
        said,
        format!("land {SLUG}: waiting for the land of sim/fall (pid 4242) to finish\n")
    );
    assert_eq!(
        result.unwrap_err(),
        format!(
            "the land of sim/fall (pid 4242) has held {} for 1 s, this land's timeout: run the \
             land of {SLUG} again once it ends",
            path.canonicalize().unwrap().display()
        )
    );
    assert_eq!(head(&r.root, "main"), before);
    // Once the other ends, it lands.
    drop(held);
    landed(&r, SLUG, &people.orch).1.unwrap();
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
