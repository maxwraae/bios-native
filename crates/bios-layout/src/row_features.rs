//! Features and primers in the wrapped rows: which pieces of each land in a row, the lane each
//! takes, what the stack costs and the outline of what is drawn. Ported from the geometry in Bios
//! `editor/rows/RowFeatures.tsx` and `editor/featureGeometry.ts`.
//!
//! A feature crossing a row boundary is one feature drawn in several pieces, and a feature crossing
//! the origin is unwrapped first, because a row view reads the sequence straight through.
//!
//! A primer is its footprint, judged column by column; an oligo placed by its alignment (Find's
//! probe) carries its runs and places every base itself, a 5' tail and an insert included
//! (`cellsFromRuns`).

use bios_core::duplex::{AlignRun, AlignRunKind};

use crate::overview::direction::HEAD_PX;
use crate::rows::{Row, span_in_row, track};

/// A feature as the rows need it, already normalised out of whatever shape the file held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowFeature<'a> {
    pub start: i64,
    pub end: i64,
    pub forward: bool,
    /// Whether orientation means anything for it. Only then is its 5' end capped.
    pub directional: bool,
    /// The name as it is shown. Only a primer's is measured here: it prints beside the ribbon.
    pub name: &'a str,
    /// Every piece of a joined location, in stored order. `None` for a simple span.
    pub locations: Option<&'a [(i64, i64)]>,
    /// An oligo's OWN bases, in query order 5'→3'. `&` marks a template column the oligo has no
    /// base for. ASCII; `None` and the ribbon is one bar that says nothing it does not know.
    pub bases: Option<&'a str>,
    /// An aligned oligo's runs (Find's probe). With these the ribbon draws what `bases` cannot say:
    /// a 5' tail off the template, an insert with no column of its own.
    pub runs: Option<&'a [AlignRun]>,
}

/// Which track this is. Only the lane height and how a piece packs differ; the geometry is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Feature,
    Primer,
    Part,
    Orf,
}

pub fn lane_height(kind: Kind) -> f64 {
    match kind {
        Kind::Feature => track::FEATURES,
        Kind::Primer => track::PRIMERS,
        Kind::Part => track::PARTS,
        Kind::Orf => track::ORFS,
    }
}

/// The drawn bar inside its lane; the rest of the lane is clear air between tracks.
pub fn bar_height(kind: Kind) -> f64 {
    lane_height(kind) - 4.0
}

/// How one column of a primer stands against its template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// The oligo's base is what the template wants there.
    Paired,
    /// The oligo has no base for the column.
    Gap,
    /// The oligo's base is not what the template wants.
    Change,
    /// A 5' run that sits over columns it does not anneal to.
    Tail,
}

/// One column of a primer against its template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimerCell {
    pub bp: i64,
    /// The oligo's base on this column, when it is known and there is one.
    pub ch: Option<char>,
    pub kind: CellKind,
}

/// One drawn piece of one feature inside one row.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Which of the features handed to [`pieces_for_row`] this is a piece of.
    pub feature: usize,
    pub x_start: f64,
    pub width: f64,
    pub lane: usize,
    /// Top of this piece's bar, measured from the top of its track's block in the row.
    pub y: f64,
    /// The clipped range this piece covers, in bp: what the base printer walks.
    pub start: i64,
    pub end: i64,
    /// True when this piece runs off that edge and the bar is continuing, not ending.
    pub cut_left: bool,
    pub cut_right: bool,
    /// Whether the 5' end of the whole feature falls in this piece and is to be capped.
    pub show_head: bool,
    /// A dashed join between covered segments, never a filled annotation.
    pub connector: bool,
    /// A primer's columns, worked out once here so packing and drawing agree on which ones lift.
    pub cells: Vec<PrimerCell>,
    /// Bases an aligned oligo carries that the template has no column for.
    pub inserts: Vec<PrimerInsert>,
    /// The columns of an aligned oligo's two real ends: the tail's far end counts, the footprint's
    /// does not. `None` for a primer read off its footprint.
    pub five_at: Option<i64>,
    pub three_at: Option<i64>,
}

