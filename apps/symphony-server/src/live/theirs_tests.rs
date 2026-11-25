//! What waits, said to each reader (docs/product/analysis/users-and-identity.md, step 4 of "The way
//! from one owner"), on a scratch board naming ana (design) and bo (engineering), each holding
//! decide, and cy (design) holding none, design mapped to `docs/design` and engineering to
//! `docs/engineering`: each named reader is shown the cards they decide alone, on main's page and a
//! branch's, in its board, counts and folders, and the person at the machine every card; the feed
//! sends a named reader no byte of a card they do not decide, a card posted or a change of the
//! people; and a board that names nobody carries nothing more on any event, and adds none.

use super::super::{Event, Hub};
use super::sent_to;
use crate::testing::board::{Fixture, Seed};
use crate::testing::{Tree, git, repository_holding};
use serde_json::{Value, json};
use symphony_board::lock::Recovered as _;
use symphony_board::people::Power;
use symphony_board::people::ownership::Place;

const DESIGN: &str = "docs/design/palette.md";
const ENGINEERING: &str = "docs/engineering/build.md";
/// The branch whose worktree proposes a card in design's document.
const BRANCH: &str = "design~policy";
/// The words of design's card posted while the feed is followed.
const POSTED: &str = "Which brand colour is ours?";

/// Design's document, its one card and `extra` after it.
fn design(extra: &str) -> String {
    format!(
        "# Palette\n\n<!-- register:start -->\n- [ ] **new-palette** — Which palette?\n\
         {extra}<!-- register:end -->\n"
    )
}

const ENGINEERING_PAGE: &str = "# Build\n\n<!-- register:start -->\n\
                                - [ ] **new-build** — Which build tool?\n<!-- register:end -->\n";

/// A project with design's and engineering's documents, a branch proposing a card in design's,
/// its board and its hub, main's page and the branch's read, and the board's people looked at.
struct Queues {
    tree: Tree,
    f: Fixture,
    hub: Hub,
}

impl Queues {
    /// The board names ana, bo and cy unless `nobody`.
    fn new(nobody: bool) -> Queues {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", DESIGN, &design(""));
        tree.write(&format!("main/{ENGINEERING}"), ENGINEERING_PAGE);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "Engineering's document"]);
        let branch = tree.path("proposal");
        let worktree = ["worktree", "add", "-b", "design/policy"];
        git(
            &root,
            &[&worktree[..], &[branch.to_str().unwrap()]].concat(),
        );
        tree.write(
            &format!("proposal/{DESIGN}"),
            &design("- [ ] **proposed** — A branch proposal.\n"),
        );
        git(&branch, &["commit", "-qam", "Post the branch card"]);
        let f = Fixture::new(Seed::default());
        if !nobody {
            let store = &f.store;
            for group in ["design", "engineering"] {
                store.add_group(group, &[], "owner").unwrap();
            }
            for (name, group, power) in [
                ("ana", "design", Power::Decide),
                ("bo", "engineering", Power::Decide),
                ("cy", "design", Power::Speak),
            ] {
                (store.add_person(name, &[group.into()], &[power], "owner")).unwrap();
            }
            for (folder, group) in [
                ("docs/design", "design"),
                ("docs/engineering", "engineering"),
            ] {
                (store.map_place(&Place::Folder(folder.into()), group, "owner")).unwrap();
            }
        }
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        hub.ensure_page(BRANCH).unwrap();
        hub.people_read(&f.store);
        Queues { tree, f, hub }
    }

    /// The version of `scope`'s page.
    fn version(&self, scope: &str) -> u64 {
        self.hub.pages.read().recover()[scope].board.version
    }

    /// The events of `scope`'s page after `version`.
    fn events(&self, scope: &str, version: u64) -> Vec<Event> {
        let pages = self.hub.pages.read().recover();
        (pages[scope].history.iter())
            .filter(|event| event.version > version)
            .cloned()
            .collect()
    }

    /// What the feed of `scope`'s page sends `reader` after `version`, event by event: as it is to
    /// the person at the machine (`None`), else as [`sent_to`] a named reader.
    fn sent(&self, scope: &str, version: u64, reader: Option<&str>) -> Vec<String> {
        (self.events(scope, version).iter())
            .map(|event| match reader {
                Some(name) => sent_to(&event.json, event.waits.as_ref(), name),
                None => event.json.clone(),
            })
            .collect()
    }

    /// Design's document posting the card [`POSTED`], read again.
    fn post(&self) {
        self.tree.write(
            &format!("main/{DESIGN}"),
            &design(&format!("- [ ] **brand-new** — {POSTED}\n")),
        );
        self.hub.refresh_docs(None).unwrap();
    }
}

