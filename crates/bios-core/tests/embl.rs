//! The EMBL reader: the web app's `embl.test.ts` in its order with its numbers, then what the
//! web app's reader returned for the sample file and for a handful of records.
//!
//! The recorded answers (`pMINI_embl.snapshot.json`, `embl-cases.json`) are what the web app's
//! `readSequence` returned for the same text, recorded by running it.

use bios_core::embl::{record_to_genbank, split_records};
use bios_core::formats::{self, Format, Opened};
use bios_core::locations::normalize_location;
use bios_core::model::{Feature, Segment, Strand};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn open(text: &str, file_name: &str) -> Opened {
    formats::read(text.as_bytes(), file_name).unwrap_or_else(|why| panic!("{why}"))
}

const PLASMID: &str = r#"ID   pTEST; SV 1; circular; DNA; STD; SYN; 60 BP.
AC   X99999;
DE   a test plasmid
KW   cloning vector.
FH   Key             Location/Qualifiers
FT   CDS             join(complement(1..20),complement(31..50))
FT                   /gene="bla"
FT                   /label="AmpR"
FT                   /note="says ""hello"" here"
FT   promoter        5..15
FT                   /label="Ptest"
SQ   Sequence 60 BP; 15 A; 15 C; 15 G; 15 T; 0 other;
     atgcatgcat gcatgcatgc atgcatgcat gcatgcatgc atgcatgcat gcatgcatgc        60
//
"#;

// ─────────────────────────────── embl.test.ts, in its order ──────────────────────────────────

#[test]
fn an_embl_plasmid_reads_at_the_right_length_topology_and_feature_count() {
    let r = open(PLASMID, "pTEST.embl");
    assert_eq!(r.format, Format::Embl);
    assert_eq!(r.construct.len(), 60, "60 bp, not the header eaten as bases");
    // The field most likely to be wrong and most expensive when it is: a plasmid read as linear
    // is a different molecule.
    assert!(r.construct.circular, "circular came off the ID line");
    assert_eq!(r.construct.name, "pTEST");
    assert_eq!(r.construct.features.len(), 2);
}

#[test]
fn the_feature_table_crosses_intact_joins_strand_and_doubled_quotes() {
    let r = open(PLASMID, "pTEST.embl").construct;
    let amp = r.features.iter().find(|f| f.name == "AmpR").expect("the feature took its name from /label, as GenBank's reader does");
    assert_eq!(amp.strand, Strand::Reverse, "complement(...) is the bottom strand");
    assert_eq!(amp.locations().map(<[Segment]>::len), Some(2), "the join kept both segments");
    // join(complement(a),complement(b)) means RC(a) + RC(b), equivalent to RC(b + a). The stored
    // segments are assembled first, then complemented once.
    assert_eq!(amp.segments, [Segment::new(30, 49), Segment::new(0, 19)]);
    assert_eq!(amp.note("gene").unwrap(), ["bla"]);
    // A doubled quote is how INSDC escapes one inside a qualifier.
    assert_eq!(amp.note("note").unwrap(), ["says \"hello\" here"]);
}

#[test]
fn the_coordinate_moves_from_the_right_of_the_line_to_the_left() {
    // EMBL numbers at the END of a sequence line, GenBank at the START. Copying those lines
    // across unchanged shifts every base in the file, silently.
    let (gb, _) = record_to_genbank(PLASMID);
    let origin = gb[gb.find("ORIGIN").unwrap()..].split('\n').nth(1).unwrap();
    assert!(origin.starts_with("        1 atgcatgcat gcatgcatgc"), "coordinate first, then tens: {origin}");
    assert!(!gb.contains("atgcatgcat gcatgcatgc        60"), "no trailing count");
}

#[test]
fn a_linear_record_stays_linear_and_says_it_was_not_told() {
    let linear = "ID   frag; SV 1; linear; DNA; STD; SYN; 20 BP.
SQ   Sequence 20 BP;
     atgcatgcat gcatgcatgc                                                    20
//
";
    let r = open(linear, "frag.embl").construct;
    assert!(!r.circular);
    assert_eq!(r.len(), 20);
}

