//! How much there is of something: the nanograms, picomoles and molecules a piece of DNA comes to,
//! what to weigh out for a ligation, and what a protein weighs and absorbs. Nothing here writes.

use bios_core::Strand;
use bios_core::coding;
use bios_core::quantities::{self as q, MassBasis};
use bios_core::seq;
use bios_project::Loaded;
use serde_json::{Value, json};

use super::analysis::{CODE, code_of};
use super::sequence::{bases, in_range, slice};
use super::{FEATURE, PLASMID};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const AMOUNT_PLASMID: Param = Param {
    name: "plasmid",
    kind: Kind::String,
    required: false,
    description: "The plasmid this DNA is (with start and end for a part of it). Its bases give the exact molecular weight.",
};
const AMOUNT_START: Param = Param { name: "start", kind: Kind::Integer, required: false, description: "First base of the part (default 1)." };
const AMOUNT_END: Param =
    Param { name: "end", kind: Kind::Integer, required: false, description: "Last base of the part (default the end of the plasmid)." };

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "dna_amount",
        description: "Nanograms, picomoles, molecules and molarity of a piece of DNA, both ways. Say what the DNA is (a plasmid, a range of one, bases, or just a length in base pairs) and what you have of it (ng, pmol or molecules), and every other number comes back with the formula it came from. Given bases, the molecular weight is added up from the base composition; given only a length, from the conventional average of 650 g/mol per base pair, and the answer says which was used. single_stranded treats it as one strand, which is what an oligo is: with target_um it says what volume to resuspend in. Give an insert as well and it says how much of each to put in a ligation at a molar ratio.",
        params: &[
            AMOUNT_PLASMID,
            AMOUNT_START,
            AMOUNT_END,
            Param { name: "sequence", kind: Kind::String, required: false, description: "Or the bases themselves." },
            Param {
                name: "length_bp",
                kind: Kind::Integer,
                required: false,
                description: "Or only a length, in base pairs (in bases, if single_stranded).",
            },
            Param {
                name: "single_stranded",
                kind: Kind::Boolean,
                required: false,
                description: "true for one strand, such as an oligo or a primer. Default false: double-stranded.",
            },
            Param { name: "ng", kind: Kind::Number, required: false, description: "How many nanograms you have." },
            Param { name: "pmol", kind: Kind::Number, required: false, description: "Or how many picomoles." },
            Param { name: "molecules", kind: Kind::Number, required: false, description: "Or how many molecules." },
            Param {
                name: "volume_ul",
                kind: Kind::Number,
                required: false,
                description: "The volume it is in, in microlitres, to get a concentration back.",
            },
            Param {
                name: "target_um",
                kind: Kind::Number,
                required: false,
                description: "Resuspend to this concentration, in micromolar: the answer says the volume to add.",
            },
            Param { name: "insert_plasmid", kind: Kind::String, required: false, description: "For a ligation: the plasmid the insert is." },
            Param { name: "insert_start", kind: Kind::Integer, required: false, description: "First base of the insert in that plasmid." },
            Param { name: "insert_end", kind: Kind::Integer, required: false, description: "Last base of the insert in that plasmid." },
            Param { name: "insert_sequence", kind: Kind::String, required: false, description: "Or the insert's bases." },
            Param { name: "insert_length_bp", kind: Kind::Integer, required: false, description: "Or only the insert's length, in base pairs." },
            Param {
                name: "molar_ratio",
                kind: Kind::Number,
                required: false,
                description: "Moles of insert per mole of vector. Default 3, the usual 3:1. Needs an insert and an amount of vector.",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(dna_amount),
    },
    CommandSpec {
        name: "protein_stats",
        description: "What a protein weighs and absorbs: length in residues, molecular weight, amino-acid composition, and the molar absorption coefficient at 280 nm, so an A280 reading turns into a concentration. Take the protein from a feature (a CDS is translated on its own strand, with its own transl_table unless code says otherwise), from a range of a plasmid in a reading frame, or give the residues directly. Isoelectric point is not reported: the published methods disagree by several tenths of a pH unit and Bios has no checked answer to give.",
        params: &[
            Param { name: "plasmid", kind: Kind::String, required: false, description: PLASMID.description },
            Param { name: "feature", kind: Kind::String, required: false, description: FEATURE.description },
            AMOUNT_START,
            AMOUNT_END,
            Param {
                name: "frame",
                kind: Kind::Integer,
                required: false,
                description: "1, 2, 3, -1, -2 or -3 for a range or a whole plasmid. Default 1.",
            },
            Param {
                name: "protein",
                kind: Kind::String,
                required: false,
                description: "Or the amino acids themselves, one letter each. A trailing * is dropped.",
            },
            CODE,
        ],
        effect: Effect::Read,
        run: Run::Call(protein_stats),
    },
];

