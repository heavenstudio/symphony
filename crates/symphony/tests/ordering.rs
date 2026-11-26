//! Full output sequences at the inspection/reporting sorts that share stable ordering.
#[path = "common/mod.rs"]
mod common;

use common::Tree;
use std::path::Path;
use symphony::{Doc, plugins::Plugins, register::parse_decisions, triage};
use symphony_plugin::{Card, CardKind, Plugin};

fn git(root: &Path, args: &[&str]) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_owned()
}

#[test]
fn equal_time_versions_keep_descending_full_commit_ids_and_limits() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    assert!(symphony::versions::versions(&tree.root, "docs/A.md", 0).is_empty());
    let mut expected = Vec::new();
    for title in ["First", "Second", "Third"] {
        tree.write("docs/A.md", &format!("# {title}\n"));
        git(&tree.root, &["add", "docs/A.md"]);
        git(&tree.root, &["commit", "-qm", title]);
        let id = git(&tree.root, &["rev-parse", "HEAD"]);
        let short = git(&tree.root, &["rev-parse", "--short", "HEAD"]);
        expected.push((
            id,
            symphony_api::Version {
                sha: short,
                date: "2026-01-01".into(),
                subject: title.into(),
            },
        ));
    }
    symphony_board::order::sort_by(&mut expected, &|a, b| b.0.cmp(&a.0));
    let expected: Vec<_> = expected.into_iter().map(|(_, version)| version).collect();
    let actual = symphony::versions::versions(&tree.root, "docs/A.md", 0);
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(
        serde_json::to_value(symphony::versions::versions(&tree.root, "docs/A.md", 1)).unwrap(),
        serde_json::to_value(&expected[..1]).unwrap()
    );
}

