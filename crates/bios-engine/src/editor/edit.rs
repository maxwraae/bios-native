//! What typing, pasting and deleting do to the bases, the features and the caret. Ported from the web app's `rows/edit.ts`.
//!
//! A key press in a sequence editor is not a string splice. Insert ten bases at position 400 and
//! every feature after 400 has to move with them; delete a range and anything on it has to
//! shrink, split or go. So the bases and the features are changed by `bios_core::edits`, the same
//! functions an agent's edit goes through, and nothing here moves a feature itself.
//!
//! Positions here are 0-based. A range of bases includes both its ends, and an end before its
//! start runs through the origin of a circular construct. A caret is a gap between bases, from
//! `0` (before the first base) to the length (after the last).

use bios_core::Construct;
use bios_core::edits::{EditError, EditResult, apply_delete, apply_insert, apply_replace};

use super::state::{Selection, SelectionEnd};

/// A run of bases, 0-based and inclusive at both ends. An `end` before the `start` runs through
/// the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseRange {
    pub start: usize,
    pub end: usize,
}

impl BaseRange {
    pub const fn new(start: usize, end: usize) -> Self {
        BaseRange { start, end }
    }

    /// True when the range runs through the origin.
    pub const fn wraps(self) -> bool {
        self.end < self.start
    }
}

impl From<Selection> for BaseRange {
    fn from(selection: Selection) -> Self {
        BaseRange { start: selection.start, end: selection.end }
    }
}

/// Where an edit happens: at a caret, or over a range of bases that it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Caret(usize),
    Range(BaseRange),
}

impl Where {
    /// The selection if there is one, otherwise the caret, otherwise nowhere. A construct that
    /// has just been opened has neither, and a key pressed then must edit nothing.
    pub fn of(selection: Option<Selection>, caret: Option<usize>) -> Option<Where> {
        selection.map(|s| Where::Range(s.into())).or(caret.map(Where::Caret))
    }
}

/// A key that moves the caret, named as the web platform names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKey {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
}

impl MoveKey {
    /// The key called `key` (`"ArrowLeft"`, `"Home"`, ...). `None` for a key that is not a
    /// movement, which is left for someone else to handle.
    pub fn named(key: &str) -> Option<MoveKey> {
        match key {
            "ArrowLeft" => Some(MoveKey::ArrowLeft),
            "ArrowRight" => Some(MoveKey::ArrowRight),
            "ArrowUp" => Some(MoveKey::ArrowUp),
            "ArrowDown" => Some(MoveKey::ArrowDown),
            "Home" => Some(MoveKey::Home),
            "End" => Some(MoveKey::End),
            _ => None,
        }
    }
}

/// Which way a deleting key points: backspace takes the base behind the caret, delete the one in
/// front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Back,
    Forward,
}

/// What extending a selection leaves: a selection, or a caret where its two ends met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extended {
    Selection(Selection),
    Caret(usize),
}

fn is_iupac(c: char) -> bool {
    matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'U' | 'N' | 'R' | 'Y' | 'K' | 'M' | 'S' | 'W' | 'B' | 'D' | 'H' | 'V')
}

/// Anything a person can type into a DNA sequence, ambiguity codes included. `key` is the key's
/// name: a single letter for a base, and anything longer (`"Enter"`) is not one.
pub fn is_base_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if is_iupac(c))
}

/// Refuses a construct holding a feature whose location Bios could not interpret (a remote
/// reference, say). Such a feature cannot be moved with the bases, so the bases are not edited.
pub fn assert_editable_locations(data: &Construct) -> Result<(), EditError> {
    for f in &data.features {
        if let Some(location) = f.location_unsupported.as_deref().filter(|text| !text.is_empty()) {
            return Err(EditError::UnsupportedLocation { name: f.name.clone(), location: location.to_string() });
        }
    }
    Ok(())
}

/// A 0-based position as the 1-based one `bios_core::edits` takes. `None` if it cannot be held.
fn one_based(position: usize) -> Option<i64> {
    i64::try_from(position).ok()?.checked_add(1)
}

/// The edit `insert_bases` makes, with what it did to each feature.
pub(super) fn replaced(data: &Construct, at: Where, bases: &str) -> Result<EditResult, EditError> {
    assert_editable_locations(data)?;
    match at {
        Where::Caret(gap) => {
            let at = one_based(gap).ok_or(EditError::InsertPosition)?;
            apply_insert(&data.sequence, &data.features, at, bases, data.circular)
        }
        Where::Range(range) => {
            let (from, to) = one_based(range.start).zip(one_based(range.end)).ok_or(EditError::Range)?;
            apply_replace(&data.sequence, &data.features, from, to, bases, data.circular)
        }
    }
}

