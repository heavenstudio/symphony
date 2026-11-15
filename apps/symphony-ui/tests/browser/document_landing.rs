//! A link to a card on a document page lands with the card's first line just below the header, at
//! the page's scroll padding, and the card stays there while the page draws around it: the scroll
//! to it brings the sections above it near, and they draw after it, taller than the ghosts they
//! replace (a picture in each, and in each card's history). On a first visit and on a move in place
//! from another document, on a desk and a phone. The landing is held by the shell
//! (`land`, assets/shell.js); here the sections' reads are held until the page has landed, so
//! they draw after it on any machine, and the device keeps no copy of them ([`NO_STORE`]): the
//! shell's read-ahead of the other document's links would otherwise put the card's sections on it
//! before the reads are held. A reader who scrolls away during the landing, by a wheel or a
//! script, is left there when the reads land; one who clicks, taps or presses a key that scrolls
//! nothing keeps the card held; and once the landing has gone quiet, nothing lands it again, not a
//! change to the document above the card after a script's scroll. Once the reader's own scroll
//! ended the landing, the browser's scroll anchoring keeps what they read in place while the
//! sections above still draw, a field typed in on a phone among it.

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

/// Holds every IntersectionObserver's callbacks until `window.observe()`: what comes near the
/// screen is drawn only then, so the page's first read is the addressed card's alone, and the
/// sections near it are read after it, when the test lets them come.
const OBSERVE_LATER: &str = "(() => { const Observer = window.IntersectionObserver, queued = []; let open = false;
    window.observe = () => { open = true; queued.splice(0).forEach(run => run()); return true; };
    window.IntersectionObserver = class extends Observer { constructor(callback, options) {
        super((entries, observer) => open ? callback(entries, observer) : queued.push(() => callback(entries, observer)), options); } }; })();";

