//! Parity with Bios `editor/overview/labels.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.
//!
//! Where the original leaves an argument out (`assignLanes(f, L)`, `mergeBands(items)`), this passes
//! `None`, which is what the original's parameter default amounts to. `LABEL_SPACING[d]` is
//! `label_spacing(d)`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bios_layout::overview::labels::{
    ColItem, LabelDensity, LiftItem, Placed, PlainSpan, Prioritised, RowItem, arc_available_px, assign_lanes, bar_available_px, cap_by_priority,
    coalesce_features, fit_label, flag_height, flag_pad_x, flag_width, group_cuts_by_position, join_enzymes, label_spacing, lanes_for_radius,
    merge_bands, pack_above_anchors, pack_column, pack_rows,
};

fn span(start: i64, end: i64, name: Option<&str>, kind: Option<&str>) -> PlainSpan {
    PlainSpan { start, end, name: name.map(str::to_owned), kind: kind.map(str::to_owned), ..PlainSpan::default() }
}

fn oriented(start: i64, end: i64, name: &str, kind: &str, forward: bool) -> PlainSpan {
    PlainSpan { forward: Some(forward), ..span(start, end, Some(name), Some(kind)) }
}

/// coalesceFeatures fuses a CDS split into abutting same-name pieces
#[test]
fn coalesce_features_fuses_a_cds_split_into_abutting_same_name_pieces() {
    // A real shape from a lab plasmid: one GFP written as three touching CDS segments.
    let out = coalesce_features(&[
        span(2318, 3031, Some("EGFP"), Some("CDS")),
        span(2315, 2317, Some("EGFP"), Some("CDS")),
        span(2312, 2314, Some("EGFP"), Some("CDS")),
    ]);
    assert_eq!(out.len(), 1);
    assert_eq!((out[0].start, out[0].end), (2312, 3031));
}

/// coalesceFeatures keeps same-name features that do not touch
#[test]
fn coalesce_features_keeps_same_name_features_that_do_not_touch() {
    // Both ITRs of an AAV vector are called "AAV2 ITR" and are genuinely two things.
    let out = coalesce_features(&[span(10, 150, Some("AAV2 ITR"), Some("repeat_region")), span(4000, 4140, Some("AAV2 ITR"), Some("repeat_region"))]);
    assert_eq!(out.len(), 2);
}

/// coalesceFeatures merges overlapping spans and spans that abut exactly
#[test]
fn coalesce_features_merges_overlapping_spans_and_spans_that_abut_exactly() {
    let abut = coalesce_features(&[span(100, 200, Some("AmpR"), Some("CDS")), span(201, 260, Some("AmpR"), Some("CDS"))]);
    assert_eq!((abut.len(), abut[0].end), (1, 260));

    let overlap = coalesce_features(&[span(100, 200, Some("AmpR"), Some("CDS")), span(150, 260, Some("AmpR"), Some("CDS"))]);
    assert_eq!((overlap.len(), overlap[0].end), (1, 260));

    let apart = coalesce_features(&[
        span(100, 200, Some("AmpR"), Some("CDS")),
        span(202, 260, Some("AmpR"), Some("CDS")), // one base of daylight — two features
    ]);
    assert_eq!(apart.len(), 2);
}

/// coalesceFeatures will not fuse across name, type or orientation
#[test]
fn coalesce_features_will_not_fuse_across_name_type_or_orientation() {
    let spans = [
        oriented(100, 200, "AmpR", "CDS", true),
        oriented(201, 260, "AmpR", "CDS", false),
        oriented(261, 300, "AmpR", "gene", true),
        oriented(301, 340, "AmpR promoter", "CDS", true),
    ];
    assert_eq!(coalesce_features(&spans).len(), 4);
}

/// coalesceFeatures preserves input order and passes wrapping spans through
#[test]
fn coalesce_features_preserves_input_order_and_passes_wrapping_spans_through() {
    let out = coalesce_features(&[
        span(500, 600, Some("ori"), None),
        span(6000, 100, Some("seam"), None), // wraps the origin — left alone
        span(601, 700, Some("ori"), None),
    ]);
    assert_eq!(out.iter().map(|f| f.name.as_deref()).collect::<Vec<_>>(), [Some("ori"), Some("seam")]);
    assert_eq!(out[0].end, 700);
}

/// coalesceFeatures leaves unnamed features alone
#[test]
fn coalesce_features_leaves_unnamed_features_alone() {
    let out = coalesce_features(&[span(10, 20, None, Some("misc")), span(21, 30, None, Some("misc"))]);
    assert_eq!(out.len(), 2);
}

/// groupCutsByPosition puts isoschizomers on one line
#[test]
fn group_cuts_by_position_puts_isoschizomers_on_one_line() {
    let groups = group_cuts_by_position(&[(33, "PaeR7I"), (0, "EcoRI"), (33, "PspXI"), (33, "XhoI"), (22, "BstXI")], None);
    assert_eq!(groups.iter().map(|g| g.pos).collect::<Vec<_>>(), [0, 22, 33]);
    assert_eq!(join_enzymes(&groups[2].enzymes), "PaeR7I - PspXI - XhoI");
    assert_eq!(groups[2].members.len(), 3);
}

/// groupCutsByPosition keeps neighbouring-but-distinct cuts apart by default
#[test]
fn group_cuts_by_position_keeps_neighbouring_but_distinct_cuts_apart_by_default() {
    let groups = group_cuts_by_position(&[(1340, "KasI"), (1341, "NarI")], None);
    assert_eq!(groups.len(), 2);
    assert_eq!(group_cuts_by_position(&[(1340, "KasI"), (1341, "NarI")], Some(1)).len(), 1);
}

