//! What a scan reads of an ask: never anything before it read the owner's last line or while it
//! could not read; waiting while the card, comment or thread still waits on an agent, from the copy
//! the owner saw it on; answered only where the answer can be read, credited to who wrote it; and
//! gone, never answered, when what it is about went.

use super::*;
use crate::asks::Answer;
use crate::test_tree::Tree;

const READ_AT: f64 = 1_000.0;

fn ask(kind: &str) -> Ask {
    Ask {
        id: 7,
        kind: kind.into(),
        path: Some("docs/A.md".into()),
        asked: 900.0,
        last: 950.0,
        lines: 1,
        open: true,
        ..Ask::default()
    }
}

fn card(tree: Option<&str>) -> Ask {
    Ask {
        card: Some("lamp-a".into()),
        tree: tree.map(str::to_string),
        ..ask("verdict")
    }
}

fn comment(tree: Option<&str>) -> Ask {
    Ask {
        block: Some("Too many lamps".into()),
        tree: tree.map(str::to_string),
        ..ask("comment")
    }
}

fn chat(thread: i64) -> Ask {
    Ask {
        path: None,
        thread: Some(thread.to_string()),
        ..ask("chat")
    }
}

fn item(path: &str, slug: &str, text: &str) -> QueueItem {
    QueueItem {
        href: format!("/{path}#{slug}"),
        path: path.into(),
        slug: slug.into(),
        title: "A".into(),
        kind: "dislike".into(),
        text: text.into(),
        branch: None,
        ask: None,
    }
}

/// A scan of a checkout with no worktree, whose page holds the cards `held`, with no dated line of
/// the owner's under them, and has `agent` waiting.
fn reading(agent: Vec<QueueItem>, held: &[(&str, &str)]) -> Reading {
    Reading {
        root: PathBuf::from("/nowhere"),
        agent,
        held: Arc::new(
            held.iter()
                .map(|(page, slug)| ((page.to_string(), slug.to_string()), None))
                .collect(),
        ),
        read_at: READ_AT,
        listed: true,
        ..Reading::default()
    }
}

/// `reading` with the owner's last line under the copy of `lamp-a` on `page` dated `at`.
fn owner_said(reading: Reading, page: &str, at: f64) -> Reading {
    let mut held = (*reading.held).clone();
    held.insert((page.to_string(), "lamp-a".to_string()), Some(at));
    Reading {
        held: Arc::new(held),
        ..reading
    }
}

/// [`reading`] with the worktree of `pic/x` listed at `path`.
fn with_tree(reading: Reading, path: &Path) -> Reading {
    Reading {
        trees: BTreeMap::from([("pic~x".to_string(), path.to_path_buf())]),
        ..reading
    }
}

fn thread(id: i64, status: &str, by: &[&str]) -> Thread {
    Thread {
        id,
        path: "docs/A.md".into(),
        anchor: String::new(),
        quote: String::new(),
        question: "why?".into(),
        asked: 900.0,
        updated: 960.0,
        answers: by
            .iter()
            .map(|by| Answer {
                text: "because".into(),
                by: (*by).into(),
                at: 960.0,
            })
            .collect(),
        messages: Vec::new(),
        status: status.into(),
    }
}

fn reply(tree: &str, kind: &str, target: &str, by: &str, at: f64) -> Reply {
    Reply {
        tree: tree.into(),
        path: "docs/A.md".into(),
        kind: kind.into(),
        target: target.into(),
        by: by.into(),
        at,
    }
}

fn answered(at: &str) -> Read {
    Read::Answered(None, at.into())
}

fn gone(why: &str) -> Read {
    Read::Gone(why.into())
}

#[test]
fn a_card_on_main_is_answered_once_its_page_holds_it_waiting_on_no_one() {
    let waiting = reading(
        vec![item("docs/A.md", "lamp-a", "too pale")],
        &[("docs/A.md", "lamp-a")],
    );
    assert_eq!(read(&card(None), &waiting), Read::Waiting);
    let left = reading(
        vec![item("docs/B.md", "lamp-a", "too pale")],
        &[("docs/A.md", "lamp-a")],
    );
    assert_eq!(read(&card(None), &left), answered("docs/A.md#lamp-a"));
}

