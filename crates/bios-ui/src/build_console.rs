//! BUILD: where the backbone opens, on the map that answers it, and what goes in, under it.
//!
//! Ported from the web app's `build/CutBar.tsx` and `build/BuildConsole.tsx`.
//!
//! Two questions, one screen. "Where do we cut" is two glass pills in the map's own tool row — the
//! pickers and the arrow that takes the other arc in one, unique | all in the other, and the
//! button that says yes to what they chose. While that is the question the map is the whole pane;
//! once it is answered the map becomes a strip and the inserts have the screen.
//!
//! What is deliberately NOT in the console: arm Tm, overlap length, junction thermodynamics.
//! Mechanical correctness is guaranteed, so it goes quiet; a checklist of numbers that are always
//! fine is noise you learn to skip. What IS here, loudly, is what the cut does to the parent's
//! annotations, because "did you mean to cut through AmpR?" is a question only a person can answer.

use egui::{Align2, Color32, CornerRadius, Id, Key, Margin, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::{self, Button, Face, Material, Segment, Tone};
use crate::theme;
use crate::{icon, lucide};

/// Which of Build's two questions is on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Where to cut: the map owns the pane.
    #[default]
    Cut,
    /// What goes in: the map is a strip.
    Fill,
}

/// Which of the two arcs a pair of cuts defines is the one that leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arc {
    #[default]
    Between,
    Origin,
}

/// Which construct the map is showing: the one you started with, or the one you are making.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// The backbone, with the design drawn on it.
    #[default]
    Parent,
    /// The plasmid this makes.
    Product,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CutMode {
    #[default]
    Enzymes,
    Region,
}

/// What the person has typed or chosen. Held by the window, changed only through [`Action`].
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub cut_mode: CutMode,
    pub enzyme_a: String,
    pub enzyme_b: String,
    pub arc: Arc,
    /// 1-based, inclusive, as typed. Text so a field can be empty.
    pub region_start: String,
    pub region_end: String,
    pub target_tm: String,
    pub max_piece: String,
    /// What the product will be called, when there is exactly one.
    pub name: String,
    /// Which construct the map draws: the parent, or what this makes.
    pub stage: Stage,
    /// The paste field's own text, until it becomes rows.
    pub paste: String,
    /// How far down the list has been scrolled, in pixels.
    pub scroll: f32,
}

/// One insert on the list, and what the design made of it.
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub name: String,
    pub bases: String,
    /// `3 pieces · 1 warning`, once there is a design.
    pub chip: String,
    pub warn: bool,
    /// One length per piece to order, in order: the bar is drawn from these.
    pub pieces: Vec<usize>,
    /// The arms the end pieces carry, so they can be drawn inside those pieces.
    pub arms: (usize, usize),
    /// What it costs to order, under the row, once there is a design.
    pub order_line: String,
}

/// Where the backbone opens, as the bar says it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opening {
    pub removed: usize,
    /// The two arcs the pair defines, so the swap can say what it would take instead.
    pub between: usize,
    pub origin: usize,
}

/// An enzyme the backbone can be opened with.
#[derive(Clone, Debug)]
pub struct EnzymePick {
    pub enzyme: String,
    pub count: usize,
}

/// What the console is drawing.
pub struct Model<'a> {
    pub rows: &'a [Row],
    /// The order the rows are drawn in: once computed, the ones with a problem come first.
    pub order: &'a [usize],
    pub options: &'a [EnzymePick],
    pub opening: Option<Opening>,
    pub phase: Phase,
    pub all_cutters: bool,
    /// How far a compute has got, while one is running.
    pub progress: Option<(usize, usize)>,
    /// The inputs have moved since the designs were built (or nothing has been built).
    pub stale: bool,
    /// The row whose design is on the map.
    pub preview: usize,
    /// A design is on screen, so there is a product to scrub to.
    pub designed: bool,
    /// The sentence along the bottom: the count before Compute, the overview after it.
    pub summary: String,
    /// How many plasmids Build is about to make.
    pub buildable: usize,
    pub busy: bool,
    /// Something that went wrong or is worth saying, beside the summary.
    pub note: Option<&'a str>,
}

