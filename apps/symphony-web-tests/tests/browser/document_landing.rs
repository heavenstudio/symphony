//! A link to a card on a document page lands with the card's first line just below the header, at
//! the page's scroll padding, and the card stays there while the page draws around it: the scroll
//! to it brings the sections above it near, and they draw after it, taller than the ghosts they
//! replace (a picture in each, and in each card's history). On a first visit and on a move in place
//! from another document, on a desk and a phone. The landing is held by the shell
//! (`land`, shell/shell.ts); here the sections' reads are held until the page has landed, so
//! they draw after it on any machine, and the device keeps no copy of them ([`NO_STORE`]): the
//! shell's read-ahead of the other document's links would otherwise put the card's sections on it
//! before the reads are held. A reader who scrolls away during the landing, by a wheel or a
//! script, is left there when the reads land; one who clicks, taps or presses a key that scrolls
//! nothing keeps the card held; and once the landing has gone quiet, nothing lands it again, not a
//! change to the document above the card after a script's scroll. Once the reader's own scroll
//! ended the landing, the browser's scroll anchoring keeps what they read in place while the
//! sections above still draw, a field typed in on a phone among it, whatever they read and
//! whatever draws beside it: a paragraph, a list or a table under code, a quote, a mockup or a
//! picture, a card beside those or over an h3 or a table, the intro under the title.

use serde_json::{Map, Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{NO_STORE, SHOWN, STALL, common, dist};

const LANDING: &str = "docs/landing.md";
const FROM: &str = "docs/from.md";
const PICTURE: &str = "/docs/tall.svg";
/// The cards waiting on the owner; a link lands on the fifth, with cards and parts above and below.
const CARDS: [&str; 7] = ["one", "two", "three", "four", "five", "six", "seven"];
/// The link to the fifth card, as the client draws it.
const LINK: &str = "/docs/landing.md#five";

fn text(words: &str) -> Value {
    json!({"type": "text", "text": words})
}

fn paragraph(spans: Vec<Value>) -> Value {
    json!({"type": "paragraph", "spans": spans})
}

/// A picture taller than any section's ghost.
fn picture() -> Value {
    json!({"type": "image", "image": {"src": PICTURE, "alt": "A tall picture",
        "presentation": {"attributes": {"width": "600", "height": "700"}}}})
}

/// The part `n` from line `start`: a heading, a paragraph and a picture.
fn part(n: usize, start: usize) -> Value {
    let anchor = format!("part-{n}");
    json!({"start": start, "end": start + 6, "key": anchor, "hash": anchor, "part": 0, "blocks": [
        {"type": "heading", "level": 2, "anchor": anchor, "spans": [text(&format!("Part {n}"))]},
        paragraph(vec![text("Words above a picture taller than its section's ghost.")]),
        paragraph(vec![picture()]),
    ]})
}

/// The card `slug` waiting on the owner, from line `start`.
fn card(slug: &str, start: usize) -> Value {
    json!({"start": start, "end": start + 6, "key": format!("card:{slug}"), "hash": slug, "part": 1, "blocks": [{
        "type": "card", "slug": slug, "mark": "pending", "title": slug,
        "body": [paragraph(vec![text(&format!("Card {slug}: keep every lamp, or merge the near-duplicates?"))])],
        "verdict": {"group": "decide", "open": true, "posted": null, "decided": null},
        "thread": [], "line": start, "line_offset": null, "comparison": []}]})
}

/// The document the link lands in, as the server reads it whole: six parts, the seven cards (each
/// with a picture on its thread), and six parts more, so the fifth card can be scrolled to the top
/// of any window. Its anchors are the ids each section holds, as the server's outline names them.
fn landing() -> Value {
    let (before, after) = (1..=6, 7..=12);
    let mut sections: Vec<Value> = before.clone().map(|n| part(n, n * 8)).collect();
    sections.extend(
        CARDS
            .iter()
            .enumerate()
            .map(|(n, slug)| card(slug, 100 + n * 8)),
    );
    sections.extend(after.clone().map(|n| part(n, 200 + n * 8)));
    let mut anchors = Map::new();
    for n in before.clone().chain(after.clone()) {
        anchors.insert(format!("part-{n}"), json!([format!("part-{n}")]));
    }
    let mut threads = Map::new();
    for slug in CARDS {
        anchors.insert(format!("card:{slug}"), json!([slug]));
        threads.insert(
            slug.into(),
            json!({"blocks": [{"type": "thread", "entries": [{"who": "agent", "timestamp": "2026-11-14 10:00",
                "verdict": null, "blocks": [paragraph(vec![text("Posted, with its evidence.")]), paragraph(vec![picture()])]}]}]}),
        );
    }
    let headings: Vec<Value> = before
        .chain(after)
        .map(|n| json!({"level": 2, "anchor": format!("part-{n}"), "text": format!("Part {n}")}))
        .collect();
    let cards: Vec<Value> = CARDS
        .iter()
        .map(|slug| json!({"slug": slug, "state": "pending", "text": slug, "group": "decide", "open": true}))
        .collect();
    json!({"version": 7, "path": LANDING, "title": "Landing", "waiting": [], "crumbs": [],
        "headings": headings, "tasks": [], "sections": sections, "cards": cards, "threads": threads,
        "anchors": anchors})
}

/// A document with a link to the fifth card.
fn from() -> Value {
    let link = json!({"type": "link", "href": LINK, "target": null, "title": "", "spans": [text("the fifth card")]});
    json!({"version": 7, "path": FROM, "title": "From", "waiting": [], "crumbs": [], "headings": [],
        "tasks": [], "cards": [], "threads": {},
        "sections": [{"start": 1, "end": 2, "key": "intro", "hash": "intro", "part": 0,
            "blocks": [paragraph(vec![text("Read "), link, text(".")])]}]})
}

/// The fourth card, just above the fifth, grown by six paragraphs: the document after an edit above
/// the card, and the feed's change that brings it, every section's ids in it again (as an outline
/// read again names them, each a new list to the page).
fn grown() -> (Value, Value) {
    let mut four = card("four", 124);
    four["hash"] = json!("four-grown");
    let body = four["blocks"][0]["body"]
        .as_array_mut()
        .expect("the card's body");
    for n in 1..=6 {
        body.push(paragraph(vec![text(&format!(
            "Grown above the fifth card, line {n}."
        ))]));
    }
    let mut document = landing();
    for section in document["sections"].as_array_mut().expect("its sections") {
        if section["key"] == "card:four" {
            *section = four.clone();
        }
    }
    let change = json!({"version": 8, "changes": [{"region": "document", "data": {
        "path": LANDING, "sections": [four], "anchors": document["anchors"].clone()}}]});
    (document, change)
}

fn site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    site.set_document(&landing());
    site.set_document(&from());
    site.set_json("/api/asks", &json!({"asks": [], "recipient": null}));
    site.set_json("/api/postponed", &json!([]));
    site.set_file(
        PICTURE,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="700"><rect width="600" height="700" fill="teal"/></svg>"#.to_vec(),
        "image/svg+xml",
    );
    site
}

