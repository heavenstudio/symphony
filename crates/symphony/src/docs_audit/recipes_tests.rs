//! [`super::recipes`]: the reader of the rule of the recipes a text may name, and the docs audit's
//! judging by it, in the Symphony's own checkout and never in a consuming project's.

use std::path::Path;

use super::recipes::{
    ADVICE, Entry, INSTALLED, LISTS, Rule, Standing, audited, beside_documents, guide, named_in,
    recipes,
};
use crate::test_tree::Tree;

/// This repository's own lists, as the audit reads them in its checkout.
const REPOSITORY: &str = include_str!("../../../../tools/named-recipes.txt");

/// Lists of one entry each: `land-approve` the consumer's, `the` and `to` prose, and `docs/A.md`
/// naming `verify` as its own and `check-fast` as filed.
const LISTED: &str = "# The lists.\n\nconsumers land-approve\nprose the to # two words\n\
                      own docs/A.md verify\nfiled docs/A.md check-fast tools/check-fast\n";

/// [`LISTED`], with `tasks` installed and `fmt` and `lint-rs` this repository's recipes.
fn rule() -> Rule<'static> {
    Rule::read(LISTED, "tasks *ARGS:\n", "fmt:\nlint-rs *ARGS:\n").unwrap()
}

fn refused(file: &str, line: usize, name: &str) -> String {
    format!(
        "{file}:{line}: names `just {name}`, a recipe a freshly initialised project lacks: {ADVICE}"
    )
}

fn stale(file: &str, name: &str) -> String {
    format!("{file}: names no `just {name}`, which {LISTS} lists for it: take that entry out")
}

fn apart(file: &str, line: usize, name: &str, task: &str) -> String {
    format!(
        "{file}:{line}: names `just {name}`, which this repository's justfile does not have yet, on \
         no line beside the task filed to make it: write [[task:{task}]] on the line that names it"
    )
}

fn unreadable(line: usize) -> String {
    format!(
        "{LISTS}:{line}: not `consumers NAME...`, `prose WORD...`, `own FILE NAME...` or `filed \
         FILE NAME TASK`"
    )
}

/// The errors of `texts`, each a file and its text from its first line, for the place that judges
/// what `owns` says.
fn errors(rule: &Rule, texts: &[(&str, &str)], owns: fn(&str) -> bool) -> Vec<String> {
    let texts: Vec<_> = (texts.iter())
        .map(|(file, text)| (file.to_string(), 1, text.to_string()))
        .collect();
    rule.errors(&texts, owns)
}

fn named(text: &str, first: usize) -> Vec<(usize, &str)> {
    named_in(text, first).collect()
}

#[test]
fn a_justfile_s_recipes_are_its_public_headers() {
    assert_eq!(
        recipes(
            "set x := 'y'\n_private := 'p'\n# just tasks: no\n[positional-arguments]\ntasks *ARGS:\n    \
             @echo\nbuild: fmt\nsymphony-cli ARG=\"a:b\":\nkey := \"v\"\n"
        ),
        ["tasks", "build", "symphony-cli"]
    );
}

/// A name follows `just` after a word boundary and across one line break, never a blank line.
#[test]
fn just_names_are_read_after_a_word_boundary_and_counted_by_line() {
    assert_eq!(
        named(
            "run `just tasks`\nthen just\n  agent-send x, adjust it, just_x y",
            3
        ),
        [(3, "tasks"), (5, "agent-send")]
    );
    for none in [
        "just-in-time, just \"quoted\", just {x}",
        "It ends with just\n\nverify, a new paragraph.",
        "`just --list`, `just -l` and `just --help` run none",
        "`just -f other.just` reads another file",
    ] {
        assert_eq!(named(none, 1), Vec::<(usize, &str)>::new(), "{none}");
    }
}

