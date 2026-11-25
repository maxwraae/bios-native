//! A picture of a construct, for an agent that wants to look at one rather than read numbers.
//!
//! The command (`draw_plasmid`) lives in the registry like every other, so the window, the command
//! line and MCP reach it the same way. Drawing is not the engine's work: it needs the interface's
//! own fonts to place a name, and only `bios-ui` may draw. So the program running the engine hands
//! it a [`Painter`] once at start ([`set_painter`], which `bios_ui::picture::set_up` calls), and a
//! test hands a session its own ([`crate::Session::with_painter`]).

use std::sync::{Arc, OnceLock};

use bios_core::Construct;

/// Which of the three ways Bios shows a construct the picture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The ring map.
    Circular,
    /// The line map.
    Linear,
    /// The sequence rows, with the features on them.
    Sequence,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            View::Circular => "circular",
            View::Linear => "linear",
            View::Sequence => "sequence",
        }
    }

    pub fn parse(word: &str) -> Option<View> {
        match word {
            "circular" => Some(View::Circular),
            "linear" => Some(View::Linear),
            "sequence" => Some(View::Sequence),
            _ => None,
        }
    }
}

/// One picture to draw.
pub struct Ask<'a> {
    pub construct: &'a Construct,
    pub view: View,
    /// The stretch to draw, 0-based and inclusive. An `end` before `start` runs through the origin
    /// of a circular construct.
    pub start: i64,
    pub end: i64,
    pub width: f64,
    pub height: f64,
    /// Centre the drawing in a square document instead of writing it at its own size. macOS's
    /// QuickLook, which turns the SVG into a PNG, fills a square and crops whatever overhangs, so
    /// a picture that is not square has to be padded to one and cropped back afterwards.
    pub square: bool,
}

/// A drawn picture.
pub struct Picture {
    /// The whole SVG document.
    pub svg: String,
    /// The bases actually drawn, 0-based and inclusive. The sequence view draws whole rows, and
    /// only as many of them as the picture is tall enough to hold, so it may be more or less than
    /// was asked for; a map does not zoom in past a hundredth of the molecule, so a short stretch
    /// comes back with its surroundings.
    pub drawn: (i64, i64),
    /// The size the drawing came out at. A map fills what it was given; the sequence view is as
    /// tall as its rows need, up to that.
    pub size: (f64, f64),
}

/// Draws a construct.
pub trait Painter: Send + Sync {
    fn draw(&self, ask: &Ask<'_>) -> Result<Picture, String>;
}

static PAINTER: OnceLock<Arc<dyn Painter>> = OnceLock::new();

/// The painter every session of this process uses unless it was given its own. Set once, at
/// start; a second call changes nothing and returns `false`.
pub fn set_painter(painter: Arc<dyn Painter>) -> bool {
    PAINTER.set(painter).is_ok()
}

pub(crate) fn installed() -> Option<Arc<dyn Painter>> {
    PAINTER.get().cloned()
}
