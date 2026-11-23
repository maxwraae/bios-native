//! Primers. A primer is a `primer_bind` feature: a footprint on the plasmid and a direction. The
//! oligo to order is always the plasmid's own bases there, so a primer can never disagree with
//! the sequence it sits on.

use bios_core::search::{SearchOptions, find_binding_sites};
use bios_core::{Construct, Feature, Segment, Strand, seq};
use serde_json::{Map, Value, json};

use super::features::set_span;
use super::sequence::{bases, new_bios_id, slice, strand_of};
use super::{ANY_VERSION, PLASMID, VERSION, feature_summary};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const PRIMER: Param = Param {
    name: "primer",
    kind: Kind::String,
    required: true,
    description: "The primer: its name, its #index from read_plasmid, or its bios id. Only primer_bind features count.",
};
const OLIGO: Param = Param {
    name: "sequence",
    kind: Kind::String,
    required: false,
    description: "The primer's sequence, 5' to 3'. It is placed where it binds, on either strand and through the origin; it must bind exactly once.",
};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "add_primer",
        description: "Add a primer (a primer_bind feature). Give start/end/strand, or just the primer's sequence and it is placed where it binds (it must bind exactly once). Returns the oligo to order.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "name", kind: Kind::String, required: true, description: "The primer's name." },
            OLIGO,
            Param { name: "start", kind: Kind::Integer, required: false, description: "First base of the footprint." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "Last base of the footprint." },
            Param { name: "strand", kind: Kind::String, required: false, description: "forward (default) or reverse." },
        ],
        effect: Effect::Write,
        run: Run::Edit(add_primer),
    },
    CommandSpec {
        name: "update_primer",
        description: "Edit a primer: rename it, give it a new sequence (it moves to where that oligo binds; must bind exactly once), or change its footprint (start..end) and/or strand. Only a rename needs no version.",
        params: &[
            PLASMID,
            ANY_VERSION,
            PRIMER,
            Param { name: "name", kind: Kind::String, required: false, description: "New name." },
            OLIGO,
            Param { name: "start", kind: Kind::Integer, required: false, description: "New first base of the footprint." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "New last base of the footprint." },
            Param { name: "strand", kind: Kind::String, required: false, description: "forward or reverse." },
        ],
        effect: Effect::Write,
        run: Run::EditWhen(places, update_primer),
    },
    CommandSpec {
        name: "move_primer",
        description: "Move a primer, keeping its length and strand. Give `start` (the new first base of its footprint) or `shift` (+n = n bases to the right, -n = left). On a circular plasmid it may cross the origin; on a linear one it must stay inside. The oligo becomes the plasmid's bases at the new place (reported).",
        params: &[
            PLASMID,
            VERSION,
            PRIMER,
            Param { name: "start", kind: Kind::Integer, required: false, description: "New first base of the footprint." },
            Param { name: "shift", kind: Kind::Integer, required: false, description: "Move by this many bases (negative = left)." },
        ],
        effect: Effect::Write,
        run: Run::Edit(move_primer),
    },
    CommandSpec {
        name: "remove_primer",
        description: "Remove a primer. The bases stay. Refuses features that are not primers.",
        params: &[PLASMID, ANY_VERSION, PRIMER],
        effect: Effect::Write,
        run: Run::EditLatest(remove_primer),
    },
];

fn places(args: &Args) -> bool {
    ["sequence", "start", "end", "strand"].iter().any(|p| args.has(p))
}

