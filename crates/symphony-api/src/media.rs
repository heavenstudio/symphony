//! What a page draws that markup cannot: an `.html` mockup, a file a plugin's renderer shows in a
//! viewer of its own, and a gallery of such files. A [`Viewer`] is data, the custom element a
//! project's script defines with its attributes and the buttons under it, so the client draws every
//! viewer with one component and holds no project's code.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Media {
    Mockup {
        src: String,
        label: String,
    },
    /// A file drawn by its renderer's viewer under its caption; `poster` is a picture shown before
    /// it loads.
    Viewer {
        src: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poster: Option<String>,
        viewer: Viewer,
    },
    /// Files to pick from, one open at a time, each in `viewer`.
    Gallery {
        items: Vec<GalleryItem>,
        #[serde(default)]
        viewer: Viewer,
    },
}

/// One file of a gallery. The poster URL already names a small copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GalleryItem {
    pub name: String,
    pub src: String,
    pub poster: Option<String>,
}

/// A custom element that draws a file, and what the client needs to show it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewer {
    /// The element's tag (`orbit-view`): a custom element's name, lower case with a hyphen.
    pub element: String,
    /// The module script that defines the element, loaded once, when the first viewer shows.
    pub script: String,
    /// Its attributes after `src` and `alt`, in order; an empty value is a bare attribute.
    pub attributes: Vec<(String, String)>,
    /// The buttons under it, in order.
    #[serde(default)]
    pub views: Vec<View>,
    /// The download link's text; none leaves the link out.
    #[serde(default)]
    pub download: Option<String>,
}

/// A button under a viewer: pressing it sets attributes on the element, then calls one of its
/// methods.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub label: String,
    /// The button's tooltip.
    #[serde(default)]
    pub title: Option<String>,
    /// The attributes a press sets, in order.
    pub set: Vec<(String, String)>,
    /// The element's method a press calls after setting them.
    #[serde(default)]
    pub call: Option<String>,
}

impl Viewer {
    /// The element for the file at `src`, captioned `label`, with `poster` shown before it loads:
    /// its markup, every value escaped, or `None` when [`Viewer::element`] names no custom
    /// element. An attribute whose name is not an attribute's is left out.
    pub fn element_html(&self, src: &str, label: &str, poster: Option<&str>) -> Option<String> {
        if !is_element(&self.element) {
            return None;
        }
        let given = [("src", src), ("alt", label)];
        let poster = poster.map(|poster| ("poster", poster));
        let attributes = given
            .into_iter()
            .chain(
                self.attributes
                    .iter()
                    .map(|(n, v)| (n.as_str(), v.as_str())),
            )
            .chain(poster)
            .filter(|(name, _)| is_name(name))
            .fold(String::new(), |mut out, (name, value)| {
                let _ = write!(out, " {name}=\"{}\"", escape(value));
                out
            });
        Some(format!("<{0}{attributes}></{0}>", self.element))
    }
}

impl View {
    /// [`View::set`] as one attribute's value: a line `name=value` for each pair, leaving out a
    /// name that is not an attribute's and a value holding a line break.
    pub fn encoded(&self) -> String {
        self.set
            .iter()
            .filter(|(name, value)| is_name(name) && !value.contains(['\n', '\r']))
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The pairs an [`View::encoded`] value holds, in order; a line without `=` or naming no
    /// attribute is passed over.
    pub fn decode(value: &str) -> Vec<(&str, &str)> {
        value
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(name, _)| is_name(name))
            .collect()
    }
}

/// Whether `name` is an attribute's name as viewers write them: a lower-case letter, then lower-case
/// letters, digits and hyphens.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `name` is a custom element's: an attribute's name holding a hyphen.
fn is_element(name: &str) -> bool {
    is_name(name) && name.contains('-')
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer() -> Viewer {
        Viewer {
            element: "orbit-view".into(),
            script: "/static/orbit.js".into(),
            attributes: vec![
                ("controls".into(), String::new()),
                ("angle".into(), "-45deg".into()),
            ],
            views: Vec::new(),
            download: None,
        }
    }

    #[test]
    fn the_element_holds_src_alt_its_attributes_then_the_poster() {
        let html = viewer().element_html("/a.item", "A", Some("/a.svg"));
        assert_eq!(
            html.as_deref(),
            Some(
                "<orbit-view src=\"/a.item\" alt=\"A\" controls=\"\" angle=\"-45deg\" \
                 poster=\"/a.svg\"></orbit-view>"
            ),
            "{html:?}"
        );
        let bare = viewer().element_html("/a.item", "A", None);
        assert!(
            bare.as_deref().is_some_and(|html| !html.contains("poster")),
            "{bare:?}"
        );
    }

    #[test]
    fn every_value_is_escaped_and_a_name_that_is_no_attribute_is_left_out() {
        let mut odd = viewer();
        odd.attributes = vec![
            ("onload x".into(), "1".into()),
            ("Upper".into(), "1".into()),
            ("data-x".into(), "\"><script>'&".into()),
        ];
        let html = odd.element_html("/a\".item", "<b>", None);
        assert_eq!(
            html.as_deref(),
            Some(
                "<orbit-view src=\"/a&quot;.item\" alt=\"&lt;b&gt;\" \
                 data-x=\"&quot;&gt;&lt;script&gt;&#x27;&amp;\"></orbit-view>"
            ),
            "{html:?}"
        );
    }

    #[test]
    fn an_element_that_is_no_custom_elements_name_draws_nothing() {
        for element in [
            "",
            "script",
            "Orbit-view",
            "-view",
            "orbit view",
            "orbit-view>",
        ] {
            let mut named = viewer();
            named.element = element.into();
            let html = named.element_html("/a.item", "A", None);
            assert_eq!(html, None, "{element:?} drew {html:?}");
        }
    }

    #[test]
    fn a_views_attributes_survive_their_encoding_in_order() {
        let view = View {
            label: "Above".into(),
            title: None,
            set: vec![
                ("angle".into(), "0deg 25deg auto".into()),
                ("zoom".into(), "a=b".into()),
                ("bad name".into(), "x".into()),
                ("wrapped".into(), "one\ntwo".into()),
            ],
            call: None,
        };
        let encoded = view.encoded();
        assert_eq!(encoded, "angle=0deg 25deg auto\nzoom=a=b", "{encoded:?}");
        let decoded = View::decode(&encoded);
        assert_eq!(
            decoded,
            [("angle", "0deg 25deg auto"), ("zoom", "a=b")],
            "{decoded:?}"
        );
        let skipped = View::decode("no pair\n=empty\nBad=x\nok=");
        assert_eq!(skipped, [("ok", "")], "{skipped:?}");
    }

    #[test]
    fn a_gallery_without_a_viewer_reads_as_one_drawing_nothing() {
        let gallery: Media = serde_json::from_str(r#"{"kind":"gallery","items":[]}"#).unwrap();
        assert_eq!(
            gallery,
            Media::Gallery {
                items: Vec::new(),
                viewer: Viewer::default()
            },
            "{gallery:?}"
        );
        let shown = Media::Viewer {
            src: "/a.item".into(),
            label: "A".into(),
            poster: None,
            viewer: viewer(),
        };
        let json = serde_json::to_string(&shown).unwrap();
        assert_eq!(
            serde_json::from_str::<Media>(&json).unwrap(),
            shown,
            "{json}"
        );
        assert!(json.starts_with(r#"{"kind":"viewer","#), "{json}");
    }
}
