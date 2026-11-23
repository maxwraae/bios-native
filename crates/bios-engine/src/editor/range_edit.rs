//! Replacing a range given by its coordinates. Ported from the web app's `rangeEdit.ts`.
//!
//! A person types where the edit goes, as they count bases: from 1, with both ends included. This
//! checks what they typed, makes the same edit typing would, and says what it does to each
//! feature and primer before anything is applied.

use std::fmt;

use bios_core::Construct;
use bios_core::edits::{EditError, EditResult};
use bios_core::model::Feature;

use super::edit::{BaseRange, Where, edited, is_js_space, removed, replaced, selected_bases};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeAction {
    Select,
    Replace,
    Insert,
    Delete,
}

/// What a person typed into the range fields. `from` and `to` are 1-based and inclusive; for an
/// insertion `from` is the base the new ones go after, so `0` is before the first base, and `to`
/// is not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeRequest<'a> {
    pub action: RangeAction,
    pub from: &'a str,
    pub to: &'a str,
    pub bases: &'a str,
}

/// Which list an annotation is shown in. A primer is a `primer_bind` feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    Feature,
    Primer,
}

impl fmt::Display for AnnotationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AnnotationKind::Feature => "Feature",
            AnnotationKind::Primer => "Primer",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Removed,
    BasesChanged,
    PositionChanged,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Change::Removed => "removed",
            Change::BasesChanged => "bases changed",
            Change::PositionChanged => "position changed",
        })
    }
}

/// What the edit does to one annotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnotationChange {
    pub name: String,
    pub kind: AnnotationKind,
    pub change: Change,
}

/// The edit a request asks for, not yet applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangePreview {
    /// Where the edit goes, 0-based: a caret for an insertion, otherwise the range.
    pub at: Where,
    /// The bases going in, cleaned and in upper case. Empty for a selection or a deletion.
    pub bases: String,
    /// How many bases the range covers.
    pub removed: usize,
    /// The construct after the edit. The construct as it is, for a selection.
    pub next: Construct,
    /// Features first, then primers, each in the construct's order.
    pub changes: Vec<AnnotationChange>,
}