/// Rounded to `digits` significant figures, so an answer reads as a number someone would write
/// down rather than as the last bit of a float.
fn signif(value: f64, digits: i32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let exponent = value.abs().log10().floor();
    let scale = 10f64.powi(digits - 1 - exponent as i32);
    if !scale.is_finite() {
        return value;
    }
    (value * scale).round() / scale
}

/// The construct a name points at, for a parameter that is not called `plasmid`.
fn load_named(session: &Session, reference: &str) -> Result<Loaded, Failure> {
    let listed = session.project().list()?;
    let id = refs::plasmid(&listed, reference)?.id.clone();
    Ok(session.project().read(&id)?)
}

/// A piece of DNA a caller pointed at: what to call it in the answer, how long it is, and its
/// bases when they are known.
struct Piece {
    of: String,
    length: u64,
    bases: Option<String>,
}

/// The piece named by `{prefix}plasmid`, `{prefix}sequence` or `{prefix}length_bp`, in that order
/// of preference. `None` when the caller named none of them.
fn piece(session: &Session, args: &Args, prefix: &str) -> Result<Option<Piece>, Failure> {
    let key = |name: &str| format!("{prefix}{name}");
    if let Some(reference) = args.opt_str(&key("plasmid"))? {
        let loaded = load_named(session, &reference)?;
        let c = &loaded.construct;
        let whole = c.len() as i64;
        let (start, end) = (args.opt_int(&key("start"))?.unwrap_or(1), args.opt_int(&key("end"))?.unwrap_or(whole));
        in_range(c, start, end)?;
        let bases = slice(c, start, end);
        let label = refs::label(&loaded.path);
        let of = if start == 1 && end == whole { label } else { format!("{label} {start}..{end}") };
        return Ok(Some(Piece { length: bases.len() as u64, bases: Some(bases), of }));
    }
    if let Some(raw) = args.opt_str(&key("sequence"))? {
        let bases = bases(&raw)?;
        return Ok(Some(Piece { of: format!("{} bases given", bases.len()), length: bases.len() as u64, bases: Some(bases) }));
    }
    if let Some(length) = args.opt_int(&key("length_bp"))? {
        if length < 1 {
            return fail(format!("{} must be 1 or more", key("length_bp")));
        }
        return Ok(Some(Piece { of: format!("{length} bp"), length: length as u64, bases: None }));
    }
    Ok(None)
}

/// What a piece weighs, and whether that came from its bases or from the average.
fn weight_of(piece: &Piece, double_stranded: bool) -> Result<(f64, MassBasis), Failure> {
    match q::dna_weight(piece.bases.as_deref(), piece.length, double_stranded) {
        Some(answer) => Ok(answer),
        None => fail("that piece of DNA has no length"),
    }
}

/// How many picomoles a caller has, from whichever of `ng`, `pmol` and `molecules` they gave.
fn picomoles(args: &Args, weight: f64) -> Result<Option<f64>, Failure> {
    let (ng, pmol, molecules) = (args.opt_num("ng")?, args.opt_num("pmol")?, args.opt_num("molecules")?);
    let given = [ng.is_some(), pmol.is_some(), molecules.is_some()].iter().filter(|given| **given).count();
    if given > 1 {
        return fail("give only one of ng, pmol and molecules; the others come back worked out");
    }
    for (name, value) in [("ng", ng), ("pmol", pmol), ("molecules", molecules)] {
        if value.is_some_and(|v| v < 0.0) {
            return fail(format!("{name} cannot be negative"));
        }
    }
    if let Some(ng) = ng {
        return match q::pmol_from_ng(ng, weight) {
            Some(pmol) => Ok(Some(pmol)),
            None => fail("that piece of DNA has no weight to divide by"),
        };
    }
    Ok(pmol.or(molecules.map(q::pmol_from_molecules)))
}

fn have(pmol: f64, weight: f64) -> Value {
    json!({
        "ng": signif(q::ng_from_pmol(pmol, weight), 4),
        "pmol": signif(pmol, 4),
        "molecules": signif(q::molecules_from_pmol(pmol), 4),
    })
}

