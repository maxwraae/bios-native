//! Verify: a sequencing run dropped in, its samples placed on maps, aligned, and called.
//!
//! A verification is a folder of maps (copies, so what it says stays true however the original is
//! edited) with the reads that came back and every alignment made from them, in the folder's own
//! drawer. Ported from the web's `verify/` (LocalVerify, FolderVerify, Board, AlignmentView).
//!
//! THE APP ALIGNS AND SHOWS. THE PERSON DECIDES. What the mapper found and what a person called are
//! stored apart, and no command ever fills in a call.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bios_core::Construct;
use bios_core::reads::fastq::is_reads_file;
use bios_core::reads::pairs::{SNIFF_BYTES, first_record_id, group_reads};
use bios_core::verify::folder::{Call, FolderMeta, RunSummary, StagedSample, StoredReads, VerificationMap, folder_name_for, ref_hash_of, run_stale};
use bios_core::verify::mapper::{self, ChipFeature, MapperOutput, MapperReference, ReferenceFeature};
use bios_core::verify::names::unplaced_hint;
use bios_core::verify::runs::{self, call_order, good_wells, next_to_call, run_note, run_tone, summary_line};
use bios_core::{Strand, zip_write};
use bios_project::{Change, ConstructId, Project};
use serde_json::{Value, json};

use super::PLASMID;
use crate::verify::{MapJob, MapReads, load_meta, load_run, reads_file, save_meta, save_run, verifications};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const FOLDER: Param =
    Param { name: "folder", kind: Kind::String, required: true, description: "The verification: its folder, as list_verifications shows it." };
