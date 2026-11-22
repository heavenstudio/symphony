//! Lists read as lists and the home queue as rows, as the owner sees them: a list item's marker
//! paints beside its text's first line, and neither list items nor queue rows stand further apart
//! than a line of the page's text, whatever the space around them.
use super::{ParitySite, painted_layout, paragraph, responses};
use crate::chrome::Chrome;
use crate::{SHOWN, chrome};
use serde_json::{Value, json};

fn list(ordered: bool, start: u64, items: Value) -> Value {
    json!({"type":"list","ordered":ordered,"start":start,"items":items})
}

/// Every kind of list a document holds: wrapped, nested, loose and ordered items, and lists
/// inside a callout and a card. Every item is a paragraph block, as the server sends tight and
/// loose Markdown items alike.
fn list_document() -> Value {
    let long = "A long item that wraps onto a second line at desktop width, and onto several on a phone, since its text runs on well past one line of the reading column.";
    let nested = list(
        false,
        1,
        json!([
            [paragraph("A nested item.")],
            [paragraph("Another nested item.")]
        ]),
    );
    json!([
        {"start":1,"end":8,"key":"lists","hash":"lists","part":0,"blocks":[list(false,1,json!([
            [paragraph("A short item.")],
            [paragraph(long), nested],
            [paragraph("A loose item's first paragraph."), paragraph("Its second paragraph.")],
            [paragraph("The last item.")]
        ]))]},
        {"start":10,"end":11,"key":"ordered","hash":"ordered","part":0,"blocks":[list(true,3,json!([[paragraph("The third step.")],[paragraph("The fourth step.")]]))]},
        {"start":13,"end":16,"key":"callout","hash":"callout","part":1,"blocks":[{"type":"callout","kind":"question","title":[{"type":"text","text":"A question"}],"resolved":false,"blocks":[
            paragraph("A callout's list:"),
            list(false,1,json!([[paragraph("One choice.")],[paragraph("Another choice.")]]))
        ]}]},
        {"start":18,"end":22,"key":"card:lists","hash":"lists-card","part":2,"blocks":[{"type":"card","slug":"lists","mark":"pending","title":"Lists","body":[
            paragraph("A card's list:"),
            list(true,1,json!([[paragraph("The first option.")],[paragraph("The second option.")]]))
        ],"verdict":{"group":"decide","open":true,"posted":null,"decided":null},"thread":[],"line":18,"line_offset":null,"comparison":[]}]}
    ])
}

/// Mark the list items SELECTOR matches, and give each in document order: its text, its first
/// line's text box, and its pitch, the distance from the top of the text line before it in the
/// same list (an earlier item, or the item holding its nested list) to the top of its own first
/// line.
const ITEMS: &str = "(() => {
  const rects = node => { const range = document.createRange(); range.selectNodeContents(node); return [...range.getClientRects()].filter(r => r.width > 0); };
  const outer = item => { let list = item.closest('ul,ol'); for (let up; (up = list.parentElement.closest('ul,ol'));) list = up; return list; };
  return [...document.querySelectorAll(SELECTOR)].map(item => {
    item.dataset.measured = '';
    const walker = document.createTreeWalker(outer(item), NodeFilter.SHOW_TEXT, {acceptNode: t => t.textContent.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP});
    const texts = [];
    for (let t; (t = walker.nextNode());) texts.push(t);
    const at = texts.findIndex(t => item.contains(t)), first = rects(texts[at])[0], style = getComputedStyle(item);
    return {text: texts[at].textContent.trim(), top: first.top, bottom: first.bottom, left: first.left,
      pitch: at > 0 ? first.top - rects(texts[at - 1]).at(-1).top : null,
      line: parseFloat(style.lineHeight), font: parseFloat(style.fontSize)};
  });
})()";

