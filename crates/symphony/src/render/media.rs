//! Pictures that markup cannot draw: an `.html` mockup, held as a [`Media`] item, and a file a
//! renderer of the page's plugins draws, as its viewer item or its blocks.
//!
//! `![Flight HUD A](/ui/mockups/hud-a.html)` is a mockup: a sandboxed frame at its native size,
//! scaled to the card's width by the client. `![Lumen](lumen.item)` and `[Lumen](lumen.item)` are
//! drawn by the renderer of the page's plugins that lists the extension `item`, captioned `Lumen`.
//! A media item leaves `<span class="media" data-media="N" data-kind="…">` in the fragment.

use std::sync::LazyLock;

use regex::{Captures, Regex};
use symphony_api::Block;
use symphony_plugin::{Drawing, File};

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
static FILE_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*?\bsrc="(?P<src>[^"]*)"[^>]*?>"#).expect("the picture pattern")
});
static FILE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*?\bhref="(?P<src>[^"]*)"[^>]*>(?P<text>.*?)</a>"#)
        .expect("the link pattern")
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

/// `blocks` of the component or renderer `name` as the fragment holds them: written as HTML, in a
/// typed render inside the `<sf-component>` the parser reads back as [`Block::Component`].
pub(super) fn blocks_html(ctx: &Ctx<'_>, name: &str, blocks: Vec<Block>) -> String {
    let mut media = |item| ctx.add_media(item);
    if ctx.typed {
        crate::blocks::write::html(
            &[Block::Component {
                kind: name.into(),
                blocks,
            }],
            &mut media,
        )
    } else if let [Block::Paragraph { spans }] = blocks.as_slice() {
        crate::blocks::write::spans_html(spans, &mut media)
    } else {
        crate::blocks::write::html(&blocks, &mut media)
    }
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

/// `src` with its path written as a URL path, its query and fragment kept as they are.
fn file_url(src: &str) -> String {
    let (path, rest) = src.split_at(src.find(['?', '#']).unwrap_or(src.len()));
    format!("{}{rest}", quote(&unquote(path), "/:"))
}

/// The file `src` (resolved against the page) captioned `label`, as the renderer of the page's
/// plugins that lists its extension draws it; `None` when none does or `src` is another site's,
/// which stays a link. A renderer's problem is shown in the file's place.
fn drawn(src: &str, label: &str, ctx: &Ctx<'_>) -> Option<String> {
    if is_external(src) {
        return None;
    }
    let renderer = crate::renderers::renderer_of(ctx.plugins, src)?;
    let path = file_url(src);
    let label = if label.is_empty() {
        file_name(&unquote(src))
    } else {
        label.to_string()
    };
    let file = File {
        src: &path,
        label: &label,
    };
    Some(match renderer.draw(&ctx.page, &file) {
        Ok(Drawing::Viewer(viewer)) => placeholder(
            "viewer",
            ctx.add_media(Media::Viewer {
                src: path,
                label,
                poster: None,
                viewer,
            }),
        ),
        Ok(Drawing::Blocks(blocks)) => blocks_html(ctx, renderer.name(), blocks),
        Err(problem) => blocks_html(ctx, renderer.name(), vec![crate::plugins::error(&problem)]),
    })
}

/// `body` with each image and link to a file a plugin's renderer draws replaced by its drawing.
fn embed_files(body: &str, ctx: &Ctx<'_>) -> String {
    if ctx.plugins.list().iter().all(|p| p.renderers().is_empty()) {
        return body.to_string();
    }
    // Links first: a viewer may carry a download link of its own, which a later link pass would
    // draw again.
    let linked = FILE_LINK.replace_all(body, |found: &Captures<'_>| {
        let text = unescape(&TAGS.replace_all(&found["text"], ""));
        drawn(&target(&found["src"], ctx.doc_path), &text, ctx)
            .unwrap_or_else(|| found[0].to_string())
    });
    FILE_IMG
        .replace_all(&linked, |found: &Captures<'_>| {
            let alt = ALT
                .captures(&found[0])
                .map(|a| unescape(&a[1]))
                .unwrap_or_default();
            drawn(&target(&found["src"], ctx.doc_path), &alt, ctx)
                .unwrap_or_else(|| found[0].to_string())
        })
        .into_owned()
}

/// The rendered markdown with its mockups held as media and its plugin-drawn files drawn.
pub(super) fn embed(body: &str, ctx: &Ctx<'_>) -> String {
    embed_mockups(&embed_files(body, ctx), ctx)
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
