//! Drawing a Lucide icon, the way `lucide-react` renders one: its shapes in a 24×24 box, stroked
//! with `currentColor` at `strokeWidth` 2 (scaled with the icon), round caps and joins, no fill.
//! The icons themselves are in [`crate::lucide`].

use egui::epaint::PathStroke;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};
use kurbo::{BezPath, Circle, Ellipse, PathEl, RoundedRect, Shape as _};

/// One element of an icon, as Lucide lists it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node {
    Path(&'static str),
    Circle { cx: f64, cy: f64, r: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Rect { x: f64, y: f64, w: f64, h: f64, rx: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Polyline(&'static str),
    Polygon(&'static str),
}

/// Lucide's own default stroke, in the 24-unit box.
pub const STROKE: f32 = 2.0;

/// How finely curves are cut into segments, in box units. A 15px icon is drawn at 2x, so a box
/// unit is about 1.25 pixels; this keeps every segment under a tenth of a pixel off the curve.
const TOLERANCE: f64 = 0.05;

fn points(list: &str) -> Vec<(f64, f64)> {
    let numbers: Vec<f64> = list.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    numbers.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect()
}

fn poly(list: &str, closed: bool) -> BezPath {
    let mut path = BezPath::new();
    for (i, (x, y)) in points(list).into_iter().enumerate() {
        if i == 0 { path.move_to((x, y)) } else { path.line_to((x, y)) }
    }
    if closed {
        path.close_path();
    }
    path
}

fn bez(node: &Node) -> BezPath {
    match *node {
        Node::Path(d) => BezPath::from_svg(d).unwrap_or_default(),
        Node::Circle { cx, cy, r } => Circle::new((cx, cy), r).to_path(TOLERANCE),
        Node::Ellipse { cx, cy, rx, ry } => Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(TOLERANCE),
        Node::Rect { x, y, w, h, rx } => RoundedRect::new(x, y, x + w, y + h, rx).to_path(TOLERANCE),
        Node::Line { x1, y1, x2, y2 } => {
            let mut path = BezPath::new();
            path.move_to((x1, y1));
            path.line_to((x2, y2));
            path
        }
        Node::Polyline(list) => poly(list, false),
        Node::Polygon(list) => poly(list, true),
    }
}

/// An icon's outlines: polylines in box units, each with whether it closes on itself.
pub type Outlines = Vec<(Vec<(f64, f64)>, bool)>;

/// Icons already cut, by where their list sits, each with the list it was cut from.
type Cut = std::collections::HashMap<usize, Vec<(Vec<Node>, std::rc::Rc<Outlines>)>>;

thread_local! {
    /// Outlines already cut, by the icon they were cut from. A sidebar draws hundreds of icons a
    /// frame, and cutting curves into segments each time was most of what a frame cost.
    static CUT: std::cell::RefCell<Cut> = Default::default();
}

/// [`outlines`], cut once per icon. Icons are static lists, found again by where they sit; the
/// list itself is compared too, so one never stands in for another.
fn outlines_cached(nodes: &[Node]) -> std::rc::Rc<Outlines> {
    let key = nodes.as_ptr() as usize;
    if let Some(found) = CUT.with_borrow(|cut| cut.get(&key).and_then(|v| v.iter().find(|(n, _)| n.as_slice() == nodes).map(|(_, o)| o.clone()))) {
        return found;
    }
    let made = std::rc::Rc::new(outlines(nodes));
    CUT.with_borrow_mut(|cut| cut.entry(key).or_default().push((nodes.to_vec(), made.clone())));
    made
}

/// The icon's outlines as polylines in box units, each with whether it closes on itself.
pub fn outlines(nodes: &[Node]) -> Outlines {
    let mut out = Vec::new();
    for node in nodes {
        let mut current: Vec<(f64, f64)> = Vec::new();
        let mut closed = false;
        let flush = |current: &mut Vec<(f64, f64)>, closed: &mut bool, out: &mut Vec<(Vec<(f64, f64)>, bool)>| {
            if current.len() > 1 {
                out.push((std::mem::take(current), *closed));
            }
            current.clear();
            *closed = false;
        };
        kurbo::flatten(bez(node), TOLERANCE, |el| match el {
            PathEl::MoveTo(p) => {
                flush(&mut current, &mut closed, &mut out);
                current.push((p.x, p.y));
            }
            PathEl::LineTo(p) => current.push((p.x, p.y)),
            PathEl::ClosePath => {
                closed = true;
                flush(&mut current, &mut closed, &mut out);
            }
            _ => {}
        });
        flush(&mut current, &mut closed, &mut out);
    }
    out
}

/// Paints `nodes` into a `size`-point square centred on `center`, in `color`, at Lucide's
/// `strokeWidth` `stroke` (2 unless the web passes another).
pub fn paint_with(painter: &Painter, center: Pos2, size: f32, nodes: &[Node], color: Color32, stroke: f32) {
    let scale = size / 24.0;
    let origin = center - egui::vec2(size / 2.0, size / 2.0);
    let at = |(x, y): (f64, f64)| pos2(origin.x + x as f32 * scale, origin.y + y as f32 * scale);
    let width = stroke * scale;
    for (line, closed) in outlines_cached(nodes).iter().map(|(l, c)| (l, *c)) {
        let pts: Vec<Pos2> = line.iter().copied().map(at).collect();
        // Round caps: egui ends a stroke square at the point, so an open line gets a dot of the
        // stroke's width at each end, which is what `stroke-linecap: round` draws.
        if !closed {
            for end in [pts.first(), pts.last()].into_iter().flatten() {
                painter.circle_filled(*end, width / 2.0, color);
            }
        }
        let shape = if closed { Shape::closed_line(pts, PathStroke::new(width, color)) } else { Shape::line(pts, PathStroke::new(width, color)) };
        painter.add(shape);
    }
}

/// Paints an icon at Lucide's default stroke.
pub fn paint(painter: &Painter, center: Pos2, size: f32, nodes: &[Node], color: Color32) {
    paint_with(painter, center, size, nodes, color, STROKE);
}

/// The square an icon of `size` occupies around `center`.
pub fn rect(center: Pos2, size: f32) -> Rect {
    Rect::from_center_size(center, egui::vec2(size, size))
}

/// A stroke of the icon's width, for things drawn beside an icon that should match it.
pub fn stroke(size: f32, color: Color32) -> Stroke {
    Stroke::new(STROKE * size / 24.0, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_to_lines_inside_its_box() {
        for (name, nodes) in crate::lucide::ALL {
            let lines = outlines(nodes);
            assert!(!lines.is_empty(), "{name} drew nothing");
            for (line, _) in &lines {
                for &(x, y) in line {
                    assert!((-0.5..=24.5).contains(&x) && (-0.5..=24.5).contains(&y), "{name} leaves its box at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn a_circle_closes_and_a_line_does_not() {
        let circle = outlines(&[Node::Circle { cx: 12.0, cy: 12.0, r: 10.0 }]);
        assert_eq!(circle.len(), 1);
        assert!(circle[0].1);
        let line = outlines(crate::lucide::MINUS);
        assert_eq!(line.len(), 1);
        assert!(!line[0].1);
        assert_eq!(line[0].0, vec![(5.0, 12.0), (19.0, 12.0)]);
    }
}
