//! A hook's event: the JSON Claude Code writes on stdin, read as Python's `json` reads it.

use serde_json::Value;

/// The event, or None when it is not JSON (every hook then lets the call through). Python's reader also
/// takes a lone surrogate escape (`"\udc80"`), which `serde_json` refuses; one stands in as U+FFFD, a
/// character that reads the same to every guard.
pub fn parse(raw: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(raw).ok()?;
    match serde_json::from_slice(text.as_bytes()) {
        Ok(value) => Some(value),
        Err(_) => serde_json::from_slice(replace_lone_surrogates(text)?.as_bytes()).ok(),
    }
}

/// `text` with each `\uD800`–`\uDFFF` escape that is not half of a pair replaced by `�`; None when
/// there is none, so the text's failure stands.
fn replace_lone_surrogates(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let hex = |at: usize| -> Option<u32> {
        let digits = text.get(at..at + 4)?;
        u32::from_str_radix(digits, 16).ok()
    };
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        if bytes[i + 1] != b'u' {
            i += 2; // any other escape, `\\` included, is two bytes
            continue;
        }
        let Some(unit) = hex(i + 2) else {
            i += 2;
            continue;
        };
        let high = (0xD800..0xDC00).contains(&unit);
        let low = (0xDC00..0xE000).contains(&unit);
        let paired = high
            && bytes.get(i + 6) == Some(&b'\\')
            && bytes.get(i + 7) == Some(&b'u')
            && hex(i + 8).is_some_and(|u| (0xDC00..0xE000).contains(&u));
        if paired {
            i += 12;
        } else if high || low {
            out.push_str(text.get(last..i).unwrap_or_default());
            out.push_str("\\uFFFD");
            i += 6;
            last = i;
            changed = true;
        } else {
            i += 6;
        }
    }
    changed.then(|| {
        out.push_str(text.get(last..).unwrap_or_default());
        out
    })
}

/// Python's truth of a JSON value: null, false, 0, "", [] and {} are false.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python's `str()` of a JSON value: a string is itself, anything else its Python spelling (near enough
/// that every guard reads it the same).
pub fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let quote = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut out = String::from(quote);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == quote => {
                        out.push('\\');
                        out.push(c);
                    }
                    c => out.push(c),
                }
            }
            out.push(quote);
            out
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&Value::from(k.as_str())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_surrogate_reads_as_a_replacement_character() {
        let event = parse(r#"{"tool_input": {"command": "pkill x \udc80 😀 \\udc80"}}"#.as_bytes())
            .unwrap();
        assert_eq!(
            event["tool_input"]["command"],
            "pkill x \u{fffd} 😀 \\udc80"
        );
        assert!(parse(b"not json").is_none());
        assert!(parse(b"\xff").is_none());
    }

    #[test]
    fn truth_and_str_are_pythons() {
        for falsy in ["null", "false", "0", "0.0", "\"\"", "[]", "{}"] {
            assert!(!truthy(&serde_json::from_str(falsy).unwrap()), "{falsy}");
        }
        let value: Value = serde_json::from_str(r#"["a", 1, null, true, {"k": "it's"}]"#).unwrap();
        assert_eq!(py_str(&value), r#"['a', 1, None, True, {'k': "it's"}]"#);
    }

    #[test]
    fn surrogate_recovery_preserves_pairs_and_valid_escapes_and_rejects_bad_hex() {
        let event = parse(br#"{"text":"\ud83d\ude00 \u0061 \udc80"}"#).unwrap();
        assert_eq!(event["text"], "😀 a �");
        assert!(parse(br#"{"text":"\uZZZZ \udc80"}"#).is_none());
        let value = serde_json::json!([false, "a\\b\n\r\t", "both'\""]);
        assert_eq!(py_str(&value), r#"[False, 'a\\b\n\r\t', 'both\'"']"#);
    }
}
