//! Making, moving, deleting and exporting constructs, and editing one as text.

use std::path::{Path, PathBuf};

use bios_core::model::Locus;
use bios_core::{Construct, Segment, genbank, genbank_write};
use bios_project::{Change, ConstructId, write_outside};
use serde_json::{Value, json};

use super::sequence::{bases, in_range, slice, span_len};
use super::{PLASMID, VERSION};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs, token};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "create_plasmid",
        description: "Make a new plasmid in this project. Give GenBank text, or bases/FASTA (+ circular), or a range of another plasmid (from_plasmid, from_start, from_end): the features fully inside the range come along and the new plasmid remembers what it was made from.",
        params: &[
            Param { name: "name", kind: Kind::String, required: true, description: "The new plasmid's name." },
            Param {
                name: "folder",
                kind: Kind::String,
                required: false,
                description: "Folder inside the project (made if missing). Default: the top of the project.",
            },
            Param { name: "genbank", kind: Kind::String, required: false, description: "GenBank text (LOCUS ... ORIGIN ... //)." },
            Param { name: "sequence", kind: Kind::String, required: false, description: "Bases, or FASTA." },
            Param {
                name: "circular",
                kind: Kind::Boolean,
                required: false,
                description: "With sequence: circular (default) or linear. With a range: default linear.",
            },
            Param { name: "from_plasmid", kind: Kind::String, required: false, description: "Take a range of this plasmid." },
            Param { name: "from_start", kind: Kind::Integer, required: false, description: "First base of the range." },
            Param {
                name: "from_end",
                kind: Kind::Integer,
                required: false,
                description: "Last base of the range (start > end runs through the origin).",
            },
        ],
        effect: Effect::Write,
        run: Run::Call(create_plasmid),
    },
    CommandSpec {
        name: "move_plasmid",
        description: "Move a plasmid to another folder of the project (\"\" = the top). Its history goes with it.",
        params: &[PLASMID, Param { name: "folder", kind: Kind::String, required: true, description: "The folder to move it to (made if missing)." }],
        effect: Effect::Write,
        run: Run::Call(move_plasmid),
    },
    CommandSpec {
        name: "delete_plasmid",
        description: "Delete a plasmid from the project. ALWAYS ask the person first and only call with confirm:true after they say yes. A deleted plasmid is kept with its history and can be brought back with restore_plasmid.",
        params: &[PLASMID, Param { name: "confirm", kind: Kind::Boolean, required: true, description: "Must be true, after the person said yes." }],
        effect: Effect::Write,
        run: Run::Call(delete_plasmid),
    },
    CommandSpec {
        name: "deleted_plasmids",
        description: "List the plasmids that were deleted and can be brought back.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(deleted_plasmids),
    },
    CommandSpec {
        name: "restore_plasmid",
        description: "Bring a deleted plasmid back to where it was, with its history.",
        params: &[Param {
            name: "plasmid",
            kind: Kind::String,
            required: true,
            description: "The deleted plasmid's name or id, as deleted_plasmids shows it.",
        }],
        effect: Effect::Write,
        run: Run::Call(restore_plasmid),
    },
    CommandSpec {
        name: "export_plasmid",
        description: "Write a plasmid to a file outside the project (only when the person asks for a file). GenBank or FASTA. The plasmid in the project stays the one true copy.",
        params: &[
            PLASMID,
            Param { name: "path", kind: Kind::String, required: true, description: "Full path of the file to write (~ allowed)." },
            Param { name: "format", kind: Kind::String, required: false, description: "genbank (default) or fasta." },
            Param {
                name: "overwrite",
                kind: Kind::Boolean,
                required: false,
                description: "Replace the file if it exists. Default false: an existing file is never replaced.",
            },
        ],
        effect: Effect::Write,
        run: Run::Call(export_plasmid),
    },
    CommandSpec {
        name: "create_folder",
        description: "Make a folder in the project.",
        params: &[Param {
            name: "folder",
            kind: Kind::String,
            required: true,
            description: "The folder's path inside the project, such as \"backbones/2026\".",
        }],
        effect: Effect::Write,
        run: Run::Call(create_folder),
    },
    CommandSpec {
        name: "rename_folder",
        description: "Rename or move a folder inside the project; its plasmids go with it.",
        params: &[
            Param { name: "from", kind: Kind::String, required: true, description: "The folder as it is now." },
            Param { name: "to", kind: Kind::String, required: true, description: "What it should become." },
        ],
        effect: Effect::Write,
        run: Run::Call(rename_folder),
    },
    CommandSpec {
        name: "edit_plasmid",
        description: "Edit the GenBank text by exact string replacement, like editing a file: old_string must appear exactly once (or use replace_all). The result must still be valid GenBank. If it changes the sequence you must set allow_sequence_change; prefer insert_sequence / delete_range / replace_range for sequence edits, because they keep features on the right bases.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "old_string", kind: Kind::String, required: true, description: "The exact text to replace, as read_plasmid shows it." },
            Param { name: "new_string", kind: Kind::String, required: true, description: "The text to put in its place." },
            Param { name: "replace_all", kind: Kind::Boolean, required: false, description: "Replace every occurrence." },
            Param {
                name: "allow_sequence_change",
                kind: Kind::Boolean,
                required: false,
                description: "Set true if the edit is meant to change bases.",
            },
        ],
        effect: Effect::Write,
        run: Run::Edit(edit_plasmid),
    },
    CommandSpec {
        name: "write_plasmid",
        description: "Replace the whole GenBank text of a plasmid, after bigger rework. It must be valid GenBank; the plasmid keeps its name.",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "genbank", kind: Kind::String, required: true, description: "The new GenBank text." },
            Param {
                name: "summary",
                kind: Kind::String,
                required: false,
                description: "One line for the history saying what changed, such as \"Typed 12 bases after base 300\". Without it the history only says the text was replaced.",
            },
        ],
        effect: Effect::Write,
        run: Run::Edit(write_plasmid),
    },
];

