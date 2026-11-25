//! Bios without a window: the same commands the window and MCP use, from a terminal or a script.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

use bios_engine::{Effect, Kind, Session};
use bios_project::recents::Recents;
use bios_project::{Actor, Project, Who};
use serde_json::Value;

const USAGE: &str = "\
bios-cli new <folder> [name]              make a folder a Bios project
bios-cli commands [command]               list every command, or describe one
bios-cli run <project> <command> [json]   run a command in a project; parameters are a JSON object
bios-cli home <tool> [json]               over the folders opened in Bios (or a folder of projects
                                          with --home <folder>): list_projects, open_project,
                                          create_project, rename_project, delete_project,
                                          remove_recent_project, or any command with its `project`
bios-cli mcp [folder]                     serve projects to an agent over MCP (stdio): the folders
                                          opened in Bios, or with a folder, that project (any
                                          folder) or folder of projects
bios-cli update [--check]                 install the newest release of Bios in place of this one
                                          (an open window reopens on it); --check only says
                                          whether there is one

  --agent    record the change as made by an agent, not a person
  --home     a folder of projects for `home` to work in";

fn main() -> ExitCode {
    bios_update::set_up(bios_update::After::RunningWindow);
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let agent = args.iter().any(|a| a == "--agent");
    args.retain(|a| a != "--agent");
    let home = args.iter().position(|a| a == "--home").map(|at| {
        let folder = args.get(at + 1).cloned();
        args.drain(at..(at + 2).min(args.len()));
        folder
    });
    let words: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = match words.as_slice() {
        ["new", folder] => new(folder, None),
        ["new", folder, name] => new(folder, Some(name)),
        ["commands"] => {
            list_commands();
            Ok(())
        }
        ["commands", name] => describe(name),
        ["run", project, command] => run(project, command, "{}", agent),
        ["run", project, command, json] => run(project, command, json, agent),
        ["home", tool] => in_home(home, tool, "{}", agent),
        ["home", tool, json] => in_home(home, tool, json, agent),
        ["mcp"] => mcp_recents(),
        ["mcp", folder] => mcp(Path::new(folder)),
        ["update"] => update("update_bios", agent),
        ["update", "--check"] => update("check_for_update", agent),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn new(folder: &str, name: Option<&str>) -> Result<(), String> {
    let path = Path::new(folder);
    let name = name.map(str::to_owned).or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "Project".into());
    let project = Project::create(path, &name).map_err(|e| e.to_string())?;
    println!("Made the project \"{}\" in {}", project.name(), project.root().display());
    Ok(())
}

fn list_commands() {
    for c in bios_engine::registry() {
        let mark = if c.effect == Effect::Write { "edits" } else { "reads" };
        let first = c.description.split(". ").next().unwrap_or(c.description).trim_end_matches('.');
        println!("{:<20} {mark}  {first}.", c.name);
    }
}

fn describe(name: &str) -> Result<(), String> {
    let c = bios_engine::command(name).ok_or_else(|| format!("there is no command called {name}"))?;
    println!("{}\n\n{}\n", c.name, c.description);
    for p in c.params {
        let kind = match p.kind {
            Kind::String => "text",
            Kind::Integer => "whole number",
            Kind::Number => "number",
            Kind::Boolean => "true/false",
            Kind::TextMap => "object",
            Kind::Objects | Kind::Strings => "list",
        };
        println!("  {:<14} {kind:<10} {}{}", p.name, if p.required { "required. " } else { "" }, p.description);
    }
    Ok(())
}

fn run(project: &str, command: &str, json: &str, agent: bool) -> Result<(), String> {
    let params: Value = serde_json::from_str(json).map_err(|e| format!("the parameters are not valid JSON: {e}"))?;
    let project = Project::open(Path::new(project)).map_err(|e| e.to_string())?;
    let who = Who { actor: if agent { Actor::Agent } else { Actor::Human }, source: "cli".into(), session: None };
    let result = Session::open(project, who).execute(command, &params).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// Runs one of the commands that act on Bios itself, with no project.
fn update(command: &str, agent: bool) -> Result<(), String> {
    let who = Who { actor: if agent { Actor::Agent } else { Actor::Human }, source: "cli".into(), session: None };
    let result = Session::for_app(who).and_then(|s| s.execute(command, &Value::Object(Default::default()))).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// Runs a tool over the folders opened in Bios, or a folder of projects, as an MCP connection would.
fn in_home(folder: Option<Option<String>>, tool: &str, json: &str, agent: bool) -> Result<(), String> {
    let params: Value = serde_json::from_str(json).map_err(|e| format!("the parameters are not valid JSON: {e}"))?;
    let who = Who { actor: if agent { Actor::Agent } else { Actor::Human }, source: "cli".into(), session: None };
    let result = match folder {
        Some(Some(folder)) => bios_mcp::run_in_home(Path::new(&folder), who, tool, &params)?,
        Some(None) => return Err("--home needs a folder".into()),
        None => bios_mcp::run_in_recents(Recents::standard(&[]), who, tool, &params)?,
    };
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// A folder of projects is served as one; any other folder is served as a project, read from
/// its files until something is changed in it.
fn mcp(folder: &Path) -> Result<(), String> {
    let tools = if !Project::is_project(folder) && holds_projects(folder) {
        bios_mcp::BiosTools::for_agent_in_home(folder)
    } else {
        bios_mcp::BiosTools::for_agent(Project::open(folder).map_err(|e| e.to_string())?)
    };
    let stdin = std::io::stdin();
    bios_mcp::serve(tools, stdin.lock(), std::io::stdout()).map_err(|e| e.to_string())
}

/// Whether a folder directly holds at least one project.
fn holds_projects(folder: &Path) -> bool {
    std::fs::read_dir(folder).into_iter().flatten().flatten().any(|e| Project::is_project(&e.path()))
}

/// Serves the folders opened in Bios: what the window lists on its projects page.
fn mcp_recents() -> Result<(), String> {
    let tools = bios_mcp::BiosTools::for_agent_in_recents(Recents::standard(&[]));
    let stdin = std::io::stdin();
    bios_mcp::serve(tools, stdin.lock(), std::io::stdout()).map_err(|e| e.to_string())
}
