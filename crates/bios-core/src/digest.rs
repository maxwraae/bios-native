//! Restriction digests and restriction cloning: the fragments a digest leaves, and what ligating
//! one of them into an opened vector gives.
//!
//! [`crate::enzymes`] says where an enzyme cuts; this module turns those cuts into the pieces that
//! come off the molecule, and joins pieces back together.
//!
//! Coordinates are 0-based. A cut is a GAP coordinate, as in [`crate::enzymes`]: `g` is the bond
//! between plus-strand bases `g-1` and `g`.
//!
//! A fragment is held as its TOP strand: the plus-strand bases from one top-strand scission to the
//! next, which is what a gel is read against and what every tool quotes as a fragment size. The
//! bases of an overhang belong to exactly one of the two fragments the cut makes — the one whose
//! top strand carries them — so the fragments of a digest always add back up to the molecule.
//!
//! An end is described by the plus-strand bases between the two scissions ([`Overhang::seq`]), the
//! same string for both fragments of the cut: one carries it on its top strand, the other on its
//! bottom. Two ends therefore ligate when their kind and those bases agree, which is also why
//! BamHI (G^GATCC) and BglII (A^GATCT) ligate to each other and neither ligates to a blunt end.

use std::collections::BTreeMap;

use crate::enzymes::{CleavageStatus, CutSite, Overhang, OverhangKind, find_cut_sites, get_enzyme};
use crate::model::{Feature, Strand};
use crate::seq::{clean_seq, revcomp};

/// A digest that cannot be worked out. Every message is read by someone about to run a gel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DigestError {
    /// A name that is in no catalog Bios has.
    UnknownEnzyme(String),
    /// A catalog entry with no verified cleavage positions. Recognition alone is never a cut.
    CleavageNotKnown(String),
    NoEnzymes,
    EmptySequence,
}

impl std::fmt::Display for DigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DigestError::UnknownEnzyme(name) => write!(f, "{name} is not an enzyme Bios knows"),
            DigestError::CleavageNotKnown(name) => {
                write!(f, "Bios has no verified cut positions for {name}, so it will not guess where it cuts")
            }
            DigestError::NoEnzymes => write!(f, "name at least one enzyme to cut with"),
            DigestError::EmptySequence => write!(f, "there are no bases to cut"),
        }
    }
}

impl std::error::Error for DigestError {}

/// One end of a fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentEnd {
    /// The enzyme that made it. `None` for an end the molecule already had, on a linear molecule.
    pub enzyme: Option<&'static str>,
    /// What the cut left. `None` for an end the molecule already had: Bios does not know what
    /// chemistry it has, so it cannot be ligated to anything here.
    pub overhang: Option<Overhang>,
    /// Gap coordinate of the top-strand scission on the molecule that was cut.
    pub top_cut: Option<usize>,
    /// Gap coordinate of the bottom-strand scission on the molecule that was cut.
    pub bottom_cut: Option<usize>,
}

impl FragmentEnd {
    /// An end of the molecule as it came, on a linear molecule.
    pub fn as_it_came() -> Self {
        FragmentEnd { enzyme: None, overhang: None, top_cut: None, bottom_cut: None }
    }

    /// A blunt end: no overhang and no enzyme, for a stretch taken out of a sequence.
    pub fn blunt() -> Self {
        FragmentEnd {
            enzyme: None,
            overhang: Some(Overhang { kind: OverhangKind::Blunt, length: 0, seq: String::new() }),
            top_cut: None,
            bottom_cut: None,
        }
    }

    /// The same end seen from the other side of the duplex: the bases read along what was the
    /// bottom strand. A 5' overhang stays a 5' overhang.
    fn flipped(&self) -> Self {
        FragmentEnd {
            enzyme: self.enzyme,
            overhang: self.overhang.as_ref().map(|o| Overhang { kind: o.kind, length: o.length, seq: revcomp(&o.seq) }),
            top_cut: self.top_cut,
            bottom_cut: self.bottom_cut,
        }
    }
}

