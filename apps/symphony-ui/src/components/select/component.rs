use dioxus::prelude::*;
use dioxus_icons::lucide::{Check, ChevronDown};
use dioxus_primitives::select::{
    self, SelectGroupLabelProps, SelectMultiProps, SelectOptionProps, SelectProps,
};
use dioxus_primitives::{dioxus_attributes::attributes, merge_attributes};

pub use dioxus_primitives::select::SelectGroup;

#[css_module("/src/components/select/style.css")]
struct Styles;

/// The Symphony's single-value fields keep their names and controlled form state.
#[component]
pub fn SelectPopup(
    id: Option<String>,
    label: String,
    value: String,
    options: Vec<(String, String)>,
    on_change: Callback<String>,
    #[props(default)] disabled: bool,
    external_trigger: Option<String>,
    on_close: Option<Callback<()>>,
) -> Element {
    let text = options
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let selected: ReadSignal<Option<String>> =
        use_memo(use_reactive!(|value| Some(value.clone()))).into();
    let initially_open = external_trigger.is_some();
    let mut open = use_signal(move || initially_open);
    // Keep one reactive open prop while the popup is mounted and its selection changes.
    let controlled_open = use_memo(move || Some(open()));
    let popup_id = use_hook(|| {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(target_arch = "wasm32")]
        let module = crate::module::kind();
        #[cfg(not(target_arch = "wasm32"))]
        let module = "host";
        format!("symphony-{module}-select-{number}")
    });
    let popup_id = external_trigger
        .as_ref()
        .map(|id| format!("{id}-popup"))
        .unwrap_or(popup_id);
    #[cfg(target_arch = "wasm32")]
    let positioned = use_hook(|| {
        std::rc::Rc::new(std::cell::RefCell::new(
            None::<super::position::AnchoredPopup>,
        ))
    });
    #[cfg(target_arch = "wasm32")]
    let mounted_position = positioned.clone();
    #[cfg(target_arch = "wasm32")]
    use_drop(move || {
        positioned.borrow_mut().take();
    });
    #[cfg(target_arch = "wasm32")]
    let mounted_trigger = external_trigger.clone();
    #[cfg(target_arch = "wasm32")]
    let mounted_id = popup_id.clone();
    let position = external_trigger
        .as_ref()
        .map(|_| "position:fixed;visibility:hidden");
    let popup_attributes = attributes!(div {
        class: Styles::dx_select_list,
        style: position,
        "aria-label": label.clone(),
        onmounted: move |event: MountedEvent| {
            #[cfg(target_arch = "wasm32")]
            if let Some(trigger) = mounted_trigger.clone() {
                *mounted_position.borrow_mut() = super::position::AnchoredPopup::new(trigger, mounted_id.clone());
            }
            spawn(async move {
                let _ = event.data().set_focus(true).await;
            });
        },
    });
    let root_attributes = attributes!(div {
        class: Styles::dx_select,
        "data-symphony-control": "select",
        onkeydown: move |event: KeyboardEvent| {
            if open() && event.key() == Key::Escape {
                event.prevent_default();
                event.stop_propagation();
                open.set(false);
                super::super::restore_trigger_focus();
            }
        },
    });
    rsx! {
        select::Select {
            value: Some(selected), disabled,
            open: controlled_open,
            on_value_change: move |value: Option<String>| if let Some(value) = value { on_change.call(value); },
            attributes: root_attributes,
            on_open_change: move |next: bool| {
                open.set(next);
                if !next {
                    super::super::restore_trigger_focus();
                    if let Some(on_close) = on_close { on_close.call(()); }
                }
            },
            select::SelectTrigger { id: if external_trigger.is_some() { None } else { id }, aria_label: label, class: Styles::dx_select_trigger,
                hidden: external_trigger.is_some(), tabindex: if external_trigger.is_some() { -1 } else { 0 },
                "data-symphony-trigger": "",
                span { "{text}" }
                ChevronDown { size: "20px" }
            }
            select::SelectList { id: popup_id, attributes: popup_attributes,
                for (index, (key, text)) in options.into_iter().enumerate() {
                    SelectOption::<String> {
                        key: "{key}", value: key.clone(), text_value: text.clone(), index,
                        attributes: { let chosen = key.clone(); vec![dioxus::events::onkeydown(move |event: KeyboardEvent| {
                            if event.key() == Key::Enter || event.key() == Key::Character(" ".into()) {
                                event.prevent_default();
                                event.stop_propagation();
                                on_change.call(chosen.clone());
                                open.set(false);
                                super::super::restore_trigger_focus();
                            }
                        })] },
                        "{text}"
                    }
                }
            }
        }
    }
}

#[component]
pub fn Select<T: Clone + PartialEq + 'static>(props: SelectProps<T>) -> Element {
    let base = attributes!(div {
        class: Styles::dx_select
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        select::Select {
            value: props.value,
            default_value: props.default_value,
            on_value_change: props.on_value_change,
            disabled: props.disabled,
            open: props.open,
            default_open: props.default_open,
            on_open_change: props.on_open_change,
            name: props.name,
            roving_loop: props.roving_loop,
            typeahead_timeout: props.typeahead_timeout,
            attributes: merged,
            select::SelectTrigger {
                class: Styles::dx_select_trigger,
                select::SelectValue {}
                ChevronDown {
                    class: "dx-select-expand-icon",
                    size: "20px",
                    stroke: "var(--primary-color-7)",
                }
            }
            select::SelectList {
                class: Styles::dx_select_list,
                {props.children}
            }
        }
    }
}

#[component]
pub fn SelectMulti<T: Clone + PartialEq + 'static>(props: SelectMultiProps<T>) -> Element {
    let base = attributes!(div {
        class: Styles::dx_select
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        select::SelectMulti {
            values: props.values,
            default_values: props.default_values,
            on_values_change: props.on_values_change,
            disabled: props.disabled,
            open: props.open,
            default_open: props.default_open,
            on_open_change: props.on_open_change,
            name: props.name,
            roving_loop: props.roving_loop,
            typeahead_timeout: props.typeahead_timeout,
            attributes: merged,
            select::SelectTrigger {
                class: Styles::dx_select_trigger,
                select::SelectValue {}
                ChevronDown {
                    class: "dx-select-expand-icon",
                    size: "20px",
                    stroke: "var(--primary-color-7)",
                }
            }
            select::SelectList {
                class: Styles::dx_select_list,
                {props.children}
            }
        }
    }
}

#[component]
pub fn SelectGroupLabel(props: SelectGroupLabelProps) -> Element {
    let base = attributes!(div {
        class: Styles::dx_select_group_label
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        select::SelectGroupLabel {
            id: props.id,
            attributes: merged,
            {props.children}
        }
    }
}

#[component]
pub fn SelectOption<T: Clone + PartialEq + 'static>(props: SelectOptionProps<T>) -> Element {
    let base = attributes!(div {
        class: Styles::dx_select_option
    });
    let merged = merge_attributes(vec![base, props.attributes]);

    rsx! {
        select::SelectOption::<T> {
            value: props.value,
            text_value: props.text_value,
            disabled: props.disabled,
            id: props.id,
            index: props.index,
            aria_label: props.aria_label,
            aria_roledescription: props.aria_roledescription,
            attributes: merged,
            {props.children}
            select::SelectItemIndicator {
                Check {
                    size: "1rem",
                    stroke: "var(--secondary-color-5)",
                }
            }
        }
    }
}
