//! The one server's notices for a project, on a scratch board and a temporary machine folder whose
//! `notices/programs.toml` names a fake program: a card that starts waiting, an agent's question to
//! the person at the machine and a reviewer's verdict reach each member of the thing's group once,
//! by their way, and nobody else; a board that names nobody tells no one; and the worker hands what
//! a wake finds and ends at the stop.

use super::super::Hub;
use super::{Read, Said};
use crate::notices::hand::Running;
use crate::notices::runner::Runner;
use crate::testing::board::{Fixture, Seed};
use crate::testing::{Tree, repository_holding, store_on};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use symphony_board::people::ownership::Place;
use symphony_board::people::{Power, Profile, Profiles};

const DOC: &str = "docs/design/palette.md";
const ORIGIN: &str = "http://studio.tailnet.ts.net:8765";
const ANA_SLACK: &str = "U024BE7LH";
const BO_EMAIL: &str = "bo@example.com";

fn page(waiting: bool) -> String {
    let card = if waiting {
        "- [ ] **new-palette** — Which palette?\n"
    } else {
        ""
    };
    page_holding(card)
}

/// The palette page, `card` waiting after the card decided.
fn page_holding(card: &str) -> String {
    format!(
        "# Palette\n\n<!-- register:start -->\n\
         - [x] **old-grid** — The grid.\n  [2026-01-02 10:00] owner · approved — A\n\
         {card}<!-- register:end -->\n"
    )
}

/// A project, its board, its machine folder and its hub, the hub's main page and board read.
struct Telling {
    tree: Tree,
    root: PathBuf,
    f: Fixture,
    hub: Arc<Hub>,
}

impl Telling {
    /// The board names ana and bo (design) and cy (engineering) unless `nobody`; design holds the
    /// folder docs/design and the area app. Ana goes by Slack, bo by email, cy by WhatsApp, for
    /// which the machine names no program.
    fn new(nobody: bool) -> Telling {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", DOC, &page(false));
        let f = Fixture::new(
            Seed::default()
                .session("worker")
                .session("rev")
                .task("app/x", "review")
                .role("app/x", "worker", "executor")
                .role("app/x", "rev", "reviewer"),
        );
        if !nobody {
            let store = &f.store;
            for group in ["design", "engineering"] {
                store.add_group(group, &[], "owner").unwrap();
            }
            for (name, group) in [("ana", "design"), ("bo", "design"), ("cy", "engineering")] {
                (store.add_person(name, &[group.into()], &[Power::Speak], "owner")).unwrap();
            }
            for place in [
                Place::Folder("docs/design".into()),
                Place::Area("app".into()),
            ] {
                store.map_place(&place, "design", "owner").unwrap();
            }
        }
        let machine = tree.path("machine");
        let sender = machine.join("notices/sender");
        std::fs::create_dir_all(sender.parent().unwrap()).unwrap();
        crate::fixture_script::write(
            &sender,
            "#!/bin/sh\nhere=\"${0%/*}\"\ncat >> \"$here/told\"\nexit 0\n",
            false,
        );
        tree.write(
            "machine/notices/programs.toml",
            &format!("slack = \"{0}\"\nemail = \"{0}\"\n", sender.display()),
        );
        profiles().write(&machine).unwrap();
        let mut hub = Hub::new(&root, &root.join("build/client"));
        hub.machine = Some(machine);
        let board = f.other_database();
        hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
        hub.refresh_page("", &root).unwrap();
        hub.observe(&f.store);
        Telling {
            tree,
            root,
            f,
            hub: Arc::new(hub),
        }
    }

    /// The notices the fake program was handed, each its kind, way, address and link.
    fn told(&self) -> Vec<(String, String, String, String)> {
        let told =
            std::fs::read_to_string(self.tree.path("machine/notices/told")).unwrap_or_default();
        (told.lines())
            .map(|line| {
                let notice = symphony_notice::Notice::read(line).unwrap();
                (
                    notice.kind().word().into(),
                    notice.to().way().into(),
                    notice.to().address().into(),
                    notice.link().into(),
                )
            })
            .collect()
    }
}

fn profiles() -> Profiles {
    let every = |way: &str| {
        let mut preferences = serde_json::Map::new();
        for kind in ["card", "question", "verdict"] {
            preferences.insert(kind.into(), json!(way));
        }
        preferences
    };
    let chat = |way: &str, address: &str| {
        let mut chat = serde_json::Map::new();
        chat.insert(way.into(), json!(address));
        chat
    };
    Profiles {
        people: vec![
            Profile {
                name: "ana".into(),
                chat: chat("slack", ANA_SLACK),
                preferences: every("slack"),
                ..Profile::default()
            },
            Profile {
                name: "bo".into(),
                emails: vec![BO_EMAIL.into()],
                preferences: every("email"),
                ..Profile::default()
            },
            Profile {
                name: "cy".into(),
                chat: chat("whatsapp", "+15551234567"),
                preferences: every("whatsapp"),
                ..Profile::default()
            },
        ],
    }
}

/// One pass at a time, as the worker makes them.
struct Passes {
    runner: Runner,
    last: Option<Read>,
    said: Said,
    running: Running,
}

