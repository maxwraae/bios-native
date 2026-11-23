//! Gibson assembly design, ported from the web app's `gibson.test.ts` in its order and with its
//! numbers.
//!
//! The golden vectors in `fixtures/gibson.goldens.json` were emitted against dnakit's `core.tm` and
//! `gibson_validate/*` (Biopython Tm_NN, DNA_NN3, Na=50, dnac1=250, dnac2=0, saltcorr=5). The
//! Python output is the oracle; this matches it to 1e-3 °C.

mod support;

use std::sync::LazyLock;

use bios_core::enzymes::find_cut_sites;
use bios_core::gibson::{
    ArmSide, DesignGibsonInput, DesignGibsonMultiInput, DesignGibsonMultiResult, EblockPiece, GibsonCut, GibsonCutSite, GibsonError, GibsonFragment,
    JunctionKind, OpeningArc, SITE_MARGIN, check_site_survival, check_uniqueness, design_gibson, design_gibson_multi, detect_tandem_duplicate,
    extract_junction_overlap, gibson_tm, idt_preflight, resolve_gibson_opening, split_eblock, tm_range, tm_range_reverse,
};
use bios_core::model::{Feature, Strand};
use serde_json::Value;
use support::{feature, named};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

static G: LazyLock<Value> = LazyLock::new(|| serde_json::from_str(&fixture("gibson.goldens.json")).expect("json"));

/// The golden backbone every vector was generated against.
fn bb() -> &'static str {
    G["backbone"].as_str().expect("backbone")
}

fn near(a: f64, b: f64, eps: f64) {
    assert!((a - b).abs() < eps, "{a} ≉ {b}");
}

fn rows(key: &str) -> &'static [Value] {
    G[key].as_array().unwrap_or_else(|| panic!("{key}")).as_slice()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{v} is not a number"))
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("{v} is not an integer"))
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("{v} is not a string"))
}

// Convenience builders: the web app's optional arguments are fields here.

fn region(start: i64, end: i64) -> GibsonCut {
    GibsonCut::Region { start, end }
}

fn single(backbone: &str, circular: bool, cut: GibsonCut, insert: &str) -> DesignGibsonInput {
    DesignGibsonInput::new(backbone, circular, cut, insert)
}

fn multi(backbone: &str, circular: bool, cut: GibsonCut, inserts: &[(&str, &str)]) -> DesignGibsonMultiInput {
    let inserts = inserts.iter().map(|(name, seq)| GibsonFragment { name: (*name).to_string(), seq: (*seq).to_string() }).collect();
    DesignGibsonMultiInput::new(backbone, circular, cut, inserts)
}

/// Deterministic non-periodic bases (xorshift-ish hash → ACGT) so junction overlaps are unambiguous.
fn hash_seq(n: usize, seed: u32) -> String {
    let mut x = seed;
    let mut out = String::with_capacity(n);
    for _ in 0..n {
        x = next_x(x);
        out.push(b"ACGT"[(x & 3) as usize] as char);
    }
    out
}

/// One step of the web app's xorshift. `>>` there is a signed shift, so the sign extends.
fn next_x(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= ((x as i32) >> 17) as u32;
    x ^= x << 5;
    x
}

#[test]
fn gibson_tm_matches_dnakit_tm_to_1e_3_c() {
    for row in rows("tm") {
        near(gibson_tm(text(&row["seq"])).expect("tm"), num(&row["tm"]), 1e-3);
    }
}

#[test]
fn tm_range_reproduces_tm_range_walk_right_to_65_c_min_18() {
    for g in rows("tm_range") {
        let start = int(&g["start"]);
        let r = tm_range(bb(), start, 65.0, 18, 60);
        assert_eq!(r.pos, int(&g["end"]), "end@{start}");
        assert_eq!(&bb()[start as usize..r.pos as usize], text(&g["seq"]));
        near(r.tm, num(&g["tm"]), 1e-3);
    }
}

#[test]
fn tm_range_reverse_reproduces_tm_range_reverse_walk_left_to_65_c() {
    for g in rows("tm_range_reverse") {
        let end = int(&g["end"]);
        let r = tm_range_reverse(bb(), end, 65.0, 18, 60);
        assert_eq!(r.pos, int(&g["start"]), "start@{end}");
        assert_eq!(&bb()[r.pos as usize..end as usize], text(&g["seq"]));
        near(r.tm, num(&g["tm"]), 1e-3);
    }
}

#[test]
fn extract_junction_overlap_matches_oracle() {
    for g in rows("extract_junction_overlap") {
        assert_eq!(extract_junction_overlap(text(&g["a"]), text(&g["b"]), 3, 60), text(&g["r"][0]));
    }
}

#[test]
fn check_uniqueness_matches_oracle_fwd_rc_circular_wrap() {
    for g in rows("check_uniqueness") {
        let r = check_uniqueness(text(&g["p"]), text(&g["o"]), g["circ"].as_bool().expect("circ"));
        assert_eq!([r.fwd as i64, r.rc as i64], [int(&g["r"][0]), int(&g["r"][1])]);
    }
}

#[test]
fn detect_tandem_duplicate_matches_oracle() {
    for g in rows("detect_tandem_duplicate") {
        let r = detect_tandem_duplicate(text(&g["seq"]), int(&g["min"]), 100);
        let want = g["r"].as_array().expect("r");
        match want.first() {
            None => assert_eq!(r, None),
            Some(first) => {
                let r = r.expect("a hit");
                assert_eq!((r.start, r.len, r.unit.as_str()), (int(&first[0]), int(&first[1]), text(&first[2])));
            }
        }
    }
}

#[test]
fn idt_preflight_matches_oracle_issue_codes() {
    for g in rows("idt_preflight") {
        let name = text(&g["name"]);
        let declared = g["declared"].as_i64();
        let mut got: Vec<String> = idt_preflight(name, &seq_for(name), declared).iter().map(|i| format!("{}:{}", i.level, i.code)).collect();
        let mut want: Vec<String> =
            g["issues"].as_array().expect("issues").iter().map(|i| format!("{}:{}", text(&i["level"]), text(&i["code"]))).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{name}");
    }
}

/// The preflight goldens don't ship the raw seq (only its length); these are the exact fixtures the
/// generator used, so `idt_preflight` sees identical bytes.
fn seq_for(name: &str) -> String {
    let unit = "CCCGTGACCGAGAGAGTTAGC";
    match name {
        "mismatch" => "A".repeat(1134),
        "short" => "A".repeat(118),
        "homo" => format!("{}{}{}", "ACGA".repeat(20), "T".repeat(15), "AGCA".repeat(20)),
        "tandem" => format!("{}{unit}{unit}", "A".repeat(400)),
        "clean" => format!("{}AGCTCCAAGGTTCAGATCCTAGGCACCTGACAATGGTCGA", "ACGT".repeat(190)),
        other => panic!("unknown preflight fixture {other}"),
    }
}

#[test]
fn design_gibson_reproduces_the_golden_eblock_design_region_cut_65_c_linear() {
    let g = &G["design"][0]; // cutUp=300, cutDown=360 (60 bp replaced)
    let cut = region(int(&g["cutUp"]), int(&g["cutDown"]) - 1); // inclusive
    let r = design_gibson(&single(bb(), false, cut, text(&g["insert"]))).expect("design");
    assert_eq!(r.overlaps[0].left, text(&g["left"]), "left arm");
    assert_eq!(r.overlaps[0].right, text(&g["right"]), "right arm");
    near(r.overlaps[0].tm_left, round1(num(&g["tmLeft"])), 0.05);
    near(r.overlaps[0].tm_right, round1(num(&g["tmRight"])), 0.05);
    assert_eq!(r.eblocks.len(), 1);
    assert_eq!(r.eblocks[0].order, text(&g["eblock"]), "eBlock to order");
    assert_eq!(r.eblocks[0].length, int(&g["eblock_len"]));
    assert_eq!(r.assembled.sequence, text(&g["assembled"]), "assembled sequence");
    assert_eq!(r.assembled.sequence.len() as i64, int(&g["assembled_len"]));
}

/// `Number(x.toFixed(1))` of the test file, for comparing against the goldens' unrounded figures.
fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap()
}

