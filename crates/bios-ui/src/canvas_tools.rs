//! The floating tool row at the foot of the editor pane, `.canvas-tools`: Back and Forward, the
//! view, and the panels, opaque pills centred over whatever is drawn, with a scrim under them.
//!
//! Native keeps only what changes what you EDIT down here (Max, 2026-11-25). The map's style and
//! the layers moved to the map's own top right corner (`map_tools`), because they change what you
//! SEE. The view trio stays: it is how the map comes back when the sequence has the pane. Ported from Bios `editor/HistoryControls.tsx` (60-123), `editor/MapControls.tsx`
//! (139-207) and `styles.css` (`.canvas-tools`, its `::before`, the `--tools-floor` rule and the
//! segment rules, lines 5659-5826; the greyed arrows, 9866-9882).
//!
//! The web's tooltips are its own ink pill after two seconds; here they are egui's.

use bios_layout::pane::{Focus, TOOLS_AIR, TOOLS_H};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Pos2, Rect, Shape, Ui, pos2, vec2};

use crate::lucide;
use crate::primitives::{Face, Material, Segment, group, group_width};
use crate::side_panel::Side;
use crate::theme;

/// The side panels a button in the row opens. Only ever one is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// "What's shown on the map": the layer toggles. Its button is in the map's corner
    /// (`map_tools`), not in this row; it stays a panel so `--panel layers` can open it.
    Layers,
    /// The scissors: which cut sites are drawn.
    Enzymes,
    /// "Features & annotations": the Contents side panel.
    Contents,
}

/// What a press in the row asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Back,
    Forward,
    Focus(Focus),
    Panel(Panel),
}

/// What the row shows.
#[derive(Clone, Debug)]
pub struct Tools {
    pub can_undo: bool,
    pub can_redo: bool,
    pub focus: Focus,
    /// The popover whose button is lit.
    pub open: Option<Panel>,
    /// The side panel open on the pane's right, which lights its button too.
    pub side: Option<crate::side_panel::Side>,
}

/// The icon size every button in the row draws at (`size={15}`).
const ICON: f32 = 15.0;
/// Between the history group and the map's controls, and between the map's own groups.
const GAP_OUTER: f32 = 14.0;
const GAP_INNER: f32 = 6.0;

/// The scrim's run-up above the row, where it fades out.
const SCRIM_RUN: f32 = 72.0;

/// Where the row put the panels' buttons, for a popover to hang from.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub enzymes: Rect,
    pub contents: Rect,
    /// The whole row.
    pub row: Rect,
}

/// Draws the row in `pane`, standing with its bottom at `bottom` (the floor less `--tools-air`).
pub fn show(ui: &mut Ui, pane: Rect, bottom: f32, tools: &Tools) -> (Option<Action>, Placed) {
    scrim(ui, pane, bottom);
    let undo_tip = "Back (⌘Z)";
    let redo_tip = "Forward (⌘⇧Z)";
    let history = [
        Segment { face: Face::Icon(lucide::UNDO_2, ICON), active: false, enabled: tools.can_undo, tip: undo_tip },
        Segment { face: Face::Icon(lucide::REDO_2, ICON), active: false, enabled: tools.can_redo, tip: redo_tip },
    ];
    let views = [
        Segment { face: Face::Icon(lucide::SQUARE, ICON), active: tools.focus == Focus::Map, enabled: true, tip: "Map only" },
        Segment { face: Face::Icon(lucide::ROWS_2, ICON), active: tools.focus == Focus::Split, enabled: true, tip: "Map and sequence" },
        Segment { face: Face::Icon(lucide::ALIGN_JUSTIFY, ICON), active: tools.focus == Focus::Sequence, enabled: true, tip: "Sequence only" },
    ];
    let panels = [
        Segment {
            face: Face::Icon(lucide::SCISSORS, ICON),
            active: tools.open == Some(Panel::Enzymes) || tools.side == Some(Side::Enzymes),
            enabled: true,
            tip: "Restriction enzymes",
        },
        Segment { face: Face::Icon(lucide::TAGS, ICON), active: tools.side == Some(Side::Contents), enabled: true, tip: "Features & annotations" },
    ];
    let widths = [group_width(ui, &history, false), group_width(ui, &views, true), group_width(ui, &panels, true)];
    let total: f32 = widths.iter().sum::<f32>() + GAP_OUTER + GAP_INNER;
    let top = bottom - TOOLS_H as f32;
    let mut x = (pane.center().x - total / 2.0).round();
    let row = Rect::from_min_size(pos2(x, top), vec2(total, TOOLS_H as f32));
    let mut action = None;
    if let Some(i) = group(ui, pos2(x, top), &history, Material::Float, false) {
        action = Some(if i == 0 { Action::Back } else { Action::Forward });
    }
    x += widths[0] + GAP_OUTER;
    if let Some(i) = group(ui, pos2(x, top), &views, Material::Float, true) {
        action = [Focus::Map, Focus::Split, Focus::Sequence].get(i).map(|f| Action::Focus(*f));
    }
    x += widths[1] + GAP_INNER;
    if let Some(i) = group(ui, pos2(x, top), &panels, Material::Float, true) {
        action = [Panel::Enzymes, Panel::Contents].get(i).map(|p| Action::Panel(*p));
    }
    // 1px ring + 2px padding, then 30-wide segments 1px apart.
    let seg = |i: f32| Rect::from_min_size(pos2(x + 3.0 + i * 31.0, top + 2.0), vec2(30.0, 30.0));
    (action, Placed { enzymes: seg(0.0), contents: seg(1.0), row })
}

/// The fade under the row (`.canvas-tools::before`): the paper coming back up under the buttons. A
/// `--cell` gradient from full at the floor to 55% at the top, under a mask that holds 92% for the
/// bottom third and fades to nothing. The web also blurs what is behind by 7px; that is not drawn.
fn scrim(ui: &Ui, pane: Rect, bottom: f32) {
    let floor = bottom + TOOLS_AIR as f32;
    let height = TOOLS_H as f32 + TOOLS_AIR as f32 + SCRIM_RUN;
    let painter = ui.painter().with_clip_rect(pane);
    let mut mesh = Mesh::default();
    let steps = 24;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let y = floor - t * height;
        let ground = 1.0 - 0.45 * t;
        let mask = if t <= 0.34 { 0.92 } else { 0.92 * (1.0 - (t - 0.34) / 0.66) };
        let color = theme::alpha(theme::CELL, ground * mask);
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex { pos: Pos2::new(pane.left(), y), uv: WHITE_UV, color });
        mesh.vertices.push(Vertex { pos: Pos2::new(pane.right(), y), uv: WHITE_UV, color });
        if k > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base + 1, base);
        }
    }
    painter.add(Shape::mesh(mesh));
}
