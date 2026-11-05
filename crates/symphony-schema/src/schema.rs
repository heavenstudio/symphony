//! Schema preparation, validation and public types for local document checks.

mod prepare;
mod types;
mod validator;

pub use types::{STRICT, SchemaError};
pub use validator::{Schema, dollar_path, repr, validate};
