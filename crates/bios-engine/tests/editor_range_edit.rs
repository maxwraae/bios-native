//! The web app's `rangeEdit.test.ts` (9 cases), in their order and with their numbers.
//!
//! The web app keeps primers in a list of their own. Here a primer is a `primer_bind` feature, so
//! the one primer of its fixture follows the one feature in the same list.

use bios_core::Construct;
use bios_core::model::{Feature, Segment, Strand};
use bios_engine::editor::edit::{BaseRange, Where};
use bios_engine::editor::range_edit::{
    AnnotationChange, AnnotationKind, Change, RangeAction, RangeError, RangePreview, RangeRequest, preview_range_edit,
};
use bios_engine::editor::state::{Action, EditorSlice, Selection};

fn feature(name: &str, kind: &str, start: usize, end: usize, strand: Strand) -> Feature {
    Feature {
        name: name.to_string(),
        kind: kind.to_string(),
        start,
        end,
        strand,
        segments: vec![Segment::new(start, end)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

const BASES: &str = "AAAACCCCGGGGTTTT";

fn primer() -> Feature {
    feature("existing primer", "primer_bind", 8, 11, Strand::Reverse)
}

fn source() -> Construct {
    Construct {
        sequence: BASES.to_string(),
        circular: true,
        features: vec![feature("annotated region", "misc_feature", 4, 7, Strand::Forward), primer()],
        ..Construct::default()
    }
}

/// The web app's default request: select base 1.
const SELECT_ONE: RangeRequest<'static> = RangeRequest { action: RangeAction::Select, from: "1", to: "1", bases: "" };

fn preview_of(data: &Construct, request: RangeRequest<'_>) -> RangePreview {
    preview_range_edit(data, &request).unwrap_or_else(|error| panic!("{error}"))
}

fn preview(request: RangeRequest<'_>) -> RangePreview {
    preview_of(&source(), request)
}

fn error(data: &Construct, request: RangeRequest<'_>) -> Option<RangeError> {
    preview_range_edit(data, &request).err()
}

fn change(kind: AnnotationKind, name: &str, change: Change) -> AnnotationChange {
    AnnotationChange { name: name.to_string(), kind, change }
}

#[test]
fn numeric_ranges_are_inclusive_a_circular_range_crosses_the_origin() {
    assert_eq!(preview(RangeRequest { from: "5", to: "8", ..SELECT_ONE }).at, Where::Range(BaseRange::new(4, 7)));
    assert_eq!(preview(RangeRequest { from: "15", to: "2", ..SELECT_ONE }).removed, 4);
    let linear = Construct { circular: false, ..source() };
    let refused = error(&linear, RangeRequest { from: "15", to: "2", ..SELECT_ONE }).expect("a linear sequence has no origin to cross");
    assert!(refused.to_string().contains("linear"), "{refused}");
}

#[test]
fn invalid_coordinates_cannot_select_or_modify_dna() {
    for from in ["0", "-1", "17", "1.5", "1e1", "", "1,2", "99999999999999999999"] {
        assert!(error(&source(), RangeRequest { action: RangeAction::Delete, from, ..SELECT_ONE }).is_some(), "{from}");
    }
    assert!(error(&source(), RangeRequest { to: "17", ..SELECT_ONE }).is_some());
}

#[test]
fn replacement_preview_preserves_the_input_and_reports_changed_features_and_shifted_primers() {
    let data = source();
    let p = preview_of(&data, RangeRequest { action: RangeAction::Replace, from: "6", to: "7", bases: "NNNN" });
    assert_eq!(p.next.sequence, "AAAACNNNNCGGGGTTTT");
    assert_eq!(
        p.changes,
        [
            change(AnnotationKind::Feature, "annotated region", Change::BasesChanged),
            change(AnnotationKind::Primer, "existing primer", Change::PositionChanged),
        ]
    );
    assert_eq!(data, source());
}

#[test]
fn a_deletion_names_the_removed_annotation_before_it_is_applied() {
    let p = preview(RangeRequest { action: RangeAction::Delete, from: "5", to: "8", ..SELECT_ONE });
    assert_eq!(p.next.sequence, "AAAAGGGGTTTT");
    assert!(p.changes.iter().any(|c| c.name == "annotated region" && c.change == Change::Removed));
}

#[test]
fn repeated_feature_names_are_distinguished_by_their_persisted_bios_ids() {
    // The web app finds a feature again by its Bios id, and without one by its name. Here each
    // is found by its place in the list, so the ids are carried but not what tells the two apart.
    let repeat = |start: usize, end: usize, id: &str| Feature {
        notes: vec![("note".to_string(), vec![format!("bios-id: {id}")])],
        ..feature("repeat", "misc_feature", start, end, Strand::Forward)
    };
    let data = Construct { features: vec![repeat(4, 7, "first"), repeat(8, 11, "second"), primer()], ..source() };
    let result = preview_of(&data, RangeRequest { action: RangeAction::Insert, from: "0", bases: "NN", ..SELECT_ONE });
    assert_eq!(result.changes.len(), 3);
    assert!(result.changes.iter().all(|change| change.change == Change::PositionChanged));
}

#[test]
fn edits_between_joined_feature_segments_do_not_claim_the_features_bases_changed() {
    let joined = Feature { segments: vec![Segment::new(4, 5), Segment::new(10, 11)], ..feature("joined", "CDS", 4, 11, Strand::Reverse) };
    let data = Construct { features: vec![joined], ..source() };
    let result = preview_of(&data, RangeRequest { action: RangeAction::Replace, from: "8", to: "9", bases: "NN" });
    assert_eq!(result.changes, []);
    let inserted = preview_of(&data, RangeRequest { action: RangeAction::Insert, from: "8", bases: "NN", ..SELECT_ONE });
    assert_eq!(inserted.changes, [change(AnnotationKind::Feature, "joined", Change::PositionChanged)]);
}

#[test]
fn insert_before_the_first_and_after_the_last_base_replacement_across_the_origin() {
    assert_eq!(preview(RangeRequest { action: RangeAction::Insert, from: "0", bases: "RY", ..SELECT_ONE }).next.sequence, format!("RY{BASES}"));
    assert_eq!(preview(RangeRequest { action: RangeAction::Insert, from: "16", bases: "RY", ..SELECT_ONE }).next.sequence, format!("{BASES}RY"));
    assert_eq!(preview(RangeRequest { action: RangeAction::Replace, from: "15", to: "2", bases: "GC" }).next.sequence, "GCAACCCCGGGGTT");
}

#[test]
fn dna_input_accepts_one_fasta_record_and_refuses_invalid_text_and_an_empty_construct() {
    assert_eq!(preview(RangeRequest { action: RangeAction::Replace, bases: ">fragment\nacgt nryk\n", ..SELECT_ONE }).bases, "ACGTNRYK");
    for bases in ["ACTZ", "Hello", ">one\nACGT\n>two\nTGCA", ""] {
        assert!(error(&source(), RangeRequest { action: RangeAction::Replace, bases, ..SELECT_ONE }).is_some(), "{bases}");
    }
    let emptied = error(&source(), RangeRequest { action: RangeAction::Delete, from: "1", to: "16", ..SELECT_ONE }).expect("nothing would be left");
    assert!(emptied.to_string().contains("at least one base"), "{emptied}");
}

#[test]
fn the_previewed_replacement_is_one_undoable_edit_including_annotations() {
    let mut state = EditorSlice::default();
    state.apply(Action::SetConstruct(source()));
    state.apply(Action::Select(Some(Selection::new(4, 7))));
    let p = preview(RangeRequest { action: RangeAction::Replace, from: "5", to: "8", bases: "RY" });
    state.apply(Action::SetConstruct(p.next.clone()));
    assert_eq!(state.history().past.len(), 1);
    state.apply(Action::Undo);
    assert_eq!(state.construct(), Some(&source()));
    state.apply(Action::Redo);
    assert_eq!(state.construct(), Some(&p.next));
}

// ── Not web cases ───────────────────────────────────────────────────────────────────────────

/// Each refusal reads as the web app words it, with its thousands separated as it prints them.
#[test]
fn rust_only_every_refusal_reads_as_the_web_app_words_it() {
    let long = Construct { sequence: "A".repeat(47_201), ..Construct::default() };
    let says = |data: &Construct, request: RangeRequest<'_>| error(data, request).map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(says(&Construct::default(), SELECT_ONE), "Open a DNA sequence first.");
    assert_eq!(
        says(&long, RangeRequest { action: RangeAction::Insert, from: "47,202", bases: "A", ..SELECT_ONE }),
        "Enter a position from 0 to 47,201; 0 is before the first base."
    );
    assert_eq!(says(&long, RangeRequest { from: "0", ..SELECT_ONE }), "Enter whole-number coordinates from 1 to 47,201.");
    assert_eq!(says(&long, RangeRequest { from: "9", to: "2", ..SELECT_ONE }), "The end must follow the start on a linear sequence.");
    assert_eq!(
        says(&long, RangeRequest { action: RangeAction::Replace, bases: "ACTZ", ..SELECT_ONE }),
        "Use DNA bases or IUPAC ambiguity codes; other text is not accepted."
    );
    assert_eq!(
        says(&long, RangeRequest { action: RangeAction::Replace, bases: " 12 ", ..SELECT_ONE }),
        "Enter the bases to insert. Choose Delete to remove bases."
    );
    assert_eq!(
        says(&source(), RangeRequest { action: RangeAction::Delete, from: "1", to: "16", ..SELECT_ONE }),
        "Keep at least one base in the construct."
    );
    // A coordinate may be typed with its thousands separated.
    assert_eq!(preview_of(&long, RangeRequest { from: "47,201", to: "47,201", ..SELECT_ONE }).at, Where::Range(BaseRange::new(47_200, 47_200)));
}
