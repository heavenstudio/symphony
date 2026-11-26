//! The shared server's part in the owner's asks: an ask closes once the documents it reads have the
//! card answered, credited to who replied, stays open and marked when its card's worktree goes, even
//! where main holds a copy of the card that never carried the owner's line, and is left as it was
//! when the worktrees cannot be listed or its worktree scanned; a private server leaves asks alone,
//! and no start of either touches a verdict task (`symphony verdict-asks` alone turns one).

use super::{Hub, owner_asks::Kept};
use crate::testing::{Tree, board, board::Fixture, git, repository_holding};
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::sys::Command;

const DOC: &str = "docs/DECISIONS.md";

fn register(answer: &str) -> String {
    format!(
        "# Decisions\n\n<!-- register:start -->\n\
         - [-] **lamp-a** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — too pale.\n{answer}\
         - [-] **lamp-b** — Lamp B.\n  [2026-10-06 10:05] owner · rejected — too dark.\n\
         <!-- register:end -->\n"
    )
}

const BRIEF: &str =
    "Cards:\n- docs/DECISIONS.md:4 **lamp-a** rejected (2026-10-06 10:00): \"too pale.\"\n";

fn hub(root: &std::path::Path, shared: bool) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    if shared {
        *hub.owner_asks.get_mut().unwrap() = Some(Kept::default());
    }
    hub.refresh_docs(None).unwrap();
    hub
}

#[test]
fn no_server_tick_turns_a_verdict_task_and_the_shared_one_closes_an_ask_once_its_card_is_answered()
{
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, &register(""));
    let fixture = Fixture::empty();
    let store = &fixture.store;
    let app = symphony::verdicts::app_handle(store).unwrap();
    store
        .create(&NewTask {
            body: BRIEF.into(),
            ..NewTask::new("verdicts/2026-10-06-1100", "Answer 1 owner verdict", &app)
        })
        .unwrap();
    let lamp_a = symphony::verdicts::open_cards(&root).unwrap()[0].ask();
    let ask = store.open_ask(&lamp_a).unwrap();
    std::fs::write(
        root.join(DOC),
        register("  [2026-10-06 10:30] agent — Paler now.\n"),
    )
    .unwrap();

    // A private server leaves the board as it is.
    hub(&root, false).keep_asks(store);
    assert!(store.ask(ask.id).unwrap().open);

    let shared = hub(&root, true);
    shared.keep_asks(store);
    let answered = store.ask(ask.id).unwrap();
    assert_eq!(
        (answered.open, answered.answer.as_deref()),
        (false, Some("docs/DECISIONS.md#lamp-a"))
    );
    shared.keep_asks(store);
    // Neither start nor tick moved the verdict task, nor opened an ask for it.
    let task = store.get("verdicts/2026-10-06-1100").unwrap();
    assert_eq!(task.status, "ready");
    assert!(
        store
            .thread("verdicts/2026-10-06-1100")
            .unwrap()
            .iter()
            .all(|m| !m.text.starts_with(symphony::verdicts::TURNED))
    );
    assert!(store.asks(None).unwrap().is_empty());
}

#[test]
fn an_ask_on_a_card_still_waiting_stays_open_and_the_scan_waits_for_a_change() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, &register(""));
    let fixture = Fixture::empty();
    let store = &fixture.store;
    let lamp_b = symphony::verdicts::open_cards(&root).unwrap()[1].ask();
    let ask = store.open_ask(&lamp_b).unwrap();
    let shared = hub(&root, true);
    shared.keep_asks(store);
    assert!(store.ask(ask.id).unwrap().open);
    // Answered on the board meanwhile, by hand: no change the scan watches, so it does not look again
    // before its minute.
    let worker = fixture.session("worker");
    store.close_ask(ask.id, &worker, "asked in person").unwrap();
    let again = store.open_ask(&lamp_b).unwrap();
    std::fs::write(
        root.join(DOC),
        register("").replace(
            "too dark.\n",
            "too dark.\n  [2026-10-06 10:30] agent — Darker now.\n",
        ),
    )
    .unwrap();
    shared.keep_asks(store);
    assert!(
        store.ask(again.id).unwrap().open,
        "the documents were not read again"
    );
    shared.refresh_docs(None).unwrap();
    shared.keep_asks(store);
    assert!(!store.ask(again.id).unwrap().open);
}

