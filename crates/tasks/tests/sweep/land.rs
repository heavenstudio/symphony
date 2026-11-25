//! `tasks land` (`just land SLUG`) in scratch repositories on a board of their own: what lands,
//! what each refusal leaves (the default branch where it was, one line saying why), the default
//! branch moving under a land, two lands at once, the project's generators (which a land never
//! runs, and the regenerator does), and the push to a scratch bare remote.

use crate::children::Child as _;
use crate::{Repo, dead_pid, git, write};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphony_board::NewLaunch;
use symphony_board::land::{DIRTY_MAIN, RECIPE, Record, Stop, land};
use symphony_board::on_land::SAYS_VAR;
use symphony_board::sweep::size;

const SLUG: &str = "sim/jump";

/// A task's people: who orchestrates it, works it and reviews it.
struct People {
    orch: String,
    exe: String,
    rev: String,
}

/// The project file on main: `[verdicts] check` counts its runs in `runs` beside the repository
/// and is red on a tree holding `red`, with `more` after it. Committed on main.
fn manifest(r: &Repo, more: &str) {
    let runs = r.dir.join("runs");
    let check = format!(
        "echo \"$TASKS_TASK $TASKS_SESSION\" >> '{}'; test ! -e red",
        runs.display()
    );
    checked_with(r, &check, more);
}

/// The project file on main with `check`, run by `sh -c`, as its check.
fn checked_by(r: &Repo, check: &str) {
    checked_with(r, check, "");
}

/// The project file on main with `check`, run by `sh -c`, as its check, and `more` after it.
fn checked_with(r: &Repo, check: &str, more: &str) {
    let text = format!(
        "[verdicts]\ncheck = [\"sh\", \"-c\", {}]\n{more}",
        toml_string(check)
    );
    project_file(r, &text);
}

/// The project file on main as `text`, with whatever else the main checkout has added.
fn project_file(r: &Repo, text: &str) {
    write(&r.root, "symphony.toml", text);
    git(&r.root, &["add", "symphony.toml"]);
    git(&r.root, &["commit", "-qm", "the project file"]);
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// How many times the check ran, and for whom.
fn runs(r: &Repo) -> Vec<String> {
    fs::read_to_string(r.dir.join("runs"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// `slug`'s branch in a worktree of its own from main, with one commit writing `file` as `text`:
/// the worktree.
fn branch(r: &Repo, slug: &str, file: &str, text: &str) -> PathBuf {
    let wt = r.add(&slug.replace('/', "-"), Some(slug), "main");
    write(&wt, file, text);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", &format!("work on {slug}")]);
    wt
}

/// `slug` filed, claimed, in review and judged by its reviewer with `verdict` and `text`.
fn judged(r: &Repo, slug: &str, verdict: &str, text: &str) -> People {
    let store = &r.board.store;
    let people = People {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        rev: r.board.session("reviewer"),
    };
    r.task(slug);
    store.take_role(slug, &people.orch, "orchestrator").unwrap();
    store.claim(slug, &people.exe).unwrap();
    store.move_to(slug, "review", &people.exe).unwrap();
    store.take_role(slug, &people.rev, "reviewer").unwrap();
    store.verdict(slug, &people.rev, verdict, text).unwrap();
    people
}

/// `slug` approved with `approve: text`.
fn approved(r: &Repo, slug: &str, text: &str) -> People {
    judged(r, slug, "approve", text)
}

/// The land of `slug` as `actor` from the main checkout: what it printed, and the line it ended on
/// or the refusal.
fn landed(r: &Repo, slug: &str, actor: &str) -> (String, Result<String, String>) {
    land_from(r, &r.root, slug, actor)
}

/// The land of `slug` as `actor` from the main checkout, the sweep it starts detached run by this
/// build, as work finishing starts one: what the land printed, the line it ended on, and what that
/// sweep wrote to its log under the line naming the land, once it wrote its summary there.
fn landed_and_swept(r: &Repo, slug: &str, actor: &str) -> (String, String, String) {
    let mut env = r.env(&r.root);
    env.vars.insert("TASKS_SWEEP".into(), "on".into());
    env.vars
        .insert("TASKS_BIN".into(), env!("CARGO_BIN_EXE_tasks").into());
    let mut out = Vec::new();
    let line = land(&r.board.store, &env, slug, actor, None, &mut out)
        .map_err(refusal)
        .unwrap();
    let said = String::from_utf8(out).unwrap();
    assert!(
        said.contains(&format!(
            "land {slug}: its finished worktrees are swept in the background (its log \
             symphony/tasks/sweep.log)"
        )),
        "{said}"
    );
    let log = r.root.join(".git/symphony/tasks/sweep.log");
    let why = format!("] the land of {slug}\n");
    let swept = || {
        let text = fs::read_to_string(&log).ok()?;
        let after = &text[text.find(&why)? + why.len()..];
        after
            .lines()
            .any(|l| l.starts_with("worktree sweep: "))
            .then(|| after.to_string())
    };
    wait_for(|| swept().is_some());
    (said, line, swept().unwrap())
}

fn land_from(r: &Repo, cwd: &Path, slug: &str, actor: &str) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let result = land(&r.board.store, &r.env(cwd), slug, actor, None, &mut out);
    (String::from_utf8(out).unwrap(), result.map_err(refusal))
}

fn refusal(stop: Stop) -> String {
    match stop {
        Stop::Refused(line) => line,
        Stop::Board(error) => panic!("the board failed: {error:?}"),
    }
}

fn head(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev])
}

/// The thread's last line.
fn last(r: &Repo, slug: &str) -> String {
    r.board.said(slug).last().cloned().unwrap_or_default()
}

/// The land refused `slug` with `line`, main as it was at `before` and the task where it stood.
fn assert_refused(r: &Repo, result: &Result<String, String>, line: &str, before: &str) {
    assert_eq!(result.as_ref().map_err(String::as_str), Err(line));
    assert_eq!(head(&r.root, "main"), before, "main stays where it was");
    assert!(
        !r.board.said(SLUG).iter().any(|l| l.starts_with("landed ")),
        "{:?}",
        r.board.said(SLUG)
    );
    // Its worktree went with it: nothing is left registered or in the way.
    let land_tree = r.root.join(".git/symphony/tasks/land/worktree");
    assert!(!land_tree.exists(), "{}", land_tree.display());
    assert!(
        !git(&r.root, &["worktree", "list"]).contains("symphony/tasks"),
        "{}",
        git(&r.root, &["worktree", "list"])
    );
}

// -- what lands ------------------------------------------------------------------------------------

#[test]
fn an_approved_branch_lands_as_one_merge_commit_checked_and_the_task_closes() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let base = head(&r.root, "main");
    let people = approved(&r, SLUG, &format!("covers {tip}, good"));
    let (said, result) = landed(&r, SLUG, &people.orch);
    let merge = head(&r.root, "main");
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    // One merge commit, its first parent main as it was, its second the approved tip.
    assert_eq!(head(&r.root, "main^1"), base);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main"]),
        format!("Merge branch '{SLUG}'")
    );
    // The main checkout moved with it and is clean.
    assert_eq!(head(&r.root, "HEAD"), merge);
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
    assert!(r.root.join("sim/jump.gd").is_file());
    // The check ran once, on the merge, as the land's actor for the task.
    assert_eq!(runs(&r), [format!("{SLUG} {}", people.orch)]);
    assert!(
        said.starts_with(&format!("land {SLUG}: approved by {} at ", people.rev)),
        "{said}"
    );
    assert!(
        said.contains(&format!("naming its tip {}", &tip[..7])),
        "{said}"
    );
    assert!(said.contains("checking (sh -c "), "{said}");
    // The task stays done, its thread ending on the land's line.
    let task = r.board.store.get(SLUG).unwrap();
    assert_eq!(task.status, "done");
    assert_eq!(last(&r, SLUG), line);
    // The land's worktree is gone, and the log holds what the check printed.
    assert!(!r.root.join(".git/symphony/tasks/land/worktree").exists());
    let log = fs::read_to_string(r.root.join(".git/symphony/tasks/land/sim%2Fjump.log")).unwrap();
    assert!(log.contains("$ sh -c "), "{log}");
    assert!(log.ends_with("exit 0\n"), "{log}");
}

