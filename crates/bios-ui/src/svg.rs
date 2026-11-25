//! What a frame of the interface drew, written out as SVG.
//!
//! Bios draws a construct once, in one place: the map's painter and the rows' painter. A picture
//! file must be the same drawing, or an agent would be looking at something the person never sees.
//! So nothing here knows about plasmids: it takes the shapes a frame produced and writes each one
//! out, which means the map and the rows need no second renderer and a change to either shows up
//! in the picture by itself.
//!
//! Text keeps the positions the interface measured with its own fonts, letter by letter, so a name
//! sits exactly where the window puts it. The letters themselves are drawn by whatever reads the
//! file, from the families named in [`font_family`]: a reader with Instrument Sans draws what the
//! window draws, and one without it draws the same words in its nearest face.

use egui::epaint::{CubicBezierShape, Mesh, PathShape, QuadraticBezierShape, RectShape, TextShape};
use egui::{Color32, CornerRadius, FontFamily, Pos2, Rect, Shape, Stroke, StrokeKind};

/// An SVG document `width` by `height` of everything in `shapes`.
///
/// `square` centres the drawing in a square document instead, for macOS's QuickLook, which fills a
/// square and crops whatever overhangs it. The drawing is clipped to its own rectangle either way,
/// so both documents hold exactly the same picture.
pub fn document(shapes: &[Shape], width: f64, height: f64, square: bool) -> String {
    let (w, h) = (width.max(1.0), height.max(1.0));
    let side = if square { w.max(h) } else { 0.0 };
    let (page_w, page_h) = if square { (side, side) } else { (w, h) };
    let mut out = String::with_capacity(4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        num(page_w),
        num(page_h),
        num(page_w),
        num(page_h)
    ));
    out.push_str(&format!(
        "<defs><clipPath id=\"bios-picture\"><rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/></clipPath></defs>\n",
        num(w),
        num(h)
    ));
    if square {
        out.push_str(&format!("<g transform=\"translate({} {})\">", num((side - w) / 2.0), num((side - h) / 2.0)));
    }
    out.push_str("<g clip-path=\"url(#bios-picture)\">\n");
    let mut sheet = Sheet { out };
    for shape in shapes {
        sheet.shape(shape);
    }
    let mut out = sheet.out;
    out.push_str("</g>\n");
    if square {
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

struct Sheet {
    out: String,
}

impl Sheet {
    fn shape(&mut self, shape: &Shape) {
        match shape {
            Shape::Noop | Shape::Callback(_) => {}
            Shape::Vec(list) => list.iter().for_each(|s| self.shape(s)),
            Shape::Circle(c) => {
                let paint = self.paint(c.fill, c.stroke);
                self.element(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{paint}/>", num_f(c.center.x), num_f(c.center.y), num_f(c.radius)));
            }
            Shape::Ellipse(e) => {
                let paint = self.paint(e.fill, e.stroke);
                let turn = turned(e.angle, e.center);
                self.element(&format!(
                    "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{paint}{turn}/>",
                    num_f(e.center.x),
                    num_f(e.center.y),
                    num_f(e.radius.x),
                    num_f(e.radius.y)
                ));
            }
            Shape::LineSegment { points, stroke } => {
                let paint = self.paint(Color32::TRANSPARENT, *stroke);
                self.element(&format!(
                    "<path d=\"M {} {} L {} {}\"{paint}/>",
                    num_f(points[0].x),
                    num_f(points[0].y),
                    num_f(points[1].x),
                    num_f(points[1].y)
                ));
            }
            Shape::Path(p) => self.path(p),
            Shape::Band(b) => self.band(&b.points, b.fill, b.stroke, b.angle),
            Shape::Rect(r) => self.rect(r),
            Shape::Text(t) => self.text(t),
            Shape::Mesh(mesh) => self.mesh(mesh),
            Shape::QuadraticBezier(q) => self.quadratic(q),
            Shape::CubicBezier(c) => self.cubic(c),
        }
    }

    fn element(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// `fill` and `stroke` as SVG attributes, opacity and all.
    fn paint(&self, fill: Color32, stroke: Stroke) -> String {
        let mut out = fill_attr(fill);
        out.push_str(&stroke_attr(stroke.color, stroke.width));
        out
    }

    fn path(&mut self, p: &PathShape) {
        let width = p.stroke.width;
        let color = match &p.stroke.color {
            egui::epaint::ColorMode::Solid(c) => *c,
            // A stroke whose colour is a callback over its own bounding box: one colour cannot
            // stand for it, so only the fill is written.
            egui::epaint::ColorMode::UV(_) => Color32::TRANSPARENT,
        };
        let Some(d) = polyline(&p.points, p.closed) else { return };
        let attrs = format!("{}{}", fill_attr(p.fill), stroke_attr(color, width));
        self.element(&format!("<path d=\"{d}\"{attrs} stroke-linejoin=\"round\"/>"));
    }

    /// A band of varying width: its two boundaries walked out and back as one closed outline.
    fn band(&mut self, points: &[egui::epaint::BandPoint], fill: Color32, stroke: Stroke, angle: f32) {
        if points.len() < 2 {
            return;
        }
        let mut outline: Vec<Pos2> = points.iter().map(|p| Pos2::new(p.x, p.y.min)).collect();
        outline.extend(points.iter().rev().map(|p| Pos2::new(p.x, p.y.max)));
        let Some(d) = polyline(&outline, true) else { return };
        let turn = turned(angle, Pos2::ZERO);
        let attrs = format!("{}{}", fill_attr(fill), stroke_attr(stroke.color, stroke.width));
        self.element(&format!("<path d=\"{d}\"{attrs}{turn} stroke-linejoin=\"round\"/>"));
    }

    fn rect(&mut self, r: &RectShape) {
        // A stroke inside or outside the rectangle is an SVG stroke on a rectangle moved half its
        // width, since SVG only strokes astride an edge.
        let half = f64::from(r.stroke.width) / 2.0;
        let inset = match r.stroke_kind {
            _ if r.stroke.width <= 0.0 => 0.0,
            StrokeKind::Inside => half,
            StrokeKind::Outside => -half,
            StrokeKind::Middle => 0.0,
        } as f32;
        let rect = Rect::from_min_max(r.rect.min + egui::vec2(inset, inset), r.rect.max - egui::vec2(inset, inset));
        let (w, h) = (rect.width().max(0.0), rect.height().max(0.0));
        let radius = corner(r.corner_radius).min(w / 2.0).min(h / 2.0);
        let attrs = format!("{}{}", fill_attr(r.fill), stroke_attr(r.stroke.color, r.stroke.width));
        let round = if radius > 0.0 { format!(" rx=\"{}\"", num_f(radius)) } else { String::new() };
        self.element(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{round}{attrs}/>",
            num_f(rect.min.x),
            num_f(rect.min.y),
            num_f(w),
            num_f(h)
        ));
    }

    fn quadratic(&mut self, q: &QuadraticBezierShape) {
        let [a, b, c] = q.points;
        let close = if q.closed { " Z" } else { "" };
        let d = format!("M {} {} Q {} {} {} {}{close}", num_f(a.x), num_f(a.y), num_f(b.x), num_f(b.y), num_f(c.x), num_f(c.y));
        self.curve(&d, q.fill, &q.stroke);
    }

    fn cubic(&mut self, c: &CubicBezierShape) {
        let [a, b, d, e] = c.points;
        let close = if c.closed { " Z" } else { "" };
        let path = format!(
            "M {} {} C {} {} {} {} {} {}{close}",
            num_f(a.x),
            num_f(a.y),
            num_f(b.x),
            num_f(b.y),
            num_f(d.x),
            num_f(d.y),
            num_f(e.x),
            num_f(e.y)
        );
        self.curve(&path, c.fill, &c.stroke);
    }

    fn curve(&mut self, d: &str, fill: Color32, stroke: &egui::epaint::PathStroke) {
        let color = match &stroke.color {
            egui::epaint::ColorMode::Solid(c) => *c,
            egui::epaint::ColorMode::UV(_) => Color32::TRANSPARENT,
        };
        let attrs = format!("{}{}", fill_attr(fill), stroke_attr(color, stroke.width));
        self.element(&format!("<path d=\"{d}\"{attrs}/>"));
    }

    /// A triangle mesh, one polygon per triangle. Bios builds one for a shape a stroke cannot
    /// make, such as a primer's ribbon, and every triangle of those is one flat colour.
    fn mesh(&mut self, mesh: &Mesh) {
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let corners: Vec<&egui::epaint::Vertex> = triangle.iter().filter_map(|&i| mesh.vertices.get(i as usize)).collect();
            let [a, b, c] = corners.as_slice() else { continue };
            let d =
                format!("M {} {} L {} {} L {} {} Z", num_f(a.pos.x), num_f(a.pos.y), num_f(b.pos.x), num_f(b.pos.y), num_f(c.pos.x), num_f(c.pos.y));
            // A mesh whose corners differ in colour is a gradient; flat triangles are the honest
            // approximation, and the average is what the middle of one reads as.
            let fill = average([a.color, b.color, c.color]);
            // Half a pixel of stroke in the same colour: without it the seam between two
            // triangles of one shape shows as a hairline of the paper behind.
            self.element(&format!("<path d=\"{d}\"{}{}/>", fill_attr(fill), stroke_attr(fill, 0.5)));
        }
    }

    /// Text, letter by letter at the positions the interface measured.
    fn text(&mut self, t: &TextShape) {
        let Some(format) = t.galley.job.sections.first().map(|s| &s.format) else { return };
        let color = t.override_text_color.unwrap_or(if format.color == Color32::PLACEHOLDER { t.fallback_color } else { format.color });
        let color = fade(color, t.opacity_factor);
        if color.a() == 0 {
            return;
        }
        let (family, weight) = font_family(&format.font_id.family);
        let size = format.font_id.size;
        let turn = turned(t.angle, t.pos);
        if !turn.is_empty() {
            self.out.push_str(&format!("<g{turn}>\n"));
        }
        for row in &t.galley.rows {
            let glyphs = &row.row.glyphs;
            if glyphs.is_empty() {
                continue;
            }
            let mut xs = String::new();
            let mut letters = String::new();
            let mut baseline = None;
            for glyph in glyphs {
                if !xs.is_empty() {
                    xs.push(' ');
                }
                xs.push_str(&num_f(t.pos.x + row.pos.x + glyph.pos.x));
                baseline.get_or_insert(t.pos.y + row.pos.y + glyph.pos.y);
                escape_into(glyph.chr, &mut letters);
            }
            let Some(baseline) = baseline else { continue };
            self.element(&format!(
                "<text x=\"{xs}\" y=\"{}\" font-family=\"{family}\" font-size=\"{}\" font-weight=\"{weight}\"{} xml:space=\"preserve\">{letters}</text>",
                num_f(baseline),
                num_f(size),
                fill_attr(color)
            ));
        }
        if !turn.is_empty() {
            self.out.push_str("</g>\n");
        }
    }
}

/// The faces a reader should try for one of Bios's, and the weight. Instrument Sans and Instrument
/// Serif are the app's own; the sequence face is the system's monospace, as it is in the window.
pub fn font_family(family: &FontFamily) -> (&'static str, u16) {
    const SANS: &str = "'Instrument Sans','Helvetica Neue',Helvetica,Arial,sans-serif";
    const MONO: &str = "'SF Mono',Menlo,Consolas,monospace";
    const SERIF: &str = "'Instrument Serif',Georgia,'Times New Roman',serif";
    let (face, weight) = match family {
        FontFamily::Proportional => ("sans", 400),
        FontFamily::Monospace => ("mono", 400),
        FontFamily::Name(name) => {
            let (face, weight) = name.split_once('-').unwrap_or((name.as_ref(), "400"));
            (face, weight.parse::<u16>().unwrap_or(400))
        }
    };
    let faces = match face {
        "mono" => MONO,
        "serif" => SERIF,
        _ => SANS,
    };
    (faces, weight)
}

fn polyline(points: &[Pos2], closed: bool) -> Option<String> {
    let first = points.first()?;
    let mut d = format!("M {} {}", num_f(first.x), num_f(first.y));
    for p in points.iter().skip(1) {
        d.push_str(&format!(" L {} {}", num_f(p.x), num_f(p.y)));
    }
    if closed {
        d.push_str(" Z");
    }
    Some(d)
}

fn corner(radius: CornerRadius) -> f32 {
    f32::from(radius.nw.max(radius.ne).max(radius.sw).max(radius.se))
}

fn turned(angle: f32, about: Pos2) -> String {
    if angle == 0.0 || !angle.is_finite() {
        return String::new();
    }
    format!(" transform=\"rotate({} {} {})\"", num_f(angle.to_degrees()), num_f(about.x), num_f(about.y))
}

fn average(colors: [Color32; 3]) -> Color32 {
    let mut sum = [0u32; 4];
    for c in colors {
        for (slot, part) in sum.iter_mut().zip(c.to_srgba_unmultiplied()) {
            *slot += u32::from(part);
        }
    }
    let part = |i: usize| u8::try_from(sum.get(i).copied().unwrap_or(0) / 3).unwrap_or(255);
    Color32::from_rgba_unmultiplied(part(0), part(1), part(2), part(3))
}

fn fade(color: Color32, opacity: f32) -> Color32 {
    if opacity >= 1.0 {
        return color;
    }
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (f32::from(a) * opacity.clamp(0.0, 1.0)) as u8)
}

