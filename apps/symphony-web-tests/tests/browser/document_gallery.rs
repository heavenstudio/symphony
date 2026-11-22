//! A card's compare row on a document page, read in the release client: ten named pictures of
//! both shapes wrap into rows wide enough to tell one from the next on a phone and a desk, each
//! a control named and captioned by its source, a long commit hash beside them wraps inside the
//! card, and a picture opened by the keyboard or a touch shows whole under its name, offers
//! itself at full size, and gives the focus back to the picture it was opened on.
use super::components::key;
use super::{SHOWN, chrome, chrome::Chrome, common, dist, focus_on, site::Site};
use serde_json::{Value, json};

const DOC: &str = "docs/GALLERY.md";
const HASH: &str = "36f1f088d1b9042f3f8c64d9f3b9133a98580c3d";
const NAMES: [&str; 10] = [
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
const ROW: &str = "#evidence .compare";

fn src(n: usize) -> String {
    format!("/docs/evidence/{n}.svg")
}

/// A figure of a compare row as the server writes it (`render/cards.rs`): its picture named
/// `alt`, under `caption`.
fn figure(n: usize, alt: &str, caption: &str) -> Value {
    json!({"type":"container","tag":"figure","presentation":{},"blocks":[
        {"type":"image","image":{"src":src(n),"alt":alt,"presentation":{"attributes":{"loading":"lazy"}}}},
        {"type":"caption","blocks":[{"type":"paragraph","spans":[{"type":"text","text":caption}]}]},
    ]})
}

/// A pending card `slug` asking about a commit by its whole hash, over `figures`.
fn card(slug: &str, start: usize, figures: Vec<Value>) -> (Value, Value) {
    let block = json!({"type":"card","slug":slug,"mark":"pending","title":slug,
        "body":[{"type":"paragraph","spans":[
            {"type":"text","text":"Accept the implementation at commit "},
            {"type":"code","text":HASH},
            {"type":"text","text":" for delivery?"},
        ]}],
        "verdict":{"group":"decide","open":true,"posted":null,"decided":null},
        "thread":[],"line":start,"line_offset":null,
        "comparison":[{"type":"comparison","figures":figures}]});
    (
        json!({"start":start,"end":start + 1,"key":format!("card:{slug}"),"hash":slug,"part":1,"blocks":[block]}),
        json!({"slug":slug,"state":"pending","text":slug,"group":"decide","open":true}),
    )
}

/// The document `docs/GALLERY.md`: the card `evidence` over ten named pictures, four of them a
/// phone's shape, and the card `choice` over a base and two lettered candidates.
fn gallery_site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    for n in 0..NAMES.len() {
        let (width, height) = if (4..8).contains(&n) {
            (390, 844)
        } else {
            (1440, 900)
        };
        site.set_file(
            &src(n),
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="teal"/></svg>"#).into_bytes(),
            "image/svg+xml",
        );
    }
    let (evidence, first) = card(
        "evidence",
        3,
        NAMES
            .iter()
            .enumerate()
            .map(|(n, name)| figure(n, name, name))
            .collect(),
    );
    let (choice, second) = card(
        "choice",
        6,
        vec![
            figure(0, "Base", "Base"),
            figure(1, "Flight HUD A", "A"),
            figure(2, "Flight HUD B", "B"),
        ],
    );
    let sections = json!([
        {"start":1,"end":1,"key":"open","hash":"open","part":1,"blocks":[{"type":"heading","level":2,"anchor":"open","spans":[{"type":"text","text":"Open"}]}]},
        evidence,
        choice,
    ]);
    site.set_json("/api/document", &json!({"document":{"version":7,"path":DOC,"title":"Gallery","waiting":[],"crumbs":[],
        "headings":[{"level":2,"anchor":"open","text":"Open"}],"sections":sections,"cards":[first,second],"threads":{},"tasks":[]},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":7,"change":{"path":DOC,"sections":sections,"threads":{}}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    site
}

fn viewport(page: &mut Chrome, width: u32, height: u32, touch: bool) {
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":touch,"maxTouchPoints":5}),
    );
    page.viewport(width, height, width < 600);
}

/// Opens the document and waits until every picture of both rows is a named control.
fn open(page: &mut Chrome, site: &Site) {
    page.open(&site.url(&format!("/{DOC}")));
    assert!(
        page.wait_for(
            "document.querySelectorAll('.decision .compare img[role=button][aria-label]').length === 13",
            SHOWN
        ),
        "both rows are drawn, each picture a control: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 400)")
    );
}

/// The pictures of the row `row`: how many stand on its first line, the narrowest's width, and
/// whether any reaches past the row's own box.
fn laid_out(page: &mut Chrome, row: &str) -> Value {
    page.eval(&format!(
        "(() => {{ const row = document.querySelector({}); const box = row.getBoundingClientRect(); \
         const boxes = [...row.querySelectorAll('img')].map(image => image.getBoundingClientRect()); \
         return {{ first: boxes.filter(one => Math.abs(one.top - boxes[0].top) < 1).length, \
         narrowest: Math.floor(Math.min(...boxes.map(one => one.width))), \
         inside: boxes.every(one => one.left >= box.left - 1 && one.right <= box.right + 1) }}; }})()",
        json!(row)
    ))
}

#[test]
fn document_gallery_wraps_ten_named_pictures_wide_enough_to_tell_apart() {
    let site = gallery_site();
    let Some(mut page) = chrome() else { return };
    // (window, pictures on the long row's first line, the least width one may have, pictures on
    // the choice's first line)
    for (width, height, per_row, least, choice) in [
        (320, 800, 1, 200, 1),
        (390, 844, 2, 130, 2),
        (1440, 900, 4, 280, 3),
    ] {
        viewport(&mut page, width, height, false);
        open(&mut page, &site);
        let long = laid_out(&mut page, ROW);
        assert_eq!(
            [&long["first"], &long["inside"]],
            [&json!(per_row), &json!(true)],
            "at {width}px the ten pictures stand {per_row} to a row inside their card: {long}"
        );
        assert!(
            long["narrowest"].as_u64().unwrap() >= least,
            "at {width}px every picture is at least {least}px wide: {long}"
        );
        let short = laid_out(&mut page, "#choice .compare");
        assert_eq!(
            [&short["first"], &short["inside"]],
            [&json!(choice), &json!(true)],
            "at {width}px the choice's three pictures stand {choice} to a row: {short}"
        );
        assert_eq!(
            page.eval(&format!(
                "[...document.querySelectorAll({})].map(caption => caption.checkVisibility() ? caption.textContent : null)",
                json!(format!("{ROW} figcaption"))
            )),
            json!(NAMES),
            "at {width}px each picture's caption says its source's name"
        );
        assert_eq!(
            page.eval(&format!(
                "[...document.querySelectorAll({})].map(image => [image.getAttribute('aria-label'), image.tabIndex])",
                json!(format!("{ROW} img"))
            )),
            json!(NAMES.map(|name| json!([format!("Open picture {name}"), 0]))),
            "at {width}px each picture is a control the keyboard reaches, named apart from the rest"
        );
        assert_eq!(
            page.eval("[...document.querySelectorAll('#choice .compare img')].map(image => image.getAttribute('aria-label'))"),
            json!(["Open picture Base", "Open picture Flight HUD A", "Open picture Flight HUD B"]),
        );
        // The hash is read whole inside the card's text: no box of it past the text's edge, and
        // the page no wider than its window.
        let hash = page.eval(
            "(() => { const code = [...document.querySelectorAll('#evidence .decision-text code')].at(-1); \
             const text = code.closest('.decision-text').getBoundingClientRect(); \
             return { text: code.textContent, inside: [...code.getClientRects()].every(box => box.left >= text.left - 1 && box.right <= text.right + 1), \
             page: document.documentElement.scrollWidth <= document.documentElement.clientWidth }; })()",
        );
        assert_eq!(
            hash,
            json!({"text":HASH,"inside":true,"page":true}),
            "at {width}px the commit's hash wraps inside its card"
        );
    }
}

/// The open viewer's picture, caption and full-size link, or null while it is closed.
const VIEWER: &str = "(() => { const box = document.querySelector('#media-lightbox[data-state=open]'); \
    return box && { src: box.querySelector('img').getAttribute('src'), alt: box.querySelector('img').alt, \
    caption: box.querySelector('.lightbox-caption').textContent, \
    full: [box.querySelector('a.lightbox-full').getAttribute('href'), box.querySelector('a.lightbox-full').target] }; })()";
const CLOSED: &str = "document.getElementById('media-lightbox') === null";

fn shown(n: usize) -> Value {
    json!({"src":src(n),"alt":NAMES[n],"caption":format!("{} · {} / 10", NAMES[n], n + 1),
        "full":[src(n),"_blank"]})
}

fn nth(n: usize) -> String {
    format!("{ROW} figure:nth-child({}) img", n + 1)
}

/// Waits until the picture `n` has its own address, scrolled to where a reader would press it.
fn reach(page: &mut Chrome, n: usize) {
    page.eval(&format!(
        "document.querySelector({}).scrollIntoView({{block: 'center'}})",
        json!(nth(n))
    ));
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}).getAttribute('src') === {}",
                json!(nth(n)),
                json!(src(n))
            ),
            SHOWN
        ),
        "the picture {n} is loaded"
    );
}

