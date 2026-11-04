//! Draft 2020-12 JSON Schema checks for document settings.
//! Files name local schemas with $schema; jsonschema has no default features.
//! References resolve within that schema: nothing is fetched or read externally.
//!
//! Refuse unenforced content annotations, legacy recursive references and
//! unknown keywords instead of accepting a schema stricter than enforcement.
//! STRICT checks the draft meta-schema with unevaluatedProperties:false at
//! every subschema.
//!
//! Integers have no fraction or exponent and retain arbitrary precision.
//! Fraction/exponent tokens must fit finite f64. Pattern $ accepts one final
//! newline. A wrong-typed node reports only its type. Diagnostic order is `not`,
//! `type`, `minimum`, `const`, `enum`, `pattern`, `minItems`, `items`,
//! `dependentRequired`, `required`, then members in file order. A reference target's lines are ordered
//! with the node's own. These semantics keep diagnostics and file checks stable.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use jsonschema::error::{TypeKind, ValidationErrorKind};
use jsonschema::paths::Location;
use jsonschema::{Draft, Keyword, ValidationError, Validator};
use serde_json::{Map, Value};

use crate::text::repr as repr_str;

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

/// Every keyword draft 2020-12's meta-schema names: its vocabularies' and the four its top schema
/// keeps for older drafts. A subschema that breaks the meta-schema loses its annotations, so the
/// strict check also calls each keyword of the nodes above it unevaluated; only a name outside this
/// list is one the draft lacks.
const DRAFT_KEYWORDS: &[&str] = &[
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

fn strict() -> &'static Validator {
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

fn integer_token(number: &serde_json::Number) -> bool {
    !number.as_str().contains(['.', 'e', 'E'])
}

fn nonfinite_numbers(value: &Value, at: &str, errors: &mut Vec<String>) {
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

fn python_type_keyword<'a>(
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
fn prepare_schema(
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

/// A schema checked and built once, for checking many files against it.
pub struct Schema {
    validator: Validator,
    /// As written, for what a line names that the crate's error does not carry (a `type` list in its
    /// order, a pattern as written, the fields of a `dependentRequired` rule).
    schema: Value,
    /// Each native wrapper's location and the `type` keyword it reports for.
    types: BTreeMap<String, String>,
}

// Where in the walk of one node a keyword's line comes (the diagnostic order in the module doc). A
// keyword reached through a `$ref` is placed by where it is written, so a `$ref` is passed through,
// its target's lines ordered as the node's own.
const NOT: u8 = 0;
const TYPE: u8 = 1;
const MINIMUM: u8 = 2;
const CONST: u8 = 3;
const ENUM: u8 = 4;
const PATTERN: u8 = 5;
const MIN_ITEMS: u8 = 6;
const ITEMS: u8 = 7;
const DEPENDENT: u8 = 8;
const REQUIRED: u8 = 9;
const MEMBERS: u8 = 10;
const OTHER: u8 = 11;

/// A member's name, then its value.
const NAME: u8 = 0;
const VALUE: u8 = 1;

/// Where the walk writes a line: at each value passed on the way down, the member's place in the
/// file (its name before its value) or the item's index, then the keyword's place in the walk of the
/// value the line is about.
type Key = Vec<(u8, usize, u8)>;

fn rank(keyword: &str) -> u8 {
    match keyword {
        "not" => NOT,
        "type" => TYPE,
        "minimum" => MINIMUM,
        "const" => CONST,
        "enum" => ENUM,
        "pattern" => PATTERN,
        "minItems" => MIN_ITEMS,
        "items" => ITEMS,
        "dependentRequired" => DEPENDENT,
        "required" => REQUIRED,
        "properties" | "additionalProperties" | "propertyNames" => MEMBERS,
        _ => OTHER,
    }
}

/// The keyword that wrote `error`: the last step of where it is written.
fn keyword<'e>(error: &'e ValidationError<'_>) -> &'e str {
    let schema_path = error.schema_path().as_str();
    schema_path.rsplit('/').next().unwrap_or(schema_path)
}

fn steps(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .skip(1)
        .map(|raw| raw.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// `name`'s place among the members of `node`, in the file's order, or an item's index.
fn position(node: Option<&Value>, step: &str) -> usize {
    match node {
        Some(Value::Object(members)) => members
            .keys()
            .position(|name| name == step)
            .unwrap_or(usize::MAX),
        Some(Value::Array(_)) => step.parse().unwrap_or(usize::MAX),
        _ => usize::MAX,
    }
}

fn child<'v>(node: Option<&'v Value>, step: &str) -> Option<&'v Value> {
    match node? {
        Value::Object(members) => members.get(step),
        Value::Array(items) => items.get(step.parse::<usize>().ok()?),
        _ => None,
    }
}

/// Where the walk of `root` goes down to the value at `pointer` ([`Key`]): a list's items after
/// its own lines, an object's members after its own, each member's value after its name.
fn place(root: &Value, pointer: &str) -> Key {
    let mut key = Key::new();
    let mut node = Some(root);
    for step in steps(pointer) {
        key.push(match node {
            Some(Value::Array(_)) => (ITEMS, position(node, &step), 0),
            _ => (MEMBERS, position(node, &step), VALUE),
        });
        node = child(node, &step);
    }
    key
}

/// `pointer` without its last step.
fn parent(pointer: &str) -> &str {
    pointer.rsplit_once('/').map_or("", |(head, _)| head)
}

fn is_type(error: &ValidationError<'_>) -> bool {
    matches!(error.kind(), ValidationErrorKind::Custom { keyword, .. } if keyword == "type")
        || matches!(error.kind(), ValidationErrorKind::Type { .. })
}

/// Whether the walk never reaches `error`: it stops at the node whose type is wrong (the schema node
/// reached at evaluation path `node`, on the value at `at`) before the node's other keywords, though
/// after its `not` and what its `$ref` reaches.
fn stopped(error: &ValidationError<'_>, at: &str, node: &str) -> bool {
    let (path, schema) = (
        error.instance_path().as_str(),
        error.evaluation_path().as_str(),
    );
    let below = path == at || path.starts_with(&format!("{at}/"));
    let Some(rest) = schema.strip_prefix(node).and_then(|r| r.strip_prefix('/')) else {
        return false;
    };
    let next = rest.split('/').next().unwrap_or("");
    below && !matches!(next, "not" | "type" | "$ref")
}

impl Schema {
    /// `schema`, refused when the check cannot enforce it as written.
    pub fn compile(schema: &Value) -> Result<Schema, SchemaError> {
        let mut nonfinite = Vec::new();
        nonfinite_numbers(schema, "$", &mut nonfinite);
        if !nonfinite.is_empty() {
            return Err(SchemaError(nonfinite.join("; ")));
        }
        let mut built = schema.clone();
        let mut types = BTreeMap::new();
        prepare_schema(&mut built, "$", "", &mut types)?;
        let mut refusals: Vec<String> = Vec::new();
        for error in strict().iter_errors(schema) {
            if let Some(line) = refusal(schema, &error)
                && !refusals.contains(&line)
            {
                refusals.push(line);
            }
        }
        if !refusals.is_empty() {
            return Err(SchemaError(refusals.join("; ")));
        }
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .should_validate_formats(true)
            .should_ignore_unknown_formats(false)
            .with_keyword("type", python_type_keyword)
            .build(&built)
            .map_err(|e| SchemaError(format!("not a schema the check can build: {e}")))?;
        Ok(Schema {
            validator,
            schema: schema.clone(),
            types,
        })
    }

    fn type_path(&self, error: &ValidationError<'_>) -> Option<&str> {
        self.types
            .get(error.schema_path().as_str())
            .map(String::as_str)
    }

    fn is_type(&self, error: &ValidationError<'_>) -> bool {
        self.type_path(error).is_some() || is_type(error)
    }

    /// The evaluation path of the node whose type failed, before the native wrapper we added.
    fn type_node<'e>(&self, error: &'e ValidationError<'_>) -> Option<&'e str> {
        let path = error.evaluation_path().as_str();
        if self.type_path(error).is_some() {
            Some(parent(parent(parent(path))))
        } else if is_type(error) {
            Some(parent(path))
        } else {
            None
        }
    }

    /// Every way `instance` breaks this schema, as `<path>: <problem>` lines, the path written from `$`
    /// (`$.a.b`, `$[1]`); empty when it is valid. The core keywords read in Python's terms (`expected
    /// string, got int`), any other as the crate writes it; the lines come in the diagnostic order the
    /// module doc gives. Overflowing float tokens
    /// are rejected before keyword evaluation, even where the schema leaves a value unconstrained.
    pub fn errors(&self, instance: &Value) -> Vec<String> {
        let mut nonfinite = Vec::new();
        nonfinite_numbers(instance, "$", &mut nonfinite);
        if !nonfinite.is_empty() {
            return nonfinite;
        }
        let found: Vec<ValidationError<'_>> = self.validator.iter_errors(instance).collect();
        let stops: Vec<(String, String)> = found
            .iter()
            .filter_map(|error| {
                self.type_node(error)
                    .map(|node| (error.instance_path().as_str().to_string(), node.to_string()))
            })
            .collect();
        let mut placed: Vec<(Key, String)> = Vec::new();
        let mut dependent: BTreeSet<(String, String)> = BTreeSet::new();
        for error in &found {
            let own_node = self.type_node(error);
            if stops.iter().any(|(at, node)| {
                !(error.instance_path().as_str() == at && own_node == Some(node.as_str()))
                    && stopped(error, at, node)
            }) {
                continue;
            }
            self.lines(instance, error, &mut dependent, &mut placed);
        }
        placed.sort_by(|a, b| a.0.cmp(&b.0));
        placed.into_iter().map(|(_, line)| line).collect()
    }

    /// `error`'s lines, each with where the walk writes it.
    fn lines(
        &self,
        root: &Value,
        error: &ValidationError<'_>,
        dependent: &mut BTreeSet<(String, String)>,
        placed: &mut Vec<(Key, String)>,
    ) {
        let (path, schema_path) = (error.instance_path().as_str(), error.schema_path().as_str());
        let at = dollar_path(root, path);
        let node = root.pointer(path);
        let mut key = place(root, path);
        match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected } => {
                for name in unexpected {
                    let mut key = key.clone();
                    key.push((MEMBERS, position(node, name), VALUE));
                    placed.push((key, format!("{at}: {} is not allowed here", repr_str(name))));
                }
            }
            ValidationErrorKind::PropertyNames { error: inner } => {
                let name = inner.instance().as_str().unwrap_or_default().to_string();
                key.push((MEMBERS, position(node, &name), NAME));
                let rank = if self.is_type(inner) {
                    TYPE
                } else {
                    kind_rank(inner.kind())
                };
                key.push((rank, 0, 0));
                let line = format!("{at}.{name} (name): {}", self.problem(inner));
                placed.push((key, line));
            }
            ValidationErrorKind::Required { .. } if keyword(error) == "dependentRequired" => {
                if dependent.insert((path.to_string(), schema_path.to_string())) {
                    key.push((DEPENDENT, 0, 0));
                    for line in self.dependent_lines(&at, node, schema_path) {
                        placed.push((key.clone(), line));
                    }
                }
            }
            _ => {
                let rank = if self.is_type(error) {
                    TYPE
                } else {
                    rank(keyword(error))
                };
                key.push((rank, 0, 0));
                placed.push((key, format!("{at}: {}", self.problem(error))));
            }
        }
    }

    /// What `error` says of its value: the subset's own words for each keyword it knows, the
    /// validator's for any other.
    fn problem(&self, error: &ValidationError<'_>) -> String {
        let value = error.instance();
        let schema_path = error.schema_path().as_str();
        if let Some(path) = self.type_path(error) {
            return self.type_problem(path, value);
        }
        match error.kind() {
            ValidationErrorKind::Custom { keyword, .. } if keyword == "type" => {
                self.type_problem(schema_path, value)
            }
            ValidationErrorKind::Type { kind } => {
                let named = match kind {
                    TypeKind::Single(name) => name.to_string(),
                    TypeKind::Multiple(names) => {
                        let names: Vec<String> = names
                            .iter()
                            .map(|name| repr_str(&name.to_string()))
                            .collect();
                        format!("[{}]", names.join(", "))
                    }
                };
                format!("expected {named}, got {}", python_type(value))
            }
            ValidationErrorKind::Minimum { limit } => format!("must be at least {limit}"),
            ValidationErrorKind::Constant { expected_value } => {
                format!("must be {}", repr(expected_value))
            }
            ValidationErrorKind::Enum { options } => {
                let options: Vec<String> = match options {
                    Value::Array(options) => options.iter().map(repr).collect(),
                    other => vec![repr(other)],
                };
                format!("{} is not one of {}", repr(value), options.join(", "))
            }
            ValidationErrorKind::Pattern { pattern } => {
                let written = self
                    .node(schema_path)
                    .and_then(Value::as_str)
                    .unwrap_or(pattern);
                format!("{} does not match {written}", repr(value))
            }
            ValidationErrorKind::MinItems { limit } => format!("needs at least {limit} item(s)"),
            ValidationErrorKind::Required { property } => {
                format!("missing required {}", repr(property))
            }
            ValidationErrorKind::Not { .. } => "must not match forbidden schema".to_string(),
            _ => error.to_string(),
        }
    }

    fn type_problem(&self, path: &str, value: &Value) -> String {
        let named = match self.node(path) {
            Some(Value::String(name)) => name.clone(),
            Some(names @ Value::Array(_)) => repr(names),
            _ => "?".to_string(),
        };
        format!("expected {named}, got {}", python_type(value))
    }

    /// The lines for the `dependentRequired` rule at `schema_path` on the object `node`: each
    /// field it holds, in the rule's order, then each field that one asks for and it lacks.
    fn dependent_lines(&self, at: &str, node: Option<&Value>, schema_path: &str) -> Vec<String> {
        let (Some(Value::Object(rules)), Some(Value::Object(members))) =
            (self.node(schema_path), node)
        else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for (trigger, needs) in rules {
            if !members.contains_key(trigger) {
                continue;
            }
            for need in needs.as_array().into_iter().flatten() {
                if !need.as_str().is_some_and(|name| members.contains_key(name)) {
                    lines.push(format!(
                        "{at}: {} requires {}",
                        repr_str(trigger),
                        repr(need)
                    ));
                }
            }
        }
        lines
    }

    /// The schema's node at `pointer`, a local `$ref` on the way followed.
    fn node(&self, pointer: &str) -> Option<&Value> {
        let mut node = &self.schema;
        for step in steps(pointer) {
            node = if step == "$ref" {
                let target = node.get("$ref")?.as_str()?.strip_prefix('#')?;
                self.schema.pointer(target)?
            } else {
                match node {
                    Value::Object(members) => members.get(&step)?,
                    Value::Array(items) => items.get(step.parse::<usize>().ok()?)?,
                    _ => return None,
                }
            };
        }
        Some(node)
    }
}

/// The place in a node's walk of the keyword `kind` comes from.
fn kind_rank(kind: &ValidationErrorKind) -> u8 {
    match kind {
        ValidationErrorKind::Custom { keyword, .. } => rank(keyword),
        ValidationErrorKind::Type { .. } => TYPE,
        ValidationErrorKind::Minimum { .. } => MINIMUM,
        ValidationErrorKind::Constant { .. } => CONST,
        ValidationErrorKind::Enum { .. } => ENUM,
        ValidationErrorKind::Pattern { .. } => PATTERN,
        ValidationErrorKind::MinItems { .. } => MIN_ITEMS,
        ValidationErrorKind::Not { .. } => NOT,
        _ => OTHER,
    }
}

/// Python's name for the type of `value`, as `type(value).__name__` gives it.
fn python_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if integer_token(number) => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `value` as Python's `repr` writes the value `json.load` reads it as.
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => repr_str(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(name, value)| format!("{}: {}", repr_str(name), repr(value)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
    }
}

/// Every way `instance` breaks `schema` ([`Schema::errors`]); a schema the check cannot enforce is
/// refused instead.
pub fn validate(instance: &Value, schema: &Value) -> Result<Vec<String>, SchemaError> {
    // Key by the schema's actual bytes, including object order (which controls diagnostics).
    // A file edit therefore compiles a new validator; repeated renders reuse the old one.
    static COMPILED: LazyLock<Mutex<BTreeMap<String, Arc<Schema>>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    let key = serde_json::to_string(schema).expect("a JSON schema serializes");
    let mut compiled = COMPILED.lock().unwrap_or_else(|e| e.into_inner());
    let validator = if let Some(validator) = compiled.get(&key) {
        validator.clone()
    } else {
        let validator = Arc::new(Schema::compile(schema)?);
        // Bound retention when a long-lived server sees many revisions of a schema.
        if compiled.len() >= 32 {
            compiled.clear();
        }
        compiled.insert(key, validator.clone());
        validator
    };
    drop(compiled);
    Ok(validator.errors(instance))
}

/// Why the strict meta-schema refused `schema` at one place; none when all it names there are keywords
/// of the draft (a node above a misspelt one, which loses its annotations with it).
fn refusal(schema: &Value, error: &ValidationError<'_>) -> Option<String> {
    let at = dollar_path(schema, error.instance_path().as_str());
    let place = if at == "$" {
        String::new()
    } else {
        format!(" (at {at})")
    };
    match error.kind() {
        ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            let mut unknown: Vec<&str> = unexpected
                .iter()
                .map(String::as_str)
                .filter(|name| !DRAFT_KEYWORDS.contains(name))
                .collect();
            unknown.sort_unstable();
            unknown.dedup();
            (!unknown.is_empty())
                .then(|| format!("unsupported keyword(s): {}{place}", unknown.join(", ")))
        }
        _ => Some(format!("not a draft 2020-12 schema: {error}{place}")),
    }
}

/// A JSON pointer into `root` written from `$`: `.key` for an object's member, `[n]` for an array's
/// item, each step read as the container there holds it (an object key `0` stays a key).
pub fn dollar_path(root: &Value, pointer: &str) -> String {
    let mut path = String::from("$");
    let mut node = Some(root);
    for step in steps(pointer) {
        node = match node {
            Some(Value::Array(items)) => {
                path.push_str(&format!("[{step}]"));
                step.parse::<usize>().ok().and_then(|i| items.get(i))
            }
            Some(Value::Object(members)) => {
                path.push('.');
                path.push_str(&step);
                members.get(&step)
            }
            _ => {
                path.push('.');
                path.push_str(&step);
                None
            }
        };
    }
    path
}
