//! ONE ALIGNMENT, filling the centre, reads first. Ported from the web's `verify/AlignmentView.tsx`,
//! `reads/MapperPileup.tsx`, `verify/DepthTrack.tsx`, `verify/MapperFiles.tsx` and
//! `verify/AlignmentHelp.tsx`.
//!
//! A band of text to skim (the chip, the map it was measured against, the numbers and words, the
//! files on this map, the way out), the map drawn linear in the reads' own columns with a pin at
//! every consensus difference, the reads taking everything that is left, and the depth over the
//! whole map at the foot with the window you are in. Look on the left of the floating bar (← →
//! through the changes), judge on the right (1 2 3), and judging moves you on.

use bios_core::verify::folder::{Call, RunSummary};
use bios_core::verify::mapper::{self, ChipFeature, Difference, MapperOutput, Reading, VerdictTone};
use bios_core::verify::runs::{Tone, disagrees};
use bios_layout::pileup::{self, Piece, Pile, Window};
use egui::{Align2, Color32, CornerRadius, CursorIcon, Key, Modifiers, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::{self, Button, Face, Material, Segment, text_width};
use crate::theme::{self, Weight};
use crate::verify_kit::{self, CHIP_H, ChipKind};

/// A feature on the map, in padded columns (0-based inclusive; `end < start` wraps).
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnFeature {
    pub name: String,
    pub kind: String,
    pub start: usize,
    pub end: usize,
}

/// What the view shows.
pub struct AlignModel<'a> {
    pub output: &'a MapperOutput,
    pub reading: &'a Reading,
    pub sample: &'a str,
    pub map_name: &'a str,
    /// The stored summary: the call on it, and whether it overrules the finding.
    pub run: Option<&'a RunSummary>,
    pub features: &'a [ColumnFeature],
    /// Which file this is of the files on its map, and how many there are.
    pub file_at: Option<(usize, usize)>,
    pub can_rerun: bool,
    pub rerunning: bool,
    pub error: Option<&'a str>,
    /// The folder's copy no longer has the bases these reads were aligned against.
    pub mismatch: Option<String>,
}

/// What the person did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlignAction {
    Call(Call),
    /// The previous (-1) or next (1) file on this map, no call made.
    BrowseFile(i32),
    Overview,
    Rerun,
    /// `consensus`, `sam`, `coverage`, `differences` or `zip`.
    Download(&'static str),
}

/// Pixels per column a step lands on, and the narrowest window worth landing on.
const LETTER_PX: f32 = 14.0;
const MIN_FLY: f64 = 40.0;
const LANE_H: f32 = 20.0;
const DEPTH_H: f32 = 36.0;

struct PileCache {
    key: String,
    pieces: Vec<(Piece, usize)>,
    pile: Pile,
    labels: Vec<String>,
}

/// The view's state between frames.
#[derive(Default)]
pub struct AlignView {
    key: String,
    window: Option<Window>,
    step: Option<usize>,
    top: usize,
    help: bool,
    hover: String,
    drag: Option<(f32, f64)>,
    strip_drag: bool,
    cache: Option<PileCache>,
    /// A change to land on when next drawn (`land_on`).
    pending: Option<usize>,
}

