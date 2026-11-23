//! The editor pane's own state and its split handle. Ported from Bios `editor/PlasmidEditor.tsx`
//! (the view, `mapFrac` and the handle, lines 406-452, 626-652, 1105-1127) and `styles.css`
//! (`.split-handle`, lines 2707-2740). Where each child sits is `bios_layout::pane`.

use std::collections::HashMap;

use bios_core::Construct;
use bios_core::enzymes::CutSite;
use bios_layout::overview::navigate;
use bios_layout::overview::viewport::{Viewport, char_width_for_width};
use bios_layout::pane::{
    Band, DOOR_SECONDS, Focus, HAND_SECONDS, MAP_FRAC_DEFAULT, MapFollow, MapStyle, ROWS_HAND_BACK_WIDTH, drag_map_frac, map_follow,
    width_below_letters,
};
use bios_layout::translations::Translation;
use egui::{CornerRadius, CursorIcon, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::canvas_tools::Panel;
use crate::enzymes_menu::{CutsiteView, cut_sites_with};
use crate::enzymes_panel::EnzymesPanel;
use crate::map::{MapView, Motion};
use crate::rows::{Cursor, Strip};
use crate::side_panel::Side;
use crate::theme;
use crate::view_tab::Layers;

/// How one construct is viewed: its map style (remembered per file, as the web does) and where the
/// map is looking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileView {
    pub map_style: MapStyle,
    pub map: MapView,
}

impl FileView {
    /// The view a construct opens in: the topology's natural map, and the map's window as the
    /// style's zoom jump leaves it (`navigate::style_jump`), so a long linear construct opens on its
    /// first four tenths.
    pub fn opening(c: &Construct) -> FileView {
        let style = MapStyle::natural(c.circular);
        let ring = style == MapStyle::Circular;
        FileView { map_style: style, map: MapView { vp: navigate::style_jump(navigate::OPENING, ring), ring } }
    }

    /// The ring and line buttons. Fold is not drawn natively; it never becomes the style.
    pub fn set_style(&mut self, style: MapStyle) {
        if style == MapStyle::Fold {
            return;
        }
        let ring = style == MapStyle::Circular;
        self.map_style = style;
        self.map = MapView { vp: navigate::style_jump(self.map.vp, ring), ring };
    }
}

/// What the pane remembers between frames.
#[derive(Clone, Debug)]
pub struct Pane {
    /// Map only, map and sequence, or sequence only. Every construct opens split
    /// (`viewPrefs.ts` `openingView`); a switch holds while it stays open.
    pub focus: Focus,
    /// The map's share of the height it shares with the rows.
    pub map_frac: f64,
    views: HashMap<String, FileView>,
    /// Which construct is in front, so another one opening goes back to split.
    front: Option<String>,
    /// Which side panel or popover the tool row has open. Only ever one.
    pub panel: Option<Panel>,
    /// What the sliders popover chose to show.
    pub layers: Layers,
    /// What the scissors dial chose.
    pub cut_view: CutsiteView,
    /// Enzymes put on the map by name from the Enzymes panel, drawn whatever the dial says.
    pub shown_enzymes: Vec<String>,
    /// The side panel on the pane's right, Contents or Enzymes, when one is open.
    pub side: Option<Side>,
    pub enzymes_panel: EnzymesPanel,
    pub contents_panel: crate::contents_panel::ContentsPanel,
    /// Translations asked for from the selection's menu, per open construct: held for the
    /// session and never saved, since a GenBank file has no place for a bare translation.
    translations: HashMap<String, Vec<Translation>>,
    /// The cut sites drawn, worked out once per revision of what is drawn, dial setting and names.
    sites: Option<(u64, CutsiteView, Vec<String>, Vec<CutSite>)>,
    /// The front map's motion: its eased zoom, and its ring as a flywheel. Another construct coming
    /// to the front starts still.
    pub motion: Motion,
    /// The map's width the last time the rows were asked to follow it, so a pan is not a zoom.
    followed: Option<f64>,
    /// When a hand was last on the map: a wheel over it or a press on it.
    pub map_hand: f64,
    /// Until when the doors between the map and the sequence stay shut.
    doors_until: f64,
    /// The strip, while the split's map pane is one: from the moment the map reaches the letters
    /// until the strip is zoomed back out past them (`letters`).
    pub strip: Option<Strip>,
    /// The focus the map gave way from when it turned into the sequence, so zooming the rows back
    /// out returns to the view that was left (`cameFromFocus`).
    came_from: Focus,
}

