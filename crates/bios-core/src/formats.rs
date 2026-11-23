//! What a file is, decided from its bytes, and reading the kinds Bios reads.
//!
//! The content decides and the file's name is only used to talk about it. A reader that goes by
//! the extension, or that tries each format until one says yes, opens meeting notes as a plasmid:
//! a FASTA reader says yes to anything. So a file is either one of the formats named here, read by
//! that format's reader, or it is refused in a sentence a person can act on.

use crate::model::{Construct, Locus};
use crate::{embl, genbank, interchange, snapgene};

/// How much of a file [`sniff`] looks at. Nothing past this can change the answer.
pub const PROBE_BYTES: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    GenBank,
    Fasta,
    /// Nothing but bases: a vendor's `.seq`, or a sequence pasted into a text file.
    BareDna,
    SnapGene,
    Embl,
    /// GFF3, with its sequence after `##FASTA`.
    Gff,
    /// JBEI SEQ XML.
    Jbei,
    /// JSON: the Teselagen sequence shape, or anything else in braces.
    Json,
    /// Sequencing reads: FASTQ or a Sanger trace.
    Reads,
    /// A zip, a gzip, or an office document.
    Archive,
    Pdf,
    Image,
    Binary,
    /// Text that matches no sequence format.
    Text,
    Empty,
}

/// A file that was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub construct: Construct,
    pub format: Format,
    /// Things the person should know that did not stop the file from opening.
    pub notices: Vec<String>,
}

fn ascii_at(bytes: &[u8], from: usize, text: &str) -> bool {
    bytes.get(from..from + text.len()).is_some_and(|b| b == text.as_bytes())
}

/// True when these bytes are not text. A NUL is decisive; otherwise it is the share of control
/// characters, because a binary container can hold plenty of readable text.
fn looks_binary(bytes: &[u8]) -> bool {
    let head = bytes.get(..bytes.len().min(8192)).unwrap_or_default();
    if head.contains(&0) {
        return true;
    }
    let odd = head.iter().filter(|c| **c < 0x20 && !matches!(**c, 9 | 10 | 13)).count();
    !head.is_empty() && odd * 20 > head.len()
}

fn line_starts(text: &str, test: impl Fn(&str) -> bool) -> bool {
    text.lines().any(test)
}

/// Names the shape of a file from its first bytes. Never guesses: text that matches no signature
/// is [`Format::Text`], which is refused.
pub fn sniff(bytes: &[u8]) -> Format {
    let probe = bytes.get(..bytes.len().min(PROBE_BYTES)).unwrap_or_default();
    if probe.is_empty() {
        return Format::Empty;
    }
    // Binary signatures first, before anything is decoded.
    if probe.first() == Some(&0x09) && ascii_at(probe, 5, "SnapGene") {
        return Format::SnapGene;
    }
    if ascii_at(probe, 0, "ABIF") {
        return Format::Reads;
    }
    if probe.starts_with(&[0x50, 0x4b, 0x03, 0x04]) || probe.starts_with(&[0x1f, 0x8b]) {
        return Format::Archive;
    }
    if ascii_at(probe, 0, "%PDF-") {
        return Format::Pdf;
    }
    let image = [&[0x89, 0x50, 0x4e, 0x47][..], &[0xff, 0xd8, 0xff], b"GIF8", &[0x49, 0x49, 0x2a, 0x00], &[0x4d, 0x4d, 0x00, 0x2a]];
    if image.iter().any(|magic| probe.starts_with(magic)) {
        return Format::Image;
    }
    if looks_binary(probe) {
        return Format::Binary;
    }

    let text = String::from_utf8_lossy(probe);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let head: String = text.chars().take(4096).collect();
    let body = head.trim_start();
    if body.is_empty() {
        return Format::Empty;
    }
    if line_starts(&head, |l| l.strip_prefix("LOCUS").is_some_and(|rest| rest.starts_with(char::is_whitespace))) {
        return Format::GenBank;
    }
    // "ID   " can begin a line of prose; an EMBL record always has another flat-file key too.
    let embl_key =
        |l: &str| ["SQ", "FT", "AC", "DE", "KW", "OS"].iter().any(|k| l.strip_prefix(k).is_some_and(|rest| rest.starts_with(char::is_whitespace)));
    if line_starts(&head, |l| l.starts_with("ID   ")) && line_starts(&head, embl_key) {
        return Format::Embl;
    }
    if body.starts_with("##gff-version") {
        return Format::Gff;
    }
    if body.starts_with('<') {
        return if head.contains("seq:seq") && head.contains("jbei") { Format::Jbei } else { Format::Text };
    }
    if body.starts_with('{') || body.starts_with('[') {
        return Format::Json;
    }
    if body.starts_with('>') || body.starts_with(';') {
        return Format::Fasta;
    }
    if body.starts_with('@') {
        let lines: Vec<&str> = body.lines().collect();
        if let [_, bases, plus, quality, ..] = lines.as_slice()
            && plus.starts_with('+')
            && bases.len() == quality.len()
        {
            return Format::Reads;
        }
    }
    // The length floor keeps a two-word note ("AGT CAT") from being read as a construct.
    let iupac = |c: char| "acgturykmswbdhvnACGTURYKMSWBDHVN.-*".contains(c) || c.is_whitespace();
    if body.chars().all(iupac) && body.chars().filter(|c| !c.is_whitespace()).count() >= 20 {
        return Format::BareDna;
    }
    Format::Text
}

