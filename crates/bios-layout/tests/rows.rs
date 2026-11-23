//! Parity with Bios `editor/rows/layout.test.ts`: one test here per test there, in the same order,
//! with the same assertions and the same numbers.
//!
//! Where the original leaves an argument out (`rowHeight()`, `laneBlockHeight()`), this passes the
//! type's `Default`, which is what the original's `= {}` parameter default amounts to.

use bios_layout::rows::{
    DEFAULT_CHAR_WIDTH, GROUP, Lanes, MIN_MARGIN, PACK_GAP, Row, Rule, StrandLanes, Tracks, above_air, bands_for, base_at_x, caret_at_x,
    center_of_base, cut_bands_for, lane_block_height, metrics_for, pack_lanes, plan_rows, regime_for, row_at, row_height, row_of_base, rows_for,
    rules_for, span_in_row, track, unwrap, x_of_base,
};

// The panel Bios actually hands the rows: 1440 window, chat and rails taken out.
const PANEL: f64 = 900.0;

/// `rowHeight()`: no lanes, default tracks.
fn bare_row_height() -> f64 {
    row_height(Lanes::default(), Tracks::default())
}

/// `rowHeight(lanes)`: default tracks.
fn row_height_of(lanes: Lanes) -> f64 {
    row_height(lanes, Tracks::default())
}

/// a row holds a whole number of tens, so it ends on a clean number
#[test]
fn a_row_holds_a_whole_number_of_tens_so_it_ends_on_a_clean_number() {
    for width in [400.0, 640.0, 900.0, 1100.0, 1440.0, 2560.0] {
        let m = metrics_for(width, None, None);
        assert_eq!(m.bps_per_row % GROUP, 0, "{width}px gave {} bases", m.bps_per_row);
    }
}

/// the row justifies: bases per row times character width is exactly the content width
#[test]
fn the_row_justifies_bases_per_row_times_character_width_is_exactly_the_content_width() {
    // This is the property the whole coordinate contract rests on. If it drifts, the last base of
    // every row hangs off the panel or leaves a gutter, and every layer inherits the error.
    for width in [400.0, 640.0, 900.0, 1100.0, 1440.0, 2560.0] {
        let m = metrics_for(width, None, None);
        assert!(
            (m.bps_per_row as f64 * m.char_width - m.content_width).abs() < 1e-9,
            "{width}px: {} × {} ≠ {}",
            m.bps_per_row,
            m.char_width,
            m.content_width
        );
    }
}

/// a wider panel never holds fewer bases
#[test]
fn a_wider_panel_never_holds_fewer_bases() {
    let mut last = 0;
    let mut width = 200.0;
    while width <= 3000.0 {
        let bps_per_row = metrics_for(width, None, None).bps_per_row;
        assert!(bps_per_row >= last, "{width}px dropped from {last} to {bps_per_row}");
        last = bps_per_row;
        width += 37.0;
    }
}

/// a panel too narrow for ten bases still gets ten, because a row of none is not a row
#[test]
fn a_panel_too_narrow_for_ten_bases_still_gets_ten_because_a_row_of_none_is_not_a_row() {
    // Everything downstream divides by this. Returning 0 here is a division by zero in the renderer.
    for width in [0.0, 10.0, MIN_MARGIN, MIN_MARGIN + 1.0, 120.0] {
        assert_eq!(metrics_for(width, None, None).bps_per_row, GROUP, "{width}px");
    }
}

/// the margin is never squeezed below what the row numbers need
#[test]
fn the_margin_is_never_squeezed_below_what_the_row_numbers_need() {
    assert_eq!(metrics_for(PANEL, None, Some(0.0)).margin_width, MIN_MARGIN);
    assert_eq!(metrics_for(PANEL, None, Some(200.0)).margin_width, 200.0);
}

