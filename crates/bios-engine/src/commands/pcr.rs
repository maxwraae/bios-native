//! PCR: the product two primers make on a plasmid, predicted and, when asked, kept as a new
//! plasmid that records which plasmid it was amplified from.

use bios_core::pcr::{PcrInput, PcrPrediction, PcrPrimer, PcrProduct, PcrProductPrimerSite, predict_pcr_product};
use bios_core::primer::{PrimerSearchOptions, PrimerSite, find_primer_sites};
use bios_core::primer_binding::PrimerBindingCheck;
use bios_core::{Construct, Feature, Segment, Strand};
use serde_json::{Value, json};

use super::files::created;
use super::primers::{find_primer, oligo};
use super::read::gc_percent;
use super::sequence::{bases, new_bios_id};
use super::{PLASMID, feature_summary};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const FORWARD: Param = Param {
    name: "forward",
    kind: Kind::String,
    required: true,
    description: "One primer: the name, #index or bios id of a primer on the plasmid, or an oligo 5' to 3'. An oligo may carry a 5' tail that does not bind; it is found where its 3' end binds and must bind in one place.",
};
const REVERSE: Param = Param {
    name: "reverse",
    kind: Kind::String,
    required: true,
    description: "The other primer, given the same way. It must read along the opposite strand.",
};

const FORWARD_TAIL: Param = Param {
    name: "forward_tail",
    kind: Kind::Integer,
    required: false,
    description: "How many bases at the 5' end of the forward oligo are a tail that does not bind. Needed when it has one: a tail is never assumed. Leave out for a primer on the plasmid or an oligo that binds along its whole length.",
};
const REVERSE_TAIL: Param = Param { name: "reverse_tail", kind: Kind::Integer, required: false, description: "The same for the reverse oligo." };

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "simulate_pcr",
        description: "Predict the product two primers make on a plasmid: its sequence and length, the stretch of the plasmid it copies, and how each primer binds (tail, mismatches, Tm of the part that binds). Tails and mismatches in the primers end up in the product. Through the origin of a circular plasmid is fine. Nothing is saved; use save_pcr_product to keep the product. It predicts a sequence, not whether the reaction will work at the bench.",
        params: &[PLASMID, FORWARD, REVERSE, FORWARD_TAIL, REVERSE_TAIL],
        effect: Effect::Read,
        run: Run::Call(simulate_pcr),
    },
    CommandSpec {
        name: "save_pcr_product",
        description: "Run simulate_pcr and keep the product as a new linear plasmid. It records which plasmid it was amplified from, carries along every feature that lies whole inside the amplified stretch, and has the two primers on its ends.",
        params: &[
            PLASMID,
            FORWARD,
            REVERSE,
            FORWARD_TAIL,
            REVERSE_TAIL,
            Param { name: "name", kind: Kind::String, required: true, description: "Name of the new plasmid." },
            Param { name: "folder", kind: Kind::String, required: false, description: "Folder for it. Default: the top of the project." },
        ],
        effect: Effect::Write,
        run: Run::Call(save_pcr_product),
    },
];

