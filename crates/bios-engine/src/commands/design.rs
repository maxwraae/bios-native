//! Designing primers: a pair that amplifies a stretch, a pair that installs a change, and the
//! checks on an oligo someone already has. Nothing here writes: a designed primer is kept with
//! `add_primer`.

use bios_core::Construct;
use bios_core::annealing::{ANNEALING_RULE, AnnealingTemperature};
use bios_core::binders::{Binder, binders_to_sites, site_label};
use bios_core::pcr::PcrPrediction;
use bios_core::primer_design::{
    DEFAULT_PRIMER_CRITERIA, DesignedPrimer, Mutagenesis, PrimerCriteria, PrimerPair, design_mutagenesis_primers, design_primer_pairs, pair_product,
};
use bios_core::primer_quality::{DimerCheck, HairpinCheck, cross_dimer, hairpins, primer_flags};
use bios_core::seq::{DNA_TM_DEFAULTS, clean_seq, tm};
use bios_core::sequencing::{DEFAULT_SEQUENCING_SETTINGS, SequencingPlan, SequencingSettings, StrandPlan, design_sequencing_primers};
use serde_json::{Value, json};

use super::PLASMID;
use super::primers::{find_primer, oligo};
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
/// `max_tm_difference` as a lone sequencing primer reads it: there is no partner to match, so it is
/// how far from the target the one primer may be.
const TM_TOLERANCE: Param = Param {
    name: "max_tm_difference",
    kind: Kind::Integer,
    required: false,
    description: "How far a primer's melting temperature may fall from the target, in °C. Default 3. A sequencing primer has no partner to match, so this is the gate: a candidate outside it is never offered, and a window holding nothing inside it comes back as a gap that says so. GC%, the 3' clamp and the quality flags then order the candidates that pass.",
};
const BOUNDARY_SLACK: Param = Param {
    name: "boundary_slack",
    kind: Kind::Integer,
    required: false,
    description: "How far each primer's 5' end may move OUTWARD from the end of the stretch asked for, in bases. Default 0, which pins both ends exactly. Raise it (20 to 40 is usual) where a boundary is GC-rich or AT-rich and no primer of any allowed length starting exactly there can reach the target Tm. Outward only, so the stretch asked for is always wholly inside the product; the product is longer by however far the ends moved, and each pair reports moved_forward and moved_reverse. Slack is taken only as far as it buys a better primer.",
};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "design_primers",
        description: "Design PCR primers for a stretch of a plasmid: give a feature, or start and end, and Bios works out the oligos to order. This is primer design — there is no need for any outside tool. Returns up to 5 candidate pairs, the best first: each primer's sequence, where it binds, its melting temperature, GC%, length, any quality warnings (no 3' G/C clamp, a long single-base run, self-complementarity) and its worst hairpin, plus one line saying why the pair ranks where it does. Every pair also carries the cross-dimer check between its two oligos — the longest complementary stretch anywhere and, separately, the longest that pairs a 3' end, which is the one a polymerase extends into primer-dimer instead of product — and the pair's `warnings`, which is the list to read before ordering. By default both primers' 5' ends are pinned to the ends of the stretch asked for, so every pair makes the same product, reported once with its length and sequence; boundary_slack loosens that. Each pair carries annealing_temperature_c, where to start a gradient: Rychlik, Spencer & Rhoads 1990, Ta = 0.3 × Tm(cooler primer) + 0.7 × Tm(product) − 14.9, the product Tm from the long-duplex %GC formula at 50 mM salt. It is a starting point for a gradient, not a guarantee, and knows nothing of the polymerase or its buffer; a hot enzyme with its own recommended temperature should be run at that one. A 5' tail can be added to either primer for a cloning site or an overhang: it is part of the oligo and ends up in the product, and is left out of the melting temperature. These are candidates ranked by well-known criteria (nearest-neighbour Tm, SantaLucia 1998, 250 nM strands, 50 mM Na+), not a promise that the reaction will amplify at the bench. To keep a pair on the plasmid, call add_primer twice with the start, end and strand reported here.",
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
            BOUNDARY_SLACK,
            Param { name: "max_pairs", kind: Kind::Integer, required: false, description: "Most pairs to return, 1 to 20. Default 5." },
        ],
        effect: Effect::Read,
        run: Run::Call(design_primers),
    },
    CommandSpec {
        name: "design_sequencing_primers",
        description: "Design Sanger sequencing primers tiled across a whole plasmid, or a range of one, so that the reads overlap and cover every base. Give the plasmid (and start/end for part of it) and Bios returns the primers in the order they read: sequence, where each binds, strand, Tm, GC%, length, quality warnings, the stretch it is expected to read, and how many bases it shares with the read before it. A read is counted from the base after the primer's 3' end, for read_length bases, so read_length is the length your sequencing provider quotes and overlap is what keeps the joins off the poor ends of a trace. Each primer anneals fully and perfectly in exactly ONE place: a primer that binds twice gives a trace of two sequences on top of each other, so Bios refuses it, names it under \"rejected\" with where it would have bound, and tries the next candidate. Anything that could not be covered comes back under \"gaps\", each with the reason — a repeat where every candidate binds twice, a tract too AT-rich for any oligo to reach the target Tm, or, at the start of a linear plasmid, bases that need a primer upstream because a read begins after its primer. Nothing is saved: keep a primer with add_primer.",
        params: &[
            PLASMID,
            Param {
                name: "start",
                kind: Kind::Integer,
                required: false,
                description: "First base to cover (1-based, inclusive). Leave start and end out for the whole plasmid.",
            },
            Param {
                name: "end",
                kind: Kind::Integer,
                required: false,
                description: "Last base to cover (inclusive). start > end runs through the origin of a circular plasmid.",
            },
            Param {
                name: "read_length",
                kind: Kind::Integer,
                required: false,
                description: "Bases one read is expected to resolve, counting from the base after the primer's 3' end. Default 800, which is what a good Sanger run gives. 50 to 2000.",
            },
            Param {
                name: "overlap",
                kind: Kind::Integer,
                required: false,
                description: "Bases each read shares with the one before it on the same strand. Default 100. It is a floor, not a wish: a primer is only ever taken at or behind its ideal position, so reads share at least this much. The one exception is where nothing acceptable sits there at all, and that read says what it actually shares.",
            },
            Param {
                name: "both_strands",
                kind: Kind::Boolean,
                required: false,
                description: "Tile the bottom strand as well, so every base is read twice, once each way. Default false. Each strand is reported with its own reads, coverage and gaps.",
            },
            Param {
                name: "search_window",
                kind: Kind::Integer,
                required: false,
                description: "How far back from its ideal position a primer may be taken when nothing there is acceptable. Default 150 bases. Widen it over a difficult plasmid; it costs more reads, never coverage.",
            },
            TARGET_TM,
            TM_TOLERANCE,
            MIN_LENGTH,
            MAX_LENGTH,
            MIN_GC,
            MAX_GC,
            GC_CLAMP,
        ],
        effect: Effect::Read,
        run: Run::Call(sequencing_primers),
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
        description: "Check an oligo you already have: its length, GC%, melting temperature (nearest-neighbour, SantaLucia 1998, over every base of it), the usual quality warnings — no 3' G/C clamp, a GC-rich 3' end, a run of five or more of one base, self-complementarity — and its hairpins: the sturdiest stem anywhere in it and, separately, any stem that pairs the 3'-terminal base, which is the one that stops the polymerase extending. Name a plasmid as well and it also says where the oligo binds it: every site with the Tm of the part that anneals there (anneal_tm_c, lower than the oligo's own Tm when a 5' tail is left over), which site is its full perfect landing, and whether it binds in more than one place, which is what makes a primer unusable. Give `against` as well — a second oligo, or the name of a primer on the plasmid — and it checks the two for cross-dimer: the longest complementary stretch anywhere between them and, separately, the longest that pairs a 3' end, which is the duplex a polymerase extends into primer-dimer instead of product, drawn out so it can be looked at. That is the check to run on a forward and reverse primer before ordering them.",
        params: &[
            Param { name: "sequence", kind: Kind::String, required: true, description: "The oligo, 5' to 3'." },
            Param {
                name: "plasmid",
                kind: Kind::String,
                required: false,
                description: "The plasmid to look for it on. Leave out to check the oligo on its own.",
            },
            Param { name: "name", kind: Kind::String, required: false, description: "What to call it in the answer. Default \"oligo\"." },
            Param {
                name: "against",
                kind: Kind::String,
                required: false,
                description: "A second oligo to check this one against for cross-dimer, 5' to 3'. Or, with a plasmid named, the name, #index or bios id of a primer on it, and its oligo is used. Leave out to check the one oligo alone.",
            },
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
    if let Some(slack) = args.opt_int("boundary_slack")? {
        if !(0..=200).contains(&slack) {
            return fail("boundary_slack must be between 0 and 200 bases");
        }
        c.boundary_slack = slack as usize;
    }
    Ok(c)
}

/// What a hairpin check found, for an agent. `null` for an oligo that folds on nothing.
fn hairpin_json(check: &HairpinCheck) -> Value {
    let shown = |h: &bios_core::primer_quality::Hairpin| {
        json!({
            "stem_base_pairs": h.stem,
            "loop_bases": h.loop_length,
            "stem_5_prime": format!("{}..{}", h.stem_5.0 + 1, h.stem_5.1 + 1),
            "stem_3_prime": format!("{}..{}", h.stem_3.0 + 1, h.stem_3.1 + 1),
            "pairs_the_3_prime_end": h.at_three_prime,
            "shape": h.shape,
        })
    };
    match check.worst.as_ref() {
        None => Value::Null,
        Some(worst) => json!({
            "worst": shown(worst),
            "on_the_3_prime_end": check.three_prime.as_ref().map(shown),
            "warnings": check.warnings,
        }),
    }
}

/// The cross-dimer between two oligos, for an agent. Positions do not come into it: a dimer is
/// between two oligos, not on the plasmid.
fn dimer_json(check: &DimerCheck) -> Value {
    json!({
        "longest_complementary_bases": check.longest,
        "three_prime_bases_first": check.three_prime_first,
        "three_prime_bases_second": check.three_prime_second,
        "diagram": check.diagram,
        "warnings": check.warnings,
    })
}

/// Where to start a gradient, with every term it was worked out from.
fn annealing_json(annealing: Option<AnnealingTemperature>) -> Value {
    match annealing {
        None => Value::Null,
        Some(a) => json!({
            "celsius": a.celsius,
            "cooler_primer_tm_c": a.primer_tm,
            "product_tm_c": a.product_tm,
            "product_length": a.product_length,
            "product_gc_percent": a.product_gc,
            "rule": ANNEALING_RULE,
        }),
    }
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
        "hairpin": hairpin_json(&p.hairpin),
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
    let mut product = product_json(c, best)?;
    if let Some(map) = product.as_object_mut().filter(|_| wanted.boundary_slack > 0) {
        map.insert(
            "note".into(),
            json!("With boundary_slack the pairs do not all make the same product. This is the best pair's; each pair reports its own amplifies and product_length."),
        );
    }
    let shown: Vec<Value> = pairs
        .iter()
        .enumerate()
        .map(|(i, pair)| {
            json!({
                "rank": i + 1,
                "reason": pair.reason,
                "warnings": pair.warnings,
                "tm_difference": pair.tm_difference,
                "annealing_temperature_c": annealing_json(pair.annealing),
                "cross_dimer": dimer_json(&pair.dimer),
                "moved_forward": pair.forward_moved,
                "moved_reverse": pair.reverse_moved,
                "amplifies": format!("{}..{}", pair.forward.start + 1, pair.reverse.end + 1),
                "product_length": length_of(c, pair),
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
            "boundary_slack": wanted.boundary_slack,
        },
        "product": product,
        "pairs": shown,
    }))
}

/// How many bases one pair amplifies: the stretch asked for plus whatever its two 5' ends moved
/// outward, and the tails on top. With no slack every pair's is the same, which is why the product
/// above is reported once.
fn length_of(c: &Construct, pair: &PrimerPair) -> usize {
    let (start, end, n) = (pair.forward.start, pair.reverse.end, c.len());
    let across = if end >= start { end - start + 1 } else { n - start + end + 1 };
    across + pair.forward.tail.len() + pair.reverse.tail.len()
}

/// How the reads are laid out, from whatever the caller named.
fn sequencing_settings(args: &Args) -> Result<SequencingSettings, Failure> {
    let mut s = DEFAULT_SEQUENCING_SETTINGS;
    if let Some(length) = args.opt_int("read_length")? {
        if !(50..=2000).contains(&length) {
            return fail("read_length must be between 50 and 2000 bases");
        }
        s.read_length = length as usize;
    }
    if let Some(overlap) = args.opt_int("overlap")? {
        if !(0..=1000).contains(&overlap) {
            return fail("overlap must be between 0 and 1000 bases");
        }
        s.overlap = overlap as usize;
    }
    if s.overlap >= s.read_length {
        return fail("overlap must be shorter than read_length, or the tiling never moves on");
    }
    if let Some(window) = args.opt_int("search_window")? {
        if !(0..=1000).contains(&window) {
            return fail("search_window must be between 0 and 1000 bases");
        }
        s.search_window = window as usize;
    }
    if let Some(both) = args.opt_bool("both_strands")? {
        s.both_strands = both;
    }
    Ok(s)
}

/// The stretch to read, 0-based inclusive: what was asked for, or the whole plasmid.
fn to_read(c: &Construct, args: &Args) -> Result<(usize, usize), Failure> {
    let length = c.len() as i64;
    match (args.opt_int("start")?, args.opt_int("end")?) {
        (None, None) => Ok((0, c.len().saturating_sub(1))),
        (Some(start), Some(end)) => {
            if !(1..=length).contains(&start) || !(1..=length).contains(&end) {
                return fail(format!("start and end must be between 1 and {length}"));
            }
            Ok(((start - 1) as usize, (end - 1) as usize))
        }
        _ => fail("give both start and end, or neither for the whole plasmid"),
    }
}

fn strand_json(c: &Construct, strand: &StrandPlan, requested: usize) -> Value {
    let reads: Vec<Value> = strand
        .reads
        .iter()
        .enumerate()
        .map(|(i, read)| {
            json!({
                "order": i + 1,
                "primer": primer_json(&read.primer),
                "reads": format!("{}..{}", read.read_start + 1, read.read_end + 1),
                "read_through_origin": read.read_end < read.read_start,
                "shares_with_previous": read.overlap_with_previous,
                "other_sites": read.other_sites,
                "warnings": read.warnings,
            })
        })
        .collect();
    let gaps: Vec<Value> = strand
        .gaps
        .iter()
        .map(|gap| {
            json!({
                "start": gap.start + 1,
                "end": gap.end + 1,
                "length": if gap.end >= gap.start { gap.end - gap.start + 1 } else { c.len() - gap.start + gap.end + 1 },
                "why": gap.why,
            })
        })
        .collect();
    json!({
        "strand": if strand.forward { "forward" } else { "reverse" },
        "primers": reads.len(),
        "covered": strand.covered,
        "covers_everything": strand.covered == requested,
        "reads": reads,
        "gaps": gaps,
    })
}

fn sequencing_primers(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let (start, end) = to_read(c, args)?;
    let wanted = criteria(args)?;
    let settings = sequencing_settings(args)?;
    let plan: SequencingPlan =
        design_sequencing_primers(&c.sequence, c.circular, start, end, &wanted, &settings).map_err(|e| Failure(e.to_string()))?;
    let strands: Vec<Value> = plan.strands.iter().map(|strand| strand_json(c, strand, plan.requested)).collect();
    let rejected: Vec<Value> = plan
        .rejected
        .iter()
        .map(|r| {
            json!({
                "sequence": r.sequence,
                "would_bind": format!("{}..{}", r.start + 1, r.end + 1),
                "strand": if r.forward { "forward" } else { "reverse" },
                "why": r.why,
            })
        })
        .collect();
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "to_read": {
            "start": plan.start + 1,
            "end": plan.end + 1,
            "length": plan.requested,
            "through_origin": plan.end < plan.start,
            "whole_plasmid": plan.requested == c.len(),
        },
        "reads": {
            "read_length": settings.read_length,
            "overlap": settings.overlap,
            "both_strands": settings.both_strands,
            "search_window": settings.search_window,
        },
        "criteria": {
            "target_tm": wanted.target_tm,
            "max_tm_difference": wanted.max_tm_difference,
            "length": format!("{}-{}", wanted.min_length, wanted.max_length),
            "gc_percent": format!("{}-{}", wanted.min_gc, wanted.max_gc),
            "prefer_gc_clamp": wanted.prefer_gc_clamp,
        },
        "strands": strands,
        "rejected": rejected,
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

/// The oligo `against` names: a second sequence, or a primer on the plasmid that is open.
fn partner(c: Option<&Construct>, given: &str) -> Result<(String, String), Failure> {
    let wanted = given.trim();
    if let Some(c) = c {
        let is_primer =
            |f: &bios_core::Feature| f.kind == "primer_bind" && (f.name.eq_ignore_ascii_case(wanted) || refs::bios_note(f, "id") == Some(wanted));
        if wanted.starts_with('#') || c.features.iter().any(is_primer) {
            let index = find_primer(c, wanted)?;
            let Some(f) = c.features.get(index) else {
                return fail("that primer is gone");
            };
            return Ok((oligo(c, f), f.name.clone()));
        }
    }
    let Ok(second) = bases(wanted) else {
        return fail(format!("against: \"{wanted}\" is not a primer on this plasmid and holds no bases"));
    };
    // A name that happens to be letters would otherwise be read as an oligo, and a dimer against
    // a misread name is worse than no answer.
    if !second.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
        return fail(format!("against: \"{wanted}\" is not a primer on this plasmid, and an oligo may only hold A, C, G and T"));
    }
    Ok((second, "the second oligo".to_string()))
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
        "hairpin": hairpin_json(&hairpins(&oligo)),
    });
    let against = args.opt_str("against")?.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    if !args.has("plasmid") {
        if let Some(given) = against {
            let (second, label) = partner(None, &given)?;
            let dimer = cross_dimer(&oligo, &second);
            if let Some(map) = out.as_object_mut() {
                map.insert("against".into(), json!({ "name": label, "sequence": second }));
                map.insert("cross_dimer".into(), dimer_json(&dimer));
            }
        }
        return Ok(out);
    }
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    if let Some(given) = against {
        let (second, label) = partner(Some(c), &given)?;
        let dimer = cross_dimer(&oligo, &second);
        if let Some(map) = out.as_object_mut() {
            map.insert("against".into(), json!({ "name": label, "sequence": second }));
            map.insert("cross_dimer".into(), dimer_json(&dimer));
        }
    }
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