/// True once the page has moved to the link's address and scrolled the fifth card's id (its ghost's,
/// while its section is read) to the page's scroll padding.
const LANDED: &str = "(() => { const line = document.getElementById('five');
    return location.pathname === '/docs/landing.md' && document.documentElement.dataset.navigation === 'ready'
        && line !== null && Math.abs(line.getBoundingClientRect().top - parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop)) < 1; })()";

/// True once the fifth card is drawn and has held still for ten frames with nothing near the
/// screen still to draw (a reserved box within its sentinel's 600 px) and nothing busy; false
/// after sixty frames without that, for the wait to look again.
const STILL: &str = "new Promise(done => {
    let last = null, still = 0, frames = 0;
    const near = box => { const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; };
    const look = () => {
        const card = document.getElementById('five')?.closest('.doc-card');
        const top = card ? card.getBoundingClientRect().top : null;
        const waiting = document.querySelector('[aria-busy=true]') !== null || [...document.querySelectorAll('[data-pending=true]')].some(near);
        still = top !== null && top === last && !waiting ? still + 1 : 0;
        last = top;
        if (still >= 10) return done(true);
        if (++frames >= 60) return done(false);
        requestAnimationFrame(look);
    };
    requestAnimationFrame(look);
})";

/// Where the fifth card sits: its first line's top (the element its id names, which the page
/// scrolls to), its box's top, the header's bottom, the page's scroll padding, and how far down the
/// document its first line is.
const PLACE: &str = "(() => { const line = document.getElementById('five'), card = line?.closest('.doc-card');
    return {line: line?.getBoundingClientRect().top ?? null, card: card?.getBoundingClientRect().top ?? null,
        header: document.querySelector('.app-header').getBoundingClientRect().bottom,
        padding: parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop),
        down: line ? line.getBoundingClientRect().top + scrollY : null, y: scrollY}; })()";

/// True once the ghost over the card's page has lifted (`.settle`, at its deadline while the reads
/// are held) and the card's ghost is at the scroll padding again.
fn lifted() -> String {
    format!(
        "document.getElementById('five')?.closest('.settle')?.dataset.settled === 'true' && {LANDED}"
    )
}

/// Counts the page's own scrolls from now on, by an element or the window (`window.pageScrolls`).
pub(crate) const COUNT_SCROLLS: &str = "(() => { window.pageScrolls = 0;
    const count = (owner, name) => { const own = owner[name]; owner[name] = function (...args) { window.pageScrolls += 1; return own.apply(this, args); }; };
    count(Element.prototype, 'scrollIntoView'); count(Element.prototype, 'scrollTo'); count(Element.prototype, 'scrollBy');
    count(window, 'scrollTo'); count(window, 'scrollBy'); count(window, 'scroll'); return true; })()";

/// Two frames from now: a hold running sees what happened before it.
pub(crate) const TWO_FRAMES: &str =
    "new Promise(done => requestAnimationFrame(() => requestAnimationFrame(() => done(true))))";

/// A browser of its own, `width` by `height`, whose device keeps no copy of what it reads.
fn browser(width: u32, height: u32) -> Option<Chrome> {
    let mut page = crate::chrome()?;
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": width < 600}),
    );
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": NO_STORE }),
    );
    Some(page)
}

/// Arrive at the fifth card by its link in a browser of its own, `width` by `height`: opened at
/// it (`in_place` false), or from a click on it in another document; with every section's read
/// held until the page has landed, then let go. Says where the card landed, where it is once the
/// page around it has drawn, and a moment later.
fn arrive(site: &Site, width: u32, height: u32, in_place: bool) -> Option<[Value; 3]> {
    let mut page = browser(width, height)?;
    if in_place {
        page.open(&site.url(&format!("/{FROM}")));
        let link = format!("a[href=\"{LINK}\"]");
        assert!(
            page.wait_for(
                &format!(
                    "document.documentElement.dataset.navigation === 'ready' && {}",
                    crate::visible(&link)
                ),
                SHOWN
            ),
            "{width}px: the link to the card is drawn"
        );
        site.hold_route("/api/section", true);
        page.eval(&format!("document.querySelector({}).click()", json!(link)));
    } else {
        site.hold_route("/api/section", true);
        page.open(&site.url(&format!("/{LANDING}#five")));
    }
    let way = if in_place { "in place" } else { "opened" };
    assert!(
        page.wait_for(LANDED, SHOWN),
        "{width}px {way}: the page scrolls to the card's ghost first: {}",
        page.eval(PLACE)
    );
    let landed = page.eval(PLACE);
    site.hold_route("/api/section", false);
    assert!(
        page.wait_for(STILL, SHOWN),
        "{width}px {way}: the page around the card draws and holds still: {}",
        page.eval(PLACE)
    );
    let drawn = page.eval(PLACE);
    page.pump(std::time::Duration::from_millis(600));
    Some([landed, drawn, page.eval(PLACE)])
}

#[test]
fn a_link_to_a_document_card_lands_it_below_the_header_and_it_stays_as_the_sections_above_draw() {
    let site = site();
    for (width, height) in [(1280, 900), (390, 844)] {
        for in_place in [false, true] {
            let Some([landed, drawn, later]) = arrive(&site, width, height, in_place) else {
                return;
            };
            let way = if in_place { "in place" } else { "opened" };
            eprintln!("LANDING {width}px {way}: landed {landed}, drawn {drawn}, later {later}");
            let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
            // What drew above the card after the landing moved it down the document: a page that
            // did not hold it would show it that much lower.
            assert!(
                number(&drawn, "down") - number(&landed, "down") > 300.0,
                "{width}px {way}: the sections above the card draw after the landing, taller than their ghosts: {landed} then {drawn}"
            );
            for place in [&drawn, &later] {
                assert!(
                    (number(place, "line") - number(place, "padding")).abs() <= 1.0
                        && number(place, "card") >= number(place, "header"),
                    "{width}px {way}: the card's first line sits at the scroll padding, its box below the header: landed {landed}, drawn {drawn}, a moment later {later}"
                );
            }
        }
    }
}