// --- assignLanes ---

/// assignLanes puts the dominant feature on the backbone and nests the rest below it
#[test]
fn assign_lanes_puts_the_dominant_feature_on_the_backbone_and_nests_the_rest_below_it() {
    // A real cluster from a lab plasmid: CASSETTE 1 ⊃ CAG ⊃ chicken β-actin promoter, with
    // eblock1 and CMV enhancer threaded through them.
    let f = [
        span(718, 1569, Some("CASSETTE 1"), None),
        span(718, 1244, Some("CAG"), None),
        span(968, 1244, Some("chicken β-actin promoter"), None),
        span(658, 967, Some("eblock1"), None),
        span(718, 967, Some("CMV enhancer"), None),
    ];
    let lanes = assign_lanes(&f, 6085, None);
    let by: HashMap<&str, usize> = f.iter().zip(&lanes).map(|(x, lane)| (x.name.as_deref().unwrap(), *lane)).collect();
    assert_eq!(by["CASSETTE 1"], 0, "the widest feature earns the backbone");
    assert!(by["CAG"] > 0, "a feature nested inside it drops inward");
    assert!(by["chicken β-actin promoter"] > by["CAG"], "and its own child drops further");
    // Nothing overlapping may share a lane.
    for i in 0..f.len() {
        for j in i + 1..f.len() {
            if f[i].start <= f[j].end && f[j].start <= f[i].end {
                assert_ne!(lanes[i], lanes[j], "{:?} and {:?} overlap but share a lane", f[i].name, f[j].name);
            }
        }
    }
}

/// assignLanes keeps non-overlapping features on the backbone together
#[test]
fn assign_lanes_keeps_non_overlapping_features_on_the_backbone_together() {
    let lanes = assign_lanes(&[span(0, 100, None, None), span(200, 300, None, None), span(400, 500, None, None)], 6000, None);
    assert_eq!(lanes, [0, 0, 0], "a plasmid with no nesting stays one clean ring");
}

/// assignLanes lets a small feature reuse a lane it does not touch
#[test]
fn assign_lanes_lets_a_small_feature_reuse_a_lane_it_does_not_touch() {
    // big spans the lot; a and b are both inside it but disjoint from each other, so they share lane 1.
    let lanes = assign_lanes(&[span(0, 1000, Some("big"), None), span(10, 100, Some("a"), None), span(200, 300, Some("b"), None)], 6000, None);
    assert_eq!(lanes[0], 0);
    assert_eq!(lanes[1], lanes[2], "disjoint children share the same lane");
    assert_ne!(lanes[1], 0);
}

/// assignLanes handles a feature that wraps the origin
#[test]
fn assign_lanes_handles_a_feature_that_wraps_the_origin() {
    let lanes = assign_lanes(
        &[
            span(5900, 200, Some("wraps"), None), // crosses the origin
            span(50, 120, Some("inside the wrap"), None),
            span(3000, 3100, Some("far away"), None),
        ],
        6000,
        None,
    );
    assert_ne!(lanes[0], lanes[1], "the wrap really does overlap what it covers");
    assert_eq!(lanes[2], 0, "a feature the wrap never reaches keeps the backbone");
}

/// assignLanes never exceeds the lane cap
#[test]
fn assign_lanes_never_exceeds_the_lane_cap() {
    // Ten features all covering the same base would want ten lanes.
    let f: Vec<PlainSpan> = (0..10).map(|i| span(100 - i, 200 + i, None, None)).collect();
    let lanes = assign_lanes(&f, 6000, Some(4));
    let deepest = lanes.iter().copied().max().unwrap();
    assert!(deepest <= 3, "expected lanes capped at 3, got {deepest}");
    assert_eq!(lanes[f.len() - 1], 0, "the widest still takes the backbone");
}

#[derive(Clone, Debug, PartialEq)]
struct Ranked {
    id: &'static str,
    pri: f64,
}

impl Prioritised for Ranked {
    fn pri(&self) -> f64 {
        self.pri
    }
}

/// capByPriority keeps the best and preserves caller order
#[test]
fn cap_by_priority_keeps_the_best_and_preserves_caller_order() {
    let items =
        [Ranked { id: "cutA", pri: 0.0 }, Ranked { id: "feature", pri: 1e6 }, Ranked { id: "cutB", pri: 0.0 }, Ranked { id: "landmark", pri: 2e6 }];
    let kept = cap_by_priority(&items, 2);
    assert_eq!(kept.iter().map(|k| k.id).collect::<Vec<_>>(), ["feature", "landmark"], "order preserved, best kept");
    assert_eq!(cap_by_priority(&items, 10).len(), 4, "under the cap nothing is touched");
    assert_eq!(cap_by_priority(&items, 0).len(), 0);
}

const DENSITIES: [LabelDensity; 3] = [LabelDensity::Roomy, LabelDensity::Balanced, LabelDensity::Dense];

/// LABEL_SPACING gets tighter and admits more as density rises
#[test]
fn label_spacing_gets_tighter_and_admits_more_as_density_rises() {
    let order = DENSITIES;
    for i in 1..order.len() {
        let prev = label_spacing(order[i - 1]);
        let cur = label_spacing(order[i]);
        assert!(cur.feature_row < prev.feature_row, "{:?} should stack tighter than {:?}", order[i], order[i - 1]);
        assert!(cur.gap < prev.gap, "{:?} should use a smaller gap", order[i]);
        assert!(cur.max_labels > prev.max_labels, "{:?} should admit more labels", order[i]);
        assert!(cur.lane_step < prev.lane_step, "{:?} should nest its feature lanes tighter", order[i]);
        assert!(cur.lane_band < prev.lane_band, "{:?} should use a tighter backbone label band", order[i]);
    }
    // Enzyme names are smaller type than feature names at every density.
    for d in order {
        assert!(label_spacing(d).cut_row < label_spacing(d).feature_row);
        // The band off the backbone must clear a lane plus the type riding on it.
        assert!(label_spacing(d).lane_band > label_spacing(d).lane_step, "{d:?}: label band must exceed lane step");
    }
}

