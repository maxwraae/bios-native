//! The sidebar's frame and head: one column from the window's left edge, in the rail's ground, with
//! the window's traffic lights and the panel toggle in its first row, then "← Projects", the
//! project's name on its own line, its quick actions as rows (glyph and words), then Edit / Build /
//! Verify as rows. The place's own list (the project's folders and plasmids, or Verify's
//! verifications) fills the rest.
//!
//! A deliberate departure from the web app (Max, 2026-11-22 and 23), modelled on the Claude desktop
//! app's sidebar: the web's separate places strip (`layout/PlaceStrip.tsx`, `layout/placeStrip.css`)
//! is gone, and the "‹ All projects" row became a small "← Projects" link. The places keep the
//! strip's colours and its active rule (`.place.is-active`: the place's colour at 12% and weight
//! 600), translated to a row; the toggle in the first row puts the panel away, and stays reachable
//! in the tab strip's left end while it is away.

use egui::{Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::icon::{self, Node};
use crate::session_nav::clip;
use crate::theme::{self, Weight};
use crate::{app_layout, lucide};

/// The three places (`MODES` in mode.tsx; Run is gated off there and has no counterpart here).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Place {
    #[default]
    Edit,
    Build,
    Verify,
}

impl Place {
    pub const ALL: [Place; 3] = [Place::Edit, Place::Build, Place::Verify];

    pub fn label(self) -> &'static str {
        match self {
            Place::Edit => "Edit",
            Place::Build => "Build",
            Place::Verify => "Verify",
        }
    }

    fn icon(self) -> &'static [Node] {
        match self {
            Place::Edit => lucide::PEN_LINE,
            Place::Build => lucide::BLOCKS,
            Place::Verify => lucide::CIRCLE_CHECK,
        }
    }

    /// `--place-edit` / `--place-build` / `--place-verify`.
    pub fn color(self) -> Color32 {
        match self {
            Place::Edit => theme::PLACE_EDIT,
            Place::Build => theme::PLACE_BUILD,
            Place::Verify => theme::PLACE_VERIFY,
        }
    }

    /// The tooltip, with its chord as the web's keymap has it (keys/commands.ts: `Mod+Alt+1`…).
    fn tip(self) -> &'static str {
        match self {
            Place::Edit => "Edit (⌥⌘1)",
            Place::Build => "Build (⌥⌘2)",
            Place::Verify => "Verify (⌥⌘3)",
        }
    }
}

/// What the head shows.
pub struct Head<'a> {
    pub project: &'a str,
    pub place: Place,
    /// The quick actions under the project's name, top to bottom: a glyph and what it does.
    pub acts: &'a [(&'a [Node], &'a str)],
}

/// What was pressed in the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    /// The toggle in the first row: put the panel away (View > Hide Panel).
    TogglePanel,
    /// "← Projects": leave for all projects.
    AllProjects,
    /// A place other than the one you are in.
    Go(Place),
    /// The quick action at this index of `Head::acts`.
    Act(usize),
}

/// Where the toggle's centre sits from the window's left edge: just after the traffic lights,
/// which run from 19 to 71, with room to breathe as the Claude app leaves.
///
/// Only a Mac puts the window's buttons inside the head. Windows draws its own title bar above
/// it, so there is nothing to clear there and the 100pt would be an empty gutter down the left of
/// every window.
const TOGGLE_X: f32 = if cfg!(target_os = "macos") { 100.0 } else { 16.0 };
/// The "← Projects" row under the first row, a little apart from the name under it.
const BACK_H: f32 = theme::SIDE_ROW + 4.0;
/// The project's name line.
const NAME_H: f32 = 34.0;
/// The space between the quick actions and the places.
const GAP: f32 = 12.0;
/// A row's slot, the row and its 1px margin above and below, as a plasmid row.
const ROW: f32 = theme::SIDE_ROW;
/// How far right the toggle reaches, from the window's left edge, so what follows it in the head
/// can start clear of it.
pub const TOGGLE_END: f32 = TOGGLE_X + 12.0;

fn radius() -> CornerRadius {
    CornerRadius::same(theme::RADIUS_SM as u8)
}

/// Draws the sidebar's ground in `rect`, before anything else in it.
pub fn ground(ui: &egui::Ui, rect: Rect) {
    ui.painter().with_clip_rect(rect).rect_filled(rect, 0.0, theme::RAIL_GROUND);
}