#[test]
fn design_gibson_assembled_is_backbone_with_region_replaced_by_insert() {
    let r = design_gibson(&single(bb(), true, region(100, 149), "ATGCATGCATGCATGC")).expect("design");
    assert_eq!(r.assembled.sequence, format!("{}ATGCATGCATGCATGC{}", &bb()[..100], &bb()[150..]));
    // Insert + both overlap features + two cut markers present.
    let names: Vec<&str> = r.assembled.features.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"Insert"));
    assert!(names.iter().any(|n| n.starts_with("Overlap L")));
    assert!(names.iter().any(|n| n.starts_with("Overlap R")));
}

#[test]
fn design_gibson_carries_and_shifts_backbone_features_across_the_junction() {
    // remove 60, insert 30 → delta -30
    let mut input = single(bb(), true, region(200, 259), &"A".repeat(30));
    input.backbone_features = vec![
        feature("before", "CDS", 10, 40, Strand::Forward),
        feature("after", "CDS", 400, 430, Strand::Forward),
        feature("straddle", "CDS", 250, 300, Strand::Forward),
    ];
    let r = design_gibson(&input).expect("design");
    let before = named(&r.assembled.features, "before").expect("before");
    let after = named(&r.assembled.features, "after").expect("after");
    assert_eq!((before.start, before.end), (10, 40), "upstream feature unchanged");
    assert_eq!((after.start, after.end), (400 - 30, 430 - 30), "downstream feature shifted by delta");

    // The assembled construct is a file the user keeps, and half of a straddling CDS is physically
    // present in it. Dropping it would assert "these bases carry no annotation", a lie about the
    // molecule.
    let straddle = named(&r.assembled.features, "straddle").expect("straddler kept, not dropped");
    assert_eq!((straddle.start, straddle.end), (230, 270), "straddler truncated to its surviving half, shifted");
    let t = r.assembled.carry.truncated.iter().find(|x| x.before.name == "straddle").expect("the truncation is reported, not silent");
    assert_eq!(t.lost_bases, 10, "10 of its 51 bases were inside the opening");
    assert!(!t.bridged, "one surviving run, so nothing was bridged");
    let note = straddle.note("note").unwrap_or_default().join(" ");
    assert!(note.contains("bios-partial: truncated by 10 bp"), "and it says so on the feature a recipient will read: {note}");
    assert_eq!(r.assembled.carry.removed.len(), 0, "nothing was entirely inside the opening");
    assert_eq!(r.assembled.carry.carried.len(), 3, "all three parent features reached the product");
}

#[test]
fn split_eblock_oversized_eblock_splits_into_pieces_that_reassemble_and_fit() {
    let full = hash_seq(7000, 12345);
    let pieces = split_eblock(&full, 65.0, 3000, "eB", false).expect("split");
    assert!(pieces.len() >= 3, "expected ≥3 pieces, got {}", pieces.len());
    for p in &pieces {
        assert!(p.order.len() <= 3000, "{} is {} bp > 3000", p.name, p.order.len());
    }
    // Each interior piece shares its declared overlap with the previous piece; splice back.
    let mut acc = pieces[0].order.clone();
    for (i, p) in pieces.iter().enumerate().skip(1) {
        let ov = &p.join_prev.as_ref().expect("joinPrev").seq;
        assert!(ov.len() >= 20, "piece {i} overlap only {} bp", ov.len());
        assert!(p.order.starts_with(ov.as_str()), "piece {i} does not start with its overlap");
        assert!(acc.ends_with(ov.as_str()), "piece {} does not end with the shared overlap", i - 1);
        acc.push_str(&p.order[ov.len()..]);
    }
    assert_eq!(acc, full, "reassembled sequence equals original");
}

#[test]
fn design_gibson_a_large_insert_produces_multiple_eblocks_with_internal_overlaps() {
    let insert: String = (0..6500).map(|i: usize| b"ACGT"[(i * 5 + 1) % 4] as char).collect();
    let r = design_gibson(&single(bb(), true, region(500, 559), &insert)).expect("design");
    assert!(r.eblocks.len() >= 3, "expected multi-eBlock split, got {}", r.eblocks.len());
    for eb in &r.eblocks {
        assert!(eb.length <= 3000);
    }
}

// ---- design_gibson_multi (N ordered named fragments) ----

#[test]
fn design_gibson_multi_n_1_matches_design_gibson_assembled_sequence_and_outer_arms() {
    let insert = hash_seq(120, 999);
    let single_r = design_gibson(&single(bb(), false, region(300, 359), &insert)).expect("single");
    let multi_r = design_gibson_multi(&multi(bb(), false, region(300, 359), &[("X", &insert)])).expect("multi");
    assert_eq!(multi_r.assembled.sequence, single_r.assembled.sequence, "same assembled sequence");
    assert_eq!(multi_r.junctions.len(), 2, "N=1 → backbone↔insert + insert↔backbone only");
    assert_eq!(multi_r.junctions[0].seq, single_r.overlaps[0].left, "same left arm");
    assert_eq!(multi_r.junctions[1].seq, single_r.overlaps[0].right, "same right arm");
    assert_eq!(multi_r.eblocks.len(), 1);
}

#[test]
fn design_gibson_multi_assembled_is_backbone_with_region_replaced_by_concatenated_fragments() {
    let (f1, f2, f3) = (hash_seq(150, 11), hash_seq(150, 22), hash_seq(150, 33));
    let inserts = [("prom", f1.as_str()), ("gene", f2.as_str()), ("term", f3.as_str())];
    let r = design_gibson_multi(&multi(bb(), true, region(100, 149), &inserts)).expect("design");
    assert_eq!(r.assembled.sequence, format!("{}{f1}{f2}{f3}{}", &bb()[..100], &bb()[150..]));
    assert_eq!(r.junctions.len(), 4, "3 fragments → 2 internal + 2 outer junctions");
    assert_eq!(r.junctions.iter().filter(|j| j.kind == JunctionKind::InsertInsert).count(), 2);
    for n in ["prom", "gene", "term"] {
        assert!(r.assembled.features.iter().any(|f| f.name == n), "{n} annotated");
    }
}

