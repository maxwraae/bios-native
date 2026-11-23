//! Bios over MCP: every engine command is a tool, with the same name and the same parameters.
//!
//! There is no second implementation here and there must never be one. A tool's definition is
//! built from the command's own spec and a call goes straight to [`Session::execute`], so an agent
//! can do exactly what the window can, and what it does is journaled as an agent's work.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use bios_engine::{CommandSpec, Effect, Kind, Session};
use bios_project::recents::{self, Recents};
use bios_project::{Actor, Project, Who};
use craft_mcp_server::{Info, Server, Tools, object, text_result, tool};
use serde_json::{Map, Value, json};

/// The longest request line read. A whole GenBank file can arrive in one call.
const MAX_LINE: usize = 64 * 1024 * 1024;

const ONE_PROJECT: &str = "\
Bios is a workspace for designing DNA. You are working inside one project: a set of plasmids, in folders, each with its full history.";

const HOME: &str = "\
Bios is a workspace for designing DNA. You are working in a folder of Bios projects; each project is a set of plasmids, in folders, each with its full history.

Start with list_projects. Every other tool takes `project`: the project's folder name as list_projects shows it (its name works too), or any folder's absolute path. A plasmid can also be written as \"Project/path\" with no `project`. Tools that take a second plasmid (from_plasmid, from) look for it in the same project. Make a new project with create_project only when the person asks for one.";

const RECENT: &str = "\
Bios is a workspace for designing DNA. A project is any folder on this computer: its plasmids are the sequence files in it (GenBank, FASTA, SnapGene, EMBL), in subfolders, each with its full history once Bios has changed it. Bios keeps a list of the folders the person has opened, as a code editor does.

Start with list_projects: the folders opened in Bios, with each one's path. Every other tool takes `project`: a folder's name as list_projects shows it, or any folder's absolute path (that folder is opened and joins the list, as open_project does). Opening or reading a folder writes nothing into it; the first change makes it a Bios project (a bios.json and a hidden .bios folder beside its files). A plasmid can also be written as \"Project/path\" with no `project`. Tools that take a second plasmid (from_plasmid, from) look for it in the same project. Make a new project with create_project only when the person asks for one.";

