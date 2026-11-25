//! Two plasmids end to end: are they the same, and if not, what changed?
//!
//! This is a different question from [`crate::find_align`], and the difference is the whole
//! module. Find's Alignment mode fits ONE sequence into a construct and reports its best local
//! hits: the right answer for "where does this oligo sit", useless for "is this map the corrected
//! map". A pair of plasmids needs every base accounted for on both sides, which means a GLOBAL
//! alignment, and for two circular molecules it means finding the turn first: a plasmid whose
//! origin was moved matches its parent at every base and shares no column with it.
//!
//! So the work is in three steps, and the first two exist to make the third affordable:
//!
//! 1. **The register.** Pack every k-mer of both sequences and keep the ones each holds exactly
//!    once. Each k-mer the two share once votes for one turn of `b` (the offset that would put it
//!    on top of its twin in `a`); the turn with the most votes wins, and the same vote is counted
//!    for `b` reverse-complemented, so a flipped map is recognised rather than called different.
//!    O(n) per seed length, against O(n·m) for a Smith-Waterman at every rotation.
//! 2. **The anchors.** On the turned pair, grow each shared unique k-mer into the longest exact
//!    stretch around it and chain the stretches that go forward on both sides. Two plasmids that
//!    differ in a handful of bases chain into a few long stretches, and the share of the shorter
//!    plasmid those stretches cover is what says whether the two are versions of each other at
//!    all ([`MIN_SHARE`]): below that the honest answer is "these do not line up", not a list of
//!    four hundred differences that mean nothing.
//! 3. **The stretches between them.** Only the gaps between chained anchors are aligned, with a
//!    global Needleman-Wunsch with affine gaps (same reasoning as [`crate::align`]: one 20 bp
//!    deletion must cost about what one 3 bp deletion costs, or clean flanks can never pay for
//!    it). A gap too wide for the table ([`NW_BUDGET`]) is re-anchored with a shorter seed, and
//!    one that still will not break up is reported as what it is — this many bases replaced by
//!    that many — rather than invented base by base.
//!
//! Three seed lengths are tried ([`SEEDS`]) and the one whose chain covers most is used: 16 finds
//! the register of near-identical plasmids in one pass, and 10 still finds it for a pair that
//! differs every few bases, where no 16-mer survives.
//!
//! Coordinates are 0-based and inclusive here, as everywhere in `bios-core`, and a span whose end
//! is before its start wraps the origin. Positions on `b` are reported in `b`'s OWN coordinates:
//! the turn and the flip are undone on the way out, so a caller never has to know what the
//! aligner did internally.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::coding::{CodingSettings, coding_residue, read_coding_settings};
use crate::model::{Construct, Feature, Strand};
use crate::seq::{clean_seq, revcomp};

/// Seed lengths tried for the register and the anchors, longest first.
pub const SEEDS: [usize; 3] = [16, 12, 10];
/// Shorter than this there is not enough sequence for a seed to mean anything, and a comparison
/// is refused rather than guessed at.
pub const MIN_LENGTH: usize = 24;
/// Most differences collected. A pair this far apart is being described, not listed.
pub const MAX_DIFFS: usize = 2_000;
/// Cells of one global alignment table: 4 MB of traceback, a few tens of milliseconds. A gap
/// wider than this is re-anchored instead.
const NW_BUDGET: usize = 4_000_000;
/// Anchors carried into the chain. The chain is O(cap²), so this bounds it at a few tens of
/// millions of comparisons in the worst case; the longest anchors are the ones kept.
const ANCHOR_CAP: usize = 4_000;
/// Base comparisons spent growing anchors before seeding gives up. Reached only by sequence made
/// almost entirely of repeats.
const GROW_BUDGET: usize = 20_000_000;
/// Share of the shorter plasmid the chained anchors must cover for the two to be called versions
/// of each other at all.
pub const MIN_SHARE: f64 = 0.25;
/// A chain covering this much is taken at once, without trying a shorter seed.
const GOOD_SHARE: f64 = 0.9;
/// How many times a stretch may be re-anchored with a shorter seed before it is reported whole.
const MAX_DEPTH: usize = 3;
/// Identity a stretch between two anchors must reach to be described base by base rather than
/// reported as replaced. Random DNA aligns at about two thirds, so this sits above that.
const REGION_IDENTITY: f64 = 0.7;
/// Stretches shorter than this on either side are always described base by base: a handful of
/// bases between two anchors is readable however little of it matches.
const REGION_MIN: usize = 30;

// Global alignment scoring. The ratio is what matters, as in [`crate::align`]: a mismatch must
// cost more than it earns back, or the aligner buys homology that is not there, and two gaps must
// cost more than one mismatch, or an equal-length change is reported as an insertion beside a
// deletion.
const MATCH: i32 = 2;
const MISMATCH: i32 = -3;
const GAP_OPEN: i32 = 5;
const GAP_EXTEND: i32 = 2;
const NEG: i32 = i32::MIN / 4;

// Traceback directions, one byte a cell, as in [`crate::align`]: H's origin in the low two bits,
// one bit each for E and F saying whether the gap extended itself or opened out of H.
const H_DIAG: u8 = 0;
const H_FROM_E: u8 = 1;
const H_FROM_F: u8 = 2;
const E_EXTEND: u8 = 1 << 2;
const F_EXTEND: u8 = 1 << 3;

/// What one difference is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// The same number of bases, read differently.
    Substitution,
    /// Bases `b` has that `a` has not.
    Insertion,
    /// Bases `a` has that `b` has not.
    Deletion,
}

/// Where a difference sits on one of the two plasmids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The changed bases themselves, 0-based and inclusive. `end` before `start` wraps the origin.
    At { start: usize, end: usize },
    /// This side has no bases of its own here: the change sits just after this 0-based base.
    After(usize),
    /// ...and this side has no base before it either: the change sits at the very start.
    Start,
}

