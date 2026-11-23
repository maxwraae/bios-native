//! EMBL flat-file reader. Ported from the Bios web app (`packages/formats/src/embl.ts`).
//!
//! An EMBL record is turned into GenBank text and read by [`crate::genbank::read`]. The two formats
//! share one feature table (both are INSDC: the same location grammar, the same qualifiers, the
//! same doubled quote), so a second reader would be a second copy of the fiddliest grammar in the
//! format. The feature lines even line up: `FT` and three spaces put the key in the column GenBank
//! puts it in, so two spaces in place of the `FT` give a GenBank feature table as it stands.
//!
//! What cannot be copied across is the sequence: EMBL writes the running coordinate at the end of
//! a line and GenBank at the start, so the block is rebuilt from the bases.
//!
//! One difference from the web app: it stamps the day the file was opened on the `LOCUS` line it
//! builds. An EMBL `ID` line holds no date, so here the construct's date is left empty.

use crate::genbank;
use crate::model::Construct;

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The text holds no record at all.
    NoRecord,
    /// The first record did not read as a construct once it was GenBank.
    Unreadable(genbank::ReadError),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::NoRecord => write!(f, "this EMBL file holds no record"),
            ReadError::Unreadable(why) => write!(f, "this EMBL file could not be read: {why}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// An EMBL file that was read.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// The first record.
    pub construct: Construct,
    /// How many records the file holds.
    pub records: usize,
    /// Things the person should know that did not stop the file from opening.
    pub notices: Vec<String>,
}

/// Splits a flat file of several records on its `//` terminators.
pub fn split_records(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut flush = |current: &mut Vec<&str>| {
        if current.iter().any(|l| !l.trim().is_empty()) {
            out.push(current.join("\n"));
        }
        current.clear();
    };
    for line in lines(text) {
        if line.strip_prefix("//").is_some_and(|rest| rest.trim().is_empty()) {
            flush(&mut current);
        } else {
            current.push(line);
        }
    }
    flush(&mut current);
    out
}

/// The lines of `text`, split as the web app splits them: on a line feed, with or without a
/// carriage return before it. The text after the last line feed is a line too, even when empty.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

struct Ident {
    name: String,
    circular: bool,
    division: String,
}

/// True when `word` stands in `text` as a field of its own: after the start, a `;` or a space,
/// and before the end, a `;` or a space. Letter case is ignored.
fn has_field(text: &str, word: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let edge = |c: Option<char>| c.is_none_or(|c| c == ';' || c.is_whitespace());
    lower.match_indices(word).any(|(at, found)| {
        let before = lower.get(..at).and_then(|head| head.chars().next_back());
        let after = lower.get(at + found.len()..).and_then(|tail| tail.chars().next());
        edge(before) && edge(after)
    })
}

/// The text of `line` from its `from`th character.
fn tail(line: &str, from: usize) -> &str {
    line.char_indices().nth(from).map_or("", |(at, _)| line.get(at..).unwrap_or_default())
}

/// Reads the `ID` line.
///
/// The modern form is separated by semicolons, the legacy form by spaces:
///
/// ```text
/// ID   X56734; SV 1; linear; mRNA; STD; PLN; 1859 BP.
/// ID   X56734  standard; DNA; PLN; 1859 BP.
/// ```
///
/// The topology is the field that matters most and the one most likely to be missing. A plasmid
/// read as linear is a different molecule, so a line that does not say is never read as circular.
fn read_ident(line: &str) -> Ident {
    let rest = tail(line, 2).trim();
    let parts: Vec<&str> = rest.split(';').map(str::trim).collect();
    let name = parts.first().and_then(|p| p.split_whitespace().next()).unwrap_or_default().to_string();
    // The division is a three-letter INSDC code near the end; SYN is the honest default for
    // something a person built. `STD` is the data class. The web app also takes the molecule
    // type for the division when it is three capitals (`DNA`), which every plasmid's is; here
    // the molecule type is passed over.
    let code = |p: &&&str| p.len() == 3 && p.chars().all(|c| c.is_ascii_uppercase()) && !["STD", "DNA", "RNA"].contains(*p);
    let division = parts.iter().find(code).map_or("SYN", |p| p).to_string();
    Ident { name, circular: has_field(rest, "circular"), division }
}

