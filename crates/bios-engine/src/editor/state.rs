//! The caret, the selection and the undo steps of one open construct. Ported from the web app's `editorState.ts`.
//!
//! Two rules everything else leans on. A selection clears the caret and a caret clears the
//! selection, so the two are never both set. And an undo step is recorded only by an edit that
//! really changes the bases or the features: the first construct written is not a step, because
//! there was nothing to go back to, and writing an identical construct is not one either.
//!
//! A step is the construct as it was, with the selection or caret it was left with. The history
//! keeps the last [`HISTORY_LIMIT`] of them and forgets the oldest first.

use std::collections::BTreeMap;

use bios_core::Construct;

/// How many steps back a construct remembers.
pub const HISTORY_LIMIT: usize = 200;

/// Which end of a selection the gesture finished on. It is the end that moves when the selection
/// is extended with shift and an arrow, and the end a primer made from it reads its direction off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionEnd {
    Start,
    #[default]
    End,
}

/// Selected bases, 0-based and inclusive at both ends. An `end` before the `start` runs through
/// the origin of a circular construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
    pub active_end: SelectionEnd,
}

impl Selection {
    /// A selection that finished on its end, which is what a gesture that does not say means.
    pub const fn new(start: usize, end: usize) -> Self {
        Selection { start, end, active_end: SelectionEnd::End }
    }

    pub const fn finished_at(start: usize, end: usize, active_end: SelectionEnd) -> Self {
        Selection { start, end, active_end }
    }
}

/// One undo step: the construct and the place in it, as they were.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryEntry {
    pub construct: Option<Construct>,
    pub selection: Option<Selection>,
    pub caret: Option<usize>,
}

/// The steps behind and ahead of the construct. The last entry of each is the nearest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub past: Vec<HistoryEntry>,
    pub future: Vec<HistoryEntry>,
}

/// Several values written at once, by something that is not a person's edit: opening a construct,
/// a change an agent made, a measurement from the drawing. A value left `None` is left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Patch {
    /// Replaces the construct, even one that differs only in its name or topology.
    pub construct: Option<Construct>,
    /// Replaces the history. `Some(History::default())` wipes it, which is how a change made from
    /// outside becomes the new starting point.
    pub history: Option<History>,
    pub char_width: Option<f32>,
}

/// What can be done to one construct's editing state. The web app's action names are given so
/// that its code can be followed.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `SELECTION_LAYER_UPDATE`. Selects bases and clears the caret. `None` is ignored, not
    /// stored: a selection is collapsed by moving the caret.
    Select(Option<Selection>),
    /// `CARET_POSITION_UPDATE`. Puts the caret in a gap, `0..=length`, and clears the selection.
    /// `None` leaves neither a caret nor a selection.
    MoveCaret(Option<usize>),
    /// `SEQUENCE_DATA_UPDATE`. A person's edit, and what undo walks. A construct with the same
    /// bases and features as the current one is ignored altogether.
    SetConstruct(Construct),
    /// `VECTOR_EDITOR_UPDATE`. A bulk write; send it with [`EditorSlice::apply_unrecorded`] unless
    /// it should be a step.
    Update(Patch),
    /// `VE_UNDO`. Does nothing when there is no step behind.
    Undo,
    /// `VE_REDO`. Does nothing when there is no step ahead.
    Redo,
}

/// The editing state of one open construct.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorSlice {
    construct: Option<Construct>,
    selection: Option<Selection>,
    caret: Option<usize>,
    history: History,
    char_width: Option<f32>,
}

#[derive(Clone, Copy)]
enum Direction {
    Back,
    Forward,
}

/// Only the bases and the features decide whether two constructs are the same edit.
fn same_construct(a: Option<&Construct>, b: &Construct) -> bool {
    a.is_some_and(|a| a.sequence == b.sequence && a.features == b.features)
}