/// The edit `delete_range` makes, with what it did to each feature.
pub(super) fn removed(data: &Construct, range: BaseRange) -> Result<EditResult, EditError> {
    assert_editable_locations(data)?;
    let (from, to) = one_based(range.start).zip(one_based(range.end)).ok_or(EditError::Range)?;
    apply_delete(&data.sequence, &data.features, from, to, data.circular)
}

/// The construct with the edited bases and features. Everything else it carries is kept.
pub(super) fn edited(data: &Construct, result: EditResult) -> Construct {
    let mut next = data.clone();
    next.sequence = result.sequence;
    next.features = result.features;
    next
}

/// Inserts bases at a caret, or replaces a selected range with them. Features move, grow, shrink
/// or go with the bases; a replacement of the same length keeps every feature where it is.
///
/// Nowhere gets nothing: with neither a caret nor a selection the construct comes back unchanged.
/// A range that runs through the origin is replaced at position 0, so the new bases begin the
/// sequence.
///
/// The inserted bases are stored in upper case, as `bios_core::edits` stores them. The web app
/// keeps the case they were typed in.
pub fn insert_bases(data: &Construct, at: Option<Where>, bases: &str) -> Result<Construct, EditError> {
    let Some(at) = at else {
        return Ok(data.clone());
    };
    Ok(edited(data, replaced(data, at, bases)?))
}

/// Where the caret lands after `n` bases went in at `at`: after them.
///
/// A selection that runs through the origin is replaced at position 0, so the caret goes there
/// too. `start + n` would be past the end of the construct, and the next base typed would land
/// at the wrong end of it.
pub fn caret_after_insert(at: Where, n: usize) -> usize {
    match at {
        Where::Caret(gap) => gap.saturating_add(n),
        Where::Range(range) if range.wraps() => n,
        Where::Range(range) => range.start.saturating_add(n),
    }
}

/// The same for a removed range: the gap where the bases were, or 0 for a range that ran through
/// the origin.
pub fn caret_after_delete(range: BaseRange) -> usize {
    if range.wraps() { 0 } else { range.start }
}

/// Removes a range of bases. A range that runs backwards is refused on a linear construct, which
/// has no origin for it to run through.
pub fn delete_range(data: &Construct, range: BaseRange) -> Result<Construct, EditError> {
    Ok(edited(data, removed(data, range)?))
}

/// Where the moving edge of a selection is, as a gap: the place an arrow moves from.
///
/// With no selection it is the caret. With one it is whichever end the gesture finished on: the
/// gap after the last base when that was the end, the gap before the first when it was the start.
/// Measuring every move from the start instead would let shift and an arrow grow a selection to
/// one base and never past it.
pub fn cursor_of(selection: Option<Selection>, caret: Option<usize>) -> usize {
    match selection {
        None => caret.unwrap_or(0),
        Some(s) if s.active_end == SelectionEnd::Start => s.start,
        Some(s) => s.end.saturating_add(1),
    }
}

/// Where a plain left or right arrow puts the caret when there is a selection: at that edge of
/// it, without moving. That is what every text field does, and it is what makes "select, arrow
/// right, paste" put the paste after the selection and not one base into it. `None` for any other
/// key, which moves from the active end as usual.
pub fn collapse_on(selection: Selection, key: MoveKey) -> Option<usize> {
    match key {
        MoveKey::ArrowLeft => Some(selection.start),
        MoveKey::ArrowRight => Some(selection.end.saturating_add(1)),
        _ => None,
    }
}

/// Extends a selection to `to`, the gap its moving edge has just gone to, keeping the other end
/// where it was. Collapses to a caret when the two ends meet, and changes which end is active
/// when the moving edge crosses the other, as shift and an arrow do in a text field.
///
/// `None` when there is neither a selection nor a caret to extend from.
pub fn extend_selection(selection: Option<Selection>, caret: Option<usize>, to: usize) -> Option<Extended> {
    let anchor = match selection {
        Some(s) if s.active_end == SelectionEnd::Start => s.end.saturating_add(1),
        Some(s) => s.start,
        None => caret?,
    };
    let (lo, hi) = (anchor.min(to), anchor.max(to));
    if hi <= lo {
        return Some(Extended::Caret(lo));
    }
    let active_end = if to >= anchor { SelectionEnd::End } else { SelectionEnd::Start };
    Some(Extended::Selection(Selection { start: lo, end: hi - 1, active_end }))
}

