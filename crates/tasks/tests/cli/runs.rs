//! `tasks runs` in a project's checkout on a board of its own, the recorder's runs in a work folder
//! of the test's own (`TASKS_WORK`): each command the task ran at its branch's tip with its newest
//! result, a red then green of one command green with the red said, a run at an earlier tip not
//! shown as this tip's, the project's `[verdicts] check` with no run said; `--tip` reads at another
//! commit; and its refusals.

use crate::recorder::git_says;
use crate::slots::in_project;
use crate::{Case, git, scratch};
use std::fs;
use std::path::Path;
use symphony_board::cli::machine_work::recorded::{self, FOLDER_VAR, Phase, Record, Step};
use symphony_board::sys::local_minute;

const SLUG: &str = "tools/query";
/// A project whose `[verdicts] check` the regenerator runs on main, as this repository's does.
const MANIFEST: &str = "[project]\nname = \"p\"\n\n[verdicts]\ncheck = [\"just\", \"docs-check\"]\n\n\
                        [land]\ncheck = false\n";

/// A commit on the branch `SLUG` of `root` writing `file`: its id. The main checkout stays on main.
fn commit(root: &Path, file: &str) -> String {
    if git_says(root, &["branch", "--list", SLUG]).is_empty() {
        git(root, &["checkout", "-q", "-b", SLUG]);
    } else {
        git(root, &["checkout", "-q", SLUG]);
    }
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "fn changed() {}\n").unwrap();
    git(root, &["add", file]);
    git(root, &["commit", "-q", "-m", file]);
    let tip = git_says(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "main"]);
    tip
}

/// A run of `label` for `task` at `commit`, by `handle`, kept in `work` as the recorder keeps one
/// that finished with `exit`, begun at `n` thousand seconds and taking `n` seconds.
fn ran(work: &Path, n: u32, task: Option<&str>, label: &str, commit: &str, exit: i32) {
    let started = 1000.0 * f64::from(n);
    let record = Record {
        version: 1,
        operation_id: format!("{n:032x}"),
        parent_id: None,
        phase: if label.contains("test") {
            Phase::Test
        } else {
            Phase::Build
        },
        label: label.into(),
        state: Step::Finished,
        handle: Some("amber-heron".into()),
        task: task.map(str::to_string),
        checkout: "/checkout".into(),
        branch: Some(SLUG.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: started,
        started_at: Some(started),
        ended_at: Some(started + f64::from(n)),
        beat_at: started,
        exit_code: Some(exit),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    };
    recorded::keep(work, &record).unwrap();
}

/// The project, `SLUG` filed and claimed, and the case reading the runs from a work folder of its
/// own: the case, the main checkout and the work folder.
fn project(name: &str) -> (Case, std::path::PathBuf, std::path::PathBuf) {
    let (mut c, root) = in_project(name, MANIFEST);
    let work = scratch(&format!("{name}-work"));
    c.env
        .vars
        .insert(FOLDER_VAR.into(), work.to_string_lossy().into_owned());
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(&orch, &["create", SLUG, "A store query"]);
    c.ok(&exe, &["claim", SLUG]);
    (c, root, work)
}

#[test]
fn runs_says_each_command_at_the_tip_and_the_check_with_no_run_there() {
    let (c, root, work) = project("runs-tip");
    let earlier = commit(&root, "src/query.rs");
    let tip = commit(&root, "tests/query.rs");
    let task = Some(SLUG);
    // At the earlier tip: green, and not this tip's.
    ran(&work, 1, task, "just test-rs -p q", &earlier, 0);
    // At the tip: a green, and a red then green of one command.
    ran(&work, 2, task, "just lint-rs", &tip, 0);
    ran(&work, 3, task, "just test-rs -p q", &tip, 101);
    ran(&work, 4, task, "just test-rs -p q", &tip, 0);
    // Another task's at the tip is not this one's.
    ran(
        &work,
        5,
        Some("tools/other"),
        "just test-rs -p other",
        &tip,
        1,
    );
    let short = |commit: &str| commit[..7].to_string();
    let at = |n: f64| local_minute(1000.0 * n + n);
    assert_eq!(
        c.ok("", &["runs", SLUG]),
        [
            format!("TIP {SLUG} {tip}"),
            format!("RUNS at {}, each command's newest run there:", short(&tip)),
            format!(
                "  green  build  just lint-rs       {}  2 s  amber-heron",
                at(2.0)
            ),
            format!(
                "  green  test   just test-rs -p q  {}  4 s  amber-heron  1 earlier here, 1 red",
                at(4.0)
            ),
            format!(
                "TESTS met: Green at {}: `just lint-rs`, `just test-rs -p q`",
                short(&tip)
            ),
            format!(
                "CHECK `just docs-check`, the project's [verdicts] check: no run at {}",
                short(&tip)
            ),
            format!(
                "ELSEWHERE 1 run of the task at other commits, the newest at {}, {}: none is this \
                 tip's",
                short(&earlier),
                at(1.0)
            ),
            String::new(),
        ]
        .join("\n")
    );
    // The check run at the tip, by no task's session, is the check's as its gate reads it.
    ran(&work, 6, None, "just docs-check", &tip, 0);
    let out = c.ok("", &["runs", SLUG]);
    assert!(
        out.contains(&format!(
            "\nCHECK `just docs-check`, the project's [verdicts] check: green at {}, {}, by \
             amber-heron\n",
            short(&tip),
            at(6.0)
        )),
        "{out}"
    );
    // Read at the earlier commit, by --tip as a prefix: its run is the tip's, the others elsewhere.
    let out = c.ok("", &["runs", SLUG, "--tip", &earlier[..9]]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], format!("TIP {earlier} (--tip)"));
    assert_eq!(
        lines[2],
        format!(
            "  green  test  just test-rs -p q  {}  1 s  amber-heron",
            at(1.0)
        )
    );
    assert_eq!(
        lines[4],
        format!(
            "CHECK `just docs-check`, the project's [verdicts] check: no run at {}",
            short(&earlier)
        )
    );
    assert!(
        lines[5].starts_with("ELSEWHERE 3 runs of the task at other commits, the newest at "),
        "{out}"
    );
}

