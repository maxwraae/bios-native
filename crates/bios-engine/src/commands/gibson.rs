//! Gibson assembly: open a plasmid, design the pieces to order that put new DNA into the
//! opening, and, when asked, keep the assembled product as a new plasmid.

use bios_core::Construct;
use bios_core::enzymes::find_cut_sites;
use bios_core::gibson::{
    DesignGibsonMultiInput, DesignGibsonMultiResult, GibsonCut, GibsonCutSite, GibsonFragment, JunctionKind, OpeningArc, design_gibson_multi,
};
use serde_json::{Value, json};

use super::files::created;
use super::sequence::bases;
use super::{PLASMID, feature_summary};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const INSERTS: Param = Param {
    name: "inserts",
    kind: Kind::Objects,
    required: true,
    description: "The new DNA, in order 5' to 3', as [{\"name\": \"GFP\", \"sequence\": \"ATG...\"}]. One fragment or several; neighbours are joined to each other and the outer two to the plasmid.",
};
const ENZYMES: Param = Param {
    name: "enzymes",
    kind: Kind::Strings,
    required: false,
    description: "Open the plasmid between two different restriction enzymes, each of which cuts it once: [\"EcoRI\", \"BamHI\"]. The stretch between them leaves and both sites are kept on the product.",
};
const ARC: Param = Param {
    name: "arc",
    kind: Kind::String,
    required: false,
    description: "With enzymes on a circular plasmid, which stretch leaves: \"between\" (from the lower cut to the higher, the default) or \"origin\" (the other way round, through the origin).",
};
const REPLACE_START: Param = Param {
    name: "replace_start",
    kind: Kind::Integer,
    required: false,
    description: "Or replace a stretch: its first base. With replace_end. start > end runs through the origin of a circular plasmid.",
};
const REPLACE_END: Param =
    Param { name: "replace_end", kind: Kind::Integer, required: false, description: "The last base of the stretch to replace." };
const INSERT_AFTER: Param = Param {
    name: "insert_after",
    kind: Kind::Integer,
    required: false,
    description: "Or insert without removing anything: the base the new DNA goes after (0 puts it before the first base).",
};
const TARGET_TM: Param =
    Param { name: "target_tm", kind: Kind::Integer, required: false, description: "Melting temperature the overlaps aim for, in °C. Default 65." };
const MAX_PIECE: Param = Param {
    name: "max_piece_length",
    kind: Kind::Integer,
    required: false,
    description: "Longest piece that can be ordered, in bases. A longer insert is split into overlapping pieces. Default 3000.",
};
const DESIGN: &[Param] = &[PLASMID, INSERTS, ENZYMES, ARC, REPLACE_START, REPLACE_END, INSERT_AFTER, TARGET_TM, MAX_PIECE];

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "design_gibson",
        description: "Design a Gibson assembly that puts new DNA into a plasmid. Say where the plasmid opens (two enzymes, a stretch to replace, or a place to insert) and give the fragments. Returns the pieces to order with their homology arms, each junction's overlap and Tm, what happens to the plasmid's features, whether the enzyme sites survive, and warnings (pieces a supplier would refuse, overlaps that are not unique). Nothing is saved; use save_gibson_design to keep the assembled plasmid.",
        params: DESIGN,
        effect: Effect::Read,
        run: Run::Call(design_gibson),
    },
    CommandSpec {
        name: "save_gibson_design",
        description: "Run design_gibson and keep the assembled product as a new plasmid. It records which plasmid it was built from, carries that plasmid's features that survive the opening, and has a feature for each inserted fragment.",
        params: &[
            PLASMID,
            INSERTS,
            ENZYMES,
            ARC,
            REPLACE_START,
            REPLACE_END,
            INSERT_AFTER,
            TARGET_TM,
            MAX_PIECE,
            Param { name: "name", kind: Kind::String, required: true, description: "Name of the new plasmid." },
            Param { name: "folder", kind: Kind::String, required: false, description: "Folder for it. Default: the top of the project." },
        ],
        effect: Effect::Write,
        run: Run::Call(save_gibson_design),
    },
];

