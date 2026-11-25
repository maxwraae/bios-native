//! What an enzyme is, when the pointer is on its cut: `EcoRI · G^AATTC · 5′ AATT`. The same
//! glass pill family as the selection read-out and the primer card.
//!
//! Ported from the web app's `editor/CutHoverCard.tsx` (lines 50-170), `describeEnzyme` in
//! `editor/cutsites.ts` (141-156) and the `.cut-hover` rules in `styles.css` 3877-3916. The web
//! shows it only while the cut names are hidden: with the names on, the row already says what the
//! cut is, and a card over it covers the letters it describes.

use bios_core::enzymes::get_enzyme;
use egui::{Align2, Color32, Id, Rect, pos2, vec2};

use crate::theme;

/// The name, the recognition site with `^` at the scission when it falls inside it, and what the
/// cut leaves (`describeEnzyme`). A type IIS enzyme cuts outside its site, which is said in words.
pub fn describe(enzyme: &str) -> Option<Vec<String>> {
    let e = get_enzyme(enzyme)?;
    let top = e.top_snip_offset();
    let bottom = e.bottom_snip_offset();
    let inside = |at: i64| !e.type2s && at >= 0 && usize::try_from(at).is_ok_and(|at| at <= e.length);
    let site = match top.filter(|t| inside(*t)).and_then(|t| usize::try_from(t).ok()) {
        Some(t) => format!("{}^{}", e.site.get(..t).unwrap_or_default(), e.site.get(t..).unwrap_or_default()),
        None => e.site.to_string(),
    };
    let note = if e.type2s {
        "cuts outside site".to_string()
    } else {
        match (top, bottom) {
            (Some(t), Some(b)) if t == b => "blunt".to_string(),
            (Some(t), Some(b)) => {
                let (lo, hi) = (t.min(b).max(0) as usize, t.max(b).max(0) as usize);
                format!("{} {}", if t < b { "5′" } else { "3′" }, e.site.get(lo..hi.min(e.site.len())).unwrap_or_default())
            }
            _ => String::new(),
        }
    };
    let mut out = vec![e.name.to_string(), site];
    if !note.is_empty() {
        out.push(note);
    }
    Some(out)
}

/// Draws the pill by the cut's box on screen: above it, or below when there is no room up top.
///
/// Find's hits use the same card (`probe — would prime · 107/107 nt aligned · …`), so it is as
/// wide as what it says, up to the window, and kept inside it.
pub fn show(ctx: &egui::Context, parts: &[String], cut: Rect) {
    let screen = ctx.content_rect();
    let above = cut.top() - screen.top() > 130.0;
    let font = theme::mono(12.0);
    let bold = theme::mono_weight(12.0, theme::Weight::Semibold);
    let measure = |text: &str, font: egui::FontId| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x);
    let inner: f32 = parts.iter().enumerate().map(|(i, p)| measure(p, if i == 0 { bold.clone() } else { font.clone() })).sum::<f32>()
        + (parts.len().saturating_sub(1)) as f32 * (7.0 + measure("·", font.clone()) + 7.0);
    let size = vec2((inner + 28.0).min(screen.width() - 16.0).max(0.0), 30.0);
    let left = (cut.left() - 10.0).min(screen.right() - 8.0 - size.x).max(screen.left() + 8.0);
    let area = egui::Area::new(Id::unique("cut-hover")).order(egui::Order::Tooltip).interactable(false).fade_in(false);
    area.fixed_pos(pos2(left, if above { cut.top() - 8.0 - 30.0 } else { cut.bottom() + 8.0 })).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter();
        crate::primitives::shadow(
            painter,
            r,
            15.0,
            &[theme::Shadow { y: 12.0, blur: 28.0, spread: -10.0, color: Color32::from_rgba_unmultiplied(30, 35, 40, 82) }],
        );
        painter.rect_filled(r, egui::CornerRadius::same(15), theme::FLOAT);
        painter.rect_stroke(r, egui::CornerRadius::same(15), egui::Stroke::new(1.0, theme::HAIRLINE), egui::StrokeKind::Inside);
        let mut x = r.left() + 14.0;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                let dot = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, "·", font.clone(), theme::HAIRLINE_STRONG);
                x = dot.right() + 7.0;
            }
            let (f, ink) = match i {
                0 => (bold.clone(), theme::INK),
                1 => (font.clone(), theme::INK),
                _ => (font.clone(), theme::MUTED),
            };
            x = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, part, f, ink).right() + 7.0;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn an_enzyme_says_its_site_with_the_scission_and_what_it_leaves() {
        assert_eq!(describe("EcoRI").unwrap(), ["EcoRI", "G^AATTC", "5′ AATT"]);
        assert_eq!(describe("EcoRV").unwrap(), ["EcoRV", "GAT^ATC", "blunt"]);
        assert_eq!(describe("PstI").unwrap(), ["PstI", "CTGCA^G", "3′ TGCA"]);
        assert_eq!(describe("BsaI").unwrap(), ["BsaI", "GGTCTC", "cuts outside site"]);
        assert!(describe("NotAnEnzyme").is_none());
    }
}