/// rounding to ten can round up, and the stretch absorbs it rather than overflowing
#[test]
fn rounding_to_ten_can_round_up_and_the_stretch_absorbs_it_rather_than_overflowing() {
    // 75 bases fit at 16px in this box, which rounds UP to 80 — a row now claims five bases the
    // panel cannot show at the asked-for size. The justified slot must shrink to 15px so the 80
    // still span the content exactly. This is the case that would push bases off the panel edge.
    let width = MIN_MARGIN + 75.0 * DEFAULT_CHAR_WIDTH;
    let m = metrics_for(width, None, Some(MIN_MARGIN));
    assert_eq!(m.bps_per_row, 80, "75 should snap up to 80");
    assert!(m.char_width < DEFAULT_CHAR_WIDTH, "the slot did not shrink");
    assert_eq!(m.char_width, 15.0);
    assert!((m.bps_per_row as f64 * m.char_width - m.content_width).abs() < 1e-9);
}

/// the rows cover every base exactly once, and the last one is short
#[test]
fn the_rows_cover_every_base_exactly_once_and_the_last_one_is_short() {
    let len = 6093;
    let bps_per_row = metrics_for(PANEL, None, None).bps_per_row;
    let rows = rows_for(len, bps_per_row);
    assert_eq!(rows[0].start, 0);
    assert_eq!(rows[rows.len() - 1].end, len - 1, "the last row must end on the last base");
    for i in 1..rows.len() {
        assert_eq!(rows[i].start, rows[i - 1].end + 1, "row {i} does not carry on from row {}", i - 1);
    }
    let covered = rows.iter().fold(0, |n, r| n + (r.end - r.start + 1));
    assert_eq!(covered, len);
}

/// an empty construct still has one row, holding no bases
#[test]
fn an_empty_construct_still_has_one_row_holding_no_bases() {
    let rows = rows_for(0, 60);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].start, 0);
    assert_eq!(rows[0].end, -1, "a row that holds nothing ends before it starts");
}

/// a construct shorter than one row is one row
#[test]
fn a_construct_shorter_than_one_row_is_one_row() {
    let rows = rows_for(12, 60);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], Row { index: 0, start: 0, end: 11 });
}

/// a construct exactly one row long does not get an empty second row
#[test]
fn a_construct_exactly_one_row_long_does_not_get_an_empty_second_row() {
    let rows = rows_for(60, 60);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].end, 59);
}

/// a base knows which row it is in
#[test]
fn a_base_knows_which_row_it_is_in() {
    assert_eq!(row_of_base(0, 60), 0);
    assert_eq!(row_of_base(59, 60), 0);
    assert_eq!(row_of_base(60, 60), 1);
    assert_eq!(row_of_base(6092, 60), 101);
}

/// x is measured from the row's own first base, not from the sequence's
#[test]
fn x_is_measured_from_the_rows_own_first_base_not_from_the_sequences() {
    let row = Row { index: 3, start: 180, end: 239 };
    assert_eq!(x_of_base(180, row, 10.0), 0.0);
    assert_eq!(x_of_base(185, row, 10.0), 50.0);
    assert_eq!(center_of_base(180, row, 10.0), 5.0, "a tick belongs in the middle of its slot");
}

/// the caret snaps to the nearest gap between bases, not the nearest base
#[test]
fn the_caret_snaps_to_the_nearest_gap_between_bases_not_the_nearest_base() {
    let row = Row { index: 0, start: 0, end: 59 };
    // Left half of base 3 → the gap before it. Right half → the gap after it.
    assert_eq!(caret_at_x(30.0 + 1.0, row, 10.0), 3);
    assert_eq!(caret_at_x(30.0 + 9.0, row, 10.0), 4);
    assert_eq!(caret_at_x(0.0, row, 10.0), 0);
}

/// clicking left of a row lands on its first base
#[test]
fn clicking_left_of_a_row_lands_on_its_first_base() {
    let row = Row { index: 2, start: 120, end: 179 };
    assert_eq!(caret_at_x(-40.0, row, 10.0), 120);
}

