//! Build: a backbone opened one way, a plasmid per insert, and one folder that holds them all.
//!
//! Ported from the web app's `build/` — `useBuild.tsx`, `startBuild.ts`, `buildFolder.ts`,
//! `designView.ts`, `order.ts` — and its folder routes.
//!
//! ## A build folder is its backbone
//!
//! Every design in one folder is a change to the same starting bases: that is what makes them
//! comparable, orderable together and assemblable in one reaction. So the folder holds its own
//! FROZEN COPY of the backbone rather than pointing at the plasmid on the bench. Edit the bench
//! copy and every design underneath it would have moved silently — the opening at a different
//! index, the arms against different flanks, the pieces already ordered no longer fitting, and
//! nothing failing. With a copy in the folder a design is a closed statement: on this day, into
//! exactly these bases, this went here.
//!
//!   same bases  → the same folder, and designing again joins it
//!   other bases → a new folder, because they are not the same campaign
//!
//! ## One cut per folder
//!
//! The folder remembers WHERE IT OPENS, not the state of the console that chose it. Building
//! again is how the cut is changed; the designs already in the folder keep the cut that made them,
//! so a change is readable rather than retroactive.
//!
//! ## On disk
//!
//! ```text
//! <folder>/<backbone>.gb              the frozen copy every design here was built into
//! <folder>/<product>.gb               one per design: an ordinary plasmid, openable anywhere
//! <folder>/.bios/folder.json          type, backbone, opening, and one small row per design
//! <folder>/.bios/designs/<name>.json  the pieces to order behind each row
//! ```
//!
//! The rows are small on purpose: they ride on every listing, so a folder can draw without reading
//! a single payload. The pieces are the one part of a design that cannot be recovered from what is
//! already on disk — the product is a construct and the backbone is the folder's copy, but the
//! pieces are the output of a search over both.

use bios_core::Construct;
use bios_core::build::{DesignPieces, OrderPiece, order_across, order_tsv, order_xlsx};
use bios_core::gibson::{DesignGibsonMultiResult, GibsonCut, GibsonCutSite, GibsonFragment, JunctionKind, OpeningArc};
use bios_project::{ConstructId, FolderMeta, Listed, folder_name_for, folder_stamp, item_file_name, pick_folder};
use serde_json::{Map, Value, json};

use super::{PLASMID, gibson};
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs};

const FOLDER: Param = Param { name: "folder", kind: Kind::String, required: true, description: "The build folder, as list_build_folders names it." };
const INSERTS: Param = Param {
    name: "inserts",
    kind: Kind::Objects,
    required: true,
    description: "What to put in the opening, as [{\"name\": \"EGFP\", \"sequence\": \"ATG...\"}]. ONE PLASMID EACH: five inserts is five designs and five plasmids, not one plasmid with five pieces in it.",
};
const ENZYMES: Param = Param {
    name: "enzymes",
    kind: Kind::Strings,
    required: false,
    description: "Open the backbone between two different restriction enzymes, each of which cuts it once: [\"EcoRI\", \"BamHI\"]. Both sites are kept on the product.",
};
const ARC: Param = Param {
    name: "arc",
    kind: Kind::String,
    required: false,
    description: "With enzymes on a circular backbone, which of the two stretches leaves: \"between\" (from the lower cut to the higher, the default) or \"origin\" (the other way, through the origin).",
};
const REPLACE_START: Param = Param {
    name: "replace_start",
    kind: Kind::Integer,
    required: false,
    description: "Or open at a stretch of bases rather than at enzymes: its first base, counted from 1. With replace_end.",
};
const REPLACE_END: Param =
    Param { name: "replace_end", kind: Kind::Integer, required: false, description: "The last base of the stretch that leaves." };
const TARGET_TM: Param =
    Param { name: "target_tm", kind: Kind::Integer, required: false, description: "Melting temperature the overlaps aim for, in °C. Default 65." };