/// The panel toggle, centred in the head band whose top is `top`, measured from the window's left
/// edge `left`. `hidden` picks the glyph: open while the panel is away, close while it is shown.
pub fn toggle(ui: &mut egui::Ui, left: f32, top: f32, hidden: bool) -> bool {
    let at = Pos2::new(left + TOGGLE_X, top + theme::HEAD_H / 2.0);
    let r = Rect::from_center_size(at, vec2(24.0, 24.0));
    let response = ui.interact(r, ui.scope_id().with("sidebar-toggle"), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(r, radius(), theme::alpha(theme::INK, 0.06));
    }
    let glyph = if hidden { lucide::PANEL_LEFT_OPEN } else { lucide::PANEL_LEFT_CLOSE };
    icon::paint(ui.painter(), at, 16.0, glyph, if hovered { theme::INK } else { theme::MUTED });
    let tip = if hidden { "Show panel (⌘⇧\\)" } else { "Hide panel (⌘⇧\\)" };
    response.on_hover_text(tip).clicked()
}

/// Draws the head in the top of the sidebar `rect`: the first row with the toggle, the project's
/// name, its actions, the places. Says where the place's own list starts, and what was pressed.
pub fn head(ui: &mut egui::Ui, rect: Rect, head: &Head<'_>) -> (f32, Option<Pressed>) {
    let painter = ui.painter().with_clip_rect(rect);
    let mut pressed = None;
    // The first row: the traffic lights, then the toggle; the rest of it moves the window.
    if toggle(ui, rect.left(), rect.top(), false) {
        pressed = Some(Pressed::TogglePanel);
    }

    // The way out, a row like the rest: an arrow and "Projects" (Max, 2026-11-23).
    let back_row = Rect::from_min_size(pos2(rect.left(), rect.top() + theme::HEAD_H), vec2(rect.width(), BACK_H));
    let back = Rect::from_min_size(pos2(rect.left() + 4.0, back_row.top() + 1.0), vec2(rect.width() - 8.0, ROW - 2.0));
    let response = ui.interact(back, ui.scope_id().with("sidebar-project"), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        painter.rect_filled(back, radius(), theme::alpha(theme::INK, 0.05));
    }
    icon::paint(
        &painter,
        pos2(back.left() + 10.0 + 8.0, back.center().y),
        theme::SIDE_ICON,
        lucide::ARROW_LEFT,
        if hovered { theme::INK } else { theme::SIDE_GLYPH },
    );
    painter.text(pos2(back.left() + 10.0 + 16.0 + 10.0, back.center().y), Align2::LEFT_CENTER, "Projects", theme::sans(theme::SIDE_TEXT), theme::INK);
    if response.clicked() {
        pressed = Some(Pressed::AllProjects);
    }

    // The project's name, on a line of its own; the whole name shows on hover when it is clipped.
    let name_row = Rect::from_min_size(pos2(rect.left(), back_row.bottom()), vec2(rect.width(), NAME_H));
    let name_font = theme::sans_weight(15.0, Weight::Semibold);
    let name_left = rect.left() + 14.0;
    let room = (rect.right() - 10.0 - name_left).max(0.0);
    let (shown, font, ink) = if head.project.is_empty() {
        ("Untitled project".to_string(), theme::sans(15.0), theme::MUTED)
    } else {
        (clip(ui, head.project, &name_font, room), name_font, theme::INK)
    };
    if shown != head.project && !head.project.is_empty() {
        let name_rect = Rect::from_min_max(pos2(name_left, name_row.top()), pos2(rect.right() - 10.0, name_row.bottom()));
        ui.interact(name_rect, ui.scope_id().with("sidebar-name"), Sense::hover()).on_hover_text(head.project);
    }
    painter.text(pos2(name_left, name_row.center().y), Align2::LEFT_CENTER, shown, font, ink);

    // The quick actions, a row each: the glyph and what it does.
    let mut top = name_row.bottom();
    for (i, (glyph, label)) in head.acts.iter().enumerate() {
        let row = Rect::from_min_size(pos2(rect.left() + 4.0, top + 1.0), vec2(rect.width() - 8.0, ROW - 2.0));
        top += ROW;
        let response = ui.interact(row, ui.scope_id().with(("sidebar-act", i)), Sense::click());
        let hovered = response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            painter.rect_filled(row, radius(), theme::alpha(theme::INK, 0.05));
        }
        icon::paint(
            &painter,
            pos2(row.left() + 10.0 + 8.0, row.center().y),
            theme::SIDE_ICON,
            glyph,
            if hovered { theme::INK } else { theme::SIDE_GLYPH },
        );
        painter.text(pos2(row.left() + 10.0 + 16.0 + 10.0, row.center().y), Align2::LEFT_CENTER, *label, theme::sans(theme::SIDE_TEXT), theme::INK);
        if response.clicked() {
            pressed = Some(Pressed::Act(i));
        }
    }

    // The places, a row each.
    top += GAP;
    for place in Place::ALL {
        let row = Rect::from_min_size(pos2(rect.left() + 4.0, top + 1.0), vec2(rect.width() - 8.0, ROW - 2.0));
        top += ROW;
        let response = ui.interact(row, ui.scope_id().with(("sidebar-place", place)), Sense::click());
        let active = place == head.place;
        let hovered = response.hovered() && !active;
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let fill = if active {
            Some(theme::alpha(place.color(), 0.12))
        } else if hovered {
            Some(theme::alpha(theme::INK, 0.05))
        } else {
            None
        };
        if let Some(fill) = fill {
            painter.rect_filled(row, radius(), fill);
        }
        icon::paint(&painter, pos2(row.left() + 10.0 + 8.0, row.center().y), theme::SIDE_ICON, place.icon(), place.color());
        let weight = if active { Weight::Semibold } else { Weight::Regular };
        painter.text(
            pos2(row.left() + 10.0 + 16.0 + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            place.label(),
            theme::sans_weight(theme::SIDE_TEXT, weight),
            theme::INK,
        );
        // In a row the place you are in is where you are: pressing it again does nothing.
        if response.on_hover_text(place.tip()).clicked() && !active {
            pressed = Some(Pressed::Go(place));
        }
    }
    (top + 8.0, pressed)
}

