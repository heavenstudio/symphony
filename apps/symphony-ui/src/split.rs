//! The program (`page-program`): the task pages with `/agents`, Reports, the chat, the menus and the
//! task timeline as one program, which `dx bundle --wasm-split` cuts into a main module and a chunk
//! for each of those kinds (bundle.sh, then `symphony-build split-repair`). The main module holds
//! the runtime, the frame and what each kind reaches before its own part; a kind's chunk holds that
//! part. The kinds share the one runtime, so a move between them loads a chunk, never another copy
//! of the framework.
//!
//! Each split point is what `wasm_split::lazy_loader!` writes, spelled out: its expansion is edition
//! 2021 (`extern` blocks and `#[no_mangle]` without `unsafe`), which this crate refuses. The names
//! follow wasm-split-cli 0.7.10's reading of them: an import
//! `__wasm_split_00___MODULE___00_import_HASH_NAME`, which the splitter turns into a call through a
//! table slot the chunk fills when it is instantiated, and its `_export_` twin, the chunk's entry.
//! The splitter's own loader (`__wasm_split_load_…`) is never imported: it logs a failed fetch and
//! never answers, which would leave a page waiting. The program's script carries a loader of its own
//! instead (`reviewChunk`, which package.sh writes with the kind-to-chunk map), and it rejects on any
//! failure: a kind's `run` awaits [`Chunk::load`] before its root shows anything, so a chunk that
//! fails rejects `run`, and the shell ends in its `gone` state (assets/shell.js).

use std::cell::Cell;

use dioxus::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// A kind of the program, named by the chunk that holds its own part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chunk {
    /// The task pages and `/agents`.
    Tasks,
    Reports,
    /// The chat, the activity list and the Appearance panel.
    Panel,
    /// A select's popup and the picture viewer, for every module.
    Overlay,
    Timeline,
}

thread_local! {
    /// The chunks instantiated, a bit each.
    static LOADED: Cell<u8> = const { Cell::new(0) };
}

impl Chunk {
    pub(crate) const ALL: [Chunk; 5] = [
        Chunk::Tasks,
        Chunk::Reports,
        Chunk::Panel,
        Chunk::Overlay,
        Chunk::Timeline,
    ];

