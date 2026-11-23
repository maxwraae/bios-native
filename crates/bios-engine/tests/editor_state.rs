//! The web app's `editorState.test.ts` (20 cases) and `undoHistory.test.ts` (9 cases), in their
//! order and with their numbers.
//!
//! Where the web app compared two states by reference ("the same object came back"), these check
//! that the action reported no change and that the state is equal to what it was.

use bios_core::Construct;
use bios_core::model::{Feature, Segment, Strand};
use bios_engine::editor::state::{Action, EditorSlice, EditorState, HISTORY_LIMIT, History, Patch, Selection, SelectionEnd};

const NAME: &str = "bios:/demo.gb";

fn seq(sequence: &str) -> Construct {
    Construct { sequence: sequence.to_string(), ..Construct::default() }
}

fn run(mut state: EditorState, actions: impl IntoIterator<Item = Action>) -> EditorState {
    for action in actions {
        state.dispatch(NAME, action);
    }
    state
}

fn edit(sequence: &str) -> Action {
    Action::SetConstruct(seq(sequence))
}

fn select(start: usize, end: usize) -> Action {
    Action::Select(Some(Selection::new(start, end)))
}

fn caret(gap: usize) -> Action {
    Action::MoveCaret(Some(gap))
}

fn slice(state: &EditorState) -> &EditorSlice {
    state.slice(NAME).expect("the construct has been written to")
}

fn bases(state: &EditorState) -> &str {
    slice(state).construct().map_or("", |c| c.sequence.as_str())
}

// ── editorState.test.ts ─────────────────────────────────────────────────────────────────────

#[test]
fn a_construct_nobody_has_opened_reads_as_the_shared_default_not_a_fresh_object() {
    // The web app sends an action of a type nobody handles. There is no such action here, so this
    // sends one that has nothing to do.
    let mut state = EditorState::default();
    assert!(!state.dispatch(NAME, Action::Undo));
    assert!(state.slice("never opened").is_none());
    assert_eq!(EditorSlice::default().caret(), None);
    assert_eq!(EditorSlice::default().selection(), None);
}

#[test]
fn an_action_with_no_editor_name_changes_nothing() {
    let mut state = EditorState::default();
    assert!(!state.dispatch("", caret(5)));
    assert_eq!(state, EditorState::default());
}

#[test]
fn a_selection_clears_the_caret_and_a_caret_clears_the_selection() {
    // Never both set. The map draws its caret marker only when there is no selection, so a state
    // holding both would draw two answers to "where am I".
    let mut s = run(EditorState::default(), [caret(40)]);
    assert_eq!(slice(&s).caret(), Some(40));
    s = run(s, [select(10, 19)]);
    assert_eq!(slice(&s).selection(), Some(Selection::new(10, 19)));
    assert_eq!(slice(&s).caret(), None, "the caret should be gone");
    s = run(s, [caret(7)]);
    assert_eq!(slice(&s).selection(), None, "the selection should be gone");
}

#[test]
fn the_end_a_selection_finished_on_is_kept_so_shift_arrow_knows_which_edge_moves() {
    let mut s = EditorState::default();
    s.dispatch("e", Action::Select(Some(Selection::finished_at(4, 9, SelectionEnd::Start))));
    assert_eq!(s.slice("e").and_then(EditorSlice::selection), Some(Selection { start: 4, end: 9, active_end: SelectionEnd::Start }));
    // The web app writes the end as "sideways" here and reads back a selection with no end
    // stated. An end that is neither cannot be written in Rust; one that is not stated is `End`.
    s.dispatch("e", Action::Select(Some(Selection::new(4, 9))));
    assert_eq!(s.slice("e").and_then(EditorSlice::selection), Some(Selection { start: 4, end: 9, active_end: SelectionEnd::End }));
}

#[test]
fn a_negative_selection_is_ignored_rather_than_stored() {
    // The web app writes `{ start: -1, end: -1 }`, its "nothing selected". Here that is `None`.
    // Collapsing a selection depends on this: it is done by moving the caret.
    let s = run(EditorState::default(), [select(10, 19)]);
    let mut after = s.clone();
    assert!(!after.dispatch(NAME, Action::Select(None)));
    assert_eq!(slice(&after).selection(), Some(Selection::new(10, 19)));
}