/// How a reader leaves a landing: a wheel up the page, or a script's scroll with no event of the
/// reader's.
#[derive(Clone, Copy, Debug)]
enum Away {
    Wheel,
    Script,
}

#[test]
fn a_reader_who_scrolls_away_from_a_landing_is_left_there_when_the_reads_land() {
    let site = site();
    for away in [Away::Wheel, Away::Script] {
        let Some(mut page) = browser(1280, 900) else {
            return;
        };
        site.hold_route("/api/section", true);
        page.open(&site.url(&format!("/{LANDING}#five")));
        // The ghost lifts first, so the reader's scroll is the only one: the lift itself moves
        // the page under it, which the landing holds.
        assert!(
            page.wait_for(&lifted(), SHOWN),
            "{away:?}: the page lands on the card's ghost and its ghost lifts: {}",
            page.eval(PLACE)
        );
        let y = page.eval(PLACE)["y"].as_f64().unwrap_or(f64::NAN);
        match away {
            Away::Wheel => {
                page.call(
                    "Input.dispatchMouseEvent",
                    json!({"type": "mouseWheel", "x": 640, "y": 450, "deltaX": 0, "deltaY": -500}),
                );
            }
            Away::Script => {
                page.eval("scrollBy(0, -500)");
            }
        }
        assert!(
            page.wait_for(&format!("scrollY < {y} - 250"), SHOWN),
            "{away:?}: the reader scrolls up from the card: {}",
            page.eval(PLACE)
        );
        page.eval(TWO_FRAMES);
        let left = page.eval(PLACE);
        page.eval(COUNT_SCROLLS);
        // The reads land: the card's own (a section read in lands it again) and the page's last.
        site.hold_route("/api/section", false);
        assert!(
            page.wait_for(STILL, SHOWN),
            "{away:?}: the page around the card draws and holds still: {}",
            page.eval(PLACE)
        );
        page.pump(std::time::Duration::from_millis(600));
        let later = page.eval(PLACE);
        eprintln!("LEFT {away:?}: left {left}, later {later}");
        assert_eq!(
            page.eval("window.pageScrolls"),
            json!(0),
            "{away:?}: nothing scrolls the page once the reader left the landing: left {left}, later {later}"
        );
        let number = |key: &str| later[key].as_f64().unwrap_or(f64::NAN);
        assert!(
            (number("line") - number("padding")).abs() > 100.0,
            "{away:?}: the card is not brought back to the scroll padding: left {left}, later {later}"
        );
    }
}

/// What a reader does on the card while it lands that scrolls nothing: a click on it, a key that
/// moves nothing (Escape; a Tab moves the focus, which the page scrolls to when it is off the
/// screen), or a tap on a phone.
#[derive(Clone, Copy, Debug)]
enum Still {
    Click,
    Key,
    Tap,
}

#[test]
fn a_click_a_tap_or_a_key_that_scrolls_nothing_leaves_the_card_held_as_the_reads_land() {
    let site = site();
    for act in [Still::Click, Still::Key, Still::Tap] {
        let (width, height) = if matches!(act, Still::Tap) {
            (390, 844)
        } else {
            (1280, 900)
        };
        let Some(mut page) = browser(width, height) else {
            return;
        };
        if matches!(act, Still::Tap) {
            page.call(
                "Emulation.setTouchEmulationEnabled",
                json!({"enabled": true, "maxTouchPoints": 1}),
            );
        }
        site.hold_route("/api/section", true);
        page.open(&site.url(&format!("/{LANDING}#five")));
        assert!(
            page.wait_for(&lifted(), SHOWN),
            "{act:?}: the page lands on the card's ghost and its ghost lifts: {}",
            page.eval(PLACE)
        );
        // On the card's ghost, below its first line.
        let (x, y) = (width / 2, 76 + 40);
        match act {
            Still::Click => {
                for kind in ["mousePressed", "mouseReleased"] {
                    page.call(
                        "Input.dispatchMouseEvent",
                        json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}),
                    );
                }
            }
            Still::Key => crate::components::key(&mut page, "Escape", "Escape", 27),
            Still::Tap => {
                page.call(
                    "Input.dispatchTouchEvent",
                    json!({"type": "touchStart", "touchPoints": [{"x": x, "y": y}]}),
                );
                page.call(
                    "Input.dispatchTouchEvent",
                    json!({"type": "touchEnd", "touchPoints": []}),
                );
            }
        }
        page.eval(TWO_FRAMES);
        let acted = page.eval(PLACE);
        site.hold_route("/api/section", false);
        assert!(
            page.wait_for(STILL, SHOWN),
            "{act:?}: the page around the card draws and holds still: {}",
            page.eval(PLACE)
        );
        page.pump(std::time::Duration::from_millis(600));
        let later = page.eval(PLACE);
        eprintln!("STILL {act:?}: acted {acted}, later {later}");
        let number = |key: &str| later[key].as_f64().unwrap_or(f64::NAN);
        assert!(
            (number("line") - number("padding")).abs() <= 1.0 && number("card") >= number("header"),
            "{act:?}: the card's first line stays at the scroll padding as the sections above draw: acted {acted}, later {later}"
        );
    }
}

#[test]
fn once_a_landing_went_quiet_a_change_above_the_card_after_a_script_s_scroll_lands_nothing() {
    let site = site();
    let Some(mut page) = browser(1280, 900) else {
        return;
    };
    page.open(&site.url(&format!("/{LANDING}#five")));
    // Ten frames still with nothing waiting or busy near: the hold (four such frames) has ended.
    assert!(
        page.wait_for(
            &format!("{LANDED} && document.querySelector('[data-feed=live]') !== null"),
            SHOWN
        ) && page.wait_for(STILL, SHOWN),
        "the page lands on the card, follows its feed and holds still: {}",
        page.eval(PLACE)
    );
    let y = page.eval(PLACE)["y"].as_f64().unwrap_or(f64::NAN);
    page.eval("scrollBy(0, -500)");
    assert!(
        page.wait_for(&format!("scrollY < {y} - 250"), SHOWN),
        "a script scrolls up from the card: {}",
        page.eval(PLACE)
    );
    page.eval(TWO_FRAMES);
    let left = page.eval(PLACE);
    page.eval(COUNT_SCROLLS);
    // An edit above the card: the fourth card grows, every section's ids named again.
    let (document, change) = grown();
    site.set_document(&document);
    site.push(8, &change.to_string());
    assert!(
        page.wait_for(
            "document.body.innerText.includes('Grown above the fifth card, line 6.')",
            SHOWN
        ) && page.wait_for(STILL, SHOWN),
        "the change draws above the card and the page holds still: {}",
        page.eval(PLACE)
    );
    page.pump(std::time::Duration::from_millis(600));
    let later = page.eval(PLACE);
    eprintln!("QUIET: left {left}, later {later}");
    assert_eq!(
        page.eval("window.pageScrolls"),
        json!(0),
        "nothing scrolls the page once its landing went quiet: left {left}, later {later}"
    );
    let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
    // The page moved since the landing let it be, not only by the script: a landing compared
    // with where it left the page would take the script's scroll for a draw.
    assert!(
        number(&later, "down") - number(&left, "down") >= 1.0,
        "the change grows the document above the card: left {left}, later {later}"
    );
    assert!(
        (number(&later, "line") - number(&later, "padding")).abs() > 100.0,
        "the card is not brought back to the scroll padding: left {left}, later {later}"
    );
}