#[test]
fn a_multi_record_file_opens_the_first_and_says_how_many_there_were() {
    let two = format!("{PLASMID}{}", PLASMID.replace("pTEST", "pTEST2"));
    assert_eq!(split_records(&two).len(), 2);

    let r = open(&two, "library.embl");
    assert_eq!(r.construct.name, "pTEST", "the first one");
    // Silently opening one of several and saying nothing is how a person loses eleven constructs
    // and never finds out.
    assert!(r.notices.iter().any(|n| n.contains("2 sequences")), "{:?}", r.notices);
}

#[test]
fn the_legacy_whitespace_id_line_still_gives_up_a_name_and_a_topology() {
    let legacy = "ID   X56734  standard; circular DNA; PLN; 20 BP.
SQ   Sequence 20 BP;
     atgcatgcat gcatgcatgc                                                    20
//
";
    let (gb, _) = record_to_genbank(legacy);
    assert!(gb.starts_with("LOCUS       X56734"), "{gb}");
    assert!(gb.contains("circular"));
}

// ───────────────────────────────── read.test.ts, the EMBL test ───────────────────────────────

#[test]
fn a_30_bp_embl_record_is_not_silently_a_95_bp_construct() {
    // Before: success, 95 bp, 0 features: the ID/AC/DE/SQ lines eaten as bases.
    let embl = "ID   pTEST; SV 1; circular; DNA; STD; SYN; 30 BP.
AC   X99999;
DE   a test plasmid
SQ   Sequence 30 BP;
     atgcatgcat gcatgcatgc atgcatgcat                                       30
//
";
    let r = open(embl, "pTEST.embl");
    assert_eq!(r.format, Format::Embl);
    assert_eq!(r.construct.len(), 30, "30 bp, not 95");
    assert!(r.construct.circular);
}

// ───────────────────────────────────── recorded answers ──────────────────────────────────────

/// Checks what was opened against what the web app's reader returned for the same text.
fn assert_same(ours: &Opened, web: &Value, at: &str) {
    assert_eq!(web["ok"], true, "{at}");
    assert_eq!(web["format"], "embl", "{at}");
    assert_eq!(ours.format, Format::Embl, "{at}");
    let c = &ours.construct;
    assert_eq!(web["name"], c.name.as_str(), "{at}");
    assert_eq!(web["circular"], c.circular, "{at}");
    assert_eq!(web["sequence"].as_str().expect("sequence").to_uppercase(), c.sequence, "{at}");
    assert_eq!(web["comments"].as_array().expect("comments").iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(), c.comments, "{at}");
    assert_eq!(web["notices"].as_array().expect("notices").iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(), ours.notices, "{at}");

    let expected = web["features"].as_array().expect("features");
    assert_eq!(c.features.len(), expected.len(), "{at}");
    for (f, e) in c.features.iter().zip(expected) {
        let at = format!("{at}: {} {}..{}", f.name, f.start, f.end);
        assert_eq!(e["name"], f.name.as_str(), "{at}");
        assert_eq!(e["type"], f.kind.as_str(), "{at}");
        assert_eq!(e["strand"], i64::from(f.strand.sign()), "{at}");
        assert_eq!((e["start"].as_u64(), e["end"].as_u64()), (Some(f.start as u64), Some(f.end as u64)), "{at}");
        // The pieces as the rest of Bios sees what the web app holds.
        let pieces = |all: &Vec<Value>| all.iter().map(|p| Segment::new(p[0].as_u64().unwrap() as usize, p[1].as_u64().unwrap() as usize)).collect();
        let mut located = Feature {
            name: String::new(),
            kind: String::new(),
            start: f.start,
            end: f.end,
            strand: f.strand,
            segments: e["locations"].as_array().map_or_else(|| vec![Segment::new(f.start, f.end)], pieces),
            notes: Vec::new(),
            start_fuzzy: None,
            end_fuzzy: None,
            location_operator: None,
            location_unsupported: None,
        };
        normalize_location(&mut located, c.len(), c.circular);
        assert_eq!(f.segments, located.segments, "{at}");
        let notes: Vec<(String, Vec<String>)> = e["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .map(|pair| {
                (pair[0].as_str().unwrap().to_string(), pair[1].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect())
            })
            .collect();
        // One standing difference, in the GenBank reader both formats share: a feature with no
        // /label that is named from its /gene or /name keeps that qualifier here, and loses it in
        // the web app.
        let naming = |key: &str, values: &[String]| ["gene", "name"].contains(&key) && values.first() == Some(&f.name);
        let ours: Vec<(String, Vec<String>)> =
            f.notes.iter().filter(|(key, values)| !(naming(key, values) && !notes.iter().any(|(k, _)| k == key))).cloned().collect();
        assert_eq!(ours, notes, "{at}");
    }
}

