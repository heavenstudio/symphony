//! One attach control for every place the owner writes: the + panel's chat and new task, a
//! document's asks and follow-ups, a card's message, verdict note or comment, and a task's thread.
//!
//! A composer takes [`use_attach`] with a key of its own, marks its form `data-attach` with
//! [`Attach::zone`] and draws [`AttachControls`] inside it. A file reaches it three ways: the
//! picker, a paste (a screenshot on the clipboard) or a drop anywhere on the form. Each is sent
//! to [`symphony_api::UPLOAD_PATH`] at once and shown as a chip; the post carries one line per file
//! ([`compose`]), which is how the posted item keeps showing it. The chips are saved with the
//! draft, so a reload keeps them.
//!
//! Only the panel, document and task page kinds compile this module: the `/` and `/tasks`
//! modules never carry it, so the first visit's bytes do not move. Its style comes with it, put
//! in the page by the first composer to mount ([`STYLE`]). One document listener per module
//! routes paste, drop and the picker's change to the composer whose zone holds the event's
//! target; a browser callback never writes a signal, it sends on the composer's channel.

use dioxus::prelude::*;
use symphony_api::Upload;

/// The control's style. Not a stylesheet asset: the shell names every kind's stylesheets, and
/// so would carry this one's name on every first visit, `/` and `/tasks` included.
pub const STYLE: &str = include_str!("../assets/attach.css");

/// The most files one post carries.
pub const MAX_FILES: usize = 10;

/// A composer's attachments: the files stored so far, the ones on their way, the last refusal.
#[derive(Clone, Copy, PartialEq)]
pub struct Attach {
    key: Signal<String>,
    files: Signal<Vec<Upload>>,
    pending: Signal<Vec<String>>,
    trouble: Signal<Option<String>>,
}

impl Attach {
    /// The `data-attach` value of the composer's form: where a paste or a drop lands.
    pub fn zone(&self) -> String {
        self.key.read().clone()
    }

    /// Whether a file is still on its way: the post waits for it.
    pub fn busy(&self) -> bool {
        !self.pending.read().is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.files.read().is_empty()
    }

    /// `text` with a line for each attached file.
    pub fn compose(&self, text: &str) -> String {
        compose(text, &self.files.read())
    }

    /// After a post: the files went with it.
    pub fn clear(&mut self) {
        self.files.set(Vec::new());
        self.trouble.set(None);
        self.save();
    }

    fn remove(&mut self, url: &str) {
        self.files.write().retain(|file| file.url != url);
        self.save();
    }

    fn save(&self) {
        crate::platform::current().save(&storage_key(&self.key.read()), &saved(&self.files.read()));
    }
}

fn storage_key(key: &str) -> String {
    format!("review-attach:{key}")
}