impl AlignView {
    /// Draws the alignment in `work` (the whole column, the band at its top included).
    pub fn show(&mut self, ui: &mut Ui, work: Rect, m: &AlignModel) -> Vec<AlignAction> {
        let mut out = Vec::new();
        let n = mapper::display_columns(m.output);
        let key = format!("{}\u{0}{}\u{0}{}", m.sample, m.map_name, m.output.rows.len());
        if self.key != key {
            let pending = self.pending.take();
            *self = AlignView { key: key.clone(), pending, ..AlignView::default() };
        }
        let mut window = self.window.unwrap_or_else(|| Window::whole(n)).clamp(n);
        self.ensure_cache(m, n);
        let walk = &m.reading.differences;
        if let Some(i) = self.pending.take().filter(|i| *i < walk.len()) {
            self.step = Some(i);
            if let Some(d) = walk.get(i) {
                window = fly(window, n, d.column, work.width());
            }
        }

        let painter = ui.painter().with_clip_rect(work);
        painter.rect_filled(work, 0.0, theme::CANVAS);
        crate::verify_board::band(ui, work, "");
        let left = work.left() + 24.0;
        let right = work.right() - 24.0;
        let mut y = work.top() + theme::HEAD_H + 12.0;

        // The head row.
        y = self.head(ui, m, left, right, y, &mut out);
        // The files the mapper wrote.
        y = files_row(ui, left, y, &mut out);
        if self.help {
            y = help_panel(ui, m.reading, left, right, y);
        }
        if let Some(e) = m.error.map(str::to_string).or(m.mismatch.clone()) {
            painter.text(pos2(left, y + 8.0), Align2::LEFT_CENTER, e, theme::sans(12.0), theme::DANGER_TEXT);
            y += 20.0;
        }

        // The map, linear, in the reads' columns: work's full width.
        let (wl, wr) = (work.left(), work.right());
        let width = wr - wl;
        let feature_lanes = lanes_of(m.features, n);
        let lanes = feature_lanes.iter().map(|(_, l)| *l + 1).max().unwrap_or(0) as f32;
        let map_h = lanes.max(1.0) * 18.0 + 16.0;
        let map_rect = Rect::from_min_max(pos2(wl, y), pos2(wr, y + map_h));
        if let Some(column) = draw_map(ui, map_rect, m, &feature_lanes, walk, window, n) {
            window = fly(window, n, column, width);
            self.step = walk.iter().position(|d| d.column == column);
        }
        y = map_rect.bottom();

        // The strip at the foot, then the reads in between.
        let strip = Rect::from_min_max(pos2(wl, work.bottom() - (DEPTH_H + 7.0 + 26.0)), pos2(wr, work.bottom()));
        let reads = Rect::from_min_max(pos2(wl, y), pos2(wr, strip.top()));
        window = self.reads(ui, reads, m, window, n);
        window = self.strip(ui, strip, m, walk, window, n);

        // The floating bar over the reads: look on the left, judge on the right.
        // Over the reads, clear of the controls line under them.
        let bar_bottom = reads.bottom() - 26.0 - 12.0;
        if let Some(column) = self.tools(ui, m, walk, reads.center().x, bar_bottom, &mut out) {
            window = fly(window, n, column, width);
        }
        if let Some(column) = self.keys(ui, walk, &mut out) {
            window = fly(window, n, column, width);
        }
        self.window = Some(window);
        out
    }

    fn ensure_cache(&mut self, m: &AlignModel, n: usize) {
        if self.cache.as_ref().is_some_and(|c| c.key == self.key) {
            return;
        }
        let circular = m.output.reference.circular;
        let pieces: Vec<(Piece, usize)> = m
            .output
            .rows
            .iter()
            .enumerate()
            .flat_map(|(i, r)| pileup::row_segments(r.leading, r.internal.chars().count(), n, circular).into_iter().map(move |p| (p, i)))
            .collect();
        let starts: Vec<usize> = pieces.iter().map(|(p, _)| p.start).collect();
        let ends: Vec<usize> = pieces.iter().map(|(p, _)| p.start + p.length.saturating_sub(1)).collect();
        let pile = pileup::pack(&starts, &ends, n, 2);
        self.cache = Some(PileCache { key: self.key.clone(), pieces, pile, labels: mapper::column_labels(&m.output.padded_reference) });
    }