/// clicking past the end of a row lands one past its last base, never in the next row
#[test]
fn clicking_past_the_end_of_a_row_lands_one_past_its_last_base_never_in_the_next_row() {
    // The gap after a row's last base and the gap before the next row's first are the same place,
    // and the row you clicked in owns it. Without the clamp, a click in the right margin jumps a row.
    let row = Row { index: 0, start: 0, end: 59 };
    assert_eq!(caret_at_x(10_000.0, row, 10.0), 60);
    assert_eq!(caret_at_x(600.0, row, 10.0), 60);
}

/// caret positions round-trip: every base's left edge gives back that base
#[test]
fn caret_positions_round_trip_every_bases_left_edge_gives_back_that_base() {
    let row = Row { index: 5, start: 300, end: 359 };
    let cw = 13.7;
    for bp in row.start..=row.end {
        assert_eq!(caret_at_x(x_of_base(bp, row, cw), row, cw), bp, "bp {bp}");
    }
}

/// hovering gives the base under the pointer, and nothing past the row's end
#[test]
fn hovering_gives_the_base_under_the_pointer_and_nothing_past_the_rows_end() {
    let row = Row { index: 0, start: 0, end: 59 };
    assert_eq!(base_at_x(0.0, row, 10.0), Some(0));
    assert_eq!(base_at_x(9.9, row, 10.0), Some(0), "still inside the first slot");
    assert_eq!(base_at_x(10.0, row, 10.0), Some(1));
    assert_eq!(base_at_x(600.0, row, 10.0), None, "past the last base there is no base");
    assert_eq!(base_at_x(-1.0, row, 10.0), None);
}

/// a span is clipped to the row, and says which edges it ran past
#[test]
fn a_span_is_clipped_to_the_row_and_says_which_edges_it_ran_past() {
    let row = Row { index: 1, start: 60, end: 119 };
    let s = span_in_row(30, 200, row, 10.0);
    assert!(s.is_some(), "the span missed the row");
    let Some(s) = s else { return };
    assert_eq!(s.start, 60);
    assert_eq!(s.end, 119);
    assert_eq!(s.x_start, 0.0);
    assert_eq!(s.width, 600.0);
    assert!(s.clipped_left && s.clipped_right, "a feature crossing the whole row is cut at both ends");
}

/// a span wholly inside a row is capped at both ends
#[test]
fn a_span_wholly_inside_a_row_is_capped_at_both_ends() {
    let row = Row { index: 1, start: 60, end: 119 };
    let s = span_in_row(70, 79, row, 10.0);
    assert!(s.is_some(), "the span missed the row");
    let Some(s) = s else { return };
    assert_eq!(s.x_start, 100.0);
    assert_eq!(s.width, 100.0);
    assert!(!s.clipped_left);
    assert!(!s.clipped_right);
}

/// a span that misses the row is not drawn at all
#[test]
fn a_span_that_misses_the_row_is_not_drawn_at_all() {
    let row = Row { index: 1, start: 60, end: 119 };
    assert_eq!(span_in_row(0, 59, row, 10.0), None);
    assert_eq!(span_in_row(120, 200, row, 10.0), None);
}

/// a single base is a span one slot wide, not a zero-width line
#[test]
fn a_single_base_is_a_span_one_slot_wide_not_a_zero_width_line() {
    let row = Row { index: 0, start: 0, end: 59 };
    let s = span_in_row(7, 7, row, 10.0);
    assert!(s.is_some(), "the span missed the row");
    let Some(s) = s else { return };
    assert_eq!(s.width, 10.0);
}

/// an origin-spanning feature is read twice: at the end, and at the beginning
#[test]
fn an_origin_spanning_feature_is_read_twice_at_the_end_and_at_the_beginning() {
    // A row view has no origin — it is a straight read from top to bottom — so a feature that
    // crosses the seam genuinely appears in the last row and again in the first.
    assert_eq!(unwrap(6080, 40, 6093), vec![(6080, 6092), (0, 40)]);
    assert_eq!(unwrap(10, 40, 6093), vec![(10, 40)], "an ordinary range stays one piece");
}