/// The options that leave a recipe to run come before its name: a justfile or a folder named,
/// either way, and the quiet, verbose, dry-run and yes switches.
#[test]
fn a_name_after_justs_options_is_the_recipe_it_runs() {
    for (text, name) in [
        ("just -f x deploy", "deploy"),
        ("`just --justfile ../j tasks`", "tasks"),
        ("just --justfile=../j tasks", "tasks"),
        ("just -d dir -q gate", "gate"),
        ("just --working-directory=d --dry-run verify", "verify"),
        ("just -vv --yes\n  land", "land"),
    ] {
        let line = 1 + text.matches('\n').count();
        assert_eq!(named(text, 1), [(line, name)], "{text}");
    }
}

/// The lists read as the rule, a comment and a blank line aside, beside the recipes of both
/// justfiles; an own line may name several recipes.
#[test]
fn the_lists_read_as_the_rule_with_the_recipes_of_both_justfiles() {
    let entry = |file, name, task| Entry { file, name, task };
    assert_eq!(
        rule(),
        Rule {
            installed: vec!["tasks"],
            justfile: vec!["fmt", "lint-rs"],
            listed: vec!["land-approve", "the", "to"],
            entries: vec![
                entry("docs/A.md", "verify", None),
                entry("docs/A.md", "check-fast", Some("tools/check-fast")),
            ],
        }
    );
    let several = Rule::read("own docs/B.md fmt lint-rs\n", "", "").unwrap();
    assert_eq!(
        several.entries,
        [
            entry("docs/B.md", "fmt", None),
            entry("docs/B.md", "lint-rs", None)
        ]
    );
    assert_eq!(Rule::read("", "", "").unwrap(), Rule::default());
    assert_eq!(
        Rule::read("# only\n\n  \n", "", "").unwrap(),
        Rule::default()
    );
}

/// A line that is no list is refused naming the lists and the line: a list the rule has none of, an
/// own line naming no recipe, a filed one with no task or one word too many.
#[test]
fn lists_that_do_not_read_as_the_rule_are_refused_by_their_line() {
    for line in [
        "owned docs/A.md verify",
        "own docs/A.md",
        "own",
        "filed docs/A.md check-fast",
        "filed docs/A.md check-fast tools/check-fast more",
        "docs/A.md verify",
    ] {
        let lists = format!("# The lists.\nprose the\n{line}\nprose to\n");
        assert_eq!(Rule::read(&lists, "", ""), Err(unreadable(3)), "{line}");
    }
}

/// This repository's prose list holds the words its texts write after "just" as English, so none of
/// them is refused anywhere; a recipe it has none of still is. (Its entries are kept current by
/// neither place here.)
#[test]
fn the_repositorys_prose_is_no_recipe() {
    let rule = Rule::read(REPOSITORY, "", "").unwrap();
    let text = "Not just in case, just to see, just because, just so, just when it ran, just the \
                docs, just as before: `just gate`.\n";
    assert_eq!(
        errors(&rule, &[("docs/A.md", text)], |_| false),
        [refused("docs/A.md", 1, "gate")]
    );
}

/// A name stands by the first list that holds it; an entry's only in the file the line names it
/// for, and a recipe of this repository's justfile only in a guide.
#[test]
fn a_name_stands_by_the_first_list_that_holds_it_and_an_entry_only_in_its_file() {
    let rule = Rule {
        installed: vec!["tasks", "verify"],
        ..rule()
    };
    assert_eq!(rule.standing("docs/B.md", "tasks"), Standing::Installed);
    assert_eq!(rule.standing("docs/A.md", "verify"), Standing::Installed);
    assert_eq!(rule.standing("docs/B.md", "land-approve"), Standing::Listed);
    assert_eq!(rule.standing("docs/B.md", "to"), Standing::Listed);
    assert_eq!(rule.standing("docs/A.md", "check-fast"), Standing::Entry(1));
    assert_eq!(rule.standing("docs/B.md", "check-fast"), Standing::Unknown);
    let rule = Rule {
        installed: Vec::new(),
        ..rule
    };
    assert_eq!(rule.standing("docs/A.md", "verify"), Standing::Entry(0));
    assert_eq!(rule.standing("docs/B.md", "verify"), Standing::Unknown);
    for guide in [
        "AGENTS.md",
        "crates/tasks/AGENTS.md",
        "apps/x/src/README.md",
    ] {
        assert_eq!(rule.standing(guide, "fmt"), Standing::Justfile, "{guide}");
    }
    for other in ["docs/A.md", "crates/symphony/shipped/skills/w/SKILL.md"] {
        assert_eq!(rule.standing(other, "fmt"), Standing::Unknown, "{other}");
    }
}

