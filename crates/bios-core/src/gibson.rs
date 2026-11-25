//! Gibson assembly design: the eBlock overlap designer, the split into synthesis pieces, and the
//! preflight checks for ordering. Ported from the Bios web app.
//!
//! The insert is a *synthesized* eBlock, not a PCR product. Nothing here designs amplification
//! primers; it sizes the two homology arms that flank the insert so the ordered eBlock Gibsons
//! straight into a cut backbone, and splits the eBlock into consecutive overlapping pieces when it
//! exceeds the synthesis limit.
//!
//! Coordinates are 0-based. `circular` is required on every origin-affected call.

use std::collections::HashSet;

use crate::edits::{EditError, TruncatedFeature, apply_insert, apply_replace};
use crate::enzymes::{find_cut_sites, get_enzyme};
use crate::model::{Feature, Segment, Strand};
use crate::seq::{clean_seq, is_js_whitespace, revcomp};

// ---------------------------------------------------------------------------
// Errors.
// ---------------------------------------------------------------------------

/// Which end of the opening has no backbone left to anneal to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmSide {
    Upstream,
    Downstream,
}

impl std::fmt::Display for ArmSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArmSide::Upstream => write!(f, "upstream"),
            ArmSide::Downstream => write!(f, "downstream"),
        }
    }
}

/// A design that cannot be satisfied. Every message is read by somebody about to order DNA, so the
/// wording is the web app's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GibsonError {
    /// A synthesis cap below the overlap consecutive pieces must share.
    EblockCapBelowOverlap {
        max_len: i64,
    },
    /// `end < start` means "wraps the origin", and a linear backbone has no origin.
    WrappingRegionOnLinear {
        start: i64,
        end: i64,
    },
    OriginArcOnLinear,
    NoFragments,
    /// The opening left fewer than [`OL_MIN`] bases of backbone on one side.
    NoRoomForArm {
        side: ArmSide,
        bp: i64,
    },
    Edit(EditError),
}

impl std::fmt::Display for GibsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GibsonError::EblockCapBelowOverlap { max_len } => write!(
                f,
                "An eBlock cap of {max_len} bp is below the {FALLBACK_OVERLAP} bp overlap each piece has to share with the next, so no set of pieces can tile the insert."
            ),
            GibsonError::WrappingRegionOnLinear { start, end } => write!(
                f,
                "Gibson opening {start}..{end} has end < start, which means \"wraps the origin\" — but this backbone is linear, so there is no origin to wrap. Order the bounds, or mark the construct circular."
            ),
            GibsonError::OriginArcOnLinear => write!(f, "The arc through the origin only exists on a circular backbone."),
            GibsonError::NoFragments => write!(f, "Add at least one insert fragment with sequence."),
            GibsonError::NoRoomForArm { side, bp } => write!(
                f,
                "The opening leaves only {bp} bp of backbone on the {side} side — too little for a homology arm, so there is nothing for the insert to join onto there."
            ),
            GibsonError::Edit(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for GibsonError {}

impl From<EditError> for GibsonError {
    fn from(e: EditError) -> Self {
        GibsonError::Edit(e)
    }
}

// ---------------------------------------------------------------------------
// The web app's string and number primitives, so the arithmetic is the same one.
// ---------------------------------------------------------------------------

pub(crate) fn len64(s: &str) -> i64 {
    i64::try_from(s.len()).unwrap_or(i64::MAX)
}

/// `String.prototype.slice`: ends clamp into the string, a negative index counts back from its
/// length, and a backwards range is empty rather than an error.
pub(crate) fn js_slice(s: &str, start: i64, end: i64) -> &str {
    let len = len64(s);
    let at = |x: i64| usize::try_from(if x < 0 { (len + x).max(0) } else { x.min(len) }).unwrap_or(0);
    let (a, b) = (at(start), at(end));
    if b <= a { "" } else { s.get(a..b).unwrap_or_default() }
}

/// `String.prototype.slice` with one argument: from here to the end.
fn js_from(s: &str, start: i64) -> &str {
    js_slice(s, start, i64::MAX)
}

/// `Number.prototype.toFixed`: the float's exact decimal value, rounded half away from zero, with
/// `digits` places always present. Rust's own formatting sends an exact half to the even digit, and
/// these strings go in front of somebody ordering DNA.
fn to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // Every f64 is a finite decimal of at most 1074 places, so this is the value itself with
    // nothing rounded away, and the first digit dropped decides the rounding alone.
    let exact = format!("{:.1074}", value.abs());
    let (whole, places) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut kept: Vec<u8> = whole.bytes().chain(places.bytes().take(digits)).collect();
    if places.as_bytes().get(digits).is_some_and(|dropped| *dropped >= b'5') {
        let mut carried = true;
        for digit in kept.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                if digit.is_ascii_digit() {
                    *digit += 1;
                }
                carried = false;
                break;
            }
        }
        if carried {
            kept.insert(0, b'1');
        }
    }
    let point = kept.len().saturating_sub(digits);
    let mut out = String::with_capacity(kept.len() + 2);
    out.push_str(sign);
    for (i, digit) in kept.iter().enumerate() {
        if i == point && digits > 0 {
            out.push('.');
        }
        out.push(char::from(*digit));
    }
    out
}

/// `Math.ceil(n / k)`, which is how every piece count here is sized.
pub(crate) fn div_ceil(n: i64, k: i64) -> i64 {
    if k == 0 {
        return n;
    }
    n.div_euclid(k) + i64::from(n.rem_euclid(k) != 0)
}

/// `Number(x.toFixed(1))`: the temperature as it is reported and compared, not as it was computed.
pub(crate) fn round1(x: f64) -> f64 {
    to_fixed(x, 1).parse::<f64>().unwrap_or(x)
}

/// `String(number)` for a figure inside a message. Rust prints a float the shortest way the web app
/// does for every value here; only negative zero differs.
fn js_num(x: f64) -> String {
    if x == 0.0 { "0".to_string() } else { format!("{x}") }
}

/// `String.prototype.trim`: Unicode space and line ends plus the byte-order mark.
fn trim_js(s: &str) -> &str {
    s.trim_matches(|c: char| is_js_whitespace(c) || c == '\u{feff}')
}

// ---------------------------------------------------------------------------
// Tm — exact Biopython Tm_NN(DNA_NN3, Na=50, dnac1=250, dnac2=0, saltcorr=5).
//
// This is the "Gibson Tm" the dnakit workflows use. It differs from [`crate::seq::tm`] only in the
// strand-concentration term (k = 250e-9 here vs Ct/4 there) and by never applying a
// self-complementary symmetry correction (Biopython's selfcomp defaults False) — which is why it
// cannot go through that function. The NN table and the SantaLucia-1998 salt correction are
// numerically identical to it.
// ---------------------------------------------------------------------------

/// Gas constant, cal/(mol·K).
const R_GAS: f64 = 1.987;
/// `(dnac1 - dnac2/2) * 1e-9` with dnac1=250, dnac2=0.
const GIBSON_K: f64 = 250e-9;
/// 50 mM Na+ as molar.
const GIBSON_NA: f64 = 0.05;

/// ΔH (kcal/mol) / ΔS (cal/mol·K) of one nearest-neighbour pair: SantaLucia 1998 unified.
fn nearest_neighbour(first: u8, second: u8) -> Option<(f64, f64)> {
    Some(match (first, second) {
        (b'A', b'A') | (b'T', b'T') => (-7.9, -22.2),
        (b'A', b'T') => (-7.2, -20.4),
        (b'T', b'A') => (-7.2, -21.3),
        (b'C', b'A') | (b'T', b'G') => (-8.5, -22.7),
        (b'G', b'T') | (b'A', b'C') => (-8.4, -22.4),
        (b'C', b'T') | (b'A', b'G') => (-7.8, -21.0),
        (b'G', b'A') | (b'T', b'C') => (-8.2, -22.2),
        (b'C', b'G') => (-10.6, -27.2),
        (b'G', b'C') => (-9.8, -24.4),
        (b'G', b'G') | (b'C', b'C') => (-8.0, -19.9),
        _ => return None,
    })
}

