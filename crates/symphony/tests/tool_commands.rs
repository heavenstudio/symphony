//! The process contracts a project's art and landing tools read. Fixtures use only scratch
//! repositories; no live board or registered project's state is opened.

mod common;

use std::fs;
use std::path::Path;

use common::{Tree, cli};
use serde_json::{Value, json};
use symphony_board::sys::{Stdio, git as git_command, output, wait_output};

const PAGE: &str = "# Cards\n\n- [ ] **pick** — Choose a shape.\n";

fn call(tree: &Tree, command: &str, args: &[&str]) -> (i32, String, String) {
    let root = tree.root.to_str().unwrap();
    let mut argv = vec![command, "--root", root];
    argv.extend(args);
    cli(&argv)
}

fn answer(tree: &Tree, command: &str, args: &[&str]) -> Value {
    let (code, out, err) = call(tree, command, args);
    assert_eq!((code, err.as_str()), (0, ""), "{out}\n{err}");
    assert_eq!(out.lines().count(), 1, "JSON answers are one line: {out}");
    serde_json::from_str(&out).unwrap()
}

fn git(root: &Path, args: &[&str]) -> String {
    let done = output(git_command().args(args).current_dir(root)).unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().into()
}

fn commit(tree: &Tree) -> String {
    git(&tree.root, &["add", "."]);
    git(&tree.root, &["commit", "-qm", "fixture"]);
    git(&tree.root, &["rev-parse", "HEAD"])
}

fn repository() -> Tree {
    let tree = Tree::new();
    git(&tree.root, &["init", "-q", "--initial-branch=main"]);
    git(&tree.root, &["config", "user.name", "Fixture"]);
    git(
        &tree.root,
        &["config", "user.email", "fixture@example.test"],
    );
    tree.write("docs/CARDS.md", PAGE);
    commit(&tree);
    tree
}

#[test]
fn new_commands_have_help_and_refuse_bad_arguments_without_json() {
    for command in ["render-card", "landing-checks"] {
        for flag in ["--help", "-h"] {
            let (code, out, err) = cli(&[command, flag]);
            assert_eq!((code, err.as_str()), (0, ""));
            assert!(out.starts_with(&format!("usage: symphony {command} ")));
        }
        for args in [vec![], vec!["--unknown"], vec!["--root"], vec!["--base"]] {
            let mut argv = vec![command];
            argv.extend(args);
            let (code, out, err) = cli(&argv);
            assert_eq!((code, out.as_str()), (2, ""));
            assert!(err.starts_with(&format!("usage: symphony {command} ")));
        }
    }
    assert_eq!(cli(&["render-card", "a", "b", "extra"]).0, 2);
}

#[test]
fn render_card_contract_is_one_body_and_one_thread_of_typed_blocks() {
    let tree = Tree::new();
    tree.write("docs/CARDS.md", PAGE);
    let value = answer(&tree, "render-card", &["docs/CARDS.md", "pick"]);
    // Hold the envelope and every field of the card block, independently of the page pipeline.
    assert_eq!(
        value,
        json!({
            "path": "docs/CARDS.md", "slug": "pick", "line": 3,
            "body": {"blocks": [{
                "type": "card", "slug": "pick", "mark": "pending",
                "title": "Choose a shape.",
                "body": [{"type": "paragraph", "spans": [{"type": "text", "text": "Choose a shape."}]}],
                "verdict": {"group": "decide", "open": false, "posted": null, "decided": null},
                "thread": [], "line": null, "line_offset": 0, "comparison": []
            }]},
            "thread": {"blocks": []}
        })
    );
}

#[test]
fn render_card_uses_route_bounds_and_keeps_only_the_named_card() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/guide/\"\nfolder = \"pages\"\nexclude = [\"private.md\"]\n",
    );
    let page = format!("{PAGE}\n- [ ] **other** — Do not draw me.\n");
    tree.write("pages/CARDS.md", &page);
    tree.write("pages/private.md", PAGE);
    let value = answer(&tree, "render-card", &["guide/CARDS.md", "pick"]);
    assert!(!value.to_string().contains("Do not draw me"));
    assert_eq!(value["line"], 3);
    for (path, slug) in [
        ("guide/CARDS.md", "missing"),
        ("guide/private.md", "pick"),
        ("pages/CARDS.md", "pick"),
        ("guide/../pages/CARDS.md", "pick"),
        ("/guide/CARDS.md", "pick"),
    ] {
        let (code, out, err) = call(&tree, "render-card", &[path, slug]);
        assert_eq!((code, out.as_str()), (1, ""), "{err}");
        assert!(!err.is_empty());
    }
    tree.write("pages/CARDS.md", &format!("{PAGE}\n{PAGE}"));
    assert!(
        call(&tree, "render-card", &["guide/CARDS.md", "pick"])
            .2
            .contains("more than one card")
    );
}