/// One difference between the two, with its place on each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub kind: DiffKind,
    pub at_a: Place,
    pub at_b: Place,
    /// The bases on `a`'s side, 5'→3' on `a`'s plus strand. Empty for an insertion.
    pub a_bases: String,
    /// The bases on `b`'s side, read on the strand the comparison used. Empty for a deletion.
    pub b_bases: String,
}

impl Diff {
    /// Bases changed: the longer side of the two.
    pub fn length(&self) -> usize {
        self.a_bases.len().max(self.b_bases.len())
    }
}

/// One stretch the two plasmids run through side by side, aligned base to base (matches and
/// mismatches both): what maps a position on one to a position on the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Block {
    a: usize,
    /// On `b` as the aligner turned it, not as `b` is stored.
    b: usize,
    len: usize,
}

/// How a position on `b` as the aligner turned it goes back to `b` as it is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BMap {
    len: usize,
    rot: usize,
    flipped: bool,
}

impl BMap {
    fn original(&self, turned: usize) -> usize {
        if self.len == 0 {
            return 0;
        }
        let at = (turned + self.rot) % self.len;
        if self.flipped { self.len - 1 - at } else { at }
    }

    fn turned(&self, original: usize) -> usize {
        if self.len == 0 {
            return 0;
        }
        let at = if self.flipped { self.len - 1 - original.min(self.len - 1) } else { original % self.len };
        (at + self.len - self.rot % self.len) % self.len
    }

    fn place(&self, place: Place) -> Place {
        match place {
            Place::At { start, end } => Place::At { start: self.original(start), end: self.original(end) },
            Place::After(at) => Place::After(self.original(at)),
            Place::Start => Place::Start,
        }
    }
}

/// Why two plasmids were not compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unalignable {
    /// What to tell the caller, as a sentence.
    pub reason: String,
    /// Bases the two share in exactly matching stretches, at the best turn and strand tried.
    pub shared: usize,
    /// The longest single exactly matching stretch.
    pub longest: usize,
}

/// Two plasmids aligned end to end, and everything that differs.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// `b` was read reverse-complemented to line it up.
    pub flipped: bool,
    /// `b` was turned (both plasmids are circular, so the origin could be moved).
    pub rotated: bool,
    /// 0-based base of `b`, as `b` is stored, that lines up with base 0 of `a`.
    pub b_origin: usize,
    pub a_length: usize,
    pub b_length: usize,
    /// Columns in the alignment: matches, mismatches and gapped columns together. What identity
    /// is measured over.
    pub aligned_columns: usize,
    pub matches: usize,
    pub diffs: Vec<Diff>,
    /// The list stopped at [`MAX_DIFFS`]; there are more.
    pub more_diffs: bool,
    /// Bases the chained anchors matched exactly. What the verdict rests on.
    pub anchored: usize,
    /// The seed length that gave the best chain.
    pub seed: usize,
    blocks: Vec<Block>,
    map: BMap,
}

impl Comparison {
    /// Matching bases over aligned columns, 0..1. Gapped columns count against it.
    pub fn identity(&self) -> f64 {
        if self.aligned_columns == 0 { 0.0 } else { self.matches as f64 / self.aligned_columns as f64 }
    }

    /// The same molecule, base for base: nothing differs and neither plasmid has bases the other
    /// has not. A turn or a flip does not stop two plasmids being identical.
    pub fn identical(&self) -> bool {
        self.diffs.is_empty() && !self.more_diffs && self.a_length == self.b_length && self.matches == self.a_length
    }

    pub fn count(&self, kind: DiffKind) -> usize {
        self.diffs.iter().filter(|d| d.kind == kind).count()
    }

    /// The base of `b` that `a`'s 0-based base lines up with, when the two are aligned there.
    pub fn b_of_a(&self, a: usize) -> Option<usize> {
        let block = self.blocks.iter().find(|x| a >= x.a && a < x.a + x.len)?;
        Some(self.map.original(block.b + (a - block.a)))
    }

    /// The base of `a` that `b`'s 0-based base (on `b` as it is stored) lines up with.
    pub fn a_of_b(&self, b: usize) -> Option<usize> {
        let turned = self.map.turned(b);
        let block = self.blocks.iter().find(|x| turned >= x.b && turned < x.b + x.len)?;
        Some(block.a + (turned - block.b))
    }
}

fn two_bit(base: u8) -> Option<u64> {
    match base {
        b'A' => Some(0),
        b'C' => Some(1),
        b'G' => Some(2),
        b'T' => Some(3),
        _ => None,
    }
}

/// Two bases that pair: both plain ACGT and the same letter. An ambiguity code never matches, not
/// even itself, which is the conservatism [`crate::align`] uses: a run of Ns must not fabricate an
/// alignment.
fn same(x: Option<&u8>, y: Option<&u8>) -> bool {
    match (x, y) {
        (Some(p), Some(q)) => p == q && two_bit(*p).is_some(),
        _ => false,
    }
}

/// Packed k-mers of `seq`, one per start position, `None` where the window holds a base that is
/// not A, C, G or T.
fn kmers(seq: &[u8], k: usize) -> Vec<Option<u64>> {
    if k == 0 || k > 31 || seq.len() < k {
        return Vec::new();
    }
    let mask: u64 = (1u64 << (2 * k)) - 1;
    let mut out = Vec::with_capacity(seq.len() - k + 1);
    let (mut value, mut run) = (0u64, 0usize);
    for (i, &base) in seq.iter().enumerate() {
        match two_bit(base) {
            Some(code) => {
                value = ((value << 2) | code) & mask;
                run += 1;
            }
            None => {
                value = 0;
                run = 0;
            }
        }
        if i + 1 >= k {
            out.push((run >= k).then_some(value));
        }
    }
    out
}

