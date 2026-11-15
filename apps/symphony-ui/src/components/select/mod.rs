// The chat's select field: its closed trigger, whose popup the web client's overlay draws
// (apps/symphony-web/src/select-popup.tsx).
#[cfg(target_arch = "wasm32")]
mod deferred;
#[cfg(target_arch = "wasm32")]
pub use deferred::SelectField;
