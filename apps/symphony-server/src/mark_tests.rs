//! The Symphony's mark: its icon as a data address and as the address's own answer, the links a
//! page carries, and what a header opens with.

use super::*;

async fn bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

fn kind(response: &Response) -> &str {
    response.headers()[header::CONTENT_TYPE].to_str().unwrap()
}

#[test]
fn a_data_address_holds_nothing_that_ends_its_attribute_or_its_url() {
    let svg = "<svg a=\"1\">\n<style>.x{fill:#fff;width:50%}</style>\r\n</svg>\n";
    assert_eq!(
        address(svg),
        "data:image/svg+xml,%3Csvg a='1'%3E%3Cstyle%3E.x{fill:%23fff;width:50%25}%3C/style%3E%3C/svg%3E"
    );
    assert_eq!(address(""), "data:image/svg+xml,");
}

/// The icon a page carries is the file, whole: both schemes' colours and the mark's two shapes.
#[test]
fn the_icon_inline_is_the_file_with_its_light_and_dark_colours() {
    let inline = ADDRESS.as_str();
    for letter in ['"', '<', '>', '#', '\n'] {
        assert!(!inline.contains(letter), "{letter:?} in {inline}");
    }
    let read = inline
        .trim_start_matches("data:image/svg+xml,")
        .replace("%3C", "<")
        .replace("%3E", ">")
        .replace("%23", "#")
        .replace("%25", "%");
    assert_eq!(read, ICON.replace('\n', "").replace('"', "'"));
    for part in [
        "prefers-color-scheme:dark",
        ".tile{fill:#2f5d9e}",
        ".tile{fill:#7fa7e0}",
        "<path class='ink' d='M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z'/>",
        "<circle class='ink' cx='16' cy='19.6' r='2.9'/>",
    ] {
        assert!(read.contains(part), "{part}: {read}");
    }
}

#[test]
fn a_page_links_the_icon_inline_and_the_home_screens_under_its_base() {
    for (base, touch) in [
        ("", "/favicon.ico?size=180"),
        ("/alpha", "/alpha/favicon.ico?size=180"),
        ("/a\"b", "/a&quot;b/favicon.ico?size=180"),
    ] {
        assert_eq!(
            links(base),
            format!(
                "<link rel=\"icon\" href=\"{}\"><link rel=\"apple-touch-icon\" href=\"{touch}\">",
                *ADDRESS
            )
        );
    }
}

#[test]
fn a_header_opens_with_the_mark_and_the_wordmark_or_a_themes_own_in_their_place() {
    let mark = "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"app-mark\" viewBox=\"4.5 9.5 23 \
                14.4\" aria-hidden=\"true\"><path d=\"M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 \
                7.4 21Z\"/><circle cx=\"16\" cy=\"19.6\" r=\"2.9\"/></svg>";
    assert_eq!(
        brand("", ""),
        format!(
            "<span class=\"app-brand\">{mark}<span \
             class=\"app-wordmark\">Symphony</span></span>"
        )
    );
    // A name alone keeps the mark; an icon and a name leave nothing of the Symphony's.
    assert_eq!(
        brand("Lantern <Works>", ""),
        format!(
            "<span class=\"app-brand\">{mark}<span class=\"app-name\">Lantern \
             &lt;Works&gt;</span></span>"
        )
    );
    assert_eq!(
        brand("Lantern", "/lantern/look/a\"b.svg"),
        "<span class=\"app-brand\"><img class=\"app-mark\" \
         src=\"/lantern/look/a&quot;b.svg\" alt=\"\"><span class=\"app-name\">Lantern</span></span>"
    );
}

/// The React frame draws the mark from its own markup: the same two shapes in the same box.
#[test]
fn the_react_frame_draws_the_same_mark() {
    let frame = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../symphony-web/src/frame.tsx"),
    )
    .unwrap();
    for part in [
        "className=\"app-mark\" viewBox=\"4.5 9.5 23 14.4\" aria-hidden=\"true\"",
        "<path d=\"M5 21A11 11 0 0 1 27 21H24.6A8.6 7.2 0 0 0 7.4 21Z\" />",
        "<circle cx=\"16\" cy=\"19.6\" r=\"2.9\" />",
    ] {
        assert!(frame.contains(part), "frame.tsx lacks {part}");
        let served = part.replace("className", "class").replace(" />", "/>");
        assert!(MARK.contains(&served), "mark.svg lacks {served}");
    }
}

#[tokio::test]
async fn the_addresss_own_icon_is_the_svg_and_the_home_screens_a_png_180_wide() {
    for query in [None, Some(""), Some("size=32"), Some("v=1&size=180")] {
        let answer = answer(query);
        assert_eq!(kind(&answer), "image/svg+xml", "{query:?}");
        assert_eq!(
            answer.headers()[header::CACHE_CONTROL],
            "public, max-age=86400"
        );
        assert_eq!(bytes(answer).await, ICON.as_bytes(), "{query:?}");
    }
    let touch = answer(Some("size=180"));
    assert_eq!(kind(&touch), "image/png");
    assert_eq!(touch.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let png = bytes(touch).await;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // The header chunk: its width and height, then eight bits a channel with no alpha (colour
    // type 2), as a phone fills what is transparent with black.
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(&png[16..24], [0, 0, 0, 180, 0, 0, 0, 180]);
    assert_eq!(png[25], 2, "the colour type");
}