const SAMPLE: Param = Param {
    name: "sample",
    kind: Kind::String,
    required: true,
    description: "The sample: a single read file's name, or a pair's shared stem, as read_verification shows it.",
};
const MAP: Param = Param {
    name: "map",
    kind: Kind::String,
    required: false,
    description: "Which of the folder's maps the alignment is against: its name or id. Needed only when the sample was aligned against more than one.",
};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "map_reads",
        description: "Map paired Illumina sequencing reads (FASTQ, plain or .gz) to a plasmid to verify it, with the same mapper and settings as the Bios web app (insert size 200, up to 5 iterations, paired-overhang trimming, weighted-60 consensus). Give r1 and r2, or one interleaved file as reads. Writes the alignment (SAM), read rows, padded reference, consensus and per-column coverage to a folder outside the project and returns a summary with every place the consensus differs from the plasmid. Changes nothing in the project. Large runs take a while (tens of thousands of pairs: seconds to a minute). A build of Bios without a read mapper keeps this command and answers that read mapping is not in this build.",
        params: &[
            PLASMID,
            Param { name: "r1", kind: Kind::String, required: false, description: "Full path of the R1 FASTQ (~ allowed). Use with r2." },
            Param { name: "r2", kind: Kind::String, required: false, description: "Full path of the R2 FASTQ (~ allowed). Use with r1." },
            Param {
                name: "reads",
                kind: Kind::String,
                required: false,
                description: "Full path of one interleaved FASTQ (R1, R2, R1, R2, ...), instead of r1 and r2.",
            },
            Param {
                name: "output",
                kind: Kind::String,
                required: false,
                description: "Folder to write the results to, outside the project; it must not hold earlier results. Default: a new folder in the system's temporary folder.",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(crate::mapping::map_reads),
    },
    CommandSpec {
        name: "create_verification",
        description: "Make a verification: a folder for one sequencing run, where reads are placed on maps, aligned and called. Its name is what the lab calls the run (default: today's date). Returns the folder.",
        params: &[Param {
            name: "name",
            kind: Kind::String,
            required: false,
            description: "What to call the run, such as \"260416 Tn5\". Default: today's date.",
        }],
        effect: Effect::Write,
        run: Run::Call(create_verification),
    },
    CommandSpec {
        name: "list_verifications",
        description: "List the verifications in this project: each folder with its maps and how many samples are waiting, aligned and called.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(list_verifications),
    },
    CommandSpec {
        name: "read_verification",
        description: "Read one verification: its maps (each with its samples), the samples not placed on any map, and every alignment with what the app found and what a person called.",
        params: &[FOLDER],
        effect: Effect::Read,
        run: Run::Call(read_verification),
    },
    CommandSpec {
        name: "rename_verification",
        description: "Rename a verification. Its maps, reads and alignments go with it.",
        params: &[FOLDER, Param { name: "name", kind: Kind::String, required: true, description: "The new name." }],
        effect: Effect::Write,
        run: Run::Call(rename_verification),
    },
    CommandSpec {
        name: "delete_verification",
        description: "Delete a verification: its map copies, its reads and its alignments. ALWAYS ask the person first and only call with confirm:true after they say yes. The maps it copied from are not touched.",
        params: &[FOLDER, Param { name: "confirm", kind: Kind::Boolean, required: true, description: "Must be true, after the person said yes." }],
        effect: Effect::Write,
        run: Run::Call(delete_verification),
    },
    CommandSpec {
        name: "add_verification_map",
        description: "Add a map to a verification, as a copy kept in its folder: a plasmid already in the project (plasmid) or a sequence file (file). Samples waiting whose names carry the map's name are placed on it.",
        params: &[
            FOLDER,
            Param { name: "plasmid", kind: Kind::String, required: false, description: "A plasmid in this project to copy in." },
            Param {
                name: "file",
                kind: Kind::String,
                required: false,
                description: "Full path of a sequence file (GenBank, FASTA, SnapGene...) to copy in.",
            },
        ],
        effect: Effect::Write,
        run: Run::Call(add_verification_map),
    },
    CommandSpec {
        name: "change_verification_map",
        description: "Swap one of a verification's maps for a copy of another plasmid. Alignments made against the old map stay readable and are marked stale; samples waiting on it move to the new one.",
        params: &[
            FOLDER,
            Param { name: "map", kind: Kind::String, required: true, description: "The map to replace: its name or id." },
            Param { name: "plasmid", kind: Kind::String, required: true, description: "The plasmid in this project to copy in instead." },
        ],
        effect: Effect::Write,
        run: Run::Call(change_verification_map),
    },
    CommandSpec {
        name: "add_reads",
        description: "Add sequencing reads (FASTQ, plain or gzipped) to a verification. They are copied into its folder, R1 and R2 are paired by name or by their first read's cluster id, and each sample is placed on the map its name fits (or on `map` when given). Nothing is aligned until run_verification.",
        params: &[
            FOLDER,
            Param { name: "files", kind: Kind::Strings, required: true, description: "Full paths of the read files." },
            Param { name: "map", kind: Kind::String, required: false, description: "Put every sample on this map instead of matching by name." },
        ],
        effect: Effect::Write,
        run: Run::Call(add_reads),
    },
    CommandSpec {
        name: "place_samples",
        description: "Move samples that have not been aligned onto a map, or off every map (map omitted). A placement made here is never moved by the name matcher.",
        params: &[
            FOLDER,
            Param { name: "samples", kind: Kind::Strings, required: true, description: "The samples, by name." },
            Param { name: "map", kind: Kind::String, required: false, description: "The map: its name or id. Omit to take them off every map." },
        ],
        effect: Effect::Write,
        run: Run::Call(place_samples),
    },
    CommandSpec {
        name: "match_samples",
        description: "Place every sample that is on no map by its file name, where the name fits one map clearly better than any other.",
        params: &[FOLDER],
        effect: Effect::Write,
        run: Run::Call(match_samples),
    },
    CommandSpec {
        name: "remove_sample",
        description: "Leave a sample out of a verification: take it off the board and delete its read files from the folder. Only for samples not yet aligned.",
        params: &[FOLDER, SAMPLE],
        effect: Effect::Write,
        run: Run::Call(remove_sample),
    },
    CommandSpec {
        name: "run_verification",
        description: "Align every sample that is placed on a map and not yet aligned (or just `samples`), one after another, with the read mapper. Each alignment is kept in the folder as it finishes, with the app's finding: \"consensus matches\", \"N consensus differences\" or \"partial coverage\".",
        params: &[
            FOLDER,
            Param { name: "samples", kind: Kind::Strings, required: false, description: "Only these samples. Default: every placed sample." },
        ],
        effect: Effect::Write,
        run: Run::Call(run_verification),
    },
    CommandSpec {
        name: "rerun_alignment",
        description: "Align one sample against its map again, from the reads kept in the folder, and replace its alignment. Its call, if it had one, is cleared.",
        params: &[FOLDER, SAMPLE, MAP],
        effect: Effect::Write,
        run: Run::Call(rerun_alignment),
    },
    CommandSpec {
        name: "read_alignment",
        description: "Read one alignment: the numbers, the app's finding, every consensus difference (1-based reference positions, with the feature it falls in) and the person's call.",
        params: &[FOLDER, SAMPLE, MAP],
        effect: Effect::Read,
        run: Run::Call(read_alignment),
    },
    CommandSpec {
        name: "call_sample",
        description: "Record a person's call on an alignment: good, bad (not good) or unsure (cannot tell); omit call to clear it. ONLY on the person's word: never call a sample yourself. The app's finding is kept beside the call.",
        params: &[
            FOLDER,
            SAMPLE,
            MAP,
            Param { name: "call", kind: Kind::String, required: false, description: "good, bad or unsure. Omit to clear the call." },
        ],
        effect: Effect::Write,
        run: Run::Call(call_sample),
    },
    CommandSpec {
        name: "delete_alignment",
        description: "Delete one alignment from a verification. Its reads stay in the folder. ALWAYS ask the person first.",
        params: &[FOLDER, SAMPLE, MAP],
        effect: Effect::Write,
        run: Run::Call(delete_alignment),
    },
    CommandSpec {
        name: "copy_picks",
        description: "The picks: one line per file called good, its file name and its map's name, tab-separated, ready to paste into a sheet.",
        params: &[FOLDER],
        effect: Effect::Read,
        run: Run::Call(copy_picks),
    },
    CommandSpec {
        name: "export_alignment",
        description: "Write one alignment's files outside the project (only when the person asks for a file): consensus (FASTA), sam, coverage (TSV of coverage and quality), differences (TSV), or zip (all of them with the map).",
        params: &[
            FOLDER,
            SAMPLE,
            MAP,
            Param { name: "what", kind: Kind::String, required: true, description: "consensus, sam, coverage, differences or zip." },
            Param { name: "path", kind: Kind::String, required: true, description: "Full path of the file to write (~ allowed)." },
            Param { name: "overwrite", kind: Kind::Boolean, required: false, description: "Replace the file if it exists. Default false." },
        ],
        effect: Effect::Write,
        run: Run::Call(export_alignment),
    },
];

