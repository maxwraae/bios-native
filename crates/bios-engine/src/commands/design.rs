//! Designing primers: a pair that amplifies a stretch, a pair that installs a change, and the
//! checks on an oligo someone already has. Nothing here writes: a designed primer is kept with
//! `add_primer`.

use bios_core::Construct;
use bios_core::binders::{Binder, binders_to_sites, site_label};
use bios_core::pcr::PcrPrediction;
use bios_core::primer_design::{
    DEFAULT_PRIMER_CRITERIA, DesignedPrimer, Mutagenesis, PrimerCriteria, PrimerPair, design_mutagenesis_primers, design_primer_pairs, pair_product,
};
use bios_core::primer_quality::primer_flags;
use bios_core::seq::{DNA_TM_DEFAULTS, clean_seq, tm};
use serde_json::{Value, json};

use super::PLASMID;
use super::read::gc_percent;
use super::sequence::bases;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const TARGET_TM: Param = Param {
    name: "target_tm",
    kind: Kind::Integer,
    required: false,
    description: "Melting temperature each primer aims for, in °C, of the part that anneals. Default 60.",
};
const MAX_TM_DIFFERENCE: Param = Param {
    name: "max_tm_difference",
    kind: Kind::Integer,
    required: false,
    description: "Most the two primers' melting temperatures may differ, in °C. Default 3. It is also the distance from the target past which a primer's own temperature is named in its reason.",
};
const MIN_LENGTH: Param =
    Param { name: "min_length", kind: Kind::Integer, required: false, description: "Shortest primer to consider, in bases. Default 18." };
const MAX_LENGTH: Param =
    Param { name: "max_length", kind: Kind::Integer, required: false, description: "Longest primer to consider, in bases. Default 25." };
const MIN_GC: Param = Param { name: "min_gc", kind: Kind::Integer, required: false, description: "Lowest wanted GC percentage. Default 40." };
const MAX_GC: Param = Param { name: "max_gc", kind: Kind::Integer, required: false, description: "Highest wanted GC percentage. Default 60." };
const GC_CLAMP: Param = Param {
    name: "gc_clamp",
    kind: Kind::Boolean,
    required: false,
    description: "Prefer a primer whose last base is G or C, which anchors extension. Default true. It is a preference: a primer without one is still offered, and its reason says so.",
};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "design_primers",
        description: "Design PCR primers for a stretch of a plasmid: give a feature, or start and end, and Bios works out the oligos to order. This is primer design — there is no need for any outside tool. Returns up to 5 candidate pairs, the best first: each primer's sequence, where it binds, its melting temperature, GC%, length and any quality warnings (no 3' G/C clamp, a long single-base run, self-complementarity), plus one line saying why the pair ranks where it does. Both primers' 5' ends are pinned to the ends of the stretch asked for, so every pair makes the same product; it is reported once, with its length and sequence. A 5' tail can be added to either primer for a cloning site or an overhang: it is part of the oligo and ends up in the product, and is left out of the annealing temperature. These are candidates ranked by well-known criteria (nearest-neighbour Tm, SantaLucia 1998, 250 nM strands, 50 mM Na+), not a promise that the reaction will amplify at the bench. To keep a pair on the plasmid, call add_primer twice with the start, end and strand reported here.",
        params: &[
            PLASMID,
            Param {
                name: "feature",
                kind: Kind::String,
                required: false,
                description: "The feature to amplify, named as in read_plasmid. Or give start and end.",
            },
            Param { name: "start", kind: Kind::Integer, required: false, description: "First base of the stretch to amplify (1-based, inclusive)." },
            Param {
                name: "end",
                kind: Kind::Integer,
                required: false,
                description: "Last base of it (inclusive). start > end runs through the origin of a circular plasmid.",
            },
            Param {
                name: "forward_tail",
                kind: Kind::String,
                required: false,
                description: "Bases to put on the 5' end of the forward primer (a restriction site, a Gibson overhang). They are not on the plasmid.",
            },
            Param { name: "reverse_tail", kind: Kind::String, required: false, description: "The same for the reverse primer." },
            TARGET_TM,
            MAX_TM_DIFFERENCE,
            MIN_LENGTH,
            MAX_LENGTH,
            MIN_GC,
            MAX_GC,
            GC_CLAMP,
            Param { name: "max_pairs", kind: Kind::Integer, required: false, description: "Most pairs to return, 1 to 20. Default 5." },
        ],
        effect: Effect::Read,
        run: Run::Call(design_primers),
    },
    CommandSpec {
        name: "design_mutagenesis_primers",
        description: "Design primers that install a change in a plasmid: one base swapped for another, a few bases put in, or a few taken out. Say where (start) and what (substitute, insert or delete). Bios returns a pair of mutagenic oligos — the forward one and its reverse complement, as whole-plasmid site-directed mutagenesis is primed — with the change in the middle and correct plasmid sequence on both sides, grown until the oligo reaches the target melting temperature. For each primer: sequence, where it sits, Tm, GC%, length and quality warnings; then what the edit does, the plasmid's bases before and after it, and the length of the product. \"binds\" is the stretch of the plasmid the oligo covers, which for an insertion or a deletion holds a different number of bases from the oligo itself. Candidates by well-known criteria, not a promise the reaction will work. Nothing is saved: make the change itself with replace_range, insert_sequence or delete_range.",
        params: &[
            PLASMID,
            Param {
                name: "start",
                kind: Kind::Integer,
                required: true,
                description: "Where the change is (1-based): the first base replaced or removed, or, for an insertion, the base the new DNA goes before.",
            },
            Param {
                name: "substitute",
                kind: Kind::String,
                required: false,
                description: "The bases that take the place of the same number of bases from start, such as \"G\" for one base.",
            },
            Param { name: "insert", kind: Kind::String, required: false, description: "Or the bases to put in before start, removing nothing." },
            Param { name: "delete", kind: Kind::Integer, required: false, description: "Or how many bases to remove from start onwards." },
            TARGET_TM,
            MAX_TM_DIFFERENCE,
            MIN_GC,
            MAX_GC,
            GC_CLAMP,
        ],
        effect: Effect::Read,
        run: Run::Call(mutagenesis_primers),
    },
    CommandSpec {
        name: "check_primer",
        description: "Check an oligo you already have: its length, GC%, melting temperature (nearest-neighbour, SantaLucia 1998, over every base of it) and the usual quality warnings — no 3' G/C clamp, a GC-rich 3' end, a run of five or more of one base, self-complementarity. Name a plasmid as well and it also says where the oligo binds it: every site with the Tm of the part that anneals there (anneal_tm_c, lower than the oligo's own Tm when a 5' tail is left over), which site is its full perfect landing, and whether it binds in more than one place, which is what makes a primer unusable.",
        params: &[
            Param { name: "sequence", kind: Kind::String, required: true, description: "The oligo, 5' to 3'." },
            Param {
                name: "plasmid",
                kind: Kind::String,
                required: false,
                description: "The plasmid to look for it on. Leave out to check the oligo on its own.",
            },
            Param { name: "name", kind: Kind::String, required: false, description: "What to call it in the answer. Default \"oligo\"." },
        ],
        effect: Effect::Read,
        run: Run::Call(check_primer),
    },
];

