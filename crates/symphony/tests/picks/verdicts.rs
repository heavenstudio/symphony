//! An owner verdict waiting on an agent becomes board work, so the Symphony's "waiting on an
//! agent" count cannot grow while nobody remembers to triage it. The failures it exists to prevent:
//! a card left with no task because an older brief happened to name its slug, a done task counted as
//! acting on a new verdict, and a second task filed for a card already on one.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use symphony_board::{NewSession, Store, rpc, sys};

use symphony::verdicts::{self, Board, BoardTask, VerdictError, epoch};

use super::board;
use super::common::{Tree, tasks_binary};

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

/// Fixture writes use the native store; assertions still exercise the real CLI board.
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

const PENDING_V10: &str = "- [ ] **ui-list-v10** — Pending, nobody has answered it.";

fn rejected_v10(at: &str) -> String {
    format!("- [-] **ui-list-v10** — A list.\n  [{at}] owner · rejected — too much text")
}

/// The board as the code under test sees it: rows in memory, a clock the test moves.
struct Fake {
    now: RefCell<f64>,
    tasks: RefCell<Vec<BoardTask>>,
    sessions: RefCell<Vec<(String, String, bool)>>,
    said: RefCell<Vec<(String, String, String)>>,
    created: RefCell<Vec<(String, String, Vec<String>)>>,
    /// (slug, by, links added) of each amendment.
    amended: RefCell<Vec<(String, String, Vec<String>)>>,
    /// How many amendments to come the board refuses.
    refuse_amends: RefCell<usize>,
}

impl Fake {
    fn at(stamp: &str) -> Fake {
        Fake {
            now: RefCell::new(epoch(stamp).unwrap()),
            tasks: RefCell::default(),
            sessions: RefCell::default(),
            said: RefCell::default(),
            created: RefCell::default(),
            amended: RefCell::default(),
            refuse_amends: RefCell::default(),
        }
    }

    fn set(&self, stamp: &str) {
        *self.now.borrow_mut() = epoch(stamp).unwrap();
    }

    fn task(&self, slug: &str, body: &str, at: &str, status: &str) {
        self.tasks.borrow_mut().push(BoardTask {
            slug: slug.into(),
            title: "a task".into(),
            body: body.into(),
            status: status.into(),
            created: epoch(at).unwrap(),
            executors: 0,
            reserved: 0,
            thread: Vec::new(),
            links: Vec::new(),
        });
    }

    fn get(&self, slug: &str) -> BoardTask {
        self.tasks
            .borrow()
            .iter()
            .find(|t| t.slug == slug)
            .unwrap()
            .clone()
    }

    fn change(&self, slug: &str, how: impl FnOnce(&mut BoardTask)) {
        let mut tasks = self.tasks.borrow_mut();
        how(tasks.iter_mut().find(|t| t.slug == slug).unwrap());
    }

    fn rolled(&self) -> Vec<String> {
        self.tasks
            .borrow()
            .iter()
            .filter(|t| t.slug.starts_with("verdicts/"))
            .map(|t| t.slug.clone())
            .collect()
    }

    fn body(&self, slug: &str) -> String {
        self.tasks
            .borrow()
            .iter()
            .find(|t| t.slug == slug)
            .unwrap()
            .body
            .clone()
    }
}

impl Board for Fake {
    fn now(&self) -> f64 {
        *self.now.borrow()
    }

    fn tasks(&self) -> Result<Vec<BoardTask>, VerdictError> {
        Ok(self.tasks.borrow().clone())
    }

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

    fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), VerdictError> {
        let at = self.now();
        self.said
            .borrow_mut()
            .push((slug.into(), by.into(), text.into()));
        self.change(slug, |t| t.thread.push((at, text.into())));
        Ok(())
    }

    fn create(
        &self,
        slug: &str,
        title: &str,
        by: &str,
        body: &str,
        links: &[String],
    ) -> Result<(), VerdictError> {
        let at = self.now();
        self.created
            .borrow_mut()
            .push((slug.into(), by.into(), links.to_vec()));
        self.tasks.borrow_mut().push(BoardTask {
            slug: slug.into(),
            title: title.into(),
            body: body.into(),
            status: "ready".into(),
            created: at,
            executors: 0,
            reserved: 0,
            thread: Vec::new(),
            links: links.to_vec(),
        });
        Ok(())
    }

    fn amend(
        &self,
        slug: &str,
        title: &str,
        links: &[String],
        by: &str,
    ) -> Result<(), VerdictError> {
        let mut refuse = self.refuse_amends.borrow_mut();
        if *refuse > 0 {
            *refuse -= 1;
            return Err(VerdictError::unreachable("the board refused the amendment"));
        }
        drop(refuse);
        self.amended
            .borrow_mut()
            .push((slug.into(), by.into(), links.to_vec()));
        self.change(slug, |t| {
            t.title = title.into();
            t.links.extend(links.iter().cloned());
        });
        Ok(())
    }
}

struct Case {
    tree: Tree,
    board: Fake,
}

impl Case {
    fn new() -> Case {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        Case {
            tree,
            board: Fake::at("2026-10-06 11:00"),
        }
    }

    fn doc(&self, text: &str) {
        self.tree.write("docs/lamps/lamps.md", text);
    }

    fn untasked(&self) -> Vec<String> {
        verdicts::untasked(&self.tree.root, &self.board)
            .unwrap()
            .into_iter()
            .map(|c| c.slug)
            .collect()
    }

    fn file(&self, by: &str) -> Option<String> {
        verdicts::file(&self.tree.root, &self.board, by).unwrap()
    }
}

mod untasked {
    use super::*;

    #[test]
    fn lists_each_card_waiting_on_an_agent_with_the_owners_words() {
        let case = Case::new();
        let cards = verdicts::untasked(&case.tree.root, &case.board).unwrap();
        let slugs: Vec<&str> = cards.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(slugs, ["lamp-a-pic", "lamp-b-pic", "ui-list-v1"]);
        let first = &cards[0];
        assert_eq!(first.path, "docs/lamps/lamps.md");
        assert_eq!(first.state, "rejected");
        assert_eq!(first.quotes, ["the turrets face inward."]);
        assert_eq!(first.since.as_deref(), Some("2026-10-06 10:00"));
    }

    #[test]
    fn a_live_task_written_after_the_verdict_that_names_the_card_takes_it() {
        let case = Case::new();
        case.board.task(
            "pic/lamp-a-redraw",
            "Redraw **lamp-a-pic**: turrets outward.",
            "2026-10-06 10:20",
            "ready",
        );
        assert!(!case.untasked().contains(&"lamp-a-pic".to_string()));
    }

