//! The editor pane's column: the map, the split handle, the sequence, and the navigator seam at the
//! floor. Ported from Bios `editor/PlasmidEditor.tsx` (the pane's children and `onSplitMove`, lines
//! 406-452, 626-652, 832-1216) and `styles.css` (`.editor-pane`, `.overview-host`, `.split-handle`,
//! lines 2675-2740).
//!
//! The web lays the pane out as a flex column. The map host takes `flex: mapFrac` and the sequence
//! host `flex: 1 - mapFrac`; the handle and the seam take their own heights. With one panel on
//! screen it takes `flex: 1`, everything the seam leaves.

/// Which tracks the construct shows (`viewPrefs.ts` `ViewFocus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// Map only.
    Map,
    /// Map and sequence, stacked, with the handle between them.
    #[default]
    Split,
    /// Sequence only.
    Sequence,
}

/// How the map is drawn (`viewPrefs.ts` `MapStyle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStyle {
    Circular,
    Linear,
    /// The single-stranded fold. Offered and greyed with its reason: Bios native does not fold.
    Fold,
}

impl MapStyle {
    /// A construct opens on the map its topology draws naturally.
    pub fn natural(circular: bool) -> MapStyle {
        if circular { MapStyle::Circular } else { MapStyle::Linear }
    }
}

/// The map's share of the shared height when a file opens (`useState(0.5)`).
pub const MAP_FRAC_DEFAULT: f64 = 0.5;
/// How far the handle may be dragged, as the map's share (`onSplitMove`).
pub const MAP_FRAC_MIN: f64 = 0.18;
pub const MAP_FRAC_MAX: f64 = 0.82;
/// `.split-handle`'s height.
pub const SPLIT_HANDLE_H: f64 = 11.0;
/// `--navigator-h`: 12 + the 42px track + 12 + two 1px rules.
pub const NAVIGATOR_H: f64 = 68.0;
/// `--tools-air`: the gap between the floating tool row and the floor it stands on.
pub const TOOLS_AIR: f64 = 16.0;
/// `--title-band`: what sequence-only keeps clear above the rows for the corner label.
pub const TITLE_BAND: f64 = 8.0 + 50.0 + 8.0;
/// The tool row's own height (`.oge-btn-group`).
pub const TOOLS_H: f64 = 34.0;

/// A band of the pane, as y from the pane's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub top: f64,
    pub bottom: f64,
}

impl Band {
    pub fn height(self) -> f64 {
        self.bottom - self.top
    }
}

/// Where each child of the pane sits. A child that is not mounted is `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLayout {
    pub map: Option<Band>,
    pub handle: Option<Band>,
    /// The sequence host. In sequence-only its top is the title band, which the rows never enter
    /// (`.editor-pane[data-focus="sequence"] .editor-host { padding-top }`).
    pub rows: Option<Band>,
    pub seam: Option<Band>,
}

/// Pixels a base must have on the map before the map gives way to the letters (`LETTERS_AT`).
pub const LETTERS_AT: f64 = 9.0;
/// Pixels a base has on the map when the strip or the rows hand back to it (`EXIT_AT`): well under
/// [`LETTERS_AT`], so a hand-back lands clear of the door it came through. The gap between the two
/// is the hysteresis; one threshold for both directions is a switch sitting on its own trip point.
pub const EXIT_AT: f64 = 5.0;
/// How long after a wheel or a press on the map the map counts as driven by hand, in seconds
/// (`markMapDriving`'s 600 ms): its zoom eases on after the gesture that caused it.
pub const HAND_SECONDS: f64 = 0.6;
/// How long a door that was just used stays shut, in seconds (`lockDoors`' 500 ms), so the rest of
/// the wheel's travel does not carry the view straight back.
pub const DOOR_SECONDS: f64 = 0.5;
/// The window the map opens at when the wrapped rows hand back to it, before [`width_below_letters`]
/// widens it (`onRowsZoomedOut`'s 0.25).
pub const ROWS_HAND_BACK_WIDTH: f64 = 0.25;

/// What a change in the map pane's window asks for (`PlasmidEditor.tsx` 778-824). In the split
/// that window is the map's, or the strip's while the strip is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapFollow {
    /// Nothing: a pan, an echo of the rows' own selection, or a door just used.
    Stay,
    /// Scroll the rows to the window.
    Scroll,
    /// The split has reached the letters: the map pane becomes the strip at the map's window, and
    /// the rows below scroll to it.
    ToStrip,
    /// The map alone has reached the letters: the screen becomes the sequence, at the map's window.
    ToSequence,
}

