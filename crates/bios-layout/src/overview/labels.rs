//! Label placement for the map: the pure geometry behind "a lot of elements, and it still reads".
//! Ported from Bios `editor/overview/labels.ts`.
//!
//! The ring used to run two independent declutter passes (feature names, then enzyme names) and let
//! each settle its own column. Neither knew the other existed, so a feature label and a cut label
//! could land on the same y and print on top of each other. And when things crowded, both passes
//! dropped the loser. One column should hold everything, and a crowd should fan outward on
//! converging leaders instead of losing members.
//!
//! So the rules here are:
//!   1. Names that belong to one thing are one label ([`coalesce_features`],
//!      [`group_cuts_by_position`]). This is where the air comes from: a plasmid whose GenBank splits
//!      EGFP into three abutting CDS segments should spend one row on EGFP, not three.
//!   2. A column never overlaps and never silently drops ([`pack_column`]): it spreads a crowd around
//!      its own centre of gravity, and only sheds a label once the frame is genuinely full.
//!
//! Names are measured by whoever calls: every function that needs a width takes a `measure(text,
//! font_px)` closure, so this module needs no fonts. Lengths of a name are counted in characters.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::text_fit::is_js_whitespace;

/// `Math.round`: a half goes up, where `f64::round` sends it away from zero.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.max`: a NaN on either side is the answer, where `f64::max` would drop it.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}

/// `Math.min`, as [`js_max`].
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// The order a JavaScript `(a, b) => a - b` comparator gives. A NaN sorts last instead of comparing
/// equal to everything: that is not an order, and a Rust sort may panic when handed one.
fn by_number(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

/// How tightly the map stacks its labels: the one dial for "a lot of elements in one place".
///
/// A construct can put a hundred features and cut sites inside a few hundred bases, and no single
/// spacing is right for all of them: a cloning map read at arm's length wants air, and a dense
/// verification map wants everything on screen at once even if it's tight. So the numbers the three
/// layouts share live here rather than as magic constants scattered through the renderer.
///
/// `max_labels` is the honest backstop. Packing never overlaps, but it can only grow a stack until it
/// runs out of frame; past that something has to go, and it should be a decision with a name on it
/// rather than whatever the geometry happened to shed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LabelDensity {
    Roomy,
    Balanced,
    Dense,
}

/// How many characters of the real name a truncation has to keep to still count as one.
///
/// Four: it keeps `eblock1` as `eblo..` and `WPRE optional` as `WPRE o..`, both of which you can
/// still recognise, and it refuses `ch..` for `chicken β-actin promoter` and `bG..` for `bGH poly A`,
/// which you cannot.
const MIN_STEM: usize = 4;

/// Air between a name and the ends of the shape it's written in, per side.
pub fn label_pad(font_px: f64) -> f64 {
    js_round(font_px * 0.8)
}

/// Room for text inside a ring arc: its own sweep, less the air at each end.
pub fn arc_available_px(width_frac: f64, radius: f64, font_px: f64) -> f64 {
    width_frac * std::f64::consts::PI * 2.0 * radius - 2.0 * label_pad(font_px)
}

/// The linear twin.
pub fn bar_available_px(x0: f64, x1: f64, font_px: f64) -> f64 {
    x1 - x0 - 2.0 * label_pad(font_px)
}

fn trim_end(stem: &mut Vec<char>) {
    while stem.last().is_some_and(|c| is_js_whitespace(*c)) {
        stem.pop();
    }
}

fn dotted(stem: &[char]) -> String {
    stem.iter().chain(['.', '.'].iter()).collect()
}