/// The sidebar's right divider and its drag edge (`.nav-slot[data-full]::after`, `.rail-resizer`),
/// drawn last so the edge takes its clicks. Says the width it was dragged to, and whether the drag
/// is over and the width worth remembering.
pub fn edge(ui: &mut egui::Ui, rect: Rect) -> Option<(f32, bool)> {
    let painter = ui.painter().with_clip_rect(rect);
    let edge = rect.right() - 0.5;
    painter.line_segment([pos2(edge, rect.top()), pos2(edge, rect.bottom())], Stroke::new(1.0, theme::HAIRLINE_STRONG));
    // Clear of the head band, which moves the window.
    let grip = Rect::from_min_max(pos2(rect.right() - 9.0, rect.top() + theme::HEAD_H), rect.max);
    let response = ui.interact(grip, ui.scope_id().with("rail-resizer"), Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
        painter.line_segment([pos2(edge, rect.top()), pos2(edge, rect.bottom())], Stroke::new(1.0, theme::ACCENT));
    }
    let width = if response.double_clicked() {
        Some((theme::NAV_FULL_W, true))
    } else {
        response
            .interact_pointer_pos()
            .filter(|_| response.dragged() || response.drag_stopped())
            .map(|at| (app_layout::clamp_nav(at.x - rect.left()), response.drag_stopped()))
    };
    response.on_hover_text("Drag to resize · double-click to reset");
    width
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, Pos2, Rect, pos2, vec2};

    use super::{Head, Place, Pressed, TOGGLE_X, head};

    /// Clicks at `at` over a few frames of the head in a 200-wide sidebar, and says what was pressed.
    fn click(place: Place, at: Pos2) -> Vec<Pressed> {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 900.0));
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut out = Vec::new();
        for events in [vec![Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
            let input = egui::RawInput { events, screen_rect: Some(rect), ..egui::RawInput::default() };
            ctx.run_ui(input, |ui| {
                let acts: [(&[crate::icon::Node], &str); 1] = [(crate::lucide::PLUS, "New")];
                let (top, pressed) = head(ui, rect, &Head { project: "Reporter plasmids", place, acts: &acts });
                assert!(top > 38.0 + 38.0 + 34.0 + 34.0 + 3.0 * 34.0, "the list starts under the places");
                out.extend(pressed);
            })
            .drop_without_applying_deltas();
        }
        out
    }

    #[test]
    fn the_first_row_holds_the_toggle_clear_of_the_window_s_own_buttons() {
        assert_eq!(click(Place::Edit, pos2(TOGGLE_X, 19.0)), [Pressed::TogglePanel]);
        // On a Mac the traffic lights sit to the toggle's left, and nothing of the sidebar's
        // takes a click there. Windows draws its buttons in a title bar of its own, above the
        // head, so the toggle starts at the edge and there is no gutter to keep clear.
        if cfg!(target_os = "macos") {
            assert!(click(Place::Edit, pos2(45.0, 19.0)).is_empty());
        }
    }

    #[test]
    fn projects_leads_out_the_name_stands_alone_and_an_action_is_a_row() {
        assert_eq!(click(Place::Edit, pos2(150.0, 38.0 + 17.0)), [Pressed::AllProjects]);
        assert!(click(Place::Edit, pos2(30.0, 38.0 + 38.0 + 17.0)).is_empty());
        assert_eq!(click(Place::Edit, pos2(150.0, 38.0 + 38.0 + 34.0 + 17.0)), [Pressed::Act(0)]);
    }

    #[test]
    fn a_place_row_goes_there_and_the_one_you_are_in_does_nothing() {
        let build = pos2(60.0, 38.0 + 38.0 + 34.0 + 34.0 + 12.0 + 34.0 + 17.0);
        assert_eq!(click(Place::Edit, build), [Pressed::Go(Place::Build)]);
        assert!(click(Place::Build, build).is_empty());
    }
}
