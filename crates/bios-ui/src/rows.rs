//! The sequence rows, drawn as the Bios web app draws them (`editor/rows/SequenceRows.tsx`,
//! `RowFeatures.tsx`, `RowCutMarks.tsx` and their rules in `styles.css`): both strands welded by a
//! rung per base pair, a ruled scale, each feature and primer stacked on the strand it reads from,
//! and the restriction cuts with their names.
//!
//! Every position comes from `bios_layout`; nothing here computes its own x.

use std::collections::HashMap;

use bios_core::acts::{Graph, Target};
use bios_core::binders::{BinderSite, annealed_bases, site_label};
use bios_core::duplex::AlignRun;
use bios_core::enzymes::{self, COMMON_ENZYMES, CutSite, OverhangKind};
use bios_core::find_align::{AlignHit, DRAWN_LIMIT, hit_runs, probe_name, template_aligned_bases};
use bios_core::parts::is_part;
use bios_core::{Construct, Feature, Strand, seq};
use bios_layout::cuts::{self, Cut, RowCut};
use bios_layout::interactions::{self as edge_layout, Anchor, EdgePiece, EdgeSpan, RowEdge};
use bios_layout::overview::viewport::Viewport;
use bios_layout::row_features::{self as annotations, CellKind, End, Kind, Piece, RowFeature, RowPieces, Seg};
use bios_layout::rows::{self as layout, Lanes, Metrics, Row, RowPlan, StrandLanes, Tracks, track};
use bios_layout::strip;
use bios_layout::translations::{self as aa, Coding, Residue, Translation};
use egui::{Align, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::theme::{self, Weight};
use crate::view_tab::{BaseStyle, Layers};

/// How far a tenth's rule pokes past each strand into the lane air.
const REACH: f64 = 8.0;

/// How strong each mark is: the ink's opacity (`styles.css`, `.seq-row-band`, `.seq-rule`, `.seq-rung`).
const ROW_BAND: f32 = 0.035;
const RULE_FIFTH: f32 = 0.11;
const RULE_TENTH: f32 = 0.165;
const RULE_FIFTIETH: f32 = 0.352;
const RUNG: f32 = 0.55;
/// The amino acids' ink (`.seq-aa { fill: #0d5334 }`): a calm extension of the CDS, not a rainbow.
const AMINO_INK: Color32 = theme::hex(0x0d5334);

/// An oligo drawn in the primer lane that is not one of the construct's features: a held binder's
/// site, or one of Find's alignments. Neither is in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oligo {
    /// Which of [`Options::binders`] this is.
    Binder(usize),
    /// Which of [`Options::hits`] this is.
    Probe(usize),
    /// Which of [`Options::found`] this is.
    Found(usize),
}

/// One place an Exact Find landed: its first and last base, 0-based (an end before the start
/// runs through the origin), and its strand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub forward: bool,
}

/// A feature or a primer as the rows draw it.
struct Shown {
    /// Which of the construct's features this is; `usize::MAX` for an [`Oligo`].
    index: usize,
    oligo: Option<Oligo>,
    /// The name as displayed: underscores read as spaces.
    name: String,
    color: Color32,
    start: i64,
    end: i64,
    forward: bool,
    directional: bool,
    locations: Option<Vec<(i64, i64)>>,
    /// A primer's own bases, 5'→3'.
    bases: Option<String>,
    /// An aligned oligo's runs, which place its tail and inserts.
    runs: Option<Vec<AlignRun>>,
    /// The colours it is drawn in, when it rides the primer lane.
    ribbon: theme::RibbonInk,
}

impl Shown {
    fn geometry(&self) -> RowFeature<'_> {
        RowFeature {
            start: self.start,
            end: self.end,
            forward: self.forward,
            directional: self.directional,
            name: &self.name,
            locations: self.locations.as_deref(),
            bases: self.bases.as_deref(),
            runs: self.runs.as_deref(),
        }
    }
}

/// What one row holds besides its bases. Index 0 of a pair is the forward strand's stack, above
/// the duplex, and index 1 the reverse strand's, below it.
#[derive(Default)]
struct RowContent {
    features: [RowPieces; 2],
    primers: [RowPieces; 2],
    parts: [RowPieces; 2],
    /// Open reading frames, on the strand each reads (`RowFeatures kind="orf"`).
    orfs: [RowPieces; 2],
    /// Amino acids under the duplex, whichever strand they read off (`RowTranslations`).
    residues: Vec<Residue>,
    cuts: Vec<RowCut>,
    /// What acts on what, in this row, and the lanes it needs.
    inter: Vec<EdgePiece>,
    inter_lanes: u32,
    lanes: Lanes,
}

/// The rows of one construct at one width. Rebuilt only when either changes.
pub struct Plan {
    width: f32,
    revision: u64,
    /// Whether the enzymes' names are shown, and so whether their lanes take room.
    cut_names: bool,
    /// The width a base was asked for: the rows' zoom.
    asked: f64,
    /// Whether the Interactions layer is on.
    interactions: bool,
    /// What acts on what, by the construct's feature indices. Empty while the layer is off.
    graph: Graph,
    /// Whether the primer lane is shown (the Binders layer), and which oligos it was built with.
    primers_shown: bool,
    oligo_key: u64,
    metrics: Metrics,
    tracks: Tracks,
    rows: Vec<Row>,
    plan: RowPlan,
    reverse: Vec<u8>,
    features: Vec<Shown>,
    primers: Vec<Shown>,
    /// Parts (`/pragma="Teselagen_Part"`), in their own lane; empty while the layer is off.
    parts: Vec<Shown>,
    /// Whether the Parts layer is on.
    show_parts: bool,
    orfs: Vec<Shown>,
    /// What else the rows were laid out with: the floating translations and the two layers that
    /// add tracks of their own.
    extras: Extras,
    /// The cuts the rows show, by `RowCut::cut`. They depend on the bases alone, so a new width
    /// keeps them.
    sites: Vec<CutSite>,
    content: Vec<RowContent>,
    /// The strip's window, when this is the strip's one row rather than the wrapped document.
    window: Option<StripKey>,
}

/// The tracks a row gains from the layers and from what was asked for: the amino acids under every
/// coding feature, the open reading frames, and the translations floating over the bases.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extras {
    pub amino_acids: bool,
    pub orfs: bool,
    /// The splice-map hits are shown.
    pub splice_scan: bool,
    pub translations: Vec<Translation>,
}

/// The shortest open reading frame drawn, in amino acids (`findOrfs`' default).
const ORF_MIN_AA: usize = 30;

/// The open reading frames of `c` as the ORF track draws them: derived from the bases, so quieter
/// than a feature someone annotated, and not a feature of the file (`index` is out of range).
fn orfs_of(c: &Construct) -> Vec<Shown> {
    seq::find_orfs(&c.sequence, ORF_MIN_AA)
        .into_iter()
        .map(|o| Shown {
            index: usize::MAX,
            oligo: None,
            runs: None,
            name: format!("ORF {} aa", o.aa_length),
            color: theme::row_feature_color("CDS", "ORF"),
            start: o.start as i64 - 1,
            end: o.end as i64 - 1,
            forward: o.strand == Strand::Forward,
            directional: true,
            locations: None,
            bases: None,
            ribbon: theme::PRIMER_RIBBON,
        })
        .collect()
}

/// Every residue the rows print, by row: each coding feature's protein when the Amino acids layer
/// is on, and each translation asked for whatever the layer says, because asking is the switch.
/// The strip's one row is asked directly, since it starts on any base.
fn residues_of(c: &Construct, extras: &Extras, rows: &[Row], metrics: &Metrics, strip: bool) -> Vec<Vec<Residue>> {
    let mut spans: Vec<Coding> = Vec::new();
    if extras.amino_acids {
        // A coding feature through the origin is left untranslated rather than read in two halves.
        let coding = c.features.iter().filter(|f| !hidden(f, extras.splice_scan) && aa::is_coding(&f.kind) && !f.wraps());
        spans.extend(coding.map(|f| Coding { start: f.start, end: f.end, forward: f.strand != Strand::Reverse }));
    }
    spans.extend(extras.translations.iter().filter_map(|t| t.reading_span().map(|(start, end)| Coding { start, end, forward: t.forward })));
    let mut by_row: Vec<Vec<Residue>> = vec![Vec::new(); rows.len()];
    for span in spans {
        let Some(bases) = aa::span_bases(span, &c.sequence) else { continue };
        let protein = seq::translate(&if span.forward { bases.to_string() } else { seq::revcomp(bases) });
        let codons = aa::codons(span, &protein);
        if strip {
            if let (Some(&row), Some(list)) = (rows.first(), by_row.first_mut()) {
                list.extend(aa::residues_for_row(&codons, row, metrics.char_width));
            }
            continue;
        }
        for (row, residue) in aa::residues_in_rows(&codons, rows, metrics.bps_per_row, metrics.char_width) {
            if let Some(list) = by_row.get_mut(row) {
                list.push(residue);
            }
        }
    }
    by_row
}

