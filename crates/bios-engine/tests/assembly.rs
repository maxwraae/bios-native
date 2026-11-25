//! `design_assembly_oligos` through `Session::execute`, the way an agent calls it: a sequence and
//! two conditions in, a numbered list of oligos to order out.

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

/// A session on an empty project. The command reads nothing from disk, but every command runs in
/// one.
fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let session = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    (dir, session)
}

/// Bases that are nothing in particular, the same on every run.
fn dna(n: usize, seed: u64) -> String {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ["A", "C", "G", "T"][(x >> 33) as usize % 4]
        })
        .collect()
}

fn revcomp(s: &str) -> String {
    s.chars()
        .rev()
        .map(|c| match c {
            'A' => 'T',
            'T' => 'A',
            'C' => 'G',
            'G' => 'C',
            other => other,
        })
        .collect()
}

fn design(s: &Session, params: Value) -> Value {
    s.execute("design_assembly_oligos", &params).unwrap()
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap().iter().map(|o| o["sequence"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_sequence_comes_back_as_a_numbered_list_of_oligos_to_order() {
    let (_dir, s) = session();
    let target = dna(600, 11);
    let out = design(&s, json!({ "sequence": target, "name": "mygene" }));

    assert_eq!(out["target_length"], 600);
    assert_eq!(out["max_length"], 60);
    assert_eq!(out["target_tm"], 60.0);
    let oligos = out["oligos"].as_array().unwrap();
    assert!(oligos.len() > 10, "a 600 bp target needs more than ten 60 nt oligos");
    assert_eq!(out["oligo_count"], oligos.len());

    // Numbered from 1, named so the order sorts, alternating strands, none over the cap.
    for (index, oligo) in oligos.iter().enumerate() {
        assert_eq!(oligo["number"], index + 1);
        assert_eq!(oligo["name"], format!("mygene_{:02}", index + 1));
        assert_eq!(oligo["strand"], if index % 2 == 0 { "forward" } else { "reverse" });
        assert!(oligo["length"].as_i64().unwrap() <= 60);
        assert_eq!(oligo["length"].as_i64().unwrap(), oligo["sequence"].as_str().unwrap().len() as i64);
    }
}

/// The promise worth testing from the outside: order these oligos, and they build what was asked
/// for. Reassembled here by the shared stretch alone, the way the reaction finds them.
#[test]
fn the_ordered_oligos_build_the_target() {
    let (_dir, s) = session();
    for length in [200usize, 600, 1500] {
        let target = dna(length, 3);
        let out = design(&s, json!({ "sequence": &target }));
        let mut built = String::new();
        for (index, ordered) in strings(&out, "oligos").iter().enumerate() {
            let top = if index % 2 == 0 { ordered.clone() } else { revcomp(ordered) };
            if built.is_empty() {
                built = top;
                continue;
            }
            let shared = (18..=built.len().min(top.len())).rev().find(|n| built.ends_with(&top[..*n])).expect("a shared stretch");
            built.push_str(&top[shared..]);
        }
        assert_eq!(built, target, "length {length}");
    }
}

#[test]
fn every_junction_reports_its_temperature_and_the_reaction_gets_one_to_run_at() {
    let (_dir, s) = session();
    let out = design(&s, json!({ "sequence": dna(900, 5), "target_tm": 60 }));
    let overlaps = out["overlaps"].as_array().unwrap();
    assert_eq!(overlaps.len(), out["oligos"].as_array().unwrap().len() - 1);

    let anneal = out["anneal_tm"].as_f64().unwrap();
    for (index, overlap) in overlaps.iter().enumerate() {
        assert_eq!(overlap["joins"], format!("{} to {}", index + 1, index + 2));
        assert!(overlap["length"].as_i64().unwrap() >= 18);
        assert!(overlap["tm"].as_f64().unwrap() >= anneal, "junction {} is below the annealing temperature", index + 1);
    }
    assert!(anneal >= 50.0, "the floor sank to {anneal} °C");
}

/// The temperature is a condition that changes the design, not a label on it.
#[test]
fn asking_for_a_hotter_overlap_makes_the_overlaps_longer() {
    let (_dir, s) = session();
    let target = dna(900, 9);
    let cool = design(&s, json!({ "sequence": &target, "target_tm": 50 }));
    let hot = design(&s, json!({ "sequence": &target, "target_tm": 68 }));
    let mean = |v: &Value| {
        let lengths: Vec<i64> = v["overlaps"].as_array().unwrap().iter().map(|o| o["length"].as_i64().unwrap()).collect();
        lengths.iter().sum::<i64>() as f64 / lengths.len() as f64
    };
    assert!(mean(&hot) > mean(&cool), "68 °C overlaps ({}) should be longer than 50 °C ones ({})", mean(&hot), mean(&cool));
    assert!(hot["oligo_count"].as_i64().unwrap() > cool["oligo_count"].as_i64().unwrap(), "longer overlaps mean more oligos");
}

/// The other condition: a shorter oligo cap means more of them, and none over the cap.
#[test]
fn the_oligo_cap_is_obeyed_and_changes_how_many_there_are() {
    let (_dir, s) = session();
    let target = dna(900, 13);
    let short = design(&s, json!({ "sequence": &target, "max_length": 40 }));
    let long = design(&s, json!({ "sequence": &target, "max_length": 100 }));
    for (out, cap) in [(&short, 40i64), (&long, 100)] {
        for oligo in out["oligos"].as_array().unwrap() {
            assert!(oligo["length"].as_i64().unwrap() <= cap);
        }
    }
    assert!(short["oligo_count"].as_i64().unwrap() > long["oligo_count"].as_i64().unwrap());
}

#[test]
fn the_two_amplification_primers_sit_on_the_ends_of_the_target() {
    let (_dir, s) = session();
    let target = dna(500, 21);
    let out = design(&s, json!({ "sequence": &target, "name": "frag" }));
    let primers = out["amplification_primers"].as_array().unwrap();
    assert_eq!(primers.len(), 2);
    assert_eq!(primers[0]["name"], "frag_F");
    assert_eq!(primers[1]["name"], "frag_R");
    assert!(target.starts_with(primers[0]["sequence"].as_str().unwrap()));
    assert!(target.ends_with(&revcomp(primers[1]["sequence"].as_str().unwrap())));
}

#[test]
fn the_bill_is_every_base_ordered() {
    let (_dir, s) = session();
    let out = design(&s, json!({ "sequence": dna(600, 31) }));
    let oligos: i64 = out["oligos"].as_array().unwrap().iter().map(|o| o["length"].as_i64().unwrap()).sum();
    let primers: i64 = out["amplification_primers"].as_array().unwrap().iter().map(|p| p["length"].as_i64().unwrap()).sum();
    assert_eq!(out["total_bases_to_order"].as_i64().unwrap(), oligos + primers);
    assert!(out["total_bases_to_order"].as_i64().unwrap() > 600, "the overlaps are ordered twice");
}

/// A target built from one repeated unit cannot have unique junctions, and ordering it blind would
/// give a plasmid assembled in the wrong order. The answer has to say so before anyone pays.
#[test]
fn an_overlap_that_anneals_in_more_than_one_place_is_warned_about() {
    let (_dir, s) = session();
    let repeated = dna(100, 41).repeat(6);
    let out = design(&s, json!({ "sequence": repeated }));
    assert!(out["overlaps"].as_array().unwrap().iter().any(|o| o["elsewhere"].as_i64().unwrap() > 0));
    let warnings = out["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("wrong order")), "warnings were {warnings:?}");
}

#[test]
fn a_target_that_fits_one_oligo_is_one_oligo() {
    let (_dir, s) = session();
    let target = dna(45, 55);
    let out = design(&s, json!({ "sequence": &target }));
    assert_eq!(out["oligo_count"], 1);
    assert_eq!(out["oligos"][0]["sequence"], target);
    assert!(out["overlaps"].as_array().unwrap().is_empty());
}

#[test]
fn fasta_headers_and_whitespace_are_ignored() {
    let (_dir, s) = session();
    let target = dna(300, 61);
    let plain = design(&s, json!({ "sequence": &target }));
    let fasta = design(&s, json!({ "sequence": format!(">my gene\n{}\n{}\n", &target[..150], &target[150..]) }));
    assert_eq!(plain["oligos"], fasta["oligos"]);
}

#[test]
fn conditions_that_cannot_work_are_refused_with_the_reason() {
    let (_dir, s) = session();
    let target = dna(300, 71);

    let err = s.execute("design_assembly_oligos", &json!({ "sequence": &target, "max_length": 30 })).unwrap_err().to_string();
    assert!(err.contains("at least 36"), "{err}");

    let err = s.execute("design_assembly_oligos", &json!({ "sequence": &target, "target_tm": 200 })).unwrap_err().to_string();
    assert!(err.contains("between 20 and 90"), "{err}");

    let err = s.execute("design_assembly_oligos", &json!({ "sequence": "   " })).unwrap_err().to_string();
    assert!(err.contains("no bases"), "{err}");

    let err = s.execute("design_assembly_oligos", &json!({ "sequence": dna(40_000, 81) })).unwrap_err().to_string();
    assert!(err.contains("400"), "{err}");
}

/// The command only reads: it must not touch the project.
#[test]
fn nothing_is_saved() {
    let (_dir, s) = session();
    design(&s, json!({ "sequence": dna(400, 91) }));
    let plasmids = s.execute("list_plasmids", &json!({})).unwrap();
    assert!(plasmids["plasmids"].as_array().is_none_or(|p| p.is_empty()), "{plasmids:?}");
}
