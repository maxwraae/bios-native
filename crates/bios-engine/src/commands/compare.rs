//! Comparing one whole plasmid with another: are they the same, and if not, what changed?
//!
//! The alignment maths is [`bios_core::compare`]; this is the reporting. What it adds is what only
//! a project knows: which plasmid the caller meant, which feature each difference falls in on
//! either map, what a change does to a protein, and which features one map carries and the other
//! does not. The list of differences is capped, because an answer an agent cannot read is no
//! answer; the count never is.

use bios_core::compare::{
    Comparison, Diff, DiffKind, Place, ProteinEffect, Unalignable, compare, describe_effect, feature_at, indel_effect, substitution_effect,
};
use bios_core::seq::revcomp;
use bios_core::verify::grouped;
use bios_core::{Construct, Feature, Strand};
use bios_project::Loaded;
use serde_json::{Value, json};

use super::PLASMID;
use super::sequence::bases;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

/// Differences listed in one reply unless the caller asks for more. The total is always reported.
const SHOWN: usize = 100;
/// Most a caller may ask for.
const MOST_SHOWN: usize = 1_000;
/// Features named in each "only in this one" list.
const FEATURES_SHOWN: usize = 50;
/// Bases of a difference written out in full before they are abbreviated.
const BASES_SHOWN: usize = 60;

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "compare_plasmids",
    description: "Compare two whole plasmids and say what changed. This is the tool for \"are these the same, and if not what is different\": a map against a corrected map, a clone against its parent, a vendor's sequence against the one you designed. Do not align two plasmids yourself.\n\nThe two are aligned END TO END, not piece by piece. For two circular plasmids the turn (and, if needed, the strand) that lines them up is found first and reported, so a plasmid whose origin was moved reads as the same plasmid instead of a completely different one. The reply gives the identity, the aligned length, and every difference — substitution, insertion, deletion — each with its 1-based position on BOTH plasmids, the bases on each side, the feature it falls in, and, for a change in a CDS, what it does to the protein (the codon, the residue, and whether it is silent). It also lists the features one plasmid has and the other has not, since two maps of the same sequence often differ only in annotation. It says plainly when the two are identical, and when they are too different to line up at all, instead of answering with noise.\n\nPositions are 1-based on each plasmid's own bases; a difference with no bases of its own on one side says which base it comes after there (0 = before base 1). If `to` had to be read on its reverse strand to line up, its side of every difference says \"strand\": \"reverse\" and its bases are the ones on that strand, which are the complement of what its file stores.",
    params: &[
        PLASMID,
        Param {
            name: "to",
            kind: Kind::String,
            required: false,
            description: "The other plasmid, named the same way (a path, a name or an id). In the same project.",
        },
        Param {
            name: "sequence",
            kind: Kind::String,
            required: false,
            description: "Or the other sequence's bases directly (FASTA accepted), for comparing against something pasted in. Read as circular when `plasmid` is circular, so a moved origin is still found.",
        },
        Param {
            name: "max_differences",
            kind: Kind::Integer,
            required: false,
            description: "How many differences to list (default 100, at most 1000). The count and the tallies are always complete.",
        },
    ],
    effect: Effect::Read,
    run: Run::Call(compare_plasmids),
}];

/// One side of the comparison: the construct and what to call it.
struct Side<'a> {
    label: String,
    construct: &'a Construct,
}

impl Side<'_> {
    fn name(&self) -> &str {
        &self.label
    }
}

/// The other construct a call names: a plasmid in the same project, or bases given directly.
fn other(session: &Session, args: &Args, a: &Construct) -> Result<(Construct, String), Failure> {
    match (args.opt_str("to")?, args.opt_str("sequence")?) {
        (Some(_), Some(_)) => fail("give either `to` (another plasmid) or `sequence` (bases), not both"),
        (Some(reference), None) => {
            let listed = session.project().list()?;
            let id = refs::plasmid(&listed, &reference)?.id.clone();
            let loaded: Loaded = session.project().read(&id)?;
            let label = refs::label(&loaded.path);
            Ok((loaded.construct, label))
        }
        (None, Some(text)) => {
            let seq = bases(&text)?;
            let construct =
                Construct { name: "the sequence given".into(), sequence: seq, circular: a.circular, features: Vec::new(), ..Construct::default() };
            Ok((construct, "the sequence given".to_string()))
        }
        (None, None) => fail("compare_plasmids needs `to` (the other plasmid) or `sequence` (its bases)"),
    }
}

