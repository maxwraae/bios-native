//! The sheet that says how a place works: open by itself the first time Build or Verify is seen,
//! and on ⌘/ or the ? in the corner after that. Ported from the web's `intro/ModeIntro.tsx`,
//! `intro/BuildIntro.tsx`, `intro/VerifyIntro.tsx` and `intro/intro.css`, with the web's text.
//! Edit has no sheet in the web, so it has none here.
//!
//! The welcome sheet is native only: what a person who has just downloaded Bios needs to know,
//! drawn as the place sheets are, with the line that connects an agent and the page's two tiles.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, CursorIcon, Key, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::theme::{self, Weight};
use crate::{primitives, theme::hex};

/// A mark in the text, standing for what the page draws (`.align-sw`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    /// Grey: agrees with the map.
    Agree,
    /// Red: differs.
    Differ,
    /// Amber: too few reads to say.
    Thin,
    /// Hollow: no reads.
    None,
    DotPick,
    DotCaveat,
    DotFail,
    DotQuiet,
    /// Build's folder blue.
    DotBuild,
}

/// A run of a bullet's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Text(&'static str),
    Bold(&'static str),
    Code(&'static str),
    Mark(Swatch),
}

pub struct Section {
    pub title: &'static str,
    pub items: &'static [&'static [Span]],
}

pub struct Sheet {
    pub title: &'static str,
    pub lede: &'static str,
    pub columns: [&'static [Section]; 2],
}

use Span::{Bold as B, Code as C, Mark as M, Text as T};

pub const BUILD: Sheet = Sheet {
    title: "Build",
    lede: "Put things into this construct, one plasmid each, and get the pieces to order.",
    columns: [
        &[Section {
            title: "Four questions, top to bottom",
            items: &[
                &[
                    B("Where to cut"),
                    T(
                        " \u{2014} the two pickers in the map's tool row. The ring redraws as you pick; click a cut site's name on the map to fill a picker. ",
                    ),
                    B("all"),
                    T(" lets in the enzymes that cut more than once."),
                ],
                &[
                    B("What goes in"),
                    T(
                        " \u{2014} paste inserts (from Excel, a FASTA, one sequence per line), drop files anywhere, upload, or take a construct or a whole folder from the project. A row each, one plasmid each. Names come with the things; nothing is typed.",
                    ),
                ],
                &[B("Settings"), T(" \u{2014} the longest piece your supplier makes and the overlap temperature, in the footer.")],
                &[
                    B("Compute"),
                    T(
                        ", then read the footer before you commit: how many designs, how many pieces, the longest, the temperatures, the warnings. Rows with a problem come to the top. Adjust, recompute, then ",
                    ),
                    B("Build"),
                    T("."),
                ],
            ],
        }],
        &[Section {
            title: "What you get",
            items: &[
                &[M(Swatch::DotBuild), T(" a blue folder in the rail: a copy of the backbone exactly as it was, and every plasmid built into it.")],
                &[T("Each design is a real construct. Open it, edit it, drop reads on it in Verify.")],
                &[T(
                    "Open a row for its pieces, with the overlaps to check. Build more into the same backbone and it joins the folder, so the designs stay comparable.",
                )],
            ],
        }],
    ],
};

pub const VERIFY: Sheet = Sheet {
    title: "Verify",
    lede: "Drop a sequencing run. Every map is a row, every file is a chip on the map it belongs to, and the foot of the page says the next thing to do.",
    columns: [
        &[
            Section {
                title: "Reading a result",
                items: &[
                    &[T(
                        "One line of numbers: how many reads are from this construct, how much of it they covered and how deep, and what differs \u{2014} each kind of change named as what it is.",
                    )],
                    &[
                        M(Swatch::Agree),
                        T(" grey agrees with the map \u{b7} "),
                        M(Swatch::Differ),
                        T(" red differs \u{b7} "),
                        M(Swatch::Thin),
                        T(" amber is too few reads to say \u{b7} "),
                        M(Swatch::None),
                        T(" nothing is no reads. The same four everywhere on the page."),
                    ],
                    &[
                        M(Swatch::DotPick),
                        T(" matches \u{b7} "),
                        M(Swatch::DotCaveat),
                        T(" gaps, or too few reads \u{b7} "),
                        M(Swatch::DotFail),
                        T(" differs \u{b7} "),
                        M(Swatch::DotQuiet),
                        T(" no reads, or not this construct."),
                    ],
                    &[T(
                        "A chip is one file, named by what tells it apart from the others on its map. Its colour is your call and its small dot is what Bios found. Open a row for every file by its whole name and its numbers.",
                    )],
                    &[T("Open a run and its own "), B("?"), T(" explains every colour and shape on the alignment.")],
                ],
            },
            Section {
                title: "Saying what you decided",
                items: &[
                    &[
                        T("The foot says "),
                        B("Call"),
                        T(" and how many are waiting. Press it, or a map's name, and you are on the first file nobody has called. "),
                        B("1"),
                        T(" good, "),
                        B("2"),
                        T(" not good, "),
                        B("3"),
                        T(
                            " cannot tell. Each one records your call and moves to the next file nobody has judged, and the last one brings you back here.",
                        ),
                    ],
                    &[T(
                        "Your call and what Bios found are kept separately, including when they disagree: a green chip with a red dot is you overruling Bios, and it stays visible.",
                    )],
                    &[
                        T("When every file has a call the foot offers "),
                        B("Copy"),
                        T(": one line per file you called good, its name and its map, for the freezer."),
                    ],
                    &[T(
                        "Nothing is ever pre-selected. When Bios is right you press the same key over and over, and the one time it is not, you notice.",
                    )],
                ],
            },
        ],
        &[Section {
            title: "What to drop",
            items: &[
                &[
                    C(".fastq"),
                    T(" or "),
                    C(".fastq.gz"),
                    T(
                        " files and map files together, as many as came back. Bios aligns paired reads on the server. They are kept in the verification alongside the map, so a result can always be traced back to the reads behind it.",
                    ),
                ],
                &[T("The two files of one sample (read 1 and read 2) are paired on their own, by name or by the reads themselves.")],
                &[T(
                    "Maps become rows the moment they land. Bios places each file on the map its name fits; anything it cannot place waits at the top of the page, where you drag it onto a row or press auto-place once the right map is there.",
                )],
                &[T("Nothing is aligned until you press Run. Each drop is kept against a copy of the map as it was that day.")],
            ],
        }],
    ],
};

/// The keys row of each sheet (`KEYS`): which commands, and what to call them.
pub const KEY_ROWS: [&str; 5] = ["Edit", "Build", "Verify", "next construct", "this sheet"];

fn r(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, 255.0) as u8)
}

