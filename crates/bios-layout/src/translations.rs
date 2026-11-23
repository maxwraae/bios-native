//! Amino acids under the codons they are made from. Ported from Bios `editor/translations.ts` (a
//! translation asked for from the selection's menu, its frame and the span it reads) and
//! `editor/rows/RowTranslations.tsx` (`isCoding`, `residuesForRow`).
//!
//! A codon is counted from the coding span's own 5' end, so a CDS starting at base 137 translates
//! from 137, not from the row edge. On the reverse strand it reads from the span's END backwards:
//! the two directions are worked out separately rather than mirrored, because mirroring puts the
//! frame one or two bases out whenever the length is not a multiple of three.

use crate::rows::Row;

/// A translation someone asked for, floating over the bases like a binder: a view of the bases held
/// against the open construct, not something the file carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Translation {
    /// The bases it reads, 0-based inclusive.
    pub start: usize,
    pub end: usize,
    /// Reads the top strand left to right, else the bottom strand right to left.
    pub forward: bool,
    /// 0, 1 or 2: how many bases in from the reading end the first codon starts.
    pub frame: u8,
}

impl Translation {
    /// `+2` / `−3`: the frame the way a biologist says it (`frameLabel`).
    pub fn frame_label(&self) -> String {
        format!("{}{}", if self.forward { "+" } else { "−" }, self.frame.min(2) + 1)
    }

    /// The span it reads once the frame is applied (`readingSpan`): a forward frame steps in from
    /// the left, a reverse one from the right. `None` when the frame steps past the end.
    pub fn reading_span(&self) -> Option<(usize, usize)> {
        let frame = usize::from(self.frame.min(2));
        let (start, end) = if self.forward { (self.start + frame, self.end) } else { (self.start, self.end.checked_sub(frame)?) };
        (start <= end).then_some((start, end))
    }
}

/// Does a feature of this GenBank key code for something to translate (`isCoding`)?
pub fn is_coding(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("cds") || kind.eq_ignore_ascii_case("gene")
}

/// One residue and where it sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Residue {
    pub aa: char,
    /// The centre of its codon, in the row's pixels.
    pub x: f64,
    /// The first residue of the span.
    pub start: bool,
    pub stop: bool,
}

/// A span to translate: first and last base, 0-based inclusive, in either order, and its strand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coding {
    pub start: usize,
    pub end: usize,
    pub forward: bool,
}

/// The bases a coding span reads, top strand left to right, when there are three or more. The
/// caller translates them, reverse-complemented for a reverse span, and hands the protein to
/// [`codons`].
pub fn span_bases(c: Coding, seq: &str) -> Option<&str> {
    let (from, to) = (c.start.min(c.end), c.start.max(c.end));
    seq.get(from..=to).filter(|bases| bases.len() >= 3)
}

/// Each residue of `protein`, the protein `c` reads, with the base its codon starts at.
pub fn codons(c: Coding, protein: &str) -> Vec<(usize, char)> {
    let (from, to) = (c.start.min(c.end), c.start.max(c.end));
    protein
        .chars()
        .enumerate()
        .filter_map(|(i, aa)| {
            let codon_start = if c.forward { from.checked_add(i * 3)? } else { to.checked_sub(i * 3 + 2)? };
            Some((codon_start, aa))
        })
        .collect()
}

/// The residues of one coding span that fall inside one row (`residuesForRow`): each centred over
/// the middle base of its codon. `codons` is what [`codons`] gave for the span.
pub fn residues_for_row(codons: &[(usize, char)], row: Row, char_width: f64) -> Vec<Residue> {
    codons
        .iter()
        .enumerate()
        .filter_map(|(i, (codon_start, aa))| {
            let centre = i64::try_from(*codon_start).ok()? + 1;
            (centre >= row.start && centre <= row.end).then(|| Residue {
                aa: *aa,
                x: (centre - row.start) as f64 * char_width + char_width / 2.0,
                start: i == 0,
                stop: *aa == '*',
            })
        })
        .collect()
}

/// Every residue of one coding span with the row it falls in, found by arithmetic rather than by
/// asking every row: the rows hold `bps_per_row` bases each from 0. Same answers as
/// [`residues_for_row`] row by row.
pub fn residues_in_rows(codons: &[(usize, char)], rows: &[Row], bps_per_row: i64, char_width: f64) -> Vec<(usize, Residue)> {
    if bps_per_row <= 0 {
        return Vec::new();
    }
    codons
        .iter()
        .enumerate()
        .filter_map(|(i, (codon_start, aa))| {
            let centre = i64::try_from(*codon_start).ok()? + 1;
            let index = usize::try_from(centre / bps_per_row).ok()?;
            let row = rows.get(index).filter(|r| centre >= r.start && centre <= r.end)?;
            let residue = Residue { aa: *aa, x: (centre - row.start) as f64 * char_width + char_width / 2.0, start: i == 0, stop: *aa == '*' };
            Some((index, residue))
        })
        .collect()
}