impl Default for Pane {
    fn default() -> Self {
        Pane {
            focus: Focus::Split,
            map_frac: MAP_FRAC_DEFAULT,
            views: HashMap::new(),
            front: None,
            panel: None,
            layers: Layers::default(),
            cut_view: CutsiteView::default(),
            shown_enzymes: Vec::new(),
            translations: HashMap::new(),
            side: None,
            enzymes_panel: EnzymesPanel::default(),
            contents_panel: crate::contents_panel::ContentsPanel::default(),
            sites: None,
            motion: Motion::spinning(),
            followed: None,
            map_hand: f64::NEG_INFINITY,
            doors_until: f64::NEG_INFINITY,
            strip: None,
            came_from: Focus::Split,
        }
    }
}

impl Pane {
    /// The view of the construct known as `key`, made the first time it is asked for.
    pub fn view(&mut self, key: &str, c: &Construct) -> &mut FileView {
        if self.front.as_deref() != Some(key) {
            self.front = Some(key.to_string());
            self.focus = Focus::Split;
            self.motion = Motion::spinning();
            self.strip = None;
        }
        self.views.entry(key.to_string()).or_insert_with(|| FileView::opening(c))
    }

    /// Asks whether the rows follow the map pane now that it looks at `map` (the strip's window
    /// while the strip is up), `box_w` wide on a construct `length` long, at `now`: the base to
    /// bring to the middle of the rows when they do. At the letters the split's map becomes the
    /// strip, and the map alone turns the screen to the sequence (`PlasmidEditor.tsx` 778-824).
    pub fn follow_map(&mut self, map: Viewport, box_w: f64, length: i64, now: f64, cursor: &Cursor) -> Option<usize> {
        if length <= 0 || box_w <= 0.0 {
            return None;
        }
        // Recorded on every frame, hand or no hand, so the first scroll after a mount is not a zoom.
        let zoomed = self.followed.is_none_or(|was| (was - map.width).abs() >= 5e-7);
        self.followed = Some(map.width);
        let char_width = char_width_for_width(map.width, box_w, length);
        let by_hand = now - self.map_hand <= HAND_SECONDS;
        match map_follow(zoomed, by_hand, now < self.doors_until, self.focus, char_width, self.strip.is_some()) {
            MapFollow::Stay => return None,
            MapFollow::Scroll => {}
            MapFollow::ToStrip => {
                self.doors_until = now + DOOR_SECONDS;
                self.strip = Some(Strip::open(map, cursor));
                // The map is put away where it handed over; the rest of its eased zoom goes with it.
                self.motion.jump(now);
            }
            MapFollow::ToSequence => {
                self.doors_until = now + DOOR_SECONDS;
                self.came_from = self.focus;
                self.focus = Focus::Sequence;
            }
        }
        let bp = (map.center.rem_euclid(1.0) * length as f64).floor().clamp(0.0, (length - 1) as f64);
        Some(bp as usize)
    }

    /// The strip has been zoomed out past the letters at `vp`: it gives the pane back to the map,
    /// which zooms out to that stretch, wide enough that a base is under `EXIT_AT` pixels across
    /// `box_w` (`onStripZoomedOut`). Nothing while a door is shut.
    pub fn strip_handed_back(&mut self, key: &str, vp: Viewport, box_w: f64, length: i64, now: f64) {
        if now < self.doors_until {
            return;
        }
        self.strip = None;
        self.doors_until = now + DOOR_SECONDS;
        self.focus_map(key, vp.center, width_below_letters(vp.width, box_w, length), now);
    }

    /// The rows have been zoomed out past the letters with `base` in the middle of their view: the
    /// view goes back to the focus the map gave way from, and the map zooms to that stretch
    /// (`onRowsZoomedOut`). Nothing while a door is shut: the rows keep zooming while the wheel is
    /// still turning, and only the first hand-back of a gesture means anything.
    pub fn rows_handed_back(&mut self, key: &str, base: usize, box_w: f64, length: i64, now: f64) {
        if now < self.doors_until || length <= 0 {
            return;
        }
        self.doors_until = now + DOOR_SECONDS;
        self.focus = self.came_from;
        self.strip = None;
        self.focus_map(key, base as f64 / length as f64, width_below_letters(ROWS_HAND_BACK_WIDTH, box_w, length), now);
    }

