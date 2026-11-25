//! The open plasmid as a person is editing it: a working copy, where they are in it, the steps
//! they can undo, and what they have done since it was last saved.
//!
//! Nothing here draws or touches the project. The rules for what a key does are the web app's,
//! ported in `bios_engine::editor`; this holds one plasmid's state and says, when the window asks,
//! what to save and how to describe it.

use std::time::Instant;

use bios_core::Construct;
use bios_engine::editor::edit::{
    BaseRange, Direction, Extended, MoveKey, Where, caret_after_delete, caret_after_insert, caret_after_move, clean_pasted_dna, collapse_on,
    cursor_of, delete_range, extend_selection, insert_bases, is_base_key, range_to_delete, selected_bases,
};
use bios_engine::editor::fragment::{DnaFragment, extract_fragment, fragment_copy_notice, fragment_matches_plain, insert_fragment};
use bios_engine::editor::guard::accepts_edit;
use bios_engine::editor::state::{Action, EditorSlice, History, Patch, Selection, SelectionEnd};

/// One thing the person did to the bases, as the history will say it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    /// Bases typed one after another: how many, and the base the first went after.
    Typed {
        count: usize,
        after: usize,
    },
    Pasted {
        count: usize,
        after: usize,
    },
    /// Bases pasted with the annotations that were copied with them.
    PastedWith {
        count: usize,
        after: usize,
        annotations: usize,
    },
    /// Bases removed: first and last, as a person counts them, before they were removed.
    Deleted {
        from: usize,
        to: usize,
    },
    Replaced {
        from: usize,
        to: usize,
        count: usize,
    },
    Undid,
    Redid,
}

impl Did {
    fn sentence(&self) -> String {
        let bases = |n: usize| if n == 1 { "1 base".to_string() } else { format!("{n} bases") };
        match self {
            Did::Typed { count, after } => format!("Typed {} after base {after}", bases(*count)),
            Did::Pasted { count, after } => format!("Pasted {} after base {after}", bases(*count)),
            Did::PastedWith { count, after, annotations: 1 } => format!("Pasted {} with 1 annotation after base {after}", bases(*count)),
            Did::PastedWith { count, after, annotations } => format!("Pasted {} with {annotations} annotations after base {after}", bases(*count)),
            Did::Deleted { from, to } if from == to => format!("Deleted base {from}"),
            Did::Deleted { from, to } => format!("Deleted {from}..{to}"),
            Did::Replaced { from, to, count } => format!("Replaced {from}..{to} with {}", bases(*count)),
            Did::Undid => "Undid an edit".to_string(),
            Did::Redid => "Redid an edit".to_string(),
        }
    }
}

/// Where an editor's caret or selection stood in a working copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    selection: Option<Selection>,
    caret: Option<usize>,
}

/// A plasmid being edited by hand.
pub struct Editing {
    slice: EditorSlice,
    /// When the working copy last moved away from what is saved. `None` when they agree.
    dirty: Option<Instant>,
    done: Vec<Did>,
}

impl Editing {
    /// Opening a construct fills the state and records no step: there is nothing to undo yet.
    pub fn open(construct: Construct) -> Editing {
        let mut slice = EditorSlice::default();
        slice.apply_unrecorded(Action::Update(Patch { construct: Some(construct), ..Patch::default() }));
        Editing { slice, dirty: None, done: Vec::new() }
    }

    /// The working copy.
    pub fn construct(&self) -> Option<&Construct> {
        self.slice.construct()
    }

    fn length(&self) -> usize {
        self.construct().map_or(0, Construct::len)
    }

    /// The gap the caret is in.
    pub fn caret(&self) -> Option<usize> {
        self.slice.caret()
    }