// --- packColumn ---

fn item_of(id: &str, y: f64, h: f64, pri: f64) -> ColItem {
    ColItem { id: id.to_owned(), y, h, pri, pinned: false }
}

/// `item(id, y)`: h = 14, pri = 1.
fn item(id: &str, y: f64) -> ColItem {
    item_of(id, y, 14.0, 1.0)
}

fn pinned(id: &str, y: f64, h: f64) -> ColItem {
    ColItem { id: id.to_owned(), y, h, pri: 0.0, pinned: true }
}

fn placed(id: &str, y: f64) -> Placed {
    Placed { id: id.to_owned(), y }
}

fn sorted_by_y(placed: &[Placed]) -> Vec<Placed> {
    let mut s = placed.to_vec();
    s.sort_by(|a, b| a.y.total_cmp(&b.y));
    s
}

fn no_overlap(placed: &[Placed], h: f64, gap: f64) {
    let s = sorted_by_y(placed);
    for i in 1..s.len() {
        assert!(s[i].y - s[i - 1].y >= h + gap - 1e-6, "\"{}\" and \"{}\" overlap: {} vs {}", s[i - 1].id, s[i].id, s[i - 1].y, s[i].y);
    }
}

/// packColumn leaves uncrowded labels exactly where they want to be
#[test]
fn pack_column_leaves_uncrowded_labels_exactly_where_they_want_to_be() {
    let placed_at = pack_column(&[item("a", 100.0), item("b", 300.0), item("c", 500.0)], 0.0, 800.0, Some(4.0));
    assert_eq!(sorted_by_y(&placed_at), [placed("a", 100.0), placed("b", 300.0), placed("c", 500.0)]);
}

/// packColumn spreads a crowd around its centre instead of only downward
#[test]
fn pack_column_spreads_a_crowd_around_its_centre_instead_of_only_downward() {
    // Seven cut sites within 55 bp of the origin — the fan at the top of a SnapGene map.
    let crowd: Vec<ColItem> = ["s0", "s1", "s2", "s3", "s4", "s5", "s6"].iter().enumerate().map(|(i, id)| item(id, 400.0 + i as f64 * 0.8)).collect();
    let placed = pack_column(&crowd, 0.0, 800.0, Some(4.0));
    assert_eq!(placed.len(), 7, "nothing is dropped — there is plenty of frame");
    no_overlap(&placed, 14.0, 4.0);
    // The block straddles its anchors rather than hanging below them.
    let ys: Vec<f64> = placed.iter().map(|p| p.y).collect();
    let mean = ys.iter().fold(0.0, |s, v| s + v) / ys.len() as f64;
    assert!((mean - 402.4).abs() < 1.0, "fan should centre on its anchors, got {mean}");
    let min = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(min < 400.0 && max > 402.0, "should spread both up and down");
}

/// packColumn keeps vertical order — a label never jumps past its neighbour
#[test]
fn pack_column_keeps_vertical_order_a_label_never_jumps_past_its_neighbour() {
    let placed = pack_column(&[item("top", 100.0), item("mid", 104.0), item("bot", 108.0)], 0.0, 800.0, Some(4.0));
    let order: Vec<String> = sorted_by_y(&placed).into_iter().map(|p| p.id).collect();
    assert_eq!(order, ["top", "mid", "bot"]);
}

/// packColumn clamps a fan to the frame instead of sailing off it
#[test]
fn pack_column_clamps_a_fan_to_the_frame_instead_of_sailing_off_it() {
    let crowd: Vec<ColItem> = (0..6).map(|i| item(&format!("c{i}"), 5.0 + i as f64 * 0.5)).collect();
    let placed = pack_column(&crowd, 0.0, 400.0, Some(4.0));
    assert_eq!(placed.len(), 6);
    no_overlap(&placed, 14.0, 4.0);
    for p in &placed {
        assert!(p.y - 7.0 >= -1e-6, "\"{}\" escaped the top of the frame at {}", p.id, p.y);
        assert!(p.y + 7.0 <= 400.0 + 1e-6, "\"{}\" escaped the bottom of the frame at {}", p.id, p.y);
    }
}

/// packColumn only drops when the frame is genuinely full, weakest first
#[test]
fn pack_column_only_drops_when_the_frame_is_genuinely_full_weakest_first() {
    // Frame fits 5 rows of 18 (14 + 4 gap); ask it to hold 8.
    let items: Vec<ColItem> = (0..8).map(|i| item_of(&format!("f{i}"), 50.0 + i as f64 * 2.0, 14.0, i as f64)).collect();
    let placed = pack_column(&items, 0.0, 90.0, Some(4.0));
    assert!(placed.len() >= 4 && placed.len() <= 5, "expected ~5 survivors, got {}", placed.len());
    no_overlap(&placed, 14.0, 4.0);
    let survivors: Vec<&str> = placed.iter().map(|p| p.id.as_str()).collect();
    assert!(survivors.contains(&"f7"), "highest priority must survive");
    assert!(!survivors.contains(&"f0"), "lowest priority is the first to go");
}

