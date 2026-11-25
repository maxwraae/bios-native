//! Restriction enzymes and where they cut. Ported from the Bios web app.
//!
//! The table (`enzymes_data.rs`) also holds recognition-only records and methyltransferases.
//! Cleavage is calculated only where the table says it is supported: an entry without offsets
//! must never become a blunt cut. `tests/enzymes.rs` pins the supported coordinates and the
//! exclusions.
//!
//! Coordinate model (0-based, plus strand):
//! - `recognition_start` is the leftmost base of the recognition match.
//! - `top_cut` and `bottom_cut` are GAP coordinates: a value `g` means the bond between plus-strand
//!   bases `g-1` and `g` is cut. So EcoRI G^AATTC has `top_cut = recognition_start + 1`. On a linear
//!   molecule a cut runs from 0 (before the first base) to N (after the last). On a circular one
//!   every coordinate is reported mod N, so a site that starts near the end and runs through the
//!   origin keeps its start and its cuts wrap round to small numbers.
//! - The two offsets come straight from the table and are added to `recognition_start` for a
//!   forward (plus-strand) match. A reverse match maps back through the origin AND swaps top and
//!   bottom, because the reverse frame's top strand is the plus strand's bottom.
//!
//! Type IIS enzymes (BsaI, BbsI, ...) cut OUTSIDE their recognition site: their offsets exceed the
//! site length, so the cuts land downstream of it with no special-casing. Two-sided cutters carry
//! both double-strand cleavage pairs; each pair is a separate [`CutSite`] tied to the same
//! recognition site.
//!
//! A degenerate site matches by letter set, exactly as the table's pattern spells it out: `R` in a
//! site matches A, G and R in the sequence, `N` matches every IUPAC letter. An ambiguous base in
//! the sequence therefore matches only a site letter that covers all of it, never a plain base.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, OnceLock};

use crate::enzymes_data::ENZYME_TABLE;
use crate::model::Strand;
use crate::seq::{clean_seq, revcomp};

/// One double-strand cleavage: where the top and the bottom strand are cut, as plus-strand gap
/// offsets from `recognition_start`. An offset may be negative (a cut upstream of the site).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CutPair {
    pub top: i64,
    pub bottom: i64,
}

/// What the table knows about where an enzyme cuts. Unsupported cleavage has no numeric offsets:
/// recognition alone must never imply a cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleavage {
    Supported {
        /// Plus-strand gap offsets from `recognition_start`; zero is a valid known offset.
        top_snip_offset: i64,
        bottom_snip_offset: i64,
        /// Additional double-strand cleavage pairs, in the forward recognition frame.
        additional_cuts: &'static [CutPair],
    },
    Unverified,
    Methyltransferase,
}

/// [`Cleavage`] without its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleavageStatus {
    Supported,
    Unverified,
    Methyltransferase,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Enzyme {
    /// Canonical display name, e.g. "EcoRI".
    pub name: &'static str,
    /// Catalog aliases. Unverified groups establish recognition, not equivalent cleavage.
    pub aliases: &'static [&'static str],
    /// Recognition sequence, uppercase, IUPAC codes allowed (e.g. "GGWCC").
    pub site: &'static str,
    /// The site on a strand as a case-insensitive pattern, IUPAC-expanded. This is what is matched.
    pub forward_regex: &'static str,
    /// Same for the reverse strand; equals `forward_regex` exactly when palindromic.
    pub reverse_regex: &'static str,
    /// Recognition site length in bases.
    pub length: usize,
    /// True when the site reads the same on both strands (scan one strand, not two).
    pub palindromic: bool,
    /// Type IIS / cuts outside the recognition site.
    pub type2s: bool,
    pub cleavage: Cleavage,
    /// A separately sourced correction, when the vendored catalog has been overridden.
    pub cleavage_source: Option<&'static str>,
}

impl Enzyme {
    pub fn cleavage_status(&self) -> CleavageStatus {
        match self.cleavage {
            Cleavage::Supported { .. } => CleavageStatus::Supported,
            Cleavage::Unverified => CleavageStatus::Unverified,
            Cleavage::Methyltransferase => CleavageStatus::Methyltransferase,
        }
    }

    pub fn top_snip_offset(&self) -> Option<i64> {
        match self.cleavage {
            Cleavage::Supported { top_snip_offset, .. } => Some(top_snip_offset),
            _ => None,
        }
    }

