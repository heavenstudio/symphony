//! Where each region a page's ghost outlines stands, against where the page draws it once its
//! reads are in: Chrome's layout-shift score cannot see it, as each real element is a new one in
//! the ghost's place. The board, the queue and the tasks are shaped as the live ones are: columns
//! of many tasks whose titles take one to three lines, some cards with chips and some without, a
//! queue group of one row, and tasks of different heads, gates and briefs.
//!
//! On a first visit a ghost knows only what every such page shows, and what the last page of its
//! kind drew in a window as wide (a task never opened takes the last task's head, gates and
//! brief); on the next one this browser kept what that page drew (the board's columns, counts and
//! cards, a task's head, gates and brief, the queue's groups), and every region is where the
//! ghost put it. The client cache's store is off ([`NO_STORE`]): with it, the board and the queue
//! draw a return visit from this device and show no ghost. No scrollbar takes room
//! ([`NO_SCROLLBARS`]), so the window's scrollbar kind cannot change every region's width.
use super::{NO_STORE, SHOWN, chrome, chrome::Chrome, layout_shift, site::Site};
use serde_json::{Value, json};

/// The most a region may stand off the ghost's box for it, in px: a height kept rounded.
const SLACK: f64 = 2.0;

/// A page whose ghost shows while `held` are read: `ghost` once it is drawn, `page` once the page
/// is, and each region by its name, the ghost's box for it and the page's (a selector, or after
/// `js:` an expression for the node).
struct Page {
    name: &'static str,
    path: &'static str,
    held: &'static [&'static str],
    ghost: &'static str,
    page: &'static str,
    regions: &'static [(&'static str, &'static str, &'static str)],
}

/// A task page's regions: its head, gates, timeline, brief and thread. The gates, the timeline's
/// box and the thread's heading are the page's own from its first draw, each part its mock-up
/// until its read answers; the head and the brief are their ghosts until the page's read does.
const TASK_REGIONS: &[(&str, &str, &str)] = &[
    ("head", "main .ghost-head", "article.task > .task-head"),
    ("gates", "main .task-gates", "article.task .task-gates"),
    (
        "timeline",
        "article.task #timeline-slot",
        "article.task #timeline-slot",
    ),
    ("brief", "main .ghost-brief", "article.task > .task-body"),
    (
        "thread",
        "article.task > h2:not([id])",
        "article.task > h2:not([id])",
    ),
];

const TASK_GHOST: &str = "document.querySelector('main .ghost-head') !== null";
/// The task's page with its gates read and its thread's cover lifted: the thread itself reads
/// only near the screen, under a timeline and a long brief on a phone.
const TASK_PAGE: &str = "document.querySelector('article.task .settle[data-settled=true]') !== null && document.querySelector('.task-gates[data-gates=read]') !== null";

/// The first task: a title of one line wide, four gates, a brief of a few paragraphs.
const GATES: Page = Page {
    name: "task board-cards",
    path: "/task/tools/board-cards",
    held: &["/api/task-page"],
    ghost: TASK_GHOST,
    page: TASK_PAGE,
    regions: TASK_REGIONS,
};

/// The second: a title of two lines wide and three on a phone, three gates, a longer brief.
const LOADING: Page = Page {
    name: "task skeleton-loading",
    path: "/task/app/skeleton-loading",
    held: &["/api/task-page"],
    ghost: TASK_GHOST,
    page: TASK_PAGE,
    regions: TASK_REGIONS,
};

const BOARD: Page = Page {
    name: "tasks",
    path: "/tasks",
    held: &["/api/task-board"],
    ghost: "document.querySelector('.ghost-kanban.waiting-columns') !== null",
    page: "innerWidth <= 760 ? document.querySelector('.column[data-status=review] .taskcard') !== null : document.querySelectorAll('.column[data-status=concept] .taskcard').length === 5",
    regions: &[
        ("filters", "main .filters", "main .filters"),
        ("kanban", ".ghost-kanban", ".kanban"),
        (
            "pipeline",
            ".ghost-kanban.waiting-columns",
            ".kanban.waiting-columns",
        ),
        (
            "column",
            ".ghost-kanban.waiting-columns > .waiting-pipeline > .ghost-column",
            ".waiting-columns .column",
        ),
        (
            "fourth card",
            "js:document.querySelectorAll('.ghost-column[data-for=concept] .ghost-card')[3]",
            "js:document.querySelectorAll('.column[data-status=concept] .taskcard')[3]",
        ),
        (
            "wait strip",
            ".waiting-ghost .wait-strip",
            ".waiting-board:not(.waiting-ghost) .wait-strip",
        ),
    ],
};

