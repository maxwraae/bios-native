//! One editor: a strip of tabs and the plasmid in front of it, with everything about looking at
//! that plasmid (where the caret is, the pane's tracks and split, Find, the cards and menus that
//! float over it). A window shows one editor, or two side by side, and a second window has its
//! own; they all share the project and its working copies.
//!
//! The editor being worked on is `App::ed`, and the working copy of its plasmid in front is
//! `App::opened`. Every other editor is parked: it names its plasmid in front by id, and that
//! plasmid's working copy waits in `App::copies`, so two editors on the same plasmid edit one copy
//! and save it once. Saving saves every copy with unsaved changes, and going to an editor that
//! shows another plasmid saves first, as going to another tab does.

use bios_project::ConstructId;
use bios_ui::ask::Ask;
use bios_ui::feature_card::FeatureCard;
use bios_ui::find_bar::{FindBar, FindMatch};
use bios_ui::row_menu::RowMenu;
use bios_ui::selection_readout::PrimerAsk;
use bios_ui::sidebar::Place;
use bios_ui::tab_strip::TabStrip;
use bios_ui::{app_layout, pane, rows, tab_strip, theme};
use egui::{Rect, UiBuilder};

use crate::editing::Spot;
use crate::{App, oligos};

pub(crate) struct Editor {
    /// Unique for the window's life: keeps two editors on the same plasmid apart in what egui
    /// remembers for them (the rows' scroll and zoom).
    pub(crate) uid: u64,
    /// While parked: the plasmid in front, and where its caret or selection was.
    pub(crate) front: Option<ConstructId>,
    pub(crate) spot: Option<Spot>,
    /// Where the person is in the open sequence.
    pub(crate) cursor: rows::Cursor,
    /// The gap a press started at, which a drag selects from.
    pub(crate) anchor: Option<usize>,
    /// A press on the rows or the map is still down: the selection's pill waits for the release,
    /// as a text selection's menu does, so it never jumps about under a drag (PARITY.md).
    pub(crate) sweeping: bool,
    /// The same for a press on the map, and how many turns of the ring a shift-press stood away
    /// from where the pointer reads: a drag round the ring counts on from there.
    pub(crate) map_anchor: i64,
    pub(crate) map_turn: i64,
    pub(crate) plan: Option<rows::Plan>,
    /// The strip's one row laid out, while the map's pane is the strip.
    pub(crate) strip_plan: Option<rows::Plan>,
    /// The sheet naming a new feature or primer on the selection, while it is open.
    pub(crate) ask: Option<Ask>,
    /// + Primer, being answered on the selection's pill.
    pub(crate) primer_ask: Option<PrimerAsk>,
    /// ⌘F's bar while it is open, what it found, and in which revision of what is drawn.
    pub(crate) find: Option<(FindBar, Vec<FindMatch>, u64)>,
    /// The menu on a selection or on what is drawn over the bases, while it is open.
    pub(crate) row_menu: Option<RowMenu>,
    /// The card of the feature that was clicked, while it is open.
    pub(crate) card: Option<FeatureCard>,
    /// The plasmids open as tabs, in the strip's order. The one on screen is `App::opened`.
    pub(crate) tabs: Vec<ConstructId>,
    pub(crate) tab_strip: TabStrip,
    /// The editor pane: which tracks show, the split, and how each construct's map is viewed.
    pub(crate) pane: pane::Pane,
    /// Oligos held against the open plasmid without landing in it (oligos.rs).
    pub(crate) oligos: oligos::Oligos,
}

impl Editor {
    pub(crate) fn new(uid: u64) -> Editor {
        Editor {
            uid,
            front: None,
            spot: None,
            cursor: rows::Cursor::default(),
            anchor: None,
            sweeping: false,
            map_anchor: 0,
            map_turn: 0,
            plan: None,
            strip_plan: None,
            ask: None,
            primer_ask: None,
            find: None,
            row_menu: None,
            card: None,
            tabs: Vec::new(),
            tab_strip: TabStrip::default(),
            pane: pane::Pane::default(),
            oligos: oligos::Oligos::default(),
        }
    }

    /// What the rows and the strip remember this editor's view of a plasmid by. The first editor
    /// keys by the plasmid alone, as Bios did with one editor.
    pub(crate) fn rows_key(&self, plasmid: &str) -> String {
        if self.uid == 0 { plasmid.to_string() } else { format!("{plasmid}#{}", self.uid) }
    }
}

/// A window's second editor, beside the one being worked on.
pub(crate) struct Split {
    /// The editor that is not being worked on, parked.
    pub(crate) other: Editor,
    /// The editor being worked on is the right one.
    pub(crate) ed_right: bool,
    /// Where the divider stands, as a share of the editors' width from the left.
    pub(crate) frac: f32,
}