/// The root's `overflow-anchor`: `none` while the page's landing holds what the address names
/// (`land`, shell/shell.ts), so no adjustment of the browser's own scroll anchoring reads to it as
/// a scroll away; `auto` once the landing has ended.
const ANCHORING: &str = "getComputedStyle(document.documentElement).overflowAnchor";

/// The root's own `overflow-anchor`, as its style has it: empty for none.
const OWN_ANCHORING: &str = "document.documentElement.style.overflowAnchor";

/// Leaves the part the page landed on out of what the browser anchors on, so it anchors on what
/// is drawn below it.
const ANCHOR_BELOW: &str = "(() => { document.getElementById('five').closest('.doc-part').style.overflowAnchor = 'none'; return true; })()";

/// How far [`GROW`] moves what is below the landed part.
const GROWN: f64 = 85.0;

/// Grows the part the page landed on below its first line: what is below it moves down the
/// document, and the line the page landed on does not.
const GROW: &str = "(() => { document.getElementById('five').closest('.doc-part').style.paddingBottom = '85px'; return true; })()";

/// A change that moves what the browser anchors on and not the element the page landed on (as a
/// near part half drawn when the ghost lifts does) scrolls nothing while the landing holds, and
/// the card stays at the scroll padding as the sections above it draw. Its control, the root's
/// anchoring turned back on by the test, shows what the landing's `overflow-anchor: none` keeps
/// away: the browser scrolls the page by that change, the page and the element alike, which ends
/// the landing as a scroll away, and the card is left where the sections put it.
#[test]
fn a_change_under_what_the_browser_anchors_on_scrolls_nothing_while_a_landing_holds() {
    let site = site();
    for browser_anchors in [false, true] {
        let Some(mut page) = browser(1280, 900) else {
            return;
        };
        site.hold_route("/api/section", true);
        page.open(&site.url(&format!("/{LANDING}#five")));
        assert!(
            page.wait_for(&lifted(), SHOWN),
            "anchoring {browser_anchors}: the page lands on the card's ghost and its ghost lifts: {}",
            page.eval(PLACE)
        );
        assert_eq!(
            page.eval(ANCHORING),
            json!("none"),
            "anchoring {browser_anchors}: the browser's own scroll anchoring is off while the landing holds"
        );
        page.eval(ANCHOR_BELOW);
        if browser_anchors {
            page.eval("document.documentElement.style.removeProperty('overflow-anchor'); true");
        }
        // The browser picks what it anchors on as it lays the page out, before the change.
        page.eval(TWO_FRAMES);
        let before = page.eval(PLACE);
        page.eval(GROW);
        page.eval(TWO_FRAMES);
        let grown = page.eval(PLACE);
        page.eval(COUNT_SCROLLS);
        site.hold_route("/api/section", false);
        assert!(
            page.wait_for(STILL, SHOWN),
            "anchoring {browser_anchors}: the page around the card draws and holds still: {}",
            page.eval(PLACE)
        );
        let later = page.eval(PLACE);
        eprintln!("ANCHORED {browser_anchors}: before {before}, grown {grown}, later {later}");
        let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
        let scrolled = number(&grown, "y") - number(&before, "y");
        let off = number(&later, "line") - number(&later, "padding");
        if browser_anchors {
            assert!(
                (scrolled - GROWN).abs() <= 1.0,
                "the control: with its anchoring on, the browser scrolls the page by what grew: before {before}, grown {grown}"
            );
            assert_eq!(
                page.eval("window.pageScrolls"),
                json!(0),
                "the control: that scroll ended the landing, and nothing scrolls the page as the sections draw: grown {grown}, later {later}"
            );
            assert!(
                off < -50.0,
                "the control: the card is not brought back to the scroll padding: grown {grown}, later {later}"
            );
        } else {
            assert!(
                scrolled.abs() < 0.5,
                "nothing scrolls the page while the landing holds: before {before}, grown {grown}"
            );
            assert!(
                number(&later, "down") - number(&grown, "down") > 300.0,
                "the sections above the card draw after the change, taller than their ghosts: grown {grown}, later {later}"
            );
            assert!(
                off.abs() <= 1.0 && number(&later, "card") >= number(&later, "header"),
                "the card's first line stays at the scroll padding as the sections above draw: grown {grown}, later {later}"
            );
        }
    }
}

/// A root `overflow-anchor` of the page's own, set before a link is followed to a card, is there
/// once the landing has ended, as the page wrote it: the landing puts back what it found, and a
/// navigation with no landing to end touches none.
#[test]
fn a_root_overflow_anchor_set_before_a_landing_is_there_after_it() {
    let site = site();
    for own in ["none", "auto"] {
        let Some(mut page) = browser(1280, 900) else {
            return;
        };
        page.open(&site.url(&format!("/{FROM}")));
        let link = format!("a[href=\"{LINK}\"]");
        assert!(
            page.wait_for(
                &format!(
                    "document.documentElement.dataset.navigation === 'ready' && {}",
                    crate::visible(&link)
                ),
                SHOWN
            ),
            "{own}: the link to the card is drawn"
        );
        page.eval(&format!(
            "document.documentElement.style.overflowAnchor = {}; true",
            json!(own)
        ));
        site.hold_route("/api/section", true);
        page.eval(&format!("document.querySelector({}).click()", json!(link)));
        assert!(
            page.wait_for(&lifted(), SHOWN),
            "{own}: the page lands on the card's ghost and its ghost lifts: {}",
            page.eval(PLACE)
        );
        assert_eq!(
            page.eval(OWN_ANCHORING),
            json!("none"),
            "{own}: the browser's own scroll anchoring is off while the landing holds"
        );
        site.hold_route("/api/section", false);
        // Ten frames still with nothing waiting or busy near: the hold (four such frames) has ended.
        assert!(
            page.wait_for(STILL, SHOWN),
            "{own}: the page around the card draws and holds still: {}",
            page.eval(PLACE)
        );
        assert_eq!(
            page.eval(OWN_ANCHORING),
            json!(own),
            "{own}: the root's own overflow-anchor is back once the landing has ended"
        );
    }
}