const DOCUMENT: Page = Page {
    name: "document",
    path: "/docs/PROBE.md",
    held: &["/api/document"],
    ghost: "document.querySelector('.ghost-doc-body') !== null",
    page: "document.querySelector('.settle[data-settled=true] .doc-body') !== null",
    regions: &[
        ("head", ".ghost-doc-head", ".doc-head"),
        ("outline", ".ghost-toc", ".doc-toc"),
        ("body", ".ghost-doc-body", ".doc-body"),
    ],
};

const QUEUE: Page = Page {
    name: "queue",
    path: "/queue",
    held: &["/api/queue"],
    ghost: "document.querySelector('.queue-page .ghost-group') !== null",
    page: "document.querySelector('[data-ready=queue] #agent .queue-row') !== null",
    regions: &[
        ("group", ".ghost-group", "#you"),
        ("chips", ".ghost-chips", "#you .queue-categories"),
        ("row", ".ghost-group .ghost-row", "#you .queue-row"),
        ("second group", ".ghost-group[data-for=spot]", "#spot"),
        ("third group", ".ghost-group[data-for=agent]", "#agent"),
    ],
};

/// The titles the board's tasks take in turn: one line to three in a column.
const TITLES: [&str; 10] = [
    "Retake the queue's pictures",
    "Keep each column's cards at the height this browser drew them, so the board does not jump as it opens",
    "A failed read says so in its place",
    "Show each gate's state beside a task's head on a phone, with who signed it and when",
    "Hold the reader's place while a card lands",
    "Report every page kind's first-visit bytes beside the owner's budget",
    "Nightly in a clean checkout outside the folder",
    "Name the recipes the skills use, and test that each is installed",
    "Move the theme menu into the appearance panel",
    "The timeline's lanes fit a phone",
];

/// A board shaped as the live one: many tasks in some columns, few in others; the cards of
/// columns no agent has taken up with a linked document on some and not others, those taken up
/// with their roles.
fn board() -> Value {
    let columns = [
        ("concept", 5),
        ("design", 6),
        ("ready", 20),
        ("in_progress", 3),
        ("review", 2),
        ("done", 4),
        ("backlog", 6),
        ("blocked", 1),
        ("cancelled", 2),
    ];
    let mut tasks = Vec::new();
    for (column, (status, count)) in columns.iter().enumerate() {
        for n in 0..*count {
            let linked = match *status {
                "concept" => n % 2 == 0,
                "design" => n != 3,
                "ready" => n % 2 == 0,
                "in_progress" => n == 0,
                "backlog" => n == 0,
                _ => false,
            };
            let staffed = ["in_progress", "review", "done"].contains(status)
                || (*status == "ready" && n % 5 == 0);
            let roles = if staffed {
                json!([{"handle":"opal-plover","role":"executor","since":1.0,"expires":null,"health":"live"}])
            } else {
                json!([])
            };
            tasks.push(json!({
                "slug": format!("{}/t-{n:02}", status.replace('_', "-")),
                "title": TITLES[(column * 3 + n) % TITLES.len()],
                "body": "", "status": status, "priority": n % 3, "parent": null,
                "created_by": "owner", "created": 1.0, "updated": 1.0,
                "links": if linked { json!(["docs/TASKS.md#gates"]) } else { json!([]) },
                "depends": [], "roles": roles, "instruction": false
            }));
        }
    }
    let statuses: Vec<&str> = columns.iter().map(|(status, _)| *status).collect();
    json!({"tasks": tasks, "statuses": statuses, "signoff": [], "counts": columns, "next": null})
}

