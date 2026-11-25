//! `dna_amount`, `protein_stats` and `genetic_codes` through `Session::execute`, and the genetic
//! code `translate` and `find_orfs` now take.
//!
//! The numbers come from the same places as the recorded answers in `bios-core/tests/quantities.rs`:
//! UniProt P62593 for the TEM-1 beta-lactamase weight, the published oligonucleotide mass formula,
//! and arithmetic from a constant stated here.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None }))
}

/// A session holding the pUC19 fixture, as an agent importing it would have.
fn with_puc19() -> (tempfile::TempDir, Session) {
    let (dir, s) = session();
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures/pUC19.gb").display().to_string();
    s.execute("import_plasmid", &json!({ "file": file })).unwrap();
    (dir, s)
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or_else(|| panic!("not a number: {value}"))
}

#[test]
fn genetic_codes_lists_every_table_with_the_codons_it_begins_and_ends_at() {
    let (_dir, s) = session();
    let listed = s.execute("genetic_codes", &json!({})).unwrap();
    assert_eq!(listed["total"], 27);
    assert_eq!(listed["default"], 1);
    let codes = listed["codes"].as_array().unwrap();
    assert_eq!(codes.len(), 27);
    assert_eq!(codes.first().unwrap()["code"], 1);
    assert_eq!(codes.first().unwrap()["name"], "Standard");
    assert_eq!(codes.first().unwrap()["stop_codons"], json!(["TAA", "TAG", "TGA"]));
    let mito = codes.iter().find(|c| c["code"] == 2).unwrap();
    assert_eq!(mito["name"], "Vertebrate Mitochondrial");
    assert_eq!(mito["stop_codons"], json!(["TAA", "TAG", "AGA", "AGG"]));
    assert_eq!(mito["start_codons"], json!(["ATT", "ATC", "ATA", "ATG", "GTG"]));
    // The long NCBI names wrap in the table and must come out on one line.
    let mould = codes.iter().find(|c| c["code"] == 4).unwrap();
    assert!(!mould["name"].as_str().unwrap().contains('\n'), "{}", mould["name"]);
    // An id that is not a table is refused by name, not quietly read as the standard code.
    assert!(s.execute("translate", &json!({ "sequence": "ATG", "code": 7 })).unwrap_err().0.contains("not an NCBI genetic code"));
}

#[test]
fn translating_with_the_mitochondrial_code_reads_tga_as_tryptophan() {
    let (_dir, s) = session();
    // TGA is a stop in the standard code and a tryptophan in code 2, where AGA stops instead.
    let standard = s.execute("translate", &json!({ "sequence": "ATGTGAAGATAA" })).unwrap();
    assert_eq!((standard["protein"].as_str(), standard["code"].as_u64()), (Some("M*R*"), Some(1)));
    let mito = s.execute("translate", &json!({ "sequence": "ATGTGAAGATAA", "code": 2 })).unwrap();
    assert_eq!((mito["protein"].as_str(), mito["code"].as_u64()), (Some("MW**"), Some(2)));
    assert_eq!(mito["code_name"], "Vertebrate Mitochondrial");
    // Code 6 reads the standard stops TAA and TAG as glutamine.
    assert_eq!(s.execute("translate", &json!({ "sequence": "ATGTAATAG", "code": 6 })).unwrap()["protein"], "MQQ");
    // Still the bottom strand when asked for it.
    assert_eq!(s.execute("translate", &json!({ "sequence": "TTATCACAT", "frame": -1, "code": 2 })).unwrap()["protein"], "MW*");
}

#[test]
fn an_orf_stops_where_the_code_asked_for_stops() {
    let (_dir, s) = session();
    let orf = "ATGGCCTGAGCCAGACCC";
    s.execute("create_plasmid", &json!({ "name": "mito", "sequence": orf, "circular": false })).unwrap();
    let standard = s.execute("find_orfs", &json!({ "plasmid": "mito", "min_aa": 1 })).unwrap();
    assert_eq!(standard["code"], 1);
    assert_eq!(standard["stop_codons"], json!(["TAA", "TAG", "TGA"]));
    assert_eq!(standard["orfs"], json!([{ "start": 1, "end": 9, "strand": "forward", "aa": 2, "protein": "MA*" }]));
    let mito = s.execute("find_orfs", &json!({ "plasmid": "mito", "min_aa": 1, "code": 2 })).unwrap();
    assert_eq!(mito["code_name"], "Vertebrate Mitochondrial");
    assert_eq!(mito["stop_codons"], json!(["TAA", "TAG", "AGA", "AGG"]));
    assert_eq!(mito["orfs"], json!([{ "start": 1, "end": 15, "strand": "forward", "aa": 4, "protein": "MAWA*" }]));
    assert!(s.execute("find_orfs", &json!({ "plasmid": "mito", "code": 99 })).unwrap_err().0.contains("not an NCBI genetic code"));
}