/// bases stop being drawn when a slot is under seven pixels
#[test]
fn bases_stop_being_drawn_when_a_slot_is_under_seven_pixels() {
    assert!(regime_for(16.0).bases);
    assert!(regime_for(7.0).bases);
    assert!(!regime_for(6.9).bases);
    assert!(regime_for(6.9).translations, "amino acids survive lower than letters do");
    assert!(!regime_for(4.0).translations);
    assert!(!regime_for(7.9).primer_bases, "a primer stops printing its own bases first");
}

/// a row with nothing stacked in it is as short as its fixed tracks, plus its gap
#[test]
fn a_row_with_nothing_stacked_in_it_is_as_short_as_its_fixed_tracks_plus_its_gap() {
    // Spelled out of TRACK rather than pinned to a number, so the weld can be tuned without
    // rewriting the test that is supposed to be watching it.
    assert_eq!(bare_row_height(), track::SPACER + track::SEQUENCE + track::RUNGS + track::REVERSE_SEQUENCE + track::ROW_GAP,);
    // It got shorter when the ruler left the middle: 16px of coordinate lane became a 9px weld.
    assert!(bare_row_height() < 74.0, "the welded row is tighter than the ruled one it replaced");
}

/// the air below a row is wider than the gap inside it, so a row reads as one block
#[test]
fn the_air_below_a_row_is_wider_than_the_gap_inside_it_so_a_row_reads_as_one_block() {
    // The bug this caught on the first render: without the gap, two rows sat 25px apart while a
    // row's own two strands sat 27px apart, so the eye grouped a reverse strand with the forward
    // strand BELOW it. A row has to be the tighter pairing or it is not a row.
    let b = bands_for(Tracks::default());
    let inside = b.reverse - b.forward;
    let between = b.height - b.reverse + b.forward;
    assert!(between > inside, "{between}px between rows is not more than {inside}px inside one");
}

/// an empty track costs nothing, including its margin
#[test]
fn an_empty_track_costs_nothing_including_its_margin() {
    // The easy bug: adding the margins unconditionally makes every plain row taller than it draws,
    // and a virtualized list built on that estimate has a scrollbar that lies.
    assert_eq!(row_height_of(Lanes { below: StrandLanes { features: 0, ..Default::default() }, ..Default::default() }), bare_row_height());
    assert_eq!(row_height_of(Lanes { above: StrandLanes::default(), below: StrandLanes::default(), ..Default::default() }), bare_row_height());
    assert_eq!(row_height_of(Lanes::default()), bare_row_height());
}

/// a row grows by one lane's worth for each stacked annotation it has to hold
#[test]
fn a_row_grows_by_one_lanes_worth_for_each_stacked_annotation_it_has_to_hold() {
    let bare = bare_row_height();
    let gap = track::LANE_GAP;
    let (f, m) = (track::FEATURES, track::FEATURES_MARGIN_BOTTOM);
    assert_eq!(row_height_of(Lanes { below: StrandLanes { features: 1, ..Default::default() }, ..Default::default() }), bare + gap + f + m);
    assert_eq!(
        row_height_of(Lanes { below: StrandLanes { features: 2, ..Default::default() }, ..Default::default() }),
        bare + gap + 2.0 * f + m,
        "the margin is paid once, not per lane",
    );
    assert_eq!(row_height_of(Lanes { below: StrandLanes { primers: 1, ..Default::default() }, ..Default::default() }), bare + gap + track::PRIMERS);
}