    fn head(&mut self, ui: &mut Ui, m: &AlignModel, left: f32, right: f32, y: f32, out: &mut Vec<AlignAction>) -> f32 {
        let painter = ui.painter().clone();
        let mid = y + 17.0;
        let call = m.run.and_then(|r| r.call);
        let (_, tone) = mapper::verdict(m.reading);
        let tone = match tone {
            VerdictTone::Pick => Tone::Ok,
            VerdictTone::Fail => Tone::Changed,
            VerdictTone::Caveat => Tone::Gaps,
        };
        let kind = ChipKind::Stored { call, tone, stale: false, overruled: m.run.is_some_and(disagrees) };
        let chip_w = verify_kit::chip_width(ui, m.sample, &kind, false);
        let chip_rect = Rect::from_min_size(pos2(left, mid - CHIP_H / 2.0), vec2(chip_w, CHIP_H));
        let facts = mapper::header_numbers(m.reading).join(" · ");
        let title = match call {
            Some(c) => format!("{facts} — you called it {}", c.word()),
            None => facts,
        };
        verify_kit::chip(
            ui,
            chip_rect,
            ui.scope_id().with("align-chip"),
            &verify_kit::ChipSpec { label: m.sample, kind: &kind, removable: false, dimmed: false, title: &title },
        );
        let mut x = chip_rect.right() + 12.0;
        let against = format!("against {}", m.map_name);
        let g = painter.text(pos2(x, mid), Align2::LEFT_CENTER, &against, theme::sans(13.0), theme::MUTED);
        x = g.right() + 12.0;

        // The buttons, from the right: Overview, Run again, ?, the files on this map.
        let mut bx = right;
        let overview = Button::label("Overview", Material::Ghost);
        bx -= overview.width(ui);
        if overview.show_at(ui, pos2(bx, y)).on_hover_text("Back to the board (Esc)").clicked() {
            out.push(AlignAction::Overview);
        }
        if m.can_rerun {
            let mut again = Button::label(if m.rerunning { "Running…" } else { "Run again" }, Material::Ghost);
            again.enabled = !m.rerunning;
            bx -= again.width(ui) + 8.0;
            if again.show_at(ui, pos2(bx, y)).clicked() {
                out.push(AlignAction::Rerun);
            }
        }
        let mut help = Button::label("?", Material::Ghost);
        help.active = self.help;
        bx -= help.width(ui) + 8.0;
        if help.show_at(ui, pos2(bx, y)).on_hover_text("How this alignment works").clicked() {
            self.help = !self.help;
        }
        if let Some((at, total)) = m.file_at.filter(|(_, t)| *t > 1) {
            let count = format!("{} / {total}", at + 1);
            let segs = [
                Segment {
                    face: Face::Icon(crate::lucide::CHEVRON_LEFT, 15.0),
                    active: false,
                    enabled: true,
                    tip: "Previous file on this map (shift ←)",
                },
                Segment { face: Face::Label(&count), active: false, enabled: false, tip: "" },
                Segment {
                    face: Face::Icon(crate::lucide::CHEVRON_RIGHT, 15.0),
                    active: false,
                    enabled: true,
                    tip: "Next file on this map (shift →)",
                },
            ];
            bx -= primitives::group_width(ui, &segs, false) + 8.0;
            match primitives::group(ui, pos2(bx, y), &segs, Material::Ghost, false) {
                Some(0) => out.push(AlignAction::BrowseFile(-1)),
                Some(2) => out.push(AlignAction::BrowseFile(1)),
                _ => {}
            }
        }

        // The numbers and the words, between the chip and the buttons.
        let mut terms: Vec<(String, Color32)> = mapper::header_numbers(m.reading).into_iter().map(|t| (t, theme::INK)).collect();
        for t in mapper::header_words(m.reading) {
            let c = match t.tone {
                Some("differ") => theme::SANA_SCARLET,
                Some("thin") => theme::CUT_SITE,
                Some("quiet") => theme::MUTED,
                _ => theme::INK,
            };
            terms.push((t.text, c));
        }
        let room = bx - 12.0;
        for (i, (text, color)) in terms.iter().enumerate() {
            if i > 0 {
                let g = painter.text(pos2(x, mid), Align2::LEFT_CENTER, "·", theme::sans(13.0), theme::MUTED);
                x = g.right() + 6.0;
            }
            let w = text_width(ui, text, theme::sans(13.0));
            if x + w > room {
                break;
            }
            let g = painter.text(pos2(x, mid), Align2::LEFT_CENTER, text, theme::sans(13.0), *color);
            x = g.right() + 6.0;
        }
        y + 34.0 + 6.0
    }

