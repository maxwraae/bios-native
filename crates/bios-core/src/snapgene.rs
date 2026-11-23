//! SnapGene `.dna` reader.
//!
//! A `.dna` file is a run of blocks, each one byte of type, four bytes of big-endian length and
//! that many bytes of data. The block layout read here (the `SnapGene` cookie, the sequence
//! block with its topology flag, and the XML blocks for notes, features and primers) follows the
//! public description in Biopython's `Bio/SeqIO/SnapGeneIO.py` (tag `biopython-188`, commit
//! `d7e4b8b19399668b09442a5b35765d9186b5f665` of <https://github.com/biopython/biopython>, under
//! its BSD-3-Clause licence; copyright 2017-2019 Damien Goutte-Gattat). None of its code is
//! here. No SnapGene code was used, and neither was the source of the container reader the Bios
//! web app calls: what that reader does with a block was learned by running the web app on files
//! and recording what came back. Those recordings are the fixtures of `tests/snapgene.rs`.
//!
//! Everything above the container is the Bios web app's own (`packages/formats/src`), ported:
//! `sniff.ts` (a container whose blocks do not tile the file is damaged), `snapgeneFeatures.ts`
//! (qualifiers and the coding settings of a feature), `snapgenePrimers.ts` (primers as
//! `primer_bind` features) and `normalize/color.ts` (colours a GenBank file can hold).
//!
//! The web app keeps each feature and primer as SnapGene wrote it, as JSON inside a qualifier
//! (`bios_snapgene_feature`, `snapgene_primer`), so that nothing is lost on the way to GenBank.
//! Those qualifiers are byte for byte what the web app writes. That is why the XML is read here
//! by hand and not by a general XML library: the JSON is the shape the web app's XML reader gives
//! an element (children before attributes, a repeated tag as a list), and its values are the
//! text as written with only the five named references resolved.

use std::collections::HashMap;

use crate::locations::normalize_location;
use crate::model::{Construct, Feature, Locus, Segment, Strand};
use json::Json;

/// A plasmid is thousands of bases. Beyond this a file is not a map.
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
/// A plasmid does not have thousands of blocks.
const MAX_BLOCKS: usize = 4096;
/// How many features one file may hold, and how many places its primers may bind. A chromosome
/// has tens of thousands of features; past these a file is a dataset, and reading it would cost
/// time that grows with the square of what it holds.
const MAX_FEATURES: usize = 100_000;
const MAX_SITES: usize = 10_000;

/// The qualifier that holds a primer as SnapGene wrote it.
pub const PRIMER_QUALIFIER: &str = "snapgene_primer";
/// What the value of [`PRIMER_QUALIFIER`] starts with. The rest is URL-safe base64 of UTF-8 JSON.
pub const PRIMER_ENCODING: &str = "base64url:";
/// What a comment that holds a primer Bios could not place starts with. The rest is base64 of
/// UTF-8 JSON.
pub const UNPLACED_COMMENT: &str = "Bios preserved unplaced SnapGene primer data (base64 UTF-8 JSON): ";
const UNPLACED_NOTE: &str = "Bios: native SnapGene primer data could not be placed completely; original information follows for recovery.";

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file is larger than [`MAX_BYTES`].
    TooLarge(usize),
    /// The blocks do not tile the file exactly, or a block Bios needs does not read. Such a file
    /// still yields something when it is half-read: a plausible construct of the wrong length
    /// with none of its features. It is refused whole.
    Damaged,
    /// The file holds no bases.
    NoSequence,
    /// What the file holds unpacks to far more than the file itself.
    Unwieldy,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::TooLarge(bytes) => write!(f, "this SnapGene file is {bytes} bytes, far bigger than any plasmid"),
            ReadError::Damaged => write!(f, "this SnapGene file is damaged"),
            ReadError::NoSequence => write!(f, "this SnapGene file has no sequence"),
            ReadError::Unwieldy => write!(f, "this SnapGene file unpacks to far more than a plasmid holds"),
        }
    }
}

impl std::error::Error for ReadError {}

/// A SnapGene file that was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub construct: Construct,
    /// Things the person should know that did not stop the file from opening.
    pub notices: Vec<String>,
}

struct Block<'a> {
    kind: u8,
    /// Where the block starts in the file.
    offset: usize,
    payload: &'a [u8],
}

/// The blocks of a container, or `None` when they do not tile the file exactly.
fn blocks(bytes: &[u8]) -> Option<Vec<Block<'_>>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        let header = bytes.get(at..at + 5)?;
        let [kind, a, b, c, d] = <[u8; 5]>::try_from(header).ok()?;
        let size = usize::try_from(u32::from_be_bytes([a, b, c, d])).ok()?;
        let end = at.checked_add(5)?.checked_add(size)?;
        out.push(Block { kind, offset: at, payload: bytes.get(at + 5..end)? });
        if out.len() > MAX_BLOCKS {
            return None;
        }
        at = end;
    }
    (at == bytes.len() && out.len() > 1).then_some(out)
}

/// True when the blocks of a SnapGene container tile the file exactly.
///
/// When they do not, the bytes were damaged on the way: most often the file was read as text
/// somewhere, every byte above 0x7F became three, the sequence block grew and the walk runs off
/// the end.
pub fn intact(bytes: &[u8]) -> bool {
    blocks(bytes).is_some()
}

/// Reads a SnapGene file. `file_name` names a construct whose file gives it no name of its own.
pub fn read(bytes: &[u8], file_name: &str) -> Result<Read, ReadError> {
    let blocks = blocks(bytes).ok_or(ReadError::Damaged)?;
    if bytes.len() > MAX_BYTES {
        return Err(ReadError::TooLarge(bytes.len()));
    }
    // The cookie: the eight letters, then the kind of sequence and two version numbers.
    let cookie = blocks.first().filter(|b| b.kind == 9 && b.payload.len() == 14 && b.payload.starts_with(b"SnapGene"));
    if cookie.is_none() {
        return Err(ReadError::Damaged);
    }
    let last = |kind: u8| blocks.iter().rev().find(|b| b.kind == kind);

    // The sequence block: one byte of flags, whose lowest bit says circular, then the bases.
    let (circular, sequence) = match last(0) {
        Some(block) => {
            let (flags, bases) = block.payload.split_first().ok_or(ReadError::Damaged)?;
            (flags & 1 == 1, bases.iter().filter(|b| is_base(**b)).map(|b| char::from(b.to_ascii_uppercase())).collect::<String>())
        }
        None => (false, String::new()),
    };
    if sequence.is_empty() {
        return Err(ReadError::NoSequence);
    }

    let mut reading = Reading {
        length: sequence.len(),
        circular,
        notices: Vec::new(),
        comments: Vec::new(),
        // Every qualifier Bios makes is charged here, so a small file cannot unpack to a huge one.
        budget: bytes.len().saturating_mul(8).saturating_add(1024 * 1024),
        sites: 0,
    };
    let mut drafts = match last(10) {
        Some(block) => reading.features(block.payload)?,
        None => Vec::new(),
    };
    for block in blocks.iter().filter(|b| b.kind == 5) {
        reading.primers(block, &mut drafts)?;
    }
    let recoloured = drafts.iter_mut().map(scrub_colours).filter(|changed| *changed).count();
    match recoloured {
        0 => {}
        1 => reading.notices.push("One feature had a colour this file format can't hold. It kept its place and its name.".to_string()),
        n => reading.notices.push(format!("{n} features had a colour this file format can't hold. They kept their places and their names.")),
    }

    let (label, description) = last(6).map(|block| notes_block(block.payload)).unwrap_or_default();
    let features = drafts.into_iter().map(|draft| draft.finish(reading.length, circular)).collect();
    let construct = Construct {
        name: label.unwrap_or_else(|| name_from(file_name)),
        circular,
        sequence,
        features,
        comments: reading.comments,
        locus: Locus { molecule: "DNA".into(), ..Locus::default() },
        // A GenBank file says what a construct is on its DEFINITION line, so that is where the
        // description goes.
        header: description.map(|text| format!("DEFINITION  {}", text.replace('\n', " "))).into_iter().collect(),
    };
    Ok(Read { construct, notices: reading.notices })
}