/// The longest form of `name` that fits `available_px`, or `None` if nothing legible does.
///
/// This is the row view's truncation rule restated pure, deliberately rather than inventedly: the
/// row view is the one view whose labels nobody complains about, and two truncation conventions in
/// one product would be worse than either. Each detail is load-bearing:
///
///   - Binary search on prefix length, because a proportional font gives no closed form for "how many
///     characters fit": `measure` has to be asked, and asked O(log n) times rather than n.
///   - The ellipsis is two ASCII dots that OVERWRITE the last two characters rather than being
///     appended. That is what keeps the result no wider than the best fit. `…` would break that (one
///     glyph replacing two, of unrelated width).
///   - A truncated name has to keep enough of itself to still be a name: [`MIN_STEM`] characters
///     before the dots, not "at least four characters total". That floor counts the ellipsis, so it
///     passes `ch..` for `chicken β-actin promoter`, which is two letters and an apology. Drawn on
///     the map it costs a reader more than blank arc does. A silent shape at least reads as
///     "something is here, ask me", and hover answers.
///
/// One deliberate divergence from the row view's rule: it assumes `".."` is never wider than the two
/// characters it replaced, which holds for almost every name and fails for a few ("Wi" is wider than
/// ".." but "il" is not). This verifies and shaves instead of assuming, so the returned string always
/// fits: the invariant the callers rely on to keep text inside its shape.
pub fn fit_label(name: &str, available_px: f64, font_px: f64, mut measure: impl FnMut(&str, f64) -> f64) -> Option<String> {
    let full: Vec<char> = name.chars().collect();
    if full.is_empty() {
        return None;
    }
    if available_px <= 0.0 {
        return None;
    }
    if measure(name, font_px) <= available_px {
        return Some(name.to_owned());
    }

    // Longest prefix that fits.
    let mut lo: i64 = 0;
    let mut hi = full.len() as i64;
    let mut best: usize = 0;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let length = mid as usize;
        let candidate: String = full.iter().take(length).collect();
        if measure(&candidate, font_px) <= available_px {
            if length > best {
                best = length;
            }
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }

    // Spend the last two characters on the ellipsis. Trailing space goes with them: `WPRE optional`
    // cut to `WPRE ..` reads as a gap before the dots rather than a shortened word.
    let mut stem: Vec<char> = full.iter().take(best.saturating_sub(2)).copied().collect();
    trim_end(&mut stem);
    // And check the dots were actually affordable: ".." costing no more than the pair it replaced is
    // true of most names and not all of them.
    while !stem.is_empty() && measure(&dotted(&stem), font_px) > available_px {
        stem.pop();
        trim_end(&mut stem);
    }

    if stem.len() < MIN_STEM {
        return None;
    }
    // A truncation that saves nothing isn't worth the dots: `f1 ori` shortened to `f1 ..` is one
    // character narrower and strictly less informative, so prefer silence and let hover answer.
    if full.len().saturating_sub(stem.len()) < 2 { None } else { Some(dotted(&stem)) }
}

/// A FLAG: the floating label a feature gets when it is too small to write its own name.
///
/// A name written along its arc needs no chrome: it is already sitting on the coloured band that
/// identifies it. Lift that same name off into the margin on a leader line and a bare string
/// floating over the middle of the ring belongs to nothing you can see. So a flag is not a lifted
/// string, it is a chip: the name on a box tinted the feature's OWN colour, with the leader in that
/// colour too. The colour is what carries identity across the gap.
///
/// The padding is proportional to the type rather than fixed, so the chip stays the same shape at
/// every density instead of turning into a slab at `dense`'s small type.
pub fn flag_pad_x(font_px: f64) -> f64 {
    js_round(font_px * 0.45)
}

pub fn flag_pad_y(font_px: f64) -> f64 {
    js_round(font_px * 0.3)
}

/// How wide a flag is drawn: its text, plus the air either side of it.
pub fn flag_width(text_px: f64, font_px: f64) -> f64 {
    text_px + 2.0 * flag_pad_x(font_px)
}

/// How tall a flag is drawn, and therefore how much of a column or row stack it claims.
///
/// Cap height is about 0.72em and descenders run below the baseline, so a box sized to `font_px`
/// plus air reads as centred on the letters even though it is not centred on the em box.
pub fn flag_height(font_px: f64) -> f64 {
    js_round(font_px + 2.0 * flag_pad_y(font_px))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelSpacing {
    /// Height a feature name reserves in a stack, type size included.
    pub feature_row: f64,
    /// Height an enzyme name reserves. Smaller type, so smaller.
    pub cut_row: f64,
    /// Clear space between two stacked labels.
    pub gap: f64,
    /// How far outside the backbone a feature name rides before its leader starts.
    pub feature_pad: f64,
    /// Enzyme names sit further out again, so the two kinds read apart at a glance.
    pub cut_pad: f64,
    /// Hard ceiling on labels drawn at once, per map.
    pub max_labels: usize,
    /// First step off the backbone into the nested feature lanes. Wider than `lane_step` because that
    /// band also carries the names written along the backbone itself.
    pub lane_band: f64,
    /// Spacing between the nested lanes below that. Every lane has to carry its own arc AND the name
    /// written along it: that's what lets a nested feature say what it is in place instead of adding
    /// another leader to the pile.
    pub lane_step: f64,
    /// How thick a lane's arc/bar is drawn: thick enough to hold `lane_font` with air, because a name
    /// written on its shape has to sit IN the shape, not in the gap beside it. Per density rather
    /// than one constant: dense wants a thinner lane and smaller type.
    pub lane_thickness: f64,
    /// Type size for a name written inside a lane.
    pub lane_font: f64,
}

/// The spacing of each density (`LABEL_SPACING` in the web app).
///
/// `lane_step` is small even though the lanes are thick, and that is not a contradiction: features
/// have no leaders, so the gap only has to read as "a different lane", and the ring gets its middle
/// back. A lane only has to be tall enough to hold its type with a little air (cap height is about
/// 0.72em, so an 11px name needs roughly 12px of band); sized off the ellipsis instead, it reads as
/// a slab.
pub const fn label_spacing(density: LabelDensity) -> LabelSpacing {
    match density {
        LabelDensity::Roomy => LabelSpacing {
            feature_row: 20.0,
            cut_row: 18.0,
            gap: 6.0,
            feature_pad: 16.0,
            cut_pad: 28.0,
            max_labels: 60,
            lane_band: 24.0,
            lane_step: 22.0,
            lane_thickness: 14.0,
            lane_font: 11.0,
        },
        LabelDensity::Balanced => LabelSpacing {
            feature_row: 16.0,
            cut_row: 14.0,
            gap: 3.0,
            feature_pad: 14.0,
            cut_pad: 24.0,
            max_labels: 120,
            lane_band: 21.0,
            lane_step: 19.0,
            lane_thickness: 12.0,
            lane_font: 10.0,
        },
        LabelDensity::Dense => LabelSpacing {
            feature_row: 13.0,
            cut_row: 12.0,
            gap: 1.5,
            feature_pad: 12.0,
            cut_pad: 20.0,
            max_labels: 260,
            lane_band: 18.0,
            lane_step: 16.0,
            lane_thickness: 11.0,
            lane_font: 8.5,
        },
    }
}

/// Roomy by default. A map that is hard to read is not a map, and the constructs people actually
/// open crowd dozens of annotations into a few hundred bases, so the resting state should be the
/// calm one, with "Balanced" and "Dense" there for when you WANT everything at once.
pub const LABEL_DENSITY_DEFAULT: LabelDensity = LabelDensity::Roomy;

/// Anything that competes for a place by priority: bigger survives.
pub trait Prioritised {
    fn pri(&self) -> f64;
}

/// Keep the best `max` candidates and drop the rest before any layout runs.
///
/// With a hundred names competing for one stretch of map, packing them all is wasted work: the frame
/// can't hold them and the tail would be shed anyway, just later and by accident. Cutting to the
/// ranked best first makes the ceiling deliberate, and because features carry a far higher priority
/// than cut sites, what survives a brutal cut is the landmarks, which is the right answer.
pub fn cap_by_priority<T: Prioritised + Clone>(items: &[T], max: usize) -> Vec<T> {
    if items.len() <= max {
        return items.to_vec();
    }
    let mut ranked: Vec<(usize, f64)> = items.iter().map(Prioritised::pri).enumerate().collect();
    ranked.sort_by(|a, b| by_number(b.1, a.1));
    let keep: HashSet<usize> = ranked.iter().take(max).map(|r| r.0).collect();
    items.iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, item)| item.clone()).collect() // preserve caller order
}

