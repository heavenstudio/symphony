//! The Symphony's client, launched by `dx` (apps/symphony-ui/bundle.sh builds it for the web).

fn main() {
    // The SPA shell calls the module's run/stop exports so page-kind changes can drop the
    // previous VirtualDom, its feed and listeners without reloading the document.
    #[cfg(target_arch = "wasm32")]
    {
        symphony_ui::module::install();
    }
}
