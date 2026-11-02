#[cfg(feature = "overlays")]
mod component;
#[cfg(target_arch = "wasm32")]
mod deferred;
#[cfg(all(feature = "overlays", any(target_arch = "wasm32", test)))]
mod position;
#[cfg(not(target_arch = "wasm32"))]
pub use component::SelectPopup as SelectField;
#[cfg(feature = "overlays")]
pub use component::*;
#[cfg(target_arch = "wasm32")]
pub use deferred::SelectField;
