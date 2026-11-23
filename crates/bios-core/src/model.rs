//! The construct: one molecule, its sequence and the features annotated on it.
//!
//! Coordinates are 0-based and inclusive at both ends. On a circular construct a feature whose
//! `end` is before its `start` wraps the origin.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strand {
    Forward,
    Reverse,
}

impl Strand {
    pub fn sign(self) -> i8 {
        match self {
            Strand::Forward => 1,
            Strand::Reverse => -1,
        }
    }
}

/// GenBank's `<` or `>` on one end of a location: the true end lies outside the stated base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fuzzy {
    /// `<`
    Before,
    /// `>`
    After,
}

/// How the pieces of a location with several pieces relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationOperator {
    Join,
    /// `order(...)`: the pieces are listed in order but nothing says they are joined end to end.
    Order,
}

/// One contiguous piece of a feature's location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub start_fuzzy: Option<Fuzzy>,
    pub end_fuzzy: Option<Fuzzy>,
}

impl Segment {
    /// A piece with both ends certain.
    pub const fn new(start: usize, end: usize) -> Self {
        Segment { start, end, start_fuzzy: None, end_fuzzy: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feature {
    pub name: String,
    /// The GenBank feature key, such as `CDS` or `promoter`.
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub strand: Strand,
    /// Every piece of a joined location, in file order. One entry for a simple span.
    pub segments: Vec<Segment>,
    /// Qualifiers in file order. A key that repeats keeps every value.
    pub notes: Vec<(String, Vec<String>)>,
    /// Uncertain ends of a simple span. A feature with several pieces keeps them on its segments
    /// and leaves these empty.
    pub start_fuzzy: Option<Fuzzy>,
    pub end_fuzzy: Option<Fuzzy>,
    /// `None` when the file gave no operator, which reads as a join.
    pub location_operator: Option<LocationOperator>,
    /// A location Bios cannot interpret (a remote reference, say), kept as written. Edits refuse
    /// to move a feature that has one.
    pub location_unsupported: Option<String>,
}

impl Feature {
    pub fn note(&self, key: &str) -> Option<&[String]> {
        self.notes.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice())
    }

    /// The pieces of a joined location. `None` for a simple span: a lone segment says no more than
    /// `start` and `end` do.
    pub fn locations(&self) -> Option<&[Segment]> {
        if self.segments.len() > 1 { Some(&self.segments) } else { None }
    }

    /// True when the feature runs through the origin of a circular construct.
    pub fn wraps(&self) -> bool {
        self.end < self.start
    }

    /// Number of bases the feature covers on a construct of `length` bases.
    pub fn span(&self, length: usize) -> usize {
        if self.wraps() { length - self.start + self.end + 1 } else { self.end - self.start + 1 }
    }
}

/// What the `LOCUS` line says besides the name, the length and the topology.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locus {
    /// The molecule type, such as `DNA` or `ds-DNA`. Empty when the file gave none.
    pub molecule: String,
    /// The three-letter GenBank division, such as `SYN`. Empty when the file gave none.
    pub division: String,
    /// The date as written, such as `20-JUL-2026`. Empty when the file gave none.
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Construct {
    pub name: String,
    pub circular: bool,
    /// Uppercase bases.
    pub sequence: String,
    pub features: Vec<Feature>,
    pub comments: Vec<String>,
    pub locus: Locus,
    /// The header lines between `LOCUS` and `FEATURES` other than comments (`DEFINITION`,
    /// `ACCESSION`, `SOURCE`, references and anything else), exactly as read, so that a file is
    /// written back with everything Bios does not itself understand.
    pub header: Vec<String>,
}

impl Construct {
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }
}