#[test]
fn the_audit_judges_the_documents_what_the_symphony_delivers_and_its_guides() {
    for file in [
        "docs/CHECKS.md",
        "docs/sub/GUIDE.md",
        "docs/analysis.md",
        "docs/historyx.md",
    ] {
        assert!(audited(file) && !beside_documents(file), "{file}");
    }
    for file in [
        "crates/symphony/shipped/symphony.just",
        "crates/symphony/shipped/codex.rules",
        "crates/symphony/shipped/skills/work/SKILL.md",
        "crates/symphony/shipped/skills/apply-review/rewrite-brief.md",
        "AGENTS.md",
        "README.md",
        "RULES.md",
        "crates/tasks/AGENTS.md",
        "apps/symphony-server/README.md",
        "apps/symphony-web/src/styles/README.md",
    ] {
        assert!(audited(file) && beside_documents(file), "{file}");
    }
    for file in [
        "docs/DECISIONS.md",
        "docs/analysis/one.md",
        "docs/history/README.md",
        "docs/history/deep/old.md",
        "docs/picture.png",
        "docs/CHECKS.md.orig",
        "CLAUDE.md",
        "crates/symphony/src/nightly.rs",
        "apps/symphony-server/src/api/client.rs",
        "tools/AGENTS.md",
        "other/docs/A.md",
        "crates/symphony/shippedx/a.md",
    ] {
        assert!(!audited(file), "{file}");
    }
    assert!(guide("crates/tasks/AGENTS.md") && guide("README.md"));
    assert!(
        !guide("docs/README.md"),
        "a document, judged by the lists' entries"
    );
    assert!(
        !guide("crates/symphony/shipped/skills/w/README.md"),
        "shipped"
    );
    assert!(!guide("crates/tasks/GUIDE.md") && !guide("tools/README.md"));
}

/// Each name none of the lists allows is refused by its file and line, in a code block or across a
/// line break too, counted from the line its text starts on.
#[test]
fn a_name_no_list_allows_is_refused_by_its_file_and_line() {
    let a = "Run `just tasks`.\n\n```\njust gate\n```\nThen just the docs, `just land-approve`, \
             just\n  verify and `just check-fast` [[task:tools/check-fast]].\n";
    let b = "# B\n\n`just verify` is A's own, not B's.\n";
    let guide = "`just fmt` and `just lint-rs`, but `just clean`.\n";
    assert_eq!(
        errors(
            &rule(),
            &[("docs/A.md", a), ("docs/B.md", b), ("AGENTS.md", guide)],
            audited,
        ),
        [
            refused("docs/A.md", 4, "gate"),
            refused("docs/B.md", 3, "verify"),
            refused("AGENTS.md", 1, "clean"),
        ]
    );
    let literal = [(
        "crates/x/src/lib.rs".to_string(),
        40,
        "say `just gate`".to_string(),
    )];
    assert_eq!(
        rule().errors(&literal, |file| !audited(file)),
        [refused("crates/x/src/lib.rs", 40, "gate")]
    );
}

