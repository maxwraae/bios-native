//! Carrying features from one construct to another: with pasted bases, or wherever their bases
//! already occur.

use std::collections::BTreeMap;
use std::path::Path;

use bios_core::locations::normalize_location;
use bios_core::search::{SearchOptions, find_binding_sites};
use bios_core::{Construct, Feature, Segment, Strand, edits, seq};
use bios_project::Listed;
use serde_json::{Map, Value, json};

use super::sequence::{in_range, new_bios_id, slice, span_len};
use super::structure::flip;
use super::{PLASMID, VERSION};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs, token};

const MISMATCHES: Param = Param { name: "max_mismatches", kind: Kind::Integer, required: false, description: "Default 0 (exact). At most 5." };

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "paste_region",
        description: "Copy bases from_start..from_end of a plasmid and paste them into this plasmid after base `position` (0 = before base 1), WITH the features that lie fully inside that range. reverse_complement pastes the flipped region, its features flipping with it. Features of the target after the point shift; one spanning the point grows (reported). Pasted features get new bios ids. The source may be the same plasmid.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "position", kind: Kind::Integer, required: true, description: "Paste after this base of the target; 0 = before base 1." },
            Param { name: "from_plasmid", kind: Kind::String, required: true, description: "The plasmid to copy from." },
            Param { name: "from_start", kind: Kind::Integer, required: true, description: "First base to copy." },
            Param { name: "from_end", kind: Kind::Integer, required: true, description: "Last base to copy (start > end runs through the origin)." },
            Param { name: "reverse_complement", kind: Kind::Boolean, required: false, description: "Paste the reverse complement." },
        ],
        effect: Effect::Write,
        run: Run::Edit(paste_region),
    },
    CommandSpec {
        name: "copy_features",
        description: "Copy features from one plasmid onto another wherever their bases occur (both strands, through the origin; with mismatches if asked). Name, type and qualifiers come along; a feature matched on the other strand flips. Reports features with no match or several matches (added at each). A feature already annotated at the same place under the same name is not added again.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "from", kind: Kind::String, required: true, description: "The plasmid to copy features from." },
            Param {
                name: "features",
                kind: Kind::Strings,
                required: true,
                description: "Feature names, #index or bios ids in the source; [\"all\"] = every feature.",
            },
            MISMATCHES,
        ],
        effect: Effect::Write,
        run: Run::Edit(copy_features),
    },
    CommandSpec {
        name: "auto_annotate",
        description: "Detect known features in a plasmid: every feature of the source plasmids whose bases occur in this plasmid (both strands, through the origin) is proposed with its name, type and where it came from. dry_run (default true) only lists them; dry_run:false adds them (never duplicating a feature already on the same bases) and needs version. Show the person the list before adding many.",
        params: &[
            PLASMID,
            Param { name: "version", kind: Kind::String, required: false, description: "Needed with dry_run:false." },
            Param {
                name: "sources",
                kind: Kind::Strings,
                required: true,
                description: "Where to take known features from: plasmid paths, folder names, or \"all\" for the whole project.",
            },
            Param {
                name: "min_length",
                kind: Kind::Integer,
                required: false,
                description: "Ignore source features shorter than this (default 15, at least 6).",
            },
            MISMATCHES,
            Param { name: "include_primers", kind: Kind::Boolean, required: false, description: "Also propose primers (default false)." },
            Param { name: "dry_run", kind: Kind::Boolean, required: false, description: "Default true: only list. false: add them." },
        ],
        effect: Effect::Write,
        run: Run::Call(auto_annotate),
    },
];

fn mismatches(args: &Args) -> Result<usize, Failure> {
    match args.opt_int("max_mismatches")?.unwrap_or(0) {
        n @ 0..=5 => Ok(n as usize),
        _ => fail("max_mismatches must be between 0 and 5"),
    }
}

/// The construct `reference` names, or `this` as it stands when it names the one being edited.
fn other(session: &Session, args: &Args, reference: &str, this: &Construct) -> Result<Construct, Failure> {
    let listed = session.project().list()?;
    let source = refs::plasmid(&listed, reference)?;
    if refs::plasmid(&listed, &args.str("plasmid")?).is_ok_and(|target| target.id == source.id) {
        return Ok(this.clone());
    }
    Ok(session.project().read(&source.id)?.construct)
}