/// A feature name for display: underscores stand in for spaces in a file and read as filenames.
fn humanize_name(name: &str) -> String {
    name.split(|c: char| c == '_' || c.is_whitespace()).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

const DIRECTIONAL_KINDS: [&str; 21] = [
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

/// Whether orientation means anything for a feature. Things that are translated, transcribed or
/// synthesised read one way; an enhancer or a binding site does not, and no mark is the honest way
/// to say so. A bare `misc_feature` has only its name to go by.
fn is_directional(kind: &str, name: &str) -> bool {
    let kind = kind.to_lowercase();
    if DIRECTIONAL_KINDS.contains(&kind.as_str()) {
        return true;
    }
    if matches!(kind.as_str(), "enhancer" | "protein_bind" | "source") {
        return false;
    }
    theme::has_word(&name.to_lowercase(), &DIRECTIONAL_NAMES)
}

/// Kept in the file and out of the rows: the `source` record, and a splice-map scan's motif hits
/// (named `..._SM`), which are search results rather than features someone placed.
#[cfg(test)]
fn is_hidden(f: &Feature) -> bool {
    hidden(f, false)
}

/// The same, with the splice-map hits shown when they are asked for (`isHidden`).
fn hidden(f: &Feature, show_splice_scan: bool) -> bool {
    f.kind.eq_ignore_ascii_case("source") || (!show_splice_scan && f.name.trim().to_lowercase().ends_with("_sm"))
}

fn is_primer(f: &Feature) -> bool {
    f.kind.eq_ignore_ascii_case("primer_bind")
}

/// The bases a primer's ribbon prints, 5'→3'. The oligo stored on the feature when it covers its
/// footprint column for column, and otherwise what the footprint itself reads.
pub fn primer_bases(f: &Feature, sequence: &str) -> Option<String> {
    if f.locations().is_some() || f.location_unsupported.is_some() || f.start_fuzzy.is_some() || f.end_fuzzy.is_some() {
        return None;
    }
    let footprint = if f.wraps() { [sequence.get(f.start..)?, sequence.get(..=f.end)?].concat() } else { sequence.get(f.start..=f.end)?.to_owned() };
    let stored = f.note("sequence").and_then(|values| values.first()).map(|text| text.split_whitespace().collect::<String>().to_uppercase());
    match stored {
        Some(oligo) if oligo.len() == footprint.len() => Some(oligo),
        _ if f.strand == Strand::Forward => Some(footprint),
        _ => Some(seq::revcomp(&footprint)),
    }
}

fn shown(index: usize, f: &Feature, sequence: &str) -> Shown {
    let name = humanize_name(&f.name);
    let primer = is_primer(f);
    Shown {
        index,
        color: theme::row_feature_color(&f.kind, &f.name),
        start: f.start as i64,
        end: f.end as i64,
        forward: f.strand == Strand::Forward,
        directional: is_directional(&f.kind, &name),
        locations: if primer { None } else { f.locations().map(|pieces| pieces.iter().map(|s| (s.start as i64, s.end as i64)).collect()) },
        bases: if primer { primer_bases(f, sequence) } else { None },
        runs: None,
        oligo: None,
        name,
        ribbon: theme::PRIMER_RIBBON,
    }
}

/// A held binder's site as the primer lane draws it: its annealed bases over its footprint, and
/// what it is beside its name (`binders.ts` `sitesToPrimers`).
fn shown_binder(index: usize, s: &BinderSite) -> Shown {
    Shown {
        index: usize::MAX,
        oligo: Some(Oligo::Binder(index)),
        name: site_label(s),
        color: theme::PRIMER_EDGE,
        start: s.site.start as i64,
        end: s.site.end as i64,
        forward: s.site.forward,
        directional: true,
        locations: None,
        bases: Some(annealed_bases(s).to_string()),
        runs: None,
        ribbon: theme::PRIMER_RIBBON,
    }
}

/// One of Find's alignments as the primer lane draws it, its runs and all (`alignment.ts`
/// `probeToOve`). Through the origin its footprint's end is past the length: taken modulo.
fn shown_probe(index: usize, h: &AlignHit, c: &Construct) -> Shown {
    let runs = hit_runs(h, c.len(), c.circular);
    let n = c.len().max(1);
    Shown {
        index: usize::MAX,
        oligo: Some(Oligo::Probe(index)),
        name: probe_name(h),
        color: theme::PRIMER_EDGE,
        start: (h.alignment.start % n) as i64,
        end: (h.alignment.end % n) as i64,
        forward: h.alignment.forward,
        directional: true,
        locations: None,
        bases: Some(template_aligned_bases(&runs)),
        runs: Some(runs),
        ribbon: theme::PRIMER_RIBBON,
    }
}

/// One place an Exact Find landed, as the primer lane draws it: its own bases 5'→3' over its
/// footprint, in the found colours. Through the origin its end is before its start.
fn shown_found(index: usize, m: &Found, c: &Construct) -> Shown {
    let top = crate::selection_readout::selected_bases(c, m.start, m.end);
    Shown {
        index: usize::MAX,
        oligo: Some(Oligo::Found(index)),
        name: "found".to_string(),
        color: theme::FOUND_RIBBON.edge,
        start: m.start as i64,
        end: m.end as i64,
        forward: m.forward,
        directional: true,
        locations: None,
        bases: Some(if m.forward { top } else { seq::revcomp(&top) }),
        runs: None,
        ribbon: theme::FOUND_RIBBON,
    }
}

/// A cut site as the row geometry takes it.
pub fn cut_of(site: &CutSite) -> Cut {
    let length = site.overhang.length as i64;
    Cut {
        top_cut: site.top_cut as i64,
        bottom_cut: site.bottom_cut as i64,
        overhang: match site.overhang.kind {
            OverhangKind::FivePrime => length,
            OverhangKind::ThreePrime => -length,
            OverhangKind::Blunt => 0,
        },
        recognition_start: site.recognition_start as i64,
        site_length: enzymes::get_enzyme(site.enzyme).map_or(6, |e| e.length as i64),
        offsets: enzymes::cut_offsets(site).map(|pair| (pair.top, pair.bottom)),
    }
}

/// The cuts of `names`, narrowed to the enzymes that cut the molecule at most `limit` times.
/// `Some(0)` draws none and `None` draws every site. The web app's scissors dial
/// (`editor/cutsites.ts`).
pub fn dial_sites<S: AsRef<str>>(sequence: &str, circular: bool, names: &[S], limit: Option<usize>) -> Vec<CutSite> {
    if limit == Some(0) {
        return Vec::new();
    }
    let all = enzymes::find_cut_sites(sequence, names, circular);
    let Some(limit) = limit else {
        return all;
    };
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in &all {
        *counts.entry(c.enzyme).or_default() += 1;
    }
    all.into_iter().filter(|c| counts.get(c.enzyme).is_some_and(|&n| n <= limit)).collect()
}

/// The cuts the rows show at rest, as the web app does: the workhorse enzymes that cut once.
pub fn default_cut_sites(c: &Construct) -> Vec<CutSite> {
    dial_sites(&c.sequence, c.circular, &COMMON_ENZYMES, Some(1))
}

/// For each row, the indices of the features that draw in it, by walking the features and the
/// rows each one covers rather than every row against every feature.
///
/// The strip's one row starts on any base, so nothing can be found by dividing: every annotation
/// is a candidate there, and [`annotations::pieces_for_row`] clips the ones that land in it.
fn bucket(list: &[Shown], strand_forward: bool, length: i64, metrics: &Metrics, row_count: usize, strip: bool) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); row_count];
    let on_strand = || list.iter().enumerate().filter(|(_, f)| f.forward == strand_forward);
    if strip {
        if let Some(row) = rows.first_mut() {
            row.extend(on_strand().map(|(index, _)| index));
        }
        return rows;
    }
    for (index, f) in on_strand() {
        for (first, last) in annotations::rows_of_feature(&f.geometry(), length, metrics.bps_per_row, row_count) {
            for row in rows.iter_mut().take(last.saturating_add(1)).skip(first) {
                if row.last() != Some(&index) {
                    row.push(index);
                }
            }
        }
    }
    rows
}

/// One track's pieces in every row, with each piece pointing back into `list`.
fn track_pieces(list: &[Shown], forward: bool, kind: Kind, c: &Construct, metrics: &Metrics, rows: &[Row], strip: bool) -> Vec<RowPieces> {
    let length = c.len() as i64;
    let buckets = bucket(list, forward, length, metrics, rows.len(), strip);
    rows.iter()
        .zip(buckets)
        .map(|(&row, indices)| {
            if indices.is_empty() {
                return RowPieces::default();
            }
            let features: Vec<RowFeature> = indices.iter().filter_map(|&i| list.get(i)).map(Shown::geometry).collect();
            let mut pieces = annotations::pieces_for_row(&features, row, metrics.char_width, length, kind, Some(&c.sequence), c.circular);
            for piece in &mut pieces.pieces {
                piece.feature = indices.get(piece.feature).copied().unwrap_or(usize::MAX);
            }
            pieces
        })
        .collect()
}

/// The staircases of every row, their names packed into lanes, and how many lanes each row needs.
/// Every cut is a candidate for the strip's one row, as every annotation is (`cutsByRow`).
fn cut_rows(sites: &[CutSite], c: &Construct, metrics: &Metrics, rows: &[Row], strip: bool) -> Vec<(Vec<RowCut>, usize)> {
    let length = c.len() as i64;
    let all: Vec<Cut> = sites.iter().map(cut_of).collect();
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
    for (index, cut) in all.iter().enumerate() {
        let touched = if strip { vec![0] } else { cuts::rows_of_cut(cut, length, c.circular, metrics.bps_per_row, rows.len()) };
        for row in touched {
            if let Some(bucket) = buckets.get_mut(row) {
                bucket.push(index);
            }
        }
    }
    rows.iter()
        .zip(buckets)
        .map(|(&row, indices)| {
            let here: Vec<Cut> = indices.iter().filter_map(|&i| all.get(i).copied()).collect();
            let mut drawn = cuts::cuts_for_row(&here, row, length, c.circular);
            for cut in &mut drawn {
                cut.cut = indices.get(cut.cut).copied().unwrap_or(usize::MAX);
            }
            let lanes = cuts::pack_names(&mut drawn, row, metrics.char_width, |cut| sites.get(cut.cut).map_or("", |site| site.enzyme));
            (drawn, lanes)
        })
        .collect()
}

fn strand_lanes(features: &RowPieces, primers: &RowPieces, parts: &RowPieces, orfs: &RowPieces) -> StrandLanes {
    StrandLanes {
        features: features.lanes as u32,
        primers: primers.lanes as u32,
        primer_lift: primers.lift,
        parts: parts.lanes as u32,
        orfs: orfs.lanes as u32,
        ..StrandLanes::default()
    }
}

impl Plan {
    /// One of the cuts the rows show, by the index [`Seen::cut`] gives.
    pub fn site(&self, index: usize) -> Option<&CutSite> {
        self.sites.get(index)
    }

    /// How many bases a row holds at this width: what the up and down arrows move by.
    pub fn bps_per_row(&self) -> usize {
        usize::try_from(self.metrics.bps_per_row).unwrap_or(0)
    }

    /// `strip` lays out the strip's one row instead of the wrapped document
    /// (`SequenceRows.tsx` `geometry`, 433-460).
    fn build(c: &Construct, width: f32, revision: u64, sites: Vec<CutSite>, options: &Options, asked: f64, strip: Option<&strip::Geometry>) -> Plan {
        let layers = &options.layers;
        let (cut_names, interactions, primers_shown) = (layers.cut_names, layers.interactions, layers.binders);
        let tracks = tracks_of(options);
        let (metrics, rows) = match strip {
            Some(g) => (g.metrics, vec![g.row]),
            None => {
                let metrics = layout::metrics_for(f64::from(width), Some(asked), None);
                (metrics, layout::rows_for(c.len() as i64, metrics.bps_per_row))
            }
        };
        let window = strip.map(StripKey::of);
        let strip = strip.is_some();
        let extras = extras_of(options);
        let visible = || c.features.iter().enumerate().filter(|(_, f)| !hidden(f, extras.splice_scan));
        let features: Vec<Shown> = visible().filter(|(_, f)| !is_primer(f) && !is_part(f)).map(|(i, f)| shown(i, f, &c.sequence)).collect();
        // The made primers, the binders held against the construct and Find's best few fits share
        // the one lane, and the Binders layer shows or hides all of it (`layerVisibility.primers`).
        let mut primers: Vec<Shown> = Vec::new();
        if primers_shown {
            primers.extend(visible().filter(|(_, f)| is_primer(f) && !is_part(f)).map(|(i, f)| shown(i, f, &c.sequence)));
            primers.extend(options.binders.iter().enumerate().map(|(i, s)| shown_binder(i, s)));
            primers.extend(options.hits.iter().take(DRAWN_LIMIT).enumerate().map(|(i, h)| shown_probe(i, h, c)));
            primers.extend(options.found.iter().take(DRAWN_LIMIT).enumerate().map(|(i, m)| shown_found(i, m, c)));
        }
        // Parts straight off the record, in a lane of their own (`SequenceRows.tsx` 498-517).
        let show_parts = layers.parts;
        let parts: Vec<Shown> =
            if show_parts { visible().filter(|(_, f)| is_part(f)).map(|(i, f)| shown(i, f, &c.sequence)).collect() } else { Vec::new() };
        let orfs: Vec<Shown> = if extras.orfs { orfs_of(c) } else { Vec::new() };

        let mut content: Vec<RowContent> = rows.iter().map(|_| RowContent::default()).collect();
        for (side, forward) in [true, false].into_iter().enumerate() {
            let feature_rows = track_pieces(&features, forward, Kind::Feature, c, &metrics, &rows, strip);
            let primer_rows = track_pieces(&primers, forward, Kind::Primer, c, &metrics, &rows, strip);
            let part_rows = track_pieces(&parts, forward, Kind::Part, c, &metrics, &rows, strip);
            let orf_rows = track_pieces(&orfs, forward, Kind::Orf, c, &metrics, &rows, strip);
            let tracks = feature_rows.into_iter().zip(primer_rows).zip(part_rows).zip(orf_rows);
            for (row, (((in_features, in_primers), in_parts), in_orfs)) in content.iter_mut().zip(tracks) {
                if let (Some(f), Some(p), Some(q), Some(o)) =
                    (row.features.get_mut(side), row.primers.get_mut(side), row.parts.get_mut(side), row.orfs.get_mut(side))
                {
                    *f = in_features;
                    *p = in_primers;
                    *q = in_parts;
                    *o = in_orfs;
                }
            }
        }
        for (row, residues) in content.iter_mut().zip(residues_of(c, &extras, &rows, &metrics, strip)) {
            row.residues = residues;
        }
        // What acts on what, only while the layer is on (`SequenceRows.tsx` 494-604).
        let graph = if interactions { Graph::of(&c.features) } else { Graph::default() };
        if !graph.edges.is_empty() {
            let names: Vec<String> = c.features.iter().map(|f| humanize_name(&f.name)).collect();
            let spans: Vec<Option<EdgeSpan>> = c
                .features
                .iter()
                .zip(&names)
                .map(|(f, name)| (!hidden(f, extras.splice_scan)).then_some(EdgeSpan { start: f.start as i64, end: f.end as i64, name }))
                .collect();
            let edges: Vec<RowEdge> = graph
                .edges
                .iter()
                .map(|e| RowEdge {
                    actor: e.actor,
                    target: match e.target {
                        Target::Feature(t) => Some(t),
                        Target::Construct => None,
                    },
                    verb: &e.verb,
                })
                .collect();
            for (row, content) in rows.iter().zip(content.iter_mut()) {
                let (pieces, lanes) = edge_layout::interactions_for_row(&edges, &spans, *row, metrics.char_width, c.len() as i64);
                content.inter = pieces;
                content.inter_lanes = lanes as u32;
            }
        }
        for (row, (cuts, name_lanes)) in content.iter_mut().zip(cut_rows(&sites, c, &metrics, &rows, strip)) {
            row.cuts = cuts;
            let inter = row.inter_lanes;
            let [above, below] = &row.features;
            let [primers_above, primers_below] = &row.primers;
            let [parts_above, parts_below] = &row.parts;
            let [orfs_above, orfs_below] = &row.orfs;
            row.lanes = Lanes {
                // None when the names are switched off, so hiding them gives the height back
                // (`lanesFor` in SequenceRows.tsx, lines 700-712).
                interactions: inter,
                cutsite_labels: if cut_names { name_lanes as u32 } else { 0 },
                above: strand_lanes(above, primers_above, parts_above, orfs_above),
                // Residues stay under the duplex whichever strand they read off.
                below: StrandLanes {
                    translations: u32::from(!row.residues.is_empty()),
                    ..strand_lanes(below, primers_below, parts_below, orfs_below)
                },
            };
        }
        let plan = layout::plan_rows(&rows, |row| content.get(row.index).map_or_else(Lanes::default, |r| r.lanes), tracks);
        let reverse = c.sequence.bytes().map(seq::complement).collect();
        Plan {
            width,
            revision,
            cut_names,
            asked,
            interactions,
            show_parts,
            primers_shown,
            oligo_key: options.oligo_key,
            metrics,
            tracks,
            rows,
            plan,
            reverse,
            features,
            primers,
            parts,
            orfs,
            extras,
            sites,
            content,
            graph,
            window,
        }
    }
}