impl EditorSlice {
    pub fn construct(&self) -> Option<&Construct> {
        self.construct.as_ref()
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    /// The caret, as a gap between bases: `0` is before the first base.
    pub fn caret(&self) -> Option<usize> {
        self.caret
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn char_width(&self) -> Option<f32> {
        self.char_width
    }

    /// Applies an action, recording an undo step when it changes the construct. Returns false
    /// when the action was ignored and nothing changed.
    pub fn apply(&mut self, action: Action) -> bool {
        self.reduce(action, true)
    }

    /// Applies an action without recording an undo step (the web app's `disregardUndo`).
    pub fn apply_unrecorded(&mut self, action: Action) -> bool {
        self.reduce(action, false)
    }

    fn reduce(&mut self, action: Action, record: bool) -> bool {
        match action {
            Action::Select(None) => false,
            Action::Select(Some(selection)) => {
                self.selection = Some(selection);
                self.caret = None;
                true
            }
            Action::MoveCaret(caret) => {
                self.caret = caret;
                self.selection = None;
                true
            }
            Action::SetConstruct(construct) => {
                if same_construct(self.construct.as_ref(), &construct) {
                    return false;
                }
                self.write(construct, record);
                true
            }
            Action::Update(patch) => {
                let recorded = patch.construct.is_some_and(|construct| self.write(construct, record));
                // A recorded step builds on the history that was there, whatever the patch said.
                if let Some(history) = patch.history.filter(|_| !recorded) {
                    self.history = history;
                }
                if patch.char_width.is_some() {
                    self.char_width = patch.char_width;
                }
                true
            }
            Action::Undo => self.step(Direction::Back),
            Action::Redo => self.step(Direction::Forward),
        }
    }

    /// Replaces the construct. Returns true when the one it replaced became an undo step.
    fn write(&mut self, construct: Construct, record: bool) -> bool {
        // Nothing to go back to on the first write, and an identical construct is not a step.
        let Some(before) = self.construct.replace(construct) else {
            return false;
        };
        if !record || self.construct.as_ref().is_some_and(|now| same_construct(Some(&before), now)) {
            return false;
        }
        self.history.past.push(HistoryEntry { construct: Some(before), selection: self.selection, caret: self.caret });
        // Oldest first out, so the limit costs the distant past, not the recent one.
        let excess = self.history.past.len().saturating_sub(HISTORY_LIMIT);
        self.history.past.drain(..excess);
        // A new edit forks the timeline: what had been undone can no longer be reached.
        self.history.future.clear();
        true
    }

    /// Steps one way through the history. Changes nothing when there is nowhere to go.
    fn step(&mut self, direction: Direction) -> bool {
        let target = match direction {
            Direction::Back => self.history.past.pop(),
            Direction::Forward => self.history.future.pop(),
        };
        let Some(target) = target else {
            return false;
        };
        let here = HistoryEntry { construct: std::mem::replace(&mut self.construct, target.construct), selection: self.selection, caret: self.caret };
        // Restore whichever of the two was set, never both.
        self.selection = if target.caret.is_some() { None } else { target.selection };
        self.caret = target.caret;
        match direction {
            Direction::Back => self.history.future.push(here),
            Direction::Forward => self.history.past.push(here),
        }
        true
    }
}

/// The editing state of every open construct, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorState {
    slices: BTreeMap<String, EditorSlice>,
}

impl EditorState {
    /// The state of one construct. `None` for a name nothing has been written to.
    pub fn slice(&self, name: &str) -> Option<&EditorSlice> {
        self.slices.get(name)
    }

    /// Applies an action to the named construct, recording an undo step when it changes the
    /// construct. Returns false when nothing changed; an action with no name changes nothing.
    pub fn dispatch(&mut self, name: &str, action: Action) -> bool {
        self.reduce(name, action, true)
    }

    /// The same without recording an undo step (the web app's `disregardUndo`).
    pub fn dispatch_unrecorded(&mut self, name: &str, action: Action) -> bool {
        self.reduce(name, action, false)
    }

    /// `VECTOR_EDITOR_DISPOSE`. Lets go of a construct's state, undo steps and all. Returns false
    /// when there was none.
    pub fn dispose(&mut self, name: &str) -> bool {
        self.slices.remove(name).is_some()
    }

    fn reduce(&mut self, name: &str, action: Action, record: bool) -> bool {
        if name.is_empty() {
            return false;
        }
        if let Some(slice) = self.slices.get_mut(name) {
            return slice.reduce(action, record);
        }
        // An ignored action does not bring a construct's state into being.
        let mut slice = EditorSlice::default();
        let applied = slice.reduce(action, record);
        if applied {
            self.slices.insert(name.to_string(), slice);
        }
        applied
    }
}
