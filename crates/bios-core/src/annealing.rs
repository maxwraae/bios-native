//! What temperature to anneal a PCR at.
//!
//! One rule, named, so the number can be traced back to print rather than to habit:
//!
//! > Ta(opt) = 0.3 × Tm(primer) + 0.7 × Tm(product) − 14.9
//!
//! Rychlik W, Spencer WJ, Rhoads RE, "Optimization of the annealing temperature for DNA
//! amplification in vitro", Nucleic Acids Research 18(21):6409–6412 (1990). `Tm(primer)` is the
//! melting temperature of the LESS STABLE of the two primer-template duplexes — the lower of the
//! two — and `Tm(product)` that of the whole amplicon.
//!
//! What each term is here:
//!
//! - `Tm(primer)`: the lower of the two primers' annealing temperatures as [`crate::seq::tm`]
//!   computes them — nearest-neighbour, SantaLucia 1998, 250 nM total strands, 50 mM Na+ — over
//!   the part of the oligo that anneals, a 5' tail excluded. The 1990 paper worked from the
//!   nearest-neighbour parameters of its day (Breslauer 1986), so Bios's number for the same
//!   primers differs from the paper's own worked examples by of the order of a degree. The rule is
//!   the paper's; the primer thermodynamics are the ones Bios uses everywhere else.
//! - `Tm(product)`: the long-duplex %GC estimate the paper takes from Baldino et al. (1989),
//!   `81.5 + 16.6 × log10[Na+] + 0.41 × %GC − 675 / length`, at the same 50 mM salt, over the
//!   whole product including any 5' tails the primers add. A nearest-neighbour calculation is not
//!   applicable at amplicon length, which is why the paper uses this formula and not that one.
//!
//! It is a starting point for a gradient, not a guarantee. The rule knows nothing about the
//! polymerase, its buffer, magnesium, additives or how specific the primers are on the template;
//! a two-step hot enzyme with its own recommended temperature should be run at that one instead.
//! [`MIN_PRODUCT_LENGTH`] is where Bios stops answering at all.

use crate::seq::{clean_seq, gc_content};

/// The one line that says where the number comes from, for a command to hand an agent verbatim.
pub const ANNEALING_RULE: &str = "Rychlik, Spencer & Rhoads 1990: Ta = 0.3 × Tm(less stable primer) + 0.7 × Tm(product) − 14.9, with the primer Tm nearest-neighbour (SantaLucia 1998, 250 nM strands, 50 mM Na+) over the annealing part and the product Tm from the long-duplex %GC formula 81.5 + 16.6·log10[Na+] + 0.41·%GC − 675/length at 50 mM. A starting point for a gradient, not a guarantee: it accounts for no polymerase, buffer, magnesium or additive.";

/// Shortest product Bios applies the rule to. The product term's `675 / length` correction comes
/// from a formula for duplexes, and below about this length it swamps the other terms and returns
/// a temperature no one should anneal at. Under this length Bios gives no number rather than a
/// misleading one.
pub const MIN_PRODUCT_LENGTH: usize = 50;

/// A recommended annealing temperature and every term it was built from, so that it can be
/// checked by hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnnealingTemperature {
    /// Ta(opt), °C, to one decimal place.
    pub celsius: f64,
    /// The lower of the two primers' melting temperatures, °C: the less stable duplex.
    pub primer_tm: f64,
    /// The product's melting temperature, °C, from the %GC formula.
    pub product_tm: f64,
    pub product_length: usize,
    /// The product's GC content, 0 to 100.
    pub product_gc: f64,
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// The melting temperature of a whole amplicon, °C: `81.5 + 16.6 log10[Na+] + 0.41 %GC − 675/L`
/// (Baldino et al. 1989, as used by Rychlik 1990), with `sodium_mm` in mM.
///
/// `None` for a product shorter than [`MIN_PRODUCT_LENGTH`], for a salt concentration that is not
/// a positive number, or for text holding no bases.
pub fn product_tm(product: &str, sodium_mm: f64) -> Option<f64> {
    let bases = clean_seq(product);
    let length = bases.len();
    if length < MIN_PRODUCT_LENGTH || !sodium_mm.is_finite() || sodium_mm <= 0.0 {
        return None;
    }
    let tm = 81.5 + 16.6 * (sodium_mm / 1000.0).log10() + 0.41 * gc_content(&bases) - 675.0 / length as f64;
    tm.is_finite().then_some(tm)
}

/// The temperature to anneal a pair at, by the rule in this module's documentation.
///
/// `forward_tm` and `reverse_tm` are the two primers' annealing temperatures in °C; the rule uses
/// the lower of them. `product` is the whole amplicon, 5' tails included.
///
/// `None` when either primer has no melting temperature (an oligo holding anything but A, C, G and
/// T has none), when the product is shorter than [`MIN_PRODUCT_LENGTH`], or when the arithmetic
/// does not come out finite.
pub fn annealing_temperature(forward_tm: f64, reverse_tm: f64, product: &str, sodium_mm: f64) -> Option<AnnealingTemperature> {
    if !forward_tm.is_finite() || !reverse_tm.is_finite() {
        return None;
    }
    let primer_tm = forward_tm.min(reverse_tm);
    let product_tm = product_tm(product, sodium_mm)?;
    let celsius = 0.3 * primer_tm + 0.7 * product_tm - 14.9;
    let bases = clean_seq(product);
    celsius.is_finite().then(|| AnnealingTemperature {
        celsius: round1(celsius),
        primer_tm: round1(primer_tm),
        product_tm: round1(product_tm),
        product_length: bases.len(),
        product_gc: round1(gc_content(&bases)),
    })
}