/// What the strip's row was laid out for: its first base, how many bases it holds and how wide a
/// base is. A slide inside one base changes none of them, so it draws without laying out again.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StripKey {
    start: i64,
    bases: i64,
    char_width: f64,
}

impl StripKey {
    fn of(g: &strip::Geometry) -> StripKey {
        StripKey { start: g.row.start, bases: g.metrics.bps_per_row, char_width: g.metrics.char_width }
    }
}

/// Where the person is in the sequence, for the rows to draw. A caret and a selection are never
/// both set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cursor {
    /// The gap the caret stands in: 0 is before the first base, the length is after the last.
    pub caret: Option<usize>,
    /// The first and last selected base, 0-based. An end before the start runs through the origin.
    pub selection: Option<(usize, usize)>,
    /// When the caret last moved, in the window's own seconds, so that it shows at once after a
    /// move and blinks from there. `None` holds it steady, for a picture.
    pub moved: Option<f64>,
    /// Scroll so that this base's row is in view. For one frame, when the caret or selection was
    /// put somewhere from outside the rows (a search hit, a click on the map).
    pub reveal: Option<usize>,
}

/// What the pointer did in the rows this frame. A gap is counted like [`Cursor::caret`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// The button went down at this gap.
    Press { gap: usize, shift: bool },
    /// The pointer is at this gap with the button still down.
    Drag { gap: usize },
    /// The button went down on a feature or a primer: which of the construct's features it is,
    /// and the bases it covers, first and last, 0-based. An end before the start runs through
    /// the origin.
    Span { feature: usize, start: usize, end: usize },
    /// The button went down on one end of a made primer, to drag it (`PrimerDrag.tsx`): which of
    /// the construct's features, and whether it is its start (left) end or its end (right) one.
    PrimerEnd { feature: usize, start: bool },
    /// Zoomed out past the letters: the rows hand back to the map, at this base, the one in the
    /// middle of the view (`onZoomedOut`).
    ZoomedOut { base: usize },
}

/// What the rows saw this frame that the cards, the readout and the menu anchor on: the web reads
/// these off the drawing (`.seq-selection`, `[data-bios='row-primer']`, `[data-bios='row-cut']`,
/// the `contextmenu` target). Kept in egui's memory for the frame; read it with [`seen`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seen {
    /// The topmost piece of the selection's highlight on screen.
    pub selection: Option<Rect>,
    /// The middle of the clear air between the row that piece is in and the row above it, where
    /// the selection's pill stands without covering bases or lanes.
    pub selection_gap: Option<f32>,
    /// The feature or primer bar under the pointer: which of the construct's features, and its
    /// box on screen.
    pub bar: Option<(usize, Rect)>,
    /// The cut under the pointer, as [`Plan::site`] takes it, and the box centred on its scission.
    pub cut: Option<(usize, Rect)>,
    /// How near the pointer is to the cut's target, so the nearest of several wins.
    cut_distance: Option<f32>,
    /// The binder site or Find hit under the pointer, and its ribbon's box on screen.
    pub oligo: Option<(Oligo, Rect)>,
    /// Where the secondary button was clicked on the rows this frame.
    pub menu: Option<Pos2>,
    /// The made primer's end handle under the pointer.
    pub handle: Option<Rect>,
}

fn seen_id() -> egui::Id {
    egui::Id::unique("bios-rows-seen")
}

/// What the strip saw, kept apart from the rows below it: both are drawn every frame in the split,
/// and one would otherwise overwrite the other. Kept with the pass it was drawn in, so a strip that
/// has handed back is not still pointing at something.
fn strip_seen_id() -> egui::Id {
    egui::Id::unique("bios-strip-seen")
}

fn strip_seen(ctx: &egui::Context) -> Seen {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, Seen)>(strip_seen_id())).filter(|(at, _)| at.saturating_add(1) >= pass).map(|(_, s)| s).unwrap_or_default()
}

/// What the rows saw in the last frame they were drawn, and what the strip saw that the rows did
/// not: the pointer is in one or the other. The selection's highlight is the rows' alone, since
/// that is where the selection's own controls stand.
pub fn seen(ctx: &egui::Context) -> Seen {
    let rows: Seen = ctx.data(|d| d.get_temp(seen_id())).unwrap_or_default();
    let strip = strip_seen(ctx);
    Seen {
        selection: rows.selection,
        selection_gap: rows.selection_gap,
        bar: rows.bar.or(strip.bar),
        cut: rows.cut.or(strip.cut),
        cut_distance: rows.cut_distance.or(strip.cut_distance),
        oligo: rows.oligo.or(strip.oligo),
        menu: rows.menu.or(strip.menu),
        handle: rows.handle,
    }
}

/// The feature the pointer is on, on the map this frame or in the rows last frame: what the
/// Interactions layer lights (`setLitFeature`), one answer for both views. Primers count as
/// features here, as they are features in the file.
pub fn lit_feature(ctx: &egui::Context) -> Option<usize> {
    crate::map::hovered(ctx).or_else(|| seen(ctx).bar.map(|(index, _)| index))
}

/// Brings `base` to the middle of the rows: the map's window, when the rows follow a zoom of the
/// map (`SequenceRows.tsx` `apply`).
fn center_on(ui: &mut egui::Ui, plan: &Plan, viewport: Rect, base: usize) {
    let index = usize::try_from(layout::row_of_base(base as i64, plan.metrics.bps_per_row)).unwrap_or(0).min(plan.rows.len().saturating_sub(1));
    if let (Some(&top), Some(&next)) = (plan.plan.tops.get(index), plan.plan.tops.get(index + 1)) {
        let want = (top + (next - top) / 2.0 - f64::from(viewport.height()) / 2.0).max(0.0) as f32;
        ui.scroll_with_delta(vec2(0.0, viewport.min.y - want));
    }
}

/// The rows zoomed out past the letters: the base in the middle of `viewport` to hand the map, with
/// the zoom put back to the default width.
fn hand_back(ui: &egui::Ui, plan: &Plan, viewport: Rect, key: &str) -> Option<Gesture> {
    if !layout::rows_hand_back(plan.metrics.char_width) || plan.rows.is_empty() {
        return None;
    }
    let index = layout::row_at(&plan.plan, f64::from(viewport.center().y));
    let start = plan.rows.get(index).map_or(0, |r| r.start);
    let base = (start + plan.metrics.bps_per_row / 2).clamp(0, plan.rows.last().map_or(0, |r| r.end));
    ui.data_mut(|d| d.insert_temp(zoom_id(key), layout::DEFAULT_CHAR_WIDTH));
    ui.ctx().request_repaint();
    Some(Gesture::ZoomedOut { base: usize::try_from(base).unwrap_or(0) })
}

/// Keeps what the rows saw for the next frame, with what only shows once the drawing is done.
fn settle(ui: &egui::Ui, seen: Seen, before: &Seen) {
    // The handle's `title` (`PrimerDrag.tsx` 110).
    if let Some(rect) = seen.handle {
        ui.interact(rect, ui.scope_id().with("primer-handle"), egui::Sense::hover()).on_hover_text("Drag to resize the primer");
    }
    if (seen.bar.map(|b| b.0), seen.oligo.map(|o| o.0)) != (before.bar.map(|b| b.0), before.oligo.map(|o| o.0)) {
        // The wash follows on the next frame, so there has to be one.
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(seen_id(), seen));
}

/// The annotation [`Seen`] says the pointer is on, as the rows drew it, and whether it is in the
/// primer lane.
fn washed<'a>(plan: &'a Plan, seen: &Seen) -> Option<(&'a Shown, bool)> {
    if let Some((oligo, _)) = seen.oligo {
        return plan.primers.iter().find(|f| f.oligo == Some(oligo)).map(|f| (f, true));
    }
    let (index, _) = seen.bar?;
    let lane = |list: &'a [Shown]| list.iter().find(|f| f.oligo.is_none() && f.index == index);
    lane(&plan.primers).map(|f| (f, true)).or_else(|| lane(&plan.features).or_else(|| lane(&plan.parts)).map(|f| (f, false)))
}

/// A press inside the selection, waiting to learn whether it is a double-click.
#[derive(Clone, Copy, Debug)]
struct Pending {
    gap: usize,
    at: Pos2,
    /// When the button came up, in the window's seconds.
    released: Option<f64>,
}

/// How long a click inside the selection waits for a second one (`collapseTimer`).
const DOUBLE_CLICK: f64 = 0.28;

fn pending_id() -> egui::Id {
    egui::Id::unique("bios-rows-pending")
}

/// The width a base is asked for, as the rows' zoom left it: one per plasmid, so zooming one
/// leaves every other where it was.
fn zoom_id(key: &str) -> egui::Id {
    egui::Id::unique(("bios-rows-zoom", key))
}

/// The base a zoom is held about, and how far down the viewport the pointer was.
fn anchor_id(key: &str) -> egui::Id {
    egui::Id::unique(("bios-rows-zoom-anchor", key))
}

/// How far either side of a cut's hairline still counts as pointing at it (`CutHoverCard.tsx`).
const CUT_SLOP_X: f32 = 5.0;
const CUT_SLOP_Y: f32 = 3.0;

/// One blink of the caret, on and then off, in seconds: the web app's.
const BLINK: f64 = 1.06;

/// What the editor pane asks of the rows beyond the construct itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'a> {
    /// Which plasmid this is: its zoom and its scroll are its own.
    pub key: &'a str,
    /// Room kept under the last row so it can scroll out from under the floating tool row
    /// (`.seq-rows-content { padding-bottom }`).
    pub run_off: f32,
    /// The cut sites to draw, as the scissors dial chose them. `None` draws the default ones.
    pub sites: Option<&'a [CutSite]>,
    /// What the layers popover shows: the rows honour Enzyme names and the base colour.
    pub layers: Layers,
    /// The feature the pointer is on, in the rows or on the map: with the Interactions layer on,
    /// everything not wired to it greys back (`editor/interactions.ts` `setLitFeature`).
    pub lit: Option<usize>,
    /// The sites of the binders held against the construct, drawn in the primer lane.
    pub binders: &'a [BinderSite],
    /// Find's alignments, best first: the first [`DRAWN_LIMIT`] are drawn, with their captions.
    pub hits: &'a [AlignHit],
    /// Where an Exact Find landed, drawn as ribbons in the found colours while the bar is open.
    pub found: &'a [Found],
    /// Changes whenever `binders`, `hits` or `found` do, so the rows know to lay out again.
    pub oligo_key: u64,
    /// The translations asked for from the selection's menu, floating over the bases.
    pub translations: &'a [Translation],
    /// A base to bring to the middle of the rows this frame: the map's window, when the rows
    /// follow a zoom of the map.
    pub center_on: Option<usize>,
}

