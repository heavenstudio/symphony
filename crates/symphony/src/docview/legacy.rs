//! A fragment as string HTML, for the pages that read sections and history as markup: each media
//! item written in place of its placeholder, a viewer as its element, buttons and download link.

use symphony_api::{Fragment, Media};

/// Existing pages consume string HTML from section/history reads. The SPA requests
/// `format=fragments` (or the document snapshot) for structured media instead.
pub fn legacy_html(fragment: &Fragment) -> String {
    let mut html = fragment.html.clone();
    for (index, item) in fragment.media.iter().enumerate() {
        let (kind, replacement) = match item {
            Media::Viewer {
                src,
                label,
                poster,
                viewer,
            } => ("viewer", viewer_html(viewer, src, label, poster.as_deref())),
            Media::Mockup { src, label } => (
                "mockup",
                format!(
                    "<span class=\"mockup\" data-w=\"1280\" data-h=\"800\"><span class=\"mockup-frame\"><iframe src=\"{0}\" sandbox=\"allow-scripts\" loading=\"lazy\" width=\"1280\" height=\"800\" title=\"{1}\" tabindex=\"-1\"></iframe><a class=\"mockup-open\" href=\"{0}\" target=\"_blank\" rel=\"noopener\" aria-label=\"Open {1} full size\"></a></span><span class=\"mockup-bar\"><span class=\"mockup-name\">{1}</span><a href=\"{0}\" target=\"_blank\" rel=\"noopener\">full size</a></span></span>",
                    crate::markup::escape(src),
                    crate::markup::escape(label)
                ),
            ),
            Media::Gallery { items, .. } => (
                "gallery",
                items
                    .iter()
                    .map(|item| {
                        format!(
                            "<a href=\"{}\" download>{}</a>",
                            crate::markup::escape(&item.src),
                            crate::markup::escape(&item.name)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
        };
        html = html.replace(
            &format!("<span class=\"media\" data-media=\"{index}\" data-kind=\"{kind}\"></span>"),
            &replacement,
        );
    }
    html
}

/// A file in its viewer, as string HTML: the element in its stage (which names the script that
/// defines it), then its caption, a button for each view and the download link.
fn viewer_html(
    viewer: &symphony_api::Viewer,
    src: &str,
    label: &str,
    poster: Option<&str>,
) -> String {
    let escape = crate::markup::escape;
    let element = viewer.element_html(src, label, poster).unwrap_or_default();
    let views: String = viewer
        .views
        .iter()
        .map(|view| {
            let call = view.call.as_deref().map_or(String::new(), |call| {
                format!(" data-viewer-call=\"{}\"", escape(call))
            });
            let title = view.title.as_deref().map_or(String::new(), |title| {
                format!(" title=\"{}\"", escape(title))
            });
            format!(
                "<button type=\"button\" data-viewer-set=\"{}\"{call}{title}>{}</button>",
                escape(&view.encoded()),
                escape(&view.label)
            )
        })
        .collect();
    let download = viewer.download.as_deref().map_or(String::new(), |text| {
        format!("<a href=\"{}\" download>{}</a>", escape(src), escape(text))
    });
    format!(
        "<span class=\"media-viewer\"><span class=\"viewer-stage\" data-viewer-script=\"{}\">\
         {element}</span><span class=\"viewer-bar\"><span class=\"viewer-name\">{}</span>{views}\
         {download}</span></span>",
        escape(&viewer.script),
        escape(label)
    )
}
