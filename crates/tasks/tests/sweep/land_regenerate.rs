//! The regenerator (`tasks regenerate`), which a land starts detached: what it makes of main's
//! newest tip, the one commit it lands in seconds under the land's lock, what it skips by a key it
//! passed on before, what it does while main moves, the red task it files and closes, the one that
//! runs at a time, the check no land runs that it runs after each round, and the status it leaves.

use super::*;
use symphony_board::land::regenerate::{
    self, CHECKED, Checked, LOCK as REGENERATING, RED, RED_CHECK, State,
};
use symphony_board::sweep::trigger::LOG_LIMIT;

/// The regenerator run from the main checkout, as a land starts it: what it said, and its last
/// line or why it stopped.
fn regenerated(r: &Repo) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let result = regenerate::run(&r.board.store, &r.env(&r.root), &mut out);
    (String::from_utf8(out).unwrap(), result)
}

/// The lands' folder.
fn lands(r: &Repo) -> PathBuf {
    r.root.join(".git/symphony/tasks/land")
}

/// Where the regenerator stands, and whether main's outputs are current.
fn status(r: &Repo) -> (String, bool) {
    let standing = regenerate::status(&r.env(&r.root)).unwrap();
    (standing.line, standing.current)
}

/// Whether the regenerator that last wrote its state still runs.
fn making(r: &Repo) -> bool {
    regenerate::status(&r.env(&r.root)).unwrap().making
}

/// A generator as a TOML inline table: `name`, writing `outputs`, by `sh -c script`, after `more`.
fn generator(name: &str, outputs: &str, script: &str, more: &str) -> String {
    format!(
        "{{ name = \"{name}\", outputs = \"{outputs}\", command = [\"sh\", \"-c\", {}]{more} }}",
        toml_string(script)
    )
}

/// The project file with the one generator `generator`, called `name`, its key `sh -c script`,
/// named in `[land] keys`.
fn keyed(r: &Repo, generator: &str, name: &str, script: &str) {
    manifest(
        r,
        &format!(
            "\n[land]\ngenerators = [\n  {generator}\n]\nkeys = {{ \"{name}\" = [\"sh\", \"-c\", {}] }}\n",
            toml_string(script)
        ),
    );
}

/// The index of `sim/`, counting its runs in `made` beside the repository.
fn index(r: &Repo) -> String {
    generator(
        "the index",
        "gen/",
        &format!(
            "echo made >> '{}'; mkdir -p gen && ls sim > gen/index",
            r.dir.join("made").display()
        ),
        "",
    )
}

/// How many times a counting generator ran.
fn made(r: &Repo) -> usize {
    fs::read_to_string(r.dir.join("made"))
        .unwrap_or_default()
        .lines()
        .count()
}

/// A commit on main in the main checkout writing `file` as `text`: the commit.
fn on_main(r: &Repo, file: &str, text: &str) -> String {
    write(&r.root, file, text);
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", &format!("{file} on main")]);
    head(&r.root, "main")
}

fn subject(r: &Repo, rev: &str) -> String {
    git(&r.root, &["log", "-1", "--format=%s", rev])
}

/// How long a stand-in build waits for the test before it gives up and fails: the backstop of the
/// minute it once had, so a loaded machine's slow lands never run it out, while a land that waited
/// for it still fails, only later.
const STAND_IN_WAIT: Duration = test_time::backstop(Duration::from_secs(60));

/// How many looks `every` apart a stand-in's shell loop takes in [`STAND_IN_WAIT`].
fn looks(every: Duration) -> u128 {
    STAND_IN_WAIT.as_millis() / every.as_millis()
}

#[test]
fn a_round_makes_the_outputs_of_mains_tip_and_lands_them_as_one_commit() {
    let r = Repo::new();
    let summary = generator(
        "the summary",
        "summary/",
        "mkdir -p summary && wc -l < gen/index | tr -d ' ' > summary/count",
        ", when_changed = true",
    );
    generating(&r, &[&index(&r), &summary]);
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    let top = head(&r.root, "main");
    assert_eq!(
        result.unwrap(),
        format!(
            "Regenerate the index and the summary for main at {}: landed on main at {}; not \
             pushed: no origin remote",
            &tip[..7],
            &top[..12]
        )
    );
    // One commit on the tip, of the generators' outputs alone.
    assert_eq!(head(&r.root, "main^"), tip);
    assert_eq!(
        subject(&r, "main"),
        format!(
            "Regenerate the index and the summary for main at {}",
            &tip[..7]
        )
    );
    assert_eq!(
        git(&r.root, &["diff", "--name-only", "main^", "main"]),
        "gen/index\nsummary/count"
    );
    assert_eq!(git(&r.root, &["show", "main:gen/index"]), "world.gd");
    assert_eq!(git(&r.root, &["show", "main:summary/count"]), "1");
    // The main checkout moved with it and is clean; the land's check never ran.
    assert_eq!(head(&r.root, "HEAD"), top);
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
    assert!(runs(&r).is_empty());
    assert!(
        said.contains(&format!(
            "regenerate: making the index for main at {} (sh -c ",
            &tip[..7]
        )),
        "{said}"
    );
    // Its worktree is kept, locked from the sweep, for the next round's warm builds.
    let kept = lands(&r).join("regenerate-worktree");
    assert!(kept.is_dir());
    assert!(
        git(&r.root, &["worktree", "list", "--porcelain"]).contains("locked symphony regenerate"),
        "{}",
        git(&r.root, &["worktree", "list", "--porcelain"])
    );
    let state = State::read(&lands(&r)).unwrap();
    assert_eq!(
        (
            state.state.as_str(),
            state.tip.as_str(),
            state.landed.as_deref()
        ),
        ("current", tip.as_str(), Some(top.as_str()))
    );
    let (line, current) = status(&r);
    assert!(current, "{line}");
    assert!(
        line.starts_with(&format!(
            "the index and the summary: current on main at {} (judged ",
            &top[..7]
        )),
        "{line}"
    );
    // Run again on what it landed: a tip it finished with is made again only once main moves.
    let (said, again) = regenerated(&r);
    assert_eq!(
        again.unwrap(),
        format!(
            "the index and the summary: current on main at {}",
            &top[..7]
        )
    );
    assert_eq!(said, "");
    assert_eq!(head(&r.root, "main"), top);
    assert_eq!(made(&r), 1);
}

#[test]
fn a_round_pushes_what_it_landed_to_origin_alone() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let bare = remote(&r);
    git(&r.root, &["tag", "v1"]);
    let line = regenerated(&r).1.unwrap();
    let top = head(&r.root, "main");
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert_eq!(head(&bare, "main"), top);
    assert_eq!(
        git(&bare, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main"
    );
}

#[test]
fn a_key_and_outputs_it_passed_on_before_make_nothing_and_a_new_key_makes_again() {
    let r = Repo::new();
    let bundle = generator(
        "the bundle",
        "gen/",
        &format!(
            "echo made >> '{}'; mkdir -p gen && cat src/*.txt > gen/out.txt",
            r.dir.join("made").display()
        ),
        "",
    );
    keyed(
        &r,
        &bundle,
        "the bundle",
        "echo reading; cat src/*.txt | cksum",
    );
    on_main(&r, "src/a.txt", "a\n");
    regenerated(&r).1.unwrap();
    assert_eq!(made(&r), 1);
    // Main moves with no source of the bundle's: the key and the outputs are a pair it passed on.
    let moved = on_main(&r, "sim/other.gd", "extends Node\n");
    let (said, result) = regenerated(&r);
    assert_eq!(
        result.unwrap(),
        format!("the bundle: current on main at {}", &moved[..7])
    );
    assert_eq!(made(&r), 1, "{said}");
    assert!(
        said.contains(&format!(
            "regenerate: the bundle passed before on what main at {} holds (its key ",
            &moved[..7]
        )) && said.contains("): nothing to make"),
        "{said}"
    );
    assert_eq!(head(&r.root, "main"), moved);
    assert!(status(&r).1);
    // A source changed: a new key, made again.
    let changed = on_main(&r, "src/b.txt", "b\n");
    regenerated(&r).1.unwrap();
    assert_eq!(made(&r), 2);
    assert_eq!(head(&r.root, "main^"), changed);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a\nb");
    // Outputs changed by hand under the same key: not a pair it passed on, made again.
    on_main(&r, "gen/out.txt", "by hand\n");
    regenerated(&r).1.unwrap();
    assert_eq!(made(&r), 3);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a\nb");
}