    fn reads(&mut self, ui: &mut Ui, rect: Rect, m: &AlignModel, mut window: Window, n: usize) -> Window {
        let Some(cache) = &self.cache else { return window };
        let painter = ui.painter().with_clip_rect(rect);
        let width = rect.width();
        let px = width / window.bases as f32;
        let header = if px >= 10.0 { if px >= 26.0 { 100.0 } else { 62.0 } } else { 24.0 };
        let controls_h = 26.0;
        let canvas = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - controls_h));
        let visible = (((canvas.height() - header) / LANE_H).floor() as usize).max(1);
        let from = window.from.floor().max(0.0) as usize;
        let end = ((window.from + window.bases).ceil() as usize).min(n);
        let lanes: Vec<usize> = (0..cache.pile.lanes)
            .filter(|&l| {
                cache.pile.order[cache.pile.offset[l]..cache.pile.offset[l + 1]]
                    .iter()
                    .any(|&j| cache.pieces[j].0.start < end && cache.pieces[j].0.start + cache.pieces[j].0.length > from)
            })
            .collect();
        self.top = self.top.min(lanes.len().saturating_sub(visible));
        let x_of = |c: usize| canvas.left() + window.x_of(c as f64, width);
        let padded: Vec<char> = m.output.padded_reference.chars().collect();
        let consensus: Vec<char> = m.output.final_consensus.consensus_aligned.chars().collect();
        let font = theme::mono(12.0);
        let step = ((70.0 / px).ceil() as usize).max(1);
        for c in from..end {
            if c % step == 0 {
                painter.text(
                    pos2(x_of(c) + 1.0, canvas.top() + 16.0),
                    Align2::LEFT_BOTTOM,
                    cache.labels.get(c).map_or("", String::as_str),
                    font.clone(),
                    theme::MUTED,
                );
            }
            if px >= 10.0 {
                let r = padded.get(c).copied().unwrap_or(' ');
                let k = consensus.get(c).copied().unwrap_or(' ');
                painter.text(pos2(x_of(c) + 1.0, canvas.top() + 38.0), Align2::LEFT_BOTTOM, r.to_string(), font.clone(), theme::INK);
                painter.text(
                    pos2(x_of(c) + 1.0, canvas.top() + 58.0),
                    Align2::LEFT_BOTTOM,
                    k.to_string(),
                    font.clone(),
                    if k != r { theme::hex(0xbb3e4b) } else { theme::INK },
                );
                if px >= 26.0 {
                    let cov = m.output.final_consensus.coverage.get(c).copied().unwrap_or(0.0);
                    let q = m.output.final_consensus.quality.get(c).copied().unwrap_or(0.0);
                    painter.text(pos2(x_of(c) + 1.0, canvas.top() + 78.0), Align2::LEFT_BOTTOM, number(cov), font.clone(), theme::INK);
                    painter.text(pos2(x_of(c) + 1.0, canvas.top() + 98.0), Align2::LEFT_BOTTOM, number(q), font.clone(), theme::INK);
                }
            }
        }
        let mut hover = String::new();
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| canvas.contains(*p));
        for (slot, &lane) in lanes.iter().enumerate().skip(self.top).take(visible) {
            let ly = canvas.top() + header + (slot - self.top) as f32 * LANE_H;
            for &j in &cache.pile.order[cache.pile.offset[lane]..cache.pile.offset[lane + 1]] {
                let (piece, read) = cache.pieces[j];
                let (a, b) = (from.max(piece.start), end.min(piece.start + piece.length));
                if a >= b {
                    continue;
                }
                let Some(row) = m.output.rows.get(read) else { continue };
                let chars: Vec<char> = row.internal.chars().collect();
                for c in a..b {
                    let off = piece.offset + c - piece.start;
                    let base = chars.get(off).copied().unwrap_or(' ');
                    let trimmed = off < row.trim_left || off + row.trim_right >= chars.len();
                    let mismatch = Some(base) != padded.get(c).copied();
                    let fill = if mismatch {
                        theme::hex(0xd98c94)
                    } else if row.reverse {
                        theme::hex(0xb5c0c4)
                    } else {
                        theme::hex(0xd0d7d9)
                    };
                    let cell = Rect::from_min_size(pos2(x_of(c), ly + 4.0), vec2(px.max(0.2), 15.0));
                    painter.rect_filled(cell, 0.0, if trimmed { theme::alpha(fill, 0.3) } else { fill });
                    if px >= 10.0 {
                        painter.text(
                            pos2(x_of(c) + 1.0, ly + 16.0),
                            Align2::LEFT_BOTTOM,
                            base.to_string(),
                            font.clone(),
                            if trimmed { theme::alpha(theme::INK, 0.3) } else { theme::INK },
                        );
                    }
                    if pointer.is_some_and(|p| cell.expand2(vec2(0.0, 2.5)).contains(p)) {
                        hover = format!(" · {} · {base}{} · MAPQ {}", row.name, if trimmed { " (trimmed)" } else { "" }, row.mapping_quality);
                    }
                }
            }
        }
        let resp = ui.interact(canvas, ui.scope_id().with("pileup"), Sense::click_and_drag());
        if let Some(p) = pointer {
            let c = window.column_at(p.x - canvas.left(), width).floor();
            if c >= 0.0 && (c as usize) < n {
                let c = c as usize;
                let f = &m.output.final_consensus;
                self.hover = format!(
                    "Base {} · column {} · reference {} · consensus {} · coverage {} · quality {}{hover}",
                    cache.labels.get(c).map_or("", String::as_str),
                    c + 1,
                    padded.get(c).copied().unwrap_or(' '),
                    consensus.get(c).copied().unwrap_or(' '),
                    number(f.coverage.get(c).copied().unwrap_or(0.0)),
                    number(f.quality.get(c).copied().unwrap_or(0.0)),
                );
            }
            window = self.wheel(ui, p.x - canvas.left(), width, window, n, lanes.len().saturating_sub(visible));
        }
        if resp.drag_started() {
            self.drag = resp.interact_pointer_pos().map(|p| (p.x, window.from));
        }
        if resp.dragged()
            && let (Some((x0, from0)), Some(p)) = (self.drag, resp.interact_pointer_pos())
        {
            window = Window { from: from0 - f64::from(p.x - x0) * window.bases / f64::from(width), bases: window.bases }.clamp(n);
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if resp.drag_stopped() {
            self.drag = None;
        }

        // The controls under the reads: zoom, the rows, and what is under the pointer.
        let cy = canvas.bottom() + 13.0;
        let segs = [
            Segment { face: Face::Label("Zoom in"), active: false, enabled: true, tip: "" },
            Segment { face: Face::Label("Zoom out"), active: false, enabled: true, tip: "" },
            Segment { face: Face::Label("Whole map"), active: false, enabled: true, tip: "" },
        ];
        match primitives::group_small(ui, pos2(rect.left() + 8.0, cy - 14.0), &segs, Material::Ghost) {
            Some(0) => window = window.zoom_at(n, 0.5, 0.5),
            Some(1) => window = window.zoom_at(n, 2.0, 0.5),
            Some(2) => window = Window::whole(n).clamp(n),
            _ => {}
        }
        let gx = rect.left() + 8.0 + primitives::group_small_width(ui, &segs) + 12.0;
        let rows = format!(
            "Rows {}–{} / {} · {} reads",
            if lanes.is_empty() { 0 } else { self.top + 1 },
            (self.top + visible).min(lanes.len()),
            lanes.len(),
            bios_core::verify::grouped(m.output.rows.len() as i64)
        );
        let g = painter.text(pos2(gx, cy), Align2::LEFT_CENTER, rows, theme::sans(11.0), theme::MUTED);
        let words = if self.hover.is_empty() {
            "Hover to inspect a base. Scroll through reads; pinch to zoom; shift-scroll to pan."
        } else {
            self.hover.as_str()
        };
        let room = rect.right() - 8.0 - g.right() - 16.0;
        let shown = crate::session_nav::clip(ui, words, &theme::mono(11.0), room);
        painter.text(pos2(g.right() + 16.0, cy), Align2::LEFT_CENTER, shown, theme::mono(11.0), theme::INK);
        window
    }

    /// A wheel or pinch over the reads: zoom, pan, or scroll the lanes.
    fn wheel(&mut self, ui: &Ui, x: f32, width: f32, mut window: Window, n: usize, last_top: usize) -> Window {
        let (scroll, zoom, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        if (zoom - 1.0).abs() > 1e-3 {
            window = window.zoom_at(n, 1.0 / f64::from(zoom), f64::from(x / width));
        } else if mods.shift || scroll.x.abs() > scroll.y.abs() {
            let d = if scroll.x != 0.0 { scroll.x } else { scroll.y };
            window = window.pan_by(n, -f64::from(d) * window.bases / f64::from(width));
        } else if scroll.y.abs() > 0.5 {
            let lines = (scroll.y.abs() / LANE_H).ceil() as usize;
            self.top = if scroll.y < 0.0 { (self.top + lines).min(last_top) } else { self.top.saturating_sub(lines) };
        }
        window
    }

    fn strip(&mut self, ui: &mut Ui, rect: Rect, m: &AlignModel, walk: &[Difference], mut window: Window, n: usize) -> Window {
        let painter = ui.painter().with_clip_rect(rect);
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme::HAIRLINE));
        let depth = Rect::from_min_size(pos2(rect.left(), rect.top() + 4.0), vec2(rect.width(), DEPTH_H));
        let mut called = vec![false; n];
        for d in walk {
            if let Some(span) = called.get_mut(d.column..(d.column + d.size).min(n)) {
                span.fill(true);
            }
        }
        let bins = pileup::depth_bins(&m.output.final_consensus.coverage, &called, (rect.width() / 2.0).round() as usize);
        let top = bins.iter().map(|b| b.0).fold(0.0f64, f64::max).max(1.0);
        let bw = rect.width() / bins.len().max(1) as f32;
        for (i, (v, hit)) in bins.iter().enumerate() {
            let h = (v / top) as f32 * DEPTH_H;
            if h <= 0.0 {
                continue;
            }
            let r =
                Rect::from_min_max(pos2(depth.left() + i as f32 * bw, depth.bottom() - h), pos2(depth.left() + (i + 1) as f32 * bw, depth.bottom()));
            painter.rect_filled(r, 0.0, if *hit { theme::SANA_SCARLET } else { theme::hex(0xc7ccce) });
        }
        painter.line_segment(
            [pos2(depth.left(), depth.bottom() - 0.5), pos2(depth.right(), depth.bottom() - 0.5)],
            Stroke::new(1.0, theme::HAIRLINE_STRONG),
        );
        // The map strip under the depth: features, pins and the window you are in.
        let nav = Rect::from_min_max(pos2(rect.left(), depth.bottom() + 3.0), rect.max);
        let x_of = |c: f64| rect.left() + (c / n.max(1) as f64) as f32 * rect.width();
        for f in m.features {
            let (a, b) = if f.end >= f.start { (f.start, f.end) } else { (f.start, n.saturating_sub(1)) };
            let r = Rect::from_min_max(pos2(x_of(a as f64), nav.top() + 6.0), pos2(x_of(b as f64 + 1.0).max(x_of(a as f64) + 1.0), nav.top() + 12.0));
            painter.rect_filled(r, 2.0, theme::alpha(theme::feature_color(&f.kind), 0.7));
        }
        for d in walk {
            let x = x_of(d.column as f64 + 0.5);
            painter.line_segment([pos2(x, nav.top()), pos2(x, nav.bottom())], Stroke::new(1.5, theme::SANA_SCARLET));
        }
        let wx = Rect::from_min_max(
            pos2(x_of(window.from), rect.top() + 1.0),
            pos2(x_of(window.from + window.bases).max(x_of(window.from) + 3.0), rect.bottom() - 1.0),
        );
        painter.rect_filled(wx, 3.0, theme::alpha(theme::INK, 0.06));
        painter.rect_stroke(wx, 3.0, Stroke::new(1.0, theme::alpha(theme::INK, 0.45)), egui::StrokeKind::Inside);
        let extent = format!("{} bp · {} columns", bios_core::verify::grouped(m.reading.ref_length as i64), bios_core::verify::grouped(n as i64));
        painter.text(pos2(rect.right() - 6.0, nav.bottom() - 2.0), Align2::RIGHT_BOTTOM, extent, theme::sans(10.5), theme::MUTED);
        let resp = ui.interact(rect, ui.scope_id().with("align-strip"), Sense::click_and_drag());
        if resp.drag_started() || resp.clicked() {
            self.strip_drag = true;
        }
        if (resp.dragged() || resp.clicked())
            && self.strip_drag
            && let Some(p) = resp.interact_pointer_pos()
        {
            let c = f64::from((p.x - rect.left()) / rect.width()) * n as f64;
            window = Window::centred(n, c, window.bases);
        }
        if resp.drag_stopped() {
            self.strip_drag = false;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        window
    }

    fn tools(&mut self, ui: &mut Ui, m: &AlignModel, walk: &[Difference], center: f32, bottom: f32, out: &mut Vec<AlignAction>) -> Option<usize> {
        let mut fly = None;
        let count = match self.step {
            Some(s) => format!("{} / {}", s + 1, walk.len()),
            None => format!("– / {}", walk.len()),
        };
        let steps = [
            Segment { face: Face::Icon(crate::lucide::CHEVRON_LEFT, 15.0), active: false, enabled: true, tip: "Previous change (←)" },
            Segment { face: Face::Label(&count), active: false, enabled: false, tip: "" },
            Segment { face: Face::Icon(crate::lucide::CHEVRON_RIGHT, 15.0), active: false, enabled: true, tip: "Next change (→)" },
        ];
        let call = m.run.and_then(|r| r.call);
        let calls = [
            Segment { face: Face::Label("Good  1"), active: call == Some(Call::Good), enabled: true, tip: "Good, use this one (1)" },
            Segment { face: Face::Label("Not good  2"), active: call == Some(Call::Bad), enabled: true, tip: "Not good, discard it (2)" },
            Segment {
                face: Face::Label("Cannot tell  3"),
                active: call == Some(Call::Unsure),
                enabled: true,
                tip: "Cannot tell from this, come back to it (3)",
            },
        ];
        let sw = if walk.is_empty() { 0.0 } else { primitives::group_width(ui, &steps, false) + 24.0 };
        let cw = primitives::group_width(ui, &calls, false);
        let x0 = center - (sw + cw) / 2.0;
        let top = bottom - 34.0;
        if !walk.is_empty() {
            let words = match self.step.and_then(|s| walk.get(s)) {
                Some(d) => {
                    let pos = d.reference_pos;
                    let features: Vec<ChipFeature> =
                        m.output.reference.features.iter().map(|f| ChipFeature { name: f.name.clone(), start: f.start, end: f.end }).collect();
                    mapper::chip_label(pos, d, mapper::feature_at(&features, pos, m.reading.ref_length).as_deref())
                }
                None => format!("{} change{} · press › or →", walk.len(), if walk.len() == 1 { "" } else { "s" }),
            };
            let font = theme::sans_weight(12.0, if self.step.is_some() { Weight::Semibold } else { Weight::Regular });
            let w = text_width(ui, &words, font.clone()) + 20.0;
            let label = Rect::from_center_size(pos2(center, top - 16.0), vec2(w, 22.0));
            ui.painter().rect_filled(label, CornerRadius::same(11), theme::FLOAT);
            ui.painter().rect_stroke(label, CornerRadius::same(11), Stroke::new(1.0, theme::HAIRLINE), egui::StrokeKind::Inside);
            ui.painter().text(
                label.center(),
                Align2::CENTER_CENTER,
                words,
                font,
                if self.step.is_some() { theme::SANA_SCARLET } else { theme::MUTED },
            );
            match primitives::group(ui, pos2(x0, top), &steps, Material::Float, false) {
                Some(0) => fly = self.step_by(walk, -1),
                Some(2) => fly = self.step_by(walk, 1),
                _ => {}
            }
        }
        match primitives::group(ui, pos2(x0 + sw, top), &calls, Material::Float, false) {
            Some(0) => out.push(AlignAction::Call(Call::Good)),
            Some(1) => out.push(AlignAction::Call(Call::Bad)),
            Some(2) => out.push(AlignAction::Call(Call::Unsure)),
            _ => {}
        }
        fly
    }

    /// Lands on change `index` (0-based) the next time the view is drawn, as the stepper would.
    pub fn land_on(&mut self, index: usize) {
        self.pending = Some(index);
    }

    fn step_by(&mut self, walk: &[Difference], by: i32) -> Option<usize> {
        if walk.is_empty() {
            return None;
        }
        let len = walk.len() as i32;
        let next = match (self.step, by) {
            (None, b) if b < 0 => len - 1,
            (None, _) => 0,
            (Some(s), b) => (s as i32 + b).rem_euclid(len),
        };
        self.step = Some(next as usize);
        walk.get(next as usize).map(|d| d.column)
    }

    fn keys(&mut self, ui: &mut Ui, walk: &[Difference], out: &mut Vec<AlignAction>) -> Option<usize> {
        if ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        let mut fly = None;
        ui.input_mut(|i| {
            for (key, call) in [(Key::Num1, Call::Good), (Key::Num2, Call::Bad), (Key::Num3, Call::Unsure)] {
                if i.consume_key(Modifiers::NONE, key) {
                    out.push(AlignAction::Call(call));
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                if self.help {
                    self.help = false;
                } else {
                    out.push(AlignAction::Overview);
                }
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
                out.push(AlignAction::BrowseFile(1));
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
                out.push(AlignAction::BrowseFile(-1));
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                fly = Some(1);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                fly = Some(-1);
            }
        });
        fly.and_then(|by| self.step_by(walk, by))
    }
}