/// A landing whose element is gone (no element has the id the address names) ends on the next
/// frame, the browser's own scroll anchoring back, though the sections near it still wait to
/// draw; and the card drawn again later, when its read lands, is not landed on again.
#[test]
fn a_landing_whose_element_is_gone_ends_without_waiting_for_the_page_to_go_quiet() {
    let site = site();
    let Some(mut page) = browser(1280, 900) else {
        return;
    };
    site.hold_route("/api/section", true);
    page.open(&site.url(&format!("/{LANDING}#five")));
    assert!(
        page.wait_for(&lifted(), SHOWN),
        "the page lands on the card's ghost and its ghost lifts: {}",
        page.eval(PLACE)
    );
    assert_eq!(
        page.eval(ANCHORING),
        json!("none"),
        "the browser's own scroll anchoring is off while the landing holds"
    );
    page.eval("document.getElementById('five').removeAttribute('id'); true");
    page.eval(TWO_FRAMES);
    assert_eq!(
        page.eval(format!("[{ANCHORING}, {PENDING_NEAR}.length > 0]").as_str()),
        json!(["auto", true]),
        "the landing ended with its element gone, the sections near it still to draw"
    );
    page.eval(COUNT_SCROLLS);
    site.hold_route("/api/section", false);
    assert!(
        page.wait_for(STILL, SHOWN),
        "the page around the card draws and holds still: {}",
        page.eval(PLACE)
    );
    assert_eq!(
        page.eval("window.pageScrolls"),
        json!(0),
        "nothing scrolls the page once its landing ended: {}",
        page.eval(PLACE)
    );
}

/// Holds every IntersectionObserver's callbacks until `window.observe()`: what comes near the
/// screen is drawn only then, so the page's first read is the addressed card's alone, and the
/// sections near it are read after it, when the test lets them come.
const OBSERVE_LATER: &str = "(() => { const Observer = window.IntersectionObserver, queued = []; let open = false;
    window.observe = () => { open = true; queued.splice(0).forEach(run => run()); return true; };
    window.IntersectionObserver = class extends Observer { constructor(callback, options) {
        super((entries, observer) => open ? callback(entries, observer) : queued.push(() => callback(entries, observer)), options); } }; })();";

/// A card's thread taller than a phone's screen (two pictures), so the card's field, under it, is
/// below the screen when the card lands.
fn tall_history() -> Value {
    json!({"blocks": [{"type": "thread", "entries": [{"who": "agent",
        "timestamp": "2026-11-14 10:00", "verdict": null, "blocks": [
            paragraph(vec![text("Posted, with its evidence.")]), paragraph(vec![picture()]), paragraph(vec![picture()])]}]}]})
}

/// The page's reads of sections so far, each by the keys it asked for.
fn section_reads(site: &Site) -> Vec<Vec<String>> {
    site.targets()
        .iter()
        .filter(|target| target.starts_with("/api/section"))
        .map(|target| crate::site::asked_keys(target))
        .collect()
}

/// Waits, the page let run meanwhile, until the page has asked for `count` reads of sections, and
/// says them; a stall past the backstop fails, naming them.
#[track_caller]
fn reads_asked(page: &mut Chrome, site: &Site, count: usize) -> Vec<Vec<String>> {
    let asked = test_time::until_true(format!("{count} reads of sections"), STALL, || {
        section_reads(site).len() >= count || {
            page.pump(std::time::Duration::from_millis(25));
            false
        }
    });
    if let Err(stall) = asked {
        panic!("{stall}: {:?}", section_reads(site));
    }
    section_reads(site)
}

/// Where what the reader reads (`window.reading`) is: its box's top and bottom, how far down the
/// document its top is, the window's height and the page's scroll.
const READ: &str = "(() => { const box = window.reading.getBoundingClientRect();
    return {top: box.top, bottom: box.bottom, down: box.top + scrollY, height: innerHeight, y: scrollY}; })()";

/// The document's sections near the screen still to draw (a reserved box or a ghost within its
/// sentinel's 600 px): the classes of the part each is in, and its top and bottom.
const PENDING_NEAR: &str = "[...document.querySelectorAll('.doc-part > [data-pending=true]')]
    .map(box => { const r = box.getBoundingClientRect(); return [box.parentElement?.className ?? null, r.top, r.bottom]; })
    .filter(([, top, bottom]) => bottom > -600 && top < innerHeight + 600)";

/// True once what the reader reads has held still for ten frames, with no section near the screen
/// still to draw when `drawn` (a reserved box or a ghost within its sentinel's 600 px; the links of
/// a phone's closed outline draw only once it is opened); false after sixty frames without that,
/// for the wait to look again.
fn held(drawn: bool) -> String {
    format!(
        "new Promise(done => {{
        let last = null, still = 0, frames = 0;
        const near = box => {{ const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; }};
        const look = () => {{
            const now = window.reading.isConnected ? `${{window.reading.getBoundingClientRect().top}} ${{scrollY}}` : null;
            const waiting = {drawn} && [...document.querySelectorAll('.doc-part > [data-pending=true]')].some(near);
            still = now !== null && now === last && !waiting ? still + 1 : 0;
            last = now;
            if (still >= 10) return done(true);
            if (++frames >= 60) return done(false);
            requestAnimationFrame(look);
        }};
        requestAnimationFrame(look);
    }})"
    )
}

/// What the reader does once the page has landed that ends the landing: scrolls on down by a
/// wheel on a desk or a swipe on a phone, or, on a phone, taps a card's words and types in the
/// field that opens, the page's scroll to the field ending the landing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reading {
    Wheeled,
    Swiped,
    Typing,
}

impl Reading {
    /// The window it reads in: a desk's, or a phone's.
    fn window(self) -> (u32, u32) {
        match self {
            Reading::Wheeled => (1280, 900),
            Reading::Swiped | Reading::Typing => (390, 844),
        }
    }
}

/// Runs the deadline of the page's ghost ([`reader`] holds it) where it still waits, and says how
/// many timers of its length the page held: its one.
const SETTLE_DEADLINE: &str =
    "(() => { const due = [...settleDeadline.values()]; settleDeadline.clear();
    due.forEach(run => run()); return due.length; })()";