#[test]
fn a_key_that_fails_or_prints_nothing_makes_the_generator_run() {
    let r = Repo::new();
    let bundle = generator(
        "the bundle",
        "gen/",
        &format!("echo made >> '{}'; true", r.dir.join("made").display()),
        "",
    );
    keyed(
        &r,
        &bundle,
        "the bundle",
        "test -e src/key || exit 4; cat src/key",
    );
    let (said, _) = regenerated(&r);
    assert!(
        said.contains("regenerate: the bundle's key (sh -c ")
            && said.contains(" failed: making it"),
        "{said}"
    );
    on_main(&r, "src/key", "");
    let (said, _) = regenerated(&r);
    assert!(said.contains(" printed nothing: making it"), "{said}");
    assert_eq!(made(&r), 2);
}

#[test]
fn an_input_main_gained_during_a_build_makes_it_stale_and_the_newest_tip_is_made_next() {
    let r = Repo::new();
    // The first time it runs, the generator commits on main in the main checkout, outside its
    // outputs: main gains an input while the round makes the index of the tip before.
    let moved = r.dir.join("moved");
    let script = format!(
        "echo made >> '{made}'; mkdir -p gen && ls sim > gen/index; if [ ! -e '{moved}' ]; then \
         touch '{moved}'; echo x > '{root}/sim/moved.gd'; git -C '{root}' add sim/moved.gd; git \
         -C '{root}' commit -qm 'main moved'; fi",
        made = r.dir.join("made").display(),
        moved = moved.display(),
        root = r.root.display()
    );
    generating(&r, &[&generator("the index", "gen/", &script, "")]);
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    let line = result.unwrap();
    // The first build ran to its end and was discarded; one commit, of the newest tip's index.
    let gained = head(&r.root, "main^");
    assert_eq!(subject(&r, &gained), "main moved");
    assert_eq!(head(&r.root, "main^^"), tip);
    assert_eq!(made(&r), 2);
    assert_eq!(
        git(&r.root, &["log", "--format=%s", &format!("{tip}..main")]),
        format!(
            "Regenerate the index for main at {}\nmain moved",
            &gained[..7]
        )
    );
    assert_eq!(
        git(&r.root, &["show", "main:gen/index"]),
        "moved.gd\nworld.gd"
    );
    assert!(
        said.contains(&format!(
            "regenerate: main moved to {} since {}, and changed sim/moved.gd, which the index may \
             read (it has no key): what was made of {} is stale, discarded and not committed; the \
             newest tip is made next\nregenerate: the default branch moved; going round",
            &gained[..7],
            &tip[..7],
            &tip[..7]
        )),
        "{said}"
    );
    assert_eq!(
        line,
        format!(
            "Regenerate the index for main at {}: landed on main at {}; not pushed: no origin \
             remote",
            &gained[..7],
            &head(&r.root, "main")[..12]
        )
    );
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
    assert!(status(&r).1);
}

#[test]
fn a_key_says_whether_what_main_gained_during_a_build_is_one_of_its_inputs() {
    let r = Repo::new();
    // The first build of each tip commits `NEXT` on main while it runs, once: outside the bundle's
    // sources the first time, one of them the second.
    let next = r.dir.join("next");
    let script = format!(
        "echo made >> '{made}'; mkdir -p gen && cat src/*.txt > gen/out.txt; if [ -e '{next}' ]; \
         then sh '{next}'; rm -f '{next}'; fi",
        made = r.dir.join("made").display(),
        next = next.display()
    );
    keyed(
        &r,
        &generator("the bundle", "gen/", &script, ""),
        "the bundle",
        "cat src/*.txt | cksum",
    );
    on_main(&r, "src/a.txt", "a\n");
    let commit = |file: &str| {
        format!(
            "echo x > '{root}/{file}'; git -C '{root}' add -A; git -C '{root}' commit -qm '{file}'",
            root = r.root.display()
        )
    };
    // Main gains a file the key does not read: what was made lands on it, made once.
    fs::write(&next, commit("sim/other.gd")).unwrap();
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    result.unwrap();
    assert_eq!(made(&r), 1, "{said}");
    assert_eq!(subject(&r, "main^"), "sim/other.gd");
    assert_eq!(
        subject(&r, "main"),
        format!("Regenerate the bundle for main at {}", &tip[..7])
    );
    assert_eq!(
        git(&r.root, &["diff", "--name-only", "main^", "main"]),
        "gen/out.txt"
    );
    assert!(status(&r).1, "{}", status(&r).0);
    // Main gains a source: the key there differs, what was made is stale, the newest tip is made.
    on_main(&r, "src/b.txt", "b\n");
    fs::write(&next, commit("src/c.txt")).unwrap();
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    result.unwrap();
    assert_eq!(made(&r), 3, "{said}");
    let gained = head(&r.root, "main^");
    assert_eq!(subject(&r, &gained), "src/c.txt");
    assert!(
        said.contains(&format!(
            "regenerate: main moved to {} since {}, and the bundle's key there is ",
            &gained[..7],
            &tip[..7]
        )) && said.contains(&format!(
            ": what was made of {} is stale, discarded and not committed",
            &tip[..7]
        )),
        "{said}"
    );
    assert_eq!(
        git(&r.root, &["log", "--format=%s", &format!("{tip}..main")]),
        format!(
            "Regenerate the bundle for main at {}\nsrc/c.txt",
            &gained[..7]
        )
    );
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a\nb\nx");
}

#[test]
fn a_file_output_lands_onto_a_moved_main_as_the_round_made_it() {
    let r = Repo::new();
    let moved = r.dir.join("moved");
    let script = format!(
        "ls sim > VERSION; if [ ! -e '{moved}' ]; then touch '{moved}'; git -C '{root}' commit \
         -q --allow-empty -m 'main moved'; fi",
        moved = moved.display(),
        root = r.root.display()
    );
    generating(&r, &[&generator("the version", "VERSION", &script, "")]);
    regenerated(&r).1.unwrap();
    // Landed on the move, then current: the move changed nothing the version is made of.
    assert_eq!(subject(&r, "main^"), "main moved");
    assert_eq!(git(&r.root, &["show", "main:VERSION"]), "world.gd");
    assert_eq!(
        git(&r.root, &["diff", "--name-only", "main^", "main"]),
        "VERSION"
    );
    assert!(status(&r).1);
}

#[test]
fn outputs_main_changed_by_hand_during_a_build_take_what_it_made_of_the_same_inputs() {
    let r = Repo::new();
    let moved = r.dir.join("moved");
    let script = format!(
        "echo made >> '{made}'; mkdir -p gen && ls sim > gen/index; if [ ! -e '{moved}' ]; then \
         touch '{moved}'; mkdir -p '{root}/gen'; echo 'by hand' > '{root}/gen/index'; git -C \
         '{root}' add gen; git -C '{root}' commit -qm 'an index by hand'; fi",
        made = r.dir.join("made").display(),
        moved = moved.display(),
        root = r.root.display()
    );
    generating(&r, &[&generator("the index", "gen/", &script, "")]);
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    result.unwrap();
    let by_hand = head(&r.root, "main^");
    assert_eq!(subject(&r, &by_hand), "an index by hand");
    assert_eq!(
        subject(&r, "main"),
        format!("Regenerate the index for main at {}", &tip[..7])
    );
    assert_eq!(git(&r.root, &["show", "main:gen/index"]), "world.gd");
    assert_eq!(made(&r), 1, "{said}");
    assert!(status(&r).1, "{}", status(&r).0);
}

#[test]
fn a_red_round_files_one_task_adds_to_it_while_red_and_a_pass_closes_it() {
    let r = Repo::new();
    let bundle = generator(
        "the bundle",
        "gen/",
        "test -e src/fixed || { echo broken; exit 3; }; mkdir -p gen && echo ok > gen/out",
        "",
    );
    generating(&r, &[&bundle]);
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    let log = lands(&r).join("regenerate.log");
    let red = format!(
        "the generator the bundle failed on main at {}; its output is in {}",
        &tip[..7],
        log.display()
    );
    assert_eq!(result.unwrap(), red);
    assert_eq!(head(&r.root, "main"), tip, "nothing landed");
    assert!(
        said.contains(&format!("regenerate: red: filed {RED}")),
        "{said}"
    );
    let task = r.board.store.get(RED).unwrap();
    assert_eq!((task.status.as_str(), task.priority), ("ready", 0));
    assert_eq!(task.title, "The regenerator is red on the default branch");
    assert!(
        symphony_board::red::marked(&task.fields),
        "{:?}",
        task.fields
    );
    assert!(task.body.contains(&red), "{}", task.body);
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("broken\nexit 3\n")
    );
    let (line, current) = status(&r);
    assert!(!current);
    assert!(
        line.starts_with(&format!("the bundle: red on main at {} since ", &tip[..7]))
            && line.ends_with(&format!(
                "{red} ({RED}); main holds none found current here yet"
            )),
        "{line}"
    );
    // Red again on main's next tip: a line on the task open, no second task.
    let again = on_main(&r, "sim/other.gd", "extends Node\n");
    let (said, _) = regenerated(&r);
    assert!(
        said.contains(&format!("regenerate: red again: a line added to {RED}")),
        "{said}"
    );
    assert!(
        r.board
            .said(RED)
            .iter()
            .any(|l| l.starts_with(&format!("Red again on main at {}: ", &again[..7]))),
        "{:?}",
        r.board.said(RED)
    );
    assert!(r.board.store.get(&format!("{RED}-2")).is_err());
    // Fixed on main: the round passes, lands, and closes the red.
    let fixed = on_main(&r, "src/fixed", "");
    let (said, result) = regenerated(&r);
    result.unwrap();
    assert!(
        said.contains(&format!("regenerate: passed: closed {RED}")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED).unwrap().status, "cancelled");
    let closing = format!(
        "The regenerator passed on main at {}: closing this red",
        &fixed[..7]
    );
    assert!(
        r.board.said(RED).contains(&closing),
        "{:?}",
        r.board.said(RED)
    );
    assert_eq!(git(&r.root, &["show", "main:gen/out"]), "ok");
    // Red once more: a new task, the closed one kept.
    git(&r.root, &["rm", "-q", "src/fixed"]);
    git(&r.root, &["commit", "-qm", "broken again"]);
    let (said, _) = regenerated(&r);
    assert!(
        said.contains(&format!("regenerate: red: filed {RED}-2")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED).unwrap().status, "cancelled");
}

