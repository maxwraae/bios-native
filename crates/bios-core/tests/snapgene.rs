//! The SnapGene reader against what the Bios web app reads from the same bytes.
//!
//! Three kinds of test, in this order:
//!
//! 1. Recorded answers. `cases.json` holds some three hundred small containers made up to cover
//!    what a real file may hold and what a damaged or hostile one may. Each answer is what the
//!    web app's `readSequence` returned for those bytes, recorded by running it; nothing in them
//!    was written by hand. Files SnapGene itself wrote are not kept in this repository (its own
//!    files are not ours to learn from or publish); the tests on them, at the end of this file,
//!    read them from `BIOS_PRIVATE_FIXTURES` and are skipped without it.
//! 2. The web app's own tests, ported in their order with their numbers:
//!    `snapgenePrimers.test.ts`, then `snapgeneFeatures.test.ts`.
//! 3. Where Bios differs from the web app on purpose, and what it does with a damaged file.
//!
//! What "the same" means, for everything the model holds:
//!
//! - The name, the topology and the sequence (the model holds bases in capitals).
//! - Each feature's name, type, start, end and strand.
//! - Its pieces, as [`normalize_location`] gives them for what the web app holds: that is the
//!   one shape the rest of Bios sees, so a plain two-piece join through the origin is one span
//!   in both.
//! - Its qualifiers, in order. After them come the ones the web app's GenBank export writes for
//!   what it holds outside the qualifiers (`direction`, `color`), recorded from that export.
//! - The comments, the notices, and the description as the `DEFINITION` line.

use bios_core::formats::{self, Format};
use bios_core::genbank_write;
use bios_core::locations::normalize_location;
use bios_core::model::{Construct, Feature, Segment, Strand};
use bios_core::snapgene::{self, PRIMER_ENCODING, PRIMER_QUALIFIER, ReadError, UNPLACED_COMMENT};
use serde_json::{Value, json};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/snapgene/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn recorded(name: &str) -> Value {
    serde_json::from_slice(&fixture(name)).expect("json")
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("a list").iter().map(|v| v.as_str().expect("a string").to_string()).collect()
}

/// The feature as the web app holds it, put into the shape the rest of Bios sees. Pieces that do
/// not lie on the sequence are the one thing not taken from the web app (see the third part of
/// this file): such a feature is its span.
///
/// `None` when the web app places the feature itself off the sequence.
fn expected_location(web: &Value, length: usize, circular: bool) -> Option<Feature> {
    let at = |v: &Value| v.as_f64().filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n < length as f64).map(|n| n as usize);
    let (start, end) = (at(&web["start"])?, at(&web["end"])?);
    let pieces: Option<Vec<Segment>> = web["locations"].as_array().and_then(|all| {
        all.iter()
            .map(|piece| {
                let (from, to) = (at(&piece[0])?, at(&piece[1])?);
                (to >= from || circular).then_some(Segment::new(from, to))
            })
            .collect()
    });
    let mut feature = Feature {
        name: String::new(),
        kind: String::new(),
        start,
        end,
        strand: Strand::Forward,
        segments: pieces.unwrap_or_else(|| vec![Segment::new(start, end)]),
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    normalize_location(&mut feature, length, circular);
    Some(feature)
}

/// Every way `ours` is not what the web app returned for the same bytes.
fn differences(ours: &Result<snapgene::Read, ReadError>, web: &Value) -> Vec<String> {
    let mut wrong = Vec::new();
    let mut check = |what: &str, same: bool, detail: String| {
        if !same {
            wrong.push(format!("{what}: {detail}"));
        }
    };
    let opened = web["ok"].as_bool().expect("ok");
    // The web app opens a file with no bases as an empty construct. Bios never holds one.
    let empty = opened && web["sequence"].as_str().is_some_and(str::is_empty);
    let ours = match ours {
        Ok(read) if opened && !empty => read,
        Ok(read) => return vec![format!("opened as {} bp where the web app refuses or has no bases", read.construct.len())],
        Err(ReadError::NoSequence) if empty => return wrong,
        Err(why) if opened => return vec![format!("refused ({why}) where the web app opens it")],
        Err(_) => return wrong,
    };
    let c = &ours.construct;
    // A label that is a plain number is one to the web app. Its text is the same.
    let name = web["name"].as_str().map(str::to_string).or_else(|| web["name"].as_u64().map(|n| n.to_string()));
    check("name", name.as_deref() == Some(c.name.as_str()), format!("{:?} for {}", c.name, web["name"]));
    check("circular", web["circular"] == c.circular, format!("{}", c.circular));
    check("sequence", web["sequence"].as_str().is_some_and(|s| s.to_uppercase() == c.sequence), format!("{} bp for {}", c.len(), web["length"]));
    check("comments", strings(&web["comments"]) == c.comments, format!("{:?}", c.comments));
    check("notices", strings(&web["notices"]) == ours.notices, format!("{:?} for {}", ours.notices, web["notices"]));
    let description = web["description"].as_str().filter(|d| !d.is_empty());
    let definition: Vec<String> = description.map(|d| format!("DEFINITION  {}", d.replace('\n', " "))).into_iter().collect();
    check("description", definition == c.header, format!("{:?} for {}", c.header, web["description"]));

    let features = web["features"].as_array().expect("features");
    check("feature count", features.len() == c.features.len(), format!("{} for {}", c.features.len(), features.len()));
    for (i, (f, e)) in c.features.iter().zip(features).enumerate() {
        let at = format!("feature {i} ({})", f.name);
        check(&at, e["name"] == f.name.as_str(), format!("name for {}", e["name"]));
        check(&at, e["type"] == f.kind.as_str(), format!("type {:?} for {}", f.kind, e["type"]));
        check(&at, e["strand"] == i64::from(f.strand.sign()), format!("strand {} for {}", f.strand.sign(), e["strand"]));
        match expected_location(e, c.len(), c.circular) {
            Some(located) => {
                check(&at, (f.start, f.end) == (located.start, located.end), format!("{}..{} for {}..{}", f.start, f.end, e["start"], e["end"]));
                check(&at, f.segments == located.segments, format!("pieces {:?} for {:?} ({})", f.segments, located.segments, e["locations"]));
            }
            None => check(&at, false, format!("the web app places it at {}..{}, off the sequence", e["start"], e["end"])),
        }

        let mut notes: Vec<(String, Vec<String>)> =
            e["notes"].as_array().expect("notes").iter().map(|pair| (pair[0].as_str().expect("key").to_string(), strings(&pair[1]))).collect();
        match e["exported"].as_array() {
            Some(extra) => {
                notes.extend(
                    extra.iter().map(|pair| (pair[0].as_str().expect("key").to_string(), vec![pair[1].as_str().expect("value").to_string()])),
                );
                check(&at, f.notes == notes, format!("qualifiers {:?} for {notes:?}", f.notes));
            }
            // The web app could not write this one as GenBank, so there is no export to compare.
            None => check(&at, f.notes.starts_with(&notes), format!("qualifiers {:?} for {notes:?}", f.notes)),
        }
    }
    wrong
}