/// A browser of its own for `reading` (a phone's takes touches), whose sections near the screen
/// are read only once the test lets them come ([`OBSERVE_LATER`]), and whose ghost's deadline
/// (`Settle`'s, [`symphony_web_tests::SETTLE_MS`]) the test runs ([`SETTLE_DEADLINE`]) in place of
/// the clock: `window.settleDeadline` holds it. The page sets no other timer of that length here:
/// the asks' retry and the chat's, as long, follow a read that failed, and none does.
fn reader(reading: Reading) -> Option<Chrome> {
    let (width, height) = reading.window();
    let mut page = browser(width, height)?;
    if reading != Reading::Wheeled {
        page.call(
            "Emulation.setTouchEmulationEnabled",
            json!({"enabled": true, "maxTouchPoints": 1}),
        );
    }
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": OBSERVE_LATER }),
    );
    crate::hold_timers(&mut page, "settleDeadline", symphony_web_tests::SETTLE_MS);
    Some(page)
}

/// True once the page has moved to `path` and scrolled the element `id` names (its ghost's, while
/// its section is read) to the page's scroll padding.
pub(crate) fn landed_on(path: &str, id: &str) -> String {
    format!(
        "(() => {{ const line = document.getElementById({id});
        return location.pathname === {path} && document.documentElement.dataset.navigation === 'ready'
            && line !== null && Math.abs(line.getBoundingClientRect().top - parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop)) < 1; }})()",
        id = json!(id),
        path = json!(format!("/{path}")),
    )
}

/// What a reader read: where what the page landed on was ([`READ`]), and where what they read was
/// before and after the sections near them drew, with the page's own scrolls meanwhile.
struct Read {
    landed: Value,
    before: Value,
    after: Value,
    scrolls: Value,
}

impl Read {
    /// Asserts that the reader's own act moved the page from the landing (to a field typed in, on
    /// the screen), and that what they read stayed within a few px, by the browser's anchoring and
    /// no scroll of the page's, while the sections near them drew more than `drew` px from where
    /// their ghosts put it in the document.
    #[track_caller]
    fn held(&self, what: &str, reading: Reading, drew: f64) {
        let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
        let on_screen = |place: &Value| {
            number(place, "top") >= 0.0 && number(place, "bottom") <= number(place, "height")
        };
        let typing = reading == Reading::Typing;
        let (landed, before, after) = (&self.landed, &self.before, &self.after);
        assert!(
            number(before, "y") - number(landed, "y") > 100.0 && (!typing || on_screen(before)),
            "{what} {reading:?}: the reader's own act moved the page from the landing (to the field, on the screen): landed {landed}, then {before}"
        );
        assert!(
            (number(after, "down") - number(before, "down")).abs() > drew,
            "{what} {reading:?}: the sections near the reader draw after their act, other than their ghosts: before {before}, after {after}"
        );
        assert_eq!(
            self.scrolls,
            json!(0),
            "{what} {reading:?}: the browser holds the reader's place, not a scroll of the page's: before {before}, after {after}"
        );
        assert!(
            (number(after, "top") - number(before, "top")).abs() <= 3.0,
            "{what} {reading:?}: what the reader reads stays where it was, within a few px: before {before}, after {after}"
        );
        assert!(
            !typing || on_screen(after),
            "{what} {reading:?}: the field typed in stays on the screen: before {before}, after {after}"
        );
    }
}

/// Opens `path` at the element `id`, which the section `key` holds, with every section's read
/// held: the page's first read is that section's alone and is let go; the sections near it come
/// near, and their read is held while the page's ghost lifts at its deadline, which the test runs
/// ([`SETTLE_DEADLINE`]), and the reader ends the landing by `reading`, then let go.
#[track_caller]
fn read_while_drawn(
    page: &mut Chrome,
    site: &Site,
    path: &str,
    id: &str,
    key: &str,
    reading: Reading,
) -> Read {
    let element = format!("document.getElementById({})", json!(id));
    let landed = landed_on(path, id);
    let asked = section_reads(site).len();
    site.hold_route("/api/section", true);
    page.open(&site.url(&format!("/{path}#{id}")));
    assert!(
        page.wait_for(&landed, SHOWN),
        "{id} {reading:?}: the page scrolls to its ghost first: {}",
        page.eval(&format!("{element}?.getBoundingClientRect().top ?? null"))
    );
    let first = reads_asked(page, site, asked + 1);
    assert_eq!(
        first[asked..],
        [vec![key.to_owned()]],
        "{id} {reading:?}: its section's read comes first, alone"
    );
    site.hold_route("/api/section", false);
    assert!(
        page.wait_for(
            &format!("{element}?.closest('[data-pending]') === null && {landed}"),
            SHOWN
        ),
        "{id} {reading:?}: it draws at the scroll padding: {}",
        page.eval(&format!("{element}?.getBoundingClientRect().top ?? null"))
    );
    // The sections near it come near, and their read is held.
    site.hold_route("/api/section", true);
    page.eval("window.observe()");
    reads_asked(page, site, asked + 2);
    // The landing is in and the near reads are held: the ghost's deadline passes now.
    assert_eq!(
        page.eval(SETTLE_DEADLINE),
        json!(1),
        "{id} {reading:?}: the page holds its ghost's deadline, and no other timer of its length"
    );
    assert!(
        page.wait_for(
            &format!("{element}?.closest('.settle')?.dataset.settled === 'true' && {landed}"),
            SHOWN
        ),
        "{id} {reading:?}: the page's ghost lifts with it at the scroll padding: {}",
        page.eval(&format!("{element}?.getBoundingClientRect().top ?? null"))
    );
    assert_eq!(
        page.eval(ANCHORING),
        json!("none"),
        "{id} {reading:?}: the browser's own scroll anchoring is off while the landing holds"
    );
    page.eval(TWO_FRAMES);
    page.eval(&format!("window.reading = {element}; true"));
    let landed = page.eval(READ);
    let y = landed["y"].as_f64().unwrap_or(f64::NAN);
    let (width, height) = reading.window();
    match reading {
        Reading::Wheeled => {
            page.call(
                "Input.dispatchMouseEvent",
                json!({"type": "mouseWheel", "x": width / 2, "y": height / 2, "deltaX": 0, "deltaY": 500}),
            );
        }
        Reading::Swiped => {
            // A finger drawn 500 px up the screen and lifted at rest: no fling carries the reader on.
            crate::swipe(page, width / 2, height * 3 / 4, 500);
        }
        Reading::Typing => {
            let at = page.eval(&format!(
                "(() => {{ const words = {element}.closest('.doc-card').querySelector('.section-prose p').getBoundingClientRect();
                    return [words.left + 20, words.top + words.height / 2]; }})()"
            ));
            let point = json!({"x": at[0], "y": at[1]});
            page.call(
                "Input.dispatchTouchEvent",
                json!({"type": "touchStart", "touchPoints": [point]}),
            );
            page.call(
                "Input.dispatchTouchEvent",
                json!({"type": "touchEnd", "touchPoints": []}),
            );
            assert!(
                page.wait_for(
                    "document.activeElement?.matches('textarea') === true",
                    SHOWN
                ),
                "{id} {reading:?}: the tap opens the card's editor, its field taking the focus: {}",
                page.eval("document.activeElement?.outerHTML.slice(0, 200)")
            );
            page.call("Input.insertText", json!({"text": "Keep every lamp"}));
            assert!(
                page.wait_for("document.activeElement.value === 'Keep every lamp'", SHOWN),
                "{id} {reading:?}: the reader types in the field"
            );
        }
    }
    if reading == Reading::Typing {
        page.eval("window.reading = document.activeElement; true");
    } else {
        assert!(
            page.wait_for(&format!("scrollY > {y} + 250"), SHOWN),
            "{id} {reading:?}: the reader scrolls on down: landed {landed}, now {}",
            page.eval("scrollY")
        );
        page.eval(
            "window.reading = document.elementFromPoint(innerWidth / 2, innerHeight * 0.4); true",
        );
        assert_eq!(
            page.eval(&format!(
                "{element}.closest('.doc-part').contains(window.reading)"
            )),
            json!(true),
            "{id} {reading:?}: what the reader reads is in the section the page landed on: {}",
            page.eval("window.reading?.outerHTML.slice(0, 200) ?? null")
        );
    }
    assert!(
        page.wait_for(&held(false), SHOWN),
        "{id} {reading:?}: what the reader reads holds still while the reads are held: {}",
        page.eval(READ)
    );
    assert_eq!(
        page.eval(ANCHORING),
        json!("auto"),
        "{id} {reading:?}: the reader's act ended the landing, and the browser's own scroll anchoring is back to hold their place"
    );
    let before = page.eval(READ);
    page.eval(COUNT_SCROLLS);
    site.hold_route("/api/section", false);
    assert!(
        page.wait_for(&held(true), SHOWN),
        "{id} {reading:?}: the sections near the screen draw and the page holds still: {}, still to draw near it: {}",
        page.eval(READ),
        page.eval(PENDING_NEAR)
    );
    let after = page.eval(READ);
    eprintln!("READING {id} {reading:?}: landed {landed}, before {before}, after {after}");
    Read {
        landed,
        before,
        after,
        scrolls: page.eval("window.pageScrolls"),
    }
}