#[test]
fn a_generator_writing_outside_its_outputs_is_red_and_lands_nothing() {
    let r = Repo::new();
    generating(
        &r,
        &[&generator(
            "the index",
            "gen/",
            "echo stray > stray.txt",
            "",
        )],
    );
    let tip = head(&r.root, "main");
    let line = regenerated(&r).1.unwrap();
    assert_eq!(
        line,
        format!(
            "the generators changed stray.txt, outside their outputs, on main at {}",
            &tip[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), tip);
    assert!(!r.root.join("stray.txt").exists());
    assert_eq!(State::read(&lands(&r)).unwrap().state, "red");
}

#[test]
fn a_generator_past_the_timeout_is_ended_and_red() {
    let r = Repo::new();
    manifest(
        &r,
        &format!(
            "\n[land]\ntimeout_seconds = 1\ngenerators = [\n  {}\n]\n",
            generator("the slow one", "gen/", "sleep 600", "")
        ),
    );
    // A generator that never ends by itself while the test runs: the line names the limit that
    // ended it, which a generator ending by itself would never be said to have run past.
    let line = regenerated(&r).1.unwrap();
    assert!(
        line.starts_with(
            "the generator the slow one ran past the land's timeout ([land] timeout_seconds, 1 \
             s) on main at "
        ),
        "{line}"
    );
}

#[test]
fn a_second_regenerator_ends_at_once_while_one_runs() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    fs::create_dir_all(lands(&r)).unwrap();
    let held = fs::File::create(lands(&r).join(REGENERATING)).unwrap();
    held.lock().unwrap();
    let before = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    assert_eq!(
        result.unwrap(),
        "the regenerator runs already: it goes round once it ends, and makes the newest tip"
    );
    assert_eq!(said, "");
    // A land finds the lock taken and not yet named, as one is between its holder's lock and its
    // write: another regenerator runs, and it starts nothing.
    assert_eq!(
        trigger(&r),
        "another regenerator: it makes main's newest tip once it ends; nothing started"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(made(&r), 0);
    assert!(State::read(&lands(&r)).is_none());
    assert_eq!(logged(&r), "");
    // Let go as the regenerator lets go of its own, by itself: a close alone leaves it held by any
    // child another test's thread forked meanwhile, until that child's exec.
    held.unlock().unwrap();
    drop(held);
    regenerated(&r).1.unwrap();
    assert_eq!(made(&r), 1);
}

#[test]
fn a_folder_in_the_way_of_its_worktree_is_never_touched() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let kept = lands(&r).join("regenerate-worktree");
    write(&kept, "mine.txt", "mine\n");
    let before = head(&r.root, "main");
    let stopped = regenerated(&r).1.unwrap_err();
    assert_eq!(
        stopped,
        format!(
            "{} is in the way of the regenerator's worktree: move it, or name another [agents] \
             worktrees folder",
            kept.display()
        )
    );
    assert_eq!(fs::read_to_string(kept.join("mine.txt")).unwrap(), "mine\n");
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(State::read(&lands(&r)).unwrap().state, "stopped");
    let (line, current) = status(&r);
    assert!(!current);
    assert!(line.contains(": stopped on main at "), "{line}");
}

#[test]
fn a_project_with_no_generators_has_nothing_to_regenerate() {
    let r = Repo::new();
    manifest(&r, "");
    assert_eq!(
        regenerated(&r).1.unwrap(),
        "no [land] generators: nothing to regenerate"
    );
    assert_eq!(
        status(&r),
        ("no [land] generators: nothing to regenerate".into(), true)
    );
}

#[test]
fn the_status_says_how_far_main_is_past_what_was_made_and_who_makes_it() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let tip = head(&r.root, "main");
    let (line, current) = status(&r);
    assert!(!current);
    assert_eq!(
        line,
        format!(
            "the index: never regenerated here; the next land starts the regenerator, which \
             makes them of main at {}",
            &tip[..7]
        )
    );
    regenerated(&r).1.unwrap();
    let top = head(&r.root, "main");
    on_main(&r, "sim/other.gd", "extends Node\n");
    let (line, current) = status(&r);
    assert!(!current);
    assert!(
        line.starts_with(&format!(
            "the index: behind, main is 1 commit past {}, which they were last made of (",
            &top[..7]
        )),
        "{line}"
    );
    on_main(&r, "sim/more.gd", "extends Node\n");
    assert!(
        status(&r).0.contains("main is 2 commits past"),
        "{}",
        status(&r).0
    );
    // A round under way, by a regenerator that lives, and by one that ended without a word.
    let pid = i64::from(std::process::id());
    let wrote = |state: &str, pid: i64, start: Option<String>| {
        let state = State {
            state: state.into(),
            tip: top.clone(),
            landed: None,
            current: Some(top.clone()),
            at: 0.0,
            pid,
            start,
            line: "what it came to".into(),
        };
        fs::write(
            lands(&r).join("regenerate.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
    };
    // What main holds is as far behind as the commit they were last found current at.
    let held = format!("; main holds them as of {}, 2 commits behind", &top[..7]);
    for state in ["building", "dropped"] {
        wrote(state, pid, symphony_board::land::started(pid));
        let (line, current) = status(&r);
        assert!(!current && making(&r));
        assert!(
            line.starts_with(&format!(
                "the index: making them of main at {} since ",
                &top[..7]
            )) && line.ends_with(&format!("(pid {pid}); main is 2 commits past it{held}")),
            "{line}"
        );
        wrote(state, dead_pid(), None);
        let (line, current) = status(&r);
        assert!(!current && !making(&r));
        assert!(
            line.ends_with(&format!(
                "ended without a word; the next land starts another{held}"
            )),
            "{line}"
        );
    }
    // A pid that lives but is another process than the one that wrote it makes nothing.
    wrote("building", pid, Some("0.000000".into()));
    assert!(!making(&r), "{}", status(&r).0);
    // Behind with no regenerator running, red, and stopped.
    wrote("current", dead_pid(), None);
    let (line, current) = status(&r);
    assert!(!current && !making(&r));
    assert!(
        line.ends_with("; the regenerator runs after each land"),
        "{line}"
    );
    wrote("red", dead_pid(), None);
    let (line, current) = status(&r);
    assert!(!current && !making(&r));
    assert!(
        line.starts_with(&format!("the index: red on main at {} since ", &top[..7]))
            && line.ends_with(&format!(": what it came to ({RED}){held}")),
        "{line}"
    );
    wrote("stopped", dead_pid(), None);
    let (line, current) = status(&r);
    assert!(!current && !making(&r));
    assert!(
        line.starts_with(&format!("the index: stopped on main at {} (", &top[..7]))
            && line.ends_with(&format!("): what it came to{held}")),
        "{line}"
    );
}

#[test]
fn the_command_line_regenerates_only_by_the_land_and_its_status_exits_by_it() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let run = cli;
    let plain = r.env(&r.root);
    let (code, _, err) = run(&["regenerate"], &plain);
    assert_ne!(code, 0);
    assert_eq!(
        err.trim_end().lines().last().unwrap(),
        "✖ `tasks regenerate` moves and pushes the default branch: a land starts it (`just land \
         SLUG`); `just tasks regenerate --status` says where it stands"
    );
    assert_eq!(made(&r), 0);
    let (code, _, err) = run(&["regenerate", "--status"], &plain);
    assert_ne!(code, 0, "behind: {err}");
    assert!(err.contains("never regenerated here"), "{err}");
    let mut landing = r.env(&r.root);
    landing.vars.insert(RECIPE.into(), "1".into());
    let (code, out, err) = run(&["regenerate"], &landing);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(": landed on main at "), "{out}");
    let (code, out, err) = run(&["regenerate", "--status"], &plain);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with(&format!(
            "the index: current on main at {}",
            &head(&r.root, "main")[..7]
        )),
        "{out}"
    );
}