/// A copy of `original` on `start..=end` (0-based) of `c`, under its own bios id. The copy goes
/// where its start puts it among the features, as they are listed.
fn graft(c: &mut Construct, original: &Feature, segments: Vec<Segment>, strand: Strand) -> usize {
    let mut copy = original.clone();
    copy.start = segments.first().map_or(0, |s| s.start);
    copy.end = segments.last().map_or(0, |s| s.end);
    copy.segments = segments;
    copy.strand = strand;
    // The copy is its own feature. Links from it to other features named ids that mean nothing
    // in the plasmid it now lives in, so they do not come along.
    for (key, values) in &mut copy.notes {
        if key == "note" {
            values.retain(|n| !n.starts_with("bios-id:") && !n.starts_with("bios-act:"));
        }
    }
    copy.notes.retain(|(_, values)| !values.is_empty());
    match copy.notes.iter_mut().find(|(k, _)| k == "note") {
        Some((_, values)) => values.push(new_bios_id()),
        None => copy.notes.push(("note".into(), vec![new_bios_id()])),
    }
    normalize_location(&mut copy, c.len(), c.circular);
    let at = c.features.iter().position(|f| f.start > copy.start).unwrap_or(c.features.len());
    c.features.insert(at, copy);
    at
}

/// A single-piece feature already on exactly these bases and strand (and under this name, if given).
fn annotated_at<'a>(c: &'a Construct, start: usize, end: usize, strand: Strand, name: Option<&str>) -> Option<&'a Feature> {
    c.features.iter().find(|f| {
        f.locations().is_none() && f.start == start && f.end == end && f.strand == strand && name.is_none_or(|n| f.name.eq_ignore_ascii_case(n))
    })
}

fn opposite(strand: Strand) -> Strand {
    if strand == Strand::Forward { Strand::Reverse } else { Strand::Forward }
}

fn paste_region(session: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let position = args.int("position")?;
    if position < 0 || position > c.len() as i64 {
        return fail(format!("position must be within 0..{}", c.len()));
    }
    let source = other(session, args, &args.str("from_plasmid")?, c)?;
    let (start, end) = (args.int("from_start")?, args.int("from_end")?);
    in_range(&source, start, end).map_err(|e| Failure(format!("source: {}", e.0)))?;
    let reverse = args.opt_bool("reverse_complement")? == Some(true);
    let (length, size) = (source.len(), span_len(&source, start, end) as usize);
    let copied = slice(&source, start, end);
    let pasted = if reverse { seq::revcomp(&copied) } else { copied };

    let offset = |x: usize| (x + length - (start - 1) as usize) % length;
    let (mut carried, mut notes_after) = (Vec::new(), Vec::new());
    for f in source.features.iter().filter(|f| f.location_unsupported.is_none()) {
        let pieces: Vec<Segment> = f.segments.iter().map(|s| Segment { start: offset(s.start), end: offset(s.end), ..*s }).collect();
        let inside: Vec<bool> = pieces.iter().map(|s| s.start <= s.end && s.end < size).collect();
        if inside.iter().all(|i| *i) && f.span(length) <= size {
            carried.push((f, pieces));
        } else if pieces.iter().zip(&inside).any(|(s, i)| *i || s.start > s.end || s.start < size) {
            notes_after.push(format!("\"{}\" is only partly inside the copied bases, so it was not copied", f.name));
        }
    }

    let result = edits::apply_insert(&c.sequence, &c.features, position + 1, &pasted, c.circular).map_err(|e| Failure(e.to_string()))?;
    c.sequence = result.sequence;
    c.features = result.features;
    let mut notes = Vec::new();
    if !result.delta.shifted.is_empty() {
        notes.push(format!("{} features of the target moved or grew with their bases", result.delta.shifted.len()));
    }

    let at = position as usize;
    let mut placed = Vec::new();
    for (f, pieces) in carried {
        let (pieces, strand) = if reverse {
            let mirrored = pieces.iter().rev().map(|s| Segment {
                start: size - 1 - s.end,
                end: size - 1 - s.start,
                start_fuzzy: flip(s.end_fuzzy),
                end_fuzzy: flip(s.start_fuzzy),
            });
            (mirrored.collect::<Vec<_>>(), opposite(f.strand))
        } else {
            (pieces, f.strand)
        };
        let pieces = pieces.into_iter().map(|s| Segment { start: s.start + at, end: s.end + at, ..s }).collect();
        placed.push(graft(c, f, pieces, strand));
    }
    // Indexes move as later features are inserted before earlier ones; name them at the end.
    let names: Vec<String> = placed.iter().filter_map(|i| c.features.get(*i)).map(|f| f.name.clone()).collect();
    notes.extend(notes_after);
    let mut result = Map::new();
    result.insert("pasted_bases".into(), json!(size));
    result.insert("pasted_features".into(), json!(names));
    Ok(Applied { summary: format!("Pasted {size} bases after base {position}, with {} features", names.len()), notes, result })
}

