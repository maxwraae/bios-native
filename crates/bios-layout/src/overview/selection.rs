//! Which arc the map should light: the pure half of "click a feature and it lights up".
//! Ported from Bios `editor/overview/selection.ts`.
//!
//! The map and the sequence panel never share an object. The map draws FUSED arcs
//! (`coalesce_features` turns a CDS split into three pieces back into the one gene it is), and the
//! editor holds its own annotations. So "is this arc the thing the editor has selected?" cannot be
//! reference equality; it has to be a decision about values, and this file is that decision.

use super::text_fit::is_js_whitespace;

/// Every Bios note entry looks like `bios-<key>: <value>`.
const NOTE_PREFIX: &str = "bios-";
/// `bios-act: …` is the one repeated key, and is never read as a field.
const ACT_PREFIX: &str = "bios-act:";
const FEATURE_ID: &str = "id";

/// A feature as the map holds it: the fused arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcLike<'a> {
    pub start: i64,
    pub end: i64,
    pub name: Option<&'a str>,
    /// The feature's `/note` lines (`notes.note` in the web app), which is where its id rides.
    pub note: &'a [String],
}

/// The annotation the editor considers selected, in the only terms the map can match on.
///
/// `id` is the Bios feature id when the file carries one (a file that has been through Bios gives
/// every feature a stable one); the name and span are the fallback for a file that has not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapSelection<'a> {
    pub id: Option<&'a str>,
    pub name: &'a str,
    pub start: i64,
    pub end: i64,
}

/// The Bios id carried in a feature's `/note` lines, as `bios-id: <value>` (`featureId` in the web
/// app's `identity.ts`). Human notes are ignored, and the last id line wins.
pub fn feature_id<S: AsRef<str>>(note: &[S]) -> Option<&str> {
    let mut id = None;
    for line in note {
        let line = line.as_ref();
        if !line.starts_with(NOTE_PREFIX) || line.starts_with(ACT_PREFIX) {
            continue;
        }
        let Some((head, value)) = line.split_once(':') else { continue };
        if head.strip_prefix(NOTE_PREFIX).is_some_and(|key| key.trim_matches(is_js_whitespace) == FEATURE_ID) {
            id = Some(value.trim_matches(is_js_whitespace));
        }
    }
    id
}

/// Does this arc draw the selected annotation?
///
/// Identity first, when both sides have one: that is exact and survives anything.
///
/// Otherwise name plus containment, and the containment is doing real work: a fused arc IS the pieces
/// of one annotation drawn as the one thing they are, so clicking piece two of a split EGFP down in
/// the sequence has to light the whole EGFP on the map. Requiring an exact span match would light
/// nothing at all in exactly the case the map goes furthest out of its way to handle.
///
/// A span that wraps the origin (`end < start`) can't be compared by containment (the arithmetic
/// needs the molecule's length, which this doesn't have and shouldn't need), so it falls back to an
/// exact match. Wrapping features are rare and a wrong light is worse than no light.
pub fn draws_selection(arc: &ArcLike<'_>, selection: &MapSelection<'_>) -> bool {
    if let (Some(id), Some(selected)) = (feature_id(arc.note), selection.id)
        && !id.is_empty()
        && !selected.is_empty()
    {
        return id == selected;
    }
    if arc.name.unwrap_or("") != selection.name {
        return false;
    }
    if arc.end < arc.start || selection.end < selection.start {
        return arc.start == selection.start && arc.end == selection.end;
    }
    arc.start <= selection.start && arc.end >= selection.end
}

/// Which end of a dragged range the pointer finished on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishedAt {
    Start,
    End,
}

/// What a drag on the map selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragSelection {
    Caret(i64),
    Range { start: i64, end: i64, finished_at: FinishedAt },
}

/// `((n % l) + l) % l`. A molecule of no length has no positions; the web app answers NaN, this 0.
fn wrap(n: i64, l: i64) -> i64 {
    if l == 0 { 0 } else { n.wrapping_rem(l).wrapping_add(l).wrapping_rem(l) }
}

/// What a drag on the map from gap `anchor` to gap `to` selects.
///
/// Both are GAP positions, the way the rows count them, so the base right of the anchor is in and
/// the base right of the pointer is not. On the ring `to` is UNWRAPPED (it keeps counting past `l` or
/// below 0 as the hand goes round), which is what lets a drag across the origin select across it
/// (`end < start`, the wrapped shape) instead of flipping to the long way round. A drag of a full
/// turn or more is the whole molecule. Back on the anchor it is a caret, not an empty range.
pub fn drag_selection(anchor: i64, to: i64, l: i64) -> DragSelection {
    let lo = anchor.min(to);
    let hi = anchor.max(to);
    let finished_at = if to >= anchor { FinishedAt::End } else { FinishedAt::Start };
    if hi == lo {
        return DragSelection::Caret(if lo >= 0 && lo <= l { lo } else { wrap(lo, l) });
    }
    if hi.saturating_sub(lo) >= l {
        return DragSelection::Range { start: 0, end: l.saturating_sub(1), finished_at };
    }
    DragSelection::Range { start: wrap(lo, l), end: wrap(hi - 1, l), finished_at }
}

/// Carry an unwrapped ring position on by one pointer step. The pointer reads a position `0..l`; a
/// step that jumps by more than half the molecule went through the origin, not across the ring.
pub fn ring_step(unwrapped: i64, prev: i64, next: i64, l: i64) -> i64 {
    let mut d = next.saturating_sub(prev);
    let half = l as f64 / 2.0;
    if d as f64 > half {
        d = d.saturating_sub(l);
    } else if (d as f64) < -half {
        d = d.saturating_add(l);
    }
    unwrapped.saturating_add(d)
}