const INSTRUCTIONS: &str = "\
How to work here:
- Find plasmids with list_plasmids or find_plasmid. Address a plasmid by its path (\"backbones/pUC19\"), its name if unique, or its id.
- Read before you edit. read_plasmid returns a `version`; every edit needs the version it was made against. If the plasmid changed in the meantime the edit is refused with CONFLICT and nothing is saved: read it again and redo the edit.
- Positions are 1-based and inclusive, as a biologist counts them. On a circular plasmid start > end runs through the origin.
- Use the sequence tools (insert_sequence, delete_range, replace_range) to change bases: features follow their bases, and the result says which moved, shrank or were removed. Tell the person about any feature that was trimmed or removed.
- There is one copy of each plasmid. To start a variant use duplicate_plasmid, which records what it was made from. Never make copies by other means.
- Everything you change is recorded as your work in plasmid_history, and any version can be brought back with restore_version. Say what you changed so the person can check it.";

/// The engine's commands as MCP tools, for one caller in one project, in a folder of projects, or
/// in the folders the person has opened in Bios.
pub struct BiosTools {
    scope: Scope,
}

enum Scope {
    One(Session),
    Many(Many),
}

/// Where the projects of a connection that serves more than one come from.
enum Source {
    /// A folder whose subfolders are projects.
    Folder(PathBuf),
    /// The folders opened in Bios, by the window, the command line or an agent.
    Recent(Recents),
}

/// Several projects. A session is opened for a project the first time a call names it, so every
/// change on this connection is journaled under the same agent.
struct Many {
    source: Source,
    who: Who,
    sessions: HashMap<PathBuf, Session>,
    /// Where the commands that act on Bios itself run (`check_for_update`, `update_bios`), made at
    /// the first such call.
    app: Option<Box<Session>>,
}

impl BiosTools {
    pub fn new(session: Session) -> Self {
        BiosTools { scope: Scope::One(session) }
    }

    /// An agent working in `project` over MCP. Everything it changes is journaled as an agent's
    /// work, grouped under one session id for this connection.
    pub fn for_agent(project: Project) -> Self {
        BiosTools::new(Session::open(project, agent()))
    }

    /// An agent working in every project inside `root`, a folder of projects.
    pub fn for_agent_in_home(root: &Path) -> Self {
        BiosTools { scope: Scope::Many(Many::new(Source::Folder(root.to_path_buf()), agent())) }
    }

    /// An agent working in the folders the person has opened in Bios, and any other folder it is
    /// given by path, which then joins that list.
    pub fn for_agent_in_recents(recents: Recents) -> Self {
        BiosTools { scope: Scope::Many(Many::new(Source::Recent(recents), agent())) }
    }
}

/// Runs one tool of a folder of projects (`list_projects`, `rename_project`…, or any command with
/// its `project`) as `who`, outside an MCP connection: what `bios-cli home --home <folder>` does.
pub fn run_in_home(root: &Path, who: Who, name: &str, args: &Value) -> Result<Value, String> {
    Many::new(Source::Folder(root.to_path_buf()), who).dispatch(name, args)
}

/// Runs one tool over the folders opened in Bios, as `who`: what `bios-cli home` does.
pub fn run_in_recents(recents: Recents, who: Who, name: &str, args: &Value) -> Result<Value, String> {
    Many::new(Source::Recent(recents), who).dispatch(name, args)
}

fn agent() -> Who {
    Who { actor: Actor::Agent, source: "mcp".into(), session: Some(uuid::Uuid::new_v4().to_string()) }
}

/// A project folder's own name: what list_projects calls it and `project` takes.
fn label(root: &Path) -> String {
    root.file_name().map_or_else(|| root.display().to_string(), |f| f.to_string_lossy().into_owned())
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

impl Many {
    fn new(source: Source, who: Who) -> Many {
        Many { source, who, sessions: HashMap::new(), app: None }
    }

    /// The projects, each by its folder's name.
    fn projects(&self) -> Result<Vec<(String, Project)>, String> {
        match &self.source {
            Source::Folder(root) => bios_project::home::Home::new(root).projects().map_err(|e| e.to_string()),
            Source::Recent(recents) => {
                Ok(recents.folders().into_iter().filter_map(|root| Project::open(&root).ok().map(|p| (label(&root), p))).collect())
            }
        }
    }

    /// A folder given by its absolute path is opened as the window opens it, and joins the list.
    fn open_path(&mut self, path: &Path) -> Result<Project, String> {
        if !path.is_dir() {
            return Err(format!("{} is not a folder", path.display()));
        }
        let root = match &mut self.source {
            Source::Recent(recents) => recents.opened(path).map_err(|e| e.to_string())?,
            Source::Folder(_) => path.to_path_buf(),
        };
        Project::open(&root).map_err(|e| e.to_string())
    }

    /// The project called `name`: an absolute path, or a folder's name, then the name inside it,
    /// then either ignoring case.
    fn find(&mut self, name: &str) -> Result<Project, String> {
        let wanted = name.trim().trim_end_matches('/');
        if Path::new(wanted).is_absolute() {
            return self.open_path(Path::new(wanted));
        }
        if let Source::Folder(root) = &self.source {
            return bios_project::home::Home::new(root).find(wanted).map_err(|e| e.to_string());
        }
        let projects = self.projects()?;
        let exact = |a: &str, b: &str| a == b;
        let loose = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        for same in [&exact as &dyn Fn(&str, &str) -> bool, &loose] {
            let hits: Vec<&(String, Project)> = projects.iter().filter(|(folder, p)| same(folder, wanted) || same(p.name(), wanted)).collect();
            match hits.as_slice() {
                [] => continue,
                [(_, one)] => return Project::open(one.root()).map_err(|e| e.to_string()),
                several => {
                    let paths: Vec<String> = several.iter().map(|(_, p)| p.root().display().to_string()).collect();
                    return Err(format!("more than one project is called \"{name}\"; pass its path as `project`: {}", paths.join(", ")));
                }
            }
        }
        Err(if projects.is_empty() {
            format!(
                "there is no project called \"{name}\"; no folders have been opened in Bios yet. Pass a folder's absolute path as `project`, or open_project it"
            )
        } else {
            format!(
                "there is no project called \"{name}\". The projects are: {}. Any other folder can be given by its absolute path",
                names(&projects)
            )
        })
    }

    fn session(&mut self, project: Project) -> &Session {
        let who = self.who.clone();
        self.sessions.entry(project.root().to_path_buf()).or_insert_with(|| Session::open(project, who))
    }

    fn row(folder: &str, p: &Project) -> Value {
        let plasmids = p.list().map(|l| l.len()).unwrap_or(0);
        json!({ "project": folder, "name": p.name(), "path": p.root().display().to_string(), "plasmids": plasmids })
    }

    fn list_projects(&self) -> Result<Value, String> {
        let rows: Vec<Value> = self.projects()?.iter().map(|(folder, p)| Many::row(folder, p)).collect();
        Ok(match &self.source {
            Source::Folder(root) => json!({ "home": root.display().to_string(), "projects": rows }),
            Source::Recent(_) => json!({ "projects": rows }),
        })
    }

    fn dispatch(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "list_projects" => self.list_projects(),
            "open_project" => self.open_project(args),
            "create_project" => self.create_project(args),
            "rename_project" => self.rename_project(args),
            "delete_project" => self.delete_project(args),
            "remove_recent_project" => self.remove_recent_project(args),
            _ => self.call(name, args),
        }
    }

    fn open_project(&mut self, args: &Value) -> Result<Value, String> {
        let path = text_arg(args, "path").ok_or("open_project needs `path`: the folder's absolute path")?;
        if !Path::new(path).is_absolute() {
            return Err(format!("open_project needs an absolute path; {path} is not one"));
        }
        let project = self.open_path(Path::new(path))?;
        let mut row = Many::row(&label(project.root()), &project);
        row["is_project_yet"] = json!(project.is_saved());
        Ok(row)
    }

    fn create_project(&mut self, args: &Value) -> Result<Value, String> {
        let name = text_arg(args, "name").unwrap_or("");
        let parent = match (text_arg(args, "path"), &self.source) {
            (Some(path), _) if Path::new(path).is_absolute() => PathBuf::from(path),
            (Some(path), _) => return Err(format!("create_project's `path` must be an absolute folder path; {path} is not one")),
            (None, Source::Folder(root)) => root.clone(),
            (None, Source::Recent(_)) => recents::default_location().ok_or("say where to make it with `path`")?,
        };
        let project = bios_project::home::create_in(&parent, name).map_err(|e| e.to_string())?;
        let mut root = project.root().to_path_buf();
        if let Source::Recent(recents) = &mut self.source {
            root = recents.opened(&root).map_err(|e| e.to_string())?;
        }
        Ok(json!({ "project": label(&root), "name": project.name(), "path": root.display().to_string() }))
    }

    fn rename_project(&mut self, args: &Value) -> Result<Value, String> {
        let (Some(project), Some(to)) = (text_arg(args, "project"), args.get("new_name").and_then(Value::as_str)) else {
            return Err("rename_project needs `project` and `new_name`".into());
        };
        let from = self.find(project)?.root().to_path_buf();
        let now = bios_project::home::rename_folder(&from, to).map_err(|e| e.to_string())?;
        if let Source::Recent(recents) = &mut self.source {
            recents.moved(&from, &now).map_err(|e| e.to_string())?;
        }
        // A session held on the old folder would write where the project no longer is.
        self.sessions.clear();
        let renamed = Project::open(&now).map_err(|e| e.to_string())?;
        Ok(json!({ "project": label(&now), "name": renamed.name(), "path": now.display().to_string() }))
    }

    fn delete_project(&mut self, args: &Value) -> Result<Value, String> {
        let project = text_arg(args, "project").ok_or("delete_project needs `project`")?;
        if args.get("confirm").and_then(Value::as_bool) != Some(true) {
            return Err("delete_project moves a whole project folder to the system Trash; pass confirm: true once the person has said yes".into());
        }
        let found = self.find(project)?;
        let root = found.root().to_path_buf();
        bios_project::home::trash_folder(&root).map_err(|e| e.to_string())?;
        if let Source::Recent(recents) = &mut self.source {
            recents.forget(&root).map_err(|e| e.to_string())?;
        }
        self.sessions.clear();
        Ok(
            json!({ "deleted": found.name(), "path": root.display().to_string(), "note": "the folder is in the system Trash; the person can put it back from there" }),
        )
    }

    fn remove_recent_project(&mut self, args: &Value) -> Result<Value, String> {
        let project = text_arg(args, "project").ok_or("remove_recent_project needs `project`")?;
        let root = self.find(project)?.root().to_path_buf();
        match &mut self.source {
            Source::Recent(recents) => recents.forget(&root).map_err(|e| e.to_string())?,
            Source::Folder(_) => return Err("this connection serves a folder of projects, which has no list of opened folders".into()),
        }
        self.sessions.remove(&root);
        Ok(json!({ "removed": label(&root), "path": root.display().to_string(), "note": "only taken off the list; the folder is untouched" }))
    }

    /// Runs a command in the project the call names, by `project` or as "Project/path".
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let mut params = match args {
            Value::Object(map) => map.clone(),
            Value::Null => Map::new(),
            _ => return Err(format!("{name} takes its parameters as a JSON object")),
        };
        let named = params.remove("project").and_then(|v| v.as_str().map(str::to_owned));
        if bios_engine::command(name).is_some_and(|spec| !spec.needs_project()) {
            return self.app_session()?.execute(name, &Value::Object(params)).map_err(|failure| failure.0);
        }
        let project = match named {
            Some(project) => self.find(&project)?,
            None => self.named_in_path(&mut params)?,
        };
        self.session(project).execute(name, &Value::Object(params)).map_err(|failure| failure.0)
    }

    /// The session the commands that act on Bios itself run in.
    fn app_session(&mut self) -> Result<&Session, String> {
        if self.app.is_none() {
            self.app = Some(Box::new(Session::for_app(self.who.clone()).map_err(|failure| failure.0)?));
        }
        self.app.as_deref().ok_or_else(|| "no session for Bios itself".to_string())
    }

    /// The project in a plasmid written as "Project/path", which is then cut down to the path.
    fn named_in_path(&mut self, params: &mut Map<String, Value>) -> Result<Project, String> {
        if let Some(Value::String(plasmid)) = params.get("plasmid")
            && let Some((head, rest)) = plasmid.split_once('/')
            && !head.is_empty()
        {
            let rest = rest.to_string();
            if let Ok(project) = self.find(head) {
                params.insert("plasmid".into(), Value::String(rest));
                return Ok(project);
            }
        }
        let projects = self.projects()?;
        match projects.len() {
            1 => projects.into_iter().next().map(|(_, p)| p).ok_or_else(|| "no projects".into()),
            0 => Err(match &self.source {
                Source::Folder(root) => format!("there are no projects in {} yet; make one with create_project", root.display()),
                Source::Recent(_) => "no folders have been opened in Bios yet; open one with open_project or make one with create_project".into(),
            }),
            _ => Err(format!("say which project with `project`. The projects are: {}", names(&projects))),
        }
    }
}

