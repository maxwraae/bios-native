//! Designing primers, as an agent calls it through `Session::execute`: the pair that amplifies a
//! stretch, the pair that installs a change, and the checks on an oligo someone already has.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn with_plasmid() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures/pUC19.gb");
    s.execute("import_plasmid", &json!({ "file": file.display().to_string() })).unwrap();
    (dir, s)
}

fn bases(s: &Session, start: i64, end: i64) -> String {
    s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": start, "end": end })).unwrap()["sequence"].as_str().unwrap().to_string()
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

#[test]
fn a_designed_pair_amplifies_the_stretch_asked_for_and_can_be_kept_with_add_primer() {
    let (_dir, s) = with_plasmid();
    let out = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    assert_eq!(out["amplifies"]["length"], 500);
    assert_eq!(out["product"]["length"], 500);
    assert_eq!(text(&out["product"]["sequence"]), bases(&s, 601, 1100), "the product is the stretch asked for");
    let pairs = out["pairs"].as_array().unwrap();
    assert!(!pairs.is_empty() && pairs.len() <= 5);
    let best = &pairs[0];
    assert_eq!(best["rank"], 1);
    assert!(text(&best["reason"]).ends_with("meets every criterion"), "{}", text(&best["reason"]));
    let (f, r) = (&best["forward"], &best["reverse"]);
    assert_eq!(text(&f["binds"]).split("..").next(), Some("601"));
    assert_eq!(text(&r["binds"]).split("..").nth(1), Some("1100"));
    assert_eq!(f["strand"], "forward");
    assert_eq!(r["strand"], "reverse");
    // The oligos are the plasmid's own bases at those footprints.
    let (f_start, f_end) = span(text(&f["binds"]));
    assert_eq!(text(&f["sequence"]), bases(&s, f_start, f_end));
    let (r_start, r_end) = span(text(&r["binds"]));
    assert_eq!(text(&r["sequence"]), bios_core::seq::revcomp(&bases(&s, r_start, r_end)));

    // What the command's description tells an agent to do with a pair, done.
    let version = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    let added = s
        .execute(
            "add_primer",
            &json!({ "plasmid": "pUC19", "version": version, "name": "pUC19-601F", "start": f_start, "end": f_end, "strand": "forward" }),
        )
        .unwrap();
    assert_eq!(added["primer"]["bases"], f["sequence"]);
    // And the pair the designer named is the pair simulate_pcr predicts the same product from.
    let simulated =
        s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": text(&f["sequence"]), "reverse": text(&r["sequence"]) })).unwrap();
    assert_eq!(simulated["product"]["sequence"], out["product"]["sequence"]);
}

/// The two 1-based ends of a `601..623` footprint.
fn span(binds: &str) -> (i64, i64) {
    let (start, end) = binds.split_once("..").unwrap();
    (start.parse().unwrap(), end.parse().unwrap())
}

#[test]
fn a_feature_can_be_named_instead_of_coordinates() {
    let (_dir, s) = with_plasmid();
    let read = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    let wanted = read["features"].as_array().unwrap().iter().find(|f| f["name"] == "lacZ alpha").expect("pUC19 has lacZ alpha");
    let (start, end) = (wanted["start"].as_i64().unwrap(), wanted["end"].as_i64().unwrap());
    let out = s.execute("design_primers", &json!({ "plasmid": "pUC19", "feature": "lacZ alpha" })).unwrap();
    assert_eq!(out["amplifies"]["feature"], "lacZ alpha");
    assert_eq!(out["amplifies"]["start"], start);
    assert_eq!(out["amplifies"]["end"], end);
    assert_eq!(out["amplifies"]["length"], 324);
    assert_eq!(text(&out["product"]["sequence"]), bases(&s, start, end));
    // One way or the other, not both and not neither.
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "feature": "lacZ alpha", "start": 1 })).is_err());
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19" })).is_err());
}