const MAX_PIECE: Param = Param {
    name: "max_piece_length",
    kind: Kind::Integer,
    required: false,
    description: "Longest piece a supplier will make, in bases. A longer insert is split into overlapping pieces. Default 3000.",
};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "list_build_folders",
        description: "Every build folder in the project: the backbone it holds a copy of, how it opens it, and how many plasmids have been built into it. A build folder is one backbone opened one way; designing into the same bases again joins the same folder.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(list_build_folders),
    },
    CommandSpec {
        name: "read_build_folder",
        description: "One build folder in full: its backbone and cut, how much of the backbone leaves, the features the cut clips, every design in it with the pieces its recipe cites, and the table of pieces to order across the whole folder.",
        params: &[FOLDER],
        effect: Effect::Read,
        run: Run::Call(read_build_folder),
    },
    CommandSpec {
        name: "set_build_opening",
        description: "Change where a build folder opens its backbone. The designs already in it keep the cut that made them and are marked as built with an earlier cut, so nothing already ordered is quietly rewritten.",
        params: &[FOLDER, ENZYMES, ARC, REPLACE_START, REPLACE_END],
        effect: Effect::Write,
        run: Run::Call(set_build_opening),
    },
    CommandSpec {
        name: "design_builds",
        description: "Design one Gibson assembly per insert into an opened plasmid, without keeping anything. Returns, for each insert, the product's length, the pieces to order with their bases, the overlaps and their melting temperatures, the features the opening clips, and any warnings. Use build_plasmids to keep them.",
        params: &[
            PLASMID,
            INSERTS,
            ENZYMES,
            ARC,
            REPLACE_START,
            REPLACE_END,
            TARGET_TM,
            MAX_PIECE,
            Param {
                name: "with_product",
                kind: Kind::Boolean,
                required: false,
                description: "Also return each product's own bases and features, for drawing it. Off by default: a hundred inserts is a hundred plasmids, and most callers only want what to order.",
            },
        ],
        effect: Effect::Read,
        run: Run::Call(design_builds),
    },
    CommandSpec {
        name: "build_plasmids",
        description: "Design one assembly per insert and keep every product as a plasmid in a build folder. The folder holds a frozen copy of the backbone and remembers the cut; building into the same bases again joins the same folder, and a product whose name is taken is numbered rather than overwriting. Each design's pieces to order are kept with it.",
        params: &[
            PLASMID,
            INSERTS,
            ENZYMES,
            ARC,
            REPLACE_START,
            REPLACE_END,
            TARGET_TM,
            MAX_PIECE,
            Param {
                name: "names",
                kind: Kind::Strings,
                required: false,
                description: "What to call each product, in the order of the inserts. Left out, a product is the backbone's name and the insert's: \"pUC19_EGFP\".",
            },
            Param {
                name: "folder",
                kind: Kind::String,
                required: false,
                description: "Build into this folder instead of choosing one. It must already be a build folder holding this backbone.",
            },
        ],
        effect: Effect::Write,
        run: Run::Call(build_plasmids),
    },
    CommandSpec {
        name: "list_designs",
        description: "The designs in a build folder, newest first: what each product is called, how many pieces it takes, and whether it was built against the backbone and cut the folder has now.",
        params: &[FOLDER],
        effect: Effect::Read,
        run: Run::Call(list_designs),
    },
    CommandSpec {
        name: "drop_design",
        description: "Forget how one plasmid in a build folder was made. The plasmid itself stays: this removes its row and the pieces kept behind it, not the construct.",
        params: &[FOLDER, Param { name: "name", kind: Kind::String, required: true, description: "The design's name, as list_designs gives it." }],
        effect: Effect::Write,
        run: Run::Call(drop_design),
    },
    CommandSpec {
        name: "build_order_table",
        description: "The pieces to order across a whole build folder: every distinct piece once, numbered, with each design's recipe as the numbers it cites. Returns the table as text (tab-separated, ready to paste into a spreadsheet or an order form), and writes it as a spreadsheet when given a file to write.",
        params: &[
            FOLDER,
            Param {
                name: "file",
                kind: Kind::String,
                required: false,
                description: "Write the table as an .xlsx workbook to this path, outside the project. The text is returned either way.",
            },
            Param { name: "overwrite", kind: Kind::Boolean, required: false, description: "Replace the file if it is already there. Default false." },
        ],
        effect: Effect::Read,
        run: Run::Call(build_order_table),
    },
];

/// What a build folder remembers, in the shape the Bios web app writes it.
const KIND: &str = "build";
const BACKBONE: &str = "backboneConstructId";
const OPENING_KEY: &str = "opening";
const DESIGNS: &str = "designs";
const DESIGNS_DIR: &str = "designs";

// ── WHERE A BACKBONE OPENS ──────────────────────────────────────────────────────────────────

/// A folder's cut, as it is stored: two enzymes and which arc leaves, or a plain span of bases.
///
/// Region coordinates are 0-based and half the reason this is not normalised: `end < start` means
/// the span wraps the origin, which is a different cut and not a mistake.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Opening {
    Enzymes { a: String, b: String, arc: OpeningArc },
    Region { start: i64, end: i64 },
}

impl Opening {
    fn to_json(&self) -> Value {
        match self {
            Opening::Enzymes { a, b, arc } => {
                json!({ "kind": "enzymes", "a": a, "b": b, "arc": if *arc == OpeningArc::Origin { "origin" } else { "between" } })
            }
            Opening::Region { start, end } => json!({ "kind": "region", "start": start, "end": end }),
        }
    }

    fn from_json(value: &Value) -> Option<Opening> {
        match value.get("kind")?.as_str()? {
            "enzymes" => Some(Opening::Enzymes {
                a: value.get("a")?.as_str()?.to_string(),
                b: value.get("b")?.as_str()?.to_string(),
                arc: if value.get("arc").and_then(Value::as_str) == Some("origin") { OpeningArc::Origin } else { OpeningArc::Between },
            }),
            "region" => Some(Opening::Region { start: value.get("start")?.as_i64()?, end: value.get("end")?.as_i64()? }),
            _ => None,
        }
    }

