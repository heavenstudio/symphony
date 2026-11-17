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
    fn direct_scalar_conversions_match_serialization_at_the_numeric_and_optional_edges() {
        assert_eq!(Value::from(i64::MIN), json!(i64::MIN));
        assert_eq!(Value::from(u64::MAX), json!(u64::MAX));
        assert_eq!(Value::from(None::<&str>), json!(null));
        assert_eq!(Value::from(Some("é\n😀")), json!("é\n😀"));
        assert_eq!(Value::from(true), json!(true));
        assert_eq!(Value::from(false), json!(false));
        for number in [
            -0.0,
            1.0,
            1e16,
            2.5e-7,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(Value::from(number).to_string(), json!(number).to_string());
            assert_eq!(
                Value::from(Some(number)).to_string(),
                json!(number).to_string()
            );
        }
        assert_eq!(Value::from(None::<f64>), json!(null));
    }

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