/// What the rows gain from the layers and from what was asked for.
fn extras_of(options: &Options) -> Extras {
    Extras {
        amino_acids: options.layers.amino_acids,
        orfs: options.layers.orfs,
        splice_scan: options.layers.splice_scan,
        translations: options.translations.to_vec(),
    }
}

/// The two layers that change the row's own shape (SequenceRows.tsx 397-400).
fn tracks_of(options: &Options) -> Tracks {
    Tracks { reverse_sequence: options.layers.reverse_strand, axis: options.layers.ruler }
}

pub fn show(ui: &mut egui::Ui, c: &Construct, revision: u64, cache: &mut Option<Plan>, cursor: &Cursor) -> Option<Gesture> {
    show_with(ui, c, revision, cache, cursor, Options::default())
}

/// What a picture of the rows came out as: the bases it holds, 0-based and inclusive, and how
/// tall those rows actually are, so a picture can be cut down to them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub from: i64,
    pub to: i64,
    pub height: f64,
}

/// The rows of `c` drawn into `rect` as a PICTURE rather than a pane: the whole rows that hold
/// bases `from`..`to`, laid out at `rect`'s width, from the top of `rect` down. Nothing answers
/// the pointer, nothing is selected and nothing scrolls, so only as many rows as `rect` is tall
/// enough to hold are drawn, and the answer says which bases those were.
///
/// The first row is always drawn, however short the rectangle: a picture with nothing in it says
/// less than a clipped one.
pub fn draw_rows(painter: &Painter, rect: Rect, c: &Construct, from: i64, to: i64) -> Drawn {
    let options = Options::default();
    let plan = Plan::build(c, rect.width(), 0, default_cut_sites(c), &options, layout::DEFAULT_CHAR_WIDTH, None);
    let last = plan.rows.len().saturating_sub(1);
    let row_of = |bp: i64| usize::try_from(layout::row_of_base(bp.max(0), plan.metrics.bps_per_row.max(1))).unwrap_or(0).min(last);
    let first = row_of(from);
    // A stretch running through the origin cannot be one run of rows, so it runs to the end.
    let wanted = if to < from { last } else { row_of(to).max(first) };
    let Some(&top0) = plan.plan.tops.get(first) else {
        return Drawn { from: 0, to: 0, height: 0.0 };
    };
    let cursor = Cursor::default();
    let on_bar = std::cell::Cell::new(None);
    let on_end = std::cell::Cell::new(None);
    let seen = std::cell::RefCell::new(Seen::default());
    let left = rect.left() + (plan.metrics.margin_width / 2.0) as f32;
    // In a pane the first row sits at the very top and the coordinate over a fiftieth in it is
    // clipped by the scroller; a picture has no scroller, so the first row gets the same air above
    // it that every row has below it, and the number reads.
    let air = track::ROW_GAP / 2.0;
    // How tall the rows are measured in, which has to be the layout's own arithmetic: rounded to
    // the screen's units first, a row that fits exactly comes out a hair too tall and is dropped.
    let room = f64::from(rect.height());
    let mut drawn = first;
    for index in first..=wanted {
        let (Some(&top), Some(&next)) = (plan.plan.tops.get(index), plan.plan.tops.get(index + 1)) else {
            break;
        };
        if index > first && air + (next - top0) > room {
            break;
        }
        let y = rect.top() + (air + top - top0) as f32;
        let sheet = Sheet {
            painter,
            origin: pos2(left, y),
            plan: &plan,
            c,
            index,
            height: next - top,
            cursor: &cursor,
            caret_on: false,
            base_style: options.layers.base_style,
            lit: None,
            press: None,
            on_bar: &on_bar,
            on_end: &on_end,
            hover: None,
            wash: None,
            hot_cut: None,
            seen: &seen,
        };
        sheet.draw();
        drawn = index;
    }
    let bases = |index: usize| plan.rows.get(index).copied().unwrap_or(Row { index, start: 0, end: 0 });
    let bottom = plan.plan.tops.get(drawn + 1).copied().unwrap_or(top0);
    Drawn { from: bases(first).start, to: bases(drawn).end.max(bases(first).start), height: air + (bottom - top0) }
}

pub fn show_with(ui: &mut egui::Ui, c: &Construct, revision: u64, cache: &mut Option<Plan>, cursor: &Cursor, options: Options) -> Option<Gesture> {
    let width = ui.available_width();
    let stale_sites = |p: &Plan| options.sites.is_some_and(|s| s != p.sites.as_slice());
    let cut_names = options.layers.cut_names;
    let tracks = tracks_of(&options);
    let stale_oligos = |p: &Plan| p.primers_shown != options.layers.binders || p.oligo_key != options.oligo_key;
    let asked = ui.data(|d| d.get_temp::<f64>(zoom_id(options.key))).unwrap_or(layout::DEFAULT_CHAR_WIDTH);
    if cache.as_ref().is_none_or(|p| {
        p.width != width
            || p.asked != asked
            || p.revision != revision
            || stale_sites(p)
            || p.cut_names != cut_names
            || p.interactions != options.layers.interactions
            || p.show_parts != options.layers.parts
            || p.tracks != tracks
            || stale_oligos(p)
            || p.extras != extras_of(&options)
    }) {
        let sites = match options.sites {
            Some(s) => s.to_vec(),
            None => cache.take().filter(|p| p.revision == revision).map_or_else(|| default_cut_sites(c), |p| p.sites),
        };
        *cache = Some(Plan::build(c, width, revision, sites, &options, asked, None));
    }
    let plan = cache.as_ref()?;
    // The caret is drawn for the first half of each blink, counted from when it last moved.
    let caret_on = cursor.moved.is_none_or(|moved| {
        let into = (ui.input(|i| i.time) - moved).rem_euclid(BLINK);
        let half = BLINK / 2.0;
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(if into < half { half - into } else { BLINK - into }));
        into < half
    });
    let mut gesture = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("rows", options.key)).show_viewport(ui, |ui, viewport| {
        ui.set_width(width);
        // The first row starts at the top of the sheet, with no air above it: the web places row 0 at
        // `top: 0` of `.seq-rows-content`, so the number over a fiftieth in it stands above the
        // scroller and is clipped, as it is there (SequenceRows.tsx 1318, `.seq-rows-content`).
        ui.set_height(plan.plan.height as f32 + options.run_off.max(0.0));
        let origin = ui.min_rect().min;
        let left = origin.x + (plan.metrics.margin_width / 2.0) as f32;
        let sheet_rect = ui.min_rect();
        let response = ui.interact(sheet_rect, ui.scope_id().with("sheet"), egui::Sense::click_and_drag());
        // Which gap a point on the sheet falls in: the row from how far down it is, the gap from
        // how far along, both by the layout's own arithmetic so a click lands where the letter is.
        let gap_at = |at: Pos2| -> Option<usize> {
            let row = plan.rows.get(layout::row_at(&plan.plan, f64::from(at.y - origin.y)))?;
            usize::try_from(layout::caret_at_x(f64::from(at.x - left), *row, plan.metrics.char_width)).ok()
        };
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        // Where a plain press landed, for the bars to say whether it was on one of them.
        let press = response.interact_pointer_pos().filter(|_| pressed && !ui.input(|i| i.modifiers.shift));
        let on_bar = std::cell::Cell::new(None);
        let on_end = std::cell::Cell::new(None);
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|at| over_rows(ui, *at));
        let seen = std::cell::RefCell::new(Seen {
            menu: if response.secondary_clicked() { ui.input(|i| i.pointer.interact_pos()) } else { None },
            ..Seen::default()
        });
        let shift = ui.input(|i| i.modifiers.shift);
        let now = ui.input(|i| i.time);
        // A press inside the selection waits: a second press makes it a double-click, which opens
        // the selection's menu, and only once the double-click window has passed does a single
        // click collapse to a caret (`onPointerUp`, 280ms). A drag from it selects from there.
        let mut pending: Option<Pending> = ui.data(|d| d.get_temp(pending_id()));
        let inside = |gap: usize| cursor.selection.is_some_and(|(s, e)| s <= e && gap >= s && gap <= e);
        if let Some(gap) = response.interact_pointer_pos().and_then(gap_at) {
            if pressed && !shift && inside(gap) {
                pending = Some(Pending { gap, at: response.interact_pointer_pos().unwrap_or_default(), released: None });
            } else if pressed {
                pending = None;
                gesture = Some(Gesture::Press { gap, shift });
            } else if response.dragged_by(egui::PointerButton::Primary) {
                match pending.take() {
                    Some(p) if p.gap != gap => gesture = Some(Gesture::Press { gap: p.gap, shift: false }),
                    Some(p) => pending = Some(p),
                    None => gesture = Some(Gesture::Drag { gap }),
                }
            }
        }
        if let Some(p) = pending.as_mut() {
            if response.double_clicked() {
                seen.borrow_mut().menu = Some(p.at);
                pending = None;
            } else if p.released.is_none() && !ui.input(|i| i.pointer.primary_down()) {
                p.released = Some(now);
            }
        }
        if let Some(p) = pending.filter(|p| p.released.is_some()) {
            let waited = now - p.released.unwrap_or(now);
            if waited >= DOUBLE_CLICK {
                gesture = Some(Gesture::Press { gap: p.gap, shift: false });
                pending = None;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(DOUBLE_CLICK - waited));
            }
        }
        ui.data_mut(|d| match pending {
            Some(p) => {
                d.insert_temp(pending_id(), p);
            }
            None => d.remove::<Pending>(pending_id()),
        });
        // A drag held against the top or bottom edge scrolls on, and keeps selecting under the
        // pointer, since the rows moved under a pointer that did not.
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(at) = ui.input(|i| i.pointer.interact_pos())
        {
            let clip = ui.clip_rect();
            let step = layout::edge_scroll(f64::from(at.y - clip.top()), f64::from(clip.height()));
            if step != 0.0 {
                ui.scroll_with_delta(vec2(0.0, -step as f32));
                ui.ctx().request_repaint();
            }
        }
        // Pinch, or ⌘ with the wheel: the bases widen or narrow about the base under the pointer,
        // which stays where it is (`onWheel`, the zoom anchor).
        if let Some(at) = ui.input(|i| i.pointer.hover_pos()).filter(|at| over_rows(ui, *at)) {
            let (zoom, scroll, command) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.command || i.modifiers.ctrl));
            let dz = if zoom != 1.0 {
                Some(-f64::from(zoom).ln() / 0.02)
            } else if command && scroll.y != 0.0 {
                Some(-f64::from(scroll.y))
            } else {
                None
            };
            if let Some(dz) = dz {
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
                let next = layout::zoom_char_width(plan.metrics.char_width, dz);
                if (next - plan.metrics.char_width).abs() > f64::EPSILON {
                    let row = plan.rows.get(layout::row_at(&plan.plan, f64::from(at.y - origin.y)));
                    let bp = row.and_then(|r| layout::base_at_x(f64::from(at.x - left), *r, plan.metrics.char_width).or(Some(r.start)));
                    if let Some(bp) = bp {
                        ui.data_mut(|d| d.insert_temp(anchor_id(options.key), (bp, at.y - ui.clip_rect().top())));
                    }
                    ui.data_mut(|d| d.insert_temp(zoom_id(options.key), next));
                    ui.ctx().request_repaint();
                }
            }
        }
        // After a zoom has laid the rows out again, the anchored base goes back under the pointer.
        if let Some((bp, y)) = ui.data(|d| d.get_temp::<(i64, f32)>(anchor_id(options.key)))
            && (plan.metrics.char_width - asked).abs() < 1.0
        {
            ui.data_mut(|d| d.remove::<(i64, f32)>(anchor_id(options.key)));
            let index = usize::try_from(layout::row_of_base(bp, plan.metrics.bps_per_row)).unwrap_or(0).min(plan.rows.len().saturating_sub(1));
            if let Some(&top) = plan.plan.tops.get(index) {
                let want = top as f32 - y;
                ui.scroll_with_delta(vec2(0.0, viewport.min.y - want));
            }
        }
        // The door back: zoomed out past the letters, the rows hand the map the base in the middle
        // of the view, and come back up to a readable width, so the next hand-over does not hand
        // straight back (`onZoomedOut`). Reported after the fact rather than refusing the zoom, so
        // the gesture that takes you out is the one you were already making.
        gesture = hand_back(ui, plan, viewport, options.key).or(gesture);
        if let Some(base) = options.center_on {
            center_on(ui, plan, viewport, base);
        }
        if let Some(base) = cursor.reveal {
            let index = usize::try_from(layout::row_of_base(base as i64, plan.metrics.bps_per_row)).unwrap_or(0);
            if let (Some(&top), Some(&next)) = (plan.plan.tops.get(index), plan.plan.tops.get(index + 1)) {
                let row = Rect::from_min_max(pos2(sheet_rect.left(), origin.y + top as f32), pos2(sheet_rect.right(), origin.y + next as f32));
                // At once: a glide through thousands of bases is a wait, and says nothing.
                // Only as far as it takes to bring the row into view: a caret walked down with the
                // arrows should move the page a row at a time, not recentre it at every step.
                ui.scroll_to_rect_animation(row, None, egui::style::ScrollAnimation::none());
            }
        }
        // What the pointer was on when the rows were last drawn: the bars claim the pointer as
        // they are drawn, and the wash goes under the letters, before them.
        let before = self::seen(ui.ctx());
        let wash = hover.and_then(|_| washed(plan, &before));
        let hot_cut = hover.and_then(|_| hot_cut(plan, &before));
        let painter = ui.painter();
        let first = layout::row_at(&plan.plan, f64::from(viewport.min.y));
        for index in first..plan.rows.len() {
            let (Some(&top), Some(&next)) = (plan.plan.tops.get(index), plan.plan.tops.get(index + 1)) else {
                break;
            };
            if top as f32 > viewport.max.y {
                break;
            }
            let sheet = Sheet {
                painter,
                origin: pos2(left, origin.y + top as f32),
                plan,
                c,
                index,
                height: next - top,
                cursor,
                caret_on,
                base_style: options.layers.base_style,
                lit: options.lit.filter(|_| plan.interactions),
                press,
                on_bar: &on_bar,
                on_end: &on_end,
                hover,
                wash,
                hot_cut,
                seen: &seen,
            };
            sheet.draw();
        }
        if let Some((feature, start, end)) = on_bar.get() {
            gesture = Some(Gesture::Span { feature, start, end });
            // A press on a bar is about the bar, even inside the selection.
            ui.data_mut(|d| d.remove::<Pending>(pending_id()));
        }
        if let Some((feature, start)) = on_end.get() {
            gesture = Some(Gesture::PrimerEnd { feature, start });
            ui.data_mut(|d| d.remove::<Pending>(pending_id()));
        }
        settle(ui, seen.into_inner(), &before);
    });
    gesture
}

