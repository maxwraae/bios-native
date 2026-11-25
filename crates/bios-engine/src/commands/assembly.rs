//! Building a sequence out of short synthetic oligos: the numbered list to order, the junction
//! temperatures the reaction will run at, and the two primers that amplify what they build.
//! Nothing here writes; the oligos are kept with `add_primer`.

use bios_core::assembly::{AssemblyPlan, AssemblyPrimer, DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, MAX_OLIGOS, MIN_OVERLAP, design_assembly_oligos};
use serde_json::{Value, json};

use super::sequence::bases;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail};

const SEQUENCE: Param = Param {
    name: "sequence",
    kind: Kind::String,
    required: true,
    description: "The DNA to build, 5' to 3' on the top strand. FASTA headers are ignored.",
};
const MAX_LENGTH: Param = Param {
    name: "max_length",
    kind: Kind::Integer,
    required: false,
    description: "Longest oligo that can be ordered, in bases. Default 60, which is what most suppliers make cheapest on a plate. It has to be at least 36: an oligo in the middle carries an overlap at each end.",
};
const TARGET_TM: Param = Param {
    name: "target_tm",
    kind: Kind::Integer,
    required: false,
    description: "Melting temperature every overlap aims for, in °C. Default 60. Nearest-neighbour, the same one Gibson overlaps are designed with.",
};
const NAME: Param = Param {
    name: "name",
    kind: Kind::String,
    required: false,
    description: "What to call the oligos. They come back as <name>_01 upward, with the two primers as <name>_F and <name>_R. Default \"oligo\".",
};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "design_assembly_oligos",
    description: "Design the short oligos that build a sequence from scratch, to assemble by PCR (polymerase cycling assembly). The oligos alternate strands and are staggered so each overlaps both its neighbours; the overlaps are what anneal, so they are sized by melting temperature rather than by length, and the oligos come out at uneven lengths bounded by max_length. Returns the numbered list to order, each junction's overlap and its temperature, the two primers that amplify the assembled product, and what the whole order costs in bases. Two things always hold: the oligos put back together are exactly the sequence given, and no oligo is longer than max_length. The temperature is not always reachable — two overlaps share one oligo and cannot both be long — so the design settles on the highest temperature every junction can meet, and anneal_tm says what that is. Check overlaps_elsewhere before ordering: an overlap that anneals in more than one place can assemble the pieces in the wrong order. Nothing is saved.",
    params: &[SEQUENCE, MAX_LENGTH, TARGET_TM, NAME],
    effect: Effect::Read,
    run: Run::Call(design),
}];

fn design(_session: &Session, args: &Args) -> Result<Value, Failure> {
    let target = bases(&args.str("sequence")?)?;
    let max_length = args.opt_int("max_length")?.unwrap_or(DEFAULT_MAX_LENGTH);
    let target_tm = args.opt_num("target_tm")?.unwrap_or(DEFAULT_TARGET_TM);
    let name = args.opt_str("name")?.unwrap_or_default();
    if max_length < MIN_OVERLAP * 2 {
        return fail(format!("max_length has to be at least {}: an oligo in the middle carries an overlap at each end.", MIN_OVERLAP * 2));
    }
    if target_tm < 20.0 || target_tm > 90.0 {
        return fail("target_tm has to be between 20 and 90 °C.");
    }
    let plan = design_assembly_oligos(&target, max_length, target_tm, &name).map_err(|e| Failure(e.to_string()))?;
    Ok(report(&plan))
}

fn report(plan: &AssemblyPlan) -> Value {
    let oligos: Vec<Value> = plan
        .oligos
        .iter()
        .map(|o| {
            json!({
                "number": o.number,
                "name": o.name,
                "sequence": o.sequence,
                "length": o.length,
                "strand": if o.forward { "forward" } else { "reverse" },
                "start": o.start + 1,
                "end": o.end,
                "gc": o.gc,
            })
        })
        .collect();
    let overlaps: Vec<Value> = plan
        .overlaps
        .iter()
        .map(|o| {
            json!({
                "joins": format!("{} to {}", o.after, o.after + 1),
                "sequence": o.sequence,
                "length": o.length,
                "start": o.start + 1,
                "end": o.end,
                "tm": o.tm,
                "reached_target": o.reached_target,
                "elsewhere": o.elsewhere,
            })
        })
        .collect();
    let mut warnings = Vec::new();
    if !plan.all_reached_target() {
        let short = plan.overlaps.iter().filter(|o| !o.reached_target).count();
        warnings.push(format!(
            "{short} of {} junctions could not reach {} °C: this sequence cannot hold two long enough overlaps on one oligo there. Anneal at {} °C.",
            plan.overlaps.len(),
            plan.target_tm,
            plan.lowest_overlap_tm().unwrap_or(plan.target_tm)
        ));
    }
    let strays: Vec<String> =
        plan.overlaps.iter().filter(|o| o.elsewhere > 0).map(|o| format!("{} to {} ({} other places)", o.after, o.after + 1, o.elsewhere)).collect();
    if !strays.is_empty() {
        warnings.push(format!(
            "These overlaps anneal somewhere else in the target as well, so the pieces could assemble in the wrong order: {}.",
            strays.join(", ")
        ));
    }
    if plan.oligos.len() > 48 {
        warnings.push(format!("{} oligos is more than one plate column; a one-pot assembly this size often needs two stages.", plan.oligos.len()));
    }
    let primers: Vec<Value> = [plan.forward_primer.as_ref(), plan.reverse_primer.as_ref()].into_iter().flatten().map(primer).collect();
    json!({
        "target_length": plan.target_length,
        "max_length": plan.max_length,
        "target_tm": plan.target_tm,
        "anneal_tm": plan.lowest_overlap_tm(),
        "oligo_count": plan.oligos.len(),
        "total_bases_to_order": plan.total_bases(),
        "oligos": oligos,
        "overlaps": overlaps,
        "amplification_primers": primers,
        "warnings": warnings,
        "max_oligos": MAX_OLIGOS,
    })
}

fn primer(p: &AssemblyPrimer) -> Value {
    json!({
        "name": p.name,
        "sequence": p.sequence,
        "length": p.length,
        "strand": if p.forward { "forward" } else { "reverse" },
        "tm": p.tm,
        "gc": p.gc,
    })
}
