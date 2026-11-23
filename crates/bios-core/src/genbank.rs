//! GenBank flat-file reader.
//!
//! Behaviour follows what Bios reads today: a feature is named from the first of `label`, `gene`,
//! `ApEinfo_label`, `name`, `product`, `region_name`, `organism`, `locus_tag`, `note`; the `label`
//! qualifier becomes the name and is not kept as a note; a quoted value that runs over several lines
//! is joined without a space; `""` inside a quoted value is one quote.

use crate::locations::normalize_location;
use crate::model::{Construct, Feature, Fuzzy, LocationOperator, Locus, Segment, Strand};

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The text has no `LOCUS` line, so it is not a GenBank record.
    NotGenBank,
    /// The record has no sequence.
    NoSequence,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::NotGenBank => write!(f, "this is not a GenBank file: it has no LOCUS line"),
            ReadError::NoSequence => write!(f, "this GenBank file has no sequence"),
        }
    }
}

impl std::error::Error for ReadError {}

const NAME_KEYS: [&str; 9] = ["label", "gene", "ApEinfo_label", "name", "product", "region_name", "organism", "locus_tag", "note"];
const MAX_NAME: usize = 100;

struct RawFeature {
    kind: String,
    location: String,
    /// One entry per qualifier line, in file order, values still quoted.
    notes: Vec<(String, String)>,
}

#[derive(PartialEq)]
enum Section {
    Header,
    Comment,
    Features,
    Origin,
}

/// Reads the first record in `text`.
pub fn read(text: &str) -> Result<Construct, ReadError> {
    let mut name = None;
    let mut circular = false;
    let mut locus = Locus::default();
    let mut header: Vec<String> = Vec::new();
    let mut sequence = String::new();
    let mut comments: Vec<String> = Vec::new();
    let mut raw: Vec<RawFeature> = Vec::new();
    let mut section = Section::Header;
    let mut in_location = false;

    for line in text.lines() {
        if line.starts_with("//") {
            break;
        }
        let top_level = line.chars().next().is_some_and(|c| !c.is_whitespace());
        if top_level {
            let (key, rest) = split_key(line);
            in_location = false;
            section = match key {
                "LOCUS" => {
                    let (n, c, l) = parse_locus(rest);
                    name = Some(n);
                    circular = c;
                    locus = l;
                    Section::Header
                }
                "COMMENT" => {
                    comments.push(rest.trim().to_string());
                    Section::Comment
                }
                "FEATURES" => Section::Features,
                "ORIGIN" => Section::Origin,
                _ => {
                    header.push(line.trim_end().to_string());
                    Section::Header
                }
            };
            continue;
        }
        match section {
            Section::Header => header.push(line.trim_end().to_string()),
            Section::Comment => comments.push(line.trim().to_string()),
            Section::Origin => sequence.extend(line.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase())),
            Section::Features => feature_line(line, &mut raw, &mut in_location),
        }
    }

    let Some(name) = name else {
        return Err(ReadError::NotGenBank);
    };
    if sequence.is_empty() {
        return Err(ReadError::NoSequence);
    }
    let length = sequence.len();
    let features = raw.into_iter().filter_map(|f| finish_feature(f, circular, length)).collect();
    Ok(Construct { name, circular, sequence, features, comments, locus, header })
}

fn split_key(line: &str) -> (&str, &str) {
    match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], &line[i..]),
        None => (line, ""),
    }
}

fn parse_locus(rest: &str) -> (String, bool, Locus) {
    let mut tokens = rest.split_whitespace();
    let name = tokens.next().unwrap_or_default().to_string();
    let mut circular = false;
    let mut locus = Locus::default();
    // After the name come the length and `bp`, then molecule, topology, division and date, any of
    // which a file may leave out.
    for token in tokens.skip_while(|t| !t.eq_ignore_ascii_case("bp")).skip(1) {
        if token.eq_ignore_ascii_case("circular") {
            circular = true;
        } else if token.eq_ignore_ascii_case("linear") {
        } else if token.contains('-') && token.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            locus.date = token.to_string();
        } else if locus.molecule.is_empty() {
            locus.molecule = token.to_string();
        } else if locus.division.is_empty() {
            locus.division = token.to_string();
        }
    }
    (name, circular || rest.split_whitespace().any(|t| t.eq_ignore_ascii_case("circular")), locus)
}