/// One piece a digest leaves, held as its top strand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    /// 0-based first base of the top strand on the molecule that was cut.
    pub start: usize,
    /// 0-based last base. Before `start` when the fragment runs through the origin.
    pub end: usize,
    /// Top-strand bases, which is the size a gel is read against.
    pub length: usize,
    pub wraps_origin: bool,
    pub sequence: String,
    /// The end at the 5' end of the top strand.
    pub left: FragmentEnd,
    /// The end at the 3' end of the top strand.
    pub right: FragmentEnd,
}

/// What a digest leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestResult {
    /// Gel order: longest first, then by where they start.
    pub fragments: Vec<Fragment>,
    /// Bases in the molecule. The fragment lengths add up to it.
    pub total: usize,
    /// Double-strand cuts made, after cuts at one position have been counted once.
    pub cuts: Vec<Cut>,
    /// Enzymes asked for that do not cut this molecule, by name.
    pub non_cutters: Vec<&'static str>,
    /// True when nothing cut a circular molecule, so it is still circular and is not a fragment.
    pub uncut_circle: bool,
}

/// One double-strand cut, as the digest counts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub enzyme: &'static str,
    /// Gap coordinate of the top-strand scission.
    pub top_cut: usize,
    pub bottom_cut: usize,
    /// 0-based leftmost base of the recognition match, which for a Type IIS enzyme is not next to
    /// the cut.
    pub recognition_start: usize,
    pub strand: Strand,
    pub overhang: Overhang,
}

impl Cut {
    fn of(site: &CutSite) -> Self {
        Cut {
            enzyme: site.enzyme,
            top_cut: site.top_cut,
            bottom_cut: site.bottom_cut,
            recognition_start: site.recognition_start,
            strand: site.strand,
            overhang: site.overhang.clone(),
        }
    }

    fn left_of_fragment(&self) -> FragmentEnd {
        FragmentEnd {
            enzyme: Some(self.enzyme),
            overhang: Some(self.overhang.clone()),
            top_cut: Some(self.top_cut),
            bottom_cut: Some(self.bottom_cut),
        }
    }
}

/// The names the caller gave, resolved to the catalog's own spelling, refusing anything Bios has
/// no verified cut positions for.
fn resolved<S: AsRef<str>>(names: &[S]) -> Result<Vec<&'static str>, DigestError> {
    let mut out: Vec<&'static str> = Vec::new();
    for raw in names {
        let name = raw.as_ref().trim();
        let Some(enz) = get_enzyme(name) else {
            return Err(DigestError::UnknownEnzyme(name.to_string()));
        };
        if enz.cleavage_status() != CleavageStatus::Supported {
            return Err(DigestError::CleavageNotKnown(enz.name.to_string()));
        }
        if !out.contains(&enz.name) {
            out.push(enz.name);
        }
    }
    if out.is_empty() {
        return Err(DigestError::NoEnzymes);
    }
    Ok(out)
}

/// Read plus-strand bases from `start`, `len` of them, wrapping the origin when circular.
fn arc(seq: &str, start: usize, len: usize, circular: bool) -> String {
    let bytes = seq.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return String::new();
    }
    (0..len)
        .filter_map(|k| {
            let at = start + k;
            let at = if circular { at % n } else { at };
            bytes.get(at).map(|&b| char::from(b))
        })
        .collect()
}

