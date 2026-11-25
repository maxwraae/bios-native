//! The Bios engine: everything Bios can do, as one list of commands.
//!
//! The window, the command line and MCP all call [`Session::execute`] with a command's name and
//! JSON parameters. A command that changes a construct reads it, checks the caller edited the
//! current version, applies the change and saves it through the project, which journals it. So an
//! edit made by a person and an edit made by an agent are the same thing, recorded the same way.
//!
//! Command names and parameters follow the Bios MCP tools, so an agent that knows one knows the
//! other. Positions in parameters and results are 1-based and inclusive, as a person counts them.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod args;
mod commands;
pub mod editor;
// Read mapping: Bios's own read mapper behind the `mapper` feature, and without it a stand-in
// whose commands say read mapping is not in this build.
#[cfg(feature = "mapper")]
pub mod mapping;
#[cfg(not(feature = "mapper"))]
#[path = "no_mapping.rs"]
pub mod mapping;
pub mod picture;
mod refs;
pub mod update;
pub mod verify;

/// One of Bios's own facts about a feature, by its key: `id`, `category`, `use`, `description`.
pub use refs::bios_note;

use std::panic::{AssertUnwindSafe, catch_unwind};

use bios_core::Construct;
use bios_project::{Change, Error, Loaded, Project, Who};
use serde_json::{Map, Value, json};

pub use args::Args;

/// What a parameter holds, for checking a call and for describing the command to an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    Integer,
    /// A number that may have a decimal part: a mass, a volume, a concentration.
    Number,
    Boolean,
    /// A JSON object of text values.
    TextMap,
    /// A list of JSON objects.
    Objects,
    /// A list of text values.
    Strings,
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Looks, changes nothing.
    Read,
    /// Changes the project and is journaled.
    Write,
}

/// One thing Bios can do.
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    pub effect: Effect,
    run: Run,
}

/// How a command runs.
pub(crate) enum Run {
    /// Anything that is not one change to one construct: looking, importing, going back.
    Call(fn(&Session, &Args) -> Result<Value, Failure>),
    /// One change to one construct in memory. The engine reads the construct `plasmid` names,
    /// checks `version`, applies the change and saves it, so every edit is checked and journaled
    /// the same way, and several can be applied one after another and saved once.
    Edit(EditFn),
    /// An edit that does not depend on positions, such as renaming a feature found by name. It
    /// needs no `version`: it is applied to the construct as it is now.
    EditLatest(EditFn),
    /// An edit that needs a `version` only for some of its parameters: moving a feature does,
    /// renaming it does not. The first function says whether this call needs one.
    EditWhen(fn(&Args) -> bool, EditFn),
}

pub(crate) type EditFn = fn(&Session, &Args, &mut Construct) -> Result<Applied, Failure>;

impl CommandSpec {
    /// The command's change to one construct, when that is what it is.
    pub(crate) fn edit_fn(&self) -> Option<EditFn> {
        match self.run {
            Run::Edit(f) | Run::EditLatest(f) | Run::EditWhen(_, f) => Some(f),
            Run::Call(_) => None,
        }
    }

    /// Whether the command needs a project at all. The ones that act on Bios itself
    /// (`check_for_update`, `update_bios`, `what_is_open`) do not, and run in [`Session::for_app`].
    pub fn needs_project(&self) -> bool {
        !commands::is_app_wide(self.name)
    }
}

/// Why a command did nothing, in words the caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure(e.to_string())
    }
}

pub(crate) fn fail<T>(message: impl Into<String>) -> Result<T, Failure> {
    Err(Failure(message.into()))
}

/// Every command, in the order they are listed to a person or an agent.
pub fn registry() -> impl Iterator<Item = &'static CommandSpec> {
    commands::ALL.iter().flat_map(|group| group.iter())
}

pub fn command(name: &str) -> Option<&'static CommandSpec> {
    registry().find(|c| c.name == name)
}

/// One caller working in one project.
pub struct Session {
    project: Project,
    who: Who,
    server: Option<Server>,
    /// The read mapper Verify's commands call.
    mapper: std::sync::Arc<dyn verify::ReadMapper>,
    /// How the command running now reports progress and learns to stop (`execute_with`).
    control: std::sync::Mutex<verify::Control>,
    /// What `check_for_update` and `update_bios` ask, in place of the one the program set up.
    updater: Option<std::sync::Arc<dyn update::Updater>>,
    /// What `draw_plasmid` draws with, in place of the one the program set up.
    painter: Option<std::sync::Arc<dyn picture::Painter>>,
    /// Where the window writes down what it shows (`what_is_open`), in place of the one in Bios's
    /// settings folder.
    window_file: Option<std::path::PathBuf>,
}

/// The Bios server this session may share projects through.
pub struct Server {
    /// What the server goes by in sentences shown to a person, such as its host name.
    pub name: String,
    pub gateway: Box<dyn bios_sync::Gateway>,
}

