//! The Symphony's client: the board view of `/`, drawn from the server's JSON API
//! (crates/symphony-api) and kept current by its feed of deltas.
//!
//! - [`state`]: a feed event applied to the board, pure; what moved, what to tell the owner.
//! - [`words`]: the board's words and times.
//! - [`view`]: the components, pure functions of a [`symphony_api::Board`].
//! - [`platform`]: the one trait every platform hook goes behind (notifications, a link opened
//!   outside, storage), so the browser and the desktop window run the same app.
//! - `live` (wasm only): the fetch and the feed.

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
pub mod components;
pub mod doc_components;
pub mod docs;
pub(crate) mod entry_events;
pub mod media;
pub mod platform;
pub mod project;
pub mod state;
pub mod tasks;
pub mod theme;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-review"))]
pub mod view;
pub mod viewport;
pub mod words;

#[cfg(target_arch = "wasm32")]
pub mod board_live;
#[cfg(all(target_arch = "wasm32", feature = "page-docs"))]
pub mod docs_live;
#[cfg(target_arch = "wasm32")]
pub mod navigation;
#[cfg(all(
    target_arch = "wasm32",
    any(feature = "page-docs", feature = "page-queue")
))]
pub mod postpone_live;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-queue"))]
pub mod queue;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-queue"))]
pub mod queue_filter;
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