/// packColumn mixes label heights without overlapping
#[test]
fn pack_column_mixes_label_heights_without_overlapping() {
    // A feature name (taller) sharing a column with enzyme names (shorter) — the case that used to
    // print "eblock1" and "NotI" on top of each other.
    let placed = pack_column(
        &[item_of("eblock1", 300.0, 18.0, 5.0), item_of("NotI", 302.0, 14.0, 1.0), item_of("Bsp1407I", 305.0, 14.0, 1.0)],
        0.0,
        800.0,
        Some(3.0),
    );
    assert_eq!(placed.len(), 3);
    let s = sorted_by_y(&placed);
    let hs: HashMap<&str, f64> = HashMap::from([("eblock1", 18.0), ("NotI", 14.0), ("Bsp1407I", 14.0)]);
    for i in 1..s.len() {
        let need = hs[s[i - 1].id.as_str()] / 2.0 + hs[s[i].id.as_str()] / 2.0 + 3.0;
        assert!(s[i].y - s[i - 1].y >= need - 1e-6, "{}/{} too close", s[i - 1].id, s[i].id);
    }
}

/// packColumn handles the empty and single cases
#[test]
fn pack_column_handles_the_empty_and_single_cases() {
    assert_eq!(pack_column(&[], 0.0, 100.0, Some(4.0)), []);
    assert_eq!(pack_column(&[item("only", 50.0)], 0.0, 100.0, Some(4.0)), [placed("only", 50.0)]);
}

/// packColumn flows around a pinned obstacle without moving it
#[test]
fn pack_column_flows_around_a_pinned_obstacle_without_moving_it() {
    // The real case: "CASSETTE 1" is written along its own arc and cannot move; the column labels
    // beside it must space themselves off it instead of printing through it.
    let placed = pack_column(
        &[pinned("arc:CASSETTE 1", 500.0, 90.0), item_of("chicken β-actin promoter", 505.0, 16.0, 5.0), item_of("oligo hyb", 520.0, 16.0, 4.0)],
        0.0,
        900.0,
        Some(3.0),
    );
    let at: HashMap<&str, f64> = placed.iter().map(|p| (p.id.as_str(), p.y)).collect();
    assert_eq!(at["arc:CASSETTE 1"], 500.0, "the pinned obstacle must not be moved");
    for id in ["chicken β-actin promoter", "oligo hyb"] {
        let gap_from_arc = (at[id] - 500.0).abs();
        assert!(gap_from_arc >= 90.0 / 2.0 + 16.0 / 2.0, "\"{id}\" still sits inside the arc label's band");
    }
}

/// packColumn never sheds a pinned obstacle when the frame is full
#[test]
fn pack_column_never_sheds_a_pinned_obstacle_when_the_frame_is_full() {
    let mut items = vec![pinned("pin", 50.0, 40.0)];
    items.extend((0..6).map(|i| item_of(&format!("f{i}"), 50.0 + i as f64, 14.0, i as f64 + 1.0)));
    let placed = pack_column(&items, 0.0, 90.0, Some(3.0));
    assert!(placed.iter().any(|p| p.id == "pin"), "the pinned item survives");
    assert!(placed.len() < items.len(), "movable labels are shed instead");
}

// --- packRows ---

fn row_of(id: &str, x: f64, half: f64, pri: f64) -> RowItem {
    RowItem { id: id.to_owned(), x, half, pri }
}

/// `row(id, x)`: half = 20, pri = 1.
fn row(id: &str, x: f64) -> RowItem {
    row_of(id, x, 20.0, 1.0)
}

/// packRows keeps well-spaced labels all on the row nearest the strand
#[test]
fn pack_rows_keeps_well_spaced_labels_all_on_the_row_nearest_the_strand() {
    let placed = pack_rows(&[row("a", 100.0), row("b", 300.0), row("c", 500.0)], 6, Some(8.0));
    let mut rows: Vec<usize> = placed.iter().map(|p| p.row).collect();
    rows.sort_unstable();
    assert_eq!(rows, [0, 0, 0]);
}

/// packRows takes another row rather than lose a neighbour
#[test]
fn pack_rows_takes_another_row_rather_than_lose_a_neighbour() {
    // Three names centred 20px apart, each 40px wide — they cannot share a row.
    let placed = pack_rows(&[row("a", 100.0), row("b", 120.0), row("c", 140.0)], 6, Some(8.0));
    assert_eq!(placed.len(), 3, "nothing is dropped while rows remain");
    let rows: BTreeSet<usize> = placed.iter().map(|p| p.row).collect();
    assert_eq!(rows.into_iter().collect::<Vec<_>>(), [0, 1, 2]);
}

/// packRows never lets two labels overlap within a row
#[test]
fn pack_rows_never_lets_two_labels_overlap_within_a_row() {
    let items: Vec<RowItem> = (0..24).map(|i| row_of(&format!("r{i}"), 60.0 + i as f64 * 17.0, 22.0, i as f64)).collect();
    let placed = pack_rows(&items, 8, Some(8.0));
    let by_id: HashMap<&str, &RowItem> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut per_row: BTreeMap<usize, Vec<&RowItem>> = BTreeMap::new();
    for p in &placed {
        per_row.entry(p.row).or_default().push(by_id[p.id.as_str()]);
    }
    for (r, list) in per_row {
        let mut s = list;
        s.sort_by(|a, b| a.x.total_cmp(&b.x));
        for i in 1..s.len() {
            assert!(s[i].x - s[i].half >= s[i - 1].x + s[i - 1].half + 8.0 - 1e-9, "row {r}: \"{}\" and \"{}\" overlap", s[i - 1].id, s[i].id);
        }
    }
}