    pub fn bottom_snip_offset(&self) -> Option<i64> {
        match self.cleavage {
            Cleavage::Supported { bottom_snip_offset, .. } => Some(bottom_snip_offset),
            _ => None,
        }
    }

    /// The extra cleavage pairs of a two-sided cutter; empty for every other enzyme.
    pub fn additional_cuts(&self) -> &'static [CutPair] {
        match self.cleavage {
            Cleavage::Supported { additional_cuts, .. } => additional_cuts,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverhangKind {
    /// 5' overhang: the top strand is cut left of the bottom strand.
    FivePrime,
    ThreePrime,
    Blunt,
}

/// The single-stranded end left by a cut. `Blunt` means length 0 and an empty `seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overhang {
    pub kind: OverhangKind,
    pub length: usize,
    /// The plus-strand bases between the two cuts, read through the origin when circular.
    pub seq: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutSite {
    /// Canonical enzyme name.
    pub enzyme: &'static str,
    /// 0-based, plus strand: leftmost base of the recognition match.
    pub recognition_start: usize,
    /// Plus-strand top-strand scission, as a gap coordinate (see the module docs).
    pub top_cut: usize,
    /// Plus-strand bottom-strand scission, as a gap coordinate.
    pub bottom_cut: usize,
    /// Strand the recognition site reads on. Palindromic sites are always reported as forward.
    pub strand: Strand,
    /// Pair number (from 1) in the enzyme's forward frame, present for two-sided cutters.
    pub cut_index: Option<usize>,
    /// Number of cleavage pairs specified per recognition site, including any off a linear end.
    pub cuts_per_recognition: Option<usize>,
    pub overhang: Overhang,
}

/// The table read once into what lookups and matching need.
struct Lookup {
    /// UPPERCASE canonical name to its place in the table.
    by_name: HashMap<String, usize>,
    /// Every name and alias (uppercased) to the uppercased canonical name, so isoschizomers resolve too.
    alias_index: HashMap<String, String>,
    /// Each entry's `forward_regex` as one letter set per base, in table order.
    patterns: Vec<Option<Vec<u32>>>,
}

/// Names the vendored catalog does not group with their isoschizomer, added by hand because a
/// person asking for one of them means an enzyme Bios already has. Each pair is the same
/// recognition sequence AND the same cleavage, checked against the supplier's own isoschizomer
/// chart; a name that merely shares a site goes nowhere near this list. `ENZYME_TABLE` is
/// generated and must not be edited, so the pairing lives here.
///
/// - PaqCI / AarI: both CACCTGC(4/8) (NEB's isoschizomer chart, AarI R0745). PaqCI is the one
///   sold today and the one a Golden Gate protocol names, so it has to resolve.
const EXTRA_ALIASES: &[(&str, &str)] = &[("PaqCI", "AarI")];

fn lookup() -> &'static Lookup {
    static LOOKUP: OnceLock<Lookup> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut by_name = HashMap::new();
        let mut alias_index = HashMap::new();
        for (i, e) in ENZYME_TABLE.iter().enumerate() {
            let key = e.name.to_uppercase();
            by_name.insert(key.clone(), i);
            // A canonical name always wins its own key, even over an alias of an earlier entry.
            alias_index.insert(key.clone(), key.clone());
            for alias in e.aliases {
                alias_index.entry(alias.to_uppercase()).or_insert_with(|| key.clone());
            }
        }
        for (extra, canonical) in EXTRA_ALIASES {
            let key = canonical.to_uppercase();
            debug_assert!(by_name.contains_key(&key), "{canonical} is not in the table");
            alias_index.entry(extra.to_uppercase()).or_insert(key);
        }
        let patterns = ENZYME_TABLE.iter().map(|e| compile_site(e.forward_regex)).collect();
        Lookup { by_name, alias_index, patterns }
    })
}

impl Lookup {
    fn index_of(&self, name: &str) -> Option<usize> {
        let canonical = self.alias_index.get(&trim_js(name).to_uppercase())?;
        self.by_name.get(canonical).copied()
    }
}

/// The web app's `String.trim`: Unicode spaces and line ends, plus the byte-order mark that a name
/// read from a file can carry. Rust's `trim` leaves the mark on and takes U+0085 off.
fn trim_js(s: &str) -> &str {
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// All enzymes, in table order. No two canonical names differ only by case, so this is also one
/// entry per uppercased name.
pub fn enzymes() -> &'static [Enzyme] {
    ENZYME_TABLE
}