const READABLE: &str = "Bios reads GenBank (.gb), FASTA, SnapGene (.dna), EMBL, GFF3, JBEI SEQ XML and Teselagen JSON.";

/// Why a file of this format was not opened, in a sentence a person can act on.
fn refusal(format: Format, name: &str, bytes: &[u8]) -> String {
    match format {
        Format::Empty => format!("{name} came back empty. If it lives in iCloud or Dropbox, make sure it is downloaded locally, then try again."),
        Format::Reads => format!("{name} is a sequencing read file, not a map."),
        Format::Archive => format!("{name} is an archive or a document. Unpack it, or export each construct as GenBank (.gb), and use those."),
        Format::Pdf => format!("{name} is a PDF, not a sequence file. {READABLE} Export one of those from wherever the map lives."),
        Format::Image => format!("{name} is an image. Bios needs the sequence, not a picture of it."),
        Format::Binary => format!("Bios can't read {name}. {READABLE} Export one of those and use it."),
        Format::Text
        | Format::GenBank
        | Format::Fasta
        | Format::BareDna
        | Format::SnapGene
        | Format::Embl
        | Format::Gff
        | Format::Jbei
        | Format::Json => {
            let seen: String = String::from_utf8_lossy(bytes.get(..bytes.len().min(200)).unwrap_or_default())
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(40)
                .collect();
            let shown = if seen.is_empty() { String::new() } else { format!(" It starts \"{seen}\".") };
            format!("{name} doesn't look like any sequence format Bios knows.{shown} GenBank starts with LOCUS, FASTA with \">\".")
        }
    }
}

/// Why a file that is one of the formats Bios reads did not open: it looks like `label` and does
/// not read as it.
fn unreadable(name: &str, label: &str) -> String {
    format!(
        "{name} looks like {label} but Bios couldn't read it. Nothing was opened. If it opens in the tool that wrote it, exporting a fresh GenBank usually fixes this."
    )
}

fn human_size(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    match bytes {
        b if b < 1024 => format!("{b} bytes"),
        b if b < MIB => format!("{} KB", (b + 512) / 1024),
        b if b < 1024 * MIB => format!("{} MB", (b + MIB / 2) / MIB),
        b => format!("{:.1} GB", b as f64 / (1024 * MIB) as f64),
    }
}

/// Why a SnapGene file did not open. A container whose blocks do not tile the file exactly was
/// damaged on the way here, and reading half of it gives a plausible construct of the wrong
/// length with none of its features. It is refused whole.
fn snapgene_refusal(name: &str, why: &snapgene::ReadError) -> String {
    match why {
        snapgene::ReadError::TooLarge(bytes) => {
            format!("{name} is {}, which is far bigger than any plasmid. Bios opens maps, not whole datasets.", human_size(*bytes))
        }
        snapgene::ReadError::NoSequence => format!("{name} looks like SnapGene but holds no sequence. Nothing was opened."),
        snapgene::ReadError::Damaged | snapgene::ReadError::Unwieldy => unreadable(name, "SnapGene"),
    }
}