    /// Its page kind, as the shell's manifest and `reviewChunk` name it.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Chunk::Tasks => "tasks",
            Chunk::Reports => "reports",
            Chunk::Panel => "panel",
            Chunk::Overlay => "overlay",
            Chunk::Timeline => "timeline",
        }
    }

    /// The element its kind's Dioxus root is mounted on, which the shell makes (`mountFor`): the
    /// persistent modules' own, a page kind's in `#main`.
    pub(crate) fn root(self) -> String {
        match self {
            Chunk::Panel => "quick-root".into(),
            Chunk::Overlay => "overlay-root".into(),
            Chunk::Timeline => "timeline-root".into(),
            Chunk::Tasks | Chunk::Reports => format!("review-root-{}", self.kind()),
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Whether its chunk is in, so the slot its split point calls through holds its entry.
    pub(crate) fn loaded(self) -> bool {
        LOADED.with(|loaded| loaded.get() & self.bit() != 0)
    }

    /// Fetches and instantiates its chunk once, through the program script's `reviewChunk`; an
    /// error for a failed fetch, an answer that is not ok or a failed instantiation, after which a
    /// later call tries again.
    pub(crate) async fn load(self) -> Result<(), JsValue> {
        if self.loaded() {
            return Ok(());
        }
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let load: js_sys::Function =
            js_sys::Reflect::get(&window, &"reviewChunk".into())?.dyn_into()?;
        let loading: js_sys::Promise = load
            .call1(&JsValue::UNDEFINED, &self.kind().into())?
            .dyn_into()?;
        JsFuture::from(loading).await?;
        LOADED.with(|loaded| loaded.set(loaded.get() | self.bit()));
        Ok(())
    }
}

/// Whether `chunk` is in. A kind's `run` loads its chunk before its root draws, so it is from the
/// first render; a part drawn in another root (a page the shell is about to replace) loads it
/// here, and draws once it is in.
fn use_chunk(chunk: Chunk) -> bool {
    let mut ready = use_signal(|| chunk.loaded());
    use_hook(move || {
        if !chunk.loaded() {
            spawn(async move {
                if chunk.load().await.is_ok() {
                    ready.set(true);
                }
            });
        }
    });
    ready()
}

// The split points. Each import is called only once its chunk is in (`use_chunk`): until then its
// slot holds the splitter's placeholder, which traps.
#[link(wasm_import_module = "./__wasm_split.js")]
unsafe extern "C" {
    #[allow(improper_ctypes)]
    fn __wasm_split_00___tasks___00_import_5b5e7a5c000000000000000000000001_tasks(
        scope: String,
        route: Memo<String>,
    ) -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___reports___00_import_5b5e7a5c000000000000000000000002_reports() -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___panel___00_import_5b5e7a5c000000000000000000000003_panel() -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___overlay___00_import_5b5e7a5c000000000000000000000004_overlay() -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___timeline___00_import_5b5e7a5c000000000000000000000005_timeline() -> Element;
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn __wasm_split_00___tasks___00_export_5b5e7a5c000000000000000000000001_tasks(
    scope: String,
    route: Memo<String>,
) -> Element {
    rsx! { crate::live::BoardPage { route, scope } }
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn __wasm_split_00___reports___00_export_5b5e7a5c000000000000000000000002_reports()
-> Element {
    rsx! { crate::reports::ReportsRoute {} }
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn __wasm_split_00___panel___00_export_5b5e7a5c000000000000000000000003_panel()
-> Element {
    rsx! { crate::frame::PanelApp {} }
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn __wasm_split_00___overlay___00_export_5b5e7a5c000000000000000000000004_overlay()
-> Element {
    rsx! { crate::components::OverlayApp {} }
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn __wasm_split_00___timeline___00_export_5b5e7a5c000000000000000000000005_timeline()
-> Element {
    rsx! { crate::task_timeline::TimelineApp {} }
}

/// A task page or `/agents`, from the task pages' chunk.
#[component]
pub(crate) fn TasksPart(scope: String, route: Memo<String>) -> Element {
    if !use_chunk(Chunk::Tasks) {
        return rsx! {};
    }
    // SAFETY: the chunk is in, and its entry takes these arguments and returns an `Element`.
    unsafe {
        __wasm_split_00___tasks___00_import_5b5e7a5c000000000000000000000001_tasks(scope, route)
    }
}

/// `/reports`, from its chunk.
#[component]
pub(crate) fn ReportsPart() -> Element {
    if !use_chunk(Chunk::Reports) {
        return rsx! {};
    }
    // SAFETY: as in `TasksPart`.
    unsafe { __wasm_split_00___reports___00_import_5b5e7a5c000000000000000000000002_reports() }
}

/// The chat's panel, from its chunk.
#[component]
pub(crate) fn PanelPart() -> Element {
    if !use_chunk(Chunk::Panel) {
        return rsx! {};
    }
    // SAFETY: as in `TasksPart`.
    unsafe { __wasm_split_00___panel___00_import_5b5e7a5c000000000000000000000003_panel() }
}

/// The overlay host, from its chunk.
#[component]
pub(crate) fn OverlayPart() -> Element {
    if !use_chunk(Chunk::Overlay) {
        return rsx! {};
    }
    // SAFETY: as in `TasksPart`.
    unsafe { __wasm_split_00___overlay___00_import_5b5e7a5c000000000000000000000004_overlay() }
}

/// A task's timeline, from its chunk.
#[component]
pub(crate) fn TimelinePart() -> Element {
    if !use_chunk(Chunk::Timeline) {
        return rsx! {};
    }
    // SAFETY: as in `TasksPart`.
    unsafe { __wasm_split_00___timeline___00_import_5b5e7a5c000000000000000000000005_timeline() }
}