#[test]
fn a_landing_starts_the_projects_on_land_command_with_its_slug_and_merge() {
    let r = Repo::new();
    let heard = r.dir.join("heard");
    // That a test's board starts none unless told is the switch's own test
    // (`on_land::tests::only_the_repositorys_own_board_starts_the_command_unless_told`). Here it is
    // told, with a generator, so the land starts the regenerator, and git's repository variables
    // in its caller's environment, which the command never sees.
    let hook = format!(
        "echo \"$@\" > '{}'; echo \"${{GIT_DIR:-none}} ${{GIT_INDEX_FILE:-none}}\" >> '{}'; \
         echo \"counted $1\"",
        heard.display(),
        heard.display()
    );
    manifest(
        &r,
        &format!(
            "\n[board]\non_land = [\"sh\", \"-c\", {}, \"hook\"]\n\n[land]\ngenerators = [{{ name \
             = \"the index\", outputs = \"gen/\", command = [\"true\"] }}]\n",
            toml_string(&hook)
        ),
    );
    // The regenerator the land starts is a stand-in that ends at once: nothing here reads what a
    // regenerator makes, which is `land::regenerate`'s.
    use std::os::unix::fs::PermissionsExt;
    let stand_in = r.dir.join("regenerator");
    write(&r.dir, "regenerator", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(&stand_in, fs::Permissions::from_mode(0o755)).unwrap();
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let mut env = r.env(&r.root);
    env.vars.insert("TASKS_ON_LAND".into(), "on".into());
    env.vars.insert("TASKS_REGENERATE".into(), "on".into());
    env.vars
        .insert("TASKS_BIN".into(), stand_in.to_string_lossy().into_owned());
    env.vars.insert("GIT_DIR".into(), "/nowhere/.git".into());
    env.vars
        .insert("GIT_INDEX_FILE".into(), "/nowhere/index".into());
    // The backstop of the land's own 3 s window, which no loaded machine's count outlasts: the land
    // listens until the command ends.
    let window = test_time::backstop(symphony_board::on_land::SAYS);
    env.vars
        .insert(SAYS_VAR.into(), window.as_secs().to_string());
    let mut out = Vec::new();
    let line = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out).unwrap();
    let said = String::from_utf8(out).unwrap();
    let merge = head(&r.root, "main");
    assert!(line.starts_with(&format!("landed {SLUG} on main at {}", &merge[..12])));
    // The regenerator is started first, so what the command starts finds it started.
    let regenerator = said
        .find(&format!("land {SLUG}: the regenerator makes the index"))
        .unwrap_or_else(|| panic!("{said}"));
    let started = said
        .find("on_land: started")
        .unwrap_or_else(|| panic!("{said}"));
    assert!(regenerator < started, "{said}");
    // The land shows what its command said at once, and names the file of its own it says it in.
    let starts = r.root.join(".git/symphony/tasks/on-land");
    let output = fs::read_dir(&starts)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        said.contains(&format!(
            "on_land: started sh -c {hook} hook {SLUG} {merge} (its lines go to {})\n  counted \
             {SLUG}\n",
            output.display()
        )),
        "{said}"
    );
    assert!(!said.contains("it runs on"), "{said}");
    let index = fs::read_to_string(r.root.join(".git/symphony/tasks/on-land.log")).unwrap();
    assert!(
        index.contains(&format!("(its lines: {})", output.display())),
        "{index}"
    );
    // Heard to its end within the window, the command wrote both lines before its count.
    assert_eq!(
        fs::read_to_string(&heard).unwrap(),
        format!("{SLUG} {merge}\nnone none\n"),
        "after the task closed, never before, naming no repository to git"
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

#[test]
fn the_lands_sweep_removes_its_in_process_executors_worktree_and_says_what_it_freed() {
    // A Claude Code subagent has no launch record. Its session names its own `agent-ID` worktree by
    // its agent id; it left, so its roles are released, and the worktree is on the task's branch.
    // One that is gone and holds its role still is the sweep's own
    // `causes::the_lands_pass_removes_its_subagents_worktree_and_names_the_bytes`.
    let r = Repo::new();
    manifest(&r, "");
    let agent = "a5a6262f496f70d23";
    let wt = r.add(&format!("agent-{agent}"), Some(SLUG), "main");
    write(&wt, "sim/jump.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work on sim/jump"]);
    write(&wt, ".target/debug/deps/libjump.rlib", "compiled\n");
    let people = approved(&r, SLUG, "good");
    let store = &r.board.store;
    assert!(store.bind_agent(agent, &people.exe, false).unwrap());
    store.leave(&people.exe).unwrap();
    let held = size::shown(size::bytes(&wt));
    let (_, _, swept) = landed_and_swept(&r, SLUG, &people.orch);
    r.assert_removed(&wt, &swept);
    for line in [
        format!("; {held})"),
        format!("deleted branch {SLUG} (merged into main, its worktree removed)"),
        format!("; freed {held}"),
    ] {
        assert!(swept.contains(&line), "{line}\n{swept}");
    }
}

#[test]
fn the_lands_sweep_says_in_its_log_the_evidence_set_aside_past_its_cap() {
    let r = Repo::new();
    // 1,000 bytes: the picture set aside goes past it.
    manifest(&r, "[board]\nswept_cap_gb = 0.000001\n");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    write(&wt, "artifacts/sim-jump/shot.png", &"p".repeat(5_000));
    let people = approved(&r, SLUG, "good");
    let store = &r.board.store;
    store
        .record_launch(&NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            pane: "w1:p1".into(),
            ..NewLaunch::new(SLUG, &people.exe, "executor")
        })
        .unwrap();
    store
        .close_task_session(SLUG, &people.exe, "idle", "just tasks restore")
        .unwrap();
    // The land says it starts the sweep; the sweep says in its log what it set aside.
    let (said, line, logged) = landed_and_swept(&r, SLUG, &people.orch);
    assert!(
        line.starts_with(&format!("landed {SLUG} on main at ")),
        "{line}"
    );
    assert!(!said.contains("set aside"), "{said}");
    r.assert_removed(&wt, &logged);
    let swept = r.root.join("artifacts/swept");
    assert!(
        swept.join("sim-jump/sim-jump/shot.png").is_file(),
        "{logged}"
    );
    let lines: Vec<&str> = logged
        .lines()
        .filter(|l| l.starts_with("set aside in "))
        .collect();
    assert_eq!(lines.len(), 1, "{logged}");
    assert!(
        lines[0].starts_with(&format!(
            "set aside in {}: {}, over its 0.000001 GB cap ([board] swept_cap_gb): ",
            swept.display(),
            size::shown(size::bytes(&swept))
        )) && lines[0].ends_with(&format!(
            "; the largest: sim-jump {} (task {SLUG}, done; no reference found); suggested, as \
             nothing found references them: sim-jump",
            size::shown(size::bytes(&swept.join("sim-jump")))
        )),
        "{logged}"
    );
}