#[test]
fn tails_ride_on_the_oligos_and_into_the_product_but_not_into_the_tm() {
    let (_dir, s) = with_plasmid();
    let plain = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    let tailed = s
        .execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100, "forward_tail": "GGAATTCC", "reverse_tail": "AAGCTTAA" }))
        .unwrap();
    let (bare, with_tail) = (&plain["pairs"][0], &tailed["pairs"][0]);
    assert_eq!(with_tail["forward"]["anneals"], bare["forward"]["anneals"]);
    assert_eq!(with_tail["forward"]["tm_c"], bare["forward"]["tm_c"], "the tail is not on the template and not in the Tm");
    assert_eq!(with_tail["forward"]["tail"], "GGAATTCC");
    assert_eq!(text(&with_tail["forward"]["sequence"]), format!("GGAATTCC{}", text(&bare["forward"]["anneals"])));
    assert_eq!(with_tail["forward"]["length"].as_u64(), bare["forward"]["length"].as_u64().map(|n| n + 8));
    assert_eq!(tailed["product"]["length"], 516);
    assert_eq!(
        text(&tailed["product"]["sequence"]),
        format!("GGAATTCC{}{}", bases(&s, 601, 1100), bios_core::seq::revcomp("AAGCTTAA")),
        "both tails are in the product"
    );
    // A tail has to be bases that can be ordered.
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100, "forward_tail": "GGNNTTCC" })).is_err());
}

#[test]
fn a_stretch_too_short_for_two_primers_is_refused_and_says_so() {
    let (_dir, s) = with_plasmid();
    let refused = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 621 })).unwrap_err();
    assert!(refused.0.contains("21 bases long"), "{}", refused.0);
    // Lowering the shortest length is what the message suggests, and it works.
    let out = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 630, "min_length": 12, "max_length": 15 })).unwrap();
    assert_eq!(out["product"]["length"], 30);
    for pair in out["pairs"].as_array().unwrap() {
        for role in ["forward", "reverse"] {
            let length = pair[role]["length"].as_u64().unwrap();
            assert!((12..=15).contains(&length), "{length} bases");
        }
    }
}

#[test]
fn the_criteria_can_be_moved_and_are_reported_back() {
    let (_dir, s) = with_plasmid();
    let out = s
        .execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100, "target_tm": 65, "max_tm_difference": 1, "max_pairs": 2 }))
        .unwrap();
    assert_eq!(out["criteria"]["target_tm"], 65.0);
    assert_eq!(out["criteria"]["max_tm_difference"], 1.0);
    assert!(out["pairs"].as_array().unwrap().len() <= 2);
    for pair in out["pairs"].as_array().unwrap() {
        assert!(pair["tm_difference"].as_f64().unwrap() <= 1.0);
    }
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100, "target_tm": 200 })).is_err());
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100, "min_length": 30, "max_length": 20 })).is_err());
}

#[test]
fn a_mutagenesis_pair_carries_the_change_and_the_edit_path_agrees_with_it() {
    let (_dir, s) = with_plasmid();
    let was = bases(&s, 1001, 1001);
    let to = if was == "A" { "G" } else { "A" };
    let out = s.execute("design_mutagenesis_primers", &json!({ "plasmid": "pUC19", "start": 1001, "substitute": to })).unwrap();
    assert_eq!(out["edit"]["kind"], "substitution");
    assert_eq!(text(&out["edit"]["describes"]), format!("{was}1001{to}"));
    assert_eq!(out["edit"]["removes"], was);
    assert_eq!(out["edit"]["installs"], to);
    assert_eq!(out["product"]["length"], 2686);
    assert_eq!(out["product"]["topology"], "circular");
    assert_eq!(text(&out["product"]["becomes"]), format!("{}[{to}]{}", bases(&s, 986, 1000), bases(&s, 1002, 1016)));
    // The oligo is the plasmid with that one base changed, and its partner reads the other way.
    let forward = text(&out["forward"]["sequence"]).to_string();
    let (five, three) = (out["correct_bases_before_the_change"].as_i64().unwrap(), out["correct_bases_after_the_change"].as_i64().unwrap());
    assert_eq!(forward, format!("{}{to}{}", bases(&s, 1001 - five, 1000), bases(&s, 1002, 1001 + three)));
    assert_eq!(text(&out["reverse"]["sequence"]), bios_core::seq::revcomp(&forward));
    assert!(five >= 10 && three >= 10, "{five} and {three} correct bases");

    // Make the change for real: the plasmid then reads exactly what the primer said it would.
    let version = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    s.execute("replace_range", &json!({ "plasmid": "pUC19", "version": version, "start": 1001, "end": 1001, "sequence": to })).unwrap();
    assert_eq!(bases(&s, 1001 - five, 1001 + three), forward);
    assert_eq!(s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["length"], 2686);
}

