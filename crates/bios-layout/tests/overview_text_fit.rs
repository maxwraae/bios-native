//! Bios `editor/overview/textFit.ts` has no test file of its own. These check the behaviour its
//! comments promise: the estimate when nothing can measure, the padding before the font is in, the
//! cache, and what the font arriving changes.

use std::cell::Cell;

use bios_layout::overview::labels::fit_label;
use bios_layout::overview::text_fit::{CACHE_MAX, PRE_LOAD_SAFETY, TEXT_MEASURE_FALLBACK, TextRuler, estimate_ui};

/// A face 6px wide per character at 10px.
fn face(text: &str, font_px: f64) -> f64 {
    text.chars().count() as f64 * font_px * 0.6
}

#[test]
fn an_empty_name_has_no_width() {
    assert_eq!(TextRuler::without_fonts().measure_ui("", 11.0), 0.0);
    let asked = Cell::new(0);
    let mut ruler = TextRuler::new(|text: &str, font_px: f64| {
        asked.set(asked.get() + 1);
        face(text, font_px)
    });
    assert_eq!(ruler.measure_ui("", 11.0), 0.0);
    assert_eq!(asked.get(), 0, "nothing is measured for an empty name");
}

#[test]
fn with_nothing_to_measure_with_the_width_is_an_estimate_per_character() {
    assert_eq!(TEXT_MEASURE_FALLBACK, 0.55);
    assert_eq!(estimate_ui("EGFP", 10.0), 4.0 * 10.0 * 0.55);
    let mut ruler = TextRuler::without_fonts();
    assert_eq!(ruler.measure_ui("EGFP", 10.0), 4.0 * 10.0 * 0.55);
    assert_eq!(ruler.measure_ui("CASSETTE 1", 11.0), estimate_ui("CASSETTE 1", 11.0));
}

#[test]
fn the_estimate_is_not_padded_and_does_not_change_when_fonts_arrive() {
    let mut ruler = TextRuler::without_fonts();
    let before = ruler.measure_ui("AmpR promoter", 11.0);
    let ran = Cell::new(false);
    ruler.on_fonts_ready(|| ran.set(true));
    assert!(!ran.get(), "there is nothing to lay out again");
    assert_eq!(ruler.font_generation(), 0);
    assert_eq!(ruler.measure_ui("AmpR promoter", 11.0), before);
}

#[test]
fn a_width_measured_before_the_font_is_in_is_padded() {
    assert_eq!(PRE_LOAD_SAFETY, 1.06);
    let mut ruler = TextRuler::new(face);
    assert_eq!(ruler.measure_ui("EGFP", 10.0), face("EGFP", 10.0) * 1.06);
    assert!(ruler.measure_ui("EGFP", 10.0) > face("EGFP", 10.0), "erring high, never low");
}

#[test]
fn once_the_font_is_in_the_width_is_the_true_one() {
    let mut ruler = TextRuler::new(face);
    let padded = ruler.measure_ui("EGFP", 10.0);
    ruler.on_fonts_ready(|| {});
    assert_eq!(ruler.measure_ui("EGFP", 10.0), face("EGFP", 10.0), "the padded width was dropped, not served from the cache");
    assert!(ruler.measure_ui("EGFP", 10.0) < padded);
}

#[test]
fn the_font_arriving_runs_the_callback_and_bumps_the_generation() {
    let mut ruler = TextRuler::new(face);
    assert_eq!(ruler.font_generation(), 0);
    let ran = Cell::new(0);
    ruler.on_fonts_ready(|| ran.set(ran.get() + 1));
    assert_eq!(ran.get(), 1);
    assert_eq!(ruler.font_generation(), 1);
}

#[test]
fn a_width_is_measured_once_and_then_remembered() {
    let asked = Cell::new(0);
    let mut ruler = TextRuler::new(|text: &str, font_px: f64| {
        asked.set(asked.get() + 1);
        face(text, font_px)
    });
    let first = ruler.measure_ui("WPRE", 11.0);
    assert_eq!(ruler.measure_ui("WPRE", 11.0), first);
    assert_eq!(ruler.measure_ui("WPRE", 11.0), first);
    assert_eq!(asked.get(), 1);
}

#[test]
fn the_same_name_at_another_size_is_another_measurement() {
    let asked = Cell::new(0);
    let mut ruler = TextRuler::new(|text: &str, font_px: f64| {
        asked.set(asked.get() + 1);
        face(text, font_px)
    });
    let small = ruler.measure_ui("WPRE", 8.5);
    let large = ruler.measure_ui("WPRE", 11.0);
    assert!(large > small);
    assert_eq!(asked.get(), 2);
    assert_eq!(ruler.measure_ui("WPRE", 8.5), small);
    assert_eq!(asked.get(), 2);
}

#[test]
fn a_full_cache_is_emptied_wholesale_and_starts_again() {
    let asked = Cell::new(0_usize);
    let mut ruler = TextRuler::new(|text: &str, font_px: f64| {
        asked.set(asked.get() + 1);
        face(text, font_px)
    });
    for i in 0..CACHE_MAX {
        ruler.measure_ui(&format!("name {i}"), 11.0);
    }
    assert_eq!(asked.get(), CACHE_MAX);
    ruler.measure_ui("name 0", 11.0);
    assert_eq!(asked.get(), CACHE_MAX, "still remembered while the cache is merely full");
    // One more new name finds the cache full, which empties it before the name goes in.
    ruler.measure_ui("one too many", 11.0);
    ruler.measure_ui("name 0", 11.0);
    assert_eq!(asked.get(), CACHE_MAX + 2, "the first name had to be measured again");
    ruler.measure_ui("one too many", 11.0);
    assert_eq!(asked.get(), CACHE_MAX + 2, "the name that triggered the emptying was kept");
}

#[test]
fn the_ruler_is_what_fit_label_measures_with() {
    let mut ruler = TextRuler::new(face);
    ruler.on_fonts_ready(|| {});
    // 6px per character at 10px: room for eight characters.
    let shown = fit_label("chicken β-actin promoter", 48.0, 10.0, |text, font_px| ruler.measure_ui(text, font_px));
    assert_eq!(shown.as_deref(), Some("chicke.."));
    assert!(ruler.measure_ui("chicke..", 10.0) <= 48.0);
}