#[test]
fn adjacent_removed_sections_keep_the_baseline_sequence_at_equal_insertion_positions() {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "-b", "main"]);
    let rel = "docs/A.md";
    tree.write(
        rel,
        "# First\n\nAlpha.\n\n# Second\n\nBeta.\n\n# Third\n\nGamma.\n",
    );
    git(&tree.root, &["add", rel]);
    git(&tree.root, &["commit", "-qm", "Baseline"]);
    let render = symphony::render::Render::new(&tree.root);
    let before = symphony::docview::document(&render, rel, "", &[]).unwrap();
    git(&tree.root, &["checkout", "-qb", "feature"]);
    let unchanged = symphony::docview::document(&render, rel, "feature", &[]).unwrap();
    assert!(unchanged.comparison.is_none());
    tree.write(rel, "");
    let after = symphony::docview::document(&render, rel, "feature", &[]).unwrap();
    let expected: Vec<_> = before
        .sections
        .iter()
        .map(|section| {
            let mut blocks = section.blocks.clone();
            // Existing comparison rendering prefixes read-only anchors so they cannot steal
            // the live document's anchors (comparison.rs, before the unchanged ordering).
            for block in &mut blocks {
                if let symphony_plugin::vocabulary::Block::Heading { anchor, .. } = block {
                    *anchor = format!("main-before-{anchor}");
                }
            }
            (
                format!("removed:{}", section.key),
                serde_json::to_value(&blocks).unwrap(),
            )
        })
        .collect();
    let actual: Vec<_> = after
        .sections
        .iter()
        .map(|section| {
            assert_eq!((section.start, section.end), (0, 0));
            assert_eq!(
                section.comparison.as_ref().unwrap().kind,
                symphony_api::Difference::Removed
            );
            (
                section.key.clone(),
                serde_json::to_value(&section.blocks).unwrap(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

struct Kinds;
static KINDS: Kinds = Kinds;
impl Plugin for Kinds {
    fn name(&self) -> &str {
        "ordering-fixture"
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        vec![self]
    }
}
impl CardKind for Kinds {
    fn name(&self) -> &str {
        "ordering-fixture"
    }
    fn recognises(&self, _: &Card<'_>) -> bool {
        true
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        !card.text.contains("signoff")
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        card.text.contains("picture").then(|| "picture".into())
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        Some(
            if card.text.contains("picture") {
                "picture"
            } else {
                "design"
            }
            .into(),
        )
    }
}

#[test]
fn triage_keeps_each_categories_full_sequence_on_equal_rank_and_time() {
    let plugins = Plugins::new(vec![&KINDS]);
    let mut text = String::new();
    for (kind, slug) in [
        ("picture", "p-z"),
        ("signoff", "s-z"),
        ("answer", "a-z"),
        ("design", "d-z"),
        ("picture", "p-a"),
        ("signoff", "s-a"),
        ("answer", "a-a"),
        ("design", "d-a"),
    ] {
        if kind == "answer" {
            text.push_str(&format!("- [?] **{slug}** — {kind}.\n  [2026-01-01 10:00] owner · question — Why?\n  [2026-01-01 10:01] agent — Answer.\n"));
        } else {
            text.push_str(&format!(
                "- [ ] **{slug}** — {kind}.\n  [2026-01-01 10:00] agent — Ready.\n"
            ));
        }
    }
    let doc = Doc {
        path: "docs/A.md".into(),
        comments: Vec::new(),
        decisions: parse_decisions(&text.lines().collect::<Vec<_>>()),
    };
    let expected: Vec<_> = [
        ("d-z", "design · gates nothing known"),
        ("d-a", "design · gates nothing known"),
        ("a-z", "your question, answered by an agent"),
        ("a-a", "your question, answered by an agent"),
        ("s-z", "design · only you sign it off"),
        ("s-a", "design · only you sign it off"),
        ("p-z", "picture · the triage left it for you"),
        ("p-a", "picture · the triage left it for you"),
    ]
    .into_iter()
    .map(|(slug, reason)| triage::Item {
        slug: slug.into(),
        doc: "docs/A.md".into(),
        reason: reason.into(),
        link: format!("http://local/docs/A.md#{slug}"),
    })
    .collect();
    let actual = triage::queue(
        &plugins,
        std::slice::from_ref(&doc),
        usize::MAX,
        "http://local",
    );
    assert_eq!(actual, expected);
    assert_eq!(
        triage::queue(&plugins, &[doc], 3, "http://local"),
        expected[..3]
    );
    assert!(triage::queue(&plugins, &[], usize::MAX, "").is_empty());
}

#[test]
fn shared_report_sorts_match_full_payloads_and_disclose_their_cost() {
    fn compare<T: Clone + std::fmt::Debug + PartialEq>(
        label: &str,
        mut rows: Vec<T>,
        compare: impl Fn(&T, &T) -> std::cmp::Ordering,
    ) {
        let mut expected = rows.clone();
        #[expect(
            clippy::disallowed_methods,
            reason = "the standard sort this test holds the shared sorter to"
        )]
        let (_, old_cpu) = test_time::cpu::spent(|| expected.sort_by(&compare));
        let calls = std::cell::Cell::new(0);
        let (_, new_cpu) = test_time::cpu::spent(|| {
            symphony_board::order::sort_by(&mut rows, &|a, b| {
                calls.set(calls.get() + 1);
                compare(a, b)
            });
        });
        assert_eq!(rows, expected, "{label}");
        assert!(calls.get() < rows.len().max(1) * 64, "{label}");
        eprintln!(
            "{label}: n={}, payload_bytes={}, position_bytes={}, comparisons={}, old_thread_cpu={old_cpu:?}, shared_thread_cpu={new_cpu:?}; usize-sort scratch in addition to positions, no payload clones inside the helper",
            rows.len(),
            std::mem::size_of::<T>(),
            rows.len() * std::mem::size_of::<usize>(),
            calls.get()
        );
    }
    let items: Vec<_> = (0..512)
        .rev()
        .map(|index| {
            (
                (index % 17).to_string(),
                triage::Item {
                    slug: index.to_string(),
                    doc: "docs/A.md".into(),
                    reason: "reason".into(),
                    link: "link".into(),
                },
            )
        })
        .collect();
    compare("triage ranked items", items, |a, b| a.0.cmp(&b.0));
    let sections: Vec<(usize, symphony_api::Section)> = (0..512)
        .rev()
        .map(|index| {
            (
                index % 17,
                serde_json::from_value(serde_json::json!({
                    "start":0,"end":1,"key":index.to_string(),"hash":"h","part":null,"blocks":[]
                }))
                .unwrap(),
            )
        })
        .collect();
    compare("removed sections", sections, |a, b| a.0.cmp(&b.0));
    let history: Vec<_> = (0..512)
        .rev()
        .map(|index| symphony_api::GatePast {
            at: f64::from(index % 17),
            by: Some(index.to_string()),
            detail: "history".into(),
            ..symphony_api::GatePast::default()
        })
        .collect();
    compare("gate past", history, |a, b| a.at.total_cmp(&b.at));
    let text = (0..512)
        .map(|index| format!("- [ ] **card-{index}** — Card.\n"))
        .collect::<String>();
    let decisions = parse_decisions(&text.lines().collect::<Vec<_>>());
    let ranks: std::collections::BTreeMap<_, _> = decisions
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(index, card)| (card.slug.clone(), index % 17))
        .collect();
    compare("canonical decisions", decisions, |a, b| {
        ranks
            .get(&a.slug)
            .cmp(&ranks.get(&b.slug))
            .then_with(|| a.slug.cmp(&b.slug))
    });
}

#[test]
fn cold_presentation_sorts_preserve_full_ties_and_report_cost() {
    fn compare<T: Clone + std::fmt::Debug + PartialEq>(
        label: &str,
        rows: Vec<T>,
        order: impl Fn(&T, &T) -> std::cmp::Ordering,
    ) {
        for count in [0, 1, rows.len()] {
            let mut old = rows[..count].to_vec();
            let mut shared = old.clone();
            #[expect(
                clippy::disallowed_methods,
                reason = "the standard sort this test holds the shared sorter to"
            )]
            let (_, old_time) = test_time::cpu::spent(|| old.sort_by(&order));
            let (_, shared_time) =
                test_time::cpu::spent(|| symphony_board::order::sort_by(&mut shared, &order));
            assert_eq!(shared, old, "{label}");
            eprintln!(
                "{label} rows={count}: old={old_time:?} shared={shared_time:?}, positions={} bytes",
                count * std::mem::size_of::<usize>()
            );
        }
    }
    let names = ["Alpha", "alpha", "Ä", "ä", "İ", "i\u{307}", "東京"];
    compare(
        "folder/media lowercase",
        (0..512)
            .map(|i| (names[i % names.len()].to_string(), i))
            .collect(),
        |a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    );
    compare(
        "branch names",
        (0..512)
            .map(|i| symphony::branches::Tree {
                branch: format!("branch-{}", i % 17),
                path: format!("/tree/{i}").into(),
            })
            .collect(),
        |a, b| a.branch.cmp(&b.branch),
    );
    let rank = |name: &str| match name {
        "settings" => 0,
        "rules" => 1,
        _ => 2,
    };
    compare(
        "instruction labels",
        (0..512)
            .map(|i| ["settings", "rules", "skill z", "skill A", "skill É"][i % 5].to_string())
            .collect(),
        |a, b| (rank(a), a).cmp(&(rank(b), b)),
    );
    compare(
        "decision dates",
        (0..512)
            .map(|i| {
                (
                    if i % 3 == 0 {
                        String::new()
                    } else {
                        format!("2026-09-{:02}", i % 28 + 1)
                    },
                    i,
                )
            })
            .collect(),
        |a, b| b.0.cmp(&a.0),
    );
}

