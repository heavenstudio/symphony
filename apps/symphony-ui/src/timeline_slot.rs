//! The task page's place for its timeline: a box as tall as the timeline was when this browser
//! last drew it for the task (else a default), placeholder rows drawn in it, into which the
//! timeline's own module ([`crate::task_timeline`], the page kind `timeline`) moves its root. The
//! box asks the shell for the module once it nears the screen (`reviewTimeline`); the shell loads
//! it once the page has its data and is idle. The page compiles none of the timeline's code, and
//! nothing outside the box moves when the timeline arrives or a read adds a step to it.

use dioxus::prelude::*;

/// The id of the box the timeline module's root moves into.
pub const SLOT: &str = "timeline-slot";

/// The box's height on a phone (the list) and wide (the lanes) for a task this browser has drawn
/// no timeline of, in pixels: the most a timeline shows before its "Show all", the owner's 240 at
/// any width (docs/DECISIONS.md), so the brief and the thread start near the page's top. What
/// does not fit is left out: on a phone the earliest steps, fading, and wide the lanes under the
/// last that fits whole, the box then kept as tall as those for the next visit.
pub const NARROW: u32 = 240;
pub const WIDE: u32 = 240;
/// The least a box is reserved, whatever was kept.
pub const LEAST: u32 = 120;

/// The box's style, the task page's own and small (the timeline's is its module's): put in the
/// page by the first box to mount, as `attach.rs` puts its own, so no first visit's manifest
/// names it.
#[cfg(any(target_arch = "wasm32", test))]
pub const STYLE: &str = include_str!("../assets/timeline-slot.css");

/// The key a task's drawn heights are kept under in this browser, before its project's id.
pub fn key(slug: &str) -> String {
    format!("timeline-height:{slug}")
}

/// The heights kept as "NARROW,WIDE" read back, each within [`LEAST`] and its default: an
/// unreadable or missing one is the default.
pub fn heights(kept: Option<&str>) -> (u32, u32) {
    let mut said = kept.unwrap_or_default().split(',').map(str::trim);
    let mut read = |default: u32| {
        said.next()
            .and_then(|height| height.parse::<u32>().ok())
            .map_or(default, |height| height.clamp(LEAST, default))
    };
    (read(NARROW), read(WIDE))
}

