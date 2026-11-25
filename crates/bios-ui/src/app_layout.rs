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

use egui::{CursorIcon, Rect, Sense, Stroke, pos2};

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

/// The narrowest either side of a split may be dragged: room for the map's corner tools and a
/// row of bases. A work column too narrow for two of these splits in half.
pub const SIDE_MIN: f32 = 360.0;

/// Where a split's divider may stand in a work column `width` wide, as a share from the left.
pub fn clamp_split(frac: f32, width: f32) -> f32 {
    let frac = if frac.is_finite() { frac } else { 0.5 };
    let least = if width > 2.0 * SIDE_MIN { SIDE_MIN / width } else { 0.5 };
    frac.clamp(least, 1.0 - least)
}

/// A work column cut in two side by side at `frac` (native only: Max asked to see two plasmids
/// at once, 2026-11-25). The divider is the right side's first point, a hairline; each side keeps
/// at least [`SIDE_MIN`].
pub fn sides(work: Rect, frac: f32) -> (Rect, Rect) {
    let x = (work.left() + work.width() * clamp_split(frac, work.width())).round();
    (Rect::from_min_max(work.min, pos2(x, work.bottom())), Rect::from_min_max(pos2(x, work.top()), work.max))
}

/// The divider between a split's sides, at the right side's left edge, `work` tall: drawn, and
/// dragged. Says the divider's new place as a share of `work`, and whether the drag ended;
/// a double-click puts it back in the middle.
pub fn split_divider(ui: &mut egui::Ui, work: Rect, right: Rect) -> Option<(f32, bool)> {
    let x = right.left() + 0.5;
    let painter = ui.painter().with_clip_rect(work);
    painter.line_segment([pos2(x, work.top()), pos2(x, work.bottom())], Stroke::new(1.0, theme::HAIRLINE_STRONG));
    // Clear of the head band, which moves the window and holds the tabs.
    let grip = Rect::from_min_max(pos2(x - 4.0, work.top() + theme::HEAD_H), pos2(x + 4.0, work.bottom()));
    let response = ui.interact(grip, ui.scope_id().with("split-divider"), Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
        painter.line_segment([pos2(x, work.top()), pos2(x, work.bottom())], Stroke::new(1.0, theme::ACCENT));
    }
    let frac = if response.double_clicked() {
        Some((0.5, true))
    } else {
        response
            .interact_pointer_pos()
            .filter(|_| response.dragged() || response.drag_stopped())
            .map(|at| (clamp_split((at.x - work.left()) / work.width().max(1.0), work.width()), response.drag_stopped()))
    };
    response.on_hover_text("Drag to resize · double-click to split in half");
    frac
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2};

    use super::{SIDE_MIN, clamp_nav, clamp_split, columns, sides};

    #[test]
    fn a_split_keeps_each_side_wide_enough_and_halves_a_narrow_column() {
        let work = Rect::from_min_max(pos2(200.0, 38.0), pos2(1440.0, 900.0));
        let (left, right) = sides(work, 0.5);
        assert_eq!((left.left(), left.right(), right.left(), right.right()), (200.0, 820.0, 820.0, 1440.0));
        let (left, right) = sides(work, 0.05);
        assert_eq!(left.width(), SIDE_MIN);
        assert!(right.width() > SIDE_MIN);
        let (_, right) = sides(work, 0.99);
        assert_eq!(right.width(), SIDE_MIN);
        assert_eq!(clamp_split(0.2, 600.0), 0.5, "too narrow for two: half each");
        assert_eq!(clamp_split(f32::NAN, 1240.0), 0.5);
    }

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
