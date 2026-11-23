//! Changing the bases. Features follow their bases: the ones after an edit shift, the ones inside
//! a deletion go, and the ones it cuts into are trimmed, all of it reported.

use bios_core::edits::{self, Annotation, EditResult, FeatureDelta};
use bios_core::{Construct, Strand, seq};

use super::{PLASMID, VERSION, feature_summary};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail};

const START: Param = Param { name: "start", kind: Kind::Integer, required: true, description: "First base (1-based)." };
const END: Param = Param {
    name: "end",
    kind: Kind::Integer,
    required: true,
    description: "Last base (1-based, inclusive). start > end wraps the origin of a circular plasmid.",
};
const BASES: Param = Param { name: "sequence", kind: Kind::String, required: true, description: "Bases, or FASTA. IUPAC letters are allowed." };

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "insert_sequence",
        description: "Insert bases. position = insert after this base; 0 = before base 1. Features after it shift; a feature spanning the point grows to include the new bases (reported).",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "position", kind: Kind::Integer, required: true, description: "Insert after this base; 0 = before base 1." },
            BASES,
        ],
        effect: Effect::Write,
        run: Run::Edit(insert_sequence),
    },
    CommandSpec {
        name: "delete_range",
        description: "Delete bases start..end (1-based inclusive; start > end wraps the origin). Features after it shift; features inside are removed and features overlapping it are trimmed (all reported).",
        params: &[PLASMID, VERSION, START, END],
        effect: Effect::Write,
        run: Run::Edit(delete_range),
    },
    CommandSpec {
        name: "replace_range",
        description: "Replace bases start..end with a new sequence. Same feature rules as delete then insert; replacing with the same number of bases moves nothing.",
        params: &[
            PLASMID,
            VERSION,
            START,
            END,
            BASES,
            Param {
                name: "expected_sequence",
                kind: Kind::String,
                required: false,
                description: "Optional: the exact bases you expect at this range, copied from get_sequence. The edit is refused if they differ, which catches a wrong coordinate before it does damage.",
            },
        ],
        effect: Effect::Write,
        run: Run::Edit(replace_range),
    },
    CommandSpec {
        name: "add_feature",
        description: "Add a feature over start..end (1-based inclusive; start > end wraps the origin). It gets a bios id so it can be addressed later whatever it is renamed to. Optionally say what it is (category), why it is here (why) and what it serves (serves): another feature, or \"construct\". A why with nothing named serves the construct.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "name", kind: Kind::String, required: true, description: "The name shown on the map." },
            START,
            END,
            Param { name: "strand", kind: Kind::String, required: false, description: "forward (default) or reverse." },
            Param {
                name: "type",
                kind: Kind::String,
                required: false,
                description: "GenBank type (CDS, promoter, terminator, rep_origin, primer_bind, misc_feature, ...). Default misc_feature.",
            },
            Param {
                name: "qualifiers",
                kind: Kind::TextMap,
                required: false,
                description: "Extra GenBank qualifiers, such as {\"gene\": \"lacZ\"}.",
            },
            Param {
                name: "category",
                kind: Kind::String,
                required: false,
                description: "What it is (promoter, cds, marker, terminator, origin, insulator, tag, linker, misc).",
            },
            Param { name: "why", kind: Kind::String, required: false, description: "Why it is here, in a sentence." },
            Param {
                name: "serves",
                kind: Kind::String,
                required: false,
                description: "What it serves: a feature (#3, its bios id or its name), or \"construct\".",
            },
        ],
        effect: Effect::Write,
        run: Run::Edit(add_feature),
    },
    CommandSpec {
        name: "set_topology",
        description: "Make the plasmid circular or linear.",
        params: &[PLASMID, VERSION, Param { name: "circular", kind: Kind::Boolean, required: true, description: "true = circular, false = linear." }],
        effect: Effect::Write,
        run: Run::Edit(set_topology),
    },
];

/// Bases as a caller may give them: plain, spaced, numbered, or a FASTA record.
pub(crate) fn bases(text: &str) -> Result<String, Failure> {
    let body: String = text.lines().filter(|l| !l.trim_start().starts_with('>')).collect::<Vec<_>>().join("");
    let clean = seq::clean_seq(&body);
    if clean.is_empty() { fail("sequence holds no bases") } else { Ok(clean) }
}

