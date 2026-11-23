//! The product of two primers on a template. Ported from the Bios web app.
//!
//! Template coordinates are 0-based inclusive on the plus strand, and a stored site through the
//! origin of a circular template has `end < start`. Product coordinates are 0-based inclusive on
//! the product, which is always linear and reads in the forward primer's direction.

use crate::model::{Fuzzy, LocationOperator, Segment};
use crate::primer_binding::{BindingStatus, PrimerBindingCheck, PrimerBindingInput, compare_primer_binding, normalize};
use crate::seq::revcomp;

/// One primer and where it is stored as binding: a [`PrimerBindingInput`] without the template.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PcrPrimer {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Full oligo, always 5′→3′, including its deliberate 5′ tail.
    pub bases: String,
    pub start: i64,
    pub end: i64,
    pub forward: bool,
    pub tail_length: usize,
    pub tail_length_inferred: bool,
    pub unbound: bool,
    pub locations: Vec<Segment>,
    pub location_operator: Option<LocationOperator>,
    pub start_fuzzy: Option<Fuzzy>,
    pub end_fuzzy: Option<Fuzzy>,
    pub location_unsupported: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcrInput {
    pub sequence: String,
    pub circular: bool,
    pub forward: PcrPrimer,
    pub reverse: PcrPrimer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcrContribution {
    ForwardPrimer,
    Template,
    ReversePrimer,
    Overlap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcrProductSegment {
    pub kind: PcrContribution,
    pub product_start: usize,
    pub product_end: usize,
    /// `None` for deliberate tails or primer-installed insertions. Wraps split segments.
    pub template_start: Option<usize>,
    pub template_end: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcrProductPrimerSite {
    pub start: usize,
    pub end: usize,
    pub forward: bool,
    pub bases: String,
    /// Always 0: the full oligo, including its former tail, is now present in the product
    /// template.
    pub tail_length: usize,
    pub source_tail_length: usize,
    pub source_id: Option<String>,
    pub name: Option<String>,
}

/// The stretch of the template the product was copied from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateArc {
    pub start: usize,
    pub end: usize,
    pub length: usize,
    pub wraps_origin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcrPrimerSites {
    pub forward: PcrProductPrimerSite,
    pub reverse: PcrProductPrimerSite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcrProduct {
    pub sequence: String,
    /// Always false.
    pub circular: bool,
    /// Source coordinates are zero-based inclusive; length includes explicit terminal overlap.
    pub template_arc: TemplateArc,
    /// One entry per product base. A substituted base retains its original template coordinate.
    pub template_map: Vec<Option<usize>>,
    pub contributions: Vec<PcrContribution>,
    pub segments: Vec<PcrProductSegment>,
    pub primer_sites: PcrPrimerSites,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bindings {
    pub forward: PrimerBindingCheck,
    pub reverse: PrimerBindingCheck,
}

/// A product, or why there is none (the web app's `ok`). Both carry the two binding reports.
#[derive(Clone, Debug, PartialEq)]
pub enum PcrPrediction {
    Refused { reasons: Vec<String>, bindings: Bindings },
    Product { product: Box<PcrProduct>, caveats: Vec<String>, bindings: Bindings },
}

impl PcrPrediction {
    pub fn ok(&self) -> bool {
        matches!(self, PcrPrediction::Product { .. })
    }

    pub fn bindings(&self) -> &Bindings {
        match self {
            PcrPrediction::Refused { bindings, .. } | PcrPrediction::Product { bindings, .. } => bindings,
        }
    }
}

fn acgt(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|base| matches!(base, b'A' | b'C' | b'G' | b'T'))
}

fn footprint_length(start: usize, end: usize, length: usize) -> usize {
    if end >= start { end - start + 1 } else { (length + end + 1).saturating_sub(start) }
}

fn binding_input<'a>(primer: &'a PcrPrimer, sequence: &'a str, circular: bool) -> PrimerBindingInput<'a> {
    PrimerBindingInput {
        sequence,
        bases: &primer.bases,
        start: primer.start,
        end: primer.end,
        forward: primer.forward,
        tail_length: primer.tail_length,
        tail_length_inferred: primer.tail_length_inferred,
        circular,
        unbound: primer.unbound,
        locations: &primer.locations,
        location_operator: primer.location_operator,
        start_fuzzy: primer.start_fuzzy,
        end_fuzzy: primer.end_fuzzy,
        location_unsupported: primer.location_unsupported.as_deref(),
    }
}

fn binding_reasons(label: &str, primer: &PcrPrimer, check: &PrimerBindingCheck) -> Vec<String> {
    let mut reasons: Vec<String> = Vec::new();
    if primer.unbound || check.status == BindingStatus::Unbound {
        reasons.push(format!("{label} primer has no stored binding site."));
    } else if primer.tail_length_inferred {
        reasons.push(format!("{label} primer has an inferred 5′ tail; confirm its tail length first."));
    } else if check.status == BindingStatus::Unverified {
        reasons.push(format!("{label} primer binding is unverified: {}.", check.summary));
    } else if check.three_prime_matched != Some(true) {
        reasons.push(format!("{label} primer's 3′ end is not paired at the stored binding site."));
    }
    if !check.bases.is_empty() && !acgt(&check.bases) {
        reasons.push(format!("{label} primer contains ambiguous or non-ACGT bases; a single exact oligo is required."));
    }
    let gap_at_an_end = |aligned: &Option<String>| aligned.as_deref().is_some_and(|text| text.starts_with('-') || text.ends_with('-'));
    let both_aligned = check.aligned_primer.as_deref().is_some_and(|text| !text.is_empty())
        && check.aligned_template.as_deref().is_some_and(|text| !text.is_empty());
    if both_aligned && (gap_at_an_end(&check.aligned_primer) || gap_at_an_end(&check.aligned_template)) {
        reasons.push(format!(
            "{label} primer has a terminal alignment gap. Confirm the binding boundaries and explicit 5′ tail before predicting a product."
        ));
    }
    reasons
}

/// Full oligo coordinates in its own 5′→3′ order, including `None` for inserted/tail bases.
/// `anchor` is the template position under the oligo's first annealed base: the footprint's start
/// for a forward primer, its end for a reverse one.
fn oligo_map(forward: bool, anchor: usize, check: &PrimerBindingCheck, length: usize) -> Vec<Option<usize>> {
    let mut out: Vec<Option<usize>> = vec![None; check.tail.len()];
    let mut template_offset = 0usize;
    let aligned = check.aligned_primer.as_deref().unwrap_or_default().bytes().zip(check.aligned_template.as_deref().unwrap_or_default().bytes());
    for (query, target) in aligned {
        if query != b'-' {
            out.push(if target == b'-' {
                None
            } else if forward {
                Some((anchor + template_offset) % length)
            } else {
                Some((anchor + length - template_offset % length) % length)
            });
        }
        if target != b'-' {
            template_offset += 1;
        }
    }
    out
}

fn segments_for(map: &[Option<usize>], contributions: &[PcrContribution]) -> Vec<PcrProductSegment> {
    let mut segments: Vec<PcrProductSegment> = Vec::new();
    for (i, (&position, &kind)) in map.iter().zip(contributions).enumerate() {
        if let Some(previous) = segments.last_mut()
            && previous.kind == kind
            && match (position, previous.template_end) {
                (None, None) => true,
                (Some(position), Some(end)) => position == end + 1,
                _ => false,
            }
        {
            previous.product_end = i;
            previous.template_end = position;
        } else {
            segments.push(PcrProductSegment { kind, product_start: i, product_end: i, template_start: position, template_end: position });
        }
    }
    segments
}

/// The product as it is assembled, one entry per base in each list.
#[derive(Default)]
struct Assembly {
    bases: String,
    template_map: Vec<Option<usize>>,
    contributions: Vec<PcrContribution>,
}

impl Assembly {
    fn append(&mut self, bases: &str, map: impl IntoIterator<Item = Option<usize>>, kind: PcrContribution) {
        for (base, position) in bases.chars().zip(map) {
            self.bases.push(base);
            self.template_map.push(position);
            self.contributions.push(kind);
        }
    }

    /// Bases the template has no position for: a tail.
    fn append_unplaced(&mut self, bases: &str, kind: PcrContribution) {
        self.append(bases, std::iter::repeat(None), kind);
    }
}

/// The two footprints on a template of `n` bases. `reverse_offset` is how far the reverse
/// footprint starts after the forward one starts, and `arc_length` runs from the forward
/// footprint's start through the reverse footprint's end.
struct Arc {
    n: usize,
    f_start: usize,
    f_length: usize,
    reverse_offset: usize,
    arc_length: usize,
}

impl Arc {
    fn source_at(&self, offset: usize) -> usize {
        (self.f_start + offset) % self.n
    }
}

/// Two footprints that share template positions, merged base by base. `Err` names the first
/// position where the two oligos disagree.
fn merge_overlapping(assembly: &mut Assembly, sequence: &str, arc: &Arc, bindings: &Bindings) -> Result<(), String> {
    assembly.append_unplaced(&bindings.forward.tail, PcrContribution::ForwardPrimer);
    let forward_bases = bindings.forward.annealing_bases.as_bytes();
    let reverse_bases = revcomp(&bindings.reverse.annealing_bases).into_bytes();
    for offset in 0..arc.arc_length {
        let left = if offset < arc.f_length { forward_bases.get(offset).copied() } else { None };
        let right = offset.checked_sub(arc.reverse_offset).and_then(|i| reverse_bases.get(i)).copied();
        let position = arc.source_at(offset);
        let (base, kind) = match (left, right) {
            (Some(left), Some(right)) if left != right => {
                return Err(format!(
                    "Overlapping primers specify conflicting bases at template position {}: forward {}, reverse-complement {}.",
                    position + 1,
                    char::from(left),
                    char::from(right)
                ));
            }
            (Some(left), Some(_)) => (Some(left), PcrContribution::Overlap),
            (Some(left), None) => (Some(left), PcrContribution::ForwardPrimer),
            (None, Some(right)) => (Some(right), PcrContribution::ReversePrimer),
            (None, None) => (sequence.as_bytes().get(position).copied(), PcrContribution::Template),
        };
        if let Some(base) = base {
            assembly.append(char::from(base).encode_utf8(&mut [0; 4]), [Some(position)], kind);
        }
    }
    assembly.append_unplaced(&revcomp(&bindings.reverse.tail), PcrContribution::ReversePrimer);
    Ok(())
}

/// Predict a sequence from two explicitly selected binding sites; this does not search for sites
/// or model PCR efficiency. Full ordered oligos replace the corresponding template footprints.
/// On a circular template the reverse site's start is unwrapped once relative to the forward
/// site's start, then its own footprint is included. This permits inverse-PCR terminal overlap
/// without choosing an arbitrary extra lap.
pub fn predict_pcr_product(input: &PcrInput) -> PcrPrediction {
    let sequence = normalize(&input.sequence);
    let f = &input.forward;
    let r = &input.reverse;
    let bindings = Bindings {
        forward: compare_primer_binding(&binding_input(f, &sequence, input.circular)),
        reverse: compare_primer_binding(&binding_input(r, &sequence, input.circular)),
    };
    let mut reasons = binding_reasons("Forward", f, &bindings.forward);
    reasons.extend(binding_reasons("Reverse", r, &bindings.reverse));
    let iupac =
        |base: u8| matches!(base, b'A' | b'C' | b'G' | b'T' | b'U' | b'R' | b'Y' | b'S' | b'W' | b'K' | b'M' | b'B' | b'D' | b'H' | b'V' | b'N');
    if sequence.is_empty() || !sequence.bytes().all(iupac) {
        reasons.push("The template must contain a nonempty DNA sequence.".to_string());
    }
    if !f.forward || r.forward {
        reasons.push("Choose a forward-strand primer and a reverse-strand primer, respectively.".to_string());
    }
    if !reasons.is_empty() {
        return PcrPrediction::Refused { reasons, bindings };
    }

    let n = sequence.len();
    // Both bindings were compared at their stored sites, so all four coordinates are inside the
    // template. Should one not be, there is no product to predict.
    let inside = |coordinate: i64| usize::try_from(coordinate).ok().filter(|&position| position < n);
    let (Some(f_start), Some(f_end), Some(r_start), Some(r_end)) = (inside(f.start), inside(f.end), inside(r.start), inside(r.end)) else {
        return PcrPrediction::Refused { reasons: vec!["Stored binding coordinates are outside the template.".to_string()], bindings };
    };
    let f_length = footprint_length(f_start, f_end, n);
    let r_length = footprint_length(r_start, r_end, n);
    let reverse_offset = if input.circular { Some((r_start + n - f_start) % n) } else { r_start.checked_sub(f_start) };
    let Some(reverse_offset) = reverse_offset else {
        return PcrPrediction::Refused {
            reasons: vec![
                "The reverse binding site lies before the forward site on a linear template; the chosen primers do not face across a product."
                    .to_string(),
            ],
            bindings,
        };
    };
    let arc_length = reverse_offset + r_length;
    if arc_length < f_length {
        return PcrPrediction::Refused {
            reasons: vec![
                "The forward binding footprint extends past the reverse primer's 5′ boundary; these sites do not define supported product ends."
                    .to_string(),
            ],
            bindings,
        };
    }
    let overlapping = reverse_offset < f_length;
    let has_gaps =
        bindings.forward.insertions > 0 || bindings.forward.deletions > 0 || bindings.reverse.insertions > 0 || bindings.reverse.deletions > 0;
    if overlapping && has_gaps {
        return PcrPrediction::Refused {
            reasons: vec![
                "Overlapping binding footprints with alignment gaps are not supported; their primer-installed edits require an explicit shared alignment."
                    .to_string(),
            ],
            bindings,
        };
    }

    let arc = Arc { n, f_start, f_length, reverse_offset, arc_length };
    let mut assembly = Assembly::default();
    if !overlapping {
        assembly.append(&bindings.forward.bases, oligo_map(true, f_start, &bindings.forward, n), PcrContribution::ForwardPrimer);
        for offset in f_length..reverse_offset {
            let position = arc.source_at(offset);
            assembly.append(sequence.get(position..=position).unwrap_or_default(), [Some(position)], PcrContribution::Template);
        }
        let reverse_map = oligo_map(false, r_end, &bindings.reverse, n);
        assembly.append(&revcomp(&bindings.reverse.bases), reverse_map.into_iter().rev(), PcrContribution::ReversePrimer);
    } else if let Err(conflict) = merge_overlapping(&mut assembly, &sequence, &arc, &bindings) {
        return PcrPrediction::Refused { reasons: vec![conflict], bindings };
    }

    let Assembly { bases: product_sequence, template_map, contributions } = assembly;
    let site = |primer: &PcrPrimer, check: &PrimerBindingCheck, is_forward: bool| PcrProductPrimerSite {
        start: if is_forward { 0 } else { product_sequence.len().saturating_sub(check.bases.len()) },
        end: if is_forward { check.bases.len().saturating_sub(1) } else { product_sequence.len().saturating_sub(1) },
        forward: is_forward,
        bases: check.bases.clone(),
        tail_length: 0,
        source_tail_length: check.tail.len(),
        source_id: primer.id.clone(),
        name: primer.name.clone(),
    };
    let mut caveats = vec!["Sequence prediction at the selected binding sites; it does not predict experimental amplification success.".to_string()];
    for (label, check) in [("Forward", &bindings.forward), ("Reverse", &bindings.reverse)] {
        if !check.mismatch_positions.is_empty() {
            caveats.push(format!("{label} primer installs {} substitution(s) relative to the template.", check.mismatch_positions.len()));
        }
        if check.insertions > 0 || check.deletions > 0 {
            caveats.push(format!(
                "{label} primer installs {} inserted and {} deleted base(s). Mapping follows the stored-site minimum-edit alignment; equally scoring gap placements may exist.",
                check.insertions, check.deletions
            ));
        }
        if !check.ambiguous_positions.is_empty() {
            caveats.push(format!("{label} binding region contains ambiguous template positions."));
        }
    }
    if arc_length > n {
        caveats.push(format!(
            "The circular template arc includes {} repeated terminal base(s); these positions occur at both product ends.",
            arc_length - n
        ));
    }
    if !product_sequence.is_empty() && !acgt(&product_sequence) {
        caveats.push("Unresolved template ambiguity codes remain in the predicted product.".to_string());
    }
    let product = PcrProduct {
        primer_sites: PcrPrimerSites { forward: site(f, &bindings.forward, true), reverse: site(r, &bindings.reverse, false) },
        segments: segments_for(&template_map, &contributions),
        sequence: product_sequence,
        circular: false,
        template_arc: TemplateArc { start: f_start, end: r_end, length: arc_length, wraps_origin: input.circular && f_start + arc_length > n },
        template_map,
        contributions,
    };
    PcrPrediction::Product { product: Box::new(product), caveats, bindings }
}
