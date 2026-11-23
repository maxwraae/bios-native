//! The few pieces of Python's behaviour the original mapper's results depend on: how a float is
//! printed (`repr` and `format(x, 'g')`), how `json.dumps` writes text, integer floor division
//! and modulo, and what `str.split()` counts as white space.

use std::fmt::Write as _;

/// Python's `a // b` for a positive or negative `a` and a nonzero `b`.
pub fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) { q - 1 } else { q }
}

/// Python's `a % b`: the result takes the sign of `b`.
pub fn py_mod(a: i64, b: i64) -> i64 {
    let r = a % b;
    if r != 0 && ((r < 0) != (b < 0)) { r + b } else { r }
}

/// What Python's `str.isspace` (and so `str.split()` and the regex `\s`) treats as white space.
pub fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python's `s[start:end]` for a slice, with negative indices counted from the end and both ends
/// clamped.
pub fn slice<T>(s: &[T], start: i64, end: i64) -> &[T] {
    let n = s.len() as i64;
    let norm = |i: i64| -> usize {
        let j = if i < 0 { i + n } else { i };
        j.clamp(0, n) as usize
    };
    let (a, b) = (norm(start), norm(end));
    if a >= b { &s[0..0] } else { s.get(a..b).unwrap_or(&s[0..0]) }
}

/// Python's `s[i]`: negative counts from the end; `None` when out of range.
pub fn index<T: Copy>(s: &[T], i: i64) -> Option<T> {
    let n = s.len() as i64;
    let j = if i < 0 { i + n } else { i };
    if j < 0 || j >= n { None } else { s.get(j as usize).copied() }
}

/// The shortest decimal digits of a finite, nonzero `value` and the position of the decimal point:
/// `value == 0.d1d2d3... * 10^decpt`.
fn shortest_digits(value: f64) -> (String, i64) {
    let text = format!("{:e}", value.abs());
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    (digits, exponent + 1)
}

/// `repr(value)` for a Python float.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, decpt) = shortest_digits(value);
    let n = digits.len() as i64;
    let body = if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
        } else if decpt >= n {
            format!("{}{}.0", digits, "0".repeat((decpt - n) as usize))
        } else {
            let (a, b) = digits.split_at(decpt as usize);
            format!("{a}.{b}")
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let exponent = decpt - 1;
        let mantissa = if rest.is_empty() { first.to_string() } else { format!("{first}.{rest}") };
        format!("{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    };
    format!("{sign}{body}")
}

/// `format(value, 'g')` (six significant digits) for a Python float.
pub fn float_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let precision = 6i64;
    let text = format!("{:.*e}", (precision - 1) as usize, value);
    let (_, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let strip = |s: String| -> String { if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s } };
    if -4 <= exponent && exponent < precision {
        strip(format!("{:.*}", (precision - 1 - exponent) as usize, value))
    } else {
        let (mantissa, _) = text.split_once('e').unwrap_or((&text, ""));
        let mantissa = strip(mantissa.to_string());
        format!("{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

/// Appends `text` as `json.dumps` writes a string (with its default `ensure_ascii=True`).
pub fn json_string(out: &mut String, text: &str) {
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
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

/// A value written the way Python's `json.dumps` (default separators) writes it.
pub enum Json<'a> {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(&'a str),
    List(Vec<Json<'a>>),
    Object(Vec<(&'a str, Json<'a>)>),
}

impl Json<'_> {
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Int(v) => {
                let _ = write!(out, "{v}");
            }
            Json::Float(v) => out.push_str(&match float_repr(*v).as_str() {
                "nan" => "NaN".to_string(),
                "inf" => "Infinity".to_string(),
                "-inf" => "-Infinity".to_string(),
                other => other.to_string(),
            }),
            Json::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            Json::Str(s) => json_string(out, s),
            Json::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(items) => {
                out.push('{');
                for (i, (key, item)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    json_string(out, key);
                    out.push_str(": ");
                    item.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// Python's `str(value)` for a float, as the original sends a number to its Java kernel, parsed
/// back by Java's `Float.parseFloat`.
pub fn python_float_to_java(value: f64) -> f32 {
    // Rust's `{}` prints the shortest digits that read back as the same double, as Python's
    // `str` does; the notation differs but the number is the same. `parse` rounds correctly.
    format!("{value}").parse::<f32>().unwrap_or(f32::NAN)
}

/// Java's `Float.toString(value)`, as the kernel prints a number, read back by Python's `float`.
pub fn java_float_to_python(value: f32) -> f64 {
    // Java (since 19) and Rust both print the shortest digits that read back as the same float.
    format!("{value}").parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (0.5, "0.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5, "-2.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (123456789.125, "123456789.125"),
            (1.5e-7, "1.5e-07"),
            (0.0, "0.0"),
            (0.6, "0.6"),
            (1e22, "1e+22"),
            (12345678901234567.0, "1.2345678901234568e+16"),
        ];
        for (value, expected) in cases {
            assert_eq!(float_repr(value), expected, "{value}");
        }
    }

    #[test]
    fn g_matches_python() {
        let cases = [
            (12.0, "12"),
            (0.5, "0.5"),
            (1234567.0, "1.23457e+06"),
            (100000.0, "100000"),
            (1000000.0, "1e+06"),
            (0.0001, "0.0001"),
            (0.00001234, "1.234e-05"),
            (2.0 / 3.0, "0.666667"),
            (0.0, "0"),
            (33.333333333, "33.3333"),
        ];
        for (value, expected) in cases {
            assert_eq!(float_g(value), expected, "{value}");
        }
    }

    #[test]
    fn json_escapes_like_python() {
        let mut out = String::new();
        json_string(&mut out, "a\"b\\c\u{7f}é😀\t");
        assert_eq!(out, ["\"a\\\"b\\\\c\\u007f", "\\u00e9", "\\ud83d\\ude00", "\\t\""].concat());
    }

    #[test]
    fn integer_division_like_python() {
        assert_eq!(floor_div(-7, 2), -4);
        assert_eq!(floor_div(7, 2), 3);
        assert_eq!(py_mod(-7, 5), 3);
        assert_eq!(py_mod(7, 5), 2);
    }
}