fn compare_plasmids(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let a = &loaded.construct;
    let (b_construct, b_label) = other(session, args, a)?;
    let shown = match args.opt_int("max_differences")?.unwrap_or(SHOWN as i64) {
        n if (1..=MOST_SHOWN as i64).contains(&n) => n as usize,
        _ => return fail(format!("max_differences must be between 1 and {MOST_SHOWN}")),
    };
    let left = Side { label: refs::label(&loaded.path), construct: a };
    let right = Side { label: b_label, construct: &b_construct };
    let head = json!({
        "plasmid": left.label,
        "to": right.label,
        "lengths": { "plasmid": left.construct.len(), "to": right.construct.len() },
        "topology": { "plasmid": topology(left.construct), "to": topology(right.construct) },
    });

    match compare(&left.construct.sequence, left.construct.circular, &right.construct.sequence, right.construct.circular) {
        Err(failed) => Ok(merge(head, unalignable(&left, &right, &failed))),
        Ok(found) => Ok(merge(head, report(&left, &right, &found, shown))),
    }
}

fn topology(c: &Construct) -> &'static str {
    if c.circular { "circular" } else { "linear" }
}

fn merge(mut head: Value, rest: Value) -> Value {
    if let (Some(into), Some(from)) = (head.as_object_mut(), rest.as_object()) {
        for (key, value) in from {
            into.insert(key.clone(), value.clone());
        }
    }
    head
}

fn unalignable(left: &Side, right: &Side, failed: &Unalignable) -> Value {
    json!({
        "comparable": false,
        "identical": false,
        "shared_exact_bases": failed.shared,
        "longest_shared_stretch": failed.longest,
        "summary": format!("{} and {}: {}.", left.name(), right.name(), failed.reason),
    })
}

/// The whole answer for a pair that does line up.
fn report(left: &Side, right: &Side, found: &Comparison, shown: usize) -> Value {
    let listed: Vec<Value> = found.diffs.iter().take(shown).map(|d| difference(left, right, found, d)).collect();
    let (features, only) = features(left, right, found);
    let total = found.diffs.len();
    json!({
        "comparable": true,
        "identical": found.identical(),
        "alignment": {
            "strand": if found.flipped { "reverse" } else { "forward" },
            "aligned_length": found.aligned_columns,
            "identity_percent": (found.identity() * 1000.0).floor() / 10.0,
            "matching_bases": found.matches,
            "lines_up_at": lines_up(left, right, found),
            "to_base_at_plasmid_base_1": found.b_origin + 1,
        },
        "differences": {
            "total": total,
            "more_than_counted": found.more_diffs,
            "substitutions": found.count(DiffKind::Substitution),
            "insertions": found.count(DiffKind::Insertion),
            "deletions": found.count(DiffKind::Deletion),
            "shown": listed.len(),
            "not_shown": total.saturating_sub(listed.len()),
            "list": listed,
        },
        "features": features,
        "summary": summary(left, right, found, only),
    })
}

/// How the two were put against each other, in words.
fn lines_up(left: &Side, right: &Side, found: &Comparison) -> String {
    let strand = if found.flipped { " (read on its reverse strand)" } else { "" };
    format!("base {} of {}{strand} lines up with base 1 of {}", grouped(found.b_origin as i64 + 1), right.name(), left.name())
}

// ---------------------------------------------------------------------------------------------
// One difference
// ---------------------------------------------------------------------------------------------

fn kind_word(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Substitution => "substitution",
        DiffKind::Insertion => "insertion",
        DiffKind::Deletion => "deletion",
    }
}

/// Bases as they are written out: in full while they can be read, the ends and the size past that.
fn written(bases: &str) -> String {
    if bases.len() <= BASES_SHOWN {
        return bases.to_string();
    }
    let (head, tail) = (bases.get(..24).unwrap_or_default(), bases.get(bases.len() - 24..).unwrap_or_default());
    format!("{head}…{tail} ({} bp)", bases.len())
}

/// The 0-based base of one side a difference is read against: the changed bases' first base, or
/// the base the change follows when that side has none of its own.
fn anchor(place: Place) -> Option<usize> {
    match place {
        Place::At { start, .. } => Some(start),
        Place::After(at) => Some(at),
        Place::Start => None,
    }
}

/// `lacZ alpha (CDS)`, the narrowest feature over the place the difference falls.
fn feature_note(c: &Construct, at: usize) -> Option<(usize, String)> {
    let i = feature_at(&c.features, c.len(), at)?;
    let f = c.features.get(i)?;
    Some((i, format!("{} ({})", f.name, f.kind)))
}

fn place_json(place: Place, bases: &str, feature: Option<&str>, reverse: bool) -> Value {
    let mut out = match place {
        Place::At { start, end } => json!({ "start": start + 1, "end": end + 1 }),
        Place::After(at) => json!({ "after": at + 1 }),
        Place::Start => json!({ "after": 0 }),
    };
    if let Some(map) = out.as_object_mut() {
        if !bases.is_empty() {
            map.insert("bases".into(), json!(written(bases)));
        }
        if let Some(name) = feature {
            map.insert("feature".into(), json!(name));
        }
        if reverse {
            map.insert("strand".into(), json!("reverse"));
        }
    }
    out
}