fn fill_attr(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 0 {
        return " fill=\"none\"".to_string();
    }
    let opacity = if a == 255 { String::new() } else { format!(" fill-opacity=\"{}\"", num(f64::from(a) / 255.0)) };
    format!(" fill=\"#{r:02x}{g:02x}{b:02x}\"{opacity}")
}

fn stroke_attr(color: Color32, width: f32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 0 || width <= 0.0 {
        return String::new();
    }
    let opacity = if a == 255 { String::new() } else { format!(" stroke-opacity=\"{}\"", num(f64::from(a) / 255.0)) };
    format!(" stroke=\"#{r:02x}{g:02x}{b:02x}\"{opacity} stroke-width=\"{}\"", num_f(width))
}

fn escape_into(ch: char, out: &mut String) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        // A control character has no glyph and would make the file unreadable to a parser.
        c if (c as u32) < 0x20 && c != '\t' => out.push(' '),
        c => out.push(c),
    }
}

fn num_f(v: f32) -> String {
    num(f64::from(v))
}

/// A number as SVG wants it: two decimals at most, and never `NaN`, which no reader accepts.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == 0.0 {
        return "0".to_string();
    }
    let mut text = format!("{rounded:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{document, num};
    use egui::{Color32, Pos2, Shape, Stroke};

    #[test]
    fn numbers_are_short_and_never_nan() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.236), "1.24");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(f64::INFINITY), "0");
        assert_eq!(num(12.5), "12.5");
    }

    #[test]
    fn a_document_states_its_size_and_a_square_one_centres_the_drawing() {
        let line = Shape::LineSegment { points: [Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)], stroke: Stroke::new(1.0, Color32::RED) };
        let plain = document(std::slice::from_ref(&line), 400.0, 200.0, false);
        assert!(plain.starts_with("<?xml"), "{plain}");
        assert!(plain.contains("width=\"400\" height=\"200\""), "{plain}");
        assert!(plain.contains("stroke=\"#ff0000\""), "{plain}");
        assert!(!plain.contains("translate"), "{plain}");

        let square = document(&[line], 400.0, 200.0, true);
        assert!(square.contains("width=\"400\" height=\"400\""), "{square}");
        assert!(square.contains("translate(0 100)"), "{square}");
    }
}