#[test]
fn design_gibson_multi_each_named_fragment_is_an_eblock_and_they_reassemble_to_arm_frags_arm() {
    let (f1, f2, f3) = (hash_seq(200, 11), hash_seq(220, 22), hash_seq(180, 33));
    let inserts = [("a", f1.as_str()), ("b", f2.as_str()), ("c", f3.as_str())];
    let r = design_gibson_multi(&multi(bb(), false, region(300, 359), &inserts)).expect("design");
    assert_eq!(r.eblocks.len(), 3, "one eBlock per fragment (none oversized)");
    assert_eq!(r.eblocks.iter().map(|e| e.fragment.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    let left_arm = &r.junctions[0].seq;
    let right_arm = &r.junctions[r.junctions.len() - 1].seq;
    let full = format!("{left_arm}{f1}{f2}{f3}{right_arm}");
    let internal: Vec<_> = r.junctions.iter().filter(|j| j.kind == JunctionKind::InsertInsert).collect();
    let mut acc = r.eblocks[0].order.clone();
    for i in 1..r.eblocks.len() {
        let ol = &internal[i - 1].seq;
        assert!(ol.len() >= 20, "junction {i} overlap only {} bp", ol.len());
        assert!(r.eblocks[i].order.starts_with(ol.as_str()), "eBlock {i} does not start with its overlap");
        assert!(acc.ends_with(ol.as_str()), "eBlock {} does not end with the shared overlap", i - 1);
        acc.push_str(&r.eblocks[i].order[ol.len()..]);
    }
    assert_eq!(acc, full, "eBlocks reassemble to leftArm + fragments + rightArm");
    // Placements point at the actual fragment bases in the assembled sequence.
    let segs: Vec<&str> = r.placements.iter().map(|p| &r.assembled.sequence[p.start..=p.end]).collect();
    assert_eq!(segs, [f1.as_str(), f2.as_str(), f3.as_str()], "placements land on each fragment");
}

#[test]
fn design_gibson_multi_a_fragment_over_the_synthesis_cap_sub_splits_with_internal_overlaps() {
    let (big, small) = (hash_seq(5000, 7), hash_seq(150, 8));
    let r = design_gibson_multi(&multi(bb(), true, region(200, 259), &[("big", &big), ("small", &small)])).expect("design");
    assert!(r.eblocks.iter().filter(|e| e.fragment == "big").count() >= 2, "oversized fragment sub-split");
    for eb in &r.eblocks {
        assert!(eb.length <= 3000, "{} is {} bp > 3000", eb.name, eb.length);
    }
}

#[test]
fn design_gibson_multi_cut_markers_sit_at_the_true_junctions_not_the_arm_ends() {
    let f1 = hash_seq(150, 11);
    let r = design_gibson_multi(&multi(bb(), true, region(100, 149), &[("a", &f1)])).expect("design");
    let up = named(&r.assembled.features, "5′ cut").expect("5′ cut");
    let down = named(&r.assembled.features, "3′ cut").expect("3′ cut");
    assert_eq!(up.start, 100, "5′ cut marks the first inserted base");
    assert_eq!(down.end, 100 + f1.len() - 1, "3′ cut marks the last inserted base");
}

#[test]
fn design_gibson_multi_warns_when_a_fragment_is_shorter_than_its_junction_overlap() {
    let (f1, tiny, f3) = (hash_seq(200, 11), hash_seq(10, 22), hash_seq(200, 33));
    let inserts = [("a", f1.as_str()), ("tiny", tiny.as_str()), ("c", f3.as_str())];
    let r = design_gibson_multi(&multi(bb(), true, region(100, 149), &inserts)).expect("design");
    assert!(r.warnings.iter().any(|w| w.contains("\"tiny\"") && w.contains("shorter than")), "expected short-fragment warning: {:?}", r.warnings);
}

#[test]
fn design_gibson_non_unique_overlap_raises_a_warning() {
    // A backbone whose upstream-of-cut region is duplicated elsewhere, so the left homology arm
    // binds twice → misassembly-risk warning.
    let arm = "GACTTCAGGCATTGCACGTAAGCTTGGCA"; // ~29 bp, unique-ish stem
    let filler = format!("TTTT{}", "AGCTGACATGCA".repeat(6));
    let backbone = format!("{filler}{arm}CCCCCCCCCC{arm}GGGTTTAAACCCGGGAAATTT{filler}");
    let at = (filler.len() + arm.len()) as i64;
    let mut input = single(&backbone, true, region(at, at + 9), "ATGCATGCATGCATGCATGC");
    input.target_tm = 55.0;
    let r = design_gibson(&input).expect("design");
    assert!(r.warnings.iter().any(|w| w.to_lowercase().contains("not unique")), "expected a uniqueness warning, got: {:?}", r.warnings);
}

// ---------------------------------------------------------------------------
// The battery that arrived with "the assembled construct is a file you keep".
//
// While the product was a preview picture, a silently dropped feature and a silently inverted
// opening were cosmetic. As a `.gb` the user saves, both are a map that lies — so each case below
// is one way the old code was wrong, pinned.
// ---------------------------------------------------------------------------

#[test]
fn design_gibson_reproduces_the_golden_pure_insertion_design_cut_up_equals_cut_down() {
    // This golden sat unasserted since it was generated: the region form is inclusive, so it cannot
    // express "insert here, remove nothing". `InsertAt` is what makes the second half reachable.
    let g = &G["design"][1];
    assert_eq!(int(&g["cutUp"]), int(&g["cutDown"]), "this golden is the zero-length opening");
    let r = design_gibson(&single(bb(), false, GibsonCut::InsertAt(int(&g["cutUp"])), text(&g["insert"]))).expect("design");
    assert_eq!(r.overlaps[0].left, text(&g["left"]), "left arm");
    assert_eq!(r.overlaps[0].right, text(&g["right"]), "right arm");
    near(r.overlaps[0].tm_left, round1(num(&g["tmLeft"])), 0.05);
    near(r.overlaps[0].tm_right, round1(num(&g["tmRight"])), 0.05);
    assert_eq!(r.eblocks[0].order, text(&g["eblock"]), "eBlock to order");
    assert_eq!(r.eblocks[0].length, int(&g["eblock_len"]));
    assert_eq!(r.assembled.sequence, text(&g["assembled"]), "assembled sequence");
    assert_eq!(r.assembled.sequence.len() as i64, int(&g["assembled_len"]));
    assert_eq!(r.cut.removed_len, 0, "nothing was removed");
}

#[test]
fn a_feature_entirely_inside_the_opening_is_removed_and_reported_as_removed() {
    let mut input = single(bb(), true, region(200, 299), &"A".repeat(10));
    input.backbone_features = vec![feature("inside", "CDS", 220, 260, Strand::Forward)];
    let r = design_gibson(&input).expect("design");
    assert_eq!(named(&r.assembled.features, "inside"), None);
    assert_eq!(r.assembled.carry.removed.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["inside"]);
    assert_eq!(r.assembled.carry.truncated.len(), 0, "removed is not truncated");
}

#[test]
fn an_origin_spanning_parent_feature_moves_only_the_arm_that_is_downstream_of_the_cut() {
    // Zero coverage before this: the old carry never shifted either arm, so an origin-spanning
    // feature silently kept parent coordinates on a product of a different length.
    let backbone = &bb()[..1200];
    // remove 60, insert 30 → delta -30
    let mut input = single(backbone, true, region(600, 659), &"A".repeat(30));
    input.backbone_features = vec![feature("wrapper", "CDS", 1180, 19, Strand::Forward)];
    let r = design_gibson(&input).expect("design");
    let w = named(&r.assembled.features, "wrapper").expect("carried");
    assert_eq!((w.start, w.end), (1150, 19), "the 1180..1199 arm is downstream of the cut and shifts by -30; the 0..19 arm is upstream and does not");
    assert_eq!(r.assembled.carry.truncated.len(), 0, "nothing was clipped — it only moved");
}

#[test]
fn a_region_opening_that_wraps_the_origin_removes_the_arc_through_the_origin() {
    let backbone = &bb()[..1200];
    let ins = "A".repeat(20);
    let mut input = single(backbone, true, region(1150, 49), &ins); // end < start ⇒ wraps
    input.backbone_features = vec![feature("far", "CDS", 600, 650, Strand::Forward)];
    let r = design_gibson(&input).expect("design");
    assert!(r.cut.wraps);
    assert_eq!(r.cut.removed_len, 100, "1150..1199 plus 0..49");
    assert_eq!(r.assembled.sequence, format!("{ins}{}", &backbone[50..1150]), "insert at the new origin, then the retained arc");
    let far = named(&r.assembled.features, "far").expect("far");
    assert_eq!((far.start, far.end), (570, 620), "shifted by the removed prefix and the replacement at the new origin");
}

#[test]
fn a_wrapping_region_on_a_linear_backbone_throws_instead_of_quietly_inverting() {
    let e = design_gibson(&single(bb(), false, region(900, 50), "AAAA")).expect_err("must refuse");
    let message = e.to_string();
    assert!(message.contains("wraps the origin") && message.contains("linear"), "{message}");
}

#[test]
fn two_enzymes_define_two_arcs_and_arc_chooses_which_one_leaves() {
    // The 4500/200 bug, pinned. Same two cuts, opposite products.
    let backbone = &bb()[..1200];
    let sites = vec![cut_site("EcoRI", 1100, None), cut_site("BamHI", 100, None)];
    let pair = |arc| GibsonCut::Enzymes { enzymes: ["EcoRI".to_string(), "BamHI".to_string()], sites: sites.clone(), arc, keep_sites: false };
    let between = design_gibson(&single(backbone, true, pair(None), "AAAA")).expect("between");
    let origin = design_gibson(&single(backbone, true, pair(Some(OpeningArc::Origin)), "AAAA")).expect("origin");

    assert_eq!((between.cut.arcs.between, between.cut.arcs.origin), (1000, 200));
    assert_eq!(between.cut.removed_len, 1000, "default takes the arc between the two cuts");
    assert_eq!(origin.cut.removed_len, 200, "arc:'origin' takes the short way round");
    assert!(origin.cut.wraps);
    assert_ne!(between.assembled.sequence, origin.assembled.sequence, "genuinely different constructs");
    assert_eq!(
        between.assembled.sequence.len() + origin.assembled.sequence.len(),
        backbone.len() + 2 * 4,
        "the two products partition the backbone, plus one insert each"
    );
}

fn cut_site(enzyme: &str, position: i64, recognition_start: Option<i64>) -> GibsonCutSite {
    GibsonCutSite { enzyme: enzyme.to_string(), position, recognition_start }
}

#[test]
fn when_the_origin_arc_is_the_smaller_one_an_unqualified_enzyme_pair_says_so() {
    let backbone = &bb()[..1200];
    let cut = GibsonCut::Enzymes {
        enzymes: ["EcoRI".to_string(), "BamHI".to_string()],
        sites: vec![cut_site("EcoRI", 1100, None), cut_site("BamHI", 100, None)],
        arc: None,
        keep_sites: false,
    };
    let r = design_gibson(&single(backbone, true, cut, "AAAA")).expect("design");
    assert_eq!(r.cut.removed_len, 1000, "behaviour unchanged — the default is still 'between'");
    assert!(r.warnings.iter().any(|w| w.contains("only 200 bp") && w.contains("arc:'origin'")), "expected the arc warning, got: {:?}", r.warnings);
}

#[test]
fn every_feature_on_the_product_is_in_range_and_only_circular_products_wrap() {
    let backbone = &bb()[..1200];
    let feats = [
        feature("a", "CDS", 10, 80, Strand::Forward),
        feature("b", "CDS", 590, 700, Strand::Forward),
        feature("c", "CDS", 1150, 30, Strand::Forward), // origin-spanning
    ];
    for circular in [true, false] {
        for ins_len in [0usize, 1, 30, 6500] {
            let mut cuts: Vec<(&str, GibsonCut)> = vec![("interior", region(600, 659)), ("insertion", GibsonCut::InsertAt(600))];
            if circular {
                cuts.push(("wrapping", region(1150, 49)));
            }
            for (label, cut) in cuts {
                let mut input = single(backbone, circular, cut, &"A".repeat(ins_len));
                input.backbone_features = if circular { feats.to_vec() } else { feats[..2].to_vec() };
                let r = design_gibson(&input).expect("design");
                let total = r.assembled.sequence.len();
                assert_eq!(r.assembled.circular, circular, "{label}/{ins_len}: topology preserved");
                for f in &r.assembled.features {
                    let where_ = format!("{label}/{ins_len}/{}/{}", if circular { "circ" } else { "lin" }, f.name);
                    assert!(f.start < total, "{where_}: start {} out of [0,{total})", f.start);
                    assert!(f.end < total, "{where_}: end {} out of [0,{total})", f.end);
                    if !circular {
                        assert!(f.start <= f.end, "{where_}: linear product must not wrap");
                    }
                }
            }
        }
    }
}

#[test]
fn the_insert_really_sits_where_the_product_says_it_does_wrapping_opening_included() {
    let backbone = &bb()[..1200];
    let ins = "ATGCATGCATGCATGCATGC";
    for cut in [region(600, 659), region(1150, 49), GibsonCut::InsertAt(600)] {
        let label = format!("{cut:?}");
        let mut input = single(backbone, true, cut, ins);
        input.insert_name = "Ins".to_string();
        let r = design_gibson(&input).expect("design");
        let f = named(&r.assembled.features, "Ins").unwrap_or_else(|| panic!("{label}: insert annotated"));
        let seq = &r.assembled.sequence;
        let got = if f.end >= f.start { seq[f.start..=f.end].to_string() } else { format!("{}{}", &seq[f.start..], &seq[..=f.end]) };
        assert_eq!(got, ins, "{label}: the annotated span holds the inserted bases (L={})", seq.len());
    }
}

#[test]
fn design_gibson_multi_n_1_carries_features_identically_to_design_gibson() {
    // The de-duplication proof. The two carries were byte-identical copies that had already been
    // fixed independently once, so the guard is deep equality, not a sequence check.
    let backbone = &bb()[..1200];
    let feats = vec![
        feature("before", "CDS", 10, 40, Strand::Forward),
        feature("straddle", "CDS", 590, 700, Strand::Forward),
        feature("inside", "CDS", 610, 640, Strand::Forward),
        feature("wrapper", "CDS", 1180, 19, Strand::Forward),
    ];
    for cut in [region(600, 659), region(1150, 49), GibsonCut::InsertAt(600)] {
        let label = format!("{cut:?}");
        let mut one = single(backbone, true, cut.clone(), &"A".repeat(30));
        one.insert_name = "F1".to_string();
        one.backbone_features = feats.clone();
        let one = design_gibson(&one).expect("single");
        let mut many = multi(backbone, true, cut, &[("F1", &"A".repeat(30))]);
        many.backbone_features = feats.clone();
        let many = design_gibson_multi(&many).expect("multi");
        assert_eq!(many.assembled.sequence, one.assembled.sequence, "{label}: same sequence");
        assert_eq!(many.assembled.carry, one.assembled.carry, "{label}: same carry report");
        let carried = |f: &[Feature]| f.iter().map(|f| (f.name.clone(), f.start, f.end)).collect::<Vec<_>>();
        assert_eq!(carried(&many.assembled.carry.carried), carried(&one.assembled.carry.carried), "{label}: same carried features");
    }
}

#[test]
fn an_eblock_cap_too_small_to_tile_is_refused_not_looped_over_forever() {
    // THE HANG. The fallback steps by `maxLen - 30`; at maxLen <= 30 that step is zero or negative,
    // so the loop allocates a piece per iteration until the tab dies. Reachable by typing: every
    // value you pass through on the way to "3000" is a real request, so "7" was a real request.
    let full = hash_seq(500, 7);
    for cap in [0, 1, 7, 29, 30] {
        assert_eq!(
            split_eblock(&full, 65.0, cap, "eB", false),
            Err(GibsonError::EblockCapBelowOverlap { max_len: cap }),
            "cap {cap} must be refused"
        );
    }
    // Just above the floor it must terminate and tile.
    let pieces = split_eblock(&full, 65.0, 31, "eB", false).expect("split");
    assert!(pieces.len() > 1, "it splits");
    assert!(pieces.iter().all(|p| p.order.len() <= 31), "every piece respects the cap");
    assert!(pieces.len() < full.len(), "and it is a finite number of them");
}

#[test]
fn a_design_asked_for_an_impossible_eblock_cap_reports_it_instead_of_hanging() {
    let mut input = single(bb(), true, region(100, 199), &"A".repeat(400));
    input.max_eblock_len = 7;
    let e = design_gibson(&input).expect_err("must refuse");
    assert!(e.to_string().contains("below the 30 bp overlap"), "{e}");
}

#[test]
fn eblock_placements_are_real_offsets_and_consecutive_pieces_overlap() {
    // The overlap is not an artefact to be tidied away in the drawing — it is the assembly. Two
    // pieces join because they share these bases.
    let full = hash_seq(4000, 99);
    let pieces = split_eblock(&full, 65.0, 1500, "eB", false).expect("split");
    assert!(pieces.len() >= 3, "expected ≥3 pieces, got {}", pieces.len());
    for p in &pieces {
        assert_eq!(slice(&full, p.start, p.end), p.order, "{}: start/end name its own bases", p.name);
    }
    assert_eq!(pieces[0].start, 0, "the first starts at the beginning");
    assert_eq!(pieces[pieces.len() - 1].end, full.len() as i64, "the last ends at the end");
    for i in 1..pieces.len() {
        let shared = pieces[i - 1].end - pieces[i].start;
        assert!(shared > 0, "{} must overlap its predecessor, got {shared}", pieces[i].name);
        assert_eq!(
            slice(&full, pieces[i].start, pieces[i - 1].end),
            pieces[i].join_prev.as_ref().expect("joinPrev").seq,
            "and the shared bases are exactly the reported overlap"
        );
    }
}

fn slice(s: &str, from: i64, to: i64) -> &str {
    &s[from as usize..to as usize]
}

#[test]
fn multi_eblocks_are_placed_on_the_whole_strand_arms_included() {
    let backbone = &bb()[..1200];
    let (a, b) = (hash_seq(900, 3), hash_seq(700, 4));
    let mut input = multi(backbone, true, region(600, 659), &[("A", &a), ("B", &b)]);
    input.max_eblock_len = 600;
    let r = design_gibson_multi(&input).expect("design");
    assert!(r.eblocks.len() > 2, "a 1600 bp insert at a 600 bp cap must split, got {}", r.eblocks.len());
    assert!(r.arms.left > 0 && r.arms.right > 0, "the arms are reported");
    // Every eBlock lies inside the laid-out strand, and they tile it end to end with overlaps.
    let strand_len = r.arms.left + 1600 + r.arms.right;
    assert_eq!(r.eblocks[0].start, 0);
    assert_eq!(r.eblocks[r.eblocks.len() - 1].end, strand_len);
    for i in 1..r.eblocks.len() {
        assert!(r.eblocks[i].start < r.eblocks[i - 1].end, "consecutive eBlocks overlap");
    }
    // The first one begins inside the upstream arm, which is what makes it orderable at all.
    assert!(r.eblocks[0].start - r.arms.left < 0, "the first eBlock reaches back into the arm");
}

// ---- matched overlaps: one reaction, one temperature ----------------------

/// A stretch whose GC content swings, so some places are hot at 18 bp and others are not.
fn patchy_seq(n: usize, seed: u32) -> String {
    let mut x = seed;
    let mut out = String::with_capacity(n);
    for i in 0..n {
        x = next_x(x);
        // Alternating 150 bp GC-rich and AT-rich blocks.
        let rich = (i / 150) % 2 == 0;
        out.push(if rich { b"GC"[(x & 1) as usize] as char } else { b"AT"[(x & 1) as usize] as char });
    }
    out
}

fn tms_of(pieces: &[EblockPiece]) -> Vec<f64> {
    pieces.iter().skip(1).map(|p| p.join_prev.as_ref().expect("joinPrev").tm).collect()
}

fn spread(t: &[f64]) -> f64 {
    t.iter().copied().fold(f64::NEG_INFINITY, f64::max) - t.iter().copied().fold(f64::INFINITY, f64::min)
}

fn worst(t: &[f64]) -> f64 {
    t.iter().map(|x| (x - 65.0).abs()).fold(f64::NEG_INFINITY, f64::max)
}

#[test]
fn matched_sliding_the_boundary_finds_a_junction_at_the_target_instead_of_77_c() {
    // A GC-rich block cannot be cooled by shortening: at 18 bp it is already far above 65, and
    // there is nothing shorter to try. The even-boundary walk lands in one and reports whatever it
    // finds; matching slides off it onto a stretch that actually melts at 65.
    let full = patchy_seq(2500, 11);
    let plain = split_eblock(&full, 65.0, 1400, "eB", false).expect("plain");
    let matched = split_eblock(&full, 65.0, 1400, "eB", true).expect("matched");

    assert!(matched.len() >= 2, "it still splits");
    let (m, p) = (tms_of(&matched), tms_of(&plain));
    assert!(worst(&m) <= worst(&p) + 1e-9, "matched should be no further from 65 than the even split: {m:?} vs {p:?}");
    assert!(worst(&m) < 3.0, "every junction near 65, got {m:?}");
    if m.len() > 1 {
        assert!(spread(&m) < 2.0, "and near each OTHER, got {m:?}");
    }
}

#[test]
fn matched_the_cap_is_a_ceiling_not_a_demand_that_the_pieces_be_equal() {
    // "if max size is 1400 and it is 2500 long then you can have one that is 100 longer than the
    // other, no problem" — the pieces are free to differ so the junctions can be right.
    let full = patchy_seq(2500, 12);
    let pieces = split_eblock(&full, 65.0, 1400, "eB", true).expect("split");
    for p in &pieces {
        assert!(p.order.len() <= 1400, "{} is {} > 1400", p.name, p.order.len());
    }
    assert_eq!(pieces[0].start, 0);
    assert_eq!(pieces[pieces.len() - 1].end, full.len() as i64);
    for i in 1..pieces.len() {
        assert_eq!(
            slice(&full, pieces[i].start, pieces[i - 1].end),
            pieces[i].join_prev.as_ref().expect("joinPrev").seq,
            "shared bases are the overlap"
        );
    }
}

#[test]
fn matched_every_junction_in_a_whole_design_runs_at_one_temperature() {
    let backbone = &bb()[..1200];
    let (a, b) = (patchy_seq(1500, 21), patchy_seq(900, 22));
    let args = |matched: bool| {
        let mut input = multi(backbone, true, region(300, 599), &[("A", &a), ("B", &b)]);
        input.max_eblock_len = 900;
        input.match_overlaps = matched;
        input
    };
    let plain = design_gibson_multi(&args(false)).expect("plain");
    let matched = design_gibson_multi(&args(true)).expect("matched");

    let all_tms = |r: &DesignGibsonMultiResult| {
        let mut t: Vec<f64> = r.junctions.iter().map(|j| j.tm).collect();
        t.extend(r.eblocks.iter().filter_map(|e| e.junction_tm));
        t
    };
    let (m, p) = (all_tms(&matched), all_tms(&plain));
    assert!(spread(&m) <= spread(&p), "matched must be at least as uniform: {m:?} vs {p:?}");
    assert!(spread(&m) < 3.0, "every junction within 3 °C of the others, got {m:?}");
    // The assembled construct is the same molecule either way — only the ordering changed.
    assert_eq!(matched.assembled.sequence, plain.assembled.sequence);
}

#[test]
fn matched_when_nothing_can_reach_the_target_everyone_drops_to_what_is_reachable() {
    // A uniformly GC-rich strand has no cool stretch to slide onto: 65 is unreachable at any length
    // or position. The right answer is not to leave one junction at 65 and the rest at 80 — it is
    // for all of them to run together at the coolest temperature the set can actually hold.
    let gc = "GCCGGCGCCGGCGCGCCGGCGCCGGCGC".repeat(120);
    let pieces = split_eblock(&gc, 65.0, 1000, "eB", true).expect("split");
    let tms = tms_of(&pieces);
    assert!(!tms.is_empty(), "it splits");
    assert!(tms.iter().copied().fold(f64::INFINITY, f64::min) > 65.0, "65 really is out of reach here");
    assert!(spread(&tms) < 3.0, "but they still match each other: {tms:?}");
}

#[test]
fn matched_a_sweep_of_sequences_every_design_uniform_and_none_worse_than_the_plain_split() {
    // The property, not one example. Across GC fractions and caps: every junction in a design must
    // agree with the others, the pieces must still tile and reassemble, and matching must never
    // make a design further from the target than the even-boundary split would have.
    let mut checked = 0;
    for gc in [0.3f64, 0.5, 0.7] {
        for (n, cap) in [(2500usize, 1400i64), (4000, 1200), (6500, 3000)] {
            let full = sweep_seq(n, gc, (n as f64 + gc * 1000.0).round() as u32);
            let plain = split_eblock(&full, 65.0, cap, "e", false).expect("plain");
            let matched = split_eblock(&full, 65.0, cap, "e", true).expect("matched");
            let label = format!("gc{gc} {n}@{cap}");

            for ps in [&plain, &matched] {
                check_tiling(ps, &full, cap, &label);
            }

            let (m, p) = (tms_of(&matched), tms_of(&plain));
            if m.len() > 1 {
                assert!(spread(&m) <= 0.5, "{label}: junctions must agree, got {m:?}");
                assert!(spread(&m) <= spread(&p) + 1e-9, "{label}: matched spread worse than plain");
            }
            assert!(worst(&m) <= worst(&p) + 1e-9, "{label}: matched further from target than plain");
            checked += 1;
        }
    }
    assert_eq!(checked, 9, "the sweep actually ran");
}

/// The web app's `rand`/`seq` pair, which drives the sweep above.
fn sweep_seq(n: usize, gc: f64, seed: u32) -> String {
    let mut x = if seed == 0 { 1 } else { seed };
    let mut next = move || {
        x = next_x(x);
        f64::from(x) / 4294967296.0
    };
    let mut s = String::with_capacity(n);
    for _ in 0..n {
        let rich = next() < gc;
        let second = next() < 0.5;
        s.push(if rich {
            if second { 'G' } else { 'C' }
        } else if second {
            'A'
        } else {
            'T'
        });
    }
    s
}

fn check_tiling(pieces: &[EblockPiece], full: &str, cap: i64, label: &str) {
    for p in pieces {
        assert!(p.order.len() as i64 <= cap, "{label}: {} over cap", p.name);
    }
    assert_eq!(pieces[0].start, 0, "{label}: starts at 0");
    assert_eq!(pieces[pieces.len() - 1].end, full.len() as i64, "{label}: covers the strand");
    let mut acc = pieces[0].order.clone();
    for i in 1..pieces.len() {
        let ov = &pieces[i].join_prev.as_ref().expect("joinPrev").seq;
        assert_eq!(slice(full, pieces[i].start, pieces[i - 1].end), *ov, "{label}: {} overlap is the shared bases", pieces[i].name);
        assert!(acc.ends_with(ov.as_str()), "{label}: {} does not join", pieces[i].name);
        acc.push_str(&pieces[i].order[ov.len()..]);
    }
    assert_eq!(acc, full, "{label}: reassembles");
}

#[test]
fn an_opening_with_no_room_for_an_arm_is_refused_not_reported_as_a_0_c_junction() {
    // Open a LINEAR backbone at its very end and there is nothing downstream to anneal to. The arm
    // came back empty, the Tm of "" is nothing, and it was rounded to 0.0 and drawn on the map as
    // though it were a temperature — a number that is not true.
    let short = &bb()[..240];
    let ins = "ATGC".repeat(50);
    for cut in [region(239, 239), region(0, 0)] {
        let e = design_gibson_multi(&multi(short, false, cut, &[("X", &ins)])).expect_err("must refuse");
        assert!(e.to_string().contains("too little for a homology arm"), "{e}");
        assert!(matches!(e, GibsonError::NoRoomForArm { side, .. } if side == ArmSide::Upstream || side == ArmSide::Downstream));
    }
    // A circular backbone has no ends, so the same cut is fine there.
    let ok = design_gibson_multi(&multi(short, true, region(239, 239), &[("X", &ins)])).expect("circular is fine");
    assert!(ok.junctions.iter().all(|j| j.tm > 30.0), "no phantom junctions: {:?}", ok.junctions.iter().map(|j| j.tm).collect::<Vec<_>>());
}

#[test]
fn a_pure_insertion_into_a_circular_plasmid_still_gets_real_homology_arms() {
    // A full turn, not an empty string: nothing was removed, so all of the backbone survives. Read
    // as empty it left both arms blank and both junctions at 0 °C.
    let backbone = &bb()[..1200];
    let r = design_gibson_multi(&multi(backbone, true, GibsonCut::InsertAt(600), &[("X", "ATGCATGCATGCATGCATGCATGC")])).expect("design");
    let arms: Vec<_> = r.junctions.iter().filter(|j| j.kind != JunctionKind::InsertInsert).collect();
    assert_eq!(arms.len(), 2);
    for a in arms {
        assert!(a.seq.len() >= 18, "{:?}: arm is {} bp", a.kind, a.seq.len());
        assert!(a.tm > 40.0, "{:?}: {} °C is not a real junction", a.kind, a.tm);
    }
    assert_eq!(r.cut.removed_len, 0, "nothing was removed");
    assert_eq!(r.assembled.sequence.len(), backbone.len() + 24, "the whole backbone survives");
}

// ---------------------------------------------------------------------------
// KEEP THE SITE — the opening goes outside the recognition site, not through it.
// ---------------------------------------------------------------------------

/// Filler that contains none of the sites planted below, so every site is where the test put it and
/// nowhere else. "ACATCA" tiles cleanly and is site-free for the enzymes used here.
fn fill(n: usize) -> String {
    "ACATCA".repeat(n.div_ceil(6))[..n].to_string()
}

/// Ends in CAC rather than GGG on purpose: butted against a retained "GATCC" a G-rich tail spells a
/// fresh GGATCC at the junction, and an accidental site would make the assertions below lie.
const INS: &str = "ATGGCTAGCAAAGGAGAAGAACTTTTCACTGGAGTTGTCCCAATTCTTGTTGAATTAGATGGTGATGTTAATCAC";

/// A cut ready for `keep_sites`, resolved off the backbone the way the app resolves it.
fn cut_of(seq: &str, a: &str, b: &str, keep_sites: bool, circular: bool) -> GibsonCut {
    let sites =
        find_cut_sites(seq, &[a, b], circular).into_iter().map(|c| cut_site(c.enzyme, c.top_cut as i64, Some(c.recognition_start as i64))).collect();
    GibsonCut::Enzymes { enzymes: [a.to_string(), b.to_string()], sites, arc: None, keep_sites }
}

fn design(seq: &str, a: &str, b: &str, keep_sites: bool, circular: bool) -> DesignGibsonMultiResult {
    built_with(seq, cut_of(seq, a, b, keep_sites, circular), INS, circular)
}

/// `design_gibson_multi` with `keep_sites`, one named insert, the way BUILD calls it.
fn built(seq: &str, a: &str, b: &str, ins: &str, circular: bool) -> DesignGibsonMultiResult {
    built_with(seq, cut_of(seq, a, b, true, circular), ins, circular)
}

fn built_with(seq: &str, cut: GibsonCut, ins: &str, circular: bool) -> DesignGibsonMultiResult {
    let mut input = multi(seq, circular, cut, &[("GOI", ins)]);
    input.match_overlaps = true;
    design_gibson_multi(&input).expect("design")
}

/// GAATTC at 300..305 (EcoRI, cuts at 301) and GGATCC at 426..431 (BamHI, cuts at 427).
fn planted() -> String {
    format!("{}GAATTC{}GGATCC{}", fill(300), fill(120), fill(300))
}

fn cutters(seq: &str, names: &[&str]) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = find_cut_sites(seq, names, true).into_iter().map(|c| c.enzyme).collect();
    found.sort_unstable();
    found
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn keeping_the_site_the_enzymes_still_cut_the_plasmid_you_made() {
    // THE WHOLE POINT. An enzyme cuts INSIDE what it reads, so opening at the scissile bond splits
    // the site and the half in the excised arc leaves with it — a product neither enzyme can touch.
    let planted = planted();
    let lost = design(&planted, "EcoRI", "BamHI", false, true);
    assert_eq!(
        find_cut_sites(&lost.assembled.sequence, &["EcoRI", "BamHI"], true),
        vec![],
        "opening at the cuts destroys both sites — this is the behaviour being replaced"
    );

    let kept = design(&planted, "EcoRI", "BamHI", true, true);
    assert_eq!(cutters(&kept.assembled.sequence, &["EcoRI", "BamHI"]), ["BamHI", "EcoRI"], "both sites are on the product, and still cut");
    assert_eq!(kept.cut.kept_sites, Some(vec!["EcoRI".to_string(), "BamHI".to_string()]));
}

#[test]
fn the_opening_is_the_sites_edge_plus_a_base_not_the_bond_inside_it() {
    let planted = planted();
    let cut = cut_of(&planted, "EcoRI", "BamHI", true, true);
    let o = resolve_gibson_opening(planted.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_eq!(o.up, 307, "GAATTC ends at 305; +1 to clear it, +1 of margin");
    assert_eq!(o.down, 425, "GGATCC starts at 426; one base of margin before it");
    assert_eq!(o.removed_len, 118, "118 bp, not the 126 between the two cuts");
    // The arcs the ⇄ control names must be the arcs this rule would actually remove.
    assert_eq!(o.arcs.between, 118);
    // The two arcs no longer partition the molecule: the 16 bases of the two sites and their
    // margins belong to neither, because both arcs now stop short of them.
    assert_eq!(o.arcs.between + o.arcs.origin, planted.len() as i64 - 16);
}

#[test]
fn keeping_the_site_is_off_unless_asked_for_and_then_nothing_moves() {
    let planted = planted();
    let cut = cut_of(&planted, "EcoRI", "BamHI", false, true);
    let o = resolve_gibson_opening(planted.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_eq!(o.up, 301, "the EcoRI scissile bond, as before");
    assert_eq!(o.down, 427);
    assert_eq!(o.removed_len, 126);
    assert_eq!(o.kept_sites, None);
}

#[test]
fn sites_that_sit_next_to_each_other_leave_nothing_to_remove_and_it_says_so() {
    // A real MCS shape: GAATTC immediately followed by GGATCC. Keeping both whole means the
    // boundaries meet, so there is no arc left between them — a pure insertion, not an error.
    let abut = format!("{}GAATTCGGATCC{}", fill(300), fill(300));
    let mut warnings: Vec<String> = Vec::new();
    let cut = cut_of(&abut, "EcoRI", "BamHI", true, true);
    let o = resolve_gibson_opening(abut.len() as i64, true, &cut, &mut warnings, SITE_MARGIN).expect("opening");
    assert_eq!(o.removed_len, 0, "nothing comes out");
    assert_eq!(o.up, o.down);
    assert_eq!(o.up, 306, "between the two sites, margin dropped so both survive");
    assert_eq!(o.kept_sites, Some(vec!["EcoRI".to_string(), "BamHI".to_string()]));
    assert!(warnings.iter().any(|w| w.contains("next to each other")), "{}", warnings.join(" | "));

    let d = design(&abut, "EcoRI", "BamHI", true, true);
    assert_eq!(cutters(&d.assembled.sequence, &["EcoRI", "BamHI"]), ["BamHI", "EcoRI"]);
    assert_eq!(d.assembled.sequence.len(), abut.len() + INS.len(), "a pure insertion");
}

#[test]
fn sites_that_overlap_are_both_kept_there_was_never_anything_between_them_to_remove() {
    // AasI reads GACNNNNNNGTC — twelve bases with room for a whole EcoRI site inside it. The three
    // bases "between the two scissile bonds" are INSIDE the pair's own recognition sequence, so
    // taking them out is not an opening, it is vandalism against both sites. Insert at the far edge.
    let nested = format!("{}GACGAATTCGTC{}", fill(300), fill(300));
    let mut warnings: Vec<String> = Vec::new();
    let cut = cut_of(&nested, "AasI", "EcoRI", true, true);
    let o = resolve_gibson_opening(nested.len() as i64, true, &cut, &mut warnings, SITE_MARGIN).expect("opening");
    assert_eq!(o.kept_sites.map(sorted), Some(vec!["AasI".to_string(), "EcoRI".to_string()]));
    assert_eq!(o.removed_len, 0, "nothing comes out");
    assert_eq!(o.up, 312, "the far edge of the twelve-base AasI site, which contains the other");
    assert_eq!(o.down, 312);
    assert!(warnings.iter().any(|w| w.contains("overlapping DNA")), "{}", warnings.join(" | "));
    let d = design(&nested, "AasI", "EcoRI", true, true);
    assert_eq!(d.sites.iter().map(|x| x.on_product).collect::<Vec<_>>(), [1, 1], "both still cut the product");
}

#[test]
fn type_iis_is_kept_like_anything_else_whichever_side_of_its_cut_the_site_is_on() {
    // BsaI reads GGTCTC and cuts a base BESIDE it, so the site's fate depends on orientation and on
    // which arc leaves — it used to be a coin flip nobody was told about. Golden Gate does want that
    // site gone, but this is the Gibson designer: the enzyme linearises the backbone and is never in
    // the reaction, so a site the user chose is a site the user keeps.
    let plus = format!("{}GGTCTC{}GGATCC{}", fill(300), fill(120), fill(300));
    let cut = cut_of(&plus, "BsaI", "BamHI", true, true);
    let o = resolve_gibson_opening(plus.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_eq!(o.kept_sites.map(sorted), Some(vec!["BamHI".to_string(), "BsaI".to_string()]));
    assert_eq!(o.up, find_cut_sites(&plus, &["BsaI"], true)[0].top_cut as i64, "reading outward, its own cut already clears the site");
    assert_eq!(design(&plus, "BsaI", "BamHI", true, true).sites.iter().map(|x| x.on_product).collect::<Vec<_>>(), [1, 1]);

    // The same site on the MINUS strand reads back across its own cut, so the boundary has to move
    // the other way. This is the orientation that used to lose the site.
    let minus = format!("{}GAGACC{}GGATCC{}", fill(300), fill(120), fill(300));
    let cut = cut_of(&minus, "BsaI", "BamHI", true, true);
    let m = resolve_gibson_opening(minus.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_eq!(m.kept_sites.map(sorted), Some(vec!["BamHI".to_string(), "BsaI".to_string()]));
    assert_eq!(design(&minus, "BsaI", "BamHI", true, true).sites.iter().map(|x| x.on_product).collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn a_site_that_straddles_the_origin_is_kept_whole_too() {
    // AATTC at the very start and G at the very end: GAATTC exists only by going round.
    let wrapped = format!("AATTC{}GGATCC{}G", fill(600), fill(300));
    let d = design(&wrapped, "EcoRI", "BamHI", true, true);
    assert_eq!(cutters(&d.assembled.sequence, &["EcoRI", "BamHI"]), ["BamHI", "EcoRI"], "the site reassembles across the origin on the product");
}

#[test]
fn a_long_site_with_ns_in_it_is_measured_by_its_own_length_not_by_six() {
    // AasI is 12 bp. Clearing it with a hard-coded 6 would cut straight through the other half.
    let long = format!("{}GACTTTTTTGTC{}GGATCC{}", fill(300), fill(120), fill(300));
    let cut = cut_of(&long, "AasI", "BamHI", true, true);
    let o = resolve_gibson_opening(long.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_eq!(o.up, 313, "GACNNNNNNGTC ends at 311; +1 to clear it, +1 of margin");
    let d = design(&long, "AasI", "BamHI", true, true);
    assert_eq!(cutters(&d.assembled.sequence, &["AasI", "BamHI"]), ["AasI", "BamHI"]);
}

#[test]
fn the_product_is_annotated_with_the_overlaps_between_its_fragments_not_just_the_outer_two() {
    // A three-piece assembly meets the backbone twice and meets itself twice. Only the first two
    // were ever drawn, so a product made of several pieces showed nothing about how the pieces
    // join — and the join is the thing a synthesis house is being paid for.
    let backbone = "ATGC".repeat(400);
    let inserts = [
        ("alpha", "GGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAAGGCCTTAA"),
        ("beta", "TTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAATTGGCCAA"),
        ("gamma", "AACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTTAACCGGTT"),
    ];
    let mut input = multi(&backbone, true, region(100, 200), &inserts);
    input.target_tm = 60.0;
    let res = design_gibson_multi(&input).expect("design");

    let named_all: Vec<&str> = res.assembled.features.iter().map(|f| f.name.as_str()).collect();
    assert!(named_all.iter().any(|n| n.starts_with("Overlap L ")), "the backbone→first join");
    assert!(named_all.iter().any(|n| n.starts_with("Overlap R ")), "the last→backbone join");

    // One per internal junction, named for the pair it joins so the map says which is which.
    let internal: Vec<&&str> = named_all.iter().filter(|n| n.starts_with("Overlap ") && n.contains('–')).collect();
    let insert_insert: Vec<_> = res.junctions.iter().filter(|j| j.kind == JunctionKind::InsertInsert).collect();
    assert_eq!(internal.len(), insert_insert.len(), "one feature per internal junction");
    assert!(internal.iter().any(|n| n.starts_with("Overlap alpha–beta ")), "named for its pair: {internal:?}");
    assert!(internal.iter().any(|n| n.starts_with("Overlap beta–gamma ")), "named for its pair: {internal:?}");

    // AND IT SITS ON THE RIGHT BASES. A feature at the wrong offset draws happily and lies quietly.
    let seq = &res.assembled.sequence;
    for j in insert_insert {
        let prefix = format!("Overlap {}–{} ", j.left, j.right);
        let feat = res.assembled.features.iter().find(|f| f.name.starts_with(&prefix)).unwrap_or_else(|| panic!("no feature for {prefix}"));
        assert_eq!(span_of(seq, feat), j.seq, "{}–{} is annotated over the wrong bases", j.left, j.right);
    }
}

fn span_of(seq: &str, f: &Feature) -> String {
    if f.end >= f.start { seq[f.start..=f.end].to_string() } else { format!("{}{}", &seq[f.start..], &seq[..=f.end]) }
}

#[test]
fn a_fragment_split_by_the_size_cap_puts_every_piece_and_every_join_on_the_product() {
    // One PART arriving as three TUBES. Before this the product drew one feature for the part and
    // said nothing about the pieces, so the only place the split existed was a panel that vanished
    // on reload — while the pieces are the thing you are buying.
    let backbone = "ATGC".repeat(500);
    let long = "GGCCTTAA".repeat(150); // 1,200 bp, well past the cap below
    let mut input = multi(&backbone, true, region(100, 200), &[("payload", &long)]);
    input.target_tm = 60.0;
    input.max_eblock_len = 500;
    let res = design_gibson_multi(&input).expect("design");

    assert!(res.eblocks.len() > 1, "the cap should have split it, got {}", res.eblocks.len());
    let named_all: Vec<&str> = res.assembled.features.iter().map(|f| f.name.as_str()).collect();
    for eb in &res.eblocks {
        assert!(named_all.contains(&eb.name.as_str()), "piece {} is not on the product", eb.name);
    }
    // One join between each consecutive pair, named for the two it joins.
    let joins: Vec<&&str> = named_all.iter().filter(|n| n.starts_with("Overlap payload") && n.contains("–payload")).collect();
    assert_eq!(joins.len(), res.eblocks.len() - 1, "expected {} joins, got {joins:?}", res.eblocks.len() - 1);

    // AND THE JOIN SITS ON THE SHARED BASES.
    let seq = &res.assembled.sequence;
    for i in 1..res.eblocks.len() {
        let (prev, eb) = (&res.eblocks[i - 1], &res.eblocks[i]);
        let prefix = format!("Overlap {}–{} ", prev.name, eb.name);
        let feat = res
            .assembled
            .features
            .iter()
            .find(|f| f.name.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no join annotated between {} and {}", prev.name, eb.name));
        assert_eq!(span_of(seq, feat), eb.order[..(prev.end - eb.start) as usize], "the join is on the wrong bases");
    }
}

#[test]
fn a_fragment_that_fits_is_not_annotated_twice() {
    // With one piece per fragment the eBlock is the fragment plus its arms; drawing both would be
    // the same span annotated under two names, which reads as two things on the map.
    let backbone = "ATGC".repeat(400);
    let mut input = multi(&backbone, true, region(100, 200), &[("solo", &"GGCCTTAA".repeat(20))]);
    input.target_tm = 60.0;
    let res = design_gibson_multi(&input).expect("design");
    assert_eq!(res.eblocks.len(), 1, "this one should not split");
    assert_eq!(res.assembled.features.iter().filter(|f| f.name == "solo").count(), 1, "the part is named once and only once");
}

// ---------------------------------------------------------------------------
// Does the site survive ON THE PRODUCT? Everything above tests the intention —
// hold the boundary outside the recognition site. These read the plasmid back.
// ---------------------------------------------------------------------------

#[test]
fn blunt_five_prime_overhang_and_three_prime_overhang_cutters_all_survive_and_say_so() {
    // The three end chemistries, on one backbone: EcoRV cuts blunt (GAT^ATC), EcoRI leaves a 5′
    // overhang (G^AATTC), KpnI a 3′ one (GGTAC^C). Keeping the site clears the WHOLE recognition
    // sequence, so the rule cannot depend on where inside it the strands part.
    let bb = format!("{}GATATC{}GAATTC{}GGTACC{}", fill(300), fill(120), fill(120), fill(300));
    for [a, b] in [["EcoRV", "EcoRI"], ["EcoRI", "KpnI"], ["EcoRV", "KpnI"]] {
        let d = built(&bb, a, b, INS, true);
        let mut got: Vec<(String, usize)> = d.sites.iter().map(|s| (s.enzyme.clone(), s.on_product)).collect();
        let mut want = vec![(a.to_string(), 1usize), (b.to_string(), 1)];
        got.sort();
        want.sort();
        assert_eq!(got, want, "{a} + {b}: both sites should be on the product, once each");
        assert!(d.sites.iter().all(|s| s.note.is_none()), "{a} + {b} should have nothing to explain");
        let mut names = vec![a.to_string(), b.to_string()];
        names.sort();
        assert_eq!(d.cut.kept_sites.map(sorted), Some(names));
    }
}

#[test]
fn a_type_iis_site_planted_on_the_minus_strand_is_kept_in_both_arcs() {
    // BbsI reads GAAGAC and cuts six bases away, so planted as GTCTTC it reads back ACROSS its own
    // cut: the arc that leaves used to swallow the site whole, in silence, on half the pairs BUILD
    // offered. Both arcs, because "which arc leaves" was the other half of the coin flip.
    let bb = format!("{}GTCTTC{}GGATCC{}", fill(300), fill(120), fill(300));
    for arc in [OpeningArc::Between, OpeningArc::Origin] {
        let GibsonCut::Enzymes { enzymes, sites, keep_sites, .. } = cut_of(&bb, "BbsI", "BamHI", true, true) else {
            panic!("an enzyme cut");
        };
        let cut = GibsonCut::Enzymes { enzymes, sites, arc: Some(arc), keep_sites };
        let d = built_with(&bb, cut, INS, true);
        let mut got: Vec<(String, usize)> = d.sites.iter().map(|x| (x.enzyme.clone(), x.on_product)).collect();
        got.sort();
        assert_eq!(got, [("BamHI".to_string(), 1), ("BbsI".to_string(), 1)], "arc {arc:?}: both sites should be on the product");
        assert!(d.sites.iter().all(|x| x.note.is_none()), "arc {arc:?} should have nothing to explain");
    }
}

#[test]
fn two_enzymes_reading_the_same_dna_keep_both_sites_as_a_pure_insertion() {
    // Acc65I and KpnI are the same GGTACC read by two enzymes. The four bases "between the cuts"
    // are inside that site, so removing them destroyed both; the honest opening removes nothing.
    let bb = format!("{}GGTACC{}", fill(300), fill(300));
    let d = built(&bb, "Acc65I", "KpnI", INS, true);
    assert_eq!(d.sites.iter().map(|s| s.on_product).collect::<Vec<_>>(), [1, 1]);
    assert!(d.sites.iter().all(|s| s.note.is_none()));
    assert_eq!(d.cut.removed_len, 0);
    assert_eq!(d.assembled.sequence.len(), bb.len() + INS.len(), "a pure insertion");
    assert!(d.warnings.iter().any(|w| w.contains("overlapping DNA")), "{:?}", d.warnings);
}

#[test]
fn a_junction_that_would_spell_a_second_copy_of_a_site_is_walked_off_it() {
    // Seen on a real backbone opened with SnaBI + Bst1107I: at one base of margin the retained
    // TACGTA meets the insert as TACGTA·T·ACATCA, which contains a second GTATAC — both sites kept,
    // both whole, and the plasmid still cut twice. The margin beside a site is free, so the design
    // walks it until the join spells nothing, and the unique cutter stays unique.
    let pad = "GGTTAA".repeat(20);
    let bb = format!("{}TACGTAT{pad}GTATAC{pad}{}", fill(300), fill(300));
    assert_eq!(find_cut_sites(&bb, &["Bst1107I"], true).len(), 1, "one Bst1107I site to start with");
    let d = built(&bb, "SnaBI", "Bst1107I", &format!("ACATCA{INS}"), true);
    let mut got: Vec<(String, usize)> = d.sites.iter().map(|x| (x.enzyme.clone(), x.on_product)).collect();
    got.sort();
    assert_eq!(got, [("Bst1107I".to_string(), 1), ("SnaBI".to_string(), 1)]);
    assert!(d.sites.iter().all(|x| x.note.is_none()), "nothing left to explain");
    assert_eq!(d.cut.kept_sites.clone().map(sorted), Some(vec!["Bst1107I".to_string(), "SnaBI".to_string()]), "and both are still kept");

    // The margin it walked off is the one the plain resolver still uses, so this is the walk doing
    // the work and not the planted case quietly failing to reproduce.
    let cut = cut_of(&bb, "SnaBI", "Bst1107I", true, true);
    let naive = resolve_gibson_opening(bb.len() as i64, true, &cut, &mut Vec::new(), SITE_MARGIN).expect("opening");
    assert_ne!(d.cut.up, naive.up, "the boundary moved to spell something else");
}

#[test]
fn an_insert_carrying_the_site_is_named_as_the_reason_not_blamed_on_the_junction() {
    let bb = format!("{}GAATTC{}GGATCC{}", fill(300), fill(120), fill(300));
    let d = built(&bb, "EcoRI", "BamHI", &format!("{}GAATTC{}", fill(60), fill(60)), true);
    let eco = d.sites.iter().find(|s| s.enzyme == "EcoRI").expect("EcoRI");
    assert_eq!(eco.on_product, 2);
    assert!(eco.note.as_deref().unwrap_or_default().contains("inside the inserted DNA"), "{:?}", eco.note);
}

#[test]
fn a_region_opening_has_no_enzymes_so_there_is_nothing_to_survive() {
    let bb = fill(1200);
    let d = design_gibson_multi(&multi(&bb, true, region(300, 400), &[("GOI", INS)])).expect("design");
    assert_eq!(d.sites, vec![]);
    assert_eq!(check_site_survival(&d.assembled.sequence, true, &d.cut, &[INS]), vec![]);
}
