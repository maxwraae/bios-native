//! A primer's facts, while the pointer is on it: its Tm, GC and length, its oligo, whether it is
//! unique and whether its design is clean. Asked of the sequence each time, so an off-target can
//! never go stale against an edit.
//!
//! Ported from the web app's `editor/primer/PrimerCard.tsx` (lines 22-141), `primerStats` in
//! `editor/primer/primers.ts` (80-89) and the `.primer-card` rules in `styles.css` 3802-3875.

use bios_core::primer::{PrimerSearchOptions, find_primer_sites};
use bios_core::primer_quality::primer_flags;
use bios_core::seq::{DNA_TM_DEFAULTS, gc_content, tm};
use bios_core::{Construct, Strand};
use egui::epaint::Shadow;
use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Stroke, pos2, vec2};

use crate::theme;

/// Above this length a "primer" is a fragment, and its 3' design flags would be noise.
const QUALITY_MAX: usize = 60;
/// The primer's violet, spent only on the key dot and the direction.
const VIOLET: Color32 = theme::hex(0x6d3bd1);
/// `.primer-card-tag.is-warn`.
const WARN: Color32 = theme::hex(0xb7791f);
const MAX_W: f32 = 360.0;

/// What the card says about one primer.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimerFacts {
    pub name: String,
    pub forward: bool,
    /// The oligo as ordered, 5' to 3'.
    pub bases: String,
    pub tm: Option<f64>,
    /// GC, a whole percentage.
    pub gc: i64,
    /// Places it primes on the whole construct, both strands; 1 is only where it was put.
    pub sites: usize,
    /// The design flags, for an oligo short enough to have them.
    pub flags: Option<Vec<String>>,
}

impl PrimerFacts {
    /// The facts of the construct's `index`th feature, when it is a primer.
    pub fn of(c: &Construct, index: usize) -> Option<PrimerFacts> {
        let f = c.features.get(index).filter(|f| f.kind == "primer_bind")?;
        let bases = crate::rows::primer_bases(f, &c.sequence)?;
        let options = PrimerSearchOptions { circular: c.circular, ..PrimerSearchOptions::default() };
        let sites = find_primer_sites(&c.sequence, &bases, options).map_or(0, |found| found.len());
        Some(PrimerFacts {
            name: f.name.clone(),
            forward: f.strand != Strand::Reverse,
            tm: tm(&bases, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm),
            gc: (gc_content(&bases) + 0.5).floor() as i64,
            sites,
            flags: (bases.len() <= QUALITY_MAX).then(|| primer_flags(&bases).warnings),
            bases,
        })
    }

    /// The first line after the name: direction, length, GC and Tm.
    pub fn facts(&self) -> [String; 4] {
        [
            if self.forward { "5'→3'" } else { "3'←5'" }.to_string(),
            format!("{} nt", self.bases.len()),
            format!("{}% GC", self.gc),
            self.tm.map_or_else(|| "Tm —".to_string(), |t| format!("Tm {}°C", (t + 0.5).floor() as i64)),
        ]
    }

    /// The status line: uniqueness, then the design verdict. Each with whether it warns.
    pub fn status(&self) -> Vec<(String, bool)> {
        let off = self.sites.saturating_sub(1);
        let unique = self.sites == 1;
        let mut out = vec![(if unique { "unique".to_string() } else { format!("{off} off-target{}", if off > 1 { "s" } else { "" }) }, !unique)];
        if let Some(flags) = &self.flags {
            let n = flags.len();
            out.push((if n == 0 { "clean".to_string() } else { format!("{n} flag{}", if n > 1 { "s" } else { "" }) }, n > 0));
        }
        out
    }
}

fn mono(size: f32) -> FontId {
    theme::mono(size)
}

/// Draws the card by the primer's box on screen (`rect`): above it, or below when there is no
/// room up top, kept inside the window.
pub fn show(ctx: &egui::Context, facts: &PrimerFacts, rect: Rect) {
    let screen = ctx.content_rect();
    let above = rect.top() - screen.top() > 130.0;
    let left = rect.left().min(screen.right() - 268.0).max(screen.left() + 8.0);
    let height = ctx.data(|d| d.get_temp::<f32>(Id::unique("primer-card-h"))).unwrap_or(80.0);
    // `animation: sr-in 0.14s ease-out`: a fade and a 5 px rise each time it appears.
    let shown = crate::primitives::entrance(ctx, Id::unique("primer-card"), 0.14);
    let top = if above { rect.top() - 8.0 - height } else { rect.bottom() + 8.0 } + 5.0 * (1.0 - shown);
    let shown =
        egui::Area::new(Id::unique("primer-card")).order(egui::Order::Tooltip).interactable(false).fixed_pos(pos2(left, top)).fade_in(false).show(
            ctx,
            |ui| {
                ui.set_opacity(shown);
                egui::Frame::new()
                    .fill(theme::FLOAT)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(theme::RADIUS)
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .shadow(Shadow { offset: [0, 14], blur: 32, spread: 0, color: Color32::from_rgba_unmultiplied(45, 30, 80, 60) })
                    .show(ui, |ui| body(ui, facts));
            },
        );
    let h = shown.response.rect.height();
    if (h - height).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(Id::unique("primer-card-h"), h));
        ctx.request_repaint();
    }
}

