//! Origin-correctness tests for feature-preserving edits, ported from the web app's
//! `edits.test.ts` in its order and with its numbers.
//!
//! There is no oracle for these. The expectations are hand-verified rather than differential,
//! which is why they are written out base by base.
//!
//! These cases exist because an earlier version dropped circular support: `end < start` was reused
//! as the "feature collapsed" signal, although it already meant "spans the origin". Both bugs
//! below were silent: wrong output, no error.

mod support;

use bios_core::edits::{EditError, apply_delete, apply_insert, apply_replace};
use bios_core::model::{Feature, Strand};
use support::{feature, named, notes, segments, strand, with_locations};

const LEN: usize = 5000;

/// Positional sequence: the base at index i is derivable from i, so content can be asserted on.
fn plasmid() -> String {
    "ACGT".repeat(LEN / 4)
}

/// Spans the origin: `end < start`.
fn ampr() -> Feature {
    feature("AmpR", "CDS", 4900, 100, Strand::Forward)
}

/// Ordinary, well away from the origin.
fn gfp() -> Feature {
    feature("GFP", "CDS", 2000, 2500, Strand::Forward)
}

// applyDelete — linear (regression: must keep working)

#[test]
fn removes_the_range_and_shifts_downstream_features() {
    let plasmid = plasmid();
    let r = apply_delete(&plasmid, &[gfp()], 1500, 1599, false).unwrap();
    assert_eq!(r.sequence.len(), 4900);
    assert_eq!(r.sequence, format!("{}{}", &plasmid[..1499], &plasmid[1599..]));
    // GFP is downstream of a 100bp cut -> shifts left by 100.
    assert_eq!(named(&r.features, "GFP").map(|f| f.start), Some(1900));
    assert_eq!(named(&r.features, "GFP").map(|f| f.end), Some(2400));
}

#[test]
fn drops_a_feature_wholly_consumed_by_the_deletion() {
    let r = apply_delete(&plasmid(), &[gfp()], 2000, 2600, false).unwrap();
    assert_eq!(named(&r.features, "GFP"), None);
    assert_eq!(r.delta.removed.len(), 1);
    assert_eq!(r.delta.removed[0].name, "GFP");
}

// applyDelete — circular: origin-spanning features (bug #1)

#[test]
fn an_unrelated_edit_must_not_drop_an_origin_spanning_feature() {
    // 100bp cut at 1500 — nowhere near the origin, nowhere near either feature.
    let r = apply_delete(&plasmid(), &[ampr(), gfp()], 1500, 1599, true).unwrap();

    let ampr = named(&r.features, "AmpR").expect("AmpR must survive an unrelated edit 3400bp away");
    // AmpR's head (4900..4999) is downstream of the cut -> shifts left by 100.
    // Its tail (0..100) is upstream -> unchanged. It must still span the origin.
    assert_eq!(ampr.start, 4800);
    assert_eq!(ampr.end, 100);
    assert!(ampr.end < ampr.start, "AmpR must still span the origin");
    assert_eq!(r.delta.removed.len(), 0, "nothing should be removed");
}

// applyDelete — circular: wrapping range (bug #2)

#[test]
fn from_greater_than_to_wraps_the_origin_and_removes_both_arms() {
    // 4950..5000 (51 bases) + 1..50 (50 bases) = 101 removed. 5000 - 101 = 4899.
    let plasmid = plasmid();
    let r = apply_delete(&plasmid, &[ampr(), gfp()], 4950, 50, true).unwrap();
    assert_eq!(r.sequence.len(), 4899, "must not gain bases");
    assert_eq!(r.sequence, &plasmid[50..4949], "keeps exactly the un-deleted arc");
}

#[test]
fn features_are_remapped_across_a_wrapping_delete() {
    let r = apply_delete(&plasmid(), &[ampr(), gfp()], 4950, 50, true).unwrap();
    // GFP is untouched but everything shifts left by the 50 bases cut at the origin.
    assert_eq!(named(&r.features, "GFP").map(|f| f.start), Some(1950));
    assert_eq!(named(&r.features, "GFP").map(|f| f.end), Some(2450));
    // AmpR overlaps BOTH arms of the cut: it loses 51 bases off its head and 50 off its
    // tail (201 -> 100), but survives and still spans the origin.
    let ampr = named(&r.features, "AmpR").expect("AmpR is trimmed by the cut, not destroyed");
    assert_eq!(ampr.start, 4850);
    assert_eq!(ampr.end, 50);
}

// applyInsert — origin