/// Cut `sequence` with every named enzyme at once.
///
/// `circular` is required: a circular molecule digested as linear loses the cuts that straddle the
/// origin and reports one fragment too many. Two enzymes that cut at the same bond count as one
/// cut, so no fragment of length zero is ever reported.
pub fn digest<S: AsRef<str>>(sequence: &str, circular: bool, enzymes: &[S]) -> Result<DigestResult, DigestError> {
    let seq = clean_seq(sequence);
    let names = resolved(enzymes)?;
    if seq.is_empty() {
        return Err(DigestError::EmptySequence);
    }
    let total = seq.len();
    let sites = find_cut_sites(&seq, &names, circular);

    // One cut per bond. Enzymes are tried in the order they were asked for, so which of two
    // enzymes sharing a bond is named is the caller's own order, not chance.
    let mut by_bond: BTreeMap<usize, Cut> = BTreeMap::new();
    for site in &sites {
        by_bond.entry(site.top_cut).or_insert_with(|| Cut::of(site));
    }
    let cuts: Vec<Cut> = by_bond.into_values().collect();
    let cutters: Vec<&'static str> = sites.iter().map(|s| s.enzyme).collect();
    let non_cutters: Vec<&'static str> = names.iter().copied().filter(|n| !cutters.contains(n)).collect();

    let mut fragments = Vec::new();
    if circular {
        match cuts.as_slice() {
            [] => {
                return Ok(DigestResult { fragments, total, cuts, non_cutters, uncut_circle: true });
            }
            // One cut opens a circle into ONE linear fragment, not two.
            [only] => fragments.push(piece_from(&seq, total, circular, only.top_cut, total, only, only)),
            _ => {
                for (i, cut) in cuts.iter().enumerate() {
                    let next = cuts.get((i + 1) % cuts.len()).unwrap_or(cut);
                    let len = (next.top_cut + total - cut.top_cut) % total;
                    let len = if len == 0 { total } else { len };
                    fragments.push(piece_from(&seq, total, circular, cut.top_cut, len, cut, next));
                }
            }
        }
    } else {
        let mut at = 0usize;
        let mut left = FragmentEnd::as_it_came();
        for cut in &cuts {
            // A cut at the very start or end of a linear molecule takes nothing off it.
            if cut.top_cut > at {
                fragments.push(linear_piece(&seq, at, cut.top_cut - at, left, cut.left_of_fragment()));
            }
            at = cut.top_cut;
            left = cut.left_of_fragment();
        }
        if total > at {
            fragments.push(linear_piece(&seq, at, total - at, left, FragmentEnd::as_it_came()));
        }
    }
    fragments.sort_by(|a, b| b.length.cmp(&a.length).then(a.start.cmp(&b.start)));
    Ok(DigestResult { fragments, total, cuts, non_cutters, uncut_circle: false })
}

fn linear_piece(seq: &str, start: usize, len: usize, left: FragmentEnd, right: FragmentEnd) -> Fragment {
    Fragment { start, end: start + len - 1, length: len, wraps_origin: false, sequence: arc(seq, start, len, false), left, right }
}

fn piece_from(seq: &str, total: usize, circular: bool, start: usize, len: usize, left: &Cut, right: &Cut) -> Fragment {
    let end = (start + len - 1) % total.max(1);
    Fragment {
        start,
        end,
        length: len,
        wraps_origin: end < start,
        sequence: arc(seq, start, len, circular),
        left: left.left_of_fragment(),
        right: FragmentEnd {
            enzyme: Some(right.enzyme),
            overhang: Some(right.overhang.clone()),
            top_cut: Some(right.top_cut),
            bottom_cut: Some(right.bottom_cut),
        },
    }
}

/// A feature that came along whole, at its place on the fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarriedFeature {
    /// Its index in the molecule's feature list.
    pub index: usize,
    /// 0-based first base on the fragment.
    pub start: usize,
    /// 0-based last base on the fragment.
    pub end: usize,
}

/// Which features a fragment carries whole and which it cuts through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FragmentFeatures {
    pub inside: Vec<CarriedFeature>,
    /// Indexes of features the fragment holds part of.
    pub clipped: Vec<usize>,
}

