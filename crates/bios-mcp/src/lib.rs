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
use bios_project::window;
use bios_project::{Actor, Project, Who};
use craft_mcp_server::{Info, Server, Tools, object, text_result, tool};
use serde_json::{Map, Value, json};

/// The longest request line read. A whole GenBank file can arrive in one call.
const MAX_LINE: usize = 64 * 1024 * 1024;

const ONE_PROJECT: &str = "\
Bios is a workspace for designing DNA. You are working inside one project: a set of plasmids, in folders, each with its full history.";

const HOME: &str = "\
Bios is a workspace for designing DNA. You are working in a folder of Bios projects; each project is a set of plasmids, in folders, each with its full history.

Start with list_projects. Every other tool takes `project`: the project's folder name as list_projects shows it (its name works too), or any folder's absolute path. A plasmid can also be written as \"Project/path\" with no `project`. Tools that take a second plasmid (from_plasmid, from) look for it in the same project. Bios works on folders where they are: when the person's files are in a folder (iCloud, Google Drive, OneDrive, anywhere), open_project that folder and work there; never make a new project and import copies of them. Make a new project with create_project only when the person asks for one, in the folder they want it in. To put a project or plasmid in front of the person in the Bios window (\"show me\", \"open it\"), call show_in_bios.";

const RECENT: &str = "\
Bios is a workspace for designing DNA. A project is any folder on this computer: its plasmids are the sequence files in it (GenBank, FASTA, SnapGene, EMBL), in subfolders, each with its full history once Bios has changed it. Bios keeps a list of the folders the person has opened, as a code editor does.