/// a row with annotations above still reads as one block, not as the row above it
#[test]
fn a_row_with_annotations_above_still_reads_as_one_block_not_as_the_row_above_it() {
    // THE BUG THE SPLIT INTRODUCED, and the reason `aboveAir` exists. Putting annotations on top of
    // the bases for the first time broke the rule this row was built on: the space BETWEEN two rows
    // has to beat the space INSIDE one. A reverse feature under row A and a forward feature over row
    // B were `rowGap` apart — exactly the distance meant to separate them — so the forward bar read
    // as belonging to A. Caught by looking at it, not by the bare-row version of this test.
    let above = StrandLanes { features: 1, ..Default::default() };
    // Inside a row: from its topmost annotation down to its own bases.
    let inside = lane_block_height(above);
    // Between two rows: the upper row's trailing gap plus the lower row's outer air.
    let between = track::ROW_GAP + above_air(above);
    assert!(between > inside, "{between}px between rows is not more than {inside}px inside one");
    assert_eq!(track::SPACER + track::ABOVE_AIR, track::ROW_GAP, "a stacked row's top air matches the gap");
}

/// a row with nothing above the bases pays no air for the stack it does not have
#[test]
fn a_row_with_nothing_above_the_bases_pays_no_air_for_the_stack_it_does_not_have() {
    assert_eq!(above_air(StrandLanes::default()), 0.0);
    assert_eq!(above_air(StrandLanes { features: 0, ..Default::default() }), 0.0);
    assert_eq!(
        row_height_of(Lanes { below: StrandLanes { features: 1, ..Default::default() }, ..Default::default() }),
        row_height_of(Lanes { below: StrandLanes { features: 1, ..Default::default() }, ..Default::default() })
    );
    // And the air is charged once for the whole stack, not per track in it.
    assert_eq!(above_air(StrandLanes { features: 2, primers: 3, orfs: 1, ..Default::default() }), track::ABOVE_AIR);
}

/// a forward annotation stacks above and a reverse one below, and both cost the same
#[test]
fn a_forward_annotation_stacks_above_and_a_reverse_one_below_and_both_cost_the_same() {
    // The side of the duplex is what says the strand, so the two sides have to be built the same way
    // — a row with one feature is the same height whichever strand that feature is on.
    assert_eq!(
        row_height_of(Lanes { above: StrandLanes { features: 1, ..Default::default() }, ..Default::default() }),
        row_height_of(Lanes { below: StrandLanes { features: 1, ..Default::default() }, ..Default::default() }) + track::ABOVE_AIR,
        "the same stack costs the same either side, plus the air that keeps a top stack attached",
    );
    // And a row carrying one of each is taller than a row carrying both on one side is NOT true —
    // it is exactly as tall, because each side buys its own laneGap. That is the price of the split.
    let bare = bare_row_height();
    let one_each = row_height_of(Lanes {
        above: StrandLanes { features: 1, ..Default::default() },
        below: StrandLanes { features: 1, ..Default::default() },
        ..Default::default()
    });
    let both_below = row_height_of(Lanes { below: StrandLanes { features: 2, ..Default::default() }, ..Default::default() });
    assert!(one_each > both_below, "two sides buy two gaps; one side buys one");
    assert_eq!(one_each - bare, 2.0 * (track::LANE_GAP + track::FEATURES + track::FEATURES_MARGIN_BOTTOM) + track::ABOVE_AIR,);
}

/// the split is what stops a forward and a reverse annotation shoving each other deeper
#[test]
fn the_split_is_what_stops_a_forward_and_a_reverse_annotation_shoving_each_other_deeper() {
    // Packed together, a forward promoter and a reverse polyA at the same coordinates take two lanes
    // and the row pays for both on one side. Packed apart they each take lane 0 of their own side.
    let together = row_height_of(Lanes { below: StrandLanes { features: 2, ..Default::default() }, ..Default::default() });
    let apart = row_height_of(Lanes {
        above: StrandLanes { features: 1, ..Default::default() },
        below: StrandLanes { features: 1, ..Default::default() },
        ..Default::default()
    });
    assert_eq!(together - bare_row_height(), track::LANE_GAP + 2.0 * track::FEATURES + track::FEATURES_MARGIN_BOTTOM);

    assert!(apart > together, "the split trades a second lane for a second gap, and says so here");
}

