//! Native media views shared by the board, typed documents, asks and model galleries.

use dioxus::prelude::*;
use symphony_api::{GalleryItem, Image, Media};

const STYLE: Asset = asset!(
    "/assets/media.css",
    AssetOptions::css().with_static_head(true)
);

fn use_runtime(shown: Signal<bool>) {
    std::hint::black_box(&STYLE);
    #[cfg(target_arch = "wasm32")]
    use_effect(move || {
        if shown()
            && let Some(window) = web_sys::window()
            && let Ok(event) = web_sys::Event::new("review:media")
        {
            let _ = window.dispatch_event(&event);
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = shown;
}

/// Only project assets may run in an embedded frame or model viewer.
pub fn local_asset(src: &str) -> bool {
    src.starts_with('/')
        && !src.starts_with("//")
        && !src.chars().any(|c| c.is_control() || c == '\\')
}

pub fn is_mockup(src: &str) -> bool {
    let path = src
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    path.ends_with(".html") || path.ends_with(".htm")
}

fn label<'a>(src: &'a str, caption: &'a str) -> &'a str {
    if caption.is_empty() {
        src.split(['?', '#'])
            .next()
            .unwrap_or(src)
            .rsplit('/')
            .next()
            .unwrap_or(src)
    } else {
        caption
    }
}

#[component]
pub fn MockupFrame(src: String, caption: String, #[props(default)] thumb: bool) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    use_runtime(shown);
    if !local_asset(&src) || !is_mockup(&src) {
        return rsx! {};
    }
    let caption = label(&src, &caption).to_string();
    rsx! {
        span { class: if thumb { "mockup-shell thumb" } else { "mockup-shell" },
            ..reveal,
            if shown() {
                span { class: if thumb { "mockup mockup-thumb" } else { "mockup" },
                    span { class: "mockup-frame",
                        iframe { src: src.clone(), "sandbox": "allow-scripts", "loading": "lazy", width: "1280", height: "800", title: caption.clone(), tabindex: "-1", referrerpolicy: "no-referrer" }
                        if !thumb { a { class: "mockup-open", href: src.clone(), target: "_blank", rel: "noopener noreferrer", "aria-label": "Open {caption} full size" } }
                    }
                    if !thumb {
                        span { class: "mockup-bar", span { "{caption}" } a { href: src.clone(), target: "_blank", rel: "noopener noreferrer", "full size" } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn ModelViewer(
    src: String,
    caption: String,
    #[props(default)] poster: Option<String>,
) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    use_runtime(shown);
    if !local_asset(&src) {
        return rsx! {};
    }
    let caption = label(&src, &caption).to_string();
    let poster = poster.filter(|poster| local_asset(poster));
    rsx! {
        span { class: "model-shell",
            ..reveal,
            if shown() {
                span { class: "glb-viewer",
                    model-viewer { "src": src.clone(), "alt": caption.clone(), "camera-controls": "", "touch-action": "pan-y", "environment-image": "neutral", "shadow-intensity": "0", "interaction-prompt": "none", "exposure": "1", "camera-orbit": "-45deg 60deg auto", "loading": "lazy", "poster": poster }
                    span { class: "glb-bar", span { "{caption}" }
                        crate::components::button::Button { r#type: "button", "data-glb-view": "tilt", "Top-down tilt" }
                        crate::components::button::Button { r#type: "button", "data-glb-view": "reset", "Reset" }
                        a { href: src.clone(), download: true, ".glb" }
                    }
                }
            }
        }
    }
}

/// The typed document block renderer calls this for every media item.
#[component]
pub fn MediaView(media: Media) -> Element {
    match media {
        Media::Mockup { src, label } => rsx! { MockupFrame { src, caption: label } },
        Media::Model { src, label, poster } => rsx! { ModelViewer { src, caption: label, poster } },
        Media::Gallery { items } => rsx! { Gallery { items } },
    }
}

/// Board and gallery images reserve a square, matching their existing contain layout. Document
/// pictures have their real dimensions and thumbnail URL supplied in the API's fragment HTML.
#[component]
pub fn PictureView(src: String, caption: String, #[props(default)] thumb: bool) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    use_runtime(shown);
    if is_mockup(&src) {
        return rsx! { MockupFrame { src, caption, thumb } };
    }
    rsx! {
        span { class: if thumb { "picture-shell thumb" } else { "picture-shell" },
            ..reveal,
            if shown() {
                img { class: "media-picture", src, alt: caption, width: "540", height: "540", loading: "lazy", decoding: "async" }
            }
        }
    }
}

/// A document image preserves the server's sizing, thumbnail original and presentation. The
/// node reserves its real box before visibility; only its src waits for the shared latch.
#[component]
pub fn DocumentPicture(image: Image) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    use_runtime(shown);
    let attrs = &image.presentation.attributes;
    rsx! {
        img {
            src: shown().then(|| image.src.clone()), alt: image.alt.clone(),
            class: image.presentation.class.clone(), id: image.presentation.id.clone(), title: image.presentation.title.clone(),
            width: attrs.get("width").cloned(), height: attrs.get("height").cloned(),
            loading: attrs.get("loading").cloned().unwrap_or_else(|| "lazy".into()),
            decoding: attrs.get("decoding").cloned().unwrap_or_else(|| "async".into()),
            style: attrs.get("style").cloned(), role: attrs.get("role").cloned(), tabindex: attrs.get("tabindex").cloned(),
            "aria-label": attrs.get("aria-label").cloned(), "aria-hidden": attrs.get("aria-hidden").cloned(),
            "data-full": attrs.get("data-full").cloned(),
            ..reveal,
        }
    }
}

/// Models to pick from: a card each, one open at a time.
#[component]
fn Gallery(items: Vec<GalleryItem>) -> Element {
    let mut selected = use_signal(|| None::<String>);
    rsx! {
        div { class: "gallery-grid",
            for item in items {
                GalleryRow { key: "{item.name}", open: selected().as_deref() == Some(item.name.as_str()),
                    on_select: move |name| selected.set(if selected().as_ref() == Some(&name) { None } else { Some(name) }), item }
            }
        }
    }
}

#[component]
fn GalleryRow(item: GalleryItem, open: bool, on_select: EventHandler<String>) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    let name = item.name.clone();
    rsx! {
        article { class: if open { "gallery-card gallery-open" } else { "gallery-card" }, "data-busy": open,
            ..reveal,
            if shown() {
                crate::components::button::Button { class: "gallery-pick", r#type: "button", "aria-expanded": open, "aria-controls": "model-{item.name}", onclick: move |_| on_select.call(name.clone()),
                    if let Some(poster) = item.poster {
                        img { src: poster, width: "80", height: "80", alt: item.name.clone(), loading: "lazy", "data-no-lightbox": "true" }
                    }
                    span { strong { "{item.name}" } small { if open { "Close model" } else { "Open 3D model" } } }
                }
                if open {
                    div { id: "model-{item.name}", ModelViewer { src: item.src, caption: item.name.clone() } }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_media_markup_keeps_scripts_only_frames_and_selection_only_models() {
        let mockup = dioxus_ssr::render_element(rsx! {
            PictureView { src: "/ui/hud.HTML?v=2", caption: "Flight HUD" }
        });
        for attribute in [
            "sandbox=\"allow-scripts\"",
            "width=\"1280\"",
            "height=\"800\"",
            "rel=\"noopener noreferrer\"",
        ] {
            assert!(mockup.contains(attribute), "{mockup}");
        }
        assert!(!mockup.contains("allow-same-origin"));
        let thumb = dioxus_ssr::render_element(rsx! {
            MockupFrame { src: "/ui/hud.html", caption: "", thumb: true }
        });
        assert!(thumb.contains("mockup-thumb"));
        assert!(!thumb.contains("mockup-open"));
        let picture = dioxus_ssr::render_element(rsx! {
            PictureView { src: "/picture.png", caption: "Picture", thumb: true }
        });
        assert!(picture.contains("<img") && picture.contains("loading=\"lazy\""));
        let media = Media::Gallery {
            items: vec![
                GalleryItem {
                    name: "FIRST".into(),
                    src: "/first.glb".into(),
                    poster: Some("/first.png".into()),
                },
                GalleryItem {
                    name: "SECOND".into(),
                    src: "/second.glb".into(),
                    poster: None,
                },
            ],
        };
        let gallery = dioxus_ssr::render_element(rsx! { MediaView { media } });
        // the component above the gallery says how many models there are; the gallery does not
        assert!(
            !gallery.contains("models.") && gallery.contains("class=\"gallery-grid\""),
            "{gallery}"
        );
        assert_eq!(
            gallery.matches("<article class=\"gallery-card\"").count(),
            2,
            "{gallery}"
        );
        assert!(gallery.contains("FIRST") && gallery.contains("/first.png"));
        assert!(!gallery.contains("model-viewer") && !gallery.contains("/first.glb"));
        let poster = Some("/m.png".to_string());
        let with = dioxus_ssr::render_element(
            rsx! { ModelViewer { src: "/m.glb", caption: "M", poster } },
        );
        let foreign = Some("https://x/m.png".to_string());
        let without = dioxus_ssr::render_element(
            rsx! { ModelViewer { src: "/m.glb", caption: "M", poster: foreign } },
        );
        assert!(with.contains("poster=\"/m.png\""), "{with}");
        assert!(!without.contains("poster="), "{without}");
        for (media, tag) in [
            (
                Media::Model {
                    src: "/model.glb".into(),
                    label: "Model".into(),
                    poster: None,
                },
                "<model-viewer",
            ),
            (
                Media::Mockup {
                    src: "/ui/a.html".into(),
                    label: "Frame".into(),
                },
                "<iframe",
            ),
        ] {
            let html = dioxus_ssr::render_element(rsx! { MediaView { media } });
            assert!(html.contains(tag), "expected {tag}: {html}");
        }
    }

    #[test]
    fn executable_media_accepts_only_local_absolute_paths() {
        for src in ["/ui/hud.html", "/@topic/model.glb?v=2#view", "/a/b.HTM"] {
            assert!(local_asset(src), "{src}");
        }
        for src in [
            "",
            "ui/hud.html",
            "//example.com/a.html",
            "https://example.com/a.html",
            "javascript:alert(1)",
            "data:text/html,hi",
            "/\\example.com/a.html",
            "/a\n.html",
            "/a\u{0}.glb",
        ] {
            assert!(!local_asset(src), "{src:?}");
        }
    }

    #[test]
    fn mockup_detection_uses_the_extension_and_labels_exclude_url_metadata() {
        for src in ["/a.html", "/a.HTM?version=1", "/a.HTML#view"] {
            assert!(is_mockup(src), "{src}");
        }
        for src in ["/a.png?x=.html", "/a.glb#x.html", "/html", ""] {
            assert!(!is_mockup(src), "{src}");
        }
        assert_eq!(label("/ui/hud.html?v=2#frame", ""), "hud.html");
        assert_eq!(label("/ui/hud.html", "Flight HUD"), "Flight HUD");
    }

    #[test]
    fn unsafe_executable_urls_do_not_mount_and_document_pictures_keep_original_metadata() {
        for src in ["//external/model.glb", "https://external/model.glb"] {
            let html = dioxus_ssr::render_element(rsx! { ModelViewer { src, caption:"bad" } });
            assert!(!html.contains("<model-viewer"), "{html}");
        }
        let html =
            dioxus_ssr::render_element(rsx! { MockupFrame { src:"/picture.svg", caption:"bad" } });
        assert!(!html.contains("<iframe"), "{html}");
        let mut image: Image = serde_json::from_value(serde_json::json!({"src":"/thumb.png","alt":"Hull","presentation":{"attributes":{"width":"640","height":"480","data-full":"/original.png"}}})).unwrap();
        image.presentation.id = "picture".into();
        let html = dioxus_ssr::render_element(rsx! { DocumentPicture { image } });
        for attr in [
            "width=\"640\"",
            "height=\"480\"",
            "data-full=\"/original.png\"",
            "id=\"picture\"",
            "alt=\"Hull\"",
        ] {
            assert!(html.contains(attr), "{html}");
        }
    }
}
