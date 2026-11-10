//! JSON as Python's `json.dumps(value, indent=1)` writes it, for the `--json` the CLI prints: keys in
//! the order given, a float as Python's `repr`, and anything past ASCII escaped (`ensure_ascii`); and a
//! string as Python's `repr` writes it.

use std::fmt::Write as _;

/// A JSON value whose objects keep their keys in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum Py {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Obj(Vec<(String, Py)>),
}

impl Py {
    pub fn obj(fields: Vec<(&str, Py)>) -> Py {
        Py::Obj(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub fn str(text: &str) -> Py {
        Py::Str(text.to_string())
    }

    pub fn opt_str(text: Option<&str>) -> Py {
        text.map_or(Py::Null, Py::str)
    }

    pub fn opt_float(value: Option<f64>) -> Py {
        value.map_or(Py::Null, Py::Float)
    }

    pub fn strs<S: AsRef<str>>(items: &[S]) -> Py {
        Py::List(items.iter().map(|item| Py::str(item.as_ref())).collect())
    }
}

/// `json.dumps(value, indent=1)`.
pub fn dumps(value: &Py) -> String {
    let mut out = String::new();
    write(value, Some(0), &mut out);
    out
}

/// `json.dumps(value)`: one line, `, ` between items and `: ` after a key.
pub fn dumps_line(value: &Py) -> String {
    let mut out = String::new();
    write(value, None, &mut out);
    out
}

/// `value` at `depth` of an indented layout, or on one line when `depth` is None.
fn write(value: &Py, depth: Option<usize>, out: &mut String) {
    let inner = depth.map(|d| d + 1);
    // Before an item (after its comma, if not the first), and before the closing bracket.
    let line = |out: &mut String, depth: Option<usize>, first: bool| match depth {
        Some(depth) => {
            out.push('\n');
            out.push_str(&" ".repeat(depth));
        }
        None if !first => out.push(' '),
        None => {}
    };
    match value {
        Py::Null => out.push_str("null"),
        Py::Bool(yes) => out.push_str(if *yes { "true" } else { "false" }),
        Py::Int(n) => out.push_str(&n.to_string()),
        Py::UInt(n) => out.push_str(&n.to_string()),
        Py::Float(x) => out.push_str(&float(*x)),
        Py::Str(text) => string(text, out),
        Py::List(items) if items.is_empty() => out.push_str("[]"),
        Py::Obj(fields) if fields.is_empty() => out.push_str("{}"),
        Py::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                line(out, inner, i == 0);
                write(item, inner, out);
            }
            line(out, depth, true);
            out.push(']');
        }
        Py::Obj(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                line(out, inner, i == 0);
                string(key, out);
                out.push_str(": ");
                write(item, inner, out);
            }
            line(out, depth, true);
            out.push('}');
        }
    }
}

/// A string as `ensure_ascii` writes it: printable ASCII as it is, the rest escaped.
fn string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Python's `repr` of a float, as `json.dumps` writes it: the shortest digits that read back as the
/// same float, fixed between 1e-4 and 1e16 (with `.0` when whole), else with an exponent of two
/// digits or more.
pub fn float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let shortest = format!("{value:e}");
    let (mantissa, exponent) = shortest.split_once('e').expect("{:e} has an exponent");
    let exponent: i32 = exponent.parse().expect("{:e}'s exponent is a number");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exponent) {
        let point = usize::try_from(exponent + 1).unwrap_or(0);
        if exponent < 0 {
            let zeros = "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0));
            return format!("{sign}0.{zeros}{digits}");
        }
        if digits.len() <= point {
            return format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()));
        }
        return format!("{sign}{}.{}", &digits[..point], &digits[point..]);
    }
    let fraction = if digits.len() > 1 {
        format!(".{}", &digits[1..])
    } else {
        String::new()
    };
    let direction = if exponent < 0 { '-' } else { '+' };
    format!(
        "{sign}{}{fraction}e{direction}{:02}",
        &digits[..1],
        exponent.abs()
    )
}

/// `text` as Python's `repr` writes a string, which the store's refusals quote names with.
pub fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c < ' ' || c == '\x7f' => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_float_is_written_as_pythons_repr() {
        for (value, said) in [
            (1791362047.125689, "1791362047.125689"),
            (1.0, "1.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1e16, "1e+16"),
            (1.5e-05, "1.5e-05"),
            (0.0001, "0.0001"),
            (123.456, "123.456"),
            (9999999999999998.0, "9999999999999998.0"),
            (12345678901234567890.0, "1.2345678901234567e+19"),
            (0.1, "0.1"),
            (2.5e-7, "2.5e-07"),
            (1e22, "1e+22"),
            (-42.5, "-42.5"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(float(value), said, "{value:e}");
        }
    }

    #[test]
    fn a_value_is_laid_out_as_indent_one_with_ascii_only() {
        let value = Py::obj(vec![
            (
                "a",
                Py::List(vec![
                    Py::Int(1),
                    Py::obj(vec![("b", Py::Null), ("c", Py::List(vec![]))]),
                    Py::Obj(vec![]),
                ]),
            ),
            ("d", Py::str("x\u{2014}y\u{7f}\n\"\\\t\u{1}\u{1F600}")),
            ("e", Py::Bool(true)),
        ]);
        assert_eq!(
            dumps(&value),
            r#"{
 "a": [
  1,
  {
   "b": null,
   "c": []
  },
  {}
 ],
 "d": "x\u2014y\u007f\n\"\\\t\u0001\ud83d\ude00",
 "e": true
}"#
        );
    }

    #[test]
    fn a_value_on_one_line_is_laid_out_as_json_dumps_does_by_default() {
        let value = Py::obj(vec![
            (
                "a",
                Py::List(vec![
                    Py::Int(1),
                    Py::obj(vec![("b", Py::Null), ("c", Py::List(vec![]))]),
                    Py::Obj(vec![]),
                ]),
            ),
            ("d", Py::str("x\u{2014}\n")),
        ]);
        assert_eq!(
            dumps_line(&value),
            r#"{"a": [1, {"b": null, "c": []}, {}], "d": "x\u2014\n"}"#
        );
        assert_eq!(dumps_line(&Py::Int(3)), "3");
    }

    #[test]
    fn keys_keep_the_order_given() {
        let value = Py::obj(vec![("z", Py::Int(1)), ("a", Py::Float(2.0))]);
        assert_eq!(dumps(&value), "{\n \"z\": 1,\n \"a\": 2.0\n}");
    }

    #[test]
    fn a_scalar_stands_alone() {
        assert_eq!(dumps(&Py::str("é")), "\"\\u00e9\"");
        assert_eq!(dumps(&Py::Bool(false)), "false");
        assert_eq!(dumps(&Py::Int(-3)), "-3");
        assert_eq!(dumps(&Py::UInt(u64::MAX)), "18446744073709551615");
    }
}
