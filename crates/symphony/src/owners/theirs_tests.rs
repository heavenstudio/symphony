use super::*;
use symphony_api::{Card, Counts, PendingItem, Progress, QueueItem, TaskCounts};
use symphony_board::people::ownership::{Did, Line};
use symphony_board::people::{Group, Member, Power};

/// A board naming ana and cy (design) and bo (engineering), ana and bo holding decide, cy not.
fn people() -> People {
    let member = |name: &str, group: &str, powers: &[Power]| Member {
        name: name.into(),
        groups: vec![group.into()],
        powers: powers.to_vec(),
        ..Member::default()
    };
    let group = |name: &str| Group {
        name: name.into(),
        ..Group::default()
    };
    People {
        members: vec![
            member("ana", "design", &[Power::Decide]),
            member("bo", "engineering", &[Power::Decide]),
            member("cy", "design", &[Power::Read]),
        ],
        groups: vec![group("design"), group("engineering")],
        revision: 7,
    }
}

/// The map giving each `(place, group)`.
fn map(entries: &[(&str, &str)]) -> Map {
    let lines: Vec<Line> = (entries.iter().enumerate())
        .map(|(at, (place, group))| Line {
            n: at as i64 + 1,
            at: 1.0,
            by: "ana".into(),
            did: Did::Map,
            place: Some((*place).into()),
            thing: None,
            name: Some((*group).into()),
            from: None,
        })
        .collect();
    Map::of(&lines)
}

/// Deciders said outright: each `(doc, slug, names)`.
fn deciders(cards: &[(&str, &str, &[&str])]) -> Deciders {
    Deciders {
        revision: (1, 1),
        cards: (cards.iter())
            .map(|(doc, slug, names)| {
                let names = names.iter().map(|name| name.to_string()).collect();
                ((doc.to_string(), slug.to_string()), names)
            })
            .collect(),
    }
}

fn card(doc: &str, slug: &str) -> Card {
    Card {
        postponed: false,
        slug: slug.into(),
        doc: doc.into(),
        doc_title: "Doc".into(),
        title: format!("Title of {slug}"),
        summary: String::new(),
        why: "design".into(),
        posted: None,
        pictures: Vec::new(),
        task: None,
    }
}

fn item(path: &str, slug: &str) -> QueueItem {
    QueueItem {
        href: format!("/{path}#{slug}"),
        path: path.into(),
        slug: slug.into(),
        title: slug.into(),
        kind: "auto".into(),
        text: String::new(),
        branch: None,
        ask: None,
    }
}

fn slugs<'a>(cards: impl IntoIterator<Item = &'a Card>) -> Vec<&'a str> {
    cards.into_iter().map(|card| card.slug.as_str()).collect()
}

const DESIGN: &str = "docs/design/palette.md";
const ENGINEERING: &str = "docs/engineering/build.md";
/// A branch's proposal in design's document.
const PROPOSAL: &str = "@app-x/docs/design/palette.md";

/// Who decides each card of the queue [`queue_of`] holds.
fn of_queue() -> Deciders {
    deciders(&[
        (DESIGN, "a", &["ana"]),
        (ENGINEERING, "b", &["bo"]),
        (DESIGN, "c", &["ana"]),
        (PROPOSAL, "p", &["ana"]),
        (DESIGN, "s", &["ana"]),
        (ENGINEERING, "t", &["bo"]),
    ])
}

/// Design's and engineering's cards, mixed, with a proposal, a spot-check of each and one card
/// waiting on an agent.
fn queue_of() -> Queue {
    Queue {
        waiting: vec![
            card(DESIGN, "a"),
            card(ENGINEERING, "b"),
            card(DESIGN, "c"),
            card(PROPOSAL, "p"),
        ],
        spot_check: vec![item(DESIGN, "s"), item(ENGINEERING, "t")],
        agent: vec![item(ENGINEERING, "asked")],
        branches: Vec::new(),
    }
}