fn dna_amount(session: &Session, args: &Args) -> Result<Value, Failure> {
    let Some(vector) = piece(session, args, "")? else {
        return fail("say what the DNA is: a plasmid, a sequence, or length_bp");
    };
    let single = args.opt_bool("single_stranded")?.unwrap_or(false);
    let (weight, basis) = weight_of(&vector, !single)?;
    let pmol = picomoles(args, weight)?;

    let mut formulas = vec![match basis {
        MassBasis::Composition => format!(
            "molecular weight = sum of the base masses (A 313.21, C 289.18, G 329.21, T 304.20 g/mol) - 61.96 per strand{}",
            if single { "" } else { ", for both strands" }
        ),
        MassBasis::Average => format!(
            "molecular weight = length x {} g/mol per {}",
            if single { q::AVERAGE_BASE_DA } else { q::AVERAGE_BP_DA },
            if single { "base" } else { "base pair" }
        ),
    }];
    formulas.push("pmol = ng / molecular weight x 1000; molecules = pmol x 1e-12 x 6.02214076e23".to_string());

    let mut out = json!({
        "of": vector.of,
        "strands": if single { "single" } else { "double" },
        "length": vector.length,
        "length_unit": if single { "bases" } else { "base pairs" },
        "molecular_weight_g_per_mol": signif(weight, 6),
        "molecular_weight_from": basis.words(),
        "ng_per_pmol": signif(weight / 1000.0, 4),
    });
    let Some(map) = out.as_object_mut() else {
        return fail("could not assemble the answer");
    };

    if let Some(pmol) = pmol {
        map.insert("have".into(), have(pmol, weight));
        if let Some(volume_ul) = args.opt_num("volume_ul")? {
            match q::um_from_pmol(pmol, volume_ul) {
                None => return fail("volume_ul must be more than zero"),
                Some(um) => {
                    map.insert(
                        "in_volume".into(),
                        json!({
                            "volume_ul": volume_ul,
                            "ng_per_ul": signif(q::ng_from_pmol(pmol, weight) / volume_ul, 4),
                            "um": signif(um, 4),
                            "nm": signif(um * 1000.0, 4),
                        }),
                    );
                    formulas.push("uM = pmol / uL (a picomole in a microlitre is already micromolar)".to_string());
                }
            }
        }
        if let Some(target_um) = args.opt_num("target_um")? {
            match q::volume_for_um(pmol, target_um) {
                None => return fail("target_um must be more than zero"),
                Some(volume_ul) => {
                    map.insert("resuspend".into(), json!({ "to_um": target_um, "add_ul": signif(volume_ul, 4) }));
                    formulas.push("uL to add = pmol / target uM".to_string());
                }
            }
        }
    } else if args.has("volume_ul") || args.has("target_um") {
        return fail("say how much DNA you have as ng, pmol or molecules before asking for a concentration");
    }

    if let Some(insert) = piece(session, args, "insert_")? {
        let ratio = args.opt_num("molar_ratio")?.unwrap_or(3.0);
        if ratio <= 0.0 {
            return fail("molar_ratio must be more than zero");
        }
        let Some(vector_pmol) = pmol else {
            return fail("say how much vector you have as ng, pmol or molecules to get the insert amount");
        };
        let (insert_weight, insert_basis) = weight_of(&insert, !single)?;
        let insert_pmol = vector_pmol * ratio;
        map.insert(
            "ligation".into(),
            json!({
                "molar_ratio_insert_to_vector": ratio,
                "vector": { "of": vector.of, "length": vector.length, "have": have(vector_pmol, weight) },
                "insert": {
                    "of": insert.of,
                    "length": insert.length,
                    "molecular_weight_g_per_mol": signif(insert_weight, 6),
                    "molecular_weight_from": insert_basis.words(),
                    "add": have(insert_pmol, insert_weight),
                },
            }),
        );
        formulas.push("insert pmol = vector pmol x molar ratio; insert ng = insert pmol x insert molecular weight / 1000".to_string());
    } else if args.has("molar_ratio") {
        return fail("give the insert as insert_plasmid, insert_sequence or insert_length_bp to use molar_ratio");
    }

    map.insert("formulas".into(), json!(formulas));
    Ok(out)
}

/// The residues a caller pointed at, and what to call them in the answer.
struct Protein {
    of: String,
    residues: String,
    had_stop: bool,
    code: u64,
}

/// A protein's residues: upper case, no white space, with one trailing stop taken off.
fn residues_of(protein: &str) -> (String, bool) {
    let kept: String = protein.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_uppercase()).collect();
    match kept.strip_suffix('*') {
        Some(without) => (without.to_string(), true),
        None => (kept, false),
    }
}

fn protein_from_feature(session: &Session, args: &Args, reference: &str) -> Result<Protein, Failure> {
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let index = refs::feature(c, reference)?;
    let Some(f) = c.features.get(index) else {
        return fail("that feature is gone");
    };
    if f.locations().is_some() || f.location_unsupported.is_some() {
        return fail("that feature has several pieces; give start and end of one piece");
    }
    let dna = slice(c, f.start as i64 + 1, f.end as i64 + 1);
    let dna = if f.strand == Strand::Reverse { seq::revcomp(&dna) } else { dna };
    // The feature's own qualifiers say how it is read: its codon_start, its partial ends, and its
    // transl_table unless the caller named a code themselves.
    let mut settings = coding::read_coding_settings(&f.notes);
    if args.has("code") {
        settings.table_id = code_of(args)?.id;
        // A transl_table Bios does not carry is the one objection the caller has just answered.
        if settings.unavailable.as_deref() == Some("transl_table is not a supported NCBI genetic code") {
            settings.unavailable = None;
        }
    }
    let table_id = settings.table_id;
    let translated = coding::translate_coding(&dna, settings);
    if let Some(reason) = translated.unavailable {
        return fail(format!("that feature does not establish a verified translation: {reason}"));
    }
    let (residues, had_stop) = residues_of(&translated.protein);
    Ok(Protein { of: format!("{} {}", refs::label(&loaded.path), f.name), residues, had_stop, code: table_id })
}