/// An own or filed entry is refused once no text of its file names it, or its file is not there,
/// by the place that judges that file alone.
#[test]
fn an_entry_its_file_no_longer_names_is_refused_where_that_file_is_judged() {
    let lists = "own docs/A.md verify\nown docs/Gone.md verify\nown crates/x/src/lib.rs nightly\n\
                 filed docs/A.md check-fast tools/check-fast\n\
                 filed crates/x/src/lib.rs later tools/later\n";
    let rule = Rule::read(lists, "", "").unwrap();
    let texts = [("docs/A.md", "Nothing to run.\n")];
    assert_eq!(
        errors(&rule, &texts, audited),
        [
            stale("docs/A.md", "verify"),
            stale("docs/Gone.md", "verify"),
            stale("docs/A.md", "check-fast"),
        ]
    );
    assert_eq!(
        errors(&rule, &texts, |file| !audited(file)),
        [
            stale("crates/x/src/lib.rs", "nightly"),
            stale("crates/x/src/lib.rs", "later"),
        ]
    );
}

/// Every entry is kept current by exactly one place, whatever its file: the audit (what
/// [`audited`] names) or the hooks test (every other file).
#[test]
fn each_entry_is_kept_current_by_exactly_one_place() {
    let files = [
        "docs/A.md",
        "docs/DECISIONS.md",
        "crates/symphony/shipped/skills/w/SKILL.md",
        "AGENTS.md",
        "crates/x/src/lib.rs",
        "apps/y/src/main.rs",
    ];
    let mut lists = String::new();
    for file in files {
        lists.push_str(&format!(
            "own {file} verify\nfiled {file} later tools/later\n"
        ));
    }
    let rule = Rule::read(&lists, "", "").unwrap();
    let audit = errors(&rule, &[], audited);
    let test = errors(&rule, &[], |file| !audited(file));
    for file in files {
        for stale in [stale(file, "verify"), stale(file, "later")] {
            let found = audit.iter().chain(&test).filter(|e| **e == stale).count();
            assert_eq!(found, 1, "{stale}: {audit:#?} {test:#?}");
        }
    }
    assert_eq!(audit.len() + test.len(), 2 * files.len());
}

/// A filed recipe is named, in code, on a line that names the task filed to make it, in a document
/// or in one of a source file's strings alike; another file's line beside the task is not its own.
#[test]
fn a_filed_recipe_is_named_beside_the_task_filed_to_make_it() {
    let lists = "filed docs/A.md check-fast tools/check-fast\n\
                 filed crates/x/src/lib.rs later tools/later\n";
    let rule = Rule::read(lists, "", "").unwrap();
    let both = |a: &str, x: &str| {
        let mut found = errors(&rule, &[("docs/A.md", a)], audited);
        found.extend(errors(&rule, &[("crates/x/src/lib.rs", x)], |f| {
            !audited(f)
        }));
        found
    };
    let beside = "Its recipe, `just check-fast`, is [[task:tools/check-fast]].\n";
    let source = "run `just later` once [[task:tools/later]] lands";
    assert_eq!(both(beside, source), Vec::<String>::new());
    let split = "Intro.\nIts recipe, `just check-fast`, is\nfiled as [[task:tools/check-fast]].\n";
    let uncoded = "Its recipe, just check-fast, is [[task:tools/check-fast]].\n";
    let other = "Its recipe, `just check-fast`, is [[task:tools/other]].\n";
    for (text, line) in [(split, 2), (uncoded, 1), (other, 1)] {
        assert_eq!(
            both(text, source),
            [apart("docs/A.md", line, "check-fast", "tools/check-fast")],
            "{text}"
        );
    }
    assert_eq!(
        both(beside, "run `just later`"),
        [apart("crates/x/src/lib.rs", 1, "later", "tools/later")]
    );
    // A source file's strings are texts of its own: the task beside the name in either counts, and
    // the first line that names the recipe is the one refused.
    let strings = [
        (
            "crates/x/src/lib.rs".to_string(),
            7,
            "`just later`".to_string(),
        ),
        (
            "crates/x/src/lib.rs".to_string(),
            9,
            "`just later`, [[task:tools/later]]".to_string(),
        ),
    ];
    assert_eq!(
        rule.errors(&strings, |file| !audited(file)),
        Vec::<String>::new()
    );
    let elsewhere = [
        (
            "crates/x/src/lib.rs".to_string(),
            7,
            "`just later`".to_string(),
        ),
        (
            "crates/x/src/other.rs".to_string(),
            3,
            "`just later` [[task:tools/later]]".to_string(),
        ),
    ];
    assert_eq!(
        rule.errors(&elsewhere, |file| !audited(file)),
        [
            refused("crates/x/src/other.rs", 3, "later"),
            apart("crates/x/src/lib.rs", 7, "later", "tools/later"),
        ]
    );
}

