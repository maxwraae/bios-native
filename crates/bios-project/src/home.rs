//! Project folders as the projects page and an agent see them: a card for each, and making,
//! renaming and deleting a project's folder wherever it is. Also a folder of projects (a folder
//! whose subfolders are projects), which `bios-cli mcp <folder>` can serve whole.
//!
//! A deleted project goes to the system's Trash, from where the person can put it back.

use std::fs;
use std::path::{Path, PathBuf};

use bios_core::Construct;

use crate::project::{Error, Project};

/// One project as the projects page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// The project's folder name inside the folder of projects.
    pub folder: String,
    pub root: PathBuf,
    /// The id in its `bios.json`: what a pin or a hand-made order remembers.
    pub id: String,
    pub name: String,
    pub plasmids: usize,
    /// When anything in it was last written, in UTC, as RFC 3339.
    pub last_changed: Option<String>,
    /// The construct the card's map draws: the first one put into the project, as the web's card
    /// draws the first construct of its session.
    pub lead: Option<Construct>,
}

pub struct Home {
    root: PathBuf,
}

/// A name that can also be a folder name: not empty, no separators, not hidden.
pub fn folder_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', ':']) || name.len() > 200 {
        return Err(Error::Unwritable(format!(
            "\"{name}\" cannot be a project's folder name: it must not be empty, hold / \\ or :, or start with a dot"
        )));
    }
    Ok(name)
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Io(format!("{}: {e}", path.display()))
}

/// A new, empty project in a folder named `name` inside `parent`.
pub fn create_in(parent: &Path, name: &str) -> Result<Project, Error> {
    let name = folder_name(name)?;
    let root = parent.join(name);
    if root.exists() {
        return Err(Error::NameTaken(root));
    }
    Project::create(&root, name)
}

/// Renames a project's folder where it is, and the name in its `bios.json` when it has one, so
/// the name a person sees and the folder an agent names it by stay one. Returns where it is now.
pub fn rename_folder(root: &Path, to: &str) -> Result<PathBuf, Error> {
    let to = folder_name(to)?;
    let parent = root.parent().ok_or_else(|| Error::Unwritable(format!("{} cannot be renamed", root.display())))?;
    let to_root = parent.join(to);
    let current = root.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    // A change of case alone is a rename on a file system that ignores case, not a clash.
    if to_root.exists() && !current.eq_ignore_ascii_case(to) {
        return Err(Error::NameTaken(to_root));
    }
    if Project::is_project(root) {
        Project::open_saved(root)?.rename(to)?;
    }
    if current != to {
        fs::rename(root, &to_root).map_err(|e| io(&to_root, e))?;
    }
    Ok(to_root)
}

/// Moves a project's folder, whole, to the system's Trash, from where the person can put it back.
pub fn trash_folder(root: &Path) -> Result<(), Error> {
    if !root.is_dir() {
        return Err(Error::Io(format!("{} is not there", root.display())));
    }
    #[allow(unused_mut)]
    let mut bin = trash::TrashContext::default();
    // The file manager's own call needs no permission to drive the Finder.
    #[cfg(target_os = "macos")]
    trash::macos::TrashContextExtMacos::set_delete_method(&mut bin, trash::macos::DeleteMethod::NsFileManager);
    bin.delete(root).map_err(|e| Error::Io(format!("{} could not be moved to the Trash: {e}", root.display())))
}

/// `whenOf`: "today", "yesterday", then the date ("Nov 3"), for a time in RFC 3339.
pub fn when_of(iso: &str) -> String {
    when_between(iso, chrono::Utc::now())
}

/// [`when_of`] as seen at `now`.
pub fn when_between(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(then) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let days = (now - then.with_timezone(&chrono::Utc)).num_milliseconds().div_euclid(86_400_000);
    match days {
        ..=0 => "today".into(),
        1 => "yesterday".into(),
        _ => then.with_timezone(&chrono::Local).format("%b %-d").to_string(),
    }
}

/// `countOf`: "1 construct", "4 constructs".
pub fn count_of(n: usize) -> String {
    if n == 1 { "1 construct".into() } else { format!("{n} constructs") }
}