#[test]
fn the_command_line_lands_and_refuses_with_one_line() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let first = head(&wt, "HEAD");
    let people = judged(&r, SLUG, "changes", &format!("{first}: it jumps twice"));
    let args = |slug: &str| -> Vec<String> {
        ["land", slug, "--as", &people.orch]
            .iter()
            .map(|a| a.to_string())
            .collect()
    };
    // As `just land` runs it, its recipe setting the marker.
    let mut env = r.env(&r.root);
    env.vars.insert(RECIPE.into(), "1".into());
    let run_in = |args: &[String], env: &symphony_board::cli::Env| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = symphony_board::cli::main(args, env, &mut "".as_bytes(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let run = |args: &[String]| run_in(args, &env);
    let before = head(&r.root, "main");
    // Run as `just tasks land`, by no recipe: refused before it reads the board.
    let (code, _, err) = run_in(&args(SLUG), &r.env(&r.root));
    assert_ne!(code, 0);
    assert_eq!(
        err.trim_end().lines().last().unwrap(),
        "✖ `tasks land` runs only as `just land SLUG`, the recipe that moves and pushes the \
         default branch",
        "{err}"
    );
    // A project whose justfile has a `land` of its own lands by it alone, marker or not.
    write(&r.root, "justfile", "land *ARGS:\n    @echo its own\n");
    let (code, _, err) = run(&args(SLUG));
    assert_ne!(code, 0);
    assert_eq!(
        err.trim_end().lines().last().unwrap(),
        format!(
            "✖ {} lands with a `land` recipe of its own, which replaces the Symphony's \
             everywhere: run `just land SLUG` there",
            r.root.join("justfile").display()
        ),
        "{err}"
    );
    fs::remove_file(r.root.join("justfile")).unwrap();
    let (code, _, err) = run(&args(SLUG));
    assert_ne!(code, 0);
    assert_eq!(
        err.trim_end().lines().last().unwrap(),
        format!("✖ {}", unapproved(SLUG, "in_progress")),
        "{err}"
    );
    assert_eq!(head(&r.root, "main"), before);

    // A later round goes to the same reviewer, as the skills say: its own earlier changes leave it
    // a fresh reviewer, and its approve of the fixed tip lands the task.
    write(&wt, "sim/jump.gd", "extends Node # once\n");
    git(&wt, &["commit", "-qam", "jump once"]);
    let fixed = head(&wt, "HEAD");
    r.board.store.move_to(SLUG, "review", &people.exe).unwrap();
    r.board
        .store
        .verdict(SLUG, &people.rev, "approve", &format!("{fixed}: once"))
        .unwrap();
    let (code, out, err) = run(&args(SLUG));
    assert_eq!(code, 0, "{err}");
    let merge = head(&r.root, "main");
    let by = format!("land {SLUG}: approved by {} at ", people.rev);
    assert!(out.starts_with(&by), "{out}");
    assert_eq!(head(&r.root, "main^2"), fixed);
    assert!(
        out.ends_with(&format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote\n",
            &merge[..12]
        )),
        "{out}"
    );
}

#[test]
fn a_land_from_a_linked_worktree_lands_on_the_main_checkout() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    // Read before the land, whose sweep removes the landed worktree and deletes its branch.
    let tip = head(&wt, "HEAD");
    // An approval naming no commit lands by time, and says so on the thread.
    let people = approved(&r, SLUG, "good");
    let (said, result) = land_from(&r, &wt, SLUG, &people.orch);
    let line = result.unwrap();
    assert_eq!(head(&r.root, "HEAD"), head(&r.root, "main"));
    assert_eq!(head(&r.root, "main^2"), tip);
    assert!(said.contains("before every commit to its tip"), "{said}");
    let landed = line.strip_suffix("; not pushed: no origin remote").unwrap();
    assert_eq!(
        last(&r, SLUG),
        format!(
            "{landed}; judged by time, the approval naming no commit of the branch: its 1 commit \
             is older than the approval; not pushed: no origin remote"
        )
    );
}

#[test]
fn the_land_worktree_goes_in_the_projects_worktrees_folder_when_it_names_one() {
    let r = Repo::new();
    let runs = r.dir.join("where");
    let check = format!("pwd -P >> '{}'", runs.display());
    checked_with(&r, &check, "\n[agents]\nworktrees = \"../wt\"\n");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    landed(&r, SLUG, &people.orch).1.unwrap();
    let ran = fs::read_to_string(&runs).unwrap();
    assert_eq!(ran.trim(), r.dir.join("wt/land").to_string_lossy());
    assert!(!r.dir.join("wt/land").exists());
}

#[test]
fn a_project_with_no_file_lands_on_main_with_no_check() {
    let r = Repo::new();
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&r.root, SLUG);
    let people = approved(&r, SLUG, "good");
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert!(!said.contains("checking"), "{said}");
    let thread = r.board.said(SLUG);
    assert!(!thread.iter().any(|l| l.contains("checked")), "{thread:?}");
    assert_eq!(head(&r.root, "main^2"), tip);
}

#[test]
fn a_project_whose_default_branch_is_not_main_lands_there() {
    let r = Repo::new();
    git(&r.root, &["branch", "-m", "main", "trunk"]);
    project_file(&r, "[project]\ndefault_branch = \"trunk\"\n");
    let wt = r.add("sim-jump", Some(SLUG), "trunk");
    write(&wt, "sim/jump.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work"]);
    let people = approved(&r, SLUG, "good");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(
        line.starts_with(&format!("landed {SLUG} on trunk at ")),
        "{line}"
    );
    assert_eq!(head(&r.root, "trunk^2"), head(&wt, "HEAD"));
}

// -- what it refuses, main untouched ---------------------------------------------------------------

#[test]
fn a_task_with_no_approval_is_refused_on_its_thread() {
    for (verdict, status) in [("changes", "in_progress"), ("approve", "done")] {
        let r = Repo::new();
        manifest(&r, "");
        branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
        let before = head(&r.root, "main");
        let people = judged(&r, SLUG, verdict, "judged");
        if verdict == "approve" {
            // An approval the board then closed another way stands no more.
            r.board
                .store
                .landed(SLUG, &people.orch, "done: elsewhere", true, &["done"])
                .unwrap();
        }
        let (_, result) = landed(&r, SLUG, &people.orch);
        let line = if status == "done" {
            format!("{SLUG} was closed after its approval: done: elsewhere")
        } else {
            unapproved(SLUG, status)
        };
        assert_refused(&r, &result, &line, &before);
        assert_eq!(last(&r, SLUG), format!("land refused: {line}"));
        assert!(runs(&r).is_empty(), "no check runs for a refusal");
    }
}

