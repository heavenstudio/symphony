//! `tasks stamps` and `tasks needs` in a project's checkout: the catalogue read from the main
//! checkout's committed `symphony.toml` (an edit not committed is not read), a task's needs as its
//! assessor writes them, its tip resolved there, and what an assessor reads first: the needs as they
//! stand, the branch's tip and the paths `[land] check_paths` names that the branch changes.

use crate::recorder::git_says;
use crate::slots::in_project;
use crate::{Case, git};
use std::fs;
use std::path::Path;

const WHY: &str = "a store query and its test, no page or wire";

/// A commit on the branch `branch` of `root` writing each of `files`, the main checkout put back on
/// main: the branch's tip.
fn branch(root: &Path, branch: &str, files: &[&str]) -> String {
    git(root, &["checkout", "-q", "-b", branch]);
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "fn changed() {}\n").unwrap();
        git(root, &["add", file]);
    }
    git(root, &["commit", "-q", "-m", "the work"]);
    let tip = git_says(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "main"]);
    tip
}

/// `tools/query` filed and orchestrated by an orchestrator, claimed by a worker: the two handles.
fn worked(c: &Case) -> (String, String) {
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(&orch, &["create", "tools/query", "A store query"]);
    c.ok(&exe, &["claim", "tools/query"]);
    (orch, exe)
}

#[test]
fn stamps_prints_the_symphonys_own_catalogue_unless_main_commits_its_own() {
    let (c, root) = in_project("stamps-own", "[project]\nname = \"p\"\n");
    let out = c.ok("", &["stamps"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "STAMPS the Symphony's own: no [stamps] in symphony.toml at HEAD of {}",
            root.display()
        )
    );
    let named: Vec<&str> = (lines.iter())
        .filter(|line| !line.starts_with("  ") && !line.starts_with("STAMPS"))
        .copied()
        .collect();
    assert_eq!(
        named,
        ["tests", "review (needed by default)", "pictures", "owner"]
    );
    assert_eq!(out.matches("\n  met: ").count(), 4, "{out}");
    assert_eq!(out.matches("\n  applies: ").count(), 4, "{out}");
    assert_eq!(out.matches("\n  skips: ").count(), 4, "{out}");
    // Written in the main checkout but not committed: not what a land is held to, so not read.
    fs::write(
        root.join("symphony.toml"),
        "[stamps.tests]\ndefault = true\napplies = \"every change\"\n",
    )
    .unwrap();
    assert_eq!(c.ok("", &["stamps"]), out);
    git(&root, &["commit", "-q", "-am", "the project's own stamps"]);
    assert_eq!(
        c.ok("", &["stamps"]),
        format!(
            "STAMPS from [stamps] in symphony.toml at HEAD of {}\ntests (needed by default)\n  \
             met: {}\n  applies: every change\n  skips: {}\n",
            root.display(),
            symphony_board::stamps::Catalogue::symphony().stamps[0].met,
            symphony_board::stamps::Catalogue::symphony().stamps[0].skips,
        )
    );
    // Outside a repository: the Symphony's own.
    assert!(
        Case::new()
            .ok("", &["stamps"])
            .starts_with("STAMPS the Symphony's own: no repository here\ntests\n")
    );
}

#[test]
fn a_committed_stamp_with_no_reader_refuses_the_catalogue_where_it_is_read() {
    let (c, _) = in_project("stamps-unknown", "[stamps.review]\n[stamps.lint]\n");
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "tools/query", "A store query"]);
    let said = "✖ symphony.toml: [stamps] names a stamp the Symphony has no reader for, \"lint\" \
                (line 2): it has no way to stand on a tip; a stamp is one of tests, review, \
                pictures, owner\n";
    for args in [
        &["stamps"][..],
        &["needs", "tools/query", "review", "--why", WHY],
    ] {
        let ran = c.run_as(&c.register("worker"), args, "");
        assert_eq!((ran.code, ran.err.as_str()), (1, said), "{args:?}");
    }
}