/// Where each k-mer that occurs EXACTLY ONCE sits. A k-mer the sequence repeats says nothing
/// about which copy is which, so it is left out rather than guessed at.
fn once_only(kmers: &[Option<u64>]) -> HashMap<u64, usize> {
    let mut seen: HashMap<u64, Option<usize>> = HashMap::with_capacity(kmers.len());
    for (i, value) in kmers.iter().enumerate() {
        if let Some(value) = value {
            seen.entry(*value).and_modify(|slot| *slot = None).or_insert(Some(i));
        }
    }
    seen.into_iter().filter_map(|(value, at)| at.map(|i| (value, i))).collect()
}

/// How many turns of `b` are worth trying out. A length change splits the vote — the k-mers before
/// an insertion vote for one turn and the ones after it for another, and the second group can be
/// the larger — so the best-voted turn is not always the one that lines the two origins up. The
/// candidates are judged on the alignment they give, not on their votes.
const TURNS_TRIED: usize = 3;

/// The turns of `b` that might line it up with `a`, best voted first: every k-mer the two hold
/// once each votes for the turn that would put it on top of its twin. With no turn to look for
/// (either plasmid linear) the only candidate is no turn at all, and the vote is a count of what
/// the two share.
fn register(a: &[u8], b: &[u8], rotate: bool, k: usize) -> Vec<(usize, usize)> {
    let in_a = once_only(&kmers(a, k));
    let in_b = once_only(&kmers(b, k));
    if in_a.is_empty() || in_b.is_empty() || b.is_empty() {
        return Vec::new();
    }
    if !rotate {
        let shared = in_b.keys().filter(|value| in_a.contains_key(*value)).count();
        return if shared == 0 { Vec::new() } else { vec![(0, shared)] };
    }
    let n = b.len();
    let mut votes: HashMap<usize, usize> = HashMap::new();
    for (value, at_b) in &in_b {
        if let Some(at_a) = in_a.get(value) {
            let turn = (at_b + n - at_a % n) % n;
            *votes.entry(turn).or_default() += 1;
        }
    }
    let mut ranked: Vec<(usize, usize)> = votes.into_iter().collect();
    // On a tie the smaller turn, so the answer does not depend on the hash order.
    ranked.sort_by_key(|&(turn, count)| (Reverse(count), turn));
    ranked.truncate(TURNS_TRIED);
    ranked
}

fn turn_by(b: &[u8], rot: usize) -> Vec<u8> {
    if b.is_empty() || rot.is_multiple_of(b.len()) {
        return b.to_vec();
    }
    let (head, tail) = b.split_at(rot % b.len());
    [tail, head].concat()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Anchor {
    a: usize,
    b: usize,
    len: usize,
}

/// The exactly matching stretches the two share, from k-mers each holds once, each grown as far
/// as it matches either way. A seed inside a stretch already grown on the same diagonal is
/// skipped, which is what keeps this linear instead of growing the same stretch once per base.
fn anchors(a: &[u8], b: &[u8], k: usize, budget: &mut usize) -> Vec<Anchor> {
    let in_b = once_only(&kmers(b, k));
    let of_a = kmers(a, k);
    let in_a = once_only(&of_a);
    let mut out: Vec<Anchor> = Vec::new();
    let mut reach: HashMap<i64, usize> = HashMap::new();
    for (i, value) in of_a.iter().enumerate() {
        let Some(value) = value else { continue };
        if in_a.get(value) != Some(&i) {
            continue;
        }
        let Some(&j) = in_b.get(value) else { continue };
        let diagonal = j as i64 - i as i64;
        if reach.get(&diagonal).is_some_and(|&upto| i < upto) {
            continue;
        }
        if *budget == 0 {
            break;
        }
        let (mut s, mut t, mut len) = (i, j, k);
        while s > 0 && t > 0 && same(a.get(s - 1), b.get(t - 1)) && *budget > 0 {
            s -= 1;
            t -= 1;
            len += 1;
            *budget -= 1;
        }
        while same(a.get(s + len), b.get(t + len)) && *budget > 0 {
            len += 1;
            *budget -= 1;
        }
        reach.insert(diagonal, s + len);
        out.push(Anchor { a: s, b: t, len });
    }
    out
}

/// What `here` adds to a chain that `there` ends, or `None` when it adds nothing.
///
/// Two anchors may overlap by a few bases: a stretch beside a deletion grows past where the
/// deletion begins whenever the next bases happen to match. Rejecting such a pair would throw
/// away a whole anchor to save a handful of chance bases, so the overlap is TRIMMED off the later
/// anchor's start instead, which is what makes a 3 kb stretch survive a lucky base beside a gap.
fn gain(there: &Anchor, here: &Anchor) -> Option<usize> {
    if there.a >= here.a || there.b >= here.b {
        return None;
    }
    let trim = (there.a + there.len).saturating_sub(here.a).max((there.b + there.len).saturating_sub(here.b));
    here.len.checked_sub(trim).filter(|&left| left > 0)
}

/// The heaviest chain of anchors that goes forward on both plasmids: the backbone of the
/// alignment. Anchors off that chain are repeats and near-repeats, which would each have to be
/// paid for with a gap either side. The chain comes back trimmed, so it only ever moves forward.
fn chain(mut found: Vec<Anchor>) -> Vec<Anchor> {
    if found.len() > ANCHOR_CAP {
        found.sort_by_key(|x| Reverse(x.len));
        found.truncate(ANCHOR_CAP);
    }
    found.sort_by_key(|x| (x.a, x.b));
    let n = found.len();
    let mut best = vec![0usize; n];
    let mut from = vec![usize::MAX; n];
    let (mut top, mut top_at) = (0usize, usize::MAX);
    for i in 0..n {
        let Some(&here) = found.get(i) else { continue };
        let mut score = here.len;
        for j in 0..i {
            let (Some(&there), Some(&was)) = (found.get(j), best.get(j)) else { continue };
            let Some(adds) = gain(&there, &here) else { continue };
            if was + adds > score {
                score = was + adds;
                if let Some(slot) = from.get_mut(i) {
                    *slot = j;
                }
            }
        }
        if let Some(slot) = best.get_mut(i) {
            *slot = score;
        }
        if score > top {
            top = score;
            top_at = i;
        }
    }
    let mut back = Vec::new();
    let mut at = top_at;
    while let Some(&here) = found.get(at) {
        back.push(here);
        match from.get(at) {
            Some(&next) if next != usize::MAX => at = next,
            _ => break,
        }
    }
    back.reverse();
    trimmed(back)
}

/// The chain with each anchor's start pulled off the one before it, so no two share a base.
fn trimmed(chain: Vec<Anchor>) -> Vec<Anchor> {
    let mut out: Vec<Anchor> = Vec::with_capacity(chain.len());
    for mut here in chain {
        if let Some(prev) = out.last() {
            let trim = (prev.a + prev.len).saturating_sub(here.a).max((prev.b + prev.len).saturating_sub(here.b));
            match here.len.checked_sub(trim).filter(|&left| left > 0) {
                Some(left) => {
                    here = Anchor { a: here.a + trim, b: here.b + trim, len: left };
                }
                None => continue,
            }
        }
        out.push(here);
    }
    out
}

/// The alignment as it is built: the differences, the stretches that map one plasmid onto the
/// other, and the tallies identity is read from.
struct Work<'a> {
    a: &'a [u8],
    b: &'a [u8],
    diffs: Vec<Diff>,
    blocks: Vec<Block>,
    matches: usize,
    columns: usize,
    anchored: usize,
    more: bool,
    grow: usize,
}