#[test]
fn the_first_construct_written_does_not_become_an_undo_step() {
    // There is nothing to go back to. A step here would offer to undo the file opening.
    let s = run(EditorState::default(), [edit("ACGT")]);
    assert_eq!(slice(&s).history().past.len(), 0);
}

#[test]
fn an_edit_stacks_the_construct_it_replaced() {
    let s = run(EditorState::default(), [edit("ACGT"), edit("ACGTA")]);
    assert_eq!(slice(&s).history().past.len(), 1);
    assert_eq!(slice(&s).history().past[0].construct.as_ref().map(|c| c.sequence.as_str()), Some("ACGT"));
}

#[test]
fn an_edit_that_changes_nothing_costs_no_history_and_no_re_render() {
    // By value: a keystroke that produces an identical construct is not a step, and reporting no
    // change is what keeps it from redrawing anything.
    let s = run(EditorState::default(), [edit("ACGT")]);
    let mut again = s.clone();
    assert!(!again.dispatch(NAME, edit("ACGT")), "identical bases should not produce a new state");
    assert_eq!(again, s);
}

#[test]
fn an_edit_marked_disregard_undo_does_not_stack() {
    // How every bulk write works, such as opening a construct, so that none of them show up as
    // something that can be undone.
    let mut s = run(EditorState::default(), [edit("ACGT")]);
    s.dispatch_unrecorded(NAME, edit("ACGTA"));
    assert_eq!(bases(&s), "ACGTA");
    assert_eq!(slice(&s).history().past.len(), 0);
}

#[test]
fn undo_restores_the_construct_and_the_place_you_were_in_it() {
    let mut s = run(EditorState::default(), [edit("ACGT"), caret(2)]);
    s = run(s, [edit("ACGTA")]);
    s = run(s, [Action::Undo]);
    assert_eq!(bases(&s), "ACGT");
    assert_eq!(slice(&s).caret(), Some(2), "the caret comes back where it was");
    assert_eq!(slice(&s).selection(), None, "and its selection stays cleared");
}

#[test]
fn undo_at_the_bottom_of_the_stack_does_nothing_rather_than_throwing() {
    let s = run(EditorState::default(), [edit("ACGT")]);
    let mut after = s.clone();
    assert!(!after.dispatch(NAME, Action::Undo));
    assert_eq!(after, s);
    let mut empty = EditorState::default();
    empty.dispatch(NAME, Action::Undo);
    assert!(empty.slice(NAME).is_none());
}

#[test]
fn redo_at_the_top_does_nothing_either() {
    let s = run(EditorState::default(), [edit("ACGT")]);
    let mut after = s.clone();
    assert!(!after.dispatch(NAME, Action::Redo));
    assert_eq!(after, s);
}

#[test]
fn undo_then_redo_lands_back_where_it_started() {
    let mut s = run(EditorState::default(), [edit("ACGT"), edit("ACGTA")]);
    s = run(s, [Action::Undo, Action::Redo]);
    assert_eq!(bases(&s), "ACGTA");
    assert_eq!(slice(&s).history().future.len(), 0);
}

#[test]
fn a_new_edit_after_an_undo_forks_the_timeline_so_redo_cannot_reach_the_abandoned_one() {
    let mut s = run(EditorState::default(), [edit("A"), edit("AC")]);
    s = run(s, [Action::Undo]);
    assert_eq!(slice(&s).history().future.len(), 1);
    s = run(s, [edit("AG")]);
    assert_eq!(slice(&s).history().future.len(), 0, "the redo branch must be dropped");
}

#[test]
fn the_history_is_capped_and_it_forgets_the_distant_past_rather_than_the_recent() {
    let mut s = EditorState::default();
    for i in 0..HISTORY_LIMIT + 30 {
        s = run(s, [edit(&"A".repeat(i + 1))]);
    }
    let past = &slice(&s).history().past;
    assert_eq!(past.len(), HISTORY_LIMIT);
    assert_eq!(past[past.len() - 1].construct.as_ref().map(Construct::len), Some(HISTORY_LIMIT + 29));
}

#[test]
fn closing_a_tab_frees_its_slice_undo_stack_and_all() {
    let mut s = run(EditorState::default(), [edit("ACGT")]);
    assert!(s.slice(NAME).is_some());
    assert!(s.dispose(NAME));
    assert!(s.slice(NAME).is_none());
}