/// The task cancelled and its worktree removed: the ask on the branch's card stays open, marked
/// gone, never answered; and a listing of the worktrees that fails changes nothing.
#[test]
fn a_branch_cards_ask_stays_open_when_its_worktree_goes_or_cannot_be_listed() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, &register(""));
    let worktree = tree.path("wt");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "pic/x",
            worktree.to_str().unwrap(),
        ],
    );
    let card = "- [-] **lamp-c** — Lamp C.\n  [2026-10-06 10:07] owner · rejected — too tall.\n";
    std::fs::write(
        worktree.join(DOC),
        register("").replace(
            "<!-- register:end -->",
            &format!("{card}<!-- register:end -->"),
        ),
    )
    .unwrap();
    git(&worktree, &["commit", "-qam", "Lamp C"]);
    let fixture = Fixture::empty();
    let store = &fixture.store;
    let lamp_c = symphony::verdicts::open_cards(&root)
        .unwrap()
        .into_iter()
        .find(|card| card.slug == "lamp-c")
        .expect("the branch's card")
        .ask();
    assert_eq!(lamp_c.tree.as_deref(), Some("pic/x"));
    let ask = store.open_ask(&lamp_c).unwrap();
    let shared = hub(&root, true);
    shared.keep_asks(store);
    let waiting = store.ask(ask.id).unwrap();
    assert!(waiting.open && waiting.gone.is_none(), "{waiting:?}");

    // `git worktree list` fails: the branch's card is in no page, and that says nothing.
    std::fs::rename(root.join(".git"), root.join(".git-away")).unwrap();
    shared.refresh_docs(None).unwrap();
    shared.keep_asks(store);
    std::fs::rename(root.join(".git-away"), root.join(".git")).unwrap();
    assert_eq!(
        store.ask(ask.id).unwrap(),
        waiting,
        "a failed listing changes nothing"
    );

    git(
        &root,
        &["worktree", "remove", "--force", worktree.to_str().unwrap()],
    );
    shared.refresh_docs(None).unwrap();
    shared.keep_asks(store);
    let gone = store.ask(ask.id).unwrap();
    assert!(
        gone.open && gone.answered.is_none() && gone.answer.is_none(),
        "{gone:?}"
    );
    assert_eq!(
        gone.gone.as_deref(),
        Some("the worktree of pic/x is gone, and main's docs/DECISIONS.md has no card lamp-c")
    );
    assert_eq!(store.asks(None).unwrap().len(), 1, "still open");
}

/// The owner's line on a branch's copy of a card main holds too, answered there long before: the
/// branch's page has it waiting; a scan of the worktree that fails changes nothing; and the task
/// cancelled and its worktree removed unmerged, main's copy, which never carried that line, answers
/// nothing: the ask stays open, marked gone.
#[test]
fn a_branch_ask_on_a_card_main_holds_is_gone_once_its_worktree_is_removed_unmerged() {
    let tree = Tree::new();
    let answered = "  [2026-10-06 10:30] agent — Paler now.\n";
    let root = repository_holding(&tree, "main", DOC, &register(answered));
    let worktree = tree.path("wt");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "pic/x",
            worktree.to_str().unwrap(),
        ],
    );
    let again = "  [2026-10-06 11:00] owner · rejected — still too pale.\n";
    std::fs::write(worktree.join(DOC), register(&format!("{answered}{again}"))).unwrap();
    git(&worktree, &["commit", "-qam", "Still too pale"]);
    let fixture = Fixture::empty();
    let store = &fixture.store;
    let lamp_a = symphony::verdicts::open_cards(&root)
        .unwrap()
        .into_iter()
        .find(|card| card.slug == "lamp-a")
        .expect("the branch's copy, waiting")
        .ask();
    assert_eq!(
        (lamp_a.tree.as_deref(), lamp_a.path.as_deref()),
        (Some("pic/x"), Some(DOC))
    );
    let ask = store.open_ask(&lamp_a).unwrap();
    let shared = hub(&root, true);
    shared.keep_asks(store);
    let waiting = store.ask(ask.id).unwrap();
    assert!(waiting.open && waiting.gone.is_none(), "{waiting:?}");

    // Git cannot read the worktree, which is still listed: its scan fails, and says so.
    let link = std::fs::read_to_string(worktree.join(".git")).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: /nowhere/at/all\n").unwrap();
    shared.refresh_docs(None).unwrap();
    shared.keep_asks(store);
    std::fs::write(worktree.join(".git"), link).unwrap();
    assert_eq!(
        store.ask(ask.id).unwrap(),
        waiting,
        "a worktree whose scan failed changes none of its asks"
    );

    git(
        &root,
        &["worktree", "remove", "--force", worktree.to_str().unwrap()],
    );
    shared.refresh_docs(None).unwrap();
    shared.keep_asks(store);
    let gone = store.ask(ask.id).unwrap();
    assert!(
        gone.open && gone.answered.is_none() && gone.answer.is_none(),
        "{gone:?}"
    );
    assert_eq!(
        gone.gone.as_deref(),
        Some(
            "the worktree of pic/x is gone, and main's copy of card lamp-a does not carry the \
             owner's line"
        )
    );
}

