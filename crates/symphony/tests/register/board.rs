//! ScanTest, RenderTest and DecisionBoardTest: the tree read, the board printed, the registers tidied.

use serde_json::Value;
use symphony::board::{scan, tidy_with};
use symphony::plugins::Plugins;
use symphony::register::ReviewError;

use crate::common::{Tree, after_open, matches, rest, status, status_ok};

// ScanTest

#[test]
fn scans_markdown_under_docs_sorted() {
    let tree = Tree::new();
    tree.write("docs/b.md", "---\nstatus: approved\n---\n");
    tree.write("docs/a/inner.md", "> [!question] why\n");
    tree.write("docs/notes.txt", "> [!question] not markdown\n");
    tree.write("sim/thing.md", "> [!question] outside docs\n");
    let docs = scan(&tree.root).unwrap();
    // A document has versions and waiting decisions, not a status: `Doc` holds no such field.
    assert_eq!(
        docs.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(),
        vec!["docs/a/inner.md", "docs/b.md"]
    );
}

#[test]
fn history_is_skipped() {
    let tree = Tree::new();
    tree.write("docs/history/old.md", "> [!dislike] archived\n");
    tree.write("docs/live.md", "");
    let docs = scan(&tree.root).unwrap();
    assert_eq!(
        docs.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(),
        vec!["docs/live.md"]
    );
}

#[test]
fn missing_docs_dir_is_an_error() {
    let tree = Tree::new();
    assert!(scan(&tree.root).is_err());
}

// RenderTest

fn render_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "docs/VISION.md",
        "---\nstatus: review\n---\n# V\n\n> [!dislike] Too grim\n> Lighten the tone.\n",
    );
    tree.write(
        "docs/MAKERS.md",
        "---\nstatus: approved\n---\n> [!like] East\n",
    );
    tree.write("docs/PLAN.md", "# plan\n");
    tree.write("docs/BAD.md", "---\nstatus: shipped\n---\n");
    tree.write("docs/DONE.md", "> [!decision] Do X\n> > [!resolved] done\n");
    tree
}

#[test]
fn board_lists_open_comments_and_has_no_status_section() {
    let tree = render_tree();
    let (code, out, _) = status(&tree.root, &[]);
    assert_eq!(code, 0);
    for gone in ["STATUS", "NO STATUS", "INVALID", "(none)", "shipped"] {
        assert!(!out.contains(gone), "{gone} in {out}");
    }
    assert!(out.contains("docs/VISION.md:6"), "{out}");
    assert!(out.contains("Too grim"));
    assert!(out.contains("Lighten the tone."));
    assert!(
        !after_open(&out).contains("East"),
        "a like is not open work"
    );
    assert!(!after_open(&out).contains("docs/DONE.md"));
}

#[test]
fn the_board_does_not_open_with_a_blank_line() {
    let tree = render_tree();
    let (_, out, _) = status(&tree.root, &[]);
    assert!(
        out.starts_with("OPEN COMMENTS") || out.starts_with("DECISIONS"),
        "{out}"
    );
}

#[test]
fn json_output_is_machine_readable() {
    let tree = render_tree();
    let (code, out, _) = status(&tree.root, &["--json"]);
    assert_eq!(code, 0);
    let data: Value = serde_json::from_str(&out).unwrap();
    let doc = |path: &str| {
        data["docs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["path"] == path)
            .unwrap()
            .clone()
    };
    let vision = doc("docs/VISION.md");
    assert!(vision.get("status").is_none());
    assert!(vision.get("status_valid").is_none());
    assert_eq!(vision["comments"][0]["kind"], "dislike");
    assert_eq!(vision["comments"][0]["open"], true);
    assert_eq!(doc("docs/DONE.md")["comments"][0]["open"], false);
    assert_eq!(data["open"], 1);
}

#[test]
fn open_only_prints_just_open_comments() {
    let tree = render_tree();
    let (_, out, _) = status(&tree.root, &["--open"]);
    assert!(out.contains("docs/VISION.md:6"), "{out}");
    assert!(!out.contains("docs/MAKERS.md"));
    assert!(
        !out.lines()
            .next()
            .unwrap_or("")
            .to_lowercase()
            .contains("status")
    );
}

#[test]
fn missing_root_fails_loudly() {
    let tree = render_tree();
    let (code, _, err) = status(&tree.path("nope"), &[]);
    assert_eq!(code, 2);
    assert!(err.contains("docs"), "{err}");
}

// DecisionBoardTest

fn decision_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "docs/DECISIONS.md",
        "---\nstatus: review\n---\n# Decisions\n\n\
         - [x] **five-workshops** — There are five workshops.\n\
         - [ ] **star-lanes** — Travel is by star lanes.\n\
         - [-] **lamp-count-141** — 141 lamps.\n  Thirty first.\n\
         - [?] **five-station-bands** — Five size bands.\n",
    );
    tree
}

