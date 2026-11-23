//! The Settings window: Appearance, Keyboard and About, over a dimmed window. Ported from the
//! web's `platform/SettingsModal.tsx` and `keys/KeyboardSettings.tsx`, with `settings.css`'s
//! geometry. Account is not here: Bios on your own machine has no accounts. The web's update
//! check and demo switch belong to its desktop wrapper and its Run preview, neither of which
//! Bios native has.

use egui::{Align2, Color32, CornerRadius, CursorIcon, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};

use crate::theme::{self, Weight};
use crate::{icon, lucide, primitives};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Appearance,
    Keyboard,
    About,
}

/// One row of the Keyboard section.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub command: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// The chord as this system writes it, or `None` when unbound.
    pub chord: Option<String>,
    /// The nine "go to construct" chords, shown and not changed.
    pub fixed: bool,
    pub is_default: bool,
    pub needs_selection: bool,
    /// "Also find in this construct." when another command has the same chord.
    pub clash: Option<String>,
}

/// What the window shows.
pub struct Model<'a> {
    pub rows: &'a [KeyRow],
    pub groups: &'a [&'a str],
    /// How many shortcuts differ from the default.
    pub changed: usize,
    pub version: &'a str,
}

/// What was done in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Close,
    /// The keys pressed for a command while it was listening.
    Bind(&'static str, Key, Modifiers),
    /// Backspace while listening: no shortcut.
    Unbind(&'static str),
    Reset(&'static str),
    ResetAll,
}

/// The window's own state.
#[derive(Default)]
pub struct Settings {
    pub section: Section,
    /// The command listening for keys (`Recorder`).
    recording: Option<&'static str>,
    /// The modifiers held so far while listening.
    held: String,
}

const W: f32 = 880.0;
const H: f32 = 640.0;
const NAV_W: f32 = 200.0;
const ROW_H: f32 = 51.0;

fn r(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, 255.0) as u8)
}

impl Settings {
    pub fn new(section: Section) -> Self {
        Settings { section, recording: None, held: String::new() }
    }