/// Paints a swatch with its left edge at `x`, centred on `y`; gives back its width.
fn paint_swatch(painter: &egui::Painter, x: f32, y: f32, swatch: Swatch) -> f32 {
    let bar = |fill: Color32, edge: Option<Color32>| {
        let rect = Rect::from_min_size(pos2(x, y - 4.0), vec2(22.0, 8.0));
        painter.rect_filled(rect, r(theme::RADIUS_SM.min(4.0)), fill);
        if let Some(edge) = edge {
            painter.rect_stroke(rect, r(4.0), Stroke::new(1.0, edge), StrokeKind::Inside);
        }
        22.0
    };
    let dot = |fill: Color32, edge: Option<Color32>| {
        let center = pos2(x + 3.5, y);
        painter.circle_filled(center, 3.5, fill);
        if let Some(edge) = edge {
            painter.circle_stroke(center, 2.8, Stroke::new(1.5, edge));
        }
        7.0
    };
    match swatch {
        Swatch::Agree => bar(theme::alpha(theme::INK, 0.13), None),
        Swatch::Differ => bar(theme::SANA_SCARLET, None),
        Swatch::Thin => bar(theme::SANA_AMBER, None),
        Swatch::None => bar(Color32::TRANSPARENT, Some(theme::HAIRLINE_STRONG)),
        Swatch::DotPick => dot(theme::SANA_GREEN, None),
        Swatch::DotCaveat => dot(theme::SANA_AMBER, None),
        Swatch::DotFail => dot(theme::SANA_SCARLET, None),
        Swatch::DotQuiet => dot(Color32::TRANSPARENT, Some(theme::HAIRLINE_STRONG)),
        Swatch::DotBuild => dot(theme::SANA_BLUE, None),
    }
}