/// The letters kept from a sequence block: the IUPAC nucleotide codes. Anything else is dropped.
fn is_base(byte: u8) -> bool {
    b"ABCDGHKMNRSTUVWY".contains(&byte.to_ascii_uppercase())
}

/// The construct's name when the file gives none: the file's own name without its folder and its
/// extension.
fn name_from(file_name: &str) -> String {
    let base = if file_name.is_empty() { "sequence" } else { file_name };
    let base = base.rsplit(['/', '\\']).next().unwrap_or(base);
    let stem = match base.rfind('.') {
        Some(dot) if dot > 0 => base.get(..dot).unwrap_or(base),
        _ => base,
    };
    if stem.is_empty() { "sequence".to_string() } else { stem.to_string() }
}

/// The construct's own name and its description, from the notes block. Both are optional, and a
/// notes block that does not read costs the name and the description, never the file.
fn notes_block(payload: &[u8]) -> (Option<String>, Option<String>) {
    let text = unix_lines(&String::from_utf8_lossy(payload));
    let Some(root) = xml::parse(text.strip_prefix('\u{feff}').unwrap_or(&text), false).filter(|root| root.name == "Notes") else {
        return (None, None);
    };
    let field = |name: &str| {
        let mut found = root.elements().filter(|e| e.name == name);
        let only = found.next().filter(|e| e.elements().next().is_none())?;
        // A field written twice says nothing that can be trusted.
        if found.next().is_some() {
            return None;
        }
        Some(js::trim(&only.text()).to_string())
    };
    // SnapGene keeps a description as a small HTML document. Its wrapper goes, once.
    let description = field("Description").map(|text| text.replacen("<html><body>", "", 1).replacen("</body></html>", "", 1));
    (field("CustomMapLabel").filter(|v| !v.is_empty()), description.filter(|v| !v.is_empty()))
}

/// CR LF and CR as LF, which is how the web app's XML reader sees every line end.
fn unix_lines(text: &str) -> String {
    if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.to_string() }
}

/// The XML of a block as text, or `None` when it is not UTF-8 or declares a document type or an
/// entity. SnapGene's records need neither, and a declared entity is how XML is made to expand.
fn block_xml(payload: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(payload).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lower = text.to_ascii_lowercase();
    if lower.contains("<!doctype") || lower.contains("<!entity") {
        return None;
    }
    Some(unix_lines(text))
}

type Notes = Vec<(String, Vec<String>)>;

fn note<'a>(notes: &'a Notes, key: &str) -> Option<&'a Vec<String>> {
    notes.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Sets a qualifier where it already stands, or adds it at the end.
fn set_note(notes: &mut Notes, key: &str, values: Vec<String>) {
    match notes.iter_mut().find(|(k, _)| k == key) {
        Some((_, slot)) => *slot = values,
        None => notes.push((key.to_string(), values)),
    }
}

/// A feature while the file is still being read.
struct Draft {
    name: String,
    kind: String,
    start: usize,
    end: usize,
    strand: Strand,
    /// The pieces of a location with several, in file order. Empty for a simple span.
    pieces: Vec<(usize, usize)>,
    notes: Notes,
    colour: Option<String>,
    /// What a GenBank file says of a feature drawn with no arrowhead or with two.
    direction: Option<&'static str>,
}

impl Draft {
    fn finish(self, length: usize, circular: bool) -> Feature {
        let mut notes = self.notes;
        // Where the web app's GenBank export puts what the model has no field for: after the
        // qualifiers, the direction and then the colour.
        if let Some(direction) = self.direction.filter(|_| note(&notes, "direction").is_none()) {
            notes.push(("direction".to_string(), vec![direction.to_string()]));
        }
        if let Some(colour) = self.colour.filter(|_| note(&notes, "color").is_none()) {
            notes.push(("color".to_string(), vec![colour]));
        }
        let segments = if self.pieces.is_empty() {
            vec![Segment::new(self.start, self.end)]
        } else {
            self.pieces.iter().map(|&(start, end)| Segment::new(start, end)).collect()
        };
        let mut feature = Feature {
            name: self.name,
            kind: self.kind,
            start: self.start,
            end: self.end,
            strand: self.strand,
            segments,
            notes,
            start_fuzzy: None,
            end_fuzzy: None,
            location_operator: None,
            location_unsupported: None,
        };
        normalize_location(&mut feature, length, circular);
        feature
    }
}

/// The feature keys the web app's reader knows, as it spells them. A type that matches one in
/// any letter case is given this spelling; any other type is kept as written.
const KINDS: [&str; 78] = [
    "assembly_gap",
    "C_region",
    "CDS",
    "centromere",
    "D-loop",
    "D_segment",
    "exon",
    "gap",
    "gene",
    "iDNA",
    "intron",
    "mat_peptide",
    "misc_binding",
    "misc_difference",
    "misc_feature",
    "misc_recomb",
    "misc_RNA",
    "misc_structure",
    "mobile_element",
    "modified_base",
    "mRNA",
    "ncRNA",
    "N_region",
    "old_sequence",
    "operon",
    "oriT",
    "polyA_site",
    "precursor_RNA",
    "prim_transcript",
    "primer_bind",
    "protein_bind",
    "regulatory",
    "repeat_region",
    "rep_origin",
    "rRNA",
    "S_region",
    "sig_peptide",
    "source",
    "stem_loop",
    "STS",
    "telomere",
    "tmRNA",
    "transit_peptide",
    "tRNA",
    "unsure",
    "V_region",
    "V_segment",
    "variation",
    "3'UTR",
    "5'UTR",
    "-10_signal",
    "-35_signal",
    "attenuator",
    "CAAT_signal",
    "enhancer",
    "GC_signal",
    "LTR",
    "misc_signal",
    "polyA_signal",
    "promoter",
    "RBS",
    "satellite",
    "scRNA",
    "snRNA",
    "snoRNA",
    "TATA_signal",
    "terminator",
    "repeat_unit",
    "transposon",
    "allele",
    "protein",
    "tag",
    "plasmid",
    "3'clip",
    "5'clip",
    "misc_marker",
    "Site",
    "primer",
];

fn kind_of(written: &str) -> String {
    let written = js::trim(written);
    if written.is_empty() {
        return "misc_feature".to_string();
    }
    match KINDS.iter().find(|k| k.eq_ignore_ascii_case(written)) {
        // SnapGene's older name for a primer binding site.
        Some(&"primer") => "primer_bind".to_string(),
        Some(known) => (*known).to_string(),
        None => written.to_string(),
    }
}

/// A span as a `range` attribute gives it, before it is checked against the sequence: 0-based,
/// and not a number where the text was not one.
type RawSpan = (f64, f64);

fn raw_span(range: &str) -> RawSpan {
    let mut parts = js::trim(range).split('-');
    let mut end = || parts.next().map_or(f64::NAN, |text| js::number(text) - 1.0);
    (end(), end())
}

/// A whole number of bases that lies on a sequence of `length`.
fn on_sequence(value: f64, length: usize) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= length as f64 {
        return None;
    }
    // In range and whole, so the conversion is exact.
    Some(value as usize)
}

/// True when every base of `inner` is a base of `outer`, on a sequence of `length`. A span that
/// ends before it starts runs through the origin.
fn within(inner: RawSpan, outer: RawSpan, length: usize) -> bool {
    let ((start, end), (from, to)) = (inner, outer);
    let top = length.saturating_sub(1) as f64;
    // An outer span that ends on the base before its start is the whole circle.
    let everything = if from <= to { from <= 0.0 && to >= top } else { to + 1.0 >= from };
    match (from <= to, start <= end) {
        (true, true) => start >= from && end <= to,
        (true, false) => everything,
        (false, true) => start >= from || end <= to || everything,
        (false, false) => (start >= from && end <= to) || everything,
    }
}

