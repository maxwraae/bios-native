//! The right-click menu the panel's rows and the tabs open, ported from the web's
//! `layout/RailMenu.tsx` (whole file) and its styles, `styles.css` 10200-10265 (`.row-menu`):
//! a cell-coloured card at the pointer, its groups split by rules, a destructive entry last and
//! red, an entry that cannot run greyed with the reason on hover, and one level of submenu.
//! It closes on a pick, on Escape, and on a press anywhere outside it.

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::text_width;
use crate::theme;

/// One entry (`RailMenuItem`).
#[derive(Clone, Debug)]
pub struct Item<T> {
    pub text: String,
    pub does: Option<T>,
    pub danger: bool,
    /// Greyed, with the reason on hover.
    pub disabled: Option<String>,
    /// One level of submenu, for Move to.
    pub children: Vec<Item<T>>,
}

impl<T> Item<T> {
    pub fn new(text: impl Into<String>, does: T) -> Self {
        Item { text: text.into(), does: Some(does), danger: false, disabled: None, children: Vec::new() }
    }

    /// The destructive entry: red, and last.
    pub fn danger(self) -> Self {
        Item { danger: true, ..self }
    }

    /// An entry that cannot run, saying why.
    pub fn disabled(text: impl Into<String>, why: impl Into<String>) -> Self {
        Item { text: text.into(), does: None, danger: false, disabled: Some(why.into()), children: Vec::new() }
    }

    pub fn submenu(text: impl Into<String>, children: Vec<Item<T>>) -> Self {
        Item { text: text.into(), does: None, danger: false, disabled: None, children }
    }
}

/// An open menu.
pub struct Menu<T> {
    at: Pos2,
    groups: Vec<Vec<Item<T>>>,
    /// Which entry's submenu is open, by group and place in it.
    sub: Option<(usize, usize)>,
    /// The frame it opened on, whose press must not close it again.
    fresh: bool,
}

/// What became of the menu this frame.
pub enum Shown<T> {
    StillOpen,
    Closed,
    Picked(T),
}

/// `.row-menu-item`'s height: 13px type padded 6 above and below.
const ITEM_H: f32 = 28.0;
/// `.row-menu-rule`: 1px with 4px above and below.
const RULE_H: f32 = 9.0;

impl<T: Clone> Menu<T> {
    pub fn new(at: Pos2, groups: Vec<Vec<Item<T>>>) -> Self {
        Menu { at, groups: groups.into_iter().filter(|g| !g.is_empty()).collect(), sub: None, fresh: true }
    }

    /// Draws the menu and says what was picked.
    pub fn show(&mut self, ui: &mut Ui) -> Shown<T> {
        let screen = ui.ctx().content_rect();
        let count: usize = self.groups.iter().map(Vec::len).sum();
        // Kept on screen, as RailMenu.tsx keeps it inside the window.
        let height = count as f32 * (ITEM_H + 2.0) + self.groups.len() as f32 * RULE_H + 16.0;
        let at = pos2(self.at.x.min(screen.right() - 240.0), self.at.y.min(screen.bottom() - height));
        let mut picked = None;
        let mut hovering_sub = None;
        let main = panel(ui, "rail-menu", at, &self.groups, self.sub, &mut picked, &mut hovering_sub);
        let mut rects = vec![main.rect];
        if let Some(hovered) = hovering_sub {
            self.sub = hovered;
        }
        if let Some((g, i)) = self.sub
            && let Some(item) = self.groups.get(g).and_then(|group| group.get(i)).filter(|item| !item.children.is_empty())
            && let Some(host) = main.subs.iter().find(|(key, _)| *key == (g, i)).map(|(_, r)| *r)
        {
            let children = vec![item.children.clone()];
            let sub = panel(ui, "rail-menu-sub", pos2(host.right() + 4.0, host.top() - 5.0), &children, None, &mut picked, &mut None);
            rects.push(sub.rect);
        }
        if let Some(does) = picked {
            return Shown::Picked(does);
        }
        let pressed_away =
            ui.input(|i| i.pointer.any_pressed()) && ui.ctx().pointer_interact_pos().is_some_and(|p| !rects.iter().any(|r| r.contains(p)));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || (pressed_away && !self.fresh) {
            return Shown::Closed;
        }
        self.fresh = false;
        Shown::StillOpen
    }
}

/// A drawn menu card: its rectangle, and where each entry with a submenu sits.
struct Panel {
    rect: Rect,
    subs: Vec<((usize, usize), Rect)>,
}

/// `.row-menu`: 220 wide at least, padding 4, the cell's colour, a hairline and `--shadow-2`.
fn panel<T: Clone>(
    ui: &mut Ui,
    salt: &str,
    at: Pos2,
    groups: &[Vec<Item<T>>],
    open_sub: Option<(usize, usize)>,
    picked: &mut Option<T>,
    hovering_sub: &mut Option<Option<(usize, usize)>>,
) -> Panel {
    let mut subs = Vec::new();
    let shadow = egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_rgba_unmultiplied(30, 35, 40, 20) };
    let area = egui::Area::new(egui::Id::unique(salt)).order(egui::Order::Foreground).fixed_pos(at).fade_in(false);
    let radius = CornerRadius::same(theme::RADIUS_SM as u8);
    let response = area.show(ui.ctx(), |ui| {
        egui::Frame::new()
            .fill(theme::CELL)
            .stroke(Stroke::new(1.0, theme::HAIRLINE))
            .corner_radius(theme::RADIUS)
            .inner_margin(4.0)
            .shadow(shadow)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let width = groups
                    .iter()
                    .flatten()
                    .map(|item| text_width(ui, &item.text, theme::sans(13.0)) + 20.0 + if item.children.is_empty() { 0.0 } else { 24.0 })
                    .fold(212.0f32, f32::max);
                for (g, group) in groups.iter().enumerate() {
                    if g > 0 {
                        let (r, _) = ui.allocate_exact_size(vec2(width, RULE_H), Sense::hover());
                        ui.painter().line_segment(
                            [pos2(r.left() + 6.0, r.center().y), pos2(r.right() - 6.0, r.center().y)],
                            Stroke::new(1.0, theme::HAIRLINE),
                        );
                    }
                    for (i, item) in group.iter().enumerate() {
                        let enabled = item.disabled.is_none();
                        let (r, response) = ui.allocate_exact_size(vec2(width, ITEM_H), if enabled { Sense::click() } else { Sense::hover() });
                        let has_sub = !item.children.is_empty();
                        let lit = (enabled && response.hovered()) || (has_sub && open_sub == Some((g, i)));
                        if lit {
                            ui.painter().rect_filled(r, radius, theme::ROW_HOVER);
                        }
                        let ink = if !enabled {
                            theme::MUTED
                        } else if item.danger {
                            theme::SANA_SCARLET
                        } else {
                            theme::INK
                        };
                        ui.painter().text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, &item.text, theme::sans(13.0), ink);
                        if has_sub {
                            ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, "›", theme::sans(13.0), theme::MUTED);
                            subs.push(((g, i), r));
                            if response.hovered() || response.clicked() {
                                *hovering_sub = Some(Some((g, i)));
                            }
                        }
                        let response = match &item.disabled {
                            Some(why) => response.on_hover_text(why),
                            None => response,
                        };
                        if enabled && !has_sub && response.clicked() {
                            *picked = item.does.clone();
                        }
                    }
                }
            });
    });
    Panel { rect: response.response.rect, subs }
}
