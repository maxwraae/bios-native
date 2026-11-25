//! What the map draws from, whichever way it is projected: the fused features, how much each is
//! worth a label, and the lane each sits in. Ported from the top of Bios
//! `editor/overview/PlasmidOverview.tsx` (`readFeatures`, the priorities and lanes its component
//! computes, and the small rules the ring and the line share).
//!
//! The ring and the line are laid out by [`super::ring_map`] and [`super::line_map`]. Both take a
//! [`MapScene`] and a [`Frame`] and return plain numbers for the painter.

use super::display::MapFeature;
use super::labels::{LabelDensity, LabelSpacing, assign_lanes, coalesce_features, flag_height};
use super::projection::Point;
use super::rank::{Ranked, make_rank_context, rank_feature};

/// The features the map draws, with what the layouts need to know about each.
#[derive(Clone, Debug, PartialEq)]
pub struct MapScene {
    /// Same-name pieces that touch are fused into the one annotation they are: writers split a
    /// CDS at every internal edit, and drawn literally that is three arcs under three identical
    /// labels. Every index below lines up with this list.
    pub features: Vec<MapFeature>,
    /// How useful each feature is, for deciding which names are kept when not all fit.
    pub priorities: Vec<f64>,
    /// Which lane each feature draws in. Lane 0 rides on the backbone and goes to the widest
    /// feature over each stretch; anything nested inside it stacks away from it.
    pub lanes: Vec<usize>,
    /// Bases in the construct.
    pub length: i64,
    pub circular: bool,
    /// The cut sites drawn: each enzyme's top-strand scission (a gap coordinate) and its name.
    pub cuts: Vec<(i64, String)>,
    /// The construct's primers, drawn in a track of their own off the backbone. Empty unless
    /// [`MapScene::with_primers`] put them there, which is what the Primers layer decides.
    pub primers: Vec<MapFeature>,
    /// Which primer lane each primer draws in. Lane 0 hugs the backbone and overlapping primers
    /// stack away from it, as [`MapScene::lanes`] does for features.
    pub primer_lanes: Vec<usize>,
}

impl MapScene {
    /// The scene for the shown features (see [`super::display::display_features`]) of a construct
    /// of `length` bases.
    pub fn new(shown: &[MapFeature], length: i64, circular: bool) -> Self {
        let features = coalesce_features(shown);
        let ranked: Vec<Ranked<'_>> = features.iter().map(|f| Ranked { kind: &f.kind, name: &f.name, start: f.start, end: f.end }).collect();
        let ctx = make_rank_context(length, circular, &ranked);
        let priorities = ranked.iter().map(|f| rank_feature(*f, &ctx)).collect();
        let lanes = assign_lanes(&features, length, None);
        Self { features, priorities, lanes, length, circular, cuts: Vec::new(), primers: Vec::new(), primer_lanes: Vec::new() }
    }

    /// The same scene with these cut sites on it.
    pub fn with_cuts(self, cuts: Vec<(i64, String)>) -> Self {
        Self { cuts, ..self }
    }

    /// The same scene with these primers in its primer track (see [`super::display::display_primers`]).
    ///
    /// They are NOT coalesced the way features are: two oligos of the same name that happen to abut
    /// are two oligos, not one long one.
    pub fn with_primers(self, primers: &[MapFeature]) -> Self {
        let primer_lanes = assign_lanes(primers, self.length, Some(MAX_PRIMER_LANES));
        Self { primers: primers.to_vec(), primer_lanes, ..self }
    }

    pub(crate) fn priority(&self, i: usize) -> f64 {
        self.priorities.get(i).copied().unwrap_or(0.0)
    }

    pub(crate) fn lane(&self, i: usize) -> usize {
        self.lanes.get(i).copied().unwrap_or(0)
    }

    pub(crate) fn primer_lane(&self, i: usize) -> usize {
        self.primer_lanes.get(i).copied().unwrap_or(0)
    }