/// One primer as the caller gave it, resolved to an oligo and the place it binds.
fn chosen(c: &Construct, args: &Args, role: &str) -> Result<PcrPrimer, Failure> {
    let given = args.str(role)?;
    let wanted = given.trim();
    let stated_tail = args.opt_int(&format!("{role}_tail"))?;
    let on_plasmid = |f: &Feature| f.kind == "primer_bind" && (f.name.eq_ignore_ascii_case(wanted) || refs::bios_note(f, "id") == Some(wanted));
    if wanted.starts_with('#') || c.features.iter().any(on_plasmid) {
        let index = find_primer(c, wanted)?;
        let Some(f) = c.features.get(index) else {
            return fail("that primer is gone");
        };
        if stated_tail.is_some_and(|tail| tail != 0) {
            return fail(format!("{} is a primer on the plasmid, which is its footprint and has no tail; leave {role}_tail out", f.name));
        }
        return Ok(PcrPrimer {
            id: refs::bios_note(f, "id").map(str::to_owned),
            name: Some(f.name.clone()),
            bases: oligo(c, f),
            start: f.start as i64,
            end: f.end as i64,
            forward: f.strand != Strand::Reverse,
            ..PcrPrimer::default()
        });
    }
    let Ok(oligo) = bases(wanted) else {
        return fail(format!("{role}: \"{wanted}\" is not a primer on this plasmid and holds no bases"));
    };
    if !oligo.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
        return fail(format!("{role}: \"{wanted}\" is not a primer on this plasmid, and an oligo may only hold A, C, G and T"));
    }
    let sites = find_primer_sites(&c.sequence, &oligo, PrimerSearchOptions { circular: c.circular, ..PrimerSearchOptions::default() })
        .map_err(|e| Failure(e.to_string()))?;
    let site = one_site(&sites, role)?;
    // A tail found by the search is a guess about what the person meant, and a guess does not
    // decide what ends up in a product. They say how long it is, and it has to agree.
    match (site.tail_length, stated_tail.unwrap_or(0)) {
        (found, stated) if i64::try_from(found).is_ok_and(|found| found == stated) => {}
        (0, _) => return fail(format!("the {role} oligo binds along its whole length at {}; leave {role}_tail out", place(site))),
        (found, 0) => {
            return fail(format!(
                "the {role} oligo binds at {} with its first {found} bases ({}) left over as a 5' tail. If that tail is meant, call again with {role}_tail: {found}",
                place(site),
                oligo.get(..found).unwrap_or_default()
            ));
        }
        (found, stated) => return fail(format!("{role}_tail is {stated}, but the {role} oligo binds at {} with a tail of {found}", place(site))),
    }
    Ok(PcrPrimer {
        name: Some(format!("{role} primer")),
        bases: oligo,
        start: site.start as i64,
        end: site.end as i64,
        forward: site.forward,
        tail_length: site.tail_length,
        ..PcrPrimer::default()
    })
}

fn place(site: &PrimerSite) -> String {
    format!("{}..{} {}", site.start + 1, site.end + 1, if site.forward { "forward" } else { "reverse" })
}

/// The one place an oligo binds. An exact site wins over places it binds only with mismatches.
fn one_site<'a>(sites: &'a [PrimerSite], role: &str) -> Result<&'a PrimerSite, Failure> {
    let exact: Vec<&PrimerSite> = sites.iter().filter(|s| s.mismatches == 0).collect();
    match (exact.as_slice(), sites) {
        ([one], _) => Ok(one),
        ([], [one]) => Ok(one),
        (_, []) => fail(format!(
            "the {role} primer does not bind this plasmid: its 3' end needs at least 10 bases that pair, with a Tm of at least 40 °C for the part that binds"
        )),
        (several, all) => {
            let shown: Vec<String> = if several.is_empty() { all.iter().map(place).collect() } else { several.iter().map(|s| place(s)).collect() };
            fail(format!(
                "the {role} primer binds in {} places ({}). Add it where you mean with add_primer and give its name instead",
                shown.len(),
                shown.join(", ")
            ))
        }
    }
}

struct Predicted {
    product: PcrProduct,
    report: Value,
}

fn binding_report(primer: &PcrProductPrimerSite, check: &PrimerBindingCheck, on_template: &PcrPrimer) -> Value {
    json!({
        "name": primer.name,
        "bases": check.bases,
        "binds": format!("{}..{}", on_template.start + 1, on_template.end + 1),
        "strand": if on_template.forward { "forward" } else { "reverse" },
        "tail": check.tail,
        "mismatches": check.mismatch_positions.len(),
        "tm_of_binding_part": check.annealing_tm.map(|tm| (tm * 10.0).round() / 10.0),
        "summary": check.summary,
    })
}