Start with list_projects: the folders opened in Bios, with each one's path. Every other tool takes `project`: a folder's name as list_projects shows it, or any folder's absolute path (that folder is opened and joins the list, as open_project does). Opening or reading a folder writes nothing into it; the first change makes it a Bios project (a bios.json and a hidden .bios folder beside its files). A plasmid can also be written as \"Project/path\" with no `project`. Tools that take a second plasmid (from_plasmid, from) look for it in the same project. Bios works on folders where they are: when the person's files are in a folder (iCloud, Google Drive, OneDrive, anywhere), open_project that folder and work there; never make a new project and import copies of them. Make a new project with create_project only when the person asks for one, in the folder they want it in. To put a project or plasmid in front of the person in the Bios window (\"show me\", \"open it\"), call show_in_bios.";

const DEFAULT_PROJECT: &str = "\
When you leave `project` out and the plasmid is not written as \"Project/path\", the tool works in the project open in the Bios window (what_is_open names it). Pass `project` to work anywhere else; it always wins.";

const INSTRUCTIONS: &str = "\
How to work here:
- The person works in the Bios window beside you. Call what_is_open first, and again whenever they refer to what is on their screen (\"this plasmid\", \"the open one\", \"here\", \"the selection\"): it names the project, the plasmid in front, the open tabs and the selected bases. \"This plasmid\" means its `plasmid`. If no window is open, ask which plasmid they mean.
- Find plasmids with list_plasmids or find_plasmid. Address a plasmid by its path (\"backbones/pUC19\"), its name if unique, or its id.
- Read before you edit. read_plasmid returns a `version`; every edit needs the version it was made against. If the plasmid changed in the meantime the edit is refused with CONFLICT and nothing is saved: read it again and redo the edit.
- Positions are 1-based and inclusive, as a biologist counts them. On a circular plasmid start > end runs through the origin.
- Do the DNA maths with these tools, never in your head and never with an outside script or library. Melting temperature and GC (sequence_stats), translation, open reading frames, restriction sites and digests, primer design and checking, PCR, Gibson and type IIS assembly, comparing two plasmids, quantities to weigh out: they are all here, and their answers are checked against recorded reference answers. Melting temperature is the nearest-neighbour estimate (SantaLucia 1998, 250 nM strands, 50 mM Na+) everywhere in Bios, so a figure you arrive at another way will disagree with what the person sees in the window. If a tool you need seems to be missing, say so and ask; do not put your own calculation in its place.
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
    /// Where the Bios window writes down what it shows (`what_is_open`): beside the list of
    /// opened folders, or in Bios's settings folder.
    window: Option<PathBuf>,
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

/// A path written from the person's home folder ("~/Lab") as the absolute path it means.
fn expand_home(path: &str) -> String {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        _ => path.to_string(),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

impl Many {
    fn new(source: Source, who: Who) -> Many {
        let window = match &source {
            Source::Recent(recents) => recents.file().map(window::file_beside),
            Source::Folder(_) => window::standard_file(),
        };
        Many { source, who, sessions: HashMap::new(), app: None, window }
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
        let path = text_arg(args, "path").map(expand_home);
        let parent = match (path.as_deref(), &self.source) {
            (Some(path), _) if Path::new(path).is_absolute() => PathBuf::from(path),
            (Some(path), _) => return Err(format!("create_project's `path` must be an absolute folder path; {path} is not one")),
            (None, Source::Folder(root)) => root.clone(),
            (None, Source::Recent(_)) => recents::default_location().ok_or("say where to make it with `path`")?,
        };
        // A folder that is already there is the person's: open it where it is, as Bios opens any.
        if let Ok(folder) = bios_project::home::folder_name(name)
            && parent.join(folder).is_dir()
        {
            let project = self.open_path(&parent.join(folder))?;
            let root = project.root().to_path_buf();
            return Ok(json!({ "project": label(&root), "name": project.name(), "path": root.display().to_string(), "opened_existing": true,
                "note": "this folder was already there, so it is opened as it is; nothing was made or copied" }));
        }
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
            let mut app = Session::for_app(self.who.clone()).map_err(|failure| failure.0)?;
            if let Some(file) = &self.window {
                app = app.with_window_file(file.clone());
            }
            self.app = Some(Box::new(app));
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
        if let Some(front) = self.window_project(&projects) {
            return Ok(front);
        }
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

impl Many {
    /// The project open in the Bios window, when a window is running and that project is one this
    /// connection serves: what a call that names no project means.
    fn window_project(&self, projects: &[(String, Project)]) -> Option<Project> {
        let shown = window::live(self.window.as_deref()?)?.front.project?;
        let same = |root: &Path| root == shown.path || root.canonicalize().ok().is_some_and(|r| Some(r) == shown.path.canonicalize().ok());
        let (_, found) = projects.iter().find(|(_, p)| same(p.root()))?;
        Project::open(found.root()).ok()
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
            Kind::Number => json!({ "type": "number" }),
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
    let project = json!({ "type": "string", "description": "The project to work in: its folder name as list_projects shows it, or a folder's absolute path. May be left out when the plasmid is written as \"Project/path\", or to work in the project open in the Bios window." });
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
            "Open any folder on this computer as a project, by its absolute path, and add it to the list of folders opened in Bios. This is how to work in a folder the person already has, wherever it is (iCloud, Google Drive, OneDrive, Dropbox, a shared drive): Bios works on the files where they are and copies nothing, and the person sees the same folder in the Bios window. A folder that is not a Bios project yet is read from its sequence files and nothing is written into it until something is changed.",
            object(json!({ "path": { "type": "string", "description": "The folder's absolute path." } }), &["path"]),
            craft_mcp_server::Effect::Edits,
        ),
        tool(
            "create_project",
            "Create project",
            "Make a new, empty project folder (only when the person asks for one): a folder named `name` inside the folder `path`, which can be anywhere on this computer and is made if it does not exist yet. When the person's files already sit in a folder, do not make a project and import copies of them: open that folder with open_project. If `path`/`name` already exists it is opened as it is, nothing copied. Put a new folder where the person says. When they have not said and their other projects sit in different places, ask where it should go; left out, it goes in the usual place for new projects (~/Documents/Bios projects when that exists). The answer gives the new project's full path.",
            object(
                json!({
                    "name": { "type": "string", "description": "The project's name, also used as its folder name." },
                    "path": { "type": "string", "description": "The folder to make it in, as an absolute path or starting with ~ for the home folder (\"~/Desktop/Lab\"). The project becomes <path>/<name>. Optional." }
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

/// A picture larger than this is left on disk to be opened, not carried in the answer. Big
/// enough for any picture `draw_plasmid` draws by default; small enough that one call cannot
/// flood the conversation it goes back to.
const MOST_INLINE_BYTES: u64 = 3 * 1024 * 1024;

/// The PNG a command drew, as the connection should carry it back, if it drew one.
///
/// An agent reaches Bios over a pipe and may have no way to open a file on this computer, so a
/// picture rides home inside the answer itself: MCP carries an image beside the text, and the
/// agent sees it the way a person sees it on screen. The path is still in the text, for an agent
/// that would rather open the file or hand it to someone.
fn drawn_image(result: &Value) -> Option<Value> {
    let path = result.get("path")?.as_str()?;
    if result.get("format")?.as_str()? != "png" || !path.to_lowercase().ends_with(".png") {
        return None;
    }
    if std::fs::metadata(path).ok()?.len() > MOST_INLINE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    Some(json!({ "type": "image", "data": data, "mimeType": "image/png" }))
}

fn reply(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(mut result) => {
            // A GenBank file is easier to read as text than as one escaped JSON string.
            let genbank = result.as_object_mut().and_then(|map| map.remove("genbank")).and_then(|v| v.as_str().map(str::to_owned));
            let image = drawn_image(&result);
            let mut reply = text_result(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()), false);
            if let Some(content) = reply.get_mut("content").and_then(Value::as_array_mut) {
                if let Some(text) = genbank {
                    content.push(json!({ "type": "text", "text": text }));
                }
                if let Some(image) = image {
                    content.push(image);
                }
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
                Source::Folder(root) => format!("{HOME}\n\n{DEFAULT_PROJECT}\n\n{INSTRUCTIONS}\n\nThe folder of projects is {}.", root.display()),
                Source::Recent(_) => format!("{RECENT}\n\n{DEFAULT_PROJECT}\n\n{INSTRUCTIONS}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest possible PNG: the 8-byte signature is all `drawn_image` reads of it.
    fn png(dir: &std::path::Path, name: &str, bytes: usize) -> String {
        let path = dir.join(name);
        std::fs::write(&path, vec![0x89u8; bytes]).unwrap();
        path.display().to_string()
    }

    #[test]
    fn a_png_rides_home_in_the_answer_and_an_oversized_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let small = png(dir.path(), "map.png", 12);
        let drawn = drawn_image(&json!({ "path": small, "format": "png" })).expect("a picture goes back with the answer");
        assert_eq!(drawn["type"], "image");
        assert_eq!(drawn["mimeType"], "image/png");
        // Twelve 0x89 bytes, base64'd.
        assert_eq!(drawn["data"], "iYmJiYmJiYmJiYmJ");

        // Too big to carry: the answer still names the file, so it can be opened instead.
        let huge = png(dir.path(), "huge.png", usize::try_from(MOST_INLINE_BYTES).unwrap() + 1);
        assert_eq!(drawn_image(&json!({ "path": huge, "format": "png" })), None);

        // Not a picture, or not one an agent can look at.
        assert_eq!(drawn_image(&json!({ "path": small, "format": "svg" })), None);
        assert_eq!(drawn_image(&json!({ "path": dir.path().join("gone.png").display().to_string(), "format": "png" })), None);
        assert_eq!(drawn_image(&json!({ "plasmid": "pUC19" })), None);
    }
}