/// The landing document with the fifth card's thread taller than a phone's screen (two pictures),
/// so the card's field, under its thread, is below the screen when the card lands.
fn tall_thread() -> Value {
    let mut document = landing();
    document["threads"]["five"] = json!({"blocks": [{"type": "thread", "entries": [{"who": "agent",
        "timestamp": "2026-11-14 10:00", "verdict": null, "blocks": [
            paragraph(vec![text("Posted, with its evidence.")]), paragraph(vec![picture()]), paragraph(vec![picture()])]}]}]});
    document
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

/// True once what the reader reads has held still for ten frames, with nothing near the screen
/// still to draw when `drawn` (a reserved box or a ghost within its sentinel's 600 px); false after
/// sixty frames without that, for the wait to look again.
fn held(drawn: bool) -> String {
    format!(
        "new Promise(done => {{
        let last = null, still = 0, frames = 0;
        const near = box => {{ const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; }};
        const look = () => {{
            const now = window.reading.isConnected ? `${{window.reading.getBoundingClientRect().top}} ${{scrollY}}` : null;
            const waiting = {drawn} && [...document.querySelectorAll('[data-pending=true]')].some(near);
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

/// What the reader does once the card has landed that ends the landing: scrolls on down the card
/// by a wheel on a desk, or, on a phone, taps the card's words and types in the field that opens,
/// the page's scroll to the field ending the landing.
#[derive(Clone, Copy, Debug)]
enum Reading {
    Wheeled,
    Typing,
}

/// The browser's own scroll anchoring keeps the reader's place while the sections above draw: the
/// spacing between the document's parts (docs.css) counts a neighbour not drawn yet as what it
/// draws as, so no margin of the card being read changes as they draw, which would stop the
/// anchoring for that frame. Before, the card's own margin changed as the cards beside it drew,
/// and the page drew them under the reader: a wheel left the card at 863 px instead of -439, and a
/// phone's field typed in went from 409 px to 1285 px, off an 844 px screen.
#[test]
fn a_reader_who_ended_a_landing_stays_on_what_they_read_while_the_sections_above_draw() {
    let site = site();
    site.set_document(&tall_thread());
    for reading in [Reading::Wheeled, Reading::Typing] {
        let typing = matches!(reading, Reading::Typing);
        let (width, height) = if typing { (390, 844) } else { (1280, 900) };
        let Some(mut page) = browser(width, height) else {
            return;
        };
        if typing {
            page.call(
                "Emulation.setTouchEmulationEnabled",
                json!({"enabled": true, "maxTouchPoints": 1}),
            );
        }
        page.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": OBSERVE_LATER }),
        );
        let asked = section_reads(&site).len();
        site.hold_route("/api/section", true);
        page.open(&site.url(&format!("/{LANDING}#five")));
        assert!(
            page.wait_for(LANDED, SHOWN),
            "{reading:?}: the page scrolls to the card's ghost first: {}",
            page.eval(PLACE)
        );
        let first = reads_asked(&mut page, &site, asked + 1);
        assert_eq!(
            first[asked..],
            [vec!["card:five".to_owned()]],
            "{reading:?}: the card's own read comes first, alone"
        );
        site.hold_route("/api/section", false);
        assert!(
            page.wait_for(
                &format!(
                    "document.getElementById('five')?.closest('.doc-card') != null && {LANDED}"
                ),
                SHOWN
            ),
            "{reading:?}: the card draws at the scroll padding: {}",
            page.eval(PLACE)
        );
        // The sections near the card come near, and their read is held.
        site.hold_route("/api/section", true);
        page.eval("window.observe()");
        let near = reads_asked(&mut page, &site, asked + 2);
        assert!(
            near[asked + 1].iter().any(|key| key == "card:four"),
            "{reading:?}: the cards above the card are read after it: {near:?}"
        );
        assert!(
            page.wait_for(&lifted(), SHOWN),
            "{reading:?}: the page's ghost lifts with the card at the scroll padding: {}",
            page.eval(PLACE)
        );
        page.eval(TWO_FRAMES);
        let landed = page.eval(PLACE);
        let y = landed["y"].as_f64().unwrap_or(f64::NAN);
        match reading {
            Reading::Wheeled => {
                page.call(
                    "Input.dispatchMouseEvent",
                    json!({"type": "mouseWheel", "x": width / 2, "y": height / 2, "deltaX": 0, "deltaY": 500}),
                );
                assert!(
                    page.wait_for(&format!("scrollY > {y} + 250"), SHOWN),
                    "{reading:?}: the reader scrolls down the card: {}",
                    page.eval(PLACE)
                );
                page.eval(
                    "window.reading = document.elementFromPoint(innerWidth / 2, innerHeight * 0.4); true",
                );
            }
            Reading::Typing => {
                let at = page.eval(
                    "(() => { const words = document.getElementById('five').closest('.doc-card').querySelector('.section-prose p').getBoundingClientRect();
                        return [words.left + 20, words.top + words.height / 2]; })()",
                );
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
                    "{reading:?}: the tap opens the card's editor, its field taking the focus: {}",
                    page.eval("document.activeElement?.outerHTML.slice(0, 200)")
                );
                page.call("Input.insertText", json!({"text": "Keep every lamp"}));
                assert!(
                    page.wait_for("document.activeElement.value === 'Keep every lamp'", SHOWN),
                    "{reading:?}: the reader types in the field"
                );
                page.eval("window.reading = document.activeElement; true");
            }
        }
        assert!(
            page.wait_for(&held(false), SHOWN),
            "{reading:?}: what the reader reads holds still while the reads are held: {}",
            page.eval(READ)
        );
        let before = page.eval(READ);
        let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
        let on_screen = |place: &Value| {
            number(place, "top") >= 0.0 && number(place, "bottom") <= number(place, "height")
        };
        assert!(
            number(&before, "y") - y > 100.0 && (!typing || on_screen(&before)),
            "{reading:?}: the reader's own scroll moved the page from the landing (to the field, on the screen): landed {landed}, then {before}"
        );
        page.eval(COUNT_SCROLLS);
        site.hold_route("/api/section", false);
        assert!(
            page.wait_for(&held(true), SHOWN),
            "{reading:?}: the sections near the screen draw and the page holds still: {}",
            page.eval(READ)
        );
        let after = page.eval(READ);
        eprintln!("READING {reading:?}: landed {landed}, before {before}, after {after}");
        // What drew above the reader after they left the landing: a page that did not hold them
        // would show what they read that much lower.
        assert!(
            number(&after, "down") - number(&before, "down") > 300.0,
            "{reading:?}: the sections above draw after the reader's scroll, taller than their ghosts: before {before}, after {after}"
        );
        assert_eq!(
            page.eval("window.pageScrolls"),
            json!(0),
            "{reading:?}: the browser holds the reader's place, not a scroll of the page's: before {before}, after {after}"
        );
        assert!(
            (number(&after, "top") - number(&before, "top")).abs() <= 3.0,
            "{reading:?}: what the reader reads stays where it was, within a few px: before {before}, after {after}"
        );
        assert!(
            !typing || on_screen(&after),
            "{reading:?}: the field typed in stays on the screen: before {before}, after {after}"
        );
    }
}