/// One indented line of the feature table: a new feature, a qualifier, or a continuation.
fn feature_line(line: &str, raw: &mut Vec<RawFeature>, in_location: &mut bool) {
    let indent = line.len() - line.trim_start().len();
    let body = line.trim();
    if body.is_empty() {
        return;
    }
    // A feature key sits in the key column; qualifiers and continuations sit in the location column.
    if indent < 12 && !body.starts_with('/') {
        let (kind, location) = split_key(body);
        raw.push(RawFeature { kind: kind.to_string(), location: location.trim().to_string(), notes: Vec::new() });
        *in_location = true;
        return;
    }
    let Some(feature) = raw.last_mut() else {
        return;
    };
    if body.starts_with('/') && !value_is_open(feature) {
        *in_location = false;
        let (key, value) = match body[1..].split_once('=') {
            Some((k, v)) => (k, v),
            None => (&body[1..], ""),
        };
        feature.notes.push((key.to_string(), value.to_string()));
    } else if *in_location {
        feature.location.push_str(body);
    } else if let Some((_, value)) = feature.notes.last_mut() {
        value.push_str(body);
    }
}

/// True while the newest qualifier's quoted value has not reached its closing quote.
fn value_is_open(feature: &RawFeature) -> bool {
    let Some((_, value)) = feature.notes.last() else {
        return false;
    };
    value.starts_with('"') && (value.len() < 2 || value.matches('"').count() % 2 == 1)
}

fn unquote(value: &str) -> String {
    let inner = value.strip_prefix('"').map(|v| v.strip_suffix('"').unwrap_or(v)).unwrap_or(value);
    inner.replace("\"\"", "\"").replace('\\', " ")
}

fn finish_feature(raw: RawFeature, circular: bool, length: usize) -> Option<Feature> {
    let expression: String = raw.location.chars().filter(|c| !c.is_whitespace()).collect();
    let parsed = parse_location(&expression, 0).filter(|l| is_local(l, circular, length));
    let (strand, segments, operator, unsupported) = match parsed {
        Some(location) => {
            let reverse = location.parts.first().is_some_and(|p| p.reverse);
            let mut segments: Vec<Segment> = location.parts.iter().map(|p| p.segment).collect();
            // Pieces are kept in top-strand order; `complement` applies once, to the whole.
            if reverse {
                segments.reverse();
            }
            let operator = (location.operator == Some(LocationOperator::Order)).then_some(LocationOperator::Order);
            (if reverse { Strand::Reverse } else { Strand::Forward }, segments, operator, None)
        }
        // A location Bios cannot interpret (a remote reference, a site between two bases, pieces
        // on both strands) is kept as written. The feature still needs somewhere to be drawn, so
        // it gets the span of whatever plain numbers the text holds.
        None => {
            let strand = if expression.contains("complement") { Strand::Reverse } else { Strand::Forward };
            let (start, end) = lenient_bounds(&expression, circular, length);
            (strand, vec![Segment::new(start, end)], None, Some(expression))
        }
    };
    let (start, end) = (segments.first()?.start, segments.last()?.end);

    // Group repeated keys, keeping each key where it first appears in the file.
    let mut notes: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in &raw.notes {
        let value = unquote(value);
        match notes.iter_mut().find(|(k, _)| k == key) {
            Some((_, values)) => values.push(value),
            None => notes.push((key.clone(), vec![value])),
        }
    }

    let mut name = NAME_KEYS.iter().find_map(|k| notes.iter().find(|(key, _)| key == k).and_then(|(_, v)| v.first())).cloned().unwrap_or_default();
    if name.is_empty() {
        name = "Untitled Feature".to_string();
    }
    if name.chars().count() > MAX_NAME {
        name = name.chars().take(MAX_NAME).collect();
    }
    notes.retain(|(k, _)| k != "label");

    let mut feature = Feature {
        name,
        kind: raw.kind,
        start,
        end,
        strand,
        segments,
        notes,
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: operator,
        location_unsupported: unsupported,
    };
    normalize_location(&mut feature, length, circular);
    Some(feature)
}

/// One piece of a location and the strand it was written on.
#[derive(Debug, PartialEq)]
struct Directed {
    segment: Segment,
    reverse: bool,
}

#[derive(Debug, PartialEq)]
struct Location {
    parts: Vec<Directed>,
    operator: Option<LocationOperator>,
}

/// How deep `complement(join(...))` may nest. Real files nest two or three deep.
const MAX_NESTING: usize = 32;