/// The strip the map's pane becomes in the split once a base is wide enough to read: the rows' own
/// row, drawn once, unwrapped and slid sideways (`SequenceRows` with `strip`, `PlasmidEditor`'s
/// `letters`). It owns its window from the moment the map hands over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// Where it looks, in the map's units.
    pub vp: Viewport,
    /// The caret and selection it last saw, so that it slides to one put outside it from elsewhere
    /// and not to the one already there when it opened.
    cursor: (Option<usize>, Option<(usize, usize)>),
}

impl Strip {
    /// The strip opening where the map was looking, with the caret where it is.
    pub fn open(vp: Viewport, cursor: &Cursor) -> Strip {
        Strip { vp: strip::apply(vp, 0.0), cursor: (cursor.caret, cursor.selection) }
    }
}

/// What the strip did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StripOut {
    pub gesture: Option<Gesture>,
    /// Zoomed out past the letters: the map takes the pane back at this window.
    pub handed_back: Option<Viewport>,
}

/// The strip's width a base at `vp` in a pane `box_w` wide on a construct `length` long: what the
/// door back is judged on.
fn strip_char_width(length: i64, box_w: f64, vp: Viewport) -> Option<f64> {
    strip::geometry(length, box_w, vp).map(|g| g.metrics.char_width)
}

/// Draws the strip in `rect` and answers its wheel: plain scroll slides along the strand, pinch or
/// ⌘-wheel zooms about the base under the pointer (`onWheel`'s strip branch). A press places the
/// caret and a drag selects, as in the rows; it takes no keys, which stay with the rows below.
/// Its weld sits on the middle of the pane, and its number at the right says the last base in view.
/// `cache` holds its row laid out, rebuilt when the window moves by a base or more.
#[allow(clippy::too_many_arguments)]
pub fn show_strip(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &Construct,
    revision: u64,
    s: &mut Strip,
    cache: &mut Option<Plan>,
    cursor: &Cursor,
    options: Options,
) -> StripOut {
    let mut out = StripOut::default();
    let length = c.len() as i64;
    let box_w = f64::from(rect.width());
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|at| rect.contains(*at) && ui.ctx().layer_id_at(*at).is_none_or(|l| l == ui.layer_id()));
    if let Some(at) = hover {
        let (zoom, scroll, command) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.command || i.modifiers.ctrl));
        let x = f64::from(at.x - rect.left());
        if zoom != 1.0 {
            s.vp = strip::wheel_zoom(s.vp, -f64::from(zoom).ln() / 0.02, x, box_w, length);
        } else if command && scroll != egui::Vec2::ZERO {
            s.vp = strip::wheel_zoom(s.vp, -f64::from(scroll.y), x, box_w, length);
        } else if scroll != egui::Vec2::ZERO {
            // egui's scroll is the content's motion; the web's wheel delta is the opposite sign.
            let delta = bios_layout::overview::navigate::dominant(-f64::from(scroll.x), -f64::from(scroll.y));
            s.vp = strip::wheel_pan(s.vp, delta, box_w, length);
        }
        if zoom != 1.0 || scroll != egui::Vec2::ZERO {
            ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
        }
    }
    s.vp = strip::apply(s.vp, strip::min_width(box_w, length));
    // Something else put the caret outside the window: slide to it.
    let now_cursor = (cursor.caret, cursor.selection);
    if now_cursor != s.cursor {
        s.cursor = now_cursor;
        let at = cursor.selection.map(|(a, _)| a).or(cursor.caret);
        if let (Some(at), Some(g)) = (at, strip::geometry(length, box_w, s.vp))
            && let Some(vp) = strip::follow_caret(s.vp, &g, at, length)
        {
            s.vp = vp;
        }
    }
    let Some(g) = strip::geometry(length, box_w, s.vp) else {
        return out;
    };
    // Not before the pane has a width: a zero-width box computes a zero slot and would hand back
    // on the first frame, before a letter had been drawn.
    if box_w > 0.0 && strip_char_width(length, box_w, s.vp).is_some_and(strip::hands_back) {
        out.handed_back = Some(s.vp);
    }
    let key = StripKey::of(&g);
    let tracks = tracks_of(&options);
    let sites = options.sites.map_or_else(|| default_cut_sites(c), <[CutSite]>::to_vec);
    if cache.as_ref().is_none_or(|p| {
        p.window != Some(key)
            || p.revision != revision
            || p.sites != sites
            || p.cut_names != options.layers.cut_names
            || p.interactions != options.layers.interactions
            || p.show_parts != options.layers.parts
            || p.tracks != tracks
            || p.primers_shown != options.layers.binders
            || p.oligo_key != options.oligo_key
            || p.extras != extras_of(&options)
    }) {
        *cache = Some(Plan::build(c, rect.width(), revision, sites, &options, layout::DEFAULT_CHAR_WIDTH, Some(&g)));
    }
    let Some(plan) = cache.as_ref() else {
        return out;
    };
    let (Some(&row), Some(content), Some(&next)) = (plan.rows.first(), plan.content.first(), plan.plan.tops.get(1)) else {
        return out;
    };
    let margin = plan.metrics.margin_width;
    let content_w = strip::content_width(box_w) as f32;
    let left = rect.left() + (margin / 2.0) as f32;
    let off = layout::duplex_top(content.lanes);
    let bands = layout::bands_for(plan.tracks);
    // The weld on the middle of whatever height the split gives it: the molecule's centre line,
    // not the row box's, which carries the stacks above.
    let row_top = (f64::from(rect.height()) / 2.0 - (off + bands.duplex)).max(0.0);
    let origin = pos2(left - g.shift as f32, rect.top() + row_top as f32);
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(left + content_w, rect.bottom())).intersect(ui.clip_rect());

    let response = ui.interact(rect, ui.scope_id().with("strip"), egui::Sense::click_and_drag());
    let gap_at = |at: Pos2| usize::try_from(layout::caret_at_x(f64::from(at.x - origin.x), row, plan.metrics.char_width)).ok();
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(gap) = response.interact_pointer_pos().and_then(gap_at) {
        if ui.input(|i| i.pointer.primary_pressed()) {
            out.gesture = Some(Gesture::Press { gap, shift });
        } else if response.dragged_by(egui::PointerButton::Primary) {
            out.gesture = Some(Gesture::Drag { gap });
        }
    }
    let press = response.interact_pointer_pos().filter(|_| ui.input(|i| i.pointer.primary_pressed()) && !shift);
    let on_bar = std::cell::Cell::new(None);
    let on_end = std::cell::Cell::new(None);
    let seen = std::cell::RefCell::new(Seen {
        menu: if response.secondary_clicked() { ui.input(|i| i.pointer.interact_pos()) } else { None },
        ..Seen::default()
    });
    let before = strip_seen(ui.ctx());
    let wash = hover.and_then(|_| washed(plan, &before));
    let hot_cut = hover.and_then(|_| hot_cut(plan, &before));
    let caret_on = cursor.moved.is_none_or(|moved| (ui.input(|i| i.time) - moved).rem_euclid(BLINK) < BLINK / 2.0);
    let painter = ui.painter().with_clip_rect(clip);
    Sheet {
        painter: &painter,
        origin,
        plan,
        c,
        index: 0,
        height: next - plan.plan.tops.first().copied().unwrap_or(0.0),
        cursor,
        caret_on,
        base_style: options.layers.base_style,
        lit: options.lit.filter(|_| plan.interactions),
        press,
        on_bar: &on_bar,
        on_end: &on_end,
        hover,
        wash,
        hot_cut,
        seen: &seen,
    }
    .draw();
    // The number stands outside the clip, in the right margin, level with the weld.
    ui.painter().with_clip_rect(rect).text(
        pos2(left + content_w + 8.0, origin.y + (off + bands.duplex - 8.0) as f32),
        egui::Align2::LEFT_TOP,
        g.last.to_string(),
        theme::mono(13.0),
        theme::MUTED,
    );
    // A press on a bar is about the bar. The strip is not where a primer is dragged.
    if let Some((feature, start, end)) = on_bar.get() {
        out.gesture = Some(Gesture::Span { feature, start, end });
    }
    let seen = seen.into_inner();
    if (seen.bar.map(|b| b.0), seen.oligo.map(|o| o.0)) != (before.bar.map(|b| b.0), before.oligo.map(|o| o.0)) {
        ui.ctx().request_repaint();
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| d.insert_temp(strip_seen_id(), (pass, seen)));
    out
}

/// A base's colour in the given style: the five `[data-base-style]` palettes in Bios `styles.css`
/// (lines 3317-3332). Calm greys every letter; the others colour A, T, C and G only, in either
/// case, and leave anything else in the strands' ink (`.seq-row-bases`).
pub fn base_color(style: BaseStyle, base: &str) -> Color32 {
    let (a, t, c, g) = match style {
        BaseStyle::Calm => return theme::MUTED,
        BaseStyle::Black => return theme::INK,
        BaseStyle::Muted => (0xa9772f, 0xb0494b, 0x4a6fb0, 0x3f8158),
        BaseStyle::Vivid => (0xe07a00, 0xda1f22, 0x045afa, 0x00934e),
        BaseStyle::GcAt => (0xb3a795, 0xb3a795, 0x0e7c86, 0x0e7c86),
    };
    match base {
        "A" | "a" => theme::hex(a),
        "T" | "t" => theme::hex(t),
        "C" | "c" => theme::hex(c),
        "G" | "g" => theme::hex(g),
        _ => theme::INK,
    }
}

