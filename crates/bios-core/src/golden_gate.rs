//! Golden Gate / MoClo assembly: one type IIS enzyme, a destination vector and a set of parts,
//! joined in one tube by the overhangs the enzyme leaves.
//!
//! The whole method rests on one fact about a type IIS enzyme: it cuts OUTSIDE its recognition
//! site, so the site leaves with the piece that is thrown away and the piece that is kept carries
//! only a short single-stranded overhang. Two pieces whose overhangs are complementary anneal, and
//! nothing else does, so the order of an assembly is fixed by its overhangs alone.
//!
//! Every number here is derived from the enzyme table, never written down. For BsaI the table says
//! `GGTCTC` with `top_snip_offset: 7, bottom_snip_offset: 11`, so the top strand is cut
//! `7 - 6 = 1` base past the site and the bottom strand `11 - 6 = 5` bases past it — GGTCTC(1/5),
//! a four-base 5' overhang. [`geometry`] does that subtraction and nothing else; an enzyme whose
//! entry says something different (a three-base overhang for SapI, a ten-base spacer for BtgZI)
//! comes out right with no special case.
//!
//! # The two sides of a cut
//!
//! A cut at gap `g` leaving a four-base 5' overhang leaves the piece on the left recessed on its
//! top strand, and the piece on the right carrying plus-strand bases `g..g+3` as a single strand.
//! Both pieces therefore name the SAME four bases as their overhang, and in the ligated product
//! those bases appear exactly once. So a piece is written here as a top strand that INCLUDES the
//! overhangs at both of its ends: its first `k` bases are its 5' overhang, its last `k` bases its
//! 3' overhang, and the product is the pieces joined with each shared `k` counted once. That is
//! also how a MoClo part is written on paper — `AATG…GCTT` — so a part handed to
//! [`design_golden_gate`] is given in exactly that form, from its first fusion base to its last,
//! with no type IIS tails on it.
//!
//! # The destination vector
//!
//! A destination vector carries two sites pointing OUTWARD from the stuffer it holds, so that both
//! leave with the stuffer and the backbone comes out of the digest with two clean overhangs and no
//! site left to re-cut it. [`choose_arc`] does not assume that layout: it looks for the one arc
//! between two cuts that holds no recognition site and no further cut, which is the same thing said
//! as a property of the sequence. A vector with a third site inside its stuffer still works.
//!
//! Coordinates inside this module are 0-based, and gap coordinates follow [`crate::enzymes`].

use std::collections::HashMap;

use crate::edits::{EditError, TruncatedFeature, apply_replace};
use crate::enzymes::{Enzyme, OverhangKind, find_cut_sites, get_enzyme};
use crate::model::{Feature, Segment, Strand};
use crate::seq::{clean_seq, revcomp};

// ---------------------------------------------------------------------------
// Errors.
// ---------------------------------------------------------------------------

