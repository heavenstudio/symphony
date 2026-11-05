//! Browser adapters for already mounted media: native-size frames, orbit controls and the
//! picture dialog. The shared visibility latch owns activation in media.rs and asks.rs.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use futures_channel::mpsc::{UnboundedSender, unbounded};
use futures_util::StreamExt;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element as DomElement, Event, MutationObserver, ResizeObserver};

use crate::media::local_asset;

// The request helpers serve the document page's asks alone.
#[cfg(feature = "page-docs")]
mod requests;
#[cfg(feature = "page-docs")]
pub(crate) use requests::{fetch, field, post, sleep};

const STYLE: Asset = asset!(
    "/assets/media.css",
    AssetOptions::css().with_static_head(true)
);
// Keep the 1.1 MB module out of dx's preload list and the board's initial requests.
const VIEWER: &str = "/static/model-viewer.min.js";

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Photo {
    src: String,
    caption: String,
}

enum Action {
    Open(Vec<Photo>, usize),
    Move(isize),
    Close,
}

/// The active page owns the media adapter, including media in the persistent conversation.
/// Documents also need its table observer without a picture to activate it. Elsewhere visibility
/// activates it once; a page-kind switch removes its observers before the replacement mounts.
/// Visible conversation media is picked up again on that mount.
pub fn use_activation() -> Signal<bool> {
    let mut needed = use_signal(|| cfg!(feature = "page-docs"));
    let listener = use_hook(move || {
        let (send, mut recv) = unbounded();
        spawn(async move {
            while recv.next().await.is_some() {
                if !*needed.peek() {
                    needed.set(true);
                }
            }
        });
        let listener = Rc::new(Closure::<dyn FnMut(Event)>::new(move |_| {
            let _ = send.unbounded_send(());
        }));
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "review:media",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
        listener
    });
    use_effect(move || {
        if let Some(root) = root() {
            each(
                &root,
                "#quick-root img, #quick-root model-viewer, #quick-root iframe",
                |node| {
                    let rect = node.get_bounding_client_rect();
                    if rect.width() > 0.0 && rect.height() > 0.0 {
                        needed.set(true);
                    }
                },
            );
        }
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "review:media",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    needed
}

/// Mount once beside the app's main region for native frames, models and picture lightboxes.
#[component]
pub fn MediaRuntime() -> Element {
    std::hint::black_box(&STYLE);
    let mut photos = use_signal(Vec::<Photo>::new);
    let mut at = use_signal(|| 0_usize);
    let sender = use_hook(|| {
        let (sender, mut receiver) = unbounded();
        spawn(async move {
            while let Some(action) = receiver.next().await {
                match action {
                    Action::Open(set, index) => {
                        at.set(index);
                        photos.set(set);
                    }
                    Action::Move(direction) => {
                        let len = photos.read().len();
                        if len > 0 {
                            at.set((at() as isize + direction).rem_euclid(len as isize) as usize);
                        }
                    }
                    Action::Close => photos.set(Vec::new()),
                }
            }
        });
        sender
    });
    #[cfg(not(feature = "overlays"))]
    let commands = sender.clone();
    let controller = use_hook(|| Rc::new(RefCell::new(None::<Controller>)));
    use_effect(move || {
        if controller.borrow().is_none() {
            *controller.borrow_mut() = Controller::new(sender.clone());
        }
    });
    #[cfg(not(feature = "overlays"))]
    {
        use crate::components::deferred::{close, property, show};
        let owner = format!("{}-pictures", crate::module::kind());
        let callback = use_hook(move || {
            Rc::new(Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
                let direction = value.as_f64().unwrap_or_default() as isize;
                let _ = commands.unbounded_send(if direction == 0 {
                    Action::Close
                } else {
                    Action::Move(direction)
                });
            }))
        });
        let dropped = owner.clone();
        use_drop(move || close(&dropped));
        use_effect(move || {
            if photos.read().is_empty() {
                close(&owner);
                return;
            }
            let request = js_sys::Object::new();
            property(&request, "kind", &"picture".into());
            property(&request, "owner", &owner.clone().into());
            property(&request, "at", &JsValue::from_f64(at() as f64));
            if let Ok(photos) = serde_wasm_bindgen::to_value(&*photos.read()) {
                property(&request, "photos", &photos);
            }
            property(&request, "callback", callback.as_ref().as_ref());
            show(&request);
        });
        rsx! {}
    }
    #[cfg(feature = "overlays")]
    rsx! { PictureOverlay { photos: photos(), at: at(), command: move |direction: isize| {
        if direction == 0 { photos.set(Vec::new()); }
        else if !photos.peek().is_empty() { at.set((at() as isize + direction).rem_euclid(photos.peek().len() as isize) as usize); }
    } } }
}