/// Bases the oligo carries that the template has no column for, and the junction they open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimerInsert {
    /// The column to the right of the junction; the insert sits on that column's left edge.
    pub edge: i64,
    pub bases: String,
}

/// What one row of an aligned oligo holds, read straight off its runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLayout {
    pub cells: Vec<PrimerCell>,
    pub inserts: Vec<PrimerInsert>,
    /// The columns of the oligo's first and last base, a tail's far end included; -1 when none.
    pub five_at: i64,
    pub three_at: i64,
}

/// Past this many bases an insert says its size; its bases are in the hover.
pub const INSERT_BASE_LIMIT: usize = 12;

/// What an insert prints: its bases while they can be read, its size once they cannot.
pub fn insert_text(bases: &str) -> String {
    if bases.len() <= INSERT_BASE_LIMIT { bases.to_string() } else { format!("{} bp", bases.len()) }
}

/// The oligo's columns in this row from its alignment runs, which already say where every base
/// sits: a match or a mismatch on its column, a tail on the columns before the anchor, a deletion on
/// the template columns it skips, and an insert on the boundary it interrupts (`start_col` on an
/// insertion run is the column the alignment RESUMES at: the same edge either way once it is named
/// by the column to its right).
pub fn cells_from_runs(runs: &[AlignRun], row: Row, len: i64, circular: bool) -> RunLayout {
    let norm = |c: i64| if circular && len > 0 { c.rem_euclid(len) } else { c };
    let on = |c: i64| c >= 0 && c < len;
    let mut cells = Vec::new();
    let mut inserts = Vec::new();
    let (mut five_at, mut three_at) = (-1, -1);
    for run in runs {
        if run.kind == AlignRunKind::Insertion {
            if run.bases.is_empty() {
                continue;
            }
            let edge = norm(if run.step == 1 { run.start_col } else { run.start_col.saturating_add(1) });
            // An insert past the last base belongs on the right edge of the last row.
            if edge >= row.start && (edge <= row.end || (edge == len && row.end == len - 1)) {
                inserts.push(PrimerInsert { edge, bases: run.bases.clone() });
            }
            continue;
        }
        let bases = run.bases.as_bytes();
        let span = run.length.max(run.template_length);
        for k in 0..span {
            let col = norm(run.start_col.saturating_add(i64::try_from(k).unwrap_or(i64::MAX).saturating_mul(run.step)));
            if !on(col) {
                continue;
            }
            if run.length > 0 {
                if five_at < 0 {
                    five_at = col;
                }
                three_at = col;
            }
            if col < row.start || col > row.end {
                continue;
            }
            let ch = if run.kind == AlignRunKind::Deletion { None } else { bases.get(k).map(|&b| char::from(b)) };
            let kind = match run.kind {
                AlignRunKind::Match => CellKind::Paired,
                AlignRunKind::Mismatch => CellKind::Change,
                AlignRunKind::Overhang => CellKind::Tail,
                AlignRunKind::Deletion | AlignRunKind::Insertion => CellKind::Gap,
            };
            cells.push(PrimerCell { bp: col, ch, kind });
        }
    }
    cells.sort_by_key(|c| c.bp);
    RunLayout { cells, inserts, five_at, three_at }
}

/// What one track holds in one row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowPieces {
    pub pieces: Vec<Piece>,
    pub lanes: usize,
    /// What the block costs in this row: the lanes plus the outward room their hanging parts need.
    pub height: f64,
    /// That outward room, in pixels, so `row_height` can pay for it.
    pub lift: f64,
}