/// Lays bases out as GenBank's `ORIGIN` block has them: the coordinate first, then six tens.
fn origin_block(bases: &str) -> String {
    let mut lines = Vec::new();
    for (row, chunk) in bases.as_bytes().chunks(60).enumerate() {
        let tens: Vec<String> = chunk.chunks(10).map(|ten| String::from_utf8_lossy(ten).into_owned()).collect();
        lines.push(format!("{:>9} {}", row * 60 + 1, tens.join(" ")));
    }
    lines.join("\n")
}

/// Breaks `value` over lines of at most 79 characters, the first after `prefix` and the rest
/// indented to match. Nothing at all for an empty value.
///
/// A line is broken at its last space. A stretch with no space is broken where the line ends; the
/// web app also drops the character there, which loses a letter of a long link, so here it is
/// kept.
fn wrap(prefix: &str, value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let indent = prefix.chars().count();
    let width = 79usize.saturating_sub(indent).max(1);
    let mut out = Vec::new();
    let mut rest: Vec<char> = value.chars().collect();
    let mut head = prefix.to_string();
    while rest.len() > width {
        let space = rest.iter().take(width + 1).rposition(|c| *c == ' ').filter(|at| *at > 0);
        let (cut, skip) = match space {
            Some(at) => (at, 1),
            None => (width, 0),
        };
        out.push(format!("{head}{}", rest.iter().take(cut).collect::<String>()));
        rest.drain(..cut + skip);
        head = " ".repeat(indent);
    }
    out.push(format!("{head}{}", rest.iter().collect::<String>()));
    out
}

/// Turns one EMBL record into GenBank text, with the notices that reading it gave.
///
/// The result goes to the GenBank reader, so anything this gets subtly wrong shows up as a wrong
/// construct and not as an error. That is why the sequence block is rebuilt and not copied.
pub fn record_to_genbank(record: &str) -> (String, Vec<String>) {
    let mut ident = Ident { name: String::new(), circular: false, division: "SYN".to_string() };
    let (mut accession, mut description, mut keywords) = (Vec::new(), Vec::new(), Vec::new());
    let (mut comment, mut organism, mut features) = (Vec::new(), Vec::new(), Vec::new());
    let mut bases = String::new();
    let mut in_sequence = false;

    for line in lines(record) {
        if in_sequence {
            // Everything that is not a letter goes: the trailing coordinate, the spacing, the counts.
            bases.extend(line.chars().filter(char::is_ascii_alphabetic));
            continue;
        }
        let key: String = line.chars().take(2).collect();
        let body = tail(line, 5).trim();
        let unterminated = body.strip_suffix(';').unwrap_or(body);
        match key.as_str() {
            "ID" => ident = read_ident(line),
            "AC" => accession.push(unterminated),
            "DE" => description.push(body),
            "KW" => keywords.push(unterminated),
            "CC" => comment.push(body),
            "OS" => organism.push(body),
            "SQ" => in_sequence = true,
            // `FT` and three spaces put the key in column 6, where GenBank puts it too.
            "FT" => features.push(format!("  {}", tail(line, 2))),
            _ => {}
        }
    }

    let mut name: String = if ident.name.is_empty() { "unnamed".to_string() } else { ident.name.chars().take(16).collect() };
    if name.is_empty() {
        name = "unnamed".to_string();
    }
    let topology = if ident.circular { "circular" } else { "linear" };
    let or_dot = |joined: String| if joined.is_empty() { ".".to_string() } else { joined };

    let mut out = vec![format!("LOCUS       {name:<16} {:>11} bp    DNA     {topology:<8} {}", bases.len(), ident.division)];
    out.extend(wrap("DEFINITION  ", &or_dot(description.join(" "))));
    if !accession.is_empty() {
        out.extend(wrap("ACCESSION   ", &accession.join(" ")));
    }
    out.extend(wrap("KEYWORDS    ", &or_dot(keywords.join(" "))));
    if !organism.is_empty() {
        out.extend(wrap("SOURCE      ", &organism.join(" ")));
        out.extend(wrap("  ORGANISM  ", &organism.join(" ")));
    }
    if !comment.is_empty() {
        out.extend(wrap("COMMENT     ", &comment.join(" ")));
    }
    out.push("FEATURES             Location/Qualifiers".to_string());
    out.extend(features);
    out.push("ORIGIN".to_string());
    if !bases.is_empty() {
        out.push(origin_block(&bases.to_ascii_lowercase()));
    }
    out.push("//".to_string());

    let mut notices = Vec::new();
    if !ident.circular && !has_field(lines(record).next().unwrap_or_default(), "linear") {
        notices.push(format!("{name} doesn't say whether it is circular, so it opened as linear."));
    }
    (format!("{}\n", out.join("\n")), notices)
}

