//! Finding constructs, and bringing them into the project.

use std::path::Path;

use bios_project::Listed;
use serde_json::{Value, json};

use super::PLASMID;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, fail, refs, token};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "list_plasmids",
        description: "List the plasmids in this project (with folders), by path. Use the path to address a plasmid in every other command. Also lists what each plasmid was made from.",
        params: &[Param {
            name: "folder",
            kind: Kind::String,
            required: false,
            description: "Only plasmids in this folder (and the folders inside it).",
        }],
        effect: Effect::Read,
        run: Run::Call(list_plasmids),
    },
    CommandSpec {
        name: "find_plasmid",
        description: "Find plasmids by (part of) their name or path.",
        params: &[Param { name: "query", kind: Kind::String, required: true, description: "Part of a name, a path or an id." }],
        effect: Effect::Read,
        run: Run::Call(find_plasmid),
    },
    CommandSpec {
        name: "search_project",
        description: "Search the whole project at once: plasmid names, feature names and qualifiers, and, when the query is eight or more bases, the sequences themselves on both strands. Use it to find what has already been made before making it again.",
        params: &[Param { name: "query", kind: Kind::String, required: true, description: "Part of a name, or bases." }],
        effect: Effect::Read,
        run: Run::Call(search_project),
    },
    CommandSpec {
        name: "import_plasmid",
        description: "Bring a GenBank or FASTA file from outside the project in as a copy. The file itself is only read, never changed. Importing a file that is already here unchanged gives back the plasmid that is here, so there is never a second copy. To work on files that already sit in a folder (iCloud, Google Drive, OneDrive, Dropbox, anywhere), do not import them: open that folder with open_project, or pass its path as `project`, and Bios works on them where they are.",
        params: &[
            Param { name: "file", kind: Kind::String, required: true, description: "Full path of the GenBank or FASTA file on this computer." },
            Param {
                name: "folder",
                kind: Kind::String,
                required: false,
                description: "Folder inside the project to put it in (created if missing). Default: the top of the project.",
            },
            Param { name: "name", kind: Kind::String, required: false, description: "Name to give it here. Default: the name in the file." },
        ],
        effect: Effect::Write,
        run: Run::Call(import_plasmid),
    },
    CommandSpec {
        name: "duplicate_plasmid",
        description: "Make a new plasmid from an existing one, in the same folder. The new plasmid remembers what it was made from. Use this to start a variant; never copy files by hand.",
        params: &[PLASMID, Param { name: "name", kind: Kind::String, required: true, description: "Name of the new plasmid." }],
        effect: Effect::Write,
        run: Run::Call(duplicate_plasmid),
    },
];

fn row(listed: &[Listed], l: &Listed) -> Value {
    let folder = l.path.parent().filter(|p| !p.as_os_str().is_empty()).map(refs::label);
    let made_from =
        l.derived_from.as_ref().map(|parent| listed.iter().find(|p| &p.id == parent).map_or_else(|| parent.to_string(), |p| refs::label(&p.path)));
    json!({
        "path": refs::label(&l.path),
        "name": l.name,
        "folder": folder,
        "length": l.length,
        "circular": l.circular,
        "features": l.features,
        "id": l.id.as_str(),
        "made_from": made_from,
    })
}

fn list_plasmids(session: &Session, args: &Args) -> Result<Value, Failure> {
    let listed = session.project().list()?;
    let folder = args.opt_str("folder")?;
    let inside = |l: &&Listed| folder.as_deref().is_none_or(|f| l.path.starts_with(Path::new(f.trim_matches('/'))));
    let plasmids: Vec<Value> = listed.iter().filter(inside).map(|l| row(&listed, l)).collect();
    let stray: Vec<Value> =
        session.project().untracked()?.into_iter().map(|(path, reason)| json!({ "file": path.display().to_string(), "reason": reason })).collect();
    let folders: Vec<String> = session.project().folders()?.iter().map(|f| refs::label(f)).collect();
    let mut out = json!({ "project": session.project().name(), "folders": folders, "plasmids": plasmids });
    if let (false, Some(map)) = (stray.is_empty(), out.as_object_mut()) {
        map.insert("files_not_in_project".into(), json!(stray));
    }
    Ok(out)
}