/// Roughly how wide a name prints inside a bar, so a name that cannot fit is not drawn.
pub const NAME_CHAR_PX: f64 = 6.4;
/// Depth of every chevron on a primer.
pub const CHEV: f64 = 4.0;
/// The ribbon inside its lane, one pixel of air either side.
pub const PRIMER_BAR: f64 = track::PRIMERS - 2.0;
/// Average advance of a name character at 10px 600 in the UI font. An over-estimate, on purpose.
pub const PRIMER_NAME_PX: f64 = 6.2;
/// Air between the 5' end and the name beside it.
pub const NAME_GAP: f64 = 9.0;
/// Air between two bars packed into one lane.
pub const PACK_GAP: f64 = 4.0;
/// A hairline off each end of a bar, so two features that meet are two features. Not taken off an
/// end that was cut by the row's edge: there the bar is continuing, and a gap would claim it stopped.
pub const NIB: f64 = 0.75;
/// Baseline of an 11px letter centred in the ribbon, down from the ribbon's top.
pub const LETTER_BASELINE: f64 = PRIMER_BAR - 4.0;

fn utf16_len(s: &str) -> f64 {
    s.encode_utf16().count() as f64
}

fn pair_of(ch: u8) -> u8 {
    match ch {
        b'a' => b't',
        b't' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'A' => b'T',
        b'T' => b'A',
        b'C' => b'G',
        b'G' => b'C',
        other => other,
    }
}

/// Index into an oligo's bases for plus-strand column `bp`, wrapping an origin-spanning footprint.
fn query_index(f: &RowFeature, bp: i64, len: i64) -> i64 {
    let raw = if f.forward { bp.saturating_sub(f.start) } else { f.end.saturating_sub(bp) };
    if len > 0 { raw.rem_euclid(len) } else { raw }
}

/// The oligo's columns across `from..=to`, judged against the template.
///
/// A forward oligo reads the same way as the top strand, so its base at a column IS the template's.
/// A reverse one is the reverse complement, so its query runs backwards through the columns and
/// pairs when it equals the COMPLEMENT of the template there. Doing both in one place is the only
/// way the two directions cannot drift apart. Without `bases` every column is paired.
pub fn primer_cells(f: &RowFeature, seq: Option<&str>, from: i64, to: i64) -> Vec<PrimerCell> {
    let known = f.bases.filter(|b| !b.is_empty()).zip(seq.filter(|s| !s.is_empty()));
    let at = |text: &str, index: i64| usize::try_from(index).ok().and_then(|i| text.as_bytes().get(i).copied());
    (from..=to)
        .map(|bp| {
            let Some((bases, seq)) = known else {
                return PrimerCell { bp, ch: None, kind: CellKind::Paired };
            };
            let ch = at(bases, query_index(f, bp, i64::try_from(seq.len()).unwrap_or(i64::MAX)));
            if ch == Some(b'&') {
                return PrimerCell { bp, ch: None, kind: CellKind::Gap };
            }
            let template = at(seq, bp);
            let want = if f.forward { template } else { template.map(pair_of) };
            let paired = ch.is_none_or(|c| want.is_some_and(|w| c.eq_ignore_ascii_case(&w)));
            PrimerCell { bp, ch: ch.map(char::from), kind: if paired { CellKind::Paired } else { CellKind::Change } }
        })
        .collect()
}

/// Consecutive cells of one kind, as the ribbon draws them. `from` and `to` are columns, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimerRun {
    pub kind: CellKind,
    pub from: i64,
    pub to: i64,
}

pub fn runs_of(cells: &[PrimerCell]) -> Vec<PrimerRun> {
    let mut runs: Vec<PrimerRun> = Vec::new();
    for c in cells {
        match runs.last_mut() {
            Some(last) if last.kind == c.kind && c.bp == last.to + 1 => last.to = c.bp,
            _ => runs.push(PrimerRun { kind: c.kind, from: c.bp, to: c.bp }),
        }
    }
    runs
}

