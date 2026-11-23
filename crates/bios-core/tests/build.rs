//! Build's own maths, case for case from the web app's `build/openings.test.ts`,
//! `build/intake.test.ts` and `build/order.test.ts`, with their numbers.

use bios_core::build::{DesignPieces, IntakeRow, arcs_between, enzyme_options, is_opening, order_across, order_tsv, order_xlsx, parse_insert_text};

// A backbone with a known EcoRI site (GAATTC) planted once and a PstI site (CTGCAG) planted three
// times, so the ordering rule has something real to sort.
fn pad(n: usize) -> String {
    "ATATATATAT".repeat(n.div_ceil(10))[..n].to_string()
}

fn seq() -> String {
    format!("{}GAATTC{}CTGCAG{}CTGCAG{}CTGCAG{}", pad(200), pad(200), pad(200), pad(200), pad(200))
}

#[test]
fn unique_cutters_come_first_and_multi_cutters_stay_in_the_list() {
    let opts = enzyme_options(&seq(), true, &["EcoRI", "PstI"]);
    assert_eq!(opts.iter().map(|o| o.enzyme.as_str()).collect::<Vec<_>>(), ["EcoRI", "PstI"], "1 site sorts ahead of 3");
    assert_eq!(opts[0].count, 1);
    assert_eq!(opts[1].count, 3);
    assert!(is_opening(&opts[0]), "a unique cutter is an opening");
    assert!(!is_opening(&opts[1]), "a triple cutter is not");
}

#[test]
fn an_enzyme_that_does_not_cut_is_dropped_not_dimmed() {
    let opts = enzyme_options(&seq(), true, &["EcoRI", "BamHI"]);
    assert_eq!(opts.iter().map(|o| o.enzyme.as_str()).collect::<Vec<_>>(), ["EcoRI"], "BamHI (GGATCC) is absent");
}

#[test]
fn each_option_carries_its_recognition_site_and_its_real_positions() {
    let opts = enzyme_options(&seq(), true, &["EcoRI"]);
    let eco = &opts[0];
    assert_eq!(eco.site, "GAATTC");
    assert_eq!(eco.cuts.len(), eco.count);
    assert_eq!(eco.cuts[0].site_start, 200, "GAATTC was planted at 200");
    assert_eq!(eco.cuts[0].at, 201, "…and G^AATTC cuts one base into it");
}

#[test]
fn cuts_come_back_ascending() {
    let opts = enzyme_options(&seq(), true, &["EcoRI", "PstI"]);
    let at: Vec<usize> = opts[1].cuts.iter().map(|c| c.at).collect();
    let mut sorted = at.clone();
    sorted.sort_unstable();
    assert_eq!(at, sorted);
}

#[test]
fn an_empty_sequence_offers_nothing_rather_than_failing() {
    assert!(enzyme_options("", true, &["EcoRI"]).is_empty());
}

#[test]
fn two_cuts_define_two_arcs_and_the_pair_alone_does_not_say_which() {
    assert_eq!(arcs_between(4500, 200, 5000), (4300, 700));
    assert_eq!(arcs_between(200, 4500, 5000), (4300, 700), "order-independent");
    let (between, origin) = arcs_between(100, 1100, 1200);
    assert_eq!(between + origin, 1200, "the two arcs partition the molecule");
}

// ── intake ──────────────────────────────────────────────────────────────────────────────────

const A: &str = "ATGGTGAGCAAGGGCGAGGAGCTGTTCACC";
const B: &str = "ATGAGCGAGCTGATTAAGGAGAACATGCAC";

fn row(name: &str, seq: &str) -> IntakeRow {
    IntakeRow { name: name.into(), seq: seq.into() }
}

#[test]
fn a_fasta_is_one_row_per_record_named_by_the_headers_first_word() {
    let text = format!(">EGFP some description\n{}\n{}\n>BFP\n{B}\n", &A[..10], &A[10..]);
    assert_eq!(parse_insert_text(&text), vec![row("EGFP", A), row("BFP", B)]);
}

#[test]
fn an_excel_paste_is_tab_separated_and_the_header_line_is_skipped() {
    let text = format!("name\tsequence\nEGFP\t{A}\nBFP\t{B}\n");
    assert_eq!(parse_insert_text(&text), vec![row("EGFP", A), row("BFP", B)]);
}

