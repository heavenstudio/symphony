// Shared controls; README.md records their pinned source and Symphony adaptations.
pub mod button;
#[cfg(feature = "overlays")]
pub mod checkbox;
#[cfg(target_arch = "wasm32")]
pub(crate) mod deferred;

// Build rich listeners at the caller, only when supplied. Keeping optional adapters
// inside a shared control retains the complete browser event converter in every entry.
macro_rules! event_attributes {
    ($builder:ident { $($event:ident: $ty:ty),* $(,)? }) => {
        impl<State> $builder<State> where Self: dioxus::core::HasAttributes {
            $(pub fn $event<Marker>(self, handler: impl dioxus::core::SuperInto<dioxus::prelude::EventHandler<$ty>, Marker>) -> Self {
                let handler: dioxus::prelude::EventHandler<$ty> = dioxus::core::SuperInto::super_into(handler);
                let attr = dioxus::events::$event(handler);
                self.push_attribute(attr.name, attr.namespace, attr.value, attr.volatile)
            })*
        }
    };
}
pub(crate) use event_attributes;
#[cfg(feature = "overlays")]
pub mod dropdown_menu;
pub mod input;
#[cfg(feature = "overlays")]
pub mod label;
pub mod select;
#[cfg(feature = "overlays")]
pub mod tabs;
pub mod textarea;
#[cfg(feature = "overlays")]
pub mod tooltip;

/// Styled markup needs only class concatenation and last-writer attribute overrides.
/// Keeping this small helper here avoids linking the overlay runtime into entry modules.
fn merge_attributes(
    lists: Vec<Vec<dioxus::prelude::Attribute>>,
) -> Vec<dioxus::prelude::Attribute> {
    use dioxus::core::AttributeValue;
    let mut merged: Vec<dioxus::prelude::Attribute> = Vec::new();
    for attr in lists.into_iter().flatten() {
        if let Some(existing) = merged
            .iter_mut()
            .find(|old| old.name == attr.name && old.namespace == attr.namespace)
        {
            if attr.name == "class"
                && let (AttributeValue::Text(left), AttributeValue::Text(right)) =
                    (&mut existing.value, &attr.value)
            {
                *left = format!("{} {}", left.trim(), right.trim()).trim().into();
                existing.volatile |= attr.volatile;
            } else {
                *existing = attr;
            }
        } else {
            merged.push(attr);
        }
    }
    merged
}

/// Closing a popup keeps keyboard navigation on its originating field.
#[cfg(feature = "overlays")]
fn restore_trigger_focus() {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        if let Some(trigger) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element())
            .and_then(|active| active.closest("[data-symphony-control]").ok().flatten())
            .and_then(|root| {
                root.query_selector("[data-symphony-trigger]")
                    .ok()
                    .flatten()
            })
            .and_then(|trigger| trigger.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = trigger.focus();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::merge_attributes;
    use dioxus::core::AttributeValue;
    use dioxus::prelude::*;

    #[test]
    fn styled_markup_keeps_classes_and_overrides_values_within_their_namespace() {
        let attrs = merge_attributes(vec![
            vec![
                Attribute::new("class", " base ", None, false),
                Attribute::new("disabled", true, None, false),
                Attribute::new("width", "10", Some("style"), false),
            ],
            vec![
                Attribute::new("class", " extra ", None, true),
                Attribute::new("disabled", false, None, false),
                Attribute::new("width", "20", None, false),
            ],
        ]);
        assert_eq!(attrs.len(), 4);
        assert!(attrs[0].volatile);
        assert_eq!(attrs[0].value, AttributeValue::Text("base extra".into()));
        assert_eq!(attrs[1].value, AttributeValue::Bool(false));
        assert_eq!(attrs[2].namespace, Some("style"));
        assert_eq!(attrs[2].value, AttributeValue::Text("10".into()));
        assert_eq!(attrs[3].value, AttributeValue::Text("20".into()));
    }
}
