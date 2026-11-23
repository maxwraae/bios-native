//! Edits that keep features in step with the sequence. Ported from the Bios web app.
//!
//! Positions in the public functions are 1-based inclusive (GenBank, and what an agent speaks);
//! features carry 0-based inclusive coordinates. The conversion lives only here, so the rest of
//! the system never converts ad hoc. See [`crate::parts`] for the coordinate contract these
//! functions are built on.

use crate::coding_edit::{EditWindow, coding_after_edit};
use crate::locations::{normalized_feature_locations, rotate_feature_location, simple_segment, unsupported_location};
use crate::model::{Feature, Segment, Strand};
use crate::parts::{Part, cut, feature_to_parts, span_to_parts};
use crate::seq::clean_seq;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// An insertion point outside `0..=length + 1`.
    InsertPosition,
    /// A range end outside the sequence, or a wrapping range on a linear construct.
    Range,
    /// The feature's location is one Bios cannot interpret, so it cannot be moved.
    UnsupportedLocation {
        name: String,
        location: String,
    },
    /// A piece of a joined location lies outside the sequence or runs backwards on a linear one.
    InvalidSegments {
        name: String,
    },
    RotateUnsupported,
    /// The sequence holds a character that is not one byte long. Positions count bytes, so
    /// cutting such a sequence could land inside a character.
    NotAscii,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::InsertPosition => write!(f, "DNA insertion position must be inside the sequence or immediately after its last base."),
            EditError::Range => write!(f, "DNA edit coordinates must be inside the sequence; wrapping ranges require circular DNA."),
            EditError::UnsupportedLocation { name, location } => write!(f, "Cannot edit DNA with the unsupported location of {name}: {location}"),
            EditError::InvalidSegments { name } => write!(f, "Invalid feature segments for {name}"),
            EditError::RotateUnsupported => write!(f, "Cannot rotate an unsupported feature location."),
            EditError::NotAscii => write!(f, "DNA sequence must hold only single-byte characters."),
        }
    }
}

impl std::error::Error for EditError {}

/// A feature the edit cut into rather than moved or destroyed.
///
/// `removed` says the annotation is gone and `shifted` says it survived intact somewhere else.
/// Neither is true of a CDS whose 5' half is still in the product: the map has to name it, and
/// say that it is a fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TruncatedFeature {
    pub before: Feature,
    pub after: Feature,
    /// Original annotated bases removed by the edit, independent of inserted sequence length.
    pub lost_bases: usize,
    /// Replacement bases connect surviving pieces of the same interval. Separate joined segments
    /// remain separate, and pure insertions do not imply truncation.
    pub bridged: bool,
}

/// What an edit did to the feature set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureDelta {
    pub added: Vec<Feature>,
    pub removed: Vec<Feature>,
    /// Features whose coordinates or covered length changed, as they are after the edit.
    pub shifted: Vec<Feature>,
    pub truncated: Vec<TruncatedFeature>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditResult {
    pub sequence: String,
    pub features: Vec<Feature>,
    pub delta: FeatureDelta,
}

/// The feature [`apply_annotate`] adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    pub kind: String,
    pub strand: Strand,
    pub notes: Vec<(String, Vec<String>)>,
}

/// The two ends of the sequence an edit maps features between.
struct Axis<'a> {
    windows: &'a [EditWindow],
    old_len: usize,
    new_len: usize,
    circular: bool,
}

/// One feature after an edit that left some of it standing.
struct Remapped {
    feature: Feature,
    shifted: bool,
    /// Bases lost and whether new bases bridge the survivors, when any were lost.
    truncated: Option<(usize, bool)>,
}

/// `sequence[from..to]`, empty when the range is empty or backwards.
fn slice(sequence: &str, from: usize, to: usize) -> &str {
    sequence.get(from..to).unwrap_or_default()
}

/// Positions count bytes, which are bases only while every character is one byte. Without this a
/// cut could fall inside a character.
fn require_ascii(sequence: &str) -> Result<(), EditError> {
    if sequence.is_ascii() { Ok(()) } else { Err(EditError::NotAscii) }
}

/// Total bases covered by a set of half-open parts. Signed because a part that is not a real
/// interval counts as negative.
fn parts_span(parts: &[Part]) -> i128 {
    parts.iter().map(|&(s, e)| e as i128 - s as i128).sum()
}

