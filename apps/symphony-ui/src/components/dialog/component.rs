use dioxus::prelude::*;
use dioxus_primitives::dialog::{self, DialogDescriptionProps, DialogRootProps, DialogTitleProps};
use dioxus_primitives::{dioxus_attributes::attributes, merge_attributes};

#[css_module("/src/components/dialog/style.css")]
struct Styles;

#[cfg(target_arch = "wasm32")]
const FOCUS: Asset = asset!("/src/components/dialog/focus-trap.js");

#[component]
pub fn Dialog(props: DialogRootProps) -> Element {
    let ready = use_signal(|| cfg!(not(target_arch = "wasm32")));
    let failure = use_signal(|| None::<String>);
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let mut ready = ready;
        let mut failure = failure;
        // A deferred dialog can be mounted open before the primitive's async script has
        // loaded. Await the same pinned support script before its focus effect runs.
        use_effect(move || {
            spawn(async move {
                let mut eval = dioxus::document::eval(
                    "const source=await dioxus.recv();try{await import(source);dioxus.send('')}catch(error){dioxus.send(String(error))}",
                );
                let _ = eval.send(FOCUS.to_string());
                match eval.recv::<String>().await {
                    Ok(message) if message.is_empty() => ready.set(true),
                    Ok(message) => failure.set(Some(message)),
                    Err(error) => failure.set(Some(error.to_string())),
                }
            });
        });
        let previous = use_hook(|| {
            web_sys::window()
                .and_then(|window| window.document())
                .and_then(|doc| doc.active_element())
                .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        });
        use_drop(move || {
            if let Some(previous) = previous {
                let _ = previous.focus();
            }
        });
    }
    let content_id = use_memo(move || (props.id)().map(|id| format!("{id}-content")));
    let base = attributes!(div {
        class: Styles::dx_dialog,
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        if let Some(why) = failure() { p { role: "alert", "Dialog focus support could not load: {why}" } }
        if ready() {
        dialog::DialogRoot {
            class: Styles::dx_dialog_backdrop,
            id: props.id,
            is_modal: props.is_modal,
            open: props.open,
            default_open: props.default_open,
            on_open_change: props.on_open_change,
            dialog::DialogContent {
                id: content_id,
                class: None,
                attributes: merged,
                {props.children}
            }
        }
        }
    }
}

#[component]
pub fn DialogTitle(props: DialogTitleProps) -> Element {
    let base = attributes!(h2 {
        class: Styles::dx_dialog_title,
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        dialog::DialogTitle {
            id: props.id,
            attributes: merged,
            {props.children}
        }
    }
}

#[component]
pub fn DialogDescription(props: DialogDescriptionProps) -> Element {
    let base = attributes!(p {
        class: Styles::dx_dialog_description,
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        dialog::DialogDescription {
            id: props.id,
            attributes: merged,
            {props.children}
        }
    }
}