/// What the person did.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    PickEnzyme {
        second: bool,
        enzyme: String,
    },
    /// Open the picker for the first or second enzyme at this point.
    OpenPicker {
        second: bool,
        at: egui::Pos2,
    },
    SwapArc,
    SetStage(Stage),
    AllCutters(bool),
    SetPhase(Phase),
    /// Text pasted or typed into the intake field.
    Take(String),
    AddConstruct(egui::Pos2),
    AddFolder(egui::Pos2),
    /// Sequence files or sheets from this machine.
    Upload,
    ClearRows,
    RemoveRow(usize),
    /// Open a row: its design goes on the map.
    OpenRow(usize),
    Compute,
    CopyPieces,
    Build,
    /// A field was edited; the window holds the text.
    Edited,
}

/// A count as a person writes one: `6,093`. One place, so the console and the folder's page
/// cannot disagree about how a number looks.
pub fn thousands(n: usize) -> String {
    let text = n.to_string();
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i > 0 && (text.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn bp(n: usize) -> String {
    format!("{} bp", thousands(n))
}

/// The fragment palette: each row owns a hue, and its pieces are that hue onward, so the row in
/// the list, the bar on it and the fragments drawn on the map are one colour for one design.
pub fn frag_color(i: usize) -> Color32 {
    const PALETTE: [Color32; 6] = [
        Color32::from_rgb(0x23, 0xac, 0x00),
        Color32::from_rgb(0x04, 0x5a, 0xfa),
        Color32::from_rgb(0x70, 0x01, 0xe1),
        Color32::from_rgb(0xc8, 0x26, 0x8c),
        Color32::from_rgb(0xfd, 0xce, 0x0b),
        Color32::from_rgb(0xf8, 0x76, 0x28),
    ];
    PALETTE[i % PALETTE.len()]
}

// ── THE CUT BAR, ON THE MAP ─────────────────────────────────────────────────────────────────

/// The bar across the top of the map: the pickers, the arc swap, unique | all, and the button
/// that accepts the opening. The same controls in both phases, and one button that changes its
/// mind — it takes the map down to a strip when the map owns the screen, and brings it back up
/// when it does not.
pub fn cut_bar(ui: &mut Ui, map: Rect, model: &Model, inputs: &mut Inputs) -> Option<Action> {
    let mut action = None;
    let top = map.top() + 10.0;
    let mut x = map.left() + 12.0;

    // The pair, or the two numbers in region mode.
    match inputs.cut_mode {
        CutMode::Enzymes => {
            for (second, chosen) in [(false, inputs.enzyme_a.clone()), (true, inputs.enzyme_b.clone())] {
                let label = if chosen.is_empty() {
                    if second { "Second enzyme…".to_string() } else { "First enzyme…".to_string() }
                } else {
                    let count = model.options.iter().find(|o| o.enzyme == chosen).map_or(1, |o| o.count);
                    format!("{chosen} · {}", if count == 1 { "1 site".to_string() } else { format!("{count} sites") })
                };
                let button = Button::label(&label, Material::Glass);
                let width = button.width(ui);
                let at = pos2(x, top);
                if button.show_at(ui, at).clicked() {
                    action = Some(Action::OpenPicker { second, at: pos2(at.x, at.y + 36.0) });
                }
                x += width + 6.0;
            }
        }
        CutMode::Region => {
            for (which, value) in [("from", &mut inputs.region_start), ("to", &mut inputs.region_end)] {
                if number_field(ui, pos2(x, top), 76.0, which, which, value) {
                    action = Some(Action::Edited);
                }
                x += 76.0 + 6.0;
            }
        }
    }

    // TWO CUTS ON A CIRCLE DEFINE TWO ARCS, and the pair alone cannot say which you meant. On most
    // plasmids the other one is the origin and the resistance marker, which is why this is a
    // control and not a default.
    let swappable = model.opening.is_some_and(|o| o.origin > 0);
    let swap_tip = match model.opening {
        Some(o) if swappable => {
            format!("Take out the other arc ({} instead)", bp(if inputs.arc == Arc::Between { o.origin } else { o.between }))
        }
        _ => "Take out the other arc".to_string(),
    };
    let swap = Button { enabled: swappable, ..Button::label("other arc", Material::Glass) };
    let width = swap.width(ui);
    if swap.show_at(ui, pos2(x, top)).on_hover_text(&swap_tip).clicked() {
        action = Some(Action::SwapArc);
    }
    x += width + 10.0;

    // THE SCRUB BELONGS ON THE MAP IT SCRUBS: what it changes is what the map draws, so it lives
    // with the map's own controls — and only once there is a design, because before that the
    // plasmid it would show does not exist yet.
    if model.designed {
        let stages = [
            Segment {
                face: Face::Label("before"),
                active: inputs.stage == Stage::Parent,
                enabled: true,
                tip: "The backbone, with the design drawn on it",
            },
            Segment { face: Face::Label("after"), active: inputs.stage == Stage::Product, enabled: true, tip: "The plasmid this makes" },
        ];
        let stage_w = primitives::group_width(ui, &stages, false);
        if let Some(i) = primitives::group(ui, pos2(x, top), &stages, Material::Glass, false) {
            action = Some(Action::SetStage(if i == 0 { Stage::Parent } else { Stage::Product }));
        }
        x += stage_w + 10.0;
    }

    // Unique is the default because a unique cutter is an opening; `all` is one press away,
    // because nothing is ever unavailable.
    let cutters = [
        Segment { face: Face::Label("unique"), active: !model.all_cutters, enabled: true, tip: "Only the enzymes that cut once" },
        Segment { face: Face::Label("all"), active: model.all_cutters, enabled: true, tip: "Every enzyme that cuts this backbone" },
    ];
    let group_w = primitives::group_width(ui, &cutters, false);
    if let Some(i) = primitives::group(ui, pos2(x, top), &cutters, Material::Glass, false) {
        action = Some(Action::AllCutters(i == 1));
    }
    x += group_w + 10.0;

    // THE ONE THING THE OPENING SAYS ABOUT ITSELF, on the button that accepts it: how much
    // leaves. Nothing is computed by pressing it — the design waits for its inserts — so it is
    // not a commitment, it is the screen moving on.
    let says = match (model.phase, model.opening) {
        (Phase::Cut, Some(o)) => format!("{} out · yes", bp(o.removed)),
        (Phase::Cut, None) => "pick two sites".to_string(),
        (Phase::Fill, Some(o)) => format!("{} out", bp(o.removed)),
        (Phase::Fill, None) => "no cut".to_string(),
    };
    let accept = Button {
        tone: if model.phase == Phase::Cut { Tone::Solid } else { Tone::Ghost },
        enabled: model.phase == Phase::Fill || model.opening.is_some(),
        ..Button::label(&says, Material::Glass)
    };
    if accept.show_at(ui, pos2(x, top)).clicked() {
        action = Some(Action::SetPhase(if model.phase == Phase::Cut { Phase::Fill } else { Phase::Cut }));
    }
    action
}

// ── THE CONSOLE ─────────────────────────────────────────────────────────────────────────────

/// A row's slot: one line before Compute, two after — every row the same, so the window can place
/// row N without measuring it.
const ROW_H: f32 = 34.0;
const ROW_H_2: f32 = 52.0;
const HEAD_H: f32 = 48.0;
const BOTTOM_H: f32 = 92.0;

/// Draws the console in `rect` and says what the person did.
pub fn console(ui: &mut Ui, rect: Rect, model: &Model, inputs: &mut Inputs) -> Option<Action> {
    ui.painter().rect_filled(rect, 0.0, theme::CELL);
    let mut action = intake(ui, Rect::from_min_size(rect.min, vec2(rect.width(), HEAD_H)), model, inputs);
    let list = Rect::from_min_max(pos2(rect.left(), rect.top() + HEAD_H), pos2(rect.right(), rect.bottom() - BOTTOM_H));
    if let Some(from_list) = rows(ui, list, model, inputs) {
        action = Some(from_list);
    }
    let bottom = Rect::from_min_max(pos2(rect.left(), rect.bottom() - BOTTOM_H), rect.max);
    if let Some(from_bottom) = foot(ui, bottom, model, inputs) {
        action = Some(from_bottom);
    }
    action
}

/// The ways in come first, then the field they fill: left to right is the order you do them in.
fn intake(ui: &mut Ui, rect: Rect, model: &Model, inputs: &mut Inputs) -> Option<Action> {
    let mut action = None;
    let mut x = rect.left() + 14.0;
    let top = rect.top() + 7.0;
    // THE WAYS IN, left to right in the order you do them in: files from this machine, a
    // construct, a whole folder.
    if Button::icon(lucide::UPLOAD, 15.0, Material::Ghost)
        .show_at(ui, pos2(x, top))
        .on_hover_text("Sequence files or a sheet from this machine")
        .clicked()
    {
        action = Some(Action::Upload);
    }
    x += 34.0 + 6.0;
    for (nodes, tip, folder) in
        [(lucide::DNA, "A construct from this project", false), (lucide::FOLDER_OPEN, "Every construct in a folder, one plasmid each", true)]
    {
        let at = pos2(x, top);
        if Button::icon(nodes, 15.0, Material::Ghost).show_at(ui, at).on_hover_text(tip).clicked() {
            let below = pos2(at.x, at.y + 36.0);
            action = Some(if folder { Action::AddFolder(below) } else { Action::AddConstruct(below) });
        }
        x += 34.0 + 6.0;
    }
    if !model.rows.is_empty() {
        let clear = Button::icon(lucide::X, 15.0, Material::Ghost);
        if clear.show_at(ui, pos2(x, top)).on_hover_text("Take every sequence off the list").clicked() {
            action = Some(Action::ClearRows);
        }
        x += 34.0 + 8.0;
    }

    // A glass bar the shape of a search field. Pasting is the thing you do here, so it is a field
    // and never a button that becomes one.
    let field = Rect::from_min_max(pos2(x, top), pos2(rect.right() - 14.0, top + 34.0));
    primitives::glass(ui.painter(), field, 17.0);
    icon::paint(ui.painter(), pos2(field.left() + 16.0, field.center().y), 14.0, lucide::CLIPBOARD_PASTE, theme::MUTED);
    let font = theme::sans(13.0);
    let id = Id::unique("build-paste");
    let edit = egui::TextEdit::singleline(&mut inputs.paste)
        .id(id)
        .frame(egui::Frame::NONE.inner_margin(Margin::ZERO))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(egui::RichText::new("Paste sequences here").font(font).color(theme::MUTED))
        .align(Align2::LEFT_CENTER);
    let text = ui.put(Rect::from_min_max(pos2(field.left() + 32.0, field.top() + 7.0), pos2(field.right() - 12.0, field.bottom() - 7.0)), edit);
    // On ⏎, and on leaving the field with something in it, the text becomes rows.
    let entered = text.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if (entered || (text.lost_focus() && !inputs.paste.trim().is_empty())) && !inputs.paste.trim().is_empty() {
        action = Some(Action::Take(std::mem::take(&mut inputs.paste)));
    }
    action
}

/// How many rows either side of the window are drawn anyway, so a fast scroll never shows a gap.
const OVERDRAW: usize = 4;

/// Which rows the viewport is over, as a half-open range into the list's own order.
fn window(scroll: f32, row_h: f32, view_h: f32, rows: usize) -> (usize, usize) {
    if row_h <= 0.0 {
        return (0, rows);
    }
    let first = ((scroll / row_h).floor().max(0.0) as usize).saturating_sub(OVERDRAW);
    let last = ((((scroll + view_h) / row_h).ceil().max(0.0) as usize) + OVERDRAW).min(rows);
    (first.min(rows), last)
}

fn rows(ui: &mut Ui, rect: Rect, model: &Model, inputs: &mut Inputs) -> Option<Action> {
    let painter = ui.painter().with_clip_rect(rect);
    let computed = model.rows.iter().any(|r| !r.pieces.is_empty());
    let head = pos2(rect.left() + 14.0, rect.top() + 14.0);
    painter.text(
        head,
        Align2::LEFT_TOP,
        if computed { "Designs" } else { "Inserts" },
        theme::sans_weight(11.0, theme::Weight::Semibold),
        theme::MUTED,
    );
    if !model.rows.is_empty() {
        let count = format!("{} {}", model.rows.len(), if model.rows.len() == 1 { "sequence" } else { "sequences" });
        painter.text(pos2(rect.right() - 14.0, rect.top() + 14.0), Align2::RIGHT_TOP, count, theme::sans(11.0), theme::MUTED);
    }
    if model.rows.is_empty() {
        // An empty list is a question, and it says what to do where you are looking.
        let hint =
            "Nothing to put in yet. Paste sequences above, drop files anywhere on this page, or take a construct or a whole folder from the project.";
        painter.text(pos2(rect.left() + 14.0, rect.top() + 46.0), Align2::LEFT_TOP, hint, theme::sans(13.0), theme::MUTED);
        return None;
    }

    let mut action = None;
    let row_h = if computed { ROW_H_2 } else { ROW_H };
    let longest = model.rows.iter().map(|r| r.bases.len()).max().unwrap_or(1).max(1);

    // ── THE WINDOW ──────────────────────────────────────────────────────────────────────────
    //
    // Only the rows the viewport is over are drawn, plus a few either side, so the list is the
    // same list at three inserts and at a million. Every row is the same height, which is what
    // lets row N be placed without measuring the ones above it.
    let list = Rect::from_min_max(pos2(rect.left(), rect.top() + 34.0), rect.max);
    let total = model.order.len() as f32 * row_h;
    let room = (total - list.height()).max(0.0);
    if ui.rect_contains_pointer(list) {
        inputs.scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    inputs.scroll = inputs.scroll.clamp(0.0, room);
    let scroll = inputs.scroll;
    let (first, last) = window(scroll, row_h, list.height(), model.order.len());

    for (place, &which) in model.order.iter().enumerate().take(last).skip(first) {
        let Some(row) = model.rows.get(which) else {
            continue;
        };
        let y = list.top() + place as f32 * row_h - scroll;
        let r = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, row_h - 2.0));
        if r.bottom() < list.top() || r.top() > list.bottom() {
            continue;
        }
        let response = ui.interact(r, ui.scope_id().with(("build-row", which)), Sense::click());
        primitives::row_ground(&painter, r, response.hovered(), which == model.preview);
        if response.clicked() {
            action = Some(Action::OpenRow(which));
        }
        row_face(ui, &painter, r, which, row, longest, computed);
        // The way off the list is a pressable, and a pressable is not clipped by a painter: a row
        // half out of the window would hang its cross below the list.
        if r.bottom() <= list.bottom() {
            let x = Button { small: true, ..Button::icon(lucide::X, 13.0, Material::Ghost) };
            if x.show_at(ui, pos2(r.right() - 32.0, r.top() + 3.0)).on_hover_text("Remove").clicked() {
                action = Some(Action::RemoveRow(which));
            }
        }
    }
    // Where you are in the list, when there is more of it than there is pane. A hairline thumb:
    // the rows are the thing, and a scrollbar drawn like furniture would compete with them.
    if room > 0.0 {
        let share = (list.height() / total).clamp(0.08, 1.0);
        let travel = list.height() * (1.0 - share);
        let top = list.top() + travel * (scroll / room);
        let thumb = Rect::from_min_size(pos2(list.right() - 7.0, top), vec2(3.0, list.height() * share));
        painter.rect_filled(thumb, CornerRadius::same(2), theme::alpha(theme::MUTED, 0.35));
    }
    action
}

/// Four columns — which one, what it is called, how long, and its bases — then what the design
/// made of it, at the end, where the eye lands last.
fn row_face(ui: &Ui, painter: &egui::Painter, r: Rect, which: usize, row: &Row, longest: usize, computed: bool) {
    let mid = if computed { r.top() + 17.0 } else { r.center().y };
    let mut x = r.left() + 10.0;
    painter.text(pos2(x, mid), Align2::LEFT_CENTER, format!("{}", which + 1), theme::mono(11.0), theme::MUTED);
    x += 26.0;
    let name = if row.name.is_empty() { format!("Insert {}", which + 1) } else { row.name.clone() };
    painter.text(pos2(x, mid), Align2::LEFT_CENTER, &name, theme::sans(13.0), theme::INK);
    x += 150.0;
    painter.text(pos2(x, mid), Align2::LEFT_CENTER, bp(row.bases.len()), theme::mono(11.0), theme::MUTED);
    x += 80.0;

    // THE BAR SITS WITH THE NUMBER IT DRAWS. Before a design it is the insert's own length against
    // the longest on the list; after one it is one block per piece you will order, proportional,
    // with the backbone arms drawn inside the end pieces because they are part of those pieces.
    let bar = Rect::from_min_size(pos2(x, mid - 5.0), vec2(120.0, 10.0));
    painter.rect_filled(bar, CornerRadius::same(3), theme::alpha(theme::MUTED, 0.14));
    if row.pieces.is_empty() {
        let share = (row.bases.len() as f32 / longest as f32).clamp(0.02, 1.0);
        painter.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height())), CornerRadius::same(3), frag_color(which));
    } else {
        let total: usize = row.pieces.iter().sum::<usize>().max(1);
        let mut at = bar.left();
        for (i, piece) in row.pieces.iter().enumerate() {
            let w = bar.width() * (*piece as f32 / total as f32);
            painter.rect_filled(Rect::from_min_size(pos2(at, bar.top()), vec2(w, bar.height())), CornerRadius::same(3), frag_color(which + i));
            at += w;
        }
    }
    x += 132.0;
    let seq_right = r.right() - 40.0 - 150.0;
    if seq_right > x {
        let font = theme::mono(11.0);
        let shown = crate::session_nav::clip(ui, &row.bases, &font, seq_right - x);
        painter.text(pos2(x, mid), Align2::LEFT_CENTER, shown, font, theme::alpha(theme::MUTED, 0.8));
    }
    if !row.chip.is_empty() {
        let ink = if row.warn { theme::DANGER_TEXT } else { theme::MUTED };
        painter.text(pos2(r.right() - 40.0, mid), Align2::RIGHT_CENTER, &row.chip, theme::sans(11.0), ink);
    }
    // And what it costs to order, under it, once there is a design to say it of.
    if computed && !row.order_line.is_empty() {
        painter.text(pos2(r.left() + 36.0, r.top() + 38.0), Align2::LEFT_CENTER, &row.order_line, theme::sans(11.0), theme::MUTED);
    }
}

