//! Prepare local schemas and enforce the supported Draft 2020-12 vocabulary.

use super::types::{STRICT, SchemaError};
use crate::text::repr as repr_str;
use jsonschema::paths::Location;
use jsonschema::{Draft, Keyword, ValidationError, Validator};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Every keyword draft 2020-12's meta-schema names: its vocabularies' and the four its top schema
/// keeps for older drafts. A subschema that breaks the meta-schema loses its annotations, so the
/// strict check also calls each keyword of the nodes above it unevaluated; only a name outside this
/// list is one the draft lacks.
pub(super) const DRAFT_KEYWORDS: &[&str] = &[
    // core
    "$id",
    "$schema",
    "$ref",
    "$anchor",
    "$dynamicRef",
    "$dynamicAnchor",
    "$vocabulary",
    "$comment",
    "$defs",
    // applicator
    "prefixItems",
    "items",
    "contains",
    "additionalProperties",
    "properties",
    "patternProperties",
    "dependentSchemas",
    "propertyNames",
    "if",
    "then",
    "else",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    // unevaluated
    "unevaluatedItems",
    "unevaluatedProperties",
    // validation
    "type",
    "const",
    "enum",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxContains",
    "minContains",
    "maxProperties",
    "minProperties",
    "required",
    "dependentRequired",
    // meta-data
    "title",
    "description",
    "default",
    "deprecated",
    "readOnly",
    "writeOnly",
    "examples",
    // format annotation
    "format",
    // content
    "contentEncoding",
    "contentMediaType",
    "contentSchema",
    // the top schema's, for older drafts
    "definitions",
    "dependencies",
    "$recursiveAnchor",
    "$recursiveRef",
];

pub(super) fn strict() -> &'static Validator {
    static BUILT: OnceLock<Validator> = OnceLock::new();
    BUILT.get_or_init(|| {
        let meta: Value = serde_json::from_str(STRICT).expect("STRICT is JSON");
        jsonschema::options()
            .with_draft(Draft::Draft202012)
            .build(&meta)
            .expect("STRICT is a draft 2020-12 schema")
    })
}

/// `type` with an integer what Python's `json` reads as an `int` (no fraction, no exponent: `1.0` is a
/// float), and a boolean no number.
struct PythonType {
    names: Vec<String>,
}

pub(super) fn integer_token(number: &serde_json::Number) -> bool {
    !number.as_str().contains(['.', 'e', 'E'])
}

pub(super) fn nonfinite_numbers(value: &Value, at: &str, errors: &mut Vec<String>) {
    match value {
        // With arbitrary precision, as_f64 returns None for a token that overflows to infinity.
        Value::Number(number) if !integer_token(number) && number.as_f64().is_none() => {
            errors.push(format!("{at}: invalid number (must be finite)"));
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                nonfinite_numbers(item, &format!("{at}[{index}]"), errors);
            }
        }
        Value::Object(members) => {
            for (name, value) in members {
                nonfinite_numbers(value, &format!("{at}.{name}"), errors);
            }
        }
        _ => {}
    }
}

fn python_is(name: &str, value: &Value) -> bool {
    match name {
        "string" => value.is_string(),
        "integer" => value.as_number().is_some_and(integer_token),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => false,
    }
}

impl<'i> Keyword<'i> for PythonType {
    fn validate(&self, instance: &'i Value) -> Result<(), ValidationError<'i>> {
        if self.is_valid(instance) {
            Ok(())
        } else {
            Err(ValidationError::custom("wrong type"))
        }
    }

    fn is_valid(&self, instance: &'i Value) -> bool {
        self.names.iter().any(|name| python_is(name, instance))
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "The jsonschema keyword factory requires a fallible signature."
)]
pub(super) fn python_type_keyword<'a>(
    _parent: &'a Map<String, Value>,
    value: &'a Value,
    _path: Location,
) -> Result<Box<dyn for<'i> Keyword<'i>>, ValidationError<'a>> {
    let names = match value {
        Value::String(name) => vec![name.clone()],
        Value::Array(names) => names
            .iter()
            .filter_map(|name| name.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    Ok(Box::new(PythonType { names }))
}

/// `pattern` with each `$` that ends the text (one outside a class, not escaped) also taking one final
/// newline, as Python's `re` does: `\n?$`.
fn python_dollar(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 4);
    let (mut escaped, mut in_class) = (false, false);
    for c in pattern.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '$' if !in_class => {
                out.push_str("\\n?$");
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Refuse unenforced keywords and formats at every schema node, even an unused definition, and read
/// each `pattern` as Python's `re` reads it ([`python_dollar`]). A `const` or `enum` is data, left as
/// it is.
pub(super) fn prepare_schema(
    schema: &mut Value,
    at: &str,
    pointer: &str,
    types: &mut BTreeMap<String, String>,
) -> Result<(), SchemaError> {
    let Value::Object(node) = schema else {
        return Ok(());
    };
    let unsupported: Vec<&str> = [
        "$recursiveRef",
        "contentEncoding",
        "contentMediaType",
        "contentSchema",
    ]
    .into_iter()
    .filter(|name| node.contains_key(*name))
    .collect();
    if !unsupported.is_empty() {
        return Err(SchemaError(format!(
            "unsupported keyword(s): {} (at {at})",
            unsupported.join(", ")
        )));
    }
    if let Some(Value::String(name)) = node.get("format")
        && !jsonschema::options().is_known_format(Draft::Draft202012, name)
    {
        return Err(SchemaError(format!(
            "unsupported format {} (at {at})",
            repr_str(name)
        )));
    }
    if let Some(Value::String(pattern)) = node.get_mut("pattern") {
        *pattern = python_dollar(pattern);
    }
    for (keyword, value) in node.iter_mut() {
        match keyword.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
                if let Value::Object(schemas) = value {
                    for (name, schema) in schemas {
                        let escaped = name.replace('~', "~0").replace('/', "~1");
                        prepare_schema(
                            schema,
                            &format!("{at}.{keyword}.{name}"),
                            &format!("{pointer}/{keyword}/{escaped}"),
                            types,
                        )?;
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Value::Array(schemas) = value {
                    for (index, schema) in schemas.iter_mut().enumerate() {
                        prepare_schema(
                            schema,
                            &format!("{at}.{keyword}[{index}]"),
                            &format!("{pointer}/{keyword}/{index}"),
                            types,
                        )?;
                    }
                }
            }
            "additionalProperties"
            | "items"
            | "not"
            | "propertyNames"
            | "contains"
            | "if"
            | "then"
            | "else"
            | "unevaluatedItems"
            | "unevaluatedProperties" => prepare_schema(
                value,
                &format!("{at}.{keyword}"),
                &format!("{pointer}/{keyword}"),
                types,
            )?,
            _ => {}
        }
    }
    // Custom keyword errors do not carry the crate's reference traversal. A native double `not`
    // reports the same validity with that traversal intact, so stopping at a wrong type also works
    // through references. Keep its location mapped to the original `type` for the diagnostic.
    if let Some(kind) = node.remove("type") {
        let all = node
            .entry("allOf")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(all) = all {
            types.insert(
                format!("{pointer}/allOf/{}/not", all.len()),
                format!("{pointer}/type"),
            );
            all.push(serde_json::json!({"not": {"not": {"type": kind}}}));
        }
    }
    Ok(())
}