/// How a run of ribbon ends: the 5' chevron cut into it, the 3' chevron pushed out of it, or flat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Notch,
    Point,
    Flat,
}

/// Where a primer piece's two real ends are, if they are in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends {
    pub left: End,
    pub right: End,
    /// The 5' end is in this piece, so the name stands beside it here.
    pub five_here: bool,
}

pub fn ends_of(p: &Piece, f: &RowFeature) -> Ends {
    let (five_at, three_at) = if f.forward { (f.start, f.end) } else { (f.end, f.start) };
    let (five_at, three_at) = (p.five_at.unwrap_or(five_at), p.three_at.unwrap_or(three_at));
    let has = |bp: i64| bp >= p.start && bp <= p.end;
    let flat_unless = |here: bool, end: End| if here { end } else { End::Flat };
    let (left, right) = if f.forward {
        (flat_unless(has(five_at), End::Notch), flat_unless(has(three_at), End::Point))
    } else {
        (flat_unless(has(three_at), End::Point), flat_unless(has(five_at), End::Notch))
    };
    Ends { left, right, five_here: has(five_at) }
}

/// Room the name and the chevrons take beside a primer, so packing keeps two of them apart.
fn primer_padding(p: &Piece, f: &RowFeature) -> (f64, f64) {
    let ends = ends_of(p, f);
    let name_w = if ends.five_here && !f.name.is_empty() { utf16_len(f.name) * PRIMER_NAME_PX + NAME_GAP } else { 0.0 };
    let chev = |e: End| if e == End::Flat { 0.0 } else { CHEV };
    if f.forward { (chev(ends.left) + name_w, chev(ends.right)) } else { (chev(ends.left), chev(ends.right) + name_w) }
}

/// How far outward a piece's unpaired parts hang: nothing, a half-line, or a whole line.
fn lift_of(p: &Piece) -> f64 {
    if !p.inserts.is_empty() {
        return track::PRIMERS;
    }
    if p.cells.iter().any(|c| matches!(c.kind, CellKind::Change | CellKind::Tail)) { track::PRIMER_LIFT } else { 0.0 }
}

/// One piece of a feature before it meets a row. Bases are 0-based and inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrawPiece {
    start: i64,
    end: i64,
    cut_left: bool,
    cut_right: bool,
    show_head: bool,
    connector: bool,
}

/// A 0-based inclusive span as one linear range, or two when it runs through the origin.
fn split(start: i64, end: i64, length: i64) -> Vec<DrawPiece> {
    let piece = |start, end, cut_left, cut_right| DrawPiece { start, end, cut_left, cut_right, show_head: false, connector: false };
    if end >= start {
        return vec![piece(start, end, false, false)];
    }
    vec![piece(start, length - 1, false, true), piece(0, end, true, false)]
}

/// The bases actually annotated, in stored segment order. Gaps never become filled bars.
fn feature_pieces(f: &RowFeature, length: i64) -> Vec<DrawPiece> {
    if length <= 0 {
        return Vec::new();
    }
    let whole = [(f.start, f.end)];
    let locations = f.locations.unwrap_or(&whole);
    if locations.is_empty() || locations.iter().any(|&(start, end)| start < 0 || end < 0 || start >= length || end >= length) {
        return Vec::new();
    }
    let mut pieces: Vec<DrawPiece> = locations.iter().flat_map(|&(start, end)| split(start, end, length)).collect();
    if f.locations.is_none() {
        // A simple span is unwrapped and each half asked for its own 5' end, so a forward feature
        // through the origin is capped at the origin as well (`piecesForRow` in Bios
        // `editor/rows/RowFeatures.tsx`, lines 384-401: `fiveAt = f.forward ? from : to` per piece).
        pieces.iter_mut().for_each(|p| p.show_head = true);
        return pieces;
    }
    let head = if f.forward { pieces.first_mut() } else { pieces.last_mut() };
    if let Some(head) = head {
        head.show_head = true;
    }
    pieces
}