/// What an edit did, beyond changing the construct.
#[derive(Default)]
pub(crate) struct Applied {
    /// One sentence for the journal.
    pub summary: String,
    /// What else changed that the caller did not ask for by name, such as features that moved.
    pub notes: Vec<String>,
    /// Extra fields for the command's result.
    pub result: Map<String, Value>,
}

/// The server a person set up for this run of Bios, read through `get` (the environment, in a
/// real run): `BIOS_SERVER` is its address, and `BIOS_SERVER_TOKEN` or `BIOS_SERVER_KEY` what it
/// gave them to sign in with. `None` when no address is set: Bios then works on its own, as it
/// does for anyone who shares nothing.
pub fn server_from(get: impl Fn(&str) -> Option<String>) -> Option<Server> {
    let set = |key: &str| get(key).map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let address = set("BIOS_SERVER")?;
    // The host is what the server goes by in what is shown to a person.
    let after_scheme = address.split_once("://").map_or(address.as_str(), |(_, rest)| rest);
    let name = after_scheme.split(['/', ':']).next().unwrap_or(after_scheme).to_string();
    let gateway = bios_sync::HttpGateway::new(&address, set("BIOS_SERVER_KEY"), set("BIOS_SERVER_TOKEN"));
    Some(Server { name, gateway: Box::new(gateway) })
}

impl Session {
    /// A session on `project` for `who`, sharing through the server set up in the environment
    /// when there is one.
    pub fn open(project: Project, who: Who) -> Self {
        let session = Session::new(project, who);
        match server_from(|key| std::env::var(key).ok()) {
            Some(server) => session.with_server(server),
            None => session,
        }
    }

    pub fn new(project: Project, who: Who) -> Self {
        Session {
            project,
            who,
            server: None,
            mapper: verify::default_mapper(),
            control: std::sync::Mutex::new(verify::Control::default()),
            updater: None,
            painter: None,
            window_file: None,
        }
    }

    /// A session for the commands that act on Bios itself rather than on a project
    /// ([`CommandSpec::needs_project`]), for a caller that has no project open. It stands on the
    /// system's temporary folder, which those commands never read or write; opening a folder
    /// writes nothing into it.
    pub fn for_app(who: Who) -> Result<Self, Failure> {
        Ok(Session::new(Project::open(&std::env::temp_dir())?, who))
    }

    /// The same session, asking `updater` about Bios's releases instead of the one set up for the
    /// whole program ([`update::set_updater`]).
    pub fn with_updater(mut self, updater: std::sync::Arc<dyn update::Updater>) -> Self {
        self.updater = Some(updater);
        self
    }

    /// The same session, drawing with `painter` instead of the one set up for the whole program
    /// ([`picture::set_painter`]).
    pub fn with_painter(mut self, painter: std::sync::Arc<dyn picture::Painter>) -> Self {
        self.painter = Some(painter);
        self
    }

    pub(crate) fn painter(&self) -> Option<std::sync::Arc<dyn picture::Painter>> {
        self.painter.clone().or_else(picture::installed)
    }

    /// The same session, reading what the window shows from `file` instead of the one in Bios's
    /// settings folder ([`bios_project::window::standard_file`]).
    pub fn with_window_file(mut self, file: std::path::PathBuf) -> Self {
        self.window_file = Some(file);
        self
    }

    pub(crate) fn window_file(&self) -> Option<std::path::PathBuf> {
        self.window_file.clone().or_else(bios_project::window::standard_file)
    }

    pub(crate) fn updater(&self) -> Option<std::sync::Arc<dyn update::Updater>> {
        self.updater.clone().or_else(update::installed)
    }

    /// The same session, aligning reads with `mapper`.
    pub fn with_mapper(mut self, mapper: std::sync::Arc<dyn verify::ReadMapper>) -> Self {
        self.mapper = mapper;
        self
    }

    pub(crate) fn mapper(&self) -> std::sync::Arc<dyn verify::ReadMapper> {
        self.mapper.clone()
    }