// ───────────────────────────────────────────── recorded answers ──────────────────────────────

/// Cases where Bios does not do what the web app does, each with its reason. The third part of
/// this file says what Bios does instead.
const DIFFERS: [(&str, &str); 4] = [
    ("bases among other bytes, with a feature", "the web app places the feature past the end of the bases it kept"),
    ("notes: numeric label", "the web app reads 007 as the number 7 and drops a description that is a number"),
    ("notes: boolean label", "the web app names the construct the boolean true"),
    ("notes: label twice", "the web app names the construct a list of both labels"),
];

#[test]
fn made_up_containers_read_as_the_web_app_reads_them() {
    let cases = recorded("cases.json");
    let cases = cases.as_array().expect("cases");
    assert!(cases.len() > 250, "{} cases", cases.len());
    let mut failures = Vec::new();
    let mut compared = 0;
    for case in cases {
        let label = case["label"].as_str().expect("label");
        let bytes = unhex(case["hex"].as_str().expect("hex"));
        let ours = snapgene::read(&bytes, case["fileName"].as_str().expect("fileName"));
        let found = differences(&ours, &case["web"]);
        if DIFFERS.iter().any(|(differs, _)| *differs == label) {
            // A case that has come to agree with the web app is no longer an exception.
            assert!(!found.is_empty(), "{label} is named in DIFFERS and does not differ");
            continue;
        }
        compared += 1;
        failures.extend(found.into_iter().map(|difference| format!("{label}: {difference}")));
    }
    assert!(failures.is_empty(), "{} differences:\n{}", failures.len(), failures.join("\n"));
    assert_eq!(compared + DIFFERS.len(), cases.len(), "a case named in DIFFERS is not in cases.json");
}

// ───────────────────────── snapgenePrimers.test.ts, in its order ─────────────────────────────

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

/// Decodes base64, in either alphabet, padded or not.
fn unbase64(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            other => panic!("not base64: {other}"),
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

fn packet(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend((payload.len() as u32).to_be_bytes());
    out.extend(payload);
    out
}

fn cookie() -> Vec<u8> {
    packet(9, b"SnapGene\x00\x01\x00\x01\x00\x01")
}

/// `synthetic` in the web app's test: sixteen bases, an optional features block, a primers block.
fn synthetic(primers: &[u8], circular: bool, features: &str) -> Vec<u8> {
    let mut dna = vec![u8::from(circular)];
    dna.extend(b"ACGTACGTACGTACGT");
    let mut out = cookie();
    out.extend(packet(0, &dna));
    if !features.is_empty() {
        out.extend(packet(10, features.as_bytes()));
    }
    out.extend(packet(5, primers));
    out
}

fn open(content: &[u8]) -> formats::Opened {
    formats::read(content, "primer-test.dna").unwrap_or_else(|why| panic!("{why}"))
}

/// Writes the construct as GenBank and reads that back, as a save and a reopen do.
fn roundtrip(construct: &Construct) -> (formats::Opened, String) {
    let text = genbank_write::write(construct).expect("writes");
    (formats::read(text.as_bytes(), "primer-test.gb").unwrap_or_else(|why| panic!("{why}")), text)
}

fn named<'a>(construct: &'a Construct, name: &str) -> Vec<&'a Feature> {
    construct.features.iter().filter(|f| f.name == name).collect()
}

fn metadata(feature: &Feature) -> Value {
    let saved = &feature.note(PRIMER_QUALIFIER).expect("the primer qualifier")[0];
    let encoded = saved.strip_prefix(PRIMER_ENCODING).expect("the encoding");
    serde_json::from_slice(&unbase64(encoded)).expect("json")
}