pub(crate) fn in_range(c: &Construct, start: i64, end: i64) -> Result<(), Failure> {
    let length = c.len() as i64;
    if start < 1 || end < 1 || start > length || end > length {
        return fail(format!("range must be within 1..{length}"));
    }
    if start > end && !c.circular {
        return fail("start after end only works on a circular plasmid");
    }
    Ok(())
}

pub(crate) fn span_len(c: &Construct, start: i64, end: i64) -> i64 {
    if start <= end { end - start + 1 } else { c.len() as i64 - start + 1 + end }
}

/// Puts an edit's result into the construct and says what happened to its features.
fn apply(c: &mut Construct, result: EditResult) -> Vec<String> {
    c.sequence = result.sequence;
    c.features = result.features;
    notes_for(&result.delta)
}

fn notes_for(delta: &FeatureDelta) -> Vec<String> {
    let mut notes = Vec::new();
    for f in &delta.removed {
        notes.push(format!("removed \"{}\" ({}..{}): all of its bases were deleted", f.name, f.start + 1, f.end + 1));
    }
    for t in &delta.truncated {
        let bridged = if t.bridged { ", and it now runs across the new bases" } else { "" };
        notes.push(format!(
            "\"{}\" lost {} of its bases; it is now {}..{}{bridged}",
            t.before.name,
            t.lost_bases,
            t.after.start + 1,
            t.after.end + 1
        ));
    }
    let cut: Vec<&str> = delta.truncated.iter().map(|t| t.after.name.as_str()).collect();
    let moved: Vec<&str> = delta.shifted.iter().map(|f| f.name.as_str()).filter(|n| !cut.contains(n)).collect();
    match moved.as_slice() {
        [] => {}
        few if few.len() <= 8 => notes.push(format!("moved or resized with their bases: {}", few.join(", "))),
        many => notes.push(format!("{} features moved or resized with their bases", many.len())),
    }
    notes
}

fn insert_sequence(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let position = args.int("position")?;
    let new = bases(&args.str("sequence")?)?;
    if position < 0 || position > c.len() as i64 {
        return fail(format!("position must be within 0..{}", c.len()));
    }
    let result = edits::apply_insert(&c.sequence, &c.features, position + 1, &new, c.circular).map_err(|e| Failure(e.to_string()))?;
    let notes = apply(c, result);
    Ok(Applied { summary: format!("Inserted {} bases after base {position}", new.len()), notes, ..Applied::default() })
}

fn delete_range(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let (start, end) = (args.int("start")?, args.int("end")?);
    in_range(c, start, end)?;
    let gone = span_len(c, start, end);
    if gone == c.len() as i64 {
        return fail("that would delete the whole plasmid; nothing was changed");
    }
    let result = edits::apply_delete(&c.sequence, &c.features, start, end, c.circular).map_err(|e| Failure(e.to_string()))?;
    let notes = apply(c, result);
    Ok(Applied { summary: format!("Deleted {start}..{end} ({gone} bases)"), notes, ..Applied::default() })
}

/// The bases of `start..end`, 1-based inclusive, through the origin when `start > end`.
pub(crate) fn slice(c: &Construct, start: i64, end: i64) -> String {
    let (s, e) = ((start - 1).max(0) as usize, end.max(0) as usize);
    if start <= end {
        c.sequence.get(s..e).unwrap_or_default().to_string()
    } else {
        format!("{}{}", c.sequence.get(s..).unwrap_or_default(), c.sequence.get(..e).unwrap_or_default())
    }
}

fn replace_range(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let (start, end) = (args.int("start")?, args.int("end")?);
    let new = bases(&args.str("sequence")?)?;
    in_range(c, start, end)?;
    if let Some(expected) = args.opt_str("expected_sequence")?
        && !slice(c, start, end).eq_ignore_ascii_case(&bases(&expected)?)
    {
        return fail("expected_sequence does not match this range; nothing was saved. Read the plasmid again and check the coordinates.");
    }
    let was = span_len(c, start, end);
    let result = edits::apply_replace(&c.sequence, &c.features, start, end, &new, c.circular).map_err(|e| Failure(e.to_string()))?;
    let notes = apply(c, result);
    Ok(Applied { summary: format!("Replaced {start}..{end} ({was} bases) with {} bases", new.len()), notes, ..Applied::default() })
}

