//! Size probe: Rust and wasm-bindgen with no framework, one element written through web-sys.

#![forbid(unsafe_code)]

fn main() {
    #[cfg(target_arch = "wasm32")]
    {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("a document");
        let title = document.create_element("h1").expect("an element");
        title.set_class_name("hero-title");
        title.set_text_content(Some("Floor"));
        if let Some(main) = document.get_element_by_id("main") {
            let _ = main.append_child(&title);
        }
    }
}