    /// The same cut, as the Gibson maths takes it against `construct`.
    fn as_cut(&self, construct: &Construct) -> Result<GibsonCut, Failure> {
        match self {
            Opening::Region { start, end } => {
                let last = construct.len().saturating_sub(1) as i64;
                let (start, end) = ((*start).clamp(0, last), (*end).clamp(0, last));
                if end < start && !construct.circular {
                    return fail("that stretch runs through the origin, but this plasmid is linear");
                }
                Ok(GibsonCut::Region { start, end })
            }
            Opening::Enzymes { a, b, arc } => {
                if a.eq_ignore_ascii_case(b) {
                    return fail("pick two different enzymes");
                }
                let names = [a.as_str(), b.as_str()];
                let found = bios_core::enzymes::find_cut_sites(&construct.sequence, &names, construct.circular);
                let mut pair = Vec::new();
                for wanted in [a, b] {
                    let Some(known) = bios_core::enzymes::get_enzyme(wanted) else {
                        return fail(format!("{wanted} is not an enzyme Bios knows"));
                    };
                    match found.iter().filter(|s| s.enzyme == known.name).count() {
                        1 => pair.push(known.name.to_string()),
                        0 => return fail(format!("{} does not cut this backbone", known.name)),
                        n => return fail(format!("{} cuts {n}× — pick an enzyme that cuts once", known.name)),
                    }
                }
                if *arc == OpeningArc::Origin && !construct.circular {
                    return fail("the stretch through the origin only exists on a circular plasmid");
                }
                let sites = found
                    .iter()
                    .map(|s| GibsonCutSite {
                        enzyme: s.enzyme.to_string(),
                        position: s.top_cut as i64,
                        recognition_start: Some(s.recognition_start as i64),
                    })
                    .collect();
                let [first, second] = pair.as_slice() else {
                    return fail("give two enzymes to open the backbone between");
                };
                // KEEP THE SITES: this is a thing that goes on a bench, and the opening is placed
                // outside the sites so the pair can cut the product again.
                Ok(GibsonCut::Enzymes { enzymes: [first.clone(), second.clone()], sites, arc: Some(*arc), keep_sites: true })
            }
        }
    }

    /// How it reads in a sentence.
    fn says(&self) -> String {
        match self {
            Opening::Enzymes { a, b, .. } => format!("cut with {a} and {b}"),
            Opening::Region { start, end } => format!("opened at {}–{}", start + 1, end + 1),
        }
    }
}

/// The opening the caller asked for, in one of the two ways. `None` when they said nothing.
fn opening_from(args: &Args) -> Result<Option<Opening>, Failure> {
    let enzymes = args.strings("enzymes");
    let region = args.has("replace_start") || args.has("replace_end");
    if !enzymes.is_empty() && region {
        return fail("say where the backbone opens in one way: enzymes, or replace_start with replace_end");
    }
    if region {
        let (start, end) = (args.int("replace_start")?, args.int("replace_end")?);
        if start < 1 || end < 1 {
            return fail("replace_start and replace_end are counted from 1");
        }
        return Ok(Some(Opening::Region { start: start - 1, end: end - 1 }));
    }
    if enzymes.is_empty() {
        return Ok(None);
    }
    let [a, b] = enzymes.as_slice() else {
        return fail("give two enzymes to open the backbone between");
    };
    let arc = match args.opt_str("arc")?.as_deref() {
        None | Some("between") => OpeningArc::Between,
        Some("origin") => OpeningArc::Origin,
        Some(other) => return fail(format!("arc is \"between\" or \"origin\", not \"{other}\"")),
    };
    Ok(Some(Opening::Enzymes { a: a.clone(), b: b.clone(), arc }))
}

// ── THE FOLDER'S RECORD ─────────────────────────────────────────────────────────────────────

struct BuildFolder {
    name: String,
    meta: FolderMeta,
}

impl BuildFolder {
    fn backbone_id(&self) -> Option<ConstructId> {
        self.meta.get(BACKBONE).as_str().and_then(ConstructId::parse)
    }

    fn opening(&self) -> Option<Opening> {
        Opening::from_json(self.meta.get(OPENING_KEY))
    }

    /// The rows, oldest first, as they are stored.
    fn designs(&self) -> Vec<Map<String, Value>> {
        self.meta.get(DESIGNS).as_array().map(|rows| rows.iter().filter_map(|r| r.as_object().cloned()).collect()).unwrap_or_default()
    }
}

fn build_folder(session: &Session, name: &str) -> Result<BuildFolder, Failure> {
    let meta = session.project().folder_meta(name)?.filter(|m| m.kind == KIND);
    match meta {
        Some(meta) => Ok(BuildFolder { name: name.to_string(), meta }),
        None => fail(format!("\"{name}\" is not a build folder. list_build_folders says which are")),
    }
}

