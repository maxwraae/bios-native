//! What kind of thing a row is, drawn the same way everywhere it is named. Ported from the web's
//! `layout/kindIcons.tsx` (whole file): a plasmid is a ring with the origin tick, a linear fragment
//! a rule with its two ends, and a folder is a folder. Hand-drawn there rather than Lucide, in the
//! same 24-unit box, stroked at 1.8.

use egui::{Color32, Painter, Pos2};

use crate::icon::{self, Node};

/// The web's `strokeWidth: 1.8`.
pub const STROKE: f32 = 1.8;

/// `PlasmidMark`.
pub const PLASMID: &[Node] = &[Node::Circle { cx: 12.0, cy: 12.0, r: 7.5 }, Node::Path("M12 4.5v3")];

/// `LinearMark`.
pub const LINEAR: &[Node] = &[Node::Path("M3.5 12h17M3.5 9v6M20.5 9v6")];

/// `FolderMark` for a folder of no particular kind (one you made yourself).
pub const FOLDER: &[Node] = &[Node::Path("M3.5 7a2 2 0 0 1 2-2h3.6l2 2.2h7.4a2 2 0 0 1 2 2v8.3a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z")];

/// Paints a kind mark `size` points across.
pub fn paint(painter: &Painter, center: Pos2, size: f32, nodes: &[Node], color: Color32) {
    icon::paint_with(painter, center, size, nodes, color, STROKE);
}

/// The plasmid mark filled, as an open row draws it (`.rail-file-row.is-open .rail-file-mark svg
/// { fill: currentcolor }`): the ring becomes a disc and the tick vanishes into it.
pub fn paint_plasmid_filled(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let scale = size / 24.0;
    painter.circle_filled(center, (7.5 + STROKE / 2.0) * scale, color);
}