    /// Flies the map of the construct known as `key` to `width` about `center` (the map's `focus`).
    fn focus_map(&mut self, key: &str, center: f64, width: f64, now: f64) {
        if let Some(view) = self.views.get(key) {
            self.motion.focus(view.map.vp, center, width, now);
        }
    }

    /// The strip belongs to the split: leaving it puts the map back.
    pub fn keep_strip(&mut self) {
        if self.focus != Focus::Split {
            self.strip = None;
        }
    }

    /// The ring and line buttons, for the construct known as `key`.
    pub fn set_style(&mut self, key: &str, style: MapStyle) {
        if let Some(view) = self.views.get_mut(key) {
            view.set_style(style);
            // A style button is a zoom jump: it wins over a zoom in flight and takes the ring back.
            let now = self.motion.now;
            self.motion.jump(now);
        }
    }

    /// The cut sites the rows, the map and the seam draw, for `revision` of `c`.
    pub fn sites(&mut self, revision: u64, c: &Construct) -> &[CutSite] {
        let view = self.cut_view;
        if self.sites.as_ref().is_none_or(|(r, v, n, _)| *r != revision || *v != view || *n != self.shown_enzymes) {
            self.sites = Some((revision, view, self.shown_enzymes.clone(), cut_sites_with(c, view, &self.shown_enzymes)));
        }
        self.sites.as_ref().map_or(&[], |(_, _, _, s)| s.as_slice())
    }

    /// The floating translations held against the construct known as `key`.
    pub fn translations_of(&self, key: &str) -> &[Translation] {
        self.translations.get(key).map_or(&[], Vec::as_slice)
    }

    /// Floats a translation over the construct known as `key`. The same one asked twice is held once.
    pub fn add_translation(&mut self, key: &str, t: Translation) {
        let list = self.translations.entry(key.to_string()).or_default();
        if !list.contains(&t) {
            list.push(t);
        }
    }

    /// Takes the `index`th translation off the construct known as `key`.
    pub fn remove_translation(&mut self, key: &str, index: usize) {
        if let Some(list) = self.translations.get_mut(key)
            && index < list.len()
        {
            list.remove(index);
        }
    }

    /// Opens a side panel, or closes it when it is the one open. Only one is on screen at a time.
    pub fn toggle_side(&mut self, side: Side) {
        self.side = if self.side == Some(side) { None } else { Some(side) };
    }

    /// Puts an enzyme on the map by name, or takes it off (`EnzymesPanel` `toggle`). True when it
    /// went on.
    pub fn toggle_shown(&mut self, name: &str) -> bool {
        let before = self.shown_enzymes.len();
        self.shown_enzymes.retain(|n| !n.eq_ignore_ascii_case(name));
        if self.shown_enzymes.len() == before {
            self.shown_enzymes.push(name.to_string());
            return true;
        }
        false
    }
}

/// Kept when Features is off: primer sites and parts, which have layers of their own.
fn own_layer(f: &bios_core::Feature) -> bool {
    bios_layout::overview::display::is_primer_bind(&f.kind) || bios_core::parts::is_part(f)
}

/// What the map and the rows draw of `c` under `layers`: all of it, or, with Features off, the
/// construct without its annotations (primer sites and parts stay, on their own layers).
pub fn drawn(c: &Construct, layers: Layers) -> std::borrow::Cow<'_, Construct> {
    if layers.features {
        return std::borrow::Cow::Borrowed(c);
    }
    let mut bare = c.clone();
    bare.features.retain(own_layer);
    std::borrow::Cow::Owned(bare)
}

/// Which of `c`'s own features the `index`th feature of [`drawn`] is.
pub fn feature_in_file(c: &Construct, layers: Layers, index: usize) -> Option<usize> {
    if layers.features {
        return Some(index);
    }
    c.features.iter().enumerate().filter(|(_, f)| own_layer(f)).nth(index).map(|(i, _)| i)
}

/// A pane band as a rectangle inside `pane`.
pub fn band_rect(pane: Rect, band: Band) -> Rect {
    Rect::from_min_max(pos2(pane.left(), pane.top() + band.top as f32), pos2(pane.right(), pane.top() + band.bottom as f32))
}