/// packRows gives the nearest row to the most useful label
#[test]
fn pack_rows_gives_the_nearest_row_to_the_most_useful_label() {
    // All three want the same spot; the highest priority should get row 0.
    let placed = pack_rows(&[row_of("weak", 100.0, 20.0, 1.0), row_of("strong", 100.0, 20.0, 9.0), row_of("mid", 100.0, 20.0, 5.0)], 6, Some(8.0));
    let row_of: HashMap<&str, usize> = placed.iter().map(|p| (p.id.as_str(), p.row)).collect();
    assert_eq!(row_of["strong"], 0);
    assert_eq!(row_of["mid"], 1);
    assert_eq!(row_of["weak"], 2);
}

/// packRows drops the least useful label when the stack hits the frame
#[test]
fn pack_rows_drops_the_least_useful_label_when_the_stack_hits_the_frame() {
    let items = [row_of("keep1", 100.0, 20.0, 9.0), row_of("keep2", 100.0, 20.0, 8.0), row_of("drop", 100.0, 20.0, 1.0)];
    let placed = pack_rows(&items, 2, Some(8.0));
    let mut ids: Vec<&str> = placed.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["keep1", "keep2"]);
}

/// packRows handles the empty and zero-row cases
#[test]
fn pack_rows_handles_the_empty_and_zero_row_cases() {
    assert_eq!(pack_rows(&[], 5, Some(8.0)), []);
    assert_eq!(pack_rows(&[row("a", 10.0)], 0, Some(8.0)), []);
}

// --- packAboveAnchors ---

fn lift_of(id: &str, x0: f64, x1: f64, y: f64, pri: f64) -> LiftItem {
    LiftItem { id: id.to_owned(), x0, x1, y, pri }
}

/// `lift(id, x0, x1, y)`: pri = 1.
fn lift(id: &str, x0: f64, x1: f64, y: f64) -> LiftItem {
    lift_of(id, x0, x1, y, 1.0)
}

/// `Math.round`, which sends a half up.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// packAboveAnchors leaves a label alone when nothing is in its way
#[test]
fn pack_above_anchors_leaves_a_label_alone_when_nothing_is_in_its_way() {
    let placed = pack_above_anchors(&[lift("a", 0.0, 40.0, 500.0), lift("b", 200.0, 240.0, 480.0)], 0.0, 15.0, Some(8.0));
    let at: HashMap<&str, f64> = placed.iter().map(|p| (p.id.as_str(), p.y)).collect();
    assert_eq!(at["a"], 500.0);
    assert_eq!(at["b"], 480.0);
}

/// packAboveAnchors separates labels whose anchors sit at different heights
#[test]
fn pack_above_anchors_separates_labels_whose_anchors_sit_at_different_heights() {
    // The regression: on a curved arc, "row 0 at a low point" and "row 2 at a high point" used to
    // land 3px apart. Overlapping x, near-identical y — must end up a full step apart.
    let placed = pack_above_anchors(&[lift_of("bGH R", 424.0, 458.0, 457.0, 5.0), lift_of("U", 430.0, 438.0, 460.0, 1.0)], 0.0, 15.0, Some(8.0));
    assert_eq!(placed.len(), 2);
    let mut ys: Vec<f64> = placed.iter().map(|p| p.y).collect();
    ys.sort_by(f64::total_cmp);
    let (a, b) = (ys[0], ys[1]);
    assert!(b - a >= 15.0, "expected a full step of separation, got {}", b - a);
}

/// packAboveAnchors never leaves two labels overlapping in both axes
#[test]
fn pack_above_anchors_never_leaves_two_labels_overlapping_in_both_axes() {
    // A shallow arc: anchors drift in y while names crowd in x.
    let items: Vec<LiftItem> = (0..30)
        .map(|i| {
            let n = i as f64;
            lift_of(&format!("n{i}"), 400.0 + n * 9.0, 400.0 + n * 9.0 + 60.0, 600.0 - js_round((n / 4.0).sin() * 40.0), n)
        })
        .collect();
    let placed = pack_above_anchors(&items, 0.0, 15.0, Some(8.0));
    let by_id: HashMap<&str, &LiftItem> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    for i in 0..placed.len() {
        for j in i + 1..placed.len() {
            let a = by_id[placed[i].id.as_str()];
            let b = by_id[placed[j].id.as_str()];
            let x_overlap = a.x1 + 8.0 > b.x0 && a.x0 < b.x1 + 8.0;
            if x_overlap {
                let apart = (placed[i].y - placed[j].y).abs();
                assert!(apart >= 15.0, "\"{}\" and \"{}\" overlap in x and sit {apart}px apart", a.id, b.id);
            }
        }
    }
}

/// packAboveAnchors reproduces plain row stacking when every anchor shares a height
#[test]
fn pack_above_anchors_reproduces_plain_row_stacking_when_every_anchor_shares_a_height() {
    // This is the t→1 seam with the linear view — it must land on ordinary rows.
    let placed = pack_above_anchors(
        &[lift_of("a", 100.0, 200.0, 500.0, 3.0), lift_of("b", 150.0, 250.0, 500.0, 2.0), lift_of("c", 180.0, 280.0, 500.0, 1.0)],
        0.0,
        15.0,
        Some(8.0),
    );
    let mut ys: Vec<f64> = placed.iter().map(|p| p.y).collect();
    ys.sort_by(|x, y| y.total_cmp(x));
    assert_eq!(ys, [500.0, 485.0, 470.0]);
}

/// packAboveAnchors keeps the best label nearest its anchor
#[test]
fn pack_above_anchors_keeps_the_best_label_nearest_its_anchor() {
    let placed = pack_above_anchors(&[lift_of("weak", 100.0, 200.0, 500.0, 1.0), lift_of("strong", 100.0, 200.0, 500.0, 9.0)], 0.0, 15.0, Some(8.0));
    let at: HashMap<&str, f64> = placed.iter().map(|p| (p.id.as_str(), p.y)).collect();
    assert_eq!(at["strong"], 500.0);
    assert_eq!(at["weak"], 485.0);
}

