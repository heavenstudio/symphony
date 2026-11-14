//! Mount expensive content once, just before scrolling it into view. Keeping it mounted after
//! that first visit preserves open forms and media. The sentinel extends 600 px each way.

use dioxus::prelude::*;

/// The one visibility latch for sections and native media with their own reserved layout.
pub fn use_visibility(initial: bool) -> (Signal<bool>, Vec<Attribute>) {
    #[cfg(target_arch = "wasm32")]
    {
        crate::entry_events::use_visibility(initial)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut shown = use_signal(|| initial || cfg!(not(target_arch = "wasm32")));
        let reveal = use_callback(move |event: Event<VisibleData>| {
            if event.is_intersecting().unwrap_or(false) {
                shown.set(true);
            }
        });
        (shown, vec![dioxus::html::onvisible(reveal)])
    }
}

/// `children` mounted once they come near the screen; until then `ghost`, the outline of what
/// they will be, in a box of `height` px (lines of text where no ghost is given).
#[component]
pub fn NearViewport(
    children: Element,
    #[props(default)] anchors: Vec<String>,
    #[props(default = 160)] height: u32,
    #[props(default)] id: String,
    #[props(default)] class: String,
    #[props(default)] list_item: bool,
    onenter: Option<EventHandler<()>>,
    ghost: Option<Element>,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    let target = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .and_then(|hash| js_sys::decode_uri_component(hash.trim_start_matches('#')).ok())
        .map(String::from)
        .unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let addressed = !target.is_empty() && (anchors.contains(&target) || target == id);
    #[cfg(not(target_arch = "wasm32"))]
    let addressed = false;
    let (shown, reveal) = use_visibility(addressed);
    let fired = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(false)));
    use_effect(move || {
        if shown() {
            #[cfg(target_arch = "wasm32")]
            if addressed {
                crate::navigation::scroll_anchor();
            }
            if !fired.replace(true)
                && let Some(handler) = onenter
            {
                handler.call(());
            }
        }
    });
    let content = rsx! {
            if !shown() {
                div { class: "viewport-sentinel", "aria-hidden": "true", ..reveal }
                div { class: "viewport-placeholder", style: "min-height:{height}px", "aria-hidden": "true", "data-pending": "true",
                    for anchor in anchors { span { id: anchor } }
                    if let Some(ghost) = ghost { {ghost} } else { Skeleton { lines: lines_for(height) } }
                }
            } else {
                {children}
            }
    };
    let style = format!(
        "contain-intrinsic-size:auto {height}px;min-height:{}px",
        if shown() { 0 } else { height }
    );
    if list_item {
        rsx! { li { id, style, class: "viewport-slot {class}", {content} } }
    } else {
        rsx! { div { id, style, class: "viewport-slot {class}", {content} } }
    }
}

/// The longest a page stays under its skeleton waiting for its first screen's reads, in ms.
pub const SETTLE_MS: f64 = 3_000.0;

/// Whether a reserved region's box (`[top, bottom, left, right]`) shows on `screen`, the part of
/// the viewport its page covers, in the same terms. A box with no area (a closed `details`, a
/// hidden tab) shows nowhere.
pub fn on_screen(region: [f64; 4], screen: [f64; 4]) -> bool {
    let [top, bottom, left, right] = region;
    bottom > top
        && right > left
        && bottom > screen[0]
        && top < screen[1]
        && right > screen[2]
        && left < screen[3]
}

/// The height of a line of body text, in px (16 px at a line height of 1.55, and its gap).
const LINE: u32 = 26;

/// How many lines of text a placeholder `height` px tall stands for: one at least, six at most.
pub fn lines_for(height: u32) -> u8 {
    u8::try_from((height / LINE).clamp(1, 6)).unwrap_or(6)
}