impl App {
    /// A new editor, numbered.
    pub(crate) fn new_editor(&mut self) -> Editor {
        let uid = self.editors_made;
        self.editors_made += 1;
        Editor::new(uid)
    }

    /// Every editor that is parked, in this window and any other.
    pub(crate) fn parked_editors(&mut self) -> impl Iterator<Item = &mut Editor> {
        let others = self.windows.iter_mut().flat_map(|w| std::iter::once(&mut w.shell.ed).chain(w.shell.split.as_mut().map(|s| &mut s.other)));
        self.split.as_mut().map(|s| &mut s.other).into_iter().chain(others)
    }

    /// A parked editor has this plasmid in front.
    fn fronted_by_parked(&mut self, id: &ConstructId) -> bool {
        self.parked_editors().any(|e| e.front.as_ref() == Some(id))
    }

    /// Parks the editor being worked on: its working copy joins the others, and where its caret
    /// was is kept with it.
    pub(crate) fn park(&mut self) {
        match self.opened.take() {
            Some(opened) => {
                self.ed.spot = Some(opened.editing.spot());
                self.ed.front = Some(opened.id.clone());
                self.copies.insert(opened.id.clone(), opened);
            }
            None => {
                self.ed.front = None;
                self.ed.spot = None;
            }
        }
    }

    /// Takes up `ed` again after [`App::park`]: its plasmid's working copy, at its own place.
    pub(crate) fn unpark(&mut self) {
        let spot = self.ed.spot.take();
        let Some(id) = self.ed.front.take() else {
            self.opened = None;
            return;
        };
        self.opened = self.copies.remove(&id).or_else(|| self.load_copy(&id));
        if let (Some(opened), Some(spot)) = (self.opened.as_mut(), spot) {
            opened.editing.go_to_spot(spot);
        }
        self.show_editing(None, false);
    }

    /// Lets go of the working copy in front, keeping it while another editor shows it.
    pub(crate) fn let_go_of_opened(&mut self) {
        if let Some(opened) = self.opened.take()
            && self.fronted_by_parked(&opened.id)
        {
            self.copies.insert(opened.id.clone(), opened);
        }
    }

    /// Forgets the working copies no editor shows any more.
    pub(crate) fn prune_copies(&mut self) {
        let shown: Vec<ConstructId> = self.parked_editors().filter_map(|e| e.front.clone()).collect();
        self.copies.retain(|id, _| shown.contains(id));
    }

    /// Swaps the editor being worked on with the other one beside it, saving nothing: what is
    /// drawn of the other side, or a press on it.
    pub(crate) fn swap_sides(&mut self) {
        if self.split.is_none() {
            return;
        }
        self.park();
        if let Some(split) = self.split.as_mut() {
            std::mem::swap(&mut self.ed, &mut split.other);
            split.ed_right = !split.ed_right;
        }
        self.unpark();
    }

    /// Makes the left or the right editor the one being worked on. What was typed into a plasmid
    /// the other side does not show is saved first, as leaving a tab saves; refused, it stays
    /// unsaved and is tried again with the next save. False when there is no such side.
    pub(crate) fn focus_side(&mut self, right: bool) -> bool {
        let Some(split) = &self.split else {
            return !right;
        };
        if split.ed_right == right {
            return true;
        }
        if self.opened.as_ref().map(|o| &o.id) != split.other.front.as_ref() {
            self.save();
        }
        self.swap_sides();
        true
    }

    /// Opens a plasmid in the editor beside this one, making one when the window has only this,
    /// and works on it there. Opening the plasmid in front shows the same working copy twice.
    pub(crate) fn open_to_side(&mut self, id: &ConstructId) {
        let same = self.opened.as_ref().is_some_and(|o| o.id == *id);
        if !same && !self.save() {
            return;
        }
        match &self.split {
            Some(split) => {
                let right = !split.ed_right;
                if self.focus_side(right) {
                    self.open_construct(id);
                }
            }
            None => {
                self.park();
                let mut beside = self.new_editor();
                beside.tabs.push(id.clone());
                beside.front = Some(id.clone());
                // The same plasmid starts where it was being looked at.
                beside.spot = self.ed.spot.filter(|_| same);
                let left = std::mem::replace(&mut self.ed, beside);
                self.split = Some(Split { other: left, ed_right: true, frac: 0.5 });
                self.unpark();
                self.revision += 1;
            }
        }
    }

    /// Closes the editor being worked on, when there is another beside it, which takes over.
    /// What was typed is saved first; false when that was refused.
    pub(crate) fn close_side(&mut self) -> bool {
        if self.split.is_none() || !self.save() {
            return false;
        }
        let Some(split) = self.split.take() else {
            return false;
        };
        self.park();
        self.ed = split.other;
        self.unpark();
        self.prune_copies();
        self.revision += 1;
        true
    }
}