#[test]
fn a_commit_after_the_approved_one_is_refused_naming_it() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let approved_tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{approved_tip} is good"));
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    git(&wt, &["commit", "-qam", "one more thing"]);
    let late = head(&wt, "HEAD");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "{SLUG} has 1 commit after the approved {}: {} one more thing",
        &approved_tip[..7],
        &late[..7]
    );
    assert_refused(&r, &result, &line, &before);
    // Back in review, its reviewer told what to read and how to approve what it read.
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        told.text,
        format!(
            "land refused: {line}; {SLUG} is back in review. Read what the approve does not \
             cover, then approve the tip naming it: `just tasks verdict {SLUG} approve \
             \"{late}: …\"`"
        )
    );
    assert_eq!(told.recipient.as_deref(), Some("@reviewer"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "review");
    // A fresh approve naming the new tip lands it.
    r.board
        .store
        .verdict(
            SLUG,
            &people.rev,
            "approve",
            &format!("{late}: one more thing is fine"),
        )
        .unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert!(
        said.contains(&format!("naming its tip {}", &late[..7])),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^2"), late);
}

#[test]
fn a_commit_made_after_an_approval_naming_none_is_refused_by_its_time() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    // Past the board's clock, where the approval stands.
    let when = format!("@{:.0} +0000", r.board.now() + 600.0);
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(["commit", "-qam", "after the verdict"])
            .env("GIT_COMMITTER_DATE", &when)
            .env("GIT_AUTHOR_DATE", &when)
            .current_dir(&wt),
    )
    .unwrap();
    assert!(done.status.success());
    let late = head(&wt, "HEAD");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.clone().unwrap_err();
    assert!(
        line.starts_with(&format!("{SLUG} has 1 commit made after its approval at ")),
        "{line}"
    );
    assert!(
        line.ends_with(&format!(
            "(which names none of its commits, so judged by time): {} after the verdict",
            &late[..7]
        )),
        "{line}"
    );
    assert_refused(&r, &result, &line, &before);
}

#[test]
fn an_approval_naming_a_commit_the_tip_does_not_hold_is_refused() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let first = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{first} is good"));
    // The branch was rewritten after the approval: its tip no longer holds the approved commit.
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    git(&wt, &["commit", "-q", "--amend", "-am", "rewritten"]);
    let tip = head(&wt, "HEAD");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "the approval of {SLUG} names {}, which its tip {} does not hold",
        &first[..7],
        &tip[..7]
    );
    assert_refused(&r, &result, &line, &before);
}

#[test]
fn a_red_check_is_refused_with_its_log_and_main_stays() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "red", "broken\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let log = r.root.join(".git/symphony/tasks/land/sim%2Fjump.log");
    let line = format!(
        "the check `sh -c echo \"$TASKS_TASK $TASKS_SESSION\" >> '{}'; test ! -e red` is red on \
         {SLUG} merged into main at {}; its output is in {}",
        r.dir.join("runs").display(),
        &before[..7],
        log.canonicalize().unwrap().display()
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(runs(&r).len(), 1);
    assert!(fs::read_to_string(&log).unwrap().ends_with("exit 1\n"));
    // Said on the thread to its executor: the first try is the branch's own red, its to fix.
    let thread = r.board.store.thread(SLUG).unwrap();
    let said = thread.last().unwrap();
    assert_eq!(
        said.text,
        format!(
            "land refused: {line}. Merge main into {SLUG} and fix it there, then move it back to \
             review (`just tasks move {SLUG} review`) and ask its reviewer to approve the new \
             tip naming it"
        )
    );
    assert_eq!(said.recipient.as_deref(), Some("@executor"), "{said:?}");
    // It stays done, its approve standing, until the executor sends it back.
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

#[test]
fn a_dirty_main_checkout_is_refused_and_left_as_it_is() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    write(&r.root, "sim/world.gd", "edited in place\n");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!("the main checkout {} {DIRTY_MAIN}", r.root.display());
    assert_refused(&r, &result, &line, &before);
    assert_eq!(
        fs::read_to_string(r.root.join("sim/world.gd")).unwrap(),
        "edited in place\n"
    );
    // An untracked file is no edit of main's: that lands.
    git(&r.root, &["checkout", "--", "sim/world.gd"]);
    write(&r.root, "notes.txt", "mine\n");
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(
        fs::read_to_string(r.root.join("notes.txt")).unwrap(),
        "mine\n"
    );
}

#[test]
fn a_main_checkout_on_another_branch_is_refused() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    git(&r.root, &["checkout", "-q", "-b", "elsewhere"]);
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "the main checkout {} has elsewhere checked out, not main",
        r.root.display()
    );
    assert_refused(&r, &result, &line, &before);
}

/// A conflict is refused naming its files, and the way out of that refused land: its executor
/// merges main into the branch and resolves it there, sends the task back to review, its reviewer
/// approves the new tip naming it, and it lands.
#[test]
fn a_conflict_fixed_on_the_branch_and_approved_again_lands() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/world.gd", "extends Node\n");
    let people = approved(&r, SLUG, &format!("{} is good", head(&wt, "HEAD")));
    write(&r.root, "sim/world.gd", "extends Object\n");
    git(&r.root, &["commit", "-qam", "main moved its way"]);
    let before = head(&r.root, "main");
    // The conflict is refused naming its files, never resolved, and no check runs.
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "{SLUG} conflicts with main at {}: sim/world.gd",
        &before[..7]
    );
    assert_refused(&r, &result, &line, &before);
    let told = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(
        told.text,
        format!(
            "land refused: {line}. Merge main into {SLUG} and fix it there, then move it back to \
             review (`just tasks move {SLUG} review`) and ask its reviewer to approve the new \
             tip naming it"
        )
    );
    assert_eq!(told.recipient.as_deref(), Some("@executor"));
    assert!(runs(&r).is_empty());
    // Approved still: the land asks the executor, which merges main and resolves on the branch.
    let store = &r.board.store;
    assert_eq!(store.get(SLUG).unwrap().status, "done");
    let merging = symphony_board::sys::output(
        symphony_board::sys::Command::of("git")
            .args(["merge", "-q", "main"])
            .current_dir(&wt),
    )
    .unwrap();
    assert!(!merging.status.success(), "the branch meets the conflict");
    write(&wt, "sim/world.gd", "extends Node # and Object\n");
    git(&wt, &["commit", "-qam", "merge main, resolved"]);
    let fixed = head(&wt, "HEAD");
    store.move_to(SLUG, "review", &people.exe).unwrap();
    // The old approve no longer stands: no land until a fresh one.
    let line = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert_eq!(line, unapproved(SLUG, "review"));
    store
        .verdict(
            SLUG,
            &people.rev,
            "approve",
            &format!("{fixed}: the merge reads right"),
        )
        .unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert!(
        said.contains(&format!("naming its tip {}", &fixed[..7])),
        "{said}"
    );
    assert_eq!(head(&r.root, "main^1"), before);
    assert_eq!(head(&r.root, "main^2"), fixed);
    assert_eq!(
        fs::read_to_string(r.root.join("sim/world.gd")).unwrap(),
        "extends Node # and Object\n"
    );
    assert_eq!(last(&r, SLUG), line);
    assert_eq!(store.get(SLUG).unwrap().status, "done");
}

/// The files in the lands' folder that keep a refusal said (`SLUG.said`).
fn refusals_kept(r: &Repo) -> Vec<String> {
    let folder = r.root.join(".git/symphony/tasks/land");
    let mut kept: Vec<String> = (fs::read_dir(&folder).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".said"))
        .collect();
    symphony_board::order::sort_by(&mut kept, &Ord::cmp);
    kept
}