/// The minimum a span needs for coalescing and lanes: everything else rides along on the winning
/// member, which is why this is a trait over the caller's own feature type.
pub trait Span {
    /// 0-based and inclusive. `end < start` wraps through the origin.
    fn start(&self) -> i64;
    fn end(&self) -> i64;
    fn name(&self) -> Option<&str> {
        None
    }
    /// The feature key, such as `CDS` (`type` in the web app).
    fn kind(&self) -> Option<&str> {
        None
    }
    /// `Some(false)` is the reverse strand; anything else reads as forward.
    fn forward(&self) -> Option<bool> {
        None
    }
    /// The ordered segments of a joined location, when the file wrote one.
    fn locations(&self) -> Option<Vec<(i64, i64)>> {
        None
    }
    /// A location outside the supported grammar, kept without interpreting it.
    fn location_unsupported(&self) -> bool {
        false
    }
    /// The same feature drawn over another extent (`{ ...f, start, end }`).
    fn with_span(&self, start: i64, end: i64) -> Self;
}

/// A [`Span`] with nothing riding along, for a caller that has no feature type of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlainSpan {
    pub start: i64,
    pub end: i64,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub forward: Option<bool>,
    pub locations: Option<Vec<(i64, i64)>>,
    pub location_unsupported: bool,
}

impl Span for PlainSpan {
    fn start(&self) -> i64 {
        self.start
    }
    fn end(&self) -> i64 {
        self.end
    }
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
    fn forward(&self) -> Option<bool> {
        self.forward
    }
    fn locations(&self) -> Option<Vec<(i64, i64)>> {
        self.locations.clone()
    }
    fn location_unsupported(&self) -> bool {
        self.location_unsupported
    }
    fn with_span(&self, start: i64, end: i64) -> Self {
        Self { start, end, ..self.clone() }
    }
}

