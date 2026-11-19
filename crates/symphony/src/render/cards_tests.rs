//! A card's compare row ([`super::compare_pictures`], [`super::compare_html`] through the
//! renderer): every picture named and captioned from its source, a fallback for one named in no
//! way, and the base and the letters a choice card is read by.

use super::{Picture, compare_pictures};
use crate::register::parse_decisions;
use crate::render::Render;
use crate::text::splitlines;

/// A pending card `probe` whose thread is `thread`, each line an entry.
fn card(thread: &[&str]) -> String {
    let mut text = String::from("- [ ] **probe** — Which one?\n");
    for line in thread {
        text.push_str("  [2026-11-18 18:38] ");
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// The card's compare row as the renderer writes it; empty when it writes none.
fn row(thread: &[&str]) -> String {
    let html = Render::new(".").html(&card(thread), "docs/DECISIONS.md");
    let Some(start) = html.find("<div class=\"compare\">") else {
        return String::new();
    };
    let end = html[start..].find("</div>").expect("the row's end") + start;
    html[start..end].to_string()
}

/// The (alt, caption) of each figure of `row`, in order.
fn named(row: &str) -> Vec<(String, String)> {
    row.split("<figure>")
        .skip(1)
        .map(|figure| {
            let between = |open: &str, close: &str| {
                let from = figure.find(open).expect("the opening") + open.len();
                let to = figure[from..].find(close).expect("the closing") + from;
                figure[from..to].to_string()
            };
            (
                between("alt=\"", "\""),
                between("<figcaption>", "</figcaption>"),
            )
        })
        .collect()
}

fn both(text: &str) -> (String, String) {
    (text.to_string(), text.to_string())
}

fn pictures(thread: &[&str]) -> (Vec<Picture>, Vec<Picture>) {
    let text = card(thread);
    let cards = parse_decisions(&splitlines(&text));
    compare_pictures(&cards[0])
}

#[test]
fn descriptive_alts_name_and_caption_every_picture_of_a_long_row() {
    let names = [
        "Desktop dark before",
        "Desktop dark after",
        "Desktop light before",
        "Desktop light after",
        "Phone dark before",
        "Phone dark after",
        "Phone light before",
        "Phone light after",
        "Docs light before",
        "Docs light after",
    ];
    let line = names
        .iter()
        .enumerate()
        .map(|(n, name)| format!("**{name}** ![{name}](evidence/{n}.png)"))
        .collect::<Vec<_>>()
        .join(" ");
    let got = named(&row(&[&format!("agent — {line}")]));
    assert_eq!(got, names.map(both));
}

#[test]
fn a_lettered_candidate_is_captioned_by_its_letter_and_named_by_its_whole_alt() {
    let got = named(&row(&[
        "agent — Base ![the current HUD](hud.png)",
        "agent — ![Flight HUD A](a.png) ![Flight HUD B](b.png)",
    ]));
    assert_eq!(
        got,
        [
            both("Base"),
            ("Flight HUD A".to_string(), "A".to_string()),
            ("Flight HUD B".to_string(), "B".to_string()),
        ]
    );
}

#[test]
fn a_picture_named_in_no_way_is_called_by_its_place_in_the_row() {
    let got = named(&row(&[
        "agent — Base ![](base.png)",
        "agent — ![](one.png) ![   ](two.png) ![Named one](three.png) ![](four.png)",
    ]));
    assert_eq!(
        got,
        [
            both("Base"),
            both("Picture 2"),
            both("Picture 3"),
            both("Named one"),
            both("Picture 5"),
        ]
    );
}

#[test]
fn a_name_is_escaped_in_its_attribute_and_its_caption() {
    let got = named(&row(&[
        "agent — ![Tom & \"Jerry\" <b>](a.png) ![After](b.png)",
    ]));
    assert_eq!(
        got[0],
        both("Tom &amp; &quot;Jerry&quot; &lt;b&gt;"),
        "{got:?}"
    );
}

#[test]
fn a_mockup_keeps_its_caption_and_its_name() {
    let got = row(&["agent — ![Board before](before.html) ![Board after](after.png)"]);
    assert!(got.contains("data-kind=\"mockup\""), "{got}");
    assert!(
        got.contains("<figcaption>Board before</figcaption>"),
        "{got}"
    );
    assert!(got.contains("alt=\"Board after\""), "{got}");
}

#[test]
fn one_picture_or_none_is_no_row() {
    assert_eq!(row(&["agent — ![Only](a.png)"]), "");
    assert_eq!(row(&["agent — no pictures here"]), "");
    assert_eq!(
        row(&["owner — ![Mine A](a.png) ![Mine B](b.png)"]),
        "",
        "an owner's pictures are no candidates"
    );
}

#[test]
fn the_row_is_the_first_base_and_the_first_agent_line_with_pictures() {
    let (base, candidates) = pictures(&[
        "agent — Base ![ current ](base.png)",
        "agent — Base ![later](later.png)",
        "agent — ![HUD A](a.png) ![plain](b.png) ![ HUD C ](c.png)",
        "agent — ![HUD D](d.png)",
    ]);
    let picture = |label: &str, name: &str, src: &str| Picture {
        label: label.into(),
        name: name.into(),
        src: src.into(),
    };
    assert_eq!(base, [picture("Base", "current", "base.png")]);
    assert_eq!(
        candidates,
        [
            picture("A", "HUD A", "a.png"),
            picture("", "plain", "b.png"),
            picture("C", "HUD C", "c.png"),
        ]
    );
}