/// a cut name and an alignment belong to the duplex, not to a strand
#[test]
fn a_cut_name_and_an_alignment_belong_to_the_duplex_not_to_a_strand() {
    // An enzyme severs both strands, so its name cannot sit on one side without lying; an alignment
    // draws its own duplex. Neither takes a side, and neither buys a strand's laneGap.
    let bare = bare_row_height();
    assert_eq!(row_height_of(Lanes { cutsite_labels: 2, ..Default::default() }), bare + 2.0 * track::CUTSITE_LABELS);
    assert_eq!(row_height_of(Lanes { probe: 1, ..Default::default() }), bare + track::PROBE);
}

/// the air above the stack is paid once, whatever is stacked, and not at all by a bare row
#[test]
fn the_air_above_the_stack_is_paid_once_whatever_is_stacked_and_not_at_all_by_a_bare_row() {
    // `laneGap` is what stops an annotation from underlining the bases. It belongs to the boundary
    // between the strands and the stack, so a row with features AND primers buys it once — and a row
    // with nothing under the bases does not buy it at all, or the scrollbar overstates every row.
    let bare = bare_row_height();
    assert_eq!(
        row_height_of(Lanes { below: StrandLanes { features: 1, primers: 1, ..Default::default() }, ..Default::default() }),
        bare + track::LANE_GAP + track::FEATURES + track::FEATURES_MARGIN_BOTTOM + track::PRIMERS,
    );
    assert_eq!(row_height_of(Lanes::default()), bare, "a bare row is not charged for a stack it does not have");
    assert_eq!(lane_block_height(StrandLanes::default()), 0.0, "an empty side costs nothing at all");
    assert_eq!(lane_block_height(StrandLanes::default()), 0.0, "and neither does a side that was never given");
    assert_eq!(bands_for(Tracks::default()).lanes_top - bands_for(Tracks::default()).after_strands, track::LANE_GAP);
}

/// turning off the reverse strand takes the weld with it
#[test]
fn turning_off_the_reverse_strand_takes_the_weld_with_it() {
    // Nothing to pair, so the channel closes rather than leaving a gap under a lone strand. The
    // rules are not a track and cost no height, so switching them off changes nothing here.
    assert_eq!(
        row_height(Lanes::default(), Tracks { reverse_sequence: false, ..Default::default() }),
        track::SPACER + track::SEQUENCE + track::ROW_GAP,
    );
    assert_eq!(row_height(Lanes::default(), Tracks { axis: false, ..Default::default() }), bare_row_height(), "rules take no height of their own");
}

/// the weld is a channel between the strands and holds only the pairing
#[test]
fn the_weld_is_a_channel_between_the_strands_and_holds_only_the_pairing() {
    let b = bands_for(Tracks::default());
    assert_eq!(b.rung_bottom - b.rung_top, track::RUNGS, "the channel is exactly the weld");
    assert!(b.forward < b.rung_top, "the forward strand's baseline is above the channel");
    assert!(b.rung_bottom < b.reverse, "the reverse strand's baseline is below it");
    assert_eq!(b.duplex, (b.rung_top + b.rung_bottom) / 2.0, "the centre line is the middle of it");
    // Close enough to read as one molecule. It was 16 — a 10px ruler with 3px either side — and at
    // that distance the two strands read as two separate lines of text.
    let rungs = track::RUNGS;
    assert!(rungs <= 10.0, "a {rungs}px weld is too wide to read as welded");
}

/// the annotation lanes sit below both strands, not over the letters
#[test]
fn the_annotation_lanes_sit_below_both_strands_not_over_the_letters() {
    let b = bands_for(Tracks::default());
    assert!(b.after_strands > b.reverse, "features would be drawn on top of the reverse strand");
    assert_eq!(b.height - b.after_strands, track::ROW_GAP, "only the row's own gap is below them");
}

/// the bands and the row height agree
#[test]
fn the_bands_and_the_row_height_agree() {
    // The one invariant the virtualizer stands on: the list sizes every row from rowHeight without
    // building it, so any disagreement here makes the scrollbar lie about how long the construct is.
    assert_eq!(bands_for(Tracks::default()).height, bare_row_height(), "the bands and the row height must disagree nowhere");
}