pub(crate) fn created(
    session: &Session,
    command: &str,
    construct: Construct,
    folder: &str,
    made_from: Option<(&ConstructId, String)>,
    summary: String,
    args: &Args,
) -> Result<Value, Failure> {
    let change = Change { command: command.into(), params: args.raw().clone(), summary };
    let made = session.project().create_construct(construct, folder, made_from.as_ref().map(|(id, _)| *id), session.who(), change)?;
    Ok(json!({
        "created": refs::label(&made.path),
        "id": made.id.as_str(),
        "version": token(&made),
        "length": made.construct.len(),
        "made_from": made_from.map(|(_, label)| label),
    }))
}

fn create_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let name = args.str("name")?.trim().to_string();
    if name.is_empty() {
        return fail("name is empty");
    }
    let folder = args.opt_str("folder")?.unwrap_or_default();
    let fresh = |sequence: String, circular: bool| Construct {
        name: name.clone(),
        circular,
        sequence,
        locus: Locus { molecule: "DNA".into(), division: "SYN".into(), date: String::new() },
        ..Construct::default()
    };

    if let Some(text) = args.opt_str("genbank")? {
        let mut construct = genbank::read(&text).map_err(|e| Failure(format!("genbank must be GenBank text (LOCUS ... ORIGIN ... //): {e}")))?;
        construct.name = name.clone();
        return created(session, "create_plasmid", construct, &folder, None, format!("Made {name} from GenBank text"), args);
    }
    if let Some(sequence) = args.opt_str("sequence")? {
        let construct = fresh(bases(&sequence)?, args.opt_bool("circular")? != Some(false));
        let summary = format!("Made {name} from {} bases", construct.len());
        return created(session, "create_plasmid", construct, &folder, None, summary, args);
    }
    let Some(source) = args.opt_str("from_plasmid")? else {
        return fail("give genbank, sequence, or from_plasmid with from_start and from_end");
    };
    let listed = session.project().list()?;
    let source = session.project().read(&refs::plasmid(&listed, &source)?.id)?;
    let (start, end) = (args.int("from_start")?, args.int("from_end")?);
    let from = &source.construct;
    in_range(from, start, end)?;
    let (length, size) = (from.len(), span_len(from, start, end) as usize);
    let mut construct = fresh(slice(from, start, end), args.opt_bool("circular")? == Some(true));
    // A feature comes along when it lies whole, in one piece, inside the range.
    let offset = |x: usize| (x + length - (start - 1) as usize) % length;
    for f in from.features.iter().filter(|f| f.locations().is_none() && f.location_unsupported.is_none()) {
        let (a, b) = (offset(f.start), offset(f.end));
        if a <= b && b < size && f.span(length) == b - a + 1 {
            let mut moved = f.clone();
            moved.start = a;
            moved.end = b;
            moved.segments = vec![Segment::new(a, b)];
            construct.features.push(moved);
        }
    }
    let label = refs::label(&source.path);
    let summary = format!("Made {name} from {start}..{end} of {}", source.construct.name);
    created(session, "create_plasmid", construct, &folder, Some((&source.id, label)), summary, args)
}

fn move_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let moved = session.project().move_to(&loaded.id, args.str("folder")?.trim_matches('/'), session.who())?;
    Ok(json!({ "moved": refs::label(&loaded.path), "to": refs::label(&moved.path), "version": token(&moved) }))
}

fn delete_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    if args.opt_bool("confirm")? != Some(true) {
        return fail("not deleted: ask the person first, then call again with confirm:true");
    }
    let loaded = session.load(args)?;
    session.project().delete(&loaded.id, session.who())?;
    Ok(json!({ "deleted": refs::label(&loaded.path), "id": loaded.id.as_str(), "note": "It can be brought back with restore_plasmid." }))
}

