//! The kit's test harness (`symphony_plugin::testing`) on this repository's test plugin, the one its
//! process serves (`every::PIPED`, crates/symphony/tests/project/seams/every.rs): what it declares,
//! and its components and its card kind asked as a host asks them, through the protocol's requests
//! and answers, in a build that links the kit alone and no Symphony library. What a plugin's own
//! tests do.

use std::path::{Path, PathBuf};

use symphony_plugin::protocol::{
    CacheVersion, CardData, Chosen, PROTOCOL, PageData, Pictures, Recognises, Resolve, Slot,
    ThreadLineData, TriageCategory,
};
use symphony_plugin::testing::Host;
use symphony_plugin::vocabulary::{Block, Picture, Span};
use symphony_plugin::{Page, error};

#[path = "../../symphony/tests/project/seams/every.rs"]
mod every;

/// A root of this test's own, so what the plugin loaded for it is told apart from another test's.
fn root(name: &str) -> PathBuf {
    PathBuf::from(format!("/harness/{name}"))
}

/// The card with `text` and a thread of an agent's line and the owner's approval.
fn card(text: &str) -> CardData {
    CardData {
        slug: "pick-widget".into(),
        doc: "docs/DECISIONS.md".into(),
        topic: "Widgets".into(),
        text: text.into(),
        thread: vec![
            ThreadLineData {
                agent: true,
                verdict: None,
                text: "B. the round one".into(),
            },
            ThreadLineData {
                agent: false,
                verdict: Some("approved".into()),
                text: "B".into(),
            },
        ],
    }
}

/// The text of the heading a render of `every` answers first.
fn heading(blocks: &[Block]) -> &str {
    match blocks.first() {
        Some(Block::Heading { spans, .. }) => match spans.as_slice() {
            [Span::Text { text }] => text,
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_plugin_declares_what_its_process_answers_to_a_host_of_this_protocol() {
    let declared = Host::new(&every::PIPED).declaration().unwrap();
    assert_eq!(declared.protocol, PROTOCOL);
    assert_eq!(declared.name, "every");
    assert_eq!(declared.components, ["every", "again", "slow"]);
    assert_eq!(declared.card_kinds.len(), 1);
    assert_eq!(declared.card_kinds[0].name, "every");
    assert_eq!(declared.card_kinds[0].markers, ["Slot"]);
    assert_eq!(declared.renderers[0].extensions, ["every"]);
    assert_eq!(declared.routes, ["every"]);
    assert_eq!(declared.nav[0].href, "/x/every");
    assert_eq!(declared.home_regions, ["every"]);
    assert_eq!(declared.board_actions, ["every"]);
    assert_eq!(declared.watches, [["data/every", "pictures"]]);
    assert_eq!(declared.doc_checks[0].name, "every");
    assert_eq!(declared.commands[0].name, "every");
    assert_eq!(declared.commands[0].summary, "print its arguments");
    assert_eq!(declared.theme.unwrap().package, "every");
}

#[test]
fn a_component_renders_through_the_harness_with_its_pages_artifacts_folder() {
    let host = Host::new(&every::PIPED);
    let root = root("render");
    let page = PageData::of(&Page::new(&root, "docs/ünï code.md").in_artifacts("out/ä"));
    let blocks = host.render(&page, "every", "lantern", Some("wide"));
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert_eq!(heading(&blocks), "lantern Some(\"wide\") in out/ä/every");
    // A page of a host that named no folder has the default one.
    let plain = PageData::of(&Page::new(&root, "docs/A.md"));
    assert_eq!(
        heading(&host.render(&plain, "every", "a", None)),
        "a None in artifacts/every"
    );
    assert_eq!(
        host.ask(Resolve {
            component: "again".into(),
            page: page.clone(),
            argument: "a".into(),
            option: Some("o".into()),
        }),
        Ok(Err("again takes no option, not \"o\" on a".into()))
    );
    assert_eq!(
        host.ask(CacheVersion {
            component: "every".into(),
            page,
        }),
        Ok(Some(format!("v1:{}|docs/ünï code.md", root.display())))
    );
}

#[test]
fn a_components_problem_and_one_the_plugin_lacks_are_shown_as_a_host_shows_them() {
    let host = Host::new(&every::PIPED);
    let page = PageData::of(&Page::new(Path::new("/harness/problem"), "docs/A.md"));
    assert_eq!(
        host.render(&page, "every", "bad", None),
        [error("every refuses \"bad\" on docs/A.md")]
    );
    assert_eq!(
        host.render(&page, "no-such", "a", None),
        [error("the plugin has no component \"no-such\"")]
    );
}

#[test]
fn one_renders_components_share_what_they_load_and_another_render_loads_again() {
    let host = Host::new(&every::PIPED);
    let root = root("shared");
    let loads = || {
        every::EVERY
            .loads
            .lock()
            .unwrap()
            .iter()
            .filter(|loaded| **loaded == root)
            .count()
    };
    let page = PageData::of(&Page::new(&root, "docs/A.md"));
    host.render(&page, "every", "a", None);
    host.render(&page, "again", "b", None);
    assert_eq!(loads(), 1);
    let another = PageData::of(&Page::new(&root, "docs/A.md"));
    host.render(&another, "again", "c", None);
    assert_eq!(loads(), 2);
}

#[test]
fn a_card_kind_answers_through_the_harness() {
    let host = Host::new(&every::PIPED);
    let kind = || "every".to_string();
    let picked = card("Pick every widget.\n*Slot:* North · cover");
    assert_eq!(
        host.ask(Recognises {
            kind: kind(),
            card: picked.clone(),
        }),
        Ok(true)
    );
    assert_eq!(
        host.ask(Recognises {
            kind: kind(),
            card: card("another card"),
        }),
        Ok(false)
    );
    assert_eq!(
        host.ask(Slot {
            kind: kind(),
            card: picked.clone(),
        }),
        Ok(Some("North · cover".into()))
    );
    assert_eq!(
        host.ask(TriageCategory {
            kind: kind(),
            card: picked.clone(),
        }),
        Ok(Some("docs/DECISIONS.md · Widgets".into()))
    );
    assert_eq!(
        host.ask(Pictures {
            kind: kind(),
            root: root("cards"),
            card: picked.clone(),
        }),
        Ok(vec![
            Picture {
                src: Some("/harness/cards/pick-widget/0.png".into()),
                caption: "Widgets B. the round one".into(),
                waiting: true,
            },
            Picture {
                src: None,
                caption: "Widgets B".into(),
                waiting: false,
            },
        ])
    );
    assert_eq!(
        host.ask(Chosen {
            kind: kind(),
            card: picked,
        }),
        Ok(Ok(Some("B".into())))
    );
    assert_eq!(
        host.ask(Chosen {
            kind: kind(),
            card: card("a broken card"),
        }),
        Ok(Err("pick-widget names no candidate".into()))
    );
    // A kind the plugin lacks is refused by name.
    assert_eq!(
        host.ask(Recognises {
            kind: "no-such".into(),
            card: card("every"),
        }),
        Err("the plugin has no card kind \"no-such\"".into())
    );
}
