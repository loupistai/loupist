//! Python `json.dumps` and `repr` for the values World prompts render.
//!
//! The extraction prompts were written in Python: context lists go through
//! `json.dumps` (or `repr()` where an f-string interpolates a list directly),
//! and response schemas are appended with `json.dumps(..., ensure_ascii=True)`.
//! `Py` keeps dictionary insertion order so the rendered bytes match.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Py {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(String, Py)>),
}

impl Py {
    pub(crate) fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub(crate) fn dict<K: Into<String>>(items: impl IntoIterator<Item = (K, Py)>) -> Self {
        Self::Dict(
            items
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub(crate) fn strings<S: AsRef<str>>(values: impl IntoIterator<Item = S>) -> Self {
        Self::List(
            values
                .into_iter()
                .map(|value| Self::str(value.as_ref()))
                .collect(),
        )
    }

    /// `json.dumps(value, ensure_ascii=...)` with the default `", "` and `": "` separators.
    pub(crate) fn dumps(&self, ensure_ascii: bool) -> String {
        let mut out = String::new();
        self.write_json(&mut out, ensure_ascii);
        out
    }

    fn write_json(&self, out: &mut String, ensure_ascii: bool) {
        match self {
            Self::None => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Float(value) => out.push_str(&float_repr(*value, true)),
            Self::Str(value) => json_string(out, value, ensure_ascii),
            Self::List(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    item.write_json(out, ensure_ascii);
                }
                out.push(']');
            }
            Self::Dict(items) => {
                out.push('{');
                for (index, (key, item)) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    json_string(out, key, ensure_ascii);
                    out.push_str(": ");
                    item.write_json(out, ensure_ascii);
                }
                out.push('}');
            }
        }
    }

    /// Python's `repr()`.
    pub(crate) fn repr(&self) -> String {
        let mut out = String::new();
        self.write_repr(&mut out);
        out
    }

    fn write_repr(&self, out: &mut String) {
        match self {
            Self::None => out.push_str("None"),
            Self::Bool(true) => out.push_str("True"),
            Self::Bool(false) => out.push_str("False"),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Float(value) => out.push_str(&float_repr(*value, false)),
            Self::Str(value) => out.push_str(&repr_str(value)),
            Self::List(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    item.write_repr(out);
                }
                out.push(']');
            }
            Self::Dict(items) => {
                out.push('{');
                for (index, (key, item)) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&repr_str(key));
                    out.push_str(": ");
                    item.write_repr(out);
                }
                out.push('}');
            }
        }
    }

    /// The same value as JSON. serde_json objects are key-sorted, which is how
    /// the worker's sorted JSON-RPC frames delivered schemas to the host.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::None => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Int(value) => Value::from(*value),
            Self::Float(value) => serde_json::Number::from_f64(*value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::Str(value) => Value::String(value.clone()),
            Self::List(items) => Value::Array(items.iter().map(Self::to_value).collect()),
            Self::Dict(items) => Value::Object(
                items
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_value()))
                    .collect(),
            ),
        }
    }

    /// A JSON value as `Py`; object keys keep serde_json's (sorted) order.
    pub(crate) fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::None,
            Value::Bool(value) => Self::Bool(*value),
            Value::Number(number) => number
                .as_i64()
                .map(Self::Int)
                .unwrap_or_else(|| Self::Float(number.as_f64().unwrap_or(0.0))),
            Value::String(value) => Self::Str(value.clone()),
            Value::Array(items) => Self::List(items.iter().map(Self::from_value).collect()),
            Value::Object(items) => Self::Dict(
                items
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from_value(value)))
                    .collect(),
            ),
        }
    }
}