/// A difference's place on a side that has no bases there: after the base before it, or at the
/// very start.
fn before(at: usize) -> Place {
    match at.checked_sub(1) {
        Some(previous) => Place::After(previous),
        None => Place::Start,
    }
}

impl<'a> Work<'a> {
    fn new(a: &'a [u8], b: &'a [u8]) -> Self {
        Work { a, b, diffs: Vec::new(), blocks: Vec::new(), matches: 0, columns: 0, anchored: 0, more: false, grow: GROW_BUDGET }
    }

    fn push(&mut self, diff: Diff) {
        if self.diffs.len() >= MAX_DIFFS {
            self.more = true;
            return;
        }
        self.diffs.push(diff);
    }

    fn bases(seq: &[u8], from: usize, to: usize) -> String {
        seq.get(from..to).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default()
    }

    /// A stretch of `b` that `a` does not have. An empty stretch is nothing, not a difference.
    fn insertion(&mut self, at_a: usize, b0: usize, b1: usize) {
        let Some(last) = b1.checked_sub(1).filter(|_| b1 > b0) else { return };
        self.columns += b1 - b0;
        let bases = Self::bases(self.b, b0, b1);
        self.push(Diff {
            kind: DiffKind::Insertion,
            at_a: before(at_a),
            at_b: Place::At { start: b0, end: last },
            a_bases: String::new(),
            b_bases: bases,
        });
    }

    /// A stretch of `a` that `b` does not have.
    fn deletion(&mut self, a0: usize, a1: usize, at_b: usize) {
        let Some(last) = a1.checked_sub(1).filter(|_| a1 > a0) else { return };
        self.columns += a1 - a0;
        let bases = Self::bases(self.a, a0, a1);
        self.push(Diff {
            kind: DiffKind::Deletion,
            at_a: Place::At { start: a0, end: last },
            at_b: before(at_b),
            a_bases: bases,
            b_bases: String::new(),
        });
    }

    fn substitution(&mut self, a0: usize, b0: usize, len: usize) {
        if len == 0 {
            return;
        }
        self.columns += len;
        let (a_bases, b_bases) = (Self::bases(self.a, a0, a0 + len), Self::bases(self.b, b0, b0 + len));
        self.push(Diff {
            kind: DiffKind::Substitution,
            at_a: Place::At { start: a0, end: a0 + len - 1 },
            at_b: Place::At { start: b0, end: b0 + len - 1 },
            a_bases,
            b_bases,
        });
    }

    /// A stretch aligned base to base, matches and mismatches together: what carries a position
    /// from one plasmid to the other.
    fn block(&mut self, a: usize, b: usize, len: usize) {
        if let Some(last) = self.blocks.last_mut()
            && last.a + last.len == a
            && last.b + last.len == b
        {
            last.len += len;
            return;
        }
        self.blocks.push(Block { a, b, len });
    }

    /// One stretch of each plasmid, between two anchors (or before the first and after the last),
    /// aligned and written down.
    fn region(&mut self, a0: usize, a1: usize, b0: usize, b1: usize, depth: usize) {
        let (la, lb) = (a1.saturating_sub(a0), b1.saturating_sub(b0));
        match (la, lb) {
            (0, 0) => {}
            (0, _) => self.insertion(a0, b0, b1),
            (_, 0) => self.deletion(a0, a1, b0),
            _ if la.saturating_mul(lb) <= NW_BUDGET => self.global(a0, a1, b0, b1),
            _ if depth < MAX_DEPTH => self.split(a0, a1, b0, b1, depth),
            // Too wide to align and too shapeless to break up: say what it is rather than invent
            // a base-by-base story about it.
            _ => {
                self.deletion(a0, a1, b0);
                self.insertion(a1, b0, b1);
            }
        }
    }