/// Turns an EMBL flat file into GenBank text, one record for each entry, with every record's
/// notices.
pub fn to_genbank(text: &str) -> (Vec<String>, Vec<String>) {
    let mut notices = Vec::new();
    let records = split_records(text)
        .iter()
        .map(|record| {
            let (genbank, said) = record_to_genbank(record);
            notices.extend(said);
            genbank
        })
        .collect();
    (records, notices)
}

/// Reads the first record of an EMBL flat file.
pub fn read(text: &str) -> Result<Read, ReadError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (records, notices) = to_genbank(text);
    let first = records.first().ok_or(ReadError::NoRecord)?;
    let construct = genbank::read(first).map_err(ReadError::Unreadable)?;
    Ok(Read { construct, records: records.len(), notices })
}

#[cfg(test)]
mod tests {
    use super::{has_field, read_ident, wrap};

    #[test]
    fn a_field_is_a_whole_word_between_separators() {
        assert!(has_field("X; SV 1; circular; DNA", "circular"));
        assert!(has_field("X56734  standard; Circular DNA; PLN", "circular"));
        assert!(has_field("circular", "circular"));
        assert!(!has_field("X; semicircular; DNA", "circular"));
        assert!(!has_field("X; circularised; DNA", "circular"));
    }

    #[test]
    fn the_division_is_the_three_capitals_that_are_not_the_data_class() {
        assert_eq!(read_ident("ID   X56734; SV 1; linear; mRNA; STD; PLN; 1859 BP.").division, "PLN");
        assert_eq!(read_ident("ID   pTEST; SV 1; circular; DNA; STD; 60 BP.").division, "SYN");
        assert_eq!(read_ident("ID   pMINI; SV 1; circular; DNA; STD; SYN; 240 BP.").division, "SYN");
        assert_eq!(read_ident("ID   X56734  standard; RNA; PLN; 1859 BP.").division, "PLN");
        assert_eq!(read_ident("ID").name, "");
    }

    #[test]
    fn a_long_value_breaks_at_a_space_and_keeps_every_letter() {
        assert_eq!(wrap("KEYWORDS    ", ""), Vec::<String>::new());
        assert_eq!(wrap("KEYWORDS    ", "short"), vec!["KEYWORDS    short"]);
        let words = "word ".repeat(20);
        let wrapped = wrap("DEFINITION  ", words.trim_end());
        assert_eq!(wrapped.len(), 2);
        assert!(wrapped.iter().all(|l| l.chars().count() <= 79));
        assert_eq!(wrapped[1], format!("            {}", "word ".repeat(7).trim_end()));
        let link = "x".repeat(150);
        let unbroken: String = wrap("DEFINITION  ", &link).iter().map(|l| l.trim_start_matches("DEFINITION").trim()).collect();
        assert_eq!(unbroken, link);
    }
}
