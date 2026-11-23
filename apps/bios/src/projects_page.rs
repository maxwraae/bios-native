//! The projects page, the window's front page when no project is open: the folders opened in
//! Bios lately, wherever they are, as cards (web: `projects/ProjectsHome.tsx`), with tiles to make
//! a new project or open a folder, as a code editor's start page lists the folders it opened.
//!
//! The list is `bios_project::recents`, the same one `bios-cli mcp` serves an agent; what a card
//! does goes through `bios_project::home`, the same code an agent's `rename_project` and
//! `delete_project` run. Pins and the order made by hand are kept with the list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use bios_layout::projects::{self, Filter};
use bios_project::Project;
use bios_project::home;
use bios_ui::projects_home::{self, Card, Chosen, ProjectsHome};
use egui::Rect;

use crate::{App, POLL};

/// The page's state between frames.
#[derive(Default)]
pub struct Page {
    pub home: ProjectsHome,
    pub filter: Filter,
    /// Each card with the project folder it is for, in the page's order.
    cards: Vec<(Card, PathBuf)>,
    /// Every card read so far, by folder: what `cards` is arranged from.
    by_root: HashMap<PathBuf, Card>,
    /// Reads the cards away from the window, so the page never waits on a disk.
    worker: Option<Worker>,
    /// A look at the projects was asked for and has not been sent to the worker yet.
    wanted: bool,
    /// The worker's projects, for tests, where its look runs on the test's own thread.
    #[cfg(test)]
    known_here: Known,
    polled: Option<Instant>,
}

impl Page {
    /// The cards are being read and the page does not show them all yet.
    pub fn loading(&self) -> bool {
        self.wanted || self.worker.as_ref().is_some_and(|w| w.busy)
    }
}

/// What the worker says: one project's card, or that it has looked at every project it was sent.
enum Said {
    Card(PathBuf, Card),
    Gone(PathBuf),
    Done,
}

/// The projects the worker keeps open, each with the stamp and name its card was made at.
type Known = HashMap<PathBuf, (Project, Option<(u64, String)>)>;

struct Worker {
    send: Sender<Vec<PathBuf>>,
    receive: Receiver<Said>,
    busy: bool,
}