/// The window at the letters around `column`, or the current zoom when already closer.
fn fly(window: Window, n: usize, column: usize, width: f32) -> Window {
    let letters = (f64::from(width / LETTER_PX).floor()).max(MIN_FLY);
    let bases = if window.bases > letters { letters } else { window.bases };
    Window::centred(n, column as f64 + 0.5, bases)
}

fn number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{v:.0}") } else { format!("{v:.2}").trim_end_matches('0').to_string() }
}

/// Features packed into lanes, by start.
fn lanes_of(features: &[ColumnFeature], n: usize) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = features.iter().map(|f| f.start).collect();
    let ends: Vec<usize> = features.iter().map(|f| if f.end >= f.start { f.end } else { n.saturating_sub(1) }).collect();
    let pile = pileup::pack(&starts, &ends, n, 0);
    pile.lane.iter().copied().enumerate().collect()
}

/// The map, linear, in the reads' columns: feature bars in lanes with their names, a pin at every
/// difference. Returns the column of a pin that was clicked.
fn draw_map(ui: &mut Ui, rect: Rect, m: &AlignModel, lanes: &[(usize, usize)], walk: &[Difference], window: Window, n: usize) -> Option<usize> {
    let painter = ui.painter().with_clip_rect(rect);
    let width = rect.width();
    let x_of = |c: f64| rect.left() + window.x_of(c, width);
    let axis = rect.bottom() - 4.0;
    painter.line_segment([pos2(rect.left(), axis), pos2(rect.right(), axis)], Stroke::new(1.0, theme::HAIRLINE_STRONG));
    for &(i, lane) in lanes {
        let Some(f) = m.features.get(i) else { continue };
        let end = if f.end >= f.start { f.end } else { n.saturating_sub(1) };
        let (a, b) = (x_of(f.start as f64), x_of(end as f64 + 1.0));
        if b < rect.left() || a > rect.right() {
            continue;
        }
        let y = axis - 8.0 - (lane as f32 + 1.0) * 18.0 + 4.0;
        let bar = Rect::from_min_max(pos2(a, y), pos2(b.max(a + 1.0), y + 12.0));
        let color = theme::feature_color(&f.kind);
        painter.rect_filled(bar, 3.0, color);
        let visible = Rect::from_min_max(pos2(bar.left().max(rect.left()), bar.top()), pos2(bar.right().min(rect.right()), bar.bottom()));
        if text_width(ui, &f.name, theme::sans(10.5)) + 6.0 < visible.width() {
            painter.text(visible.center(), Align2::CENTER_CENTER, &f.name, theme::sans(10.5), theme::label_color(color));
        }
    }
    let mut clicked = None;
    for d in walk {
        let x = x_of(d.column as f64 + 0.5);
        if x < rect.left() - 4.0 || x > rect.right() + 4.0 {
            continue;
        }
        painter.line_segment([pos2(x, rect.top() + 2.0), pos2(x, axis + 4.0)], Stroke::new(1.5, theme::SANA_SCARLET));
        painter.circle_filled(pos2(x, rect.top() + 4.0), 3.0, theme::SANA_SCARLET);
        let hit = Rect::from_center_size(pos2(x, rect.center().y), vec2(8.0, rect.height()));
        let features: Vec<ChipFeature> =
            m.output.reference.features.iter().map(|f| ChipFeature { name: f.name.clone(), start: f.start, end: f.end }).collect();
        let label = mapper::chip_label(d.reference_pos, d, mapper::feature_at(&features, d.reference_pos, m.reading.ref_length).as_deref());
        let resp = ui.interact(hit, ui.scope_id().with(("pin-hit", d.column)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.on_hover_text(label).clicked() {
            clicked = Some(d.column);
        }
    }
    clicked
}

/// The mapper's files as buttons (`MapperFiles`).
fn files_row(ui: &mut Ui, left: f32, y: f32, out: &mut Vec<AlignAction>) -> f32 {
    let files: [(&str, &'static str); 5] = [
        ("Consensus", "consensus"),
        ("Alignment SAM", "sam"),
        ("Coverage & quality", "coverage"),
        ("Consensus differences", "differences"),
        ("All alignment files", "zip"),
    ];
    let mut x = left;
    for (label, what) in files {
        let mut b = Button::label(label, Material::Ghost);
        b.small = true;
        let w = b.width(ui);
        if b.show_at(ui, pos2(x, y)).clicked() {
            out.push(AlignAction::Download(what));
        }
        x += w + 6.0;
    }
    y + 28.0 + 8.0
}

/// What every colour and shape means, and how the mapper was run (`AlignmentHelpPanel`).
fn help_panel(ui: &mut Ui, reading: &Reading, left: f32, right: f32, y: f32) -> f32 {
    let paragraphs = [
        mapper::header_numbers(reading).join(" · "),
        "The reference and reads share insertion columns. Consensus, coverage and quality come directly from the mapper. ? means no consensus at that position; – is an alignment gap. Faded read bases were trimmed.".to_string(),
        "Scroll through reads, pinch to zoom, and shift-scroll to move along the map. Hover over any base for its exact coverage, consensus quality and read name. The arrows step through consensus differences; these are not a separate variant-calling result.".to_string(),
        "Mappings use five maximum passes, an expected insert size of 200 bp, paired-overhang trimming and weighted-60 final consensus.".to_string(),
    ];
    let width = (right - left).min(720.0);
    let mut galleys = Vec::new();
    let mut h = 12.0;
    for p in paragraphs {
        let g = ui.fonts_mut(|f| f.layout(p, theme::sans(12.0), theme::INK, width - 24.0));
        h += g.size().y + 8.0;
        galleys.push(g);
    }
    let panel = Rect::from_min_size(pos2(left, y), vec2(width, h + 4.0));
    let painter = ui.painter().clone();
    painter.rect_filled(panel, CornerRadius::same(theme::RADIUS_SM as u8), theme::FLOAT);
    painter.rect_stroke(panel, CornerRadius::same(theme::RADIUS_SM as u8), Stroke::new(1.0, theme::HAIRLINE), egui::StrokeKind::Inside);
    let mut gy = panel.top() + 12.0;
    for g in galleys {
        let gh = g.size().y;
        painter.galley(pos2(panel.left() + 12.0, gy), g, theme::INK);
        gy += gh + 8.0;
    }
    panel.bottom() + 10.0
}
