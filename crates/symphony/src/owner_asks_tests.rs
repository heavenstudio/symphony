//! When a scan reads an ask's answer: never before it read the owner's last line, never while the
//! card, comment or thread still waits on an agent, and from the copy the owner saw it on.

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
    }
}

fn reading(agent: Vec<QueueItem>) -> Reading {
    Reading {
        root: PathBuf::from("/nowhere"),
        agent,
        read_at: READ_AT,
        ..Reading::default()
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

#[test]
fn a_card_on_main_is_answered_once_it_leaves_the_agent_queue() {
    let waiting = reading(vec![item("docs/A.md", "lamp-a", "too pale")]);
    assert_eq!(answer(&card(None), &waiting), None);
    let left = reading(vec![item("docs/B.md", "lamp-a", "too pale")]);
    assert_eq!(
        answer(&card(None), &left),
        Some((None, "docs/A.md#lamp-a".into()))
    );
}

#[test]
fn an_ask_the_owner_said_more_to_after_the_read_waits_for_the_next_scan() {
    let empty = reading(Vec::new());
    for ask in [card(None), comment(None)] {
        let later = Ask {
            last: READ_AT,
            ..ask.clone()
        };
        assert_eq!(answer(&later, &empty), None, "{}", ask.kind);
        assert!(answer(&ask, &empty).is_some(), "{}", ask.kind);
    }
}

#[test]
fn a_branch_card_waits_while_its_branch_page_or_mains_copy_waits() {
    let ask = card(Some("pic/x"));
    let branch = reading(vec![item("@pic~x/docs/A.md", "lamp-a", "no")]);
    assert_eq!(answer(&ask, &branch), None);
    let main = reading(vec![item("docs/A.md", "lamp-a", "no")]);
    assert_eq!(answer(&ask, &main), None);
    assert_eq!(
        answer(&ask, &reading(Vec::new())),
        Some((None, "@pic~x/docs/A.md#lamp-a".into()))
    );
}

#[test]
fn a_message_on_a_card_is_read_as_its_card() {
    let message = Ask {
        kind: "message".into(),
        ..card(None)
    };
    assert_eq!(
        answer(&message, &reading(vec![item("docs/A.md", "lamp-a", "?")])),
        None
    );
    assert!(answer(&message, &reading(Vec::new())).is_some());
}

#[test]
fn a_comment_on_main_waits_while_an_open_comment_has_its_title() {
    let open = reading(vec![item("docs/A.md", "L12", " Too many lamps ")]);
    assert_eq!(answer(&comment(None), &open), None);
    // A card whose last line says the same words is no comment.
    let card_words = reading(vec![item("docs/A.md", "lamp-a", "Too many lamps")]);
    assert_eq!(
        answer(&comment(None), &card_words),
        Some((None, "docs/A.md: Too many lamps".into()))
    );
    let elsewhere = reading(vec![item("docs/B.md", "L12", "Too many lamps")]);
    assert!(answer(&comment(None), &elsewhere).is_some());
}

#[test]
fn a_branch_comment_is_read_from_its_worktree_and_from_main_once_the_worktree_is_gone() {
    let tree = Tree::new();
    let worktree = Tree::new();
    let mut scan = Reading {
        root: tree.root.clone(),
        trees: BTreeMap::from([("pic~x".to_string(), worktree.root.clone())]),
        ..reading(Vec::new())
    };
    let ask = comment(Some("pic/x"));
    worktree.write(
        "docs/A.md",
        "# A\n\n> [!dislike] Too many lamps\n> Fewer.\n",
    );
    assert_eq!(answer(&ask, &scan), None);
    worktree.write(
        "docs/A.md",
        "# A\n\n> [!dislike] Too many lamps\n> Fewer.\n> > [!resolved] Two now.\n",
    );
    assert_eq!(
        answer(&ask, &scan),
        Some((None, "@pic~x/docs/A.md: Too many lamps".into()))
    );
    std::fs::remove_file(worktree.path("docs/A.md")).unwrap();
    assert!(
        answer(&ask, &scan).is_some(),
        "a document gone answers nothing more"
    );
    scan.trees.clear();
    scan.agent = vec![item("docs/A.md", "L3", "Too many lamps")];
    assert_eq!(answer(&ask, &scan), None);
}

#[test]
fn a_chat_is_answered_by_its_threads_answer_naming_its_session() {
    let mut scan = reading(Vec::new());
    assert_eq!(answer(&chat(4), &scan), None, "the ask store was not read");
    scan.chats = Some((vec![thread(4, "open", &[])], READ_AT));
    assert_eq!(answer(&chat(4), &scan), None);
    scan.chats = Some((
        vec![thread(4, "answered", &["agent", "amber-heron"])],
        READ_AT,
    ));
    assert_eq!(
        answer(&chat(4), &scan),
        Some((Some("amber-heron".into()), "ask 4's answer".into()))
    );
    scan.chats = Some((vec![thread(4, "answered", &["agent"])], READ_AT));
    assert_eq!(
        answer(&chat(4), &scan),
        Some((None, "ask 4's answer".into()))
    );
    scan.chats = Some((Vec::new(), READ_AT));
    assert_eq!(
        answer(&chat(4), &scan),
        Some((None, "ask 4, gone from the ask store".into()))
    );
    let followed_up = Ask {
        last: READ_AT,
        ..chat(4)
    };
    assert_eq!(answer(&followed_up, &scan), None);
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
    let scan = reading(Vec::new());
    let task = Ask {
        task: Some("tools/x".into()),
        ..ask("task")
    };
    assert_eq!(answer(&task, &scan), None);
    let no_card = ask("verdict");
    assert_eq!(answer(&no_card, &scan), None);
    let bad_thread = Ask {
        thread: Some("four".into()),
        ..chat(4)
    };
    let scan = Reading {
        chats: Some((Vec::new(), READ_AT)),
        ..scan
    };
    assert_eq!(answer(&bad_thread, &scan), None);
}