impl Worker {
    fn start(ctx: egui::Context) -> Worker {
        let (send, asked) = channel::<Vec<PathBuf>>();
        let (tell, receive) = channel();
        std::thread::spawn(move || {
            // Each project stays open between looks, so a file that has not changed is not read
            // again; a card is made again only when its project's stamp moved.
            let mut known: Known = HashMap::new();
            while let Ok(mut roots) = asked.recv() {
                // Only the latest look matters.
                while let Ok(later) = asked.try_recv() {
                    roots = later;
                }
                known.retain(|root, _| roots.contains(root));
                for root in &roots {
                    let said = look(&mut known, root);
                    // The window may have closed; then there is no one to tell.
                    if let Some(said) = said
                        && tell.send(said).is_err()
                    {
                        return;
                    }
                }
                if tell.send(Said::Done).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        Worker { send, receive, busy: false }
    }
}

/// One project's card when it changed since it was last looked at.
fn look(known: &mut Known, root: &Path) -> Option<Said> {
    let Ok(fresh) = Project::open(root) else {
        return known.remove(root).map(|_| Said::Gone(root.to_path_buf()));
    };
    // Opening reads only `bios.json`. The project kept from before is what holds the files
    // already read; it is replaced only when it is new or was renamed since.
    let renamed = known.get(root).is_none_or(|(kept, _)| kept.name() != fresh.name());
    if renamed {
        known.insert(root.to_path_buf(), (fresh, None));
    }
    let entry = known.get_mut(root)?;
    let stamp = |p: &Project| (p.stamp(), p.name().to_string());
    let now = stamp(&entry.0);
    if entry.1.as_ref() == Some(&now) {
        return None;
    }
    let card = card_of(&home::card(&entry.0));
    // Reading may itself have recorded a change made by another program.
    entry.1 = Some(stamp(&entry.0));
    Some(Said::Card(root.to_path_buf(), card))
}

/// Where a folder is, short: its parent, with the home folder as `~`.
fn place_of(root: &Path) -> String {
    let parent = root.parent().unwrap_or(root);
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    match home.as_deref().and_then(|h| parent.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => parent.display().to_string(),
    }
}

fn card_of(card: &home::Card) -> Card {
    let when = card.last_changed.as_deref().map(home::when_of).unwrap_or_default();
    Card {
        id: card.root.display().to_string(),
        name: card.name.clone(),
        meta: if when.is_empty() { home::count_of(card.plasmids) } else { format!("{} \u{b7} {when}", home::count_of(card.plasmids)) },
        place: place_of(&card.root),
        thumb: card.lead.as_ref().map(projects_home::thumb),
    }
}

fn paths(ids: Vec<String>) -> Vec<PathBuf> {
    ids.into_iter().map(PathBuf::from).collect()
}

fn ids(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

impl App {
    /// Asks for the cards to be looked at again soon, away from the window.
    pub(crate) fn read_cards(&mut self, _force: bool) {
        self.page.wanted = true;
        // Tests look at the cards straight after asking, so there the worker's look runs here.
        #[cfg(test)]
        self.read_cards_here();
    }

    #[cfg(test)]
    fn read_cards_here(&mut self) {
        let mut known = std::mem::take(&mut self.page.known_here);
        for root in self.recents.folders() {
            match look(&mut known, &root) {
                Some(Said::Card(root, card)) => {
                    self.page.by_root.insert(root, card);
                }
                Some(Said::Gone(root)) => {
                    self.page.by_root.remove(&root);
                }
                Some(Said::Done) | None => {}
            }
        }
        self.page.known_here = known;
        self.page.wanted = false;
        self.arrange_cards();
    }

    /// Takes in what the worker read, arranges the cards, and sends it another look when one is
    /// due: when asked for, and twice a second while the page shows.
    fn tend_cards(&mut self, ctx: &egui::Context) {
        let worker = self.page.worker.get_or_insert_with(|| Worker::start(ctx.clone()));
        let mut arrived = false;
        while let Ok(said) = worker.receive.try_recv() {
            match said {
                Said::Card(root, card) => {
                    self.page.by_root.insert(root, card);
                }
                Said::Gone(root) => {
                    self.page.by_root.remove(&root);
                }
                Said::Done => worker.busy = false,
            }
            arrived = true;
        }
        let due = self.page.wanted || self.page.polled.is_none_or(|at| at.elapsed() >= POLL);
        if due && !worker.busy {
            self.page.wanted = false;
            self.page.polled = Some(Instant::now());
            if worker.send.send(self.recents.folders()).is_ok() {
                worker.busy = true;
            }
        }
        if arrived || due {
            self.arrange_cards();
        }
        ctx.request_repaint_after(POLL);
    }

    /// The latest opened first, as the list keeps them; once arranged by hand, that order, with
    /// any folder the order does not know yet in front, where a new one appears.
    fn arrange_cards(&mut self) {
        let list = self.recents.list();
        // Only folders still there: a project deleted meanwhile loses its card at once.
        let roots: Vec<PathBuf> = self.recents.folders();
        let mut cards: Vec<(Card, PathBuf)> = roots.iter().filter_map(|root| Some((self.page.by_root.get(root)?.clone(), root.clone()))).collect();
        if !list.order.is_empty() {
            cards.sort_by_key(|(_, root)| list.order.iter().position(|p| p == root).map_or(0, |at| at + 1));
        }
        self.page.cards = cards;
    }

    /// The cards, in the page's order.
    pub(crate) fn page_cards(&self) -> Vec<Card> {
        self.page.cards.iter().map(|(c, _)| c.clone()).collect()
    }

    /// The counts on the rail's rows.
    pub(crate) fn page_counts(&self) -> (usize, usize) {
        let ids: Vec<&str> = self.page.cards.iter().map(|(c, _)| c.id.as_str()).collect();
        projects::counts(&ids, &ids_of_pins(self))
    }

    /// Draws the page and does what was asked on it.
    pub(crate) fn projects_page(&mut self, ui: &mut egui::Ui, rect: Rect) {
        self.tend_cards(ui.ctx());
        let list = self.recents.list();
        let pins = ids(&list.pins);
        let cards = self.page_cards();
        let model = projects_home::Model { cards: &cards, pinned: &pins, arranged: !list.order.is_empty(), filter: self.page.filter };
        let chosen = self.page.home.show(ui, rect, &model);
        for chosen in chosen {
            self.page_chose(chosen);
        }
    }

    fn root_of(&self, id: &str) -> Option<PathBuf> {
        self.page.cards.iter().find(|(c, _)| c.id == id).map(|(_, root)| root.clone())
    }

    pub(crate) fn page_chose(&mut self, chosen: Chosen) {
        let remembered = match chosen {
            Chosen::Open(id) => {
                if let Some(root) = self.root_of(&id) {
                    self.open_project(&root);
                }
                // The page is going away; its cards are looked at again when it comes back.
                return;
            }
            Chosen::NewProject => {
                self.command(crate::menus::NEW_PROJECT);
                Ok(())
            }
            Chosen::OpenFolder => {
                self.command(crate::menus::OPEN_FOLDER);
                Ok(())
            }
            Chosen::Rename { id, name } => {
                let Some(root) = self.root_of(&id) else {
                    return;
                };
                match home::rename_folder(&root, &name) {
                    Ok(now) => self.recents.moved(&root, &now),
                    Err(e) => {
                        self.problem = Some(e.to_string());
                        Ok(())
                    }
                }
            }
            Chosen::Remove(id) => {
                let Some(root) = self.root_of(&id) else {
                    return;
                };
                self.notice = Some(format!("Removed from the list. The folder is still at {}.", root.display()));
                self.recents.forget(&root)
            }
            Chosen::Delete(id) => {
                let Some(root) = self.root_of(&id) else {
                    return;
                };
                match home::trash_folder(&root) {
                    Ok(()) => {
                        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        self.notice = Some(format!("{name} is in the Trash. Put it back from there if you need it."));
                        self.recents.forget(&root)
                    }
                    Err(e) => {
                        self.problem = Some(e.to_string());
                        Ok(())
                    }
                }
            }
            Chosen::Pins(pins) => self.recents.set_pins(paths(pins)),
            Chosen::Order(order) => self.recents.set_order(paths(order)),
            Chosen::SortByRecent => self.recents.set_order(Vec::new()),
            Chosen::PickFiles => {
                self.command(crate::menus::IMPORT);
                Ok(())
            }
        };
        if let Err(e) = remembered {
            self.problem = Some(format!("The list of projects could not be saved: {e}"));
        }
        self.read_cards(true);
    }

    /// Files dropped on the page start a project from them: a new project, in the place new
    /// projects are made, named after the first file and holding all of them (`startFromUpload`).
    pub(crate) fn start_from_files(&mut self, files: &[PathBuf]) {
        let Some(place) = self.new_projects_place() else {
            self.problem = Some("There is no folder to make the project in; use File > New Project.".into());
            return;
        };
        let Some(first) = files.first() else {
            return;
        };
        let stem = first.file_stem().map_or_else(|| "Untitled project".to_string(), |s| s.to_string_lossy().into_owned());
        let name = home::Home::new(&place).free_name(stem.trim_start_matches('.'));
        match home::create_in(&place, &name) {
            Ok(project) => {
                let root = project.root().to_path_buf();
                self.open_project(&root);
                for file in files {
                    self.run("import_plasmid", serde_json::json!({ "file": file.display().to_string() }));
                }
                if let Some(id) = self.listing.first().map(|l| l.id.clone()) {
                    self.open_construct(&id);
                }
            }
            Err(e) => self.problem = Some(e.to_string()),
        }
    }
}

fn ids_of_pins(app: &App) -> Vec<String> {
    ids(&app.recents.list().pins)
}