#[cfg(feature = "overlays")]
#[component]
pub(crate) fn PictureOverlay(photos: Vec<Photo>, at: usize, command: Callback<isize>) -> Element {
    let shown = photos.get(at).cloned();
    let multiple = photos.len() > 1;
    let opened = use_memo(use_reactive!(|photos| Some(!photos.is_empty())));
    rsx! {
        crate::components::dialog::Dialog {
            id: "media-lightbox", "aria-label": "Picture viewer",
            open: opened,
            on_open_change: move |open: bool| if !open { command.call(0); },
            if let Some(photo) = shown {
                div { class: "lightbox-controls",
                    if multiple {
                        crate::components::button::Button { r#type: "button", "aria-label": "Previous picture", actions: crate::entry_events::click(move || command.call(-1)), "←" }
                    }
                    crate::components::button::Button { r#type: "button", actions: crate::entry_events::click(move || command.call(0)), "Close" }
                    if multiple {
                        crate::components::button::Button { r#type: "button", "aria-label": "Next picture", actions: crate::entry_events::click(move || command.call(1)), "→" }
                    }
                }
                img { src: photo.src, alt: photo.caption.clone() }
                div { class: "lightbox-caption",
                    "{photo.caption}"
                    if multiple { " · {at + 1} / {photos.len()}" }
                }
            }
        }
    }
}

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn each(root: &DomElement, selector: &str, mut visit: impl FnMut(DomElement)) {
    if let Ok(nodes) = root.query_selector_all(selector) {
        for index in 0..nodes.length() {
            if let Some(element) = nodes.item(index).and_then(|n| n.dyn_into().ok()) {
                visit(element);
            }
        }
    }
}

fn root() -> Option<DomElement> {
    document()?.document_element()
}

fn scale_frame(stage: &DomElement) {
    let width = stage.get_bounding_client_rect().width();
    if width > 0.0
        && let Ok(Some(frame)) = stage.query_selector("iframe")
    {
        let _ = frame.set_attribute("style", &format!("transform:scale({})", width / 1280.0));
    }
}

fn load_viewer() {
    let Some(doc) = document() else {
        return;
    };
    if doc.get_element_by_id("review-model-viewer").is_none()
        && let Ok(script) = doc.create_element("script")
    {
        script.set_id("review-model-viewer");
        let _ = script.set_attribute("type", "module");
        let _ = script.set_attribute("src", VIEWER);
        if let Some(head) = doc.head() {
            let _ = head.append_child(&script);
        }
    }
}

fn activate(stage: &DomElement) {
    let Some(src) = stage
        .get_attribute("data-media-src")
        .filter(|s| local_asset(s))
    else {
        return;
    };
    if stage.get_attribute("data-media-active").is_some() {
        return;
    }
    let _ = stage.set_attribute("data-media-active", "true");
    if stage.get_attribute("data-media-kind").as_deref() == Some("mockup") {
        if let Ok(Some(frame)) = stage.query_selector("iframe") {
            scale_frame(stage);
            let _ = frame.set_attribute("src", &src);
        }
        return;
    }
    let Some(doc) = document() else {
        return;
    };
    load_viewer();
    let Ok(model) = doc.create_element("model-viewer") else {
        return;
    };
    for (name, value) in [
        ("src", src.as_str()),
        (
            "alt",
            &stage.get_attribute("data-label").unwrap_or_default(),
        ),
        ("camera-controls", ""),
        ("touch-action", "pan-y"),
        ("environment-image", "neutral"),
        ("shadow-intensity", "0"),
        ("interaction-prompt", "none"),
        ("exposure", "1"),
        ("camera-orbit", "-45deg 60deg auto"),
        ("loading", "lazy"),
    ] {
        let _ = model.set_attribute(name, value);
    }
    stage.set_text_content(None);
    let _ = stage.append_child(&model);
}

fn scan(resize: &ResizeObserver, targets: &RefCell<Vec<DomElement>>) {
    targets.borrow_mut().retain(|target| {
        if target.is_connected() {
            true
        } else {
            resize.unobserve(target);
            false
        }
    });
    if let Some(root) = root() {
        #[cfg(feature = "page-docs")]
        {
            each(&root, ".doc-body table:not([data-toc-observed])", |table| {
                let _ = table.set_attribute("data-toc-observed", "true");
                targets.borrow_mut().push(table.clone());
                resize.observe(&table);
            });
            fold_contents(&root);
        }
        each(&root, ".mockup-frame:not([data-media-observed])", |stage| {
            let _ = stage.set_attribute("data-media-observed", "true");
            targets.borrow_mut().push(stage.clone());
            resize.observe(&stage);
            scale_frame(&stage);
        });
        if root.query_selector("model-viewer").ok().flatten().is_some() {
            load_viewer();
        }
        // The visibility latch mounts a section or native media shell. loading=lazy handles
        // raw HTML pictures/frames inside it; model-viewer also defers its own model requests.
        each(
            &root,
            "[data-media-src]:not([data-media-active])",
            |stage| {
                if stage
                    .closest("details:not([open])")
                    .ok()
                    .flatten()
                    .is_none()
                {
                    activate(&stage);
                }
            },
        );
        each(&root, "main img:not([data-zoom-ready])", |image| {
            let _ = image.set_attribute("data-zoom-ready", "true");
            if zoomable(&image) && image.closest("a").ok().flatten().is_none() {
                if !image.has_attribute("tabindex") {
                    let _ = image.set_attribute("tabindex", "0");
                }
                if !image.has_attribute("role") {
                    let _ = image.set_attribute("role", "button");
                }
                let caption = image.get_attribute("alt").unwrap_or_default();
                if !image.has_attribute("aria-label") {
                    let _ = image.set_attribute("aria-label", &format!("Open picture {caption}"));
                }
            }
        });
    }
}

fn zoomable(image: &DomElement) -> bool {
    if image.has_attribute("data-no-lightbox")
        || image.get_attribute("aria-hidden").as_deref() == Some("true")
        || matches!(
            image.get_attribute("role").as_deref(),
            Some("presentation" | "none")
        )
    {
        return false;
    }
    image.closest("a[href]").ok().flatten().is_none_or(|link| {
        let href = link.get_attribute("href").unwrap_or_default();
        let path = href
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"]
            .iter()
            .any(|ext| path.ends_with(ext))
    })
}

fn photo(image: &DomElement) -> Option<Photo> {
    let src = image
        .get_attribute("data-full")
        .or_else(|| image.get_attribute("src"))?;
    Some(Photo {
        src,
        caption: image.get_attribute("alt").unwrap_or_default(),
    })
}

/// Bodies and image dimensions arrive lazily. Fold once when an actual table needs the width;
/// a click on the contents summary takes ownership away from this automatic adjustment.
#[cfg(feature = "page-docs")]
fn fold_contents(root: &DomElement) {
    each(root, ".doc-toc details:not([data-toc-chosen])", |toc| {
        let Some(layout) = toc.closest(".doc-layout").ok().flatten() else {
            return;
        };
        let Ok(tables) = layout.query_selector_all(".doc-body table") else {
            return;
        };
        let cramped = (0..tables.length()).any(|index| {
            tables
                .item(index)
                .and_then(|node| node.dyn_into::<DomElement>().ok())
                .is_some_and(|table| {
                    table.client_width() > 0 && table.scroll_width() > table.client_width() + 2
                })
        });
        if cramped && let Ok(details) = toc.dyn_into::<web_sys::HtmlDetailsElement>() {
            let _ = details.set_attribute("data-toc-chosen", "automatic");
            details.set_open(false);
        }
    });
}

fn click(event: &Event, sender: &UnboundedSender<Action>) {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<DomElement>().ok()) else {
        return;
    };
    #[cfg(feature = "page-docs")]
    if let Ok(Some(summary)) = target.closest(".doc-toc summary")
        && let Some(details) = summary.parent_element()
    {
        let _ = details.set_attribute("data-toc-chosen", "manual");
    }
    if target.id() == "media-lightbox" {
        let _ = sender.unbounded_send(Action::Close);
        return;
    }
    if let Ok(Some(button)) = target.closest("[data-glb-view]") {
        if let Some(model) = button
            .closest(".glb-viewer")
            .ok()
            .flatten()
            .and_then(|v| v.query_selector("model-viewer").ok().flatten())
        {
            let tilt = button.get_attribute("data-glb-view").as_deref() == Some("tilt");
            let _ = model.set_attribute(
                "camera-orbit",
                if tilt {
                    "0deg 25deg auto"
                } else {
                    "-45deg 60deg auto"
                },
            );
            let _ = model.set_attribute("field-of-view", if tilt { "30deg" } else { "auto" });
            if let Ok(method) = js_sys::Reflect::get(&model, &JsValue::from_str("jumpCameraToGoal"))
                && let Some(method) = method.dyn_ref::<js_sys::Function>()
            {
                let _ = method.call0(&model);
            }
        }
        return;
    }
    if event
        .dyn_ref::<web_sys::MouseEvent>()
        .is_some_and(|e| e.ctrl_key() || e.meta_key() || e.shift_key() || e.button() != 0)
    {
        return;
    }
    let Some(image) = target.closest("main img").ok().flatten().filter(zoomable) else {
        return;
    };
    let card = image.closest(".decision").ok().flatten();
    let scope = card
        .and_then(|card| {
            if image.closest(".history").ok().flatten().is_none() {
                card.query_selector(".compare")
                    .ok()
                    .flatten()
                    .or(Some(card))
            } else {
                Some(card)
            }
        })
        .or_else(|| image.closest("main").ok().flatten());
    let Some(scope) = scope else {
        return;
    };
    let mut photos = Vec::new();
    let mut at = None;
    each(&scope, "img", |other| {
        if zoomable(&other)
            && let Some(photo) = photo(&other)
        {
            if other == image {
                at = Some(photos.len());
            }
            photos.push(photo);
        }
    });
    let at = at.unwrap_or_else(|| {
        photos.clear();
        photos.extend(photo(&image));
        0
    });
    if !photos.is_empty() {
        event.prevent_default();
        let _ = sender.unbounded_send(Action::Open(photos, at));
    }
}

/// Every observer and listener belongs to the mounted app and is disconnected on unmount.
struct Controller {
    resize: ResizeObserver,
    mutation: MutationObserver,
    _resized: Closure<dyn FnMut(js_sys::Array)>,
    _changed: Closure<dyn FnMut()>,
    click: Closure<dyn FnMut(Event)>,
    keys: Closure<dyn FnMut(web_sys::KeyboardEvent)>,
    toggle: Closure<dyn FnMut(Event)>,
    #[cfg(feature = "page-docs")]
    scroll: Closure<dyn FnMut(Event)>,
}

impl Controller {
    fn new(sender: UnboundedSender<Action>) -> Option<Self> {
        let resized = Closure::new(move |entries: js_sys::Array| {
            for entry in entries
                .iter()
                .filter_map(|e| e.dyn_into::<web_sys::ResizeObserverEntry>().ok())
            {
                scale_frame(&entry.target());
            }
            #[cfg(feature = "page-docs")]
            if let Some(root) = root() {
                fold_contents(&root);
            }
        });
        let resize = ResizeObserver::new(resized.as_ref().unchecked_ref()).ok()?;
        let r = resize.clone();
        let targets = Rc::new(RefCell::new(Vec::new()));
        let tracked = targets.clone();
        let changed = Closure::new(move || scan(&r, &tracked));
        let mutation = MutationObserver::new(changed.as_ref().unchecked_ref()).ok()?;
        let options = web_sys::MutationObserverInit::new();
        options.set_child_list(true);
        options.set_subtree(true);
        let root = root()?;
        mutation.observe_with_options(&root, &options).ok()?;
        let clicked = sender.clone();
        let click = Closure::new(move |event: Event| click(&event, &clicked));
        let keys = Closure::new(move |event: web_sys::KeyboardEvent| {
            if document()
                .and_then(|d| d.get_element_by_id("media-lightbox"))
                .is_some_and(|d| d.get_attribute("data-state").as_deref() == Some("open"))
            {
                let action = match event.key().as_str() {
                    "Escape" => Action::Close,
                    "ArrowLeft" => Action::Move(-1),
                    "ArrowRight" => Action::Move(1),
                    _ => return,
                };
                event.prevent_default();
                let _ = sender.unbounded_send(action);
            } else if matches!(event.key().as_str(), "Enter" | " ") && !event.repeat() {
                self::click(event.as_ref(), &sender);
            }
        });
        let doc = document()?;
        let resized_observer = resize.clone();
        let tracked = targets.clone();
        let toggle = Closure::new(move |event: Event| {
            let Some(details) = event
                .target()
                .and_then(|e| e.dyn_into::<DomElement>().ok())
                .filter(|e| e.tag_name() == "DETAILS")
            else {
                return;
            };
            if details.has_attribute("open") {
                let _ = details.set_attribute("data-busy", "true");
                scan(&resized_observer, &tracked);
            } else {
                let _ = details.remove_attribute("data-busy");
                each(&details, "[data-media-kind=model]", |stage| {
                    stage.set_text_content(Some("3D model · drag to orbit"));
                    let _ = stage.remove_attribute("data-media-active");
                });
            }
        });
        doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())
            .ok()?;
        doc.add_event_listener_with_callback("keydown", keys.as_ref().unchecked_ref())
            .ok()?;
        doc.add_event_listener_with_callback_and_bool(
            "toggle",
            toggle.as_ref().unchecked_ref(),
            true,
        )
        .ok()?;
        #[cfg(feature = "page-docs")]
        let scroll = Closure::new(move |_event: Event| {
            if let Some(root) = self::root() {
                fold_contents(&root);
            }
        });
        #[cfg(feature = "page-docs")]
        doc.add_event_listener_with_callback("scroll", scroll.as_ref().unchecked_ref())
            .ok()?;
        scan(&resize, &targets);
        Some(Self {
            resize,
            mutation,
            _resized: resized,
            _changed: changed,
            click,
            keys,
            toggle,
            #[cfg(feature = "page-docs")]
            scroll,
        })
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.resize.disconnect();
        self.mutation.disconnect();
        if let Some(doc) = document() {
            #[cfg(feature = "page-docs")]
            let _ = doc.remove_event_listener_with_callback(
                "scroll",
                self.scroll.as_ref().unchecked_ref(),
            );
            let _ = doc
                .remove_event_listener_with_callback("click", self.click.as_ref().unchecked_ref());
            let _ = doc
                .remove_event_listener_with_callback("keydown", self.keys.as_ref().unchecked_ref());
            let _ = doc.remove_event_listener_with_callback_and_bool(
                "toggle",
                self.toggle.as_ref().unchecked_ref(),
                true,
            );
        }
        if let Some(root) = root() {
            each(&root, "[data-media-observed]", |stage| {
                let _ = stage.remove_attribute("data-media-observed");
            });
        }
    }
}