/// What is left of `a..=b` once `p` is taken out of it.
fn uncover((a, b): (i64, i64), p: &DrawPiece) -> Vec<(i64, i64)> {
    if b < p.start || a > p.end {
        return vec![(a, b)];
    }
    let mut rest = Vec::new();
    if a < p.start {
        rest.push((a, p.start - 1));
    }
    if b > p.end {
        rest.push((p.end + 1, b));
    }
    rest
}

/// Dashed, non-annotated gaps connecting consecutive segments, including the circular seam, then
/// the pieces themselves.
fn feature_drawing_pieces(f: &RowFeature, length: i64) -> Vec<DrawPiece> {
    let pieces = feature_pieces(f, length);
    let mut gaps: Vec<DrawPiece> = Vec::new();
    for pair in pieces.windows(2) {
        let &[left, right] = pair else { continue };
        let after_left = (left.end + 1) % length;
        // Adjacent/overlapping arms, including the two halves of an origin span, have no gap.
        if after_left == right.start || (left.start <= right.end && right.start <= left.end) {
            continue;
        }
        for gap in split(after_left, (right.start - 1 + length) % length, length) {
            // Out-of-order joins can pass over another segment. Never draw a gap over covered bases.
            let mut uncovered = vec![(gap.start, gap.end)];
            for piece in &pieces {
                uncovered = uncovered.into_iter().flat_map(|open| uncover(open, piece)).collect();
            }
            for (start, end) in uncovered {
                if !gaps.iter().any(|g| g.start == start && g.end == end) {
                    gaps.push(DrawPiece { start, end, cut_left: false, cut_right: false, show_head: false, connector: true });
                }
            }
        }
    }
    gaps.extend(pieces);
    gaps
}

/// The rows a feature draws in, as an inclusive range of row indices per drawn piece.
pub fn rows_of_feature(f: &RowFeature, length: i64, bps_per_row: i64, row_count: usize) -> Vec<(usize, usize)> {
    if bps_per_row <= 0 || row_count == 0 {
        return Vec::new();
    }
    let row_of = |bp: i64| usize::try_from(bp.div_euclid(bps_per_row)).map_or(0, |row| row.min(row_count - 1));
    feature_drawing_pieces(f, length).iter().map(|p| (row_of(p.start), row_of(p.end))).collect()
}