/// packAboveAnchors drops a label with nowhere left to go
#[test]
fn pack_above_anchors_drops_a_label_with_nowhere_left_to_go() {
    // Frame only has room for two steps above 470.
    let items = [lift_of("a", 0.0, 100.0, 500.0, 3.0), lift_of("b", 0.0, 100.0, 500.0, 2.0), lift_of("c", 0.0, 100.0, 500.0, 1.0)];
    let placed = pack_above_anchors(&items, 480.0, 15.0, Some(8.0));
    let mut ids: Vec<&str> = placed.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["a", "b"]);
}

// ---------------------------------------------------------------------------
// fitLabel — the rule that lets a name live inside its own shape.
//
// A fixed fake measure, not a real font: these assert the ALGORITHM (does it find the longest
// fitting prefix, does the result actually fit, does it refuse to print nonsense), which is what can
// regress silently. How wide the interface font is at 11px is the drawing crate's business.
//
// `s.length` in the original counts UTF-16 units, so this does too.
fn fake(s: &str, px: f64) -> f64 {
    s.encode_utf16().count() as f64 * px * 0.55
}

/// `W(s)`: px = 10.
fn w(s: &str) -> f64 {
    fake(s, 10.0)
}

/// fitLabel keeps the whole name when it fits
#[test]
fn fit_label_keeps_the_whole_name_when_it_fits() {
    assert_eq!(fit_label("EGFP", w("EGFP") + 1.0, 10.0, fake).as_deref(), Some("EGFP"));
}

/// fitLabel keeps the whole name at the exact boundary, and truncates one px under it
#[test]
fn fit_label_keeps_the_whole_name_at_the_exact_boundary_and_truncates_one_px_under_it() {
    let need = w("CASSETTE 1");
    assert_eq!(fit_label("CASSETTE 1", need, 10.0, fake).as_deref(), Some("CASSETTE 1"));
    let short = fit_label("CASSETTE 1", need - 0.01, 10.0, fake);
    assert_ne!(short.as_deref(), Some("CASSETTE 1"));
    assert!(short.is_some_and(|s| s.ends_with("..")));
}

/// fitLabel truncates with two dots that overwrite rather than extend
#[test]
fn fit_label_truncates_with_two_dots_that_overwrite_rather_than_extend() {
    // Room for 8 chars: the answer is 8 chars, not 8 + "..".
    let out = fit_label("chicken β-actin promoter", w("12345678"), 10.0, fake);
    assert_eq!(out.as_deref(), Some("chicke.."));
    assert_eq!(out.unwrap().encode_utf16().count(), 8);
}

/// fitLabel never returns something wider than the room it was given
#[test]
fn fit_label_never_returns_something_wider_than_the_room_it_was_given() {
    // The invariant every caller depends on to keep text inside its shape.
    let names =
        ["chicken β-actin promoter", "CASSETTE 1", "SV40 poly(A) signal", "WPRE optional", "AmpR promoter", "gibson homology", "Wilm", "illi"];
    for name in names {
        let mut room = 1.0;
        while room <= w(name) + 4.0 {
            if let Some(out) = fit_label(name, room, 10.0, fake) {
                assert!(fake(&out, 10.0) <= room, "\"{name}\" at room {room} produced \"{out}\" ({}px)", fake(&out, 10.0));
            }
            room += 0.5;
        }
    }
}

/// fitLabel refuses to print a stub of three characters or fewer
#[test]
fn fit_label_refuses_to_print_a_stub_of_three_characters_or_fewer() {
    assert_eq!(fit_label("chicken β-actin promoter", w("abc"), 10.0, fake), None);
    assert_eq!(fit_label("EGFP", 0.0, 10.0, fake), None);
    assert_eq!(fit_label("EGFP", -5.0, 10.0, fake), None);
    assert_eq!(fit_label("", 500.0, 10.0, fake), None);
}

/// fitLabel is monotone: more room never yields a shorter name
#[test]
fn fit_label_is_monotone_more_room_never_yields_a_shorter_name() {
    let name = "SV40 poly(A) signal";
    let mut prev = 0;
    let mut room = 1.0;
    while room <= w(name) + 4.0 {
        let out = fit_label(name, room, 10.0, fake);
        let len = out.map_or(0, |s| s.encode_utf16().count());
        assert!(len >= prev, "room {room}: {len} < {prev}");
        prev = len;
        room += 0.5;
    }
}

/// fitLabel survives a non-ASCII name (the two-dot slice must not split a character)
#[test]
fn fit_label_survives_a_non_ascii_name_the_two_dot_slice_must_not_split_a_character() {
    let out = fit_label("β-actin promoter", w("1234567"), 10.0, fake);
    assert!(out.as_deref().is_some_and(|s| s.ends_with("..")));
    assert!(fake(&out.unwrap(), 10.0) <= w("1234567"));
}

/// arcAvailablePx measures a sweep, less air at both ends
#[test]
fn arc_available_px_measures_a_sweep_less_air_at_both_ends() {
    // A quarter turn at r=100 is ~157px of arc; 10px type reserves 8px per side.
    assert_eq!(js_round(arc_available_px(0.25, 100.0, 10.0)), js_round(0.25 * std::f64::consts::PI * 2.0 * 100.0 - 16.0));
    // A feature too small to hold any type reports negative room, which fitLabel rejects.
    assert!(arc_available_px(0.001, 100.0, 10.0) < 0.0);
    assert_eq!(fit_label("U", arc_available_px(0.001, 100.0, 10.0), 10.0, fake), None);
}

