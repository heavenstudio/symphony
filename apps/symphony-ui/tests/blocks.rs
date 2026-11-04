//! The generic blocks a plugin renders to, drawn on the host by dioxus-ssr: one component each,
//! whatever plugin made them.

use dioxus::prelude::*;
use symphony_api::{Block, Cell, FigureSize, Image, Presentation, Span, VectorKind, VectorShape};
use symphony_ui::block_view::BlocksView;

fn drawn(blocks: Vec<Block>) -> String {
    dioxus_ssr::render_element(rsx! { BlocksView { blocks } })
}

fn text(text: &str) -> Span {
    Span::Text { text: text.into() }
}

fn kite() -> Image {
    Image {
        src: "/k.png".into(),
        alt: "Kite".into(),
        presentation: Presentation::default(),
    }
}

fn figure(
    picture: Option<Image>,
    href: Option<&str>,
    badge: Option<&str>,
    size: FigureSize,
) -> Block {
    Block::Figure {
        picture,
        caption: vec![text("NOR-L01 Kite")],
        href: href.map(str::to_string),
        badge: badge.map(str::to_string),
        size,
    }
}

#[test]
fn a_lamp_sized_figure_is_a_link_card_its_badge_standing_in_for_a_missing_picture() {
    for size in [
        FigureSize::Small,
        FigureSize::Row,
        FigureSize::Large,
        FigureSize::Tile,
    ] {
        let class = format!("figure figure-{}", size.as_str());
        let html = drawn(vec![figure(
            Some(kite()),
            Some("/docs/lamps/north.md#nor-l01"),
            None,
            size,
        )]);
        assert!(
            html.contains(&format!(
                "<a class=\"{class}\" href=\"/docs/lamps/north.md#nor-l01\">"
            )),
            "{html}"
        );
        assert!(html.contains("<img src=\"/k.png\" alt=\"Kite\""), "{html}");
        assert!(
            html.contains("<span class=\"figure-caption\">NOR-L01 Kite</span>"),
            "{html}"
        );
        assert!(!html.contains("figure-badge"), "{html}");
        let html = drawn(vec![figure(None, None, Some("coming"), size)]);
        assert!(
            html.contains(&format!("<span class=\"{class}\">")),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"figure-badge\">coming</span>"),
            "{html}"
        );
        assert!(!html.contains("<img") && !html.contains("<a "), "{html}");
    }
}

#[test]
fn any_other_figure_is_a_figure_marked_missing_without_its_picture() {
    let html = drawn(vec![figure(
        Some(kite()),
        Some("/screens/title.png"),
        None,
        FigureSize::Wide,
    )]);
    assert!(
        html.starts_with("<figure class=\"figure figure-wide\">"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"/screens/title.png\"><img src=\"/k.png\""),
        "{html}"
    );
    assert!(
        html.contains("<figcaption>NOR-L01 Kite</figcaption>"),
        "{html}"
    );
    let html = drawn(vec![figure(Some(kite()), None, None, FigureSize::Slot)]);
    assert!(
        html.starts_with("<figure class=\"figure figure-slot\"><img"),
        "{html}"
    );
    let html = drawn(vec![figure(
        None,
        None,
        Some("not captured"),
        FigureSize::Cell,
    )]);
    assert!(
        html.starts_with("<figure class=\"figure figure-cell missing\">"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"figure-badge\">not captured</span>"),
        "{html}"
    );
    let html = drawn(vec![Block::Figure {
        picture: None,
        caption: Vec::new(),
        href: None,
        badge: None,
        size: FigureSize::Wide,
    }]);
    assert_eq!(
        html,
        "<figure class=\"figure figure-wide missing\"></figure>"
    );
}

#[test]
fn an_icon_is_its_vector_named_by_its_label_and_tipped_on_hover() {
    let path = VectorShape {
        kind: VectorKind::Path,
        attributes: [("d".to_string(), "M4 4L20 20".to_string())].into(),
    };
    let icon = |named: bool, tip: &str| {
        drawn(vec![Block::Paragraph {
            spans: vec![Span::Icon {
                shape: vec![path.clone()],
                label: "Cloning bay".into(),
                named,
                tip: tip.into(),
            }],
        }])
    };
    let html = icon(true, "Cloning bay: brings a lost crew back");
    for part in [
        "class=\"icon\"",
        "data-tip=\"Cloning bay: brings a lost crew back\"",
        "title=\"Cloning bay: brings a lost crew back\"",
        "tabindex=\"0\"",
        "role=\"img\"",
        "aria-label=\"Cloning bay\"",
        "viewBox=\"0 0 24 24\"",
        "stroke=\"currentColor\"",
        "aria-hidden=\"true\"",
        "<path d=\"M4 4L20 20\"",
        "<span class=\"icon-name\">Cloning bay</span>",
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
    let html = icon(false, "");
    assert!(html.contains("aria-label=\"Cloning bay\""), "{html}");
    assert!(
        !html.contains("data-tip") && !html.contains("title=") && !html.contains("icon-name"),
        "{html}"
    );
}

#[test]
fn a_component_keeps_its_kind_for_the_stylesheet_and_stays_in_its_line() {
    let html = drawn(vec![Block::Component {
        kind: "lamp-matrix".into(),
        blocks: vec![Block::Paragraph {
            spans: vec![text("inside")],
        }],
    }]);
    assert_eq!(
        html,
        "<div data-component=\"lamp-matrix\"><p>inside</p></div>"
    );
    let html = drawn(vec![Block::Paragraph {
        spans: vec![
            text("see "),
            Span::Component {
                kind: "task".into(),
                blocks: vec![Block::Paragraph {
                    spans: vec![text("sim/jump")],
                }],
            },
            text(" now"),
        ],
    }]);
    assert_eq!(
        html,
        "<p>see <span data-component=\"task\">sim/jump</span> now</p>"
    );
    // An inline component of several blocks keeps them, still inside its line's wrapper.
    let html = drawn(vec![Block::Paragraph {
        spans: vec![Span::Component {
            kind: "lamps".into(),
            blocks: vec![
                Block::Paragraph {
                    spans: vec![text("a")],
                },
                Block::Paragraph {
                    spans: vec![text("b")],
                },
            ],
        }],
    }]);
    assert_eq!(
        html,
        "<p><span data-component=\"lamps\"><p>a</p><p>b</p></span></p>"
    );
}

#[test]
fn a_components_table_names_its_look_and_holds_figures_in_its_cells() {
    let cell = |spans: Vec<Span>| Cell {
        header: false,
        spans,
        presentation: Presentation::default(),
    };
    let html = drawn(vec![Block::Table {
        head: vec![Cell {
            header: true,
            spans: vec![text("Lamp")],
            presentation: Presentation::default(),
        }],
        rows: vec![vec![cell(vec![Span::Blocks {
            blocks: vec![figure(Some(kite()), None, None, FigureSize::Cell)],
        }])]],
        presentation: Presentation::class("matrix shape-matrix"),
    }]);
    assert!(
        html.starts_with(
            "<table class=\"matrix shape-matrix\"><thead><tr><th>Lamp</th></tr></thead>"
        ),
        "{html}"
    );
    assert!(
        html.contains("<td><figure class=\"figure figure-cell\"><img src=\"/k.png\""),
        "{html}"
    );
    let plain = drawn(vec![Block::Table {
        head: Vec::new(),
        rows: vec![vec![cell(vec![text("x")])]],
        presentation: Presentation::default(),
    }]);
    assert_eq!(plain, "<table><tbody><tr><td>x</td></tr></tbody></table>");
}