#[test]
fn a_card_no_page_holds_is_gone_and_never_answered() {
    let moved = reading(Vec::new(), &[("docs/B.md", "lamp-a")]);
    assert_eq!(
        read(&card(None), &moved),
        gone("docs/A.md has no card lamp-a")
    );
}

#[test]
fn an_ask_the_owner_said_more_to_after_the_read_waits_for_the_next_scan() {
    let empty = reading(Vec::new(), &[("docs/A.md", "lamp-a")]);
    for ask in [card(None), comment(None)] {
        let later = Ask {
            last: READ_AT,
            ..ask.clone()
        };
        assert_eq!(read(&later, &empty), Read::Unsure, "{}", ask.kind);
    }
    assert_ne!(read(&card(None), &empty), Read::Unsure);
}

#[test]
fn a_branch_card_waits_while_its_branch_page_or_mains_copy_with_the_owners_line_waits() {
    let worktree = Tree::new();
    let ask = card(Some("pic/x"));
    let held = [("@pic~x/docs/A.md", "lamp-a"), ("docs/A.md", "lamp-a")];
    let branch = with_tree(
        reading(vec![item("@pic~x/docs/A.md", "lamp-a", "no")], &held),
        &worktree.root,
    );
    assert_eq!(read(&ask, &branch), Read::Waiting);
    // The owner's line laid on main's copy, for a card the branch did not post.
    let main = with_tree(
        owner_said(
            reading(vec![item("docs/A.md", "lamp-a", "no")], &held),
            "docs/A.md",
            900.0,
        ),
        &worktree.root,
    );
    assert_eq!(read(&ask, &main), Read::Waiting);
    // Main's copy waiting on a line of its own says nothing of the branch's.
    let mains_own = with_tree(
        reading(vec![item("docs/A.md", "lamp-a", "no")], &held),
        &worktree.root,
    );
    assert_eq!(read(&ask, &mains_own), answered("@pic~x/docs/A.md#lamp-a"));
    let answered_there = with_tree(reading(Vec::new(), &held), &worktree.root);
    assert_eq!(
        read(&ask, &answered_there),
        answered("@pic~x/docs/A.md#lamp-a"),
        "the page the owner saw it on first"
    );
}

#[test]
fn a_branch_card_that_left_its_branch_page_is_read_as_mains_only_where_it_carries_the_owners_line()
{
    let worktree = Tree::new();
    let ask = card(Some("pic/x"));
    let main = reading(Vec::new(), &[("docs/A.md", "lamp-a")]);
    // Merged: main's copy holds the owner's line, dated the minute the ask was opened in or the
    // one before it.
    for at in [840.0, 900.0, 960.0] {
        let merged = with_tree(owner_said(main.clone(), "docs/A.md", at), &worktree.root);
        assert_eq!(read(&ask, &merged), answered("docs/A.md#lamp-a"), "{at}");
    }
    // Main's copy as it was before the branch's line, the worktree still there.
    let before = with_tree(owner_said(main.clone(), "docs/A.md", 780.0), &worktree.root);
    let not_carried = "main's copy of card lamp-a does not carry the owner's line";
    assert_eq!(
        read(&ask, &before),
        gone(&format!(
            "card lamp-a is not on pic/x's docs/A.md, and {not_carried}"
        ))
    );
    assert_eq!(
        read(&ask, &with_tree(main.clone(), &worktree.root)),
        gone(&format!(
            "card lamp-a is not on pic/x's docs/A.md, and {not_carried}"
        ))
    );
    // The task cancelled and its worktree removed unmerged: main's copy, answered long before or
    // with no line of the owner's, answers nothing.
    for removed in [owner_said(main.clone(), "docs/A.md", 600.0), main] {
        assert_eq!(
            read(&ask, &removed),
            gone(&format!("the worktree of pic/x is gone, and {not_carried}"))
        );
    }
}