pub(crate) fn find_primer(c: &Construct, reference: &str) -> Result<usize, Failure> {
    let wanted = reference.trim();
    let is_primer = |f: &Feature| f.kind == "primer_bind";
    // By number or by id the caller means one exact feature; by name only primers are looked at,
    // so a primer may share its name with the gene it amplifies.
    let exact = wanted.trim_start_matches('#').parse::<usize>().is_ok() || c.features.iter().any(|f| refs::bios_note(f, "id") == Some(wanted));
    let index = if exact {
        refs::feature(c, wanted)?
    } else {
        let named = |test: &dyn Fn(&str) -> bool| -> Vec<usize> {
            c.features.iter().enumerate().filter(|(_, f)| is_primer(f) && test(&f.name)).map(|(i, _)| i).collect()
        };
        let mut hits = named(&|n| n == wanted);
        if hits.is_empty() {
            hits = named(&|n| n.eq_ignore_ascii_case(wanted));
        }
        match hits.as_slice() {
            [one] => *one,
            [] => {
                if let Some(other) = c.features.iter().find(|f| f.name.eq_ignore_ascii_case(wanted)) {
                    return fail(format!("\"{wanted}\" is a {} feature, not a primer; use update_feature or remove_feature for it", other.kind));
                }
                let all: Vec<String> =
                    c.features.iter().enumerate().filter(|(_, f)| is_primer(f)).map(|(i, f)| format!("#{} {}", i + 1, f.name)).collect();
                return fail(format!("no primer called \"{wanted}\". Primers: {}", if all.is_empty() { "none".into() } else { all.join(", ") }));
            }
            many => return fail(format!("\"{wanted}\" matches {} primers. Use the #index or the bios id", many.len())),
        }
    };
    match c.features.get(index) {
        Some(f) if !is_primer(f) => {
            fail(format!("\"{}\" is a {} feature, not a primer; use update_feature or remove_feature for it", f.name, f.kind))
        }
        Some(f) if f.locations().is_some() => fail("that primer has a location in several pieces; change it with edit_plasmid"),
        Some(_) => Ok(index),
        None => fail("that primer is gone"),
    }
}

/// The oligo to order for a footprint: the plasmid's bases there, read 5' to 3' on its strand.
pub(crate) fn oligo(c: &Construct, f: &Feature) -> String {
    let top = slice(c, f.start as i64 + 1, f.end as i64 + 1);
    if f.strand == Strand::Reverse { seq::revcomp(&top) } else { top }
}

/// What a person needs to know about a primer: where it is, the oligo, and how many places it binds.
fn card(c: &Construct, index: usize) -> Value {
    let Some(f) = c.features.get(index) else {
        return Value::Null;
    };
    let bases = oligo(c, f);
    let sites = find_binding_sites(&c.sequence, &bases, SearchOptions { max_mismatches: 0, circular: c.circular }).len();
    let mut out = feature_summary(c, index, f);
    if let Some(map) = out.as_object_mut() {
        map.insert("bases".into(), json!(bases));
        map.insert("binding_sites".into(), json!(sites));
    }
    out
}

/// Where `sequence` binds, as (start, end, strand), 1-based. It must bind exactly once.
fn binding(c: &Construct, sequence: &str) -> Result<(i64, i64, Strand), Failure> {
    let sites = find_binding_sites(&c.sequence, &bases(sequence)?, SearchOptions { max_mismatches: 0, circular: c.circular });
    match sites.as_slice() {
        [one] => Ok((one.start as i64 + 1, one.end as i64 + 1, if one.forward { Strand::Forward } else { Strand::Reverse })),
        [] => fail("that sequence does not bind anywhere on the plasmid exactly; give start/end"),
        many => {
            let places: Vec<String> =
                many.iter().map(|s| format!("{}..{} {}", s.start + 1, s.end + 1, if s.forward { "forward" } else { "reverse" })).collect();
            fail(format!("that sequence binds {} times ({}); give start/end/strand instead", many.len(), places.join(", ")))
        }
    }
}