/// The draft's files as the browser keeps them: one `url\tname\tpicture\tbytes\tmarkup` row each,
/// so the host's tests and the browser need no JSON parser.
fn saved(files: &[Upload]) -> String {
    files
        .iter()
        .map(|f| {
            format!(
                "{}\t{}\t{}\t{}\t{}",
                f.url, f.name, f.picture, f.bytes, f.markup
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn restored(text: &str) -> Vec<Upload> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some(Upload {
                url: fields.next()?.into(),
                name: fields.next()?.into(),
                picture: fields.next()? == "true",
                bytes: fields.next()?.parse().ok()?,
                markup: fields.next()?.into(),
            })
        })
        .collect()
}

/// `text` followed, after a blank line, by one line per file: the line the server gave each
/// upload. Text with no files is sent as it was typed.
pub fn compose(text: &str, files: &[Upload]) -> String {
    if files.is_empty() {
        return text.to_string();
    }
    let lines = files
        .iter()
        .map(|file| file.markup.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim_end();
    if text.trim().is_empty() {
        lines
    } else {
        format!("{text}\n\n{lines}")
    }
}

/// A file bigger than the server takes, said before any byte is sent.
pub fn too_large(name: &str, bytes: f64) -> Option<String> {
    let max = symphony_api::UPLOAD_MAX_BYTES as f64;
    (bytes > max).then(|| {
        format!(
            "{name} is {:.1} MB; uploads are at most {} MB",
            bytes / (1024.0 * 1024.0),
            symphony_api::UPLOAD_MAX_BYTES / (1024 * 1024)
        )
    })
}

/// A composer's attachments, under `key` (unique among the composers mounted at once).
pub fn use_attach(key: String) -> Attach {
    let saved_key = storage_key(&key);
    let files = use_signal(move || {
        crate::platform::current()
            .load(&saved_key)
            .map(|text| restored(&text))
            .unwrap_or_default()
    });
    let attach = Attach {
        key: use_signal(move || key),
        files,
        pending: use_signal(Vec::new),
        trouble: use_signal(|| None),
    };
    #[cfg(target_arch = "wasm32")]
    {
        // The key is read at mount: the signals may be gone by the time the drop runs.
        let (key, id) = use_hook(move || (attach.zone(), runtime::start(attach)));
        use_drop(move || runtime::stop(&key, id));
    }
    attach
}

/// The picker, the chips of the files attached so far and what went wrong. Drawn inside the
/// composer's `data-attach` form.
#[component]
pub fn AttachControls(attach: Attach, #[props(default)] disabled: bool) -> Element {
    let files = attach.files.read().clone();
    let pending = attach.pending.read().clone();
    let full = files.len() + pending.len() >= MAX_FILES;
    rsx! {
        // While the composer sends, a paste or a drop is not taken: it would miss the post.
        div { class: "attach", "data-attach-off": disabled,
            label { class: if disabled || full { "attach-pick disabled" } else { "attach-pick" },
                title: "Attach pictures or documents: pick them, paste a screenshot or drop files here",
                crate::components::input::Input { r#type: "file", multiple: true, accept: symphony_api::UPLOAD_ACCEPT, disabled: disabled || full, "data-attach-input": "true" }
                span { "aria-hidden": "true", "📎" } " Attach"
            }
            span { class: "attach-hint", "or paste, or drop files here" }
            if !files.is_empty() || !pending.is_empty() {
                ul { class: "attach-files", "aria-label": "Attached files",
                    for file in files {
                        li { key: "{file.url}", class: "attach-file", "data-url": "{file.url}",
                            if file.picture {
                                img { src: "{file.url}", alt: "", width: 40, height: 40, loading: "lazy", decoding: "async" }
                            } else {
                                span { class: "attach-icon", "aria-hidden": "true", "📄" }
                            }
                            span { class: "attach-name", "{file.name}" }
                            crate::components::button::Button { r#type: "button", "aria-label": "Remove {file.name}", disabled,
                                onclick: { let mut attach = attach; let url = file.url.clone(); move |_| attach.remove(&url) },
                                "×"
                            }
                        }
                    }
                    for (index, name) in pending.into_iter().enumerate() {
                        li { key: "pending-{index}-{name}", class: "attach-file pending", role: "status",
                            span { class: "attach-name", "Uploading {name}…" }
                        }
                    }
                }
            }
            if let Some(why) = attach.trouble.read().clone() { p { class: "attach-trouble", role: "alert", "{why}" } }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod runtime {
    //! The browser's half: one set of document listeners per module, a channel per composer.

    use super::{Attach, MAX_FILES, too_large};
    use dioxus::prelude::*;
    use futures_channel::mpsc::{UnboundedSender, unbounded};
    use futures_util::StreamExt;
    use futures_util::future::{Either, select};
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use wasm_bindgen_futures::JsFuture;

    const EVENTS: [&str; 5] = ["paste", "dragover", "dragleave", "drop", "change"];

    type Listener = Closure<dyn FnMut(web_sys::Event)>;
    /// A mounted composer: its mount's id and the channel its files go on.
    type Zone = (u64, UnboundedSender<Vec<web_sys::File>>);

    thread_local! {
        static ZONES: RefCell<BTreeMap<String, Zone>> = const { RefCell::new(BTreeMap::new()) };
        static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) };
        static NEXT: Cell<u64> = const { Cell::new(0) };
    }

    /// Put [`super::STYLE`] in the page once: the panel's and the page's modules share it.
    fn style() {
        const ID: &str = "attach-style";
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        if document.get_element_by_id(ID).is_some() {
            return;
        }
        if let (Ok(style), Some(head)) = (document.create_element("style"), document.head()) {
            style.set_id(ID);
            style.set_text_content(Some(super::STYLE));
            let _ = head.append_child(&style);
        }
    }

    /// Register the composer's zone and drain its files, one upload at a time.
    pub(super) fn start(mut attach: Attach) -> u64 {
        style();
        let (send, mut receive) = unbounded::<Vec<web_sys::File>>();
        let id = NEXT.with(|next| {
            next.set(next.get() + 1);
            next.get()
        });
        ZONES.with(|zones| zones.borrow_mut().insert(attach.zone(), (id, send)));
        listen(true);
        spawn(async move {
            while let Some(batch) = receive.next().await {
                for file in batch {
                    let name = file.name();
                    if attach.files.peek().len() + attach.pending.peek().len() >= MAX_FILES {
                        attach
                            .trouble
                            .set(Some(format!("At most {MAX_FILES} files per post")));
                        break;
                    }
                    if let Some(why) = too_large(&name, file.size()) {
                        attach.trouble.set(Some(why));
                        continue;
                    }
                    attach.pending.write().push(name.clone());
                    let result = upload(&file).await;
                    {
                        let mut pending = attach.pending.write();
                        if let Some(at) = pending.iter().position(|p| *p == name) {
                            pending.remove(at);
                        }
                    }
                    match result {
                        Ok(stored) => {
                            if !attach.files.peek().iter().any(|f| f.url == stored.url) {
                                attach.files.write().push(stored);
                            }
                            attach.trouble.set(None);
                            attach.save();
                        }
                        Err(why) => attach.trouble.set(Some(format!("{name}: {why}"))),
                    }
                }
            }
        });
        id
    }

    /// A composer leaves; the last one takes the document listeners with it. A newer composer
    /// that took the same key keeps it.
    pub(super) fn stop(key: &str, id: u64) {
        let empty = ZONES.with(|zones| {
            let mut zones = zones.borrow_mut();
            if zones.get(key).is_some_and(|(owner, _)| *owner == id) {
                zones.remove(key);
            }
            zones.is_empty()
        });
        if empty {
            listen(false);
        }
    }

    fn listen(on: bool) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        LISTENER.with(|slot| {
            let mut slot = slot.borrow_mut();
            if on && slot.is_none() {
                let listener = Listener::new(handle);
                for name in EVENTS {
                    let _ = document
                        .add_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
                }
                *slot = Some(listener);
            } else if !on && let Some(listener) = slot.take() {
                for name in EVENTS {
                    let _ = document.remove_event_listener_with_callback(
                        name,
                        listener.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    fn files(list: Option<web_sys::FileList>) -> Vec<web_sys::File> {
        list.map(|list| (0..list.length()).filter_map(|i| list.item(i)).collect())
            .unwrap_or_default()
    }

    fn handle(event: web_sys::Event) {
        let Some(target) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let Some(zone) = target.closest("[data-attach]").ok().flatten() else {
            return;
        };
        let Some(key) = zone.get_attribute("data-attach") else {
            return;
        };
        if zone
            .query_selector("[data-attach-off=true]")
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        let Some(send) = ZONES.with(|zones| zones.borrow().get(&key).map(|(_, s)| s.clone()))
        else {
            return;
        };
        let carries_files = |transfer: &web_sys::DataTransfer| {
            transfer
                .types()
                .iter()
                .any(|kind| kind.as_string().as_deref() == Some("Files"))
        };
        let found = match event.type_().as_str() {
            "paste" => event
                .dyn_ref::<web_sys::ClipboardEvent>()
                .and_then(web_sys::ClipboardEvent::clipboard_data)
                .map(|data| files(data.files()))
                .unwrap_or_default(),
            "dragover" => {
                if event
                    .dyn_ref::<web_sys::DragEvent>()
                    .and_then(web_sys::DragEvent::data_transfer)
                    .is_some_and(|t| carries_files(&t))
                {
                    event.prevent_default();
                    let _ = zone.set_attribute("data-dropping", "true");
                }
                return;
            }
            "dragleave" => {
                let _ = zone.remove_attribute("data-dropping");
                return;
            }
            "drop" => {
                let _ = zone.remove_attribute("data-dropping");
                event
                    .dyn_ref::<web_sys::DragEvent>()
                    .and_then(web_sys::DragEvent::data_transfer)
                    .map(|data| files(data.files()))
                    .unwrap_or_default()
            }
            "change" => {
                let Some(input) = target
                    .dyn_ref::<web_sys::HtmlInputElement>()
                    .filter(|input| input.has_attribute("data-attach-input"))
                else {
                    return;
                };
                let picked = files(input.files());
                // The same file picked again must fire again.
                input.set_value("");
                picked
            }
            _ => return,
        };
        if found.is_empty() {
            return;
        }
        // A pasted picture is not text for the box, and a dropped file must not open in the tab.
        event.prevent_default();
        let _ = send.unbounded_send(found);
    }

    /// Send one file as the raw body; the server answers its stored line, or why it refused.
    async fn upload(file: &web_sys::File) -> Result<symphony_api::Upload, String> {
        let window = web_sys::window().ok_or("No window")?;
        let controller = web_sys::AbortController::new().map_err(crate::live::describe)?;
        let init = web_sys::RequestInit::new();
        init.set_method("POST");
        init.set_signal(Some(&controller.signal()));
        init.set_body(file.as_ref());
        let headers = web_sys::Headers::new().map_err(crate::live::describe)?;
        let kind = file.type_();
        headers
            .set(
                "Content-Type",
                if kind.is_empty() {
                    "application/octet-stream"
                } else {
                    &kind
                },
            )
            .map_err(crate::live::describe)?;
        init.set_headers(&headers);
        let url = format!(
            "{}?name={}",
            crate::project::current().here(symphony_api::UPLOAD_PATH),
            String::from(js_sys::encode_uri_component(&file.name()))
        );
        let operation = async {
            let response: web_sys::Response =
                JsFuture::from(window.fetch_with_str_and_init(&url, &init))
                    .await
                    .map_err(crate::live::describe)?
                    .unchecked_into();
            let value = JsFuture::from(response.json().map_err(crate::live::describe)?)
                .await
                .map_err(|_| format!("the server answered {}", response.status()))?;
            if !response.ok() {
                return Err(js_sys::Reflect::get(&value, &JsValue::from_str("error"))
                    .ok()
                    .and_then(|e| e.as_string())
                    .unwrap_or_else(|| format!("the server answered {}", response.status())));
            }
            serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
        };
        // A phone on a slow link sends 10 MB in well under two minutes.
        match select(
            std::pin::pin!(operation),
            std::pin::pin!(crate::live::sleep(120_000)),
        )
        .await
        {
            Either::Left((result, _)) => result,
            Either::Right(_) => {
                controller.abort();
                Err("no answer from the server; try again".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(name: &str, picture: bool) -> Upload {
        let url = format!(
            "/artifacts/uploads/{}.{}",
            "a".repeat(64),
            if picture { "png" } else { "pdf" }
        );
        Upload {
            markup: format!("{}[{name}]({url})", if picture { "!" } else { "" }),
            url,
            name: name.into(),
            picture,
            bytes: 12,
        }
    }

    #[test]
    fn a_post_carries_one_line_per_file_after_its_words() {
        let shot = upload("shot.png", true);
        let brief = upload("brief.pdf", false);
        assert_eq!(
            compose("  typed \n", &[]),
            "  typed \n",
            "no file: the text as typed"
        );
        assert_eq!(
            compose("Look at this\n\n", &[shot.clone(), brief.clone()]),
            format!("Look at this\n\n{}\n{}", shot.markup, brief.markup)
        );
        assert_eq!(
            compose(" \n ", std::slice::from_ref(&shot)),
            shot.markup,
            "files alone"
        );
    }

    #[test]
    fn the_saved_draft_reads_back_its_files() {
        let files = vec![upload("shot one.png", true), upload("brief.pdf", false)];
        assert_eq!(restored(&saved(&files)), files);
        assert!(restored("").is_empty());
        assert!(restored("broken\trow").is_empty());
    }

    #[test]
    fn a_file_past_the_cap_is_named_before_it_is_sent() {
        let max = symphony_api::UPLOAD_MAX_BYTES as f64;
        assert_eq!(too_large("a.png", max), None);
        assert_eq!(
            too_large("big.png", max * 1.5).as_deref(),
            Some("big.png is 15.0 MB; uploads are at most 10 MB")
        );
    }

    #[component]
    fn Shown(
        files: Vec<Upload>,
        pending: Vec<String>,
        trouble: Option<String>,
        disabled: bool,
    ) -> Element {
        let mut attach = use_attach("fixture".into());
        use_hook(move || {
            attach.files.set(files);
            attach.pending.set(pending);
            attach.trouble.set(trouble);
        });
        rsx! { form { "data-attach": attach.zone(), AttachControls { attach, disabled } } }
    }

    #[test]
    fn the_controls_offer_the_picker_and_show_each_file_and_upload() {
        let html = dioxus_ssr::render_element(rsx! {
            Shown { files: vec![upload("shot.png", true), upload("brief.pdf", false)], pending: vec!["next.png".to_string()], trouble: Some("old.svg is not a picture".to_string()), disabled: false }
        });
        assert!(html.contains("data-attach=\"fixture\""), "{html}");
        assert!(html.contains("type=\"file\""), "{html}");
        assert!(html.contains("multiple"), "{html}");
        assert!(
            html.contains(&format!("accept=\"{}\"", symphony_api::UPLOAD_ACCEPT)),
            "{html}"
        );
        assert!(html.contains("data-attach-input"), "{html}");
        assert_eq!(html.matches("class=\"attach-file\"").count(), 2, "{html}");
        assert!(
            html.contains(&format!(
                "<img src=\"/artifacts/uploads/{}.png\"",
                "a".repeat(64)
            )),
            "{html}"
        );
        assert!(html.contains("aria-label=\"Remove shot.png\""), "{html}");
        assert!(html.contains("aria-label=\"Remove brief.pdf\""), "{html}");
        assert!(html.contains("Uploading next.png…"), "{html}");
        assert!(html.contains("role=\"alert\""), "{html}");
        assert!(html.contains("old.svg is not a picture"), "{html}");
    }

    #[test]
    fn an_empty_or_full_composer_shows_no_chips_and_a_full_one_no_picker() {
        let html = dioxus_ssr::render_element(rsx! {
            Shown { files: vec![], pending: vec![], trouble: None, disabled: false }
        });
        assert!(!html.contains("attach-files"), "{html}");
        assert!(!html.contains("role=\"alert\""), "{html}");
        assert!(!html.contains("disabled"), "{html}");
        let full: Vec<_> = (0..MAX_FILES)
            .map(|i| upload(&format!("{i}.png"), true))
            .collect();
        let html = dioxus_ssr::render_element(rsx! {
            Shown { files: full, pending: vec![], trouble: None, disabled: false }
        });
        assert!(html.contains("attach-pick disabled"), "{html}");
    }
}
