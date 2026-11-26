//! The comments held on documents the hub lists on its pages' queues (`super`), on a scratch board
//! naming ana (design) and bo (engineering), who may direct agents, and cy (design), who may not:
//! what cy leaves on design's document is listed whole on main's page and a branch's and counted,
//! read at the tick's first look and after a read that failed, and gone once passed on; a named
//! reader's feed moves the counts of the one it waits on alone; and a board that names nobody
//! reads and draws as before.

use super::super::Hub;
use super::super::theirs::sent_to;
use crate::testing::board::{Fixture, Seed};
use crate::testing::{Tree, git, repository, repository_holding};
use serde_json::{Value, json};
use symphony_api::QueueHeld;
use symphony_board::held::{Block, Place};
use symphony_board::lock::Recovered as _;
use symphony_board::people::Power;
use symphony_board::people::ownership::Place as Mapped;

const DESIGN: &str = "docs/design/palette.md";
const ENGINEERING: &str = "docs/engineering/build.md";
/// The branch whose worktree holds the project too.
const BRANCH: &str = "design~policy";
/// A word of cy's question, which no page's queue carries.
const WORDS: &str = "quartzlamp";

#[test]
fn a_held_comment_moved_is_one_event_on_every_page() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let before = hub.cursor("").unwrap();
    let quiet = hub.pages.read().unwrap()[""].history.len();
    hub.held_moved();
    let pages = hub.pages.read().unwrap();
    let history = &pages[""].history;
    assert_eq!(history.len(), quiet + 1, "one event");
    let event: Value = serde_json::from_str(&history.back().unwrap().json).unwrap();
    assert_eq!(event["version"], before + 1);
    assert_eq!(event["changes"], json!([{"region": "held", "data": {}}]));
}

/// A project with design's and engineering's documents and a branch's worktree, its scratch board
/// naming ana, bo and cy unless `nobody`, and its hub, main's page and the branch's read. The
/// board is not the project's own, so the hub's own read of it fails, and a tick reads it.
struct Held {
    _tree: Tree,
    f: Fixture,
    hub: Hub,
}

impl Held {
    fn new(nobody: bool) -> Held {
        let tree = Tree::new();
        let root = repository_holding(&tree, "main", DESIGN, "# Palette\n\nWarm lamps.\n");
        tree.write(&format!("main/{ENGINEERING}"), "# Build\n");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "Engineering's document"]);
        let branch = tree.path("proposal");
        let worktree = ["worktree", "add", "-b", "design/policy"];
        git(
            &root,
            &[&worktree[..], &[branch.to_str().unwrap()]].concat(),
        );
        let f = Fixture::new(Seed::default());
        if !nobody {
            let store = &f.store;
            for group in ["design", "engineering"] {
                store.add_group(group, &[Power::Speak], "owner").unwrap();
                let folder = Mapped::Folder(format!("docs/{group}"));
                store.map_place(&folder, group, "owner").unwrap();
            }
            for (name, group, powers) in [
                ("ana", "design", &[Power::DirectAgents][..]),
                ("bo", "engineering", &[Power::DirectAgents]),
                ("cy", "design", &[]),
            ] {
                (store.add_person(name, &[group.into()], powers, "owner")).unwrap();
            }
        }
        let hub = Hub::new(&root, &root.join("build/client"));
        hub.refresh_docs(None).unwrap();
        hub.ensure_page(BRANCH).unwrap();
        Held {
            _tree: tree,
            f,
            hub,
        }
    }

    /// cy's question after the first paragraph of design's document: its number.
    fn hold(&self) -> i64 {
        let place = Place {
            path: DESIGN.into(),
            scope: None,
            kind: "question".into(),
            card: None,
            block: Some(Block {
                key: "intro".into(),
                line: 3,
                text: "Warm lamps.".into(),
            }),
        };
        let cy = self.f.other().for_person("cy");
        let text = format!("Too many {WORDS}s");
        cy.hold_on_document("cy", &text, &place).unwrap().id
    }

    /// The version of `scope`'s page.
    fn version(&self, scope: &str) -> u64 {
        self.hub.pages.read().recover()[scope].board.version
    }

    /// The queue's held comments and the count of them on `scope`'s page.
    fn listed(&self, scope: &str) -> (Vec<QueueHeld>, u32) {
        let pages = self.hub.pages.read().recover();
        let page = &pages[scope];
        (page.queue.held.clone(), page.board.counts.held)
    }

    /// What the feed of `scope`'s page sends `reader` after `version`, the events joined: as it is
    /// to the person at the machine (`None`), else as [`sent_to`] a named reader.
    fn sent(&self, scope: &str, version: u64, reader: Option<&str>) -> String {
        let pages = self.hub.pages.read().recover();
        (pages[scope].history.iter())
            .filter(|event| event.version > version)
            .map(|event| match reader {
                Some(name) => sent_to(&event.json, event.waits.as_ref(), name),
                None => event.json.clone(),
            })
            .collect()
    }
}

/// The row the queue lists of the comment `id` cy held on design's document.
fn row(id: i64) -> QueueHeld {
    QueueHeld {
        id,
        href: format!("/{DESIGN}#held-{id}"),
        doc_title: "Palette".into(),
        author: "cy".into(),
    }
}

