//! [`super::pictures`], [`super::definitions`] and [`super::local_route`]: every form a picture
//! takes, and the server's own URLs.

use super::{Definitions, Picture, definitions, local_route, pictures};

fn read(text: &str) -> Vec<Picture> {
    pictures(text, &definitions(text))
}

fn at(line: usize, target: &str) -> Picture {
    Picture {
        line,
        target: target.into(),
        defined: None,
    }
}

fn targets(text: &str) -> Vec<String> {
    read(text).into_iter().map(|p| p.target).collect()
}

/// An inline picture, bare, with a title in each quoting, in angle brackets, several on a line.
#[test]
fn an_inline_picture_is_read_with_or_without_a_title_or_angle_brackets() {
    let text = "![a](one.png)\n\
        ![b](two.png \"a title\") and ![c](three.png 'single') and ![d](four.png (paren))\n\
        ![e](<with space.png>) ![f]( padded.png )\n";
    assert_eq!(
        read(text),
        [
            at(0, "one.png"),
            at(1, "two.png"),
            at(1, "three.png"),
            at(1, "four.png"),
            at(2, "with space.png"),
            at(2, "padded.png"),
        ]
    );
}

/// A reference takes its path from its definition, wherever that stands and however its label is
/// cased or spaced: full, collapsed (`[]`) and shortcut forms. One with no definition, and a plain
/// link, show nothing; a definition is no picture itself.
#[test]
fn a_reference_picture_takes_the_path_its_definition_names() {
    let text = "Full ![A][Lamp  Shot], collapsed ![lamp shot][], shortcut ![lamp shot].\n\
        ![none][missing] and [a link](x.png) and [lamp shot]\n\
        \n\
        [lamp shot]: /artifacts/lamp.png \"title\"\n\
        [other]: <docs/other one.png>\n\
        ![x][other]\n";
    let lamp = Picture {
        line: 0,
        target: "/artifacts/lamp.png".into(),
        defined: Some(3),
    };
    assert_eq!(
        read(text),
        [
            lamp.clone(),
            lamp.clone(),
            lamp,
            Picture {
                line: 5,
                target: "docs/other one.png".into(),
                defined: Some(4),
            },
        ]
    );
}

/// The first definition of a label wins; a definition read from another text (the document a
/// card's lines sit in) serves a picture of this one.
#[test]
fn the_first_definition_wins_and_definitions_can_come_from_the_document() {
    let doc = "[s]: first.png\n[S]: second.png\n";
    let found: Definitions = definitions(doc);
    assert_eq!(found["s"], ("first.png".to_string(), 0));
    assert_eq!(
        pictures("  [10:00] agent — ![shot][s]", &found),
        [Picture {
            line: 0,
            target: "first.png".into(),
            defined: Some(0),
        }]
    );
}

/// Raw HTML pictures, however their `src` is quoted and wherever it stands among the attributes;
/// in order with the Markdown ones on the line.
#[test]
fn a_raw_img_shows_its_src_however_it_is_quoted() {
    let text = "<img src=\"a.png\"> then ![b](b.png) then <IMG alt=\"x\" SRC='c.png'/> <img width=3 src=d.png>\n\
        <image src=\"no.png\"> <img data-src=\"no.png\">\n";
    assert_eq!(targets(text), ["a.png", "b.png", "c.png", "d.png"]);
}

/// Code spans and fenced blocks show no picture, of any form; the line after a fence counts again.
#[test]
fn code_shows_no_picture() {
    let text = "`![a](a.png)` and `<img src=\"b.png\">`\n\
        ```\n\
        ![c](c.png)\n\
        [d]: d.png\n\
        ```\n\
        ![d] ![e](e.png)\n";
    assert_eq!(read(text), [at(5, "e.png")]);
}

/// This server's own URL names the route after it: under `/<id>/` the rest whatever it is; at the
/// root (a server of the project's own) only a route `serves` admits. Another host, or not a URL,
/// is none.
#[test]
fn the_servers_own_url_names_its_route() {
    let serves = |route: &str| route.starts_with("artifacts/") || route.starts_with("docs/");
    let route = |target: &str| local_route(target, "symphony", serves);
    assert_eq!(
        route("http://127.0.0.1:8765/symphony/artifacts/t3.png"),
        Some(Some("/artifacts/t3.png".into()))
    );
    assert_eq!(
        route("http://localhost:1/symphony/docs/a.png?x=1#y"),
        Some(Some("/docs/a.png".into()))
    );
    assert_eq!(route("https://[::1]/symphony"), Some(Some("/".into())));
    assert_eq!(
        route("http://127.0.0.1:62263/artifacts/t3.png"),
        Some(Some("/artifacts/t3.png".into()))
    );
    // Another project's, a page of the Symphony's own, a project whose id only begins the same.
    assert_eq!(route("http://127.0.0.1:8765/consumer/x.png"), Some(None));
    assert_eq!(route("http://127.0.0.1:8765/tasks"), Some(None));
    assert_eq!(route("http://127.0.0.1:8765/symphonyx/a.png"), Some(None));
    assert_eq!(route("http://example.com/symphony/a.png"), None);
    assert_eq!(route("/artifacts/a.png"), None);
}

#[test]
fn unicode_prefixes_keep_all_mixed_picture_fields_in_byte_offset_order() {
    let text = "é猫 ![one](a.png) <img src='b.png'> ![ref][r] ![r][]\n[r]: c.png\n";
    assert_eq!(
        read(text),
        [
            at(0, "a.png"),
            at(0, "b.png"),
            Picture {
                line: 0,
                target: "c.png".into(),
                defined: Some(1)
            },
            Picture {
                line: 0,
                target: "c.png".into(),
                defined: Some(1)
            },
        ]
    );
    assert!(read("").is_empty());
    assert_eq!(read("![solo](one.png)"), [at(0, "one.png")]);
}

#[test]
fn picture_offset_ties_preserve_complete_payloads_and_report_cost() {
    for count in [0, 1, 32, 512] {
        let mut rows: Vec<_> = (0..count)
            .map(|i| {
                (
                    i % 7,
                    Picture {
                        line: i,
                        target: format!("猫-{i}.png"),
                        defined: (i % 2 == 0).then_some(i + 1),
                    },
                )
            })
            .collect();
        let mut old = rows.clone();
        #[expect(
            clippy::disallowed_methods,
            reason = "the standard sort this test holds the shared sorter to"
        )]
        let (_, old_cpu) = test_time::cpu::spent(|| old.sort_by_key(|(start, _)| *start));
        let (_, new_cpu) = test_time::cpu::spent(|| {
            symphony_board::order::sort_by_key(&mut rows, &|(start, _)| *start)
        });
        assert_eq!(rows, old);
        eprintln!(
            "picture offsets n={count}: old_cpu={old_cpu:?},shared_cpu={new_cpu:?},positions={}bytes plus usize-sort scratch",
            count * std::mem::size_of::<usize>()
        );
    }
}