#[test]
fn board_counts_decisions_and_lists_open_ones() {
    let tree = decision_tree();
    let out = status_ok(&tree.root, &[]);
    assert!(out.contains("DECISIONS"));
    assert!(matches(r"approved\s+1", &out), "{out}");
    assert!(matches(r"pending\s+1", &out), "{out}");
    let open = after_open(&out);
    assert!(open.contains("docs/DECISIONS.md:8"), "{out}");
    assert!(open.contains("lamp-count-141"));
    assert!(open.contains("Thirty first."));
    assert!(open.contains("five-station-bands"));
    assert!(!open.contains("five-workshops"));
    assert!(!open.contains("star-lanes"));
}

#[test]
fn open_only_includes_decisions() {
    let tree = decision_tree();
    let out = status_ok(&tree.root, &["--open"]);
    assert!(out.contains("lamp-count-141"), "{out}");
    assert!(!out.contains("DECISIONS ("));
}

#[test]
fn board_shows_how_many_wait_on_the_owner() {
    let tree = decision_tree();
    let out = status_ok(&tree.root, &[]);
    assert!(matches(r"needs your decision\s+1", &out), "{out}");
    assert!(matches(r"waiting on an agent\s+2", &out), "{out}");
    assert!(matches(r"resolved\s+1", &out), "{out}");
}

#[test]
fn tidy_resolves_and_sorts_the_register_in_place() {
    let tree = decision_tree();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n- [ ] **new** — proposal.\n<!-- register:end -->\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let text = tree.read("docs/DECISIONS.md");
    assert!(out.contains("resolved: done"), "{out}");
    assert!(
        text.find("**new**").unwrap() < text.find("**done**").unwrap(),
        "{text}"
    );
    assert!(
        text.contains("- [r] **done** — plain.\n  [2026-09-26 08:00] agent · resolved"),
        "{text}"
    );
}

fn drop_the_table(text: &str, _: &Plugins) -> Result<String, ReviewError> {
    Ok(text.replace("    | t |\n", ""))
}

#[test]
fn tidy_refuses_to_write_a_register_that_would_lose_a_card_line() {
    let tree = decision_tree();
    let text = "# D\n\n<!-- register:start -->\n### T\n\n- [ ] **new** — proposal.\n  [2026-09-26 08:00] agent — one\n\n    | t |\n<!-- register:end -->\n";
    tree.write("docs/DECISIONS.md", text);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tidy_with(
        &tree.root,
        Some("2026-09-26 08:00"),
        drop_the_table,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 1);
    assert!(String::from_utf8(err).unwrap().contains("new"));
    assert_eq!(tree.read("docs/DECISIONS.md"), text);
}

#[test]
fn a_tidy_that_cannot_tidy_one_file_writes_none_of_them() {
    // The whole tidy is worked out before a file is written: the register it reaches first, and
    // the comment it would archive, stay as they were when a later register cannot be tidied.
    let tree = decision_tree();
    let first =
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
    tree.write("docs/DECISIONS.md", first);
    let later = "# L\n\n<!-- register:start -->\n### T\n\n- [ ] **new** — proposal.\n  [2026-09-26 08:00] agent — one\n\n    | t |\n<!-- register:end -->\n";
    tree.write("docs/LATER.md", later);
    let notes = "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n";
    tree.write("docs/NOTES.md", notes);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tidy_with(
        &tree.root,
        Some("2026-09-26 08:00"),
        drop_the_table,
        &mut out,
        &mut err,
    );
    let err = String::from_utf8(err).unwrap();
    assert_eq!(code, 1);
    assert!(err.starts_with("✖ docs/LATER.md not tidied: "), "{err}");
    assert_eq!(String::from_utf8(out).unwrap(), "");
    assert_eq!(tree.read("docs/DECISIONS.md"), first);
    assert_eq!(tree.read("docs/LATER.md"), later);
    assert_eq!(tree.read("docs/NOTES.md"), notes);
    assert_eq!(tree.names("docs"), ["DECISIONS.md", "LATER.md", "NOTES.md"]);
}

