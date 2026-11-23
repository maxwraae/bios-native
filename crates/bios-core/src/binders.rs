//! A binder is an oligo held against the open construct (an ASO, a primer, a guide) that has NOT
//! landed in the file. Ported from the Bios web app's `editor/binders.ts`.
//!
//! The whole point is what a binder does not have: a position. Nothing here stores one. Where it
//! binds is asked of the sequence every time, which is why an off-target site cannot be forgotten
//! to be checked: it arrives in the same list as the on-target one, by construction.
//!
//! Binders are never written to a file: a construct is byte-identical whether fifty are held or
//! none. Landing one is a different act (an assembly), not a save.
//!
//! The question asked is the PRIMER question ([`find_primer_sites`]), not the plain duplex one:
//! sites are anchored at the oligo's 3' end and a non-matching 5' tail may hang off. A tailed
//! primer is the whole point of Gibson and of adding a site or a tag, and the plain duplex question
//! answers "binds nowhere" for one. The trade: a site whose 3'-most base is unpaired, or one melting
//! below the floor, is not reported. For a primer that is correct; neither primes.

use crate::primer::{PrimerSearchOptions, PrimerSite, find_primer_sites};
use crate::seq::{gc_content, revcomp};

/// How far off a duplex can be before it stops being called a site at all.
pub const MAX_MISMATCHES: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binder {
    /// Unique among the binders held in one window; binders never reach disk.
    pub id: String,
    pub name: String,
    /// The oligo, 5'→3': the thing you would actually order.
    pub bases: String,
}

/// One place a binder lands, with everything about it already worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct BinderSite {
    pub site: PrimerSite,
    /// `<binder id>-site-<n>`.
    pub id: String,
    pub binder_id: String,
    pub name: String,
    /// The binder's oligo, carried through so the rows can draw the duplex.
    pub bases: String,
    /// 0..100, of the whole oligo, tail included. Tm belongs to the site (`site.tm`), because a
    /// tailed oligo's two sites can anneal by different amounts.
    pub gc: f64,
    /// This site anneals by LESS than the binder's best site does: only part of the oligo found
    /// anything here. A deliberate 5' tail and a chance landing both leave 5' bases unpaired; only
    /// the comparison against the oligo's own best site tells them apart.
    pub partial: bool,
    /// The oligo's one full, perfect landing. An oligo with two equally good sites has no
    /// on-target: it is ambiguous, and every site is suspect.
    pub on_target: bool,
}

/// The binder that hits `sequence[start..=end]` (0-based): the reverse complement of what was
/// selected, which is what actually anneals there. `None` for a span outside the sequence.
/// `number` names it (`binder <number>`) and makes its id.
pub fn binder_from_selection(sequence: &str, start: usize, end: usize, number: u64) -> Option<Binder> {
    let picked = sequence.get(start..=end).filter(|_| start <= end)?;
    Some(Binder { id: format!("binder-{number}"), name: format!("binder {number}"), bases: revcomp(picked) })
}

/// A binder from an oligo you already have, pasted off an order sheet rather than drawn on the map.
/// Everything but letters is dropped. `None` when no letters are left.
pub fn binder_from_bases(bases: &str, name: Option<&str>, number: u64) -> Option<Binder> {
    let clean: String = bases.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase()).collect();
    if clean.is_empty() {
        return None;
    }
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| format!("binder {number}"), str::to_string);
    Some(Binder { id: format!("binder-{number}"), name, bases: clean })
}

/// Every place these binders land on this sequence.
pub fn binders_to_sites(binders: &[Binder], sequence: &str, circular: bool) -> Vec<BinderSite> {
    let options = PrimerSearchOptions { max_mismatches: MAX_MISMATCHES, circular, ..PrimerSearchOptions::default() };
    let mut out = Vec::new();
    for b in binders {
        let sites = find_primer_sites(sequence, &b.bases, options).unwrap_or_default();
        for (i, (site, partial, on_target)) in classify_sites(sites).into_iter().enumerate() {
            out.push(BinderSite {
                site,
                id: format!("{}-site-{i}", b.id),
                binder_id: b.id.clone(),
                name: b.name.clone(),
                bases: b.bases.clone(),
                gc: gc_content(&b.bases),
                partial,
                on_target,
            });
        }
    }
    out
}

/// Which of ONE oligo's sites is the real one: each site with its `partial` and `on_target`.
///
/// The oligo's best landing defines "fully annealed" for this oligo. An untailed primer's best site
/// anneals end to end and a Gibson primer's leaves its overlap hanging; both are correct. Anything
/// that anneals by less than the best is a partial. Full length matters and is not redundant with
/// no mismatches: a tail is not a mismatch, so a chance 14-of-19 anneal scores zero mismatches too.
/// Find's alignment asks the same question of a kept oligo, so there is one definition of
/// off-target.
pub fn classify_sites(sites: Vec<PrimerSite>) -> Vec<(PrimerSite, bool, bool)> {
    let best = sites.iter().map(|s| s.anneal_length).max().unwrap_or(0);
    let full = sites.iter().filter(|s| s.anneal_length == best && s.mismatches == 0).count();
    sites
        .into_iter()
        .map(|s| {
            let partial = s.anneal_length < best;
            let on_target = s.mismatches == 0 && s.anneal_length == best && full == 1;
            (s, partial, on_target)
        })
        .collect()
}

/// What a site says about itself beside its name (`siteLabel`): a partial is named for what it is,
/// a designed tail as a tail, mismatches counted, and a second equally good landing as such.
pub fn site_label(s: &BinderSite) -> String {
    let mut notes: Vec<String> = Vec::new();
    if s.partial {
        notes.push(format!("only {} of {} nt anneal", s.site.anneal_length, s.bases.len()));
    } else if s.site.tail_length > 0 {
        notes.push(format!("{}nt 5' tail", s.site.tail_length));
    }
    if s.site.mismatches > 0 {
        notes.push(format!("{} mismatch{}", s.site.mismatches, if s.site.mismatches > 1 { "es" } else { "" }));
    }
    if !s.on_target && !s.partial && s.site.mismatches == 0 && s.site.tail_length == 0 {
        notes.push("also binds here".to_string());
    }
    if notes.is_empty() { s.name.clone() } else { format!("{} — {}", s.name, notes.join(", ")) }
}

/// The ANNEALED bases of a site, 5'→3': what is drawn over its footprint. The tail has no column,
/// so handing the whole oligo to a span shorter than it would desync the duplex from its own
/// coordinates; the tail lives in the label instead.
pub fn annealed_bases(s: &BinderSite) -> &str {
    s.bases.get(s.site.tail_length..).unwrap_or_default()
}