/// What a backspace or a delete should remove.
///
/// With a selection, the selection. With a caret, the single base on the side the key points at.
/// `None` when there is nothing to remove: backspace at position 0 and delete at the end do
/// nothing. They do not go round the origin and eat the far end of a circular construct.
pub fn range_to_delete(selection: Option<Selection>, caret: Option<usize>, length: usize, direction: Direction) -> Option<BaseRange> {
    if let Some(selection) = selection {
        return Some(selection.into());
    }
    let caret = caret?;
    match direction {
        Direction::Back => caret.checked_sub(1).map(|base| BaseRange::new(base, base)),
        Direction::Forward => (caret < length).then_some(BaseRange::new(caret, caret)),
    }
}

/// Where the caret goes after a key that moves it.
///
/// It stops at the ends: the rows read straight through from top to bottom, so an arrow left off
/// the first base has nowhere to go. Up and down move a whole row, which is why this needs
/// `bps_per_row`, the number the drawing used, so that the caret lands under the letter it looked
/// like it would. A row of no bases moves the caret nowhere.
pub fn caret_after_move(caret: usize, key: MoveKey, length: usize, bps_per_row: usize) -> usize {
    let row_start = caret - caret.checked_rem(bps_per_row).unwrap_or(0);
    match key {
        MoveKey::ArrowLeft => caret.saturating_sub(1).min(length),
        MoveKey::ArrowRight => caret.saturating_add(1).min(length),
        MoveKey::ArrowUp => caret.saturating_sub(bps_per_row).min(length),
        MoveKey::ArrowDown => caret.saturating_add(bps_per_row).min(length),
        MoveKey::Home => row_start,
        MoveKey::End => row_start.saturating_add(bps_per_row).min(length),
    }
}

/// `seq[from..to]`, with both ends held inside the sequence. Empty when backwards.
fn slice(seq: &str, from: usize, to: usize) -> &str {
    seq.get(from.min(seq.len())..to.min(seq.len())).unwrap_or_default()
}

/// The bases a selection covers, in order, through the origin if it runs through it.
pub fn selected_bases(seq: &str, sel: BaseRange) -> String {
    let past_end = sel.end.saturating_add(1);
    if sel.wraps() { [slice(seq, sel.start, seq.len()), slice(seq, 0, past_end)].concat() } else { slice(seq, sel.start, past_end).to_string() }
}

/// What the web app's `\s` matches: Unicode white space and the byte-order mark, but not U+0085.
pub(super) fn is_js_space(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}

/// The characters that end a line in the web app's patterns.
fn is_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn at_line_start(text: &[char], i: usize) -> bool {
    i.checked_sub(1).and_then(|before| text.get(before)).is_none_or(|&c| is_line_end(c))
}

/// The end of the line `from` is on: the next line end, or the end of the text.
fn line_end(text: &[char], from: usize) -> usize {
    run(text, from, |c| !is_line_end(c))
}

/// Where the run of characters passing `test` that starts at `from` stops.
fn run(text: &[char], from: usize, test: impl Fn(char) -> bool) -> usize {
    text.iter().skip(from).position(|&c| !test(c)).map_or(text.len().max(from), |n| from + n)
}

fn starts_with(text: &[char], at: usize, word: &str) -> bool {
    let mut rest = text.iter().skip(at);
    word.chars().all(|w| rest.next() == Some(&w))
}