/// Where a feature with these segments lies: its start, its end, and its pieces when there are
/// several that can be trusted.
fn locate(spans: &[RawSpan], length: usize, circular: bool) -> (usize, usize, Vec<(usize, usize)>) {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return (0, 0, Vec::new());
    };
    let outer = (first.0, last.1);
    // A start that is not on the sequence falls back to its first base. So does an end before
    // the first base; an end that is past the last base, or not a number, falls to the last.
    let top = length.saturating_sub(1);
    let mut start = on_sequence(outer.0.floor(), length).unwrap_or(0);
    let end = if outer.1 < 0.0 { 0 } else { on_sequence(outer.1.floor(), length).unwrap_or(top) };
    if !circular && start > end {
        start = 0;
    }

    let piece = |span: &RawSpan| {
        let (from, to) = (on_sequence(span.0, length)?, on_sequence(span.1, length)?);
        (within(*span, outer, length) && (to >= from || circular)).then_some((from, to))
    };
    let pieces = if spans.len() > 1 { spans.iter().map(piece).collect::<Option<Vec<_>>>().unwrap_or_default() } else { Vec::new() };
    (start, end, pieces)
}

struct Reading {
    length: usize,
    circular: bool,
    notices: Vec<String>,
    comments: Vec<String>,
    budget: usize,
    /// How many places primers bind so far.
    sites: usize,
}

impl Reading {
    fn spend(&mut self, bytes: usize) -> Result<(), ReadError> {
        self.budget = self.budget.checked_sub(bytes).ok_or(ReadError::Unwieldy)?;
        Ok(())
    }

    /// Reads the features block. A block that does not read refuses the file: its features would
    /// otherwise be silently gone.
    fn features(&mut self, payload: &[u8]) -> Result<Vec<Draft>, ReadError> {
        let text = block_xml(payload).ok_or(ReadError::Damaged)?;
        let root = xml::parse(&text, true).filter(|root| root.name == "Features" && root.attr("Feature").is_none()).ok_or(ReadError::Damaged)?;
        let kept = json::value_of(&root, &["Feature", "Segment", "Q", "V"]);
        let sources: Vec<&Json> = match &kept {
            Json::Str(text) if text.is_empty() => Vec::new(),
            Json::Obj(_) => json::list(kept.get("Feature")),
            _ => return Err(ReadError::Damaged),
        };
        let elements: Vec<&xml::Element> = root.elements().filter(|e| e.name == "Feature").collect();
        // Qualifiers are never paired with another feature.
        if elements.len() != sources.len() {
            return Err(ReadError::Damaged);
        }
        if elements.len() > MAX_FEATURES {
            return Err(ReadError::Unwieldy);
        }
        elements.into_iter().zip(sources).map(|(element, source)| self.feature(element, source)).collect()
    }

    fn feature(&mut self, element: &xml::Element, source: &Json) -> Result<Draft, ReadError> {
        if !matches!(source, Json::Obj(_)) {
            return Err(ReadError::Damaged);
        }
        let name = js::trim(element.attr("name").unwrap_or_default());
        let name = if name.is_empty() { "Untitled Feature" } else { name }.to_string();
        let kind = kind_of(element.attr("type").unwrap_or_default());
        // 0 or nothing: no arrowhead. 1: forward. 2: reverse. 3: an arrowhead at both ends.
        let (strand, direction) = match js::trim(element.attr("directionality").unwrap_or_default()) {
            "" | "0" => (Strand::Forward, Some("NONE")),
            "1" => (Strand::Forward, None),
            "2" => (Strand::Reverse, None),
            "3" => (Strand::Forward, Some("BOTH")),
            _ => return Err(ReadError::Damaged),
        };

        let mut spans = Vec::new();
        let mut colour = None;
        for segment in element.elements().filter(|e| e.name == "Segment") {
            spans.push(raw_span(segment.attr("range").ok_or(ReadError::Damaged)?));
            // The colour of the last segment that has one.
            colour = segment.attr("color").map(js::trim).filter(|c| !c.is_empty()).map(str::to_string).or(colour);
        }
        // A colour qualifier is the feature's own colour and wins over its segments', even when
        // it states none: its first value, when that is text.
        if let Some(stated) = element.elements().find(|e| e.name == "Q" && e.attr("name").map(js::trim) == Some("color")) {
            let value = stated.elements().find(|e| e.name == "V").and_then(|v| v.attr("text"));
            colour = Some(js::trim(value.unwrap_or_default()).to_string());
        }
        let (start, end, pieces) = locate(&spans, self.length, self.circular);

        let mut notes = qualifiers(source);
        let kept = format!("base64:{}", json::base64(source.to_text().as_bytes(), false));
        self.spend(kept.len())?;
        set_note(&mut notes, "bios_snapgene_feature", vec![kept]);
        if kind.eq_ignore_ascii_case("cds") && coding_settings(source, &mut notes) {
            self.notices.push(format!(
                "The native translation settings for \"{name}\" need review. Original feature metadata is retained; its protein is unverified."
            ));
        }
        Ok(Draft { name, kind, start, end, strand, pieces, notes, colour, direction })
    }

    /// Keeps what could not be placed in the construct's comments, as JSON a person can recover,
    /// and says so. `members` are the JSON members of the record, after its version.
    fn preserve(&mut self, members: &[&str], notice: String) -> Result<(), ReadError> {
        // Charged before it is built, for the JSON and for the base64 of it.
        self.spend(members.iter().map(|m| m.len()).sum::<usize>().saturating_mul(2).saturating_add(256))?;
        let record = format!("{{\"version\":1,{}}}", members.concat());
        self.comments.push(UNPLACED_NOTE.to_string());
        self.comments.push(format!("{UNPLACED_COMMENT}{}", json::base64(record.as_bytes(), false)));
        self.notices.push(notice);
        Ok(())
    }

    /// Reads one primers block: one `primer_bind` feature for each place a primer binds. The
    /// whole oligo is kept beside its footprint, so a tail or a mismatch survives.
    fn primers(&mut self, block: &Block<'_>, drafts: &mut Vec<Draft>) -> Result<(), ReadError> {
        let offset = format!("\"packetOffset\":{}", block.offset);
        let Some(root) = primers_root(block.payload) else {
            let packet = json::base64(block.payload, false);
            let notice =
                "SnapGene primer XML could not be read. Its original packet is retained in GenBank comments; no binding positions were invented.";
            let members = [offset.as_str(), ",\"reason\":\"invalid or unsupported primer XML\",\"packetBase64\":\"", packet.as_str(), "\""];
            return self.preserve(&members, notice.to_string());
        };
        let settings = root.get("HybridizationParams").map(|v| format!(",\"hybridizationParameters\":{}", v.to_text())).unwrap_or_default();
        for (index, original) in json::list(root.get("Primer")).into_iter().enumerate() {
            let head = format!("{offset},\"primerIndex\":{index}");
            self.primer(index, original, &Recorded { head: &head, settings: &settings }, drafts)?;
        }
        Ok(())
    }

