//! Changing a construct's features and its name.

use super::{ANY_VERSION, FEATURE, PLASMID};
use bios_core::acts::{self, Action};
use bios_core::{Construct, Feature, Segment};
use serde_json::json;

use super::feature_summary;
use super::sequence::{in_range, new_bios_id, strand_of};

use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "rename_plasmid",
        description: "Rename a plasmid. Its file is renamed with it and its history stays attached.",
        params: &[PLASMID, ANY_VERSION, Param { name: "new_name", kind: Kind::String, required: true, description: "The new name." }],
        effect: Effect::Write,
        run: Run::EditLatest(rename_plasmid),
    },
    CommandSpec {
        name: "rename_feature",
        description: "Rename a feature (its /label, the name shown on the map).",
        params: &[PLASMID, ANY_VERSION, FEATURE, Param { name: "new_name", kind: Kind::String, required: true, description: "The new name." }],
        effect: Effect::Write,
        run: Run::EditLatest(rename_feature),
    },
    CommandSpec {
        name: "update_feature",
        description: "Change a feature: its name, type, strand, start..end, or any GenBank qualifier, and Bios's own facts about it (what it is, why it is here, what it does, its colour). Without start/end the change does not depend on positions and needs no version; moving it does.",
        params: &[
            PLASMID,
            ANY_VERSION,
            FEATURE,
            Param { name: "name", kind: Kind::String, required: false, description: "New name." },
            Param { name: "type", kind: Kind::String, required: false, description: "New GenBank type (CDS, promoter, ...)." },
            Param { name: "strand", kind: Kind::String, required: false, description: "forward or reverse." },
            Param { name: "start", kind: Kind::Integer, required: false, description: "New first base. Needs version." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "New last base. Needs version." },
            Param {
                name: "category",
                kind: Kind::String,
                required: false,
                description: "What it is (promoter, cds, marker, terminator, origin, primer, insulator, tag, linker, misc). Empty text clears it.",
            },
            Param { name: "why", kind: Kind::String, required: false, description: "Why it is here. Empty text clears it." },
            Param { name: "description", kind: Kind::String, required: false, description: "What it does. Empty text clears it." },
            Param { name: "color", kind: Kind::String, required: false, description: "Display colour, such as \"#b7e6d7\". Empty text clears it." },
            Param {
                name: "qualifiers",
                kind: Kind::TextMap,
                required: false,
                description: "GenBank qualifiers to set, such as {\"gene\": \"lacZ\"}. An empty value removes the qualifier.",
            },
        ],
        effect: Effect::Write,
        run: Run::EditWhen(moves, update_feature),
    },
    CommandSpec {
        name: "set_feature_actions",
        description: "Say what a feature does, and to what: the construct described as a system. Each action is a verb in your own words (transcribes, cuts, represses, binds, ...) and the things it acts on: other features, or \"construct\" for the whole plasmid. One verb can have several targets; a different verb is a different action. This replaces every action the feature had, so pass the ones to keep too; an empty list removes them all. The other features read it back as what acts on them. Stored on the feature as /note=\"bios-act: <verb> > <id>, <id>\" lines naming the targets' Bios ids (a feature without one is given one), and marked as said by a person.",
        params: &[
            PLASMID,
            ANY_VERSION,
            FEATURE,
            Param {
                name: "actions",
                kind: Kind::Objects,
                required: true,
                description: "The feature's actions, each {\"verb\": \"transcribes\", \"targets\": [\"T7 promoter\", \"construct\"]}. A target is a feature (\"#3\", its bios id or its name) or \"construct\". An action with no verb or no targets is dropped.",
            },
        ],
        effect: Effect::Write,
        run: Run::EditLatest(set_feature_actions),
    },
    CommandSpec {
        name: "remove_feature",
        description: "Remove a feature. The bases under it are not touched.",
        params: &[PLASMID, ANY_VERSION, FEATURE],
        effect: Effect::Write,
        run: Run::EditLatest(remove_feature),
    },
];

