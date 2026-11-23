//! The owner's verdicts waiting on an agent: the cards (main's and each unmerged branch's own), the
//! ask each one is on the board, and the `verdicts/…` tasks filed before there were asks, turned
//! into asks. The failures it exists to prevent: a card's words lost in the turning, a task turned
//! twice, a task an executor holds taken from it, and a session registered when nothing was turned.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use symphony_board::{NewAsk, NewSession, NewTask, Store, rpc, sys};

use symphony::owner_asks::{self, Reading};
use symphony::verdicts::{self, Board, TURNED, VerdictError, epoch};

use super::board;
use super::common::{Tree, older_board, tasks_binary};

/// The variables that reach a board of its own: a database of its own on the tests' SurrealDB server,
/// its clock at `board::NOW`.
fn board_env() -> Vec<(String, String)> {
    let db = board::client();
    vec![
        ("TASKS_SERVER".into(), db.addr().to_string()),
        ("TASKS_DATABASE".into(), db.database().to_string()),
        ("TASKS_NOW".into(), board::NOW.to_string()),
    ]
}

/// The native store on the board `env` reaches, as the shared server holds it.
fn fixture_store(env: &[(String, String)]) -> Store {
    let value = |name| env.iter().find(|(key, _)| key == name).unwrap().1.as_str();
    Store::with(
        rpc::Client::new(value("TASKS_SERVER"), "sf", value("TASKS_DATABASE")),
        Box::new(|| board::NOW),
        Box::new(|_| false),
        board::HOST,
    )
    .unwrap()
}

fn fixture_worker(store: &Store, root: &Path, kind: &str) -> String {
    store
        .register(&NewSession {
            worktree: root.to_string_lossy().into_owned(),
            ..NewSession::new(kind)
        })
        .unwrap()
        .row
        .handle
}