/// Why a request cannot be previewed. Each reads as the web app words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RangeError {
    NoSequence,
    /// The insertion point is not a whole number from 0 to `length`.
    InsertPosition {
        length: usize,
    },
    /// An end of the range is not a whole number from 1 to `length`.
    Coordinates {
        length: usize,
    },
    /// The end is before the start, and the sequence is linear.
    EndBeforeStart,
    NotDna,
    NoBases,
    /// The edit would leave no bases.
    WouldEmpty,
    Edit(EditError),
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeError::NoSequence => write!(f, "Open a DNA sequence first."),
            RangeError::InsertPosition { length } => write!(f, "Enter a position from 0 to {}; 0 is before the first base.", grouped(*length)),
            RangeError::Coordinates { length } => write!(f, "Enter whole-number coordinates from 1 to {}.", grouped(*length)),
            RangeError::EndBeforeStart => write!(f, "The end must follow the start on a linear sequence."),
            RangeError::NotDna => write!(f, "Use DNA bases or IUPAC ambiguity codes; other text is not accepted."),
            RangeError::NoBases => write!(f, "Enter the bases to insert. Choose Delete to remove bases."),
            RangeError::WouldEmpty => write!(f, "Keep at least one base in the construct."),
            RangeError::Edit(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RangeError {}

impl From<EditError> for RangeError {
    fn from(error: EditError) -> Self {
        RangeError::Edit(error)
    }
}

/// A count with a comma between each three digits, as the web app prints one: `47,201`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The largest whole number the web app can hold exactly. It refuses anything above.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// The whole number a person typed: digits alone, or digits with a comma before each three
/// (`1,234`). `None` for anything else, such as a sign, a fraction or an exponent.
fn coordinate(text: &str) -> Option<usize> {
    let value = text.trim_matches(is_js_space);
    let digits_only = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let mut groups = value.split(',');
    let first = groups.next()?;
    let rest: Vec<&str> = groups.collect();
    let well_formed =
        digits_only(first) && (rest.is_empty() || (first.len() <= 3 && rest.iter().all(|group| group.len() == 3 && digits_only(group))));
    if !well_formed {
        return None;
    }
    let mut n: u64 = 0;
    for digit in value.bytes().filter(u8::is_ascii_digit) {
        n = n.checked_mul(10)?.checked_add(u64::from(digit - b'0'))?;
    }
    usize::try_from(n).ok().filter(|_| n <= MAX_SAFE_INTEGER)
}

fn is_primer(f: &Feature) -> bool {
    f.kind.eq_ignore_ascii_case("primer_bind")
}

/// The bases an annotation covers, piece by piece, in upper case.
fn annotated_bases(sequence: &str, annotation: &Feature) -> String {
    let pieces: Vec<BaseRange> = match annotation.locations() {
        Some(locations) => locations.iter().map(|p| BaseRange::new(p.start, p.end)).collect(),
        None => vec![BaseRange::new(annotation.start, annotation.end)],
    };
    pieces.into_iter().map(|piece| selected_bases(sequence, piece)).collect::<String>().to_uppercase()
}

/// The same record before and after an edit: the edit does not change these.
fn same_record(a: &Feature, b: &Feature) -> bool {
    a.name == b.name && a.kind == b.kind && a.strand == b.strand && a.start == b.start && a.end == b.end
}

/// What the edit did to each annotation that was there before it.
///
/// An edit keeps the features it does not remove in their order, and lists the ones it removes
/// in their order, so each feature is found again by walking the two lists together. A name would
/// not do: two features may share one.
fn annotation_changes(before: &Construct, result: &EditResult) -> Vec<AnnotationChange> {
    let mut removed = result.delta.removed.iter().peekable();
    let mut kept = result.features.iter();
    let (mut features, mut primers) = (Vec::new(), Vec::new());
    let (mut feature_count, mut primer_count) = (0usize, 0usize);
    for old in &before.features {
        let after = if removed.next_if(|gone| same_record(gone, old)).is_some() { None } else { kept.next() };
        let (kind, count, changes) = if is_primer(old) {
            (AnnotationKind::Primer, &mut primer_count, &mut primers)
        } else {
            (AnnotationKind::Feature, &mut feature_count, &mut features)
        };
        *count += 1;
        let change = match after {
            None => Change::Removed,
            Some(new) if annotated_bases(&before.sequence, old) != annotated_bases(&result.sequence, new) => Change::BasesChanged,
            Some(new) if old.start != new.start || old.end != new.end || old.locations() != new.locations() => Change::PositionChanged,
            Some(_) => continue,
        };
        let name = if old.name.is_empty() { format!("{kind} {count}") } else { old.name.clone() };
        changes.push(AnnotationChange { name, kind, change });
    }
    features.append(&mut primers);
    features
}

/// The bases a person typed or pasted: one FASTA header line is dropped, digits and white space
/// are dropped, and anything that is not a base code is refused.
fn typed_bases(text: &str) -> Result<String, RangeError> {
    let trimmed = text.trim_matches(is_js_space);
    let raw = match trimmed.strip_prefix('>') {
        Some(header) => {
            let line = header.find(['\r', '\n']).unwrap_or(header.len());
            let (_, rest) = header.split_at(line);
            // A header ends at a line feed, with a carriage return before it or not, or at the
            // end of the text. A lone carriage return does not end one.
            match rest.strip_prefix("\r\n").or(rest.strip_prefix('\n')) {
                Some(after) => after,
                None if rest.is_empty() => rest,
                None => trimmed,
            }
        }
        None => trimmed,
    };
    let is_base =
        |c: char| matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'U' | 'N' | 'R' | 'Y' | 'K' | 'M' | 'S' | 'W' | 'B' | 'D' | 'H' | 'V');
    if raw.chars().any(|c| !is_base(c) && !is_js_space(c) && !c.is_ascii_digit()) {
        return Err(RangeError::NotDna);
    }
    let bases: String = raw.chars().filter(|&c| is_base(c)).map(|c| c.to_ascii_uppercase()).collect();
    if bases.is_empty() { Err(RangeError::NoBases) } else { Ok(bases) }
}

/// Checks the coordinates a person typed, then previews the edit, which treats features exactly
/// as typing does.
pub fn preview_range_edit(data: &Construct, request: &RangeRequest<'_>) -> Result<RangePreview, RangeError> {
    let length = data.len();
    if length == 0 {
        return Err(RangeError::NoSequence);
    }
    let from = coordinate(request.from);
    let (at, removed_count) = if request.action == RangeAction::Insert {
        let gap = from.filter(|&gap| gap <= length).ok_or(RangeError::InsertPosition { length })?;
        (Where::Caret(gap), 0)
    } else {
        let inside = |n: Option<usize>| n.filter(|n| (1..=length).contains(n));
        let (from, to) = inside(from).zip(inside(coordinate(request.to))).ok_or(RangeError::Coordinates { length })?;
        if to < from && !data.circular {
            return Err(RangeError::EndBeforeStart);
        }
        let removed_count = if to >= from { to - from + 1 } else { length - from + 1 + to };
        (Where::Range(BaseRange::new(from - 1, to - 1)), removed_count)
    };
    let bases = match request.action {
        RangeAction::Insert | RangeAction::Replace => typed_bases(request.bases)?,
        RangeAction::Select | RangeAction::Delete => String::new(),
    };
    if request.action == RangeAction::Select {
        return Ok(RangePreview { at, bases, removed: removed_count, next: data.clone(), changes: Vec::new() });
    }
    if length - removed_count + bases.len() == 0 {
        return Err(RangeError::WouldEmpty);
    }
    let result = match at {
        Where::Range(range) if request.action == RangeAction::Delete => removed(data, range)?,
        _ => replaced(data, at, &bases)?,
    };
    let changes = annotation_changes(data, &result);
    Ok(RangePreview { at, bases, removed: removed_count, next: edited(data, result), changes })
}