/// Fuse same-name features that are really one annotation chopped into pieces.
///
/// Writers split a CDS at every internal codon edit, so a file says `2312..2314 EGFP`,
/// `2315..2317 EGFP`, `2318..3031 EGFP` where the bench means one GFP. Drawn literally that's three
/// arcs and three identical labels fighting for the same spot on the ring.
///
/// Two features merge only when they carry the same name, type and orientation AND their spans touch
/// (overlap, or abut with no gap). Two same-name features on opposite sides of the plasmid (the two
/// ITRs of an AAV vector, say) are genuinely two things and stay two things.
///
/// Wrapping spans (`end < start`, crossing the origin) are passed through untouched: they're rare,
/// and merging across the seam would need interval arithmetic that isn't worth the risk of fusing
/// two distinct features into one arc that swallows the whole molecule.
pub fn coalesce_features<T: Span + Clone>(features: &[T]) -> Vec<T> {
    if features.len() < 2 {
        return features.to_vec();
    }

    // Group by identity, remembering where each member sat so the output keeps the input's order.
    let mut groups: Vec<Vec<(&T, usize)>> = Vec::new();
    let mut group_of: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(T, usize)> = Vec::new();
    for (i, f) in features.iter().enumerate() {
        let name = f.name().unwrap_or("");
        if name.is_empty() || f.end() < f.start() || f.locations().is_some() || f.location_unsupported() {
            out.push((f.clone(), i));
            continue;
        }
        let key = format!("{} {} {}", name, f.kind().unwrap_or(""), if f.forward() == Some(false) { "-" } else { "+" });
        let at = *group_of.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if let Some(g) = groups.get_mut(at) {
            g.push((f, i));
        }
    }

    for g in &groups {
        // Sweep left→right fusing anything that touches the run so far. `end + 1 >= start` is the abut
        // test: [2312..2314] and [2315..2317] touch, [100..200] and [500..600] do not.
        let mut sorted = g.clone();
        sorted.sort_by(|a, b| a.0.start().cmp(&b.0.start()).then_with(|| a.0.end().cmp(&b.0.end())));
        let mut members = sorted.into_iter();
        let Some(mut run) = members.next() else { continue };
        let mut start = run.0.start();
        let mut end = run.0.end();
        for next in members {
            if next.0.start() <= end.saturating_add(1) {
                end = end.max(next.0.end());
                // Keep the earliest-declared member as the survivor so its colour and other props win,
                // but the fused span is the run's full extent, not the survivor's own.
                if next.1 < run.1 {
                    run = next;
                }
            } else {
                out.push((run.0.with_span(start, end), run.1));
                run = next;
                start = next.0.start();
                end = next.0.end();
            }
        }
        out.push((run.0.with_span(start, end), run.1));
    }
    out.sort_by_key(|e| e.1);
    out.into_iter().map(|e| e.0).collect()
}

/// `splitSpan` from the web app's `viewport.ts`, as `(start_frac, width_frac)`: a 0-based inclusive
/// span as one piece, or two when it crosses the origin.
fn split_span(start: i64, end: i64, l: i64) -> Vec<(f64, f64)> {
    if l <= 0 {
        return Vec::new();
    }
    let (s, e, len) = (start as f64, end as f64, l as f64);
    if end >= start {
        return vec![(s / len, (e - s + 1.0) / len)];
    }
    vec![(s / len, (len - s) / len), (0.0, (e + 1.0) / len)]
}

/// The bases one piece of a fraction-space split covers.
fn piece_bases(start_frac: f64, width_frac: f64, length: i64) -> (i64, i64) {
    let len = length as f64;
    (js_round(start_frac * len) as i64, (js_round((start_frac + width_frac) * len) - 1.0) as i64)
}

/// `featurePieces` from the web app's `featureGeometry.ts`, keeping each piece's bases only: the
/// bases actually annotated, in stored segment order. Gaps never become filled bars.
pub(crate) fn feature_pieces<T: Span>(feature: &T, length: i64) -> Vec<(i64, i64)> {
    if length <= 0 {
        return Vec::new();
    }
    let locations = feature.locations().unwrap_or_else(|| vec![(feature.start(), feature.end())]);
    if locations.is_empty() || locations.iter().any(|&(start, end)| start < 0 || end < 0 || start >= length || end >= length) {
        return Vec::new();
    }
    locations
        .iter()
        .flat_map(|&(start, end)| split_span(start, end, length))
        .map(|(start_frac, width_frac)| piece_bases(start_frac, width_frac, length))
        .collect()
}

/// What is left of `a..=b` once `p0..=p1` is taken out of it.
fn uncover((a, b): (i64, i64), (p0, p1): (i64, i64)) -> Vec<(i64, i64)> {
    if b < p0 || a > p1 {
        return vec![(a, b)];
    }
    let mut rest = Vec::new();
    if a < p0 {
        rest.push((a, p0.saturating_sub(1)));
    }
    if b > p1 {
        rest.push((p1.saturating_add(1), b));
    }
    rest
}

/// `featureDrawingPieces` from the web app's `featureGeometry.ts`, bases only: the non-annotated
/// gaps connecting consecutive segments (the circular seam included), then the pieces themselves.
pub(crate) fn feature_drawing_pieces<T: Span>(feature: &T, length: i64) -> Vec<(i64, i64)> {
    if length <= 0 {
        return Vec::new();
    }
    let pieces = feature_pieces(feature, length);
    let mut gaps: Vec<(i64, i64)> = Vec::new();
    for pair in pieces.windows(2) {
        let &[left, right] = pair else { continue };
        let after_left = left.1.saturating_add(1) % length;
        // Adjacent/overlapping arms, including the two halves of an origin span, have no gap.
        if after_left == right.0 || (left.0 <= right.1 && right.0 <= left.1) {
            continue;
        }
        let to = right.0.saturating_sub(1).saturating_add(length) % length;
        for (start_frac, width_frac) in split_span(after_left, to, length) {
            // Out-of-order joins can pass over another segment. Never draw a gap over covered bases.
            let mut uncovered = vec![piece_bases(start_frac, width_frac, length)];
            for &piece in &pieces {
                uncovered = uncovered.into_iter().flat_map(|open| uncover(open, piece)).collect();
            }
            for gap in uncovered {
                if !gaps.contains(&gap) {
                    gaps.push(gap);
                }
            }
        }
    }
    gaps.extend(pieces);
    gaps
}