/// A range of a plasmid, or the whole of it, read in one frame.
fn protein_from_range(session: &Session, args: &Args) -> Result<Protein, Failure> {
    let code = code_of(args)?;
    let loaded = session.load(args)?;
    let c = &loaded.construct;
    let whole = c.len() as i64;
    let (start, end) = (args.opt_int("start")?.unwrap_or(1), args.opt_int("end")?.unwrap_or(whole));
    in_range(c, start, end)?;
    let dna = slice(c, start, end);
    let frame = args.opt_int("frame")?.unwrap_or(1);
    let read = match frame {
        1..=3 => dna.get((frame - 1) as usize..).unwrap_or_default().to_string(),
        -3..=-1 => seq::revcomp(&dna).get((-frame - 1) as usize..).unwrap_or_default().to_string(),
        _ => return fail("frame must be 1, 2, 3, -1, -2 or -3"),
    };
    let (residues, had_stop) = residues_of(&seq::translate_with(&read, code.id).unwrap_or_default());
    Ok(Protein { of: format!("{} {start}..{end} frame {frame}", refs::label(&loaded.path)), residues, had_stop, code: code.id })
}

fn protein_stats(session: &Session, args: &Args) -> Result<Value, Failure> {
    let found = if let Some(raw) = args.opt_str("protein")? {
        let (residues, had_stop) = residues_of(&raw);
        Protein { of: format!("{} residues given", residues.chars().count()), residues, had_stop, code: code_of(args)?.id }
    } else if let Some(reference) = args.opt_str("feature")? {
        protein_from_feature(session, args, &reference)?
    } else if args.has("plasmid") {
        protein_from_range(session, args)?
    } else {
        return fail("say where the protein comes from: a plasmid with a feature, a range of a plasmid, or protein");
    };

    if found.residues.is_empty() {
        return fail("that is no protein: it has no residues");
    }
    let unknown: Vec<char> = found.residues.chars().filter(|aa| q::residue_weight(*aa).is_none()).collect();
    let weight = q::protein_weight(&found.residues);
    let (trp, tyr, cys) = (q::residue_count(&found.residues, 'W'), q::residue_count(&found.residues, 'Y'), q::residue_count(&found.residues, 'C'));
    let reduced = q::extinction_280(trp, tyr, 0);
    let oxidised = q::extinction_280(trp, tyr, cys / 2);
    let total = found.residues.chars().count();
    let mut composition: Vec<(char, usize)> = q::composition(&found.residues);
    composition.sort_by_key(|(aa, _)| *aa);
    let composition: Vec<Value> = composition
        .into_iter()
        .map(|(aa, count)| json!({ "residue": aa.to_string(), "count": count, "percent": signif(count as f64 / total as f64 * 100.0, 3) }))
        .collect();

    let code = coding::genetic_code(found.code);
    Ok(json!({
        "of": found.of,
        "code": found.code,
        "code_name": code.map(coding::code_name),
        "residues": total,
        "protein": found.residues,
        "ended_at_a_stop": found.had_stop,
        "unknown_residues": unknown.iter().collect::<String>(),
        "molecular_weight_da": weight.map(|w| signif(w, 7)),
        "molecular_weight_kda": weight.map(|w| signif(w / 1000.0, 4)),
        "composition": composition,
        "tryptophan": trp,
        "tyrosine": tyr,
        "cysteine": cys,
        "extinction_280_m1cm1": {
            "cysteines_reduced": reduced,
            "all_cysteines_as_cystines": oxidised,
        },
        "a280_of_1_mg_per_ml": weight.filter(|w| *w > 0.0).map(|w| signif(reduced / w, 3)),
        "formulas": [
            "molecular weight = sum of the average residue masses + one water (18.015 g/mol)",
            "extinction at 280 nm = 5500 x Trp + 1490 x Tyr + 125 x cystine (Pace et al. 1995, Protein Science 4:2411-2423)",
            "A280 of a 1 mg/mL solution = extinction / molecular weight; concentration mg/mL = A280 x molecular weight / extinction",
        ],
    }))
}
