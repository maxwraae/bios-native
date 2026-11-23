//! THE FOLDER IS THE BUILD. This is its page.
//!
//! Ported from the web app's `build/FolderBuild.tsx`. The same object as the console, seen at the
//! other end of its life: a build folder holds a copy of one backbone and every plasmid ever built
//! into it, and this is that, with the verbs — open a design, drop its row, build more into the
//! same backbone, take the order away.
//!
//! Everything on it is what is on disk: the folder's own record and the pieces beside it, and
//! nothing else. The backbone is the folder's copy, never the plasmid on the bench, because the
//! whole reason every design in one folder is comparable is that they all went into the same
//! frozen bases.

use bios_core::Construct;
use bios_layout::overview::viewport::{Projection, Viewport};
use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::build_console::frag_color;
use crate::primitives::{self, Button, Material};
use crate::{map, theme};

/// One design, as the page draws it.
#[derive(Clone, Debug, Default)]
pub struct DesignRow<'a> {
    pub name: String,
    pub bp: usize,
    /// The numbers of the pieces in the table below that this one is made of.
    pub recipe: Vec<usize>,
    /// How many of its own warnings there are to check.
    pub warnings: usize,
    /// `built with an earlier cut`, or against an earlier backbone. Empty when it is current.
    pub note: String,
    /// The plasmid this design made, for the map under it.
    ///
    /// It is a construct in this folder already — nothing is rebuilt to show it. A row with none
    /// draws its two lines and no map, rather than a gap where a picture should be.
    pub product: Option<&'a Construct>,
}

/// One row of the order table.
#[derive(Clone, Debug, Default)]
pub struct PieceRow {
    pub n: usize,
    pub name: String,
    pub length: usize,
    pub sequence: String,
}

pub struct Model<'a> {
    pub folder: &'a str,
    /// The folder's own copy of the backbone, and its length.
    pub backbone: Option<(&'a str, usize)>,
    /// How this folder opens it, as a sentence.
    pub opening: &'a str,
    /// The two bands of the gel: what comes out, and what stays.
    pub digest: Option<(usize, usize)>,
    /// Features the cut clips. One fact about the cut, not one per design.
    pub clipped: &'a [String],
    pub designs: &'a [DesignRow<'a>],
    pub pieces: &'a [PieceRow],
    pub confirming: Option<&'a str>,
    pub note: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenBackbone,
    OpenDesign(String),
    /// Ask before forgetting how one was made.
    ConfirmDrop(String),
    DropDesign(String),
    KeepDesign,
    BuildMore,
    CopyOrder,
    SaveOrder,
}

use crate::build_console::thousands as fmt;

const PAD: f32 = 16.0;
/// How tall the map under one design is: enough for the lane, the names above it and the axis
/// under them.
const DESIGN_MAP_H: f32 = 96.0;
/// Where the axis sits inside that band: just above its floor, so the features stand on it and
/// the band is read from the bottom up, as a figure in a list is.
const AXIS_FROM_FLOOR: f64 = 26.0;

/// Draws the page in `rect` and says what the person did.
pub fn show(ui: &mut Ui, rect: Rect, model: &Model) -> Option<Action> {
    ui.painter().rect_filled(rect, 0.0, theme::CELL);
    let mut action = None;
    let mut y = rect.top() + PAD;

    y = about(ui, rect, model, &mut action, y);
    y = designs(ui, rect, model, &mut action, y);
    to_order(ui, rect, model, &mut action, y);

    // Build more into the same backbone, opened the same way.
    let more = Button { small: true, enabled: model.backbone.is_some(), ..Button::label("Build more", Material::Ghost) };
    let width = more.width(ui);
    if more.show_at(ui, pos2(rect.right() - PAD - width, rect.bottom() - 40.0)).clicked() {
        action = Some(Action::BuildMore);
    }
    action
}

