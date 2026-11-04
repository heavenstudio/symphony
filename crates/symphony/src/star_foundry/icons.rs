//! The icon catalogue's labels, hover meanings and SVG shapes: the `capability`, `weapon`, `gear`,
//! `status` and `defence` components.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use symphony_plugin::vocabulary::{Block, Span, VectorShape};

#[derive(Deserialize)]
struct Icon {
    key: String,
    label: String,
    meaning: String,
    svg: String,
}

/// Each family's icons with their parsed shapes.
type Catalogue = HashMap<String, Vec<(Icon, Vec<VectorShape>)>>;

static ICONS: LazyLock<Catalogue> = LazyLock::new(|| {
    let catalogue: HashMap<String, Vec<Icon>> =
        serde_json::from_str(include_str!("icons.json")).expect("the embedded icon catalogue");
    catalogue
        .into_iter()
        .map(|(family, icons)| {
            let icons = icons
                .into_iter()
                .map(|icon| {
                    let shapes = crate::blocks::vector_shapes(&icon.svg);
                    (icon, shapes)
                })
                .collect();
            (family, icons)
        })
        .collect()
});

/// The icon `arg` of `family`, its name shown after it with the option `name`.
pub(super) fn render(family: &str, arg: &str, option: Option<&str>) -> Result<Vec<Block>, String> {
    let catalogue = ICONS
        .get(family)
        .ok_or_else(|| format!("unknown component {family}"))?;
    let key = arg.trim().to_lowercase();
    let (known, shape) = catalogue
        .iter()
        .find(|(icon, _)| icon.key == key)
        .ok_or_else(|| {
            format!(
                "{family}: no {family} is called {}; use {}",
                arg.trim(),
                catalogue
                    .iter()
                    .map(|(icon, _)| icon.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let option = option.unwrap_or("").trim();
    if !["", "name"].contains(&option) {
        return Err(format!("{family}: unknown option {option}; use name"));
    }
    Ok(vec![Block::Paragraph {
        spans: vec![Span::Icon {
            shape: shape.clone(),
            label: known.label.clone(),
            named: option == "name",
            tip: format!("{}: {}", known.label, known.meaning),
        }],
    }])
}