#[test]
fn a_cards_deciders_are_those_of_its_owner_who_hold_decide_read_as_the_notices_read_it() {
    let tree = crate::test_tree::Tree::checkout();
    tree.write(
        DESIGN,
        "# Palette\n\n<!-- register:start -->\n\
         - [ ] **new-palette** — Which palette?\n\
         - [ ] **new-type** — *For:* bo. Which typeface?\n\
         <!-- register:end -->\n",
    );
    let people = people();
    let map = map(&[("folder:docs/design", "design"), ("default", "engineering")]);
    let card = |doc: &str, slug: &str| (doc.to_string(), slug.to_string());
    let read = Deciders::read(
        &people,
        &map,
        &tree.path(""),
        &[
            card(DESIGN, "new-palette"),
            card(DESIGN, "new-type"),
            card(&format!("/{ENGINEERING}"), "lost"),
        ],
    );
    assert_eq!(read.revision, (7, 2));
    assert!(read.decides(DESIGN, "new-palette", "ana"));
    assert!(
        !read.decides(DESIGN, "new-palette", "cy"),
        "cy is of design and holds no decide"
    );
    assert!(!read.decides(DESIGN, "new-palette", "bo"));
    assert!(read.decides(DESIGN, "new-type", "bo"), "written for bo");
    assert!(!read.decides(DESIGN, "new-type", "ana"));
    // A document no file is goes by its places, then the default; its leading slash is no part of
    // its key.
    assert!(read.decides(ENGINEERING, "lost", "bo"));
    assert!(read.decides(&format!("/{ENGINEERING}"), "lost", "bo"));
    assert!(!read.decides(ENGINEERING, "never-read", "bo"));
    assert!(
        !read.decides(DESIGN, "new-palette", "owner"),
        "no name is the machine's"
    );
    // On a board that names nobody every card is the person at the machine's: no one else's.
    let nobody = Deciders::read(
        &People::default(),
        &map,
        &tree.path(""),
        &[card(DESIGN, "new-palette")],
    );
    assert!(!nobody.decides(DESIGN, "new-palette", "ana"));
}

#[test]
fn the_cards_asked_of_are_the_waiting_then_the_spot_checks_each_once() {
    let mut queue = queue_of();
    queue.spot_check.push(item(&format!("/{DESIGN}"), "a"));
    assert_eq!(
        cards(&queue),
        [
            (DESIGN, "a"),
            (ENGINEERING, "b"),
            (DESIGN, "c"),
            (PROPOSAL, "p"),
            (DESIGN, "s"),
            (ENGINEERING, "t"),
        ]
        .map(|(doc, slug)| (doc.to_string(), slug.to_string()))
    );
    assert!(cards(&Queue::default()).is_empty());
}

#[test]
fn a_reader_is_shown_their_waiting_cards_and_spot_checks_in_the_queues_order_and_every_agents() {
    let (queue, deciders) = (queue_of(), of_queue());
    let ana = super::queue(&queue, Some(&deciders), "ana");
    assert_eq!(slugs(&ana.waiting), ["a", "c", "p"]);
    assert_eq!(ana.spot_check, [item(DESIGN, "s")]);
    assert_eq!(
        ana.agent, queue.agent,
        "what waits on an agent is read by all"
    );
    let bo = super::queue(&queue, Some(&deciders), "bo");
    assert_eq!(slugs(&bo.waiting), ["b"]);
    assert_eq!(bo.spot_check, [item(ENGINEERING, "t")]);
    let cy = super::queue(&queue, Some(&deciders), "cy");
    assert!(cy.waiting.is_empty() && cy.spot_check.is_empty());
    assert_eq!(cy.agent, queue.agent);
    // Where no deciders were read, a reader the board names is shown no card.
    let unread = super::queue(&queue, None, "ana");
    assert!(unread.waiting.is_empty() && unread.spot_check.is_empty());
    assert_eq!(unread.agent, queue.agent);
}

