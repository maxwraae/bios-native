//! A copied stretch of DNA with the annotations on it, and putting one back. Ported from the web
//! app's `rows/fragment.ts` and the half of `rows/clipboard.ts` that decides what a copy holds.
//!
//! Copying bases alone loses what they are. So a copy is a [`DnaFragment`]: the bases, every
//! feature on them moved to where it sits in the copy, and every primer that was selected whole.
//! A feature the selection cuts is clipped to it and marked partial. A primer the selection cuts
//! is left out and counted, because half an oligo is not an oligo.
//!
//! Positions are 0-based and inclusive at both ends. A fragment is linear, so nothing in one runs
//! through an origin. A selection whose end is before its start runs through the origin of a
//! circular construct, and its bases are copied in the order they are read.
//!
//! Four things differ from the web app, because a [`Construct`] is not its document:
//!
//! - A construct keeps its file's `source` record among its features. The web app takes it out of
//!   the document it edits, so it is never copied there, and it is not copied here.
//! - A primer is a `primer_bind` feature. Its oligo is the bases it sits on unless a `sequence`
//!   qualifier says otherwise, and that qualifier travels with it. Nothing is added to a primer
//!   that has none.
//! - A construct has nowhere to keep an oligo that binds nowhere. A primer selected whole whose
//!   footprint the cut through the selection splits in two is counted in
//!   [`DnaFragment::split_primer_count`] and left out. The web app carries it without a place.
//! - Every feature has a strand. The web app also has features with none.
//!
//! As JSON a fragment is written the way the web app writes one, so that there is one format.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use bios_core::Construct;
use bios_core::edits::EditError;
use bios_core::model::{Feature, Fuzzy, LocationOperator, Segment, Strand};
use bios_core::seq::revcomp;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use super::edit::{BaseRange, Where, assert_editable_locations, caret_after_insert, insert_bases, is_js_space, selected_bases};

/// What a fragment says it is when written as JSON.
pub const FRAGMENT_KIND: &str = "bios-dna-fragment";
/// The clipboard format a fragment's JSON is offered under.
pub const DNA_CLIPBOARD_TYPE: &str = "application/x-bios-dna+json";
/// The longest JSON a copy may hold, counted as the web app counts text.
const MAX_PAYLOAD: usize = 8 * 1024 * 1024;
/// The largest whole number the web app can hold exactly.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A linear piece of DNA with the annotations on it, in its own coordinates.
///
/// The ids the annotations carry say how they relate to each other inside the fragment. Pasting
/// gives every one a new id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DnaFragment {
    pub sequence: String,
    /// Every feature that is not a primer, clipped to the fragment.
    pub features: Vec<Feature>,
    /// The `primer_bind` features that were selected whole, each one span inside the fragment.
    pub primers: Vec<Feature>,
    /// How many of `features` the selection cut and are marked partial.
    pub partial_feature_count: usize,
    /// Primers the selection covered only part of, which are left out.
    pub omitted_partial_primer_count: usize,
    /// Primers selected whole whose footprint the cut through the selection splits in two, which
    /// are left out. Only a primer through the origin of a circular construct can be one.
    pub split_primer_count: usize,
}

impl DnaFragment {
    /// The fragment as the JSON a copy holds. `None` when the fragment is not valid.
    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(&wire_fragment(&validate_fragment(self)?)).ok()
    }

    /// The fragment some JSON holds, which may have come from anywhere. `None` when it is not a
    /// valid fragment, or is longer than a copy may be.
    pub fn from_json(json: &str) -> Option<DnaFragment> {
        if too_long(json) {
            return None;
        }
        read_wire(&serde_json::from_str::<Json>(json).ok()?)
    }
}

/// Why a fragment cannot be pasted. Each reads as the web app words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FragmentError {
    InvalidFragment,
    DestinationNotDna,
    /// The caret or the selection is not inside the destination.
    Position,
    Edit(EditError),
}

impl fmt::Display for FragmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FragmentError::InvalidFragment => write!(f, "The copied DNA fragment contains invalid sequence or annotation data."),
            FragmentError::DestinationNotDna => write!(f, "The destination must contain DNA bases."),
            FragmentError::Position => write!(f, "Choose an insertion position or selection inside the destination DNA."),
            FragmentError::Edit(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FragmentError {}

impl From<EditError> for FragmentError {
    fn from(error: EditError) -> Self {
        FragmentError::Edit(error)
    }
}

// ── Spans ───────────────────────────────────────────────────────────────────────────────────

/// A private copy of `edit.rs`'s `is_iupac`, which is not visible from here.
fn is_iupac(c: char) -> bool {
    matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'U' | 'N' | 'R' | 'Y' | 'K' | 'M' | 'S' | 'W' | 'B' | 'D' | 'H' | 'V')
}

/// At least one base, and nothing that is not a base or an ambiguity code.
fn is_dna(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_iupac)
}

fn is_primer(f: &Feature) -> bool {
    f.kind.eq_ignore_ascii_case("primer_bind")
}

/// The file's record of what the whole molecule is. It describes the file it is in, not a
/// stretch of bases, so a copy of some bases does not take it along.
fn is_source(f: &Feature) -> bool {
    f.kind.eq_ignore_ascii_case("source")
}

/// Both ends inside a sequence of `length` bases, and not backwards unless it is circular.
fn in_span(range: BaseRange, length: usize, circular: bool) -> bool {
    range.start < length && range.end < length && (circular || range.start <= range.end)
}

/// The range as runs that do not go through the origin: two when it does.
fn pieces(range: BaseRange, length: usize) -> Vec<BaseRange> {
    if range.wraps() { vec![BaseRange::new(range.start, length.saturating_sub(1)), BaseRange::new(0, range.end)] } else { vec![range] }
}

fn length_of(segments: &[BaseRange]) -> usize {
    segments.iter().map(|s| (s.end + 1).saturating_sub(s.start)).sum()
}

/// A location Bios cannot move: one it could not read, or one with an uncertain end.
fn uncertain_location(f: &Feature) -> bool {
    f.location_unsupported.as_deref().is_some_and(|text| !text.is_empty())
        || f.start_fuzzy.is_some()
        || f.end_fuzzy.is_some()
        || f.segments.iter().any(|p| p.start_fuzzy.is_some() || p.end_fuzzy.is_some())
}