/// A paragraph waiting for its read: a bar where each of its lines will be, at the line's own
/// height, shimmering only for a reader who accepts motion, and `label` for one who reads the
/// page's text instead. A ghost of a larger part (a card, a page) draws its own boxes, with a
/// `span.bone` for each line in them.
#[component]
pub fn Skeleton(#[props(default = 3)] lines: u8, #[props(default)] label: String) -> Element {
    rsx! { div { class: "skeleton",
        if !label.is_empty() { span { class: "status-text", role: "status", "{label}" } }
        for line in 0..lines { span { key: "{line}", class: "bone", "aria-hidden": "true" } }
    } }
}

/// Draw `children` hidden under `cover`, the outline of the page they make, until no region they
/// reserve on the first screen (`[data-pending=true]`) waits for its read any longer, until
/// `failed`, or for [`SETTLE_MS`] at most: the reader's first screen arrives whole, and no read
/// that lands moves it.
#[component]
pub fn Settle(children: Element, cover: Element, #[props(default)] failed: bool) -> Element {
    #[cfg(target_arch = "wasm32")]
    let (settled, id) = use_settled(failed);
    #[cfg(not(target_arch = "wasm32"))]
    let (settled, id) = (use_signal(|| true), String::new());
    rsx! { div { class: "settle", "data-settle": id, "data-settled": "{settled}", "aria-busy": "{!settled()}",
        if !settled() { div { class: "settle-cover", {cover} } }
        {children}
    } }
}

/// The page's settled latch: it checks each frame, from the first its root is drawn in, whether
/// a pending region is still on the screen.
#[cfg(target_arch = "wasm32")]
fn use_settled(failed: bool) -> (Signal<bool>, String) {
    let id = use_hook(crate::entry_events::marker);
    let mut settled = use_signal(|| false);
    use_effect(use_reactive!(|failed| {
        if failed {
            settled.set(true);
        }
    }));
    let selector = format!("[data-settle='{id}']");
    use_hook(move || {
        spawn(async move {
            let mut drawn = None;
            while !*settled.peek() {
                crate::live::sleep(16).await;
                let Some(root) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.query_selector(&selector).ok().flatten())
                else {
                    continue;
                };
                let since = *drawn.get_or_insert_with(js_sys::Date::now);
                if !waiting(&root) || js_sys::Date::now() - since > SETTLE_MS {
                    settled.set(true);
                }
            }
        });
    });
    (settled, id)
}

/// Whether `root` has not drawn its page yet (only its skeleton is in it), or a region of it on
/// the screen still waits for its read.
#[cfg(target_arch = "wasm32")]
fn waiting(root: &web_sys::Element) -> bool {
    if root.child_element_count() < 2 {
        return true;
    }
    let Some(window) = web_sys::window() else {
        return false;
    };
    let page = root.get_bounding_client_rect();
    let screen = [
        0.0,
        size(window.inner_height()),
        page.left().max(0.0),
        page.right().min(size(window.inner_width())),
    ];
    root.query_selector_all("[data-pending=true]")
        .is_ok_and(|pending| any_shows(&pending, screen))
}

/// A window dimension in CSS pixels, 0 when the browser gives none.
#[cfg(target_arch = "wasm32")]
fn size(value: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>) -> f64 {
    value.ok().and_then(|v| v.as_f64()).unwrap_or_default()
}

/// Whether any of the reserved regions in `pending` shows on the `screen` ([`on_screen`]).
#[cfg(target_arch = "wasm32")]
fn any_shows(pending: &web_sys::NodeList, screen: [f64; 4]) -> bool {
    use wasm_bindgen::JsCast;
    (0..pending.length())
        .filter_map(|index| pending.item(index)?.dyn_into::<web_sys::Element>().ok())
        .any(|region| {
            let r = region.get_bounding_client_rect();
            on_screen([r.top(), r.bottom(), r.left(), r.right()], screen)
        })
}

/// Where this browser keeps the height parts of pages were last drawn at, newest first, as lines
/// of `WIDTH HEIGHT KEY`: a page's ghost draws a part it saw before at the height it took in a
/// window as wide.
const DRAWN: &str = "drawn-heights";
/// How many parts' heights a browser keeps.
const DRAWN_MAX: usize = 128;

