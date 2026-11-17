//! Ordered JSON objects share their insertion loop instead of compiling it at every field.
//!
//! Each caller constructs its values once; moving them into the object also avoids serializing
//! an already constructed `Value` again. The field order and duplicate-key behavior are those of
//! `serde_json::Map`, as in `json!` objects.

use serde_json::Value;

#[inline(never)]
pub(crate) fn object(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_objects_and_fields_match_the_json_macro_including_their_order() {
        assert_eq!(object(vec![]).to_string(), "{}");
        let fields = object(vec![
            ("z", json!(null)),
            ("é", json!("a\n\"😀")),
            ("integer", json!(i64::MAX)),
            ("unsigned", json!(u64::MAX)),
            ("float", json!(-0.0)),
            ("flag", json!(true)),
            ("array", json!([1, {"nested": "value"}])),
        ]);
        let expected = json!({
            "z": null,
            "é": "a\n\"😀",
            "integer": i64::MAX,
            "unsigned": u64::MAX,
            "float": -0.0,
            "flag": true,
            "array": [1, {"nested": "value"}],
        });
        assert_eq!(fields.to_string(), expected.to_string());
    }

    #[test]
    fn duplicate_keys_replace_values_without_moving_their_first_position() {
        let fields = object(vec![
            ("z", json!(1)),
            ("a", json!(2)),
            ("z", json!({"last": true})),
        ]);
        assert_eq!(fields.to_string(), "{\"z\":{\"last\":true},\"a\":2}");
    }
}