/// The feature's pieces in the order its location gives them, which is the order its bases are
/// read in and is not always the order they lie in. `None` when a piece is outside the sequence.
fn feature_segments(f: &Feature, length: usize, circular: bool) -> Option<Vec<BaseRange>> {
    let whole = BaseRange::new(f.start, f.end);
    if !in_span(whole, length, circular) {
        return None;
    }
    let Some(locations) = f.locations() else {
        return Some(pieces(whole, length));
    };
    let spans: Vec<BaseRange> = locations.iter().map(|p| BaseRange::new(p.start, p.end)).collect();
    spans.iter().all(|&p| in_span(p, length, circular)).then(|| spans.iter().flat_map(|&p| pieces(p, length)).collect())
}

/// What the selection keeps of each segment, in the fragment's coordinates. Pieces that end up
/// next to each other are joined.
fn clipped_segments(segments: &[BaseRange], selection: &[BaseRange]) -> Vec<BaseRange> {
    let mut offset = 0;
    let selected: Vec<(BaseRange, usize)> = selection
        .iter()
        .map(|&s| {
            let part = (s, offset);
            offset += (s.end + 1).saturating_sub(s.start);
            part
        })
        .collect();
    let mut out: Vec<BaseRange> = Vec::new();
    for segment in segments {
        let mut overlaps: Vec<(usize, BaseRange)> = selected
            .iter()
            .filter_map(|&(s, offset)| {
                let (start, end) = (segment.start.max(s.start), segment.end.min(s.end));
                (start <= end).then(|| (start, BaseRange::new(start - s.start + offset, end - s.start + offset)))
            })
            .collect();
        overlaps.sort_by_key(|&(original_start, _)| original_start);
        for (_, overlap) in overlaps {
            match out.last_mut() {
                Some(previous) if previous.end + 1 == overlap.start => previous.end = overlap.end,
                _ => out.push(overlap),
            }
        }
    }
    out
}

/// The segments as they lie once the fragment of `length` bases is reverse-complemented.
fn transformed(segments: &[BaseRange], length: usize, reverse: bool) -> Vec<BaseRange> {
    if !reverse {
        return segments.to_vec();
    }
    segments.iter().rev().map(|s| BaseRange::new(length.saturating_sub(s.end + 1), length.saturating_sub(s.start + 1))).collect()
}

/// The feature on `segments`, read along `strand`.
fn located(feature: &Feature, segments: &[BaseRange], strand: Strand) -> Feature {
    let mut out = feature.clone();
    out.start = segments.iter().map(|s| s.start).min().unwrap_or(feature.start);
    out.end = segments.iter().map(|s| s.end).max().unwrap_or(feature.end);
    out.segments =
        if segments.len() > 1 { segments.iter().map(|s| Segment::new(s.start, s.end)).collect() } else { vec![Segment::new(out.start, out.end)] };
    out.strand = strand;
    out
}

/// The run of the feature's own bases the selection keeps, counted 5' to 3' along the feature.
/// `None` when what is kept is not one run, so that which codon each base belongs to is unknown.
fn coding_window(segments: &[BaseRange], selection: &[BaseRange], forward: bool) -> Option<BaseRange> {
    let mut offset = 0;
    let mut retained: Vec<BaseRange> = Vec::new();
    for segment in segments {
        for s in selection {
            let (start, end) = (segment.start.max(s.start), segment.end.min(s.end));
            if start <= end {
                retained.push(BaseRange::new(offset + start - segment.start, offset + end - segment.start));
            }
        }
        offset += (segment.end + 1).saturating_sub(segment.start);
    }
    if !forward {
        retained = retained.iter().map(|s| BaseRange::new(offset.saturating_sub(s.end + 1), offset.saturating_sub(s.start + 1))).collect();
    }
    retained.sort_by_key(|s| s.start);
    let (first, last) = (retained.first()?, retained.last()?);
    retained.windows(2).all(|pair| matches!(pair, [a, b] if b.start == a.end + 1)).then(|| BaseRange::new(first.start, last.end))
}

// ── Qualifiers ──────────────────────────────────────────────────────────────────────────────

type Notes = Vec<(String, Vec<String>)>;

fn note<'a>(notes: &'a Notes, key: &str) -> Option<&'a [String]> {
    notes.iter().find(|(k, _)| k == key).map(|(_, values)| values.as_slice())
}

fn first_note<'a>(notes: &'a Notes, key: &str) -> Option<&'a str> {
    note(notes, key)?.first().map(String::as_str)
}

/// Replaces a qualifier where it stands, or adds it after the others.
fn set_note(notes: &mut Notes, key: &str, values: Vec<String>) {
    match notes.iter_mut().find(|(k, _)| k == key) {
        Some((_, held)) => *held = values,
        None => notes.push((key.to_string(), values)),
    }
}

fn remove_note(notes: &mut Notes, key: &str) {
    notes.retain(|(k, _)| k != key);
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// The number some text holds, as the web app reads one: nothing at all is 0.
fn js_number(text: &str) -> Option<f64> {
    let text = js_trim(text);
    if text.is_empty() { Some(0.0) } else { text.parse::<f64>().ok() }
}

/// The codon position a coding feature's qualifiers give, 1 when they give none. `None` when
/// what they give is not 1, 2 or 3.
fn codon_start(notes: &Notes) -> Option<i128> {
    let stated = first_note(notes, "codon_start").map_or(Some(1.0), js_number)?;
    [1.0, 2.0, 3.0].contains(&stated).then_some(stated as i128)
}

/// The feature marked as a part of what it was: `kept` of its `original` bases are in the
/// fragment. A coding feature keeps the frame of the codons that are left when `coding` says
/// which of its bases they are, and otherwise says it can no longer be translated.
fn partial_feature(mut feature: Feature, kept: usize, original: usize, coding: Option<BaseRange>) -> Feature {
    let cds = feature.kind.eq_ignore_ascii_case("cds");
    let frame = coding.zip(codon_start(&feature.notes)).filter(|_| first_note(&feature.notes, "bios_translation_unavailable") != Some("true"));
    let review = if cds && frame.is_none() { " CDS reading frame and translation require review." } else { "" };
    let mut lines = note(&feature.notes, "note").unwrap_or_default().to_vec();
    lines.push(format!("Partial feature copied: {kept} of {original} annotated bases retained.{review}"));
    set_note(&mut feature.notes, "note", lines);
    set_note(&mut feature.notes, "bios_fragment_partial", vec!["true".to_string()]);
    if cds {
        remove_note(&mut feature.notes, "translation");
        remove_note(&mut feature.notes, "protein_id");
        match frame {
            Some((coding, old_start)) => {
                let start = 1 + (old_start - 1 - coding.start as i128).rem_euclid(3);
                set_note(&mut feature.notes, "codon_start", vec![start.to_string()]);
            }
            None => {
                remove_note(&mut feature.notes, "codon_start");
                set_note(&mut feature.notes, "bios_translation_unavailable", vec!["true".to_string()]);
            }
        }
    }
    if !feature.name.trim_end_matches(is_js_space).to_ascii_lowercase().ends_with("(partial)") {
        feature.name.push_str(" (partial)");
    }
    feature
}

// ── Copying ─────────────────────────────────────────────────────────────────────────────────

/// A selection of a construct, and how it is copied.
struct Cut<'a> {
    selection: &'a [BaseRange],
    /// How many bases the construct has.
    length: usize,
    circular: bool,
    /// How many bases are selected.
    selected: usize,
    reverse: bool,
}