#[test]
fn a_land_refused_once_and_then_landed_here_leaves_no_refusal_kept() {
    // Only a delivery by pull request reads the last refusal said, and only its end forgets it:
    // a land that merges here keeps none, so a slug filed again finds no refusal it never made.
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = judged(&r, SLUG, "changes", "not yet");
    let refused = landed(&r, SLUG, &people.orch).1.unwrap_err();
    assert_eq!(refused, unapproved(SLUG, "in_progress"));
    assert_eq!(last(&r, SLUG), format!("land refused: {refused}"));
    assert_eq!(refusals_kept(&r), [""; 0], "none kept by the refusal");
    // Refused again at the same head: said again, as a land a person runs always says.
    landed(&r, SLUG, &people.orch).1.unwrap_err();
    let said = (r.board.store.thread(SLUG).unwrap().iter())
        .filter(|post| post.text == format!("land refused: {refused}"))
        .count();
    assert_eq!(said, 2);
    let store = &r.board.store;
    store.move_to(SLUG, "review", &people.exe).unwrap();
    let tip = head(&wt, "HEAD");
    store
        .verdict(SLUG, &people.rev, "approve", &format!("{tip}: reads well"))
        .unwrap();
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(store.get(SLUG).unwrap().status, "done");
    assert_eq!(refusals_kept(&r), [""; 0], "none left by the land");
}

/// The refusal of a task in `status` with no approve standing: who approves it, and by what.
fn unapproved(slug: &str, status: &str) -> String {
    let approve = format!(
        "a fresh reviewer, one that did not write it, approves it naming the tip it read: `just \
         tasks verdict {slug} approve \"FULL_SHA: why\" --as HANDLE`"
    );
    match status {
        "review" => format!("{slug} is review, with no approving verdict standing: {approve}"),
        _ => format!(
            "{slug} is {status}, with no approving verdict standing: it goes to review (`just \
             tasks move {slug} review`), where {approve}"
        ),
    }
}

#[test]
fn a_branch_already_on_main_or_not_there_is_refused() {
    let r = Repo::new();
    manifest(&r, "");
    let before = head(&r.root, "main");
    let people = approved(&r, SLUG, &format!("{before}: good"));
    let (_, result) = landed(&r, SLUG, &people.orch);
    // The approve names main's tip, no branch of the work's: the rename is said with the branch to
    // name, never main's.
    assert_refused(
        &r,
        &result,
        &format!(
            "no branch {SLUG} to land: the land takes the branch named as the task, {SLUG}: \
             rename the branch holding the approved work (`git branch -m BRANCH {SLUG}`), then \
             land again"
        ),
        &before,
    );

    git(&r.root, &["branch", SLUG, "main"]);
    let (_, result) = landed(&r, SLUG, &people.orch);
    // No merge brought it: what landed it is the tip itself.
    assert_refused(&r, &result, &outside(&r, &before, &before), &before);
}

/// Work on a branch not named as the task: the land names the branch whose tip is the commit the
/// approve names, and the rename, and lands once the branch is renamed.
#[test]
fn work_on_a_branch_not_named_as_the_task_is_refused_naming_it_and_its_rename() {
    let r = Repo::new();
    manifest(&r, "");
    let wt = branch(&r, "bye", "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: it reads right"));
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "no branch {SLUG} to land: the land takes the branch named as the task, {SLUG}, and the \
         approved tip {} is on bye: `git branch -m bye {SLUG}` renames it, then land again",
        &tip[..7]
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(last(&r, SLUG), format!("land refused: {line}"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");

    git(&r.root, &["branch", "-m", "bye", SLUG]);
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
}

/// An approve on the command line says the task is not on the default branch until it lands, and
/// what lands it: the default landing, or the project's own `land`, which replaces it.
#[test]
fn an_approve_names_the_default_branch_and_the_land_that_lands_it() {
    let r = Repo::new();
    git(&r.root, &["branch", "trunk", "main"]);
    project_file(&r, "[project]\ndefault_branch = \"trunk\"\n");
    let approve = |slug: &str| {
        let store = &r.board.store;
        let (exe, rev) = (r.board.session("worker"), r.board.session("reviewer"));
        r.task(slug);
        store.claim(slug, &exe).unwrap();
        store.move_to(slug, "review", &exe).unwrap();
        store.take_role(slug, &rev, "reviewer").unwrap();
        let args: Vec<String> = ["verdict", slug, "approve", "good", "--as", &rev]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = r.env(&r.root);
        let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        String::from_utf8(out).unwrap()
    };
    assert_eq!(
        approve("sim/a"),
        "sim/a is approved: done on the board, not on trunk until it lands; `just land sim/a --as \
         HANDLE` lands it\n"
    );
    write(&r.root, "justfile", "land *ARGS:\n    @echo its own\n");
    assert_eq!(
        approve("sim/b"),
        format!(
            "sim/b is approved: done on the board, not on trunk until it lands; the `land` recipe \
             of the project's own lands it ({}): `just land sim/b`\n",
            r.root.join("justfile").display()
        )
    );
}

/// The refusal of a tip on main that no record of a land's merge covers, `merge` the commit that
/// brought it: merged outside land, with how to finish by hand a land whose record was lost.
fn outside(r: &Repo, tip: &str, merge: &str) -> String {
    format!(
        "{SLUG}'s tip {} is on main already, merged outside land: nothing to finish. Were it a \
         land's merge whose record ({}) was lost, finish it by hand: push main once a check \
         passed on what it holds (`git push origin main`), then say the landing on the thread \
         (`just tasks say {SLUG} \"landed {SLUG} on main at {}\"`) and sweep its worktrees (`just \
         tasks sweep`)",
        &tip[..7],
        Record::path(&r.root.join(".git/symphony/tasks/land"), SLUG).display(),
        &merge[..12]
    )
}

#[test]
fn a_task_the_board_does_not_hold_is_the_boards_refusal() {
    let r = Repo::new();
    let actor = r.board.session("orchestrator");
    let mut out = Vec::new();
    match land(
        &r.board.store,
        &r.env(&r.root),
        "sim/none",
        &actor,
        None,
        &mut out,
    ) {
        Err(Stop::Refused(line)) => assert!(line.contains("sim/none"), "{line}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_project_file_the_land_cannot_read_is_refused_before_anything() {
    let r = Repo::new();
    project_file(
        &r,
        "[land]\ngenerators = [{ name = \"x\", outputs = \"../up\", command = [\"true\"] }]\n",
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_refused(
        &r,
        &result,
        "symphony.toml: [land] generator x writes \"../up\", not a path inside the project",
        &before,
    );
}

#[test]
fn a_folder_in_the_way_of_the_land_worktree_is_never_touched() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let in_way = r.root.join(".git/symphony/tasks/land/worktree");
    write(&in_way, "keep.txt", "mine\n");
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "{} is in the way of the land's worktree: move it, or name another [agents] \
             worktrees folder",
            in_way.display()
        )
    );
    assert_eq!(
        fs::read_to_string(in_way.join("keep.txt")).unwrap(),
        "mine\n"
    );
    // Another worktree there is not the land's either.
    fs::remove_dir_all(&in_way).unwrap();
    git(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &in_way.to_string_lossy(),
            "main",
        ],
    );
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert!(
        result
            .unwrap_err()
            .contains("is another worktree, where a land makes its own"),
    );
    assert!(in_way.join("sim/world.gd").is_file());
    // A land's own whose land still runs is kept: its lock names a live pid (init's, here).
    git(
        &r.root,
        &["worktree", "remove", "--force", &in_way.to_string_lossy()],
    );
    let leftover = |reason: &str| {
        git(
            &r.root,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                "--lock",
                "--reason",
                reason,
                &in_way.to_string_lossy(),
                "main",
            ],
        );
    };
    leftover("symphony land: x (pid 1)");
    assert_eq!(
        landed(&r, SLUG, &people.orch).1.unwrap_err(),
        format!(
            "{} is the worktree of a land still running (pid 1, symphony land: x (pid 1)): wait \
             for it to end",
            in_way.display()
        )
    );
    assert!(in_way.join("sim/world.gd").is_file());
    // One left behind by a land that ended (killed part way) is replaced.
    git(&r.root, &["worktree", "unlock", &in_way.to_string_lossy()]);
    git(
        &r.root,
        &["worktree", "remove", "--force", &in_way.to_string_lossy()],
    );
    leftover(&format!("symphony land: x (pid {})", dead_pid()));
    landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(!in_way.exists());
}