#[test]
fn at_0_means_the_origin_on_a_circular_construct() {
    let plasmid = plasmid();
    let r = apply_insert(&plasmid, &[gfp()], 0, "TTTT", true).unwrap();
    assert_eq!(r.sequence.len(), 5004);
    assert!(r.sequence.starts_with("TTTT"), "insert lands at the origin, not before the last base");
    assert_eq!(r.sequence, format!("TTTT{plasmid}"));
}

#[test]
fn at_1_is_equivalent_to_the_origin() {
    let plasmid = plasmid();
    let r = apply_insert(&plasmid, &[gfp()], 1, "TTTT", true).unwrap();
    assert_eq!(r.sequence, format!("TTTT{plasmid}"));
}

#[test]
fn insertion_shifts_downstream_features_right() {
    let r = apply_insert(&plasmid(), &[gfp()], 1000, "TTTT", false).unwrap();
    assert_eq!(named(&r.features, "GFP").map(|f| f.start), Some(2004));
    assert_eq!(named(&r.features, "GFP").map(|f| f.end), Some(2504));
}

// applyReplace — circular

#[test]
fn a_wrapping_replace_does_not_corrupt_the_sequence() {
    let plasmid = plasmid();
    let r = apply_replace(&plasmid, &[ampr(), gfp()], 4950, 50, "GGGG", true).unwrap();
    // 101 bases out, 4 in.
    assert_eq!(r.sequence.len(), 4899 + 4);
    assert_eq!(r.sequence, format!("GGGG{}", &plasmid[50..4949]), "replacement starts the new origin, matching browser editing");
}

#[test]
fn apply_replace_reports_a_clipped_feature_as_truncated_not_merely_shifted() {
    // The distinction the delta could not previously make: `shifted` says it survived intact
    // somewhere else, `removed` says it is gone. Neither is true of a CDS with half of it left.
    let seq = "ACGT".repeat(50); // 200 bp
    let feats = [
        feature("clipped", "CDS", 90, 120, Strand::Forward),
        feature("eaten", "CDS", 102, 108, Strand::Forward),
        feature("untouched", "CDS", 10, 20, Strand::Forward),
    ];
    let r = apply_replace(&seq, &feats, 101, 110, "", false).unwrap(); // delete 0-based [100, 110)
    let t = &r.delta.truncated;
    assert_eq!(t.len(), 1, "one clipped feature");
    assert_eq!(t[0].before.name, "clipped");
    assert_eq!(t[0].lost_bases, 10, "bases 100..109 were inside it");
    assert!(!t[0].bridged);
    assert_eq!(r.delta.removed.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["eaten"], "fully consumed is removed, not truncated");
    assert!(!t.iter().any(|x| x.before.name == "untouched"), "an untouched feature is not truncated");
}

#[test]
fn inserting_at_the_origin_grows_only_the_origin_spanning_features_own_arc() {
    let plasmid = plasmid();
    let wrapper = feature("wrapper", "CDS", LEN - 20, 19, Strand::Forward);
    let r = apply_insert(&plasmid, &[wrapper], 1, "AAAAA", true).unwrap();
    let f = &r.features[0];
    assert_eq!((f.start, f.end), (LEN - 15, 24));
    assert_eq!(format!("{}{}", &r.sequence[f.start..], &r.sequence[..f.end + 1]), format!("{}AAAAA{}", &plasmid[LEN - 20..], &plasmid[..20]));
    assert_eq!(r.delta.truncated, [], "growing an annotation is not truncation");
}

#[test]
fn an_interior_deletion_truncates_without_bridging_the_flanks_become_adjacent() {
    let feats = [feature("spanning", "CDS", 10, 60, Strand::Forward)];
    let r = apply_replace(&plasmid(), &feats, 31, 40, "", false).unwrap();
    let t = r.delta.truncated.first().expect("reported");
    assert_eq!(t.lost_bases, 10);
    assert!(!t.bridged, "one contiguous run survives, so nothing is bridged");
}

fn origin_edits_retain_the_original_feature_arc_and_oligo(sign: i8) {
    let sequence = "ACGTTGCAAGTCGATCGTACGATC";
    let feature = feature("Origin", "misc_feature", 20, 3, strand(sign));
    let primer = Feature {
        name: "Primer".to_string(),
        kind: "primer_bind".to_string(),
        notes: notes(&[("sequence", &["GGGATCACGT"]), ("bios_primer_tail_length", &["2"])]),
        ..feature.clone()
    };
    for at in [1, sequence.len() as i64 + 1] {
        let r = apply_insert(sequence, &[feature.clone(), primer.clone()], at, "TTAAA", true).unwrap();
        let f = &r.features[0];
        assert_eq!((f.start, f.end), if at == 1 { (25, 8) } else { (20, 3) });
        assert_eq!(format!("{}{}", &r.sequence[f.start..], &r.sequence[..f.end + 1]), format!("{}TTAAA{}", &sequence[20..], &sequence[..4]));
        assert_eq!(r.features[1].notes, primer.notes);
        assert_eq!(r.delta.shifted.len(), 2);
        assert_eq!(r.delta.truncated, []);
    }
    let r = apply_replace(sequence, &[feature.clone(), primer.clone()], 23, 2, "TTAA", true).unwrap();
    assert_eq!(r.sequence, format!("TTAA{}", &sequence[2..22]));
    assert_eq!(r.features.iter().map(|f| (f.start, f.end)).collect::<Vec<_>>(), [(22, 5), (22, 5)]);
    assert_eq!(r.features[1].notes, primer.notes);
    assert_eq!(r.delta.truncated, [], "a substitution retains annotation footprints rather than truncating them");
}