/// The backbone and how it opens, read as a sentence: the first step of the protocol. Both belong
/// here because they are the folder's identity, not a property of any row.
fn about(ui: &mut Ui, rect: Rect, model: &Model, action: &mut Option<Action>, top: f32) -> f32 {
    let painter = ui.painter().with_clip_rect(rect);
    let mut y = top;
    painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, model.folder, theme::serif(20.0), theme::INK);
    let count = format!("{} {}", model.designs.len(), if model.designs.len() == 1 { "design" } else { "designs" });
    painter.text(pos2(rect.right() - PAD, y + 4.0), Align2::RIGHT_TOP, count, theme::sans(12.0), theme::MUTED);
    y += 30.0;

    match model.backbone {
        Some((name, _)) => {
            let font = theme::sans(14.0);
            let width = primitives::text_width(ui, name, font.clone());
            let hit = Rect::from_min_size(pos2(rect.left() + PAD, y), vec2(width, 20.0));
            let response = ui.interact(hit, ui.scope_id().with("fb-backbone"), Sense::click());
            let ink = if response.hovered() { theme::INK } else { theme::ACCENT };
            painter.text(pos2(hit.left(), hit.center().y), Align2::LEFT_CENTER, name, font, ink);
            if response.clicked() {
                *action = Some(Action::OpenBackbone);
            }
            painter.text(pos2(hit.right() + 8.0, hit.center().y), Align2::LEFT_CENTER, model.opening, theme::sans(14.0), theme::MUTED);
        }
        None => {
            let said = format!("The backbone this was built into is not in {} any more.", model.folder);
            painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, said, theme::sans(13.0), theme::DANGER_TEXT);
        }
    }
    y += 28.0;

    // WHAT THE DIGEST GIVES YOU, AS THE TWO THINGS IT GIVES YOU: two fragments, one of which you
    // keep. Drawn to scale against each other, so a small drop-out beside a big backbone looks
    // like what it will look like on the gel.
    if let Some((out, stays)) = model.digest {
        let total = (out + stays).max(1) as f32;
        let bar = Rect::from_min_size(pos2(rect.left() + PAD, y), vec2(rect.width() - 2.0 * PAD, 26.0));
        let keep_w = bar.width() * (stays as f32 / total);
        let keep = Rect::from_min_size(bar.min, vec2(keep_w.max(40.0), bar.height()));
        let gone = Rect::from_min_max(pos2(keep.right() + 2.0, bar.top()), bar.max);
        painter.rect_filled(keep, CornerRadius::same(4), theme::alpha(theme::MUTED, 0.18));
        painter.rect_filled(gone, CornerRadius::same(4), theme::alpha(theme::DANGER_TEXT, 0.18));
        painter.text(
            pos2(keep.left() + 8.0, keep.center().y),
            Align2::LEFT_CENTER,
            format!("{} bp backbone, keep this", fmt(stays)),
            theme::sans(11.0),
            theme::INK,
        );
        if gone.width() > 60.0 {
            painter.text(
                pos2(gone.center().x, gone.center().y),
                Align2::CENTER_CENTER,
                format!("{} bp out", fmt(out)),
                theme::sans(11.0),
                theme::INK,
            );
        } else {
            painter.text(
                pos2(gone.right() + 8.0, gone.center().y),
                Align2::LEFT_CENTER,
                format!("{} bp out", fmt(out)),
                theme::sans(11.0),
                theme::MUTED,
            );
        }
        y += 34.0;
    }
    if !model.clipped.is_empty() {
        let said = if model.clipped.len() == 1 {
            format!("The cut clips 1 feature: {}", model.clipped[0])
        } else {
            format!("The cut clips {} features, on every design here: {}", model.clipped.len(), model.clipped.join(", "))
        };
        painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, said, theme::sans(12.0), theme::DANGER_TEXT);
        y += 22.0;
    }
    if let Some(note) = model.note {
        painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, note, theme::sans(12.0), theme::DANGER_TEXT);
        y += 22.0;
    }
    y + 8.0
}

