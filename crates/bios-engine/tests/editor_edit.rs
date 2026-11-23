//! The web app's `rows/edit.test.ts` (18 cases), `rows/editAnnotations.test.ts` (16 cases) and
//! `rows/caretAfterEdit.test.tsx` (10 cases), in their order and with their numbers.
//!
//! One thing differs from the web app throughout, and every assertion it touches is marked
//! `DIFFERS (case)`: the web app keeps the case a base was typed in, and `bios_core::edits` stores
//! what it inserts in upper case. The bases and where they land are the same.

use std::collections::BTreeMap;

use bios_core::Construct;
use bios_core::edits::EditError;
use bios_core::model::{Feature, Segment, Strand};
use bios_engine::editor::edit::{
    BaseRange, Direction, Extended, MoveKey, Where, assert_editable_locations, caret_after_delete, caret_after_insert, caret_after_move,
    clean_pasted_dna, collapse_on, cursor_of, delete_range, extend_selection, insert_bases, is_base_key, range_to_delete, selected_bases,
};
use bios_engine::editor::guard::accepts_edit;
use bios_engine::editor::state::{Action, EditorSlice, Patch, Selection, SelectionEnd};

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

fn caret(gap: usize) -> Option<Where> {
    Some(Where::Caret(gap))
}

fn range(start: usize, end: usize) -> Option<Where> {
    Some(Where::Range(BaseRange::new(start, end)))
}

/// The web app's recorded answer holds a base as it was typed, in lower case. Rust's answer is
/// the same bases with that one in upper case. Used only where the construct's own bases are in
/// upper case, so that the whole answer is.
fn typed_in_upper_case(recorded: &str) -> String {
    recorded.to_ascii_uppercase()
}

// ── rows/edit.test.ts ───────────────────────────────────────────────────────────────────────

#[test]
fn a_letter_that_is_a_base_is_typing_anything_else_is_a_shortcut() {
    for k in ["a", "c", "g", "t", "n", "A", "T", "r", "y", "w"] {
        assert!(is_base_key(k), "{k}");
    }
    for k in ["z", "q", "1", " ", "Enter", "ArrowLeft", "-", "."] {
        assert!(!is_base_key(k), "{k}");
    }
}

#[test]
fn backspace_with_a_selection_removes_the_selection_not_one_base() {
    let selection = Some(Selection::new(10, 19));
    assert_eq!(range_to_delete(selection, None, 100, Direction::Back), Some(BaseRange::new(10, 19)));
    assert_eq!(range_to_delete(selection, Some(20), 100, Direction::Forward), Some(BaseRange::new(10, 19)));
}

#[test]
fn backspace_at_a_caret_takes_the_base_behind_it_delete_takes_the_one_in_front() {
    assert_eq!(range_to_delete(None, Some(40), 100, Direction::Back), Some(BaseRange::new(39, 39)));
    assert_eq!(range_to_delete(None, Some(40), 100, Direction::Forward), Some(BaseRange::new(40, 40)));
}

#[test]
fn backspace_at_the_very_start_and_delete_at_the_very_end_do_nothing() {
    // The naive answer goes round the origin and eats the far end of the plasmid.
    assert_eq!(range_to_delete(None, Some(0), 100, Direction::Back), None);
    assert_eq!(range_to_delete(None, Some(100), 100, Direction::Forward), None);
    assert_eq!(range_to_delete(None, None, 100, Direction::Back), None, "no caret, nothing to delete");
}

#[test]
fn the_arrows_move_by_a_base_sideways_and_by_a_whole_row_up_and_down() {
    assert_eq!(caret_after_move(50, MoveKey::ArrowLeft, 100, 40), 49);
    assert_eq!(caret_after_move(50, MoveKey::ArrowRight, 100, 40), 51);
    assert_eq!(caret_after_move(50, MoveKey::ArrowUp, 100, 40), 10);
    assert_eq!(caret_after_move(50, MoveKey::ArrowDown, 100, 40), 90);
}

