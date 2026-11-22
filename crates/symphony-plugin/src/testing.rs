//! A plugin's seams asked as a host asks them, for the plugin's own tests, with no Symphony library
//! in its build. [`Host`] writes each call as the protocol's request (docs/engineering/PLUGIN_PROTOCOL.md), has
//! the plugin answer it as its process answers a line ([`crate::serve`]: the seam's object found by
//! its name, a request that does not read or a panic refused by why, one render's requests answered
//! on one page whose components share what they load), and reads the answer back as the call's
//! type. A test so sees what a host shows: a field a request or an answer loses on the wire fails
//! the test as it would fail the page.
//!
//! ```
//! use symphony_plugin::protocol::{CardData, Recognises};
//! use symphony_plugin::testing::Host;
//! use symphony_plugin::vocabulary::{Block, Span};
//! use symphony_plugin::{Card, CardKind, Component, Page, Plugin};
//!
//! struct Hello;
//! impl Plugin for Hello {
//!     fn name(&self) -> &str { "hello" }
//!     fn components(&self) -> Vec<&dyn Component> { vec![self] }
//!     fn card_kinds(&self) -> Vec<&dyn CardKind> { vec![self] }
//! }
//! impl Component for Hello {
//!     fn name(&self) -> &str { "hello" }
//!     fn render(&self, page: &Page<'_>, argument: &str, _: Option<&str>) -> Result<Vec<Block>, String> {
//!         let text = format!("hello {argument} in {}", page.artifact("x"));
//!         Ok(vec![Block::Paragraph { spans: vec![Span::Text { text }] }])
//!     }
//! }
//! impl CardKind for Hello {
//!     fn name(&self) -> &str { "hello" }
//!     fn recognises(&self, card: &Card<'_>) -> bool { card.text.starts_with("Hi") }
//! }
//!
//! let host = Host::new(&Hello);
//! let page = Page::new("/p".as_ref(), "docs/A.md").in_artifacts("out");
//! assert_eq!(
//!     host.render(&page, "hello", "you", None),
//!     [Block::Paragraph { spans: vec![Span::Text { text: "hello you in out/x".into() }] }]
//! );
//! let card = Card { slug: "a", doc: "docs/A.md", topic: "General", text: "Hi", thread: &[] };
//! assert_eq!(host.ask(Recognises::new("hello".into(), CardData::of(&card))), Ok(true));
//! ```

use crate::protocol::{Answered, Ask, Declaration, Declare, PROTOCOL, PageData, Render, Request};
use crate::serve::{Pages, answered};
use crate::vocabulary::Block;
use crate::{Page, Plugin, error};

/// A host of one plugin, in this process: each call goes to the plugin as its JSON request and
/// comes back as its JSON answer.
pub struct Host<'p> {
    plugin: &'p dyn Plugin,
    /// The pages of the renders asked of it, as a plugin's process keeps them.
    pages: Pages,
}

impl<'p> Host<'p> {
    pub fn new(plugin: &'p dyn Plugin) -> Host<'p> {
        Host {
            plugin,
            pages: Pages::default(),
        }
    }

    /// The call `request`'s answer, as the plugin answers it on the wire; or why the call was
    /// refused (no object of that name, a request it could not read, a panic), or why the request
    /// would not write or the answer not read back as the call's type.
    pub fn ask<A: Ask>(&self, request: A) -> Result<A::Answer, String> {
        let request: Request = request.into();
        let written = serde_json::to_string(&request)
            .map_err(|e| format!("the request {} does not write: {e}", A::TAG))?;
        let answer = answered(self.plugin, &written, &self.pages);
        serde_json::from_str::<Answered<A::Answer>>(answer.get())
            .map_err(|e| {
                format!(
                    "the answer to {} does not read: {e}: {}",
                    A::TAG,
                    answer.get()
                )
            })?
            .shown()
    }

    /// What the plugin declares to a host of this build's protocol, the first request a host makes.
    pub fn declaration(&self) -> Result<Declaration, String> {
        self.ask(Declare::new(PROTOCOL))
    }

    /// The component `component` for `argument` and `option` on `page`, as a host shows it: its
    /// blocks, or its problem in their place ([`error`]), a refused call's included. Two renders on
    /// one `page` are one render's requests, so its components share what they load
    /// ([`Page::once`]); another `page` loads again.
    pub fn render(
        &self,
        page: &Page<'_>,
        component: &str,
        argument: &str,
        option: Option<&str>,
    ) -> Vec<Block> {
        self.ask(Render::new(
            component.into(),
            PageData::of(page),
            argument.into(),
            option.map(str::to_string),
        ))
        .and_then(|answer| answer)
        .unwrap_or_else(|problem| vec![error(&problem)])
    }
}