fn names(projects: &[(String, Project)]) -> String {
    if projects.is_empty() {
        return "(none yet)".into();
    }
    projects.iter().map(|(folder, _)| folder.as_str()).collect::<Vec<_>>().join(", ")
}

fn title(name: &str) -> String {
    let mut words = name.replace('_', " ");
    if let Some(first) = words.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    words
}

fn definition(spec: &CommandSpec) -> Value {
    let mut props = Map::new();
    for p in spec.params {
        let mut schema = match p.kind {
            Kind::String => json!({ "type": "string" }),
            Kind::Integer => json!({ "type": "integer" }),
            Kind::Boolean => json!({ "type": "boolean" }),
            Kind::TextMap => json!({ "type": "object", "additionalProperties": { "type": "string" } }),
            Kind::Objects => json!({ "type": "array", "items": { "type": "object" } }),
            Kind::Strings => json!({ "type": "array", "items": { "type": "string" } }),
        };
        schema["description"] = json!(p.description);
        props.insert(p.name.to_string(), schema);
    }
    let required: Vec<&str> = spec.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    // A write here is journaled and can be undone, so it is an edit, not a destructive act.
    let effect = match spec.effect {
        Effect::Read => craft_mcp_server::Effect::ReadOnly,
        Effect::Write => craft_mcp_server::Effect::Edits,
    };
    tool(spec.name, &title(spec.name), spec.description, object(Value::Object(props), &required), effect)
}

