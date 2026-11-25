//! The recommended annealing temperature, against numbers worked out by hand from the one rule
//! the module names (Rychlik, Spencer & Rhoads 1990).
//!
//! Each number below was computed from the formula with a calculator before the code was run, so
//! a change in Bios that moves one of them is a change in the rule and has to be argued, not
//! accommodated.

use bios_core::annealing::{MIN_PRODUCT_LENGTH, annealing_temperature, product_tm};

/// 100 bases, exactly 50 G or C.
fn product() -> String {
    format!("{}{}", "GC".repeat(25), "AT".repeat(25))
}

/// `81.5 + 16.6 log10(0.05) + 0.41 × 50 − 675/100`
///   `= 81.5 − 21.597097927622088 + 20.5 − 6.75`
///   `= 73.65290207237791`
#[test]
fn the_product_tm_is_the_gc_formula_at_the_salt_it_is_given() {
    let tm = product_tm(&product(), 50.0).expect("a 100-base product has one");
    assert!((tm - 73.652_902_072_377_91).abs() < 1e-9, "{tm}");
    // Raising the salt raises it: 16.6 × log10(0.1) = −16.6, so 81.5 − 16.6 + 20.5 − 6.75.
    let salty = product_tm(&product(), 100.0).expect("at 100 mM too");
    assert!((salty - 78.65).abs() < 1e-9, "{salty}");
}

/// `0.3 × 60 + 0.7 × 73.65290207237791 − 14.9 = 18 + 51.55703145066454 − 14.9 = 54.65703145066454`
#[test]
fn the_annealing_temperature_takes_the_cooler_primer_and_the_product() {
    let a = annealing_temperature(60.0, 62.0, &product(), 50.0).expect("both primers melt");
    assert_eq!(a.celsius, 54.7, "54.65703… to one place");
    assert_eq!(a.primer_tm, 60.0, "the less stable of the two primer-template duplexes");
    assert_eq!(a.product_tm, 73.7);
    assert_eq!(a.product_length, 100);
    assert_eq!(a.product_gc, 50.0);
    // Which primer is named forward makes no difference: the rule takes the lower of the two.
    assert_eq!(annealing_temperature(62.0, 60.0, &product(), 50.0), Some(a));
}

#[test]
fn a_hotter_pair_on_the_same_product_anneals_hotter_by_three_tenths_of_the_difference() {
    let cool = annealing_temperature(60.0, 60.0, &product(), 50.0).expect("melts");
    let hot = annealing_temperature(70.0, 70.0, &product(), 50.0).expect("melts");
    // 0.3 × 10 °C of primer Tm: the product term does not move.
    assert!((hot.celsius - cool.celsius - 3.0).abs() < 1e-9, "{} vs {}", hot.celsius, cool.celsius);
}

#[test]
fn a_product_too_short_for_the_rule_gets_no_number_at_all() {
    let short = "ATGC".repeat(12); // 48 bases, under the 50-base floor
    assert_eq!(short.len(), MIN_PRODUCT_LENGTH - 2);
    assert_eq!(product_tm(&short, 50.0), None);
    assert_eq!(annealing_temperature(60.0, 60.0, &short, 50.0), None);
    // One base over the floor, and it answers.
    assert!(product_tm(&"ATGC".repeat(13), 50.0).is_some());
}

#[test]
fn nothing_that_is_not_a_number_comes_back_as_one() {
    assert_eq!(annealing_temperature(f64::NAN, 60.0, &product(), 50.0), None);
    assert_eq!(annealing_temperature(60.0, f64::INFINITY, &product(), 50.0), None);
    assert_eq!(product_tm(&product(), 0.0), None, "zero salt has no logarithm");
    assert_eq!(product_tm(&product(), -5.0), None);
}