    /// A command is listening for keys, so no shortcut should fire meanwhile.
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Takes this frame's key presses while a command listens for them, so nothing else sees them.
    fn listen(&mut self, ui: &egui::Ui, out: &mut Vec<Outcome>) {
        let Some(command) = self.recording else {
            return;
        };
        let events = ui.ctx().input_mut(|i| std::mem::take(&mut i.events));
        let mac = cfg!(target_os = "macos");
        for event in events {
            match event {
                egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                    self.recording = None;
                }
                egui::Event::Key { key: Key::Backspace | Key::Delete, pressed: true, .. } => {
                    out.push(Outcome::Unbind(command));
                    self.recording = None;
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    out.push(Outcome::Bind(command, key, modifiers));
                    self.recording = None;
                }
                _ => {}
            }
        }
        if self.recording.is_some() {
            let m = ui.input(|i| i.modifiers);
            let parts: Vec<&str> = [
                (m.mac_cmd || (m.command && mac), "\u{2318}"),
                (m.ctrl && mac, "\u{2303}"),
                (m.ctrl && !mac, "Ctrl"),
                (m.alt, if mac { "\u{2325}" } else { "Alt" }),
                (m.shift, if mac { "\u{21e7}" } else { "Shift" }),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| *s)
            .collect();
            self.held = if parts.is_empty() { String::new() } else { format!("{}\u{2026}", parts.concat()) };
            ui.ctx().request_repaint();
        }
    }

    /// Draws the window over `screen`.
    pub fn show(&mut self, ui: &mut egui::Ui, screen: Rect, model: &Model<'_>) -> Vec<Outcome> {
        let mut out = Vec::new();
        self.listen(ui, &mut out);
        egui::Area::new(egui::Id::unique("settings-modal")).order(egui::Order::Foreground).fixed_pos(screen.min).fade_in(false).show(
            ui.ctx(),
            |ui| {
                let backdrop = ui.interact(screen, ui.scope_id().with("settings-backdrop"), Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(80));
                let modal = Rect::from_center_size(screen.center(), vec2(W.min(screen.width() - 32.0), H.min(screen.height() - 32.0)));
                // The modal swallows presses so only the dim around it closes.
                let inside = ui.interact(modal, ui.scope_id().with("settings-inside"), Sense::click());
                let _ = inside;
                if backdrop.clicked() {
                    out.push(Outcome::Close);
                }
                primitives::shadow(ui.painter(), modal, theme::RADIUS, theme::SHADOW_GLASS);
                ui.painter().rect_filled(modal, r(theme::RADIUS), theme::CANVAS);
                let nav = Rect::from_min_size(modal.min, vec2(NAV_W, modal.height()));
                let left_round = CornerRadius { nw: r(theme::RADIUS).nw, sw: r(theme::RADIUS).sw, ne: 0, se: 0 };
                ui.painter().rect_filled(nav, left_round, theme::CELL);
                ui.painter().line_segment([nav.right_top(), nav.right_bottom()], Stroke::new(1.0, theme::HAIRLINE));
                ui.painter().rect_stroke(modal, r(theme::RADIUS), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
                ui.painter().text(pos2(nav.left() + 23.0, nav.top() + 34.0), Align2::LEFT_CENTER, "Settings", theme::serif(17.0), theme::INK);
                let mut y = nav.top() + 58.0;
                for (section, label, glyph) in [
                    (Section::Appearance, "Appearance", lucide::PALETTE),
                    (Section::Keyboard, "Keyboard", lucide::KEYBOARD),
                    (Section::About, "About", lucide::INFO),
                ] {
                    let row = Rect::from_min_size(pos2(nav.left() + 15.0, y), vec2(NAV_W - 30.0, 30.0));
                    let response = ui.interact(row, ui.scope_id().with(("settings-nav", label)), Sense::click());
                    let on = self.section == section;
                    if on {
                        ui.painter().rect_filled(row, r(theme::RADIUS_SM), theme::FLOAT);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row, r(theme::RADIUS_SM), theme::alpha(theme::INK, 0.04));
                    }
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let ink = if on { theme::INK } else { theme::MUTED };
                    icon::paint(ui.painter(), pos2(row.left() + 8.0 + 7.5, row.center().y), 15.0, glyph, ink);
                    ui.painter().text(pos2(row.left() + 31.0, row.center().y), Align2::LEFT_CENTER, label, theme::sans(13.0), ink);
                    if response.clicked() {
                        self.section = section;
                        self.recording = None;
                    }
                    y += 32.0;
                }
                let x = Rect::from_center_size(pos2(modal.right() - 27.0, modal.top() + 27.0), vec2(26.0, 26.0));
                let close = ui.interact(x, ui.scope_id().with("settings-x"), Sense::click());
                if close.hovered() {
                    ui.painter().rect_filled(x, r(theme::RADIUS_SM), theme::alpha(theme::INK, 0.06));
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                icon::paint(ui.painter(), x.center(), 16.0, lucide::X, theme::MUTED);
                if close.clicked() {
                    out.push(Outcome::Close);
                }
                let body = Rect::from_min_max(pos2(nav.right() + 34.0, modal.top() + 40.0), pos2(modal.right() - 34.0, modal.bottom()));
                ui.painter().text(body.left_top() + vec2(0.0, 16.0), Align2::LEFT_CENTER, self.title(), theme::serif(20.0), theme::INK);
                let content = Rect::from_min_max(pos2(body.left(), body.top() + 40.0), body.max);
                ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                    ui.set_clip_rect(content.intersect(ui.clip_rect()));
                    match self.section {
                        Section::Appearance => appearance(ui, content),
                        Section::About => about(ui, content, model.version),
                        Section::Keyboard => self.keyboard(ui, content, model, &mut out),
                    }
                });
            },
        );
        if self.recording.is_none() && ui.input(|i| i.key_pressed(Key::Escape)) {
            out.push(Outcome::Close);
        }
        out
    }

    fn title(&self) -> &'static str {
        match self.section {
            Section::Appearance => "Appearance",
            Section::Keyboard => "Keyboard",
            Section::About => "About",
        }
    }

    fn keyboard(&mut self, ui: &mut egui::Ui, content: Rect, model: &Model<'_>, out: &mut Vec<Outcome>) {
        let note = "Saved on this computer. Click a shortcut to change it \u{2014} Esc cancels, Backspace clears it.";
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("settings-keys").show(ui, |ui| {
            let top = ui.min_rect().top();
            let (left, right) = (content.left(), content.right());
            let painter = ui.painter().clone();
            painter.text(pos2(left, top + 8.0), Align2::LEFT_CENTER, note, theme::sans(13.0), theme::MUTED);
            let mut y = top + 29.0;
            painter.line_segment([pos2(left, y), pos2(right, y)], Stroke::new(1.0, theme::HAIRLINE));
            y += 1.0;
            for group in model.groups {
                let rows: Vec<&KeyRow> = model.rows.iter().filter(|row| row.group == *group).collect();
                if rows.is_empty() {
                    continue;
                }
                y += 22.0;
                painter.text(pos2(left, y + 7.0), Align2::LEFT_CENTER, group.to_uppercase(), theme::sans_weight(11.0, Weight::Medium), theme::MUTED);
                y += 14.0 + 4.0;
                for row in rows {
                    let line = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW_H));
                    let mut label = row.label.to_string();
                    let label_w = primitives::text_width(ui, &label, theme::sans(13.0));
                    painter.text(pos2(left, line.center().y), Align2::LEFT_CENTER, &label, theme::sans(13.0), theme::INK);
                    if row.needs_selection {
                        label = " \u{b7} needs a selection".into();
                        painter.text(pos2(left + label_w, line.center().y), Align2::LEFT_CENTER, &label, theme::sans(13.0), theme::MUTED);
                    }
                    let chip = Rect::from_min_size(pos2(right - 92.0, line.center().y - 17.0), vec2(92.0, 34.0));
                    let listening = self.recording == Some(row.command);
                    let response =
                        ui.interact(chip, ui.scope_id().with(("keymap-row", row.command)), if row.fixed { Sense::hover() } else { Sense::click() });
                    let fill = if listening || (response.hovered() && !row.fixed) { theme::CELL } else { theme::FLOAT };
                    painter.rect_filled(chip, r(theme::RADIUS_SM), fill);
                    let edge = if listening { theme::INK } else { theme::HAIRLINE_STRONG };
                    painter.rect_stroke(chip, r(theme::RADIUS_SM), Stroke::new(1.0, edge), StrokeKind::Inside);
                    let text = if listening {
                        if self.held.is_empty() { "Press keys\u{2026}".to_string() } else { self.held.clone() }
                    } else {
                        row.chord.clone().unwrap_or_else(|| "Not set".into())
                    };
                    let ink = if row.chord.is_some() || listening { theme::INK } else { theme::MUTED };
                    painter.text(chip.center(), Align2::CENTER_CENTER, text, theme::mono(12.0), ink);
                    if !row.fixed && response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let tip = if row.fixed { "\u{2318}1 through \u{2318}9" } else { "Click, then press the keys you want" };
                    if response.on_hover_text(tip).clicked() && !row.fixed {
                        self.recording = Some(row.command);
                        self.held.clear();
                    }
                    if !row.is_default && !row.fixed {
                        let w = primitives::text_width(ui, "Reset", theme::sans(12.0)) + 12.0;
                        let reset = Rect::from_min_size(pos2(chip.left() - 8.0 - w, line.center().y - 12.0), vec2(w, 24.0));
                        let response = ui.interact(reset, ui.scope_id().with(("keymap-reset", row.command)), Sense::click());
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        let ink = if response.hovered() { theme::INK } else { theme::MUTED };
                        painter.text(reset.center(), Align2::CENTER_CENTER, "Reset", theme::sans(12.0), ink);
                        if response.on_hover_text("Back to the keymap's default").clicked() {
                            out.push(Outcome::Reset(row.command));
                        }
                    }
                    y += ROW_H;
                    if let Some(clash) = &row.clash {
                        painter.text(pos2(left, y + 9.0), Align2::LEFT_CENTER, clash, theme::sans(12.5), theme::DANGER_TEXT);
                        y += 22.0;
                    }
                    painter.line_segment([pos2(left, y - 0.5), pos2(right, y - 0.5)], Stroke::new(1.0, theme::HAIRLINE));
                }
            }
            if model.changed > 0 {
                let line = Rect::from_min_max(pos2(left, y + 8.0), pos2(right, y + 8.0 + ROW_H));
                let said = format!("{} shortcut{} changed from the default", model.changed, if model.changed == 1 { "" } else { "s" });
                painter.text(pos2(left, line.center().y), Align2::LEFT_CENTER, said, theme::sans(13.0), theme::MUTED);
                let w = primitives::text_width(ui, "Reset all", theme::sans(13.0)) + 20.0;
                let button = Rect::from_min_size(pos2(right - w, line.center().y - 14.0), vec2(w, 28.0));
                let response = ui.interact(button, ui.scope_id().with("keymap-reset-all"), Sense::click());
                painter.rect_stroke(button, r(theme::RADIUS_SM), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
                if response.hovered() {
                    painter.rect_filled(button, r(theme::RADIUS_SM), theme::alpha(theme::INK, 0.04));
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                painter.text(button.center(), Align2::CENTER_CENTER, "Reset all", theme::sans(13.0), theme::INK);
                if response.clicked() {
                    out.push(Outcome::ResetAll);
                }
                y = line.bottom();
            }
            ui.allocate_rect(Rect::from_min_max(pos2(left, top), pos2(right, y + 24.0)), Sense::hover());
        });
    }
}