/// One row being drawn: the painter, and where the row's own (0, 0) falls on screen. `x` runs
/// from the first base's left edge and `y` from the row's top, as in `bios_layout`.
struct Sheet<'a> {
    painter: &'a Painter,
    origin: Pos2,
    plan: &'a Plan,
    c: &'a Construct,
    index: usize,
    /// The row's height, the gap below it included.
    height: f64,
    cursor: &'a Cursor,
    /// Whether the caret is in the lit half of its blink.
    caret_on: bool,
    base_style: BaseStyle,
    /// The feature lit by the pointer while the Interactions layer is on.
    lit: Option<usize>,
    /// Where a plain press landed this frame.
    press: Option<Pos2>,
    /// The span of the feature or primer that press was on, once a bar has claimed it.
    on_bar: &'a std::cell::Cell<Option<(usize, usize, usize)>>,
    /// The made primer end that press was on, and whether it is the start.
    on_end: &'a std::cell::Cell<Option<(usize, bool)>>,
    /// Where the pointer is over the rows.
    hover: Option<Pos2>,
    /// The annotation the pointer is on, washed into the bases under it, and whether it is in
    /// the primer lane.
    wash: Option<(&'a Shown, bool)>,
    /// The cut the pointer is on: the bases it reads and the pairing it destroys (`cutHover.ts`).
    hot_cut: Option<HotCut>,
    seen: &'a std::cell::RefCell<Seen>,
}

/// The cut under the pointer, as the rows answer it: its site washed in place and the rungs
/// across its overhang dropped (`SequenceRows.tsx` 1363-1377 and 224-228, `rows/cutHover.ts`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct HotCut {
    /// Which cut, as [`Plan::site`] takes it.
    index: usize,
    /// 0-based inclusive: the bases the enzyme reads.
    site: (i64, i64),
    /// 0-based inclusive: the bases left paired to nothing; `None` for a blunt cutter.
    overhang: Option<(i64, i64)>,
}

/// `overhangOf` in `rows/cutHover.ts`: between the two scissions, empty when they meet.
fn overhang_of(top: i64, bottom: i64) -> Option<(i64, i64)> {
    let (from, to) = (top.min(bottom), top.max(bottom) - 1);
    (to >= from).then_some((from, to))
}

/// The hovered cut as last seen, looked up in the rows' cuts.
fn hot_cut(plan: &Plan, before: &Seen) -> Option<HotCut> {
    let (index, _) = before.cut?;
    let cut = plan.content.iter().flat_map(|c| c.cuts.iter()).find(|c| c.cut == index)?;
    Some(HotCut { index, site: (cut.site_from, cut.site_to), overhang: overhang_of(cut.top_cut, cut.bottom_cut) })
}

