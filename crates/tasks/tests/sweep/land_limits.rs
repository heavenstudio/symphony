//! What bounds a land: its timeout over its check and fetch, and none over its wait for another
//! land; and what its push sends, the checked commit by name, never forced, no tag.

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use symphony_board::land::land_with_clock;
use symphony_board::sys;

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

/// A whole pid line, once the shell has written it (creating the file comes first).
pub(super) fn written_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()
        .filter(|text| text.ends_with('\n'))?
        .trim()
        .parse()
        .ok()
}

/// End only this test's check group if an assertion fails or its land stalls.
pub(super) struct CheckGroup(pub(super) PathBuf);

impl CheckGroup {
    pub(super) fn end(&self) {
        if let Some(group) = written_pid(&self.0).filter(|&group| sys::group_alive(group)) {
            // SAFETY: this group belongs to the check this test started, recorded by its shell.
            unsafe { libc::kill(-i32::try_from(group).unwrap(), libc::SIGKILL) };
        }
    }
}

impl Drop for CheckGroup {
    fn drop(&mut self) {
        self.end();
    }
}

#[test]
fn a_check_past_the_timeout_is_ended_whole_and_refused() {
    let r = Repo::new();
    let child = r.dir.join("child.pid");
    let group = r.dir.join("check.pid");
    // Both processes ignore TERM; the child reports readiness after installing that trap, so
    // the timeout must reach the watchdog's KILL path. Neither has a wall-clock end of its own.
    let background = script(
        &r.dir,
        "background",
        &format!(
            "trap '' TERM\necho $$ > '{}'\nwhile :; do sleep 600; done",
            child.display()
        ),
    );
    let check = format!(
        "trap '' TERM; echo $$ > '{}'; '{}' & wait",
        group.display(),
        background.display()
    );
    checked_with(&r, &check, &timed(1));
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let began = Instant::now();
    let (expired, aborted) = (AtomicBool::new(false), AtomicBool::new(false));
    let (ready_reads, abort_ticks) = (AtomicU64::new(0), AtomicU64::new(0));
    let clock = || {
        if written_pid(&child).is_some() {
            ready_reads.fetch_add(1, Ordering::SeqCst);
        }
        let seconds = if aborted.load(Ordering::SeqCst) {
            // Even a command starting after a stalled setup must expire, so the scoped land
            // can be joined before the stall is reported.
            abort_ticks.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            u64::from(expired.load(Ordering::SeqCst))
        };
        began + Duration::from_secs(seconds)
    };
    let cleanup = CheckGroup(group.clone());
    let (pid, result) = std::thread::scope(|scope| {
        let store = r.board.other();
        let env = r.env(&r.root);
        let orch = &people.orch;
        let clock = &clock;
        let landing = scope.spawn(move || {
            land_with_clock(&store, &env, SLUG, orch, None, &mut Vec::new(), clock).map_err(refusal)
        });
        let ready = test_time::until(
            "the check's child is ready and its watchdog has read the frozen clock twice",
            test_time::backstop(Duration::from_secs(60)),
            || written_pid(&child).filter(|_| ready_reads.load(Ordering::SeqCst) >= 2),
        );
        // Two reads after readiness include the watchdog's deadline and first look, even if
        // it started after the child: the merge can never consume this command's timeout.
        aborted.store(ready.is_err(), Ordering::SeqCst);
        expired.store(true, Ordering::SeqCst);
        let ended = test_time::until_true(
            "the land refused the expired check",
            test_time::backstop(Duration::from_secs(60)),
            || landing.is_finished(),
        );
        if ended.is_err() {
            cleanup.end();
            aborted.store(true, Ordering::SeqCst);
        }
        let result = landing.join().unwrap();
        ended.unwrap();
        (ready.unwrap(), result)
    });
    let log = r.root.join(".git/symphony/tasks/land/sim%2Fjump.log");
    let shown = log.canonicalize().unwrap();
    let line = format!(
        "the check `sh -c {check}` ran past the land's timeout ([land] timeout_seconds, 1 s) on \
         {SLUG} merged into main at {}; its output is in {}",
        &before[..7],
        shown.display()
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    let logged = fs::read_to_string(&log).unwrap();
    assert!(
        logged.ends_with("ended: it ran past the land's 1 s\n"),
        "{logged}"
    );
    let group = written_pid(&group).unwrap();
    test_time::until_true(
        format!("the timed-out check's child {pid} and whole group {group} are gone"),
        test_time::backstop(Duration::from_secs(10)),
        || !sys::pid_alive(i64::from(pid)) && !sys::group_alive(group),
    )
    .unwrap();
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
    // The land's clock, an hour past its timeout once it waits: its wait reads no clock, and each
    // of its own commands counts its second from the hour it starts in, so none can run past it
    // however long a loaded machine takes over it.
    let began = Instant::now();
    let past = AtomicBool::new(false);
    let clock = || {
        let hours = u64::from(past.load(Ordering::SeqCst));
        began + Duration::from_secs(3600 * hours)
    };
    let line = std::thread::scope(|scope| {
        let other = r.board.other();
        let env = r.env(&r.root);
        let shared = Shared(Arc::clone(&said));
        let orch = people.orch.clone();
        let clock = &clock;
        let waiting = scope.spawn(move || {
            let mut out = shared;
            land_with_clock(&other, &env, SLUG, &orch, None, &mut out, clock).map_err(refusal)
        });
        wait_for(|| {
            String::from_utf8_lossy(&said.lock().unwrap())
                .contains("waiting for the land of sim/fall (pid 4242) to finish")
        });
        past.store(true, Ordering::SeqCst);
        // Past the land's timeout, which bounds its check and not its wait, it waits on.
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a test holds past the land's timeout to show the land still waits, where a clock the land reads is the event"
        )]
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