/// Did the feature's coordinates change?
fn moved(a: &Feature, s: usize, e: usize) -> bool {
    a.start != s || a.end != e
}

/// Windows on the old axis. A wrapped replacement starts the new sequence, as in the editor.
/// Equal half-open endpoints mean a whole-circle inclusive selection, not an insertion.
fn windows_for(edit_start: usize, edit_end: usize, replacement_len: usize, old_len: usize, circular: bool) -> Vec<EditWindow> {
    if circular && edit_end <= edit_start {
        return vec![EditWindow { ws: edit_start, we: old_len, wl: 0 }, EditWindow { ws: 0, we: edit_end, wl: replacement_len }];
    }
    vec![EditWindow { ws: edit_start, we: edit_end, wl: replacement_len }]
}

/// The 1-based inclusive range as given, once both ends are known to be inside the sequence.
fn validate_range(length: usize, from: i64, to: i64, circular: bool) -> Result<(usize, usize), EditError> {
    let inside = |p: i64| usize::try_from(p).ok().filter(|p| (1..=length).contains(p));
    match (inside(from), inside(to)) {
        (Some(from), Some(to)) if circular || to >= from => Ok((from, to)),
        _ => Err(EditError::Range),
    }
}

/// Maps a 0-based coordinate in the old sequence to its new home. A coordinate inside a deleted
/// window has no new home and comes back unmoved; callers cut those away first.
fn transform_coord(c: usize, windows: &[EditWindow]) -> usize {
    let mut coord = c;
    for w in windows {
        if w.ws == w.we {
            // Pure insertion before ws: everything from ws onward slides right.
            if c >= w.ws {
                coord = coord.saturating_add(w.wl);
            }
        } else if c >= w.we {
            coord = coord.saturating_add(w.wl).saturating_sub(w.we.saturating_sub(w.ws));
        }
    }
    coord
}

/// Split only at deleted intervals, retaining feature order. Work scales with edits, not bases.
fn apply_part(part: Part, windows: &[EditWindow]) -> Vec<Part> {
    let mut pieces = vec![part];
    for w in windows {
        if w.ws != w.we {
            pieces = cut(&pieces, w.ws, w.we);
        }
    }
    pieces
        .into_iter()
        .filter(|&(s, e)| e > s)
        .map(|(s, e)| (transform_coord(s, windows), transform_coord(e - 1, windows).saturating_add(1)))
        .collect()
}

/// First and last base of what survived, 0-based inclusive, or `None` if nothing did.
///
/// Numeric sorting would lose which arc belongs to an origin-spanning annotation. This keeps its
/// first and last surviving bases even when inserted bases separate its arms at the new origin.
fn parts_bounds(parts: &[Part]) -> Option<(usize, usize)> {
    Some((parts.first()?.0, parts.last()?.1.saturating_sub(1)))
}

fn removed_bases(parts: &[Part], windows: &[EditWindow]) -> usize {
    parts.iter().map(|&(s, e)| windows.iter().map(|w| e.min(w.we).saturating_sub(s.max(w.ws))).sum::<usize>()).sum()
}

/// The feature as one simple span over `start..=end`.
fn with_span(f: &Feature, start: usize, end: usize) -> Feature {
    let mut out = f.clone();
    out.start = start;
    out.end = end;
    out.segments = vec![simple_segment(&out)];
    out
}

fn refuse_unsupported(f: &Feature) -> Result<(), EditError> {
    match unsupported_location(f) {
        Some(location) => Err(EditError::UnsupportedLocation { name: f.name.clone(), location: location.to_string() }),
        None => Ok(()),
    }
}