const FORMS: &str = "docs/forms.md";
const MOCKUP: &str = "/docs/forms/mock.html";

/// A paragraph long enough to wrap over a few lines on a phone and on a desk.
fn words(what: &str) -> Value {
    paragraph(vec![text(&format!(
        "{what}. Words of a paragraph that a reader reads, long enough to wrap over a few lines on a \
         phone and on a desk, so the section is taller than one line and holds the reader's eye \
         while the page draws around it."
    ))])
}

/// A paragraph taller than a window on a desk and on a phone, which a reader reads on in.
fn long_words(what: &str) -> Value {
    let lines: String = (1..=64)
        .map(|n| {
            format!("{what}, sentence {n}: words a reader reads on in while the page draws above. ")
        })
        .collect();
    paragraph(vec![text(lines.trim_end())])
}

fn heading(level: u8, anchor: &str, words: &str) -> Value {
    json!({"type": "heading", "level": level, "anchor": anchor, "spans": [text(words)]})
}

fn code() -> Value {
    json!({"type": "code", "language": "", "text": "let lamps = \"a code block\";"})
}

fn quote() -> Value {
    json!({"type": "quote", "blocks": [words("A quote")]})
}

/// A project's HTML mockup, framed.
fn mockup() -> Value {
    json!({"type": "media", "media": {"kind": "mockup", "src": MOCKUP, "label": "A mockup"}})
}

/// A list of `items`, each a paragraph.
fn list(items: usize) -> Value {
    let items: Vec<Value> = (1..=items)
        .map(|n| json!([words(&format!("Item {n}"))]))
        .collect();
    json!({"type": "list", "ordered": false, "start": 1, "items": items})
}

/// A table of lamps, `rows` of them.
fn table(rows: usize) -> Value {
    let cell = |words: &str, header: bool| json!({"header": header, "spans": [text(words)], "presentation": {}});
    let rows: Vec<Value> = (1..=rows)
        .map(|n| json!([cell(&format!("Lamp {n}"), false), cell("Warm white", false)]))
        .collect();
    json!({"type": "table", "head": [cell("Lamp", true), cell("Colour", true)], "rows": rows})
}

/// What a form's reader reads: a section of prose (its one block), or a card.
enum Target {
    Prose(Value),
    Card,
}

/// A form of the forms document: the blocks above what the reader reads, and below it.
struct Form {
    case: &'static str,
    above: Vec<Value>,
    target: Target,
    below: Vec<Value>,
}

/// The forms beside what a reader reads that a guess of a section not drawn yet got wrong, or that
/// main's page did not hold either: code, a quote, a mockup and a picture alone (a block of its
/// own, as the server makes a paragraph of one picture) above a paragraph, a list or a table, and
/// above or below a card.
fn forms() -> Vec<Form> {
    let below = || vec![words("A paragraph below")];
    let prose = |case: &'static str, above: Value, target: Value| Form {
        case,
        above: vec![above],
        target: Target::Prose(target),
        below: below(),
    };
    let card = |case: &'static str, above: Value, below: Vec<Value>| Form {
        case,
        above: vec![above],
        target: Target::Card,
        below,
    };
    vec![
        prose("p-after-code", code(), long_words("Read")),
        prose("p-after-quote", quote(), long_words("Read")),
        prose("p-after-mockup", mockup(), long_words("Read")),
        prose("p-after-picture", picture(), long_words("Read")),
        prose("list-after-code", code(), list(16)),
        prose("table-after-quote", quote(), table(40)),
        card("card-after-code", code(), below()),
        card("card-after-quote", quote(), below()),
        card("card-after-mockup", mockup(), below()),
        card("card-after-picture", picture(), below()),
        card(
            "card-then-h3",
            words("A paragraph above"),
            vec![
                heading(3, "card-then-h3-sub", "A subsection"),
                words("Under it"),
            ],
        ),
        card(
            "card-then-table",
            words("A paragraph above"),
            vec![table(2), words("Under it")],
        ),
        // The last section of its part.
        card("last-card", picture(), Vec::new()),
    ]
}