fn add_primer(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let (start, end, strand) = match (args.opt_str("sequence")?, args.opt_int("start")?, args.opt_int("end")?) {
        (Some(sequence), None, None) => binding(c, &sequence)?,
        (_, Some(start), Some(end)) => (start, end, strand_of(args)?.unwrap_or(Strand::Forward)),
        _ => return fail("give the primer's sequence, or both start and end"),
    };
    let mut f = Feature {
        name: name.clone(),
        kind: "primer_bind".into(),
        start: 0,
        end: 0,
        strand,
        segments: vec![Segment::new(0, 0)],
        notes: vec![("note".into(), vec![new_bios_id()])],
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    set_span(c, &mut f, start, end)?;
    c.features.push(f);
    let index = c.features.len().saturating_sub(1);
    let mut result = Map::new();
    result.insert("primer".into(), card(c, index));
    Ok(Applied { summary: format!("Added primer {name} at {start}..{end}"), result, ..Applied::default() })
}

fn update_primer(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    if args.has("sequence") && (args.has("start") || args.has("end") || args.has("strand")) {
        return fail("give either a new sequence or a new start/end/strand, not both");
    }
    let index = find_primer(c, &args.str("primer")?)?;
    let before = card(c, index);
    let Some(mut f) = c.features.get(index).cloned() else {
        return fail("that primer is gone");
    };
    if let Some(name) = args.opt_str("name")?.filter(|n| !n.trim().is_empty()) {
        f.name = name.trim().to_string();
    }
    if let Some(sequence) = args.opt_str("sequence")? {
        let (start, end, strand) = binding(c, &sequence)?;
        set_span(c, &mut f, start, end)?;
        f.strand = strand;
    } else if places(args) {
        let start = args.opt_int("start")?.unwrap_or(f.start as i64 + 1);
        let end = args.opt_int("end")?.unwrap_or(f.end as i64 + 1);
        set_span(c, &mut f, start, end)?;
        if let Some(strand) = strand_of(args)? {
            f.strand = strand;
        }
    }
    let name = f.name.clone();
    if let Some(slot) = c.features.get_mut(index) {
        *slot = f;
    }
    let after = card(c, index);
    let notes = match after["binding_sites"].as_u64() {
        Some(n) if n > 1 => vec![format!("\"{name}\" binds {n} places on this plasmid")],
        _ => Vec::new(),
    };
    let mut result = Map::new();
    result.insert("before".into(), before);
    result.insert("after".into(), after);
    Ok(Applied { summary: format!("Changed primer {name}"), notes, result })
}

fn move_primer(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let (start, shift) = (args.opt_int("start")?, args.opt_int("shift")?);
    if start.is_some() == shift.is_some() {
        return fail("give either start or shift");
    }
    let index = find_primer(c, &args.str("primer")?)?;
    let before = card(c, index);
    let Some(mut f) = c.features.get(index).cloned() else {
        return fail("that primer is gone");
    };
    let length = c.len() as i64;
    let size = f.span(c.len()) as i64;
    // 0-based from here on, so that wrapping is plain modular arithmetic.
    let mut at = match (start, shift) {
        (Some(start), _) => start - 1,
        (_, Some(shift)) => f.start as i64 + shift,
        _ => 0,
    };
    if c.circular {
        at = at.rem_euclid(length);
    } else if at < 0 || at + size > length {
        return fail(format!("the primer ({size} bases) would run off the end of this linear plasmid (1..{length})"));
    }
    set_span(c, &mut f, at + 1, (at + size - 1).rem_euclid(length) + 1)?;
    let name = f.name.clone();
    if let Some(slot) = c.features.get_mut(index) {
        *slot = f;
    }
    let after = card(c, index);
    let note = format!(
        "\"{name}\" {} → {}; oligo {} → {}",
        text(&before["location"]),
        text(&after["location"]),
        text(&before["bases"]),
        text(&after["bases"])
    );
    let mut result = Map::new();
    result.insert("before".into(), before);
    result.insert("after".into(), after);
    Ok(Applied { summary: format!("Moved primer {name}"), notes: vec![note], result })
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn remove_primer(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let index = find_primer(c, &args.str("primer")?)?;
    let removed = card(c, index);
    let f = c.features.remove(index);
    let note = format!("removed primer \"{}\" ({}, {})", f.name, text(&removed["location"]), text(&removed["bases"]));
    let mut result = Map::new();
    result.insert("removed".into(), removed);
    Ok(Applied { summary: format!("Removed primer {}", f.name), notes: vec![note], result })
}