// ── helpers ──────────────────────────────────────────────────────────────────────────────────

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn folder_arg(args: &Args) -> Result<String, Failure> {
    Ok(args.str("folder")?.trim_matches('/').to_string())
}

fn meta_of(session: &Session, folder: &str) -> Result<FolderMeta, Failure> {
    load_meta(session.project(), folder).map_err(Failure)?.map_or_else(|| fail(format!("{folder} is not a verification in this project")), Ok)
}

fn save(session: &Session, folder: &str, meta: &FolderMeta) -> Result<(), Failure> {
    save_meta(session.project(), folder, meta).map_err(Failure)
}

/// The folder's maps, as `(id, name, construct)`, in the folder's order. Maps whose copy is gone
/// are left out.
fn maps_of(session: &Session, meta: &FolderMeta) -> Result<Vec<(String, String, Construct)>, Failure> {
    let listed = session.project().list()?;
    let mut out = Vec::new();
    for m in meta.maps() {
        let Some(id) = ConstructId::parse(&m.construct_id) else { continue };
        if !listed.iter().any(|l| l.id == id) {
            continue;
        }
        let loaded = session.project().read(&id)?;
        out.push((m.construct_id.clone(), loaded.construct.name.clone(), loaded.construct));
    }
    Ok(out)
}

fn id_names(maps: &[(String, String, Construct)]) -> Vec<(String, String)> {
    maps.iter().map(|(id, name, _)| (id.clone(), name.clone())).collect()
}

/// A map named by its name or id.
fn map_named(maps: &[(String, String, Construct)], wanted: &str) -> Result<String, Failure> {
    if let Some((id, ..)) = maps.iter().find(|(id, ..)| id == wanted) {
        return Ok(id.clone());
    }
    let hits: Vec<&String> = maps.iter().filter(|(_, name, _)| name.eq_ignore_ascii_case(wanted)).map(|(id, ..)| id).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => fail(format!("this verification has no map called \"{wanted}\"")),
        _ => fail(format!("\"{wanted}\" names more than one map here; use its id")),
    }
}

/// The map a stored alignment of `sample` is against: `map` when given, otherwise the only one.
fn run_map(meta: &FolderMeta, maps: &[(String, String, Construct)], sample: &str, map: Option<String>) -> Result<Option<String>, Failure> {
    if let Some(wanted) = map {
        return map_named(maps, &wanted).map(Some).or_else(|e| {
            // A map whose copy has gone can still be named by id.
            if meta.runs.iter().any(|r| r.name == sample && meta.map_id_of(r).as_deref() == Some(wanted.as_str())) {
                Ok(Some(wanted))
            } else {
                Err(e)
            }
        });
    }
    let found: Vec<Option<String>> = meta.runs.iter().filter(|r| r.name == sample).map(|r| meta.map_id_of(r)).collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => fail(format!("{sample} has no alignment in this verification")),
        _ => fail(format!("{sample} was aligned against more than one map; say which with map")),
    }
}

/// The construct as the mapper's reference: uppercase bases and its features.
fn reference_of(c: &Construct) -> MapperReference {
    MapperReference {
        name: c.name.clone(),
        bases: c.sequence.to_uppercase(),
        circular: c.circular,
        features: c
            .features
            .iter()
            .map(|f| ReferenceFeature {
                name: f.name.clone(),
                kind: f.kind.clone(),
                start: f.start,
                end: f.end,
                strand: if f.strand == Strand::Reverse { -1 } else { 1 },
            })
            .collect(),
    }
}