/// A queue whose first group shows one row, its second none and its third three.
fn queue() -> Value {
    let board: Value = serde_json::from_str(&super::common::fixture("board.json")).unwrap();
    let agent: Vec<Value> = (0..3)
        .map(|n| {
            json!({"href": format!("/docs/DECISIONS.md#a-{n}"), "path": "docs/DECISIONS.md",
            "slug": format!("a-{n}"), "title": TITLES[n * 3], "kind": "comment",
            "text": "Waiting on the agent's answer to the owner's comment."})
        })
        .collect();
    json!({"waiting": [board["next"][0]], "spot_check": [], "agent": agent, "branches": []})
}

/// The task `slug`, titled `title`, its page with `gates` (name, state, detail) and a brief of
/// `paragraphs`, and a thread of three messages, as the site answers them.
fn show_task(
    site: &Site,
    slug: &str,
    title: &str,
    gates: &[(&str, &str, &str)],
    paragraphs: usize,
) {
    let task = json!({"slug": slug, "title": title, "body": "", "status": "in_progress",
        "priority": 1, "parent": "app/review-ui", "created_by": "owner", "created": 1.0,
        "updated": 1.0, "links": ["docs/TASKS.md#gates"], "depends": ["tools/board-writer"],
        "roles": [{"handle":"opal-plover","role":"executor","since":1.0,"expires":null,"health":"live"}],
        "instruction": false});
    let brief: Vec<Value> = (0..paragraphs)
        .map(|n| {
            json!({"type":"paragraph","spans":[{"type":"text","text":format!(
            "Part {n} of the brief: what the task changes, why, and the tests that show it, \
             long enough to wrap onto a second and a third line in a column of text.")}]})
        })
        .collect();
    let waiting = gates
        .iter()
        .filter(|(_, state, _)| *state == "missing")
        .count();
    site.set_json(
        "/api/task-page",
        &json!({"task": task, "blocks": brief, "children": [], "children_count": 0,
            "gates": {"count": gates.len(), "waiting": waiting, "unsigned": 0}}),
    );
    let gates: Vec<Value> = gates
        .iter()
        .map(|(name, state, detail)| json!({"name": name, "state": state, "by": "studio", "at": 1_794_363_240.0, "detail": detail}))
        .collect();
    site.set_json(
        "/api/task-gates",
        &json!({"slug": slug, "branch": slug, "gates": gates}),
    );
    let entries: Vec<Value> = (1..=3)
        .map(|id| json!({"message": {"id": id, "slug": slug, "sender": if id == 2 { "opal-plover" } else { "owner" },
            "recipient": null, "kind": "message", "text": "A message on the task's thread.", "ts": 1.0},
            "stamp": "2026-11-12", "blocks": []}))
        .collect();
    site.set_json(
        "/api/task-history",
        &json!({"entries": entries, "before": null, "after": 3}),
    );
}

/// The first task's ([`GATES`]).
fn board_cards(site: &Site) {
    show_task(
        site,
        "tools/board-cards",
        "Keep the board's cards as they were drawn",
        &[
            ("review", "missing", "Not reviewed yet"),
            ("nightly", "met", "Green on main a71f0d2"),
            ("certificate", "missing", "Verify running on the merge tree"),
            (
                "dependency",
                "waived",
                "Waits on tools/board-writer, waived",
            ),
        ],
        3,
    );
}

/// The second task's ([`LOADING`]).
fn skeleton_loading(site: &Site) {
    show_task(
        site,
        "app/skeleton-loading",
        "Placeholders keep what the last visit drew, on the board, a task, a document and the queue",
        &[
            ("review", "missing", "Not reviewed yet"),
            ("nightly", "met", "Green on main a71f0d2"),
            ("dependency", "met", "tools/board-writer landed"),
        ],
        6,
    );
}

/// Each region's box, `[top in the page, width]`, by the `index`th selector of its entry
/// (1 the ghost's, 2 the page's); null for one not drawn or out of the flow.
fn boxes(page: &Page, index: usize) -> String {
    let selectors: Vec<Value> = page
        .regions
        .iter()
        .map(|region| json!([region.0, if index == 1 { region.1 } else { region.2 }]))
        .collect();
    format!(
        "Object.fromEntries({}.map(([name, css]) => {{ const node = css.startsWith('js:') ? eval(css.slice(3)) : document.querySelector(css), box = node?.getBoundingClientRect();
            return [name, box && box.height > 0 && getComputedStyle(node).position !== 'fixed' ? [Math.round(box.top + scrollY), Math.round(box.width)] : null]; }}))",
        json!(selectors)
    )
}