    fn primer(&mut self, index: usize, original: &Json, recorded: &Recorded<'_>, drafts: &mut Vec<Draft>) -> Result<(), ReadError> {
        let text = |key: &str| original.get(key).and_then(Json::as_str).unwrap_or_default();
        let name = js::trim(text("name"));
        let name = if name.is_empty() { format!("Unnamed SnapGene primer {}", index + 1) } else { name.to_string() };
        let sequence = bases_of(text("sequence"));
        // The primer whole, for when it cannot be placed, or not all of it.
        let whole = |reason: &str| [format!(",\"primer\":{}", original.to_text()), format!(",\"reason\":\"{reason}\"")];
        if sequence.is_empty() || !sequence.chars().all(|c| "ACGTURYSWKMBDHVN".contains(c)) {
            let notice = format!(
                "SnapGene primer \"{name}\" has no supported oligo sequence and could not be placed. Its original data is retained in GenBank comments."
            );
            let [primer, reason] = whole("missing or unsupported oligo sequence");
            return self.preserve(&[recorded.head, &primer, recorded.settings, &reason], notice);
        }

        // One site for each distinct place and strand. SnapGene writes a site twice, in full and
        // simplified; both records are kept with the one site they describe.
        let mut sites: Vec<Site<'_>> = Vec::new();
        let mut known: HashMap<(usize, usize, bool), usize> = HashMap::new();
        let mut unsupported = 0usize;
        for raw in json::list(original.get("BindingSite")) {
            match placed_site(raw, self.length, self.circular) {
                Some(at) => match known.get(&at).and_then(|i| sites.get_mut(*i)) {
                    Some(site) => site.originals.push(raw),
                    None => {
                        known.insert(at, sites.len());
                        sites.push(Site { at, originals: vec![raw] });
                    }
                },
                None => unsupported += 1,
            }
        }
        self.sites += sites.len();
        if self.sites > MAX_SITES {
            return Err(ReadError::Unwieldy);
        }
        if sites.is_empty() || unsupported > 0 {
            let (reason, notice) = if sites.is_empty() {
                ("no supported binding site", "could not be placed")
            } else {
                ("unsupported binding site(s)", "has binding sites that could not be placed")
            };
            let notice = format!("SnapGene primer \"{name}\" {notice}. Its original oligo and binding data are retained in GenBank comments.");
            let [primer, reason] = whole(reason);
            self.preserve(&[recorded.head, &primer, recorded.settings, &reason], notice)?;
        }
        if sites.len() > 1 {
            self.notices.push(format!(
                "SnapGene primer \"{name}\" has {} binding sites. Bios imported one primer annotation per site, each retaining the full oligo and shared source identity.",
                sites.len()
            ));
        }

        // The primer without its sites: each site's record holds it beside that site alone.
        let fields = match original {
            Json::Obj(entries) => json::object_text(entries.iter().filter(|(key, _)| key != "BindingSite")),
            _ => "{}".to_string(),
        };
        let count = sites.len();
        for (site_index, site) in sites.into_iter().enumerate() {
            // The detailed record first, then the simplified one.
            let mut records = site.originals;
            records.sort_by_key(|record| record.get("simplified").and_then(Json::as_str) == Some("1"));
            let records = records.iter().map(|record| record.to_text()).collect::<Vec<_>>().join(",");
            let numbers = format!(",\"siteIndex\":{site_index},\"siteCount\":{count},\"primer\":");
            let members = ["{\"version\":1,", recorded.head, &numbers, &fields, ",\"bindingSites\":[", &records, "]", recorded.settings, "}"];
            // Charged before it is built, for the JSON and for the base64 of it.
            self.spend(members.iter().map(|m| m.len()).sum::<usize>().saturating_mul(2).saturating_add(64))?;
            // URL-safe base64, so that a wrapped line of it can never begin with `/` and read
            // as a new qualifier.
            let metadata = format!("{PRIMER_ENCODING}{}", json::base64(members.concat().as_bytes(), true));
            self.place(&name, &sequence, site.at, metadata, drafts);
        }
        Ok(())
    }

    /// Adds a primer's site as a feature, or adds what is known of the primer to the feature that
    /// already draws it: some files hold the same primer as a feature too.
    fn place(&self, name: &str, sequence: &str, at: (usize, usize, bool), metadata: String, drafts: &mut Vec<Draft>) {
        let (start, end, forward) = at;
        let same = |d: &&mut Draft| {
            // The oligo the feature states, when it states one, has to be this one.
            let said = note(&d.notes, "sequence").and_then(|v| v.first()).map(String::as_str).unwrap_or_default();
            let mut said = said.chars().filter(|c| !js::is_space(*c)).flat_map(char::to_uppercase).peekable();
            d.start == start
                && d.end == end
                && (d.strand == Strand::Forward) == forward
                && d.kind == "primer_bind"
                && d.name == name
                && (said.peek().is_none() || said.eq(sequence.chars()))
        };
        if let Some(found) = drafts.iter_mut().find(same) {
            let mut all = note(&found.notes, PRIMER_QUALIFIER).cloned().unwrap_or_default();
            if !all.contains(&metadata) {
                all.push(metadata);
            }
            set_note(&mut found.notes, "sequence", vec![sequence.to_string()]);
            set_note(&mut found.notes, PRIMER_QUALIFIER, all);
            return;
        }
        drafts.push(Draft {
            name: name.to_string(),
            kind: "primer_bind".to_string(),
            start,
            end,
            strand: if forward { Strand::Forward } else { Strand::Reverse },
            pieces: if start > end { vec![(start, self.length.saturating_sub(1)), (0, end)] } else { Vec::new() },
            notes: vec![("sequence".to_string(), vec![sequence.to_string()]), (PRIMER_QUALIFIER.to_string(), vec![metadata])],
            colour: None,
            direction: None,
        });
    }
}

/// What every record of a primer holds besides the primer: where its block and the primer are in
/// the file, and the block's hybridisation settings. Both are JSON members, ready to be written.
struct Recorded<'a> {
    head: &'a str,
    settings: &'a str,
}

/// The qualifiers a feature was saved with, in file order. A name that repeats keeps every value.
fn qualifiers(source: &Json) -> Notes {
    let mut notes: Notes = Vec::new();
    let mut known: HashMap<&str, usize> = HashMap::new();
    for q in json::list(source.get("Q")) {
        let Some(name) = q.get("name").and_then(Json::as_str) else {
            continue;
        };
        let mut values: Vec<String> = json::list(q.get("V"))
            .into_iter()
            .map(|v| match v {
                Json::Obj(_) => ["text", "int", "predef", "#text"].iter().find_map(|key| v.get(key)).map(Json::as_js_string).unwrap_or_default(),
                other => other.as_js_string(),
            })
            .collect();
        if values.is_empty() {
            values.push(String::new());
        }
        match known.get(name).and_then(|i| notes.get_mut(*i)) {
            Some((_, all)) => all.extend(values),
            None => {
                known.insert(name, notes.len());
                notes.push((name.to_string(), values));
            }
        }
    }
    notes
}

/// Records how SnapGene translates a coding feature, and whether Bios can translate it the same
/// way. True when it cannot, which the person is told.
///
/// SnapGene's own reading frame is signed and some files disagree with their `codon_start`
/// qualifier. Both are kept; neither is silently made the authority.
fn coding_settings(source: &Json, notes: &mut Notes) -> bool {
    let is = |key: &str, value: &str| source.get(key).and_then(Json::as_str) == Some(value);
    set_note(notes, "bios_start_codon", vec![if is("translateFirstCodonAsMet", "1") { "methionine" } else { "ordinary" }.to_string()]);

    let frame = source.get("readingFrame");
    let native = frame.map_or(f64::NAN, |v| js::number(&v.as_js_string()).abs());
    let saved = note(notes, "codon_start").and_then(|v| v.first()).map_or(f64::NAN, |v| js::number(v));
    // Written so that a frame that is not a number disagrees with everything, itself included.
    let disagrees = frame.is_some() && !(native == saved);
    let first_codon = source.get("translateFirstCodonAsMet").map(Json::as_js_string);
    let untranslated = json::list(source.get("Segment"))
        .into_iter()
        .any(|s| s.get("type").and_then(Json::as_str) == Some("gap") || s.get("translated").and_then(Json::as_str) == Some("0"));
    let unsupported = (frame.is_some() && (![1.0, 2.0, 3.0].contains(&native) || disagrees))
        || first_codon.is_some_and(|v| v != "0" && v != "1")
        || untranslated
        || (source.get("geneticCode").is_some() && note(notes, "transl_table").is_none());
    if !unsupported {
        return false;
    }
    let reason = match frame.filter(|_| disagrees) {
        Some(frame) => {
            let saved = if saved.is_finite() { js::number_text(saved) } else { "missing".to_string() };
            format!("SnapGene readingFrame={} does not agree with codon_start={saved}", frame.as_js_string())
        }
        None => "The native SnapGene translation settings or translated segments require review".to_string(),
    };
    set_note(notes, "bios_translation_unavailable", vec!["true".to_string()]);
    set_note(notes, "bios_translation_unavailable_reason", vec![reason]);
    true
}