/// Every design, as two lines: the NAME, and the BLOCKS it takes. The recipe is the sentence a
/// person takes to the bench — backbone plus these numbered pieces from the table below.
fn designs(ui: &mut Ui, rect: Rect, model: &Model, action: &mut Option<Action>, top: f32) -> f32 {
    let painter = ui.painter().with_clip_rect(rect);
    let mut y = section(&painter, rect, "Designs", "the backbone, and the blocks that go in it", top);
    if model.designs.is_empty() {
        painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, "Nothing built into this backbone yet.", theme::sans(13.0), theme::MUTED);
        return y + 30.0;
    }
    for (i, design) in model.designs.iter().enumerate() {
        if y + 48.0 + DESIGN_MAP_H > rect.bottom() - 160.0 {
            let left = model.designs.len() - i;
            painter.text(pos2(rect.left() + PAD, y), Align2::LEFT_TOP, format!("{left} more"), theme::sans(11.0), theme::MUTED);
            y += 20.0;
            break;
        }
        let font = theme::sans(14.0);
        let width = primitives::text_width(ui, &design.name, font.clone());
        let hit = Rect::from_min_size(pos2(rect.left() + PAD, y), vec2(width, 20.0));
        let response = ui.interact(hit, ui.scope_id().with(("fb-design", i)), Sense::click());
        let ink = if response.hovered() { theme::INK } else { theme::ACCENT };
        painter.text(pos2(hit.left(), hit.center().y), Align2::LEFT_CENTER, &design.name, font, ink);
        if response.clicked() {
            *action = Some(Action::OpenDesign(design.name.clone()));
        }
        let mut x = hit.right() + 10.0;
        painter.text(pos2(x, hit.center().y), Align2::LEFT_CENTER, format!("{} bp", fmt(design.bp)), theme::mono(11.0), theme::MUTED);
        x += 80.0;
        if !design.note.is_empty() {
            painter.text(pos2(x, hit.center().y), Align2::LEFT_CENTER, &design.note, theme::sans(11.0), theme::MUTED);
        }
        if design.warnings > 0 {
            let said = format!("{} to check", design.warnings);
            painter.text(pos2(rect.right() - PAD - 90.0, hit.center().y), Align2::RIGHT_CENTER, said, theme::sans(11.0), theme::DANGER_TEXT);
        }

        // Dropping a row is not deleting the plasmid, so it asks first and says which it is.
        if model.confirming == Some(design.name.as_str()) {
            let drop = Button { small: true, ..Button::label("Drop", Material::Ghost) };
            let keep = Button { small: true, ..Button::label("Keep", Material::Ghost) };
            let keep_w = keep.width(ui);
            if keep.show_at(ui, pos2(rect.right() - PAD - keep_w, y)).clicked() {
                *action = Some(Action::KeepDesign);
            }
            let drop_w = drop.width(ui);
            if drop.show_at(ui, pos2(rect.right() - PAD - keep_w - 6.0 - drop_w, y)).clicked() {
                *action = Some(Action::DropDesign(design.name.clone()));
            }
        } else {
            let x = Button { small: true, ..Button::icon(crate::lucide::X, 13.0, Material::Ghost) };
            let tip = "Forget how this one was made. The plasmid stays.";
            if x.show_at(ui, pos2(rect.right() - PAD - 28.0, y)).on_hover_text(tip).clicked() {
                *action = Some(Action::ConfirmDrop(design.name.clone()));
            }
        }
        y += 24.0;

        // The recipe: backbone + the numbered pieces, in the colours the console drew them in.
        let mut x = rect.left() + PAD;
        painter.text(pos2(x, y + 9.0), Align2::LEFT_CENTER, "backbone", theme::sans(11.0), theme::MUTED);
        x += 62.0;
        for (slot, n) in design.recipe.iter().enumerate() {
            painter.text(pos2(x, y + 9.0), Align2::LEFT_CENTER, "+", theme::sans(11.0), theme::MUTED);
            x += 12.0;
            let chip = Rect::from_min_size(pos2(x, y), vec2(20.0, 18.0));
            painter.rect_filled(chip, CornerRadius::same(5), theme::alpha(frag_color(i + slot), 0.22));
            painter.text(chip.center(), Align2::CENTER_CENTER, format!("{n}"), theme::mono(11.0), theme::INK);
            x += 26.0;
        }
        if design.recipe.is_empty() {
            painter.text(pos2(x, y + 9.0), Align2::LEFT_CENTER, "no blocks kept", theme::sans(11.0), theme::MUTED);
        }
        y += 26.0;

        // THE PLASMID, DRAWN BY THE MAP THAT DRAWS EVERY OTHER PLASMID IN THIS APP. A bar of
        // coloured boxes would be an illustration of a plasmid; this is the plasmid, straightened,
        // with the blocks and their joins already on it as real features. A figure in a document,
        // so it takes no wheel and answers no pointer.
        if let Some(product) = design.product {
            let band = Rect::from_min_size(pos2(rect.left() + PAD, y), vec2(rect.width() - 2.0 * PAD, DESIGN_MAP_H));
            // The whole molecule, not a window into it: a row that shows a slice of a design
            // answers a question nobody asked.
            let whole = map::MapView { vp: Viewport { center: 0.5, width: 1.0, projection: Projection::Line }, ring: false };
            // Clipped to its own band: a figure in a page may not write over the row above it.
            map::draw_view(&painter.with_clip_rect(band), band, product, whole, Some(AXIS_FROM_FLOOR));
            y += DESIGN_MAP_H + 6.0;
        }
        y += 6.0;
    }
    y + 8.0
}