    /// The first and last selected base, 0-based.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.slice.selection().map(|s| (s.start, s.end))
    }

    /// The gap the moving edge of the selection, or the caret, is at: what should stay in view.
    pub fn cursor(&self) -> usize {
        cursor_of(self.slice.selection(), self.slice.caret())
    }

    /// When the first unsaved change was made.
    pub fn dirty_since(&self) -> Option<Instant> {
        self.dirty
    }

    pub fn can_undo(&self) -> bool {
        !self.slice.history().past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.slice.history().future.is_empty()
    }

    /// The gap a shift-click or a drag extends from: the end the last gesture did not finish on.
    pub fn anchor(&self) -> Option<usize> {
        match self.slice.selection() {
            Some(s) if s.active_end == SelectionEnd::Start => Some(s.end.saturating_add(1)),
            Some(s) => Some(s.start),
            None => self.slice.caret(),
        }
    }

    /// Two gaps become a selection of the bases between them, and one gap twice is a caret: an
    /// empty highlight is a thing you cannot see but can still paste into.
    pub fn point(&mut self, anchor: usize, gap: usize) {
        let (anchor, gap) = (anchor.min(self.length()), gap.min(self.length()));
        let (low, high) = (anchor.min(gap), anchor.max(gap));
        if low == high {
            self.slice.apply(Action::MoveCaret(Some(low)));
        } else {
            let end = if gap >= anchor { SelectionEnd::End } else { SelectionEnd::Start };
            self.slice.apply(Action::Select(Some(Selection::finished_at(low, high - 1, end))));
        }
    }

    /// Selects these bases as they are given, which may run through the origin.
    pub fn select(&mut self, start: usize, end: usize) {
        if start < self.length() && end < self.length() {
            self.slice.apply(Action::Select(Some(Selection::new(start, end))));
        }
    }

    /// Selects these bases and says which end the gesture finished on, which is the end shift
    /// and an arrow then move.
    pub fn select_from(&mut self, start: usize, end: usize, finished_at_start: bool) {
        if start < self.length() && end < self.length() {
            let end_of = if finished_at_start { SelectionEnd::Start } else { SelectionEnd::End };
            self.slice.apply(Action::Select(Some(Selection::finished_at(start, end, end_of))));
        }
    }

    pub fn select_all(&mut self) {
        if let Some(last) = self.length().checked_sub(1) {
            self.select(0, last);
        }
    }

    pub fn put_caret(&mut self, gap: usize) {
        self.slice.apply(Action::MoveCaret(Some(gap.min(self.length()))));
    }

    /// An arrow, Home or End, with or without shift. `bps_per_row` is the number the rows were
    /// drawn with, so that up and down land under the letter they looked like they would.
    pub fn step(&mut self, key: MoveKey, shift: bool, bps_per_row: usize) {
        let (selection, caret) = (self.slice.selection(), self.slice.caret());
        if selection.is_none() && caret.is_none() {
            return;
        }
        if !shift && let Some(edge) = selection.and_then(|s| collapse_on(s, key)) {
            self.slice.apply(Action::MoveCaret(Some(edge)));
            return;
        }
        let to = caret_after_move(cursor_of(selection, caret), key, self.length(), bps_per_row);
        if !shift {
            self.slice.apply(Action::MoveCaret(Some(to)));
            return;
        }
        match extend_selection(selection, caret, to) {
            Some(Extended::Selection(selection)) => self.slice.apply(Action::Select(Some(selection))),
            Some(Extended::Caret(caret)) => self.slice.apply(Action::MoveCaret(Some(caret))),
            None => false,
        };
    }

    /// The one door an edit goes through: a construct with no bases is refused, anything else
    /// becomes the working copy and a step that can be undone.
    fn commit(&mut self, next: Construct, did: Did) -> bool {
        if !accepts_edit(Some(&next)) || self.construct() == Some(&next) {
            return false;
        }
        self.slice.apply(Action::SetConstruct(next));
        self.dirty.get_or_insert_with(Instant::now);
        self.note(did);
        true
    }

    /// Typing runs together: forty keys are one "typed 40 bases", not forty lines.
    fn note(&mut self, did: Did) {
        if let (Some(Did::Typed { count, after }), Did::Typed { count: 1, after: at }) = (self.done.last_mut(), &did)
            && *after + *count == *at
        {
            *count += 1;
            return;
        }
        self.done.push(did);
    }

    fn target(&self) -> Option<Where> {
        Where::of(self.slice.selection(), self.slice.caret())
    }

    /// Puts `bases` where the caret is, or in place of the selection.
    fn insert(&mut self, bases: &str, pasted: bool) -> Result<bool, String> {
        let (Some(target), Some(current)) = (self.target(), self.construct()) else {
            return Ok(false);
        };
        let next = insert_bases(current, Some(target), bases).map_err(|e| e.to_string())?;
        let count = bases.chars().count();
        let did = match target {
            Where::Caret(after) if pasted => Did::Pasted { count, after },
            Where::Caret(after) => Did::Typed { count, after },
            Where::Range(range) => Did::Replaced { from: range.start + 1, to: range.end + 1, count },
        };
        let changed = self.commit(next, did);
        if changed {
            self.slice.apply(Action::MoveCaret(Some(caret_after_insert(target, count))));
        }
        Ok(changed)
    }

    /// What the keyboard typed. Anything that is not a base letter is ignored.
    pub fn type_text(&mut self, text: &str) -> Result<bool, String> {
        let mut changed = false;
        for letter in text.chars() {
            let key = letter.to_string();
            if is_base_key(&key) {
                changed |= self.insert(&key, false)?;
            }
        }
        Ok(changed)
    }

    /// What was on the clipboard, if it reads as DNA. Prose is refused, with a sentence.
    pub fn paste(&mut self, text: &str) -> Result<bool, String> {
        if text.trim().is_empty() {
            return Ok(false);
        }
        let bases = clean_pasted_dna(text);
        if bases.is_empty() {
            return Err("That does not look like DNA, so nothing was pasted.".to_string());
        }
        self.insert(&bases, true)
    }

    /// Backspace (`Direction::Back`) or Delete (`Direction::Forward`).
    pub fn delete(&mut self, direction: Direction) -> Result<bool, String> {
        let Some(range) = range_to_delete(self.slice.selection(), self.slice.caret(), self.length(), direction) else {
            return Ok(false);
        };
        self.remove(range)
    }

    fn remove(&mut self, range: BaseRange) -> Result<bool, String> {
        let Some(current) = self.construct() else {
            return Ok(false);
        };
        if selected_bases(&current.sequence, range).len() >= current.len() {
            return Err("A plasmid cannot be left with no bases. To get rid of it, delete the plasmid.".to_string());
        }
        let next = delete_range(current, range).map_err(|e| e.to_string())?;
        let changed = self.commit(next, Did::Deleted { from: range.start + 1, to: range.end + 1 });
        if changed {
            self.slice.apply(Action::MoveCaret(Some(caret_after_delete(range))));
        }
        Ok(changed)
    }

    /// The selected bases, for the clipboard.
    pub fn copy(&self) -> Option<String> {
        let selection = self.slice.selection()?;
        Some(selected_bases(&self.construct()?.sequence, selection.into()))
    }

    /// The selection as a fragment: its bases with the features and primers on them, as they are
    /// or reverse-complemented. With it comes a sentence when something could not come along
    /// whole (a feature cut by the selection, a primer only partly selected).
    pub fn copy_fragment(&self, reverse: bool) -> Option<(DnaFragment, Option<String>)> {
        let fragment = extract_fragment(self.construct()?, self.slice.selection()?.into(), reverse)?;
        let notice = fragment_copy_notice(&fragment);
        Some((fragment, notice))
    }

    /// Pastes what is on the clipboard. When it is the bases of the fragment that was copied
    /// here last, the fragment's features and primers come in with them; anything else is
    /// pasted as plain bases, if it reads as DNA.
    pub fn paste_with(&mut self, text: &str, copied: Option<&DnaFragment>) -> Result<bool, String> {
        let Some(fragment) = copied.filter(|fragment| fragment_matches_plain(fragment, text)) else {
            return self.paste(text);
        };
        let (Some(target), Some(current)) = (self.target(), self.construct()) else {
            return Ok(false);
        };
        let next = insert_fragment(current, target, fragment).map_err(|e| e.to_string())?;
        let count = fragment.sequence.chars().count();
        let annotations = fragment.features.len() + fragment.primers.len();
        let did = match target {
            Where::Caret(after) if annotations > 0 => Did::PastedWith { count, after, annotations },
            Where::Caret(after) => Did::Pasted { count, after },
            Where::Range(range) => Did::Replaced { from: range.start + 1, to: range.end + 1, count },
        };
        let changed = self.commit(next, did);
        if changed {
            self.slice.apply(Action::MoveCaret(Some(caret_after_insert(target, count))));
        }
        Ok(changed)
    }

    /// The selected bases, removed: what a cut puts on the clipboard.
    pub fn cut(&mut self) -> Result<Option<String>, String> {
        let (Some(bases), Some(selection)) = (self.copy(), self.slice.selection()) else {
            return Ok(None);
        };
        Ok(self.remove(selection.into())?.then_some(bases))
    }

    pub fn undo(&mut self) -> bool {
        let changed = self.slice.apply(Action::Undo);
        if changed {
            self.dirty.get_or_insert_with(Instant::now);
            self.done.push(Did::Undid);
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        let changed = self.slice.apply(Action::Redo);
        if changed {
            self.dirty.get_or_insert_with(Instant::now);
            self.done.push(Did::Redid);
        }
        changed
    }

    /// One line for the history about everything done since the last save.
    pub fn summary(&self) -> String {
        const SHOWN: usize = 3;
        let mut parts: Vec<String> = self.done.iter().take(SHOWN).map(Did::sentence).collect();
        if let Some(more) = self.done.len().checked_sub(SHOWN).filter(|more| *more > 0) {
            parts.push(format!("and {more} more by hand"));
        }
        if parts.is_empty() { "Edited by hand".to_string() } else { parts.join("; ") }
    }

    /// The working copy is what the project now holds. `saved` is the construct as the project
    /// read it back, which stands in for the working copy without becoming a step.
    pub fn saved(&mut self, saved: Construct) {
        self.adopt(saved);
        self.dirty = None;
        self.done.clear();
    }

    fn adopt(&mut self, construct: Construct) {
        if self.construct() != Some(&construct) {
            let (selection, caret) = (self.slice.selection(), self.slice.caret());
            self.slice.apply_unrecorded(Action::Update(Patch { construct: Some(construct), ..Patch::default() }));
            self.keep_place(selection, caret);
        }
    }

    /// Someone else changed the plasmid (an agent, another window). Their version becomes the
    /// working copy, and the steps behind it are let go: undoing one would quietly take their
    /// change back as well.
    pub fn replaced(&mut self, construct: Construct) {
        let (selection, caret) = (self.slice.selection(), self.slice.caret());
        self.slice.apply_unrecorded(Action::Update(Patch { construct: Some(construct), history: Some(History::default()), ..Patch::default() }));
        self.keep_place(selection, caret);
        self.dirty = None;
        self.done.clear();
    }

    /// Where the caret or selection is, to come back to: one working copy can be shown by two
    /// editors, and each keeps its own place in it.
    pub fn spot(&self) -> Spot {
        Spot { selection: self.slice.selection(), caret: self.slice.caret() }
    }

    /// Goes back to a place taken with [`Editing::spot`], as far as the bases still exist.
    pub fn go_to_spot(&mut self, spot: Spot) {
        if spot.selection.is_none() && spot.caret.is_none() {
            // Nowhere yet: another editor's caret is not this one's.
            self.slice.apply_unrecorded(Action::MoveCaret(None));
            return;
        }
        self.keep_place(spot.selection, spot.caret);
    }

    /// The caret or selection stays where it was when the bases under it still exist.
    fn keep_place(&mut self, selection: Option<Selection>, caret: Option<usize>) {
        let length = self.length();
        match (selection, caret) {
            (Some(s), _) if s.start < length && s.end < length => self.slice.apply_unrecorded(Action::Select(Some(s))),
            (Some(s), _) => self.slice.apply_unrecorded(Action::MoveCaret(Some(s.start.min(length)))),
            (None, Some(caret)) => self.slice.apply_unrecorded(Action::MoveCaret(Some(caret.min(length)))),
            (None, None) => false,
        };
    }
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;
    use bios_engine::editor::edit::{Direction, MoveKey};

    use super::Editing;

    fn editing(sequence: &str) -> Editing {
        Editing::open(Construct { name: "t".into(), sequence: sequence.into(), ..Construct::default() })
    }

    fn bases(e: &Editing) -> &str {
        &e.construct().unwrap().sequence
    }

    #[test]
    fn typing_goes_in_at_the_caret_and_the_caret_follows() {
        let mut e = editing("AAAATTTT");
        assert_eq!(e.type_text("g"), Ok(false), "no caret yet, so nowhere to type");
        e.put_caret(4);
        assert_eq!(e.type_text("gc!x"), Ok(true));
        assert_eq!(bases(&e), "AAAAGCTTTT");
        assert_eq!(e.caret(), Some(6));
        assert!(e.dirty_since().is_some());
        assert_eq!(e.summary(), "Typed 2 bases after base 4");
    }

    #[test]
    fn typing_over_a_selection_replaces_it_and_backspace_takes_one_base() {
        let mut e = editing("AAAATTTT");
        e.point(2, 6);
        assert_eq!(e.selection(), Some((2, 5)));
        e.type_text("c").unwrap();
        assert_eq!((bases(&e), e.caret()), ("AACTT", Some(3)));
        e.delete(Direction::Back).unwrap();
        assert_eq!((bases(&e), e.caret()), ("AATT", Some(2)));
        e.delete(Direction::Forward).unwrap();
        assert_eq!(bases(&e), "AAT");
        assert_eq!(e.summary(), "Replaced 3..6 with 1 base; Deleted base 3; Deleted base 3");
    }

    #[test]
    fn each_edit_is_a_step_back_and_a_step_forward() {
        let mut e = editing("AAAATTTT");
        e.put_caret(0);
        e.type_text("gg").unwrap();
        assert!(e.can_undo() && !e.can_redo());
        assert!(e.undo());
        assert_eq!(bases(&e), "GAAAATTTT");
        assert!(e.undo());
        assert_eq!(bases(&e), "AAAATTTT");
        assert!(!e.undo());
        assert!(e.redo() && e.redo() && !e.redo());
        assert_eq!(bases(&e), "GGAAAATTTT");
    }

    #[test]
    fn the_last_bases_cannot_be_deleted_and_prose_cannot_be_pasted() {
        let mut e = editing("ACGT");
        e.select_all();
        assert_eq!(e.delete(Direction::Back), Err("A plasmid cannot be left with no bases. To get rid of it, delete the plasmid.".to_string()));
        assert_eq!(bases(&e), "ACGT");
        e.put_caret(2);
        assert_eq!(e.paste("a note about the plasmid, nothing more"), Err("That does not look like DNA, so nothing was pasted.".to_string()));
        assert_eq!(e.paste(">gfp\nATG GTG\n"), Ok(true));
        assert_eq!(bases(&e), "ACATGGTGGT");
        assert_eq!(e.summary(), "Pasted 6 bases after base 2");
    }

    #[test]
    fn cut_takes_the_selection_and_copy_leaves_it() {
        let mut e = editing("AAAATTTT");
        e.point(6, 2);
        assert_eq!(e.copy().as_deref(), Some("AATT"));
        assert_eq!(e.cut(), Ok(Some("AATT".to_string())));
        assert_eq!((bases(&e), e.caret(), e.selection()), ("AATT", Some(2), None));
        assert_eq!(e.cut(), Ok(None));
    }

    #[test]
    fn a_copied_stretch_brings_its_features_when_pasted_and_plain_bases_bring_none() {
        use bios_core::{Feature, Segment, Strand};
        let gene = Feature {
            name: "gene".into(),
            kind: "CDS".into(),
            start: 4,
            end: 11,
            strand: Strand::Forward,
            segments: vec![Segment::new(4, 11)],
            notes: Vec::new(),
            start_fuzzy: None,
            end_fuzzy: None,
            location_operator: None,
            location_unsupported: None,
        };
        let mut e = Editing::open(Construct { name: "t".into(), sequence: "AAAACCGGTTAACCCC".into(), features: vec![gene], ..Construct::default() });
        e.point(2, 14);
        let (fragment, notice) = e.copy_fragment(false).unwrap();
        assert_eq!((fragment.sequence.as_str(), fragment.features.len(), notice), ("AACCGGTTAACC", 1, None));
        e.put_caret(16);
        // The clipboard still holds what was copied here: the gene comes with its bases.
        assert_eq!(e.paste_with("AACCGGTTAACC", Some(&fragment)), Ok(true));
        let features = &e.construct().unwrap().features;
        assert_eq!(features.iter().map(|f| (f.name.as_str(), f.start, f.end)).collect::<Vec<_>>(), [("gene", 4, 11), ("gene", 18, 25)]);
        assert_eq!(e.summary(), "Pasted 12 bases with 1 annotation after base 16");
        // Something else was copied since, in another program: it is pasted as bases alone.
        assert_eq!(e.paste_with("ggg", Some(&fragment)), Ok(true));
        assert_eq!(e.construct().unwrap().features.len(), 2);
        // A selection that cuts a feature says so.
        e.point(0, 8);
        let (_, notice) = e.copy_fragment(true).unwrap();
        assert!(notice.is_some());
    }

    #[test]
    fn arrows_move_and_shift_extends_from_where_the_selection_began() {
        let mut e = editing(&"ACGT".repeat(30));
        e.put_caret(10);
        e.step(MoveKey::ArrowRight, false, 50);
        assert_eq!(e.caret(), Some(11));
        e.step(MoveKey::ArrowDown, true, 50);
        assert_eq!((e.selection(), e.anchor(), e.cursor()), (Some((11, 60)), Some(11), 61));
        e.step(MoveKey::ArrowUp, true, 50);
        assert_eq!((e.selection(), e.caret()), (None, Some(11)));
        e.step(MoveKey::ArrowLeft, true, 50);
        assert_eq!((e.selection(), e.anchor()), (Some((10, 10)), Some(11)));
        // A plain arrow on a selection goes to that edge of it without moving further.
        e.step(MoveKey::ArrowRight, false, 50);
        assert_eq!(e.caret(), Some(11));
        e.step(MoveKey::End, false, 50);
        assert_eq!(e.caret(), Some(50));
    }

    #[test]
    fn a_save_keeps_the_steps_and_someone_elses_change_lets_them_go() {
        let mut e = editing("AAAATTTT");
        e.put_caret(4);
        e.type_text("c").unwrap();
        let mine = e.construct().unwrap().clone();
        e.saved(mine);
        assert!(e.dirty_since().is_none() && e.can_undo());
        assert_eq!(e.summary(), "Edited by hand");
        // Undoing after a save is a new unsaved change.
        assert!(e.undo());
        assert_eq!(e.summary(), "Undid an edit");
        assert!(e.dirty_since().is_some());

        e.replaced(Construct { name: "t".into(), sequence: "GG".into(), ..Construct::default() });
        assert!(!e.can_undo() && !e.can_redo() && e.dirty_since().is_none());
        assert_eq!((bases(&e), e.caret()), ("GG", Some(2)));
    }
}