impl Passes {
    fn of(telling: &Telling) -> Passes {
        let mut said = Said::default();
        let runner = telling.hub.open_notices("symphony", &mut said).unwrap();
        Passes {
            runner,
            last: None,
            said,
            running: Running::default(),
        }
    }

    fn pass(&mut self, hub: &Hub) {
        let mut origin = || ORIGIN.to_string();
        hub.notice_pass(
            &mut self.runner,
            &mut self.last,
            &self.running,
            &mut origin,
            &mut self.said,
            "symphony",
        );
    }
}

/// The card waiting on design, the question asked the person at the machine and the reviewer's
/// verdict on app/x, then the hub's page and board read again.
fn ask_everything(telling: &Telling) {
    std::fs::write(telling.root.join(DOC), page(true)).unwrap();
    let store = &telling.f.store;
    (store.say("app/x", "worker", "Which way?", Some("owner"))).unwrap();
    (store.say("app/x", "worker", "Working on it.", Some("@orchestrator"))).unwrap();
    store.say("app/x", "owner", "Carry on.", None).unwrap();
    store.verdict("app/x", "rev", "approve", "").unwrap();
    telling.hub.refresh_page("", &telling.root).unwrap();
    telling.hub.observe(store);
}

#[test]
fn a_waiting_card_a_question_and_a_verdict_reach_each_member_of_its_group_once_by_their_way() {
    let telling = Telling::new(false);
    let mut passes = Passes::of(&telling);
    passes.pass(&telling.hub);
    assert!(passes.runner.cursor().is_some(), "the first look");
    assert!(telling.told().is_empty(), "nothing of what stood");
    ask_everything(&telling);
    passes.pass(&telling.hub);
    let task = format!("{ORIGIN}/symphony/task/app/x");
    let card = format!("{ORIGIN}/symphony/{DOC}#new-palette");
    let by = |kind: &str, way: &str, address: &str, link: &str| {
        (
            kind.to_string(),
            way.to_string(),
            address.to_string(),
            link.to_string(),
        )
    };
    assert_eq!(
        telling.told(),
        [
            by("question", "slack", ANA_SLACK, &task),
            by("question", "email", BO_EMAIL, &task),
            by("verdict", "slack", ANA_SLACK, &task),
            by("verdict", "email", BO_EMAIL, &task),
            by("card", "slack", ANA_SLACK, &card),
            by("card", "email", BO_EMAIL, &card),
        ],
        "cy is in engineering; the line to @orchestrator and the owner's own tell no one"
    );
    // Another pass, and another after the board moved for nothing that tells: nobody again.
    passes.pass(&telling.hub);
    (telling.f.store.say("app/x", "worker", "Done.", None)).unwrap();
    telling.hub.observe(&telling.f.store);
    passes.pass(&telling.hub);
    assert_eq!(telling.told().len(), 6);
    // A card that starts waiting by a document's edit alone, no board line with it.
    let both = "- [ ] **new-palette** — Which palette?\n- [ ] **new-grid** — Which grid?\n";
    std::fs::write(telling.root.join(DOC), page_holding(both)).unwrap();
    telling.hub.refresh_page("", &telling.root).unwrap();
    passes.pass(&telling.hub);
    let grid = format!("{ORIGIN}/symphony/{DOC}#new-grid");
    assert_eq!(
        telling.told()[6..],
        [
            by("card", "slack", ANA_SLACK, &grid),
            by("card", "email", BO_EMAIL, &grid),
        ]
    );
    let record = std::fs::read_to_string(telling.root.join("artifacts/notices.json")).unwrap();
    assert!(
        !record.contains(ANA_SLACK) && !record.contains(BO_EMAIL),
        "{record}"
    );
}

#[test]
fn a_board_that_names_nobody_tells_no_one() {
    let telling = Telling::new(true);
    let mut passes = Passes::of(&telling);
    passes.pass(&telling.hub);
    ask_everything(&telling);
    passes.pass(&telling.hub);
    assert!(telling.told().is_empty());
    assert_eq!(passes.runner.next_due(), None);
}

#[test]
fn the_worker_hands_what_a_wake_finds_and_ends_at_the_stop() {
    let telling = Telling::new(false);
    telling
        .hub
        .start_notices_with("symphony", Box::new(|| ORIGIN.to_string()));
    // Its first pass takes what stands; a wake after the asks finds them.
    let record = telling.root.join("artifacts/notices.json");
    test_time::until_true("the worker's first look", hand_bound(), || record.is_file())
        .unwrap_or_else(|stall| panic!("{stall}"));
    ask_everything(&telling);
    telling.hub.wake_notices();
    test_time::until_true("the six notices handed", hand_bound(), || {
        telling.told().len() == 6
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    telling.hub.stop_notices();
    let worker = telling.hub.notice_worker.get().unwrap();
    assert!(worker.thread.lock().unwrap().is_none(), "joined");
    assert!(worker.running.stopping());
    telling.hub.wake_notices();
}

/// A wall-clock backstop a stall alone reaches: six fake programs' runs on a loaded machine.
fn hand_bound() -> std::time::Duration {
    crate::notices::hand::MOST
}