    /// A stretch too wide for one table: anchor inside it with a shorter seed and align the gaps
    /// between those anchors.
    fn split(&mut self, a0: usize, a1: usize, b0: usize, b1: usize, depth: usize) {
        let seed = SEEDS.get(depth + 1).copied().unwrap_or(SEEDS[SEEDS.len() - 1]);
        let (Some(a), Some(b)) = (self.a.get(a0..a1), self.b.get(b0..b1)) else { return };
        let mut budget = self.grow;
        let inside = chain(anchors(a, b, seed, &mut budget));
        self.grow = budget;
        if inside.is_empty() {
            self.deletion(a0, a1, b0);
            self.insertion(a1, b0, b1);
            return;
        }
        self.walk(&inside, (a0, a1), (b0, b1), depth + 1);
    }

    /// The chain itself: each anchor is a matching stretch, and each hole between two of them is a
    /// region of its own.
    fn walk(&mut self, chain: &[Anchor], a: (usize, usize), b: (usize, usize), depth: usize) {
        let (mut at_a, mut at_b) = (a.0, b.0);
        for anchor in chain {
            let (start_a, start_b) = (a.0 + anchor.a, b.0 + anchor.b);
            self.region(at_a, start_a, at_b, start_b, depth);
            self.block(start_a, start_b, anchor.len);
            self.matches += anchor.len;
            self.columns += anchor.len;
            self.anchored += anchor.len;
            at_a = start_a + anchor.len;
            at_b = start_b + anchor.len;
        }
        self.region(at_a, a.1, at_b, b.1, depth);
    }

    /// Needleman-Wunsch with affine gaps over one stretch: both ends anchored, because between two
    /// exact anchors (or at the ends of two linear molecules) every base must be accounted for.
    ///
    /// A stretch that turns out not to resemble its counterpart at all is then thrown away and
    /// reported whole. Aligning two unrelated kilobases base by base is what produces four hundred
    /// differences that mean nothing: random DNA lines up at about two thirds identity, and every
    /// one of those columns would be reported as a change. "This stretch was replaced" is both
    /// shorter and truer.
    fn global(&mut self, a0: usize, a1: usize, b0: usize, b1: usize) {
        let (Some(a), Some(b)) = (self.a.get(a0..a1), self.b.get(b0..b1)) else { return };
        let Some(dir) = fill(a, b) else {
            // No table could be allocated: report the stretch whole rather than crash.
            self.deletion(a0, a1, b0);
            self.insertion(a1, b0, b1);
            return;
        };
        let mark = (self.diffs.len(), self.blocks.len(), self.blocks.last().copied(), self.matches, self.columns);
        self.trace(&dir, a, b, a0, b0);
        let (matched, columns) = (self.matches.saturating_sub(mark.3), self.columns.saturating_sub(mark.4));
        let alike = columns == 0 || matched as f64 / columns as f64 >= REGION_IDENTITY;
        if alike || a.len() < REGION_MIN || b.len() < REGION_MIN {
            return;
        }
        self.diffs.truncate(mark.0);
        self.blocks.truncate(mark.1);
        if let (Some(last), Some(was)) = (self.blocks.last_mut(), mark.2) {
            *last = was;
        }
        self.matches = mark.3;
        self.columns = mark.4;
        self.deletion(a0, a1, b0);
        self.insertion(a1, b0, b1);
    }

    /// Walk the filled table back from the far corner, then forward again writing down what it
    /// says, runs of the same thing together.
    fn trace(&mut self, dir: &[u8], a: &[u8], b: &[u8], a0: usize, b0: usize) {
        let w = b.len() + 1;
        let (mut i, mut j) = (a.len(), b.len());
        let mut steps: Vec<Step> = Vec::new();
        let mut state = State::H;
        while i > 0 || j > 0 {
            let cell = dir.get(i * w + j).copied().unwrap_or(0);
            if i == 0 {
                steps.push(Step::GapInA);
                j -= 1;
                continue;
            }
            if j == 0 {
                steps.push(Step::GapInB);
                i -= 1;
                continue;
            }
            match state {
                State::E => {
                    steps.push(Step::GapInA);
                    j -= 1;
                    if cell & E_EXTEND == 0 {
                        state = State::H;
                    }
                }
                State::F => {
                    steps.push(Step::GapInB);
                    i -= 1;
                    if cell & F_EXTEND == 0 {
                        state = State::H;
                    }
                }
                State::H => match cell & 3 {
                    H_FROM_E => state = State::E,
                    H_FROM_F => state = State::F,
                    _ => {
                        steps.push(Step::Diagonal(same(a.get(i - 1), b.get(j - 1))));
                        i -= 1;
                        j -= 1;
                    }
                },
            }
        }
        steps.reverse();
        self.write(&steps, a0, b0);
    }

    /// The traceback's steps as the differences a person reads: a run of matches is one block, a
    /// run of mismatches one substitution, a run of gaps one insertion or deletion.
    fn write(&mut self, steps: &[Step], a0: usize, b0: usize) {
        let (mut i, mut j) = (a0, b0);
        let mut k = 0;
        while let Some(&step) = steps.get(k) {
            let mut run = 1;
            while steps.get(k + run) == Some(&step) {
                run += 1;
            }
            match step {
                Step::Diagonal(matched) => {
                    self.block(i, j, run);
                    if matched {
                        self.matches += run;
                        self.columns += run;
                    } else {
                        self.substitution(i, j, run);
                    }
                    i += run;
                    j += run;
                }
                Step::GapInA => {
                    self.insertion(i, j, j + run);
                    j += run;
                }
                Step::GapInB => {
                    self.deletion(i, i + run, j);
                    i += run;
                }
            }
            k += run;
        }
    }
}