/// The box of every marked item's `::marker`, in document order, as DevTools outlines it:
/// `[left, top, right, bottom]`.
fn marker_boxes(browser: &mut Chrome) -> Vec<[f64; 4]> {
    fn collect(node: &Value, out: &mut Vec<u64>) {
        let measured = node["attributes"]
            .as_array()
            .is_some_and(|pairs| pairs.chunks(2).any(|pair| pair[0] == "data-measured"));
        if measured {
            let marker = node["pseudoElements"]
                .as_array()
                .and_then(|pseudo| pseudo.iter().find(|p| p["pseudoType"] == "marker"))
                .unwrap_or_else(|| panic!("a list item has no marker: {node}"));
            out.push(marker["nodeId"].as_u64().unwrap());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            collect(child, out);
        }
    }
    let document = browser.call("DOM.getDocument", json!({"depth":-1}));
    let mut markers = Vec::new();
    collect(&document["root"], &mut markers);
    markers
        .into_iter()
        .map(|id| {
            let quad =
                browser.call("DOM.getBoxModel", json!({"nodeId":id}))["model"]["border"].clone();
            let at = |index: usize| quad[index].as_f64().unwrap();
            [at(0), at(1), at(2), at(5)]
        })
        .collect()
}

/// The pixels inside each box, as the page paints them now.
fn shots(browser: &mut Chrome, boxes: &[[f64; 4]]) -> Vec<String> {
    boxes
        .iter()
        .map(|[left, top, right, bottom]| {
            let clip =
                json!({"x":left,"y":top,"width":right - left,"height":bottom - top,"scale":1});
            browser.call(
                "Page.captureScreenshot",
                json!({"format":"png","clip":clip}),
            )["data"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// Whether each marker paints: the pixels in its box change when the marked items' markers are
/// taken away. A marker clipped by its item's containment changes nothing.
fn painted(browser: &mut Chrome, boxes: &[[f64; 4]]) -> Vec<bool> {
    let shown = shots(browser, boxes);
    browser.eval("document.querySelectorAll('[data-measured]').forEach(item => item.style.listStyleType = 'none'); new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
    let hidden = shots(browser, boxes);
    browser.eval("document.querySelectorAll('[data-measured]').forEach(item => item.style.listStyleType = '')");
    shown.iter().zip(&hidden).map(|(a, b)| a != b).collect()
}

/// Every list item `selector` matches (`count` of them, `measured` with a text line before them
/// in their list) paints its marker left of its first line, its middle within that line, and
/// starts within a line plus half an em of the line before it.
fn assert_list_lines(
    browser: &mut Chrome,
    selector: &str,
    count: usize,
    measured: usize,
    at: &str,
) {
    let items = browser.eval(&ITEMS.replace("SELECTOR", &json!(selector).to_string()));
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), count, "{selector} {at}");
    let boxes = marker_boxes(browser);
    assert_eq!(boxes.len(), count, "one marker per item {at}");
    let painted = painted(browser, &boxes);
    for ((item, [left, top, right, bottom]), painted) in items.iter().zip(boxes).zip(painted) {
        let text = item["text"].as_str().unwrap();
        let number = |key: &str| item[key].as_f64().unwrap();
        let middle = (top + bottom) / 2.0;
        assert!(
            painted,
            "{text:?} {at}: its marker [{left}, {top}, {right}, {bottom}] paints nothing"
        );
        assert!(
            (number("top")..=number("bottom")).contains(&middle) && right <= number("left"),
            "{text:?} {at}: its marker [{left}, {top}, {right}, {bottom}] must sit left of its first line {item}"
        );
        if let Some(pitch) = item["pitch"].as_f64() {
            // A line of text, plus no more than half the em a paragraph break would add.
            let limit = number("line") + number("font") / 2.0;
            assert!(
                pitch <= limit,
                "{text:?} {at} starts {pitch}px below the line before it, over a line of the list ({limit}px)"
            );
        }
    }
    assert_eq!(
        items.iter().filter(|item| !item["pitch"].is_null()).count(),
        measured,
        "every item but each list's first is measured against the line before it {at}"
    );
}

/// Widths the owner reads at; tall enough that every lazy slot is revealed and on screen.
fn viewport(browser: &mut Chrome, width: u32) {
    browser.viewport(width, if width < 400 { 2400 } else { 1600 }, width < 400);
}

#[test]
fn a_list_items_marker_sits_on_its_first_line_and_items_are_spaced_as_lines() {
    let mut data = responses();
    let sections = list_document();
    let document = &mut data.get_mut("/api/document").unwrap()["document"];
    document["sections"] = sections.clone();
    document["headings"] = json!([]);
    document["cards"] =
        json!([{"slug":"lists","state":"pending","text":"Lists","group":"decide","open":true}]);
    data.get_mut("/api/section").unwrap()["change"]["sections"] = sections;
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        viewport(&mut browser, width);
        browser.open(&site.url("/docs/look-parity.md"));
        assert!(
            browser.wait_for("(() => {const items=[...document.querySelectorAll('.doc-body li')]; return items.length === 12 && items.every(li=>li.textContent.trim()) && !document.querySelector('.doc-body .viewport-placeholder, .doc-body .section-loading');})()", SHOWN),
            "every list item must be mounted at {width}px: {}",
            browser.eval("document.querySelector('.doc-body')?.innerText")
        );
        painted_layout(&mut browser);
        assert_list_lines(
            &mut browser,
            ".doc-body li",
            12,
            8,
            &format!("at {width}px"),
        );
    }
}

/// A task's Subtasks list mounts each row lazily as the list item itself, not inside one.
#[test]
fn a_task_pages_subtasks_paint_their_markers_on_their_lines() {
    let mut data = responses();
    data.get_mut("/api/task-page").unwrap()["children_count"] = json!(9);
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        viewport(&mut browser, width);
        browser.open(&site.url("/task/visual/in-progress"));
        assert!(
            browser.wait_for("(() => {const items=[...document.querySelectorAll('li[id^=\"child-\"]')]; return items.length === 9 && items.every(li=>li.textContent.trim());})()", SHOWN),
            "every subtask must be mounted at {width}px: {}",
            browser.eval("document.body.innerText")
        );
        painted_layout(&mut browser);
        assert_list_lines(
            &mut browser,
            "li[id^=\"child-\"]",
            9,
            8,
            &format!("at {width}px"),
        );
    }
}

#[test]
fn the_home_queue_rows_keep_list_spacing_beside_a_taller_next_card() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        browser.viewport(width, 900, width < 400);
        browser.open(&site.url("/"));
        assert!(browser.wait_for(
            "document.querySelectorAll('.nextcard').length === 4 && document.querySelector('.hero .pics') !== null",
            SHOWN
        ));
        painted_layout(&mut browser);
        let page = browser.eval("(() => {const rows=[...document.querySelectorAll('.nextcard')].map(e=>e.getBoundingClientRect()), card=document.querySelector('.next').getBoundingClientRect(), hero=document.querySelector('.hero').getBoundingClientRect(); return {line:parseFloat(getComputedStyle(document.querySelector('.nextlist')).lineHeight), gaps:rows.slice(1).map((r,i)=>r.top-rows[i].bottom), stretched:card.height === hero.height, room:document.querySelector('.next a.more').getBoundingClientRect().top-rows.at(-1).bottom};})()");
        let line = page["line"].as_f64().unwrap();
        if width == 1280 {
            assert_eq!(
                page["stretched"],
                json!(true),
                "the Then card stands beside the hero at its height: {page}"
            );
            assert!(
                page["room"].as_f64().unwrap() > line,
                "the fixture's hero must leave the Then card more room than its rows need: {page}"
            );
        }
        let gaps = page["gaps"].as_array().unwrap();
        assert_eq!(gaps.len(), 3);
        for gap in gaps {
            assert!(
                gap.as_f64().unwrap() <= line,
                "queue rows at {width}px must stand no further apart than a line ({line}px): {page}"
            );
        }
    }
}
