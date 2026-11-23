//! GenBank flat-file writer.
//!
//! The inverse of [`crate::genbank::read`]: reading what this writes gives back the same
//! construct, header lines and unknown qualifiers included. Two details exist only to make that
//! true, because the reader joins a wrapped qualifier's lines without a space and trims each one:
//! a value is never broken next to a space, and never between the two quotes of an escaped quote.

use crate::model::{Construct, Feature, Fuzzy, LocationOperator, Segment, Strand};

#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    /// A construct with no bases is never written: an empty file where a plasmid used to be is
    /// the failure this refusal exists to prevent.
    Empty,
    /// The sequence holds something that is not a letter.
    NotDna,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Empty => write!(f, "this construct has no sequence, so nothing was written"),
            WriteError::NotDna => write!(f, "this construct's sequence holds characters that are not bases, so nothing was written"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Column where locations and qualifiers start, and how much fits from there to column 80.
const INDENT: usize = 21;
const WIDTH: usize = 80 - INDENT - 1;

/// Qualifiers that are written bare, with no value.
const FLAGS: [&str; 11] = [
    "pseudo",
    "partial",
    "environmental_sample",
    "focus",
    "germline",
    "macronuclear",
    "proviral",
    "rearranged",
    "ribosomal_slippage",
    "trans_splicing",
    "transgenic",
];

/// Renders `construct` as one GenBank record.
pub fn write(construct: &Construct) -> Result<String, WriteError> {
    if construct.sequence.is_empty() {
        return Err(WriteError::Empty);
    }
    if !construct.sequence.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(WriteError::NotDna);
    }

    let mut out = String::with_capacity(construct.sequence.len() * 2 + 4096);
    out.push_str(&locus_line(construct));
    out.push('\n');
    for line in &construct.header {
        out.push_str(line);
        out.push('\n');
    }
    for (i, comment) in construct.comments.iter().enumerate() {
        out.push_str(if i == 0 { "COMMENT     " } else { "            " });
        out.push_str(comment);
        out.push('\n');
    }
    out.push_str("FEATURES             Location/Qualifiers\n");
    for feature in &construct.features {
        write_feature(&mut out, feature, construct.circular, construct.sequence.len());
    }
    out.push_str("ORIGIN\n");
    write_origin(&mut out, &construct.sequence);
    out.push_str("//\n");
    Ok(out)
}

fn locus_line(c: &Construct) -> String {
    // A LOCUS name is one token; a name with spaces would be read back as its first word.
    let name: String = c.name.chars().map(|ch| if ch.is_whitespace() { '_' } else { ch }).collect();
    let molecule = if c.locus.molecule.is_empty() { "DNA" } else { &c.locus.molecule };
    let topology = if c.circular { "circular" } else { "linear" };
    let line = format!("LOCUS       {name:<16} {:>11} bp    {molecule:<7} {topology:<8} {} {}", c.sequence.len(), c.locus.division, c.locus.date);
    line.trim_end().to_string()
}

fn write_feature(out: &mut String, f: &Feature, circular: bool, length: usize) {
    let location = location_of(f, circular, length);
    let mut first = true;
    for piece in wrap_location(&location) {
        if first {
            out.push_str(&format!("     {:<16}", f.kind));
            first = false;
        } else {
            out.push_str(&" ".repeat(INDENT));
        }
        out.push_str(&piece);
        out.push('\n');
    }
    write_qualifier(out, "label", &f.name);
    // The name is the label. A reader that also kept the file's own label among the qualifiers
    // (the SnapGene one does) would otherwise have it written a second time.
    for (key, values) in f.notes.iter().filter(|(key, _)| key != "label") {
        for value in values {
            write_qualifier(out, key, value);
        }
    }
}

/// The feature's location in GenBank's 1-based notation, such as `complement(join(750..800,1..100))`.
pub fn location_of(f: &Feature, circular: bool, length: usize) -> String {
    // What could not be read goes back exactly as it came.
    if let Some(raw) = &f.location_unsupported {
        return raw.clone();
    }
    let mark = |fuzzy: Option<Fuzzy>| match fuzzy {
        Some(Fuzzy::Before) => "<",
        Some(Fuzzy::After) => ">",
        None => "",
    };
    let span = |s: &Segment| format!("{}{}..{}{}", mark(s.start_fuzzy), s.start + 1, mark(s.end_fuzzy), s.end + 1);
    let list = |pieces: &[Segment]| pieces.iter().map(span).collect::<Vec<_>>().join(",");
    let ordered = f.location_operator == Some(LocationOperator::Order);
    let inner = if let Some(pieces) = f.locations() {
        format!("{}({})", if ordered { "order" } else { "join" }, list(pieces))
    } else if circular && f.end < f.start && length > 0 {
        // Through the origin. `750..100` is what the reader gives back, but only a join is valid GenBank.
        format!("join({})", list(&[Segment::new(f.start, length - 1), Segment::new(0, f.end)]))
    } else {
        let one = span(&Segment { start: f.start, end: f.end, start_fuzzy: f.start_fuzzy, end_fuzzy: f.end_fuzzy });
        if ordered { format!("order({one})") } else { one }
    };
    match f.strand {
        Strand::Forward => inner,
        Strand::Reverse => format!("complement({inner})"),
    }
}

/// Breaks a long location after commas so that no line passes column 80.
fn wrap_location(location: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for part in location.split_inclusive(',') {
        if !current.is_empty() && current.len() + part.len() > WIDTH {
            lines.push(std::mem::take(&mut current));
        }
        current.push_str(part);
    }
    lines.push(current);
    lines
}