/// Maps each original segment independently. An insertion in an intron must not enlarge either
/// exon, and adjacent surviving exons retain their separate identities.
fn remap_joined(f: &Feature, locations: &[Segment], old_parts: &[Part], axis: &Axis) -> Result<Option<Remapped>, EditError> {
    let mut mapped: Vec<Segment> = Vec::with_capacity(locations.len());
    let mut survived = 0i128;
    for location in locations {
        let parts: Vec<Part> =
            span_to_parts(location.start, location.end, axis.old_len, axis.circular).into_iter().flat_map(|p| apply_part(p, axis.windows)).collect();
        survived += parts_span(&parts);
        if let Some((start, end)) = parts_bounds(&parts) {
            mapped.push(Segment { start, end, ..*location });
        }
    }
    let (Some(first), Some(last)) = (mapped.first().copied(), mapped.last().copied()) else {
        return Ok(None);
    };
    let shifted = locations != mapped.as_slice();
    let mut nf = f.clone();
    nf.start = first.start;
    nf.end = last.end;
    // One surviving piece is a simple span, which carries its uncertain ends itself.
    let single = mapped.len() == 1;
    nf.start_fuzzy = first.start_fuzzy.filter(|_| single);
    nf.end_fuzzy = first.end_fuzzy.filter(|_| single);
    nf.segments = mapped;
    let lost_bases = removed_bases(old_parts, axis.windows);
    let bridged = lost_bases > 0 && parts_span(&feature_to_parts(&nf, axis.new_len, axis.circular)?) > survived;
    Ok(Some(Remapped { feature: nf, shifted, truncated: (lost_bases > 0).then_some((lost_bases, bridged)) }))
}

fn remap_simple(f: &Feature, old_parts: &[Part], axis: &Axis) -> Option<Remapped> {
    let parts: Vec<Part> = old_parts.iter().flat_map(|&p| apply_part(p, axis.windows)).collect();
    let (start, end) = parts_bounds(&parts)?;
    let nf = with_span(f, start, end);
    let new_span = parts_span(&span_to_parts(start, end, axis.new_len, axis.circular));
    let shifted = moved(f, start, end) || new_span != parts_span(old_parts);
    let lost_bases = removed_bases(old_parts, axis.windows);
    // Insertions grow an interval without implying truncation or negative loss. A replacement
    // can bridge its surviving flanks with new bases; report the actual old bases removed.
    let truncated = (lost_bases > 0).then_some((lost_bases, new_span > parts_span(&parts)));
    Some(Remapped { feature: nf, shifted, truncated })
}

/// Remaps every feature through the edit windows, recording what shifted and what was lost.
fn remap_features(features: &[Feature], axis: &Axis) -> Result<(Vec<Feature>, FeatureDelta), EditError> {
    let mut delta = FeatureDelta::default();
    let mut out: Vec<Feature> = Vec::with_capacity(features.len());
    for original in features {
        // A file's exact two-arm origin join represents one contiguous interval, not two exons.
        let two_arm_join = original.segments.len() == 2 && normalized_feature_locations(original, axis.old_len, axis.circular).is_none();
        let simple =
            if two_arm_join || original.locations().is_none() { with_span(original, original.start, original.end) } else { original.clone() };
        let f = coding_after_edit(&simple, axis.old_len, axis.circular, axis.windows, false)?;
        refuse_unsupported(&f)?;
        let old_parts = feature_to_parts(&f, axis.old_len, axis.circular)?;
        let remapped = match f.locations() {
            Some(locations) => remap_joined(&f, locations, &old_parts, axis)?,
            None => remap_simple(&f, &old_parts, axis),
        };
        let Some(Remapped { feature: nf, shifted, truncated }) = remapped else {
            delta.removed.push(f);
            continue;
        };
        out.push(nf.clone());
        if shifted {
            delta.shifted.push(nf.clone());
        }
        if let Some((lost_bases, bridged)) = truncated {
            delta.truncated.push(TruncatedFeature { before: f, after: nf, lost_bases, bridged });
        }
    }
    Ok((out, delta))
}

/// Inserts `ins_seq` immediately before 1-based position `at` (so `at = length + 1` appends).
/// On a circular construct `at = 0` is shorthand for "at the origin". Features at or after the
/// insertion point shift right; none are dropped.
pub fn apply_insert(sequence: &str, features: &[Feature], at: i64, ins_seq: &str, circular: bool) -> Result<EditResult, EditError> {
    let at0 = match usize::try_from(at) {
        Ok(at) if at <= sequence.len().saturating_add(1) => at.saturating_sub(1),
        _ => return Err(EditError::InsertPosition),
    };
    require_ascii(sequence)?;
    let ins = clean_seq(ins_seq);
    let sequence_out = [slice(sequence, 0, at0), ins.as_str(), slice(sequence, at0, sequence.len())].concat();
    let windows = [EditWindow { ws: at0, we: at0, wl: ins.len() }];
    let axis = Axis { windows: &windows, old_len: sequence.len(), new_len: sequence_out.len(), circular };
    let (features, delta) = remap_features(features, &axis)?;
    Ok(EditResult { sequence: sequence_out, features, delta })
}