#[test]
fn native_document_and_register_orders_keep_full_owner_content_and_configured_priority() {
    let tree = Tree::new();
    assert!(symphony::board::documents(&tree.root).unwrap().is_empty());
    assert!(symphony::board::register_files(&tree.root).is_empty());
    tree.write("symphony.toml", "[project]\nregister=\"docs/Ω.md\"\n");
    let mut expected = Vec::new();
    for (name, slug) in [("zeta", "z"), ("Alpha", "a"), ("Ω", "omega")] {
        let path = format!("docs/{name}.md");
        let text = format!(
            "# {name}\n\n<!-- register:start -->\n### Topic\n\n- [ ] **{slug}** — Exact owner question\n  [2026-09-25 09:00] owner — exact history {slug}\n<!-- register:end -->\n"
        );
        tree.write(&path, &text);
        let lines = symphony::text::splitlines(&text);
        expected.push(Doc {
            path,
            comments: symphony::callouts::parse_comments(&lines),
            decisions: parse_decisions(&lines),
        });
    }
    symphony_board::order::sort_by(&mut expected, &|a, b| a.path.cmp(&b.path));
    assert_eq!(symphony::board::documents(&tree.root).unwrap(), expected);
    // A cache hit must preserve every field and the same final route order.
    assert_eq!(symphony::board::documents(&tree.root).unwrap(), expected);
    assert_eq!(
        symphony::board::register_files(&tree.root),
        [
            tree.root.join("docs/Ω.md"),
            tree.root.join("docs/Alpha.md"),
            tree.root.join("docs/zeta.md")
        ]
    );
    let one = Tree::new();
    one.write("docs/Only.md", "# Only\n");
    assert_eq!(
        symphony::board::documents(&one.root).unwrap(),
        [Doc {
            path: "docs/Only.md".into(),
            comments: Vec::new(),
            decisions: Vec::new()
        }]
    );
    assert!(symphony::board::register_files(&one.root).is_empty());
}

