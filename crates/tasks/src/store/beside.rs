//! A read type whose own fields sit beside another type's in one record: a task's row and its
//! `version`, a session's binding and its `handle`.
//!
//! That is what serde's `flatten` reads, but `flatten` holds the record in serde's own buffer first,
//! and under `serde_json`'s `arbitrary_precision` (as the board links it: the `tasks` binary, the
//! server and every run of the board's tests, docs/workflow/CHECKS.md) a float reaches that buffer as a
//! one-entry map of its text, which an `f64` refuses: every float read from JSON text, `1.5`
//! included, and one read from a parsed [`Value`] unless its text is the float's shortest form.
//! [`Beside`] holds the record as a [`Value`] instead, which keeps a number as the build reads it and
//! gives it to an `f64` in every build, and hands the other type what `flatten` hands it: the fields
//! it names.

use serde::de::{DeserializeOwned, Error, Visitor};
use serde::{Deserialize, Deserializer, forward_to_deserialize_any};
use serde_json::{Map, Value};

/// One record, read whole: its own fields are taken out by name, and the rest read as one type.
pub(super) struct Beside(Map<String, Value>);

/// The fields left, handed to a struct as `flatten` hands them: only those it names (its aliases
/// among them), so a field the record holds beside them (another type's, or one a newer schema
/// added) is not its to refuse, though the struct refuses unknown fields read alone.
struct Named(Map<String, Value>);

impl<'de> Deserializer<'de> for Named {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        Value::Object(self.0).deserialize_any(visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let mut named = self.0;
        named.retain(|key, _| fields.contains(&key.as_str()));
        Value::Object(named).deserialize_struct(name, fields, visitor)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option
        unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}

impl Beside {
    pub(super) fn read<'de, D: Deserializer<'de>>(from: D) -> Result<Beside, D::Error> {
        Map::deserialize(from).map(Beside)
    }

    /// Its field `name`, taken out of it; one it lacks is refused as a derived reader refuses it.
    pub(super) fn take<T: DeserializeOwned, E: Error>(
        &mut self,
        name: &'static str,
    ) -> Result<T, E> {
        let value = self.0.remove(name).ok_or_else(|| E::missing_field(name))?;
        T::deserialize(value).map_err(E::custom)
    }

    /// The fields not taken that `T` names, read as `T` ([`Named`]).
    pub(super) fn rest<T: DeserializeOwned, E: Error>(self) -> Result<T, E> {
        T::deserialize(Named(self.0)).map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Row {
        #[serde(alias = "label")]
        name: String,
        at: f64,
        until: Option<f64>,
    }

    #[derive(Debug, PartialEq)]
    struct Versioned {
        row: Row,
        version: i64,
    }

    impl<'de> Deserialize<'de> for Versioned {
        fn deserialize<D: Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
            let mut record = Beside::read(from)?;
            Ok(Versioned {
                version: record.take("version")?,
                row: record.rest()?,
            })
        }
    }