/// Deletes the 1-based inclusive range `from..=to`. On a circular construct `from > to` means the
/// range wraps through the origin, and both arms go. Features wholly consumed are dropped;
/// origin-spanning features are preserved.
pub fn apply_delete(sequence: &str, features: &[Feature], from: i64, to: i64, circular: bool) -> Result<EditResult, EditError> {
    let (from, to) = validate_range(sequence.len(), from, to, circular)?;
    require_ascii(sequence)?;
    let edit_start = from - 1;
    let edit_end = to; // half-open end of the 1-based inclusive range
    let wrap = circular && to < from;
    let sequence_out = if wrap {
        slice(sequence, edit_end, edit_start).to_string()
    } else {
        [slice(sequence, 0, edit_start), slice(sequence, edit_end, sequence.len())].concat()
    };
    let windows = windows_for(edit_start, edit_end, 0, sequence.len(), circular);
    let axis = Axis { windows: &windows, old_len: sequence.len(), new_len: sequence_out.len(), circular };
    let (features, delta) = remap_features(features, &axis)?;
    Ok(EditResult { sequence: sequence_out, features, delta })
}

/// Replaces the 1-based inclusive range `from..=to` with `new_seq`. As with delete, `from > to`
/// on a circular construct wraps the origin; the replacement starts the resulting sequence,
/// followed by the surviving arc, using the same origin convention as editing in the window.
pub fn apply_replace(sequence: &str, features: &[Feature], from: i64, to: i64, new_seq: &str, circular: bool) -> Result<EditResult, EditError> {
    let (from, to) = validate_range(sequence.len(), from, to, circular)?;
    require_ascii(sequence)?;
    let replacement = clean_seq(new_seq);
    let edit_start = from - 1;
    let edit_end = to;
    let wrap = circular && to < from;
    let sequence_out = if wrap {
        [replacement.as_str(), slice(sequence, edit_end, edit_start)].concat()
    } else {
        [slice(sequence, 0, edit_start), replacement.as_str(), slice(sequence, edit_end, sequence.len())].concat()
    };
    let windows = windows_for(edit_start, edit_end, replacement.len(), sequence.len(), circular);
    if sequence_out.len() == sequence.len() {
        // Substitution changes bases, not annotation identity or segment boundaries. This also
        // keeps independently saved primer sites available for current-template comparison.
        let offset = if wrap { sequence.len() - edit_start } else { 0 };
        let (features, shifted) = substitute(features, &windows, sequence.len(), circular, offset)?;
        return Ok(EditResult { sequence: sequence_out, features, delta: FeatureDelta { shifted, ..FeatureDelta::default() } });
    }
    let axis = Axis { windows: &windows, old_len: sequence.len(), new_len: sequence_out.len(), circular };
    let (features, delta) = remap_features(features, &axis)?;
    Ok(EditResult { sequence: sequence_out, features, delta })
}

/// Every feature after a same-length replacement that moved the origin `offset` bases, and those
/// of them whose coordinates changed.
fn substitute(
    features: &[Feature],
    windows: &[EditWindow],
    length: usize,
    circular: bool,
    offset: usize,
) -> Result<(Vec<Feature>, Vec<Feature>), EditError> {
    let turn = i64::try_from(offset).unwrap_or(i64::MAX);
    let mut out: Vec<Feature> = Vec::with_capacity(features.len());
    let mut shifted: Vec<Feature> = Vec::new();
    for original in features {
        refuse_unsupported(original)?;
        let mut f = rotate_feature_location(&coding_after_edit(original, length, circular, windows, true)?, length, turn)?;
        if f.locations().is_none() {
            f.segments = vec![simple_segment(&f)];
        }
        if offset != 0 && (moved(original, f.start, f.end) || f.locations() != original.locations()) {
            shifted.push(f.clone());
        }
        out.push(f);
    }
    Ok((out, shifted))
}