#[test]
fn a_tidy_that_cannot_write_a_file_names_it_and_says_nothing_of_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let tree = decision_tree();
    let register =
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
    tree.write("docs/DECISIONS.md", register);
    // A folder nothing can be written in: the file beside the register cannot be made.
    let docs = tree.path("docs");
    std::fs::set_permissions(&docs, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, out, err) = status(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    std::fs::set_permissions(&docs, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "");
    assert!(err.starts_with("✖ docs/DECISIONS.md not tidied: "), "{err}");
    assert_eq!(tree.read("docs/DECISIONS.md"), register);
}

#[test]
fn a_register_holding_an_answered_comment_is_tidied_and_archived_in_one_pass() {
    // The archive reads the register as its tidy left it, so the file takes both.
    let tree = decision_tree();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    assert_eq!(
        out,
        "resolved: done\nregrouped docs/DECISIONS.md\narchived: docs/DECISIONS.md (1)\n"
    );
    let text = tree.read("docs/DECISIONS.md");
    assert!(!text.contains("[!comment] Odd"), "{text}");
    assert!(
        text.contains("- [r] **done** — plain.\n  [2026-09-26 08:00] agent · resolved"),
        "{text}"
    );
    let ledger = tree.read("docs/history/review-ledger.md");
    assert!(
        ledger.contains("## 2026-09-26 08:00 · docs/DECISIONS.md"),
        "{ledger}"
    );
}

#[test]
fn tidy_moves_answered_callouts_into_the_ledger() {
    let tree = decision_tree();
    tree.write(
        "docs/NOTES.md",
        "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n\nrest\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    assert_eq!(tree.read("docs/NOTES.md"), "# N\n\nrest\n");
    let ledger = tree.read("docs/history/review-ledger.md");
    assert!(
        ledger.contains("## 2026-09-26 08:00 · docs/NOTES.md"),
        "{ledger}"
    );
    assert!(
        ledger.contains("> [!comment] Odd\n> > [!resolved] Fixed."),
        "{ledger}"
    );
    assert!(out.contains("archived: docs/NOTES.md (1)"), "{out}");
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 09:00"]);
    assert_eq!(tree.read("docs/history/review-ledger.md"), ledger);
}

#[test]
fn a_tidy_replaces_each_file_whole_so_a_reader_never_sees_it_cut_short() {
    // Through a file beside it and a rename: a reader that opened a register or a page before the
    // tidy reads the whole old text, never a truncated one, and the file keeps its permissions.
    let tree = decision_tree();
    let register =
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n";
    tree.write("docs/DECISIONS.md", register);
    let notes = "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n";
    tree.write("docs/NOTES.md", notes);
    let open_register = tree.open_with_mode("docs/DECISIONS.md", 0o640);
    let open_notes = tree.open_with_mode("docs/NOTES.md", 0o600);
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    assert_eq!(rest(open_register), register);
    assert_eq!(rest(open_notes), notes);
    assert!(tree.read("docs/DECISIONS.md").contains("- [r] **done**"));
    assert_eq!(tree.read("docs/NOTES.md"), "# N\n");
    assert_eq!(
        (tree.mode("docs/DECISIONS.md"), tree.mode("docs/NOTES.md")),
        (0o640, 0o600)
    );
    assert_eq!(tree.names("docs"), ["DECISIONS.md", "NOTES.md", "history"]);
    assert_eq!(tree.names("docs/history"), ["review-ledger.md"]);
}

#[test]
fn a_tidy_of_a_linked_register_writes_the_file_it_links_to_and_keeps_the_link() {
    let tree = decision_tree();
    let real = tree.write(
        "kept/register.md",
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n",
    );
    std::fs::remove_file(tree.path("docs/DECISIONS.md")).unwrap();
    std::os::unix::fs::symlink(&real, tree.path("docs/DECISIONS.md")).unwrap();
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let link = std::fs::symlink_metadata(tree.path("docs/DECISIONS.md")).unwrap();
    assert!(
        link.file_type().is_symlink(),
        "the link became a plain file"
    );
    assert!(tree.read("kept/register.md").contains("- [r] **done**"));
    assert_eq!(tree.names("kept"), ["register.md"]);
}

#[test]
fn a_write_that_cannot_land_leaves_nothing_beside_its_file() {
    // A folder where the file would go: the rename over it fails, and the file written beside it goes.
    let tree = Tree::new();
    tree.write("docs/place/inside.md", "x\n");
    let refused = symphony::text::write_whole(&tree.path("docs/place"), "text\n");
    assert!(refused.is_err());
    assert_eq!(tree.names("docs"), ["place"]);
    assert_eq!(tree.read("docs/place/inside.md"), "x\n");
}

#[test]
fn a_later_archive_is_added_under_the_ledgers_earlier_entries() {
    let tree = decision_tree();
    tree.write("docs/A.md", "> [!comment] First\n> > [!resolved] Done.\n");
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    tree.write("docs/B.md", "> [!comment] Second\n> > [!resolved] Done.\n");
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 09:00"]);
    let ledger = tree.read("docs/history/review-ledger.md");
    let first = ledger.find("## 2026-09-26 08:00 · docs/A.md").unwrap();
    let second = ledger.find("## 2026-09-26 09:00 · docs/B.md").unwrap();
    assert!(first < second, "{ledger}");
    assert_eq!(ledger.matches("[!comment] First").count(), 1, "{ledger}");
}

#[test]
fn tidy_without_a_docs_folder_is_exit_two_naming_the_root() {
    let tree = Tree::new();
    let (code, out, err) = status(&tree.root, &["--tidy"]);
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.starts_with("✖ no docs/ directory under "), "{err}");
}