/// The bottom, in the same glass as the cut: the two settings in a group, the verbs in another,
/// and one solid button for the act that makes atoms. The overview sits above it, quiet, because
/// it is something you read rather than press.
fn foot(ui: &mut Ui, rect: Rect, model: &Model, inputs: &mut Inputs) -> Option<Action> {
    let mut action = None;
    let painter = ui.painter().with_clip_rect(rect);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme::HAIRLINE));
    let say = match (&model.note, model.progress) {
        (_, Some((done, total))) => format!("{done} of {total} computed"),
        (Some(note), _) => format!("{} {note}", model.summary),
        _ => model.summary.clone(),
    };
    let ink = if model.note.is_some() { theme::DANGER_TEXT } else { theme::MUTED };
    painter.text(pos2(rect.left() + 14.0, rect.top() + 18.0), Align2::LEFT_CENTER, say, theme::sans(12.0), ink);

    let top = rect.top() + 38.0;
    let mut x = rect.left() + 14.0;
    // Two settings in one pill read as one sentence without a mark between them.
    for (label, value, unit) in [("longest", &mut inputs.max_piece, "bp"), ("overlap", &mut inputs.target_tm, "°C")] {
        let pill = Rect::from_min_size(pos2(x, top), vec2(146.0, 34.0));
        primitives::glass(&painter, pill, 17.0);
        painter.text(pos2(pill.left() + 12.0, pill.center().y), Align2::LEFT_CENTER, label, theme::sans(11.0), theme::MUTED);
        if number_field(ui, pos2(pill.left() + 62.0, top), 56.0, label, "", value) {
            action = Some(Action::Edited);
        }
        painter.text(pos2(pill.right() - 10.0, pill.center().y), Align2::RIGHT_CENTER, unit, theme::sans(11.0), theme::MUTED);
        x += 146.0 + 8.0;
    }

    let computed = !model.stale && model.progress.is_none() && model.buildable > 0;
    let mut verbs = Vec::new();
    if computed {
        verbs.push(Segment { face: Face::Label("Copy pieces"), active: false, enabled: true, tip: "Every piece across every design, as FASTA" });
    }
    let compute_label = if model.progress.is_some() {
        "Computing…"
    } else if model.stale {
        "Compute"
    } else {
        "Recompute"
    };
    verbs.push(Segment {
        face: Face::Label(compute_label),
        active: model.stale && !model.rows.is_empty(),
        enabled: !model.rows.is_empty() && model.progress.is_none(),
        tip: "Design one plasmid per insert",
    });
    let verbs_w = primitives::group_width(ui, &verbs, false);
    if let Some(i) = primitives::group(ui, pos2(x, top), &verbs, Material::Glass, false) {
        action = Some(if computed && i == 0 { Action::CopyPieces } else { Action::Compute });
    }
    x += verbs_w + 8.0;

    // THE NAME FIELD ONLY NAMES SOMETHING WHEN THERE IS ONE THING.
    if model.rows.len() == 1 {
        let pill = Rect::from_min_size(pos2(x, top), vec2(180.0, 34.0));
        primitives::glass(&painter, pill, 17.0);
        if text_field(
            ui,
            Rect::from_min_size(pos2(pill.left() + 12.0, top + 7.0), vec2(156.0, 20.0)),
            "build-name",
            "What to call it",
            &mut inputs.name,
        ) {
            action = Some(Action::Edited);
        }
    }

    let label = if model.busy {
        "Building…".to_string()
    } else if model.buildable > 1 {
        format!("Build {} plasmids", model.buildable)
    } else {
        "Build".to_string()
    };
    let build = Button {
        tone: Tone::Solid,
        enabled: model.buildable > 0 && !model.busy && !model.stale && model.progress.is_none(),
        ..Button::label(&label, Material::Glass)
    };
    let width = build.width(ui);
    if build.show_at(ui, pos2(rect.right() - 14.0 - width, top)).clicked() {
        action = Some(Action::Build);
    }
    action
}