fn full_path(given: &str) -> Result<PathBuf, Failure> {
    let path = match given.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)).unwrap_or_else(|| PathBuf::from(given)),
        None => PathBuf::from(given),
    };
    if !path.is_absolute() {
        return fail(format!("{given} is not a full path; give the file's absolute path"));
    }
    Ok(path)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn summary_json(meta: &FolderMeta, maps: &[(String, String, Construct)], r: &RunSummary) -> Value {
    let map_id = meta.map_id_of(r);
    let map = map_id.as_ref().and_then(|id| maps.iter().find(|(m, ..)| m == id));
    let stale = run_stale(r.ref_length, r.ref_hash.as_deref(), map.map(|(_, _, c)| c.sequence.as_str()));
    json!({
        "sample": r.name,
        "map": map.map(|(_, name, _)| name.clone()),
        "map_id": map_id,
        "read_files": r.read_files,
        "aligned_at": r.added_at,
        "found": r.verdict,
        "tone": run_tone(r).as_str(),
        "note": run_note(r),
        "numbers": summary_line(r),
        "call": r.call.map(Call::as_str),
        "called_at": r.called_at,
        "called_by": r.called_by,
        "overruled": runs::disagrees(r),
        "stale": stale,
    })
}

// ── the folder ───────────────────────────────────────────────────────────────────────────────

fn create_verification(session: &Session, args: &Args) -> Result<Value, Failure> {
    let wanted = args.opt_str("name")?.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(bios_project::folder_stamp);
    let base = folder_name_for(&wanted, "verification");
    let taken: HashSet<String> = session.project().folders()?.iter().map(|p| refs::label(p)).collect();
    let name = std::iter::once(base.clone())
        .chain((2..=50).map(|n| format!("{base} ({n})")))
        .find(|n| !taken.contains(n))
        .unwrap_or_else(|| format!("{base} {}", chrono::Utc::now().timestamp_millis()));
    session.project().create_folder(&name)?;
    save(session, &name, &FolderMeta::new(&now()))?;
    Ok(json!({ "created": name }))
}

fn list_verifications(session: &Session, _: &Args) -> Result<Value, Failure> {
    let mut out = Vec::new();
    for (folder, meta) in verifications(session.project()) {
        let maps = maps_of(session, &meta)?;
        out.push(json!({
            "folder": folder,
            "maps": maps.iter().map(|(_, name, _)| name.clone()).collect::<Vec<_>>(),
            "waiting": meta.staged.len(),
            "aligned": meta.runs.len(),
            "called": meta.runs.iter().filter(|r| r.call.is_some()).count(),
        }));
    }
    Ok(json!({ "verifications": out }))
}

fn read_verification(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let ordered = call_order(&meta);
    let rows: Vec<Value> = maps
        .iter()
        .map(|(id, name, c)| {
            json!({
                "map": name,
                "id": id,
                "length": c.len(),
                "waiting": meta.staged.iter().filter(|s| s.map_id.as_deref() == Some(id.as_str())).map(|s| json!({ "sample": s.name, "files": s.files })).collect::<Vec<_>>(),
                "aligned": ordered.iter().filter(|r| meta.map_id_of(r).as_deref() == Some(id.as_str())).map(|r| summary_json(&meta, &maps, r)).collect::<Vec<_>>(),
            })
        })
        .collect();
    let tray: Vec<&StagedSample> = meta.staged.iter().filter(|s| s.map_id.is_none()).collect();
    let placed: Vec<String> =
        meta.staged.iter().filter(|s| s.map_id.is_some()).map(|s| s.name.clone()).chain(meta.runs.iter().map(|r| r.name.clone())).collect();
    let hint =
        unplaced_hint(&tray.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), &placed, &maps.iter().map(|(_, n, _)| n.clone()).collect::<Vec<_>>());
    let here: HashSet<&String> = maps.iter().map(|(id, ..)| id).collect();
    let orphaned: Vec<Value> =
        ordered.iter().filter(|r| !meta.map_id_of(r).is_some_and(|id| here.contains(&id))).map(|r| summary_json(&meta, &maps, r)).collect();
    Ok(json!({
        "folder": folder,
        "created_at": meta.created_at,
        "maps": rows,
        "not_placed": tray.iter().map(|s| json!({ "sample": s.name, "files": s.files })).collect::<Vec<_>>(),
        "not_placed_hint": hint,
        "alignments_without_a_map": orphaned,
        "reads": meta.reads.iter().map(|r| json!({ "name": r.name, "bytes": r.bytes })).collect::<Vec<_>>(),
    }))
}

fn rename_verification(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    meta_of(session, &folder)?;
    let wanted = args.str("name")?;
    let wanted = wanted.trim();
    let here = folder.rsplit('/').next().unwrap_or(&folder);
    if wanted.is_empty() || wanted == here {
        return Ok(json!({ "folder": folder, "unchanged": true }));
    }
    let parent = &folder[..folder.len() - here.len()];
    let to = format!("{parent}{}", folder_name_for(wanted, "verification"));
    let moved = session.project().rename_folder(&folder, &to)?;
    Ok(json!({ "folder": refs::label(&moved) }))
}