#[test]
fn a_board_is_shown_its_reader_their_first_cards_and_their_counts_and_the_rest_whole() {
    let mut queue = queue_of();
    // More of design's than the board shows.
    for n in 0..6 {
        queue.waiting.push(card(DESIGN, &format!("more-{n}")));
    }
    let mut deciders = of_queue();
    for n in 0..6 {
        deciders.cards.insert(
            (DESIGN.to_string(), format!("more-{n}")),
            vec!["ana".to_string()],
        );
    }
    let tasks = TaskCounts {
        in_progress: 3,
        in_review: 1,
        agents_live: 2,
    };
    let whole = Board {
        version: 41,
        build: "/assets/symphony-shell-x.js".into(),
        ping_ms: 1000,
        next: queue.waiting.iter().take(5).cloned().collect(),
        counts: Counts {
            waiting: 10,
            docs_waiting: 2,
            spot_check: 2,
            answered_today: 4,
            tasks: Some(tasks.clone()),
        },
        progress: Progress {
            regions: Vec::new(),
            docs: 12,
            decisions_waiting: 10,
        },
    };
    let theirs = super::queue(&queue, Some(&deciders), "ana");
    let ana = board(&whole, &theirs);
    assert_eq!(slugs(&ana.next), ["a", "c", "p", "more-0", "more-1"]);
    assert_eq!(ana.counts.waiting, 9);
    assert_eq!(ana.counts.docs_waiting, 1, "a proposal is its document's");
    assert_eq!(ana.counts.spot_check, 1);
    assert_eq!(ana.progress.decisions_waiting, 9);
    assert_eq!(
        (ana.version, ana.build.as_str(), ana.ping_ms),
        (41, whole.build.as_str(), 1000)
    );
    assert_eq!(
        ana.counts.answered_today, 4,
        "an agent's answer is read by all"
    );
    assert_eq!(ana.counts.tasks, Some(tasks));
    assert_eq!(ana.progress.docs, 12);
    let bo = board(&whole, &super::queue(&queue, Some(&deciders), "bo"));
    assert_eq!(slugs(&bo.next), ["b"]);
    assert_eq!(
        (
            bo.counts.waiting,
            bo.counts.docs_waiting,
            bo.counts.spot_check
        ),
        (1, 1, 1)
    );
    let cy = board(&whole, &super::queue(&queue, Some(&deciders), "cy"));
    assert!(cy.next.is_empty());
    assert_eq!(
        (
            cy.counts.waiting,
            cy.counts.docs_waiting,
            cy.counts.spot_check
        ),
        (0, 0, 0)
    );
    assert_eq!(cy.progress.decisions_waiting, 0);
}

#[test]
fn the_pending_list_keeps_its_readers_cards_and_a_branchs_document_only_for_theirs() {
    let pending_item = |path: &str, slug: &str| PendingItem {
        path: path.into(),
        slug: slug.into(),
        href: format!("/{path}#{slug}"),
        branch: path.starts_with('@').then(|| "app/x".to_string()),
    };
    const BO_BRANCH: &str = "@app-y/docs/engineering/build.md";
    let pending = Pending {
        items: vec![
            pending_item(DESIGN, "a"),
            pending_item(ENGINEERING, "b"),
            pending_item(PROPOSAL, "p"),
            pending_item(BO_BRANCH, "q"),
        ],
        docs: vec![
            DESIGN.into(),
            ENGINEERING.into(),
            "docs/README.md".into(),
            PROPOSAL.into(),
            BO_BRANCH.into(),
        ],
    };
    let mut deciders = of_queue();
    deciders.cards.insert(
        (BO_BRANCH.to_string(), "q".to_string()),
        vec!["bo".to_string()],
    );
    let ana = super::pending(&pending, Some(&deciders), "ana");
    let paths = |pending: &Pending| -> Vec<String> {
        pending.items.iter().map(|item| item.href.clone()).collect()
    };
    assert_eq!(
        paths(&ana),
        [format!("/{DESIGN}#a"), format!("/{PROPOSAL}#p")]
    );
    assert_eq!(
        ana.docs,
        [DESIGN, ENGINEERING, "docs/README.md", PROPOSAL],
        "every document of the project, and her branch's alone"
    );
    let bo = super::pending(&pending, Some(&deciders), "bo");
    assert_eq!(
        paths(&bo),
        [format!("/{ENGINEERING}#b"), format!("/{BO_BRANCH}#q")]
    );
    assert_eq!(bo.docs, [DESIGN, ENGINEERING, "docs/README.md", BO_BRANCH]);
    let unread = super::pending(&pending, None, "ana");
    assert!(unread.items.is_empty());
    assert_eq!(unread.docs, [DESIGN, ENGINEERING, "docs/README.md"]);
}