/// The draggable divider between the map and the sequence: a hairline inset 16 from each side, and
/// a 34×4 grip in muted ink while the pointer is over it or dragging. Returns the map's new share
/// while it is dragged. `pane` is the whole pane and `seam_h` the seam's height at its floor.
pub fn split_handle(ui: &mut Ui, rect: Rect, pane: Rect, seam_h: f32) -> Option<f64> {
    let response = ui.interact(rect, ui.scope_id().with("split-handle"), Sense::drag()).on_hover_text("Drag to resize");
    let painter = ui.painter();
    painter.hline(rect.left() + 16.0..=rect.right() - 16.0, rect.center().y, Stroke::new(1.0, theme::HAIRLINE));
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeRow);
        painter.rect_filled(Rect::from_center_size(rect.center(), vec2(34.0, 4.0)), CornerRadius::same(2), theme::MUTED);
    }
    if !response.dragged() {
        return None;
    }
    let y = response.interact_pointer_pos()?.y - pane.top();
    Some(drag_map_frac(f64::from(y), f64::from(pane.height()), f64::from(seam_h)))
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;
    use bios_layout::overview::viewport::{Projection, Viewport};
    use bios_layout::pane::Focus;

    use super::Pane;
    use crate::rows::Cursor;

    fn construct() -> Construct {
        Construct { name: "t".into(), sequence: "ACGT".repeat(1500), ..Construct::default() }
    }

    /// 114 of 6,000 bases across 1,180 px: past the letters, at about 10 px a base.
    const LETTERS: Viewport = Viewport { center: 0.2, width: 0.019, projection: Projection::Line };

    /// A pane with a construct in front, `focus` chosen, and a hand on the map at time 0.
    fn pane(focus: Focus) -> Pane {
        let mut pane = Pane::default();
        pane.view("t", &construct());
        pane.focus = focus;
        pane.map_hand = 0.0;
        pane
    }

    #[test]
    fn the_split_turns_into_the_strip_at_the_letters_and_back_into_the_map_after_the_door() {
        let mut pane = pane(Focus::Split);
        let cursor = Cursor { caret: Some(10), ..Cursor::default() };
        let base = pane.follow_map(LETTERS, 1180.0, 6000, 0.1, &cursor);
        assert_eq!(base, Some(1200), "the rows below scroll to the map's window");
        assert_eq!(pane.strip.map(|s| s.vp), Some(LETTERS), "the strip opens where the map was looking");
        assert_eq!(pane.focus, Focus::Split, "the rows stay below it");
        // Still inside the door's half second: the rest of the gesture cannot carry it back out.
        pane.strip_handed_back("t", LETTERS, 1180.0, 6000, 0.3);
        assert!(pane.strip.is_some());
        pane.strip_handed_back("t", LETTERS, 1180.0, 6000, 0.7);
        assert!(pane.strip.is_none(), "zoomed out past the letters, the map takes the pane back");
    }

    #[test]
    fn the_map_alone_turns_into_the_sequence_and_zooming_the_rows_out_returns_to_it() {
        let mut pane = pane(Focus::Map);
        pane.follow_map(LETTERS, 1180.0, 6000, 0.1, &Cursor::default());
        assert_eq!(pane.focus, Focus::Sequence);
        assert!(pane.strip.is_none(), "the strip belongs to the split");
        pane.rows_handed_back("t", 1200, 1180.0, 6000, 0.3);
        assert_eq!(pane.focus, Focus::Sequence, "not through a door just used");
        pane.rows_handed_back("t", 1200, 1180.0, 6000, 0.7);
        assert_eq!(pane.focus, Focus::Map, "back to the view that was left");
    }

    #[test]
    fn the_rows_hand_back_to_the_split_when_the_map_never_gave_way() {
        // The web's `cameFromFocus` starts at split: sequence-only chosen by its button and zoomed
        // out comes back to the split.
        let mut pane = pane(Focus::Sequence);
        pane.rows_handed_back("t", 100, 1180.0, 6000, 1.0);
        assert_eq!(pane.focus, Focus::Split);
    }

    #[test]
    fn leaving_the_split_or_the_construct_puts_the_map_back() {
        let mut pane = pane(Focus::Split);
        pane.follow_map(LETTERS, 1180.0, 6000, 0.1, &Cursor::default());
        assert!(pane.strip.is_some());
        pane.focus = Focus::Map;
        pane.keep_strip();
        assert!(pane.strip.is_none());
        pane.focus = Focus::Split;
        pane.map_hand = 2.0;
        pane.follow_map(Viewport { width: 0.018, ..LETTERS }, 1180.0, 6000, 2.1, &Cursor::default());
        assert!(pane.strip.is_some());
        pane.view("other", &construct());
        assert!(pane.strip.is_none());
    }
}
