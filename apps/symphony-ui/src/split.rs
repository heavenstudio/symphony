//! The program (`page-program`): the chat and the menus every page mounts, as one program, which
//! `dx bundle --wasm-split` cuts into a main module and a chunk for each of those kinds (bundle.sh,
//! then `symphony-build split-repair`). No page kind is the program's: every page is the web
//! client's (apps/symphony-web), and the program's kinds are the shell's persistent ones
//! (assets/shell.js `PERSISTENT`). The main module holds
//! the runtime and what each kind reaches before its own part; a kind's chunk holds that part. The
//! kinds share the one runtime, so the second to load is a chunk, never another copy of the
//! framework. The media layer (`media_live::MediaRuntime`) is a chunk of its own too, though no
//! kind: the chat loads it once it shows media ([`MediaPart`]), so a chat without any loads none of
//! it.
//!
//! Each split point is what `wasm_split::lazy_loader!` writes, spelled out: its expansion is edition
//! 2021 (`extern` blocks and `#[no_mangle]` without `unsafe`), which this crate refuses. The names
//! follow wasm-split-cli 0.7.10's reading of them: an import
//! `__wasm_split_00___MODULE___00_import_HASH_NAME`, which the splitter turns into a call through a
//! table slot the chunk fills when it is instantiated, and its `_export_` twin, the chunk's entry.
//! The splitter's own loader (`__wasm_split_load_…`) is never imported: it logs a failed fetch and
//! never answers, which would leave a page waiting. The program's script carries a loader of its own
//! instead (`reviewChunk`, which package.sh writes with the kind-to-chunk map), and it rejects on any
//! failure. The shell awaits it before it mounts the kind (assets/shell.js `initialize`), and a
//! kind's `run` awaits [`Chunk::load`] before its root shows anything, so a chunk that fails leaves
//! the page as it was with the shell's notice and retry, never an empty panel or one left waiting.

use std::cell::Cell;

use dioxus::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// A kind of the program, named by the chunk that holds its own part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chunk {
    /// The chat, the activity list and the Appearance panel.
    Panel,
    /// A select's popup and the picture viewer, for every module.
    Overlay,
}

thread_local! {
    /// The chunks instantiated, a bit each.
    static LOADED: Cell<u8> = const { Cell::new(0) };
}

/// The media layer's chunk, which no kind is: its name in the program script's map of chunks (the
/// shell's manifest never names it), and its bit, past the kinds'.
const MEDIA: (&str, u8) = ("media", 1 << Chunk::ALL.len());

/// Whether the chunk of `bit` is in, so the slot its split point calls through holds its entry.
fn is_in(bit: u8) -> bool {
    LOADED.with(|loaded| loaded.get() & bit != 0)
}

/// Fetches and instantiates the chunk `name` once, through the program script's `reviewChunk`; an
/// error for a failed fetch, an answer that is not ok or a failed instantiation, after which a
/// later call tries again.
async fn fetch(name: &str, bit: u8) -> Result<(), JsValue> {
    if is_in(bit) {
        return Ok(());
    }
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let load: js_sys::Function =
        js_sys::Reflect::get(&window, &"reviewChunk".into())?.dyn_into()?;
    let loading: js_sys::Promise = load.call1(&JsValue::UNDEFINED, &name.into())?.dyn_into()?;
    JsFuture::from(loading).await?;
    LOADED.with(|loaded| loaded.set(loaded.get() | bit));
    Ok(())
}

impl Chunk {
    pub(crate) const ALL: [Chunk; 2] = [Chunk::Panel, Chunk::Overlay];

    /// Its page kind, as the shell's manifest and `reviewChunk` name it.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Chunk::Panel => "panel",
            Chunk::Overlay => "overlay",
        }
    }

    /// The element its kind's Dioxus root is mounted on, which the shell makes (`mountFor`): the
    /// persistent modules' own.
    pub(crate) fn root(self) -> &'static str {
        match self {
            Chunk::Panel => "quick-root",
            Chunk::Overlay => "overlay-root",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Fetches and instantiates its chunk once ([`fetch`]).
    pub(crate) async fn load(self) -> Result<(), JsValue> {
        fetch(self.kind(), self.bit()).await
    }
}

/// Whether `chunk` is in. A kind's `run` loads its chunk before its root draws, so it is from the
/// first render; a part drawn before it is loads it here, and draws once it is in. A kind's load
/// that fails is the shell's to say.
fn use_chunk(chunk: Chunk) -> bool {
    use_loaded(chunk.kind(), chunk.bit(), None)
}