impl Sheet<'_> {
    fn at(&self, x: f64, y: f64) -> Pos2 {
        pos2(self.origin.x + x as f32, self.origin.y + y as f32)
    }

    fn line(&self, from: (f64, f64), to: (f64, f64), width: f32, color: Color32) {
        self.painter.line_segment([self.at(from.0, from.1), self.at(to.0, to.1)], Stroke::new(width, color));
    }

    /// Text standing on `baseline`, with `anchor` saying which part of it sits at `x`.
    /// Gives the box the text was drawn in.
    fn text(&self, (x, baseline): (f64, f64), anchor: Align, text: &str, font: FontId, color: Color32) -> Rect {
        let galley = self.painter.layout_no_wrap(text.to_owned(), font, color);
        let ascent = galley.rows.first().and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y)).unwrap_or(galley.size().y);
        let shift = match anchor {
            Align::Min => 0.0,
            Align::Center => galley.size().x / 2.0,
            Align::Max => galley.size().x,
        };
        let at = self.at(x, baseline) - vec2(shift, ascent);
        let drawn = Rect::from_min_size(at, galley.size());
        self.painter.galley(at, galley, color);
        drawn
    }

    fn draw(&self) {
        let (Some(&row), Some(content)) = (self.plan.rows.get(self.index), self.plan.content.get(self.index)) else {
            return;
        };
        let off = layout::duplex_top(content.lanes);
        let bands = layout::bands_for(self.plan.tracks);
        let [features_above, features_below] = &content.features;
        let [primers_above, primers_below] = &content.primers;

        self.band(row);
        self.wash(row, off, &bands);
        self.cut_site(row, off, &bands);
        // The selection's fill goes down first, under the scale and the letters it is showing.
        let pieces = self.selected(row);
        for span in &pieces {
            let (top, bottom) = (off + track::SPACER, off + bands.after_strands);
            let rect = Rect::from_min_max(self.at(span.x_start, top), self.at(span.x_start + span.width, bottom));
            self.painter.rect_filled(rect, 0.0, theme::CELL_ACTIVE);
            let mut seen = self.seen.borrow_mut();
            if seen.selection.is_none_or(|was| rect.top() < was.top()) && rect.bottom() > self.painter.clip_rect().top() {
                seen.selection = Some(rect);
                seen.selection_gap = Some(self.at(0.0, -track::ROW_GAP / 2.0).y);
            }
        }
        if self.plan.tracks.axis {
            self.rules(row, off + track::SPACER, off + bands.after_strands);
        }
        // The forward strand's own, above it, stacked outward from the bases: primers sit nearest
        // on both sides, so the row is symmetrical about its molecule.
        // Above, read outward from the duplex: ORFs outermost, then parts, then features, then
        // primers (`upOrfs`, `upParts`, `upFeatures`, `upPrimers`).
        let [parts_above, parts_below] = &content.parts;
        let [orfs_above, orfs_below] = &content.orfs;
        let above_top = off - layout::lane_block_height(content.lanes.above);
        self.orfs(orfs_above, above_top);
        let above_top = above_top + orfs_above.height;
        self.features(&self.plan.parts, Kind::Part, parts_above, above_top);
        let above_top = above_top + parts_above.height;
        self.features(&self.plan.features, Kind::Feature, features_above, above_top);
        self.primers(primers_above, above_top + features_above.height, true);
        self.strands(row, off);
        // Below: primers, features, parts, ORFs, then the amino acids. What each of Find's hits is
        // is said in the Find bar, not on the rows (PARITY.md).
        let below_top = off + bands.lanes_top;
        self.primers(primers_below, below_top, false);
        self.features(&self.plan.features, Kind::Feature, features_below, below_top + primers_below.height);
        let after_features = below_top + primers_below.height + features_below.height;
        self.features(&self.plan.parts, Kind::Part, parts_below, after_features);
        let after_parts = after_features + parts_below.height;
        self.orfs(orfs_below, after_parts);
        let after_orfs = after_parts + orfs_below.height;
        self.residues(&content.residues, after_orfs);
        self.edges(row, &pieces);
        self.cuts(row, &content.cuts, off);
        // The ribbons take the very top of the row, above the enzyme names: a relationship spans
        // the molecule and everything else in the row is about one place in it.
        self.interactions(&content.inter);
    }

    /// Whether this feature steps back: the layer is asking what is wired to the lit feature, and
    /// this is not part of the answer.
    fn dimmed(&self, index: usize) -> bool {
        self.lit.is_some() && !self.plan.graph.is_lit(self.lit, index)
    }

    /// `RowInteractions`: each relationship in the actor's colour, a bracket or a stub with its
    /// verb, greyed back when it is not part of what the pointer is asking.
    fn interactions(&self, pieces: &[EdgePiece]) {
        for p in pieces {
            let lit = self.lit.is_none() || (self.plan.graph.is_lit(self.lit, p.actor) && self.plan.graph.is_lit(self.lit, p.target));
            let base = self.c.features.get(p.actor).map_or(theme::MUTED, crate::acts_on::wire_color);
            let color = if lit { base } else { theme::alpha(base, 0.14) };
            let d = edge_layout::drawing(p, 0.0);
            let line: Vec<Pos2> = d.line.iter().map(|&(x, y)| self.at(x, y)).collect();
            self.painter.add(Shape::line(line, Stroke::new(1.25, color)));
            if let Some(head) = d.head {
                self.painter.add(Shape::convex_polygon(head.iter().map(|&(x, y)| self.at(x, y)).collect(), color, Stroke::NONE));
            }
            let anchor = match d.anchor {
                Anchor::Start => Align::Min,
                Anchor::Middle => Align::Center,
                Anchor::End => Align::Max,
            };
            self.text(d.label_at, anchor, &p.label, theme::sans(10.5), color);
        }
    }

    /// A press on this bar selects what the bar stands for. An oligo that is not in the file only
    /// says it is under the pointer.
    fn claim(&self, bar: Rect, f: &Shown) {
        if let Some(oligo) = f.oligo {
            if self.hover.is_some_and(|at| bar.contains(at)) {
                self.seen.borrow_mut().oligo = Some((oligo, bar));
            }
            return;
        }
        if let (Some(at), Ok(start), Ok(end)) = (self.press, usize::try_from(f.start), usize::try_from(f.end))
            && bar.contains(at)
        {
            self.on_bar.set(Some((f.index, start, end)));
        }
        if self.hover.is_some_and(|at| bar.contains(at)) {
            self.seen.borrow_mut().bar = Some((f.index, bar));
        }
    }

    /// Hovering a feature or a primer washes it into the letters under it, on its own strand: a
    /// feature in its own colour thinned, an oligo in the primer wash (`SequenceRows.tsx`
    /// 1379-1395, `.seq-ann-wash` in `styles.css` 10192-10199).
    fn wash(&self, row: Row, off: f64, bands: &layout::Bands) {
        let Some((f, primer)) = self.wash else {
            return;
        };
        let color = if primer { theme::PRIMER_WASH } else { theme::alpha(f.color, 0.22) };
        let (top, height) = if f.forward { (off + track::SPACER, track::SEQUENCE) } else { (off + bands.rung_bottom, track::REVERSE_SEQUENCE) };
        for (from, to) in layout::unwrap(f.start, f.end, self.c.len() as i64) {
            if let Some(span) = layout::span_in_row(from, to, row, self.plan.metrics.char_width) {
                let rect = Rect::from_min_max(self.at(span.x_start, top), self.at(span.x_start + span.width, top + height));
                self.painter.rect_filled(rect, 0.0, color);
            }
        }
    }

    /// The site the hovered enzyme reads, washed in its colour on the bases it reads
    /// (`.seq-cut-site`: the cut-site wash, a 1px cut-site edge, corners of 2).
    fn cut_site(&self, row: Row, off: f64, bands: &layout::Bands) {
        let Some(HotCut { site: (from, to), .. }) = self.hot_cut else {
            return;
        };
        let (top, bottom) = (off + track::SPACER, off + bands.after_strands);
        for (from, to) in layout::unwrap(from, to, self.c.len() as i64) {
            if let Some(span) = layout::span_in_row(from, to, row, self.plan.metrics.char_width) {
                let rect = Rect::from_min_max(self.at(span.x_start, top), self.at(span.x_start + span.width, bottom));
                self.painter.rect(rect, 2.0, theme::CUT_SITE_WASH, Stroke::new(1.0, theme::CUT_SITE), egui::StrokeKind::Inside);
            }
        }
    }

    /// A made primer is pressed on its ribbon, but its card answers for the whole drawn box, the
    /// name beside it included, with a little give above and below (`PrimerCard.tsx` 47-66), and
    /// sits by that box. An oligo that is not in the file claims as any bar does.
    fn claim_primer(&self, ribbon: Rect, named: Rect, f: &Shown) {
        const SLOP: f32 = 4.0;
        if f.oligo.is_some() {
            self.claim(ribbon, f);
            return;
        }
        if let (Some(at), Ok(start), Ok(end)) = (self.press, usize::try_from(f.start), usize::try_from(f.end))
            && ribbon.contains(at)
        {
            self.on_bar.set(Some((f.index, start, end)));
        }
        if self.hover.is_some_and(|at| named.expand2(vec2(0.0, SLOP)).contains(at)) {
            self.seen.borrow_mut().bar = Some((f.index, named));
        }
    }

    /// The parts of the selection that fall in this row.
    fn selected(&self, row: Row) -> Vec<layout::Span> {
        let Some((start, end)) = self.cursor.selection else {
            return Vec::new();
        };
        layout::unwrap(start as i64, end as i64, self.c.len() as i64)
            .into_iter()
            .filter_map(|(from, to)| layout::span_in_row(from, to, row, self.plan.metrics.char_width))
            .collect()
    }

    /// Where a selection starts and ends, and the caret: one straight line the whole height of
    /// the row, through the bars above and below, so that where you are reads against a feature or
    /// a primer and not only against the letters. A selection that merely carries on into the next
    /// row has no edge there.
    fn edges(&self, row: Row, pieces: &[layout::Span]) {
        let bottom = self.height - track::ROW_GAP;
        for span in pieces {
            if !span.clipped_left {
                self.line((span.x_start, 0.0), (span.x_start, bottom), 1.0, theme::INK);
            }
            if !span.clipped_right {
                self.line((span.x_start + span.width, 0.0), (span.x_start + span.width, bottom), 1.0, theme::INK);
            }
        }
        let Some(caret) = self.cursor.caret.map(|caret| caret as i64).filter(|_| self.caret_on && self.cursor.selection.is_none()) else {
            return;
        };
        if caret >= row.start && caret <= row.end + 1 {
            let x = (caret - row.start) as f64 * self.plan.metrics.char_width;
            self.line((x, 0.0), (x, bottom), 1.5, theme::INK);
        }
    }

    /// The horizontal zebra: every other row on a faint band of the ink, so a row and everything
    /// stacked on it read as one block. Paper, not a highlight; drawn under everything.
    fn band(&self, row: Row) {
        if let Some((x, y, w, h)) = layout::band_for(row, self.height, &self.plan.metrics) {
            self.painter.rect_filled(Rect::from_min_max(self.at(x, y), self.at(x + w, y + h)), 0.0, theme::alpha(theme::INK, ROW_BAND));
        }
    }

    /// The scale, ranked by length the way a ruler is: a fifth stays inside the strands, a tenth
    /// pokes a little past them, a fiftieth runs the whole row and carries its coordinate.
    fn rules(&self, row: Row, top: f64, bottom: f64) {
        if bottom <= top {
            return;
        }
        let row_bottom = self.height - track::ROW_GAP;
        for rule in layout::rules_for(row, self.plan.metrics.char_width) {
            let fiftieth = rule.w > 1.0;
            let (y1, y2, strength) = if fiftieth {
                (0.0, row_bottom, RULE_FIFTIETH)
            } else if rule.k < 1.0 {
                (top, bottom, RULE_FIFTH)
            } else {
                ((top - REACH).max(0.0), (bottom + REACH).min(row_bottom), RULE_TENTH)
            };
            self.line((rule.x, y1), (rule.x, y2), rule.w as f32, theme::alpha(theme::INK, strength));
            if fiftieth {
                // The base the line stands before, 1-based, at the line's top right.
                self.text((rule.x + 3.0, -2.0), Align::Min, &(rule.bp + 1).to_string(), theme::mono_weight(9.5, Weight::Regular), theme::MUTED);
            }
        }
    }

    /// Both strands at one weight and colour, and the weld between them: one rung per base pair.
    fn strands(&self, row: Row, off: f64) {
        let cw = self.plan.metrics.char_width;
        let regime = layout::regime_for(cw);
        let bands = layout::bands_for(self.plan.tracks);
        let font = theme::mono_weight(13.0, Weight::Semibold);
        let rung = theme::alpha(theme::INK, RUNG);
        // In the case the construct holds them (`Strand` in SequenceRows.tsx prints `seq[bp]` as is).
        let letter = |bytes: &[u8], bp: i64| usize::try_from(bp).ok().and_then(|i| bytes.get(i)).map(|&b| char::from(b).to_string());
        for bp in row.start..=row.end {
            let cx = layout::center_of_base(bp, row, cw);
            if regime.bases {
                if let Some(base) = letter(self.c.sequence.as_bytes(), bp) {
                    self.text((cx, off + bands.forward), Align::Center, &base, font.clone(), base_color(self.base_style, &base));
                }
                if let Some(base) = letter(&self.plan.reverse, bp).filter(|_| self.plan.tracks.reverse_sequence) {
                    self.text((cx, off + bands.reverse), Align::Center, &base, font.clone(), base_color(self.base_style, &base));
                }
            }
            // Pointing at an enzyme breaks the pairing it destroys; a blunt cutter breaks none.
            let broken = self.hot_cut.and_then(|h| h.overhang).is_some_and(|(from, to)| (from..=to).contains(&bp));
            if regime.rungs && self.plan.tracks.reverse_sequence && !broken {
                self.line((cx, off + bands.rung_top), (cx, off + bands.rung_bottom), 1.0, rung);
            }
        }
    }

    /// Features as flat tinted bars: the feature's own colour thinned over the paper, the name
    /// inside when it fits, and a solid cap at the 5' end of a feature that reads one way.
    fn features(&self, list: &[Shown], kind: Kind, pieces: &RowPieces, top: f64) {
        let bar = annotations::bar_height(kind);
        for p in &pieces.pieces {
            let Some(f) = list.get(p.feature) else { continue };
            // Greyed back while the Interactions layer is asking about something else.
            let fade = |c: Color32| if self.dimmed(f.index) { theme::alpha(c, 0.2) } else { c };
            let y = top + p.y;
            if p.connector {
                let mid = y + bar / 2.0;
                let ends = [self.at(p.x_start, mid), self.at(p.x_start + p.width, mid)];
                self.painter.extend(Shape::dashed_line(&ends, Stroke::new(1.0, fade(f.color)), 4.0, 3.0));
                continue;
            }
            let (x, width) = annotations::bar_span(p);
            let rect = Rect::from_min_size(self.at(x, y), vec2(width as f32, bar as f32));
            self.claim(rect, f);
            self.painter.rect_filled(rect, 2.0, fade(theme::mix(f.color, theme::CELL, theme::TINT_ROW_BAR)));
            if p.show_head {
                let head = annotations::head_points(p, y, kind, f.forward).map(|(x, y)| self.at(x, y));
                self.painter.add(Shape::convex_polygon(head.to_vec(), fade(f.color), Stroke::new(1.0, fade(f.color))));
            }
            if annotations::name_fits(p, &f.name) {
                self.text(
                    (p.x_start + p.width / 2.0, y + bar - 4.0),
                    Align::Center,
                    &f.name,
                    theme::sans_weight(10.0, Weight::Medium),
                    fade(theme::INK),
                );
            }
        }
    }

    /// Open reading frames: derived, not declared, so drawn quieter than a feature somebody
    /// annotated, a faint fill under a dashed outline (`.seq-orf .seq-feature-bar`).
    fn orfs(&self, pieces: &RowPieces, top: f64) {
        let bar = annotations::bar_height(Kind::Orf);
        for p in &pieces.pieces {
            let Some(f) = self.plan.orfs.get(p.feature) else { continue };
            let y = top + p.y;
            let (x, width) = annotations::bar_span(p);
            let rect = Rect::from_min_size(self.at(x, y), vec2(width as f32, bar as f32));
            self.painter.rect_filled(rect, 2.0, theme::alpha(f.color, 0.18));
            let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
            self.painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, f.color), 2.0, 3.0));
            if p.show_head {
                let head = annotations::head_points(p, y, Kind::Orf, f.forward).map(|(x, y)| self.at(x, y));
                self.painter.add(Shape::convex_polygon(head.to_vec(), f.color, Stroke::new(1.0, f.color)));
            }
            if annotations::name_fits(p, &f.name) {
                self.text((p.x_start + p.width / 2.0, y + bar - 3.0), Align::Center, &f.name, theme::sans_weight(10.0, Weight::Medium), theme::INK);
            }
        }
    }

    /// The amino acids, one letter centred over each codon's middle base, in a calm green with the
    /// first in bold and a stop in scarlet (`.seq-aa`), standing on the track's baseline.
    fn residues(&self, residues: &[Residue], top: f64) {
        let baseline = top + track::TRANSLATIONS - 5.0;
        for r in residues {
            let (font, ink) = if r.stop {
                (theme::mono(11.0), theme::SANA_SCARLET)
            } else if r.start {
                (theme::mono_weight(11.0, Weight::Bold), AMINO_INK)
            } else {
                (theme::mono(11.0), AMINO_INK)
            };
            self.text((r.x, baseline), Align::Center, &r.aa.to_string(), font, ink);
        }
    }

    /// Primers as ribbons: one filled outline of what anneals, the 5' chevron cut into it and the
    /// 3' one pushed out of it, printing its own bases, with its name beside the 5' end.
    fn primers(&self, pieces: &RowPieces, top: f64, above: bool) {
        for p in &pieces.pieces {
            let Some(f) = self.plan.primers.get(p.feature) else { continue };
            let y = top + p.y;
            let ribbon = Rect::from_min_size(self.at(p.x_start, y), vec2(p.width as f32, annotations::bar_height(Kind::Primer) as f32));
            // What does not anneal hangs half a line off the ribbon, outward from the duplex.
            let half_y = if above { y - track::PRIMER_LIFT } else { y + track::PRIMER_LIFT };
            let ends = annotations::ends_of(p, &f.geometry());
            let cw = self.plan.metrics.char_width;
            let mut named = ribbon;
            if ends.five_here && !f.name.is_empty() {
                let five_at = p.five_at.unwrap_or(if f.forward { f.start } else { f.end });
                let edge = annotations::col_x(p, five_at, cw);
                let (x, anchor) = if above { (edge - annotations::NAME_GAP, Align::Max) } else { (edge + cw + annotations::NAME_GAP, Align::Min) };
                // Level with whatever the 5' end sits on: the tail, if there is one.
                let level = if p.cells.iter().any(|c| c.kind == CellKind::Tail) { half_y } else { y };
                let font = theme::sans_weight(10.0, Weight::Semibold);
                named = named.union(self.text((x, level + annotations::LETTER_BASELINE), anchor, &f.name, font, f.ribbon.ink));
            }
            self.claim_primer(ribbon, named, f);
            let chains = annotations::primer_chains(p, cw, y, half_y);
            for (i, segs) in chains.iter().enumerate() {
                let left = if i == 0 { ends.left } else { End::Flat };
                let right = if i + 1 == chains.len() { ends.right } else { End::Flat };
                self.ribbon(segs, left, right, &f.ribbon);
            }
            self.primer_handles(p, f, y);
            self.primer_breaks(p, y, half_y, above, &f.ribbon);
            self.primer_inserts(p, y, above, &f.ribbon);
            self.primer_letters(p, y, half_y, &f.ribbon);
        }
    }

    /// One chain of ribbon, filled and outlined. The fill is two bands meeting on the centre line,
    /// so a chevron cut into an end is filled as drawn; the outline goes over its edges.
    fn ribbon(&self, segs: &[Seg], left: End, right: End, tone: &theme::RibbonInk) {
        let h = annotations::PRIMER_BAR;
        let reach = |end: End| match end {
            End::Notch => -annotations::CHEV,
            End::Point => annotations::CHEV,
            End::Flat => 0.0,
        };
        let mut fill = Mesh::default();
        for (i, s) in segs.iter().enumerate() {
            let mid_left = if i == 0 { s.x0 - reach(left) } else { s.x0 };
            let mid_right = if i + 1 == segs.len() { s.x1 + reach(right) } else { s.x1 };
            let corners = [(s.x0, s.y), (s.x1, s.y), (mid_right, s.y + h / 2.0), (s.x1, s.y + h), (s.x0, s.y + h), (mid_left, s.y + h / 2.0)];
            let base = fill.vertices.len() as u32;
            for (x, y) in corners {
                fill.colored_vertex(self.at(x, y), tone.fill);
            }
            for [a, b, c] in [[0, 1, 2], [0, 2, 5], [5, 2, 3], [5, 3, 4]] {
                fill.add_triangle(base + a, base + b, base + c);
            }
        }
        self.painter.add(Shape::mesh(fill));
        let outline: Vec<Pos2> = annotations::chain_path(segs, h, left, right).into_iter().map(|(x, y)| self.at(x, y)).collect();
        self.painter.add(Shape::closed_line(outline, Stroke::new(1.5, tone.edge)));
    }

    /// Where a primer leaves its template: a deletion is one line across the gap, and a changed
    /// base hangs half a line off it on two legs, in the deeper shade.
    fn primer_breaks(&self, p: &Piece, y: f64, half_y: f64, above: bool, tone: &theme::RibbonInk) {
        let cw = self.plan.metrics.char_width;
        let h = annotations::PRIMER_BAR;
        let edge = Stroke::new(1.5, tone.edge);
        let (leg_from, leg_to) = if above { (y + h, half_y) } else { (y, half_y + h) };
        for run in annotations::runs_of(&p.cells) {
            let (x0, x1) = (annotations::col_x(p, run.from, cw), annotations::col_x(p, run.to, cw) + cw);
            match run.kind {
                CellKind::Gap => self.line((x0, y + h / 2.0), (x1, y + h / 2.0), 1.5, tone.edge),
                CellKind::Change => {
                    let rect = Rect::from_min_max(self.at(x0, half_y), self.at(x1, half_y + h));
                    self.painter.rect(rect, 0.0, tone.lift, edge, egui::StrokeKind::Middle);
                    self.line((x0, leg_from), (x0, leg_to), 1.5, tone.edge);
                    self.line((x1, leg_from), (x1, leg_to), 1.5, tone.edge);
                }
                // The run that does not anneal, laid over the outline in the deeper shade.
                CellKind::Tail => {
                    let rect = Rect::from_min_max(self.at(x0, half_y), self.at(x1, half_y + h));
                    self.painter.rect(rect, 0.0, tone.lift, edge, egui::StrokeKind::Middle);
                }
                CellKind::Paired => {}
            }
        }
    }

    /// A slim handle over each end of a made primer that lies in this piece: grab one and drag to
    /// lengthen or shorten it (`PrimerDrag.tsx`, `.primer-handle`). Not on a binder or a Find hit,
    /// which have no footprint of their own, nor on a primer through the origin.
    fn primer_handles(&self, p: &Piece, f: &Shown, y: f64) {
        if f.oligo.is_some() || f.end < f.start {
            return;
        }
        let cw = self.plan.metrics.char_width;
        for (bp, start) in [(f.start, true), (f.end, false)] {
            if bp < p.start || bp > p.end {
                continue;
            }
            let x = annotations::col_x(p, bp, cw) + if start { 0.0 } else { cw };
            let rect = Rect::from_min_max(self.at(x - 4.0, y), self.at(x + 4.0, y + annotations::PRIMER_BAR));
            let hovered = self.hover.is_some_and(|at| rect.contains(at));
            // `.primer-handle { transition: background 0.1s ease }`: the grip fades in and out.
            let ctx = self.painter.ctx();
            let lit =
                ctx.animate_bool_with_time_and_easing(egui::Id::unique(("primer-handle", f.index, start)), hovered, 0.1, crate::primitives::css_ease);
            if lit > 0.0 {
                self.painter.rect_filled(rect, 4.0, theme::alpha(theme::hex(0x6d3bd1), lit));
            }
            if hovered {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                self.seen.borrow_mut().handle = Some(rect);
            }
            if self.press.is_some_and(|at| rect.contains(at)) {
                self.on_end.set(Some((f.index, start)));
            }
        }
    }

    /// Bases an aligned oligo carries that the template has no column for: a whole line off,
    /// narrowing to the one junction they open, with a line through the ribbon there.
    fn primer_inserts(&self, p: &Piece, y: f64, above: bool, tone: &theme::RibbonInk) {
        let cw = self.plan.metrics.char_width;
        let h = annotations::PRIMER_BAR;
        let full_y = if above { y - track::PRIMERS } else { y + track::PRIMERS };
        let edge = Stroke::new(1.5, tone.edge);
        let print = layout::regime_for(cw).primer_bases;
        for ins in &p.inserts {
            let jx = annotations::col_x(p, ins.edge, cw);
            let label = annotations::insert_text(&ins.bases);
            let w = label.len() as f64 * cw;
            // The funnel starts a pixel inside the body, so no edge is drawn across the join.
            let near = if above { full_y + h - 1.0 } else { full_y + 1.0 };
            let apex = if above { y } else { y + h };
            let thru = if above { y + h } else { y };
            let funnel = vec![self.at(jx - w / 2.0, near), self.at(jx, apex), self.at(jx + w / 2.0, near)];
            self.painter.add(Shape::convex_polygon(funnel, tone.lift, edge));
            let rect = Rect::from_min_max(self.at(jx - w / 2.0, full_y), self.at(jx + w / 2.0, full_y + h));
            self.painter.rect(rect, 0.0, tone.lift, edge, egui::StrokeKind::Middle);
            self.line((jx, apex), (jx, thru), 1.5, tone.edge);
            if print {
                for (k, ch) in label.chars().enumerate() {
                    let x = jx - w / 2.0 + k as f64 * cw + cw / 2.0;
                    let font = theme::mono_weight(11.0, Weight::Semibold);
                    self.text((x, full_y + annotations::LETTER_BASELINE), Align::Center, &ch.to_string(), font, tone.lift_ink);
                }
            }
        }
    }

    fn primer_letters(&self, p: &Piece, y: f64, half_y: f64, tone: &theme::RibbonInk) {
        let cw = self.plan.metrics.char_width;
        if !layout::regime_for(cw).primer_bases {
            return;
        }
        for cell in &p.cells {
            let Some(ch) = cell.ch else { continue };
            let (top, font, color) = match cell.kind {
                CellKind::Paired => (y, theme::mono_weight(11.0, Weight::Medium), tone.ink),
                CellKind::Change | CellKind::Tail => (half_y, theme::mono_weight(11.0, Weight::Semibold), tone.lift_ink),
                CellKind::Gap => continue,
            };
            let x = annotations::col_x(p, cell.bp, cw) + cw / 2.0;
            self.text((x, top + annotations::LETTER_BASELINE), Align::Center, &ch.to_string(), font, color);
        }
    }

    /// The restriction cuts: a staircase across the two strands on a paper halo, so the
    /// across-segment sits over the rungs instead of fighting them, and the enzyme's name above.
    fn cuts(&self, row: Row, cuts: &[RowCut], off: f64) {
        let bands = layout::cut_bands_for(self.plan.tracks);
        for cut in cuts {
            let mark = cuts::staircase(cut, row, self.plan.metrics.char_width, bands, off);
            let points: Vec<Pos2> = mark.points.iter().map(|&(x, y)| self.at(x, y)).collect();
            self.hover_cut(cut.cut, &points, None);
            // Pointing at a cut is the whole interaction, so the staircase answers first
            // (`.is-picked .seq-cut-line`: 2.25px against 1.25).
            let hot = self.hot_cut.is_some_and(|h| h.index == cut.cut);
            self.painter.add(Shape::line(points.clone(), Stroke::new(3.75, theme::CELL)));
            self.painter.add(Shape::line(points.clone(), Stroke::new(if hot { 2.25 } else { 1.25 }, theme::CUT_SITE)));
            if let Some(site) = self.plan.sites.get(cut.cut).filter(|_| self.plan.cut_names) {
                // The hovered enzyme's name takes its colour (`.is-picked .seq-cut-name`).
                let ink = if hot { theme::CUT_SITE } else { theme::MUTED };
                let name = self.text((mark.anchor_x, cuts::name_baseline(cut.lane, off)), Align::Min, site.enzyme, theme::sans(11.0), ink);
                // The name answers as readily as the hairline (`CutHoverCard.tsx`).
                self.hover_cut(cut.cut, &points, Some(name));
            }
        }
    }
}