/// Every distinct piece across the folder, once, numbered. Four columns and no more, because four
/// is what an order form takes.
fn to_order(ui: &mut Ui, rect: Rect, model: &Model, action: &mut Option<Action>, top: f32) {
    let painter = ui.painter().with_clip_rect(rect);
    let y = section(&painter, rect, "To order", "", top);
    let copy = Button { small: true, enabled: !model.pieces.is_empty(), ..Button::label("Copy", Material::Ghost) };
    let excel = Button { small: true, enabled: !model.pieces.is_empty(), ..Button::label("Excel", Material::Ghost) };
    let excel_w = excel.width(ui);
    if excel.show_at(ui, pos2(rect.right() - PAD - excel_w, top - 2.0)).on_hover_text("Save the table as a spreadsheet").clicked() {
        *action = Some(Action::SaveOrder);
    }
    let copy_w = copy.width(ui);
    if copy.show_at(ui, pos2(rect.right() - PAD - excel_w - 6.0 - copy_w, top - 2.0)).on_hover_text("Copy the table, ready to paste").clicked() {
        *action = Some(Action::CopyOrder);
    }

    let mut row_y = y;
    let head = theme::sans_weight(11.0, theme::Weight::Semibold);
    painter.text(pos2(rect.left() + PAD, row_y), Align2::LEFT_TOP, "#", head.clone(), theme::MUTED);
    painter.text(pos2(rect.left() + PAD + 36.0, row_y), Align2::LEFT_TOP, "name", head.clone(), theme::MUTED);
    painter.text(pos2(rect.left() + PAD + 220.0, row_y), Align2::LEFT_TOP, "length", head.clone(), theme::MUTED);
    painter.text(pos2(rect.left() + PAD + 300.0, row_y), Align2::LEFT_TOP, "sequence", head, theme::MUTED);
    row_y += 20.0;
    for piece in model.pieces {
        if row_y + 20.0 > rect.bottom() - 44.0 {
            let left = model.pieces.len() + 1 - piece.n;
            painter.text(pos2(rect.left() + PAD, row_y), Align2::LEFT_TOP, format!("{left} more"), theme::sans(11.0), theme::MUTED);
            break;
        }
        let mid = row_y + 9.0;
        painter.text(pos2(rect.left() + PAD, mid), Align2::LEFT_CENTER, format!("{}", piece.n), theme::mono(11.0), theme::INK);
        painter.text(pos2(rect.left() + PAD + 36.0, mid), Align2::LEFT_CENTER, &piece.name, theme::sans(12.0), theme::INK);
        painter.text(pos2(rect.left() + PAD + 220.0, mid), Align2::LEFT_CENTER, format!("{} bp", fmt(piece.length)), theme::mono(11.0), theme::MUTED);
        let font = theme::mono(11.0);
        let room = rect.right() - PAD - (rect.left() + PAD + 300.0);
        let shown = crate::session_nav::clip(ui, &piece.sequence, &font, room);
        painter.text(pos2(rect.left() + PAD + 300.0, mid), Align2::LEFT_CENTER, shown, font, theme::alpha(theme::MUTED, 0.9));
        row_y += 20.0;
    }
    let sum = if model.pieces.is_empty() {
        "No pieces kept for these designs.".to_string()
    } else {
        let total: usize = model.pieces.iter().map(|p| p.length).sum();
        format!("{} {} · {} bp", model.pieces.len(), if model.pieces.len() == 1 { "eBlock" } else { "eBlocks" }, fmt(total))
    };
    painter.text(pos2(rect.left() + PAD, row_y + 4.0), Align2::LEFT_TOP, sum, theme::sans(11.0), theme::MUTED);
}

fn section(painter: &egui::Painter, rect: Rect, title: &str, sub: &str, top: f32) -> f32 {
    painter.line_segment([pos2(rect.left() + PAD, top), pos2(rect.right() - PAD, top)], Stroke::new(1.0, theme::HAIRLINE));
    painter.text(pos2(rect.left() + PAD, top + 10.0), Align2::LEFT_TOP, title, theme::sans_weight(13.0, theme::Weight::Semibold), theme::INK);
    if !sub.is_empty() {
        painter.text(pos2(rect.left() + PAD + 90.0, top + 12.0), Align2::LEFT_TOP, sub, theme::sans(11.0), theme::MUTED);
    }
    top + 34.0
}
