//! The open plasmids as tabs across the top of the work column, in the head's 38px band. Ported
//! from the web's `layout/TabStrip.tsx` (the `TabStrip` component) and `layout/tabStrip.css`,
//! where the last rules win: square tabs outlined on three sides with neighbours sharing a line,
//! a faint ground of their own, and the one in front in the sheet's paper with its place's colour
//! doubled along the top edge, open into the sheet under it.
//!
//! A tab dragged across its neighbours moves as it goes (`reorderTab` on `dragover`), faded to half
//! while it is held. Not ported: the build and plate page tabs (Build and Verify are not built here).

use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::rail_menu::{self, Item, Menu};
use crate::session_nav::clip;
use crate::theme::{self, Weight};
use crate::{icon, lucide};

/// One tab.
pub struct Tab<'a> {
    pub name: &'a str,
    pub circular: bool,
    /// Something typed in it is not saved yet.
    pub dirty: bool,
}

/// What was done to the tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    CloseRight(usize),
    /// The tab at the first index, being dragged, is now over the second: move it there.
    Move(usize, usize),
}

/// Moves the item at `from` to `to`, the others keeping their order (`moveTab`). Out of range does
/// nothing.
pub fn move_tab<T>(tabs: &mut Vec<T>, from: usize, to: usize) {
    if from >= tabs.len() || to >= tabs.len() || from == to {
        return;
    }
    let tab = tabs.remove(from);
    tabs.insert(to, tab);
}

/// The strip's own state between frames: its right-click menu.
#[derive(Default)]
pub struct TabStrip {
    menu: Option<Menu<Pressed>>,
    /// Which tab is being dragged, by where it stands now.
    dragging: Option<usize>,
}

/// `.tab-strip .tab`: 112 to 200 wide, 30 high.
const MIN_W: f32 = 112.0;
const MAX_W: f32 = 200.0;
/// Max asked for the tabs to fill the head (2026-11-22): the whole band from the window's top edge
/// to the foot line, flush with the panel's divider, where the web insets 30px tabs by 8 above and
/// 10 on the left.
const PAD_LEFT: f32 = 0.0;

fn font(active: bool) -> egui::FontId {
    theme::sans_weight(12.5, if active { Weight::Semibold } else { Weight::Book })
}

/// A tab's natural width: padding 10, the 14px mark, a 6 gap, the name, a 6 gap, the 16px slot
/// for the close button (and another for the dot while it is there), padding 6. Taken as it is
/// at rest, so a tab never changes width under the pointer.
fn natural(ui: &Ui, tab: &Tab<'_>, active: bool) -> f32 {
    let dot = if tab.dirty { 16.0 + 6.0 } else { 0.0 };
    let name = crate::primitives::text_width(ui, tab.name, font(active));
    (10.0 + 14.0 + 6.0 + name + 6.0 + dot + 16.0 + 6.0).clamp(MIN_W, MAX_W)
}