#[test]
fn an_insertion_and_a_deletion_are_designed_and_match_what_the_edit_does() {
    let (_dir, s) = with_plasmid();
    let inserted = s.execute("design_mutagenesis_primers", &json!({ "plasmid": "pUC19", "start": 1201, "insert": "GGTACC" })).unwrap();
    assert_eq!(inserted["edit"]["kind"], "insertion");
    assert_eq!(inserted["edit"]["removes"], "");
    assert_eq!(inserted["product"]["length"], 2692);
    let (five, three) = (inserted["correct_bases_before_the_change"].as_i64().unwrap(), inserted["correct_bases_after_the_change"].as_i64().unwrap());
    let oligo = text(&inserted["forward"]["sequence"]).to_string();
    assert_eq!(oligo, format!("{}GGTACC{}", bases(&s, 1201 - five, 1200), bases(&s, 1201, 1200 + three)));

    let deleted = s.execute("design_mutagenesis_primers", &json!({ "plasmid": "pUC19", "start": 1201, "delete": 9 })).unwrap();
    assert_eq!(deleted["edit"]["kind"], "deletion");
    assert_eq!(deleted["edit"]["installs"], "");
    assert_eq!(deleted["edit"]["removes"], bases(&s, 1201, 1209));
    assert_eq!(deleted["product"]["length"], 2677);
    let (five, three) = (deleted["correct_bases_before_the_change"].as_i64().unwrap(), deleted["correct_bases_after_the_change"].as_i64().unwrap());
    let joined = text(&deleted["forward"]["sequence"]).to_string();
    assert_eq!(joined, format!("{}{}", bases(&s, 1201 - five, 1200), bases(&s, 1210, 1209 + three)));

    // Insert for real; the designed oligo is then the plasmid's own bases there.
    let version = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "version": version, "position": 1200, "sequence": "GGTACC" })).unwrap();
    let (five, three) = (inserted["correct_bases_before_the_change"].as_i64().unwrap(), inserted["correct_bases_after_the_change"].as_i64().unwrap());
    assert_eq!(bases(&s, 1201 - five, 1206 + three), oligo);
}

#[test]
fn a_change_has_to_be_said_in_one_way_and_stay_inside_the_plasmid() {
    let (_dir, s) = with_plasmid();
    let one_way = |params: Value| s.execute("design_mutagenesis_primers", &params).unwrap_err();
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 1001 })).0.contains("one way"));
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 1001, "substitute": "G", "delete": 3 })).0.contains("one way"));
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 9999, "substitute": "G" })).0.contains("between 1 and 2686"));
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 1001, "substitute": "N" })).0.contains("A, C, G and T"));
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 1001, "delete": 500 })).0.contains("between 1 and 30"));
    // Changing a base to itself is nothing to design for.
    let same = bases(&s, 1001, 1001);
    assert!(one_way(json!({ "plasmid": "pUC19", "start": 1001, "substitute": same })).0.contains("nothing to change"));
}

#[test]
fn check_primer_reports_an_oligo_on_its_own_and_where_it_binds() {
    let (_dir, s) = with_plasmid();
    let alone = s.execute("check_primer", &json!({ "sequence": "GCTTTCCAGTCGGGAAACCTGTC" })).unwrap();
    assert_eq!(alone["length"], 23);
    assert_eq!(alone["tm_c"], 59.8);
    assert_eq!(alone["gc_percent"], 56.5);
    assert_eq!(alone["gc_clamp"], true);
    assert_eq!(alone["warnings"].as_array().unwrap().len(), 0);
    assert!(alone.get("sites").is_none(), "with no plasmid named there is nothing to bind to");

    let on_plasmid = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": "GCTTTCCAGTCGGGAAACCTGTC" })).unwrap();
    assert_eq!(on_plasmid["binds_more_than_once"], false);
    let site = &on_plasmid["sites"][0];
    assert_eq!((site["start"].as_i64(), site["end"].as_i64(), site["strand"].as_str()), (Some(601), Some(623), Some("forward")));
    assert_eq!(site["on_target"], true);
    assert_eq!(site["mismatches"], 0);
    assert_eq!(site["tail_length"], 0);
    assert_eq!(site["anneal_tm_c"], 59.8, "with no tail the site's Tm is the whole oligo's");
    assert!(text(&on_plasmid["verdict"]).starts_with("binds once"), "{}", text(&on_plasmid["verdict"]));

    // The same oligo with a HindIII site on its 5' end: the oligo is hotter, the site is not.
    let tailed = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": "AAGCTTAGCTTTCCAGTCGGGAAACCTGTC" })).unwrap();
    assert_eq!(tailed["length"], 30);
    assert_eq!(tailed["tm_c"], 63.7);
    let site = &tailed["sites"][0];
    assert_eq!((site["start"].as_i64(), site["end"].as_i64(), site["tail_length"].as_i64()), (Some(601), Some(623), Some(7)));
    assert_eq!(site["anneal_tm_c"], 59.8, "a 5' tail is not on the plasmid and not in the site's Tm");
    assert_eq!(site["on_target"], true);
}