#[test]
fn a_land_starts_the_regenerator_detached_and_waits_for_none_of_it() {
    let r = Repo::new();
    // The index waits until the land has returned, so the land cannot have waited for it: one that
    // did would wait out the index, which then fails and lands nothing.
    let returned = r.dir.join("returned");
    let script = format!(
        "i=0; while [ ! -e '{returned}' ] && [ $i -lt {looks} ]; do sleep 0.1; i=$((i+1)); done; \
         [ -e '{returned}' ] || exit 9; mkdir -p gen && ls sim > gen/index",
        returned = returned.display(),
        looks = looks(Duration::from_millis(100))
    );
    generating(&r, &[&generator("the index", "gen/", &script, "")]);
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let mut env = r.env(&r.root);
    env.vars.insert(RECIPE.into(), "1".into());
    env.vars.insert("TASKS_REGENERATE".into(), "on".into());
    env.vars
        .insert("TASKS_BIN".into(), env!("CARGO_BIN_EXE_tasks").into());
    let mut out = Vec::new();
    let line = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out).unwrap();
    fs::write(&returned, "").unwrap();
    let merge = head(&r.root, "main");
    let said = String::from_utf8(out).unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    let log = lands(&r).join("regenerate.log");
    assert!(
        said.contains(&format!(
            "land {SLUG}: the regenerator makes the index in the background (`just tasks \
             regenerate --status`; its log {})",
            log.display()
        )),
        "{said}"
    );
    // It ends on its last line, once it landed the index of the land's merge.
    wait_for(|| {
        fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .any(|l| l.starts_with("Regenerate the index for main at "))
    });
    let logged = fs::read_to_string(&log).unwrap();
    assert_eq!(head(&r.root, "main^"), merge, "{logged}");
    assert_eq!(
        subject(&r, "main"),
        format!("Regenerate the index for main at {}", &merge[..7])
    );
    assert_eq!(
        git(&r.root, &["show", "main:gen/index"]),
        "jump.gd\nworld.gd"
    );
    assert!(
        logged.contains("regenerate: making the index for main at "),
        "{logged}"
    );
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
}