#[test]
fn the_ticks_first_look_lists_what_was_held_on_every_page_whole_and_to_its_passer_alone() {
    let q = Held::new(false);
    let id = q.hold();
    let (main, branch) = (q.version(""), q.version(BRANCH));
    q.hub.people_read(&q.f.store);
    for scope in ["", BRANCH] {
        assert_eq!(q.listed(scope), (vec![row(id)], 1), "{scope:?}");
    }
    for (scope, since) in [("", main), (BRANCH, branch)] {
        let machine = q.sent(scope, since, None);
        assert!(machine.contains(r#""region":"held""#), "{machine}");
        assert!(machine.contains(r#""held":1"#), "{scope:?}: {machine}");
        let ana = q.sent(scope, since, Some("ana"));
        assert!(ana.contains(r#""held":1"#), "{scope:?}: {ana}");
        for name in ["cy", "bo"] {
            let theirs = q.sent(scope, since, Some(name));
            assert!(!theirs.contains(r#""held":1"#), "{name}: {theirs}");
            assert!(theirs.contains(r#""region":"held","data":{}"#), "{name}");
        }
    }
    let pages = q.hub.pages.read().recover();
    let whole = serde_json::to_string(&pages[""].queue).unwrap();
    assert!(!whole.contains(WORDS), "{whole}");
    let deciders = pages[""].deciders.clone().unwrap();
    assert!(deciders.passes(id, "ana"));
    assert!(!deciders.passes(id, "cy") && !deciders.passes(id, "bo"));
}

#[test]
fn a_read_that_fails_leaves_the_list_and_the_next_tick_reads_it_again_so_a_pass_leaves_it() {
    let q = Held::new(false);
    q.hub.people_read(&q.f.store);
    assert_eq!(q.listed(""), (Vec::new(), 0));
    let id = q.hold();
    // The hub's own read finds no board of the project's: the list stays as it was, unread.
    let before = q.version("");
    q.hub.held_moved();
    assert_eq!(q.listed(""), (Vec::new(), 0));
    assert_eq!(
        q.sent("", before, None),
        json!({"version": before + 1, "changes": [{"region": "held", "data": {}}]}).to_string()
    );
    q.hub.people_read(&q.f.store);
    assert_eq!(q.listed(""), (vec![row(id)], 1));
    // A refresh of the documents keeps it, its document's title read again.
    q.hub.refresh_docs(None).unwrap();
    assert_eq!(q.listed(""), (vec![row(id)], 1));
    assert_eq!(q.listed(BRANCH), (vec![row(id)], 1));
    // Read once, it is not read again at each tick: passed on, it goes at the next move.
    let ana = q.f.other().for_person("ana");
    ana.pass_on_document(id, "ana").unwrap();
    q.hub.people_read(&q.f.store);
    assert_eq!(q.listed(""), (vec![row(id)], 1), "nothing moved it yet");
    q.hub.held_moved();
    q.hub.people_read(&q.f.store);
    assert_eq!(q.listed(""), (Vec::new(), 0));
    assert_eq!(q.listed(BRANCH), (Vec::new(), 0));
}

#[test]
fn a_change_of_the_people_tells_the_queue_who_a_held_comment_waits_on_now() {
    let q = Held::new(false);
    let id = q.hold();
    q.hub.people_read(&q.f.store);
    let before = q.version("");
    // ana no longer directs agents: none of design may, so every person who may is told.
    let ana = symphony_board::people::Holder::Person("ana");
    (q.f.store.revoke(ana, &[Power::DirectAgents], "owner")).unwrap();
    q.hub.people_read(&q.f.store);
    let deciders = q.hub.pages.read().recover()[""].deciders.clone().unwrap();
    assert!(deciders.passes(id, "bo") && !deciders.passes(id, "ana"));
    let bo = q.sent("", before, Some("bo"));
    assert!(bo.contains(r#""region":"held","data":{}"#), "{bo}");
    assert!(bo.contains(r#""held":1"#), "his count moved: {bo}");
    let ana = q.sent("", before, Some("ana"));
    assert!(!ana.contains(r#""held":"#), "hers is none: {ana}");
}

#[test]
fn a_board_that_names_nobody_reads_and_draws_as_before() {
    let q = Held::new(true);
    let quiet: Vec<usize> = (["", BRANCH].iter())
        .map(|scope| q.hub.pages.read().recover()[*scope].history.len())
        .collect();
    q.hub.people_read(&q.f.store);
    q.hub.refresh_docs(None).unwrap();
    let pages = q.hub.pages.read().recover();
    for (scope, quiet) in ["", BRANCH].iter().zip(quiet) {
        let page = &pages[*scope];
        assert_eq!(page.history.len(), quiet, "{scope:?}: no event");
        let queue = serde_json::to_value(&page.queue).unwrap();
        assert_eq!(queue.get("held"), None, "{queue}");
        let counts = serde_json::to_value(&page.board.counts).unwrap();
        assert_eq!(counts.get("held"), None, "{counts}");
    }
}