/// `tasks ARGS` on the board `env` reaches, run in `cwd`, as no session: its stdout, once it succeeded.
fn tasks(env: &[(String, String)], cwd: &Path, args: &[&str]) -> String {
    let done = sys::output(
        sys::Command::new(tasks_binary())
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .env_remove("TASKS_SESSION")
            .current_dir(cwd),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

const REGISTER: &str = "# Lamps

<!-- register:start -->
- [-] **lamp-a-pic** — Lamp A. Approve and say A or B.
  [2026-10-06 09:00] agent — A ![a](/a.png)
  [2026-10-06 10:00] owner · rejected — the turrets face inward.
- [x] **lamp-b-pic** — Lamp B.
  [2026-10-06 10:05] owner · approved — B
- [x] **lamp-c-pic** — Lamp C.
  [2026-10-06 10:06] owner · approved — A
  [2026-10-06 10:30] agent — Pick A recorded; **game/lineup** takes it in.
- [-] **ui-list-v1** — A list.
  [2026-10-06 10:10] owner · rejected — needs image explanation
- [ ] **ui-list-v10** — Pending, nobody has answered it.
<!-- register:end -->
";

const PAGE: &str = "docs/lamps/lamps.md";

fn tree() -> Tree {
    let tree = Tree::new();
    tree.write(PAGE, REGISTER);
    tree
}

fn slugs(root: &Path) -> Vec<String> {
    verdicts::open_cards(root)
        .unwrap()
        .into_iter()
        .map(|c| c.slug)
        .collect()
}

mod open {
    use super::*;

    #[test]
    fn every_card_waiting_on_an_agent_with_the_owners_words_and_time() {
        let tree = tree();
        let cards = verdicts::open_cards(&tree.root).unwrap();
        let slugs: Vec<&str> = cards.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, ["lamp-a-pic", "lamp-b-pic", "ui-list-v1"]);
        let first = &cards[0];
        assert_eq!(
            (
                first.path.as_str(),
                first.state.as_str(),
                first.branch.as_str()
            ),
            (PAGE, "rejected", "")
        );
        assert_eq!(first.quotes, ["the turrets face inward."]);
        assert_eq!(first.since.as_deref(), Some("2026-10-06 10:00"));
    }

    #[test]
    fn a_cards_ask_is_about_the_card_from_the_owners_last_line_in_one_line() {
        let tree = Tree::new();
        tree.write(
            PAGE,
            "<!-- register:start -->\n- [-] **a-card** — x\n  [2026-10-06 10:00] owner · rejected\n\
             - [-] **b-card** — x\n  [2026-10-06 10:00] owner · rejected — two\n    lines   here\n<!-- register:end -->\n",
        );
        let cards = verdicts::open_cards(&tree.root).unwrap();
        assert_eq!(
            cards[1].ask(),
            NewAsk {
                kind: "verdict".into(),
                path: Some(PAGE.into()),
                card: Some("b-card".into()),
                words: "two lines here".into(),
                at: epoch("2026-10-06 10:00"),
                ..NewAsk::default()
            }
        );
        // A card with no note is asked by its state.
        assert_eq!(cards[0].ask().words, "rejected");
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = sys::output(sys::Command::new("git").args(args).current_dir(root)).unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

const LAMP_D: &str = "- [-] **lamp-d-pic** — Lamp D, only on the branch.
  [2026-10-06 09:30] agent — D ![d](/d.png)
  [2026-10-06 10:15] owner · rejected — the fins are too long.
";
const LAMP_A_ANSWER: &str = "  [2026-10-06 10:30] agent — Redrawn on **pic/lamp-a-redraw**.\n";

/// The root is main's checkout. A worktree's register holds main's cards as of its fork as well as the
/// branch's own: only the branch's own count, as the Symphony lists them.
struct Branched {
    tree: Tree,
    trees: Tree,
}

impl Branched {
    fn new() -> Branched {
        let tree = tree();
        let root = &tree.root;
        git(root, &["init", "-q", "-b", "main"]);
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .append(true)
            .open(root.join(".git/config"))
            .unwrap()
            .write_all(
                b"\n[user]\n\temail = t@example.com\n\tname = t\n[commit]\n\tgpgsign = false\n",
            )
            .unwrap();
        git(root, &["add", "docs"]);
        git(root, &["commit", "-q", "-m", "base"]);
        Branched {
            tree,
            trees: Tree::new(),
        }
    }

    /// A worktree on a new branch off main, its register replaced by `text` and committed: a card
    /// reaches the owner once its branch posts it.
    fn branch(&self, name: &str, text: &str) -> PathBuf {
        let path = self.trees.root.join(name.replace('/', "-"));
        git(
            &self.tree.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                name,
                path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(path.join(PAGE), text).unwrap();
        git(
            &path,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-am",
                "post the branch's cards",
            ],
        );
        path
    }

    fn with_lamp_d(text: &str) -> String {
        text.replace(
            "<!-- register:end -->",
            &format!("{LAMP_D}<!-- register:end -->"),
        )
    }
}

mod branches {
    use super::*;

    #[test]
    fn a_card_open_only_on_a_branch_is_listed_under_its_branch_and_asked_on_it() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::open_cards(&b.tree.root).unwrap();
        let card = cards.iter().find(|c| c.slug == "lamp-d-pic").unwrap();
        assert_eq!(card.path, "@pic~x/docs/lamps/lamps.md");
        assert_eq!(card.branch, "pic/x");
        assert_eq!(card.quotes, ["the fins are too long."]);
        let ask = card.ask();
        assert_eq!(
            (
                ask.path.as_deref(),
                ask.tree.as_deref(),
                ask.card.as_deref()
            ),
            (Some(PAGE), Some("pic/x"), Some("lamp-d-pic"))
        );
    }

    #[test]
    fn a_card_main_answered_after_the_branch_forked_is_not_listed_from_the_branch() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER)); // its copy of lamp-a-pic still waits on an agent
        let answered = REGISTER.replace(
            "turrets face inward.\n",
            &format!("turrets face inward.\n{LAMP_A_ANSWER}"),
        );
        b.tree.write(PAGE, &answered);
        git(&b.tree.root, &["commit", "-q", "-am", "answer lamp-a-pic"]);
        let waiting = slugs(&b.tree.root);
        assert!(!waiting.contains(&"lamp-a-pic".to_string()), "{waiting:?}");
        assert!(waiting.contains(&"lamp-d-pic".to_string()), "{waiting:?}");
    }

    #[test]
    fn a_card_only_on_a_branch_that_waits_on_the_owner_is_not_listed() {
        let b = Branched::new();
        let asked = "- [-] **lamp-e-pic** — Lamp E.\n  [2026-10-06 10:20] owner · rejected — too pale.\n  [2026-10-06 10:40] agent · question — Paler trim, or a darker lamp?\n";
        b.branch(
            "pic/x",
            &REGISTER.replace(
                "<!-- register:end -->",
                &format!("{asked}<!-- register:end -->"),
            ),
        );
        assert!(!slugs(&b.tree.root).contains(&"lamp-e-pic".to_string()));
    }

    #[test]
    fn a_branch_card_identical_to_mains_is_listed_once_from_main() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::open_cards(&b.tree.root).unwrap();
        let paths: Vec<&str> = cards
            .iter()
            .filter(|c| c.slug == "lamp-a-pic")
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(paths, [PAGE]);
    }

    #[test]
    fn a_card_stacked_branches_carry_is_listed_once() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        b.branch("pic/y", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::open_cards(&b.tree.root).unwrap();
        assert_eq!(cards.iter().filter(|c| c.slug == "lamp-d-pic").count(), 1);
    }

    #[test]
    fn a_root_with_no_git_lists_only_main() {
        assert_eq!(
            slugs(&tree().root),
            ["lamp-a-pic", "lamp-b-pic", "ui-list-v1"]
        );
    }
}

mod actor {
    use super::*;

    /// The board's sessions in memory: (handle, kind, left).
    #[derive(Default)]
    struct Fake {
        sessions: RefCell<Vec<(String, String, bool)>>,
    }