/// A land runs under the `land` recipe's mark and, as `just land` runs `just tasks land`, the
/// unasked recipes' mark as well. The regenerator it starts keeps the land's, by which alone it
/// lands what it makes, and not the unasked recipes' (`sys::MARKS`), under which its generators
/// and its check would be confined to the checkout as one command an agent ran is.
#[test]
fn the_regenerator_keeps_the_lands_mark_and_not_the_unasked_recipes() {
    use std::os::unix::fs::PermissionsExt;
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let record = r.dir.join("record");
    let heard = r.dir.join("tasks-heard");
    fs::write(
        &heard,
        format!(
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$*\" \"${{SYMPHONY_LAND-unset}}\" \
             \"${{SYMPHONY_UNASKED-unset}}\" > '{record}.part'\nmv '{record}.part' '{record}'\n",
            record = record.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&heard, fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = starting(&r);
    env.vars
        .insert("TASKS_BIN".into(), heard.display().to_string());
    env.vars
        .insert(symphony_board::confine::MARKER.into(), "1".into());
    let settings = symphony_board::land::Settings::of(&r.root).unwrap();
    let line = regenerate::start(&env, &r.root, &settings).unwrap();
    assert!(
        line.starts_with("the regenerator makes the index in the background"),
        "{line}"
    );
    wait_for(|| record.exists());
    assert_eq!(fs::read_to_string(&record).unwrap(), "regenerate|1|unset\n");
}

// -- one build of main at a time --------------------------------------------------------------------

/// The environment a land starts the regenerator from: the land's recipe, on a test's board that
/// is told to start one, by this build of `tasks`.
fn starting(r: &Repo) -> symphony_board::cli::Env {
    let mut env = r.env(&r.root);
    env.vars.insert(RECIPE.into(), "1".into());
    env.vars.insert("TASKS_REGENERATE".into(), "on".into());
    env.vars
        .insert("TASKS_BIN".into(), env!("CARGO_BIN_EXE_tasks").into());
    env
}

/// The regenerator started as a land starts it, after main moved: the line the land says.
fn trigger(r: &Repo) -> String {
    let settings = symphony_board::land::Settings::of(&r.root).unwrap();
    regenerate::start(&starting(r), &r.root, &settings).unwrap()
}

/// `tasks regenerate` run by this test, detached from it as a land's is, its output in its log.
fn spawned(r: &Repo) -> symphony_board::sys::Child {
    fs::create_dir_all(lands(r)).unwrap();
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(lands(r).join("regenerate.log"))
        .unwrap();
    let mut command = symphony_board::sys::Command::of(env!("CARGO_BIN_EXE_tasks"));
    command
        .arg("regenerate")
        .cleared()
        .envs(r.vars())
        .env(RECIPE, "1")
        .current_dir(&r.root)
        .stdin(symphony_board::sys::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    command.spawn().unwrap()
}

/// The regenerator's log.
fn logged(r: &Repo) -> String {
    fs::read_to_string(lands(r).join("regenerate.log")).unwrap_or_default()
}

/// Whether no process holds the regenerator's lock.
fn lock_free(r: &Repo) -> bool {
    match fs::File::open(lands(r).join(REGENERATING)) {
        Ok(file) => file.try_lock().is_ok(),
        Err(_) => true,
    }
}

/// The lines `file` beside the repository holds whole: a line its writer has not ended yet is
/// not one, as a shell's `>>` creates the file before it writes.
fn whole_lines(r: &Repo, file: &str) -> Vec<String> {
    let text = fs::read_to_string(r.dir.join(file)).unwrap_or_default();
    let ended = text.rfind('\n').map_or("", |last| &text[..=last]);
    ended.lines().map(str::to_string).collect()
}

/// The tips a [`blocking`] generator was run on, in order.
fn builds(r: &Repo) -> Vec<String> {
    whole_lines(r, "builds")
}

/// The pids of the [`blocking`] builds, in order.
fn pids(r: &Repo) -> Vec<i64> {
    whole_lines(r, "pids")
        .iter()
        .map(|pid| pid.parse().unwrap())
        .collect()
}

/// The index, made once `release` is there beside the repository: until then each build waits,
/// while the regenerator that runs it lives, for [`STAND_IN_WAIT`] at most, and fails past it. It
/// notes the tip it builds in `builds`, its pid in `pids`, and any signal it takes in `signals`.
fn blocking(r: &Repo) -> String {
    generator(
        "the index",
        "gen/",
        &format!(
            "git rev-parse HEAD >> '{builds}'; echo $$ >> '{pids}'; trap \"echo signalled >> \
             '{signals}'\" HUP INT TERM; i=0; while [ ! -e '{release}' ] && kill -0 $PPID \
             2>/dev/null && [ $i -lt {looks} ]; do sleep 0.05; i=$((i+1)); done; [ -e \
             '{release}' ] || exit 9; mkdir -p gen && ls sim > gen/index",
            builds = r.dir.join("builds").display(),
            pids = r.dir.join("pids").display(),
            signals = r.dir.join("signals").display(),
            release = r.dir.join("release").display(),
            looks = looks(Duration::from_millis(50)),
        ),
        "",
    )
}

/// Let every [`blocking`] build go on.
fn release(r: &Repo) {
    fs::write(r.dir.join("release"), "").unwrap();
}

/// The regenerator's state as one with `pid` and `start` wrote it: `state` on `tip`, main's
/// outputs last current at `current`. Written whole, staged and renamed as the regenerator writes
/// it, since a status may read it meanwhile.
fn wrote(r: &Repo, state: &str, tip: &str, pid: i64, current: Option<&str>) {
    let state = State {
        state: state.into(),
        tip: tip.into(),
        landed: None,
        current: current.map(str::to_string),
        at: 0.0,
        pid,
        start: symphony_board::land::started(pid),
        line: "what it came to".into(),
    };
    fs::create_dir_all(lands(r)).unwrap();
    let staged = lands(r).join(".regenerate.json.test");
    fs::write(&staged, serde_json::to_string(&state).unwrap()).unwrap();
    fs::rename(&staged, lands(r).join(regenerate::STATE)).unwrap();
}

/// The command line of `tasks` in `env`: its exit, its output and its errors.
fn cli(args: &[&str], env: &symphony_board::cli::Env) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, env, &mut "".as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn three_lands_during_one_build_make_one_more_build_of_the_last_tip_and_signal_none() {
    let r = Repo::new();
    generating(&r, &[&blocking(&r)]);
    let first = head(&r.root, "main");
    let started = trigger(&r);
    assert!(
        started.starts_with("the regenerator makes the index in the background"),
        "{started}"
    );
    wait_for(|| builds(&r).len() == 1);
    let builder = State::read(&lands(&r)).unwrap();
    assert_eq!(
        (builder.state.as_str(), builder.tip.as_str()),
        ("building", first.as_str())
    );
    let named = fs::read_to_string(lands(&r).join(REGENERATING)).unwrap();
    assert!(
        named.starts_with(&format!("the regenerator (pid {}, started ", builder.pid)),
        "{named}"
    );
    // Three lands while it builds: each starts nothing and waits for nothing. The build waits for
    // the test's release, which comes after them all, so a land that waited for it would wait out
    // the build, which then fails, and the one building would not be the one that runs on below.
    let env = starting(&r);
    for (slug, file) in [
        ("sim/a", "sim/a.gd"),
        ("sim/b", "sim/b.gd"),
        ("sim/c", "sim/c.gd"),
    ] {
        branch(&r, slug, file, "extends Node\n");
        let people = approved(&r, slug, "good");
        let mut out = Vec::new();
        land(&r.board.store, &env, slug, &people.orch, None, &mut out).unwrap();
        let said = String::from_utf8(out).unwrap();
        assert!(
            said.contains(&format!(
                "land {slug}: the regenerator (pid {}, started ",
                builder.pid
            )) && said.contains(&format!(", making them of {} since ", &first[..7]))
                && said.contains(": it makes main's newest tip once it ends; nothing started"),
            "{said}"
        );
    }
    let last = head(&r.root, "main");
    // The build went on through them, the one building still, and was never signalled.
    assert_eq!(builds(&r), vec![first.clone()]);
    assert!(symphony_board::sys::pid_alive(builder.pid));
    assert_eq!(State::read(&lands(&r)).unwrap().pid, builder.pid);
    release(&r);
    let landed = format!("Regenerate the index for main at {}: landed", &last[..7]);
    wait_for(|| logged(&r).lines().any(|l| l.starts_with(&landed)) && lock_free(&r));
    let log = logged(&r);
    // It finished the build it had, found it stale, and made the last tip alone: one more build.
    assert_eq!(builds(&r), vec![first.clone(), last.clone()], "{log}");
    assert!(!r.dir.join("signals").exists(), "{log}");
    assert!(
        log.contains(&format!(
            "what was made of {} is stale, discarded and not committed",
            &first[..7]
        )),
        "{log}"
    );
    let subjects = git(&r.root, &["log", "--format=%s", &format!("{first}..main")]);
    assert_eq!(
        subjects
            .lines()
            .filter(|s| s.starts_with("Regenerate "))
            .collect::<Vec<_>>(),
        vec![format!("Regenerate the index for main at {}", &last[..7])],
        "{subjects}"
    );
    assert_eq!(head(&r.root, "main^"), last);
    assert_eq!(
        git(&r.root, &["show", "main:gen/index"]),
        "a.gd\nb.gd\nc.gd\nworld.gd"
    );
    assert!(status(&r).1, "{}", status(&r).0);
}

#[test]
fn a_builder_that_died_is_replaced_by_the_next_trigger_which_makes_mains_tip() {
    let r = Repo::new();
    generating(&r, &[&blocking(&r)]);
    let tip = head(&r.root, "main");
    let mut builder = spawned(&r);
    // The build noted its tip and its pid, each a whole line, whichever it wrote first.
    wait_saying(
        || builds(&r).len() == 1 && pids(&r).len() == 1,
        || {
            let held = |file: &str| fs::read_to_string(r.dir.join(file)).ok();
            format!("builds holds {:?}, pids {:?}", held("builds"), held("pids"))
        },
    );
    assert_eq!(builds(&r), vec![tip.clone()]);
    let build = pids(&r)[0];
    let pid = builder.id();
    builder.kill().unwrap();
    builder.wait().unwrap();
    // Its build ends once it is gone; the lock it held is free, and names it still.
    wait_for(|| !symphony_board::sys::pid_alive(build));
    assert!(lock_free(&r));
    let named = fs::read_to_string(lands(&r).join(REGENERATING)).unwrap();
    assert!(
        named.starts_with(&format!("the regenerator (pid {pid}, started ")),
        "{named}"
    );
    let (line, current) = status(&r);
    assert!(!current && !making(&r));
    assert!(
        line.contains("ended without a word; the next land starts another"),
        "{line}"
    );
    // The next land's trigger takes it over and makes main's tip.
    release(&r);
    let started = trigger(&r);
    assert!(
        started.starts_with("the regenerator makes the index in the background"),
        "{started}"
    );
    let landed = format!("Regenerate the index for main at {}: landed", &tip[..7]);
    wait_for(|| logged(&r).lines().any(|l| l.starts_with(&landed)) && lock_free(&r));
    assert_eq!(builds(&r), vec![tip.clone(), tip.clone()]);
    assert_eq!(head(&r.root, "main^"), tip);
    assert_eq!(git(&r.root, &["show", "main:gen/index"]), "world.gd");
    assert!(status(&r).1, "{}", status(&r).0);
    let named = fs::read_to_string(lands(&r).join(REGENERATING)).unwrap();
    assert!(!named.contains(&format!("pid {pid},")), "{named}");
}

#[test]
fn two_triggers_at_once_start_one_build() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let tip = head(&r.root, "main");
    let together = std::sync::Barrier::new(2);
    let lines: Vec<String> = std::thread::scope(|s| {
        let one = || {
            together.wait();
            trigger(&r)
        };
        let (a, b) = (s.spawn(one), s.spawn(one));
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    let started = lines
        .iter()
        .filter(|l| l.contains("in the background"))
        .count();
    assert!((1..=2).contains(&started), "{lines:?}");
    // Each one started ends: one made the index, any other found it made or making.
    let ended = |log: &str| {
        log.lines()
            .filter(|l| {
                l.starts_with("Regenerate the index for main at ")
                    || l.starts_with("the index: current on main at ")
                    || l.starts_with("the regenerator runs already: ")
            })
            .count()
    };
    wait_for(|| ended(&logged(&r)) == started && lock_free(&r));
    assert_eq!(made(&r), 1, "{}", logged(&r));
    assert_eq!(
        git(&r.root, &["log", "--format=%s", &format!("{tip}..main")]),
        format!("Regenerate the index for main at {}", &tip[..7])
    );
}

/// Its log past the sweep's bound is set aside as `regenerate.log.1`, the one before dropped, by a
/// start that starts one; a start that finds one running leaves it where it is, as that one writes
/// on into it.
#[test]
fn a_start_sets_its_log_aside_past_its_bound_and_never_under_one_that_runs() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    fs::create_dir_all(lands(&r)).unwrap();
    let log = lands(&r).join("regenerate.log");
    let aside = lands(&r).join("regenerate.log.1");
    let full = format!("{}\n", "x".repeat(LOG_LIMIT as usize));
    fs::write(&log, &full).unwrap();
    fs::write(&aside, "the one before\n").unwrap();
    let held = fs::File::create(lands(&r).join(REGENERATING)).unwrap();
    held.lock().unwrap();
    assert_eq!(
        trigger(&r),
        "another regenerator: it makes main's newest tip once it ends; nothing started"
    );
    assert_eq!(fs::read_to_string(&log).unwrap(), full);
    assert_eq!(fs::read_to_string(&aside).unwrap(), "the one before\n");
    // Let go as the regenerator lets go of its own (`a_second_regenerator_ends_at_once_while_one_runs`).
    held.unlock().unwrap();
    drop(held);
    let tip = head(&r.root, "main");
    let started = trigger(&r);
    assert!(
        started.starts_with("the regenerator makes the index in the background"),
        "{started}"
    );
    let landed = format!("Regenerate the index for main at {}: landed", &tip[..7]);
    wait_for(|| logged(&r).lines().any(|l| l.starts_with(&landed)) && lock_free(&r));
    assert_eq!(fs::read_to_string(&aside).unwrap(), full);
    let fresh = logged(&r);
    assert!(!fresh.contains(&"x".repeat(64)), "{fresh}");
    assert!(
        fresh.contains("regenerate: making the index for main at "),
        "{fresh}"
    );
}

#[test]
fn the_status_waits_while_the_regenerator_makes_them_then_says_where_it_stands() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    regenerated(&r).1.unwrap();
    let top = head(&r.root, "main");
    let moved = on_main(&r, "sim/other.gd", "extends Node\n");
    // A regenerator making the moved tip, as far as its state says: this process.
    let me = i64::from(std::process::id());
    wrote(&r, "building", &moved, me, Some(&top));
    let env = r.env(&r.root);
    let printed = Arc::new(Mutex::new(Vec::new()));
    let said = || String::from_utf8_lossy(&printed.lock().unwrap()).into_owned();
    let mut out = Shared(Arc::clone(&printed));
    let (code, err) = std::thread::scope(|s| {
        let waiting = s.spawn(|| {
            let args = ["regenerate", "--status", "--wait"].map(String::from);
            let mut err = Vec::new();
            let code =
                symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
            (code, String::from_utf8(err).unwrap())
        });
        // It read the state a regenerator that lives is building, said so, and waits on.
        wait_saying(|| said().starts_with("waiting: "), said);
        assert!(!waiting.is_finished(), "it waits");
        wrote(&r, "current", &moved, dead_pid(), Some(&moved));
        waiting.join().unwrap()
    });
    let out = said();
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with(&format!(
            "waiting: the index: making them of main at {} since ",
            &moved[..7]
        )) && out.contains(&format!(
            "; main holds them as of {}, 1 commit behind\nthe index: current on main at {} (judged ",
            &top[..7],
            &moved[..7]
        )),
        "{out}"
    );
    // Red, or behind with none making them: it waits for nothing and exits 1, `--wait` alone too.
    wrote(&r, "red", &moved, dead_pid(), Some(&top));
    let (code, out, err) = cli(&["regenerate", "--wait"], &env);
    assert_eq!(code, 1, "{out}{err}");
    // No `waiting:` line: it never waited.
    assert_eq!(out, "");
    assert!(
        err.contains(&format!("the index: red on main at {} since ", &moved[..7])),
        "{err}"
    );
    wrote(&r, "current", &top, dead_pid(), Some(&top));
    let (code, _, err) = cli(&["regenerate", "--status", "--wait"], &env);
    assert_eq!(code, 1);
    assert!(
        err.contains("the regenerator runs after each land"),
        "{err}"
    );
}

// -- the check no land runs -------------------------------------------------------------------------

/// The check this section's projects run: it notes in `checks` beside the repository what it found
/// where it ran (the subject of the commit there, where main's ref was, and whether `gen/` was
/// there), and is red on a tree holding `red`.
const AUDIT: &str = r#"printf '%s|%s|%s\n' "$(git log -1 --format=%s)" "$(git rev-parse refs/heads/main)" "$(if [ -d gen ]; then echo made; else echo bare; fi)" >> CHECKS
if [ -e red ]; then echo 'red on purpose'; exit 4; fi
"#;

/// The project file on main with [`AUDIT`] as its `[verdicts] check`, the land's own turned off when
/// `off`, and `land` in its `[land]`: the check as the regenerator names it.
fn audited(r: &Repo, off: bool, land: &str) -> String {
    let script = r.dir.join("audit.sh");
    let checks = format!("'{}'", r.dir.join("checks").display());
    fs::write(&script, AUDIT.replace("CHECKS", &checks)).unwrap();
    let off = if off { "check = false\n" } else { "" };
    project_file(
        r,
        &format!(
            "[verdicts]\ncheck = [\"sh\", {}]\n\n[land]\n{off}{land}",
            toml_string(&script.to_string_lossy())
        ),
    );
    format!("sh {}", script.display())
}

/// [`audited`], the land's check off: the regenerator runs it.
fn auditing(r: &Repo, land: &str) -> String {
    audited(r, true, land)
}

/// `[land] generators` of `generator` alone.
fn alone(generator: &str) -> String {
    format!("generators = [\n  {generator}\n]\n")
}

/// What a run of [`AUDIT`] found: the subject of the commit it ran at, where main was, and whether
/// `gen/` was there.
#[derive(Debug, PartialEq)]
struct Found {
    subject: String,
    main: String,
    made: bool,
}

impl Found {
    fn new(subject: &str, main: &str, made: bool) -> Found {
        Found {
            subject: subject.into(),
            main: main.into(),
            made,
        }
    }
}

/// What each run of [`AUDIT`] found, in order.
fn checks(r: &Repo) -> Vec<Found> {
    fs::read_to_string(r.dir.join("checks"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            Found::new(parts[0], parts[1], parts[2] == "made")
        })
        .collect()
}

/// Whether the regenerator's lines `said` hold any of the check's own.
fn checked_in(said: &str) -> bool {
    said.contains("regenerate: checking ") || said.contains("the check `")
}

/// The subject of the regenerator's commit of `generators` for main at `tip`.
fn regenerating(generators: &str, tip: &str) -> String {
    format!("Regenerate {generators} for main at {}", &tip[..7])
}

#[test]
fn the_check_no_land_runs_runs_on_the_commit_a_round_landed_once_it_landed() {
    let r = Repo::new();
    let shown = auditing(&r, &alone(&index(&r)));
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    let top = head(&r.root, "main");
    // The round's line is the commit it landed; the check came after it, on that commit.
    assert_eq!(
        result.unwrap(),
        format!(
            "{}: landed on main at {}; not pushed: no origin remote",
            regenerating("the index", &tip),
            &top[..12]
        )
    );
    assert_eq!(
        checks(&r),
        [Found::new(&regenerating("the index", &tip), &top, true)]
    );
    let passed = format!("the check `{shown}` passed on main at {}", &top[..7]);
    let checking = format!("regenerate: checking main at {} ({shown})\n", &top[..7]);
    assert!(
        said.contains(&format!("{checking}regenerate: {passed}\n")),
        "{said}"
    );
    assert!(
        said.find(": landed on main at ").unwrap() < said.find(&checking).unwrap(),
        "{said}"
    );
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (
            checked.state.as_str(),
            checked.tip.as_str(),
            checked.task.as_deref(),
            checked.line.as_str()
        ),
        ("passed", top.as_str(), None, passed.as_str())
    );
    assert!(r.board.store.get(RED_CHECK).is_err());
    let (line, current) = status(&r);
    assert!(current, "{line}");
    assert!(
        line.starts_with(&format!(
            "the index: current on main at {} (judged ",
            &top[..7]
        )) && line.contains(&format!(
            "); the check `{shown}` passed on main at {} (",
            &top[..7]
        )) && line.ends_with(')'),
        "{line}"
    );
    // A project whose land runs the check keeps it out of the regenerator, and its status too.
    audited(&r, false, &alone(&index(&r)));
    let (said, result) = regenerated(&r);
    result.unwrap();
    assert_eq!(checks(&r).len(), 1, "{said}");
    assert!(!checked_in(&said), "{said}");
    let (line, current) = status(&r);
    assert!(current && !line.contains("the check"), "{line}");
}