#[test]
fn unknown_keys_on_a_construct_survive_an_edit() {
    // The web app's construct is an open record and this checks a key nobody knows (`weird: 7`).
    // A `Construct` keeps what Bios does not understand in its header lines and comments.
    let first = Construct {
        sequence: "ACGT".to_string(),
        circular: true,
        header: vec!["WEIRD       7".to_string()],
        comments: vec!["kept".to_string()],
        ..Construct::default()
    };
    let s = run(EditorState::default(), [Action::SetConstruct(first.clone())]);
    assert_eq!(slice(&s).construct().map(|c| c.circular), Some(true));
    assert_eq!(slice(&s).construct(), Some(&first));
}

#[test]
fn a_bulk_update_merges_per_key_and_leaves_the_rest_alone() {
    let mut s = run(EditorState::default(), [edit("ACGT"), caret(3)]);
    s.dispatch_unrecorded(NAME, Action::Update(Patch { char_width: Some(16.0), ..Patch::default() }));
    assert_eq!(slice(&s).char_width(), Some(16.0));
    assert_eq!(slice(&s).caret(), Some(3), "an unrelated key must not disturb the caret");
}

#[test]
fn the_assistant_can_wipe_the_undo_stack_by_writing_an_empty_history() {
    // How a change made from outside makes itself the new starting point.
    let mut s = run(EditorState::default(), [edit("A"), edit("AC")]);
    assert_eq!(slice(&s).history().past.len(), 1);
    s.dispatch_unrecorded(NAME, Action::Update(Patch { construct: Some(seq("AG")), history: Some(History::default()), ..Patch::default() }));
    assert_eq!(slice(&s).history(), &History::default());
    assert_eq!(bases(&s), "AG");
}

#[test]
fn two_constructs_keep_separate_selections_carets_and_histories() {
    const OTHER: &str = "bios:/other.gb";
    let mut s = run(EditorState::default(), [edit("ACGT"), caret(2)]);
    s.dispatch(OTHER, edit("TTTT"));
    s.dispatch(OTHER, caret(9));
    assert_eq!(slice(&s).caret(), Some(2));
    assert_eq!(s.slice(OTHER).and_then(EditorSlice::caret), Some(9));
    assert_eq!(bases(&s), "ACGT");
}

// ── undoHistory.test.ts ─────────────────────────────────────────────────────────────────────
// The web app's first three cases go through its `updateEditor` helper, which sends a bulk update
// that is not recorded unless told otherwise. `open` is that helper.

fn open(state: &mut EditorState, patch: Patch) {
    state.dispatch_unrecorded(NAME, Action::Update(patch));
}

fn construct(sequence: &str) -> Patch {
    Patch { construct: Some(seq(sequence)), ..Patch::default() }
}

#[test]
fn opening_a_construct_through_the_real_update_editor_then_another_tab_switch_re_push_records_no_history() {
    let mut s = EditorState::default();
    open(&mut s, construct("ACGT"));
    open(&mut s, construct("TTTT"));
    assert_eq!(slice(&s).history().past.len(), 0, "a bulk push must not be a step");
    assert_eq!(bases(&s), "TTTT");
}

#[test]
fn update_editor_fixes_size_to_the_bases_and_an_explicit_disregard_undo_false_does_record() {
    // The web app's construct carries a `size` that can disagree with its bases (99 for four of
    // them). A `Construct` has no such field: its length is the length of its bases.
    let mut s = EditorState::default();
    open(&mut s, construct("ACGT"));
    assert_eq!(slice(&s).construct().map(Construct::len), Some(4));
    s.dispatch(NAME, Action::Update(construct("ACGTA")));
    assert_eq!(slice(&s).history().past.len(), 1);
}

#[test]
fn an_accepted_proposals_push_sequence_data_history_wipes_the_persons_own_undo_stack() {
    let mut s = run(EditorState::default(), [edit("A"), edit("AC"), edit("ACG")]);
    assert_eq!(slice(&s).history().past.len(), 2);
    open(&mut s, Patch { history: Some(History::default()), ..construct("ACGTTT") });
    assert_eq!(slice(&s).history().past.len(), 0);
}