/// A difference's span in `b`'s own numbering, lowest base first: `None` when it crosses `b`'s
/// origin, where there is no one span to name.
///
/// The bases stay as the comparison read them, NOT as `b` stores them. For a plasmid stored the
/// other way round that is the complement of what is in the file, and it has to be: the point of
/// the pair is that these two bases sit in the same column, and complementing one of them would
/// make every matching base look like a mismatch. The side says `strand: reverse` so a reader
/// knows which strand they are on.
fn b_span(found: &Comparison, d: &Diff) -> Option<(usize, usize)> {
    let Place::At { start, end } = d.at_b else { return None };
    let wrapped = if found.flipped { start < end } else { start > end };
    if wrapped && start != end {
        return None;
    }
    Some(if start <= end { (start, end) } else { (end, start) })
}

/// `a`'s bases as `b`'s own plus strand would carry them: what a protein on `b` would be read
/// with if the change were made there.
fn as_b_reads(found: &Comparison, d: &Diff) -> String {
    if found.flipped { revcomp(&d.a_bases) } else { d.a_bases.clone() }
}

/// What the difference does to a protein, on whichever map has a coding feature over it. The
/// codons read from the plasmid the feature belongs to towards the other one, so the plasmid is
/// named in the note.
fn protein(left: &Side, right: &Side, found: &Comparison, d: &Diff) -> Option<String> {
    if let Some(at) = anchor(d.at_a)
        && let Some((i, name)) = feature_note(left.construct, at)
        && let Some(f) = left.construct.features.get(i)
        && let Some(effect) = effect_of(left.construct, f, at, d, &d.b_bases, true)
    {
        return Some(format!("{name} on {}: {}", left.name(), describe_effect(&effect)));
    }
    // An insertion or a deletion names one base on `b`; that is enough to place it.
    let at = b_span(found, d).map(|(lo, _)| lo).or_else(|| anchor(d.at_b))?;
    let (i, name) = feature_note(right.construct, at)?;
    let f = right.construct.features.get(i)?;
    let effect = effect_of(right.construct, f, at, d, &as_b_reads(found, d), false)?;
    Some(format!("{name} on {}: {}", right.name(), describe_effect(&effect)))
}

/// `on_a` says whose feature this is, which is what decides whether bases were added or removed:
/// an insertion is bases the second plasmid has, so it reads as added to the first one's protein
/// and as removed from the second one's.
fn effect_of(c: &Construct, f: &Feature, at: usize, d: &Diff, replacement: &str, on_a: bool) -> Option<ProteinEffect> {
    match d.kind {
        DiffKind::Substitution => substitution_effect(c, f, at, replacement),
        DiffKind::Insertion => indel_effect(c, f, at, d.length(), on_a),
        DiffKind::Deletion => indel_effect(c, f, at, d.length(), !on_a),
    }
}