/// The criteria from whatever the caller named, the rest at [`DEFAULT_PRIMER_CRITERIA`].
fn criteria(args: &Args) -> Result<PrimerCriteria, Failure> {
    let mut c = DEFAULT_PRIMER_CRITERIA;
    if let Some(tm) = args.opt_int("target_tm")? {
        if !(30..=85).contains(&tm) {
            return fail("target_tm must be between 30 and 85 °C");
        }
        c.target_tm = tm as f64;
    }
    if let Some(difference) = args.opt_int("max_tm_difference")? {
        if !(0..=30).contains(&difference) {
            return fail("max_tm_difference must be between 0 and 30 °C");
        }
        c.max_tm_difference = difference as f64;
    }
    if let Some(min) = args.opt_int("min_length")? {
        if !(10..=100).contains(&min) {
            return fail("min_length must be between 10 and 100 bases");
        }
        c.min_length = min as usize;
    }
    if let Some(max) = args.opt_int("max_length")? {
        if !(10..=100).contains(&max) {
            return fail("max_length must be between 10 and 100 bases");
        }
        c.max_length = max as usize;
    }
    if c.max_length < c.min_length {
        return fail("max_length is below min_length");
    }
    for (name, slot) in [("min_gc", &mut c.min_gc), ("max_gc", &mut c.max_gc)] {
        if let Some(gc) = args.opt_int(name)? {
            if !(0..=100).contains(&gc) {
                return fail(format!("{name} must be between 0 and 100"));
            }
            *slot = gc as f64;
        }
    }
    if c.max_gc < c.min_gc {
        return fail("max_gc is below min_gc");
    }
    if let Some(wanted) = args.opt_bool("gc_clamp")? {
        c.prefer_gc_clamp = wanted;
    }
    if let Some(pairs) = args.opt_int("max_pairs")? {
        if !(1..=20).contains(&pairs) {
            return fail("max_pairs must be between 1 and 20");
        }
        c.max_pairs = pairs as usize;
    }
    Ok(c)
}

