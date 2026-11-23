//! Looking for things in a construct: a sequence, the places enzymes cut.

use std::collections::BTreeMap;

use bios_core::enzymes::{COMMON_ENZYMES, CutSite, OverhangKind, find_cut_sites, get_enzyme};
use bios_core::search::{SearchOptions, find_binding_sites};
use bios_core::seq::{self, DNA_TM_DEFAULTS};
use bios_core::{Construct, Strand};
use serde_json::{Value, json};

use super::read::gc_percent;
use super::sequence::{bases, in_range, slice};
use super::{FEATURE, PLASMID, VERSION};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const OPTIONAL_PLASMID: Param = Param {
    name: "plasmid",
    kind: Kind::String,
    required: false,
    description: "The plasmid to take the bases from (with start and end for a part of it).",
};
const RANGE_START: Param = Param { name: "start", kind: Kind::Integer, required: false, description: "First base (default 1)." };
const RANGE_END: Param = Param {
    name: "end",
    kind: Kind::Integer,
    required: false,
    description: "Last base (default the end). start > end runs through the origin of a circular plasmid.",
};
const RAW: Param = Param { name: "sequence", kind: Kind::String, required: false, description: "Or give bases directly." };

/// Hits listed in one reply. The total is always reported.
const MAX_HITS: usize = 200;

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "find_sequence",
        description: "Find a sequence in a plasmid on both strands, through the origin, allowing mismatches. Positions are on the top strand whichever strand it binds. Letters other than A, C, G and T in the query count as mismatches.",
        params: &[
            PLASMID,
            Param { name: "query", kind: Kind::String, required: true, description: "The bases to look for, 5' to 3'." },
            Param { name: "max_mismatches", kind: Kind::Integer, required: false, description: "Default 0 (exact). At most 10." },
        ],
        effect: Effect::Read,
        run: Run::Call(find_sequence),
    },
    CommandSpec {
        name: "sequence_stats",
        description: "Length, GC% and melting temperature of a range of a plasmid or of bases you give. Tm is the nearest-neighbour estimate (SantaLucia 1998, 250 nM strands, 50 mM Na+); it is null for a sequence with letters other than A, C, G and T.",
        params: &[OPTIONAL_PLASMID, RANGE_START, RANGE_END, RAW],
        effect: Effect::Read,
        run: Run::Call(sequence_stats),
    },
    CommandSpec {
        name: "translate",
        description: "Translate a range, or bases you give, in a reading frame with the standard genetic code. Frames 1 to 3 read the top strand from the start; -1 to -3 read the bottom strand from the end. A stop is *.",
        params: &[
            OPTIONAL_PLASMID,
            RANGE_START,
            RANGE_END,
            RAW,
            Param { name: "frame", kind: Kind::Integer, required: false, description: "1, 2, 3, -1, -2 or -3. Default 1." },
        ],
        effect: Effect::Read,
        run: Run::Call(translate),
    },
    CommandSpec {
        name: "find_orfs",
        description: "Open reading frames (ATG to the first stop, both strands). Positions are on the top strand and include the stop codon.",
        params: &[
            PLASMID,
            Param { name: "min_aa", kind: Kind::Integer, required: false, description: "Shortest protein to report, in amino acids. Default 30." },
        ],
        effect: Effect::Read,
        run: Run::Call(find_orfs),
    },
    CommandSpec {
        name: "codon_optimise",
        description: "Codon-optimise a coding range keeping the protein identical, using the most-used codon per amino acid for the organism. Give a feature or start..end (whole codons; a reverse feature is read on its own strand). Reports restriction sites it created. The number of bases does not change, so no feature moves.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "feature", kind: Kind::String, required: false, description: FEATURE.description },
            Param { name: "start", kind: Kind::Integer, required: false, description: "First base (if no feature)." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "Last base (if no feature)." },
            Param { name: "organism", kind: Kind::String, required: true, description: "human, mouse, ecoli or yeast." },
        ],
        effect: Effect::Write,
        run: Run::Edit(codon_optimise),
    },
    CommandSpec {
        name: "find_restriction_sites",
        description: "Where restriction enzymes cut a plasmid. Default: the common set (about 80 enzymes). With start/end: only cuts inside that range. A cut is reported as the base it falls after on the top strand (0 = before base 1) and on the bottom strand, with the overhang it leaves.",
        params: &[
            PLASMID,
            Param { name: "enzymes", kind: Kind::Strings, required: false, description: "Enzyme names (default: the common set)." },
            Param {
                name: "max_cuts",
                kind: Kind::Integer,
                required: false,
                description: "Only enzymes that cut at most this many times (1 = single cutters).",
            },
            Param { name: "start", kind: Kind::Integer, required: false, description: "Only cuts from this base..." },
            Param { name: "end", kind: Kind::Integer, required: false, description: "...to this base." },
        ],
        effect: Effect::Read,
        run: Run::Call(find_restriction_sites),
    },
    CommandSpec {
        name: "list_enzymes",
        description: "Every restriction enzyme in the table against a plasmid, grouped by how often it cuts: once, twice, 3 or more, or not at all. Rests on the common set; a query (a name, an isoschizomer such as Eco31I, or the recognition bases such as GAATTC) searches the whole table. Each enzyme says its site with ^ at the cut, the end it leaves, whether it is Type IIS, and where it lands (1-based).",
        params: &[
            PLASMID,
            Param { name: "query", kind: Kind::String, required: false, description: "A name, alias or recognition sequence to look for." },
            Param {
                name: "filter",
                kind: Kind::String,
                required: false,
                description: "common (default), all, type2s (cuts outside its site) or blunt (leaves no overhang).",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(list_enzymes),
    },
];

fn find_sequence(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let query = bases(&args.str("query")?)?;
    let max_mismatches = args.opt_int("max_mismatches")?.unwrap_or(0);
    if !(0..=10).contains(&max_mismatches) {
        return fail("max_mismatches must be between 0 and 10");
    }
    if query.len() > c.len() {
        return fail("the query is longer than the plasmid");
    }
    let sites = find_binding_sites(&c.sequence, &query, SearchOptions { max_mismatches: max_mismatches as usize, circular: c.circular });
    let hits: Vec<Value> = sites
        .iter()
        .take(MAX_HITS)
        .map(|s| {
            json!({
                "start": s.start + 1,
                "end": s.end + 1,
                "strand": if s.forward { "forward" } else { "reverse" },
                "mismatches": s.mismatches,
                "through_origin": s.wraps,
            })
        })
        .collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "query": query, "total": sites.len(), "hits": hits }))
}