#[test]
fn render_card_loads_a_process_plugin_and_matches_the_pages_body_and_history() {
    let tree = Tree::new();
    let plugin = env!("CARGO_BIN_EXE_symphony-test-plugin");
    tree.write(
        "symphony.toml",
        &format!("[plugins]\ncrates = [\"every\"]\n[plugins.process]\nevery = [{plugin:?}]\n"),
    );
    tree.write("docs/CARDS.md", "# Cards\n\n- [ ] **pick** — Choose every shape.\n  [[every:shape|front]]\n  [2026-11-13 12:00] owner — Can it be round?\n  [2026-11-13 12:01] agent — Here is [[again:reply]].\n\n- [ ] **other** — Keep this out.\n");
    let value = answer(&tree, "render-card", &["docs/CARDS.md", "pick"]);
    let render = symphony::render::Render::new(&tree.root);
    let page = symphony::docview::document(&render, "docs/CARDS.md", "", &[]).unwrap();
    let section = page
        .sections
        .iter()
        .find(|section| section.key == "card:pick")
        .unwrap();
    assert_eq!(
        value["body"]["blocks"],
        serde_json::to_value(&section.blocks).unwrap()
    );
    assert_eq!(
        value["thread"],
        serde_json::to_value(&page.threads["pick"]).unwrap()
    );
    let text = value.to_string();
    assert!(
        text.contains("shape Some(\\\"front\\\") in artifacts/every"),
        "{text}"
    );
    assert!(
        text.contains("Can it be round?") && text.contains("reply"),
        "{text}"
    );
    assert!(!text.contains("Keep this out"));
}

