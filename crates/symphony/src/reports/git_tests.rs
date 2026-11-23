//! The default branch's hours from two `git log` reads of a scratch repository: commits by the hour
//! they reached the branch, merges and lines along the branch's first parents, a branch's own merge
//! of the default branch counted as neither, a commit that changed only what is left out counted as
//! none, the generator's outputs, lock files and binary files left out of the lines, and a read
//! that fails.

use super::*;
use crate::reports::{DAY, HOUR, day_start};
use crate::test_tree::Tree;
use symphony_api::reports::ReportHour;

/// A UTC midnight in January.
fn midnight() -> f64 {
    day_start(1_800_000_000.0)
}

/// Run git in `root` as of `hours` after midnight, with no user or system settings.
fn git(root: &Path, hours: f64, args: &[&str]) {
    #[allow(clippy::cast_possible_truncation)]
    let date = format!("@{} +0000", (midnight() + hours * HOUR) as i64);
    let mut command = symphony_board::sys::Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(name);
    }
    let done = symphony_board::sys::output(
        command
            .args([
                "-c",
                "user.name=Reports test",
                "-c",
                "user.email=r@example.invalid",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
}

/// `count` numbered lines.
fn lines(count: usize, word: &str) -> String {
    (0..count).map(|n| format!("{word} {n}\n")).collect()
}

/// Write `files` in `tree` and commit them as of `hours`, its subject `subject`.
fn commit(tree: &Tree, hours: f64, subject: &str, files: &[(&str, &str)]) {
    for (path, text) in files {
        tree.write(path, text);
    }
    git(&tree.root, hours, &["add", "-A"]);
    git(&tree.root, hours, &["commit", "-qm", subject]);
}

/// A project whose generator's outputs are `dist/`, its history: a commit the day before, one on
/// main at 00:30 with a lock file, a branch's work and revert merged at 02:15 with a generated file
/// and a picture, a branch that took main in at 02:48 before its merge at 03:12, a commit on main
/// between, a branch changing a lock file alone, merged at 03:30, and the generator's own commit
/// on main at 03:45.
fn repository() -> Tree {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[land]\ngenerators = [{ name = \"dist\", outputs = \"dist/\", command = [\"true\"] }]\n",
    );
    git(&tree.root, -2.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -2.0, "base", &[("a.txt", &lines(3, "a"))]);
    commit(
        &tree,
        0.5,
        "docs: owner verdicts",
        &[
            ("a.txt", &lines(5, "a")),
            ("Cargo.lock", &lines(10, "lock")),
        ],
    );
    git(&tree.root, 1.0, &["switch", "-qc", "feature"]);
    commit(
        &tree,
        1.0,
        "work",
        &[
            ("b.txt", &lines(5, "b")),
            ("dist/app.js", &lines(100, "js")),
            ("crates/x/Cargo.lock", &lines(7, "lock")),
        ],
    );
    std::fs::write(tree.path("img.png"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    commit(
        &tree,
        1.5,
        "Revert \"docs: owner verdicts\"",
        &[("a.txt", &lines(3, "a"))],
    );
    git(&tree.root, 2.25, &["switch", "-q", "main"]);
    git(
        &tree.root,
        2.25,
        &["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"],
    );
    git(&tree.root, 2.5, &["switch", "-qc", "other"]);
    commit(&tree, 2.5, "other work", &[("c.txt", "c\n")]);
    git(&tree.root, 2.6, &["switch", "-q", "main"]);
    commit(&tree, 2.6, "tweak", &[("d.txt", "d\n")]);
    git(&tree.root, 2.8, &["switch", "-q", "other"]);
    git(
        &tree.root,
        2.8,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge main into other",
            "main",
        ],
    );
    git(&tree.root, 3.2, &["switch", "-q", "main"]);
    git(
        &tree.root,
        3.2,
        &["merge", "-q", "--no-ff", "-m", "Merge other", "other"],
    );
    git(&tree.root, 3.4, &["switch", "-qc", "locks"]);
    commit(
        &tree,
        3.4,
        "lock bump",
        &[("Cargo.lock", &lines(11, "lock"))],
    );
    git(&tree.root, 3.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        3.5,
        &["merge", "-q", "--no-ff", "-m", "Merge locks", "locks"],
    );
    commit(
        &tree,
        3.75,
        "client: rebuild the distribution",
        &[("dist/app.js", &lines(101, "js"))],
    );
    tree
}

/// The line of first parents' read alone, its pathspecs those of `excluded`: what git itself
/// leaves out.
fn line(tree: &Tree, excluded: &[String]) -> Vec<Commit> {
    read(&tree.root, "main", midnight(), excluded).unwrap().line
}

/// A day file with its hours up to `until` hours after midnight.
fn day(until: f64) -> ReportDay {
    let start = midnight();
    ReportDay {
        start,
        until: start + until * HOUR,
        hours: (0..24u32)
            .take_while(|hour| *hour == 0 || f64::from(*hour) < until)
            .map(|hour| ReportHour {
                start: start + f64::from(hour) * HOUR,
                ..ReportHour::default()
            })
            .collect(),
        ..ReportDay::default()
    }
}

fn counts(day: &ReportDay) -> Vec<(u32, u32, u64, u64, u32)> {
    day.hours
        .iter()
        .map(|hour| {
            let git = hour.git.unwrap();
            (git.commits, git.merges, git.added, git.removed, git.reverts)
        })
        .collect()
}

#[test]
fn two_reads_count_commits_by_the_hour_they_reached_the_branch_and_lines_along_the_first_parents() {
    let tree = repository();
    let project = Project::of(&tree.root);
    let excluded = excluded(&project);
    assert_eq!(
        excluded[0], "dist/",
        "the generator's outputs, then the lock files"
    );
    assert_eq!(&excluded[1..], LOCKS.map(str::to_string));
    let read = read(&tree.root, "main", midnight(), &excluded).unwrap();
    let subjects: Vec<&str> = read.line.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        subjects,
        [
            "client: rebuild the distribution",
            "Merge locks",
            "Merge other",
            "tweak",
            "Merge feature",
            "docs: owner verdicts"
        ],
        "the line of first parents from the tip, a commit and a merge whose lines are all left out \
         kept, and nothing made before the window"
    );
    assert_eq!(
        read.all.len(),
        11,
        "every commit since the one the line starts from"
    );
    assert!(read.all.iter().all(|c| c.added == 0 && c.removed == 0));
    let unchanged: Vec<&str> = read
        .all
        .iter()
        .filter(|c| c.parents.len() < 2 && !c.changed)
        .map(|c| c.subject.as_str())
        .collect();
    assert_eq!(
        unchanged,
        ["client: rebuild the distribution", "lock bump"],
        "the commits that changed only what is left out"
    );
    let mut day = day(4.0);
    let from = GitRead {
        branch: "main".into(),
        excluded,
    };
    fill(&mut day, &read, from.clone());
    assert_eq!(
        counts(&day),
        [
            (1, 0, 2, 0, 0),
            (0, 0, 0, 0, 0),
            (3, 1, 6, 2, 1),
            (1, 2, 1, 0, 0),
        ],
        "00:30's commit and its two lines, the lock file's left out; nothing reached the branch \
         in hour 1; the branch's two commits, its revert among them, with their merge at 02:15 and \
         its lines, less the generated file, the lock files and the picture, and the tweak beside \
         them; the branch's merge of main counted as nothing, its own merge bringing c.txt alone, \
         and the lock file's branch and the generator's own commit a landing of no lines and no \
         commit"
    );
    assert_eq!(day.git, Some(from));
}

#[test]
fn a_branch_s_commit_made_the_day_before_counts_on_the_day_its_merge_landed() {
    let tree = Tree::new();
    git(&tree.root, -30.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -30.0, "base", &[("a.txt", "a\n")]);
    git(&tree.root, -2.0, &["switch", "-qc", "late"]);
    commit(&tree, -2.0, "made at 22:00", &[("b.txt", "b\n")]);
    commit(&tree, -1.0, "Revert \"made at 22:00\"", &[("b.txt", "")]);
    git(&tree.root, 0.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        0.5,
        &["merge", "-q", "--no-ff", "-m", "Merge late", "late"],
    );
    let today = read(&tree.root, "main", midnight(), &[]).unwrap();
    let mut day = day(1.0);
    fill(&mut day, &today, GitRead::default());
    assert_eq!(
        counts(&day),
        [(2, 1, 0, 0, 1)],
        "both commits, made the day before, reached main with its merge at 00:30"
    );
    let yesterday = read(&tree.root, "main", midnight() - DAY, &[]).unwrap();
    let mut before = ReportDay {
        start: midnight() - DAY,
        until: midnight(),
        hours: (0..24u32)
            .map(|hour| ReportHour {
                start: midnight() - DAY + f64::from(hour) * HOUR,
                ..ReportHour::default()
            })
            .collect(),
        ..ReportDay::default()
    };
    fill(&mut before, &yesterday, GitRead::default());
    assert!(
        before.hours.iter().all(|h| h.git.unwrap().commits == 0),
        "the day they were made reached nothing"
    );
}

#[test]
fn git_itself_leaves_out_the_outputs_and_the_lock_files_at_any_depth() {
    let tree = repository();
    let lines = |excluded: &[String], subject: &str| {
        let line = line(&tree, excluded);
        let commit = line.iter().find(|c| c.subject == subject).unwrap();
        (commit.added, commit.removed)
    };
    let merge = "Merge feature";
    assert_eq!(
        lines(&[], merge),
        (112, 2),
        "b.txt, dist/app.js and crates/x/Cargo.lock"
    );
    assert_eq!(
        lines(&[], "docs: owner verdicts"),
        (12, 0),
        "a.txt and Cargo.lock"
    );
    let excluded = ["dist/".to_string(), "Cargo.lock".to_string()];
    assert_eq!(
        lines(&excluded, merge),
        (5, 2),
        "b.txt alone, no line left out after the read"
    );
    assert_eq!(
        lines(&excluded, "docs: owner verdicts"),
        (2, 0),
        "the top lock file too"
    );
    assert_eq!(
        pathspecs(&excluded),
        [
            ".",
            ":(exclude,literal)dist/",
            ":(exclude,glob)**/Cargo.lock"
        ]
    );
}

#[test]
fn the_hour_under_way_counts_up_to_the_read() {
    let tree = repository();
    let read = read(&tree.root, "main", midnight(), &[]).unwrap();
    let mut day = day(2.5);
    fill(&mut day, &read, GitRead::default());
    assert_eq!(
        counts(&day)[2],
        (2, 1, 112, 2, 1),
        "up to 02:30: the merge and the branch's two commits it brought, no tweak yet, nothing \
         left out but the picture"
    );
}

#[test]
fn a_window_with_nothing_on_the_line_reads_nothing_more() {
    let tree = repository();
    let read = read(&tree.root, "main", midnight() + DAY, &[]).unwrap();
    assert_eq!(read, Read::default());
    let mut day = day(1.0);
    fill(&mut day, &read, GitRead::default());
    assert_eq!(counts(&day), [(0, 0, 0, 0, 0)]);
}

#[test]
fn a_merge_counts_only_what_no_commit_before_it_on_the_line_brought() {
    let commit = |hash: &str, parents: &[&str], at: f64, subject: &str| Commit {
        hash: hash.into(),
        parents: parents.iter().map(|p| (*p).to_string()).collect(),
        at,
        subject: subject.into(),
        changed: true,
        ..Commit::default()
    };
    // m1 lands b1; b2 was branched after it, from m1, and lands with m2, whose walk reaches b1
    // again through m1, the line's own; c1 is the base, from before the window.
    let line = vec![
        commit("m2", &["m1", "b2"], 20.0, "Merge b2"),
        commit("m1", &["c1", "b1"], 10.0, "Merge b1"),
    ];
    let all = vec![
        line[0].clone(),
        commit("b2", &["m1"], 15.0, "Revert \"x\""),
        line[1].clone(),
        commit("b1", &["c1"], 5.0, "b1"),
    ];
    let reached = reached(&Read { line, all });
    let counted: Vec<(f64, bool, u32, u32)> = reached
        .iter()
        .map(|r| (r.at, r.merge, r.commits, r.reverts))
        .collect();
    assert_eq!(
        counted,
        [(10.0, true, 1, 0), (20.0, true, 1, 1)],
        "oldest first, each commit once, by the merge that brought it"
    );
}

#[test]
fn the_settings_marks_are_the_commits_on_the_line_that_changed_the_project_file() {
    let tree = repository();
    // A branch's change of the file lands by its merge, at 04:30; one on main itself at 05:00.
    git(&tree.root, 4.0, &["switch", "-qc", "gates"]);
    commit(
        &tree,
        4.0,
        "gates: a slower check",
        &[("symphony.toml", "[land]\ncheck = false\n")],
    );
    git(&tree.root, 4.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        4.5,
        &["merge", "-q", "--no-ff", "-m", "Merge gates", "gates"],
    );
    commit(
        &tree,
        5.0,
        "settings: limits",
        &[("symphony.toml", "[land]\ncheck = true\n")],
    );
    commit(&tree, 5.5, "elsewhere", &[("e.txt", "e\n")]);
    assert_eq!(
        marks(&tree, midnight()),
        [
            (5.0, "settings: limits".to_string(), "settings".to_string()),
            (4.5, "Merge gates".to_string(), "settings".to_string())
        ],
        "tip first, the merge for the branch's commit, the file's first commit before the window \
         and every commit that left it alone out"
    );
    assert!(
        changes(&tree.root, "main", midnight() + DAY)
            .unwrap()
            .is_empty()
    );
    let error = changes(&tree.root, "trunk", midnight()).unwrap_err();
    assert!(error.contains("trunk"), "{error}");
}

/// The marks of `tree`'s main since `since`, tip first: each one's hour after midnight, its subject
/// and what it changed, joined by commas.
fn marks(tree: &Tree, since: f64) -> Vec<(f64, String, String)> {
    changes(&tree.root, "main", since)
        .unwrap()
        .into_iter()
        .map(|change| {
            (
                (change.commit.at - midnight()) / HOUR,
                change.commit.subject,
                change.changed.join(", "),
            )
        })
        .collect()
}

#[test]
fn a_skill_a_rule_and_a_setting_are_a_mark_each_at_their_hour_and_another_file_is_none() {
    let tree = repository();
    commit(
        &tree,
        4.0,
        "skills: the work skill asks first",
        &[(".agents/skills/work/SKILL.md", "# work\n")],
    );
    commit(&tree, 5.0, "rules: one more", &[("RULES.md", "# Rules\n")]);
    commit(
        &tree,
        6.0,
        "settings: limits",
        &[("symphony.toml", "[land]\ncheck = true\n")],
    );
    commit(
        &tree,
        7.0,
        "guide: the board's",
        &[("crates/tasks/AGENTS.md", "# The board\n")],
    );
    // None of them: a guide alone (the commit above), a source file, a document, a file named
    // like the skills' folder and a link's own path under it, which is no file of a skill.
    commit(
        &tree,
        8.0,
        "elsewhere",
        &[
            ("src/lib.rs", "// code\n"),
            ("crates/tasks/README.md", "# Read me\n"),
            ("docs/skills.md", "# Skills\n"),
            (".claude/skills/work", "../../.agents/skills/work\n"),
            ("docs/RULES.md", "# Not the rules\n"),
        ],
    );
    assert_eq!(
        marks(&tree, midnight()),
        [
            (6.0, "settings: limits", "settings"),
            (5.0, "rules: one more", "rules"),
            (4.0, "skills: the work skill asks first", "skill work"),
        ]
        .map(|(at, subject, changed)| (at, subject.to_string(), changed.to_string())),
    );
}

#[test]
fn a_skill_is_a_file_where_skills_are_installed_or_shipped_and_a_code_folder_named_skills_is_none()
{
    let tree = repository();
    commit(
        &tree,
        4.0,
        "code: the skills module",
        &[
            ("src/skills/x/mod.rs", "// code\n"),
            ("skills/triage/SKILL.md", "# triage\n"),
        ],
    );
    commit(
        &tree,
        5.0,
        "a package's own Claude skill",
        &[("apps/web/.claude/skills/review/SKILL.md", "# review\n")],
    );
    commit(
        &tree,
        6.0,
        "a skill shipped at the root",
        &[("shipped/skills/tasks/SKILL.md", "# tasks\n")],
    );
    assert_eq!(
        marks(&tree, midnight()),
        [
            (6.0, "a skill shipped at the root", "skill tasks"),
            (5.0, "a package's own Claude skill", "skill review"),
        ]
        .map(|(at, subject, changed)| (at, subject.to_string(), changed.to_string())),
        "a skill installed or shipped under any folder is a mark, and code in a folder named \
         `skills` none"
    );
}

#[test]
fn a_landing_is_one_mark_naming_each_thing_its_branch_changed_once() {
    let tree = repository();
    // A branch changes two files of one skill, its shipped copy, a second skill, both guides at
    // the root, the rules and the settings, over three commits, and code; its merge lands them.
    git(&tree.root, 4.0, &["switch", "-qc", "instructions"]);
    commit(
        &tree,
        4.0,
        "skills",
        &[
            (".agents/skills/work/SKILL.md", "# work\n"),
            (".agents/skills/work/start.md", "# start\n"),
            ("crates/app/shipped/skills/work/SKILL.md", "# work\n"),
            ("crates/app/shipped/skills/tasks/SKILL.md", "# tasks\n"),
        ],
    );
    commit(
        &tree,
        4.2,
        "guides and rules",
        &[
            ("AGENTS.md", "# Guide\n"),
            ("CLAUDE.md", "# Guide\n"),
            ("RULES.md", "# Rules\n"),
        ],
    );
    commit(
        &tree,
        4.4,
        "settings and code",
        &[
            ("symphony.toml", "[land]\ncheck = false\n"),
            ("src/lib.rs", "// code\n"),
        ],
    );
    git(&tree.root, 4.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        4.5,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge instructions",
            "instructions",
        ],
    );
    assert_eq!(
        marks(&tree, midnight()),
        [(
            4.5,
            "Merge instructions".to_string(),
            "settings, rules, skill tasks, skill work".to_string()
        )],
        "the merge alone, at its own hour: the settings and the rules, then the skills by name, \
         the guides it changed unnamed"
    );
}

#[test]
fn the_instructions_are_the_settings_the_rules_and_each_skill_s_files_and_no_guide() {
    for (path, named) in [
        ("symphony.toml", Some("settings")),
        ("RULES.md", Some("rules")),
        (".agents/skills/work/project.md", Some("skill work")),
        // A guide is none, at the root or in a package.
        ("AGENTS.md", None),
        ("CLAUDE.md", None),
        ("apps/web/AGENTS.md", None),
        (".agents/skills/work/SKILL.md", Some("skill work")),
        (".agents/skills/work/notes/deep.md", Some("skill work")),
        (".claude/skills/review/SKILL.md", Some("skill review")),
        (
            "crates/app/shipped/skills/tasks/start.md",
            Some("skill tasks"),
        ),
        // Installed or shipped under any folder, and a skill's folder found past a code folder.
        (
            "apps/web/.claude/skills/review/SKILL.md",
            Some("skill review"),
        ),
        ("apps/web/.agents/skills/work/SKILL.md", Some("skill work")),
        ("shipped/skills/tasks/SKILL.md", Some("skill tasks")),
        (
            "src/skills/x/.agents/skills/work/SKILL.md",
            Some("skill work"),
        ),
        // Whatever a skill's folder holds is the skill's.
        (".agents/skills/work/AGENTS.md", Some("skill work")),
        // A project's own folder named `skills` is no place skills are installed or shipped, at
        // the root or under one, and neither is one beside a folder named like those places.
        ("src/skills/x/mod.rs", None),
        ("skills/triage/SKILL.md", None),
        ("crates/app/skills/tasks/start.md", None),
        ("agents/skills/work/SKILL.md", None),
        (".claude/skill/work/SKILL.md", None),
        ("shipped/x/skills/tasks/SKILL.md", None),
        // No file of a skill: the folder's own files and links.
        (".claude/skills/work", None),
        (".agents/skills/README.md", None),
        ("skills", None),
        ("docs/skills.md", None),
        // The settings and the rules are the root's alone, and a name is matched whole.
        ("crates/app/symphony.toml", None),
        ("docs/RULES.md", None),
        ("docs/AGENTS.md.txt", None),
        ("agents.md", None),
        ("src/lib.rs", None),
        ("", None),
    ] {
        assert_eq!(instruction(path).as_deref(), named, "{path}");
    }
}

/// The full name of what `rev` names in `root`.
fn rev_parse(root: &Path, rev: &str) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["rev-parse", rev])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE"),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// Land the task `slug` on main as of `hours`, as the land merges it: a branch of one commit,
/// merged with the land's own subject, or `subject` when given.
fn land(tree: &Tree, hours: f64, slug: &str, subject: Option<&str>) {
    git(&tree.root, hours, &["switch", "-qc", slug]);
    commit(tree, hours, slug, &[(&format!("{slug}.txt"), "work\n")]);
    git(&tree.root, hours, &["switch", "-q", "main"]);
    let said = format!("Merge branch '{slug}'");
    let subject = subject.unwrap_or(&said);
    git(
        &tree.root,
        hours,
        &["merge", "-q", "--no-ff", "-m", subject, slug],
    );
}

#[test]
fn a_landing_is_the_lands_merge_or_gits_own_onto_the_branch() {
    assert_eq!(
        landing_slug("Merge branch 'tools/x'", "main"),
        Some("tools/x")
    );
    assert_eq!(
        landing_slug("Merge branch 'tools/x' into main", "main"),
        Some("tools/x")
    );
    for none in [
        "Merge remote-tracking branch 'origin/main'",
        "Merge branch 'main' of github.com:heavenstudio/symphony",
        "Merge branch 'main' into tools/x",
        "Merge branch 'main'",
        "Merge branch ''",
        "Merge branch 'unclosed",
        "Regenerate the client distribution after tools/x",
        "Merge pull request #3 from x/y",
    ] {
        assert_eq!(landing_slug(none, "main"), None, "{none}");
    }
    // The default branch is the project's: another's merge of `main` is a landing of it.
    assert_eq!(
        landing_slug("Merge branch 'main' into trunk", "trunk"),
        Some("main")
    );
    assert_eq!(
        landing_slug("Merge branch 'trunk' into tools/x", "trunk"),
        None
    );
}

/// A red's range from the commit it was green on to the one it was red on: a landing before it,
/// three in it (the last merged by hand, its branch having taken main in first), the regenerator's
/// commit and a merge of the remote's main between them, and a landing after it.
#[test]
fn the_landings_in_a_red_s_range_are_its_merges_on_the_line_of_first_parents_by_slug() {
    let tree = Tree::new();
    git(&tree.root, 0.0, &["init", "-q", "-b", "main"]);
    commit(&tree, 0.0, "base", &[("a.txt", "a\n")]);
    land(&tree, 0.5, "sim/before", None);
    let good = rev_parse(&tree.root, "HEAD");
    land(&tree, 1.0, "sim/one", None);
    commit(
        &tree,
        1.25,
        "Regenerate the client distribution for main at 1234567",
        &[("dist/app.js", "js\n")],
    );
    land(&tree, 1.5, "sim/two", None);
    git(&tree.root, 1.75, &["switch", "-qc", "upstream"]);
    commit(&tree, 1.75, "pushed from elsewhere", &[("u.txt", "u\n")]);
    git(&tree.root, 1.75, &["switch", "-q", "main"]);
    git(
        &tree.root,
        1.75,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge remote-tracking branch 'origin/main'",
            "upstream",
        ],
    );
    git(&tree.root, 2.0, &["switch", "-qc", "sim/three"]);
    commit(&tree, 2.0, "three", &[("three.txt", "3\n")]);
    git(
        &tree.root,
        2.0,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'main' into sim/three",
            "main",
        ],
    );
    git(&tree.root, 2.25, &["switch", "-q", "main"]);
    git(
        &tree.root,
        2.25,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'sim/three' into main",
            "sim/three",
        ],
    );
    let bad = rev_parse(&tree.root, "HEAD");
    land(&tree, 3.0, "sim/after", None);
    let range = format!("{good}..{bad}");
    assert_eq!(
        landed(&tree.root, "main", &range).unwrap(),
        ["sim/three", "sim/two", "sim/one"],
        "newest first; the landings before and after the range, the regenerator's commit, the \
         remote's merge and a branch's merge of main left out"
    );
    assert_eq!(
        landed(&tree.root, "main", &format!("{bad}..{bad}")).unwrap(),
        Vec::<String>::new(),
        "red where it was green: no landing"
    );
    let unknown = format!("{}..{bad}", "0".repeat(40));
    let error = landed(&tree.root, "main", &unknown).unwrap_err();
    assert!(
        error.starts_with(&format!("git log of {unknown} failed: ")),
        "{error}"
    );

    let red = |slug: &str, between: Option<&str>| symphony_api::reports::Red {
        slug: slug.into(),
        between: between.map(str::to_string),
        ..symphony_api::reports::Red::default()
    };
    let mut day = day(4.0);
    day.reds = vec![
        red("tests/release-red-verify", Some(&range)),
        red("tests/main-red-1", None),
        red("tests/release-red-lint", Some(&unknown)),
    ];
    let mut problems = Vec::new();
    trace(&mut day, &tree.root, "main", &mut problems);
    trace(&mut day, &tree.root, "main", &mut problems);
    let traced: Vec<Option<Vec<String>>> = day.reds.iter().map(|red| red.landed.clone()).collect();
    assert_eq!(
        traced,
        [
            Some(vec!["sim/three".into(), "sim/two".into(), "sim/one".into()]),
            None,
            None,
        ],
        "a red that names no range, or one git cannot read, has no landings"
    );
    assert_eq!(problems.len(), 1, "said once: {problems:?}");
    assert!(
        problems[0].starts_with(&format!(
            "git: tests/release-red-lint: git log of {unknown}"
        )),
        "{problems:?}"
    );
}