/// `.settings-row`: a label on the left, a value on the right, a hairline under.
fn settings_row(ui: &egui::Ui, left: f32, right: f32, y: f32, label: &str, value: &str) -> f32 {
    let mid = y + 22.0;
    ui.painter().text(pos2(left, mid), Align2::LEFT_CENTER, label, theme::sans(13.0), theme::INK);
    ui.painter().text(pos2(right, mid), Align2::RIGHT_CENTER, value, theme::sans(13.0), theme::MUTED);
    ui.painter().line_segment([pos2(left, y + 44.5), pos2(right, y + 44.5)], Stroke::new(1.0, theme::HAIRLINE));
    y + 45.0
}

fn note(ui: &egui::Ui, left: f32, y: f32, text: &str) -> f32 {
    ui.painter().text(pos2(left, y + 18.0), Align2::LEFT_CENTER, text, theme::sans(13.0), theme::MUTED);
    y + 32.0
}

/// `AppearanceSection`: Bios is light only, and says so in the web's words.
fn appearance(ui: &egui::Ui, content: Rect) {
    let y = settings_row(ui, content.left(), content.right(), content.top() - 4.0, "Theme", "Light");
    note(ui, content.left(), y, "A dark theme is on the way. Map colours live on the map\u{2019}s \u{201c}Show on map\u{201d} panel.");
}

/// `AboutSection`, without the desktop wrapper's update check and the Run demo.
fn about(ui: &egui::Ui, content: Rect, version: &str) {
    let y = settings_row(ui, content.left(), content.right(), content.top() - 4.0, "Bios", &format!("Beta {version}"));
    let y = note(ui, content.left(), y, "A free gene and plasmid editor. An open SnapGene.");
    note(ui, content.left(), y - 8.0, "Open source, MIT or Apache-2.0, at your option. Runs on your own machine and needs no account.");
}
