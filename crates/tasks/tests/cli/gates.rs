//! `tasks gate` in a project's checkout: a raise reads its limit's number from the main checkout's
//! default branch, in a file `[land] check_paths` names whole; a waiver's tip is resolved there,
//! the branch's tip when none is named; the decision is its orchestrator's alone; and `tasks
//! stamps SLUG` shows each decision that stands.

use crate::recorder::git_says;
use crate::slots::in_project;
use crate::{Case, git};
use std::fs;
use std::path::Path;

const GATE: &str = "tools/size-budget.toml:symphony";
const WHY: &str = "11,920,000 measured after the cut";

/// The project with a size budget and a limit outside any check committed on main.
fn project(name: &str) -> (Case, std::path::PathBuf) {
    let (c, root) = in_project(name, "[project]\nname = \"p\"\n");
    for (file, text) in [
        (
            "tools/size-budget.toml",
            "[binaries]\nsymphony = 12_390_000\n",
        ),
        ("docs/limits.toml", "pages = 9\n"),
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        git(&root, &["add", file]);
    }
    git(&root, &["commit", "-q", "-m", "the limits"]);
    (c, root)
}

/// `tools/size` filed and orchestrated by an orchestrator, claimed by a worker: the two handles.
fn worked(c: &Case) -> (String, String) {
    let (orch, exe) = (c.register("orchestrator"), c.register("worker"));
    c.ok(&orch, &["create", "tools/size", "A heavier binary"]);
    c.ok(&exe, &["claim", "tools/size"]);
    (orch, exe)
}

/// `gate tools/size` and `args`.
fn gate<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["gate", "tools/size"];
    all.extend_from_slice(args);
    all
}

/// A commit on the branch `tools/size` of `root`, the main checkout put back on main: its tip.
fn branch(root: &Path) -> String {
    git(root, &["checkout", "-q", "-b", "tools/size"]);
    fs::write(
        root.join("tools/size-budget.toml"),
        "[binaries]\nsymphony = 12_400_000\n",
    )
    .unwrap();
    git(
        root,
        &["commit", "-q", "-am", "the raise: 11,920,000 measured"],
    );
    let tip = git_says(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "-q", "main"]);
    tip
}

#[test]
fn the_orchestrator_raises_a_limit_from_the_default_branchs_number_and_stamps_shows_it() {
    let (c, root) = project("gate-raise");
    let (orch, _) = worked(&c);
    assert_eq!(
        c.ok(&orch, &gate(&[GATE, "--raise", "12,400,000", "--why", WHY])),
        "tools/size: tools/size-budget.toml:symphony raised 12,390,000 -> 12,400,000; commit the \
         file's change on the task's branch, its message the account\n"
    );
    let decisions = c.store().get("tools/size").unwrap().gates();
    assert_eq!(decisions.len(), 1);
    assert_eq!(
        (decisions[0].from, decisions[0].to, decisions[0].by.as_str()),
        (Some(12_390_000), Some(12_400_000), orch.as_str())
    );
    let line = format!("gate {GATE} raised 12,390,000 -> 12,400,000: {WHY}; by {orch}");
    assert!(c.ok("", &["show", "tools/size"]).contains(&line));
    let stamps = c.ok("", &["stamps", "tools/size"]);
    assert!(
        stamps.contains(&format!(
            "\nGATES its decisions that stand, the newest on each gate:\n  {line}\nTIP none"
        )),
        "{stamps}"
    );
    // Its number is the default branch's, whatever the task's branch holds.
    branch(&root);
    c.ok(&orch, &gate(&[GATE, "--raise", "12_410_000", "--why", WHY]));
    let decisions = c.store().get("tools/size").unwrap().gates();
    assert_eq!(
        (decisions[0].from, decisions[0].to),
        (Some(12_390_000), Some(12_410_000))
    );
}

