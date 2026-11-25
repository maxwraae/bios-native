//! The commands, grouped the way they are listed.

mod analysis;
mod assembly;
mod batch;
mod browse;
mod build;
mod compare;
mod copy;
mod design;
mod digest;
mod features;
mod files;
mod gibson;
mod golden_gate;
mod history;
mod oligos;
mod pcr;
mod picture;
mod primers;
mod quantities;
mod read;
mod sequence;
mod share;
mod show;
mod structure;
mod update;
mod verify;
mod window;

use bios_core::{Construct, Feature, Strand, genbank_write};
use serde_json::{Value, json};

use crate::{CommandSpec, Kind, Param, refs};

pub static ALL: &[&[CommandSpec]] = &[
    window::COMMANDS,
    show::COMMANDS,
    browse::COMMANDS,
    files::COMMANDS,
    read::COMMANDS,
    picture::COMMANDS,
    analysis::COMMANDS,
    compare::COMMANDS,
    quantities::COMMANDS,
    oligos::COMMANDS,
    sequence::COMMANDS,
    structure::COMMANDS,
    features::COMMANDS,
    primers::COMMANDS,
    design::COMMANDS,
    assembly::COMMANDS,
    pcr::COMMANDS,
    gibson::COMMANDS,
    digest::COMMANDS,
    golden_gate::COMMANDS,
    build::COMMANDS,
    copy::COMMANDS,
    batch::COMMANDS,
    history::COMMANDS,
    share::COMMANDS,
    verify::COMMANDS,
    update::COMMANDS,
];

pub(crate) const PLASMID: Param = Param {
    name: "plasmid",
    kind: Kind::String,
    required: true,
    description: "The plasmid: its path as list_plasmids shows it (\"backbones/pUC19\"), its name if only one plasmid has it, or its id.",
};

pub(crate) const VERSION: Param = Param {
    name: "version",
    kind: Kind::String,
    required: true,
    description: "The version you read (read_plasmid returns it). The edit is refused if the plasmid changed since.",
};

/// For edits that do not depend on positions.
pub(crate) const ANY_VERSION: Param = Param {
    name: "version",
    kind: Kind::String,
    required: false,
    description: "Optional here: this edit does not depend on positions, so it is applied to the plasmid as it is now.",
};

pub(crate) const FEATURE: Param = Param {
    name: "feature",
    kind: Kind::String,
    required: true,
    description: "The feature: its name, its #index from read_plasmid, or its bios id.",
};

/// One feature as a person reads it: 1-based, inclusive.
pub(crate) fn feature_summary(construct: &Construct, index: usize, f: &Feature) -> Value {
    let mut out = json!({
        "index": index + 1,
        "name": f.name,
        "type": f.kind,
        "start": f.start + 1,
        "end": f.end + 1,
        "length": f.span(construct.len()),
        "strand": if f.strand == Strand::Reverse { "reverse" } else { "forward" },
        "location": genbank_write::location_of(f, construct.circular, construct.len()),
    });
    if let (Some(id), Some(map)) = (refs::bios_note(f, "id"), out.as_object_mut()) {
        map.insert("bios_id".into(), json!(id));
    }
    // What it acts on (set_feature_actions), its targets by name.
    let acts = bios_core::acts::actions_of(f);
    if let (false, Some(map)) = (acts.is_empty(), out.as_object_mut()) {
        let name_of = |t: &str| match t {
            bios_core::acts::FOR_CONSTRUCT => "this construct".to_string(),
            id => construct
                .features
                .iter()
                .find(|g| bios_core::acts::feature_id(g) == Some(id))
                .map_or_else(|| "something removed".to_string(), |g| g.name.clone()),
        };
        let shown: Vec<Value> =
            acts.iter().map(|a| json!({ "verb": a.verb, "targets": a.targets.iter().map(|t| name_of(t)).collect::<Vec<_>>() })).collect();
        map.insert("acts_on".into(), json!(shown));
    }
    let qualifiers: Vec<String> = f
        .notes
        .iter()
        .flat_map(|(key, values)| values.iter().filter(move |v| !(key == "note" && v.starts_with("bios-"))).map(move |v| format!("/{key}={v}")))
        .collect();
    if let (false, Some(map)) = (qualifiers.is_empty(), out.as_object_mut()) {
        map.insert("qualifiers".into(), json!(qualifiers));
    }
    out
}

/// Whether the command acts on Bios itself rather than on a project.
pub(crate) fn is_app_wide(name: &str) -> bool {
    update::COMMANDS.iter().chain(window::COMMANDS).any(|c| c.name == name)
}