/// An annotation whose place cannot be worked out. Nothing is copied from a construct that holds
/// one, because the copy would silently leave it behind.
struct Unreadable;

fn strand_in(cut: &Cut<'_>, strand: Strand) -> Strand {
    match (strand, cut.reverse) {
        (Strand::Forward, false) | (Strand::Reverse, true) => Strand::Forward,
        _ => Strand::Reverse,
    }
}

/// The feature as it is in the fragment, and whether the selection cut it. `None` when the
/// selection does not touch it.
fn copied_feature(raw: &Feature, cut: &Cut<'_>) -> Result<Option<(Feature, bool)>, Unreadable> {
    if uncertain_location(raw) {
        return Err(Unreadable);
    }
    let segments = feature_segments(raw, cut.length, cut.circular).ok_or(Unreadable)?;
    let overlap = clipped_segments(&segments, cut.selection);
    if overlap.is_empty() {
        return Ok(None);
    }
    let feature = located(raw, &transformed(&overlap, cut.selected, cut.reverse), strand_in(cut, raw.strand));
    let (kept, original) = (length_of(&overlap), length_of(&segments));
    if kept >= original {
        return Ok(Some((feature, false)));
    }
    let coding = coding_window(&segments, cut.selection, raw.strand == Strand::Forward);
    Ok(Some((partial_feature(feature, kept, original, coding), true)))
}

enum CopiedPrimer {
    /// The selection does not touch it, or it is not on the construct.
    Outside,
    /// The selection covers part of it.
    Partial,
    /// The selection covers all of it, in two places of the fragment.
    Split,
    Whole(Feature),
}

fn copied_primer(raw: &Feature, cut: &Cut<'_>) -> Result<CopiedPrimer, Unreadable> {
    let footprint = BaseRange::new(raw.start, raw.end);
    if !in_span(footprint, cut.length, cut.circular) {
        return Ok(CopiedPrimer::Outside);
    }
    if raw.locations().is_some() || raw.location_operator == Some(LocationOperator::Order) || uncertain_location(raw) {
        return Err(Unreadable);
    }
    let original = pieces(footprint, cut.length);
    let overlap = clipped_segments(&original, cut.selection);
    if overlap.is_empty() {
        return Ok(CopiedPrimer::Outside);
    }
    if length_of(&overlap) != length_of(&original) {
        return Ok(CopiedPrimer::Partial);
    }
    // Every base of the primer was selected, but the cut through the selection splits its
    // footprint. One place in the pasted DNA must not be made up for it.
    let [position] = transformed(&overlap, cut.selected, cut.reverse)[..] else {
        return Ok(CopiedPrimer::Split);
    };
    let mut primer = located(raw, &[position], strand_in(cut, raw.strand));
    primer.location_operator = None;
    Ok(CopiedPrimer::Whole(primer))
}

/// Copies a selected range with the annotations on it. `reverse` gives the reverse complement,
/// which turns every annotation round with the bases. A primer turned round binds the other
/// strand and is still the same oligo.
///
/// `None` when the range is not inside the construct, when it runs backwards on a linear one, or
/// when the construct holds something that is not DNA or an annotation whose place cannot be
/// worked out.
pub fn extract_fragment(data: &Construct, range: BaseRange, reverse: bool) -> Option<DnaFragment> {
    let sequence = data.sequence.as_str();
    if !is_dna(sequence) || !in_span(range, sequence.len(), data.circular) {
        return None;
    }
    let selection = pieces(range, sequence.len());
    let selected = selected_bases(sequence, range);
    let cut = Cut { selection: &selection, length: sequence.len(), circular: data.circular, selected: selected.len(), reverse };
    let mut fragment = DnaFragment { sequence: if reverse { revcomp(&selected) } else { selected }, ..DnaFragment::default() };
    for raw in data.features.iter().filter(|f| !is_primer(f) && !is_source(f)) {
        if let Some((feature, partial)) = copied_feature(raw, &cut).ok()? {
            fragment.partial_feature_count += usize::from(partial);
            fragment.features.push(feature);
        }
    }
    for raw in data.features.iter().filter(|f| is_primer(f)) {
        match copied_primer(raw, &cut).ok()? {
            CopiedPrimer::Outside => {}
            CopiedPrimer::Partial => fragment.omitted_partial_primer_count += 1,
            CopiedPrimer::Split => fragment.split_primer_count += 1,
            CopiedPrimer::Whole(primer) => fragment.primers.push(primer),
        }
    }
    validate_fragment(&fragment)
}

// ── A primer's oligo ────────────────────────────────────────────────────────────────────────

/// The oligo a primer's `sequence` qualifier holds, without white space and in upper case.
fn stored_bases(notes: &Notes) -> Option<String> {
    let text: String = first_note(notes, "sequence")?.chars().filter(|&c| !is_js_space(c)).collect();
    (!text.is_empty()).then(|| text.to_uppercase())
}

/// What the footprint reads 5' to 3' along the primer. Empty when it is not inside `sequence`.
fn footprint_bases(sequence: &str, start: usize, end: usize, forward: bool) -> String {
    let top = sequence.get(start..end.saturating_add(1)).unwrap_or_default().to_ascii_uppercase();
    if forward { top } else { revcomp(&top) }
}

/// The length of the 5' tail the qualifiers state. `None` when they state none, and `Some(None)`
/// when what they state is not a whole number from 0.
fn stated_tail(notes: &Notes) -> Option<Option<usize>> {
    let text = first_note(notes, "bios_primer_tail_length").filter(|text| !text.is_empty())?;
    Some(js_number(text).filter(|n| n.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER).contains(n)).map(|n| n as usize))
}

