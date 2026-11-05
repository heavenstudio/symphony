//! Schema refusal errors and the strict Draft 2020-12 meta-schema identity.

use std::fmt;

/// A schema the check cannot enforce as written: one the draft does not take, or a keyword it lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError(pub String);

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Draft 2020-12's meta-schema, but no keyword it does not know, at any depth: its vocabularies reach
/// every subschema through `$dynamicRef: "#meta"`, which this schema's own `$dynamicAnchor` takes over,
/// so each subschema is held to `unevaluatedProperties: false` as the top one is.
pub const STRICT: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:symphony:schema:draft-2020-12-strict",
  "$dynamicAnchor": "meta",
  "$ref": "https://json-schema.org/draft/2020-12/schema",
  "unevaluatedProperties": false
}"#;