/// `git` in `at`, which must succeed.
fn git(at: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(at)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The Symphony's own checkout, each file in git's index: its project file names it, and it holds
/// the lists, the recipes init installs, a justfile, a shipped skill, guides and documents.
fn symphony() -> Tree {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nname = \"Symphony\"\nid = \"symphony\"\n",
    );
    tree.write(LISTS, LISTED);
    tree.write(INSTALLED, "tasks *ARGS:\n    tasks \"$@\"\n");
    tree.write(
        "justfile",
        "fmt:\n    cargo fmt\n\nlint-rs:\n    cargo clippy\n",
    );
    tree.write(
        "crates/symphony/shipped/skills/work/SKILL.md",
        "# Work\n\nRun `just tasks claim`,\nthen `just lint-rs`.\n",
    );
    tree.write(
        "AGENTS.md",
        "# Agents\n\nUse `just fmt` and `just tasks`, never `just clean`.\n",
    );
    tree.write("crates/tasks/AGENTS.md", "# Board\n\n`just deploy` it.\n");
    tree.write("apps/x/src/lib.rs", "const S: &str = \"just gate\";\n");
    tree.write(
        "docs/A.md",
        "# A\n\n`just verify` is A's own, and `just check-fast` is [[task:tools/check-fast]].\n\n\
         Not `just gate`.\n",
    );
    tree.write("docs/analysis/x.md", "# X\n\nWe ran `just gate`.\n");
    git(&tree.root, &["init", "-q"]);
    git(&tree.root, &["add", "-A"]);
    tree
}

/// A project shaped as a consumer, in git: its justfile importing the Symphony's recipes beside its
/// own, a guide and a document naming both, and the Symphony's crate vendored at the path the
/// Symphony's own checkout keeps it, with lists of its own.
fn consumer() -> Tree {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nname = \"Lamp Shop\"\nid = \"lamp-shop\"\n",
    );
    tree.write(
        "justfile",
        "import 'symphony.just'\n\ngate:\n    cargo test\n\ndeploy:\n    ./deploy.sh\n",
    );
    tree.write("AGENTS.md", "# Agents\n\nRun `just gate`.\n");
    tree.write(
        "docs/README.md",
        "# Lamp shop\n\nRun `just gate` before `just deploy`, and `just tasks list` for the board.\n",
    );
    tree.write(INSTALLED, "tasks *ARGS:\n    tasks \"$@\"\n");
    tree.write(
        "crates/symphony/shipped/skills/work/SKILL.md",
        "Run `just gate`.\n",
    );
    tree.write(LISTS, "prose a\n");
    git(&tree.root, &["init", "-q"]);
    git(&tree.root, &["add", "-A"]);
    tree
}

fn audited_by_cli(root: &Path) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = [
        "--check".to_string(),
        "--root".into(),
        root.display().to_string(),
    ];
    let code = super::main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// A consuming project's texts name its own recipes: its audit judges none of them, the Symphony's
/// crate and a lists file in its checkout notwithstanding.
#[test]
fn a_consuming_projects_audit_judges_no_recipe_its_texts_name() {
    let tree = consumer();
    assert_eq!(super::audit(&tree.root).errors, Vec::<String>::new());
    let (code, out, err) = audited_by_cli(&tree.root);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.ends_with(
            "docs audit: every document renders, and every card waiting on the owner can be answered\n"
        ),
        "{out}"
    );
}