#[test]
fn a_csv_with_the_sequence_first_still_finds_the_name() {
    let text = format!("{A},EGFP\n{B},BFP");
    assert_eq!(parse_insert_text(&text), vec![row("EGFP", A), row("BFP", B)]);
}

#[test]
fn one_sequence_per_line_is_a_row_each_unnamed_spaces_and_case_tolerated() {
    let text = format!("{}\n\n{} {}\n", A.to_lowercase(), &B[..5], &B[5..]);
    assert_eq!(parse_insert_text(&text), vec![row("", A), row("", B)]);
}

#[test]
fn a_short_label_on_its_own_line_is_not_a_sequence() {
    assert_eq!(parse_insert_text("TATA\nhello"), Vec::new());
}

// ── the order across a folder ───────────────────────────────────────────────────────────────

fn pieces(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(name, order)| ((*name).to_string(), (*order).to_string())).collect()
}

#[test]
fn a_piece_shared_by_two_designs_is_ordered_once_and_cited_by_both() {
    let a = pieces(&[("prom", "ACGTACGT"), ("A_2", "TTTTGGGG")]);
    let b = pieces(&[("prom", "acgtacgt"), ("B_2", "CCCCAAAA")]);
    let o = order_across(&[DesignPieces { design: "A", pieces: &a }, DesignPieces { design: "B", pieces: &b }]);
    assert_eq!(o.pieces.iter().map(|p| (p.n, p.name.as_str(), p.length)).collect::<Vec<_>>(), vec![(1, "prom", 8), (2, "A_2", 8), (3, "B_2", 8)]);
    assert_eq!(o.recipe("A"), Some([1, 2].as_slice()));
    assert_eq!(o.recipe("B"), Some([1, 3].as_slice()));
}

#[test]
fn a_design_with_no_payload_has_an_empty_recipe_and_adds_nothing_to_order() {
    let o = order_across(&[DesignPieces { design: "old", pieces: &[] }]);
    assert!(o.pieces.is_empty());
    assert_eq!(o.recipe("old"), Some([].as_slice()));
}

#[test]
fn a_name_two_different_pieces_share_says_which_design_it_came_from() {
    // Nobody types an insert name, so `Insert_1` in one design and `Insert_1` in another are two
    // different molecules under one name — and an order form would take both as the same tube.
    let a = pieces(&[("Insert_1", "AAAA"), ("only", "TTTT")]);
    let b = pieces(&[("Insert_1", "GGGG")]);
    let o = order_across(&[DesignPieces { design: "d1", pieces: &a }, DesignPieces { design: "d2", pieces: &b }]);
    assert_eq!(o.pieces.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Insert_1 · d1", "only", "Insert_1 · d2"]);
}

#[test]
fn the_tsv_is_four_columns_with_a_header() {
    let a = pieces(&[("EGFP", "ATGGTG")]);
    let o = order_across(&[DesignPieces { design: "A", pieces: &a }]);
    assert_eq!(order_tsv(&o.pieces), "#\tname\tlength\tsequence\n1\tEGFP\t6\tATGGTG");
}

#[test]
fn the_xlsx_is_a_workbook_with_one_sheet_holding_the_same_rows() {
    let a = pieces(&[("EGFP", "ATGGTG"), ("x<y", "CCC")]);
    let o = order_across(&[DesignPieces { design: "A", pieces: &a }]);
    let book = order_xlsx(&o.pieces, "To order");
    let text = String::from_utf8_lossy(&book);
    for part in ["xl/workbook.xml", "[Content_Types].xml", "xl/worksheets/sheet1.xml"] {
        assert!(text.contains(part), "{part} is missing from the workbook");
    }
    assert!(text.contains(
        "<row r=\"2\"><c r=\"A2\"><v>1</v></c><c r=\"B2\" t=\"inlineStr\"><is><t>EGFP</t></is></c>\
<c r=\"C2\"><v>6</v></c><c r=\"D2\" t=\"inlineStr\"><is><t>ATGGTG</t></is></c></row>"
    ));
    assert!(text.contains("x&lt;y"), "a name with punctuation is escaped");
    // And it is a zip: the local header, the central directory and the end record, in that order.
    assert_eq!(&book[..4], b"PK\x03\x04");
    assert!(book.windows(4).any(|w| w == b"PK\x01\x02"));
    assert_eq!(&book[book.len() - 22..book.len() - 18], b"PK\x05\x06");
}
