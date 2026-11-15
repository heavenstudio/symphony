//! The Symphony's Dioxus client: the pages the web client (apps/symphony-web) does not draw yet,
//! drawn from the server's JSON API (crates/symphony-api) and kept current by its feed of deltas.
//!
//! - [`state`]: following the feed, pure: the waits after a lost feed, another build loaded.
//! - [`words`]: the board's words and times.
//! - [`cache`]: what a page drew, kept on the device by the shell's store (`assets/cache.js`).
//! - [`platform`]: the one trait every platform hook goes behind (a link opened outside,
//!   storage), so the browser and the desktop window run the same app.
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
    feature = "page-panel"
))]
pub mod attach;
#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "page-tasks",
    feature = "page-panel"
))]
pub mod block_view;
pub mod cache;
pub mod components;
pub mod doc_components;
pub(crate) mod entry_events;
/// The machines' build and test work, as the task page draws it.
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
pub mod viewer;
pub mod viewport;
pub mod words;

#[cfg(target_arch = "wasm32")]
pub mod navigation;
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

#[cfg(target_arch = "wasm32")]
pub mod frame;