/// A region on one visit: its box on the ghost and on the page.
#[derive(Debug)]
struct Region {
    name: &'static str,
    ghost: Value,
    page: Value,
}

impl Region {
    /// How far the page's box stands from the ghost's, `[down, wider]` in px; none where either
    /// has no box.
    fn off(&self) -> Option<[f64; 2]> {
        let at = |value: &Value, index: usize| value.get(index)?.as_f64();
        Some([
            at(&self.page, 0)? - at(&self.ghost, 0)?,
            at(&self.page, 1)? - at(&self.ghost, 1)?,
        ])
    }

    /// Whether the page draws it where the ghost did: both or neither drew it, as wide, within
    /// [`SLACK`].
    fn kept(&self) -> bool {
        match self.off() {
            Some([down, wider]) => down.abs() <= SLACK && wider.abs() <= SLACK,
            None => self.ghost.is_null() && self.page.is_null(),
        }
    }
}

/// One visit to `page`: its ghost drawn while its reads are held, measured, then the page once
/// they are in and it is still.
fn visit(site: &Site, browser: &mut Chrome, page: &Page) -> Vec<Region> {
    for route in page.held {
        site.hold_route(route, true);
    }
    browser.open(&site.url(page.path));
    let drawn = browser.wait_for(page.ghost, SHOWN);
    let ghost = browser.eval(&boxes(page, 1));
    for route in page.held {
        site.hold_route(route, false);
    }
    assert!(drawn, "{}: no ghost was drawn", page.name);
    assert!(
        browser.wait_for(page.page, SHOWN),
        "{}: the page never replaced its ghost: {}",
        page.name,
        browser.eval("[...document.querySelectorAll('main > *, main > * > *, main .settle, main .settle > *')].map(n => n.tagName + '.' + n.className + JSON.stringify({...n.dataset})).join(' | ')")
    );
    browser.eval(layout_shift::SETTLED);
    let drawn = browser.eval(&boxes(page, 2));
    page.regions
        .iter()
        .map(|&(name, _, _)| Region {
            name,
            ghost: ghost[name].clone(),
            page: drawn[name].clone(),
        })
        .collect()
}

/// The site, with the board, the queue and the first task shaped as the live ones, and a fresh
/// browser at `width` on it.
fn browser_at(width: layout_shift::Width) -> Option<(Site, Chrome)> {
    let mut browser = chrome()?;
    Some((site_at(&mut browser, width), browser))
}

/// The site of [`browser_at`], and `browser` at `width` for it: a page as a launch leaves it (a
/// fresh browser's, or a fresh context's), which draws at that width, records its shifts and
/// keeps no store.
fn site_at(browser: &mut Chrome, width: layout_shift::Width) -> Site {
    let site = layout_shift::site(
        "docs/PROBE.md",
        &layout_shift::document_sections(),
        json!([]),
    );
    site.set_json("/api/task-board", &board());
    site.set_json("/api/queue", &queue());
    board_cards(&site);
    browser.viewport(width.width, width.height, width.mobile);
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": layout_shift::RECORD }),
    );
    // A ghost shows on a visit this device has no copy of the page for: its first, or one whose
    // cache opened late or let it go. With the store off every visit is such a one.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": NO_STORE }),
    );
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": NO_SCROLLBARS }),
    );
    site
}

/// A page script that takes every scrollbar out of layout, so each region is as wide as the
/// window lets it be whichever scrollbar Chrome draws: a classic one takes 15 px of every part's
/// width and an overlay one none, and a fresh browser can draw its ghost under one kind and its
/// page under the other, which a stable gutter cannot hold (an overlay scrollbar has none).
/// `layout_shift` keeps the real scrollbars, gutter and all.
const NO_SCROLLBARS: &str = "(() => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('* { scrollbar-width: none !important }');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
})();";

