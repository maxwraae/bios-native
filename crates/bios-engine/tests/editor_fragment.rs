//! The web app's `rows/fragment.test.ts` (14 cases) and `rows/clipboard.test.ts` (4 cases), in
//! their order and with their numbers.
//!
//! A `Construct` is not the web app's document, and the cases are written against what a
//! construct holds:
//!
//! - A primer is a `primer_bind` feature in `features`. What the web app keeps on a primer as
//!   `bases`, `tailLength` and `tailLengthInferred` are its `sequence`, `bios_primer_tail_length`
//!   and `bios_primer_tail_inferred` qualifiers, which is how the web app writes them to a file.
//!   A primer with no `sequence` qualifier is the bases it sits on.
//! - A feature's id is its `bios-id` note. The web app's features also have an `id` of their own;
//!   where a case reads that, it reads the `bios-id` here.
//! - A colour is the `color` qualifier, and a partial feature says so in its
//!   `bios_fragment_partial` qualifier. The web app also sets `partial` and
//!   `translationUnavailable` beside the qualifiers; a feature here has nothing beside them.
//!
//! Where the result itself differs from the web app's, the assertion is marked `DIFFERS` with
//! what the web app returns and what is returned here. There are four kinds:
//!
//! - `DIFFERS (no place)`: a construct cannot hold an oligo that binds nowhere. The web app keeps
//!   one for a primer whose footprint is gone; here the primer is left out, and counted.
//! - `DIFFERS (strand)`: every feature of a construct has a strand. The web app has features
//!   with none.
//! - `DIFFERS (fields)`: a feature holds a name, a type, a place, a strand and qualifiers. Other
//!   fields a payload gives it are not kept.
//! - `DIFFERS (carrier)`: the HTML form of a copy, which carries the fragment between browser
//!   tabs, is not ported.

use bios_core::Construct;
use bios_core::model::{Feature, Segment, Strand};
use bios_core::seq::{revcomp, translate};
use bios_core::{genbank, genbank_write};
use bios_engine::editor::edit::{BaseRange, Where, insert_bases, selected_bases};
use bios_engine::editor::fragment::{
    ClipboardError, DnaFragment, FragmentClipboard, FragmentError, extract_fragment, fragment_clipboard_data, fragment_copy_notice,
    fragment_matches_plain, insert_fragment, read_fragment_clipboard, validate_fragment,
};
use serde_json::{Value, json};

const SEQUENCE: &str = "ATGCGTACGTTAGCACCTGAGTCCGATAACGTGCATCGGA";

