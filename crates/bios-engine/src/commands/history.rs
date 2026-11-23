//! What happened to a construct, and going back.

use serde_json::{Value, json};

use super::PLASMID;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Session, refs, token};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "plasmid_history",
        description: "Every change made to a plasmid, oldest first: who made it (a person or an agent), with what command, and when. Use it to see what an agent did, or to pick a version to go back to.",
        params: &[PLASMID],
        effect: Effect::Read,
        run: Run::Call(plasmid_history),
    },
    CommandSpec {
        name: "restore_version",
        description: "Go back to an earlier version of a plasmid. The earlier version is saved again as the newest one, so nothing in between is lost and the restore itself can be undone.",
        params: &[
            PLASMID,
            Param {
                name: "to_version",
                kind: Kind::Integer,
                required: true,
                description: "The version number to go back to, as plasmid_history shows it.",
            },
        ],
        effect: Effect::Write,
        run: Run::Call(restore_version),
    },
];

fn plasmid_history(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let entries: Vec<Value> = session
        .project()
        .history(&loaded.id)?
        .into_iter()
        .map(|e| {
            json!({
                "version": e.version,
                "at": e.at,
                "by": e.actor,
                "from": e.source,
                "session": e.session,
                "command": e.command,
                "summary": e.summary,
            })
        })
        .collect();
    Ok(json!({ "plasmid": refs::label(&loaded.path), "current_version": token(&loaded), "history": entries }))
}

fn restore_version(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let wanted = u64::try_from(args.int("to_version")?).map_err(|_| Failure("to_version must be 1 or more".into()))?;
    let restored = session.project().restore(&loaded.id, wanted, session.who())?;
    Ok(json!({
        "saved": restored.version != loaded.version,
        "plasmid": refs::label(&restored.path),
        "version": token(&restored),
        "previous_version": token(&loaded),
        "restored": wanted,
    }))
}