fn find_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let query = args.str("query")?.to_lowercase();
    let listed = session.project().list()?;
    let hits: Vec<Value> = listed
        .iter()
        .filter(|l| {
            refs::label(&l.path).to_lowercase().contains(&query) || l.name.to_lowercase().contains(&query) || l.id.as_str().starts_with(&query)
        })
        .take(50)
        .map(|l| row(&listed, l))
        .collect();
    Ok(json!(hits))
}

/// Most rows of one kind in a search reply. The totals are always given.
const MAX_FOUND: usize = 50;

fn search_project(session: &Session, args: &Args) -> Result<Value, Failure> {
    let query = args.str("query")?.trim().to_string();
    if query.is_empty() {
        return fail("query is empty");
    }
    let lower = query.to_lowercase();
    let as_bases = bios_core::seq::clean_seq(&query);
    let look_in_sequence =
        as_bases.len() >= 8 && query.chars().all(|c| c.is_ascii_alphabetic() || c.is_whitespace()) && as_bases.bytes().all(|b| b"ACGT".contains(&b));

    let (listed, constructs): (Vec<_>, Vec<_>) = session.project().list_with_constructs()?.into_iter().unzip();
    let (mut plasmids, mut features, mut sequences) = (Vec::new(), Vec::new(), Vec::new());
    for (l, construct) in listed.iter().zip(&constructs) {
        let path = refs::label(&l.path);
        if path.to_lowercase().contains(&lower) {
            plasmids.push(row(&listed, l));
        }
        for f in &construct.features {
            let in_notes = f.notes.iter().any(|(_, values)| values.iter().any(|v| !v.starts_with("bios-id:") && v.to_lowercase().contains(&lower)));
            if f.name.to_lowercase().contains(&lower) || in_notes {
                features.push(json!({ "plasmid": path, "feature": f.name, "type": f.kind, "start": f.start + 1, "end": f.end + 1 }));
            }
        }
        if look_in_sequence && as_bases.len() <= construct.len() {
            let options = bios_core::search::SearchOptions { max_mismatches: 0, circular: construct.circular };
            for hit in bios_core::search::find_binding_sites(&construct.sequence, &as_bases, options) {
                sequences.push(
                    json!({ "plasmid": path, "start": hit.start + 1, "end": hit.end + 1, "strand": if hit.forward { "forward" } else { "reverse" } }),
                );
            }
        }
    }
    let capped = |rows: Vec<Value>| -> Value { json!({ "total": rows.len(), "rows": rows.into_iter().take(MAX_FOUND).collect::<Vec<_>>() }) };
    Ok(json!({
        "query": query,
        "plasmids": capped(plasmids),
        "features": capped(features),
        "sequence": if look_in_sequence { capped(sequences) } else { Value::Null },
    }))
}

fn import_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let file = args.str("file")?;
    let path = Path::new(&file);
    if !path.is_absolute() {
        return fail(format!("{file} is not a full path; give the file's absolute path"));
    }
    let imported = session.project().import(path, &args.opt_str("folder")?.unwrap_or_default(), args.opt_str("name")?.as_deref(), session.who())?;
    let loaded = imported.construct;
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "id": loaded.id.as_str(),
        "version": token(&loaded),
        "length": loaded.construct.len(),
        "already_here": imported.already_here,
        "notes": imported.notices,
    }))
}

fn duplicate_plasmid(session: &Session, args: &Args) -> Result<Value, Failure> {
    let source = session.load(args)?;
    let made = session.project().duplicate(&source.id, &args.str("name")?, session.who())?;
    Ok(json!({ "plasmid": refs::label(&made.path), "id": made.id.as_str(), "version": token(&made), "made_from": refs::label(&source.path) }))
}
