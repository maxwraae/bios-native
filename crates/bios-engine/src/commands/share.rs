//! Sharing: a project kept in step with the same project on a Bios server.
//!
//! The project here stays a whole project with its own history. These commands bring in what
//! other people saved on the server and send what was saved here. They never merge: when a
//! plasmid changed in both places both versions are kept, and `settle_conflict` is the person's
//! decision.

use bios_sync::{Keep, Report, Role};
use serde_json::{Value, json};

use super::PLASMID;
use crate::{Args, CommandSpec, Effect, Failure, Kind, Param, Run, Server, Session, fail};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "sharing_status",
        description: "Whether this project is shared on a Bios server, which one, how many plasmids are kept in step, and whether a server is set up for this session at all.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(sharing_status),
    },
    CommandSpec {
        name: "server_projects",
        description: "The projects this account can open on the Bios server, with what it may do in each (owner, editor or viewer). Use it to find the project to join.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(server_projects),
    },
    CommandSpec {
        name: "share_project",
        description: "Put this project on the Bios server as a new project there, with every plasmid in it, so that other people can be invited. Ask the person before sharing: it sends their plasmids to the server.",
        params: &[Param {
            name: "title",
            kind: Kind::String,
            required: false,
            description: "What the project is called on the server. Default: its name here.",
        }],
        effect: Effect::Write,
        run: Run::Call(share_project),
    },
    CommandSpec {
        name: "join_shared_project",
        description: "Connect this project to a project that is already on the Bios server and bring in everything in it. From then on sync_project keeps the two in step.",
        params: &[Param {
            name: "project",
            kind: Kind::String,
            required: true,
            description: "The project on the server: its id, or its title when only one project has it (see server_projects).",
        }],
        effect: Effect::Write,
        run: Run::Call(join_shared_project),
    },
    CommandSpec {
        name: "sync_project",
        description: "Bring in what other people saved on the server and send what was saved here. Nothing is ever merged or overwritten: a plasmid that changed in both places is reported as a conflict with both versions untouched, to be settled with settle_conflict.",
        params: &[],
        effect: Effect::Write,
        run: Run::Call(sync_project),
    },
    CommandSpec {
        name: "settle_conflict",
        description: "Settle a plasmid that changed both here and on the server. \"mine\" saves the version here on top of the server's; \"theirs\" makes the server's version the newest one here, and the version it replaces stays in the history. Ask the person which; never choose for them.",
        params: &[PLASMID, Param { name: "keep", kind: Kind::String, required: true, description: "\"mine\" or \"theirs\"." }],
        effect: Effect::Write,
        run: Run::Call(settle_conflict),
    },
];

fn server(session: &Session) -> Result<&Server, Failure> {
    session.server().ok_or_else(|| {
        Failure(
            "No Bios server is set up for this session. Start Bios with BIOS_SERVER set to the server's address, and BIOS_SERVER_TOKEN or BIOS_SERVER_KEY to what it gave you to sign in with."
                .to_string(),
        )
    })
}

fn role(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
    }
}

/// A report as an agent reads it: the sentences, and the conflicts by name so they can be settled.
fn told(report: &Report) -> Value {
    let names = |items: &[bios_sync::Item]| items.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
    json!({
        "server": report.server,
        "in_step": report.is_clean(),
        "said": if report.is_empty() { vec![report.to_string()] } else { report.lines() },
        "brought_in": names(&report.brought_in),
        "sent": names(&report.sent),
        "added_here": names(&report.added_here),
        "added_on_server": names(&report.added_there),
        "no_longer_on_server": names(&report.gone),
        "conflicts": report.conflicts.iter().map(|c| json!({
            "plasmid": c.name, "saved_here": c.mine_saved, "saved_on_server": c.theirs_saved, "what": c.what,
        })).collect::<Vec<_>>(),
        "skipped": report.skipped.iter().map(|s| json!({ "plasmid": s.name, "why": s.why })).collect::<Vec<_>>(),
    })
}

fn sharing_status(session: &Session, _: &Args) -> Result<Value, Failure> {
    let connection = bios_sync::connection(session.project()).map_err(|e| Failure(e.to_string()))?;
    Ok(json!({
        "shared": connection.is_some(),
        "server": connection.as_ref().map(|c| c.server.clone()),
        "project_on_server": connection.as_ref().map(|c| c.project.clone()),
        "plasmids_in_step": connection.as_ref().map(|c| c.constructs),
        "server_set_up": session.server().map(|s| s.name.clone()),
    }))
}

fn server_projects(session: &Session, _: &Args) -> Result<Value, Failure> {
    let server = server(session)?;
    let projects = server.gateway.list_projects().map_err(|e| Failure(e.to_string()))?;
    Ok(json!({
        "server": server.name,
        "projects": projects.iter().map(|p| json!({ "id": p.id, "title": p.title, "role": role(p.role) })).collect::<Vec<_>>(),
    }))
}

fn share_project(session: &Session, args: &Args) -> Result<Value, Failure> {
    let server = server(session)?;
    let title =
        args.opt_str("title")?.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| session.project().name().to_string());
    let report =
        bios_sync::publish(session.project(), server.gateway.as_ref(), &server.name, &title, session.who()).map_err(|e| Failure(e.to_string()))?;
    Ok(json!({ "shared_as": title, "report": told(&report) }))
}

fn join_shared_project(session: &Session, args: &Args) -> Result<Value, Failure> {
    let server = server(session)?;
    let wanted = args.str("project")?;
    let wanted = wanted.trim();
    let projects = server.gateway.list_projects().map_err(|e| Failure(e.to_string()))?;
    let by_id: Vec<_> = projects.iter().filter(|p| p.id == wanted).collect();
    let hits = if by_id.is_empty() { projects.iter().filter(|p| p.title.eq_ignore_ascii_case(wanted)).collect() } else { by_id };
    let project = match hits.as_slice() {
        [one] => one,
        [] => return fail(format!("there is no project called \"{wanted}\" on {} that this account can open", server.name)),
        many => return fail(format!("{} projects on {} are called \"{wanted}\"; give the id of the one you mean", many.len(), server.name)),
    };
    let report = bios_sync::connect(session.project(), server.gateway.as_ref(), &server.name, &project.id, session.who())
        .map_err(|e| Failure(e.to_string()))?;
    Ok(json!({ "joined": project.title, "role": role(project.role), "report": told(&report) }))
}

fn sync_project(session: &Session, _: &Args) -> Result<Value, Failure> {
    let server = server(session)?;
    let report = bios_sync::sync(session.project(), server.gateway.as_ref(), session.who()).map_err(|e| Failure(e.to_string()))?;
    Ok(told(&report))
}

fn settle_conflict(session: &Session, args: &Args) -> Result<Value, Failure> {
    let server = server(session)?;
    let keep = match args.str("keep")?.trim().to_lowercase().as_str() {
        "mine" => Keep::Mine,
        "theirs" => Keep::Theirs,
        other => return fail(format!("keep is \"mine\" or \"theirs\", not \"{other}\"")),
    };
    let loaded = session.load(args)?;
    let report =
        bios_sync::resolve(session.project(), server.gateway.as_ref(), &loaded.id, keep, session.who()).map_err(|e| Failure(e.to_string()))?;
    Ok(json!({ "kept": if keep == Keep::Mine { "mine" } else { "theirs" }, "report": told(&report) }))
}