fn delete_verification(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    if args.opt_bool("confirm")? != Some(true) {
        return fail("ask the person first, then call again with confirm:true");
    }
    let meta = meta_of(session, &folder)?;
    let rel = PathBuf::from(&folder);
    // Every map, and anything else that ended up in the folder: the folder is removed only empty.
    for listed in session.project().list()?.into_iter().filter(|l| l.path.starts_with(&rel)) {
        session.project().delete(&listed.id, session.who())?;
    }
    session.project().remove_folder(&folder)?;
    Ok(json!({ "deleted": folder, "maps": meta.maps().len(), "alignments": meta.runs.len() }))
}

/// Copies `construct` into the folder as a new map, registered last so the folder never names a
/// map that is not there.
fn adopt_map(session: &Session, folder: &str, construct: Construct, from: Option<&ConstructId>) -> Result<(ConstructId, String), Failure> {
    let summary = format!("Copied into the verification {folder}");
    let change = Change { command: "add_verification_map".into(), params: json!({ "folder": folder }), summary };
    let made = session.project().create_construct(construct, folder, from, session.who(), change)?;
    Ok((made.id, made.construct.name))
}

fn add_verification_map(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    meta_of(session, &folder)?;
    let (construct, from) = match (args.opt_str("plasmid")?, args.opt_str("file")?) {
        (Some(plasmid), None) => {
            let listed = session.project().list()?;
            let id = refs::plasmid(&listed, &plasmid)?.id.clone();
            (session.project().read(&id)?.construct, Some(id))
        }
        (None, Some(file)) => {
            let path = full_path(&file)?;
            let bytes = std::fs::read(&path).map_err(|e| Failure(format!("{}: {e}", path.display())))?;
            (bios_core::formats::read(&bytes, &file_name(&path)).map_err(Failure)?.construct, None)
        }
        _ => return fail("give either plasmid or file"),
    };
    let (id, name) = adopt_map(session, &folder, construct, from.as_ref())?;
    let mut meta = meta_of(session, &folder)?;
    meta.maps = meta.maps();
    meta.map_construct_id = None;
    meta.maps.push(VerificationMap { construct_id: id.as_str().to_string(), added_at: now() });
    let maps = maps_of(session, &meta)?;
    meta.place_loose(&id_names(&maps));
    save(session, &folder, &meta)?;
    let placed = meta.staged.iter().filter(|s| s.map_id.as_deref() == Some(id.as_str())).count();
    Ok(json!({ "added": name, "id": id.as_str(), "samples_placed": placed }))
}

fn change_verification_map(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let old = map_named(&maps, &args.str("map")?)?;
    let listed = session.project().list()?;
    let from = refs::plasmid(&listed, &args.str("plasmid")?)?.id.clone();
    let construct = session.project().read(&from)?.construct;
    let (id, name) = adopt_map(session, &folder, construct, Some(&from))?;
    let mut meta = meta_of(session, &folder)?;
    meta.maps = meta.maps();
    meta.map_construct_id = None;
    // The old alignments keep naming the map they were measured against, which is going: they say
    // so by being stale against the new one, so they are moved to it rather than orphaned.
    for r in meta.runs.iter_mut() {
        if r.map_id.as_deref().is_none_or(|m| m == old) {
            r.map_id = Some(id.as_str().to_string());
        }
    }
    for s in meta.staged.iter_mut().filter(|s| s.map_id.as_deref() == Some(old.as_str())) {
        s.map_id = Some(id.as_str().to_string());
    }
    if let Some(slot) = meta.maps.iter_mut().find(|m| m.construct_id == old) {
        *slot = VerificationMap { construct_id: id.as_str().to_string(), added_at: now() };
    }
    save(session, &folder, &meta)?;
    // Only once the folder points at the new copy: it must never point at nothing.
    if let Some(old_id) = ConstructId::parse(&old) {
        session.project().delete(&old_id, session.who())?;
    }
    Ok(json!({ "map": name, "id": id.as_str(), "stale_alignments": meta.runs.iter().filter(|r| r.map_id.as_deref() == Some(id.as_str())).count() }))
}

// ── the board ────────────────────────────────────────────────────────────────────────────────