/// A tool's definition with the `project` it runs in added in front of its own parameters.
fn in_a_project(spec: &CommandSpec) -> Value {
    let mut def = definition(spec);
    let schema = &mut def["inputSchema"];
    let project = json!({ "type": "string", "description": "The project to work in: its folder name as list_projects shows it, or a folder's absolute path. May be left out when the plasmid is written as \"Project/path\"." });
    if let Some(props) = schema["properties"].as_object_mut() {
        let mut with = Map::new();
        with.insert("project".into(), project);
        with.extend(std::mem::take(props));
        *props = with;
    }
    def
}

fn home_tools() -> Vec<Value> {
    let project =
        json!({ "type": "string", "description": "The project: its folder's name as list_projects shows it, or the folder's absolute path." });
    vec![
        tool(
            "list_projects",
            "List projects",
            "List the projects: each one's folder name (pass it as `project` to every other tool), its name, its path, and how many plasmids it holds.",
            object(json!({}), &[]),
            craft_mcp_server::Effect::ReadOnly,
        ),
        tool(
            "open_project",
            "Open project",
            "Open any folder on this computer as a project, by its absolute path, and add it to the list of folders opened in Bios. A folder that is not a Bios project yet is read from its sequence files and nothing is written into it until something is changed.",
            object(json!({ "path": { "type": "string", "description": "The folder's absolute path." } }), &["path"]),
            craft_mcp_server::Effect::Edits,
        ),
        tool(
            "create_project",
            "Create project",
            "Make a new, empty project (only when the person asks for one): a folder named `name` inside `path`, or inside the usual place for new projects when `path` is left out.",
            object(
                json!({
                    "name": { "type": "string", "description": "The project's name, also used as its folder name." },
                    "path": { "type": "string", "description": "The absolute path of the folder to make it in. Optional." }
                }),
                &["name"],
            ),
            craft_mcp_server::Effect::Edits,
        ),
        tool(
            "rename_project",
            "Rename project",
            "Rename a project (only when the person asks). Its folder is renamed where it is, so use the new name as `project` afterwards.",
            object(
                json!({ "project": project, "new_name": { "type": "string", "description": "The new name, also used as its folder name." } }),
                &["project", "new_name"],
            ),
            craft_mcp_server::Effect::Edits,
        ),
        tool(
            "delete_project",
            "Delete project",
            "Move a whole project folder to the system Trash (only when the person asks, and after they confirm). The person can put it back from the Trash.",
            object(
                json!({ "project": project, "confirm": { "type": "boolean", "description": "Must be true: the person has said yes." } }),
                &["project", "confirm"],
            ),
            craft_mcp_server::Effect::Files,
        ),
        tool(
            "remove_recent_project",
            "Remove project from the list",
            "Take a project off the list of folders opened in Bios. The folder and everything in it stay where they are.",
            object(json!({ "project": project }), &["project"]),
            craft_mcp_server::Effect::Edits,
        ),
    ]
}