#[test]
fn an_alternative_start_is_only_used_when_it_is_asked_for() {
    let (_dir, s) = session();
    // CTG initiates in the standard code, five codons upstream of the ATG in the same frame.
    s.execute("create_plasmid", &json!({ "name": "ctg", "sequence": "CTGGCCGCCGCCGCCATGGCCGCCGCCGCCTAA", "circular": false })).unwrap();
    let plain = s.execute("find_orfs", &json!({ "plasmid": "ctg", "min_aa": 2 })).unwrap();
    assert_eq!(plain["start_codons"], json!(["ATG"]));
    assert_eq!(plain["orfs"], json!([{ "start": 16, "end": 33, "strand": "forward", "aa": 5, "protein": "MAAAA*" }]));
    let wider = s.execute("find_orfs", &json!({ "plasmid": "ctg", "min_aa": 2, "alternative_starts": true })).unwrap();
    assert_eq!(wider["start_codons"], json!(["TTG", "CTG", "ATG"]));
    assert_eq!(wider["orfs"], json!([{ "start": 1, "end": 33, "strand": "forward", "aa": 10, "protein": "LAAAAMAAAA*" }]));
}

#[test]
fn a_hundred_nanograms_of_a_fragment_comes_back_as_picomoles_and_molecules() {
    let (_dir, s) = session();
    let said = s.execute("dna_amount", &json!({ "length_bp": 1200, "ng": 100 })).unwrap();
    // 1200 bp at the conventional 650 g/mol per base pair.
    assert_eq!(said["molecular_weight_g_per_mol"], 780_000.0);
    assert_eq!(said["molecular_weight_from"], "a conventional average, because only a length was given");
    assert_eq!(said["strands"], "double");
    assert_eq!(said["length_unit"], "base pairs");
    // 100 / 780000 x 1000 = 0.1282 pmol, and that many molecules is 7.721e10.
    assert_eq!(said["have"]["pmol"], 0.1282);
    assert_eq!(said["have"]["ng"], 100.0);
    assert_eq!((number(&said["have"]["molecules"]) / 1e9).round(), 77.0);
    assert_eq!(said["ng_per_pmol"], 780.0);
    assert!(said["formulas"].as_array().unwrap().iter().any(|f| f.as_str().unwrap().contains("650 g/mol per base pair")));
    // Picomoles the other way round give the nanograms back.
    let back = s.execute("dna_amount", &json!({ "length_bp": 1200, "pmol": 0.128205 })).unwrap();
    assert_eq!(back["have"]["ng"], 100.0);
    // And so do molecules.
    let counted = s.execute("dna_amount", &json!({ "length_bp": 1200, "molecules": 7.7210e10 })).unwrap();
    assert_eq!(counted["have"]["pmol"], 0.1282);
}

#[test]
fn a_real_sequence_is_weighed_from_its_bases_not_from_the_average() {
    let (_dir, s) = with_puc19();
    let said = s.execute("dna_amount", &json!({ "plasmid": "pUC19", "ng": 50 })).unwrap();
    assert_eq!(said["of"], "pUC19");
    assert_eq!(said["length"], 2686);
    assert_eq!(said["molecular_weight_from"], "the base composition of the sequence given");
    // Added up base by base: 1,659,570 g/mol, below the 2686 x 650 = 1,745,900 the average gives.
    assert_eq!(said["molecular_weight_g_per_mol"], 1_659_570.0);
    assert!(number(&said["molecular_weight_g_per_mol"]) < 2686.0 * 650.0);
    assert_eq!(said["have"]["pmol"], 0.03013);
    // A range of it is weighed the same way: AmpR's CDS is 861 bp.
    let part = s.execute("dna_amount", &json!({ "plasmid": "pUC19", "start": 1626, "end": 2486, "ng": 200, "volume_ul": 20 })).unwrap();
    assert_eq!(part["of"], "pUC19 1626..2486");
    assert_eq!(part["length"], 861);
    assert_eq!(part["in_volume"]["ng_per_ul"], 10.0);
    // 0.376 pmol in 20 uL is 0.0188 uM.
    assert_eq!(part["have"]["pmol"], 0.376);
    assert_eq!(part["in_volume"]["um"], 0.0188);
    assert_eq!(part["in_volume"]["nm"], 18.8);
}

