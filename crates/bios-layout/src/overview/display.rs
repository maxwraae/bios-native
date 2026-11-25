//! Which of a file's features the map draws, under what name, in which colour family, and whether
//! each reads in a direction. Ported from the display half of Bios `editor/features.ts`.
//!
//! The file is never changed by any of this: these are decisions about the drawing. Colours are
//! named here as a [`FeatureRole`]; the hue of each role belongs to whoever paints.

use super::labels::Span;
use super::text_fit::is_js_whitespace;

/// The colour family a feature is drawn in. One colour means one kind of thing everywhere.
///
/// The first four are the palette: what the construct makes, where transcription starts, the rest
/// of the control layer, and the parts that carry the molecule. The last three are for features
/// that ride as `misc_feature` and are told apart only by name: gene structure that is not coding,
/// the scaffolding a construct is built with, and a bare in-frame signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeatureRole {
    Coding,
    Promoter,
    Regulatory,
    Structural,
    Sage,
    Scaffold,
    Signal,
}

/// The family of a GenBank feature key (`colorForType`). Anything unlisted is structural.
pub fn role_for_type(kind: &str) -> FeatureRole {
    match kind.to_lowercase().as_str() {
        "cds" | "gene" | "ncrna" | "mrna" => FeatureRole::Coding,
        "promoter" | "rbs" => FeatureRole::Promoter,
        "enhancer" | "terminator" | "polya_signal" | "regulatory" => FeatureRole::Regulatory,
        _ => FeatureRole::Structural,
    }
}

/// A character a JavaScript `\b` counts as part of a word.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `\bword\b`: `word` somewhere in `text` with no word character on either side of it.
fn has_word(text: &str, word: &str) -> bool {
    text.char_indices().any(|(at, _)| {
        let Some(rest) = text.get(at..).and_then(|tail| tail.strip_prefix(word)) else { return false };
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        !before.is_some_and(is_word) && !rest.chars().next().is_some_and(is_word)
    })
}

fn has_any_word(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| has_word(text, word))
}

const SCAFFOLD_WORDS: [&str; 10] = ["eblock", "e-block", "gibson", "homolog", "homology", "homologous", "scaffold", "overlap", "anneal", "primer"];

/// The build-layer classifier (`colorByName`): reads a feature's name to tell construction
/// scaffolding from gene anatomy. Scaffolding is matched first, because "eblock for new intron"
/// contains "intron" yet is scaffolding, not a gene part.
fn role_by_name(name: &str) -> Option<FeatureRole> {
    let n = name.to_lowercase();
    if n.is_empty() {
        return None;
    }
    if has_any_word(&n, &SCAFFOLD_WORDS) {
        return Some(FeatureRole::Scaffold);
    }
    if has_word(&n, "exon") {
        return Some(FeatureRole::Coding);
    }
    if has_word(&n, "intron") {
        return Some(FeatureRole::Sage);
    }
    if has_word(&n, "stop") {
        return Some(FeatureRole::Signal);
    }
    None
}

/// The family a feature's arc, bar and label use (`colorForFeature`). The type wins first: a real
/// CDS, promoter or origin always keeps its role, and only features that would otherwise be
/// structural are classified by name, so a stray name can never repaint genuine biology.
pub fn role_for_feature(kind: &str, name: &str) -> FeatureRole {
    let by_type = role_for_type(kind);
    if by_type == FeatureRole::Structural
        && let Some(by_name) = role_by_name(name)
    {
        return by_name;
    }
    by_type
}

const DIRECTIONAL_TYPES: [&str; 21] = [
    "cds",
    "gene",
    "mrna",
    "ncrna",
    "trna",
    "rrna",
    "precursor_rna",
    "promoter",
    "rbs",
    "terminator",
    "polya_signal",
    "polya_site",
    "primer_bind",
    "exon",
    "intron",
    "sig_peptide",
    "mat_peptide",
    "transit_peptide",
    "misc_recomb",
    "rep_origin",
    "ltr",
];

// Reached only for the untyped/misc bucket, where the name is the only evidence there is. The
// listed parts are ones whose orientation is unambiguous. It errs toward NOT drawing a mark,
// because a missing mark is a smaller lie than a confidently wrong one.
const DIRECTIONAL_NAMES: [&str; 23] = [
    "primer",
    "exon",
    "intron",
    "stop",
    "start",
    "kozak",
    "polya",
    "poly a",
    "polyadenylation",
    "terminator",
    "promoter",
    "orf",
    "cds",
    "tag",
    "linker",
    "signal",
    "cag",
    "wpre",
    "itr",
    "ires",
    "t2a",
    "e2a",
    "p2a",
];

/// Does this annotation actually READ in a direction?
///
/// GenBank gives every feature a strand because the format has nowhere to say "not applicable", so
/// the strand is always there and almost always meaningless. Restricting the direction mark to
/// things that genuinely have a 5'→3' sense makes its ABSENCE informative too: no mark means
/// orientation does not apply here.
///
///   - Translated, transcribed or synthesised things read one way: CDS, promoter, terminator,
///     poly(A), primer, exon. These get a mark.
///   - Enhancers do not: working independently of orientation is close to their definition.
///   - `protein_bind` sites are usually palindromic, because the proteins that bind them are dimers.
///   - Recombination sites are directional: the orientation is the whole mechanism.
///   - `misc_feature` is the bookkeeping bucket (cassettes, fillers, eblocks). Named exceptions
///     apply, because a stop codon or a primer often arrives typed as `misc_feature`.
pub fn is_directional(kind: &str, name: &str) -> bool {
    let kind = kind.to_lowercase();
    if DIRECTIONAL_TYPES.contains(&kind.as_str()) {
        return true;
    }
    // Anything explicitly non-directional stays that way whatever it is called.
    if kind == "enhancer" || kind == "protein_bind" || kind == "source" {
        return false;
    }
    has_any_word(&name.to_ascii_lowercase(), &DIRECTIONAL_NAMES)
}