/// Where the plasmid opens, from whichever of the three ways the caller chose.
fn opening(c: &Construct, args: &Args) -> Result<GibsonCut, Failure> {
    let length = c.len() as i64;
    let enzymes = args.strings("enzymes");
    let region = args.has("replace_start") || args.has("replace_end");
    let ways = usize::from(!enzymes.is_empty()) + usize::from(region) + usize::from(args.has("insert_after"));
    if ways != 1 {
        return fail("say where the plasmid opens in one way: enzymes, or replace_start with replace_end, or insert_after");
    }
    if let Some(after) = args.opt_int("insert_after")? {
        if !(0..=length).contains(&after) {
            return fail(format!("insert_after is {after}, but the plasmid has {length} bases"));
        }
        return Ok(GibsonCut::InsertAt(after));
    }
    if region {
        let (start, end) = (args.int("replace_start")?, args.int("replace_end")?);
        if !(1..=length).contains(&start) || !(1..=length).contains(&end) {
            return fail(format!("replace_start and replace_end must be between 1 and {length}"));
        }
        if end < start && !c.circular {
            return fail("that stretch runs through the origin, but this plasmid is linear");
        }
        return Ok(GibsonCut::Region { start: start - 1, end: end - 1 });
    }
    let [a, b] = enzymes.as_slice() else {
        return fail("give two enzymes to open the plasmid between");
    };
    if a.eq_ignore_ascii_case(b) {
        return fail("give two different enzymes");
    }
    let names: Vec<&str> = vec![a.as_str(), b.as_str()];
    let found = find_cut_sites(&c.sequence, &names, c.circular);
    let mut pair: Vec<String> = Vec::new();
    for wanted in [a, b] {
        // The table's own spelling of the name, so "ecori" and "EcoRI" are the same enzyme.
        let Some(known) = bios_core::enzymes::get_enzyme(wanted) else {
            return fail(format!("{wanted} is not an enzyme Bios knows"));
        };
        match found.iter().filter(|s| s.enzyme == known.name).count() {
            1 => pair.push(known.name.to_string()),
            0 => return fail(format!("{} does not cut this plasmid", known.name)),
            n => return fail(format!("{} cuts {n} times. Pick an enzyme that cuts once", known.name)),
        }
    }
    let arc = match args.opt_str("arc")?.as_deref() {
        None => None,
        Some("between") => Some(OpeningArc::Between),
        Some("origin") if c.circular => Some(OpeningArc::Origin),
        Some("origin") => return fail("the stretch through the origin only exists on a circular plasmid"),
        Some(other) => return fail(format!("arc is \"between\" or \"origin\", not \"{other}\"")),
    };
    let sites = found
        .iter()
        .map(|s| GibsonCutSite { enzyme: s.enzyme.to_string(), position: s.top_cut as i64, recognition_start: Some(s.recognition_start as i64) })
        .collect();
    let [first, second] = pair.as_slice() else {
        return fail("give two enzymes to open the plasmid between");
    };
    Ok(GibsonCut::Enzymes { enzymes: [first.clone(), second.clone()], sites, arc, keep_sites: true })
}

pub(super) fn fragments(args: &Args) -> Result<Vec<GibsonFragment>, Failure> {
    let mut out = Vec::new();
    for (i, row) in args.objects("inserts").iter().enumerate() {
        let Some(sequence) = row.get("sequence").and_then(Value::as_str) else {
            return fail(format!("insert {} has no sequence", i + 1));
        };
        let seq = bases(sequence).map_err(|_| Failure(format!("insert {} holds no bases", i + 1)))?;
        if !seq.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
            return fail(format!("insert {} may only hold A, C, G and T: a piece to order has to be exact", i + 1));
        }
        let name = row.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
        out.push(GibsonFragment { name: name.map_or_else(|| format!("Insert {}", i + 1), str::to_owned), seq });
    }
    if out.is_empty() {
        return fail("give at least one insert, as [{\"name\": ..., \"sequence\": ...}]");
    }
    Ok(out)
}

fn design(c: &Construct, args: &Args) -> Result<DesignGibsonMultiResult, Failure> {
    design_with(c, opening(c, args)?, fragments(args)?, args.opt_int("target_tm")?, args.opt_int("max_piece_length")?)
}

