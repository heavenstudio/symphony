//! Preserve one meaning for every JSON target: duplicate keys are refused at any depth.

use futures_util::StreamExt;
use serde::de::{Deserialize, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Refusal;

struct Unique;

pub(super) async fn collect(
    body: axum::body::Body,
    digest: Option<[u8; 32]>,
) -> Result<Vec<u8>, Refusal> {
    const MAX: usize = 2 * 1024 * 1024;
    let mut bytes = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| Refusal::Invalid)?;
        if chunk.len() > MAX - bytes.len() {
            return Err(Refusal::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    if digest.is_some_and(|expected| <[u8; 32]>::from(Sha256::digest(&bytes)) != expected) {
        return Err(Refusal::Denied);
    }
    Ok(bytes)
}

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor).map(|()| Unique)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("JSON without duplicate keys")
    }

    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<(), A::Error> {
        while sequence.next_element::<Unique>()?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = std::collections::BTreeSet::new();
        while let Some((key, Unique)) = map.next_entry::<String, Unique>()? {
            if !keys.insert(key) {
                return Err(A::Error::custom("duplicate key"));
            }
        }
        Ok(())
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<Value, Refusal> {
    // serde_json's workspace features may represent numbers through a private visitor map.
    // Validate uniqueness without building values, then let its own Value decoder preserve
    // exact numeric semantics; no private serialization marker becomes a payload object.
    serde_json::from_slice::<Unique>(bytes).map_err(|_| Refusal::Invalid)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Refusal::Invalid)?;
    finite(&value)?;
    Ok(value)
}

fn finite(value: &Value) -> Result<(), Refusal> {
    match value {
        Value::Number(number) if number.as_f64().is_none_or(|n| !n.is_finite()) => {
            Err(Refusal::Invalid)
        }
        Value::Object(values) => values.values().try_for_each(finite),
        Value::Array(values) => values.iter().try_for_each(finite),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_json_values_keep_their_types() {
        let bytes = br#"{"s":"hello","escaped":"a\nb","b":true,"n":null,"negative":-1,"u":18446744073709551615,"float":1.5,"array":[false,{"x":2}]}"#;
        assert_eq!(
            parse(bytes).unwrap(),
            serde_json::from_slice::<Value>(bytes).unwrap()
        );
    }

    #[test]
    fn duplicate_escaped_nested_and_trailing_values_are_refused() {
        for bytes in [
            br#"{"project":"other","project":"fixture"}"#.as_slice(),
            br#"{"p":1,"\u0070":2}"#,
            br#"{"a":[{"path":"one","path":"two"}]}"#,
            b"{} {}",
            b"{",
            b"1e9999",
        ] {
            assert_eq!(parse(bytes), Err(Refusal::Invalid));
        }
    }
}