#[test]
fn check_primer_names_the_faults_that_make_an_oligo_unusable() {
    let (_dir, s) = with_plasmid();
    // A poly-A 3' end: no clamp and a run of five.
    let poor = s.execute("check_primer", &json!({ "sequence": "GCTTTCCAGTCGGGAAAAAAA" })).unwrap();
    let warnings: Vec<&str> = poor["warnings"].as_array().unwrap().iter().map(text).collect();
    assert!(warnings.iter().any(|w| w.contains("clamp")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("A run")), "{warnings:?}");

    // A made-up construct carrying one 22mer twice: the oligo has no on-target site at all.
    let twice_over = "GCATTCAGGTACCATCGAGTCA";
    let made_up = format!("TTGACCTAGCAATGGTCAAGC{twice_over}AATCGGTTACCAGTTCAGCATTGGACAACT{twice_over}CCTAAGGTTCAACGGATCAGTTAACGGTTC");
    s.execute("create_plasmid", &json!({ "name": "repeat test", "sequence": made_up, "circular": true })).unwrap();
    let twice = s.execute("check_primer", &json!({ "plasmid": "repeat test", "sequence": twice_over })).unwrap();
    assert_eq!(twice["binds_more_than_once"], true);
    assert_eq!(twice["sites"].as_array().unwrap().len(), 2);
    assert!(twice["sites"].as_array().unwrap().iter().all(|s| s["on_target"] == false && s["mismatches"] == 0));
    assert!(text(&twice["verdict"]).contains("binds equally well in 2 places"), "{}", text(&twice["verdict"]));

    // An oligo that is not on the plasmid at all.
    let stranger = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": "GGGGTTTTGGGGTTTTGGGGTTTT" })).unwrap();
    assert_eq!(stranger["sites"].as_array().unwrap().len(), 0);
    assert!(text(&stranger["verdict"]).contains("does not bind"), "{}", text(&stranger["verdict"]));

    assert!(s.execute("check_primer", &json!({ "sequence": "12 -" })).is_err());
}

#[test]
fn designing_never_touches_the_file() {
    let (_dir, s) = with_plasmid();
    let before = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    s.execute("design_mutagenesis_primers", &json!({ "plasmid": "pUC19", "start": 1001, "delete": 3 })).unwrap();
    s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": "GCTTTCCAGTCGGGAAACCTGTC" })).unwrap();
    s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19" })).unwrap();
    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    assert_eq!(before, after);
}

#[test]
fn every_pair_carries_an_annealing_temperature_an_agent_can_check_by_hand() {
    let (_dir, s) = with_plasmid();
    let out = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    let best = &out["pairs"][0];
    let a = &best["annealing_temperature_c"];
    // Recorded: a 500 bp product at 57.4% GC off two primers melting at 59.4 and 59.9 °C.
    assert_eq!(a["cooler_primer_tm_c"], 59.4);
    assert_eq!(a["product_tm_c"], 82.1);
    assert_eq!(a["product_length"], 500);
    assert_eq!(a["product_gc_percent"], 57.4);
    assert_eq!(a["celsius"], 60.4, "0.3 × 59.4 + 0.7 × 82.1 − 14.9");
    assert!(text(&a["rule"]).contains("Rychlik"), "the number says where it comes from: {}", text(&a["rule"]));
    assert!(text(&a["rule"]).contains("not a guarantee"), "{}", text(&a["rule"]));
    // Every pair gets one, and it is the rule applied to that pair's own numbers.
    for pair in out["pairs"].as_array().unwrap() {
        let a = &pair["annealing_temperature_c"];
        let (primer, product, ta) = (a["cooler_primer_tm_c"].as_f64().unwrap(), a["product_tm_c"].as_f64().unwrap(), a["celsius"].as_f64().unwrap());
        assert!((ta - (0.3 * primer + 0.7 * product - 14.9)).abs() < 0.1, "{ta}");
    }
}

