//! The Python semantics decider's output depends on: `json.dumps` text, float `repr`, `str`/`repr` in messages,
//! `round`, the built-in `sum` over floats, truthiness and `float(str)`.
//!
//! JSON values arrive through `serde_json` (with `preserve_order`), so objects keep insertion order like Python dicts.
//! Integers outside the `i64`/`u64` range are parsed as floats by `serde_json`, where Python keeps them exact; that is
//! the one known difference.

use serde_json::{Number, Value};

/// `json.dumps(value, ensure_ascii=False, sort_keys=sort_keys)` with the default `", "` and `": "` separators.
pub fn dumps(value: &Value, sort_keys: bool) -> String {
    let mut out = String::new();
    write_json(&mut out, value, sort_keys);
    out
}

fn write_json(out: &mut String, value: &Value, sort_keys: bool) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => out.push_str(&json_string(s)),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_json(out, item, sort_keys);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if sort_keys {
                // Python compares str by code point, which is UTF-8 byte order.
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&json_string(key));
                out.push_str(": ");
                write_json(out, item, sort_keys);
            }
            out.push('}');
        }
    }
}

/// A JSON string literal escaped like `json.dumps(ensure_ascii=False)`: `"`, `\`, `\b`, `\f`, `\n`, `\r`, `\t` and
/// `\u00XX` for other control characters, which is also what `serde_json` writes.
fn json_string(s: &str) -> String {
    serde_json::to_string(s).expect("serialising a str cannot fail")
}

/// An integer as its digits, a float as Python's `repr`.
fn number(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        float_repr(n.as_f64().unwrap_or(f64::NAN))
    }
}

/// Python's `repr(float)`: the shortest round-trip digits, positional for exponents -4..16, else `1e+16` style.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("`{:e}` always writes an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` writes a decimal exponent");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let decpt = exp + 1;
    let body = if decpt <= -4 || decpt > 16 {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{first}{frac}e{esign}{:02}", exp.abs())
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt >= n {
        format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
    } else {
        let (int, frac) = digits.split_at(decpt as usize);
        format!("{int}.{frac}")
    };
    format!("{sign}{body}")
}

/// Python's `str()` of a JSON value: a string as itself, anything else as its `repr`.
pub fn str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// Python's `repr()` of the value `json.loads` returns: `None`, `True`, `'text'`, `[1, 2]`, `{'a': 1}`.
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => number(n),
        Value::String(s) => str_repr(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python's `repr(str)`: single quotes unless the text holds `'` and no `"`, non-printable characters escaped.
pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if !printable(c) => {
                let code = c as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `str.isprintable` for one character: control, format, separator (other than space), private-use and surrogate
/// characters are not printable. Unassigned code points count as printable here.
fn printable(c: char) -> bool {
    let code = c as u32;
    !matches!(code,
        0x00..=0x1f | 0x7f..=0xa0 | 0xad
        | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x1680 | 0x180e
        | 0x2000..=0x200f | 0x2028..=0x202f | 0x205f..=0x2064 | 0x2066..=0x206f
        | 0x3000 | 0xe000..=0xf8ff | 0xfeff | 0xfff9..=0xfffb
        | 0xf0000..=0x10ffff)
}

/// Python's `round(x, ndigits)` for floats: the exact binary value rounded half-to-even at `ndigits` decimals, read
/// back as the nearest float. Rust's fixed-precision formatting rounds the exact value the same way.
pub fn round(x: f64, ndigits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.ndigits$}")
        .parse()
        .expect("a formatted finite float parses back")
}

/// The built-in `sum()` over floats as CPython 3.12+ computes it: Neumaier-compensated. An empty sum is 0.
pub fn sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut total = 0.0_f64;
    let mut c = 0.0_f64;
    for x in values {
        let t = total + x;
        if total.abs() >= x.abs() {
            c += (total - t) + x;
        } else {
            c += (x - t) + total;
        }
        total = t;
    }
    if c != 0.0 && c.is_finite() {
        total += c;
    }
    total
}

/// Python truthiness of a JSON value: `null`, `false`, `0`, `""`, `[]` and `{}` are false.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `str.isspace` for one character (what `\s` and `str.strip` use).
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python's `float(text)`: surrounding whitespace ignored, single underscores between digits allowed, `inf`,
/// `infinity` and `nan` in any case. `None` where Python raises `ValueError`.
pub fn float_from_str(text: &str) -> Option<f64> {
    let text = text.trim_matches(is_space);
    let bytes: Vec<char> = text.chars().collect();
    let mut cleaned = String::with_capacity(text.len());
    for (i, &c) in bytes.iter().enumerate() {
        if c == '_' {
            let digit_before = i > 0 && bytes[i - 1].is_ascii_digit();
            let digit_after = bytes.get(i + 1).is_some_and(char::is_ascii_digit);
            if !(digit_before && digit_after) {
                return None;
            }
            continue;
        }
        cleaned.push(c);
    }
    let lower = cleaned.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if lower.len() - unsigned.len() > 1 {
        return None;
    }
    if matches!(unsigned, "inf" | "infinity" | "nan") {
        return lower.parse().ok();
    }
    if unsigned.is_empty()
        || !unsigned
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    cleaned.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (123.456, "123.456"),
            (-2.5e300, "-2.5e+300"),
            (1.2345678901234567, "1.2345678901234567"),
        ];
        for (x, want) in cases {
            assert_eq!(float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn dumps_matches_python() {
        let v = json!({"b": [1, 2.0, "é\n\"x\""], "a": {"t": true, "n": null}, "c": []});
        assert_eq!(
            dumps(&v, false),
            r#"{"b": [1, 2.0, "é\n\"x\""], "a": {"t": true, "n": null}, "c": []}"#
        );
        assert_eq!(
            dumps(&v, true),
            r#"{"a": {"n": null, "t": true}, "b": [1, 2.0, "é\n\"x\""], "c": []}"#
        );
    }

    #[test]
    fn repr_and_str_match_python() {
        assert_eq!(repr(&json!("x")), "'x'");
        assert_eq!(repr(&json!("it's")), "\"it's\"");
        assert_eq!(
            repr(&json!({"a": [1, null, true, 1.5]})),
            "{'a': [1, None, True, 1.5]}"
        );
        assert_eq!(str(&json!("x")), "x");
        assert_eq!(str(&json!(false)), "False");
    }

    #[test]
    fn round_is_half_even_on_the_exact_value() {
        assert_eq!(round(0.125, 2), 0.12);
        assert_eq!(round(0.375, 2), 0.38);
        assert_eq!(round(2.5, 0), 2.0);
        assert_eq!(round(0.00015, 4), 0.0001);
        assert_eq!(round(0.12345, 4), 0.1235);
        assert_eq!(round(1.00005, 4), 1.0001);
    }

    #[test]
    fn sum_is_compensated() {
        assert_eq!(sum([0.1; 10]), 1.0);
        assert_eq!(sum([1e100, 1.0, -1e100]), 1.0);
        assert_eq!(sum([]), 0.0);
    }

    #[test]
    fn float_from_str_matches_python() {
        assert_eq!(float_from_str(" 1.3 "), Some(1.3));
        assert_eq!(float_from_str("1_000"), Some(1000.0));
        assert_eq!(float_from_str("1__0"), None);
        assert_eq!(float_from_str("-inf"), Some(f64::NEG_INFINITY));
        assert!(float_from_str("NaN").is_some_and(f64::is_nan));
        assert_eq!(float_from_str("1e3"), Some(1000.0));
        assert_eq!(float_from_str("abc"), None);
        assert_eq!(float_from_str(""), None);
    }
}
