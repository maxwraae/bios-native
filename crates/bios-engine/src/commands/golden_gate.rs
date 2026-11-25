//! Golden Gate assembly: one type IIS enzyme, a destination vector and the parts to put in it;
//! the overhangs the enzyme leaves, the order they force, the pieces to order, and, when asked,
//! the product kept as a new plasmid.

use bios_core::Construct;
use bios_core::golden_gate::{GoldenGateDesign, GoldenGateInput, Part, design_golden_gate as design_core};
use bios_core::model::Segment;
use serde_json::{Value, json};

use super::files::created;
use super::sequence::{bases, in_range, slice, span_len};
use super::{PLASMID, feature_summary};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const ENZYME: Param = Param {
    name: "enzyme",
    kind: Kind::String,
    required: false,
    description: "The type IIS enzyme that opens the vector and releases the parts. Default BsaI. BsmBI (Esp3I), BbsI (BpiI), SapI (BspQI), AarI and BtgZI work too; the overhang comes from the enzyme, so SapI's is three bases and not four.",
};
const PARTS: Param = Param {
    name: "parts",
    kind: Kind::Objects,
    required: true,
    description: "The parts, each either bases — [{\"name\": \"GFP\", \"sequence\": \"AATG...GCTT\"}] — or a stretch of another plasmid — [{\"name\": \"GFP\", \"plasmid\": \"parts/pGFP\", \"start\": 101, \"end\": 820}], whose features inside the stretch come along. Each part runs from its first fusion base to its last and carries NO type IIS tails: its first and last bases ARE its two overhangs, and this command works out the tails. The order of the list does not matter; the overhangs decide the order.",
};
const DESIGN: &[Param] = &[PLASMID, ENZYME, PARTS];

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "design_golden_gate",
        description: "Design a Golden Gate / MoClo-style type IIS assembly: a destination vector, one type IIS enzyme (BsaI by default) and the parts that go into it, all cut and ligated in one tube. This is the tool for that whole job — do not work out overhangs or assembly order yourself. Bios reads the enzyme's cut geometry from its own enzyme table, works out the overhang the digest leaves on the vector and on each part, matches them up, and returns: the order the overhangs force, the product's sequence and length, every junction with its overhang and where it lands, the vector's features at their new positions, and each part with the type IIS tails it needs, ready to paste into an order form. The warnings matter as much as the answer and each one names the part and the overhang: a site for the enzyme inside a part or left on the product (which would cut the assembly apart again), two junctions sharing an overhang (so the order is ambiguous), a palindromic overhang (so a part can go in backwards), an overhang nothing matches, and parts that form no single circular product. Nothing is saved; use save_golden_gate_product to keep the product.",
        params: DESIGN,
        effect: Effect::Read,
        run: Run::Call(design_golden_gate),
    },
    CommandSpec {
        name: "save_golden_gate_product",
        description: "Run design_golden_gate and keep the assembled product as a new plasmid. It records which destination vector it was built from, carries that vector's features that survive the opening, and has a feature for each part at its place in the product. Refused if the parts do not form one circular product.",
        params: &[
            PLASMID,
            ENZYME,
            PARTS,
            Param { name: "name", kind: Kind::String, required: true, description: "Name of the new plasmid." },
            Param { name: "folder", kind: Kind::String, required: false, description: "Folder for it. Default: the top of the project." },
        ],
        effect: Effect::Write,
        run: Run::Call(save_golden_gate_product),
    },
];

/// One part, from bases given straight or from a stretch of another plasmid in the project.
fn parts(session: &Session, args: &Args) -> Result<Vec<Part>, Failure> {
    let rows = args.objects("parts");
    if rows.is_empty() {
        return fail("give at least one part, as [{\"name\": ..., \"sequence\": ...}]");
    }
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let given = |key: &str| row.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
        let name = given("name").map(str::to_owned);
        if let Some(source) = given("plasmid") {
            out.push(from_plasmid(session, source, row, name, i)?);
            continue;
        }
        let Some(sequence) = given("sequence") else {
            return fail(format!("part {} has neither sequence nor plasmid with start and end", i + 1));
        };
        let seq = bases(sequence).map_err(|_| Failure(format!("part {} holds no bases", i + 1)))?;
        if !seq.bytes().all(|b| matches!(b.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T')) {
            return fail(format!("part {} may only hold A, C, G and T: a piece to order has to be exact", i + 1));
        }
        out.push(Part { name: name.unwrap_or_else(|| format!("Part {}", i + 1)), seq, features: Vec::new() });
    }
    Ok(out)
}

/// A part taken from a stretch of another plasmid, with the features that lie whole inside it.
fn from_plasmid(session: &Session, source: &str, row: &serde_json::Map<String, Value>, name: Option<String>, i: usize) -> Result<Part, Failure> {
    let listed = session.project().list()?;
    let loaded = session.project().read(&refs::plasmid(&listed, source)?.id)?;
    let from = &loaded.construct;
    let (Some(start), Some(end)) = (row.get("start").and_then(Value::as_i64), row.get("end").and_then(Value::as_i64)) else {
        return fail(format!("part {} names a plasmid, so it needs start and end too", i + 1));
    };
    in_range(from, start, end)?;
    let (length, size) = (from.len(), span_len(from, start, end) as usize);
    // A feature comes along when it lies whole, in one piece, inside the stretch, as
    // create_plasmid's own range copy does it.
    let offset = |x: usize| (x + length - (start - 1) as usize) % length;
    let features = from
        .features
        .iter()
        .filter(|f| f.locations().is_none() && f.location_unsupported.is_none())
        .filter_map(|f| {
            let (a, b) = (offset(f.start), offset(f.end));
            (a <= b && b < size && f.span(length) == b - a + 1).then(|| {
                let mut moved = f.clone();
                moved.start = a;
                moved.end = b;
                moved.segments = vec![Segment::new(a, b)];
                moved
            })
        })
        .collect();
    let name = name.unwrap_or_else(|| format!("{} {start}..{end}", from.name));
    Ok(Part { name, seq: slice(from, start, end), features })
}