/// The height `kept` (as [`DRAWN`] holds it) gives the part `key` in a window `width` px wide;
/// for a part never drawn at that width, the one the last part of its kind took there (the kind
/// is the key before its last space: `head` of `head SLUG`, so a task never opened takes the last
/// task's head). A key with no space has no kind.
pub fn recall_height(kept: &str, key: &str, width: u32) -> Option<u32> {
    let kind = key.rsplit_once(' ').map(|(kind, _)| kind);
    let drawn = |line: &str| {
        let mut parts = line.splitn(3, ' ');
        let drawn = parts.next()?.parse::<u32>().ok()?;
        let height = parts.next()?.parse::<u32>().ok()?;
        (drawn == width).then_some((height, parts.next()?.to_string()))
    };
    let mut alike = None;
    for (height, part) in kept.lines().filter_map(drawn) {
        if part == key {
            return Some(height);
        }
        if alike.is_none() && kind.is_some() && part.rsplit_once(' ').map(|(k, _)| k) == kind {
            alike = Some(height);
        }
    }
    alike
}

/// `kept` with the part `key` drawn `height` px tall in a window `width` px wide, first, its line
/// before and the oldest past [`DRAWN_MAX`] dropped.
pub fn keep_height(kept: &str, key: &str, width: u32, height: u32) -> String {
    let line = format!("{width} {height} {key}");
    std::iter::once(line.as_str())
        .chain(
            kept.lines()
                .filter(|old| old.splitn(3, ' ').nth(2) != Some(key)),
        )
        .take(DRAWN_MAX)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The height this browser last drew the part `key` at (or the last part of its kind,
/// [`recall_height`]), in a window as wide as this one.
pub fn drawn_height(key: &str) -> Option<u32> {
    recall_height(
        &crate::platform::current().load(DRAWN)?,
        key,
        window_width(),
    )
}

/// The window's width in whole px; 0 on the host, which has none.
pub fn window_width() -> u32 {
    #[cfg(target_arch = "wasm32")]
    return web_sys::window().map_or(0, |window| size(window.inner_width()).round() as u32);
    #[cfg(not(target_arch = "wasm32"))]
    0
}

/// The style that draws a ghost's box `height` px tall, border and padding in: as tall as the
/// part it stands for was drawn, whatever it holds. None for no height kept.
pub fn drawn_style(height: Option<u32>) -> String {
    height.map_or_else(String::new, |height| {
        format!("box-sizing:border-box;height:{height}px;overflow:hidden")
    })
}

/// How far above and below the screen a placeholder's sentinel reaches, so a region waiting
/// there is about to draw (`.viewport-sentinel`), in px.
pub const REACH: f64 = 600.0;

/// `screen` (`[top, bottom, left, right]`) with [`REACH`] above and below it: where a reserved
/// region draws once its sentinel says so.
pub fn within_reach(screen: [f64; 4]) -> [f64; 4] {
    let [top, bottom, left, right] = screen;
    [top - REACH, bottom + REACH, left, right]
}

/// `screen` cut to the box `clip` (both `[top, bottom, left, right]`): the part of it a region
/// inside a scroller of that box shows in, since the scroller hides what lies past it.
pub fn clip_to(screen: [f64; 4], clip: [f64; 4]) -> [f64; 4] {
    [
        screen[0].max(clip[0]),
        screen[1].min(clip[1]),
        screen[2].max(clip[2]),
        screen[3].min(clip[3]),
    ]
}

/// The longest a keep for the next visit's ghost watches what is near the screen draw, in ms
/// ([`remember_drawn`], as the web client's task board keeps its columns'): only a bound on its
/// work, as on a loaded machine or a slow phone a region near the screen draws many seconds after
/// its data.
pub const KEEP_MS: f64 = 30_000.0;

/// Whether a region inside `root` still waits for its read within [`REACH`] of the screen: the
/// window, cut to the box of `root`'s closest ancestor matching `scroller` when it sits in one.
#[cfg(target_arch = "wasm32")]
pub fn waits_near(root: &web_sys::Element, scroller: Option<&str>) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let mut screen = [
        0.0,
        size(window.inner_height()),
        0.0,
        size(window.inner_width()),
    ];
    if let Some(Ok(Some(clip))) = scroller.map(|selector| root.closest(selector)) {
        let r = clip.get_bounding_client_rect();
        screen = clip_to(screen, [r.top(), r.bottom(), r.left(), r.right()]);
    }
    root.query_selector_all("[data-pending=true]")
        .is_ok_and(|pending| any_shows(&pending, within_reach(screen)))
}

/// Keep, from `after` ms once it is drawn, the height of each of `parts` (`(key, selector)`) for
/// its ghost on the next visit ([`drawn_height`]): measured every 100 ms and kept again when one
/// changed, until no region still waiting for its read in them is within [`REACH`] of the screen
/// (what is near the screen has drawn), or for [`KEEP_MS`] at most. A region near the screen
/// draws when its sentinel says so, which on a loaded machine or a slow phone comes well after
/// the page's data.
#[cfg(target_arch = "wasm32")]
pub fn remember_drawn(parts: Vec<(String, String)>, after: i32) {
    spawn(async move {
        crate::live::sleep(after).await;
        let mut waited = f64::from(after);
        let mut saved = Vec::new();
        loop {
            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(document) = window.document() else {
                return;
            };
            let screen = within_reach([
                0.0,
                size(window.inner_height()),
                0.0,
                size(window.inner_width()),
            ]);
            let mut heights = Vec::new();
            let mut waiting = false;
            for (key, selector) in &parts {
                if let Ok(Some(part)) = document.query_selector(selector) {
                    let height = part.get_bounding_client_rect().height().round() as u32;
                    heights.push((key.as_str(), height));
                    waiting |= part
                        .query_selector_all("[data-pending=true]")
                        .is_ok_and(|pending| any_shows(&pending, screen));
                }
            }
            if heights != saved {
                let width = window_width();
                let platform = crate::platform::current();
                let mut kept = platform.load(DRAWN).unwrap_or_default();
                for &(key, height) in &heights {
                    kept = keep_height(&kept, key, width, height);
                }
                platform.save(DRAWN, &kept);
                saved = heights;
            }
            if !waiting || waited >= KEEP_MS {
                return;
            }
            crate::live::sleep(100).await;
            waited += 100.0;
        }
    });
}

/// Keep what the reader looks at in place across a live update about to be drawn, once they have
/// scrolled: the focused field on the screen, else the first of `rows` on it (the shell's
/// `reviewHoldPlace`). The browser's own scroll anchoring keeps neither a field being typed in
/// below a message that lands, nor a row in a scroller off its top (the board's columns).
#[cfg(target_arch = "wasm32")]
pub fn hold_place(rows: &str) {
    use wasm_bindgen::{JsCast, JsValue};
    if let Some(window) = web_sys::window()
        && let Ok(hold) = js_sys::Reflect::get(&window, &"reviewHoldPlace".into())
        && let Some(hold) = hold.dyn_ref::<js_sys::Function>()
    {
        let _ = hold.call1(&JsValue::UNDEFINED, &JsValue::from_str(rows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: [f64; 4] = [0.0, 844.0, 16.0, 374.0];

    #[test]
    fn a_region_shows_when_any_of_its_area_meets_the_screen() {
        assert!(on_screen([100.0, 300.0, 16.0, 374.0], SCREEN));
        // Partly above, partly below, partly to either side.
        assert!(on_screen([-200.0, 1.0, 16.0, 374.0], SCREEN));
        assert!(on_screen([843.0, 1200.0, 16.0, 374.0], SCREEN));
        assert!(on_screen([100.0, 300.0, -100.0, 17.0], SCREEN));
        assert!(on_screen([100.0, 300.0, 373.0, 800.0], SCREEN));
        // A region larger than the screen on every side.
        assert!(on_screen([-1000.0, 2000.0, -50.0, 900.0], SCREEN));
    }

    #[test]
    fn a_region_off_the_screen_or_without_area_does_not_show() {
        // Edges that only touch the screen's.
        assert!(!on_screen([-200.0, 0.0, 16.0, 374.0], SCREEN));
        assert!(!on_screen([844.0, 1000.0, 16.0, 374.0], SCREEN));
        // A column scrolled out past either side of the page.
        assert!(!on_screen([100.0, 300.0, 374.0, 700.0], SCREEN));
        assert!(!on_screen([100.0, 300.0, -300.0, 16.0], SCREEN));
        // A closed `details` or a hidden tab: no height, or no width.
        assert!(!on_screen([200.0, 200.0, 16.0, 374.0], SCREEN));
        assert!(!on_screen([200.0, 300.0, 50.0, 50.0], SCREEN));
        assert!(!on_screen([0.0, 0.0, 0.0, 0.0], SCREEN));
    }

    /// A region waiting within its sentinel's reach above or below the screen is about to draw;
    /// one past it, or beside the screen, is not.
    #[test]
    fn a_region_within_the_sentinel_s_reach_of_the_screen_is_about_to_draw() {
        let reach = within_reach(SCREEN);
        assert_eq!(reach, [-REACH, 844.0 + REACH, 16.0, 374.0]);
        assert!(on_screen([1200.0, 1300.0, 16.0, 374.0], reach));
        assert!(on_screen([-700.0, -500.0, 16.0, 374.0], reach));
        assert!(!on_screen([1444.0, 1600.0, 16.0, 374.0], reach));
        assert!(!on_screen([-900.0, -600.0, 16.0, 374.0], reach));
        assert!(!on_screen([100.0, 300.0, 400.0, 700.0], reach));
    }

    /// Inside a scroller, a region's reach is the screen cut to the scroller's box: a row past the
    /// board's own bottom waits for a scroll of the board, however near the window's.
    #[test]
    fn a_region_inside_a_scroller_reaches_only_as_far_as_the_scroller_shows() {
        let board = [300.0, 524.0, 0.0, 2000.0];
        assert_eq!(clip_to(SCREEN, board), [300.0, 524.0, 16.0, 374.0]);
        assert_eq!(clip_to(SCREEN, [-50.0, 2000.0, 0.0, 400.0]), SCREEN);
        let reach = within_reach(clip_to(SCREEN, board));
        assert!(on_screen([1100.0, 1239.0, 16.0, 296.0], reach));
        assert!(
            !on_screen([1200.0, 1339.0, 16.0, 296.0], reach),
            "within the window's reach, past the board's"
        );
        assert!(on_screen(
            [1200.0, 1339.0, 16.0, 296.0],
            within_reach(SCREEN)
        ));
    }

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn a_skeleton_draws_a_bar_per_line_for_the_eye_and_its_label_for_a_reader_of_text() {
        let html = render(|| rsx! { Skeleton { lines: 4, label: "Loading tasks…" } });
        assert_eq!(
            html.matches("class=\"bone\" aria-hidden=\"true\"").count(),
            4,
            "{html}"
        );
        assert!(
            html.contains("<span class=\"status-text\" role=\"status\">Loading tasks…</span>"),
            "{html}"
        );
        let unlabelled = render(|| rsx! { Skeleton {} });
        assert_eq!(
            unlabelled.matches("class=\"bone\"").count(),
            3,
            "{unlabelled}"
        );
        assert!(!unlabelled.contains("status-text"), "{unlabelled}");
    }

    #[test]
    fn a_placeholder_stands_for_a_line_of_text_per_26_px_from_one_to_six() {
        assert_eq!(lines_for(0), 1);
        assert_eq!(lines_for(28), 1);
        assert_eq!(lines_for(51), 1);
        assert_eq!(lines_for(52), 2);
        assert_eq!(lines_for(100), 3);
        assert_eq!(lines_for(156), 6);
        assert_eq!(lines_for(160), 6);
        assert_eq!(lines_for(u32::MAX), 6);
    }

    #[test]
    fn a_settled_page_draws_its_content_without_its_cover_over_it() {
        // A page drawn ahead of the browser (and its text for a test) is settled: only the
        // browser can tell what a first screen shows.
        let html = render(
            || rsx! { Settle { cover: rsx! { div { class: "ghost-task" } }, p { "The page" } } },
        );
        assert_eq!(
            html,
            "<div class=\"settle\" data-settle=\"\" data-settled=\"true\" aria-busy=\"false\"><p>The page</p></div>"
        );
    }

    #[test]
    fn a_part_s_kept_height_is_the_one_drawn_last_in_a_window_as_wide() {
        let kept = keep_height("", "head tools/x", 390, 312);
        let kept = keep_height(&kept, "brief tools/x", 390, 640);
        assert_eq!(recall_height(&kept, "head tools/x", 390), Some(312));
        assert_eq!(recall_height(&kept, "brief tools/x", 390), Some(640));
        // Another width: nothing kept.
        assert_eq!(recall_height(&kept, "head tools/x", 1280), None);
        // Drawn again, in a wider window: its line moves first and holds the new height alone.
        let kept = keep_height(&kept, "head tools/x", 1280, 220);
        assert_eq!(kept, "1280 220 head tools/x\n390 640 brief tools/x");
        assert_eq!(recall_height(&kept, "head tools/x", 390), None);
        assert_eq!(
            recall_height("garbled\n390 tall head tools/x", "head tools/x", 390),
            None
        );
    }

    #[test]
    fn a_browser_keeps_the_heights_of_its_last_128_parts() {
        let kept = (0..200).fold(String::new(), |kept, n| {
            keep_height(&kept, &format!("head t/{n}"), 390, n)
        });
        assert_eq!(kept.lines().count(), DRAWN_MAX);
        assert_eq!(recall_height(&kept, "head t/199", 390), Some(199));
        assert_eq!(recall_height(&kept, "head t/72", 390), Some(72));
        // The oldest are gone: t/71 takes the newest head's, as a task never drawn does.
        assert!(!kept.lines().any(|line| line.ends_with(" head t/71")));
        assert_eq!(recall_height(&kept, "head t/71", 390), Some(199));
    }

    #[test]
    fn a_part_never_drawn_takes_the_height_of_the_last_of_its_kind_drawn_as_wide() {
        let kept = keep_height("", "head tools/x", 390, 312);
        let kept = keep_height(&kept, "head tools/y", 1280, 200);
        let kept = keep_height(&kept, "head tools/z", 390, 280);
        let kept = keep_height(&kept, "brief tools/z", 390, 640);
        // The newest head drawn in a window as wide.
        assert_eq!(recall_height(&kept, "head tools/new", 390), Some(280));
        assert_eq!(recall_height(&kept, "head tools/new", 1280), Some(200));
        assert_eq!(recall_height(&kept, "brief tools/new", 390), Some(640));
        // A part's own height wins over a newer one of its kind.
        assert_eq!(recall_height(&kept, "head tools/x", 390), Some(312));
        // Another kind, another width, or a key with no kind: nothing.
        assert_eq!(recall_height(&kept, "gates tools/new", 390), None);
        assert_eq!(recall_height(&kept, "brief tools/new", 1280), None);
        assert_eq!(recall_height(&kept, "head tools/new", 800), None);
        let queue = keep_height("", "queue:you", 390, 180);
        assert_eq!(recall_height(&queue, "queue:spot", 390), None);
        assert_eq!(recall_height(&queue, "queue:you", 390), Some(180));
    }

    #[test]
    fn a_ghost_drawn_at_a_kept_height_holds_its_border_and_padding_in_it() {
        assert_eq!(
            drawn_style(Some(312)),
            "box-sizing:border-box;height:312px;overflow:hidden"
        );
        assert_eq!(drawn_style(None), "");
        assert_eq!(drawn_height("head tools/x"), None, "the host keeps nothing");
    }
}