#[test]
fn a_red_check_files_one_task_of_its_own_and_never_holds_back_the_generators_commit() {
    let r = Repo::new();
    let shown = auditing(&r, &alone(&index(&r)));
    let tip = on_main(&r, "red", "");
    let (said, result) = regenerated(&r);
    let top = head(&r.root, "main");
    // What the round made landed all the same, and the round ends on it.
    assert_eq!(
        result.unwrap(),
        format!(
            "{}: landed on main at {}; not pushed: no origin remote",
            regenerating("the index", &tip),
            &top[..12]
        )
    );
    assert_eq!(head(&r.root, "main^"), tip);
    assert_eq!(
        checks(&r),
        [Found::new(&regenerating("the index", &tip), &top, true)]
    );
    let log = lands(&r).join("regenerate.log");
    let red = format!(
        "the check `{shown}` failed on main at {}; its output is in {}",
        &top[..7],
        log.display()
    );
    assert!(
        said.contains(&format!(
            "regenerate: {red}\nregenerate: red: filed {RED_CHECK}\n"
        )),
        "{said}"
    );
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("red on purpose\nexit 4\n")
    );
    let task = r.board.store.get(RED_CHECK).unwrap();
    assert_eq!(
        (task.status.as_str(), task.priority, task.title.as_str()),
        ("ready", 0, "The check is red on the default branch")
    );
    assert!(
        symphony_board::red::marked(&task.fields),
        "{:?}",
        task.fields
    );
    let what = format!(
        "`[verdicts] check` on what each round leaves the default branch holding, as no land does \
         there (`[land] check = false`), and it failed: {red}"
    );
    let done = "**Done when.** A commit on the default branch makes the check pass";
    assert!(
        task.body.contains(&what) && task.body.contains(done),
        "{}",
        task.body
    );
    // The generators passed: theirs is another series, and nothing is filed in it.
    assert!(r.board.store.get(RED).is_err());
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (
            checked.state.as_str(),
            checked.tip.as_str(),
            checked.task.as_deref()
        ),
        ("red", top.as_str(), Some(RED_CHECK))
    );
    // The status says it, and exits as the outputs stand: current.
    let red_on = format!("; the check `{shown}` is red on main at {} (", &top[..7]);
    let (line, current) = status(&r);
    assert!(current, "{line}");
    assert!(
        line.contains(&red_on) && line.ends_with(&format!(", {RED_CHECK})")),
        "{line}"
    );
    let (code, out, err) = cli(&["regenerate", "--status"], &r.env(&r.root));
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(&red_on), "{out}");
    // Red again on main's next tip: a line on the task open, no second task.
    on_main(&r, "sim/other.gd", "extends Node\n");
    let (said, _) = regenerated(&r);
    let again = head(&r.root, "main");
    assert!(
        said.contains(&format!(
            "regenerate: red again: a line added to {RED_CHECK}\n"
        )),
        "{said}"
    );
    assert!(
        r.board
            .said(RED_CHECK)
            .iter()
            .any(|l| l.starts_with(&format!(
                "Red again on main at {}: the check `{shown}` failed on main at {}",
                &again[..7],
                &again[..7]
            ))),
        "{:?}",
        r.board.said(RED_CHECK)
    );
    assert!(r.board.store.get(&format!("{RED_CHECK}-2")).is_err());
    // Fixed on main: the check passes and closes it.
    git(&r.root, &["rm", "-q", "red"]);
    git(&r.root, &["commit", "-qm", "fixed"]);
    let fixed = head(&r.root, "main");
    let (said, _) = regenerated(&r);
    assert!(
        said.contains(&format!("regenerate: passed: closed {RED_CHECK}\n")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED_CHECK).unwrap().status, "cancelled");
    assert!(
        r.board.said(RED_CHECK).contains(&format!(
            "The check passed on main at {}: closing this red",
            &fixed[..7]
        )),
        "{:?}",
        r.board.said(RED_CHECK)
    );
    let (line, current) = status(&r);
    assert!(
        current
            && line.contains(&format!(
                "; the check `{shown}` passed on main at {} (",
                &fixed[..7]
            )),
        "{line}"
    );
    // Red once more: a new task, the closed one kept.
    on_main(&r, "red", "");
    let (said, _) = regenerated(&r);
    assert!(
        said.contains(&format!("regenerate: red: filed {RED_CHECK}-2\n")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED_CHECK).unwrap().status, "cancelled");
}