/// Nearest-neighbour Tm (°C) with the exact conditions dnakit uses for Gibson overlap design.
/// `None` for sequences shorter than 2 nt or holding non-ACGT bases: an uncertain arm has no scalar
/// estimate.
pub fn gibson_tm(seq: &str) -> Option<f64> {
    let s = clean_seq(seq);
    let bases = s.as_bytes();
    let unsupported = seq.chars().any(|c| !(matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T') || is_js_whitespace(c) || c.is_ascii_digit()));
    if bases.len() < 2 || unsupported {
        return None;
    }
    let mut dh = 0.0;
    let mut ds = 0.0;
    // Terminal initiation: init_A/T=[2.3,4.1] per A/T end, init_G/C=[0.1,-2.8] per G/C end.
    for end in [*bases.first()?, *bases.last()?] {
        if matches!(end, b'G' | b'C') {
            dh += 0.1;
            ds += -2.8;
        } else {
            dh += 2.3;
            ds += 4.1;
        }
    }
    for pair in bases.windows(2) {
        let (pair_h, pair_s) = nearest_neighbour(*pair.first()?, *pair.last()?)?;
        dh += pair_h;
        ds += pair_s;
    }
    // saltcorr=5: ΔS += 0.368·(N-1)·ln[Na+]
    ds += 0.368 * (bases.len() - 1) as f64 * GIBSON_NA.ln();
    Some((1000.0 * dh) / (ds + R_GAS * GIBSON_K.ln()) - 273.15)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmRangeResult {
    /// 0-based exclusive end ([`tm_range`]) or inclusive start ([`tm_range_reverse`]).
    pub pos: i64,
    pub tm: f64,
}

/// Grow a subsequence rightward from `start` until it reaches `target_tm`. Returns the shortest
/// window ≥ target, or the longest available window (capped at `max_len` / the end of the sequence)
/// when the target is unreachable.
pub fn tm_range(seq: &str, start: i64, target_tm: f64, min_len: i64, max_len: i64) -> TmRangeResult {
    let n = len64(seq);
    for length in min_len..=max_len {
        let end = start + length;
        if end > n {
            return TmRangeResult { pos: n, tm: gibson_tm(js_slice(seq, start, n)).unwrap_or(0.0) };
        }
        let cur = gibson_tm(js_slice(seq, start, end)).unwrap_or(0.0);
        if cur >= target_tm {
            return TmRangeResult { pos: end, tm: cur };
        }
    }
    let end = start + max_len.min(n - start);
    TmRangeResult { pos: end, tm: gibson_tm(js_slice(seq, start, end)).unwrap_or(0.0) }
}

/// Grow a subsequence leftward from the exclusive `end` to `target_tm`.
pub fn tm_range_reverse(seq: &str, end: i64, target_tm: f64, min_len: i64, max_len: i64) -> TmRangeResult {
    for length in min_len..=max_len {
        let start = end - length;
        if start < 0 {
            return TmRangeResult { pos: 0, tm: gibson_tm(js_slice(seq, 0, end)).unwrap_or(0.0) };
        }
        let cur = gibson_tm(js_slice(seq, start, end)).unwrap_or(0.0);
        if cur >= target_tm {
            return TmRangeResult { pos: start, tm: cur };
        }
    }
    let start = (end - max_len).max(0);
    TmRangeResult { pos: start, tm: gibson_tm(js_slice(seq, start, end)).unwrap_or(0.0) }
}

// ---------------------------------------------------------------------------
// Validation primitives.
// ---------------------------------------------------------------------------

/// Longest suffix of `a` that equals a prefix of `b` (case-insensitive), within
/// `[min_len, max_len]`. Returns the overlap in `a`'s case, or "" if none.
pub fn extract_junction_overlap(a: &str, b: &str, min_len: i64, max_len: i64) -> String {
    let la = len64(a);
    let upper = max_len.min(la).min(len64(b));
    if upper < min_len {
        return String::new();
    }
    let mut size = upper;
    while size >= min_len {
        if js_from(a, la - size).to_lowercase() == js_slice(b, 0, size).to_lowercase() {
            return js_from(a, la - size).to_string();
        }
        size -= 1;
    }
    String::new()
}

/// Forward and reverse-complement hit counts of an overlap in a plasmid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uniqueness {
    pub fwd: usize,
    pub rc: usize,
}

/// Count occurrences of `overlap` in `plasmid` on the + strand and the − strand. Circular plasmids
/// wrap by `qlen - 1` so an origin-straddling hit counts once. The critical Gibson check: forward
/// hits must equal 1.
pub fn check_uniqueness(plasmid: &str, overlap: &str, circular: bool) -> Uniqueness {
    let p = plasmid.to_uppercase();
    let q = overlap.to_uppercase();
    let l = p.len();
    let qlen = len64(&q);
    if qlen == 0 {
        return Uniqueness { fwd: 0, rc: 0 };
    }
    let search = if circular && qlen > 1 { format!("{p}{}", js_slice(&p, 0, qlen - 1)) } else { p };
    let rcq = revcomp(&q);
    let is_palindrome = q == rcq;
    let mut seen: HashSet<(usize, bool)> = HashSet::new();
    let fwd = scan_hits(&search, &q, l, true, &mut seen);
    let rc = if is_palindrome { 0 } else { scan_hits(&search, &rcq, l, false, &mut seen) };
    Uniqueness { fwd, rc }
}

/// Every distinct position of `needle` in `search`, counted once per place on the molecule.
fn scan_hits(search: &str, needle: &str, l: usize, forward: bool, seen: &mut HashSet<(usize, bool)>) -> usize {
    let mut count = 0;
    let mut from = 0usize;
    while let Some(rel) = search.get(from..).and_then(|rest| rest.find(needle)) {
        let idx = from + rel;
        if seen.insert((if l > 0 { idx % l } else { idx }, forward)) {
            count += 1;
        }
        from = idx + 1;
    }
    count
}

/// An adjacent tandem direct repeat at the 3′ end: the signature of a Gibson overlap
/// double-appended during assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TandemDuplicate {
    pub start: i64,
    pub len: i64,
    pub unit: String,
}

/// Detect an adjacent tandem direct repeat at the 3′ end. Returns the smallest-N hit, or `None`.
pub fn detect_tandem_duplicate(seq: &str, min_dup_len: i64, scan3prime_bp: i64) -> Option<TandemDuplicate> {
    if seq.is_empty() {
        return None;
    }
    let n = len64(seq);
    let max_n = scan3prime_bp.div_euclid(2).min(n.div_euclid(2));
    if max_n < min_dup_len {
        return None;
    }
    let lower = seq.to_lowercase();
    let mut k = min_dup_len;
    while k <= max_n {
        if 2 * k > n {
            break;
        }
        if js_from(&lower, n - k) == js_slice(&lower, n - 2 * k, n - k) {
            return Some(TandemDuplicate { start: n - 2 * k, len: k, unit: js_slice(seq, n - 2 * k, n - k).to_string() });
        }
        k += 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightLevel {
    Fail,
    Warn,
}

impl std::fmt::Display for PreflightLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreflightLevel::Fail => write!(f, "FAIL"),
            PreflightLevel::Warn => write!(f, "WARN"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightCode {
    LengthMismatch,
    LengthOutOfRange,
    GcOutOfRange,
    Homopolymer,
    TandemDup3Prime,
}

impl std::fmt::Display for PreflightCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreflightCode::LengthMismatch => write!(f, "length_mismatch"),
            PreflightCode::LengthOutOfRange => write!(f, "length_out_of_range"),
            PreflightCode::GcOutOfRange => write!(f, "gc_out_of_range"),
            PreflightCode::Homopolymer => write!(f, "homopolymer"),
            PreflightCode::TandemDup3Prime => write!(f, "tandem_dup_3prime"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightIssue {
    pub level: PreflightLevel,
    pub code: PreflightCode,
    pub detail: String,
}

/// IDT eBlock ordering preflight.
pub fn idt_preflight(name: &str, seq: &str, declared_len: Option<i64>) -> Vec<PreflightIssue> {
    let mut issues: Vec<PreflightIssue> = Vec::new();
    let len = len64(seq);
    if let Some(declared) = declared_len
        && (len - declared).abs() > 1
    {
        let detail = format!("{name}: declared_len={declared} but len(seq)={len} (delta={})", len - declared);
        issues.push(PreflightIssue { level: PreflightLevel::Fail, code: PreflightCode::LengthMismatch, detail });
    }
    if !(300..=3000).contains(&len) {
        let detail = format!("{name}: len(seq)={len} is outside IDT eBlock range 300..3000 bp");
        issues.push(PreflightIssue { level: PreflightLevel::Warn, code: PreflightCode::LengthOutOfRange, detail });
    }
    if len > 0 {
        let u = seq.to_uppercase();
        let gcc = u.chars().filter(|&c| c == 'G' || c == 'C').count();
        let gc = (100.0 * gcc as f64) / len as f64;
        if !(25.0..=65.0).contains(&gc) {
            let detail = format!("{name}: GC%={} is outside 25..65", to_fixed(gc, 2));
            issues.push(PreflightIssue { level: PreflightLevel::Warn, code: PreflightCode::GcOutOfRange, detail });
        }
        homopolymer_issues(name, &u, &mut issues);
    }
    if let Some(tandem) = detect_tandem_duplicate(seq, 20, 100) {
        let detail = format!("{name}: tandem duplicate at pos {}, dup_len={} bp, unit={}", tandem.start, tandem.len, tandem.unit);
        issues.push(PreflightIssue { level: PreflightLevel::Fail, code: PreflightCode::TandemDup3Prime, detail });
    }
    issues
}

/// Homopolymer runs of 13 bp or more, in the order they appear.
fn homopolymer_issues(name: &str, upper: &str, issues: &mut Vec<PreflightIssue>) {
    let bytes = upper.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(&b) = bytes.get(i) else { break };
        if matches!(b, b'A' | b'C' | b'G' | b'T') {
            let mut j = i;
            while bytes.get(j) == Some(&b) {
                j += 1;
            }
            let run = j - i;
            if run >= 13 {
                let detail = format!("{name}: {} x {run} at pos {i}", char::from(b));
                issues.push(PreflightIssue { level: PreflightLevel::Warn, code: PreflightCode::Homopolymer, detail });
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// eBlock splitting.
// ---------------------------------------------------------------------------

/// The overlap joining a piece to the one before it.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinOverlap {
    pub seq: String,
    pub tm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EblockPiece {
    pub name: String,
    pub order: String,
    pub gc: f64,
    /// Overlap with the previous piece; `None` for the first.
    pub join_prev: Option<JoinOverlap>,
    /// Where this piece sits in the string it was split from, 0-based half-open.
    ///
    /// Consecutive pieces OVERLAP — `start` of one is before `end` of the last, by exactly the
    /// homology they share — so this is not a partition and must not be drawn as one. That overlap
    /// is the assembly, and a map that hides it is hiding the only reason the pieces join.
    pub start: i64,
    pub end: i64,
}

/// The overlap the extreme fallback gives consecutive pieces when the Tm walk cannot place one.
const FALLBACK_OVERLAP: i64 = 30;

// ---------------------------------------------------------------------------
// MATCHED OVERLAPS — every junction at the same temperature.
//
// A Gibson reaction runs at ONE temperature, so overlaps that melt at 65, 66 and 77 °C are not
// three fine junctions; they are one reaction in which the 77 never breathes and the 65 never
// closes. What matters is not that each overlap hit a target on its own — it is that they MATCH.
//
// Growing each overlap rightward from a fixed point until it passes the target, with a floor of
// 18 bp, cannot cool a junction standing in a GC-rich stretch: even 18 bp is already 77 °C and
// there is nothing shorter to try. Two freedoms fix it, in this order:
//   • LENGTH, everywhere. A junction that must stay put can still choose how long its overlap is.
//   • POSITION, where we own it. The boundaries INSIDE one ordered fragment are ours to place, so
//     they slide until they find a stretch that melts at the shared temperature.
// Where the two disagree the fixed junctions win: they set what is achievable. If nothing can reach
// the asked-for temperature, everyone drops to the coolest one that all of them can reach.
// ---------------------------------------------------------------------------

/// Overlap length bounds. The same 18–60 the arms have always used.
const OL_MIN: i64 = 18;
const OL_MAX: i64 = 60;
/// How far a boundary we own may slide, in bp, looking for a stretch that melts at the target.
const JUNCTION_WIGGLE: i64 = 220;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlapChoice {
    /// Where the overlap starts, in the string it was chosen from.
    pub pos: i64,
    pub len: i64,
    pub tm: f64,
}

/// The GC percentage of a stretch, 0 for an empty one.
pub(crate) fn gc_of(s: &str) -> f64 {
    let u = s.to_uppercase();
    let len = u.encode_utf16().count();
    if len == 0 {
        return 0.0;
    }
    let g = u.chars().filter(|&c| c == 'G' || c == 'C').count();
    100.0 * g as f64 / len as f64
}

/// Every (position, length) overlap in a window, with its Tm. `from == to` is the fixed case: one
/// position, and only the length to choose.
fn overlap_options(full: &str, from: i64, to: i64) -> Vec<OverlapChoice> {
    let n = len64(full);
    let mut out = Vec::new();
    let mut pos = from.max(0);
    let last = to.min(n - OL_MIN);
    while pos <= last {
        for len in OL_MIN..=OL_MAX {
            if pos + len > n {
                break;
            }
            if let Some(tm) = gibson_tm(js_slice(full, pos, pos + len)).filter(|t| t.is_finite()) {
                out.push(OverlapChoice { pos, len, tm });
            }
        }
        pos += 1;
    }
    out
}

/// The coolest and hottest this junction can be made to run.
fn reachable(opts: &[OverlapChoice]) -> Option<(f64, f64)> {
    if opts.is_empty() {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for o in opts {
        if o.tm < lo {
            lo = o.tm;
        }
        if o.tm > hi {
            hi = o.tm;
        }
    }
    Some((lo, hi))
}

/// The option nearest `target`, approached FROM BELOW.
///
/// Nearest-in-absolute-value is not good enough: Tm moves in steps of about a degree per base, so
/// two junctions can each be "closest" to 65 while sitting at 64.3 and 65.7 — each individually
/// right, 1.4 °C apart, which is the thing we are trying to remove. Preferring the cooler side puts
/// every junction on the same side of the target. A junction whose whole range is hot takes the
/// coolest it has.
fn pick_at(opts: &[OverlapChoice], target: f64) -> Option<OverlapChoice> {
    let mut below: Option<OverlapChoice> = None;
    let mut above: Option<OverlapChoice> = None;
    for &o in opts {
        if o.tm <= target + 1e-9 {
            if below.is_none_or(|b| o.tm > b.tm || (o.tm == b.tm && o.len < b.len)) {
                below = Some(o);
            }
        } else if above.is_none_or(|a| o.tm < a.tm || (o.tm == a.tm && o.len < a.len)) {
            above = Some(o);
        }
    }
    below.or(above)
}

/// Settle a set of junctions on ONE temperature: pick at `target`, then drop everyone to the
/// coolest of those picks and pick again.
fn settle(windows: &[Vec<OverlapChoice>], target: f64) -> Vec<OverlapChoice> {
    let first: Vec<Option<OverlapChoice>> = windows.iter().map(|w| pick_at(w, target)).collect();
    if first.iter().any(Option::is_none) {
        return first.into_iter().flatten().collect();
    }
    let lowest = first.iter().flatten().map(|c| c.tm).fold(f64::INFINITY, f64::min);
    windows.iter().zip(&first).filter_map(|(w, f)| pick_at(w, lowest).or(*f)).collect()
}

/// The one temperature every junction will be held to.
///
/// `fixed` are the junctions that cannot move — the two backbone arms, and every boundary between
/// fragments the user actually typed. They decide what is possible. Aim for `target`; if some
/// junction cannot be cooled that far, everyone rises to the warmest floor, and if some cannot be
/// warmed that far, everyone drops to the coolest ceiling.
fn shared_tm(target: f64, fixed: &[Option<(f64, f64)>]) -> f64 {
    let real: Vec<(f64, f64)> = fixed.iter().flatten().copied().collect();
    if real.is_empty() {
        return target;
    }
    let floor = real.iter().map(|r| r.0).fold(f64::NEG_INFINITY, f64::max);
    let ceil = real.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    // No temperature suits everyone: take the warmest floor, which is the coolest any of them can
    // actually reach. Whoever can go lower will simply be the coolest in the set.
    if floor > ceil {
        return floor;
    }
    target.max(floor).min(ceil)
}

/// Split with the boundaries free to slide, so every internal junction runs at `target_tm`.
///
/// The cap is a ceiling on each piece, not a demand that they be equal — so a 2,500 bp strand at a
/// 1,400 bp cap may come out 1,300 + 1,250 rather than 1,250 + 1,250, if that is where the
/// temperature is. `None` when no piece count from 2..12 leaves room to slide AND fit.
fn split_matched(full: &str, target_tm: f64, max_len: i64, base_name: &str) -> Option<Vec<EblockPiece>> {
    let n = len64(full);
    for k in 2usize..=12 {
        let Some(windows) = matched_windows(full, max_len, k) else { continue };
        let reach: Vec<Option<(f64, f64)>> = windows.iter().map(|w| reachable(w)).collect();
        let shared = shared_tm(target_tm, &reach);
        let chosen = settle(&windows, shared);
        if chosen.len() != windows.len() {
            continue;
        }
        let mut pieces = Vec::with_capacity(k);
        let mut fits = true;
        for idx in 0..k {
            let start = if idx == 0 { 0 } else { chosen.get(idx - 1).map_or(0, |c| c.pos) };
            let end = if idx == k - 1 { n } else { chosen.get(idx).map_or(n, |c| c.pos + c.len) };
            let order = js_slice(full, start, end);
            if len64(order) > max_len {
                fits = false;
                break;
            }
            let join_prev = idx
                .checked_sub(1)
                .and_then(|prev| chosen.get(prev))
                .map(|c| JoinOverlap { seq: js_slice(full, c.pos, c.pos + c.len).to_string(), tm: round1(c.tm) });
            pieces.push(EblockPiece { name: format!("{base_name}_{}", idx + 1), order: order.to_string(), gc: gc_of(order), join_prev, start, end });
        }
        if fits {
            return Some(pieces);
        }
    }
    None
}

/// What each internal boundary of a `k`-way split can reach.
///
/// TWO PASSES. This collects what each boundary can do; [`settle`] then picks, all of them aiming
/// at the same number. One pass would let each boundary chase the target independently, and where
/// the target is out of reach they would settle at different temperatures.
fn matched_windows(full: &str, max_len: i64, k: usize) -> Option<Vec<Vec<OverlapChoice>>> {
    let n = len64(full);
    let seg = div_ceil(n, i64::try_from(k).unwrap_or(1));
    let k64 = i64::try_from(k).unwrap_or(1);
    let mut windows = Vec::with_capacity(k - 1);
    for j in 1..k64 {
        let nominal = j * seg;
        // THE REAL WINDOW, not a symmetric guess. Sliding a boundary right grows the piece behind
        // it and shrinks the piece ahead, so the room is bounded by its two NEIGHBOURS, not by half
        // the leftover slack. (Assuming both neighbours at nominal costs a little reach and keeps
        // this a one-pass placement; the piece lengths are validated by the caller regardless.)
        let prev = if j == 1 { 0 } else { (j - 1) * seg };
        let next = if j == k64 - 1 { n } else { (j + 1) * seg };
        let lo = (prev + OL_MIN).max(next - max_len).max(nominal - JUNCTION_WIGGLE);
        let hi = (prev + max_len - OL_MAX).min(next - OL_MIN).min(nominal + JUNCTION_WIGGLE);
        if hi < lo {
            return None;
        }
        let opts = overlap_options(full, lo, hi);
        if opts.is_empty() {
            return None;
        }
        windows.push(opts);
    }
    Some(windows)
}

/// Split a full ordered eBlock sequence into consecutive pieces each ≤ `max_len`, where adjacent
/// pieces share a homology overlap sized to `target_tm` (min 20 bp). The pieces reassemble (via
/// [`extract_junction_overlap`]) back into `full`.
///
/// `match_overlaps` lets the boundaries SLIDE to find stretches that melt at `target_tm`, instead of
/// cutting the strand into equal parts and taking whatever temperature happens to be there. The
/// even-boundary walk is what dnakit does and what the golden vectors pin; the app turns matching
/// on, because a reaction runs at one temperature.
pub fn split_eblock(full: &str, target_tm: f64, max_len: i64, base_name: &str, match_overlaps: bool) -> Result<Vec<EblockPiece>, GibsonError> {
    let n = len64(full);
    if n <= max_len {
        return Ok(vec![EblockPiece { name: base_name.to_string(), order: full.to_string(), gc: gc_of(full), join_prev: None, start: 0, end: n }]);
    }
    // A CAP THIS SMALL CANNOT TILE, AND ASKING IT TO USED TO HANG THE PROCESS. The fallback below
    // steps by `max_len - 30`; at max_len <= 30 that step is zero or negative, so the loop never
    // advances. It was reachable by typing — every intermediate value on the way to 3000 is a real
    // request — so say so rather than clamp to something nobody asked for.
    if max_len <= FALLBACK_OVERLAP {
        return Err(GibsonError::EblockCapBelowOverlap { max_len });
    }
    // MATCHED: place the boundaries where the temperature is right, not where the arithmetic says.
    // When no placement satisfies the cap, fall through to the even-boundary search below, which is
    // free to be uneven in temperature but is guaranteed to fit.
    if match_overlaps && let Some(matched) = split_matched(full, target_tm, max_len, base_name) {
        return Ok(matched);
    }
    // The smallest piece count k whose evenly-placed boundaries (plus their homology overlaps) keep
    // every piece within max_len.
    for k in 2usize..=12 {
        if let Some(pieces) = even_split(full, target_tm, max_len, base_name, k) {
            return Ok(pieces);
        }
    }
    Ok(hard_chunks(full, max_len, base_name))
}

/// One `k`-way even-boundary attempt, or `None` when a piece would blow the cap.
fn even_split(full: &str, target_tm: f64, max_len: i64, base_name: &str, k: usize) -> Option<Vec<EblockPiece>> {
    let n = len64(full);
    let seg = div_ceil(n, i64::try_from(k).unwrap_or(1));
    let boundaries: Vec<i64> = (1..k).map(|j| (i64::try_from(j).unwrap_or(0) * seg).min(n - 20)).collect();
    // Overlap length at each boundary (grow rightward to target_tm, min 20 bp).
    let overlap_len: Vec<i64> = boundaries.iter().map(|&b| tm_range(full, b, target_tm, 20, 60).pos - b).collect();
    let mut pieces = Vec::with_capacity(k);
    for idx in 0..k {
        let start = if idx == 0 { 0 } else { boundaries.get(idx - 1).copied().unwrap_or(0) };
        let end = if idx == k - 1 { n } else { boundaries.get(idx).copied().unwrap_or(n) + overlap_len.get(idx).copied().unwrap_or(0) };
        let order = js_slice(full, start, end);
        if len64(order) > max_len {
            return None;
        }
        let join_prev = idx.checked_sub(1).map(|prev| {
            let b = boundaries.get(prev).copied().unwrap_or(0);
            let ol = js_slice(full, b, b + overlap_len.get(prev).copied().unwrap_or(0));
            JoinOverlap { seq: ol.to_string(), tm: round1(gibson_tm(ol).unwrap_or(0.0)) }
        });
        pieces.push(EblockPiece { name: format!("{base_name}_{}", idx + 1), order: order.to_string(), gc: gc_of(order), join_prev, start, end });
    }
    Some(pieces)
}

/// Extreme fallback: hard chunks with 30 bp overlaps, no Tm guarantee.
fn hard_chunks(full: &str, max_len: i64, base_name: &str) -> Vec<EblockPiece> {
    let n = len64(full);
    let mut pieces = Vec::new();
    // Guarded by the caller, and guarded again here: this is the line whose sign decides whether
    // the loop terminates, so it does not depend on a check made 40 lines away staying true.
    let step = (max_len - FALLBACK_OVERLAP).max(1);
    let mut s = 0i64;
    let mut idx = 0usize;
    while s < n {
        let end = (s + max_len).min(n);
        let order = js_slice(full, s, end);
        let join_prev = (idx > 0).then(|| {
            let ol = js_slice(full, s, s + FALLBACK_OVERLAP);
            JoinOverlap { seq: ol.to_string(), tm: round1(gibson_tm(ol).unwrap_or(0.0)) }
        });
        pieces.push(EblockPiece { name: format!("{base_name}_{}", idx + 1), order: order.to_string(), gc: gc_of(order), join_prev, start: s, end });
        if s + max_len >= n {
            break;
        }
        s += step;
        idx += 1;
    }
    pieces
}

// ---------------------------------------------------------------------------
// The opening: where the backbone comes apart.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GibsonCutSite {
    pub enzyme: String,
    /// 0-based cut position on the top strand.
    pub position: i64,
    /// 0-based leftmost base of the recognition match, as `find_cut_sites` reports it.
    ///
    /// Optional, and its absence is the old behaviour: without it there is nothing to hold back, so
    /// `keep_sites` cannot apply and the scissile bond is the boundary.
    pub recognition_start: Option<i64>,
}

/// Two cuts on a circle define TWO arcs; this says which one leaves. `Between` is the arc from the
/// lower position to the higher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningArc {
    Between,
    Origin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GibsonCut {
    /// Resolved cut sites (from `find_cut_sites`).
    Enzymes {
        enzymes: [String; 2],
        sites: Vec<GibsonCutSite>,
        /// `None` is the unqualified pair, which means `Between` and warns when the other arc is
        /// the smaller one.
        arc: Option<OpeningArc>,
        /// KEEP THE SITE. Open the backbone OUTSIDE each recognition site (plus a base of margin)
        /// instead of at the scissile bond inside it, so the product still carries both sites. Off
        /// in the golden vectors, which pin dnakit's cut-position walk; the app turns it on.
        keep_sites: bool,
    },
    /// 0-based INCLUSIVE region to replace. `end < start` means it wraps the origin.
    Region { start: i64, end: i64 },
    /// 0-based pure insertion — nothing is removed. The region form cannot express this: it is
    /// inclusive, so `{start: 800, end: 800}` takes out a base.
    InsertAt(i64),
}

/// Both arcs the two boundaries define, so a caller can name the choice it just made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpeningArcs {
    pub between: i64,
    pub origin: i64,
}

/// Where the backbone comes apart.
///
/// The removed span is the half-open `[up, down)`, or `[up, len) ∪ [0, down)` when `down < up` —
/// the same "end < start means it wraps" convention features already use, so no code between here
/// and the assembled map has to translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GibsonOpening {
    pub up: i64,
    pub down: i64,
    /// `down < up` — the opening runs through the origin.
    pub wraps: bool,
    /// Bases removed. NOT `down - up`, which is negative when the opening wraps.
    pub removed_len: i64,
    pub arcs: OpeningArcs,
    pub enzymes: Option<[String; 2]>,
    pub sites: Vec<GibsonCutSite>,
    /// The enzyme whose site sits at the upstream boundary of the removed arc.
    ///
    /// Named here rather than re-derived by matching a position, because under `keep_sites` the
    /// boundary is no longer AT a cut position and every such match silently returns nothing.
    pub up_enzyme: Option<String>,
    pub down_enzyme: Option<String>,
    /// Enzymes whose recognition sites survive on the product, for the console to name.
    pub kept_sites: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// KEEP THE SITE — ALL OF IT, FOR EVERY ENZYME.
//
// An enzyme cuts INSIDE or BESIDE the sequence it reads, so opening the backbone at the scissile
// bond leaves whatever part of the recognition site falls in the excised arc to leave with it. We
// SYNTHESIZE the whole insert, so keeping the site costs nothing: the arm was going to be 18–60 bp
// of backbone anyway, and the site simply falls inside it. The digest still ends the backbone at
// the scissile bond; the bases between that bond and the site's far edge sit INSIDE the eBlock,
// where polymerase copies them back onto the backbone's 3′ end. That is why the arm is sized on the
// stretch that actually anneals (see `beyond_cut`) rather than on the whole arm.
//
// THE RULE: THE REMOVED ARC MAY NOT TOUCH EITHER RECOGNITION SITE, AND NEITHER BOUNDARY MAY LAND
// INSIDE ONE. Everything else falls out of it — blunt, 5′ and 3′ ends read the same; Type IIS is
// kept like anything else (the enzyme linearises the backbone and is never in the reaction, so a
// retained site is a site you can cut again); a site on the minus strand is kept; and when two
// enzymes read overlapping DNA (Acc65I with KpnI, both GGTACC) the answer is a pure insertion at
// the far edge of the pair, not a four-base "opening" that destroys both.
// ---------------------------------------------------------------------------

/// The margin the boundary prefers to leave beside a site. A preference: it is dropped when keeping
/// both sites needs the room, and walked when a junction would otherwise spell a second copy of a
/// site the backbone carried once.
pub const SITE_MARGIN: i64 = 1;

/// The bases one enzyme reads: `len` of them starting at `start`, wrapping the origin if it must.
#[derive(Clone, Copy, Debug)]
struct SiteSpan {
    start: i64,
    len: i64,
}

/// The arithmetic every boundary decision does on one molecule.
#[derive(Clone, Copy)]
struct Circle {
    len: i64,
    circular: bool,
}

impl Circle {
    fn clamp(self, x: i64) -> i64 {
        x.max(0).min(self.len)
    }

    /// Walking 5'→3' from `up` to `down`. Equal ⇒ nothing removed, never the whole molecule.
    fn span(self, up: i64, down: i64) -> i64 {
        if down >= up { down - up } else { self.len - up + down }
    }

    fn arcs_for(self, a: i64, b: i64) -> OpeningArcs {
        let between = (b - a).abs();
        OpeningArcs { between, origin: self.len - between }
    }

    /// Where a boundary lands once it has been pushed outside a recognition site. A circular
    /// backbone wraps — a site can straddle the origin — and a linear one clamps, because it has no
    /// origin to come back round through.
    fn place(self, x: i64) -> i64 {
        if self.len <= 0 {
            return 0;
        }
        if self.circular { x.rem_euclid(self.len) } else { self.clamp(x) }
    }
}

/// The two boundaries of one arc, and the site standing at each.
struct Ends<'a> {
    up_pos: i64,
    down_pos: i64,
    up_site: Option<&'a GibsonCutSite>,
    down_site: Option<&'a GibsonCutSite>,
}

/// What one arc's boundaries resolved to under whichever rule is in force.
struct Bounds {
    up: i64,
    down: i64,
    kept: Vec<String>,
    note: Option<String>,
}

/// Everything `resolve_bounds` needs about the pair of cuts it is holding back.
struct Keep<'a> {
    circle: Circle,
    enzymes: &'a [String; 2],
    lo: i64,
    hi: i64,
    first: Option<&'a GibsonCutSite>,
    last: Option<&'a GibsonCutSite>,
    keep_sites: bool,
    margin: i64,
}

/// The bases an enzyme reads, or `None` when there is nothing to hold back.
///
/// EVERY enzyme, Type IIS included: where the enzyme cuts relative to what it reads is the caller's
/// business, not the boundary's.
fn site_span(site: Option<&GibsonCutSite>) -> Option<SiteSpan> {
    let site = site?;
    let start = site.recognition_start?;
    let enz = get_enzyme(&site.enzyme)?;
    Some(SiteSpan { start, len: i64::try_from(enz.length).unwrap_or(0) })
}

impl Keep<'_> {
    /// Which site stands at each end of the removed arc. TAKEN FROM THE ORDER, NOT BY MATCHING A
    /// POSITION. Two enzymes can cut at the SAME base — ChaI GATC^ and TrsKTIII CA^TATG do it
    /// wherever GATCATATG appears — and a lookup by cut position then answered the first entry for
    /// both ends, so the second enzyme was never held back.
    fn ends(&self, a: OpeningArc) -> Ends<'_> {
        let origin = a == OpeningArc::Origin;
        Ends {
            up_pos: if origin { self.hi } else { self.lo },
            down_pos: if origin { self.lo } else { self.hi },
            up_site: if origin { self.last } else { self.first },
            down_site: if origin { self.first } else { self.last },
        }
    }

    /// Does the arc that leaves take any base this enzyme READS? Asked base by base, because a site
    /// can straddle the origin and a site can sit either side of its own cut.
    fn eaten(&self, s: SiteSpan, up: i64, down: i64) -> bool {
        let len = self.circle.span(up, down);
        (0..s.len).any(|i| self.circle.span(up, self.circle.place(s.start + i)) < len)
    }

    /// Would a boundary here cut a site in half? A boundary at either EDGE of a site is fine.
    fn splits(&self, held: &[SiteSpan], x: i64) -> bool {
        held.iter().any(|s| {
            let off = self.circle.span(self.circle.place(s.start), x);
            off > 0 && off < s.len
        })
    }

    /// The boundaries of the removed arc, under whichever rule is in force.
    ///
    /// Returned per-arc rather than computed once, because `arcs` has to report what each choice
    /// would actually remove — and under `keep_sites` the two arcs no longer simply partition the
    /// molecule, they are each a little shorter than the cut positions suggest.
    fn resolve_bounds(&self, a: OpeningArc) -> Bounds {
        let e = self.ends(a);
        let plain = Bounds { up: e.up_pos, down: e.down_pos, kept: Vec::new(), note: None };
        if !self.keep_sites {
            return plain;
        }
        let (u, d) = (site_span(e.up_site), site_span(e.down_site));
        if u.is_none() && d.is_none() {
            return plain;
        }
        let held: Vec<SiteSpan> = [u, d].into_iter().flatten().collect();
        let kept: Vec<String> =
            [u.and(e.up_site), d.and(e.down_site)].into_iter().flatten().map(|s| s.enzyme.clone()).filter(|n| !n.is_empty()).collect();
        // Holding a site back can only ever remove LESS, so a span that GREW means the two
        // boundaries crossed each other and this margin cannot be afforded.
        let raw = self.circle.span(e.up_pos, e.down_pos);
        for m in [self.margin, 0] {
            let mut up = e.up_pos;
            let mut down = e.down_pos;
            if let Some(u) = u
                && self.eaten(u, up, down)
            {
                up = self.circle.place(u.start + u.len + m);
            }
            if let Some(d) = d
                && self.eaten(d, up, down)
            {
                down = self.circle.place(d.start - m);
            }
            if self.circle.span(up, down) > raw || self.splits(&held, up) || self.splits(&held, down) {
                continue;
            }
            let note = (up == down && raw > 0).then(|| {
                format!(
                    "{} sit next to each other, so keeping both sites whole leaves nothing between them to remove — this is a pure insertion between the two sites.",
                    kept.join(" and ")
                )
            });
            return Bounds { up, down, kept, note };
        }
        self.insert_between(&held, e.up_pos, kept, plain)
    }

    /// NOTHING CAN COME OUT WITHOUT BREAKING A SITE: the two enzymes read overlapping DNA. They are
    /// isoschizomers (Acc65I and KpnI are both GGTACC), or one site is nested in the other, so there
    /// was never a stretch "between the cuts" to remove. Put the insert in whole, at the first place
    /// that splits neither, WALKED forward from the opening the user asked for — on a circle "the
    /// far edge of the pair" is only meaningful once you know which way round the pair runs, and
    /// reading it off one site's far edge sent the point into the other one.
    fn insert_between(&self, held: &[SiteSpan], up_pos: i64, kept: Vec<String>, plain: Bounds) -> Bounds {
        let reach = held.iter().map(|x| x.len).max().unwrap_or(0) * 2 + 2;
        for step in 0..=reach {
            let at = self.circle.place(up_pos + step);
            if self.splits(held, at) {
                continue;
            }
            let note = format!(
                "{} read overlapping DNA, so there is nothing between them to remove — the insert goes in whole at the first place that splits neither site, and both are kept.",
                self.enzymes.join(" and ")
            );
            return Bounds { up: at, down: at, kept, note: Some(note) };
        }
        // Unreachable for two sites (they are at most a couple of dozen bases together, and the walk
        // covers more than that). Kept honest rather than deleted: if it ever fires, say what was lost.
        let note = format!(
            "The {} sites cover every base around the opening, so neither can be kept whole — the backbone opens at the cut positions and both sites are lost.",
            self.enzymes.join(" and ")
        );
        Bounds { note: Some(note), ..plain }
    }
}

/// Resolve `cut` into one opening, for both designers and for the UI.
///
/// This used to be four copies of the same rule, and every one of them sorted the two cut positions
/// ascending and took first→last. On a circular plasmid that is a silent wrong answer: EcoRI at 4500
/// and BamHI at 200 removed the 4300 bp middle instead of the 700 bp through the origin.
pub fn resolve_gibson_opening(
    backbone_len: i64,
    circular: bool,
    cut: &GibsonCut,
    warnings: &mut Vec<String>,
    margin: i64,
) -> Result<GibsonOpening, GibsonError> {
    let circle = Circle { len: backbone_len, circular };
    match cut {
        GibsonCut::InsertAt(at) => {
            let at = circle.clamp(*at);
            Ok(GibsonOpening {
                up: at,
                down: at,
                wraps: false,
                removed_len: 0,
                arcs: circle.arcs_for(at, at),
                enzymes: None,
                sites: Vec::new(),
                up_enzyme: None,
                down_enzyme: None,
                kept_sites: None,
            })
        }
        GibsonCut::Region { start, end } => {
            let start = circle.clamp(*start);
            let end = circle.clamp(*end);
            if end < start && !circular {
                return Err(GibsonError::WrappingRegionOnLinear { start, end });
            }
            let up = start;
            let down = if end + 1 > backbone_len { backbone_len } else { end + 1 };
            Ok(GibsonOpening {
                up,
                down,
                wraps: down < up,
                removed_len: circle.span(up, down),
                arcs: circle.arcs_for(up, down),
                enzymes: None,
                sites: Vec::new(),
                up_enzyme: None,
                down_enzyme: None,
                kept_sites: None,
            })
        }
        GibsonCut::Enzymes { enzymes, sites, arc, keep_sites } => resolve_enzyme_opening(circle, enzymes, sites, *arc, *keep_sites, warnings, margin),
    }
}

/// [`resolve_gibson_opening`] for a resolved pair of cut sites.
fn resolve_enzyme_opening(
    circle: Circle,
    enzymes: &[String; 2],
    sites: &[GibsonCutSite],
    arc: Option<OpeningArc>,
    keep_sites: bool,
    warnings: &mut Vec<String>,
    margin: i64,
) -> Result<GibsonOpening, GibsonError> {
    for en in enzymes {
        let hits = sites.iter().filter(|site| &site.enzyme == en).count();
        if hits != 1 {
            warnings.push(format!("{en} cuts {hits}× on the backbone (need exactly 1)"));
        }
    }
    let positions: Vec<i64> = sites.iter().map(|site| circle.clamp(site.position)).collect();
    let lo = positions.iter().copied().min().unwrap_or(0);
    let hi = positions.iter().copied().max().unwrap_or(lo);
    let chosen_arc = arc.unwrap_or(OpeningArc::Between);
    if chosen_arc == OpeningArc::Origin && !circle.circular {
        return Err(GibsonError::OriginArcOnLinear);
    }
    let mut ordered: Vec<&GibsonCutSite> = sites.iter().collect();
    ordered.sort_by_key(|site| circle.clamp(site.position));
    let keep = Keep { circle, enzymes, lo, hi, first: ordered.first().copied(), last: ordered.last().copied(), keep_sites, margin };
    let chosen = keep.resolve_bounds(chosen_arc);
    let other = keep.resolve_bounds(if chosen_arc == OpeningArc::Origin { OpeningArc::Between } else { OpeningArc::Origin });
    let chosen_len = circle.span(chosen.up, chosen.down);
    let other_len = circle.span(other.up, other.down);
    let arcs = if chosen_arc == OpeningArc::Origin {
        OpeningArcs { between: other_len, origin: chosen_len }
    } else {
        OpeningArcs { between: chosen_len, origin: other_len }
    };
    // THE ANTI-SILENCE LINE. Default behaviour is unchanged — "between" is still what an unqualified
    // pair means — but the moment the other arc is the smaller one, say so. Removing the big arc
    // usually means removing the origin and the resistance marker.
    if arc.is_none() && circle.circular && arcs.origin < arcs.between {
        let [a, b] = enzymes;
        warnings.push(format!(
            "Removing the {} bp arc between {a} and {b}; the arc through the origin is only {} bp — pass arc:'origin' to remove that one instead.",
            arcs.between, arcs.origin
        ));
    }
    if let Some(note) = &chosen.note {
        warnings.push(note.clone());
    }
    let ends = keep.ends(chosen_arc);
    Ok(GibsonOpening {
        up: chosen.up,
        down: chosen.down,
        wraps: chosen.down < chosen.up,
        removed_len: circle.span(chosen.up, chosen.down),
        arcs,
        enzymes: Some(enzymes.clone()),
        sites: sites.to_vec(),
        up_enzyme: ends.up_site.map(|s| s.enzyme.clone()),
        down_enzyme: ends.down_site.map(|s| s.enzyme.clone()),
        kept_sites: (!chosen.kept.is_empty()).then_some(chosen.kept),
    })
}

/// What became of one chosen enzyme's site on the plasmid the design actually makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteSurvival {
    pub enzyme: String,
    /// Times it cuts the ASSEMBLED product. A chosen unique cutter should be 1, and often is not.
    pub on_product: usize,
    /// Why it is not 1, in a sentence fit to put in front of a person. Absent when it is 1.
    pub note: Option<String>,
}

/// Do the enzymes you opened with still cut the plasmid you made?
///
/// `keep_sites` is an INTENTION. This checks the product: it reads the assembled sequence back with
/// the same `find_cut_sites` the map draws with and counts. Three things it catches that intending
/// cannot: a Type IIS opening always loses its site, by design and in silence; two enzymes reading
/// overlapping DNA lose both; and a junction can SPELL A NEW SITE, so the unique cutter you chose
/// cuts the product twice.
///
/// `inserted` is what was put in, so an extra copy can be attributed to the DNA the user supplied
/// rather than to the join we designed. Region openings have no enzymes and get an empty list.
pub fn check_site_survival(assembled: &str, circular: bool, opening: &GibsonOpening, inserted: &[&str]) -> Vec<SiteSurvival> {
    let Some(enzymes) = &opening.enzymes else { return Vec::new() };
    let hits = find_cut_sites(assembled, enzymes.as_slice(), circular);
    enzymes.iter().map(|enzyme| survival_of(enzyme, &hits, inserted)).collect()
}

/// One enzyme's fate on the product.
fn survival_of(enzyme: &str, hits: &[crate::enzymes::CutSite], inserted: &[&str]) -> SiteSurvival {
    let on_product = hits.iter().filter(|h| h.enzyme == enzyme).count();
    if on_product == 1 {
        return SiteSurvival { enzyme: enzyme.to_string(), on_product, note: None };
    }
    if on_product == 0 {
        let note = if get_enzyme(enzyme).is_some_and(|e| e.type2s) {
            format!(
                "{enzyme} reads outside its own site, so the site leaves with the excised arc — the product cannot be cut with {enzyme} again. (Golden Gate wants exactly that; a Gibson diagnostic digest does not.)"
            )
        } else {
            format!(
                "{enzyme} no longer cuts the product: the backbone was opened inside its recognition site, so half of it left with the excised arc."
            )
        };
        return SiteSurvival { enzyme: enzyme.to_string(), on_product, note: Some(note) };
    }
    let in_insert: usize = inserted.iter().map(|seq| find_cut_sites(seq, &[enzyme], false).len()).sum();
    let note = if in_insert >= on_product - 1 {
        format!(
            "{enzyme} cuts the product {on_product}× — it is on the backbone and {in_insert}× inside the inserted DNA, so it is no longer a unique site."
        )
    } else {
        format!(
            "{enzyme} cuts the product {on_product}× — a junction spells a second site that neither the backbone nor the insert carried, so it is no longer a unique site."
        )
    };
    SiteSurvival { enzyme: enzyme.to_string(), on_product, note: Some(note) }
}

// ---------------------------------------------------------------------------
// Assembling the product.
// ---------------------------------------------------------------------------

/// What the opening did to the parent's annotations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryReport {
    /// Every feature present on the product, truncated ones included.
    pub carried: Vec<Feature>,
    /// Clipped by the opening — half of it is really there, so it is kept and marked.
    pub truncated: Vec<TruncatedFeature>,
    /// Entirely inside the removed span.
    pub removed: Vec<Feature>,
}

/// Every Bios note entry looks like `bios-<key>: <value>`.
const NOTE_PREFIX: &str = "bios-";

/// `writeBiosFields` of the web app's `bios/model.ts`, ported privately because that module is not
/// ported and this is the only part of it the designer needs: merge one Bios field into a feature's
/// qualifiers, leaving every human `/note` exactly where it was.
///
/// The `bios-` prefix rather than a position is deliberate: NCBI's table2asn silently strips
/// qualifiers it does not know, and SnapGene treats the LAST note as the machine-readable one. A
/// prefix coexists with both. The fields are written sorted, so a no-op save is byte-identical.
fn write_bios_fields(notes: &[(String, Vec<String>)], key: &str, value: &str) -> Vec<(String, Vec<String>)> {
    let existing: &[String] = notes.iter().find(|(k, _)| k == "note").map_or(&[], |(_, v)| v.as_slice());
    let human: Vec<String> = existing.iter().filter(|l| !l.starts_with(NOTE_PREFIX)).cloned().collect();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut set = |k: &str, v: &str| match fields.iter_mut().find(|(existing, _)| existing == k) {
        Some(entry) => entry.1 = v.to_string(),
        None => fields.push((k.to_string(), v.to_string())),
    };
    for line in existing.iter().filter(|l| l.starts_with(NOTE_PREFIX)) {
        let Some(sep) = line.find(':') else { continue };
        let name = line.get(NOTE_PREFIX.len()..sep).unwrap_or_default().trim().to_string();
        let read = line.get(sep + 1..).unwrap_or_default().trim().to_string();
        set(&name, &read);
    }
    set(key, value);
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    let all: Vec<String> = human.into_iter().chain(fields.iter().map(|(k, v)| format!("{NOTE_PREFIX}{k}: {v}"))).collect();
    let mut out = notes.to_vec();
    if all.is_empty() {
        out.retain(|(k, _)| k != "note");
    } else if let Some(entry) = out.iter_mut().find(|(k, _)| k == "note") {
        entry.1 = all;
    } else {
        out.push(("note".to_string(), all));
    }
    out
}

/// The backbone that SURVIVES, read 5'→3' from `from` around to `to`.
///
/// `from == to` IS A FULL TURN, NOT AN EMPTY STRING. It is reached when nothing was removed — a
/// pure insertion — and on a circle "walk from here back to here" means all the way round. Reading
/// it as empty gave a pure insertion into a circular plasmid no backbone to size its arms against,
/// so both arms came out empty and both junctions reported 0 °C.
fn retained_arc(seq: &str, from: i64, to: i64) -> String {
    if from == to {
        return format!("{}{}", js_from(seq, from), js_slice(seq, 0, from));
    }
    if from < to { js_slice(seq, from, to).to_string() } else { format!("{}{}", js_from(seq, from), js_slice(seq, 0, to)) }
}

struct Assembly {
    sequence: String,
    features: Vec<Feature>,
    /// 0-based index in the ASSEMBLED sequence where the inserted bases start.
    ins_start: i64,
    carry: CarryReport,
}

/// Open the backbone and put `insert_seq` in, carrying the parent's annotations correctly.
///
/// The carry is [`apply_replace`] (or [`apply_insert`] for a pure insertion) and nothing else: that
/// is this exact operation, it splits around deleted runs and re-collapses origin-spanning
/// features, and a hand-written carry dropped every feature touching a cut boundary — so a CDS
/// whose 5' half survives was deleted outright.
fn assemble_opening(
    backbone: &str,
    backbone_features: &[Feature],
    circular: bool,
    opening: &GibsonOpening,
    insert_seq: &str,
) -> Result<Assembly, GibsonError> {
    // The one ±1 in this file: the opening is 0-based half-open, edits.rs speaks 1-based inclusive.
    let r = if opening.removed_len == 0 {
        apply_insert(backbone, backbone_features, opening.up + 1, insert_seq, circular)?
    } else {
        apply_replace(backbone, backbone_features, opening.up + 1, opening.down, insert_seq, circular)?
    };
    let mut truncated = r.delta.truncated;
    let mut features = r.features;
    // The web app pairs these by object identity. Here both lists are built in one pass over the
    // input features, so position does the same job: the truncated entries are a subsequence of the
    // surviving ones, in the same order.
    let mut next = 0usize;
    for f in features.iter_mut() {
        let Some(t) = truncated.get_mut(next) else { break };
        if &t.after != f {
            continue;
        }
        let bridged = if t.bridged { ", and bridged across it" } else { "" };
        let partial = format!("truncated by {} bp at the Gibson opening{bridged}", t.lost_bases);
        // Marked, not renamed: feature colours are keyed by name, and the note is what a recipient
        // reads.
        f.notes = write_bios_fields(&f.notes, "partial", &partial);
        t.after = f.clone();
        next += 1;
    }
    Ok(Assembly {
        sequence: r.sequence,
        // A wrapped replacement starts at the new origin, followed by the retained backbone arc.
        ins_start: if opening.wraps { 0 } else { opening.up },
        carry: CarryReport { carried: features.clone(), truncated, removed: r.delta.removed },
        features,
    })
}

/// A 1 bp-or-longer marker over `len` bases from the half-open `start`, wrapped onto the product.
///
/// The web app also writes the name into a `label` qualifier. Here the name IS the label — the
/// reader takes it out of the notes and the writer puts it back (see `apply_annotate`) — so a second
/// copy in the notes would reach the file twice.
fn add_span(features: &mut Vec<Feature>, total: i64, start_half_open: i64, len: i64, name: &str, kind: &str) {
    if len <= 0 {
        return;
    }
    let wrap = |x: i64| if total <= 0 { 0 } else { x.rem_euclid(total) };
    let start = usize::try_from(wrap(start_half_open)).unwrap_or(0);
    let end = usize::try_from(wrap(start_half_open + len - 1)).unwrap_or(0);
    features.push(Feature {
        name: name.to_string(),
        kind: kind.to_string(),
        start,
        end,
        strand: Strand::Forward,
        segments: vec![Segment::new(start, end)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    });
}

// ---------------------------------------------------------------------------
// Sizing the two homology arms. Shared by both designers, which is why the Tm walk is identical in
// the single- and multi-fragment cases.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Up,
    Down,
}

/// The most the keep rule can hold back: the longest recognition site plus a Type IIS reach.
const MAX_HELD_BACK: i64 = 40;

/// How many retained bases lie between an enzyme's scissile bond and the boundary the opening chose.
///
/// WHAT ACTUALLY ANNEALS. The digest still ends the backbone AT THE BOND: the bases the keep rule
/// holds back exist only on the synthesised eBlock, between its homology and the insert, where
/// polymerase copies them onto the backbone's 3′ end. They are product, not homology, and an arm
/// sized as though they annealed reports a junction hotter than the one you get. So the Tm walk
/// runs on `arm − this`, and the arm is what the walk found plus this.
fn beyond_cut(opening: &GibsonOpening, enzyme: Option<&str>, side: Side, len: i64, circular: bool) -> i64 {
    let Some(site) = opening.sites.iter().find(|s| Some(s.enzyme.as_str()) == enzyme) else { return 0 };
    if len <= 0 {
        return 0;
    }
    let fwd = |a: i64, b: i64| if circular { (b - a).rem_euclid(len) } else { b - a };
    let n = match side {
        Side::Up => fwd(site.position, opening.up),
        Side::Down => fwd(opening.down, site.position),
    };
    // A boundary BEHIND its own cut needs no correction: it is inside the retained backbone. That
    // is the overlapping-sites case, where the opening is a pure insertion at the far edge of the
    // pair, and on a circle it reads as very nearly a whole turn — so anything bigger than a site
    // plus a Type IIS reach is not a correction, it is the other direction.
    if (0..=MAX_HELD_BACK).contains(&n) { n } else { 0 }
}

/// The retained backbone and where each arm grows from on it.
struct VectorContext {
    vector: String,
    /// Exclusive end the left arm terminates at.
    left_end: i64,
    /// Start the right arm grows from.
    right_start: i64,
    anneal_end: i64,
    anneal_start: i64,
}

fn vector_context(backbone: &str, circular: bool, opening: &GibsonOpening) -> VectorContext {
    // The retained backbone read 5'→3' from the downstream cut around to the upstream one, so both
    // Tm walks get real context across the origin.
    let vector = if circular { retained_arc(backbone, opening.down, opening.up) } else { backbone.to_string() };
    let vlen = len64(&vector);
    let left_end = if circular { vlen } else { opening.up };
    let right_start = if circular { 0 } else { opening.down };
    let blen = len64(backbone);
    // The stretch that anneals ends at the scissile bond, not at the boundary — see `beyond_cut`.
    let held_up = beyond_cut(opening, opening.up_enzyme.as_deref(), Side::Up, blen, circular);
    let held_down = beyond_cut(opening, opening.down_enzyme.as_deref(), Side::Down, blen, circular);
    VectorContext {
        anneal_end: OL_MIN.max(left_end - held_up),
        anneal_start: (vlen - OL_MIN).max(0).min(right_start + held_down),
        vector,
        left_end,
        right_start,
    }
}

/// Margins to try, best first. One base of clearance is the preference; the rest are the room.
const MARGIN_WALK: [i64; 4] = [SITE_MARGIN, 0, 2, 3];

/// The opening, with the site margin walked until no junction spells a second copy of a chosen site.
///
/// The keep rule guarantees the sites are ON the product. It cannot, on its own, guarantee they are
/// on it ONCE: a boundary is a place where two stretches that never met before now read as one.
/// Measured on a real backbone opened with SnaBI + Bst1107I, the retained TACGTA meets the insert as
/// TACGTA·T·ACATCA, which spells a second GTATAC. The margin beside the site is free, so walk it and
/// take the first that spells nothing. If none does, the new site is in the DNA itself rather than
/// in our choice of boundary, and `check_site_survival` says so on the product.
fn opening_for(backbone: &str, circular: bool, cut: &GibsonCut, warnings: &mut Vec<String>, inserted: &str) -> Result<GibsonOpening, GibsonError> {
    let blen = len64(backbone);
    let walk = match cut {
        GibsonCut::Enzymes { enzymes, keep_sites: true, .. } if !inserted.is_empty() => Some(enzymes),
        _ => None,
    };
    let Some(enzymes) = walk else {
        return resolve_gibson_opening(blen, circular, cut, warnings, SITE_MARGIN);
    };
    for margin in MARGIN_WALK {
        let opening = resolve_gibson_opening(blen, circular, cut, &mut Vec::new(), margin)?;
        if !junction_spells_site(backbone, &opening, inserted, enzymes) {
            return resolve_gibson_opening(blen, circular, cut, warnings, margin);
        }
    }
    resolve_gibson_opening(blen, circular, cut, warnings, SITE_MARGIN)
}

/// Does either backbone↔insert join read as a copy of one of the chosen sites that spans it?
fn junction_spells_site(backbone: &str, opening: &GibsonOpening, inserted: &str, enzymes: &[String; 2]) -> bool {
    let w = enzymes.iter().map(|e| get_enzyme(e).map_or(0i64, |x| i64::try_from(x.length).unwrap_or(0))).max().unwrap_or(0);
    if w <= 1 {
        return false;
    }
    let n = len64(backbone);
    let bytes = backbone.as_bytes();
    let at = |i: i64| -> Option<char> {
        if n <= 0 {
            return None;
        }
        bytes.get(usize::try_from(i.rem_euclid(n)).ok()?).map(|&b| char::from(b))
    };
    // The bases either side of each join, on the PRODUCT: retained backbone ending at `up`, then
    // the insert; the insert's end, then retained backbone from `down`.
    let before: String = ((opening.up - w)..opening.up).filter_map(at).collect();
    let after: String = (opening.down..(opening.down + w)).filter_map(at).collect();
    let joins =
        [(format!("{before}{}", js_slice(inserted, 0, w)), len64(&before)), (format!("{}{after}", js_from(inserted, -w)), w.min(len64(inserted)))];
    for (window, boundary) in joins {
        for hit in find_cut_sites(&window, enzymes.as_slice(), false) {
            let len = get_enzyme(hit.enzyme).map_or(0i64, |e| i64::try_from(e.length).unwrap_or(0));
            let start = i64::try_from(hit.recognition_start).unwrap_or(0);
            if start < boundary && start + len > boundary {
                return true;
            }
        }
    }
    false
}

/// What to call the 1 bp marker at a backbone↔insert boundary.
///
/// "EcoRI cut" is only true where the backbone was opened INSIDE the site. When the site is kept the
/// boundary sits just outside it and the real site is still there, drawn by the map's own enzyme
/// layer — so a marker claiming to be the cut would put a second, false EcoRI on the picture.
fn boundary_label(opening: &GibsonOpening, enzyme: &str) -> String {
    let kept = opening.kept_sites.as_ref().is_some_and(|k| k.iter().any(|e| e == enzyme));
    format!("{enzyme} {}", if kept { "junction" } else { "cut" })
}

/// The pair of boundary-marker labels, named for the enzyme whose site stands at each boundary.
///
/// The opening reports that directly, because matching a boundary back to a cut POSITION stops
/// working the moment the boundary is the site's edge rather than the bond inside it — and it fails
/// by silently naming `enzymes[0]` twice.
fn cut_labels(opening: &GibsonOpening) -> (String, String) {
    match &opening.enzymes {
        Some([a, b]) => {
            (boundary_label(opening, opening.up_enzyme.as_deref().unwrap_or(a)), boundary_label(opening, opening.down_enzyme.as_deref().unwrap_or(b)))
        }
        None => ("5′ cut".to_string(), "3′ cut".to_string()),
    }
}

// ---------------------------------------------------------------------------
// designGibson — the whole thing.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct DesignGibsonInput {
    pub backbone: String,
    pub circular: bool,
    pub cut: GibsonCut,
    pub insert: String,
    /// Homology-arm target Tm in °C. The web app's default is 65 (the NEB HiFi sweet spot).
    pub target_tm: f64,
    /// Max synthesized piece length before splitting. The web app's default is 3000 (IDT eBlock).
    pub max_eblock_len: i64,
    /// Backbone features to carry/shift onto the assembled construct.
    pub backbone_features: Vec<Feature>,
    /// Label stem for ordered eBlocks. The web app's default is "Insert".
    pub insert_name: String,
}