fn find_restriction_sites(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let asked = args.strings("enzymes");
    let mut names: Vec<&'static str> = Vec::new();
    if asked.is_empty() {
        names.extend(COMMON_ENZYMES.iter().copied());
    } else {
        let unknown: Vec<&str> = asked.iter().filter(|n| get_enzyme(n).is_none()).map(String::as_str).collect();
        if !unknown.is_empty() {
            return fail(format!("not in the enzyme table: {}", unknown.join(", ")));
        }
        names.extend(asked.iter().filter_map(|n| get_enzyme(n)).map(|e| e.name));
    }

    let sites = find_cut_sites(&c.sequence, &names, c.circular);
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for site in &sites {
        *totals.entry(site.enzyme).or_default() += 1;
    }
    let max_cuts = args.opt_int("max_cuts")?;
    let (from, to) = (args.opt_int("start")?, args.opt_int("end")?);
    let in_window = |after: usize| from.is_none_or(|s| after as i64 >= s - 1) && to.is_none_or(|e| after as i64 <= e);

    let mut by_enzyme: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for site in &sites {
        let total = totals.get(site.enzyme).copied().unwrap_or(0);
        if max_cuts.is_some_and(|m| total as i64 > m) || !in_window(site.top_cut) {
            continue;
        }
        let overhang = match site.overhang.kind {
            OverhangKind::Blunt => "blunt".to_string(),
            OverhangKind::FivePrime => format!("5' {}", site.overhang.seq),
            OverhangKind::ThreePrime => format!("3' {}", site.overhang.seq),
        };
        by_enzyme.entry(site.enzyme).or_default().push(json!({
            "after_base": site.top_cut,
            "bottom_after_base": site.bottom_cut,
            "site_starts": site.recognition_start + 1,
            "strand": if site.strand == Strand::Reverse { "reverse" } else { "forward" },
            "overhang": overhang,
        }));
    }
    let enzymes: Vec<Value> = by_enzyme
        .into_iter()
        .map(|(name, cuts)| json!({ "enzyme": name, "site": get_enzyme(name).map(|e| e.site), "total_cuts_in_plasmid": totals.get(name).copied().unwrap_or(0), "cuts": cuts }))
        .collect();
    let non_cutters: Vec<&str> = names.iter().copied().filter(|n| !totals.contains_key(n)).collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "enzymes": enzymes, "non_cutters": non_cutters }))
}