    /// How many primer lanes this scene actually needs: 0 with no primers, which is what keeps a
    /// construct without any drawn exactly as it was before the track existed.
    pub(crate) fn primer_lanes_used(&self) -> usize {
        if self.primers.is_empty() { 0 } else { self.primer_lanes.iter().map(|l| l + 1).max().unwrap_or(1).min(MAX_PRIMER_LANES) }
    }
}

/// The rectangle the map is drawn in and the window onto the molecule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub w: f64,
    pub h: f64,
    /// Fraction of the molecule at the middle of the view: 0 for a whole ring, 0.5 for a whole line.
    pub center: f64,
    /// Fraction of the molecule in view. 1 is all of it.
    pub width: f64,
    pub density: LabelDensity,
    /// Whether enzyme names compete for a place (the "Enzyme names" layer). Off, every cut still
    /// draws its hash.
    pub cut_names: bool,
    /// Pin the line's axis this far above the floor of the frame, instead of centring the whole
    /// drawing in it.
    ///
    /// FOR A MAP THAT IS A BAND IN A PAGE rather than a pane of its own. Centred, a construct
    /// several lanes deep puts its features above the top of a short band and they are simply not
    /// there; pinned, the features stand on the axis and the band is read from the bottom up,
    /// which is what a figure in a list wants.
    pub axis_at_bottom: Option<f64>,
    /// Room outside the ring for a band of what is being ADDED, in pixels (0 when there is none).
    ///
    /// Outside the ring is not empty: the coordinate ticks and their numbers are out there, so a
    /// band drawn any closer would be written through the scale. The ring gives this up to make
    /// room, and it has to be ONE number — the renderer and the hit-test both read the margin, and
    /// a disagreement turns a click into the wrong base.
    pub band_reserve: f64,
}

impl Frame {
    /// The whole molecule as a ring, origin at twelve o'clock.
    pub fn ring(w: f64, h: f64) -> Self {
        Self {
            w,
            h,
            center: 0.0,
            width: 1.0,
            density: super::labels::LABEL_DENSITY_DEFAULT,
            cut_names: true,
            axis_at_bottom: None,
            band_reserve: 0.0,
        }
    }

    /// The whole molecule as a line.
    pub fn line(w: f64, h: f64) -> Self {
        Self {
            w,
            h,
            center: 0.5,
            width: 1.0,
            density: super::labels::LABEL_DENSITY_DEFAULT,
            cut_names: true,
            axis_at_bottom: None,
            band_reserve: 0.0,
        }
    }
}

/// A name lifted off its feature onto a chip, with the leader that ties the two together.
///
/// A name written along its own arc needs no chrome. Lifted into the margin, a bare string belongs
/// to nothing you can see, so it rides on a box in the feature's own tint and the leader is in that
/// colour too: the colour is what carries identity across the gap.
#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    /// Index into [`MapScene::features`].
    pub feature: usize,
    pub text: String,
    /// Left edge of the box.
    pub x: f64,
    /// Centre y: the box straddles it.
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Type size of the name.
    pub font: f64,
    /// From the feature's own lane to the box.
    pub leader: (Point, Point),
}

/// A lane expressed as depth in pixels: inward on the ring, upward on the line. The first step is
/// wider than the rest because that band also carries the names written along the backbone.
pub fn lane_depth(lane: usize, sp: LabelSpacing) -> f64 {
    if lane == 0 { 0.0 } else { sp.lane_band + (lane - 1) as f64 * sp.lane_step }
}

/// Enzyme names are one size on both projections: they name a point, not a shape.
pub const CUT_FONT: f64 = 10.5;

/// How far a cut tick reaches either side of the strand: enough to be seen against a lane's band
/// rather than swallowed by it.
pub fn cut_overhang(thickness: f64) -> f64 {
    thickness / 2.0 + 5.0
}

/// The gap between the molecule's own axis and lane 0. One number for both projections: the ring
/// measures it inward from the coordinate circle and the line measures it up from the axis.
pub fn lane_inset(sp: LabelSpacing) -> f64 {
    sp.lane_thickness / 2.0 + 12.0
}

