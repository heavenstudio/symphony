//! A file in the viewer its plugin's renderer names: the custom element a project's script defines,
//! drawn from the [`Viewer`] data alone, with its caption, a button for each view and the download
//! link. The client holds no viewer's code: `media_live` loads the element's script once, when the
//! first stage naming it is mounted, and a view's button sets the attributes it lists on the element
//! and calls the method it names.

use dioxus::prelude::*;
use symphony_api::Viewer;

use crate::media::{label, local_asset, use_runtime};

#[component]
pub fn FileViewer(
    src: String,
    caption: String,
    viewer: Viewer,
    #[props(default)] poster: Option<String>,
) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    use_runtime(shown);
    if !local_asset(&src) || !local_asset(&viewer.script) {
        return rsx! {};
    }
    let caption = label(&src, &caption).to_string();
    let poster = poster.filter(|poster| local_asset(poster));
    let Some(element) = viewer.element_html(&src, &caption, poster.as_deref()) else {
        return rsx! {};
    };
    rsx! {
        span { class: "viewer-shell",
            ..reveal,
            if shown() {
                span { class: "media-viewer",
                    span { class: "viewer-stage", "data-viewer-script": viewer.script.clone(), dangerous_inner_html: element }
                    span { class: "viewer-bar", span { "{caption}" }
                        for view in viewer.views.iter() {
                            crate::components::button::Button { r#type: "button", title: view.title.clone(), "data-viewer-set": view.encoded(), "data-viewer-call": view.call.clone(), "{view.label}" }
                        }
                        if let Some(download) = viewer.download.clone() {
                            a { href: src.clone(), download: true, "{download}" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_api::View;

    fn viewer() -> Viewer {
        Viewer {
            element: "orbit-view".into(),
            script: "/static/orbit.js".into(),
            attributes: vec![("controls".into(), String::new())],
            views: vec![View {
                label: "Above".into(),
                title: Some("From above".into()),
                set: vec![
                    ("angle".into(), "0deg 25deg".into()),
                    ("zoom".into(), "2".into()),
                ],
                call: Some("settle".into()),
            }],
            download: Some(".item".into()),
            ..Viewer::default()
        }
    }

    fn drawn(src: &str, poster: Option<&str>, viewer: Viewer) -> String {
        let poster = poster.map(str::to_string);
        dioxus_ssr::render_element(rsx! { FileViewer { src, caption: "Item", viewer, poster } })
    }

    #[test]
    fn the_element_its_views_and_download_are_drawn_from_the_viewer_alone() {
        let html = drawn("/a.item", Some("/a.png"), viewer());
        for part in [
            "class=\"viewer-stage\" data-viewer-script=\"/static/orbit.js\"",
            "<orbit-view src=\"/a.item\" alt=\"Item\" controls=\"\" poster=\"/a.png\"></orbit-view>",
            "data-viewer-set=\"angle=0deg 25deg\nzoom=2\"",
            "data-viewer-call=\"settle\"",
            "title=\"From above\"",
            ">Above</button>",
            "<a href=\"/a.item\" download=true>.item</a>",
        ] {
            assert!(html.contains(part), "{part:?} missing: {html}");
        }
    }

    #[test]
    fn a_foreign_file_poster_or_script_and_a_bad_element_draw_nothing_or_no_poster() {
        for src in ["//external/a.item", "https://external/a.item"] {
            let html = drawn(src, None, viewer());
            assert!(!html.contains("<orbit-view"), "{src}: {html}");
        }
        let mut foreign = viewer();
        foreign.script = "https://external/orbit.js".into();
        let html = drawn("/a.item", None, foreign);
        assert!(!html.contains("<orbit-view"), "{html}");
        let mut bad = viewer();
        bad.element = "script".into();
        let html = drawn("/a.item", None, bad);
        assert!(!html.contains("media-viewer"), "{html}");
        let html = drawn("/a.item", Some("https://x/a.png"), viewer());
        assert!(
            html.contains("<orbit-view") && !html.contains("poster="),
            "{html}"
        );
        let mut plain = viewer();
        plain.download = None;
        plain.views.clear();
        let html = drawn("/a.item", None, plain);
        assert!(!html.contains("<a ") && !html.contains("<button"), "{html}");
    }
}
