//! Verifying what came back from the sequencer: the folder a run lives in, the matcher that places
//! samples on maps, the mapper's result read as differences and a verdict, and the person's call
//! kept apart from the finding. Pure: nothing here reads a file. Ported from the web app's
//! `verify/` and `packages/shared/src/folders.ts`.

pub mod folder;
pub mod mapper;
pub mod names;
pub mod runs;

use std::cmp::Ordering;

/// `a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" })`, closely enough for
/// file names: case is ignored and runs of digits compare as numbers, so `S9` comes before `S10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let mut da = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    da.push(c);
                    x.next();
                }
                let mut db = String::new();
                while let Some(c) = y.peek().copied().filter(char::is_ascii_digit) {
                    db.push(c);
                    y.next();
                }
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let by = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if by != Ordering::Equal {
                    return by;
                }
            }
            (Some(p), Some(q)) => {
                let by = p.to_lowercase().cmp(q.to_lowercase());
                if by != Ordering::Equal {
                    return by;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// A whole number with thousands grouped by commas, as `Intl.NumberFormat("en")` writes it.
pub fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_inside_names_compare_as_numbers() {
        let mut v = vec!["A10_S10", "A8_S8", "a7_S7", "A9_S9"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a7_S7", "A8_S8", "A9_S9", "A10_S10"]);
    }

    #[test]
    fn grouping_matches_the_web() {
        assert_eq!(grouped(2401), "2,401");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1234567), "1,234,567");
    }
}