// ── THE PRIMER TRACK ────────────────────────────────────────────────────────────────────────────
//
// A lane of its own between the molecule's axis and the feature lanes, on both projections: the
// ring measures it inward from the backbone and the line measures it up from the axis, exactly as
// the feature lanes are measured, so a primer hugs the backbone in either view.
//
// A DEPARTURE from the web app, which draws no oligos on its map (PARITY.md, Max 2026-11-25). The
// numbers are the sequence rows' own: slimmer than a feature's band, its own lanes, the name beside
// the 5' end (`rows.rs` `lanes.primers`, `row_features.rs` `PRIMER_BAR` and `NAME_GAP`). A map with
// twenty-two primers on it has to still read as a map of its FEATURES, so everything here is
// lighter: thinner, shorter-headed, smaller type, and never a flag out in the margin.

/// How many lanes the primer track may grow to before the rest share its outermost one. Three: a
/// fourth would push the features further off the backbone than the oligos are worth.
pub const MAX_PRIMER_LANES: usize = 3;

/// How thick a primer's arrow is drawn: a little under half a feature's band, which is what makes
/// the track read as a margin note rather than another row of features.
pub fn primer_thickness(sp: LabelSpacing) -> f64 {
    sp.lane_thickness * 0.45
}

/// Centre-to-centre between two primer lanes.
pub fn primer_step(sp: LabelSpacing) -> f64 {
    primer_thickness(sp) + 4.0
}

/// The gap between the molecule's axis and the first primer lane's centre line.
///
/// Clear of the cut hashes, which reach [`cut_overhang`] either side of the axis on the line (6px
/// on the ring): a hash drawn across an oligo's name is the one collision this track can actually
/// have, because nothing else lives between the axis and the feature lanes.
pub fn primer_inset(sp: LabelSpacing) -> f64 {
    primer_thickness(sp) / 2.0 + cut_overhang(sp.lane_thickness) + 2.0
}

/// How much room the primer track takes between the axis and the feature lanes, which is how far
/// the feature lanes move over to make way for it. 0 with no primers drawn.
pub fn primer_block(lanes_used: usize, sp: LabelSpacing) -> f64 {
    match lanes_used.checked_sub(1) {
        None => 0.0,
        Some(after_first) => primer_inset(sp) + after_first as f64 * primer_step(sp) + primer_thickness(sp) / 2.0,
    }
}

/// How long a primer's arrowhead is, along the molecule. Shorter than a feature's
/// [`super::direction::HEAD_PX`], because a primer is drawn as one arrow rather than as a band with
/// a mark on it, and a 7px head on a 20-mer would be the whole oligo.
pub const PRIMER_HEAD_PX: f64 = 5.0;

/// Type size of a primer's name: a step under a feature's in-lane name, never below 7.
pub fn primer_font(sp: LabelSpacing) -> f64 {
    (sp.lane_font - 1.5).max(7.0)
}

/// Air between a primer's 5' end and its name, as the rows leave (`row_features::NAME_GAP`).
pub const PRIMER_NAME_GAP: f64 = 5.0;

/// How big a flag's type is. A flag is read across a gap rather than off the shape it names, so it
/// does not shrink as far as an in-lane name does.
pub fn flag_font(sp: LabelSpacing) -> f64 {
    9.5f64.max(sp.lane_font)
}

/// The height one flag claims in a stack: its box, which at `dense` is taller than a feature row.
pub(crate) fn flag_row(sp: LabelSpacing) -> f64 {
    sp.feature_row.max(flag_height(flag_font(sp)) + 2.0)
}

/// A round number near `raw`: 1, 2 or 5 times a power of ten, the way a ruler is divided.
pub fn nice_step(raw: f64) -> f64 {
    let mag = 10f64.powf(raw.max(1.0).log10().floor());
    let n = raw / mag;
    let lead = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    lead * mag
}

/// A base position as the scale prints it: whole, with a comma between thousands.
pub fn group_thousands(bp: f64) -> String {
    let digits = format!("{:.0}", bp.abs());
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if bp < 0.0 && digits != "0" {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