#[test]
fn a_read_of_no_repository_or_no_branch_fails_with_why() {
    let tree = Tree::new();
    let error = read(&tree.root, "main", midnight(), &[]).unwrap_err();
    assert!(error.starts_with("git log of main failed: "), "{error}");
    let tree = repository();
    let error = read(&tree.root, "trunk", midnight(), &[]).unwrap_err();
    assert!(error.contains("trunk"), "{error}");
}

#[test]
fn a_record_reads_its_fields_and_lines_and_skips_what_does_not_read() {
    let text = "\x1eaaa\x1fbbb ccc\x1f100\x1fMerge x\n\n3\t1\tsrc/a.rs\n-\t-\tpic.png\n2\t2\tdist/x.js\n\
                odd line\nx\t1\tbad.rs\n\
                \x1eddd\x1f\x1f200\x1fRoot\n\
                \x1enot a record\n\
                \x1eeee\x1fddd\x1fnot a time\x1fBad\n";
    let commits = parse(text);
    assert_eq!(
        commits,
        [
            Commit {
                hash: "aaa".into(),
                parents: vec!["bbb".into(), "ccc".into()],
                at: 100.0,
                subject: "Merge x".into(),
                added: 5,
                removed: 3,
                changed: true,
            },
            Commit {
                hash: "ddd".into(),
                parents: Vec::new(),
                at: 200.0,
                subject: "Root".into(),
                ..Commit::default()
            },
        ],
        "a numstat line's counts, a binary file's and a name's none but a change, and a record \
         with no change"
    );
}

