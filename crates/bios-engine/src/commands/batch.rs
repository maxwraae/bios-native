//! Several edits as one step, and taking the last step back.

use bios_core::Construct;
use serde_json::{Map, Value, json};

use super::{PLASMID, VERSION};
use crate::{Applied, Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, command, fail, refs, token};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "batch_edit",
        description: "Apply several edits to ONE plasmid in order and save them once (one version check, one save, one step in the history). All-or-nothing: if any operation fails, NOTHING is saved and the error names the operation. Each operation is {\"op\": <an editing command's name>, ...that command's parameters without plasmid and version}. COORDINATES: each operation sees the plasmid exactly as the previous operations left it. After insert_sequence of 10 bases at position 100, everything after base 100 is 10 higher for the next operation. To use coordinates of the plasmid as you read it, order sequence edits from the highest position to the lowest. Example: [{\"op\":\"delete_range\",\"start\":900,\"end\":950},{\"op\":\"insert_sequence\",\"position\":120,\"sequence\":\"GAATTC\"},{\"op\":\"add_feature\",\"name\":\"EcoRI site\",\"start\":121,\"end\":126}].",
        params: &[
            PLASMID,
            VERSION,
            Param { name: "operations", kind: Kind::Objects, required: true, description: "The edits, applied in this order." },
        ],
        effect: Effect::Write,
        run: Run::Edit(batch_edit),
    },
    CommandSpec {
        name: "undo_last_edit",
        description: "Take back the last change to a plasmid's sequence or features, whoever made it. The plasmid goes back to how it was just before; the undo is itself a step in the history, so it can be undone too.",
        params: &[PLASMID],
        effect: Effect::Write,
        run: Run::Call(undo_last_edit),
    },
];

fn batch_edit(session: &Session, args: &Args, c: &mut Construct) -> Result<Applied, Failure> {
    let operations = args.objects("operations");
    if operations.is_empty() {
        return fail("operations must be a non-empty list");
    }
    let (mut notes, mut results, mut summaries) = (Vec::new(), Vec::new(), Vec::new());
    for (i, operation) in operations.iter().enumerate() {
        let name = operation.get("op").and_then(Value::as_str).unwrap_or_default();
        let tag = format!("operation {} ({name})", i + 1);
        let nothing_saved = |why: String| Failure(format!("{tag} failed: {why}\nNothing was saved: a batch is all-or-nothing."));
        let Some((spec, change)) = command(name).filter(|s| s.name != "batch_edit").and_then(|s| s.edit_fn().map(|f| (s, f))) else {
            return Err(nothing_saved(format!("\"{name}\" is not an editing command")));
        };
        if let Some(key) = ["plasmid", "version"].iter().find(|k| operation.contains_key(**k)) {
            return Err(nothing_saved(format!("leave out \"{key}\"; the batch's plasmid and version apply to every operation")));
        }
        // The operation is checked as the command itself would check it, with the batch's own
        // plasmid and version standing in for the two it may not name.
        let mut params: Map<String, Value> = operation.clone();
        params.remove("op");
        params.insert("plasmid".into(), json!(args.str("plasmid")?));
        params.insert("version".into(), json!(args.str("version")?));
        let op_args = Args::check(spec, &Value::Object(params)).map_err(|e| nothing_saved(e.0))?;
        let applied = change(session, &op_args, c).map_err(|e| nothing_saved(e.0))?;
        notes.extend(applied.notes.into_iter().map(|n| format!("{tag}: {n}")));
        summaries.push(applied.summary);
        let mut result = applied.result;
        result.insert("op".into(), json!(name));
        results.push(Value::Object(result));
    }
    let mut result = Map::new();
    result.insert("operations".into(), json!(results.len()));
    result.insert("results".into(), json!(results));
    Ok(Applied { summary: format!("{} edits: {}", summaries.len(), summaries.join("; ")), notes, result })
}

fn undo_last_edit(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let history = session.project().history(&loaded.id)?;
    // A move or a deletion is a step in the history but changes no text; skip back to the last
    // step that did.
    let Some(last) = history.iter().rev().find(|e| e.base_sha.as_deref().is_some_and(|base| base != e.result_sha)) else {
        return fail("there is no edit of this plasmid to undo");
    };
    let before = last.version.saturating_sub(1);
    let restored = session.project().restore(&loaded.id, before, session.who())?;
    Ok(json!({
        "undone": last.command,
        "what_it_was": last.summary,
        "made_by": last.actor,
        "plasmid": refs::label(&restored.path),
        "version": token(&restored),
        "previous_version": token(&loaded),
    }))
}