/// The heights to keep once a draw took `natural` pixels at one width (`wide` or not), the other
/// kept as it was.
pub fn kept(before: Option<&str>, wide: bool, natural: f64) -> String {
    let (narrow, wide_height) = heights(before);
    let natural = natural.ceil().clamp(0.0, f64::from(u32::MAX)) as u32;
    if wide {
        format!("{narrow},{}", natural.clamp(LEAST, WIDE))
    } else {
        format!("{},{wide_height}", natural.clamp(LEAST, NARROW))
    }
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
fn stored(slug: &str) -> String {
    crate::project::stored(&crate::project::current(), &key(slug))
}

/// The heights this browser keeps for `slug`'s timeline: the box's, and its ghost's on the task's
/// placeholder (`GhostTask`).
pub fn reserved(slug: &str) -> (u32, u32) {
    #[cfg(target_arch = "wasm32")]
    {
        heights(
            storage()
                .and_then(|storage| storage.get_item(&stored(slug)).ok().flatten())
                .as_deref(),
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = slug;
        heights(None)
    }
}

/// Keep the height `slug`'s timeline took now, at this width, for its next visit (the module
/// calls it after each draw); the box shown keeps its own until the page is left.
#[cfg(target_arch = "wasm32")]
pub fn keep(slug: &str, wide: bool, natural: f64) {
    if let Some(storage) = storage() {
        let key = stored(slug);
        let before = storage.get_item(&key).ok().flatten();
        let _ = storage.set_item(&key, &kept(before.as_deref(), wide, natural));
    }
}

/// How many placeholder rows a box `height` pixels tall draws.
pub fn placeholders(height: u32) -> u32 {
    (height.saturating_sub(90) / 46).clamp(1, 9)
}

/// The task's timeline box, one per task: another task's page gets a new one (a component is
/// mounted anew only for a new key in a list), so its height holds for the visit.
#[component]
pub fn TimelineSlot(slug: String, revision: f64) -> Element {
    rsx! { for task in std::iter::once(slug) { SlotBox { key: "{task}", slug: task.clone(), revision } } }
}

/// The box of the task `slug`. It asks the shell for the timeline of the task at `revision` (its
/// `updated`) once it nears the screen and again when the task changes, and says the box is gone
/// when it unmounts.
#[component]
fn SlotBox(slug: String, revision: f64) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_hook(put_style);
    let (narrow, wide) = use_hook({
        let slug = slug.clone();
        move || reserved(&slug)
    });
    let (shown, reveal) = crate::viewport::use_visibility(false);
    #[cfg(target_arch = "wasm32")]
    {
        let owner = use_hook(next_owner);
        let asked = slug.clone();
        use_effect(use_reactive((&revision,), move |(revision,)| {
            if shown() {
                ask(owner, &asked, revision, (narrow, wide));
            }
        }));
        use_drop(move || close(owner));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (revision, shown());
    // The development build has every page kind in one module and no shell to load another: the
    // timeline is drawn in the box directly.
    #[cfg(feature = "devtools")]
    let direct = shown()
        .then(|| rsx! { crate::task_timeline::TimelineHost { slug: slug.clone(), revision } });
    #[cfg(not(feature = "devtools"))]
    let direct: Option<Element> = None;
    let rows = placeholders(narrow.max(wide));
    rsx! { section { class: "tl", id: "timeline", "aria-labelledby": "timeline-title",
        if !shown() { div { class: "viewport-sentinel", "aria-hidden": "true", ..reveal } }
        h2 { id: "timeline-title", "Timeline" }
        div { class: "tl-box card", id: SLOT, "data-slug": "{slug}", style: "--tl-narrow:{narrow}px;--tl-wide:{wide}px",
            div { class: "tl-skeleton",
                span { class: "tl-sk-said", "Reading the timeline…" }
                span { class: "tl-sk-line tl-sk-head", "aria-hidden": "true" }
                for row in 0..rows {
                    span { key: "{row}", class: "tl-sk-row", "aria-hidden": "true",
                        span { class: "tl-sk-line tl-sk-at" }
                        span { class: "tl-sk-line tl-sk-label", style: "width:{30 + (row * 17) % 40}%" }
                        span { class: "tl-sk-line tl-sk-bar", style: "width:{20 + (row * 29) % 50}%" }
                    }
                }
            }
            if let Some(direct) = direct { {direct} }
        }
    } }
}

/// A number no other box on this page has had: the shell takes a close only from the box whose
/// request it holds.
#[cfg(target_arch = "wasm32")]
fn next_owner() -> f64 {
    thread_local! {
        static NEXT: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }
    NEXT.with(|next| {
        next.set(next.get() + 1.0);
        next.get()
    })
}

#[cfg(target_arch = "wasm32")]
fn send(request: &js_sys::Object) {
    use wasm_bindgen::{JsCast, JsValue};
    if let Some(window) = web_sys::window()
        && let Ok(send) = js_sys::Reflect::get(&window, &"reviewTimeline".into())
        && let Some(send) = send.dyn_ref::<js_sys::Function>()
    {
        let _ = send.call1(&JsValue::UNDEFINED, request);
    }
}

#[cfg(target_arch = "wasm32")]
fn set(object: &js_sys::Object, key: &str, value: &wasm_bindgen::JsValue) {
    let _ = js_sys::Reflect::set(object, &key.into(), value);
}

/// Ask the shell for the timeline of `slug` at `revision`, drawn in a box `heights` tall.
#[cfg(target_arch = "wasm32")]
fn ask(owner: f64, slug: &str, revision: f64, (narrow, wide): (u32, u32)) {
    let request = js_sys::Object::new();
    set(&request, "owner", &owner.into());
    set(&request, "slug", &slug.into());
    set(&request, "revision", &revision.into());
    set(&request, "narrow", &narrow.into());
    set(&request, "wide", &wide.into());
    send(&request);
}

/// Tell the shell the box `owner` is gone: the module draws nothing and reads no more.
#[cfg(target_arch = "wasm32")]
fn close(owner: f64) {
    let request = js_sys::Object::new();
    set(&request, "kind", &"close".into());
    set(&request, "owner", &owner.into());
    send(&request);
}

/// Put [`STYLE`] in the page once.
#[cfg(target_arch = "wasm32")]
fn put_style() {
    const ID: &str = "timeline-slot-style";
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.get_element_by_id(ID).is_some() {
        return;
    }
    if let (Ok(style), Some(head)) = (document.create_element("style"), document.head()) {
        style.set_id(ID);
        style.set_text_content(Some(STYLE));
        let _ = head.append_child(&style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_at_most_240_px_tall_on_a_phone_and_wide() {
        assert_eq!((NARROW, WIDE), (240, 240));
        // Nothing kept, nor a height kept by a build whose box was taller, reserves more.
        assert_eq!(heights(Some("460,480")), (240, 240));
        assert_eq!(kept(Some("460,480"), true, 2000.0), "240,240");
        assert_eq!(kept(Some("460,480"), false, 2000.0), "240,240");
    }

    #[test]
    fn a_kept_height_is_read_back_within_the_least_and_its_default() {
        assert_eq!(heights(None), (NARROW, WIDE));
        assert_eq!(heights(Some("")), (NARROW, WIDE));
        assert_eq!(heights(Some("200,150")), (200, 150));
        assert_eq!(heights(Some(" 200 , 150 ")), (200, 150));
        assert_eq!(heights(Some("9999,10")), (NARROW, LEAST));
        assert_eq!(heights(Some("tall,150")), (NARROW, 150));
        assert_eq!(heights(Some("200")), (200, WIDE));
        assert_eq!(heights(Some(&format!("{NARROW},{WIDE}"))), (NARROW, WIDE));
    }

    #[test]
    fn a_draw_keeps_its_own_widths_height_and_the_other_as_it_was() {
        assert_eq!(kept(None, false, 212.4), format!("213,{WIDE}"));
        assert_eq!(kept(Some("213,150"), true, 180.0), "213,180");
        assert_eq!(kept(Some("213,150"), true, 2000.0), format!("213,{WIDE}"));
        assert_eq!(kept(Some("213,150"), false, 12.0), format!("{LEAST},150"));
        assert_eq!(kept(Some("213,150"), false, -5.0), format!("{LEAST},150"));
        assert_eq!(key("tools/a"), "timeline-height:tools/a");
    }

    #[test]
    fn a_taller_box_draws_more_placeholder_rows_never_none_nor_more_than_nine() {
        assert_eq!(placeholders(0), 1);
        assert_eq!(placeholders(LEAST), 1);
        assert_eq!(placeholders(NARROW), 3);
        assert_eq!(placeholders(5000), 9);
    }

    fn html(slug: &str) -> String {
        fn page(slug: String) -> Element {
            rsx! { TimelineSlot { slug, revision: 1.0 } }
        }
        let mut dom = VirtualDom::new_with_props(page, slug.to_string());
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn the_box_is_reserved_as_tall_as_the_timeline_will_be_with_its_placeholder_rows() {
        let html = html("tools/new");
        assert!(
            html.contains("<h2 id=\"timeline-title\">Timeline</h2>"),
            "{html}"
        );
        assert!(
            html.contains(&format!(
                "id=\"{SLOT}\" data-slug=\"tools/new\" style=\"--tl-narrow:{NARROW}px;--tl-wide:{WIDE}px\""
            )),
            "{html}"
        );
        assert_eq!(
            html.matches("class=\"tl-sk-row\"").count(),
            placeholders(NARROW) as usize,
            "{html}"
        );
        // It says it reads, to assistive technology, inside the box's own height: no one-line
        // note the timeline then replaces.
        assert!(html.contains(">Reading the timeline…</span>"), "{html}");
        assert!(!html.contains("role=\"alert\""), "{html}");
    }

    #[test]
    fn the_boxs_style_holds_its_height_per_width_and_takes_down_the_placeholder_once_drawn() {
        assert!(
            STYLE.contains(".tl-box { height:var(--tl-narrow);"),
            "{STYLE}"
        );
        assert!(
            STYLE.contains("@media (min-width: 900px) { .tl-box { height:var(--tl-wide); } }"),
            "{STYLE}"
        );
        assert!(
            STYLE.contains(".tl-box:has(.tl-drawn) > .tl-skeleton { display:none; }"),
            "{STYLE}"
        );
        assert!(
            STYLE.contains(".tl-box:has(.tl-all) { height:auto; }"),
            "{STYLE}"
        );
        // A failed read never cuts off its Try again: the box grows past its height, never under.
        assert!(
            STYLE.contains(
                ".tl-box:has(.tl-failed) { height:auto; min-height:var(--tl-narrow); }\n@media (min-width: 900px) { .tl-box:has(.tl-failed) { min-height:var(--tl-wide); } }"
            ),
            "{STYLE}"
        );
        for written in ["#", "rgb(", "rgba(", "hsl("] {
            assert!(
                !STYLE.contains(written),
                "a colour written out ({written}): {STYLE}"
            );
        }
    }
}