fn copy_features(session: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let source = other(session, args, &args.str("from")?, c)?;
    let max_mismatches = mismatches(args)?;
    let wanted = args.strings("features");
    let mut report = Vec::new();
    let picked: Vec<&Feature> = if wanted.iter().any(|w| w.trim().eq_ignore_ascii_case("all")) {
        source.features.iter().filter(|f| f.kind != "source").collect()
    } else {
        let mut picked = Vec::new();
        for reference in &wanted {
            match refs::feature(&source, reference) {
                Ok(index) => picked.extend(source.features.get(index)),
                Err(_) => report.push(format!("\"{}\" isn't a feature of the source plasmid, or names more than one", reference.trim())),
            }
        }
        picked
    };

    let mut added = Vec::new();
    for f in picked {
        if f.locations().is_some() || f.location_unsupported.is_some() {
            report.push(format!("\"{}\" has several pieces, so it can't be found by its sequence; add it by hand", f.name));
            continue;
        }
        let top = slice(&source, f.start as i64 + 1, f.end as i64 + 1);
        let hits = find_binding_sites(&c.sequence, &top, SearchOptions { max_mismatches, circular: c.circular });
        if hits.is_empty() {
            report.push(format!("\"{}\": no match in the target", f.name));
            continue;
        }
        if hits.len() > 1 {
            let places: Vec<String> = hits.iter().map(|h| format!("{}..{}", h.start + 1, h.end + 1)).collect();
            report.push(format!("\"{}\" matches {} places: {}", f.name, hits.len(), places.join(", ")));
        }
        for hit in hits {
            let strand = if hit.forward { f.strand } else { opposite(f.strand) };
            if annotated_at(c, hit.start, hit.end, strand, Some(&f.name)).is_some() {
                report.push(format!("\"{}\" is already annotated at {}..{}; not added again", f.name, hit.start + 1, hit.end + 1));
                continue;
            }
            let (start_fuzzy, end_fuzzy) = if hit.forward { (f.start_fuzzy, f.end_fuzzy) } else { (flip(f.end_fuzzy), flip(f.start_fuzzy)) };
            let mut copy = f.clone();
            copy.start_fuzzy = start_fuzzy;
            copy.end_fuzzy = end_fuzzy;
            graft(c, &copy, vec![Segment::new(hit.start, hit.end)], strand);
            added.push(json!({ "name": f.name, "start": hit.start + 1, "end": hit.end + 1, "strand": if strand == Strand::Reverse { "reverse" } else { "forward" }, "mismatches": hit.mismatches }));
        }
    }
    let mut result = Map::new();
    result.insert("added".into(), json!(added));
    if added.is_empty() {
        result.insert("note".into(), json!(if report.is_empty() { "nothing matched".to_string() } else { report.join("; ") }));
    }
    Ok(Applied { summary: format!("Copied {} features from {}", added.len(), source.name), notes: report, result })
}