    #[test]
    fn a_brief_written_the_same_minute_as_the_verdict_counts() {
        let case = Case::new();
        case.board.task(
            "pic/lamp-a-redraw",
            "Redraw lamp-a-pic.",
            "2026-10-06 10:00",
            "ready",
        );
        assert!(!case.untasked().contains(&"lamp-a-pic".to_string()));
    }

    #[test]
    fn a_brief_written_before_the_verdict_does_not_act_on_it() {
        let case = Case::new();
        case.board.task(
            "pic/lamp-a-first",
            "Draw lamp-a-pic.",
            "2026-10-06 08:00",
            "ready",
        );
        assert!(case.untasked().contains(&"lamp-a-pic".to_string()));
    }

    #[test]
    fn a_thread_message_after_the_verdict_names_the_card() {
        let case = Case::new();
        case.board.task(
            "pic/lamp-a-first",
            "Draw lamp A.",
            "2026-10-06 08:00",
            "ready",
        );
        case.board.set("2026-10-06 10:40");
        case.board
            .say(
                "pic/lamp-a-first",
                "w",
                "Redrawing lamp-a-pic after the owner's rejection.",
            )
            .unwrap();
        assert!(!case.untasked().contains(&"lamp-a-pic".to_string()));
    }

    #[test]
    fn a_brief_from_before_and_a_message_after_the_verdict_take_it() {
        let case = Case::new();
        case.board.task(
            "pic/lamp-a-first",
            "Draw lamp-a-pic.",
            "2026-10-06 08:00",
            "ready",
        );
        case.board.set("2026-10-06 10:40");
        case.board
            .say("pic/lamp-a-first", "w", "Redrawing lamp-a-pic again.")
            .unwrap();
        assert!(!case.untasked().contains(&"lamp-a-pic".to_string()));
    }

    #[test]
    fn a_done_or_cancelled_task_acts_on_nothing() {
        for status in ["cancelled", "done"] {
            let case = Case::new();
            case.board.task(
                "pic/lamp-a-redraw",
                "Redraw lamp-a-pic.",
                "2026-10-06 10:20",
                status,
            );
            assert!(
                case.untasked().contains(&"lamp-a-pic".to_string()),
                "{status}"
            );
        }
    }

    #[test]
    fn a_slug_inside_a_longer_slug_is_not_a_mention() {
        let case = Case::new();
        case.board.task(
            "design/ui-list-v10",
            "Answer ui-list-v10 and ui-list-v1x.",
            "2026-10-06 10:30",
            "ready",
        );
        assert!(case.untasked().contains(&"ui-list-v1".to_string()));
    }

    #[test]
    fn a_slug_ending_a_longer_slug_is_not_a_mention() {
        let case = Case::new();
        case.board.task(
            "design/old-ui-list",
            "Answer old-ui-list-v1 and x_ui-list-v1.",
            "2026-10-06 10:30",
            "ready",
        );
        assert!(case.untasked().contains(&"ui-list-v1".to_string()));
    }

    #[test]
    fn a_slug_next_to_punctuation_is_a_mention() {
        let case = Case::new();
        case.board.task(
            "design/x",
            "Answer (ui-list-v1), `lamp-a-pic`; **lamp-b-pic**.",
            "2026-10-06 10:30",
            "ready",
        );
        assert!(case.untasked().is_empty());
    }
}

mod filing {
    use super::*;

    #[test]
    fn files_one_task_naming_every_untasked_card_with_the_owners_words() {
        let case = Case::new();
        let slug = case.file("w").unwrap();
        assert_eq!(slug, "verdicts/2026-10-06-1100");
        let body = case.board.body(&slug);
        for words in [
            "lamp-a-pic",
            "the turrets face inward.",
            "docs/lamps/lamps.md",
            "lamp-b-pic",
            "ui-list-v1",
            "needs image explanation",
            "the apply-review skill",
        ] {
            assert!(body.contains(words), "{words}: {body}");
        }
        assert!(!body.contains("lamp-c-pic"));
        let created = case.board.created.borrow();
        assert_eq!(created[0].1, "w");
        assert_eq!(created[0].2, ["docs/lamps/lamps.md"]);
        drop(created);
        assert!(case.untasked().is_empty());
    }

    /// The title the filing gave the task it filed.
    fn titled(case: &Case) -> String {
        let slug = case.file("w").unwrap();
        let tasks = case.board.tasks.borrow();
        tasks.iter().find(|t| t.slug == slug).unwrap().title.clone()
    }

    fn rejected(slugs: &[&str]) -> String {
        let cards: String = slugs
            .iter()
            .map(|slug| {
                format!("- [-] **{slug}** — A card.\n  [2026-10-06 10:00] owner · rejected — no.\n")
            })
            .collect();
        format!("<!-- register:start -->\n{cards}<!-- register:end -->\n")
    }

    #[test]
    fn the_task_is_titled_with_the_cards_it_answers() {
        assert_eq!(
            titled(&Case::new()),
            "Answer the owner on lamp-a-pic, lamp-b-pic and 1 more"
        );
        let case = Case::new();
        case.doc(&rejected(&["lamp-a-pic"]));
        assert_eq!(titled(&case), "Answer the owner on lamp-a-pic");
        let case = Case::new();
        case.doc(&rejected(&["lamp-a-pic", "ui-list-v1"]));
        assert_eq!(
            titled(&case),
            "Answer the owner on lamp-a-pic and ui-list-v1"
        );
        let case = Case::new();
        case.doc(&rejected(&[
            "card-a", "card-b", "card-c", "card-d", "card-e",
        ]));
        assert_eq!(
            titled(&case),
            "Answer the owner on card-a, card-b and 3 more"
        );
    }

    #[test]
    fn a_card_slug_two_documents_share_is_named_once_in_the_title() {
        let case = Case::new();
        case.doc(&rejected(&["lamp-a-pic"]));
        case.tree.write("docs/other.md", &rejected(&["lamp-a-pic"]));
        assert_eq!(
            verdicts::untasked(&case.tree.root, &case.board)
                .unwrap()
                .len(),
            2,
            "both documents' cards wait"
        );
        assert_eq!(titled(&case), "Answer the owner on lamp-a-pic");
    }