fn preserved(construct: &Construct) -> Vec<Value> {
    construct.comments.iter().filter_map(|c| c.strip_prefix(UNPLACED_COMMENT)).map(|c| serde_json::from_slice(&unbase64(c)).expect("json")).collect()
}

fn place(f: &Feature) -> (usize, usize, i8) {
    (f.start, f.end, f.strand.sign())
}

/// A whole file built here from the container the reader takes: the cookie, the bases (circular),
/// two features and a primer. With it, where each block after the bases ends, as offsets into the
/// file: cut there, what is left still tiles.
fn hand_built() -> (Vec<u8>, Vec<usize>) {
    let mut dna = vec![1u8];
    dna.extend(HAND_BUILT_BASES.as_bytes());
    let features = r#"<Features><Feature name="Forward part" type="CDS" directionality="1"><Segment range="11-40" type="standard"/></Feature><Feature name="Back part" type="misc_feature" directionality="2"><Segment range="51-70" type="standard"/></Feature></Features>"#;
    let primers =
        r#"<Primers><Primer name="Hand primer" sequence="GAATTCGAGCTCGGTACCCG"><BindingSite location="80-99" boundStrand="0"/></Primer></Primers>"#;
    let mut out = cookie();
    out.extend(packet(0, &dna));
    let mut boundaries = vec![out.len()];
    out.extend(packet(10, features.as_bytes()));
    boundaries.push(out.len());
    out.extend(packet(5, primers.as_bytes()));
    (out, boundaries)
}

/// 120 bases with pUC19's EcoRI to SmaI stretch at 81..100, where the hand-built primer binds.
const HAND_BUILT_BASES: &str =
    "ACGTTGCAAGCTAGCTTACGGATCCATGCAAGTCTGACCTGAGGTACCATGGCTAGCAATCGTTAGCCTAGGCATGCTTAGAATTCGAGCTCGGTACCCGTTAACGCGTCGACTGCAGAA";

#[test]
fn a_file_built_by_hand_reads_as_its_blocks_say() {
    let (bytes, _) = hand_built();
    assert_eq!(HAND_BUILT_BASES.len(), 120);
    let opened = formats::read(&bytes, "/somewhere/hand built.dna").expect("opens");
    assert_eq!(opened.format, Format::SnapGene);
    let c = opened.construct;
    assert_eq!(c.name, "hand built");
    assert!(c.circular);
    assert_eq!(c.sequence, HAND_BUILT_BASES);
    // Ranges in the features block count from 1; the model counts from 0. Primer sites count from 0.
    let forward = named(&c, "Forward part");
    assert_eq!(forward.iter().map(|f| (f.kind.as_str(), place(f))).collect::<Vec<_>>(), [("CDS", (10, 39, 1))]);
    let back = named(&c, "Back part");
    assert_eq!(back.iter().map(|f| (f.kind.as_str(), place(f))).collect::<Vec<_>>(), [("misc_feature", (50, 69, -1))]);
    let primer = named(&c, "Hand primer");
    assert_eq!(primer.iter().map(|f| (f.kind.as_str(), place(f))).collect::<Vec<_>>(), [("primer_bind", (80, 99, 1))]);
    assert_eq!(&c.sequence[80..100], "GAATTCGAGCTCGGTACCCG");
    assert_eq!(primer[0].note("sequence").unwrap(), ["GAATTCGAGCTCGGTACCCG"]);
    // And it survives a save as GenBank and a reopen.
    let (saved, _) = roundtrip(&c);
    assert_eq!(saved.construct.sequence, c.sequence);
    assert_eq!(saved.construct.features.iter().map(place).collect::<Vec<_>>(), c.features.iter().map(place).collect::<Vec<_>>());
}

#[test]
fn multiple_native_sites_become_linked_annotations_without_simplified_duplicates() {
    let xml = r#"<Primers><Primer recentID="7" name="Several sites" sequence="GGACGT" description="a &quot;quoted&quot; note &amp; URL https://example.com">
    <BindingSite simplified="1" location="0-3" boundStrand="0"/>
    <BindingSite location="0-3" boundStrand="0"><Component bases="GG"/><Component hybridizedRange="0-3" bases="ACGT"/></BindingSite>
    <BindingSite location="8-11" boundStrand="1"/>
    <BindingSite simplified="1" location="8-11" boundStrand="1"/>
  </Primer></Primers>"#;
    let result = open(&synthetic(xml.as_bytes(), true, ""));
    let ps: Vec<&Feature> = result.construct.features.iter().filter(|f| f.kind == "primer_bind").collect();
    assert_eq!(ps.len(), 2);
    assert_eq!(ps.iter().map(|f| place(f)).collect::<Vec<_>>(), [(0, 3, 1), (8, 11, -1)]);
    assert!(result.notices.iter().any(|n| n.contains("has 2 binding sites") && n.contains("one primer annotation per site")));
    let (saved, _) = roundtrip(&result.construct);
    for p in &saved.construct.features {
        assert_eq!(p.note("sequence").unwrap(), ["GGACGT"]);
        assert_eq!(metadata(p)["primer"]["recentID"], "7");
        assert_eq!(metadata(p)["siteCount"], 2);
        assert_eq!(metadata(p)["primer"]["description"], "a \"quoted\" note & URL https://example.com");
    }
    assert_eq!(
        metadata(ps[0])["bindingSites"][0]["Component"][0],
        json!({ "bases": "GG" }),
        "prefer the detailed record even when simplified came first"
    );
}