// -- the default branch moving under a land --------------------------------------------------------

/// A check that, the first time it runs, commits on main in the main checkout: main moves while the
/// land checks.
fn moving_check(r: &Repo, then: &str) {
    let runs = r.dir.join("runs");
    let moved = r.dir.join("moved");
    let check = format!(
        "echo ran >> '{runs}'; if [ ! -e '{moved}' ]; then touch '{moved}'; git -C '{root}' \
         commit -q --allow-empty -m 'main moved'; fi; {then}",
        runs = runs.display(),
        moved = moved.display(),
        root = r.root.display()
    );
    checked_by(r, &check);
}

#[test]
fn main_moving_during_the_check_merges_and_checks_again_on_its_new_tip() {
    let r = Repo::new();
    moving_check(&r, "true");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, "good");
    let (said, result) = landed(&r, SLUG, &people.orch);
    result.unwrap();
    assert_eq!(runs(&r).len(), 2, "{said}");
    let moved = head(&r.root, "main^1");
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", &moved]),
        "main moved"
    );
    assert_eq!(head(&r.root, "main^2"), tip);
    assert!(
        said.contains(&format!(
            "land {SLUG}: main moved to {}; merging and checking again",
            &moved[..7]
        )),
        "{said}"
    );
}

#[test]
fn a_red_after_main_moved_is_said_to_the_orchestrator() {
    let r = Repo::new();
    // Green on the first try, red on the second: main moved to something that breaks the branch.
    let red_again = format!("test $(wc -l < '{}') -lt 2", r.dir.join("runs").display());
    moving_check(&r, &red_again);
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(line.starts_with("the check `sh -c "), "{line}");
    assert_ne!(head(&r.root, "main"), before, "main moved by itself");
    let thread = r.board.store.thread(SLUG).unwrap();
    let said = thread.last().unwrap();
    assert_eq!(said.text, format!("land refused: {line}"));
    assert_eq!(said.recipient.as_deref(), Some("@orchestrator"), "{said:?}");
}