    /// The board links `serde_json` with the features the server's build unifies on it, as a dependency
    /// of its own (its Cargo.toml) and with no dev-dependency beside it, so the `tasks` binary, the
    /// server's board and these tests are one configuration: keys kept in the order written
    /// (`preserve_order`), a number kept as its text (`arbitrary_precision`), a float read exactly
    /// (`float_roundtrip`) and `RawValue` (`raw_value`). Without one of them a test here could pass
    /// alone and fail in the workspace's run, or test a board no build makes.
    #[test]
    fn the_board_links_serde_json_as_the_server_does() {
        let value: Value = serde_json::from_str(r#"{"b":1.50,"a":2}"#).unwrap();
        assert_eq!(value.to_string(), r#"{"b":1.50,"a":2}"#);
        // One ulp off without float_roundtrip: 51.24817837550541.
        let float: f64 = serde_json::from_str("51.248178375505404").unwrap();
        assert_eq!(float.to_bits(), 51.248_178_375_505_404_f64.to_bits());
        let raw = serde_json::value::RawValue::from_string("[1.50]".into()).unwrap();
        assert_eq!(raw.get(), "[1.50]");
    }

    /// Floats as a reader that keeps a number's text holds them: past their shortest form, in
    /// exponent form, as a whole number.
    #[test]
    fn a_record_reads_beside_its_own_fields_from_text_and_from_a_value_however_a_float_is_written()
    {
        for (at, until, want_at, want_until) in [
            ("1762912345.25", "null", 1_762_912_345.25, None),
            (
                "1762912345.250",
                "1762912399.50",
                1_762_912_345.25,
                Some(1_762_912_399.5),
            ),
            ("1.76291234525e9", "1e9", 1_762_912_345.25, Some(1e9)),
            ("1762912345", "0", 1_762_912_345.0, Some(0.0)),
            (
                "0.30000000000000004441",
                "-2.50",
                0.300_000_000_000_000_04,
                Some(-2.5),
            ),
        ] {
            let text = format!(r#"{{"name":"a","at":{at},"until":{until},"version":3}}"#);
            let want = Versioned {
                row: Row {
                    name: "a".into(),
                    at: want_at,
                    until: want_until,
                },
                version: 3,
            };
            assert_eq!(
                serde_json::from_str::<Versioned>(&text).unwrap(),
                want,
                "{text}"
            );
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                serde_json::from_value::<Versioned>(value).unwrap(),
                want,
                "{text}"
            );
        }
    }

    #[test]
    fn a_record_without_its_own_field_is_refused_naming_it() {
        let error = serde_json::from_str::<Versioned>(r#"{"name":"a","at":1.0,"until":null}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing field `version`"), "{error}");
    }

    /// What serde's `flatten` hands the row: the fields it names, by name or alias, and none of the
    /// others the record holds, which the row refuses read alone. (Whole numbers here: `flatten`
    /// itself refuses a float read from text as the board links `serde_json`.)
    #[test]
    fn the_rest_is_handed_the_fields_it_names_as_flatten_hands_them() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Flattened {
            #[serde(flatten)]
            row: Row,
            version: i64,
        }
        let text = r#"{"label":"a","at":2,"until":null,"version":3,"stall":null,"extra":{"x":1}}"#;
        let flattened = serde_json::from_str::<Flattened>(text).unwrap();
        let beside = serde_json::from_str::<Versioned>(text).unwrap();
        assert_eq!(beside.row, flattened.row);
        assert_eq!(
            (beside.version, beside.row.name.as_str()),
            (flattened.version, "a")
        );
        let alone = serde_json::from_str::<Row>(r#"{"name":"a","at":2,"until":null,"stall":null}"#)
            .unwrap_err()
            .to_string();
        assert!(alone.contains("unknown field `stall`"), "{alone}");
    }

    #[test]
    fn a_field_of_the_wrong_kind_is_refused_whichever_part_it_is_in() {
        let kind = serde_json::from_str::<Versioned>(
            r#"{"name":"a","at":1.0,"until":null,"version":"3"}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(kind.contains("expected i64"), "{kind}");
        let row = serde_json::from_str::<Versioned>(r#"{"name":"a","at":"1.0","version":3}"#)
            .unwrap_err()
            .to_string();
        assert!(row.contains("expected f64"), "{row}");
        assert!(serde_json::from_str::<Versioned>("[1]").is_err());
    }

    /// A rest that is a map, as a map flattened is, is handed every field left.
    #[test]
    fn a_map_read_as_the_rest_holds_every_field_not_taken() {
        let mut record =
            Beside::read(serde_json::json!({"handle": "h", "a": 1, "b": [true]})).unwrap();
        assert_eq!(
            record.take::<String, serde_json::Error>("handle").unwrap(),
            "h"
        );
        let rest: std::collections::BTreeMap<String, Value> =
            record.rest::<_, serde_json::Error>().unwrap();
        assert_eq!(
            rest,
            std::collections::BTreeMap::from([
                ("a".to_string(), serde_json::json!(1)),
                ("b".to_string(), serde_json::json!([true])),
            ])
        );
    }

    /// A field is taken once, and the rest reads what is left.
    #[test]
    fn a_taken_field_is_the_records_and_the_rest_reads_without_it() {
        #[derive(Debug, PartialEq, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Label {
            name: String,
        }
        let mut record =
            Beside::read(serde_json::json!({"name": "row", "rank": 2, "score": 1.50})).unwrap();
        assert_eq!(record.take::<u8, serde_json::Error>("rank").unwrap(), 2);
        assert_eq!(
            record
                .take::<f64, serde_json::Error>("score")
                .unwrap()
                .to_bits(),
            1.5_f64.to_bits()
        );
        assert!(
            record.take::<f64, serde_json::Error>("score").is_err(),
            "taken once"
        );
        assert_eq!(
            record.rest::<Label, serde_json::Error>().unwrap(),
            Label { name: "row".into() }
        );
    }
}