fn number_field(ui: &mut Ui, at: egui::Pos2, width: f32, id: &str, hint: &str, value: &mut String) -> bool {
    let before = value.clone();
    let rect = Rect::from_min_size(at, vec2(width, 34.0));
    primitives::glass(ui.painter(), rect, 17.0);
    let changed = text_field(ui, Rect::from_min_size(pos2(at.x + 10.0, at.y + 7.0), vec2(width - 20.0, 20.0)), id, hint, value);
    // Only digits: every other character is a typo in a coordinate.
    value.retain(|c| c.is_ascii_digit());
    changed || *value != before
}

fn text_field(ui: &mut Ui, rect: Rect, id: &str, hint: &str, value: &mut String) -> bool {
    let font = theme::sans(12.0);
    let edit = egui::TextEdit::singleline(value)
        .id(Id::unique(("build-field", id)))
        .frame(egui::Frame::NONE.inner_margin(Margin::ZERO))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(egui::RichText::new(hint).font(font).color(theme::MUTED))
        .align(Align2::LEFT_CENTER);
    ui.put(rect, edit).changed()
}

/// The strip of glass the map keeps when the console owns the screen, so the cut stays visible
/// and changeable without going back up.
pub fn map_strip(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, theme::CANVAS);
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::HAIRLINE));
}