#[test]
fn instruction_changes_keep_rank_names_and_one_label_per_skill() {
    let tree = Tree::new();
    git(&tree.root, -1.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -1.0, "baseline", &[("README.md", "# Baseline\n")]);
    assert!(changes(&tree.root, "main", midnight()).unwrap().is_empty());
    commit(
        &tree,
        1.0,
        "instructions",
        &[
            ("symphony.toml", "[land]\ncheck=false\n"),
            ("RULES.md", "Rules\n"),
            (".agents/skills/zeta/SKILL.md", "Z\n"),
            (".agents/skills/zeta/details.md", "Details\n"),
            (".agents/skills/alpha/SKILL.md", "A\n"),
            (".agents/skills/Éclair/SKILL.md", "E\n"),
        ],
    );
    let rows = changes(&tree.root, "main", midnight()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].commit.subject, "instructions");
    assert_eq!(
        rows[0].changed,
        [
            "settings",
            "rules",
            "skill alpha",
            "skill zeta",
            "skill Éclair"
        ]
    );
}

/// Keep the receipt of a land a pull request delivered for `slug` in `lands`, naming `squash` the
/// commit the host made, as `tasks land` writes one.
fn receipt(lands: &Path, slug: &str, squash: &str) {
    let path = symphony_board::land::delivery::Receipt::path(lands, slug);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let receipt = serde_json::json!({
        "slug": slug, "task": 1, "operation": "op", "repository": 1, "name": "o/r",
        "pull": 1, "head": "h", "base_observed": "b", "parent": "p", "squash": squash,
        "tree": "t", "revision": "1", "by": "board",
    });
    std::fs::write(path, receipt.to_string()).unwrap();
}