/// A 5' tail as the caller gave it: exact bases, or nothing.
fn tail(args: &Args, name: &str) -> Result<String, Failure> {
    let Some(given) = args.opt_str(name)?.filter(|t| !t.trim().is_empty()) else {
        return Ok(String::new());
    };
    let tail = bases(&given).map_err(|_| Failure(format!("{name} holds no bases")))?;
    if !tail.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
        return fail(format!("{name} may only hold A, C, G and T: an oligo to order has to be exact"));
    }
    if tail.len() > 60 {
        return fail(format!("{name} is {} bases; a 5' tail longer than 60 is not a primer tail", tail.len()));
    }
    Ok(tail)
}

/// The stretch to amplify, 0-based inclusive, from a feature or from start and end.
fn stretch(c: &Construct, args: &Args) -> Result<(usize, usize, Option<String>), Failure> {
    let length = c.len() as i64;
    match (args.opt_str("feature")?, args.opt_int("start")?, args.opt_int("end")?) {
        (Some(wanted), None, None) => {
            let index = refs::feature(c, &wanted)?;
            let Some(f) = c.features.get(index) else {
                return fail("that feature is gone");
            };
            Ok((f.start, f.end, Some(f.name.clone())))
        }
        (None, Some(start), Some(end)) => {
            if !(1..=length).contains(&start) || !(1..=length).contains(&end) {
                return fail(format!("start and end must be between 1 and {length}"));
            }
            Ok(((start - 1) as usize, (end - 1) as usize, None))
        }
        _ => fail("say what to amplify in one way: a feature, or both start and end"),
    }
}

fn primer_json(p: &DesignedPrimer) -> Value {
    json!({
        "sequence": p.sequence,
        "length": p.length,
        "binds": format!("{}..{}", p.start + 1, p.end + 1),
        "strand": if p.forward { "forward" } else { "reverse" },
        "anneals": p.anneal,
        "anneal_length": p.anneal.len(),
        "tail": p.tail,
        "tm_c": p.tm,
        "gc_percent": p.gc,
        "gc_clamp": p.flags.gc_clamp,
        "warnings": p.flags.warnings,
    })
}

/// The product every pair of one design makes. The two 5' ends are pinned, so it is the same
/// sequence for all of them; it is predicted through the same path as `simulate_pcr`.
fn product_json(c: &Construct, pair: &PrimerPair) -> Result<Value, Failure> {
    match pair_product(&c.sequence, c.circular, pair) {
        PcrPrediction::Refused { reasons, .. } => fail(format!("the designed pair makes no product, which is a bug in Bios: {}", reasons.join(" "))),
        PcrPrediction::Product { product, .. } => {
            let arc = product.template_arc;
            Ok(json!({
                "length": product.sequence.len(),
                "gc_percent": gc_percent(&product.sequence),
                "copies": format!("{}..{}", arc.start + 1, arc.end + 1),
                "through_origin": arc.wraps_origin,
                "sequence": product.sequence,
            }))
        }
    }
}

fn design_primers(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let (start, end, feature) = stretch(c, args)?;
    let wanted = criteria(args)?;
    let (forward_tail, reverse_tail) = (tail(args, "forward_tail")?, tail(args, "reverse_tail")?);
    let pairs =
        design_primer_pairs(&c.sequence, c.circular, start, end, &wanted, &forward_tail, &reverse_tail).map_err(|e| Failure(e.to_string()))?;
    let Some(best) = pairs.first() else {
        return fail("no pair of primers could be designed over this stretch");
    };
    let product = product_json(c, best)?;
    let shown: Vec<Value> = pairs
        .iter()
        .enumerate()
        .map(|(i, pair)| {
            json!({
                "rank": i + 1,
                "reason": pair.reason,
                "tm_difference": pair.tm_difference,
                "forward": primer_json(&pair.forward),
                "reverse": primer_json(&pair.reverse),
            })
        })
        .collect();
    let length = if end >= start { end - start + 1 } else { c.len() - start + end + 1 };
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "amplifies": {
            "feature": feature,
            "start": start + 1,
            "end": end + 1,
            "length": length,
            "through_origin": end < start,
        },
        "criteria": {
            "target_tm": wanted.target_tm,
            "max_tm_difference": wanted.max_tm_difference,
            "length": format!("{}-{}", wanted.min_length, wanted.max_length),
            "gc_percent": format!("{}-{}", wanted.min_gc, wanted.max_gc),
            "prefer_gc_clamp": wanted.prefer_gc_clamp,
        },
        "product": product,
        "pairs": shown,
    }))
}