fn add_reads(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let mut meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let onto = args.opt_str("map")?.map(|m| map_named(&maps, &m)).transpose()?;
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut refused = Vec::new();
    for given in args.strings("files") {
        let path = full_path(&given)?;
        let name = file_name(&path);
        if !is_reads_file(&name) {
            refused.push(format!("{name} is not a read file (.fastq, .fq, gzipped or not)"));
            continue;
        }
        if !path.is_file() {
            refused.push(format!("{} is not there", path.display()));
            continue;
        }
        files.push((name, path));
    }
    // Copied in the moment they arrive: the reads go where everything else about this run goes.
    for (name, path) in &files {
        let bytes = session.project().import_folder_file(&folder, &reads_file(name), path)?;
        meta.reads.retain(|r| &r.name != name);
        meta.reads.push(StoredReads { name: name.clone(), bytes, added_at: now() });
    }
    let groups = group_reads(&files, |name, path| {
        let mut head = vec![0u8; SNIFF_BYTES];
        let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).ok()?;
        first_record_id(&head[..n], bios_core::reads::fastq::is_gzip_name(name))
    });
    let mut added = Vec::new();
    for group in groups {
        let names: Vec<String> = group.files().iter().map(|p| file_name(p)).collect();
        let sample = group.name().to_string();
        // Already on the board or aligned: a second drop of the same file is the same sample.
        if let Some(s) = meta.staged.iter_mut().find(|s| s.name == sample) {
            if let Some(m) = &onto {
                s.map_id = Some(m.clone());
                s.hand = true;
            }
            continue;
        }
        meta.staged.push(StagedSample { name: sample.clone(), files: names, map_id: onto.clone(), hand: onto.is_some() });
        added.push(sample);
    }
    meta.place_loose(&id_names(&maps));
    save(session, &folder, &meta)?;
    let samples: Vec<Value> = meta
        .staged
        .iter()
        .filter(|s| added.contains(&s.name))
        .map(|s| json!({ "sample": s.name, "files": s.files, "map": s.map_id.as_ref().and_then(|id| maps.iter().find(|(m, ..)| m == id)).map(|(_, n, _)| n.clone()) }))
        .collect();
    Ok(json!({ "added": samples, "refused": refused }))
}

fn place_samples(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let mut meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let to = args.opt_str("map")?.filter(|m| !m.is_empty()).map(|m| map_named(&maps, &m)).transpose()?;
    let wanted = args.strings("samples");
    let mut moved = 0;
    for s in meta.staged.iter_mut().filter(|s| wanted.contains(&s.name)) {
        s.map_id = to.clone();
        s.hand = true;
        moved += 1;
    }
    if moved < wanted.len() {
        return fail("some of those samples are not waiting on this board (aligned samples cannot be moved)");
    }
    save(session, &folder, &meta)?;
    Ok(json!({ "moved": moved }))
}

fn match_samples(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let mut meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let placed = meta.try_match(&id_names(&maps));
    save(session, &folder, &meta)?;
    Ok(json!({ "placed": placed, "still_not_placed": meta.staged.iter().filter(|s| s.map_id.is_none()).count() }))
}

fn remove_sample(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let mut meta = meta_of(session, &folder)?;
    let Some(at) = meta.staged.iter().position(|s| s.name == sample) else {
        return fail(format!("{sample} is not waiting on this board"));
    };
    let gone = meta.staged.remove(at);
    // Only files no alignment came from: "leave this file out" never takes evidence away.
    let used: HashSet<String> = meta.runs.iter().flat_map(|r| r.read_files.clone().unwrap_or_default()).collect();
    for file in gone.files.iter().filter(|f| !used.contains(*f)) {
        session.project().remove_folder_file(&folder, &reads_file(file))?;
        meta.reads.retain(|r| &r.name != file);
    }
    save(session, &folder, &meta)?;
    Ok(json!({ "removed": sample }))
}

// ── running ──────────────────────────────────────────────────────────────────────────────────

/// One stored read file's bytes, as they were sent.
fn stored_bytes(project: &Project, folder: &str, name: &str) -> Result<Vec<u8>, String> {
    let mut file = project
        .open_folder_file(folder, &reads_file(name))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{name} is not in the folder any more"))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(bytes)
}

/// Aligns one sample's files against one map and keeps the result. The record is read again
/// just before it is written, so a placement made meanwhile is not lost.
fn align_one(session: &Session, folder: &str, sample: &str, files: &[String], map_id: &str, construct: &Construct) -> Result<RunSummary, String> {
    let control = session.control();
    control.say(&format!("{sample} · reading the reads"));
    let reads = match files {
        [one] => MapReads::Interlaced(stored_bytes(session.project(), folder, one)?),
        [r1, r2] => MapReads::Pair { r1: stored_bytes(session.project(), folder, r1)?, r2: stored_bytes(session.project(), folder, r2)? },
        _ => return Err(format!("{sample} has no read files")),
    };
    if control.cancelled() {
        return Err("cancelled".into());
    }
    let reference = reference_of(construct);
    control.say(&format!("{sample} · aligning"));
    let output: MapperOutput = session.mapper().map(&MapJob { reference: reference.clone(), reads }, &control)?;
    mapper::validate(&output)?;
    if output.reference.bases != reference.bases {
        return Err("the mapper answered for a different reference".into());
    }
    save_run(session.project(), folder, sample, Some(map_id), &output)?;
    let summary = mapper::summary_of(sample, &output, &now(), &ref_hash_of(&reference.bases), map_id, files);
    let mut meta = load_meta(session.project(), folder)?.ok_or_else(|| format!("{folder} is not a verification any more"))?;
    let first = meta.maps().first().map(|m| m.construct_id.clone());
    meta.runs.retain(|r| !(r.name == sample && r.map_id.clone().or_else(|| first.clone()).as_deref() == Some(map_id)));
    meta.runs.push(summary.clone());
    meta.staged.retain(|s| s.name != sample);
    save_meta(session.project(), folder, &meta)?;
    Ok(summary)
}

