//! What Fold would fold, or why it cannot. Ported from Bios `editor/fold/target.ts`, with its tests
//! from `target.test.ts` in their order. The map-style switch greys Fold and shows the reason as
//! its tooltip (`editor/MapControls.tsx`).

use crate::overview::scene::group_thousands;

/// The longest stretch the fold engine takes (`@bios/shared` `FOLD_MAX_NT`).
pub const FOLD_MAX_NT: i64 = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldTarget {
    /// The stretch to fold, 0-based inclusive, or `None` when nothing can be folded.
    pub window: Option<(i64, i64)>,
    /// Why not, phrased for a tooltip. `None` exactly when `window` is set.
    pub reason: Option<String>,
}

fn refuse(reason: String) -> FoldTarget {
    FoldTarget { window: None, reason: Some(reason) }
}

fn n(v: i64) -> String {
    group_thousands(v as f64)
}

pub fn fold_target(sequence_length: i64, circular: bool, selection: Option<(i64, i64)>) -> FoldTarget {
    if sequence_length <= 0 {
        return refuse("Nothing open to fold.".into());
    }
    if let Some((start, end)) = selection {
        if end < start {
            return refuse("That selection wraps the origin. Select a stretch that does not cross it.".into());
        }
        let start = 0.max(start.min(sequence_length - 1));
        let end = 0.max(end.min(sequence_length - 1));
        let len = end - start + 1;
        if len > FOLD_MAX_NT {
            return refuse(format!("Selection is {} nt. Fold needs {} or fewer.", n(len), n(FOLD_MAX_NT)));
        }
        if len < 2 {
            return refuse("Select at least two bases to fold.".into());
        }
        return FoldTarget { window: Some((start, end)), reason: None };
    }
    if circular {
        return refuse(format!("A circular plasmid has no single-strand fold. Select a region under {} nt.", n(FOLD_MAX_NT)));
    }
    if sequence_length > FOLD_MAX_NT {
        return refuse(format!("Too long to fold ({} nt). Select a region under {} nt.", n(sequence_length), n(FOLD_MAX_NT)));
    }
    FoldTarget { window: Some((0, sequence_length - 1)), reason: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(t: &FoldTarget) -> String {
        t.reason.clone().unwrap_or_default().to_lowercase()
    }

    #[test]
    fn a_short_linear_construct_folds_whole() {
        let t = fold_target(300, false, None);
        assert_eq!(t.window, Some((0, 299)));
        assert_eq!(t.reason, None);
    }

    #[test]
    fn a_plasmid_never_folds_whole_however_short() {
        let t = fold_target(500, true, None);
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("circular"));
    }

    #[test]
    fn a_selection_inside_a_plasmid_is_foldable() {
        let t = fold_target(5000, true, Some((900, 1199)));
        assert_eq!(t.window, Some((900, 1199)));
        assert_eq!(t.reason, None);
    }

    #[test]
    fn a_long_construct_with_no_selection_is_refused_by_length() {
        let t = fold_target(4812, false, None);
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("4,812"));
        assert!(t.reason.unwrap_or_default().contains("Select a region"));
    }

    #[test]
    fn a_selection_past_the_cap_is_refused_and_says_how_big_it_was() {
        let t = fold_target(5000, false, Some((0, FOLD_MAX_NT)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("1,001 nt"));
    }

    #[test]
    fn exactly_the_cap_is_allowed() {
        let t = fold_target(5000, true, Some((0, FOLD_MAX_NT - 1)));
        assert_eq!(t.window, Some((0, FOLD_MAX_NT - 1)));
        assert_eq!(t.reason, None);
    }

    #[test]
    fn an_origin_wrapping_selection_is_refused() {
        let t = fold_target(5000, true, Some((4900, 100)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("origin"));
    }

    #[test]
    fn a_caret_is_not_something_to_fold() {
        let t = fold_target(300, false, Some((42, 42)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("at least two"));
    }

    #[test]
    fn an_empty_construct_refuses_without_crashing() {
        assert_eq!(fold_target(0, false, None).window, None);
    }

    #[test]
    fn a_selection_that_now_runs_past_the_end_is_clamped() {
        let t = fold_target(610, true, Some((600, 620)));
        assert_eq!(t.window, Some((600, 609)));
        assert_eq!(t.reason, None);
    }

    #[test]
    fn clamping_still_refuses_when_what_is_left_is_a_caret() {
        let t = fold_target(601, true, Some((600, 620)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("at least two"));
    }

    #[test]
    fn a_selection_entirely_past_the_end_collapses_and_is_refused() {
        let t = fold_target(100, true, Some((600, 620)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("at least two"));
    }

    #[test]
    fn the_origin_wrap_refusal_is_decided_before_the_clamp() {
        let t = fold_target(200, true, Some((4900, 100)));
        assert_eq!(t.window, None);
        assert!(reason(&t).contains("origin"));
    }

    #[test]
    fn a_selection_that_still_fits_is_untouched() {
        let t = fold_target(5000, true, Some((900, 1199)));
        assert_eq!(t.window, Some((900, 1199)));
    }

    #[test]
    fn every_refusal_carries_a_reason_and_every_success_none() {
        let cases = [(300, false, None), (5000, true, None), (0, false, None), (5000, true, Some((900, 1199))), (5000, false, Some((0, 2000)))];
        for (len, circ, sel) in cases {
            let t = fold_target(len, circ, sel);
            assert_eq!(t.window.is_none(), t.reason.is_some(), "mismatch for {len}/{circ}/{sel:?}");
        }
    }
}
