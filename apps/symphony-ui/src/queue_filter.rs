//! Document-folder filters for the review queue. Preferences belong to this device.
use crate::components::button::{Button, ButtonSize};
use crate::platform::Platform;
use dioxus::prelude::*;
use std::collections::BTreeMap;

const KEY: &str = "review:queue:you:category";

pub fn load(platform: &dyn Platform) -> String {
    platform.load(KEY).unwrap_or_default()
}

pub fn remember(platform: &dyn Platform, category: &str) {
    platform.save(KEY, category);
}

pub fn counts<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for path in paths {
        *counts
            .entry(symphony_api::queue_category(path).into())
            .or_default() += 1;
    }
    counts
}

/// A saved category absent from this snapshot shows All rather than an empty, hidden queue.
pub fn active<'a>(selected: &'a str, counts: &BTreeMap<String, usize>) -> &'a str {
    if counts.contains_key(selected) {
        selected
    } else {
        ""
    }
}

#[component]
pub fn CategoryChips(
    counts: BTreeMap<String, usize>,
    selected: String,
    onselect: EventHandler<String>,
) -> Element {
    let selected = active(&selected, &counts);
    let total: usize = counts.values().sum();
    rsx! {
        div { class: "queue-categories", role: "group", "aria-label": "Review categories",
            Button { size: ButtonSize::Sm, "aria-pressed": selected.is_empty(),
                "data-category": "", actions: crate::entry_events::click(move || onselect.call(String::new())),
                "All ({total})"
            }
            for (category, count) in &counts {
                Button { key: "{category}", size: ButtonSize::Sm,
                    "aria-pressed": selected == category, "data-category": "{category}",
                    actions: crate::entry_events::click({ let category = category.clone(); move || onselect.call(category.clone()) }),
                    "{category} ({count})"
                }
            }
        }
    }
}