fn run_verification(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let only = args.strings("samples");
    let todo: Vec<StagedSample> = meta.staged.iter().filter(|s| s.map_id.is_some() && (only.is_empty() || only.contains(&s.name))).cloned().collect();
    if todo.is_empty() {
        return fail("no sample is placed on a map; place them first (add_reads, place_samples or match_samples)");
    }
    let control = session.control();
    let (mut ran, mut failed) = (Vec::new(), Vec::new());
    let mut cancelled = false;
    for (i, s) in todo.iter().enumerate() {
        if control.cancelled() {
            cancelled = true;
            break;
        }
        control.say(&format!("{} of {} · {}", i + 1, todo.len(), s.name));
        let Some((map_id, _, construct)) = maps.iter().find(|(id, ..)| Some(id) == s.map_id.as_ref()) else {
            failed.push(json!({ "sample": s.name, "error": "its map is not in the folder" }));
            continue;
        };
        match align_one(session, &folder, &s.name, &s.files, map_id, construct) {
            Ok(summary) => ran.push(summary_json(&meta, &maps, &summary)),
            Err(e) if control.cancelled() => {
                cancelled = true;
                failed.push(json!({ "sample": s.name, "error": e }));
                break;
            }
            Err(e) => failed.push(json!({ "sample": s.name, "error": e })),
        }
    }
    Ok(json!({ "aligned": ran, "failed": failed, "cancelled": cancelled }))
}

fn rerun_alignment(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let map_id = run_map(&meta, &maps, &sample, args.opt_str("map")?)?.ok_or_else(|| Failure("that alignment names no map".into()))?;
    let Some((_, _, construct)) = maps.iter().find(|(id, ..)| *id == map_id) else {
        return fail("the map this was aligned against is not in the folder any more");
    };
    let run = meta.run(&sample, Some(&map_id)).cloned().ok_or_else(|| Failure(format!("{sample} has no alignment against that map")))?;
    let files = run.read_files.clone().unwrap_or_else(|| meta.reads.iter().map(|r| r.name.clone()).filter(|n| n.starts_with(&sample)).collect());
    let kept: Vec<String> = files.iter().filter(|f| meta.reads.iter().any(|r| &r.name == *f)).cloned().collect();
    // A missing file is not a run: aligning the half that is left answers a different question.
    if kept.is_empty() || kept.len() != files.len() {
        return fail(if kept.is_empty() {
            "the reads for this alignment are not in the folder any more".to_string()
        } else {
            format!("only {} of {} read files are still in the folder", kept.len(), files.len())
        });
    }
    let summary = align_one(session, &folder, &sample, &kept, &map_id, construct).map_err(Failure)?;
    Ok(summary_json(&meta, &maps, &summary))
}

// ── reading and calling ──────────────────────────────────────────────────────────────────────

fn read_alignment(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let map_id = run_map(&meta, &maps, &sample, args.opt_str("map")?)?;
    let run = meta.run(&sample, map_id.as_deref()).cloned().ok_or_else(|| Failure(format!("{sample} has no alignment against that map")))?;
    let output = load_run(session.project(), &folder, &sample, map_id.as_deref()).map_err(Failure)?;
    let reading = mapper::read(&output);
    let features: Vec<ChipFeature> =
        output.reference.features.iter().map(|f| ChipFeature { name: f.name.clone(), start: f.start, end: f.end }).collect();
    let differences: Vec<Value> = reading
        .differences
        .iter()
        .map(|d| {
            let feature = mapper::feature_at(&features, d.reference_pos, reading.ref_length);
            json!({
                "position": d.reference_pos + 1,
                "column": d.column + 1,
                "expected": d.expected,
                "observed": d.observed,
                "kind": d.indel_kind.map_or("substitution", |k| if k == mapper::IndelKind::Insertion { "insertion" } else { "deletion" }),
                "size": d.size,
                "coverage": output.final_consensus.coverage.get(d.column),
                "quality": output.final_consensus.quality.get(d.column),
                "feature": feature,
                "label": mapper::chip_label(d.reference_pos, d, feature.as_deref()),
            })
        })
        .collect();
    let mut out = summary_json(&meta, &maps, &run);
    if let Some(map) = out.as_object_mut() {
        map.insert("numbers".into(), json!(mapper::header_numbers(&reading)));
        map.insert("words".into(), json!(mapper::header_words(&reading).iter().map(|t| t.text.clone()).collect::<Vec<_>>()));
        map.insert("reads".into(), json!({ "total": reading.total_reads, "mapped": reading.mapped_reads }));
        map.insert("differences".into(), json!(differences));
        let next = next_to_call(&meta, &sample, map_id.as_deref(), |_| false);
        map.insert("next_to_call".into(), json!(next.map(|r| r.name)));
    }
    Ok(out)
}