/// The primers of a block as the web app keeps them, or `None` when the block does not read.
fn primers_root(payload: &[u8]) -> Option<Json> {
    let text = block_xml(payload)?;
    let root = xml::parse(&text, true).filter(|root| root.name == "Primers")?;
    match json::value_of(&root, &["Primer", "BindingSite", "Component"]) {
        Json::Str(text) if text.is_empty() => Some(Json::Obj(Vec::new())),
        kept @ Json::Obj(_) => Some(kept),
        _ => None,
    }
}

/// An oligo as written, without its spacing and in capitals.
fn bases_of(text: &str) -> String {
    text.chars().filter(|c| !js::is_space(*c)).collect::<String>().to_uppercase()
}

struct Site<'a> {
    /// Start, end and whether the primer binds the forward strand.
    at: (usize, usize, bool),
    originals: Vec<&'a Json>,
}

/// Where a binding site lies, when it says so in a way Bios can place: `start-end`, 0-based and
/// inclusive, on strand 0 (forward) or 1 (reverse). A site that runs through the origin is only
/// placed on a circular construct.
fn placed_site(site: &Json, length: usize, circular: bool) -> Option<(usize, usize, bool)> {
    let text = |key: &str| site.get(key).and_then(Json::as_str).unwrap_or_default();
    let (start, end) = text("location").split_once('-')?;
    let forward = match text("boundStrand") {
        "0" => true,
        "1" => false,
        _ => return None,
    };
    let base = |digits: &str| {
        let whole = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
        digits.parse::<usize>().ok().filter(|n| whole && *n < length)
    };
    let (start, end) = (base(start)?, base(end)?);
    (start <= end || circular).then_some((start, end, forward))
}

/// Values that mean "no colour". SnapGene writes `noColor` for a segment with none.
const NO_COLOUR: [&str; 8] = ["nocolor", "none", "transparent", "default", "inherit", "", "null", "undefined"];

/// The 148 named colours of CSS.
const CSS_COLOURS: [&str; 148] = [
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

#[derive(Debug, PartialEq, Eq)]
enum Colour {
    /// A colour a GenBank file can hold, as it is written there.
    Keep(String),
    /// No colour was stated.
    Nothing,
    /// Something that is not a colour Bios can read, kept as written so the file stays honest
    /// about what came in.
    Unreadable(String),
}

/// `rgb(1, 2, 3)` or `rgba(1, 2, 3, 0.5)`, in lower case.
fn is_rgb(lower: &str) -> bool {
    let Some(inner) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")).and_then(|rest| rest.strip_suffix(')')) else {
        return false;
    };
    let space = |c: char| js::is_space(c);
    let parts: Vec<&str> = inner.split(',').map(|part| part.trim_matches(space)).collect();
    let whole = |part: &&str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let alpha = |part: &&str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    match parts.as_slice() {
        [r, g, b] => [r, g, b].into_iter().all(whole),
        [r, g, b, a] => [r, g, b].into_iter().all(whole) && alpha(a),
        _ => false,
    }
}

/// Decides what to do with one colour value. "No colour" is tested first, so that an empty value
/// is nothing stated and not a malformed hex colour.
fn normalize_colour(written: &str) -> Colour {
    let value = js::trim(written);
    let lower = value.to_lowercase();
    if NO_COLOUR.contains(&lower.as_str()) {
        return Colour::Nothing;
    }
    let hex = |text: &str, digits: usize| text.len() == digits && text.bytes().all(|b| b.is_ascii_hexdigit());
    if let Some(digits) = lower.strip_prefix('#') {
        // #abc is legal CSS; #rrggbbaa has an alpha GenBank has nowhere to put.
        if hex(digits, 3) {
            return Colour::Keep(format!("#{}", digits.chars().flat_map(|c| [c, c]).collect::<String>()));
        }
        if hex(digits, 6) {
            return Colour::Keep(lower);
        }
        if hex(digits, 8) {
            return Colour::Keep(lower.chars().take(7).collect());
        }
    }
    if hex(&lower, 6) {
        return Colour::Keep(format!("#{lower}"));
    }
    if is_rgb(&lower) || CSS_COLOURS.contains(&lower.as_str()) {
        return Colour::Keep(lower);
    }
    Colour::Unreadable(value.to_string())
}

/// Makes a feature's colours ones a GenBank file can hold: both the `color` and `labelColor`
/// qualifiers and the feature's own colour. True when anything was changed or dropped.
fn scrub_colours(draft: &mut Draft) -> bool {
    let mut changed = false;
    for key in ["color", "labelColor"] {
        if let Some(written) = note(&draft.notes, key).map(|values| values.first().cloned().unwrap_or_default()) {
            match normalize_colour(&written) {
                Colour::Keep(colour) if colour == written => {}
                Colour::Keep(colour) => {
                    set_note(&mut draft.notes, key, vec![colour]);
                    changed = true;
                }
                dropped => {
                    draft.notes.retain(|(k, _)| k != key);
                    if let Colour::Unreadable(raw) = dropped {
                        set_note(&mut draft.notes, &format!("{key}_raw"), vec![raw]);
                    }
                    changed = true;
                }
            }
        }
        if key != "color" {
            continue;
        }
        if let Some(written) = draft.colour.take() {
            match normalize_colour(&written) {
                Colour::Keep(colour) => {
                    changed |= colour != written;
                    draft.colour = Some(colour);
                }
                dropped => {
                    if let Colour::Unreadable(raw) = dropped {
                        set_note(&mut draft.notes, "color_raw", vec![raw]);
                    }
                    changed = true;
                }
            }
        }
    }
    changed
}

/// The few rules of JavaScript the web app's answers depend on.
mod js {
    /// What JavaScript's `trim` and `\s` call white space.
    pub(super) fn is_space(c: char) -> bool {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    }

    pub(super) fn trim(text: &str) -> &str {
        text.trim_matches(is_space)
    }

    fn radix(digits: &str, base: u32) -> f64 {
        if digits.is_empty() {
            return f64::NAN;
        }
        let mut value = 0.0;
        for c in digits.chars() {
            match c.to_digit(base) {
                Some(digit) => value = value * f64::from(base) + f64::from(digit),
                None => return f64::NAN,
            }
        }
        value
    }

    /// True for a decimal literal: digits with an optional fraction and exponent.
    fn is_decimal(text: &str) -> bool {
        let (mantissa, exponent) = match text.find(['e', 'E']) {
            Some(at) => (text.get(..at).unwrap_or_default(), text.get(at + 1..)),
            None => (text, None),
        };
        let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let exponent_ok = exponent.is_none_or(|e| {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && digits(e)
        });
        digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty()) && exponent_ok
    }

    /// `Number(text)`: the number a string is, or NaN.
    pub(super) fn number(text: &str) -> f64 {
        let text = trim(text);
        if text.is_empty() {
            return 0.0;
        }
        for (prefix, base) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
            if let Some(digits) = text.strip_prefix(prefix) {
                return radix(digits, base);
            }
        }
        let (sign, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (-1.0, rest),
            None => (1.0, text.strip_prefix('+').unwrap_or(text)),
        };
        if unsigned == "Infinity" {
            return sign * f64::INFINITY;
        }
        if !is_decimal(unsigned) {
            return f64::NAN;
        }
        sign * unsigned.parse::<f64>().unwrap_or(f64::NAN)
    }

    /// `String(value)` for a finite number.
    pub(super) fn number_text(value: f64) -> String {
        if value == 0.0 {
            return "0".to_string();
        }
        let size = value.abs();
        if (1e-6..1e21).contains(&size) {
            return format!("{value}");
        }
        // Past those bounds JavaScript writes an exponent, with its sign.
        let written = format!("{value:e}");
        match written.split_once('e') {
            Some((mantissa, exponent)) if !exponent.starts_with('-') => format!("{mantissa}e+{exponent}"),
            _ => written,
        }
    }
}

