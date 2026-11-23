//! Looking at a construct.

use bios_core::{genbank_write, seq};
use serde_json::{Value, json};

use super::{PLASMID, feature_summary};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs, token};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "read_plasmid",
        description: "Read a plasmid: its version (needed to edit), a summary (length, topology, every feature with 1-based coordinates and strand), and by default the full GenBank text exactly as it is stored.",
        params: &[
            PLASMID,
            Param {
                name: "genbank",
                kind: Kind::Boolean,
                required: false,
                description: "Include the full GenBank text (default true). Set false for just the summary.",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(read_plasmid),
    },
    CommandSpec {
        name: "get_sequence",
        description: "Get bases from a plasmid. 1-based inclusive; start > end wraps the origin of a circular plasmid.",
        params: &[
            PLASMID,
            Param { name: "start", kind: Kind::Integer, required: false, description: "First base (default 1)." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "Last base (default the end)." },
            Param {
                name: "strand",
                kind: Kind::String,
                required: false,
                description: "forward (default) or reverse; reverse gives the reverse complement.",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(get_sequence),
    },
];

/// GC content as a percentage, to one decimal.
pub(crate) fn gc_percent(bases: &str) -> f64 {
    (seq::gc_content(bases) * 10.0).round() / 10.0
}

fn read_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let features: Vec<Value> = c.features.iter().enumerate().map(|(i, f)| feature_summary(c, i, f)).collect();
    let mut out = json!({
        "plasmid": refs::label(&loaded.path),
        "id": loaded.id.as_str(),
        "name": c.name,
        "version": token(&loaded),
        "length": c.len(),
        "topology": if c.circular { "circular" } else { "linear" },
        "gc_percent": gc_percent(&c.sequence),
        "features": features,
    });
    if args.opt_bool("genbank")? != Some(false)
        && let Some(map) = out.as_object_mut()
    {
        let text = genbank_write::write(c).map_err(|e| Failure(e.to_string()))?;
        map.insert("genbank".into(), json!(text));
    }
    Ok(out)
}

fn get_sequence(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let length = c.len() as i64;
    let start = args.opt_int("start")?.unwrap_or(1);
    let end = args.opt_int("end")?.unwrap_or(length);
    if start < 1 || end < 1 || start > length || end > length {
        return fail(format!("range must be within 1..{length}"));
    }
    if start > end && !c.circular {
        return fail("start after end only works on a circular plasmid");
    }
    let (s, e) = ((start - 1) as usize, end as usize);
    let forward = if start <= end {
        c.sequence.get(s..e).unwrap_or_default().to_string()
    } else {
        format!("{}{}", c.sequence.get(s..).unwrap_or_default(), c.sequence.get(..e).unwrap_or_default())
    };
    let reverse = match args.opt_str("strand")?.as_deref() {
        None | Some("forward") => false,
        Some("reverse") => true,
        Some(other) => return fail(format!("strand must be forward or reverse, not {other}")),
    };
    let bases = if reverse { seq::revcomp(&forward) } else { forward };
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "start": start,
        "end": end,
        "strand": if reverse { "reverse" } else { "forward" },
        "length": bases.len(),
        "gc_percent": gc_percent(&bases),
        "sequence": bases,
    }))
}