impl DesignGibsonInput {
    /// The design with the web app's defaults for everything optional.
    pub fn new(backbone: impl Into<String>, circular: bool, cut: GibsonCut, insert: impl Into<String>) -> Self {
        DesignGibsonInput {
            backbone: backbone.into(),
            circular,
            cut,
            insert: insert.into(),
            target_tm: 65.0,
            max_eblock_len: 3000,
            backbone_features: Vec::new(),
            insert_name: "Insert".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GibsonOverlap {
    pub left: String,
    pub right: String,
    pub tm_left: f64,
    pub tm_right: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Eblock {
    pub name: String,
    pub order: String,
    pub length: i64,
    pub gc: f64,
    /// Tm of the overlap joining it to the previous piece, when the eBlock was split.
    pub tm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assembled {
    pub sequence: String,
    pub features: Vec<Feature>,
    pub circular: bool,
    pub carry: CarryReport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignGibsonResult {
    pub assembled: Assembled,
    pub eblocks: Vec<Eblock>,
    pub overlaps: Vec<GibsonOverlap>,
    pub warnings: Vec<String>,
    /// The opening that was used, for the UI to draw. `removed_len` rather than `down - up`.
    pub cut: GibsonOpening,
    /// What became of each chosen enzyme's site ON THE PRODUCT. See [`check_site_survival`].
    pub sites: Vec<SiteSurvival>,
}

/// Design a Gibson assembly: size the two homology arms flanking `insert` so the ordered eBlock
/// Gibsons into the backbone opened at `cut`, and return the assembled construct (annotated), the
/// eBlock(s) to order, and validation warnings.
pub fn design_gibson(input: &DesignGibsonInput) -> Result<DesignGibsonResult, GibsonError> {
    let backbone = clean_seq(&input.backbone);
    let insert = clean_seq(&input.insert);
    let circular = input.circular;
    let target_tm = input.target_tm;
    let mut warnings: Vec<String> = Vec::new();

    // ---- Resolve the opening ----
    let opening = opening_for(&backbone, circular, &input.cut, &mut warnings, &insert)?;

    // ---- Size the homology arms ----
    let v = vector_context(&backbone, circular, &opening);
    let left_r = tm_range_reverse(&v.vector, v.anneal_end, target_tm, 18, 60);
    let right_r = tm_range(&v.vector, v.anneal_start, target_tm, 18, 60);
    let left_arm = js_slice(&v.vector, left_r.pos, v.left_end).to_string();
    let right_arm = js_slice(&v.vector, v.right_start, right_r.pos).to_string();
    let tm_left = round1(left_r.tm);
    let tm_right = round1(right_r.tm);
    let overlaps = vec![GibsonOverlap { left: left_arm.clone(), right: right_arm.clone(), tm_left, tm_right }];

    // ---- Build the eBlock(s) to order ----
    let full_eblock = format!("{left_arm}{insert}{right_arm}");
    let pieces = split_eblock(&full_eblock, target_tm, input.max_eblock_len, &input.insert_name, false)?;
    let eblocks: Vec<Eblock> = pieces
        .iter()
        .map(|p| Eblock {
            name: p.name.clone(),
            order: p.order.clone(),
            length: len64(&p.order),
            gc: round1(p.gc),
            tm: p.join_prev.as_ref().map(|j| j.tm),
        })
        .collect();

    // ---- Assemble ----
    let asm = assemble_opening(&backbone, &input.backbone_features, circular, &opening, &insert)?;
    let assembled_seq = asm.sequence;
    let mut features = asm.features;
    let total = len64(&assembled_seq);
    let ins_start = asm.ins_start;
    let insert_len = len64(&insert);
    let left_len = len64(&left_arm);
    let right_len = len64(&right_arm);
    add_span(&mut features, total, ins_start, insert_len, &input.insert_name, "misc_feature");
    add_span(&mut features, total, ins_start - left_len, left_len, &format!("Overlap L ({}°C)", js_num(tm_left)), "misc_feature");
    add_span(&mut features, total, ins_start + insert_len, right_len, &format!("Overlap R ({}°C)", js_num(tm_right)), "misc_feature");
    // Cut-site markers (1 bp) at the two backbone junctions.
    let (up_label, down_label) = cut_labels(&opening);
    add_span(&mut features, total, ins_start - left_len, 1, &up_label, "misc_feature");
    add_span(&mut features, total, ins_start + insert_len + right_len - 1, 1, &down_label, "misc_feature");

    // ---- Validate (surface as warnings) ----
    for (side, arm, tmv) in [("L", &left_arm, tm_left), ("R", &right_arm, tm_right)] {
        warn_overlap(&mut warnings, side, arm, tmv, &assembled_seq, circular);
    }
    // Internal eBlock junctions (when split).
    for p in &pieces {
        if let Some(join) = &p.join_prev
            && !(55.0..=72.0).contains(&join.tm)
        {
            warnings.push(format!("{} junction Tm {}°C is outside 55–72°C", p.name, js_num(join.tm)));
        }
    }
    for eb in &eblocks {
        for issue in idt_preflight(&eb.name, &eb.order, None) {
            if issue.code == PreflightCode::LengthOutOfRange && pieces.len() == 1 && eb.length < 300 {
                warnings.push(format!(
                    "{} is {} bp — below the 300 bp IDT eBlock minimum (order as a gBlock/gene fragment or Ultramer)",
                    eb.name, eb.length
                ));
            } else {
                warnings.push(issue.detail);
            }
        }
    }

    let sites = check_site_survival(&assembled_seq, circular, &opening, &[insert.as_str()]);
    for s in &sites {
        if let Some(note) = &s.note {
            warnings.push(note.clone());
        }
    }

    Ok(DesignGibsonResult {
        assembled: Assembled { sequence: assembled_seq, features, circular, carry: asm.carry },
        eblocks,
        overlaps,
        warnings,
        cut: opening,
        sites,
    })
}

/// The three things that can be wrong with one outer homology arm.
fn warn_overlap(warnings: &mut Vec<String>, side: &str, arm: &str, tmv: f64, assembled: &str, circular: bool) {
    let len = len64(arm);
    if len < 15 {
        warnings.push(format!("Overlap {side} is {len} bp (<15 bp — weak Gibson junction)"));
    }
    if !(55.0..=72.0).contains(&tmv) {
        warnings.push(format!("Overlap {side} Tm {}°C is outside 55–72°C", js_num(tmv)));
    }
    let fwd = check_uniqueness(assembled, arm, circular).fwd;
    if fwd != 1 {
        warnings.push(format!("Overlap {side} is not unique in the assembled plasmid ({fwd} forward hits — misassembly risk)"));
    }
}

// ---------------------------------------------------------------------------
// designGibsonMulti — N ordered, named fragments into one opening.
//
// [`design_gibson`] above is the oracle-matched primitive and is left exactly as it is. This
// generalizes it to the way people actually think about a build: "I want to drop THESE things in
// here, in order, and order an eBlock for each." Fragments assemble backbone → frag1 → … → fragN →
// backbone; the two outer homology arms are sized against the backbone (identical Tm walk to the
// single case), and every fragment↔fragment boundary gets its own homology overlap. Each named
// fragment becomes its own eBlock, sub-split only if it exceeds the synthesis limit. N=1 reduces to
// the same assembled sequence + arms as `design_gibson`.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GibsonFragment {
    pub name: String,
    pub seq: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignGibsonMultiInput {
    pub backbone: String,
    pub circular: bool,
    pub cut: GibsonCut,
    /// Ordered fragments to insert (5′→3′). Each becomes at least one eBlock.
    pub inserts: Vec<GibsonFragment>,
    pub target_tm: f64,
    pub max_eblock_len: i64,
    pub backbone_features: Vec<Feature>,
    /// Hold every junction to ONE temperature instead of aiming each at the target on its own.
    /// Off in the golden vectors, which pin dnakit's per-junction walk; the app turns it on.
    pub match_overlaps: bool,
}

impl DesignGibsonMultiInput {
    /// The design with the web app's defaults for everything optional.
    pub fn new(backbone: impl Into<String>, circular: bool, cut: GibsonCut, inserts: Vec<GibsonFragment>) -> Self {
        DesignGibsonMultiInput {
            backbone: backbone.into(),
            circular,
            cut,
            inserts,
            target_tm: 65.0,
            max_eblock_len: 3000,
            backbone_features: Vec::new(),
            match_overlaps: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JunctionKind {
    BackboneInsert,
    InsertInsert,
    InsertBackbone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GibsonJunction {
    pub kind: JunctionKind,
    /// Upstream / downstream member names ("backbone" or a fragment name).
    pub left: String,
    pub right: String,
    /// The homology overlap sequence spanning the junction.
    pub seq: String,
    pub tm: f64,
    /// One forward hit in the assembled plasmid, and no reverse-complement hit unless palindromic.
    pub unique: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiEblock {
    pub name: String,
    pub order: String,
    pub length: i64,
    pub gc: f64,
    /// The fragment this eBlock (or sub-piece) belongs to.
    pub fragment: String,
    /// Tm of the overlap joining it to the previous eBlock, when this is a sub-piece.
    pub junction_tm: Option<f64>,
    /// Where this eBlock sits on the laid-out strand `left arm + inserts + right arm`, 0-based
    /// half-open. Consecutive eBlocks OVERLAP here, by the homology they share.
    ///
    /// Subtract `arms.left` to get insert coordinates: the first eBlock then starts NEGATIVE,
    /// because it carries the upstream homology arm, and the last ends past the insert for the same
    /// reason. That is true and worth drawing — the arm is part of what you order.
    pub start: i64,
    pub end: i64,
}

/// Where one fragment lands in the assembled sequence, 0-based inclusive — for the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// The two outer homology arms, in bp. The origin of [`MultiEblock::start`]'s coordinate frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmLengths {
    pub left: i64,
    pub right: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DesignGibsonMultiResult {
    pub assembled: Assembled,
    pub eblocks: Vec<MultiEblock>,
    pub junctions: Vec<GibsonJunction>,
    pub warnings: Vec<String>,
    pub cut: GibsonOpening,
    /// What became of each chosen enzyme's site ON THE PRODUCT. See [`check_site_survival`].
    pub sites: Vec<SiteSurvival>,
    pub placements: Vec<Placement>,
    pub arms: ArmLengths,
}

/// The two outer arms as chosen, and the temperature the whole reaction is held to.
struct OuterArms {
    left: String,
    right: String,
    tm_left: f64,
    tm_right: f64,
    shared_target: f64,
}

/// MATCHED: one temperature for the whole reaction.
///
/// The junctions that CANNOT move decide what is achievable: the two backbone arms, anchored at the
/// cuts, and every boundary between fragments the user actually typed. Each may still choose its own
/// LENGTH. Once they agree on a temperature, the boundaries we own — the ones inside a single
/// ordered fragment — slide until they match it.
fn match_outer_junctions(arms: &mut OuterArms, v: &VectorContext, frags: &[GibsonFragment], target_tm: f64) {
    let insert_only: String = frags.iter().map(|f| f.seq.as_str()).collect();
    // A fragment-boundary overlap grows RIGHTWARD from the boundary, so its bases come from the
    // insert (and, at the last boundary, the right arm) and never from the left arm. Measuring it on
    // `insert + right arm` at cumulative insert offsets therefore gives exactly the same bases as
    // measuring it on the full strand — and, unlike that, it does not move when the left arm's
    // length is re-chosen below. Getting this wrong shifted every boundary by the difference and
    // quietly made the matched design WORSE than the unmatched one.
    let tail = format!("{insert_only}{}", arms.right);
    // The left arm grows BACKWARD from the cut, so its options are the suffixes of `vector` ending
    // at left_end; the right arm's are the prefixes starting at right_start.
    let left_opts: Vec<OverlapChoice> = (OL_MIN..=OL_MAX)
        .filter(|len| v.anneal_end - len >= 0)
        .filter_map(|len| {
            let from = v.anneal_end - len;
            gibson_tm(js_slice(&v.vector, from, v.anneal_end)).map(|tm| OverlapChoice { pos: from, len, tm })
        })
        .collect();
    let right_opts = overlap_options(&v.vector, v.anneal_start, v.anneal_start);
    // Fragment boundaries, walked plainly. Searching for the fragment would misplace a junction the
    // moment two fragments share a name, which is exactly the kind of thing nobody notices.
    let mut fixed_frag_opts: Vec<Vec<OverlapChoice>> = Vec::new();
    let mut cursor = 0i64;
    for f in frags.iter().take(frags.len().saturating_sub(1)) {
        cursor += len64(&f.seq);
        fixed_frag_opts.push(overlap_options(&tail, cursor, cursor));
    }

    let mut reach: Vec<Option<(f64, f64)>> = vec![reachable(&left_opts), reachable(&right_opts)];
    reach.extend(fixed_frag_opts.iter().map(|w| reachable(w)));
    arms.shared_target = shared_tm(target_tm, &reach);

    // Everything that cannot move, settled together — the arms and the fragment boundaries. Use
    // `settle`'s OWN picks: it has already lowered them onto a common temperature, and lowering a
    // second time on top of that just ratchets the whole design down away from the target.
    let mut fixed_windows: Vec<Vec<OverlapChoice>> = vec![left_opts.clone(), right_opts.clone()];
    fixed_windows.append(&mut fixed_frag_opts);
    let non_empty: Vec<Vec<OverlapChoice>> = fixed_windows.into_iter().filter(|w| !w.is_empty()).collect();
    let settled = settle(&non_empty, arms.shared_target);
    if settled.len() != non_empty.len() {
        return;
    }
    // The temperature the immovable junctions agreed on. Everything else is held to it.
    arms.shared_target = settled.iter().map(|c| c.tm).fold(f64::NEG_INFINITY, f64::max);
    if !left_opts.is_empty()
        && let Some(l) = settled.first()
    {
        arms.left = js_slice(&v.vector, l.pos, v.left_end).to_string();
        arms.tm_left = round1(l.tm);
    }
    if !right_opts.is_empty()
        && let Some(r) = settled.get(usize::from(!left_opts.is_empty()))
    {
        arms.right = js_slice(&v.vector, v.right_start, r.pos + r.len).to_string();
        arms.tm_right = round1(r.tm);
    }
}

/// The laid-out strand and the offsets everything in the multi designer is measured against.
struct MultiLayout<'a> {
    full: String,
    frags: &'a [GibsonFragment],
    /// Offsets in `full` of each fragment after the first — the N-1 internal boundaries.
    boundaries: Vec<i64>,
    /// Overlap length grown rightward at each of those boundaries.
    overlap_len: Vec<i64>,
}

impl MultiLayout<'_> {
    /// The homology overlap at internal boundary `i`.
    fn overlap(&self, i: usize) -> &str {
        let b = self.boundaries.get(i).copied().unwrap_or(0);
        js_slice(&self.full, b, b + self.overlap_len.get(i).copied().unwrap_or(0))
    }

    /// The stretch ordered for fragment `i`: from its boundary to the next one plus its overlap.
    fn fragment_span(&self, i: usize) -> (i64, i64) {
        let start = if i == 0 { 0 } else { self.boundaries.get(i - 1).copied().unwrap_or(0) };
        let end = if i + 1 == self.frags.len() {
            len64(&self.full)
        } else {
            self.boundaries.get(i).copied().unwrap_or(0) + self.overlap_len.get(i).copied().unwrap_or(0)
        };
        (start, end)
    }
}

pub fn design_gibson_multi(input: &DesignGibsonMultiInput) -> Result<DesignGibsonMultiResult, GibsonError> {
    let backbone = clean_seq(&input.backbone);
    let frags: Vec<GibsonFragment> = input
        .inserts
        .iter()
        .map(|f| {
            let name = trim_js(&f.name);
            GibsonFragment { name: if name.is_empty() { "Insert".to_string() } else { name.to_string() }, seq: clean_seq(&f.seq) }
        })
        .filter(|f| !f.seq.is_empty())
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    if frags.is_empty() {
        return Err(GibsonError::NoFragments);
    }
    let full_insert: String = frags.iter().map(|f| f.seq.as_str()).collect();

    // ---- Resolve the opening (one implementation, shared with design_gibson) ----
    let opening = opening_for(&backbone, input.circular, &input.cut, &mut warnings, &full_insert)?;

    // ---- Size the two outer homology arms (identical walk to design_gibson) ----
    let v = vector_context(&backbone, input.circular, &opening);
    let left_r = tm_range_reverse(&v.vector, v.anneal_end, input.target_tm, 18, 60);
    let right_r = tm_range(&v.vector, v.anneal_start, input.target_tm, 18, 60);
    let mut arms = OuterArms {
        left: js_slice(&v.vector, left_r.pos, v.left_end).to_string(),
        right: js_slice(&v.vector, v.right_start, right_r.pos).to_string(),
        tm_left: round1(left_r.tm),
        tm_right: round1(right_r.tm),
        shared_target: input.target_tm,
    };
    if input.match_overlaps {
        match_outer_junctions(&mut arms, &v, &frags, input.target_tm);
    }

    // AN ARM THAT ISN'T THERE IS NOT A 0 °C JUNCTION. Opening a LINEAR backbone at one of its ends
    // leaves nothing downstream to anneal to, so the slice comes back empty and the Tm of an empty
    // string is nothing — which used to be rounded to 0.0 and drawn on the map as if it were a
    // temperature. There is no assembly here to report; say so.
    let (left_len, right_len) = (len64(&arms.left), len64(&arms.right));
    if left_len < OL_MIN || right_len < OL_MIN {
        let upstream = left_len < OL_MIN;
        let side = if upstream { ArmSide::Upstream } else { ArmSide::Downstream };
        return Err(GibsonError::NoRoomForArm { side, bp: if upstream { left_len } else { right_len } });
    }

    // ---- Lay the fragments out on one strand: arm + frag1 + … + fragN + arm ----
    let full = format!("{}{full_insert}{}", arms.left, arms.right);
    let mut frag_start: Vec<i64> = Vec::with_capacity(frags.len());
    let mut acc = left_len;
    for f in &frags {
        frag_start.push(acc);
        acc += len64(&f.seq);
    }
    let boundaries: Vec<i64> = frag_start.iter().skip(1).copied().collect();
    // Overlap length at each fragment boundary. Matched: the length whose Tm is nearest the shared
    // temperature. Otherwise: grown rightward past the target, min 20 bp, as dnakit does.
    let overlap_len: Vec<i64> = boundaries
        .iter()
        .map(|&b| {
            let walked = || tm_range(&full, b, input.target_tm, 20, 60).pos - b;
            if input.match_overlaps { pick_at(&overlap_options(&full, b, b), arms.shared_target).map_or_else(walked, |c| c.len) } else { walked() }
        })
        .collect();
    let layout = MultiLayout { full, frags: &frags, boundaries, overlap_len };

    // ---- Build eBlocks (one per fragment; sub-split if oversized) + junctions ----
    let asm = assemble_opening(&backbone, &input.backbone_features, input.circular, &opening, &full_insert)?;
    let assembled_seq = asm.sequence;
    let ins_start = asm.ins_start;
    // A junction overlap is only truly unique if it appears exactly once on the + strand AND
    // (unless it is a palindrome) never on the − strand — a reverse-complement hit drives inverted
    // misassembly.
    let arm_unique = |s: &str| {
        if s.is_empty() {
            return false;
        }
        let u = check_uniqueness(&assembled_seq, s, input.circular);
        u.fwd == 1 && (s == revcomp(s) || u.rc == 0)
    };

    let first_name = frags.first().map_or_else(String::new, |f| f.name.clone());
    let last_name = frags.last().map_or_else(String::new, |f| f.name.clone());
    let mut junctions: Vec<GibsonJunction> = vec![GibsonJunction {
        kind: JunctionKind::BackboneInsert,
        left: "backbone".to_string(),
        right: first_name,
        seq: arms.left.clone(),
        tm: arms.tm_left,
        unique: arm_unique(&arms.left),
    }];
    let mut eblocks: Vec<MultiEblock> = Vec::new();
    for (i, frag) in frags.iter().enumerate() {
        if let Some(prev) = i.checked_sub(1) {
            let ol = layout.overlap(prev);
            junctions.push(GibsonJunction {
                kind: JunctionKind::InsertInsert,
                left: frags.get(prev).map_or_else(String::new, |f| f.name.clone()),
                right: frag.name.clone(),
                seq: ol.to_string(),
                tm: round1(gibson_tm(ol).unwrap_or(0.0)),
                unique: arm_unique(ol),
            });
        }
        let (start, end) = layout.fragment_span(i);
        let order = js_slice(&layout.full, start, end);
        // Each named fragment is its own eBlock; only sub-split if it blows the synthesis cap.
        for p in split_eblock(order, arms.shared_target, input.max_eblock_len, &frag.name, input.match_overlaps)? {
            eblocks.push(MultiEblock {
                name: p.name,
                length: len64(&p.order),
                order: p.order,
                gc: round1(p.gc),
                fragment: frag.name.clone(),
                junction_tm: p.join_prev.map(|j| j.tm),
                // split_eblock reports the offset within THIS fragment's stretch; `start` puts it
                // back on the whole laid-out strand, which is the only frame the map can draw in.
                start: start + p.start,
                end: start + p.end,
            });
        }
    }
    junctions.push(GibsonJunction {
        kind: JunctionKind::InsertBackbone,
        left: last_name,
        right: "backbone".to_string(),
        seq: arms.right.clone(),
        tm: arms.tm_right,
        unique: arm_unique(&arms.right),
    });

    // ---- Assembled features: carry backbone features, annotate each fragment + the junctions ----
    let mut features = asm.features;
    let frame = Frame { total: len64(&assembled_seq), ins_start, left_len };
    let placements = annotate_product(&mut features, &layout, &arms, frame, &eblocks, &opening);

    // ---- Warnings ----
    multi_warnings(&mut warnings, &junctions, &assembled_seq, input.circular);
    for (i, &ol) in layout.overlap_len.iter().enumerate() {
        // A fragment shorter than the overlap grown into it can't form its own junction — the
        // neighbours' homology merges through it, quietly turning an N-way join into fewer joins.
        let Some(down) = frags.get(i + 1) else { continue };
        if ol >= len64(&down.seq) {
            warnings.push(format!(
                "Fragment \"{}\" ({} bp) is shorter than its {ol} bp junction overlap — it may not form an independent junction.",
                down.name,
                len64(&down.seq)
            ));
        }
    }
    for eb in &eblocks {
        for issue in idt_preflight(&eb.name, &eb.order, None) {
            if issue.code == PreflightCode::LengthOutOfRange && eb.length < 300 {
                let oligo = if eb.length <= 200 { " or a long oligo" } else { "" };
                warnings
                    .push(format!("{} is {} bp — below the 300 bp IDT eBlock minimum (order as a gBlock Gene Fragment{oligo})", eb.name, eb.length));
            } else {
                warnings.push(issue.detail);
            }
        }
    }

    let inserted: Vec<&str> = frags.iter().map(|f| f.seq.as_str()).collect();
    let sites = check_site_survival(&assembled_seq, input.circular, &opening, &inserted);
    for s in &sites {
        if let Some(note) = &s.note {
            warnings.push(note.clone());
        }
    }

    Ok(DesignGibsonMultiResult {
        assembled: Assembled { sequence: assembled_seq, features, circular: input.circular, carry: asm.carry },
        eblocks,
        junctions,
        warnings,
        cut: opening,
        sites,
        placements,
        arms: ArmLengths { left: left_len, right: right_len },
    })
}

/// Where the laid-out strand sits in the assembled product.
#[derive(Clone, Copy)]
struct Frame {
    total: i64,
    ins_start: i64,
    left_len: i64,
}

/// Annotate the product: each fragment, the two outer arms, every join between fragments, and —
/// only where a fragment had to be split — the pieces you actually order and their joins.
///
/// A three-piece assembly meets the backbone twice and meets itself twice. Only the outer two were
/// ever drawn, so a product made of several pieces showed nothing about how the pieces join — and
/// the join is the thing a synthesis house is being paid for. Doing it here rather than in the UI is
/// what makes the information travel in the file.
fn annotate_product(
    features: &mut Vec<Feature>,
    layout: &MultiLayout,
    arms: &OuterArms,
    frame: Frame,
    eblocks: &[MultiEblock],
    opening: &GibsonOpening,
) -> Vec<Placement> {
    let Frame { total, ins_start, left_len } = frame;
    let wrap = |x: i64| if total <= 0 { 0usize } else { usize::try_from(x.rem_euclid(total)).unwrap_or(0) };
    let mut placements: Vec<Placement> = Vec::with_capacity(layout.frags.len());
    let mut off = ins_start;
    for f in layout.frags {
        let len = len64(&f.seq);
        add_span(features, total, off, len, &f.name, "misc_feature");
        placements.push(Placement { name: f.name.clone(), start: wrap(off), end: wrap(off + len.max(1) - 1) });
        off += len;
    }
    let full_insert_len = off - ins_start;
    add_span(features, total, ins_start - left_len, left_len, &format!("Overlap L ({}°C)", js_num(arms.tm_left)), "misc_feature");
    add_span(features, total, ins_start + full_insert_len, len64(&arms.right), &format!("Overlap R ({}°C)", js_num(arms.tm_right)), "misc_feature");
    // The overlaps BETWEEN fragments. `boundaries[i-1]` is an offset on the laid-out strand; the
    // assembled frame starts the insert at `ins_start`, so the shift is one subtraction.
    for i in 1..layout.frags.len() {
        let (Some(b), Some(&len)) = (layout.boundaries.get(i - 1).copied(), layout.overlap_len.get(i - 1)) else { continue };
        let ol = js_slice(&layout.full, b, b + len);
        let tm = round1(gibson_tm(ol).unwrap_or(0.0));
        let (Some(a), Some(c)) = (layout.frags.get(i - 1), layout.frags.get(i)) else { continue };
        let name = format!("Overlap {}–{} ({}°C)", a.name, c.name, js_num(tm));
        add_span(features, total, ins_start + b - left_len, len, &name, "misc_feature");
    }
    annotate_split_pieces(features, layout, frame, eblocks);
    let (up_label, down_label) = cut_labels(opening);
    // Cut markers sit AT the true backbone↔insert junctions (first + last inserted base), not out
    // at the arm ends.
    add_span(features, total, ins_start, 1, &up_label, "misc_feature");
    add_span(features, total, ins_start + full_insert_len - 1, 1, &down_label, "misc_feature");
    placements
}

/// The pieces you actually order, when a fragment had to be split.
///
/// A fragment is a part — EGFP — and an eBlock is a tube. They are the same thing right up until the
/// fragment is longer than the synthesis cap, and then one part arrives as three tubes that OVERLAP
/// each other. ONLY WHEN THERE IS A SPLIT: with one piece per fragment the eBlock is the fragment
/// plus its arms, and drawing both would be the same span annotated twice.
fn annotate_split_pieces(features: &mut Vec<Feature>, layout: &MultiLayout, frame: Frame, eblocks: &[MultiEblock]) {
    let Frame { total, ins_start, left_len } = frame;
    let mut order: Vec<&str> = Vec::new();
    for eb in eblocks {
        if !order.iter().any(|f| *f == eb.fragment) {
            order.push(&eb.fragment);
        }
    }
    for fragment in order {
        let pieces: Vec<&MultiEblock> = eblocks.iter().filter(|eb| eb.fragment == fragment).collect();
        if pieces.len() < 2 {
            continue;
        }
        for (i, eb) in pieces.iter().enumerate() {
            add_span(features, total, ins_start + eb.start - left_len, eb.end - eb.start, &eb.name, "misc_feature");
            let Some(prev) = i.checked_sub(1).and_then(|p| pieces.get(p)) else { continue };
            // Consecutive eBlocks overlap by exactly the homology that joins them: how far the
            // previous one reaches past where this one starts.
            let ol_len = prev.end - eb.start;
            if ol_len <= 0 {
                continue;
            }
            let tm = eb.junction_tm.unwrap_or_else(|| round1(gibson_tm(js_slice(&layout.full, eb.start, eb.start + ol_len)).unwrap_or(0.0)));
            let name = format!("Overlap {}–{} ({}°C)", prev.name, eb.name, js_num(tm));
            add_span(features, total, ins_start + eb.start - left_len, ol_len, &name, "misc_feature");
        }
    }
}

/// Every junction checked for length, temperature and uniqueness.
fn multi_warnings(warnings: &mut Vec<String>, junctions: &[GibsonJunction], assembled: &str, circular: bool) {
    for j in junctions {
        let label = format!("{} ↔ {}", j.left, j.right);
        let len = len64(&j.seq);
        if len < 15 {
            warnings.push(format!("Junction {label} overlap is {len} bp (<15 bp — weak Gibson junction)"));
        }
        if !(55.0..=72.0).contains(&j.tm) {
            warnings.push(format!("Junction {label} Tm {}°C is outside 55–72°C", js_num(j.tm)));
        }
        let u = check_uniqueness(assembled, &j.seq, circular);
        if u.fwd != 1 {
            warnings.push(format!("Junction {label} overlap is not unique in the assembled plasmid ({} forward hits — misassembly risk)", u.fwd));
        } else if j.seq != revcomp(&j.seq) && u.rc > 0 {
            warnings.push(format!("Junction {label} overlap also matches a reverse-complement site (inverted-misassembly risk)"));
        }
    }
}
