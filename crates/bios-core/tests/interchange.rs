//! GFF3, JBEI SEQ XML and Teselagen JSON: named from their bytes, as the web's
//! packages/formats/src/read.test.ts names them, and read into constructs with their features.

use bios_core::formats::{Format, read, sniff};
use bios_core::interchange::{Json, parse_json};
use bios_core::model::Strand;

// read.test.ts, SNIFFS: the rows for the three formats, with the web's bytes.
#[test]
fn sniff_names_gff_and_json_from_their_bytes() {
    assert_eq!(sniff(b"##gff-version 3\nchr1\t.\tgene\t1\t10\t.\t+\t.\tID=a\n"), Format::Gff);
    assert_eq!(sniff(b"{\"sequence\":\"atgc\"}"), Format::Json);
    // sniff.ts: XML is JBEI when it says so, and otherwise text.
    assert_eq!(sniff(b"<?xml version=\"1.0\"?>\n<seq:seq xmlns:seq=\"http://jbei.org/sequence\"></seq:seq>"), Format::Jbei);
    assert_eq!(sniff(b"<html><body>not a plasmid</body></html>"), Format::Text);
}

const GFF: &str = "##gff-version 3\n\
##sequence-region pDemo 1 40\n\
pDemo\t.\tregion\t1\t40\t.\t+\t.\tID=pDemo;Is_circular=true\n\
pDemo\tBios\tpromoter\t3\t12\t.\t+\t.\tID=p1;Name=lac%20promoter;note=strong\n\
pDemo\tBios\tCDS\t15\t36\t.\t-\t0\tID=c1;Name=tiny\n\
other\tBios\tgene\t1\t5\t.\t+\t.\tID=elsewhere\n\
##FASTA\n\
>pDemo\n\
atgcatgcatgcatgcatgc\n\
atgcatgcatgcatgcatgc\n\
>other\n\
gggggggggg\n";

#[test]
fn gff3_reads_its_features_on_the_sequence_after_fasta() {
    let opened = read(GFF.as_bytes(), "demo.gff3").unwrap();
    assert_eq!(opened.format, Format::Gff);
    let c = opened.construct;
    assert_eq!(c.name, "pDemo");
    assert_eq!(c.len(), 40);
    assert!(c.circular, "Is_circular on the region");
    assert_eq!(c.features.len(), 2, "the whole-sequence region and the other sequence's gene are not features of it");
    let promoter = &c.features[0];
    assert_eq!((promoter.name.as_str(), promoter.kind.as_str(), promoter.start, promoter.end), ("lac promoter", "promoter", 2, 11));
    assert_eq!(promoter.note("note"), Some(&["strong".to_string()][..]));
    assert_eq!((c.features[1].start, c.features[1].end, c.features[1].strand), (14, 35, Strand::Reverse));
}

#[test]
fn gff3_without_bases_is_refused_in_a_sentence() {
    let refused = read(b"##gff-version 3\nchr1\t.\tgene\t1\t10\t.\t+\t.\tID=a\n", "notes.gff").unwrap_err();
    assert!(refused.starts_with("notes.gff is GFF3 without a sequence."), "{refused}");
}

const JBEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<seq:seq xmlns:seq="http://jbei.org/sequence" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <seq:name>pJBEI &amp; co</seq:name>
  <seq:circular>true</seq:circular>
  <seq:sequence>atgcatgcatgcatgcatgcatgcatgcatgc</seq:sequence>
  <seq:features>
    <seq:feature>
      <seq:label>gfp</seq:label>
      <seq:complement>false</seq:complement>
      <seq:type>CDS</seq:type>
      <seq:location><seq:genbankStart>3</seq:genbankStart><seq:end>20</seq:end></seq:location>
    </seq:feature>
    <seq:feature>
      <seq:label>split</seq:label>
      <seq:complement>true</seq:complement>
      <seq:type>misc_feature</seq:type>
      <seq:location><seq:genbankStart>22</seq:genbankStart><seq:end>25</seq:end></seq:location>
      <seq:location><seq:genbankStart>28</seq:genbankStart><seq:end>30</seq:end></seq:location>
    </seq:feature>
  </seq:features>
</seq:seq>"#;

#[test]
fn jbei_seq_xml_reads_its_name_shape_bases_and_features() {
    let opened = read(JBEI.as_bytes(), "p.xml").unwrap();
    assert_eq!(opened.format, Format::Jbei);
    let c = opened.construct;
    assert_eq!((c.name.as_str(), c.circular, c.len()), ("pJBEI & co", true, 32));
    assert_eq!(c.features.len(), 2);
    assert_eq!((c.features[0].name.as_str(), c.features[0].kind.as_str(), c.features[0].start, c.features[0].end), ("gfp", "CDS", 2, 19));
    let split = &c.features[1];
    assert_eq!((split.start, split.end, split.strand, split.segments.len()), (21, 29, Strand::Reverse, 2));
}

#[test]
fn teselagen_json_reads_the_first_sequence_with_its_features() {
    let text = r#"[{"name":"pTes","circular":false,"sequence":"ATGCATGCATGCATGCATGC",
        "features":{"f1":{"name":"ori","type":"rep_origin","start":2,"end":9,"strand":-1,"notes":{"note":["kept"]}},
                    "f2":{"name":"tag","type":"misc_feature","start":10,"end":12,"forward":true},
                    "bad":{"name":"past the end","start":30,"end":40}}},
        {"name":"second","sequence":"GGGG"}]"#;
    let opened = read(text.as_bytes(), "seqs.json").unwrap();
    assert_eq!(opened.format, Format::Json);
    assert_eq!(opened.notices, ["seqs.json holds 2 sequences. Bios opened the first."]);
    let c = opened.construct;
    assert_eq!((c.name.as_str(), c.circular, c.len()), ("pTes", false, 20));
    assert_eq!(c.features.len(), 2, "a feature past the end of the bases is dropped");
    assert_eq!((c.features[0].start, c.features[0].end, c.features[0].strand), (2, 9, Strand::Reverse));
    assert_eq!(c.features[0].note("note"), Some(&["kept".to_string()][..]));
    assert_eq!(c.features[1].strand, Strand::Forward);
}

#[test]
fn json_that_is_not_a_sequence_is_refused_and_bad_json_never_panics() {
    let refused = read(b"{\"name\":\"settings\",\"theme\":\"dark\"}", "prefs.json").unwrap_err();
    assert!(refused.contains("not a sequence Bios knows"), "{refused}");
    for bad in ["{", "[1,2", "{\"a\":}", "\"unterminated", "{\"a\":1}x", &"[".repeat(10_000)] {
        assert!(read(bad.as_bytes(), "x.json").is_err(), "{bad:.20}");
    }
    assert_eq!(
        parse_json(r#"{"a":[1,-2.5e1,true,null,"é\n"]}"#).and_then(|j| j.get("a").cloned()),
        Some(Json::List(vec![Json::Number(1.0), Json::Number(-25.0), Json::Bool(true), Json::Null, Json::Text("é\n".into())]))
    );
}