#[test]
fn a_raise_names_a_number_in_a_check_set_once_and_above_the_default_branchs() {
    let (c, root) = project("gate-raise-refused");
    let (orch, _) = worked(&c);
    let refused = |args: &[&str]| {
        let mut all = vec!["gate", "tools/size"];
        all.extend_from_slice(args);
        let ran = c.run_as(&orch, &all, "");
        assert_eq!(ran.code, 1, "{all:?}: {}", ran.out);
        ran.err
    };
    assert_eq!(
        refused(&["docs/limits.toml:pages", "--raise", "10", "--why", WHY]),
        "✖ docs/limits.toml is no check: a raised limit is a number in a file [land] check_paths \
         names whole (a test, tools/, a budget), so the raise keeps a fresh reviewer\n"
    );
    assert_eq!(
        refused(&["tools/size-budget.toml:nope", "--raise", "10", "--why", WHY]),
        "✖ main: no line of tools/size-budget.toml sets nope\n"
    );
    assert_eq!(
        refused(&["tools/absent.toml:x", "--raise", "10", "--why", WHY]),
        format!(
            "✖ main: {} holds no file tools/absent.toml\n",
            &git_says(&root, &["rev-parse", "main"])[..7]
        )
    );
    assert_eq!(
        refused(&[GATE, "--raise", "a lot", "--why", WHY]),
        "✖ --raise names the number the limit goes to, a whole number: not \"a lot\"\n"
    );
    assert_eq!(
        refused(&[GATE, "--raise", "12_000_000", "--why", WHY]),
        format!(
            "✖ a raise of {GATE} goes above 12,390,000, its number now: 12,000,000 is not, and a \
             limit lowered needs no decision\n"
        )
    );
    assert_eq!(
        refused(&[GATE, "--raise", "12_400_000", "--tip", "main", "--why", WHY]),
        "✖ a raise holds for every tip after it lands: --tip is a waiver's\n"
    );
    assert_eq!(
        refused(&[GATE, "--why", WHY]),
        "✖ name what the decision does: --raise NEW for a limit (PATH:NAME), --waive for a stamp \
         at one tip\n"
    );
    assert!(c.store().get("tools/size").unwrap().gates().is_empty());
}

#[test]
fn a_waiver_is_for_the_branchs_tip_or_the_one_named_and_never_of_tests_review_or_owner() {
    let (c, root) = project("gate-waive");
    let (orch, exe) = worked(&c);
    let ran = |who: &str, args: &[&str]| {
        let mut all = vec!["gate", "tools/size"];
        all.extend_from_slice(args);
        c.run_as(who, &all, "")
    };
    let no_branch = ran(&orch, &["pictures", "--waive", "--why", WHY]);
    assert_eq!(
        (no_branch.code, no_branch.err),
        (
            1,
            format!(
                "✖ no branch tools/size in {}: a waiver is for one tip, which --tip names\n",
                root.display()
            )
        )
    );
    let tip = branch(&root);
    let waived = ran(&orch, &["pictures", "--waive", "--why", WHY]);
    assert_eq!(
        waived.out,
        format!("tools/size: pictures waived at {} alone\n", &tip[..7])
    );
    let main = git_says(&root, &["rev-parse", "main"]);
    ran(
        &orch,
        &["pictures", "--waive", "--tip", &main[..9], "--why", WHY],
    );
    let decisions = c.store().get("tools/size").unwrap().gates();
    assert_eq!(
        decisions[0].tip.as_deref(),
        Some(main.as_str()),
        "resolved whole"
    );
    let nowhere = ran(
        &orch,
        &["pictures", "--waive", "--tip", "no-such-tip", "--why", WHY],
    );
    assert_eq!(
        (nowhere.code, nowhere.err),
        (
            1,
            format!(
                "✖ --tip no-such-tip names no commit in {}\n",
                root.display()
            )
        )
    );
    for never in ["tests", "review", "owner"] {
        let refused = ran(&orch, &[never, "--waive", "--why", WHY]);
        assert_eq!(refused.code, 1);
        assert!(
            refused
                .err
                .starts_with(&format!("✖ {never} is never waived: ")),
            "{}",
            refused.err
        );
    }
    let typo = ran(
        &orch,
        &["picture", "--waive", "--tip", "main", "--why", WHY],
    );
    assert!(
        typo.err
            .starts_with("✖ the catalogue has no stamp \"picture\""),
        "{}",
        typo.err
    );
    // Its executor decides no gate of its own.
    let refused = ran(&exe, &["pictures", "--waive", "--why", WHY]);
    assert_eq!(
        (refused.code, refused.err),
        (
            1,
            format!(
                "✖ {exe} executed tools/size: a gate decision is its orchestrator's, never its \
                 executor's or its reviewer's, so no session decides its own gate\n"
            )
        )
    );
    // Outside a repository there is no limit or tip to read.
    let mut outside = Case::new();
    outside.env.vars = c.env.vars.clone();
    let ran = outside.run_as(
        &orch,
        &["gate", "tools/size", GATE, "--raise", "1", "--why", WHY],
        "",
    );
    assert_eq!(
        (ran.code, ran.err.as_str()),
        (
            1,
            "✖ tasks gate runs in a checkout of the task's repository: a raise reads its limit, \
             and a waiver its stamp and tip, from the main checkout\n"
        )
    );
}
