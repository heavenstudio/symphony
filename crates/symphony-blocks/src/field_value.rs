//! A board field's value, read the same by every reader that hands one: `serde_json` with or without
//! its `arbitrary_precision` feature, the browser's values and serde's own buffers.

use std::fmt;

use serde::de::{self, MapAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// A board field's value: a number, or text (a text, a choice or a link), written as the bare
/// number or string.
///
/// Read by hand: telling a number from text takes `deserialize_any`, where `serde_json` built with
/// `arbitrary_precision` (the server's build, the `tasks` binary's and every `--workspace` one,
/// docs/workflow/CHECKS.md) hands a float as a one-entry map holding its text, which a derived untagged enum
/// would refuse.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum FieldValue {
    Number(f64),
    Text(String),
}

/// The one key of the map `serde_json`'s `arbitrary_precision` hands a number as to
/// `deserialize_any`, its value the number's text.
const JSON_NUMBER: &str = "$serde_json::private::Number";

impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FieldValueVisitor)
    }
}

struct FieldValueVisitor;

impl<'de> Visitor<'de> for FieldValueVisitor {
    type Value = FieldValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a board field's value: a number or text")
    }

    fn visit_f64<E: de::Error>(self, number: f64) -> Result<FieldValue, E> {
        Ok(FieldValue::Number(number))
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a field's number is an f64, as JSON's readers take one: past 2^53 it is the nearest"
    )]
    fn visit_i64<E: de::Error>(self, number: i64) -> Result<FieldValue, E> {
        Ok(FieldValue::Number(number as f64))
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a field's number is an f64, as JSON's readers take one: past 2^53 it is the nearest"
    )]
    fn visit_u64<E: de::Error>(self, number: u64) -> Result<FieldValue, E> {
        Ok(FieldValue::Number(number as f64))
    }

    /// An integer past 64 bits, as `serde_json`'s `arbitrary_precision` hands one from a value.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a field's number is an f64, as JSON's readers take one: past 2^53 it is the nearest"
    )]
    fn visit_i128<E: de::Error>(self, number: i128) -> Result<FieldValue, E> {
        Ok(FieldValue::Number(number as f64))
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a field's number is an f64, as JSON's readers take one: past 2^53 it is the nearest"
    )]
    fn visit_u128<E: de::Error>(self, number: u128) -> Result<FieldValue, E> {
        Ok(FieldValue::Number(number as f64))
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<FieldValue, E> {
        Ok(FieldValue::Text(text.to_owned()))
    }

    fn visit_string<E: de::Error>(self, text: String) -> Result<FieldValue, E> {
        Ok(FieldValue::Text(text))
    }

    /// A number as `arbitrary_precision` hands it, `{JSON_NUMBER: "1.50"}`; any other map is
    /// refused, as is a number past an `f64`'s range, which `serde_json` refuses without the feature.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FieldValue, A::Error> {
        if map.next_key::<String>()?.as_deref() != Some(JSON_NUMBER) {
            return Err(de::Error::invalid_type(Unexpected::Map, &self));
        }
        let text: String = map.next_value()?;
        if map.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_type(Unexpected::Map, &self));
        }
        match text.parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(FieldValue::Number(number)),
            _ => Err(de::Error::invalid_value(
                Unexpected::Other(&format!("the number {text}")),
                &self,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::IntoDeserializer;
    use serde::de::value::Error as PlainError;
    use serde_json::{Value, json};

    /// Each way a number is written, with the `f64` it reads as. The forms past a float's shortest
    /// text (`3.0` aside) are the ones `serde_json` with `arbitrary_precision` keeps as written.
    const NUMBERS: [(&str, f64); 13] = [
        ("3", 3.0),
        ("-4", -4.0),
        ("0", 0.0),
        ("3.0", 3.0),
        ("1.5", 1.5),
        ("1.50", 1.5),
        ("-0.25", -0.25),
        ("1e2", 100.0),
        ("1E+2", 100.0),
        ("2.5e-3", 0.0025),
        ("0.30000000000000004441", 0.300_000_000_000_000_04),
        // Integers past 64 bits, read as the f64s 2^64 and -2^63, written in their shortest form.
        ("18446744073709551616", 18_446_744_073_709_552_000.0),
        ("-9223372036854775809", -9_223_372_036_854_776_000.0),
    ];

    #[test]
    fn a_number_reads_the_same_from_json_text_and_from_a_json_value_however_it_is_written() {
        for (text, number) in NUMBERS {
            assert_eq!(
                serde_json::from_str::<FieldValue>(text).unwrap(),
                FieldValue::Number(number),
                "{text} from text"
            );
            let value: Value = serde_json::from_str(text).unwrap();
            assert_eq!(
                serde_json::from_value::<FieldValue>(value).unwrap(),
                FieldValue::Number(number),
                "{text} from a value"
            );
        }
    }

    /// What a reader of plain values and `serde_json` without `arbitrary_precision`
    /// hand: a plain number or a string, never the map.
    #[test]
    fn a_plain_number_or_string_reads_as_a_number_or_text() {
        let float: de::value::F64Deserializer<PlainError> = 1.5.into_deserializer();
        assert_eq!(
            FieldValue::deserialize(float).unwrap(),
            FieldValue::Number(1.5)
        );
        let negative: de::value::I64Deserializer<PlainError> = (-4_i64).into_deserializer();
        assert_eq!(
            FieldValue::deserialize(negative).unwrap(),
            FieldValue::Number(-4.0)
        );
        let positive: de::value::U64Deserializer<PlainError> = 3_u64.into_deserializer();
        assert_eq!(
            FieldValue::deserialize(positive).unwrap(),
            FieldValue::Number(3.0)
        );
        let small: de::value::U8Deserializer<PlainError> = 7_u8.into_deserializer();
        assert_eq!(
            FieldValue::deserialize(small).unwrap(),
            FieldValue::Number(7.0)
        );
        let borrowed: de::value::StrDeserializer<'_, PlainError> = "high".into_deserializer();
        assert_eq!(
            FieldValue::deserialize(borrowed).unwrap(),
            FieldValue::Text("high".into())
        );
        let owned: de::value::StringDeserializer<PlainError> =
            String::from("docs/A.md").into_deserializer();
        assert_eq!(
            FieldValue::deserialize(owned).unwrap(),
            FieldValue::Text("docs/A.md".into())
        );
    }

    #[test]
    fn text_stays_text_even_when_it_reads_as_a_number() {
        for text in ["3", "1.50", "", "high", "$serde_json::private::Number"] {
            let json = serde_json::to_string(text).unwrap();
            assert_eq!(
                serde_json::from_str::<FieldValue>(&json).unwrap(),
                FieldValue::Text(text.into())
            );
            assert_eq!(
                serde_json::from_value::<FieldValue>(json!(text)).unwrap(),
                FieldValue::Text(text.into())
            );
        }
    }

    #[test]
    fn a_value_that_is_neither_a_number_nor_text_is_refused_naming_what_a_field_holds() {
        let refused = |text: &str| {
            let error = serde_json::from_str::<FieldValue>(text).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("a board field's value: a number or text"),
                "{text}: {error}"
            );
        };
        for text in ["true", "null", "[1]", "{}", r#"{"a":1}"#] {
            refused(text);
            let value: Value = serde_json::from_str(text).unwrap();
            assert!(
                serde_json::from_value::<FieldValue>(value).is_err(),
                "{text}"
            );
        }
        // The map `arbitrary_precision` hands, holding no number an f64 holds, or more than it.
        refused(r#"{"$serde_json::private::Number":"many"}"#);
        refused(r#"{"$serde_json::private::Number":"1e400"}"#);
        refused(r#"{"$serde_json::private::Number":"1","more":2}"#);
        // Past an f64's range: `serde_json` refuses it itself without `arbitrary_precision`.
        assert!(serde_json::from_str::<FieldValue>("1e400").is_err());
        assert!(serde_json::from_str::<FieldValue>("-1e400").is_err());
    }

    /// serde's buffers, which an untagged, flattened or internally tagged parent reads through: under
    /// `arbitrary_precision` a float reaches the field there as the map, kept as serde read it.
    #[test]
    fn a_field_inside_one_of_serdes_buffers_reads_as_it_reads_alone() {
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(untagged)]
        enum Either {
            Field(FieldValue),
        }
        #[derive(Debug, PartialEq, Deserialize)]
        struct Inner {
            value: FieldValue,
        }
        #[derive(Debug, PartialEq, Deserialize)]
        struct Outer {
            #[serde(flatten)]
            inner: Inner,
            version: i64,
        }
        for (text, number) in NUMBERS {
            assert_eq!(
                serde_json::from_str::<Either>(text).unwrap(),
                Either::Field(FieldValue::Number(number)),
                "{text}"
            );
            let record = format!(r#"{{"value":{text},"version":2}}"#);
            assert_eq!(
                serde_json::from_str::<Outer>(&record).unwrap().inner.value,
                FieldValue::Number(number),
                "{record}"
            );
        }
    }
}