#[test]
fn a_matching_feature_based_primer_is_enriched_not_duplicated_or_stripped() {
    let xml = r#"<Primers><Primer name="Same" sequence="GGACGT"><BindingSite location="0-3" boundStrand="0"/></Primer></Primers>"#;
    let features = r#"<Features><Feature name="Same" type="primer_bind" directionality="1"><Segment range="1-4"/></Feature></Features>"#;
    let result = open(&synthetic(xml.as_bytes(), true, features)).construct;
    assert_eq!(named(&result, "Same").len(), 1);
    assert_eq!(result.features[0].note("sequence").unwrap(), ["GGACGT"]);
    // The web app's test goes on to call its primer step alone, on a construct whose primer sits
    // in a separate primers lane with a qualifier of its own. The model here has one lane and the
    // reader one entry, so the same is asked of a file: the feature's own qualifier is kept.
    let features = r#"<Features><Feature name="Same" type="primer_bind" directionality="1"><Segment range="1-4"/><Q name="custom"><V text="keep"/></Q></Feature></Features>"#;
    let enriched = open(&synthetic(xml.as_bytes(), true, features)).construct;
    assert_eq!(enriched.features.len(), 1);
    assert_eq!(enriched.features[0].note("custom").unwrap(), ["keep"]);
    assert_eq!(enriched.features[0].note("sequence").unwrap(), ["GGACGT"]);
}

#[test]
fn unbound_and_unsupported_primer_records_survive_in_exported_comments_with_a_notice() {
    for binding in ["", r#"<BindingSite location="90-99" boundStrand="0"/>"#, r#"<BindingSite location="0-3" boundStrand="2"/>"#] {
        let xml = format!(
            r#"<Primers><Primer name="Unplaced" sequence="TTACGT" description="library: α &quot;recover me&quot;">{binding}</Primer></Primers>"#
        );
        let result = open(&synthetic(xml.as_bytes(), true, ""));
        assert_eq!(result.construct.features.len(), 0, "no fabricated coordinates");
        assert!(result.notices.iter().any(|n| n.contains("could not be placed")));
        let original = preserved(&result.construct);
        assert_eq!(original.len(), 1);
        assert_eq!(original[0]["primer"]["sequence"], "TTACGT");
        assert_eq!(original[0]["primer"]["description"], "library: α \"recover me\"");
        let (saved, _) = roundtrip(&result.construct);
        assert_eq!(preserved(&saved.construct), original);
        assert!(saved.construct.comments.iter().any(|s| s.contains("could not be placed")));
    }
}

#[test]
fn a_mixture_of_supported_and_unsupported_sites_preserves_both_parts() {
    let xml = r#"<Primers><Primer name="Partial" sequence="ACGT"><BindingSite location="0-3" boundStrand="0"/><BindingSite location="far-away" boundStrand="0"/></Primer></Primers>"#;
    let result = open(&synthetic(xml.as_bytes(), true, ""));
    assert_eq!(result.construct.features.len(), 1);
    assert_eq!(preserved(&result.construct)[0]["primer"]["BindingSite"].as_array().unwrap().len(), 2);
    assert!(result.notices.iter().any(|n| n.contains("could not be placed")));
}

#[test]
fn synthetic_reverse_wrapped_sites_are_placed_only_on_circular_dna() {
    let xml = r#"<Primers><Primer name="Wrapped" sequence="ACGTACGT"><BindingSite location="12-3" boundStrand="1"/></Primer></Primers>"#;
    let circle = open(&synthetic(xml.as_bytes(), true, "")).construct;
    assert_eq!(place(&circle.features[0]), (12, 3, -1));
    assert!(roundtrip(&circle).1.contains("complement(join(13..16,1..4))"));
    let linear = open(&synthetic(xml.as_bytes(), false, "")).construct;
    assert_eq!(linear.features.len(), 0);
    assert_eq!(preserved(&linear).len(), 1);
}

#[test]
fn malformed_unsupported_and_non_utf8_primer_xml_never_throws_or_silently_disappears() {
    let payloads: [&[u8]; 4] = [
        br#"<Primers><Primer sequence="ACGT"></Primers>"#,
        br#"<!DOCTYPE Primers [<!ENTITY x "ACGT">]><Primers><Primer sequence="&x;"/></Primers>"#,
        br#"<Unexpected><Primer sequence="ACGT"/></Unexpected>"#,
        &[0xff, 0xfe, 0x81],
    ];
    for payload in payloads {
        let result = open(&synthetic(payload, true, ""));
        assert!(result.notices.iter().any(|n| n.contains("XML could not be read")));
        assert_eq!(result.construct.features.len(), 0);
        let archive = &preserved(&result.construct)[0];
        assert_eq!(unbase64(archive["packetBase64"].as_str().unwrap()), payload);
        assert_eq!(preserved(&roundtrip(&result.construct).0.construct), preserved(&result.construct));
    }
    let valid = synthetic(b"<Primers/>", true, "");
    assert_eq!(open(&valid).notices, Vec::<String>::new());
    assert!(formats::read(&valid[..valid.len() - 1], "").is_err());
}

#[test]
fn unsupported_oligo_letters_are_preserved_instead_of_silently_stripped() {
    let xml = r#"<Primers><Primer name="Modified" sequence="ACGT[mod]"><BindingSite location="0-3" boundStrand="0"/></Primer></Primers>"#;
    let result = open(&synthetic(xml.as_bytes(), true, ""));
    assert_eq!(result.construct.features.len(), 0);
    assert_eq!(preserved(&result.construct)[0]["primer"]["sequence"], "ACGT[mod]");
    assert!(result.notices.iter().any(|n| n.contains("no supported oligo")));
}