/// Lays out one bullet in `width`, with room left in the text for each swatch, and paints it.
/// Gives back its height.
fn bullet(ui: &egui::Ui, painter: &egui::Painter, at: egui::Pos2, width: f32, spans: &[Span]) -> f32 {
    let size = 12.5;
    let mut job = LayoutJob { wrap: egui::text::TextWrapping { max_width: width, ..Default::default() }, ..Default::default() };
    job.halign = egui::Align::LEFT;
    let plain = TextFormat { font_id: theme::sans(size), color: theme::INK, line_height: Some(size * 1.45), ..Default::default() };
    // A swatch is room left before the run after it (`leading_space`), painted there afterwards.
    let mut marks: Vec<(usize, f32, Swatch)> = Vec::new();
    let mut room = 0.0;
    for span in spans {
        let (text, format) = match span {
            Span::Text(t) => (*t, plain.clone()),
            Span::Bold(t) => (*t, TextFormat { font_id: theme::sans_weight(size, Weight::Semibold), ..plain.clone() }),
            Span::Code(t) => (*t, TextFormat { font_id: theme::mono(11.5), ..plain.clone() }),
            Span::Mark(s) => {
                room = if matches!(s, Swatch::Agree | Swatch::Differ | Swatch::Thin | Swatch::None) { 22.0 } else { 7.0 };
                marks.push((job.text.len(), room, *s));
                continue;
            }
        };
        job.append(text, room, format);
        room = 0.0;
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(at, galley.clone(), theme::INK);
    for (byte, room, swatch) in marks {
        let index = galley.text().get(..byte).map_or(0, |s| s.chars().count());
        let rect = galley.pos_from_cursor(egui::text::CCursor::new(index));
        paint_swatch(painter, at.x + rect.min.x - room, at.y + rect.center().y, swatch);
    }
    // The bullet itself.
    painter.circle_filled(pos2(at.x - 9.0, at.y + size * 1.45 / 2.0), 2.0, theme::INK);
    galley.size().y
}

/// Draws `sheet` over `area` (`.intro-veil` and `.intro-sheet`), with the key rows as `keys`
/// (each shortcut's text and what it does; an unbound one is left out). True when it closes.
pub fn show(ui: &mut egui::Ui, area: Rect, sheet: &Sheet, keys: &[(String, &str)]) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique("intro-veil")).order(egui::Order::Foreground).fixed_pos(area.min).fade_in(false).show(ui.ctx(), |ui| {
        let veil = ui.interact(area, ui.scope_id().with("intro-veil"), Sense::click());
        ui.painter().rect_filled(area, 0.0, theme::alpha(theme::CANVAS, 0.85));
        let width = (area.width() - 48.0).min(820.0);
        let inner_w = width - 64.0;
        let col_w = (inner_w - 32.0) / 2.0;
        let left = area.center().x - width / 2.0;
        let top = area.top() + 48.0;
        // Measure, then draw: the sheet is as tall as its taller column.
        let painter = ui.painter().clone();
        let lede_galley = ui.fonts_mut(|f| f.layout(sheet.lede.to_string(), theme::sans(14.0), theme::MUTED, inner_w - 120.0));
        let head_h = 30.0 + 6.0 + lede_galley.size().y + 20.0;
        let measure = ui.painter().with_clip_rect(Rect::NOTHING);
        let column_h = |sections: &[Section], with_keys: bool, painter: &egui::Painter, x: f32, y0: f32| {
            let mut y = y0;
            for (i, section) in sections.iter().enumerate() {
                if i > 0 {
                    y += 14.0;
                }
                painter.text(
                    pos2(x, y + 7.0),
                    Align2::LEFT_CENTER,
                    section.title.to_uppercase(),
                    theme::sans_weight(11.0, Weight::Semibold),
                    theme::INK,
                );
                y += 14.0 + 5.0;
                for item in section.items {
                    y += bullet(ui, painter, pos2(x + 16.0, y), col_w - 16.0, item) + 4.0;
                }
            }
            if with_keys && !keys.is_empty() {
                y += 14.0;
                painter.text(pos2(x, y + 7.0), Align2::LEFT_CENTER, "KEYS", theme::sans_weight(11.0, Weight::Semibold), theme::INK);
                y += 14.0 + 5.0;
                let key_w = keys.iter().map(|(k, _)| primitives::text_width(ui, k, theme::mono(11.5))).fold(0.0, f32::max);
                for (chord, label) in keys {
                    painter.text(pos2(x, y + 9.0), Align2::LEFT_CENTER, chord, theme::mono(11.5), theme::MUTED);
                    painter.text(pos2(x + key_w + 12.0, y + 9.0), Align2::LEFT_CENTER, *label, theme::sans(12.5), theme::INK);
                    y += 18.0 + 4.0;
                }
            }
            y - y0
        };
        let body_h = column_h(sheet.columns[0], false, &measure, 0.0, 0.0).max(column_h(sheet.columns[1], true, &measure, 0.0, 0.0));
        let height = 28.0 + head_h + body_h + 30.0;
        let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
        let inside = ui.interact(rect, ui.scope_id().with("intro-sheet"), Sense::click());
        if veil.clicked() && !inside.hovered() {
            close = true;
        }
        let (x0, y0) = head(ui, &painter, rect, sheet.title, lede_galley, &mut close);
        let body_top = y0 + head_h;
        column_h(sheet.columns[0], false, &painter, x0, body_top);
        column_h(sheet.columns[1], true, &painter, x0 + col_w + 32.0, body_top);
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    close
}

/// The sheet's card: its shadow, its white and its edge.
fn paint_card(painter: &egui::Painter, rect: Rect) {
    primitives::shadow(painter, rect, theme::RADIUS_TAB, theme::SHADOW_CELL);
    painter.rect_filled(rect, r(theme::RADIUS_TAB), theme::FLOAT);
    painter.rect_stroke(rect, r(theme::RADIUS_TAB), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
}

/// Paints the card, its title and lede, and Done in the top right; gives back where the text
/// starts. Sets `close` when Done is pressed.
fn head(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, title: &str, lede: std::sync::Arc<egui::Galley>, close: &mut bool) -> (f32, f32) {
    paint_card(painter, rect);
    let x0 = rect.left() + 32.0;
    let y0 = rect.top() + 28.0;
    painter.text(pos2(x0, y0), Align2::LEFT_TOP, title, theme::serif(30.0), theme::INK);
    painter.galley(pos2(x0, y0 + 30.0 + 6.0), lede, theme::MUTED);
    let done_w = pill_width(ui, "Done", Some("esc"));
    let done = Rect::from_min_size(pos2(rect.right() - 32.0 - done_w, y0), vec2(done_w, 30.0));
    if pill(ui, painter, done, "Done", Some("esc"), false) {
        *close = true;
    }
    (x0, y0)
}

/// How wide a pill is for its words and key.
fn pill_width(ui: &egui::Ui, text: &str, key: Option<&str>) -> f32 {
    let key_w = key.map_or(0.0, |k| 8.0 + primitives::text_width(ui, k, theme::mono(11.5)));
    primitives::text_width(ui, text, theme::sans(13.0)) + key_w + 24.0
}

/// A button in the sheet, drawn as Done is: words on the left, its key on the right. The primary
/// one is ink. True when pressed.
fn pill(ui: &egui::Ui, painter: &egui::Painter, at: Rect, text: &str, key: Option<&str>, primary: bool) -> bool {
    let response = ui.interact(at, ui.scope_id().with(("intro-pill", text)), Sense::click());
    let (fill, ink) = match (primary, response.hovered()) {
        (true, false) => (theme::ACCENT, theme::ON_ACCENT),
        (true, true) => (theme::ACCENT_HOVER, theme::ON_ACCENT),
        (false, false) => (theme::CELL, theme::INK),
        (false, true) => (theme::CELL_HOVER, theme::INK),
    };
    painter.rect_filled(at, r(theme::RADIUS_SM), fill);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    painter.text(pos2(at.left() + 12.0, at.center().y), Align2::LEFT_CENTER, text, theme::sans(13.0), ink);
    if let Some(key) = key {
        painter.text(pos2(at.right() - 12.0, at.center().y), Align2::RIGHT_CENTER, key, theme::mono(11.5), theme::MUTED);
    }
    response.clicked()
}

pub const WELCOME_TITLE: &str = "Welcome to Bios";
pub const WELCOME_LEDE: &str = "Four things to know before you start.";

/// The four points, each led by its sentence in bold.
pub const WELCOME: [&[Span]; 4] = [
    &[
        B("A project is a folder."),
        T(" Make a new one or open any folder on your Mac. Your plasmids stay there as ordinary files any other tool can read."),
    ],
    &[B("Every change is kept."), T(" Go back to any version, whether you made the change or your agent did.")],
    &[
        B("Bring your own agent."),
        T(" Connect Claude, or any agent that speaks MCP, and it can do everything Bios can do. For Claude Code, run this in a terminal:"),
    ],
    &[B("Bios updates itself."), T(" New versions arrive on their own.")],
];

/// Which point the connect line stands under.
const WELCOME_CONNECT: usize = 2;

/// What was pressed on the welcome sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Welcomed {
    /// Done, esc, or a click on the veil.
    Close,
    /// Copy, beside the connect line.
    Copy,
    NewProject,
    OpenFolder,
}

/// Draws the welcome sheet over `area`, with `connect` as the line that connects Claude Code;
/// `copied` says Copied on its button. Gives back what was pressed.
pub fn welcome(ui: &mut egui::Ui, area: Rect, connect: &str, copied: bool) -> Option<Welcomed> {
    let mut chosen = None;
    egui::Area::new(egui::Id::unique("welcome-veil")).order(egui::Order::Foreground).fixed_pos(area.min).fade_in(false).show(ui.ctx(), |ui| {
        let veil = ui.interact(area, ui.scope_id().with("welcome-veil"), Sense::click());
        ui.painter().rect_filled(area, 0.0, theme::alpha(theme::CANVAS, 0.85));
        let width = (area.width() - 48.0).min(640.0);
        let inner_w = width - 64.0;
        let left = area.center().x - width / 2.0;
        let top = area.top() + 48.0;
        let painter = ui.painter().clone();
        let lede = ui.fonts_mut(|f| f.layout(WELCOME_LEDE.to_string(), theme::sans(14.0), theme::MUTED, inner_w - 120.0));
        let head_h = 30.0 + 6.0 + lede.size().y + 20.0;
        // The connect line, in a well with Copy at its right.
        let copy_text = if copied { "Copied" } else { "Copy" };
        let copy_w = pill_width(ui, "Copied", None);
        let well_w = inner_w - 16.0;
        let line = ui.fonts_mut(|f| f.layout(connect.to_string(), theme::mono(11.5), theme::INK, well_w - 24.0 - copy_w - 8.0));
        let well_h = (line.size().y + 20.0).max(42.0);
        // Measure, then draw.
        let points = |painter: &egui::Painter, x: f32, y0: f32| {
            let mut y = y0;
            let mut well_top = y0;
            for (i, point) in WELCOME.iter().enumerate() {
                if i > 0 {
                    y += 10.0;
                }
                y += bullet(ui, painter, pos2(x + 16.0, y), inner_w - 16.0, point);
                if i == WELCOME_CONNECT {
                    well_top = y + 8.0;
                    y = well_top + well_h;
                }
            }
            (y - y0, well_top)
        };
        let (body_h, _) = points(&ui.painter().with_clip_rect(Rect::NOTHING), 0.0, 0.0);
        let height = 28.0 + head_h + body_h + 28.0 + 30.0 + 30.0;
        let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
        let inside = ui.interact(rect, ui.scope_id().with("welcome-sheet"), Sense::click());
        if veil.clicked() && !inside.hovered() {
            chosen = Some(Welcomed::Close);
        }
        let mut close = false;
        let (x0, y0) = head(ui, &painter, rect, WELCOME_TITLE, lede, &mut close);
        if close {
            chosen = Some(Welcomed::Close);
        }
        let (_, well_top) = points(&painter, x0, y0 + head_h);
        let well = Rect::from_min_size(pos2(x0 + 16.0, well_top), vec2(well_w, well_h));
        painter.rect_filled(well, r(theme::RADIUS_SM), theme::CELL_SUNKEN);
        painter.galley(pos2(well.left() + 12.0, well.center().y - line.size().y / 2.0), line, theme::INK);
        let copy = Rect::from_min_size(pos2(well.right() - 8.0 - copy_w, well.center().y - 15.0), vec2(copy_w, 30.0));
        if pill(ui, &painter, copy, copy_text, None, false) {
            chosen = Some(Welcomed::Copy);
        }
        // The page's two tiles, as buttons at the foot.
        let foot = rect.bottom() - 30.0 - 30.0;
        let new_w = pill_width(ui, "New project", None);
        let open_w = pill_width(ui, "Open folder", None);
        let new = Rect::from_min_size(pos2(rect.right() - 32.0 - new_w, foot), vec2(new_w, 30.0));
        let open = Rect::from_min_size(pos2(new.left() - 8.0 - open_w, foot), vec2(open_w, 30.0));
        if pill(ui, &painter, open, "Open folder", None, false) {
            chosen = Some(Welcomed::OpenFolder);
        }
        if pill(ui, &painter, new, "New project", None, true) {
            chosen = Some(Welcomed::NewProject);
        }
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        chosen = Some(Welcomed::Close);
    }
    chosen
}

/// The ? in a corner of the work column, 16 from its walls (`.intro-btn`): the bottom right, or
/// the top right where a place keeps its own actions at the foot. True when pressed.
pub fn button(ui: &mut egui::Ui, area: Rect, top: bool, open: bool, tip: &str) -> bool {
    let y = if top { area.top() + 16.0 } else { area.bottom() - 16.0 - 32.0 };
    let rect = Rect::from_min_size(pos2(area.right() - 16.0 - 32.0, y), vec2(32.0, 32.0));
    let response = ui.interact(rect, ui.scope_id().with("intro-btn"), Sense::click());
    primitives::glass(ui.painter(), rect, theme::RADIUS_SM);
    if open || response.hovered() {
        ui.painter().rect_filled(rect, r(theme::RADIUS_SM), theme::alpha(theme::INK, 0.06));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "?", theme::sans(15.0), if open { theme::INK } else { hex(0x3a3a3c) });
    response.on_hover_text(tip).clicked()
}