fn write_qualifier(out: &mut String, key: &str, value: &str) {
    let text = if value.is_empty() && FLAGS.contains(&key) {
        format!("/{key}")
    } else if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
        format!("/{key}={value}")
    } else {
        // One line break inside a value would end the qualifier for every reader.
        let flat: String = value.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
        format!("/{key}=\"{}\"", flat.replace('"', "\"\""))
    };
    for line in wrap_value(&text) {
        out.push_str(&" ".repeat(INDENT));
        out.push_str(&line);
        out.push('\n');
    }
}

/// Splits a qualifier into lines of at most [`WIDTH`] characters that the reader joins back exactly.
fn wrap_value(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut start = 0;
    while chars.len() - start > WIDTH {
        let mut cut = start + WIDTH;
        // Walk back to a break the reader survives. If none exists in this line, break at full
        // width: a run of spaces that long cannot be kept by any break.
        while cut > start + 1 && !safe_break(&chars, cut) {
            cut -= 1;
        }
        if !safe_break(&chars, cut) {
            cut = start + WIDTH;
        }
        lines.push(chars[start..cut].iter().collect());
        start = cut;
    }
    lines.push(chars[start..].iter().collect());
    lines
}

/// True when breaking before `chars[cut]` loses nothing on the way back in.
fn safe_break(chars: &[char], cut: usize) -> bool {
    let (Some(&before), Some(&after)) = (chars.get(cut - 1), chars.get(cut)) else {
        return false;
    };
    if before.is_whitespace() || after.is_whitespace() {
        return false;
    }
    // An escaped quote is two quotes. Cut inside a run of quotes and the first line can look like
    // a finished value, so that a next line starting with `/` is taken for a new qualifier.
    if before == '"' && after == '"' {
        return false;
    }
    // A line that starts with `/` is read as a new qualifier unless the value so far is visibly
    // still open, which the reader decides by counting its quotes.
    after != '/' || chars.iter().take(cut).filter(|c| **c == '"').count() % 2 == 1
}

fn write_origin(out: &mut String, sequence: &str) {
    let bases = sequence.as_bytes();
    for (row, line) in bases.chunks(60).enumerate() {
        out.push_str(&format!("{:>9}", row * 60 + 1));
        for group in line.chunks(10) {
            out.push(' ');
            out.extend(group.iter().map(|b| b.to_ascii_lowercase() as char));
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::{WIDTH, WriteError, wrap_value, write};
    use crate::genbank::read;
    use crate::model::Construct;

    fn with_note(value: &str) -> String {
        format!(
            "LOCUS       t                         12 bp    DNA     linear   SYN 01-JAN-2026\nFEATURES             Location/Qualifiers\n     misc_feature    1..4\n                     /label=\"x\"\n                     /note=\"{}\"\nORIGIN\n        1 acgtacgtac gt\n//\n",
            value.replace('"', "\"\"")
        )
    }

    fn note_after_round_trip(value: &str) -> String {
        let written = write(&read(&with_note(value)).unwrap()).unwrap();
        assert!(written.lines().all(|l| l.chars().count() <= 80), "a line passed column 80:\n{written}");
        read(&written).unwrap().features[0].note("note").unwrap()[0].clone()
    }

    #[test]
    fn a_label_kept_among_the_qualifiers_is_written_once() {
        let mut c = read("LOCUS       t                         12 bp    DNA     linear   SYN 01-JAN-2026\nFEATURES             Location/Qualifiers\n     misc_feature    1..4\n                     /label=\"x\"\nORIGIN\n        1 acgtacgtac gt\n//\n").unwrap();
        c.features[0].notes.push(("label".into(), vec!["x".into()]));
        let written = write(&c).unwrap();
        assert_eq!(written.matches("/label=").count(), 1, "{written}");
    }

    #[test]
    fn an_empty_construct_is_refused() {
        assert_eq!(write(&Construct::default()), Err(WriteError::Empty));
    }

    #[test]
    fn a_sequence_that_is_not_letters_is_refused() {
        let c = Construct { sequence: "ACGT 12".into(), ..Construct::default() };
        assert_eq!(write(&c), Err(WriteError::NotDna));
    }

    #[test]
    fn a_long_note_keeps_every_space() {
        let value = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen";
        assert_eq!(note_after_round_trip(value), value);
    }

    #[test]
    fn quotes_survive_wherever_the_line_breaks() {
        // Slide a quote across the place a line would break.
        for pad in 0..WIDTH + 4 {
            let value = format!("{}\"q\" and /slash after the break {}", "a".repeat(pad), "b".repeat(70));
            assert_eq!(note_after_round_trip(&value), value, "pad {pad}");
        }
    }

    #[test]
    fn a_slash_never_starts_a_line_that_could_be_read_as_a_qualifier() {
        for pad in 0..WIDTH + 4 {
            let value = format!("{}/x=1 {}", "a".repeat(pad), "c".repeat(70));
            assert_eq!(note_after_round_trip(&value), value, "pad {pad}");
        }
    }

    #[test]
    fn short_text_is_one_line() {
        assert_eq!(wrap_value("/label=\"GFP\""), vec!["/label=\"GFP\"".to_string()]);
    }

    #[test]
    fn numbers_are_bare_and_flags_have_no_value() {
        let text = "LOCUS       t                         12 bp    DNA     linear   SYN 01-JAN-2026\nFEATURES             Location/Qualifiers\n     CDS             1..12\n                     /label=\"x\"\n                     /codon_start=1\n                     /pseudo\nORIGIN\n        1 acgtacgtac gt\n//\n";
        let c = read(text).unwrap();
        let written = write(&c).unwrap();
        assert!(written.contains("/codon_start=1\n"), "{written}");
        assert!(written.contains("/pseudo\n"), "{written}");
        assert_eq!(read(&written).unwrap(), c);
    }
}