#[test]
fn a_branch_card_gone_from_both_pages_or_with_its_worktree_is_gone() {
    let worktree = Tree::new();
    let ask = card(Some("pic/x"));
    // The worktree listed, and a scan of it that holds the card on neither page.
    let nowhere = with_tree(reading(Vec::new(), &[]), &worktree.root);
    assert_eq!(
        read(&ask, &nowhere),
        gone("card lamp-a is not on pic/x's docs/A.md, and main's docs/A.md has no card lamp-a")
    );
    // The task cancelled and its worktree removed: the listing has no such worktree.
    let removed = reading(Vec::new(), &[("docs/B.md", "lamp-a")]);
    assert_eq!(
        read(&ask, &removed),
        gone("the worktree of pic/x is gone, and main's docs/A.md has no card lamp-a")
    );
}

#[test]
fn a_branch_card_on_a_worktree_whose_scan_failed_changes_nothing() {
    let worktree = Tree::new();
    let unscanned = Reading {
        unscanned: BTreeSet::from(["pic/x".to_string()]),
        ..with_tree(reading(Vec::new(), &[]), &worktree.root)
    };
    assert_eq!(read(&card(Some("pic/x")), &unscanned), Read::Unsure);
    // Another branch's, and main's, are read as ever.
    assert!(matches!(
        read(&card(Some("pic/y")), &unscanned),
        Read::Gone(_)
    ));
    assert!(matches!(read(&card(None), &unscanned), Read::Gone(_)));
}

#[test]
fn a_failed_listing_of_the_worktrees_leaves_every_branch_ask_as_it_was() {
    let unlisted = Reading {
        listed: false,
        ..reading(Vec::new(), &[])
    };
    assert_eq!(read(&card(Some("pic/x")), &unlisted), Read::Unsure);
    assert_eq!(read(&comment(Some("pic/x")), &unlisted), Read::Unsure);
    // Main's are read as ever.
    let unlisted = Reading {
        listed: false,
        ..reading(Vec::new(), &[("docs/A.md", "lamp-a")])
    };
    assert_eq!(read(&card(None), &unlisted), answered("docs/A.md#lamp-a"));
}

#[test]
fn a_main_card_a_branch_proposes_again_is_read_on_that_branchs_page() {
    let proposed = reading(
        vec![item("@pic~x/docs/A.md", "lamp-a", "no")],
        &[("@pic~x/docs/A.md", "lamp-a")],
    );
    assert_eq!(read(&card(None), &proposed), Read::Waiting);
    let answered_there = reading(Vec::new(), &[("@pic~x/docs/A.md", "lamp-a")]);
    assert_eq!(
        read(&card(None), &answered_there),
        answered("@pic~x/docs/A.md#lamp-a")
    );
    // Another document's card of the same slug is another card.
    let other = reading(
        vec![item("@pic~x/docs/B.md", "lamp-a", "no")],
        &[("docs/A.md", "lamp-a")],
    );
    assert_eq!(read(&card(None), &other), answered("docs/A.md#lamp-a"));
}

#[test]
fn a_message_on_a_card_is_read_as_its_card() {
    let message = Ask {
        kind: "message".into(),
        ..card(None)
    };
    let held = [("docs/A.md", "lamp-a")];
    assert_eq!(
        read(
            &message,
            &reading(vec![item("docs/A.md", "lamp-a", "?")], &held)
        ),
        Read::Waiting
    );
    assert_eq!(
        read(&message, &reading(Vec::new(), &held)),
        answered("docs/A.md#lamp-a")
    );
}

/// A checkout whose `docs/A.md` says `text`, read as main's.
fn on_main(text: &str) -> (Tree, Reading) {
    let tree = Tree::new();
    tree.write("docs/A.md", text);
    let scan = Reading {
        root: tree.root.clone(),
        ..reading(Vec::new(), &[])
    };
    (tree, scan)
}