/// arcAvailablePx is the same for a sweep whether or not it crosses the origin
#[test]
fn arc_available_px_is_the_same_for_a_sweep_whether_or_not_it_crosses_the_origin() {
    // Callers hand it a widthFrac, so a feature spanning 0.9 -> 0.05 (widthFrac 0.15) must measure
    // identically to one spanning 0.2 -> 0.35. The seam is the caller's problem, not this function's.
    assert_eq!(arc_available_px(0.15, 120.0, 11.0), arc_available_px(0.15, 120.0, 11.0));
}

/// barAvailablePx measures a bar, less air at both ends
#[test]
fn bar_available_px_measures_a_bar_less_air_at_both_ends() {
    assert_eq!(bar_available_px(100.0, 200.0, 10.0), 100.0 - 16.0);
    assert!(bar_available_px(100.0, 104.0, 10.0) < 0.0);
}

/// lane geometry stays self-consistent across densities
#[test]
fn lane_geometry_stays_self_consistent_across_densities() {
    // These four numbers have to keep a relationship, not just exist: a lane must be thinner than the
    // gap between lanes (or lanes touch), and its type must fit inside its thickness with air. Nothing
    // else checks this, and it is exactly the kind of constant that rots during a retune.
    for d in DENSITIES {
        let sp = label_spacing(d);
        assert!(sp.lane_thickness < sp.lane_step, "{d:?}: lane {} >= step {}", sp.lane_thickness, sp.lane_step);
        // Cap height is ~0.72em, and a name centred in a band wants ~2px of air above and below it.
        assert!(
            sp.lane_font * 0.72 + 4.0 <= sp.lane_thickness,
            "{d:?}: {}px type does not sit in a {}px lane with air",
            sp.lane_font,
            sp.lane_thickness
        );
        // ...and the band must not be so tall it reads as a slab rather than a line.
        assert!(sp.lane_thickness <= sp.lane_font * 1.5, "{d:?}: a {}px band around {}px type is a slab", sp.lane_thickness, sp.lane_font);
        assert!(sp.lane_band >= sp.lane_step, "{d:?}: first step in should not be tighter than the rest");
    }
}

/// lanesForRadius refuses to draw a lane that would invert through the centre
#[test]
fn lanes_for_radius_refuses_to_draw_a_lane_that_would_invert_through_the_centre() {
    let sp = label_spacing(LabelDensity::Roomy);
    // The measured failure case: the real map pane leaves R0 = 142px.
    let max = lanes_for_radius(142.0, sp, None);
    // Every lane it allows must sit at a radius that can actually hold an arc.
    for lane in 0..=max {
        let r = 142.0 - if lane == 0 { 0.0 } else { sp.lane_band + (lane - 1) as f64 * sp.lane_step };
        assert!(r > sp.lane_thickness, "lane {lane} at r={r} is not drawable");
    }
    // And the first lane it refuses must genuinely not fit.
    let beyond = 142.0 - (sp.lane_band + max as f64 * sp.lane_step);
    assert!(beyond <= 34.0, "lane {} at r={beyond} should have been allowed", max + 1);
}

/// lanesForRadius collapses to the backbone on a ring with no room to nest
#[test]
fn lanes_for_radius_collapses_to_the_backbone_on_a_ring_with_no_room_to_nest() {
    assert_eq!(lanes_for_radius(30.0, label_spacing(LabelDensity::Roomy), None), 0);
    assert_eq!(lanes_for_radius(0.0, label_spacing(LabelDensity::Roomy), None), 0);
    assert_eq!(lanes_for_radius(-10.0, label_spacing(LabelDensity::Roomy), None), 0);
}

/// lanesForRadius allows more lanes as the ring grows, and never fewer
#[test]
fn lanes_for_radius_allows_more_lanes_as_the_ring_grows_and_never_fewer() {
    let mut prev = 0;
    for r in (0..=600).step_by(5) {
        let n = lanes_for_radius(f64::from(r), label_spacing(LabelDensity::Roomy), None);
        assert!(n >= prev, "r={r}: {n} < {prev}");
        prev = n;
    }
    // A big ring should comfortably reach the assignLanes ceiling of 6.
    assert!(lanes_for_radius(400.0, label_spacing(LabelDensity::Roomy), None) >= 5);
}

/// a denser setting fits more lanes into the same radius
#[test]
fn a_denser_setting_fits_more_lanes_into_the_same_radius() {
    let r = 200.0;
    assert!(lanes_for_radius(r, label_spacing(LabelDensity::Dense), None) >= lanes_for_radius(r, label_spacing(LabelDensity::Balanced), None));
    assert!(lanes_for_radius(r, label_spacing(LabelDensity::Balanced), None) >= lanes_for_radius(r, label_spacing(LabelDensity::Roomy), None));
}

/// fitLabel refuses a stub too short to recognise the name from
#[test]
fn fit_label_refuses_a_stub_too_short_to_recognise_the_name_from() {
    // `ch..` for `chicken β-actin promoter` passes a naive four-character floor (the ellipsis counts)
    // and tells a reader nothing. Four characters of the real NAME is the bar.
    assert_eq!(fit_label("chicken β-actin promoter", w("ch.."), 10.0, fake), None);
    assert_eq!(fit_label("bGH poly A", w("bG.."), 10.0, fake), None);
    assert_eq!(fit_label("eblock1", w("eblo.."), 10.0, fake).as_deref(), Some("eblo.."));
    assert_eq!(fit_label("WPRE optional", w("WPRE o.."), 10.0, fake).as_deref(), Some("WPRE o.."));
}

