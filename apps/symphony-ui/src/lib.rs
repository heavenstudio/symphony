//! The Symphony's Dioxus client: the chat, its activity and Appearance panels, and the menus (a
//! select's popup and the picture viewer) that every page mounts over the web client's
//! (apps/symphony-web), which draws every page. They read the server's JSON API
//! (crates/symphony-api) themselves.
//!
//! - [`state`]: what the panel makes of what the page heard, pure: its toasts, the asks' news.
//! - [`platform`]: the one trait every platform hook goes behind (a link opened outside,
//!   storage), so the browser and the desktop window run the same app.
//! - `live` (wasm only): the program's waits and marks.

#![cfg_attr(not(feature = "page-program"), forbid(unsafe_code))]
// The program's split points (`split`) are the one place that calls across a chunk's boundary.
#![cfg_attr(feature = "page-program", deny(unsafe_code))]

/// The program's split points: its kinds' own parts, each in a chunk of its own.
#[cfg(all(target_arch = "wasm32", feature = "page-program"))]
#[allow(unsafe_code)]
mod split;

/// Files attached in the `+` sheet's composers, which the chat's kind alone (`page-panel`) draws.
#[cfg(any(not(target_arch = "wasm32"), feature = "page-panel"))]
pub mod attach;
#[cfg(any(not(target_arch = "wasm32"), feature = "page-panel"))]
pub mod block_view;
pub mod components;
pub mod doc_components;
/// The browser's event attributes and visibility latch: no control of the native build takes one.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod entry_events;
pub mod media;
pub mod platform;
pub mod project;
pub mod state;
#[cfg(any(target_arch = "wasm32", test))]
mod task_save;
pub mod theme;
pub mod trouble;
pub mod viewer;
pub mod viewport;

#[cfg(target_arch = "wasm32")]
pub mod navigation;
#[cfg(target_arch = "wasm32")]
pub mod request;

#[cfg(target_arch = "wasm32")]
pub mod live;
#[cfg(target_arch = "wasm32")]
pub mod module;

#[cfg(target_arch = "wasm32")]
pub mod media_live;

#[cfg(target_arch = "wasm32")]
pub mod frame;