fn stem(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem).to_string()
}

fn from_bases(name: String, sequence: String) -> Construct {
    Construct { name, sequence, locus: Locus { molecule: "DNA".into(), ..Locus::default() }, ..Construct::default() }
}

fn bases_of(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase()).collect()
}

/// Reads the first record of a FASTA file. The name is the first word of its header line.
fn read_fasta(text: &str, file_name: &str) -> Option<(Construct, usize)> {
    let mut records = 0;
    let (mut name, mut sequence) = (String::new(), String::new());
    for line in text.lines() {
        if let Some(header) = line.strip_prefix('>') {
            records += 1;
            if records == 1 {
                name = header.split_whitespace().next().unwrap_or_default().to_string();
            }
        } else if records == 1 && !line.starts_with(';') {
            sequence.push_str(&bases_of(line));
        }
    }
    if sequence.is_empty() {
        return None;
    }
    if name.is_empty() {
        name = stem(file_name);
    }
    Some((from_bases(name, sequence), records))
}

/// Reads a sequence file of any kind Bios reads. `file_name` is used for a construct that has no
/// name of its own and to name the file in a refusal.
pub fn read(bytes: &[u8], file_name: &str) -> Result<Opened, String> {
    let shown = if file_name.is_empty() { "That file" } else { file_name.rsplit(['/', '\\']).next().unwrap_or(file_name) };
    let format = sniff(bytes);
    // A binary container is read from its bytes. Decoding it as text first is what damages one.
    if format == Format::SnapGene {
        let read = snapgene::read(bytes, file_name).map_err(|why| snapgene_refusal(shown, &why))?;
        return Ok(Opened { construct: read.construct, format, notices: read.notices });
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut notices = Vec::new();
    let construct = match format {
        Format::GenBank => {
            let records = text.lines().filter(|l| l.strip_prefix("LOCUS").is_some_and(|rest| rest.starts_with(char::is_whitespace))).count();
            if records > 1 {
                notices.push(format!("{shown} holds {records} sequences. Bios opened the first."));
            }
            genbank::read(text).map_err(|_| unreadable(shown, "GenBank"))?
        }
        Format::Fasta => {
            let (construct, records) =
                read_fasta(text, file_name).ok_or_else(|| format!("{shown} looks like FASTA but holds no sequence. Nothing was opened."))?;
            if records > 1 {
                notices.push(format!("{shown} holds {records} sequences. Bios opened the first."));
            }
            construct
        }
        Format::BareDna => from_bases(stem(file_name), bases_of(text)),
        Format::Embl => {
            let read = embl::read(text).map_err(|_| unreadable(shown, "EMBL"))?;
            notices.extend(read.notices);
            if read.records > 1 {
                notices.push(format!("{shown} holds {} sequences. Bios opened the first.", read.records));
            }
            read.construct
        }
        Format::Gff => interchange::read_gff(text, &stem(file_name)).ok_or_else(|| {
            format!("{shown} is GFF3 without a sequence. Bios needs the bases too: export it with its ##FASTA section, or as GenBank.")
        })?,
        Format::Jbei => interchange::read_jbei(text, &stem(file_name)).ok_or_else(|| unreadable(shown, "JBEI SEQ XML"))?,
        Format::Json => {
            let (construct, records) = interchange::read_teselagen(text, &stem(file_name)).ok_or_else(|| {
                format!("{shown} is JSON, but not a sequence Bios knows: it needs a \"sequence\". Export the construct as GenBank and use that.")
            })?;
            if records > 1 {
                notices.push(format!("{shown} holds {records} sequences. Bios opened the first."));
            }
            construct
        }
        other => return Err(refusal(other, shown, bytes)),
    };
    Ok(Opened { construct, format, notices })
}

#[cfg(test)]
mod tests {
    use super::{Format, read, sniff};

    #[test]
    fn meeting_notes_are_not_a_plasmid() {
        let notes = b"Meeting notes, Tuesday. Order more tips. Ask about the freezer.\n";
        assert_eq!(sniff(notes), Format::Text);
        let refused = read(notes, "notes.txt").unwrap_err();
        assert_eq!(
            refused,
            "notes.txt doesn't look like any sequence format Bios knows. It starts \"Meeting notes, Tuesday. Order more tips.\". GenBank starts with LOCUS, FASTA with \">\"."
        );
    }

    #[test]
    fn the_content_decides_not_the_name() {
        let genbank = b"LOCUS       t                         12 bp    DNA     linear   SYN 01-JAN-2026\nORIGIN\n        1 acgtacgtac gt\n//\n";
        assert_eq!(sniff(genbank), Format::GenBank);
        assert_eq!(read(genbank, "plasmid.txt").unwrap().construct.sequence, "ACGTACGTACGT");
        assert_eq!(sniff(b"\tSnapGene"), Format::Text);
        assert_eq!(sniff(b"\x09\x00\x00\x00\x0eSnapGene\x00\x01"), Format::SnapGene);
        // A SnapGene container that stops early is damaged, and is refused whole.
        assert_eq!(
            read(b"\x09\x00\x00\x00\x0eSnapGene\x00\x01", "Plasmid.DNA").unwrap_err(),
            "Plasmid.DNA looks like SnapGene but Bios couldn't read it. Nothing was opened. If it opens in the tool that wrote it, exporting a fresh GenBank usually fixes this."
        );
        assert_eq!(sniff(b"%PDF-1.7 ..."), Format::Pdf);
        assert_eq!(sniff(b"PK\x03\x04rest"), Format::Archive);
        assert_eq!(sniff(b"ABIF\x00\x65"), Format::Reads);
        assert_eq!(sniff(b""), Format::Empty);
        assert_eq!(sniff(b"  \n\n"), Format::Empty);
    }

    #[test]
    fn fasta_gives_the_first_record_under_its_own_name() {
        let opened = read(b">gfp green fluorescent protein\nATGGTG\nagcaag\n>second\nTTTT\n", "anything.fa").unwrap();
        assert_eq!(opened.format, Format::Fasta);
        assert_eq!(opened.construct.name, "gfp");
        assert_eq!(opened.construct.sequence, "ATGGTGAGCAAG");
        assert!(!opened.construct.circular);
        assert_eq!(opened.notices, vec!["anything.fa holds 2 sequences. Bios opened the first.".to_string()]);
        assert!(read(b">empty\n", "e.fa").unwrap_err().contains("holds no sequence"));
    }

    #[test]
    fn bare_bases_need_to_be_long_enough_to_mean_it() {
        assert_eq!(sniff(b"AGT CAT"), Format::Text);
        let opened = read(b"acgtacgtac gtacgtacgt\nACGT\n", "/somewhere/oligo 7.seq").unwrap();
        assert_eq!(opened.format, Format::BareDna);
        assert_eq!(opened.construct.name, "oligo 7");
        assert_eq!(opened.construct.len(), 24);
    }

    #[test]
    fn a_fastq_is_reads_and_an_embl_file_is_read_as_one() {
        assert_eq!(sniff(b"@read1\nACGT\n+\nIIII\n"), Format::Reads);
        let embl = b"ID   X56734; SV 1; linear; mRNA; STD; PLN; 1859 BP.\nAC   X56734;\nSQ   Sequence 4 BP;\n     acgt\n//\n";
        assert_eq!(sniff(embl), Format::Embl);
        let opened = read(embl, "x.embl").unwrap();
        assert_eq!((opened.format, opened.construct.name.as_str(), opened.construct.sequence.as_str()), (Format::Embl, "X56734", "ACGT"));
        assert_eq!(read(b"ID   X; SV 1; linear; DNA; STD; SYN; 0 BP.\nAC   X;\n//\n", "x.embl").unwrap_err(), super::unreadable("x.embl", "EMBL"));
    }

    #[test]
    fn a_size_is_said_the_way_a_person_says_it() {
        assert_eq!(super::human_size(900), "900 bytes");
        assert_eq!(super::human_size(4096), "4 KB");
        assert_eq!(super::human_size(300 * 1024 * 1024), "300 MB");
        assert_eq!(super::human_size(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
    }
}