fn deleted_plasmids(session: &Session, _: &Args) -> Result<Value, Failure> {
    let rows: Vec<Value> = session
        .project()
        .deleted()?
        .into_iter()
        .map(|d| json!({ "name": d.name, "was_at": refs::label(&d.path), "id": d.id.as_str(), "deleted_at": d.at }))
        .collect();
    Ok(json!(rows))
}

fn restore_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let wanted = args.str("plasmid")?;
    let wanted = wanted.trim();
    let deleted = session.project().deleted()?;
    let hits: Vec<_> = deleted
        .iter()
        .filter(|d| d.id.as_str() == wanted || d.name.eq_ignore_ascii_case(wanted) || refs::label(&d.path).eq_ignore_ascii_case(wanted))
        .collect();
    let one = match hits.as_slice() {
        [one] => one,
        [] => return fail(format!("no deleted plasmid called \"{wanted}\"; deleted_plasmids lists them")),
        many => return fail(format!("\"{wanted}\" matches {} deleted plasmids; use the id", many.len())),
    };
    let back = session.project().undelete(&one.id, session.who())?;
    Ok(json!({ "restored": refs::label(&back.path), "id": back.id.as_str(), "version": token(&back) }))
}

fn export_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let given = args.str("path")?;
    let path = match given.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)).unwrap_or_else(|| PathBuf::from(&given)),
        None => PathBuf::from(&given),
    };
    if !path.is_absolute() {
        return fail(format!("{given} is not a full path; give the file's absolute path"));
    }
    if path.starts_with(session.project().root()) {
        return fail("that path is inside the project; a plasmid lives in the project once. Use duplicate_plasmid for a variant");
    }
    let c = &loaded.construct;
    let text = match args.opt_str("format")?.as_deref() {
        None | Some("genbank") => genbank_write::write(c).map_err(|e| Failure(e.to_string()))?,
        Some("fasta") => {
            let lines: Vec<&str> = (0..c.len()).step_by(60).filter_map(|i| c.sequence.get(i..(i + 60).min(c.len()))).collect();
            format!(">{}\n{}\n", c.name, lines.join("\n"))
        }
        Some(other) => return fail(format!("format must be genbank or fasta, not {other}")),
    };
    write_outside(Path::new(&path), &text, args.opt_bool("overwrite")? == Some(true))?;
    Ok(json!({ "exported": refs::label(&loaded.path), "to": path.display().to_string(), "bytes": text.len() }))
}

fn create_folder(session: &Session, args: &Args) -> Result<Value, Failure> {
    let made = session.project().create_folder(args.str("folder")?.trim_matches('/'))?;
    Ok(json!({ "created": refs::label(&made) }))
}

fn rename_folder(session: &Session, args: &Args) -> Result<Value, Failure> {
    let from = args.str("from")?;
    let to = session.project().rename_folder(from.trim_matches('/'), args.str("to")?.trim_matches('/'))?;
    Ok(json!({ "renamed": from.trim_matches('/'), "to": refs::label(&to) }))
}

/// Replaces what a construct is with `text`, keeping what it is called: a plasmid's name is its
/// file's name, not what a LOCUS line happens to say.
fn replace_with(c: &mut Construct, text: &str) -> Result<(), Failure> {
    let mut parsed = genbank::read(text).map_err(|e| Failure(format!("the result would not be valid GenBank ({e}); nothing was saved")))?;
    parsed.name = std::mem::take(&mut c.name);
    *c = parsed;
    Ok(())
}

fn edit_plasmid(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let (old, new) = (args.str("old_string")?, args.str("new_string")?);
    if old.is_empty() {
        return fail("old_string is empty");
    }
    let text = genbank_write::write(c).map_err(|e| Failure(e.to_string()))?;
    let count = text.matches(&old).count();
    let all = args.opt_bool("replace_all")? == Some(true);
    match count {
        0 => return fail("old_string was not found in the GenBank text (read_plasmid shows the exact text)"),
        n if n > 1 && !all => return fail(format!("old_string appears {n} times; add more context to make it unique, or set replace_all")),
        _ => {}
    }
    let edited = if all { text.replace(&old, &new) } else { text.replacen(&old, &new, 1) };
    let before = c.sequence.clone();
    replace_with(c, &edited)?;
    if c.sequence != before && args.opt_bool("allow_sequence_change")? != Some(true) {
        return fail(
            "this edit changes the sequence; set allow_sequence_change if you mean it (or use insert_sequence / delete_range / replace_range, which move the features with it)",
        );
    }
    let replaced = if all { count } else { 1 };
    let mut result = serde_json::Map::new();
    result.insert("replaced".into(), json!(replaced));
    Ok(Applied {
        summary: format!("Edited the GenBank text ({replaced} replacement{})", if replaced == 1 { "" } else { "s" }),
        result,
        ..Applied::default()
    })
}

fn write_plasmid(_: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    replace_with(c, &args.str("genbank")?)?;
    let summary = args.opt_str("summary")?.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(Applied { summary: summary.unwrap_or_else(|| "Replaced the whole GenBank text".into()), ..Applied::default() })
}
