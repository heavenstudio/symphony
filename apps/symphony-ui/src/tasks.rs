//! Task views and their local state. Drafts belong to the composer, never to feed snapshots.

use crate::machine_work::{Scope, SharedWork, phases_view};
use crate::viewport::NearViewport as Near;
use dioxus::prelude::*;
use std::cmp::Ordering;
use std::rc::Rc;
use symphony_api::{Role, Task, TaskFields, TaskStage, slug_order};

/// Both of the board's task orders have this one type, so the board compiles one sort for them.
type TaskOrder = fn(&&Task, &&Task) -> Ordering;

/// Open work: the most urgent first, then by slug, numbers by their value (`ready-3` before
/// `ready-30`), as the server pages a column.
fn by_priority(a: &&Task, b: &&Task) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then_with(|| slug_order(&a.slug, &b.slug))
}

/// Finished work: the most recently updated first, then by slug as open work is.
fn by_recent(a: &&Task, b: &&Task) -> Ordering {
    b.updated
        .total_cmp(&a.updated)
        .then_with(|| slug_order(&a.slug, &b.slug))
}

mod card_height;
use card_height::{Layout, row_height};

/// What a column waiting for its page reserves each of the page's rows, whose tasks it does not
/// know yet: a typical card, its slug, a title of two lines, a role chip and a link chip
/// (`row_height`), near the median of a live board's cards (142 px).
const ROW: u32 = 139;

/// The rows of a column's page, as the server pages it (`api/task_board.rs`), and the height of
/// the Load more button under a column with more.
const PAGE: usize = 30;
const MORE: u32 = 40;

pub fn merge(task: &mut Task, fields: &TaskFields) {
    macro_rules! field {
        ($($name:ident),*) => {$(if let Some(value) = &fields.$name { task.$name = value.clone(); })*};
    }
    field!(title, body, status, links, depends, roles);
    if let Some(value) = fields.priority {
        task.priority = value;
    }
    if let Some(value) = fields.updated {
        task.updated = value;
    }
    if let Some(value) = fields.instruction {
        task.instruction = value;
    }
    if let Some(parent) = &fields.parent {
        task.parent = (!parent.is_empty()).then(|| parent.clone());
    }
}