#[test]
fn the_caret_stops_at_the_ends_rather_than_wrapping_round_the_plasmid() {
    assert_eq!(caret_after_move(0, MoveKey::ArrowLeft, 100, 40), 0);
    assert_eq!(caret_after_move(100, MoveKey::ArrowRight, 100, 40), 100);
    assert_eq!(caret_after_move(5, MoveKey::ArrowUp, 100, 40), 0);
    assert_eq!(caret_after_move(95, MoveKey::ArrowDown, 100, 40), 100);
}

#[test]
fn home_and_end_go_to_the_ends_of_the_row_you_are_on() {
    assert_eq!(caret_after_move(50, MoveKey::Home, 100, 40), 40);
    assert_eq!(caret_after_move(50, MoveKey::End, 100, 40), 80);
}

#[test]
fn a_key_that_is_not_a_movement_is_left_for_someone_else_to_handle() {
    // The web app's function takes any key's name and answers null for this one. Here a key that
    // is not a movement has no `MoveKey`, so there is nothing to ask the function with.
    assert_eq!(MoveKey::named("PageDown"), None);
    assert_eq!(MoveKey::named("ArrowLeft").map(|key| caret_after_move(50, key, 100, 40)), Some(49));
}

#[test]
fn the_selected_bases_come_back_in_order_even_across_the_origin() {
    let seq = "AAAACCCCGGGGTTTT";
    assert_eq!(selected_bases(seq, BaseRange::new(4, 7)), "CCCC");
    assert_eq!(selected_bases(seq, BaseRange::new(14, 1)), "TTAA", "a selection wrapping the origin");
}

#[test]
fn a_pasted_fasta_record_loses_its_header_and_keeps_its_bases() {
    assert_eq!(clean_pasted_dna(">pUC19 fragment\nacgtACGT\nacgt\n"), "acgtACGTacgt");
}

#[test]
fn pasted_numbers_spaces_and_line_breaks_are_dropped_as_in_every_sequence_editor() {
    assert_eq!(clean_pasted_dna("  1 acgt acgt\n 9 ttttcccc\n"), "acgtacgtttttcccc");
}

#[test]
fn prose_is_refused_rather_than_quietly_turned_into_bases() {
    // Strip everything that is not a base code out of "a note about the plasmid" and what is left
    // still reads as a sequence. Most letters in English are not bases; that is the tell.
    assert_eq!(clean_pasted_dna("Hello, this is a note about the plasmid! 12345"), "");
    assert_eq!(clean_pasted_dna("Digest with EcoRI and run a gel tomorrow"), "");
}

#[test]
fn a_real_sequence_with_a_stray_label_or_two_still_pastes() {
    assert_eq!(clean_pasted_dna("acgtacgtacgtacgtacgtn"), "acgtacgtacgtacgtacgtn");
    assert_eq!(clean_pasted_dna(">seq\nACGTACGTAC GTACGTACGT\n"), "ACGTACGTACGTACGTACGT");
}

// What a key press must not do to the rest of the construct.

const ODD_BASES: &str = "atgcatgcatgcatgcatgcatgcatgcatgc";

fn odd() -> Construct {
    Construct {
        sequence: ODD_BASES.to_string(),
        circular: true,
        name: "p".to_string(),
        features: vec![feature("odd", "promoter_region", 2, 9, Strand::Forward), feature("cds", "CDS", 12, 20, Strand::Reverse)],
        ..Construct::default()
    }
}

fn kinds(data: &Construct) -> Vec<&str> {
    data.features.iter().map(|f| f.kind.as_str()).collect()
}

#[test]
fn a_feature_type_the_library_does_not_know_survives_an_insert_and_a_delete() {
    assert_eq!(kinds(&insert_bases(&odd(), caret(30), "a").unwrap()), ["promoter_region", "CDS"]);
    assert_eq!(kinds(&delete_range(&odd(), BaseRange::new(30, 31)).unwrap()), ["promoter_region", "CDS"]);
}

#[test]
fn an_edit_does_not_invent_top_level_lanes_the_construct_never_had() {
    // The web app compares the keys of its open record. A `Construct` has fixed fields, so this
    // checks that the edit touched only the bases and the features.
    let out = insert_bases(&odd(), caret(30), "a").unwrap();
    assert_eq!(
        Construct { sequence: String::new(), features: Vec::new(), ..out.clone() },
        Construct { sequence: String::new(), features: Vec::new(), ..odd() }
    );
    // DIFFERS (case). Web: the first 30 bases + "a" + the rest. Rust: the same with "A".
    assert_eq!(out.sequence, format!("{}A{}", &ODD_BASES[..30], &ODD_BASES[30..]));
}

