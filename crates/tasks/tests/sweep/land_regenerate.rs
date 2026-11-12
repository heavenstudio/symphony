//! The regenerator (`tasks regenerate`), which a land starts detached: what it makes of main's
//! newest tip, the one commit it lands in seconds under the land's lock, what it skips by a key it
//! passed on before, what it does while main moves, the red task it files and closes, the one that
//! runs at a time, and the status it leaves.

use super::*;
use symphony_board::land::regenerate::{self, LOCK as REGENERATING, RED, State};

/// The regenerator run from the main checkout, as a land starts it: what it said, and its last
/// line or why it stopped.
fn regenerated(r: &Repo) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let result = regenerate::run(&r.board.store, &r.env(&r.root), &mut out);
    (String::from_utf8(out).unwrap(), result)
}

/// The lands' folder.
fn lands(r: &Repo) -> PathBuf {
    r.root.join(".git/sf-tasks/land")
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
    // Run again on what it landed: nothing to make, the summary waiting for a change it never saw.
    let (_, again) = regenerated(&r);
    assert_eq!(
        again.unwrap(),
        format!(
            "the index and the summary: current on main at {}",
            &top[..7]
        )
    );
    assert_eq!(head(&r.root, "main"), top);
    assert_eq!(made(&r), 2);
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
        ", key = [\"sh\", \"-c\", \"echo reading; cat src/*.txt | cksum\"]",
    );
    generating(&r, &[&bundle]);
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
        ", key = [\"sh\", \"-c\", \"test -e src/key || exit 4; cat src/key\"]",
    );
    generating(&r, &[&bundle]);
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
fn main_moving_during_a_round_lands_onto_its_new_tip_and_goes_round_for_it() {
    let r = Repo::new();
    // The first time it runs, the generator commits on main in the main checkout, outside its
    // outputs: main moves while the round makes the index of the tip before.
    let moved = r.dir.join("moved");
    let script = format!(
        "mkdir -p gen && ls sim > gen/index; if [ ! -e '{moved}' ]; then touch '{moved}'; echo x \
         > '{root}/sim/moved.gd'; git -C '{root}' add sim/moved.gd; git -C '{root}' commit -qm \
         'main moved'; fi",
        moved = moved.display(),
        root = r.root.display()
    );
    generating(&r, &[&generator("the index", "gen/", &script, "")]);
    let tip = head(&r.root, "main");
    let (said, result) = regenerated(&r);
    let line = result.unwrap();
    // The first round's commit went on main's new tip; the second made the index of that.
    let (first, second) = (head(&r.root, "main^"), head(&r.root, "main"));
    assert_eq!(subject(&r, &format!("{first}^")), "main moved");
    assert_eq!(head(&r.root, &format!("{first}^^")), tip);
    assert_eq!(
        subject(&r, &first),
        format!("Regenerate the index for main at {}", &tip[..7])
    );
    assert_eq!(
        git(&r.root, &["show", &format!("{first}:gen/index")]),
        "world.gd"
    );
    assert_eq!(
        git(&r.root, &["show", &format!("{first}:sim/moved.gd")]),
        "x",
        "main's tree kept under the outputs"
    );
    assert_eq!(
        subject(&r, &second),
        format!("Regenerate the index for main at {}", &first[..7])
    );
    assert_eq!(
        git(&r.root, &["show", "main:gen/index"]),
        "moved.gd\nworld.gd"
    );
    assert_eq!(
        line,
        format!(
            "Regenerate the index for main at {}: landed on main at {}; not pushed: no origin \
             remote",
            &first[..7],
            &second[..12]
        )
    );
    assert!(
        said.contains("regenerate: the default branch moved; going round"),
        "{said}"
    );
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
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
fn outputs_main_changed_meanwhile_drop_the_round_and_the_next_makes_them_again() {
    let r = Repo::new();
    let moved = r.dir.join("moved");
    let script = format!(
        "mkdir -p gen && ls sim > gen/index; if [ ! -e '{moved}' ]; then touch '{moved}'; mkdir \
         -p '{root}/gen'; echo 'by hand' > '{root}/gen/index'; git -C '{root}' add gen; git -C \
         '{root}' commit -qm 'an index by hand'; fi",
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
        format!("Regenerate the index for main at {}", &by_hand[..7])
    );
    assert_eq!(git(&r.root, &["show", "main:gen/index"]), "world.gd");
    assert!(
        said.contains(&format!(
            "regenerate: main changed gen/ since {}, at {}: what was made of it is dropped, and \
             made again of the newest tip",
            &tip[..7],
            &by_hand[..7]
        )),
        "{said}"
    );
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
            && line.ends_with(&format!("{red} ({RED})")),
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
    on_main(&r, "src/fixed", "");
    let (said, result) = regenerated(&r);
    result.unwrap();
    assert!(
        said.contains(&format!("regenerate: passed: closed {RED}")),
        "{said}"
    );
    assert_eq!(r.board.store.get(RED).unwrap().status, "cancelled");
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
            generator("the slow one", "gen/", "sleep 30", "")
        ),
    );
    let began = Instant::now();
    let line = regenerated(&r).1.unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "{:?}",
        began.elapsed()
    );
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
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(made(&r), 0);
    assert!(State::read(&lands(&r)).is_none());
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
    for state in ["building", "dropped"] {
        wrote(state, pid, symphony_board::land::started(pid));
        let (line, current) = status(&r);
        assert!(!current && making(&r));
        assert!(
            line.starts_with(&format!(
                "the index: making them of main at {} since ",
                &top[..7]
            )) && line.ends_with(&format!("(pid {pid}); main is 2 commits past it")),
            "{line}"
        );
        wrote(state, dead_pid(), None);
        let (line, current) = status(&r);
        assert!(!current && !making(&r));
        assert!(
            line.contains("ended without a word; the next land starts another"),
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
            && line.ends_with(&format!(": what it came to ({RED})")),
        "{line}"
    );
    wrote("stopped", dead_pid(), None);
    let (line, current) = status(&r);
    assert!(!current && !making(&r));
    assert!(
        line.starts_with(&format!("the index: stopped on main at {} (", &top[..7]))
            && line.ends_with("): what it came to"),
        "{line}"
    );
}

#[test]
fn the_command_line_regenerates_only_by_the_land_and_its_status_exits_by_it() {
    let r = Repo::new();
    generating(&r, &[&index(&r)]);
    let run = |args: &[&str], env: &symphony_board::cli::Env| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = symphony_board::cli::main(&args, env, &mut "".as_bytes(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
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
    // The index waits until the land has returned, so the land cannot have waited for it.
    let returned = r.dir.join("returned");
    let script = format!(
        "i=0; while [ ! -e '{returned}' ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; \
         mkdir -p gen && ls sim > gen/index",
        returned = returned.display()
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
