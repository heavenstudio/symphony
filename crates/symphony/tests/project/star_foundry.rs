//! Star Foundry's ship art through its plugin's seams: the home's progress region, a bar per
//! faction, and the pictures of the hulls a card names.

use symphony::plugins::Plugins;
use symphony_plugin::Card;
use symphony_plugin::vocabulary::{Picture, ProgressBar, Region};

use super::Tree;

const ROSTER: &str = "docs/data/ship-roster.v2.json";

/// Two listed factions and a hull of one the roster does not list.
fn roster(tree: &Tree) {
    tree.write(
        ROSTER,
        r#"{"factions":[{"id":"republic","name":"Republic"},{"id":"empire","name":"Empire"}],
            "hulls":[{"id":"REP-F01","name":"Kite","faction":"republic"},
                     {"id":"REP-F02","name":"Dart","faction":"republic"},
                     {"id":"REP-F03","name":"Lark","faction":"republic"},
                     {"id":"EMP-F01","name":"Spear","faction":"empire"},
                     {"id":"ZZ-F01","name":"Ghost","faction":"stray"}]}"#,
    );
}

/// The republic's ships page: Kite approved, Dart waiting on review, Lark not drawn.
fn republic(tree: &Tree, dart: char) {
    tree.write(
        "docs/ships/republic.md",
        &format!(
            "# Republic\n\n<!-- register:start -->\n### Ship art\n\n\
             - [x] **rep-f01-art** — Kite. *Shows:* REP-F01 Kite · ship\n  [2026-09-26 07:00] agent — ![k](/k.png)\n\
             \x20 [2026-09-26 07:30] owner · approved\n\
             - [{dart}] **rep-f02-art** — Dart. *Shows:* REP-F02 Dart · ship\n  [2026-09-26 07:00] agent — ![d](/d.png)\n\
             <!-- register:end -->\n"
        ),
    );
}

fn ship_art(tree: &Tree) -> Option<Region> {
    let plugins = Plugins::of(&tree.root);
    let regions = plugins.home_regions();
    let region = regions.iter().find(|region| region.id() == "ship-art")?;
    region.region(&tree.root)
}

fn bar(key: &str, name: &str, done: u32, waiting: u32, total: u32) -> ProgressBar {
    ProgressBar {
        key: key.into(),
        name: name.into(),
        page: format!("docs/ships/{key}.md"),
        title: format!(
            "{done} approved, {waiting} waiting on review, {} not drawn yet",
            total - done - waiting
        ),
        done,
        waiting,
        total,
    }
}

#[test]
fn the_ship_art_region_has_a_bar_per_faction_in_the_rosters_order() {
    let tree = Tree::star_foundry();
    assert_eq!(ship_art(&tree), None, "no roster, nothing to count");
    roster(&tree);
    republic(&tree, ' ');
    assert_eq!(
        ship_art(&tree),
        Some(Region {
            id: "ship-art".into(),
            title: "Ship art".into(),
            note: "1 of 5 hulls approved".into(),
            bars: vec![
                bar("republic", "Republic", 1, 1, 3),
                bar("empire", "Empire", 0, 0, 1),
                // A faction the roster does not list keeps its id as its name, after the rest.
                bar("stray", "stray", 0, 0, 1),
            ],
            legend: [
                "approved".into(),
                "waiting on you".into(),
                "not drawn".into()
            ],
        })
    );
    // The owner approves Dart: the next read follows the page.
    republic(&tree, 'x');
    let region = ship_art(&tree).unwrap();
    assert_eq!(region.note, "2 of 5 hulls approved");
    assert_eq!(region.bars[0], bar("republic", "Republic", 2, 0, 3));
    tree.write(ROSTER, "{unfinished");
    assert_eq!(
        ship_art(&tree),
        None,
        "a roster that does not read draws no region"
    );
    assert!(
        Plugins::of(&Tree::new().root).home_regions().is_empty(),
        "a plain project's home has no ship art"
    );
}

#[test]
fn a_card_naming_hulls_shows_their_pictures_once_each_six_at_most() {
    let tree = Tree::star_foundry();
    roster(&tree);
    republic(&tree, ' ');
    let plugins = Plugins::of(&tree.root);
    let kinds = plugins.card_kinds();
    let mentions = kinds
        .iter()
        .find(|kind| kind.name() == "hull mention")
        .expect("Star Foundry's hull mentions");
    let card = |text| Card {
        slug: "pick",
        doc: "docs/DECISIONS.md",
        topic: "Hulls",
        text,
        thread: &[],
    };
    assert!(!mentions.recognises(&card("No hull named here.")));
    let named = card("Compare [[ship:REP-F01]] with [[ships:rep-f02, REP-F01,REP-F03,NO-SUCH]].");
    assert!(mentions.recognises(&named));
    let picture = |src: Option<&str>, caption: &str, waiting| Picture {
        src: src.map(str::to_string),
        caption: caption.into(),
        waiting,
    };
    assert_eq!(
        mentions.pictures(&tree.root, &named),
        [
            picture(Some("/k.png"), "REP-F01 Kite", false),
            picture(Some("/d.png"), "REP-F02 Dart", true),
            picture(None, "REP-F03 Lark", false),
        ]
    );
    let many = card(
        "[[ships:REP-F01,REP-F02,REP-F03,EMP-F01,ZZ-F01]] and [[ship:REP-F01]] [[ships:EMP-F01]]",
    );
    assert_eq!(mentions.pictures(&tree.root, &many).len(), 5);
    tree.write(
        ROSTER,
        &format!(
            r#"{{"hulls":[{}]}}"#,
            (1..=8)
                .map(|n| format!(r#"{{"id":"H-{n}","name":"N{n}","faction":"f"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    let eight = card("[[ships:H-1,H-2,H-3,H-4,H-5,H-6,H-7,H-8]]");
    let shown = mentions.pictures(&tree.root, &eight);
    assert_eq!(
        shown.iter().map(|p| p.caption.as_str()).collect::<Vec<_>>(),
        ["H-1 N1", "H-2 N2", "H-3 N3", "H-4 N4", "H-5 N5", "H-6 N6"]
    );
}