// ───────────────────────── snapgeneFeatures.test.ts, in its order ────────────────────────────

/// `native` in the web app's test: a linear eleven bases and one coding feature.
fn native(feature: &str) -> Vec<u8> {
    let mut out = cookie();
    out.extend(packet(0, b"\x00CCATGTGATAA"));
    out.extend(packet(10, format!(r#"<Features><Feature name="Coding" type="CDS" directionality="1" {feature}</Feature></Features>"#).as_bytes()));
    out
}

fn kept_element(feature: &Feature) -> Value {
    let kept = &feature.note("bios_snapgene_feature").expect("the kept element")[0];
    serde_json::from_slice(&unbase64(kept.strip_prefix("base64:").expect("the encoding"))).expect("json")
}

#[test]
fn native_coding_qualifiers_and_first_codon_choice_survive_genbank_saves() {
    for first in ["0", "1"] {
        let feature = format!(
            r#"translateFirstCodonAsMet="{first}" readingFrame="3"><Segment range="1-11" type="standard" translated="1"/>
    <Q name="codon_start"><V int="3"/></Q><Q name="transl_table"><V int="2"/></Q>
    <Q name="note"><V text="Keep this note"/><V text="And this one"/></Q><Q name="translation"><V text="MW*"/></Q>"#
        );
        let result = formats::read(&native(&feature), "coding.dna").expect("opens").construct;
        let saved = formats::read(genbank_write::write(&result).expect("writes").as_bytes(), "coding.gb").expect("reopens").construct;
        for data in [&result, &saved] {
            let f = named(data, "Coding")[0];
            assert_eq!(f.note("codon_start").unwrap(), ["3"]);
            assert_eq!(f.note("transl_table").unwrap(), ["2"]);
            assert_eq!(f.note("bios_start_codon").unwrap(), [if first == "1" { "methionine" } else { "ordinary" }]);
            assert_eq!(f.note("note").unwrap(), ["Keep this note", "And this one"]);
            let raw = kept_element(f);
            assert_eq!(raw["translateFirstCodonAsMet"], first);
            assert_eq!(raw["readingFrame"], "3");
        }
    }
}

#[test]
fn a_signed_native_reading_frame_is_never_silently_substituted_for_a_missing_codon_start() {
    let result = formats::read(&native(r#"readingFrame="-2"><Segment range="1-11" type="standard" translated="1"/>"#), "coding.dna").expect("opens");
    let f = &result.construct.features[0];
    assert_eq!(f.note("bios_translation_unavailable").unwrap(), ["true"]);
    assert_eq!(f.note("codon_start"), None);
    assert!(result.notices.iter().any(|n| n.contains("protein is unverified")));
}

#[test]
fn conflicting_native_frame_and_genbank_qualifier_remain_recoverable_and_explicitly_unverified() {
    let feature = r#"readingFrame="2"><Segment range="1-11" translated="1"/><Q name="codon_start"><V int="1"/></Q>"#;
    let result = formats::read(&native(feature), "conflict.dna").expect("opens");
    let f = &result.construct.features[0];
    assert_eq!(f.note("codon_start").unwrap(), ["1"]);
    assert_eq!(f.note("bios_translation_unavailable").unwrap(), ["true"]);
    assert_eq!(kept_element(f)["readingFrame"], "2");
    assert_eq!(f.note("bios_translation_unavailable_reason").unwrap(), ["SnapGene readingFrame=2 does not agree with codon_start=1"]);
    assert!(result.notices.iter().any(|n| n.contains("protein is unverified")));
}

#[test]
fn translation_exceptions_are_retained_instead_of_turning_an_imported_feature_into_an_ordinary_cds() {
    let feature = r#"><Segment range="1-11"/><Q name="transl_except"><V text="(pos:6..8,aa:Sec)"/></Q><Q name="pseudo"><V/></Q>"#;
    let result = formats::read(&native(feature), "coding.dna").expect("opens").construct;
    let reopened = formats::read(genbank_write::write(&result).expect("writes").as_bytes(), "coding.gb").expect("reopens").construct;
    assert_eq!(reopened.features[0].note("transl_except").unwrap(), ["(pos:6..8,aa:Sec)"]);
    assert_eq!(reopened.features[0].note("pseudo").unwrap(), [""]);
}

// ─────────────────────────── where Bios differs, and damaged files ───────────────────────────

fn case(label: &str) -> (Vec<u8>, Value) {
    let cases = recorded("cases.json");
    let found = cases.as_array().expect("cases").iter().find(|c| c["label"] == label).unwrap_or_else(|| panic!("no case {label}")).clone();
    (unhex(found["hex"].as_str().expect("hex")), found["web"].clone())
}

#[test]
fn a_file_with_no_bases_is_refused_where_the_web_app_opens_an_empty_construct() {
    for label in ["empty sequence", "no sequence block", "cookie and an unknown block"] {
        let (bytes, web) = case(label);
        assert_eq!((web["ok"].as_bool(), web["sequence"].as_str()), (Some(true), Some("")), "{label}");
        assert_eq!(snapgene::read(&bytes, "x.dna"), Err(ReadError::NoSequence), "{label}");
        assert_eq!(formats::read(&bytes, "x.dna").unwrap_err(), "x.dna looks like SnapGene but holds no sequence. Nothing was opened.");
    }
}

#[test]
fn a_feature_never_lies_past_the_bases_that_were_kept() {
    // Sixteen bytes in the block, twelve of them bases. The web app measures the feature against
    // the sixteen and puts its end at base 13 of 12.
    let (bytes, web) = case("bases among other bytes, with a feature");
    assert_eq!((web["length"].as_u64(), web["features"][0]["end"].as_u64()), (Some(12), Some(13)));
    let ours = snapgene::read(&bytes, "x.dna").expect("reads").construct;
    assert_eq!(ours.sequence, "ACGTACGTACGT");
    assert_eq!((ours.features[0].start, ours.features[0].end), (2, 11));

    let (bytes, web) = case("every byte");
    let ours = snapgene::read(&bytes, "x.dna").expect("reads").construct;
    assert_eq!(ours.sequence, web["sequence"].as_str().unwrap().to_uppercase());
    assert_eq!(ours.sequence, "ABCDGHKMNRSTUVWYABCDGHKMNRSTUVWY");
}

#[test]
fn pieces_that_are_not_on_the_sequence_are_not_kept() {
    // The web app keeps a piece that ends at base 89 of 40, and a piece that runs through the
    // origin of a linear molecule. Here such a feature is its span, which both agree on.
    let features = r#"<Features><Feature name="past" type="misc_feature"><Segment range="3-10"/><Segment range="20-90"/></Feature><Feature name="through" type="misc_feature"><Segment range="35-5"/><Segment range="10-20"/></Feature></Features>"#;
    let mut bytes = cookie();
    bytes.extend(packet(0, format!("\x00{}", "ACGT".repeat(10)).as_bytes()));
    bytes.extend(packet(10, features.as_bytes()));
    let ours = snapgene::read(&bytes, "x.dna").expect("reads").construct;
    assert_eq!(
        ours.features.iter().map(|f| (f.start, f.end, f.segments.clone())).collect::<Vec<_>>(),
        [(2, 39, vec![Segment::new(2, 39)]), (0, 19, vec![Segment::new(0, 19)])]
    );
}

#[test]
fn a_label_or_description_that_looks_like_a_number_is_kept_as_written() {
    let (bytes, web) = case("notes: numeric label");
    assert_eq!((&web["name"], &web["description"]), (&json!(7), &Value::Null));
    let ours = snapgene::read(&bytes, "x.dna").expect("reads").construct;
    assert_eq!((ours.name.as_str(), ours.header.as_slice()), ("007", ["DEFINITION  42".to_string()].as_slice()));

    let (bytes, web) = case("notes: boolean label");
    assert_eq!(web["name"], json!(true));
    assert_eq!(snapgene::read(&bytes, "x.dna").expect("reads").construct.name, "true");

    // Two labels name nothing; the file's own name does.
    let (bytes, web) = case("notes: label twice");
    assert_eq!(web["name"], json!(["one", "two"]));
    assert_eq!(snapgene::read(&bytes, "/here/x.dna").expect("reads").construct.name, "x");
}

#[test]
fn a_container_whose_blocks_do_not_tile_the_file_is_refused_never_half_read() {
    let (whole, boundaries) = hand_built();
    assert!(snapgene::intact(&whole));
    let refusal = "mini.dna looks like SnapGene but Bios couldn't read it. Nothing was opened. If it opens in the tool that wrote it, exporting a fresh GenBank usually fixes this.";
    // Cut anywhere, the file is damaged. A cut that falls between two blocks leaves a container
    // that tiles, and that one reads or is refused for what it then lacks; it is never half-read.
    for cut in 13..whole.len() {
        let part = &whole[..cut];
        if boundaries.contains(&cut) {
            assert!(snapgene::intact(part), "cut at {cut}");
            continue;
        }
        assert!(!snapgene::intact(part), "cut at {cut}");
        assert_eq!(formats::read(part, "mini.dna").unwrap_err(), refusal, "cut at {cut}");
    }
    let mut longer = whole.clone();
    longer.push(0);
    assert_eq!(formats::read(&longer, "mini.dna").unwrap_err(), refusal);

    // Read as text somewhere on the way: every byte above 0x7F became three, and the walk runs
    // off the end. This is the damage the check exists for.
    let (as_text, web) = case("read as text");
    assert_eq!(web["ok"], false);
    assert!(!snapgene::intact(&as_text));
    assert_eq!(snapgene::read(&as_text, "x.dna"), Err(ReadError::Damaged));
}

#[test]
fn a_block_bios_needs_that_does_not_read_refuses_the_file() {
    for label in [
        "features: wrong end tag",
        "features: doctype",
        "features: not utf-8",
        "segment without a range",
        "direction \"4\"",
        "sequence block with no bytes",
        "short cookie",
    ] {
        let (bytes, web) = case(label);
        assert_eq!(web["ok"], false, "{label}");
        assert_eq!(snapgene::read(&bytes, "x.dna"), Err(ReadError::Damaged), "{label}");
    }
}

#[test]
fn hostile_containers_are_refused_or_read_and_never_crash() {
    let (whole, _) = hand_built();
    // One byte changed at a time, through the whole file.
    for at in (0..whole.len()).step_by(7) {
        for value in [0x00, 0x3c, 0xff] {
            let mut bytes = whole.clone();
            bytes[at] = value;
            if let Ok(read) = snapgene::read(&bytes, "x.dna") {
                let length = read.construct.len();
                assert!(length > 0);
                for f in &read.construct.features {
                    assert!(f.start < length && f.end < length, "{at}: {} {}..{}", f.name, f.start, f.end);
                    assert!(f.segments.iter().all(|s| s.start < length && s.end < length), "{at}: {}", f.name);
                }
            }
        }
    }
    // A block that claims four gigabytes.
    let mut huge = cookie();
    huge.extend([0, 0xff, 0xff, 0xff, 0xff, 1, b'A']);
    assert_eq!(snapgene::read(&huge, "x.dna"), Err(ReadError::Damaged));
    // More blocks than any plasmid has.
    let mut many = cookie();
    many.extend(packet(0, b"\x01ACGT"));
    for _ in 0..5000 {
        many.extend(packet(77, b""));
    }
    assert_eq!(snapgene::read(&many, "x.dna"), Err(ReadError::Damaged));
    // Elements nested without end.
    let deep = format!("<Features>{}{}</Features>", "<Feature name='f'>".repeat(5000), "</Feature>".repeat(5000));
    let mut nested = cookie();
    nested.extend(packet(0, b"\x01ACGTACGTACGT"));
    nested.extend(packet(10, deep.as_bytes()));
    assert_eq!(snapgene::read(&nested, "x.dna"), Err(ReadError::Damaged));
}

#[test]
fn a_small_file_cannot_unpack_into_a_huge_construct() {
    // Each binding site of a primer keeps the primer's own fields beside it. A long field and
    // many sites would multiply into gigabytes of qualifiers.
    let sites: String = (0..4000).map(|i| format!(r#"<BindingSite location="{i}-{}" boundStrand="0"/>"#, i + 20)).collect();
    let xml = format!(r#"<Primers><Primer name="p" sequence="ACGT" description="{}">{sites}</Primer></Primers>"#, "d".repeat(200_000));
    let mut bytes = cookie();
    bytes.extend(packet(0, format!("\x01{}", "ACGT".repeat(2000)).as_bytes()));
    bytes.extend(packet(5, xml.as_bytes()));
    assert_eq!(snapgene::read(&bytes, "x.dna"), Err(ReadError::Unwieldy));
    assert!(formats::read(&bytes, "x.dna").unwrap_err().starts_with("x.dna looks like SnapGene but Bios couldn't read it."));
}

#[test]
fn what_would_take_time_that_grows_with_the_square_of_the_file_is_bounded() {
    let with = |kind: u8, xml: &str| {
        let mut bytes = cookie();
        bytes.extend(packet(0, format!("\x01{}", "ACGT".repeat(5000)).as_bytes()));
        bytes.extend(packet(kind, xml.as_bytes()));
        bytes
    };
    let started = std::time::Instant::now();

    // Each place a primer binds is looked for among the features that are already there.
    let site = |i: usize| format!(r#"<BindingSite location="{i}-{}" boundStrand="0"/>"#, i + 20);
    let sites =
        |count: usize| format!(r#"<Primers><Primer name="p" sequence="ACGT">{}</Primer></Primers>"#, (0..count).map(site).collect::<String>());
    assert_eq!(snapgene::read(&with(5, &sites(10_000)), "x.dna").expect("reads").construct.features.len(), 10_000);
    assert_eq!(snapgene::read(&with(5, &sites(10_001)), "x.dna"), Err(ReadError::Unwieldy));
    // The worst there is: every primer at the same place under the same name, each another oligo.
    let alike: String = (0..10_000)
        .map(|i| format!(r#"<Primer name="p" sequence="ACGT{}"><BindingSite location="5-25" boundStrand="0"/></Primer>"#, "A".repeat(i % 97)))
        .collect();
    assert!(snapgene::read(&with(5, &format!("<Primers>{alike}</Primers>")), "x.dna").is_ok());

    // A qualifier is looked up by name, and so are an element's children and attributes.
    let qualifiers: String = (0..50_000).map(|i| format!(r#"<Q name="q{i}"><V text="v"/></Q>"#)).collect();
    let feature = format!(r#"<Features><Feature name="f" type="misc_feature"><Segment range="1-9"/>{qualifiers}</Feature></Features>"#);
    assert_eq!(snapgene::read(&with(10, &feature), "x.dna").expect("reads").construct.features[0].notes.len(), 50_002);
    let attributes: String = (0..2000).map(|i| format!(r#" a{i}="v""#)).collect();
    let feature = format!(r#"<Features><Feature name="f" type="misc_feature"{attributes}><Segment range="1-9"/></Feature></Features>"#);
    assert_eq!(snapgene::read(&with(10, &feature), "x.dna"), Err(ReadError::Damaged));
    let tags: String = (0..300).map(|i| format!("<T{i}/>")).collect();
    let feature = format!(r#"<Features><Feature name="f" type="misc_feature"><Segment range="1-9"/>{tags}</Feature></Features>"#);
    assert_eq!(snapgene::read(&with(10, &feature), "x.dna"), Err(ReadError::Damaged));
    let features: String = (0..100_001).map(|_| r#"<Feature name="f"><Segment range="1-9"/></Feature>"#).collect();
    assert_eq!(snapgene::read(&with(10, &format!("<Features>{features}</Features>")), "x.dna"), Err(ReadError::Unwieldy));

    // Generous: all of the above takes a second or two in a debug build.
    assert!(started.elapsed() < std::time::Duration::from_secs(60), "{:?}", started.elapsed());
}

// ───────────────────────────── SnapGene's own files, kept privately ───────────────────────────

/// The `.dna` files SnapGene itself wrote, with what the web app read from each. They are not in
/// this repository: they are read from the folder `BIOS_PRIVATE_FIXTURES` names, which keeps this
/// repository's layout.
mod private {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        let path = std::path::PathBuf::from(root).join("crates/bios-core/tests/fixtures/snapgene").join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn recorded(name: &str) -> Value {
        serde_json::from_slice(&fixture(name)).expect("json")
    }

    const FILES: [&str; 6] = [
        "mini_snapgene",
        "clipboard-standard-paste",
        "reverse-tail-and-origin",
        "tail-after-template-mutation",
        "tail-before-edit",
        "tail-explicitly-shortened",
    ];

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn every_snapgene_file_reads_as_the_web_app_reads_it() {
        for name in FILES {
            let bytes = fixture(&format!("{name}.dna"));
            let web = recorded(&format!("{name}.snapshot.json"));
            assert_eq!(web["ok"], true, "{name}");
            let ours = snapgene::read(&bytes, &format!("{name}.dna"));
            assert_eq!(differences(&ours, &web), Vec::<String>::new(), "{name}");
            // And through the one door every file comes through.
            let opened = formats::read(&bytes, &format!("/somewhere/{name}.dna")).expect("opens");
            assert_eq!(opened.format, Format::SnapGene);
            assert_eq!(opened.construct, ours.expect("reads").construct, "{name}");
        }
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_fixtures_are_the_files_their_provenance_names() {
        // The same five files the web app tests with, byte for byte.
        let provenance = recorded("provenance.json");
        let files = provenance["files"].as_object().expect("files");
        assert_eq!(files.len(), 5);
        for name in files.keys() {
            assert!(snapgene::intact(&fixture(name)), "{name}");
        }
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn native_tailed_primer_retains_its_ordered_oligo_before_and_after_template_mutation() {
        let full = "GGGGGGCGTCCTGGGTGTCGGGGTCG";
        let before = open(&fixture("tail-before-edit.dna")).construct;
        let after = open(&fixture("tail-after-template-mutation.dna")).construct;
        assert_ne!(before.sequence.as_bytes()[309], after.sequence.as_bytes()[309]);
        for data in [&before, &after] {
            let primers = named(data, "REA tail test");
            assert_eq!(primers.len(), 1, "the simplified BindingSite is not a second primer");
            let p = primers[0];
            assert_eq!(place(p), (299, 319, 1));
            assert_eq!(full.len(), 26);
            assert_eq!(p.note("sequence").unwrap(), [full]);
            assert_eq!(named(data, "Demo primer").len(), 1, "existing feature-based primer retained");
            let m = metadata(p);
            assert_eq!(m["primer"]["sequence"], full);
            assert_eq!(m["bindingSites"].as_array().unwrap().len(), 2, "detailed and simplified source records retained as metadata");
            assert_eq!(m["bindingSites"][0]["location"], "299-319");
            assert_eq!(m["bindingSites"][0]["Component"][0]["bases"], "GGGGG");
            let (saved, _) = roundtrip(data);
            let restored = named(&saved.construct, "REA tail test")[0];
            assert_eq!(restored.note("sequence").unwrap(), [full]);
            assert_eq!(metadata(restored), m);
        }
        let mutated = metadata(named(&after, "REA tail test")[0]);
        assert_eq!(mutated["bindingSites"][0]["Component"].as_array().unwrap().len(), 4);
        assert_eq!(
            mutated["bindingSites"][0]["Component"][2],
            json!({ "bases": "T" }),
            "native mismatch is retained, not rewritten from the template"
        );
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn explicit_native_oligo_shortening_imports_the_shorter_oligo_and_recalculated_site() {
        let result = open(&fixture("tail-explicitly-shortened.dna")).construct;
        let p = named(&result, "REA tail test")[0];
        assert_eq!((p.start, p.end), (299, 315));
        assert_eq!(p.note("sequence").unwrap(), ["GGGGGGCGTCCTGGGTGTCGGG"]);
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn native_reverse_and_origin_spanning_primers_survive_repeated_valid_genbank_saves() {
        let result = open(&fixture("reverse-tail-and-origin.dna")).construct;
        let reverse = named(&result, "REA reverse tail")[0];
        assert_eq!(place(reverse), (500, 519, -1));
        assert_eq!(reverse.note("sequence").unwrap(), ["AAAAAATGAGAGTTCCCAGGATGCTC"]);
        let origin = named(&result, "REA reverse across origin")[0];
        assert_eq!(place(origin), (2990, 9, -1));
        assert_eq!(origin.note("sequence").unwrap(), ["TGCACTACCCTGCCGACTGG"]);
        let mut data = result.clone();
        for _ in 0..3 {
            let (saved, text) = roundtrip(&data);
            assert!(text.contains("complement(join(2991..3000,1..10))"), "{text}");
            data = saved.construct;
            let p = named(&data, "REA reverse across origin")[0];
            assert_eq!(place(p), (2990, 9, -1));
            assert_eq!(p.note("sequence"), origin.note("sequence"));
            assert_eq!(metadata(p), metadata(origin));
        }
    }
}
