//! The icon catalogue's labels, hover meanings and SVG shapes, moved verbatim from the components.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::markup::escape;

#[derive(Deserialize)]
struct Icon {
    key: String,
    label: String,
    meaning: String,
    svg: String,
}

static ICONS: LazyLock<HashMap<String, Vec<Icon>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("component_icons.json")).expect("the embedded icon catalogue")
});

pub(super) fn render(family: &str, arg: &str, option: Option<&str>) -> Result<String, String> {
    let catalogue = &ICONS[family];
    let key = arg.trim().to_lowercase();
    let known = catalogue
        .iter()
        .find(|icon| icon.key == key)
        .ok_or_else(|| {
            format!(
                "{family}: no {family} is called {}; use {}",
                arg.trim(),
                catalogue
                    .iter()
                    .map(|i| i.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let option = option.unwrap_or("").trim();
    if !["", "name"].contains(&option) {
        return Err(format!("{family}: unknown option {option}; use name"));
    }
    let css = if family == "status" {
        "status-effect"
    } else {
        family
    };
    let icon = format!(
        "<svg viewBox=\"0 0 24 24\" width=\"18\" height=\"18\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\">{}</svg>",
        known.svg
    );
    let name = if option == "name" {
        format!("<span class=\"{css}-name\">{}</span>", escape(&known.label))
    } else {
        String::new()
    };
    let tip = escape(&format!("{}: {}", known.label, known.meaning));
    Ok(format!(
        "<span class=\"{css} {family}-{key}\" data-tip=\"{tip}\" tabindex=\"0\" aria-label=\"{}\" role=\"img\">{icon}{name}</span>",
        escape(&known.label)
    ))
}