/// The slugs of `cards` in their order.
fn slugs(cards: &[symphony_api::Card]) -> Vec<&str> {
    cards.iter().map(|card| card.slug.as_str()).collect()
}

/// The regions of the event `json`'s changes, in their order.
fn regions(json: &str) -> Vec<String> {
    let event: Value = serde_json::from_str(json).unwrap();
    (event["changes"].as_array().unwrap().iter())
        .map(|change| change["region"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn each_named_reader_is_shown_the_cards_they_decide_and_the_machine_every_card() {
    let q = Queues::new(false);
    for scope in ["", BRANCH] {
        let pages = q.hub.pages.read().recover();
        let page = &pages[scope];
        let shown = |name: &str| {
            let deciders = page.deciders.as_deref();
            super::shown(&page.board, &page.queue, &page.pending, deciders, name)
        };
        let (ana, bo, cy) = (shown("ana"), shown("bo"), shown("cy"));
        assert_eq!(
            slugs(&ana.queue.waiting),
            ["new-palette", "proposed"],
            "{scope}"
        );
        assert_eq!(slugs(&ana.board.next), ["new-palette", "proposed"]);
        assert_eq!(
            (ana.board.counts.waiting, ana.board.counts.docs_waiting),
            (2, 1),
            "{scope}: main's card and the branch's proposal, in one document"
        );
        assert_eq!(slugs(&bo.queue.waiting), ["new-build"], "{scope}");
        assert_eq!(bo.board.counts.waiting, 1);
        assert_eq!(bo.board.progress.decisions_waiting, 1);
        assert_eq!(
            (bo.pending.items.iter())
                .map(|item| item.href.as_str())
                .collect::<Vec<_>>(),
            [format!("/{ENGINEERING}#new-build")]
        );
        assert!(
            !bo.pending.docs.iter().any(|doc| doc.starts_with('@')),
            "{scope}: the branch's document is design's: {:?}",
            bo.pending.docs
        );
        assert!(cy.queue.waiting.is_empty(), "of design, holding no decide");
        assert_eq!(
            page.board.counts.waiting, 3,
            "{scope}: the page itself, whole"
        );
        drop(pages);
        let folder = |reader: Option<&str>, path: &str| {
            let folder = q.hub.folder(scope, "docs", reader).unwrap();
            let found = folder.entries.iter().find(|e| e.path.ends_with(path));
            found.unwrap().waiting
        };
        let counted = |reader| {
            (
                folder(reader, "docs/design"),
                folder(reader, "docs/engineering"),
            )
        };
        assert_eq!(counted(Some("ana")), (2, 0), "{scope}");
        assert_eq!(counted(Some("bo")), (0, 1), "{scope}");
        assert_eq!(
            counted(None),
            (2, 1),
            "{scope}: the person at the machine's"
        );
    }
    let board = |reader| q.hub.main_board(reader).unwrap();
    assert_eq!(slugs(&board(Some("bo")).next), ["new-build"]);
    assert_eq!(board(Some("bo")).counts.waiting, 1);
    assert_eq!(board(Some("cy")).counts.waiting, 0);
    assert_eq!(board(None).counts.waiting, 3);
    assert_eq!(board(None).version, q.version(""));
}

#[test]
fn a_card_posted_reaches_the_feed_of_those_who_decide_it_and_no_byte_of_it_anyone_elses() {
    let q = Queues::new(false);
    let since = q.version("");
    q.post();
    let ana = q.sent("", since, Some("ana")).concat();
    assert!(ana.contains("brand-new") && ana.contains(POSTED), "{ana}");
    let machine = q.sent("", since, None).concat();
    assert!(machine.contains("brand-new") && machine.contains(POSTED));
    let bo = q.sent("", since, Some("bo"));
    assert!(!bo.is_empty(), "every event is sent, its version kept");
    for (sent, event) in bo.iter().zip(q.events("", since)) {
        for leak in ["brand-new", POSTED, "palette", "proposed", "\"counts\""] {
            assert!(!sent.contains(leak), "{leak} in {sent}");
        }
        let regions = regions(sent);
        assert!(
            !regions
                .iter()
                .any(|region| super::REGIONS.contains(&region.as_str())),
            "bo's view did not move: {regions:?}"
        );
        // What else the event says, it says to him as to everyone.
        let shared: Value = serde_json::from_str(&event.json).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(sent).unwrap()["version"],
            shared["version"]
        );
    }
    // Ana's view moved by the one card: it is upserted and placed, and her counts follow.
    let ana: Vec<Value> = (q.sent("", since, Some("ana")).iter())
        .flat_map(|sent| {
            let event: Value = serde_json::from_str(sent).unwrap();
            event["changes"].as_array().unwrap().clone()
        })
        .collect();
    let counts = ana
        .iter()
        .find(|change| change["region"] == "counts")
        .unwrap();
    assert_eq!(counts["data"]["waiting"], 3);
    let upserted: Vec<&str> = (ana.iter())
        .filter(|change| change["region"] == "queue-item")
        .map(|change| change["data"]["key"].as_str().unwrap())
        .collect();
    assert_eq!(upserted, [format!("{DESIGN}#brand-new")]);
}

#[test]
fn a_change_of_the_people_is_a_bare_version_to_the_machine_and_each_readers_own_diff() {
    let q = Queues::new(false);
    let since = q.version("");
    // Nothing moved: a look again publishes nothing.
    q.hub.people_read(&q.f.store);
    assert_eq!(q.version(""), since);
    q.f.store.join_group("bo", "design", "owner").unwrap();
    q.hub.people_read(&q.f.store);
    let version = since + 1;
    assert_eq!(q.version(""), version);
    assert_eq!(
        q.sent("", since, None),
        [format!(r#"{{"version":{version},"changes":[]}}"#)],
        "the machine's stream: a version, no card's data"
    );
    assert_eq!(
        q.sent("", since, Some("ana")),
        [format!(r#"{{"version":{version},"changes":[]}}"#)],
        "ana's view did not move"
    );
    let bo = q.sent("", since, Some("bo")).concat();
    for card in ["new-palette", "proposed"] {
        assert!(bo.contains(&format!("{card}\"")), "{card} became his: {bo}");
    }
    // A branch's page takes main's deciders, and says the same.
    assert!(
        q.sent(BRANCH, q.version(BRANCH) - 1, Some("bo"))
            .concat()
            .contains("new-palette")
    );
    // Out of design again, he is told those cards went: their keys, which he held.
    let since = q.version("");
    q.f.store.leave_group("bo", "design", "owner").unwrap();
    q.hub.people_read(&q.f.store);
    let bo = q.sent("", since, Some("bo")).concat();
    assert!(bo.contains(r#""removed":true"#), "{bo}");
    assert!(!bo.contains("Which palette?"), "no card's words: {bo}");
    // Its people all removed, the board names nobody: one event says the cards went, and no later
    // one carries anything for a named reader.
    for name in ["ana", "bo", "cy"] {
        q.f.store.remove_person(name, "owner").unwrap();
    }
    let since = q.version("");
    q.hub.people_read(&q.f.store);
    let events = q.events("", since);
    assert_eq!(events.len(), 1);
    assert!(
        q.sent("", since, Some("ana"))
            .concat()
            .contains(r#""removed":true"#)
    );
    q.post();
    assert!(
        q.events("", since + 1)
            .iter()
            .all(|event| event.waits.is_none())
    );
    assert!(
        !q.sent("", since + 1, Some("ana"))
            .concat()
            .contains("brand-new")
    );
}

#[test]
fn a_board_naming_nobody_carries_nothing_more_on_any_event_and_adds_none() {
    let q = Queues::new(true);
    let since = q.version("");
    q.hub.people_read(&q.f.store);
    assert_eq!(q.version(""), since, "a look at nobody publishes nothing");
    q.post();
    let events = q.events("", since);
    assert!(!events.is_empty());
    for event in &events {
        assert!(event.waits.is_none());
        assert!(!event.json.contains("deciders"));
    }
    assert!(q.sent("", since, None).concat().contains("brand-new"));
    let pages = q.hub.pages.read().recover();
    assert!(pages[""].deciders.is_none() && pages[BRANCH].deciders.is_none());
    drop(pages);
    // A reader it would name is shown no card, there being no one to decide but the machine.
    assert_eq!(q.hub.main_board(Some("ana")).unwrap().counts.waiting, 0);
    assert_eq!(q.hub.main_board(None).unwrap().counts.waiting, 4);
}

#[test]
fn a_named_readers_event_keeps_every_other_region_and_one_that_does_not_read_is_sent_as_it_is() {
    let event = json!({"version": 9, "changes": [
        {"region": "document", "data": {"path": DESIGN}},
        {"region": "counts", "data": {"waiting": 5}},
        {"region": "queue-item", "data": {"key": "docs/x.md#y", "value": {}}},
        {"region": "queue-item-order", "data": {"key": "docs/x.md#y", "after": null}},
        {"region": "pending", "data": {"key": "/docs/x.md#y", "removed": true}},
        {"region": "docs", "data": ["docs/x.md"]},
        {"region": "client", "data": {"build": "/assets/next.js"}},
        {"region": "activity", "data": {"key": "a", "value": {}}},
    ]})
    .to_string();
    let sent = sent_to(&event, None, "bo");
    assert_eq!(regions(&sent), ["document", "client", "activity"]);
    assert_eq!(sent_to("not json", None, "bo"), "not json");
    let bare = json!({"version": 9}).to_string();
    assert_eq!(sent_to(&bare, None, "bo"), bare);
}

#[test]
fn a_spot_check_of_a_card_a_reader_decides_reaches_them_alone_keyed_by_its_link() {
    let q = Queues::new(false);
    let pages = q.hub.pages.read().recover();
    let page = &pages[""];
    let before = super::Waits::of(page);
    let mut after = super::Waits::of(page);
    after.queue.spot_check.push(symphony_api::QueueItem {
        href: format!("/{DESIGN}#new-palette"),
        path: DESIGN.into(),
        slug: "new-palette".into(),
        title: "Which palette?".into(),
        kind: "spot".into(),
        text: String::new(),
        branch: None,
        ask: None,
    });
    let regions = |name: &str| -> Vec<(String, Value)> {
        (super::changes(&before, &after, name).into_iter())
            .map(|change| {
                (
                    change["region"].as_str().unwrap().to_string(),
                    change["data"].clone(),
                )
            })
            .collect()
    };
    let ana = regions("ana");
    let spot = ana
        .iter()
        .find(|(region, _)| region == "spot-check")
        .unwrap();
    assert_eq!(spot.1["key"], format!("/{DESIGN}#new-palette"));
    assert!(
        ana.iter()
            .any(|(region, data)| region == "counts" && data["spot_check"] == 1)
    );
    assert_eq!(regions("bo"), [], "not his: nothing moved for him");
}