const CHILD: &str = "SYMPHONY_ASKS_SAVE_TO_ANSWER";
const SAVE_TO_ANSWER: &str =
    "live::owner_asks_tests::a_verdict_and_a_chat_go_from_the_owners_save_to_their_answer";

/// `when`, as the board and its log say it: local time to the second.
fn when(seconds: f64) -> String {
    let at = seconds as i64;
    format!(
        "{}:{:02}",
        symphony::clock::stamp_at(at as _),
        at.rem_euclid(60)
    )
}

fn said(ask: &symphony_board::Ask) -> String {
    format!(
        "ask {} {} about {} {:?}: asked {}, {}",
        ask.id,
        ask.kind,
        ask.about,
        ask.words,
        when(ask.asked),
        match (ask.open, ask.answered) {
            (true, _) => format!(
                "open, held by {}",
                ask.holder.as_deref().unwrap_or("nobody")
            ),
            (false, answered) => format!(
                "answered {} by {} at {}",
                answered.map_or_else(String::new, when),
                ask.answered_by.as_deref().unwrap_or("(no session named)"),
                ask.answer.as_deref().unwrap_or("")
            ),
        }
    )
}

/// The whole way, on a board of its own, as the shared server runs it: the owner's verdict and chat
/// question saved, their journal records handed to the board by the server's tick, which opens their
/// asks; an agent's line on the card and an agent's answer in the ask store; the next ticks read the
/// answers and close the asks. Run with `--nocapture`, it says each step and when.
#[test]
fn a_verdict_and_a_chat_go_from_the_owners_save_to_their_answer() {
    let Ok(_) = std::env::var(CHILD) else {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", DOC, &register(""));
        // Saved off main, so no verdict commit runs.
        git(&root, &["checkout", "-q", "-b", "review"]);
        let db = board::database();
        // The board's command line makes the board; the Symphony, a reader, never does.
        crate::testing::store_on(&db);
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", SAVE_TO_ANSWER, "--nocapture"])
            .env(CHILD, "1")
            .env("TASKS_SERVER", db.addr())
            .env("TASKS_DATABASE", db.database())
            .env_remove("TASKS_NOW")
            .env_remove("ASKS_DB")
            .env_remove("TASKS_SESSION")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .current_dir(&root);
        let done = symphony_board::sys::output(&mut child).unwrap();
        let out = String::from_utf8_lossy(&done.stdout);
        let err = String::from_utf8_lossy(&done.stderr);
        assert!(done.status.success(), "{out}\n{err}");
        assert!(out.contains("1 passed"), "the child ran no test: {out}");
        for line in out.lines().filter(|line| line.starts_with("DEMO ")) {
            println!("{line}");
        }
        return;
    };
    let root = std::env::current_dir().unwrap();
    let store = symphony::taskview::connect(&root).unwrap();
    let mut hub = Hub::new(&root, &root.join("build/client"));
    *hub.callbacks.get_mut().unwrap() = Some(symphony::callbacks::Flusher::new(&root));
    *hub.owner_asks.get_mut().unwrap() = Some(Kept::default());
    hub.refresh_docs(None).unwrap();
    let saves = symphony::save::Saves::start(&root);
    let save = |route: &str, payload: serde_json::Value| {
        saves
            .post("", route, &payload)
            .unwrap_or_else(|e| panic!("{route}: {} {}", e.status, e.error))
    };
    let step = |what: &str| println!("DEMO {} {what}", when(owner_asks_now()));
    let asks = |store: &symphony_board::Store| store.asks(Some(0.0)).unwrap();

    step("the owner rejects lamp-a: \"too pale, and the base is wrong\"");
    save(
        "/api/decision",
        json!({"path": DOC, "slug": "lamp-a", "state": "rejected", "note": "too pale, and the base is wrong"}),
    );
    step("the owner asks in the chat on the document: \"Why five lamps?\"");
    let chat = save(
        "/api/ask",
        json!({"path": DOC, "question": "Why five lamps?", "anchor": "", "quote": ""}),
    )["id"]
        .as_i64()
        .unwrap();
    hub.refresh_docs(None).unwrap();
    step("the shared server's tick hands the journal to the board");
    hub.board_read(&store).unwrap();
    let opened = asks(&store);
    for ask in &opened {
        println!("DEMO   {}", said(ask));
    }
    assert_eq!(opened.len(), 2, "{opened:?}");
    assert!(opened.iter().all(|ask| ask.open));
    assert_eq!(
        (opened[0].kind.as_str(), opened[0].card.as_deref()),
        ("verdict", Some("lamp-a"))
    );
    assert_eq!(
        (opened[1].kind.as_str(), opened[1].thread.clone()),
        ("chat", Some(chat.to_string()))
    );

    step("brisk-lark answers on the card (`symphony reply --as brisk-lark`)");
    let file = root.join(DOC).to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let replied = symphony::cli::main(
        &[
            "reply",
            file.as_str(),
            "lamp-a",
            "Paler, on the old base.",
            "--as",
            "brisk-lark",
        ]
        .map(String::from),
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    assert_eq!(replied, 0, "{}", String::from_utf8_lossy(&err));
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    hub.refresh_docs(None).unwrap();
    step("the next tick reads the card answered");
    hub.board_read(&store).unwrap();
    let verdict = store.ask(opened[0].id).unwrap();
    println!("DEMO   {}", said(&verdict));
    assert_eq!(
        (verdict.open, verdict.answered_by.as_deref()),
        (false, Some("brisk-lark"))
    );
    assert!(
        store.ask(opened[1].id).unwrap().open,
        "the chat still waits"
    );

    step("amber-heron answers the chat (`symphony ask answer`)");
    let id = chat.to_string();
    let answered = symphony::asks::main(
        &[
            "answer",
            id.as_str(),
            "Five fill the hall; four left a gap.",
            "--by",
            "amber-heron",
        ]
        .map(String::from),
        &std::collections::BTreeMap::new(),
        &mut out,
        &mut err,
    );
    assert_eq!(answered, 0, "{}", String::from_utf8_lossy(&err));
    step("the next tick reads the thread answered");
    hub.board_read(&store).unwrap();
    let chat_ask = store.ask(opened[1].id).unwrap();
    println!("DEMO   {}", said(&chat_ask));
    assert_eq!(
        (chat_ask.open, chat_ask.answered_by.as_deref()),
        (false, Some("amber-heron"))
    );
    assert!(store.asks(None).unwrap().is_empty(), "nothing is left open");
}

fn owner_asks_now() -> f64 {
    symphony::owner_asks::now()
}

/// The ask each row of main's agent queue says, by its slug.
fn told(hub: &Hub) -> Vec<(String, Option<symphony_api::QueueAsk>)> {
    hub.pages.read().unwrap()[""]
        .queue
        .agent
        .iter()
        .map(|item| (item.slug.clone(), item.ask.clone()))
        .collect()
}

/// A private server reads the open asks too: the row of a card waiting on an agent says the ask the
/// board holds for it, who holds it once someone does, and nothing once it is answered; each
/// change is one event of the agent queue, a tick that finds the asks as they were none, and a
/// read of the documents keeps what the board last said.
#[test]
fn a_tick_tells_the_agent_queues_cards_their_open_asks_and_a_document_read_keeps_them() {
    let tree = Tree::new();
    let root = repository_holding(&tree, "main", DOC, &register(""));
    let fixture = Fixture::empty();
    let store = &fixture.store;
    let private = hub(&root, false);
    private.note_asks(store);
    assert_eq!(
        told(&private),
        [("lamp-a".to_string(), None), ("lamp-b".to_string(), None)],
        "a card no ask is open for says none"
    );
    let version = || private.pages.read().unwrap()[""].board.version;
    let before = version();

    let lamp_a = symphony::verdicts::open_cards(&root).unwrap()[0].ask();
    let ask = store.open_ask(&lamp_a).unwrap();
    private.note_asks(store);
    let nobody = symphony_api::QueueAsk {
        id: ask.id,
        asked: ask.asked.floor() as i64,
        holder: None,
        held: None,
    };
    assert_eq!(
        told(&private),
        [
            ("lamp-a".to_string(), Some(nobody.clone())),
            ("lamp-b".to_string(), None)
        ]
    );
    assert_eq!(version(), before + 1);
    let event = private.pages.read().unwrap()[""]
        .history
        .back()
        .unwrap()
        .json
        .clone();
    assert!(
        event.contains("\"agent-queue\"") && !event.contains("agent-queue-order"),
        "the row changed and the order did not: {event}"
    );
    // The same asks again publish nothing.
    private.note_asks(store);
    assert_eq!(version(), before + 1);

    let worker = fixture.session("worker");
    let held = store.hold_ask(ask.id, Some(&worker), "took it").unwrap();
    private.note_asks(store);
    assert_eq!(
        told(&private)[0].1,
        Some(symphony_api::QueueAsk {
            holder: Some(worker.clone()),
            held: held.held.map(|since| since.floor() as i64),
            ..nobody
        })
    );
    // The documents read again: the rows are built anew and still say the ask.
    private.refresh_docs(None).unwrap();
    assert_eq!(
        told(&private)[0]
            .1
            .as_ref()
            .and_then(|ask| ask.holder.clone()),
        Some(worker.clone())
    );

    store.close_ask(ask.id, &worker, "asked in person").unwrap();
    private.note_asks(store);
    assert_eq!(told(&private)[0].1, None);
    assert!(
        store.asks(None).unwrap().is_empty(),
        "a private server wrote nothing"
    );
}