const OPEN: &str = "# A\n\n> [!dislike] Too many lamps\n> Fewer.\n";
const RESOLVED: &str = "# A\n\n> [!dislike] Too many lamps\n> Fewer.\n> > [!resolved] Two now.\n";

#[test]
fn a_comment_on_main_waits_while_it_is_open_in_its_document() {
    let (tree, scan) = on_main(OPEN);
    assert_eq!(read(&comment(None), &scan), Read::Waiting);
    tree.write("docs/A.md", RESOLVED);
    assert_eq!(
        read(&comment(None), &scan),
        answered("docs/A.md: Too many lamps")
    );
    // A like of the same title waits on no one, and answers nothing.
    tree.write("docs/A.md", "# A\n\n> [!like] Too many lamps\n");
    assert_eq!(
        read(&comment(None), &scan),
        gone("docs/A.md has no comment \"Too many lamps\"")
    );
}

#[test]
fn a_comment_whose_document_or_title_went_is_gone_and_never_answered() {
    let (tree, scan) = on_main("# A\n\n> [!dislike] Other\n");
    assert_eq!(
        read(&comment(None), &scan),
        gone("docs/A.md has no comment \"Too many lamps\"")
    );
    std::fs::remove_file(tree.path("docs/A.md")).unwrap();
    assert_eq!(read(&comment(None), &scan), gone("docs/A.md is gone"));
}

#[test]
fn a_comment_the_tidy_archived_resolved_is_answered_from_the_ledger() {
    let (tree, scan) = on_main("# A\n");
    let ledger = "# Review ledger\n\nIntro.\n\n## 2026-11-12 09:00 · docs/A.md\n\n\
                  > [!dislike] Too many lamps\n> Fewer.\n> > [!resolved] Two now.\n";
    tree.write("docs/history/review-ledger.md", ledger);
    assert_eq!(
        read(&comment(None), &scan),
        answered("docs/A.md: Too many lamps")
    );
    // Archived from another document, or unresolved, it is not this one's answer.
    let elsewhere = ledger.replace("· docs/A.md", "· docs/B.md");
    tree.write("docs/history/review-ledger.md", &elsewhere);
    assert!(matches!(read(&comment(None), &scan), Read::Gone(_)));
}

#[test]
fn a_document_that_cannot_be_read_changes_nothing() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("docs/A.md")).unwrap();
    let scan = Reading {
        root: tree.root.clone(),
        ..reading(Vec::new(), &[])
    };
    assert_eq!(read(&comment(None), &scan), Read::Unsure);
}

#[test]
fn a_branch_comment_is_read_in_its_worktree_and_gone_with_it() {
    let main = Tree::new();
    let worktree = Tree::new();
    let ask = comment(Some("pic/x"));
    let scan = with_tree(
        Reading {
            root: main.root.clone(),
            ..reading(Vec::new(), &[])
        },
        &worktree.root,
    );
    worktree.write("docs/A.md", OPEN);
    // Main's copy says it is resolved: it is the worktree's that counts.
    main.write("docs/A.md", RESOLVED);
    assert_eq!(read(&ask, &scan), Read::Waiting);
    worktree.write("docs/A.md", RESOLVED);
    assert_eq!(
        read(&ask, &scan),
        answered("@pic~x/docs/A.md: Too many lamps")
    );
    std::fs::remove_file(worktree.path("docs/A.md")).unwrap();
    assert_eq!(read(&ask, &scan), gone("docs/A.md is gone"));
    let removed = Reading {
        trees: BTreeMap::new(),
        ..scan
    };
    assert_eq!(read(&ask, &removed), gone("the worktree of pic/x is gone"));
}

