//! A shadcn registry theme is data; its cssVars apply to every page kind.
use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub css_vars: CssVars,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CssVars {
    #[serde(default)]
    pub theme: BTreeMap<String, String>,
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
}

impl Theme {
    /// CSS declarations only. A theme cannot introduce selectors, imports or remote assets.
    pub fn stylesheet(&self) -> Result<String, String> {
        fn declarations(tokens: &BTreeMap<String, String>) -> Result<String, String> {
            let mut result = String::new();
            for (key, value) in tokens {
                let key = key.strip_prefix("--").unwrap_or(key);
                if key.is_empty()
                    || !key
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    || value.trim().is_empty()
                    || value.contains([';', '{', '}', '<', '>'])
                    || value.to_ascii_lowercase().contains("url(")
                    || value.contains("/*")
                {
                    return Err(format!("Invalid theme token: {key}"));
                }
                // Older shadcn exports use HSL channels; current exports use full CSS colours.
                let value = if value.contains('%')
                    && !value.contains('(')
                    && value.split_whitespace().count() == 3
                    && value
                        .bytes()
                        .all(|c| c.is_ascii_digit() || b" .%+-".contains(&c))
                {
                    format!("hsl({value})")
                } else {
                    value.clone()
                };
                result.push_str(&format!("--{key}:{value};"));
            }
            Ok(result)
        }
        let common = declarations(&self.css_vars.theme)?;
        let light = declarations(&self.css_vars.light)?;
        let dark = declarations(&self.css_vars.dark)?;
        if common.is_empty() && light.is_empty() && dark.is_empty() {
            return Err("The theme has no cssVars tokens.".into());
        }
        Ok(format!(
            ":root[data-studio-theme]{{{common}{light}}}@media(prefers-color-scheme:dark){{:root[data-studio-theme]{{{dark}}}}}"
        ))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "page-panel"))]
mod live {
    use dioxus::prelude::*;
    use wasm_bindgen::JsCast;

    use crate::components::{dropdown_menu::*, input::Input};

    const KEY: &str = "studio.cssVars";

    fn install(css: &str) -> Result<(), String> {
        let window = web_sys::window().ok_or("No window")?;
        let document = window.document().ok_or("No document")?;
        let style = match document.get_element_by_id("studio-theme") {
            Some(style) => style,
            None => {
                let style = document
                    .create_element("style")
                    .map_err(|e| format!("{e:?}"))?;
                style.set_id("studio-theme");
                document
                    .head()
                    .ok_or("No document head")?
                    .append_child(&style)
                    .map_err(|e| format!("{e:?}"))?;
                style
            }
        };
        style.set_text_content(Some(css));
        document
            .document_element()
            .ok_or("No document root")?
            .set_attribute("data-studio-theme", "")
            .map_err(|e| format!("{e:?}"))?;
        if let Ok(Some(storage)) = window.local_storage() {
            let key = crate::project::stored(&crate::project::current(), KEY);
            let _ = storage.set_item(&key, css);
        }
        Ok(())
    }

    #[component]
    pub fn ThemeMenu() -> Element {
        let mut importing = use_signal(|| false);
        let mut status = use_signal(String::new);
        rsx! {
            div { class: "theme-menu",
                DropdownMenu {
                    DropdownMenuTrigger { "aria-label": "Appearance", "Appearance" }
                    DropdownMenuContent {
                        DropdownMenuItem::<String> { value: "import".to_string(), index: 0usize, on_select: move |_| importing.set(true), "Import shadcn theme…" }
                        DropdownMenuItem::<String> { value: "reset".to_string(), index: 1usize, on_select: move |_| {
                            match install("") {
                                Ok(()) => status.set("Default theme restored.".into()),
                                Err(why) => status.set(why),
                            }
                        }, "Default theme" }
                    }
                }
                if importing() {
                    label { r#for: "theme-file", "Theme JSON" }
                    Input { id: "theme-file", r#type: "file", accept: ".json,application/json", onchange: move |_| {
                        let Some(file) = web_sys::window()
                            .and_then(|window| window.document())
                            .and_then(|document| document.get_element_by_id("theme-file"))
                            .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
                            .and_then(|input| input.files())
                            .and_then(|files| files.item(0)) else { return };
                        spawn(async move {
                            let result = async {
                                let text = wasm_bindgen_futures::JsFuture::from(file.text()).await.map_err(|e| format!("{e:?}"))?;
                                let value = js_sys::JSON::parse(&text.as_string().unwrap_or_default()).map_err(|_| "The theme is not valid JSON.".to_string())?;
                                let theme: super::Theme = serde_wasm_bindgen::from_value(value).map_err(|_| "The theme needs cssVars with theme, light or dark tokens.".to_string())?;
                                install(&theme.stylesheet()?)
                            }.await;
                            match result {
                                Ok(()) => { status.set("Theme applied.".into()); importing.set(false); }
                                Err(why) => status.set(why),
                            }
                        });
                    } }
                }
                p { role: "status", "{status}" }
            }
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "page-panel"))]
pub use live::ThemeMenu;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exports_preserve_fonts_radius_modes_and_explicit_statuses() {
        let theme: Theme = serde_json::from_str(r##"{"name":"Copper","type":"registry:style","cssVars":{"theme":{"font-sans":"Georgia, serif","radius":"1rem"},"light":{"background":"0 0% 98%","primary":"oklch(0.6 0.1 45)"},"dark":{"background":"#121212","like":"#00aa00"}}}"##).unwrap();
        assert_eq!(
            theme.stylesheet().unwrap(),
            ":root[data-studio-theme]{--font-sans:Georgia, serif;--radius:1rem;--background:hsl(0 0% 98%);--primary:oklch(0.6 0.1 45);}@media(prefers-color-scheme:dark){:root[data-studio-theme]{--background:#121212;--like:#00aa00;}}"
        );
        assert!(!theme.stylesheet().unwrap().contains("--question:"));
    }

    #[test]
    fn invalid_exports_cannot_replace_the_current_theme() {
        assert!(serde_json::from_str::<Theme>(r#"{"colors":{}}"#).is_err());
        assert!(serde_json::from_str::<Theme>(r#"{"cssVars":{"light":{"radius":4}}}"#).is_err());
        assert!(
            Theme {
                css_vars: CssVars::default()
            }
            .stylesheet()
            .is_err()
        );
        for (key, value) in [
            ("", "red"),
            ("a}body", "red"),
            ("primary", "red;}body{color:red"),
            ("font-sans", "url(https://example.com/font)"),
            ("primary", "red/*"),
            ("radius", " "),
        ] {
            let theme = Theme {
                css_vars: CssVars {
                    light: BTreeMap::from([(key.into(), value.into())]),
                    ..CssVars::default()
                },
            };
            assert!(theme.stylesheet().is_err(), "{key}: {value}");
        }
    }
}