/// fitLabel prefers silence to a truncation that saves nothing
#[test]
fn fit_label_prefers_silence_to_a_truncation_that_saves_nothing() {
    // `f1 ori` -> `f1 ..` is one character narrower and strictly less informative.
    assert_eq!(fit_label("f1 ori", w("f1 .."), 10.0, fake), None);
    // But a real saving is worth the dots.
    assert_eq!(fit_label("cassette 2", w("casse.."), 10.0, fake).as_deref(), Some("casse.."));
}

/// fitLabel does not leave a trailing space before the dots
#[test]
fn fit_label_does_not_leave_a_trailing_space_before_the_dots() {
    let out = fit_label("WPRE optional", w("WPRE o.."), 10.0, fake);
    assert_eq!(out.as_deref(), Some("WPRE o.."));
    // The cut landing exactly on the space must lose it, not print `WPRE ..`.
    let tight = fit_label("WPRE optional", w("WPRE .."), 10.0, fake);
    assert!(tight.as_deref().is_none_or(|t| !t.ends_with(" ..")), "got \"{tight:?}\"");
}

/// flagWidth leaves the text room to breathe on both sides
#[test]
fn flag_width_leaves_the_text_room_to_breathe_on_both_sides() {
    // The invariant the chip is drawn to: the box is the text plus equal air either side, so the
    // renderer can centre the name in it and be sure nothing touches the border.
    let text_px = 60.0;
    let font = 11.0;
    assert_eq!(flag_width(text_px, font), text_px + 2.0 * flag_pad_x(font));
    assert!(flag_width(text_px, font) > text_px);
}

/// a flag's box grows with its type, never shrinks below it
#[test]
fn a_flags_box_grows_with_its_type_never_shrinks_below_it() {
    // A box shorter than its own font would clip the letters — and `dense` is where that would bite,
    // because it is the density that shrinks the type furthest.
    for font in [8.5, 9.5, 10.0, 11.0, 12.0] {
        assert!(flag_height(font) > font, "flagHeight({font}) must clear the type");
    }
    assert!(flag_height(11.0) > flag_height(9.5));
}

/// flag chrome scales with the type, so the chip keeps its shape at every density
#[test]
fn flag_chrome_scales_with_the_type_so_the_chip_keeps_its_shape_at_every_density() {
    // Fixed padding would make the roomy chip look right and turn the dense one into a slab. The
    // ratio of box to text should stay roughly constant instead.
    let ratio = |font: f64| flag_height(font) / font;
    let roomy = ratio(label_spacing(LabelDensity::Roomy).lane_font);
    let dense = ratio(f64::max(9.5, label_spacing(LabelDensity::Dense).lane_font));
    assert!((roomy - dense).abs() < 0.25, "chip proportions drifted: {roomy} vs {dense}");
}

/// a flag is wider than the same name written bare, and that width is what gets packed
#[test]
fn a_flag_is_wider_than_the_same_name_written_bare_and_that_width_is_what_gets_packed() {
    // Why `w` exists on the candidate: a column that aligned on the TEXT width would let every chip's
    // border hang past the frame edge it was clamped to.
    let measure = |t: &str, px: f64| t.encode_utf16().count() as f64 * px * 0.5;
    let name = "chicken β-actin promoter";
    let font = 11.0;
    let bare = measure(name, font);
    assert!(flag_width(bare, font) > bare);
}

/// mergeBands fuses arc names that cover the same stretch of column
#[test]
fn merge_bands_fuses_arc_names_that_cover_the_same_stretch_of_column() {
    // Three nested lanes whose names all cross 3 o'clock. They cover one band between them, and
    // counting them separately is what told packColumn the frame was full when it was not.
    let merged = merge_bands(&[pinned("a", 100.0, 60.0), pinned("b", 120.0, 60.0), pinned("c", 130.0, 60.0)], None);
    assert_eq!(merged.len(), 1);
    assert_eq!((merged[0].y, merged[0].h), (115.0, 90.0)); // union of 70..160
    assert!(merged[0].pinned, "the fused band is still an immovable obstacle");
}

/// mergeBands leaves genuinely separate bands alone
#[test]
fn merge_bands_leaves_genuinely_separate_bands_alone() {
    let merged = merge_bands(&[pinned("a", 100.0, 20.0), pinned("b", 400.0, 20.0)], None);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged.iter().map(|m| m.y).collect::<Vec<_>>(), [100.0, 400.0]);
}

/// merging bands stops packColumn shedding labels that had room
#[test]
fn merging_bands_stops_pack_column_shedding_labels_that_had_room() {
    // The ring bug, reduced: three overlapping arc-name obstacles totalling 180px of claimed height
    // in a 200px frame, plus two labels. Summed naively the column "overflows" and sheds them.
    let obstacles = [pinned("o1", 100.0, 60.0), pinned("o2", 110.0, 60.0), pinned("o3", 120.0, 60.0)];
    let labels = [item_of("L1", 30.0, 20.0, 50.0), item_of("L2", 180.0, 20.0, 40.0)];
    let naive = pack_column(&[&labels[..], &obstacles[..]].concat(), 0.0, 200.0, Some(0.0));
    let fused = pack_column(&[&labels[..], &merge_bands(&obstacles, None)[..]].concat(), 0.0, 200.0, Some(0.0));
    assert!(naive.iter().filter(|p| p.id.starts_with('L')).count() < 2, "precondition: the naive sum sheds a label");
    assert_eq!(fused.iter().filter(|p| p.id.starts_with('L')).count(), 2, "both labels fit once the obstacles are counted as the one band they are");
}
