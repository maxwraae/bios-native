//! How wide is this name, really? The map's one text ruler. Ported from Bios `editor/overview/textFit.ts`.
//!
//! Every "does this name fit" decision in the map used to be a character count times a fudge
//! factor. That was survivable while names only had to be *roughly* placed beside the thing they
//! name. It stops being survivable the moment a name is written INSIDE its own arc or bar, because
//! then the estimate is the difference between a label that fits and a label that overhangs its
//! shape. So: measure.
//!
//! The web app measures on an offscreen canvas in the interface font. This crate has no fonts, so
//! the measuring itself is handed to [`TextRuler::new`] by whoever can draw text, and the ruler
//! keeps the two things a bare measurement would get wrong:
//!
//!   1. Fonts can arrive late. Measure before the real face is in and the answer is for the
//!      fallback face, which is NARROWER, so a name measures short, is judged to fit, and then
//!      overflows its arc when the real font lands. Hence [`TextRuler::on_fonts_ready`] and the
//!      pre-load safety factor.
//!   2. The map asks about a few dozen names and their prefixes, over and over, so the widths are
//!      cached, and dropped wholesale when the font arrives.

use std::collections::HashMap;

/// Per character, per pixel of type: the estimate used when nothing can measure.
pub const TEXT_MEASURE_FALLBACK: f64 = 0.55;

/// Applied to every measurement taken BEFORE the real face is available. The fallback stack is
/// narrower than the interface font, so unpadded measurements taken against it are optimistic.
/// Erring high means the first frame may over-truncate by a character and correct itself once the
/// font arrives; the opposite mistake, a name overhanging its shape, is the ugly one.
pub const PRE_LOAD_SAFETY: f64 = 1.06;

/// The cache is emptied wholesale when it holds this many widths.
pub const CACHE_MAX: usize = 4096;

/// What JavaScript's `trim` strips. Rust's `char::is_whitespace` differs by two characters: it has
/// U+0085 and lacks U+FEFF.
pub(crate) fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// The width guessed for `text` when nothing can measure it. Counts UTF-16 units, as the web app's
/// `text.length` does.
pub fn estimate_ui(text: &str, font_px: f64) -> f64 {
    text.encode_utf16().count() as f64 * font_px * TEXT_MEASURE_FALLBACK
}

/// The interface font's ruler: a measuring function, and what is known about how far to trust it.
pub struct TextRuler<M = fn(&str, f64) -> f64> {
    measure: Option<M>,
    fonts_ready: bool,
    // Keyed on the type size, then the text, so a lookup borrows the text instead of copying it.
    cache: HashMap<u64, HashMap<String, f64>>,
    cached: usize,
    // Bumped whenever cached widths stop being valid. The map can depend on this to lay out again
    // once the real font is in, instead of leaving the first frame's guesses on screen forever.
    generation: u64,
}

impl TextRuler {
    /// A ruler with nothing to measure with: every width is [`estimate_ui`]. This is what the web
    /// app's ruler is when it runs without a page.
    pub fn without_fonts() -> Self {
        Self { measure: None, fonts_ready: false, cache: HashMap::new(), cached: 0, generation: 0 }
    }
}

impl<M: FnMut(&str, f64) -> f64> TextRuler<M> {
    /// A ruler over `measure(text, font_px)`, the true advance width of `text` in the interface font.
    pub fn new(measure: M) -> Self {
        Self { measure: Some(measure), fonts_ready: false, cache: HashMap::new(), cached: 0, generation: 0 }
    }

    /// Advance width of `text` at `font_px`, in the interface font.
    pub fn measure_ui(&mut self, text: &str, font_px: f64) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(measure) = self.measure.as_mut() else { return estimate_ui(text, font_px) };

        let size = font_px.to_bits();
        if let Some(hit) = self.cache.get(&size).and_then(|widths| widths.get(text)) {
            return *hit;
        }

        let mut w = measure(text, font_px);
        if !self.fonts_ready {
            w *= PRE_LOAD_SAFETY;
        }

        if self.cached >= CACHE_MAX {
            self.cache.clear();
            self.cached = 0;
        }
        self.cache.entry(size).or_default().insert(text.to_owned(), w);
        self.cached += 1;
        w
    }

    /// Say the interface font is now measurable, and run `cb` (the caller's "lay out again").
    ///
    /// Flips the ruler out of its pessimistic pre-load mode and drops every width measured while it
    /// was in it. The web app waits for the page's fonts itself; here the caller says when. A ruler
    /// with nothing to measure with has no widths to correct, so `cb` is not run for it.
    pub fn on_fonts_ready(&mut self, cb: impl FnOnce()) {
        self.fonts_ready = true;
        if self.measure.is_none() {
            return;
        }
        self.cache.clear();
        self.cached = 0;
        self.generation = self.generation.wrapping_add(1);
        cb();
    }

    /// How many times the cached widths have been thrown away.
    pub fn font_generation(&self) -> u64 {
        self.generation
    }
}