    #[test]
    fn filing_again_with_nothing_new_files_nothing() {
        let case = Case::new();
        case.file("w");
        *case.board.now.borrow_mut() += 60.0;
        assert_eq!(case.file("w"), None);
        assert_eq!(case.board.rolled().len(), 1);
    }

    #[test]
    fn a_new_verdict_joins_the_unclaimed_roll_up_on_its_thread() {
        let case = Case::new();
        let first = case.file("w").unwrap();
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:15")));
        assert_eq!(case.file("w").as_deref(), Some(first.as_str()));
        let said = case.board.said.borrow().last().unwrap().2.clone();
        assert!(said.starts_with("More verdicts for this task:\n"), "{said}");
        assert!(said.contains("ui-list-v10"));
        assert!(said.contains("too much text"));
        drop(said);
        assert!(case.untasked().is_empty());
    }

    /// One card on the roll-up, then the owner answers a second on another page: the title names
    /// both and the task links both pages, as one amendment by the filer.
    #[test]
    fn a_card_joining_the_roll_up_is_named_in_its_title_and_links_its_page() {
        let case = Case::new();
        case.doc("<!-- register:start -->\n- [-] **lamp-a-pic** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — no.\n<!-- register:end -->\n");
        let slug = case.file("w").unwrap();
        assert_eq!(
            case.board.get(&slug).title,
            "Answer the owner on lamp-a-pic"
        );
        case.board.set("2026-10-06 11:20");
        case.tree.write(
            "docs/menus/menus.md",
            "<!-- register:start -->\n- [?] **menu-v2** — A menu.\n  [2026-10-06 11:15] owner · question — which font?\n<!-- register:end -->\n",
        );
        assert_eq!(case.file("app-1").as_deref(), Some(slug.as_str()));
        let task = case.board.get(&slug);
        assert_eq!(task.title, "Answer the owner on lamp-a-pic and menu-v2");
        assert_eq!(task.links, ["docs/lamps/lamps.md", "docs/menus/menus.md"]);
        assert_eq!(
            *case.board.amended.borrow(),
            [(
                slug.clone(),
                "app-1".to_string(),
                vec!["docs/menus/menus.md".to_string()]
            )]
        );
        assert!(case.untasked().is_empty());
    }

    const MENUS: &str = "<!-- register:start -->\n- [?] **menu-v2** — A menu.\n  [2026-10-06 11:15] owner · question — which font?\n<!-- register:end -->\n";

    /// The amendment goes before the post: one the board refuses posts nothing, so the card is
    /// still waiting, and the next filing amends and posts it.
    #[test]
    fn a_refused_amendment_posts_nothing_and_the_next_filing_does_both() {
        let case = Case::new();
        case.doc("<!-- register:start -->\n- [-] **lamp-a-pic** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — no.\n<!-- register:end -->\n");
        let slug = case.file("w").unwrap();
        case.board.set("2026-10-06 11:20");
        case.tree.write("docs/menus/menus.md", MENUS);
        *case.board.refuse_amends.borrow_mut() = 1;
        let refused = verdicts::file(&case.tree.root, &case.board, "w").unwrap_err();
        assert!(refused.is_unreachable(), "{refused}");
        assert!(case.board.said.borrow().is_empty());
        assert_eq!(
            case.board.get(&slug).title,
            "Answer the owner on lamp-a-pic"
        );
        assert_eq!(case.untasked(), ["menu-v2"]);
        assert_eq!(case.file("w").as_deref(), Some(slug.as_str()));
        let task = case.board.get(&slug);
        assert_eq!(task.title, "Answer the owner on lamp-a-pic and menu-v2");
        assert_eq!(task.links, ["docs/lamps/lamps.md", "docs/menus/menus.md"]);
        assert!(
            task.thread[0].1.contains("**menu-v2**"),
            "{:?}",
            task.thread
        );
        assert!(case.untasked().is_empty());
    }

    /// A roll-up an earlier filing posted to but never amended (a build that posted first, and
    /// whose amendment failed) is mended by the next card's filing: the title names every card it
    /// lists, in the order it lists them, and the pages of the cards posted before are linked with
    /// the new one's.
    #[test]
    fn a_later_filing_mends_the_title_and_links_an_earlier_one_left() {
        let case = Case::new();
        case.doc("<!-- register:start -->\n- [-] **lamp-a-pic** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — no.\n<!-- register:end -->\n");
        let slug = case.file("w").unwrap();
        case.board.set("2026-10-06 11:20");
        case.tree.write("docs/menus/menus.md", MENUS);
        let menus = verdicts::untasked(&case.tree.root, &case.board).unwrap();
        let listing = menus[0].entry();
        case.board
            .say(
                &slug,
                "w",
                &format!("More verdicts for this task:\n{listing}"),
            )
            .unwrap();
        assert!(case.untasked().is_empty());
        case.board.set("2026-10-06 11:40");
        case.tree.write(
            "docs/chairs/chairs.md",
            "<!-- register:start -->\n- [?] **chair-v3** — A chair.\n  [2026-10-06 11:35] owner · question — oak?\n<!-- register:end -->\n",
        );
        assert_eq!(case.file("w").as_deref(), Some(slug.as_str()));
        let task = case.board.get(&slug);
        assert_eq!(
            task.title,
            "Answer the owner on lamp-a-pic, menu-v2 and 1 more"
        );
        assert_eq!(
            task.links,
            [
                "docs/lamps/lamps.md",
                "docs/chairs/chairs.md",
                "docs/menus/menus.md"
            ]
        );
        assert_eq!(
            case.board.amended.borrow().last().unwrap().2,
            ["docs/chairs/chairs.md", "docs/menus/menus.md"]
        );
    }