/// One place in the target where a known feature's bases occur.
struct Found {
    start: usize,
    end: usize,
    mismatches: usize,
    /// Each source feature that has these bases, with the strand it would lie on here.
    uses: Vec<(usize, Strand)>,
}

/// A source feature, where it came from, and the top-strand bases it covers there.
struct Known {
    path: String,
    feature: Feature,
    bases: String,
}

struct Proposal {
    row: Value,
    known: usize,
    start: usize,
    end: usize,
    strand: Strand,
}

fn sources(session: &Session, listed: &[Listed], wanted: &[String], except: &Listed) -> Result<Vec<Listed>, Failure> {
    let mut picked: BTreeMap<String, Listed> = BTreeMap::new();
    for source in wanted {
        let source = source.trim().trim_matches('/');
        let in_folder: Vec<&Listed> = listed.iter().filter(|l| l.path.starts_with(Path::new(source))).collect();
        if source.eq_ignore_ascii_case("all") {
            picked.extend(listed.iter().map(|l| (l.id.to_string(), l.clone())));
        } else if session.project().root().join(source).is_dir() && !source.is_empty() {
            picked.extend(in_folder.into_iter().map(|l| (l.id.to_string(), l.clone())));
        } else {
            let one = refs::plasmid(listed, source)?;
            picked.insert(one.id.to_string(), one.clone());
        }
    }
    picked.remove(except.id.as_str());
    Ok(picked.into_values().collect())
}

fn proposals(target: &Construct, known: &[Known], min_length: usize, max_mismatches: usize) -> (Vec<Proposal>, Vec<Value>, usize) {
    // One entry per distinct sequence, read in the feature's own direction.
    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, k) in known.iter().enumerate() {
        if k.bases.len() < min_length {
            continue;
        }
        let own = if k.feature.strand == Strand::Reverse { seq::revcomp(&k.bases) } else { k.bases.clone() };
        index.entry(own).or_default().push(i);
    }
    let mut found: BTreeMap<(usize, usize), Found> = BTreeMap::new();
    for (bases, uses) in &index {
        for hit in find_binding_sites(&target.sequence, bases, SearchOptions { max_mismatches, circular: target.circular }) {
            let strand = if hit.forward { Strand::Forward } else { Strand::Reverse };
            let place =
                found.entry((hit.start, hit.end)).or_insert(Found { start: hit.start, end: hit.end, mismatches: hit.mismatches, uses: Vec::new() });
            place.uses.extend(uses.iter().map(|u| (*u, strand)));
            place.mismatches = place.mismatches.min(hit.mismatches);
        }
    }

    let length = target.len();
    let covers = |f: &Feature, start: usize, end: usize| {
        let spans = |s: usize, e: usize| if e >= s { vec![(s, e)] } else { vec![(s, length.saturating_sub(1)), (0, e)] };
        f.segments.iter().flat_map(|s| spans(s.start, s.end)).any(|(a, b)| spans(start, end).iter().any(|(c, d)| a <= *d && b >= *c))
    };
    let (mut proposed, mut already) = (Vec::new(), Vec::new());
    for place in found.values() {
        let forward = place.uses.iter().filter(|(_, s)| *s == Strand::Forward).count();
        let reverse = place.uses.len() - forward;
        let strand = if reverse > forward { Strand::Reverse } else { Strand::Forward };
        let uses: Vec<usize> = place.uses.iter().filter(|(_, s)| *s == strand).map(|(u, _)| *u).collect();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for u in &uses {
            if let Some(k) = known.get(*u) {
                *counts.entry(k.feature.name.as_str()).or_default() += 1;
            }
        }
        let mut names: Vec<(&str, usize)> = counts.into_iter().collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let Some((name, _)) = names.first().copied() else {
            continue;
        };
        let Some(use_index) = uses.iter().copied().find(|u| known.get(*u).is_some_and(|k| k.feature.name == name)) else {
            continue;
        };
        let Some(chosen) = known.get(use_index) else {
            continue;
        };
        let mut from: Vec<&str> = uses.iter().filter_map(|u| known.get(*u)).map(|k| k.path.as_str()).collect();
        from.dedup();
        from.truncate(3);
        let span = if place.end >= place.start { place.end - place.start + 1 } else { length - place.start + place.end + 1 };
        let mut row = json!({
            "name": name,
            "type": chosen.feature.kind,
            "start": place.start + 1,
            "end": place.end + 1,
            "strand": if strand == Strand::Reverse { "reverse" } else { "forward" },
            "length": span,
            "mismatches": place.mismatches,
            "from": from,
        });
        if let (true, Some(map)) = (names.len() > 1, row.as_object_mut()) {
            map.insert("also_called".into(), json!(names.iter().skip(1).take(5).map(|n| n.0).collect::<Vec<_>>()));
        }
        if let (true, Some(map)) = (forward > 0 && reverse > 0, row.as_object_mut()) {
            map.insert(
                "strand_note".into(),
                json!(format!("sources disagree on the strand ({forward} forward, {reverse} reverse); the majority is used")),
            );
        }
        // Already there: a feature on exactly these bases, or one of the same name over them.
        let have = annotated_at(target, place.start, place.end, strand, None)
            .or_else(|| annotated_at(target, place.start, place.end, opposite(strand), None))
            .or_else(|| target.features.iter().find(|f| f.name.eq_ignore_ascii_case(name) && covers(f, place.start, place.end)));
        match have {
            Some(existing) => {
                if let Some(map) = row.as_object_mut() {
                    map.insert("existing".into(), json!(existing.name));
                }
                already.push(row);
            }
            None => proposed.push(Proposal { row, known: use_index, start: place.start, end: place.end, strand }),
        }
    }
    (proposed, already, index.len())
}