#[test]
fn a_chat_is_answered_by_its_threads_answer_naming_its_session() {
    let mut scan = reading(Vec::new(), &[]);
    assert_eq!(
        read(&chat(4), &scan),
        Read::Unsure,
        "the ask store was not read"
    );
    scan.chats = Some((vec![thread(4, "open", &[])], READ_AT));
    assert_eq!(read(&chat(4), &scan), Read::Waiting);
    scan.chats = Some((
        vec![thread(4, "answered", &["agent", "amber-heron"])],
        READ_AT,
    ));
    assert_eq!(
        read(&chat(4), &scan),
        Read::Answered(Some("amber-heron".into()), "ask 4's answer".into())
    );
    scan.chats = Some((vec![thread(4, "answered", &["agent"])], READ_AT));
    assert_eq!(read(&chat(4), &scan), answered("ask 4's answer"));
    let followed_up = Ask {
        last: READ_AT,
        ..chat(4)
    };
    assert_eq!(read(&followed_up, &scan), Read::Unsure);
}

#[test]
fn a_chat_whose_thread_went_is_gone_and_never_answered() {
    let scan = Reading {
        chats: Some((vec![thread(5, "answered", &["amber-heron"])], READ_AT)),
        ..reading(Vec::new(), &[])
    };
    assert_eq!(
        read(&chat(4), &scan),
        gone("chat 4 is gone from the ask store")
    );
}

#[test]
fn an_answer_on_a_card_or_comment_is_credited_to_who_replied_since_the_owners_last_line() {
    let mut scan = reading(Vec::new(), &[("docs/A.md", "lamp-a")]);
    scan.replies = vec![
        reply("", "card", "lamp-a", "early-wren", 940.0),
        reply("", "comment", "lamp-a", "wrong-kind", 960.0),
        reply("", "card", "lamp-b", "other-card", 960.0),
    ];
    assert_eq!(
        read(&card(None), &scan),
        answered("docs/A.md#lamp-a"),
        "a reply before the owner's last line answered an earlier one"
    );
    scan.replies
        .push(reply("", "card", "lamp-a", "amber-heron", 960.0));
    assert_eq!(
        read(&card(None), &scan),
        Read::Answered(Some("amber-heron".into()), "docs/A.md#lamp-a".into())
    );
    // The owner's own checkout first, else a reply in another: a branch's card main took.
    scan.replies
        .push(reply("pic/x", "card", "lamp-a", "late-finch", 970.0));
    assert_eq!(
        read(&card(None), &scan),
        Read::Answered(Some("amber-heron".into()), "docs/A.md#lamp-a".into())
    );
    scan.replies.retain(|r| r.tree == "pic/x");
    assert_eq!(
        read(&card(None), &scan),
        Read::Answered(Some("late-finch".into()), "docs/A.md#lamp-a".into())
    );
    let (_tree, mut on) = on_main(RESOLVED);
    on.replies = vec![reply(
        "",
        "comment",
        " Too many lamps",
        "amber-heron",
        960.0,
    )];
    assert_eq!(
        read(&comment(None), &on),
        Read::Answered(
            Some("amber-heron".into()),
            "docs/A.md: Too many lamps".into()
        )
    );
}

#[test]
fn a_scan_is_due_when_the_documents_or_the_ask_store_change_or_a_minute_passes() {
    let tree = Tree::new();
    let mut due = Due::default();
    assert!(due.now(&tree.root, 10.0, 100.0), "the first look");
    assert!(!due.now(&tree.root, 10.0, 130.0));
    assert!(
        due.now(&tree.root, 20.0, 131.0),
        "the documents were read again"
    );
    assert!(!due.now(&tree.root, 20.0, 150.0));
    assert!(
        due.now(&tree.root, 20.0, 131.0 + RESCAN_SECONDS),
        "a minute went by"
    );
    // The ask store is the one `ASKS_DB` names when it is set: a test writes only its own.
    if std::env::var_os("ASKS_DB").is_none() {
        let store = crate::asks::default_path(&tree.root, None);
        std::fs::create_dir_all(store.parent().unwrap()).unwrap();
        std::fs::write(&store, "x").unwrap();
        assert!(due.now(&tree.root, 20.0, 200.0), "the ask store changed");
        assert!(!due.now(&tree.root, 20.0, 201.0));
    }
}

