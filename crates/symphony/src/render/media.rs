//! Pictures that markup cannot draw: an `.html` mockup and a `.glb` model, held as [`Media`] items.
//!
//! `![Flight HUD A](/ui/mockups/hud-a.html)` is a mockup: a sandboxed frame at its native size,
//! scaled to the card's width by the client. `![Lumen](lumen.glb)` and `[Lumen](lumen.glb)` are a model the
//! viewer orbits. Both leave `<span class="media" data-media="N" data-kind="…">` in the fragment.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::markup::{is_external, quote, target, unescape, unquote};

use super::{Ctx, Media};

/// A mockup's native size, which the frame is drawn at before it is scaled.
pub const MOCKUP_NATIVE: (u32, u32) = (1280, 800);
/// The frame's sandbox: animation only (no same-origin, no top navigation, no forms, no popups).
pub const MOCKUP_SANDBOX: &str = "allow-scripts";

static MOCKUP_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*?\bsrc="(?P<src>[^"]*\.html?(?:[?#][^"]*)?)"[^>]*?>"#)
        .expect("the mockup pattern")
});
static MODEL_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*?\bsrc="(?P<src>[^"]*\.glb)"[^>]*?>"#).expect("the model pattern")
});
static MODEL_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*?\bhref="(?P<src>[^"]*\.glb)"[^>]*>(?P<text>.*?)</a>"#)
        .expect("the model link pattern")
});
static ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\balt="([^"]*)""#).expect("the alt pattern"));
static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("the tag pattern"));

fn file_name(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

fn placeholder(kind: &str, n: usize) -> String {
    format!("<span class=\"media\" data-media=\"{n}\" data-kind=\"{kind}\"></span>")
}

/// `body` (rendered markdown) with each local image of an `.html` file replaced by a mockup item.
pub(super) fn embed_mockups(body: &str, ctx: &Ctx<'_>) -> String {
    MOCKUP_IMG
        .replace_all(body, |found: &Captures<'_>| {
            let src = target(&found["src"], ctx.doc_path);
            if is_external(&src) {
                return found[0].to_string();
            }
            let alt = ALT
                .captures(&found[0])
                .map(|a| unescape(&a[1]))
                .unwrap_or_default();
            let label = if alt.is_empty() { file_name(&src) } else { alt };
            placeholder("mockup", ctx.add_media(Media::Mockup { src, label }))
        })
        .into_owned()
}

fn model(src: &str, label: &str, ctx: &Ctx<'_>) -> String {
    let path = quote(&unquote(src), "/:");
    let label = if label.is_empty() {
        file_name(&unquote(src))
    } else {
        label.to_string()
    };
    placeholder(
        "model",
        ctx.add_media(Media::Model {
            src: path,
            label,
            poster: None,
        }),
    )
}

/// `body` with each image and link to a `.glb` replaced by a model item.
fn embed_models(body: &str, ctx: &Ctx<'_>) -> String {
    // Links first: an image's viewer carries a download link of its own, which a later link pass would embed again.
    let linked = MODEL_LINK.replace_all(body, |found: &Captures<'_>| {
        let text = unescape(&TAGS.replace_all(&found["text"], ""));
        model(&target(&found["src"], ctx.doc_path), &text, ctx)
    });
    MODEL_IMG
        .replace_all(&linked, |found: &Captures<'_>| {
            let alt = ALT
                .captures(&found[0])
                .map(|a| unescape(&a[1]))
                .unwrap_or_default();
            model(&target(&found["src"], ctx.doc_path), &alt, ctx)
        })
        .into_owned()
}

/// The rendered markdown with its models and mockups held as media.
pub(super) fn embed(body: &str, ctx: &Ctx<'_>) -> String {
    embed_mockups(&embed_models(body, ctx), ctx)
}

/// Whether the picture at `src` is an `.html` mockup, which an `<img>` cannot draw.
pub fn is_mockup(src: &str) -> bool {
    let path = src
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("")
        .to_lowercase();
    path.ends_with(".html") || path.ends_with(".htm")
}
