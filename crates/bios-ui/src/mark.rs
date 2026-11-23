//! The Bios mark, ported from the web's `landing/BiosMark.tsx` (whole file): a real base pair
//! drawn in droplets. Two purines (A, G) are the big dots and two pyrimidines (C, T) the small
//! ones, each pair flowing together like two water droplets, tilted 30°.
//!
//! The web draws it as SVG with a "gooey" filter: each pair is blurred (`feGaussianBlur`
//! stdDeviation 5) and its alpha pushed through `20a - 8` (`feColorMatrix`), which fuses the two
//! circles of a pair into one droplet. egui has no filters, so the same arithmetic is done here
//! once per size, on the CPU, into a texture.

use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};

use crate::theme;

/// How many samples per output pixel, each way, before the picture is averaged down.
const SUPERSAMPLE: usize = 4;
/// `feGaussianBlur stdDeviation="5"`, in the mark's 100-unit box.
const BLUR: f32 = 5.0;

/// One circle of the mark, in the 100-unit box before the group's 30° turn.
struct Dot {
    cx: f32,
    cy: f32,
    r: f32,
    color: Color32,
}

/// The two pairs, as BiosMark.tsx draws them: A–T on top, G–C below (the top pair turned 180°).
fn pairs() -> [[Dot; 2]; 2] {
    [
        [Dot { cx: 40.0, cy: 30.0, r: 20.0, color: theme::SANA_GREEN }, Dot { cx: 67.0, cy: 30.0, r: 15.0, color: theme::SANA_ORANGE }],
        [Dot { cx: 58.0, cy: 70.0, r: 20.0, color: theme::SANA_MAGENTA }, Dot { cx: 35.0, cy: 70.0, r: 15.0, color: theme::SANA_BLUE }],
    ]
}