/// What `day`'s landed tasks were given: each one's slug and its lines added and removed.
fn landed_lines(day: &ReportDay) -> Vec<(&str, Option<(u64, u64)>)> {
    (day.landed.iter())
        .map(|task| {
            let lines = task.lines.map(|lines| (lines.added, lines.removed));
            (task.slug.as_str(), lines)
        })
        .collect()
}

/// A day's landings: one the land merged, one a pull request squashed with its receipt kept, a
/// commit whose subject is a slug and has no receipt, the regenerator's squash of its outputs
/// alone, and a task whose commit is not on the line read.
#[test]
fn a_landed_task_takes_the_lines_of_the_merge_or_the_squash_that_landed_it() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[land]\ngenerators = [{ name = \"dist\", outputs = \"dist/\", command = [\"true\"] }]\n",
    );
    git(&tree.root, -2.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -2.0, "base", &[("a.txt", &lines(3, "a"))]);
    land(&tree, 0.5, "sim/merged", None);
    commit(
        &tree,
        1.0,
        "sim/squashed",
        &[
            ("a.txt", &lines(5, "a")),
            ("x.txt", &lines(3, "x")),
            ("Cargo.lock", &lines(9, "lock")),
        ],
    );
    let squashed = rev_parse(&tree.root, "HEAD");
    commit(&tree, 1.5, "sim/unreceipted", &[("y.txt", "y\n")]);
    commit(
        &tree,
        2.0,
        "generated/outputs",
        &[("dist/app.js", &lines(4, "js"))],
    );
    let generated = rev_parse(&tree.root, "HEAD");
    let lands = tree.path("lands");
    receipt(&lands, "sim/squashed", &squashed);
    receipt(&lands, "generated/outputs", &generated);
    // A receipt of a task the day did not land changes nothing.
    receipt(&lands, "sim/elsewhere", &squashed);
    let excluded = excluded(&Project::of(&tree.root));
    let read = read(&tree.root, "main", midnight(), &excluded).unwrap();
    let mut day = day(3.0);
    day.landed = [
        "sim/merged",
        "sim/squashed",
        "sim/unreceipted",
        "generated/outputs",
        "sim/before",
    ]
    .iter()
    .map(|slug| symphony_api::reports::Landed {
        slug: (*slug).to_string(),
        ..symphony_api::reports::Landed::default()
    })
    .collect();
    landings(&mut day, &read, "main", Some(&lands));
    assert_eq!(
        landed_lines(&day),
        [
            ("sim/merged", Some((1, 0))),
            // Its lock file left out, as the hours leave it out.
            ("sim/squashed", Some((5, 0))),
            ("sim/unreceipted", None),
            ("generated/outputs", Some((0, 0))),
            ("sim/before", None),
        ]
    );
    // With no lands folder a squash is no landing; the land's merge still is.
    landings(&mut day, &read, "main", None);
    assert_eq!(
        landed_lines(&day)[..2],
        [("sim/merged", Some((1, 0))), ("sim/squashed", None)]
    );
    // The lands folder is in the repository's git common dir; none outside a repository.
    let common = symphony_board::git::common_dir(&tree.root).unwrap();
    assert_eq!(
        super::lands(&tree.root),
        Some(common.join("symphony/tasks/land"))
    );
    assert_eq!(super::lands(Path::new("/")), None);
}

/// A task landed twice takes the lines of its newest landing on the line.
#[test]
fn a_task_landed_again_takes_its_newest_landings_lines() {
    let tree = Tree::new();
    git(&tree.root, -2.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -2.0, "base", &[("a.txt", "a\n")]);
    land(&tree, 0.5, "sim/again", None);
    git(&tree.root, 1.0, &["switch", "-q", "sim/again"]);
    commit(&tree, 1.0, "more", &[("more.txt", &lines(4, "m"))]);
    git(&tree.root, 1.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        1.5,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'sim/again'",
            "sim/again",
        ],
    );
    let read = read(&tree.root, "main", midnight(), &[]).unwrap();
    let mut day = day(2.0);
    day.landed = vec![symphony_api::reports::Landed {
        slug: "sim/again".into(),
        ..symphony_api::reports::Landed::default()
    }];
    landings(&mut day, &read, "main", None);
    assert_eq!(landed_lines(&day), [("sim/again", Some((4, 0)))]);
}