/// Sort `features` into the ones that lie whole inside `fragment` and the ones it clips.
///
/// `total` is the length of the molecule that was cut. A feature with several pieces is left out
/// of both lists: moving one onto a fragment would have to guess what its pieces mean.
pub fn fragment_features(fragment: &Fragment, features: &[Feature], total: usize) -> FragmentFeatures {
    let mut out = FragmentFeatures::default();
    if total == 0 {
        return out;
    }
    for (index, f) in features.iter().enumerate() {
        if f.locations().is_some() || f.location_unsupported.is_some() {
            continue;
        }
        let span = f.span(total);
        let offset = (f.start + total - fragment.start % total) % total;
        if offset + span <= fragment.length {
            out.inside.push(CarriedFeature { index, start: offset, end: offset + span - 1 });
        } else if offset < fragment.length || offset + span > total {
            out.clipped.push(index);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ligation.
// ---------------------------------------------------------------------------

/// A linear double-stranded piece about to be ligated, held as its top strand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub sequence: String,
    pub left: FragmentEnd,
    pub right: FragmentEnd,
}

impl Piece {
    /// The piece turned round: the other strand read 5' to 3', and the ends swapped.
    pub fn flipped(&self) -> Piece {
        Piece { sequence: revcomp(&self.sequence), left: self.right.flipped(), right: self.left.flipped() }
    }
}

impl From<&Fragment> for Piece {
    fn from(f: &Fragment) -> Piece {
        Piece { sequence: f.sequence.clone(), left: f.left.clone(), right: f.right.clone() }
    }
}

/// Whether the 3' end of one piece's top strand can be ligated to the 5' end of the next piece's
/// top strand: the same kind of end, and single-stranded bases that pair.
///
/// Both ends describe their overhang by the same plus-strand reading, so they anneal when those
/// readings are equal. An end the molecule already had has no known chemistry and never joins.
pub fn joins(right: &FragmentEnd, left: &FragmentEnd) -> bool {
    match (&right.overhang, &left.overhang) {
        (Some(a), Some(b)) => a.kind == b.kind && a.seq.eq_ignore_ascii_case(&b.seq),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// The insert goes in as given.
    Forward,
    /// The insert goes in the other way round.
    Reverse,
}

/// One bond the ligase has to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Junction {
    /// The enzyme that made the end on the 5' side of the bond.
    pub upstream_enzyme: Option<&'static str>,
    /// The enzyme that made the end on the 3' side.
    pub downstream_enzyme: Option<&'static str>,
    /// Gap coordinate on the product: the bond between bases `at - 1` and `at`. On a circular
    /// product `0` is the bond through the origin, between the last base and the first.
    pub at: usize,
    pub overhang: Overhang,
}

/// A product that can be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigationProduct {
    pub orientation: Orientation,
    pub sequence: String,
    pub circular: bool,
    /// Vector first, then the insert: the bond into the insert, then the bond back out of it.
    pub junctions: Vec<Junction>,
    /// 0-based first base of the insert on the product.
    pub insert_start: usize,
    /// 0-based last base of the insert on the product.
    pub insert_end: usize,
}

/// Put `insert` into the opened `vector` and close the circle, in every orientation whose ends can
/// be ligated. Forward first. An empty result means the ends cannot be joined at all.
///
/// The vector's top strand is kept as it is and the insert follows it, so the product's origin is
/// the vector's first base and the insert's position on the product is the vector's length onward.
pub fn ligate(vector: &Piece, insert: &Piece) -> Vec<LigationProduct> {
    let mut out = Vec::new();
    for (orientation, piece) in [(Orientation::Forward, insert.clone()), (Orientation::Reverse, insert.flipped())] {
        if !joins(&vector.right, &piece.left) || !joins(&piece.right, &vector.left) {
            continue;
        }
        let at = vector.sequence.len();
        let (Some(into), Some(back)) = (piece.left.overhang.clone(), vector.left.overhang.clone()) else {
            continue;
        };
        out.push(LigationProduct {
            orientation,
            sequence: format!("{}{}", vector.sequence, piece.sequence),
            circular: true,
            junctions: vec![
                Junction { upstream_enzyme: vector.right.enzyme, downstream_enzyme: piece.left.enzyme, at, overhang: into },
                Junction { upstream_enzyme: piece.right.enzyme, downstream_enzyme: vector.left.enzyme, at: 0, overhang: back },
            ],
            insert_start: at,
            insert_end: at + piece.sequence.len().saturating_sub(1),
        });
    }
    out
}

/// Whether an opened vector can close on itself with no insert, which is the background a cloning
/// reaction has to beat.
pub fn can_self_close(vector: &Piece) -> bool {
    joins(&vector.right, &vector.left)
}

/// Whether the product still carries a site the enzyme would cut at exactly this bond, which is
/// what "the site survives" means: cut the product again and the same junction comes apart.
pub fn site_survives_at(product: &str, circular: bool, enzyme: &str, at: usize) -> bool {
    let Some(enz) = get_enzyme(enzyme) else {
        return false;
    };
    find_cut_sites(product, &[enz.name], circular).iter().any(|s| s.top_cut == at)
}