/// Give every feature a lane, so nested features can all be seen at once.
///
/// A backbone is one line, and features nest freely on it: one construct has `CASSETTE 1` holding
/// `CAG` holding `chicken β-actin promoter`, with `eblock1` and `CMV enhancer` threaded through them.
/// Painted onto a single ring they simply overwrite each other, and the map stops being a picture of
/// the construct and becomes an index of it.
///
/// So features are packed into lanes the way a genome browser packs tracks. Lane 0 rides ON the
/// backbone and goes to the biggest feature covering each stretch (the one you'd name the region
/// after), and anything that would collide with it drops to the next lane inward, and so on. Because
/// the walk is largest-first, the dominant feature always wins the line and the detail stacks below
/// it. Features that don't overlap share a lane happily, so a plasmid with no nesting still draws as
/// a single clean ring.
///
/// Returns one lane index per input feature, positionally. `max_lanes` is 6 when not given.
pub fn assign_lanes<T: Span>(features: &[T], l: i64, max_lanes: Option<usize>) -> Vec<usize> {
    let max_lanes = max_lanes.unwrap_or(6);
    let mut lanes = vec![0; features.len()];
    if features.is_empty() || l <= 0 {
        return lanes;
    }

    // A span as the real intervals it covers, so a feature crossing the origin is compared honestly
    // against its neighbours instead of appearing to span the whole molecule backwards.
    let hit = |a: &[(i64, i64)], b: &[(i64, i64)]| a.iter().any(|&(a0, a1)| b.iter().any(|&(b0, b1)| a0 <= b1 && b0 <= a1));

    let span = |f: &T| feature_pieces(f, l).iter().fold(0_i64, |n, p| n.saturating_add(p.1.saturating_sub(p.0).saturating_add(1)));
    // Biggest first: the widest feature over a stretch is the one that earns the backbone.
    let mut order: Vec<(usize, &T, i64)> = features.iter().enumerate().map(|(i, f)| (i, f, span(f))).collect();
    order.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.start().cmp(&b.1.start())));

    let mut occupied: Vec<Vec<Vec<(i64, i64)>>> = Vec::new(); // lane → the pieces already placed in it
    for (i, feature, _) in order {
        let p = feature_drawing_pieces(feature, l);
        let mut lane = 0;
        while lane < max_lanes && occupied.get(lane).is_some_and(|placed| placed.iter().any(|q| hit(&p, q))) {
            lane += 1;
        }
        // Past the cap everything shares the innermost lane. Better a rare overlap deep inside the ring
        // than a feature the map silently refuses to draw.
        if lane >= max_lanes {
            lane = max_lanes.saturating_sub(1);
        }
        if occupied.len() <= lane {
            occupied.resize_with(lane + 1, Vec::new);
        }
        if let Some(placed) = occupied.get_mut(lane) {
            placed.push(p);
        }
        if let Some(slot) = lanes.get_mut(i) {
            *slot = lane;
        }
    }
    lanes
}

/// The deepest lane the ring can actually draw at this radius.
///
/// Lanes nest inward, so lane N sits at `radius - lane_depth(N)`, and nothing stopped that going
/// negative. It was not a theoretical worry: on a real map pane (659 x 1105) a 24-character feature
/// name drives the ring margin to 188px, which leaves R0 = 142px, while roomy's sixth lane sits 184px
/// inward. Lane 4 rendered at radius -6 and lane 3 at radius 30: a 60px blob at the origin. The inner
/// lanes were not thin, they were inside out.
///
/// `min_inner` (34 when not given) is the radius below which a lane is not worth drawing: an arc that
/// small cannot carry a name or be aimed at, and a lane that cannot say what it is should not be
/// there. Features past the cap share the innermost drawn lane, matching what [`assign_lanes`] does
/// at its own ceiling.
pub fn lanes_for_radius(radius: f64, sp: LabelSpacing, min_inner: Option<f64>) -> i64 {
    let min_inner = min_inner.unwrap_or(34.0);
    if radius <= min_inner {
        return 0;
    }
    // Lane 1 costs lane_band, each lane after that costs lane_step.
    let after_band = radius - min_inner - sp.lane_band;
    if after_band < 0.0 {
        return 0;
    }
    (1.0 + (after_band / sp.lane_step).floor()) as i64
}