#[test]
fn every_pair_carries_the_cross_dimer_between_its_two_oligos() {
    let (_dir, s) = with_plasmid();
    let out = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    for pair in out["pairs"].as_array().unwrap() {
        let dimer = &pair["cross_dimer"];
        assert!(dimer["longest_complementary_bases"].as_u64().is_some());
        assert!(dimer["three_prime_bases_first"].as_u64().is_some());
        assert!(dimer["three_prime_bases_second"].as_u64().is_some());
        // The drawing is the two oligos, one over the other, with the second read 3' to 5'.
        let drawn = text(&dimer["diagram"]);
        assert!(drawn.starts_with("5'-") && drawn.contains("3'-"), "{drawn}");
        assert_eq!(drawn.lines().count(), 3, "{drawn}");
        assert!(drawn.contains(text(&pair["forward"]["sequence"])), "{drawn}");
        // The pair's own warnings are the dimer's plus either primer's hairpins.
        assert!(pair["warnings"].as_array().is_some());
    }
    // Recorded: the best pair over this stretch has no dimer worth warning about, and its forward
    // primer folds on a 5-bp stem.
    let best = &out["pairs"][0];
    assert_eq!(best["cross_dimer"]["warnings"].as_array().unwrap().len(), 0);
    assert_eq!(best["warnings"][0], "forward: 5-bp hairpin stem round a 5-base loop");
    assert_eq!(best["forward"]["hairpin"]["worst"]["stem_base_pairs"], 5);
    assert_eq!(best["forward"]["hairpin"]["worst"]["loop_bases"], 5);
    assert_eq!(best["forward"]["hairpin"]["worst"]["pairs_the_3_prime_end"], false);
    assert_eq!(best["forward"]["hairpin"]["on_the_3_prime_end"], serde_json::Value::Null);
}

#[test]
fn boundary_slack_lets_a_gc_rich_boundary_be_cooled_and_says_how_far_each_end_moved() {
    let (_dir, s) = with_plasmid();
    // pUC19 is 78% GC from base 101, so pinned primers there run hot however short they are.
    let pinned = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 101, "end": 600 })).unwrap();
    assert_eq!(pinned["criteria"]["boundary_slack"], 0);
    assert_eq!(pinned["pairs"][0]["forward"]["tm_c"], 64.4);
    assert_eq!(pinned["pairs"][0]["moved_forward"], 0);
    assert_eq!(pinned["pairs"][0]["moved_reverse"], 0);
    assert_eq!(pinned["pairs"][0]["product_length"], 500);
    assert!(pinned["product"].get("note").is_none(), "with no slack every pair makes the same product");

    let loosened = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 101, "end": 600, "boundary_slack": 20 })).unwrap();
    assert_eq!(loosened["criteria"]["boundary_slack"], 20);
    let best = &loosened["pairs"][0];
    assert_eq!(best["forward"]["tm_c"], 59.6, "the forward primer now reaches the target");
    assert_eq!(best["moved_forward"], 15);
    assert_eq!(best["moved_reverse"], 8);
    assert_eq!(text(&best["amplifies"]), "86..608");
    assert_eq!(best["product_length"], 523, "500 asked for, plus the 23 the two ends moved");
    assert!(text(&loosened["product"]["note"]).contains("do not all make the same product"), "{}", text(&loosened["product"]["note"]));
    // The stretch asked for is still whole inside every pair's product.
    for pair in loosened["pairs"].as_array().unwrap() {
        let (from, to) = span(text(&pair["amplifies"]));
        assert!(from <= 101 && to >= 600, "{from}..{to}");
        assert_eq!(pair["product_length"].as_i64().unwrap(), to - from + 1);
    }
    // And the product reported at the top is the best pair's.
    assert_eq!(loosened["product"]["length"], best["product_length"]);
    assert_eq!(text(&loosened["product"]["sequence"]), bases(&s, 86, 608));

    // Out of range is refused rather than silently clamped.
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 101, "end": 600, "boundary_slack": 500 })).is_err());
    assert!(s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 101, "end": 600, "boundary_slack": -5 })).is_err());
}

#[test]
fn check_primer_checks_two_oligos_against_each_other() {
    let (_dir, s) = with_plasmid();
    // The hand-counted pair: the last six bases of each pair with the last six of the other, and
    // neither holds a T, so nothing else in them can pair at all.
    let out = s.execute("check_primer", &json!({ "sequence": "AAAAAAAAAAGGCGGC", "against": "AAAAAAAAAAGCCGCC" })).unwrap();
    assert_eq!(out["against"]["sequence"], "AAAAAAAAAAGCCGCC");
    let dimer = &out["cross_dimer"];
    assert_eq!(dimer["longest_complementary_bases"], 6);
    assert_eq!(dimer["three_prime_bases_first"], 6);
    assert_eq!(dimer["three_prime_bases_second"], 6);
    let warnings: Vec<&str> = dimer["warnings"].as_array().unwrap().iter().map(text).collect();
    assert_eq!(warnings, vec!["6-bp cross-complementarity between the two", "6-bp dimer on both 3' ends, which the polymerase extends"]);
    assert_eq!(text(&dimer["diagram"]), format!("5'-AAAAAAAAAAGGCGGC-3'\n{}||||||\n3'-{}CCGCCGAAAAAAAAAA-5'", " ".repeat(13), " ".repeat(10)));
    // Without `against` there is no dimer section at all: there is nothing to dimerise with.
    let alone = s.execute("check_primer", &json!({ "sequence": "AAAAAAAAAAGGCGGC" })).unwrap();
    assert!(alone.get("cross_dimer").is_none());
}

