//! The one server's notices for a project, on a scratch board and a temporary machine folder whose
//! `notices/programs.toml` names a fake program: a card that starts waiting, an agent's question to
//! the person at the machine and a reviewer's verdict reach each member of the thing's group once,
//! by their way, and nobody else; a held message's line tells who may pass it on, of the task's
//! group else of the board, never its author, and its pass, a person's line, tells no one, and a
//! look that cannot read its held row tells it at the next that can; a board that names nobody
//! tells no one; and the worker hands what a wake finds and ends at the stop.

use super::super::Hub;
use super::{Said, Turns};
use crate::notices::hand::Bounds;
use crate::notices::runner::Runner;
use crate::testing::board::{Fixture, Seed};
use crate::testing::{Tree, repository_holding, store_on};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use symphony_board::people::ownership::Place;
use symphony_board::people::{Power, Profile, Profiles};
use symphony_board::recording::Gate;

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

/// A project, its board, its machine folder and its hub, the hub's main page and board read, and
/// its server generation's recording gate, open.
struct Telling {
    tree: Tree,
    root: PathBuf,
    f: Fixture,
    hub: Arc<Hub>,
    gate: Arc<Gate>,
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
        let gate = Arc::new(Gate::default());
        hub.gate_recorders(Arc::clone(&gate));
        Telling {
            tree,
            root,
            f,
            hub: Arc::new(hub),
            gate,
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

/// The worker's turns of the project `symphony`, taken one at a time by the test.
fn turns() -> Turns {
    Turns::new(
        "symphony",
        bounds(),
        Box::new(|| ORIGIN.to_string()),
        Arc::default(),
    )
}

impl Turns {
    fn pass(&mut self, hub: &Hub) {
        hub.notice_turn(self);
    }

    /// The runner's cursor: none before a runner opened its record and looked.
    fn cursor(&self) -> Option<i64> {
        self.runner.as_ref().and_then(Runner::cursor)
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
    let mut passes = turns();
    passes.pass(&telling.hub);
    assert!(passes.cursor().is_some(), "the first look");
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

impl Telling {
    /// Gives each of `names` the direct agents power, as the person at the machine.
    fn direct_agents(&self, names: &[&str]) {
        for name in names {
            let holder = symphony_board::people::Holder::Person(name);
            (self.f.store.grant(holder, &[Power::DirectAgents], "owner")).unwrap();
        }
    }

    /// Names a program for WhatsApp too, cy's way.
    fn whatsapp(&self) {
        let sender = self.tree.path("machine/notices/sender");
        self.tree.write(
            "machine/notices/programs.toml",
            &format!(
                "slack = \"{0}\"\nemail = \"{0}\"\nwhatsapp = \"{0}\"\n",
                sender.display()
            ),
        );
    }

    /// Ana's message on app/x, held, its line read by the hub: its link.
    fn hold(&self, text: &str) -> (i64, String) {
        let ana = self.f.other().for_person("ana");
        let held = (ana.hold_words("app/x", "ana", text, None)).unwrap();
        self.hub.observe(&self.f.store);
        (
            held.id,
            format!("{ORIGIN}/symphony/task/app/x#message-{}", held.id),
        )
    }
}

/// What the fake program was handed after its first `from`, each its kind, way and address.
fn told_after(telling: &Telling, from: usize) -> Vec<(String, String, String)> {
    (telling.told().into_iter().skip(from))
        .map(|(kind, way, address, _)| (kind, way, address))
        .collect()
}

fn notice(kind: &str, way: &str, address: &str) -> (String, String, String) {
    (kind.into(), way.into(), address.into())
}

/// A message held for people (`symphony_board::held`) is told of by the board's line for it: as
/// `held`, a link alone to its place on the task's page, to each of the task's group who may pass
/// it on, never its author, and to no member who may not; once it is passed on, no one, and its
/// pass, a person's line, tells no one either. A line to `owner` that only reads as one is a
/// question to the group.
#[test]
fn a_held_message_tells_who_of_its_tasks_group_may_pass_it_never_its_author() {
    let telling = Telling::new(false);
    telling.whatsapp();
    // cy joins design without direct agents, and bo is given it.
    (telling.f.store.join_group("cy", "design", "owner")).unwrap();
    telling.direct_agents(&["bo"]);
    let mut passes = turns();
    passes.pass(&telling.hub);
    let (held, link) = telling.hold("Cut the header");
    passes.pass(&telling.hub);
    assert_eq!(
        telling.told(),
        [("held".into(), "email".into(), BO_EMAIL.into(), link.clone())],
        "bo alone: ana wrote it and cy may not direct agents"
    );
    // Ana may pass a message now; hers are still not told to her, two in one look each told.
    telling.direct_agents(&["ana"]);
    let (_, second) = telling.hold("And the footer");
    let (_, third) = telling.hold("And the margins");
    passes.pass(&telling.hub);
    let bo = |link: &str| {
        (
            "held".into(),
            "email".into(),
            BO_EMAIL.into(),
            link.to_string(),
        )
    };
    assert_eq!(telling.told()[1..], [bo(&second), bo(&third)]);
    // A pass, a person's line, tells no one.
    (telling.f.store.pass("app/x", held, "owner")).unwrap();
    telling.hub.observe(&telling.f.store);
    passes.pass(&telling.hub);
    assert_eq!(telling.told().len(), 3, "the pass is a person's line");
    // One passed on before the runner looked at its line tells no one.
    let ana = telling.f.other().for_person("ana");
    let early = (ana.hold_words("app/x", "ana", "Never mind", None)).unwrap();
    (telling.f.store.pass("app/x", early.id, "owner")).unwrap();
    telling.hub.observe(&telling.f.store);
    passes.pass(&telling.hub);
    assert_eq!(telling.told().len(), 3, "passed before its notice was made");
    // An agent's line to owner that reads as one is a question to the group, ana, bo and cy.
    let marker = format!("{}#{held}", symphony_board::held::MARKER);
    (telling
        .f
        .store
        .say("app/x", "worker", &marker, Some("owner")))
    .unwrap();
    telling.hub.observe(&telling.f.store);
    passes.pass(&telling.hub);
    assert_eq!(
        told_after(&telling, 3),
        [
            notice("question", "slack", ANA_SLACK),
            notice("question", "email", BO_EMAIL),
            notice("question", "whatsapp", "+15551234567"),
        ]
    );
}

/// With nobody of the task's group able to pass it on but its author, every person of the board
/// who may is told; with nobody but its author on the whole board, no one is, as the person at the
/// machine is told by the page.
#[test]
fn a_held_message_no_one_of_its_group_may_pass_tells_the_board_who_may_else_no_one() {
    let telling = Telling::new(false);
    telling.whatsapp();
    let mut passes = turns();
    passes.pass(&telling.hub);
    // Nobody may pass it but ana, its author: no one is told.
    telling.direct_agents(&["ana"]);
    telling.hold("Cut the header");
    passes.pass(&telling.hub);
    assert!(telling.told().is_empty(), "{:?}", telling.told());
    // cy, of engineering, may: told, and design's bo, who may not, is not.
    telling.direct_agents(&["cy"]);
    telling.hold("And the footer");
    passes.pass(&telling.hub);
    assert_eq!(
        told_after(&telling, 0),
        [notice("held", "whatsapp", "+15551234567")]
    );
    // Once bo of design may, design's alone are told again.
    telling.direct_agents(&["bo"]);
    telling.hold("And the margins");
    passes.pass(&telling.hub);
    assert_eq!(told_after(&telling, 1), [notice("held", "email", BO_EMAIL)]);
}

/// A look whose read of a task's held messages fails takes no line and moves no cursor, and says
/// so once: the next look reads the same lines again, though nothing moved, and tells the held
/// message then.
#[test]
fn a_held_message_whose_row_does_not_read_is_told_at_the_next_look_that_reads_it() {
    let telling = Telling::new(false);
    telling.direct_agents(&["bo"]);
    let mut passes = turns();
    passes.pass(&telling.hub);
    let before = passes.cursor();
    assert!(before.is_some(), "the first look");
    let (held, link) = telling.hold("Cut the header");
    // A field no build knows: the task's held rows do not read.
    let row = || telling.f.store.raw().row("held_message", held);
    row().set(json!({"stray": true})).unwrap();
    for _ in 0..2 {
        passes.pass(&telling.hub);
        assert!(telling.told().is_empty(), "{:?}", telling.told());
        assert_eq!(passes.cursor(), before, "no line taken");
    }
    let said = passes.said.0.get("lines").cloned().unwrap_or_default();
    assert!(
        said.starts_with("the board's lines: ") && said.contains("the held messages"),
        "{said}"
    );
    row().set(json!({"stray": null})).unwrap();
    passes.pass(&telling.hub);
    assert_eq!(
        telling.told(),
        [("held".into(), "email".into(), BO_EMAIL.into(), link)]
    );
    assert!(passes.cursor() > before, "{:?}", passes.cursor());
    assert!(!passes.said.0.contains_key("lines"), "the fault cleared");
}

/// Who of a board is told of a held message, by whose its task is: of a group, its members who may
/// pass it on; of a person, they where they may; of the person at the machine, or where none of
/// its owner may, every person of the board who may; never its author; none where nobody but its
/// author may, and none on a board that names nobody.
#[test]
fn who_is_told_of_a_held_message_is_who_may_pass_it_of_its_owner_else_of_the_board() {
    use super::passers;
    use symphony_board::people::ownership::{By, Kind, Owning};
    use symphony_board::people::{Group, Member, People};
    let passes = [Power::Speak, Power::DirectAgents];
    let member = |name: &str, group: &str, powers: &[Power]| Member {
        name: name.into(),
        groups: vec![group.into()],
        powers: powers.to_vec(),
        revision: 0,
    };
    let people = People {
        members: vec![
            member("ana", "design", &passes),
            member("bo", "design", &[Power::Speak]),
            member("cy", "engineering", &passes),
            member("dee", "design", &passes),
        ],
        groups: ["design", "engineering"]
            .map(|name| Group {
                name: name.into(),
                ..Group::default()
            })
            .into(),
        revision: 1,
    };
    let owning = |name: &str, kind: Kind| Owning {
        name: name.into(),
        kind,
        by: By::Named,
        place: None,
        unheld: None,
    };
    let design = owning("design", Kind::Group);
    assert_eq!(passers(&people, &design, "bo"), ["ana", "dee"]);
    assert_eq!(
        passers(&people, &design, "ana"),
        ["dee"],
        "never its author"
    );
    let engineering = owning("engineering", Kind::Group);
    assert_eq!(passers(&people, &engineering, "bo"), ["cy"]);
    assert_eq!(
        passers(&people, &engineering, "cy"),
        ["ana", "dee"],
        "none of engineering but its author: the board's"
    );
    assert_eq!(
        passers(&people, &owning("dee", Kind::Person), "bo"),
        ["dee"]
    );
    assert_eq!(
        passers(&people, &owning("dee", Kind::Person), "dee"),
        ["ana", "cy"],
        "its owner wrote it: the board's"
    );
    assert_eq!(
        passers(&people, &owning("bo", Kind::Person), "dee"),
        ["ana", "cy"],
        "bo may not pass it: the board's"
    );
    let machine = owning("owner", Kind::Machine);
    assert_eq!(passers(&people, &machine, "bo"), ["ana", "cy", "dee"]);
    let alone = People {
        members: vec![
            member("ana", "design", &passes),
            member("bo", "design", &[]),
        ],
        ..people.clone()
    };
    assert!(
        passers(&alone, &design, "ana").is_empty(),
        "the page tells the machine's person"
    );
    assert!(passers(&People::default(), &machine, "ana").is_empty());
}

#[test]
fn a_board_that_names_nobody_tells_no_one() {
    let telling = Telling::new(true);
    let mut passes = turns();
    passes.pass(&telling.hub);
    ask_everything(&telling);
    passes.pass(&telling.hub);
    assert!(telling.told().is_empty());
    assert_eq!(passes.runner.as_ref().and_then(Runner::next_due), None);
}

#[test]
fn the_worker_hands_what_a_wake_finds_and_ends_at_the_stop() {
    let telling = Telling::new(false);
    telling
        .hub
        .start_notices_with("symphony", bounds(), Box::new(|| ORIGIN.to_string()));
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

#[test]
fn a_second_start_starts_no_second_worker() {
    let telling = Telling::new(false);
    let hub = &telling.hub;
    hub.start_notices_with("symphony", bounds(), Box::new(|| ORIGIN.to_string()));
    let second = Arc::new(());
    let held = Arc::clone(&second);
    hub.start_notices_with(
        "symphony",
        bounds(),
        Box::new(move || {
            let _ = &held;
            ORIGIN.to_string()
        }),
    );
    assert_eq!(
        Arc::strong_count(&second),
        1,
        "its origin is let go as it returns: no worker took it"
    );
    hub.stop_notices();
}

#[test]
fn a_hub_dropped_without_a_stop_ends_its_worker_and_lets_the_record_go() {
    let Telling {
        tree,
        root,
        f: _board,
        hub,
        gate: _gate,
    } = Telling::new(false);
    let alive = Arc::new(());
    let held = Arc::clone(&alive);
    hub.start_notices_with(
        "symphony",
        bounds(),
        Box::new(move || {
            let _ = &held;
            ORIGIN.to_string()
        }),
    );
    let record = root.join("artifacts/notices.json");
    test_time::until_true("the worker's first look", hand_bound(), || record.is_file())
        .unwrap_or_else(|stall| panic!("{stall}"));
    drop(hub);
    // Its wake went with the hub: the worker ends and lets its turns go, its origin before its
    // runner, whose lock is then waited for on its own.
    test_time::until_true("the worker's turns let go", hand_bound(), || {
        Arc::strong_count(&alive) == 1
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    let machine = tree.path("machine");
    test_time::until_true("the record's lock let go", hand_bound(), || {
        let key = symphony::notices::Key::of(&machine).unwrap();
        Runner::open(&root, "symphony", key).is_ok()
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
}

/// A wall-clock backstop a stall alone reaches: six fake programs' runs on a loaded machine.
fn hand_bound() -> std::time::Duration {
    test_time::backstop(crate::notices::hand::MOST)
}

/// The server's hand bounds, each as the test's backstop, so a hand's outcome is the code's and
/// never a loaded machine's stall.
fn bounds() -> Bounds {
    Bounds {
        ends: test_time::backstop(Bounds::SERVER.ends),
        heard: test_time::backstop(Bounds::SERVER.heard),
    }
}

#[test]
fn what_the_machine_folder_does_not_let_it_trust_tells_no_one_said_once() {
    use std::os::unix::fs::PermissionsExt as _;
    let chmod = |path: &std::path::Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let telling = Telling::new(false);
    let machine = telling.tree.path("machine");
    // A key another account may read opens no runner, said once; mended, it opens one.
    symphony::notices::Key::of(&machine).unwrap();
    let key = machine.join("notices/record.key");
    chmod(&key, 0o644);
    let mut said = Said::default();
    assert!(telling.hub.open_notices("symphony", &mut said).is_none());
    assert!(
        said.0["key"].starts_with("nothing is told: "),
        "{:?}",
        said.0
    );
    chmod(&key, 0o600);
    let mut passes = turns();
    // Profiles that do not read: no look, said.
    let profiles = machine.join("people.json");
    let kept = std::fs::read(&profiles).unwrap();
    std::fs::write(&profiles, "{ not json").unwrap();
    passes.pass(&telling.hub);
    assert_eq!(passes.cursor(), None);
    assert!(passes.said.0.contains_key("profiles"));
    std::fs::write(&profiles, kept).unwrap();
    passes.pass(&telling.hub);
    assert!(!passes.said.0.contains_key("profiles"));
    let first = passes.cursor().unwrap();
    // Programs another account may write: nothing sent, said, and the look goes on.
    let programs = machine.join("notices/programs.toml");
    chmod(&programs, 0o666);
    ask_everything(&telling);
    passes.pass(&telling.hub);
    assert!(telling.told().is_empty());
    assert!(passes.said.0["programs"].contains("may be written by another account"));
    assert!(passes.cursor().unwrap() > first, "its lines are looked at");
    // Mended: what passed while it was refused is not told after.
    chmod(&programs, 0o600);
    passes.pass(&telling.hub);
    assert!(!passes.said.0.contains_key("programs"));
    assert!(telling.told().is_empty());
    // A hub with no machine folder opens no runner.
    let bare = Hub::new(&telling.root, &telling.root.join("build/client"));
    assert!(
        bare.open_notices("symphony", &mut Said::default())
            .is_none()
    );
}

#[test]
fn a_fault_is_said_once_until_it_changes_or_clears() {
    let mut said = Said::default();
    said.once("symphony", "board", "down".into());
    said.once("symphony", "board", "down".into());
    assert_eq!(said.0["board"], "down");
    said.once("symphony", "board", "refused".into());
    assert_eq!(said.0["board"], "refused");
    said.clear("board");
    assert!(said.0.is_empty());
}

#[test]
fn a_replacement_tells_nothing_until_its_gate_opens_then_tells_from_the_record_as_it_stands() {
    let telling = Telling::new(false);
    // The generation being replaced: its runner holds the record and tells.
    let mut old = turns();
    old.pass(&telling.hub);
    assert!(old.cursor().is_some());
    // The replacement's hub on the same project, its gate held until its hand-off commits.
    let gate = Arc::new(Gate::default());
    gate.hold();
    let mut replacement = Hub::new(&telling.root, &telling.root.join("build/client"));
    replacement.machine = telling.hub.machine.clone();
    let board = telling.f.other_database();
    replacement.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    replacement.refresh_page("", &telling.root).unwrap();
    replacement.gate_recorders(Arc::clone(&gate));
    let mut new = turns();
    let record = telling.root.join("artifacts/notices.json");
    let before = std::fs::read(&record).unwrap();
    ask_everything(&telling);
    replacement.refresh_page("", &telling.root).unwrap();
    replacement.observe(&telling.f.store);
    new.pass(&replacement);
    assert!(new.runner.is_none(), "a held gate reads no record");
    old.pass(&telling.hub);
    assert_eq!(telling.told().len(), 6, "the old generation tells");
    assert_ne!(std::fs::read(&record).unwrap(), before);
    // The hand-off commits: the old gate closes, the new one opens.
    telling.gate.close(1.0);
    gate.open(1.0);
    // A hand the old generation has under way still holds the record: the new reads it after.
    new.pass(&replacement);
    assert!(new.runner.is_none());
    assert!(new.said.0["lock"].contains("held by another server's notices"));
    old.pass(&telling.hub);
    assert!(old.runner.is_none(), "a closed gate lets the record go");
    new.pass(&replacement);
    assert!(new.runner.is_some());
    assert_eq!(
        new.cursor(),
        old_cursor(&record),
        "the record as the old one left it"
    );
    assert_eq!(telling.told().len(), 6, "nothing told twice");
    // From then the new generation tells what comes.
    let both = "- [ ] **new-palette** — Which palette?\n- [ ] **new-grid** — Which grid?\n";
    std::fs::write(telling.root.join(DOC), page_holding(both)).unwrap();
    replacement.refresh_page("", &telling.root).unwrap();
    new.pass(&replacement);
    assert_eq!(telling.told().len(), 8);
    assert!(
        telling.told()[6..]
            .iter()
            .all(|told| told.3.ends_with("#new-grid"))
    );
}

/// The cursor the record at `path` holds.
fn old_cursor(path: &std::path::Path) -> Option<i64> {
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    record["line"].as_i64()
}