/// One enzyme cutting at one base.
pub trait Cut {
    fn pos(&self) -> i64;
    fn enzyme(&self) -> &str;
}

impl<S: AsRef<str>> Cut for (i64, S) {
    fn pos(&self) -> i64 {
        self.0
    }
    fn enzyme(&self) -> &str {
        self.1.as_ref()
    }
}

/// The enzymes that cut at one base, as one label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutGroup<T> {
    pub pos: i64,
    pub enzymes: Vec<String>,
    pub members: Vec<T>,
}

/// Collapse enzymes that cut at the same base into one label.
///
/// Isoschizomers are the normal case, not an edge case: PaeR7I, PspXI and XhoI all cut at 33, and
/// printing three stacked labels off one tick spends three rows saying one thing. "PaeR7I - PspXI -
/// XhoI" on a single line is both shorter and truer. Names are joined in the order given.
///
/// `tolerance` is in bases and 0 when not given: 0 fuses only exact co-cutters, which is the honest
/// default, because an enzyme cutting one base over really is a different cut.
pub fn group_cuts_by_position<T: Cut + Clone>(cuts: &[T], tolerance: Option<i64>) -> Vec<CutGroup<T>> {
    let tolerance = tolerance.unwrap_or(0);
    let mut sorted: Vec<&T> = cuts.iter().collect();
    sorted.sort_by_key(|c| c.pos());
    let mut groups: Vec<CutGroup<T>> = Vec::new();
    for c in sorted {
        match groups.last_mut() {
            Some(last) if c.pos().saturating_sub(last.pos) <= tolerance => {
                last.enzymes.push(c.enzyme().to_owned());
                last.members.push(c.clone());
            }
            _ => groups.push(CutGroup { pos: c.pos(), enzymes: vec![c.enzyme().to_owned()], members: vec![c.clone()] }),
        }
    }
    groups
}

