//! Bios's own version: is there a newer one, and installing it.
//!
//! These two act on Bios itself, not on a project, so they run in any session
//! ([`crate::Session::for_app`] when no project is open).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::update::{Check, Updater};
use crate::{Args, CommandSpec, Effect, Failure, Run, Session};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "check_for_update",
        description: "Whether a newer version of Bios has been published: the version running now, the newest release, whether it is newer, and its release notes. It changes nothing. Install it with update_bios once the person agrees.",
        params: &[],
        effect: Effect::Read,
        run: Run::Call(check_for_update),
    },
    CommandSpec {
        name: "update_bios",
        description: "Download the newest release of Bios, check it and install it in place of this copy; an open Bios window closes and opens again on the new version. Ask the person first. Afterwards this MCP connection still runs the old version: tell the person to reconnect Bios's MCP server (in Claude Code: /mcp, then reconnect bios) to get the new version's tools.",
        params: &[],
        effect: Effect::Write,
        run: Run::Call(update_bios),
    },
];

/// Reconnecting is the only way an agent picks up the new commands.
const RECONNECT: &str = "This connection still runs the old version. Tell the person to reconnect Bios's MCP server (in Claude Code: /mcp, then reconnect bios) to use the new version and its tools.";

fn updater(session: &Session) -> Result<Arc<dyn Updater>, Failure> {
    session.updater().ok_or_else(|| Failure("this copy of Bios was built without its updater, so it cannot check for or install updates".into()))
}

fn described(check: &Check) -> Value {
    let said = match (&check.latest, check.newer) {
        (None, _) => format!("No release of Bios has been published yet; this is {}.", check.current),
        (Some(latest), true) => format!("Bios {latest} is out; this is {}.", check.current),
        (Some(_), false) => format!("Bios {} is the newest version.", check.current),
    };
    json!({
        "current_version": check.current,
        "latest_version": check.latest,
        "update_available": check.newer,
        "release_notes": check.notes,
        "release_page": check.page,
        "can_install_itself": check.cannot_install.is_none(),
        "cannot_install_because": check.cannot_install,
        "said": said,
    })
}

fn check_for_update(session: &Session, _: &Args) -> Result<Value, Failure> {
    let check = updater(session)?.check().map_err(Failure)?;
    Ok(described(&check))
}

fn update_bios(session: &Session, _: &Args) -> Result<Value, Failure> {
    let updater = updater(session)?;
    let check = updater.check().map_err(Failure)?;
    if !check.newer {
        let mut out = described(&check);
        out["updated"] = json!(false);
        return Ok(out);
    }
    if let Some(why) = &check.cannot_install {
        return Err(Failure(format!(
            "Bios {} is out, but this copy cannot install it itself: {why}. Download it from {}",
            check.latest.as_deref().unwrap_or("?"),
            check.page
        )));
    }
    let done = updater.install().map_err(Failure)?;
    Ok(json!({
        "updated": true,
        "from_version": done.from,
        "to_version": done.to,
        "installed_at": done.location,
        "window": done.relaunch,
        "release_notes": check.notes,
        "next": RECONNECT,
    }))
}