/// The pieces of every feature that touch this row, packed into lanes.
///
/// Lanes are assigned by OVERLAP, not by label width: two features covering the same bases must be
/// on separate lines even if both are nameless, because they are two different things and a single
/// bar would claim they are one. A primer's name sits beside it rather than inside, so a primer
/// packs by its VISUAL span, name and chevrons included, or the next one would print over it.
///
/// `seq` is the template a primer's bases are judged against; other kinds ignore it. All of
/// `features` read on one strand: the hang of a lifted primer lane goes OUTWARD from the duplex, up
/// off a forward primer and down off a reverse one.
///
/// `circular` says how an aligned oligo's columns wrap.
pub fn pieces_for_row(
    features: &[RowFeature],
    row: Row,
    char_width: f64,
    sequence_length: i64,
    kind: Kind,
    seq: Option<&str>,
    circular: bool,
) -> RowPieces {
    let mut pieces: Vec<Piece> = Vec::new();
    for (index, f) in features.iter().enumerate() {
        if let Some(runs) = f.runs.filter(|_| kind == Kind::Primer) {
            // An aligned oligo places every base itself, tail and all, so its piece is whatever of
            // it lands in this row rather than a clip of its footprint.
            let laid = cells_from_runs(runs, row, sequence_length, circular);
            let (Some(first), Some(last)) = (laid.cells.first(), laid.cells.last()) else { continue };
            let (start, end) = (first.bp, last.bp);
            pieces.push(Piece {
                feature: index,
                x_start: (start - row.start) as f64 * char_width,
                width: (end - start + 1) as f64 * char_width,
                lane: 0,
                y: 0.0,
                start,
                end,
                cut_left: false,
                cut_right: false,
                show_head: false,
                connector: false,
                cells: laid.cells,
                inserts: laid.inserts,
                five_at: Some(laid.five_at),
                three_at: Some(laid.three_at),
            });
            continue;
        }
        for piece in feature_drawing_pieces(f, sequence_length) {
            let Some(span) = span_in_row(piece.start, piece.end, row, char_width) else { continue };
            // The 5' end is the start for a forward feature and the end for a reverse one.
            let five_at = if f.forward { piece.start } else { piece.end };
            pieces.push(Piece {
                feature: index,
                x_start: span.x_start,
                width: span.width,
                lane: 0,
                y: 0.0,
                start: span.start,
                end: span.end,
                cut_left: span.clipped_left || piece.cut_left,
                cut_right: span.clipped_right || piece.cut_right,
                show_head: piece.show_head && f.directional && five_at >= span.start && five_at <= span.end,
                connector: piece.connector,
                cells: if kind == Kind::Primer { primer_cells(f, seq, span.start, span.end) } else { Vec::new() },
                inserts: Vec::new(),
                five_at: None,
                three_at: None,
            });
        }
    }
    let spans: Vec<(f64, f64)> = pieces
        .iter()
        .map(|p| {
            let (left, right) = features.get(p.feature).filter(|_| kind == Kind::Primer).map_or((0.0, 0.0), |f| primer_padding(p, f));
            (p.x_start - left, p.x_start + p.width + right)
        })
        .collect();
    let lane_of = pack_groups(&pieces, &spans);
    let lanes = lane_of.iter().map(|&lane| lane + 1).max().unwrap_or(0);
    // A lane lifts by the most anything in it hangs off the ribbon.
    let mut lifts = vec![0.0_f64; lanes];
    if kind == Kind::Primer {
        for (p, &lane) in pieces.iter().zip(&lane_of) {
            if let Some(lift) = lifts.get_mut(lane) {
                *lift = lift.max(lift_of(p));
            }
        }
    }
    let mut tops = Vec::with_capacity(lanes);
    let mut height = 0.0;
    for lift in &lifts {
        tops.push(height);
        height += lane_height(kind) + lift;
    }
    // A lifted lane's room sits above the ribbon on the forward side and below it on the reverse.
    let above = features.first().is_none_or(|f| f.forward);
    for (p, &lane) in pieces.iter_mut().zip(&lane_of) {
        p.lane = lane;
        p.y = tops.get(lane).copied().unwrap_or(0.0) + if above { lifts.get(lane).copied().unwrap_or(0.0) } else { 0.0 };
    }
    RowPieces { pieces, lanes, height, lift: lifts.iter().sum() }
}

/// A lane per piece. Every piece and gap of one feature stays on one lane in this row; features
/// take the first lane whose last bar has ended before they start, greedy by left edge.
fn pack_groups(pieces: &[Piece], spans: &[(f64, f64)]) -> Vec<usize> {
    // (feature, leftmost, rightmost), in the order each feature first appears.
    let mut groups: Vec<(usize, f64, f64)> = Vec::new();
    for (p, &(from, to)) in pieces.iter().zip(spans) {
        match groups.iter_mut().find(|g| g.0 == p.feature) {
            Some(group) => {
                group.1 = group.1.min(from);
                group.2 = group.2.max(to);
            }
            None => groups.push((p.feature, from, to)),
        }
    }
    groups.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut last_end: Vec<f64> = Vec::new();
    let mut lane_of_feature: Vec<(usize, usize)> = Vec::with_capacity(groups.len());
    for (feature, from, to) in groups {
        let lane = match last_end.iter().position(|&end| end <= from) {
            Some(lane) => lane,
            None => {
                last_end.push(0.0);
                last_end.len() - 1
            }
        };
        if let Some(end) = last_end.get_mut(lane) {
            *end = to + PACK_GAP;
        }
        lane_of_feature.push((feature, lane));
    }
    pieces.iter().map(|p| lane_of_feature.iter().find(|(feature, _)| *feature == p.feature).map_or(0, |&(_, lane)| lane)).collect()
}