/// The design itself, once the opening and the fragments are known. Build calls this once per
/// insert; one design that fails is then one row that says why, not the whole call.
pub(super) fn design_with(
    c: &Construct,
    cut: GibsonCut,
    inserts: Vec<GibsonFragment>,
    target_tm: Option<i64>,
    max_piece: Option<i64>,
) -> Result<DesignGibsonMultiResult, Failure> {
    let mut input = DesignGibsonMultiInput::new(c.sequence.clone(), c.circular, cut, inserts);
    input.backbone_features = c.features.clone();
    // What the web app turns on for a design that goes to the bench: one temperature for every junction.
    input.match_overlaps = true;
    if let Some(tm) = target_tm {
        if !(40..=80).contains(&tm) {
            return fail("target_tm must be between 40 and 80 °C");
        }
        input.target_tm = tm as f64;
    }
    if let Some(cap) = max_piece {
        if !(100..=20_000).contains(&cap) {
            return fail("max_piece_length must be between 100 and 20000");
        }
        input.max_eblock_len = cap;
    }
    design_gibson_multi(&input).map_err(|e| Failure(e.to_string()))
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The design as an agent reads it. Positions are 1-based and inclusive.
fn report(backbone: &Construct, d: &DesignGibsonMultiResult) -> Value {
    let length = backbone.len() as i64;
    let removed = if d.cut.removed_len == 0 {
        format!("nothing; the new DNA goes after base {}", d.cut.up)
    } else {
        format!("{}..{}", d.cut.up + 1, if d.cut.down == 0 { length } else { d.cut.down })
    };
    let names = |features: &[bios_core::Feature]| features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    json!({
        "product": { "length": d.assembled.sequence.len(), "topology": if d.assembled.circular { "circular" } else { "linear" } },
        "opening": {
            "removed": removed,
            "removed_length": d.cut.removed_len,
            "through_origin": d.cut.wraps,
            "enzymes": d.cut.enzymes,
            "sites_kept": d.cut.kept_sites,
        },
        "order": d.eblocks.iter().map(|e| json!({
            "name": e.name, "fragment": e.fragment, "length": e.length, "gc": e.gc, "sequence": e.order,
        })).collect::<Vec<_>>(),
        "junctions": d.junctions.iter().map(|j| json!({
            "between": format!("{} and {}", j.left, j.right),
            "kind": match j.kind {
                JunctionKind::BackboneInsert => "plasmid to insert",
                JunctionKind::InsertInsert => "insert to insert",
                JunctionKind::InsertBackbone => "insert to plasmid",
            },
            "overlap": j.seq, "length": j.seq.len(), "tm": round1(j.tm), "unique": j.unique,
        })).collect::<Vec<_>>(),
        "placed": d.placements.iter().map(|p| json!({ "name": p.name, "start": p.start + 1, "end": p.end + 1 })).collect::<Vec<_>>(),
        "features": {
            "kept": d.assembled.carry.carried.len(),
            "cut_short": d.assembled.carry.truncated.iter().map(|t| t.before.name.clone()).collect::<Vec<_>>(),
            "removed": names(&d.assembled.carry.removed),
        },
        "enzyme_sites": d.sites.iter().map(|s| json!({ "enzyme": s.enzyme, "cuts_product": s.on_product, "note": s.note })).collect::<Vec<_>>(),
        "warnings": d.warnings,
    })
}

fn design_gibson(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let designed = design(&loaded.construct, args)?;
    Ok(json!({ "plasmid": refs::label(&loaded.path), "design": report(&loaded.construct, &designed) }))
}

fn save_gibson_design(session: &Session, args: &Args) -> Result<Value, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let loaded = session.load(args)?;
    let backbone = &loaded.construct;
    let designed = design(backbone, args)?;
    let construct = Construct {
        name: name.clone(),
        circular: designed.assembled.circular,
        sequence: designed.assembled.sequence.clone(),
        features: designed.assembled.features.clone(),
        locus: bios_core::model::Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    };
    let inserted: Vec<String> = designed.placements.iter().map(|p| p.name.clone()).collect();
    let kept: Vec<Value> = construct.features.iter().enumerate().map(|(i, f)| feature_summary(&construct, i, f)).collect();
    let summary = format!("Gibson assembly of {} into {} ({} bp)", inserted.join(", "), backbone.name, construct.len());
    let folder = args.opt_str("folder")?.unwrap_or_default();
    let label = refs::label(&loaded.path);
    let mut made = created(session, "save_gibson_design", construct, &folder, Some((&loaded.id, label)), summary, args)?;
    if let Some(map) = made.as_object_mut() {
        map.insert("design".into(), report(backbone, &designed));
        map.insert("features".into(), json!(kept));
    }
    Ok(made)
}