/// Resolve an enzyme by canonical name OR any alias, case-insensitively.
pub fn get_enzyme(name: &str) -> Option<&'static Enzyme> {
    ENZYME_TABLE.get(lookup().index_of(name)?)
}

/// The workhorse cloning enzymes: the default map/search set. Curated (not generated) so the
/// resting map stays legible. Every name is checked against the table on first use, and any that
/// is not in the dataset is silently dropped rather than failing.
pub static COMMON_ENZYMES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| COMMON_ENZYME_NAMES.iter().copied().filter(|n| get_enzyme(n).is_some()).collect());

#[rustfmt::skip]
const COMMON_ENZYME_NAMES: &[&str] = &[
    // Classic 6-cutters
    "EcoRI", "BamHI", "HindIII", "XhoI", "XbaI", "SalI", "PstI", "SphI", "SmaI", "KpnI",
    "SacI", "NcoI", "NdeI", "NheI", "NotI", "EcoRV", "ClaI", "BglII", "SpeI", "ApaI",
    "AvrII", "AflII", "AgeI", "AscI", "BspEI", "BsrGI", "DraI", "FseI", "HpaI", "MfeI",
    "MluI", "NruI", "PacI", "PmeI", "PvuI", "PvuII", "SacII", "ScaI", "SnaBI", "StuI",
    "SwaI", "AatII", "Acc65I", "AclI", "AsiSI", "BbeI", "BclI", "BmtI", "BstBI", "EagI",
    "EcoNI", "MscI", "NsiI", "PciI", "PmlI", "PspOMI", "RsrII", "XcmI", "ZraI", "BstZ17I",
    // Type IIS / Golden Gate
    "BsaI", "BbsI", "BsmBI", "SapI", "AarI", "BtgZI", "Esp3I", "PaqCI", "BspQI",
    // Common frequent cutters
    "AluI", "HaeIII", "HhaI", "MspI", "RsaI", "TaqI", "Sau3AI", "MboI", "DpnI", "NlaIII",
];

// internals

/// One bit per letter of the alphabet, either case. `None` for anything that is not a letter.
fn letter_bit(c: u8) -> Option<u32> {
    c.is_ascii_alphabetic().then(|| 1 << (c.to_ascii_uppercase() - b'A'))
}

/// The table's recognition pattern as one letter set per base. The patterns are the output of one
/// generator and use only three things: a letter, a `[...]` class of letters, and a `{n}` repeat
/// of either. Anything else gives `None` and that enzyme is not searched for; a test below holds
/// every entry to this.
fn compile_site(pattern: &str) -> Option<Vec<u32>> {
    let mut sets = Vec::new();
    let mut bytes = pattern.bytes().peekable();
    while let Some(b) = bytes.next() {
        let mut set = 0;
        if b == b'[' {
            loop {
                match bytes.next()? {
                    b']' => break,
                    c => set |= letter_bit(c)?,
                }
            }
        } else {
            set = letter_bit(b)?;
        }
        let mut count = 1;
        if bytes.next_if_eq(&b'{').is_some() {
            count = 0usize;
            loop {
                match bytes.next()? {
                    b'}' => break,
                    d @ b'0'..=b'9' => count = count.checked_mul(10)?.checked_add(usize::from(d - b'0'))?,
                    _ => return None,
                }
            }
        }
        // No site is longer than a few dozen bases; a repeat beyond that is not a site.
        if count > MAX_SITE_LENGTH || sets.len() + count > MAX_SITE_LENGTH {
            return None;
        }
        sets.extend(std::iter::repeat_n(set, count));
    }
    Some(sets)
}

const MAX_SITE_LENGTH: usize = 64;

/// Whether the site starts at base `j` of `text`. On a circular molecule the site may run through
/// the origin; the caller has already made sure it is no longer than the molecule, so it cannot
/// come round to bases it has used.
fn matches_at(text: &[u8], j: usize, pattern: &[u32], circular: bool) -> bool {
    let n = text.len();
    if n == 0 || pattern.len() > n || (!circular && j + pattern.len() > n) {
        return false;
    }
    pattern.iter().enumerate().all(|(k, set)| text.get((j + k) % n).and_then(|&b| letter_bit(b)).is_some_and(|bit| set & bit != 0))
}

