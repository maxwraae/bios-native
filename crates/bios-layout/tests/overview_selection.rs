//! Parity with Bios `editor/overview/selection.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.

use bios_layout::overview::selection::{ArcLike, DragSelection, FinishedAt, MapSelection, drag_selection, draws_selection, ring_step};

// The id lives in a `bios-id:` /note line, the same door the web app's identity.ts writes it through.
fn with_id(id: &str) -> Vec<String> {
    vec![format!("bios-id: {id}")]
}

/// `sel({})`: what every test's selection starts from.
const SEL: MapSelection<'static> = MapSelection { id: None, name: "EGFP", start: 100, end: 200 };

fn arc(start: i64, end: i64, name: Option<&'static str>) -> ArcLike<'static> {
    ArcLike { start, end, name, note: &[] }
}

/// identity wins when both sides carry one
#[test]
fn identity_wins_when_both_sides_carry_one() {
    let f1 = with_id("f1");
    let arc = ArcLike { start: 0, end: 999, name: Some("something else"), note: &f1 };
    assert!(draws_selection(&arc, &MapSelection { id: Some("f1"), ..SEL }));
    // Same span and name, different id — a different record, so it must not light.
    let f2 = with_id("f2");
    let other = ArcLike { start: 100, end: 200, name: Some("EGFP"), note: &f2 };
    assert!(!draws_selection(&other, &MapSelection { id: Some("f1"), ..SEL }));
}

/// a fused arc lights when any of its pieces is selected
#[test]
fn a_fused_arc_lights_when_any_of_its_pieces_is_selected() {
    // What coalesceFeatures produces: three abutting EGFP pieces drawn as one 2312..3031 arc.
    let fused = arc(2312, 3031, Some("EGFP"));
    for (start, end) in [(2312, 2314), (2315, 2317), (2318, 3031)] {
        assert!(draws_selection(&fused, &MapSelection { name: "EGFP", start, end, ..SEL }), "piece {start}..{end} should light the fused arc");
    }
}

/// a same-name feature elsewhere on the molecule stays dark
#[test]
fn a_same_name_feature_elsewhere_on_the_molecule_stays_dark() {
    // The two ITRs of an AAV vector are genuinely two things — coalesceFeatures keeps them apart and
    // so must this: selecting one must not light the other.
    let left = (10, 150);
    let right = (4000, 4140);
    assert!(!draws_selection(&arc(left.0, left.1, Some("ITR")), &MapSelection { name: "ITR", start: right.0, end: right.1, ..SEL }));
    assert!(!draws_selection(&arc(right.0, right.1, Some("ITR")), &MapSelection { name: "ITR", start: left.0, end: left.1, ..SEL }));
}

/// a different name never matches without an id
#[test]
fn a_different_name_never_matches_without_an_id() {
    assert!(!draws_selection(&arc(0, 999, Some("mCherry")), &MapSelection { name: "EGFP", ..SEL }));
}

/// an id on only one side falls back to name and span
#[test]
fn an_id_on_only_one_side_falls_back_to_name_and_span() {
    // A file that has never been through Bios has no ids at all; a half-annotated one has some.
    let f1 = with_id("f1");
    let with_note = ArcLike { start: 100, end: 200, name: Some("EGFP"), note: &f1 };
    assert!(draws_selection(&with_note, &MapSelection { id: None, ..SEL }));
    assert!(draws_selection(&arc(100, 200, Some("EGFP")), &MapSelection { id: Some("f1"), ..SEL }));
}

/// a span that wraps the origin needs an exact match
#[test]
fn a_span_that_wraps_the_origin_needs_an_exact_match() {
    // end < start means it crosses the seam; containment arithmetic would need L and would happily
    // swallow the whole molecule, so the wrapping case demands the exact same span.
    let wrapping = arc(4900, 120, Some("ori"));
    assert!(draws_selection(&wrapping, &MapSelection { name: "ori", start: 4900, end: 120, ..SEL }));
    assert!(!draws_selection(&wrapping, &MapSelection { name: "ori", start: 4950, end: 100, ..SEL }));
    // A non-wrapping arc must not claim a wrapping selection either.
    assert!(!draws_selection(&arc(0, 5000, Some("ori")), &MapSelection { name: "ori", start: 4900, end: 120, ..SEL }));
}

/// an unnamed arc matches an unnamed selection by span
#[test]
fn an_unnamed_arc_matches_an_unnamed_selection_by_span() {
    assert!(draws_selection(&arc(100, 200, None), &MapSelection { name: "", ..SEL }));
    assert!(!draws_selection(&arc(300, 400, None), &MapSelection { name: "", ..SEL }));
}

/// a drag right selects the bases between the two gaps, finishing at the end
#[test]
fn a_drag_right_selects_the_bases_between_the_two_gaps_finishing_at_the_end() {
    assert_eq!(drag_selection(10, 20, 100), DragSelection::Range { start: 10, end: 19, finished_at: FinishedAt::End });
}

/// a drag left selects the same bases, finishing at the start
#[test]
fn a_drag_left_selects_the_same_bases_finishing_at_the_start() {
    assert_eq!(drag_selection(20, 10, 100), DragSelection::Range { start: 10, end: 19, finished_at: FinishedAt::Start });
}

/// back on the anchor is a caret, not an empty range
#[test]
fn back_on_the_anchor_is_a_caret_not_an_empty_range() {
    assert_eq!(drag_selection(42, 42, 100), DragSelection::Caret(42));
}

/// a ring drag across the origin selects across it, wrapped
#[test]
fn a_ring_drag_across_the_origin_selects_across_it_wrapped() {
    // From gap 95, forward through the origin to gap 5 (unwrapped 105).
    assert_eq!(drag_selection(95, 105, 100), DragSelection::Range { start: 95, end: 4, finished_at: FinishedAt::End });
    // From gap 5, backward through the origin to gap 95 (unwrapped -5).
    assert_eq!(drag_selection(5, -5, 100), DragSelection::Range { start: 95, end: 4, finished_at: FinishedAt::Start });
}

/// a full turn or more is the whole molecule
#[test]
fn a_full_turn_or_more_is_the_whole_molecule() {
    assert_eq!(drag_selection(30, 130, 100), DragSelection::Range { start: 0, end: 99, finished_at: FinishedAt::End });
    assert_eq!(drag_selection(30, 180, 100), DragSelection::Range { start: 0, end: 99, finished_at: FinishedAt::End });
}

/// a caret off the molecule's ends is read round the ring
#[test]
fn a_caret_off_the_molecules_ends_is_read_round_the_ring() {
    assert_eq!(drag_selection(-3, -3, 100), DragSelection::Caret(97));
}

/// ringStep carries a step through the origin instead of across the ring
#[test]
fn ring_step_carries_a_step_through_the_origin_instead_of_across_the_ring() {
    assert_eq!(ring_step(98, 98, 2, 100), 102);
    assert_eq!(ring_step(2, 2, 98, 100), -2);
    assert_eq!(ring_step(50, 50, 60, 100), 60);
    assert_eq!(ring_step(150, 50, 40, 100), 140);
}