#[test]
fn a_task_ask_or_one_missing_what_it_is_about_is_never_read_here() {
    let scan = reading(Vec::new(), &[]);
    let task = Ask {
        task: Some("tools/x".into()),
        ..ask("task")
    };
    assert_eq!(read(&task, &scan), Read::Unsure);
    let no_card = ask("verdict");
    assert_eq!(read(&no_card, &scan), Read::Unsure);
    let no_block = ask("comment");
    assert_eq!(read(&no_block, &scan), Read::Unsure);
    let bad_thread = Ask {
        thread: Some("four".into()),
        ..chat(4)
    };
    let scan = Reading {
        chats: Some((Vec::new(), READ_AT)),
        ..scan
    };
    assert_eq!(read(&bad_thread, &scan), Read::Unsure);
}

/// `git` in `at`, which must succeed.
fn git(at: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
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

#[test]
fn a_reply_is_placed_by_its_checkout_its_branch_and_its_documents_route() {
    let main = Tree::new();
    main.write("docs/A.md", "# A\n");
    git(&main.root, &["init", "-q"]);
    git(&main.root, &["add", "."]);
    git(&main.root, &["commit", "-qm", "a"]);
    let linked = main.path("wt");
    git(
        &main.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "pic/x",
            linked.to_str().unwrap(),
        ],
    );
    let top = std::fs::canonicalize(&main.root).unwrap();
    assert_eq!(
        reply_place(&main.path("docs/A.md")),
        Some((top.clone(), String::new(), "docs/A.md".into()))
    );
    assert_eq!(
        reply_place(&linked.join("docs/A.md")),
        Some((top.join("wt"), "pic/x".into(), "docs/A.md".into()))
    );
    let outside = Tree::new();
    outside.write("docs/A.md", "# A\n");
    assert_eq!(reply_place(&outside.path("docs/A.md")), None);
    assert_eq!(reply_place(&main.path("docs/none.md")), None);
}

#[test]
fn held_card_selection_prefers_main_then_first_lexical_equal_rank() {
    let held = [
        ("@z/docs/A.md", "lamp-a"),
        ("@a/docs/A.md", "lamp-a"),
        ("@0/docs/B.md", "lamp-a"),
        ("@0/docs/A.md", "another-card"),
    ];
    let branch_copies = reading(Vec::new(), &held);
    assert_eq!(
        read(&card(None), &branch_copies),
        answered("@a/docs/A.md#lamp-a"),
        "equal ranks retain the first held-key order, regardless of insertion order"
    );
    let mut on_main = held.to_vec();
    on_main.push(("docs/A.md", "lamp-a"));
    assert_eq!(
        read(&card(None), &reading(Vec::new(), &on_main)),
        answered("docs/A.md#lamp-a"),
        "main outranks the lexically earlier branch pages"
    );
    assert_eq!(
        read(
            &card(None),
            &reading(vec![item("@z/docs/A.md", "lamp-a", "waiting")], &on_main)
        ),
        Read::Waiting,
        "an agent waiting on any matching copy takes precedence over selection"
    );
}

/// The ask the rows of `agent` are told from `open`, on a page holding `held`.
fn noted(
    mut agent: Vec<QueueItem>,
    open: &[Ask],
    held: Reading,
) -> Vec<Option<symphony_api::QueueAsk>> {
    note(&mut agent, open, &held.held);
    agent.into_iter().map(|item| item.ask).collect()
}

fn told(id: i64, asked: i64, holder: Option<(&str, i64)>) -> Option<symphony_api::QueueAsk> {
    Some(symphony_api::QueueAsk {
        id,
        asked,
        holder: holder.map(|(handle, _)| handle.to_string()),
        held: holder.map(|(_, since)| since),
    })
}