fn predict(c: &Construct, args: &Args) -> Result<Predicted, Failure> {
    let (mut forward, mut reverse) = (chosen(c, args, "forward")?, chosen(c, args, "reverse")?);
    let mut notes: Vec<String> = Vec::new();
    if forward.forward == reverse.forward {
        return fail("both primers read along the same strand, so nothing is amplified between them. One must read forward and the other reverse");
    }
    if !forward.forward {
        std::mem::swap(&mut forward, &mut reverse);
        notes.push("The primer given as forward reads along the bottom strand, so the two were taken the other way round.".into());
    }
    let input = PcrInput { sequence: c.sequence.clone(), circular: c.circular, forward, reverse };
    match predict_pcr_product(&input) {
        PcrPrediction::Refused { reasons, .. } => fail(format!("No product. {}", reasons.join(" "))),
        PcrPrediction::Product { product, caveats, bindings } => {
            notes.extend(caveats);
            let arc = product.template_arc;
            let report = json!({
                "length": product.sequence.len(),
                "gc": gc_percent(&product.sequence),
                "copies": format!("{}..{}", arc.start + 1, arc.end + 1),
                "through_origin": arc.wraps_origin,
                "forward": binding_report(&product.primer_sites.forward, &bindings.forward, &input.forward),
                "reverse": binding_report(&product.primer_sites.reverse, &bindings.reverse, &input.reverse),
                "notes": notes,
                "sequence": product.sequence,
            });
            Ok(Predicted { product: *product, report })
        }
    }
}

fn simulate_pcr(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let predicted = predict(&loaded.construct, args)?;
    Ok(json!({ "plasmid": refs::label(&loaded.path), "product": predicted.report }))
}

/// The features of `template` that lie whole and unbroken inside the product, moved onto it.
fn carried(template: &Construct, product: &PcrProduct) -> Vec<Feature> {
    let length = template.len();
    let map = &product.template_map;
    let whole_at = |f: &Feature, at: usize| {
        let span = f.span(length);
        (0..span).all(|k| at.checked_add(k).and_then(|i| map.get(i)).is_some_and(|base| *base == Some((f.start + k) % length)))
    };
    let mut out = Vec::new();
    for f in template.features.iter().filter(|f| f.kind != "primer_bind" && f.locations().is_none() && f.location_unsupported.is_none()) {
        let starts = map.iter().enumerate().filter(|(_, base)| **base == Some(f.start)).map(|(i, _)| i);
        if let Some(at) = starts.into_iter().find(|at| whole_at(f, *at)) {
            let end = at + f.span(length).saturating_sub(1);
            let mut moved = f.clone();
            (moved.start, moved.end, moved.segments) = (at, end, vec![Segment::new(at, end)]);
            out.push(moved);
        }
    }
    out
}

fn primer_feature(site: &PcrProductPrimerSite, fallback: &str) -> Feature {
    Feature {
        name: site.name.clone().unwrap_or_else(|| fallback.to_string()),
        kind: "primer_bind".into(),
        start: site.start,
        end: site.end,
        strand: if site.forward { Strand::Forward } else { Strand::Reverse },
        segments: vec![Segment::new(site.start, site.end)],
        notes: vec![("note".into(), vec![new_bios_id()])],
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

fn save_pcr_product(session: &Session, args: &Args) -> Result<Value, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let loaded = session.load(args)?;
    let template = &loaded.construct;
    let Predicted { product, report } = predict(template, args)?;
    let mut features = carried(template, &product);
    features.push(primer_feature(&product.primer_sites.forward, "forward primer"));
    features.push(primer_feature(&product.primer_sites.reverse, "reverse primer"));
    let construct = Construct {
        name: name.clone(),
        circular: false,
        sequence: product.sequence.clone(),
        features,
        locus: bios_core::model::Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    };
    let kept: Vec<Value> = construct.features.iter().enumerate().map(|(i, f)| feature_summary(&construct, i, f)).collect();
    let label = refs::label(&loaded.path);
    let summary = format!("PCR product of {} ({} bp)", template.name, construct.len());
    let folder = args.opt_str("folder")?.unwrap_or_default();
    let mut made = created(session, "save_pcr_product", construct, &folder, Some((&loaded.id, label)), summary, args)?;
    if let Some(map) = made.as_object_mut() {
        map.insert("product".into(), report);
        map.insert("features".into(), json!(kept));
    }
    Ok(made)
}
