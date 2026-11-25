//! The viewing controls, in the map's top right corner: how the molecule is drawn, and what is
//! drawn on it.
//!
//! Native only (Max, 2026-11-25). The web app keeps every control in the floating row at the foot
//! of the editor pane. Here the controls that change what you SEE sit in the corner of the thing
//! they change, and the foot keeps the ones that change what you EDIT: Back and Forward, the
//! enzymes to cut with, and the contents. They show only while a map is on screen, which is why
//! the view trio (map, split, sequence) stays at the foot — it is how you get the map back.

use bios_layout::pane::MapStyle;
use egui::{Pos2, Rect, Ui, pos2, vec2};

use crate::lucide;
use crate::primitives::{Face, Material, Segment, group, group_width};

/// How far the control stands in from the map's top right corner. The same air the plasmid's
/// name has in the opposite corner.
const INSET: f32 = 16.0;

/// The icon size every button draws at, as in the row at the foot.
const ICON: f32 = 15.0;

/// Between the style group and the View button.
const GAP: f32 = 6.0;

/// What the corner shows.
#[derive(Clone, Debug)]
pub struct MapTools {
    pub style: MapStyle,
    /// Why Fold cannot run, which greys it and is its tooltip.
    pub fold_reason: Option<String>,
    /// The View popover is open, which lights its button.
    pub view_open: bool,
}

/// What a press in the corner asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Style(MapStyle),
    /// Open or shut the popover of what is drawn on the map.
    View,
}

/// Where the corner put things, for a popover to hang from.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub view: Rect,
    /// Everything drawn, for a press test.
    pub row: Rect,
}

/// Draws the controls in the top right of `map`. `map` is the map's own band, so they ride with
/// it when the split moves.
pub fn show(ui: &mut Ui, map: Rect, tools: &MapTools) -> (Option<Action>, Placed) {
    let fold_tip = tools.fold_reason.clone().unwrap_or_else(|| "Fold — secondary structure".to_string());
    let styles = [
        Segment { face: Face::Icon(lucide::CIRCLE, ICON), active: tools.style == MapStyle::Circular, enabled: true, tip: "Circular map" },
        Segment { face: Face::Icon(lucide::MINUS, ICON), active: tools.style == MapStyle::Linear, enabled: true, tip: "Linear map" },
        Segment {
            face: Face::Icon(lucide::SPLINE, ICON),
            active: tools.style == MapStyle::Fold,
            enabled: tools.fold_reason.is_none(),
            tip: &fold_tip,
        },
    ];
    let view =
        [Segment { face: Face::Icon(lucide::SLIDERS_HORIZONTAL, ICON), active: tools.view_open, enabled: true, tip: "What's shown on the map" }];
    let (styles_w, view_w) = (group_width(ui, &styles, true), group_width(ui, &view, true));
    let total = styles_w + GAP + view_w;
    let top = (map.top() + INSET).round();
    let left = (map.right() - INSET - total).round();
    let row = Rect::from_min_size(pos2(left, top), vec2(total, HEIGHT));
    let mut action = None;
    if let Some(i) = group(ui, pos2(left, top), &styles, Material::Float, true) {
        action = [MapStyle::Circular, MapStyle::Linear, MapStyle::Fold].get(i).map(|s| Action::Style(*s));
    }
    let view_at = pos2(left + styles_w + GAP, top);
    if group(ui, view_at, &view, Material::Float, true).is_some() {
        action = Some(Action::View);
    }
    (action, Placed { view: button(view_at), row })
}

/// A group stands 34 high: a 1px ring, 2px of padding, and 30-high segments.
const HEIGHT: f32 = 34.0;

/// The one segment of a group placed at `at`.
fn button(at: Pos2) -> Rect {
    Rect::from_min_size(pos2(at.x + 3.0, at.y + 2.0), vec2(30.0, 30.0))
}

/// Where the View popover hangs when there is no map to hang it from (the sequence on its own,
/// reached by `--panel layers`): the same corner the controls would have stood in.
pub fn view_anchor_without_map(pane: Rect) -> Rect {
    button(pos2(pane.right() - INSET - 36.0, pane.top() + INSET))
}

// The air these need at the top of the map is kept by `line_map::TITLE_CLEAR`, which holds the
// drawing clear of both corners' furniture.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_controls_stand_in_from_the_maps_top_right_corner() {
        let map = Rect::from_min_size(pos2(100.0, 50.0), vec2(600.0, 400.0));
        let row = Rect::from_min_size(pos2(map.right() - INSET - 150.0, map.top() + INSET), vec2(150.0, HEIGHT));
        assert_eq!(row.right(), map.right() - INSET);
        assert_eq!(row.top(), map.top() + INSET);
        // The View button is the last thing in it, so the popover hangs from the corner.
        assert!(button(pos2(row.right() - 36.0, row.top())).right() <= row.right());
    }
}
