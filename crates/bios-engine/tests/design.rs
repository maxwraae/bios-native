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
    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap()["version"].clone();
    assert_eq!(before, after);
}