/// The sections of a document being written, as the server splits them: each block a section of
/// its own, each card one, each in the part of the heading it follows.
#[derive(Default)]
struct Written {
    sections: Vec<Value>,
    anchors: Map<String, Value>,
    threads: Map<String, Value>,
    cards: Vec<Value>,
    headings: Vec<Value>,
}

impl Written {
    /// The section of `block`, the next of those under `anchor`, in `part`: the id it holds (`L`
    /// and its start line) and its key.
    fn section(&mut self, anchor: &str, part: usize, block: Value) -> (String, String) {
        let start = 4 * self.sections.len() + 1;
        let under = format!("section:{anchor}:");
        let n = self
            .sections
            .iter()
            .filter(|section| {
                section["key"]
                    .as_str()
                    .is_some_and(|key| key.starts_with(&under))
            })
            .count();
        let key = format!("{under}{n}");
        let mut ids = Vec::new();
        if block["type"] == "heading" {
            ids.push(block["anchor"].clone());
            self.headings
                .push(json!({"level": block["level"], "anchor": block["anchor"],
                "text": block["spans"][0]["text"]}));
        }
        self.anchors.insert(key.clone(), json!(ids));
        self.sections.push(
            json!({"start": start, "end": start + 2, "key": key, "hash": key,
            "part": part, "blocks": [block]}),
        );
        (format!("L{start}"), key)
    }

    /// The card `slug` waiting on the owner, in `part`, its thread taller than a phone's screen:
    /// the id it holds (its slug) and its key.
    fn card(&mut self, slug: &str, part: usize) -> (String, String) {
        let mut section = card(slug, 4 * self.sections.len() + 1);
        section["part"] = json!(part);
        let key = format!("card:{slug}");
        self.anchors.insert(key.clone(), json!([slug]));
        self.threads.insert(slug.into(), tall_history());
        self.cards.push(
            json!({"slug": slug, "state": "pending", "text": slug, "group": "decide", "open": true}),
        );
        self.sections.push(section);
        (slug.into(), key)
    }
}

/// What a reader of the forms document reads: the form's name, the id the page lands on, the key
/// of its section, and whether it is a card.
struct Reads {
    case: String,
    id: String,
    key: String,
    card: bool,
}

/// The forms document: its title over its intro, a part for each form (its heading, a long
/// paragraph over a tall picture, what is above what is read, what is read, what is below and three
/// paragraphs), and a part of paragraphs after the last. Says it, and what each form reads, the
/// intro under the title first.
fn forms_document() -> (Value, Vec<Reads>) {
    let mut written = Written::default();
    written.section("forms", 0, heading(1, "forms", "Forms"));
    let (id, key) = written.section("forms", 0, long_words("The intro under the title"));
    let mut reads = vec![Reads {
        case: "intro-under-title".into(),
        id,
        key,
        card: false,
    }];
    for (part, form) in (1..).zip(forms()) {
        let anchor = format!("case-{}", form.case);
        written.section(
            &anchor,
            part,
            heading(2, &anchor, &format!("Case {}", form.case)),
        );
        // Far taller than its ghost, so what draws above the reader moves them a long way where
        // the browser does not hold them.
        written.section(&anchor, part, long_words("A paragraph opening the case"));
        written.section(&anchor, part, picture());
        for block in form.above {
            written.section(&anchor, part, block);
        }
        let card = matches!(form.target, Target::Card);
        let (id, key) = match form.target {
            Target::Prose(block) => written.section(&anchor, part, block),
            Target::Card => written.card(&format!("t-{}", form.case), part),
        };
        reads.push(Reads {
            case: form.case.into(),
            id,
            key,
            card,
        });
        let last = form.below.is_empty();
        for block in form.below {
            written.section(&anchor, part, block);
        }
        for n in (0..3).filter(|_| !last) {
            written.section(&anchor, part, words(&format!("Filler {n} after the case")));
        }
    }
    let part = reads.len();
    written.section("after", part, heading(2, "after", "After"));
    for n in 0..6 {
        written.section("after", part, words(&format!("Filler {n} after the cases")));
    }
    let Written {
        sections,
        anchors,
        threads,
        cards,
        headings,
    } = written;
    let document = json!({"version": 7, "path": FORMS, "title": "Forms", "waiting": [],
        "crumbs": [], "headings": headings, "tasks": [], "sections": sections, "cards": cards,
        "threads": threads, "anchors": anchors});
    (document, reads)
}

/// Reads each form of the forms document that is a card (`cards`) or prose, on a desk by a wheel
/// and on a phone by a swipe, or for a card by typing in its field, and asserts the reader stays
/// on what they read while the sections near them draw.
fn each_form_holds(cards: bool) {
    let site = site();
    let (document, reads) = forms_document();
    site.set_document(&document);
    site.set_html_page(
        MOCKUP,
        "<!doctype html><title>A mockup</title><p>A mockup's page.</p>",
    );
    let phone = if cards {
        Reading::Typing
    } else {
        Reading::Swiped
    };
    for reading in [Reading::Wheeled, phone] {
        let Some(mut page) = reader(reading) else {
            return;
        };
        for read in reads.iter().filter(|read| read.card == cards) {
            page.open("about:blank");
            let held = read_while_drawn(&mut page, &site, FORMS, &read.id, &read.key, reading);
            held.held(&read.case, reading, 50.0);
        }
    }
}

/// The spacing between parts reads a neighbour not drawn yet by the kinds of its first and last
/// blocks, which the outline carries (`ends`), so it is exact for prose as for cards: a paragraph,
/// a list or a table being read keeps its margins as the code, quote, mockup or picture above it
/// draws, and the intro its own as the title draws. A guess (prose ends in a paragraph and starts
/// with one) moved these readers by what drew above them, 838 to 1225 px on a desk.
#[test]
fn a_reader_of_prose_stays_on_it_whatever_draws_beside_it() {
    each_form_holds(false);
}

/// A card being read keeps its margins as the code, quote, mockup or picture above it draws, and
/// the h3 or the table below it, and so does the last card of its part: main's page moved its
/// reader by everything drawn above, 823 to 2158 px on a desk, and its field off a phone's screen.
#[test]
fn a_reader_of_a_card_stays_on_it_whatever_draws_beside_it() {
    each_form_holds(true);
}