#[test]
fn check_primer_takes_a_primer_on_the_plasmid_as_the_other_oligo() {
    let (_dir, s) = with_plasmid();
    // Design a pair, keep the reverse one on the plasmid, then check the forward one against it by
    // name: what an agent does between designing and ordering.
    let designed = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    let (forward, reverse) = (&designed["pairs"][0]["forward"], &designed["pairs"][0]["reverse"]);
    let (r_start, r_end) = span(text(&reverse["binds"]));
    let version = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    s.execute(
        "add_primer",
        &json!({ "plasmid": "pUC19", "version": version, "name": "pUC19-1100R", "start": r_start, "end": r_end, "strand": "reverse" }),
    )
    .unwrap();

    let out = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": text(&forward["sequence"]), "against": "pUC19-1100R" })).unwrap();
    assert_eq!(out["against"]["name"], "pUC19-1100R");
    assert_eq!(out["against"]["sequence"], reverse["sequence"], "the primer's own oligo is used");
    // The same two oligos the designer already compared, so the same answer.
    assert_eq!(out["cross_dimer"], designed["pairs"][0]["cross_dimer"]);
    // It still reports where the oligo binds, as it does without `against`.
    assert_eq!(out["binds_more_than_once"], false);
    assert!(text(&out["verdict"]).starts_with("binds once"), "{}", text(&out["verdict"]));

    // A name that is neither a primer on the plasmid nor an exact oligo is refused, rather than
    // read as bases: a dimer worked out against a misread name is worse than no answer.
    let oligo = text(&forward["sequence"]).to_string();
    let bad = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": oligo.clone(), "against": "no such primer" })).unwrap_err();
    assert!(bad.0.contains("may only hold A, C, G and T"), "{}", bad.0);
    let empty = s.execute("check_primer", &json!({ "plasmid": "pUC19", "sequence": oligo, "against": "12 -" })).unwrap_err();
    assert!(empty.0.contains("holds no bases"), "{}", empty.0);
}

#[test]
fn check_primer_names_a_hairpin_and_tells_a_three_prime_one_apart() {
    let (_dir, s) = with_plasmid();
    // Hand-counted: GGCGG against the CCGCC five bases from the end, round four Ts.
    let middle = s.execute("check_primer", &json!({ "sequence": "GGCGGTTTTCCGCCTTT" })).unwrap();
    let worst = &middle["hairpin"]["worst"];
    assert_eq!(worst["stem_base_pairs"], 5);
    assert_eq!(worst["loop_bases"], 4);
    assert_eq!(text(&worst["stem_5_prime"]), "1..5");
    assert_eq!(text(&worst["stem_3_prime"]), "10..14");
    assert_eq!(worst["pairs_the_3_prime_end"], false);
    assert_eq!(text(&worst["shape"]), "[GGCGG]TTTT[CCGCC]TTT");
    assert_eq!(middle["hairpin"]["on_the_3_prime_end"], serde_json::Value::Null);
    assert_eq!(text(&middle["hairpin"]["warnings"][0]), "5-bp hairpin stem round a 4-base loop");

    // Three pairs only, but on the 3' end, which is the one that stops extension.
    let on_the_end = s.execute("check_primer", &json!({ "sequence": "GGGTTTTCCC" })).unwrap();
    assert_eq!(on_the_end["hairpin"]["worst"]["pairs_the_3_prime_end"], true);
    assert_eq!(on_the_end["hairpin"]["on_the_3_prime_end"]["stem_base_pairs"], 3);
    assert_eq!(text(&on_the_end["hairpin"]["warnings"][0]), "hairpin pairs the 3' end: 3-bp stem round a 4-base loop");

    // An oligo that folds on nothing says so plainly.
    let flat = s.execute("check_primer", &json!({ "sequence": "AAAAAAAAAAAAAAAAAAAA" })).unwrap();
    assert_eq!(flat["hairpin"], serde_json::Value::Null);
}