/// One step of a global alignment, in query order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Both plasmids move on; `true` when the bases pair.
    Diagonal(bool),
    /// Only `b` moves on: a base `b` has that `a` has not.
    GapInA,
    /// Only `a` moves on.
    GapInB,
}

enum State {
    H,
    E,
    F,
}

/// The O(n·m) fill. Score rows roll; only the traceback directions are kept, one byte a cell.
fn fill(a: &[u8], b: &[u8]) -> Option<Vec<u8>> {
    let w = b.len() + 1;
    let cells = (a.len() + 1).checked_mul(w)?;
    let mut dir: Vec<u8> = Vec::new();
    dir.try_reserve_exact(cells).ok()?;
    dir.resize(cells, 0);

    // H and F of the row above; E rolls along the row.
    let mut above_h: Vec<i32> = Vec::new();
    above_h.try_reserve_exact(w).ok()?;
    let mut above_f: Vec<i32> = Vec::new();
    above_f.try_reserve_exact(w).ok()?;
    for j in 0..w {
        above_h.push(if j == 0 { 0 } else { -(GAP_OPEN + GAP_EXTEND * j as i32) });
        above_f.push(NEG);
    }
    let mut row_h = above_h.clone();
    let mut row_f = above_f.clone();

    for i in 1..=a.len() {
        let here = a.get(i - 1);
        let mut e = NEG;
        let mut left = -(GAP_OPEN + GAP_EXTEND * i as i32);
        if let Some(slot) = row_h.first_mut() {
            *slot = left;
        }
        if let Some(slot) = row_f.first_mut() {
            *slot = left;
        }
        for j in 1..w {
            let mut flags = 0u8;
            let open_e = left - (GAP_OPEN + GAP_EXTEND);
            let extend_e = e - GAP_EXTEND;
            if extend_e > open_e {
                e = extend_e;
                flags |= E_EXTEND;
            } else {
                e = open_e;
            }
            let up_h = above_h.get(j).copied().unwrap_or(NEG);
            let up_f = above_f.get(j).copied().unwrap_or(NEG);
            let open_f = up_h - (GAP_OPEN + GAP_EXTEND);
            let extend_f = up_f - GAP_EXTEND;
            let f = if extend_f > open_f {
                flags |= F_EXTEND;
                extend_f
            } else {
                open_f
            };
            let step = if same(here, b.get(j - 1)) { MATCH } else { MISMATCH };
            let diagonal = above_h.get(j - 1).copied().unwrap_or(NEG).saturating_add(step);
            let mut best = diagonal;
            let mut from = H_DIAG;
            if e > best {
                best = e;
                from = H_FROM_E;
            }
            if f > best {
                best = f;
                from = H_FROM_F;
            }
            left = best;
            if let (Some(h), Some(ff)) = (row_h.get_mut(j), row_f.get_mut(j)) {
                *h = best;
                *ff = f;
            }
            if let Some(slot) = dir.get_mut(i * w + j) {
                *slot = flags | from;
            }
        }
        std::mem::swap(&mut above_h, &mut row_h);
        std::mem::swap(&mut above_f, &mut row_f);
    }
    Some(dir)
}

/// One register tried: a strand, a turn and the chain it gives.
struct Try {
    flipped: bool,
    rot: usize,
    seed: usize,
    turned: Vec<u8>,
    chain: Vec<Anchor>,
    covered: usize,
}

impl Try {
    /// What makes one try better than another: the bases its chain covers first, then no turn
    /// over a turn, a smaller turn over a larger one, and the plain strand over the flipped one.
    /// Everything after the first is only there to make the answer the same every time.
    fn rank(&self) -> (usize, usize, Reverse<usize>, usize) {
        (self.covered, usize::from(self.rot == 0), Reverse(self.rot), usize::from(!self.flipped))
    }
}

/// The best turn and strand at one seed length.
fn at_seed(a: &[u8], b: &[u8], rotate: bool, seed: usize, grow: &mut usize) -> Option<Try> {
    let mut best: Option<Try> = None;
    for flipped in [false, true] {
        let read = if flipped { revcomp(&String::from_utf8_lossy(b)).into_bytes() } else { b.to_vec() };
        for (rot, _) in register(a, &read, rotate, seed) {
            let turned = turn_by(&read, rot);
            let chain = chain(anchors(a, &turned, seed, grow));
            let covered: usize = chain.iter().map(|x| x.len).sum();
            let tried = Try { flipped, rot, seed, turned, chain, covered };
            if best.as_ref().is_none_or(|had| tried.rank() > had.rank()) {
                best = Some(tried);
            }
        }
    }
    best
}

fn best_register(a: &[u8], b: &[u8], rotate: bool, grow: &mut usize) -> Option<Try> {
    let shorter = a.len().min(b.len()).max(1);
    // A shorter seed must cover MEANINGFULLY more to be preferred, not just a little more. Two
    // unrelated stretches share a chance 10-mer about once per million base pairs of each other,
    // and those chance anchors add up to a few bases: enough to beat a longer seed on coverage,
    // and worth nothing. A real gain — the longer seed finding no register at all because the two
    // differ every few bases — is many times this.
    let margin = (shorter / 50).max(16);
    let mut best: Option<Try> = None;
    for &seed in &SEEDS {
        if seed * 2 > shorter {
            continue;
        }
        let tried = at_seed(a, b, rotate, seed, grow);
        match (&best, tried) {
            (_, None) => {}
            (None, Some(tried)) => best = Some(tried),
            (Some(had), Some(tried)) if tried.covered > had.covered + margin => best = Some(tried),
            _ => {}
        }
        if best.as_ref().is_some_and(|had| had.covered as f64 / shorter as f64 >= GOOD_SHARE) {
            break;
        }
    }
    best
}