fn reply(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(mut result) => {
            // A GenBank file is easier to read as text than as one escaped JSON string.
            let genbank = result.as_object_mut().and_then(|map| map.remove("genbank")).and_then(|v| v.as_str().map(str::to_owned));
            let mut reply = text_result(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()), false);
            if let (Some(text), Some(content)) = (genbank, reply.get_mut("content").and_then(Value::as_array_mut)) {
                content.push(json!({ "type": "text", "text": text }));
            }
            reply
        }
        Err(message) => text_result(message, true),
    }
}

impl Tools for BiosTools {
    fn instructions(&self) -> String {
        match &self.scope {
            Scope::One(session) => format!("{ONE_PROJECT}\n\n{INSTRUCTIONS}\n\nThis project is \"{}\".", session.project().name()),
            Scope::Many(many) => match &many.source {
                Source::Folder(root) => format!("{HOME}\n\n{INSTRUCTIONS}\n\nThe folder of projects is {}.", root.display()),
                Source::Recent(_) => format!("{RECENT}\n\n{INSTRUCTIONS}"),
            },
        }
    }

    fn list(&mut self) -> Vec<Value> {
        match &self.scope {
            Scope::One(_) => bios_engine::registry().map(definition).collect(),
            Scope::Many(_) => home_tools()
                .into_iter()
                .chain(bios_engine::registry().map(|spec| if spec.needs_project() { in_a_project(spec) } else { definition(spec) }))
                .collect(),
        }
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        reply(match &mut self.scope {
            Scope::One(session) => session.execute(name, args).map_err(|failure| failure.0),
            Scope::Many(many) => many.dispatch(name, args),
        })
    }
}

/// Serves `tools` over newline-delimited JSON-RPC until `input` closes.
pub fn serve(tools: BiosTools, input: impl BufRead, output: impl Write) -> std::io::Result<()> {
    let info = Info { name: "bios", title: "Bios", version: env!("CARGO_PKG_VERSION"), max_line: MAX_LINE };
    Server::new(info, tools).serve(input, output)
}