#[test]
fn design_sequencing_primers_reads_the_whole_of_puc19() {
    let (_dir, s) = with_plasmid();
    let out = s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(out["to_read"]["start"], 1);
    assert_eq!(out["to_read"]["end"], 2686);
    assert_eq!(out["to_read"]["length"], 2686);
    assert_eq!(out["to_read"]["whole_plasmid"], true);
    assert_eq!(out["reads"]["read_length"], 800);
    assert_eq!(out["reads"]["overlap"], 100);
    assert_eq!(out["reads"]["both_strands"], false);
    let strands = out["strands"].as_array().unwrap();
    assert_eq!(strands.len(), 1);
    let forward = &strands[0];
    assert_eq!(forward["strand"], "forward");
    assert_eq!(forward["primers"], 4, "2686 bases at 800 a read, overlapping 100");
    assert_eq!(forward["covered"], 2686);
    assert_eq!(forward["covers_everything"], true);
    assert_eq!(forward["gaps"].as_array().unwrap().len(), 0);
    assert_eq!(out["rejected"].as_array().unwrap().len(), 0);

    // Recorded: the four oligos, where each binds and what each reads.
    let picked: Vec<(String, String, f64)> = forward["reads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (text(&r["primer"]["binds"]).to_string(), text(&r["reads"]).to_string(), r["primer"]["tm_c"].as_f64().unwrap()))
        .collect();
    assert_eq!(
        picked,
        vec![
            ("2661..2683".to_string(), "2684..797".to_string(), 60.2),
            ("676..695".to_string(), "696..1495".to_string(), 59.9),
            ("1366..1389".to_string(), "1390..2189".to_string(), 60.1),
            ("2047..2070".to_string(), "2071..184".to_string(), 60.3),
        ]
    );
    for (i, read) in forward["reads"].as_array().unwrap().iter().enumerate() {
        assert_eq!(read["order"], i + 1);
        assert_eq!(read["primer"]["strand"], "forward");
        // Each oligo is the plasmid's own bases at its footprint, so it can be kept with add_primer.
        let (from, to) = span(text(&read["primer"]["binds"]));
        assert_eq!(text(&read["primer"]["sequence"]), bases(&s, from, to));
        // And each read begins at the base after the primer's 3' end.
        let (reads_from, _) = span(text(&read["reads"]));
        assert_eq!(reads_from, to % 2686 + 1);
        if i > 0 {
            assert!(read["shares_with_previous"].as_u64().unwrap() >= 100, "{}", read["shares_with_previous"]);
        }
    }
    // The first two primers happen to be unique in every sense; the third has one weaker site.
    assert_eq!(forward["reads"][2]["other_sites"], 1);
}

#[test]
fn design_sequencing_primers_reads_both_strands_and_a_range() {
    let (_dir, s) = with_plasmid();
    let both = s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "both_strands": true })).unwrap();
    let strands = both["strands"].as_array().unwrap();
    assert_eq!(strands.len(), 2);
    assert_eq!(strands[0]["strand"], "forward");
    assert_eq!(strands[1]["strand"], "reverse");
    for strand in strands {
        assert_eq!(strand["covered"], 2686, "every base is read on both strands");
        assert_eq!(strand["gaps"].as_array().unwrap().len(), 0);
        assert_eq!(strand["primers"], 4);
    }
    // A reverse primer is the reverse complement of the plasmid at its footprint, and reads back
    // towards lower numbers.
    for read in strands[1]["reads"].as_array().unwrap() {
        assert_eq!(read["primer"]["strand"], "reverse");
        let (from, to) = span(text(&read["primer"]["binds"]));
        assert_eq!(text(&read["primer"]["sequence"]), bios_core::seq::revcomp(&bases(&s, from, to)));
    }

    // Part of a plasmid: 1201 bases at 800 a read is two reads.
    let part = s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "start": 400, "end": 1600 })).unwrap();
    assert_eq!(part["to_read"]["length"], 1201);
    assert_eq!(part["to_read"]["whole_plasmid"], false);
    assert_eq!(part["strands"][0]["primers"], 2);
    assert_eq!(part["strands"][0]["covered"], 1201);

    // One end without the other is not a stretch.
    assert!(s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "start": 400 })).is_err());
    assert!(s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "start": 400, "end": 9999 })).is_err());
    // Settings that cannot tile are refused with a reason, not quietly adjusted.
    let stuck = s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "read_length": 300, "overlap": 300 })).unwrap_err();
    assert!(stuck.0.contains("never moves on"), "{}", stuck.0);
    assert!(s.execute("design_sequencing_primers", &json!({ "plasmid": "pUC19", "read_length": 10 })).is_err());
}