/// Compare two plasmids end to end. `a` is the one whose coordinates lead; both sequences are
/// cleaned first, so numbered or wrapped text is accepted.
///
/// Fails, rather than answering, when the two do not line up: a difference list between plasmids
/// that are not versions of each other is noise, and saying so is the useful answer.
pub fn compare(a_seq: &str, a_circular: bool, b_seq: &str, b_circular: bool) -> Result<Comparison, Unalignable> {
    let a = clean_seq(a_seq).into_bytes();
    let b = clean_seq(b_seq).into_bytes();
    let shorter = a.len().min(b.len());
    if shorter < MIN_LENGTH {
        return Err(Unalignable {
            reason: format!("a comparison needs at least {MIN_LENGTH} bases on each side; these are {} and {} bp", a.len(), b.len()),
            shared: 0,
            longest: 0,
        });
    }
    let rotate = a_circular && b_circular;
    let mut grow = GROW_BUDGET;
    let found = best_register(&a, &b, rotate, &mut grow);
    let longest = found.as_ref().map_or(0, |had| had.chain.iter().map(|x| x.len).max().unwrap_or(0));
    let shared = found.as_ref().map_or(0, |had| had.covered);
    let Some(had) = found.filter(|had| had.covered as f64 / shorter as f64 >= MIN_SHARE) else {
        return Err(Unalignable {
            reason: format!(
                "these two do not line up: only {shared} bp of exactly matching sequence was found between them (longest stretch {longest} bp, {:.0}% of the shorter plasmid). They are not versions of the same plasmid, so no list of differences is given",
                100.0 * shared as f64 / shorter as f64
            ),
            shared,
            longest,
        });
    };

    let mut work = Work::new(&a, &had.turned);
    work.grow = grow;
    work.walk(&had.chain, (0, a.len()), (0, had.turned.len()), 0);
    let map = BMap { len: b.len(), rot: had.rot, flipped: had.flipped };
    let mut diffs = work.diffs;
    for diff in &mut diffs {
        diff.at_b = map.place(diff.at_b);
    }
    Ok(Comparison {
        flipped: had.flipped,
        rotated: rotate && had.rot % b.len().max(1) != 0,
        b_origin: map.original(0),
        a_length: a.len(),
        b_length: b.len(),
        aligned_columns: work.columns,
        matches: work.matches,
        diffs,
        more_diffs: work.more,
        anchored: work.anchored,
        seed: had.seed,
        blocks: work.blocks,
        map,
    })
}

// ---------------------------------------------------------------------------------------------
// What a difference means where it lands: the feature it is in, and the protein it changes.
// ---------------------------------------------------------------------------------------------

/// Whether a feature covers a 0-based position, any of its pieces counting, a piece whose end is
/// before its start wrapping the origin.
pub fn covers(f: &Feature, pos: usize) -> bool {
    let within = |start: usize, end: usize| if end < start { pos >= start || pos <= end } else { pos >= start && pos <= end };
    if f.segments.len() > 1 {
        return f.segments.iter().any(|piece| within(piece.start, piece.end));
    }
    within(f.start, f.end)
}

/// The NARROWEST feature covering a 0-based position, by its index: the one that says the most
/// about what the change hits. A feature with no name is skipped, as it tells a reader nothing.
pub fn feature_at(features: &[Feature], length: usize, pos: usize) -> Option<usize> {
    features.iter().enumerate().filter(|(_, f)| !f.name.trim().is_empty() && covers(f, pos)).min_by_key(|(_, f)| f.span(length)).map(|(i, _)| i)
}

/// The bases a feature covers, read 5'→3' on its own strand. `None` for a feature with several
/// pieces or a location Bios cannot interpret: its reading frame is not this module's to guess.
pub fn feature_bases(c: &Construct, f: &Feature) -> Option<String> {
    if f.locations().is_some() || f.location_unsupported.is_some() {
        return None;
    }
    let n = c.len();
    if f.start >= n || f.end >= n {
        return None;
    }
    let plus = if f.end < f.start {
        if !c.circular {
            return None;
        }
        format!("{}{}", c.sequence.get(f.start..)?, c.sequence.get(..=f.end)?)
    } else {
        c.sequence.get(f.start..=f.end)?.to_string()
    };
    Some(if f.strand == Strand::Reverse { revcomp(&plus) } else { plus })
}

/// Where a 0-based plus-strand position falls inside [`feature_bases`], or `None` when it is
/// outside the feature.
pub fn feature_offset(f: &Feature, length: usize, pos: usize) -> Option<usize> {
    if f.locations().is_some() || f.location_unsupported.is_some() {
        return None;
    }
    let span = f.span(length);
    let from_start = if f.end < f.start {
        if pos >= f.start {
            pos - f.start
        } else if pos <= f.end {
            length - f.start + pos
        } else {
            return None;
        }
    } else if pos < f.start || pos > f.end {
        return None;
    } else {
        pos - f.start
    };
    Some(if f.strand == Strand::Reverse { span.checked_sub(from_start + 1)? } else { from_start })
}

/// What a change does to a coding feature's protein.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProteinEffect {
    /// Codons read differently. Numbers are 1-based on the protein.
    Codons {
        first: usize,
        last: usize,
        from_codons: String,
        to_codons: String,
        /// One letter a residue.
        from_aa: String,
        to_aa: String,
        /// The protein is unchanged: the change is silent.
        silent: bool,
    },
    /// A length change that is not a whole number of codons: everything downstream is read in
    /// another frame.
    Frameshift { at_codon: usize, bases: usize, added: bool },
    /// A whole number of codons added or removed; the rest of the protein still reads the same.
    InFrame { at_codon: usize, codons: usize, added: bool },
    /// The change is in the feature but outside its reading frame (in the bases `codon_start`
    /// drops), or the frame runs out before the change.
    OutsideFrame,
}