#[test]
fn a_round_whose_generator_is_red_is_checked_on_its_tip_and_each_red_is_a_task_of_its_own() {
    let r = Repo::new();
    let bundle = generator(
        "the bundle",
        "gen/",
        "mkdir -p gen && echo half > gen/half; test -e src/fixed || { echo broken; exit 3; }; echo \
         ok > gen/out",
        "",
    );
    auditing(&r, &alone(&bundle));
    let tip = on_main(&r, "red", "");
    let (said, result) = regenerated(&r);
    assert!(
        result
            .unwrap()
            .starts_with("the generator the bundle failed on main at "),
        "{said}"
    );
    // On the tip as committed: what the generator half made before it failed is gone.
    assert_eq!(checks(&r), [Found::new("red on main", &tip, false)]);
    assert!(
        said.contains(&format!("regenerate: red: filed {RED}\n"))
            && said.contains(&format!("regenerate: red: filed {RED_CHECK}\n")),
        "{said}"
    );
    // The check fixed alone: its red closes, and the generators' stays, a line added.
    git(&r.root, &["rm", "-q", "red"]);
    git(&r.root, &["commit", "-qm", "fixed"]);
    let fixed = head(&r.root, "main");
    let (said, _) = regenerated(&r);
    assert_eq!(checks(&r)[1], Found::new("fixed", &fixed, false));
    assert!(
        said.contains(&format!("regenerate: red again: a line added to {RED}\n"))
            && said.contains(&format!("regenerate: passed: closed {RED_CHECK}\n")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED).unwrap().status, "ready");
    assert_eq!(r.board.store.get(RED_CHECK).unwrap().status, "cancelled");
    let (line, current) = status(&r);
    assert!(
        !current
            && line.starts_with(&format!(
                "the bundle: red on main at {} since ",
                &fixed[..7]
            ))
            && line.contains(&format!(" passed on main at {} (", &fixed[..7])),
        "{line}"
    );
}

#[test]
fn a_dropped_round_is_not_checked_and_the_round_after_it_checks_what_it_landed() {
    let r = Repo::new();
    // The first build commits one of the bundle's sources on main while it runs: what it made is
    // stale, found by reading the key again on main's new tip in its worktree.
    let next = r.dir.join("next");
    let script = format!(
        "mkdir -p gen && cat src/*.txt > gen/out.txt; if [ -e '{next}' ]; then sh '{next}'; rm -f \
         '{next}'; fi",
        next = next.display()
    );
    auditing(
        &r,
        &format!(
            "{}keys = {{ \"the bundle\" = [\"sh\", \"-c\", \"cat src/*.txt | cksum\"] }}\n",
            alone(&generator("the bundle", "gen/", &script, ""))
        ),
    );
    on_main(&r, "src/a.txt", "a\n");
    fs::write(
        &next,
        format!(
            "echo b > '{root}/src/b.txt'; git -C '{root}' add -A; git -C '{root}' commit -qm \
             'src/b.txt'",
            root = r.root.display()
        ),
    )
    .unwrap();
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    result.unwrap();
    let gained = head(&r.root, "main^");
    assert_eq!(subject(&r, &gained), "src/b.txt");
    assert!(
        said.contains(&format!(
            "what was made of {} is stale, discarded and not committed",
            &tip[..7]
        )),
        "{said}"
    );
    // Checked once: not what the dropped round made, which nothing holds, but the commit the
    // round after it landed on the newest.
    let top = head(&r.root, "main");
    assert_eq!(
        checks(&r),
        [Found::new(&regenerating("the bundle", &gained), &top, true)],
        "{said}"
    );
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (checked.state.as_str(), checked.tip.as_str()),
        ("passed", top.as_str())
    );
}

#[test]
fn a_land_while_it_builds_that_turns_the_check_red_is_checked_on_what_main_holds() {
    let r = Repo::new();
    // The build commits `red` on main while it runs, as a land would: outside the bundle's
    // sources, so what it made lands onto main's new tip, which the check fails on.
    let next = r.dir.join("next");
    let script = format!(
        "mkdir -p gen && cat src/*.txt > gen/out.txt; if [ -e '{next}' ]; then sh '{next}'; rm -f \
         '{next}'; fi",
        next = next.display()
    );
    let shown = auditing(
        &r,
        &format!(
            "{}keys = {{ \"the bundle\" = [\"sh\", \"-c\", \"cat src/*.txt | cksum\"] }}\n",
            alone(&generator("the bundle", "gen/", &script, ""))
        ),
    );
    on_main(&r, "src/a.txt", "a\n");
    fs::write(
        &next,
        format!(
            "echo red > '{root}/red'; git -C '{root}' add -A; git -C '{root}' commit -qm 'red'",
            root = r.root.display()
        ),
    )
    .unwrap();
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    assert!(result.unwrap().contains(": landed on main at "), "{said}");
    let top = head(&r.root, "main");
    assert_eq!(subject(&r, "main^"), "red", "{said}");
    assert_eq!(subject(&r, &top), regenerating("the bundle", &tip));
    // Checked once, on the commit main holds: the land's `red` beside what was made.
    assert_eq!(
        checks(&r),
        [Found::new(&regenerating("the bundle", &tip), &top, true)],
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "regenerate: the check `{shown}` failed on main at {}; its output is in ",
            &top[..7]
        )) && said.contains(&format!("regenerate: red: filed {RED_CHECK}\n")),
        "{said}"
    );
    let task = r.board.store.get(RED_CHECK).unwrap();
    assert_eq!(task.title, "The check is red on the default branch");
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (
            checked.state.as_str(),
            checked.tip.as_str(),
            checked.task.as_deref()
        ),
        ("red", top.as_str(), Some(RED_CHECK))
    );
    let (line, current) = status(&r);
    assert!(
        current && line.contains(&format!("is red on main at {} (", &top[..7])),
        "{line}"
    );
}

#[test]
fn a_check_that_cannot_run_is_red() {
    let r = Repo::new();
    let missing = r.dir.join("no-such-check");
    project_file(
        &r,
        &format!(
            "[verdicts]\ncheck = [{}]\n\n[land]\ncheck = false\n{}",
            toml_string(&missing.to_string_lossy()),
            alone(&index(&r))
        ),
    );
    let (said, result) = regenerated(&r);
    assert!(result.unwrap().contains(": landed on main at "), "{said}");
    let top = head(&r.root, "main");
    let cannot = format!(
        "the check `{}` could not run on main at {}: {}: ",
        missing.display(),
        &top[..7],
        missing.display()
    );
    assert!(
        said.contains(&format!("regenerate: {cannot}"))
            && said.contains(&format!("regenerate: red: filed {RED_CHECK}\n")),
        "{said}"
    );
    assert_eq!(Checked::read(&lands(&r)).unwrap().state, "red");
    // The task says it could not run, no more: not that it failed, nor what a commit would do.
    let task = r.board.store.get(RED_CHECK).unwrap();
    assert_eq!(task.title, "The check cannot run on the default branch");
    assert!(
        task.body
            .contains("and could not run it, so it neither passed nor failed: the check `")
            && !task.body.contains("and it failed")
            && !task.body.contains("makes the check pass"),
        "{}",
        task.body
    );
}