#[test]
fn an_oligo_is_one_strand_and_resuspends_to_the_concentration_asked_for() {
    let (_dir, s) = session();
    // The M13 forward primer, 24 bases. The published formula gives 7380.9 g/mol for one strand.
    let oligo = "GTAAAACGACGGCCAGTGCCAAGC";
    let said = s.execute("dna_amount", &json!({ "sequence": oligo, "single_stranded": true, "pmol": 33_400 })).unwrap();
    assert_eq!(said["strands"], "single");
    assert_eq!(said["length_unit"], "bases");
    assert_eq!(said["length"], 24);
    assert!((number(&said["molecular_weight_g_per_mol"]) - 7380.9).abs() < 0.2, "{}", said["molecular_weight_g_per_mol"]);
    // Double-stranded, the same bases weigh about twice as much.
    let duplex = s.execute("dna_amount", &json!({ "sequence": oligo, "pmol": 1 })).unwrap();
    assert!((number(&duplex["molecular_weight_g_per_mol"]) / number(&said["molecular_weight_g_per_mol"]) - 2.0).abs() < 0.01);
    // 33.4 nmol to 100 uM wants 334 uL.
    let resuspended = s.execute("dna_amount", &json!({ "sequence": oligo, "single_stranded": true, "pmol": 33_400, "target_um": 100 })).unwrap();
    assert_eq!(resuspended["resuspend"], json!({ "to_um": 100.0, "add_ul": 334.0 }));
}

#[test]
fn a_ligation_at_three_to_one_says_how_much_of_each_to_add() {
    let (_dir, s) = with_puc19();
    let said = s.execute("dna_amount", &json!({ "plasmid": "pUC19", "ng": 50, "insert_length_bp": 1200, "molar_ratio": 3 })).unwrap();
    let ligation = &said["ligation"];
    assert_eq!(ligation["molar_ratio_insert_to_vector"], 3.0);
    assert_eq!(ligation["vector"]["have"]["pmol"], 0.03013);
    // Three times the vector's moles, at 780,000 g/mol: 0.09038 pmol = 70.5 ng.
    assert_eq!(ligation["insert"]["add"]["pmol"], 0.09038);
    assert_eq!(ligation["insert"]["add"]["ng"], 70.5);
    assert_eq!(ligation["insert"]["length"], 1200);
    // The default ratio is the usual 3:1.
    let default = s.execute("dna_amount", &json!({ "plasmid": "pUC19", "ng": 50, "insert_length_bp": 1200 })).unwrap();
    assert_eq!(default["ligation"]["insert"]["add"]["ng"], 70.5);
    // An insert given as a sequence is weighed from its bases.
    let sequenced = s
        .execute("dna_amount", &json!({ "plasmid": "pUC19", "ng": 50, "insert_plasmid": "pUC19", "insert_start": 1626, "insert_end": 2486 }))
        .unwrap();
    assert_eq!(sequenced["ligation"]["insert"]["of"], "pUC19 1626..2486");
    assert_eq!(sequenced["ligation"]["insert"]["molecular_weight_from"], "the base composition of the sequence given");
}

#[test]
fn dna_amount_refuses_a_question_it_cannot_answer() {
    let (_dir, s) = session();
    assert!(s.execute("dna_amount", &json!({ "ng": 50 })).unwrap_err().0.contains("say what the DNA is"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "ng": 50, "pmol": 1 })).unwrap_err().0.contains("only one of ng, pmol"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "ng": -1 })).unwrap_err().0.contains("cannot be negative"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 0, "ng": 1 })).unwrap_err().0.contains("1 or more"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "volume_ul": 10 })).unwrap_err().0.contains("say how much DNA you have"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "ng": 1, "volume_ul": 0 })).unwrap_err().0.contains("more than zero"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "ng": 1, "molar_ratio": 3 })).unwrap_err().0.contains("give the insert"));
    assert!(s.execute("dna_amount", &json!({ "length_bp": 100, "insert_length_bp": 50 })).unwrap_err().0.contains("say how much vector you have"));
}