/// A visit's line: how far each region of `regions` stands off its ghost.
fn line(page: &Page, width: &layout_shift::Width, visit: &str, regions: &[Region]) -> String {
    let each: Vec<String> = regions
        .iter()
        .map(|region| match region.off() {
            Some([down, wider]) if wider.abs() > 0.5 => {
                format!("{} {down:+} px, {wider:+} px wide", region.name)
            }
            Some([down, _]) => format!("{} {down:+} px", region.name),
            None => format!(
                "{} ghost {} page {}",
                region.name, region.ghost, region.page
            ),
        })
        .collect();
    format!(
        "LANDMARKS {} {} {visit}: {}",
        page.name,
        width.name,
        each.join(", ")
    )
}

/// The names of the regions of `regions` the page drew elsewhere than the ghost, of `known`.
fn moved(regions: &[Region], known: &[&str]) -> Vec<&'static str> {
    regions
        .iter()
        .filter(|region| known.contains(&region.name) && !region.kept())
        .map(|region| region.name)
        .collect()
}

/// `page`'s regions at each width, from a fresh browser: on a first visit those of `first`'s
/// names are where the ghost put them; on the next visit those of `next`'s.
fn holds(page: &Page, first: &[&str], next: &[&str]) {
    for width in [layout_shift::PHONE, layout_shift::WIDE] {
        let Some((site, mut browser)) = browser_at(width) else {
            return;
        };
        let cold = visit(&site, &mut browser, page);
        let warm = visit(&site, &mut browser, page);
        let report = [
            line(page, &width, "first", &cold),
            line(page, &width, "next", &warm),
        ]
        .join("\n");
        eprintln!("{report}");
        assert_eq!(
            (moved(&cold, first), moved(&warm, next)),
            (Vec::new(), Vec::new()),
            "the page drew these regions elsewhere than its ghost on the first and the next visit:\n{report}"
        );
    }
}

const ALL_TASK: [&str; 5] = ["head", "gates", "timeline", "brief", "thread"];

/// A task opened for the first time in a browser that drew none takes the ghost's guesses; one
/// opened after another task takes that task's sizes; opened again, its own. Each of the two
/// tasks is opened both ways first. Its head is where its ghost put it on every visit, and on
/// the next visit every part.
#[test]
fn a_task_draws_its_head_where_its_ghost_did_and_on_the_next_visit_every_part() {
    let Some(mut browser) = chrome() else { return };
    for width in [layout_shift::PHONE, layout_shift::WIDE] {
        // A browser that drew none is a context of its own, on a site of its own.
        browser.fresh_context();
        let site = site_at(&mut browser, width);
        skeleton_loading(&site);
        let loading_guessed = visit(&site, &mut browser, &LOADING);
        board_cards(&site);
        let gates_alike = visit(&site, &mut browser, &GATES);
        browser.fresh_context();
        let site = site_at(&mut browser, width);
        let gates_guessed = visit(&site, &mut browser, &GATES);
        skeleton_loading(&site);
        let loading_alike = visit(&site, &mut browser, &LOADING);
        let again = visit(&site, &mut browser, &LOADING);
        let report = [
            line(&GATES, &width, "first, nothing kept", &gates_guessed),
            line(&GATES, &width, "first, after the other task", &gates_alike),
            line(&LOADING, &width, "first, nothing kept", &loading_guessed),
            line(
                &LOADING,
                &width,
                "first, after the other task",
                &loading_alike,
            ),
            line(&LOADING, &width, "next", &again),
        ]
        .join("\n");
        eprintln!("{report}");
        let first = [
            &gates_guessed,
            &gates_alike,
            &loading_guessed,
            &loading_alike,
        ];
        assert_eq!(
            (
                first.map(|visit| moved(visit, &["head"])),
                moved(&again, &ALL_TASK)
            ),
            ([(); 4].map(|()| Vec::new()), Vec::new()),
            "the page drew these regions elsewhere than its ghost:\n{report}"
        );
    }
}