fn rfc3339(time: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The card for one project folder. Reading it can record a change made by another program, as
/// opening the project would.
pub fn card(project: &Project) -> Card {
    let folder = project.root().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let listing = project.list_with_constructs().unwrap_or_default();
    // The oldest construct leads. One never written down was found just now, so it sorts as now.
    let now = crate::project::now();
    let first_at = |id: &crate::ConstructId| project.first_written(id).ok().flatten().unwrap_or_else(|| now.clone());
    let lead = listing
        .iter()
        .map(|(l, c)| (first_at(&l.id), l, c))
        .min_by(|(a, la, _), (b, lb, _)| a.cmp(b).then_with(|| la.name.to_lowercase().cmp(&lb.name.to_lowercase())))
        .map(|(_, _, c)| c.clone());
    Card {
        folder,
        root: project.root().to_path_buf(),
        id: project.id().to_string(),
        name: project.name().to_string(),
        plasmids: listing.len(),
        last_changed: project.last_changed().map(rfc3339),
        lead,
    }
}

impl Home {
    pub fn new(root: &Path) -> Home {
        Home { root: root.to_path_buf() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The projects directly inside the folder, by folder name, ignoring case.
    pub fn projects(&self) -> Result<Vec<(String, Project)>, Error> {
        let entries = fs::read_dir(&self.root).map_err(|e| io(&self.root, e))?;
        let mut found: Vec<(String, Project)> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let folder = e.file_name().to_string_lossy().into_owned();
                if folder.starts_with('.') {
                    return None;
                }
                Project::open_saved(&e.path()).ok().map(|p| (folder, p))
            })
            .collect();
        found.sort_by_key(|(folder, _)| folder.to_lowercase());
        Ok(found)
    }

    /// Every project's card, the one written to last first: the web grid's recency order.
    pub fn cards(&self) -> Result<Vec<Card>, Error> {
        let mut cards: Vec<Card> = self.projects()?.iter().map(|(_, p)| card(p)).collect();
        cards.sort_by(|a, b| b.last_changed.cmp(&a.last_changed).then_with(|| a.folder.to_lowercase().cmp(&b.folder.to_lowercase())));
        Ok(cards)
    }

    /// The project called `name`: its folder name first, then the name inside it, then either
    /// ignoring case.
    pub fn find(&self, name: &str) -> Result<Project, Error> {
        let projects = self.projects()?;
        let wanted = name.trim().trim_end_matches('/');
        let pick = |same: &dyn Fn(&str, &str) -> bool| projects.iter().position(|(folder, p)| same(folder, wanted) || same(p.name(), wanted));
        let exact = |a: &str, b: &str| a == b;
        let loose = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        let names = projects.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ");
        match pick(&exact).or_else(|| pick(&loose)) {
            Some(i) => projects.into_iter().nth(i).map(|(_, p)| p).ok_or_else(|| Error::Unwritable(format!("there is no project called {name}"))),
            None if names.is_empty() => Err(Error::Unwritable(format!("there is no project called \"{name}\"; there are no projects here yet"))),
            None => Err(Error::Unwritable(format!("there is no project called \"{name}\". The projects are: {names}"))),
        }
    }

    /// A new, empty project in a folder named `name`.
    pub fn create(&self, name: &str) -> Result<Project, Error> {
        create_in(&self.root, name)
    }

    /// The first of `name`, `name 2`, `name 3`… that no folder here has yet.
    pub fn free_name(&self, name: &str) -> String {
        let name = name.trim();
        let taken = |candidate: &str| self.root.join(candidate).exists();
        std::iter::once(name.to_string()).chain((2..10_000).map(|n| format!("{name} {n}"))).find(|c| !taken(c)).unwrap_or_else(|| name.to_string())
    }

    /// Renames a project: the name in its `bios.json` and its folder, so that the folder an agent
    /// names it by stays the name a person sees. Returns where it is now.
    pub fn rename(&self, project: &str, to: &str) -> Result<Project, Error> {
        let found = self.find(project)?;
        Project::open(&rename_folder(found.root(), to)?)
    }

    /// Moves a project to the system's Trash, whole. Returns its name.
    pub fn delete(&self, project: &str) -> Result<String, Error> {
        let found = self.find(project)?;
        trash_folder(found.root())?;
        Ok(found.name().to_string())
    }
}