/// dropping a track closes the gap it left, and the two heights still agree
#[test]
fn dropping_a_track_closes_the_gap_it_left_and_the_two_heights_still_agree() {
    for tracks in [
        Tracks { axis: false, ..Default::default() },
        Tracks { reverse_sequence: false, ..Default::default() },
        Tracks { axis: false, reverse_sequence: false },
    ] {
        assert_eq!(bands_for(tracks).height, row_height(Lanes::default(), tracks), "{tracks:?}");
    }
}

/// a rule falls every fifth base, and the tenth and fiftieth outrank it
#[test]
fn a_rule_falls_every_fifth_base_and_the_tenth_and_fiftieth_outrank_it() {
    let row = Row { index: 0, start: 0, end: 99 };
    let rules = rules_for(row, 10.0);
    assert_eq!(rules.len(), 20, "one every five across a hundred bases, and no more");
    // 1-based for the reader: the first rule falls before base 5, so at bp index 4, and it is faint.
    assert_eq!(rules[0].bp, 4);
    assert!(rules[0].k < 1.0, "a fifth is fainter than a tenth");
    // The tenth falls before base 10, so at bp index 9, at full strength.
    assert_eq!(rules[1].bp, 9);
    assert_eq!(rules[1].w, 1.0, "a plain tenth");
    assert_eq!(rules[1].k, 1.0);
    let fiftieth = rules.iter().find(|r| r.bp == 49);
    assert!(fiftieth.is_some(), "the fiftieth base carries a rule");
    assert_eq!(fiftieth.map(|r| r.w), Some(2.0), "and it is drawn heavier, not twice");
    assert_eq!(rules.iter().filter(|r| r.bp == 49).count(), 1, "the heaviest level wins outright");
}

/// a rule stands on a base's left edge, so ten bases sit between two of them
#[test]
fn a_rule_stands_on_a_bases_left_edge_so_ten_bases_sit_between_two_of_them() {
    let row = Row { index: 0, start: 0, end: 99 };
    let rules = rules_for(row, 10.0);
    // The boundary BEFORE base 10, not the middle of base 10 — otherwise every rule sits half a base
    // off the block it bounds, and a run of ten does not measure ten.
    assert_eq!(rules[1].x, 90.0);
    assert_eq!(rules[3].x - rules[1].x, 100.0, "ten bases at ten pixels each");
}

/// a row shorter than the first rule draws none
#[test]
fn a_row_shorter_than_the_first_rule_draws_none() {
    assert_eq!(rules_for(Row { index: 0, start: 0, end: 3 }, 10.0), Vec::<Rule>::new());
}

/// a cut mark spans from the forward strand, across the weld, to the reverse strand
#[test]
fn a_cut_mark_spans_from_the_forward_strand_across_the_weld_to_the_reverse_strand() {
    let c = cut_bands_for(Tracks::default());
    let b = bands_for(Tracks::default());
    assert!(c.top_y0 < c.mid_y && c.mid_y < c.bot_y1, "the staircase must run downwards");
    assert_eq!(c.mid_y, b.duplex, "the across-segment runs along the centre line, not beside it");
    assert_eq!(c.bot_y1, b.after_strands, "and stops at the foot of the bases");
}

/// two names too close to share a line go on separate lines
#[test]
fn two_names_too_close_to_share_a_line_go_on_separate_lines() {
    let (lanes, count) = pack_lanes(&[(0.0, 40.0), (10.0, 40.0)], PACK_GAP);
    assert_eq!(count, 2);
    assert_ne!(lanes[0], lanes[1]);
}

/// names that clear each other share one line
#[test]
fn names_that_clear_each_other_share_one_line() {
    let (lanes, count) = pack_lanes(&[(0.0, 40.0), (100.0, 40.0)], PACK_GAP);
    assert_eq!(count, 1);
    assert_eq!(lanes, vec![0, 0]);
}