/// The board's filters and the top of its pipeline are where its ghost put them on every visit;
/// on the next, its columns' cards and the row under the pipeline too.
#[test]
fn the_board_draws_its_filters_and_pipeline_where_its_ghost_did_and_next_its_cards() {
    holds(
        &BOARD,
        &["filters", "kanban", "pipeline", "column"],
        &[
            "filters",
            "kanban",
            "pipeline",
            "column",
            "fourth card",
            "wait strip",
        ],
    );
}

#[test]
fn a_document_draws_its_head_outline_and_body_where_its_ghost_did() {
    let all = ["head", "outline", "body"];
    holds(&DOCUMENT, &all, &all);
}

/// The queue's first group, its chips and first row are where its ghost put them on every visit;
/// on the next, the groups under it too.
#[test]
fn the_queue_draws_its_first_group_where_its_ghost_did_and_next_every_group() {
    holds(
        &QUEUE,
        &["group", "chips", "row"],
        &["group", "chips", "row", "second group", "third group"],
    );
}

/// A page script under which every `IntersectionObserver` callback comes 800 ms late, as on a
/// loaded machine or a slow phone, so a page's rows draw well after its data. `window.__kept`
/// records, `[ms, what, …]`, each change in how many regions wait for their read, and each save
/// to the browser's storage of the lines the regular expression `kept` (JavaScript) finds.
fn late_rows(kept: &str) -> String {
    format!(
        r#"window.__kept = [];
const Observer = window.IntersectionObserver;
window.IntersectionObserver = class extends Observer {{
    constructor(callback, options) {{ super((entries, observer) => setTimeout(() => callback(entries, observer), 800), options); }}
}};
const setItem = Storage.prototype.setItem;
Storage.prototype.setItem = function (key, value) {{
    const lines = String(value).split('\n').filter(line => {kept}.test(line));
    if (lines.length) window.__kept.push([Math.round(performance.now()), 'kept', lines.join(' | ')]);
    return setItem.call(this, key, value);
}};
let pending = -1;
(function frame() {{
    const waiting = document.querySelectorAll('[data-pending=true]').length;
    if (waiting !== pending) {{ pending = waiting; window.__kept.push([Math.round(performance.now()), 'pending', waiting]); }}
    requestAnimationFrame(frame);
}})();"#
    )
}

/// `page`'s regions at each width, from a fresh browser whose rows draw late ([`late_rows`]): on
/// the next visit those of `next`'s names are where the ghost put them. A failure says what the
/// first visit kept (the lines `kept` finds) and when its regions drew.
fn holds_with_rows_late(page: &Page, kept: &str, next: &[&str]) {
    for width in [layout_shift::PHONE, layout_shift::WIDE] {
        let Some((site, mut browser)) = browser_at(width) else {
            return;
        };
        browser.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": late_rows(kept) }),
        );
        let cold = visit(&site, &mut browser, page);
        let trace = browser.eval("JSON.stringify(window.__kept)");
        let warm = visit(&site, &mut browser, page);
        let report = [
            line(page, &width, "first, rows late", &cold),
            line(page, &width, "next, rows late", &warm),
        ]
        .join("\n");
        eprintln!("{report}");
        assert_eq!(
            moved(&warm, next),
            Vec::<&str>::new(),
            "the next visit drew these regions elsewhere than its ghost:\n{report}\nthe first visit's regions waiting and lines kept, [ms, what, …]: {trace}"
        );
    }
}

/// A board whose rows draw late, well after its data, keeps the cards they draw: on the next
/// visit its ghost puts every region, the fourth card among them, where the board does.
#[test]
fn a_board_whose_rows_draw_late_keeps_them_for_its_next_ghost() {
    holds_with_rows_late(
        &BOARD,
        r"/^\d+ concept( |$)/",
        &[
            "filters",
            "kanban",
            "pipeline",
            "column",
            "fourth card",
            "wait strip",
        ],
    );
}

/// A queue whose rows draw late keeps each group's height as they drew it: on the next visit its
/// ghost puts every group where the queue does.
#[test]
fn a_queue_whose_rows_draw_late_keeps_its_groups_for_its_next_ghost() {
    holds_with_rows_late(
        &QUEUE,
        r"/ queue:/",
        &["group", "chips", "row", "second group", "third group"],
    );
}