fn new_name(args: &Args) -> Result<String, Failure> {
    let name = args.str("new_name")?.trim().to_string();
    if name.is_empty() { fail("new_name is empty") } else { Ok(name) }
}

fn rename_plasmid(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let name = new_name(args)?;
    let summary = format!("Renamed {} to {name}", c.name);
    c.name = name;
    Ok(Applied { summary, ..Applied::default() })
}

fn rename_feature(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let name = new_name(args)?;
    let index = refs::feature(c, &args.str("feature")?)?;
    let Some(f) = c.features.get_mut(index) else {
        return fail("that feature is gone");
    };
    let note = format!("\"{}\" → \"{name}\"", f.name);
    f.name = name;
    Ok(Applied { summary: format!("Renamed feature {note}"), notes: vec![note], ..Applied::default() })
}

fn remove_feature(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let index = refs::feature(c, &args.str("feature")?)?;
    let removed = c.features.remove(index);
    let note = format!("removed \"{}\" ({}..{})", removed.name, removed.start + 1, removed.end + 1);
    Ok(Applied { summary: format!("Removed feature {}", removed.name), notes: vec![note], ..Applied::default() })
}

/// The feature at `index`'s Bios id, minting one when it has none, so a statement can point at it.
fn stamped_id(c: &mut Construct, index: usize) -> Result<String, Failure> {
    let Some(f) = c.features.get_mut(index) else {
        return fail("that feature is gone");
    };
    if let Some(id) = acts::feature_id(f) {
        return Ok(id.to_string());
    }
    let line = new_bios_id();
    let id = line.trim_start_matches("bios-id:").trim().to_string();
    match f.notes.iter_mut().find(|(k, _)| k == "note") {
        Some((_, values)) => values.push(line),
        None => f.notes.push(("note".into(), vec![line])),
    }
    Ok(id)
}

/// `applyFeatureActions` in the web's `editor/feature/editFeature.ts`: the actor and every feature
/// it names get a Bios id, the statements replace the feature's own, and the feature is marked as
/// described by a person (`bios-use-source: human`) when anything changed.
fn set_feature_actions(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let index = refs::feature(c, &args.str("feature")?)?;
    let had: Vec<String> = c.features.get(index).map(|f| acts::actions_of(f).into_iter().flat_map(|a| a.targets).collect()).unwrap_or_default();
    let mut actions = Vec::new();
    let mut said = Vec::new();
    for raw in args.objects("actions") {
        let verb = raw.get("verb").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let mut targets = Vec::new();
        let mut names = Vec::new();
        for t in raw.get("targets").and_then(|v| v.as_array()).map(Vec::as_slice).unwrap_or_default() {
            let Some(t) = t.as_str().map(str::trim).filter(|t| !t.is_empty()) else {
                return fail("each target is text: a feature or \"construct\"");
            };
            if t == acts::FOR_CONSTRUCT {
                targets.push(t.to_string());
                names.push("this construct".to_string());
                continue;
            }
            match refs::feature(c, t) {
                Ok(at) => {
                    targets.push(stamped_id(c, at)?);
                    names.push(c.features.get(at).map(|f| f.name.clone()).unwrap_or_default());
                }
                // A target the feature already named and that has since gone is kept as it was,
                // as the web keeps it ("something removed"), rather than refusing the whole edit.
                Err(_) if had.iter().any(|h| h == t) => {
                    targets.push(t.to_string());
                    names.push("something removed".to_string());
                }
                Err(e) => return Err(e),
            }
        }
        let action = Action { verb, targets }.cleaned();
        if action.is_statement() {
            said.push(json!({ "verb": action.verb, "targets": names }));
            actions.push(action);
        }
    }
    stamped_id(c, index)?;
    let Some(f) = c.features.get_mut(index) else {
        return fail("that feature is gone");
    };
    let before = f.notes.clone();
    acts::write_actions(f, &actions);
    if f.notes != before {
        set_bios_note(f, "use-source", "human");
    }
    let name = f.name.clone();
    let mut result = serde_json::Map::new();
    result.insert("feature".into(), json!(name));
    result.insert("actions".into(), json!(said));
    let summary = if actions.is_empty() {
        format!("Cleared what {name} acts on")
    } else {
        format!("{name}: {}", actions.iter().map(|a| a.verb.as_str()).collect::<Vec<_>>().join(", "))
    };
    Ok(Applied { summary, result, ..Applied::default() })
}

