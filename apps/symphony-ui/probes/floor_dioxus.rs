//! Size probe: the smallest page the client's Dioxus build can make (same features), with one
//! signal and one plain action listener as `entry_events::action` attaches them.
use dioxus::core::AttributeValue;
use dioxus::prelude::*;

fn main() {
    #[cfg(target_arch = "wasm32")]
    dioxus::web::launch::launch_cfg(App, dioxus::web::Config::default().rootname("main"));
}

#[component]
fn App() -> Element {
    let mut count = use_signal(|| 0_u32);
    let click = vec![Attribute::new(
        "onclick",
        AttributeValue::listener(move |_: Event<dioxus::html::PlatformEventData>| count += 1),
        None,
        false,
    )];
    rsx! {
        main {
            h1 { class: "hero-title", "Floor" }
            button { ..click, "Clicked {count}" }
        }
    }
}