/// The change the caller asked for, in one of the three ways.
fn change(args: &Args) -> Result<Mutagenesis, Failure> {
    let (substitute, insert, delete) = (args.opt_str("substitute")?, args.opt_str("insert")?, args.opt_int("delete")?);
    let ways = usize::from(substitute.is_some()) + usize::from(insert.is_some()) + usize::from(delete.is_some());
    if ways != 1 {
        return fail("say what the change is in one way: substitute, insert or delete");
    }
    if let Some(given) = substitute.or(insert.clone()) {
        let new = bases(&given)?;
        if !new.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
            return fail("the new bases may only be A, C, G and T: an oligo to order has to be exact");
        }
        if new.len() > 30 {
            return fail(format!("{} bases is more than mutagenic primers can carry; build that with design_gibson instead", new.len()));
        }
        return Ok(if insert.is_some() { Mutagenesis::insert(&new) } else { Mutagenesis::substitute(&new) });
    }
    match delete {
        Some(length) if (1..=30).contains(&length) => Ok(Mutagenesis::delete(length as usize)),
        Some(_) => fail("delete must be between 1 and 30 bases; take out more than that with delete_range and design_gibson"),
        None => fail("say what the change is in one way: substitute, insert or delete"),
    }
}

fn mutagenesis_primers(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let start = args.int("start")?;
    if !(1..=c.len() as i64).contains(&start) {
        return fail(format!("start must be between 1 and {}", c.len()));
    }
    let change = change(args)?;
    let wanted = criteria(args)?;
    let d = design_mutagenesis_primers(&c.sequence, c.circular, (start - 1) as usize, &change, &wanted).map_err(|e| Failure(e.to_string()))?;
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "edit": {
            "kind": change.kind(),
            "describes": d.describes,
            "at": start,
            "removes": d.changed_from,
            "installs": d.changed_to,
        },
        "reason": d.reason,
        "forward": primer_json(&d.forward),
        "reverse": primer_json(&d.reverse),
        "correct_bases_before_the_change": d.flank_5,
        "correct_bases_after_the_change": d.flank_3,
        "three_prime_flank_tm_c": d.flank_3_tm,
        "product": {
            "length": d.product.len(),
            "topology": if c.circular { "circular" } else { "linear" },
            "was": d.before_window,
            "becomes": d.after_window,
        },
    }))
}

fn check_primer(session: &Session, args: &Args) -> Result<Value, Failure> {
    let oligo = bases(&args.str("sequence")?)?;
    let name = args.opt_str("name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "oligo".to_string());
    let flags = primer_flags(&oligo);
    let mut out = json!({
        "name": name,
        "sequence": oligo,
        "length": oligo.len(),
        "gc_percent": gc_percent(&oligo),
        "tm_c": tm(&oligo, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm).map(|t| (t * 10.0).round() / 10.0),
        "gc_clamp": flags.gc_clamp,
        "longest_run": flags.long_run,
        "self_complementarity": flags.self_comp,
        "warnings": flags.warnings,
    });
    if !args.has("plasmid") {
        return Ok(out);
    }
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let binder = Binder { id: "oligo".to_string(), name: name.clone(), bases: clean_seq(&oligo) };
    let sites = binders_to_sites(std::slice::from_ref(&binder), &c.sequence, c.circular);
    let shown: Vec<Value> = sites
        .iter()
        .map(|s| {
            json!({
                "start": s.site.start + 1,
                "end": s.site.end + 1,
                "strand": if s.site.forward { "forward" } else { "reverse" },
                "through_origin": s.site.wraps,
                "tail_length": s.site.tail_length,
                "anneal_length": s.site.anneal_length,
                "mismatches": s.site.mismatches,
                "anneal_tm_c": s.site.tm.map(|t| (t * 10.0).round() / 10.0),
                "on_target": s.on_target,
                "partial": s.partial,
                "label": site_label(s),
            })
        })
        .collect();
    let full = sites.iter().filter(|s| !s.partial && s.site.mismatches == 0).count();
    if let Some(map) = out.as_object_mut() {
        map.insert("plasmid".into(), json!(refs::label(&loaded.path)));
        map.insert("sites".into(), json!(shown));
        map.insert("binds_more_than_once".into(), json!(full > 1));
        map.insert(
            "verdict".into(),
            json!(match (full, sites.len()) {
                (1, 1) => "binds once, in one place".to_string(),
                (1, all) => format!("binds once fully, with {} weaker site(s) as well", all - 1),
                (0, 0) => "does not bind this plasmid as a primer".to_string(),
                (0, all) => format!("no full landing: {all} site(s) anneal by only part of the oligo"),
                (full, _) => format!("binds equally well in {full} places, so it cannot be used as a primer here"),
            }),
        );
    }
    Ok(out)
}