/// `rotate(30 50 50)`, as SVG turns a point: clockwise on screen.
fn turned(x: f32, y: f32) -> (f32, f32) {
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let (dx, dy) = (x - 50.0, y - 50.0);
    (50.0 + dx * cos - dy * sin, 50.0 + dx * sin + dy * cos)
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// A square of premultiplied linear RGBA samples.
struct Layer {
    side: usize,
    px: Vec<[f32; 4]>,
}

impl Layer {
    fn new(side: usize) -> Self {
        Layer { side, px: vec![[0.0; 4]; side * side] }
    }

    /// Paints a circle over what is there, with its edge anti-aliased by distance.
    fn circle(&mut self, dot: &Dot, scale: f32) {
        let (cx, cy) = turned(dot.cx, dot.cy);
        let (cx, cy, r) = (cx * scale, cy * scale, dot.r * scale);
        let color = [to_linear(dot.color.r()), to_linear(dot.color.g()), to_linear(dot.color.b())];
        for y in 0..self.side {
            for x in 0..self.side {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let cover = (r - d + 0.5).clamp(0.0, 1.0);
                if cover > 0.0
                    && let Some(p) = self.px.get_mut(y * self.side + x)
                {
                    for (channel, value) in p.iter_mut().zip([color[0], color[1], color[2], 1.0]) {
                        *channel = value * cover + *channel * (1.0 - cover);
                    }
                }
            }
        }
    }

    /// A separable Gaussian blur of `sigma` samples, on every channel.
    fn blur(&mut self, sigma: f32) {
        let reach = (sigma * 3.0).ceil() as isize;
        let kernel: Vec<f32> = (-reach..=reach).map(|i| (-(i as f32).powi(2) / (2.0 * sigma * sigma)).exp()).collect();
        let total: f32 = kernel.iter().sum();
        let kernel: Vec<f32> = kernel.iter().map(|k| k / total).collect();
        let side = self.side as isize;
        for horizontal in [true, false] {
            let from = self.px.clone();
            for y in 0..side {
                for x in 0..side {
                    let mut sum = [0.0f32; 4];
                    for (k, weight) in kernel.iter().enumerate() {
                        let offset = k as isize - reach;
                        let (sx, sy) = if horizontal { (x + offset, y) } else { (x, y + offset) };
                        if (0..side).contains(&sx)
                            && (0..side).contains(&sy)
                            && let Some(p) = from.get((sy * side + sx) as usize)
                        {
                            for (s, v) in sum.iter_mut().zip(p) {
                                *s += v * weight;
                            }
                        }
                    }
                    if let Some(p) = self.px.get_mut((y * side + x) as usize) {
                        *p = sum;
                    }
                }
            }
        }
    }

    /// `feColorMatrix` with `0 0 0 20 -8` on alpha: the colour stays, the alpha is pushed hard
    /// either way so the blurred pair has a crisp outline again.
    fn goo(&mut self) {
        for p in &mut self.px {
            let alpha = p[3];
            let kept = (20.0 * alpha - 8.0).clamp(0.0, 1.0);
            let k = if alpha > 1e-6 { kept / alpha } else { 0.0 };
            *p = [p[0] * k, p[1] * k, p[2] * k, kept];
        }
    }
}

/// The mark as `px` × `px` pixels.
pub fn picture(px: usize) -> ColorImage {
    let px = px.clamp(4, 512);
    let side = px * SUPERSAMPLE;
    let scale = side as f32 / 100.0;
    let mut out = Layer::new(side);
    for pair in pairs() {
        let mut layer = Layer::new(side);
        for dot in &pair {
            layer.circle(dot, scale);
        }
        layer.blur(BLUR * scale);
        layer.goo();
        // The second pair over the first, premultiplied.
        for (o, l) in out.px.iter_mut().zip(&layer.px) {
            let under = 1.0 - l[3];
            for (oc, lc) in o.iter_mut().zip(l) {
                *oc = lc + *oc * under;
            }
        }
    }
    let mut pixels = Vec::with_capacity(px * px);
    let n = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    for y in 0..px {
        for x in 0..px {
            let mut sum = [0.0f32; 4];
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    if let Some(p) = out.px.get((y * SUPERSAMPLE + sy) * side + x * SUPERSAMPLE + sx) {
                        for (s, v) in sum.iter_mut().zip(p) {
                            *s += v / n;
                        }
                    }
                }
            }
            let a = sum[3];
            let straight = |c: f32| if a > 1e-6 { to_srgb(c / a) } else { 0.0 };
            let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels.push(Color32::from_rgba_premultiplied(
                byte(straight(sum[0]) * a),
                byte(straight(sum[1]) * a),
                byte(straight(sum[2]) * a),
                byte(a),
            ));
        }
    }
    ColorImage::new([px, px], pixels)
}

/// Paints the mark `size` points across, centred on `center`.
pub fn paint(painter: &Painter, center: Pos2, size: f32) {
    let ctx = painter.ctx();
    let px = (size * ctx.pixels_per_point()).round().max(4.0) as usize;
    let id = egui::Id::unique(("bios-mark", px));
    let texture = ctx.data(|d| d.get_temp::<TextureHandle>(id)).unwrap_or_else(|| {
        let handle = ctx.load_texture(format!("bios-mark-{px}"), picture(px), TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
        handle
    });
    let rect = Rect::from_center_size(center, vec2(size, size));
    painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::picture;

    #[test]
    fn the_mark_has_its_four_colours_where_its_dots_are_and_nothing_in_the_corners() {
        let image = picture(50);
        let at = |x: usize, y: usize| image.pixels[y * 50 + x];
        // The corners are outside every droplet.
        for (x, y) in [(0, 0), (49, 0), (0, 49), (49, 49)] {
            assert_eq!(at(x, y).a(), 0, "corner {x},{y}");
        }
        // The middle of the big green purine, turned 30° about the centre: (51.3, 27.7) of 100.
        let a = at(25, 13);
        assert_eq!(a.a(), 255);
        assert!(a.g() > a.r() && a.g() > a.b(), "{a:?}");
        // The middle of the big magenta purine: (46.9, 71.3) of 100.
        let g = at(23, 35);
        assert_eq!(g.a(), 255);
        assert!(g.r() > g.g(), "{g:?}");
    }
}