fn design(session: &Session, construct: &Construct, name: &str, args: &Args) -> Result<GoldenGateDesign, Failure> {
    let mut input =
        GoldenGateInput::new(name, construct.sequence.clone(), args.opt_str("enzyme")?.unwrap_or_else(|| "BsaI".into()), parts(session, args)?);
    input.vector_circular = construct.circular;
    input.vector_features = construct.features.clone();
    design_core(&input).map_err(|e| Failure(e.to_string()))
}

/// The design as an agent reads it. Positions are 1-based and inclusive.
fn report(d: &GoldenGateDesign) -> Value {
    let span = |(start, end): (i64, i64)| format!("{start}..{end}");
    json!({
        "enzyme": {
            "name": d.geometry.enzyme,
            "site": d.geometry.site,
            "cuts": d.geometry.notation(),
            "overhang_length": d.geometry.overhang_len,
        },
        "vector": {
            "backbone": d.backbone.name,
            "opened": span(d.backbone.removed),
            "removed_length": d.backbone.removed_len,
            "through_origin": d.backbone.through_origin,
            "kept_length": d.backbone.length,
            "overhang_the_first_part_joins": d.backbone.right_overhang,
            "overhang_the_last_part_joins": d.backbone.left_overhang,
        },
        "assembly_order": d.order,
        "assembles": d.product.is_some(),
        "product": d.product.as_ref().map(|p| json!({
            "length": p.sequence.len(),
            "topology": if p.circular { "circular" } else { "linear" },
            "sequence": p.sequence,
        })),
        "junctions": d.junctions.iter().map(|j| json!({
            "between": format!("{} and {}", j.left, j.right),
            "overhang": j.overhang,
            "length": j.overhang.len(),
            "position": j.position.map(span),
            "unique": j.unique,
            "palindromic": j.palindromic,
        })).collect::<Vec<_>>(),
        "to_order": d.pieces.iter().map(|p| json!({
            "name": p.name,
            "part_length": p.part.len(),
            "left_overhang": p.left_overhang,
            "right_overhang": p.right_overhang,
            "tails": { "five_prime": p.five_prime_tail, "three_prime": p.three_prime_tail },
            "length": p.order.len(),
            "sequence": p.order,
        })).collect::<Vec<_>>(),
        "placed": d.product.as_ref().map(|p| p.placements.iter().map(|a| json!({
            "name": a.name, "start": a.start + 1, "end": a.end + 1,
        })).collect::<Vec<_>>()),
        "features": d.product.as_ref().map(|p| {
            let c = product_construct(String::new(), p);
            c.features.iter().enumerate().map(|(i, f)| feature_summary(&c, i, f)).collect::<Vec<_>>()
        }),
        "features_cut_short": d.product.as_ref().map(|p| p.carried.truncated.iter().map(|t| t.before.name.clone()).collect::<Vec<_>>()),
        "features_removed": d.product.as_ref().map(|p| p.carried.removed.iter().map(|f| f.name.clone()).collect::<Vec<_>>()),
        "warnings": d.warnings,
    })
}

fn product_construct(name: String, product: &bios_core::golden_gate::Product) -> Construct {
    Construct {
        name,
        circular: product.circular,
        sequence: product.sequence.clone(),
        features: product.features.clone(),
        locus: bios_core::model::Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    }
}

fn design_golden_gate(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let label = refs::label(&loaded.path);
    let designed = design(session, &loaded.construct, &loaded.construct.name, args)?;
    Ok(json!({ "plasmid": label, "design": report(&designed) }))
}

fn save_golden_gate_product(session: &Session, args: &Args) -> Result<Value, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let loaded = session.load(args)?;
    let vector = &loaded.construct;
    let designed = design(session, vector, &vector.name, args)?;
    let Some(product) = designed.product.as_ref() else {
        return fail(format!(
            "nothing was saved: the parts do not form one circular product. {}",
            if designed.warnings.is_empty() { "Check the overhangs.".to_string() } else { designed.warnings.join(" ") }
        ));
    };
    let construct = product_construct(name.clone(), product);
    let assembled: Vec<String> = product.placements.iter().map(|p| p.name.clone()).collect();
    let kept: Vec<Value> = construct.features.iter().enumerate().map(|(i, f)| feature_summary(&construct, i, f)).collect();
    let summary = format!("{} assembly of {} into {} ({} bp)", designed.geometry.enzyme, assembled.join(", "), vector.name, construct.len());
    let folder = args.opt_str("folder")?.unwrap_or_default();
    let label = refs::label(&loaded.path);
    let mut made = created(session, "save_golden_gate_product", construct, &folder, Some((&loaded.id, label)), summary, args)?;
    if let Some(map) = made.as_object_mut() {
        map.insert("design".into(), report(&designed));
        map.insert("features".into(), json!(kept));
    }
    Ok(made)
}