#[test]
fn origin_edits_retain_the_original_feature_arc_and_oligo_on_strand_1() {
    origin_edits_retain_the_original_feature_arc_and_oligo(1);
}

#[test]
fn origin_edits_retain_the_original_feature_arc_and_oligo_on_strand_minus_1() {
    origin_edits_retain_the_original_feature_arc_and_oligo(-1);
}

#[test]
fn whole_circle_selections_from_every_origin_remove_every_old_base_exactly_once() {
    let sequence = "ACGTTGCA";
    let feature = feature("Circle", "misc_feature", 6, 2, Strand::Forward);
    for from in 1..=sequence.len() as i64 {
        let to = if from == 1 { sequence.len() as i64 } else { from - 1 };
        let r = apply_replace(sequence, std::slice::from_ref(&feature), from, to, "GGG", true).unwrap();
        assert_eq!(r.sequence, "GGG");
        assert_eq!(r.features, []);
        assert_eq!(apply_delete(sequence, std::slice::from_ref(&feature), from, to, true).unwrap().sequence, "");
    }
}

#[test]
fn replacement_loss_counts_original_removed_bases_even_when_longer_replacement_dna_grows_the_feature() {
    let f = feature("Gene", "CDS", 1, 9, Strand::Forward);
    let r = apply_replace("ACGTTGCAAGTC", &[f], 5, 6, "TTTTTT", false).unwrap();
    assert_eq!(r.delta.truncated.first().map(|t| t.lost_bases), Some(2));
    assert_eq!(r.features[0].end, 13);
    assert_eq!(r.delta.truncated.first().map(|t| t.bridged), Some(true));
}

/// The web app also feeds 1.5, NaN and Infinity as positions, and 2.5 and NaN as range ends.
/// Positions here are `i64`, which cannot hold any of those, so those cases cannot be written
/// and cannot happen. Every case an `i64` can hold is kept.
#[test]
fn invalid_and_fractional_coordinates_cannot_silently_edit_another_range() {
    for at in [-1, 6] {
        assert_eq!(apply_insert("ACGT", &[], at, "A", true), Err(EditError::InsertPosition));
    }
    for (from, to) in [(0, 2), (1, 5), (3, 2)] {
        assert_eq!(apply_delete("ACGT", &[], from, to, false), Err(EditError::Range));
        assert_eq!(apply_replace("ACGT", &[], from, to, "A", false), Err(EditError::Range));
    }
}

fn substituting_a_whole_joined_feature_preserves_its_identity_and_coding_frame(sign: i8) {
    let sequence = "CCATGTGATAA";
    let f = Feature {
        notes: notes(&[("codon_start", &["3"]), ("transl_table", &["2"]), ("translation", &["stale"]), ("note", &["keep provenance"])]),
        ..with_locations(feature("Coding", "CDS", 0, 10, strand(sign)), &segments(&[(0, 1), (4, 10)]))
    };
    let result = apply_replace(sequence, std::slice::from_ref(&f), 1, 11, "GGTGGAAATAA", false).unwrap();
    assert_eq!(result.features.len(), 1);
    let expected = Feature { notes: notes(&[("codon_start", &["3"]), ("transl_table", &["2"]), ("note", &["keep provenance"])]), ..f.clone() };
    assert_eq!(result.features[0], expected);
    assert_eq!(result.delta.removed, []);
    assert_eq!(result.delta.truncated, []);
}

#[test]
fn substituting_a_whole_joined_feature_preserves_its_identity_and_coding_frame_strand_1() {
    substituting_a_whole_joined_feature_preserves_its_identity_and_coding_frame(1);
}

#[test]
fn substituting_a_whole_joined_feature_preserves_its_identity_and_coding_frame_strand_minus_1() {
    substituting_a_whole_joined_feature_preserves_its_identity_and_coding_frame(-1);
}