fn call_sample(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let call = match args.opt_str("call")?.filter(|c| !c.is_empty()) {
        None => None,
        Some(text) => Some(Call::parse(&text).ok_or_else(|| Failure(format!("call is good, bad or unsure, not {text}")))?),
    };
    let mut meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let map_id = run_map(&meta, &maps, &sample, args.opt_str("map")?)?;
    let at = meta
        .runs
        .iter()
        .position(|r| r.name == sample && meta.map_id_of(r) == map_id)
        .ok_or_else(|| Failure(format!("{sample} has no alignment against that map")))?;
    let who = match session.who().actor {
        bios_project::Actor::Agent => format!("an agent ({})", session.who().source),
        _ => "you".to_string(),
    };
    if let Some(r) = meta.runs.get_mut(at) {
        r.call = call;
        r.called_at = call.map(|_| now());
        r.called_by = call.map(|_| who);
    }
    save(session, &folder, &meta)?;
    let next = next_to_call(&meta, &sample, map_id.as_deref(), |_| false);
    Ok(
        json!({ "sample": sample, "call": call.map(Call::as_str), "next_to_call": next.map(|r| json!({ "sample": r.name, "map_id": meta.map_id_of(&r) })) }),
    )
}

fn delete_alignment(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let mut meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let map_id = run_map(&meta, &maps, &sample, args.opt_str("map")?)?;
    let before = meta.runs.len();
    let keep: Vec<RunSummary> = meta.runs.iter().filter(|r| !(r.name == sample && meta.map_id_of(r) == map_id)).cloned().collect();
    meta.runs = keep;
    if meta.runs.len() == before {
        return fail(format!("{sample} has no alignment against that map"));
    }
    session.project().remove_folder_file(&folder, &crate::verify::run_file(&sample, map_id.as_deref()))?;
    save(session, &folder, &meta)?;
    Ok(json!({ "deleted": sample }))
}

fn copy_picks(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let text = good_wells(&meta, |id| maps.iter().find(|(m, ..)| m == id).map(|(_, n, _)| n.clone()));
    let count = if text.is_empty() { 0 } else { text.lines().count() };
    Ok(json!({ "text": text, "good": count }))
}

fn export_alignment(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = folder_arg(args)?;
    let sample = args.str("sample")?;
    let meta = meta_of(session, &folder)?;
    let maps = maps_of(session, &meta)?;
    let map_id = run_map(&meta, &maps, &sample, args.opt_str("map")?)?;
    let output = load_run(session.project(), &folder, &sample, map_id.as_deref()).map_err(Failure)?;
    let path = full_path(&args.str("path")?)?;
    if path.starts_with(session.project().root()) {
        return fail("that path is inside the project; choose a place outside it");
    }
    let bytes: Vec<u8> = match args.str("what")?.as_str() {
        "consensus" => output.consensus_fasta.clone().into_bytes(),
        "sam" => output.sam.clone().into_bytes(),
        "coverage" => output.coverage_tsv.clone().into_bytes(),
        "differences" => output.differences_tsv.clone().into_bytes(),
        "zip" => {
            let map = map_id
                .as_ref()
                .and_then(|id| maps.iter().find(|(m, ..)| m == id))
                .map(|(_, _, c)| bios_core::genbank_write::write(c).unwrap_or_default())
                .unwrap_or_default();
            let rows: String = output.rows.iter().filter_map(|r| serde_json::to_string(r).ok()).map(|l| l + "\n").collect();
            let final_json = serde_json::to_string(&output.final_consensus).unwrap_or_default();
            let manifest = serde_json::to_string_pretty(&output.manifest).unwrap_or_default();
            let files: Vec<(&str, &[u8])> = vec![
                ("alignment.sam", output.sam.as_bytes()),
                ("sam-rows.jsonl", rows.as_bytes()),
                ("reference-aligned.seq", output.padded_reference.as_bytes()),
                ("manifest.json", manifest.as_bytes()),
                ("consensus.fasta", output.consensus_fasta.as_bytes()),
                ("final-consensus.json", final_json.as_bytes()),
                ("coverage.tsv", output.coverage_tsv.as_bytes()),
                ("column-differences.tsv", output.differences_tsv.as_bytes()),
                ("reference.gb", map.as_bytes()),
            ];
            zip_write::zip(&files).map_err(Failure)?
        }
        other => return fail(format!("what is consensus, sam, coverage, differences or zip, not {other}")),
    };
    bios_project::write_outside_bytes(&path, &bytes, args.opt_bool("overwrite")? == Some(true))?;
    Ok(json!({ "exported": sample, "to": path.display().to_string(), "bytes": bytes.len() }))
}
