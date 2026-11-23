//! How useful is an annotation? The map's rank function. Ported from Bios `editor/rank.ts`.
//!
//! It is called a map, and cartographers beat this problem four hundred years ago: a city has ten
//! thousand labels and a good map never stacks ten on top of each other. It ranks by usefulness,
//! draws what fits, and lets the rest arrive when you zoom.
//!
//! This module answers one question, "how useful is this annotation?", as a pure number. It decides
//! the DEFAULT, never the ceiling: a demoted feature keeps its arc, keeps its colour and stays one
//! gesture from view. Only a label is declined.

use std::collections::HashMap;

use super::text_fit::is_js_whitespace;

/// What the rank reads of a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ranked<'a> {
    /// The feature key, such as `CDS`. Empty when the feature has none.
    pub kind: &'a str,
    /// Empty when the feature has no name.
    pub name: &'a str,
    /// 0-based and inclusive.
    pub start: i64,
    pub end: i64,
}

// 1. CLASS: "would you name a plasmid after it?"
//
// This does most of the work, and size alone cannot: a T7 promoter is a real, load-bearing
// feature and it is TWELVE BASES, shorter than half the junk in a label wall. Class is what
// separates them.
fn class_rank(kind: &str) -> f64 {
    match kind.to_lowercase().as_str() {
        "cds" | "gene" => 100.0,                                                                // genes: the heroes
        "rep_origin" => 80.0,                                                                   // ori: a landmark you navigate by
        "promoter" | "enhancer" | "terminator" | "polya_signal" | "regulatory" | "rbs" => 65.0, // where/when/how much
        "primer_bind" => 60.0,                                                                  // you designed it and you'll order it
        "protein_bind" => 50.0,
        "misc_recomb" | "ncrna" | "mrna" => 45.0,
        "misc_feature" => 15.0, // the default dumping ground; earns a label only by being big
        "source" => 0.0,        // whole-molecule metadata
        _ => DEFAULT_CLASS_RANK,
    }
}

const DEFAULT_CLASS_RANK: f64 = 15.0;

// 2. SIZE: log-scaled, deliberately.
//
// A 2kb gene should beat a 12bp motif, but softly: linear scaling lets one giant feature drown
// the entire ring. Log keeps a 170x size difference to a ~10x rank difference, so class stays in
// charge.
const SIZE_WEIGHT: f64 = 12.0;

// 3. INFORMATIVENESS: a name that occurs four times tells you nothing.
//
// Penalise repeats, but bound it: five loxP sites should sink to one label, not below the
// misc_feature floor.
const REPEAT_PENALTY: f64 = 8.0;
const MAX_REPEAT_PENALTY: f64 = 24.0;

/// The floor: below this a label says nothing worth the ink even if there is room for it.
/// Deliberately low: culling noise is its only job. Density is handled by collision
/// ([`labeled_indices`]), not by this, and it is not a quota.
pub const MIN_RANK: f64 = 40.0;

/// Two labels collide when they are closer than this around the ring. Measured off a real
/// construct: a cluster about 4.3° apart reads well and one about 2.1° apart is a wall, so legible
/// and illegible sit either side of about 3°.
pub const MIN_LABEL_GAP_DEG: f64 = 3.0;

/// Span in bases, inclusive of both ends.
///
/// `end < start` means the feature wraps the origin, but only on a circular molecule. It is never
/// safe to read `end < start` as a wrap signal on its own (that is how a malformed 1bp feature
/// gets a 6000bp span), so `circular` is required.
pub fn span_of(start: i64, end: i64, length: i64, circular: bool) -> i64 {
    if end >= start {
        return end - start + 1;
    }
    if circular && length > 0 {
        return length - start + end + 1;
    }
    1 // malformed on a linear molecule: treat as a point, don't guess
}

/// A name as a reader would compare it: trimmed, without case.
fn name_key(name: &str) -> String {
    name.trim_matches(is_js_whitespace).to_lowercase()
}

/// How many features share each name, so repeats can be penalised.
pub fn name_counts<'a>(names: impl IntoIterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for name in names {
        let key = name_key(name);
        if key.is_empty() {
            continue;
        }
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankContext {
    pub length: i64,
    pub circular: bool,
    pub counts: HashMap<String, usize>,
}

/// The context for ranking `features` on a construct of `length` bases (`makeRankContext`).
pub fn make_rank_context(length: i64, circular: bool, features: &[Ranked<'_>]) -> RankContext {
    RankContext { length, circular, counts: name_counts(features.iter().map(|f| f.name)) }
}

/// The rank. Higher is more worth a label. Pure: the same feature and context give the same number.
pub fn rank_feature(f: Ranked<'_>, ctx: &RankContext) -> f64 {
    let cls = class_rank(f.kind);
    let span = span_of(f.start, f.end, ctx.length, ctx.circular);
    let size = (1.0 + span as f64).log10() * SIZE_WEIGHT;

    let key = name_key(f.name);
    // An unnamed feature has no label to spend, so it can't win one.
    if key.is_empty() {
        return 0.0;
    }
    let repeats = ctx.counts.get(&key).copied().unwrap_or(1).saturating_sub(1);
    let penalty = (repeats as f64 * REPEAT_PENALTY).min(MAX_REPEAT_PENALTY);

    cls + size - penalty
}

/// Where a feature's label points, in degrees clockwise from the origin: the arc's midpoint.
fn midpoint_deg(f: Ranked<'_>, ctx: &RankContext) -> f64 {
    if ctx.length <= 0 {
        return 0.0;
    }
    let span = span_of(f.start, f.end, ctx.length, ctx.circular);
    // Works for origin-wrapping features too: the remainder brings the midpoint back around.
    let mid = (f.start as f64 + span as f64 / 2.0) % ctx.length as f64;
    (mid / ctx.length as f64) * 360.0
}

/// Smallest angle between two bearings on a circle (never more than 180°).
fn angular_gap(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// Which features earn a label: rank, then collision. Walk the features best-first and take a
/// label only if it clears everything already placed. A sparse arc keeps everything; a crowded one
/// thins to its best few, with no quota.
///
/// Greedy is correct here rather than merely convenient: descending by rank, a label can only ever
/// be blocked by something MORE useful than itself. Returns indices into `features`, in the order
/// the labels were taken.
pub fn labeled_indices(features: &[Ranked<'_>], ctx: &RankContext) -> Vec<usize> {
    let mut candidates: Vec<(usize, f64)> =
        features.iter().enumerate().map(|(i, f)| (i, rank_feature(*f, ctx))).filter(|(_, score)| *score >= MIN_RANK).collect();
    // Stable: the same file gives the same map.
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut kept = Vec::new();
    let mut placed_degs: Vec<f64> = Vec::new();
    for (i, _) in candidates {
        let Some(f) = features.get(i) else { continue };
        let deg = midpoint_deg(*f, ctx);
        if placed_degs.iter().any(|p| angular_gap(*p, deg) < MIN_LABEL_GAP_DEG) {
            continue;
        }
        kept.push(i);
        placed_degs.push(deg);
    }
    kept
}