fn auto_annotate(session: &Session, args: &Args) -> Result<Value, Failure> {
    let listed = session.project().list()?;
    let target = refs::plasmid(&listed, &args.str("plasmid")?)?.clone();
    let min_length = match args.opt_int("min_length")?.unwrap_or(15) {
        n if n >= 6 => n as usize,
        _ => return fail("min_length must be 6 or more"),
    };
    let max_mismatches = mismatches(args)?;
    let primers = args.opt_bool("include_primers")? == Some(true);

    let mut known = Vec::new();
    let from = sources(session, &listed, &args.strings("sources"), &target)?;
    for source in &from {
        let construct = session.project().read(&source.id)?.construct;
        for f in &construct.features {
            if f.kind == "source" || (!primers && f.kind == "primer_bind") || f.locations().is_some() || f.location_unsupported.is_some() {
                continue;
            }
            known.push(Known { path: refs::label(&source.path), feature: f.clone(), bases: slice(&construct, f.start as i64 + 1, f.end as i64 + 1) });
        }
    }

    if args.opt_bool("dry_run")? != Some(false) {
        let loaded = session.project().read(&target.id)?;
        let (proposed, already, indexed) = proposals(&loaded.construct, &known, min_length, max_mismatches);
        return Ok(json!({
            "plasmid": refs::label(&loaded.path),
            "version": token(&loaded),
            "dry_run": true,
            "sources_searched": from.len(),
            "feature_sequences_indexed": indexed,
            "proposed": proposed.into_iter().map(|p| p.row).collect::<Vec<_>>(),
            "already_annotated": already,
        }));
    }
    session.edit("auto_annotate", args, true, |c| {
        let (proposed, already, _) = proposals(c, &known, min_length, max_mismatches);
        let mut rows = Vec::new();
        for p in proposed {
            let Some(k) = known.get(p.known) else {
                continue;
            };
            let mut original = k.feature.clone();
            if p.strand != original.strand {
                (original.start_fuzzy, original.end_fuzzy) = (flip(original.end_fuzzy), flip(original.start_fuzzy));
            }
            graft(c, &original, vec![Segment::new(p.start, p.end)], p.strand);
            rows.push(p.row);
        }
        let mut result = Map::new();
        result.insert("sources_searched".into(), json!(from.len()));
        result.insert("already_annotated".into(), json!(already.len()));
        let summary = format!("Added {} known features", rows.len());
        result.insert("added".into(), json!(rows));
        Ok(Applied { summary, result, ..Applied::default() })
    })
}