impl App {
    /// The editor column in Edit: one editor under its tabs, or two side by side with a divider
    /// between them. A press on a side makes it the one worked on before anything acts on it.
    pub(crate) fn editors(&mut self, ui: &mut egui::Ui, cell: Rect, tabs: Option<Rect>, lead: f32) {
        let Some(frac) = self.split.as_ref().map(|s| s.frac) else {
            self.editor_in(ui, cell, tabs, lead, true);
            return;
        };
        let column = tabs.map_or(cell, |t| t.union(cell));
        let (left, right) = app_layout::sides(column, frac);
        let pressed = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.press_origin()).flatten());
        // Not on the divider's grip, which belongs to neither side.
        if let Some(at) = pressed.filter(|at| column.contains(*at) && (at.x - right.left()).abs() > 4.0) {
            self.focus_side(right.contains(at));
        }
        let ed_right = self.split.as_ref().is_some_and(|s| s.ed_right);
        for on_right in [false, true] {
            let side = if on_right { right } else { left };
            let mine = on_right == ed_right;
            if !mine {
                self.swap_sides();
            }
            let tabs = tabs.map(|t| t.intersect(side));
            self.editor_in(ui, cell.intersect(side), tabs, if on_right { 0.0 } else { lead }, mine);
            if !mine {
                self.swap_sides();
            }
        }
        if let Some((frac, _)) = app_layout::split_divider(ui, column, right)
            && let Some(split) = self.split.as_mut()
        {
            split.frac = frac;
        }
    }

    /// One editor in its own part of the window: the map and rows in `cell`, the tabs above.
    fn editor_in(&mut self, ui: &mut egui::Ui, cell: Rect, tabs: Option<Rect>, lead: f32, focused: bool) {
        let split = self.split.is_some();
        let area = tabs.map_or(cell, |t| t.union(cell));
        ui.scope_builder(UiBuilder::new().id_salt(("editor", self.ed.uid)).max_rect(area), |ui| {
            if split {
                ui.set_clip_rect(area);
            }
            self.work(ui, cell, focused);
            if let Some(tabs) = tabs {
                self.tab_row(ui, tabs, lead, focused);
            }
        });
    }

    /// The tab strip of the editor being drawn, and what its tabs were asked to do. `focused` says
    /// it is the editor being worked on.
    pub(crate) fn tab_row(&mut self, ui: &mut egui::Ui, rect: Rect, lead: f32, focused: bool) {
        let dirty = self.opened.as_ref().filter(|o| o.editing.dirty_since().is_some()).map(|o| o.id.clone());
        let tabs: Vec<tab_strip::Tab> = self
            .ed
            .tabs
            .iter()
            .map(|id| {
                let listed = self.listing.iter().find(|l| l.id == *id);
                tab_strip::Tab {
                    name: listed.map_or("", |l| l.name.as_str()),
                    circular: listed.is_some_and(|l| l.circular),
                    dirty: dirty.as_ref() == Some(id),
                }
            })
            .collect();
        let active = self.opened.as_ref().and_then(|o| self.ed.tabs.iter().position(|t| *t == o.id));
        // The side not being worked on marks its tab in front in grey, not in the place's colour.
        let mark = if focused { theme::PLACE_EDIT } else { theme::HAIRLINE_STRONG };
        let pressed = self.ed.tab_strip.show(ui, rect, lead, &tabs, active, mark);
        match pressed {
            // A tab's click lands in Edit (TabStrip.tsx: `selectTab`, then `setMode("edit")`).
            Some(tab_strip::Pressed::Select(which)) => {
                self.go_to_place(Place::Edit);
                self.select_tab(which);
            }
            Some(tab_strip::Pressed::Close(which)) => self.close_tab(which),
            Some(tab_strip::Pressed::Move(from, to)) => tab_strip::move_tab(&mut self.ed.tabs, from, to),
            Some(tab_strip::Pressed::CloseOthers(which)) => {
                if let Some(keep) = self.ed.tabs.get(which).cloned() {
                    self.open_construct(&keep);
                    self.ed.tabs.retain(|t| *t == keep);
                }
            }
            Some(tab_strip::Pressed::CloseRight(which)) => {
                let front_goes = self.opened.as_ref().and_then(|o| self.ed.tabs.iter().position(|t| *t == o.id)).is_some_and(|at| at > which);
                if front_goes {
                    self.select_tab(which);
                }
                self.ed.tabs.truncate(which + 1);
            }
            Some(tab_strip::Pressed::NewWindow(which)) => {
                let id = self.ed.tabs.get(which).cloned();
                self.open_in_new_window(id);
            }
            Some(tab_strip::Pressed::ToSide(which)) => {
                if let Some(id) = self.ed.tabs.get(which).cloned() {
                    self.go_to_place(Place::Edit);
                    self.open_to_side(&id);
                }
            }
            None => {}
        }
    }
}