fn residues(coding: &str, settings: &CodingSettings, first: usize, last: usize) -> String {
    let total = coding.len() / 3;
    (first..=last)
        .filter_map(|ci| {
            let codon = coding.get(ci * 3..ci * 3 + 3)?;
            Some(coding_residue(codon, settings, ci == 0, ci + 1 == total))
        })
        .collect()
}

fn codons_of(c: &Construct, f: &Feature) -> Option<(String, CodingSettings)> {
    if f.kind != "CDS" {
        return None;
    }
    let settings = read_coding_settings(&f.notes);
    if settings.unavailable.is_some() {
        return None;
    }
    let own = feature_bases(c, f)?;
    let phase = usize::try_from(settings.phase).ok()?;
    let coding = own.get(phase..)?.to_string();
    Some((coding, settings))
}

/// What a substitution at plus-strand `pos` does to the protein of a coding feature. `new` is the
/// other plasmid's bases at those columns, read on the plus strand of the plasmid `f` belongs to.
pub fn substitution_effect(c: &Construct, f: &Feature, pos: usize, new: &str) -> Option<ProteinEffect> {
    let (coding, settings) = codons_of(c, f)?;
    let phase = usize::try_from(settings.phase).ok()?;
    let reverse = f.strand == Strand::Reverse;
    // On the reverse strand the feature reads the other way, so the block's own-strand start is
    // its plus-strand END, and the replacement bases are complemented.
    let plus_start = if reverse { pos + new.len().saturating_sub(1) } else { pos };
    let own_at = feature_offset(f, c.len(), plus_start)?;
    let put = if reverse { revcomp(new) } else { new.to_string() };
    let at = own_at.checked_sub(phase)?;
    if at + put.len() > coding.len() {
        return Some(ProteinEffect::OutsideFrame);
    }
    let (first, last) = (at / 3, (at + put.len() - 1) / 3);
    if (last + 1) * 3 > coding.len() {
        return Some(ProteinEffect::OutsideFrame);
    }
    let mut changed = coding.clone();
    changed.replace_range(at..at + put.len(), &put);
    let (from_codons, to_codons) = (coding.get(first * 3..(last + 1) * 3)?.to_string(), changed.get(first * 3..(last + 1) * 3)?.to_string());
    let from_aa = residues(&coding, &settings, first, last);
    let to_aa = residues(&changed, &settings, first, last);
    Some(ProteinEffect::Codons { first: first + 1, last: last + 1, from_codons, to_codons, silent: from_aa == to_aa, from_aa, to_aa })
}

/// What an insertion or deletion of `bases` bases at plus-strand `pos` does to the protein of a
/// coding feature.
pub fn indel_effect(c: &Construct, f: &Feature, pos: usize, bases: usize, added: bool) -> Option<ProteinEffect> {
    let (coding, settings) = codons_of(c, f)?;
    let phase = usize::try_from(settings.phase).ok()?;
    let own_at = feature_offset(f, c.len(), pos)?;
    let at = match own_at.checked_sub(phase) {
        Some(at) if at < coding.len() => at,
        _ => return Some(ProteinEffect::OutsideFrame),
    };
    let codon = at / 3 + 1;
    Some(if bases.is_multiple_of(3) {
        ProteinEffect::InFrame { at_codon: codon, codons: bases / 3, added }
    } else {
        ProteinEffect::Frameshift { at_codon: codon, bases, added }
    })
}

/// The three-letter name of a one-letter residue, for naming a change the way a person writes it
/// (`Gly42Val`).
pub fn three_letter(aa: char) -> &'static str {
    match aa.to_ascii_uppercase() {
        'A' => "Ala",
        'R' => "Arg",
        'N' => "Asn",
        'D' => "Asp",
        'C' => "Cys",
        'Q' => "Gln",
        'E' => "Glu",
        'G' => "Gly",
        'H' => "His",
        'I' => "Ile",
        'L' => "Leu",
        'K' => "Lys",
        'M' => "Met",
        'F' => "Phe",
        'P' => "Pro",
        'S' => "Ser",
        'T' => "Thr",
        'W' => "Trp",
        'Y' => "Tyr",
        'V' => "Val",
        '*' => "Ter",
        _ => "Xaa",
    }
}

/// A protein effect as one line: `codon 42 GGC→GTC, Gly42Val`.
pub fn describe_effect(effect: &ProteinEffect) -> String {
    match effect {
        ProteinEffect::Codons { first, last, from_codons, to_codons, from_aa, to_aa, silent } => {
            let where_ = if first == last { format!("codon {first}") } else { format!("codons {first}..{last}") };
            let change = from_aa
                .chars()
                .zip(to_aa.chars())
                .enumerate()
                .filter(|(_, (was, now))| was != now)
                .map(|(i, (was, now))| format!("{}{}{}", three_letter(was), first + i, three_letter(now)))
                .collect::<Vec<_>>()
                .join(", ");
            let verdict = if *silent { "silent".to_string() } else { format!("changes the protein: {change}") };
            format!("{where_} {from_codons}→{to_codons}, {verdict}")
        }
        ProteinEffect::Frameshift { at_codon, bases, added } => {
            format!("{bases} bp {} at codon {at_codon}: shifts the reading frame from there on", if *added { "added" } else { "removed" })
        }
        ProteinEffect::InFrame { at_codon, codons, added } => {
            format!(
                "{codons} codon{} {} at codon {at_codon}, in frame",
                if *codons == 1 { "" } else { "s" },
                if *added { "added" } else { "removed" }
            )
        }
        ProteinEffect::OutsideFrame => "inside the feature but outside its reading frame".to_string(),
    }
}