#[test]
fn a_snapshot_taken_with_a_selection_restores_the_selection_one_with_neither_restores_neither() {
    let mut s = run(EditorState::default(), [edit("ACGT"), select(1, 2)]);
    s = run(s, [edit("AGT")]);
    s = run(s, [Action::Undo]);
    assert_eq!(slice(&s).selection(), Some(Selection::new(1, 2)));
    assert_eq!(slice(&s).caret(), None);

    let mut t = run(EditorState::default(), [edit("ACGT")]);
    t = run(t, [edit("ACGTA"), caret(5)]);
    t = run(t, [Action::Undo]);
    assert_eq!(slice(&t).caret(), None);
    assert_eq!(slice(&t).selection(), None);
    t = run(t, [Action::Redo]);
    assert_eq!(slice(&t).caret(), Some(5), "redo brings the post-edit caret back");
}

#[test]
fn a_selections_force_update_end_survives_the_round_trip_through_history() {
    let start = Selection::finished_at(1, 2, SelectionEnd::Start);
    let mut s = run(EditorState::default(), [edit("ACGT"), Action::Select(Some(start))]);
    s = run(s, [edit("AGT"), Action::Undo]);
    assert_eq!(slice(&s).selection(), Some(start));
}

#[test]
fn undo_new_edit_then_redo_nothing_to_redo() {
    let mut s = run(EditorState::default(), [edit("A"), edit("AC")]);
    s = run(s, [Action::Undo, edit("AG")]);
    let before = s.clone();
    assert!(!s.dispatch(NAME, Action::Redo));
    assert_eq!(s, before);
    assert_eq!(bases(&s), "AG");
}

#[test]
fn a_bulk_push_update_editor_after_an_undo_keeps_the_redo_branch_alive() {
    // The web app writes its layer switches here (`annotationVisibility`), which this state does
    // not hold. The character width is the other bulk write its comment names.
    let mut s = run(EditorState::default(), [edit("A"), edit("AC"), Action::Undo]);
    s.dispatch_unrecorded(NAME, Action::Update(Patch { char_width: Some(16.0), ..Patch::default() }));
    assert_eq!(slice(&s).history().future.len(), 1);
}

#[test]
fn a_sequence_data_update_that_changes_only_name_circular_or_description_is_dropped_not_merely_unrecorded() {
    // Which is why a rename goes through a bulk update instead. The description is a header line.
    let first = Construct { sequence: "ACGT".to_string(), name: "old".to_string(), circular: true, ..Construct::default() };
    let s = run(EditorState::default(), [Action::SetConstruct(first.clone())]);
    let renamed = Construct { name: "new".to_string(), ..first.clone() };
    let linear = Construct { circular: false, ..first.clone() };
    let described = Construct { header: vec!["DEFINITION  hello".to_string()], ..first.clone() };
    for changed in [renamed, linear, described] {
        let mut after = s.clone();
        assert!(!after.dispatch(NAME, Action::SetConstruct(changed.clone())), "the store took {changed:?}");
        assert_eq!(after, s);
    }
}

fn gfp(name: &str) -> Feature {
    Feature {
        name: name.to_string(),
        kind: "CDS".to_string(),
        start: 1,
        end: 3,
        strand: Strand::Forward,
        segments: vec![Segment::new(1, 3)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

#[test]
#[ignore = "differs from the web app: it compares the feature list by reference, so a feature renamed in place is invisible to its store. \
            Rust holds its own copy and compares by value, so the renamed feature is a change and a step"]
fn a_feature_edited_in_place_same_array_mutated_object_is_invisible_to_the_store() {
    let first = Construct { sequence: "ACGT".to_string(), features: vec![gfp("GFP")], ..Construct::default() };
    let s = run(EditorState::default(), [Action::SetConstruct(first.clone())]);
    let renamed = Construct { features: vec![gfp("eGFP")], ..first };
    let mut after = s.clone();
    assert!(!after.dispatch(NAME, Action::SetConstruct(renamed)));
    assert_eq!(after, s);
}

/// Not a web case: what Rust does with the input of the ignored case above.
#[test]
fn rust_only_a_renamed_feature_is_a_change_and_a_step() {
    let first = Construct { sequence: "ACGT".to_string(), features: vec![gfp("GFP")], ..Construct::default() };
    let mut s = run(EditorState::default(), [Action::SetConstruct(first.clone())]);
    assert!(s.dispatch(NAME, Action::SetConstruct(Construct { features: vec![gfp("eGFP")], ..first })));
    assert_eq!(slice(&s).history().past.len(), 1);
    assert_eq!(slice(&s).construct().map(|c| c.features[0].name.as_str()), Some("eGFP"));
}