fn difference(left: &Side, right: &Side, found: &Comparison, d: &Diff) -> Value {
    let a_feature = anchor(d.at_a).and_then(|at| feature_note(left.construct, at)).map(|(_, name)| name);
    let span = b_span(found, d);
    let b_feature = span.map(|(lo, _)| lo).or_else(|| anchor(d.at_b)).and_then(|at| feature_note(right.construct, at)).map(|(_, name)| name);
    // The span is named lowest base first, in `b`'s own numbering.
    let place = match span {
        Some((lo, hi)) => Place::At { start: lo, end: hi },
        None => d.at_b,
    };
    let mut out = json!({
        "kind": kind_word(d.kind),
        "length": d.length(),
        "plasmid": place_json(d.at_a, &d.a_bases, a_feature.as_deref(), false),
        "to": place_json(place, &d.b_bases, b_feature.as_deref(), found.flipped),
    });
    if let (Some(note), Some(map)) = (protein(left, right, found, d), out.as_object_mut()) {
        map.insert("protein".into(), json!(note));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Features one map has and the other has not
// ---------------------------------------------------------------------------------------------

/// Whether `other` annotates the same thing: a feature of the same name over the bases this one's
/// ends line up with. Two maps of one sequence usually differ only here.
fn annotated_in(other: &Construct, name: &str, places: &[usize]) -> bool {
    other
        .features
        .iter()
        .filter(|g| g.name.trim().eq_ignore_ascii_case(name.trim()))
        .any(|g| places.iter().any(|&at| bios_core::compare::covers(g, at)))
}

/// What each map annotates and the other does not. A side with no features at all is not
/// compared: every feature of the other one would be "only there", which says nothing about the
/// two plasmids and buries the differences that do matter.
fn features(left: &Side, right: &Side, found: &Comparison) -> (Value, Option<(usize, usize)>) {
    let named = |side: &Side| side.construct.features.iter().any(|f| !f.name.trim().is_empty());
    if !named(left) || !named(right) {
        let bare = if named(left) { right.name() } else { left.name() };
        return (json!({ "not_compared": format!("{bare} carries no features") }), None);
    }
    let (only_left, left_total) = only_in(left, right, found, true);
    let (only_right, right_total) = only_in(right, left, found, false);
    let out = json!({
        "only_in_plasmid": only_left,
        "only_in_plasmid_total": left_total,
        "only_in_to": only_right,
        "only_in_to_total": right_total,
    });
    (out, Some((left_total, right_total)))
}

/// The features of `from` that `to` does not have, and how many there are in all. `forward` says
/// which way the alignment carries a position.
fn only_in(from: &Side, to: &Side, found: &Comparison, forward: bool) -> (Vec<Value>, usize) {
    let carry = |at: usize| if forward { found.b_of_a(at) } else { found.a_of_b(at) };
    let mut out = Vec::new();
    let mut total = 0;
    for (i, f) in from.construct.features.iter().enumerate() {
        if f.name.trim().is_empty() {
            continue;
        }
        let places: Vec<usize> = [f.start, f.end].iter().filter_map(|&at| carry(at)).collect();
        if !places.is_empty() && annotated_in(to.construct, &f.name, &places) {
            continue;
        }
        total += 1;
        if out.len() < FEATURES_SHOWN {
            out.push(json!({
                "index": i + 1,
                "name": f.name,
                "type": f.kind,
                "start": f.start + 1,
                "end": f.end + 1,
                "strand": if f.strand == Strand::Reverse { "reverse" } else { "forward" },
            }));
        }
    }
    (out, total)
}

// ---------------------------------------------------------------------------------------------
// The sentence
// ---------------------------------------------------------------------------------------------

fn tally(found: &Comparison) -> String {
    let mut parts = Vec::new();
    for (kind, word) in [(DiffKind::Substitution, "substitution"), (DiffKind::Insertion, "insertion"), (DiffKind::Deletion, "deletion")] {
        let n = found.count(kind);
        if n > 0 {
            parts.push(format!("{n} {word}{}", if n == 1 { "" } else { "s" }));
        }
    }
    parts.join(", ")
}

/// `None` where the features were not compared at all, which says nothing either way.
fn annotation_note(left: &Side, right: &Side, only: Option<(usize, usize)>) -> String {
    match only {
        None | Some((0, 0)) => String::new(),
        Some((n, 0)) => format!(" {n} feature{} only on {}.", if n == 1 { "" } else { "s" }, left.name()),
        Some((0, n)) => format!(" {n} feature{} only on {}.", if n == 1 { "" } else { "s" }, right.name()),
        Some((a, b)) => format!(" {a} feature{} only on {}, {b} only on {}.", if a == 1 { "" } else { "s" }, left.name(), right.name()),
    }
}

fn turn_note(left: &Side, right: &Side, found: &Comparison) -> String {
    match (found.rotated, found.flipped) {
        (false, false) => String::new(),
        (true, false) => {
            format!(" Its origin is moved: base {} of {} is base 1 of {}.", grouped(found.b_origin as i64 + 1), right.name(), left.name())
        }
        (false, true) => format!(" It is stored the other way round: {} reads on the reverse strand of {}.", left.name(), right.name()),
        (true, true) => format!(
            " It is stored the other way round and turned: base {} of {}, on its reverse strand, is base 1 of {}.",
            grouped(found.b_origin as i64 + 1),
            right.name(),
            left.name()
        ),
    }
}

fn summary(left: &Side, right: &Side, found: &Comparison, only: Option<(usize, usize)>) -> String {
    let annotation = annotation_note(left, right, only);
    let turn = turn_note(left, right, found);
    if found.identical() {
        let same = format!(
            "{} and {} are the same {} bp {} plasmid, base for base.",
            left.name(),
            right.name(),
            grouped(found.a_length as i64),
            topology(left.construct)
        );
        // "Their features agree" is only sayable where both sides have features to compare.
        let agree = if only == Some((0, 0)) { " Their features agree too." } else { "" };
        return format!("{same}{turn}{annotation}{agree}");
    }
    let more = if found.more_diffs { format!(" (the first {} are listed)", found.diffs.len()) } else { String::new() };
    format!(
        "{} and {} are {:.1}% identical over {} aligned bases: {}{more}.{turn}{annotation}",
        left.name(),
        right.name(),
        (found.identity() * 1000.0).floor() / 10.0,
        grouped(found.aligned_columns as i64),
        tally(found),
    )
}