/// A coordinate as it is reported: wrapped onto a circular molecule, or `None` when it falls off a
/// linear one (below 0 or past the last gap, N).
fn place(x: i64, n: i64, circular: bool) -> Option<usize> {
    let x = if circular { x.checked_rem_euclid(n)? } else { x };
    usize::try_from(x).ok().filter(|_| x <= n)
}

/// Read plus-strand bases `lo..=hi_inclusive`, wrapping the origin when circular.
fn slice_wrap(seq: &str, lo: i64, hi_inclusive: i64, circular: bool) -> String {
    let bytes = seq.as_bytes();
    let n = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    (lo..=hi_inclusive)
        .filter_map(|i| {
            let at = if circular { i.checked_rem_euclid(n)? } else { i };
            bytes.get(usize::try_from(at).ok()?).map(|&b| char::from(b))
        })
        .collect()
}

/// Build the overhang descriptor from two plus-strand gap coordinates.
fn overhang_of(seq: &str, top_cut_abs: i64, bottom_cut_abs: i64, circular: bool) -> Overhang {
    if top_cut_abs == bottom_cut_abs {
        return Overhang { kind: OverhangKind::Blunt, length: 0, seq: String::new() };
    }
    let kind = if top_cut_abs < bottom_cut_abs { OverhangKind::FivePrime } else { OverhangKind::ThreePrime };
    let lo = top_cut_abs.min(bottom_cut_abs);
    let hi = top_cut_abs.max(bottom_cut_abs) - 1; // gap to base: last base before the far cut
    Overhang { kind, length: usize::try_from(hi - lo + 1).unwrap_or(0), seq: slice_wrap(seq, lo, hi, circular) }
}

/// Every cleavage pair of a supported enzyme, in cut-index order: the additional pairs first, then
/// the table's main pair. `None` when cleavage is not supported.
fn cleavage_pairs(enz: &Enzyme) -> Option<Vec<CutPair>> {
    let Cleavage::Supported { top_snip_offset, bottom_snip_offset, additional_cuts } = enz.cleavage else {
        return None;
    };
    let mut pairs = additional_cuts.to_vec();
    pairs.push(CutPair { top: top_snip_offset, bottom: bottom_snip_offset });
    Some(pairs)
}

/// One molecule being searched, with what every match on it needs.
struct Target<'a> {
    seq: &'a str,
    n: i64,
    circular: bool,
}

impl Target<'_> {
    /// The cuts one recognition match at base `j` of the scanned strand gives, one per cleavage
    /// pair that lies on the molecule. `j` counts along the reverse complement for a reverse match.
    fn cuts_at(&self, enz: &'static Enzyme, pairs: &[CutPair], strand: Strand, j: i64) -> Vec<CutSite> {
        let n = self.n;
        let forward = strand == Strand::Forward;
        let length = i64::try_from(enz.length).unwrap_or(i64::MAX);
        let Some(recognition_start) = place(if forward { j } else { n - j - length }, n, self.circular) else {
            return Vec::new();
        };
        let mut cuts = Vec::new();
        for (index, pair) in pairs.iter().enumerate() {
            let (top, bottom) = if forward { (j + pair.top, j + pair.bottom) } else { (n - j - pair.bottom, n - j - pair.top) };
            // A pair is available only if BOTH scissions are on the linear molecule.
            let (Some(top_cut), Some(bottom_cut)) = (place(top, n, self.circular), place(bottom, n, self.circular)) else {
                continue;
            };
            let two_sided = pairs.len() > 1;
            cuts.push(CutSite {
                enzyme: enz.name,
                recognition_start,
                top_cut,
                bottom_cut,
                strand,
                cut_index: two_sided.then_some(index + 1),
                cuts_per_recognition: two_sided.then_some(pairs.len()),
                // Derive polarity and bases BEFORE circular normalization.
                overhang: overhang_of(self.seq, top, bottom, self.circular),
            });
        }
        cuts
    }
}