/// A design that cannot be attempted at all. Anything that CAN be reported alongside a design is a
/// warning instead, because somebody about to order DNA needs every problem at once, not the first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenGateError {
    UnknownEnzyme(String),
    /// In the table, but not as a type IIS cutter: it cuts inside its own site, which leaves the
    /// site on both pieces.
    NotTypeIIS(&'static str),
    /// The table records recognition only — no cut positions, so no overhang can be worked out.
    CleavageUnverified(&'static str),
    /// A two-sided cutter (two cleavage pairs per site) does not release one piece with one
    /// overhang.
    TwoSidedCutter(&'static str),
    /// The site holds IUPAC codes, so there is no one sequence to put on a part's tail.
    DegenerateSite(&'static str),
    /// The cut falls at or before the site, so a tail could not carry it.
    CutsInsideSite(&'static str),
    /// A blunt cut or a 3' overhang: the piece model above does not hold.
    NotAFivePrimeOverhang(&'static str),
    /// Two cuts on one molecule leaving overhangs of different length or polarity.
    MixedOverhangs {
        enzyme: &'static str,
        position: i64,
    },
    VectorLinear,
    VectorEmpty,
    NoParts,
    /// A part shorter than its two overhangs plus one base of body.
    PartTooShort {
        name: String,
        length: i64,
        least: i64,
    },
    /// Fewer than two cuts: nothing to open the vector between.
    VectorNotCutTwice {
        enzyme: &'static str,
        cuts: Vec<i64>,
    },
    /// No arc between two cuts is free of the enzyme's sites, so every backbone this vector could
    /// give would be cut apart again.
    NoCleanBackbone {
        enzyme: &'static str,
        cuts: Vec<i64>,
        sites: Vec<i64>,
    },
    /// More than one arc qualifies, so which piece is the backbone would be a guess.
    AmbiguousBackbone {
        enzyme: &'static str,
        arcs: Vec<(i64, i64)>,
    },
    Edit(EditError),
}

impl std::fmt::Display for GoldenGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pick = "Use a type IIS enzyme that leaves a 5' overhang, such as BsaI, BsmBI, BbsI or SapI.";
        match self {
            GoldenGateError::UnknownEnzyme(name) => write!(f, "{name} is not an enzyme Bios knows."),
            GoldenGateError::NotTypeIIS(name) => {
                write!(f, "{name} cuts inside its own recognition site, so it cannot hold a Golden Gate assembly together. {pick}")
            }
            GoldenGateError::CleavageUnverified(name) => {
                write!(f, "Bios knows where {name} binds but not where it cuts, so it cannot work out the overhangs. {pick}")
            }
            GoldenGateError::TwoSidedCutter(name) => {
                write!(f, "{name} cuts on both sides of its recognition site, which does not release a piece with one overhang at each end. {pick}")
            }
            GoldenGateError::DegenerateSite(name) => {
                write!(f, "{name}'s recognition site holds IUPAC codes, so there is no one sequence to put on a part's tail. {pick}")
            }
            GoldenGateError::CutsInsideSite(name) => {
                write!(f, "{name} cuts at or before its own recognition site, so a tail on a part would not release the part. {pick}")
            }
            GoldenGateError::NotAFivePrimeOverhang(name) => {
                write!(f, "{name} leaves no 5' overhang, and Golden Gate needs one. {pick}")
            }
            GoldenGateError::MixedOverhangs { enzyme, position } => {
                write!(f, "The {enzyme} cut at {position} leaves an overhang unlike the others on this plasmid, so the pieces cannot be matched up.")
            }
            GoldenGateError::VectorLinear => write!(f, "The destination vector must be circular: a Golden Gate assembly closes into a circle."),
            GoldenGateError::VectorEmpty => write!(f, "The destination vector holds no bases."),
            GoldenGateError::NoParts => write!(f, "Give at least one part, as [{{\"name\": ..., \"sequence\": ...}}]."),
            GoldenGateError::PartTooShort { name, length, least } => write!(
                f,
                "Part \"{name}\" is {length} bases, and a part has to be at least {least}: its first and last bases ARE its two overhangs, so there has to be some sequence between them."
            ),
            GoldenGateError::VectorNotCutTwice { enzyme, cuts } => match cuts.as_slice() {
                [] => write!(f, "{enzyme} does not cut this plasmid, so it cannot open it. A destination vector carries two {enzyme} sites."),
                [one] => write!(
                    f,
                    "{enzyme} cuts this plasmid once, at {one}. A destination vector carries two {enzyme} sites, pointing outward so that both leave with the stuffer between them."
                ),
                _ => write!(f, "{enzyme} cuts this plasmid at {}, which does not open it.", list(cuts)),
            },
            GoldenGateError::NoCleanBackbone { enzyme, cuts, sites } => write!(
                f,
                "{enzyme} cuts this plasmid at {} and has sites at {}, and no piece of that digest is free of the sites, so whatever backbone it gave would be cut apart again. A destination vector's two sites point outward from the stuffer, so both leave with it.",
                list(cuts),
                list(sites)
            ),
            GoldenGateError::AmbiguousBackbone { enzyme, arcs } => write!(
                f,
                "More than one piece of the {enzyme} digest is free of {enzyme} sites ({}), so which one is the backbone would be a guess. Open this vector by hand instead.",
                arcs.iter().map(|(from, to)| format!("{from}..{to}")).collect::<Vec<_>>().join(", ")
            ),
            GoldenGateError::Edit(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for GoldenGateError {}

impl From<EditError> for GoldenGateError {
    fn from(e: EditError) -> Self {
        GoldenGateError::Edit(e)
    }
}

fn list(xs: &[i64]) -> String {
    xs.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------
// The enzyme's geometry, read off the table.
// ---------------------------------------------------------------------------

/// What one type IIS enzyme does, as the numbers an assembly needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub enzyme: &'static str,
    /// The recognition site, 5'→3' on the strand the enzyme reads.
    pub site: &'static str,
    /// Bases between the site and the top-strand cut: the `1` of GGTCTC(1/5).
    pub spacer: i64,
    /// Bases of 5' overhang the cut leaves: the `5 - 1` of GGTCTC(1/5).
    pub overhang_len: i64,
    /// What a part needs on its 5' end for the enzyme to release it: the site, then the spacer.
    pub five_prime_tail: String,
    /// What it needs on its 3' end: the spacer, then the site read on the other strand.
    pub three_prime_tail: String,
}

impl Geometry {
    /// The cut as a catalogue writes it, e.g. `GGTCTC(1/5)`.
    pub fn notation(&self) -> String {
        format!("{}({}/{})", self.site, self.spacer, self.spacer + self.overhang_len)
    }
}

/// The base a tail puts between the site and the cut. Nothing reads it, so the only requirement is
/// that it does not spell another site; [`warn_about_sites`] checks the tail that was built.
const SPACER_FILLER: char = 'A';

/// One type IIS enzyme's cut geometry, worked out from the enzyme table alone.
pub fn geometry(name: &str) -> Result<Geometry, GoldenGateError> {
    let enz = get_enzyme(name).ok_or_else(|| GoldenGateError::UnknownEnzyme(name.to_string()))?;
    if !enz.type2s {
        return Err(GoldenGateError::NotTypeIIS(enz.name));
    }
    let (Some(top), Some(bottom)) = (enz.top_snip_offset(), enz.bottom_snip_offset()) else {
        return Err(GoldenGateError::CleavageUnverified(enz.name));
    };
    if !enz.additional_cuts().is_empty() {
        return Err(GoldenGateError::TwoSidedCutter(enz.name));
    }
    if !enz.site.bytes().all(|b| matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T')) {
        return Err(GoldenGateError::DegenerateSite(enz.name));
    }
    let length = i64::try_from(enz.length).unwrap_or(i64::MAX);
    let (spacer, overhang_len) = (top - length, bottom - top);
    if spacer < 0 {
        return Err(GoldenGateError::CutsInsideSite(enz.name));
    }
    if overhang_len <= 0 {
        return Err(GoldenGateError::NotAFivePrimeOverhang(enz.name));
    }
    let filler: String = std::iter::repeat_n(SPACER_FILLER, usize::try_from(spacer).unwrap_or(0)).collect();
    Ok(Geometry {
        enzyme: enz.name,
        site: enz.site,
        spacer,
        overhang_len,
        five_prime_tail: format!("{}{filler}", enz.site),
        three_prime_tail: format!("{filler}{}", revcomp(enz.site)),
    })
}

// ---------------------------------------------------------------------------
// Finding the enzyme's sites and cuts.
// ---------------------------------------------------------------------------

/// One double-strand cut: the gap it falls in, and the plus-strand bases of the overhang it leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    /// 0-based gap coordinate, as [`crate::enzymes`] reports it.
    pub at: i64,
    pub overhang: String,
}

/// One enzyme's cuts and recognition starts on one molecule, sorted and without duplicates.
pub struct EnzymeDigest {
    pub cuts: Vec<Cut>,
    pub sites: Vec<i64>,
}

/// Where the enzyme cuts a molecule, refusing a cut whose overhang is not the one the geometry
/// promised — the pieces could not be matched up if the ends were not all alike.
///
/// [`crate::digest`] also cuts molecules into pieces, and holds a piece as the top strand from one
/// scission to the next, so a cut's overhang bases belong to exactly one of the two pieces and the
/// pieces of a digest add back up to the molecule. This module needs the other convention — the
/// overhang at BOTH ends of every piece, because that is how a part is written on paper and handed
/// in (see the module docs) — so it works from the cuts rather than from those pieces.
pub fn cuts_on(seq: &str, circular: bool, g: &Geometry) -> Result<EnzymeDigest, GoldenGateError> {
    let mut cuts: Vec<Cut> = Vec::new();
    let mut sites: Vec<i64> = Vec::new();
    for site in find_cut_sites(seq, &[g.enzyme], circular) {
        let at = i64::try_from(site.top_cut).unwrap_or(i64::MAX);
        if site.overhang.kind != OverhangKind::FivePrime || i64::try_from(site.overhang.length).unwrap_or(0) != g.overhang_len {
            return Err(GoldenGateError::MixedOverhangs { enzyme: g.enzyme, position: at + 1 });
        }
        if !cuts.iter().any(|c| c.at == at) {
            cuts.push(Cut { at, overhang: site.overhang.seq.to_ascii_uppercase() });
        }
        let start = i64::try_from(site.recognition_start).unwrap_or(i64::MAX);
        if !sites.contains(&start) {
            sites.push(start);
        }
    }
    cuts.sort_by_key(|c| c.at);
    sites.sort_unstable();
    Ok(EnzymeDigest { cuts, sites })
}

/// How far past a site the padding below has to reach. No entry in the table cuts further from its
/// site than this, and a short pad would hide exactly the site an assembly has to warn about.
const PAD: usize = 64;

/// Every recognition site for `enzyme` in `seq`, as 0-based starts.
///
/// [`find_cut_sites`] reports a site only when both of its scissions land on the molecule, and a
/// type IIS site near the end of a LINEAR piece cuts past it — exactly the site an assembly has to
/// warn about. So a linear sequence is padded on both sides before searching, and only matches
/// lying wholly inside the original are kept: a match reaching into the padding is the padding's,
/// not the piece's. On a circular molecule every cut lands, so no padding is needed.
pub fn sites_within(seq: &str, enzyme: &Enzyme, circular: bool) -> Vec<i64> {
    let reach = if circular { 0 } else { enzyme.length + PAD };
    let pad: String = std::iter::repeat_n(SPACER_FILLER, reach).collect();
    let padded = if circular { seq.to_string() } else { format!("{pad}{seq}{pad}") };
    let (lo, hi) = (reach, reach + seq.len());
    let mut out: Vec<i64> = Vec::new();
    for site in find_cut_sites(&padded, &[enzyme.name], circular) {
        if !circular && (site.recognition_start < lo || site.recognition_start + enzyme.length > hi) {
            continue;
        }
        if let Ok(at) = i64::try_from(site.recognition_start.saturating_sub(lo))
            && !out.contains(&at)
        {
            out.push(at);
        }
    }
    out.sort_unstable();
    out
}

/// Whether `x` lies in the half-open arc running forward from `from` to `to` on a circle of `n`.
/// `from == to` is a full turn, so everything lies in it.
fn in_arc(x: i64, from: i64, to: i64, n: i64) -> bool {
    if n <= 0 {
        return false;
    }
    let (x, from, to) = (x.rem_euclid(n), from.rem_euclid(n), to.rem_euclid(n));
    if from == to {
        return true;
    }
    if from < to { from <= x && x < to } else { x >= from || x < to }
}

/// Which cut starts the stuffer and which starts the backbone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arc {
    /// The cut the backbone's 3' overhang comes from; the stuffer runs forward from here.
    stuffer_start: i64,
    /// The cut its 5' overhang comes from; the backbone runs forward from here.
    backbone_start: i64,
}

/// The one arc of the digest holding no recognition site and no further cut. That arc is the
/// backbone: it is the only piece the enzyme cannot touch again.
fn choose_arc(n: i64, d: &EnzymeDigest, g: &Geometry) -> Result<Arc, GoldenGateError> {
    let plus_one = |xs: &[i64]| xs.iter().map(|x| x + 1).collect::<Vec<_>>();
    let gaps: Vec<i64> = d.cuts.iter().map(|c| c.at).collect();
    if d.cuts.len() < 2 {
        return Err(GoldenGateError::VectorNotCutTwice { enzyme: g.enzyme, cuts: plus_one(&gaps) });
    }
    let clean = |arc: &Arc| {
        let (a, b) = (arc.stuffer_start, arc.backbone_start);
        a != b
            && (a - b).rem_euclid(n) > g.overhang_len
            && !d.sites.iter().any(|&s| in_arc(s, b, a, n))
            && !gaps.iter().any(|&c| c != a && c != b && in_arc(c, b, a, n))
    };
    let found: Vec<Arc> = gaps.iter().flat_map(|&a| gaps.iter().map(move |&b| Arc { stuffer_start: a, backbone_start: b })).filter(clean).collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(GoldenGateError::NoCleanBackbone { enzyme: g.enzyme, cuts: plus_one(&gaps), sites: plus_one(&d.sites) }),
        many => Err(GoldenGateError::AmbiguousBackbone {
            enzyme: g.enzyme,
            arcs: many.iter().map(|arc| (arc.backbone_start + 1, arc.stuffer_start + 1)).collect(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Input.
// ---------------------------------------------------------------------------

/// One part, written from its first fusion base to its last: the first [`Geometry::overhang_len`]
/// bases are its 5' overhang and the last are its 3' overhang. No type IIS tails —
/// [`design_golden_gate`] works out which tails it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub seq: String,
    /// Annotations on the part, 0-based in the part's own frame, carried onto the product.
    pub features: Vec<Feature>,
}

impl Part {
    pub fn new(name: impl Into<String>, seq: impl Into<String>) -> Self {
        Part { name: name.into(), seq: seq.into(), features: Vec::new() }
    }
}

/// A destination vector, an enzyme, and the parts to put in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenGateInput {
    pub vector_name: String,
    pub vector_seq: String,
    pub vector_circular: bool,
    pub vector_features: Vec<Feature>,
    /// By name or alias: "Esp3I" and "BsmBI" are one entry in the table.
    pub enzyme: String,
    pub parts: Vec<Part>,
}

impl GoldenGateInput {
    pub fn new(vector_name: impl Into<String>, vector_seq: impl Into<String>, enzyme: impl Into<String>, parts: Vec<Part>) -> Self {
        GoldenGateInput {
            vector_name: vector_name.into(),
            vector_seq: vector_seq.into(),
            vector_circular: true,
            vector_features: Vec::new(),
            enzyme: enzyme.into(),
            parts,
        }
    }
}

// ---------------------------------------------------------------------------
// Output.
// ---------------------------------------------------------------------------

/// The backbone the digest leaves, in the piece model of the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backbone {
    pub name: String,
    /// 1-based inclusive stretch of the vector that leaves with the stuffer.
    pub removed: (i64, i64),
    pub removed_len: i64,
    /// Bases of vector kept, both overhangs included.
    pub length: i64,
    /// True when the stretch that leaves runs through the vector's origin.
    pub through_origin: bool,
    /// The overhang at the backbone's 5' end: the last part joins onto it.
    pub left_overhang: String,
    /// The overhang at its 3' end: the first part joins onto it.
    pub right_overhang: String,
}

/// Where two pieces meet on the product.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Junction {
    pub left: String,
    pub right: String,
    pub overhang: String,
    /// 1-based inclusive position of the overhang on the product, when there is a product.
    pub position: Option<(i64, i64)>,
    /// Reads the same on both strands, so the piece after it can also go in backwards.
    pub palindromic: bool,
    /// No other junction of this assembly uses the same overhang.
    pub unique: bool,
}

/// One part as it goes onto an order form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderPiece {
    pub name: String,
    pub five_prime_tail: String,
    pub three_prime_tail: String,
    /// The part as given, without tails.
    pub part: String,
    /// Tails and part together: what to paste into the order form.
    pub order: String,
    pub left_overhang: String,
    pub right_overhang: String,
}

/// Where a part sits on the product, 0-based inclusive, overhangs included — so neighbours share
/// [`Geometry::overhang_len`] bases, as they really do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// What the vector's own annotations became.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    pub kept: Vec<Feature>,
    pub truncated: Vec<TruncatedFeature>,
    pub removed: Vec<Feature>,
}

/// The assembled plasmid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    pub sequence: String,
    pub circular: bool,
    /// Everything on the product: the vector's surviving features, one marker per part, and each
    /// part's own annotations.
    pub features: Vec<Feature>,
    pub carried: Carried,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenGateDesign {
    pub geometry: Geometry,
    pub backbone: Backbone,
    /// Every piece in assembly order, starting with the backbone. Short of the full set when the
    /// pieces do not close into a circle.
    pub order: Vec<String>,
    pub junctions: Vec<Junction>,
    pub pieces: Vec<OrderPiece>,
    /// `None` when the overhangs do not form one circle; the warnings then say where it breaks.
    pub product: Option<Product>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// The design.
// ---------------------------------------------------------------------------

/// Work out a Golden Gate assembly: the overhangs, the order they force, the product, the pieces to
/// order, and everything about the set that would make it misbehave at the bench.
pub fn design_golden_gate(input: &GoldenGateInput) -> Result<GoldenGateDesign, GoldenGateError> {
    let g = geometry(&input.enzyme)?;
    let k = g.overhang_len;
    let vector = clean_seq(&input.vector_seq).to_ascii_uppercase();
    let n = i64::try_from(vector.len()).unwrap_or(i64::MAX);
    if n == 0 {
        return Err(GoldenGateError::VectorEmpty);
    }
    if !input.vector_circular {
        return Err(GoldenGateError::VectorLinear);
    }
    if input.parts.is_empty() {
        return Err(GoldenGateError::NoParts);
    }
    let parts = cleaned_parts(input, k)?;
    let d = cuts_on(&vector, true, &g)?;
    let arc = choose_arc(n, &d, &g)?;
    let backbone = backbone_of(&input.vector_name, n, &d, arc, k);

    let pieces = Pieces::new(&backbone, &parts, k);
    let mut warnings = Vec::new();
    let chain = chain(&pieces, &mut warnings);
    let built = chain.closed.then(|| assemble(input, &vector, n, arc, &parts, &chain.order, k)).transpose()?;
    let junctions = junctions_of(&pieces, &chain, built.as_ref().map(|b| b.frame), k);

    let to_order: Vec<OrderPiece> = parts.iter().map(|p| order_piece(p, &g)).collect();
    let product = built.map(|b| b.product);
    warn_about_sites(&mut warnings, &parts, &to_order, &g, product.as_ref());
    warn_about_overhangs(&mut warnings, &junctions);
    Ok(GoldenGateDesign {
        geometry: g,
        backbone,
        order: chain.order.iter().filter_map(|&i| pieces.names.get(i).cloned()).collect(),
        junctions,
        pieces: to_order,
        product,
        warnings,
    })
}

/// The parts with their sequences cleaned, refusing any too short to carry two overhangs and a
/// body.
fn cleaned_parts(input: &GoldenGateInput, k: i64) -> Result<Vec<Part>, GoldenGateError> {
    let least = 2 * k + 1;
    input
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let seq = clean_seq(&p.seq).to_ascii_uppercase();
            let name = if p.name.trim().is_empty() { format!("Part {}", i + 1) } else { p.name.trim().to_string() };
            let length = i64::try_from(seq.len()).unwrap_or(i64::MAX);
            if length < least {
                return Err(GoldenGateError::PartTooShort { name, length, least });
            }
            Ok(Part { name, seq, features: p.features.clone() })
        })
        .collect()
}

/// A piece's 5' or 3' overhang: its first or last `k` bases.
fn overhang(seq: &str, k: i64, five_prime: bool) -> String {
    let k = usize::try_from(k).unwrap_or(0);
    let at = if five_prime { 0 } else { seq.len().saturating_sub(k) };
    seq.get(at..at.saturating_add(k)).unwrap_or_default().to_string()
}

fn backbone_of(vector_name: &str, n: i64, d: &EnzymeDigest, arc: Arc, k: i64) -> Backbone {
    let overhang_at = |gap: i64| d.cuts.iter().find(|c| c.at == gap).map_or_else(String::new, |c| c.overhang.clone());
    let removed_len = (arc.backbone_start - arc.stuffer_start).rem_euclid(n);
    // A backbone starting at the origin means the stuffer reaches the vector's last base.
    let to = if arc.backbone_start == 0 { n } else { arc.backbone_start };
    let from = arc.stuffer_start + 1;
    Backbone {
        name: format!("{vector_name} backbone"),
        removed: (from, to),
        removed_len,
        length: n - removed_len + k,
        through_origin: from > to,
        left_overhang: overhang_at(arc.backbone_start),
        right_overhang: overhang_at(arc.stuffer_start),
    }
}

/// Every piece of the assembly by name and by the two overhangs it carries. Index 0 is the
/// backbone; index `i` is `parts[i - 1]`.
struct Pieces {
    names: Vec<String>,
    lefts: Vec<String>,
    rights: Vec<String>,
    /// Bases each piece contributes to the product, less the overhang it shares with the next.
    lengths: Vec<i64>,
}

impl Pieces {
    fn new(backbone: &Backbone, parts: &[Part], k: i64) -> Self {
        let mut p = Pieces {
            names: vec![backbone.name.clone()],
            lefts: vec![backbone.left_overhang.clone()],
            rights: vec![backbone.right_overhang.clone()],
            lengths: vec![backbone.length],
        };
        for part in parts {
            p.names.push(part.name.clone());
            p.lengths.push(i64::try_from(part.seq.len()).unwrap_or(0));
            p.lefts.push(overhang(&part.seq, k, true));
            p.rights.push(overhang(&part.seq, k, false));
        }
        p
    }

    fn name(&self, i: usize) -> &str {
        self.names.get(i).map_or("", String::as_str)
    }

    fn left(&self, i: usize) -> &str {
        self.lefts.get(i).map_or("", String::as_str)
    }

    fn right(&self, i: usize) -> &str {
        self.rights.get(i).map_or("", String::as_str)
    }
}

/// The order the overhangs force, as indices into [`Pieces`].
struct Chain {
    /// Starting with 0, the backbone. Stops where the overhangs stop matching.
    order: Vec<usize>,
    closed: bool,
}

/// Follow the overhangs from the backbone round to the backbone again. Each step looks for the one
/// unused piece whose 5' overhang is the current piece's 3' overhang; nothing else anneals there.
fn chain(pieces: &Pieces, warnings: &mut Vec<String>) -> Chain {
    let mut order = vec![0usize];
    let mut used = vec![false; pieces.names.len()];
    let close = pieces.left(0).to_string();
    let mut cur = 0usize;
    loop {
        if let Some(seen) = used.get_mut(cur) {
            *seen = true;
        }
        let want = pieces.right(cur).to_string();
        let open: Vec<usize> = (1..pieces.names.len()).filter(|&i| unused(&used, i) && pieces.left(i) == want).collect();
        if want == close {
            for &i in &open {
                warnings.push(format!(
                    "Overhang {want} both closes the circle onto \"{}\" and opens \"{}\", so the order is ambiguous. Give those two pieces different overhangs.",
                    pieces.name(0),
                    pieces.name(i)
                ));
            }
            warn_unused(warnings, pieces, &used);
            return Chain { order, closed: true };
        }
        match open.as_slice() {
            [] => {
                warnings.push(dead_end(pieces, &used, cur, &want, &close));
                return Chain { order, closed: false };
            }
            [one] => {
                order.push(*one);
                cur = *one;
            }
            many => {
                warnings.push(format!(
                    "Overhang {want} after \"{}\" fits {}, so the order is ambiguous. Bios takes the first of them; give them different overhangs.",
                    pieces.name(cur),
                    many.iter().map(|&i| format!("\"{}\"", pieces.name(i))).collect::<Vec<_>>().join(" and ")
                ));
                let next = many.first().copied().unwrap_or(0);
                order.push(next);
                cur = next;
            }
        }
    }
}

/// Whether piece `i` has not been placed in the chain yet.
fn unused(used: &[bool], i: usize) -> bool {
    used.get(i) != Some(&true)
}

fn dead_end(pieces: &Pieces, used: &[bool], cur: usize, want: &str, close: &str) -> String {
    let backwards: Vec<String> =
        (1..pieces.names.len()).filter(|&i| unused(used, i) && revcomp(pieces.right(i)) == want).map(|i| format!("\"{}\"", pieces.name(i))).collect();
    let hint = if backwards.is_empty() {
        String::new()
    } else {
        format!(" {} would match if it went in backwards; Bios assembles every part in the orientation it was given.", backwards.join(" and "))
    };
    format!(
        "No single circular product: the 3' overhang {want} of \"{}\" matches no part's 5' overhang, and it is not the backbone's {close} either, so the assembly stops there.{hint}",
        pieces.name(cur)
    )
}

fn warn_unused(warnings: &mut Vec<String>, pieces: &Pieces, used: &[bool]) {
    for i in (1..pieces.names.len()).filter(|&i| unused(used, i)) {
        warnings.push(format!(
            "No single circular product: \"{}\" ({}…{}) is left over — nothing in the closed circle ends in {}.",
            pieces.name(i),
            pieces.left(i),
            pieces.right(i),
            pieces.left(i)
        ));
    }
}

/// Each junction of the chain, with where its overhang lands on the product.
///
/// A junction's overhang is the 3' overhang of the piece before it. The first junction sits where
/// the stuffer was; each one after it is one piece's contribution further on, because every piece
/// gives the product its own length less the overhang it shares with the next.
fn junctions_of(pieces: &Pieces, chain: &Chain, frame: Option<Frame>, k: i64) -> Vec<Junction> {
    let mut out: Vec<Junction> = Vec::new();
    let mut at = frame.map(|f| f.ins_start);
    for (step, &i) in chain.order.iter().enumerate() {
        let next = match chain.order.get(step + 1) {
            Some(&next) => next,
            None if chain.closed => 0,
            None => break,
        };
        let seq = pieces.right(i).to_string();
        let position = frame.zip(at).map(|(f, a)| (a.rem_euclid(f.total) + 1, (a + k - 1).rem_euclid(f.total) + 1));
        out.push(Junction {
            left: pieces.name(i).to_string(),
            right: pieces.name(next).to_string(),
            palindromic: !seq.is_empty() && revcomp(&seq) == seq,
            unique: true,
            position,
            overhang: seq,
        });
        at = at.map(|a| a + pieces.lengths.get(next).copied().unwrap_or(0) - k);
    }
    let counts = out.iter().fold(HashMap::<String, usize>::new(), |mut acc, j| {
        *acc.entry(j.overhang.clone()).or_default() += 1;
        acc
    });
    for j in out.iter_mut() {
        j.unique = counts.get(&j.overhang) == Some(&1);
    }
    out
}

/// What the ordered piece for one part looks like.
fn order_piece(part: &Part, g: &Geometry) -> OrderPiece {
    OrderPiece {
        name: part.name.clone(),
        five_prime_tail: g.five_prime_tail.clone(),
        three_prime_tail: g.three_prime_tail.clone(),
        left_overhang: overhang(&part.seq, g.overhang_len, true),
        right_overhang: overhang(&part.seq, g.overhang_len, false),
        order: format!("{}{}{}", g.five_prime_tail, part.seq, g.three_prime_tail),
        part: part.seq.clone(),
    }
}

/// Where the inserted stretch starts on the product, and how long the product is.
#[derive(Clone, Copy)]
struct Frame {
    ins_start: i64,
    total: i64,
}

struct Built {
    product: Product,
    frame: Frame,
}

/// Build the product: the vector with its stuffer replaced by the parts' contributions.
fn assemble(input: &GoldenGateInput, vector: &str, n: i64, arc: Arc, parts: &[Part], order: &[usize], k: i64) -> Result<Built, GoldenGateError> {
    let ordered: Vec<&Part> = order.iter().skip(1).filter_map(|&i| parts.get(i.saturating_sub(1))).collect();
    // Each piece gives its own bases less the overhang it shares with the piece after it, so every
    // overhang reaches the product exactly once.
    let replacement: String = ordered.iter().map(|p| contribution(&p.seq, k)).collect();
    let to = if arc.backbone_start == 0 { n } else { arc.backbone_start };
    let from = arc.stuffer_start + 1;
    let r = apply_replace(vector, &input.vector_features, from, to, &replacement, true)?;
    let total = i64::try_from(r.sequence.len()).unwrap_or(i64::MAX);
    // A replacement that wraps the origin starts the product; otherwise it stays where it was.
    let frame = Frame { ins_start: if from > to { 0 } else { arc.stuffer_start }, total };
    let mut features = r.features.clone();
    let mut placements = Vec::new();
    let mut at = frame.ins_start;
    for p in &ordered {
        let length = i64::try_from(p.seq.len()).unwrap_or(0);
        add_span(&mut features, total, at, length, &p.name, "misc_feature");
        for f in &p.features {
            add_shifted(&mut features, total, at, f);
        }
        placements.push(Placement {
            name: p.name.clone(),
            start: usize::try_from(at.rem_euclid(total)).unwrap_or(0),
            end: usize::try_from((at + length - 1).rem_euclid(total)).unwrap_or(0),
        });
        at += length - k;
    }
    let product = Product {
        sequence: r.sequence,
        circular: true,
        features,
        carried: Carried { kept: r.features, truncated: r.delta.truncated, removed: r.delta.removed },
        placements,
    };
    Ok(Built { product, frame })
}

/// A piece's own bases less the overhang it shares with the piece after it.
fn contribution(seq: &str, k: i64) -> &str {
    let keep = seq.len().saturating_sub(usize::try_from(k).unwrap_or(0));
    seq.get(..keep).unwrap_or_default()
}

/// A marker over `len` bases from the half-open `start`, wrapped onto the product.
fn add_span(features: &mut Vec<Feature>, total: i64, start: i64, len: i64, name: &str, kind: &str) {
    if len <= 0 || total <= 0 {
        return;
    }
    let (s, e) = (start.rem_euclid(total), (start + len - 1).rem_euclid(total));
    let (s, e) = (usize::try_from(s).unwrap_or(0), usize::try_from(e).unwrap_or(0));
    features.push(Feature {
        name: name.to_string(),
        kind: kind.to_string(),
        start: s,
        end: e,
        strand: Strand::Forward,
        segments: vec![Segment::new(s, e)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    });
}

/// One of a part's own annotations, moved onto the product.
fn add_shifted(features: &mut Vec<Feature>, total: i64, at: i64, f: &Feature) {
    if total <= 0 {
        return;
    }
    let shift = |x: usize| usize::try_from((at + i64::try_from(x).unwrap_or(0)).rem_euclid(total)).unwrap_or(0);
    let (s, e) = (shift(f.start), shift(f.end));
    let mut moved = f.clone();
    moved.start = s;
    moved.end = e;
    moved.segments = vec![Segment::new(s, e)];
    features.push(moved);
}

/// A site for the enzyme left on a part, on a tail, or on the product is the assembly being cut
/// apart again, so each one is named with where it is.
fn warn_about_sites(warnings: &mut Vec<String>, parts: &[Part], to_order: &[OrderPiece], g: &Geometry, product: Option<&Product>) {
    let Some(enz) = get_enzyme(g.enzyme) else { return };
    let length = i64::try_from(enz.length).unwrap_or(0);
    for p in parts {
        for at in sites_within(&p.seq, enz, false) {
            warnings.push(format!(
                "Part \"{}\" carries a {} site of its own at {}..{}, so the enzyme would cut the part apart. Take the site out (a silent codon change, if it is coding) before ordering.",
                p.name,
                g.enzyme,
                at + 1,
                at + length
            ));
        }
    }
    for piece in to_order {
        // The tails put one site at each end; anything beyond that is the spacer or the part's own
        // ends having spelled a new one.
        let extra = sites_within(&piece.order, enz, false).len().saturating_sub(2);
        if extra > 0 {
            warnings.push(format!(
                "The piece to order for \"{}\" holds {extra} {} site(s) beyond its two tails, so the tails as written would not release it cleanly.",
                piece.name, g.enzyme
            ));
        }
    }
    if let Some(product) = product {
        let sites = sites_within(&product.sequence, enz, product.circular);
        if !sites.is_empty() {
            warnings.push(format!(
                "The assembled product still carries {} {} site(s), at {}, so the reaction would keep cutting it open. A Golden Gate product has to be free of the enzyme's sites.",
                sites.len(),
                g.enzyme,
                list(&sites.iter().map(|x| x + 1).collect::<Vec<_>>())
            ));
        }
    }
}

/// The two ways a set of overhangs misbehaves even when it does form a circle.
fn warn_about_overhangs(warnings: &mut Vec<String>, junctions: &[Junction]) {
    let at = |j: &Junction| j.position.map_or_else(String::new, |(s, e)| format!(" at {s}..{e}"));
    for j in junctions.iter().filter(|j| j.palindromic) {
        warnings.push(format!(
            "The {} junction between \"{}\" and \"{}\"{} reads the same on both strands, so those two ends can also anneal the other way round and a part can go in backwards. Choose an overhang that is not its own reverse complement.",
            j.overhang,
            j.left,
            j.right,
            at(j)
        ));
    }
    let mut repeated: Vec<&str> = junctions.iter().filter(|j| !j.unique).map(|j| j.overhang.as_str()).collect();
    repeated.sort_unstable();
    repeated.dedup();
    for oh in repeated {
        let shared: Vec<String> = junctions.iter().filter(|j| j.overhang == oh).map(|j| format!("\"{}\"→\"{}\"{}", j.left, j.right, at(j))).collect();
        warnings.push(format!(
            "Overhang {oh} is used by {} junctions ({}), so those pieces can swap places. Every junction of an assembly needs an overhang of its own.",
            shared.len(),
            shared.join(", ")
        ));
    }
}
