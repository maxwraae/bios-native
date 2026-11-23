//! Where each letter of a name written along an arc sits.
//!
//! The web app hands the browser an arc and a string (an SVG `textPath`, anchored at its middle)
//! and the browser sets the letters along it. A painter without that has to place each letter
//! itself; this is that placement, as numbers.

use super::projection::{RingGeom, TAU};
use super::ring_map::ArcLabel;

/// One letter on the arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcGlyph {
    /// Where the middle of the letter's advance meets the circle the text rides on.
    pub x: f64,
    pub y: f64,
    /// How far the letter is turned, in radians clockwise on screen, from lying flat left to right.
    pub angle: f64,
}

/// The letters of `label`, given where each one's middle falls along the straight text
/// (`centres`, measured from the start of the text) and how wide the whole text is.
///
/// The middle of the text sits at the label's own fraction. On the top of the ring the letters
/// run the way the bases do; on the bottom they run the other way and are turned half a turn
/// more, so they come out the right way up.
pub fn arc_glyphs(label: &ArcLabel, center: f64, geom: RingGeom, centres: &[f64], total: f64) -> Vec<ArcGlyph> {
    if label.radius <= 0.0 {
        return Vec::new();
    }
    let mid = (label.frac - center) * TAU;
    centres
        .iter()
        .map(|c| {
            let along = (c - total / 2.0) / label.radius;
            let theta = if label.upright { mid + along } else { mid - along };
            ArcGlyph {
                x: geom.cx + label.radius * theta.sin(),
                y: geom.cyc - label.radius * theta.cos(),
                angle: if label.upright { theta } else { theta + std::f64::consts::PI },
            }
        })
        .collect()
}