    impl Board for Fake {
        fn app_session(&self) -> Result<Option<String>, VerdictError> {
            let sessions = self.sessions.borrow();
            Ok(sessions
                .iter()
                .find(|(_, kind, left)| kind == "app" && !left)
                .map(|(h, _, _)| h.clone()))
        }

        fn register_app(&self) -> Result<String, VerdictError> {
            let handle = format!("app-{}", self.sessions.borrow().len() + 1);
            self.sessions
                .borrow_mut()
                .push((handle.clone(), "app".into(), false));
            Ok(handle)
        }
    }

    #[test]
    fn the_app_acts_as_one_app_session_never_as_the_owner() {
        let board = Fake::default();
        let first = verdicts::app_actor(&board).unwrap();
        assert_eq!(verdicts::app_actor(&board).unwrap(), first);
        assert_eq!(board.sessions.borrow().len(), 1);
        assert_ne!(first, "owner");
    }

    #[test]
    fn an_app_session_that_left_is_not_reused() {
        let board = Fake::default();
        let gone = verdicts::app_actor(&board).unwrap();
        board.sessions.borrow_mut()[0].2 = true;
        assert_ne!(verdicts::app_actor(&board).unwrap(), gone);
    }

    #[test]
    fn the_shared_servers_store_finds_the_live_app_session_or_registers_one() {
        let env = board_env();
        let store = fixture_store(&env);
        fixture_worker(&store, Path::new("/x"), "worker");
        let app = verdicts::app_handle(&store).unwrap();
        assert_eq!(verdicts::app_handle(&store).unwrap(), app);
        let session = store.session(&app).unwrap();
        assert_eq!(
            (session.row.kind.as_str(), session.row.model.as_str()),
            ("app", "review-app")
        );
        store.leave(&app).unwrap();
        assert_ne!(verdicts::app_handle(&store).unwrap(), app);
    }
}

mod epochs {
    use super::*;

    #[test]
    fn a_stamp_and_a_bare_date_read_as_local_time() {
        let minute = epoch("2026-10-06 10:01").unwrap() - epoch("2026-10-06 10:00").unwrap();
        assert_eq!(minute, 60.0);
        let day = epoch("2026-10-07").unwrap() - epoch("2026-10-06").unwrap();
        assert!((82_800.0..=90_000.0).contains(&day), "{day}"); // a day, give or take a clock change
        assert!(epoch("2026-10-06 25:00").is_none());
        assert!(epoch("2026-02-30 10:00").is_none());
        assert!(epoch("soon").is_none());
    }
}

mod turning {
    //! `to_asks` on the real store, as `symphony verdict-asks` runs it.

    use super::*;

    struct Case {
        tree: Tree,
        store: Store,
        app: String,
    }

    /// The brief the filing wrote, listing `cards` (slug, line) of the lamps page.
    fn brief(cards: &[(&str, usize)]) -> String {
        let listed: Vec<String> = cards
            .iter()
            .map(|(slug, line)| {
                format!("- {PAGE}:{line} **{slug}** rejected (2026-10-06 10:00): \"x\"")
            })
            .collect();
        format!(
            "The owner's verdicts below wait on an agent.\n\nCards:\n{}\n",
            listed.join("\n")
        )
    }

    impl Case {
        fn new() -> Case {
            let tree = tree();
            let store = fixture_store(&board_env());
            let app = verdicts::app_handle(&store).unwrap();
            Case { tree, store, app }
        }

        fn verdict_task(&self, slug: &str, body: &str) {
            self.store
                .create(&NewTask {
                    body: body.into(),
                    ..NewTask::new(
                        slug,
                        "Answer 2 owner verdicts waiting on an agent",
                        &self.app,
                    )
                })
                .unwrap();
        }

        fn turn(&self) -> Vec<String> {
            let app = self.app.clone();
            verdicts::to_asks(&self.tree.root, &self.store, move || Ok(app)).unwrap()
        }

        fn said(&self, slug: &str) -> Vec<String> {
            self.store
                .thread(slug)
                .unwrap()
                .into_iter()
                .map(|m| m.text)
                .collect()
        }
    }

    /// A turning that must not need the app's session.
    fn no_session() -> Result<String, String> {
        panic!("no task to turn: no session is asked for")
    }