#[test]
fn runs_with_nothing_recorded_says_so_and_names_no_check_the_project_lacks() {
    let (mut c, root) = in_project("runs-none", "[project]\nname = \"p\"\n");
    let work = scratch("runs-none-work");
    c.env
        .vars
        .insert(FOLDER_VAR.into(), work.to_string_lossy().into_owned());
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", SLUG, "A store query"]);
    let tip = commit(&root, "src/query.rs");
    assert_eq!(
        c.ok("", &["runs", SLUG]),
        format!(
            "TIP {SLUG} {tip}\nRUNS none at {at}: no build or test of the task is recorded \
             there\nTESTS not met: No build or test recorded for it at {at}\n",
            at = &tip[..7]
        )
    );
}

#[test]
fn runs_is_refused_with_no_task_no_tip_to_read_or_no_repository() {
    let (c, root, _) = project("runs-refused");
    let refused = |c: &Case, args: &[&str]| -> String {
        let ran = c.run_as("", args, "");
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.out);
        ran.err
    };
    assert_eq!(
        refused(&c, &["runs", SLUG]),
        format!(
            "✖ no branch {SLUG} in {}: name the commit to read with --tip\n",
            root.display()
        )
    );
    assert_eq!(
        refused(&c, &["runs", SLUG, "--tip", "nosuchcommit"]),
        format!(
            "✖ --tip nosuchcommit names no commit in {}\n",
            root.display()
        )
    );
    assert!(
        refused(&c, &["runs", "tools/missing"]).contains("tools/missing"),
        "a task the board does not hold"
    );
    let mut outside = Case::new();
    outside.env.vars = c.env.vars.clone();
    assert_eq!(
        refused(&outside, &["runs", SLUG]),
        "✖ tasks runs runs in a checkout of the task's repository: the tip it reads runs at is a \
         commit there\n"
    );
}
