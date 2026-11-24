//! What the person has open in the Bios window, for an agent working beside them.
//!
//! The window writes it down as it changes ([`bios_project::window`]); this reads it. It needs no
//! project, so it runs in any session ([`crate::Session::for_app`] when none is open).

use std::path::Path;

use bios_project::Project;
use bios_project::recents::{self, Recents};
use bios_project::window::{self, Record};
use serde_json::{Value, json};

use crate::{Args, CommandSpec, Effect, Failure, Run, Session};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "what_is_open",
    description: "What the person has open in the Bios window right now: the project (its name and folder path), the place (edit, build, verify, or the projects page), the plasmid in front (id, path as list_plasmids shows it, name, length, circular), the open tabs, and the selected bases (1-based, inclusive). Call this first, before anything else, to learn which project and plasmid the person is looking at, and again whenever they refer to what is on their screen: \"this plasmid\", \"the open one\" or \"here\" means its `plasmid`, \"this project\" its `project`, and \"the selection\" or \"these bases\" its `selection`. Pass the project's path as `project` and the plasmid's path as `plasmid` to the other tools. When no Bios window is open it says so, and names the folder opened most recently as a hint. It changes nothing.",
    params: &[],
    effect: Effect::Read,
    run: Run::Call(what_is_open),
}];

fn what_is_open(session: &Session, _: &Args) -> Result<Value, Failure> {
    let Some(file) = session.window_file() else {
        return Ok(no_window(None));
    };
    match window::live(&file) {
        Some(record) => Ok(described(&record)),
        None => {
            let latest = Recents::at(&file.with_file_name(recents::FILE)).folders().into_iter().next();
            Ok(no_window(latest.as_deref()))
        }
    }
}

fn no_window(latest: Option<&Path>) -> Value {
    let latest = latest.map(|root| {
        let name = Project::open(root).map(|p| p.name().to_string()).unwrap_or_else(|_| root.display().to_string());
        json!({ "name": name, "path": root.display().to_string() })
    });
    let said = match &latest {
        Some(l) => format!(
            "No Bios window is open, so nothing is on the person's screen. The folder opened most recently in Bios is {} ({}); ask the person before assuming that is what they mean.",
            l["name"].as_str().unwrap_or(""),
            l["path"].as_str().unwrap_or("")
        ),
        None => "No Bios window is open, so nothing is on the person's screen.".into(),
    };
    json!({ "window_open": false, "latest_project": latest, "said": said })
}

fn described(record: &Record) -> Value {
    let front = &record.front;
    let mut said = match &front.project {
        None => "The Bios window is on the projects page; no project is open.".to_string(),
        Some(project) => match &front.plasmid {
            Some(p) => format!("The person has {} ({}) open in project {}, in {}.", p.name, p.path, project.name, front.place),
            None => format!("The person has project {} open, in {}, with no plasmid in front.", project.name, front.place),
        },
    };
    if let Some(s) = front.selection {
        said.push_str(&format!(" Bases {}..{} are selected.", s.start, s.end));
    }
    json!({
        "window_open": true,
        "project": front.project.as_ref().map(|p| json!({ "name": p.name, "path": p.path.display().to_string() })),
        "place": front.place,
        "plasmid": front.plasmid,
        "tabs": front.tabs,
        "selection": front.selection,
        "updated_at": record.updated_at,
        "said": said,
    })
}
