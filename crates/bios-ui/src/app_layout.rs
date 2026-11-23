//! Where the window's columns go, ported from the web's `layout/AppLayout.tsx` (the `nav` and the
//! final `app-shell` return), `App.tsx` 160-301 (`.canvas-head` over `.app-main`) and
//! `styles.css` 372-840 (`.app-main`, `.nav-slot`, `.work-slot`, the dividers).
//!
//! The web pads `.app-main` by `--gap` and then pulls every column back out to the window's edges
//! with negative margins (chrome meets the window), and pulls the columns up into the 38px head
//! band. What is left is simple: the sidebar and the work column, edge to edge and full height,
//! with the tab strip standing in the head band at the top of the work column.
//!
//! The web's 60px places strip left of the panel is gone on purpose (Max, 2026-11-22): the places
//! are rows in the sidebar, which runs from the window's left edge (see `sidebar`).
//!
//! The work column has no left gutter because there is no assistant column: the web's
//! `.app-columns > .work-slot:first-child` rule (styles.css, "AND THE LEFT GUTTER GOES TOO WHEN
//! THERE IS NO CHAT") is the one that applies, as on the live site where the AI is off. The native
//! Changes column, which the web has no counterpart for, sits at the right edge when it is shown.

use egui::{Rect, pos2};

use crate::theme;

/// How wide the sidebar may be dragged (`layout/railWidth.ts`, `RAIL`).
pub const NAV_MIN: f32 = 168.0;
pub const NAV_MAX: f32 = 420.0;

/// The panel's width, within its bounds (`clampWidth`).
pub fn clamp_nav(width: f32) -> f32 {
    if width.is_finite() { width.round().clamp(NAV_MIN, NAV_MAX) } else { theme::NAV_FULL_W }
}

/// The window, cut into its columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    /// The sidebar, from the window's left edge; `None` while it is put away (width 0, as the web's `[data-hidden]`).
    pub nav: Option<Rect>,
    /// The tab strip, in the head band at the top of the work column; `None` with no tabs.
    pub tabs: Option<Rect>,
    /// The one cell: the work, under the tabs.
    pub cell: Rect,
    /// The native Changes column, when it is shown.
    pub changes: Option<Rect>,
}

/// Lays the columns out in `full`. `nav` is the panel's width when it is shown, `changes` the
/// Changes column's, and `tabs` whether there are tabs to stand in the head band.
pub fn columns(full: Rect, nav: Option<f32>, tabs: bool, changes: Option<f32>) -> Columns {
    let nav = nav.map(|w| Rect::from_min_max(full.min, pos2((full.left() + clamp_nav(w)).min(full.right()), full.bottom())));
    let left = nav.map_or(full.left(), |n| n.right());
    let changes = changes.map(|w| Rect::from_min_max(pos2((full.right() - w).max(left), full.top()), full.max));
    let right = changes.map_or(full.right(), |c| c.left());
    let work = Rect::from_min_max(pos2(left, full.top()), pos2(right.max(left), full.bottom()));
    let tabs = tabs.then(|| Rect::from_min_max(work.min, pos2(work.right(), (work.top() + theme::HEAD_H).min(work.bottom()))));
    let cell = Rect::from_min_max(pos2(work.left(), tabs.map_or(work.top(), |t| t.bottom())), work.max);
    Columns { nav, tabs, cell, changes }
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2};

    use super::{clamp_nav, columns};

    #[test]
    fn the_sidebar_and_the_work_run_edge_to_edge_from_the_windows_left() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1440.0, 900.0));
        let c = columns(full, Some(200.0), true, None);
        assert_eq!(c.nav, Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 900.0))));
        assert_eq!(c.tabs, Some(Rect::from_min_max(pos2(200.0, 0.0), pos2(1440.0, 38.0))));
        assert_eq!(c.cell, Rect::from_min_max(pos2(200.0, 38.0), pos2(1440.0, 900.0)));
        // The panel put away hands the work its whole width; no tabs, and the cell rises to the top.
        let away = columns(full, None, false, Some(300.0));
        assert_eq!(away.nav, None);
        assert_eq!(away.cell, Rect::from_min_max(pos2(0.0, 0.0), pos2(1140.0, 900.0)));
        assert_eq!(away.changes, Some(Rect::from_min_max(pos2(1140.0, 0.0), pos2(1440.0, 900.0))));
    }

    #[test]
    fn the_panel_is_dragged_only_between_168_and_420() {
        assert_eq!(clamp_nav(10.0), 168.0);
        assert_eq!(clamp_nav(1000.0), 420.0);
        assert_eq!(clamp_nav(233.4), 233.0);
        assert_eq!(clamp_nav(f32::NAN), crate::theme::NAV_FULL_W);
    }
}
