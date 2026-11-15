//! Development only: the program's kind, the chat, in a runtime of its own, using the same client
//! library as releases. DX updates RSX in place and incrementally rebuilds Rust logic; dev.js
//! gates the resulting reload. The pages and the menus (a select's popup, the picture viewer) are
//! the web client's, whose own dev server serves them (apps/symphony-web/AGENTS.md, "The dev
//! server"): here a select or a picture opens nothing.

#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
use symphony_ui::{frame, navigation};

fn main() {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::closure::Closure;
        let route = Closure::<dyn FnMut(String) -> bool>::new(|path: String| {
            navigation::Route::parse(&path).is_some()
        });
        if let Some(window) = web_sys::window() {
            let _ = js_sys::Reflect::set(&window, &"reviewDevRoute".into(), route.as_ref());
        }
        route.forget();
        // Install the reload gate before Dioxus opens its devtools WebSocket.
        let _ = js_sys::eval(include_str!("../assets/dev.js"));
        frame::keep_styles();
        dioxus::web::launch::launch_cfg(Development, dioxus::web::Config::default());
    }
}

#[cfg(target_arch = "wasm32")]
#[dioxus::prelude::component]
fn Development() -> dioxus::prelude::Element {
    use dioxus::prelude::*;
    rsx! { frame::PanelApp {} }
}