    /// How the command running now reports progress and learns to stop.
    pub(crate) fn control(&self) -> verify::Control {
        self.control.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Runs a command as [`Session::execute`] does, reporting through `control` and stopping when
    /// it is cancelled. For the long ones, such as `run_verification`, run off the window's thread.
    pub fn execute_with(&self, name: &str, params: &Value, control: verify::Control) -> Result<Value, Failure> {
        if let Ok(mut slot) = self.control.lock() {
            *slot = control;
        }
        let result = self.execute(name, params);
        if let Ok(mut slot) = self.control.lock() {
            *slot = verify::Control::default();
        }
        result
    }

    /// The same session, able to share its project through `server`.
    pub fn with_server(mut self, server: Server) -> Self {
        self.server = Some(server);
        self
    }

    pub fn server(&self) -> Option<&Server> {
        self.server.as_ref()
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub(crate) fn who(&self) -> &Who {
        &self.who
    }

    /// Runs the command `name` with `params`, a JSON object.
    pub fn execute(&self, name: &str, params: &Value) -> Result<Value, Failure> {
        let Some(spec) = command(name) else {
            return fail(format!("there is no command called {name}"));
        };
        let args = Args::check(spec, params)?;
        // The engine's own code does not panic, but a command must never take the window or an
        // agent's connection down with it, whatever a dependency does.
        let run = || match spec.run {
            Run::Call(call) => call(self, &args),
            Run::Edit(change) => self.edit(spec.name, &args, true, |c| change(self, &args, c)),
            Run::EditLatest(change) => self.edit(spec.name, &args, false, |c| change(self, &args, c)),
            Run::EditWhen(needs_version, change) => self.edit(spec.name, &args, needs_version(&args), |c| change(self, &args, c)),
        };
        match catch_unwind(AssertUnwindSafe(run)) {
            Ok(result) => result,
            Err(_) => fail(format!("{name} failed inside Bios and changed nothing; this is a bug in Bios")),
        }
    }

    /// Reads the construct `args` names, applies `change` and saves the result as its next version.
    ///
    /// With `needs_version` the caller must pass the `version` it read; if the construct has
    /// moved on, nothing is applied and the caller reads again. Without it the change is applied
    /// to the construct as it is now, and tried again if someone else saves in between.
    pub(crate) fn edit(
        &self,
        command: &str,
        args: &Args,
        needs_version: bool,
        change: impl Fn(&mut Construct) -> Result<Applied, Failure>,
    ) -> Result<Value, Failure> {
        for _ in 0..4 {
            let loaded = self.load(args)?;
            let current = token(&loaded);
            let label = refs::label(&loaded.path);
            let based = args.opt_str("version")?;
            if needs_version {
                let Some(based) = &based else {
                    return fail(format!("version is required for this edit: call read_plasmid first (current version: {current})"));
                };
                if sha_of_token(based) != loaded.sha {
                    return fail(conflict(&label, based, &current));
                }
            }

            let mut construct = loaded.construct.clone();
            let applied = change(&mut construct)?;
            if construct == loaded.construct {
                return Ok(json!({ "plasmid": label, "version": current, "unchanged": true }));
            }

            let record = Change { command: command.to_string(), params: journal_params(args.raw()), summary: applied.summary };
            let saved = match self.project.save(&loaded.id, &loaded.sha, &construct, &self.who, record) {
                Ok(saved) => saved,
                // Someone saved between the read and the write. An edit that named its version
                // is refused; one that did not is simply made again on what is there now.
                Err(Error::Conflict { .. }) if needs_version => {
                    return fail(conflict(&label, based.as_deref().unwrap_or_default(), "a newer version"));
                }
                Err(Error::Conflict { .. }) => continue,
                Err(e) => return Err(e.into()),
            };
            let mut out = Map::new();
            out.insert("saved".into(), json!(true));
            out.insert("plasmid".into(), json!(refs::label(&saved.path)));
            out.insert("version".into(), json!(token(&saved)));
            out.insert("previous_version".into(), json!(current));
            if !applied.notes.is_empty() {
                out.insert("changes".into(), json!(applied.notes));
            }
            out.extend(applied.result);
            return Ok(Value::Object(out));
        }
        fail("this plasmid kept changing while the edit was being made; nothing was saved. Try again.")
    }

    /// The construct that `args.plasmid` names.
    pub(crate) fn load(&self, args: &Args) -> Result<Loaded, Failure> {
        let listed = self.project.list()?;
        let id = refs::plasmid(&listed, &args.str("plasmid")?)?.id.clone();
        Ok(self.project.read(&id)?)
    }
}

/// What a caller passes back as `version` to show which version it edited.
/// A command's parameters as the history keeps them. A whole GenBank text passed in is left
/// out: the version it made holds exactly that text, and a second copy on every save would make
/// the history several times the size of the work.
fn journal_params(params: &Value) -> Value {
    let mut kept = params.clone();
    if let Some(text) = kept.get_mut("genbank").filter(|v| v.is_string()) {
        *text = json!("(the text of the version this made)");
    }
    kept
}

pub(crate) fn token(loaded: &Loaded) -> String {
    format!("{}:{}", loaded.version, loaded.sha)
}

/// The part of a version token that says what the text is. The number in front is for people;
/// a move or a rename changes it without changing a base, and must not turn into a conflict.
fn sha_of_token(token: &str) -> &str {
    token.rsplit(':').next().unwrap_or(token)
}

fn conflict(plasmid: &str, based: &str, current: &str) -> String {
    format!(
        "CONFLICT: nothing was saved. \"{plasmid}\" changed since version {based}; it is now {current}. Someone else, a person or another agent, edited it. Read it again with read_plasmid and redo the edit on the new version."
    )
}