/// Whether the rows follow the map pane. Only a ZOOM moves them (a pan is not a request to read
/// somewhere else), only while a hand is on the pane (the map also re-centres itself on a click in
/// the rows, and following that would scroll them out from under the pointer), and never while a
/// door is shut. `char_width` is the pixels a base has at the window across the pane's width, and
/// `strip` whether the strip is already up.
pub fn map_follow(zoomed: bool, by_hand: bool, doors_shut: bool, focus: Focus, char_width: f64, strip: bool) -> MapFollow {
    if !zoomed || !by_hand || doors_shut {
        return MapFollow::Stay;
    }
    let letters = char_width >= LETTERS_AT;
    match focus {
        Focus::Split if letters && !strip => MapFollow::ToStrip,
        Focus::Split => MapFollow::Scroll,
        Focus::Map if letters => MapFollow::ToSequence,
        Focus::Map | Focus::Sequence => MapFollow::Stay,
    }
}

/// The width a hand-back opens the map at: `want`, or wider, so that a base is at most
/// [`EXIT_AT`] pixels across a pane `box_w` wide on a construct `length` long (`widthBelowLetters`).
pub fn width_below_letters(want: f64, box_w: f64, length: i64) -> f64 {
    if box_w <= 0.0 || length <= 0 {
        return want;
    }
    let floor = box_w / (EXIT_AT * length as f64);
    want.max(floor.min(1.0))
}

/// Lays out a pane `h` high. `seam` says whether the navigator is mounted (it is not for Fold, which
/// has no window to show).
pub fn pane_layout(h: f64, focus: Focus, map_frac: f64, seam: bool) -> PaneLayout {
    let h = h.max(0.0);
    let seam_h = if seam { NAVIGATOR_H.min(h) } else { 0.0 };
    let floor = h - seam_h;
    let seam = seam.then_some(Band { top: floor, bottom: h });
    match focus {
        Focus::Map => PaneLayout { map: Some(Band { top: 0.0, bottom: floor }), handle: None, rows: None, seam },
        Focus::Sequence => PaneLayout { map: None, handle: None, rows: Some(Band { top: TITLE_BAND.min(floor), bottom: floor }), seam },
        Focus::Split => {
            let handle_h = SPLIT_HANDLE_H.min(floor);
            let free = (floor - handle_h).max(0.0);
            let map_h = free * clamp_frac(map_frac);
            let map = Band { top: 0.0, bottom: map_h };
            let handle = Band { top: map_h, bottom: map_h + handle_h };
            PaneLayout { map: Some(map), handle: Some(handle), rows: Some(Band { top: handle.bottom, bottom: floor }), seam }
        }
    }
}

fn clamp_frac(f: f64) -> f64 {
    if f.is_nan() { MAP_FRAC_DEFAULT } else { MAP_FRAC_MIN.max(MAP_FRAC_MAX.min(f)) }
}

/// The map's share for a pointer `y` below the pane's top while the handle is dragged: measured
/// against the height the map and the rows share, since the seam at the floor is not theirs to
/// split (`onSplitMove`).
pub fn drag_map_frac(y: f64, h: f64, seam_h: f64) -> f64 {
    let f = y / 1f64.max(h - seam_h);
    clamp_frac(f)
}

/// The bottom of the floating tool row, as y from the pane's top: the floor it stands on (the
/// seam's top where there is a seam), less `--tools-air` (`.canvas-tools { bottom }`).
pub fn tools_bottom(h: f64, seam: bool) -> f64 {
    let floor = if seam { NAVIGATOR_H } else { 0.0 };
    h - floor - TOOLS_AIR
}