#[test]
fn the_embl_sample_reads_as_the_web_app_reads_it() {
    let web: Value = serde_json::from_str(&fixture("pMINI_embl.snapshot.json")).expect("json");
    let ours = open(&fixture("pMINI_embl.embl"), "pMINI_embl.embl");
    assert_same(&ours, &web, "pMINI_embl.embl");
    assert_eq!(ours.construct.len(), 240);
    assert_eq!(ours.construct.features.len(), 2);
    // What the model holds beyond the web app's answer: the record's own header, as GenBank.
    assert_eq!(
        ours.construct.header,
        ["DEFINITION  A tiny EMBL plasmid, so the format Bios claims to read has a file", "            to prove it.", "KEYWORDS    cloning vector."]
    );
    assert_eq!(
        (ours.construct.locus.molecule.as_str(), ours.construct.locus.division.as_str(), ours.construct.locus.date.as_str()),
        ("DNA", "SYN", "")
    );
}

#[test]
fn embl_records_read_as_the_web_app_reads_them() {
    let cases: Value = serde_json::from_str(&fixture("embl-cases.json")).expect("json");
    let cases = cases.as_array().expect("cases");
    assert_eq!(cases.len(), 11);
    for case in cases {
        let label = case["label"].as_str().expect("label");
        let ours = open(case["text"].as_str().expect("text"), case["fileName"].as_str().expect("fileName"));
        assert_same(&ours, &case["web"], label);
    }
}

// ─────────────────────────────────────── what is refused ─────────────────────────────────────

#[test]
fn an_embl_file_with_no_record_or_no_bases_is_refused_in_a_sentence() {
    let refusal = "x.embl looks like EMBL but Bios couldn't read it. Nothing was opened. If it opens in the tool that wrote it, exporting a fresh GenBank usually fixes this.";
    for text in [
        "ID   X; SV 1; linear; DNA; STD; SYN; 0 BP.\nAC   X;\n//\n",
        "ID   X; SV 1; linear; DNA; STD; SYN; 0 BP.\nAC   X;\nSQ   Sequence 0 BP;\n//\n",
    ] {
        assert_eq!(formats::sniff(text.as_bytes()), Format::Embl);
        assert_eq!(formats::read(text.as_bytes(), "x.embl").unwrap_err(), refusal);
    }
}

#[test]
fn text_of_any_shape_never_crashes_the_reader() {
    let odd = [
        "ID   ",
        "ID\nAC\nSQ\n",
        "ID   é; circular\nAC   ü;\nFT   é\nFT\nSQ   \n     acgt é 1\n//\n//\n",
        "ID   x; SV 1; linear; DNA; STD; SYN; 4 BP.\nFT   CDS             join(1..2,\nSQ\n     acgt\n",
        "//\n//\n//\n",
    ];
    for text in odd {
        let _ = bios_core::embl::read(text);
        let _ = formats::read(text.as_bytes(), "odd.embl");
    }
    let unbroken = format!("ID   x; SV 1; linear; DNA; STD; SYN; 4 BP.\nDE   {}\nSQ   Sequence 4 BP;\n     acgt\n//\n", "é".repeat(400));
    let read = bios_core::embl::read(&unbroken).expect("reads");
    assert_eq!(read.construct.sequence, "ACGT");
    // A word too long for a line is broken, and every letter of it is kept.
    let definition: String =
        read.construct.header.iter().filter(|l| !l.starts_with("KEYWORDS")).map(|l| l.trim_start_matches("DEFINITION").trim()).collect();
    assert_eq!(definition, "é".repeat(400));
}