    /// The owner answers a card the roll-up already lists again: it is posted, and still one card.
    #[test]
    fn a_card_answered_again_is_named_once_and_amends_nothing() {
        let case = Case::new();
        let slug = case.file("w").unwrap();
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(
            "  [2026-10-06 10:10] owner · rejected — needs image explanation\n",
            "  [2026-10-06 10:10] owner · rejected — needs image explanation\n  [2026-10-06 11:15] owner · question — and the colours?\n",
        ));
        assert_eq!(case.file("w").as_deref(), Some(slug.as_str()));
        assert!(
            case.board
                .said
                .borrow()
                .last()
                .unwrap()
                .2
                .contains("and the colours?")
        );
        assert_eq!(
            case.board.get(&slug).title,
            "Answer the owner on lamp-a-pic, lamp-b-pic and 1 more"
        );
        assert!(case.board.amended.borrow().is_empty());
    }

    /// The title names what the roll-up lists, whoever wrote it: a card named in a message that is
    /// not a post of more verdicts is not one of its cards.
    #[test]
    fn only_the_brief_and_posts_of_more_verdicts_are_named() {
        let case = Case::new();
        let slug = case.file("w").unwrap();
        case.board.change(&slug, |t| {
            t.thread.push((
                0.0,
                "Note:\n- docs/other.md:3 **not-a-card** quoted from elsewhere".into(),
            ))
        });
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:15")));
        case.file("w");
        assert_eq!(
            case.board.get(&slug).title,
            "Answer the owner on lamp-a-pic, lamp-b-pic and 2 more"
        );
        assert_eq!(case.board.get(&slug).links, ["docs/lamps/lamps.md"]);
    }

    #[test]
    fn a_claimed_roll_up_gets_no_more_cards_a_new_one_is_filed() {
        let case = Case::new();
        let first = case.file("w").unwrap();
        case.board.change(&first, |t| t.executors = 1);
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:15")));
        let second = case.file("w").unwrap();
        assert_eq!(second, "verdicts/2026-10-06-1120");
        let body = case.board.body(&second);
        assert!(body.contains("ui-list-v10"));
        assert!(!body.contains("lamp-a-pic"));
    }

    #[test]
    fn a_roll_up_reserved_for_someone_gets_no_more_cards() {
        let case = Case::new();
        let first = case.file("w").unwrap();
        case.board.change(&first, |t| t.reserved = 1);
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:15")));
        assert_eq!(case.file("w").as_deref(), Some("verdicts/2026-10-06-1120"));
    }

    #[test]
    fn only_a_ready_roll_up_takes_more_cards() {
        let case = Case::new();
        let first = case.file("w").unwrap();
        case.board.change(&first, |t| t.status = "review".into());
        case.board.set("2026-10-06 11:20");
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:15")));
        assert_eq!(case.file("w").as_deref(), Some("verdicts/2026-10-06-1120"));
    }

    #[test]
    fn the_roll_up_is_not_a_review_task_whose_branch_land_request_would_prefix_again() {
        let case = Case::new();
        assert!(!case.file("w").unwrap().starts_with("review/"));
    }

    #[test]
    fn two_roll_ups_in_one_minute_get_distinct_slugs() {
        let case = Case::new();
        let first = case.file("w").unwrap();
        case.board.change(&first, |t| t.executors = 1);
        case.doc(&REGISTER.replace(PENDING_V10, &rejected_v10("2026-10-06 11:00")));
        assert_eq!(
            case.file("w").as_deref(),
            Some("verdicts/2026-10-06-1100-2")
        );
        let third_case = case
            .board
            .tasks
            .borrow()
            .iter()
            .map(|t| t.slug.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            third_case,
            ["verdicts/2026-10-06-1100", "verdicts/2026-10-06-1100-2"]
        );
    }

    #[test]
    fn the_owner_is_never_the_one_filing() {
        let case = Case::new();
        let VerdictError(message) =
            verdicts::file(&case.tree.root, &case.board, "owner").unwrap_err();
        assert_eq!(
            message,
            "the owner's words are quoted, never posted as the owner: file as a session"
        );
        assert!(case.board.rolled().is_empty());
    }

    #[test]
    fn a_card_with_no_note_is_listed_as_such_and_words_are_one_line() {
        let case = Case::new();
        case.doc(
            "<!-- register:start -->\n- [-] **a-card** — x\n  [2026-10-06 10:00] owner · rejected\n\
             - [-] **b-card** — x\n  [2026-10-06 10:00] owner · rejected — two\n    lines   here\n<!-- register:end -->\n",
        );
        let cards = verdicts::untasked(&case.tree.root, &case.board).unwrap();
        assert_eq!(
            cards[0].entry(),
            "- docs/lamps/lamps.md:2 **a-card** rejected (2026-10-06 10:00): (no note)"
        );
        assert_eq!(
            cards[1].entry(),
            "- docs/lamps/lamps.md:4 **b-card** rejected (2026-10-06 10:00): \"two lines here\""
        );
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
    case: Case,
    trees: Tree,
}

impl Branched {
    fn new() -> Branched {
        let case = Case::new();
        let root = &case.tree.root;
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
            case,
            trees: Tree::new(),
        }
    }

    /// A worktree on a new branch off main, its register replaced by `text`, uncommitted.
    fn branch(&self, name: &str, text: &str) -> PathBuf {
        let path = self.trees.root.join(name.replace('/', "-"));
        git(
            &self.case.tree.root,
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
        std::fs::write(path.join("docs/lamps/lamps.md"), text).unwrap();
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
    fn a_card_open_only_on_a_branch_is_listed_under_its_branch() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::untasked(&b.case.tree.root, &b.case.board).unwrap();
        let card = cards.iter().find(|c| c.slug == "lamp-d-pic").unwrap();
        assert_eq!(card.path, "@pic~x/docs/lamps/lamps.md");
        assert_eq!(card.quotes, ["the fins are too long."]);
        assert!(card.entry().contains("pic/x"), "{}", card.entry());
        assert!(
            card.entry()
                .ends_with("on branch pic/x: \"the fins are too long.\""),
            "{}",
            card.entry()
        );
    }

    #[test]
    fn the_branch_card_is_filed_linking_its_branch_page() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        let slug = b.case.file("w").unwrap();
        assert!(b.case.board.body(&slug).contains("lamp-d-pic"));
        assert!(
            b.case.board.created.borrow()[0]
                .2
                .contains(&"@pic~x/docs/lamps/lamps.md".to_string())
        );
    }

    /// A verdict on a branch page joins a roll-up of main's cards: named, and its branch page linked.
    #[test]
    fn a_branch_card_joining_mains_roll_up_is_named_and_its_page_linked() {
        let b = Branched::new();
        let slug = b.case.file("w").unwrap();
        b.case.board.set("2026-10-06 11:20");
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        assert_eq!(b.case.file("w").as_deref(), Some(slug.as_str()));
        let task = b.case.board.get(&slug);
        assert_eq!(
            task.title,
            "Answer the owner on lamp-a-pic, lamp-b-pic and 2 more"
        );
        assert_eq!(
            task.links,
            ["docs/lamps/lamps.md", "@pic~x/docs/lamps/lamps.md"]
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
        b.case.doc(&answered);
        git(
            &b.case.tree.root,
            &["commit", "-q", "-am", "answer lamp-a-pic"],
        );
        let waiting = b.case.untasked();
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
        assert!(!b.case.untasked().contains(&"lamp-e-pic".to_string()));
    }

    #[test]
    fn a_branch_card_identical_to_mains_is_listed_once_from_main() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::untasked(&b.case.tree.root, &b.case.board).unwrap();
        let paths: Vec<&str> = cards
            .iter()
            .filter(|c| c.slug == "lamp-a-pic")
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(paths, ["docs/lamps/lamps.md"]);
    }

    #[test]
    fn a_card_stacked_branches_carry_is_listed_once() {
        let b = Branched::new();
        b.branch("pic/x", &Branched::with_lamp_d(REGISTER));
        b.branch("pic/y", &Branched::with_lamp_d(REGISTER));
        let cards = verdicts::untasked(&b.case.tree.root, &b.case.board).unwrap();
        assert_eq!(cards.iter().filter(|c| c.slug == "lamp-d-pic").count(), 1);
    }

    #[test]
    fn the_roots_own_checkout_and_a_root_with_no_git_list_only_main() {
        let case = Case::new(); // no repository at all
        assert_eq!(case.untasked(), ["lamp-a-pic", "lamp-b-pic", "ui-list-v1"]);
    }
}