/// What the rows keep clear under their last row so it can scroll out from under the tool row
/// (`.seq-rows-content { padding-bottom }`): the floor, the air, the row and one more air.
pub fn rows_run_off(seam: bool) -> f64 {
    let floor = if seam { NAVIGATOR_H } else { 0.0 };
    floor + TOOLS_AIR + TOOLS_H + TOOLS_AIR
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_rows_follow_a_zoom_by_hand_and_never_a_pan_an_echo_or_a_shut_door() {
        use super::{Focus, LETTERS_AT, MapFollow, map_follow};
        assert_eq!(map_follow(true, true, false, Focus::Split, 2.0, false), MapFollow::Scroll);
        assert_eq!(map_follow(false, true, false, Focus::Split, 2.0, false), MapFollow::Stay, "a pan");
        assert_eq!(map_follow(true, false, false, Focus::Split, 2.0, false), MapFollow::Stay, "the map re-centring on a click in the rows");
        assert_eq!(map_follow(true, true, true, Focus::Split, 2.0, false), MapFollow::Stay, "a door just used");
        assert_eq!(map_follow(true, true, false, Focus::Map, LETTERS_AT - 0.1, false), MapFollow::Stay);
        assert_eq!(map_follow(true, true, false, Focus::Map, LETTERS_AT, false), MapFollow::ToSequence);
        assert_eq!(map_follow(true, true, false, Focus::Sequence, 20.0, false), MapFollow::Stay, "no map on screen");
    }

    #[test]
    fn the_split_becomes_the_strip_at_the_letters_and_the_rows_below_keep_following() {
        use super::{Focus, LETTERS_AT, MapFollow, map_follow};
        assert_eq!(map_follow(true, true, false, Focus::Split, LETTERS_AT - 0.1, false), MapFollow::Scroll);
        assert_eq!(map_follow(true, true, false, Focus::Split, LETTERS_AT, false), MapFollow::ToStrip);
        assert_eq!(map_follow(true, true, false, Focus::Split, 20.0, true), MapFollow::Scroll, "the strip is up: the rows follow it");
        assert_eq!(
            map_follow(true, true, false, Focus::Split, 7.5, true),
            MapFollow::Scroll,
            "a strip zoomed back under the letters is still a strip"
        );
        assert_eq!(map_follow(true, true, true, Focus::Split, 20.0, false), MapFollow::Stay, "not through a door just used");
    }

    #[test]
    fn a_hand_back_opens_the_map_where_a_base_is_five_pixels() {
        use super::{EXIT_AT, width_below_letters};
        // 6,093 bp across 1,180 px: five pixels a base is 236 bases.
        let w = width_below_letters(0.01, 1180.0, 6093);
        assert!((w - 1180.0 / (EXIT_AT * 6093.0)).abs() < 1e-12);
        assert!((w * 6093.0 - 236.0).abs() < 0.1);
        assert_eq!(width_below_letters(0.25, 1180.0, 6093), 0.25, "a wider window asked for stands");
        assert_eq!(width_below_letters(0.1, 1180.0, 100), 1.0, "never wider than the whole");
        assert_eq!(width_below_letters(0.1, 0.0, 6093), 0.1, "before the pane is measured");
    }

    use super::*;

    #[test]
    fn split_shares_what_the_handle_and_seam_leave() {
        let p = pane_layout(800.0, Focus::Split, 0.5, true);
        let free = 800.0 - 11.0 - 68.0;
        assert_eq!(p.map, Some(Band { top: 0.0, bottom: free / 2.0 }));
        assert_eq!(p.handle, Some(Band { top: free / 2.0, bottom: free / 2.0 + 11.0 }));
        assert_eq!(p.rows, Some(Band { top: free / 2.0 + 11.0, bottom: 732.0 }));
        assert_eq!(p.seam, Some(Band { top: 732.0, bottom: 800.0 }));
    }

    #[test]
    fn one_panel_takes_everything_above_the_seam() {
        let p = pane_layout(800.0, Focus::Map, 0.3, true);
        assert_eq!(p.map, Some(Band { top: 0.0, bottom: 732.0 }));
        assert!(p.handle.is_none() && p.rows.is_none());
        let p = pane_layout(800.0, Focus::Sequence, 0.3, false);
        assert_eq!(p.rows, Some(Band { top: TITLE_BAND, bottom: 800.0 }));
        assert!(p.map.is_none() && p.seam.is_none());
    }

    #[test]
    fn the_drag_is_measured_against_the_shared_height_and_held_in_range() {
        assert_eq!(drag_map_frac(366.0, 800.0, 68.0), 0.5);
        assert_eq!(drag_map_frac(10.0, 800.0, 68.0), MAP_FRAC_MIN);
        assert_eq!(drag_map_frac(790.0, 800.0, 68.0), MAP_FRAC_MAX);
        // A pane no taller than its seam divides by 1, never by zero.
        assert_eq!(drag_map_frac(0.5, 10.0, 68.0), 0.5);
    }

    #[test]
    fn a_pane_shorter_than_its_parts_never_goes_negative() {
        let p = pane_layout(40.0, Focus::Split, 0.5, true);
        for band in [p.map, p.handle, p.rows, p.seam].into_iter().flatten() {
            assert!(band.height() >= 0.0, "{band:?}");
        }
    }

    #[test]
    fn the_tool_row_stands_on_the_seam() {
        assert_eq!(tools_bottom(800.0, true), 800.0 - 68.0 - 16.0);
        assert_eq!(tools_bottom(800.0, false), 784.0);
        assert_eq!(rows_run_off(true), 68.0 + 16.0 + 34.0 + 16.0);
    }
}