fn moves(args: &Args) -> bool {
    args.has("start") || args.has("end")
}

/// Sets, or with an empty value removes, one of Bios's own facts about a feature. They live in
/// `bios-<key>: value` notes, after the feature's ordinary notes.
pub(crate) fn set_bios_note(f: &mut Feature, key: &str, value: &str) {
    let prefix = format!("bios-{key}:");
    let value: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let position = f.notes.iter().position(|(k, _)| k == "note");
    let Some(values) = position.and_then(|i| f.notes.get_mut(i)).map(|(_, v)| v) else {
        if !value.is_empty() {
            f.notes.push(("note".into(), vec![format!("{prefix} {value}")]));
        }
        return;
    };
    values.retain(|n| !n.starts_with(&prefix));
    if !value.is_empty() {
        values.push(format!("{prefix} {value}"));
    }
    if values.is_empty() {
        f.notes.retain(|(k, _)| k != "note");
    }
}

/// Sets, or with an empty value removes, a qualifier. One value replaces every value it had.
pub(crate) fn set_qualifier(f: &mut Feature, key: &str, value: &str) {
    if value.is_empty() {
        f.notes.retain(|(k, _)| k != key);
        return;
    }
    match f.notes.iter_mut().find(|(k, _)| k == key) {
        Some((_, values)) => *values = vec![value.to_string()],
        None => f.notes.push((key.to_string(), vec![value.to_string()])),
    }
}

/// Puts a simple feature on `start..end`, 1-based inclusive. A feature in several pieces is refused.
pub(crate) fn set_span(c: &Construct, f: &mut Feature, start: i64, end: i64) -> Result<(), Failure> {
    in_range(c, start, end)?;
    if f.locations().is_some() || f.location_unsupported.is_some() {
        return fail(format!("\"{}\" has a location in several pieces; change it with edit_plasmid", f.name));
    }
    f.start = (start - 1) as usize;
    f.end = (end - 1) as usize;
    f.segments = vec![Segment::new(f.start, f.end)];
    Ok(())
}

fn update_feature(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let index = refs::feature(c, &args.str("feature")?)?;
    let Some(mut f) = c.features.get(index).cloned() else {
        return fail("that feature is gone");
    };
    let before = feature_summary(c, index, &f);
    if let Some(name) = args.opt_str("name")? {
        let name = name.trim();
        if name.is_empty() {
            return fail("name is empty");
        }
        f.name = name.to_string();
    }
    if let Some(kind) = args.opt_str("type")?.filter(|k| !k.trim().is_empty()) {
        f.kind = kind.trim().to_string();
    }
    if let Some(strand) = strand_of(args)? {
        f.strand = strand;
    }
    if moves(args) {
        let start = args.opt_int("start")?.unwrap_or(f.start as i64 + 1);
        let end = args.opt_int("end")?.unwrap_or(f.end as i64 + 1);
        set_span(c, &mut f, start, end)?;
    }
    for (param, key) in [("category", "category"), ("why", "use"), ("description", "description")] {
        if let Some(value) = args.opt_str(param)? {
            set_bios_note(&mut f, key, &value);
        }
    }
    if let Some(color) = args.opt_str("color")? {
        set_qualifier(&mut f, "color", color.trim());
    }
    for (key, value) in args.text_map("qualifiers") {
        if key == "label" {
            return fail("the name is the label; set it with name");
        }
        set_qualifier(&mut f, &key, &value);
    }
    let name = f.name.clone();
    if let Some(slot) = c.features.get_mut(index) {
        *slot = f;
    }
    let mut result = serde_json::Map::new();
    result.insert("before".into(), before);
    if let Some(f) = c.features.get(index) {
        result.insert("after".into(), feature_summary(c, index, f));
    }
    Ok(Applied { summary: format!("Changed feature {name}"), result, ..Applied::default() })
}