mod actor {
    use super::*;

    #[test]
    fn the_app_acts_as_one_app_session_never_as_the_owner() {
        let case = Case::new();
        let first = verdicts::app_actor(&case.board).unwrap();
        assert_eq!(verdicts::app_actor(&case.board).unwrap(), first);
        assert_eq!(case.board.sessions.borrow().len(), 1);
        let slug = case.file(&first).unwrap();
        assert!(!slug.is_empty());
        assert_eq!(case.board.created.borrow()[0].1, first);
    }

    #[test]
    fn an_app_session_that_left_is_not_reused() {
        let case = Case::new();
        let gone = verdicts::app_actor(&case.board).unwrap();
        case.board.sessions.borrow_mut()[0].2 = true;
        let fresh = verdicts::app_actor(&case.board).unwrap();
        assert_ne!(fresh, gone);
        assert!(case.file(&fresh).is_some());
    }
}

/// CPU the calling thread has spent: the budget below is the CPU the call costs, not wall-clock (which a
/// busy machine stretches past any limit) and not the process's (which would charge it the other tests'
/// threads and the children they reap). `untasked` runs on this thread and starts no child.
fn cpu_seconds() -> f64 {
    // SAFETY: an all-zero `timespec` is a valid plain-integer C struct which clock_gettime overwrites.
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: clock_gettime writes only into the struct given, which outlives the call.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    now.tv_sec as f64 + now.tv_nsec as f64 / 1e9
}

#[test]
fn a_large_backlog_against_a_busy_board_is_judged_quickly() {
    // Exercise many cards against many task texts to catch repeated per-card matching work.
    let case = Case::new();
    let cards: String = (0..300)
        .map(|i| {
            format!("- [-] **card-{i}-pic** — C.\n  [2026-10-06 10:00] owner · rejected — no.\n")
        })
        .collect();
    case.doc(&format!(
        "<!-- register:start -->\n{cards}<!-- register:end -->\n"
    ));
    for i in 0..300 {
        case.board.task(
            &format!("pic/t-{i}"),
            &format!("Brief naming card-{}-pic and others. ", i + 1000).repeat(20),
            "2026-10-06 10:30",
            "ready",
        );
        for j in 0..10 {
            case.board.change(&format!("pic/t-{i}"), |t| {
                t.thread.push((
                    epoch("2026-10-06 10:31").unwrap(),
                    format!("progress on card-{}-pic, step {j}", i + 2000),
                ));
            });
        }
    }
    let started = cpu_seconds();
    assert_eq!(case.untasked().len(), 300);
    let spent = cpu_seconds() - started;
    assert!(spent < 0.5, "{spent} CPU-s");
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
        assert!(epoch("soon").is_none());
    }
}

mod command {
    //! `MainTest`: the command line, on the board in memory.

    use std::rc::Rc;

    use symphony::cli::verdict_tasks_main_with;

    use super::*;

    struct Shared(Rc<Fake>);

    impl Board for Shared {
        fn now(&self) -> f64 {
            self.0.now()
        }
        fn tasks(&self) -> Result<Vec<BoardTask>, VerdictError> {
            self.0.tasks()
        }
        fn app_session(&self) -> Result<Option<String>, VerdictError> {
            self.0.app_session()
        }
        fn register_app(&self) -> Result<String, VerdictError> {
            self.0.register_app()
        }
        fn say(&self, slug: &str, by: &str, text: &str) -> Result<(), VerdictError> {
            self.0.say(slug, by, text)
        }
        fn create(
            &self,
            slug: &str,
            title: &str,
            by: &str,
            body: &str,
            links: &[String],
        ) -> Result<(), VerdictError> {
            self.0.create(slug, title, by, body, links)
        }
        fn amend(
            &self,
            slug: &str,
            title: &str,
            links: &[String],
            by: &str,
        ) -> Result<(), VerdictError> {
            self.0.amend(slug, title, links, by)
        }
    }