/// Replaces every match that starts a line with one space. `matcher` is given the start of a
/// line and returns where its match ends.
fn blank(text: &[char], matcher: impl Fn(&[char], usize) -> Option<usize>) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let (mut i, mut tried_to) = (0, 0);
    while let Some(&c) = text.get(i) {
        if i >= tried_to && at_line_start(text, i) {
            if let Some(end) = matcher(text, i).filter(|&end| end > i) {
                out.push(' ');
                i = end;
                continue;
            }
            // A line that starts inside this run of white space would fail the same way. Without
            // this a paste of blank lines costs the square of its length.
            tried_to = run(text, i, is_js_space);
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The header lines of a GenBank record, which hold no bases.
const GENBANK_KEYWORDS: [&str; 16] = [
    "LOCUS",
    "DEFINITION",
    "ACCESSION",
    "VERSION",
    "KEYWORDS",
    "SOURCE",
    "ORGANISM",
    "REFERENCE",
    "AUTHORS",
    "TITLE",
    "JOURNAL",
    "PUBMED",
    "COMMENT",
    "FEATURES",
    "BASE COUNT",
    "CONTIG",
];

/// `word` at `at`, ending there: the next character is not a letter, a digit or an underscore.
fn whole_word(text: &[char], at: usize, word: &str) -> Option<usize> {
    let end = at + word.chars().count();
    (starts_with(text, at, word) && !text.get(end).is_some_and(|&c| is_word(c))).then_some(end)
}

/// A header line: one of the keywords, then the rest of its line.
fn keyword_line(text: &[char], at: usize) -> Option<usize> {
    GENBANK_KEYWORDS.iter().find_map(|keyword| whole_word(text, at, keyword)).map(|end| line_end(text, end))
}

/// A line of the feature table: space, a feature key, space, then a location such as `5..15` or
/// `complement(join(1..10,21..30))`, and the rest of that line.
fn location_line(text: &[char], at: usize) -> Option<usize> {
    let key = run(text, at, is_js_space);
    let gap = run(text, key, |c| !is_js_space(c));
    let mut location = run(text, gap, is_js_space);
    if key == at || gap == key || location == gap {
        return None;
    }
    for opening in ["complement(", "join(", "<"] {
        if starts_with(text, location, opening) {
            location += opening.chars().count();
        }
    }
    let dots = run(text, location, |c| c.is_ascii_digit());
    (dots > location && starts_with(text, dots, "..")).then(|| line_end(text, dots + 2))
}

/// A qualifier of the feature table: space, then `/name` and the rest of its line.
fn qualifier_line(text: &[char], at: usize) -> Option<usize> {
    let slash = run(text, at, is_js_space);
    let name = run(text, slash + 1, is_word);
    (slash > at && text.get(slash) == Some(&'/') && name > slash + 1).then(|| line_end(text, name))
}

/// A FASTA header: `>` and the rest of its line.
fn fasta_header(text: &[char], at: usize) -> Option<usize> {
    (text.get(at) == Some(&'>')).then(|| line_end(text, at + 1))
}

/// What follows the `ORIGIN` line of a GenBank record, up to its closing `//`. `None` when the
/// text has no `ORIGIN` line.
fn after_origin(text: &[char]) -> Option<&[char]> {
    let origin = (0..text.len()).filter(|&i| at_line_start(text, i)).find_map(|i| whole_word(text, i, "ORIGIN"))?;
    let rest = text.get(line_end(text, origin)..)?;
    let close = (0..rest.len()).find(|&i| at_line_start(rest, i) && starts_with(rest, i, "//"));
    Some(close.and_then(|close| rest.get(..close)).unwrap_or(rest))
}

/// Pulls a usable DNA string out of whatever was on the clipboard, or refuses it.
///
/// People paste FASTA, GenBank fragments and numbered alignments, all carrying headers, digits
/// and white space, so those are stripped. But stripping alone is a trap: a page of prose is
/// mostly letters that happen to be base codes, so "a note about the plasmid" would quietly
/// become bases in somebody's construct. So what is left has to look like DNA: at least nine in
/// ten of its letters must be plain bases (A, C, G, T, U or N). Prose fails that on the vowels
/// alone. Returns an empty string when it does, and the caller inserts nothing.
pub fn clean_pasted_dna(text: &str) -> String {
    let text: Vec<char> = text.chars().collect();
    // A whole GenBank record: the bases are what follows ORIGIN, and nothing above it is
    // sequence. Without this, `LOCUS`, `ORIGIN` and the feature table pass the nine-in-ten rule on
    // a long enough record, and their letters that are base codes land in the construct.
    let record = after_origin(&text).unwrap_or(&text);
    // A record pasted without its ORIGIN, or a header on its own: the keyword lines, the feature
    // table's location lines and its qualifiers are not bases either.
    let record = blank(&blank(&blank(record, keyword_line), location_line), qualifier_line);
    let without_fasta = blank(&record, fasta_header);
    let letters = without_fasta.iter().filter(|c| c.is_ascii_alphabetic()).count();
    if letters == 0 {
        return String::new();
    }
    let core = without_fasta.iter().filter(|c| matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'U' | 'N')).count();
    if (core as f64) / (letters as f64) < 0.9 {
        return String::new();
    }
    without_fasta.into_iter().filter(|&c| is_iupac(c)).collect()
}