#[test]
fn tidy_takes_the_decisions_register_before_the_others() {
    let tree = Tree::new();
    let region = |slug: &str| {
        format!(
            "# R\n\n<!-- register:start -->\n### T\n\n- [x] **{slug}** — done.\n<!-- register:end -->\n"
        )
    };
    tree.write("docs/A.md", &region("from-a"));
    tree.write("docs/DECISIONS.md", &region("from-decisions"));
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let decisions = out.find("regrouped docs/DECISIONS.md").unwrap();
    let other = out.find("regrouped docs/A.md").unwrap();
    assert!(decisions < other, "{out}");
}

#[test]
fn tidy_leaves_a_maker_pages_picture_slot_to_the_app() {
    let tree = decision_tree();
    tree.write(
        "docs/makers/x.md",
        "# X\n\n### Scouts\n\n[[picture:Scouts · lamp]]\n\n<!-- register:start -->\n### Workshop picture\n\n\
         - [x] **scout-lamp** — A scout. *Fills:* Scouts · lamp\n  [2026-09-26 07:00] agent — ![s](/s.png)\n\
         \x20 [2026-09-26 07:30] owner · approved\n<!-- register:end -->\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let page = tree.read("docs/makers/x.md");
    assert!(page.contains("\n[[picture:Scouts · lamp]]\n"), "{page}");
    assert!(!page.contains("<figure"));
    assert!(!out.contains("pictures:"));
}

#[test]
fn tidy_covers_every_document_holding_cards_under_docs() {
    let tree = decision_tree();
    tree.write(
        "docs/pictures/lamps.md",
        "# Lamps\n\n<!-- register:start -->\n### North\n\n- [x] **rep-a** — one image.\n- [ ] **rep-b** — another.\n<!-- register:end -->\n",
    );
    let plain = "# Notes\n\n- [x] **not-a-register** — outside any region.\n";
    tree.write("docs/notes.md", plain);
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let text = tree.read("docs/pictures/lamps.md");
    assert!(out.contains("rep-a"), "{out}");
    assert!(out.contains("regrouped docs/pictures/lamps.md"), "{out}");
    assert!(
        text.find("**rep-b**").unwrap() < text.find("**rep-a**").unwrap(),
        "{text}"
    );
    assert!(text.contains("- [r] **rep-a**"), "{text}");
    // A card outside any region is inline: resolved where it sits, nothing around it moved.
    assert!(out.contains("tidied docs/notes.md"), "{out}");
    assert_eq!(
        tree.read("docs/notes.md"),
        "# Notes\n\n- [r] **not-a-register** — outside any region.\n  [2026-09-26 08:00] agent · resolved\n"
    );
}

#[test]
fn json_carries_decisions() {
    let tree = decision_tree();
    let data: Value = serde_json::from_str(&status_ok(&tree.root, &["--json"])).unwrap();
    let doc = data["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "docs/DECISIONS.md")
        .unwrap();
    let field = |name: &str| {
        doc["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d[name].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        field("state"),
        vec!["approved", "pending", "rejected", "question"]
    );
    assert_eq!(
        doc["decisions"][2]["note"],
        serde_json::json!(["Thirty first."])
    );
    assert_eq!(field("group"), vec!["resolved", "decide", "agent", "agent"]);
    assert_eq!(
        doc["decisions"][2]["entries"],
        serde_json::json!([{"ts": null, "who": "owner", "verdict": null, "text": "Thirty first."}])
    );
    assert_eq!(data["open"], 2);
}