#[test]
fn a_check_past_its_time_is_filed_as_late_not_as_failed() {
    use super::limits::{CheckGroup, written_pid};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Instant;
    /// `[land] timeout_seconds`, which the test's clock passes in one step.
    const LIMIT: u64 = 3;
    let r = Repo::new();
    let pid = r.dir.join("check.pid");
    // A check that never ends by itself while the test runs: the line names the limit that ended
    // it, which a check ending by itself would never be said to have run past.
    let check = format!("echo $$ > '{}'; exec sleep 600", pid.display());
    project_file(
        &r,
        &format!(
            "[verdicts]\ncheck = [\"sh\", \"-c\", {}]\n\n[land]\ncheck = false\ntimeout_seconds = \
             {LIMIT}\n{}",
            toml_string(&check),
            alone(&index(&r))
        ),
    );
    // The clock the timeouts read stands still until the check runs, so the round's own commands
    // (its generator, its `git merge --ff-only`), each under the same limit, take what time a
    // loaded machine gives them and only the check runs past it.
    let began = Instant::now();
    let (expired, aborted) = (AtomicBool::new(false), AtomicBool::new(false));
    let (ready_reads, abort_ticks) = (AtomicU64::new(0), AtomicU64::new(0));
    let clock = || {
        if written_pid(&pid).is_some() {
            ready_reads.fetch_add(1, Ordering::SeqCst);
        }
        let limits = if aborted.load(Ordering::SeqCst) {
            // A command starting after a stalled setup expires too, so the scoped regenerator
            // can be joined before the stall is reported.
            abort_ticks.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            u64::from(expired.load(Ordering::SeqCst))
        };
        began + Duration::from_secs(limits * LIMIT)
    };
    let cleanup = CheckGroup(pid.clone());
    let (said, result) = std::thread::scope(|scope| {
        let store = r.board.other();
        let env = r.env(&r.root);
        let clock = &clock;
        let regenerator = scope.spawn(move || {
            let mut out = Vec::new();
            let result = regenerate::run_with_clock(&store, &env, &mut out, None, clock);
            (String::from_utf8(out).unwrap(), result)
        });
        let ready = test_time::until_true(
            "the check runs and its watchdog has read the held clock twice",
            test_time::backstop(Duration::from_secs(60)),
            || ready_reads.load(Ordering::SeqCst) >= 2,
        );
        // Two reads once the check runs include its watchdog's deadline and first look, even if
        // the watchdog started after the check: no command before it can use up this limit.
        aborted.store(ready.is_err(), Ordering::SeqCst);
        expired.store(true, Ordering::SeqCst);
        let ended = test_time::until_true(
            "the regenerator ended once its check ran past its time",
            test_time::backstop(Duration::from_secs(60)),
            || regenerator.is_finished(),
        );
        if ended.is_err() {
            cleanup.end();
            aborted.store(true, Ordering::SeqCst);
        }
        let done = regenerator.join().unwrap();
        ended.unwrap();
        ready.unwrap();
        done
    });
    assert!(result.unwrap().contains(": landed on main at "), "{said}");
    let top = head(&r.root, "main");
    assert!(
        said.contains(&format!(
            "regenerate: the check `sh -c {check}` ran past the land's timeout ([land] \
             timeout_seconds, {LIMIT} s) on main at {}; its output is in ",
            &top[..7]
        )) && said.contains(&format!("regenerate: red: filed {RED_CHECK}\n")),
        "{said}"
    );
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (checked.state.as_str(), checked.tip.as_str()),
        ("red", top.as_str())
    );
    let task = r.board.store.get(RED_CHECK).unwrap();
    assert_eq!(
        task.title,
        "The check runs past its time on the default branch"
    );
    assert!(
        task.body
            .contains("ran past its time and was ended, so it neither passed nor failed: ")
            && !task.body.contains("and it failed")
            && !task.body.contains("makes the check pass"),
        "{}",
        task.body
    );
}

#[test]
fn a_round_that_stops_is_not_checked_and_the_next_that_gets_through_is() {
    let r = Repo::new();
    auditing(&r, &alone(&index(&r)));
    let tip = head(&r.root, "main");
    // An edit in the main checkout: what the round made cannot land, and the round stops.
    write(&r.root, "sim/world.gd", "edited\n");
    let (said, result) = regenerated(&r);
    let stopped = result.unwrap_err();
    assert!(stopped.contains(" did not land: "), "{stopped}");
    assert!(checks(&r).is_empty(), "{said}");
    assert!(!checked_in(&said), "{said}");
    assert!(!lands(&r).join(CHECKED).exists());
    git(&r.root, &["checkout", "--", "sim/world.gd"]);
    regenerated(&r).1.unwrap();
    assert_eq!(
        checks(&r),
        [Found::new(
            &regenerating("the index", &tip),
            &head(&r.root, "main"),
            true
        )]
    );
}

#[test]
fn a_signal_while_it_checks_ends_the_check_and_the_regenerator_judging_nothing() {
    let r = Repo::new();
    let pid = r.dir.join("check.pid");
    // A check that outlives any wait of the test's, so only the signal ends it.
    let check = format!("echo $$ > '{pid}'; exec sleep 600", pid = pid.display());
    project_file(
        &r,
        &format!(
            "[verdicts]\ncheck = [\"sh\", \"-c\", {}]\n\n[land]\ncheck = false\n{}",
            toml_string(&check),
            alone(&index(&r))
        ),
    );
    let tip = head(&r.root, "main");
    let mut regenerator = spawned(&r);
    wait_for(|| fs::read_to_string(&pid).is_ok_and(|p| p.ends_with('\n')));
    let checking: i64 = fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
    // What the round made landed before the check began, and the status says it checks that.
    assert_eq!(subject(&r, "main"), regenerating("the index", &tip));
    let top = head(&r.root, "main");
    let (line, current) = status(&r);
    assert!(
        current
            && line.contains(&format!("; checking main at {} since ", &top[..7]))
            && line.contains(&format!("(pid {}): the check `sh -c ", regenerator.id())),
        "{line}"
    );
    let target = i32::try_from(regenerator.id()).unwrap();
    // SAFETY: a signal to the process this test started.
    assert_eq!(unsafe { libc::kill(target, libc::SIGTERM) }, 0);
    test_time::until_true(
        "the regenerator ended at its SIGTERM",
        test_time::backstop(Duration::from_secs(60)),
        || regenerator.try_wait().unwrap().is_some(),
    )
    .unwrap();
    assert!(!symphony_board::sys::pid_alive(checking));
    let log = logged(&r);
    assert!(
        log.contains(&format!(
            "interrupted by SIGTERM while it checked main at {}: what the round made stands, the \
             check not judged",
            &top[..7]
        )),
        "{log}"
    );
    // Nothing judged: the record still says it was checking, and the status that it ended so.
    let checked = Checked::read(&lands(&r)).unwrap();
    assert_eq!(
        (checked.state.as_str(), checked.tip.as_str()),
        ("checking", top.as_str())
    );
    assert!(r.board.store.get(RED_CHECK).is_err());
    assert!(lock_free(&r));
    let (line, current) = status(&r);
    assert!(
        current
            && line.ends_with(", ended unjudged; the next round checks again")
            && line.contains(&format!("of main at {}, begun ", &top[..7])),
        "{line}"
    );
}

#[test]
fn the_status_says_a_red_check_the_board_did_not_take_without_a_task() {
    let r = Repo::new();
    let shown = auditing(&r, &alone(&index(&r)));
    regenerated(&r).1.unwrap();
    let top = head(&r.root, "main");
    let checked = Checked {
        state: "red".into(),
        tip: top.clone(),
        task: None,
        at: 0.0,
        line: "what it came to".into(),
        pid: 0,
        start: None,
    };
    fs::write(
        lands(&r).join(CHECKED),
        serde_json::to_string(&checked).unwrap(),
    )
    .unwrap();
    let (line, current) = status(&r);
    assert!(current, "{line}");
    let when = symphony_board::sys::local_minute(0.0);
    assert!(
        line.ends_with(&format!(
            "; the check `{shown}` is red on main at {} ({when})",
            &top[..7]
        )),
        "{line}"
    );
}
