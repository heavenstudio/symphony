//! A comment on a document held for people (docs/product/analysis/users-and-identity.md, "Words
//! without agent reach"), through the real router: ana, who may speak and may not direct agents,
//! leaves a callout and a card's message, and neither reaches a clone (no file, commit, branch,
//! ask or chat journal carries her words, on a checkout that delivers by pull request too) while
//! the document's held read shows them, with Pass on for one who may. A pass writes each as hers,
//! the passer first in its words, after its block found again once the file moved, or refuses in
//! plain words with nothing written; then the ask and the callback follow, once. Asks stay hers to
//! make only with direct agents. The note beside her verdict is written with it
//! ("The note beside a verdict").

use super::decide_tests::{MACHINE, as_person, post, read};
use crate::testing::{Board, LAPTOP, Tree, git, repository_holding};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::path::Path;
use symphony_board::people::Power;
use symphony_board::people::ownership::Place;

const DOC: &str = "docs/TOPIC.md";
const PAGE: &str = "# Topic\n\nSome prose about lamps.\n\n<!-- register:start -->\n- [ ] **one** — \
                    proposal.\n<!-- register:end -->\n";
/// A word in each of her held words, which no clone may carry until they are passed on.
const TOKEN: &str = "quartzlamp";

/// The board at `root` naming ana, who may speak, and stefano, who may speak and direct agents.
fn named(root: &Path) -> Board {
    Board::holding(
        root,
        &[
            ("ana", &[Power::Speak]),
            ("stefano", &[Power::Speak, Power::DirectAgents]),
        ],
    )
}

/// ana's question after the paragraph on line 3.
fn question() -> Value {
    json!({"path": DOC, "kind": "question", "after_line": 3,
        "text": format!("Too many {TOKEN}s\nShow the thirty that matter.")})
}

/// ana's message on the card `one`.
fn message() -> Value {
    json!({"path": DOC, "slug": "one", "text": format!("And why {TOKEN}?")})
}

/// The files under `root` but its git folder that hold `word`, by their path from it.
fn holding(root: &Path, word: &str) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, word: &str, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, word, found);
            } else if (std::fs::read(&path).ok())
                .is_some_and(|bytes| bytes.windows(word.len()).any(|w| w == word.as_bytes()))
            {
                found.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut found = Vec::new();
    walk(root, root, word, &mut found);
    found
}

/// Everything git holds of every ref and the stash, as text: what a clone or a push could carry.
fn history(root: &Path) -> String {
    let output = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["log", "--all", "-p", "--format=%H %s"])
            .current_dir(root),
    )
    .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Whether the chat's journal of the project at `root`, or its log, holds `word`.
fn journal_holds(root: &Path, word: &str) -> bool {
    let path = symphony::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref());
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    [path.clone(), wal.into()].iter().any(|file| {
        (std::fs::read(file).ok())
            .is_some_and(|bytes| bytes.windows(word.len()).any(|w| w == word.as_bytes()))
    })
}

/// The board's asks of the project at `root` once the chat's journal was handed to it, as the
/// shared server hands it: each one's kind and card.
fn asks(root: &Path) -> Vec<(String, Option<String>)> {
    symphony::callbacks::Flusher::new(root).flush(root).unwrap();
    (Board::store(root).asks(None).unwrap().into_iter())
        .map(|ask| (ask.kind, ask.card))
        .collect()
}

/// That nothing of a clone's, the chat's or the board's asks carries her words.
fn nowhere_but_the_board(root: &Path) {
    assert_eq!(std::fs::read_to_string(root.join(DOC)).unwrap(), PAGE);
    assert_eq!(holding(root, TOKEN), Vec::<String>::new());
    assert!(!history(root).contains(TOKEN));
    assert!(!journal_holds(root, TOKEN), "nothing for the chat");
    assert_eq!(asks(root), [], "no ask");
}

/// The held read of [`DOC`] by `client` of `shared`.
async fn held(shared: &std::sync::Arc<crate::front::Shared>, client: &str) -> Value {
    let (status, held) = read(shared, client, "/api/document-held?path=docs/TOPIC.md").await;
    assert_eq!(status, StatusCode::OK, "{held}");
    held
}

