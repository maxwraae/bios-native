//! What a binder is, while the pointer is on one of its sites: the oligo, and every place it lands.
//! Ported from the web app's `editor/BinderCard.tsx` and the `.binder-card` rules in `styles.css`
//! 3728-3795.
//!
//! ONE card per binder, not per site: the oligo is the thing, and where it binds is a fact about
//! it, so the card lists its sites, which is also the only shape that can say the useful thing out
//! loud: this oligo sticks in more than one place. Everything on it was known the moment the oligo
//! existed. Length and GC are of the molecule you would order, tail included; Tm belongs to a site.

use bios_core::binders::BinderSite;
use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, Id, Margin, Rect, Stroke, pos2};

use crate::theme::{self, Weight};

/// One line per site, worst first after the on-target: `binds at 301 · − strand · Tm 58.2°C ·
/// perfect`. `forward` means the oligo MATCHES the plus strand, so it anneals to the minus one.
pub fn site_line(s: &BinderSite) -> String {
    let tm = s.site.tm.map_or_else(|| "Tm —".to_string(), |t| format!("Tm {t:.1}°C"));
    let fit = if s.site.mismatches == 0 {
        "perfect".to_string()
    } else {
        let at: Vec<String> = s.site.mismatch_positions.iter().map(|p| (p + 1).to_string()).collect();
        format!("{} mismatch{} at base {}", s.site.mismatches, if s.site.mismatches > 1 { "es" } else { "" }, at.join(", "))
    };
    format!("binds at {} · {} · {tm} · {fit}", s.site.start + 1, if s.site.forward { "− strand" } else { "+ strand" })
}

/// The sites in the card's order: the on-target first, then by where they bind.
pub fn ordered(sites: &[BinderSite]) -> Vec<&BinderSite> {
    let mut out: Vec<&BinderSite> = sites.iter().collect();
    out.sort_by_key(|s| (!s.on_target, s.site.start));
    out
}

/// Draws the card for one binder's sites by `anchor`, the box of the site under the pointer: its
/// bottom just above it, in the empty space rather than on the bases, or below when there is no room.
pub fn show(ctx: &egui::Context, sites: &[BinderSite], anchor: Rect) {
    let Some(b) = sites.first() else {
        return;
    };
    let above = anchor.top() > 160.0;
    let (at, pivot) = if above {
        (pos2(anchor.left().max(8.0), anchor.top() - 8.0), Align2::LEFT_BOTTOM)
    } else {
        (pos2(anchor.left().max(8.0), anchor.bottom() + 8.0), Align2::LEFT_TOP)
    };
    let area = egui::Area::new(Id::unique("binder-card")).order(egui::Order::Tooltip).interactable(false).fade_in(false).pivot(pivot).fixed_pos(at);
    area.show(ctx, |ui| {
        let frame = egui::Frame::NONE
            .fill(theme::CELL)
            .stroke(Stroke::new(1.0, theme::HAIRLINE_STRONG))
            .corner_radius(CornerRadius::same(theme::RADIUS as u8))
            .inner_margin(Margin::symmetric(10, 8))
            .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(41) });
        frame.show(ui, |ui| {
            ui.set_min_width(220.0 - 20.0);
            ui.set_max_width(340.0 - 20.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(&b.name).font(theme::sans_weight(12.0, Weight::Semibold)).color(theme::INK));
            ui.add_space(4.0);
            // The tail is drawn apart from the annealing bases: bases you are INSTALLING, not bases
            // that find anything. It recedes rather than alarms.
            let mono = theme::mono(11.0);
            let mut job =
                LayoutJob { wrap: egui::text::TextWrapping { max_width: 320.0, break_anywhere: true, ..Default::default() }, ..Default::default() };
            let muted = TextFormat { font_id: mono.clone(), color: theme::MUTED, extra_letter_spacing: 0.44, ..Default::default() };
            let ink = TextFormat { color: theme::INK, ..muted.clone() };
            let tail = TextFormat { underline: Stroke::new(1.0, theme::HAIRLINE_STRONG), ..muted.clone() };
            let tail_len = b.site.tail_length.min(b.bases.len());
            job.append("5'", 0.0, muted.clone());
            if tail_len > 0 {
                job.append(b.bases.get(..tail_len).unwrap_or_default(), 4.0, tail);
            }
            job.append(b.bases.get(tail_len..).unwrap_or_default(), if tail_len > 0 { 0.0 } else { 4.0 }, ink);
            job.append("3'", 4.0, muted);
            ui.label(job);
            ui.add_space(5.0);
            let mut facts = vec![format!("{} nt", b.bases.len()), format!("{:.0}% GC", b.gc)];
            if b.site.tail_length > 0 {
                facts.push(format!("{} annealed", b.site.anneal_length));
            }
            ui.label(egui::RichText::new(facts.join("    ")).font(theme::sans(12.0)).color(theme::MUTED));
            rule(ui);
            let mut first = true;
            for s in ordered(sites) {
                // A stray is its own warning, ruled off: that line is the one you need.
                if !s.on_target && !first {
                    rule(ui);
                }
                first = false;
                let color = if s.on_target { theme::MUTED } else { theme::SANA_SCARLET };
                ui.add(egui::Label::new(egui::RichText::new(site_line(s)).font(theme::sans(12.0)).color(color)).wrap());
                ui.add_space(2.0);
            }
            if sites.iter().any(|s| !s.on_target) {
                rule(ui);
                ui.label(egui::RichText::new("this oligo doesn't only go where you put it").font(theme::sans(12.0)).color(theme::MUTED));
            }
        });
    });
}

/// `border-top: 1px solid var(--hairline)` with 5px either side.
fn rule(ui: &mut egui::Ui) {
    ui.add_space(5.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::HAIRLINE));
    ui.add_space(5.0);
}

#[cfg(test)]
mod tests {
    use bios_core::binders::{Binder, binders_to_sites};

    use super::{ordered, site_line};

    #[test]
    fn a_site_says_where_on_which_strand_how_hot_and_how_well() {
        let seq = "GGGGGGGGGGGGGGGGGGGGATGCGTACGTTGACCGATCAGGCATTACGGCTATTCAGATTTTTTTTTTTTTTTTTTTT";
        let anneal = &seq[20..42];
        let b = Binder { id: "binder-1".into(), name: "p1".into(), bases: anneal.into() };
        let sites = binders_to_sites(&[b], seq, false);
        let line = site_line(&sites[0]);
        assert!(line.starts_with("binds at 21 · − strand · Tm "), "{line}");
        assert!(line.ends_with("°C · perfect"), "{line}");
        assert_eq!(ordered(&sites).len(), 1);
    }
}