/// The picker's list, as the menu draws it: every enzyme that can open this backbone, the count on
/// the row because it is the fact that decides.
pub fn enzyme_items(options: &[EnzymePick], all: bool, chosen: &str) -> Vec<crate::rail_menu::Item<String>> {
    options
        .iter()
        .filter(|o| all || o.count == 1 || o.enzyme == chosen)
        .map(|o| {
            let sites = if o.count == 1 { "1 site".to_string() } else { format!("{} sites", o.count) };
            crate::rail_menu::Item::new(format!("{} · {sites}", o.enzyme), o.enzyme.clone())
        })
        .collect()
}

/// A ring round the field that has the keyboard, so a console full of pills says which one is live.
pub fn focus_ring(painter: &egui::Painter, rect: Rect) {
    primitives::focus_ring(painter, rect, 17.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::{EnzymePick, bp, enzyme_items, window};

    #[test]
    fn the_list_draws_the_rows_it_is_over_and_a_few_either_side() {
        // The same list at three inserts and at a million: only what the viewport is over is
        // drawn, plus an overdraw so a fast scroll never shows a gap.
        assert_eq!(window(0.0, 34.0, 340.0, 1000), (0, 14));
        assert_eq!(window(3400.0, 34.0, 340.0, 1000), (96, 114));
        // Short lists are whole, and the end is never past the last row.
        assert_eq!(window(0.0, 34.0, 340.0, 6), (0, 6));
        assert_eq!(window(33_966.0, 34.0, 340.0, 1000), (995, 1000));
        // Nothing is asked of a row height of zero but an answer.
        assert_eq!(window(0.0, 0.0, 340.0, 12), (0, 12));
    }

    #[test]
    fn a_length_reads_as_a_person_writes_one() {
        assert_eq!(bp(694), "694 bp");
        assert_eq!(bp(6093), "6,093 bp");
        assert_eq!(bp(0), "0 bp");
        assert_eq!(super::thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn the_picker_offers_the_openings_and_lets_the_rest_in_on_request() {
        let options = vec![EnzymePick { enzyme: "EcoRI".into(), count: 1 }, EnzymePick { enzyme: "PstI".into(), count: 3 }];
        let unique = enzyme_items(&options, false, "");
        assert_eq!(unique.iter().map(|i| i.text.clone()).collect::<Vec<_>>(), ["EcoRI · 1 site"]);
        let all = enzyme_items(&options, true, "");
        assert_eq!(all.iter().map(|i| i.text.clone()).collect::<Vec<_>>(), ["EcoRI · 1 site", "PstI · 3 sites"]);
        // A multi-cutter already chosen stays listed, or choosing it would unchoose it.
        let kept = enzyme_items(&options, false, "PstI");
        assert_eq!(kept.len(), 2);
    }
}
