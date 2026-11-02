//! CBOR (RFC 8949), as much of it as SurrealDB's RPC speaks, to and from JSON values.
//!
//! The RPC's JSON format is lossy on the way in: a string variable that starts like a record id
//! (`approve: looks right` reads as the record `approve:looks`), a datetime or a UUID is taken as one.
//! CBOR carries a string as a string. SurrealDB's own types come back tagged: NONE (tag 6) reads as
//! null, and any other tag (a record id is tag 8) as the value it wraps.

use serde_json::Value;

pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write(value, &mut out);
    out
}

/// One CBOR item, the whole of `bytes`.
pub fn decode(bytes: &[u8]) -> Result<Value, String> {
    let mut reader = Reader { bytes, at: 0 };
    let value = reader.item()?;
    if reader.at != bytes.len() {
        return Err(format!("{} bytes after the item", bytes.len() - reader.at));
    }
    Ok(value)
}

fn write(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i < 0 {
                    head(1, (-1 - i) as u64, out);
                } else {
                    head(0, i as u64, out);
                }
            } else if let Some(u) = n.as_u64() {
                head(0, u, out);
            } else {
                out.push(0xfb);
                out.extend(n.as_f64().unwrap_or(f64::NAN).to_be_bytes());
            }
        }
        Value::String(text) => {
            head(3, text.len() as u64, out);
            out.extend(text.as_bytes());
        }
        Value::Array(items) => {
            head(4, items.len() as u64, out);
            for item in items {
                write(item, out);
            }
        }
        Value::Object(fields) => {
            head(5, fields.len() as u64, out);
            for (key, item) in fields {
                head(3, key.len() as u64, out);
                out.extend(key.as_bytes());
                write(item, out);
            }
        }
    }
}

/// An item's first byte (its major type) and the number it carries, in the fewest bytes.
fn head(major: u8, n: u64, out: &mut Vec<u8>) {
    let major = major << 5;
    if n < 24 {
        out.push(major | n as u8);
    } else if let Ok(n) = u8::try_from(n) {
        out.extend([major | 24, n]);
    } else if let Ok(n) = u16::try_from(n) {
        out.push(major | 25);
        out.extend(n.to_be_bytes());
    } else if let Ok(n) = u32::try_from(n) {
        out.push(major | 26);
        out.extend(n.to_be_bytes());
    } else {
        out.push(major | 27);
        out.extend(n.to_be_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len());
        let end =
            end.ok_or_else(|| format!("cut off at byte {} of an item needing {n} more", self.at))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn number(&mut self, size: usize) -> Result<u64, String> {
        Ok(self
            .take(size)?
            .iter()
            .fold(0, |n, &b| n << 8 | u64::from(b)))
    }

    fn item(&mut self) -> Result<Value, String> {
        let first = self.take(1)?[0];
        let (major, info) = (first >> 5, first & 0x1f);
        if major == 7 {
            return self.simple(info);
        }
        let n = match info {
            0..=23 => u64::from(info),
            24 => self.number(1)?,
            25 => self.number(2)?,
            26 => self.number(4)?,
            27 => self.number(8)?,
            31 => return Err("indefinite-length items are not read".into()),
            _ => return Err(format!("a reserved head {first:#04x}")),
        };
        let length = || usize::try_from(n).map_err(|_| format!("a length of {n}"));
        Ok(match major {
            0 => Value::from(n),
            1 => match i64::try_from(n) {
                Ok(n) => Value::from(-1 - n),
                Err(_) => serde_json::json!(-1.0 - n as f64),
            },
            2 => Value::from(self.take(length()?)?.to_vec()),
            3 => Value::from(self.text(length()?)?),
            4 => Value::Array(
                (0..length()?)
                    .map(|_| self.item())
                    .collect::<Result<_, _>>()?,
            ),
            5 => {
                let mut fields = serde_json::Map::new();
                for _ in 0..length()? {
                    let Value::String(key) = self.item()? else {
                        return Err("a map key that is not text".into());
                    };
                    fields.insert(key, self.item()?);
                }
                Value::Object(fields)
            }
            _ => match (n, self.item()?) {
                (6, _) => Value::Null, // SurrealDB's NONE
                (_, wrapped) => wrapped,
            },
        })
    }

    fn text(&mut self, length: usize) -> Result<String, String> {
        let at = self.at;
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| format!("a string at byte {at} that is not UTF-8"))
    }

    fn simple(&mut self, info: u8) -> Result<Value, String> {
        let number = |f: f64| serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number);
        Ok(match info {
            20 => Value::Bool(false),
            21 => Value::Bool(true),
            22 | 23 => Value::Null,
            25 => number(half(self.number(2)? as u16)),
            26 => number(f64::from(f32::from_bits(self.number(4)? as u32))),
            27 => number(f64::from_bits(self.number(8)?)),
            _ => return Err(format!("a simple value {info} that is not read")),
        })
    }
}