impl TabStrip {
    /// Draws the tabs in `rect` and says what was pressed. The first `lead` points are left to
    /// what sits at the strip's left end while the sidebar is away: the traffic lights and the
    /// sidebar's toggle.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, lead: f32, tabs: &[Tab<'_>], active: Option<usize>, place: Color32) -> Option<Pressed> {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme::CANVAS);
        // The strip's own line along the foot, which closes the row.
        let foot = rect.bottom() - 0.5;
        painter.line_segment([pos2(rect.left(), foot), pos2(rect.right(), foot)], Stroke::new(1.0, theme::HAIRLINE_STRONG));
        // Tabs give width up before the strip runs out, down to their least.
        let wants: Vec<f32> = tabs.iter().enumerate().map(|(i, tab)| natural(ui, tab, active == Some(i))).collect();
        let room = rect.width() - PAD_LEFT - lead;
        let total: f32 = wants.iter().sum::<f32>() - (tabs.len().saturating_sub(1)) as f32;
        let shrink = if total > room {
            let give: f32 = wants.iter().map(|w| w - MIN_W).sum();
            if give > 0.0 { ((total - room) / give).min(1.0) } else { 0.0 }
        } else {
            0.0
        };
        let mut pressed = None;
        let mut x = rect.left() + PAD_LEFT + lead;
        let mut rects = Vec::with_capacity(tabs.len());
        for want in &wants {
            let w = want - (want - MIN_W) * shrink;
            rects.push(Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())));
            // Neighbours share the line between them.
            x += w - 1.0;
        }
        // The one in front is drawn last, over its neighbours' edges.
        let mut order: Vec<usize> = (0..tabs.len()).filter(|i| Some(*i) != active).collect();
        order.extend(active.filter(|a| *a < tabs.len()));
        // A drag carries its tab over its neighbours; it moves the moment it is over another one.
        let held = ui.input(|i| i.pointer.primary_down());
        if !held {
            self.dragging = None;
        }
        if let (Some(from), Some(at)) = (self.dragging, ui.input(|i| i.pointer.interact_pos()))
            && let Some(to) = rects.iter().position(|r| r.contains(at))
            && to != from
        {
            self.dragging = Some(to);
            return Some(Pressed::Move(from, to));
        }
        for i in order {
            let (Some(tab), Some(r)) = (tabs.get(i), rects.get(i).copied()) else { continue };
            let faded;
            let painter = if self.dragging == Some(i) {
                faded = {
                    let mut p = painter.clone();
                    p.set_opacity(0.5);
                    p
                };
                &faded
            } else {
                &painter
            };
            if let Some(p) = self.tab(ui, painter, r, tab, active == Some(i), place) {
                pressed = Some(match p {
                    TabPress::Select => Pressed::Select(i),
                    TabPress::Drag => {
                        self.dragging = Some(i);
                        continue;
                    }
                    TabPress::Close => Pressed::Close(i),
                    TabPress::Menu(at) => {
                        let others = tabs.len() > 1;
                        let right = i + 1 < tabs.len();
                        let groups = vec![
                            vec![Item::new("Close", Pressed::Close(i))],
                            vec![
                                if others {
                                    Item::new("Close others", Pressed::CloseOthers(i))
                                } else {
                                    Item::disabled("Close others", "Nothing else is open")
                                },
                                if right {
                                    Item::new("Close to the right", Pressed::CloseRight(i))
                                } else {
                                    Item::disabled("Close to the right", "Nothing to the right")
                                },
                            ],
                        ];
                        self.menu = Some(Menu::new(at, groups));
                        continue;
                    }
                });
            }
        }
        if let Some(menu) = self.menu.as_mut() {
            match menu.show(ui) {
                rail_menu::Shown::StillOpen => {}
                rail_menu::Shown::Closed => self.menu = None,
                rail_menu::Shown::Picked(p) => {
                    self.menu = None;
                    pressed = Some(p);
                }
            }
        }
        pressed
    }

    fn tab(&mut self, ui: &mut Ui, painter: &egui::Painter, r: Rect, tab: &Tab<'_>, active: bool, place: Color32) -> Option<TabPress> {
        let response = ui.interact(r, ui.scope_id().with(("tab", r.left() as i32)), Sense::click_and_drag());
        let drag = response.drag_started();
        let hovered = response.hovered();
        let ground = if active {
            theme::CELL
        } else if hovered {
            theme::alpha(theme::INK, 0.09)
        } else {
            theme::alpha(theme::INK, 0.05)
        };
        // The front tab's paper covers the strip's foot line, so it opens into the sheet.
        let fill = if active { r } else { Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 1.0)) };
        painter.rect_filled(fill, 0.0, ground);
        let edge = Stroke::new(1.0, theme::HAIRLINE_STRONG);
        let (l, rt, t, b) = (r.left() + 0.5, r.right() - 0.5, r.top(), r.bottom());
        // The first tab meets the panel's own divider, so it draws no left edge of its own; the
        // window's top edge is the tabs' top, so they draw no top line either.
        if r.left() > ui.clip_rect().left() + 1.0 {
            painter.line_segment([pos2(l, t), pos2(l, b)], edge);
        }
        painter.line_segment([pos2(rt, t), pos2(rt, b)], edge);
        if active {
            // `border-top-color` and an inset 1px: the place's colour, two pixels deep.
            painter.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 2.0)), 0.0, place);
        }
        let mid = r.center().y;
        let mark_ink = theme::alpha(theme::INK, 0.6);
        let mark_at = pos2(r.left() + 10.0 + 7.0, mid);
        icon::paint(painter, mark_at, 14.0, if tab.circular { lucide::CIRCLE } else { lucide::MINUS }, mark_ink);
        let slot = Rect::from_center_size(pos2(r.right() - 6.0 - 8.0, mid), vec2(16.0, 16.0));
        let dot = tab.dirty && !hovered;
        let name_left = r.left() + 10.0 + 14.0 + 6.0;
        let name_right = slot.left() - 6.0 - if dot { 16.0 + 6.0 } else { 0.0 };
        let ink = if active || hovered { theme::INK } else { theme::alpha(theme::INK, 0.72) };
        let shown = clip(ui, tab.name, &font(active), (name_right - name_left).max(0.0));
        painter.with_clip_rect(r).text(pos2(name_left, mid), Align2::LEFT_CENTER, shown, font(active), ink);
        if dot {
            // Where the close button goes, yielding to it on hover.
            let at = Rect::from_center_size(pos2(slot.center().x - 16.0 - 6.0, mid), vec2(16.0, 16.0));
            painter.circle_filled(at.center(), 3.0, theme::DIRTY);
        }
        let mut close = false;
        if active || hovered {
            let x = ui.interact(slot, ui.scope_id().with(("tab-close", r.left() as i32)), Sense::click());
            let on = x.hovered();
            if on {
                painter.circle_filled(slot.center(), 8.0, theme::CELL_ACTIVE);
            }
            icon::paint(painter, slot.center(), 13.0, lucide::X, if on { theme::INK } else { theme::MUTED });
            close = x.on_hover_text(format!("Close {}", tab.name)).clicked();
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let secondary = response.secondary_clicked();
        let middle = response.middle_clicked();
        let clicked = response.on_hover_text(tab.name).clicked();
        if close || middle {
            Some(TabPress::Close)
        } else if drag {
            Some(TabPress::Drag)
        } else if secondary {
            ui.ctx().pointer_interact_pos().map(TabPress::Menu)
        } else if clicked {
            Some(TabPress::Select)
        } else {
            None
        }
    }
}

enum TabPress {
    Select,
    Drag,
    Close,
    Menu(Pos2),
}

#[cfg(test)]
mod tests {
    use super::move_tab;

    #[test]
    fn a_tab_moves_to_where_it_is_dragged_and_the_rest_keep_their_order() {
        let mut tabs = vec!["a", "b", "c", "d"];
        move_tab(&mut tabs, 0, 2);
        assert_eq!(tabs, ["b", "c", "a", "d"]);
        move_tab(&mut tabs, 3, 0);
        assert_eq!(tabs, ["d", "b", "c", "a"]);
        move_tab(&mut tabs, 1, 9);
        assert_eq!(tabs, ["d", "b", "c", "a"], "out of range does nothing");
    }
}