#[test]
fn document_and_lossy_path_ties_match_standard_order_and_report_cost() {
    use std::os::unix::ffi::OsStringExt;
    let register = std::path::PathBuf::from("/docs/Ω.md");
    let fixtures = [
        std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/docs/a\x80.md".to_vec())),
        std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/docs/a\x81.md".to_vec())),
        register.clone(),
        "/docs/Éclair.md".into(),
    ];
    assert_eq!(fixtures[0].to_string_lossy(), fixtures[1].to_string_lossy());
    for count in [0, 1, 32, 512] {
        let mut paths: Vec<_> = (0..count)
            .map(|i| fixtures[i % fixtures.len()].clone())
            .collect();
        let mut old = paths.clone();
        let key =
            |path: &std::path::PathBuf| (*path != register, path.to_string_lossy().to_string());
        #[expect(
            clippy::disallowed_methods,
            reason = "the standard sort this test holds the shared sorter to"
        )]
        let (_, old_cpu) = test_time::cpu::spent(|| old.sort_by_key(&key));
        let (_, new_cpu) =
            test_time::cpu::spent(|| symphony_board::order::sort_by_key(&mut paths, &key));
        assert_eq!(paths, old);
        let mut docs: Vec<_> = (0..count)
            .map(|i| Doc {
                path: format!("docs/{}.md", i % 17),
                comments: Vec::new(),
                decisions: parse_decisions(&[&format!("- [ ] **card-{i}** — Exact payload")]),
            })
            .collect();
        let mut expected = docs.clone();
        #[expect(
            clippy::disallowed_methods,
            reason = "the standard sort this test holds the shared sorter to"
        )]
        let (_, doc_old_cpu) =
            test_time::cpu::spent(|| expected.sort_by(|a, b| a.path.cmp(&b.path)));
        let (_, doc_new_cpu) = test_time::cpu::spent(|| {
            symphony_board::order::sort_by(&mut docs, &|a, b| a.path.cmp(&b.path))
        });
        assert_eq!(docs, expected);
        eprintln!(
            "board n={count}: paths old={old_cpu:?}/shared={new_cpu:?}; Doc old={doc_old_cpu:?}/shared={doc_new_cpu:?}; positions={} bytes per call plus usize-sort scratch",
            count * std::mem::size_of::<usize>()
        );
    }
}