    fn run(
        tree: &Tree,
        board: &Rc<Fake>,
        session: Option<&str>,
        args: &[&str],
    ) -> (i32, String, String) {
        let mut argv = vec![
            "--root".to_string(),
            tree.root.to_string_lossy().to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let shared = Rc::clone(board);
        let code = verdict_tasks_main_with(
            &argv,
            session,
            &move |_| Box::new(Shared(Rc::clone(&shared))),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn setup() -> (Tree, Rc<Fake>) {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        (tree, Rc::new(Fake::at("2026-10-06 11:00")))
    }

    #[test]
    fn lists_the_untasked_cards_and_files_nothing() {
        let (tree, board) = setup();
        let (code, out, _) = run(&tree, &board, None, &[]);
        assert_eq!(code, 0);
        assert!(out.contains("docs/lamps/lamps.md"), "{out}");
        assert!(out.contains("lamp-a-pic"), "{out}");
        assert!(
            out.ends_with(
                "3 open cards with no task: `just symphony-cli verdict-tasks --file` puts them on the board\n"
            ),
            "{out}"
        );
        assert!(board.rolled().is_empty());
    }

    #[test]
    fn file_refuses_without_a_session_and_files_nothing() {
        let (tree, board) = setup();
        let (code, out, _) = run(&tree, &board, None, &["--file"]);
        assert_eq!(code, 2);
        assert_eq!(
            out,
            "--file needs a session to file as: set TASKS_SESSION or pass --as HANDLE\n"
        );
        assert!(board.rolled().is_empty());
    }

    #[test]
    fn file_files_the_roll_up_as_the_session_and_says_its_slug() {
        let (tree, board) = setup();
        let (code, out, _) = run(&tree, &board, Some("w1"), &["--file"]);
        assert_eq!(
            (code, out.as_str()),
            (0, "filed on verdicts/2026-10-06-1100\n")
        );
        assert_eq!(board.created.borrow()[0].1, "w1");
        let (code, out, _) = run(&tree, &board, None, &["--file", "--as", "w2"]);
        assert_eq!((code, out.as_str()), (0, "every open card has a task\n"));
        let (code, out, _) = run(&tree, &board, None, &[]);
        assert_eq!((code, out.as_str()), (0, "every open card has a task\n"));
    }

    #[test]
    fn as_beats_the_environment_and_app_files_as_the_apps_own_session() {
        let (tree, board) = setup();
        run(&tree, &board, Some("env"), &["--file", "--as", "flag"]);
        assert_eq!(board.created.borrow()[0].1, "flag");
        let (tree, board) = setup();
        run(&tree, &board, None, &["--file", "--app"]);
        assert_eq!(board.created.borrow()[0].1, "app-1");
        assert_eq!(board.sessions.borrow().len(), 1);
    }

    #[test]
    fn the_owner_is_refused_with_a_message_and_exit_1() {
        let (tree, board) = setup();
        let (code, _, err) = run(&tree, &board, None, &["--file", "--as", "owner"]);
        assert_eq!(code, 1);
        assert!(err.starts_with("✖ the owner's words are quoted"), "{err}");
    }

    #[test]
    fn an_unknown_argument_is_refused_as_argparse_refuses() {
        let (tree, board) = setup();
        let (code, _, err) = run(&tree, &board, None, &["now"]);
        assert_eq!(code, 2);
        assert!(err.contains("unrecognized arguments: now"), "{err}");
    }

    #[test]
    fn there_is_no_db_flag_the_board_is_the_one_the_environment_reaches() {
        let (tree, board) = setup();
        let (code, out, err) = run(&tree, &board, Some("w1"), &["--file", "--db", "x.db"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("unrecognized arguments: --db"), "{err}");
        assert!(board.rolled().is_empty());
        let (code, out, _) = run(&tree, &board, None, &["--help"]);
        assert_eq!(code, 0);
        assert!(!out.contains("--db"), "{out}");
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
    //! The command against the board the `tasks` binary reaches (a database of its own on the tests'
    //! server), and the binary a recipe runs.

    use super::*;

    fn review(tree: &Tree, env: &[(String, String)], args: &[&str]) -> (i32, String, String) {
        let done = sys::output(
            sys::Command::new(crate::review_binary())
                .args(args)
                .args(["--root", tree.root.to_str().unwrap()])
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

    fn register_worker(tree: &Tree, env: &[(String, String)]) -> String {
        let handle = fixture_worker(&fixture_store(env), &tree.root, "worker");
        assert!(!handle.is_empty());
        handle
    }

    #[test]
    fn three_waiting_cards_are_filed_once_and_a_second_run_files_none() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        let env = board_env();
        let handle = register_worker(&tree, &env);

        let (code, out, err) = review(&tree, &env, &["verdict-tasks"]);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.ends_with(
                "3 open cards with no task: `just symphony-cli verdict-tasks --file` puts them on the board\n"
            ),
            "{out}"
        );

        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--as", &handle]);
        assert_eq!(code, 0, "{err}");
        assert!(out.starts_with("filed on verdicts/"), "{out}");
        let slug = out.trim().strip_prefix("filed on ").unwrap().to_string();

        let shown = tasks(&env, &tree.root, &["show", &slug, "--json"]);
        for slug in ["lamp-a-pic", "lamp-b-pic", "ui-list-v1"] {
            assert!(shown.contains(slug), "{slug}: {shown}");
        }

        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--as", &handle]);
        assert_eq!(
            (code, out.as_str()),
            (0, "every open card has a task\n"),
            "{err}"
        );
        let (_, out, _) = review(&tree, &env, &["verdict-tasks"]);
        assert_eq!(out, "every open card has a task\n");
    }

    #[test]
    fn the_app_files_as_one_app_session_and_a_new_verdict_joins_its_roll_up() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        let env = board_env();
        register_worker(&tree, &env);
        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--app"]);
        assert_eq!(code, 0, "{err}");
        let first = out.trim().strip_prefix("filed on ").unwrap().to_string();
        tree.write(
            "docs/lamps/lamps.md",
            &REGISTER.replace(PENDING_V10, &rejected_v10("2999-01-01 00:00")),
        );
        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--app"]);
        assert_eq!((code, out), (0, format!("filed on {first}\n")), "{err}");
    }

    /// The acceptance of the roll-up's title: one verdict files it naming that card, a second on
    /// another page joins it named beside the first, with that page linked and the change on its
    /// thread.
    #[test]
    fn a_second_verdict_is_named_in_the_roll_ups_title_and_links_its_page() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", "<!-- register:start -->\n- [-] **lamp-a-pic** — Lamp A.\n  [2026-10-06 10:00] owner · rejected — no.\n<!-- register:end -->\n");
        let env = board_env();
        register_worker(&tree, &env);
        let board = symphony::taskcli::CliBoard::with_command(
            vec![tasks_binary().to_string()],
            &env,
            &tree.root,
        );
        let roll_up = |slug: &str| {
            board
                .tasks()
                .unwrap()
                .into_iter()
                .find(|t| t.slug == slug)
                .unwrap()
        };
        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--app"]);
        assert_eq!(code, 0, "{err}");
        let slug = out.trim().strip_prefix("filed on ").unwrap().to_string();
        let task = roll_up(&slug);
        assert_eq!(task.title, "Answer the owner on lamp-a-pic");
        assert_eq!(task.links, ["docs/lamps/lamps.md"]);

        tree.write(
            "docs/menus/menus.md",
            "<!-- register:start -->\n- [?] **menu-v2** — A menu.\n  [2999-01-01 00:00] owner · question — which font?\n<!-- register:end -->\n",
        );
        let (code, out, err) = review(&tree, &env, &["verdict-tasks", "--file", "--app"]);
        assert_eq!((code, out), (0, format!("filed on {slug}\n")), "{err}");
        let task = roll_up(&slug);
        assert_eq!(task.title, "Answer the owner on lamp-a-pic and menu-v2");
        assert_eq!(task.links, ["docs/lamps/lamps.md", "docs/menus/menus.md"]);
        let said: Vec<&str> = task.thread.iter().map(|(_, text)| text.as_str()).collect();
        assert!(
            said.iter().any(|text| text.starts_with(
                "More verdicts for this task:\n- docs/menus/menus.md:2 **menu-v2** question"
            )),
            "{said:?}"
        );
        assert!(
            said.iter().any(|text| text.ends_with(
                "retitled it: Answer the owner on lamp-a-pic → Answer the owner on lamp-a-pic and menu-v2"
            )),
            "{said:?}"
        );
    }

    #[test]
    fn a_board_it_cannot_reach_is_said_so_in_one_line_naming_it_and_exit_two() {
        // Exit 2, as a refusal's 1 is not, and the line names the address it tried.
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        let gone = [("TASKS_SERVER".to_string(), "127.0.0.1:1".to_string())];
        for args in [
            &["verdict-tasks"][..],
            &["verdict-tasks", "--file", "--app"],
            &["verdict-tasks", "--file", "--as", "w1"],
        ] {
            let (code, out, err) = review(&tree, &gone, args);
            assert_eq!((code, out.as_str()), (2, ""), "{args:?}: {err}");
            assert!(
                err.starts_with("✖ cannot reach the task board: ")
                    && err.contains("127.0.0.1:1")
                    && err.lines().count() == 1,
                "{args:?}: {err}"
            );
        }
    }

    #[test]
    fn triage_prints_the_ranking_of_the_docs() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
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
}

mod contract {
    //! `CliBoard`, the board production reads, on the real store through the `tasks` binary: the cases
    //! the in-memory board cannot check for it.

    use symphony::taskcli::CliBoard;
    use symphony_board::health::RESERVE_SECONDS;

    use super::*;

    struct Real {
        tree: Tree,
        env: Vec<(String, String)>,
        setup: std::sync::OnceLock<Store>,
    }

    impl Real {
        fn new() -> Real {
            Real {
                tree: Tree::new(),
                env: board_env(),
                setup: std::sync::OnceLock::new(),
            }
        }

        /// `tasks ARGS` on the board; its stdout.
        fn tasks(&self, args: &[&str]) -> String {
            tasks(&self.env, &self.tree.root, args)
        }

        fn worker(&self, kind: &str) -> String {
            fixture_worker(self.setup(), &self.tree.root, kind)
        }

        fn setup(&self) -> &Store {
            self.setup.get_or_init(|| fixture_store(&self.env))
        }

        fn create(&self, by: &str, slug: &str, body: &str) {
            self.create_with(by, slug, body, 2, None);
        }

        fn create_with(
            &self,
            by: &str,
            slug: &str,
            body: &str,
            priority: i64,
            assignee: Option<&str>,
        ) {
            self.setup()
                .create(&symphony_board::NewTask {
                    body: body.into(),
                    priority,
                    assignee: assignee.map(str::to_owned),
                    ..symphony_board::NewTask::new(slug, "a task", by)
                })
                .unwrap();
        }

        fn board(&self) -> CliBoard {
            CliBoard::with_command(vec![tasks_binary().to_string()], &self.env, &self.tree.root)
        }

        /// The board read with its clock at `now`.
        fn board_at(&self, now: f64) -> CliBoard {
            let mut env = self.env.clone();
            env.retain(|(key, _)| key != "TASKS_NOW");
            env.push(("TASKS_NOW".into(), now.to_string()));
            CliBoard::with_command(vec![tasks_binary().to_string()], &env, &self.tree.root)
        }

        fn task(&self, slug: &str) -> BoardTask {
            find(&self.board(), slug)
        }
    }

    fn find(board: &CliBoard, slug: &str) -> BoardTask {
        board
            .tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.slug == slug)
            .unwrap()
    }

    #[test]
    fn a_task_is_read_with_its_words_status_and_creation_time() {
        let real = Real::new();
        let w = real.worker("worker");
        real.create(&w, "pic/one", "Redraw lamp-a-pic.");
        let task = real.task("pic/one");
        assert_eq!(
            (
                task.title.as_str(),
                task.body.as_str(),
                task.status.as_str()
            ),
            ("a task", "Redraw lamp-a-pic.", "ready")
        );
        assert_eq!((task.created, real.board().now()), (board::NOW, board::NOW));
        assert_eq!((task.executors, task.reserved), (0, 0));
    }

    #[test]
    fn the_clock_is_the_one_the_board_writes_by() {
        let real = Real::new();
        assert_eq!(real.board().now(), board::NOW);
        assert_eq!(real.board_at(board::NOW + 60.0).now(), board::NOW + 60.0);
    }

    #[test]
    fn tasks_come_in_the_boards_order_priority_then_age() {
        let real = Real::new();
        let w = real.worker("worker");
        real.create_with(&w, "pic/late", "x", 3, None);
        real.create_with(&w, "pic/first", "x", 1, None);
        real.create_with(&w, "pic/second", "x", 1, None);
        let slugs: Vec<String> = real
            .board()
            .tasks()
            .unwrap()
            .into_iter()
            .map(|t| t.slug)
            .collect();
        assert_eq!(slugs, ["pic/first", "pic/second", "pic/late"]);
    }

    #[test]
    fn the_thread_holds_every_message_of_its_own_task() {
        let real = Real::new();
        let w = real.worker("worker");
        real.create(&w, "pic/one", "x");
        real.create(&w, "pic/two", "x");
        real.tasks(&["--as", &w, "say", "pic/one", "Redrawing lamp-a-pic."]);
        let one = real.task("pic/one");
        assert!(
            one.thread
                .iter()
                .any(|(at, text)| text == "Redrawing lamp-a-pic." && *at == board::NOW),
            "{:?}",
            one.thread
        );
        assert!(
            !real
                .task("pic/two")
                .thread
                .iter()
                .any(|(_, text)| text.contains("lamp-a-pic"))
        );
    }

    #[test]
    fn a_claimed_task_has_an_executor_and_only_the_claimed_one() {
        let real = Real::new();
        let w = real.worker("worker");
        real.create(&w, "pic/one", "x");
        real.create(&w, "pic/two", "x");
        real.tasks(&["--as", &w, "claim", "pic/one"]);
        assert_eq!(real.task("pic/one").executors, 1);
        assert_eq!(real.task("pic/two").executors, 0);
    }

    #[test]
    fn a_reservation_counts_until_it_lapses() {
        let real = Real::new();
        let w = real.worker("worker");
        real.create_with(&w, "pic/for", "x", 2, Some(&w)); // no lapse
        real.create(&w, "pic/lapsing", "x");
        real.tasks(&["--as", &w, "reserve", "pic/lapsing"]);
        assert_eq!(real.task("pic/for").reserved, 1);
        assert_eq!(real.task("pic/lapsing").reserved, 1);
        let later = real.board_at(board::NOW + RESERVE_SECONDS + 1.0);
        assert_eq!(find(&later, "pic/lapsing").reserved, 0);
        assert_eq!(find(&later, "pic/for").reserved, 1);
    }

    #[test]
    fn the_app_session_is_the_live_app_one_and_a_left_one_is_not() {
        let real = Real::new();
        real.worker("worker");
        let board = real.board();
        assert_eq!(board.app_session().unwrap(), None);
        let app = board.register_app().unwrap();
        assert_eq!(board.app_session().unwrap().as_deref(), Some(app.as_str()));
        real.tasks(&["--as", &app, "leave"]);
        assert_eq!(board.app_session().unwrap(), None);
        assert!(!verdicts::app_actor(&board).unwrap().is_empty());
        assert_ne!(verdicts::app_actor(&board).unwrap(), app);
    }

    #[test]
    fn saying_and_creating_go_through_as_the_session_with_the_body_and_links() {
        let real = Real::new();
        let w = real.worker("worker");
        let board = real.board();
        board
            .create(
                "verdicts/x",
                "Answer 1 owner verdict",
                &w,
                "line one\nline \"two\"\n",
                &["docs/a.md".into(), "@b~c/docs/a.md".into()],
            )
            .unwrap();
        let task = real.task("verdicts/x");
        assert_eq!(
            (task.title.as_str(), task.body.as_str()),
            ("Answer 1 owner verdict", "line one\nline \"two\"\n")
        );
        board.say("verdicts/x", &w, "More verdicts").unwrap();
        assert!(
            real.task("verdicts/x")
                .thread
                .iter()
                .any(|(_, t)| t == "More verdicts")
        );
        let shown = real.tasks(&["show", "verdicts/x", "--json"]);
        assert!(
            shown.contains("docs/a.md") && shown.contains("@b~c/docs/a.md"),
            "{shown}"
        );
        assert!(board.say("verdicts/nope", &w, "x").is_err());
    }

    #[test]
    fn amending_retitles_and_adds_links_as_the_session_and_a_refusal_is_the_boards_words() {
        let real = Real::new();
        let w = real.worker("worker");
        let board = real.board();
        board
            .create(
                "verdicts/x",
                "Answer 1 owner verdict",
                &w,
                "x",
                &["docs/a.md".into()],
            )
            .unwrap();
        board
            .amend(
                "verdicts/x",
                "Answer 2 owner verdicts",
                &["@b~c/docs/a.md".into()],
                &w,
            )
            .unwrap();
        let task = real.task("verdicts/x");
        assert_eq!(task.title, "Answer 2 owner verdicts");
        assert_eq!(task.links, ["@b~c/docs/a.md", "docs/a.md"]);
        assert!(
            task.thread.iter().any(|(_, t)| t
                == &format!("{w} retitled it: Answer 1 owner verdict → Answer 2 owner verdicts")),
            "{:?}",
            task.thread
        );
        let VerdictError(why) = board.amend("verdicts/nope", "T", &[], &w).unwrap_err();
        assert_eq!(why, "no task 'verdicts/nope'");
    }

    #[test]
    fn a_board_it_cannot_reach_or_a_binary_that_cannot_run_is_a_refusal_naming_it() {
        let tree = Tree::new();
        let gone = [("TASKS_SERVER".to_string(), "127.0.0.1:1".to_string())];
        let board = CliBoard::with_command(vec![tasks_binary().to_string()], &gone, &tree.root);
        for problem in [board.tasks().map(|_| ()), board.app_session().map(|_| ())] {
            let why = problem.unwrap_err().0;
            assert!(why.starts_with("cannot reach the task board: "), "{why}");
        }
        let missing = tree.path("no-such-binary");
        let board = CliBoard::with_command(
            vec![missing.to_string_lossy().to_string()],
            &gone,
            &tree.root,
        );
        let why = board.tasks().unwrap_err().0;
        assert!(why.starts_with("cannot run "), "{why}");
    }
}

mod solar {
    //! What the command does at the edges of its input.

    use super::*;

    /// The binary run in `cwd`, on the board `env` reaches through the `tasks` binary.
    fn run(cwd: &Path, env: &[(String, String)], args: &[&str]) -> (i32, String, String) {
        let done = sys::output(
            sys::Command::new(crate::review_binary())
                .args(args)
                .current_dir(cwd)
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

    const IMPOSSIBLE: &str = "<!-- register:start -->\n- [-] **odd-card** — x\n  [2026-02-30 10:00] owner · rejected — no\n<!-- register:end -->\n";

    #[test]
    fn an_owner_line_with_an_impossible_date_refuses_naming_the_card_and_its_date() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", IMPOSSIBLE);
        let env = board_env();
        let handle = fixture_worker(&fixture_store(&env), &tree.root, "worker");
        for args in [
            vec!["verdict-tasks"],
            vec!["verdict-tasks", "--file", "--as", &handle],
        ] {
            let (code, out, err) = run(&tree.root, &env, &[&args[..], &["--root", "."]].concat());
            assert_eq!(code, 1, "{args:?}: {out}{err}");
            assert!(
                err.contains("odd-card") && err.contains("2026-02-30 10:00"),
                "{err}"
            );
            assert!(!out.contains("every open card has a task"), "{out}");
        }
        let listed = tasks(&env, &tree.root, &["list", "--json"]);
        assert!(!listed.contains("verdicts/"), "{listed}");
    }

    #[test]
    fn a_board_nothing_has_written_to_yet_lists_every_waiting_card() {
        let tree = Tree::new();
        tree.write("docs/lamps/lamps.md", REGISTER);
        let (code, out, err) = run(&tree.root, &board_env(), &["verdict-tasks", "--root", "."]);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.ends_with(
                "3 open cards with no task: `just symphony-cli verdict-tasks --file` puts them on the board\n"
            ),
            "{out}"
        );
    }
}
