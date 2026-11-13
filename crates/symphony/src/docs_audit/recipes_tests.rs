//! [`super`]: the rule of the recipes a text may name, and the docs audit's judging of documents by
//! it, in the Symphony's own checkout and never in a consuming project's.

use super::{
    ADVICE, CONSUMERS, FILED, OWN, PROSE, RULE, Rule, SHIPPED, Standing, installed, judged,
    named_in, recipes, symphony_checkout,
};
use crate::test_tree::Tree;

/// A rule of small lists: `tasks` installed, `land-approve` the consumer's, `the` prose, and
/// `docs/A.md` naming `verify` as its own and `check-fast` as filed.
fn rule(own: &'static [(&'static str, &'static str)]) -> Rule {
    Rule {
        installed: ["tasks".to_string()].into(),
        consumers: &["land-approve"],
        prose: &["the"],
        own,
        filed: &[("docs/A.md", "check-fast", "tools/check-fast")],
    }
}

const A_OWN: &[(&str, &str)] = &[("docs/A.md", "verify")];

fn refused(file: &str, line: usize, name: &str) -> String {
    format!(
        "{file}:{line}: names `just {name}`, a recipe a freshly initialised project lacks: {ADVICE}"
    )
}

#[test]
fn a_justfile_s_recipes_are_its_public_headers() {
    assert_eq!(
        recipes(
            "set x := 'y'\n_private := 'p'\n# just tasks: no\n[positional-arguments]\ntasks *ARGS:\n    \
             @echo\nbuild: fmt\nsymphony-cli ARG=\"a:b\":\nkey := \"v\"\n"
        ),
        ["build", "symphony-cli", "tasks"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn just_names_are_read_after_a_word_boundary_and_counted_by_line() {
    assert_eq!(
        named_in(
            "run `just tasks`\nthen just\n  agent-send x, adjust it, just_x y",
            3
        ),
        [(3, "tasks".to_string()), (5, "agent-send".to_string())]
    );
    assert_eq!(
        named_in("just-in-time, just \"quoted\", just {x}", 1),
        Vec::<(usize, String)>::new()
    );
}

/// The Symphony's rule reads the recipes this build installs, and its lists are the constants.
#[test]
fn the_symphonys_rule_is_the_installed_recipes_and_the_lists() {
    assert_eq!(installed(), &recipes(crate::init::SHIPPED_RECIPES));
    assert!(installed().contains("tasks") && installed().contains("land"));
    assert!(!installed().contains("lint-rs"), "{:?}", installed());
    assert_eq!(RULE.consumers, CONSUMERS);
    assert_eq!(RULE.prose, PROSE);
    assert_eq!(RULE.own, OWN);
    assert_eq!(RULE.filed, FILED);
}

/// A name stands by the first list that holds it, an own or filed one only in the file that list
/// names it for.
#[test]
fn a_name_stands_by_the_first_list_that_holds_it_and_an_own_one_only_in_its_file() {
    let rule = Rule {
        installed: ["tasks".to_string(), "verify".to_string()].into(),
        ..rule(&[("docs/A.md", "verify"), ("docs/A.md", "gate")])
    };
    assert_eq!(rule.standing("docs/B.md", "tasks"), Standing::Installed);
    assert_eq!(rule.standing("docs/A.md", "verify"), Standing::Installed);
    assert_eq!(
        rule.standing("docs/B.md", "land-approve"),
        Standing::Consumer
    );
    assert_eq!(rule.standing("docs/B.md", "the"), Standing::Prose);
    assert_eq!(rule.standing("docs/A.md", "gate"), Standing::Own(1));
    assert_eq!(rule.standing("docs/B.md", "gate"), Standing::Unknown);
    assert_eq!(rule.standing("docs/A.md", "check-fast"), Standing::Filed(0));
    assert_eq!(rule.standing("docs/B.md", "check-fast"), Standing::Unknown);
    assert_eq!(rule.standing("docs/A.md", "deploy"), Standing::Unknown);
}

#[test]
fn the_audit_judges_the_markdown_under_docs_but_the_records() {
    for file in [
        "docs/CHECKS.md",
        "docs/sub/GUIDE.md",
        "docs/analysis.md",
        "docs/historyx.md",
    ] {
        assert!(judged(file), "{file}");
    }
    for file in [
        "docs/DECISIONS.md",
        "docs/analysis/one.md",
        "docs/history/README.md",
        "docs/history/deep/old.md",
        "docs/picture.png",
        "docs/CHECKS.md.orig",
        "README.md",
        "AGENTS.md",
        "crates/symphony/shipped/skills/work/SKILL.md",
        "other/docs/A.md",
    ] {
        assert!(!judged(file), "{file}");
    }
}

#[test]
fn the_symphonys_own_checkout_holds_the_recipes_init_installs() {
    let tree = Tree::new();
    assert!(!symphony_checkout(&tree.root));
    std::fs::create_dir_all(tree.path(SHIPPED)).unwrap();
    assert!(!symphony_checkout(&tree.root), "a folder of that name");
    std::fs::remove_dir(tree.path(SHIPPED)).unwrap();
    tree.write(SHIPPED, "tasks *ARGS:\n");
    assert!(symphony_checkout(&tree.root));
}

/// Each name none of the lists allows is refused by its file and line, in a code block or across a
/// line break too; the records and what is not under `docs/` are not judged.
#[test]
fn a_document_naming_a_recipe_no_list_allows_is_refused_by_its_file_and_line() {
    let a = "Run `just tasks`.\n\n```\njust gate\n```\nThen just the docs, `just land-approve`, \
             just\n  verify and `just check-fast` [[task:tools/check-fast]].\n";
    let b = "# B\n\n`just verify` is A's own, not B's.\n";
    let records = "`just gate`\n";
    let errors = rule(A_OWN).in_documents([
        ("docs/A.md", a),
        ("docs/B.md", b),
        ("docs/DECISIONS.md", records),
        ("docs/analysis/x.md", records),
        ("docs/history/x.md", records),
        ("README.md", records),
    ]);
    assert_eq!(
        errors,
        [
            refused("docs/A.md", 4, "gate"),
            refused("docs/B.md", 3, "verify")
        ]
    );
}

/// An own or filed entry for a judged document is refused once the document no longer names it, or
/// is not there; one for a file the audit does not judge is the hooks test's.
#[test]
fn an_own_or_filed_entry_a_document_no_longer_names_is_refused() {
    let errors = Rule {
        filed: &[
            ("docs/A.md", "check-fast", "tools/check-fast"),
            ("crates/x/src/lib.rs", "later", "tools/later"),
        ],
        ..rule(&[
            ("docs/A.md", "verify"),
            ("docs/Gone.md", "verify"),
            ("crates/x/src/lib.rs", "nightly"),
            ("docs/DECISIONS.md", "nightly"),
        ])
    }
    .in_documents([("docs/A.md", "Nothing to run.\n")]);
    let lists = "crates/symphony/src/docs_audit/recipes.rs";
    assert_eq!(
        errors,
        [
            format!(
                "docs/A.md: names no `just verify`, which OWN lists as its own: take that entry out \
                 of OWN, in {lists}"
            ),
            format!(
                "docs/Gone.md: names no `just verify`, which OWN lists as its own: take that entry \
                 out of OWN, in {lists}"
            ),
            format!(
                "docs/A.md: names no `just check-fast`, which FILED lists as filed to be made: take \
                 that entry out of FILED, in {lists}"
            ),
        ]
    );
}

/// A filed recipe is named, in code, on a line that names the task filed to make it.
#[test]
fn a_filed_recipe_is_named_beside_the_task_filed_to_make_it() {
    let beside = |text: &str| rule(&[]).in_documents([("docs/A.md", text)]);
    assert_eq!(
        beside("Its recipe, `just check-fast`, is [[task:tools/check-fast]].\n"),
        Vec::<String>::new()
    );
    let apart = "Intro.\nIts recipe, `just check-fast`, is\nfiled as [[task:tools/check-fast]].\n";
    let uncoded = "Its recipe, just check-fast, is [[task:tools/check-fast]].\n";
    let other = "Its recipe, `just check-fast`, is [[task:tools/other]].\n";
    for (text, line) in [(apart, 2), (uncoded, 1), (other, 1)] {
        assert_eq!(
            beside(text),
            [format!(
                "docs/A.md:{line}: names `just check-fast`, which this repository's justfile does \
                 not have yet, on no line beside the task filed to make it: write \
                 [[task:tools/check-fast]] on the line that names `just check-fast`"
            )],
            "{text}"
        );
    }
}

/// A project shaped as a consumer: its justfile importing the Symphony's recipes beside its own,
/// and a guide naming both.
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
    tree.write(
        "docs/README.md",
        "# Lamp shop\n\nRun `just gate` before `just deploy`, and `just tasks list` for the board.\n",
    );
    tree.write("docs/analysis/old.md", "# Old\n\nWe ran `just gate`.\n");
    tree
}

fn audited(root: &std::path::Path) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = [
        "--check".to_string(),
        "--root".into(),
        root.display().to_string(),
    ];
    let code = super::super::main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// A consuming project's documents name its own recipes: its audit judges none of them.
#[test]
fn a_consuming_projects_audit_judges_no_recipe_its_documents_name() {
    let tree = consumer();
    assert!(!symphony_checkout(&tree.root));
    assert_eq!(super::super::audit(&tree.root).errors, Vec::<String>::new());
    let (code, out, err) = audited(&tree.root);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.ends_with(
            "docs audit: every document renders, and every card waiting on the owner can be answered\n"
        ),
        "{out}"
    );
}

/// The same documents in the Symphony's own checkout are judged by its rule: each recipe init does
/// not install is refused with its file and line, the records aside, and the rule's own entries for
/// documents this checkout lacks are refused too.
#[test]
fn the_symphonys_own_audit_refuses_a_recipe_init_does_not_install() {
    let tree = consumer();
    tree.write(SHIPPED, crate::init::SHIPPED_RECIPES);
    let errors = super::super::audit(&tree.root).errors;
    let guide: Vec<&String> = errors
        .iter()
        .filter(|error| error.starts_with("docs/README.md"))
        .collect();
    assert_eq!(
        guide,
        [
            &refused("docs/README.md", 3, "gate"),
            &refused("docs/README.md", 3, "deploy"),
        ]
    );
    assert!(
        !errors.iter().any(|error| error.contains("analysis")),
        "{errors:#?}"
    );
    for (file, name) in OWN.iter().filter(|(file, _)| judged(file)) {
        let stale = format!("{file}: names no `just {name}`, which OWN lists as its own");
        assert!(
            errors.iter().any(|error| error.starts_with(&stale)),
            "{stale}: {errors:#?}"
        );
    }
    let (code, _, err) = audited(&tree.root);
    assert_eq!(code, 1);
    assert!(
        err.starts_with(&format!("✖ {}\n", refused("docs/README.md", 3, "gate"))),
        "{err}"
    );
}