fn json_string(out: &mut String, value: &str, ensure_ascii: bool) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch if ensure_ascii && !(' '..='~').contains(&ch) => {
                let code = ch as u32;
                if code >= 0x10000 {
                    let offset = code - 0x10000;
                    let high = 0xd800 | (offset >> 10);
                    let low = 0xdc00 | (offset & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// CPython's `unicode_repr`.
pub(crate) fn repr_str(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        let code = ch as u32;
        if ch == quote || ch == '\\' {
            out.push('\\');
            out.push(ch);
        } else if ch == '\t' {
            out.push_str("\\t");
        } else if ch == '\n' {
            out.push_str("\\n");
        } else if ch == '\r' {
            out.push_str("\\r");
        } else if code < 0x20 || code == 0x7f {
            out.push_str(&format!("\\x{code:02x}"));
        } else if code < 0x7f || is_printable(ch) {
            out.push(ch);
        } else if code <= 0xff {
            out.push_str(&format!("\\x{code:02x}"));
        } else if code <= 0xffff {
            out.push_str(&format!("\\u{code:04x}"));
        } else {
            out.push_str(&format!("\\U{code:08x}"));
        }
    }
    out.push(quote);
    out
}

/// Python's `str.isprintable()` for one non-ASCII character: false for
/// controls, separators other than space, format characters, private use and
/// noncharacters. Unassigned code points are treated as printable.
fn is_printable(ch: char) -> bool {
    let code = ch as u32;
    let excluded: &[(u32, u32)] = &[
        (0x80, 0xa0),
        (0xad, 0xad),
        (0x600, 0x605),
        (0x61c, 0x61c),
        (0x6dd, 0x6dd),
        (0x70f, 0x70f),
        (0x890, 0x891),
        (0x8e2, 0x8e2),
        (0x1680, 0x1680),
        (0x180e, 0x180e),
        (0x2000, 0x200f),
        (0x2028, 0x202f),
        (0x205f, 0x2064),
        (0x2066, 0x206f),
        (0x3000, 0x3000),
        (0xe000, 0xf8ff),
        (0xfdd0, 0xfdef),
        (0xfeff, 0xfeff),
        (0xfff9, 0xfffb),
        (0x110bd, 0x110bd),
        (0x110cd, 0x110cd),
        (0x13430, 0x1343f),
        (0x1bca0, 0x1bca3),
        (0x1d173, 0x1d17a),
        (0xe0001, 0xe0001),
        (0xe0020, 0xe007f),
        (0xf0000, 0x10ffff),
    ];
    if code & 0xfffe == 0xfffe {
        return false;
    }
    !excluded
        .iter()
        .any(|(low, high)| (*low..=*high).contains(&code))
}

/// Python's float `repr`; JSON uses the same text for finite values.
fn float_repr(value: f64, json: bool) -> String {
    if value.is_nan() {
        return if json { "NaN".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        let text = if value > 0.0 { "inf" } else { "-inf" };
        return if json {
            if value > 0.0 {
                "Infinity".into()
            } else {
                "-Infinity".into()
            }
        } else {
            text.into()
        };
    }
    let magnitude = value.abs();
    if value == 0.0 || (1e-4..1e16).contains(&magnitude) {
        let text = format!("{value:?}");
        return text;
    }
    // Rust: 1e16 / 1e-5; Python: 1e+16 / 1e-05.
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ('-', digits),
        None => ('+', exponent),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_and_repr_follow_python_quoting_and_escapes() {
        let value = Py::dict([
            ("idx", Py::Int(0)),
            ("fact", Py::str("Ada's \"engine\"\n 삼성\u{a0}x\u{200b}")),
        ]);
        assert_eq!(
            value.dumps(false),
            "{\"idx\": 0, \"fact\": \"Ada's \\\"engine\\\"\\n 삼성\u{a0}x\u{200b}\"}"
        );
        assert_eq!(
            value.dumps(true),
            "{\"idx\": 0, \"fact\": \"Ada's \\\"engine\\\"\\n \\uc0bc\\uc131\\u00a0x\\u200b\"}"
        );
        assert_eq!(
            value.repr(),
            "{'idx': 0, 'fact': 'Ada\\'s \"engine\"\\n 삼성\\xa0x\\u200b'}"
        );
        assert_eq!(Py::str("it's").repr(), "\"it's\"");
        assert_eq!(Py::str("\u{1f600}").dumps(true), "\"\\ud83d\\ude00\"");
        assert_eq!(Py::str("\u{7f}").dumps(true), "\"\\u007f\"");
        assert_eq!(Py::str("\u{7f}").dumps(false), "\"\u{7f}\"");
        assert_eq!(Py::Float(1e16).repr(), "1e+16");
        assert_eq!(Py::Float(0.5).repr(), "0.5");
        assert_eq!(Py::Float(1.0).repr(), "1.0");
    }
}