/// Adds a feature over the 1-based inclusive range `from..=to`. No sequence change. `from > to`
/// on a circular construct yields an origin-spanning feature (`end < start`).
///
/// The range is not checked against the sequence. A position below 1 is refused only because a
/// feature cannot hold a coordinate before the first base.
pub fn apply_annotate(sequence: &str, features: &[Feature], from: i64, to: i64, annotation: Annotation) -> Result<EditResult, EditError> {
    let zero_based = |p: i64| p.checked_sub(1).and_then(|p| usize::try_from(p).ok()).ok_or(EditError::Range);
    let (start, end) = (zero_based(from)?, zero_based(to)?);
    let nf = Feature {
        name: annotation.name,
        kind: annotation.kind,
        start,
        end,
        strand: annotation.strand,
        segments: vec![Segment::new(start, end)],
        // The web app gives a feature with no qualifiers a `label` so that its name reaches the
        // file. Here the name is the label: the reader takes it out of the notes and the writer
        // puts it back, so a second copy in the notes would be written twice.
        notes: annotation.notes,
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    let mut delta = FeatureDelta::default();
    delta.added.push(nf.clone());
    let mut all = features.to_vec();
    all.push(nf);
    Ok(EditResult { sequence: sequence.to_string(), features: all, delta })
}

#[cfg(test)]
mod tests {
    use super::{Annotation, EditError, apply_annotate, apply_delete, apply_insert, apply_replace};
    use crate::model::{Segment, Strand};

    fn annotation(name: &str) -> Annotation {
        Annotation { name: name.to_string(), kind: "misc_feature".to_string(), strand: Strand::Reverse, notes: Vec::new() }
    }

    #[test]
    fn annotate_adds_a_feature_and_leaves_the_sequence_alone() {
        let r = apply_annotate("ACGTACGT", &[], 3, 6, annotation("site")).unwrap();
        assert_eq!(r.sequence, "ACGTACGT");
        let f = &r.features[0];
        assert_eq!((f.name.as_str(), f.start, f.end, f.strand), ("site", 2, 5, Strand::Reverse));
        assert_eq!(f.segments, vec![Segment::new(2, 5)]);
        assert_eq!(r.delta.added, r.features);

        let through_origin = apply_annotate("ACGTACGT", &r.features, 7, 2, annotation("wrap")).unwrap();
        assert_eq!(through_origin.features.len(), 2);
        assert_eq!((through_origin.features[1].start, through_origin.features[1].end), (6, 1));
    }

    #[test]
    fn annotate_refuses_a_position_before_the_first_base() {
        assert_eq!(apply_annotate("ACGT", &[], 0, 2, annotation("x")), Err(EditError::Range));
        assert_eq!(apply_annotate("ACGT", &[], 1, -3, annotation("x")), Err(EditError::Range));
    }

    #[test]
    fn a_sequence_with_a_wide_character_is_refused_rather_than_cut_inside_it() {
        assert_eq!(apply_insert("AC\u{e9}T", &[], 4, "A", false), Err(EditError::NotAscii));
        assert_eq!(apply_delete("AC\u{e9}T", &[], 4, 4, false), Err(EditError::NotAscii));
        assert_eq!(apply_replace("AC\u{e9}T", &[], 4, 4, "G", false), Err(EditError::NotAscii));
    }

    #[test]
    fn errors_read_as_the_web_app_words_them() {
        assert_eq!(EditError::InsertPosition.to_string(), "DNA insertion position must be inside the sequence or immediately after its last base.");
        assert_eq!(EditError::Range.to_string(), "DNA edit coordinates must be inside the sequence; wrapping ranges require circular DNA.");
        assert_eq!(
            EditError::UnsupportedLocation { name: "exon".to_string(), location: "join(2..5,ACC.1:12..15)".to_string() }.to_string(),
            "Cannot edit DNA with the unsupported location of exon: join(2..5,ACC.1:12..15)"
        );
        assert_eq!(EditError::InvalidSegments { name: "exon".to_string() }.to_string(), "Invalid feature segments for exon");
        assert_eq!(EditError::RotateUnsupported.to_string(), "Cannot rotate an unsupported feature location.");
    }
}