/// A row of facts, dot-separated like the pill, from `at` (left, middle). Gives where it ended.
fn line(painter: &egui::Painter, at: Pos2, parts: &[(String, FontId, Color32)]) -> f32 {
    let mut x = at.x;
    for (i, (text, font, color)) in parts.iter().enumerate() {
        if i > 0 {
            let dot = painter.text(pos2(x, at.y), Align2::LEFT_CENTER, "·", mono(12.0), theme::HAIRLINE_STRONG);
            x = dot.right() + 6.0;
        }
        let drawn = painter.text(pos2(x, at.y), Align2::LEFT_CENTER, text, font.clone(), *color);
        x = drawn.right() + 6.0;
    }
    x - 6.0
}

fn body(ui: &mut egui::Ui, facts: &PrimerFacts) {
    let row_h = 12.0 * 1.3;
    let [dir, len, gc, tm_words] = facts.facts();
    let name_font = theme::mono_weight(12.0, theme::Weight::Semibold);
    // The width the first line needs, and the bases wrapped to it (at most the card's width).
    let probe = |text: &str, font: FontId| crate::primitives::text_width(ui, text, font);
    let first_w = 7.0
        + 6.0
        + probe(&facts.name, name_font.clone())
        + 6.0
        + probe(&dir, mono(11.0))
        + [&len, &gc, &tm_words].iter().map(|t| 6.0 + probe("·", mono(12.0)) + 6.0 + probe(t, mono(12.0))).sum::<f32>();
    let width = first_w.min(MAX_W - 28.0);
    let (r, _) = ui.allocate_exact_size(vec2(width, row_h), egui::Sense::hover());
    let painter = ui.painter().clone();
    painter.circle_filled(pos2(r.left() + 3.5, r.center().y), 3.5, VIOLET);
    let name = painter.text(pos2(r.left() + 13.0, r.center().y), Align2::LEFT_CENTER, &facts.name, name_font, theme::INK);
    let dir_drawn = painter.text(pos2(name.right() + 6.0, r.center().y), Align2::LEFT_CENTER, &dir, mono(11.0), VIOLET);
    let rest: Vec<(String, FontId, Color32)> = [len, gc, tm_words].into_iter().map(|t| (t, mono(12.0), theme::MUTED)).collect();
    let dot = painter.text(pos2(dir_drawn.right() + 6.0, r.center().y), Align2::LEFT_CENTER, "·", mono(12.0), theme::HAIRLINE_STRONG);
    line(&painter, pos2(dot.right() + 6.0, r.center().y), &rest);

    // `.primer-card-bases`: 5' and 3' small at the ends, the oligo spaced a little, wrapping.
    ui.add_space(6.0);
    let mut job = egui::text::LayoutJob::default();
    let end = |job: &mut egui::text::LayoutJob, text: &str| {
        job.append(text, 5.0, egui::TextFormat { font_id: mono(9.0), color: theme::MUTED, valign: egui::Align::Center, ..Default::default() });
    };
    end(&mut job, "5'");
    job.append(&facts.bases, 5.0, egui::TextFormat { font_id: mono(11.0), color: theme::INK, extra_letter_spacing: 0.44, ..Default::default() });
    end(&mut job, "3'");
    job.wrap = egui::text::TextWrapping { max_width: width, break_anywhere: true, ..Default::default() };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (r, _) = ui.allocate_exact_size(vec2(width, galley.size().y), egui::Sense::hover());
    ui.painter().galley(r.min, galley, theme::INK);
    ui.add_space(5.0);

    let status: Vec<(String, FontId, Color32)> =
        facts.status().into_iter().map(|(text, warn)| (text, mono(12.0), if warn { WARN } else { theme::ACCENT })).collect();
    let (r, _) = ui.allocate_exact_size(vec2(width, row_h), egui::Sense::hover());
    line(ui.painter(), pos2(r.left(), r.center().y), &status);
}

#[cfg(test)]
mod tests {
    use bios_core::genbank;

    use super::PrimerFacts;

    const FILE: &str = "LOCUS       t                         40 bp    DNA     linear   SYN 01-JAN-2026\n\
FEATURES             Location/Qualifiers\n     primer_bind     5..22\n                     /label=\"fwd\"\n     \
misc_feature    1..4\n                     /label=\"x\"\nORIGIN\n        1 acgtatcaat gagtacagga tcaaaaacgt acgtacgtac\n//\n";

    #[test]
    fn the_card_says_what_the_primer_is_and_whether_it_is_unique_and_clean() {
        let c = genbank::read(FILE).unwrap();
        let facts = PrimerFacts::of(&c, 0).unwrap();
        assert_eq!(facts.bases, "ATCAATGAGTACAGGATC");
        let [dir, len, gc, _] = facts.facts();
        assert_eq!((dir.as_str(), len.as_str(), gc.as_str()), ("5'→3'", "18 nt", "39% GC"));
        assert_eq!(facts.status(), [("unique".to_string(), false), ("clean".to_string(), false)]);
        assert!(PrimerFacts::of(&c, 1).is_none(), "a feature that is not a primer has no primer card");
    }
}