/// XML, as much of it as a SnapGene block uses.
///
/// Read strictly, a document that is not well formed is not read at all. Read leniently, what
/// could be read is kept: an end tag closes the innermost open element whatever it names, and a
/// document that stops early keeps the elements it had.
mod xml {
    /// How deep elements may nest. SnapGene's own nest four deep.
    const MAX_DEPTH: usize = 64;
    /// How many elements one block may hold.
    const MAX_ELEMENTS: usize = 1_000_000;
    /// How many attributes one tag may have, and how many different tags one block may use.
    /// SnapGene's own use a dozen of each. The bounds keep looking one up by name cheap.
    const MAX_ATTRIBUTES: usize = 1024;
    const MAX_NAMES: usize = 256;

    #[derive(Debug, Default)]
    pub(super) struct Element {
        pub(super) name: String,
        /// Attributes in document order, their values as written with the named references
        /// resolved.
        pub(super) attrs: Vec<(String, String)>,
        pub(super) children: Vec<Node>,
    }

    #[derive(Debug)]
    pub(super) enum Node {
        Element(Element),
        Text(String),
    }

    impl Element {
        pub(super) fn attr(&self, name: &str) -> Option<&str> {
            self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
        }

        /// Child elements in document order.
        pub(super) fn elements(&self) -> impl Iterator<Item = &Element> {
            self.children.iter().filter_map(|n| match n {
                Node::Element(e) => Some(e),
                Node::Text(_) => None,
            })
        }

        /// The element's own text.
        pub(super) fn text(&self) -> String {
            self.children
                .iter()
                .filter_map(|n| match n {
                    Node::Text(t) => Some(t.as_str()),
                    Node::Element(_) => None,
                })
                .collect()
        }
    }

    fn is_space(c: char) -> bool {
        matches!(c, ' ' | '\t' | '\n' | '\r')
    }

    /// Resolves the five named references and their numeric spellings. Any other reference, a
    /// numeric one included, stays as written: that is what the web app's reader does, and the
    /// qualifiers that keep an element as it was written have to agree with it.
    pub(super) fn decode(text: &str) -> String {
        if !text.contains('&') {
            return text.to_string();
        }
        let mut out = text.to_string();
        // `&amp;` goes last, so that `&amp;lt;` is `&lt;` and not `<`.
        for (to, spellings) in [
            ("'", ["&apos;", "&#39;", "&#x27;"]),
            (">", ["&gt;", "&#62;", "&#x3E;"]),
            ("<", ["&lt;", "&#60;", "&#x3C;"]),
            ("\"", ["&quot;", "&#34;", "&#x22;"]),
            ("&", ["&amp;", "&#38;", "&#x26;"]),
        ] {
            for spelling in spellings {
                if out.contains(spelling) {
                    out = out.replace(spelling, to);
                }
            }
        }
        out
    }

    fn is_name(name: &str) -> bool {
        let start = |c: char| c.is_alphabetic() || c == '_' || c == ':';
        let mut chars = name.chars();
        chars.next().is_some_and(start) && chars.all(|c| start(c) || c.is_numeric() || c == '-' || c == '.')
    }

    /// True when every `&` in a stretch of text begins a reference.
    fn references_end(text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut at = 0;
        while let Some(found) = bytes.get(at..).and_then(|rest| rest.iter().position(|b| *b == b'&')) {
            let mut i = at + found + 1;
            let numeric = bytes.get(i) == Some(&b'#');
            let hex = numeric && bytes.get(i + 1) == Some(&b'x');
            i += usize::from(numeric) + usize::from(hex);
            let from = i;
            let part = |b: u8| match (numeric, hex) {
                (true, true) => b.is_ascii_hexdigit(),
                (true, false) => b.is_ascii_digit(),
                _ => b.is_ascii_alphanumeric() || b == b'_',
            };
            while bytes.get(i).is_some_and(|b| part(*b)) && (numeric || i - from < 20) {
                i += 1;
            }
            // A name may be empty only for a numeric reference; `&;` is nothing.
            if bytes.get(i) != Some(&b';') || (!numeric && i == from) {
                return false;
            }
            at = i + 1;
        }
        true
    }

    /// The attributes of a start tag. Strictly, each needs a space before it, a name, `=` and a
    /// quoted value, and no name may repeat.
    fn attributes(text: &str, strict: bool) -> Option<Vec<(String, String)>> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut rest = text;
        loop {
            let unspaced = rest.trim_start_matches(is_space);
            if unspaced.is_empty() {
                return Some(out);
            }
            if strict && unspaced.len() == rest.len() {
                return None;
            }
            let name_end = unspaced.find(|c: char| is_space(c) || c == '=').unwrap_or(unspaced.len());
            let (name, after) = unspaced.split_at_checked(name_end)?;
            let quoted = after.trim_start_matches(is_space).strip_prefix('=').map(|value| value.trim_start_matches(is_space));
            let quote = quoted.and_then(|value| value.chars().next()).filter(|c| *c == '"' || *c == '\'');
            let value = quote.and_then(|quote| {
                let body = quoted?.get(1..)?;
                let end = body.find(quote)?;
                Some((body.get(..end)?, body.get(end + 1..)?))
            });
            match value {
                Some((value, remainder)) => {
                    if out.len() >= MAX_ATTRIBUTES || (strict && (!is_name(name) || out.iter().any(|(k, _)| k == name))) {
                        return None;
                    }
                    out.push((name.to_string(), decode(value)));
                    rest = remainder;
                }
                None if strict => return None,
                // Leniently, what cannot be read ends the attributes.
                None => return Some(out),
            }
        }
    }

    /// Where a tag that starts at the beginning of `rest` ends: its `>` outside any quoted value.
    fn tag_end(rest: &str) -> Option<usize> {
        let mut quote = None;
        for (at, c) in rest.char_indices() {
            match (quote, c) {
                (Some(open), _) if c == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => return Some(at),
                (None, _) => {}
            }
        }
        None
    }

    struct Reader<'a> {
        text: &'a str,
        at: usize,
        strict: bool,
        /// The elements that are open, outermost first.
        open: Vec<Element>,
        root: Option<Element>,
        /// Text read since the last tag.
        pending: String,
        elements: usize,
        /// The different tags seen so far.
        names: Vec<String>,
    }

    impl Reader<'_> {
        fn flush(&mut self) {
            let text = std::mem::take(&mut self.pending);
            if let Some(top) = self.open.last_mut().filter(|_| !text.is_empty()) {
                top.children.push(Node::Text(text));
            }
        }

        fn attach(&mut self, element: Element) -> Option<()> {
            match self.open.last_mut() {
                Some(parent) => parent.children.push(Node::Element(element)),
                None if self.root.is_none() => self.root = Some(element),
                None => return None,
            }
            Some(())
        }

        fn run(&mut self) -> Option<()> {
            while let Some(rest) = self.text.get(self.at..).filter(|rest| !rest.is_empty()) {
                if rest.starts_with('<') {
                    self.markup(rest)?;
                } else {
                    let end = rest.find('<').unwrap_or(rest.len());
                    let chunk = rest.get(..end)?;
                    if self.open.is_empty() {
                        // Outside the root there is only white space.
                        if self.strict && !chunk.chars().all(is_space) {
                            return None;
                        }
                    } else {
                        if self.strict && !references_end(chunk) {
                            return None;
                        }
                        self.pending.push_str(&decode(chunk));
                    }
                    self.at += end;
                }
            }
            Some(())
        }

        fn markup(&mut self, rest: &str) -> Option<()> {
            if rest.starts_with("<!--") {
                self.at += rest.find("-->")? + 3;
            } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
                let end = body.find("]]>")?;
                if !self.open.is_empty() {
                    self.pending.push_str(body.get(..end)?);
                }
                self.at += 9 + end + 3;
            } else if rest.starts_with("<!") {
                // A document type. The caller has refused the block already when it matters.
                if self.strict {
                    return None;
                }
                self.at += rest.find('>')? + 1;
            } else if let Some(body) = rest.strip_prefix("<?") {
                let end = body.find("?>")?;
                self.instruction(body.get(..end)?)?;
                self.at += 2 + end + 2;
            } else if let Some(body) = rest.strip_prefix("</") {
                let end = body.find('>')?;
                self.close(body.get(..end)?.trim_end_matches(is_space))?;
                self.at += 2 + end + 1;
            } else {
                let end = tag_end(rest)?;
                self.start(rest.get(1..end)?)?;
                self.at += end + 1;
            }
            Some(())
        }

        /// A processing instruction. The XML declaration is one, and belongs at the very start.
        fn instruction(&mut self, body: &str) -> Option<()> {
            let (target, rest) = body.split_at_checked(body.find(is_space).unwrap_or(body.len()))?;
            if self.strict && target == "xml" && self.at != 0 {
                return None;
            }
            if self.open.is_empty() {
                return Some(());
            }
            self.flush();
            let attrs = attributes(rest, false).unwrap_or_default();
            self.attach(Element { name: format!("?{target}"), attrs, children: Vec::new() })
        }

        fn close(&mut self, name: &str) -> Option<()> {
            let Some(top) = self.open.last() else {
                return if self.strict { None } else { Some(()) };
            };
            if self.strict && top.name != name {
                return None;
            }
            self.flush();
            let element = self.open.pop()?;
            self.attach(element)
        }

        fn start(&mut self, inner: &str) -> Option<()> {
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(inner) => (inner, true),
                None => (inner, false),
            };
            let (name, rest) = inner.split_at_checked(inner.find(is_space).unwrap_or(inner.len()))?;
            if self.strict && !is_name(name) {
                return None;
            }
            // One root, and no deeper or larger than a block of features ever is.
            if (self.open.is_empty() && self.root.is_some()) || self.open.len() >= MAX_DEPTH || self.elements >= MAX_ELEMENTS {
                return None;
            }
            if !self.names.iter().any(|known| known == name) {
                if self.names.len() >= MAX_NAMES {
                    return None;
                }
                self.names.push(name.to_string());
            }
            let attrs = attributes(rest, self.strict)?;
            self.elements += 1;
            self.flush();
            let element = Element { name: name.to_string(), attrs, children: Vec::new() };
            if empty {
                self.attach(element)
            } else {
                self.open.push(element);
                Some(())
            }
        }
    }

    /// Reads a document and gives its root element.
    pub(super) fn parse(text: &str, strict: bool) -> Option<Element> {
        let mut reader = Reader { text, at: 0, strict, open: Vec::new(), root: None, pending: String::new(), elements: 0, names: Vec::new() };
        let whole = reader.run().is_some() && reader.open.is_empty();
        if strict && !whole {
            return None;
        }
        // Leniently, a document that stopped early keeps its elements. Text that no tag followed
        // is not kept.
        reader.pending.clear();
        while let Some(element) = reader.open.pop() {
            reader.attach(element)?;
        }
        reader.root
    }
}