#[test]
fn stamps_of_a_task_says_its_needs_its_tip_and_the_check_paths_its_branch_changes() {
    let (c, root) = in_project("stamps-task", "[project]\nname = \"p\"\n");
    worked(&c);
    let task = |c: &Case| -> Vec<String> {
        let out = c.ok("", &["stamps", "tools/query"]);
        let at = out
            .find("\nNEEDS ")
            .expect("the task's lines follow the catalogue");
        out[at + 1..].lines().map(str::to_string).collect()
    };
    assert_eq!(
        task(&c),
        [
            "NEEDS review: no assessment yet, so the catalogue's defaults",
            "TIP none: no branch tools/query, so an assessment reads the brief alone (no --tip)",
        ]
    );
    let tip = branch(
        &root,
        "tools/query",
        &["src/query.rs", "tests/query.rs", "justfile"],
    );
    assert_eq!(
        task(&c),
        [
            "NEEDS review: no assessment yet, so the catalogue's defaults".to_string(),
            format!("TIP tools/query {tip}"),
            format!(
                "CHECK PATHS {} changes what [land] check_paths names:",
                &tip[..7]
            ),
            "  justfile".into(),
            "  tests/query.rs".into(),
        ]
    );
    // A branch that changes none of them says so.
    let (orch, other) = (c.register("orchestrator"), "tools/docs");
    c.ok(&orch, &["create", other, "A design"]);
    let docs = branch(&root, other, &["docs/design.md"]);
    let out = c.ok("", &["stamps", other]);
    assert!(
        out.ends_with(&format!(
            "TIP {other} {docs}\nCHECK PATHS none: {} changes nothing [land] check_paths names\n",
            &docs[..7]
        )),
        "{out}"
    );
    // Assessed: the needs as they stand, who and why.
    let assessor = c.register("worker");
    c.ok(
        &assessor,
        &[
            "needs",
            "tools/query",
            "tests",
            "review",
            "--tip",
            &tip[..7],
            "--why",
            WHY,
        ],
    );
    assert_eq!(
        task(&c)[0],
        format!("NEEDS tests, review at {} by {assessor}: {WHY}", &tip[..7])
    );
}

#[test]
fn needs_writes_an_assessment_its_tip_resolved_and_the_thread_says_it() {
    let (c, root) = in_project("needs-written", "[project]\nname = \"p\"\n");
    worked(&c);
    let tip = branch(&root, "tools/query", &["src/query.rs", "tests/query.rs"]);
    let assessor = c.register("worker");
    assert_eq!(
        c.ok(
            &assessor,
            &[
                "needs",
                "tools/query",
                "review",
                "tests",
                "--tip",
                &tip[..9],
                "--why",
                WHY
            ],
        ),
        format!("tools/query needs tests, review at {}\n", &tip[..7])
    );
    let task = c.store().get("tools/query").unwrap();
    let needs = task.needs().unwrap();
    assert_eq!(needs.tip.as_deref(), Some(tip.as_str()), "the whole commit");
    assert_eq!(
        (needs.by.as_str(), needs.why.as_str()),
        (assessor.as_str(), WHY)
    );
    let shown = c.ok("", &["show", "tools/query"]);
    assert!(
        shown.contains(&format!(
            "needs tests, review at {} (was review): {WHY}; by {assessor}",
            &tip[..7]
        )),
        "{shown}"
    );
    // None, with a reason, from the brief alone: no --tip.
    assert_eq!(
        c.ok(
            &assessor,
            &["needs", "tools/query", "none", "--why", "documents only"],
        ),
        "tools/query needs none from the brief\n"
    );
    assert!(c.ok("", &["show", "tools/query"]).contains(&format!(
        "needs none from the brief (was tests, review): documents only; by {assessor}"
    )));
}

#[test]
fn needs_is_refused_to_the_tasks_own_sessions_and_on_what_it_cannot_take() {
    let (c, root) = in_project("needs-refused", "[stamps.tests]\n[stamps.review]\n");
    let (orch, exe) = worked(&c);
    let assessor = c.register("worker");
    let refused = |who: &str, args: &[&str]| {
        let mut all = vec!["needs", "tools/query"];
        all.extend_from_slice(args);
        let ran = c.run_as(who, &all, "");
        assert_eq!(ran.code, 1, "{all:?}: {}", ran.out);
        ran.err
    };
    let tail = "its needs are its assessor's, a session that never executed, orchestrated or \
                filed it, which its orchestrator starts as a session of its own";
    assert_eq!(
        refused(&exe, &["none", "--why", "only a typo"]),
        format!("✖ {exe} executed tools/query: {tail}\n")
    );
    assert_eq!(
        refused(&orch, &["none", "--why", "only a typo"]),
        format!("✖ {orch} orchestrated tools/query: {tail}\n")
    );
    assert_eq!(
        refused(&assessor, &["review", "--why", " "]),
        "✖ an assessment says why (--why WHY): one line for each stamp it left out or added\n"
    );
    assert_eq!(
        refused(&assessor, &["review", "pictures", "--why", WHY]),
        "✖ the catalogue has no stamp \"pictures\": its stamps are tests, review (`just tasks \
         stamps` lists them)\n"
    );
    assert_eq!(
        refused(
            &assessor,
            &["review", "--tip", "nosuchcommit", "--why", WHY]
        ),
        format!(
            "✖ --tip nosuchcommit names no commit in {}\n",
            root.display()
        )
    );
    assert_eq!(c.store().get("tools/query").unwrap().needs(), None);
    // Outside a repository there is no catalogue a land holds the task to.
    let mut outside = Case::new();
    outside.env.vars = c.env.vars.clone();
    let ran = outside.run_as(
        &assessor,
        &["needs", "tools/query", "review", "--why", WHY],
        "",
    );
    assert_eq!(
        (ran.code, ran.err.as_str()),
        (
            1,
            "✖ tasks needs runs in a checkout of the task's repository: the catalogue it names \
             stamps from is the main checkout's committed symphony.toml\n"
        )
    );
}