// Shift and an arrow grow a selection, one base a press.

fn selection_of(extended: Option<Extended>) -> Selection {
    match extended {
        Some(Extended::Selection(selection)) => selection,
        other => panic!("not a selection: {other:?}"),
    }
}

#[test]
fn shift_arrow_grows_a_selection_past_one_base_and_shrinks_it_back() {
    // Three presses of shift-right from a caret at 10: 10..10, 10..11, 10..12.
    let mut sel = extend_selection(None, Some(10), 11);
    assert_eq!(sel, Some(Extended::Selection(Selection { start: 10, end: 10, active_end: SelectionEnd::End })));
    let mut s = selection_of(sel);
    sel = extend_selection(Some(s), None, cursor_of(Some(s), None) + 1);
    s = selection_of(sel);
    assert_eq!(s, Selection { start: 10, end: 11, active_end: SelectionEnd::End });
    sel = extend_selection(Some(s), None, cursor_of(Some(s), None) + 1);
    s = selection_of(sel);
    assert_eq!(s, Selection { start: 10, end: 12, active_end: SelectionEnd::End });
    // Shift-left shrinks from the same end.
    sel = extend_selection(Some(s), None, cursor_of(Some(s), None) - 1);
    assert_eq!(sel, Some(Extended::Selection(Selection { start: 10, end: 11, active_end: SelectionEnd::End })));
}

#[test]
fn shift_arrow_back_past_the_anchor_collapses_to_a_caret_then_grows_the_other_way() {
    let one = Selection { start: 10, end: 10, active_end: SelectionEnd::End };
    assert_eq!(extend_selection(Some(one), None, cursor_of(Some(one), None) - 1), Some(Extended::Caret(10)));
    assert_eq!(extend_selection(None, Some(10), 9), Some(Extended::Selection(Selection { start: 9, end: 9, active_end: SelectionEnd::Start })));
    let left = Selection { start: 9, end: 9, active_end: SelectionEnd::Start };
    assert_eq!(cursor_of(Some(left), None), 9, "the moving edge is the start");
    assert_eq!(extend_selection(Some(left), None, 8), Some(Extended::Selection(Selection { start: 8, end: 9, active_end: SelectionEnd::Start })));
}

#[test]
fn a_plain_arrow_with_a_selection_lands_at_that_edge_of_it_moving_nothing() {
    let sel = Selection { start: 10, end: 14, active_end: SelectionEnd::Start };
    assert_eq!(collapse_on(sel, MoveKey::ArrowLeft), Some(10));
    assert_eq!(collapse_on(sel, MoveKey::ArrowRight), Some(15), "the gap after the last selected base");
    assert_eq!(collapse_on(sel, MoveKey::ArrowUp), None, "up and down still move from the active end");
}

// ── rows/editAnnotations.test.ts ────────────────────────────────────────────────────────────

const SEQ: &str = "AAAACCCCGGGGTTTTAAAACCCCGGGGTTTTAAAACCCC"; // 40 bp

fn feat(id: &str, start: usize, end: usize) -> Feature {
    feature(id, "CDS", start, end, Strand::Forward)
}

fn base() -> Construct {
    Construct {
        sequence: SEQ.to_string(),
        circular: true,
        name: "p".to_string(),
        features: vec![feat("wrap", 35, 2), feat("mid", 10, 20), feat("tail", 30, 34)],
        ..Construct::default()
    }
}

fn feats(data: &Construct) -> BTreeMap<&str, (usize, usize)> {
    data.features.iter().map(|f| (f.name.as_str(), (f.start, f.end))).collect()
}