#[test]
fn document_gallery_picture_opens_by_keyboard_and_gives_the_focus_back() {
    let site = gallery_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1440, 900, false);
    open(&mut page, &site);
    reach(&mut page, 5);
    page.eval(&format!(
        "document.querySelector({}).focus()",
        json!(nth(5))
    ));
    key(&mut page, "Enter", "Enter", 13);
    assert!(
        page.wait_for(&format!("{VIEWER} !== null"), SHOWN),
        "Enter opens it"
    );
    assert_eq!(
        page.eval(VIEWER),
        shown(5),
        "the picture chosen, by its name"
    );
    // The picture fits the window, whole.
    assert_eq!(
        page.eval("(() => { const box = document.querySelector('#media-lightbox img').getBoundingClientRect(); return box.width > 0 && box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight; })()"),
        json!(true)
    );
    key(&mut page, "ArrowRight", "ArrowRight", 39);
    assert!(
        page.wait_for(
            &format!(
                "{VIEWER}?.caption === {}",
                json!(format!("{} · 7 / 10", NAMES[6]))
            ),
            SHOWN
        ),
        "the right arrow moves to the next named picture: {}",
        page.eval(VIEWER)
    );
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for(CLOSED, SHOWN), "Escape closes it");
    assert_eq!(
        focus_on(&mut page, &nth(5)),
        json!(true),
        "the focus is back on the picture it was opened on"
    );
}

#[test]
fn document_gallery_picture_opens_by_touch_and_gives_the_focus_back() {
    let site = gallery_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 390, 844, true);
    open(&mut page, &site);
    reach(&mut page, 2);
    page.tap(&nth(2));
    assert!(
        page.wait_for(&format!("{VIEWER} !== null"), SHOWN),
        "a tap opens it"
    );
    assert_eq!(
        page.eval(VIEWER),
        shown(2),
        "the picture tapped, by its name"
    );
    // Every control of the viewer is on the phone's screen, large enough for a finger.
    assert_eq!(
        page.eval("[...document.querySelectorAll('#media-lightbox .lightbox-controls > *')].map(control => { const box = control.getBoundingClientRect(); return [control.textContent, box.width >= 44 && box.height >= 44 && box.left >= 0 && box.right <= innerWidth]; })"),
        json!([["←", true], ["Close", true], ["Full size", true], ["→", true]])
    );
    page.tap("#media-lightbox .lightbox-controls button:nth-of-type(2)");
    assert!(page.wait_for(CLOSED, SHOWN), "Close closes it");
    assert_eq!(
        focus_on(&mut page, &nth(2)),
        json!(true),
        "the focus is back on the picture it was opened on"
    );
}