fn build_folders(session: &Session) -> Result<Vec<BuildFolder>, Failure> {
    Ok(session
        .project()
        .folder_metas()?
        .into_iter()
        .filter(|(_, meta)| meta.kind == KIND)
        .map(|(path, meta)| BuildFolder { name: refs::label(&path.with_extension("")), meta })
        .collect())
}

/// The pieces kept behind one design, as the order table reads them.
fn payload_pieces(session: &Session, folder: &str, design: &str) -> Vec<(String, String)> {
    let file = item_file_name(DESIGNS_DIR, design, "design");
    let Ok(Some(text)) = session.project().folder_file(folder, &file) else {
        return Vec::new();
    };
    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    payload
        .get("eblocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| {
                    Some((b.get("name").and_then(Value::as_str).unwrap_or_default().to_string(), b.get("order").and_then(Value::as_str)?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What one design's payload says besides its pieces: its warnings, and what the cut clipped.
fn payload_notes(session: &Session, folder: &str, design: &str) -> (Vec<String>, Vec<String>) {
    let file = item_file_name(DESIGNS_DIR, design, "design");
    let Ok(Some(text)) = session.project().folder_file(folder, &file) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
        return (Vec::new(), Vec::new());
    };
    let list = |at: &Value| at.as_array().map(|xs| xs.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    (list(payload.get("warnings").unwrap_or(&Value::Null)), list(payload.get("carry").and_then(|c| c.get("truncated")).unwrap_or(&Value::Null)))
}

fn order_of(session: &Session, folder: &BuildFolder) -> bios_core::build::FolderOrder {
    let names: Vec<String> = folder.designs().iter().filter_map(|d| d.get("name").and_then(Value::as_str).map(str::to_string)).collect();
    let pieces: Vec<Vec<(String, String)>> = names.iter().map(|name| payload_pieces(session, &folder.name, name)).collect();
    let designs: Vec<DesignPieces> =
        names.iter().zip(&pieces).map(|(name, pieces)| DesignPieces { design: name.as_str(), pieces: pieces.as_slice() }).collect();
    order_across(&designs)
}

// ── READING ─────────────────────────────────────────────────────────────────────────────────

fn folder_line(session: &Session, folder: &BuildFolder) -> Value {
    let listed = session.project().list().unwrap_or_default();
    let backbone = folder.backbone_id().and_then(|id| listed.iter().find(|l| l.id == id).cloned());
    json!({
        "folder": folder.name,
        "backbone": backbone.as_ref().map(|l| refs::label(&l.path)),
        "backbone_length": backbone.as_ref().map(|l| l.length),
        "opening": folder.opening().map_or_else(|| "cut not recorded".to_string(), |o| o.says()),
        "designs": folder.designs().len(),
    })
}

fn list_build_folders(session: &Session, _args: &Args) -> Result<Value, Failure> {
    let folders = build_folders(session)?;
    Ok(json!({ "build_folders": folders.iter().map(|f| folder_line(session, f)).collect::<Vec<_>>() }))
}

/// Is this design still describing the backbone the folder holds NOW?
///
/// The folder's copy is frozen, so this should never fire — it is here so a mismatch is
/// DETECTABLE rather than merely impossible. It fires for real after the backbone is changed on
/// purpose, which leaves the older designs readable and marked rather than deleting them.
fn design_row(session: &Session, folder: &BuildFolder, row: &Map<String, Value>, backbone: Option<&Listed>) -> Value {
    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
    let stale = backbone.is_some_and(|b| row.get("backboneLength").and_then(Value::as_u64).is_some_and(|n| n as usize != b.length));
    let other_cut = row.get(OPENING_KEY).map(Opening::from_json).unwrap_or(None) != folder.opening();
    let (warnings, clipped) = payload_notes(session, &folder.name, name);
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    out.insert("built".into(), row.get("addedAt").cloned().unwrap_or(Value::Null));
    out.insert("plasmid".into(), row.get("constructId").cloned().unwrap_or(Value::Null));
    out.insert("length".into(), row.get("bp").cloned().unwrap_or(Value::Null));
    out.insert("pieces".into(), row.get("fragments").cloned().unwrap_or(Value::Null));
    out.insert("recipe".into(), json!(order_of(session, folder).recipe(name).unwrap_or_default()));
    out.insert("verdict".into(), row.get("verdict").cloned().unwrap_or(Value::Null));
    out.insert("warnings".into(), json!(warnings));
    out.insert("cut_clips".into(), json!(clipped));
    if stale {
        out.insert("note".into(), json!("built against an earlier backbone"));
    } else if other_cut {
        out.insert("note".into(), json!("built with an earlier cut"));
    }
    Value::Object(out)
}

fn read_build_folder(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = build_folder(session, &args.str("folder")?)?;
    let listed = session.project().list()?;
    let backbone = folder.backbone_id().and_then(|id| listed.iter().find(|l| l.id == id).cloned());
    let order = order_of(session, &folder);
    // WHAT THE CUT CLIPS IS A FACT ABOUT THE CUT, not about any one design: the folder is one
    // backbone opened one way, so it is said once, here, rather than counted onto every row.
    let mut clipped: Vec<String> = Vec::new();
    for row in folder.designs() {
        let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
        for one in payload_notes(session, &folder.name, name).1 {
            if !clipped.contains(&one) {
                clipped.push(one);
            }
        }
    }
    // How much leaves, and how much stays: the two bands of the gel this folder's digest gives.
    let leaves = match (&backbone, folder.opening()) {
        (Some(b), Some(opening)) => {
            let construct = session.project().read(&b.id)?.construct;
            let cut = opening.as_cut(&construct)?;
            let mut warnings = Vec::new();
            bios_core::gibson::resolve_gibson_opening(construct.len() as i64, construct.circular, &cut, &mut warnings, bios_core::gibson::SITE_MARGIN)
                .ok()
                .map(|o| json!({ "comes_out": o.removed_len, "stays": construct.len() as i64 - o.removed_len }))
        }
        _ => None,
    };
    let rows: Vec<Value> = folder.designs().iter().rev().map(|row| design_row(session, &folder, row, backbone.as_ref())).collect();
    Ok(json!({
        "folder": folder.name,
        "backbone": backbone.as_ref().map(|l| refs::label(&l.path)),
        "backbone_length": backbone.as_ref().map(|l| l.length),
        "opening": folder.opening().map_or_else(|| "cut not recorded".to_string(), |o| o.says()),
        "digest": leaves,
        "cut_clips": clipped,
        "designs": rows,
        "to_order": order_table(&order.pieces),
    }))
}

fn order_table(pieces: &[OrderPiece]) -> Vec<Value> {
    pieces.iter().map(|p| json!({ "n": p.n, "name": p.name, "length": p.length, "sequence": p.order })).collect()
}

fn list_designs(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = build_folder(session, &args.str("folder")?)?;
    let listed = session.project().list()?;
    let backbone = folder.backbone_id().and_then(|id| listed.iter().find(|l| l.id == id).cloned());
    let rows: Vec<Value> = folder.designs().iter().rev().map(|row| design_row(session, &folder, row, backbone.as_ref())).collect();
    Ok(json!({ "folder": folder.name, "designs": rows }))
}

fn build_order_table(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = build_folder(session, &args.str("folder")?)?;
    let order = order_of(session, &folder);
    let mut out = Map::new();
    out.insert("folder".into(), json!(folder.name));
    out.insert("pieces".into(), json!(order_table(&order.pieces)));
    out.insert("recipes".into(), json!(order.recipes.iter().map(|(name, nums)| json!({ "design": name, "pieces": nums })).collect::<Vec<_>>()));
    out.insert("text".into(), json!(order_tsv(&order.pieces)));
    if let Some(file) = args.opt_str("file")? {
        let bytes = order_xlsx(&order.pieces, "To order");
        let path = std::path::PathBuf::from(&file);
        let overwrite = args.opt_bool("overwrite")? == Some(true);
        // The workbook is bytes, and `write_outside` writes text; the same rules, one step down.
        write_bytes_outside(&path, &bytes, overwrite)?;
        out.insert("written".into(), json!(file));
    }
    Ok(Value::Object(out))
}

fn write_bytes_outside(path: &std::path::Path, bytes: &[u8], overwrite: bool) -> Result<(), Failure> {
    if path.exists() && !overwrite {
        return fail(format!("{} is already there; pass overwrite to replace it", path.display()));
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Failure(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| Failure(format!("{}: {e}", path.display())))
}

// ── DESIGNING ───────────────────────────────────────────────────────────────────────────────

/// One design per insert. Each insert is its own plasmid, so each gets its own call with a list of
/// one — the multi path is unchanged and still joins what it is given, it is simply given one
/// thing. A fragment that cannot be designed does not take the others down: its own row carries
/// the reason and the rest are still buildable.
fn design_each(
    construct: &Construct,
    opening: &Opening,
    inserts: &[GibsonFragment],
    target_tm: Option<i64>,
    max_piece: Option<i64>,
) -> Result<Vec<Result<DesignGibsonMultiResult, Failure>>, Failure> {
    let cut = opening.as_cut(construct)?;
    Ok(inserts.iter().map(|insert| gibson::design_with(construct, cut.clone(), vec![insert.clone()], target_tm, max_piece)).collect())
}

/// The one-line verdict beside a design. It says what you have to DO, not how it went — nothing
/// has happened yet — which is why the piece count is the sentence's subject: it is the only
/// number here that costs money.
fn verdict(design: &DesignGibsonMultiResult) -> String {
    let pieces = design.eblocks.len();
    let mut parts = vec![format!("{pieces} {} to order", if pieces == 1 { "piece" } else { "pieces" })];
    let truncated = design.assembled.carry.truncated.len();
    if truncated > 0 {
        parts.push(format!("{truncated} clipped"));
    }
    if !design.warnings.is_empty() {
        parts.push(format!("{} to check", design.warnings.len()));
    }
    parts.join(" · ")
}

fn design_report(name: &str, design: &DesignGibsonMultiResult) -> Value {
    json!({
        "name": name,
        "length": design.assembled.sequence.len(),
        "verdict": verdict(design),
        // WHERE THE BACKBONE OPENS AND WHAT THE PIECES SPAN. 0-based and in the frame the pieces
        // are laid out in — `left arm + inserts + right arm` — because that is what a picture of
        // the design is drawn from: the band over the opening, a run per piece, a seam per
        // junction. Without it a caller can say what a design costs and not what it looks like.
        "opening": { "up": design.cut.up, "down": design.cut.down, "removed_length": design.cut.removed_len },
        "arms": { "left": design.arms.left, "right": design.arms.right },
        "order": design.eblocks.iter().map(|e| json!({
            "name": e.name, "length": e.length, "gc": (e.gc * 1000.0).round() / 1000.0, "sequence": e.order,
            "start": e.start, "end": e.end,
        })).collect::<Vec<_>>(),
        "overlaps": design.junctions.iter().map(|j| json!({
            "between": format!("{} and {}", j.left, j.right),
            "kind": match j.kind {
                JunctionKind::BackboneInsert => "plasmid to insert",
                JunctionKind::InsertInsert => "insert to insert",
                JunctionKind::InsertBackbone => "insert to plasmid",
            },
            "sequence": j.seq, "tm": (j.tm * 10.0).round() / 10.0, "unique": j.unique,
        })).collect::<Vec<_>>(),
        "cut_clips": design.assembled.carry.truncated.iter().map(|t| t.before.name.clone()).collect::<Vec<_>>(),
        "warnings": design.warnings,
    })
}

/// The product itself, for a caller that is going to DRAW it rather than order it.
///
/// Asked for by name, because it is the one part of a design that is as big as a plasmid: a
/// hundred inserts is a hundred products, and a window that wants to show one wants exactly one.
fn product_report(design: &DesignGibsonMultiResult) -> Value {
    json!({
        "sequence": design.assembled.sequence,
        "circular": design.assembled.circular,
        "features": design.assembled.features.iter().map(|f| json!({
            "name": f.name,
            "type": f.kind,
            "start": f.start + 1,
            "end": f.end + 1,
            "strand": if f.strand == bios_core::Strand::Reverse { "reverse" } else { "forward" },
        })).collect::<Vec<_>>(),
    })
}

/// What each product is called.
///
/// ONE NAME PER INSERT, derived from the insert, because with five plasmids there is no single
/// thing one name could name. An unnamed insert is numbered by its place in the list, so a hundred
/// pasted sequences become a hundred distinct plasmids without anyone typing a hundred names.
fn product_names(backbone: &str, inserts: &[GibsonFragment], given: &[String]) -> Vec<String> {
    let stem = backbone.trim_end_matches(".gb");
    let width = inserts.len().to_string().len();
    inserts
        .iter()
        .enumerate()
        .map(|(i, insert)| {
            if let Some(name) = given.get(i).map(|n| n.trim()).filter(|n| !n.is_empty()) {
                return name.to_string();
            }
            let part: String = insert.name.split_whitespace().collect();
            if part.is_empty() || part.starts_with("Insert") { format!("{stem}_{:0width$}", i + 1) } else { format!("{stem}_{part}") }
        })
        .collect()
}

/// A product name that is not already a design in this folder.
///
/// Building the same insert twice is an ordinary thing to do — you changed the opening and want
/// both — so the second one is numbered rather than overwriting the first. The name is the key of
/// the row AND of the file behind it.
fn free_design_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    for n in 2..=999 {
        let candidate = format!("{base} ({n})");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base} {}", chrono::Utc::now().timestamp_millis())
}

fn design_builds(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let Some(opening) = opening_from(args)? else {
        return fail("say where the backbone opens: two enzymes, or replace_start with replace_end");
    };
    let inserts = gibson::fragments(args)?;
    let names = product_names(&refs::label(&loaded.path), &inserts, &args.strings("names"));
    let designed = design_each(&loaded.construct, &opening, &inserts, args.opt_int("target_tm")?, args.opt_int("max_piece_length")?)?;
    let with_product = args.opt_bool("with_product")? == Some(true);
    let rows: Vec<Value> = designed
        .iter()
        .zip(&names)
        .map(|(design, name)| match design {
            Ok(design) => {
                let mut row = design_report(name, design);
                if let (true, Some(map)) = (with_product, row.as_object_mut()) {
                    map.insert("product".into(), product_report(design));
                }
                row
            }
            Err(why) => json!({ "name": name, "could_not_design": why.to_string() }),
        })
        .collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "opening": opening.says(), "designs": rows }))
}

// ── BUILDING ────────────────────────────────────────────────────────────────────────────────

/// Where these products go: the same folder as last time when its backbone holds these bases, and
/// otherwise a free name. Never a folder somebody made by hand.
fn choose_folder(session: &Session, backbone: &Listed, bases: &str) -> Result<(String, Option<ConstructId>), Failure> {
    let folders: Vec<String> = session.project().folders()?.iter().map(|f| refs::label(f)).collect();
    let listed = session.project().list()?;
    let bases = bases.to_uppercase();
    let pick = pick_folder(
        &folder_name_for(&backbone.name, KIND),
        &folder_stamp(),
        &folders,
        |name| session.project().folder_meta(name).ok().flatten(),
        |meta| meta.kind == KIND,
        |meta| {
            // The test is on the BASES, never on the id or the name: a plasmid keeps its id
            // through an edit, and it is the edit that makes the older designs incomparable.
            let held = meta.get(BACKBONE).as_str().and_then(ConstructId::parse)?;
            let listed = listed.iter().find(|l| l.id == held)?;
            let construct = session.project().read(&listed.id).ok()?;
            (construct.construct.sequence.to_uppercase() == bases).then(|| held.to_string())
        },
    );
    Ok((pick.folder, pick.reuse.as_deref().and_then(ConstructId::parse)))
}

/// The folder this build goes into, made and stamped if it is new.
///
/// The copy exists before the folder is typed, so a folder is never stamped `build` with nothing
/// to build into, and the type is set before any design is written.
fn prepare_folder(session: &Session, backbone: &Listed, opening: &Opening, asked: Option<String>) -> Result<(String, ConstructId), Failure> {
    let bases = session.project().read(&backbone.id)?.construct.sequence.clone();
    let (folder, reuse) = match asked {
        Some(folder) => {
            let existing = build_folder(session, &folder)?;
            let Some(id) = existing.backbone_id() else {
                return fail(format!("\"{folder}\" has no backbone to build into"));
            };
            (folder, Some(id))
        }
        None => choose_folder(session, backbone, &bases)?,
    };
    let backbone_id = match reuse {
        // Same bases, same folder. The cut is re-stamped rather than left alone, because building
        // again IS how a folder's cut is changed.
        Some(id) => id,
        None => {
            // Named after the original, not `<name> copy`: the folder is `pUC19` and the backbone
            // inside it should be `pUC19` too. Different folders, so nothing collides. It is made
            // straight into the folder rather than copied and moved, so a half-made folder cannot
            // be left behind by a failure between the two.
            let mut copy = session.project().read(&backbone.id)?.construct;
            copy.name = backbone.name.clone();
            let change = bios_project::Change {
                command: "build_plasmids".into(),
                params: json!({ "folder": folder }),
                summary: format!("Copied into {folder} as the backbone every design there is built into"),
            };
            session.project().create_construct(copy, &folder, Some(&backbone.id), session.who(), change)?.id
        }
    };
    let mut meta = session.project().folder_meta(&folder)?.filter(|m| m.kind == KIND).unwrap_or_else(|| FolderMeta::new(KIND, Map::new()));
    meta.set(BACKBONE, json!(backbone_id.as_str()));
    meta.set(OPENING_KEY, opening.to_json());
    if meta.get(DESIGNS).is_null() {
        meta.set(DESIGNS, json!([]));
    }
    session.project().write_folder_meta(&folder, &meta)?;
    Ok((folder, backbone_id))
}

/// One design, kept: the product as a plasmid, its row on the folder, and its pieces beside it.
fn keep_design(
    session: &Session,
    folder: &str,
    backbone: &bios_project::Loaded,
    name: &str,
    design: &DesignGibsonMultiResult,
    opening: &Opening,
) -> Result<Value, Failure> {
    let construct = Construct {
        name: name.to_string(),
        circular: design.assembled.circular,
        sequence: design.assembled.sequence.clone(),
        features: design.assembled.features.clone(),
        locus: bios_core::model::Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    };
    let summary = format!("Built into {} ({} bp)", backbone.construct.name, construct.len());
    let change = bios_project::Change { command: "build_plasmids".into(), params: json!({ "folder": folder }), summary };
    let made = session.project().create_construct(construct, folder, Some(&backbone.id), session.who(), change)?;

    // The row the folder lists: small on purpose, so a folder draws without reading a payload.
    let row = json!({
        "name": name,
        "addedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "constructId": made.id.as_str(),
        "parts": design.eblocks.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
        "bp": design.assembled.sequence.len(),
        "fragments": design.eblocks.len(),
        "warnings": design.warnings.len() + design.assembled.carry.truncated.len(),
        "verdict": verdict(design),
        "backboneName": backbone.construct.name,
        "backboneLength": backbone.construct.len(),
        "opening": opening.to_json(),
    });
    // The pieces: the one part of a design that cannot be recovered from what is on disk.
    let payload = json!({
        "v": 1,
        "eblocks": design.eblocks.iter().map(|e| json!({ "name": e.name, "order": e.order, "length": e.length, "gc": e.gc })).collect::<Vec<_>>(),
        "junctions": design.junctions.iter().map(|j| json!({ "kind": format!("{:?}", j.kind), "seq": j.seq, "tm": j.tm, "unique": j.unique })).collect::<Vec<_>>(),
        "arms": { "left": design.arms.left, "right": design.arms.right },
        "warnings": design.warnings,
        "carry": {
            "carried": design.assembled.carry.carried.len(),
            "truncated": design.assembled.carry.truncated.iter().map(|t| t.before.name.clone()).collect::<Vec<_>>(),
            "removed": design.assembled.carry.removed.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        },
    });
    let file = item_file_name(DESIGNS_DIR, name, "design");
    let text = serde_json::to_string(&payload).map_err(|e| Failure(e.to_string()))?;
    session.project().write_folder_file(folder, &file, &text)?;
    Ok(row)
}

fn build_plasmids(session: &Session, args: &Args) -> Result<Value, Failure> {
    let Some(opening) = opening_from(args)? else {
        return fail("say where the backbone opens: two enzymes, or replace_start with replace_end");
    };
    let inserts = gibson::fragments(args)?;
    let listed = session.project().list()?;
    let backbone_row = refs::plasmid(&listed, &args.str("plasmid")?)?.clone();
    // The cut is resolved against the plasmid BEFORE anything is made, so a cut that cannot open
    // it leaves no folder and no copy behind.
    opening.as_cut(&session.project().read(&backbone_row.id)?.construct)?;
    let (folder, backbone_id) = prepare_folder(session, &backbone_row, &opening, args.opt_str("folder")?)?;
    let backbone = session.project().read(&backbone_id)?;

    let names = product_names(&backbone.construct.name, &inserts, &args.strings("names"));
    let designed = design_each(&backbone.construct, &opening, &inserts, args.opt_int("target_tm")?, args.opt_int("max_piece_length")?)?;

    let mut meta = build_folder(session, &folder)?.meta;
    let mut rows: Vec<Value> = meta.get(DESIGNS).as_array().cloned().unwrap_or_default();
    let mut taken: Vec<String> = rows.iter().filter_map(|r| r.get("name").and_then(Value::as_str).map(str::to_string)).collect();
    let mut built = Vec::new();
    let mut refused = Vec::new();
    for (design, wanted) in designed.iter().zip(&names) {
        match design {
            Ok(design) => {
                let name = free_design_name(wanted, &taken);
                // KEPT AS IT LANDS, not at the end: a build that fails on the fourth of five
                // leaves three plasmids filed, with their rows and their pieces.
                let row = keep_design(session, &folder, &backbone, &name, design, &opening)?;
                taken.push(name.clone());
                rows.push(row);
                meta.set(DESIGNS, Value::Array(rows.clone()));
                session.project().write_folder_meta(&folder, &meta)?;
                built.push(json!({ "name": name, "length": design.assembled.sequence.len(), "verdict": verdict(design) }));
            }
            Err(why) => refused.push(json!({ "name": wanted, "could_not_design": why.to_string() })),
        }
    }
    Ok(json!({
        "folder": folder,
        "backbone": refs::label(&backbone.path),
        "opening": opening.says(),
        "built": built,
        "not_built": refused,
    }))
}

fn set_build_opening(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = build_folder(session, &args.str("folder")?)?;
    let Some(opening) = opening_from(args)? else {
        return fail("say where the backbone opens: two enzymes, or replace_start with replace_end");
    };
    let Some(id) = folder.backbone_id() else {
        return fail(format!("\"{}\" has no backbone to open", folder.name));
    };
    // Checked against the folder's own copy, so a cut that cannot open it is refused here rather
    // than at the next build.
    let backbone = session.project().read(&id)?;
    opening.as_cut(&backbone.construct)?;
    let mut meta = folder.meta;
    meta.set(OPENING_KEY, opening.to_json());
    session.project().write_folder_meta(&folder.name, &meta)?;
    Ok(json!({ "folder": folder.name, "opening": opening.says() }))
}

fn drop_design(session: &Session, args: &Args) -> Result<Value, Failure> {
    let folder = build_folder(session, &args.str("folder")?)?;
    let name = args.str("name")?;
    let rows = folder.designs();
    if !rows.iter().any(|r| r.get("name").and_then(Value::as_str) == Some(name.as_str())) {
        return fail(format!("\"{}\" has no design called \"{name}\"", folder.name));
    }
    let kept: Vec<Value> = rows.into_iter().filter(|r| r.get("name").and_then(Value::as_str) != Some(name.as_str())).map(Value::Object).collect();
    let mut meta = folder.meta;
    meta.set(DESIGNS, Value::Array(kept));
    session.project().write_folder_meta(&folder.name, &meta)?;
    session.project().remove_folder_file(&folder.name, &item_file_name(DESIGNS_DIR, &name, "design"))?;
    // DROPPING A ROW IS NOT DELETING THE PLASMID: the product is an ordinary construct in this
    // folder and stays one. This forgets how it was made.
    Ok(json!({ "folder": folder.name, "dropped": name, "plasmid_kept": true }))
}