fn spans<const N: usize>(pairs: [(&'static str, (usize, usize)); N]) -> BTreeMap<&'static str, (usize, usize)> {
    BTreeMap::from(pairs)
}

fn inserted(at: Option<Where>, bases: &str) -> Construct {
    insert_bases(&base(), at, bases).unwrap()
}

fn deleted(start: usize, end: usize) -> Construct {
    delete_range(&base(), BaseRange::new(start, end)).unwrap()
}

// The ends and the origin.

#[test]
fn typing_with_no_caret_and_no_selection_caret_minus_1_edits_nothing_and_moves_no_feature() {
    // A construct that has just been opened sits in exactly this state.
    let before = base();
    assert_eq!(Where::of(None, None), None);
    let out = insert_bases(&before, None, "a").unwrap();
    assert_eq!(out, before, "the same construct comes back untouched");
    assert_eq!(out.sequence, SEQ);
    assert_eq!(feats(&out), feats(&base()));
}

#[test]
fn replacing_a_selection_that_wraps_the_origin_puts_the_new_bases_at_0_and_the_caret_lands_after_them() {
    // Selection 35..2 on a circular 40-mer: 35..39 and 0..2 go and the new base starts the
    // sequence. `start + n` would be 36 on a construct of 33 bases.
    let out = inserted(range(35, 2), "a");
    // DIFFERS (case). Web: "a" + SEQ[3..35]. Rust: "A" + SEQ[3..35].
    assert_eq!(out.sequence, typed_in_upper_case(&format!("a{}", &SEQ[3..35])));
    let wrapped = Where::Range(BaseRange::new(35, 2));
    assert_eq!(caret_after_insert(wrapped, 1), 1);
    assert_eq!(caret_after_insert(wrapped, 4), 4, "a four-base paste: after the four");
    // A plain selection and a caret land where they always did.
    assert_eq!(caret_after_insert(Where::Range(BaseRange::new(10, 12)), 1), 11);
    assert_eq!(caret_after_insert(Where::Caret(7), 3), 10);
}

#[test]
fn deleting_a_selection_that_wraps_the_origin_leaves_the_caret_at_0_where_the_bases_were() {
    let out = deleted(35, 2);
    assert_eq!(out.sequence, &SEQ[3..35]);
    assert_eq!(caret_after_delete(BaseRange::new(35, 2)), 0);
    assert_eq!(caret_after_delete(BaseRange::new(10, 12)), 10, "a plain range: the gap where it started");
}

#[test]
fn typing_at_the_very_end_and_at_0_of_a_circular_construct() {
    // DIFFERS (case), twice. Web: SEQ + "a" and "a" + SEQ. Rust: SEQ + "A" and "A" + SEQ.
    assert_eq!(inserted(caret(40), "a").sequence, typed_in_upper_case(&format!("{SEQ}a")));
    assert_eq!(inserted(caret(0), "a").sequence, typed_in_upper_case(&format!("a{SEQ}")));
    // Insert at 0 shifts every feature by one, the wrapped one included (both halves).
    assert_eq!(feats(&inserted(caret(0), "a")), spans([("wrap", (36, 3)), ("mid", (11, 21)), ("tail", (31, 35))]));
    // Insert at 40 (after the last base) leaves every index alone; the wrapped feature now spans the new base.
    assert_eq!(feats(&inserted(caret(40), "a")), spans([("wrap", (35, 2)), ("mid", (10, 20)), ("tail", (30, 34))]));
}

#[test]
fn backspace_at_0_and_delete_at_the_last_gap_are_no_ops_delete_at_the_last_base_works() {
    assert_eq!(range_to_delete(None, Some(0), 40, Direction::Back), None);
    assert_eq!(range_to_delete(None, Some(40), 40, Direction::Forward), None);
    let r = range_to_delete(None, Some(39), 40, Direction::Forward);
    assert_eq!(r, Some(BaseRange::new(39, 39)));
    let out = delete_range(&base(), r.unwrap()).unwrap();
    assert_eq!(out.len(), 39);
    assert_eq!(feats(&out), spans([("wrap", (35, 2)), ("mid", (10, 20)), ("tail", (30, 34))]), "wrap keeps 35..2; its head half shrank by one");
}

#[test]
fn selecting_everything_and_typing_replaces_the_construct_and_drops_every_feature() {
    let out = inserted(range(0, 39), "acgt");
    // DIFFERS (case). Web: "acgt". Rust: "ACGT".
    assert_eq!(out.sequence, typed_in_upper_case("acgt"));
    assert_eq!(out.features, []);
}

// Features moving with the bases.

#[test]
fn insert_exactly_at_a_features_start_pushes_it_at_end_plus_1_leaves_it_inside_grows_it() {
    assert_eq!(feats(&inserted(caret(10), "aa"))["mid"], (12, 22), "at start: pushed, not grown");
    assert_eq!(feats(&inserted(caret(21), "aa"))["mid"], (10, 20), "after the last base: untouched");
    assert_eq!(feats(&inserted(caret(20), "aa"))["mid"], (10, 22), "before the last base: grown");
    assert_eq!(feats(&inserted(caret(15), "aa"))["mid"], (10, 22), "inside: grown");
}

#[test]
fn a_feature_whose_whole_footprint_is_deleted_goes_one_partly_deleted_shrinks() {
    let gone = deleted(30, 34);
    assert!(!feats(&gone).contains_key("tail"));
    let shrunk = deleted(12, 15);
    assert_eq!(feats(&shrunk)["mid"], (10, 16));
    let front = deleted(8, 12);
    assert_eq!(feats(&front)["mid"], (8, 15));
}

#[test]
fn a_wrapped_feature_survives_deletions_on_either_side_of_the_origin() {
    assert_eq!(feats(&deleted(0, 1))["wrap"], (33, 0), "its tail half shrinks to one base");
    assert_eq!(feats(&deleted(36, 38))["wrap"], (35, 2), "inside its head half");
    assert!(!feats(&deleted(34, 3)).contains_key("wrap"), "a wrapped range covering it entirely");
}

#[test]
fn features_come_back_as_an_array_and_size_circular_and_name_survive_an_edit() {
    // A `Construct` holds its features as a list and has no `size` apart from its bases.
    let out = inserted(caret(5), "a");
    assert_eq!(out.features.len(), 3);
    assert_eq!(out.len(), 41);
    assert!(out.circular);
    assert_eq!(out.name, "p");
    let lin = insert_bases(&Construct { circular: false, ..base() }, caret(5), "a").unwrap();
    assert!(!lin.circular);
}

#[test]
fn without_keep_types_the_library_coerces_an_unknown_feature_type_through_edit_ts_it_survives() {
    // The web app first calls its third-party edit library directly, to show what it does when
    // left alone: the type becomes `misc_feature`. There is no such library here, so only the
    // second half is a claim about this code.
    let d = Construct {
        sequence: SEQ.to_string(),
        circular: true,
        features: vec![feature("x", "promoter_region", 1, 5, Strand::Forward)],
        ..Construct::default()
    };
    assert_eq!(insert_bases(&d, caret(20), "a").unwrap().features[0].kind, "promoter_region");
}

// Paste.

#[test]
fn a_pasted_genbank_origin_block_gives_only_the_bases_not_the_word_origins_iupac_letters() {
    let block = "ORIGIN\n        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt\n//\n";
    assert_eq!(clean_pasted_dna(block), "acgtacgtacgtacgtacgtacgtacgtacgtacgtacgt");
}

#[test]
fn a_pasted_locus_header_line_is_dropped_even_when_the_record_is_long_enough_to_pass_the_90_percent_rule() {
    let rec = format!("LOCUS       pX 40 bp DNA circular\n{}\n", "acgtacgtac".repeat(12));
    assert_eq!(clean_pasted_dna(&rec), "acgtacgtac".repeat(12));
}

#[test]
fn a_whole_genbank_record_pasted_in_keywords_the_feature_table_and_its_qualifiers_all_go_the_bases_stay() {
    let record = [
        "LOCUS       pX                        40 bp    DNA     circular SYN 01-JAN-2026",
        "DEFINITION  a test construct.",
        "ACCESSION   pX",
        "KEYWORDS    .",
        "SOURCE      synthetic DNA construct",
        "  ORGANISM  synthetic DNA construct",
        "FEATURES             Location/Qualifiers",
        "     CDS             complement(join(1..10,21..30))",
        "                     /label=\"GFP\"",
        "                     /note=\"bios-use: drives the reporter\"",
        "     promoter        5..15",
        "ORIGIN",
        "        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt",
        "//",
        "",
    ]
    .join("\n");
    assert_eq!(clean_pasted_dna(&record), "acgtacgtacgtacgtacgtacgtacgtacgtacgtacgt");
}

#[test]
fn case_is_preserved_as_typed_and_pasted_not_normalised() {
    // This passes as recorded only because the pasted bases are already in upper case: the
    // construct's own lower-case bases are left alone. See the ignored case below.
    let out = insert_bases(&Construct { sequence: "aaaa".to_string(), ..Construct::default() }, caret(2), "GG").unwrap();
    assert_eq!(out.sequence, "aaGGaa");
}

#[test]
fn digits_whitespace_and_a_fasta_header_are_stripped_prose_is_refused() {
    assert_eq!(clean_pasted_dna(">x\n 1 acgt\n 5 acgt"), "acgtacgt");
    assert_eq!(clean_pasted_dna("please insert this"), "");
}

/// Not a web case of its own: the one difference from the web app, stated as the web app has it.
#[test]
#[ignore = "differs from the web app: it keeps the case a base was typed in (insertBases(\"AAAA\", 2, \"gg\") gives \"AAggAA\"). \
            bios_core::edits stores what it inserts in upper case, so Rust gives \"AAGGAA\""]
fn web_only_a_base_typed_in_lower_case_stays_in_lower_case() {
    let out = insert_bases(&Construct { sequence: "AAAA".to_string(), ..Construct::default() }, caret(2), "gg").unwrap();
    assert_eq!(out.sequence, "AAggAA");
}

// ── rows/caretAfterEdit.test.tsx ────────────────────────────────────────────────────────────
// The web app mounts its key handler (`useRowEditing`) against its store and presses keys. The
// handler is not ported, since it is the part that listens; `Rows` makes the same calls in the
// same order for a typed base, a backspace, a delete and a paste, which is all these cases use.

fn construct() -> Construct {
    Construct { sequence: SEQ.to_string(), circular: true, features: vec![feature("wrap", "CDS", 35, 2, Strand::Forward)], ..Construct::default() }
}

struct Rows {
    slice: EditorSlice,
    /// What the workspace holds: the construct as last reported to it.
    held: Construct,
    reported: Vec<Construct>,
}

impl Rows {
    /// Opening a construct: the bulk write that fills the state and records nothing.
    fn mount(construct: Construct) -> Rows {
        let mut slice = EditorSlice::default();
        slice.apply_unrecorded(Action::Update(Patch { construct: Some(construct.clone()), ..Patch::default() }));
        Rows { slice, held: construct, reported: Vec::new() }
    }

    /// The one door an edit goes through: the guard, then the workspace, then the state.
    fn commit(&mut self, next: Construct) -> bool {
        if !accepts_edit(Some(&next)) {
            return false;
        }
        self.reported.push(next.clone());
        self.held = next.clone();
        self.slice.apply(Action::SetConstruct(next));
        true
    }

    fn target(&self) -> Option<Where> {
        Where::of(self.slice.selection(), self.slice.caret())
    }

    fn key(&mut self, key: &str) {
        if key == "Backspace" || key == "Delete" {
            let direction = if key == "Backspace" { Direction::Back } else { Direction::Forward };
            let Some(range) = range_to_delete(self.slice.selection(), self.slice.caret(), self.held.len(), direction) else {
                return;
            };
            let next = delete_range(&self.held, range).unwrap();
            if self.commit(next) {
                self.slice.apply(Action::MoveCaret(Some(caret_after_delete(range))));
            }
        } else if is_base_key(key) {
            let Some(target) = self.target() else {
                return;
            };
            let next = insert_bases(&self.held, Some(target), key).unwrap();
            if self.commit(next) {
                self.slice.apply(Action::MoveCaret(Some(caret_after_insert(target, 1))));
            }
        }
    }

    fn paste(&mut self, text: &str) {
        let bases = clean_pasted_dna(text);
        let Some(target) = self.target().filter(|_| !bases.is_empty()) else {
            return;
        };
        let next = insert_bases(&self.held, Some(target), &bases).unwrap();
        if self.commit(next) {
            self.slice.apply(Action::MoveCaret(Some(caret_after_insert(target, bases.len()))));
        }
    }

    fn select(&mut self, start: usize, end: usize) {
        self.slice.apply(Action::Select(Some(Selection::new(start, end))));
    }

    fn caret(&mut self, gap: usize) {
        self.slice.apply(Action::MoveCaret(Some(gap)));
    }
}

#[test]
fn typing_over_a_selection_that_wraps_the_origin_the_bases_land_at_0_and_the_caret_sits_after_them() {
    let mut rows = Rows::mount(construct());
    rows.select(35, 2);
    rows.key("a");
    // DIFFERS (case). Web: "a" + SEQ[3..35]. Rust: "A" + SEQ[3..35].
    assert_eq!(rows.held.sequence, typed_in_upper_case(&format!("a{}", &SEQ[3..35])), "the replacement went in at the origin");
    assert_eq!(rows.slice.caret(), Some(1));
}

#[test]
fn and_the_next_typed_base_follows_it_there_not_the_end_of_the_construct() {
    let mut rows = Rows::mount(construct());
    rows.select(35, 2);
    rows.key("a");
    rows.key("c");
    // DIFFERS (case). Web: "ac" + SEQ[3..35]. Rust: "AC" + SEQ[3..35].
    assert_eq!(rows.held.sequence, typed_in_upper_case(&format!("ac{}", &SEQ[3..35])));
}

#[test]
fn deleting_a_selection_that_wraps_the_origin_puts_the_caret_at_0_where_the_bases_were() {
    let mut rows = Rows::mount(construct());
    rows.select(35, 2);
    rows.key("Backspace");
    assert_eq!(rows.held.sequence, &SEQ[3..35]);
    assert_eq!(rows.slice.caret(), Some(0));
}

#[test]
fn pasting_over_a_wrapped_selection_the_caret_sits_after_the_pasted_bases() {
    let mut rows = Rows::mount(construct());
    rows.select(35, 2);
    rows.paste("ACGT");
    assert_eq!(rows.held.sequence, format!("ACGT{}", &SEQ[3..35]));
    assert_eq!(rows.slice.caret(), Some(4));
}

#[test]
fn typing_with_no_caret_and_no_selection_edits_nothing() {
    // Reachable: the rows keep the keyboard when another construct is opened, and one that has
    // just been opened has no caret and no selection.
    let mut rows = Rows::mount(construct());
    assert_eq!(rows.slice.caret(), None);
    rows.key("g");
    assert_eq!(rows.reported.len(), 0, "a key press with nowhere to go should not edit; it produced {}", rows.held.sequence);
    assert_eq!(rows.held.sequence, SEQ);
    let f = &rows.held.features[0];
    assert_eq!((f.start, f.end), (35, 2), "and the wrapped feature did not move");
}

#[test]
fn typing_at_the_end_of_a_circular_construct_appends_and_the_caret_follows() {
    let mut rows = Rows::mount(construct());
    rows.caret(40);
    rows.key("t");
    // DIFFERS (case). Web: SEQ + "t". Rust: SEQ + "T".
    assert_eq!(rows.held.sequence, typed_in_upper_case(&format!("{SEQ}t")));
    assert_eq!(rows.slice.caret(), Some(41));
}

#[test]
fn typing_at_0_prepends_the_caret_follows_the_wrapped_feature_shifts_both_halves() {
    let mut rows = Rows::mount(construct());
    rows.caret(0);
    rows.key("t");
    // DIFFERS (case). Web: "t" + SEQ. Rust: "T" + SEQ.
    assert_eq!(rows.held.sequence, typed_in_upper_case(&format!("t{SEQ}")));
    assert_eq!(rows.slice.caret(), Some(1));
    let f = &rows.held.features[0];
    assert_eq!((f.start, f.end), (36, 3));
}

#[test]
fn backspace_at_0_and_delete_at_the_end_do_nothing() {
    let mut rows = Rows::mount(construct());
    rows.caret(0);
    rows.key("Backspace");
    assert_eq!(rows.reported.len(), 0);
    rows.caret(40);
    rows.key("Delete");
    assert_eq!(rows.reported.len(), 0);
}

#[test]
fn select_all_then_backspace_is_refused_by_the_edit_guard_nothing_changes_no_history() {
    let mut rows = Rows::mount(construct());
    rows.select(0, 39);
    rows.key("Backspace");
    assert_eq!(rows.reported.len(), 0);
    assert_eq!(rows.held.sequence, SEQ);
}

#[test]
fn undo_after_a_keystroke_restores_the_caret_to_where_it_was_before_the_keystroke() {
    let mut rows = Rows::mount(construct());
    rows.caret(5);
    rows.key("a");
    assert_eq!(rows.slice.caret(), Some(6));
    rows.slice.apply(Action::Undo);
    assert_eq!(rows.slice.caret(), Some(5));
    rows.slice.apply(Action::Redo);
    assert_eq!(rows.slice.caret(), Some(6));
}

// ── Not web cases ───────────────────────────────────────────────────────────────────────────

/// The web app throws from `assertEditableLocations`; here the edit is an error with its words.
#[test]
fn rust_only_a_feature_with_a_location_bios_cannot_read_stops_the_edit() {
    let mut data = base();
    data.features[1].location_unsupported = Some("join(2..5,ACC.1:12..15)".to_string());
    let refused = EditError::UnsupportedLocation { name: "mid".to_string(), location: "join(2..5,ACC.1:12..15)".to_string() };
    assert_eq!(assert_editable_locations(&data), Err(refused.clone()));
    assert_eq!(insert_bases(&data, caret(0), "a"), Err(refused.clone()));
    assert_eq!(delete_range(&data, BaseRange::new(0, 0)), Err(refused.clone()));
    assert_eq!(refused.to_string(), "Cannot edit DNA with the unsupported location of mid: join(2..5,ACC.1:12..15)");
    assert_eq!(insert_bases(&data, None, "a"), Ok(data), "nowhere is still nowhere");
}

// The three below were found by running the web app's own `insertBases` and `deleteRange` on
// 23,537 edits (every simple span and every edit on a seven-base molecule, as its
// `circularParity.test.ts` has them, and joined features on a twelve-base one) and comparing. On
// simple spans the bases and the coordinates agree everywhere else.

/// The web app edits a linear construct over a range that runs backwards: deleting 4..1 of
/// "ACGTGCA" leaves "GT". Here that is refused.
#[test]
fn rust_only_a_range_that_runs_backwards_on_a_linear_construct_is_refused() {
    let linear = Construct { sequence: "ACGTGCA".to_string(), ..Construct::default() };
    assert_eq!(delete_range(&linear, BaseRange::new(4, 1)), Err(EditError::Range));
    assert_eq!(insert_bases(&linear, range(4, 1), "TTA"), Err(EditError::Range));
    assert_eq!(EditError::Range.to_string(), "DNA edit coordinates must be inside the sequence; wrapping ranges require circular DNA.");
}

/// In the web app a joined feature that loses every base to a deletion through the origin stays,
/// at 0..0, with its old pieces (1..2 and 6..8) pointing past the one base that is left. Here it
/// goes with its bases.
#[test]
fn rust_only_a_joined_feature_wholly_deleted_through_the_origin_goes() {
    let joined = Feature { segments: vec![Segment::new(1, 2), Segment::new(6, 8)], ..feature("two", "misc_feature", 1, 8, Strand::Forward) };
    let data = Construct { sequence: "ACGTGCATTGCA".to_string(), circular: true, features: vec![joined], ..Construct::default() };
    let out = delete_range(&data, BaseRange::new(4, 2)).unwrap();
    assert_eq!(out.sequence, "T");
    assert_eq!(out.features, []);
}

/// A join of exactly the two arms of a span through the origin (9..11 then 0..2) stays two pieces
/// in the web app: after three bases go in at 0 it is 12..14 then 3..5. Here it comes back as the
/// one span it is, 12..5.
#[test]
fn rust_only_a_join_of_the_two_arms_of_a_span_through_the_origin_comes_back_as_one_span() {
    let arms = Feature { segments: vec![Segment::new(9, 11), Segment::new(0, 2)], ..feature("twoarm", "misc_feature", 9, 2, Strand::Forward) };
    let data = Construct { sequence: "ACGTGCATTGCA".to_string(), circular: true, features: vec![arms], ..Construct::default() };
    let out = insert_bases(&data, caret(0), "TTA").unwrap();
    assert_eq!(out.sequence, "TTAACGTGCATTGCA");
    assert_eq!((out.features[0].start, out.features[0].end), (12, 5));
    assert_eq!(out.features[0].segments, [Segment::new(12, 5)]);
}