fn stated_inferred(notes: &Notes) -> Option<bool> {
    match first_note(notes, "bios_primer_tail_inferred")? {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// A primer as the web app holds one: the whole oligo, how much of its 5' end is a tail, and
/// whether that length is only a guess from the oligo being longer than its footprint.
struct Oligo {
    bases: String,
    /// `None` when the qualifiers state a tail that is not a length.
    tail_length: Option<usize>,
    tail_inferred: Option<bool>,
}

fn oligo_of(primer: &Feature, sequence: &str) -> Oligo {
    let forward = primer.strand == Strand::Forward;
    let bases = stored_bases(&primer.notes).unwrap_or_else(|| footprint_bases(sequence, primer.start, primer.end, forward));
    let span = primer.end.saturating_add(1).saturating_sub(primer.start);
    match stated_tail(&primer.notes) {
        Some(tail_length) => Oligo { bases, tail_length, tail_inferred: stated_inferred(&primer.notes) },
        None => Oligo { tail_length: Some(bases.len().saturating_sub(span)), bases, tail_inferred: Some(true) },
    }
}

// ── JSON that keeps its order ───────────────────────────────────────────────────────────────

/// A JSON value whose objects keep the order of their keys, as the web app's do. A feature's
/// qualifiers are written to its file in the order they are held, and `serde_json`'s own value
/// would sort them.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Text(text) => Some(text),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    fn as_list(&self) -> Option<&[Json]> {
        match self {
            Json::List(items) => Some(items),
            _ => None,
        }
    }

    /// What the web app takes for true: anything but nothing, false, zero and empty text.
    fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(value) => *value,
            Json::Number(n) => *n != 0.0 && !n.is_nan(),
            Json::Text(text) => !text.is_empty(),
            Json::List(_) | Json::Object(_) => true,
        }
    }

    /// A whole number the web app can hold exactly.
    fn safe_integer(&self) -> Option<f64> {
        match self {
            Json::Number(n) if n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER => Some(*n),
            _ => None,
        }
    }

    /// A whole number from 0.
    fn count(&self) -> Option<usize> {
        self.safe_integer().filter(|n| *n >= 0.0).map(|n| n as usize)
    }
}

fn truthy(value: Option<&Json>) -> bool {
    value.is_some_and(Json::truthy)
}

fn number(n: usize) -> Json {
    Json::Number(n as f64)
}

fn text(value: &str) -> Json {
    Json::Text(value.to_string())
}

fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_unit(),
            Json::Bool(value) => serializer.serialize_bool(*value),
            Json::Number(n) => match self.safe_integer() {
                Some(whole) => serializer.serialize_i64(whole as i64),
                None => serializer.serialize_f64(*n),
            },
            Json::Text(text) => serializer.serialize_str(text),
            Json::List(items) => {
                let mut list = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    list.serialize_element(item)?;
                }
                list.end()
            }
            Json::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Number(value as f64))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
        Ok(Json::Number(value as f64))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Json, E> {
        Ok(Json::Number(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Json, E> {
        Ok(Json::Text(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Json, E> {
        Ok(Json::Text(value))
    }

    // Nothing is set aside ahead of what is read: the text says how long a list is, and the text
    // may have come from anywhere.
    fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = list.next_element()? {
            items.push(item);
        }
        Ok(Json::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut fields: Vec<(String, Json)> = Vec::new();
        let mut places: HashMap<String, usize> = HashMap::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            // A key given twice keeps its first place and its last value, as in the web app.
            match places.get(&key).and_then(|&at| fields.get_mut(at)) {
                Some(field) => field.1 = value,
                None => {
                    places.insert(key.clone(), fields.len());
                    fields.push((key, value));
                }
            }
        }
        Ok(Json::Object(fields))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

/// Longer than a copy may be. The web app counts text in 16-bit units, which are never more
/// than its bytes.
fn too_long(text: &str) -> bool {
    text.len() > MAX_PAYLOAD && text.encode_utf16().count() > MAX_PAYLOAD
}

// ── Writing a fragment ──────────────────────────────────────────────────────────────────────

fn wire_fuzzy(fuzzy: Fuzzy) -> Json {
    text(match fuzzy {
        Fuzzy::Before => "<",
        Fuzzy::After => ">",
    })
}

fn wire_notes(notes: &Notes) -> Json {
    let mut fields: Vec<(String, Vec<Json>)> = Vec::new();
    for (key, values) in notes {
        let values = values.iter().map(|value| text(value));
        match fields.iter_mut().find(|(k, _)| k == key) {
            Some((_, held)) => held.extend(values),
            None => fields.push((key.clone(), values.collect())),
        }
    }
    Json::Object(fields.into_iter().map(|(key, values)| (key, Json::List(values))).collect())
}

/// Where an annotation is. Whatever a fragment may not hold (an uncertain end, a location Bios
/// could not read) is written too, so that reading the fragment back refuses it.
fn wire_location(f: &Feature) -> Vec<(&'static str, Json)> {
    let mut fields = vec![("start", number(f.start)), ("end", number(f.end))];
    if let Some(locations) = f.locations() {
        let piece = |p: &Segment| {
            let mut piece = vec![("start", number(p.start)), ("end", number(p.end))];
            piece.extend(p.start_fuzzy.map(|fuzzy| ("startFuzzy", wire_fuzzy(fuzzy))));
            piece.extend(p.end_fuzzy.map(|fuzzy| ("endFuzzy", wire_fuzzy(fuzzy))));
            object(piece)
        };
        fields.push(("locations", Json::List(locations.iter().map(piece).collect())));
    }
    let lone = f.segments.first().filter(|_| f.segments.len() == 1);
    fields.extend(f.start_fuzzy.or(lone.and_then(|p| p.start_fuzzy)).map(|fuzzy| ("startFuzzy", wire_fuzzy(fuzzy))));
    fields.extend(f.end_fuzzy.or(lone.and_then(|p| p.end_fuzzy)).map(|fuzzy| ("endFuzzy", wire_fuzzy(fuzzy))));
    fields.extend(f.location_operator.map(|operator| {
        let name = match operator {
            LocationOperator::Join => "join",
            LocationOperator::Order => "order",
        };
        ("locationOperator", text(name))
    }));
    fields.extend(f.location_unsupported.as_deref().map(|location| ("locationUnsupported", text(location))));
    fields
}

fn wire_direction(strand: Strand) -> [(&'static str, Json); 2] {
    [("forward", Json::Bool(strand == Strand::Forward)), ("strand", Json::Number(f64::from(strand.sign())))]
}

fn wire_feature(f: &Feature) -> Json {
    let mut fields = vec![("name", text(&f.name)), ("type", text(&f.kind))];
    fields.extend(wire_location(f));
    fields.push(("notes", wire_notes(&f.notes)));
    fields.extend(wire_direction(f.strand));
    object(fields)
}

/// A primer as the web app holds one, with the feature it is here kept beside it as `_src`.
fn wire_primer(primer: &Feature, index: usize, sequence: &str) -> Json {
    let oligo = oligo_of(primer, sequence);
    let id = feature_id(note_lines(&primer.notes)).filter(|id| !id.is_empty()).unwrap_or_else(|| format!("fragment-{index}"));
    let mut source =
        vec![("type", text(&primer.kind)), ("notes", wire_notes(&primer.notes)), ("start", number(primer.start)), ("end", number(primer.end))];
    source.extend(wire_direction(primer.strand));
    let mut fields = vec![("id", Json::Text(format!("primer-{id}"))), ("name", text(&primer.name))];
    fields.extend(wire_location(primer));
    fields.push(("forward", Json::Bool(primer.strand == Strand::Forward)));
    fields.push(("bases", Json::Text(oligo.bases)));
    fields.push(("tailLength", oligo.tail_length.map_or(Json::Null, number)));
    fields.extend(oligo.tail_inferred.map(|inferred| ("tailLengthInferred", Json::Bool(inferred))));
    fields.push(("_src", object(source)));
    fields.push(("unbound", Json::Bool(false)));
    object(fields)
}

fn wire_fragment(fragment: &DnaFragment) -> Json {
    let primers = fragment.primers.iter().enumerate().map(|(index, primer)| wire_primer(primer, index, &fragment.sequence));
    let mut fields = vec![
        ("kind", text(FRAGMENT_KIND)),
        ("version", number(1)),
        ("sequence", text(&fragment.sequence)),
        ("features", Json::List(fragment.features.iter().map(wire_feature).collect())),
        ("primers", Json::List(primers.collect())),
        ("partialFeatureCount", number(fragment.partial_feature_count)),
        ("omittedPartialPrimerCount", number(fragment.omitted_partial_primer_count)),
    ];
    // Not a field of the web app's, which reads past it.
    if fragment.split_primer_count > 0 {
        fields.push(("splitPrimerCount", number(fragment.split_primer_count)));
    }
    object(fields)
}

// ── Reading a fragment ──────────────────────────────────────────────────────────────────────

/// A qualifier map: every value a list of text.
fn valid_notes(value: Option<&Json>) -> bool {
    match value {
        None => true,
        Some(Json::Object(fields)) => fields.iter().all(|(_, values)| values.as_list().is_some_and(|list| list.iter().all(|v| v.as_str().is_some()))),
        Some(_) => false,
    }
}

/// The fields drawing and identity read as plain values must be plain values. Anything else an
/// annotation carries is not looked at.
fn valid_annotation_fields(value: &Json) -> bool {
    let all = |keys: &[&str], test: fn(&Json) -> bool| keys.iter().all(|key| value.get(key).is_none_or(test));
    all(&["name", "type", "id", "color", "labelColor", "description", "bases"], |v| v.as_str().is_some())
        && value.get("id").and_then(Json::as_str) != Some("")
        && all(&["forward", "partial", "translationUnavailable"], |v| v.as_bool().is_some())
        && all(&["strand"], |v| matches!(v, Json::Number(n) if [-1.0, 0.0, 1.0].contains(n)))
        && valid_notes(value.get("notes"))
}

fn wire_uncertain(value: &Json) -> bool {
    let fuzzy = |v: &Json| truthy(v.get("startFuzzy")) || truthy(v.get("endFuzzy"));
    truthy(value.get("locationUnsupported"))
        || fuzzy(value)
        || value.get("locations").and_then(Json::as_list).is_some_and(|pieces| pieces.iter().any(|p| matches!(p, Json::Null) || fuzzy(p)))
}

/// The span an annotation gives, when both ends are whole numbers inside a linear sequence of
/// `length` bases and the end is not before the start.
fn wire_span(value: &Json, length: usize) -> Option<BaseRange> {
    let end_of = |key: &str| value.get(key).and_then(Json::count).filter(|n| *n < length);
    let (start, end) = (end_of("start")?, end_of("end")?);
    (start <= end).then(|| BaseRange::new(start, end))
}

/// The feature's span and its pieces, when it gives any. `None` when one is outside the sequence.
fn wire_segments(value: &Json, length: usize) -> Option<(BaseRange, Option<Vec<BaseRange>>)> {
    let whole = wire_span(value, length)?;
    let Some(locations) = value.get("locations") else {
        return Some((whole, None));
    };
    let pieces = locations.as_list().filter(|pieces| !pieces.is_empty())?;
    let spans: Vec<BaseRange> = pieces.iter().map(|p| wire_span(p, length).filter(|_| valid_annotation_fields(p))).collect::<Option<_>>()?;
    Some((whole, Some(spans)))
}

fn wire_note_lines(value: &Json) -> Vec<String> {
    let lines = value.get("notes").and_then(|notes| notes.get("note")).and_then(Json::as_list).unwrap_or_default();
    lines.iter().filter_map(Json::as_str).map(str::to_string).collect()
}

fn wire_notes_of(value: &Json) -> Notes {
    let Some(Json::Object(fields)) = value.get("notes") else {
        return Notes::new();
    };
    let values = |list: &Json| list.as_list().unwrap_or_default().iter().filter_map(Json::as_str).map(str::to_string).collect();
    fields.iter().map(|(key, list)| (key.clone(), values(list))).collect()
}

/// The names an annotation goes by inside a fragment. No two annotations may share one, or a
/// link from one to another would not say which it means.
struct Aliases(HashSet<String>);

impl Aliases {
    fn remember(&mut self, ids: &[Option<String>]) -> bool {
        let own: HashSet<&String> = ids.iter().flatten().filter(|id| !id.is_empty()).collect();
        own.into_iter().all(|id| self.0.insert(id.clone()))
    }
}

fn wire_strand(value: &Json) -> Strand {
    let forward = value.get("forward").and_then(Json::as_bool);
    let reverse = matches!(value.get("strand"), Some(Json::Number(n)) if *n == -1.0);
    // The web app has features with no direction. A construct's features all have one.
    if forward.unwrap_or(!reverse) { Strand::Forward } else { Strand::Reverse }
}

fn read_feature(value: &Json, length: usize, aliases: &mut Aliases) -> Option<Feature> {
    if !value.is_object() || !valid_annotation_fields(value) || wire_uncertain(value) {
        return None;
    }
    let kind = value.get("type").and_then(Json::as_str);
    let id = value.get("id").and_then(Json::as_str);
    let overlay = id
        .is_some_and(|id| ["binder", "probe"].iter().any(|word| id.strip_prefix(word).is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))));
    if kind.is_some_and(|kind| kind.to_lowercase() == "primer_bind") || overlay {
        return None;
    }
    let (whole, pieces) = wire_segments(value, length)?;
    if !aliases.remember(&[id.map(str::to_string), feature_id(&wire_note_lines(value))]) {
        return None;
    }
    let segments = match pieces {
        Some(pieces) if pieces.len() > 1 => pieces.iter().map(|p| Segment::new(p.start, p.end)).collect(),
        _ => vec![Segment::new(whole.start, whole.end)],
    };
    Some(Feature {
        name: value.get("name").and_then(Json::as_str).unwrap_or_default().to_string(),
        kind: kind.unwrap_or("misc_feature").to_string(),
        start: whole.start,
        end: whole.end,
        strand: wire_strand(value),
        segments,
        notes: wire_notes_of(value),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: match value.get("locationOperator").and_then(Json::as_str) {
            Some("order") => Some(LocationOperator::Order),
            Some("join") => Some(LocationOperator::Join),
            _ => None,
        },
        location_unsupported: None,
    })
}

/// The feature a primer was read from is where it came from, not where it is, so its place may
/// lie outside the fragment.
fn valid_primer_source(value: Option<&Json>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let end = |key: &str| value.get(key).is_none_or(|v| v.safe_integer().is_some_and(|n| n >= -1.0));
    let piece = |p: &Json| p.is_object() && p.get("start").and_then(Json::count).is_some() && p.get("end").and_then(Json::count).is_some();
    value.is_object()
        && valid_annotation_fields(value)
        && end("start")
        && end("end")
        && value
            .get("locations")
            .is_none_or(|pieces| pieces.as_list().is_some_and(|pieces| pieces.iter().all(|p| piece(p) && valid_annotation_fields(p))))
}

/// A primer as the web app holds one, checked. `unbound` is an oligo with no place.
struct WirePrimer<'a> {
    name: &'a str,
    span: Option<BaseRange>,
    forward: bool,
    bases: &'a str,
    tail_length: usize,
    tail_inferred: Option<bool>,
    source: Option<&'a Json>,
}

fn checked_primer<'a>(value: &'a Json, length: usize, aliases: &mut Aliases) -> Option<WirePrimer<'a>> {
    if !value.is_object() || !valid_annotation_fields(value) {
        return None;
    }
    let unbound = value.get("unbound");
    if unbound.is_some_and(|v| v.as_bool().is_none()) {
        return None;
    }
    if !truthy(unbound) && (wire_uncertain(value) || truthy(value.get("locations")) || truthy(value.get("locationOperator"))) {
        return None;
    }
    let span = if unbound.and_then(Json::as_bool) == Some(true) {
        let nowhere = |key: &str| matches!(value.get(key), Some(Json::Number(n)) if *n == -1.0);
        (nowhere("start") && nowhere("end")).then_some(None)?
    } else {
        Some(wire_span(value, length)?)
    };
    let id = value.get("id").and_then(Json::as_str).filter(|id| id.starts_with("primer-"))?;
    let bases = value.get("bases").and_then(Json::as_str).filter(|bases| is_dna(bases))?;
    let tail_length = value.get("tailLength").and_then(Json::count).filter(|tail| *tail < bases.len())?;
    let tail_inferred = value.get("tailLengthInferred");
    let source = value.get("_src");
    if tail_inferred.is_some_and(|v| v.as_bool().is_none()) || !valid_primer_source(source) {
        return None;
    }
    let source = source.filter(|source| source.is_object());
    let source_id = source.and_then(|source| source.get("id")).and_then(Json::as_str).map(str::to_string);
    if !aliases.remember(&[Some(id.to_string()), source_id, source.and_then(|source| feature_id(&wire_note_lines(source)))]) {
        return None;
    }
    Some(WirePrimer {
        name: value.get("name").and_then(Json::as_str)?,
        span,
        forward: value.get("forward").and_then(Json::as_bool)?,
        bases,
        tail_length,
        tail_inferred: tail_inferred.and_then(Json::as_bool),
        source,
    })
}

/// The primer as a `primer_bind` feature on `span` of the fragment.
///
/// Its qualifiers are the ones the feature it came from had. The oligo, its tail and whether the
/// tail is a guess are written into them only where they say something the qualifiers and the
/// footprint do not already say, so that a primer which is just its footprint stays one.
fn primer_feature(primer: &WirePrimer<'_>, span: BaseRange, sequence: &str) -> Feature {
    let source_kind =
        primer.source.and_then(|source| source.get("type")).and_then(Json::as_str).filter(|kind| kind.eq_ignore_ascii_case("primer_bind"));
    let mut feature = Feature {
        name: primer.name.to_string(),
        kind: source_kind.unwrap_or("primer_bind").to_string(),
        start: span.start,
        end: span.end,
        strand: if primer.forward { Strand::Forward } else { Strand::Reverse },
        segments: vec![Segment::new(span.start, span.end)],
        notes: primer.source.map(wire_notes_of).unwrap_or_default(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    let bases = primer.bases.to_uppercase();
    if oligo_of(&feature, sequence).bases != bases {
        set_note(&mut feature.notes, "sequence", vec![bases]);
    }
    if oligo_of(&feature, sequence).tail_length != Some(primer.tail_length) {
        set_note(&mut feature.notes, "bios_primer_tail_length", vec![primer.tail_length.to_string()]);
    }
    if let Some(inferred) = primer.tail_inferred.filter(|inferred| oligo_of(&feature, sequence).tail_inferred != Some(*inferred)) {
        set_note(&mut feature.notes, "bios_primer_tail_inferred", vec![inferred.to_string()]);
    }
    feature
}

/// Checks JSON that may have come from anywhere and takes the fragment out of it. Only what a
/// fragment holds is taken: a document's comments, or anything else beside it, is left behind.
fn read_wire(input: &Json) -> Option<DnaFragment> {
    let is_one = matches!(input.get("version"), Some(Json::Number(n)) if *n == 1.0);
    if input.get("kind").and_then(Json::as_str) != Some(FRAGMENT_KIND) || !is_one {
        return None;
    }
    let sequence = input.get("sequence").and_then(Json::as_str).filter(|sequence| is_dna(sequence))?;
    let (features, primers) = (input.get("features")?.as_list()?, input.get("primers")?.as_list()?);
    let mut fragment = DnaFragment {
        sequence: sequence.to_string(),
        partial_feature_count: input.get("partialFeatureCount")?.count()?,
        omitted_partial_primer_count: input.get("omittedPartialPrimerCount")?.count()?,
        split_primer_count: input.get("splitPrimerCount").map_or(Some(0), Json::count)?,
        ..DnaFragment::default()
    };
    let mut aliases = Aliases(HashSet::new());
    for value in features {
        fragment.features.push(read_feature(value, sequence.len(), &mut aliases)?);
    }
    for value in primers {
        let primer = checked_primer(value, sequence.len(), &mut aliases)?;
        match primer.span {
            Some(span) => fragment.primers.push(primer_feature(&primer, span, sequence)),
            // An oligo with no place, which a construct cannot hold.
            None => fragment.split_primer_count += 1,
        }
    }
    Some(fragment)
}

/// The fragment, if everything in it is something a fragment may hold: DNA, features and primers
/// inside it with places Bios can move, and no id on two annotations. `None` otherwise.
///
/// A fragment is checked by writing it as it would be copied and reading that back, so there is
/// one set of rules for a fragment made here and one that arrived as JSON.
pub fn validate_fragment(fragment: &DnaFragment) -> Option<DnaFragment> {
    if !fragment.primers.iter().all(is_primer) {
        return None;
    }
    read_wire(&wire_fragment(fragment))
}

// ── Ids and what a feature acts on ──────────────────────────────────────────────────────────
//
// Bios keeps its own facts about a feature as `bios-<key>: <value>` lines among its notes. The id
// is one. What the feature acts on is another: `bios-act: <verb> > <id>, <id>`, or the older
// `bios-for: <id>`. Ported from the web app's `identity.ts`.

const NOTE_PREFIX: &str = "bios-";
const ACT_PREFIX: &str = "bios-act:";
/// What a feature that serves the whole construct points at, in place of an id.
const FOR_CONSTRUCT: &str = "construct";

fn note_lines(notes: &Notes) -> &[String] {
    note(notes, "note").unwrap_or_default()
}

fn is_act_line(line: &str) -> bool {
    line.starts_with(ACT_PREFIX)
}

/// The Bios facts among a feature's notes, one value to a key. What it acts on is not one of
/// them: a feature may act on several things, and a map would keep only the last.
fn read_fields(lines: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in lines.iter().filter(|line| !is_act_line(line)) {
        if let Some((key, value)) = line.strip_prefix(NOTE_PREFIX).and_then(|rest| rest.split_once(':')) {
            out.insert(js_trim(key).to_string(), js_trim(value).to_string());
        }
    }
    out
}

fn feature_id(lines: &[String]) -> Option<String> {
    read_fields(lines).remove("id")
}

/// A private copy of what `commands/sequence.rs`'s `new_bios_id` puts after `bios-id: `. That
/// function is not visible from here, and it returns the whole note line.
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn set_note_lines(notes: &mut Notes, lines: Vec<String>) {
    if lines.is_empty() { remove_note(notes, "note") } else { set_note(notes, "note", lines) }
}

/// Sets or removes Bios facts, leaving every note a person wrote where it was.
fn write_fields(notes: &mut Notes, changes: &[(&str, Option<String>)]) {
    let lines = note_lines(notes);
    let mut merged = read_fields(lines);
    for (key, value) in changes {
        match value {
            Some(value) => merged.insert((*key).to_string(), value.clone()),
            None => merged.remove(*key),
        };
    }
    let human = lines.iter().filter(|line| !line.starts_with(NOTE_PREFIX)).cloned();
    let mine = merged.iter().map(|(key, value)| format!("{NOTE_PREFIX}{key}: {value}"));
    let acts = lines.iter().filter(|line| is_act_line(line)).cloned();
    let all: Vec<String> = human.chain(mine).chain(acts).collect();
    set_note_lines(notes, all);
}

/// One thing a feature does: a verb in a person's own words, and what it is done to.
struct Action {
    verb: String,
    targets: Vec<String>,
}

fn clean(action: Action) -> Action {
    let mut targets: Vec<String> = Vec::new();
    for target in action.targets.iter().map(|target| js_trim(target)).filter(|target| !target.is_empty()) {
        if !targets.iter().any(|held| held == target) {
            targets.push(target.to_string());
        }
    }
    Action { verb: action.verb.split(is_js_space).filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" "), targets }
}

fn stated(action: &Action) -> bool {
    !action.verb.is_empty() && !action.targets.is_empty()
}

/// What the feature does, an older `bios-for` read as `serves`.
fn actions_of(lines: &[String]) -> Vec<Action> {
    let acts: Vec<Action> = lines
        .iter()
        .filter_map(|line| line.strip_prefix(ACT_PREFIX))
        // The verb ends at the last `>`: an id never holds one, and a verb may.
        .filter_map(|body| body.rsplit_once('>'))
        .map(|(verb, targets)| clean(Action { verb: verb.to_string(), targets: targets.split(',').map(str::to_string).collect() }))
        .filter(stated)
        .collect();
    if !acts.is_empty() {
        return acts;
    }
    let legacy = read_fields(lines).remove("for").filter(|target| !target.is_empty());
    legacy.map(|target| Action { verb: "serves".to_string(), targets: vec![target] }).into_iter().collect()
}

/// Replaces what the feature does. `bios-for` is kept pointing at the first target, for readers
/// that know only it.
fn write_actions(notes: &mut Notes, actions: Vec<Action>) {
    let kept: Vec<Action> = actions.into_iter().map(clean).filter(stated).collect();
    write_fields(notes, &[("for", kept.first().and_then(|action| action.targets.first().cloned()))]);
    let others = note_lines(notes).iter().filter(|line| !is_act_line(line)).cloned();
    let mine = kept.iter().map(|action| format!("{ACT_PREFIX} {} > {}", action.verb, action.targets.join(", ")));
    let all: Vec<String> = others.chain(mine).collect();
    set_note_lines(notes, all);
}

/// Every annotation of the fragment under a new id, features first and then primers. What one
/// acts on is pointed at the new id of an annotation that came along, kept when it is the whole
/// construct, and dropped when it is something that was not copied.
fn new_records(fragment: &DnaFragment) -> Vec<Feature> {
    let mut records: Vec<(Feature, String)> = fragment.features.iter().chain(&fragment.primers).map(|f| (f.clone(), new_id())).collect();
    let mut targets: HashMap<String, String> = HashMap::new();
    for (feature, id) in &records {
        if let Some(old) = feature_id(note_lines(&feature.notes)).filter(|old| !old.is_empty()) {
            targets.insert(old, id.clone());
        }
    }
    for (feature, id) in &mut records {
        let mut actions = actions_of(note_lines(&feature.notes));
        for action in &mut actions {
            action.targets =
                action.targets.iter().filter_map(|old| if old == FOR_CONSTRUCT { Some(old.clone()) } else { targets.get(old).cloned() }).collect();
        }
        write_fields(&mut feature.notes, &[("id", Some(id.clone()))]);
        write_actions(&mut feature.notes, actions);
        // The id the web app knows a primer by, on a primer that came from one of its files.
        if note(&feature.notes, "bios_primer_id").is_some() {
            set_note(&mut feature.notes, "bios_primer_id", vec![id.clone()]);
        }
    }
    records.into_iter().map(|(feature, _)| feature).collect()
}

// ── Pasting ─────────────────────────────────────────────────────────────────────────────────

/// The feature `offset` bases further along. `None` if a position cannot be held.
fn shifted(mut feature: Feature, offset: usize) -> Option<Feature> {
    feature.start = feature.start.checked_add(offset)?;
    feature.end = feature.end.checked_add(offset)?;
    for piece in &mut feature.segments {
        piece.start = piece.start.checked_add(offset)?;
        piece.end = piece.end.checked_add(offset)?;
    }
    Some(feature)
}

/// Pastes a fragment at a caret, or over a selected range. The bases go in exactly as typing
/// them would put them, so the features already there move, grow, shrink or go as they do for
/// typing. The fragment's annotations are added after them, features first and then primers,
/// each under a new id.
///
/// A range that runs through the origin of a circular construct is replaced at position 0, so
/// the fragment begins the sequence.
pub fn insert_fragment(data: &Construct, at: Where, fragment: &DnaFragment) -> Result<Construct, FragmentError> {
    assert_editable_locations(data)?;
    let fragment = validate_fragment(fragment).ok_or(FragmentError::InvalidFragment)?;
    if !data.sequence.is_empty() && !is_dna(&data.sequence) {
        return Err(FragmentError::DestinationNotDna);
    }
    let inside = match at {
        Where::Caret(gap) => gap <= data.len(),
        Where::Range(range) => in_span(range, data.len(), data.circular),
    };
    if !inside {
        return Err(FragmentError::Position);
    }
    let mut next = insert_bases(data, Some(at), &fragment.sequence)?;
    let offset = caret_after_insert(at, 0);
    for record in new_records(&fragment) {
        next.features.push(shifted(record, offset).ok_or(FragmentError::Position)?);
    }
    Ok(next)
}

// ── What a copy holds ───────────────────────────────────────────────────────────────────────

/// What a copy puts on the clipboard: the bases as plain text, which every other program can
/// paste, and the whole fragment as JSON under [`DNA_CLIPBOARD_TYPE`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentClipboardData {
    pub plain: String,
    pub json: String,
}

/// Why a fragment cannot be copied. Each reads as the web app words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardError {
    Invalid,
    TooLarge,
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::Invalid => write!(f, "The selected DNA fragment could not be copied."),
            ClipboardError::TooLarge => write!(f, "This annotated selection is too large to copy. Export the construct instead."),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// What to put on the clipboard for a copy of `fragment`. The web app also writes HTML that holds
/// the same JSON, which is how a fragment gets from one browser tab to another; that is not
/// written here.
pub fn fragment_clipboard_data(fragment: &DnaFragment) -> Result<FragmentClipboardData, ClipboardError> {
    let checked = validate_fragment(fragment).ok_or(ClipboardError::Invalid)?;
    let json = serde_json::to_string(&wire_fragment(&checked)).map_err(|_| ClipboardError::Invalid)?;
    if too_long(&json) {
        return Err(ClipboardError::TooLarge);
    }
    Ok(FragmentClipboardData { plain: checked.sequence, json })
}

/// What a paste found on the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentClipboard {
    pub fragment: Option<DnaFragment>,
    /// There was a fragment and it cannot be used. With no fragment and this false, the plain
    /// text is pasted as plain text.
    pub invalid: bool,
}

/// True when the plain text on the clipboard is still the fragment's bases, white space and
/// letter case aside.
pub fn fragment_matches_plain(fragment: &DnaFragment, plain: &str) -> bool {
    let bases: String = plain.chars().filter(|&c| !is_js_space(c)).collect();
    bases.to_uppercase() == fragment.sequence.to_uppercase()
}

/// Reads a paste. `plain` is the clipboard's plain text and `json` what it holds under
/// [`DNA_CLIPBOARD_TYPE`], each empty when there is none.
///
/// A clipboard manager, or a copy made in another program since, may have changed the plain text
/// and left the fragment. So a fragment is used only while the plain text is still its bases:
/// annotations must never be attached to bases they were not copied with. With no plain text at
/// all there is nothing to disagree with, and the fragment is used.
pub fn read_fragment_clipboard(plain: &str, json: &str) -> FragmentClipboard {
    if json.is_empty() {
        return FragmentClipboard { fragment: None, invalid: false };
    }
    match DnaFragment::from_json(json).filter(|fragment| plain.is_empty() || fragment_matches_plain(fragment, plain)) {
        Some(fragment) => FragmentClipboard { fragment: Some(fragment), invalid: false },
        None => FragmentClipboard { fragment: None, invalid: true },
    }
}

/// What to tell a person about a copy that did not carry everything as it was. `None` when it
/// did.
pub fn fragment_copy_notice(fragment: &DnaFragment) -> Option<String> {
    let mut notes: Vec<String> = Vec::new();
    let partial = fragment.partial_feature_count;
    if partial > 0 {
        notes.push(format!("{partial} feature{} clipped to the selection and marked partial.", if partial == 1 { "" } else { "s" }));
    }
    let omitted = fragment.omitted_partial_primer_count;
    if omitted > 0 {
        notes.push(format!(
            "{omitted} partly selected primer{} omitted; select the full binding region to include {}.",
            if omitted == 1 { "" } else { "s" },
            if omitted == 1 { "it" } else { "them" }
        ));
    }
    let split = fragment.split_primer_count;
    if split > 0 {
        // The web app keeps these as oligos with no place and says "retained without a binding
        // site". Here they are left out, and the notice says so.
        notes.push(format!(
            "{split} primer{} left out because the copied fragment cuts through {} circular binding region.",
            if split == 1 { "" } else { "s" },
            if split == 1 { "its" } else { "their" }
        ));
    }
    if notes.is_empty() { None } else { Some(notes.join(" ")) }
}
