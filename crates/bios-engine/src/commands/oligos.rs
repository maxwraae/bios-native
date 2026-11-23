//! Oligos held against a plasmid without landing in it: where a binder binds, and how a sequence
//! fits (Find's alignment). Both only read; nothing here writes the file.

use bios_core::align::ALIGN_DEFAULTS;
use bios_core::align::{AlignBlock, AlignOp};
use bios_core::binders::{Binder, binders_to_sites, site_label};
use bios_core::find_align::{CELL_BUDGET, FIND_MAX_NT, FIND_MIN_NT, alignment_hits, cells_for, describe_hit, query_limit};
use bios_core::seq::clean_seq;
use serde_json::{Value, json};

use super::PLASMID;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "binder_sites",
        description: "Every place oligos would bind a plasmid as primers, without adding them to it (what the window calls binders). A site is anchored at the oligo's 3' end; a 5' tail that does not anneal is allowed and reported, and up to 3 mismatches inside the anneal. Each site says whether it is the oligo's one full perfect landing (on_target), anneals by less than the oligo's best site (partial: a chance landing), or is a second equally good landing (neither: the oligo is ambiguous). Positions are on the top strand, 1-based; strand forward means the oligo reads along the top strand (so it anneals to the bottom one).",
        params: &[
            PLASMID,
            Param { name: "oligos", kind: Kind::Strings, required: true, description: "The oligos, 5' to 3'." },
            Param {
                name: "names",
                kind: Kind::Strings,
                required: false,
                description: "A name for each oligo, in the same order (default binder 1, binder 2, ...).",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(binder_sites),
    },
    CommandSpec {
        name: "align_sequence",
        description: "How a sequence fits a plasmid, mismatches, insertions, deletions and overhangs included (local Smith-Waterman, both strands, through the origin): the best fits first, at most 24. Unlike find_sequence it always reports a partial fit, with what does not fit counted as overhang. Each fit has a one-line caption, and for an oligo whether it would prime and why not. Positions are on the top strand, 1-based. The query is 6 to 50,000 bases, and no longer than the plasmid's size allows (the error says the limit).",
        params: &[PLASMID, Param { name: "query", kind: Kind::String, required: true, description: "The bases, 5' to 3'." }],
        effect: Effect::Read,
        run: Run::Call(align_sequence),
    },
];

fn binder_sites(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let names = args.strings("names");
    let mut binders = Vec::new();
    for (i, oligo) in args.strings("oligos").iter().enumerate() {
        let bases = clean_seq(oligo);
        if bases.is_empty() {
            return fail(format!("oligo {} has no bases", i + 1));
        }
        let name = names.get(i).map(|n| n.trim()).filter(|n| !n.is_empty()).map_or_else(|| format!("binder {}", i + 1), str::to_string);
        binders.push(Binder { id: format!("binder-{}", i + 1), name, bases });
    }
    if binders.is_empty() {
        return fail("give at least one oligo");
    }
    let sites = binders_to_sites(&binders, &c.sequence, c.circular);
    let out: Vec<Value> = binders
        .iter()
        .map(|b| {
            let mine: Vec<Value> = sites
                .iter()
                .filter(|s| s.binder_id == b.id)
                .map(|s| {
                    json!({
                        "start": s.site.start + 1,
                        "end": s.site.end + 1,
                        "strand": if s.site.forward { "forward" } else { "reverse" },
                        "through_origin": s.site.wraps,
                        "tail_length": s.site.tail_length,
                        "anneal_length": s.site.anneal_length,
                        "mismatches": s.site.mismatches,
                        "mismatch_positions": s.site.mismatch_positions.iter().map(|p| p + 1).collect::<Vec<_>>(),
                        "tm": s.site.tm.map(|t| (t * 10.0).round() / 10.0),
                        "on_target": s.on_target,
                        "partial": s.partial,
                        "label": site_label(s),
                    })
                })
                .collect();
            let strays = sites.iter().filter(|s| s.binder_id == b.id && !s.on_target).count();
            json!({
                "name": b.name,
                "sequence": b.bases,
                "length": b.bases.len(),
                "gc_percent": (bios_core::seq::gc_content(&b.bases) * 10.0).round() / 10.0,
                "sites": mine,
                "off_target_sites": strays,
            })
        })
        .collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "binders": out }))
}

fn block(b: &AlignBlock, n: usize) -> Value {
    let op = match b.op {
        AlignOp::Match => "match",
        AlignOp::Mismatch => "mismatch",
        AlignOp::Insertion => "insertion",
        AlignOp::Deletion => "deletion",
    };
    json!({
        "op": op,
        "query_start": b.query_start.map(|s| s + 1),
        "query_length": b.query_length,
        "start": b.target_start.map(|s| s % n.max(1) + 1),
        "length": b.target_length,
    })
}

fn align_sequence(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let query = clean_seq(&args.str("query")?);
    if query.len() < FIND_MIN_NT {
        return fail(format!("the query needs at least {FIND_MIN_NT} bases"));
    }
    let limit = query_limit(c.len(), c.circular);
    if query.len() > FIND_MAX_NT || cells_for(c.len(), query.len(), c.circular) > CELL_BUDGET {
        return fail(format!("this plasmid can be aligned against a query of up to {limit} bases; this one has {}", query.len()));
    }
    let hits = alignment_hits(&c.sequence, c.circular, &query);
    let n = c.len();
    let out: Vec<Value> = hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let a = &h.alignment;
            json!({
                "rank": i + 1,
                "start": a.start % n.max(1) + 1,
                "end": a.end % n.max(1) + 1,
                "strand": if a.forward { "forward" } else { "reverse" },
                "score": a.score,
                "identity": a.identity,
                "leading_overhang": a.leading_overhang,
                "trailing_overhang": a.trailing_overhang,
                "mismatch_positions": a.mismatch_positions.iter().map(|p| p + 1).collect::<Vec<_>>(),
                "blocks": a.blocks.iter().map(|b| block(b, n)).collect::<Vec<_>>(),
                "anneal_tm": h.anneal_tm.map(|t| (t * 10.0).round() / 10.0),
                "would_prime": h.primable.then_some(h.priming_faults.is_empty()),
                "priming_faults": h.priming_faults,
                "caption": describe_hit(h, Some(n)),
            })
        })
        .collect();
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "query_length": query.len(),
        "total": hits.len(),
        "capped": hits.len() >= ALIGN_DEFAULTS.max_results,
        "hits": out,
    }))
}