#[test]
fn render_card_refuses_broken_settings_or_an_unavailable_plugin() {
    let tree = Tree::new();
    tree.write("docs/CARDS.md", PAGE);
    for settings in [
        "broken = [",
        "[plugins]\ncrates = [\"missing\"]\n",
        "[plugins]\ncrates = [\"missing\"]\n[plugins.process]\nmissing = [\"/no/such/plugin\"]\n",
    ] {
        tree.write("symphony.toml", settings);
        let (code, out, err) = call(&tree, "render-card", &["docs/CARDS.md", "pick"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("symphony.toml"), "{err}");
    }
}

#[test]
fn unreadable_document_text_and_a_missing_checkout_fail_without_success_json() {
    let tree = Tree::new();
    let page = tree.write("docs/CARDS.md", PAGE);
    fs::write(page, b"not UTF-8: \xff").unwrap();
    let (code, out, err) = call(&tree, "render-card", &["docs/CARDS.md", "pick"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("symphony render-card:"), "{err}");

    let missing = Tree {
        root: tree.path("missing"),
    };
    let (code, out, err) = call(&missing, "landing-checks", &["main"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("symphony landing-checks:"), "{err}");
}

#[test]
fn status_json_contract_holds_cards_comments_and_their_history() {
    let tree = Tree::new();
    tree.write("docs/CARDS.md", &format!("{PAGE}  [2026-11-13 12:00] owner — More choices.\n\n> [!comment] A note\n> Please clarify.\n"));
    let (code, out, err) = call(&tree, "status", &["--json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({
            "docs": [{
                "path": "docs/CARDS.md",
                "decisions": [{
                    "line": 3, "slug": "pick", "state": "pending", "text": "Choose a shape.",
                    "note": ["[2026-11-13 12:00] owner — More choices."], "open": true, "group": "agent", "topic": "General",
                    "decided_at": null,
                    "entries": [{"ts": "2026-11-13 12:00", "who": "owner", "verdict": null, "text": "More choices."}]
                }],
                "comments": [{"line": 6, "kind": "comment", "title": "A note", "body": ["Please clarify."], "resolution": null, "open": true}]
            }], "open": 2
        })
    );
    assert_eq!(call(&tree, "status", &["--open"]), (0,
        "docs/CARDS.md:3  pending  **pick** Choose a shape.\n    More choices.\ndocs/CARDS.md:6  comment  A note\n    Please clarify.\n".into(), "".into()));
}

#[test]
fn triage_contract_holds_stats_and_nonempty_queue_text() {
    let tree = Tree::new();
    tree.write("docs/CARDS.md", PAGE);
    let (code, stats, err) = call(&tree, "triage", &["stats"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        stats,
        "FIRST-PASS APPROVAL (the owner's first verdict was an approval)\n\n  first post  0/0        -\n  redraw      0/0        -\n\napprovals with a note: 0/0 - (a note is still work)\npicture letters: none\ndesign letters: none\nanswered with a question: 0/0 -\n"
    );
    let (code, queue, err) = call(&tree, "triage", &["queue", "5"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        queue,
        " 1. pick  docs/CARDS.md\n    design · gates nothing known\n    http://127.0.0.1:8765/docs/CARDS.md#pick\n"
    );
    assert_eq!(
        call(&tree, "triage", &["queue", "0"]),
        (0, "nothing waiting on the owner\n".into(), "".into())
    );
}

#[test]
fn landing_contract_classifies_changed_and_extra_paths_and_reports_owner_lines_and_lock() {
    let tree = repository();
    let base = git(&tree.root, &["rev-parse", "HEAD"]);
    git(&tree.root, &["switch", "-qc", "proposal"]);
    tree.write("docs/space name.md", "# New\n");
    tree.write("code.rs", "// code\n");
    let tip = commit(&tree);
    tree.write(
        "docs/CARDS.md",
        &format!("{PAGE}  [2026-11-13 12:00] owner — Keep this.\n"),
    );
    let value = answer(
        &tree,
        "landing-checks",
        &["proposal", "docs/.hidden.md", "README.md"],
    );
    let common = git(
        &tree.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    assert_eq!(
        value,
        json!({
            "base": base, "tip": tip, "regenerates": false,
            "documents": {"code.rs": false, "docs/space name.md": true, "docs/.hidden.md": false, "README.md": false},
            "owner_lines_left": ["docs/CARDS.md"],
            "write_lock": Path::new(&common).join(symphony_board::folders::SERVER).join("verdict-writes.lock")
        })
    );
    // Consumers take this very file's OS advisory exclusive lock, held through the main swap.
    let held = symphony::commit::write_lock(&tree.root).unwrap().unwrap();
    let file = fs::OpenOptions::new()
        .append(true)
        .open(value["write_lock"].as_str().unwrap())
        .unwrap();
    assert!(file.try_lock().is_err(), "must contend with document saves");
    // Unlocked before its close, as the contract says: another test's git spawn may hold a copy
    // of its descriptor, and a close alone releases the lock only with the last copy.
    held.unlock().unwrap();
    drop(held);
    file.try_lock().unwrap();
}

fn generated() -> (Tree, String) {
    let tree = repository();
    tree.write("symphony.toml", "[verdicts]\nland = [\"just\", \"land\"]\nrecord = \".git/verdict-records\"\ngenerators = [{name = \"content\", outputs = \"content/\", command = [\"sh\", \"-c\", \"mkdir -p content; cat docs/CARDS.md > content/result.txt\"]}]\n");
    symphony::project::Project::load(&tree.root).unwrap();
    tree.write("content/result.txt", PAGE);
    commit(&tree);
    git(&tree.root, &["switch", "-qc", "verdict"]);
    let approved = PAGE.replace("[ ]", "[a]");
    tree.write("docs/CARDS.md", &approved);
    tree.write("content/result.txt", &approved);
    let tip = commit(&tree);
    tree.write(&format!(".git/verdict-records/{tip}"), "record\n");
    git(&tree.root, &["switch", "-q", "main"]);
    (tree, tip)
}

#[test]
fn landing_uses_committed_base_settings_and_proves_generated_bytes() {
    let (tree, _) = generated();
    // Neither the lander's uncommitted project file nor plugin generators widen this proof.
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/content/\"\nfolder = \"content\"\n",
    );
    let value = answer(&tree, "landing-checks", &["verdict"]);
    assert_eq!(value["regenerates"], true);
    assert_eq!(
        value["documents"],
        json!({"content/result.txt": false, "docs/CARDS.md": true})
    );
    assert_eq!(
        tree.read("content/result.txt"),
        PAGE,
        "generation never runs in caller's checkout"
    );
    assert_eq!(
        git(&tree.root, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    git(&tree.root, &["restore", "--", "symphony.toml"]);
    git(&tree.root, &["switch", "-q", "verdict"]);
    tree.write("content/result.txt", "forged\n");
    let tip = commit(&tree);
    tree.write(&format!(".git/verdict-records/{tip}"), "record\n");
    assert_eq!(
        answer(&tree, "landing-checks", &["verdict"])["regenerates"],
        false
    );
}

#[test]
fn landing_requires_record_and_refuses_extra_code_even_if_outputs_match() {
    let (tree, tip) = generated();
    fs::remove_file(tree.path(&format!(".git/verdict-records/{tip}"))).unwrap();
    assert_eq!(
        answer(&tree, "landing-checks", &["verdict"])["regenerates"],
        false
    );
    git(&tree.root, &["switch", "-q", "verdict"]);
    tree.write("extra.rs", "// not generated\n");
    let tip = commit(&tree);
    tree.write(&format!(".git/verdict-records/{tip}"), "record\n");
    assert_eq!(
        answer(&tree, "landing-checks", &["verdict"])["regenerates"],
        false
    );
}

#[test]
fn landing_base_option_controls_settings_and_invalid_revisions_fail_without_json() {
    let tree = repository();
    git(&tree.root, &["switch", "-qc", "other-base"]);
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/guide/\"\nfolder = \"pages\"\n",
    );
    let base = commit(&tree);
    git(&tree.root, &["switch", "-qc", "proposal"]);
    tree.write("pages/new.md", "# New\n");
    commit(&tree);
    assert_eq!(
        answer(
            &tree,
            "landing-checks",
            &["--base", "other-base", "proposal"]
        )["base"],
        base
    );
    assert_eq!(
        answer(
            &tree,
            "landing-checks",
            &["--base", "other-base", "proposal"]
        )["documents"],
        json!({"pages/new.md": true})
    );
    assert_eq!(
        answer(&tree, "landing-checks", &["proposal"])["documents"],
        json!({"pages/new.md": false, "symphony.toml": false})
    );
    for args in [
        vec!["missing"],
        vec!["--base", "missing", "proposal"],
        vec!["--", "--help"],
    ] {
        let (code, out, err) = call(&tree, "landing-checks", &args);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(!err.is_empty());
    }
    let empty = Tree::new();
    assert_eq!(call(&empty, "landing-checks", &["main"]).0, 1);
}

#[test]
fn landing_keeps_renamed_paths_exact_and_rejects_non_utf8_names() {
    let tree = repository();
    git(&tree.root, &["switch", "-qc", "proposal"]);
    fs::rename(tree.path("docs/CARDS.md"), tree.path("docs/new é\npage.md")).unwrap();
    commit(&tree);
    assert_eq!(
        answer(&tree, "landing-checks", &["proposal"])["documents"],
        json!({
            "docs/CARDS.md": true, "docs/new é\npage.md": true
        })
    );
    // macOS cannot create a non-UTF-8 filename. Such a commit can still arrive from another
    // machine, so construct its tree object without checking its path out on this filesystem.
    let blob = git(&tree.root, &["hash-object", "-w", "docs/new é\npage.md"]);
    let mut bytes = format!("100644 blob {blob}\t").into_bytes();
    bytes.extend_from_slice(b"invalid-\xff\0");
    fs::write(tree.path(".git/tree-input"), bytes).unwrap();
    let made = wait_output(
        git_command()
            .args(["mktree", "-z"])
            .current_dir(&tree.root)
            .stdin(fs::File::open(tree.path(".git/tree-input")).unwrap())
            .stdout(Stdio::Piped)
            .stderr(Stdio::Piped)
            .spawn()
            .unwrap(),
    )
    .unwrap();
    assert!(made.status.success(), "{made:?}");
    let object = String::from_utf8(made.stdout).unwrap();
    let shown = output(
        git_command()
            .args(["ls-tree", "-z", object.trim()])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(shown.status.success() && shown.stdout.ends_with(b"invalid-\xff\0"));
    let tip = git(
        &tree.root,
        &["commit-tree", object.trim(), "-p", "main", "-m", "fixture"],
    );
    let (code, out, err) = call(&tree, "landing-checks", &[&tip]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains("a changed path is not UTF-8"));
}

#[test]
fn landing_lock_identity_is_shared_with_linked_worktrees() {
    let tree = repository();
    let linked = Tree::new();
    let path = linked.root.to_str().unwrap();
    git(&tree.root, &["worktree", "add", "--detach", path, "main"]);
    let main = answer(&tree, "landing-checks", &["main"]);
    let other = answer(&linked, "landing-checks", &["main"]);
    assert_eq!(other, main);
    git(&tree.root, &["worktree", "remove", path]);
}

#[test]
fn landing_uses_plain_defaults_when_committed_settings_are_invalid() {
    let tree = repository();
    tree.write("symphony.toml", "invalid = [");
    commit(&tree);
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/\"\nfolder = \".\"\n",
    );
    let value = answer(
        &tree,
        "landing-checks",
        &["main", "docs/CARDS.md", "code.md"],
    );
    assert_eq!(value["regenerates"], false);
    assert_eq!(
        value["documents"],
        json!({"docs/CARDS.md": true, "code.md": false})
    );
}