/// An element as the web app keeps it: the JSON its XML reader makes of one, written out the way
/// JavaScript writes JSON.
mod json {
    use super::xml::{Element, Node};

    #[derive(Clone, Debug, PartialEq)]
    pub(super) enum Json {
        Str(String),
        Arr(Vec<Json>),
        /// Keys in the order they were first set.
        Obj(Vec<(String, Json)>),
    }

    fn set(entries: &mut Vec<(String, Json)>, key: &str, value: Json) {
        match entries.iter_mut().find(|(k, _)| k == key) {
            Some((_, slot)) => *slot = value,
            None => entries.push((key.to_string(), value)),
        }
    }

    /// What the web app's XML reader makes of an element.
    ///
    /// Child elements come first, in the order each tag first appears, and a tag that repeats, or
    /// one named in `lists`, is a list. Then the element's own text, as `#text`. Then its
    /// attributes, each replacing a child of the same name. An element with nothing but text is
    /// that text, and an element with nothing at all is the empty text.
    pub(super) fn value_of(element: &Element, lists: &[&str]) -> Json {
        let mut entries: Vec<(String, Json)> = Vec::new();
        let mut text = String::new();
        for child in &element.children {
            match child {
                Node::Text(t) => text.push_str(t),
                Node::Element(e) => {
                    let value = value_of(e, lists);
                    match entries.iter_mut().find(|(k, _)| *k == e.name) {
                        Some((_, Json::Arr(items))) => items.push(value),
                        Some((_, slot)) => *slot = Json::Arr(vec![slot.clone(), value]),
                        None => entries.push((e.name.clone(), if lists.contains(&e.name.as_str()) { Json::Arr(vec![value]) } else { value })),
                    }
                }
            }
        }
        if !text.is_empty() {
            set(&mut entries, "#text", Json::Str(text));
        }
        if element.attrs.is_empty() {
            return match entries.as_slice() {
                [] => Json::Str(String::new()),
                [(key, only)] if key == "#text" => only.clone(),
                _ => Json::Obj(entries),
            };
        }
        for (key, value) in &element.attrs {
            let value = Json::Str(value.clone());
            set(&mut entries, key, if lists.contains(&key.as_str()) { Json::Arr(vec![value]) } else { value });
        }
        Json::Obj(entries)
    }

    /// A value as a list: a list is itself, nothing is the empty list, anything else is a list of
    /// one.
    pub(super) fn list(value: Option<&Json>) -> Vec<&Json> {
        match value {
            Some(Json::Arr(items)) => items.iter().collect(),
            Some(other) => vec![other],
            None => Vec::new(),
        }
    }

    impl Json {
        pub(super) fn get(&self, key: &str) -> Option<&Json> {
            match self {
                Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }

        pub(super) fn as_str(&self) -> Option<&str> {
            match self {
                Json::Str(text) => Some(text),
                _ => None,
            }
        }

        /// `String(value)`.
        pub(super) fn as_js_string(&self) -> String {
            match self {
                Json::Str(text) => text.clone(),
                Json::Arr(items) => items.iter().map(Json::as_js_string).collect::<Vec<_>>().join(","),
                Json::Obj(_) => "[object Object]".to_string(),
            }
        }

        /// `JSON.stringify(value)`.
        pub(super) fn to_text(&self) -> String {
            let mut out = String::new();
            self.write(&mut out);
            out
        }

        fn write(&self, out: &mut String) {
            match self {
                Json::Str(text) => write_string(out, text),
                Json::Arr(items) => {
                    out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        item.write(out);
                    }
                    out.push(']');
                }
                Json::Obj(entries) => write_object(out, entries.iter()),
            }
        }
    }