/// The bases a command was pointed at: bases given directly, or a plasmid or a range of one.
fn input(session: &Session, args: &Args) -> Result<(String, String), Failure> {
    if let Some(raw) = args.opt_str("sequence")? {
        return Ok((bases(&raw)?, "sequence".to_string()));
    }
    if !args.has("plasmid") {
        return fail("give a plasmid, or bases as sequence");
    }
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let (start, end) = (args.opt_int("start")?.unwrap_or(1), args.opt_int("end")?.unwrap_or(c.len() as i64));
    in_range(c, start, end)?;
    Ok((slice(c, start, end), format!("{} {start}..{end}", refs::label(&loaded.path))))
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn melting(bases: &str) -> Option<f64> {
    seq::tm(bases, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm).map(round1)
}

fn sequence_stats(session: &Session, args: &Args) -> Result<Value, Failure> {
    let (bases, of) = input(session, args)?;
    Ok(json!({ "of": of, "length": bases.len(), "gc_percent": gc_percent(&bases), "tm_c": melting(&bases) }))
}

fn translate(session: &Session, args: &Args) -> Result<Value, Failure> {
    let (bases, of) = input(session, args)?;
    let frame = args.opt_int("frame")?.unwrap_or(1);
    let read = match frame {
        1..=3 => bases.get((frame - 1) as usize..).unwrap_or_default().to_string(),
        -3..=-1 => seq::revcomp(&bases).get((-frame - 1) as usize..).unwrap_or_default().to_string(),
        _ => return fail("frame must be 1, 2, 3, -1, -2 or -3"),
    };
    let protein = seq::translate(&read);
    Ok(json!({ "of": of, "frame": frame, "aa": protein.len(), "stops": protein.matches('*').count(), "protein": protein }))
}

fn find_orfs(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let min_aa = match args.opt_int("min_aa")?.unwrap_or(30) {
        n if n >= 1 => n as usize,
        _ => return fail("min_aa must be 1 or more"),
    };
    let orfs: Vec<Value> = seq::find_orfs(&loaded.construct.sequence, min_aa)
        .into_iter()
        .map(|o| {
            let shown: String = if o.protein.chars().count() > 60 { o.protein.chars().take(60).chain(std::iter::once('…')).collect() } else { o.protein.clone() };
            json!({ "start": o.start, "end": o.end, "strand": if o.strand == Strand::Reverse { "reverse" } else { "forward" }, "aa": o.aa_length, "protein": shown })
        })
        .collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "orfs": orfs }))
}

/// The most-used codon for each amino acid, per organism, in the order of [`AMINO_ACIDS`].
const AMINO_ACIDS: &str = "ARNDCQEGHILKMFPSTWYV*";
const HUMAN: [&str; 21] = [
    "GCC", "AGA", "AAC", "GAC", "TGC", "CAG", "GAG", "GGC", "CAC", "ATC", "CTG", "AAG", "ATG", "TTC", "CCC", "AGC", "ACC", "TGG", "TAC", "GTG", "TGA",
];
const ECOLI: [&str; 21] = [
    "GCG", "CGC", "AAC", "GAT", "TGC", "CAG", "GAA", "GGC", "CAT", "ATT", "CTG", "AAA", "ATG", "TTT", "CCG", "AGC", "ACC", "TGG", "TAT", "GTG", "TAA",
];
const YEAST: [&str; 21] = [
    "GCT", "AGA", "AAT", "GAT", "TGT", "CAA", "GAA", "GGT", "CAT", "ATT", "TTG", "AAA", "ATG", "TTT", "CCA", "TCT", "ACT", "TGG", "TAT", "GTT", "TAA",
];

/// `cds` with every codon replaced by the organism's most-used codon for the same amino acid.
fn optimise(cds: &str, organism: &str) -> Result<String, Failure> {
    let table = match organism.to_lowercase().as_str() {
        "human" | "mouse" => &HUMAN,
        "ecoli" => &ECOLI,
        "yeast" => &YEAST,
        _ => return fail("organism must be one of human, mouse, ecoli, yeast"),
    };
    if !cds.len().is_multiple_of(3) {
        return fail(format!("the range is {} bp, not a whole number of codons", cds.len()));
    }
    let mut out = String::with_capacity(cds.len());
    for (i, codon) in cds.as_bytes().chunks(3).enumerate() {
        let codon = std::str::from_utf8(codon).unwrap_or_default();
        let plain = codon.bytes().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T'));
        let best = seq::translate(codon).chars().next().and_then(|aa| AMINO_ACIDS.find(aa)).and_then(|at| table.get(at));
        match (plain, best) {
            (true, Some(best)) => out.push_str(best),
            _ => return fail(format!("codon {codon} at +{} isn't a plain ACGT codon", i * 3 + 1)),
        }
    }
    Ok(out)
}