/// Whether the chunk `name` (of `bit`) is in, fetched by the first render that finds it out. A part
/// that is no kind, the media layer, passes its activation, `shown`: a load that fails sets it
/// back, and the shell hears of each load ([`media_said`]). One future serves every part, kind or
/// not: a spawned future's code stays with its vtable in the main module, so a second would grow
/// that.
fn use_loaded(name: &'static str, bit: u8, shown: Option<Signal<bool>>) -> bool {
    let mut ready = use_signal(|| is_in(bit));
    use_hook(move || {
        if !is_in(bit) {
            spawn(async move {
                let loaded = fetch(name, bit).await;
                let ok = loaded.is_ok();
                if let Some(mut shown) = shown {
                    if !ok {
                        shown.set(false);
                    }
                    media_said(loaded);
                }
                if ok {
                    ready.set(true);
                }
            });
        }
    });
    ready()
}

/// What the program's chunk loader hears of a load of the media layer's chunk (assets/chunks.js):
/// one that failed (`reviewMediaFailed`, which loads the page again when its build is gone and
/// otherwise says so with the shell's retry), or one that worked (`reviewMediaLoaded`, which takes
/// down the notice an earlier failure put up). Called as `fetch` calls `reviewChunk`, it needs no
/// import of its own.
fn media_said(loaded: Result<(), JsValue>) {
    let (name, error) = match loaded {
        Ok(()) => ("reviewMediaLoaded", JsValue::UNDEFINED),
        Err(error) => ("reviewMediaFailed", error),
    };
    if let Some(window) = web_sys::window()
        && let Ok(said) = js_sys::Reflect::get(&window, &name.into())
        && let Ok(said) = said.dyn_into::<js_sys::Function>()
    {
        let _ = said.call1(&JsValue::UNDEFINED, &error);
    }
}

// The split points. Each import is called only once its chunk is in (`use_chunk`, `MediaPart`):
// until then its slot holds the splitter's placeholder, which traps.
#[link(wasm_import_module = "./__wasm_split.js")]
unsafe extern "C" {
    #[allow(improper_ctypes)]
    fn __wasm_split_00___panel___00_import_5b5e7a5c000000000000000000000003_panel() -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___overlay___00_import_5b5e7a5c000000000000000000000004_overlay() -> Element;
    #[allow(improper_ctypes)]
    fn __wasm_split_00___media___00_import_5b5e7a5c000000000000000000000006_media() -> Element;
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
pub extern "C" fn __wasm_split_00___media___00_export_5b5e7a5c000000000000000000000006_media()
-> Element {
    rsx! { crate::media_live::MediaRuntime {} }
}

/// The chat's panel, from its chunk.
#[component]
pub(crate) fn PanelPart() -> Element {
    if !use_chunk(Chunk::Panel) {
        return rsx! {};
    }
    // SAFETY: the chunk is in, and its entry takes no argument and returns an `Element`.
    unsafe { __wasm_split_00___panel___00_import_5b5e7a5c000000000000000000000003_panel() }
}

/// The overlay host, from its chunk.
#[component]
pub(crate) fn OverlayPart() -> Element {
    if !use_chunk(Chunk::Overlay) {
        return rsx! {};
    }
    // SAFETY: as in `PanelPart`.
    unsafe { __wasm_split_00___overlay___00_import_5b5e7a5c000000000000000000000004_overlay() }
}

/// The media layer (`media_live::MediaRuntime`), from its chunk, drawn once the chat shows media
/// (`needed`, `media_live::use_activation`): once the chunk is in, nothing before, so the chat's
/// media wait for it as they wait for the activation. A chunk that does not load leaves the page as
/// it is and sets `needed` back, so the next media the chat shows asks for it again; the page loads
/// again when its build is gone, as for a kind, and otherwise says so with a retry that shows the
/// media again (`reviewMediaFailed`, assets/chunks.js, over the shell's `gone` and `alarm`), its
/// notice taken down once the chunk is in or the page moves on.
#[component]
pub(crate) fn MediaPart(needed: Signal<bool>) -> Element {
    let (name, bit) = MEDIA;
    if !use_loaded(name, bit, Some(needed)) {
        return rsx! {};
    }
    // SAFETY: as in `PanelPart`.
    unsafe { __wasm_split_00___media___00_import_5b5e7a5c000000000000000000000006_media() }
}
