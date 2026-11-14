//! The Symphony's client: the board view of `/`, drawn from the server's JSON API
//! (crates/symphony-api) and kept current by its feed of deltas.
//!
//! - [`state`]: a feed event applied to the board, pure; what moved, what to tell the owner.
//! - [`words`]: the board's words and times.
//! - [`view`]: the components, pure functions of a [`symphony_api::Board`].
//! - [`cache`]: what a page drew, kept on the device by the shell's store (`assets/cache.js`).
//! - [`platform`]: the one trait every platform hook goes behind (notifications, a link opened
//!   outside, storage), so the browser and the desktop window run the same app.
//! - `live` (wasm only): the fetch and the feed.

#![cfg_attr(not(feature = "page-program"), forbid(unsafe_code))]
// The program's split points (`split`) are the one place that calls across a chunk's boundary.
#![cfg_attr(feature = "page-program", deny(unsafe_code))]

/// The program's split points: its kinds' own parts, each in a chunk of its own.
#[cfg(all(target_arch = "wasm32", feature = "page-program"))]
#[allow(unsafe_code)]
mod split;

#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "page-tasks",
    feature = "page-docs",
    feature = "page-panel"
))]
pub mod attach;
#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "page-tasks",
    feature = "page-docs",
    feature = "page-panel"
))]
pub mod block_view;
pub mod cache;
pub mod components;
pub mod doc_components;
pub mod docs;
pub(crate) mod entry_events;
/// An HTML document's page: the file in a sandboxed frame, in the documents' kind.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-docs"))]
pub mod html_page;
/// The machines' build and test work, as the agents page and the task page draw it.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
pub(crate) mod machine_work;
pub mod media;
pub mod platform;
pub mod project;
pub mod state;
#[cfg(any(target_arch = "wasm32", test))]
mod task_save;
/// A task's timeline, the page kind `timeline` of its own, loaded into the task page's box.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-timeline"))]
pub mod task_timeline;
pub mod tasks;
pub mod theme;
/// The task page's box for its timeline, in the task kind; the timeline module keeps its heights.
#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "page-tasks",
    feature = "page-timeline"
))]
pub mod timeline_slot;
pub mod trouble;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-review"))]
pub mod view;
pub mod viewer;
pub mod viewport;
pub mod words;

#[cfg(all(target_arch = "wasm32", feature = "page-docs"))]
pub mod docs_live;
#[cfg(target_arch = "wasm32")]
pub mod navigation;
#[cfg(all(target_arch = "wasm32", feature = "page-docs"))]
pub mod postpone_live;
/// The Reports page, drawn by the task pages' module as the Agents page is.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
pub mod reports;
#[cfg(target_arch = "wasm32")]
pub mod request;
#[cfg(target_arch = "wasm32")]
pub mod task_live;

#[cfg(target_arch = "wasm32")]
pub mod live;
#[cfg(target_arch = "wasm32")]
pub mod module;

#[cfg(target_arch = "wasm32")]
pub mod media_live;

#[cfg(all(target_arch = "wasm32", feature = "page-docs"))]
pub mod asks;

#[cfg(target_arch = "wasm32")]
pub mod frame;