/// Whether a bar is wide enough to carry its name inside it.
pub fn name_fits(p: &Piece, name: &str) -> bool {
    !p.connector && p.width > utf16_len(name) * NAME_CHAR_PX + 8.0
}

/// A feature bar's left edge and width, with the hairline off each end that really is an end.
pub fn bar_span(p: &Piece) -> (f64, f64) {
    let left = if p.cut_left { 0.0 } else { NIB };
    let right = if p.cut_right { 0.0 } else { NIB };
    (p.x_start + left, (p.width - left - right).max(1.0))
}

/// The direction mark at the 5' edge of a bar, drawn inside it: the two corners of its flat side
/// and its tip on the bar's centre line. The map's cap, from the map's own length.
pub fn head_points(p: &Piece, bar_top: f64, kind: Kind, forward: bool) -> [(f64, f64); 3] {
    let half = bar_height(kind) / 2.0 - 2.0;
    let y = bar_top + bar_height(kind) / 2.0;
    let (x0, x1) = (p.x_start, p.x_start + p.width);
    let (base, tip) = if forward { (x0, x0 + HEAD_PX) } else { (x1, x1 - HEAD_PX) };
    [(base, y - half), (tip, y), (base, y + half)]
}

/// One straight stretch of a primer's outline, at its own height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub x0: f64,
    pub x1: f64,
    pub y: f64,
}

/// The left edge of a column inside a piece.
pub fn col_x(p: &Piece, bp: i64, char_width: f64) -> f64 {
    p.x_start + (bp - p.start) as f64 * char_width
}

/// Chains of touching ribbon: a gap or a change ends one chain and starts the next. `y` is the
/// ribbon's top and `half_y` the top of what hangs half a line off it.
pub fn primer_chains(p: &Piece, char_width: f64, y: f64, half_y: f64) -> Vec<Vec<Seg>> {
    let mut chains: Vec<Vec<Seg>> = Vec::new();
    let mut current: Vec<Seg> = Vec::new();
    for r in runs_of(&p.cells) {
        if matches!(r.kind, CellKind::Gap | CellKind::Change) {
            if !current.is_empty() {
                chains.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(Seg {
            x0: col_x(p, r.from, char_width),
            x1: col_x(p, r.to, char_width) + char_width,
            y: if r.kind == CellKind::Tail { half_y } else { y },
        });
    }
    if !current.is_empty() {
        chains.push(current);
    }
    chains
}

/// The outline of one chain of touching segments, as a closed polygon: it walks the tops left to
/// right, stepping where a segment sits at a different height, turns at the right end, walks the
/// bottoms back and turns at the left.
pub fn chain_path(segs: &[Seg], h: f64, left: End, right: End) -> Vec<(f64, f64)> {
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else {
        return Vec::new();
    };
    let mut d = Vec::with_capacity(segs.len() * 4 + 2);
    for s in segs {
        d.push((s.x0, s.y));
        d.push((s.x1, s.y));
    }
    match right {
        End::Point => d.push((last.x1 + CHEV, last.y + h / 2.0)),
        End::Notch => d.push((last.x1 - CHEV, last.y + h / 2.0)),
        End::Flat => {}
    }
    for s in segs.iter().rev() {
        d.push((s.x1, s.y + h));
        d.push((s.x0, s.y + h));
    }
    match left {
        End::Point => d.push((first.x0 - CHEV, first.y + h / 2.0)),
        End::Notch => d.push((first.x0 + CHEV, first.y + h / 2.0)),
        End::Flat => {}
    }
    d
}
