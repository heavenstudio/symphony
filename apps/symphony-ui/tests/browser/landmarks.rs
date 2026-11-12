//! Where each region a page's ghost outlines stands, against where the page draws it once its
//! reads are in: Chrome's layout-shift score cannot see it, as each real element is a new one in
//! the ghost's place. On a first visit a ghost knows only what every such page shows; on the next
//! one this browser kept what the last one drew (the board's columns and counts, a task's head,
//! brief and gates), and every region is where the ghost put it.
use super::{SHOWN, chrome, chrome::Chrome, layout_shift, site::Site};
use serde_json::{Value, json};

/// The most a region may stand off the ghost's box for it, in px: a height kept rounded.
const SLACK: f64 = 2.0;

/// A page whose ghost shows while `held` are read: `ghost` once it is drawn, `page` once the page
/// is, and each region by its name, the ghost's box for it and the page's.
struct Page {
    name: &'static str,
    path: &'static str,
    held: &'static [&'static str],
    ghost: &'static str,
    page: &'static str,
    regions: &'static [(&'static str, &'static str, &'static str)],
}

const TASK: Page = Page {
    name: "task",
    path: "/task/visual/in-progress",
    held: &["/api/task-page"],
    ghost: "document.querySelector('main .ghost-head') !== null",
    page: "document.querySelector('article.task .settle[data-settled=true] .history') !== null && document.querySelector('.task-gates[data-gates=read]') !== null",
    regions: &[
        ("head", "main .ghost-head", "article.task > .task-head"),
        ("gates", "main .task-gates", "article.task .task-gates"),
        ("brief", "main .ghost-brief", "article.task > .task-body"),
        ("thread", "main .ghost-heading", "article.task > h2"),
    ],
};

const BOARD: Page = Page {
    name: "tasks",
    path: "/tasks",
    held: &["/api/task-board"],
    ghost: "document.querySelector('.ghost-pipeline') !== null",
    page: "document.querySelector('.kanban .taskcard') !== null",
    regions: &[
        ("filters", "main .filters", "main .filters"),
        ("kanban", ".ghost-kanban", ".kanban"),
        ("pipeline", ".ghost-pipeline", ".board-scroll"),
        (
            "column",
            ".ghost-pipeline > .ghost-column",
            ".pipeline > .column",
        ),
        ("offline", ".ghost-offline", ".offline"),
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
    page: "document.querySelector('[data-ready=queue] #you') !== null",
    regions: &[
        ("group", ".ghost-group", "#you"),
        ("chips", ".ghost-chips", "#you .queue-categories"),
        ("row", ".ghost-group .ghost-row", "#you .queue-row"),
    ],
};

/// Each region's box, `[top in the page, width]`, by the `index`th selector of its entry
/// (1 the ghost's, 2 the page's); null for one not drawn or out of the flow.
fn boxes(page: &Page, index: usize) -> String {
    let selectors: Vec<Value> = page
        .regions
        .iter()
        .map(|region| json!([region.0, if index == 1 { region.1 } else { region.2 }]))
        .collect();
    format!(
        "Object.fromEntries({}.map(([name, css]) => {{ const node = document.querySelector(css), box = node?.getBoundingClientRect();
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
        "{}: the page never replaced its ghost",
        page.name
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

/// The first and the next visit to `page` at `width`, from a fresh profile; and their lines for
/// the task's thread.
fn visits(page: &Page, width: layout_shift::Width) -> Option<([Vec<Region>; 2], String)> {
    let site = layout_shift::site(
        "docs/PROBE.md",
        &layout_shift::document_sections(),
        json!([]),
    );
    let mut browser = chrome()?;
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width.width, "height": width.height, "deviceScaleFactor": 1, "mobile": width.mobile}),
    );
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": layout_shift::RECORD }),
    );
    let first = visit(&site, &mut browser, page);
    let next = visit(&site, &mut browser, page);
    let mut lines = Vec::new();
    for (kind, regions) in [("first", &first), ("next", &next)] {
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
        lines.push(format!(
            "LANDMARKS {} {} {kind}: {}",
            page.name,
            width.name,
            each.join(", ")
        ));
    }
    let report = lines.join("\n");
    eprintln!("{report}");
    Some(([first, next], report))
}

/// The names of the regions of `regions` the page drew elsewhere than the ghost.
fn moved(regions: &[Region]) -> Vec<&'static str> {
    regions
        .iter()
        .filter(|region| !region.kept())
        .map(|region| region.name)
        .collect()
}

/// `page`'s regions at each width: on a first visit those of `first`'s names are where the ghost
/// put them; on the next visit those of `next`'s.
fn holds(page: &Page, first: &[&str], next: &[&str]) {
    for width in [layout_shift::PHONE, layout_shift::WIDE] {
        let Some(([cold, warm], report)) = visits(page, width) else {
            return;
        };
        let wrong = |regions: &[Region], known: &[&str]| {
            moved(regions)
                .into_iter()
                .filter(|name| known.contains(name))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            (wrong(&cold, first), wrong(&warm, next)),
            (Vec::new(), Vec::new()),
            "the page drew these regions elsewhere than its ghost on the first and the next visit:\n{report}"
        );
    }
}

#[test]
fn a_task_draws_its_head_where_its_ghost_did_and_on_the_next_visit_every_part() {
    holds(&TASK, &["head"], &["head", "gates", "brief", "thread"]);
}

#[test]
fn the_board_draws_its_filters_and_pipeline_where_its_ghost_did() {
    holds(
        &BOARD,
        &["filters", "kanban", "pipeline", "column"],
        &["filters", "kanban", "pipeline", "column"],
    );
}

#[test]
fn a_document_draws_its_head_outline_and_body_where_its_ghost_did() {
    let all = ["head", "outline", "body"];
    holds(&DOCUMENT, &all, &all);
}

#[test]
fn the_queue_draws_its_first_group_its_chips_and_first_row_where_its_ghost_did() {
    let all = ["group", "chips", "row"];
    holds(&QUEUE, &all, &all);
}