impl Sheet<'_> {
    /// Whether the pointer is on this cut: a box centred on the forward scission, the height of
    /// the staircase, with the web's slop round it. Where several match the nearest wins.
    fn hover_cut(&self, index: usize, points: &[Pos2], name: Option<Rect>) {
        let (Some(at), Some(first)) = (self.hover, points.first()) else {
            return;
        };
        let (top, bottom) = points.iter().fold((first.y, first.y), |(t, b), p| (t.min(p.y), b.max(p.y)));
        let hit = Rect::from_min_max(pos2(first.x - 1.0, top), pos2(first.x + 1.0, bottom));
        // The target the pointer is on: the hairline's box, or the name's.
        let target = name.unwrap_or(hit);
        if !target.expand2(vec2(CUT_SLOP_X, CUT_SLOP_Y)).contains(at) {
            return;
        }
        let distance = |r: Rect| (at.x - r.center().x).abs() + (at.y - r.center().y).abs();
        let mut seen = self.seen.borrow_mut();
        // Nearest target wins; the card stays anchored on the hairline, centred on the scission.
        if seen.cut_distance.is_none_or(|was| distance(target) < was) {
            seen.cut = Some((index, hit));
            seen.cut_distance = Some(distance(target));
        }
    }
}

/// Whether a point is on what the rows show and not on something floating over them.
fn over_rows(ui: &egui::Ui, at: Pos2) -> bool {
    ui.clip_rect().contains(at) && ui.ctx().layer_id_at(at).is_none_or(|layer| layer == ui.layer_id())
}

#[cfg(test)]
mod tests {
    use bios_core::genbank;

    use super::{Found, Oligo, anchor_id, humanize_name, is_directional, is_hidden, is_primer, primer_bases, shown_found, zoom_id};

    #[test]
    fn what_exact_found_rides_the_primer_lane_in_its_own_colours_and_reads_5_to_3() {
        let c = genbank::read(include_str!("../../bios-core/tests/fixtures/pUC19.gb")).unwrap();
        let top = c.sequence.get(1500..1525).unwrap().to_string();
        let forward = shown_found(0, &Found { start: 1500, end: 1524, forward: true }, &c);
        assert_eq!(forward.bases.as_deref(), Some(top.as_str()));
        assert_eq!(forward.ribbon, crate::theme::FOUND_RIBBON);
        assert_eq!(forward.oligo, Some(Oligo::Found(0)));
        let minus = shown_found(1, &Found { start: 1500, end: 1524, forward: false }, &c);
        assert_eq!(minus.bases, Some(bios_core::seq::revcomp(&top)), "a minus-strand hit carries its own bases, 5' to 3'");
        assert!(!minus.forward && minus.directional);
    }

    #[test]
    fn each_plasmid_keeps_its_own_zoom() {
        // One shared key made zooming one plasmid zoom every other the same way.
        assert_ne!(zoom_id("pUC19"), zoom_id("pBR322"));
        assert_ne!(anchor_id("pUC19"), anchor_id("pBR322"));
        assert_eq!(zoom_id("pUC19"), zoom_id("pUC19"));
    }

    #[test]
    fn a_name_reads_with_spaces_where_the_file_has_underscores() {
        assert_eq!(humanize_name("CMV_enhancer"), "CMV enhancer");
        assert_eq!(humanize_name("  T7__promoter \t x "), "T7 promoter x");
        assert_eq!(humanize_name(""), "");
    }

    #[test]
    fn direction_is_marked_only_where_it_means_something() {
        assert!(is_directional("CDS", "AmpR"));
        assert!(is_directional("rep_origin", "SV40 ori"));
        assert!(!is_directional("enhancer", "CMV enhancer"), "an enhancer works in either orientation");
        assert!(!is_directional("protein_bind", "lac operator promoter"), "a named kind stays what it is");
        assert!(is_directional("misc_feature", "SV40 poly A signal"));
        assert!(is_directional("misc_feature", "WPRE optional"));
        assert!(!is_directional("misc_feature", "gibson homology"));
        assert!(!is_directional("misc_feature", "stopper"), "a whole word, not a prefix");
    }

    const FILE: &str = "LOCUS       t                         40 bp    DNA     linear   SYN 01-JAN-2026\n\
FEATURES             Location/Qualifiers\n     source          1..40\n     misc_feature    3..8\n                     /label=\"ESE_ESRseq_SM\"\n     \
primer_bind     5..14\n                     /label=\"fwd\"\n     primer_bind     complement(21..30)\n                     /label=\"rev\"\n     \
primer_bind     5..14\n                     /label=\"stored\"\n                     /sequence=\"ggaat cgatt\"\n     primer_bind     5..14\n                     \
/label=\"tailed\"\n                     /sequence=\"TTTTGGATCCGATC\"\nORIGIN\n        1 acgtggatcc gatcaaaaac ccccgggggt tttttacgta\n//\n";

    #[test]
    fn a_primer_prints_the_oligo_stored_on_it_or_else_what_its_footprint_reads() {
        let c = genbank::read(FILE).unwrap();
        let by_name = |name: &str| c.features.iter().find(|f| f.name == name).unwrap();
        assert!(is_hidden(by_name("ESE_ESRseq_SM")), "a splice scan's hit is kept out of the rows");
        assert!(is_primer(by_name("fwd")) && !is_hidden(by_name("fwd")));
        assert_eq!(primer_bases(by_name("fwd"), &c.sequence).as_deref(), Some("GGATCCGATC"));
        assert_eq!(primer_bases(by_name("rev"), &c.sequence).as_deref(), Some("ACCCCCGGGG"), "5' to 3' along the bottom strand");
        assert_eq!(primer_bases(by_name("stored"), &c.sequence).as_deref(), Some("GGAATCGATT"), "column for column, so the changes show");
        assert_eq!(primer_bases(by_name("tailed"), &c.sequence).as_deref(), Some("GGATCCGATC"), "a tail has no column until the alignment is ported");
    }
}