    #[test]
    fn each_listed_card_that_still_waits_becomes_an_ask_keeping_the_owners_time_and_the_task_is_cancelled()
     {
        let case = Case::new();
        // lamp-c-pic was answered since; lamp-b-pic waits but no task listed it; ui-list-v1 joined on
        // the thread.
        case.verdict_task(
            "verdicts/2026-10-06-1100",
            &brief(&[("lamp-a-pic", 4), ("lamp-c-pic", 9)]),
        );
        case.store
            .say(
                "verdicts/2026-10-06-1100",
                &case.app,
                &format!(
                    "More verdicts for this task:\n- {PAGE}:12 **ui-list-v1** rejected: \"y\""
                ),
                None,
            )
            .unwrap();
        assert_eq!(
            case.turn(),
            ["verdicts/2026-10-06-1100 cancelled: 2 ask(s)"]
        );
        let asks = case.store.asks(None).unwrap();
        let about: Vec<_> = asks
            .iter()
            .map(|a| {
                (
                    a.card.as_deref().unwrap(),
                    a.words.as_str(),
                    a.asked,
                    a.path.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            about,
            [
                (
                    "lamp-a-pic",
                    "the turrets face inward.",
                    epoch("2026-10-06 10:00").unwrap(),
                    Some(PAGE)
                ),
                (
                    "ui-list-v1",
                    "needs image explanation",
                    epoch("2026-10-06 10:10").unwrap(),
                    Some(PAGE)
                ),
            ]
        );
        assert!(asks.iter().all(|a| a.kind == "verdict" && a.open));
        assert_eq!(
            case.store.get("verdicts/2026-10-06-1100").unwrap().status,
            "cancelled"
        );
        let line = case
            .said("verdicts/2026-10-06-1100")
            .into_iter()
            .find(|text| text.starts_with(TURNED))
            .unwrap();
        assert_eq!(
            line,
            format!(
                "{TURNED}: ask {} (lamp-a-pic in {PAGE}), ask {} (ui-list-v1 in {PAGE}). \
                 `just tasks asks` lists them; this task is cancelled.",
                asks[0].id, asks[1].id
            )
        );
        // Turned once: a second run finds nothing to turn and asks for no session.
        let again = verdicts::to_asks(&case.tree.root, &case.store, no_session).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(case.store.asks(None).unwrap(), asks);
    }

    #[test]
    fn a_task_an_executor_holds_stays_with_its_holder_and_its_line_says_so() {
        let case = Case::new();
        case.verdict_task("verdicts/held", &brief(&[("lamp-a-pic", 4)]));
        let worker = fixture_worker(&case.store, &case.tree.root, "worker");
        case.store.claim("verdicts/held", &worker).unwrap();
        let done = case.turn();
        assert_eq!(done.len(), 1);
        assert!(
            done[0].starts_with("verdicts/held kept by its holder (")
                && done[0].ends_with(": 1 ask(s)"),
            "{done:?}"
        );
        assert_eq!(
            case.store.get("verdicts/held").unwrap().status,
            "in_progress"
        );
        // The line is posted after the cancel was tried, and says what happened.
        let said = case.said("verdicts/held");
        let line = said
            .iter()
            .find(|text| text.starts_with(TURNED))
            .expect("the turning's line");
        assert!(!line.contains("this task is cancelled"), "{line}");
        assert!(
            line.contains("This task is not cancelled: the app's session may not cancel it (")
                && line.ends_with(
                    "so it stays with its holder, who cancels it once the asks have it."
                ),
            "{line}"
        );
        assert_eq!(case.store.asks(None).unwrap().len(), 1);
        assert!(case.turn().is_empty(), "its line says it was turned");
        assert_eq!(case.store.asks(None).unwrap()[0].lines, 1);
    }

    #[test]
    fn a_task_none_of_whose_cards_waits_is_cancelled_saying_so() {
        let case = Case::new();
        case.verdict_task("verdicts/answered", &brief(&[("lamp-c-pic", 9)]));
        assert_eq!(case.turn(), ["verdicts/answered cancelled: 0 ask(s)"]);
        assert!(case.store.asks(None).unwrap().is_empty());
        assert!(case.said("verdicts/answered").contains(&format!(
            "{TURNED}, and none of its cards waits on an agent: this task is cancelled."
        )));
    }

    #[test]
    fn an_ask_open_about_the_card_takes_the_turning_as_one_more_line() {
        let case = Case::new();
        let open = case
            .store
            .open_ask(&verdicts::open_cards(&case.tree.root).unwrap()[0].ask())
            .unwrap();
        case.verdict_task("verdicts/x", &brief(&[("lamp-a-pic", 4)]));
        case.turn();
        let asks = case.store.asks(None).unwrap();
        assert_eq!((asks.len(), asks[0].id, asks[0].lines), (1, open.id, 2));
    }

    #[test]
    fn finished_verdict_tasks_and_other_tasks_are_left_alone_and_no_session_is_asked_for() {
        let case = Case::new();
        case.verdict_task("verdicts/cancelled", &brief(&[("lamp-a-pic", 4)]));
        case.store
            .move_to("verdicts/cancelled", "cancelled", &case.app)
            .unwrap();
        case.verdict_task("verdicts/done", &brief(&[("lamp-a-pic", 4)]));
        case.store
            .raw()
            .row("task", "verdicts/done")
            .set(serde_json::json!({"status": "done"}))
            .unwrap();
        case.store
            .create(&NewTask {
                body: brief(&[("lamp-a-pic", 4)]),
                ..NewTask::new("pic/lamp-a-redraw", "Redraw lamp A", &case.app)
            })
            .unwrap();
        let done = verdicts::to_asks(&case.tree.root, &case.store, no_session).unwrap();
        assert!(done.is_empty(), "{done:?}");
        assert!(case.store.asks(None).unwrap().is_empty());
        assert_eq!(case.store.get("pic/lamp-a-redraw").unwrap().status, "ready");
    }
}

mod closing {
    //! `owner_asks::keep` on the real store: what a scan reads answered is answered on the board,
    //! credited to who replied; what it reads gone is marked and left open; what it cannot read
    //! changes nothing.

    use super::*;

    fn queued(slug: &str) -> symphony_api::QueueItem {
        symphony_api::QueueItem {
            href: format!("/{PAGE}#{slug}"),
            path: PAGE.into(),
            slug: slug.into(),
            title: "Lamps".into(),
            kind: "rejected".into(),
            text: "the turrets face inward.".into(),
            branch: None,
            ask: None,
        }
    }

    /// A scan of `root` with `agent` waiting and `held` on main's page, after the owner's lines:
    /// main's copies carry them, the last a minute before the read.
    fn scan(root: &Path, agent: Vec<symphony_api::QueueItem>, held: &[&str]) -> Reading {
        let read_at = epoch("2026-10-06 12:00").unwrap();
        Reading {
            root: root.to_path_buf(),
            agent,
            held: std::sync::Arc::new(
                held.iter()
                    .map(|slug| ((PAGE.to_string(), slug.to_string()), Some(read_at - 60.0)))
                    .collect(),
            ),
            read_at,
            listed: true,
            ..Reading::default()
        }
    }

    #[test]
    fn an_ask_the_scan_reads_answered_is_answered_and_one_still_waiting_stays_open() {
        let store = fixture_store(&board_env());
        let tree = tree();
        let cards = verdicts::open_cards(&tree.root).unwrap();
        let waiting = store.open_ask(&cards[0].ask()).unwrap();
        let answered = store.open_ask(&cards[2].ask()).unwrap();
        let reading = scan(
            &tree.root,
            vec![queued("lamp-a-pic")],
            &["lamp-a-pic", "ui-list-v1"],
        );
        let open = store.asks(None).unwrap();
        assert_eq!(
            owner_asks::keep(&store, &open, &reading),
            [format!("ask {} answered: {PAGE}#ui-list-v1", answered.id)]
        );
        let now = store.ask(answered.id).unwrap();
        assert_eq!(
            (now.open, now.answer.as_deref(), now.answered_by.as_deref()),
            (false, Some("docs/lamps/lamps.md#ui-list-v1"), None)
        );
        assert!(store.ask(waiting.id).unwrap().open);
        // Read again, it closes nothing more.
        let open = store.asks(None).unwrap();
        assert!(owner_asks::keep(&store, &open, &reading).is_empty());
    }

    #[test]
    fn an_answer_is_credited_to_the_session_that_replied() {
        let store = fixture_store(&board_env());
        let tree = tree();
        let card = verdicts::open_cards(&tree.root).unwrap()[2].clone();
        let ask = store.open_ask(&card.ask()).unwrap();
        let mut reading = scan(&tree.root, Vec::new(), &["ui-list-v1"]);
        reading.replies = vec![symphony::asks::Reply {
            tree: String::new(),
            path: PAGE.into(),
            kind: "card".into(),
            target: "ui-list-v1".into(),
            by: "amber-heron".into(),
            at: ask.last + 1.0,
        }];
        let open = store.asks(None).unwrap();
        assert_eq!(
            owner_asks::keep(&store, &open, &reading),
            [format!(
                "ask {} answered by amber-heron: {PAGE}#ui-list-v1",
                ask.id
            )]
        );
        assert_eq!(
            store.ask(ask.id).unwrap().answered_by.as_deref(),
            Some("amber-heron")
        );
    }

    #[test]
    fn an_ask_whose_card_or_worktree_went_stays_open_marked_gone_until_it_is_found() {
        let store = fixture_store(&board_env());
        let tree = tree();
        let card = verdicts::open_cards(&tree.root).unwrap()[2].clone();
        let ask = store.open_ask(&card.ask()).unwrap();
        let branch = store
            .open_ask(&NewAsk {
                tree: Some("pic/gone".into()),
                ..card.ask()
            })
            .unwrap();
        let gone = scan(&tree.root, Vec::new(), &[]);
        let open = store.asks(None).unwrap();
        let mut said = owner_asks::keep(&store, &open, &gone);
        said.sort();
        assert_eq!(
            said,
            [
                format!(
                    "ask {} left open, gone: {PAGE} has no card ui-list-v1",
                    ask.id
                ),
                format!(
                    "ask {} left open, gone: the worktree of pic/gone is gone, and main's {PAGE} \
                     has no card ui-list-v1",
                    branch.id
                ),
            ]
        );
        for id in [ask.id, branch.id] {
            let now = store.ask(id).unwrap();
            assert!(
                now.open && now.answered.is_none() && now.answer.is_none(),
                "{now:?}"
            );
            assert!(now.gone.is_some() && now.gone_since.is_some(), "{now:?}");
        }
        // The same reading says nothing more; a failed listing changes nothing on the branch's.
        let open = store.asks(None).unwrap();
        assert!(owner_asks::keep(&store, &open, &gone).is_empty());
        let unlisted = Reading {
            listed: false,
            ..scan(&tree.root, Vec::new(), &[])
        };
        assert!(owner_asks::keep(&store, &open, &unlisted).is_empty());
        // Found waiting again, the mark comes off; answered, it closes.
        let back = scan(&tree.root, vec![queued("ui-list-v1")], &["ui-list-v1"]);
        let mut said = owner_asks::keep(&store, &open, &back);
        said.sort();
        assert_eq!(
            said,
            [
                format!("ask {} waits where it was again", ask.id),
                format!("ask {} waits where it was again", branch.id),
            ]
        );
        assert!(store.ask(ask.id).unwrap().gone.is_none());
        let open = store.asks(None).unwrap();
        let answered = scan(&tree.root, Vec::new(), &["ui-list-v1"]);
        assert_eq!(owner_asks::keep(&store, &open, &answered).len(), 2);
        assert!(store.asks(None).unwrap().is_empty());
    }

    fn comment_item(line: usize, title: &str) -> symphony_api::QueueItem {
        symphony_api::QueueItem {
            href: format!("/docs/notes.md#L{line}"),
            path: "docs/notes.md".into(),
            slug: format!("L{line}"),
            title: "Notes".into(),
            kind: "dislike".into(),
            text: title.into(),
            branch: None,
            ask: None,
        }
    }

    /// The second open question's other answer, built but off: each comment open before asks gets one,
    /// aged from the commit that wrote it, once.
    #[test]
    fn comments_open_before_asks_are_asked_once_from_when_they_were_written() {
        let b = Branched::new();
        b.tree.write(
            "docs/notes.md",
            "# Notes\n\n> [!dislike] Too long\n> Cut it.\n",
        );
        let before = owner_asks::now().floor();
        git(&b.tree.root, &["add", "docs/notes.md"]);
        git(&b.tree.root, &["commit", "-q", "-m", "a comment"]);
        let after = owner_asks::now().ceil();
        b.tree.write(
            "docs/notes.md",
            "# Notes\n\n> [!dislike] Too long\n> Cut it.\n\n> [!question] Why blue?\n",
        );
        let store = fixture_store(&board_env());
        let queue = [comment_item(3, "Too long"), comment_item(6, "Why blue?")];
        let opened = owner_asks::open_comments(&b.tree.root, &store, &queue).unwrap();
        assert_eq!(opened.len(), 2, "{opened:?}");
        let asks = store.asks(None).unwrap();
        let committed = asks
            .iter()
            .find(|a| a.block.as_deref() == Some("Too long"))
            .unwrap();
        assert!(
            (before..=after).contains(&committed.asked),
            "{before} <= {} <= {after}",
            committed.asked
        );
        assert_eq!(
            (
                committed.kind.as_str(),
                committed.path.as_deref(),
                committed.words.as_str()
            ),
            ("comment", Some("docs/notes.md"), "Too long")
        );
        let uncommitted = asks
            .iter()
            .find(|a| a.block.as_deref() == Some("Why blue?"))
            .unwrap();
        assert!(uncommitted.asked >= after - 1.0, "{}", uncommitted.asked);
        // Once: a comment an ask is about already, open or answered, gets no second one.
        store
            .answer_ask(committed.id, None, "answered elsewhere")
            .unwrap();
        assert!(
            owner_asks::open_comments(&b.tree.root, &store, &queue)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.asks(Some(0.0)).unwrap().len(), 2);
    }
}

mod words {
    use symphony::taskcli::shell_words;

    #[test]
    fn a_command_is_split_as_a_shell_splits_it() {
        assert_eq!(shell_words("run -B tools/x"), ["run", "-B", "tools/x"]);
        assert_eq!(shell_words("  a   b  "), ["a", "b"]);
        assert_eq!(
            shell_words(r#"run 'a b' "c d" e\ f"#),
            ["run", "a b", "c d", "e f"]
        );
        assert_eq!(
            shell_words(r#"say "it's" 'say "hi"' x"y"z"#),
            ["say", "it's", "say \"hi\"", "xyz"]
        );
        assert_eq!(shell_words(r#"empty '' end"#), ["empty", "", "end"]);
        assert!(shell_words("   ").is_empty());
    }
}

mod real {
    //! The binary a recipe runs, on a board of its own.

    use super::*;
    use symphony_board::live::seen;

    fn review(tree: &Tree, env: &[(String, String)], args: &[&str]) -> (i32, String, String) {
        review_in(&tree.root, env, args)
    }

    /// `symphony ARGS --root ROOT` with `env`, as no session.
    fn review_in(root: &Path, env: &[(String, String)], args: &[&str]) -> (i32, String, String) {
        let done = sys::output(
            sys::Command::new(crate::review_binary())
                .args(args)
                .args(["--root", root.to_str().unwrap()])
                .envs(env.iter().map(|(k, v)| (k, v)))
                .env("SYMPHONY_TASKS_CLI", tasks_binary())
                .env_remove("TASKS_SESSION"),
        )
        .unwrap();
        (
            done.status.code().unwrap(),
            String::from_utf8_lossy(&done.stdout).to_string(),
            String::from_utf8_lossy(&done.stderr).to_string(),
        )
    }

    #[test]
    fn triage_prints_the_ranking_of_the_docs() {
        let tree = tree();
        let env = board_env();
        let (code, out, err) = review(&tree, &env, &["triage", "queue", "2"]);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.starts_with(
                " 1. ui-list-v10  docs/lamps/lamps.md\n    design · gates nothing known"
            ),
            "{out}"
        );
        let (code, out, _) = review(&tree, &env, &["triage", "stats"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("FIRST-PASS APPROVAL"), "{out}");
    }

    /// `symphony verdict-asks`, the one command that turns the verdict tasks: on a scratch board
    /// from anywhere, as the app's own session, once.
    #[test]
    fn verdict_asks_turns_the_open_verdict_tasks_once_and_says_what_it_did() {
        let tree = tree();
        let env = board_env();
        let store = fixture_store(&env);
        let filer = fixture_worker(&store, &tree.root, "worker");
        store
            .create(&NewTask {
                body: format!(
                    "Cards:\n- {PAGE}:4 **lamp-a-pic** rejected (2026-10-06 10:00): \"x\"\n"
                ),
                ..NewTask::new("verdicts/2026-10-06-1100", "Answer the owner", &filer)
            })
            .unwrap();
        let (code, out, err) = review(&tree, &env, &["verdict-asks"]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (0, "verdicts/2026-10-06-1100 cancelled: 1 ask(s)\n", "")
        );
        let asks = store.asks(None).unwrap();
        assert_eq!(
            asks.iter()
                .map(|a| (a.kind.as_str(), a.card.as_deref()))
                .collect::<Vec<_>>(),
            [("verdict", Some("lamp-a-pic"))]
        );
        let by = store.thread("verdicts/2026-10-06-1100").unwrap();
        let turned = by.iter().find(|m| m.text.starts_with(TURNED)).unwrap();
        let app = store
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.row.handle == turned.sender)
            .unwrap();
        assert_eq!(
            app.row.kind, "app",
            "the app's own session, never the owner"
        );
        let (code, out, err) = review(&tree, &env, &["verdict-asks"]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (
                0,
                "no open verdict task to turn into asks: nothing changed\n",
                ""
            )
        );
        assert_eq!(store.asks(None).unwrap(), asks);
    }

    /// On a project's live board it runs only in the main checkout, as `just tasks migrate` does:
    /// from a linked worktree it is refused before it reads; from the main checkout a board not at
    /// this build's schema yet (one whose pin moved before `just tasks migrate`) is refused by the
    /// board's own rule, naming `just tasks migrate`. Neither writes anything.
    #[test]
    fn verdict_asks_moves_a_live_board_only_from_its_main_checkout() {
        let b = Branched::new();
        let wt = b.branch("pic/x", REGISTER);
        let (_server, env) = live_board(&b);
        // The board as the build before asks left it: made at the schema one behind this build's,
        // and seen at no other, as its record says.
        let version = symphony_board::schema::VERSION;
        older_board(live_client(&env), version - 1, board::NOW);
        let record = || seen::read(&b.tree.root.join(".git"));
        let behind = Some(seen::Seen {
            version: version - 1,
            at: board::NOW,
        });
        assert_eq!(record(), behind);
        let (code, out, err) = review_in(&wt, &env, &["verdict-asks"]);
        assert_eq!((code, out.as_str()), (2, ""), "{err}");
        assert!(
            err.starts_with("✖ `symphony verdict-asks` turns the ")
                && err.contains("board's verdict tasks only from its main checkout, ")
                && err.contains("nothing was read or written"),
            "{err}"
        );
        let (code, out, err) = review_in(&b.tree.root, &env, &["verdict-asks"]);
        assert_eq!((code, out.as_str()), (2, ""), "{err}");
        assert!(
            err.contains(&format!(
                "is at schema {}, behind this build's {version}",
                version - 1
            )) && err.contains("`just tasks migrate` in the main checkout"),
            "{err}"
        );
        assert_eq!(
            held(&env),
            [
                serde_json::json!(version - 1),
                serde_json::json!(0),
                serde_json::json!(0)
            ],
            "the board stayed behind, and nothing was written"
        );
        assert_eq!(record(), behind, "nor was its record");
    }

    /// A live board that went back, seen at this build's schema and its store put back one schema,
    /// is no board awaiting `just tasks migrate`: from the main checkout it is refused in the
    /// rollback's own words (`symphony_board::live::seen`), and nothing is read or written, its
    /// record included.
    #[test]
    fn verdict_asks_refuses_a_live_board_that_went_back_as_the_rollback_it_is() {
        let b = Branched::new();
        let (_server, env) = live_board(&b);
        // Made by this build, whose opening records its schema; then its store goes back one.
        let version = symphony_board::schema::VERSION;
        fixture_store(&env);
        symphony_board::Raw::on(&live_client(&env))
            .row("meta", "schema")
            .set(serde_json::json!({"version": version - 1}))
            .unwrap();
        let common = b.tree.root.join(".git");
        let why = seen::Rollback {
            project: b.tree.root.file_name().unwrap().to_string_lossy().into(),
            from: version,
            since: board::NOW,
            to: Some(version - 1),
            record: seen::path(&common),
        }
        .refusal();
        let (code, out, err) = review_in(&b.tree.root, &env, &["verdict-asks"]);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", format!("✖ {why}\n").as_str())
        );
        assert_eq!(
            held(&env),
            [
                serde_json::json!(version - 1),
                serde_json::json!(0),
                serde_json::json!(0)
            ],
            "the board stayed where it went back to, and nothing was written"
        );
        assert_eq!(
            seen::read(&common),
            Some(seen::Seen {
                version,
                at: board::NOW
            }),
            "and its record still says where it was"
        );
    }

    /// The fixture repository's own board, `b`'s main checkout's: the server on its common dir's
    /// data, which is how an address is known for the live board's
    /// (`symphony_board::live::served`), and the variables that reach it.
    fn live_board(b: &Branched) -> (Serving, [(String, String); 2]) {
        let server = Serving(symphony_board::server::Home::in_common(
            &b.tree.root.join(".git"),
            Some(&board::surreal()),
        ));
        let addr = server.0.ensure().unwrap().addr;
        let env = [
            ("TASKS_SERVER".to_string(), addr),
            ("TASKS_DATABASE".to_string(), "tasks".to_string()),
        ];
        (server, env)
    }

    /// A raw client of the board `env` reaches, which reads no record and opens no store.
    fn live_client(env: &[(String, String)]) -> rpc::Client {
        rpc::Client::new(&env[0].1, "sf", "tasks")
    }

    /// What a refused run must leave on the board `env` reaches as it was: its schema, and how
    /// many sessions and asks it holds.
    fn held(env: &[(String, String)]) -> Vec<serde_json::Value> {
        let client = live_client(env);
        let board = symphony_board::Raw::on(&client);
        vec![
            board.row("meta", "schema").field("version").unwrap(),
            serde_json::json!(board.table("session").count().unwrap()),
            serde_json::json!(board.table("ask").count().unwrap()),
        ]
    }

    /// A board server this test started, stopped when the test ends, however it ends.
    struct Serving(symphony_board::server::Home);

    impl Drop for Serving {
        fn drop(&mut self) {
            let _ = self.0.stop_board();
        }
    }
}

mod contract {
    //! `CliBoard`, the board a verdict commit finds the app's session through, on the real store
    //! through the `tasks` binary.

    use symphony::taskcli::CliBoard;

    use super::*;

    fn board(tree: &Tree, env: &[(String, String)]) -> CliBoard {
        CliBoard::with_command(vec![tasks_binary().to_string()], env, &tree.root)
    }

    #[test]
    fn the_app_session_is_the_live_app_one_and_a_left_one_is_not() {
        let tree = Tree::new();
        let env = board_env();
        fixture_worker(&fixture_store(&env), &tree.root, "worker");
        let board = board(&tree, &env);
        assert_eq!(board.app_session().unwrap(), None);
        let app = board.register_app().unwrap();
        assert_eq!(board.app_session().unwrap().as_deref(), Some(app.as_str()));
        tasks(&env, &tree.root, &["--as", &app, "leave"]);
        assert_eq!(board.app_session().unwrap(), None);
        assert!(!verdicts::app_actor(&board).unwrap().is_empty());
        assert_ne!(verdicts::app_actor(&board).unwrap(), app);
    }

    #[test]
    fn a_board_it_cannot_reach_or_a_binary_that_cannot_run_is_a_refusal_naming_it() {
        let tree = Tree::new();
        let gone = [("TASKS_SERVER".to_string(), "127.0.0.1:1".to_string())];
        let why = board(&tree, &gone).app_session().unwrap_err();
        assert!(why.is_unreachable(), "{why}");
        let missing = tree.path("no-such-binary");
        let board = CliBoard::with_command(
            vec![missing.to_string_lossy().to_string()],
            &gone,
            &tree.root,
        );
        let why = board.app_session().unwrap_err();
        assert!(
            why.0.starts_with("cannot run ") && !why.is_unreachable(),
            "{why}"
        );
    }
}