/// Reads a location made of ranges, single bases, `join`, `order` and `complement`. Anything
/// else, such as a reference to another record, gives `None`: it is never guessed at.
fn parse_location(expression: &str, nesting: usize) -> Option<Location> {
    if nesting > MAX_NESTING {
        return None;
    }
    if let Some(segment) = parse_range(expression) {
        return Some(Location { parts: vec![Directed { segment, reverse: false }], operator: None });
    }
    let (op, rest) = ["complement", "join", "order"].iter().find_map(|op| expression.strip_prefix(op).map(|rest| (*op, rest)))?;
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;

    let mut arguments = Vec::new();
    let (mut depth, mut from) = (0usize, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                arguments.push(inner.get(from..i)?);
                from = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    arguments.push(inner.get(from..)?);
    let children: Vec<Location> = arguments.into_iter().map(|a| parse_location(a, nesting + 1)).collect::<Option<_>>()?;

    if op == "complement" {
        let [child] = <[Location; 1]>::try_from(children).ok()?;
        let parts = child.parts.into_iter().rev().map(|p| Directed { segment: p.segment, reverse: !p.reverse }).collect();
        return Some(Location { parts, operator: child.operator });
    }
    let operator = if op == "order" { LocationOperator::Order } else { LocationOperator::Join };
    if children.iter().any(|c| c.operator.is_some_and(|o| o != operator)) {
        return None;
    }
    Some(Location { parts: children.into_iter().flat_map(|c| c.parts).collect(), operator: Some(operator) })
}

/// `235..614`, `<5..>9` or a single base `42`, as a 0-based inclusive piece.
fn parse_range(text: &str) -> Option<Segment> {
    fn end(text: &str) -> Option<(Option<Fuzzy>, usize)> {
        let (fuzzy, digits) = match text.strip_prefix('<') {
            Some(rest) => (Some(Fuzzy::Before), rest),
            None => match text.strip_prefix('>') {
                Some(rest) => (Some(Fuzzy::After), rest),
                None => (None, text),
            },
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n: usize = digits.parse().ok()?;
        n.checked_sub(1).map(|at| (fuzzy, at))
    }
    match text.split_once("..") {
        Some((left, right)) => {
            let ((start_fuzzy, start), (end_fuzzy, end)) = (end(left)?, end(right)?);
            Some(Segment { start, end, start_fuzzy, end_fuzzy })
        }
        None => match end(text)? {
            (None, at) => Some(Segment::new(at, at)),
            _ => None,
        },
    }
}

/// True when every piece lies on this construct, on one strand.
fn is_local(location: &Location, circular: bool, length: usize) -> bool {
    let Some(first) = location.parts.first() else {
        return false;
    };
    location
        .parts
        .iter()
        .all(|p| p.reverse == first.reverse && p.segment.start < length && p.segment.end < length && (p.segment.end >= p.segment.start || circular))
}

/// Where to draw a feature whose location could not be read: from the first to the last plain
/// number in the text that is a base of this construct.
fn lenient_bounds(expression: &str, circular: bool, length: usize) -> (usize, usize) {
    let numbers: Vec<usize> = expression
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse::<usize>().ok())
        .filter(|n| *n >= 1 && *n <= length)
        .map(|n| n - 1)
        .collect();
    match (numbers.first(), numbers.last()) {
        (Some(&start), Some(&end)) if end >= start || circular => (start, end),
        (Some(&start), _) => (start, start),
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::{Directed, ReadError, parse_location, read, unquote};
    use crate::model::{Fuzzy, LocationOperator, Segment};

    fn pieces(expression: &str) -> Vec<(usize, usize, bool)> {
        parse_location(expression, 0).unwrap().parts.iter().map(|p| (p.segment.start, p.segment.end, p.reverse)).collect()
    }

    #[test]
    fn text_without_locus_is_refused() {
        assert_eq!(read(">gfp\nATGC\n"), Err(ReadError::NotGenBank));
    }

    #[test]
    fn locations_become_zero_based_inclusive() {
        assert_eq!(pieces("235..614"), vec![(234, 613, false)]);
        assert_eq!(pieces("42"), vec![(41, 41, false)]);
        assert_eq!(pieces("join(10..20,30..40)"), vec![(9, 19, false), (29, 39, false)]);
    }

    #[test]
    fn complement_reverses_the_order_and_the_strand() {
        assert_eq!(pieces("complement(join(10..20,30..40))"), vec![(29, 39, true), (9, 19, true)]);
        assert_eq!(pieces("join(complement(30..40),complement(10..20))"), vec![(29, 39, true), (9, 19, true)]);
    }

    #[test]
    fn uncertain_ends_and_order_are_kept() {
        let partial = parse_location("<5..>9", 0).unwrap();
        assert_eq!(
            partial.parts,
            vec![Directed { segment: Segment { start: 4, end: 8, start_fuzzy: Some(Fuzzy::Before), end_fuzzy: Some(Fuzzy::After) }, reverse: false }]
        );
        assert_eq!(parse_location("order(2..5,12..15)", 0).unwrap().operator, Some(LocationOperator::Order));
    }

    #[test]
    fn what_cannot_be_read_is_not_guessed() {
        for expression in ["2^3", "join(2..5,ACC.1:2..5)", "join(2..5", "complement(2..5,7..9)", "0..5", "join(order(2..5),7..9)"] {
            assert_eq!(parse_location(expression, 0), None, "{expression}");
        }
    }

    #[test]
    fn doubled_quote_is_one_quote() {
        assert_eq!(unquote("\"a \"\"b\"\" c\""), "a \"b\" c");
    }
}