fn plain(name: &str, kind: &str, start: usize, end: usize) -> Feature {
    Feature {
        name: name.to_string(),
        kind: kind.to_string(),
        start,
        end,
        strand: Strand::Forward,
        segments: vec![Segment::new(start, end)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

fn set(mut f: Feature, key: &str, values: &[&str]) -> Feature {
    let values: Vec<String> = values.iter().map(|v| (*v).to_string()).collect();
    match f.notes.iter_mut().find(|(k, _)| k == key) {
        Some((_, held)) => *held = values,
        None => f.notes.push((key.to_string(), values)),
    }
    f
}

/// The web app's `feature(id, start, end)`: a forward `misc_feature` named after its id.
fn feature(id: &str, start: usize, end: usize) -> Feature {
    set(plain(id, "misc_feature", start, end), "note", &[&format!("bios-id: {id}")])
}

/// The same with qualifiers of its own. The `note` follows them, as the web app writes it.
fn feature_with(id: &str, kind: &str, start: usize, end: usize, notes: &[(&str, &[&str])]) -> Feature {
    let f = notes.iter().fold(plain(id, kind, start, end), |f, (key, values)| set(f, key, values));
    set(f, "note", &[&format!("bios-id: {id}")])
}

fn on(mut f: Feature, strand: Strand) -> Feature {
    f.strand = strand;
    f
}

fn joined(mut f: Feature, pieces: &[(usize, usize)]) -> Feature {
    f.segments = pieces.iter().map(|&(start, end)| Segment::new(start, end)).collect();
    f
}

/// The web app's `primer(id, start, end, forward)`: an oligo that is its footprint.
fn primer(id: &str, start: usize, end: usize, forward: bool) -> Feature {
    on(set(plain(id, "primer_bind", start, end), "note", &[&format!("bios-id: {id}")]), if forward { Strand::Forward } else { Strand::Reverse })
}

/// A primer with an oligo of its own: `bases`, of which the first `tail` do not bind.
fn with_oligo(p: Feature, bases: &str, tail: usize) -> Feature {
    set(set(p, "sequence", &[bases]), "bios_primer_tail_length", &[&tail.to_string()])
}

fn make(primers: Vec<Feature>, features: Vec<Feature>, circular: bool) -> Construct {
    Construct {
        name: "source".to_string(),
        circular,
        sequence: SEQUENCE.to_string(),
        features: features.into_iter().chain(primers).collect(),
        comments: vec!["bios_construct_id: original".to_string(), "private document note".to_string()],
        ..Construct::default()
    }
}

fn linear(sequence: &str) -> Construct {
    Construct { sequence: sequence.to_string(), ..Construct::default() }
}

fn is_primer(f: &Feature) -> bool {
    f.kind == "primer_bind"
}

fn primers_of(c: &Construct) -> Vec<&Feature> {
    c.features.iter().filter(|f| is_primer(f)).collect()
}

fn features_of(c: &Construct) -> Vec<&Feature> {
    c.features.iter().filter(|f| !is_primer(f)).collect()
}

fn note<'a>(f: &'a Feature, key: &str) -> Option<Vec<&'a str>> {
    f.note(key).map(|values| values.iter().map(String::as_str).collect())
}

fn lines(f: &Feature) -> Vec<&str> {
    note(f, "note").unwrap_or_default()
}

fn bios_id(f: &Feature) -> String {
    lines(f).iter().find_map(|line| line.strip_prefix("bios-id:")).map(|id| id.trim().to_string()).unwrap()
}

/// What the feature's first action is done to: a `bios-act`, or the older `bios-for`.
fn targets(f: &Feature) -> Vec<String> {
    let act = lines(f).into_iter().find_map(|line| line.strip_prefix("bios-act:")).map(|body| body.rsplit_once('>').unwrap().1);
    let legacy = lines(f).into_iter().find_map(|line| line.strip_prefix("bios-for:"));
    act.or(legacy).map(|list| list.split(',').map(|target| target.trim().to_string()).collect()).unwrap_or_default()
}

fn spans(f: &Feature) -> Vec<(usize, usize)> {
    f.segments.iter().map(|p| (p.start, p.end)).collect()
}

/// The web test's `featureBases`: the feature's pieces in their order, read along its strand.
fn feature_bases(sequence: &str, f: &Feature) -> String {
    let bases: String = spans(f).iter().map(|&(start, end)| &sequence[start..=end]).collect();
    if f.strand == Strand::Reverse { revcomp(&bases) } else { bases }
}

/// A primer's oligo, 5' to 3': its `sequence` qualifier, or the bases it sits on.
fn oligo(sequence: &str, p: &Feature) -> String {
    if let Some(stored) = p.note("sequence").and_then(|values| values.first()) {
        return stored.clone();
    }
    let footprint = selected_bases(sequence, BaseRange::new(p.start, p.end));
    if p.strand == Strand::Reverse { revcomp(&footprint) } else { footprint }
}

fn tail_length(p: &Feature) -> usize {
    p.note("bios_primer_tail_length").and_then(|values| values.first()).map_or(0, |tail| tail.parse().unwrap())
}

fn codon_start(f: &Feature) -> usize {
    note(f, "codon_start").unwrap()[0].parse().unwrap()
}

/// The fragment as the JSON a copy holds, to be changed and read back.
fn payload(fragment: &DnaFragment) -> Value {
    serde_json::from_str(&fragment.to_json().unwrap()).unwrap()
}

/// The payload with one of its fields replaced.
fn with(payload: &Value, key: &str, value: Value) -> Value {
    let mut out = payload.clone();
    out[key] = value;
    out
}

/// A copy of `record` with `fields` laid over it, as `{ ...record, ...fields }` is.
fn over(record: &Value, fields: Value) -> Value {
    let mut out = record.clone();
    for (key, value) in fields.as_object().unwrap() {
        out[key] = value.clone();
    }
    out
}

/// The web app's `validateFragment` on JSON that may have come from anywhere.
fn validate(payload: &Value) -> Option<DnaFragment> {
    DnaFragment::from_json(&payload.to_string())
}

// ── rows/fragment.test.ts ───────────────────────────────────────────────────────────────────

#[test]
fn extract_carries_selected_features_colors_and_independent_full_oligos_excludes_overlays_unbound_and_document_comments() {
    let full = with_oligo(primer("primer-full", 6, 15, true), &format!("GGG{}", &SEQUENCE[6..16]), 3);
    let partial = primer("primer-partial", 3, 8, true);
    // The web case also holds an oligo that binds nowhere, and a `binder-1` and a `probe` that the
    // window draws in the primer lane. A construct has no place for an oligo without a footprint
    // and no lane of things drawn, so there is nothing here to leave out.
    let f = feature_with("F", "misc_feature", 8, 17, &[("color", &["#123456"]), ("note", &["Useful note"])]);
    let f = set(f, "note", &["Useful note", "bios-id: F"]);
    let data = make(vec![full.clone(), partial], vec![f, feature("outside", 25, 30)], true);
    let before = data.clone();
    let fragment = extract_fragment(&data, BaseRange::new(5, 20), false).unwrap();
    assert_eq!(fragment.sequence, &SEQUENCE[5..21]);
    assert_eq!(fragment.features.len(), 1);
    assert_eq!((fragment.features[0].start, fragment.features[0].end, note(&fragment.features[0], "color")), (3, 12, Some(vec!["#123456"])));
    let copied: Vec<(String, usize, usize, String, usize)> =
        fragment.primers.iter().map(|p| (bios_id(p), p.start, p.end, oligo(&fragment.sequence, p), tail_length(p))).collect();
    assert_eq!(copied, vec![(bios_id(&full), 1, 10, oligo(SEQUENCE, &full), 3)]);
    assert_eq!(fragment.omitted_partial_primer_count, 1);
    assert!(payload(&fragment).get("comments").is_none());
    assert_eq!(data, before, "extraction never mutates source annotations");
}

#[test]
fn circular_selection_carries_an_origin_spanning_primer_contiguously_and_preserves_joined_feature_order() {
    let n = SEQUENCE.len();
    let p = primer("primer-wrap", n - 4, 3, false);
    let joined_feature = joined(feature("joined", n - 6, 5), &[(n - 6, n - 2), (1, 5)]);
    let data = make(vec![p.clone()], vec![joined_feature.clone()], true);
    let fragment = extract_fragment(&data, BaseRange::new(n - 8, 7), false).unwrap();
    assert_eq!(fragment.sequence, format!("{}{}", &SEQUENCE[n - 8..], &SEQUENCE[..8]));
    assert_eq!(spans(&fragment.features[0]), vec![(2, 6), (9, 13)]);
    assert_eq!((fragment.primers[0].start, fragment.primers[0].end), (4, 11));
    assert_eq!(oligo(&fragment.sequence, &fragment.primers[0]), oligo(SEQUENCE, &p));
    let full = extract_fragment(&data, BaseRange::new(0, n - 1), false).unwrap();
    // DIFFERS (no place). Web: `full.primers` holds the one oligo, with start -1, end -1 and
    // `unbound` true ("fully selected oligo survives a split binding footprint"), and its bases
    // are the primer's. Rust: `full.primers` is empty and `split_primer_count` is 1.
    assert_eq!(full.primers.len(), 0);
    assert_eq!(full.split_primer_count, 1);
    assert_eq!(full.omitted_partial_primer_count, 0);
    assert_eq!(spans(&full.features[0]), spans(&joined_feature));
    assert_eq!(full.features[0].start, 1, "linear envelope does not discard the original join order");
}

#[test]
fn full_circular_selection_retains_its_split_primer_as_an_unbound_oligo_through_paste_and_genbank() {
    let n = SEQUENCE.len();
    let footprint = oligo(SEQUENCE, &primer("primer-origin", n - 4, 3, false));
    let p = with_oligo(primer("primer-origin", n - 4, 3, false), &format!("GGG{footprint}"), 3);
    let fragment = extract_fragment(&make(vec![p], vec![], true), BaseRange::new(0, n - 1), true).unwrap();
    // DIFFERS (no place), and it is what this whole case is about. Web: `fragment.primers[0]` is
    // the oligo, `unbound` and now forward; pasted into "AAAA" at 2 the document's primers are
    // that one oligo, still unbound, with its bases (GGG and the footprint) and a new id, and
    // none is drawn; written to GenBank and read back it is there again with its bases, a tail of
    // 3 and unbound. Rust: the fragment holds no primer and counts one split, the paste adds none,
    // and so there is nothing to write to a file.
    assert_eq!(fragment.primers.len(), 0);
    assert_eq!(fragment.split_primer_count, 1);
    let pasted = insert_fragment(&linear("AAAA"), Where::Caret(2), &fragment).unwrap();
    assert_eq!(pasted.sequence, format!("AA{}AA", revcomp(SEQUENCE)));
    assert_eq!(pasted.features, Vec::new());
}

#[test]
fn reverse_complement_fragment_preserves_biological_joined_feature_sequence_and_does_not_reverse_complement_the_ordered_oligo() {
    let footprint = oligo(SEQUENCE, &primer("primer-RC", 7, 17, false));
    let p = set(with_oligo(primer("primer-RC", 7, 17, false), &format!("AAAA{footprint}"), 4), "bios_primer_tail_inferred", &["true"]);
    let joined_feature = joined(feature_with("CDS", "CDS", 4, 25, &[]), &[(4, 9), (20, 25)]);
    for strand in [Strand::Forward, Strand::Reverse] {
        let data = make(vec![p.clone()], vec![on(joined_feature.clone(), strand)], true);
        let plain = extract_fragment(&data, BaseRange::new(2, 28), false).unwrap();
        let reversed = extract_fragment(&data, BaseRange::new(2, 28), true).unwrap();
        assert_eq!(reversed.sequence, revcomp(&plain.sequence));
        assert_eq!(feature_bases(&reversed.sequence, &reversed.features[0]), feature_bases(&plain.sequence, &plain.features[0]));
        assert_eq!(oligo(&reversed.sequence, &reversed.primers[0]), oligo(SEQUENCE, &p));
        assert_eq!(tail_length(&reversed.primers[0]), 4);
        assert_eq!(note(&reversed.primers[0], "bios_primer_tail_inferred"), Some(vec!["true"]));
        assert_eq!(reversed.primers[0].strand, Strand::Forward);
        assert_eq!((reversed.primers[0].start, reversed.primers[0].end), (11, 21));
    }
}

#[test]
fn clipped_forward_reverse_joined_cds_preserves_retained_residues_with_a_recalculated_codon_start() {
    for strand in [Strand::Forward, Strand::Reverse] {
        let forward = strand == Strand::Forward;
        // The web case also sets `frame: 2` and `translation: "WRONG"` beside the qualifiers, and
        // checks that the copy has neither. A feature here has nothing beside its qualifiers.
        let notes: [(&str, &[&str]); 4] =
            [("codon_start", &["2"]), ("translation", &["WRONG"]), ("protein_id", &["complete-protein"]), ("product", &["Protein"])];
        let cds = on(joined(feature_with("coding", "CDS", 2, 30, &notes), &[(2, 9), (20, 30)]), strand);
        let data = make(vec![], vec![cds.clone()], true);
        let fragment = extract_fragment(&data, BaseRange::new(6, 24), false).unwrap();
        let copied = &fragment.features[0];
        assert_eq!(fragment.partial_feature_count, 1);
        assert!(copied.name.contains("partial"));
        assert_eq!(note(copied, "bios_fragment_partial"), Some(vec!["true"]));
        assert_eq!(spans(copied), vec![(0, 3), (14, 18)]);
        assert_eq!(note(copied, "translation"), None);
        assert_eq!(note(copied, "codon_start"), Some(vec![if forward { "1" } else { "2" }]));
        assert_eq!(note(copied, "protein_id"), None);
        assert_eq!(note(copied, "product"), Some(vec!["Protein"]));
        let old_coding = feature_bases(SEQUENCE, &cds);
        let removed_five_prime: usize = if forward { 4 } else { 6 };
        let (retained, old_offset) = (9, 1);
        let expected =
            &translate(&old_coding[old_offset..])[(removed_five_prime - old_offset).div_ceil(3)..(removed_five_prime + retained - old_offset) / 3];
        assert_eq!(translate(&feature_bases(&fragment.sequence, copied)[codon_start(copied) - 1..]), expected);
        let reversed = extract_fragment(&data, BaseRange::new(6, 24), true).unwrap();
        assert_eq!(translate(&feature_bases(&reversed.sequence, &reversed.features[0])[codon_start(copied) - 1..]), expected);
    }
}

#[test]
fn contiguous_cds_phase_is_preserved_on_each_strand_and_unrecoverable_internal_clipping_disables_translation() {
    for strand in [Strand::Forward, Strand::Reverse] {
        let forward = strand == Strand::Forward;
        let cds = on(feature_with("contiguous", "CDS", 2, 30, &[("codon_start", &["2"])]), strand);
        let data = make(vec![], vec![cds.clone()], true);
        let fragment = extract_fragment(&data, BaseRange::new(6, 24), false).unwrap();
        let f = &fragment.features[0];
        let removed_five_prime: usize = if forward { 4 } else { 6 };
        let (retained, old_offset) = (19, 1);
        let expected = &translate(&feature_bases(SEQUENCE, &cds)[old_offset..])
            [(removed_five_prime - old_offset).div_ceil(3)..(removed_five_prime + retained - old_offset) / 3];
        assert_eq!(translate(&feature_bases(&fragment.sequence, f)[codon_start(f) - 1..]), expected);
        let disjoint = extract_fragment(&data, BaseRange::new(20, 8), false).unwrap();
        let uncertain = &disjoint.features[0];
        assert_eq!(note(uncertain, "bios_translation_unavailable"), Some(vec!["true"]));
        assert_eq!(note(uncertain, "codon_start"), None);
    }
}

#[test]
fn undirected_features_stay_undirected_when_copied_reversed_and_pasted() {
    // The web case runs twice: for a feature with no `forward` and no `strand`, and for one with
    // `strand: 0`. Both are a feature with no direction, which a construct cannot hold: a feature
    // read from a file that gives it none is forward. So the case runs once, on a forward one.
    let undirected = feature("undirected", 3, 12);
    for reverse in [true, false] {
        let fragment = extract_fragment(&make(vec![], vec![undirected.clone()], true), BaseRange::new(0, 15), reverse).unwrap();
        // DIFFERS (strand). Web: the copy has no `forward`, and the `strand` it had (none, or 0),
        // reversed or not. Rust: forward, and reverse when the fragment is reverse-complemented.
        assert_eq!(fragment.features[0].strand, if reverse { Strand::Reverse } else { Strand::Forward });
        assert_eq!((fragment.features[0].start, fragment.features[0].end), (3, 12));
        let target = Construct { features: vec![feature("existing", 0, 2)], ..linear("ATGC") };
        let result = insert_fragment(&target, Where::Caret(4), &fragment).unwrap();
        let copied = &result.features[1];
        // DIFFERS (strand). Web: the pasted copy still has no `forward` and the `strand` it had,
        // and the feature already there keeps `strand: 0` and no `forward`. Rust: the pasted copy
        // keeps the strand the fragment gave it, and the feature already there stays forward.
        assert_eq!(copied.strand, fragment.features[0].strand);
        assert_eq!((copied.name.as_str(), copied.start, copied.end), ("undirected", 7, 16));
        assert_eq!(result.features[0].strand, Strand::Forward);
    }
}

#[test]
fn each_paste_creates_fresh_persisted_ids_and_remaps_internal_actions_while_dropping_external_targets() {
    let cargo = feature("cargo", 4, 10);
    let actor = set(feature("actor", 1, 3), "note", &["bios-for: cargo", "bios-id: actor", "bios-act: transcribes > cargo, external-id, construct"]);
    let p = set(primer("primer-copy", 6, 12, true), "note", &["bios-for: cargo", "bios-id: old-primer-feature"]);
    let fragment = extract_fragment(&make(vec![p], vec![actor, cargo], true), BaseRange::new(0, 15), false).unwrap();
    let before = fragment.clone();
    let target = Construct { comments: vec!["target".to_string()], ..linear("TTTT") };
    let first = insert_fragment(&target, Where::Caret(2), &fragment).unwrap();
    let second = insert_fragment(&first, Where::Caret(first.sequence.len()), &fragment).unwrap();
    let fs = features_of(&second);
    let mut ids: Vec<String> = fs.iter().map(|f| bios_id(f)).collect();
    ids.sort();
    ids.dedup();
    assert_eq!((fs.len(), ids.len()), (4, 4));
    // The web case also checks that each feature's own `id` is its `bios-id`. Here they are one.
    assert_eq!(targets(fs[0]), vec![bios_id(fs[1]), "construct".to_string()]);
    assert_eq!(targets(fs[2]), vec![bios_id(fs[3]), "construct".to_string()]);
    let ps = primers_of(&second);
    assert_eq!(ps.len(), 2);
    assert_ne!(bios_id(ps[0]), bios_id(ps[1]));
    assert!(ps.iter().all(|p| bios_id(p) != "old-primer-feature"));
    assert_eq!(targets(ps[0]), vec![bios_id(fs[1])]);
    assert_eq!(targets(ps[1]), vec![bios_id(fs[3])]);
    assert_eq!(second.comments, vec!["target".to_string()]);
    assert_eq!(fragment, before);
}

#[test]
fn insert_replace_shifts_target_annotations_through_the_typing_primitive_and_retains_deleted_target_oligos() {
    let target_primer = primer("primer-target", 6, 15, true);
    let target = make(vec![target_primer.clone()], vec![feature("target", 20, 25)], true);
    let source = make(vec![primer("primer-source", 1, 4, true)], vec![feature("source", 0, 5)], true);
    let fragment = extract_fragment(&source, BaseRange::new(0, 5), false).unwrap();
    for at in [Where::Caret(3), Where::Range(BaseRange::new(5, 16)), Where::Range(BaseRange::new(30, 3))] {
        let before = target.clone();
        let expected = insert_bases(&target, Some(at), &fragment.sequence).unwrap();
        let result = insert_fragment(&target, at, &fragment).unwrap();
        assert_eq!(result.sequence, expected.sequence);
        let expected_feature = features_of(&expected)[0];
        let actual_feature = *features_of(&result).iter().find(|f| bios_id(f) == "target").unwrap();
        assert_eq!((actual_feature.start, actual_feature.end), (expected_feature.start, expected_feature.end));
        let model = primers_of(&result);
        if at == Where::Range(BaseRange::new(5, 16)) {
            // DIFFERS (no place). Web: the target's primer, whose footprint 6..15 the paste
            // replaces, is kept as an oligo with its bases and `unbound` true, so the document has
            // 2 primers. Rust: typing over a primer's footprint removes the primer, as
            // `insert_bases` does, so the construct has 1 primer, the pasted one.
            assert_eq!(model.len(), 1);
            assert_eq!(oligo(&result.sequence, model[0]), oligo(&fragment.sequence, &fragment.primers[0]));
        } else {
            assert_eq!(model.len(), 2);
            assert_eq!(oligo(&result.sequence, model[0]), oligo(SEQUENCE, &target_primer));
            assert_eq!(oligo(&result.sequence, model[1]), oligo(&fragment.sequence, &fragment.primers[0]));
        }
        assert_eq!(target, before);
    }
}

#[test]
fn new_empty_construct_and_whole_document_replacement_preserve_fragment_annotations_through_genbank() {
    let footprint = oligo(SEQUENCE, &primer("primer-tail", 3, 12, false));
    let p = with_oligo(primer("primer-tail", 3, 12, false), &format!("GG{footprint}"), 2);
    let fragment = extract_fragment(&make(vec![p.clone()], vec![feature("feature", 2, 14)], true), BaseRange::new(0, 15), false).unwrap();
    let cases = [
        (linear(""), Where::Caret(0)),
        (make(vec![primer("primer-old", 4, 10, true)], vec![], true), Where::Range(BaseRange::new(0, SEQUENCE.len() - 1))),
    ];
    for (target, at) in cases {
        let result = insert_fragment(&target, at, &fragment).unwrap();
        assert_eq!(result.sequence, fragment.sequence);
        // The web case counts the primers that have a place, which leaves out the target's own
        // primer: the paste replaced its footprint and the web app keeps its oligo without one.
        // Here that primer is gone (DIFFERS (no place)), and the count is the same 1.
        assert_eq!(primers_of(&result).len(), 1);
        let document = Construct { name: "fragment".to_string(), circular: false, ..result.clone() };
        let read = genbank::read(&genbank_write::write(&document).unwrap()).unwrap();
        let restored = primers_of(&read);
        assert_eq!(oligo(&read.sequence, restored[0]), oligo(SEQUENCE, &p));
        assert_eq!(tail_length(restored[0]), 2);
        assert_eq!(restored[0].strand, Strand::Reverse);
    }
}

#[test]
fn validation_rejects_bad_ranges_and_malformed_payloads_before_insert_and_strips_foreign_document_fields() {
    let source = make(vec![primer("primer-test", 4, 10, true)], vec![feature("feature", 3, 12)], true);
    let fragment = extract_fragment(&source, BaseRange::new(0, 15), false).unwrap();
    let sent = payload(&fragment);
    let (feature0, primer0) = (&sent["features"][0], &sent["primers"][0]);
    assert_eq!(extract_fragment(&make(vec![], vec![], false), BaseRange::new(15, 3), false), None);
    // The web case also asks for a range from -1 and one from 0.5. A range here holds whole
    // numbers from 0, so neither can be asked for.
    assert_eq!(extract_fragment(&make(vec![], vec![], true), BaseRange::new(0, SEQUENCE.len()), false), None);
    assert_eq!(validate(&with(&sent, "sequence", json!("AC!T"))), None);
    assert_eq!(validate(&with(&sent, "features", json!([over(feature0, json!({ "id": {} }))]))), None);
    assert_eq!(validate(&with(&sent, "features", json!([over(feature0, json!({ "start": -1 }))]))), None);
    assert_eq!(validate(&with(&sent, "features", json!([over(feature0, json!({ "locations": [{ "start": 1, "end": 99 }] }))]))), None);
    assert_eq!(validate(&with(&sent, "features", json!([feature0, feature0]))), None);
    assert_eq!(validate(&with(&sent, "primers", json!([over(primer0, json!({ "unbound": true }))]))), None);
    // DIFFERS (no place). Web: the fragment is valid and holds the oligo, with no place. Rust: the
    // fragment is valid, the oligo is left out of it, and `split_primer_count` is 1.
    let unbound = validate(&with(&sent, "primers", json!([over(primer0, json!({ "start": -1, "end": -1, "unbound": true }))]))).unwrap();
    assert_eq!((unbound.primers.len(), unbound.split_primer_count), (0, 1));
    assert_eq!(validate(&with(&sent, "primers", json!([over(primer0, json!({ "tailLength": -1 }))]))), None);
    let foreign = validate(&with(&sent, "comments", json!(["foreign"]))).unwrap();
    assert!(payload(&foreign).get("comments").is_none());
    assert_eq!(insert_fragment(&linear(SEQUENCE), Where::Caret(SEQUENCE.len() + 1), &fragment), Err(FragmentError::Position));
    assert_eq!(insert_fragment(&linear(SEQUENCE), Where::Range(BaseRange::new(20, 3)), &fragment), Err(FragmentError::Position));
    let bad = DnaFragment { sequence: "BAD!".to_string(), ..fragment.clone() };
    assert_eq!(insert_fragment(&linear(SEQUENCE), Where::Caret(0), &bad), Err(FragmentError::InvalidFragment));
}

#[test]
fn hostile_json_cannot_smuggle_nonscalar_annotation_fields_through_feature_or_primer_provenance() {
    let source = make(vec![primer("primer-json", 4, 10, true)], vec![feature("feature", 3, 12)], true);
    let fragment = extract_fragment(&source, BaseRange::new(0, 15), false).unwrap();
    let sent = payload(&fragment);
    let (feature0, primer0) = (&sent["features"][0], &sent["primers"][0]);
    let invalid = [
        json!({ "name": { "x": 1 } }),
        json!({ "type": [] }),
        json!({ "id": 42 }),
        json!({ "id": "" }),
        json!({ "forward": "no" }),
        json!({ "strand": "-1" }),
        json!({ "strand": 2 }),
        json!({ "color": { "value": "red" } }),
        json!({ "partial": [] }),
    ];
    for bad in invalid {
        let feature_payload = with(&sent, "features", json!([over(feature0, bad.clone())]));
        assert_eq!(validate(&feature_payload), None, "{bad}");
        // The web case then checks that pasting the payload is refused. A paste here takes a
        // fragment, and this payload is not one, so there is nothing to paste.
        let source_payload = with(&sent, "primers", json!([over(primer0, json!({ "_src": bad }))]));
        assert_eq!(validate(&source_payload), None, "source {bad}");
    }
    assert_eq!(validate(&with(&sent, "primers", json!([over(primer0, json!({ "_src": { "locations": [{ "start": "1", "end": 5 }] } }))]))), None);
    let provenance = json!({ "origin": { "nativeVersion": 7, "sourceCoordinates": [500, 520] }, "notes": { "note": ["Kept provenance"] }, "start": 500, "end": 520 });
    let kept = with(&sent, "features", json!([over(feature0, json!({ "provenance": provenance }))]));
    let kept = with(&kept, "primers", json!([over(primer0, json!({ "_src": provenance }))]));
    let accepted = validate(&kept).unwrap();
    // DIFFERS (fields). Web: `accepted.features[0].provenance` is the whole of `provenance`, and
    // `accepted.primers[0]._src` is too. Rust: the payload is accepted, the feature is as it was
    // without it, and of what the primer came from only the qualifiers are kept, as its own.
    assert_eq!(accepted.features, fragment.features);
    assert!(payload(&accepted)["features"][0].get("provenance").is_none());
    assert_eq!(note(&accepted.primers[0], "note"), Some(vec!["Kept provenance"]));
    assert_eq!(payload(&accepted)["primers"][0]["_src"].get("origin"), None);
}

#[test]
fn clipping_an_already_partial_annotation_does_not_repeat_its_partial_suffix() {
    let first = extract_fragment(&make(vec![], vec![feature_with("CDS", "CDS", 2, 25, &[])], true), BaseRange::new(4, 20), false).unwrap();
    let inserted = insert_fragment(&linear(""), Where::Caret(0), &first).unwrap();
    let second = extract_fragment(&inserted, BaseRange::new(2, 12), false).unwrap();
    assert_eq!(second.features[0].name, "CDS (partial)");
    assert_eq!(note(&second.features[0], "bios_fragment_partial"), Some(vec!["true"]));
}

#[test]
fn equal_length_rich_paste_retains_existing_feature_boundaries_and_adds_the_copied_annotations() {
    let notes: [(&str, &[&str]); 2] = [("codon_start", &["2"]), ("translation", &["stale"])];
    let original = joined(feature_with("Existing", "CDS", 0, 5, &notes), &[(0, 1), (4, 5)]);
    let source = make(vec![], vec![feature("Copied", 0, 5)], true);
    let copied = extract_fragment(&source, BaseRange::new(0, 5), false).unwrap();
    let rich = DnaFragment { sequence: "TTTTTT".to_string(), ..copied.clone() };
    let pasted = insert_fragment(&make(vec![], vec![original.clone()], true), Where::Range(BaseRange::new(0, 5)), &rich).unwrap();
    assert_eq!(pasted.features.len(), 2);
    let retained = pasted.features.iter().find(|f| f.name == "Existing").unwrap();
    assert_eq!((retained.start, retained.end, spans(retained)), (original.start, original.end, spans(&original)));
    assert_eq!(note(retained, "codon_start"), Some(vec!["2"]));
    assert_eq!(note(retained, "translation"), None);
    let added = pasted.features.iter().find(|f| f.name == "Copied").unwrap();
    assert_eq!((added.start, added.end), (0, 5));
    assert_ne!(bios_id(added), bios_id(&copied.features[0]));
}

// ── rows/clipboard.test.ts ──────────────────────────────────────────────────────────────────

fn clipboard_data() -> Construct {
    let promoter = set(plain("α promoter — 测试", "promoter_region", 2, 9), "note", &["Keep <literal> & Unicode"]);
    Construct { features: vec![promoter], ..linear("ACGTCAGTGCCA") }
}

fn clipboard_fragment() -> DnaFragment {
    extract_fragment(&clipboard_data(), BaseRange::new(0, 11), false).unwrap()
}

#[test]
fn copy_carries_plain_dna_full_metadata_and_a_portable_html_payload_without_rendering_annotation_text() {
    let copy = fragment_clipboard_data(&clipboard_fragment()).unwrap();
    assert_eq!(copy.plain, clipboard_data().sequence);
    assert!(!copy.json.is_empty());
    // DIFFERS (carrier). Web: the copy also holds HTML, which shows the bases and none of the
    // annotation text, and from which the fragment is read back whole, name included, when the
    // JSON is gone from the clipboard. Rust: a copy holds the plain text and the JSON.
    assert_eq!(read_fragment_clipboard(&copy.plain, &copy.json), FragmentClipboard { fragment: Some(clipboard_fragment()), invalid: false });
    assert_eq!(read_fragment_clipboard(&copy.plain, &copy.json).fragment.unwrap().features[0].name, "α promoter — 测试");
}

#[test]
fn stale_or_malformed_rich_metadata_is_rejected_instead_of_attaching_it_to_unrelated_plain_dna() {
    let copy = fragment_clipboard_data(&clipboard_fragment()).unwrap();
    assert_eq!(read_fragment_clipboard("TTTT", &copy.json), FragmentClipboard { fragment: None, invalid: true });
    assert_eq!(read_fragment_clipboard("TTTT", "{broken"), FragmentClipboard { fragment: None, invalid: true });
    // DIFFERS (carrier). Web: with the JSON gone, HTML whose marker is not base64, and HTML that
    // holds the marker twice, are each refused as invalid. Rust: there is no HTML to read.
}

#[test]
fn ordinary_dna_fasta_and_unrelated_html_retain_the_existing_plain_text_paste_path() {
    // The web case also puts `<p>text</p>` on the clipboard as HTML, which holds no fragment.
    for text in ["ACGT", ">record\nACGT", "text from a website"] {
        assert_eq!(read_fragment_clipboard(text, ""), FragmentClipboard { fragment: None, invalid: false });
    }
}

#[test]
fn foreign_versions_and_out_of_range_annotations_cannot_become_a_valid_clipboard_fragment() {
    let sent = payload(&clipboard_fragment());
    let out_of_range = over(&sent["features"][0], json!({ "end": 999 }));
    for bad in [with(&sent, "version", json!(99)), with(&sent, "features", json!([out_of_range]))] {
        assert_eq!(read_fragment_clipboard("", &bad.to_string()), FragmentClipboard { fragment: None, invalid: true });
    }
}

// ── Not in the web app's files ──────────────────────────────────────────────────────────────

/// What the web app's `extractFragment` returned for the first case above, as its
/// `JSON.stringify` wrote it.
const WEB_FIRST_CASE: &str = r##"{"kind":"bios-dna-fragment","version":1,"sequence":"TACGTTAGCACCTGAG","features":[{"id":"F","name":"F","type":"misc_feature","start":3,"end":12,"color":"#123456","notes":{"note":["Useful note","bios-id: F"]},"forward":true,"strand":1}],"primers":[{"id":"primer-full","name":"primer-full","start":1,"end":10,"forward":true,"bases":"GGGACGTTAGCAC","tailLength":3,"tailLengthInferred":true,"unbound":false}],"partialFeatureCount":0,"omittedPartialPrimerCount":1}"##;

/// And for the whole circle of the second case, whose primer has no place.
const WEB_WHOLE_CIRCLE: &str = r#"{"kind":"bios-dna-fragment","version":1,"sequence":"ATGCGTACGTTAGCACCTGAGTCCGATAACGTGCATCGGA","features":[{"id":"joined","name":"joined","type":"misc_feature","start":1,"end":38,"notes":{"note":["bios-id: joined"]},"locations":[{"start":34,"end":38},{"start":1,"end":5}],"forward":true,"strand":1}],"primers":[{"id":"primer-wrap","name":"primer-wrap","start":-1,"end":-1,"forward":false,"bases":"GCATTCCG","tailLength":0,"tailLengthInferred":true,"unbound":true}],"partialFeatureCount":0,"omittedPartialPrimerCount":0}"#;

#[test]
fn a_fragment_the_web_app_wrote_is_read() {
    let fragment = DnaFragment::from_json(WEB_FIRST_CASE).unwrap();
    assert_eq!(fragment.sequence, "TACGTTAGCACCTGAG");
    assert_eq!(fragment.omitted_partial_primer_count, 1);
    let f = &fragment.features[0];
    assert_eq!((f.name.as_str(), f.kind.as_str(), f.start, f.end, f.strand), ("F", "misc_feature", 3, 12, Strand::Forward));
    assert_eq!(lines(f), vec!["Useful note", "bios-id: F"]);
    let p = &fragment.primers[0];
    assert_eq!((p.name.as_str(), p.kind.as_str(), p.start, p.end, p.strand), ("primer-full", "primer_bind", 1, 10, Strand::Forward));
    // The oligo is longer than its footprint, so it is written down. Its tail of 3 is what the
    // lengths already say, so that is not.
    assert_eq!(p.notes, vec![("sequence".to_string(), vec!["GGGACGTTAGCAC".to_string()])]);

    let whole = DnaFragment::from_json(WEB_WHOLE_CIRCLE).unwrap();
    assert_eq!(spans(&whole.features[0]), vec![(34, 38), (1, 5)]);
    assert_eq!((whole.primers.len(), whole.split_primer_count), (0, 1));
}

#[test]
fn a_fragment_comes_back_from_its_json_as_it_was() {
    let notes: [(&str, &[&str]); 3] = [("product", &["Protein"]), ("codon_start", &["1"]), ("db_xref", &["a", "b"])];
    let cds = on(joined(feature_with("coding", "CDS", 2, 30, &notes), &[(2, 9), (20, 30)]), Strand::Reverse);
    let tailed = with_oligo(primer("tailed", 21, 28, false), "GGTTATCGGAC", 3);
    let data = make(vec![primer("bare", 10, 18, true), tailed], vec![cds, feature("whole", 12, 16)], true);
    for reverse in [false, true] {
        let fragment = extract_fragment(&data, BaseRange::new(5, 33), reverse).unwrap();
        assert_eq!((fragment.features.len(), fragment.primers.len(), fragment.partial_feature_count), (2, 2, 1));
        assert_eq!(DnaFragment::from_json(&fragment.to_json().unwrap()), Some(fragment.clone()));
        assert_eq!(validate_fragment(&fragment), Some(fragment.clone()));
        // A primer that is the bases it sits on is given no qualifier it did not have.
        let bare = fragment.primers.iter().find(|p| p.name == "bare").unwrap();
        assert_eq!(bare.notes, vec![("note".to_string(), vec!["bios-id: bare".to_string()])]);
    }
}

#[test]
fn a_fragment_that_holds_what_it_may_not_is_refused() {
    let fragment = extract_fragment(&make(vec![primer("p", 4, 10, true)], vec![feature("f", 3, 12)], true), BaseRange::new(0, 15), false).unwrap();
    let mut misplaced = fragment.clone();
    misplaced.primers.push(feature("not a primer", 0, 3));
    assert_eq!(validate_fragment(&misplaced), None);
    let mut primer_among_features = fragment.clone();
    primer_among_features.features.push(primer("q", 0, 3, true));
    assert_eq!(validate_fragment(&primer_among_features), None);
    let mut outside = fragment.clone();
    outside.features[0].end = 16;
    assert_eq!(validate_fragment(&outside), None);
    let mut unreadable = fragment.clone();
    unreadable.features[0].location_unsupported = Some("join(2..5,ACC.1:12..15)".to_string());
    assert_eq!(validate_fragment(&unreadable), None);
    assert_eq!(fragment_clipboard_data(&outside), Err(ClipboardError::Invalid));
    assert_eq!(outside.to_json(), None);
    // A place no sequence could hold is refused, not added up.
    let mut nowhere = fragment.clone();
    nowhere.primers[0].end = usize::MAX;
    assert_eq!(validate_fragment(&nowhere), None);
    assert_eq!(insert_fragment(&linear("ACGT"), Where::Caret(0), &nowhere), Err(FragmentError::InvalidFragment));
}

/// The lines of a feature's note with each id replaced by the name it is given in `ids`.
fn named_lines(f: &Feature, ids: &[(&str, String)]) -> Vec<String> {
    lines(f).into_iter().map(|line| ids.iter().fold(line.to_string(), |line, (name, id)| line.replace(id.as_str(), name))).collect()
}

#[test]
fn the_web_app_s_own_answers_for_the_cases_above() {
    // What the web app's `extractFragment` and `insertFragment` returned for the inputs of the
    // cases above, where its tests check less than it returned.
    let place = |f: &Feature| (f.name.clone(), f.start, f.end);
    let at = |name: &str, start: usize, end: usize| (name.to_string(), start, end);

    // Sixth case: a coding feature kept in two runs that are not one run of its bases.
    let cds = feature_with("contiguous", "CDS", 2, 30, &[("codon_start", &["2"])]);
    let disjoint = extract_fragment(&make(vec![], vec![cds], true), BaseRange::new(20, 8), false).unwrap();
    assert_eq!(disjoint.sequence, "GTCCGATAACGTGCATCGGAATGCGTACG");
    let f = &disjoint.features[0];
    assert_eq!((place(f), spans(f)), (at("contiguous (partial)", 0, 28), vec![(22, 28), (0, 10)]));
    let partial = "Partial feature copied: 18 of 29 annotated bases retained. CDS reading frame and translation require review.";
    let expected: Vec<(String, Vec<String>)> =
        [("note", vec!["bios-id: contiguous", partial]), ("bios_fragment_partial", vec!["true"]), ("bios_translation_unavailable", vec!["true"])]
            .into_iter()
            .map(|(key, values)| (key.to_string(), values.into_iter().map(str::to_string).collect()))
            .collect();
    assert_eq!(f.notes, expected);

    // Eighth case: the ids and what each feature acts on, after one paste.
    let cargo = feature("cargo", 4, 10);
    let actor = set(feature("actor", 1, 3), "note", &["bios-for: cargo", "bios-id: actor", "bios-act: transcribes > cargo, external-id, construct"]);
    let p = set(primer("primer-copy", 6, 12, true), "note", &["bios-for: cargo", "bios-id: old-primer-feature"]);
    let fragment = extract_fragment(&make(vec![p], vec![actor, cargo], true), BaseRange::new(0, 15), false).unwrap();
    let first = insert_fragment(&linear("TTTT"), Where::Caret(2), &fragment).unwrap();
    assert_eq!(first.sequence, "TTATGCGTACGTTAGCACTT");
    assert_eq!(first.features.iter().map(place).collect::<Vec<_>>(), vec![at("actor", 3, 5), at("cargo", 6, 12), at("primer-copy", 8, 14)]);
    let ids = [("ACTOR", bios_id(&first.features[0])), ("CARGO", bios_id(&first.features[1])), ("PRIMER", bios_id(&first.features[2]))];
    assert_eq!(named_lines(&first.features[0], &ids), vec!["bios-for: CARGO", "bios-id: ACTOR", "bios-act: transcribes > CARGO, construct"]);
    assert_eq!(named_lines(&first.features[1], &ids), vec!["bios-id: CARGO"]);
    assert_eq!(named_lines(&first.features[2], &ids), vec!["bios-for: CARGO", "bios-id: PRIMER", "bios-act: serves > CARGO"]);

    // Ninth case: where everything is after a paste at a caret, over a range, and over a range
    // that runs through the origin.
    let target = make(vec![primer("primer-target", 6, 15, true)], vec![feature("target", 20, 25)], true);
    let source = make(vec![primer("primer-source", 1, 4, true)], vec![feature("source", 0, 5)], true);
    let fragment = extract_fragment(&source, BaseRange::new(0, 5), false).unwrap();
    let pasted = |at: Where| {
        let result = insert_fragment(&target, at, &fragment).unwrap();
        (result.sequence.clone(), result.features.iter().map(place).collect::<Vec<_>>())
    };
    assert_eq!(
        pasted(Where::Caret(3)),
        (
            "ATGATGCGTCGTACGTTAGCACCTGAGTCCGATAACGTGCATCGGA".to_string(),
            vec![at("target", 26, 31), at("primer-target", 12, 21), at("source", 3, 8), at("primer-source", 4, 7)]
        )
    );
    // DIFFERS (no place). Web: `primer-target` is also there, as an oligo with no place.
    assert_eq!(
        pasted(Where::Range(BaseRange::new(5, 16))),
        ("ATGCGATGCGTTGAGTCCGATAACGTGCATCGGA".to_string(), vec![at("target", 14, 19), at("source", 5, 10), at("primer-source", 6, 9)])
    );
    assert_eq!(
        pasted(Where::Range(BaseRange::new(30, 3))),
        (
            "ATGCGTGTACGTTAGCACCTGAGTCCGATAAC".to_string(),
            vec![at("target", 22, 27), at("primer-target", 8, 17), at("source", 0, 5), at("primer-source", 1, 4)]
        )
    );

    // Thirteenth case: a partial feature clipped again says so again, and its frame follows.
    let first = extract_fragment(&make(vec![], vec![feature_with("CDS", "CDS", 2, 25, &[])], true), BaseRange::new(4, 20), false).unwrap();
    assert_eq!(note(&first.features[0], "codon_start"), Some(vec!["2"]));
    let inserted = insert_fragment(&linear(""), Where::Caret(0), &first).unwrap();
    let second = extract_fragment(&inserted, BaseRange::new(2, 12), false).unwrap();
    let ids = [("PASTED", bios_id(&inserted.features[0]))];
    assert_eq!(
        named_lines(&second.features[0], &ids),
        vec![
            "Partial feature copied: 17 of 24 annotated bases retained.",
            "bios-id: PASTED",
            "Partial feature copied: 11 of 17 annotated bases retained."
        ]
    );
    assert_eq!(note(&second.features[0], "codon_start"), Some(vec!["3"]));
}

#[test]
fn the_file_s_source_record_is_not_copied() {
    // The web app takes the `source` record out of the document it edits, so its copy never sees
    // one. A construct keeps it among its features.
    let source = plain("source", "source", 0, SEQUENCE.len() - 1);
    let data = make(vec![], vec![source, feature("f", 3, 12)], true);
    let fragment = extract_fragment(&data, BaseRange::new(0, 15), false).unwrap();
    assert_eq!(fragment.features.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["f"]);
    assert_eq!(fragment.partial_feature_count, 0);
}

#[test]
fn the_plain_text_is_the_fragment_s_bases_white_space_and_case_aside() {
    let fragment = clipboard_fragment();
    assert!(fragment_matches_plain(&fragment, "acgt cagt\r\n\tGCCA\n"));
    assert!(!fragment_matches_plain(&fragment, "ACGTCAGTGCC"));
    let copy = fragment_clipboard_data(&fragment).unwrap();
    assert_eq!(read_fragment_clipboard(" acgtcagtgcca ", &copy.json).fragment, Some(fragment.clone()));
    // With no plain text there is nothing for the fragment to disagree with.
    assert_eq!(read_fragment_clipboard("", &copy.json).fragment, Some(fragment));
}

#[test]
fn the_notice_says_what_a_copy_did_not_carry_as_it_was() {
    assert_eq!(fragment_copy_notice(&clipboard_fragment()), None);
    let one = DnaFragment { partial_feature_count: 1, omitted_partial_primer_count: 1, split_primer_count: 1, ..clipboard_fragment() };
    assert_eq!(
        fragment_copy_notice(&one).as_deref(),
        Some(
            "1 feature clipped to the selection and marked partial. 1 partly selected primer omitted; select the full binding region to include it. \
             1 primer left out because the copied fragment cuts through its circular binding region."
        )
    );
    let many = DnaFragment { partial_feature_count: 2, omitted_partial_primer_count: 3, split_primer_count: 2, ..clipboard_fragment() };
    assert_eq!(
        fragment_copy_notice(&many).as_deref(),
        Some(
            "2 features clipped to the selection and marked partial. 3 partly selected primers omitted; select the full binding region to include them. \
             2 primers left out because the copied fragment cuts through their circular binding region."
        )
    );
}