#[component]
pub fn TaskBoard(
    tasks: Vec<Task>,
    #[props(default)] work: SharedWork,
    #[props(default = true)] connected: bool,
    signoff: Vec<String>,
    counts: Vec<(String, usize)>,
    statuses: Vec<String>,
    refreshed: Signal<u64>,
    onfilter: EventHandler<(String, u8)>,
    onload: EventHandler<(String, bool, String, u8)>,
) -> Element {
    let mut search = use_signal(String::new);
    let mut status = use_signal(String::new);
    let mut days = use_signal(|| 1_u8);
    let mut subtree = use_signal(|| false);
    let toggle = if subtree() { "Hide" } else { "Show" };
    let search_input = crate::entry_events::use_value("oninput", move |query| {
        search.set(query.clone());
        onfilter.call((query, days()));
    });
    let query = search();
    let selected = status();
    let mut statuses: Vec<String> = if statuses.is_empty() {
        counts.iter().map(|(s, _)| s.clone()).collect()
    } else {
        statuses
    };
    // With no machine reporting this project's work, nothing can place a task in Build or Test:
    // one narrow rail says so once, where two empty columns would each repeat it.
    let unreported = work.machines.is_empty();
    let stages = [TaskStage::Build, TaskStage::Test].map(TaskStage::as_str);
    if unreported {
        statuses.retain(|s| !stages.contains(&s.as_str()));
    } else {
        with_machine_stages(&mut statuses);
    }
    let mut filtered: Vec<_> = tasks
        .iter()
        .filter(|task| {
            (task.is_task() || !query.is_empty())
                && (selected.is_empty() || work.task_stage(task, connected).as_str() == selected)
        })
        .collect();
    if query.is_empty() {
        filtered.sort_unstable_by(by_priority as TaskOrder);
    }
    let column = |state: &str| {
        rsx! {
            TaskColumn { work: work.clone(), connected, key: "{state}", status: state.to_string(), tasks: filtered.iter().filter(|t| work.task_stage(t, connected).as_str() == state && !signoff.contains(&t.slug)).map(|t| (*t).clone()).collect(), count: counts.iter().find(|(s,_)| s == state).map_or(0, |(_,n)| *n), query: search, days, onload, refreshed }
        }
    };
    rsx! {
        div { class: "row filters",
            label { "Search tasks and messages " crate::components::input::Input { r#type: "search", placeholder: "Slug or words", value: "{search}", actions: search_input } }
            label { "Status " crate::components::select::SelectField {
                label: "Status", value: status(),
                options: std::iter::once((String::new(), "All statuses".to_string())).chain(statuses.iter().map(|state| (state.clone(), status_label(state)))).collect::<Vec<_>>(),
                on_change: move |value| status.set(value),
            } }
            label { "Finished tasks " crate::components::select::SelectField {
                label: "Finished tasks", value: days().to_string(),
                options: vec![("1".into(), "Last 24 hours".into()), ("7".into(), "Last week".into())],
                on_change: move |value| {
                    let window = if value == "7" { 7 } else { 1 };
                    days.set(window);
                    onfilter.call((search(), window));
                },
            } }
        }
        div { class: "kanban",
            if !signoff.is_empty() {
                TaskColumn { work: work.clone(), connected, status: "signoff".to_string(), tasks: filtered.iter().filter(|t| signoff.contains(&t.slug)).map(|t| (*t).clone()).collect(), count: signoff.len(), query: search, days, onload, refreshed }
            }
            if selected.is_empty() {
                p { id: "board-scroll-hint", class: "board-scroll-hint muted", "Columns continue right → · Swipe, scroll, or focus the board and use arrow keys." }
            }
            if selected.is_empty() || TaskStage::PIPELINE.iter().any(|stage| stage.as_str() == selected) {
            div { class: "board-scroll", tabindex: "0", role: "region", aria_label: "Task pipeline columns", aria_describedby: if selected.is_empty() { "board-scroll-hint" } else { "" },
                div { class: "pipeline",
                    for stage in TaskStage::PIPELINE {
                        if unreported && stage == TaskStage::Build && (selected.is_empty() || stages.contains(&selected.as_str())) {
                            {unreported_rail(connected)}
                        }
                        if statuses.iter().any(|s| s == stage.as_str()) && (selected.is_empty() || selected == stage.as_str()) {
                            {column(stage.as_str())}
                        }
                    }
                }
            }
            }
                div { class: "offline",
                    for state in statuses.iter().filter(|s| ["backlog", "blocked", "cancelled"].contains(&s.as_str())) {
                        if selected.is_empty() || selected == *state {
                            {column(state)}
                        }
                    }
                }
        }
        section { class: "task-groups", aria_labelledby: "task-groups-title",
            h2 { id: "task-groups-title", "Task groups" }
            crate::components::button::Button { aria_expanded: subtree(), "aria-controls": "task-groups", actions: crate::entry_events::click(move || subtree.set(!subtree())),
                "{toggle} task groups"
            }
            if subtree() {
                TreePanel { known: Rc::new(tasks) }
            }
        }
    }
}

/// A status as the Status filter names it, a sentence's first word beside "All statuses":
/// `in_progress` reads "In progress".
fn status_label(status: &str) -> String {
    let words = status.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `statuses` with Build and Test where the board draws their columns: after In progress, Build
/// before Test. One already named is moved there; with no In progress they go last.
fn with_machine_stages(statuses: &mut Vec<String>) {
    let stages = [TaskStage::Build, TaskStage::Test].map(TaskStage::as_str);
    statuses.retain(|status| !stages.contains(&status.as_str()));
    let at = statuses
        .iter()
        .position(|status| status == TaskStage::Lifecycle("in_progress").as_str())
        .map_or(statuses.len(), |at| at + 1);
    statuses.splice(at..at, stages.map(String::from));
}

/// Build and Test of a project no machine reports for: one narrow rail where their columns go.
fn unreported_rail(connected: bool) -> Element {
    rsx! { section { class: "column rail", "data-status": "build-test", aria_label: "Build and test",
        h2 { "build · test" }
        p { class: "muted work-empty",
            if connected { "No machine reports this project's builds or tests." } else { "Disconnected · build and test activity unknown" }
        }
        a { href: crate::project::href("/agents#machine-context"), "Machine details" }
    } }
}

#[component(lazy)]
fn TreePanel(known: Rc<Vec<Task>>) -> Element {
    let roots = use_signal(Vec::<Task>::new);
    let total = use_signal(|| 0_usize);
    let cursor = use_signal(|| None::<String>);
    let trouble = use_signal(String::new);
    use_hook(|| {
        load_tree(None, false, roots, total, cursor, trouble);
    });
    rsx! {
        div { id: "task-groups",
            // `trouble::Trouble`'s markup for a part of a page, written out: the component would add
            // to the board module's first load, which only this block of it would use.
            if !trouble.read().is_empty() {
                section { class: "trouble card", role: "status",
                    h3 { "The task groups could not be loaded" }
                    p { "{crate::trouble::failed(&trouble())}" }
                    p { class: "trouble-actions",
                        crate::components::button::Button { r#type: "button", actions: crate::entry_events::click(move || load_tree(None, false, roots, total, cursor, trouble)), "Retry" }
                    }
                }
            }
            ul { for task in roots() { TreeNode { key: "{task.slug}", task, known: known.clone() } } }
            if roots.read().len() < total() { crate::components::button::Button { actions: crate::entry_events::click(move || load_tree(None,true,roots,total,cursor,trouble)), "Load more task groups" } }
        }
    }
}

fn load_tree(
    parent: Option<String>,
    more: bool,
    mut rows: Signal<Vec<Task>>,
    mut total: Signal<usize>,
    mut cursor: Signal<Option<String>>,
    mut trouble: Signal<String>,
) {
    #[cfg(target_arch = "wasm32")]
    spawn(async move {
        let after = if more { cursor() } else { None };
        let after = crate::task_live::cursor_query(after.as_deref());
        let selection = parent.as_ref().map_or_else(
            || "roots=1".into(),
            |p| {
                format!(
                    "parent={}",
                    js_sys::encode_uri_component(p)
                        .as_string()
                        .unwrap_or_default()
                )
            },
        );
        match crate::task_live::get::<symphony_api::TaskListing>(&format!(
            "/api/task-board?{selection}&older=1{after}"
        ))
        .await
        {
            Ok(list) => {
                trouble.set(String::new());
                cursor.set(list.next);
                total.set(list.counts.iter().map(|(_, n)| n).sum());
                if !more {
                    rows.set(Vec::new());
                }
                for task in list.tasks {
                    if !rows.read().iter().any(|t| t.slug == task.slug) {
                        rows.write().push(task);
                    }
                }
            }
            Err(err) => trouble.set(err),
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (
            parent,
            more,
            &mut rows,
            &mut total,
            &mut cursor,
            &mut trouble,
        );
    }
}

#[component]
fn TreeNode(task: Task, known: Rc<Vec<Task>>) -> Element {
    let shown = known
        .iter()
        .find(|t| t.slug == task.slug)
        .cloned()
        .unwrap_or(task);
    let mut open = use_signal(|| false);
    let children = use_signal(Vec::<Task>::new);
    let total = use_signal(|| 0_usize);
    let cursor = use_signal(|| None::<String>);
    let trouble = use_signal(String::new);
    let slug = shown.slug.clone();
    let more_slug = shown.slug.clone();
    rsx! { Near { id: "tree-{shown.slug}", height: 40, list_item: true,
        a { href: crate::project::href(&format!("/task/{}", shown.slug)), "{shown.title}" } " " span { class: "pill", "{shown.status.replace('_', \" \")}" }
        crate::components::button::Button { aria_expanded: open(), actions: crate::entry_events::click(move || { if !open() { load_tree(Some(slug.clone()),false,children,total,cursor,trouble); } open.set(!open()); }), "Subtasks" }
        if open() {
            if !trouble.read().is_empty() { p { role: "status", "Its subtasks could not be loaded: {trouble}" } }
            ul { for child in children() { TreeNode { key: "{child.slug}", task: child, known: known.clone() } } }
            if children.read().len() < total() { crate::components::button::Button { actions: crate::entry_events::click(move || load_tree(Some(more_slug.clone()),true,children,total,cursor,trouble)), "Load more subtasks" } }
        }
    } }
}

#[component]
fn TaskColumn(
    status: String,
    work: SharedWork,
    connected: bool,
    tasks: Vec<Task>,
    count: usize,
    query: Signal<String>,
    days: Signal<u8>,
    onload: EventHandler<(String, bool, String, u8)>,
    refreshed: Signal<u64>,
) -> Element {
    let mut visible = use_signal(|| false);
    // The layout a row's reserve is worked out for, read once: a window resized later only
    // misjudges rows still waiting.
    let layout = use_hook(Layout::now);
    let requested_status = status.clone();
    // Complete totals make empty-column reads unnecessary. Retained rows still refresh so a
    // reconnect can remove stale cards, and a newly populated column must request its page.
    let needs_page = count > 0 || !tasks.is_empty();
    use_effect(use_reactive!(|needs_page| {
        let query = query();
        let days = days();
        let _ = refreshed();
        if visible() && needs_page {
            onload.call((requested_status.clone(), false, query, days));
        }
    }));
    let mut rows: Vec<_> = tasks.iter().collect();
    if query().is_empty() && (status == "done" || status == "cancelled") {
        rows.sort_unstable_by(by_recent as TaskOrder);
    }
    let more_status = status.clone();
    // Keep unloaded columns in place while their first page is fetched: a card's box for each
    // row of the page, each as tall as this browser last drew the row in its place, and the Load
    // more button a column of more than a page will show. Otherwise revealing an empty column
    // briefly shrinks it, pulling every later column into the viewport too. The board says once
    // that its tasks load (`board_live::TasksPage`).
    let kept = use_hook(|| kept_cards(&status));
    let reserved = first_page_height(count, &kept);
    let reading = count > 0 && tasks.is_empty();
    // Once its cards are drawn on the board as it opens, their heights for the next visit.
    #[cfg(target_arch = "wasm32")]
    {
        let drawn = status.clone();
        let shown = tasks.len();
        use_effect(use_reactive!(|shown| {
            if shown > 0 && query.peek().is_empty() && *days.peek() == 1 {
                remember_cards(drawn.clone());
            }
        }));
    }
    rsx! { section { class: if status == "signoff" { "column signoff" } else { "column" }, "data-status": "{status}", "aria-busy": "{reading}",
        h2 { if status == "signoff" { "Waiting for your sign-off" } else { "{status.replace('_', \" \")}" } " " span { class: "count", "{count}" } }
        if status == "build" { {crate::machine_work::board_context(&work, connected)} }
        if count == 0 && (status == "build" || status == "test") {
            p { class: "muted work-empty", "No tasks with fresh {status} activity." }
        }
        Near { id: "column-{status}", height: reserved, onenter: move |_| visible.set(true), ghost: rsx! { for n in 0..count.min(PAGE) { GhostCard { key: "{n}", card: card(&kept, n, &status) } } },
            if reading { div { style: "min-height:{reserved}px", "data-pending": "true",
                for n in 0..count.min(PAGE) { GhostCard { key: "{n}", card: card(&kept, n, &status) } }
            } }
            // A row waiting to be drawn holds what its own card will take (`row_height`).
            for (task, reserve) in rows.into_iter().map(|task| (task, row_card(task, &work, connected, layout))) { Near { key: "{task.slug}", id: "row-{task.slug}", height: reserve.0, ghost: rsx! { GhostCard { card: reserve } }, TaskRow { task: task.clone(), work: work.clone(), connected } } }
            // Only once the first page is in: while it is read, there is nothing more to load.
            if !tasks.is_empty() && count > tasks.len() {
                crate::components::button::Button { actions: crate::entry_events::click(move || onload.call((more_status.clone(),true,query(),days()))), "Load more tasks" }
            }
        }
    } }
}

/// A card as its column keeps it: its pitch (its height with the space under it) and how many
/// rows of chips it carries (its build and test phases, its linked documents, its roles).
type Kept = (u32, u8);

/// The card a row of `task` waits for, in `layout`, with the build and test phases `work` gives
/// it: as tall as it will draw (`row_height`), and its rows of chips.
fn row_card(task: &Task, work: &SharedWork, connected: bool, layout: Layout) -> Kept {
    let phases = crate::machine_work::phase_labels(work, &task.slug, connected);
    let rows = [
        !phases.is_empty(),
        !task.links.is_empty(),
        !task.roles.is_empty(),
    ];
    (
        row_height(task, &phases, layout),
        rows.into_iter().map(u8::from).sum(),
    )
}

/// Whether the tasks of the column `status` wait for an agent to take them up, so a card guessed
/// for them draws no role chip (docs/DECISIONS.md, skeleton-loading-outline).
fn unstaffed(status: &str) -> bool {
    ["concept", "design", "ready", "backlog"].contains(&status)
}

/// The card in the place `slot` of the column `status`: as `kept` has it, else as tall as the
/// typical card [`ROW`] stands for, with its row of linked documents and, unless no agent has
/// taken the column's tasks up, its row of roles.
fn card(kept: &[Kept], slot: usize, status: &str) -> Kept {
    kept.get(slot)
        .copied()
        .unwrap_or((ROW, if unstaffed(status) { 1 } else { 2 }))
}

/// What a column of `count` tasks takes before its first page is in: a card for each row of the
/// page ([`card`], [`PAGE`] at most), and the Load more button when they are more than a page.
fn first_page_height(count: usize, kept: &[Kept]) -> u32 {
    let cards = (0..count.min(PAGE))
        .map(|slot| kept.get(slot).map_or(ROW, |card| card.0))
        .sum();
    if count > PAGE { cards + MORE } else { cards }
}

/// The space under a card.
const GAP: u32 = 8;

/// Where this browser keeps the cards of each column of the board as it opens, as drawn last,
/// newest first, as lines of `WIDTH STATUS PITCH...`, a pitch followed by a `c` for each row of
/// chips its card carries.
const CARDS: &str = "board-cards";
/// How many columns' cards a browser keeps, at every width.
#[cfg(any(target_arch = "wasm32", test))]
const CARDS_MAX: usize = 32;

/// The cards `kept` (as [`CARDS`] holds them) gives the column `status` in a window `width` px
/// wide.
fn recall_cards(kept: &str, status: &str, width: u32) -> Vec<Kept> {
    let width = width.to_string();
    kept.lines()
        .find_map(|line| {
            let mut parts = line.split(' ');
            (parts.next()? == width && parts.next()? == status).then_some(parts)
        })
        .map(|parts| {
            parts
                .map_while(|pitch| {
                    let height = pitch.trim_end_matches('c');
                    let chips = u8::try_from(pitch.len() - height.len()).ok()?;
                    Some((height.parse().ok()?, chips))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `kept` with `cards` as the column `status`'s in a window `width` px wide, first, its line
/// before and the oldest past [`CARDS_MAX`] dropped.
#[cfg(any(target_arch = "wasm32", test))]
fn keep_cards(kept: &str, status: &str, width: u32, cards: &[Kept]) -> String {
    let mut line = format!("{width} {status}");
    for (pitch, chips) in cards {
        line += &format!(" {pitch}{}", "c".repeat(usize::from(*chips)));
    }
    let own = format!("{width} {status} ");
    std::iter::once(line.as_str())
        .chain(
            kept.lines()
                .filter(|old| !format!("{old} ").starts_with(&own)),
        )
        .take(CARDS_MAX)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The cards this browser kept for the column `status` in a window as wide as this one.
fn kept_cards(status: &str) -> Vec<Kept> {
    crate::platform::current()
        .load(CARDS)
        .map(|kept| recall_cards(&kept, status, crate::viewport::window_width()))
        .unwrap_or_default()
}

/// What a column keeps of the rows of its page as they stand, having kept `saved`: each row's
/// pitch, its card's as drawn or, for a row still waiting, the place it holds (`row_card`), so
/// the next visit's ghost stands where this visit's rows do; none when that is what it kept.
#[cfg(any(target_arch = "wasm32", test))]
fn keep_rows(rows: &[Kept], saved: &[Kept]) -> Option<Vec<Kept>> {
    (!rows.is_empty() && rows != saved).then(|| rows.to_vec())
}

/// Whether a column's keep watches on, `waited` ms in: until every row has drawn its card
/// (`drawn`), or no row still waiting lies within the sentinel's reach of the screen (`near`
/// false: what is about to draw has), or `viewport::KEEP_MS` has passed, a bound on the work
/// alone. A row near the screen draws when its sentinel says so, which on a loaded machine or a
/// slow phone comes many seconds after the column's data.
#[cfg(any(target_arch = "wasm32", test))]
fn keeps_watching(drawn: bool, near: bool, waited: f64) -> bool {
    !drawn && near && waited < crate::viewport::KEEP_MS
}

/// Keep the rows of the column `status` as they draw, its page's at most: every 100 ms, each
/// row's pitch (`drawn_rows`), again whenever one changed (`keep_rows`), while `keeps_watching`.
#[cfg(target_arch = "wasm32")]
fn remember_cards(status: String) {
    spawn(async move {
        let mut saved = Vec::new();
        let mut waited = 0.0;
        loop {
            crate::live::sleep(100).await;
            waited += 100.0;
            let Some(column) = board_column(&status) else {
                return;
            };
            let (rows, drawn) = drawn_rows(&column);
            if let Some(cards) = keep_rows(&rows, &saved) {
                let platform = crate::platform::current();
                let kept = platform.load(CARDS).unwrap_or_default();
                platform.save(
                    CARDS,
                    &keep_cards(&kept, &status, crate::viewport::window_width(), &cards),
                );
                saved = cards;
            }
            let near = crate::viewport::waits_near(&column, Some(".board-scroll"));
            if !keeps_watching(drawn, near, waited) {
                return;
            }
        }
    });
}

/// The board's column `status` on the page, if it still is.
#[cfg(target_arch = "wasm32")]
fn board_column(status: &str) -> Option<web_sys::Element> {
    let selector = format!(".kanban .column[data-status=\"{status}\"]");
    web_sys::window()?
        .document()?
        .query_selector(&selector)
        .ok()?
}

/// The rows of `column`'s page as they stand now, each its pitch and its rows of chips: a drawn
/// card's, else the place the row holds and its ghost's chips; and whether every row has drawn
/// its card.
#[cfg(target_arch = "wasm32")]
fn drawn_rows(column: &web_sys::Element) -> (Vec<Kept>, bool) {
    use wasm_bindgen::JsCast;
    let elements = |node: &web_sys::Element, selector: &str| -> Vec<web_sys::Element> {
        let Ok(found) = node.query_selector_all(selector) else {
            return Vec::new();
        };
        (0..found.length())
            .filter_map(|index| found.item(index)?.dyn_into::<web_sys::Element>().ok())
            .collect()
    };
    let height = |node: &web_sys::Element| node.get_bounding_client_rect().height().round() as u32;
    let count = |found: usize| u8::try_from(found).unwrap_or(u8::MAX);
    let mut drawn = true;
    let rows = elements(column, "[id^=\"row-\"]")
        .into_iter()
        .take(PAGE)
        .map(|row| match row.query_selector(".taskcard").ok().flatten() {
            Some(card) => {
                let chips = elements(&card, ".chips")
                    .iter()
                    .filter(|chips| chips.first_element_child().is_some())
                    .count();
                (height(&card) + GAP, count(chips))
            }
            None => {
                drawn = false;
                (height(&row), count(elements(&row, ".ghost-pill").len()))
            }
        })
        .collect();
    (rows, drawn)
}

/// What a card's box holds besides its title and chips: its border and padding, its slug's line,
/// the space over its title and its two chip rows' margins, in px.
const FRAME: u32 = 52;
/// A row of chips.
const CHIP_ROW: u32 = 21;
/// A line of a card's title.
const TITLE_LINE: u32 = 20;

/// The lines of title `card`'s box has room for, one to five: what its pitch leaves of its frame
/// and chips, with a few px given for a height kept rounded.
fn title_lines((pitch, chips): Kept) -> u32 {
    let room = pitch.saturating_sub(GAP + FRAME + u32::from(chips) * CHIP_ROW);
    ((room + 4) / TITLE_LINE).clamp(1, 5)
}

/// A task's card while it loads: its box as tall as `card`'s pitch less the space under it, a bar
/// for its slug, one for each line of title it has room for and a pill for each row of chips.
#[component]
fn GhostCard(card: Kept) -> Element {
    let (pitch, chips) = card;
    let lines = title_lines(card);
    rsx! { div { class: "ghost-card", style: "height:{pitch.saturating_sub(GAP)}px",
        span { class: "bone" }
        div { for line in 0..lines { span { key: "{line}", class: "bone", style: if line > 0 && line + 1 == lines { "--w:62%" } else { "" } } } }
        for row in 0..chips { span { key: "{row}", class: "ghost-pill", span { class: "bone", style: if row % 2 == 0 { "--w:8ch" } else { "--w:12ch" } } } }
    } }
}

/// The columns the board draws with no filter, in its order, each with its count: Waiting for
/// your sign-off when a task waits for it, the pipeline's (`build-test`, one rail, for Build and
/// Test when no machine `reported` them), then backlog, blocked and cancelled ([`TaskBoard`]).
pub fn board_shape(
    counts: &[(String, usize)],
    statuses: &[String],
    signoff: usize,
    reported: bool,
) -> Vec<(String, usize)> {
    let mut known: Vec<&str> = if statuses.is_empty() {
        counts.iter().map(|(status, _)| status.as_str()).collect()
    } else {
        statuses.iter().map(String::as_str).collect()
    };
    known.retain(|status| reported || !["build", "test"].contains(status));
    let count = |status: &str| {
        (
            status.to_string(),
            counts
                .iter()
                .find(|(s, _)| s == status)
                .map_or(0, |(_, n)| *n),
        )
    };
    let mut shape: Vec<_> = (signoff > 0)
        .then(|| ("signoff".to_string(), signoff))
        .into_iter()
        .collect();
    for stage in TaskStage::PIPELINE.map(TaskStage::as_str) {
        if !reported && stage == "build" {
            shape.push(("build-test".to_string(), 0));
        }
        if known.contains(&stage) || (reported && ["build", "test"].contains(&stage)) {
            shape.push(count(stage));
        }
    }
    shape.extend(
        known
            .iter()
            .filter(|s| OFFLINE.contains(s))
            .map(|s| count(s)),
    );
    shape
}

/// The columns off the pipeline.
const OFFLINE: [&str; 3] = ["backlog", "blocked", "cancelled"];

/// Where this browser keeps the shape of the project's board as last read ([`board_shape`]), as
/// lines of `STATUS COUNT`.
pub const SHAPE: &str = "board-shape";

/// The board's shape for a browser that kept none: the pipeline, Build and Test among it, and the
/// columns off it, a task in each.
fn default_shape() -> Vec<(String, usize)> {
    TaskStage::PIPELINE
        .map(TaskStage::as_str)
        .iter()
        .chain(&OFFLINE)
        .map(|status| (status.to_string(), 1))
        .collect()
}

/// The shape `kept` (as [`SHAPE`] holds it) gives the board; [`default_shape`] for none.
pub fn recall_shape(kept: Option<&str>) -> Vec<(String, usize)> {
    let shape: Vec<_> = kept
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (status, count) = line.split_once(' ')?;
            Some((status.to_string(), count.parse().ok()?))
        })
        .collect();
    if shape.is_empty() {
        default_shape()
    } else {
        shape
    }
}

/// `shape` as [`SHAPE`] keeps it.
pub fn keep_shape(shape: &[(String, usize)]) -> String {
    shape
        .iter()
        .map(|(status, count)| format!("{status} {count}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The task board before its first read: its filters and, in the `shape` it last had, its
/// columns with a card for each task they held, each as this browser last drew it ([`CARDS`]).
#[component]
pub fn GhostBoard(shape: Vec<(String, usize)>) -> Element {
    let column = |(status, count): &(String, usize)| rsx! { GhostColumn { key: "{status}", status: status.clone(), count: *count, cards: kept_cards(status) } };
    rsx! {
        div { class: "row filters",
            div { "Search tasks and messages" span { class: "ghost-field", style: "width:10.5rem" } }
            div { "Status" span { class: "ghost-field", style: "width:7.5rem", span { class: "bone", style: "--w:10ch" } } }
            div { "Finished tasks" span { class: "ghost-field", style: "width:8.125rem", span { class: "bone", style: "--w:11ch" } } }
        }
        // Its own class, laid out as `.kanban` is: a wait for the board never finds its ghost.
        div { class: "ghost-kanban",
            for part in shape.iter().filter(|(status, _)| status == "signoff") { {column(part)} }
            // Two bars that wrap as the hint's words do: one line wide, two on a phone.
            div { class: "ghost-hint", span { class: "bone", style: "--w:38ch" } " " span { class: "bone", style: "--w:26ch" } }
            // It scrolls as the pipeline's scroller does, its scroll bar in the same place; no stop
            // for the keyboard.
            div { class: "ghost-pipeline", tabindex: "-1",
                for part in shape.iter().filter(|(status, _)| status != "signoff" && !OFFLINE.contains(&status.as_str())) { {column(part)} }
            }
            div { class: "ghost-offline",
                for part in shape.iter().filter(|(status, _)| OFFLINE.contains(&status.as_str())) { {column(part)} }
            }
        }
    }
}

/// A column of the board's ghost: its status's colour, a bar for its heading, a card for each of
/// its `count` tasks (a page's at most, as `cards` kept them) and the room of the Load more button
/// past a page, as its column reserves them ([`first_page_height`]); Build and Test's lines of
/// text where they hold none.
#[component]
fn GhostColumn(status: String, count: usize, cards: Vec<Kept>) -> Element {
    // The rail's say of why it is one, then its link; Build's Machine context; the line an empty
    // Build or Test column shows.
    let lines = match status.as_str() {
        "build-test" => 5,
        "build" => 1 + u8::from(count == 0),
        "test" => u8::from(count == 0),
        _ => 0,
    };
    rsx! { div { class: "ghost-column", "data-for": "{status}",
        div { span { class: "bone", style: "--w:9ch" } }
        if lines > 0 { crate::viewport::Skeleton { lines } }
        if status == "build-test" { div { span { class: "bone", style: "--w:12ch" } } }
        div { for n in 0..count.min(PAGE) { GhostCard { key: "{n}", card: card(&cards, n, &status) } }
            if count > PAGE { div { style: "height:{MORE}px" } }
        }
    } }
}

#[component]
pub fn Roles(roles: Vec<Role>) -> Element {
    rsx! { span { class: "chips", for role in roles {
        span { key: "{role.role}:{role.handle}", class: "chip health-{role.health}", title: "{role.role} is {role.health}", "{role.role} {role.handle}" }
    } } }
}

#[component]
fn TaskRow(
    task: Task,
    #[props(default)] work: SharedWork,
    #[props(default = true)] connected: bool,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_effect(crate::live::mark_tasks_ready);
    // The title is the card's one link to its task; the slug above it is text to read and copy.
    rsx! { div { class: "taskcard p{task.priority}", "data-slug": "{task.slug}",
        code { class: "taskcard-slug", "{task.slug}" } " " span { class: "prio", "p{task.priority}" }
        span { class: "task-status st-{task.status}", "{task.status.replace('_', \" \")}" }
        a { class: "taskcard-title", href: crate::project::href(&format!("/task/{}", task.slug)), "{task.title}" }
        {phases_view(&work, &Scope::Task(&task.slug), connected)}
        if let Some(parent) = &task.parent {
            div { class: "subtask-parent", "Subtask of " a { href: crate::project::href(&format!("/task/{parent}")), "{parent}" } }
        }
        div { class: "chips", for link in &task.links { a { class: "docchip", href: crate::project::href(&format!("/{link}")), "{link.rsplit('/').next().unwrap_or(link)}" } } }
        Roles { roles: task.roles }
    } }
}

#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_detail.rs"]
mod detail;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
use crate::timeline_slot as timeline;
#[cfg(all(target_arch = "wasm32", feature = "page-tasks"))]
pub(crate) use detail::LiveTaskPage;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
pub use detail::{AgentsView, TaskDetail};
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
#[path = "task_gates.rs"]
pub mod gates;

/// Whether the card the feed's `pending` region names by `key` (`/docs/X.md#card`) is in a
/// document the task links (`links`, `docs/X.md#part`): one its owner gate may count, so a task
/// page's gates read again when it comes, changes or goes (`task_live::Data::gates_nudged`).
pub fn links_card(links: &[String], key: &str) -> bool {
    let path = key
        .trim_start_matches('/')
        .split('#')
        .next()
        .unwrap_or_default();
    !path.is_empty()
        && links
            .iter()
            .any(|link| link.split('#').next() == Some(path))
}

/// The branch whose new tip changes the gates of the task `slug`: the branch they were last
/// answered for (`answered`, with the task they were answered for) while that is this task, else
/// the slug, the branch the board names a task's work by.
pub fn gates_branch<'a>(slug: &'a str, answered: Option<&'a (String, String)>) -> &'a str {
    match answered {
        Some((task, branch)) if task == slug => branch,
        _ => slug,
    }
}

#[cfg(test)]
mod tests {
    use super::AgentsView;
    use dioxus::prelude::*;
    use serde_json::json;

    /// A server from before the panes probe's rename names it `herdr` alone: the agents page still
    /// shows its answer, under the probe's name.
    #[test]
    fn the_agents_page_shows_the_panes_probe_a_server_named_herdr() {
        fn page() -> Element {
            let agents: symphony_api::Agents = serde_json::from_value(json!({
                "generated": 1.5,
                "probes": {"herdr": "unavailable", "slots": "ok"},
                "agents": [],
            }))
            .unwrap();
            rsx! { AgentsView { agents, tasks: Vec::new(), total: 0, more: false, lost_more: false, onmore: |_| {} } }
        }
        let mut dom = VirtualDom::new(page);
        dom.rebuild_in_place();
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(">Terminal panes: unavailable · Test slots: ok</p>"),
            "{html}"
        );
    }

    #[derive(Clone, PartialEq, Props)]
    struct Board {
        work: crate::machine_work::SharedWork,
        connected: bool,
        statuses: Vec<String>,
    }

    #[expect(non_snake_case, reason = "a Dioxus component")]
    fn BoardPage(board: Board) -> Element {
        let refreshed = use_signal(|| 0_u64);
        rsx! { super::TaskBoard {
            tasks: Vec::new(), work: board.work, connected: board.connected, signoff: Vec::new(),
            counts: Vec::new(), statuses: board.statuses, refreshed, onfilter: |_| {}, onload: |_| {},
        } }
    }

    /// The pipeline's columns, in order, as `data-status` names them.
    fn pipeline(work: symphony_api::MachineWork, connected: bool, statuses: &[&str]) -> String {
        let board = Board {
            work: work.into(),
            connected,
            statuses: statuses.iter().map(|s| (*s).to_string()).collect(),
        };
        let mut dom = VirtualDom::new_with_props(BoardPage, board);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn columns(html: &str) -> Vec<&str> {
        html.split("data-status=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    fn reported() -> symphony_api::MachineWork {
        serde_json::from_value(
            json!({"freshness":"incomplete","unassigned":[],"machines":[{
            "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
            "age_ms":0,"freshness":"incomplete","complete":false,"reason":"","counts":null,
            "operations":[]}]}),
        )
        .unwrap()
    }

    const LIFECYCLE: [&str; 9] = [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
    ];

    /// No machine reports the project's work: Build and Test are one narrow rail, said once, and
    /// a server's listing that names them draws no column of either.
    #[test]
    fn a_project_no_machine_reports_for_draws_one_rail_for_build_and_test() {
        let mut named = LIFECYCLE.to_vec();
        named.extend(["build", "test"]);
        for statuses in [&LIFECYCLE[..], &named[..]] {
            let html = pipeline(symphony_api::MachineWork::default(), true, statuses);
            assert_eq!(
                columns(&html),
                [
                    "concept",
                    "design",
                    "ready",
                    "in_progress",
                    "build-test",
                    "review",
                    "done",
                    "backlog",
                    "blocked",
                    "cancelled"
                ],
                "{html}"
            );
            assert_eq!(
                html.matches("No machine reports this project").count(),
                1,
                "{html}"
            );
            assert!(!html.contains("No tasks with fresh"), "{html}");
            assert!(!html.contains("Machine context"), "{html}");
            assert!(html.contains("/agents#machine-context"), "{html}");
        }
        let html = pipeline(symphony_api::MachineWork::default(), false, &LIFECYCLE);
        assert!(
            html.contains("Disconnected · build and test activity unknown"),
            "{html}"
        );
    }

    /// Once a machine reports, even with nothing going, Build and Test are full columns again.
    #[test]
    fn a_reporting_machine_brings_back_the_build_and_test_columns() {
        let html = pipeline(reported(), true, &LIFECYCLE);
        let columns = columns(&html);
        assert_eq!(
            columns[..8],
            [
                "concept",
                "design",
                "ready",
                "in_progress",
                "build",
                "test",
                "review",
                "done"
            ],
            "{html}"
        );
        assert!(!columns.contains(&"build-test"), "{html}");
        assert_eq!(html.matches("No tasks with fresh").count(), 2, "{html}");
        assert_eq!(html.matches("Machine context").count(), 1, "{html}");
    }

    /// The Status filter names each status as "All statuses" is written: capital first, spaces.
    #[test]
    fn the_status_filter_names_each_status_as_a_sentence_starts() {
        for (status, label) in [
            ("backlog", "Backlog"),
            ("in_progress", "In progress"),
            ("build", "Build"),
            ("a_b_c", "A b c"),
            ("_x", " x"),
            ("élan", "Élan"),
            ("", ""),
        ] {
            assert_eq!(super::status_label(status), label, "{status}");
        }
    }

    /// The Status filter lists Build and Test where the board draws them, between In progress and
    /// Review, whether the server named them (anywhere, even twice) or not; with no In progress
    /// they go last.
    #[test]
    fn the_status_filter_lists_build_and_test_after_in_progress() {
        let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let board = owned(&[
            "backlog",
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done",
            "blocked",
            "cancelled",
        ]);
        let mut named = LIFECYCLE.to_vec();
        named.extend(["build", "test"]);
        let mut twice = owned(&["test", "build"]);
        twice.extend(owned(&LIFECYCLE));
        twice.push("build".into());
        for given in [owned(&LIFECYCLE), owned(&named), twice] {
            let mut statuses = given.clone();
            super::with_machine_stages(&mut statuses);
            assert_eq!(statuses, board, "{given:?}");
        }
        let mut none = owned(&["backlog", "done"]);
        super::with_machine_stages(&mut none);
        assert_eq!(none, owned(&["backlog", "done", "build", "test"]));
        let mut empty = Vec::new();
        super::with_machine_stages(&mut empty);
        assert_eq!(empty, owned(&["build", "test"]));
    }

    /// A card opens its task from its title: the title is the card's one link to the task, and
    /// the slug above it is plain text. The subtask's parent and the linked documents keep theirs.
    #[test]
    fn a_cards_title_is_its_one_link_to_the_task() {
        let task: symphony_api::Task = serde_json::from_value(json!({"slug": "tools/fix-it",
            "title": "Fix the thing", "body": "", "status": "ready", "priority": 2,
            "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
            "links": [], "depends": [], "roles": [], "instruction": false}))
        .unwrap();
        let html = dioxus_ssr::render_element(rsx! { super::TaskRow { task: task.clone() } });
        assert_eq!(html.matches("<a ").count(), 1, "{html}");
        assert!(
            html.contains(
                r#"<a class="taskcard-title" href="/task/tools/fix-it">Fix the thing</a>"#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"<code class="taskcard-slug">tools/fix-it</code>"#),
            "{html}"
        );
        let mut subtask = task;
        subtask.parent = Some("tools/parent".into());
        subtask.links = vec!["docs/TASKS.md".into()];
        let html = dioxus_ssr::render_element(rsx! { super::TaskRow { task: subtask } });
        assert_eq!(
            html.matches(r#"href="/task/tools/fix-it""#).count(),
            1,
            "{html}"
        );
        assert!(html.contains(r#"href="/task/tools/parent""#), "{html}");
        assert!(html.contains(r#"href="/docs/TASKS.md""#), "{html}");
    }

    #[test]
    fn open_work_sorts_by_priority_then_slug_and_finished_work_newest_first_then_slug() {
        let task = |slug: &str, priority: i64, updated: f64| -> symphony_api::Task {
            serde_json::from_value(json!({"slug": slug, "title": slug, "body": "",
                "status": "ready", "priority": priority, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": updated, "links": [], "depends": [], "roles": [],
                "instruction": false}))
            .unwrap()
        };
        let tasks = [
            task("b", 2, 10.0),
            task("c", 1, 30.0),
            task("a", 2, 30.0),
            task("d", 1, 10.0),
        ];
        let slugs = |order: super::TaskOrder| {
            let mut rows: Vec<_> = tasks.iter().collect();
            rows.sort_unstable_by(order);
            rows.iter().map(|t| t.slug.as_str()).collect::<Vec<_>>()
        };
        assert_eq!(slugs(super::by_priority), ["c", "d", "a", "b"]);
        assert_eq!(slugs(super::by_recent), ["a", "c", "b", "d"]);
        // A slug's numbers order by their value, as the server pages a column.
        let tasks = [
            task("ready-30", 3, 10.0),
            task("ready-3", 3, 10.0),
            task("ready-10", 3, 10.0),
            task("ready-9", 3, 10.0),
            task("urgent-40", 1, 10.0),
        ];
        let slugs = |order: super::TaskOrder| {
            let mut rows: Vec<_> = tasks.iter().collect();
            rows.sort_unstable_by(order);
            rows.iter().map(|t| t.slug.as_str()).collect::<Vec<_>>()
        };
        assert_eq!(
            slugs(super::by_priority),
            ["urgent-40", "ready-3", "ready-9", "ready-10", "ready-30"]
        );
        assert_eq!(
            slugs(super::by_recent),
            ["ready-3", "ready-9", "ready-10", "ready-30", "urgent-40"]
        );
    }

    /// A column reserves a card for each row of the page its first read brings, and the Load more
    /// button only when its tasks are more than that page; with nothing kept, each a typical card.
    #[test]
    fn a_column_reserves_a_card_per_row_of_its_page_and_load_more_past_it() {
        use super::{MORE, PAGE, ROW, first_page_height};
        assert_eq!(PAGE, 30, "the server's page of a column");
        assert_eq!(first_page_height(0, &[]), 0);
        assert_eq!(first_page_height(1, &[]), ROW);
        assert_eq!(first_page_height(17, &[]), 17 * ROW);
        assert_eq!(
            first_page_height(30, &[]),
            30 * ROW,
            "one page holds 30: no Load more"
        );
        assert_eq!(first_page_height(31, &[]), 30 * ROW + MORE);
        assert_eq!(first_page_height(usize::MAX, &[]), 30 * ROW + MORE);
    }

    /// Each card a column kept takes its place at its own height; past them, the guess.
    #[test]
    fn a_column_reserves_each_card_it_kept_at_its_height_and_guesses_the_rest() {
        use super::{MORE, ROW, card, first_page_height};
        let kept = [(134, 1), (113, 0), (192, 3)];
        assert_eq!(card(&kept, 1, "ready"), (113, 0));
        assert_eq!(
            card(&kept, 3, "review"),
            (ROW, 2),
            "a link's row and a role's"
        );
        assert_eq!(card(&[], 0, "in_progress"), (ROW, 2));
        for unstaffed in ["concept", "design", "ready", "backlog"] {
            assert_eq!(
                card(&kept, 3, unstaffed),
                (ROW, 1),
                "as tall, with no role chip where no agent has taken the tasks up: {unstaffed}"
            );
        }
        assert_eq!(first_page_height(2, &kept), 134 + 113);
        assert_eq!(first_page_height(5, &kept), 134 + 113 + 192 + 2 * ROW);
        assert_eq!(
            first_page_height(31, &kept),
            134 + 113 + 192 + 27 * ROW + MORE
        );
    }

    /// A row waiting to be drawn holds the height its own card will draw at, chips among it: a
    /// row of them for its links, one for its roles and one for its build and test phases.
    #[test]
    fn a_row_waiting_holds_its_own_card_s_height() {
        use super::{Layout, row_card, row_height};
        let task = |links: serde_json::Value, roles: serde_json::Value| -> symphony_api::Task {
            serde_json::from_value(json!({"slug": "load/ready-1", "title": "Two words",
                "body": "", "status": "ready", "priority": 2, "parent": null,
                "created_by": "owner", "created": 0.0, "updated": 0.0, "links": links,
                "depends": [], "roles": roles, "instruction": false}))
            .unwrap()
        };
        let work = super::SharedWork::default();
        let bare = task(json!([]), json!([]));
        let linked = task(json!(["docs/TASKS.md"]), json!([]));
        let staffed = task(
            json!([]),
            json!([{"role": "executor", "handle": "opal-plover", "since": 0.0, "expires": null, "health": "live"}]),
        );
        let mut both = linked.clone();
        both.roles.clone_from(&staffed.roles);
        // A build running for the task on a machine that reports: a row of phases above the rest.
        let building: symphony_api::MachineWork = serde_json::from_value(json!({
            "freshness": "fresh", "unassigned": [], "machines": [{
                "machine_id": "alpha", "label": "alpha", "producer_id": "fixture", "sequence": 1,
                "age_ms": 0, "freshness": "fresh", "complete": true, "reason": "",
                "counts": {"running_builds": 1, "queued_builds": 0, "running_tests": 0, "queued_tests": 0},
                "operations": [{"machine_id": "alpha", "operation_id": "b", "parent_id": null,
                    "handle": "opal-plover", "task": "load/ready-1", "checkout": "/opaque/checkout",
                    "recipe": "fixture", "kind": "build", "state": "running",
                    "resource": "run_slot", "reason": "", "phase_since": null}]}]}))
        .unwrap();
        let phases = crate::machine_work::phase_labels(&building, "load/ready-1", true);
        assert_eq!(phases.len(), 1, "{phases:?}");
        let building = super::SharedWork::from(building);
        for layout in [Layout::Desk, Layout::Phone] {
            assert_eq!(
                row_card(&both, &building, true, layout),
                (row_height(&both, &phases, layout), 3)
            );
            assert_eq!(
                row_card(&bare, &work, true, layout),
                (row_height(&bare, &[], layout), 0)
            );
            assert_eq!(
                row_card(&linked, &work, true, layout),
                (row_height(&linked, &[], layout), 1)
            );
            assert_eq!(
                row_card(&staffed, &work, true, layout),
                (row_height(&staffed, &[], layout), 1)
            );
            assert_eq!(
                row_card(&both, &work, true, layout),
                (row_height(&both, &[], layout), 2)
            );
        }
        assert!(
            row_card(&staffed, &work, true, Layout::Desk).0
                > row_card(&bare, &work, true, Layout::Desk).0
        );
    }

    /// A card's ghost draws the lines of title its kept box has room for: one under its rows of
    /// chips in a box of one line, three in a bare box of three, the guess's two over a link's row
    /// and a role's, never none nor more than five.
    #[test]
    fn a_card_s_ghost_draws_the_title_lines_its_box_has_room_for() {
        use super::{ROW, title_lines};
        // As the board draws them: 18 px of border and padding, a 20.3 px slug line, 2 px, the
        // title's lines of 20.3 px, the chip rows' 12 px of margins, 21 px for a row of roles or
        // of phases and 17 px for one of links, and 8 px under the card.
        let drawn = |lines: f64, rows: &[f64]| {
            let height = 18.0 + 20.3 + 2.0 + lines * 20.3 + 12.0 + rows.iter().sum::<f64>();
            (height.round() as u32 + 8, u8::try_from(rows.len()).unwrap())
        };
        for lines in 1..=5 {
            for rows in [
                &[][..],
                &[21.0],
                &[17.0],
                &[17.0, 21.0],
                &[21.0, 17.0, 21.0],
            ] {
                assert_eq!(
                    title_lines(drawn(f64::from(lines), rows)),
                    lines,
                    "{lines} lines over {rows:?}"
                );
            }
        }
        assert_eq!(title_lines((ROW, 2)), 2);
        assert_eq!(title_lines((20, 1)), 1);
        assert_eq!(title_lines((20, u8::MAX)), 1);
        assert_eq!(title_lines((900, 0)), 5);
    }

    /// A browser keeps the cards of each column at each width, newest first, its own line again
    /// when drawn again, and 32 lines at most.
    #[test]
    fn a_column_s_kept_cards_are_those_drawn_last_in_a_window_as_wide() {
        use super::{CARDS_MAX, keep_cards, recall_cards};
        let kept = keep_cards("", "ready", 1280, &[(134, 1), (113, 0), (160, 2)]);
        assert_eq!(kept, "1280 ready 134c 113 160cc");
        let kept = keep_cards(&kept, "ready", 390, &[(160, 1)]);
        let kept = keep_cards(&kept, "ready_x", 1280, &[(101, 0)]);
        assert_eq!(
            recall_cards(&kept, "ready", 1280),
            [(134, 1), (113, 0), (160, 2)]
        );
        assert_eq!(recall_cards(&kept, "ready", 390), [(160, 1)]);
        assert_eq!(recall_cards(&kept, "ready", 800), []);
        assert_eq!(recall_cards(&kept, "review", 1280), []);
        // Drawn again: one line, first, with the new cards; a column of none keeps none.
        let kept = keep_cards(&kept, "ready", 1280, &[]);
        assert_eq!(kept, "1280 ready\n1280 ready_x 101\n390 ready 160c");
        assert_eq!(recall_cards(&kept, "ready", 1280), []);
        // A pitch it cannot read ends the column's cards there.
        assert_eq!(
            recall_cards("1280 ready 134c tall 113", "ready", 1280),
            [(134, 1)]
        );
        assert_eq!(
            recall_cards("1280 ready 134c c 113", "ready", 1280),
            [(134, 1)]
        );
        let many = (0..40).fold(String::new(), |kept, n| {
            keep_cards(&kept, &format!("s{n}"), 1280, &[(n, 0)])
        });
        assert_eq!(many.lines().count(), CARDS_MAX);
        assert_eq!(recall_cards(&many, "s39", 1280), [(39, 0)]);
        assert_eq!(recall_cards(&many, "s7", 1280), [], "the oldest went");
    }

    /// A column whose rows draw their cards one after another keeps each row as it stands, a row
    /// still waiting at the place it holds: again whenever one changed, never what it kept
    /// already, and nothing for a column of no rows (so a line kept before stays).
    #[test]
    fn a_column_whose_rows_draw_late_keeps_each_row_as_it_stands() {
        use super::keep_rows;
        // The second row still waits, holding 139 px; the others drew.
        let (a, held, c) = ((98, 1), (139, 2), (141, 0));
        assert_eq!(keep_rows(&[a, held, c], &[]), Some(vec![a, held, c]));
        assert_eq!(keep_rows(&[a, held, c], &[a, held, c]), None);
        // It drew at 121 px: kept again.
        let b = (121, 1);
        assert_eq!(keep_rows(&[a, b, c], &[a, held, c]), Some(vec![a, b, c]));
        // A card drawn again at another height, or with a row of chips more, is kept again.
        assert_eq!(
            keep_rows(&[a, (121, 2), c], &[a, b, c]),
            Some(vec![a, (121, 2), c])
        );
        let taller = (162, 0);
        assert_eq!(
            keep_rows(&[a, b, taller], &[a, b, c]),
            Some(vec![a, b, taller])
        );
        assert_eq!(keep_rows(&[], &[a]), None);
    }

    /// A column's keep watches its rows draw for as long as one still waits near the screen,
    /// past `SETTLE_MS` however loaded the machine, and stops once every row has drawn, once none
    /// waiting is within the sentinel's reach (a row past it draws only on a scroll), or at
    /// `KEEP_MS`, the bound on its work.
    #[test]
    fn a_column_s_keep_watches_until_what_is_near_the_screen_has_drawn() {
        use super::keeps_watching;
        use crate::viewport::{KEEP_MS, SETTLE_MS};
        assert!(keeps_watching(false, true, 100.0));
        assert!(
            keeps_watching(false, true, SETTLE_MS + 100.0),
            "rows near the screen still waiting after SETTLE_MS: it watches on"
        );
        assert!(keeps_watching(false, true, KEEP_MS - 100.0));
        assert!(
            !keeps_watching(false, true, KEEP_MS),
            "the bound on its work"
        );
        assert!(!keeps_watching(true, true, 100.0), "every row drew");
        assert!(!keeps_watching(true, false, 100.0));
        assert!(
            !keeps_watching(false, false, 100.0),
            "none waiting near the screen: those past it draw only on a scroll"
        );
    }

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    /// A column not yet near the screen draws a card's box for each row of its first page, in the
    /// height that page will take, and says it is busy.
    #[test]
    fn a_column_not_yet_near_the_screen_draws_a_card_box_per_task_in_its_first_page_s_height() {
        fn column(status: &str, count: usize) -> Element {
            rsx! { super::TaskColumn { status, work: Default::default(), connected: false,
            tasks: Vec::new(), count, query: use_signal(String::new), days: use_signal(|| 1_u8),
            onload: |_| {}, refreshed: use_signal(|| 0_u64) } }
        }
        let guessed = format!(
            "class=\"ghost-card\" style=\"height:{}px\"",
            super::ROW - super::GAP
        );
        let html = render(|| column("ready", 40));
        assert_eq!(html.matches(&guessed).count(), super::PAGE, "{html}");
        assert_eq!(
            html.matches("ghost-pill").count(),
            super::PAGE,
            "a typical card's row of links, and no role chip in a column no agent has taken up: {html}"
        );
        assert!(
            html.contains(&format!("min-height:{}px", 30 * super::ROW + super::MORE)),
            "{html}"
        );
        assert!(html.contains("aria-busy=\"true\""), "{html}");
        assert!(
            !html.contains("role=\"status\""),
            "the board says once that it loads: {html}"
        );
        let html = render(|| column("review", 2));
        assert_eq!(html.matches(&guessed).count(), 2, "{html}");
        assert_eq!(
            html.matches("ghost-pill").count(),
            2 * 2,
            "a typical card's row of links and its row of roles: {html}"
        );
        let html = render(|| column("review", 0));
        assert!(html.contains("aria-busy=\"false\""), "{html}");
        assert!(!html.contains("ghost-card"), "{html}");
    }

    fn strings(statuses: &[&str]) -> Vec<String> {
        statuses.iter().map(|s| (*s).to_string()).collect()
    }

    /// The board's shape names the columns the board draws, in its order, whatever the server
    /// lists and whether a machine reports.
    #[test]
    fn the_board_s_shape_is_the_columns_the_board_draws_in_their_order() {
        let mut named = LIFECYCLE.to_vec();
        named.extend(["build", "test"]);
        for (work, statuses) in [
            (symphony_api::MachineWork::default(), &LIFECYCLE[..]),
            (symphony_api::MachineWork::default(), &named[..]),
            (reported(), &LIFECYCLE[..]),
            (reported(), &["ready", "backlog"][..]),
        ] {
            let reporting = !work.machines.is_empty();
            let html = pipeline(work, true, statuses);
            let shape = super::board_shape(&[], &strings(statuses), 0, reporting);
            assert_eq!(
                shape
                    .iter()
                    .map(|(status, _)| status.as_str())
                    .collect::<Vec<_>>(),
                columns(&html),
                "{html}"
            );
        }
    }

    /// Each column of the shape has its count, none where the board counts none; sign-off comes
    /// first while a task waits for it; with no statuses listed, the counted ones are the board's.
    #[test]
    fn the_board_s_shape_counts_each_column_and_opens_on_the_tasks_waiting_for_sign_off() {
        let counts = vec![("ready".to_string(), 14), ("backlog".to_string(), 3)];
        assert_eq!(
            super::board_shape(&counts, &[], 2, false),
            [
                ("signoff", 2),
                ("ready", 14),
                ("build-test", 0),
                ("backlog", 3)
            ]
            .map(|(s, n)| (s.to_string(), n))
        );
        assert_eq!(
            super::board_shape(&counts, &strings(&["ready", "review"]), 0, true),
            [("ready", 14), ("build", 0), ("test", 0), ("review", 0)]
                .map(|(s, n)| (s.to_string(), n))
        );
    }

    /// The shape is kept as lines of `STATUS COUNT` and read back as it was; one never kept, or
    /// kept unreadable, is the pipeline and the columns off it with a task in each.
    #[test]
    fn a_kept_shape_reads_back_as_it_was_kept_and_none_as_a_task_in_each_column() {
        let shape = super::board_shape(&[("ready".to_string(), 14)], &[], 1, false);
        let kept = super::keep_shape(&shape);
        assert_eq!(kept, "signoff 1\nready 14\nbuild-test 0");
        assert_eq!(super::recall_shape(Some(&kept)), shape);
        let default = super::recall_shape(None);
        assert_eq!(super::recall_shape(Some("not a shape\n")), default);
        assert_eq!(
            default
                .iter()
                .map(|(status, _)| status.as_str())
                .collect::<Vec<_>>(),
            [
                "concept",
                "design",
                "ready",
                "in_progress",
                "build",
                "test",
                "review",
                "done",
                "backlog",
                "blocked",
                "cancelled"
            ]
        );
        assert!(default.iter().all(|(_, count)| *count == 1));
    }

    /// Each ghost column is the column it stands for, in its colour, with a card per task it held
    /// (a page's at most, then the Load more button's room; none for an empty column); Build and
    /// Test draw their lines of text where they hold no card.
    #[test]
    fn the_board_s_ghost_draws_each_column_it_last_had_with_its_cards() {
        let html = render(|| {
            let shape = [
                ("signoff", 1),
                ("ready", 20),
                ("build", 0),
                ("test", 0),
                ("review", 2),
                ("done", 31),
                ("blocked", 0),
            ];
            rsx! { super::GhostBoard { shape: shape.map(|(s, n)| (s.to_string(), n)).to_vec() } }
        });
        let column = |status: &str| {
            let start = html.find(&format!("data-for=\"{status}\"")).unwrap();
            let end = html[start + 1..]
                .find("data-for=")
                .map_or(html.len(), |n| start + 1 + n);
            &html[start..end]
        };
        let cards = |status: &str, height: u32| {
            column(status)
                .matches(&format!("class=\"ghost-card\" style=\"height:{height}px\""))
                .count()
        };
        let guessed = super::ROW - super::GAP;
        let more = format!("style=\"height:{}px\"></div>", super::MORE);
        assert_eq!(cards("signoff", guessed), 1, "{html}");
        assert_eq!(cards("ready", guessed), 20, "{html}");
        assert_eq!(
            column("ready").matches("ghost-pill").count(),
            20,
            "a typical card's links and, where no agent has taken the tasks up, no role: {html}"
        );
        assert!(!column("ready").contains(&more), "{html}");
        assert_eq!(cards("review", guessed), 2, "{html}");
        assert_eq!(
            column("review").matches("ghost-pill").count(),
            2 * 2,
            "{html}"
        );
        assert_eq!(cards("done", guessed), super::PAGE, "{html}");
        assert!(column("done").contains(&more), "{html}");
        for empty in ["build", "test", "blocked"] {
            assert!(!column(empty).contains("ghost-card"), "{empty}: {html}");
        }
        assert_eq!(
            column("build").matches("class=\"bone\"").count(),
            3,
            "heading, Machine context and the empty line: {html}"
        );
        assert_eq!(
            column("test").matches("class=\"bone\"").count(),
            2,
            "heading and the empty line: {html}"
        );
        assert!(
            html.find("data-for=\"signoff\"") < html.find("ghost-pipeline"),
            "{html}"
        );
        assert!(
            html.find("ghost-offline") < html.find("data-for=\"blocked\""),
            "{html}"
        );
        assert!(
            !html.contains("role=\"status\""),
            "the page says once that it loads: {html}"
        );
    }

    /// The board's ghost is drawn in classes of its own: a wait for the board's grid, a column, a
    /// card or the scroller never takes the ghost for the board.
    #[test]
    fn the_board_s_ghost_carries_none_of_the_classes_the_board_is_found_by() {
        let html = render(|| rsx! { super::GhostBoard { shape: super::recall_shape(None) } });
        let classes: Vec<&str> = html
            .split("class=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .flat_map(str::split_whitespace)
            .collect();
        assert!(classes.contains(&"ghost-kanban"), "{html}");
        assert!(!html.contains("data-status"), "{html}");
        for real in [
            "kanban",
            "pipeline",
            "board-scroll",
            "offline",
            "column",
            "taskcard",
        ] {
            assert!(!classes.contains(&real), "the ghost is a .{real}: {html}");
        }
    }

    #[test]
    fn a_task_counts_a_card_in_a_document_it_links_whatever_part_it_links() {
        let links = ["docs/analysis/x.md#part".to_string(), "docs/B.md".into()];
        for (key, linked) in [
            ("/docs/analysis/x.md#card", true),
            ("docs/analysis/x.md#other", true),
            ("/docs/B.md#c", true),
            ("/docs/B.md", true),
            ("/docs/analysis/y.md#card", false),
            ("/docs/analysis/x.md.bak#card", false),
            ("/docs/analysis", false),
            ("#card", false),
            ("", false),
        ] {
            assert_eq!(super::links_card(&links, key), linked, "{key}");
        }
        assert!(!super::links_card(&[], "/docs/B.md#c"));
        assert!(
            !super::links_card(&[String::new()], "#card"),
            "an empty path is no document"
        );
    }

    #[test]
    fn a_tasks_gates_follow_the_branch_they_were_answered_for_else_its_slug() {
        let answered = ("tools/x".to_string(), "feature/x".to_string());
        assert_eq!(super::gates_branch("tools/x", Some(&answered)), "feature/x");
        assert_eq!(
            super::gates_branch("tools/y", Some(&answered)),
            "tools/y",
            "another task's answer says nothing of this one's"
        );
        assert_eq!(super::gates_branch("tools/x", None), "tools/x");
    }
}