#[test]
fn the_ampr_protein_of_puc19_weighs_what_its_entry_publishes() {
    let (_dir, s) = with_puc19();
    let said = s.execute("protein_stats", &json!({ "plasmid": "pUC19", "feature": "AmpR" })).unwrap();
    assert_eq!(said["of"], "pUC19 AmpR");
    assert_eq!(said["residues"], 286);
    assert_eq!(said["ended_at_a_stop"], true);
    assert_eq!(said["unknown_residues"], "");
    let protein = said["protein"].as_str().unwrap();
    assert!(protein.starts_with("MSIQHFRVALIPFFAAFCLPVFAHPETLVK"), "{protein}");
    assert!(protein.ends_with("RNRQIAEIGASLIKHW"), "{protein}");
    // UniProt P62593 (beta-lactamase TEM-1) publishes "286 AA; 31515 MW". pUC19's copy differs at
    // two residues, V82I (one CH2, 14.03) and A182V (one C2H4, 28.05), so it weighs 42.08 more.
    let weight = number(&said["molecular_weight_da"]);
    assert!((weight - (31515.0 + 42.08)).abs() < 0.5, "{weight} against UniProt's 31515 plus 42.08");
    assert_eq!(said["molecular_weight_kda"], 31.56);
    // Four tryptophans and four tyrosines: 4 x 5500 + 4 x 1490 (Pace et al. 1995), and the three
    // cysteines make at most one cystine, worth another 125.
    assert_eq!((said["tryptophan"].as_u64(), said["tyrosine"].as_u64(), said["cysteine"].as_u64()), (Some(4), Some(4), Some(3)));
    assert_eq!(said["extinction_280_m1cm1"]["cysteines_reduced"], 27_960.0);
    assert_eq!(said["extinction_280_m1cm1"]["all_cysteines_as_cystines"], 28_085.0);
    // An A280 of 1 on a 1 cm path is 27960 / 31557 = 0.886 mg/mL of this protein.
    assert_eq!(said["a280_of_1_mg_per_ml"], 0.886);
    // Every residue is counted once, and the counts add up to the length.
    let composition = said["composition"].as_array().unwrap();
    assert_eq!(composition.len(), 20);
    assert_eq!(composition.iter().map(|r| r["count"].as_u64().unwrap()).sum::<u64>(), 286);
    assert_eq!(composition.first().unwrap()["residue"], "A");
    assert_eq!(composition.first().unwrap()["count"], 28);
    // No isoelectric point: Bios has no answer it can check.
    assert!(said.get("isoelectric_point").is_none());
    assert!(said.get("pi").is_none());
}

#[test]
fn a_protein_can_come_from_a_range_a_frame_or_the_residues_themselves() {
    let (_dir, s) = with_puc19();
    // AmpR's CDS is complement(1626..2486), so frame -1 of that range is the same protein.
    let ranged = s.execute("protein_stats", &json!({ "plasmid": "pUC19", "start": 1626, "end": 2486, "frame": -1 })).unwrap();
    assert_eq!(ranged["residues"], 286);
    assert_eq!(ranged["of"], "pUC19 1626..2486 frame -1");
    let feature = s.execute("protein_stats", &json!({ "plasmid": "pUC19", "feature": "AmpR" })).unwrap();
    assert_eq!(ranged["protein"], feature["protein"]);
    assert_eq!(ranged["molecular_weight_da"], feature["molecular_weight_da"]);
    // Residues given directly: Met 131.193 + Trp 186.214 + one water 18.015 = 335.422.
    let given = s.execute("protein_stats", &json!({ "protein": "MW*" })).unwrap();
    assert_eq!(given["residues"], 2);
    assert_eq!(given["molecular_weight_da"], 335.422);
    assert_eq!(given["ended_at_a_stop"], true);
    assert_eq!(given["extinction_280_m1cm1"]["cysteines_reduced"], 5500.0);
    // A residue outside the twenty leaves no weight to give, and is named.
    let ragged = s.execute("protein_stats", &json!({ "protein": "MXW" })).unwrap();
    assert_eq!(ragged["unknown_residues"], "X");
    assert_eq!(ragged["molecular_weight_da"], Value::Null);
    assert_eq!(ragged["a280_of_1_mg_per_ml"], Value::Null);
    assert!(s.execute("protein_stats", &json!({ "protein": "*" })).unwrap_err().0.contains("no residues"));
    assert!(s.execute("protein_stats", &json!({})).unwrap_err().0.contains("say where the protein comes from"));
}

#[test]
fn a_cds_is_translated_with_its_own_table_unless_a_code_is_named() {
    let (_dir, s) = session();
    // ATG TGA GCC TAA on its own strand: a stop in the standard code, a tryptophan in code 2.
    let made = s.execute("create_plasmid", &json!({ "name": "cds", "sequence": "ATGTGAGCCTAA", "circular": false })).unwrap();
    let added = s
        .execute("add_feature", &json!({ "plasmid": "cds", "version": made["version"], "name": "gene", "start": 1, "end": 12, "type": "CDS" }))
        .unwrap();
    assert_eq!(s.execute("protein_stats", &json!({ "plasmid": "cds", "feature": "gene" })).unwrap()["protein"], "M*A");
    // Its own transl_table is honoured without the caller saying anything.
    s.execute("update_feature", &json!({ "plasmid": "cds", "version": added["version"], "feature": "gene", "qualifiers": { "transl_table": "2" } }))
        .unwrap();
    let mito = s.execute("protein_stats", &json!({ "plasmid": "cds", "feature": "gene" })).unwrap();
    assert_eq!((mito["protein"].as_str(), mito["code"].as_u64()), (Some("MWA"), Some(2)));
    assert_eq!(mito["code_name"], "Vertebrate Mitochondrial");
    // And a code the caller names wins over the qualifier.
    let asked = s.execute("protein_stats", &json!({ "plasmid": "cds", "feature": "gene", "code": 1 })).unwrap();
    assert_eq!((asked["protein"].as_str(), asked["code"].as_u64()), (Some("M*A"), Some(1)));
}