#[test]
fn a_card_waiting_on_an_agent_says_its_open_ask_who_holds_it_and_since_when() {
    let nobody = card(None);
    let agent = vec![item("docs/A.md", "lamp-a", "")];
    assert_eq!(
        noted(
            agent.clone(),
            std::slice::from_ref(&nobody),
            reading(vec![], &[])
        ),
        [told(7, 900, None)],
        "held by nobody is said as that"
    );
    let held = Ask {
        holder: Some("umber-stoat".into()),
        held: Some(930.9),
        ..card(None)
    };
    assert_eq!(
        noted(agent.clone(), &[held], reading(vec![], &[])),
        [told(7, 900, Some(("umber-stoat", 930)))]
    );
    // A message on the card is the card's ask too.
    let message = Ask {
        kind: "message".into(),
        ..card(None)
    };
    assert_eq!(
        noted(agent, &[message], reading(vec![], &[])),
        [told(7, 900, None)]
    );
}

#[test]
fn a_row_no_open_ask_is_about_says_none() {
    let rows = vec![
        item("docs/A.md", "lamp-b", ""),
        item("docs/B.md", "lamp-a", ""),
        item("docs/A.md", "L12", "Too many lamps"),
    ];
    let answered = Ask {
        open: false,
        ..card(None)
    };
    // Another card, another document, a comment's row; an answered ask, a chat, a task's message
    // and a comment's ask, which names no card.
    let asks = [card(None), answered, chat(3), ask("task"), comment(None)];
    assert_eq!(noted(rows, &asks, reading(vec![], &[])), [None, None, None]);
    // A comment's row is never taken for the card of its name.
    let named = Ask {
        card: Some("L12".into()),
        ..card(None)
    };
    assert_eq!(
        noted(
            vec![item("docs/A.md", "L12", "Too many lamps")],
            &[named],
            reading(vec![], &[])
        ),
        [None]
    );
    // An ask said of nothing leaves a row told before with none.
    let mut before = item("docs/A.md", "lamp-a", "");
    before.ask = told(7, 900, None);
    assert_eq!(noted(vec![before], &[], reading(vec![], &[])), [None]);
}

#[test]
fn the_oldest_of_a_cards_open_asks_is_the_one_its_row_says() {
    let later = Ask {
        id: 9,
        asked: 950.0,
        ..card(None)
    };
    let same_time = Ask {
        id: 8,
        ..card(None)
    };
    assert_eq!(
        noted(
            vec![item("docs/A.md", "lamp-a", "")],
            &[later, same_time, card(None)],
            reading(vec![], &[])
        ),
        [told(7, 900, None)]
    );
}

#[test]
fn a_branchs_ask_is_said_on_its_branchs_page_and_on_mains_only_with_the_owners_line() {
    let branch = card(Some("pic/x"));
    let rows = || {
        vec![
            item("@pic~x/docs/A.md", "lamp-a", ""),
            item("docs/A.md", "lamp-a", ""),
        ]
    };
    assert_eq!(
        noted(
            rows(),
            std::slice::from_ref(&branch),
            reading(vec![], &[("docs/A.md", "lamp-a")])
        ),
        [told(7, 900, None), None],
        "main's copy without the owner's line is another card"
    );
    assert_eq!(
        noted(
            rows(),
            std::slice::from_ref(&branch),
            owner_said(reading(vec![], &[]), "docs/A.md", 890.0)
        ),
        [told(7, 900, None), told(7, 900, None)],
        "main's copy carrying the owner's line is the branch's card"
    );
    assert_eq!(
        noted(
            rows(),
            std::slice::from_ref(&branch),
            owner_said(reading(vec![], &[]), "docs/A.md", 700.0)
        ),
        [told(7, 900, None), None],
        "a line from before the ask is not the one it is about"
    );
    // Main's ask is said on main's page and on the page of a branch that proposes the card again.
    assert_eq!(
        noted(rows(), &[card(None)], reading(vec![], &[])),
        [told(7, 900, None), told(7, 900, None)]
    );
}
