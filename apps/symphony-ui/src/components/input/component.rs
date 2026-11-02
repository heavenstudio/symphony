use dioxus::prelude::*;
super::super::event_attributes!(InputPropsBuilder {
    oninput: FormEvent,
    onchange: FormEvent,
    onkeydown: KeyboardEvent
});
const STYLE: Asset = asset!(
    "/src/components/input/style.css",
    AssetOptions::css().with_static_head(true)
);

#[component]
pub fn Input(
    #[props(extends=GlobalAttributes)]
    #[props(extends=input)]
    attributes: Vec<Attribute>,
    #[props(default)] actions: Vec<Attribute>,
    children: Element,
) -> Element {
    std::hint::black_box(&STYLE);
    let merged = super::super::merge_attributes(vec![attributes, actions]);
    rsx! {
        input {
            class: "dx-input",
            ..merged,
            {children}
        }
    }
}