/// Feature names in `.gb` files are machine tokens (`CMV_enhancer`), underscores standing in for
/// spaces. Shown verbatim they read as filenames, not labels, so runs of underscores and white
/// space become one space. Display only.
pub fn humanize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut gap = false;
    for c in name.chars() {
        if c == '_' || is_js_whitespace(c) {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push(' ');
        }
        gap = false;
        out.push(c);
    }
    out
}

/// Whole-molecule metadata, never drawn.
pub fn is_source(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("source")
}

/// A primer lives in the file as a `primer_bind` feature, but it is not drawn in the feature lanes:
/// primers are the construct's own list of oligos and have a track of their own — the primer lanes
/// in the sequence rows, and ([`display_primers`]) a lane off the backbone on the map.
pub fn is_primer_bind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("primer_bind")
}

/// Splice-map scan output: dozens of tiny `..._SM` motif hits that are search results, not
/// features a scientist placed. Hidden unless asked for. The suffix is the pipeline's own marker.
pub fn is_splice_scan(name: &str) -> bool {
    let trimmed = name.trim_matches(is_js_whitespace);
    let Some(at) = trimmed.len().checked_sub(3) else { return false };
    trimmed.get(at..).is_some_and(|tail| tail.eq_ignore_ascii_case("_sm"))
}

/// A feature as the file states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFeature<'a> {
    pub name: &'a str,
    /// The GenBank feature key, such as `CDS`.
    pub kind: &'a str,
    /// 0-based and inclusive. `end < start` wraps through the origin.
    pub start: i64,
    pub end: i64,
    pub forward: bool,
    /// The ordered segments of a joined location, when the file wrote one.
    pub locations: Option<Vec<(i64, i64)>>,
    pub location_unsupported: bool,
}

/// A feature as the map draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFeature {
    pub start: i64,
    pub end: i64,
    /// The name as shown ([`humanize_name`]).
    pub name: String,
    pub kind: String,
    pub forward: bool,
    pub locations: Option<Vec<(i64, i64)>>,
    pub location_unsupported: bool,
    pub role: FeatureRole,
    /// Whether it carries a direction mark ([`is_directional`]).
    pub directional: bool,
    /// Index of the file's feature this draws. Fusing rewrites the span, so this is the way back.
    pub source: usize,
}

impl Span for MapFeature {
    fn start(&self) -> i64 {
        self.start
    }
    fn end(&self) -> i64 {
        self.end
    }
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn kind(&self) -> Option<&str> {
        Some(&self.kind)
    }
    fn forward(&self) -> Option<bool> {
        Some(self.forward)
    }
    fn locations(&self) -> Option<Vec<(i64, i64)>> {
        self.locations.clone()
    }
    fn location_unsupported(&self) -> bool {
        self.location_unsupported
    }
    fn with_span(&self, start: i64, end: i64) -> Self {
        Self { start, end, ..self.clone() }
    }
}

/// What the map is handed (`toDisplaySequence`): the shown features only, each with its display
/// name and colour family. `source` is always left out, primers are left to their own lane
/// ([`display_primers`]), and splice-scan hits are shown only when `show_splice_scan` asks for them.
///
/// The family is read from the file's own name and the direction from the shown one, as the web
/// app does.
pub fn display_features(file: &[FileFeature<'_>], show_splice_scan: bool) -> Vec<MapFeature> {
    file.iter()
        .enumerate()
        .filter(|(_, f)| !is_source(f.kind) && (show_splice_scan || !is_splice_scan(f.name)) && !is_primer_bind(f.kind))
        .map(|(source, f)| shown(source, f))
        .collect()
}

/// What the map draws in its own primer lane: the file's `primer_bind` features, and nothing else.
///
/// A DEPARTURE from the Bios web app, which draws no oligos on its map (PARITY.md, Max 2026-11-25).
/// A primer is read like any other annotation here — the same display name, the same direction —
/// because it is a stretch of the molecule; what differs is where it is drawn and how lightly.
pub fn display_primers(file: &[FileFeature<'_>]) -> Vec<MapFeature> {
    file.iter().enumerate().filter(|(_, f)| is_primer_bind(f.kind)).map(|(source, f)| shown(source, f)).collect()
}

/// One file feature as the map draws it.
fn shown(source: usize, f: &FileFeature<'_>) -> MapFeature {
    let name = humanize_name(f.name);
    MapFeature {
        start: f.start,
        end: f.end,
        directional: is_directional(f.kind, &name),
        name,
        kind: f.kind.to_owned(),
        forward: f.forward,
        locations: f.locations.clone(),
        location_unsupported: f.location_unsupported,
        role: role_for_feature(f.kind, f.name),
        source,
    }
}