fn site_keys(sites: &[CutSite]) -> Vec<(&'static str, usize)> {
    sites.iter().map(|s| (s.enzyme, s.top_cut)).collect()
}

fn codon_optimise(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let (start, end, reverse) = match args.opt_str("feature")? {
        Some(reference) => {
            let Some(f) = c.features.get(refs::feature(c, &reference)?) else {
                return fail("that feature is gone");
            };
            if f.locations().is_some() || f.location_unsupported.is_some() {
                return fail("that feature has several pieces; give start and end of one piece");
            }
            (f.start as i64 + 1, f.end as i64 + 1, f.strand == Strand::Reverse)
        }
        None => (args.int("start")?, args.int("end")?, false),
    };
    in_range(c, start, end)?;
    if start > end {
        return fail("the range runs through the origin; move the origin first (set_origin)");
    }
    let old = slice(c, start, end);
    let coding = if reverse { seq::revcomp(&old) } else { old.clone() };
    let optimised = optimise(&coding, &args.str("organism")?)?;
    // The whole point is that the protein does not change. Checked, not assumed.
    if seq::translate(&optimised) != seq::translate(&coding) {
        return fail("internal check failed: the protein would change; nothing was saved");
    }
    let put = if reverse { seq::revcomp(&optimised) } else { optimised.clone() };

    let common: Vec<&str> = COMMON_ENZYMES.iter().copied().collect();
    let before = site_keys(&find_cut_sites(&c.sequence, &common, c.circular));
    let (s, e) = ((start - 1) as usize, end as usize);
    let (Some(head), Some(tail)) = (c.sequence.get(..s), c.sequence.get(e..)) else {
        return fail("range must be within the plasmid");
    };
    c.sequence = format!("{head}{put}{tail}");
    let created: Vec<String> = site_keys(&find_cut_sites(&c.sequence, &common, c.circular))
        .into_iter()
        .filter(|k| !before.contains(k))
        .map(|(enzyme, after)| format!("{enzyme} cuts after {after}"))
        .collect();

    let changed = old.bytes().zip(put.bytes()).filter(|(a, b)| a != b).count();
    let mut notes = vec![format!("{changed} bases changed over {start}..{end}; protein identical ({} aa)", seq::translate(&optimised).len())];
    if !created.is_empty() {
        notes.push(format!("new restriction sites created: {}", created.join(", ")));
    }
    Ok(Applied { summary: format!("Codon-optimised {start}..{end} for {}", args.str("organism")?), notes, ..Applied::default() })
}

fn list_enzymes(session: &Session, args: &Args) -> Result<Value, Failure> {
    use bios_core::enzyme_list::{EnzymeFilter, enzyme_rows, group_rows};
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let filter = match args.opt_str("filter")? {
        Some(text) => EnzymeFilter::parse(&text).ok_or_else(|| Failure(format!("filter is common, all, type2s or blunt, not {text}")))?,
        None => EnzymeFilter::Common,
    };
    let query = args.opt_str("query")?.unwrap_or_default();
    let rows = enzyme_rows(&c.sequence, c.circular);
    let groups = group_rows(&rows, &query, filter, &[]);
    let listed: Vec<Value> = groups
        .iter()
        .filter(|g| !g.rows.is_empty())
        .map(|g| {
            let enzymes: Vec<Value> = g
                .rows
                .iter()
                .filter_map(|(i, hit)| rows.get(*i).map(|r| (r, hit)))
                .map(|(r, hit)| {
                    let sites: Vec<usize> = r.sites.iter().map(|s| s.recognition_start + 1).collect();
                    json!({
                        "enzyme": r.name,
                        "site": r.caret_site.clone().unwrap_or_else(|| r.site.to_string()),
                        "leaves": if r.type2s { "cuts outside its site".to_string() } else { r.overhang_text.clone() },
                        "type2s": r.type2s,
                        "matched_alias": hit.via,
                        "site_starts": sites,
                    })
                })
                .collect();
            json!({ "group": g.key.label(), "count": enzymes.len(), "enzymes": enzymes })
        })
        .collect();
    Ok(json!({ "plasmid": c.name, "groups": listed }))
}