pub(crate) fn strand_of(args: &Args) -> Result<Option<Strand>, Failure> {
    match args.opt_str("strand")?.as_deref() {
        None => Ok(None),
        Some("forward") => Ok(Some(Strand::Forward)),
        Some("reverse") => Ok(Some(Strand::Reverse)),
        Some(other) => fail(format!("strand must be forward or reverse, not {other}")),
    }
}

/// A fresh `bios-id` note. Bios keeps its own facts about a feature in notes, where every other
/// tool leaves them alone, and the id is what lets a feature be found whatever it is renamed to.
pub(crate) fn new_bios_id() -> String {
    format!("bios-id: {}", uuid::Uuid::new_v4())
}

fn add_feature(session: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let (start, end) = (args.int("start")?, args.int("end")?);
    in_range(c, start, end)?;
    let strand = strand_of(args)?.unwrap_or(Strand::Forward);
    let kind = args.opt_str("type")?.unwrap_or_else(|| "misc_feature".to_string());
    let mut notes: Vec<(String, Vec<String>)> =
        args.text_map("qualifiers").into_iter().filter(|(k, _)| k != "label").map(|(k, v)| (k, vec![v])).collect();
    match notes.iter_mut().find(|(k, _)| k == "note") {
        Some((_, values)) => values.push(new_bios_id()),
        None => notes.push(("note".into(), vec![new_bios_id()])),
    }
    // What it serves is named by the target's bios id, so a rename does not break it.
    let serves = match args.opt_str("serves")?.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(s) if s.eq_ignore_ascii_case("construct") => Some("construct".to_string()),
        Some(s) => {
            let target = crate::refs::feature(c, &s)?;
            match c.features.get(target).and_then(|f| crate::refs::bios_note(f, "id")) {
                Some(id) => Some(id.to_string()),
                None => return fail(format!("\"{s}\" has no bios id to be served by; give it one with update_feature first")),
            }
        }
        None => None,
    };
    let why = args.opt_str("why")?.map(|w| w.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|w| !w.is_empty());
    let category = args.opt_str("category")?.map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty());
    let annotation = Annotation { name: name.clone(), kind, strand, notes };
    let result = edits::apply_annotate(&c.sequence, &c.features, start, end, annotation).map_err(|e| Failure(e.to_string()))?;
    c.features = result.features;
    let index = c.features.len().saturating_sub(1);
    if let Some(f) = c.features.get_mut(index) {
        // `newFeature.ts`: a why with no target serves the construct, and who said it is kept.
        let serves = serves.or_else(|| why.as_ref().map(|_| "construct".to_string()));
        for (key, value) in [("category", category), ("use", why.clone()), ("for", serves)] {
            if let Some(value) = value {
                super::features::set_bios_note(f, key, &value);
            }
        }
        if why.is_some() {
            let source = if session.who.actor == bios_project::Actor::Human { "human" } else { "agent" };
            super::features::set_bios_note(f, "use-source", source);
        }
    }
    let mut out = serde_json::Map::new();
    if let Some(f) = c.features.get(index) {
        out.insert("feature".into(), feature_summary(c, index, f));
    }
    Ok(Applied { summary: format!("Added feature {name} at {start}..{end}"), result: out, ..Applied::default() })
}

fn set_topology(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let circular = args.opt_bool("circular")?.unwrap_or(true);
    if !circular {
        let through: Vec<&str> = c.features.iter().filter(|f| f.wraps()).map(|f| f.name.as_str()).collect();
        if !through.is_empty() {
            return fail(format!("these features run through the origin, so the plasmid cannot be made linear as it stands: {}", through.join(", ")));
        }
    }
    c.circular = circular;
    Ok(Applied { summary: format!("Made the plasmid {}", if circular { "circular" } else { "linear" }), ..Applied::default() })
}