#[test]
fn main_that_keeps_moving_lands_nothing_after_its_tries() {
    let r = Repo::new();
    checked_by(
        &r,
        &format!(
            "git -C '{}' commit -q --allow-empty -m 'main moved'",
            r.root.display()
        ),
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let (said, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap_err(),
        format!("main moved 3 times while {SLUG} was merged; nothing landed: run the land again")
    );
    assert_eq!(
        said.matches("merging and checking again").count(),
        2,
        "{said}"
    );
    assert_eq!(
        git(&r.root, &["branch", "--list", "--merged", "main", SLUG]),
        "",
        "the branch is on no main"
    );
    assert!(!r.board.said(SLUG).iter().any(|l| l.starts_with("landed ")));
}

#[test]
fn a_main_checkout_the_merge_would_overwrite_is_refused_and_kept() {
    let r = Repo::new();
    manifest(&r, "");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // Untracked in the main checkout, and added by the branch: the fast-forward would overwrite it.
    write(&r.root, "sim/jump.gd", "mine\n");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap_err();
    assert!(
        line.starts_with("main could not move to ") && line.contains(" in the main checkout: "),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(
        fs::read_to_string(r.root.join("sim/jump.gd")).unwrap(),
        "mine\n"
    );
    assert_eq!(runs(&r).len(), 1, "it was checked, then could not move");
}

#[test]
fn a_check_named_by_a_path_runs_the_merges_own_script_and_one_missing_is_refused() {
    let r = Repo::new();
    write(
        &r.root,
        "tools/check.sh",
        &format!(
            "#!/bin/sh\npwd -P > '{0}'\ngit rev-parse --show-toplevel >> '{0}' && echo \
             \"${{GIT_DIR:-none}}\" >> '{0}'\n",
            r.dir.join("ran").display()
        ),
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        r.root.join("tools/check.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    git(&r.root, &["add", "tools/check.sh"]);
    project_file(&r, "[verdicts]\ncheck = [\"tools/check.sh\"]\n");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    // Git's repository variables in its caller's environment, which the check never sees: its git
    // reads the land's worktree.
    let mut env = r.env(&r.root);
    env.vars.insert("GIT_DIR".into(), "/nowhere/.git".into());
    env.vars
        .insert("GIT_INDEX_FILE".into(), "/nowhere/index".into());
    let mut out = Vec::new();
    land(&r.board.store, &env, SLUG, &people.orch, None, &mut out).unwrap();
    let ran = fs::read_to_string(r.dir.join("ran")).unwrap();
    let lines: Vec<&str> = ran.lines().collect();
    assert_eq!(lines.len(), 3, "{ran}");
    for line in &lines[..2] {
        assert!(line.ends_with(".git/symphony/tasks/land/worktree"), "{ran}");
    }
    assert_eq!(lines[2], "none", "{ran}");

    project_file(&r, "[verdicts]\ncheck = [\"no-such-check-anywhere\"]\n");
    branch(&r, "sim/fall", "sim/fall.gd", "extends Node\n");
    let fall = approved(&r, "sim/fall", "good");
    let before = head(&r.root, "main");
    let line = landed(&r, "sim/fall", &fall.orch).1.unwrap_err();
    assert!(
        line.starts_with("no-such-check-anywhere: "),
        "a check that cannot start says so: {line}"
    );
    assert_eq!(head(&r.root, "main"), before);
}

#[test]
fn two_lands_at_once_go_one_after_the_other() {
    let r = Repo::new();
    let started = r.dir.join("started");
    let go = r.dir.join("go");
    // The check waits for the test's word, for six minutes of its looks at most (the backstop of
    // the minute it once had), and fails past them.
    let check = format!(
        "echo \"$TASKS_TASK\" >> '{runs}'; touch '{started}'; i=0; while [ ! -e '{go}' ] && \
         [ $i -lt 3600 ]; do sleep 0.1; i=$((i+1)); done; test -e '{go}'",
        runs = r.dir.join("runs").display(),
        started = started.display(),
        go = go.display()
    );
    checked_by(&r, &check);
    branch(&r, "sim/jump", "sim/jump.gd", "extends Node\n");
    branch(&r, "sim/fall", "sim/fall.gd", "extends Node\n");
    let jump = approved(&r, "sim/jump", "good");
    let fall = approved(&r, "sim/fall", "good");
    let second = Arc::new(Mutex::new(Vec::new()));
    let (first, then) = std::thread::scope(|scope| {
        let first = scope.spawn(|| landed(&r, "sim/jump", &jump.orch));
        wait_for(|| started.exists());
        let other = r.board.other();
        let env = r.env(&r.root);
        let shared = Shared(Arc::clone(&second));
        let then = scope.spawn(move || {
            let mut out = shared;
            land(&other, &env, "sim/fall", &fall.orch, None, &mut out).map_err(refusal)
        });
        wait_for(|| {
            String::from_utf8_lossy(&second.lock().unwrap())
                .contains("land sim/fall: waiting for the land of sim/jump (pid ")
        });
        // The second waits on the first's lock, its check not started.
        assert_eq!(runs(&r), ["sim/jump"]);
        fs::write(&go, "").unwrap();
        (first.join().unwrap(), then.join().unwrap())
    });
    first.1.unwrap();
    then.unwrap();
    assert_eq!(runs(&r), ["sim/jump", "sim/fall"]);
    // The second merged onto the first's landing.
    assert_eq!(
        git(
            &r.root,
            &["log", "--first-parent", "--format=%s", "-2", "main"]
        ),
        "Merge branch 'sim/fall'\nMerge branch 'sim/jump'"
    );
}

/// Wait for `ready`, up to the backstop of the minute it once had; past it, fail where it was
/// called.
#[track_caller]
fn wait_for(ready: impl Fn() -> bool) {
    wait_saying(ready, String::new);
}

/// [`wait_for`], failing with what `found` says was there instead.
#[track_caller]
fn wait_saying(ready: impl Fn() -> bool, found: impl Fn() -> String) {
    let what = format!("the condition at {}", std::panic::Location::caller());
    let waited = test_time::until_true(what, test_time::backstop(Duration::from_secs(60)), ready);
    waited.unwrap_or_else(|stall| {
        let found = found();
        let sep = if found.is_empty() { "" } else { ": " };
        panic!("{stall}{sep}{found}");
    });
}

/// A writer another thread reads while it is written.
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// -- the project's generators ----------------------------------------------------------------------

/// The project file with a check that counts and the generators `generators` (TOML inline tables).
fn generating(r: &Repo, generators: &[&str]) {
    manifest(
        r,
        &format!(
            "\n[land]\ngenerators = [\n  {}\n]\n",
            generators.join(",\n  ")
        ),
    );
}

#[test]
fn a_land_runs_no_generator_and_lands_its_merge_alone() {
    let r = Repo::new();
    let marker = r.dir.join("generated");
    let index = format!(
        "{{ name = \"the index\", outputs = \"gen/\", command = [\"sh\", \"-c\", {}] }}",
        toml_string(&format!(
            "touch '{}'; mkdir -p gen && ls sim > gen/index",
            marker.display()
        ))
    );
    generating(&r, &[&index]);
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
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
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main"]),
        format!("Merge branch '{SLUG}'")
    );
    assert!(!marker.exists(), "no generator ran: {said}");
    assert!(!r.root.join("gen").exists());
    assert_eq!(runs(&r).len(), 1, "the check judged the merge");
    // A test's board starts no regenerator.
    assert!(!said.contains("regenerat"), "{said}");
}

#[test]
fn a_project_that_turns_the_lands_check_off_lands_its_merge_unchecked() {
    let r = Repo::new();
    manifest(&r, "\n[land]\ncheck = false\n");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let merge = head(&r.root, "main");
    assert_eq!(
        result.unwrap(),
        format!(
            "landed {SLUG} on main at {}; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert!(runs(&r).is_empty(), "no check ran");
    assert!(!said.contains("checking"), "{said}");
    // The thread names the merge it moves main to, and no check, since none ran.
    let thread = r.board.said(SLUG);
    assert!(
        thread.contains(&format!(
            "land {SLUG}: merged {} as {merge}; moving main there",
            head(&r.root, "main^2")
        )),
        "{thread:?}"
    );
    assert!(!thread.iter().any(|l| l.contains("checked")), "{thread:?}");
}

#[test]
fn finishing_a_land_that_ran_no_check_says_none_was_checked() {
    let r = Repo::new();
    manifest(&r, "\n[land]\ncheck = false\n");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    // A land stopped after it moved main: its merge there, and its record of it.
    let message = format!("Merge branch '{SLUG}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, SLUG]);
    let merge = head(&r.root, "main");
    let folder = r.root.join(".git/symphony/tasks/land");
    fs::create_dir_all(&folder).unwrap();
    Record {
        slug: SLUG.into(),
        tip: tip.clone(),
        merge: merge.clone(),
        checked: merge.clone(),
        regenerated: Vec::new(),
    }
    .write(&folder)
    .unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch);
    assert_eq!(
        result.unwrap(),
        format!(
            "landed {SLUG} on main at {}, finishing a land interrupted after its merge; not \
             pushed: no origin remote",
            &merge[..12]
        )
    );
    assert!(
        said.contains(&format!(
            "land {SLUG}: its tip {} is on main already, by this land's merge {}: finishing the \
             land (the push, the landed line, the sweep)",
            &tip[..7],
            &merge[..7]
        )),
        "{said}"
    );
    assert!(!said.contains("checked"), "{said}");
}

// -- the push ------------------------------------------------------------------------------------

/// A bare repository beside the checkout, its `origin`, holding main as it is now.
fn remote(r: &Repo) -> PathBuf {
    let bare = r.dir.join("origin.git");
    git(
        &r.dir,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &bare.to_string_lossy(),
        ],
    );
    git(
        &r.root,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    bare
}

/// A clone of `bare` elsewhere that commits `file` as `text` and pushes main: what origin gained.
fn pushed_elsewhere(r: &Repo, bare: &Path, file: &str, text: &str) -> String {
    let other = r.dir.join("elsewhere");
    if !other.exists() {
        git(
            &r.dir,
            &[
                "clone",
                "-q",
                &bare.to_string_lossy(),
                &other.to_string_lossy(),
            ],
        );
        for (key, value) in [
            ("user.email", "o@example.com"),
            ("user.name", "o"),
            ("commit.gpgsign", "false"),
        ] {
            git(&other, &["config", key, value]);
        }
    }
    write(&other, file, text);
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-qm", "pushed elsewhere"]);
    git(&other, &["push", "-q", "origin", "main"]);
    head(&other, "HEAD")
}

#[test]
fn a_landing_line_names_the_tokens_its_task_took_by_role_and_round_and_their_cost() {
    use symphony_board::Beat;
    use symphony_board::usage::{Prices, Spent, Tokens, price};
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    git(&r.root, &["tag", "v1"]);
    let store = &r.board.store;
    let (orch, exe, rev) = (
        r.board.session("orchestrator"),
        r.board.session("worker"),
        r.board.session("reviewer"),
    );
    let worth = |input, read, output, calls| Tokens {
        input,
        cache_write_5m: 1_000,
        cache_write_1h: 0,
        cache_read: read,
        output,
        calls,
    };
    let spend = |key: &str, input, read, output, calls| {
        let spent = Spent {
            model: "claude-opus-5-5".into(),
            speed: "standard".into(),
            tokens: worth(input, read, output, calls),
        };
        store
            .agent_beat_spending(key, &Beat::default(), None, None, &[spent])
            .unwrap()
            .expect("the key is bound");
    };
    r.task(SLUG);
    store.take_role(SLUG, &orch, "orchestrator").unwrap();
    store.claim(SLUG, &exe).unwrap();
    assert!(store.bind_agent("exe-key", &exe, false).unwrap());
    spend("exe-key", 10, 50_000, 500, 4);
    store.move_to(SLUG, "review", &exe).unwrap();
    store.take_role(SLUG, &rev, "reviewer").unwrap();
    assert!(store.bind_agent("rev-key", &rev, false).unwrap());
    spend("rev-key", 5, 19_000, 100, 2);
    store.verdict(SLUG, &rev, "approve", "good").unwrap();
    let line = landed(&r, SLUG, &orch).1.unwrap();
    let merge = head(&r.root, "main");
    // Their cost, at the prices every build embeds.
    let prices = Prices::embedded();
    let mut cost = prices.cost("claude-opus-5-5", "standard", &worth(10, 50_000, 500, 4));
    cost.add(&prices.cost("claude-opus-5-5", "standard", &worth(5, 19_000, 100, 2)));
    assert!(cost.total() > 0 && cost.unpriced.is_empty());
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; 71.6k tokens in 6 calls (executor 51.5k, reviewer \
             20.1k; round 1 51.5k, round 2 20.1k), {} estimated at API list prices read {}; \
             pushed to origin",
            &merge[..12],
            price::dollars(cost.total()),
            prices.read
        )
    );
    // The thread's landed line says the same, before how the approval was judged.
    let said = r.board.said(SLUG);
    let tokens = line.trim_end_matches("; pushed to origin");
    assert!(
        said.last().is_some_and(|last| last.starts_with(tokens)),
        "{said:?}"
    );
    // The push is main's alone: the remote holds it at the merge, and nothing else.
    assert_eq!(head(&bare, "main"), merge);
    assert_eq!(
        git(&bare, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main",
        "no other branch and no tag"
    );
    assert_eq!(head(&r.root, "origin/main"), merge);
}

#[test]
fn what_origin_gained_is_merged_and_checked_again_before_the_push() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let gained = pushed_elsewhere(&r, &bare, "sim/other.gd", "extends Node\n");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    let top = head(&r.root, "main");
    assert_eq!(head(&bare, "main"), top, "{said}");
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main"]),
        "Merge remote-tracking branch 'origin/main'"
    );
    assert_eq!(head(&r.root, "main^2"), gained);
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main^1"]),
        format!("Merge branch '{SLUG}'")
    );
    // The landing's line names its own merge, and the check ran on both.
    assert!(line.contains(&head(&r.root, "main^1")[..12]), "{line}");
    assert_eq!(runs(&r).len(), 2);
    assert!(
        said.contains(&format!(
            "land {SLUG}: origin/main gained {}; merging and checking it",
            &gained[..7]
        )),
        "{said}"
    );
    assert!(r.root.join("sim/other.gd").is_file());
}

#[test]
fn a_push_that_fails_leaves_the_landing_here_says_so_and_closes_the_task() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    let hook = bare.join("hooks/pre-receive");
    write(
        &bare,
        "hooks/pre-receive",
        "#!/bin/sh\necho 'pushes are closed' >&2\nexit 1\n",
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let before = head(&bare, "main");
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    // Naming the tip, so the thread's line is the land's whole.
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    let merge = head(&r.root, "main");
    assert!(
        line.starts_with(&format!(
            "landed {SLUG} on main at {}; NOT PUSHED, landed here only: git push origin: ",
            &merge[..12]
        )),
        "{line}"
    );
    assert!(
        line.ends_with(". Push it with: git push origin main"),
        "{line}"
    );
    assert!(line.contains("pushes are closed"), "{line}");
    assert_eq!(head(&bare, "main"), before);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert_eq!(last(&r, SLUG), line);
}

#[test]
fn origin_that_moves_at_every_push_leaves_the_landing_here_after_its_tries() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    // Origin's main moves past what each push expects it at, out of the push's quarantine.
    let hook = bare.join("hooks/pre-receive");
    write(
        &bare,
        "hooks/pre-receive",
        "#!/bin/sh\nunset GIT_QUARANTINE_PATH GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES\n\
         export GIT_AUTHOR_NAME=o GIT_AUTHOR_EMAIL=o@example.com GIT_COMMITTER_NAME=o \
         GIT_COMMITTER_EMAIL=o@example.com\n\
         top=$(git rev-parse refs/heads/main)\n\
         next=$(git commit-tree --no-gpg-sign -p \"$top\" -m 'moved while pushing' \"$top^{tree}\")\n\
         git update-ref refs/heads/main \"$next\" \"$top\"\n",
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let wt = branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip} is good"));
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    // The landing's merge, under the two merges of what origin gained.
    let merge = head(&r.root, "main~2");
    assert_eq!(head(&r.root, "main~2^2"), tip, "{said}");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; NOT PUSHED, landed here only: origin/main kept moving \
             while the land pushed. Push it with: git fetch origin && git merge origin/main (in \
             the main checkout), then git push origin main",
            &merge[..12]
        )
    );
    assert_eq!(
        said.matches("moved while pushing; fetching again").count(),
        2,
        "{said}"
    );
    assert_eq!(
        said.matches("; merging and checking it").count(),
        2,
        "{said}"
    );
    assert_eq!(
        runs(&r).len(),
        3,
        "the landing's check, and one for each merge of origin"
    );
    assert_ne!(head(&bare, "main"), head(&r.root, "main"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert_eq!(last(&r, SLUG), line);
}

#[test]
fn origin_conflicting_with_the_landing_leaves_it_here_and_tells_the_orchestrator() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = remote(&r);
    branch(&r, SLUG, "sim/world.gd", "extends Node\n");
    let tip = head(&r.root, SLUG);
    let people = approved(&r, SLUG, "good");
    let before = head(&bare, "main");
    pushed_elsewhere(&r, &bare, "sim/world.gd", "extends Object\n");
    let gained = head(&bare, "main");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    let merge = head(&r.root, "main");
    assert_ne!(gained, before);
    assert_eq!(head(&bare, "main"), gained, "origin keeps what it gained");
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; NOT PUSHED, landed here only: origin/main conflicts \
             with main at {}: sim/world.gd. Push it with: git fetch origin && git merge \
             origin/main (in the main checkout), then git push origin main",
            &merge[..12],
            &merge[..7]
        )
    );
    let thread = r.board.store.thread(SLUG).unwrap();
    let told = thread
        .iter()
        .find(|m| m.text.starts_with(&format!("land of {SLUG} not pushed: ")))
        .unwrap();
    assert_eq!(told.recipient.as_deref(), Some("@orchestrator"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
}

#[test]
fn an_origin_with_no_main_yet_gets_it_from_the_first_land() {
    let r = Repo::new();
    manifest(&r, "");
    let bare = r.dir.join("origin.git");
    git(
        &r.dir,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &bare.to_string_lossy(),
        ],
    );
    git(
        &r.root,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert_eq!(head(&bare, "main"), head(&r.root, "main"));
}

#[test]
fn a_project_that_says_not_to_push_lands_without_it() {
    let r = Repo::new();
    manifest(&r, "\n[land]\npush = false\n");
    let bare = remote(&r);
    let before = head(&bare, "main");
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(line.ends_with("; not pushed: [land] push is off"), "{line}");
    assert_eq!(head(&bare, "main"), before);
}

#[test]
fn a_remote_that_cannot_be_reached_is_a_push_to_retry() {
    let r = Repo::new();
    manifest(&r, "");
    git(
        &r.root,
        &[
            "remote",
            "add",
            "origin",
            &r.dir.join("gone.git").to_string_lossy(),
        ],
    );
    branch(&r, SLUG, "sim/jump.gd", "extends Node\n");
    let people = approved(&r, SLUG, "good");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(
        line.contains("; NOT PUSHED, landed here only: git fetch origin: "),
        "{line}"
    );
    assert!(
        line.ends_with(". Push it with: git push origin main"),
        "{line}"
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

#[path = "land_answers.rs"]
mod answers;
#[path = "land_approval.rs"]
mod approval;
#[path = "land_carried.rs"]
mod carried;
#[path = "land_covered.rs"]
mod covered;
#[path = "land_gates.rs"]
mod gates;
#[path = "land_generated.rs"]
mod generated;
#[path = "land_hooks.rs"]
mod hooks;
#[path = "land_limits.rs"]
mod limits;
#[path = "land_regenerate.rs"]
mod regenerate;
#[path = "land_signals.rs"]
mod signals;
#[path = "land_stamps.rs"]
mod stamps;
#[path = "land_workflow.rs"]
mod workflow;