/// A half-precision float's bits as the number they are.
fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f64::from(bits & 0x3ff);
    sign * match exponent {
        0 => mantissa * 2f64.powi(-24),
        31 if mantissa == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        _ => (1.0 + mantissa / 1024.0) * 2f64.powi(exponent - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// RFC 8949 Appendix A's examples of what this speaks, each both ways.
    const EXAMPLES: &[(&str, &str)] = &[
        ("0", "00"),
        ("23", "17"),
        ("24", "1818"),
        ("100", "1864"),
        ("1000", "1903e8"),
        ("1000000", "1a000f4240"),
        ("1000000000000", "1b000000e8d4a51000"),
        ("18446744073709551615", "1bffffffffffffffff"),
        ("-1", "20"),
        ("-10", "29"),
        ("-100", "3863"),
        ("-1000", "3903e7"),
        ("-9223372036854775808", "3b7fffffffffffffff"),
        ("1.1", "fb3ff199999999999a"),
        ("-4.1", "fbc010666666666666"),
        ("1e300", "fb7e37e43c8800759c"),
        ("false", "f4"),
        ("true", "f5"),
        ("null", "f6"),
        (r#""""#, "60"),
        (r#""a""#, "6161"),
        (r#""IETF""#, "6449455446"),
        (r#""\"\\""#, "62225c"),
        (r#""ü""#, "62c3bc"),
        (r#""水""#, "63e6b0b4"),
        ("[]", "80"),
        ("[1,[2,3],[4,5]]", "8301820203820405"),
        (r#"{"a":1,"b":[2,3]}"#, "a26161016162820203"),
        ("{}", "a0"),
    ];

    #[test]
    fn what_it_writes_is_rfc_8949s() {
        for (value, bytes) in EXAMPLES {
            let value: Value = serde_json::from_str(value).unwrap();
            assert_eq!(encode(&value), hex(bytes), "{value}");
        }
    }

    #[test]
    fn what_it_reads_is_rfc_8949s() {
        for (value, bytes) in EXAMPLES {
            assert_eq!(
                decode(&hex(bytes)),
                Ok(serde_json::from_str::<Value>(value).unwrap()),
                "{bytes}"
            );
        }
    }

    #[test]
    fn a_long_string_and_a_long_array_take_longer_heads() {
        let long = "x".repeat(300);
        let mut bytes = hex("79012c");
        bytes.extend(long.as_bytes());
        assert_eq!(encode(&json!(long)), bytes);
        assert_eq!(decode(&bytes), Ok(json!(long)));
        let many = json!(vec![0; 25]);
        assert_eq!(&encode(&many)[..2], hex("9819"));
        assert_eq!(decode(&encode(&many)), Ok(many));
    }

    #[test]
    fn half_and_single_floats_read_as_the_numbers_they_are() {
        for (bytes, number) in [
            ("f93e00", 1.5),
            ("f9c400", -4.0),
            ("f97bff", 65504.0),
            ("f90001", 5.960464477539063e-8),
            ("f90400", 6.103515625e-5),
            ("f98000", -0.0),
            ("fa47c35000", 100000.0),
            ("fa7f7fffff", 3.4028234663852886e38),
            ("fa4ed693a4", 1_800_000_000.0),
        ] {
            assert_eq!(decode(&hex(bytes)), Ok(json!(number)), "{bytes}");
        }
    }

    #[test]
    fn surrealdbs_none_is_null_and_any_other_tag_is_what_it_wraps() {
        assert_eq!(decode(&hex("c6f6")), Ok(Value::Null));
        assert_eq!(
            decode(&hex("c882647461736b6573696d2f61")),
            Ok(json!(["task", "sim/a"]))
        );
        assert!(decode(&hex("d825")).is_err(), "a tag with nothing after it");
        assert_eq!(decode(&hex("a16161c6f6")), Ok(json!({"a": null})));
    }

    #[test]
    fn undefined_reads_as_null() {
        assert_eq!(decode(&hex("f7")), Ok(Value::Null));
    }

    #[test]
    fn what_it_cannot_read_is_refused_not_guessed() {
        for bytes in [
            "",           // nothing
            "18",         // a head cut off
            "6261",       // a string cut off
            "830102",     // an array cut off
            "a161610102", // a value after the item
            "62c328",     // a string that is not UTF-8
            "5f",         // indefinite length
            "a10102",     // a map key that is not text
            "44010203",   // bytes cut off
            "1c",         // a reserved head
            "f0",         // a simple value SurrealDB never sends
        ] {
            assert!(
                decode(&hex(bytes)).is_err(),
                "{bytes:?} read as {:?}",
                decode(&hex(bytes))
            );
        }
    }

    #[test]
    fn bytes_read_as_their_numbers() {
        assert_eq!(decode(&hex("4401020304")), Ok(json!([1, 2, 3, 4])));
    }

    #[test]
    fn a_reply_of_every_kind_reads_back_as_written() {
        let reply = json!({"id": 1, "result": [{"status": "OK", "result": [{"slug": "sim/a", "since": 1_800_000_000.5,
            "text": "approve: looks right", "n": -3, "ok": true, "none": null, "list": [1.25, "2024-01-01T00:00:00Z"]}]}]});
        assert_eq!(decode(&encode(&reply)), Ok(reply));
    }

    #[test]
    fn a_number_json_cannot_hold_reads_as_near_as_it_can() {
        assert_eq!(
            decode(&hex("3bffffffffffffffff")),
            Ok(json!(-18446744073709551616.0))
        );
        assert_eq!(decode(&hex("f97c00")), Ok(Value::Null), "infinity");
        assert_eq!(decode(&hex("f9fc00")), Ok(Value::Null), "minus infinity");
        assert_eq!(decode(&hex("f97e00")), Ok(Value::Null), "not a number");
    }
}