/// The Symphony's own checkout is judged by its lists: its documents but the records, and of the
/// files git tracks what it delivers and its guides, each name no list allows refused with its file
/// and line; a binary's string is the hooks test's. A file git does not track yet is judged once it
/// does, and a project named `symphony` with no lists is not judged.
#[test]
fn the_symphonys_own_audit_refuses_a_recipe_its_lists_do_not_allow() {
    let tree = symphony();
    let judged = [
        refused("docs/A.md", 5, "gate"),
        refused("AGENTS.md", 3, "clean"),
        refused("crates/symphony/shipped/skills/work/SKILL.md", 4, "lint-rs"),
        refused("crates/tasks/AGENTS.md", 3, "deploy"),
    ];
    assert_eq!(super::audit(&tree.root).errors, judged);
    let (code, _, err) = audited_by_cli(&tree.root);
    assert_eq!(code, 1);
    assert!(
        err.starts_with(&format!("✖ {}\n", refused("docs/A.md", 5, "gate"))),
        "{err}"
    );
    let new = "crates/symphony/shipped/skills/new/SKILL.md";
    tree.write(new, "Run `just gate`.\n");
    assert_eq!(super::audit(&tree.root).errors, judged);
    git(&tree.root, &["add", new]);
    let errors = super::audit(&tree.root).errors;
    assert!(errors.contains(&refused(new, 1, "gate")), "{errors:#?}");
    // A file git tracks that is gone from the checkout names nothing.
    std::fs::remove_file(tree.path(new)).unwrap();
    std::fs::remove_file(tree.path("crates/tasks/AGENTS.md")).unwrap();
    assert_eq!(super::audit(&tree.root).errors, judged[..3]);
    std::fs::remove_file(tree.path(LISTS)).unwrap();
    assert_eq!(super::audit(&tree.root).errors, Vec::<String>::new());
}

/// A card's own lines, inline in a document or in a region of it, are a record of what was
/// proposed, not words that tell a project what to run: the audit judges the prose around them,
/// each name at the line it is on.
#[test]
fn the_symphonys_own_audit_passes_over_a_cards_lines_and_judges_the_prose_around_them() {
    let tree = symphony();
    tree.write(
        "docs/B.md",
        "# B\n\n- [r] **gate-recipe** — `just gate` would run it.\n  [2026-11-21 10:00] agent — Ran \
         `just gate`.\n\n    Then `just gate` again.\n\nNow `just gate`.\n\n<!-- register:start -->\n## \
         Resolved\n\n`just gate`\n<!-- register:end -->\nAnd `just clean`.\n",
    );
    let errors = super::audit(&tree.root).errors;
    let of_b: Vec<&String> = errors
        .iter()
        .filter(|e| e.starts_with("docs/B.md"))
        .collect();
    assert_eq!(
        of_b,
        [
            &refused("docs/B.md", 8, "gate"),
            &refused("docs/B.md", 15, "clean")
        ]
    );
}

/// Outside git the audit judges the documents alone.
#[test]
fn outside_git_the_symphonys_own_audit_judges_its_documents() {
    let tree = symphony();
    std::fs::remove_dir_all(tree.path(".git")).unwrap();
    assert_eq!(
        super::audit(&tree.root).errors,
        [refused("docs/A.md", 5, "gate")]
    );
}

/// Lists that do not read, or recipes init installs that are not there, are one error naming the
/// file, and nothing is judged by a rule it could not read.
#[test]
fn the_symphonys_own_audit_refuses_lists_it_cannot_read() {
    let tree = symphony();
    tree.write(LISTS, "prose the\nowned docs/A.md verify\n");
    assert_eq!(super::audit(&tree.root).errors, [unreadable(2)]);
    tree.write(LISTS, LISTED);
    std::fs::remove_file(tree.path(INSTALLED)).unwrap();
    let errors = super::audit(&tree.root).errors;
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with(&format!("{INSTALLED}: does not read: ")),
        "{errors:#?}"
    );
}