/// A save's answer, its status OK.
fn ok((status, body): (StatusCode, String)) -> Value {
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn held_words_reach_no_clone_and_a_pass_writes_them_as_hers_under_both_names() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root);
    let ana = as_person(&root, "ana");
    let asked = ok(post(&ana, LAPTOP, "/api/comment", question()).await)["held"].clone();
    let messaged = ok(post(&ana, LAPTOP, "/api/message", message()).await)["held"].clone();
    let (callout, on_card) = (asked.as_i64().unwrap(), messaged.as_i64().unwrap());
    // An ask is for an agent alone, and stays refused to her.
    let (status, _) = post(&ana, LAPTOP, "/api/ask", json!({"path": DOC, "question": "why?"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    ana.saves.get().unwrap().close();
    nowhere_but_the_board(&root);

    // Everyone who reads the page sees them; only one who may direct agents may pass them on.
    let mut seen = held(&ana, LAPTOP).await;
    assert_eq!(seen["passes"], false);
    assert_eq!(
        seen["held"],
        json!([
            {"id": callout, "author": "ana", "kind": "question",
             "text": format!("Too many {TOKEN}s\nShow the thirty that matter."),
             "key": "section:topic:1", "line": 3},
            {"id": on_card, "author": "ana", "kind": "message", "text": format!("And why {TOKEN}?"),
             "card": "one"},
        ])
    );
    let stefano = as_person(&root, "stefano");
    assert_eq!(held(&stefano, LAPTOP).await["passes"], true);
    assert_eq!(held(&stefano, MACHINE).await["passes"], true);
    let (status, said) = post(&ana, LAPTOP, "/api/document-pass", json!({"id": callout})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{said}");
    assert!(said.contains("ana may not direct agents"), "{said}");

    ok(post(&stefano, LAPTOP, "/api/document-pass", json!({"id": callout})).await);
    let (status, said) = post(&stefano, MACHINE, "/api/document-pass", json!({"id": callout})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{said}");
    assert!(said.contains("passed on already, by stefano: nothing was written"), "{said}");
    ok(post(&stefano, MACHINE, "/api/document-pass", json!({"id": on_card})).await);
    stefano.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    let lines: Vec<&str> = page.lines().collect();
    assert_eq!(
        &lines[2..6],
        [
            "Some prose about lamps.",
            "",
            &*format!("> [!question] ana · [passed on by stefano] Too many {TOKEN}s"),
            "> Show the thirty that matter.",
        ],
        "{page}"
    );
    assert!(
        (lines.iter()).any(|line| line.starts_with("  [")
            && line.ends_with(&format!("] ana — [passed on by owner] And why {TOKEN}?"))),
        "{page}"
    );
    assert!(history(&root).contains(TOKEN), "committed once passed");
    // The chat is told each as hers, and each opens the ask an agent answers.
    assert!(journal_holds(&root, "by=ana"), "the callback is hers");
    assert!(journal_holds(&root, "[passed on by stefano]"));
    assert_eq!(
        asks(&root),
        [("comment".into(), None), ("message".into(), Some("one".into()))]
    );
    seen = held(&stefano, MACHINE).await;
    assert_eq!(seen["held"], json!([]), "a passed comment is the file's");
    // One who may direct agents writes as before, under their own name.
    let own = json!({"path": DOC, "kind": "comment", "after_line": 3, "text": "Keep the lamps"});
    let again = as_person(&root, "stefano");
    let answer = ok(post(&again, LAPTOP, "/api/comment", own).await);
    assert_eq!(answer.get("held"), None, "{answer}");
    again.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    assert!(page.contains("> [!comment] stefano · Keep the lamps"), "{page}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pass_finds_its_block_once_the_file_moved_or_refuses_with_nothing_written() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = named(&root);
    let ana = as_person(&root, "ana");
    let id = |answer: Value| answer["held"].as_i64().unwrap();
    let moved = id(ok(post(&ana, LAPTOP, "/api/comment", question()).await));
    let changed = id(ok(post(&ana, LAPTOP, "/api/comment", question()).await));
    ana.saves.get().unwrap().close();
    // Lines come above the block: the pass finds it where it is now.
    let grown = PAGE.replace("# Topic\n\n", "# Topic\n\nA new opening.\n\nAnother one.\n\n");
    std::fs::write(root.join(DOC), &grown).unwrap();
    git(&root, &["commit", "-qam", "lines above"]);
    let stefano = as_person(&root, "stefano");
    ok(post(&stefano, MACHINE, "/api/document-pass", json!({"id": moved})).await);
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    let at = page.lines().position(|line| line == "Some prose about lamps.").unwrap();
    assert!(
        page.lines().nth(at + 2).unwrap().starts_with("> [!question] ana · [passed on by owner]"),
        "{page}"
    );
    // Its block's words changed: refused, the file as it was and the comment still held.
    let edited = page.replace("Some prose about lamps.", "Some prose about lights.");
    std::fs::write(root.join(DOC), &edited).unwrap();
    git(&root, &["commit", "-qam", "its words changed"]);
    let (status, said) = post(&stefano, MACHINE, "/api/document-pass", json!({"id": changed})).await;
    assert_eq!(
        (status, said),
        (
            StatusCode::CONFLICT,
            json!({"error": "the block this comment was left after has changed or is gone: \
                nothing was written"})
            .to_string()
        )
    );
    stefano.saves.get().unwrap().close();
    assert_eq!(std::fs::read_to_string(root.join(DOC)).unwrap(), edited);
    let still: Vec<i64> = (held(&stefano, MACHINE).await["held"].as_array().unwrap().iter())
        .map(|held| held["id"].as_i64().unwrap())
        .collect();
    assert_eq!(still, [changed]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_comment_on_a_checkout_that_delivers_by_pull_request_keeps_nothing_to_deliver() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    tree.write(
        "main/symphony.toml",
        "[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n",
    );
    git(&root, &["add", "symphony.toml"]);
    git(&root, &["commit", "-qm", "deliver by pull request"]);
    let refs = || {
        let listed = symphony_board::sys::output(
            symphony_board::sys::Command::new("git")
                .args(["for-each-ref", "--format=%(refname)"])
                .current_dir(&root),
        );
        String::from_utf8_lossy(&listed.unwrap().stdout).into_owned()
    };
    let before = refs();
    let _board = named(&root);
    let ana = as_person(&root, "ana");
    for (route, body) in [("/api/comment", question()), ("/api/message", message())] {
        let answer = ok(post(&ana, LAPTOP, route, body).await);
        assert!(answer["held"].is_i64(), "{route}: {answer}");
        assert_eq!(answer.get("delivery"), None, "{route}: nothing pending");
    }
    ana.saves.get().unwrap().close();
    nowhere_but_the_board(&root);
    assert_eq!(refs(), before, "no branch made to deliver");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_note_beside_her_verdict_is_written_with_it_and_asks() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, PAGE);
    let _board = Board::holding(&root, &[("ana", &[Power::Speak, Power::Decide])]);
    let store = Board::store(&root);
    store.add_group("design", &[], "owner").unwrap();
    store.join_group("ana", "design", "owner").unwrap();
    (store.map_place(&Place::Folder("docs".into()), "design", "owner")).unwrap();
    let ana = as_person(&root, "ana");
    let verdict = json!({"path": DOC, "slug": "one", "state": "rejected",
        "note": format!("needs a {TOKEN} picture"), "tap": "t1"});
    ok(post(&ana, LAPTOP, "/api/decision", verdict).await);
    ana.saves.get().unwrap().close();
    let page = std::fs::read_to_string(root.join(DOC)).unwrap();
    assert!(
        page.contains(&format!("] ana · rejected — needs a {TOKEN} picture")),
        "{page}"
    );
    assert!(journal_holds(&root, TOKEN), "the chat is told her note");
    assert_eq!(asks(&root), [("verdict".into(), Some("one".into()))]);
    assert_eq!(held(&ana, LAPTOP).await["held"], json!([]), "nothing held");
}
