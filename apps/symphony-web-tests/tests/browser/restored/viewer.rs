//! The viewer the gallery cases open their items in: a fixture element whose module script is made
//! here, as a project's renderer would name its own. It fetches its `src` when it connects (then
//! `loaded` is true) and counts the calls of its `settle` method, which each view names.

use serde_json::{Value, json};

/// Where the fixture's module is served.
pub(super) const SCRIPT: &str = "/static/fixture-viewer.js";

/// The module defining `fixture-viewer`.
pub(super) const MODULE: &[u8] =
    b"customElements.define('fixture-viewer', class extends HTMLElement {\n\
  connectedCallback() { fetch(this.getAttribute('src')).then(r => { this.loaded = r.ok; }); }\n\
  settle() { this.settled = (this.settled || 0) + 1; }\n\
});\n";

/// The viewer as a gallery names it: a `Turn` view and a `Back` one, each calling `settle`.
pub(super) fn viewer() -> Value {
    json!({
        "element": "fixture-viewer",
        "script": SCRIPT,
        "attributes": [["angle", "-45"], ["loading", "lazy"]],
        "views": [
            {"label": "Turn", "set": [["angle", "90"], ["zoom", "2"]], "call": "settle"},
            {"label": "Back", "set": [["angle", "-45"], ["zoom", "auto"]], "call": "settle"}
        ],
        "download": ".item"
    })
}