/// Every position where any of `enzyme_names` cuts `sequence`, on both strands.
///
/// `sequence` is raw (cleaned here); `enzyme_names` are canonical names or aliases; `circular` is
/// required (a circular target scanned as linear silently loses origin-crossing sites). Results
/// are ascending by `recognition_start`, then forward before reverse, then by cut index; cuts that
/// tie keep the order their enzymes were asked for in. Unknown names and catalog entries without
/// supported double-strand cleavage positions are skipped (not asserted to be noncutters).
pub fn find_cut_sites<S: AsRef<str>>(sequence: &str, enzyme_names: &[S], circular: bool) -> Vec<CutSite> {
    let seq = clean_seq(sequence);
    let Ok(n) = i64::try_from(seq.len()) else {
        return Vec::new();
    };
    if n == 0 {
        return Vec::new();
    }
    let rc = revcomp(&seq);
    let target = Target { seq: &seq, n, circular };
    let lookup = lookup();

    let mut sites = Vec::new();
    // An enzyme asked for twice, or under two of its names, is reported once.
    let mut seen = HashSet::new();
    for raw_name in enzyme_names {
        let Some(index) = lookup.index_of(raw_name.as_ref()) else { continue };
        let (Some(enz), Some(Some(pattern))) = (ENZYME_TABLE.get(index), lookup.patterns.get(index)) else { continue };
        let Some(pairs) = cleavage_pairs(enz) else { continue };
        if enz.length > seq.len() {
            continue;
        }
        let strands: &[Strand] = if enz.palindromic { &[Strand::Forward] } else { &[Strand::Forward, Strand::Reverse] };
        for &strand in strands {
            let text = if strand == Strand::Forward { seq.as_bytes() } else { rc.as_bytes() };
            for j in (0..n).filter(|&j| usize::try_from(j).is_ok_and(|j| matches_at(text, j, pattern, circular))) {
                for cut in target.cuts_at(enz, &pairs, strand, j) {
                    if seen.insert((cut.enzyme, cut.recognition_start, cut.strand.sign(), cut.cut_index)) {
                        sites.push(cut);
                    }
                }
            }
        }
    }
    sites.sort_by(|a, b| {
        let by_start = a.recognition_start.cmp(&b.recognition_start);
        let by_strand = b.strand.sign().cmp(&a.strand.sign());
        by_start.then(by_strand).then(a.cut_index.unwrap_or(1).cmp(&b.cut_index.unwrap_or(1)))
    });
    sites
}

/// Plus-strand gap offsets relative to `recognition_start`, before circular wrapping.
pub fn cut_offsets(c: &CutSite) -> Option<CutPair> {
    let enz = get_enzyme(c.enzyme)?;
    let pair = *cleavage_pairs(enz)?.get(c.cut_index.unwrap_or(1).checked_sub(1)?)?;
    let length = i64::try_from(enz.length).ok()?;
    Some(match c.strand {
        Strand::Forward => pair,
        Strand::Reverse => CutPair { top: length - pair.bottom, bottom: length - pair.top },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{compile_site, enzymes, get_enzyme, trim_js};

    /// The search reads each pattern as letter sets, one per base of the site. An entry whose
    /// pattern did not read, or read to another length, would silently never be found.
    #[test]
    fn every_pattern_in_the_table_reads_to_the_length_of_its_site() {
        for e in enzymes() {
            for pattern in [e.forward_regex, e.reverse_regex] {
                assert_eq!(compile_site(pattern).map(|sets| sets.len()), Some(e.length), "{} {pattern}", e.name);
            }
            assert_eq!(e.site.len(), e.length, "{}", e.name);
        }
    }

    /// `enzymes()` hands out the table as the web app's name-keyed record; that holds only while
    /// no canonical name shadows another.
    #[test]
    fn canonical_names_are_unique_ignoring_case() {
        let names: HashSet<String> = enzymes().iter().map(|e| e.name.to_uppercase()).collect();
        assert_eq!(names.len(), enzymes().len());
    }

    #[test]
    fn a_pattern_outside_the_three_forms_is_refused() {
        assert_eq!(compile_site("g[acm]t{2}"), Some(vec![1 << 6, 0b1_0000_0000_0101, 1 << 19, 1 << 19]));
        for bad in ["ga|tc", "g.c", "g[ac", "ga{2", "ga{x}", "ga+", "g{99999999999999999999}", "g{65}"] {
            assert_eq!(compile_site(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_name_is_trimmed_as_the_web_app_trims_it() {
        assert_eq!(trim_js("\u{feff} \tEcoRI\r\n\u{a0}"), "EcoRI");
        assert_eq!(get_enzyme("\u{feff}ecori\n").map(|e| e.name), Some("EcoRI"));
        assert!(get_enzyme("\u{85}EcoRI").is_none());
    }
}