#[test]
fn design_sequencing_primers_refuses_a_primer_that_binds_twice_and_says_what_it_could_not_read() {
    let (_dir, s) = with_plasmid();
    // A made-up plasmid carrying one 600-base stretch of pUC19 twice. No oligo from inside that
    // stretch binds once, so its second copy cannot be read with a unique primer at all.
    let repeat = bases(&s, 1, 600);
    let made_up = format!("{repeat}{}{repeat}{}", bases(&s, 601, 1400), bases(&s, 1401, 2200));
    s.execute("create_plasmid", &json!({ "name": "repeat plasmid", "sequence": made_up, "circular": true })).unwrap();
    let out = s.execute("design_sequencing_primers", &json!({ "plasmid": "repeat plasmid", "read_length": 300, "overlap": 50 })).unwrap();
    let forward = &out["strands"][0];
    assert_eq!(forward["covers_everything"], false);
    assert!(forward["covered"].as_u64().unwrap() < 2800);

    let rejected = out["rejected"].as_array().unwrap();
    assert!(!rejected.is_empty(), "candidates inside the repeat have to be refused, not offered");
    assert!(
        rejected.iter().any(|r| text(&r["why"]).contains("binds fully and perfectly in 2 places")),
        "{:?}",
        rejected.iter().map(|r| text(&r["why"])).collect::<Vec<_>>()
    );
    assert!(rejected.iter().all(|r| !text(&r["would_bind"]).is_empty()), "a refusal says where it would have bound");

    let gaps = forward["gaps"].as_array().unwrap();
    assert!(!gaps.is_empty(), "the repeat cannot be covered and the plan has to say so");
    assert!(
        gaps.iter().any(|g| text(&g["why"]).contains("all bind the construct in more than one place")),
        "{:?}",
        gaps.iter().map(|g| text(&g["why"])).collect::<Vec<_>>()
    );
    // The reason is the repeat and nothing else: no gap here blames a temperature.
    assert!(gaps.iter().all(|g| !text(&g["why"]).contains("°C target")), "{:?}", gaps.iter().map(|g| text(&g["why"])).collect::<Vec<_>>());
    for gap in gaps {
        assert!(gap["length"].as_u64().unwrap() > 0);
    }
    // Everything it did offer binds exactly once, which is what check_primer says of it.
    for read in forward["reads"].as_array().unwrap() {
        let checked = s.execute("check_primer", &json!({ "plasmid": "repeat plasmid", "sequence": text(&read["primer"]["sequence"]) })).unwrap();
        assert_eq!(checked["binds_more_than_once"], false, "{}", text(&read["primer"]["sequence"]));
    }
}

#[test]
fn simulate_pcr_says_what_to_anneal_at() {
    let (_dir, s) = with_plasmid();
    let designed = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 1100 })).unwrap();
    let (forward, reverse) = (&designed["pairs"][0]["forward"], &designed["pairs"][0]["reverse"]);
    let out = s
        .execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": text(&forward["sequence"]), "reverse": text(&reverse["sequence"]) }))
        .unwrap();
    let a = &out["product"]["annealing_temperature_c"];
    // The same product off the same two primers, so the same number the designer gave.
    assert_eq!(a, &designed["pairs"][0]["annealing_temperature_c"]);
    assert_eq!(a["celsius"], 60.4);
    assert_eq!(a["product_length"], 500);
    assert!(text(&a["rule"]).contains("Rychlik"), "{}", text(&a["rule"]));

    // A product under the 50 bases the rule applies to gets no number rather than a wrong one.
    let tiny = s.execute("design_primers", &json!({ "plasmid": "pUC19", "start": 601, "end": 640, "min_length": 12, "max_length": 18 })).unwrap();
    assert_eq!(tiny["pairs"][0]["annealing_temperature_c"], serde_json::Value::Null);
    let short = s
        .execute(
            "simulate_pcr",
            &json!({
                "plasmid": "pUC19",
                "forward": text(&tiny["pairs"][0]["forward"]["sequence"]),
                "reverse": text(&tiny["pairs"][0]["reverse"]["sequence"]),
            }),
        )
        .unwrap();
    assert_eq!(short["product"]["length"], 40);
    assert_eq!(short["product"]["annealing_temperature_c"], serde_json::Value::Null);
}