    fn write_object<'a>(out: &mut String, entries: impl Iterator<Item = &'a (String, Json)>) {
        out.push('{');
        for (i, (key, value)) in entries.enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_string(out, key);
            out.push(':');
            value.write(out);
        }
        out.push('}');
    }

    /// `JSON.stringify` of an object with these entries.
    pub(super) fn object_text<'a>(entries: impl Iterator<Item = &'a (String, Json)>) -> String {
        let mut out = String::new();
        write_object(&mut out, entries);
        out
    }

    fn write_string(out: &mut String, text: &str) {
        out.push('"');
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
                c => out.push(c),
            }
        }
        out.push('"');
    }

    /// Base64 with padding, or the URL-safe alphabet without it.
    pub(super) fn base64(bytes: &[u8], url_safe: bool) -> String {
        let alphabet: &[u8; 64] = if url_safe {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        } else {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        };
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let group = chunk.iter().fold(0u32, |all, byte| (all << 8) | u32::from(*byte)) << (8 * (3 - chunk.len()));
            for i in 0..4 {
                if i <= chunk.len() {
                    let index = ((group >> (18 - 6 * i)) & 63) as usize;
                    out.push(alphabet.get(index).map_or('A', |b| char::from(*b)));
                } else if !url_safe {
                    out.push('=');
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::json::{Json, base64, value_of};
    use super::{Colour, js, locate, normalize_colour, raw_span, xml};

    fn kept(document: &str, lists: &[&str]) -> String {
        value_of(&xml::parse(document, true).unwrap(), lists).to_text()
    }

    #[test]
    fn an_element_is_kept_children_first_then_text_then_attributes() {
        assert_eq!(kept(r#"<F name="x" type="CDS"><S range="1-5"/></F>"#, &["S"]), r#"{"S":[{"range":"1-5"}],"name":"x","type":"CDS"}"#);
        assert_eq!(kept(r#"<F a="1">hello<S/>world</F>"#, &[]), r##"{"S":"","#text":"helloworld","a":"1"}"##);
        assert_eq!(kept("<F><A>1</A><A>2</A><B x='1'/><A>3</A></F>", &[]), r#"{"A":["1","2","3"],"B":{"x":"1"}}"#);
        assert_eq!(kept("<F><A></A><B/><C> </C><D a='1'>t</D></F>", &[]), r##"{"A":"","B":"","C":" ","D":{"#text":"t","a":"1"}}"##);
        assert_eq!(kept("<F name='x'><name>child</name></F>", &[]), r#"{"name":"x"}"#);
        assert_eq!(kept("<F S='attr'/>", &["S"]), r#"{"S":["attr"]}"#);
    }

    #[test]
    fn only_the_five_named_references_are_resolved() {
        assert_eq!(kept(r#"<F t="&lt;&gt;&quot;&apos;&amp;lt; &#65; &nbsp; &amp"/>"#, &[]), r#"{"t":"<>\"'&lt; &#65; &nbsp; &amp"}"#);
        assert_eq!(kept("<F t='a\nb\tc'/>", &[]), r#"{"t":"a\nb\tc"}"#);
        assert_eq!(kept("<F><![CDATA[raw <b> &amp;]]></F>", &[]), r#""raw <b> &amp;""#);
    }

    #[test]
    fn a_document_that_is_not_well_formed_is_not_read_strictly() {
        for bad in [
            "",
            "<F>",
            "<F></G>",
            "<F/><G/>",
            "<F/>junk",
            "junk<F/>",
            "<F a=1/>",
            "<F a/>",
            "<F a='1' a='2'/>",
            "<F a='1'b='2'/>",
            "<F>a & b</F>",
            "<F>&;</F>",
            "<1F/>",
            "<!DOCTYPE F><F/>",
            "<F><!-- open</F>",
            " <?xml version='1.0'?><F/>",
        ] {
            assert!(xml::parse(bad, true).is_none(), "{bad}");
        }
        for good in ["<?xml version='1.0'?>\n<F/>\n", "<F a = '1' />", "<F a='<>'/>", "<F>&amp; &#65; &x;</F>", "<F><!-- c --></F><!-- d -->"] {
            assert!(xml::parse(good, true).is_some(), "{good}");
        }
    }

    #[test]
    fn a_damaged_document_keeps_what_it_had_when_read_leniently() {
        let root = xml::parse("<Notes><Description>oops</Notes>", false).unwrap();
        assert_eq!(root.elements().next().unwrap().text(), "oops");
        let root = xml::parse("<Notes><Description>d", false).unwrap();
        assert_eq!(root.elements().next().unwrap().text(), "");
        assert!(xml::parse("hello", false).is_none());
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = format!("{}{}", "<a>".repeat(200), "</a>".repeat(200));
        assert!(xml::parse(&deep, true).is_none());
        assert!(xml::parse(&deep, false).is_some());
    }

    #[test]
    fn json_is_written_as_javascript_writes_it() {
        let value = Json::Obj(vec![("a\"b".to_string(), Json::Arr(vec![Json::Str("1".to_string()), Json::Str("line\n\u{1}\\ é".to_string())]))]);
        assert_eq!(value.to_text(), r#"{"a\"b":["1","line\n\u0001\\ é"]}"#);
    }

    #[test]
    fn base64_pads_and_the_url_safe_form_does_not() {
        assert_eq!(base64(b"", false), "");
        assert_eq!(base64(b"f", false), "Zg==");
        assert_eq!(base64(b"fo", false), "Zm8=");
        assert_eq!(base64(b"foo", false), "Zm9v");
        assert_eq!(base64(b"fo", true), "Zm8");
        assert_eq!(base64(&[0xfb, 0xff], false), "+/8=");
        assert_eq!(base64(&[0xfb, 0xff], true), "-_8");
    }

    #[test]
    fn a_string_is_the_number_javascript_says_it_is() {
        assert_eq!(js::number(""), 0.0);
        assert_eq!(js::number(" 12 "), 12.0);
        assert_eq!(js::number("0x10"), 16.0);
        assert_eq!(js::number("1e1"), 10.0);
        assert_eq!(js::number("-3"), -3.0);
        assert_eq!(js::number(".5"), 0.5);
        assert_eq!(js::number("1."), 1.0);
        assert_eq!(js::number("-Infinity"), f64::NEG_INFINITY);
        for not in ["abc", "3abc", "1,000", "inf", "nan", "infinity", ".", "e3", "1e", "--1", "0x", "1_0"] {
            assert!(js::number(not).is_nan(), "{not}");
        }
        assert_eq!(js::number_text(1.0), "1");
        assert_eq!(js::number_text(1.5), "1.5");
        assert_eq!(js::number_text(-0.0), "0");
        assert_eq!(js::number_text(1e21), "1e+21");
        assert_eq!(js::number_text(1e-7), "1e-7");
    }

    #[test]
    fn a_range_is_one_based_and_falls_back_to_the_ends_of_the_sequence() {
        let at = |range: &str, circular: bool| {
            let (start, end, _) = locate(&[raw_span(range)], 40, circular);
            (start, end)
        };
        assert_eq!(at("3-10", true), (2, 9));
        assert_eq!(at("35-5", true), (34, 4));
        assert_eq!(at("35-5", false), (0, 4));
        assert_eq!(at("0-0", true), (0, 0));
        assert_eq!(at("50-60", true), (0, 39));
        assert_eq!(at("5", true), (4, 39));
        assert_eq!(at("x", true), (0, 39));
        assert_eq!(locate(&[], 40, true), (0, 0, Vec::new()));
    }

    #[test]
    fn pieces_are_kept_only_when_each_lies_on_the_sequence_inside_the_feature() {
        let pieces = |ranges: &[&str], circular: bool| locate(&ranges.iter().map(|r| raw_span(r)).collect::<Vec<_>>(), 40, circular);
        assert_eq!(pieces(&["3-10", "20-30"], true), (2, 29, vec![(2, 9), (19, 29)]));
        assert_eq!(pieces(&["20-30", "3-10"], true), (19, 9, vec![(19, 29), (2, 9)]));
        assert_eq!(pieces(&["3-20", "5-8"], true), (2, 7, Vec::new()));
        assert_eq!(pieces(&["3-10", "x"], true), (2, 39, Vec::new()));
        // The web app keeps a piece that runs past the end of the sequence. Bios never does.
        assert_eq!(pieces(&["3-10", "20-90"], true), (2, 39, Vec::new()));
        assert_eq!(pieces(&["35-5", "10-20"], false), (0, 19, Vec::new()));
    }

    #[test]
    fn a_colour_is_kept_only_in_a_form_genbank_can_hold() {
        assert_eq!(normalize_colour(" #F00 "), Colour::Keep("#ff0000".to_string()));
        assert_eq!(normalize_colour("#FF000080"), Colour::Keep("#ff0000".to_string()));
        assert_eq!(normalize_colour("ff0000"), Colour::Keep("#ff0000".to_string()));
        assert_eq!(normalize_colour("RGB(1, 2,3)"), Colour::Keep("rgb(1, 2,3)".to_string()));
        assert_eq!(normalize_colour("rgba(1,2,3,0.5)"), Colour::Keep("rgba(1,2,3,0.5)".to_string()));
        assert_eq!(normalize_colour("Black"), Colour::Keep("black".to_string()));
        assert_eq!(normalize_colour("noColor"), Colour::Nothing);
        assert_eq!(normalize_colour(""), Colour::Nothing);
        assert_eq!(normalize_colour("blurple"), Colour::Unreadable("blurple".to_string()));
        assert_eq!(normalize_colour("rgb(1,2)"), Colour::Unreadable("rgb(1,2)".to_string()));
    }
}
