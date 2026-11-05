//! Local JSON Schema validation with stable diagnostics and no external reference reads.

#![forbid(unsafe_code)]

mod schema;
pub mod text;

pub use schema::{STRICT, Schema, SchemaError, dollar_path, repr, validate};