/// Join co-positional enzyme names the way a plasmid map writes them.
pub fn join_enzymes<S: AsRef<str>>(names: &[S]) -> String {
    names.iter().map(AsRef::as_ref).collect::<Vec<&str>>().join(" - ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowItem {
    pub id: String,
    /// Centre x the label wants to sit at, already clamped into the frame by the caller.
    pub x: f64,
    /// Half the label's rendered width.
    pub half: f64,
    /// Bigger claims a lower (closer to the strand) row, and survives when rows run out.
    pub pri: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowPlaced {
    pub id: String,
    /// 0 is the row nearest the strand; higher rows stack away from it.
    pub row: usize,
}

/// Stack a linear map's labels into rows so none overlap, dropping nothing until it must.
///
/// The ring's problem is vertical, the line's is horizontal: two names centred over neighbouring
/// features run into each other side by side. The answer is the same in spirit: take another row
/// rather than lose a label.
///
/// Labels are placed best-first rather than left-to-right, which matters twice: the most useful name
/// gets the row nearest the strand (the shortest, clearest leader), and when the stack finally hits
/// the top of the frame it's the least useful name that goes, not whichever one happened to be last
/// in x order. Each row keeps its own occupied intervals, so a label drops into the lowest row where
/// it clears everything already there by `gap` (8 when not given).
pub fn pack_rows(items: &[RowItem], max_rows: usize, gap: Option<f64>) -> Vec<RowPlaced> {
    let gap = gap.unwrap_or(8.0);
    if items.is_empty() || max_rows < 1 {
        return Vec::new();
    }
    let mut rows: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut out: Vec<RowPlaced> = Vec::new();
    let mut order: Vec<&RowItem> = items.iter().collect();
    order.sort_by(|a, b| by_number(b.pri, a.pri).then_with(|| by_number(a.x, b.x)));

    for it in order {
        let x0 = it.x - it.half;
        let x1 = it.x + it.half;
        let mut row = None;
        for r in 0..max_rows {
            if rows.len() <= r {
                rows.push(Vec::new());
            }
            if rows.get(r).is_some_and(|taken| taken.iter().all(|iv| x1 + gap <= iv.0 || x0 >= iv.1 + gap)) {
                row = Some(r);
                break;
            }
        }
        let Some(row) = row else { continue }; // the stack is as tall as the frame allows: this one goes
        if let Some(taken) = rows.get_mut(row) {
            taken.push((x0, x1));
        }
        out.push(RowPlaced { id: it.id.clone(), row });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiftItem {
    pub id: String,
    /// The label's horizontal extent, already clamped into the frame.
    pub x0: f64,
    pub x1: f64,
    /// Where the label wants to sit vertically: just above its own anchor.
    pub y: f64,
    pub pri: f64,
}

/// Place labels above anchors that are at DIFFERENT heights, lifting each until it is clear.
///
/// Row indices only guarantee separation when every label measures from the same baseline. During
/// the ring→line unroll they don't: each name hugs its own point on a curve, so "row 0" at a low part
/// of the arc and "row 2" at a high part can land three pixels apart and print through each other.
///
/// So instead of assigning rows, each label starts where it wants and steps upward until it clears
/// everything already placed, in x AND y, which is the only test that means anything when the
/// baseline moves. Best-first, so the most useful name gets to stay nearest its anchor. Where the arc
/// is flat (the seam with the linear view) every anchor shares a height and this reproduces ordinary
/// row stacking exactly, which is what keeps the transition from snapping. `gap` is 8 when not given.
pub fn pack_above_anchors(items: &[LiftItem], min_y: f64, step: f64, gap: Option<f64>) -> Vec<Placed> {
    let gap = gap.unwrap_or(8.0);
    let mut placed: Vec<(f64, f64, f64)> = Vec::new();
    let mut out: Vec<Placed> = Vec::new();
    let mut order: Vec<&LiftItem> = items.iter().collect();
    order.sort_by(|a, b| by_number(b.pri, a.pri).then_with(|| by_number(a.x0, b.x0)));

    for it in order {
        let mut y = it.y;
        let mut free = false;
        // Bounded by the frame: once a label would sit above `min_y` it has nowhere left to go.
        while y >= min_y {
            if !placed.iter().any(|p| it.x1 + gap > p.0 && it.x0 < p.1 + gap && (y - p.2).abs() < step) {
                free = true;
                break;
            }
            // A step too small to move a y this large would loop here forever (the web app does).
            // Such a label has nowhere to go either, so it is dropped.
            if y - step == y {
                break;
            }
            y -= step;
        }
        if !free {
            continue; // ran out of frame: this name goes, the feature keeps its mark
        }
        placed.push((it.x0, it.x1, y));
        out.push(Placed { id: it.id.clone(), y });
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColItem {
    /// Caller's handle, returned untouched so it can look the placement back up.
    pub id: String,
    /// Where the label WANTS to sit (its centre y, level with its anchor on the arc).
    pub y: f64,
    /// How tall the label is, including the air it needs above and below.
    pub h: f64,
    /// Bigger survives when the column is genuinely too full.
    pub pri: f64,
    /// This item cannot move: it is something already drawn elsewhere whose space the column must
    /// flow around (a name written along its own arc). Its placement comes back unchanged; it is here
    /// only so the movable labels are spaced off it.
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: String,
    /// Settled centre y.
    pub y: f64,
}

impl Prioritised for RowItem {
    fn pri(&self) -> f64 {
        self.pri
    }
}

impl Prioritised for LiftItem {
    fn pri(&self) -> f64 {
        self.pri
    }
}

impl Prioritised for ColItem {
    fn pri(&self) -> f64 {
        self.pri
    }
}

/// Fuse overlapping pinned bands into the one stretch of column they actually cover.
///
/// Names written along their own arcs become obstacles the floating column has to flow around, and
/// nested features put several of them at the SAME angle on different radii: `CASSETTE 1`, `CAG` and
/// `PCR1` all cross 3 o'clock, one lane inside the next. Between them they cover ONE band of the
/// column, three lanes deep; they do not cover three bands end to end.
///
/// [`pack_column`] decides whether to shed by summing item heights, so counting those separately
/// claimed three times the column they really occupy. On the ring that was the difference between
/// fourteen flags and seven: the frame was declared full and labels were shed that had somewhere
/// perfectly good to go. Pinned items can never be shed, so the whole over-count came out of the
/// movable ones, which is to say, out of exactly the names the map exists to show.
///
/// Merging is safe for spacing too: a label has to clear the union of the bands either way, and the
/// fused item is precisely that union. `id_prefix` is `band` when not given.
pub fn merge_bands(items: &[ColItem], id_prefix: Option<&str>) -> Vec<ColItem> {
    let id_prefix = id_prefix.unwrap_or("band");
    if items.len() < 2 {
        return items.to_vec();
    }
    let mut bands: Vec<(f64, f64)> = items.iter().map(|it| (it.y - it.h / 2.0, it.y + it.h / 2.0)).collect();
    bands.sort_by(|a, b| by_number(a.0, b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for b in bands {
        match merged.last_mut() {
            Some(last) if b.0 <= last.1 => last.1 = js_max(last.1, b.1),
            _ => merged.push(b),
        }
    }
    merged
        .iter()
        .enumerate()
        .map(|(k, b)| ColItem { id: format!("{id_prefix}{k}"), y: (b.0 + b.1) / 2.0, h: b.1 - b.0, pri: 0.0, pinned: true })
        .collect()
}

/// A run of labels stacked edge to edge.
struct Cluster<'a> {
    members: Vec<&'a ColItem>,
    h: f64,
    want: f64,
    top_y: f64,
}

fn height_of(xs: &[&ColItem], gap: f64) -> f64 {
    xs.iter().fold(-gap, |s, x| s + x.h + gap)
}

/// Where a block wants its centre. Normally the mean of its members' own ideal positions, but a
/// block holding something PINNED has no say: it is positioned so the pinned member lands exactly
/// where it already is, and the movable labels arrange themselves above and below it.
fn want_of(members: &[&ColItem], h: f64, gap: f64) -> f64 {
    let mut offset = 0.0;
    for m in members {
        if m.pinned {
            return m.y + h / 2.0 - (offset + m.h / 2.0);
        }
        offset += m.h + gap;
    }
    members.iter().fold(0.0, |s, m| s + m.y) / members.len() as f64
}

/// Put a cluster where it wants to be, centred on its members' mean ideal, without leaving the
/// frame. Clamping here is what keeps a fan that reaches the top of the ring from sailing off it.
fn seat(c: &mut Cluster<'_>, top: f64, bottom: f64) {
    c.top_y = js_min(js_max(c.want - c.h / 2.0, top), bottom - c.h);
}

/// Fuse the first two neighbours that overlap. `false` when the column is quiet.
fn merge_first_overlap(clusters: &mut Vec<Cluster<'_>>, top: f64, bottom: f64, gap: f64) -> bool {
    let mut merged = false;
    let mut rest = std::mem::take(clusters).into_iter().peekable();
    while let Some(a) = rest.next() {
        if merged || rest.peek().is_none_or(|b| a.top_y + a.h + gap <= b.top_y + 1e-9) {
            clusters.push(a);
            continue;
        }
        let Some(b) = rest.next() else {
            clusters.push(a);
            continue;
        };
        let h = a.h + gap + b.h;
        let mut members = a.members;
        members.extend(b.members);
        // Centre of gravity: the mean of where the members individually wanted to be, so the block
        // straddles the crowd instead of hanging off one end of it, unless something in it is pinned,
        // in which case that member's real position decides where the whole block sits.
        let want = want_of(&members, h, gap);
        let mut fused = Cluster { members, h, want, top_y: 0.0 };
        seat(&mut fused, top, bottom);
        clusters.push(fused);
        merged = true;
    }
    merged
}

/// Stack one side of the map so nothing overlaps and a crowd fans out instead of losing members.
///
/// The naive version places top-down, `y = max(ideal, prev_bottom + gap)`, which makes a cluster
/// grow downward only, so seven sites at 0–55 bp all slide below their anchors and the whole fan
/// leans. Instead this grows clusters around their own centre of gravity: overlapping neighbours
/// merge into a block, the block centres on the mean of its members' ideal positions, and merging
/// repeats until nothing overlaps. A crowd therefore spreads symmetrically, half above its anchor,
/// half below, which is the converging-leader look, and an uncrowded label never moves at all.
///
/// Dropping is the last resort, not the first: only when the column's total height genuinely exceeds
/// the frame do we shed labels, lowest priority first, and only as many as it takes to fit. `gap` is
/// 0 when not given.
pub fn pack_column(items: &[ColItem], top: f64, bottom: f64, gap: Option<f64>) -> Vec<Placed> {
    let gap = gap.unwrap_or(0.0);
    if items.is_empty() {
        return Vec::new();
    }

    let frame = bottom - top;
    // Shed only what physically cannot fit. Everything below survives the crowd and gets spread.
    let mut live: Vec<&ColItem> = items.iter().collect();
    if height_of(&live, gap) > frame {
        // Weakest first, but a pinned item is already drawn on the map, so shedding it would only
        // hide the obstacle, not remove it.
        let mut by_pri: Vec<&ColItem> = live.iter().copied().filter(|x| !x.pinned).collect();
        by_pri.sort_by(|a, b| by_number(a.pri, b.pri));
        let mut doomed: HashSet<&str> = HashSet::new();
        let mut h = height_of(&live, gap);
        for victim in by_pri {
            if h <= frame {
                break;
            }
            doomed.insert(victim.id.as_str());
            h -= victim.h + gap;
        }
        live.retain(|x| !doomed.contains(x.id.as_str()));
    }
    if live.is_empty() {
        return Vec::new();
    }

    // Each label starts as a one-member cluster sitting exactly where it wants to be.
    live.sort_by(|a, b| by_number(a.y, b.y));
    let mut clusters: Vec<Cluster<'_>> = live.into_iter().map(|m| Cluster { members: vec![m], h: m.h, want: m.y, top_y: m.y - m.h / 2.0 }).collect();
    for c in &mut clusters {
        seat(c, top, bottom);
    }

    // Merge overlapping neighbours until the column is quiet. Each merge can push the new block into
    // its neighbour, so restart the scan rather than assuming one pass settles it. n−1 merges is the
    // most a column of n can need; the bound is fixed up front because `clusters` shrinks as we go.
    let max_merges = clusters.len();
    for _guard in 0..=max_merges {
        if !merge_first_overlap(&mut clusters, top, bottom, gap) {
            break;
        }
    }

    // Walk each block top-down handing out rows in the members' original vertical order.
    let mut out: Vec<Placed> = Vec::new();
    for c in &clusters {
        let mut y = c.top_y;
        for m in &c.members {
            out.push(Placed { id: m.id.clone(), y: y + m.h / 2.0 });
            y += m.h + gap;
        }
    }
    out
}