/// lanes come back in the order given, not in the order packed
#[test]
fn lanes_come_back_in_the_order_given_not_in_the_order_packed() {
    // The caller holds a parallel array of cuts; a reordered answer silently mislabels every one.
    let (lanes, _) = pack_lanes(&[(200.0, 40.0), (0.0, 40.0), (210.0, 40.0)], PACK_GAP);
    assert_eq!(lanes[1], 0, "the leftmost still takes the first lane");
    assert_ne!(lanes[0], lanes[2], "the two that overlap are still separated");
}

/// a third name that clears the first goes back on the first line
#[test]
fn a_third_name_that_clears_the_first_goes_back_on_the_first_line() {
    let (_, count) = pack_lanes(&[(0.0, 40.0), (10.0, 40.0), (300.0, 40.0)], PACK_GAP);
    assert_eq!(count, 2, "the third should reuse a lane, not open a new one");
}

/// rows are stacked by what each one actually holds, not by a uniform height
#[test]
fn rows_are_stacked_by_what_each_one_actually_holds_not_by_a_uniform_height() {
    let rows = rows_for(180, 60);
    let plan =
        plan_rows(&rows, |r| if r.index == 1 { Lanes { cutsite_labels: 2, ..Default::default() } } else { Lanes::default() }, Tracks::default());
    assert_eq!(plan.tops[0], 0.0);
    assert_eq!(plan.tops[1], bare_row_height());
    assert_eq!(plan.tops[2] - plan.tops[1], row_height_of(Lanes { cutsite_labels: 2, ..Default::default() }), "row 1 is the tall one");
    assert_eq!(plan.height, plan.tops[3]);
}

/// a scroll offset finds the row it lands in, whatever the heights are
#[test]
fn a_scroll_offset_finds_the_row_it_lands_in_whatever_the_heights_are() {
    let rows = rows_for(600, 60);
    let plan =
        plan_rows(&rows, |r| if r.index % 3 == 0 { Lanes { cutsite_labels: 3, ..Default::default() } } else { Lanes::default() }, Tracks::default());
    for i in 0..rows.len() {
        assert_eq!(row_at(&plan, plan.tops[i]), i, "at row {i}'s own top");
        assert_eq!(row_at(&plan, plan.tops[i + 1] - 1.0), i, "just before row {}", i + 1);
    }
    assert_eq!(row_at(&plan, -50.0), 0, "above the top is the first row");
    assert_eq!(row_at(&plan, 1e9), rows.len() - 1, "past the end is the last row");
}

// ── The interactions lane ───────────────────────────────────────────────────────────────────
// It buys its own room the way every other stacked track does. If `rowHeight` and what is drawn
// disagree, `planRows`'s prefix sum is wrong and the scrollbar lies about where the molecule ends.

/// an interactions lane makes the row taller by exactly its own height
#[test]
fn an_interactions_lane_makes_the_row_taller_by_exactly_its_own_height() {
    let bare = row_height(Lanes::default(), Tracks::default());
    assert_eq!(row_height(Lanes { interactions: 1, ..Default::default() }, Tracks::default()), bare + track::INTERACTIONS);
    assert_eq!(row_height(Lanes { interactions: 3, ..Default::default() }, Tracks::default()), bare + 3.0 * track::INTERACTIONS);
}

/// no interactions costs no height — a hidden layer gives the room back
#[test]
fn no_interactions_costs_no_height_a_hidden_layer_gives_the_room_back() {
    assert_eq!(row_height(Lanes { interactions: 0, ..Default::default() }, Tracks::default()), row_height(Lanes::default(), Tracks::default()));
}

/// the interactions lane stacks with the cut names rather than replacing them
#[test]
fn the_interactions_lane_stacks_with_the_cut_names_rather_than_replacing_them() {
    let bare = row_height(Lanes::default(), Tracks::default());
    assert_eq!(
        row_height(Lanes { interactions: 2, cutsite_labels: 1, ..Default::default() }, Tracks::default()),
        bare + 2.0 * track::INTERACTIONS + track::CUTSITE_LABELS
    );
}
