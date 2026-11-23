//! Turning a construct around: its other strand, or a different base as base 1.

use bios_core::locations::{normalize_location, rotate_feature_location};
use bios_core::{Construct, Feature, Fuzzy, Segment, Strand, seq};

use super::{PLASMID, VERSION};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "reverse_complement",
        description: "Reverse-complement the whole plasmid (features flip strand and move to their new positions), or just start..end in place (features fully inside flip; ones crossing the edge are reported and left alone).",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "start", kind: Kind::Integer, required: false, description: "Optional: first base of the region." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "Optional: last base of the region." },
        ],
        effect: Effect::Write,
        run: Run::Edit(reverse_complement),
    },
    CommandSpec {
        name: "set_origin",
        description: "Move the origin of a circular plasmid: base `position` becomes base 1. Features keep their bases; ones across the new origin run through it.",
        params: &[PLASMID, VERSION, Param { name: "position", kind: Kind::Integer, required: true, description: "The base that becomes base 1." }],
        effect: Effect::Write,
        run: Run::Edit(set_origin),
    },
];

pub(crate) fn flip(fuzzy: Option<Fuzzy>) -> Option<Fuzzy> {
    fuzzy.map(|f| match f {
        Fuzzy::Before => Fuzzy::After,
        Fuzzy::After => Fuzzy::Before,
    })
}

/// The feature as it lies after the bases `low..=high` (0-based) are reverse-complemented:
/// every position mirrors inside that span, ends swap, and the strand flips.
fn mirrored(f: &Feature, low: usize, high: usize) -> Feature {
    let at = |n: usize| (low + high).saturating_sub(n);
    let mut out = f.clone();
    out.start = at(f.end);
    out.end = at(f.start);
    out.start_fuzzy = flip(f.end_fuzzy);
    out.end_fuzzy = flip(f.start_fuzzy);
    // Pieces are kept in top-strand order, so the last becomes the first.
    out.segments = f
        .segments
        .iter()
        .rev()
        .map(|s| Segment { start: at(s.end), end: at(s.start), start_fuzzy: flip(s.end_fuzzy), end_fuzzy: flip(s.start_fuzzy) })
        .collect();
    out.strand = if f.strand == Strand::Forward { Strand::Reverse } else { Strand::Forward };
    out
}

fn unreadable(c: &Construct) -> Result<(), Failure> {
    match c.features.iter().find(|f| f.location_unsupported.is_some()) {
        Some(f) => fail(format!(
            "\"{}\" has a location Bios cannot interpret ({}), so it cannot be moved",
            f.name,
            f.location_unsupported.as_deref().unwrap_or_default()
        )),
        None => Ok(()),
    }
}

fn reverse_complement(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let length = c.len();
    if length == 0 {
        return fail("this plasmid has no bases");
    }
    if !args.has("start") && !args.has("end") {
        unreadable(c)?;
        c.sequence = seq::revcomp(&c.sequence);
        let circular = c.circular;
        for f in &mut c.features {
            *f = mirrored(f, 0, length - 1);
            normalize_location(f, length, circular);
        }
        return Ok(Applied { summary: "Reverse-complemented the whole plasmid".into(), ..Applied::default() });
    }

    let start = args.opt_int("start")?.unwrap_or(1);
    let end = args.opt_int("end")?.unwrap_or(length as i64);
    if start < 1 || end > length as i64 {
        return fail(format!("range must be within 1..{length}"));
    }
    if start > end {
        return fail("the range runs through the origin; move the origin first (set_origin)");
    }
    let (low, high) = ((start - 1) as usize, (end - 1) as usize);
    let (Some(head), Some(region), Some(tail)) = (c.sequence.get(..low), c.sequence.get(low..=high), c.sequence.get(high + 1..)) else {
        return fail(format!("range must be within 1..{length}"));
    };
    c.sequence = format!("{head}{}{tail}", seq::revcomp(region));

    let (mut flipped, mut crossing) = (Vec::new(), Vec::new());
    let circular = c.circular;
    for f in &mut c.features {
        let inside = !f.wraps() && f.segments.iter().all(|s| s.start >= low && s.end <= high && s.start <= s.end);
        let outside = !f.wraps() && f.segments.iter().all(|s| s.end < low || s.start > high);
        if inside && f.location_unsupported.is_none() {
            *f = mirrored(f, low, high);
            normalize_location(f, length, circular);
            flipped.push(f.name.clone());
        } else if !outside {
            crossing.push(f.name.clone());
        }
    }
    let mut notes = Vec::new();
    if !flipped.is_empty() {
        notes.push(format!("flipped with the region: {}", flipped.join(", ")));
    }
    if !crossing.is_empty() {
        notes.push(format!("cross the edge of the region and were left where they are; check them: {}", crossing.join(", ")));
    }
    Ok(Applied { summary: format!("Reverse-complemented {start}..{end} in place"), notes, ..Applied::default() })
}

fn set_origin(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let length = c.len();
    let position = args.int("position")?;
    if !c.circular {
        return fail("only a circular plasmid has an origin to move");
    }
    if position < 1 || position > length as i64 {
        return fail(format!("position must be within 1..{length}"));
    }
    let cut = (position - 1) as usize;
    if cut == 0 {
        return Ok(Applied::default());
    }
    let (Some(head), Some(tail)) = (c.sequence.get(..cut), c.sequence.get(cut..)) else {
        return fail(format!("position must be within 1..{length}"));
    };
    let rotated = format!("{tail}{head}");
    let offset = (length - cut) as i64;
    let mut features = Vec::with_capacity(c.features.len());
    for f in &c.features {
        let mut moved = rotate_feature_location(f, length, offset).map_err(|e| Failure(format!("\"{}\": {e}", f.name)))?;
        normalize_location(&mut moved, length, true);
        features.push(moved);
    }
    let through: Vec<&str> = features.iter().filter(|f| f.wraps()).map(|f| f.name.as_str()).collect();
    let notes = if through.is_empty() { Vec::new() } else { vec![format!("now run through the origin: {}", through.join(", "))] };
    c.sequence = rotated;
    c.features = features;
    Ok(Applied { summary: format!("Moved the origin: base {position} is now base 1"), notes, ..Applied::default() })
}
